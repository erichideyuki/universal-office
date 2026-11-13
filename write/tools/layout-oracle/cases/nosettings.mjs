// Wave 7c (W, item 2 — the Rust port): um pacote SEM word/settings.xml. O Word abre-o no modo de compatibilidade 12
// (COM Document.CompatibilityMode = 12, medido), como uma parte de configurações sem compatSetting; o TS (parts.ts:411)
// lê o modo 15. Variantes: sem a parte; a parte vazia; a parte sem defaultTabStop no modo 15; os controles 12 e 15.
// Liberation Sans 11.
import { buildDocx, docDefaults, lorem, p, r } from '../docx-builder.mjs';

const tbl = (cells, grid = 2835) => '<w:tbl><w:tblPr><w:tblW w:w="0" w:type="auto"/><w:tblLook w:val="04A0"/></w:tblPr><w:tblGrid>'
  + cells.map(() => `<w:gridCol w:w="${grid}"/>`).join('') + '</w:tblGrid><w:tr>'
  + cells.map((c) => `<w:tc><w:tcPr><w:tcW w:w="${grid}" w:type="dxa"/></w:tcPr>${c}</w:tc>`).join('') + '</w:tr></w:tbl>';
const VARIANTS = {
  none: { noSettings: true },
  bare: { rawSettings: '' },
  c15notab: { rawSettings: '<w:compat><w:compatSetting w:name="compatibilityMode" w:uri="http://schemas.microsoft.com/office/word" w:val="15"/></w:compat>' },
  c12: { compat: 12 },
  c15: { compat: 15 },
};
const BODIES = {
  tbl: p(r('antes da tabela')) + tbl([p(r('celula um')), p(r('celula dois'))]) + p(r('depois')),
  nest: p(r('antes')) + tbl([tbl([p(r('dentro'))], 2000) + p(''), p(r('fora'))]) + p(r('depois')),
  // the first glyph after k default tab stops starts its line: x0 = k stops (708 or 720 tw)
  tabs: [1, 2, 3, 5, 9, 14].map((k) => p(r('\t'.repeat(k) + 'X' + k))).join('') + p(r('palavra\tx')),
  just: p(r(lorem(120, 3)), '<w:jc w:val="both"/>') + p(r(lorem(80, 5) + ' ' + lorem(30, 7)), '<w:jc w:val="both"/>'),
  ind: p(r('recuo negativo ' + lorem(30, 9)), '<w:ind w:left="-300" w:right="-200"/>') + tbl([p(r('x')), p(r('y'))]),
};

export const cases = [];
for (const [v, opt] of Object.entries(VARIANTS)) {
  for (const [b, body] of Object.entries(BODIES)) {
    cases.push({ name: `ns-${b}-${v}`, docx: buildDocx({ body, defaults: docDefaults({ font: 'Liberation Sans', size: 11 }), ...opt }) });
  }
}
