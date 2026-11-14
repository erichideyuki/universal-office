// WordCraft study (2026-11-13), planner "fields" — last details of the \\* general switches (Fields.Update dump):
// lower-case hex, rounding of non-integers, zero and limits per format, DollarText limits (en-US; es-ES; pt-BR without
// DollarText, which crashes Word 365 pt-BR).
import { buildDocx, docDefaults, p, r } from '../docx-builder.mjs';

const fld = (instr) => `<w:r><w:fldChar w:fldCharType="begin"/></w:r><w:r><w:instrText xml:space="preserve"> ${instr} </w:instrText></w:r><w:r><w:fldChar w:fldCharType="separate"/></w:r><w:r><w:t>C?</w:t></w:r><w:r><w:fldChar w:fldCharType="end"/></w:r>`;
const F = ['= 255 \\* hex', '= 255 \\* HEX', '= 255 \\* Hex \\* Lower', '= 3,5 \\* CardText', '= 2,4 \\* CardText', '= 2,5 \\* roman', '= 3,5 \\* Ordinal', '= 0,4 \\* CardText',
  '= -0,4 \\* CardText', '= 0 \\* OrdText', '= 0 \\* Ordinal', '= 0 \\* alphabetic', '= 0 \\* Hex', '= 0 \\* Arabic', '= -3 \\* Arabic', '= -3 \\* ArabicDash', '= -3 \\* Hex',
  '= -3 \\* alphabetic', '= -3 \\* Ordinal', '= 2,5 \\* Arabic', '= 2,5 \\* ArabicDash', '= 2,5 \\* Hex', '= 999999 \\* OrdText', '= 1000000 \\* roman', '= 1000000 \\* Hex',
  '= 16777216 \\* Hex', '= 1000000 \\* alphabetic', '= 32767 \\* ROMAN', '= 1000000 \\* DollarText', '= 999999,99 \\* DollarText', '= -5 \\* DollarText', '= 0 \\* DollarText',
  '= 2,999 \\* DollarText', '= 2,005 \\* DollarText', '= 21 \\* CardText \\* Upper \\* Lower', '= 21 \\* Lower \\* CardText', '= 21 \\* Caps \\* FirstCap'];
export const cases = [];
for (const lang of ['en-US', 'es-ES', 'pt-BR']) {
  const list = lang === 'pt-BR' ? F.filter((f) => !f.includes('DollarText')) : F;
  cases.push({ name: `swx-${lang}`, docx: buildDocx({ body: list.map((f, k) => p(r(`s${k}: `) + fld(f))).join(''), defaults: docDefaults({ font: 'Liberation Sans', size: 11, lang }) }) });
}
