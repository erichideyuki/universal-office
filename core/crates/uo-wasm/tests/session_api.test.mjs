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
import { SessionError } from '../js/session.mjs';
import { loadModule, fixture, fontFiles, withFonts, OPTS } from './session_support.mjs';

const { has, w, uo, ss, counting, settle } = await loadModule();
const skip = has ? false : 'this build has no session exports (--features session)';
const M4 = Number(process.env.UO_WS_M4 ?? 10_000);
const SOAK = Number(process.env.UO_WS_SOAK ?? 200);
const live = () => w.uo_memory_live_bytes();

const PNG = Uint8Array.from([
  0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a, 0, 0, 0, 0x0d, 0x49, 0x48, 0x44, 0x52, 0, 0, 0, 1, 0, 0, 0, 1, 8, 6, 0, 0, 0, 0x1f, 0x15,
  0xc4, 0x89, 0, 0, 0, 0x0d, 0x49, 0x44, 0x41, 0x54, 0x78, 0x9c, 0x63, 0xf8, 0xcf, 0xc0, 0xf0, 0x1f, 0, 0x05, 0x00, 0x01, 0xff, 0x89,
  0x99, 0x3d, 0x1d, 0, 0, 0, 0, 0x49, 0x45, 0x4e, 0x44, 0xae, 0x42, 0x60, 0x82,
]);
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
  const face = fontFiles()[1][1][0];
  const out = s.saveWithFonts(req.requests.map((_, i) => (i === 0 ? face : undefined)), new Uint8Array(16 * req.requests.length).fill(7));
  assert.ok(out.bytes.length > 0);
  assert.equal(out.events[0].t, 'uw-font-embedding');
  // (follow-up 5) a malformed phase 2 keeps phase 1: the host answers again
  const req2 = s.fontRequests();
  const guids = new Uint8Array(16 * req2.requests.length).fill(7);
  assert.throws(() => s.saveWithFonts([...req2.requests.map(() => undefined), undefined], new Uint8Array(16 * (req2.requests.length + 1))), (e) => e instanceof SessionError && e.code === 5);
  assert.throws(() => s.saveWithFonts(req2.requests.map(() => undefined), guids.subarray(1)), (e) => e instanceof SessionError && e.code === 5);
  const again = s.saveWithFonts(req2.requests.map(() => undefined), guids);
  assert.ok(again.bytes.length > 0, 'phase 1 survived the malformed phase 2');
  assert.throws(() => s.saveWithFonts(req2.requests.map(() => undefined), guids), (e) => e instanceof SessionError && e.code === 2);
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
  // protected (follow-up 4): every refused command answers `done` and changes nothing, as the TS
  // `locked()` returns silently; `CommandError::Locked` is never produced (a gate item of the record)
  const pr = ss.open(fixture('f8__7__0'), { protected: true });
  const before = pr.save();
  const same = () => { const b = pr.save(); return b.length === before.length && b.every((v, k) => v === before[k]); };
  const t = pr.type('x');
  assert.equal(t.t, 'done');
  assert.ok(same(), 'typing in a protected session leaves the document unchanged');
  assert.equal(pr.query({ t: 'isDirty' }).v, false);
  const outcomes = [];
  for (const [name, op] of [
    ['bold', { t: 'command', v: { t: 'format', v: { t: 'toggleRunProp', key: 'bold', button: 'single' } } }],
    ['center', { t: 'command', v: { t: 'paragraph', v: { t: 'setAlignment', alignment: 'center' } } }],
    ['enter', { t: 'command', v: { t: 'text', v: { t: 'enter' } } }],
    ['backspace', { t: 'command', v: { t: 'text', v: { t: 'backspace' } } }],
    ['comment', { t: 'command', v: { t: 'comment', v: { t: 'add', text: 'x' } } }],
    ['pasteText', { t: 'command', v: { t: 'clipboard', v: { t: 'pasteText', text: 'pasted' } } }],
  ]) {
    let r;
    try { r = pr.exec(op); } catch (e) { if (e instanceof SessionError && e.code === 5) assert.fail(`${name}: refused by the codec (5)`); throw e; }
    assert.notEqual(r.t, 'error', `${name}: ${JSON.stringify(r)}`);
    assert.ok(!(r.t === 'error' && r.v?.t === 'locked'));
    // `pasteText` is the one command the TS does not guard with `locked()` (pasteTextOnly checks only the read view): a
    // faithful port lets it through in a protected session (a gate item of the record)
    if (name === 'pasteText') { outcomes.push(`${name}: ${r.t}, ${same() ? 'unchanged' : 'CHANGED the document (as the TS: no protected-mode guard)'}`); continue; }
    assert.ok(same(), `${name} leaves the protected document unchanged`);
    outcomes.push(`${name}: ${r.t}`);
  }
  console.log(`protected: type done, unchanged; ${outcomes.join('; ')}`);
  pr.free();
  // not a docx
  assert.throws(() => ss.open(Uint8Array.from([0x50, 0x4b, 1, 2])));
  // a freed session
  s.free();
  assert.throws(() => s.type('x'), (e) => e instanceof SessionError && e.code === 11);
  s.free();
});

const STALE = Number(process.env.UO_WS_STALE ?? 200);
test('stale layouts: hit / caret / selection / moves after edits and context changes never trap', { skip }, async () => {
  const floats = [NaN, Infinity, -Infinity, -1, 0, 1500, -1e300, 1e300, 300, 5000];
  const dirs = ['left', 'right', 'up', 'down', 'home', 'end', 'wordLeft', 'wordRight'];
  let calls = 0;
  let seed = 0x57a1e;
  const rnd = (n) => { seed ^= seed << 13; seed >>>= 0; seed ^= seed >>> 17; seed ^= seed << 5; seed >>>= 0; return seed % n; };
  const active = { 'uw-cellmode': 0, 'uw-notemode': 0, 'uw-hfmode': 0 };
  let inCell = false;
  let cellCalls = 0;
  const count = (out) => { for (const e of out?.events ?? []) { if (e.t === 'uw-cellmode') inCell = e.active; if (e.active && e.t in active) active[e.t]++; } };
  const ctx = (v) => count(s_.command({ t: 'context', v }));
  let s_;
  const round = () => { const env = withFonts(uo.newEnv()); for (const key of ['f8__7__0', 'f8__7__3', 'f8__7__10']) {
    const s = ss.open(fixture(key));
    s_ = s;
    let layout = s.layout(env, OPTS);
    const pages = layout.pageCount;
    const queries = () => {
      const page = [0, Math.max(pages - 1, 0), pages, 4294967295][rnd(4)];
      s.hitTest(layout, page, floats[rnd(floats.length)], floats[rnd(floats.length)]);
      s.caretRect(layout);
      s.selectionRects(layout, page);
      s.moveCaret(dirs[rnd(8)], rnd(2) === 1, layout);
      calls += 4;
      if (inCell) cellCalls += 4;
    };
    // every context once before the random part, each against the layout taken above AND queried while it is active
    inCell = false;
    ctx({ t: 'enterCell', anchor: 0, hops: [{ row: 0, col: 0 }] });
    queries();
    ctx({ t: 'exitCell' });
    ctx({ t: 'enterNote', footnote: true, id: 3, atEnd: false });
    queries();
    ctx({ t: 'exitNote' });
    ctx({ t: 'enterHeaderFooter', part: 0, page: 0 });
    queries();
    ctx({ t: 'exitHeaderFooter' });
    // the cells the document really has (anchor 0, rows and columns up to 3): the random part enters THESE, not blind indices
    const cells = [];
    for (let row = 0; row < 3; row++) {
      for (let col = 0; col < 3; col++) {
        inCell = false;
        ctx({ t: 'enterCell', anchor: 0, hops: [{ row, col }] });
        if (inCell) { cells.push([row, col]); queries(); ctx({ t: 'exitCell' }); }
      }
    }
    for (let it = 0; it < STALE; it++) {
      switch (rnd(16)) {
        case 0: for (let k = 0; k <= rnd(30); k++) s.backspace(); break;
        case 1: for (let k = 0; k <= rnd(30); k++) s.delete(); break;
        case 2: s.setSelection({ para: 0, offset: 0 }, { para: 1e9, offset: 1e9 }); s.delete(); break;
        case 3: for (let k = 0; k <= rnd(5); k++) s.enter(); break;
        case 4: s.type('abc def'); break;
        // real contexts: a cell of the document's tables, a note of the fixtures (ids 3/4 and 7/9), a header/footer part
        case 5: if (cells.length) { const [row, col] = cells[rnd(cells.length)]; ctx({ t: 'enterCell', anchor: 0, hops: [{ row, col }] }); } else ctx({ t: 'enterCell', anchor: rnd(3), hops: [{ row: rnd(3), col: rnd(3) }] }); break;
        case 6: ctx({ t: 'enterNote', footnote: true, id: [3, 4, 7, 9][rnd(4)], atEnd: rnd(2) === 1 }); break;
        case 7: ctx({ t: 'enterHeaderFooter', part: rnd(4), page: rnd(3) }); break;
        case 8: ctx({ t: ['exitNote', 'exitCell', 'exitHeaderFooter', 'exitTextbox'][rnd(4)] }); break;
        case 9: s.undo(); break;
        case 10: s.redo(); break;
        // a fresh layout now and then: the lines cache of a non-body scope is rebuilt against new geometry
        case 11: if (rnd(4) === 0) { layout.free(); layout = s.layout(env, OPTS); } break;
        default: s.setSelection({ para: rnd(40), offset: rnd(200) }, { para: rnd(40), offset: rnd(200) });
      }
      queries();
    }
    assert.ok(!ss.dead, `${key}: no trap`);
    layout.free();
    s.free();
  } env.free(); };
  // two rounds: the first may build lazily initialised tables, the second must leave the live bytes where the first did
  round();
  ss.clipboardClear();
  const base = await settle();
  round();
  ss.clipboardClear();
  if (counting) assert.equal(await settle(), base, 'live bytes back after layout.free() and s.free()');
  for (const [k, n] of Object.entries(active)) assert.ok(n > 0, `${k} was never activated`);
  assert.ok(cellCalls > 0, 'stale layouts were queried while in a cell');
  console.log(`stale layouts: ${calls} calls in 2 rounds (${cellCalls} made while in a cell), no trap; activations ${JSON.stringify(active)}; live bytes ${base}`);
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
