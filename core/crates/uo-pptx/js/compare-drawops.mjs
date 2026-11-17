// The Rust-list-vs-TS comparison of the Slides port (plan section 4.3, B4's primary gate). Ready for B4, which has the
// producer: for every (deck, slide, view) of a TS capture (capture-drawops.mjs, `.ops.gz`) the list the Rust producer built is
// decoded, painted by `paint-slides.ts` over the recording context, and its draw-form op stream is compared line by line
// with the frozen TS stream.
//
//   node core/crates/uo-pptx/js/compare-drawops.mjs --ts core/crates/uo-pptx/tests/captures/drawops-samples.ops.gz \
//        --rust <dir of .uolf files> [--decks core/crates/uo-pptx/tests/captures/decks] [--form draw|loose] [--eps 1e-9]
//        [--only <key substring>] [--max <n>] [--caps nolayers,...] [--show 5]
//   exit 0 when every compared slide view is equal, 1 otherwise; 2 on a usage or input error
//
// Input of the Rust side (`<dir>/<key with / as __>.uolf`, one file per deck: the key is the capture's `samples/<path>` or
// `gen/<group>/<NNNN>`), written by the B4 producer test tool, little endian:
//   "UOLF" u8 version = 1, then one frame per (slide, view) in the order of the capture (slide ascending, views a then b):
//   u32 listLen, the `SlidesList` message bytes (one stream per deck: the tables grow with each message, the generation
//   is the deck's), u32 nBitmaps, then per bitmap newly added to the tables by this message: u32 len, the RGBA bytes.
// A missing file is reported as `missing` (the deck is not compared); a frame that does not decode as `decode-error`.
// The pictures come from the deck itself: `tables.media[i].part` is looked up in the package, so the image identity (the hash
// of its bytes) is the one the TS capture recorded. Slots are painted static (no frame).
import { existsSync, readFileSync } from 'node:fs';
import { join } from 'node:path';
import { gunzipSync } from 'node:zlib';
import { args, capturesDir, loadSlidesCore, readSample, tsRoot } from './lib/common.mjs';
import { GROUPS, buildGroupDeck, deckFile } from './lib/gen/groups.mjs';
import { compare, form, parseOps } from './lib/opstream.mjs';
import { recordList } from './lib/rust-paint.mjs';
import { parseCaps } from './lib/views.mjs';
import { decodeSlidesList, newTables } from '../../uo-wasm/js/gen/slides.ts';

const a = args();
const tsPath = a.opt('--ts');
const rustDir = a.opt('--rust');
if (!tsPath || !rustDir) {
  console.error('usage: compare-drawops.mjs --ts <capture.ops.gz> --rust <dir of .uolf> [--decks dir] [--form draw|loose] [--eps n] [--only s] [--max n] [--caps list] [--show n]');
  process.exit(2);
}
const which = a.opt('--form', 'draw');
const eps = Number(a.opt('--eps', '0'));
const only = a.opt('--only');
const max = Number(a.opt('--max', '1e9'));
const show = Number(a.opt('--show', '5'));
const caps = parseCaps(a.opt('--caps'));
const decksDir = a.opt('--decks', join(capturesDir, 'decks'));
const core = await loadSlidesCore(tsRoot(a));

const entries = parseOps(gunzipSync(readFileSync(tsPath)).toString('utf8'));
const byDeck = new Map();
for (const e of entries) { if (!byDeck.has(e.deck)) byDeck.set(e.deck, []); byDeck.get(e.deck).push(e); }

function deckBytes(key) {
  if (key.startsWith('samples/')) return readSample(key.slice('samples/'.length));
  const m = /^gen\/([^/]+)\/(\d+)$/.exec(key);
  if (!m) throw new Error(`unknown deck key ${key}`);
  const file = join(decksDir, m[1], deckFile(m[1], Number(m[2])));
  if (existsSync(file)) return new Uint8Array(readFileSync(file));
  if (!GROUPS[m[1]]) throw new Error(`unknown group ${m[1]}`);
  return new Uint8Array(buildGroupDeck(m[1], Number(m[2])).bytes);
}

/** The frames of a `.uolf` file: [{ list: Uint8Array, bitmaps: Uint8Array[] }]. */
function readFrames(buf) {
  if (buf.length < 5 || buf.toString('latin1', 0, 4) !== 'UOLF' || buf[4] !== 1) throw new Error('not a UOLF v1 file');
  const frames = [];
  let p = 5;
  const u32 = () => { const v = buf.readUInt32LE(p); p += 4; return v; };
  while (p < buf.length) {
    const n = u32();
    const list = new Uint8Array(buf.subarray(p, p + n)); p += n;
    const nb = u32();
    const bitmaps = [];
    for (let i = 0; i < nb; i++) {
      const raw = u32();
      const l = raw & 0x7fffffff;
      const bm = new Uint8Array(buf.subarray(p, p + l));
      bm.isRecipe = (raw & 0x80000000) !== 0; // B5b: a metafile recipe, not RGBA
      bitmaps.push(bm);
      p += l;
    }
    frames.push({ list, bitmaps });
  }
  return frames;
}

const stats = { decks: 0, missing: 0, compared: 0, equal: 0, different: 0, decodeErrors: 0, tsErrors: 0, paintErrors: 0 };
let shown = 0;
for (const [key, slides] of byDeck) {
  if (only && !key.includes(only)) continue;
  if (stats.decks >= max) break;
  stats.decks++;
  const file = join(rustDir, `${key.replace(/\//g, '__')}.uolf`);
  if (!existsSync(file)) { stats.missing++; console.log(`missing  ${key}`); continue; }
  const frames = readFrames(readFileSync(file));
  const pkg = core.PptxPackage.load(deckBytes(key));
  const tables = newTables();
  const bitmaps = [];
  slides.forEach((s, i) => {
    const label = `${key} s${s.slide}.${s.view}`;
    const frame = frames[i];
    if (!frame) { stats.decodeErrors++; console.log(`decode-error  ${label}: no frame ${i}`); return; }
    let list;
    try { list = decodeSlidesList(frame.list, tables); } catch (e) { stats.decodeErrors++; console.log(`decode-error  ${label}: ${e?.message ?? e}`); return; }
    for (const b of frame.bitmaps) bitmaps.push(b);
    if (s.error) { stats.tsErrors++; return; }
    const [w, h] = s.header.canvas.split('x').map(Number);
    const r = recordList({
      list, tables, width: w, height: h, caps, bitmap: (idx) => (bitmaps[idx]?.isRecipe ? null : bitmaps[idx]),
      recipe: (idx) => (bitmaps[idx]?.isRecipe ? bitmaps[idx] : null),
      media: (idx) => { const m = tables.media[idx]; return m ? pkg.getPart(m.part) ?? null : null; },
    });
    if (r.error) { stats.paintErrors++; console.log(`paint-error  ${label}: ${r.error}`); }
    stats.compared++;
    const c = compare(form(s.lines, which), r.text, { which, eps });
    if (c.equal) { stats.equal++; return; }
    stats.different++;
    if (shown++ < show) {
      console.log(`DIFFERS  ${label}: first difference at line ${c.index} of ${c.lengths[0]} (TS) / ${c.lengths[1]} (list)\n  TS:   ${c.a}\n  list: ${c.b}`);
    }
  });
}
console.log(`${stats.decks} decks (${stats.missing} missing), ${stats.compared} slide views compared: ${stats.equal} equal, ${stats.different} different; ` +
  `${stats.decodeErrors} decode errors, ${stats.paintErrors} paint errors, ${stats.tsErrors} TS errors skipped; form=${which} eps=${eps}`);
process.exit(stats.different || stats.decodeErrors || stats.paintErrors || stats.missing ? 1 : 0);
