// Parity self-test of uo-drawingml inside the wasm module (hybrid mode, continuous verification): the module
// replays the committed TS captures of the crate (presets, custom guides, arcs, colour, patterns, adjusts, blip,
// light, bevel, raster, effects: crates/uo-drawingml/tests/captures/*.capture) with the replay functions of the
// native parity tests, so native, wasm and the frozen TS (Node 24.15.0 on Linux) must give the same digests,
// bit for bit. UO_DML_EVERY=<n> (default 4) replays the n-th item of each loop of every corpus (a representative
// subset, a few seconds); 1 is the whole corpus. The same file runs in WebKit through `browser/run.mjs`.
//   cargo build -p uo-wasm --features dml-selftest --target wasm32-unknown-unknown --profile wasm
//   UO_WASM_PATH=<that .wasm> node --test crates/uo-wasm/tests/dml_selftest.test.mjs
//   UO_PLAYWRIGHT=<playwright package> UO_WASM_PATH=<that .wasm> node crates/uo-wasm/tests/browser/run.mjs --browser webkit \
//     crates/uo-wasm/tests/dml_selftest.test.mjs
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { join, dirname } from 'node:path';
import { fileURLToPath } from 'node:url';

const here = dirname(fileURLToPath(import.meta.url));
const wasmPath = process.env.UO_WASM_PATH ?? join(here, '../../../target/wasm32-unknown-unknown/wasm/uo_wasm.wasm');
const { instance } = await WebAssembly.instantiate(readFileSync(wasmPath));
const w = instance.exports;
const decoder = new TextDecoder();
const EVERY = Number(process.env.UO_DML_EVERY ?? 4);
const has = typeof w.uo_dml_selftest_run === 'function';

const OUT = 1 << 16;
const outPtr = has ? w.uo_alloc_bytes(OUT) : 0;
const out = (len) => decoder.decode(new Uint8Array(w.memory.buffer, outPtr, len));
const names = has ? Array.from({ length: w.uo_dml_selftest_group_count() }, (_, i) => out(w.uo_dml_selftest_group_name(i, outPtr, OUT))) : [];
const parse = (text) => Object.fromEntries(text.split('\n')[0].split(' ').map((kv) => kv.split('=')).map(([k, v]) => [k, Number(v)]));

if (!has) {
  test('the module has the dml-selftest exports', { skip: 'this build has no dml-selftest exports' }, () => {});
} else {
  test('the module lists the 20 groups of the committed captures', () => {
    assert.equal(names.length, 20);
    assert.ok(names.every((n) => n.length > 0));
  });

  let equal = 0;
  for (let i = 0; i < names.length; i++) {
    test(`${names[i]}: every ${EVERY}-th item of the capture replays equal inside the module`, () => {
      const t = performance.now();
      const text = out(w.uo_dml_selftest_run(i, EVERY, outPtr, OUT));
      const ms = Math.round(performance.now() - t);
      assert.ok(text.length > 0, 'no result');
      const r = parse(text);
      assert.equal(r.mismatches, 0, `${r.mismatches} mismatches:\n${text}`);
      assert.equal(r.accepted, 0, 'accepted differences are not expected');
      assert.ok(r.equal > 0, 'nothing was replayed');
      equal += r.equal;
      console.log(`# ${names[i]}: ${r.equal} facets equal in ${ms} ms (${r.missing} of the capture not replayed at every=${EVERY})`);
    });
  }

  test('the buffer is released', () => {
    console.log(`# total: ${equal} facets equal at every=${EVERY}`);
    assert.ok(equal > 0);
    w.uo_free_bytes(outPtr, OUT);
  });
}
