// The shell exports of the editor switch through js/session.mjs (record uo-bindings.md "Milestone 2: the editor
// switch"): the API on three committed fixtures (open, type, relayout and its report, the caret on the relayout's
// layout, the toolbar state after a bold toggle, paraText, storyInfo, hyphenationPatterns, copied) and M4 at the
// boundary (10,000 shell engines with a relayout each through free() and through the FinalizationRegistry; live
// bytes back exactly).
//   cargo build -j 4 -p uo-wasm --features session,count --target wasm32-unknown-unknown --profile wasm
//   UO_WASM_PATH=<that .wasm> node --expose-gc --test crates/uo-wasm/tests/shell_api.test.mjs
// UO_ES_M4 (default 10000) sizes the memory test.
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { SessionError } from '../js/session.mjs';
import { loadModule, fixture, withFonts, OPTS } from './session_support.mjs';

const { has, names, uo, ss, counting, settle } = await loadModule();
const skip = has && names.includes('uo_shell_relayout') ? false : 'this build has no shell exports (--features session)';
const M4 = Number(process.env.UO_ES_M4 ?? 10_000);

test('the shell API on three fixtures', { skip }, () => {
  const env = withFonts(uo.newEnv());
  const lines = [];
  for (const key of ['f8__7__0', 'f8__7__3', 'f8__7__10']) {
    const s = ss.open(fixture(key), { env });
    const e = ss.newShellEngine();
    assert.equal(e.report(), null, 'no report before the first relayout');
    let l = e.relayout(s, env, OPTS);
    assert.ok(l.pageCount > 0);
    assert.equal(e.report().path, 'full');
    // the same pages as the session's full layout
    const full = s.layout(env, OPTS);
    assert.deepEqual(l.pageHeads(), full.pageHeads());
    full.free();
    const info = s.storyInfo();
    assert.ok(info.paras > 0);
    const last = s.paraText(info.paras - 1);
    assert.equal(last.length, info.lastLength);
    assert.equal(s.paraText(info.paras), null);
    s.setSelection({ para: 0, offset: 0 });
    assert.equal(s.type('Olá').t, 'done');
    assert.ok(s.paraText(0).startsWith('Olá'));
    l.free();
    l = e.relayout(s, env, OPTS);
    const r = e.report();
    assert.ok(r.path === 'full' || r.path === 'incremental');
    const caret = s.caretRect(l);
    assert.ok(caret === null || caret.page >= 0);
    // the toolbar state follows a bold toggle (pending props at a caret)
    const st0 = s.selectionState();
    assert.equal(typeof st0.run.fontAscii, 'string');
    assert.equal(typeof st0.run.szHalfPt, 'number');
    assert.equal(s.command({ t: 'format', v: { t: 'toggleRunProp', key: 'bold', button: 'single' } }).t, 'done');
    const st1 = s.selectionState();
    assert.equal(st1.run.bold, !st0.run.bold);
    assert.equal(st1.pending, true);
    // a copy
    s.setSelection({ para: 0, offset: 0 }, { para: 0, offset: 3 });
    assert.equal(s.command({ t: 'clipboard', v: { t: 'copy' } }).t, 'done');
    const c = s.copied();
    assert.equal(c?.text, 'Olá');
    assert.ok(Array.isArray(s.hyphenationPatterns(env, ['pt-BR', 'en', 'es'])));
    lines.push(`${key}: ${l.pageCount} pages, ${info.paras} paragraphs, relayout ${r.path}${r.reason ? ` (${r.reason})` : ''}, font ${st0.run.fontAscii} ${st0.run.szHalfPt / 2} pt, html ${c.html.length} B`);
    l.free();
    e.free();
    e.free(); // idempotent
    assert.throws(() => e.relayout(s, env, OPTS), (x) => x instanceof SessionError && x.code === 11);
    s.free();
  }
  env.free();
  console.log(lines.join('\n'));
});

test('M4: 10,000 shell engines with a relayout through free() and the FinalizationRegistry', { skip: skip || (!counting && 'not a count build') }, async () => {
  const env = withFonts(uo.newEnv());
  const blank = ss.newBlank();
  blank.type('tiny');
  const tiny = blank.save();
  blank.free();
  const s = ss.open(tiny, { env });
  const once = () => { const e = ss.newShellEngine(); e.relayout(s, env, OPTS).free(); return e; };
  // warm-up: the face cache, the thread-local slots, the first allocation of each size class
  for (let i = 0; i < 50; i++) once().free();
  ss.clipboardClear();
  const base = await settle();
  const t0 = performance.now();
  for (let i = 0; i < M4; i++) once().free();
  const ms = performance.now() - t0;
  assert.equal(await settle(), base, 'new + relayout + free');
  if (globalThis.gc) {
    for (let i = 0; i < M4; i++) { once(); if (i % 500 === 499) await settle(); }
    let after = await settle();
    for (let k = 0; k < 20 && after !== base; k++) after = await settle();
    assert.equal(after, base, 'FinalizationRegistry');
    console.log(`M4: ${M4} x (newShellEngine + relayout + free) in ${ms.toFixed(0)} ms, ${M4} x dropped (FinalizationRegistry): live bytes back to ${base}`);
  } else {
    console.log('M4: the FinalizationRegistry part needs node --expose-gc');
  }
  s.free();
  env.free();
});
