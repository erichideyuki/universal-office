// Parity self-test of uo-jsnum inside the wasm module (stage 4): the module recomputes the digests
// of the committed captures (crates/uo-jsnum/tests/captures: jsnum.capture, adv.capture, pow.capture)
// with the generators of the native tests, and runs the string corpora that JS feeds it; native,
// wasm and the capture (Node 24.15.0 / V8 13.6.233.17) must give the same results. The same file
// runs in WebKit through `browser/run.mjs`.
//   cargo build -p uo-wasm --features jsnum-selftest --target wasm32-unknown-unknown --profile wasm
//   node --test crates/uo-wasm/tests/jsnum_selftest.test.mjs
//   UO_PLAYWRIGHT=<playwright package> node crates/uo-wasm/tests/browser/run.mjs --browser webkit \
//     crates/uo-wasm/tests/jsnum_selftest.test.mjs
// UO_WASM_PATH picks another .wasm; UO_JSNUM_FULL=1 (Node only) also replays the full-size digests
// (10^7 samples per numeric distribution, 2 x 10^5 strings per family, 2 x 10^6 math samples):
// about 1.2 x 10^9 numeric samples, a few minutes.
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { join, dirname } from 'node:path';
import { fileURLToPath } from 'node:url';

const here = dirname(fileURLToPath(import.meta.url));
const wasmPath = process.env.UO_WASM_PATH ?? join(here, '../../../target/wasm32-unknown-unknown/wasm/uo_wasm.wasm');
const captureDir = join(here, '../../uo-jsnum/tests/captures');
const { instance } = await WebAssembly.instantiate(readFileSync(wasmPath));
const w = instance.exports;
const FULL = process.env.UO_JSNUM_FULL === '1';
const encoder = new TextEncoder();
const decoder = new TextDecoder();

// ── captures ────────────────────────────────────────────────────────────────────────────────────
function readCapture(name) {
  const records = new Map();
  const meta = new Map();
  for (const line of decoder.decode(readFileSync(join(captureDir, name))).split('\n')) {
    if (line === '' || line.startsWith('#')) continue;
    if (line.startsWith('@')) { const i = line.indexOf(' '); meta.set(line.slice(1, i), line.slice(i + 1)); continue; }
    const [key, ...facets] = line.split('\t');
    const r = {};
    for (const f of facets) { const i = f.indexOf('='); r[f.slice(0, i)] = f.slice(i + 1); }
    records.set(key, r);
  }
  return { records, meta };
}
const jsnum = readCapture('jsnum.capture');
const adv = readCapture('adv.capture');
const pow = readCapture('pow.capture');

// ── the boundary: buffers owned by JS, plain numbers ────────────────────────────────────────────
const IN = 1 << 16;
const OUT = 1 << 14;
const inPtr = w.uo_alloc_bytes(IN);
const outPtr = w.uo_alloc_bytes(OUT);
const view = (ptr, len) => new Uint8Array(w.memory.buffer, ptr, len);
const read = (len) => decoder.decode(view(outPtr, len));
function putString(s) {
  const bytes = encoder.encode(s);
  assert.ok(bytes.length <= IN, 'string too long for the test buffer');
  view(inPtr, bytes.length).set(bytes);
  return bytes.length;
}
const key = (group, index) => read(w.uo_jsnum_selftest_key(group, index, outPtr, OUT));
const digest = (group, index, n) => read(w.uo_jsnum_selftest_digest(group, index, n, outPtr, OUT));
const number = (s) => read(w.uo_jsnum_selftest_number(inPtr, putString(s), outPtr));
const parseInt_ = (s, radix) => read(w.uo_jsnum_selftest_parse_int(inPtr, putString(s), radix, outPtr));
const textOp = (s, op) => read(w.uo_jsnum_selftest_text(inPtr, putString(s), op, outPtr, OUT));

const GROUPS = { numeric: 0, adv: 1, ext: 2, pow: 3, conv: 4 };

test('numeric records of jsnum.capture: wasm digests equal the capture (10^5 samples; 10^7 with UO_JSNUM_FULL=1)', () => {
  assert.equal(jsnum.meta.get('tiers-agree').includes('--no-opt --no-maglev'), true);
  const count = w.uo_jsnum_selftest_count(GROUPS.numeric);
  let checked = 0;
  const bad = [];
  for (let i = 0; i < count; i++) {
    const k = key(GROUPS.numeric, i);
    const rec = jsnum.records.get(k);
    assert.ok(rec, `${k} is not in the capture`);
    const sizes = [['small', Number(rec.n_small)]];
    if (FULL) sizes.push(['full', Number(rec.n_full)]);
    for (const [facet, n] of sizes) {
      const got = digest(GROUPS.numeric, i, n);
      if (got !== rec[facet]) bad.push(`${k} ${facet}: wasm ${got}, capture ${rec[facet]}`);
      checked++;
    }
  }
  assert.deepEqual(bad, [], `${bad.length} of ${checked} digests differ`);
  assert.equal(count, 104);
  console.log(`# ${count} numeric records, ${checked} digests equal`);
});

test('pow distributions of pow.capture (platform: Linux, glibc 2.39, FMA masked): wasm digests equal the capture', () => {
  const count = w.uo_jsnum_selftest_count(GROUPS.pow);
  const bad = [];
  let checked = 0;
  for (let i = 0; i < count; i++) {
    const k = key(GROUPS.pow, i);
    const rec = pow.records.get(k);
    assert.ok(rec, `${k} is not in the capture`);
    for (const [facet, n] of [['n5', 100_000], ...(FULL ? [['n7', 10_000_000]] : [])]) {
      const got = digest(GROUPS.pow, i, n);
      if (got !== rec[facet]) bad.push(`${k} ${facet}: wasm ${got}, capture ${rec[facet]}`);
      checked++;
    }
  }
  assert.deepEqual(bad, [], `${bad.length} of ${checked} digests differ`);
  assert.equal(count, 8);
  console.log(`# ${count} pow distributions, ${checked} digests equal`);
});

test('generated strings of adv.capture (Number and parseInt in 9 radices): wasm digests equal the capture', () => {
  const count = w.uo_jsnum_selftest_count(GROUPS.adv);
  const bad = [];
  let checked = 0;
  for (let i = 0; i < count; i++) {
    const k = key(GROUPS.adv, i);
    const rec = adv.records.get(k);
    assert.ok(rec, `${k} is not in the capture`);
    const sizes = [['small', Number(rec.n_small)]];
    if (FULL) sizes.push(['full', Number(rec.n_full)]);
    for (const [facet, n] of sizes) {
      const got = digest(GROUPS.adv, i, n);
      if (got !== rec[facet]) bad.push(`${k} ${facet}: wasm ${got}, capture ${rec[facet]}`);
      checked++;
    }
  }
  assert.deepEqual(bad, [], `${bad.length} of ${checked} digests differ`);
  assert.equal(count, 13);
  console.log(`# ${count} string families, ${checked} digests equal`);
});

test('targeted math distributions of adv.capture: wasm digests equal the capture', () => {
  const count = w.uo_jsnum_selftest_count(GROUPS.ext);
  const bad = [];
  let checked = 0;
  for (let i = 0; i < count; i++) {
    const k = key(GROUPS.ext, i);
    const rec = adv.records.get(k);
    assert.ok(rec, `${k} is not in the capture`);
    const sizes = [['small', Number(rec.n_small)]];
    if (FULL) sizes.push(['full', Number(rec.n_full)]);
    for (const [facet, n] of sizes) {
      const got = digest(GROUPS.ext, i, n);
      if (got !== rec[facet]) bad.push(`${k} ${facet}: wasm ${got}, capture ${rec[facet]}`);
      checked++;
    }
  }
  assert.deepEqual(bad, [], `${bad.length} of ${checked} digests differ`);
  assert.equal(count, 60);
  console.log(`# ${count} math distributions, ${checked} digests equal`);
});

test('integer conversions over every exponent: wasm digest equals the capture', () => {
  assert.equal(digest(GROUPS.conv, 0, 0), adv.records.get('conv/exp_sweep').digest);
});

test('the corpus of jsnum.capture fed through the boundary: Number, parseInt, trim, length', () => {
  const bad = [];
  let checked = 0;
  for (const [k, rec] of jsnum.records) {
    if (!k.startsWith('corpus/')) continue;
    const s = JSON.parse(rec.s);
    const cmp = (what, got, want) => { checked++; if (got !== want) bad.push(`${k} ${JSON.stringify(s).slice(0, 60)} ${what}: wasm ${got}, capture ${want}`); };
    cmp('num', number(s), rec.num);
    for (const facet of Object.keys(rec)) {
      if (facet.startsWith('pi_')) cmp(facet, parseInt_(s, Number(facet.slice(3)) | 0), rec[facet]);
    }
    cmp('trim', textOp(s, 0), JSON.parse(rec.trim));
    cmp('trim_start', textOp(s, 1), JSON.parse(rec.trim_start));
    cmp('trim_end', textOp(s, 2), JSON.parse(rec.trim_end));
    cmp('len', textOp(s, 3), rec.len);
  }
  assert.deepEqual(bad, [], `${bad.length} of ${checked} differ`);
  assert.ok(checked > 3000);
  console.log(`# ${checked} corpus comparisons equal`);
});

test('the exact midpoints of neighbouring doubles fed through the boundary: Number and parseInt(s, 10)', () => {
  const bad = [];
  let checked = 0;
  for (const [k, rec] of adv.records) {
    if (!k.startsWith('hard/')) continue;
    const s = JSON.parse(rec.s);
    checked += 2;
    if (number(s) !== rec.num) bad.push(`${k} num: wasm ${number(s)}, capture ${rec.num}`);
    if (parseInt_(s, 10) !== rec.pi_10) bad.push(`${k} pi_10: wasm ${parseInt_(s, 10)}, capture ${rec.pi_10}`);
  }
  assert.deepEqual(bad, [], `${bad.length} of ${checked} differ`);
  assert.ok(checked >= 1900);
  console.log(`# ${checked} midpoint comparisons equal`);
});

test('the buffers are released', () => {
  w.uo_free_bytes(inPtr, IN);
  w.uo_free_bytes(outPtr, OUT);
});
