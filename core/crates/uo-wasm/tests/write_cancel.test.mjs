// G4 (uo-bindings.md): cancellation inside the layout of the wasm Write binding. `cancelCheck` of
// js/write.mjs sets flag bit 9 and answers the host import `uo_host.layout_poll`; true stops the
// layout with WriteError code 10. Same setup as write_api.test.mjs:
//   cargo build -p uo-wasm --features count,write --target wasm32-unknown-unknown --profile wasm
//   UO_WASM_PATH=<that .wasm> node --expose-gc --test crates/uo-wasm/tests/write_cancel.test.mjs
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync, readdirSync, statSync } from 'node:fs';
import { join, dirname } from 'node:path';
import { fileURLToPath } from 'node:url';
import { writeImports, bindWrite, WriteError } from '../js/write.mjs';

const here = dirname(fileURLToPath(import.meta.url));
const repo = join(here, '../../../..');
const wasmPath = process.env.UO_WASM_PATH ?? join(here, '../../../target/wasm32-unknown-unknown/wasm/uo_wasm.wasm');
const bytes = readFileSync(wasmPath);
const mod = new WebAssembly.Module(bytes);
const has = WebAssembly.Module.imports(mod).some((i) => i.module === 'uo_host');
const hasPoll = WebAssembly.Module.imports(mod).some((i) => i.module === 'uo_host' && i.name === 'layout_poll');
const skip = has && hasPoll ? false : 'this build has no write exports or no uo_host.layout_poll (--features write)';
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
const fontFiles = has ? OPEN_FONTS.map(([family, dir, stem]) => [family, ['regular', 'bold', 'italic', 'bolditalic'].map((s) => readFileSync(join(repo, dir, `${stem}-${s}.ttf`)))]) : [];
const VARIANTS = ['regular', 'bold', 'italic', 'boldItalic'];
const withFonts = (env) => {
  for (const [family, files] of fontFiles) env.pushFamily(family, { source: 'installed', embeddable: true, faces: files.map((b, i) => ({ variant: VARIANTS[i], bytes: b })) });
  return env;
};
const docsDir = join(repo, 'core/crates/uo-docx-layout/tests/captures/f8/docs');
// the largest committed fixture: the most polls
const biggest = has ? readdirSync(docsDir).filter((f) => f.endsWith('.docx')).map((f) => [f, statSync(join(docsDir, f)).size]).sort((a, b) => b[1] - a[1])[0][0] : '';
const fixture = (key) => readFileSync(join(docsDir, key.endsWith('.docx') ? key : `${key}.docx`));
const OPTS = { generation: 1, now: 1_768_478_400_000, tzOffsetMin: 0, hyphenationLanguages: ['pt-BR', 'en', 'es'] };

const pages = (layout) => {
  const out = [];
  for (let i = 0; i < layout.pageCount; i++) out.push(Buffer.from(layout.paint.encodePage(i).take().data).toString('base64'));
  return out;
};
const cancelled = (e) => e instanceof WriteError && e.code === 10 && e.stage === '';
// the number of polls of an uncancelled layout, then half of it: the k that cancels in the middle
const midK = (doc, env) => {
  let n = 0;
  doc.layout(env, { ...OPTS, cancelCheck: () => { n++; return false; } }).free();
  return Math.max(1, Math.floor(n / 2));
};
const session = () => ({ env: withFonts(uo.newEnv()), doc: uo.open(fixture(biggest)) });

test('cancelCheck returning true stops the layout with code 10', { skip }, () => {
  const { env, doc } = session();
  assert.throws(() => doc.layout(env, { ...OPTS, cancelCheck: () => true }), cancelled);
  doc.free(); env.free();
});

test('a counting cancelCheck that says no changes nothing', { skip }, () => {
  const { env, doc } = session();
  const plain = doc.layout(env, OPTS);
  let polls = 0;
  const counted = doc.layout(env, { ...OPTS, cancelCheck: () => { polls++; return false; } });
  assert.ok(polls >= 1, `${polls} polls`);
  assert.deepEqual(pages(counted), pages(plain));
  console.log(`G4 wasm: ${polls} polls in ${biggest}`);
  plain.free(); counted.free(); doc.free(); env.free();
});

test('the k-th poll stops it, deterministically', { skip }, () => {
  const { env, doc } = session();
  let n = 0;
  doc.layout(env, { ...OPTS, cancelCheck: () => { n++; return false; } }).free();
  const k = Math.max(1, Math.floor(n / 2));
  for (let run = 0; run < 2; run++) {
    let polls = 0;
    assert.throws(() => doc.layout(env, { ...OPTS, cancelCheck: () => ++polls >= k }), cancelled);
    assert.equal(polls, k);
  }
  doc.free(); env.free();
});

test('a layout after a cancelled one equals a fresh environment\'s', { skip }, () => {
  const a = session();
  const k = midK(a.doc, a.env);
  let polls = 0;
  assert.throws(() => a.doc.layout(a.env, { ...OPTS, cancelCheck: () => ++polls >= k }), cancelled);
  assert.equal(polls, k);
  const after = a.doc.layout(a.env, OPTS);
  const b = session();
  const fresh = b.doc.layout(b.env, OPTS);
  assert.deepEqual(pages(after), pages(fresh));
  after.free(); fresh.free(); a.doc.free(); a.env.free(); b.doc.free(); b.env.free();
});

test('a throwing cancelCheck counts as a cancel and does not trap the module', { skip }, () => {
  const { env, doc } = session();
  assert.throws(() => doc.layout(env, { ...OPTS, cancelCheck: () => { throw new Error('boom'); } }), cancelled);
  assert.equal(host.poll, null);
  const l = doc.layout(env, OPTS);
  assert.ok(l.pageCount > 0);
  l.free(); doc.free(); env.free();
});

test('a nested layout() from cancelCheck is refused, not re-entered', { skip }, () => {
  const { env, doc } = session();
  let nested = null;
  assert.throws(() => doc.layout(env, { ...OPTS, cancelCheck: () => { try { doc.layout(env, OPTS); } catch (e) { nested = e; } return true; } }), cancelled);
  assert.ok(nested instanceof Error && /cancelCheck/.test(nested.message), String(nested));
  const l = doc.layout(env, OPTS);
  assert.ok(l.pageCount > 0);
  l.free(); doc.free(); env.free();
});

test('M1: 20 cancelled layouts leave the live bytes where they were', { skip: skip || (counting ? false : 'needs the counting build (--features count)') }, async () => {
  const { env, doc } = session();
  const k = midK(doc, env); // also the warm-up: faces, hyphenation
  const before = await settle();
  for (let i = 0; i < 20; i++) {
    let polls = 0;
    assert.throws(() => doc.layout(env, { ...OPTS, cancelCheck: () => ++polls >= k }), cancelled);
    assert.equal(polls, k);
  }
  assert.equal(await settle(), before);
  doc.free(); env.free();
});
