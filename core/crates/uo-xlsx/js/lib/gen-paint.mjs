// Seeded deterministic generator of PAINT workbooks (frontier C3c, record core/docs/modules/uo-xlsx.md "C3c — as
// built"): table styles (every built-in family light/medium/dark, custom styles with stripe sizes and dxf borders,
// banding options, header/total rows, outlines, overlapping tables, filters), pivot styles (compact and tabular
// layouts, subtotals, sub-headings, page fields), direct formatting and conditional formats over them, dense border
// conflicts (every style pair, colours, diagonals, merges), every pattern and gradient fill, row/column default
// styles, and hostile references. Pure Node: same group + index -> same bytes.
//
//   buildPaintWorkbook(group, i) -> { bytes, name, desc }     groups: paint1 (CI, committed), paint-big (local)
import { zipBytes } from '../../../uo-pptx/js/lib/zip.mjs';

export const PAINT_GROUPS = {
  paint1: { seedBase: 0x9a1000, count: 14 },
  'paint-big': { seedBase: 0x9a8000, count: 160 },
};
export const paintKey = (g, i) => `gen/${g}/${String(i).padStart(4, '0')}`;
export const paintFile = (g, i) => `${g}-${String(i).padStart(4, '0')}.xlsx`;

const NS = 'http://schemas.openxmlformats.org/spreadsheetml/2006/main';
const NSR = 'http://schemas.openxmlformats.org/officeDocument/2006/relationships';
const REL = 'http://schemas.openxmlformats.org/officeDocument/2006/relationships';
const PKG_REL = 'http://schemas.openxmlformats.org/package/2006/relationships';
const XML = '<?xml version="1.0" encoding="UTF-8" standalone="yes"?>\n';
const esc = (s) => String(s).replace(/&/g, '&amp;').replace(/</g, '&lt;').replace(/>/g, '&gt;').replace(/"/g, '&quot;');
export const colName = (c) => { let s = ''; for (c += 1; c > 0; c = Math.floor((c - 1) / 26)) s = String.fromCharCode(65 + ((c - 1) % 26)) + s; return s; };
const ref = (c, r) => `${colName(c)}${r + 1}`;
const rng = (a, b, c, d) => `${ref(a, b)}:${ref(c, d)}`;

class Rng {
  constructor(seed) { this.s = (seed >>> 0) || 1; }
  next() { let x = this.s; x ^= x << 13; x >>>= 0; x ^= x >>> 17; x ^= x << 5; x >>>= 0; this.s = x; return x; }
  below(n) { return this.next() % n; }
  pick(a) { return a[this.below(a.length)]; }
  chance(p) { return this.next() / 4294967296 < p; }
  int(a, b) { return a + this.below(b - a + 1); }
  hex6() { return this.next().toString(16).padStart(8, '0').slice(0, 6).toUpperCase(); }
}

export const BORDER_STYLES = ['hair', 'dashDotDot', 'dashDot', 'dotted', 'dashed', 'thin', 'mediumDashDotDot', 'slantDashDot', 'mediumDashDot', 'mediumDashed', 'medium', 'thick', 'double'];
export const PATTERNS = ['gray0625', 'gray125', 'lightGray', 'mediumGray', 'darkGray', 'darkHorizontal', 'lightHorizontal', 'darkVertical', 'lightVertical', 'darkDown', 'lightDown', 'darkUp', 'lightUp', 'darkGrid', 'lightGrid', 'darkTrellis', 'lightTrellis'];
const ELEMENTS = ['wholeTable', 'headerRow', 'totalRow', 'firstColumn', 'lastColumn', 'firstRowStripe', 'secondRowStripe', 'firstColumnStripe', 'secondColumnStripe', 'firstHeaderCell', 'lastHeaderCell', 'firstTotalCell', 'lastTotalCell'];
const PIVOT_ELEMENTS = ['firstSubtotalColumn', 'secondSubtotalColumn', 'thirdSubtotalColumn', 'firstSubtotalRow', 'secondSubtotalRow', 'thirdSubtotalRow', 'firstRowSubheading', 'secondRowSubheading', 'thirdRowSubheading', 'blankRow', 'pageFieldLabels', 'pageFieldValues'];
export const TABLE_STYLE_NAMES = [
  ...Array.from({ length: 21 }, (_, i) => `TableStyleLight${i + 1}`),
  ...Array.from({ length: 28 }, (_, i) => `TableStyleMedium${i + 1}`),
  ...Array.from({ length: 11 }, (_, i) => `TableStyleDark${i + 1}`),
];
export const PIVOT_STYLE_NAMES = ['Light', 'Medium', 'Dark'].flatMap((f) => Array.from({ length: 28 }, (_, i) => `PivotStyle${f}${i + 1}`));

const THEMES = [
  ['000000', 'FFFFFF', '44546A', 'E7E6E6', '4472C4', 'ED7D31', 'A5A5A5', 'FFC000', '5B9BD5', '70AD47'],
  ['1F1F1F', 'FAFAFA', '2E4057', 'F0EBD8', '048A81', 'E4572E', '54C6EB', 'F7B801', '8D6A9F', '9BC53D'],
  ['222222', 'F8F4E3', '5E3023', 'DCC9B6', 'C1666B', '4281A4', '48A9A6', 'E4DFDA', 'D4B483', '7D8491'],
];
const theme = (n) => {
  const c = THEMES[n % THEMES.length];
  const clr = (tag, v) => `<a:${tag}><a:srgbClr val="${v}"/></a:${tag}>`;
  return `${XML}<a:theme xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main" name="T${n}"><a:themeElements><a:clrScheme name="T${n}">${clr('dk1', c[0])}${clr('lt1', c[1])}${clr('dk2', c[2])}${clr('lt2', c[3])}${clr('accent1', c[4])}${clr('accent2', c[5])}${clr('accent3', c[6])}${clr('accent4', c[7])}${clr('accent5', c[8])}${clr('accent6', c[9])}${clr('hlink', '0563C1')}${clr('folHlink', '954F72')}</a:clrScheme><a:fontScheme name="Office"><a:majorFont><a:latin typeface="Calibri Light"/><a:ea typeface=""/><a:cs typeface=""/></a:majorFont><a:minorFont><a:latin typeface="Calibri"/><a:ea typeface=""/><a:cs typeface=""/></a:minorFont></a:fontScheme><a:fmtScheme name="Office"><a:fillStyleLst><a:solidFill><a:schemeClr val="phClr"/></a:solidFill><a:solidFill><a:schemeClr val="phClr"/></a:solidFill><a:solidFill><a:schemeClr val="phClr"/></a:solidFill></a:fillStyleLst><a:lnStyleLst><a:ln w="6350"><a:solidFill><a:schemeClr val="phClr"/></a:solidFill></a:ln><a:ln w="12700"><a:solidFill><a:schemeClr val="phClr"/></a:solidFill></a:ln><a:ln w="19050"><a:solidFill><a:schemeClr val="phClr"/></a:solidFill></a:ln></a:lnStyleLst><a:effectStyleLst><a:effectStyle><a:effectLst/></a:effectStyle><a:effectStyle><a:effectLst/></a:effectStyle><a:effectStyle><a:effectLst/></a:effectStyle></a:effectStyleLst><a:bgFillStyleLst><a:solidFill><a:schemeClr val="phClr"/></a:solidFill><a:solidFill><a:schemeClr val="phClr"/></a:solidFill><a:solidFill><a:schemeClr val="phClr"/></a:solidFill></a:bgFillStyleLst></a:fmtScheme></a:themeElements></a:theme>`;
};

/** A colour element: rgb, theme (+tint), indexed or auto. */
function colorEl(tag, R) {
  switch (R.below(6)) {
    case 0: return `<${tag} theme="${R.int(0, 9)}"/>`;
    case 1: return `<${tag} theme="${R.int(0, 9)}" tint="${R.pick(['0.39997558519241921', '-0.249977111117893', '0.79998168889431442', '-0.499984740745262'])}"/>`;
    case 2: return `<${tag} indexed="${R.int(8, 63)}"/>`;
    case 3: return `<${tag} auto="1"/>`;
    default: return `<${tag} rgb="FF${R.hex6()}"/>`;
  }
}

/** The workbook builder. */
class Book {
  constructor(themeN = 0) {
    this.themeN = themeN;
    this.fonts = ['<font><sz val="11"/><color theme="1"/><name val="Calibri"/><family val="2"/><scheme val="minor"/></font>'];
    this.fills = ['<fill><patternFill patternType="none"/></fill>', '<fill><patternFill patternType="gray125"/></fill>'];
    this.borders = ['<border><left/><right/><top/><bottom/><diagonal/></border>'];
    this.xfs = ['<xf numFmtId="0" fontId="0" fillId="0" borderId="0" xfId="0"/>'];
    this.dxfs = [];
    this.tableStyles = [];
    this.defaultTable = 'TableStyleMedium2';
    this.sheets = [];
    this.tableSeq = 0;
    this.pivotSeq = 0;
  }
  font(x) { this.fonts.push(`<font>${x}<sz val="11"/><name val="Calibri"/></font>`); return this.fonts.length - 1; }
  fill(x) { this.fills.push(`<fill>${x}</fill>`); return this.fills.length - 1; }
  solid(rgb) { return this.fill(`<patternFill patternType="solid"><fgColor rgb="FF${rgb}"/><bgColor indexed="64"/></patternFill>`); }
  border(x) { this.borders.push(`<border>${x}</border>`); return this.borders.length - 1; }
  xf({ font = 0, fill = 0, border = 0, numFmt = 0, align = '' } = {}) {
    const x = `<xf numFmtId="${numFmt}" fontId="${font}" fillId="${fill}" borderId="${border}" xfId="0"${fill ? ' applyFill="1"' : ''}${border ? ' applyBorder="1"' : ''}${font ? ' applyFont="1"' : ''}>${align}</xf>`.replace('></xf>', '/>');
    const i = this.xfs.indexOf(x);
    if (i >= 0) return i;
    this.xfs.push(x);
    return this.xfs.length - 1;
  }
  dxf(x) { this.dxfs.push(`<dxf>${x}</dxf>`); return this.dxfs.length - 1; }
  tableStyle(name, elements, pivot = false) {
    this.tableStyles.push(`<tableStyle name="${esc(name)}" pivot="${pivot ? 1 : 0}" count="${elements.length}">${elements.map(([type, dxfId, size]) => `<tableStyleElement type="${type}"${dxfId === undefined ? '' : ` dxfId="${dxfId}"`}${size === undefined ? '' : ` size="${size}"`}/>`).join('')}</tableStyle>`);
  }
  sheet(name) {
    const s = { name, cells: new Map(), tables: [], pivots: [], merges: [], cols: [], rowDefs: new Map(), cf: [], autoFilter: undefined, grid: true };
    this.sheets.push(s);
    return s;
  }
  build() {
    const sst = [];
    const parts = [];
    const ct = [];
    this.sheets.forEach((sh, si) => {
      const rows = new Map();
      for (const [k, v] of sh.cells) { const [c, r] = k.split(',').map(Number); if (!rows.has(r)) rows.set(r, new Map()); rows.get(r).set(c, v); }
      for (const r of sh.rowDefs.keys()) if (!rows.has(r)) rows.set(r, new Map());
      const rowXml = [...rows.entries()].sort((a, b) => a[0] - b[0]).map(([r, cs]) => {
        const rd = sh.rowDefs.get(r);
        const cells = [...cs.entries()].sort((a, b) => a[0] - b[0]).map(([c, v]) => {
          const at = ref(c, r);
          const s = v.s ? ` s="${v.s}"` : '';
          if (v.n !== undefined) return `<c r="${at}"${s}><v>${v.n}</v></c>`;
          if (v.t !== undefined) { let k = sst.indexOf(v.t); if (k < 0) { k = sst.length; sst.push(v.t); } return `<c r="${at}"${s} t="s"><v>${k}</v></c>`; }
          return `<c r="${at}"${s}/>`;
        }).join('');
        return `<row r="${r + 1}"${rd ? ` ${rd}` : ''}>${cells}</row>`;
      }).join('');
      const rels = [];
      let tableParts = '';
      const tp = [];
      for (const t of sh.tables) { const n = ++this.tableSeq; rels.push([`rId${rels.length + 1}`, `${REL}/table`, `../tables/table${n}.xml`]); tp.push(`<tablePart r:id="rId${rels.length}"/>`); parts.push([`xl/tables/table${n}.xml`, t.xml(n)]); ct.push([`/xl/tables/table${n}.xml`, 'application/vnd.openxmlformats-officedocument.spreadsheetml.table+xml']); }
      if (tp.length) tableParts = `<tableParts count="${tp.length}">${tp.join('')}</tableParts>`;
      for (const p of sh.pivots) { const n = ++this.pivotSeq; rels.push([`rId${rels.length + 1}`, `${REL}/pivotTable`, `../pivotTables/pivotTable${n}.xml`]); parts.push([`xl/pivotTables/pivotTable${n}.xml`, p]); ct.push([`/xl/pivotTables/pivotTable${n}.xml`, 'application/vnd.openxmlformats-officedocument.spreadsheetml.pivotTable+xml']); }
      if (rels.length) parts.push([`xl/worksheets/_rels/sheet${si + 1}.xml.rels`, `${XML}<Relationships xmlns="${PKG_REL}">${rels.map(([id, t, tg]) => `<Relationship Id="${id}" Type="${t}" Target="${tg}"/>`).join('')}</Relationships>`]);
      const cols = sh.cols.length ? `<cols>${sh.cols.join('')}</cols>` : '';
      const merges = sh.merges.length ? `<mergeCells count="${sh.merges.length}">${sh.merges.map((m) => `<mergeCell ref="${m}"/>`).join('')}</mergeCells>` : '';
      const af = sh.autoFilter ? sh.autoFilter : '';
      parts.push([`xl/worksheets/sheet${si + 1}.xml`, `${XML}<worksheet xmlns="${NS}" xmlns:r="${NSR}"><sheetViews><sheetView${sh.grid ? '' : ' showGridLines="0"'} workbookViewId="0"/></sheetViews><sheetFormatPr defaultRowHeight="15"/>${cols}<sheetData>${rowXml}</sheetData>${af}${merges}${sh.cf.join('')}${tableParts}</worksheet>`]);
    });
    const ctAll = [['/xl/workbook.xml', 'application/vnd.openxmlformats-officedocument.spreadsheetml.sheet.main+xml'], ['/xl/styles.xml', 'application/vnd.openxmlformats-officedocument.spreadsheetml.styles+xml'], ['/xl/theme/theme1.xml', 'application/vnd.openxmlformats-officedocument.theme+xml'], ['/xl/sharedStrings.xml', 'application/vnd.openxmlformats-officedocument.spreadsheetml.sharedStrings+xml'],
      ...this.sheets.map((_, i) => [`/xl/worksheets/sheet${i + 1}.xml`, 'application/vnd.openxmlformats-officedocument.spreadsheetml.worksheet+xml']), ...ct];
    const n = this.sheets.length;
    const wbRels = [...this.sheets.map((_, i) => `<Relationship Id="rId${i + 1}" Type="${REL}/worksheet" Target="worksheets/sheet${i + 1}.xml"/>`), `<Relationship Id="rId${n + 1}" Type="${REL}/styles" Target="styles.xml"/>`, `<Relationship Id="rId${n + 2}" Type="${REL}/theme" Target="theme/theme1.xml"/>`, `<Relationship Id="rId${n + 3}" Type="${REL}/sharedStrings" Target="sharedStrings.xml"/>`];
    const styles = `${XML}<styleSheet xmlns="${NS}"><fonts count="${this.fonts.length}">${this.fonts.join('')}</fonts><fills count="${this.fills.length}">${this.fills.join('')}</fills><borders count="${this.borders.length}">${this.borders.join('')}</borders><cellStyleXfs count="1"><xf numFmtId="0" fontId="0" fillId="0" borderId="0"/></cellStyleXfs><cellXfs count="${this.xfs.length}">${this.xfs.join('')}</cellXfs><cellStyles count="1"><cellStyle name="Normal" xfId="0" builtinId="0"/></cellStyles><dxfs count="${this.dxfs.length}">${this.dxfs.join('')}</dxfs><tableStyles count="${this.tableStyles.length}" defaultTableStyle="${this.defaultTable}" defaultPivotStyle="PivotStyleLight16">${this.tableStyles.join('')}</tableStyles></styleSheet>`;
    return zipBytes([
      ['[Content_Types].xml', `${XML}<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/><Default Extension="xml" ContentType="application/xml"/>${ctAll.map(([p, t]) => `<Override PartName="${p}" ContentType="${t}"/>`).join('')}</Types>`],
      ['_rels/.rels', `${XML}<Relationships xmlns="${PKG_REL}"><Relationship Id="rId1" Type="${REL}/officeDocument" Target="xl/workbook.xml"/></Relationships>`],
      ['xl/workbook.xml', `${XML}<workbook xmlns="${NS}" xmlns:r="${NSR}"><workbookPr/><sheets>${this.sheets.map((s, i) => `<sheet name="${esc(s.name)}" sheetId="${i + 1}" r:id="rId${i + 1}"/>`).join('')}</sheets><calcPr calcId="191029"/></workbook>`],
      ['xl/_rels/workbook.xml.rels', `${XML}<Relationships xmlns="${PKG_REL}">${wbRels.join('')}</Relationships>`],
      ['xl/styles.xml', styles],
      ['xl/theme/theme1.xml', theme(this.themeN)],
      ...parts,
      ['xl/sharedStrings.xml', `${XML}<sst xmlns="${NS}" count="${sst.length}" uniqueCount="${sst.length}">${sst.map((t) => `<si><t xml:space="preserve">${esc(t)}</t></si>`).join('')}</sst>`],
    ]);
  }
}

/** A table part. */
function table(c, r, w, h, { style, header = true, totals = false, rowStripes = true, colStripes = false, first = false, last = false, filter = false, hidden = [], active = [], borderDxf, name, headerCount } = {}) {
  return {
    c, r, w, h,
    xml: (n) => {
      const cols = Array.from({ length: w }, (_, k) => `<tableColumn id="${k + 1}" name="Col${k + 1}"/>`).join('');
      const af = filter ? `<autoFilter ref="${rng(c, r, c + w - 1, r + h - 1)}">${[...hidden.map((k) => `<filterColumn colId="${k}" hiddenButton="1"/>`), ...active.map((k) => `<filterColumn colId="${k}"><filters><filter val="x"/></filters></filterColumn>`)].join('')}</autoFilter>` : '';
      const si = style === null ? '' : `<tableStyleInfo${style === undefined ? '' : ` name="${esc(style)}"`} showFirstColumn="${first ? 1 : 0}" showLastColumn="${last ? 1 : 0}" showRowStripes="${rowStripes ? 1 : 0}" showColumnStripes="${colStripes ? 1 : 0}"/>`;
      return `${XML}<table xmlns="${NS}" id="${n}" name="${name ?? `T${n}`}" displayName="${name ?? `T${n}`}" ref="${rng(c, r, c + w - 1, r + h - 1)}"${headerCount !== undefined ? ` headerRowCount="${headerCount}"` : header ? '' : ' headerRowCount="0"'}${totals ? ' totalsRowCount="1"' : ''}${borderDxf !== undefined ? ` tableBorderDxfId="${borderDxf}"` : ''}>${af}<tableColumns count="${w}">${cols}</tableColumns>${si}</table>`;
    },
  };
}

function fillTable(sh, t, R, xf = 0) {
  for (let k = 0; k < t.h; k++) for (let j = 0; j < t.w; j++) sh.cells.set(`${t.c + j},${t.r + k}`, k === 0 && t.header !== false ? { t: `H${j + 1}`, s: xf } : { n: R.int(1, 99), s: xf });
}

/** A pivot definition: `loc` = [c1,r1,c2,r2]. */
function pivot(R, loc, o = {}) {
  const compact = o.compact ?? R.chance(0.5);
  const nRow = o.rowItems ?? R.int(2, 6);
  const rowItems = [];
  const nRowF = o.rowFields ?? R.pick([1, 1, 2]);
  for (let k = 0; k < nRow; k++) {
    const t = R.below(8);
    const r = nRowF > 1 ? R.int(0, nRowF - 1) : 0;
    if (t === 0 && nRowF > 1) rowItems.push(`<i t="default" r="${r}"><x/></i>`);
    else if (t === 1) rowItems.push(`<i t="blank" r="${r}"><x/></i>`);
    else if (t === 2) rowItems.push(`<i t="sum" r="${r}"><x v="${R.int(0, 3)}"/></i>`);
    else rowItems.push(`<i r="${r}">${'<x/>'.repeat(R.int(1, Math.max(1, nRowF)))}</i>`);
  }
  const grandRow = o.grandRow ?? R.chance(0.7);
  if (grandRow) rowItems.push('<i t="grand"><x/></i>');
  const nCol = o.colItems ?? R.int(1, 5);
  const colItems = [];
  for (let k = 0; k < nCol; k++) colItems.push(R.below(5) === 0 ? '<i t="default" r="0"><x/></i>' : '<i><x/></i>');
  const grandCol = o.grandCol ?? R.chance(0.6);
  if (grandCol) colItems.push('<i t="grand"><x/></i>');
  const pages = o.pages ?? R.pick([0, 0, 1, 2, 3, 5]);
  const rowF = Array.from({ length: nRowF }, (_, k) => `<field x="${k}"/>`).join('');
  const colF = R.chance(0.5) ? '<field x="2"/>' : R.chance(0.5) ? '<field x="-2"/>' : '';
  const pageFields = pages ? `<pageFields count="${pages}">${Array.from({ length: pages }, (_, k) => `<pageField fld="${k + 3}"${R.chance(0.4) ? ` item="${R.int(0, 2)}"` : ''} hier="-1"/>`).join('')}</pageFields>` : '';
  const pfs = `<pivotFields count="6">${Array.from({ length: 6 }, (_, k) => `<pivotField${k < nRowF ? ' axis="axisRow"' : ''} showAll="0">${R.chance(0.2) ? '<items count="2"><item x="0" h="1"/><item t="default"/></items>' : ''}</pivotField>`).join('')}</pivotFields>`;
  const filters = R.chance(0.15) ? '<filters count="1"><filter fld="1" type="captionEqual" evalOrder="-1" id="1"/></filters>' : '';
  const style = o.style === null ? '' : `<pivotTableStyleInfo${o.style === undefined ? '' : ` name="${esc(o.style)}"`} showRowHeaders="${o.rh ?? 1}" showColHeaders="${o.ch ?? 1}" showRowStripes="${o.rs ?? R.below(2)}" showColStripes="${o.cs ?? R.below(2)}" showLastColumn="${o.lc ?? R.below(2)}"/>`;
  return `${XML}<pivotTableDefinition xmlns="${NS}" name="PT${R.below(1000)}" cacheId="1" compact="${compact ? 1 : 0}" compactData="${compact ? 1 : 0}" outline="${R.below(2)}" outlineData="1" showHeaders="${o.showHeaders ?? R.pick([1, 1, 0])}" rowGrandTotals="${grandRow ? 1 : 0}" colGrandTotals="${grandCol ? 1 : 0}"><location ref="${rng(...o.loc)}" firstHeaderRow="${o.fh ?? R.pick([0, 1, 1, 2])}" firstDataRow="${o.fdr ?? R.pick([1, 2, 2, 3])}" firstDataCol="${o.fdc ?? R.pick([1, 1, 2])}"${pages ? ` rowPageCount="${R.pick([1, pages, 2])}" colPageCount="1"` : ''}/>${pfs}<rowFields count="${nRowF}">${rowF}</rowFields><rowItems count="${rowItems.length}">${rowItems.join('')}</rowItems>${colF ? `<colFields count="1">${colF}</colFields>` : ''}<colItems count="${colItems.length}">${colItems.join('')}</colItems>${pageFields}${filters}${style}</pivotTableDefinition>`;
}

function pivotSheet(sh, R, idx, name, o = {}) {
  const c = o.c ?? 1, r = o.r ?? (4 + idx * 12);
  const h = o.h ?? R.int(5, 9), w = o.w ?? R.int(3, 6);
  for (let k = 0; k < h; k++) for (let j = 0; j < w; j++) sh.cells.set(`${c + j},${r + k}`, j === 0 || k === 0 ? { t: `L${j}${k}` } : { n: R.int(1, 50) });
  sh.pivots.push(pivot(R, [], { ...o, loc: [c, r, c + w - 1, r + h - 1], style: name }));
}

const WORD = ['alfa', 'beta', 'gama', 'delta'];
function plainTable(b, sh, R, c, r, w, h, style, extra = {}) {
  const t = table(c, r, w, h, { style, ...extra });
  sh.tables.push(t);
  fillTable(sh, t, R);
  return t;
}

// ---------------------------------------------------------------- the CI workbooks
function family(i, names, themeN, label) {
  const R = new Rng(PAINT_GROUPS.paint1.seedBase + i);
  const b = new Book(themeN);
  const sh = b.sheet(label);
  names.forEach((nm, k) => {
    const o = { header: true, totals: k % 3 !== 1, rowStripes: k % 4 !== 3, colStripes: k % 5 === 2, first: k % 2 === 0, last: k % 3 === 0 };
    plainTable(b, sh, R, 1 + (k % 2) * 6, 1 + Math.floor(k / 2) * 9, 4, 7, nm, o);
  });
  return b;
}

function options(i) {
  const R = new Rng(PAINT_GROUPS.paint1.seedBase + i);
  const b = new Book(1);
  const sh = b.sheet('opts');
  const styles = ['TableStyleMedium2', 'TableStyleLight8', 'TableStyleDark3', 'TableStyleMedium9'];
  for (let m = 0; m < 64; m++) {
    const o = { header: !(m & 1), totals: !!(m & 2), rowStripes: !!(m & 4), colStripes: !!(m & 8), first: !!(m & 16), last: !!(m & 32) };
    plainTable(b, sh, R, 1 + (m % 8) * 5, 1 + Math.floor(m / 8) * 7, 4, 5, styles[m % 4], o);
  }
  return b;
}

function custom(i) {
  const R = new Rng(PAINT_GROUPS.paint1.seedBase + i);
  const b = new Book(2);
  const sh = b.sheet('custom');
  const d = {
    whole: b.dxf('<font><color rgb="FF333333"/></font><fill><patternFill patternType="solid"><bgColor rgb="FFF2F2F2"/></patternFill></fill><border><left style="thin"><color rgb="FF808080"/></left><right style="thin"><color rgb="FF808080"/></right><top style="thin"><color rgb="FF808080"/></top><bottom style="thin"><color rgb="FF808080"/></bottom><vertical style="hair"><color rgb="FFBFBFBF"/></vertical><horizontal style="hair"><color rgb="FFBFBFBF"/></horizontal></border>'),
    head: b.dxf('<font><b/><color theme="0"/></font><fill><patternFill patternType="solid"><bgColor theme="4" tint="-0.249977111117893"/></patternFill></fill><border><bottom style="double"><color theme="1"/></bottom></border>'),
    total: b.dxf('<font><b/><i/></font><fill><patternFill patternType="solid"><bgColor rgb="FFFFE699"/></patternFill></fill><border><top style="medium"><color rgb="FF000000"/></top></border>'),
    s1: b.dxf('<fill><patternFill patternType="solid"><bgColor rgb="FFDDEBF7"/></patternFill></fill>'),
    s2: b.dxf('<fill><patternFill patternType="solid"><bgColor rgb="FFFCE4D6"/></patternFill></fill><border><top style="dotted"><color rgb="FFC00000"/></top></border>'),
    col: b.dxf('<font><u/><strike/><color rgb="FF7030A0"/></font><border><left style="thick"><color rgb="FF7030A0"/></left></border>'),
    corner: b.dxf('<font><b val="0"/><color rgb="FFFF0000"/></font><fill><patternFill patternType="gray125"><fgColor rgb="FF000000"/><bgColor rgb="FFFFFF00"/></patternFill></fill>'),
    none: b.dxf('<fill><patternFill patternType="none"/></fill>'),
    outline: b.dxf('<border><left style="mediumDashDot"><color rgb="FF00B050"/></left><right style="mediumDashed"><color rgb="FF00B050"/></right><top style="slantDashDot"><color rgb="FF00B050"/></top><bottom style="thick"><color rgb="FF00B050"/></bottom></border>'),
  };
  b.tableStyle('MyStyle', [['wholeTable', d.whole], ['headerRow', d.head], ['totalRow', d.total], ['firstRowStripe', d.s1, 2], ['secondRowStripe', d.s2, 3], ['firstColumn', d.col], ['lastColumn', d.col], ['firstColumnStripe', d.s1, 2], ['secondColumnStripe', d.none], ['firstHeaderCell', d.corner], ['lastHeaderCell', d.corner], ['firstTotalCell', d.corner], ['lastTotalCell', d.corner]]);
  b.tableStyle('Sparse', [['wholeTable'], ['headerRow', d.head, 5], ['firstRowStripe', d.s1, 1.5], ['bogusElement', d.s1], ['lastColumn', 99], ['totalRow', d.total]]);
  b.tableStyle('MyPivot', [['wholeTable', d.whole], ['headerRow', d.head], ['firstRowStripe', d.s1, 2], ['totalRow', d.total], ['firstSubtotalRow', d.s2], ['secondSubtotalRow', d.col], ['blankRow', d.corner], ['pageFieldLabels', d.head], ['pageFieldValues', d.s1], ['firstRowSubheading', d.s2], ['firstSubtotalColumn', d.col]], true);
  const configs = [
    { style: 'MyStyle', totals: true, rowStripes: true, colStripes: true, first: true, last: true },
    { style: 'MyStyle', totals: false, rowStripes: true, colStripes: false, first: false, last: true },
    { style: 'Sparse', totals: true, rowStripes: true, colStripes: true, first: true, last: true },
    { style: 'MyStyle', totals: true, rowStripes: false, colStripes: true, first: true, last: false, borderDxf: d.outline },
    { style: 'TableStyleMedium2', totals: true, rowStripes: true, borderDxf: d.outline },
    { style: 'NoSuchStyle', totals: true, rowStripes: true, borderDxf: d.outline },
    { style: '', totals: true, rowStripes: true },
    { style: null, totals: false, rowStripes: true, borderDxf: d.outline },
    { style: undefined, totals: false, rowStripes: true, borderDxf: 99 },
    { style: 'MyStyle', header: false, totals: false, rowStripes: true, first: true },
  ];
  configs.forEach((o, k) => plainTable(b, sh, R, 1 + (k % 2) * 7, 1 + Math.floor(k / 2) * 10, 5, 8, o.style, o));
  // custom pivot styles
  const sh2 = b.sheet('cpivot');
  for (let k = 0; k < 6; k++) pivotSheet(sh2, R, k, k % 2 ? 'MyPivot' : 'PivotStyleMedium9', { c: 1 + (k % 2) * 8, r: 5 + Math.floor(k / 2) * 13, w: 5, h: 8 });
  return b;
}

function gradients(b, R) {
  const lin = (deg, stops) => b.fill(`<gradientFill degree="${deg}">${stops.map(([p, c]) => `<stop position="${p}"><color rgb="FF${c}"/></stop>`).join('')}</gradientFill>`);
  const path = (l, r, t, bt, stops) => b.fill(`<gradientFill type="path" left="${l}" right="${r}" top="${t}" bottom="${bt}">${stops.map(([p, c]) => `<stop position="${p}"><color rgb="FF${c}"/></stop>`).join('')}</gradientFill>`);
  return [
    lin(0, [[0, 'FF0000'], [1, '0000FF']]), lin(45, [[0, 'FFFFFF'], [0.5, '00B050'], [1, '000000']]), lin(90, [[0, 'FFC000'], [1, '7030A0']]), lin(135, [[0, '00FFFF'], [1, 'FF00FF']]),
    lin(270, [[0, '123456'], [0.3, '654321'], [1, 'ABCDEF']]), lin(360, [[0, 'FF0000'], [1, '00FF00']]), lin(-30, [[0, 'FF0000'], [1, '00FF00']]), lin(12.5, [[1, 'FF0000'], [0, '00FF00']]),
    path(0.5, 0.5, 0.5, 0.5, [[0, 'FFFFFF'], [1, '4472C4']]), path(0, 1, 0, 1, [[0, 'FF0000'], [0.5, 'FFFF00'], [1, '00B050']]), path(0.25, 0.75, 0.25, 0.75, [[0, '000000'], [1, 'FFFFFF']]), path(0, 0, 0, 0, [[0, 'FFFFFF'], [1, '000000']]),
    lin(30, [[0.5, '777777']]), path(0.5, 0.5, 0.5, 0.5, [[1, 'FFFFFF'], [0, 'FF0000']]),
  ];
}

function direct(i) {
  const R = new Rng(PAINT_GROUPS.paint1.seedBase + i);
  const b = new Book(0);
  const sh = b.sheet('direct');
  const red = b.font('<color rgb="FFFF0000"/>');
  const same = b.font('<color rgb="FF000000"/>');
  const themeBlack = b.font('<color theme="1"/>');
  const bold = b.font('<b/>');
  const ital = b.font('<i/><u/><strike/><color rgb="FFFFFFFF"/>');
  const yellow = b.solid('FFFF00');
  const patt = b.fill('<patternFill patternType="darkTrellis"><fgColor rgb="FF0000FF"/><bgColor rgb="FFFFFFFF"/></patternFill>');
  const bdAll = b.border('<left style="thin"><color rgb="FFFF0000"/></left><right style="medium"><color rgb="FF00FF00"/></right><top style="hair"><color rgb="FF0000FF"/></top><bottom style="double"><color rgb="FF000000"/></bottom><diagonal/>');
  const bdBot = b.border('<left/><right/><top/><bottom style="thick"><color rgb="FF7030A0"/></bottom><diagonal/>');
  const xfs = [
    b.xf({ fill: yellow }), b.xf({ font: red }), b.xf({ font: same }), b.xf({ font: themeBlack }), b.xf({ border: bdAll }), b.xf({ border: bdBot }), b.xf({ fill: patt, font: bold }),
    b.xf({ font: ital }), b.xf({ font: bold, border: bdBot, fill: yellow }),
  ];
  const styles = ['TableStyleMedium2', 'TableStyleLight9', 'TableStyleDark1', 'TableStyleMedium16', 'TableStyleLight15'];
  styles.forEach((st, k) => {
    const t = plainTable(b, sh, R, 1 + (k % 2) * 8, 1 + Math.floor(k / 2) * 12, 6, 9, st, { totals: true, rowStripes: true, colStripes: k % 2 === 1, first: true, last: k % 3 === 0 });
    for (let n = 0; n < 14; n++) { const cc = t.c + R.below(t.w), rr = t.r + R.below(t.h); const cell = sh.cells.get(`${cc},${rr}`); if (cell) cell.s = R.pick(xfs); }
  });
  // conditional formats over the tables: a dxf with fill/font/border, and colour scales / data bars
  const dx1 = b.dxf('<font><b/><i/><color rgb="FF9C0006"/></font><fill><patternFill><bgColor rgb="FFFFC7CE"/></patternFill></fill><border><left style="thin"><color rgb="FFFF0000"/></left><bottom style="mediumDashed"><color rgb="FF0000FF"/></bottom></border>');
  const dx2 = b.dxf('<font><u/></font><fill><patternFill patternType="lightGray"><fgColor rgb="FF00B050"/><bgColor rgb="FFFFFFFF"/></patternFill></fill>');
  sh.cf.push(`<conditionalFormatting sqref="B3:G8"><cfRule type="cellIs" dxfId="${dx1}" priority="1" operator="greaterThan"><formula>60</formula></cfRule></conditionalFormatting>`);
  sh.cf.push(`<conditionalFormatting sqref="I3:N9"><cfRule type="cellIs" dxfId="${dx2}" priority="2" operator="lessThan"><formula>30</formula></cfRule><cfRule type="colorScale" priority="3"><colorScale><cfvo type="min"/><cfvo type="max"/><color rgb="FFFFFFFF"/><color rgb="FF63BE7B"/></colorScale></cfRule></conditionalFormatting>`);
  sh.cf.push(`<conditionalFormatting sqref="B15:G21"><cfRule type="colorScale" priority="4"><colorScale><cfvo type="min"/><cfvo type="percentile" val="50"/><cfvo type="max"/><color rgb="FFF8696B"/><color rgb="FFFFEB84"/><color rgb="FF63BE7B"/></colorScale></cfRule><cfRule type="expression" dxfId="${dx1}" priority="5"><formula>MOD(ROW(),2)=0</formula></cfRule></conditionalFormatting>`);
  // pivots with direct formatting
  const sh2 = b.sheet('pivdirect');
  for (let k = 0; k < 4; k++) {
    pivotSheet(sh2, R, k, PIVOT_STYLE_NAMES[k * 21 % 84], { c: 1 + (k % 2) * 8, r: 5 + Math.floor(k / 2) * 13, w: 6, h: 8, pages: k });
    for (let n = 0; n < 6; n++) { const cell = sh2.cells.get(`${1 + (k % 2) * 8 + R.below(6)},${5 + Math.floor(k / 2) * 13 + R.below(8)}`); if (cell) cell.s = R.pick(xfs); }
  }
  void gradients;
  return b;
}

function filters(i) {
  const R = new Rng(PAINT_GROUPS.paint1.seedBase + i);
  const b = new Book(1);
  const sh = b.sheet('filters');
  plainTable(b, sh, R, 1, 1, 6, 8, 'TableStyleMedium2', { filter: true, hidden: [1, 4], active: [0, 4], totals: true });
  plainTable(b, sh, R, 9, 1, 4, 6, 'TableStyleLight14', { filter: true, header: false });
  plainTable(b, sh, R, 1, 12, 5, 6, 'TableStyleDark9', { filter: false });
  plainTable(b, sh, R, 9, 10, 3, 5, 'TableStyleMedium28', { filter: true, active: [1] });
  sh.autoFilter = `<autoFilter ref="${rng(15, 1, 19, 8)}"><filterColumn colId="2" hiddenButton="1"/><filterColumn colId="3"><filters><filter val="1"/></filters></filterColumn></autoFilter>`;
  for (let k = 0; k < 9; k++) for (let j = 15; j <= 19; j++) sh.cells.set(`${j},${k}`, k === 0 ? { t: `A${j}` } : { n: k * j });
  // a pivot overlapping a table's rows; the autofilter overlapping a table header
  const sh2 = b.sheet('pivots');
  for (let k = 0; k < 4; k++) pivotSheet(sh2, R, k, PIVOT_STYLE_NAMES[k * 27 % 84], { c: 1 + (k % 2) * 9, r: 6 + Math.floor(k / 2) * 14, w: 6, h: 8, showHeaders: 1, pages: k % 3 });
  return b;
}

function borderXf(b, R, o = {}) {
  const edge = (tag) => (R.chance(o.p ?? 0.5) ? `<${tag} style="${R.pick(BORDER_STYLES)}">${colorEl('color', R)}</${tag}>` : `<${tag}/>`);
  const diag = R.chance(o.diag ?? 0.12) ? `<diagonal style="${R.pick(BORDER_STYLES)}">${colorEl('color', R)}</diagonal>` : '<diagonal/>';
  const attr = diag !== '<diagonal/>' ? ` diagonalUp="${R.below(2)}" diagonalDown="${R.below(2)}"` : '';
  const bi = b.borders.length;
  b.borders.push(`<border${attr}>${edge('left')}${edge('right')}${edge('top')}${edge('bottom')}${diag}</border>`);
  return bi;
}

function borders(i, mode) {
  const R = new Rng(PAINT_GROUPS.paint1.seedBase + i);
  const b = new Book(mode);
  const sh = b.sheet('borders');
  sh.grid = mode !== 2;
  if (mode === 0) {
    // every style against every style: cell (i, j) has bottom style i, its neighbour below has top style j; same for right/left
    const n = BORDER_STYLES.length;
    const bd = {};
    BORDER_STYLES.forEach((s, k) => {
      bd[`b${k}`] = b.border(`<left/><right/><top/><bottom style="${s}"><color rgb="FF${k % 2 ? 'FF0000' : '0000FF'}"/></bottom><diagonal/>`);
      bd[`t${k}`] = b.border(`<left/><right/><top style="${s}"><color rgb="FF00${(k * 17).toString(16).padStart(2, '0')}00"/></top><bottom/><diagonal/>`);
      bd[`r${k}`] = b.border(`<left/><right style="${s}"><color rgb="FF${k % 2 ? 'FF00FF' : '00AA00'}"/></right><top/><bottom/><diagonal/>`);
      bd[`l${k}`] = b.border(`<left style="${s}"><color rgb="FFAA${(k * 17).toString(16).padStart(2, '0')}00"/></left><right/><top/><bottom/><diagonal/>`);
    });
    for (let a = 0; a < n; a++) for (let c = 0; c < n; c++) {
      const r0 = 1 + a * 4, c0 = 1 + c * 4;
      sh.cells.set(`${c0},${r0}`, { n: 1, s: b.xf({ border: bd[`b${a}`] }) });
      sh.cells.set(`${c0},${r0 + 1}`, { n: 2, s: b.xf({ border: bd[`t${c}`] }) });
      sh.cells.set(`${c0 + 1},${r0}`, { n: 3, s: b.xf({ border: bd[`r${a}`] }) });
      sh.cells.set(`${c0 + 2},${r0}`, { n: 4, s: b.xf({ border: bd[`l${c}`] }) });
    }
  } else {
    const nfill = [b.solid('FFFF00'), b.solid('C6EFCE'), ...PATTERNS.map((p) => b.fill(`<patternFill patternType="${p}"><fgColor rgb="FF${R.hex6()}"/><bgColor rgb="FF${R.hex6()}"/></patternFill>`)), ...gradients(b, R)];
    const xfs = [];
    for (let k = 0; k < 60; k++) xfs.push(b.xf({ border: borderXf(b, R, { p: 0.55 }), fill: R.chance(0.4) ? R.pick(nfill) : 0 }));
    const rows = 40, cols = 24;
    for (let r = 0; r < rows; r++) for (let c = 0; c < cols; c++) if (R.chance(0.7)) sh.cells.set(`${1 + c},${1 + r}`, { n: r * c, s: R.pick(xfs) });
    for (let k = 0; k < 12; k++) { const c = R.int(1, 20), r = R.int(1, 35); sh.merges.push(rng(c, r, c + R.int(0, 3), r + R.int(0, 3))); }
    sh.cols.push(`<col min="3" max="4" width="${R.pick([2, 5, 12])}" customWidth="1" style="${R.pick(xfs)}"/>`, '<col min="9" max="9" width="20" customWidth="1" hidden="1"/>');
    sh.rowDefs.set(5, `s="${R.pick(xfs)}" customFormat="1" ht="30" customHeight="1"`);
    sh.rowDefs.set(9, `s="${R.pick(xfs)}" ht="12" customHeight="1"`);
    sh.rowDefs.set(12, 'hidden="1"');
    const t = plainTable(b, sh, R, 12, 14, 8, 10, 'TableStyleMedium7', { totals: true, first: true });
    for (let n = 0; n < 12; n++) { const cell = sh.cells.get(`${t.c + R.below(t.w)},${t.r + R.below(t.h)}`); if (cell) cell.s = R.pick(xfs); }
  }
  return b;
}

function hostile(i) {
  const R = new Rng(PAINT_GROUPS.paint1.seedBase + i);
  const b = new Book(0);
  const sh = b.sheet('hostile');
  for (let r = 0; r < 12; r++) for (let c = 0; c < 8; c++) sh.cells.set(`${c},${r}`, { n: r + c, s: R.pick([0, 1, 9999, 4294967295, 7]) });
  const mk = (r, o = {}) => { const t = table(0, 0, 4, 5, o); t.xml = (n) => `${XML}<table xmlns="${NS}" id="${n}" name="H${n}" displayName="H${n}" ref="${r}"><tableColumns count="1"><tableColumn id="1" name="a"/></tableColumns><tableStyleInfo name="TableStyleMedium2" showFirstColumn="1" showLastColumn="1" showRowStripes="1" showColumnStripes="1"/></table>`; return t; };
  for (const r of ['A1', 'B2:D6', 'B2:D6', 'C3:E9', 'A0:C2', ':', 'nope', 'A1:B2:C3', '$B$2:$C$3', 'XFD1048576', 'A1:A1']) sh.tables.push(mk(r));
  for (let k = 0; k < 4; k++) sh.pivots.push(pivot(R, [], { loc: [1 + k, 1 + k, 6 + k, 8 + k], style: PIVOT_STYLE_NAMES[k * 9], pages: k * 5, fh: -3, fdr: 99, fdc: -1 }));
  sh.pivots.push(`${XML}<pivotTableDefinition xmlns="${NS}" name="x"><location ref="B2:F9"/><rowItems><i t="grand"/></rowItems><colItems><i t="grand"/></colItems><pivotTableStyleInfo name="PivotStyleDark3" showRowHeaders="1" showColHeaders="1" showRowStripes="1" showColStripes="1" showLastColumn="1"/></pivotTableDefinition>`);
  sh.pivots.push(`${XML}<pivotTableDefinition xmlns="${NS}" name="y"><location ref="bad"/></pivotTableDefinition>`);
  return b;
}

/** A random workbook mixing everything (the local group). */
function random(seed) {
  const R = new Rng(seed);
  const b = new Book(R.below(3));
  const nSheets = R.int(1, 3);
  const xfs = [0];
  const nfill = [b.solid(R.hex6()), b.solid(R.hex6()), ...PATTERNS.filter(() => R.chance(0.4)).map((p) => b.fill(`<patternFill patternType="${p}"><fgColor rgb="FF${R.hex6()}"/><bgColor rgb="FF${R.hex6()}"/></patternFill>`)), ...gradients(b, R).filter(() => R.chance(0.3))];
  const fonts = [b.font(`<color rgb="FF${R.hex6()}"/>`), b.font('<b/><i/>'), b.font('<u/><strike/>'), b.font('<color theme="1"/>'), b.font('<color rgb="FF000000"/>')];
  for (let k = 0; k < 24; k++) xfs.push(b.xf({ border: R.chance(0.6) ? borderXf(b, R, { p: R.pick([0.2, 0.5, 0.8]) }) : 0, fill: R.chance(0.35) ? R.pick(nfill) : 0, font: R.chance(0.3) ? R.pick(fonts) : 0 }));
  const dx = [
    b.dxf(`<font><b/><color rgb="FF${R.hex6()}"/></font><fill><patternFill><bgColor rgb="FF${R.hex6()}"/></patternFill></fill>`),
    b.dxf(`<border><left style="${R.pick(BORDER_STYLES)}"><color rgb="FF${R.hex6()}"/></left><bottom style="${R.pick(BORDER_STYLES)}"><color rgb="FF${R.hex6()}"/></bottom></border>`),
    b.dxf(`<fill><patternFill patternType="${R.pick(PATTERNS)}"><fgColor rgb="FF${R.hex6()}"/><bgColor rgb="FF${R.hex6()}"/></patternFill></fill>`),
    b.dxf('<font><i/><u/></font>'),
  ];
  const customN = R.below(3);
  for (let k = 0; k < customN; k++) {
    const els = [...ELEMENTS, ...PIVOT_ELEMENTS].filter(() => R.chance(0.45)).map((e) => [e, R.chance(0.9) ? R.pick(dx) : undefined, e.includes('Stripe') && R.chance(0.6) ? R.int(1, 4) : undefined]);
    b.tableStyle(`Cust${k}`, els, k % 2 === 1);
  }
  const styleNames = [...TABLE_STYLE_NAMES, ...PIVOT_STYLE_NAMES.filter((_, k) => k % 3 === 0)];
  for (let s = 0; s < nSheets; s++) {
    const sh = b.sheet(`S${s + 1}`);
    sh.grid = R.chance(0.7);
    const rows = R.int(14, 50), cols = R.int(8, 20);
    for (let r = 0; r < rows; r++) for (let c = 0; c < cols; c++) if (R.chance(0.6)) sh.cells.set(`${c},${r}`, R.chance(0.15) ? { t: R.pick(WORD), s: R.pick(xfs) } : { n: R.int(0, 99), s: R.chance(0.2) ? R.pick(xfs) : 0 });
    const nT = R.int(0, 4);
    for (let k = 0; k < nT; k++) {
      const w = R.int(2, 6), h = R.int(2, 9), c = R.below(cols - 1), r = R.below(rows - 1);
      const styleName = R.chance(0.1) ? `Cust${R.below(Math.max(1, customN))}` : R.pick(styleNames);
      const t = table(c, r, w, h, { style: R.chance(0.05) ? null : styleName, header: R.chance(0.8), totals: R.chance(0.4), rowStripes: R.chance(0.6), colStripes: R.chance(0.3), first: R.chance(0.4), last: R.chance(0.4), filter: R.chance(0.3), hidden: R.chance(0.3) ? [R.below(w)] : [], active: R.chance(0.3) ? [R.below(w)] : [], borderDxf: R.chance(0.2) ? R.pick(dx) : undefined });
      sh.tables.push(t);
    }
    const nP = R.int(0, 2);
    for (let k = 0; k < nP; k++) pivotSheet(sh, R, k, R.pick(R.chance(0.2) ? ['Cust1', ...PIVOT_STYLE_NAMES] : PIVOT_STYLE_NAMES), { c: R.int(2, 6), r: R.int(8, 20), w: R.int(3, 7), h: R.int(4, 10), pages: R.pick([0, 0, 1, 2]) });
    if (R.chance(0.5)) { const c = R.int(1, cols - 3), r = R.int(1, rows - 3); sh.cf.push(`<conditionalFormatting sqref="${rng(c, r, c + 4, r + 6)}"><cfRule type="cellIs" dxfId="${R.pick(dx)}" priority="1" operator="${R.pick(['greaterThan', 'lessThan', 'between'])}"><formula>${R.int(10, 60)}</formula>${R.chance(0.3) ? '<formula>80</formula>' : ''}</cfRule></conditionalFormatting>`); }
    if (R.chance(0.3)) { const c = R.int(1, cols - 3), r = R.int(1, rows - 3); sh.cf.push(`<conditionalFormatting sqref="${rng(c, r, c + 3, r + 5)}"><cfRule type="colorScale" priority="2"><colorScale><cfvo type="min"/><cfvo type="max"/><color rgb="FF${R.hex6()}"/><color rgb="FF${R.hex6()}"/></colorScale></cfRule></conditionalFormatting>`); }
    for (let k = 0; k < R.below(4); k++) { const c = R.below(cols - 2), r = R.below(rows - 2); sh.merges.push(rng(c, r, c + R.int(0, 2), r + R.int(0, 2))); }
    if (R.chance(0.3)) sh.cols.push(`<col min="${R.int(1, 5)}" max="${R.int(5, 8)}" width="${R.pick([4, 9, 15])}" customWidth="1"${R.chance(0.5) ? ` style="${R.pick(xfs)}"` : ''}/>`);
    if (R.chance(0.3)) sh.rowDefs.set(R.below(rows), `s="${R.pick(xfs)}" customFormat="1"`);
  }
  return b;
}

/** `{ bytes, name }` of workbook `i` of group `g`. */
export function buildPaintWorkbook(g, i) {
  let book;
  const P = PAINT_GROUPS;
  if (g === 'paint1') {
    const fn = [
      () => family(0, TABLE_STYLE_NAMES.slice(0, 21), 0, 'light'),
      () => family(1, TABLE_STYLE_NAMES.slice(21, 49), 1, 'medium'),
      () => family(2, TABLE_STYLE_NAMES.slice(49), 2, 'dark'),
      () => { const R = new Rng(P.paint1.seedBase + 3); const b = new Book(0); const sh = b.sheet('pl'); PIVOT_STYLE_NAMES.slice(0, 28).forEach((nm, k) => pivotSheet(sh, R, 0, nm, { c: 1 + (k % 3) * 8, r: 6 + Math.floor(k / 3) * 13, w: 5, h: 8 })); return b; },
      () => { const R = new Rng(P.paint1.seedBase + 4); const b = new Book(1); const sh = b.sheet('pm'); PIVOT_STYLE_NAMES.slice(28, 56).forEach((nm, k) => pivotSheet(sh, R, 0, nm, { c: 1 + (k % 3) * 8, r: 6 + Math.floor(k / 3) * 13, w: 5, h: 8 })); return b; },
      () => { const R = new Rng(P.paint1.seedBase + 5); const b = new Book(2); const sh = b.sheet('pd'); PIVOT_STYLE_NAMES.slice(56).forEach((nm, k) => pivotSheet(sh, R, 0, nm, { c: 1 + (k % 3) * 8, r: 6 + Math.floor(k / 3) * 13, w: 5, h: 8 })); return b; },
      () => options(6),
      () => custom(7),
      () => { const R = new Rng(P.paint1.seedBase + 8); const b = new Book(1); const sh = b.sheet('overlap'); const d = b.dxf('<border><left style="medium"><color rgb="FFFF0000"/></left><top style="medium"><color rgb="FFFF0000"/></top><right style="medium"><color rgb="FFFF0000"/></right><bottom style="medium"><color rgb="FFFF0000"/></bottom></border>'); [[1, 1, 6, 8, 'TableStyleMedium2'], [3, 4, 6, 8, 'TableStyleLight8'], [5, 7, 5, 6, 'TableStyleDark5'], [2, 2, 3, 3, 'TableStyleMedium9']].forEach(([c, r, w, h, st], k) => plainTable(b, sh, R, c, r, w, h, st, { totals: k % 2 === 0, first: true, borderDxf: k === 1 ? d : undefined })); pivotSheet(sh, R, 0, 'PivotStyleMedium4', { c: 4, r: 5, w: 6, h: 8, pages: 2 }); return b; },
      () => direct(9),
      () => filters(10),
      () => borders(11, 0),
      () => borders(12, 1),
      () => hostile(13),
    ];
    book = fn[i]();
  } else {
    book = random(P[g].seedBase + i);
  }
  return { bytes: book.build(), name: paintFile(g, i) };
}
