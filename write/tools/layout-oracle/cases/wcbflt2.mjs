// WordCraft study (2026-11-13), section B — floats, second batch (core/target/wordcraft-study/B/wcbflt2).
//  B12 floating table (tblpPr): the narrowest strip that gets text, actual gaps ~18.9–22.4 pt (a drawing: 18 pt; a
//      floating table rejected 18.4 pt in the first batch)
//  B15 layoutInCell: a wrapping box pushed past the cell's LEFT edge; topAndBottom and tight; compat 15 / 14
import { buildDocx, docDefaults, p, r } from '../docx-builder.mjs';
import { tbx } from './_tbx.mjs';

const D = (body, compat = 15) => buildDocx({ body, compat, defaults: docDefaults({ font: 'Liberation Sans', size: 11 }) });
const letters = (n) => Array.from({ length: n }, (_, k) => 'abcdefghij'[k % 10]).join(' ');
export const cases = [];
// actual gap = param - 5.61 pt (tblpX 108 tw, the right border's centre)
for (const gap of [24.5, 25, 25.5, 26, 26.5, 27, 28, 30]) {
  const wTw = Math.round((425.2 - gap) * 20);
  const ft = `<w:tbl><w:tblPr><w:tblpPr w:leftFromText="0" w:rightFromText="0" w:vertAnchor="text" w:horzAnchor="margin" w:tblpX="108" w:tblpY="1"/><w:tblW w:w="${wTw}" w:type="dxa"/>`
    + '<w:tblBorders>' + ['top', 'left', 'bottom', 'right'].map((s) => `<w:${s} w:val="single" w:sz="4" w:space="0" w:color="auto"/>`).join('') + '</w:tblBorders>'
    + `<w:tblLayout w:type="fixed"/><w:tblCellMar><w:left w:w="0" w:type="dxa"/><w:right w:w="0" w:type="dxa"/></w:tblCellMar></w:tblPr><w:tblGrid><w:gridCol w:w="${wTw}"/></w:tblGrid>`
    + `<w:tr><w:trPr><w:trHeight w:val="1200" w:hRule="exact"/></w:trPr><w:tc><w:tcPr><w:tcW w:w="${wTw}" w:type="dxa"/></w:tcPr>${p(r('Tabela Hxg'))}</w:tc></w:tr></w:tbl>`;
  cases.push({ name: `b12t-${String(gap).replace('.', '_')}`, docx: D(p(r('Antes Hxg')) + ft + p(r(letters(160))) + p(r('Fim Hxg'))) });
  // an EMPTY paragraph beside (WordCraft's evidence) — then text
  cases.push({ name: `b12t-${String(gap).replace('.', '_')}-empty`, docx: D(p(r('Antes Hxg')) + ft + p('') + p(r('Depois Hxg')) + p(r(letters(160)))) });
}
for (const compat of [15, 14]) {
  for (const [name, x, wrap] of [['b15-sqneg', -40, 'wrapSquare'], ['b15-tb', 120, 'wrapTopAndBottom'], ['b15-sq-far', 300, 'wrapSquare'], ['b15-sq-wide', 0, 'wrapSquare']]) {
    const w = name === 'b15-sq-wide' ? 200 : 80;
    const a = tbx({ i: 1, ins: [0, 0, 0, 0], content: p(r('Caixa Hxg')), wrap, x, y: 14, w, h: 30, relH: 'column', relV: 'paragraph' });
    const tc = (inner) => `<w:tc><w:tcPr><w:tcW w:w="3000" w:type="dxa"/></w:tcPr>${inner}</w:tc>`;
    const t = '<w:tbl><w:tblPr><w:tblW w:w="0" w:type="auto"/><w:tblInd w:w="1000" w:type="dxa"/><w:tblBorders>'
      + ['top', 'left', 'bottom', 'right', 'insideH', 'insideV'].map((s) => `<w:${s} w:val="single" w:sz="4" w:space="0" w:color="auto"/>`).join('')
      + '</w:tblBorders></w:tblPr><w:tblGrid><w:gridCol w:w="3000"/><w:gridCol w:w="3000"/></w:tblGrid>'
      + `<w:tr>${tc(p(a + r('Celula um Hxg')) + p(r('segunda Hxg')) + p(r('terceira Hxg')) + p(r('quarta Hxg')))}${tc(p(r('Celula dois Hxg')))}</w:tr></w:tbl>`;
    cases.push({ name: `${name}${compat === 15 ? '' : '-c14'}`, docx: D(p(r('Antes Hxg')) + t + p(r('Depois Hxg')), compat) });
  }
}
