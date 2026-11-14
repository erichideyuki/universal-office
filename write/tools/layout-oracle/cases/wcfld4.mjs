// WordCraft study (2026-11-13), planner "fields" — STYLEREF name matching and switches per page (B21 left the name rules
// for unused/undefined styles, letter case, styleId, aliases, custom styles and \n/\w open). Word 365 pt-BR, PDF lines.
// One header paragraph per field ("sN= <result>"), a tall page (22 in) so the header's error texts do not crowd the body.
// Pages: p1 headings A, B (numbered "1."/"2.": heading 1 linked to a list); p2 none; p3 text, a "Meu Estilo" paragraph,
// heading C; p4 a "Destaque" run. Liberation Sans 11; cached results "CX".
import { buildDocx, docDefaults, p, r } from '../docx-builder.mjs';

const fx = (instr) => instr.replace(/&/g, '&amp;').replace(/</g, '&lt;').replace(/>/g, '&gt;').replace(/"/g, '&quot;');
const fld = (instr) => `<w:r><w:fldChar w:fldCharType="begin"/></w:r><w:r><w:instrText xml:space="preserve"> ${fx(instr)} </w:instrText></w:r><w:r><w:fldChar w:fldCharType="separate"/></w:r><w:r><w:t>CX</w:t></w:r><w:r><w:fldChar w:fldCharType="end"/></w:r>`;
const pb = '<w:r><w:br w:type="page"/></w:r>';
const DEF = docDefaults({ font: 'Liberation Sans', size: 11, lang: 'pt-BR' });
const STYLES = `<w:style w:type="paragraph" w:styleId="Ttulo1"><w:name w:val="heading 1"/><w:basedOn w:val="Normal"/><w:next w:val="Normal"/><w:uiPriority w:val="9"/><w:qFormat/><w:pPr><w:keepNext/><w:numPr><w:numId w:val="1"/></w:numPr><w:outlineLvl w:val="0"/></w:pPr><w:rPr><w:b/></w:rPr></w:style>`
  + `<w:style w:type="paragraph" w:styleId="Ttulo2"><w:name w:val="heading 2"/><w:basedOn w:val="Normal"/><w:next w:val="Normal"/><w:uiPriority w:val="9"/><w:qFormat/><w:pPr><w:keepNext/><w:outlineLvl w:val="1"/></w:pPr></w:style>`
  + `<w:style w:type="paragraph" w:customStyle="1" w:styleId="MeuEstilo"><w:name w:val="Meu Estilo"/><w:aliases w:val="Cap,Apelido"/><w:basedOn w:val="Normal"/></w:style>`
  + `<w:style w:type="paragraph" w:customStyle="1" w:styleId="EstiloVazio"><w:name w:val="Estilo Vazio"/><w:basedOn w:val="Normal"/></w:style>`
  + `<w:style w:type="character" w:customStyle="1" w:styleId="Destaque"><w:name w:val="Destaque"/><w:rPr><w:i/></w:rPr></w:style>`;
const NUMBERING = `<w:abstractNum w:abstractNumId="0"><w:multiLevelType w:val="multilevel"/><w:lvl w:ilvl="0"><w:start w:val="1"/><w:numFmt w:val="decimal"/><w:pStyle w:val="Ttulo1"/><w:lvlText w:val="%1."/><w:lvlJc w:val="left"/><w:pPr><w:ind w:left="432" w:hanging="432"/></w:pPr></w:lvl>`
  + `<w:lvl w:ilvl="1"><w:start w:val="1"/><w:numFmt w:val="lowerLetter"/><w:lvlText w:val="%1.%2"/><w:lvlJc w:val="left"/><w:pPr><w:ind w:left="576" w:hanging="576"/></w:pPr></w:lvl></w:abstractNum><w:num w:numId="1"><w:abstractNumId w:val="0"/></w:num>`;
const h1 = (t) => p(r(t), '<w:pStyle w:val="Ttulo1"/>');
const txt = (t) => p(r(t));
const body = h1('Capitulo A') + txt('texto a1 Hxg') + h1('Capitulo B') + txt('texto b1 Hxg') + p(pb)
  + txt('texto p2 Hxg') + p(pb)
  + txt('texto p3 Hxg') + p(r('Paragrafo Meu'), '<w:pStyle w:val="MeuEstilo"/>') + h1('Capitulo C') + txt('texto c1 Hxg') + p(pb)
  + p(r('antes ') + '<w:r><w:rPr><w:rStyle w:val="Destaque"/></w:rPr><w:t>Marca Um</w:t></w:r>' + r(' depois Hxg')) + txt('fim Hxg');
const SECT = '<w:sectPr><w:pgSz w:w="11906" w:h="31680"/><w:pgMar w:top="1417" w:right="1701" w:bottom="1417" w:left="1701" w:header="708" w:footer="708" w:gutter="0"/><w:cols w:space="708"/><w:docGrid w:linePitch="360"/></w:sectPr>';

const GROUPS = {
  // names
  n: ['STYLEREF "Título 2"', 'STYLEREF "Título 3"', 'STYLEREF "Inexistente"', 'STYLEREF "título 1"', 'STYLEREF "Titulo 1"', 'STYLEREF "Heading 1"',
    'STYLEREF Ttulo1', 'STYLEREF "Meu Estilo"', 'STYLEREF "Estilo Vazio"', 'STYLEREF "meu estilo"', 'STYLEREF Cap', 'STYLEREF 2', 'STYLEREF 3'],
  // switches
  s: ['STYLEREF 1', 'STYLEREF 1 \\n', 'STYLEREF 1 \\r', 'STYLEREF 1 \\w', 'STYLEREF 1 \\s', 'STYLEREF 1 \\l \\n', 'STYLEREF 1 \\t', 'STYLEREF "Título 1" \\n',
    'STYLEREF "Meu Estilo" \\l', 'STYLEREF Destaque \\l', 'STYLEREF 1 \\p'],
};
export const cases = [];
for (const [k, list] of Object.entries(GROUPS)) {
  const hdr = list.map((f, i) => p(r(`${k}${i}= `) + fld(f) + r(' Hxg'))).join('');
  const ftr = p(r('rodape ') + fld(list[0]) + r(' Hxg'));
  cases.push({ name: `sr-${k}`, docx: buildDocx({ body, sect: SECT, styles: STYLES, numbering: NUMBERING, headers: { default: hdr }, footers: { default: ftr }, defaults: DEF }) });
}
// in the body: by name on a page without the heading (nearest before), before the first "Meu Estilo" (forward), char style
{
  const b = h1('Capitulo A') + txt('texto Hxg') + p(pb)
    + p(r('n1= ') + fld('STYLEREF "Título 1"') + r(' n2= ') + fld('STYLEREF "Meu Estilo"') + r(' n3= ') + fld('STYLEREF Destaque') + r(' n4= ') + fld('STYLEREF "Inexistente"') + r(' n5= ') + fld('STYLEREF "Título 2"') + r(' Hxg'))
    + p(r('Paragrafo Meu'), '<w:pStyle w:val="MeuEstilo"/>') + p(r('x ') + '<w:r><w:rPr><w:rStyle w:val="Destaque"/></w:rPr><w:t>Marca Dois</w:t></w:r>');
  cases.push({ name: 'sr-body', docx: buildDocx({ body: b, styles: STYLES, numbering: NUMBERING, defaults: DEF }) });
}
