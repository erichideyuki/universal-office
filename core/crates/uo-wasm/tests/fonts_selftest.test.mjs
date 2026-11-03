// Parity self-test of uo-fonts inside the wasm module (stage 4): the module recomputes the facets of the
// committed captures (crates/uo-fonts/tests/parity/fonts.cap and legacy_kern.cap) with the same code the
// native replay uses, over the generated fonts it builds itself and over the real fonts and hyphenation
// data JS feeds it; native, wasm and the capture (frozen TS on Node 24.15.0; .NET 10 for G16) must give
// the same digests. The same file runs in WebKit through `browser/run.mjs`.
//   cargo build -p uo-wasm --features fonts-selftest --target wasm32-unknown-unknown --profile wasm
//   UO_WASM_PATH=<that .wasm> node --test crates/uo-wasm/tests/fonts_selftest.test.mjs
//   UO_PLAYWRIGHT=<playwright package> UO_WASM_PATH=<that .wasm> node crates/uo-wasm/tests/browser/run.mjs --browser webkit \
//     crates/uo-wasm/tests/fonts_selftest.test.mjs
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { join, dirname } from 'node:path';
import { fileURLToPath } from 'node:url';

const here = dirname(fileURLToPath(import.meta.url));
const wasmPath = process.env.UO_WASM_PATH ?? join(here, '../../../target/wasm32-unknown-unknown/wasm/uo_wasm.wasm');
const parity = join(here, '../../uo-fonts/tests/parity');
const repo = join(here, '../../../..');
const { instance } = await WebAssembly.instantiate(readFileSync(wasmPath));
const w = instance.exports;
const encoder = new TextEncoder();
const decoder = new TextDecoder();

function readCapture(path) {
  const records = new Map();
  for (const line of decoder.decode(readFileSync(path)).split('\n')) {
    if (line === '' || line.startsWith('#') || line.startsWith('@')) continue;
    const [key, ...facets] = line.split('\t');
    const r = {};
    for (const f of facets) { const i = f.indexOf('='); r[f.slice(0, i)] = f.slice(i + 1); }
    records.set(key, r);
  }
  return records;
}
const skip = typeof w.uo_fonts_selftest_generated_count === 'function' ? false : 'this build has no fonts-selftest exports';
const fonts = readCapture(join(parity, 'fonts.cap'));
const legacy = readCapture(join(parity, 'legacy_kern.cap'));

const OUT = 1 << 16;
const outPtr = w.uo_alloc_bytes(OUT);
const out = (len) => decoder.decode(new Uint8Array(w.memory.buffer, outPtr, len));
/** Moves bytes into a module buffer, runs `fn(ptr, len)` and releases it. */
function withBytes(bytes, fn) {
  const size = Math.max(bytes.length, 1);
  const ptr = w.uo_alloc_bytes(size);
  new Uint8Array(w.memory.buffer, ptr, bytes.length).set(bytes);
  try { return fn(ptr, bytes.length); } finally { w.uo_free_bytes(ptr, size); }
}
const withText = (text, fn) => withBytes(encoder.encode(text), fn);
const facetMap = (text) => new Map(text.split('\n').filter(Boolean).map((l) => { const i = l.indexOf('\t'); return [l.slice(0, i), l.slice(i + 1)]; }));

function compare(key, rec, got, bad) {
  let checked = 0;
  for (const [facet, value] of got) {
    checked++;
    if (rec[facet] === 'hang') continue;
    if (rec[facet] !== value) bad.push(`${key} [${facet}]: wasm ${value}, capture ${rec[facet]}`);
  }
  return checked;
}

test('generated fonts built in the module: every facet equals the capture (gen/b quirk fonts, gen/c kerning and ligature fonts)', { skip }, () => {
  const count = w.uo_fonts_selftest_generated_count();
  const bad = [];
  let checked = 0;
  for (let i = 0; i < count; i++) {
    const key = out(w.uo_fonts_selftest_generated_key(i, outPtr, OUT));
    const rec = fonts.get(key);
    assert.ok(rec, `${key} is not in the capture`);
    const hang = encoder.encode(rec['subset.hang'] ?? '');
    const text = withBytes(hang, (p, l) => out(w.uo_fonts_selftest_generated_facets(i, p, l, outPtr, OUT)));
    checked += compare(key, rec, facetMap(text), bad);
  }
  assert.deepEqual(bad, [], `${bad.length} of ${checked} facets differ`);
  assert.equal(count, 82);
  console.log(`# ${count} generated fonts, ${checked} facets equal`);
});

const REAL = [
  'shared/fonts/files/carlito-regular.ttf',
  'shared/fonts/files/carlito-bolditalic.ttf',
  'shared/fonts/files/caladea-regular.ttf',
  'shared/fonts/files/liberationmono-bold.ttf',
  'site/assets/fonts/liberationsans-italic.ttf',
  'slides/packages/slides/demo/fonts/os/amatic-sc-bold.ttf',
];

test('open fonts of the repository fed through the boundary: every facet equals the capture (full depth)', { skip }, () => {
  const bad = [];
  let checked = 0;
  for (const rel of REAL) {
    const key = `file/${rel}`;
    const rec = fonts.get(key);
    assert.ok(rec, `${key} is not in the capture`);
    const bytes = new Uint8Array(readFileSync(join(repo, rel)));
    assert.equal(rec.in.split(':')[0], String(bytes.length));
    const text = withBytes(bytes, (p, l) => out(w.uo_fonts_selftest_font_facets(p, l, 0, 0, 0, outPtr, OUT)));
    checked += compare(key, rec, facetMap(text), bad);
  }
  assert.deepEqual(bad, [], `${bad.length} of ${checked} facets differ`);
  console.log(`# ${REAL.length} open fonts, ${checked} facets equal`);
});

test('G16: the legacy kern of the C# reader (generated cases in the module, open fonts fed by JS): wasm equals the capture', { skip }, () => {
  const bad = [];
  let checked = 0;
  const count = w.uo_fonts_selftest_legacy_count();
  for (let i = 0; i < count; i++) {
    const key = out(w.uo_fonts_selftest_legacy_key(i, outPtr, OUT));
    const got = out(w.uo_fonts_selftest_legacy_generated(i, outPtr, OUT));
    checked++;
    if (got !== legacy.get(key)?.legacykern) bad.push(`${key}: wasm ${got}, capture ${legacy.get(key)?.legacykern}`);
  }
  for (const [key, rec] of legacy) {
    if (!key.startsWith('file/')) continue;
    const bytes = new Uint8Array(readFileSync(join(repo, key.slice(5))));
    const got = withBytes(bytes, (p, l) => out(w.uo_fonts_selftest_legacy(p, l, outPtr, OUT)));
    checked++;
    if (got !== rec.legacykern) bad.push(`${key}: wasm ${got}, capture ${rec.legacykern}`);
  }
  assert.deepEqual(bad, [], `${bad.length} of ${checked} differ`);
  assert.ok(checked > 40);
  console.log(`# ${checked} legacy kern facets equal`);
});

test('hyphenation: the pattern fields of the seven data files and the first 750 oracle words of each language equal the capture', { skip }, () => {
  const words = decoder.decode(readFileSync(join(parity, 'fonts_oracle_words.txt'))).split('\n').filter(Boolean).slice(0, 750);
  const bad = [];
  let checked = 0;
  for (const id of ['pt', 'en-us', 'es', 'fr', 'de-1996', 'it', 'nl']) {
    const file = readFileSync(join(here, '../../uo-fonts/data/hyphen', `${id}.txt`));
    const rec = fonts.get(`hyphen/${id}`);
    const fields = withBytes(file, (p, l) => out(w.uo_fonts_selftest_hyphen_fields(p, l, outPtr, OUT)));
    checked++;
    if (fields !== rec.fields) bad.push(`${id} fields: wasm ${fields}, capture ${rec.fields}`);
    for (let chunk = 0; chunk < 3; chunk++) {
      const got = withBytes(file, (fp, fl) => withText(words.join('\n'), (wp, wl) => out(w.uo_fonts_selftest_hyphen_chunk(fp, fl, wp, wl, chunk, outPtr, OUT))));
      checked++;
      if (got !== rec[`chunk${chunk}`]) bad.push(`${id} chunk${chunk}: wasm ${got}, capture ${rec[`chunk${chunk}`]}`);
    }
  }
  assert.deepEqual(bad, [], `${bad.length} of ${checked} differ`);
  console.log(`# ${checked} hyphenation facets equal`);
});

test('the buffer is released', { skip }, () => {
  w.uo_free_bytes(outPtr, OUT);
});
