// WordCraft study (2026-11-13), section B — tables. Measured into core/target/wordcraft-study/B/wcbtbl (--fixdir).
//  B1 autofit (tblLayout autofit, tblW auto) with tcW dxa and an unbreakable word wider than its column: does the
//     column grow and the table widen (ours) or does the grid stay and the other columns donate (WordCraft)?
//  B4 trHeight with hRule="auto" and a value: ignored (ours) or a minimum (WordCraft)?
//  lower: vMerge under an exact-height last row; atLeast row taller than the page.
// Liberation Sans 11 (open, metric twin of Arial): "M" = 9.17 pt; 14 × M = 128.4 pt = 2567 tw.
import { buildDocx, docDefaults, p, r } from '../docx-builder.mjs';

const BD = '<w:tblBorders>' + ['top', 'left', 'bottom', 'right', 'insideH', 'insideV'].map((s) => `<w:${s} w:val="single" w:sz="4" w:space="0" w:color="auto"/>`).join('') + '</w:tblBorders>';
const D = (body, extra = {}) => buildDocx({ body, defaults: docDefaults({ font: 'Liberation Sans', size: 11 }), ...extra });
const tc = (w, inner, tcPr = '') => `<w:tc><w:tcPr>${w ? `<w:tcW w:w="${w}" w:type="dxa"/>` : ''}${tcPr}</w:tcPr>${inner}</w:tc>`;
const tbl = (grid, rows, tblPr = '<w:tblW w:w="0" w:type="auto"/>') => `<w:tbl><w:tblPr>${tblPr}${BD}<w:tblLook w:val="04A0"/></w:tblPr><w:tblGrid>${grid.map((g) => `<w:gridCol w:w="${g}"/>`).join('')}</w:tblGrid>${rows}</w:tbl>`;
const M = (n) => 'M'.repeat(n);

export const cases = [];
for (const compat of [15, 14]) {
  const s = compat === 15 ? '' : '-c14';
  const DD = (body) => D(body, { compat });
  // B1: grid 1500/3000/3000 (7500 of 8504 tw), the 2567-tw word in column 1 / 2
  for (const [name, grid, col, word] of [
    ['b1-a', [1500, 3000, 3000], 0, M(14)],
    ['b1-b', [1500, 3000, 3000], 1, M(14) + M(14)], // 5134 tw in a 3000-tw column
    ['b1-full', [1500, 3500, 3500], 0, M(14)], // 8500: no room to grow
    ['b1-small', [1500, 3000, 3000], 0, M(8)], // 1467 tw: fits the 1500 column minus margins? (1500-216=1284 → no)
  ]) {
    const cells = grid.map((g, k) => tc(g, p(r(k === col ? word : `c${k + 1} Hxg`)))).join('');
    const row2 = grid.map((g, k) => tc(g, p(r(`d${k + 1} Hxg`)))).join('');
    cases.push({ name: `${name}${s}`, docx: DD(p(r('Antes Hxg')) + tbl(grid, `<w:tr>${cells}</w:tr><w:tr>${row2}</w:tr>`) + p(r('Depois Hxg'))) });
    // the same with tblW dxa = sum of the grid
    const sum = grid.reduce((a, b) => a + b, 0);
    cases.push({ name: `${name}-tblw${s}`, docx: DD(p(r('Antes Hxg')) + tbl(grid, `<w:tr>${cells}</w:tr><w:tr>${row2}</w:tr>`, `<w:tblW w:w="${sum}" w:type="dxa"/>`) + p(r('Depois Hxg'))) });
  }
  // B4: trHeight val 2000 with hRule auto / absent / atLeast / exact, one line
  for (const [name, trh] of [
    ['b4-auto', '<w:trHeight w:val="2000" w:hRule="auto"/>'],
    ['b4-none', '<w:trHeight w:val="2000"/>'],
    ['b4-atleast', '<w:trHeight w:val="2000" w:hRule="atLeast"/>'],
    ['b4-exact', '<w:trHeight w:val="2000" w:hRule="exact"/>'],
    ['b4-plain', ''],
  ]) {
    const rows = `<w:tr>${trh ? `<w:trPr>${trh}</w:trPr>` : ''}${tc(4000, p(r('Linha um Hxg')))}</w:tr><w:tr>${tc(4000, p(r('Linha dois Hxg')))}</w:tr>`;
    cases.push({ name: `${name}${s}`, docx: DD(p(r('Antes Hxg')) + tbl([4000], rows) + p(r('Depois Hxg'))) });
  }
  // lower: vMerge continuing under an exact-height LAST row (the merged text is taller than both rows)
  {
    const tall = p(r(Array.from({ length: 6 }, (_, k) => `alta ${k + 1} Hxg`).join('\n')));
    const rows = `<w:tr>${tc(3000, tall, '<w:vMerge w:val="restart"/>')}${tc(3000, p(r('direita 1 Hxg')))}</w:tr>`
      + `<w:tr><w:trPr><w:trHeight w:val="400" w:hRule="exact"/></w:trPr>${tc(3000, p(''), '<w:vMerge/>')}${tc(3000, p(r('direita 2 Hxg')))}</w:tr>`;
    cases.push({ name: `low-vmexact${s}`, docx: DD(p(r('Antes Hxg')) + tbl([3000, 3000], rows) + p(r('Depois Hxg'))) });
  }
  // lower: atLeast row taller than the page (20000 tw > 14004 tw of body) after 10 lines
  {
    const filler = Array.from({ length: 10 }, (_, k) => p(r(`enchimento ${k + 1} Hxg`))).join('');
    const rows = `<w:tr><w:trPr><w:trHeight w:val="20000" w:hRule="atLeast"/></w:trPr>${tc(4000, p(r('Alta um Hxg')))}</w:tr><w:tr>${tc(4000, p(r('Seguinte Hxg')))}</w:tr>`;
    cases.push({ name: `low-atleast-tall${s}`, docx: DD(filler + tbl([4000], rows) + p(r('Depois Hxg'))) });
    const rows2 = `<w:tr><w:trPr><w:trHeight w:val="20000" w:hRule="exact"/></w:trPr>${tc(4000, p(r('Alta um Hxg')))}</w:tr><w:tr>${tc(4000, p(r('Seguinte Hxg')))}</w:tr>`;
    cases.push({ name: `low-exact-tall${s}`, docx: DD(filler + tbl([4000], rows2) + p(r('Depois Hxg'))) });
  }
}
