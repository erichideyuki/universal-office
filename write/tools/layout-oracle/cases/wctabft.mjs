// WordCraft study (2026-11-13), planner "tables" — B12 completed (core/target/wordcraft-study/tables/wctabft).
// B/wcbflt2: a floating table's (tblpPr) wrap edge is 1.05 pt (21 tw) right of tblpX + tblW, measured only with a
// 0.5 pt right border, cell margins 0 and rightFromText 0. Here: how the edge moves with the border width (none, 0.5,
// 1.5, 3 pt), rightFromText (0, 7.2 pt), the cell margins (0, 108 tw), mode 14; the LEFT edge (table at the right
// margin, text on its left in right-aligned paragraphs: the line's right end is the edge); the BOTTOM edge (a table
// as wide as the column: the first line below it).
// The strip is wide (the table leaves ~100 pt), so the strip text's x0 is the wrap edge itself.
// Liberation Sans 11; A4, margins 1701 tw (text width 425.2 pt, column 85.05–510.25 pt).
import { buildDocx, docDefaults, p, r } from '../docx-builder.mjs';

const D = (body, compat = 15) => buildDocx({ body, compat, defaults: docDefaults({ font: 'Liberation Sans', size: 11 }) });
const words = (n) => Array.from({ length: n }, (_, k) => ['ab', 'cd', 'ef', 'gh', 'ij'][k % 5]).join(' ');
const bd = (sz) => (sz ? '<w:tblBorders>' + ['top', 'left', 'bottom', 'right'].map((s) => `<w:${s} w:val="single" w:sz="${sz}" w:space="0" w:color="auto"/>`).join('') + '</w:tblBorders>' : '');
const ft = ({ wTw, sz = 4, mar = 0, lft = 0, rft = 0, bft = 0, xSpec = '', x = 108, hRows = 1200 }) => {
  const pos = xSpec ? `w:tblpXSpec="${xSpec}"` : `w:tblpX="${x}"`;
  const cm = mar === null ? '' : `<w:tblCellMar><w:left w:w="${mar}" w:type="dxa"/><w:right w:w="${mar}" w:type="dxa"/></w:tblCellMar>`;
  return `<w:tbl><w:tblPr><w:tblpPr w:leftFromText="${lft}" w:rightFromText="${rft}" w:bottomFromText="${bft}" w:vertAnchor="text" w:horzAnchor="margin" ${pos} w:tblpY="1"/><w:tblW w:w="${wTw}" w:type="dxa"/>`
    + `${bd(sz)}<w:tblLayout w:type="fixed"/>${cm}</w:tblPr><w:tblGrid><w:gridCol w:w="${wTw}"/></w:tblGrid>`
    + `<w:tr><w:trPr><w:trHeight w:val="${hRows}" w:hRule="exact"/></w:trPr><w:tc><w:tcPr><w:tcW w:w="${wTw}" w:type="dxa"/></w:tcPr>${p(r('Tabela Hxg'))}</w:tc></w:tr></w:tbl>`;
};

export const cases = [];
// right edge: table 6000 tw wide at tblpX 108 (85.05 + 5.4 → 390.45 pt), the strip ~120 pt
for (const compat of [15, 14]) {
  const s = compat === 15 ? '' : '-c14';
  for (const sz of [0, 4, 12, 24]) {
    for (const rft of [0, 144]) {
      for (const mar of [0, 108]) {
        if (compat === 14 && (rft !== 0 || sz === 12)) continue;
        cases.push({ name: `ft-r-sz${sz}-rft${rft}-m${mar}${s}`, docx: D(p(r('Antes Hxg')) + ft({ wTw: 6000, sz, mar, rft }) + p(r(words(120))) + p(r('Fim Hxg')), compat) });
      }
    }
  }
  // left edge: the table at the right margin (tblpXSpec right), text left of it, right-aligned
  for (const sz of [0, 4, 24]) {
    for (const lft of [0, 144]) {
      if (compat === 14 && lft !== 0) continue;
      cases.push({ name: `ft-l-sz${sz}-lft${lft}${s}`, docx: D(p(r('Antes Hxg')) + ft({ wTw: 6000, sz, lft, xSpec: 'right' }) + p(r(words(120)), '<w:jc w:val="right"/>') + p(r('Fim Hxg')), compat) });
    }
  }
  // bottom edge: a table as wide as the column (no strip): the first line below it
  for (const sz of [0, 4, 24]) {
    for (const bft of [0, 144]) {
      if (compat === 14 && bft !== 0) continue;
      cases.push({ name: `ft-b-sz${sz}-bft${bft}${s}`, docx: D(p(r('Antes Hxg')) + ft({ wTw: 8504, sz, bft, x: 0 }) + p(r(words(20))) + p(r('Fim Hxg')), compat) });
    }
  }
}
// the default cell margins (no tblCellMar: the style's 108) with a 0.5 pt border, mode 15
cases.push({ name: 'ft-r-sz4-rft0-mdef', docx: D(p(r('Antes Hxg')) + ft({ wTw: 6000, sz: 4, mar: null }) + p(r(words(120))) + p(r('Fim Hxg'))) });
// the B12 threshold with a thick border: the strip left by a 3 pt-bordered table at 18.5 / 19.5 pt from the column edge
// measured from Word's edge (tblpX 0: the table's right border at 85.05 + w)
for (const gap of [17.5, 18.5, 19.5]) {
  const wTw = Math.round((425.2 - gap - 1.05) * 20);
  cases.push({ name: `ft-gap-sz24-${String(gap).replace('.', '_')}`, docx: D(p(r('Antes Hxg')) + ft({ wTw, sz: 24, x: 0 }) + p(r(words(120))) + p(r('Fim Hxg'))) });
}
