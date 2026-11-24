// Seeded deterministic generator of CONDITIONAL-FORMAT workbooks (frontier C3b, record core/docs/modules/uo-xlsx.md
// "C3b — as built"): every rule type of cond-format.ts and its edges — ties, blanks, text in numeric ranges, errors,
// percent/percentile/formula thresholds, negative data bars (x14 axis modes), reverse and custom icon sets, time
// periods around the month/week boundaries of the frozen clock (2025-01-15, a Wednesday), priorities and stopIfTrue,
// overlapping dxfs, dxf number formats, x14 rules with their own dxf, cross-sheet formulas, 1904 dates, and the
// cf-formula.ts functions. Pure Node: same group + index -> same bytes.
//
//   buildCfWorkbook(group, i) -> { bytes, name }      groups: cf1 (CI, committed), cf-big (local)
import { zipBytes } from '../../../uo-pptx/js/lib/zip.mjs';

export const CF_GROUPS = {
  cf1: { seedBase: 0xcf000, count: 40, fixed: true, rows: [8, 30] },
  'cf-big': { seedBase: 0xcf800, count: 300, fixed: false, rows: [20, 200] },
};
export const cfKey = (g, i) => `gen/${g}/${String(i).padStart(4, '0')}`;
export const cfFile = (g, i) => `${g}-${String(i).padStart(4, '0')}.xlsx`;

const NS = 'http://schemas.openxmlformats.org/spreadsheetml/2006/main';
const NSR = 'http://schemas.openxmlformats.org/officeDocument/2006/relationships';
const REL = 'http://schemas.openxmlformats.org/officeDocument/2006/relationships';
const PKG_REL = 'http://schemas.openxmlformats.org/package/2006/relationships';
const X14 = 'http://schemas.microsoft.com/office/spreadsheetml/2009/9/main';
const XM = 'http://schemas.microsoft.com/office/excel/2006/main';
const XML = '<?xml version="1.0" encoding="UTF-8" standalone="yes"?>\n';
const esc = (s) => String(s).replace(/&/g, '&amp;').replace(/</g, '&lt;').replace(/>/g, '&gt;').replace(/"/g, '&quot;');
export const colName = (c) => { let s = ''; for (c += 1; c > 0; c = Math.floor((c - 1) / 26)) s = String.fromCharCode(65 + ((c - 1) % 26)) + s; return s; };
const ref = (c, r) => `${colName(c)}${r + 1}`;

class Rng {
  constructor(seed) { this.s = (seed >>> 0) || 1; }
  next() { let x = this.s; x ^= x << 13; x >>>= 0; x ^= x >>> 17; x ^= x << 5; x >>>= 0; this.s = x; return x; }
  below(n) { return this.next() % n; }
  pick(a) { return a[this.below(a.length)]; }
  chance(p) { return this.next() / 4294967296 < p; }
  int(a, b) { return a + this.below(b - a + 1); }
  hex6() { return this.next().toString(16).padStart(8, '0').slice(0, 6).toUpperCase(); }
}

// the frozen clock of the captures: 2025-01-15 (serial 45672), a Wednesday; week Sunday 12 .. Saturday 18
const TODAY = 45672;
const DXFS = [
  '<dxf><font><b/><color rgb="FF9C0006"/></font><fill><patternFill><bgColor rgb="FFFFC7CE"/></patternFill></fill></dxf>',
  '<dxf><font><i/><color rgb="FF006100"/></font><fill><patternFill><bgColor rgb="FFC6EFCE"/></patternFill></fill></dxf>',
  '<dxf><font><u/><strike/></font><border><left style="thin"><color rgb="FFFF0000"/></left><bottom style="double"><color rgb="FF0000FF"/></bottom></border></dxf>',
  '<dxf><numFmt numFmtId="164" formatCode="0.000"/><fill><patternFill patternType="solid"><fgColor rgb="FFFFEB9C"/><bgColor rgb="FFFFEB9C"/></patternFill></fill></dxf>',
  '<dxf><font><b val="0"/><color theme="4" tint="-0.249977111117893"/></font><border><right style="thin"><color auto="1"/></right><top style="medium"><color indexed="10"/></top></border></dxf>',
  '<dxf><numFmt numFmtId="10" formatCode="0.00%"/></dxf>',
  '<dxf><fill><patternFill patternType="none"/></fill><font><color rgb="FF0070C0"/></font></dxf>',
  '<dxf><numFmt numFmtId="165" formatCode="[Red]&quot;neg&quot; 0;&quot;pos&quot; 0.0"/><font><b/></font></dxf>',
];

/** A cell value spec -> `<c>` XML (shared strings collected in `sst`). */
function cellXml(c, r, v, sst) {
  const at = ref(c, r);
  if (v === undefined || v === null) return '';
  if (typeof v === 'number') return `<c r="${at}"><v>${v}</v></c>`;
  if (typeof v === 'string') { let k = sst.indexOf(v); if (k < 0) { k = sst.length; sst.push(v); } return `<c r="${at}" t="s"><v>${k}</v></c>`; }
  if (typeof v === 'boolean') return `<c r="${at}" t="b"><v>${v ? 1 : 0}</v></c>`;
  if (v.e !== undefined) return `<c r="${at}" t="e"><v>${esc(v.e)}</v></c>`;
  if (v.is !== undefined) return `<c r="${at}" t="inlineStr"><is><t xml:space="preserve">${esc(v.is)}</t></is></c>`;
  if (v.raw !== undefined) return `<c r="${at}"${v.t !== undefined ? ` t="${v.t}"` : ''}>${v.raw === null ? '' : `<v>${esc(v.raw)}</v>`}</c>`;
  if (v.f !== undefined) return `<c r="${at}"${v.t ? ` t="${v.t}"` : ''}><f>${esc(v.f)}</f>${v.v === undefined ? '' : `<v>${esc(v.v)}</v>`}</c>`;
  if (v.d !== undefined) return `<c r="${at}" t="d"><v>${esc(v.d)}</v></c>`;
  if (v.s !== undefined) return `<c r="${at}" s="${v.s}"><v>${v.n}</v></c>`;
  throw new Error('cell spec');
}

let GUID = 0;
const guid = () => `{00000000-0000-4000-8000-${String(++GUID).padStart(12, '0')}}`;

/**
 * A workbook from sheets `{ name, rows: value[][] (row-major, from A1), cf: string[] (conditionalFormatting XML),
 * x14: string[] (x14:conditionalFormatting XML) }`.
 */
function book(sheets, { date1904 = false } = {}) {
  const sst = [];
  const parts = [];
  sheets.forEach((sh, si) => {
    const rows = sh.rows.map((row, r) => {
      const cells = row.map((v, c) => cellXml(c, r, v, sst)).join('');
      return cells ? `<row r="${r + 1}">${cells}</row>` : '';
    }).join('');
    const ext = sh.x14 && sh.x14.length
      ? `<extLst><ext uri="{78C0D931-6437-407d-A8EE-F0AAD7539E65}" xmlns:x14="${X14}"><x14:conditionalFormattings>${sh.x14.join('')}</x14:conditionalFormattings></ext></extLst>`
      : '';
    parts.push([`xl/worksheets/sheet${si + 1}.xml`, `${XML}<worksheet xmlns="${NS}" xmlns:r="${NSR}" xmlns:x14="${X14}" xmlns:xm="${XM}"><sheetData>${rows}</sheetData>${(sh.cf ?? []).join('')}${ext}</worksheet>`]);
  });
  const ct = ['<Override PartName="/xl/workbook.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.sheet.main+xml"/>',
    '<Override PartName="/xl/styles.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.styles+xml"/>',
    '<Override PartName="/xl/sharedStrings.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.sharedStrings+xml"/>',
    ...sheets.map((_, i) => `<Override PartName="/xl/worksheets/sheet${i + 1}.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.worksheet+xml"/>`)];
  const rels = [...sheets.map((_, i) => `<Relationship Id="rId${i + 1}" Type="${REL}/worksheet" Target="worksheets/sheet${i + 1}.xml"/>`),
    `<Relationship Id="rId${sheets.length + 1}" Type="${REL}/styles" Target="styles.xml"/>`,
    `<Relationship Id="rId${sheets.length + 2}" Type="${REL}/sharedStrings" Target="sharedStrings.xml"/>`];
  const styles = `${XML}<styleSheet xmlns="${NS}"><numFmts count="1"><numFmt numFmtId="166" formatCode="dd/mm/yyyy"/></numFmts><fonts count="2"><font><sz val="11"/><name val="Calibri"/></font><font><b/><sz val="11"/><name val="Calibri"/></font></fonts><fills count="2"><fill><patternFill patternType="none"/></fill><fill><patternFill patternType="gray125"/></fill></fills><borders count="1"><border><left/><right/><top/><bottom/><diagonal/></border></borders><cellStyleXfs count="1"><xf numFmtId="0" fontId="0" fillId="0" borderId="0"/></cellStyleXfs><cellXfs count="4"><xf numFmtId="0" fontId="0" fillId="0" borderId="0" xfId="0"/><xf numFmtId="166" fontId="0" fillId="0" borderId="0" xfId="0" applyNumberFormat="1"/><xf numFmtId="4" fontId="1" fillId="0" borderId="0" xfId="0" applyNumberFormat="1"/><xf numFmtId="0" fontId="0" fillId="0" borderId="0" xfId="0"><alignment horizontal="left" indent="1"/></xf></cellXfs><cellStyles count="1"><cellStyle name="Normal" xfId="0" builtinId="0"/></cellStyles><dxfs count="${DXFS.length}">${DXFS.join('')}</dxfs></styleSheet>`;
  return zipBytes([
    ['[Content_Types].xml', `${XML}<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/><Default Extension="xml" ContentType="application/xml"/>${ct.join('')}</Types>`],
    ['_rels/.rels', `${XML}<Relationships xmlns="${PKG_REL}"><Relationship Id="rId1" Type="${REL}/officeDocument" Target="xl/workbook.xml"/></Relationships>`],
    ['xl/workbook.xml', `${XML}<workbook xmlns="${NS}" xmlns:r="${NSR}"><fileVersion appName="xl" lastEdited="7" lowestEdited="7" rupBuild="27425"/><workbookPr${date1904 ? ' date1904="1"' : ''}/><sheets>${sheets.map((s, i) => `<sheet name="${esc(s.name)}" sheetId="${i + 1}" r:id="rId${i + 1}"/>`).join('')}</sheets><calcPr calcId="191029"/></workbook>`],
    ['xl/_rels/workbook.xml.rels', `${XML}<Relationships xmlns="${PKG_REL}">${rels.join('')}</Relationships>`],
    ['xl/styles.xml', styles],
    ...parts,
    ['xl/sharedStrings.xml', `${XML}<sst xmlns="${NS}" count="${sst.length}" uniqueCount="${sst.length}">${sst.map((t) => `<si><t xml:space="preserve">${esc(t)}</t></si>`).join('')}</sst>`],
  ]);
}

// ---------------------------------------------------------------- rule XML helpers
const cf = (sqref, rules) => `<conditionalFormatting sqref="${sqref}">${rules.join('')}</conditionalFormatting>`;
const attrs = (o) => Object.entries(o).filter(([, v]) => v !== undefined).map(([k, v]) => ` ${k}="${esc(v)}"`).join('');
const rule = (a, inner = '') => `<cfRule${attrs(a)}>${inner}</cfRule>`;
const fx = (...fs) => fs.map((f) => `<formula>${esc(f)}</formula>`).join('');
const cfvo = (type, val, gte) => `<cfvo${attrs({ type, val, gte })}/>`;
const color = (rgb) => `<color rgb="FF${rgb}"/>`;
const x14cfvo = (type, f) => (f === undefined ? `<x14:cfvo type="${type}"/>` : `<x14:cfvo type="${type}"><xm:f>${esc(f)}</xm:f></x14:cfvo>`);
const x14cf = (sqref, rules) => `<x14:conditionalFormatting>${rules.join('')}<xm:sqref>${sqref}</xm:sqref></x14:conditionalFormatting>`;
const x14id = (id) => `<extLst><ext uri="{B025F937-C7B1-47D3-B67F-A62EFF666E3E}" xmlns:x14="${X14}"><x14:id>${id}</x14:id></ext></extLst>`;

const NUMS = [0, 1, 2, 2, 3, 5, 5, 7, 10, 10, -1, -4.5, 0.5, 12.25, 100, 1e6, 3.14159, -0.001, 42, 99];
const TEXTS = ['alfa', 'Alfa', 'ALFA', 'beta', 'Gama', 'texto com a', 'banana', 'São', 'são', '', '  ', 'x1', '1,5', '7', 'TRUE', 'ab*c', 'a?c'];
function value(R, kinds) {
  switch (R.pick(kinds)) {
    case 'n': return R.pick(NUMS);
    case 'i': return R.int(-20, 120);
    case 'd': return TODAY + R.int(-45, 45);
    case 't': return R.pick(TEXTS);
    case 'b': return R.chance(0.5);
    case 'e': return { e: R.pick(['#N/A', '#DIV/0!', '#VALUE!', '#n/a']) };
    case 'blank': return undefined;
    case 'inline': return { is: R.pick(TEXTS) };
    case 'raw': return { raw: R.pick([' 7', '1E3', '0x10', 'abc', '', 'Infinity', '-0', '1.50']) };
    case 'iso': return { d: R.pick(['2025-01-15', '2025-01-14T12:00:00', 'x']) };
    case 'f': return { f: 'A1+1', v: String(R.int(0, 50)) };
    case 'fs': return { f: '"x"&A1', t: 'str', v: R.pick(['xa', 'xb', '']) };
    default: return undefined;
  }
}
function grid(R, rows, cols, kinds) {
  const out = [];
  for (let r = 0; r < rows; r++) { const row = []; for (let c = 0; c < cols; c++) row.push(value(R, kinds)); out.push(row); }
  return out;
}

const FORMULAS = [
  'MOD(ROW(),2)=0', 'MOD(COLUMN(),3)=1', '$A1>5', 'A1>AVERAGE($A$1:$A$30)', 'COUNTIF($A$1:$A$30,A1)>1', 'ISBLANK(A1)', 'ISERROR(A1)',
  'AND(A1>0,A1<10)', 'OR(A1="alfa",A1>50)', 'NOT(ISNUMBER(A1))', 'LEN(A1)>3', 'LEFT(A1,1)="a"', 'RIGHT(A1;2)="fa"', 'MID(A1,2,2)="lf"',
  'ISNUMBER(SEARCH("a*a",A1))', 'IFERROR(FIND("a",A1),0)>1', 'TEXT(A1,"0.0")="5,0"', 'YEAR(A1)=2025', 'MONTH(A1)=1', 'WEEKDAY(A1,2)>5',
  'A1>=TODAY()', 'A1<NOW()', 'EOMONTH(A1,0)=EOMONTH(TODAY(),0)', 'EDATE(A1,1)>TODAY()', 'A1=MAX($A$1:$A$30)', 'A1=MIN($A:$A)', 'A1>=LARGE($A$1:$A$30,3)',
  'A1<=SMALL($A$1:$A$30,2)', 'A1>MEDIAN($A$1:$A$30)', 'SUMIF($A$1:$A$30,">5")>A1', 'COUNTIFS($A$1:$A$30,">0",$B$1:$B$30,"<>")>3',
  'ROUND(A1,0)=A1', 'ROUNDUP(A1/3,1)>1', 'ROUNDDOWN(A1,-1)=10', 'INT(A1)=A1', 'ABS(A1)>5', 'SIGN(A1)<0', 'SQRT(ABS(A1))>2', 'POWER(A1,2)>50',
  'A1^2>A1*10', 'A1&"x"="alfax"', 'UPPER(A1)="ALFA"', 'LOWER(A1)="alfa"', 'TRIM(A1)=""', 'EXACT(A1,"Alfa")', 'SUBSTITUTE(A1,"a","")="lf"',
  'CONCATENATE(A1,B1)<>""', 'VALUE(A1)>3', 'N(A1)>0', 'T(A1)<>""', 'ISTEXT(A1)', 'ISLOGICAL(A1)', 'ISNA(A1)', 'ISERR(A1)', 'IFNA(A1,0)=0',
  'XOR(A1>1,B1>1)', 'ISEVEN(A1)', 'ISODD(A1)', 'COUNTBLANK($A$1:$B$10)>2', 'COUNTA($A$1:$A$30)>COUNT($A$1:$A$30)', 'SUM($A$1:A1)>20',
  'MATCH(A1,$A$1:$A$30,0)<5', 'VLOOKUP(A1,$A$1:$B$30,2,FALSE)>0', 'HLOOKUP(A1,$A$1:$C$3,2,FALSE)<>""', 'ROWS($A$1:$A$9)=9', 'COLUMNS(A1:C1)=3',
  'A1=DATE(2025,1,15)', 'DAY(A1)=15', "'Outra Aba'!$A$1>A1", 'Outra!A1=A1', 'NoSuch!A1=1', 'INDIRECT("A1")=1', 'A1>#N/A', 'A1="#N/A"',
  'IF(A1>5,TRUE)', 'IF(A1,1,0)', '_xlfn.IFNA(A1,1)=1', 'A1%>0,05', '-A1>0', '+A1>"a"', '(A1)', 'A1:A3=1', '1:1=1', 'A:A>0', 'SUM(1:1)>0',
  'A1=TRUE', 'A1<>FALSE', 'foo(A1)', 'A1=', '"a"="A"', '1=1', '0', '""', '#REF!', 'NA()', 'TRUE()', 'A1>=1E3', '.5<A1', 'A1 > 2',
];

/** Random rules over the sheet (sqref ranges inside rows x cols, data in A..). */
function randomRules(R, rows, cols, prioStart) {
  const cfs = [], x14 = [];
  let p = prioStart;
  const col = () => colName(R.below(cols));
  const area = () => {
    const c1 = R.below(cols), c2 = Math.min(cols - 1, c1 + R.below(3)), r1 = R.below(3), r2 = Math.min(rows + 1, r1 + R.int(2, rows));
    const a = `${colName(c1)}${r1 + 1}:${colName(c2)}${r2 + 1}`;
    return R.chance(0.15) ? `${a} ${col()}${R.int(1, rows)}` : R.chance(0.1) ? `${colName(c1)}:${colName(c1)}` : a;
  };
  const dxf = () => (R.chance(0.9) ? R.below(DXFS.length) : R.pick([99, -1]));
  const stop = () => (R.chance(0.2) ? { stopIfTrue: 1 } : {});
  const n = R.int(1, 5);
  for (let k = 0; k < n; k++) {
    const sq = area();
    const prio = R.chance(0.1) ? p : ++p;
    const kind = R.pick(['cellIs', 'expression', 'colorScale', 'dataBar', 'iconSet', 'top10', 'aboveAverage', 'dup', 'text', 'blanks', 'errors', 'time', 'x14bar', 'x14icon', 'x14expr']);
    const base = { priority: prio, ...stop() };
    switch (kind) {
      case 'cellIs': {
        const op = R.pick(['equal', 'notEqual', 'greaterThan', 'lessThan', 'greaterThanOrEqual', 'lessThanOrEqual', 'between', 'notBetween', undefined, 'bogus']);
        const v1 = R.pick(['5', '0', '"alfa"', '$A$1', 'A1', '10', '-1', 'TRUE', '#N/A', '""', 'AVERAGE($A$1:$A$9)', '2.5', 'unsupported(']), v2 = R.pick(['20', '1', '"zeta"', '$B$2', '0']);
        cfs.push(cf(sq, [rule({ type: 'cellIs', dxfId: dxf(), operator: op, ...base }, fx(v1, ...(op === 'between' || op === 'notBetween' ? [v2] : [])))]));
        break;
      }
      case 'expression': cfs.push(cf(sq, [rule({ type: 'expression', dxfId: dxf(), ...base }, fx(R.pick(FORMULAS)))])); break;
      case 'colorScale': {
        const three = R.chance(0.5);
        const t = () => R.pick(['min', 'max', 'num', 'percent', 'percentile', 'formula', 'autoMin', 'bogus']);
        const v = (ty) => (ty === 'num' ? R.pick(['5', '0', '-3', '1e999', 'abc']) : ty === 'percent' || ty === 'percentile' ? R.pick(['0', '10', '50', '90', '100', '150', '-5']) : ty === 'formula' ? R.pick(['$A$1', '2*A1+2', 'AVERAGE($A$1:$A$9)', '0/0', 'NA()']) : undefined);
        const tys = three ? [R.pick(['min', 'num', 'percent']), t(), R.pick(['max', 'num', 'percentile'])] : [R.pick(['min', 'num', 'percent', 'formula']), R.pick(['max', 'percentile', 'num', 'formula'])];
        const cols2 = three ? ['F8696B', 'FFEB84', '63BE7B'] : ['FFFFFF', '5A8AC6'];
        cfs.push(cf(sq, [rule({ type: 'colorScale', ...base }, `<colorScale>${tys.map((ty) => cfvo(ty, v(ty))).join('')}${cols2.map(color).join('')}${R.chance(0.1) ? '<color theme="5" tint="0.4"/>' : ''}</colorScale>`)]));
        break;
      }
      case 'dataBar': {
        const ty = () => R.pick(['min', 'max', 'num', 'percent', 'percentile', 'formula']);
        const v = (t) => (t === 'num' ? R.pick(['0', '5', '-5']) : t === 'percent' || t === 'percentile' ? R.pick(['0', '25', '75', '100']) : t === 'formula' ? R.pick(['$A$1', 'MAX($A:$A)']) : undefined);
        const [a, b] = [ty(), ty()];
        cfs.push(cf(sq, [rule({ type: 'dataBar', ...base }, `<dataBar${R.chance(0.2) ? ' showValue="0"' : ''}>${cfvo(a, v(a))}${cfvo(b, v(b))}${color(R.hex6())}</dataBar>`)]));
        break;
      }
      case 'x14bar': {
        const id = guid();
        cfs.push(cf(sq, [rule({ type: 'dataBar', ...base }, `<dataBar><cfvo type="min"/><cfvo type="max"/>${color('638EC6')}</dataBar>${x14id(id)}`)]));
        const axis = R.pick([undefined, 'middle', 'none', 'automatic']);
        const ml = R.pick([[0, 100], [10, 90], [20, 80], [undefined, undefined]]);
        x14.push(x14cf(sq, [`<x14:cfRule type="dataBar" id="${id}"><x14:dataBar${attrs({ minLength: ml[0], maxLength: ml[1], border: R.pick([1, 0, undefined]), gradient: R.pick([0, 1, undefined]), axisPosition: axis, negativeBarColorSameAsPositive: R.pick([1, undefined]), negativeBarBorderColorSameAsPositive: R.pick([0, undefined]) })}>${x14cfvo(R.pick(['autoMin', 'min', 'num']), R.chance(0.3) ? '-5' : undefined)}${x14cfvo(R.pick(['autoMax', 'max', 'percent']), R.chance(0.3) ? '50' : undefined)}${R.chance(0.6) ? `<x14:borderColor rgb="FF${R.hex6()}"/>` : ''}${R.chance(0.6) ? '<x14:negativeFillColor rgb="FFFF0000"/>' : ''}${R.chance(0.5) ? '<x14:negativeBorderColor rgb="FFC00000"/>' : ''}${R.chance(0.5) ? '<x14:axisColor rgb="FF00B050"/>' : ''}</x14:dataBar></x14:cfRule>`]));
        break;
      }
      case 'iconSet': {
        const set = R.pick(['3Arrows', '3ArrowsGray', '3Flags', '3TrafficLights1', '3TrafficLights2', '3Signs', '3Symbols', '3Symbols2', '4Arrows', '4ArrowsGray', '4RedToBlack', '4Rating', '4TrafficLights', '5Arrows', '5ArrowsGray', '5Rating', '5Quarters', undefined, 'NoIcons', '10Foo']);
        const n = parseInt(set ?? '3', 10) || 3;
        const ty = R.pick(['percent', 'percentile', 'num', 'formula']);
        const vals = Array.from({ length: Math.min(n, 5) }, (_, i) => (ty === 'formula' ? `$A$${i + 1}` : String(ty === 'num' ? i * 3 : Math.round((100 * i) / n))));
        cfs.push(cf(sq, [rule({ type: 'iconSet', ...base }, `<iconSet${attrs({ iconSet: set, reverse: R.pick([1, undefined]), showValue: R.pick([0, undefined]) })}>${vals.map((v, i) => cfvo(ty, v, i > 0 && R.chance(0.3) ? '0' : undefined)).join('')}</iconSet>`)]));
        break;
      }
      case 'x14icon': {
        const set = R.pick(['3Stars', '5Boxes', '3Triangles', '3Arrows']);
        const n = parseInt(set, 10);
        const custom = R.chance(0.5);
        const icons = custom ? Array.from({ length: n }, () => `<x14:cfIcon iconSet="${R.pick(['3Arrows', 'NoIcons', '5Quarters', '3Flags'])}" iconId="${R.pick(['0', '1', '2', 'x'])}"/>`).join('') : '';
        x14.push(x14cf(sq, [`<x14:cfRule type="iconSet" priority="${prio}" id="${guid()}"><x14:iconSet${attrs({ iconSet: set, custom: custom ? 1 : undefined, reverse: R.pick([1, undefined]), showValue: R.pick([0, undefined]) })}>${Array.from({ length: n }, (_, i) => x14cfvo('percent', String(Math.round((100 * i) / n)))).join('')}${icons}</x14:iconSet></x14:cfRule>`]));
        break;
      }
      case 'x14expr':
        x14.push(x14cf(sq, [`<x14:cfRule type="expression" priority="${prio}" id="${guid()}"><xm:f>${esc(R.pick(FORMULAS))}</xm:f><x14:dxf><font><b/><color rgb="FF7030A0"/></font><fill><patternFill><bgColor rgb="FFE2EFDA"/></patternFill></fill></x14:dxf></x14:cfRule>`]));
        break;
      case 'top10': cfs.push(cf(sq, [rule({ type: 'top10', dxfId: dxf(), rank: R.pick([1, 2, 3, 10, 0, 50, 'x']), percent: R.pick([1, undefined]), bottom: R.pick([1, undefined]), ...base })])); break;
      case 'aboveAverage': cfs.push(cf(sq, [rule({ type: 'aboveAverage', dxfId: dxf(), aboveAverage: R.pick([0, undefined]), equalAverage: R.pick([1, undefined]), stdDev: R.pick([1, 2, undefined]), ...base })])); break;
      case 'dup': cfs.push(cf(sq, [rule({ type: R.pick(['duplicateValues', 'uniqueValues']), dxfId: dxf(), ...base })])); break;
      case 'text': {
        const type = R.pick(['containsText', 'notContainsText', 'beginsWith', 'endsWith']);
        const text = R.pick(['a', 'A', 'alf', 'FA', '', '1,5', 'são', '*']);
        const withF = R.chance(0.6);
        const f = type === 'containsText' ? `NOT(ISERROR(SEARCH("${text}",${ref(0, 0)})))` : type === 'notContainsText' ? `ISERROR(SEARCH("${text}",A1))` : type === 'beginsWith' ? `LEFT(A1,LEN("${text}"))="${text}"` : `RIGHT(A1,LEN("${text}"))="${text}"`;
        cfs.push(cf(sq, [rule({ type, dxfId: dxf(), operator: type, text, ...base }, withF ? fx(f) : '')]));
        break;
      }
      case 'blanks': cfs.push(cf(sq, [rule({ type: R.pick(['containsBlanks', 'notContainsBlanks']), dxfId: dxf(), ...base }, fx('LEN(TRIM(A1))=0'))])); break;
      case 'errors': cfs.push(cf(sq, [rule({ type: R.pick(['containsErrors', 'notContainsErrors']), dxfId: dxf(), ...base })])); break;
      case 'time': {
        const tp = R.pick(['today', 'yesterday', 'tomorrow', 'last7Days', 'thisWeek', 'lastWeek', 'nextWeek', 'thisMonth', 'lastMonth', 'nextMonth', undefined, 'bogus']);
        cfs.push(cf(sq, [rule({ type: 'timePeriod', dxfId: dxf(), timePeriod: tp, ...base }, fx('FLOOR(A1,1)=TODAY()'))]));
        break;
      }
    }
  }
  return { cfs, x14 };
}

// ---------------------------------------------------------------- the fixed workbooks (cf1, 0..15): one theme each
function fixed(i) {
  const N = (a) => a.map((x) => [x]);
  switch (i) {
    case 0: { // cellIs: every operator, blanks compare as 0/"", text, errors
      const rows = N([5, 10, undefined, 'alfa', 'Alfa', 0, -3, { e: '#N/A' }, 20, 7.5, true, '', 15, 3]);
      return book([{ name: 'cellIs', rows, cf: [
        cf('A1:A14', [rule({ type: 'cellIs', dxfId: 0, priority: 1, operator: 'lessThan' }, fx('4'))]),
        cf('A1:A14', [rule({ type: 'cellIs', dxfId: 1, priority: 2, operator: 'between' }, fx('20', '7'))]),
        cf('A1:A14', [rule({ type: 'cellIs', dxfId: 2, priority: 3, operator: 'equal' }, fx('"ALFA"'))]),
        cf('A1:A14', [rule({ type: 'cellIs', dxfId: 3, priority: 4, operator: 'notBetween' }, fx('$A$1', '$A$2'))]),
        cf('A1:A14', [rule({ type: 'cellIs', dxfId: 4, priority: 5, operator: 'greaterThanOrEqual' }, fx('A1'))]),
        cf('A1:A14', [rule({ type: 'cellIs', dxfId: 5, priority: 6, operator: 'notEqual' }, fx('TRUE'))]),
        cf('A1:A14', [rule({ type: 'cellIs', dxfId: 6, priority: 7 }, fx('""'))]),
        cf('A1:A14', [rule({ type: 'cellIs', dxfId: 7, priority: 8, operator: 'lessThanOrEqual' }, fx('#N/A'))]),
      ] }]);
    }
    case 1: { // colour scales: 2/3 colours, every cfvo type, mid clamped above/below, min = max, text in range
      const v = [1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 'x', undefined, 2.5, -1];
      return book([{ name: 'scales', rows: v.map((x, k) => [x, k < 3 ? x : 3, k % 2 ? 'a' : k, -x * 2 || 0]), cf: [
        cf('A1:A14', [rule({ type: 'colorScale', priority: 1 }, `<colorScale>${cfvo('min')}${cfvo('percentile', '50')}${cfvo('max')}${['F8696B', 'FFEB84', '63BE7B'].map(color).join('')}</colorScale>`)]),
        cf('B1:B14', [rule({ type: 'colorScale', priority: 2 }, `<colorScale>${cfvo('min')}${cfvo('max')}${color('FFFFFF')}${color('5A8AC6')}</colorScale>`)]),
        cf('C1:C14', [rule({ type: 'colorScale', priority: 3 }, `<colorScale>${cfvo('num', '0')}${cfvo('num', '50')}${cfvo('max')}${['F8696B', 'FFEB84', '63BE7B'].map(color).join('')}</colorScale>`)]),
        cf('D1:D14', [rule({ type: 'colorScale', priority: 4 }, `<colorScale>${cfvo('percent', '10')}${cfvo('num', '-100')}${cfvo('formula', '$A$1*3')}${['00FF00', 'FFFF00', 'FF0000'].map(color).join('')}</colorScale>`)]),
        cf('A1:D3', [rule({ type: 'colorScale', priority: 5 }, `<colorScale>${cfvo('percentile', '0')}${cfvo('percentile', '100')}<color theme="4"/><color theme="9" tint="-0.5"/></colorScale>`)]),
        cf('E1:E5', [rule({ type: 'colorScale', priority: 6 }, `<colorScale>${cfvo('min')}${cfvo('max')}${color('000000')}${color('FFFFFF')}</colorScale>`)]),
      ] }]);
    }
    case 2: { // data bars 2007 and x14: negatives, axis modes, lengths, gradient/border/colours, showValue
      const v = [-10, -5, 0, 5, 10, 20, 'x', undefined, 2.5, -2.5, 40, 1];
      const ids = [guid(), guid(), guid(), guid()];
      return book([{ name: 'bars', rows: v.map((x) => [x, x, x, x, x, typeof x === 'number' ? Math.abs(x) : x]), cf: [
        cf('A1:A12', [rule({ type: 'dataBar', priority: 1 }, `<dataBar><cfvo type="min"/><cfvo type="max"/>${color('638EC6')}</dataBar>`)]),
        cf('B1:B12', [rule({ type: 'dataBar', priority: 2 }, `<dataBar><cfvo type="min"/><cfvo type="max"/>${color('FF555A')}</dataBar>${x14id(ids[0])}`)]),
        cf('C1:C12', [rule({ type: 'dataBar', priority: 3 }, `<dataBar showValue="0"><cfvo type="num" val="0"/><cfvo type="num" val="20"/>${color('63C384')}</dataBar>${x14id(ids[1])}`)]),
        cf('D1:D12', [rule({ type: 'dataBar', priority: 4 }, `<dataBar><cfvo type="percentile" val="10"/><cfvo type="percent" val="90"/>${color('D6007B')}</dataBar>${x14id(ids[2])}`)]),
        cf('E1:E12', [rule({ type: 'dataBar', priority: 5 }, `<dataBar minLength="0" maxLength="100"><cfvo type="max"/><cfvo type="min"/>${color('FFB628')}</dataBar>${x14id(ids[3])}`)]),
        cf('F1:F12', [rule({ type: 'dataBar', priority: 6 }, `<dataBar><cfvo type="formula" val="MIN($F$1:$F$12)"/><cfvo type="formula" val="$F$11"/>${color('008AEF')}</dataBar>`)]),
      ], x14: [
        x14cf('B1:B12', [`<x14:cfRule type="dataBar" id="${ids[0]}"><x14:dataBar minLength="0" maxLength="100" border="1" negativeBarBorderColorSameAsPositive="0"><x14:cfvo type="autoMin"/><x14:cfvo type="autoMax"/><x14:borderColor rgb="FFFF555A"/><x14:negativeFillColor rgb="FFFF0000"/><x14:negativeBorderColor rgb="FFFF0000"/><x14:axisColor rgb="FF000000"/></x14:dataBar></x14:cfRule>`]),
        x14cf('C1:C12', [`<x14:cfRule type="dataBar" id="${ids[1]}"><x14:dataBar minLength="0" maxLength="100" gradient="0" axisPosition="middle" negativeBarColorSameAsPositive="1"><x14:cfvo type="num"><xm:f>-20</xm:f></x14:cfvo><x14:cfvo type="num"><xm:f>20</xm:f></x14:cfvo></x14:dataBar></x14:cfRule>`]),
        x14cf('D1:D12', [`<x14:cfRule type="dataBar" id="${ids[2]}"><x14:dataBar minLength="20" maxLength="80" axisPosition="none"><x14:cfvo type="percentile"><xm:f>10</xm:f></x14:cfvo><x14:cfvo type="percent"><xm:f>90</xm:f></x14:cfvo></x14:dataBar></x14:cfRule>`]),
        x14cf('E1:E12', [`<x14:cfRule type="dataBar" id="${ids[3]}"><x14:dataBar minLength="0" maxLength="100" border="1"><x14:cfvo type="max"/><x14:cfvo type="min"/><x14:axisColor rgb="FF7030A0"/></x14:dataBar></x14:cfRule>`]),
      ] }]);
    }
    case 3: { // icon sets: every set, reverse, showValue 0, gte, NoIcons, unknown set, cfvo types
      const sets = ['3Arrows', '3ArrowsGray', '3Flags', '3TrafficLights1', '3TrafficLights2', '3Signs', '3Symbols', '3Symbols2', '4Arrows', '4ArrowsGray', '4RedToBlack', '4Rating', '4TrafficLights', '5Arrows', '5ArrowsGray', '5Rating', '5Quarters', 'NoIcons', 'Weird'];
      const v = [0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 'x', undefined, 3.3, 6.6];
      const rows = v.map((x) => sets.map(() => x));
      const rules = sets.map((s, k) => {
        const n = parseInt(s, 10) || 3;
        const ty = ['percent', 'num', 'percentile', 'formula'][k % 4];
        const vals = Array.from({ length: n }, (_, i) => (ty === 'formula' ? `$${colName(k)}$${i + 2}` : String(ty === 'num' ? i * 3 : Math.round((100 * i) / n))));
        return cf(`${colName(k)}1:${colName(k)}15`, [rule({ type: 'iconSet', priority: k + 1 }, `<iconSet${attrs({ iconSet: s, reverse: k % 3 === 0 ? 1 : undefined, showValue: k % 5 === 0 ? 0 : undefined })}>${vals.map((vv, i) => cfvo(ty, vv, i === 1 && k % 2 ? '0' : undefined)).join('')}</iconSet>`)]);
      });
      rules.push(cf('A1:A15', [rule({ type: 'iconSet', priority: 50 }, '')]));
      return book([{ name: 'icons', rows, cf: rules, x14: [
        x14cf('B1:C15', [`<x14:cfRule type="iconSet" priority="40" id="${guid()}"><x14:iconSet iconSet="3Stars" custom="1"><x14:cfvo type="percent"><xm:f>0</xm:f></x14:cfvo><x14:cfvo type="percent"><xm:f>33</xm:f></x14:cfvo><x14:cfvo type="percent"><xm:f>67</xm:f></x14:cfvo><x14:cfIcon iconSet="NoIcons" iconId="0"/><x14:cfIcon iconSet="3Flags" iconId="1"/><x14:cfIcon iconSet="5Boxes" iconId="4"/></x14:iconSet></x14:cfRule>`]),
        x14cf('D1:D15', [`<x14:cfRule type="iconSet" priority="0" id="${guid()}"><x14:iconSet iconSet="5Boxes" reverse="1"><x14:cfvo type="percent"><xm:f>0</xm:f></x14:cfvo><x14:cfvo type="percent"><xm:f>20</xm:f></x14:cfvo><x14:cfvo type="percent"><xm:f>40</xm:f></x14:cfvo><x14:cfvo type="percent"><xm:f>60</xm:f></x14:cfvo><x14:cfvo type="percent"><xm:f>80</xm:f></x14:cfvo></x14:iconSet></x14:cfRule>`]),
        x14cf('E1:E15', [`<x14:cfRule type="iconSet" priority="41" id="${guid()}"><x14:iconSet iconSet="3Triangles" showValue="0"><x14:cfvo type="num"><xm:f>0</xm:f></x14:cfvo><x14:cfvo type="num" gte="0"><xm:f>3</xm:f></x14:cfvo><x14:cfvo type="formula"><xm:f>$E$9</xm:f></x14:cfvo></x14:iconSet></x14:cfRule>`]),
      ] }]);
    }
    case 4: { // top / bottom N and %: ties, rank 0, rank > n, small percents, text and blanks in range
      const v = [5, 5, 5, 3, 3, 9, 9, 1, 'x', undefined, 7, 7, 2, 10, 10];
      const t = (a, d) => rule({ type: 'top10', dxfId: d, ...a });
      return book([{ name: 'top', rows: v.map((x) => [x, x, x, x, x, x]), cf: [
        cf('A1:A15', [t({ priority: 1, rank: 3 }, 0)]), cf('B1:B15', [t({ priority: 2, rank: 2, bottom: 1 }, 1)]),
        cf('C1:C15', [t({ priority: 3, rank: 10, percent: 1 }, 0)]), cf('D1:D15', [t({ priority: 4, rank: 1, percent: 1, bottom: 1 }, 1)]),
        cf('E1:E15', [t({ priority: 5, rank: 0 }, 2), t({ priority: 6, rank: 99, bottom: 1 }, 3)]), cf('F1:F15', [t({ priority: 7, rank: 'x' }, 0), t({ priority: 8, percent: 1, rank: 50 }, 5)]),
      ] }]);
    }
    case 5: { // above/below average, equalAverage, standard deviations, all equal
      const v = [1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 5.5, 'x', undefined, 100];
      const a = (o, d) => rule({ type: 'aboveAverage', dxfId: d, ...o });
      return book([{ name: 'avg', rows: v.map((x) => [x, x, x, x, 4, x]), cf: [
        cf('A1:A14', [a({ priority: 1 }, 0)]), cf('B1:B14', [a({ priority: 2, aboveAverage: 0 }, 1)]), cf('C1:C14', [a({ priority: 3, equalAverage: 1, aboveAverage: 0 }, 2)]),
        cf('D1:D14', [a({ priority: 4, stdDev: 1 }, 0), a({ priority: 5, stdDev: 2, aboveAverage: 0 }, 1)]), cf('E1:E14', [a({ priority: 6, equalAverage: 1 }, 3)]),
        cf('F1:F3 F10:F14', [a({ priority: 7, stdDev: 'x' }, 5)]),
      ] }]);
    }
    case 6: { // duplicates / unique: numbers, case-insensitive text, booleans, blanks, errors, 1 vs "1"
      const v = [1, 1, 2, 'a', 'A', 'b', true, true, false, '', undefined, { e: '#N/A' }, { e: '#N/A' }, '1', 1.0, { is: 'b' }, ' a'];
      return book([{ name: 'dup', rows: v.map((x) => [x, x]), cf: [
        cf('A1:A17', [rule({ type: 'duplicateValues', dxfId: 0, priority: 1 })]),
        cf('B1:B17', [rule({ type: 'uniqueValues', dxfId: 1, priority: 2 })]),
        cf('A1:B17', [rule({ type: 'duplicateValues', dxfId: 2, priority: 3 })]),
        cf('A1:A5 A3:A9', [rule({ type: 'uniqueValues', dxfId: 3, priority: 4 })]),
      ] }]);
    }
    case 7: { // contains / not contains / begins / ends: with and without formula, numbers as text, errors, case
      const v = ['alfa', 'ALFAbeto', 'beta alfa', 1.5, 15, true, { e: '#DIV/0!' }, undefined, '', 'Ação', 'açÃo', 'xa*b', 'a?c'];
      const t = (type, text, p, f) => rule({ type, dxfId: p % DXFS.length, operator: type, text, priority: p }, f ? fx(f) : '');
      return book([{ name: 'text', rows: v.map((x) => [x, x, x, x, x, x]), cf: [
        cf('A1:A13', [t('containsText', 'ALF', 1)]), cf('B1:B13', [t('notContainsText', 'a', 2)]), cf('C1:C13', [t('beginsWith', 'AÇ', 3)]),
        cf('D1:D13', [t('endsWith', 'O', 4), t('containsText', '1,5', 5)]), cf('E1:E13', [t('containsText', 'a', 6, 'NOT(ISERROR(SEARCH("a?",E1)))')]),
        cf('F1:F13', [t('beginsWith', 'x', 7, 'LEFT(F1,1)="x"'), t('endsWith', '', 8), t('containsText', 'b', 9, 'BOGUS(F1)')]),
      ] }]);
    }
    case 8: { // blanks / errors
      const v = [undefined, '', '  ', '\t', 0, 'x', { e: '#N/A' }, { e: '#DIV/0!' }, false, { is: '' }, { raw: '' }, { f: 'A1', t: 'str', v: '' }, { raw: null, t: 'e' }];
      return book([{ name: 'blanks', rows: v.map((x) => [x, x, x, x]), cf: [
        cf('A1:A14', [rule({ type: 'containsBlanks', dxfId: 0, priority: 1 }, fx('LEN(TRIM(A1))=0'))]),
        cf('B1:B14', [rule({ type: 'notContainsBlanks', dxfId: 1, priority: 2 })]),
        cf('C1:C14', [rule({ type: 'containsErrors', dxfId: 2, priority: 3 })]),
        cf('D1:D14', [rule({ type: 'notContainsErrors', dxfId: 3, priority: 4 })]),
      ] }]);
    }
    case 9: case 10: { // time periods around 2025-01-15 (1900 and 1904 dates)
      const d1904 = i === 10;
      const off = d1904 ? 1462 : 0;
      const days = [];
      for (let k = -50; k <= 50; k += 1) if (k % 3 === 0 || Math.abs(k) <= 9) days.push(TODAY + k - off);
      days.push(TODAY - off + 0.75, 'x', undefined, -1);
      const periods = ['today', 'yesterday', 'tomorrow', 'last7Days', 'thisWeek', 'lastWeek', 'nextWeek', 'thisMonth', 'lastMonth', 'nextMonth'];
      return book([{ name: 'time', rows: days.map((x) => periods.map(() => (typeof x === 'number' ? { s: 1, n: x } : x))), cf: periods.map((p, k) => cf(`${colName(k)}1:${colName(k)}${days.length}`, [rule({ type: 'timePeriod', dxfId: k % DXFS.length, timePeriod: p, priority: k + 1 })])) }], { date1904: d1904 });
    }
    case 11: { // priorities, stopIfTrue, overlapping dxfs, a format with several ranges, rules sharing a priority
      const v = grid(new Rng(11), 12, 4, ['i', 'i', 'n', 't', 'blank']);
      return book([{ name: 'prio', rows: v, cf: [
        cf('A1:D12', [rule({ type: 'cellIs', dxfId: 2, priority: 3, operator: 'greaterThan' }, fx('50'))]),
        cf('A1:B6 C7:D12', [rule({ type: 'expression', dxfId: 0, priority: 1, stopIfTrue: 1 }, fx('A1>90')), rule({ type: 'expression', dxfId: 1, priority: 2 }, fx('A1>20'))]),
        cf('A1:D12', [rule({ type: 'colorScale', priority: 2 }, `<colorScale>${cfvo('min')}${cfvo('max')}${color('FFFFFF')}${color('FF0000')}</colorScale>`)]),
        cf('B1:C12', [rule({ type: 'expression', dxfId: 4, priority: 4 }, fx('TRUE')), rule({ type: 'expression', dxfId: 3, priority: 5, stopIfTrue: 1 }, fx('B1<0')), rule({ type: 'cellIs', dxfId: 7, priority: 6, operator: 'lessThan' }, fx('10'))]),
        cf('D1:D12', [rule({ type: 'dataBar', priority: 0, stopIfTrue: 1 }, `<dataBar><cfvo type="min"/><cfvo type="max"/>${color('638EC6')}</dataBar>`), rule({ type: 'iconSet', priority: 7 }, `<iconSet iconSet="3Arrows">${cfvo('percent', '0')}${cfvo('percent', '33')}${cfvo('percent', '67')}</iconSet>`)]),
        cf('A12:D12', [rule({ type: 'expression', dxfId: 5, priority: 8 }, fx('1'))]),
      ] }]);
    }
    case 12: { // cross-sheet formulas, sheet names with spaces/quotes/accents, whole rows/columns
      const v = grid(new Rng(12), 10, 3, ['i', 'n']);
      return book([
        { name: 'Main', rows: v, cf: [cf('A1:C10', [
          rule({ type: 'expression', dxfId: 0, priority: 1 }, fx("A1>'Outra Aba'!$A$1")), rule({ type: 'expression', dxfId: 1, priority: 2 }, fx('A1=Ação!A1')),
          rule({ type: 'expression', dxfId: 2, priority: 3 }, fx("COUNTIF('O''Brien'!$A:$A,A1)>0")), rule({ type: 'expression', dxfId: 3, priority: 4 }, fx('SUM(Main!$1:$1)>A1')),
          rule({ type: 'expression', dxfId: 4, priority: 5 }, fx('A1>MAX(outra aba!A1)')), rule({ type: 'expression', dxfId: 5, priority: 6 }, fx('Missing!A1=1')),
        ])] },
        { name: 'Outra Aba', rows: [[50], [3]] }, { name: 'Ação', rows: grid(new Rng(13), 10, 3, ['i']) }, { name: "O'Brien", rows: N([1, 5, 10, 'x', 17, 22]) },
      ]);
    }
    case 13: { // dxf number formats over numbers / text / dates, x14 rule with own dxf, numFmt by builtin id
      const v = [1.5, -2, 'texto', 45672, 0.25, true, undefined, 1234.5678];
      return book([{ name: 'numfmt', rows: v.map((x) => [x, x, typeof x === 'number' ? { s: 2, n: x } : x, x]), cf: [
        cf('A1:A8', [rule({ type: 'expression', dxfId: 3, priority: 1 }, fx('TRUE'))]),
        cf('B1:B8', [rule({ type: 'expression', dxfId: 5, priority: 2 }, fx('ISNUMBER(B1)')), rule({ type: 'expression', dxfId: 7, priority: 3 }, fx('1'))]),
        cf('C1:C8', [rule({ type: 'expression', dxfId: 7, priority: 4 }, fx('C1<>0'))]),
      ], x14: [x14cf('D1:D8', [`<x14:cfRule type="expression" priority="5" id="${guid()}"><xm:f>D1&gt;1</xm:f><x14:dxf><numFmt numFmtId="14" formatCode="dd/mm/yyyy"/><font><i/></font></x14:dxf></x14:cfRule>`])] }]);
    }
    case 14: { // cf-formula functions over mixed data (one expression per column)
      const data = grid(new Rng(14), 30, 3, ['i', 'n', 't', 'b', 'e', 'blank', 'd']);
      const fs = FORMULAS.slice(0, 48);
      return book([{ name: 'funcs', rows: data, cf: fs.map((f, k) => cf(`${colName(k % 3)}1:${colName(k % 3)}30`, [rule({ type: 'expression', dxfId: k % DXFS.length, priority: k + 1 }, fx(f))])) }]);
    }
    case 15: { // more functions + odd syntax (unsupported, errors, ranges as scalars, row/column ranges)
      const data = grid(new Rng(15), 30, 3, ['i', 'n', 't', 'b', 'e', 'blank', 'd', 'raw', 'iso', 'f', 'fs', 'inline']);
      const fs = FORMULAS.slice(48);
      return book([{ name: 'funcs2', rows: data, cf: fs.map((f, k) => cf(`${colName(k % 3)}1:${colName(k % 3)}30`, [rule({ type: 'expression', dxfId: k % DXFS.length, priority: k + 1 }, fx(f))])) }, { name: 'Outra', rows: [[5], [6]] }]);
    }
    default: return undefined;
  }
}

/** One workbook of a group. */
export function buildCfWorkbook(group, i) {
  const g = CF_GROUPS[group];
  GUID = (g.seedBase + i) * 1000;
  if (g.fixed) { const b = fixed(i); if (b) return { bytes: b, name: cfFile(group, i) }; }
  const R = new Rng(g.seedBase + i * 7919);
  const rows = R.int(g.rows[0], g.rows[1]);
  const cols = R.int(2, 6);
  const kinds = R.pick([['i'], ['i', 'n'], ['i', 'n', 't', 'blank'], ['i', 'n', 't', 'b', 'e', 'blank', 'd', 'raw', 'iso', 'f', 'fs', 'inline'], ['t', 'blank', 'inline'], ['d', 'i']]);
  const sheets = [];
  const nsheets = R.chance(0.2) ? 2 : 1;
  for (let s = 0; s < nsheets; s++) {
    const { cfs, x14 } = randomRules(R, rows, cols, s * 50);
    sheets.push({ name: s === 0 ? 'Plan1' : 'Outra Aba', rows: grid(R, rows, cols, kinds), cf: cfs, x14 });
  }
  return { bytes: book(sheets, { date1904: R.chance(0.1) }), name: cfFile(group, i) };
}
