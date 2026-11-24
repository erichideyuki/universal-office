// TS side of the C3b benchmark (examples/cf_bench.rs): the frozen `<universal-sheet>` element headless (the harness of
// capture-cf.mjs), its own CF engine (`cfEngine()` → `CfEngine.at`) over a 1080p viewport (45 rows × 34 columns),
// the same scenarios: cold (a new engine + the viewport), warm (the same viewport), scroll (a new viewport per
// frame), stats (a new engine + one cell per column). Medians.
//
//   cargo run --release -p uo-xlsx --features grid --example cf_bench -- --write core/target/sheet-cap   # writes the 100k workbook
//   node --expose-gc core/crates/uo-xlsx/js/bench-cf.mjs
process.env.TZ = 'UTC';
const { readFileSync, existsSync } = await import('node:fs');
const { join } = await import('node:path');
const { pathToFileURL } = await import('node:url');
const C = await import('./lib/common.mjs');

const harness = join(C.coreDir, 'target', 'sheet-cap', 'harness');
if (!existsSync(join(harness, 'cf.mjs'))) throw new Error('run capture-cf.mjs once first (it builds the bundle)');
const dom = (await import(pathToFileURL(join(harness, 'dom.mjs')).href)).installFakeDom();
dom.customElements.define('u-ripple', class extends globalThis.HTMLElement {});
const M = await import(pathToFileURL(join(harness, 'cf.mjs')).href);
const ROWS = 45, COLS = 34;
const median = (v) => v.sort((a, b) => a - b)[v.length >> 1];

function open(bytes, inject) {
  const el = new M.UniversalSheetElement();
  el.setAttribute('formula-locale', 'pt-BR');
  dom.document.body.appendChild(el);
  el.shouldRecalcOnOpen = () => false;
  el.loadXlsx(bytes);
  el.sheetIndex = 0;
  if (inject) { el.views[0].condFormats = inject; el.cfEngines.clear(); }
  return el;
}
const viewport = (eng, r0) => { let n = 0; for (let r = r0; r < r0 + ROWS; r++) for (let c = 1; c <= COLS; c++) if (eng.at(c, r + 1) !== undefined) n++; return n; };

function scenario(name, el, maxRow) {
  const fresh = () => { el.cfEngines.clear(); return el.cfEngine(); };
  const cold = [];
  for (let k = 0; k < 5; k++) { const t = performance.now(); viewport(fresh(), 1000); cold.push(performance.now() - t); }
  const eng = fresh();
  viewport(eng, 1000);
  const warm = [];
  for (let k = 0; k < 21; k++) { const t = performance.now(); viewport(eng, 1000); warm.push(performance.now() - t); }
  const scroll = [];
  for (let k = 0; k < 40; k++) { const r0 = 2000 + k * ROWS * 3; if (r0 + ROWS >= maxRow) break; const t = performance.now(); viewport(eng, r0); scroll.push(performance.now() - t); }
  const stats = [];
  for (let k = 0; k < 5; k++) { const e = fresh(); const t = performance.now(); for (let c = 1; c <= COLS; c++) e.at(c, 6); stats.push(performance.now() - t); }
  console.log(`${name}: viewport ${ROWS}x${COLS} (${viewport(eng, 1000)} cells with a layer) | cold ${median(cold).toFixed(3)} ms | warm ${median(warm).toFixed(4)} ms | scroll ${median(scroll).toFixed(3)} ms | stats ${median(stats).toFixed(3)} ms`);
}

const ten = join(C.bigDir, 'cf-bench-100k.xlsx');
scenario('ten rules, 100k rows', open(new Uint8Array(readFileSync(ten))), 100000);
const giant = C.readSample('giant-1m.xlsx');
const formats = [{ sqref: 'A1:CV10000', rules: [
  { type: 'expression', priority: 2, formula: ['MOD(A1,7)=0'], x14: true, dxf: { bold: true, fontColor: '#9C0006' } },
  { type: 'colorScale', priority: 1, stops: [{ type: 'min', color: '#F8696B' }, { type: 'percentile', val: '50', value: 50, color: '#FFEB84' }, { type: 'max', color: '#63BE7B' }] },
] }];
scenario('giant-1m, colour scale + formula', open(giant, formats), 10000);
