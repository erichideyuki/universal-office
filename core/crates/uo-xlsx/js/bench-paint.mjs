// TS side of the C3c benchmark (examples/paint_bench.rs): the frozen `<universal-sheet>` element headless (the harness of
// capture-paint.mjs) doing, for a 1080p viewport (45 rows x 34 columns + 1 around it), what its painter does per frame:
// `styleAt` per cell (table layer + CF), the real `paintGridBorders` into a counting canvas and `paintCellFill` for the
// filled cells. Same scenarios and workbooks as the Rust example: cold (the first frame; the table layer of the whole
// sheet is built at load: `layer` below times `buildTableLayer` alone), warm (the same frame again), scroll (a new
// viewport per frame). Medians.
//
//   cargo run --release -p uo-xlsx --features grid --example paint_bench -- --write core/target/sheet-cap/paint-bench
//   node --expose-gc core/crates/uo-xlsx/js/bench-paint.mjs
process.env.TZ = 'UTC';
const { readFileSync, existsSync } = await import('node:fs');
const { join } = await import('node:path');
const { pathToFileURL } = await import('node:url');
const C = await import('./lib/common.mjs');
globalThis.DOMMatrix = class { constructor(m) { [this.a, this.b, this.c, this.d, this.e, this.f] = m; } };

const harness = join(C.coreDir, 'target', 'sheet-cap', 'harness');
if (!existsSync(join(harness, 'paint.mjs'))) throw new Error('run capture-paint.mjs once first (it builds the bundle)');
const dom = (await import(pathToFileURL(join(harness, 'dom.mjs')).href)).installFakeDom();
dom.customElements.define('u-ripple', class extends globalThis.HTMLElement {});
const M = await import(pathToFileURL(join(harness, 'paint.mjs')).href);
const ROWS = 45, COLS = 34;
const median = (v) => v.sort((a, b) => a - b)[v.length >> 1];

// a canvas that only counts fills (the fills of paintGridBorders; gradients/patterns are measured by the fake DOM's own)
class Counter {
  constructor() { this.n = 0; this.fillStyle = '#000'; }
  fillRect() { this.n++; }
  createLinearGradient() { return { addColorStop() {} }; }
  createPattern() { return { setTransform() {} }; }
  drawImage() { this.n++; }
}

function open(bytes) {
  const t = performance.now();
  const el = new M.UniversalSheetElement();
  el.setAttribute('formula-locale', 'pt-BR');
  dom.document.body.appendChild(el);
  el.shouldRecalcOnOpen = () => false;
  el.loadXlsx(bytes);
  el.sheetIndex = 0;
  el.zoom = 1;
  el.recalcGrid();
  return { el, openMs: performance.now() - t };
}

function scenario(name, bytes, row0, maxRow) {
  const { el, openMs } = open(bytes);
  const v = el.views[0];
  const idx = el.cellIndex(v, 0);
  const cx = M.sheetCore.colorContext(M.sheetCore.openSpreadsheet(bytes));
  const layer = [];
  for (let k = 0; k < 5; k++) { const t = performance.now(); M.tableStyles.buildTableLayer(v.worksheet, el.styles, cx); layer.push(performance.now() - t); }
  const ctx = new Counter();
  const frame = (r1) => {
    const sy = el.ys[r1 - 1] ?? 0;
    let filled = 0;
    const rows = idx;
    for (let r = Math.max(1, r1 - 1); r <= r1 + ROWS; r++) {
      const cells = rows.get(r);
      for (let c = 1; c <= COLS + 1; c++) {
        const st = el.styleAt(cells?.get(c), c, r);
        if (st.filled && r >= r1 && r < r1 + ROWS && c <= COLS) { M.fills.paintCellFill(ctx, st.fill, (el.xs[c - 1] ?? 0), (el.ys[r - 1] ?? 0) - sy, 65, 21, 0, -sy); filled++; }
      }
    }
    ctx.n = 0;
    el.paintGridBorders(ctx, idx, 1, COLS, r1, r1 + ROWS - 1, 0, sy);
    return ctx.n + filled;
  };
  const t0 = performance.now();
  const n0 = frame(row0);
  const cold = performance.now() - t0;
  const warm = [];
  for (let k = 0; k < 101; k++) { const t = performance.now(); frame(row0); warm.push(performance.now() - t); }
  const scroll = [];
  for (let k = 0; k < 200; k++) { const r1 = 2 + (k * 7) % Math.max(1, maxRow - ROWS - 2); const t = performance.now(); frame(r1); scroll.push(performance.now() - t); }
  console.log(`${name}: open ${openMs.toFixed(0)} ms | layer build ${median(layer).toFixed(3)} ms | cold frame ${cold.toFixed(3)} ms | warm ${median(warm).toFixed(4)} ms (${n0} rects+fills) | scroll ${median(scroll).toFixed(4)} ms`);
  el.remove();
}

const dir = join(C.bigDir, 'paint-bench');
const SC = [['table', 1000, 49000], ['many', 1000, 7900], ['borders', 100, 250]];
for (const [name, row0, maxRow] of SC) {
  const f = join(dir, `paint-bench-${name}.xlsx`);
  if (!existsSync(f)) { console.log(`${f} not found: run the Rust example with --write first`); continue; }
  scenario(name, new Uint8Array(readFileSync(f)), row0, maxRow);
}
scenario('giant', C.readSample('giant-1m.xlsx'), 1000, 9900);
