// Memory gate of uo-opc in wasm (stage 4, M2): the native soak's round trip (open → read and
// parse every XML part → edit → save → reopen → verify → parse again → drop) on real files of the
// repository, through the test-only export of the feature `opc-memtest`. The module's live bytes
// must be flat after warm-up and `memory.buffer.byteLength` must stop growing (wasm memory never
// shrinks: the high-water mark is a permanent cost, printed). Then the wasm defaults of `Limits`
// (gate 2, D7) and, last, S1: a tiny crafted zip must not grow the wasm memory (D1).
//   cargo build -p uo-wasm --features count,opc-memtest --target wasm32-unknown-unknown --profile wasm
//   node --test crates/uo-wasm/tests/opc_memory.test.mjs
// UO_OPC_SOAK=<iterations> overrides the 1,000 iterations.
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { join, dirname } from 'node:path';
import { fileURLToPath } from 'node:url';

const here = dirname(fileURLToPath(import.meta.url));
const repo = join(here, '../../../..');
const wasmPath = join(here, '../../../target/wasm32-unknown-unknown/wasm/uo_wasm.wasm');
const { instance } = await WebAssembly.instantiate(readFileSync(wasmPath));
const w = instance.exports;
const ITERATIONS = Number(process.env.UO_OPC_SOAK ?? 1000);
const EVERY = 50;

const files = [
  ['write/packages/docx-layout/test/oracle/justify4/sw-16-8.docx', 0],
  ['write/packages/docx-layout/test/oracle/emwidth/ew-emb.docx', 0],
  ['sheet/samples/showcase.xlsx', 1],
  ['slides/samples/stress-media.pptx', 2],
  ['slides/samples/legacy/lo-reference.ppt', 3],
].map(([rel, kind]) => ({ rel, kind, bytes: new Uint8Array(readFileSync(join(repo, rel))) }));

// moves a copy of `bytes` into the module and runs the round trip (the export takes the buffer)
function roundTrip(bytes, kind) {
  const p = w.uo_alloc_bytes(bytes.length);
  new Uint8Array(w.memory.buffer, p, bytes.length).set(bytes);
  return w.uo_opc_memtest_round_trip(p, bytes.length, kind);
}
const live = () => w.uo_memory_live_bytes();

test('the module counts memory and has the opc-memtest export', () => {
  assert.ok(live() >= 0, 'build uo-wasm with --features count');
  assert.equal(typeof w.uo_opc_memtest_round_trip, 'function', 'build uo-wasm with --features opc-memtest');
  for (const f of files) assert.ok(roundTrip(f.bytes, f.kind) > 0, `${f.rel} failed`);
});

test(`uo-opc round trip, ${ITERATIONS} iterations: live bytes flat, wasm memory stops growing (M2)`, () => {
  const lives = [live()];
  const sizes = [w.memory.buffer.byteLength];
  const t = performance.now();
  for (let i = 0; i < ITERATIONS; i++) {
    for (const f of files) roundTrip(f.bytes, f.kind);
    if ((i + 1) % EVERY === 0) { lives.push(live()); sizes.push(w.memory.buffer.byteLength); }
  }
  const ms = performance.now() - t;
  console.log(`# ${ITERATIONS} iterations in ${Math.round(ms)} ms; live bytes every ${EVERY}: ${[...new Set(lives)].join(', ')}; ` +
    `byteLength: ${sizes[0]} at the start, ${sizes.at(-1)} at the end (high-water mark)`);
  // live bytes: exactly flat from the first sample (the warm-up ran in the previous test)
  assert.equal(new Set(lives).size, 1, `live bytes changed: ${lives.join(', ')}`);
  // wasm memory: no growth after the first sample
  assert.equal(new Set(sizes).size, 1, `wasm memory kept growing: ${sizes.join(', ')}`);
});

// a zip whose single deflated entry has no compressed bytes and declares `size` bytes
function crafted(name, size) {
  const n = new TextEncoder().encode(name);
  const local = new Uint8Array(30 + n.length);
  const lv = new DataView(local.buffer);
  lv.setUint32(0, 0x04034b50, true); lv.setUint16(4, 20, true); lv.setUint16(8, 8, true);
  lv.setUint32(22, size, true); lv.setUint16(26, n.length, true); local.set(n, 30);
  const central = new Uint8Array(46 + n.length);
  const cv = new DataView(central.buffer);
  cv.setUint32(0, 0x02014b50, true); cv.setUint16(4, 20, true); cv.setUint16(6, 20, true); cv.setUint16(10, 8, true);
  cv.setUint32(24, size, true); cv.setUint16(28, n.length, true); central.set(n, 46);
  const end = new Uint8Array(22);
  const ev = new DataView(end.buffer);
  ev.setUint32(0, 0x06054b50, true); ev.setUint16(8, 1, true); ev.setUint16(10, 1, true);
  ev.setUint32(12, central.length, true); ev.setUint32(16, local.length, true);
  const out = new Uint8Array(local.length + central.length + end.length);
  out.set(local); out.set(central, local.length); out.set(end, local.length + central.length);
  return out;
}

test('D7: the wasm32 defaults of Limits (512 MiB per binary part, 1 GiB in total)', () => {
  const field = (i) => w.uo_opc_memtest_default_limit(i);
  assert.equal(field(0), 256 * 2 ** 20, 'XML part: min(256 MiB, the binary cap)');
  assert.equal(field(1), 512 * 2 ** 20, 'binary part');
  assert.equal(field(2), 2 ** 30, 'total');
  assert.equal(field(3), 65535);
  assert.equal(field(4), 1000);
  assert.equal(field(5), 20000000);
});

// before D1 this raised the wasm memory by about twice the declared size, permanently
test('S1: a tiny zip with an empty deflate stream and a declared size does not grow the memory (D1)', () => {
  const size = 256 << 20;
  const zip = crafted('word/document.xml', size);
  const before = w.memory.buffer.byteLength;
  const liveBefore = live();
  const r = roundTrip(zip, 0);
  const after = w.memory.buffer.byteLength;
  console.log(`# S1: a ${zip.length} B docx declaring a ${size} B document.xml: result ${r}; wasm memory ${before} -> ${after} B; ` +
    `live bytes back to ${live()} (from ${liveBefore})`);
  assert.equal(live(), liveBefore);
  // rejected as corrupt before anything near the declared size is allocated
  assert.equal(r, -1);
  assert.equal(after, before, 'the wasm memory grew');
});
