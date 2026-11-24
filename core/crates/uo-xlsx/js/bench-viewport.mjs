// TS baseline of the viewport emission (frontier C4): the frozen `paint()` of the real element on the fake DOM over a counting 2D
// context (every call is a counter increment: the cost of the JS work of one frame), 1920 x 1080 at zoom 1, the scroll steps of
// `examples/view_bench.rs`.   node --max-old-space-size=8192 core/crates/uo-xlsx/js/bench-viewport.mjs sheet/samples/giant-1m.xlsx ...
process.env.TZ = 'UTC';
const { readFileSync } = await import('node:fs');
const { pathToFileURL } = await import('node:url');
const C = await import('./lib/common.mjs');
const { buildHarness } = await import('./harness/build-harness.mjs');

const h = buildHarness(C.tsRoot(C.args()), `${C.coreDir}/target/sheet-cap/harness`);
const dom = (await import(pathToFileURL(h.dom).href)).installFakeDom();
dom.customElements.define('u-ripple', class extends globalThis.HTMLElement {});
const M = await import(pathToFileURL(h.sheet).href);
globalThis.DOMMatrix = class { constructor(m) { [this.a, this.b, this.c, this.d, this.e, this.f] = m; } };

let calls = 0;
const base = { measureText: (t) => ({ width: t.length * 6, fontBoundingBoxAscent: 9, fontBoundingBoxDescent: 3 }), getTransform: () => ({ a: 1 }) };
const ctx = new Proxy(base, { get(t, k) { if (k in t) return t[k]; return () => { calls++; return { addColorStop() {} }; }; }, set(t, k, v) { t[k] = v; calls++; return true; } });

for (const f of process.argv.slice(2).filter((a) => a.endsWith('.xlsx'))) {
  const el = new M.UniversalSheetElement();
  el.setAttribute('formula-locale', 'pt-BR');
  dom.document.body.appendChild(el);
  el.shouldRecalcOnOpen = () => false;
  const t0 = performance.now();
  el.loadXlsx(new Uint8Array(readFileSync(f)));
  const open = performance.now() - t0;
  Object.defineProperty(el.canvas, 'clientWidth', { value: 1920, configurable: true });
  Object.defineProperty(el.canvas, 'clientHeight', { value: 1080, configurable: true });
  el.paintScrollbars = () => {};
  el.paintDrawings = () => {};
  el.canvas.getContext = () => ctx;
  el.sel = { col: 2, row: 2 };
  el.range = { c1: 2, r1: 2, c2: 4, r2: 6 };
  const frame = (k) => { el.scrollX = (k % 40) * 11.5; el.scrollY = (k % 90) * 37; el.paint(); };
  for (let k = 0; k < 20; k++) frame(k);
  const ts = [];
  calls = 0;
  for (let k = 0; k < 200; k++) { const t = performance.now(); frame(k); ts.push(performance.now() - t); }
  ts.sort((a, b) => a - b);
  console.log(`${f}: loadXlsx ${open.toFixed(0)} ms; paint() 1080p z1.00: ${ts[100].toFixed(3)} ms median (${ts[0].toFixed(3)} min), ${(calls / 200).toFixed(0)} canvas calls/frame`);
  el.remove();
}
