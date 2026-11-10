// Hostile documents and a full font ledger in the wasm Write binding (milestone-1 gate fixes, record uo-bindings.md):
//  - nested tables / VML text boxes at the reader's `max_nesting` 64 (and 200 tables) lay out and export to PDF;
//  - M5: pushing distinct 8 MiB faces fills the 128 MiB font ledger of an environment until the push fails with
//    WriteError code 5 (OverBudget); the environment still lays out a fixture exactly like a fresh one, and freeing it
//    returns the live bytes to the baseline.
// Same setup as write_cancel.test.mjs:
//   cargo build -p uo-wasm --features count,write,render-fixture --target wasm32-unknown-unknown --profile wasm
//   UO_WASM_PATH=<that .wasm> node --expose-gc --test crates/uo-wasm/tests/write_hostile.test.mjs
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { join, dirname } from 'node:path';
import { fileURLToPath } from 'node:url';
import { writeImports, bindWrite, WriteError } from '../js/write.mjs';

const here = dirname(fileURLToPath(import.meta.url));
const repo = join(here, '../../../..');
const wasmPath = process.env.UO_WASM_PATH ?? join(here, '../../../target/wasm32-unknown-unknown/wasm/uo_wasm.wasm');
const bytes = readFileSync(wasmPath);
const mod = new WebAssembly.Module(bytes);
const has = WebAssembly.Module.imports(mod).some((i) => i.module === 'uo_host');
const skip = has ? false : 'this build has no write exports (--features write)';
const { imports, host } = writeImports();
const instance = has ? (await WebAssembly.instantiate(bytes, imports)).instance : null;
const w = instance?.exports;
const uo = has ? bindWrite(w, host) : null;
const counting = has && w.uo_memory_live_bytes() >= 0;
const live = () => w.uo_memory_live_bytes();
const settle = async () => {
  for (let i = 0; i < 5; i++) { globalThis.gc?.(); await new Promise((ok) => setTimeout(ok, 10)); }
  return live();
};

const OPEN_FONTS = [
  ['Liberation Sans', 'site/assets/fonts', 'liberationsans'],
  ['Liberation Serif', 'shared/fonts/files', 'liberationserif'],
  ['Liberation Mono', 'shared/fonts/files', 'liberationmono'],
  ['Carlito', 'shared/fonts/files', 'carlito'],
  ['Caladea', 'shared/fonts/files', 'caladea'],
];
const VARIANTS = ['regular', 'bold', 'italic', 'boldItalic'];
const fontFiles = has ? OPEN_FONTS.map(([family, dir, stem]) => [family, ['regular', 'bold', 'italic', 'bolditalic'].map((s) => readFileSync(join(repo, dir, `${stem}-${s}.ttf`)))]) : [];
const withFonts = (env) => {
  for (const [family, files] of fontFiles) env.pushFamily(family, { source: 'installed', embeddable: true, faces: files.map((b, i) => ({ variant: VARIANTS[i], bytes: b })) });
  return env;
};
const hostile = (name) => readFileSync(join(repo, 'core/crates/uo-ffi/tests/hostile', name));
const f8 = readFileSync(join(repo, 'core/crates/uo-docx-layout/tests/captures/f8/docs/f8__7__0.docx'));
const OPTS = { generation: 1, now: 1_768_478_400_000, tzOffsetMin: 0, hyphenationLanguages: ['pt-BR', 'en', 'es'] };
const pdfOf = (doc, env) => {
  const l = doc.layout(env, OPTS);
  try { return { pages: l.pageCount, pdf: Buffer.from(l.pdf({ compress: true, untrusted: true }).chunks.map((c) => Buffer.from(c)).reduce((a, c) => Buffer.concat([a, c]), Buffer.alloc(0))) }; } finally { l.free(); }
};

for (const name of ['nest_tables_64.docx', 'nest_tables_200.docx', 'nest_vml_64.docx', 'nest_mixed_64.docx']) {
  test(`nested ${name} lays out and exports to PDF`, { skip }, () => {
    const env = withFonts(uo.newEnv());
    const doc = uo.open(hostile(name));
    const r = pdfOf(doc, env);
    assert.ok(r.pages >= 1, `${r.pages} pages`);
    assert.ok(r.pdf.subarray(0, 5).toString() === '%PDF-');
    console.log(`${name}: ${r.pages} page(s), PDF ${r.pdf.length} B`);
    doc.free(); env.free();
  });
}

test('M5: a full font ledger refuses the push (WriteError 5), still lays out, and frees back to the baseline', { skip: skip || (counting ? false : 'needs the count build') }, async () => {
  // warm-up: the first layout fills the module's once-per-process state (hyphenation patterns, caches)
  {
    const e = withFonts(uo.newEnv()); const d = uo.open(f8);
    pdfOf(d, e); d.free(); e.free();
  }
  const base = await settle();
  const sans = fontFiles[0][1][0];
  const env = withFonts(uo.newEnv());
  let refusedAt = -1;
  for (let i = 0; i < 64 && refusedAt < 0; i++) {
    // distinct content keys, a valid sfnt in front: 8 MiB each
    const b = new Uint8Array(8 << 20);
    b.set(sans);
    new DataView(b.buffer).setUint32(b.length - 8, i + 1);
    try {
      env.pushFamily(`Fill${i}`, { source: 'installed', embeddable: false, faces: [{ variant: 'regular', bytes: b }] });
    } catch (e) {
      assert.ok(e instanceof WriteError, String(e));
      assert.equal(e.code, 5, e.message);
      refusedAt = i;
    }
  }
  assert.ok(refusedAt > 0, 'the ledger must refuse before 64 x 8 MiB');
  console.log(`M5: the font ledger refused the push of face #${refusedAt} (8 MiB each, 128 MiB budget)`);
  // the environment still works, and lays the fixture out exactly like a fresh one
  const doc = uo.open(f8);
  const filled = pdfOf(doc, env);
  const fresh = withFonts(uo.newEnv());
  const same = pdfOf(doc, fresh);
  assert.equal(filled.pages, same.pages);
  assert.ok(filled.pdf.equals(same.pdf), 'the PDF of the full environment equals the fresh one');
  doc.free(); fresh.free(); env.free();
  assert.equal(await settle(), base, 'live bytes back to the baseline');
});
