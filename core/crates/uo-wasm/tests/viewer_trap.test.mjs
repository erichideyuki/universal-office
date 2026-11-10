// A trap kills the instance (milestone-1 gate fixes, record uo-bindings.md): after a WebAssembly.RuntimeError the
// viewer module (`js/viewer.ts`) is dead, every object of it refuses calls or frees as a no-op, and the next
// `loadViewerModule` instantiates a fresh module that gives the same PDF. The trap comes from the count build's
// test-only export `uo_test_trap`:
//   cargo build -p uo-wasm --features count,write,render-fixture --target wasm32-unknown-unknown --profile wasm
//   UO_WASM_PATH=<that .wasm> node --expose-gc --test crates/uo-wasm/tests/viewer_trap.test.mjs
// Node 24 strips the types of viewer.ts (erasable TypeScript only).
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { join, dirname } from 'node:path';
import { fileURLToPath } from 'node:url';
import { loadViewerModule, __trapForTest } from '../js/viewer.ts';

const here = dirname(fileURLToPath(import.meta.url));
const repo = join(here, '../../../..');
const wasmPath = process.env.UO_WASM_PATH ?? join(here, '../../../target/wasm32-unknown-unknown/wasm/uo_wasm.wasm');
const bytes = readFileSync(wasmPath);
const mod0 = new WebAssembly.Module(bytes);
const exportsOf = WebAssembly.Module.exports(mod0).map((e) => e.name);
const skip = exportsOf.includes('uo_test_trap') && WebAssembly.Module.imports(mod0).some((i) => i.module === 'uo_host')
  ? false : 'this build has no uo_test_trap (--features count,write)';

const OPEN_FONTS = [
  ['Liberation Sans', 'site/assets/fonts', 'liberationsans'],
  ['Liberation Serif', 'shared/fonts/files', 'liberationserif'],
  ['Liberation Mono', 'shared/fonts/files', 'liberationmono'],
  ['Carlito', 'shared/fonts/files', 'carlito'],
  ['Caladea', 'shared/fonts/files', 'caladea'],
];
const VARIANTS = ['regular', 'bold', 'italic', 'boldItalic'];
const fontFiles = skip ? [] : OPEN_FONTS.map(([family, dir, stem]) => [family, VARIANTS.map((_, i) => readFileSync(join(repo, dir, `${stem}-${['regular', 'bold', 'italic', 'bolditalic'][i]}.ttf`)))]);
const withFonts = (env) => {
  for (const [family, files] of fontFiles) env.pushFamily(family, { source: 'installed', embeddable: true, faces: files.map((b, i) => ({ variant: VARIANTS[i], bytes: b })) });
  return env;
};
const docx = skip ? null : readFileSync(join(repo, 'core/crates/uo-docx-layout/tests/captures/f8/docs/f8__7__0.docx'));
const OPTS = { generation: 1, now: 1_768_478_400_000, tzOffsetMin: 0, hyphenationLanguages: ['pt-BR', 'en', 'es'] };
const load = () => loadViewerModule({ wasm: bytes, engine: 'wasm' });

test('a trap kills the instance and a fresh module gives the same PDF', { skip }, async () => {
  const mod = await load();
  const env = withFonts(mod.newEnv({ page: true }));
  const doc = mod.open(docx);
  const l = doc.layout(env, OPTS);
  const baseline = l.pdf({ compress: true });
  assert.ok(baseline.length > 1000);
  assert.equal(mod.dead, false);
  assert.deepEqual([mod.stats().handles, mod.stats().pageEnvs], [2, 1]);

  // the trap: a RuntimeError, and the instance is dead
  assert.throws(() => __trapForTest(), WebAssembly.RuntimeError);
  assert.equal(mod.dead, true);
  assert.equal(env.freed, true, 'the page environment of a dead instance reads as freed (the caller makes a new one)');
  assert.equal(l.freed, true);
  assert.equal(mod.stats().live, null);
  assert.deepEqual([mod.stats().handles, mod.stats().pageEnvs], [0, 0]);

  // frees of a dead instance are no-ops, any other call refuses
  l.free(); doc.free(); env.free();
  assert.throws(() => mod.open(docx), /stopped after a trap/);
  assert.throws(() => mod.newEnv(), /stopped after a trap/);
  assert.throws(() => l.pdf({ compress: true }), /stopped after a trap/);
  assert.equal(l.faceBytes(0), null);

  // the next load is a fresh instance, and the same document gives the same bytes
  const mod2 = await load();
  assert.notEqual(mod2, mod);
  assert.equal(mod2.dead, false);
  const env2 = withFonts(mod2.newEnv({ page: true }));
  const doc2 = mod2.open(docx);
  const l2 = doc2.layout(env2, OPTS);
  assert.deepEqual(l2.pdf({ compress: true }), baseline);
  l2.free(); doc2.free(); env2.free();
  const s = mod2.stats();
  assert.deepEqual([s.handles, s.pageEnvs], [0, 0]);
  assert.ok(s.live !== null && s.live >= 0);
  console.log(`trap: dead=${mod.dead}, fresh module live ${s.live} B, buffer ${s.buffer} B, PDF ${baseline.length} B equal`);

  // a second trap on the fresh module works the same way (no state leaks from the first instance)
  assert.throws(() => __trapForTest(), WebAssembly.RuntimeError);
  assert.equal(mod2.dead, true);
  const mod3 = await load();
  assert.notEqual(mod3, mod2);
  assert.equal(mod3.dead, false);
});
