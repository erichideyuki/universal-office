// Seeded deterministic generator of DRAWING workbooks (frontier C4b, record core/docs/modules/uo-xlsx.md "C4b — as built"):
// the objects of `xl/drawings/drawingN.xml` the viewport paints — shapes (presets, fills, outlines, text with runs and
// paragraphs, rotation in every swap band, flips), text boxes, connectors, pictures (PNG/JPEG, crops), groups (with and without
// chOff/chExt), charts of every common family (bar/column clustered and stacked, line with markers, pie, doughnut, area, scatter,
// radar) with titles, legends, data labels and references to other sheets, defined names, hidden rows/columns (plotVisOnly on
// and off), multi-level categories, an external reference and a missing sheet; a chart inside a group; the three anchor kinds
// with offsets past the cell, negative offsets, hidden rows/columns under the anchor; form controls (check box, option button,
// command button, group box); frozen panes that cut objects; a chart sheet (zoom and zoomToFit); workbooks without a theme part
// (the Office 2023 rule of the charts) and written by Excel 2007 (`AppVersion` 12). Pure Node: same group + index -> same bytes.
//
//   buildDrawingWorkbook(group, i) -> { bytes, name }     groups: dr1 (CI, committed), dr-big (local)
import { deflateSync } from 'node:zlib';
import { Rand } from '../../../uo-pptx/js/lib/gen/rand.mjs';
import { zipBytes } from '../../../uo-pptx/js/lib/zip.mjs';

export const DRAW_GROUPS = {
  dr1: { seedBase: 0xd4a100, count: 16 },
  'dr-big': { seedBase: 0xd4b000, count: 160 },
};
export const drawKey = (g, i) => `gen/${g}/${String(i).padStart(4, '0')}`;
export const drawFile = (g, i) => `${g}-${String(i).padStart(4, '0')}.xlsx`;

const NS = 'http://schemas.openxmlformats.org/spreadsheetml/2006/main';
const NSR = 'http://schemas.openxmlformats.org/officeDocument/2006/relationships';
const REL = 'http://schemas.openxmlformats.org/officeDocument/2006/relationships';
const PKG_REL = 'http://schemas.openxmlformats.org/package/2006/relationships';
const XDR = 'http://schemas.openxmlformats.org/drawingml/2006/spreadsheetDrawing';
const A = 'http://schemas.openxmlformats.org/drawingml/2006/main';
const CNS = 'http://schemas.openxmlformats.org/drawingml/2006/chart';
const XML = '<?xml version="1.0" encoding="UTF-8" standalone="yes"?>\n';
const esc = (s) => String(s).replace(/&/g, '&amp;').replace(/</g, '&lt;').replace(/>/g, '&gt;').replace(/"/g, '&quot;');
const colName = (c) => { let s = ''; for (c += 1; c > 0; c = Math.floor((c - 1) / 26)) s = String.fromCharCode(65 + ((c - 1) % 26)) + s; return s; };
const qs = (n) => (/^[A-Za-z_][A-Za-z0-9_]*$/.test(n) ? n : `'${n.replace(/'/g, "''")}'`);

const THEME = `${XML}<a:theme xmlns:a="${A}" name="Office Theme"><a:themeElements><a:clrScheme name="Office"><a:dk1><a:sysClr val="windowText" lastClr="000000"/></a:dk1><a:lt1><a:sysClr val="window" lastClr="FFFFFF"/></a:lt1><a:dk2><a:srgbClr val="44546A"/></a:dk2><a:lt2><a:srgbClr val="E7E6E6"/></a:lt2><a:accent1><a:srgbClr val="4472C4"/></a:accent1><a:accent2><a:srgbClr val="ED7D31"/></a:accent2><a:accent3><a:srgbClr val="A5A5A5"/></a:accent3><a:accent4><a:srgbClr val="FFC000"/></a:accent4><a:accent5><a:srgbClr val="5B9BD5"/></a:accent5><a:accent6><a:srgbClr val="70AD47"/></a:accent6><a:hlink><a:srgbClr val="0563C1"/></a:hlink><a:folHlink><a:srgbClr val="954F72"/></a:folHlink></a:clrScheme><a:fontScheme name="Office"><a:majorFont><a:latin typeface="Calibri Light"/><a:ea typeface=""/><a:cs typeface=""/></a:majorFont><a:minorFont><a:latin typeface="Calibri"/><a:ea typeface=""/><a:cs typeface=""/></a:minorFont></a:fontScheme><a:fmtScheme name="Office"><a:fillStyleLst><a:solidFill><a:schemeClr val="phClr"/></a:solidFill><a:gradFill rotWithShape="1"><a:gsLst><a:gs pos="0"><a:schemeClr val="phClr"><a:lumMod val="110000"/><a:satMod val="105000"/><a:tint val="67000"/></a:schemeClr></a:gs><a:gs pos="100000"><a:schemeClr val="phClr"><a:lumMod val="105000"/><a:satMod val="109000"/><a:tint val="81000"/></a:schemeClr></a:gs></a:gsLst><a:lin ang="5400000" scaled="0"/></a:gradFill><a:solidFill><a:schemeClr val="phClr"><a:shade val="80000"/></a:schemeClr></a:solidFill></a:fillStyleLst><a:lnStyleLst><a:ln w="6350"><a:solidFill><a:schemeClr val="phClr"/></a:solidFill></a:ln><a:ln w="12700"><a:solidFill><a:schemeClr val="phClr"/></a:solidFill></a:ln><a:ln w="19050"><a:solidFill><a:schemeClr val="phClr"/></a:solidFill></a:ln></a:lnStyleLst><a:effectStyleLst><a:effectStyle><a:effectLst/></a:effectStyle><a:effectStyle><a:effectLst/></a:effectStyle><a:effectStyle><a:effectLst><a:outerShdw blurRad="57150" dist="19050" dir="5400000" algn="ctr" rotWithShape="0"><a:srgbClr val="000000"><a:alpha val="63000"/></a:srgbClr></a:outerShdw></a:effectLst></a:effectStyle></a:effectStyleLst><a:bgFillStyleLst><a:solidFill><a:schemeClr val="phClr"/></a:solidFill><a:solidFill><a:schemeClr val="phClr"/></a:solidFill><a:solidFill><a:schemeClr val="phClr"/></a:solidFill></a:bgFillStyleLst></a:fmtScheme></a:themeElements></a:theme>`;

// ------------------------------------------------------------------------------------------------ pictures
const CRC = (() => { const t = new Uint32Array(256); for (let n = 0; n < 256; n++) { let c = n; for (let k = 0; k < 8; k++) c = c & 1 ? 0xedb88320 ^ (c >>> 1) : c >>> 1; t[n] = c >>> 0; } return t; })();
const crc32 = (b) => { let c = 0xffffffff; for (const x of b) c = CRC[(c ^ x) & 0xff] ^ (c >>> 8); return (c ^ 0xffffffff) >>> 0; };
function chunk(type, data) {
  const len = Buffer.alloc(4); len.writeUInt32BE(data.length);
  const td = Buffer.concat([Buffer.from(type, 'latin1'), data]);
  const crc = Buffer.alloc(4); crc.writeUInt32BE(crc32(td));
  return Buffer.concat([len, td, crc]);
}
/** A small RGB PNG of `w` x `h` with a seeded pattern. */
function png(R, w, h) {
  const ihdr = Buffer.alloc(13);
  ihdr.writeUInt32BE(w, 0); ihdr.writeUInt32BE(h, 4); ihdr[8] = 8; ihdr[9] = 2;
  const raw = Buffer.alloc((w * 3 + 1) * h);
  const c1 = [R.int(0, 255), R.int(0, 255), R.int(0, 255)], c2 = [R.int(0, 255), R.int(0, 255), R.int(0, 255)];
  for (let y = 0; y < h; y++) for (let x = 0; x < w; x++) { const c = ((x >> 2) + (y >> 2)) & 1 ? c1 : c2; raw.set(c, y * (w * 3 + 1) + 1 + x * 3); }
  return Buffer.concat([Buffer.from([0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a]), chunk('IHDR', ihdr), chunk('IDAT', deflateSync(raw)), chunk('IEND', Buffer.alloc(0))]);
}
/** A JPEG header with the SOF0 size (the content is never decoded by the parity harness: images are fake-decoded by size). */
function jpeg(w, h, tag) {
  return Buffer.from([0xff, 0xd8, 0xff, 0xe0, 0, 16, 0x4a, 0x46, 0x49, 0x46, 0, 1, 1, 0, 0, 1, 0, 1, 0, 0,
    0xff, 0xc0, 0, 11, 8, h >> 8, h & 255, w >> 8, w & 255, 1, 1, 0x11, 0, 0xff, 0xd9, tag & 255]);
}

// ------------------------------------------------------------------------------------------------ the builder
class DrawBook {
  constructor(R, i) {
    this.R = R; this.i = i;
    this.parts = []; this.overrides = []; this.defaults = new Set();
    this.sheets = []; this.names = [];
    this.chartSeq = 0; this.drawSeq = 0; this.mediaSeq = 0; this.ctrlSeq = 0;
    this.theme = !(i % 5 === 3);
    this.app12 = i % 7 === 5;
  }

  /** The data sheet: categories, numbers, dates, booleans, texts; hidden rows/cols, defined names. */
  dataSheet() {
    const R = this.R;
    const name = R.pick(['Dados', 'Data 1', 'Vendas', "Ano'24"]);
    const rows = [];
    const n = R.int(5, 10);
    rows.push(`<row r="1"><c r="A1" t="inlineStr"><is><t>Mês</t></is></c><c r="B1" t="inlineStr"><is><t>Série A</t></is></c><c r="C1" t="inlineStr"><is><t>Série B</t></is></c><c r="D1" t="inlineStr"><is><t>Grupo</t></is></c><c r="E1" t="inlineStr"><is><t>X</t></is></c></row>`);
    const hiddenRow = R.chance(0.4) ? R.int(3, n) : 0;
    for (let r = 2; r <= n + 1; r++) {
      const cat = R.pick(['jan', 'fev', 'mar', 'abr', 'mai', 'jun', 'jul', 'ago', 'set', 'out', 'nov', 'dez']) + r;
      const b = R.chance(0.1) ? `<c r="B${r}" t="e"><v>#N/A</v></c>` : R.chance(0.1) ? `<c r="B${r}" t="inlineStr"><is><t>n/d</t></is></c>` : `<c r="B${r}" s="${R.pick([0, 1, 2])}"><v>${R.int(-20, 120) + (R.chance(0.3) ? 0.25 : 0)}</v></c>`;
      const c = R.chance(0.1) ? `<c r="C${r}" t="b"><v>1</v></c>` : `<c r="C${r}"><v>${R.int(1, 60)}</v></c>`;
      const g = r < (n + 1) / 2 ? 'Norte' : 'Sul';
      const x = `<c r="E${r}" s="3"><v>${45000 + r * 7}</v></c>`;
      // a font with an alternate name (`scheme="minor"` over another face: the theme's minor font, `altName` Georgia)
      rows.push(`<row r="${r}"${r === hiddenRow ? ' hidden="1"' : ''}><c r="A${r}" t="inlineStr"${r % 2 ? ' s="4"' : ''}><is><t>${esc(cat)}</t></is></c>${b}${c}<c r="D${r}" t="inlineStr"><is><t>${g}</t></is></c>${x}</row>`);
    }
    const cols = R.chance(0.3) ? '<cols><col min="3" max="3" width="9" hidden="1"/></cols>' : '';
    this.names.push(`<definedName name="Serie1">${esc(qs(name))}!$B$2:$B$${n + 1}</definedName>`);
    this.names.push(`<definedName name="Cats" localSheetId="0">${esc(qs(name))}!$A$2:$A$${n + 1}</definedName>`);
    // a right-to-left sheet (the camera render mirrors it) and list validations (the dropdown of the selected cell)
    const rtl = this.i % 3 === 2 ? ' rightToLeft="1"' : '';
    const dv = this.i % 2 === 0 ? '<dataValidations count="1"><dataValidation type="list" allowBlank="1" showErrorMessage="1" sqref="A1:B3 C3 E7"><formula1>"x,y"</formula1></dataValidation></dataValidations>' : '';
    this.sheets.push({ name, kind: 'ws', xml: `${XML}<worksheet xmlns="${NS}" xmlns:r="${NSR}"><sheetViews><sheetView workbookViewId="0"${rtl}/></sheetViews><sheetFormatPr defaultRowHeight="15"/>${cols}<sheetData>${rows.join('')}</sheetData>${dv}</worksheet>`, rels: [] });
    this.data = { name, n };
  }

  anchor(kind, body, R = this.R) {
    const c = R.int(0, 6), r = R.int(0, 10);
    const off = () => R.pick([0, 0, 76200, 304800, 1900000, -95250]);
    const from = `<xdr:from><xdr:col>${c}</xdr:col><xdr:colOff>${off()}</xdr:colOff><xdr:row>${r}</xdr:row><xdr:rowOff>${off()}</xdr:rowOff></xdr:from>`;
    if (kind === 'one') return `<xdr:oneCellAnchor>${from}<xdr:ext cx="${R.int(4, 30) * 95250}" cy="${R.int(3, 20) * 95250}"/>${body}<xdr:clientData/></xdr:oneCellAnchor>`;
    if (kind === 'abs') return `<xdr:absoluteAnchor><xdr:pos x="${R.int(0, 60) * 95250}" y="${R.int(0, 40) * 95250}"/><xdr:ext cx="${R.int(4, 30) * 95250}" cy="${R.int(3, 20) * 95250}"/>${body}<xdr:clientData/></xdr:absoluteAnchor>`;
    const to = `<xdr:to><xdr:col>${c + R.int(1, 6)}</xdr:col><xdr:colOff>${off()}</xdr:colOff><xdr:row>${r + R.int(1, 9)}</xdr:row><xdr:rowOff>${off()}</xdr:rowOff></xdr:to>`;
    return `<xdr:twoCellAnchor${R.chance(0.2) ? ' editAs="oneCell"' : ''}>${from}${to}${body}<xdr:clientData/></xdr:twoCellAnchor>`;
  }

  xfrm(R = this.R, rot = true) {
    const r = rot && R.chance(0.45) ? ` rot="${R.pick([1800000, 5400000, 8100000, 14400000, 16200000, 19800000])}"` : '';
    const f = R.chance(0.15) ? ' flipH="1"' : '';
    return `<a:xfrm${r}${f}><a:off x="${R.int(0, 9) * 95250}" y="${R.int(0, 9) * 95250}"/><a:ext cx="${R.int(5, 30) * 95250}" cy="${R.int(3, 18) * 95250}"/></a:xfrm>`;
  }

  fill(R = this.R) {
    const k = R.int(0, 4);
    if (k === 0) return `<a:solidFill><a:srgbClr val="${R.hex6()}"/></a:solidFill>`;
    if (k === 1) return `<a:solidFill><a:schemeClr val="${R.pick(['accent1', 'accent2', 'accent4', 'accent6'])}"><a:lumMod val="75000"/></a:schemeClr></a:solidFill>`;
    if (k === 2) return `<a:gradFill><a:gsLst><a:gs pos="0"><a:srgbClr val="${R.hex6()}"/></a:gs><a:gs pos="100000"><a:srgbClr val="${R.hex6()}"/></a:gs></a:gsLst><a:lin ang="${R.pick([0, 2700000, 5400000])}" scaled="1"/></a:gradFill>`;
    if (k === 3) return '<a:noFill/>';
    return `<a:pattFill prst="${R.pick(['pct20', 'dkHorz', 'smCheck'])}"><a:fgClr><a:srgbClr val="${R.hex6()}"/></a:fgClr><a:bgClr><a:srgbClr val="FFFFFF"/></a:bgClr></a:pattFill>`;
  }

  line(R = this.R) {
    if (R.chance(0.2)) return '<a:ln><a:noFill/></a:ln>';
    const dash = R.chance(0.3) ? `<a:prstDash val="${R.pick(['dash', 'sysDot', 'lgDashDot'])}"/>` : '';
    return `<a:ln w="${R.pick([3175, 9525, 12700, 28575])}"><a:solidFill><a:srgbClr val="${R.hex6()}"/></a:solidFill>${dash}</a:ln>`;
  }

  txBody(R = this.R) {
    const runs = (n) => Array.from({ length: n }, () => {
      const pr = [`lang="pt-BR"`, R.chance(0.5) ? `sz="${R.pick([800, 1100, 1400, 2000])}"` : '', R.chance(0.3) ? 'b="1"' : '', R.chance(0.2) ? 'i="1"' : ''].filter(Boolean).join(' ');
      const color = R.chance(0.4) ? `<a:solidFill><a:srgbClr val="${R.hex6()}"/></a:solidFill>` : '';
      const latin = R.chance(0.3) ? `<a:latin typeface="${R.pick(['Arial', 'Liberation Serif', '+mj-lt'])}"/>` : '';
      return `<a:r><a:rPr ${pr}>${color}${latin}</a:rPr><a:t>${esc(R.pick(['Texto', 'Olá mundo', 'Área 51', 'AV To', 'Total: 1.234,56', 'Linha longa que quebra em mais de uma linha']))}</a:t></a:r>`;
    }).join('');
    const paras = Array.from({ length: R.int(1, 3) }, () => `<a:p><a:pPr algn="${R.pick(['l', 'ctr', 'r'])}"/>${runs(R.int(1, 3))}</a:p>`).join('');
    const bp = `<a:bodyPr${R.chance(0.5) ? ` anchor="${R.pick(['t', 'ctr', 'b'])}"` : ''}${R.chance(0.3) ? ' wrap="none"' : ''}${R.chance(0.2) ? ' vertOverflow="clip"' : ''}/>`;
    return `<xdr:txBody>${bp}<a:lstStyle/>${paras}</xdr:txBody>`;
  }

  sp(id, R = this.R, txBox = false) {
    const prst = R.pick(['rect', 'roundRect', 'ellipse', 'triangle', 'rightArrow', 'star5', 'wedgeRectCallout', 'can']);
    return `<xdr:sp macro="" textlink=""><xdr:nvSpPr><xdr:cNvPr id="${id}" name="Shape ${id}"${R.chance(0.05) ? ' hidden="1"' : ''}/><xdr:cNvSpPr${txBox ? ' txBox="1"' : ''}/></xdr:nvSpPr><xdr:spPr>${this.xfrm(R)}<a:prstGeom prst="${txBox ? 'rect' : prst}"><a:avLst/></a:prstGeom>${txBox ? '<a:noFill/>' : this.fill(R)}${this.line(R)}</xdr:spPr>${R.chance(0.3) && !txBox ? '<xdr:style><a:lnRef idx="2"><a:schemeClr val="accent1"><a:shade val="50000"/></a:schemeClr></a:lnRef><a:fillRef idx="1"><a:schemeClr val="accent1"/></a:fillRef><a:effectRef idx="0"><a:schemeClr val="accent1"/></a:effectRef><a:fontRef idx="minor"><a:schemeClr val="lt1"/></a:fontRef></xdr:style>' : ''}${txBox || R.chance(0.7) ? this.txBody(R) : ''}</xdr:sp>`;
  }

  cxn(id, R = this.R) {
    return `<xdr:cxnSp macro=""><xdr:nvCxnSpPr><xdr:cNvPr id="${id}" name="Connector ${id}"/><xdr:cNvCxnSpPr/></xdr:nvCxnSpPr><xdr:spPr>${this.xfrm(R, false)}<a:prstGeom prst="${R.pick(['line', 'straightConnector1', 'bentConnector3'])}"><a:avLst/></a:prstGeom><a:ln w="19050"><a:solidFill><a:srgbClr val="${R.hex6()}"/></a:solidFill><a:tailEnd type="${R.pick(['triangle', 'arrow', 'oval'])}"/></a:ln></xdr:spPr></xdr:cxnSp>`;
  }

  pic(id, rels, R = this.R) {
    const k = ++this.mediaSeq;
    const isJpeg = R.chance(0.3);
    const w = R.int(8, 40), h = R.int(8, 40);
    const part = `xl/media/image${k}.${isJpeg ? 'jpeg' : 'png'}`;
    this.parts.push([part, isJpeg ? jpeg(w * 4, h * 4, k) : png(R, w, h)]);
    this.defaults.add(isJpeg ? 'jpeg' : 'png');
    const rid = `rId${rels.length + 1}`;
    rels.push(`<Relationship Id="${rid}" Type="${REL}/image" Target="../media/image${k}.${isJpeg ? 'jpeg' : 'png'}"/>`);
    const crop = R.chance(0.3) ? '<a:srcRect l="10000" t="5000" r="20000" b="0"/>' : '';
    return `<xdr:pic><xdr:nvPicPr><xdr:cNvPr id="${id}" name="Picture ${id}" descr="img"/><xdr:cNvPicPr><a:picLocks noChangeAspect="1"/></xdr:cNvPicPr></xdr:nvPicPr><xdr:blipFill><a:blip r:embed="${rid}"/>${crop}<a:stretch><a:fillRect/></a:stretch></xdr:blipFill><xdr:spPr>${this.xfrm(R)}<a:prstGeom prst="rect"><a:avLst/></a:prstGeom>${R.chance(0.4) ? this.line(R) : ''}</xdr:spPr></xdr:pic>`;
  }

  chartFrame(id, rels, R = this.R, inGroup = false) {
    const k = this.chart(R);
    const rid = `rId${rels.length + 1}`;
    rels.push(`<Relationship Id="${rid}" Type="${REL}/chart" Target="../charts/chart${k}.xml"/>`);
    const xf = inGroup ? `<xdr:xfrm><a:off x="${R.int(0, 9) * 95250}" y="${R.int(0, 9) * 95250}"/><a:ext cx="${R.int(20, 40) * 95250}" cy="${R.int(14, 30) * 95250}"/></xdr:xfrm>` : '<xdr:xfrm><a:off x="0" y="0"/><a:ext cx="0" cy="0"/></xdr:xfrm>';
    return `<xdr:graphicFrame macro=""><xdr:nvGraphicFramePr><xdr:cNvPr id="${id}" name="Chart ${id}"/><xdr:cNvGraphicFramePr/></xdr:nvGraphicFramePr>${xf}<a:graphic><a:graphicData uri="${CNS}"><c:chart xmlns:c="${CNS}" r:id="${rid}"/></a:graphicData></a:graphic></xdr:graphicFrame>`;
  }

  /** A chart part over the data sheet; returns its number. */
  chart(R = this.R) {
    const k = ++this.chartSeq;
    const { name, n } = this.data;
    const sh = qs(name);
    const kind = R.pick(['bar', 'bar', 'line', 'pie', 'doughnut', 'area', 'scatter', 'radar']);
    const ref = (col) => {
      const v = R.int(0, 12);
      if (v === 0) return 'Serie1';
      if (v === 1) return `${sh}!$${col}$2:$${col}$${n + 1}`.replace(/^/, `(`) + `,${sh}!$${col}$${n + 1})`;
      if (v === 2) return `[1]Externa!$${col}$2:$${col}$5`;
      if (v === 3) return `Ausente!$${col}$2:$${col}$4`;
      return `${sh}!$${col}$2:$${col}$${n + 1}`;
    };
    const cache = (cnt) => `<c:numCache><c:formatCode>General</c:formatCode><c:ptCount val="${cnt}"/>${Array.from({ length: Math.min(cnt, 3) }, (_, i) => `<c:pt idx="${i}"><c:v>${(i + 1) * 7}</c:v></c:pt>`).join('')}</c:numCache>`;
    const cat = R.chance(0.2)
      ? `<c:cat><c:multiLvlStrRef><c:f>${esc(sh)}!$A$2:$D$${n + 1}</c:f></c:multiLvlStrRef></c:cat>`
      : `<c:cat><c:strRef><c:f>${esc(sh)}!$A$2:$A$${n + 1}</c:f></c:strRef></c:cat>`;
    const sers = Array.from({ length: kind === 'pie' || kind === 'doughnut' ? 1 : R.int(1, 3) }, (_, s) => {
      const col = ['B', 'C', 'B'][s];
      const tx = `<c:tx><c:strRef><c:f>${esc(sh)}!$${col}$1</c:f></c:strRef></c:tx>`;
      const sp = R.chance(0.4) ? `<c:spPr><a:solidFill><a:srgbClr val="${R.hex6()}"/></a:solidFill></c:spPr>` : '';
      const lbl = R.chance(0.3) ? '<c:dLbls><c:showLegendKey val="0"/><c:showVal val="1"/><c:showCatName val="0"/><c:showSerName val="0"/><c:showPercent val="0"/><c:showBubbleSize val="0"/></c:dLbls>' : '';
      const marker = kind === 'line' ? `<c:marker><c:symbol val="${R.pick(['circle', 'square', 'none'])}"/></c:marker>` : '';
      if (kind === 'scatter') {
        return `<c:ser><c:idx val="${s}"/><c:order val="${s}"/>${tx}${sp}<c:xVal><c:numRef><c:f>${esc(sh)}!$E$2:$E$${n + 1}</c:f>${cache(n)}</c:numRef></c:xVal><c:yVal><c:numRef><c:f>${esc(ref(col))}</c:f>${cache(n)}</c:numRef></c:yVal><c:smooth val="0"/></c:ser>`;
      }
      return `<c:ser><c:idx val="${s}"/><c:order val="${s}"/>${tx}${sp}${marker}${lbl}${cat}<c:val><c:numRef><c:f>${esc(ref(col))}</c:f>${cache(n)}</c:numRef></c:val>${kind === 'line' ? '<c:smooth val="0"/>' : ''}</c:ser>`;
    }).join('');
    const axes = '<c:axId val="10"/><c:axId val="20"/>';
    const catAx = (pos) => `<c:catAx><c:axId val="10"/><c:scaling><c:orientation val="minMax"/></c:scaling><c:delete val="0"/><c:axPos val="${pos}"/><c:numFmt formatCode="General" sourceLinked="1"/><c:tickLblPos val="nextTo"/><c:crossAx val="20"/><c:crosses val="autoZero"/></c:catAx>`;
    const valAx = (pos, id = 20, cross = 10) => `<c:valAx><c:axId val="${id}"/><c:scaling><c:orientation val="minMax"/></c:scaling><c:delete val="0"/><c:axPos val="${pos}"/>${R.chance(0.6) ? '<c:majorGridlines/>' : ''}<c:numFmt formatCode="General" sourceLinked="1"/><c:tickLblPos val="nextTo"/><c:crossAx val="${cross}"/><c:crosses val="autoZero"/></c:valAx>`;
    let plot;
    if (kind === 'bar') {
      const dir = R.pick(['col', 'bar']);
      const grouping = R.pick(['clustered', 'stacked', 'percentStacked']);
      plot = `<c:barChart><c:barDir val="${dir}"/><c:grouping val="${grouping}"/><c:varyColors val="0"/>${sers}<c:gapWidth val="${R.pick([50, 150, 219])}"/>${grouping !== 'clustered' ? '<c:overlap val="100"/>' : ''}${axes}</c:barChart>${catAx(dir === 'col' ? 'b' : 'l')}${valAx(dir === 'col' ? 'l' : 'b')}`;
    } else if (kind === 'line') {
      plot = `<c:lineChart><c:grouping val="standard"/><c:varyColors val="0"/>${sers}<c:marker val="1"/>${axes}</c:lineChart>${catAx('b')}${valAx('l')}`;
    } else if (kind === 'area') {
      plot = `<c:areaChart><c:grouping val="${R.pick(['standard', 'stacked'])}"/><c:varyColors val="0"/>${sers}${axes}</c:areaChart>${catAx('b')}${valAx('l')}`;
    } else if (kind === 'pie') {
      plot = `<c:pieChart><c:varyColors val="1"/>${sers}<c:firstSliceAng val="${R.pick([0, 90])}"/></c:pieChart>`;
    } else if (kind === 'doughnut') {
      plot = `<c:doughnutChart><c:varyColors val="1"/>${sers}<c:firstSliceAng val="0"/><c:holeSize val="${R.pick([50, 75])}"/></c:doughnutChart>`;
    } else if (kind === 'scatter') {
      plot = `<c:scatterChart><c:scatterStyle val="lineMarker"/><c:varyColors val="0"/>${sers}${axes}</c:scatterChart>${valAx('b', 10, 20)}${valAx('l', 20, 10)}`;
    } else {
      plot = `<c:radarChart><c:radarStyle val="marker"/><c:varyColors val="0"/>${sers}${axes}</c:radarChart>${catAx('b')}${valAx('l')}`;
    }
    const title = R.chance(0.5)
      ? `<c:title><c:tx><c:rich><a:bodyPr/><a:lstStyle/><a:p><a:r><a:rPr lang="pt-BR" sz="${R.pick([1200, 1400])}" b="1"/><a:t>${esc(R.pick(['Vendas por mês', 'Título', 'Receita 2024']))}</a:t></a:r></a:p></c:rich></c:tx><c:overlay val="0"/></c:title><c:autoTitleDeleted val="0"/>`
      : `<c:autoTitleDeleted val="${R.pick([0, 1])}"/>`;
    const legend = R.chance(0.6) ? `<c:legend><c:legendPos val="${R.pick(['r', 'b', 't'])}"/><c:overlay val="0"/></c:legend>` : '';
    const vis = R.chance(0.25) ? '<c:plotVisOnly val="0"/>' : R.chance(0.2) ? '' : '<c:plotVisOnly val="1"/>';
    const style = R.chance(0.3) ? `<c:style val="${R.pick([2, 18, 26, 34, 42])}"/>` : '';
    const area = R.chance(0.3) ? `<c:spPr><a:solidFill><a:srgbClr val="${R.hex6()}"/></a:solidFill><a:ln><a:solidFill><a:srgbClr val="808080"/></a:solidFill></a:ln></c:spPr>` : '';
    const txPr = R.chance(0.3) ? `<c:txPr><a:bodyPr/><a:lstStyle/><a:p><a:pPr><a:defRPr sz="${R.pick([800, 900, 1200])}"><a:latin typeface="${R.pick(['Arial', 'Liberation Sans'])}"/></a:defRPr></a:pPr><a:endParaRPr lang="pt-BR"/></a:p></c:txPr>` : '';
    this.parts.push([`xl/charts/chart${k}.xml`, `${XML}<c:chartSpace xmlns:c="${CNS}" xmlns:a="${A}" xmlns:r="${NSR}"><c:roundedCorners val="${R.pick([0, 1])}"/>${style}<c:chart>${title}<c:plotArea><c:layout/>${plot}</c:plotArea>${legend}${vis}<c:dispBlanksAs val="gap"/></c:chart>${area}${txPr}</c:chartSpace>`]);
    this.overrides.push([`/xl/charts/chart${k}.xml`, 'application/vnd.openxmlformats-officedocument.drawingml.chart+xml']);
    return k;
  }

  drawingSheet() {
    const R = this.R;
    const name = R.pick(['Desenhos', 'Painel', 'Gráficos']);
    const rels = [];
    let id = 2;
    const objs = [];
    const count = R.int(3, 7);
    for (let o = 0; o < count; o++) {
      const what = R.pick(['sp', 'sp', 'txbox', 'cxn', 'pic', 'pic', 'grp', 'chart', 'chart', 'grpchart']);
      let body;
      if (what === 'sp') body = this.sp(id++);
      else if (what === 'txbox') body = this.sp(id++, R, true);
      else if (what === 'cxn') body = this.cxn(id++);
      else if (what === 'pic') body = this.pic(id++, rels);
      else if (what === 'chart') body = this.chartFrame(id++, rels);
      else {
        const kids = [this.sp(id++), what === 'grpchart' ? this.chartFrame(id++, rels, R, true) : this.pic(id++, rels), this.sp(id++)].join('');
        const ch = R.chance(0.7) ? `<a:chOff x="0" y="0"/><a:chExt cx="${R.int(20, 40) * 95250}" cy="${R.int(14, 30) * 95250}"/>` : '';
        const rot = R.chance(0.3) ? ' rot="5400000"' : '';
        body = `<xdr:grpSp><xdr:nvGrpSpPr><xdr:cNvPr id="${id++}" name="Group"/><xdr:cNvGrpSpPr/></xdr:nvGrpSpPr><xdr:grpSpPr><a:xfrm${rot}><a:off x="0" y="0"/><a:ext cx="${R.int(20, 40) * 95250}" cy="${R.int(14, 30) * 95250}"/>${ch}</a:xfrm></xdr:grpSpPr>${kids}</xdr:grpSp>`;
      }
      const kind = what === 'chart' || what === 'grpchart' ? R.pick(['two', 'two', 'one']) : R.pick(['two', 'two', 'one', 'abs']);
      objs.push(this.anchor(kind, body));
    }
    if (R.chance(0.4)) objs.push(`<mc:AlternateContent xmlns:mc="http://schemas.openxmlformats.org/markup-compatibility/2006"><mc:Choice xmlns:a14="http://schemas.microsoft.com/office/drawing/2010/main" Requires="a14">${this.anchor('two', this.sp(id++))}</mc:Choice><mc:Fallback/></mc:AlternateContent>`);
    const k = ++this.drawSeq;
    this.parts.push([`xl/drawings/drawing${k}.xml`, `${XML}<xdr:wsDr xmlns:xdr="${XDR}" xmlns:a="${A}" xmlns:r="${NSR}">${objs.join('')}</xdr:wsDr>`]);
    if (rels.length) this.parts.push([`xl/drawings/_rels/drawing${k}.xml.rels`, `${XML}<Relationships xmlns="${PKG_REL}">${rels.join('')}</Relationships>`]);
    this.overrides.push([`/xl/drawings/drawing${k}.xml`, 'application/vnd.openxmlformats-officedocument.drawing+xml']);
    // the sheet: some cells, hidden column/row under the objects, frozen panes, form controls
    const sheetRels = [`<Relationship Id="rId1" Type="${REL}/drawing" Target="../drawings/drawing${k}.xml"/>`];
    const pane = R.chance(0.5) ? `<pane xSplit="${R.int(1, 3)}" ySplit="${R.int(1, 4)}" topLeftCell="E6" activePane="bottomRight" state="frozen"/>` : '';
    const cols = R.chance(0.5) ? `<cols><col min="2" max="2" width="${R.pick([4, 20])}" customWidth="1"/><col min="${R.int(3, 6)}" max="${R.int(6, 7)}" width="9" hidden="1"/></cols>` : '';
    const rows = [];
    for (let r = 1; r <= 14; r++) {
      const hid = R.chance(0.08) ? ' hidden="1"' : R.chance(0.1) ? ` ht="${R.pick([6, 30, 45])}" customHeight="1"` : '';
      rows.push(`<row r="${r}"${hid}><c r="A${r}"><v>${r * 3}</v></c><c r="${colName(R.int(1, 8))}${r}" t="inlineStr"><is><t>c${r}</t></is></c></row>`);
    }
    let controls = '';
    if (R.chance(0.5)) {
      const ctrls = [];
      for (const type of R.pickN(['CheckBox', 'Radio', 'Button', 'GBox'], R.int(1, 4))) {
        const c = ++this.ctrlSeq;
        const rid = `rId${sheetRels.length + 1}`;
        sheetRels.push(`<Relationship Id="${rid}" Type="http://schemas.microsoft.com/office/2006/relationships/ctrlProp" Target="../ctrlProps/ctrlProp${c}.xml"/>`);
        const checked = type === 'CheckBox' || type === 'Radio' ? R.pick(['', ' checked="Checked"', ' checked="Mixed"']) : '';
        this.parts.push([`xl/ctrlProps/ctrlProp${c}.xml`, `${XML}<formControlPr xmlns="http://schemas.microsoft.com/office/spreadsheetml/2009/9/main" objectType="${type}"${checked} lockText="1"/>`]);
        this.overrides.push([`/xl/ctrlProps/ctrlProp${c}.xml`, 'application/vnd.ms-excel.controlproperties+xml']);
        const c0 = R.int(0, 5), r0 = R.int(0, 9);
        ctrls.push(`<control shapeId="${1024 + c}" r:id="${rid}" name="${type} ${c}"><controlPr defaultSize="0" autoPict="0"><anchor moveWithCells="1"><from><xdr:col>${c0}</xdr:col><xdr:colOff>19050</xdr:colOff><xdr:row>${r0}</xdr:row><xdr:rowOff>9525</xdr:rowOff></from><to><xdr:col>${c0 + R.int(1, 3)}</xdr:col><xdr:colOff>0</xdr:colOff><xdr:row>${r0 + R.int(1, 4)}</xdr:row><xdr:rowOff>0</xdr:rowOff></to></anchor></controlPr></control>`);
      }
      controls = `<controls>${ctrls.join('')}</controls>`;
    }
    const rtl = this.i % 4 === 3 ? ' rightToLeft="1"' : '';
    this.sheets.push({ name, kind: 'ws', xml: `${XML}<worksheet xmlns="${NS}" xmlns:r="${NSR}" xmlns:xdr="${XDR}"><sheetViews><sheetView workbookViewId="0"${rtl}>${pane}</sheetView></sheetViews><sheetFormatPr defaultRowHeight="15"/>${cols}<sheetData>${rows.join('')}</sheetData><drawing r:id="rId1"/>${controls}</worksheet>`, rels: sheetRels });
  }

  chartSheet() {
    const R = this.R;
    const k = this.chart(R);
    const d = ++this.drawSeq;
    this.parts.push([`xl/drawings/drawing${d}.xml`, `${XML}<xdr:wsDr xmlns:xdr="${XDR}" xmlns:a="${A}" xmlns:r="${NSR}"><xdr:absoluteAnchor><xdr:pos x="0" y="0"/><xdr:ext cx="${R.int(60, 90) * 95250}" cy="${R.int(40, 60) * 95250}"/><xdr:graphicFrame macro=""><xdr:nvGraphicFramePr><xdr:cNvPr id="2" name="Chart 1"/><xdr:cNvGraphicFramePr/></xdr:nvGraphicFramePr><xdr:xfrm><a:off x="0" y="0"/><a:ext cx="0" cy="0"/></xdr:xfrm><a:graphic><a:graphicData uri="${CNS}"><c:chart xmlns:c="${CNS}" r:id="rId1"/></a:graphicData></a:graphic></xdr:graphicFrame><xdr:clientData/></xdr:absoluteAnchor></xdr:wsDr>`]);
    this.parts.push([`xl/drawings/_rels/drawing${d}.xml.rels`, `${XML}<Relationships xmlns="${PKG_REL}"><Relationship Id="rId1" Type="${REL}/chart" Target="../charts/chart${k}.xml"/></Relationships>`]);
    this.overrides.push([`/xl/drawings/drawing${d}.xml`, 'application/vnd.openxmlformats-officedocument.drawing+xml']);
    const view = R.chance(0.5) ? '<sheetView zoomScale="85" workbookViewId="0" zoomToFit="1"/>' : `<sheetView zoomScale="${R.pick([75, 100, 130])}" workbookViewId="0"/>`;
    this.sheets.push({ name: 'Gráfico1', kind: 'cs', xml: `${XML}<chartsheet xmlns="${NS}" xmlns:r="${NSR}"><sheetPr/><sheetViews>${view}</sheetViews><pageMargins left="0.7" right="0.7" top="0.75" bottom="0.75" header="0.3" footer="0.3"/><drawing r:id="rId1"/></chartsheet>`, rels: [`<Relationship Id="rId1" Type="${REL}/drawing" Target="../drawings/drawing${d}.xml"/>`] });
  }

  bytes() {
    const ws = [], wbRels = [];
    this.sheets.forEach((s, i) => {
      const dir = s.kind === 'cs' ? 'chartsheets' : 'worksheets';
      const part = `xl/${dir}/sheet${i + 1}.xml`;
      ws.push([part, s.xml]);
      if (s.rels.length) ws.push([`xl/${dir}/_rels/sheet${i + 1}.xml.rels`, `${XML}<Relationships xmlns="${PKG_REL}">${s.rels.join('')}</Relationships>`]);
      this.overrides.push([`/${part}`, s.kind === 'cs' ? 'application/vnd.openxmlformats-officedocument.spreadsheetml.chartsheet+xml' : 'application/vnd.openxmlformats-officedocument.spreadsheetml.worksheet+xml']);
      wbRels.push(`<Relationship Id="rId${i + 1}" Type="${REL}/${s.kind === 'cs' ? 'chartsheet' : 'worksheet'}" Target="${dir}/sheet${i + 1}.xml"/>`);
    });
    const n = this.sheets.length;
    wbRels.push(`<Relationship Id="rId${n + 1}" Type="${REL}/styles" Target="styles.xml"/>`);
    if (this.theme) wbRels.push(`<Relationship Id="rId${n + 2}" Type="${REL}/theme" Target="theme/theme1.xml"/>`);
    const styles = `${XML}<styleSheet xmlns="${NS}"><numFmts count="2"><numFmt numFmtId="164" formatCode="0.0%"/><numFmt numFmtId="165" formatCode="#,##0.00"/></numFmts><fonts count="2"><font><sz val="11"/><name val="Calibri"/><family val="2"/></font><font><sz val="12"/><name val="Georgia"/><family val="1"/><scheme val="minor"/></font></fonts><fills count="2"><fill><patternFill patternType="none"/></fill><fill><patternFill patternType="gray125"/></fill></fills><borders count="1"><border><left/><right/><top/><bottom/><diagonal/></border></borders><cellStyleXfs count="1"><xf numFmtId="0" fontId="0" fillId="0" borderId="0"/></cellStyleXfs><cellXfs count="5"><xf numFmtId="0" fontId="0" fillId="0" borderId="0" xfId="0"/><xf numFmtId="164" fontId="0" fillId="0" borderId="0" xfId="0" applyNumberFormat="1"/><xf numFmtId="165" fontId="0" fillId="0" borderId="0" xfId="0" applyNumberFormat="1"/><xf numFmtId="14" fontId="0" fillId="0" borderId="0" xfId="0" applyNumberFormat="1"/><xf numFmtId="0" fontId="1" fillId="0" borderId="0" xfId="0" applyFont="1"/></cellXfs></styleSheet>`;
    const ct = [...this.overrides, ['/xl/workbook.xml', 'application/vnd.openxmlformats-officedocument.spreadsheetml.sheet.main+xml'], ['/xl/styles.xml', 'application/vnd.openxmlformats-officedocument.spreadsheetml.styles+xml']];
    if (this.theme) ct.push(['/xl/theme/theme1.xml', 'application/vnd.openxmlformats-officedocument.theme+xml']);
    const defaults = [...this.defaults].sort().map((e) => `<Default Extension="${e}" ContentType="image/${e}"/>`).join('');
    const entries = [
      ['[Content_Types].xml', `${XML}<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/><Default Extension="xml" ContentType="application/xml"/>${defaults}${ct.map(([p, t]) => `<Override PartName="${p}" ContentType="${t}"/>`).join('')}</Types>`],
      ['_rels/.rels', `${XML}<Relationships xmlns="${PKG_REL}"><Relationship Id="rId1" Type="${REL}/officeDocument" Target="xl/workbook.xml"/><Relationship Id="rId2" Type="${REL}/extended-properties" Target="docProps/app.xml"/></Relationships>`],
      ['docProps/app.xml', `${XML}<Properties xmlns="http://schemas.openxmlformats.org/officeDocument/2006/extended-properties"><Application>Microsoft Excel</Application><AppVersion>${this.app12 ? '12.0000' : '16.0300'}</AppVersion></Properties>`],
      ['xl/workbook.xml', `${XML}<workbook xmlns="${NS}" xmlns:r="${NSR}"><workbookPr/><bookViews><workbookView activeTab="${this.sheets.length > 1 ? 1 : 0}"/></bookViews><sheets>${this.sheets.map((s, i) => `<sheet name="${esc(s.name)}" sheetId="${i + 1}" r:id="rId${i + 1}"/>`).join('')}</sheets><definedNames>${this.names.join('')}</definedNames><calcPr calcId="191029"/></workbook>`],
      ['xl/_rels/workbook.xml.rels', `${XML}<Relationships xmlns="${PKG_REL}">${wbRels.join('')}</Relationships>`],
      ['xl/styles.xml', styles],
      ...ws,
      ...this.parts,
    ];
    if (this.theme) entries.push(['xl/theme/theme1.xml', THEME]);
    return zipBytes(entries);
  }
}

export function buildDrawingWorkbook(g, i) {
  const G = DRAW_GROUPS[g];
  if (!G) throw new Error(`unknown group ${g}`);
  const R = new Rand(G.seedBase + i);
  const b = new DrawBook(R, i);
  b.dataSheet();
  b.drawingSheet();
  if (R.chance(0.4)) b.drawingSheet();
  if (i % 4 === 1 || R.chance(0.15)) b.chartSheet();
  return { bytes: b.bytes(), name: drawFile(g, i) };
}
