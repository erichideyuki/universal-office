// WordCraft study (2026-11-14), implementer "tables" — wctablic's lic-tight-*/lic-through-* with a FILLED box (wctabtt2:
// an unfilled, unoutlined tight/through box blocks only beside its own text), so the clamp of tight/through boxes is
// checked by whole lines. Same table and box as wctablic (x 300 → clamped right, x −60 → clamped left), modes 15 and 14.
import { buildDocx, docDefaults, p, r } from '../docx-builder.mjs';
import { tbx } from './_tbx.mjs';

const D = (body, compat) => buildDocx({ body, compat, defaults: docDefaults({ font: 'Liberation Sans', size: 11 }) });
const NOLN = '<a:ln><a:noFill/></a:ln>';
const FILL = '<a:solidFill><a:srgbClr val="DDEEFF"/></a:solidFill>';
const poly = '<wp:wrapPolygon edited="0"><wp:start x="0" y="0"/><wp:lineTo x="0" y="21600"/><wp:lineTo x="21600" y="21600"/><wp:lineTo x="21600" y="0"/><wp:lineTo x="0" y="0"/></wp:wrapPolygon>';
const box = ({ x, wrap }) => tbx({ i: 1, ins: [0, 0, 0, 0], content: p(r('Caixa Hxg')), wrap: 'wrapNone', x, y: 14, w: 80, h: 30, relH: 'column', relV: 'paragraph', line: NOLN, fill: FILL })
  .replace('<wp:wrapNone/>', `<wp:${wrap} wrapText="bothSides">${poly}</wp:${wrap}>`);
const bd = '<w:tblBorders>' + ['top', 'left', 'bottom', 'right', 'insideH', 'insideV'].map((s) => `<w:${s} w:val="single" w:sz="4" w:space="0" w:color="auto"/>`).join('') + '</w:tblBorders>';
const table = (a) => {
  const tc = (inner) => `<w:tc><w:tcPr><w:tcW w:w="3000" w:type="dxa"/></w:tcPr>${inner}</w:tc>`;
  return `<w:tbl><w:tblPr><w:tblW w:w="0" w:type="auto"/><w:tblInd w:w="1000" w:type="dxa"/>${bd}</w:tblPr><w:tblGrid><w:gridCol w:w="3000"/><w:gridCol w:w="3000"/></w:tblGrid>`
    + `<w:tr>${tc(p(a + r('Celula um Hxg')) + p(r('segunda Hxg')) + p(r('terceira Hxg')) + p(r('quarta Hxg')))}${tc(p(r('Celula dois Hxg')) + p(r('segunda dois Hxg')) + p(r('terceira dois Hxg')))}</w:tr></w:tbl>`;
};

export const cases = [];
for (const compat of [15, 14]) {
  const s = compat === 15 ? '' : '-c14';
  for (const wrap of ['wrapTight', 'wrapThrough']) {
    const w = wrap === 'wrapTight' ? 'tight' : 'through';
    cases.push({ name: `licf-${w}-r${s}`, docx: D(p(r('Antes Hxg')) + table(box({ x: 300, wrap })) + p(r('Depois Hxg')), compat) });
    cases.push({ name: `licf-${w}-l${s}`, docx: D(p(r('Antes Hxg')) + table(box({ x: -60, wrap })) + p(r('Depois Hxg')), compat) });
  }
}
