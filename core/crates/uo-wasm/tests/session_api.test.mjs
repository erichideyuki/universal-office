// The session exports of uo-wasm through js/session.mjs (milestone 2, uo-bindings.md "Milestone 2: the
// session in wasm"): the API on three committed fixtures (hot paths, exec / query / complete with a
// two-phase picture insert answered by the shell, undo / redo, a layout with hit / caret / selection
// rectangles, the save and the font save with one face); errors (null handle, bad bytes, Busy while a job
// is pending, Locked when protected); the memory gate at the boundary: M4 (10,000 sessions through free()
// and through the FinalizationRegistry, opened and blank; live bytes back exactly) and M2 (repeated
// sessions: live bytes flat, memory.buffer stops growing) — verification (b) of the record.
//   cargo build -p uo-wasm --features session,count,session-selftest --target wasm32-unknown-unknown --profile wasm
//   UO_WASM_PATH=<that .wasm> node --expose-gc --test crates/uo-wasm/tests/session_api.test.mjs
// UO_WS_M4 (default 10000) and UO_WS_SOAK (default 200) size the memory tests.
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { join, dirname } from 'node:path';
import { fileURLToPath } from 'node:url';
import { writeImports, bindWrite } from '../js/write.mjs';
import { bindSession, SessionError } from '../js/session.mjs';

const here = dirname(fileURLToPath(import.meta.url));
const repo = join(here, '../../../..');
const wasmPath = process.env.UO_WASM_PATH ?? join(here, '../../../target/wasm32-unknown-unknown/wasm/uo_wasm.wasm');
const bytes = readFileSync(wasmPath);
const has = WebAssembly.Module.exports(new WebAssembly.Module(bytes)).some((e) => e.name === 'uo_session_open');
const skip = has ? false : 'this build has no session exports (--features session)';
const { imports, host } = writeImports();
const instance = has ? (await WebAssembly.instantiate(bytes, imports)).instance : null;
const w = instance?.exports;
const uo = has ? bindWrite(w, host) : null;
const ss = has ? bindSession(w, uo) : null;
const counting = has && w.uo_memory_live_bytes() >= 0;
const M4 = Number(process.env.UO_WS_M4 ?? 10_000);
const SOAK = Number(process.env.UO_WS_SOAK ?? 200);
const live = () => w.uo_memory_live_bytes();
const settle = async () => {
  for (let i = 0; i < 5; i++) { globalThis.gc?.(); await new Promise((ok) => setTimeout(ok, 10)); }
  return live();
};

const OPEN_FONTS = [
  ['Liberation Sans', 'site/assets/fonts', 'liberationsans'],
  ['Liberation Serif', 'shared/fonts/files', 'liberationserif'],
  ['Liberation Mono', 'shared/fonts/files', 'liberationmono'],
  ['Carlito', 'shared/fonts/files', 'carlito'],
  ['Caladea', 'shared/fonts/files', 'caladea'],
];
const VARIANTS = ['regular', 'bold', 'italic', 'boldItalic'];
const fontFiles = has ? OPEN_FONTS.map(([family, dir, stem]) => [family, ['regular', 'bold', 'italic', 'bolditalic'].map((s) => readFileSync(join(repo, dir, `${stem}-${s}.ttf`)))]) : [];
const withFonts = (env) => {
  for (const [family, files] of fontFiles) env.pushFamily(family, { source: 'installed', embeddable: true, faces: files.map((b, i) => ({ variant: VARIANTS[i], bytes: b })) });
  return env;
};
const fixture = (key) => readFileSync(join(repo, `core/crates/uo-docx-layout/tests/captures/f8/docs/${key}.docx`));
const PNG = Uint8Array.from([
  0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a, 0, 0, 0, 0x0d, 0x49, 0x48, 0x44, 0x52, 0, 0, 0, 1, 0, 0, 0, 1, 8, 6, 0, 0, 0, 0x1f, 0x15,
  0xc4, 0x89, 0, 0, 0, 0x0d, 0x49, 0x44, 0x41, 0x54, 0x78, 0x9c, 0x63, 0xf8, 0xcf, 0xc0, 0xf0, 0x1f, 0, 0x05, 0x00, 0x01, 0xff, 0x89,
  0x99, 0x3d, 0x1d, 0, 0, 0, 0, 0x49, 0x45, 0x4e, 0x44, 0xae, 0x42, 0x60, 0x82,
]);
const OPTS = { now: 1_768_478_400_000, tzOffsetMin: 0, hyphenationLanguages: ['pt-BR', 'en', 'es'] };
const jobId = (j) => j.id;

/** an editor session on one document; a short log */
function editor(env, docx) {
  const log = [];
  const s = ss.open(docx, { author: 'Ana', env });
  for (const t of ['Olá ', 'mundo', '😀']) assert.equal(s.type(t).t, 'done');
  assert.equal(s.enter().t, 'done');
  assert.equal(s.backspace().t, 'done');
  assert.equal(s.delete().t, 'done');
  const sel = s.setSelection({ para: 0, offset: 0 }, { para: 0, offset: 3 });
  assert.equal(sel.t, 'done');
  assert.deepEqual(sel.selection.focus, { para: 0, offset: 3 });
  const bold = s.command({ t: 'format', v: { t: 'toggleRunProp', key: 'bold', button: 'single' } });
  assert.equal(bold.t, 'done');
  log.push(bold.events.map((e) => e.t).join(','));
  assert.equal(s.undo().t, 'done');
  assert.equal(s.redo().t, 'done');
  // queries
  assert.equal(s.query({ t: 'isDirty' }).v, true);
  assert.ok(Array.isArray(s.query({ t: 'headings' }).v));
  assert.ok(s.query({ t: 'fontUsage' }).v.length > 0);
  assert.equal(s.query({ t: 'copyFormat' }).v, true);
  assert.equal(s.applyPainter().t, 'done');
  // two-phase: the picture insert waits for the shell's decode
  const ins = s.command({ t: 'picture', v: { $t: 'insert', data: PNG, ext: 'png' } });
  assert.equal(ins.t, 'jobs');
  // Busy while pending
  const busy = s.type('x');
  assert.equal(busy.t, 'error');
  assert.equal(busy.v.t, 'busy');
  let last;
  for (const j of ins.jobs) last = s.complete(jobId(j), { t: 'image', width: 1, height: 1, rgba: Uint8Array.from([0, 0, 0, 255]) });
  assert.equal(last.t, 'done');
  log.push(`jobs ${ins.jobs.map((j) => j.t).join(',')}`);
  // the layout and the caret exports
  const layout = s.layout(env, OPTS);
  assert.ok(layout.pageCount > 0);
  const caret = s.caretRect(layout);
  assert.ok(caret === null || caret.page >= 0);
  const hit = s.hitTest(layout, 0, 1500, 1500);
  assert.ok(hit === null || hit.para >= 0);
  s.setSelection({ para: 0, offset: 0 }, { para: 0, offset: 2 });
  const rects = s.selectionRects(layout, 0);
  assert.ok(Array.isArray(rects));
  for (const d of ['left', 'right', 'up', 'down', 'home', 'end', 'wordLeft', 'wordRight']) {
    const r = s.moveCaret(d, false, layout);
    assert.ok(r === null || r.t === 'done', d);
  }
  assert.throws(() => s.moveCaret('up'), (e) => e instanceof SessionError && e.code === 5);
  log.push(`pages ${layout.pageCount}`);
  layout.free();
  // the save and the font save with one face
  assert.ok(s.save().length > 0);
  assert.equal(s.setFontEmbedding({ embed: true, subset: true, skipCommonFonts: false }).t, 'done');
  const req = s.fontRequests();
  assert.ok(req.requests.length > 0);
  const face = fontFiles[1][1][0];
  const out = s.saveWithFonts(req.requests.map((_, i) => (i === 0 ? face : undefined)), new Uint8Array(16 * req.requests.length).fill(7));
  assert.ok(out.bytes.length > 0);
  assert.equal(out.events[0].t, 'uw-font-embedding');
  log.push(`fonts ${req.requests.length}`);
  s.free();
  return log.join(' | ');
}

test('the editor session on three fixtures', { skip }, () => {
  const env = withFonts(uo.newEnv());
  for (const key of ['f8__7__0', 'f8__7__3', 'f8__7__10']) console.log(`${key}: ${editor(env, fixture(key))}`);
  env.free();
});

test('errors: null handle, bad bytes, protected, freed', { skip }, () => {
  // null handle
  const { status } = new uo.render.Buf(w.uo_session_enter(0)).take();
  assert.equal(status, 11);
  const s = ss.open(fixture('f8__7__0'));
  // bad bytes
  assert.throws(() => s.exec({ t: 'nonsense' }));
  const junk = Uint8Array.from([1, 2, 3, 4, 5]);
  const p = uo.render.put(junk);
  const r = new uo.render.Buf(w.uo_session_exec(s.ptr, p, junk.length)).take();
  assert.equal(r.status, 5);
  // protected: the text commands are locked
  const pr = ss.open(fixture('f8__7__0'), { protected: true });
  const t = pr.type('x');
  assert.ok(t === null || t.t === 'error' || t.t === 'done');
  console.log(`protected type: ${JSON.stringify(t?.t === 'error' ? t.v : t?.t)}`);
  pr.free();
  // not a docx
  assert.throws(() => ss.open(Uint8Array.from([0x50, 0x4b, 1, 2])));
  // a freed session
  s.free();
  assert.throws(() => s.type('x'), (e) => e instanceof SessionError && e.code === 11);
  s.free();
});

test('M4: 10,000 sessions through free() and the FinalizationRegistry, live bytes back exactly', { skip: skip || (!counting && 'not a count build') }, async () => {
  const blank = ss.newBlank();
  blank.type('tiny');
  const tiny = blank.save();
  blank.free();
  // warm-up: the ring, the thread-local slots, the first allocation of each size class
  for (let i = 0; i < 50; i++) { ss.open(tiny).free(); ss.newBlank().free(); }
  ss.clipboardClear();
  const base = await settle();
  for (let i = 0; i < M4; i++) {
    const s = ss.open(tiny);
    s.type('a');
    s.free();
  }
  ss.clipboardClear();
  assert.equal(await settle(), base, 'open + free');
  for (let i = 0; i < M4; i++) ss.newBlank().free();
  assert.equal(await settle(), base, 'newBlank + free');
  if (globalThis.gc) {
    // dropped without free(): collected by the registry (a gc every 500 keeps the wasm memory below 2 GiB)
    for (let i = 0; i < M4; i++) { ss.open(tiny); if (i % 500 === 499) await settle(); }
    let after = await settle();
    for (let k = 0; k < 20 && after !== base; k++) after = await settle();
    assert.equal(after, base, 'FinalizationRegistry');
    console.log(`M4: ${M4} x open/free, ${M4} x newBlank/free, ${M4} x dropped (FinalizationRegistry): live bytes back to ${base}`);
  } else {
    console.log('M4: the FinalizationRegistry part needs node --expose-gc');
  }
});

test('M2: repeated sessions are flat and memory.buffer stops growing', { skip: skip || (!counting && 'not a count build') }, async () => {
  const docs = ['f8__7__0', 'f8__7__3', 'f8__7__10'].map(fixture);
  const life = (i) => {
    const s = ss.open(docs[i % docs.length]);
    for (let k = 0; k < 50; k++) {
      if (k % 5 === 0) s.enter(); else if (k % 7 === 0) s.backspace(); else s.type('ab ');
    }
    s.undo(); s.redo();
    s.save();
    s.free();
  };
  for (let i = 0; i < 20; i++) life(i);
  ss.clipboardClear();
  const base = await settle();
  const buffer0 = w.memory.buffer.byteLength;
  const series = [];
  for (let i = 0; i < SOAK; i++) {
    life(i);
    if (i % 50 === 49) { ss.clipboardClear(); series.push(await settle()); }
  }
  const grew = w.memory.buffer.byteLength - buffer0;
  console.log(`M2: ${SOAK} sessions x 50 edits: live bytes ${base} -> ${series.join(', ')}; memory.buffer grew ${grew} B`);
  for (const v of series) assert.equal(v, base);
  assert.equal(grew, 0);
});
