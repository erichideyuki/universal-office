// WordCraft study (2026-11-13), section B — tables, second batch. Measured into core/target/wordcraft-study/B/wcbtbl2.
//  B5 keepNext on a table row: only the FIRST paragraph of the FIRST cell counts (WordCraft) or any paragraph of any
//     cell (ours)? keepNext only in the 2nd cell / only in the 2nd paragraph of the 1st cell.
//  B6 tcBorders insideV / insideH inside a conditional table style part (firstRow / band1Horz / wholeTable): drawn?
//  lower: compat-14 repeated header row alone at the foot of the page.
// Liberation Sans 11 (12.65 pt per line, 55 lines per A4 page).
import { buildDocx, docDefaults, p, r } from '../docx-builder.mjs';

const KN = '<w:keepNext/>';
const BD = '<w:tblBorders>' + ['top', 'left', 'bottom', 'right', 'insideH', 'insideV'].map((s) => `<w:${s} w:val="single" w:sz="4" w:space="0" w:color="auto"/>`).join('') + '</w:tblBorders>';
const tbl = (rows, cols = 1, tblPr = BD) => `<w:tbl><w:tblPr><w:tblW w:w="${3000 * cols}" w:type="dxa"/>${tblPr}</w:tblPr><w:tblGrid>${'<w:gridCol w:w="3000"/>'.repeat(cols)}</w:tblGrid>${rows}</w:tbl>`;
const tr = (cells, trPr = '') => `<w:tr>${trPr ? `<w:trPr>${trPr}</w:trPr>` : ''}${cells.map((c) => `<w:tc><w:tcPr><w:tcW w:w="3000" w:type="dxa"/></w:tcPr>${c}</w:tc>`).join('')}</w:tr>`;
const filler = (n) => Array.from({ length: n }, (_, k) => p(r(`enchimento ${k + 1} Hxg`))).join('');
const tall = '<w:trHeight w:val="5000"/>';
const D = (body, extra = {}) => buildDocx({ body, defaults: docDefaults({ font: 'Liberation Sans', size: 11 }), ...extra });

export const cases = [];
// B5 — row 0 (keepNext somewhere) + a tall row that does not fit after 40 filler lines
for (const [name, row0] of [
  ['b5-first', [p(r('zero A Hxg'), KN), p(r('zero B Hxg'))]],
  ['b5-second', [p(r('zero A Hxg')), p(r('zero B Hxg'), KN)]],
  ['b5-para2', [p(r('zero A1 Hxg')) + p(r('zero A2 Hxg'), KN), p(r('zero B Hxg'))]],
  ['b5-none', [p(r('zero A Hxg')), p(r('zero B Hxg'))]],
]) {
  cases.push({ name, docx: D(filler(40) + p(r('Titulo Hxg')) + tbl(tr(row0) + tr([p(r('alta A Hxg')), p(r('alta B Hxg'))], tall), 2) + p(r('Fim Hxg'))) });
}
// B6 — a table style with no table borders; the conditional part carries tcBorders insideV / insideH (thick, 3 pt)
{
  const T = (part, tcb) => `<w:style w:type="table" w:customStyle="1" w:styleId="TCond"><w:name w:val="TCond"/><w:basedOn w:val="TableNormal"/><w:tblPr><w:tblStyleRowBandSize w:val="1"/></w:tblPr>`
    + `<w:tblStylePr w:type="${part}"><w:tcPr><w:tcBorders>${tcb}</w:tcBorders></w:tcPr></w:tblStylePr></w:style>`;
  const B = (s) => `<w:${s} w:val="single" w:sz="24" w:space="0" w:color="FF0000"/>`;
  const rows = Array.from({ length: 4 }, (_, k) => tr([p(r(`l${k} a Hxg`)), p(r(`l${k} b Hxg`)), p(r(`l${k} c Hxg`))])).join('');
  const look = '<w:tblStyle w:val="TCond"/><w:tblLook w:val="04A0" w:firstRow="1" w:lastRow="0" w:firstColumn="1" w:lastColumn="0" w:noHBand="0" w:noVBand="1"/>';
  for (const [name, part, tcb] of [
    ['b6-first-insv', 'firstRow', B('insideV')],
    ['b6-first-insh', 'firstRow', B('insideH')],
    ['b6-band-insv', 'band1Horz', B('insideV')],
    ['b6-whole-insh', 'wholeTable', B('insideH') + B('insideV')],
    ['b6-first-bottom', 'firstRow', B('bottom')],
  ]) {
    cases.push({ name, docx: D(p(r('Antes Hxg')) + tbl(rows, 3, look) + p(r('Depois Hxg')), { styles: T(part, tcb) }) });
  }
}
// lower — repeated header row (tblHeader) alone at the foot: header + tall rows, compat 14 / 15
for (const compat of [14, 15]) {
  const hdr = tr([p(r('Cabecalho Hxg'))], '<w:tblHeader/>');
  const body = filler(52) + tbl(hdr + tr([filler(4)], '<w:cantSplit/>') + tr([p(r('fim tab Hxg'))])) + p(r('Fim Hxg'));
  cases.push({ name: `low-hdrfoot-c${compat}`, docx: D(body, { compat }) });
}
