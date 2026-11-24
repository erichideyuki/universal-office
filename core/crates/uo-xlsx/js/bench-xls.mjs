// C2 benchmark of the frozen TS `.xls` converter (the baseline of the T13 row ".xls conversion"): `xlsToXlsxPackage` alone, and the
// whole read (convert, then readWorkbook/readStyles/readSharedStrings and readWorksheet of every sheet — what `openSpreadsheet` +
// the element's read do), time and memory (heap + external after GC with the result alive minus before; peak RSS of the process).
//
//   node --expose-gc core/crates/uo-xlsx/js/bench-xls.mjs <file.xls>... [--rounds N]
import { readFileSync } from 'node:fs';
import { args, tsRoot } from './lib/common.mjs';
import { loadHarness } from './harness/build-harness.mjs';

const a = args();
const files = a.argv.filter((x) => /\.xls$/i.test(x));
const rounds = Number(a.opt('--rounds', 5));
const h = await loadHarness(tsRoot(a));
const SC = h.sheetCore;
const gc = () => { globalThis.gc?.(); globalThis.gc?.(); };
const med = (v) => [...v].sort((x, y) => x - y)[Math.floor(v.length / 2)];
for (const f of files) {
  const bytes = new Uint8Array(readFileSync(f));
  const conv = [], full = [];
  for (let r = 0; r < rounds; r++) {
    let t = performance.now();
    let pkg = SC.xlsToXlsxPackage(bytes);
    conv.push(performance.now() - t);
    pkg = undefined;
    t = performance.now();
    const p2 = SC.xlsToXlsxPackage(bytes);
    const wb = SC.readWorkbook(p2);
    const cx = SC.colorContext(p2);
    SC.readStyles(p2, cx);
    SC.readSharedStrings(p2, cx);
    let cells = 0;
    for (const ref of wb.sheets) if (ref.target) { const ws = SC.readWorksheet(p2, ref.target, cx); for (const row of ws.rows) cells += row.cells.length; }
    full.push(performance.now() - t);
    if (r === rounds - 1) globalThis.__keep = [p2, wb, cells];
  }
  gc();
  const mem = () => { const m = process.memoryUsage(); return m.heapUsed + m.external; };
  const h0 = mem();
  const keep = SC.xlsToXlsxPackage(bytes);
  gc();
  const retainedConv = mem() - h0;
  let outBytes = 0;
  for (const n of keep.partNames()) outBytes += keep.getPart(n).length;
  const mb = (x) => (x / 1048576).toFixed(1);
  console.log(`${f}: input ${mb(bytes.length)} MiB, converted ${mb(outBytes)} MiB | convert median ${med(conv).toFixed(1)} ms (min ${Math.min(...conv).toFixed(1)}); retained ${mb(retainedConv)} MiB | convert+read all sheets median ${med(full).toFixed(1)} ms (min ${Math.min(...full).toFixed(1)}); cells read ${globalThis.__keep?.[2]} | process peak RSS ${mb(process.resourceUsage().maxRSS * 1024)} MiB`);
}
