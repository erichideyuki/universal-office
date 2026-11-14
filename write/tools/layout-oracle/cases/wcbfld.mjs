// WordCraft study (2026-11-13), section B — fields. Measured into core/target/wordcraft-study/B/wcbfld (--fixdir), and
// the results after Fields.Update by core/target/wordcraft-study/B/tools/fields-dump.ps1.
//  B21 STYLEREF in the header, per page: style name (built-in English / localized), \l, number, character style, \* Upper
//  B22 \* general switches on PAGE / NUMPAGES / SECTIONPAGES / PAGEREF / SEQ, in pt-BR, en-US, es-ES (pages 1, 21, 112)
//  B23 SECTIONPAGES with continuous sections; NUMPAGES with a gallery cover page
//  B24 REF \r (relative context) and \p (above/below) per language
//  B25 formula INT(-2,5) (trunc or floor), \# "0%" — in the footer (Word recomputes footer formulas on export)
// Liberation Sans 11; invented text; cached results deliberately wrong ("C?").
import { buildDocx, docDefaults, p, r, sectPr } from '../docx-builder.mjs';

const fld = (instr, cached = 'C?', rPr = '') => `<w:r>${rPr ? `<w:rPr>${rPr}</w:rPr>` : ''}<w:fldChar w:fldCharType="begin"/></w:r><w:r>${rPr ? `<w:rPr>${rPr}</w:rPr>` : ''}<w:instrText xml:space="preserve"> ${instr.replace(/&/g, '&amp;').replace(/</g, '&lt;').replace(/"/g, '&quot;')} </w:instrText></w:r><w:r>${rPr ? `<w:rPr>${rPr}</w:rPr>` : ''}<w:fldChar w:fldCharType="separate"/></w:r><w:r>${rPr ? `<w:rPr>${rPr}</w:rPr>` : ''}<w:t>${cached}</w:t></w:r><w:r>${rPr ? `<w:rPr>${rPr}</w:rPr>` : ''}<w:fldChar w:fldCharType="end"/></w:r>`;
const pb = '<w:r><w:br w:type="page"/></w:r>';
const bm = (id, name, inner) => `<w:bookmarkStart w:id="${id}" w:name="${name}"/>${inner}<w:bookmarkEnd w:id="${id}"/>`;
const DEF = (lang = 'pt-BR') => docDefaults({ font: 'Liberation Sans', size: 11, lang });
const H = `<w:style w:type="paragraph" w:styleId="Ttulo1"><w:name w:val="heading 1"/><w:basedOn w:val="Normal"/><w:next w:val="Normal"/><w:uiPriority w:val="9"/><w:qFormat/><w:pPr><w:keepNext/><w:outlineLvl w:val="0"/></w:pPr><w:rPr><w:b/></w:rPr></w:style>`
  + `<w:style w:type="paragraph" w:styleId="Ttulo2"><w:name w:val="heading 2"/><w:basedOn w:val="Normal"/><w:next w:val="Normal"/><w:uiPriority w:val="9"/><w:qFormat/><w:pPr><w:keepNext/><w:outlineLvl w:val="1"/></w:pPr></w:style>`
  + `<w:style w:type="character" w:customStyle="1" w:styleId="Destaque"><w:name w:val="Destaque"/><w:rPr><w:i/></w:rPr></w:style>`;
const h1 = (t) => p(r(t), '<w:pStyle w:val="Ttulo1"/>');
const h2 = (t) => p(r(t), '<w:pStyle w:val="Ttulo2"/>');
const txt = (t) => p(r(t));
// a sectPr in schema order (type, pgSz, pgMar, pgNumType, cols, titlePg, docGrid)
const SP = ({ type, start, titlePg, ftr } = {}) => `<w:sectPr>${ftr ? '<w:footerReference w:type="default" r:id="rId_footer_default"/>' : ''}${type ? `<w:type w:val="${type}"/>` : ''}<w:pgSz w:w="11906" w:h="16838"/><w:pgMar w:top="1417" w:right="1701" w:bottom="1417" w:left="1701" w:header="708" w:footer="708" w:gutter="0"/>`
  + `${start !== undefined ? `<w:pgNumType w:start="${start}"/>` : ''}<w:cols w:space="708"/>${titlePg ? '<w:titlePg/>' : ''}<w:docGrid w:linePitch="360"/></w:sectPr>`;
const sectP = (o) => `<w:p><w:pPr>${SP(o)}</w:pPr></w:p>`;

export const cases = [];

// ── B21 ──
{
  // p1: A, B (two headings); p2: none; p3: text then C; p4: char-styled run "Marca Um", then D (level 2)
  const body = h1('Capitulo A') + txt('texto a1 Hxg') + h1('Capitulo B') + txt('texto b1 Hxg') + p(pb)
    + txt('texto p2 Hxg') + txt(`corpo ${'x'}`) + p(pb)
    + txt('texto p3 Hxg') + h1('Capitulo C') + txt('texto c1 Hxg') + p(pb)
    + p(r('antes ') + `<w:r><w:rPr><w:rStyle w:val="Destaque"/></w:rPr><w:t>Marca Um</w:t></w:r>` + r(' depois Hxg')) + h2('Secao D') + txt('fim Hxg');
  for (const [name, instr] of [
    ['b21-name', 'STYLEREF "heading 1"'],
    ['b21-name-l', 'STYLEREF "heading 1" \\l'],
    ['b21-ptname', 'STYLEREF "Título 1"'],
    ['b21-num', 'STYLEREF 1'],
    ['b21-num-l', 'STYLEREF 1 \\l'],
    ['b21-char', 'STYLEREF Destaque'],
    ['b21-upper', 'STYLEREF "heading 1" \\* Upper'],
    ['b21-h2', 'STYLEREF "heading 2"'],
  ]) {
    const hdr = p(r('Cab ') + fld(instr, 'CACHEX') + r(' Hxg'));
    cases.push({ name, docx: buildDocx({ body, styles: H, headers: { default: hdr }, defaults: DEF() }) });
  }
  // in the body (page 2, no heading on the page): by name and by number
  const body2 = h1('Capitulo A') + txt('texto Hxg') + p(pb) + p(r('nome ') + fld('STYLEREF "heading 1"', 'CACHEX') + r(' num ') + fld('STYLEREF 1', 'CACHEY') + r(' Hxg')) + h1('Capitulo B');
  cases.push({ name: 'b21-body', docx: buildDocx({ body: body2, styles: H, defaults: DEF() }) });
}

// ── B22 ──
const SW = [
  ['ar', ''], ['r', '\\* roman'], ['R', '\\* ROMAN'], ['ct', '\\* CardText'], ['ot', '\\* OrdText'], ['o', '\\* Ordinal'],
  ['ad', '\\* ArabicDash'], ['dt', '\\* DollarText'], ['hx', '\\* Hex'], ['hxl', '\\* hex \\* Lower'], ['al', '\\* alphabetic'],
  ['AL', '\\* ALPHABETIC'], ['ctc', '\\* CardText \\* Caps'], ['ctu', '\\* CardText \\* Upper'], ['otf', '\\* OrdText \\* FirstCap'],
  ['adr', '\\* ArabicDash \\* roman'], ['ra', '\\* roman \\* ArabicDash'],
];
for (const lang of ['pt-BR', 'en-US', 'es-ES']) {
  const sec = (start, last) => {
    const ps = SW.map(([k, s]) => p(r(`${k}= `) + fld(`PAGE ${s}`) + r(' Hxg'))).join('');
    const extra = p(r('np= ') + fld('NUMPAGES \\* CardText') + r(' sp= ') + fld('SECTIONPAGES \\* roman') + r(' Hxg'))
      + p(r('pr= ') + fld('PAGEREF alvo \\* roman') + r(' prc= ') + fld('PAGEREF alvo \\* CardText') + r(' Hxg'))
      + p(r('sq= ') + fld('SEQ Fig \\* CardText') + r(' sqo= ') + fld('SEQ Fig \\* OrdText') + r(' Hxg'));
    return ps + extra + (last ? p(bm(1, 'alvo', r('Alvo Hxg'))) : sectP({ start }));
  };
  const body = sec(1) + sec(21) + sec(112, true);
  cases.push({ name: `b22-${lang}`, docx: buildDocx({ body, sect: SP({ start: 112 }), defaults: DEF(lang) }) });
}

// ── B23 ──
{
  const fill = (n, w) => Array.from({ length: n }, (_, k) => txt(`${w} ${k + 1} Hxg`)).join('');
  const ftr = p(r('P ') + fld('PAGE') + r(' S ') + fld('SECTIONPAGES') + r(' N ') + fld('NUMPAGES') + r(' sec ') + fld('SECTION') + r(' Hxg'));
  // sec 1: 80 lines (page 1 + half of page 2); sec 2 starts CONTINUOUS (its own sectPr holds the type), 80 lines;
  // sec 3 next page, 20 lines
  for (const [name, restart] of [['b23-cont', false], ['b23-cont-restart', true]]) {
    const body = fill(80, 'um') + sectP({ ftr: true }) + fill(80, 'dois') + sectP({ ftr: true, type: 'continuous', start: restart ? 1 : undefined }) + fill(20, 'tres');
    cases.push({ name, docx: buildDocx({ body, sect: SP({}), footers: { default: ftr }, defaults: DEF() }) });
  }
  // NUMPAGES with a gallery cover page (sdt docPartGallery "Cover Pages") and pgNumType start 0, titlePg
  const cover = (gallery) => (gallery ? '<w:sdt><w:sdtPr><w:id w:val="1"/><w:docPartObj><w:docPartGallery w:val="Cover Pages"/><w:docPartUnique/></w:docPartObj></w:sdtPr><w:sdtContent>' : '')
    + txt('Capa Hxg') + p(pb) + (gallery ? '</w:sdtContent></w:sdt>' : '');
  for (const [name, gallery, start] of [['b23-cover', true, 0], ['b23-cover-nogal', false, 0], ['b23-cover-start1', true, 1]]) {
    const ftr2 = p(r('P ') + fld('PAGE') + r(' de ') + fld('NUMPAGES') + r(' Hxg'));
    const body = cover(gallery) + txt('pag dois Hxg') + p(pb) + txt('pag tres Hxg');
    cases.push({ name, docx: buildDocx({ body, sect: SP({ start, titlePg: true }), footers: { default: ftr2 }, defaults: DEF() }) });
  }
}

// ── B24 ──
{
  const lvls = Array.from({ length: 3 }, (_, i) => `<w:lvl w:ilvl="${i}"><w:start w:val="1"/><w:numFmt w:val="${['decimal', 'lowerLetter', 'lowerRoman'][i]}"/><w:lvlText w:val="${['%1.', '%2)', '%3.'][i]}"/><w:lvlJc w:val="left"/><w:pPr><w:ind w:left="${720 + 360 * i}" w:hanging="360"/></w:pPr></w:lvl>`).join('');
  const numbering = `<w:abstractNum w:abstractNumId="1"><w:multiLevelType w:val="multilevel"/>${lvls}</w:abstractNum><w:num w:numId="1"><w:abstractNumId w:val="1"/></w:num>`;
  const it = (inner, l) => `<w:p><w:pPr><w:numPr><w:ilvl w:val="${l}"/><w:numId w:val="1"/></w:numPr></w:pPr>${inner}</w:p>`;
  for (const lang of ['pt-BR', 'en-US', 'es-ES']) {
    // 1. / a) / i.(target) / ii.(refs) / b) (refs) / 2. (refs)
    const refs = (k) => r(`${k} r=`) + fld('REF alvo \\r \\h') + r(' n=') + fld('REF alvo \\n \\h') + r(' w=') + fld('REF alvo \\w \\h') + r(' p=') + fld('REF alvo \\p \\h') + r(' Hxg');
    const body = p(refs('topo')) + it(r('um Hxg'), 0) + it(r('letra a Hxg'), 1) + it(bm(1, 'alvo', r('Alvo Hxg')), 2) + it(refs('irmao'), 2)
      + it(refs('letra b'), 1) + it(refs('dois'), 0) + p(refs('fim'));
    cases.push({ name: `b24-${lang}`, docx: buildDocx({ body, numbering, defaults: DEF(lang) }) });
  }
}

// ── B25 ── (footer formulas; pt-BR uses "," and ";", en-US ".")
for (const [lang, F] of [['pt-BR', ['= INT(-2,5)', '= INT(2,7)', '= INT(-0,5)', '= 0,256 \\# "0%"', '= 25 \\# "0%"', '= -2,5 \\# "0"', '= ROUND(-2,5;0)', '= MOD(-7;3)', '= ABS(-2,5)']],
  ['en-US', ['= INT(-2.5)', '= INT(2.7)', '= INT(-0.5)', '= 0.256 \\# "0%"', '= 25 \\# "0%"', '= -2.5 \\# "0"', '= ROUND(-2.5,0)', '= MOD(-7,3)', '= ABS(-2.5)']]]) {
  const ftr = F.map((f, k) => p(r(`f${k}= `) + fld(f) + r(' Hxg'))).join('');
  const body = F.map((f, k) => p(r(`b${k}= `) + fld(f) + r(' Hxg'))).join('');
  cases.push({ name: `b25-${lang}`, docx: buildDocx({ body, footers: { default: ftr }, defaults: DEF(lang) }) });
}
