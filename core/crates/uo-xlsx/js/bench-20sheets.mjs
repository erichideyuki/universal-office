// The 20-sheet workbook of record T13 ("load all sheets of a generated 20-sheet workbook", C1a): 20 sheets x
// 3,000 rows x 14 columns (numbers, shared strings, inline strings, styled cells, a formula every 7th row),
// 840,000 cells. Writes it to --out and, with --ts, times the frozen TS read of every part and every sheet
// (the `open-core` scenario of core/tools/bench/speed/sheet.mjs: openSpreadsheet + readWorkbook + readStyles +
// readSharedStrings + every readWorksheet), median of --rounds.
//
//   node core/crates/uo-xlsx/js/bench-20sheets.mjs --out core/target/sheet-cap/bench/sheets20.xlsx [--ts] [--rounds 5]
//   cargo run --release -p uo-xlsx --features read --example read_bench -- core/target/sheet-cap/bench/sheets20.xlsx --all
import { mkdirSync, writeFileSync, readFileSync } from 'node:fs';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';
import { zipBytes } from '../../uo-pptx/js/lib/zip.mjs';

const here = dirname(fileURLToPath(import.meta.url));
const argv = process.argv.slice(2);
const opt = (k, d) => { const i = argv.indexOf(k); return i >= 0 ? argv[i + 1] : d; };
const out = resolve(opt('--out', join(here, '../../../target/sheet-cap/bench/sheets20.xlsx')));
const rounds = Number(opt('--rounds', 5));
const NS = 'http://schemas.openxmlformats.org/spreadsheetml/2006/main';
const NSR = 'http://schemas.openxmlformats.org/officeDocument/2006/relationships';
const REL = NSR;
const XML = '<?xml version="1.0" encoding="UTF-8" standalone="yes"?>\n';
const SHEETS = 20, ROWS = 3000, COLS = 14;
const col = (c) => { let s = ''; for (c += 1; c > 0; c = Math.floor((c - 1) / 26)) s = String.fromCharCode(65 + ((c - 1) % 26)) + s; return s; };

let seed = 12345;
const rnd = () => { seed ^= seed << 13; seed >>>= 0; seed ^= seed >>> 17; seed ^= seed << 5; seed >>>= 0; return seed; };
const words = ['alpha', 'beta', 'gamma', 'delta', 'épsilon', 'zeta', 'eta', 'theta'];
const sst = words.map((w) => `<si><t>${w}</t></si>`).join('');
const files = [];
const ct = ['<Override PartName="/xl/workbook.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.sheet.main+xml"/>',
  '<Override PartName="/xl/styles.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.styles+xml"/>',
  '<Override PartName="/xl/sharedStrings.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.sharedStrings+xml"/>'];
const rels = [`<Relationship Id="rId${SHEETS + 1}" Type="${REL}/styles" Target="styles.xml"/>`, `<Relationship Id="rId${SHEETS + 2}" Type="${REL}/sharedStrings" Target="sharedStrings.xml"/>`];
const sheetEls = [];
for (let s = 1; s <= SHEETS; s++) {
  const rows = [];
  for (let r = 1; r <= ROWS; r++) {
    const cells = [];
    for (let c = 0; c < COLS; c++) {
      const ref = `${col(c)}${r}`;
      const k = rnd() % 9;
      if (k < 4) cells.push(`<c r="${ref}"${c % 3 === 0 ? ' s="1"' : ''}><v>${rnd() % 100000}</v></c>`);
      else if (k < 6) cells.push(`<c r="${ref}" t="s"><v>${rnd() % words.length}</v></c>`);
      else if (k === 6) cells.push(`<c r="${ref}"><v>${(rnd() % 100000) / 100}</v></c>`);
      else if (k === 7) cells.push(`<c r="${ref}" t="inlineStr"><is><t>n${rnd() % 500}</t></is></c>`);
      else cells.push(`<c r="${ref}" s="1"><v>${rnd() % 1000}</v></c>`);
    }
    if (r % 7 === 0) cells.push(`<c r="${col(COLS)}${r}"><f>SUM(A${r}:D${r})</f><v>0</v></c>`);
    rows.push(`<row r="${r}" spans="1:${COLS + 1}">${cells.join('')}</row>`);
  }
  files.push([`xl/worksheets/sheet${s}.xml`, `${XML}<worksheet xmlns="${NS}" xmlns:r="${NSR}"><dimension ref="A1:${col(COLS)}${ROWS}"/><sheetViews><sheetView workbookViewId="0"/></sheetViews><sheetFormatPr defaultRowHeight="15"/><cols><col min="1" max="3" width="12" customWidth="1"/></cols><sheetData>${rows.join('')}</sheetData><pageMargins left="0.7" right="0.7" top="0.75" bottom="0.75" header="0.3" footer="0.3"/></worksheet>`]);
  ct.push(`<Override PartName="/xl/worksheets/sheet${s}.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.worksheet+xml"/>`);
  rels.push(`<Relationship Id="rId${s}" Type="${REL}/worksheet" Target="worksheets/sheet${s}.xml"/>`);
  sheetEls.push(`<sheet name="Plan${s}" sheetId="${s}" r:id="rId${s}"/>`);
}
const PKG_REL = 'http://schemas.openxmlformats.org/package/2006/relationships';
const zip = zipBytes([
  ['[Content_Types].xml', `${XML}<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/><Default Extension="xml" ContentType="application/xml"/>${ct.join('')}</Types>`],
  ['_rels/.rels', `${XML}<Relationships xmlns="${PKG_REL}"><Relationship Id="rId1" Type="${REL}/officeDocument" Target="xl/workbook.xml"/></Relationships>`],
  ['xl/workbook.xml', `${XML}<workbook xmlns="${NS}" xmlns:r="${NSR}"><fileVersion appName="xl" lastEdited="7" lowestEdited="7" rupBuild="27425"/><workbookPr/><bookViews><workbookView activeTab="0"/></bookViews><sheets>${sheetEls.join('')}</sheets><calcPr calcId="191029"/></workbook>`],
  ['xl/_rels/workbook.xml.rels', `${XML}<Relationships xmlns="${PKG_REL}">${rels.join('')}</Relationships>`],
  ['xl/styles.xml', `${XML}<styleSheet xmlns="${NS}"><fonts count="1"><font><sz val="11"/><name val="Calibri"/></font></fonts><fills count="2"><fill><patternFill patternType="none"/></fill><fill><patternFill patternType="gray125"/></fill></fills><borders count="1"><border><left/><right/><top/><bottom/><diagonal/></border></borders><cellStyleXfs count="1"><xf numFmtId="0" fontId="0" fillId="0" borderId="0"/></cellStyleXfs><cellXfs count="2"><xf numFmtId="0" fontId="0" fillId="0" borderId="0" xfId="0"/><xf numFmtId="2" fontId="0" fillId="0" borderId="0" xfId="0" applyNumberFormat="1"/></cellXfs></styleSheet>`],
  ['xl/sharedStrings.xml', `${XML}<sst xmlns="${NS}" count="${words.length}" uniqueCount="${words.length}">${sst}</sst>`],
  ...files,
]);
mkdirSync(dirname(out), { recursive: true });
writeFileSync(out, zip);
console.log(`wrote ${out}: ${zip.length} bytes, ${SHEETS} sheets, ${SHEETS * ROWS * (COLS + 1 / 7)} cells (approx.)`);

if (argv.includes('--ts')) {
  const root = resolve(process.env.UO_TS_ROOT || 'D:/Projects/OfficeNext/.claude/worktrees/ts-ref-092');
  const SC = await import(pathToFileURL(join(root, 'sheet/packages/sheet-core/dist/index.js')).href);
  const bytes = new Uint8Array(readFileSync(out));
  const read = () => {
    const pkg = SC.openSpreadsheet(bytes);
    const workbook = SC.readWorkbook(pkg);
    const cx = SC.colorContext(pkg);
    SC.readSharedStrings(pkg, cx);
    SC.readStyles(pkg, cx);
    return workbook.sheets.map((s) => (s.target ? SC.readWorksheet(pkg, s.target, cx) : undefined));
  };
  read();
  const t = [];
  for (let i = 0; i < rounds; i++) { const a = performance.now(); read(); t.push(performance.now() - a); }
  t.sort((a, b) => a - b);
  console.log(`TS read of every part and sheet: median ${t[t.length >> 1].toFixed(1)} ms (min ${t[0].toFixed(1)}) over ${rounds} rounds`);
}
