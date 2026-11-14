// WordCraft study (2026-11-13), section B — fields, second batch: b22-pt-BR crashed Word 365 on PDF export and on
// Fields.Update (RPC 0x800706BE). One pt-BR document per general switch (pages 1, 21, 112) to find the culprit, and
// the B23–B25 cases again (they failed only because the crash took Word down). Run with --fresh.
// (Case names differ beyond letter case: the .docx/.pdf names collide on Windows otherwise.)
import { buildDocx, docDefaults, p, r } from '../docx-builder.mjs';
import { cases as first } from './wcbfld.mjs';

const fld = (instr, cached = 'C?') => `<w:r><w:fldChar w:fldCharType="begin"/></w:r><w:r><w:instrText xml:space="preserve"> ${instr} </w:instrText></w:r><w:r><w:fldChar w:fldCharType="separate"/></w:r><w:r><w:t>${cached}</w:t></w:r><w:r><w:fldChar w:fldCharType="end"/></w:r>`;
const SP = (start) => `<w:sectPr><w:pgSz w:w="11906" w:h="16838"/><w:pgMar w:top="1417" w:right="1701" w:bottom="1417" w:left="1701" w:header="708" w:footer="708" w:gutter="0"/><w:pgNumType w:start="${start}"/><w:cols w:space="708"/><w:docGrid w:linePitch="360"/></w:sectPr>`;
const SW = [
  ['ar', ''], ['rlo', '\\* roman'], ['rup', '\\* ROMAN'], ['ct', '\\* CardText'], ['ot', '\\* OrdText'], ['o', '\\* Ordinal'],
  ['ad', '\\* ArabicDash'], ['dt', '\\* DollarText'], ['hx', '\\* Hex'], ['al', '\\* alphabetic'],
  ['ctc', '\\* CardText \\* Caps'], ['ctu', '\\* CardText \\* Upper'], ['otf', '\\* OrdText \\* FirstCap'], ['adr', '\\* ArabicDash \\* roman'],
];
export const cases = [];
for (const [k, s] of SW) {
  const para = (n) => p(r(`${k}${n}= `) + fld(`PAGE ${s}`) + r(' Hxg'));
  const body = para(1) + `<w:p><w:pPr>${SP(1)}</w:pPr></w:p>` + para(21) + `<w:p><w:pPr>${SP(21)}</w:pPr></w:p>` + para(112);
  cases.push({ name: `b22pt-${k}`, docx: buildDocx({ body, sect: SP(112), defaults: docDefaults({ font: 'Liberation Sans', size: 11, lang: 'pt-BR' }) }) });
}
// the other fields of b22 in pt-BR
{
  const body = p(r('np= ') + fld('NUMPAGES \\* CardText') + r(' sp= ') + fld('SECTIONPAGES \\* roman') + r(' Hxg'))
    + p(r('sq= ') + fld('SEQ Fig \\* CardText') + r(' sqo= ') + fld('SEQ Fig \\* OrdText') + r(' Hxg'));
  cases.push({ name: 'b22pt-other', docx: buildDocx({ body, sect: SP(112), defaults: docDefaults({ font: 'Liberation Sans', size: 11, lang: 'pt-BR' }) }) });
}
for (const c of first) if (/^b2[345]-/.test(c.name)) cases.push(c);
