// Wave 7c (W, item 4 — the Rust port): REF/PAGEREF/NOTEREF/STYLEREF a indicadores ausentes, sem fim, invertidos e
// que cobrem mais de um parágrafo. O Word atualiza REF e PAGEREF ao exportar o PDF e escreve os textos de erro (em
// negrito); o TS mostra o texto do parágrafo do início, a página 1 ou o resultado guardado ("CACHED").
// Liberation Sans 11, documentos em pt-BR.
import { buildDocx, docDefaults, p, r } from '../docx-builder.mjs';
import { tbx } from './_tbx.mjs';

const fld = (instr) => '<w:r><w:fldChar w:fldCharType="begin"/></w:r>' + `<w:r><w:instrText xml:space="preserve"> ${instr} </w:instrText></w:r>`
  + '<w:r><w:fldChar w:fldCharType="separate"/></w:r>' + r('CACHED') + '<w:r><w:fldChar w:fldCharType="end"/></w:r>';
const bs = (id, n) => `<w:bookmarkStart w:id="${id}" w:name="${n}"/>`, be = (id) => `<w:bookmarkEnd w:id="${id}"/>`;
const show = (label, instr) => p(r(`${label}=[`) + fld(instr) + r(']'));
const D = (body, more = {}) => buildDocx({ body, defaults: docDefaults({ font: 'Liberation Sans', size: 11 }), ...more });
const cell = (inner) => '<w:tbl><w:tblPr><w:tblW w:w="0" w:type="auto"/><w:tblLook w:val="04A0"/></w:tblPr><w:tblGrid><w:gridCol w:w="4000"/></w:tblGrid>'
  + `<w:tr><w:tc><w:tcPr><w:tcW w:w="4000" w:type="dxa"/></w:tcPr>${inner}</w:tc></w:tr></w:tbl>`;
const H1 = '<w:style w:type="paragraph" w:styleId="Ttulo1"><w:name w:val="heading 1"/><w:basedOn w:val="Normal"/><w:qFormat/><w:pPr><w:keepNext/><w:outlineLvl w:val="0"/></w:pPr><w:rPr><w:sz w:val="32"/></w:rPr></w:style>';

export const cases = [
  // sem bookmarkEnd
  { name: 'rb-unterm', docx: D(p(r('antes ') + bs(1, 'bmA') + r('alvo sem fim') + r(' resto do paragrafo')) + p(r('outro paragrafo'))
    + show('REF', 'REF bmA \\h') + show('PAGEREF', 'PAGEREF bmA \\h') + show('REFN', 'REF bmA \\n') + show('REFP', 'REF bmA \\p') + show('PAGEREFP', 'PAGEREF bmA \\p')
    + show('REFW', 'REF bmA \\w') + show('REFR', 'REF bmA \\r') + show('NOTEREF', 'NOTEREF bmA \\h')) },
  // o intervalo cobre dois parágrafos (com um trecho em negrito)
  { name: 'rb-multi', docx: D(p(r('antes ') + bs(1, 'bmB') + r('primeira parte')) + p(r('segunda ') + r('parte', '<w:b/>') + be(1) + r(' fora'))
    + show('REF', 'REF bmB \\h') + show('PAGEREF', 'PAGEREF bmB \\h')) },
  { name: 'rb-multi3', docx: D(p(r('zero ') + bs(1, 'bmM') + r('um')) + p(r('dois')) + p(r('tres') + be(1) + r(' quatro'))
    + show('REF', 'REF bmM')) },
  // nome que não existe
  { name: 'rb-missing', docx: D(p(r('nada')) + show('REF', 'REF bmZ \\h') + show('PAGEREF', 'PAGEREF bmZ \\h') + show('NOTEREF', 'NOTEREF bmZ \\h')
    + show('REFN', 'REF bmZ \\n') + show('REFP', 'REF bmZ \\p')) },
  // o fim antes do início
  { name: 'rb-endfirst', docx: D(p(r('antes ') + be(1) + r('meio ') + bs(1, 'bmC') + r('depois')) + show('REF', 'REF bmC') + show('PAGEREF', 'PAGEREF bmC')) },
  // STYLEREF sem parágrafo do estilo
  { name: 'rb-styleref', docx: D(p(r('texto')) + show('STYLEREF', 'STYLEREF 1'), { styles: H1 }) },
  // alvo numa célula de tabela (com e sem fim) e numa caixa de texto
  { name: 'rb-cell', docx: D(p(r('antes')) + cell(p(r('na ') + bs(1, 'bmD') + r('celula') + be(1) + r(' fim')) + p(r('outra ') + bs(2, 'bmE') + r('sem fim'))) + p(r('depois'))
    + show('REF', 'REF bmD') + show('PAGEREF', 'PAGEREF bmD') + show('REFE', 'REF bmE') + show('PAGEREFE', 'PAGEREF bmE')) },
  { name: 'rb-txbx', docx: D(p(tbx({ i: 1, x: 300, y: 80, w: 150, h: 60, id: 1, content: p(r('na ') + bs(1, 'bmT') + r('caixa') + be(1)) }) + r('ancora'))
    + show('REF', 'REF bmT') + show('PAGEREF', 'PAGEREF bmT')) },
  // alvo no cabeçalho
  { name: 'rb-header', docx: D(p(r('corpo')) + show('REF', 'REF bmH'),
    { headers: { default: p(r('cab ') + bs(9, 'bmH') + r('alvo') + be(9)) } }) },
  // sem fim numa página seguinte (PAGEREF)
  { name: 'rb-unterm-p2', docx: D(show('PAGEREF', 'PAGEREF bmP') + show('REF', 'REF bmP') + '<w:p><w:r><w:br w:type="page"/></w:r></w:p>' + p(r('pagina dois ') + bs(1, 'bmP') + r('alvo'))) },
];
