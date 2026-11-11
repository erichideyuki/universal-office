// Verification (c) of the session in wasm (record uo-bindings.md "Milestone 2: the session in wasm"):
// hostile bytes into the decoding exports for a bounded time — random byte strings of 0–4 KiB, the
// prefixes and bit flips of valid messages, and the module's own random `Exec` and `Query` messages
// (`uo_session_test_random_exec` / `_query`, the generated test support) — into exec / query / complete and the open
// options. Every call succeeds or throws SessionError, never a WebAssembly.RuntimeError (a trap); a valid
// exec still works afterwards; the live bytes come back after free.
//   UO_WASM_PATH=<session-test .wasm> UO_WS_FUZZ_MS=10000 node --expose-gc --test crates/uo-wasm/tests/session_hostile.test.mjs
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { join, dirname } from 'node:path';
import { fileURLToPath } from 'node:url';
import { writeImports, bindWrite } from '../js/write.mjs';
import { bindSession } from '../js/session.mjs';
import { Rng } from '../../uo-testkit/js/testkit.mjs';
import * as codec from '../js/gen/session.ts';

const here = dirname(fileURLToPath(import.meta.url));
const repo = join(here, '../../../..');
const wasmPath = process.env.UO_WASM_PATH ?? join(here, '../../../target/wasm32-unknown-unknown/wasm/uo_wasm.wasm');
const bytes = readFileSync(wasmPath);
const names = WebAssembly.Module.exports(new WebAssembly.Module(bytes)).map((e) => e.name);
const skip = names.includes('uo_session_test_random_exec') && names.includes('uo_session_test_random_query') ? false : 'this build has no session self-test (--features session-selftest)';
const { imports, host } = writeImports();
const instance = skip ? null : (await WebAssembly.instantiate(bytes, imports)).instance;
const w = instance?.exports;
const uo = skip ? null : bindWrite(w, host);
const ss = skip ? null : bindSession(w, uo);
const FUZZ_MS = Number(process.env.UO_WS_FUZZ_MS ?? 10_000);
const fixture = (key) => readFileSync(join(repo, `core/crates/uo-docx-layout/tests/captures/f8/docs/${key}.docx`));
const settle = async () => {
  for (let i = 0; i < 5; i++) { globalThis.gc?.(); await new Promise((ok) => setTimeout(ok, 10)); }
  return w.uo_memory_live_bytes();
};

test('hostile messages never trap', { skip }, async () => {
  const doc = fixture('f8__7__0');
  let s = ss.open(doc);
  const enc = (fn, m) => fn(m, codec.newEncodeState(0));
  const valid = [
    enc(codec.encodeExec, { op: { t: 'command', v: { t: 'text', v: { t: 'type', text: 'x' } } } }),
    enc(codec.encodeExec, { op: { t: 'undo' } }),
    enc(codec.encodeQuery, { q: { t: 'headings' } }),
    enc(codec.encodeQuery, { q: { t: 'bookmarks', hidden: true, byLocation: false } }),
    enc(codec.encodeComplete, { job: 0, result: { t: 'failed' } }),
    enc(codec.encodeSessionOpen, { protected: false, seed: 1 }),
  ];
  let rng = new Rng(0xbad5e55);
  const counts = { calls: 0, ok: 0, refused: 0, randomExec: 0, randomQuery: 0 };
  let worst = 0;
  let worstWhat = '';
  const call = (b, which) => {
    const t = performance.now();
    const p = b.length ? uo.render.put(b) : 0;
    let status;
    try {
      switch (which % 4) {
        case 0: status = new uo.render.Buf(w.uo_session_exec(s.ptr, p, b.length)).take().status; break;
        case 1: status = new uo.render.Buf(w.uo_session_query(s.ptr, p, b.length)).take().status; break;
        case 2: status = new uo.render.Buf(w.uo_session_complete(s.ptr, p, b.length)).take().status; break;
        default: {
          const h = w.uo_session_new_blank(0, p, b.length);
          status = h ? 0 : 5;
          if (h) w.uo_session_free(h); else w.uo_last_error_clear();
        }
      }
    } catch (e) {
      assert.fail(`a trap on ${which % 4} with ${b.length} bytes: ${e}`);
    }
    const dt = performance.now() - t;
    if (dt > worst) { worst = dt; worstWhat = `${['exec', 'query', 'complete', 'new_blank'][which % 4]} ${b.length} B`; }
    counts.calls++;
    if (status === 0) counts.ok++; else counts.refused++;
  };
  let i = 0;
  // `ms` of hostile calls, or exactly `count` iterations; the number of iterations run
  const fuzz = (ms, count = Infinity) => {
  const t0 = performance.now();
  const start = i;
  while (i - start < count && (count !== Infinity || performance.now() - t0 < ms)) {
    const kind = i % 5;
    if (kind === 0) {
      const n = rng.below(4097);
      const b = new Uint8Array(n);
      for (let k = 0; k < n; k++) b[k] = rng.below(256);
      call(b, i);
    } else if (kind === 1) {
      const v = valid[rng.below(valid.length)];
      call(v.subarray(0, rng.below(v.length + 1)), valid.indexOf(v) < 2 ? 0 : valid.indexOf(v) < 4 ? 1 : valid.indexOf(v) === 4 ? 2 : 3);
    } else if (kind === 2) {
      const vi = rng.below(valid.length);
      const b = valid[vi].slice();
      if (b.length) b[rng.below(b.length)] ^= 1 << rng.below(8);
      call(b, vi < 2 ? 0 : vi < 4 ? 1 : vi === 4 ? 2 : 3);
    } else if (kind === 3) {
      // a random Exec of the module's generated support, run on the session
      const { status, data } = new uo.render.Buf(w.uo_session_test_random_exec(i)).take();
      if (status === 0) { counts.randomExec++; call(data, 0); }
    } else {
      // a random Query (generated support), run on the session
      const { status, data } = new uo.render.Buf(w.uo_session_test_random_query(i)).take();
      if (status === 0) { counts.randomQuery++; call(data, 1); }
    }
    i++;
  }
  return i - start;
  };
  // pass 1 for FUZZ_MS; pass 2 repeats exactly the same calls (same seeds) on a new session: a path
  // reached the first time may build a lazily initialised table once, so the gate is that pass 2 leaves
  // the live bytes where pass 1 left them
  const iterations = fuzz(FUZZ_MS);
  s.free();
  ss.clipboardClear();
  const base = await settle();
  rng = new Rng(0xbad5e55);
  i = 0;
  s = ss.open(doc);
  fuzz(0, iterations);
  // still works
  assert.equal(s.type('ok').t, 'done');
  s.free();
  ss.clipboardClear();
  const after = await settle();
  console.log(`hostile: ${counts.calls} calls in 2 passes of ${iterations} iterations (${FUZZ_MS} ms for the first; ${counts.ok} answered, ${counts.refused} refused, ${counts.randomExec} random Exec and ${counts.randomQuery} random Query run), worst ${worst.toFixed(1)} ms (${worstWhat}); live bytes ${base} -> ${after}`);
  assert.ok(!ss.dead);
  assert.equal(after, base);
});
