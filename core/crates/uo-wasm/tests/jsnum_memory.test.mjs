// Memory gate of uo-jsnum in wasm (stage 4): M1 (the numeric and parsing functions allocate 0 bytes
// in wasm too, the string path frees what it allocates), M2 (a soak of every numeric function and
// of the string functions over real and hostile buffers: live bytes flat and `memory.buffer.byteLength`
// stops growing after warm-up; wasm memory never shrinks, so the high-water mark is printed) and the
// stack depth of the deepest functions (the shadow stack is painted below the call and the lowest
// touched byte is found; wasm32 has 1 MiB of it). The same file runs in WebKit through
// `browser/run.mjs`.
//   cargo build -p uo-wasm --features jsnum-memtest --target wasm32-unknown-unknown --profile wasm
//   node --test crates/uo-wasm/tests/jsnum_memory.test.mjs
//   node crates/uo-wasm/tests/browser/run.mjs --browser webkit crates/uo-wasm/tests/jsnum_memory.test.mjs
// UO_JSNUM_SOAK=<iterations> overrides the 1,000 iterations; UO_WASM_PATH picks another .wasm.
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { join, dirname } from 'node:path';
import { fileURLToPath } from 'node:url';

const here = dirname(fileURLToPath(import.meta.url));
const wasmPath = process.env.UO_WASM_PATH ?? join(here, '../../../target/wasm32-unknown-unknown/wasm/uo_wasm.wasm');
const { instance } = await WebAssembly.instantiate(readFileSync(wasmPath));
const w = instance.exports;
const ITERATIONS = Number(process.env.UO_JSNUM_SOAK ?? 1000);
const EVERY = 50;
const live = () => w.uo_memory_live_bytes();
const allocated = () => w.uo_jsnum_memtest_allocated_bytes();
const encoder = new TextEncoder();

const small = [
  '', ' ', ' ﻿ 　', '12.5', '-12.5e-3', '+.5', '5.', '.', '1e', '1e400', '-1e-400', 'Infinity', 'NaN',
  'abc', '0x', '0x1F', '0X1f ', '-0x1F', '0o17', '0b101', '0b102', '00012', '9007199254740993',
  '123456789012345678901234567890', '1_000', '\u{1f600}', '12\u{1f600}', 'a\u{1f600}b\u{1f600}c', 'éèê',
  '  7 8\t\n9  ',
];
const big = [
  '1'.repeat(32768), 'z'.repeat(32768), ' '.repeat(32768) + '1', '0x' + 'f'.repeat(32768), '1e' + '9'.repeat(32768),
  '\u{1f600}'.repeat(8192), 'a '.repeat(16384),
];

// a buffer moved into the module: owned by JS until `release`
function put(text) {
  const bytes = encoder.encode(text);
  const ptr = w.uo_alloc_bytes(bytes.length);
  new Uint8Array(w.memory.buffer, ptr, bytes.length).set(bytes);
  return { ptr, len: bytes.length, release: () => w.uo_free_bytes(ptr, bytes.length) };
}

test('the module counts memory and has the jsnum-memtest exports', () => {
  assert.ok(live() >= 0, 'build uo-wasm with --features jsnum-memtest (implies count)');
  assert.equal(typeof w.uo_jsnum_memtest_numeric, 'function');
  assert.equal(typeof w.uo_jsnum_memtest_strings, 'function');
});

test('M1 in wasm: every numeric function allocates 0 bytes; string functions leave live bytes unchanged', () => {
  w.uo_jsnum_memtest_numeric(0);
  const a0 = allocated();
  const l0 = live();
  for (let seed = 0; seed < 100; seed++) w.uo_jsnum_memtest_numeric(seed);
  assert.equal(allocated(), a0, 'numeric functions allocated');
  assert.equal(live(), l0);

  // to_number, parse_int, trim*, split_js_space, utf16_len, utf16_slice (borrowed) and js_space_len allocate nothing;
  // kinds 20..25 and 27 of the generic export are exactly those
  const buffers = [...small, ...big].map(put);
  const l1 = live();
  for (const b of buffers) {
    for (const kind of [20, 21, 22, 23, 24, 25, 27]) {
      const before = allocated();
      w.uo_jsnum_memtest_call(kind, 0, 0, b.ptr, b.len);
      assert.equal(allocated(), before, `kind ${kind} allocated on a ${b.len} B string`);
    }
    // the all-in-one export owns one cut-surrogate String and drops it: live bytes unchanged
    const before = live();
    w.uo_jsnum_memtest_strings(b.ptr, b.len);
    assert.equal(live(), before);
  }
  assert.equal(live(), l1);
  for (const b of buffers) b.release();
  assert.equal(live(), l0, 'the buffers were released');
  console.log(`# M1 wasm: 100 numeric batches allocated 0 B; ${small.length + big.length} strings (up to ${big[0].length} B) x 7 kinds allocated 0 B`);
});

test(`M2 in wasm, ${ITERATIONS} iterations: live bytes flat, memory.buffer.byteLength stops growing`, () => {
  const buffers = [...small, ...big].map(put);
  const batch = (i) => {
    for (let k = 0; k < 20; k++) w.uo_jsnum_memtest_numeric(i + k);
    for (const b of buffers) w.uo_jsnum_memtest_strings(b.ptr, b.len);
  };
  batch(0);
  batch(1);
  const lives = [live()];
  const sizes = [w.memory.buffer.byteLength];
  const start = sizes[0];
  const t = performance.now();
  for (let i = 0; i < ITERATIONS; i++) {
    batch(i);
    if ((i + 1) % EVERY === 0) { lives.push(live()); sizes.push(w.memory.buffer.byteLength); }
  }
  const ms = performance.now() - t;
  const inputBytes = buffers.reduce((n, b) => n + b.len, 0);
  console.log(`# ${ITERATIONS} iterations (20 numeric batches + ${buffers.length} strings, ${inputBytes} input bytes) in ${Math.round(ms)} ms; ` +
    `live bytes every ${EVERY}: ${[...new Set(lives)].join(', ')}; byteLength ${start} (after warm-up) -> ${sizes.at(-1)}, ` +
    `distinct sizes ${[...new Set(sizes)].join(', ')}`);
  assert.equal(new Set(lives).size, 1, `live bytes changed: ${lives.join(', ')}`);
  assert.equal(new Set(sizes).size, 1, `wasm memory kept growing: ${sizes.join(', ')}`);
  for (const b of buffers) b.release();
});

// painted stack: the lowest byte below the probe address that a call touched
function stackDepth(call) {
  const sp = w.uo_jsnum_memtest_stack_probe();
  const span = 262144;
  assert.ok(sp > span, `unexpected stack address ${sp}`);
  let depth = 0;
  for (const pattern of [0xa5, 0x5a]) {
    const view = new Uint8Array(w.memory.buffer, sp - span, span);
    view.fill(pattern);
    call();
    const after = new Uint8Array(w.memory.buffer, sp - span, span);
    let lowest = 0;
    while (lowest < span && after[lowest] === pattern) lowest++;
    assert.ok(lowest > 4096, 'the call used more than the painted stack');
    depth = Math.max(depth, span - lowest);
  }
  return depth;
}

test('stack depth of the deepest functions on wasm32 (shadow stack of 1 MiB)', () => {
  const xs = [0, 0.5, 1, 1.5707963267948966, 3.141592653589793, 1e6, 1e22, 1e300, -1e300, 5e-324, 1.7976931348623157e308, NaN, Infinity];
  const names = [
    'sin', 'cos', 'tan', 'asin', 'acos', 'atan', 'atan2', 'exp', 'expm1', 'log', 'log1p', 'log2', 'log10', 'sinh', 'cosh', 'tanh', 'atanh',
    'cbrt', 'pow', 'hypot3',
  ];
  const results = [];
  let worst = { name: '', depth: 0 };
  for (let kind = 0; kind < names.length; kind++) {
    let depth = 0;
    for (const x of xs) for (const y of [2, 0.5, -3.7, 1e300]) depth = Math.max(depth, stackDepth(() => w.uo_jsnum_memtest_call(kind, x, y, 0, 0)));
    results.push(`${names[kind]} ${depth}`);
    if (depth > worst.depth) worst = { name: names[kind], depth };
  }
  const int32 = stackDepth(() => w.uo_jsnum_memtest_call(28, 1e300, 0, 0, 0));
  results.push(`to_int32 ${int32}`);
  console.log(`# stack bytes per numeric function (max over 13 x 4 inputs, ±16 B): ${results.join(', ')}`);
  console.log(`# deepest numeric function: ${worst.name}, ${worst.depth} B`);
  assert.ok(worst.depth < 4096, `${worst.name} uses ${worst.depth} B of stack`);
});

test('stack depth of the parsers and text functions on wasm32, on hostile inputs', () => {
  // a halfway case of 770 digits sends the f64 parse to its slow path; long runs of digits, of
  // zeros, of nines, with exponent and with junk
  const half = '1.' + '0'.repeat(767) + '5' + '0'.repeat(10);
  const inputs = [
    '', '1', '12.5e-3', '1'.repeat(25), '1'.repeat(400), '9'.repeat(100_000), '0'.repeat(100_000) + '1', '1.' + '9'.repeat(100_000),
    '1.' + '0'.repeat(100_000) + 'e-99', half, '2.2250738585072011360574097967091319759348195463516456480234261097248222220210769455165295239081350879141491589130396211068700864386945946455276572074078206217433799881410632673292535522868813721490129811224514518898490572223072852551331557550159143974763979834412767214e-308',
    '1e' + '9'.repeat(100_000), '0x' + 'f'.repeat(100_000), '0b' + '1'.repeat(100_000), 'z'.repeat(100_000), '6'.repeat(100_000),
    ' '.repeat(100_000) + '1', 'a '.repeat(50_000), '\u{1f600}'.repeat(50_000), 'a' + '\u{1f600}'.repeat(50_000) + 'b',
  ];
  const names = { 20: 'to_number', 21: 'parse_int(10)', 22: 'parse_int(7)', 23: 'parse_int(16)', 24: 'parse_int(36)', 25: 'split_js_space', 26: 'utf16_slice', 27: 'trim' };
  const worst = {};
  const buffers = inputs.map(put);
  for (const b of buffers) {
    for (const kind of Object.keys(names)) {
      const depth = stackDepth(() => w.uo_jsnum_memtest_call(Number(kind), 0, 0, b.ptr, b.len));
      worst[kind] = Math.max(worst[kind] ?? 0, depth);
    }
  }
  for (const b of buffers) b.release();
  const line = Object.entries(worst).map(([kind, d]) => `${names[kind]} ${d}`).join(', ');
  console.log(`# stack bytes per parser/text function (max over ${inputs.length} inputs, ±16 B): ${line}`);
  assert.ok(Math.max(...Object.values(worst)) < 8192, `a parser uses ${Math.max(...Object.values(worst))} B of stack`);
});
