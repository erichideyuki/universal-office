// Seeded deterministic .xlsx generator (plan section 4.3 "Generators"; record core/docs/modules/uo-xlsx.md
// "Captures"). Pure Node, no dependencies: same seed + group -> same bytes (lib zip: fixed times, deflate level 6;
// no Date, no Math.random). The workbooks are hostile on purpose where the TS reader has rules (SH1 and the
// `CellExtra`/`FileOrder` side tables of uo-calc.md S2): non-canonical `<v>` texts, out-of-order and duplicate cells,
// `<c r>` on another row than its `<row r>`, cells and rows without `r`, shared strings next to inline strings.
//
// Features (`FEATURES`; a workbook lists the ones it contains): styles, numfmt (custom + builtin formats), cf, merge,
// frozen, names, tables, xsheet (cross-sheet formulas), shared (shared formulas), cse (legacy array), spill (dynamic
// arrays + metadata.xml), volatile, cycle (+ iterate), drawing (shape), chart (bar chart), rich (rich SST runs),
// noncanon (non-canonical <v>), order (rows/cells out of order + duplicates), rowmismatch (SH1), noref (cells/rows
// without r), inline (inline strings), errors, bools, dates (date1904 too), hidden (rows/cols), cachedv (formula
// cached values: absent / stale / typed), calcpr (calcPr variants that drive the open-recalc rule), unicode
// (astral, combining, lone-surrogate entities, non-ASCII sheet names), wide (columns past Z), widths (cols/ht).
//
// `buildWorkbook(group, i)` -> { bytes: Buffer, seed, features: string[], name }.
import { Rand } from '../../../uo-pptx/js/lib/gen/rand.mjs';
import { zipBytes } from '../../../uo-pptx/js/lib/zip.mjs';

export const FEATURES = ['styles', 'numfmt', 'cf', 'merge', 'frozen', 'names', 'tables', 'xsheet', 'shared', 'cse', 'spill',
  'volatile', 'cycle', 'drawing', 'chart', 'rich', 'noncanon', 'order', 'rowmismatch', 'noref', 'inline', 'errors', 'bools',
  'dates', 'hidden', 'cachedv', 'calcpr', 'unicode', 'wide', 'widths'];

/** Groups: CI (committed workbooks + captures) and local-only runs. */
export const GROUPS = {
  x1: { seedBase: 0x51000, count: 30, ci: true, rows: [4, 14], sheets: [1, 3], p: 0.35 },
  'x1-big': { seedBase: 0x52000, count: 400, ci: false, rows: [4, 120], sheets: [1, 4], p: 0.45 },
};

export const groupKey = (name, i) => `gen/${name}/${String(i).padStart(4, '0')}`;
export const workbookFile = (name, i) => `${name}-${String(i).padStart(4, '0')}.xlsx`;

const NS = 'http://schemas.openxmlformats.org/spreadsheetml/2006/main';
const NSR = 'http://schemas.openxmlformats.org/officeDocument/2006/relationships';
const REL = 'http://schemas.openxmlformats.org/officeDocument/2006/relationships';
const PKG_REL = 'http://schemas.openxmlformats.org/package/2006/relationships';
const XML = '<?xml version="1.0" encoding="UTF-8" standalone="yes"?>\n';
const esc = (s) => String(s).replace(/&/g, '&amp;').replace(/</g, '&lt;').replace(/>/g, '&gt;').replace(/"/g, '&quot;');
/** text node: escapes, and writes lone surrogates as numeric entities (the TS decodes them back) */
const escT = (s) => esc(s).replace(/[\uD800-\uDBFF](?![\uDC00-\uDFFF])|(?<![\uD800-\uDBFF])[\uDC00-\uDFFF]/g, (c) => `&#x${c.charCodeAt(0).toString(16).toUpperCase()};`);
export const colName = (c) => { let s = ''; for (c += 1; c > 0; c = Math.floor((c - 1) / 26)) s = String.fromCharCode(65 + ((c - 1) % 26)) + s; return s; };
const ref = (c, r) => `${colName(c)}${r + 1}`;
const absArea = (c1, r1, c2, r2) => `$${colName(c1)}$${r1 + 1}:$${colName(c2)}$${r2 + 1}`;
const qs = (name) => (/^[A-Za-z_][A-Za-z0-9_.]*$/.test(name) && !/^[A-Za-z]{1,3}\d+$/.test(name) ? name : `'${name.replace(/'/g, "''")}'`);

const SHEET_NAMES = ['Plan1', 'Dados 2', 'Ação', "O'Brien", 'Résumé', 'Sheet5', 'Total'];
const CUSTOM_FMTS = ['#,##0.00', '0.0%', 'dd/mm/yyyy', '[Red]-0.00;0.00', '"R$" #,##0.00', '@', '0.00E+00', '# ?/?', '[h]:mm:ss',
  'yyyy-mm-dd hh:mm', '_-* #,##0.00_-;-* #,##0.00_-;_-* "-"??_-;_-@_-', '0;-0;"zero";@', 'mmm/yy', '#,##0,"k"', '[$-409]d-mmm-yy'];
const BUILTIN_FMTS = [1, 2, 3, 4, 9, 10, 11, 14, 15, 18, 20, 22, 37, 40, 45, 46, 47, 49];
const FONTS = [['Calibri', 11], ['Aptos Narrow', 11], ['Arial', 10], ['Liberation Sans', 10], ['Cambria', 12], ['Consolas', 9]];
const TEXTS = ['alfa', 'Beta', 'gamma delta', 'São Paulo', 'straße', 'ÇÃO', 'x y  z', '  lead', 'trail  ', '123', '1,5', '1.234,56', 'TRUE', '#N/A', '=1+1', 'line\nbreak', 'tab\tin'];
const UNICODE_TEXTS = ['😀 smile', '𝒳 astral', 'é combining', 'İstanbul ı', 'ǅ title', 'ΣΑΣ ς', '漢字', 'lone \uD800 high', 'lone \uDC00 low', 'ﬃ lig'];
const NONCANON = ['1.50', '1E3', ' 7', '007', '-0', '1e-7', '0.1000', '+5', '.5', '1.0E+21', '0x10', '12abc', '', '  ', 'Infinity', '9007199254740993', '1.7976931348623159e308', '4.9e-324'];

/** One workbook of a group. */
export function buildWorkbook(group, i) {
  const g = GROUPS[group];
  if (!g) throw new Error(`unknown group ${group}`);
  const seed = g.seedBase + i;
  const R = new Rand(seed);
  const forced = FEATURES[i % FEATURES.length];
  const on = new Set(FEATURES.filter((f) => f === forced || R.chance(g.p)));
  // dependencies between features
  if (on.has('chart')) on.add('drawing');
  if (on.has('cycle') && R.chance(0.5)) on.add('calcpr');
  const ctx = new Book(R, on, g);
  const bytes = ctx.build();
  return { bytes, seed, features: [...ctx.used].sort(), name: workbookFile(group, i) };
}

class Book {
  constructor(R, on, g) {
    this.R = R; this.on = on; this.g = g; this.used = new Set();
    this.sst = []; this.sstIndex = new Map();
    this.numFmts = []; this.fonts = []; this.fills = []; this.borders = []; this.xfs = []; this.dxfs = [];
    this.parts = []; this.overrides = []; this.wbRels = [];
    this.sheets = []; this.names = []; this.tableSeq = 0; this.drawingSeq = 0; this.chartSeq = 0;
    this.spill = false;
  }
  use(f) { if (this.on.has(f)) { this.used.add(f); return true; } return false; }

  sstOf(text, rich) {
    const k = rich ? `\u0001${JSON.stringify(rich)}` : text;
    if (!this.sstIndex.has(k)) { this.sstIndex.set(k, this.sst.length); this.sst.push(rich ? { rich } : { text }); }
    return this.sstIndex.get(k);
  }

  // ---------------------------------------------------------------- styles
  buildStyles() {
    const R = this.R;
    const [fname, fsize] = this.use('styles') && R.chance(0.5) ? R.pick(FONTS) : FONTS[0];
    this.fonts.push(`<font><sz val="${fsize}"/><color theme="1"/><name val="${esc(fname)}"/><family val="2"/>${fname === 'Calibri' ? '<scheme val="minor"/>' : ''}</font>`);
    this.fills.push('<fill><patternFill patternType="none"/></fill>', '<fill><patternFill patternType="gray125"/></fill>');
    this.borders.push('<border><left/><right/><top/><bottom/><diagonal/></border>');
    this.xfs.push('<xf numFmtId="0" fontId="0" fillId="0" borderId="0" xfId="0"/>');
    this.styleIds = [0];
    this.dateXf = undefined;
    if (this.use('numfmt') || this.use('dates')) {
      const codes = R.pickN(CUSTOM_FMTS, R.int(2, 6));
      codes.forEach((code, k) => this.numFmts.push([164 + k, code]));
    }
    // a date xf always exists when dates are on (built-in 14 or a custom date code)
    const dateFmt = this.numFmts.find(([, c]) => /d|y/.test(c) && !/"/.test(c))?.[0] ?? 14;
    this.dateXf = this.addXf({ numFmtId: dateFmt });
    if (this.used.has('styles')) {
      for (let k = 0; k < R.int(2, 5); k++) {
        const [n, s] = R.pick(FONTS);
        this.fonts.push(`<font>${R.chance(0.5) ? '<b/>' : ''}${R.chance(0.3) ? '<i/>' : ''}${R.chance(0.2) ? '<u/>' : ''}${R.chance(0.1) ? '<strike/>' : ''}<sz val="${R.pick([8, 9, 10, 11, 12, 14, 18, 24, s])}"/><color rgb="FF${R.hex6()}"/><name val="${esc(n)}"/></font>`);
      }
      for (let k = 0; k < R.int(1, 3); k++) {
        const pt = R.pick(['solid', 'solid', 'darkGray', 'lightGrid', 'gray0625', 'darkTrellis']);
        this.fills.push(`<fill><patternFill patternType="${pt}"><fgColor rgb="FF${R.hex6()}"/><bgColor ${R.chance(0.5) ? 'indexed="64"' : `theme="${R.int(0, 9)}" tint="${R.pick(['0.39997558519241921', '-0.249977111117893', '0.59999389629810485'])}"`}/></patternFill></fill>`);
      }
      for (let k = 0; k < R.int(1, 3); k++) {
        const side = (n) => (R.chance(0.7) ? `<${n} style="${R.pick(['thin', 'medium', 'thick', 'double', 'dashed', 'dotted', 'hair', 'mediumDashDot', 'slantDashDot'])}"><color rgb="FF${R.hex6()}"/></${n}>` : `<${n}/>`);
        this.borders.push(`<border>${side('left')}${side('right')}${side('top')}${side('bottom')}<diagonal/></border>`);
      }
      for (let k = 0; k < R.int(3, 9); k++) {
        const align = R.chance(0.5) ? {
          horizontal: R.pick([undefined, 'left', 'center', 'right', 'fill', 'justify', 'centerContinuous', 'distributed']),
          vertical: R.pick([undefined, 'top', 'center', 'bottom']), wrapText: R.chance(0.3) ? 1 : undefined,
          indent: R.chance(0.2) ? R.int(1, 3) : undefined, textRotation: R.chance(0.1) ? R.pick([45, 90, 255]) : undefined,
          shrinkToFit: R.chance(0.1) ? 1 : undefined,
        } : undefined;
        this.addXf({
          numFmtId: this.numFmts.length && R.chance(0.6) ? R.pick(this.numFmts)[0] : R.chance(0.5) ? R.pick(BUILTIN_FMTS) : 0,
          fontId: R.below(this.fonts.length), fillId: R.below(this.fills.length), borderId: R.below(this.borders.length), align,
        });
      }
    } else if (this.numFmts.length) {
      for (const [id] of this.numFmts) this.addXf({ numFmtId: id });
      for (let k = 0; k < 2; k++) this.addXf({ numFmtId: R.pick(BUILTIN_FMTS) });
    }
    if (this.on.has('cf')) {
      for (let k = 0; k < 3; k++) this.dxfs.push(`<dxf><font>${k === 0 ? '<b/>' : ''}<color rgb="FF${this.R.hex6()}"/></font><fill><patternFill><bgColor rgb="FF${this.R.hex6()}"/></patternFill></fill></dxf>`);
    }
  }
  addXf({ numFmtId = 0, fontId = 0, fillId = 0, borderId = 0, align }) {
    const attrs = [`numFmtId="${numFmtId}"`, `fontId="${fontId}"`, `fillId="${fillId}"`, `borderId="${borderId}"`, 'xfId="0"'];
    if (numFmtId) attrs.push('applyNumberFormat="1"');
    if (fontId) attrs.push('applyFont="1"');
    if (fillId) attrs.push('applyFill="1"');
    if (borderId) attrs.push('applyBorder="1"');
    let inner = '';
    if (align) {
      const a = Object.entries(align).filter(([, v]) => v !== undefined).map(([k, v]) => `${k}="${v}"`).join(' ');
      if (a) { attrs.push('applyAlignment="1"'); inner = `<alignment ${a}/>`; }
    }
    this.xfs.push(inner ? `<xf ${attrs.join(' ')}>${inner}</xf>` : `<xf ${attrs.join(' ')}/>`);
    this.styleIds.push(this.xfs.length - 1);
    return this.xfs.length - 1;
  }
  stylesXml() {
    const nf = this.numFmts.length ? `<numFmts count="${this.numFmts.length}">${this.numFmts.map(([id, c]) => `<numFmt numFmtId="${id}" formatCode="${esc(c)}"/>`).join('')}</numFmts>` : '';
    return `${XML}<styleSheet xmlns="${NS}">${nf}<fonts count="${this.fonts.length}">${this.fonts.join('')}</fonts><fills count="${this.fills.length}">${this.fills.join('')}</fills><borders count="${this.borders.length}">${this.borders.join('')}</borders><cellStyleXfs count="1"><xf numFmtId="0" fontId="0" fillId="0" borderId="0"/></cellStyleXfs><cellXfs count="${this.xfs.length}">${this.xfs.join('')}</cellXfs><cellStyles count="1"><cellStyle name="Normal" xfId="0" builtinId="0"/></cellStyles><dxfs count="${this.dxfs.length}">${this.dxfs.join('')}</dxfs><tableStyles count="0" defaultTableStyle="TableStyleMedium2" defaultPivotStyle="PivotStyleLight16"/></styleSheet>`;
  }

  // ---------------------------------------------------------------- sheets
  build() {
    const R = this.R;
    this.buildStyles();
    const nSheets = R.int(this.g.sheets[0], this.g.sheets[1]) + (this.on.has('xsheet') ? 1 : 0);
    const names = this.on.has('unicode') ? R.pickN(SHEET_NAMES, nSheets) : SHEET_NAMES.filter((n) => /^[\x20-\x7e]*$/.test(n) && !n.includes("'")).slice(0, nSheets);
    while (names.length < nSheets) names.push(`Sheet${names.length + 1}`);
    if (this.used.has('unicode') || names.some((n) => !/^[\x20-\x7e]*$/.test(n))) this.used.add('unicode');
    this.sheetNames = names;
    this.date1904 = this.on.has('dates') && R.chance(0.2);
    for (let s = 0; s < nSheets; s++) this.sheets.push(this.buildSheet(s));
    return this.pack();
  }

  buildSheet(s) {
    const R = this.R;
    const nRows = R.int(this.g.rows[0], this.g.rows[1]);
    const wide = this.use('wide') && R.chance(0.6);
    const c0 = wide ? R.int(24, 40) : 0; // data starts past Z sometimes
    const dataCols = 4;
    const rows = new Map(); // r -> Map(c -> cell)
    const put = (r, c, cell) => { if (!rows.has(r)) rows.set(r, new Map()); rows.get(r).set(c, cell); };
    const style = () => (this.styleIds.length > 1 && R.chance(0.4) ? R.pick(this.styleIds) : undefined);
    const headers = ['Id', 'Valor', 'Nome', this.on.has('dates') ? 'Data' : 'Flag'];
    headers.forEach((h, k) => put(0, c0 + k, { t: 's', text: h, s: style() }));
    for (let r = 1; r <= nRows; r++) {
      put(r, c0, { n: r * (R.chance(0.2) ? -1 : 1) * R.int(1, 50), s: style() });
      const dec = R.pick([R.int(-1000, 1000) / 8, R.int(1, 99999) / 100, 0.1 * R.int(1, 30), 1e15 + R.int(0, 9), R.int(1, 9) * 1e-9]);
      put(r, c0 + 1, { n: dec, s: style() });
      const pool = this.on.has('unicode') && R.chance(0.4) ? UNICODE_TEXTS : TEXTS;
      if (pool === UNICODE_TEXTS) this.used.add('unicode');
      const text = R.pick(pool);
      if (this.on.has('inline') && R.chance(0.4)) { this.used.add('inline'); put(r, c0 + 2, { t: 'inlineStr', text, s: style() }); }
      else if (this.on.has('rich') && R.chance(0.3)) {
        this.used.add('rich');
        put(r, c0 + 2, { t: 's', text, rich: [[text.slice(0, 3), { b: R.chance(0.5), color: R.hex6(), sz: R.pick([9, 11, 14]) }], [text.slice(3) || '.', { i: true, font: R.pick(FONTS)[0] }]] });
      } else put(r, c0 + 2, { t: 's', text, s: style() });
      if (this.on.has('dates')) { this.used.add('dates'); put(r, c0 + 3, { n: R.pick([45658, 45351, 1, 60, 61, 2958465, 0.5, 45658.75, 36526.999988426]), s: this.dateXf }); }
      else if (this.use('bools')) put(r, c0 + 3, { t: 'b', v: R.chance(0.5) ? '1' : '0' });
      else put(r, c0 + 3, { n: R.int(0, 9) });
    }
    if (this.use('errors')) for (let k = 0; k < 2; k++) put(R.int(1, nRows), c0 + dataCols + 6, { t: 'e', v: R.pick(['#DIV/0!', '#N/A', '#NAME?', '#NULL!', '#NUM!', '#REF!', '#VALUE!', '#SPILL!', '#CALC!', '#GETTING_DATA']) });
    if (this.use('noncanon')) for (let k = 0; k < 4; k++) put(R.int(1, nRows), c0 + dataCols + 7, { raw: R.pick(NONCANON), t: R.pick([undefined, undefined, 'n', 'b']) });

    // ---------------- formulas
    const fc0 = c0 + dataCols;
    const A = (k, r) => ref(c0 + k, r);
    const last = nRows;
    const col = (k) => `${colName(c0 + k)}2:${colName(c0 + k)}${last + 1}`;
    const other = this.sheetNames.length > 1 ? this.sheetNames[(s + 1) % this.sheetNames.length] : null;
    const templates = [
      (r) => `${A(0, r)}+${A(1, r)}`, (r) => `SUM(${col(0)})`, (r) => `AVERAGE(${col(1)})`, (r) => `IF(${A(0, r)}>5,"big","small")`,
      (r) => `ROUND(${A(1, r)}*1.5,2)`, (r) => `${A(2, r)}&"-"&${A(0, r)}`, (r) => `TEXT(${A(1, r)},"0.00")`, (r) => `LEN(${A(2, r)})`,
      (r) => `UPPER(${A(2, r)})`, (r) => `VLOOKUP(${A(0, r)},${colName(c0)}2:${colName(c0 + 2)}${last + 1},3,FALSE)`,
      (r) => `MATCH(${A(0, r)},${col(0)},0)`, (r) => `INDEX(${col(1)},${Math.max(1, r - 1)})`, (r) => `SUMIF(${col(0)},">3",${col(1)})`,
      (r) => `COUNTIFS(${col(0)},">0",${col(1)},"<500")`, (r) => `IFERROR(1/(${A(0, r)}-${A(0, r)}),"div")`, (r) => `${A(1, r)}/0`,
      (r) => `DATE(2024,2,29)+${A(0, r)}`, (r) => `_xlfn.XLOOKUP(${A(0, r)},${col(0)},${col(2)},"none")`, (r) => `_xlfn.LET(x,${A(1, r)},x*x)`,
      (r) => `MAX(${col(0)})-MIN(${col(0)})`, (r) => `PRODUCT(${A(0, r)},2)`, (r) => `SUMPRODUCT(${col(0)},${col(1)})`, (r) => `MOD(${A(0, r)},3)`,
      (r) => `CONCATENATE(${A(2, r)},"!")`, (r) => `TRIM(${A(2, r)})`, (r) => `VALUE("1,5")`, (r) => `${A(0, r)}^2`, (r) => `-${A(1, r)}%`,
    ];
    if (other && this.use('xsheet')) templates.push((r) => `${qs(other)}!${ref(0, r)}*2`, (r) => `SUM(${qs(other)}!A1:A5)`, (r) => `COUNTA(${qs(other)}!A:A)`);
    const nf = R.int(2, 6);
    const formulaCols = [];
    for (let k = 0; k < nf; k++) formulaCols.push(fc0 + k);
    const cached = () => {
      if (!this.on.has('cachedv')) return {};
      this.used.add('cachedv');
      return R.pick([{}, { v: '0' }, { v: '42' }, { t: 'str', v: 'stale' }, { t: 'e', v: '#N/A' }, { t: 'b', v: '1' }, { ca: true }]);
    };
    for (let r = 1; r <= nRows; r++) {
      for (const c of formulaCols) {
        if (R.chance(0.15)) continue;
        put(r, c, { f: R.pick(templates)(r), ...cached(), s: style() });
      }
    }
    // previous-row chains (evaluation order across columns)
    for (let r = 2; r <= nRows; r++) if (R.chance(0.5)) put(r, fc0 + nf, { f: `${ref(fc0 + nf, r - 1)}+${A(0, r)}`, ...cached() });
    put(1, fc0 + nf, { f: `${A(0, 1)}`, ...cached() });
    // ranges wider than the formula columns (the `fc.keys()` and `rangeMasters` order sites of uo-calc.md S7)
    const wideCol = fc0 + nf + 9;
    put(1, wideCol, { f: `SUM(${colName(c0)}1:${colName(fc0 + nf + 4)}${last + 1})` });
    put(2, wideCol, { f: `COUNT(${colName(fc0)}1:${colName(fc0 + nf + 4)}${last + 6})+${ref(wideCol, 1)}` });
    if (this.use('volatile')) {
      put(nRows + 2, fc0, { f: 'RAND()' }); put(nRows + 2, fc0 + 1, { f: 'RANDBETWEEN(1,100)' }); put(nRows + 2, fc0 + 2, { f: 'NOW()' });
      put(nRows + 3, fc0, { f: 'TODAY()' }); put(nRows + 3, fc0 + 1, { f: `OFFSET(${A(0, 1)},1,1)` }); put(nRows + 3, fc0 + 2, { f: `INDIRECT("${colName(c0)}"&2)` });
      put(nRows + 4, fc0, { f: `SUM(${ref(fc0, nRows + 2)}:${ref(fc0 + 1, nRows + 2)})` });
      put(nRows + 4, fc0 + 1, { f: '_xlfn.RANDARRAY(2,2)', dyn: '2x2' });
    }
    if (this.use('cycle')) {
      const r = nRows + 6;
      put(r, fc0, { f: `${ref(fc0 + 1, r)}+1`, v: '0' }); put(r, fc0 + 1, { f: `${ref(fc0, r)}*0.5`, v: '0' });
      put(r, fc0 + 2, { f: `${ref(fc0 + 2, r)}+1` }); // self cycle
      put(r + 1, fc0, { f: `SUM(${ref(fc0, r)}:${ref(fc0 + 2, r)})` });
    }
    if (this.use('shared') && nRows >= 3) {
      const c = fc0 + nf + 1;
      put(1, c, { f: `${A(0, 1)}*2`, shared: { si: s, ref: `${ref(c, 1)}:${ref(c, nRows)}` } });
      for (let r = 2; r <= nRows; r++) put(r, c, { sharedChild: s, ...cached() });
    }
    if (this.use('cse') && nRows >= 3) {
      const c = fc0 + nf + 2;
      put(1, c, { f: `${A(0, 1)}:${A(0, 3)}*10`, array: `${ref(c, 1)}:${ref(c, 3)}` });
      put(2, c, { v: '0' }); put(3, c, { v: '0' });
    }
    if (this.use('spill')) {
      this.spill = true;
      const c = fc0 + nf + 3;
      const forms = [`_xlfn.SEQUENCE(3,2)`, `_xlfn._xlws.SORT(${col(0)})`, `_xlfn.UNIQUE(${col(3)})`, `_xlfn._xlws.FILTER(${col(0)},${col(0)}>3,"none")`, `_xlfn.TRANSPOSE(${colName(c0)}1:${colName(c0 + 3)}1)`];
      put(1, c, { f: R.pick(forms), dyn: '1' });
      put(1, c + 3, { f: `_xlfn.SEQUENCE(2)`, dyn: '1' });
      put(2, c + 3, { n: 99 }); // a blocker: #SPILL!
      put(nRows + 1, c, { f: `SUM(${ref(c, 1)}#)`, dyn: '1' });
    }

    // ---------------- sheet-level parts
    const merges = [];
    if (this.use('merge')) {
      merges.push(`${ref(fc0 + nf + 6, 0)}:${ref(fc0 + nf + 7, 1)}`);
      put(0, fc0 + nf + 6, { t: 's', text: 'merged', s: style() });
      if (R.chance(0.5)) merges.push(`${ref(c0, nRows + 8)}:${ref(c0 + 3, nRows + 8)}`);
    }
    const sheet = { name: this.sheetNames[s], rows, merges, nRows, c0, fc0, nf, idx: s };
    if (this.use('frozen')) sheet.pane = R.pick([{ x: 0, y: 1 }, { x: 1, y: 1 }, { x: 2, y: 0 }, { x: 1, y: 3 }]);
    if (this.use('hidden')) { sheet.hiddenRows = [R.int(1, nRows)]; sheet.hiddenCols = [c0 + R.int(0, 3)]; }
    if (this.use('widths')) {
      sheet.cols = [[c0 + 1, c0 + 1, R.pick([3.5, 8.43, 12.7109375, 25, 0.5])], [c0 + 2, c0 + 3, R.pick([15, 30.140625])]];
      sheet.heights = [[R.int(1, nRows), R.pick([5, 30, 45.75, 99.95])]];
    }
    if (this.use('cf')) sheet.cf = this.cfXml(sheet);
    if (this.use('tables') && s === 0) sheet.table = { name: `Tabela${++this.tableSeq}`, ref: `${ref(c0, 0)}:${ref(c0 + 3, nRows)}`, headers };
    if (this.use('drawing')) sheet.drawing = { chart: this.use('chart') };
    if (s === 0 && this.use('names')) {
      this.names.push(['Taxas', `${qs(sheet.name)}!${absArea(c0, 1, c0, nRows)}`], ['Dobro', `${qs(sheet.name)}!$${colName(c0 + 1)}$2*2`]);
      this.names.push(['Local1', `${qs(sheet.name)}!$${colName(c0)}$2`, 0]);
      this.names.push(['_xlnm.Print_Area', `${qs(sheet.name)}!${absArea(c0, 0, c0 + 3, nRows)}`, 0]);
      put(nRows + 5, fc0, { f: 'SUM(Taxas)' }); put(nRows + 5, fc0 + 1, { f: 'Dobro+Local1' }); put(nRows + 5, fc0 + 2, { f: 'Undefined_Name*2' });
    }
    if (sheet.table) put(nRows + 5, fc0 + 3, { f: `SUM(${sheet.table.name}[Valor])+COUNTA(${sheet.table.name}[[#Headers],[Nome]])` });
    return sheet;
  }

  cfXml(sheet) {
    const R = this.R;
    const a = (k) => `${colName(sheet.c0 + k)}2:${colName(sheet.c0 + k)}${sheet.nRows + 1}`;
    const rules = [
      () => `<conditionalFormatting sqref="${a(0)}"><cfRule type="cellIs" dxfId="0" priority="1" operator="${R.pick(['greaterThan', 'between', 'lessThan', 'equal'])}"><formula>5</formula><formula>20</formula></cfRule></conditionalFormatting>`,
      () => { const mid = R.chance(0.5); return `<conditionalFormatting sqref="${a(1)}"><cfRule type="colorScale" priority="2"><colorScale><cfvo type="min"/>${mid ? '<cfvo type="percentile" val="50"/>' : ''}<cfvo type="max"/><color rgb="FFF8696B"/>${mid ? '<color rgb="FFFFEB84"/>' : ''}<color rgb="FF63BE7B"/></colorScale></cfRule></conditionalFormatting>`; },
      () => `<conditionalFormatting sqref="${a(0)}"><cfRule type="dataBar" priority="3"><dataBar><cfvo type="min"/><cfvo type="max"/><color rgb="FF638EC6"/></dataBar></cfRule></conditionalFormatting>`,
      () => `<conditionalFormatting sqref="${a(1)}"><cfRule type="iconSet" priority="4"><iconSet iconSet="${R.pick(['3Arrows', '3TrafficLights1', '4Rating', '5Quarters'])}"><cfvo type="percent" val="0"/><cfvo type="percent" val="33"/><cfvo type="percent" val="67"/></iconSet></cfRule></conditionalFormatting>`,
      () => `<conditionalFormatting sqref="${a(2)}"><cfRule type="expression" dxfId="1" priority="5"><formula>MOD(ROW(),2)=0</formula></cfRule></conditionalFormatting>`,
      () => `<conditionalFormatting sqref="${a(0)}"><cfRule type="top10" dxfId="2" priority="6" rank="3"/></conditionalFormatting>`,
      () => `<conditionalFormatting sqref="${a(2)}"><cfRule type="duplicateValues" dxfId="0" priority="7"/></conditionalFormatting>`,
      () => `<conditionalFormatting sqref="${a(2)}"><cfRule type="containsText" dxfId="1" priority="8" operator="containsText" text="a"><formula>NOT(ISERROR(SEARCH("a",${colName(sheet.c0 + 2)}2)))</formula></cfRule></conditionalFormatting>`,
    ];
    return R.pickN(rules, R.int(1, 4)).map((f) => f()).join('');
  }

  cellXml(r, c, cell, rowR) {
    const R = this.R;
    const rAttr = this.on.has('noref') && R.chance(0.1) ? (this.used.add('noref'), '') : ` r="${ref(c, rowR ?? r)}"`;
    const sAttr = cell.s ? ` s="${cell.s}"` : '';
    if (cell.f !== undefined || cell.sharedChild !== undefined) {
      let f;
      if (cell.sharedChild !== undefined) f = `<f t="shared" si="${cell.sharedChild}"/>`;
      else if (cell.shared) f = `<f t="shared" ref="${cell.shared.ref}" si="${cell.shared.si}">${escT(cell.f)}</f>`;
      else if (cell.array) f = `<f t="array" ref="${cell.array}">${escT(cell.f)}</f>`;
      else if (cell.dyn) f = `<f t="array" ref="${ref(c, r)}"${cell.ca ? ' ca="1"' : ''}>${escT(cell.f)}</f>`;
      else f = `<f${cell.ca ? ' ca="1"' : ''}>${escT(cell.f)}</f>`;
      const t = cell.t ? ` t="${cell.t}"` : '';
      const cm = cell.dyn ? ' cm="1"' : '';
      const v = cell.v !== undefined ? `<v>${escT(cell.v)}</v>` : '';
      return `<c${rAttr}${sAttr}${t}${cm}>${f}${v}</c>`;
    }
    if (cell.raw !== undefined) return `<c${rAttr}${sAttr}${cell.t ? ` t="${cell.t}"` : ''}><v>${escT(cell.raw)}</v></c>`;
    if (cell.t === 's') return `<c${rAttr}${sAttr} t="s"><v>${this.sstOf(cell.text, cell.rich)}</v></c>`;
    if (cell.t === 'inlineStr') return `<c${rAttr}${sAttr} t="inlineStr"><is><t${/^\s|\s$/.test(cell.text) ? ' xml:space="preserve"' : ''}>${escT(cell.text)}</t></is></c>`;
    if (cell.t === 'b' || cell.t === 'e') return `<c${rAttr}${sAttr} t="${cell.t}"><v>${escT(cell.v)}</v></c>`;
    if (cell.n !== undefined) return `<c${rAttr}${sAttr}><v>${String(cell.n).replace('e+', 'E+').replace(/e-/, 'E-')}</v></c>`;
    if (cell.v !== undefined) return `<c${rAttr}${sAttr}><v>${escT(cell.v)}</v></c>`;
    return `<c${rAttr}${sAttr}/>`;
  }

  sheetXml(sheet) {
    const R = this.R;
    const order = this.on.has('order') && R.chance(0.7);
    let rowKeys = [...sheet.rows.keys()].sort((x, y) => x - y);
    if (order) { this.used.add('order'); const k = R.below(rowKeys.length - 1); [rowKeys[k], rowKeys[k + 1]] = [rowKeys[k + 1], rowKeys[k]]; }
    const heights = new Map(sheet.heights ?? []);
    const hidden = new Set(sheet.hiddenRows ?? []);
    const rowsXml = [];
    for (const r of rowKeys) {
      let cells = [...sheet.rows.get(r)].sort((x, y) => x[0] - y[0]);
      const parts = [];
      if (order && cells.length > 2 && R.chance(0.3)) { const k = R.below(cells.length - 1); [cells[k], cells[k + 1]] = [cells[k + 1], cells[k]]; }
      for (const [c, cell] of cells) parts.push(this.cellXml(r, c, cell));
      if (order && R.chance(0.15) && cells.length) { const [c, cell] = R.pick(cells); parts.push(this.cellXml(r, c, cell.f === undefined && cell.sharedChild === undefined ? { n: 777 } : cell)); }
      if (this.on.has('rowmismatch') && R.chance(0.12) && cells.length) {
        this.used.add('rowmismatch');
        const c = cells[cells.length - 1][0] + 1;
        parts.push(this.cellXml(r, c, { n: 31337 }, r + R.int(1, 3)));
      }
      const attrs = [];
      if (!(this.on.has('noref') && R.chance(0.05))) attrs.push(`r="${r + 1}"`); else this.used.add('noref');
      if (heights.has(r)) attrs.push(`ht="${heights.get(r)}"`, 'customHeight="1"');
      if (hidden.has(r)) attrs.push('hidden="1"');
      if (this.used.has('styles') && R.chance(0.05)) attrs.push(`s="${R.pick(this.styleIds)}"`, 'customFormat="1"');
      rowsXml.push(`<row ${attrs.join(' ')}>${parts.join('')}</row>`);
    }
    const pane = sheet.pane ? `<pane${sheet.pane.x ? ` xSplit="${sheet.pane.x}"` : ''}${sheet.pane.y ? ` ySplit="${sheet.pane.y}"` : ''} topLeftCell="${ref(sheet.pane.x, sheet.pane.y)}" activePane="${sheet.pane.x && sheet.pane.y ? 'bottomRight' : sheet.pane.y ? 'bottomLeft' : 'topRight'}" state="frozen"/>` : '';
    const view = `<sheetViews><sheetView workbookViewId="0"${sheet.idx === 0 ? ' tabSelected="1"' : ''}>${pane}</sheetView></sheetViews>`;
    const cols = [];
    for (const [a, b, w] of sheet.cols ?? []) cols.push(`<col min="${a + 1}" max="${b + 1}" width="${w}" customWidth="1"/>`);
    for (const c of sheet.hiddenCols ?? []) cols.push(`<col min="${c + 1}" max="${c + 1}" width="9.140625" hidden="1"/>`);
    cols.sort((x, y) => Number(/min="(\d+)"/.exec(x)[1]) - Number(/min="(\d+)"/.exec(y)[1]));
    const merges = sheet.merges.length ? `<mergeCells count="${sheet.merges.length}">${sheet.merges.map((m) => `<mergeCell ref="${m}"/>`).join('')}</mergeCells>` : '';
    const rels = [];
    let drawing = '', tableParts = '';
    if (sheet.drawing) { rels.push([`rId${rels.length + 1}`, `${REL}/drawing`, `../drawings/drawing${++this.drawingSeq}.xml`]); drawing = `<drawing r:id="rId${rels.length}"/>`; this.addDrawing(sheet, this.drawingSeq); }
    if (sheet.table) { rels.push([`rId${rels.length + 1}`, `${REL}/table`, `../tables/table${this.tableSeq}.xml`]); tableParts = `<tableParts count="1"><tablePart r:id="rId${rels.length}"/></tableParts>`; this.addTable(sheet, this.tableSeq); }
    const xml = `${XML}<worksheet xmlns="${NS}" xmlns:r="${NSR}">${view}<sheetFormatPr defaultRowHeight="15"/>${cols.length ? `<cols>${cols.join('')}</cols>` : ''}<sheetData>${rowsXml.join('')}</sheetData>${merges}${sheet.cf ?? ''}<pageMargins left="0.7" right="0.7" top="0.75" bottom="0.75" header="0.3" footer="0.3"/>${drawing}${tableParts}</worksheet>`;
    return { xml, rels };
  }

  addTable(sheet, n) {
    const cols = sheet.table.headers.map((h, k) => `<tableColumn id="${k + 1}" name="${esc(h)}"/>`).join('');
    this.parts.push([`xl/tables/table${n}.xml`, `${XML}<table xmlns="${NS}" id="${n}" name="${sheet.table.name}" displayName="${sheet.table.name}" ref="${sheet.table.ref}" totalsRowShown="0"><autoFilter ref="${sheet.table.ref}"/><tableColumns count="${sheet.table.headers.length}">${cols}</tableColumns><tableStyleInfo name="TableStyleMedium2" showFirstColumn="0" showLastColumn="0" showRowStripes="1" showColumnStripes="0"/></table>`]);
    this.overrides.push([`/xl/tables/table${n}.xml`, 'application/vnd.openxmlformats-officedocument.spreadsheetml.table+xml']);
  }

  addDrawing(sheet, n) {
    const R = this.R;
    const anchor = (c1, r1, c2, r2, body) => `<xdr:twoCellAnchor><xdr:from><xdr:col>${c1}</xdr:col><xdr:colOff>0</xdr:colOff><xdr:row>${r1}</xdr:row><xdr:rowOff>0</xdr:rowOff></xdr:from><xdr:to><xdr:col>${c2}</xdr:col><xdr:colOff>0</xdr:colOff><xdr:row>${r2}</xdr:row><xdr:rowOff>0</xdr:rowOff></xdr:to>${body}<xdr:clientData/></xdr:twoCellAnchor>`;
    const c = sheet.fc0 + sheet.nf + 2;
    const shape = `<xdr:sp macro="" textlink=""><xdr:nvSpPr><xdr:cNvPr id="2" name="Shape 1"/><xdr:cNvSpPr/></xdr:nvSpPr><xdr:spPr><a:xfrm><a:off x="0" y="0"/><a:ext cx="1219200" cy="571500"/></a:xfrm><a:prstGeom prst="${R.pick(['rect', 'roundRect', 'ellipse', 'rightArrow'])}"><a:avLst/></a:prstGeom><a:solidFill><a:srgbClr val="${R.hex6()}"/></a:solidFill><a:ln w="12700"><a:solidFill><a:srgbClr val="000000"/></a:solidFill></a:ln></xdr:spPr><xdr:txBody><a:bodyPr anchor="ctr"/><a:lstStyle/><a:p><a:pPr algn="ctr"/><a:r><a:rPr lang="pt-BR" sz="1100"/><a:t>Forma</a:t></a:r></a:p></xdr:txBody></xdr:sp>`;
    let body = anchor(c, 1, c + 2, 4, shape);
    const rels = [];
    if (sheet.drawing.chart) {
      const k = ++this.chartSeq;
      rels.push(`<Relationship Id="rId1" Type="${REL}/chart" Target="../charts/chart${k}.xml"/>`);
      body += anchor(c, 5, c + 5, 15, `<xdr:graphicFrame macro=""><xdr:nvGraphicFramePr><xdr:cNvPr id="3" name="Chart 1"/><xdr:cNvGraphicFramePr/></xdr:nvGraphicFramePr><xdr:xfrm><a:off x="0" y="0"/><a:ext cx="0" cy="0"/></xdr:xfrm><a:graphic><a:graphicData uri="http://schemas.openxmlformats.org/drawingml/2006/chart"><c:chart xmlns:c="http://schemas.openxmlformats.org/drawingml/2006/chart" r:id="rId1"/></a:graphicData></a:graphic></xdr:graphicFrame>`);
      const nPts = Math.min(sheet.nRows, 8);
      const pts = Array.from({ length: nPts }, (_, i) => `<c:pt idx="${i}"><c:v>${(i + 1) * 3}</c:v></c:pt>`).join('');
      const dir = R.pick(['col', 'bar']);
      this.parts.push([`xl/charts/chart${k}.xml`, `${XML}<c:chartSpace xmlns:c="http://schemas.openxmlformats.org/drawingml/2006/chart" xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main" xmlns:r="${NSR}"><c:chart><c:autoTitleDeleted val="1"/><c:plotArea><c:layout/><c:barChart><c:barDir val="${dir}"/><c:grouping val="clustered"/><c:varyColors val="0"/><c:ser><c:idx val="0"/><c:order val="0"/><c:val><c:numRef><c:f>${esc(qs(sheet.name))}!${absArea(sheet.c0, 1, sheet.c0, nPts)}</c:f><c:numCache><c:formatCode>General</c:formatCode><c:ptCount val="${nPts}"/>${pts}</c:numCache></c:numRef></c:val></c:ser><c:axId val="1"/><c:axId val="2"/></c:barChart><c:catAx><c:axId val="1"/><c:scaling><c:orientation val="minMax"/></c:scaling><c:delete val="0"/><c:axPos val="${dir === 'col' ? 'b' : 'l'}"/><c:crossAx val="2"/></c:catAx><c:valAx><c:axId val="2"/><c:scaling><c:orientation val="minMax"/></c:scaling><c:delete val="0"/><c:axPos val="${dir === 'col' ? 'l' : 'b'}"/><c:majorGridlines/><c:crossAx val="1"/></c:valAx></c:plotArea><c:plotVisOnly val="1"/></c:chart></c:chartSpace>`]);
      this.overrides.push([`/xl/charts/chart${k}.xml`, 'application/vnd.openxmlformats-officedocument.drawingml.chart+xml']);
    }
    this.parts.push([`xl/drawings/drawing${n}.xml`, `${XML}<xdr:wsDr xmlns:xdr="http://schemas.openxmlformats.org/drawingml/2006/spreadsheetDrawing" xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main" xmlns:r="${NSR}">${body}</xdr:wsDr>`]);
    if (rels.length) this.parts.push([`xl/drawings/_rels/drawing${n}.xml.rels`, `${XML}<Relationships xmlns="${PKG_REL}">${rels.join('')}</Relationships>`]);
    this.overrides.push([`/xl/drawings/drawing${n}.xml`, 'application/vnd.openxmlformats-officedocument.drawing+xml']);
  }

  sstXml() {
    const si = this.sst.map((e) => {
      if (e.rich) {
        return `<si>${e.rich.map(([t, p]) => `<r><rPr>${p.b ? '<b/>' : ''}${p.i ? '<i/>' : ''}<sz val="${p.sz ?? 11}"/>${p.color ? `<color rgb="FF${p.color}"/>` : '<color theme="1"/>'}<rFont val="${esc(p.font ?? 'Calibri')}"/></rPr><t xml:space="preserve">${escT(t)}</t></r>`).join('')}</si>`;
      }
      return `<si><t${/^\s|\s$|\n/.test(e.text) ? ' xml:space="preserve"' : ''}>${escT(e.text)}</t></si>`;
    });
    return `${XML}<sst xmlns="${NS}" count="${si.length}" uniqueCount="${si.length}">${si.join('')}</sst>`;
  }

  workbookXml() {
    const R = this.R;
    const sheets = this.sheets.map((s, i) => `<sheet name="${esc(s.name)}" sheetId="${i + 1}" r:id="rId${i + 1}"/>`).join('');
    const names = this.names.length ? `<definedNames>${this.names.map(([n, f, local]) => `<definedName name="${esc(n)}"${local !== undefined ? ` localSheetId="${local}"` : ''}>${esc(f)}</definedName>`).join('')}</definedNames>` : '';
    let calc = '<calcPr calcId="191029"/>';
    if (this.use('calcpr')) calc = R.pick(['<calcPr calcId="191029"/>', '<calcPr calcId="191029" fullCalcOnLoad="1"/>', '<calcPr calcId="122211"/>', '<calcPr calcId="191029" calcMode="manual"/>', '<calcPr calcId="191029" iterate="1" iterateCount="50" iterateDelta="0.0001"/>', '', '<calcPr calcId="191029" forceFullCalc="1"/>']);
    else if (this.used.has('cycle') && R.chance(0.5)) calc = '<calcPr calcId="191029" iterate="1"/>';
    const fileVersion = R.chance(0.85) ? '<fileVersion appName="xl" lastEdited="7" lowestEdited="7" rupBuild="27425"/>' : '';
    return `${XML}<workbook xmlns="${NS}" xmlns:r="${NSR}">${fileVersion}<workbookPr${this.date1904 ? ' date1904="1"' : ''}/><bookViews><workbookView activeTab="0"/></bookViews><sheets>${sheets}</sheets>${names}${calc}</workbook>`;
  }

  pack() {
    const sheetEntries = [];
    this.sheets.forEach((s, i) => {
      const { xml, rels } = this.sheetXml(s);
      sheetEntries.push([`xl/worksheets/sheet${i + 1}.xml`, xml]);
      if (rels.length) sheetEntries.push([`xl/worksheets/_rels/sheet${i + 1}.xml.rels`, `${XML}<Relationships xmlns="${PKG_REL}">${rels.map(([id, t, tg]) => `<Relationship Id="${id}" Type="${t}" Target="${tg}"/>`).join('')}</Relationships>`]);
    });
    const n = this.sheets.length;
    const wbRels = this.sheets.map((_, i) => `<Relationship Id="rId${i + 1}" Type="${REL}/worksheet" Target="worksheets/sheet${i + 1}.xml"/>`);
    wbRels.push(`<Relationship Id="rId${n + 1}" Type="${REL}/styles" Target="styles.xml"/>`, `<Relationship Id="rId${n + 2}" Type="${REL}/theme" Target="theme/theme1.xml"/>`, `<Relationship Id="rId${n + 3}" Type="${REL}/sharedStrings" Target="sharedStrings.xml"/>`);
    const ct = [['/xl/workbook.xml', 'application/vnd.openxmlformats-officedocument.spreadsheetml.sheet.main+xml'], ['/xl/styles.xml', 'application/vnd.openxmlformats-officedocument.spreadsheetml.styles+xml'],
      ['/xl/theme/theme1.xml', 'application/vnd.openxmlformats-officedocument.theme+xml'], ['/xl/sharedStrings.xml', 'application/vnd.openxmlformats-officedocument.spreadsheetml.sharedStrings+xml'],
      ...this.sheets.map((_, i) => [`/xl/worksheets/sheet${i + 1}.xml`, 'application/vnd.openxmlformats-officedocument.spreadsheetml.worksheet+xml'])];
    const extra = [];
    if (this.spill) {
      wbRels.push(`<Relationship Id="rId${n + 4}" Type="${REL}/sheetMetadata" Target="metadata.xml"/>`);
      ct.push(['/xl/metadata.xml', 'application/vnd.openxmlformats-officedocument.spreadsheetml.sheetMetadata+xml']);
      extra.push(['xl/metadata.xml', `${XML}<metadata xmlns="${NS}" xmlns:xda="http://schemas.microsoft.com/office/spreadsheetml/2017/dynamicarray"><metadataTypes count="1"><metadataType name="XLDAPR" minSupportedVersion="120000" copy="1" pasteAll="1" pasteValues="1" merge="1" splitFirst="1" rowColShift="1" clearFormats="1" clearComments="1" assign="1" coerce="1" cellMeta="1"/></metadataTypes><futureMetadata name="XLDAPR" count="1"><bk><extLst><ext uri="{bdbb8cdc-fa1e-496e-a857-3c3f30c029c3}"><xda:dynamicArrayProperties fDynamic="1" fCollapsed="0"/></ext></extLst></bk></futureMetadata><cellMetadata count="1"><bk><rc t="1" v="0"/></bk></cellMetadata></metadata>`]);
    }
    ct.push(...this.overrides);
    const contentTypes = `${XML}<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/><Default Extension="xml" ContentType="application/xml"/>${ct.map(([p, t]) => `<Override PartName="${p}" ContentType="${t}"/>`).join('')}</Types>`;
    // sharedStrings after the sheets (the sheets fill the table)
    return zipBytes([
      ['[Content_Types].xml', contentTypes],
      ['_rels/.rels', `${XML}<Relationships xmlns="${PKG_REL}"><Relationship Id="rId1" Type="${REL}/officeDocument" Target="xl/workbook.xml"/></Relationships>`],
      ['xl/workbook.xml', this.workbookXml()],
      ['xl/_rels/workbook.xml.rels', `${XML}<Relationships xmlns="${PKG_REL}">${wbRels.join('')}</Relationships>`],
      ['xl/styles.xml', this.stylesXml()],
      ['xl/theme/theme1.xml', THEME],
      ...sheetEntries,
      ['xl/sharedStrings.xml', this.sstXml()],
      ...extra,
      ...this.parts,
    ]);
  }
}

const THEME = `${XML}<a:theme xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main" name="Office Theme"><a:themeElements><a:clrScheme name="Office"><a:dk1><a:sysClr val="windowText" lastClr="000000"/></a:dk1><a:lt1><a:sysClr val="window" lastClr="FFFFFF"/></a:lt1><a:dk2><a:srgbClr val="44546A"/></a:dk2><a:lt2><a:srgbClr val="E7E6E6"/></a:lt2><a:accent1><a:srgbClr val="4472C4"/></a:accent1><a:accent2><a:srgbClr val="ED7D31"/></a:accent2><a:accent3><a:srgbClr val="A5A5A5"/></a:accent3><a:accent4><a:srgbClr val="FFC000"/></a:accent4><a:accent5><a:srgbClr val="5B9BD5"/></a:accent5><a:accent6><a:srgbClr val="70AD47"/></a:accent6><a:hlink><a:srgbClr val="0563C1"/></a:hlink><a:folHlink><a:srgbClr val="954F72"/></a:folHlink></a:clrScheme><a:fontScheme name="Office"><a:majorFont><a:latin typeface="Calibri Light"/><a:ea typeface=""/><a:cs typeface=""/></a:majorFont><a:minorFont><a:latin typeface="Calibri"/><a:ea typeface=""/><a:cs typeface=""/></a:minorFont></a:fontScheme><a:fmtScheme name="Office"><a:fillStyleLst><a:solidFill><a:schemeClr val="phClr"/></a:solidFill><a:solidFill><a:schemeClr val="phClr"/></a:solidFill><a:solidFill><a:schemeClr val="phClr"/></a:solidFill></a:fillStyleLst><a:lnStyleLst><a:ln w="6350"><a:solidFill><a:schemeClr val="phClr"/></a:solidFill></a:ln><a:ln w="12700"><a:solidFill><a:schemeClr val="phClr"/></a:solidFill></a:ln><a:ln w="19050"><a:solidFill><a:schemeClr val="phClr"/></a:solidFill></a:ln></a:lnStyleLst><a:effectStyleLst><a:effectStyle><a:effectLst/></a:effectStyle><a:effectStyle><a:effectLst/></a:effectStyle><a:effectStyle><a:effectLst/></a:effectStyle></a:effectStyleLst><a:bgFillStyleLst><a:solidFill><a:schemeClr val="phClr"/></a:solidFill><a:solidFill><a:schemeClr val="phClr"/></a:solidFill><a:solidFill><a:schemeClr val="phClr"/></a:solidFill></a:bgFillStyleLst></a:fmtScheme></a:themeElements></a:theme>`;

// ---------------------------------------------------------------- calc-heavy workbooks (benchmarks, calc-order)
/**
 * A formula-heavy workbook for the full-recalc benchmark and the large calc-order run: `rows` data rows and the
 * classes of uo-calc.md "Performance budgets" in one sheet: a chain (G = G above + A), range sums, lookups
 * (VLOOKUP/MATCH/XLOOKUP over the data), text functions, a few spills and volatiles. No cached values, calcId of an
 * old Excel (the open-recalc rule says `full`).
 */
export function buildCalcHeavy(rows = 20000, seed = 0x5eed) {
  const R = new Rand(seed);
  const NSx = NS;
  const out = [];
  const sst = ['Id', 'Valor', 'Nome', 'Chave'];
  const names = ['ana', 'bruno', 'carla', 'davi', 'eva', 'fábio', 'gil', 'hélio'];
  for (const n of names) sst.push(n);
  out.push(`<row r="1"><c r="A1" t="s"><v>0</v></c><c r="B1" t="s"><v>1</v></c><c r="C1" t="s"><v>2</v></c><c r="D1" t="s"><v>3</v></c></row>`);
  for (let i = 2; i <= rows + 1; i++) {
    const a = R.int(1, rows);
    const b = R.int(1, 100000) / 100;
    const nm = 4 + R.below(names.length);
    const f = [
      `<c r="E${i}"><f>${i === 2 ? 'A2' : `E${i - 1}+A${i}`}</f></c>`, // chain
      `<c r="F${i}"><f>B${i}*2+IF(A${i}&gt;${rows / 2},1,0)</f></c>`,
      i % 10 === 0 ? `<c r="G${i}"><f>SUM(B${Math.max(2, i - 99)}:B${i})</f></c>` : '',
      i % 25 === 0 ? `<c r="H${i}"><f>VLOOKUP(${R.int(1, rows)},$A$2:$C$${rows + 1},2,FALSE)</f></c>` : '',
      i % 25 === 5 ? `<c r="H${i}"><f>MATCH(A${i},$A$2:$A$${rows + 1},0)</f></c>` : '',
      i % 50 === 7 ? `<c r="H${i}"><f>_xlfn.XLOOKUP(A${i},$A$2:$A$${rows + 1},$C$2:$C$${rows + 1},"-")</f></c>` : '',
      i % 4 === 0 ? `<c r="I${i}"><f>UPPER(C${i})&amp;"-"&amp;LEN(C${i})</f></c>` : '',
      i % 100 === 0 ? `<c r="J${i}"><f>SUMIF($C$2:$C$${rows + 1},C${i},$B$2:$B$${rows + 1})</f></c>` : '',
    ].join('');
    out.push(`<row r="${i}"><c r="A${i}"><v>${a}</v></c><c r="B${i}"><v>${b}</v></c><c r="C${i}" t="s"><v>${nm}</v></c><c r="D${i}"><v>${R.int(1, 9)}</v></c>${f}</row>`);
  }
  out.push(`<row r="${rows + 3}"><c r="A${rows + 3}"><f>SUM(A2:A${rows + 1})</f></c><c r="B${rows + 3}"><f>RAND()</f></c><c r="C${rows + 3}"><f>NOW()</f></c><c r="D${rows + 3}" cm="1"><f t="array" ref="D${rows + 3}">_xlfn.UNIQUE(C2:C${rows + 1})</f></c><c r="E${rows + 3}" cm="1"><f t="array" ref="E${rows + 3}">_xlfn._xlws.SORT(D2:D${Math.min(rows + 1, 2001)})</f></c></row>`);
  return singleSheetBook(out.join(''), { sst, name: 'Calc', dyn: true, calcPr: '<calcPr calcId="191029" fullCalcOnLoad="1"/>' });
}

/** A one-sheet package around `<sheetData>` rows (the calc classes and the heavy workbook). */
export function singleSheetBook(rowsXml, { sst = [], name = 'Calc', dyn = false, calcPr = '<calcPr calcId="191029" fullCalcOnLoad="1"/>' } = {}) {
  const sheet = `${XML}<worksheet xmlns="${NS}" xmlns:r="${NSR}"><sheetData>${rowsXml}</sheetData></worksheet>`;
  const sstXml = `${XML}<sst xmlns="${NS}" count="${sst.length}" uniqueCount="${sst.length}">${sst.map((t) => `<si><t>${esc(t)}</t></si>`).join('')}</sst>`;
  const meta = `${XML}<metadata xmlns="${NS}" xmlns:xda="http://schemas.microsoft.com/office/spreadsheetml/2017/dynamicarray"><metadataTypes count="1"><metadataType name="XLDAPR" minSupportedVersion="120000" copy="1" pasteAll="1" pasteValues="1" merge="1" splitFirst="1" rowColShift="1" clearFormats="1" clearComments="1" assign="1" coerce="1" cellMeta="1"/></metadataTypes><futureMetadata name="XLDAPR" count="1"><bk><extLst><ext uri="{bdbb8cdc-fa1e-496e-a857-3c3f30c029c3}"><xda:dynamicArrayProperties fDynamic="1" fCollapsed="0"/></ext></extLst></bk></futureMetadata><cellMetadata count="1"><bk><rc t="1" v="0"/></bk></cellMetadata></metadata>`;
  const ct = ['<Override PartName="/xl/workbook.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.sheet.main+xml"/>',
    '<Override PartName="/xl/styles.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.styles+xml"/>',
    '<Override PartName="/xl/sharedStrings.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.sharedStrings+xml"/>',
    '<Override PartName="/xl/worksheets/sheet1.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.worksheet+xml"/>'];
  const rels = [`<Relationship Id="rId1" Type="${REL}/worksheet" Target="worksheets/sheet1.xml"/>`, `<Relationship Id="rId2" Type="${REL}/styles" Target="styles.xml"/>`, `<Relationship Id="rId3" Type="${REL}/sharedStrings" Target="sharedStrings.xml"/>`];
  if (dyn) { ct.push('<Override PartName="/xl/metadata.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.sheetMetadata+xml"/>'); rels.push(`<Relationship Id="rId4" Type="${REL}/sheetMetadata" Target="metadata.xml"/>`); }
  return zipBytes([
    ['[Content_Types].xml', `${XML}<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/><Default Extension="xml" ContentType="application/xml"/>${ct.join('')}</Types>`],
    ['_rels/.rels', `${XML}<Relationships xmlns="${PKG_REL}"><Relationship Id="rId1" Type="${REL}/officeDocument" Target="xl/workbook.xml"/></Relationships>`],
    ['xl/workbook.xml', `${XML}<workbook xmlns="${NS}" xmlns:r="${NSR}"><fileVersion appName="xl" lastEdited="7" lowestEdited="7" rupBuild="27425"/><workbookPr/><sheets><sheet name="${esc(name)}" sheetId="1" r:id="rId1"/></sheets>${calcPr}</workbook>`],
    ['xl/_rels/workbook.xml.rels', `${XML}<Relationships xmlns="${PKG_REL}">${rels.join('')}</Relationships>`],
    ['xl/styles.xml', `${XML}<styleSheet xmlns="${NS}"><fonts count="1"><font><sz val="11"/><name val="Calibri"/></font></fonts><fills count="2"><fill><patternFill patternType="none"/></fill><fill><patternFill patternType="gray125"/></fill></fills><borders count="1"><border><left/><right/><top/><bottom/><diagonal/></border></borders><cellStyleXfs count="1"><xf numFmtId="0" fontId="0" fillId="0" borderId="0"/></cellStyleXfs><cellXfs count="1"><xf numFmtId="0" fontId="0" fillId="0" borderId="0" xfId="0"/></cellXfs><cellStyles count="1"><cellStyle name="Normal" xfId="0" builtinId="0"/></cellStyles></styleSheet>`],
    ['xl/worksheets/sheet1.xml', sheet],
    ['xl/sharedStrings.xml', sstXml],
    ...(dyn ? [['xl/metadata.xml', meta]] : []),
  ]);
}

/**
 * The generated calc classes of uo-calc.md "Performance budgets" (one sheet each, no cached values, full calc on
 * load): the benchmark workbooks of the `sheet` suite (core/tools/bench/speed/sheet.mjs).
 */
export const CALC_CLASSES = {
  'chain-100k': () => rowsBook(100000, (i) => (i === 1 ? '<c r="A1"><v>1</v></c>' : `<c r="A${i}"><f>A${i - 1}+1</f></c>`)),
  'sum-ranges-100k': () => rowsBook(100000, (i) => `<c r="A${i}"><v>${(i * 7919) % 1000}</v></c><c r="B${i}"><f>SUM(A${Math.max(1, i - 99)}:A${i})</f></c>`),
  'lookup-10kx10k': () => rowsBook(10000, (i) => `<c r="A${i}"><v>${(i * 7919) % 10007}</v></c><c r="B${i}"><v>${i}</v></c><c r="D${i}"><f>${i % 3 === 0 ? `VLOOKUP(${(i * 31) % 10007},$A$1:$B$10000,2,FALSE)` : i % 3 === 1 ? `MATCH(${(i * 17) % 10007},$A$1:$A$10000,0)` : `_xlfn.XLOOKUP(${(i * 13) % 10007},$A$1:$A$10000,$B$1:$B$10000,-1)`}</f></c>`),
  'text-50k': () => rowsBook(50000, (i) => `<c r="A${i}" t="inlineStr"><is><t>texto ${i} ção</t></is></c><c r="B${i}"><f>${['UPPER(A' + i + ')', 'LEN(A' + i + ')&amp;"x"', 'SUBSTITUTE(A' + i + ',"o","0")', 'TEXT(' + i + '/7,"0.00")', 'LEFT(A' + i + ',5)&amp;RIGHT(A' + i + ',3)'][i % 5]}</f></c>`),
  'dynamic-1k': () => rowsBook(1000, (i) => `<c r="A${i}"><v>${(i * 7919) % 997}</v></c>${i <= 20 ? `<c r="${colName(2 + i)}1" cm="1"><f t="array" ref="${colName(2 + i)}1">${['_xlfn._xlws.SORT($A$1:$A$1000)', '_xlfn.UNIQUE($A$1:$A$1000)', '_xlfn._xlws.FILTER($A$1:$A$1000,$A$1:$A$1000&gt;500)', '_xlfn.SEQUENCE(1000)'][i % 4]}</f></c>` : ''}`, { dyn: true }),
  'volatile-10k': () => rowsBook(10000, (i) => `<c r="A${i}"><f>RAND()</f></c><c r="B${i}"><f>A${i}*2+${i}</f></c>`),
  'cycles-iterate': () => rowsBook(1000, (i) => `<c r="A${i}"><f>B${i}*0.5+1</f></c><c r="B${i}"><f>A${i}*0.5</f></c>`, { calcPr: '<calcPr calcId="191029" fullCalcOnLoad="1" iterate="1" iterateCount="100" iterateDelta="0.001"/>' }),
  'shared-1m': () => rowsBook(1000000, (i) => (i === 1 ? '<c r="A1"><v>1</v></c><c r="B1"><f t="shared" ref="B1:B1000000" si="0">A1*2</f></c>' : `<c r="A${i}"><v>${i}</v></c><c r="B${i}"><f t="shared" si="0"/></c>`)),
};
function rowsBook(n, cells, opts = {}) {
  const parts = [];
  for (let i = 1; i <= n; i++) parts.push(`<row r="${i}">${cells(i)}</row>`);
  return singleSheetBook(parts.join(''), opts);
}
