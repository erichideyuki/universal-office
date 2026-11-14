// WordCraft study (2026-11-14), implementer "tables" — the open lic-tight-*/lic-through-* cases of wctablic: a
// tight/through box (rectangular wrap polygon) in a cell lets a line whose band overlaps the box's lower part run
// beside it at full width, while a square box blocks it. Probe: the box's vertical offset swept in 2-pt steps, square
// vs tight vs through, in a cell (the wctablic table) and in the body. Liberation Sans 11, mode 15.
import { buildDocx, docDefaults, p, r } from '../docx-builder.mjs';
import { tbx } from './_tbx.mjs';

const D = (body) => buildDocx({ body, compat: 15, defaults: docDefaults({ font: 'Liberation Sans', size: 11 }) });
const NOLN = '<a:ln><a:noFill/></a:ln>';
const poly = '<wp:wrapPolygon edited="0"><wp:start x="0" y="0"/><wp:lineTo x="0" y="21600"/><wp:lineTo x="21600" y="21600"/><wp:lineTo x="21600" y="0"/><wp:lineTo x="0" y="0"/></wp:wrapPolygon>';
const box = ({ x, wrap, y }) => {
  let xml = tbx({ i: 1, ins: [0, 0, 0, 0], content: p(r('Caixa Hxg')), wrap: wrap === 'wrapSquare' ? wrap : 'wrapNone', x, y, w: 80, h: 30, relH: 'column', relV: 'paragraph', line: NOLN });
  if (wrap === 'wrapTight' || wrap === 'wrapThrough') xml = xml.replace('<wp:wrapNone/>', `<wp:${wrap} wrapText="bothSides">${poly}</wp:${wrap}>`);
  return xml;
};
const bd = '<w:tblBorders>' + ['top', 'left', 'bottom', 'right', 'insideH', 'insideV'].map((s) => `<w:${s} w:val="single" w:sz="4" w:space="0" w:color="auto"/>`).join('') + '</w:tblBorders>';
const paras = (a) => p(a + r('Celula um Hxg')) + p(r('segunda Hxg')) + p(r('terceira Hxg')) + p(r('quarta Hxg')) + p(r('quinta Hxg'));
const table = (a) => {
  const tc = (inner) => `<w:tc><w:tcPr><w:tcW w:w="3000" w:type="dxa"/></w:tcPr>${inner}</w:tc>`;
  return `<w:tbl><w:tblPr><w:tblW w:w="0" w:type="auto"/><w:tblInd w:w="1000" w:type="dxa"/>${bd}</w:tblPr><w:tblGrid><w:gridCol w:w="3000"/><w:gridCol w:w="3000"/></w:tblGrid>`
    + `<w:tr>${tc(paras(a))}${tc(p(r('Celula dois Hxg')))}</w:tr></w:tbl>`;
};

export const cases = [];
const W = { sq: 'wrapSquare', tt: 'wrapTight', th: 'wrapThrough' };
for (const [k, wrap] of Object.entries(W)) {
  for (const y of [6, 8, 10, 12, 14, 16, 18, 20]) {
    // in a cell: the box at the cell's right (x 64 pt from the cell's text: 140.66+64 = 204.7, inside the interior)
    cases.push({ name: `tt-cell-${k}-y${y}`, docx: D(p(r('Antes Hxg')) + table(box({ x: 64, wrap, y })) + p(r('Depois Hxg'))) });
    // in the body: the box 64 pt into a narrow paragraph's column (right indent leaves 150 pt of text width)
    if (k !== 'th') {
      const ind = '<w:pPr><w:ind w:right="5504"/></w:pPr>';
      const bp = (a, t) => `<w:p>${ind}${a}${r(t)}</w:p>`;
      cases.push({ name: `tt-body-${k}-y${y}`, docx: D(p(r('Antes Hxg')) + bp(box({ x: 64, wrap, y }), 'Celula um Hxg') + bp('', 'segunda Hxg') + bp('', 'terceira Hxg') + bp('', 'quarta Hxg') + bp('', 'quinta Hxg') + p(r('Depois Hxg'))) });
    }
  }
}
