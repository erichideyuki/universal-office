// WordCraft study (2026-11-13), planner "tables" — second round (core/target/wordcraft-study/tables/wctab2).
//  ft-   B12: the floating table's right wrap edge against rightFromText (36/72/288 tw; 0 and 144 are in wctabft) and its
//        LEFT edge with the table placed by tblpX (right-aligned text in the strip on its left; a full-width
//        right-aligned control paragraph below calibrates the glyph's right edge).
//  fix-  B15: a fixed-layout table (the cell cannot grow) with a wrapping box wider than the cell, pushed right / left.
//  fw-   a mode-15 finding of wctablic (lic-ctl-*): a box in a cell leaving a 30 pt strip — Word put the cell's next
//        line BELOW the box (mode 14 and ours: beside, the word broken by characters). Is it "the first word must fit
//        the strip"? Cells and body, short and long words, modes 15 / 14.
// Liberation Sans 11, A4, margins 1701 tw (column 85.05–510.25 pt).
import { buildDocx, docDefaults, p, r } from '../docx-builder.mjs';
import { tbx } from './_tbx.mjs';

const D = (body, compat = 15) => buildDocx({ body, compat, defaults: docDefaults({ font: 'Liberation Sans', size: 11 }) });
const words = (n) => Array.from({ length: n }, (_, k) => ['ab', 'cd', 'ef', 'gh', 'ij'][k % 5]).join(' ');
const bd = (sz) => (sz ? '<w:tblBorders>' + ['top', 'left', 'bottom', 'right'].map((s) => `<w:${s} w:val="single" w:sz="${sz}" w:space="0" w:color="auto"/>`).join('') + '</w:tblBorders>' : '');
const ft = ({ wTw, sz = 4, lft = 0, rft = 0, x = 108 }) => `<w:tbl><w:tblPr><w:tblpPr w:leftFromText="${lft}" w:rightFromText="${rft}" w:vertAnchor="text" w:horzAnchor="margin" w:tblpX="${x}" w:tblpY="1"/><w:tblW w:w="${wTw}" w:type="dxa"/>`
  + `${bd(sz)}<w:tblLayout w:type="fixed"/><w:tblCellMar><w:left w:w="0" w:type="dxa"/><w:right w:w="0" w:type="dxa"/></w:tblCellMar></w:tblPr><w:tblGrid><w:gridCol w:w="${wTw}"/></w:tblGrid>`
  + `<w:tr><w:trPr><w:trHeight w:val="1200" w:hRule="exact"/></w:trPr><w:tc><w:tcPr><w:tcW w:w="${wTw}" w:type="dxa"/></w:tcPr>${p(r('Tabela Hxg'))}</w:tc></w:tr></w:tbl>`;
const NOLN = '<a:ln><a:noFill/></a:ln>';

export const cases = [];
// ── B12 right edge vs rightFromText ──
for (const sz of [0, 4, 24]) {
  for (const rft of [36, 72, 288]) {
    cases.push({ name: `ft-r-sz${sz}-rft${rft}`, docx: D(p(r('Antes Hxg')) + ft({ wTw: 6000, sz, rft }) + p(r(words(120))) + p(r('Fim Hxg'))) });
  }
}
// ── B12 left edge: the table at tblpX 2508 tw (grid left 85.05 + 125.4 = 210.45 pt), 6000 tw wide (to 510.45) ──
const ctl = p(r(words(60)), '<w:jc w:val="right"/>');
for (const compat of [15, 14]) {
  for (const sz of [0, 4, 24]) {
    for (const lft of [0, 144]) {
      if (compat === 14 && (lft !== 0 || sz === 0)) continue;
      const s = compat === 15 ? '' : '-c14';
      cases.push({ name: `ft-l-sz${sz}-lft${lft}${s}`, docx: D(p(r('Antes Hxg')) + ft({ wTw: 6000, sz, lft, x: 2508 }) + p(r(words(60)), '<w:jc w:val="right"/>') + p('') + p('') + p('') + p('') + ctl, compat) });
    }
  }
}
// ── B15 fixed layout: a 200 pt square box in a 150 pt cell ──
const fixTbl = (a) => {
  const tc = (inner) => `<w:tc><w:tcPr><w:tcW w:w="3000" w:type="dxa"/></w:tcPr>${inner}</w:tc>`;
  return '<w:tbl><w:tblPr><w:tblW w:w="6000" w:type="dxa"/><w:tblInd w:w="1000" w:type="dxa"/><w:tblBorders>'
    + ['top', 'left', 'bottom', 'right', 'insideH', 'insideV'].map((s) => `<w:${s} w:val="single" w:sz="4" w:space="0" w:color="auto"/>`).join('')
    + '</w:tblBorders><w:tblLayout w:type="fixed"/></w:tblPr><w:tblGrid><w:gridCol w:w="3000"/><w:gridCol w:w="3000"/></w:tblGrid>'
    + `<w:tr>${tc(p(a + r('Celula um Hxg')) + p(r('segunda Hxg')) + p(r('terceira Hxg')))}${tc(p(r('Celula dois Hxg')))}</w:tr></w:tbl>`;
};
for (const compat of [15, 14]) {
  const s = compat === 15 ? '' : '-c14';
  for (const [name, x] of [['fix-wide-r', 40], ['fix-wide-l', -40], ['fix-fit-r', 300]]) {
    const w = name === 'fix-fit-r' ? 80 : 200;
    const a = tbx({ i: 1, ins: [0, 0, 0, 0], content: p(r('Caixa Hxg')), wrap: 'wrapSquare', x, y: 14, w, h: 30, relH: 'column', relV: 'paragraph', line: NOLN });
    cases.push({ name: `${name}${s}`, docx: D(p(r('Antes Hxg')) + fixTbl(a) + p(r('Depois Hxg')), compat) });
  }
}
// ── the strip beside a box: does the first word have to fit? ──
for (const compat of [15, 14]) {
  const s = compat === 15 ? '' : '-c14';
  // in a cell (fixed 3000 tw cells): box 80 pt at x +20 (right strip ~30 pt after distR 9), then short / long words
  const cell = (a, rest) => {
    const tc = (inner) => `<w:tc><w:tcPr><w:tcW w:w="3000" w:type="dxa"/></w:tcPr>${inner}</w:tc>`;
    return '<w:tbl><w:tblPr><w:tblW w:w="6000" w:type="dxa"/><w:tblInd w:w="1000" w:type="dxa"/><w:tblBorders>'
      + ['top', 'left', 'bottom', 'right', 'insideH', 'insideV'].map((q) => `<w:${q} w:val="single" w:sz="4" w:space="0" w:color="auto"/>`).join('')
      + '</w:tblBorders><w:tblLayout w:type="fixed"/></w:tblPr><w:tblGrid><w:gridCol w:w="3000"/><w:gridCol w:w="3000"/></w:tblGrid>'
      + `<w:tr>${tc(p(a + r('Celula um Hxg')) + rest)}${tc(p(r('Celula dois Hxg')))}</w:tr></w:tbl>`;
  };
  const cbox = (x, w = 80) => tbx({ i: 1, ins: [0, 0, 0, 0], content: p(r('Caixa Hxg')), wrap: 'wrapSquare', x, y: 14, w, h: 30, relH: 'column', relV: 'paragraph', line: NOLN });
  cases.push({ name: `fw-cell-short${s}`, docx: D(p(r('Antes Hxg')) + cell(cbox(20), p(r('ab cd ef gh ij ab cd ef gh ij ab cd'))) + p(r('Depois Hxg')), compat) });
  cases.push({ name: `fw-cell-long${s}`, docx: D(p(r('Antes Hxg')) + cell(cbox(20), p(r('segunda terceira'))) + p(r('Depois Hxg')), compat) });
  // a 60 pt strip (box x 0, 70 wide: strip = 139.6 - 70 - 9 = 60.6) with "segunda" (≈ 42 pt) — fits
  cases.push({ name: `fw-cell-wide${s}`, docx: D(p(r('Antes Hxg')) + cell(cbox(0, 70), p(r('segunda terceira quarta'))) + p(r('Depois Hxg')), compat) });
  // the body: a box at the left margin leaving a 45 pt strip (distR 9: box 371.2 wide), a long word first (≈ 75 pt) /
  // short words / a long word later in the paragraph
  const bbox = tbx({ i: 1, ins: [0, 0, 0, 0], content: p(r('Caixa Hxg')), wrap: 'wrapSquare', x: 0, y: 0, w: 371.2, h: 60, relH: 'margin', relV: 'paragraph', line: NOLN });
  cases.push({ name: `fw-body-long${s}`, docx: D(p(r('Antes Hxg')) + p(bbox + r('responsabilidades administrativas fim')) + p(r('Depois Hxg')), compat) });
  cases.push({ name: `fw-body-short${s}`, docx: D(p(r('Antes Hxg')) + p(bbox + r(words(40))) + p(r('Depois Hxg')), compat) });
  cases.push({ name: `fw-body-mixed${s}`, docx: D(p(r('Antes Hxg')) + p(bbox + r('ab cd ef responsabilidades gh ij ab cd ef gh ij')) + p(r('Depois Hxg')), compat) });
}
