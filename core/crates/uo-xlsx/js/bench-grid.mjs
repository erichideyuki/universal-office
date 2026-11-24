// Benchmark of the frozen TS grid model (the baselines C3a's Rust numbers are compared with): `fitRowHeights`,
// `recalcGrid` and `layoutCellText` (through the component's `cellLayout`: style resolution + cell text input +
// layout) over a 1080p viewport and over 100,000 cells, on a workbook of `sheet/samples`.
//
//   node core/crates/uo-xlsx/js/bench-grid.mjs [file.xlsx ...] [--rounds N]     (default: giant-1m and giant-200k)
//
// The bundle is the one `capture-grid.mjs` builds (core/target/sheet-cap/harness/grid.mjs); run that script first.
process.env.TZ = 'UTC';
const { join } = await import('node:path');
const { pathToFileURL } = await import('node:url');
const { readFileSync, existsSync } = await import('node:fs');
const C = await import('./lib/common.mjs');

const a = C.args();
const rounds = Number(a.opt('--rounds', 7));
const base = join(C.coreDir, 'target', 'sheet-cap', 'harness');
if (!existsSync(join(base, 'grid.mjs'))) { console.error('run capture-grid.mjs first (it builds the bundle)'); process.exit(2); }
const domMod = await import(pathToFileURL(join(base, 'dom.mjs')).href);
const dom = domMod.installFakeDom();
dom.customElements.define('u-ripple', class extends globalThis.HTMLElement {});
const M = await import(pathToFileURL(join(base, 'grid.mjs')).href);

const files = a.argv.filter((x) => x.endsWith('.xlsx'));
if (!files.length) files.push(join(C.repoDir, 'sheet', 'samples', 'giant-1m.xlsx'), join(C.repoDir, 'sheet', 'samples', 'giant-200k.xlsx'));
const median = (v) => [...v].sort((x, y) => x - y)[Math.floor(v.length / 2)];
const time = (fn, n = rounds) => { const t = []; for (let i = 0; i < n; i++) { const t0 = performance.now(); fn(); t.push(performance.now() - t0); } return median(t); };

for (const f of files) {
  const bytes = new Uint8Array(readFileSync(f));
  const el = new M.UniversalSheetElement();
  dom.document.body.appendChild(el);
  el.shouldRecalcOnOpen = () => false;
  const t0 = performance.now();
  el.loadXlsx(bytes);
  const load = performance.now() - t0;
  const v = el.view;
  const fit = time(() => el.fitRowHeights(v), Math.min(rounds, 5));
  const grid = time(() => el.recalcGrid(), rounds * 3);
  // the cells of the sheet by (row, col)
  const rows = new Map(v.worksheet.rows.map((r) => [r.r, new Map(r.cells.map((c) => [M.sheetCore.refToRowCol(c.ref).col, c]))]));
  const layoutOf = (r1, r2, c1, c2) => () => {
    for (let r = r1; r <= r2; r++) {
      const cells = rows.get(r);
      if (!cells) continue;
      for (let c = c1; c <= c2; c++) {
        const cell = cells.get(c);
        if (!cell) continue;
        const st = el.resolveStyle(cell, c, r);
        el.cellLayout(cell, st, v.colWidths.get(c) ?? v.defaultColW, 1);
      }
    }
  };
  const frame = time(layoutOf(1, 52, 1, 29), 40);
  const cells100k = time(layoutOf(1, Math.ceil(100_000 / Math.max(1, v.maxCol > 100 ? 100 : v.maxCol)), 1, Math.min(100, v.maxCol)), Math.min(rounds, 5));
  console.log(`${f.split(/[\\/]/).pop()}: loadXlsx ${load.toFixed(0)} ms; fitRowHeights ${fit.toFixed(1)} ms; recalcGrid ${grid.toFixed(2)} ms; 1080p viewport (52 x 29) layout ${frame.toFixed(3)} ms; ~100k cells layout ${cells100k.toFixed(1)} ms (maxRow ${v.maxRow}, maxCol ${v.maxCol})`);
  el.remove();
}
