// The Slides EDITOR exports (B8 routing, src/slides_edit.rs, js/slides-editor.mjs) at their JS boundary, on the editor module
// (`slides` + `slides-edit`, with `count`): the editing API over a viewer deck of the SAME instance (the list the viewer emits after
// an edit shows it), the changed-parts protocol (a mirror kept from the answers equals the saved package), undo/redo, the clip
// between two editors, the save with fonts in two phases, refusals (another deck, junk messages), M4 (10,000 editors through
// free() and the FinalizationRegistry), M2 (a long edit/undo loop keeps live bytes and the wasm memory flat) and M5 (the
// 100-step history cap).
//
//   cargo build -p uo-wasm --features slides-edit,count --target wasm32-unknown-unknown --profile wasm
//   UO_WASM_PATH=<that .wasm> [UO_SE_SOAK=10000] node --expose-gc --test crates/uo-wasm/tests/slides_edit_api.test.mjs
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { join, dirname } from 'node:path';
import { fileURLToPath } from 'node:url';
import { bindSlides } from '../js/slides.mjs';
import { bindSlidesEditor, hasEditorExports, SlidesEditError } from '../js/slides-editor.mjs';
import { emitAll as emitAllWith } from './support/slides-stub.mjs';
// Slides wave 5: these replays compare with TS captures: every TS switch (`SlidesRules::ts_strict`)
const TS = { tsStrict: true };

const here = dirname(fileURLToPath(import.meta.url));
const wasmPath = process.env.UO_WASM_PATH ?? join(here, '../../../target/w4-editor/uo-slides-editor-count.wasm');
const { instance } = await WebAssembly.instantiate(readFileSync(wasmPath), {});
const w = instance.exports;
const uo = bindSlides(w);
const ue = bindSlidesEditor(w, uo);
const SOAK = Number(process.env.UO_SE_SOAK ?? 10_000);
const DECKS = join(here, '../../uo-pptx/tests/captures/decks');
const FONTS = join(here, '../../../../shared/fonts/files');
const deckBytes = (group, i) => new Uint8Array(readFileSync(join(DECKS, group, `${String(i).padStart(4, '0')}.pptx`)));
const emitAll = (...a) => emitAllWith(uo, ...a);
const live = () => uo.liveBytes();
const settle = async () => {
  for (let i = 0; i < 5; i++) { globalThis.gc?.(); await new Promise((r) => setTimeout(r, 10)); }
  return live();
};
const memory = () => w.memory.buffer.byteLength;

/** The shell side: a mirror of the package kept from the answers. */
function mirrorOf(bytes) {
  const d = uo.open(bytes, TS);
  const names = [];
  // the part list of the opened deck, read through the saved package of a fresh editor (the mirror starts as the file)
  const ed = ue.newEditor(d.ptr);
  const parts = new Map();
  const zipNames = listZip(ed.save());
  for (const n of zipNames) parts.set(n, d.part(n));
  ed.free(); d.free();
  names.push(...zipNames);
  return parts;
}
/** The entry names of a ZIP (central directory). */
function listZip(z) {
  const dv = new DataView(z.buffer, z.byteOffset, z.byteLength);
  let eocd = z.length - 22;
  while (eocd >= 0 && dv.getUint32(eocd, true) !== 0x06054b50) eocd--;
  const n = dv.getUint16(eocd + 10, true);
  let at = dv.getUint32(eocd + 16, true);
  const out = [];
  for (let i = 0; i < n; i++) {
    const nl = dv.getUint16(at + 28, true), xl = dv.getUint16(at + 30, true), cl = dv.getUint16(at + 32, true);
    out.push(new TextDecoder().decode(z.subarray(at + 46, at + 46 + nl)));
    at += 46 + nl + xl + cl;
  }
  return out;
}
const applyChanges = (mirror, r) => { for (const c of r.changes) { if (c.bytes) mirror.set(c.name, c.bytes); else mirror.delete(c.name); } };
const sameBytes = (a, b) => a.length === b.length && a.every((v, i) => v === b[i]);

test('the editor module has the editor exports and counts memory', () => {
  assert.ok(hasEditorExports(w), 'build uo-wasm with --features slides-edit');
  assert.ok(live() >= 0, 'build with count');
});

test('edits reach the viewer deck of the same instance; the mirror kept from the answers equals the package', () => {
  const bytes = deckBytes('b1a', 2);
  const mirror = mirrorOf(bytes);
  const d = uo.open(bytes, TS);
  const ed = ue.newEditor(d.ptr);
  ed.setRandomSeed(1);
  ed.setTextLang('en-US');
  const v = uo.newView(1);
  const scale = 1280 / d.size().w;
  const before = emitAll(d, v, 0, scale, 0, 0);
  assert.equal(before.status, 0);
  const st0 = ed.state();
  assert.equal(st0.changes.length, 0);
  assert.equal(st0.state.slides, d.count);

  let r = ed.apply({ t: 'insertTextBox', text: 'Hello from Rust', frame: { x: 914400, y: 914400, cx: 3657600, cy: 914400 } });
  assert.equal(r.outcome.t, 'ok');
  assert.equal(r.outcome.v.t, 'shape');
  assert.ok(r.outcome.v.value, 'the new shape id');
  assert.equal(r.state.selected, r.outcome.v.value, 'the insert selects the new shape');
  assert.ok(r.state.canUndo && r.state.dirty && r.state.undoLabel === 'textBox');
  assert.ok(r.changes.length > 0 && r.changes.some((c) => /^ppt\/slides\/slide\d+\.xml$/.test(c.name)));
  applyChanges(mirror, r);
  const after = emitAll(d, v, 0, scale, 0, 0);
  assert.equal(after.status, 0);
  assert.ok(!sameBytes(after.message, before.message), 'the emitted list changed with the edit');

  r = ed.apply({ t: 'addSlide' });
  assert.equal(r.outcome.v.t, 'index');
  applyChanges(mirror, r);
  assert.equal(d.count, r.state.slides, 'the deck re-read its slide list');
  assert.equal(d.count, st0.state.slides + 1);
  assert.equal(emitAll(d, v, r.outcome.v.v, scale, 0, 0).status, 0, 'the new slide emits');

  // undo twice: the package returns to the file; the mirror follows
  applyChanges(mirror, ed.undo());
  r = ed.undo();
  applyChanges(mirror, r);
  assert.ok(!r.state.canUndo && r.state.canRedo && r.state.redoLabel === 'textBox');
  assert.equal(d.count, st0.state.slides);
  const saved = listZip(ed.save());
  assert.deepEqual(saved, [...mirror.keys()], 'same parts, same order');
  for (const n of saved) assert.ok(sameBytes(d.part(n), mirror.get(n)), n);
  assert.equal(emitAll(d, v, 0, scale, 0, 0).status, 0, 'the restored slide emits');

  // a failed or refused command
  r = ed.apply({ t: 'setShapeFrame', shape: 'no-such-shape', frame: { x: 0, y: 0, cx: 1, cy: 1 } });
  assert.equal(r.outcome.t, 'ok');
  assert.deepEqual(r.changes, []);
  ed.free(); v.free(); d.free();
});

test('a clip copied in one editor pastes in another; queries; the save with fonts in two phases', () => {
  const a = uo.open(deckBytes('b1a', 3), TS);
  const b = uo.open(deckBytes('b1a', 4), TS);
  const ea = ue.newEditor(a.ptr);
  const eb = ue.newEditor(b.ptr);
  const ins = ea.apply({ t: 'insertShape', prst: 'ellipse' });
  const id = ins.outcome.v.value;
  const clip = ea.copyShape(id);
  assert.ok(clip instanceof Uint8Array && clip.length > 0);
  const c = ue.decodeClip(clip);
  assert.ok(c.xml.includes('ellipse') && c.sourceId > 0);
  const n0 = eb.state().state;
  const p = eb.apply({ t: 'pasteShape', clip: c });
  assert.equal(p.outcome.v.t, 'shape');
  assert.ok(p.state.canUndo && p.state.undoLabel === 'paste');
  assert.equal(p.state.index, n0.index);
  assert.equal(ea.copyShape('nope'), null, 'nothing to copy');

  const hits = eb.query({ t: 'findText', query: 'e', matchCase: false });
  assert.equal(hits.t, 'hits');
  assert.equal(eb.query({ t: 'defaultNewLayout' }).t, 'layout');
  assert.equal(eb.query({ t: 'fontEmbedding' }).t, 'embedding');

  eb.apply({ t: 'formatText', shape: p.outcome.v.value, f: { font: 'Carlito', bold: true } });
  eb.setFontEmbedding({ embed: true, subset: true });
  const req = eb.fontRequests();
  assert.ok(req.embed && req.requests.some((r) => r.family === 'Carlito'), JSON.stringify(req.requests));
  const file = { regular: 'regular', bold: 'bold', italic: 'italic', boldItalic: 'bolditalic' };
  const faces = req.requests.map((r) => (r.family === 'Carlito' ? new Uint8Array(readFileSync(join(FONTS, `carlito-${file[r.variant]}.ttf`))) : undefined));
  const out = eb.saveWithFonts(faces);
  assert.ok(out.bytes.length > 0 && out.embedded.includes('Carlito'), JSON.stringify(out.embedded));
  assert.ok(listZip(out.bytes).some((n) => /^ppt\/fonts\//.test(n)), 'the embedded font parts');
  assert.throws(() => eb.saveWithFonts([]), (e) => e instanceof SlidesEditError && e.status === 5, 'one answer per request');
  assert.ok(eb.markClean().state.dirty === false);
  ea.free(); eb.free(); a.free(); b.free();
});

test('refusals: another deck, a null editor, junk messages, a freed editor', () => {
  const a = uo.open(deckBytes('b1a', 0), TS);
  const b = uo.open(deckBytes('b1a', 0), TS);
  const status = (res) => { const s = w.uo_buf_status(res); w.uo_buf_free(res); return s; };
  const raw = w.uo_slides_editor_new(a.ptr);
  assert.ok(raw !== 0);
  assert.equal(status(w.uo_slides_editor_state(raw, b.ptr)), 11, 'another deck');
  assert.equal(status(w.uo_slides_editor_state(0, a.ptr)), 11, 'a null editor');
  assert.equal(status(w.uo_slides_editor_history(raw, a.ptr, 7)), 5, 'an unknown history command');
  for (const junk of [new Uint8Array([1, 2, 3]), new Uint8Array(64).fill(0x55)]) {
    const p = w.uo_alloc_bytes(junk.length);
    new Uint8Array(w.memory.buffer, p, junk.length).set(junk);
    assert.equal(status(w.uo_slides_editor_apply(raw, a.ptr, p, junk.length)), 5, 'junk');
  }
  w.uo_slides_editor_free(raw);
  const ed = ue.newEditor(a.ptr);
  assert.throws(() => ed.apply({ t: 'nope' }), /not a variant/);
  ed.free();
  assert.throws(() => ed.state(), (e) => e instanceof SlidesEditError && e.status === 11);
  a.free(); b.free();
});

test('M4: 10,000 editors through free() and through the FinalizationRegistry leave no live bytes', async () => {
  const bytes = deckBytes('b1a', 1);
  const d = uo.open(bytes, TS);
  // warm: the deck's caches the first editor fills (presentation, font option)
  { const e = ue.newEditor(d.ptr); e.state(); e.free(); }
  const before = await settle();
  for (let i = 0; i < SOAK; i++) {
    const e = ue.newEditor(d.ptr);
    if (i % 500 === 0) { e.state(); e.query({ t: 'fontEmbedding' }); }
    e.free();
  }
  assert.equal(await settle(), before, 'free(): live bytes back exactly');
  if (typeof globalThis.gc !== 'function') { d.free(); return; }
  for (let i = 0; i < SOAK / 10; i++) ue.newEditor(d.ptr);
  let after = await settle();
  for (let k = 0; k < 40 && after !== before; k++) { await new Promise((r) => setTimeout(r, 25)); after = await settle(); }
  assert.equal(after, before, 'finalizers: live bytes back exactly');
  d.free();
});

test('M2: 600 edit/undo/redo cycles keep live bytes and the wasm memory flat; M5: the history keeps 100 steps', async () => {
  const d = uo.open(deckBytes('b1a', 6), TS);
  const ed = ue.newEditor(d.ptr);
  ed.setRandomSeed(7);
  const v = uo.newView(1);
  const scale = 1280 / d.size().w;
  const cycle = (i) => {
    const r = ed.apply({ t: 'insertTextBox', text: `t${i % 7}` });
    const id = r.outcome.v.value;
    ed.apply({ t: 'formatText', shape: id, f: { bold: i % 2 === 0, size: 10 + (i % 5) } });
    ed.apply({ t: 'setShapeFrame', shape: id, frame: { x: 1000 * (i % 9), y: 2000, cx: 900000, cy: 300000 } });
    ed.undo(); ed.undo(); ed.redo(); ed.undo(); ed.undo();
    emitAll(d, v, 0, scale, 0, 0);
  };
  for (let i = 0; i < 120; i++) cycle(i);
  const samples = [], mem = [];
  for (let i = 0; i < 600; i++) {
    cycle(i);
    if (i % 50 === 49) { samples.push(await settle()); mem.push(memory()); }
  }
  const spread = Math.max(...samples) - Math.min(...samples);
  console.log(`M2: live bytes ${samples.join(',')} (spread ${spread}); wasm memory ${mem.join(',')}`);
  assert.ok(spread < 64 * 1024, `live bytes spread ${spread}: ${samples.join(',')}`);
  assert.equal(mem[mem.length - 1], mem[2], `the wasm memory stops growing: ${mem.join(',')}`);
  // M5: 150 edits, then undo until nothing is left: 100 steps
  for (let i = 0; i < 150; i++) ed.apply({ t: 'setShapeText', shape: ed.apply({ t: 'insertTextBox', text: 'x' }).outcome.v.value, text: `y${i}` });
  let undos = 0;
  while (ed.state().state.canUndo) { ed.undo(); undos++; }
  assert.equal(undos, 100, 'the 100-step limit of the TS history');
  ed.free(); v.free(); d.free();
});
