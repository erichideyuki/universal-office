// WordCraft study (2026-11-13), section B — floating objects. Measured into core/target/wordcraft-study/B/wcbflt (--fixdir).
//  B10 float anchored in a line on a LATER page than its paragraph's first line (paragraph / line / margin relative)
//  B11 paragraph-relative float ABOVE its own paragraph (negative offset): do earlier paragraphs wrap?
//  B12 narrowest strip beside a float that gets text: fine sweep 16–24 pt, a text box and a floating table (tblpPr)
//  B15 layoutInCell="1": a box pushed past the cell's right edge — clamped to the cell? (compat 15 / 14)
//  B16 unequal distL / distR (wrapSquare bothSides)
// Liberation Sans 11; text width 425.2 pt (A4, margins 1701 tw); invented text.
import { buildDocx, docDefaults, p, r, lorem } from '../docx-builder.mjs';
import { tbx } from './_tbx.mjs';

const E = 12700;
const D = (body, compat = 15) => buildDocx({ body, compat, defaults: docDefaults({ font: 'Liberation Sans', size: 11 }) });
const dist = (x, l, rr) => x.replace('distL="114300" distR="114300"', `distL="${Math.round(l * E)}" distR="${Math.round(rr * E)}"`);
const box = (o) => tbx({ i: 1, ins: [0, 0, 0, 0], content: p(r('Caixa Hxg')), wrap: 'wrapSquare', ...o });
const filler = (n, w = 'enchimento') => Array.from({ length: n }, (_, k) => p(r(`${w} ${k + 1} Hxg`))).join('');
const letters = (n) => Array.from({ length: n }, (_, k) => 'abcdefghij'[k % 10]).join(' ');

export const cases = [];

// ── B10: anchor run at the END of a paragraph that starts on page 1 and ends on page 2 ──
// 50 filler lines (12.65 pt each; 55 per page) + a 12-line paragraph: lines 6–12 go to page 2
{
  const long = Array.from({ length: 12 }, (_, k) => `longa ${k + 1} Hxg`).join('\n');
  const parts = long.split('\n');
  const para = (anchor, where) => {
    const before = where === 'start' ? anchor : '';
    const after = where === 'end' ? anchor : '';
    const mid = where === 'mid';
    const runs = parts.map((t, k) => r(t + (k < parts.length - 1 ? '\n' : '')) + (mid && k === 8 ? anchor : '')).join('');
    return p(before + runs + after);
  };
  for (const [name, relV, y, where, wrap] of [
    ['b10-para-end', 'paragraph', 0, 'end', 'wrapSquare'],
    ['b10-para-mid', 'paragraph', 0, 'mid', 'wrapSquare'],
    ['b10-para-start', 'paragraph', 0, 'start', 'wrapSquare'],
    ['b10-para-end-none', 'paragraph', 0, 'end', 'wrapNone'],
    ['b10-para-end-20', 'paragraph', 20, 'end', 'wrapSquare'],
    ['b10-line-end', 'line', 0, 'end', 'wrapSquare'],
    ['b10-margin-end', 'margin', 300, 'end', 'wrapSquare'],
    ['b10-page-end', 'page', 400, 'end', 'wrapNone'],
  ]) {
    const a = box({ x: 250, y, w: 120, h: 40, relH: 'margin', relV, wrap });
    cases.push({ name, docx: D(filler(50) + para(a, where) + filler(6, 'depois')) });
  }
}

// ── B11: float anchored in paragraph 6 with a negative offset (reaches up into paragraphs 4–5) ──
for (const [name, y, wrap, h] of [
  ['b11-sq-60', -60, 'wrapSquare', 40],
  ['b11-sq-120', -120, 'wrapSquare', 80],
  ['b11-tb-60', -60, 'wrapTopAndBottom', 40],
  ['b11-sq-0', 0, 'wrapSquare', 40], // control
]) {
  const a = box({ x: 0, y, w: 200, h, relH: 'margin', relV: 'paragraph', wrap });
  const body = Array.from({ length: 9 }, (_, k) => p((k === 5 ? a : '') + r(lorem(40, 3 + k)))).join('');
  cases.push({ name, docx: D(filler(2) + body) });
}

// ── B12: the strip right of a box at the left margin (distL = distR = 0): gaps 16–24 pt ──
for (const gap of [16, 17, 17.5, 18, 18.5, 19, 19.5, 20, 21, 24]) {
  const g = String(gap).replace('.', '_');
  const a = dist(box({ x: 0, y: 14, w: 425.2 - gap, h: 60, relH: 'margin', relV: 'paragraph' }), 0, 0);
  cases.push({ name: `b12-box-${g}`, docx: D(p(r('Antes Hxg')) + p(a + r(letters(160))) + p(r('Fim Hxg'))) });
  // right indent of 1 in: the strip to the column edge is gap, to the indent gap - 72 (only gap >= 72 meaningful) —
  // here the box leaves `gap` to the RIGHT INDENT (w = 425.2 - 72 - gap)
  const b = dist(box({ x: 0, y: 14, w: 425.2 - 72 - gap, h: 60, relH: 'margin', relV: 'paragraph' }), 0, 0);
  cases.push({ name: `b12-ind-${g}`, docx: D(p(r('Antes Hxg')) + p(b + r(letters(160)), '<w:ind w:right="1440"/>') + p(r('Fim Hxg'))) });
}
// floating table at the left margin, width 425.2 - gap - 2 cell margins? (the table's own box is measured from the PDF)
for (const gap of [16, 18, 19, 20, 24]) {
  const wTw = Math.round((425.2 - gap) * 20);
  const ft = `<w:tbl><w:tblPr><w:tblpPr w:leftFromText="0" w:rightFromText="0" w:vertAnchor="text" w:horzAnchor="margin" w:tblpX="108" w:tblpY="1"/><w:tblW w:w="${wTw}" w:type="dxa"/>`
    + '<w:tblBorders>' + ['top', 'left', 'bottom', 'right'].map((s) => `<w:${s} w:val="single" w:sz="4" w:space="0" w:color="auto"/>`).join('') + '</w:tblBorders>'
    + `<w:tblLayout w:type="fixed"/><w:tblCellMar><w:left w:w="0" w:type="dxa"/><w:right w:w="0" w:type="dxa"/></w:tblCellMar></w:tblPr><w:tblGrid><w:gridCol w:w="${wTw}"/></w:tblGrid>`
    + `<w:tr><w:trPr><w:trHeight w:val="1200" w:hRule="exact"/></w:trPr><w:tc><w:tcPr><w:tcW w:w="${wTw}" w:type="dxa"/></w:tcPr>${p(r('Tabela Hxg'))}</w:tc></w:tr></w:tbl>`;
  cases.push({ name: `b12-tbl-${gap}`, docx: D(p(r('Antes Hxg')) + ft + p(r(letters(160))) + p(r('Fim Hxg'))) });
}

// ── B16: box in the middle of the column (x 150 pt, 120 × 50), distL / distR unequal ──
for (const [name, l, rr] of [['b16-l0-r36', 0, 36], ['b16-l36-r0', 36, 0], ['b16-l9-r9', 9, 9], ['b16-l0-r0', 0, 0]]) {
  const a = dist(box({ x: 150, y: 14, w: 120, h: 50, relH: 'margin', relV: 'paragraph' }), l, rr);
  cases.push({ name, docx: D(p(r('Antes Hxg')) + p(a + r(lorem(120, 5))) + p(r('Fim Hxg'))) });
}

// ── B15: box in a table cell (layoutInCell 1), x 120 pt from the column (the cell is 150 pt wide) ──
for (const compat of [15, 14]) {
  for (const [name, x, wrap] of [['b15-none', 120, 'wrapNone'], ['b15-sq', 120, 'wrapSquare'], ['b15-neg', -40, 'wrapNone']]) {
    const a = box({ x, y: 14, w: 80, h: 30, relH: 'column', relV: 'paragraph', wrap });
    const tc = (inner) => `<w:tc><w:tcPr><w:tcW w:w="3000" w:type="dxa"/></w:tcPr>${inner}</w:tc>`;
    const t = '<w:tbl><w:tblPr><w:tblW w:w="0" w:type="auto"/><w:tblInd w:w="1000" w:type="dxa"/><w:tblBorders>'
      + ['top', 'left', 'bottom', 'right', 'insideH', 'insideV'].map((s) => `<w:${s} w:val="single" w:sz="4" w:space="0" w:color="auto"/>`).join('')
      + '</w:tblBorders></w:tblPr><w:tblGrid><w:gridCol w:w="3000"/><w:gridCol w:w="3000"/></w:tblGrid>'
      + `<w:tr>${tc(p(a + r('Celula um Hxg')) + p(r('segunda Hxg')) + p(r('terceira Hxg')) + p(r('quarta Hxg')))}${tc(p(r('Celula dois Hxg')))}</w:tr></w:tbl>`;
    cases.push({ name: `${name}${compat === 15 ? '' : '-c14'}`, docx: D(p(r('Antes Hxg')) + t + p(r('Depois Hxg')), compat) });
  }
}
