// WordCraft study (2026-11-14), implementer "tables" — follow-up of wctabtt: is the tight/through behaviour of an
// UNFILLED, unoutlined text box (only the line holding the box's own text blocks) the shape's visible content? The
// same box filled (solid), outlined, and with edited="1" polygons, in a cell and in the body. Liberation Sans 11, mode 15.
import { buildDocx, docDefaults, p, r } from '../docx-builder.mjs';
import { tbx } from './_tbx.mjs';

const D = (body) => buildDocx({ body, compat: 15, defaults: docDefaults({ font: 'Liberation Sans', size: 11 }) });
const NOLN = '<a:ln><a:noFill/></a:ln>';
const FILL = '<a:solidFill><a:srgbClr val="DDEEFF"/></a:solidFill>';
const poly = (ed) => `<wp:wrapPolygon edited="${ed}"><wp:start x="0" y="0"/><wp:lineTo x="0" y="21600"/><wp:lineTo x="21600" y="21600"/><wp:lineTo x="21600" y="0"/><wp:lineTo x="0" y="0"/></wp:wrapPolygon>`;
const box = ({ wrap, y, fill, line, ed = 0 }) => {
  let xml = tbx({ i: 1, ins: [0, 0, 0, 0], content: p(r('Caixa Hxg')), wrap: 'wrapNone', x: 64, y, w: 80, h: 30, relH: 'column', relV: 'paragraph', line: line ?? NOLN, fill });
  return xml.replace('<wp:wrapNone/>', `<wp:${wrap} wrapText="bothSides">${poly(ed)}</wp:${wrap}>`);
};
const bd = '<w:tblBorders>' + ['top', 'left', 'bottom', 'right', 'insideH', 'insideV'].map((s) => `<w:${s} w:val="single" w:sz="4" w:space="0" w:color="auto"/>`).join('') + '</w:tblBorders>';
const paras = (a) => p(a + r('Celula um Hxg')) + p(r('segunda Hxg')) + p(r('terceira Hxg')) + p(r('quarta Hxg')) + p(r('quinta Hxg'));
const table = (a) => {
  const tc = (inner) => `<w:tc><w:tcPr><w:tcW w:w="3000" w:type="dxa"/></w:tcPr>${inner}</w:tc>`;
  return `<w:tbl><w:tblPr><w:tblW w:w="0" w:type="auto"/><w:tblInd w:w="1000" w:type="dxa"/>${bd}</w:tblPr><w:tblGrid><w:gridCol w:w="3000"/><w:gridCol w:w="3000"/></w:tblGrid>`
    + `<w:tr>${tc(paras(a))}${tc(p(r('Celula dois Hxg')))}</w:tr></w:tbl>`;
};
const ind = '<w:pPr><w:ind w:right="5504"/></w:pPr>';
const bp = (a, t) => `<w:p>${ind}${a}${r(t)}</w:p>`;
const body = (a) => bp(a, 'Celula um Hxg') + bp('', 'segunda Hxg') + bp('', 'terceira Hxg') + bp('', 'quarta Hxg') + bp('', 'quinta Hxg');

export const cases = [];
const V = {
  fill: { fill: FILL },
  line: { line: '<a:ln w="6350"><a:solidFill><a:prstClr val="black"/></a:solidFill></a:ln>' }, // a 0.5-pt black outline
  ed1: { ed: 1 },
  fillnoln: { fill: FILL, line: NOLN },
};
for (const [k, o] of Object.entries(V)) {
  for (const wrap of ['wrapTight', 'wrapThrough']) {
    const w = wrap === 'wrapTight' ? 'tt' : 'th';
    for (const y of [6, 14]) {
      const b = box({ wrap, y, ...o });
      cases.push({ name: `t2-cell-${w}-${k}-y${y}`, docx: D(p(r('Antes Hxg')) + table(b) + p(r('Depois Hxg'))) });
      cases.push({ name: `t2-body-${w}-${k}-y${y}`, docx: D(p(r('Antes Hxg')) + body(b) + p(r('Depois Hxg'))) });
    }
  }
}
