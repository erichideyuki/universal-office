// WordCraft study (2026-11-13), planner "fields" — number words, a larger sample for the native fixture of the plan's
// number-to-words rules (wcfld3 nw-* showed Word's quirks: pt "Cinqüenta", "Mil e cento", "Dois milésimos", "Cento
// milésimo"; es "Ciento", " tricentésimo", "Dosmilésimo"…). Every N in 1..1100, 600 more drawn log-uniformly from
// 1101..999999 (mulberry32, seed 20261113), CardText and OrdText per language, Ordinal in en-US; DollarText (en-US,
// es-ES) over 200 values with cents. Read with Fields.Update (B/tools/fields-dump.ps1 under word-lock).
import { buildDocx, docDefaults, p, r } from '../docx-builder.mjs';

const fld = (instr) => `<w:r><w:fldChar w:fldCharType="begin"/></w:r><w:r><w:instrText xml:space="preserve"> ${instr} </w:instrText></w:r><w:r><w:fldChar w:fldCharType="separate"/></w:r><w:r><w:t>C?</w:t></w:r><w:r><w:fldChar w:fldCharType="end"/></w:r>`;
function mulberry32(a) { return () => { a |= 0; a = (a + 0x6d2b79f5) | 0; let t = Math.imul(a ^ (a >>> 15), 1 | a); t = (t + Math.imul(t ^ (t >>> 7), 61 | t)) ^ t; return ((t ^ (t >>> 14)) >>> 0) / 4294967296; }; }
const rnd = mulberry32(20261113);
const big = new Set();
while (big.size < 600) big.add(Math.floor(Math.exp(Math.log(1101) + rnd() * (Math.log(999999) - Math.log(1101)))));
export const NUMS = [...Array.from({ length: 1100 }, (_, i) => i + 1), ...[...big].sort((a, b) => a - b)];
const cents = Array.from({ length: 200 }, () => `${Math.floor(Math.exp(rnd() * Math.log(999999)))},${String(Math.floor(rnd() * 100)).padStart(2, '0')}`);
export const cases = [];
const chunk = (list, n) => Array.from({ length: Math.ceil(list.length / n) }, (_, i) => list.slice(i * n, i * n + n));
for (const lang of ['pt-BR', 'en-US', 'es-ES']) {
  const fmts = lang === 'en-US' ? ['CardText', 'OrdText', 'Ordinal', 'DollarText'] : lang === 'es-ES' ? ['CardText', 'OrdText', 'DollarText'] : ['CardText', 'OrdText'];
  for (const f of fmts) {
    const list = f === 'DollarText' ? cents : NUMS;
    chunk(list, 600).forEach((part, k) => {
      const body = part.map((n) => p(r(`${n}: `) + fld(`= ${n} \\* ${f}`))).join('');
      cases.push({ name: `nwb-${lang}-${f.toLowerCase()}-${k}`, docx: buildDocx({ body, defaults: docDefaults({ font: 'Liberation Sans', size: 11, lang }) }) });
    });
  }
}
