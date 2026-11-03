// Memory gate of uo-drawingml in wasm (hybrid mode, continuous verification): M2 (the soak of the module
// record -- presets through every function, colour sequences, custom guides, flatten and fill, the four effect
// masks on 512 x 512, shade_bevel, the four adjusts on 1024 x 768 RGBA, blip and patterns -- leaves the live bytes
// flat at 0 bytes and `memory.buffer.byteLength` stops growing after the first rounds), the big effects at 1 and
// 4 Mpx (8 and 12 Mpx with UO_DML_BIG=1) with the peak live bytes of each call against the M3 budgets of the
// record and against the phone budget of the record (G12: 128 MiB of live bytes per effect call), the wasm32 limit
// rows and the worst case per limit. wasm memory never shrinks, so the high-water mark of `byteLength` is printed
// at every stage: that is what a phone tab keeps. The same file runs in WebKit through `browser/run.mjs`.
//   cargo build -p uo-wasm --features dml-memtest --target wasm32-unknown-unknown --profile wasm
//   UO_WASM_PATH=<that .wasm> node --test crates/uo-wasm/tests/dml_memory.test.mjs
//   UO_PLAYWRIGHT=<playwright package> UO_WASM_PATH=<that .wasm> node crates/uo-wasm/tests/browser/run.mjs --browser webkit \
//     crates/uo-wasm/tests/dml_memory.test.mjs
// UO_DML_SOAK=<rounds> overrides the 60 rounds of the soak (the plan asks 200); UO_DML_BIG=1 adds 8 and 12 Mpx and the
// morph to the padded-plane limit (about 0.5 GB: Node only).
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { join, dirname } from 'node:path';
import { fileURLToPath } from 'node:url';

const here = dirname(fileURLToPath(import.meta.url));
const wasmPath = process.env.UO_WASM_PATH ?? join(here, '../../../target/wasm32-unknown-unknown/wasm/uo_wasm.wasm');
const { instance } = await WebAssembly.instantiate(readFileSync(wasmPath));
const w = instance.exports;
const ROUNDS = Number(process.env.UO_DML_SOAK ?? 60);
const BIG = process.env.UO_DML_BIG === '1';
const EVERY = 5;
const MiB = 1048576;
const PHONE_CALL_BUDGET = 128 * MiB;
const has = typeof w.uo_dml_memtest_round === 'function';
const skip = has ? false : 'this build has no dml-memtest exports';
const live = () => w.uo_memory_live_bytes();
const heap = () => w.memory.buffer.byteLength;
const mb = (n) => (n / MiB).toFixed(2);
const sameValues = (xs) => new Set(xs).size === 1;

test('the module counts memory and has the dml-memtest exports', { skip }, () => {
  assert.ok(live() >= 0, 'build uo-wasm with --features dml-memtest (implies count)');
});

let start = 0;
test('the long-lived inputs of the soak move into the module (512 and 256 px discs, a 1024 x 768 RGBA image)', { skip }, () => {
  start = live();
  w.uo_dml_memtest_prepare();
  console.log(`# prepared: live ${mb(live() - start)} MB (expected 1.0 + 0.25 + 3.0 MB), byteLength ${mb(heap())} MB`);
  assert.ok(live() - start > 4 * MiB);
});

test(`M2 soak in wasm, ${ROUNDS} rounds: live bytes flat at 0 bytes, memory.buffer.byteLength stops growing`, { skip }, () => {
  const parts = ['presets', 'colour', 'custom guides', 'flatten and fill', 'effects 512x512', 'shade_bevel 256x256', 'adjusts 1024x768', 'blip and patterns'];
  assert.equal(w.uo_dml_memtest_part_count(), parts.length);
  const sizes = [];
  const ms = new Array(parts.length).fill(0);
  for (let i = 0; i < 2; i++) {
    for (let p = 0; p < parts.length; p++) assert.ok(w.uo_dml_memtest_part(p, i) >= 0);
    sizes.push(heap());
  }
  const lives = [live()];
  const heaps = [heap()];
  const t = performance.now();
  for (let i = 0; i < ROUNDS; i++) {
    for (let p = 0; p < parts.length; p++) {
      const t0 = performance.now();
      assert.ok(w.uo_dml_memtest_part(p, i + 2) >= 0);
      ms[p] += performance.now() - t0;
    }
    if ((i + 1) % EVERY === 0) { lives.push(live()); heaps.push(heap()); }
  }
  const total = performance.now() - t;
  console.log(`# soak: ${ROUNDS} rounds in ${Math.round(total)} ms (${Math.round(total / ROUNDS)} ms per round); parts per round (ms): ` +
    parts.map((n, p) => `${n} ${(ms[p] / ROUNDS).toFixed(1)}`).join(', '));
  console.log(`# soak: live bytes above the start every ${EVERY} rounds: ${[...new Set(lives.map((v) => v - lives[0]))].join(', ')}; byteLength after round 0, 1: ${sizes.map(mb).join(', ')} MB, then ` +
    `${[...new Set(heaps)].map(mb).join(', ')} MB (high-water ${mb(Math.max(...heaps))} MB; the phone budget of one call is ${mb(PHONE_CALL_BUDGET)} MB)`);
  assert.ok(sameValues(lives), `live bytes changed: ${lives.join(', ')}`);
  assert.ok(sameValues(heaps), `wasm memory kept growing: ${heaps.map(mb).join(', ')}`);
});

test('worst case per limit with the wasm32 limits: peak live bytes, and what the module keeps (never shrinks)', { skip }, () => {
  const cases = [
    ['flatten_path into its 4 Mi point limit (8,192 ellipse arcs; refused at the limit)', 3],
    ['gaussian_blur at the kernel limit (sigma 1365, a 64 x 64 mask)', 0],
  ];
  if (BIG) cases.push(['morph to the padded-plane limit of 2^25 pixels (a 512 x 512 silhouette)', 0]);
  const rows = [];
  for (let kind = 0; kind < cases.length; kind++) {
    const before = live();
    const h0 = heap();
    const peak = w.uo_dml_memtest_hostile(kind, BIG ? 1 : 0);
    const allocated = w.uo_dml_memtest_big_allocated();
    assert.equal(live(), before, `${cases[kind][0]} left live bytes`);
    const code = w.uo_dml_memtest_last_code();
    assert.equal(code, cases[kind][1], `${cases[kind][0]}: error ${code}`);
    rows.push(`${cases[kind][0]}: ${code ? `refused (error ${code}) after ` : ''}peak ${mb(peak)} MB, allocated ${mb(allocated)} MB, byteLength ${mb(h0)} -> ${mb(heap())} MB`);
  }
  console.log(`# worst case (wasm32 limits):\n#   ${rows.join('\n#   ')}`);
  console.log(`# wasm memory high-water after the worst cases: ${mb(heap())} MB`);
});

test('wasm32 limits: the defaults and the error rows that need no big allocation allocate nothing before their check', { skip }, () => {
  assert.equal(w.uo_dml_memtest_default_limit(0), 1 << 25, 'max_mask_pixels');
  assert.equal(w.uo_dml_memtest_default_limit(1), 1 << 25, 'max_image_pixels');
  assert.equal(w.uo_dml_memtest_default_limit(2), 4096, 'max_kernel_radius');
  assert.equal(w.uo_dml_memtest_default_limit(3), 1 << 22, 'max_path_points');
  assert.equal(w.uo_dml_memtest_default_limit(4), 1 << 30, 'max_fill_work');
  assert.equal(w.uo_dml_memtest_default_limit(5), 2000, 'max_custom_guides');
  const expected = [1, 2, 1, 1, 4];
  const names = ['Mask::new(8192, 8192)', 'blur sigma 2000', 'morph radius 1e5', 'shade_bevel on a mask of width u32::MAX', 'fill_polygons: 70,000 edges over 65,536 rows'];
  const rows = [];
  for (let row = 0; row < expected.length; row++) {
    const before = live();
    const h = heap();
    assert.equal(w.uo_dml_memtest_limit_row(row), expected[row], names[row]);
    assert.equal(live(), before, `${names[row]} left live bytes`);
    rows.push(`${names[row]}: error ${expected[row]}, byteLength ${mb(h)} -> ${mb(heap())} MB`);
  }
  console.log(`# limit rows (1 mask pixels, 2 kernel radius, 4 fill work):\n#   ${rows.join('\n#   ')}`);
  assert.equal(w.uo_dml_memtest_limit_row(99), -1);
});

// M3 budgets of the record in bytes per pixel (peak live bytes above the start of one call, native, 1 Mpx and 4 Mpx),
// the asserted ceiling is 5% over
const KINDS = [
  { name: 'outer shadow', budget: 12.01 },
  { name: 'inner shadow', budget: 12.01 },
  { name: 'glow (r 4)', budget: 16.4 },
  { name: 'soft edge', budget: 16.6 },
  { name: 'morph (r 4)', budget: 16.5 },
  { name: 'blur (sigma 4)', budget: 8.1 },
  { name: 'shade_bevel', budget: 16.5 },
  { name: 'sharpen', budget: 28.1 },
  { name: 'soften', budget: 24.1 },
  { name: 'distance transform', budget: 12.1 },
];
const SIDES = BIG ? [1024, 2048, 2896, 3464] : [1024, 2048];

test('big effects: peak live bytes per call inside the M3 budgets, nothing left behind, against the phone budget', { skip }, () => {
  assert.equal(w.uo_dml_memtest_big_count(), KINDS.length);
  const rows = [];
  for (const side of SIDES) {
    const n = side * side;
    for (let kind = 0; kind < KINDS.length; kind++) {
      const before = live();
      const peak = w.uo_dml_memtest_big(kind, side);
      const allocated = w.uo_dml_memtest_big_allocated();
      assert.equal(live(), before, `${KINDS[kind].name} at ${side} left live bytes`);
      assert.equal(w.uo_dml_memtest_last_code(), 0, `${KINDS[kind].name} at ${side} was refused`);
      assert.ok(peak > 0);
      const over = peak > PHONE_CALL_BUDGET ? ' OVER the 128 MiB phone budget' : '';
      rows.push(`${(n / 1e6).toFixed(1)} Mpx ${KINDS[kind].name}: peak ${mb(peak)} MB (${(peak / n).toFixed(2)} B/px), allocated ${mb(allocated)} MB, byteLength ${mb(heap())} MB${over}`);
      if (side <= 2048) {
        assert.ok(peak <= KINDS[kind].budget * n * 1.05, `${KINDS[kind].name} at ${side}: ${(peak / n).toFixed(2)} B/px over the budget ${KINDS[kind].budget}`);
        assert.ok(peak <= PHONE_CALL_BUDGET, `${KINDS[kind].name} at ${side}: peak ${mb(peak)} MB over the phone budget`);
      }
    }
  }
  console.log(`# big effects (wasm32 limits, peak live bytes above the start of the call; the input silhouette or image is held by the caller):\n#   ${rows.join('\n#   ')}`);
  console.log(`# wasm memory high-water after the big effects: ${mb(heap())} MB`);
});

test('dropping the long-lived inputs returns the module to its start', { skip }, () => {
  w.uo_dml_memtest_reset();
  assert.equal(live(), start);
  console.log(`# final wasm memory high-water: ${mb(heap())} MB (live ${live()} bytes)`);
});
