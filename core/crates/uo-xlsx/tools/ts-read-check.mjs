// Reads every .xlsx of a folder with the product's own TS reader (frozen sheet-core dist: openSpreadsheet,
// readWorkbook, readWorksheet) and prints rows, cells, columns, merges and panes per sheet; a second parser besides Excel.
//   UO_TS_ROOT=<frozen checkout with dist> node core/crates/uo-xlsx/tools/ts-read-check.mjs <folder>
import { readFileSync, readdirSync } from 'node:fs';
import { join } from 'node:path';
import { pathToFileURL } from 'node:url';
const root = process.env.UO_TS_ROOT;
const core = await import(pathToFileURL(join(root, 'sheet/packages/sheet-core/dist/index.js')).href);
const dir = process.argv[2];
for (const f of readdirSync(dir).filter((n) => n.endsWith('.xlsx'))) {
  try {
    const pkg = core.openSpreadsheet(new Uint8Array(readFileSync(join(dir, f))));
    const wb = core.readWorkbook(pkg);
    const out = [];
    for (const s of wb.sheets) {
      const ws = core.readWorksheet(pkg, s.target);
      const cells = ws.rows.reduce((n, r) => n + r.cells.length, 0);
      out.push(`${s.name}: ${ws.rows.length} rows, ${cells} cells, cols ${ws.cols?.length ?? 0}, merges ${(ws.merges ?? []).length}, pane ${JSON.stringify(ws.pane ?? null)}`);
    }
    console.log(`${f}\tread\t${out.join(' | ')}`);
  } catch (e) { console.log(`${f}\tFAILED\t${e.stack}`); }
}
