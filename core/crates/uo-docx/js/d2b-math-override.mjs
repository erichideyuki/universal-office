// The math override of the Chromium harnesses of D2b (G17, U5): V8 in Chromium may compute
// `Math.sin/cos/tan/atan2/pow/hypot/exp/log` with other bits than the Node 24.15 the TS engines
// are the reference in, so the harness page replaces them with the uo-jsnum functions (proven equal
// to Node 24.15 in the uo-jsnum record) from a wasm module built here:
//
//   cargo build -p uo-jsnum --release --target wasm32-unknown-unknown   (the rlib)
//   rustc … d2b-math-override.rs                                         (the exports)
//
// Used by every harness whose steps go through math (F3: the table formulas through `Math.pow`; F7:
// the drawing and picture ops). Usage in a capture script:
//
//   import { mathOverride } from './d2b-math-override.mjs';
//   const mo = await mathOverride();                  // builds on first use, cached in the target dir
//   await context.addInitScript(mo.initScript);       // before any page script runs
//   out.meta('math-override', mo.meta);               // recorded in the capture metadata
import { execFileSync } from 'node:child_process';
import { createHash } from 'node:crypto';
import { existsSync, mkdirSync, readFileSync, statSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';

const here = dirname(fileURLToPath(import.meta.url));
const core = join(here, '..', '..', '..');
const src = join(here, 'd2b-math-override.rs');
const target = process.env.UO_MATH_TARGET ?? join(core, 'target', 'd2b-math-override');
const wasm = join(target, 'd2b_math_override.wasm');

const NAMES = ['sin', 'cos', 'tan', 'exp', 'log', 'atan2', 'pow'];

function build() {
  const env = { ...process.env, CARGO_TARGET_DIR: target };
  execFileSync('cargo', ['build', '-p', 'uo-jsnum', '--release', '--target', 'wasm32-unknown-unknown'], { cwd: core, env, stdio: 'inherit' });
  const deps = join(target, 'wasm32-unknown-unknown', 'release', 'deps');
  const rlib = join(target, 'wasm32-unknown-unknown', 'release', 'libuo_jsnum.rlib');
  mkdirSync(target, { recursive: true });
  execFileSync('rustc', [
    '--edition', '2024', '--target', 'wasm32-unknown-unknown', '--crate-type', 'cdylib', '-C', 'opt-level=3', '-C', 'panic=abort',
    '-L', `dependency=${deps}`, '--extern', `uo_jsnum=${rlib}`, src, '-o', wasm,
  ], { cwd: core, stdio: 'inherit' });
}

/** `{ initScript, meta }`: the script for `context.addInitScript` and the text for the capture metadata. */
export async function mathOverride() {
  if (!existsSync(wasm) || statSync(wasm).mtimeMs < statSync(src).mtimeMs) build();
  const bytes = readFileSync(wasm);
  const b64 = bytes.toString('base64');
  const sha = createHash('sha256').update(bytes).digest('hex').slice(0, 16);
  // the page: the module is instantiated before any script; `hypot` keeps the original for the
  // arities the module does not export (the editor calls it with two or three arguments)
  const initScript = `(() => {
    const bytes = Uint8Array.from(atob(${JSON.stringify(b64)}), (c) => c.charCodeAt(0));
    const x = new WebAssembly.Instance(new WebAssembly.Module(bytes), {}).exports;
    const orig = Math.hypot;
    for (const k of ${JSON.stringify(NAMES)}) Math[k] = x[k];
    Math.hypot = function (...a) { return a.length === 2 ? x.hypot(a[0], a[1]) : a.length === 3 ? x.hypot3(a[0], a[1], a[2]) : orig.apply(Math, a); };
  })();`;
  return { initScript, meta: `uo-jsnum wasm ${sha}: Math.${NAMES.join(',')},hypot` };
}
