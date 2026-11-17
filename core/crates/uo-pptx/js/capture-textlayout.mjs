// Text-layout capture of the frozen TS (frontier B3, `uo-dml-render.md` "B3 — text layout"): every text body of every
// slide of the committed samples and generated decks goes through `layoutTextBody` of the frozen slides-core with the
// frozen `pptMeasure` (or, for WordArt bodies, the plain measurer of `paintWarp`) over a measuring context whose
// widths are the REAL advance widths of the open fonts (Liberation Sans/Serif/Mono, Carlito, Caladea; every other
// family is measured with its metric-compatible substitute or Liberation Sans, as the TS substitute rule falls back),
// so the line breaking is realistic. Every `measureText` the TS asks is logged; the capture records per call its
// inputs, the digest of the set of measurements asked, and the digest of the canonical JSON of the result. The Rust
// replay (`core/crates/uo-pptx/tests/text_layout.rs`) answers from the log (the `MeasureTable`) and must give the same.
//
//   node core/crates/uo-pptx/js/capture-textlayout.mjs --samples
//   node core/crates/uo-pptx/js/capture-textlayout.mjs --gen b1a --decks core/crates/uo-pptx/tests/captures/decks
//   node core/crates/uo-pptx/js/capture-textlayout.mjs --gen b3a --decks core/crates/uo-pptx/tests/captures/decks
//   options: --out <stem>      writes <stem>.capture and <stem>.measures.tsv.gz (default tests/captures/textlayout-<name>)
//            --caps nokerning,noseg   capability set (`'fontKerning' in ctx` absent; no `Intl.Segmenter`)
//            --detail <file.json.gz>  the canonical JSON of every result (local diffing)
//            --only <s>  --max <n>  --ts-root <dir>
//
// Calls per slide (the walk of `paintText`/`paintTable` kept to what decides the layout arguments; the geometry text
// rectangle of presets is B4's and is not applied): for each shape `k` with a body, `t<slide>.<k>` with the box
// `(cx × s) / s` (s = 1280 / slide width, view a of capture-drawops), swapped for `vert`/`eaVert`/`vert270`; WordArt
// bodies (`prstTxWarp`, not `wordArtVert*`) with the plain measurer and `wrap: 'none'` for the single-line presets;
// every 5th body again with the emphasis `sizeScale` 1.5 (`t<slide>.<k>.x`); each table cell with a body (not covered by
// a merge) as `t<slide>.<k>.c<row>.<col>` through the TS `cellBody` with the column width and a height of 1e12
// (paintTable's row growth). Facets: `<id>.in` (box, measurer, options), `<id>.ask` (`n:sha256[0..16]` of the sorted
// distinct measurement keys), `<id>` (`len:sha256[0..32]` of the canonical JSON, or `err:<name>`).
//
// Measurement log (`.measures.tsv.gz`, sorted, one line per distinct key):
//   family<TAB>bold<TAB>italic<TAB>px (String(n))<TAB>kerning (auto|none|normal)<TAB>JSON text<TAB>width (String(n))
// The text has lone surrogates as U+FFFD (the key the Rust side builds); the same line without the width is the ask key.
import { createHash } from 'node:crypto';
import { mkdirSync, readFileSync, writeFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { pathToFileURL } from 'node:url';
import { gzipSync } from 'node:zlib';
import { CaptureWriter, fffd } from '../../uo-testkit/js/testkit.mjs';
import { canonJson, textDigest } from './lib/canon-json.mjs';
import { args, capturesDir, loadSlidesCore, oneLine, readSample, repoDir, trackedSamples, tsCommit, tsRoot } from './lib/common.mjs';
import { GROUPS, buildGroupDeck, deckFile, groupKey } from './lib/gen/groups.mjs';

const a = args();
const root = tsRoot(a);
const core = await loadSlidesCore(root);
const only = a.opt('--only');
const max = Number(a.opt('--max', '1e9'));
const capsSpec = (a.opt('--caps') ?? '').split(',').filter(Boolean);
for (const c of capsSpec) if (c !== 'nokerning' && c !== 'noseg') throw new Error(`unknown capability ${c}`);
const caps = { fontKerning: !capsSpec.includes('nokerning'), segmenter: !capsSpec.includes('noseg') };
const dist = (f) => pathToFileURL(join(root, 'slides', 'packages', 'slides-core', 'dist', f)).href;

// the layout: without the segmenter the module must be evaluated with `Intl.Segmenter` absent (a fresh instance)
let layoutTextBody;
if (caps.segmenter) ({ layoutTextBody } = await import(dist('text-layout.js')));
else {
  const saved = Intl.Segmenter;
  delete Intl.Segmenter;
  try { ({ layoutTextBody } = await import(`${dist('text-layout.js')}?noseg`)); } finally { Intl.Segmenter = saved; }
}
const { pptMeasure, paintShapesTo, DEFAULT_TITLE_PT, DEFAULT_BODY_PT } = await import(dist('render.js'));
const { SfntFont } = await import(pathToFileURL(join(root, 'write', 'packages', 'docx-layout', 'dist', 'typography', 'font', 'sfnt.js')).href);

// ------------------------------------------------------------------------------------------------ the open fonts
const FONT_FILES = {
  'liberation sans': join(repoDir, 'site', 'assets', 'fonts', 'liberationsans'),
  'liberation serif': join(repoDir, 'shared', 'fonts', 'files', 'liberationserif'),
  'liberation mono': join(repoDir, 'shared', 'fonts', 'files', 'liberationmono'),
  carlito: join(repoDir, 'shared', 'fonts', 'files', 'carlito'),
  caladea: join(repoDir, 'shared', 'fonts', 'files', 'caladea'),
};
const SUBST = { calibri: 'carlito', cambria: 'caladea', 'times new roman': 'liberation serif', times: 'liberation serif', 'courier new': 'liberation mono', courier: 'liberation mono', arial: 'liberation sans' };
const fonts = new Map();
function openFont(family, bold, italic) {
  const f = family.toLowerCase();
  const face = FONT_FILES[f] ? f : SUBST[f] ?? 'liberation sans';
  const style = bold && italic ? 'bolditalic' : bold ? 'bold' : italic ? 'italic' : 'regular';
  const key = `${face}-${style}`;
  let font = fonts.get(key);
  if (!font) { font = new SfntFont(new Uint8Array(readFileSync(`${FONT_FILES[face]}-${style}.ttf`))); fonts.set(key, font); }
  return font;
}
function kernUnits(font, g1, g2) {
  if (g1 > 0xffff || g2 > 0xffff) return 0;
  let u = font.kernMap.get(g1 * 65536 + g2);
  if (u === undefined) {
    const c0 = font.kernClass0.get(g1);
    if (c0 && !c0.cd2.has(g2)) u = c0.adv;
  }
  return u || 0;
}
const STACK = "', 'Aptos', Calibri, 'Liberation Sans', Arial, sans-serif";
/** `[italic ]{700|400} <px>px '<family>', <the default stack>` → its parts. */
function parseFont(s) {
  const m = /^(italic )?(700|400) (\S+)px '/.exec(s);
  if (!m || !s.endsWith(STACK)) throw new Error(`unexpected font string ${s}`);
  return { italic: !!m[1], bold: m[2] === '700', px: m[3], family: s.slice(m[0].length, s.length - STACK.length) };
}
/** The deterministic width: font units of the advances (and the GPOS/kern pairs unless kerning is `none`), × px / upem. */
function fakeWidth(f, kerning, text) {
  const font = openFont(f.family, f.bold, f.italic);
  let units = 0, prev = -1;
  for (const ch of text) {
    let cp = ch.codePointAt(0);
    if (cp >= 0xd800 && cp <= 0xdfff) cp = 0xfffd;
    const g = font.glyphId(cp);
    units += font.advanceWidth(g);
    if (kerning !== 'none' && prev >= 0) units += kernUnits(font, prev, g);
    prev = g;
  }
  return (units * Number(f.px)) / font.metrics.unitsPerEm;
}

// ------------------------------------------------------------------------------------------------ measuring context
const measures = new Map(); // ask key -> width string
class MeasureCtx {
  constructor() {
    this.font = '10px sans-serif';
    if (caps.fontKerning) this.fontKerning = 'auto';
    this.asks = new Set();
  }
  measureText(text) {
    const f = parseFont(this.font);
    const k = this.fontKerning ?? 'auto';
    const width = fakeWidth(f, k, text);
    const key = `${f.family}\t${f.bold ? 1 : 0}\t${f.italic ? 1 : 0}\t${f.px}\t${k}\t${JSON.stringify(fffd(text))}`;
    this.asks.add(key);
    const prev = measures.get(key);
    if (prev !== undefined && prev !== String(width)) throw new Error(`non-deterministic width for ${key}`);
    measures.set(key, String(width));
    return { width };
  }
  save() {}
  restore() {}
  setLineDash() {}
}
const ctx = new MeasureCtx();
const ppt = pptMeasure(ctx);
// the plain measurer of paintWarp (render.ts:1889) with the default font stack (layoutFontStr is module-private)
const plain = (text, f) => { ctx.font = `${f.italic ? 'italic ' : ''}${f.bold ? '700 ' : '400 '}${f.sizePt * 1}px '${f.family}${STACK}`; return ctx.measureText(text).width; };
/** The TS clears its per-character cache per paint (render.ts:224, 296): an empty paint clears it per call. */
const clearAdvCache = () => paintShapesTo(ctx, [], { slideW: 1, slideH: 1, images: new Map() }, 1);

// ------------------------------------------------------------------------------------------------ the walk
/** `cellBody` (render.ts:2160), as the TS writes it. */
function cellBody(cell) {
  const { txColor, txBold, txItalic } = cell;
  return {
    ...cell.txBody, anchor: cell.anchor ?? 't', ...(cell.vert ? { vert: cell.vert } : {}),
    insL: cell.marL ?? 91440, insR: cell.marR ?? 91440, insT: cell.marT ?? 45720, insB: cell.marB ?? 45720,
    paragraphs: txColor === undefined && txBold === undefined && txItalic === undefined ? cell.txBody.paragraphs
      : cell.txBody.paragraphs.map((p) => ({ ...p, runs: p.runs.map((r) => { const o = { ...r }; if (o.color === undefined && txColor !== undefined) o.color = txColor; if (o.bold === undefined && txBold !== undefined) o.bold = txBold; if (o.italic === undefined && txItalic !== undefined) o.italic = txItalic; return o; }) })),
  };
}

/** The layout calls of one slide: [{ id, body, w, h, measurer, opts }]. */
export function slideCalls(slide, slideW) {
  const s = 1280 / slideW;
  const calls = [];
  slide.shapes.forEach((sh, k) => {
    const cx = sh.xfrm?.cx ?? 0, cy = sh.xfrm?.cy ?? 0;
    const W = (cx * s) / s, H = (cy * s) / s;
    const isTitle = sh.phType === 'title' || sh.phType === 'ctrTitle';
    const dsz = isTitle ? DEFAULT_TITLE_PT : DEFAULT_BODY_PT, anchor = isTitle ? 'ctr' : 't';
    const tb = sh.txBody;
    if (tb) {
      if (tb.warp) {
        if (tb.vert !== 'wordArtVert' && tb.vert !== 'wordArtVertRtl') {
          const single = /^text(ArchUp|ArchDown|Circle|Button)$/.test(tb.warp.prst);
          calls.push({ id: `${k}`, body: single ? { ...tb, wrap: 'none' } : tb, w: W, h: H, measurer: 'plain', opts: { defaultSzPt: dsz, defaultAnchor: anchor } });
        }
      } else {
        const turn = tb.vert === 'vert' || tb.vert === 'eaVert' || tb.vert === 'vert270';
        let body = tb;
        if (turn) { const { vert: _v, ...flat } = tb; body = flat; }
        const [bw, bh] = turn ? [(cy * s) / s, (cx * s) / s] : [W, H];
        calls.push({ id: `${k}`, body, w: bw, h: bh, measurer: 'ppt', opts: { defaultSzPt: dsz, defaultAnchor: anchor, sizeScale: 1 } });
        if (k % 5 === 0) calls.push({ id: `${k}.x`, body, w: bw, h: bh, measurer: 'ppt', opts: { defaultSzPt: dsz, defaultAnchor: anchor, sizeScale: 1.5 } });
      }
    }
    const t = sh.table;
    if (t) {
      t.rows.forEach((row, ri) => row.cells.forEach((cell, ci) => {
        if (cell.hMerge || cell.vMerge || !cell.txBody) return;
        let cw = 0;
        for (let q = ci; q < ci + (cell.gridSpan ?? 1); q++) cw += t.colWidthsEmu[q] ?? 0;
        calls.push({ id: `${k}.c${ri}.${ci}`, body: cellBody(cell), w: cw, h: 1e12, measurer: 'ppt', opts: { defaultSzPt: DEFAULT_BODY_PT, defaultAnchor: 't' } });
      }));
    }
  });
  return calls;
}

const inFacet = (c) => `w=${c.w},h=${c.h},m=${c.measurer},sz=${c.opts.defaultSzPt},a=${c.opts.defaultAnchor},s=${c.opts.sizeScale ?? '-'}`;

const writer = new CaptureWriter(['Slides text layout of the frozen TS (B3); see js/capture-textlayout.mjs and core/docs/modules/uo-dml-render.md "B3 — text layout"']);
writer.meta('ts-root-commit', tsCommit(root));
writer.meta('caps', `fontKerning=${caps.fontKerning},segmenter=${caps.segmenter}`);
writer.meta('measure', 'real advance widths (hmtx) and GPOS/kern pairs of the open fonts, units * px / upem; substitutes: calibri->carlito, cambria->caladea, times->liberation serif, courier->liberation mono, else liberation sans');
const detail = {};
let nDecks = 0, nCalls = 0, nErr = 0;
const t0 = Date.now();

function record(key, bytes) {
  if (only && !key.includes(only)) return;
  if (nDecks >= max) return;
  nDecks++;
  let pkg, pres;
  try { pkg = core.PptxPackage.load(bytes); pres = core.readPresentation(pkg); } catch (e) { writer.set(key, 'read', `err:${oneLine(e?.message ?? e)}`); return; }
  writer.set(key, 'read', 'ok');
  writer.set(key, 'slides', pres.slides.length);
  for (let i = 0; i < pres.slides.length; i++) {
    let slide;
    try { slide = core.readSlide(pkg, pres.slides[i].target); } catch (e) { writer.set(key, `s${i}`, `err:${oneLine(e?.message ?? e)}`); continue; }
    const calls = slideCalls(slide, pres.slideW);
    writer.set(key, `s${i}.calls`, calls.length);
    for (const c of calls) {
      const id = `t${i}.${c.id}`;
      clearAdvCache();
      ctx.asks = new Set();
      let out;
      try {
        const res = layoutTextBody(c.body, c.w, c.h, c.measurer === 'plain' ? plain : ppt, c.opts);
        const text = canonJson(res);
        out = textDigest(text);
        if (a.opt('--detail')) detail[`${key}|${id}`] = text;
      } catch (e) {
        out = `err:${e?.name ?? 'Error'}`;
        nErr++;
      }
      nCalls++;
      const asks = [...ctx.asks].sort();
      writer.set(key, `${id}.in`, inFacet(c));
      writer.set(key, `${id}.ask`, `${asks.length}:${createHash('sha256').update(asks.join('\n')).digest('hex').slice(0, 16)}`);
      writer.set(key, id, out);
    }
  }
}

let name;
if (a.flag('--samples')) {
  name = 'samples';
  for (const rel of trackedSamples()) if (!/\.ppt$/i.test(rel)) record(`samples/${rel}`, readSample(rel));
} else if (a.opt('--gen')) {
  name = a.opt('--gen');
  const decksDir = a.opt('--decks');
  const g = GROUPS[name];
  if (!g) throw new Error(`unknown group ${name}`);
  for (let i = 0; i < g.count; i++) {
    const bytes = decksDir ? new Uint8Array(readFileSync(join(decksDir, name, deckFile(name, i)))) : new Uint8Array(buildGroupDeck(name, i).bytes);
    record(groupKey(name, i), bytes);
  }
} else {
  console.error('usage: capture-textlayout.mjs (--samples | --gen <group>) [--decks dir] [--out stem] [--caps nokerning,noseg] [--detail file] [--only s] [--max n]');
  process.exit(2);
}

const suffix = capsSpec.length ? `-${capsSpec.join('-')}` : '';
const stem = a.opt('--out', join(capturesDir, `textlayout-${name}${suffix}`));
mkdirSync(dirname(stem), { recursive: true });
writer.write(`${stem}.capture`);
const mt = [...measures].sort((x, y) => (x[0] < y[0] ? -1 : x[0] > y[0] ? 1 : 0)).map(([k, w]) => `${k}\t${w}`).join('\n');
const mgz = gzipSync(Buffer.from(mt + '\n', 'utf8'), { level: 9 });
writeFileSync(`${stem}.measures.tsv.gz`, mgz);
if (a.opt('--detail')) writeFileSync(a.opt('--detail'), gzipSync(Buffer.from(JSON.stringify(detail), 'utf8'), { level: 9 }));
console.log(`${nDecks} decks, ${nCalls} layout calls (${nErr} throw), ${measures.size} distinct measurements; measures.gz ${mgz.length} B, ${((Date.now() - t0) / 1000).toFixed(1)} s -> ${stem}.*`);
