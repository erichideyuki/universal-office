// WordCraft study (2026-11-13), planner "fields" — list numbers B26/B27 left open, read through COM
// (Paragraph.Range.ListFormat.ListString, core/target/wordcraft-study/fields/tools/list-dump.ps1 under word-lock):
//  lw-<lang>   cardinalText / ordinalText / ordinal list numbers at chosen values (one w:num per value, startOverride),
//              to compare the list words with the field words (wcfld3 nw-*) beyond 1/21/112/1000
//  lgl-*       isLgl: own level formats (decimalZero, ordinalText, upperLetter) and parent formats (upperLetter,
//              cardinalText, decimalEnclosedCircle, ordinal, decimalZero) — which keep their own form
import { buildDocx, docDefaults, p, r } from '../docx-builder.mjs';

const DEF = (lang) => docDefaults({ font: 'Liberation Sans', size: 11, lang });
const lvl = (i, fmt, text, extra = '') => `<w:lvl w:ilvl="${i}"><w:start w:val="1"/><w:numFmt w:val="${fmt}"/>${extra}<w:lvlText w:val="${text}"/><w:lvlJc w:val="left"/><w:pPr><w:ind w:left="${720 + 360 * i}" w:hanging="360"/></w:pPr></w:lvl>`;
const item = (numId, ilvl, t) => p(r(t), `<w:numPr><w:ilvl w:val="${ilvl}"/><w:numId w:val="${numId}"/></w:numPr>`);
export const cases = [];

// ── list words at chosen values ──
const VALS = [11, 12, 13, 14, 15, 16, 17, 18, 19, 20, 30, 99, 100, 101, 200, 345, 999, 1000, 1001, 2000, 21000];
for (const lang of ['pt-BR', 'en-US', 'es-ES']) {
  let abs = '';
  let nums = '';
  let body = '';
  let id = 1;
  ['cardinalText', 'ordinalText', 'ordinal'].forEach((fmt, a) => {
    abs += `<w:abstractNum w:abstractNumId="${a}"><w:multiLevelType w:val="singleLevel"/>${lvl(0, fmt, '%1.')}</w:abstractNum>`;
    for (const v of VALS) {
      nums += `<w:num w:numId="${id}"><w:abstractNumId w:val="${a}"/><w:lvlOverride w:ilvl="0"><w:startOverride w:val="${v}"/></w:lvlOverride></w:num>`;
      body += item(id, 0, `${fmt} ${v} Hxg`);
      id++;
    }
  });
  cases.push({ name: `lw-${lang}`, docx: buildDocx({ body, numbering: abs + nums, defaults: DEF(lang) }) });
}

// ── isLgl ──
{
  const LGL = '<w:isLgl/>';
  const docs = [
    // [name, level 0 (fmt, text), level 1 (fmt, text, isLgl)]
    ['lgl-own-zero', ['decimal', '%1.'], ['decimalZero', '%1.%2', LGL]],
    ['lgl-own-ordtext', ['decimal', '%1.'], ['ordinalText', '%1.%2', LGL]],
    ['lgl-own-upper', ['decimal', '%1.'], ['upperLetter', '%1.%2', LGL]],
    ['lgl-own-only', ['decimal', '%1.'], ['upperRoman', '%2)', LGL]],
    ['lgl-par-upper', ['upperLetter', '%1.'], ['decimal', '%1.%2', LGL]],
    ['lgl-par-card', ['cardinalText', '%1.'], ['decimal', '%1.%2', LGL]],
    ['lgl-par-circle', ['decimalEnclosedCircle', '%1.'], ['decimal', '%1.%2', LGL]],
    ['lgl-par-ordinal', ['ordinal', '%1.'], ['decimal', '%1.%2', LGL]],
    ['lgl-par-zero-own-zero', ['decimalZero', '%1.'], ['decimalZero', '%1.%2', LGL]],
    ['lgl-l0', ['upperRoman', '%1.', LGL], ['lowerLetter', '%1.%2']],
    ['lgl-bullet-par', ['bullet', '•'], ['lowerLetter', '%1.%2', LGL]],
  ];
  for (const [name, l0, l1] of docs) {
    const numbering = `<w:abstractNum w:abstractNumId="0"><w:multiLevelType w:val="multilevel"/>${lvl(0, l0[0], l0[1], l0[2] ?? '')}${lvl(1, l1[0], l1[1], l1[2] ?? '')}</w:abstractNum><w:num w:numId="1"><w:abstractNumId w:val="0"/></w:num>`;
    const body = item(1, 0, 'um Hxg') + item(1, 1, 'um a Hxg') + item(1, 1, 'um b Hxg') + item(1, 0, 'dois Hxg') + item(1, 1, 'dois a Hxg')
      + Array.from({ length: 10 }, (_, k) => item(1, 1, `dois ${k} Hxg`)).join('');
    cases.push({ name, docx: buildDocx({ body, numbering, defaults: DEF('pt-BR') }) });
  }
}
