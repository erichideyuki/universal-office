// Builds the no-WebAssembly engine of the viewer module: uo_wasm.wasm -> (wasm-opt) -> wasm2js -> a plain ES
// module `export default function uoWasm2js(imports) -> exports` that js/write.mjs loads with
// `loadWrite({ engine: 'js' })`. For hosts whose Content-Security-Policy blocks WebAssembly (no
// 'wasm-unsafe-eval'; Safari 15 does not know that keyword at all). Output goes under core/target/, never
// into git. Record: core/docs/modules/uo-bindings.md, "No-WebAssembly fallback (wasm2js) and wasm-opt".
//
//   cargo build -p uo-wasm --features write --target wasm32-unknown-unknown --profile wasm
//   npm install --prefix core/target/tools/binaryen binaryen        # once; wasm-opt and wasm2js, not a repo dependency
//   node core/crates/uo-wasm/tools/build-wasm2js.mjs --in <uo_wasm.wasm> --out <uo_wasm.js>
//        [--wasm-opt Oz|O3|none]  (default Oz; the optimised .wasm is kept next to --out as <out>.opt.wasm)
//        [--wasm2js-opt Oz|O3|none] (default Oz: 30% off the minified size for 2-3x the build time; needs the wasm-opt step first, wasm2js -O on a raw rustc module aborts on unaligned stores)
//        [--no-minify]            (default: esbuild minifies whitespace, syntax and identifiers)
//        [--allow-nan-payload]    (skip the guard: wasm2js prints every NaN constant as the plain `NaN`; a NaN-payload marker in the Rust source, e.g. an OptF64 "absent", changes meaning, G22)
//        [--binaryen <bin dir>] [--esbuild <esbuild package dir>]   (or UO_BINARYEN_BIN / UO_ESBUILD)
//
// The module must hold no i64 at its export/import boundary (true for uo-wasm: lengths and pointers are i32).
import { execFileSync } from 'node:child_process';
import { readFileSync, writeFileSync, existsSync, mkdirSync, unlinkSync } from 'node:fs';
import { createRequire } from 'node:module';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

const here = dirname(fileURLToPath(import.meta.url));
const core = resolve(here, '../../..');
const args = process.argv.slice(2);
const opt = (name, dflt) => { const i = args.indexOf(name); return i < 0 ? dflt : args[i + 1]; };
const input = opt('--in'), out = opt('--out');
if (!input || !out) { console.error('usage: build-wasm2js.mjs --in <uo_wasm.wasm> --out <uo_wasm.js> [--wasm-opt Oz|O3|none] [--wasm2js-opt Oz|O3|none] [--no-minify] [--allow-nan-payload]'); process.exit(2); }
const wasmOpt = opt('--wasm-opt', 'Oz'), js2Opt = opt('--wasm2js-opt', 'Oz');
const bin = opt('--binaryen', process.env.UO_BINARYEN_BIN ?? join(core, 'target/tools/binaryen/node_modules/binaryen/bin'));
const exe = (n) => join(bin, n);
if (!existsSync(exe('wasm2js'))) { console.error(`no wasm2js in ${bin}: npm install --prefix core/target/tools/binaryen binaryen`); process.exit(2); }
// what rustc emits for wasm32-unknown-unknown today; wasm-opt/wasm2js refuse a module whose features are not enabled
const FEATURES = ['--enable-bulk-memory', '--enable-sign-ext', '--enable-nontrapping-float-to-int', '--enable-mutable-globals', '--enable-multivalue', '--enable-reference-types'];
const run = (n, a) => execFileSync(process.execPath, ["--stack-size=8000", exe(n), ...a], { stdio: ['ignore', 'inherit', 'inherit'], windowsHide: true, maxBuffer: 1 << 30 }); // the binaryen CLIs are node scripts

mkdirSync(dirname(resolve(out)), { recursive: true });
let wasm = resolve(input);
{
  // --alignment-lowering is not optional: rustc emits i32/i64/f64 loads and stores with align=1 (unaligned reads of
  // byte buffers), and an asm.js heap view silently rounds such an address down. Without it the opt-level 3 build
  // gave different page messages than WebAssembly (the "z" build happened not to). It runs after the optimiser.
  const optimised = resolve(out) + '.opt.wasm';
  run('wasm-opt', [...(wasmOpt === 'none' ? [] : [`-${wasmOpt}`]), '--alignment-lowering', ...FEATURES, wasm, '-o', optimised]);
  wasm = optimised;
  // The NaN-constant census. JavaScript has no NaN payloads: wasm2js prints every NaN constant as `NaN`, so a
  // payload marker that LLVM materialised as an f64/f32 constant (the old OptF64 "absent" at opt-level 3) is read
  // back as a present value. Only the canonical NaNs may appear.
  const wat = resolve(out) + '.opt.wat';
  run('wasm-dis', [optimised, ...FEATURES, '-o', wat]);
  const census = new Map();
  for (const m of readFileSync(wat, 'utf8').matchAll(/\b(?:f32|f64)\.const -?nan(?::0x[0-9a-f]+)?/g)) census.set(m[0], (census.get(m[0]) ?? 0) + 1);
  unlinkSync(wat);
  const CANONICAL = new Set(['f64.const nan:0x8000000000000', 'f32.const nan:0x400000', 'f64.const nan', 'f32.const nan']);
  const bad = [...census].filter(([k]) => !CANONICAL.has(k));
  console.log('wasm2js: NaN constants ' + ([...census].map(([k, n]) => `${n} x ${k}`).join(', ') || 'none'));
  if (bad.length && !args.includes('--allow-nan-payload')) {
    console.error('wasm2js: NaN-payload constants in the module: ' + bad.map(([k, n]) => `${n} x ${k}`).join(', '));
    console.error('JavaScript has no NaN payloads; a NaN-payload marker in the Rust source changes meaning in this build (G22, uo-bindings.md "No-WebAssembly fallback"). --allow-nan-payload skips this check.');
    process.exit(2);
  }
}
const raw = resolve(out) + '.raw.js';
run('wasm2js', [wasm, ...FEATURES, ...(js2Opt === 'none' ? [] : [`-${js2Opt}`]), '-o', raw]);

// wasm2js writes an ES module that imports the bare specifier 'uo_host' and exports every function; make it a
// factory instead (several instances, no module-level state, no import map needed)
let src = readFileSync(raw, 'utf8');
const importLine = "import * as uo_host from 'uo_host';\n";
const call = /\nvar retasmFunc = asmFunc\(\{\s*"uo_host": uo_host,?\s*\}\);\n/;
if (!src.startsWith(importLine) || !call.test(src)) throw new Error('unexpected wasm2js output shape (binaryen version?)');
src = src.slice(importLine.length);
src = src.slice(0, src.search(call)) + '\nreturn asmFunc({ "uo_host": imports.uo_host });\n';
src = `export default function uoWasm2js(imports) {\n${src}}\n`;

if (args.includes('--no-minify')) {
  writeFileSync(out, src);
} else {
  const candidates = [opt('--esbuild'), process.env.UO_ESBUILD, join(core, '../write/node_modules/esbuild')];
  try { candidates.push(join(dirname(execFileSync('git', ['rev-parse', '--path-format=absolute', '--git-common-dir'], { cwd: core, encoding: 'utf8' }).trim()), 'write/node_modules/esbuild')); } catch {}
  const pkg = candidates.find((c) => c && existsSync(join(c, 'package.json')));
  if (!pkg) { console.error('no esbuild found (--esbuild <package dir> / UO_ESBUILD); use --no-minify'); process.exit(2); }
  const esbuild = createRequire(join(pkg, 'package.json'))('./lib/main.js');
  const r = esbuild.transformSync(src, { minify: true, format: 'esm', target: 'es2018', legalComments: 'none' });
  writeFileSync(out, r.code);
}
console.log(`${out}: ${readFileSync(out).length} bytes`);
