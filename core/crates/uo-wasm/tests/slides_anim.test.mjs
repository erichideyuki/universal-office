// The animated-frame gate of B4b through the wasm module (Node only: it reads the gzip captures and runs the recording context):
// every frame of the TS animated captures (`anim-<group>.ops.gz`, uo-pptx/js/capture-anim.mjs: `paintSlideTo` with the viewer's
// hooks at sampled times of the default playback timeline) is produced by uo-slides.wasm (`deck.stage` over the same timeline:
// the stage list with slots and the frame's `SlotFrame`, with the re-emitted shapes), decoded, painted by `paint-slides.ts` over
// the recording context and compared line by line with the frozen TS stream (draw form, exact). The media frame is the capture's
// deterministic stub. A fresh view per frame, as the TS capture's fresh renderer; a second pass paints every frame of a deck
// through ONE view (the stage reused across frames) and must give the same streams.
//
//   cargo build -p uo-wasm --features count,slides --target wasm32-unknown-unknown --profile wasm
//   UO_WASM_PATH=<that .wasm> [UO_ANIM_DECKS=1000] [UO_ANIM_CAPTURES=anim-b6a,anim-samples] [UO_ANIM_DIR=<dir of the captures>] node --test crates/uo-wasm/tests/slides_anim.test.mjs
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { existsSync, readFileSync } from 'node:fs';
import { join, dirname } from 'node:path';
import { gunzipSync } from 'node:zlib';
import { fileURLToPath } from 'node:url';
import { bindSlides, bindSlidesPpt } from '../js/slides.mjs';
import { decodeSlidesList, newTables } from '../js/gen/slides.ts';
import { decodeSlotFrame, newTables as newSlotTables } from '../js/gen/slots.ts';
import { capturesDir, readSample } from '../../uo-pptx/js/lib/common.mjs';
import { GROUPS, buildGroupDeck, deckFile } from '../../uo-pptx/js/lib/gen/groups.mjs';
import { compare, form } from '../../uo-pptx/js/lib/opstream.mjs';
import { recordList } from '../../uo-pptx/js/lib/rust-paint.mjs';
import { stageAll } from './support/slides-stub.mjs';
// Slides wave 5: these replays compare with TS captures: every TS switch (`SlidesRules::ts_strict`)
const TS = { tsStrict: true };

const here = dirname(fileURLToPath(import.meta.url));
const wasmPath = process.env.UO_WASM_PATH ?? join(here, '../../../target/wasm32-unknown-unknown/wasm/uo_wasm.wasm');
const uo = bindSlides((await WebAssembly.instantiate(readFileSync(wasmPath), {})).instance.exports);
const pptPath = process.env.UO_PPT_WASM_PATH ?? join(here, '../../../target/b7/uo-slides-ppt.wasm');
const ppt = existsSync(pptPath) ? bindSlidesPpt((await WebAssembly.instantiate(readFileSync(pptPath), {})).instance.exports) : null;
const isCfb = (b) => b[0] === 0xd0 && b[1] === 0xcf && b[2] === 0x11 && b[3] === 0xe0;
const DECKS = Number(process.env.UO_ANIM_DECKS ?? 1000);
const CAPTURES = (process.env.UO_ANIM_CAPTURES ?? 'anim-b6a,anim-samples,anim-b1a').split(',');

/** The media frame stub of capture-anim.mjs. */
function mediaStub(shape, frame) {
  const k = (shape + frame) % 3;
  if (k === 0) return null;
  return { image: { width: 64, height: 48, naturalWidth: 64, naturalHeight: 48, __h: 'media-frame-stub' }, alpha: k === 1 ? 1 : 0.5 };
}

function deckBytes(key) {
  if (key.startsWith('samples/')) return readSample(key.slice('samples/'.length));
  const m = /^gen\/([^/]+)\/(\d+)$/.exec(key);
  if (!m || !GROUPS[m[1]]) throw new Error(`unknown deck key ${key}`);
  const f = join(capturesDir, 'decks', m[1], deckFile(m[1], Number(m[2])));
  return existsSync(f) ? new Uint8Array(readFileSync(f)) : new Uint8Array(buildGroupDeck(m[1], Number(m[2])).bytes);
}

/** `[{ deck, frames: [{ slide, header, lines, error }] }]` of an `anim-*.ops.gz`. */
function parseAnim(text) {
  const decks = [];
  let cur = null, fr = null;
  for (const line of text.split('\n')) {
    if (line.startsWith('#deck ')) { cur = { deck: line.slice(6), frames: [] }; decks.push(cur); fr = null; } else if (line.startsWith('#frame ')) {
      const parts = line.slice(7).split(' ');
      fr = { slide: Number(parts[0]), header: Object.fromEntries(parts.slice(1).map((kv) => kv.split('='))), lines: [], error: undefined };
      cur.frames.push(fr);
    } else if (line.startsWith('#error ') && fr) fr.error = line.slice(7);
    else if (line.length && fr) fr.lines.push(line);
  }
  return decks;
}

/** One frame through a view: the stage list (decoded into `st.tables` when new), the frame and its replacements, painted. */
function paintFrame(deck, tl, view, st, h, label) {
  const scale = Number(h.scale), ox = Number(h.ox), oy = Number(h.oy), n = Number(h.n);
  const r = stageAll(uo, deck, view, tl, Number(h.t), 0, scale, ox, oy, h.media === '1');
  assert.equal(r.status, 0, `${label}: status ${r.status}`);
  assert.equal(r.empty, false, `${label}: nothing on stage`);
  if (r.list) st.list = decodeSlidesList(r.list, st.tables);
  const frame = decodeSlotFrame(r.frame, st.slotTables);
  const replacements = new Map();
  for (const rep of frame.replacements) replacements.set(rep.shape, { list: decodeSlidesList(rep.list, st.tables), values: rep.values });
  const [cw, ch] = h.canvas.split('x').map(Number);
  const tables = st.tables;
  const rec = recordList({
    list: st.list, tables, frame, replacements, width: cw, height: ch,
    bitmap: (i) => view.bitmap(tables.bitmaps[i].data),
    media: (i) => { const m = tables.media[i]; return m ? deck.part(m.part) : null; },
    mediaFrame: (shape) => mediaStub(shape, n),
  });
  assert.equal(rec.error, undefined, `${label}: ${rec.error}`);
  return { text: rec.text, replaced: frame.replacements.length, slots: st.list.slots.length, fresh: r.list !== null, probes: st.list.slots.some((s) => s.kind === 'boundsPass') };
}

function animGate(capture) {
  const file = join(process.env.UO_ANIM_DIR ?? capturesDir, `${capture}.ops.gz`);
  test(`the stage list + slot frame equal the frozen TS animated frames (${capture})`, { skip: !existsSync(file) && `${capture}.ops.gz not captured` }, () => {
    const decks = parseAnim(gunzipSync(readFileSync(file)).toString('utf8'));
    let nDecks = 0, compared = 0, equal = 0, reemit = 0, slotsTotal = 0, reused = 0, probed = 0;
    const bad = [];
    for (const { deck: key, frames } of decks) {
      if (nDecks >= DECKS) break;
      nDecks++;
      let bytes = deckBytes(key);
      if (isCfb(bytes)) { if (!ppt) continue; bytes = ppt.convert(bytes, TS); }
      const deck = uo.open(bytes, TS);
      const tl = deck.newTimeline({});
      const shared = uo.newView(1);
      const sst = { tables: newTables(), slotTables: newSlotTables(), list: null };
      for (const f of frames) {
        if (f.error) continue;
        const label = `${key} s${f.slide} t=${f.header.t}`;
        const view = uo.newView(1);
        const r = paintFrame(deck, tl, view, { tables: newTables(), slotTables: newSlotTables(), list: null }, f.header, label);
        view.free();
        compared++;
        slotsTotal += r.slots;
        if (r.replaced) reemit++;
        const c = compare(form(f.lines, 'draw'), r.text, { which: 'draw', eps: 0 });
        if (c.equal) equal++;
        else bad.push(`${label} line ${c.index}: TS ${c.a?.slice(0, 120)} | RUST ${c.b?.slice(0, 120)}`);
        // the stage reused across the frames of the deck gives the same stream, except the `chartPartBounds` probes, which
        // the TS draws only while its bounds cache misses (the first frame of a stage: a fresh view always is one)
        const s = paintFrame(deck, tl, shared, sst, f.header, `${label} (shared view)`);
        if (s.text === r.text) reused++;
        else if (!s.fresh && s.probes) probed++;
        else bad.push(`${label}: the shared view differs from a fresh one`);
      }
      shared.free();
      tl.free();
      deck.free();
    }
    console.log(`  ${capture}: ${nDecks} decks, ${compared} frames compared, ${equal} equal, ${reused} equal through a reused stage (+${probed} without the first frame's chart probes), ${reemit} with re-emitted shapes, ${slotsTotal} slots`);
    if (process.env.UO_ANIM_LIST) for (const b of bad) console.log("  BAD", b.slice(0, 200));
    assert.deepEqual(bad.slice(0, 5), [], `${bad.length} frames differ`);
    assert.ok(compared > 0);
  });
}

for (const c of CAPTURES) animGate(c);
