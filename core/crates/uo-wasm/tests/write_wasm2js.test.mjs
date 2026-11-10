// The wasm2js engine (a plain-JS build of the viewer module, for pages whose CSP forbids WebAssembly) against
// the WebAssembly engine, through loadWrite (js/write.mjs): over the 12 committed fixtures every page message
// (render and Write sections), annotations, navigation, link boxes, page info, the first embedded face and the PDF
// (compressed and not) are byte-identical, the PDF equals the native checksum where one is pinned, `engine:'auto'`
// picks WebAssembly here and falls back to js when the compile is refused, and errors carry the same codes.
// Since G22 (OptF64 is an integer, 0 = absent) both the default opt-level 3 module and the z one (cargo
// CARGO_PROFILE_WASM_OPT_LEVEL=z) convert correctly. The .js must be built with tools/build-wasm2js.mjs, which
// refuses a module holding NaN-payload constants (JavaScript has no NaN payloads):
//   cargo build -p uo-wasm --features write --target wasm32-unknown-unknown --profile wasm
//   node core/crates/uo-wasm/tools/build-wasm2js.mjs --in <that .wasm> --out <uo_wasm.js>
//   UO_WASM_PATH=<.wasm> UO_WASM2JS_PATH=<uo_wasm.js> node --test crates/uo-wasm/tests/write_wasm2js.test.mjs
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync, existsSync } from 'node:fs';
import { join, dirname, resolve } from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';
import { loadWrite, canCompileWasm, WriteError } from '../js/write.mjs';
import { fnv } from '../js/render.mjs';
import { loadAssets, collect, digest, firstDifference, FIXTURES, PINNED_PDF } from './wasm2js_common.mjs';

const here = dirname(fileURLToPath(import.meta.url));
const repo = join(here, '../../../..');
const wasmPath = process.env.UO_WASM_PATH ?? join(here, '../../../target/wasm32-unknown-unknown/wasm/uo_wasm.wasm');
const jsPath = process.env.UO_WASM2JS_PATH ?? join(here, '../../../target/wasm2js/uo_wasm.js');
const skip = existsSync(jsPath) && existsSync(wasmPath) ? false : `needs ${wasmPath} and ${jsPath} (tools/build-wasm2js.mjs)`;

const assets = skip ? null : await loadAssets(async (p) => new Uint8Array(readFileSync(join(repo, p))));
const wasmBytes = skip ? null : new Uint8Array(readFileSync(wasmPath));
const jsUrl = pathToFileURL(resolve(jsPath)).href;
const viaWasm = skip ? null : await loadWrite({ engine: 'wasm', wasm: wasmBytes });
const viaJs = skip ? null : await loadWrite({ engine: 'js', js: jsUrl });

test('both engines load, and say which they are', { skip }, async () => {
  assert.equal(viaWasm.engine, 'wasm');
  assert.equal(viaJs.engine, 'js');
  assert.ok(canCompileWasm());
  assert.equal((await loadWrite({ wasm: wasmBytes, js: jsUrl })).engine, 'wasm');
  assert.equal((await loadWrite({ engine: 'js', js: () => import(jsUrl) })).engine, 'js');
  await assert.rejects(loadWrite({ engine: 'js' }), /no js/);
  await assert.rejects(loadWrite({ engine: 'nope' }), /unknown engine/);
});

test('a refused WebAssembly compile falls back to the js engine (auto)', { skip }, async () => {
  const real = WebAssembly.Module;
  WebAssembly.Module = function () { throw new EvalError("Refused to compile or instantiate WebAssembly module because 'unsafe-eval' is not an allowed source of script"); };
  try {
    assert.equal(canCompileWasm(), false);
    assert.equal((await loadWrite({ wasm: wasmBytes, js: jsUrl })).engine, 'js');
  } finally { WebAssembly.Module = real; }
  const inst = WebAssembly.instantiate;
  WebAssembly.instantiate = async () => { throw new EvalError('refused'); }; // the probe passes, the real compile throws
  try {
    assert.equal((await loadWrite({ wasm: wasmBytes, js: jsUrl })).engine, 'js');
    await assert.rejects(loadWrite({ wasm: wasmBytes }), EvalError); // nothing to fall back to
  } finally { WebAssembly.instantiate = inst; }
});

test('the 12 fixtures give byte-identical results in both engines', { skip }, () => {
  const lines = [];
  for (const key of FIXTURES) {
    const a = collect(viaWasm.uo, assets, key), b = collect(viaJs.uo, assets, key);
    assert.equal(firstDifference(a, b), null, key);
    if (PINNED_PDF[key]) assert.equal(fnv(b.pdf), PINNED_PDF[key], `${key} pdf vs native`);
    lines.push(`${key}: ${a.pages} pages, ${a.pdf.length} B pdf, ${digest(b).split(' ').slice(0, 3).join(' ')}`);
  }
  console.log(lines.join('\n'));
});

test('errors carry the same code and stage in the js engine', { skip }, () => {
  for (const { uo } of [viaWasm, viaJs]) {
    assert.throws(() => uo.open(new Uint8Array([1, 2, 3])), (e) => e instanceof WriteError && e.code === 3 && e.stage === 'read' && e.detail.length > 0);
    const env = uo.newEnv();
    const doc = uo.open(assets.docs.f8__7__0);
    assert.throws(() => doc.layout(env, { generation: 1 }), (e) => e instanceof WriteError && e.code === 11 && e.stage === 'layout');
    assert.throws(() => doc.layout(env, { view: 'nope' }), (e) => e instanceof WriteError && e.code === 1);
    doc.free(); env.free();
  }
});

test('the js engine memory is flat across sessions', { skip }, () => {
  const sizes = [];
  for (let i = 0; i < 6; i++) { collect(viaJs.uo, assets, 'f8__7__0'); sizes.push(viaJs.exports.memory.buffer.byteLength); }
  assert.ok(sizes.slice(2).every((v) => v === sizes[2]), `memory.buffer: ${sizes}`);
});
