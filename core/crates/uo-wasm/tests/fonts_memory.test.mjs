// Memory gate of uo-fonts in wasm (stage 4): M2 (the fresh scenario of the native soak, 0 bytes of
// live growth, and the long-lived engine within its caches; `memory.buffer.byteLength` stops growing
// after warm-up, and since wasm memory never shrinks the high-water mark is printed), the worst-case
// fonts judged for a phone (peak live bytes with the wasm32 limits and the memory the module keeps
// afterwards), and the return to the start when the corpus and the engine are dropped. The scenarios
// are the code of `uo-fonts/tests/memory_support`, compiled into the module. The same file runs in
// WebKit through `browser/run.mjs`.
//   cargo build -p uo-wasm --features fonts-memtest --target wasm32-unknown-unknown --profile wasm
//   node --test crates/uo-wasm/tests/fonts_memory.test.mjs
//   node crates/uo-wasm/tests/browser/run.mjs --browser webkit crates/uo-wasm/tests/fonts_memory.test.mjs
// UO_FONTS_SOAK=<rounds> overrides the 300 rounds (the native soak runs 1,000); UO_WASM_PATH picks another .wasm.
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { join, dirname } from 'node:path';
import { fileURLToPath } from 'node:url';

const here = dirname(fileURLToPath(import.meta.url));
const repo = join(here, '../../../..');
const wasmPath = process.env.UO_WASM_PATH ?? join(here, '../../../target/wasm32-unknown-unknown/wasm/uo_wasm.wasm');
const { instance } = await WebAssembly.instantiate(readFileSync(wasmPath));
const w = instance.exports;
const ROUNDS = Number(process.env.UO_FONTS_SOAK ?? 300);
const EVERY = 50;
const live = () => w.uo_memory_live_bytes();
const heap = () => w.memory.buffer.byteLength;
const mb = (n) => (n / 1048576).toFixed(2);

const families = ['caladea', 'carlito', 'liberationmono', 'liberationserif'];
const styles = ['bold', 'bolditalic', 'italic', 'regular'];
const faceFiles = families.flatMap((f) => styles.map((s) => `shared/fonts/files/${f}-${s}.ttf`));
const patternIds = ['pt', 'en-us', 'es', 'fr', 'de-1996', 'it', 'nl'];

function put(bytes) {
  const p = w.uo_alloc_bytes(bytes.length);
  new Uint8Array(w.memory.buffer, p, bytes.length).set(bytes);
  return p;
}

function loadCorpus() {
  for (const f of faceFiles) {
    const bytes = readFileSync(join(repo, f));
    w.uo_fonts_memtest_add_face(put(bytes), bytes.length);
  }
  for (const id of patternIds) {
    const bytes = readFileSync(join(repo, `core/crates/uo-fonts/data/hyphen/${id}.txt`));
    assert.equal(w.uo_fonts_memtest_add_pattern(put(bytes), bytes.length), 1);
  }
  return w.uo_fonts_memtest_build_corpus();
}

test('the module counts memory and has the fonts-memtest exports', () => {
  assert.ok(live() >= 0, 'build uo-wasm with --features fonts-memtest (implies count)');
  assert.equal(typeof w.uo_fonts_memtest_fresh_round, 'function');
});

let start = 0;
test('the corpus moves into the module', () => {
  start = live();
  const n = loadCorpus();
  assert.equal(n, faceFiles.length + 6);
  console.log(`# corpus: ${n} faces, 7 pattern files; live ${mb(live() - start)} MB, byteLength ${mb(heap())} MB`);
});

test(`M2 fresh in wasm, ${ROUNDS} rounds: live bytes flat at 0 bytes, memory.buffer.byteLength stops growing`, () => {
  w.uo_fonts_memtest_fresh_round(0);
  w.uo_fonts_memtest_fresh_round(1);
  const lives = [live()];
  const sizes = [heap()];
  const t = performance.now();
  for (let i = 0; i < ROUNDS; i++) {
    assert.ok(w.uo_fonts_memtest_fresh_round(i) >= 0);
    if ((i + 1) % EVERY === 0) { lives.push(live()); sizes.push(heap()); }
  }
  const ms = performance.now() - t;
  console.log(`# fresh: ${ROUNDS} rounds in ${Math.round(ms)} ms (${Math.round(ms / ROUNDS)} ms per round); live bytes above the start every ${EVERY}: ` +
    `${[...new Set(lives.map((v) => v - lives[0]))].join(', ')}; byteLength ${mb(sizes[0])} MB (after warm-up) -> ${mb(sizes.at(-1))} MB, distinct sizes ${[...new Set(sizes)].map(mb).join(', ')} MB`);
  assert.equal(new Set(lives).size, 1, `live bytes changed: ${lives.join(', ')}`);
  assert.equal(new Set(sizes).size, 1, `wasm memory kept growing: ${sizes.join(', ')}`);
});

test(`M2 long-lived in wasm, ${ROUNDS} rounds: bounded by the caches, byteLength stops growing, back to the start when dropped`, () => {
  const before = live();
  assert.equal(w.uo_fonts_memtest_engine_new(), 1);
  const built = live() - before;
  const lives = [];
  const sizes = [];
  const t = performance.now();
  for (let i = 0; i < ROUNDS; i++) {
    assert.ok(w.uo_fonts_memtest_engine_round() >= 0);
    if ((i + 1) % EVERY === 0) { lives.push(live() - before - built); sizes.push(heap()); }
  }
  const ms = performance.now() - t;
  const third = Math.max(1, Math.floor(lives.length / 3));
  const mid = Math.max(...lives.slice(third, 2 * third));
  const last = Math.max(...lives.slice(2 * third));
  console.log(`# long-lived: engine built ${mb(built)} MB; ${ROUNDS} rounds in ${Math.round(ms)} ms; ${w.uo_fonts_memtest_engine_cached_words()} cached words; ` +
    `live above the engine every ${EVERY} (MB): ${lives.map(mb).join(', ')}; byteLength ${mb(sizes[0])} -> ${mb(sizes.at(-1))} MB`);
  assert.ok(last <= mid + 8 * 1048576, `no growth after warm-up: mid third ${mb(mid)} MB, last third ${mb(last)} MB`);
  assert.ok(Math.max(...lives) <= 90 * 1048576, 'bounded by the native caps (outlines 39.7 MB + sessions 41.6 MB)');
  if (sizes.length > 3) {
    const warm = sizes.slice(Math.ceil(sizes.length / 2));
    assert.ok(Math.max(...warm) - Math.min(...warm) <= 4 * 1048576, `wasm memory kept growing after warm-up: ${warm.map(mb).join(', ')}`);
  }
  w.uo_fonts_memtest_engine_drop();
  assert.equal(live(), before, 'dropping the engine returns every byte');
});

test('worst case per limit with the wasm32 limits: peak live bytes, and what the module keeps (never shrinks)', () => {
  const names = ['gsub-30000-ligature-sets', 'gpos-1500-class0-entries', 'gpos-pair-map-to-the-cap (1024 x 1024 = 2^20)', 'outline-4-composites-asking-for-1M-points (stopped at 2^16)',
    'subset-overlapping-glyphs (60 MB output)', 'ttc-every-table-is-the-whole-file'];
  assert.equal(w.uo_fonts_memtest_hostile_count(), names.length);
  const rows = [];
  for (let kind = 0; kind < names.length; kind++) {
    const before = live();
    const h0 = heap();
    const peak = w.uo_fonts_memtest_hostile(kind);
    assert.equal(live(), before, `${names[kind]} left live bytes`);
    rows.push(`${names[kind]}: peak ${mb(peak)} MB, byteLength ${mb(h0)} -> ${mb(heap())} MB`);
    assert.ok(peak > 0 && peak < 200 * 1048576, `${names[kind]} peaked at ${mb(peak)} MB`);
  }
  console.log(`# worst case (wasm32 limits):\n#   ${rows.join('\n#   ')}`);
  console.log(`# wasm memory high-water after the worst cases: ${mb(heap())} MB`);
});

test('dropping the corpus returns the module to its start', () => {
  w.uo_fonts_memtest_reset();
  assert.equal(live(), start);
});
