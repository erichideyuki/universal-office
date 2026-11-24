// Writes the parts of the package the frozen TS `.xls` converter builds for a file into a folder (names with `/` as `__`), to
// diff with `cargo run -p uo-xlsx --features xls --example xls_dump -- <file> <folder>`.
//   node core/crates/uo-xlsx/js/dump-xls-parts.mjs <file.xls> <out folder>
import { mkdirSync, readFileSync, writeFileSync } from 'node:fs';
import { join } from 'node:path';
import { args, tsRoot } from './lib/common.mjs';
import { loadHarness } from './harness/build-harness.mjs';

const a = args();
const [file, out] = a.argv.filter((x) => !x.startsWith('--'));
const h = await loadHarness(tsRoot(a));
const t0 = performance.now();
const pkg = h.sheetCore.xlsToXlsxPackage(new Uint8Array(readFileSync(file)));
const ms = performance.now() - t0;
mkdirSync(out, { recursive: true });
for (const n of pkg.partNames()) writeFileSync(join(out, n.replace(/\//g, '__')), pkg.getPart(n));
console.log(`${pkg.partNames().length} parts in ${ms.toFixed(1)} ms -> ${out}`);
