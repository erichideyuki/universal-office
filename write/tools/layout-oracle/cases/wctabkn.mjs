// WordCraft study (2026-11-13), planner "tables" — B5 completed (core/target/wordcraft-study/tables/wctabkn).
// Word keeps a table row with the next one only when the FIRST paragraph of the FIRST cell has keepNext (B/wcbtbl2,
// B/wcbmisc). Not pinned down there: three paragraphs in the first cell, a first cell that is a vMerge continuation, a
// row with gridBefore, a nested table first in the cell, keepNext from a paragraph STYLE, an empty first paragraph,
// compatibility mode 14.
// Layout: 40 filler lines, a title, the table: row A (no keepNext), row K (the candidate), row T (5000 tw, does not
// fit) — row K goes to page 2 with row T when it keeps with it, else stays on page 1.
// Liberation Sans 11.
import { buildDocx, docDefaults, p, r } from '../docx-builder.mjs';

const KN = '<w:keepNext/>';
const BD = '<w:tblBorders>' + ['top', 'left', 'bottom', 'right', 'insideH', 'insideV'].map((s) => `<w:${s} w:val="single" w:sz="4" w:space="0" w:color="auto"/>`).join('') + '</w:tblBorders>';
const tc = (inner, tcPr = '') => `<w:tc><w:tcPr><w:tcW w:w="3000" w:type="dxa"/>${tcPr}</w:tcPr>${inner}</w:tc>`;
const tr = (cells, trPr = '') => `<w:tr>${trPr ? `<w:trPr>${trPr}</w:trPr>` : ''}${cells.join('')}</w:tr>`;
const tbl = (rows) => `<w:tbl><w:tblPr><w:tblW w:w="6000" w:type="dxa"/>${BD}</w:tblPr><w:tblGrid><w:gridCol w:w="3000"/><w:gridCol w:w="3000"/></w:tblGrid>${rows}</w:tbl>`;
const filler = (n) => Array.from({ length: n }, (_, k) => p(r(`enchimento ${k + 1} Hxg`))).join('');
const tall = '<w:trHeight w:val="5000"/>';
const rowA = tr([tc(p(r('linhaA um Hxg'))), tc(p(r('linhaA dois Hxg')))]);
const rowT = tr([tc(p(r('alta A Hxg'))), tc(p(r('alta B Hxg')))], tall);
const STY = '<w:style w:type="paragraph" w:customStyle="1" w:styleId="ComKN"><w:name w:val="ComKN"/><w:basedOn w:val="Normal"/><w:qFormat/><w:pPr><w:keepNext/></w:pPr></w:style>';
const nested = (kn) => `<w:tbl><w:tblPr><w:tblW w:w="2000" w:type="dxa"/>${BD}</w:tblPr><w:tblGrid><w:gridCol w:w="2000"/></w:tblGrid>`
  + `<w:tr><w:tc><w:tcPr><w:tcW w:w="2000" w:type="dxa"/></w:tcPr>${p(r('aninhada Hxg'), kn ? KN : '')}</w:tc></w:tr></w:tbl>`;

export const cases = [];
const add = (name, rowK, opts = {}) => {
  const rows = (opts.rowA ?? rowA) + rowK + rowT;
  const body = filler(opts.fill ?? 39) + p(r('Titulo Hxg')) + tbl(rows) + p(r('Fim Hxg'));
  cases.push({ name, docx: buildDocx({ body, compat: opts.compat ?? 15, styles: opts.styles ?? '', defaults: docDefaults({ font: 'Liberation Sans', size: 11 }) }) });
};
for (const compat of [15, 14]) {
  const s = compat === 15 ? '' : '-c14';
  // three paragraphs in the first cell, keepNext on exactly one of them
  for (const k of [1, 2, 3]) {
    const paras = [1, 2, 3].map((i) => p(r(`linhaK p${i} Hxg`), i === k ? KN : '')).join('');
    add(`kn-p${k}of3${s}`, tr([tc(paras), tc(p(r('linhaK B Hxg')))]), { compat });
  }
  // control: keepNext on the single paragraph of cell 1 (moves), on cell 2 only (stays)
  add(`kn-cell1${s}`, tr([tc(p(r('linhaK A Hxg'), KN)), tc(p(r('linhaK B Hxg')))]), { compat });
  add(`kn-cell2${s}`, tr([tc(p(r('linhaK A Hxg'))), tc(p(r('linhaK B Hxg'), KN))]), { compat });
}
// the first cell of row K is a vMerge continuation (its paragraph has keepNext; row A's cell restarts the merge)
{
  const a = tr([tc(p(r('linhaA um Hxg')), '<w:vMerge w:val="restart"/>'), tc(p(r('linhaA dois Hxg')))]);
  add('kn-vmcont-kn', tr([tc(p('', KN), '<w:vMerge/>'), tc(p(r('linhaK B Hxg')))]), { rowA: a });
  add('kn-vmcont-cell2', tr([tc(p(''), '<w:vMerge/>'), tc(p(r('linhaK B Hxg'), KN))]), { rowA: a });
  // the merge restart cell (row A) has keepNext: row A keeps with row K, row K (no keepNext) with nothing
  const a2 = tr([tc(p(r('linhaA um Hxg'), KN), '<w:vMerge w:val="restart"/>'), tc(p(r('linhaA dois Hxg')))]);
  add('kn-vmrestart-kn', tr([tc(p(''), '<w:vMerge/>'), tc(p(r('linhaK B Hxg')))]), { rowA: a2 });
}
// gridBefore: row K starts at grid column 1 (its first cell is the 2nd grid column)
{
  const gb = '<w:gridBefore w:val="1"/><w:wBefore w:w="3000" w:type="dxa"/>';
  add('kn-gridbefore-kn', tr([tc(p(r('linhaK B Hxg'), KN))], gb));
  add('kn-gridbefore-none', tr([tc(p(r('linhaK B Hxg')))], gb));
}
// a nested table first in cell 1 (its paragraph has keepNext), then a paragraph without; and the reverse
add('kn-nested-kn', tr([tc(nested(true) + p(r('linhaK depois Hxg'))), tc(p(r('linhaK B Hxg')))]));
add('kn-nested-after', tr([tc(nested(false) + p(r('linhaK depois Hxg'), KN)), tc(p(r('linhaK B Hxg')))]));
// keepNext from the paragraph style of the first paragraph
add('kn-style', tr([tc(p(r('linhaK A Hxg'), '<w:pStyle w:val="ComKN"/>')), tc(p(r('linhaK B Hxg')))]), { styles: STY });
// an empty first paragraph with keepNext, then text
add('kn-empty-first', tr([tc(p('', KN) + p(r('linhaK A Hxg'))), tc(p(r('linhaK B Hxg')))]));
