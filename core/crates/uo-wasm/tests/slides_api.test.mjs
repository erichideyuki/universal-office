// The Slides wasm module (frontier B7, src/slides.rs, js/slides.mjs) at its JS boundary: the API over committed generated decks, the
// two-phase measure loop (answered by the TS captures' deterministic stub, tests/support/slides-stub.mjs), the .ppt module, the timing
// handles, errors, M4 (10,000 create/drop per handle, free() and the FinalizationRegistry path) and M2 (flat live bytes). The op-stream
// gate against the frozen TS streams is slides_drawops.test.mjs (Node only).
//
//   cargo build -p uo-wasm --features count,slides --target wasm32-unknown-unknown --profile wasm
//   UO_WASM_PATH=<that .wasm> [UO_PPT_WASM_PATH=<uo-slides-ppt.wasm>] [UO_SLIDES_SOAK=10000] node --expose-gc --test crates/uo-wasm/tests/slides_api.test.mjs
// The same file runs in WebKit through browser/run.mjs (no Node-only import here).
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { existsSync, readFileSync, readdirSync } from 'node:fs';
import { join, dirname } from 'node:path';
import { fileURLToPath } from 'node:url';
import { bindSlides, bindSlidesPpt, SlidesError } from '../js/slides.mjs';
import { decodeSlidesList, newTables } from '../js/gen/slides.ts';
import { emitAll as emitAllWith, stageAll } from './support/slides-stub.mjs';
// Slides wave 5: these replays compare with TS captures: every TS switch (`SlidesRules::ts_strict`)
const TS = { tsStrict: true };

const here = dirname(fileURLToPath(import.meta.url));
const wasmPath = process.env.UO_WASM_PATH ?? join(here, '../../../target/wasm32-unknown-unknown/wasm/uo_wasm.wasm');
const { instance } = await WebAssembly.instantiate(readFileSync(wasmPath), {});
const uo = bindSlides(instance.exports);
// the lazily loaded `.ppt` converter module (feature slides-ppt)
const pptPath = process.env.UO_PPT_WASM_PATH ?? join(here, '../../../target/b7/uo-slides-ppt.wasm');
const ppt = existsSync(pptPath) ? bindSlidesPpt((await WebAssembly.instantiate(readFileSync(pptPath), {})).instance.exports) : null;
const isCfb = (b) => b[0] === 0xd0 && b[1] === 0xcf && b[2] === 0x11 && b[3] === 0xe0;
const openAny = (bytes) => uo.open(isCfb(bytes) && ppt ? ppt.convert(bytes, TS) : bytes, TS);
const SOAK = Number(process.env.UO_SLIDES_SOAK ?? 10_000);
const emitAll = (...a) => emitAllWith(uo, ...a);
const DECKS_DIR = join(here, '../../uo-pptx/tests/captures/decks');
const SAMPLES = join(here, '../../../../slides/samples');
const groupBytes = (group, i) => new Uint8Array(readFileSync(join(DECKS_DIR, group, `${String(i).padStart(4, '0')}.pptx`)));

const live = () => uo.liveBytes();
const settle = async () => {
  for (let i = 0; i < 5; i++) { globalThis.gc?.(); await new Promise((r) => setTimeout(r, 10)); }
  return live();
};

test('the .ppt module converts a legacy deck that the main module then opens', (t) => {
  if (!ppt) return t.skip('uo-slides-ppt.wasm not built');
  const bytes = new Uint8Array(readFileSync(join(SAMPLES, 'legacy', readdirSync(join(SAMPLES, 'legacy')).find((f) => /\.ppt$/i.test(f)))));
  assert.ok(isCfb(bytes));
  assert.throws(() => uo.open(bytes, TS), (e) => e.status === 14);
  const d = openAny(bytes);
  assert.ok(d.count > 0);
  d.free();
  assert.throws(() => ppt.convert(new Uint8Array([0xd0, 0xcf, 0x11, 0xe0, 1, 2, 3])), (e) => e instanceof SlidesError && e.status === 3);
});

test('the module counts memory and the API opens a deck', () => {
  assert.ok(live() >= 0, 'build uo-wasm with --features count,slides');
  const d = uo.open(groupBytes('b1a', 0), TS);
  const s = d.size();
  assert.ok(s.w > 0 && s.h > 0 && s.count === d.count && s.count > 0);
  assert.equal(typeof d.notes(0), 'string');
  assert.equal(d.part('does/not/exist'), null);
  assert.ok(d.part('ppt/presentation.xml').length > 0);
  d.free();
  d.free(); // a second free is a no-op
});

test('errors: garbage, an empty file, a .ppt signature and bad arguments', () => {
  for (const bytes of [new Uint8Array(0), new Uint8Array([1, 2, 3]), Uint8Array.from({ length: 4096 }, (_, i) => (i * 31) & 255)]) {
    assert.throws(() => uo.open(bytes, TS), (e) => e instanceof SlidesError && e.status === 3);
  }
  assert.throws(() => uo.open(new Uint8Array([0xd0, 0xcf, 0x11, 0xe0, 0, 0, 0, 0]), TS), (e) => e instanceof SlidesError && e.status === 14);
  const d = uo.open(groupBytes('b1a', 1), TS);
  const v = uo.newView(1);
  assert.equal(d.emit(v, 9999, 1, 0, 0).status, 6);
  assert.equal(d.emit(v, 0, NaN, 0, 0).status, 5);
  assert.equal(d.emit(v, 0, -1, 0, 0).status, 5);
  assert.equal(d.emit(v, 0, 1, Infinity, 0).status, 5);
  assert.equal(d.notes(9999), '');
  // an answer of the wrong size is refused and leaves the deck usable
  const r = d.emit(v, 0, 1280 / d.size().w, 0, 0);
  if (r.status === 12) { assert.throws(() => d.answer(new Float64Array(0)), (e) => e.status === 5); }
  d.free(); v.free();
});

test('the measure loop: round 1 asks, later paints of the same deck need no round', () => {
  const d = uo.open(groupBytes('b1a', 2), TS);
  const v = uo.newView(7);
  const scale = 1280 / d.size().w;
  const first = emitAll(d, v, 0, scale, 0, 0);
  assert.equal(first.status, 0);
  assert.ok(first.rounds >= 2, 'a fresh table asks the shell');
  const again = emitAll(d, v, 0, scale, 0, 0);
  assert.equal(again.rounds, 1, 'the table keeps the answers');
  // a new font generation empties the table: asked again
  d.setGeneration(1);
  assert.ok(emitAll(d, v, 0, scale, 0, 0).rounds >= 2);
  // the same list twice is the same bytes (the encoder only adds table entries once)
  const t = newTables();
  const a = decodeSlidesList(first.message, t);
  assert.ok(a.items.length > 0);
  d.free(); v.free();
});

test('the click show and the playback timeline answer like the TS state machines', () => {
  const d = uo.open(groupBytes('b6a', 3), TS);
  const show = d.newShow({ from: 0, now: 0 });
  const f0 = show.frame(10);
  assert.equal(f0.index, 0);
  assert.ok(f0.steps >= 0);
  let guard = 0;
  let last = 'none';
  while (last !== 'end' && last !== 'exit' && guard++ < 200) last = show.next(100 + guard * 1000, true);
  assert.ok(['end', 'exit'].includes(last), `the show reaches the end screen (${last})`);
  assert.equal(show.frame(1e6).ended, true);
  assert.ok(Number.isFinite(show.dueAt()) || show.dueAt() === Infinity);
  assert.equal(show.goTo(0, 5e6), 'slide');
  assert.equal(show.frame(5e6).index, 0);
  assert.ok(['none', 'transition', 'step', 'slide', 'end', 'exit'].includes(show.prev(6e6)));
  assert.ok(['none', 'transition', 'step', 'slide', 'end', 'exit'].includes(show.update(7e6)));
  show.first(8e6); show.last(9e6);
  show.free();
  const tl = d.newTimeline({});
  const info = tl.info();
  assert.ok(info.slides.length > 0 && info.durationMs > 0);
  const st = tl.stateAt(info.durationMs / 2);
  assert.ok(st.pos >= 0 && st.index >= 0 && st.animMs <= info.durationMs);
  assert.equal(tl.stateAt(info.durationMs + 1).ended, true);
  assert.equal(tl.stateAt(NaN).pos >= -1, true);
  tl.free();
  // a range hides the slides outside it
  const ranged = d.newShow({ from: -1, now: 0, range: [1, 1] });
  assert.equal(ranged.frame(0).index, 1);
  ranged.free();
  d.free();
});

test('M4: 10,000 decks, views, shows and timelines through free() and through the FinalizationRegistry leave no live bytes', async () => {
  const bytes = groupBytes('b6a', 4);
  const before = await settle();
  for (let i = 0; i < SOAK; i++) {
    const d = uo.open(bytes, TS);
    const v = uo.newView(i);
    const s = d.newShow({ from: 0, now: 0 });
    const t = d.newTimeline({});
    if (i % 100 === 0) { emitAll(d, v, 0, 0.0001, 0, 0); s.next(1, true); t.stateAt(10); }
    s.free(); t.free(); v.free(); d.free();
  }
  assert.equal(await settle(), before, 'free(): live bytes back exactly');
  // the finalizer path: handles dropped without free() (needs gc(): the browser runner has none, so the Node run covers it)
  if (typeof globalThis.gc !== 'function') return;
  for (let i = 0; i < SOAK / 10; i++) { uo.open(bytes, TS); uo.newView(i); }
  let after = await settle();
  for (let k = 0; k < 40 && after !== before; k++) { await new Promise((r) => setTimeout(r, 25)); after = await settle(); }
  assert.equal(after, before, 'finalizers: live bytes back exactly');
});

test('B4b: the stage of a show and of a timeline, the end screen, bad arguments, and a playback that returns its memory', async () => {
  const d = uo.open(groupBytes('b6a', 3), TS);
  const scale = 1280 / d.size().w;
  let before = 0;
  for (let round = 0; round < 3; round++) {
    const v = uo.newView(1);
    const tl = d.newTimeline({});
    const info = tl.info();
    let lists = 0, frames = 0;
    for (let k = 0; k <= 40; k++) {
      const r = stageAll(uo, d, v, tl, (info.durationMs * k) / 40, 0, scale, 0, 0, true);
      assert.equal(r.status, 0);
      if (r.empty) continue;
      frames++;
      if (r.list) lists++;
      assert.ok(r.frame.length > 0);
    }
    assert.ok(frames > 30 && lists >= 1 && lists < frames, `${lists} lists over ${frames} frames`);
    // the origin of a transition: nothing at t = 0, the previous slide in its final state later
    assert.equal(stageAll(uo, d, v, tl, 0, 1, scale, 0, 0, false).empty, true);
    const show = d.newShow({ from: 0, now: 0 });
    assert.equal(stageAll(uo, d, v, show, 10, 0, scale, 0, 0, true).status, 0);
    let guard = 0;
    while (!show.frame(1e7).ended && guard++ < 200) show.next(1e3 * guard, true);
    assert.equal(stageAll(uo, d, v, show, 1e7, 0, scale, 0, 0, true).empty, true, 'the end screen has nothing on stage');
    assert.equal(d.stage(v, show, 0, 2, scale, 0, 0).status, 5, 'which = 2 is a bad argument');
    assert.equal(d.stage(v, show, 0, 0, NaN, 0, 0).status, 5, 'a NaN scale is a bad argument');
    v.clearStage();
    show.free(); tl.free(); v.free();
    // round 0 warms the deck's slide cache and the engines' lazily built tables
    if (round === 0) before = await settle();
  }
  assert.equal(await settle(), before, 'stages, frames and caches freed with the view: live bytes back exactly');
  d.free();
});

test('M2: 300 paints of one deck at a rotating view keep memory flat', async () => {
  const d = uo.open(groupBytes('b1a', 5), TS);
  const v = uo.newView(1);
  const n = d.count;
  const scale = 1280 / d.size().w;
  for (let i = 0; i < 40; i++) emitAll(d, v, i % n, scale, 0, 0);
  const samples = [];
  for (let i = 0; i < 300; i++) {
    emitAll(d, v, i % n, scale * (1 + (i % 3) * 0.01), 0, 0);
    if (i % 50 === 49) samples.push(await settle());
  }
  const spread = Math.max(...samples) - Math.min(...samples);
  assert.ok(spread < 256 * 1024, `live bytes spread ${spread} over ${samples.join(',')}`);
  d.free(); v.free();
});
