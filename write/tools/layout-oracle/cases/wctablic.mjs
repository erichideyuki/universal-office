// WordCraft study (2026-11-13), planner "tables" — B15 completed (core/target/wordcraft-study/tables/wctablic).
// B/wcbflt + wcbflt2: with layoutInCell="1" Word clamps a WRAPPING box (square, topAndBottom) horizontally into its
// cell; wrapNone is not clamped; a box wider than the cell is not clamped. Not pinned down: wrapTight / wrapThrough,
// the exact clamp edges against the cell's border width and margins (the box there had a 0.5 pt outline, which hides
// the edge), mode 14's left edge, a box in the SECOND cell, behindDoc with wrapping, and vertical clamping.
// The box here has NO outline and no insets, so its text x is the box x (+ the glyph offset of a control box).
// Table: tblInd 1000 tw, 2 cells of 3000 tw; the box 80 × 30 pt anchored in cell 1 (or 2), relativeFrom column.
// Liberation Sans 11.
import { buildDocx, docDefaults, p, r } from '../docx-builder.mjs';
import { tbx } from './_tbx.mjs';

const D = (body, compat) => buildDocx({ body, compat, defaults: docDefaults({ font: 'Liberation Sans', size: 11 }) });
const NOLN = '<a:ln><a:noFill/></a:ln>';
const poly = '<wp:wrapPolygon edited="0"><wp:start x="0" y="0"/><wp:lineTo x="0" y="21600"/><wp:lineTo x="21600" y="21600"/><wp:lineTo x="21600" y="0"/><wp:lineTo x="0" y="0"/></wp:wrapPolygon>';
const box = ({ x, wrap, w = 80, i = 1, behind = false, y = 14 }) => {
  let xml = tbx({ i, ins: [0, 0, 0, 0], content: p(r('Caixa Hxg')), wrap: wrap === 'wrapSquare' || wrap === 'wrapTopAndBottom' ? wrap : 'wrapNone', x, y, w, h: 30, relH: 'column', relV: 'paragraph', line: NOLN });
  if (wrap === 'wrapTight' || wrap === 'wrapThrough') xml = xml.replace('<wp:wrapNone/>', `<wp:${wrap} wrapText="bothSides">${poly}</wp:${wrap}>`);
  if (behind) xml = xml.replace('behindDoc="0"', 'behindDoc="1"');
  return xml;
};
const bd = (sz) => (sz ? '<w:tblBorders>' + ['top', 'left', 'bottom', 'right', 'insideH', 'insideV'].map((s) => `<w:${s} w:val="single" w:sz="${sz}" w:space="0" w:color="auto"/>`).join('') + '</w:tblBorders>' : '');
const table = ({ a = '', b = '', sz = 4, mar = null }) => {
  const tc = (inner) => `<w:tc><w:tcPr><w:tcW w:w="3000" w:type="dxa"/></w:tcPr>${inner}</w:tc>`;
  const cm = mar === null ? '' : `<w:tblCellMar><w:left w:w="${mar}" w:type="dxa"/><w:right w:w="${mar}" w:type="dxa"/></w:tblCellMar>`;
  return `<w:tbl><w:tblPr><w:tblW w:w="0" w:type="auto"/><w:tblInd w:w="1000" w:type="dxa"/>${bd(sz)}${cm}</w:tblPr><w:tblGrid><w:gridCol w:w="3000"/><w:gridCol w:w="3000"/></w:tblGrid>`
    + `<w:tr>${tc(p(a + r('Celula um Hxg')) + p(r('segunda Hxg')) + p(r('terceira Hxg')) + p(r('quarta Hxg')))}${tc(p(b + r('Celula dois Hxg')) + p(r('segunda dois Hxg')) + p(r('terceira dois Hxg')))}</w:tr></w:tbl>`;
};

export const cases = [];
for (const compat of [15, 14]) {
  const s = compat === 15 ? '' : '-c14';
  const add = (name, t) => cases.push({ name: `${name}${s}`, docx: D(p(r('Antes Hxg')) + table(t) + p(r('Depois Hxg')), compat) });
  // the clamp edges against the border width (none / 0.5 / 3 pt) and the margins (default 108 / 0)
  for (const sz of [0, 4, 24]) {
    for (const mar of [null, 0]) {
      const m = mar === null ? 'mdef' : 'm0';
      add(`lic-sq-r-sz${sz}-${m}`, { a: box({ x: 300, wrap: 'wrapSquare' }), sz, mar });
      add(`lic-sq-l-sz${sz}-${m}`, { a: box({ x: -60, wrap: 'wrapSquare' }), sz, mar });
      add(`lic-ctl-sz${sz}-${m}`, { a: box({ x: 20, wrap: 'wrapSquare' }), sz, mar }); // unclamped control
    }
  }
  // tight / through (rectangular polygon), right and left
  for (const wrap of ['wrapTight', 'wrapThrough']) {
    const w = wrap === 'wrapTight' ? 'tight' : 'through';
    add(`lic-${w}-r`, { a: box({ x: 300, wrap }) });
    add(`lic-${w}-l`, { a: box({ x: -60, wrap }) });
  }
  // a box in the SECOND cell pushed right (clamped into cell 2?) and left (into cell 2, not cell 1?)
  add('lic-cell2-r', { b: box({ x: 300, wrap: 'wrapSquare' }) });
  add('lic-cell2-l', { b: box({ x: -200, wrap: 'wrapSquare' }) });
  // behindDoc with wrapSquare (mode 15 puts it in front), and a wrapNone control far right
  add('lic-behind-sq-r', { a: box({ x: 300, wrap: 'wrapSquare', behind: true }) });
  add('lic-none-r', { a: box({ x: 300, wrap: 'wrapNone' }) });
  // vertical: a square box pushed far DOWN (y 200 pt) — clamped into the row, or the row grows?
  add('lic-sq-down', { a: box({ x: 20, wrap: 'wrapSquare', y: 200 }) });
  // exactly as wide as the cell text area + margins (150 pt): clamped or not?
  add('lic-sq-w150', { a: box({ x: 40, wrap: 'wrapSquare', w: 150 }) });
  add('lic-sq-w140', { a: box({ x: 40, wrap: 'wrapSquare', w: 140 }) });
}
