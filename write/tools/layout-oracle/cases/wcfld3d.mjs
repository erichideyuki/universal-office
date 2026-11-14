// WordCraft study (2026-11-13), planner "fields" — number words, the STRUCTURE of the thousands (wcfld3b showed
// table-driven quirks: es "Veintidósmil", "Trescientossetenta y milésimo", "Quincientos…milésimo"; pt "Cento quatro
// mil", "Cento e um mil", "Cento milésimo"). Every thousands group g·1000 (g = 1..999, low part 0), and every low
// part l = 1..999 after g = 1, 22 and 371. CardText and OrdText per language, Ordinal in en-US. Fields.Update dump.
import { buildDocx, docDefaults, p, r } from '../docx-builder.mjs';

const fld = (instr) => `<w:r><w:fldChar w:fldCharType="begin"/></w:r><w:r><w:instrText xml:space="preserve"> ${instr} </w:instrText></w:r><w:r><w:fldChar w:fldCharType="separate"/></w:r><w:r><w:t>C?</w:t></w:r><w:r><w:fldChar w:fldCharType="end"/></w:r>`;
export const NUMS = [
  ...Array.from({ length: 999 }, (_, g) => (g + 1) * 1000),
  ...[1, 22, 371].flatMap((g) => Array.from({ length: 999 }, (_, l) => g * 1000 + l + 1)),
  // every head followed by a low part (the head can change with it: es 501000 'Quinientosun mil' but 501713 'Quinientos un mil')
  ...Array.from({ length: 999 }, (_, g) => (g + 1) * 1000 + 7).filter((n) => ![1007, 22007, 371007].includes(n)),
];
const chunk = (list, n) => Array.from({ length: Math.ceil(list.length / n) }, (_, i) => list.slice(i * n, i * n + n));
export const cases = [];
for (const lang of ['pt-BR', 'en-US', 'es-ES']) {
  const fmts = lang === 'en-US' ? ['CardText', 'OrdText', 'Ordinal'] : ['CardText', 'OrdText'];
  for (const f of fmts) {
    chunk(NUMS, 1000).forEach((part, k) => {
      const body = part.map((n) => p(r(`${n}: `) + fld(`= ${n} \\* ${f}`))).join('');
      cases.push({ name: `nwd-${lang}-${f.toLowerCase()}-${k}`, docx: buildDocx({ body, defaults: docDefaults({ font: 'Liberation Sans', size: 11, lang }) }) });
    });
  }
}
