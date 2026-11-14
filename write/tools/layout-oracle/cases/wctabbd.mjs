// WordCraft study (2026-11-13), planner "tables" — B6 completed (core/target/wordcraft-study/tables/wctabbd).
// B/wcbtbl2 measured: a conditional part's tcBorders insideV is drawn between the cells of firstRow / band1Horz rows;
// firstRow insideH (one row) nothing; wholeTable tcBorders insideH+insideV nothing. Not pinned down: the region's
// OUTER sides (does firstRow tcBorders left/right paint every cell's side or only the table edge?), insideH of a
// multi-row region (firstCol, band1Vert, band1Horz with tblStyleRowBandSize 2, two header rows), lastRow/lastCol,
// the conflict with the table's own tblBorders insideV (heavier wins?) and with the cell's direct tcBorders, and
// wholeTable left/right/top/bottom in tcBorders.
// 4 rows × 3 columns, 3 pt red borders from the style, tblLook firstRow+firstColumn+lastRow+lastColumn as needed.
// The order of tblPr follows the schema (tblStyle first) — see wctabtw for the order question.
import { buildDocx, docDefaults, p, r } from '../docx-builder.mjs';

const B = (s, sz = 24, c = 'FF0000') => `<w:${s} w:val="single" w:sz="${sz}" w:space="0" w:color="${c}"/>`;
const T = (parts, extraTblPr = '') => `<w:style w:type="table" w:customStyle="1" w:styleId="TCond"><w:name w:val="TCond"/><w:basedOn w:val="TableNormal"/><w:tblPr>${extraTblPr}</w:tblPr>`
  + parts.map(([part, tcb]) => `<w:tblStylePr w:type="${part}"><w:tcPr><w:tcBorders>${tcb}</w:tcBorders></w:tcPr></w:tblStylePr>`).join('') + '</w:style>';
const tc = (inner, tcPr = '') => `<w:tc><w:tcPr><w:tcW w:w="2000" w:type="dxa"/>${tcPr}</w:tcPr>${inner}</w:tc>`;
const rows = (n, trPr = () => '') => Array.from({ length: n }, (_, k) => `<w:tr>${trPr(k) ? `<w:trPr>${trPr(k)}</w:trPr>` : ''}${['a', 'b', 'c'].map((c) => tc(p(r(`l${k} ${c} Hxg`)))).join('')}</w:tr>`).join('');
const look = (o) => `<w:tblLook w:val="0000" w:firstRow="${o.fr ?? 0}" w:lastRow="${o.lr ?? 0}" w:firstColumn="${o.fc ?? 0}" w:lastColumn="${o.lc ?? 0}" w:noHBand="${o.nh ?? 1}" w:noVBand="${o.nv ?? 1}"/>`;
const tbl = (body, o, tblBorders = '') => `<w:tbl><w:tblPr><w:tblStyle w:val="TCond"/><w:tblW w:w="6000" w:type="dxa"/>${tblBorders}${look(o)}</w:tblPr>`
  + `<w:tblGrid>${'<w:gridCol w:w="2000"/>'.repeat(3)}</w:tblGrid>${body}</w:tbl>`;
const D = (body, styles) => buildDocx({ body: p(r('Antes Hxg')) + body + p(r('Depois Hxg')), styles, defaults: docDefaults({ font: 'Liberation Sans', size: 11 }) });

export const cases = [];
const add = (name, parts, o, extra = {}) => cases.push({ name, docx: D(tbl(extra.body ?? rows(4, extra.trPr), o, extra.tblBorders ?? ''), T(parts, extra.styleTblPr ?? '')) });
// outer sides of a one-row region
add('bd-first-left', [['firstRow', B('left')]], { fr: 1 });
add('bd-first-right', [['firstRow', B('right')]], { fr: 1 });
add('bd-first-top', [['firstRow', B('top')]], { fr: 1 });
// a multi-row region: firstCol (4 rows, one column) — insideH between its cells? left/right/top/bottom where?
add('bd-fcol-insh', [['firstCol', B('insideH')]], { fc: 1 });
add('bd-fcol-insv', [['firstCol', B('insideV')]], { fc: 1 });
add('bd-fcol-top', [['firstCol', B('top')]], { fc: 1 });
add('bd-fcol-right', [['firstCol', B('right')]], { fc: 1 });
add('bd-lcol-insh', [['lastCol', B('insideH')]], { lc: 1 });
add('bd-last-insv', [['lastRow', B('insideV')]], { lr: 1 });
// banded rows of size 2 (rows 0-1 band1, 2-3 band2): insideH inside a band, top/bottom of the band, left of the band
add('bd-band2-insh', [['band1Horz', B('insideH')]], { nh: 0 }, { styleTblPr: '<w:tblStyleRowBandSize w:val="2"/>' });
add('bd-band2-top', [['band1Horz', B('top')]], { nh: 0 }, { styleTblPr: '<w:tblStyleRowBandSize w:val="2"/>' });
add('bd-band2-left', [['band1Horz', B('left')]], { nh: 0 }, { styleTblPr: '<w:tblStyleRowBandSize w:val="2"/>' });
add('bd-band1-insv', [['band1Horz', B('insideV')]], { nh: 0 }, { styleTblPr: '<w:tblStyleRowBandSize w:val="1"/>' });
// banded columns (band1Vert: columns 0 and 2): insideH inside the column band
add('bd-vband-insh', [['band1Vert', B('insideH')]], { nv: 0 }, { styleTblPr: '<w:tblStyleColBandSize w:val="1"/>' });
// two header rows (tblHeader on rows 0 and 1, both firstRow): one region (insideH between them) or two?
add('bd-hdr2-insh', [['firstRow', B('insideH')]], { fr: 1 }, { trPr: (k) => (k < 2 ? '<w:tblHeader/>' : '') });
// the conflict with the table's own insideV (thin black, direct tblBorders) and with direct tcBorders of a cell
add('bd-first-insv-vs-tbl', [['firstRow', B('insideV')]], { fr: 1 },
  { tblBorders: '<w:tblBorders>' + ['top', 'left', 'bottom', 'right', 'insideH', 'insideV'].map((s) => B(s, 4, '000000')).join('') + '</w:tblBorders>' });
add('bd-first-insv-thin-vs-tbl', [['firstRow', B('insideV', 4, 'FF0000')]], { fr: 1 },
  { tblBorders: '<w:tblBorders>' + ['top', 'left', 'bottom', 'right', 'insideH', 'insideV'].map((s) => B(s, 12, '0000FF')).join('') + '</w:tblBorders>' });
{
  // row 0, cell b has a direct right border (green, 1 pt): does it beat the part's insideV (red 3 pt) on its right side?
  const body = rows(4).replace('<w:tc><w:tcPr><w:tcW w:w="2000" w:type="dxa"/></w:tcPr><w:p><w:r><w:t xml:space="preserve">l0 b Hxg',
    `<w:tc><w:tcPr><w:tcW w:w="2000" w:type="dxa"/><w:tcBorders>${B('right', 8, '00AA00')}</w:tcBorders></w:tcPr><w:p><w:r><w:t xml:space="preserve">l0 b Hxg`);
  add('bd-first-insv-vs-tc', [['firstRow', B('insideV')]], { fr: 1 }, { body });
}
// wholeTable tcBorders sides (not inside): left/right/top/bottom of every cell, or the table's outline?
add('bd-whole-left', [['wholeTable', B('left')]], {});
add('bd-whole-top', [['wholeTable', B('top')]], {});
// wholeTable insideV alone, and firstRow insideV over a wholeTable insideV (does the part revive it?)
add('bd-whole-insv', [['wholeTable', B('insideV')]], {});
add('bd-whole-insv-first', [['wholeTable', B('insideV', 24, '0000FF')], ['firstRow', B('insideV')]], { fr: 1 });
// firstRow + firstCol both with insideV: nwCell corner, the precedence (firstRow is applied after firstCol)
add('bd-first-fcol-insv', [['firstCol', B('insideV', 24, '0000FF')], ['firstRow', B('insideV')]], { fr: 1, fc: 1 });
