// WordCraft study (2026-11-13), planner "fields" — measurements the plan depends on that B left open. Read through
// Fields.Update (core/target/wordcraft-study/B/tools/fields-dump.ps1 under tools/parallel/word-lock.mjs), not PDFs.
//  nw-<lang>-<fmt>-<k>  number words: `= N \* CardText|OrdText|Ordinal|DollarText` (formula fields use the same
//                       general switches as PAGE; DollarText is NOT asked in pt-BR: it crashes Word 365 pt-BR)
//  fl-*                 which language a field's \* words and REF \p follow (pt-BR document, en-US placed on one part)
//  sw-*                 switch-name letter case, non-integers, large roman/alphabetic/hex (en-US document)
//  sec-*                \* on PAGE/SECTION/SECTIONPAGES/NUMPAGES/PAGEREF in a section with pgNumType upperRoman
//  cs-*                 case switches on text results (REF, STYLEREF)
//  hf-*                 footer formulas: the functions, comparisons and Word's error texts (pt-BR UI)
// Liberation Sans 11; cached results deliberately wrong ("C?").
import { buildDocx, docDefaults, p, r } from '../docx-builder.mjs';

const fx = (instr) => instr.replace(/&/g, '&amp;').replace(/</g, '&lt;').replace(/>/g, '&gt;').replace(/"/g, '&quot;');
const fld = (instr, rp = {}) => {
  const rr = (k) => (rp[k] ? `<w:rPr>${rp[k]}</w:rPr>` : '');
  return `<w:r>${rr('begin')}<w:fldChar w:fldCharType="begin"/></w:r><w:r>${rr('instr')}<w:instrText xml:space="preserve"> ${fx(instr)} </w:instrText></w:r>`
    + `<w:r>${rr('sep')}<w:fldChar w:fldCharType="separate"/></w:r><w:r>${rr('res')}<w:t>C?</w:t></w:r><w:r>${rr('end')}<w:fldChar w:fldCharType="end"/></w:r>`;
};
const DEF = (lang) => docDefaults({ font: 'Liberation Sans', size: 11, lang });
const bm = (id, name, inner) => `<w:bookmarkStart w:id="${id}" w:name="${name}"/>${inner}<w:bookmarkEnd w:id="${id}"/>`;
const pb = '<w:r><w:br w:type="page"/></w:r>';
export const cases = [];

// ── number words ──
const NUMS = [0, ...Array.from({ length: 32 }, (_, i) => i + 1), 39, 40, 41, 50, 60, 70, 80, 90, 99, 100, 101, 102, 110, 111, 112, 113, 115, 119, 120,
  121, 150, 199, 200, 201, 211, 250, 300, 333, 400, 444, 500, 555, 600, 666, 700, 777, 800, 888, 900, 999, 1000, 1001, 1002, 1011, 1020, 1100,
  1101, 1111, 1200, 1999, 2000, 2001, 2002, 2020, 2100, 3000, 9999, 10000, 10001, 11000, 12345, 20000, 21000, 99999, 100000, 100001, 101000,
  200000, 999999, 1000000, 1000001, 2000000];
const DOLLAR = [1, 2, 21, 112, 1000, 2001, '14,55', '0,5', '1,01', '100,99'];
for (const lang of ['pt-BR', 'en-US', 'es-ES']) {
  for (const f of ['CardText', 'OrdText', 'Ordinal', 'DollarText']) {
    if (f === 'DollarText' && lang === 'pt-BR') continue;
    const list = f === 'DollarText' ? DOLLAR : NUMS;
    const body = list.map((n) => p(r(`${n}: `) + fld(`= ${n} \\* ${f}`))).join('');
    cases.push({ name: `nw-${lang}-${f.toLowerCase()}`, docx: buildDocx({ body, defaults: DEF(lang) }) });
  }
}

// ── field language (pt-BR document) ──
{
  const EN = '<w:lang w:val="en-US"/>';
  const target = p(bm(1, 'alvo', r('Alvo Hxg')));
  const trio = (rp) => r('a ') + fld('= 21 \\* CardText', rp) + r(' b ') + fld('PAGE \\* OrdText', rp) + r(' c ') + fld('REF alvo \\p', rp) + r(' d ') + fld('SEQ Fig \\* CardText', rp);
  const all = { begin: EN, instr: EN, sep: EN, res: EN, end: EN };
  const styles = '<w:style w:type="paragraph" w:customStyle="1" w:styleId="EnPara"><w:name w:val="En Para"/><w:basedOn w:val="Normal"/><w:rPr><w:lang w:val="en-US"/></w:rPr></w:style>'
    + '<w:style w:type="character" w:customStyle="1" w:styleId="EnChar"><w:name w:val="En Char"/><w:rPr><w:lang w:val="en-US"/></w:rPr></w:style>';
  const ENC = '<w:rStyle w:val="EnChar"/>';
  const variants = [
    ['fl-all', p(trio(all))],
    ['fl-mark', p(trio({}), `<w:rPr>${EN}</w:rPr>`)],
    ['fl-res', p(trio({ res: EN }))],
    ['fl-instr', p(trio({ instr: EN }))],
    ['fl-begin', p(trio({ begin: EN }))],
    ['fl-style', p(trio({}), '<w:pStyle w:val="EnPara"/>')],
    ['fl-charstyle', p(trio({ begin: ENC, instr: ENC, sep: ENC, res: ENC, end: ENC }))],
    ['fl-text', p(r('English text ', EN) + trio({}))],
    ['fl-none', p(trio({}))],
  ];
  for (const [name, para] of variants) cases.push({ name, docx: buildDocx({ body: target + para, styles, defaults: DEF('pt-BR') }) });
}

// ── switch names, non-integers, large values (en-US document) ──
{
  const F = ['= 21 \\* Roman', '= 21 \\* roman', '= 21 \\* ROMAN', '= 21 \\* cardtext', '= 21 \\* CARDTEXT', '= 21 \\* ORDTEXT', '= 21 \\* ordinal',
    '= 21 \\* Alphabetic', '= 21 \\* alphabetic', '= 21 \\* CardText \\* FIRSTCAP', '= 21 \\* CardText \\* caps', '= 21 \\* CardText \\* upper',
    '= 21 \\* ROMAN \\* Lower', '= 21 \\* Arabic', '= 21 \\* ArabicDash', '= 0 \\* ArabicDash', '= 0 \\* roman', '= 0 \\* CardText',
    '= 2,5 \\* CardText', '= -5 \\* CardText', '= -5 \\* roman', '= 255 \\* Hex', '= 4096 \\* hex', '= 27 \\* alphabetic', '= 52 \\* alphabetic',
    '= 53 \\* ALPHABETIC', '= 3999 \\* ROMAN', '= 4000 \\* ROMAN', '= 10000 \\* roman', '= 21 \\* Ordinal \\* Upper', '= 21 \\* OrdText \\* Caps',
    '= 1 \\* Ordinal', '= 2 \\* Ordinal', '= 3 \\* Ordinal', '= 11 \\* Ordinal', '= 12 \\* Ordinal', '= 13 \\* Ordinal', '= 111 \\* Ordinal',
    '= 21 \\* bogus', '= 21 \\* MERGEFORMAT', '= 21 \\* CardText \\* MERGEFORMAT'];
  cases.push({ name: 'sw-en', docx: buildDocx({ body: F.map((f, k) => p(r(`s${k}: `) + fld(f))).join(''), defaults: DEF('en-US') }) });
  const G = ['= 21 \\* Roman', '= 21 \\* cardtext', '= 21 \\* CardText \\* caps', '= 21 \\* OrdText \\* Caps', '= 21 \\* CardText \\* FirstCap',
    '= 0 \\* CardText', '= 2,5 \\* CardText', '= 1 \\* Ordinal', '= 1 \\* OrdText \\* Upper'];
  for (const lang of ['pt-BR', 'es-ES']) cases.push({ name: `sw-${lang}`, docx: buildDocx({ body: G.map((f, k) => p(r(`s${k}: `) + fld(f))).join(''), defaults: DEF(lang) }) });
}

// ── section page format (sec 2: pgNumType upperRoman start 21) ──
{
  const SP = (fmt, start) => `<w:sectPr><w:pgSz w:w="11906" w:h="16838"/><w:pgMar w:top="1417" w:right="1701" w:bottom="1417" w:left="1701" w:header="708" w:footer="708" w:gutter="0"/>`
    + `<w:pgNumType${fmt ? ` w:fmt="${fmt}"` : ''}${start ? ` w:start="${start}"` : ''}/><w:cols w:space="708"/><w:docGrid w:linePitch="360"/></w:sectPr>`;
  const F = ['PAGE', 'PAGE \\* Arabic', 'PAGE \\* CardText', 'PAGE \\* Lower', 'PAGE \\* Caps', 'PAGE \\* ArabicDash', 'PAGE \\* roman',
    'SECTION', 'SECTION \\* roman', 'SECTION \\* CardText', 'SECTIONPAGES \\* CardText', 'NUMPAGES \\* roman', 'PAGEREF alvo', 'PAGEREF alvo \\* Arabic',
    'PAGEREF alvo \\* CardText', 'PAGEREF alvo \\* Lower'];
  for (const lang of ['pt-BR', 'en-US']) {
    const body = p(r('sec one Hxg')) + `<w:p><w:pPr>${SP(null, null)}</w:pPr></w:p>` + p(bm(1, 'alvo', r('Alvo Hxg')))
      + F.map((f, k) => p(r(`f${k}: `) + fld(f))).join('');
    cases.push({ name: `sec-${lang}`, docx: buildDocx({ body, sect: SP('upperRoman', 21), defaults: DEF(lang) }) });
  }
}

// ── case switches on text results ──
{
  const styles = '<w:style w:type="paragraph" w:styleId="Ttulo1"><w:name w:val="heading 1"/><w:basedOn w:val="Normal"/><w:next w:val="Normal"/><w:uiPriority w:val="9"/><w:qFormat/><w:pPr><w:keepNext/><w:outlineLvl w:val="0"/></w:pPr></w:style>';
  const F = ['REF alvo', 'REF alvo \\* Upper', 'REF alvo \\* Lower', 'REF alvo \\* Caps', 'REF alvo \\* FirstCap', 'STYLEREF 1 \\* Upper', 'STYLEREF 1 \\* Caps',
    'STYLEREF "Título 1" \\* Lower', 'REF alvo \\* roman'];
  const body = p(r('capítulo de Teste ágil'), '<w:pStyle w:val="Ttulo1"/>') + p(bm(1, 'alvo', r('texto de Exemplo ágil-mente'))) + F.map((f, k) => p(r(`c${k}: `) + fld(f))).join('');
  cases.push({ name: 'cs-pt-BR', docx: buildDocx({ body, styles, defaults: DEF('pt-BR') }) });
}

// ── footer formulas (pt-BR UI): functions, comparisons, error texts ──
for (const lang of ['pt-BR', 'en-US']) {
  const F = ['= SUM(1;2;3)', '= AVERAGE(1;2)', '= MAX(1;5;3)', '= MIN(4;2)', '= PRODUCT(2;3)', '= SIGN(-2)', '= SIGN(0)', '= IF(1>0;5;6)', '= IF(1<0;5;6)',
    '= AND(1;0)', '= OR(0;1)', '= NOT(1)', '= TRUE', '= FALSE', '= COUNT(1;2;3)', '= DEFINED(nada)', '= 2^3', '= 1=1', '= 3<>3', '= 50%', '= 200*10%',
    '= INT(7,9)', '= ROUND(2,345;2)', '= ROUND(-2,345;2)', '= ROUND(1234,5;-2)', '= MOD(7;-3)', '= MOD(7,5;2)', '= ABS(-3)', '= 1/0', '= SUM(', '= 2 3',
    '= FOO(1)', '= nada', '= (1+2', '= 1,2,3', '= 1.000,5', '= 1.5', '= 3 \\# "0,00"', '= INT(-2.5)', '= SUM(1,2)', '= 1;2', '= int(2,5)', '= Round(2,5;0)',
    '= -2,5 \\# "0,0"', '= 1234,5 \\# "#.##0,00"', '= 0,5 \\# "0%"', '= 7 \\# "00%"', '= 5 \\* CardText', '= MAX()', '= 1+', '= )', '= 2*(3'];
  const ftr = F.map((f, k) => p(r(`f${k}= `) + fld(f))).join('');
  cases.push({ name: `hf-${lang}`, docx: buildDocx({ body: p(r('corpo Hxg')), footers: { default: ftr }, defaults: DEF(lang) }) });
}
