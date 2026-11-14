// WordCraft study (2026-11-13), section B — last batch (core/target/wordcraft-study/B/wcbmisc). Run with --fresh
// (the DollarText cases may crash Word 365 pt-BR, as b22-pt-BR did).
//  B5   keepNext on the FIRST of two paragraphs of the first cell; on all paragraphs of the first cell
//  aside an autofit table whose tblW / tcW add up to more than the text width (9000 > 8504 tw): fitted to the margins?
//  B22  PAGE \* DollarText in pt-BR, one value per document (1, 21, 112)
//  lower TOC entries with PAGEREF WITHOUT \h (cached wrong page numbers) and a body PAGEREF without \h
// Liberation Sans 11.
import { buildDocx, docDefaults, p, r } from '../docx-builder.mjs';

const KN = '<w:keepNext/>';
const BD = '<w:tblBorders>' + ['top', 'left', 'bottom', 'right', 'insideH', 'insideV'].map((s) => `<w:${s} w:val="single" w:sz="4" w:space="0" w:color="auto"/>`).join('') + '</w:tblBorders>';
const tr = (cells, trPr = '') => `<w:tr>${trPr ? `<w:trPr>${trPr}</w:trPr>` : ''}${cells.map((c) => `<w:tc><w:tcPr><w:tcW w:w="3000" w:type="dxa"/></w:tcPr>${c}</w:tc>`).join('')}</w:tr>`;
const filler = (n) => Array.from({ length: n }, (_, k) => p(r(`enchimento ${k + 1} Hxg`))).join('');
const D = (body, extra = {}) => buildDocx({ body, defaults: docDefaults({ font: 'Liberation Sans', size: 11 }), ...extra });
const fld = (instr, cached = 'C?') => `<w:r><w:fldChar w:fldCharType="begin"/></w:r><w:r><w:instrText xml:space="preserve"> ${instr} </w:instrText></w:r><w:r><w:fldChar w:fldCharType="separate"/></w:r><w:r><w:t>${cached}</w:t></w:r><w:r><w:fldChar w:fldCharType="end"/></w:r>`;
const SP = (start) => `<w:sectPr><w:pgSz w:w="11906" w:h="16838"/><w:pgMar w:top="1417" w:right="1701" w:bottom="1417" w:left="1701" w:header="708" w:footer="708" w:gutter="0"/><w:pgNumType w:start="${start}"/><w:cols w:space="708"/><w:docGrid w:linePitch="360"/></w:sectPr>`;

export const cases = [];
// B5
const tbl2 = (rows) => `<w:tbl><w:tblPr><w:tblW w:w="6000" w:type="dxa"/>${BD}</w:tblPr><w:tblGrid><w:gridCol w:w="3000"/><w:gridCol w:w="3000"/></w:tblGrid>${rows}</w:tbl>`;
const tall = '<w:trHeight w:val="5000"/>';
for (const [name, cellA] of [
  ['b5-para1', p(r('zero A1 Hxg'), KN) + p(r('zero A2 Hxg'))],
  ['b5-allcell', p(r('zero A1 Hxg'), KN) + p(r('zero A2 Hxg'), KN)],
  ['b5-bothcells', null],
]) {
  const row0 = cellA ? tr([cellA, p(r('zero B Hxg'))]) : tr([p(r('zero A Hxg'), KN), p(r('zero B Hxg'), KN)]);
  cases.push({ name, docx: D(filler(40) + p(r('Titulo Hxg')) + tbl2(row0 + tr([p(r('alta A Hxg')), p(r('alta B Hxg'))], tall)) + p(r('Fim Hxg'))) });
}
// aside: 9000-tw table (3 × 3000) in an 8504-tw text column
for (const compat of [15, 14]) {
  for (const [name, layout] of [['tw-auto', ''], ['tw-fixed', '<w:tblLayout w:type="fixed"/>']]) {
    const t = `<w:tbl><w:tblPr><w:tblW w:w="9000" w:type="dxa"/>${BD}${layout}</w:tblPr><w:tblGrid>${'<w:gridCol w:w="3000"/>'.repeat(3)}</w:tblGrid>`
      + tr([p(r('um Hxg')), p(r('dois Hxg')), p(r('tres Hxg'))]) + '</w:tbl>';
    cases.push({ name: `${name}${compat === 15 ? '' : '-c14'}`, docx: D(p(r('Antes Hxg')) + t + p(r('Depois Hxg')), { compat }) });
  }
}
// B22: DollarText in pt-BR
for (const n of [1, 21, 112]) {
  cases.push({ name: `dt-pt-${n}`, docx: buildDocx({ body: p(r('dt= ') + fld('PAGE \\* DollarText') + r(' Hxg')), sect: SP(n), defaults: docDefaults({ font: 'Liberation Sans', size: 11, lang: 'pt-BR' }) }) });
}
cases.push({ name: 'dt-en-1', docx: buildDocx({ body: p(r('dt= ') + fld('= 14,55 \\* DollarText') + r(' Hxg')), sect: SP(1), footers: { default: p(r('f= ') + fld('= 14,55 \\* DollarText') + r(' Hxg')) }, defaults: docDefaults({ font: 'Liberation Sans', size: 11, lang: 'en-US' }) }) });
// lower: TOC with PAGEREF without \h
{
  const bm = (id, name, inner) => `<w:bookmarkStart w:id="${id}" w:name="${name}"/>${inner}<w:bookmarkEnd w:id="${id}"/>`;
  const tabs = '<w:pPr><w:tabs><w:tab w:val="right" w:leader="dot" w:pos="8494"/></w:tabs></w:pPr>';
  const toc = `<w:p>${tabs}<w:r><w:fldChar w:fldCharType="begin"/></w:r><w:r><w:instrText xml:space="preserve"> TOC \\o "1-3" \\z \\u </w:instrText></w:r><w:r><w:fldChar w:fldCharType="separate"/></w:r>${r('Capitulo A Hxg')}<w:r><w:tab/></w:r>${fld('PAGEREF _Toc1', '99')}</w:p>`
    + `<w:p>${tabs}${r('Capitulo B Hxg')}<w:r><w:tab/></w:r>${fld('PAGEREF _Toc2', '98')}<w:r><w:fldChar w:fldCharType="end"/></w:r></w:p>`;
  const pb = '<w:r><w:br w:type="page"/></w:r>';
  const body = toc + p(r('Ref ') + fld('PAGEREF _Toc2', '77') + r(' fim Hxg')) + p(pb) + `<w:p>${bm(1, '_Toc1', r('Capitulo A Hxg'))}</w:p>` + p(pb) + p(pb) + `<w:p>${bm(2, '_Toc2', r('Capitulo B Hxg'))}</w:p>`;
  cases.push({ name: 'toc-noh', docx: D(body) });
}
