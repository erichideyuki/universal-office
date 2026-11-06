// The page script of browser/wasm2js_run.mjs (an external module on purpose: the CSP test page forbids inline
// scripts). Query: mode = csp | correct | speed, wasm = <repo path of the .wasm>, js = <repo path of the wasm2js
// module>, engine = wasm | js (speed), runs = n. The result is left in window.__result as plain JSON.
import { loadWrite, canCompileWasm } from '../../js/write.mjs';
import { fnv } from '../../js/render.mjs';
import { loadAssets, collect, digest, firstDifference, timeFixture, FIXTURES, PINNED_PDF } from '../wasm2js_common.mjs';

const q = new URLSearchParams(location.search);
const mode = q.get('mode');
const wasmUrl = '/' + q.get('wasm'), jsUrl = '/' + q.get('js');
const read = async (p) => new Uint8Array(await (await fetch('/' + p)).arrayBuffer());
const violations = [];
document.addEventListener('securitypolicyviolation', (e) => violations.push(`${e.violatedDirective}: ${e.blockedURI || 'wasm-eval'}`));
const errInfo = (e) => ({ name: e?.name, constructor: e?.constructor?.name, message: String(e?.message ?? e).slice(0, 300) });
const heap = () => (performance.memory ? performance.memory.usedJSHeapSize : null);

async function csp() {
  const r = { canCompileWasm: canCompileWasm() };
  try { new Function('return 1')(); r.evalAllowed = true; } catch (e) { r.evalAllowed = false; }
  try { await loadWrite({ engine: 'wasm', wasm: wasmUrl }); r.wasmEngine = 'loaded'; } catch (e) { r.wasmEngine = 'refused'; r.wasmError = errInfo(e); }
  const assets = await loadAssets(read, ['f8__7__0', 'f8__7__3', 'f8__7__10']);
  r.js = {};
  try {
    const t0 = performance.now();
    const js = await loadWrite({ engine: 'js', js: jsUrl });
    r.js.loadMs = Math.round(performance.now() - t0);
    r.js.engine = js.engine;
    r.js.pdf = {};
    for (const k of Object.keys(PINNED_PDF)) { const c = collect(js.uo, assets, k); r.js.pdf[k] = { bytes: c.pdf.length, fnv: fnv(c.pdf), native: PINNED_PDF[k], equal: fnv(c.pdf) === PINNED_PDF[k] }; }
  } catch (e) { r.js.error = errInfo(e); }
  try { const a = await loadWrite({ wasm: wasmUrl, js: jsUrl }); r.auto = a.engine; } catch (e) { r.auto = 'error'; r.autoError = errInfo(e); }
  r.violations = violations;
  return r;
}

async function correct() {
  const assets = await loadAssets(read);
  const a = await loadWrite({ engine: 'wasm', wasm: wasmUrl });
  const b = await loadWrite({ engine: 'js', js: jsUrl });
  const rows = [];
  for (const k of FIXTURES) {
    const x = collect(a.uo, assets, k), y = collect(b.uo, assets, k);
    rows.push({ key: k, pages: x.pages, pdf: x.pdf.length, diff: firstDifference(x, y), pinned: PINNED_PDF[k] ? fnv(y.pdf) === PINNED_PDF[k] : null, digest: digest(y).split(' ').slice(0, 2).join(' ') });
  }
  return { rows };
}

async function speed() {
  const engine = q.get('engine'), runs = Number(q.get('runs') ?? 5);
  const keys = ['f8__7__0', 'f8__7__10', 'f8__7__9', 'f8__7__7'];
  const assets = await loadAssets(read, keys);
  const h0 = heap(), t0 = performance.now();
  const lib = await loadWrite(engine === 'wasm' ? { engine, wasm: wasmUrl } : { engine, js: jsUrl });
  const loadMs = performance.now() - t0, h1 = heap();
  const rows = {};
  for (const k of keys) rows[k] = timeFixture(lib.uo, assets, k, runs);
  return { engine, loadMs, heapBeforeLoad: h0, heapAfterLoad: h1, heapAfterRuns: heap(), linearMemory: lib.exports.memory.buffer.byteLength, rows };
}

try {
  window.__result = { ok: true, mode, ua: navigator.userAgent, ...(await { csp, correct, speed }[mode]()) };
} catch (e) {
  window.__result = { ok: false, mode, error: errInfo(e), stack: String(e?.stack ?? '').slice(0, 600), violations };
}
