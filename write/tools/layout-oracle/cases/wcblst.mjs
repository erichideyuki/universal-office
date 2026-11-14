// WordCraft study (2026-11-13), section B — list numbers. Measured into core/target/wordcraft-study/B/wcblst (--fixdir).
//  B26 numFmt cardinalText / ordinalText / ordinal in pt-BR, en-US and es-ES, items 1, 21, 112 (and 1000):
//      which language decides (document default, paragraph mark, run)?
//  B27 isLgl (with upperRoman / lowerLetter / decimalZero levels) and startOverride cases WordCraft infers:
//      override of a shared abstract list restarts only its own level; a deeper item first counts the level above;
//      startOverride 0; a num used again after another num of the same abstract list.
//  B28 (= A21) number suffix: the tab when the number ends exactly at / just before the stop; the space suffix width.
// Liberation Sans 11; suffix "space" for B26 so the extracted line is the number + text.
import { buildDocx, docDefaults, p, r } from '../docx-builder.mjs';

const lvl = (i, { fmt = 'decimal', text, start = 1, suff = 'space', isLgl = false, left, hanging = 360, tab } = {}) =>
  `<w:lvl w:ilvl="${i}"><w:start w:val="${start}"/><w:numFmt w:val="${fmt}"/>${isLgl ? '<w:isLgl/>' : ''}<w:suff w:val="${suff}"/>`
  + `<w:lvlText w:val="${text ?? `%${i + 1}.`}"/><w:lvlJc w:val="left"/><w:pPr>${tab ? `<w:tabs><w:tab w:val="num" w:pos="${tab}"/></w:tabs>` : ''}<w:ind w:left="${left ?? 720 + 360 * i}" w:hanging="${hanging}"/></w:pPr></w:lvl>`;
const abs = (id, levels) => `<w:abstractNum w:abstractNumId="${id}"><w:multiLevelType w:val="multilevel"/>${levels.join('')}</w:abstractNum>`;
const num = (numId, absId, ovr = '') => `<w:num w:numId="${numId}"><w:abstractNumId w:val="${absId}"/>${ovr}</w:num>`;
const NUMPR = (numId, ilvl = 0) => `<w:numPr><w:ilvl w:val="${ilvl}"/><w:numId w:val="${numId}"/></w:numPr>`;
const item = (t, numId, ilvl = 0, extra = '', rPr = '') => p(r(t, rPr), NUMPR(numId, ilvl) + extra);

export const cases = [];

// ── B26 ──
const FMTS = ['cardinalText', 'ordinalText', 'ordinal'];
const STARTS = [1, 21, 112, 1000];
{
  let numbering = '';
  let id = 1;
  const plan = [];
  for (const fmt of FMTS) for (const s of STARTS) {
    numbering += abs(id, [lvl(0, { fmt, start: s, text: '%1' })]);
    plan.push([fmt, s, id]);
    id++;
  }
  numbering += plan.map(([, , a]) => num(a, a)).join('');
  const body = (mark) => plan.map(([fmt, s, n]) => item(`${fmt.slice(0, 3)}${s} Hxg`, n, 0, mark)).join('');
  for (const lang of ['pt-BR', 'en-US', 'es-ES']) {
    cases.push({ name: `b26-${lang}`, docx: buildDocx({ body: body(''), numbering, defaults: docDefaults({ font: 'Liberation Sans', size: 11, lang }) }) });
  }
  // the document in pt-BR, the paragraph MARK in en-US / es-ES (the number takes the mark's run properties)
  for (const lang of ['en-US', 'es-ES']) {
    cases.push({ name: `b26-mark-${lang}`, docx: buildDocx({ body: body(`<w:rPr><w:lang w:val="${lang}"/></w:rPr>`), numbering, defaults: docDefaults({ font: 'Liberation Sans', size: 11, lang: 'pt-BR' }) }) });
  }
  // the document in pt-BR, only the TEXT run in en-US
  {
    const b = plan.map(([fmt, s, n]) => item(`${fmt.slice(0, 3)}${s} Hxg`, n, 0, '', '<w:lang w:val="en-US"/>')).join('');
    cases.push({ name: 'b26-run-en-US', docx: buildDocx({ body: b, numbering, defaults: docDefaults({ font: 'Liberation Sans', size: 11, lang: 'pt-BR' }) }) });
  }
  // the level's own rPr (numbering lvl rPr lang)
  {
    const nb = numbering.replace(/<w:pPr>/g, '<w:rPr><w:lang w:val="en-US"/></w:rPr><w:pPr>');
    cases.push({ name: 'b26-lvlrpr-en-US', docx: buildDocx({ body: body(''), numbering: nb, defaults: docDefaults({ font: 'Liberation Sans', size: 11, lang: 'pt-BR' }) }) });
  }
}

// ── B27 ──
const DEF = docDefaults({ font: 'Liberation Sans', size: 11 });
const L9 = (over = {}) => Array.from({ length: 9 }, (_, i) => lvl(i, { text: Array.from({ length: i + 1 }, (_, k) => `%${k + 1}.`).join(''), ...(over[i] ?? {}) }));
const ALL = (v) => Array.from({ length: 9 }, (_, i) => `<w:lvlOverride w:ilvl="${i}"><w:startOverride w:val="${v}"/></w:lvlOverride>`).join('');
// isLgl on level 2 (index 1) with level 1 upperRoman; own level lowerLetter; level 1 decimalZero
cases.push({ name: 'b27-lgl-roman', docx: buildDocx({ defaults: DEF, numbering: abs(1, L9({ 0: { fmt: 'upperRoman' }, 1: { isLgl: true } })) + num(1, 1),
  body: item('um Hxg', 1) + item('um.um Hxg', 1, 1) + item('dois Hxg', 1) + item('dois.um Hxg', 1, 1) + item('nivel3 Hxg', 1, 2) }) });
cases.push({ name: 'b27-lgl-letter', docx: buildDocx({ defaults: DEF, numbering: abs(1, L9({ 0: { fmt: 'upperRoman' }, 1: { fmt: 'lowerLetter', isLgl: true } })) + num(1, 1),
  body: item('um Hxg', 1) + item('um.um Hxg', 1, 1) + item('um.dois Hxg', 1, 1) }) });
cases.push({ name: 'b27-lgl-zero', docx: buildDocx({ defaults: DEF, numbering: abs(1, L9({ 0: { fmt: 'decimalZero' }, 1: { fmt: 'upperLetter', isLgl: true } })) + num(1, 1),
  body: item('um Hxg', 1) + item('um.um Hxg', 1, 1) }) });
// isLgl on a level whose text does not show the level above (only %2)
cases.push({ name: 'b27-lgl-own', docx: buildDocx({ defaults: DEF, numbering: abs(1, L9({ 0: { fmt: 'upperRoman' }, 1: { fmt: 'upperRoman', isLgl: true, text: '%2)' } })) + num(1, 1),
  body: item('um Hxg', 1) + item('um.um Hxg', 1, 1) + item('um.dois Hxg', 1, 1) }) });
// shared abstract list: num 1 for chapters (level 0), num 2 with startOverride 1 on every level, first used at level 4
cases.push({ name: 'b27-ovr-shared', docx: buildDocx({ defaults: DEF, numbering: abs(1, L9()) + num(1, 1) + num(2, 1, ALL(1)),
  body: item('cap um Hxg', 1) + item('cap dois Hxg', 1) + item('fundo a Hxg', 2, 4) + item('fundo b Hxg', 2, 4) + item('cap tres Hxg', 1) + item('sub Hxg', 1, 1) }) });
// num 2 (override 5 on level 0) used, then num 1, then num 2 again
cases.push({ name: 'b27-ovr-again', docx: buildDocx({ defaults: DEF, numbering: abs(1, L9()) + num(1, 1) + num(2, 1, '<w:lvlOverride w:ilvl="0"><w:startOverride w:val="5"/></w:lvlOverride>'),
  body: item('n1 a Hxg', 1) + item('n1 b Hxg', 1) + item('n2 a Hxg', 2) + item('n2 b Hxg', 2) + item('n1 c Hxg', 1) + item('n2 c Hxg', 2) }) });
// a deeper item before any level-0 item; then level 0
cases.push({ name: 'b27-deep-first', docx: buildDocx({ defaults: DEF, numbering: abs(1, L9()) + num(1, 1),
  body: item('fundo a Hxg', 1, 1) + item('fundo b Hxg', 1, 1) + item('topo Hxg', 1) + item('fundo c Hxg', 1, 1) }) });
cases.push({ name: 'b27-deep-first-ovr', docx: buildDocx({ defaults: DEF, numbering: abs(1, L9()) + num(1, 1, '<w:lvlOverride w:ilvl="0"><w:startOverride w:val="4"/></w:lvlOverride>'),
  body: item('fundo a Hxg', 1, 1) + item('topo Hxg', 1) }) });
// startOverride 0
cases.push({ name: 'b27-ovr0', docx: buildDocx({ defaults: DEF, numbering: abs(1, L9()) + num(1, 1, ALL(0)),
  body: item('a Hxg', 1) + item('b Hxg', 1) + item('c Hxg', 1, 1) }) });
// override of level 1 only on num 2 used after num 1's level-0 items (does num 2 continue level 0?)
cases.push({ name: 'b27-ovr-lvl1', docx: buildDocx({ defaults: DEF, numbering: abs(1, L9()) + num(1, 1) + num(2, 1, '<w:lvlOverride w:ilvl="1"><w:startOverride w:val="7"/></w:lvlOverride>'),
  body: item('n1 a Hxg', 1) + item('n1 b Hxg', 1) + item('n2 sub Hxg', 2, 1) + item('n2 top Hxg', 2) + item('n2 sub2 Hxg', 2, 1) }) });

// ── B28 / A21: suffix tab with the number ending at the stop; suffix space width ──
// Liberation Sans (1/4096 pt floor): "1" at 8 pt = 88.984 tw, "1." at 25 pt = 416.992 tw. The number starts at
// left - hanging; hanging 89 / 417 puts its end 0.016 / 0.008 tw BEFORE the indent's stop (WordCraft: the stop must be
// past end + 0.01 pt = 0.2 tw, so it skips to the next stop; ours: the number may end on the stop)
for (const [size, text, hangs] of [[8, '%1', [88, 89, 90]], [25, '%1.', [416, 417, 418]]]) {
  for (const hang of hangs) {
    const nb = abs(1, [lvl(0, { text, suff: 'tab', left: 1440, hanging: hang })]) + num(1, 1);
    cases.push({ name: `b28-tab-${size}-${hang}`, docx: buildDocx({ defaults: docDefaults({ font: 'Liberation Sans', size }), numbering: nb, body: item('Texto Hxg', 1) + p(r('controle Hxg'), '<w:ind w:left="1440"/>') }) });
  }
}
for (const size of [11, 20]) {
  const nb = abs(1, [lvl(0, { text: '%1.', suff: 'space', left: 1440, hanging: 720 })]) + num(1, 1);
  cases.push({ name: `b28-space-${size}`, docx: buildDocx({ defaults: docDefaults({ font: 'Liberation Sans', size }), numbering: nb, body: item('Texto Hxg', 1) }) });
}
