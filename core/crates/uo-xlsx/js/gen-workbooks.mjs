// Seeded deterministic workbook generator (lib/gen-workbook.mjs): writes the .xlsx files of a group.
// Same seed -> same bytes (fixed zip times, zlib deflate level 6, no Date/Math.random).
//
//   node core/crates/uo-xlsx/js/gen-workbooks.mjs --list
//   node core/crates/uo-xlsx/js/gen-workbooks.mjs --gen x1 [--out <dir>]        # default tests/captures/workbooks/<group>
//   node core/crates/uo-xlsx/js/gen-workbooks.mjs --gen x1-big --out core/target/sheet-cap/workbooks/x1-big   # local
//   node core/crates/uo-xlsx/js/gen-workbooks.mjs --gen x1 --check              # committed files == regenerated
//   node core/crates/uo-xlsx/js/gen-workbooks.mjs --heavy 20000 --out <dir>     # the calc-heavy benchmark workbook
//   options: --only <i[,j..]>  --stats (feature coverage over the group)
//
// .xls: the TS reads .xls by converting BIFF in memory; generated .xls would come from Excel COM `SaveAs` of these
// .xlsx files (FileFormat 56), which this task does not run (no COM). Hook: `--xls-list <file>` writes the list of
// generated .xlsx paths for a later `SaveAs` batch (sheet/tools/excel-lab style), whose outputs are then captured
// by capture-sheet.mjs --dir <dir> like any other workbook.
import { existsSync, mkdirSync, readFileSync, writeFileSync } from 'node:fs';
import { join } from 'node:path';
import { args, capturesDir } from './lib/common.mjs';
import { FEATURES, GROUPS, buildCalcHeavy, buildWorkbook, workbookFile } from './lib/gen-workbook.mjs';

const a = args();
if (a.flag('--list')) {
  for (const [name, g] of Object.entries(GROUPS)) console.log(`${name}\tseeds ${g.seedBase}..${g.seedBase + g.count - 1}\t${g.count} workbooks\t${g.ci ? 'CI (committed)' : 'local only'}`);
  console.log(`features: ${FEATURES.join(' ')}`);
  process.exit(0);
}
if (a.opt('--heavy')) {
  const rows = Number(a.opt('--heavy'));
  const out = a.opt('--out', '.');
  mkdirSync(out, { recursive: true });
  const file = join(out, `calc-heavy-${rows}.xlsx`);
  writeFileSync(file, buildCalcHeavy(rows));
  console.log(file);
  process.exit(0);
}
const name = a.opt('--gen');
if (!name || !GROUPS[name]) { console.error(`usage: gen-workbooks.mjs --gen <${Object.keys(GROUPS).join('|')}> [--out dir] [--check] [--only i,j] [--stats]`); process.exit(2); }
const g = GROUPS[name];
const out = a.opt('--out', join(capturesDir, 'workbooks', name));
const only = a.opt('--only');
const indices = only ? only.split(',').map(Number) : Array.from({ length: g.count }, (_, i) => i);
const check = a.flag('--check');
if (!check) mkdirSync(out, { recursive: true });
let bytes = 0, diff = 0;
const cover = new Map();
const files = [];
for (const i of indices) {
  const d = buildWorkbook(name, i);
  bytes += d.bytes.length;
  for (const f of d.features) cover.set(f, (cover.get(f) ?? 0) + 1);
  const file = join(out, workbookFile(name, i));
  files.push(file);
  if (check) {
    if (!existsSync(file) || !readFileSync(file).equals(d.bytes)) { diff++; console.error(`DIFFERS: ${file}`); }
  } else writeFileSync(file, d.bytes);
}
if (a.opt('--xls-list')) writeFileSync(a.opt('--xls-list'), files.join('\n') + '\n');
console.log(`${name}: ${indices.length} workbooks, ${(bytes / 1024).toFixed(0)} KiB${check ? `, ${diff} differ from ${out}` : ` -> ${out}`}`);
if (a.flag('--stats')) {
  for (const f of FEATURES) console.log(`${String(cover.get(f) ?? 0).padStart(5)}  ${f}`);
}
process.exit(diff ? 1 : 0);
