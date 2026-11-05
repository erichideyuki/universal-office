// Parity capture of D2b-F3's pure model files (record `core/docs/modules/uo-docx-d2b/F3.md`): the
// frozen docx-core `table-ops.ts` (`TOps`), `table-sort.ts` and `table-formula.ts`, over the tables
// of corpus documents, as the editor calls them (the editor's `compareText` for the sort, the JSON
// round trip and `relink` of an undo step after every op). Node 24.15 x64: the formulas go
// through `Math.pow` and `Math.round`, which are the V8 implementations of this Node (proven equal
// to `uo_jsnum` in the uo-jsnum record, G17).
//
//   UO_TS_ROOT=<built checkout of main> node core/crates/uo-docx/js/capture-d2b-f3-ops.mjs [--docs N] [--ops N]
//
// Writes tests/parity/d2b-f3-ops.capture: per `<doc>#t<k>#<j>` (k = the table's order among the
// body tables, j = the op index) the facets
//   op    the op, encoded (the Rust replay runs this);
//   ret   what the op returned (booleans, texts, the digest of a second table or of paragraphs) or `err`;
//   tbl   the canonical digest (js/canon.mjs) of the table after the op;
//   json  the digest of the JSON round trip of the table with `relink` (what an undo restores).
// and per table one `#init` record (`tbl`, `json`). Pure value functions (`pc`, `ps`, `fx`, `df`)
// do not change the table.
import { join } from 'node:path';
import { CaptureWriter, Rng, Stream, importFrom } from '../../uo-testkit/js/testkit.mjs';
import { Canon } from './canon.mjs';
import { crate, corpus, readBytes, root, ts } from './capture-r2-common.mjs';

const argv = process.argv.slice(2);
const opt = (k, d) => (argv.includes(k) ? Number(argv[argv.indexOf(k) + 1]) : d);
const DOCS = opt('--docs', 150);
const SYN = opt('--syn', 120);
const OPS = opt('--ops', 60);
// `--dump <doc>#t<k>#<j>`: prints the JSON of that table after that op (before it with `#init`) and stops
const DUMP = argv.includes('--dump') ? argv[argv.indexOf('--dump') + 1] : undefined;

const { DocxPackage } = await ts('package.js');
const { readFullDocument } = await ts('fulldoc.js');
const T = await ts('table-ops.js');
const SORT = await ts('table-sort.js');
const FX = await ts('table-formula.js');
const { DEFAULT_RUN_PROPS } = await ts('document.js');
const { compareText } = await importFrom(root, 'sheet/packages/calc/src/collation.ts');

function roundTrip(v) {
  const pool = [];
  const idx = new Map();
  const json = JSON.stringify(v, (k, x) => {
    if (x instanceof Uint8Array) {
      let i = idx.get(x);
      if (i === undefined) { i = pool.length; pool.push(x); idx.set(x, i); }
      return { __imgPool: i };
    }
    return x;
  });
  return JSON.parse(json, (k, x) => (x && typeof x === 'object' && '__imgPool' in x ? pool[x.__imgPool] : x));
}

function digestOf(v, key) {
  const s = new Stream();
  new Canon(s).value(v, key);
  return s.finish().short;
}

const tdig = (t) => digestOf(t, 'table');
const parasDig = (paras) => {
  const s = new Stream();
  const c = new Canon(s);
  paras.forEach((p, i) => { s.tag('E').u32(i); c.value(p, 'paragraph'); });
  return s.finish().short;
};
const hex = (t) => [...Array(t.length).keys()].map((i) => t.charCodeAt(i).toString(16)).join('.') || '-';
const jnum = (x) => (x === undefined ? 'u' : String(x));

// ─────────────── argument tables (the Rust replay has the same, by index) ───────────────
const WIDTHS = [null, { w: 0, type: 'auto' }, { w: 5000, type: 'pct' }, { w: 4000, type: 'pct' }, { w: 6000, type: 'dxa' }, { w: 2500.4, type: 'dxa' }, { w: 5000, type: 'nil' }];
const CELLPATCH = [
  { vAlign: 'center' }, { vAlign: undefined }, { shd: { val: 'clear', color: 'auto', fill: 'FFFF00' } }, { shd: undefined },
  { tcMar: { top: { w: 50, type: 'dxa' }, left: { w: 70, type: 'dxa' } } }, { noWrap: true, tcFitText: true }, { tcW: { w: 1800, type: 'dxa' } },
  { textDirection: 'tbRl', vAlign: 'bottom' }, { tcBorders: { top: { val: 'single', sz: 4, space: 0, color: 'FF0000' } } }, { gridSpan: undefined, vMerge: undefined },
];
const ROWPATCH = [
  { cantSplit: true }, { tblHeader: true }, { tblHeader: undefined, cantSplit: undefined }, { trHeight: { val: 400, hRule: 'exact' } },
  { jc: 'center' }, { cnfStyle: '100000000000' }, { hidden: true, tblCellSpacing: { w: 20, type: 'dxa' } },
];
const TBLPATCH = [
  { tblStyle: 'Tabelacomgrade' }, { tblStyle: undefined }, { tblLayout: 'fixed' }, { tblLayout: undefined },
  { tblBorders: { top: { val: 'single', sz: 8, space: 0, color: '00FF00' }, insideH: { val: 'double', sz: 4, space: 0, color: 'auto' } } },
  { tblBorders: undefined }, { shd: { val: 'clear', color: 'auto', fill: 'CCCCCC' } }, { tblInd: { w: 360, type: 'dxa' }, tblCaption: 'Titulo' },
  { tblpPr: { leftFromText: 141, vertAnchor: 'text', tblpY: 1 }, tblOverlap: 'never' }, { tblpPr: undefined, tblOverlap: undefined, tblInd: undefined },
  { tblCellMar: { top: { w: 10, type: 'dxa' }, bottom: { w: 20, type: 'dxa' } }, tblDescription: 'Desc' },
];
const SPECS = [
  null, { val: 'single', sz: 4, space: 0, color: 'auto' }, { val: 'double', sz: 12, space: 0, color: 'FF0000' }, { val: 'dashed', sz: 8, space: 0, color: '0000FF' }, { val: 'nil' },
];
const STYLEB = [undefined, { top: { val: 'single', sz: 4, space: 0, color: 'auto' }, insideH: { val: 'single', sz: 4, space: 0, color: 'auto' } }, { left: { val: 'none' }, right: { val: 'nil' } }];
const KINDS = ['bottom', 'top', 'left', 'right', 'none', 'all', 'outside', 'inside', 'insideH', 'insideV', 'tl2br', 'tr2bl'];
const SHD = [null, { val: 'clear', color: 'auto', fill: 'FF0000' }, { val: 'pct25', color: 'auto', fill: 'auto' }, { val: 'clear', color: 'auto', fill: 'FFFFFF', themeFill: 'accent1', themeFillTint: '33' }];
const LOOKS = [{}, { firstRow: false }, { lastRow: true, noHBand: true }, { firstColumn: false, lastColumn: true, noVBand: false }];
const DIRS = ['lrTb', 'tbRl', 'btLr', 'lrTbV', undefined];
const JCS = ['left', 'center', 'right', undefined];
const SEPS = [(i) => 'tab', () => 'paragraph', () => 'semicolon', () => 'comma', () => ({ char: '|' }), () => ({ char: '' })];
const SEPNAMES = ['tab', 'paragraph', 'semicolon', 'comma', '|', '-'];
const sepOf = (i) => SEPS[i]();
const SORTKEYS = [
  [{ col: 0, type: 'text' }], [{ col: 0, type: 'number', desc: true }], [{ col: 1, type: 'date' }, { col: 0, type: 'text', desc: true }],
  [{ col: 0, type: 'text' }, { col: 1, type: 'number' }], [{ col: 2, type: 'text', desc: true }],
];
const FORMULAS = [
  '=SUM(ABOVE)', '=SUM(LEFT)', '=SUM(RIGHT)', '=SUM(BELOW)', '=AVERAGE(ABOVE)', '=MAX(LEFT)', '=A1+B2', '=SUM(A1:B2)', '=2^10', '=2^3^2', '=-2^2',
  '=ROUND(10/3;2)', '=IF(1<2;10;20)', '=10%', '=1/0', '=1+', '=PRODUCT(1;2;3;4)', '=MOD(10;3)', '=INT(-3,7)', '=SIGN(-5)', '=ABS(-2,5)', '=AND(1;0)',
  '=OR(0;0)', '=NOT(0)', '=COUNT(ABOVE)', '=MIN(1;2;3)', '=SUM(ABOVE)*2', '= 1+2 ', '=3,5*2', '=1.000,5+1', '=foo', '=(1+2)*3', '=ROUND(2,5;0)', '=ROUND(-2,5;0)',
  '=ROUND(1234,5678;-2)', '=A0', '=ZZ99', '=SUM(A1:A1)', '=TRUE+1', '=DEFINED(x)', '=1 2', '=)', '=2^0,5', '=(2^2)^2', '=10^-2', '=A1:B2', '=SUM(A1:B2;C3)',
];
const PICS = [undefined, '0,00', '#.##0,00', '0%', '0;(0);zero', "'x'0", '#', '0,0000'];
const NUMS = [
  '10', ' R$ 1.234,56', '(4)', '5%', '-3,5', '1.000', '1.2345', 'abc', '', 'US$ 2', '€3', '£ 4,0', '1,5,5', '+7', '12.345.678,9 x', '$ (5,5)', '(5', '5)', '٣',
  '0,10', '1.00', '.5', '-.5', '100%', 'R$', '2 itens', '\t8', '1.234.5', '12345.678',
];
const DATES = [
  '1/2/2020', '31/12/99', '2020-02-30', '29.02.2024', '1-1-30', '0050-02-29', '31/4/2020', '1 de março de 2020', '2020-1-5', '99/9/9999', '5/5/05', ' 3/3/2003 ', '', '01.13.2020', '2023-12-31',
];
const HEIGHTS = (n) => Array.from({ length: n }, (_, i) => 240 + 61 * i);

const rc = (r) => `${r.r1}:${r.c1}:${r.r2}:${r.c2}`;
const rectOf = (f, i) => ({ r1: +f[i], c1: +f[i + 1], r2: +f[i + 2], c2: +f[i + 3] });

function pickRect(rng, t) {
  const m = T.mapTable(t);
  const nr = t.rows.length, nc = Math.max(1, m.nCols);
  const r1 = rng.below(nr), c1 = rng.below(nc);
  const r2 = rng.below(3) === 0 ? r1 : Math.min(nr - 1, r1 + rng.below(3));
  const c2 = rng.below(3) === 0 ? c1 : Math.min(nc - 1, c1 + rng.below(3));
  return { r1, c1, r2, c2 };
}

function nextOp(rng, t) {
  const m = T.mapTable(t);
  const nr = t.rows.length, nc = Math.max(1, m.nCols);
  const k = rng.below(100);
  const R = () => pickRect(rng, t);
  if (k < 5) return `ir:${rc(R())}:${rng.below(2) ? 'a' : 'b'}`;
  if (k < 10) return `ic:${rc(R())}:${rng.below(2) ? 'l' : 'r'}`;
  if (k < 14) { const a = rng.below(nr); return `dr:${a}:${Math.min(nr - 1, a + rng.below(2))}`; }
  if (k < 18) { const a = rng.below(nc); return `dc:${a}:${Math.min(nc - 1, a + rng.below(2))}`; }
  if (k < 22) return `dl:${rc(R())}:${rng.below(2) ? 'l' : 'u'}`;
  if (k < 26) return `il:${rc(R())}:${rng.below(2) ? 'r' : 'd'}`;
  if (k < 33) return `mg:${rc(R())}`;
  if (k < 40) return `sp:${rc(R())}:${1 + rng.below(3)}:${1 + rng.below(3)}:${rng.below(2)}`;
  if (k < 42) return `st:${rng.below(nr + 1)}`;
  if (k < 46) { const a = rng.below(nr); return `rh:${a}:${Math.min(nr - 1, a + rng.below(2))}:${[0, 300, 720.4][rng.below(3)]}:${rng.below(2) ? 'exact' : 'atLeast'}`; }
  if (k < 48) { const a = rng.below(nr); return `dsr:${a}:${Math.min(nr - 1, a + rng.below(3))}`; }
  if (k < 52) { const a = rng.below(nc); return `cw:${a}:${Math.min(nc - 1, a + rng.below(2))}:${[500, 1440, 2999.6][rng.below(3)]}`; }
  if (k < 54) return `cc:${rc(R())}:${[700, 2000][rng.below(2)]}`;
  if (k < 56) return `pw:${rng.below(WIDTHS.length)}`;
  if (k < 58) { const a = rng.below(nc); return `cpct:${a}:${Math.min(nc - 1, a + rng.below(2))}:${[500, 1250, 2500][rng.below(3)]}`; }
  if (k < 60) { const a = rng.below(nc); return `dsc:${a}:${Math.min(nc - 1, a + rng.below(3))}`; }
  if (k < 62) return `af:${['content', 'window', 'fixed'][rng.below(3)]}:${[0, 9000][rng.below(2)]}`;
  if (k < 65) return `scp:${rc(R())}:${rng.below(CELLPATCH.length)}`;
  if (k < 67) { const a = rng.below(nr); return `srp:${a}:${Math.min(nr - 1, a + rng.below(2))}:${rng.below(ROWPATCH.length)}`; }
  if (k < 69) return `stp:${rng.below(TBLPATCH.length)}`;
  if (k < 70) return `tal:${rng.below(JCS.length)}`;
  if (k < 71) return `csp:${[0, 40, 15.5][rng.below(3)]}`;
  if (k < 73) return `tdr:${rc(R())}:${rng.below(DIRS.length)}`;
  if (k < 80) return `bd:${rc(R())}:${KINDS[rng.below(KINDS.length)]}:${rng.below(SPECS.length)}:${rng.below(4) === 0 ? 1 : 0}:${rng.below(STYLEB.length)}`;
  if (k < 83) return `shd:${rc(R())}:${rng.below(SHD.length)}:${rng.below(4) === 0 ? 1 : 0}`;
  if (k < 84) return `lk:${rng.below(LOOKS.length)}`;
  if (k < 86) return `t2t:${rng.below(SEPS.length)}`;
  if (k < 88) return `x2t:${rng.below(SEPS.length)}:${1 + rng.below(3)}:${rng.below(3)}`;
  if (k < 91) return `srt:${rng.below(nr)}:${nr - 1}:${rng.below(SORTKEYS.length)}:${rng.below(2)}`;
  if (k < 94) return `fx:${rng.below(nr)}:${rng.below(nc)}:${rng.below(FORMULAS.length)}:${rng.below(PICS.length)}`;
  if (k < 95) return `df:${rng.below(nr)}:${rng.below(nc)}`;
  if (k < 96) return `pc:${rng.below(NUMS.length)}`;
  if (k < 97) return `ps:${rng.below(DATES.length + NUMS.length)}`;
  if (k < 98) return 'clt';
  return 'rt';
}

const NORMAL = { after: 160, line: 259 };
function hasImage(t) {
  const blocksOf = (cell) => cell.blocks ?? [];
  for (const row of t.rows) for (const cell of row.cells) for (const b of blocksOf(cell)) {
    if (b.type === 'table') { if (hasImage(b)) return true; continue; }
    for (const r of b.paragraph.runs) if (r.image || r.textbox || r.shape || r.group) return true;
  }
  return false;
}
let nextIdTag = 0;

/** Runs one op on `box.t`; returns the `ret` text. */
function run(code, box) {
  const t = box.t;
  const f = code.split(':');
  switch (f[0]) {
    case 'ir': return `r${rc(T.insertRows(t, rectOf(f, 1), f[5] === 'a' ? 'above' : 'below'))}`;
    case 'ic': return `r${rc(T.insertColumns(t, rectOf(f, 1), f[5] === 'l' ? 'left' : 'right'))}`;
    case 'dr': return T.deleteRows(t, +f[1], +f[2]) ? 't' : 'f';
    case 'dc': return T.deleteColumns(t, +f[1], +f[2]) ? 't' : 'f';
    case 'dl': T.deleteCells(t, rectOf(f, 1), f[5] === 'l' ? 'left' : 'up'); return 'u';
    case 'il': return `r${rc(T.insertCells(t, rectOf(f, 1), f[5] === 'r' ? 'right' : 'down'))}`;
    case 'mg': { const r = rectOf(f, 1); if (!T.canMerge(t, r)) return 'n'; return `r${rc(T.mergeCells(t, r, { normalSpacing: NORMAL }))}`; }
    case 'sp': {
      const r = rectOf(f, 1);
      const heights = HEIGHTS(t.rows.length).slice(r.r1, r.r2 + 1);
      return `r${rc(T.splitCells(t, r, +f[5], +f[6], { mergeFirst: f[7] === '1', normalSpacing: NORMAL, rowHeights: heights }))}`;
    }
    case 'st': { const t2 = T.splitTable(t, +f[1]); return t2 ? `t2:${tdig(t2)}` : 'null'; }
    case 'rh': T.setRowHeight(t, +f[1], +f[2], +f[3], f[4]); return 'u';
    case 'dsr': { const a = +f[1], b = +f[2]; T.distributeRows(t, a, b, HEIGHTS(t.rows.length).slice(a, b + 1)); return 'u'; }
    case 'cw': T.setColumnWidth(t, +f[1], +f[2], +f[3]); return 'u';
    case 'cc': T.setCellWidth(t, rectOf(f, 1), +f[5]); return 'u';
    case 'pw': T.setPreferredWidth(t, WIDTHS[+f[1]], 8000); return 'u';
    case 'cpct': T.setColumnPct(t, +f[1], +f[2], +f[3]); return 'u';
    case 'dsc': T.distributeColumns(t, +f[1], +f[2]); return 'u';
    case 'af': T.autoFit(t, f[1], +f[2] || undefined); return 'u';
    case 'scp': T.setCellProps(t, rectOf(f, 1), CELLPATCH[+f[5]]); return 'u';
    case 'srp': T.setRowProps(t, +f[1], +f[2], ROWPATCH[+f[3]]); return 'u';
    case 'stp': T.setTableProps(t, TBLPATCH[+f[1]]); return 'u';
    case 'tal': T.setTableAlign(t, JCS[+f[1]]); return 'u';
    case 'csp': T.setCellSpacing(t, +f[1]); return 'u';
    case 'tdr': T.setTextDirection(t, rectOf(f, 1), DIRS[+f[5]]); return 'u';
    case 'bd': T.setBorders(t, rectOf(f, 1), f[5], SPECS[+f[6]], { wholeTable: f[7] === '1', styleBorders: STYLEB[+f[8]] }); return 'u';
    case 'shd': T.setShading(t, rectOf(f, 1), SHD[+f[5]], f[6] === '1'); return 'u';
    case 'lk': T.setLook(t, LOOKS[+f[1]]); return 'u';
    case 't2t': return `p${parasDig(T.tableToText(t, sepOf(+f[1])))}`;
    case 'x2t': {
      const paras = T.tableToText(t, 'tab');
      const o = { cols: +f[2], availTwips: 9000, indTwips: f[3] === '1' ? 360 : undefined, styleId: f[3] === '2' ? null : 'Tabelacomgrade', autofit: ['fixed', 'content', 'window'][+f[3]] };
      return `t${tdig(T.textToTable(paras, sepOf(+f[1]), o))}`;
    }
    case 'srt': {
      const keys = SORTKEYS[+f[3]];
      const r1 = +f[1] + (f[4] === '1' ? 1 : 0), r2 = +f[2];
      const ok = SORT.sortTable(t, r1, r2, { keys, caseSensitive: f[4] === '1', compare: compareText });
      return ok ? 't' : 'f';
    }
    case 'fx': {
      const env = { table: t, r: +f[1], c: +f[2] };
      const instr = FX.formulaInstr(FORMULAS[+f[3]], PICS[+f[4]]);
      let txt;
      try { txt = FX.formulaResultText(instr, env); } catch { txt = 'THROW'; }
      const ev = FX.evaluateFormula(FORMULAS[+f[3]], env);
      return `x${hex(txt)}|${'error' in ev ? hex(ev.error) : `${jnum(ev.value)},${ev.fmt.decimals},${ev.fmt.thousands ? 1 : 0},${hex(ev.fmt.prefix ?? '')},${hex(ev.fmt.suffix ?? '')}`}`;
    }
    case 'df': return `d${hex(FX.defaultFormula(t, +f[1], +f[2]))}`;
    case 'pc': { const x = FX.parseCellNumber(NUMS[+f[1]]); return x ? `n${jnum(x.n)},${x.decimals},${x.thousands ? 1 : 0},${hex(x.prefix ?? '')},${hex(x.suffix ?? '')}` : 'null'; }
    case 'ps': {
      const i = +f[1];
      const txt = i < DATES.length ? DATES[i] : NUMS[i - DATES.length];
      return `s${jnum(SORT.parseSortDate(txt))},${jnum(SORT.parseSortNumber(txt))}`;
    }
    // the TS clone is JSON: cell views come back apart (relink, as the undo does) and image bytes
    // become index-keyed objects (declared: not compared)
    case 'clt': return hasImage(t) ? 'c-img' : `c${tdig(T.relink(T.cloneTable(t)))}`;
    case 'rt': box.t = T.relink(roundTrip(t)); return 'u';
    default: throw new Error(`op ${code}`);
  }
}

// ─────────────── synthetic tables: newTable + texts (numbers, words, dates) ───────────────
const SYN_TEXTS = ['10', '20', '5,5', 'R$ 3,00', 'abc', '', '1.234,5', '(2)', 'b', 'A', 'á', '10%', '7/1/2021', '2020-01-03', 'x y', '-4', 'é', 'Z', '1,5', '0'];
/** `rows:cols:autofit:ind:style:colw:avail:seed` → a table with a text in every cell. */
function makeSynthetic(build) {
  const [rows, cols, af, ind, style, colw, avail, seed] = build.split(':').map(Number);
  const t = T.newTable(rows, cols, {
    availTwips: avail, indTwips: ind || undefined, autofit: ['fixed', 'content', 'window'][af],
    styleId: style === 1 ? null : style === 2 ? 'Outro1' : undefined, colTwips: colw || undefined,
  });
  t.rows.forEach((row, r) => row.cells.forEach((cell, c) => {
    const text = SYN_TEXTS[(r * 7 + c * 3 + seed) % SYN_TEXTS.length];
    if (text) T.setCellContent(cell, [{ type: 'paragraph', paragraph: { runs: [{ text, props: { ...DEFAULT_RUN_PROPS } }], props: {} } }]);
  }));
  T.syncLegacy(t);
  return t;
}

const out = new CaptureWriter([
  'D2b-F3 pure model files: docx-core table-ops.ts, table-sort.ts (editor compareText), table-formula.ts over corpus tables; JSON round trip + relink (undo)',
  'facets: op (encoded), ret, tbl, json; init record: tbl, json',
]);
out.meta('ops', OPS);
const files = corpus();
let nTables = 0, nDocs = 0;
for (const { rel, file } of files) {
  if (nDocs >= DOCS) break;
  let full;
  try { full = readFullDocument(DocxPackage.load(readBytes(file)), { sources: true }); } catch { continue; }
  const tables = full.blocks.filter((b) => b.type === 'table');
  if (!tables.length) continue;
  nDocs++;
  tables.slice(0, 3).forEach((table, ti) => {
    // very large tables only slow the replay: the first 60 rows
    const base = `${rel}#t${ti}`;
    const box = { t: table };
    if (DUMP === `${base}#init`) { console.log(JSON.stringify(box.t, (k, v) => (v instanceof Uint8Array ? `#${v.length}` : v))); process.exit(0); }
    out.set(`${base}#init`, 'tbl', tdig(box.t));
    out.set(`${base}#init`, 'json', tdig(T.relink(roundTrip(box.t))));
    let seed = 0xf3;
    for (let i = 0; i < base.length; i++) seed = (Math.imul(seed, 31) + base.charCodeAt(i)) >>> 0;
    const rng = new Rng(seed);
    for (let j = 0; j < OPS; j++) {
      if (!box.t.rows.length) break;
      const code = nextOp(rng, box.t);
      const key = `${base}#${j}`;
      out.set(key, 'op', code);
      let ret;
      try { ret = run(code, box); } catch (e) { ret = 'err'; }
      out.set(key, 'ret', ret);
      if (DUMP === key) { console.log(JSON.stringify(box.t, (k, v) => (v instanceof Uint8Array ? `#${v.length}` : v))); process.exit(0); }
      if (ret === 'err') break;
      if (!box.t.rows.length) { out.set(key, 'tbl', 'empty'); out.set(key, 'json', 'empty'); break; }
      out.set(key, 'tbl', tdig(box.t));
      out.set(key, 'json', tdig(T.relink(roundTrip(box.t))));
    }
    nTables++;
  });
}
for (let k = 0; k < SYN; k++) {
  const rngB = new Rng(0x5eed + k);
  const build = `${1 + rngB.below(7)}:${1 + rngB.below(5)}:${rngB.below(3)}:${rngB.below(2) * 360}:${rngB.below(3)}:${[0, 1500][rngB.below(2)]}:${[9000, 8504, 4000][rngB.below(3)]}:${rngB.below(20)}`;
  const base = `syn#t${k}`;
  const box = { t: makeSynthetic(build) };
  out.set(`${base}#init`, 'build', build);
  out.set(`${base}#init`, 'tbl', tdig(box.t));
  out.set(`${base}#init`, 'json', tdig(T.relink(roundTrip(box.t))));
  const rng = new Rng(0x7e57 + k);
  for (let j = 0; j < OPS; j++) {
    if (!box.t.rows.length) break;
    const code = nextOp(rng, box.t);
    const key = `${base}#${j}`;
    out.set(key, 'op', code);
    let ret;
    try { ret = run(code, box); } catch (e) { ret = 'err'; }
    out.set(key, 'ret', ret);
    if (DUMP === key) { console.log(JSON.stringify(box.t)); process.exit(0); }
    if (ret === 'err') break;
    if (!box.t.rows.length) { out.set(key, 'tbl', 'empty'); out.set(key, 'json', 'empty'); break; }
    out.set(key, 'tbl', tdig(box.t));
    out.set(key, 'json', tdig(T.relink(roundTrip(box.t))));
  }
  nTables++;
}
out.write(join(crate, 'tests', 'parity', 'd2b-f3-ops.capture'));
console.log(`d2b-f3-ops: ${nDocs} documents, ${nTables} tables x up to ${OPS} ops`);
