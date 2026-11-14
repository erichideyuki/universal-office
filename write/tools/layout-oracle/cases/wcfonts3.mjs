// WordCraft study (2026-11-13), planner "fields" — follow-ups of wcfonts2 (run with --fresh):
//  c2y-*  charset suffixes: is " Baltic" stripped for any family (wcfonts2: "Segoe UI Baltic" → Segoe UI, but
//         "Tahoma Greek"/"Verdana Cyr" → Calibri), and the parenthesised script names on other families
//  c5y-*  localized full names (pt-BR records: "Arial Negrito"…) and camel-case names without a separator
//  c6y-*  a missing charset-02 font: is Symbol vs Wingdings decided by w:family or by the PANOSE
//  c1y-*  multiple spacing over a run whose face has a larger ascent but a smaller descent than the paragraph's
//         (or the reverse): is the multiplied height max(ascent)+max(descent) or the largest run's own height
//  a2y-*  East Asian w:lang on the Latin text: Korean, Traditional Chinese, and a mixed paragraph
import { strToU8 } from 'fflate';
import { buildDocx, docDefaults, esc, p, r, sz } from '../docx-builder.mjs';

const W_NS = 'xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"';
const fontTable = (entries) => strToU8(`<?xml version="1.0" encoding="UTF-8" standalone="yes"?><w:fonts ${W_NS}>${entries}</w:fonts>`);
const fe = (name, { charset = '00', family = 'auto', panose, pitch = 'variable' } = {}) =>
  `<w:font w:name="${esc(name)}">${panose ? `<w:panose1 w:val="${panose}"/>` : ''}<w:charset w:val="${charset}"/><w:family w:val="${family}"/><w:pitch w:val="${pitch}"/></w:font>`;
const FT_BASE = fe('Calibri', { family: 'swiss', panose: '020F0502020204030204' }) + fe('Times New Roman', { family: 'roman', panose: '02020603050405020304' });
const rf = (f) => `<w:rFonts w:ascii="${esc(f)}" w:hAnsi="${esc(f)}" w:cs="${esc(f)}"/>`;
const TEXT = 'Hxg WMwm iIl1 0O quick brown fox jumps over the lazy dog';
const LONG = `${TEXT}; ${TEXT}; ${TEXT}.`;
export const cases = [];
const add = (name, body, { font = 'Calibri', size = 11, line = 240, ft = '', lang = 'en-US', defaults } = {}) => cases.push({
  name,
  docx: buildDocx({ body, defaults: defaults ?? docDefaults({ font, size, after: 0, line, lang }), extraParts: { 'word/fontTable.xml': fontTable(FT_BASE + ft) } }),
});
const slug = (s) => s.toLowerCase().replace(/ /g, '_').replace(/[()]/g, '').replace(/[^a-z0-9_-]+/g, '');
const one = (name) => p(r('Before Hxg')) + p(r(LONG, rf(name) + sz(12))) + p(r('After Hxg'));

for (const [name, charset] of [['Tahoma Baltic', 'BA'], ['Calibri Baltic', 'BA'], ['Segoe UI Greek', 'A1'], ['Segoe UI CE', 'EE'], ['Verdana Baltic', 'BA'],
  ['Tahoma (Hebrew)', 'B1'], ['Calibri (Arabic)', 'B2'], ['Segoe UI Cyr', 'CC']]) add(`c2y-${slug(name)}`, one(name), { ft: fe(name, { charset, family: 'swiss' }) });
add('c2y-segoe_ui_baltic-notable', one('Segoe UI Baltic'));
for (const name of ['Arial Negrito', 'Georgia Negrito', 'Verdana Negrito', 'Arial Itálico', 'Times New Roman Negrito', 'SegoeUILight', 'SegoeUIBold', 'ArialBold',
  'CalibriBold', 'TahomaBold', 'ArialNarrowBold', 'TimesNewRoman', 'CourierNew', 'SegoeUI']) add(`c5y-${slug(name)}`, one(name));

for (const [tag, entry] of [['roman-nopanose', { charset: '02', family: 'roman' }], ['auto-sympanose', { charset: '02', family: 'auto', panose: '05050102010706020507' }],
  ['roman-wingpanose', { charset: '02', family: 'roman', panose: '05000000000000000000' }]]) {
  const name = 'ZzSymbolFont';
  const body = p(r('Before Hxg')) + p(r('A ') + r('', rf(name) + sz(14)) + r(' Z')) + p(r('C ') + r('abg', rf(name) + sz(14)) + r(' Z')) + p(r('After Hxg'));
  add(`c6y-${tag}`, body, { ft: fe(name, entry) });
}

const plain = (n) => Array.from({ length: n }, (_, i) => p(r(`Plain line ${i + 1} Hxg`))).join('');
for (const [tag, name, size, line] of [['tahoma-150', 'Tahoma', 11, 360], ['verdana-150', 'Verdana', 11, 360], ['courier-150', 'Courier New', 11, 360],
  ['tahoma-108', 'Tahoma', 11, 259], ['cambria20-150', 'Cambria', 20, 360], ['tahoma-200', 'Tahoma', 11, 480]]) {
  const lines = Array.from({ length: 6 }, (_, i) => p(r(`Run ${i + 1} Hxg `) + r('abg Hxg', rf(name) + sz(size)))).join('');
  add(`c1y-${tag}`, plain(4) + lines + plain(4), { line });
}

const dd = (rfonts, lang) => `<w:docDefaults><w:rPrDefault><w:rPr>${rfonts}<w:sz w:val="22"/><w:szCs w:val="22"/>${lang}</w:rPr></w:rPrDefault><w:pPrDefault><w:pPr><w:spacing w:after="0" w:line="240" w:lineRule="auto"/></w:pPr></w:pPrDefault></w:docDefaults>`;
const SEG = '<w:rFonts w:ascii="Segoe UI" w:hAnsi="Segoe UI" w:eastAsia="Segoe UI" w:cs="Segoe UI"/>';
const twelve = '<w:sz w:val="24"/><w:szCs w:val="24"/>';
add('a2y-valko', p(r(Array.from({ length: 6 }, (_, i) => `Hxg 한국어 ${i}`).join('\n'), twelve)), { defaults: dd(SEG, '<w:lang w:val="ko-KR" w:eastAsia="ko-KR" w:bidi="ar-SA"/>') });
add('a2y-valzhtw', p(r(Array.from({ length: 6 }, (_, i) => `Hxg 中文字 ${i}`).join('\n'), twelve)), { defaults: dd(SEG, '<w:lang w:val="zh-TW" w:eastAsia="zh-TW" w:bidi="ar-SA"/>') });
add('a2y-mixed', Array.from({ length: 6 }, (_, i) => p(r('Hxg ', twelve + '<w:lang w:val="en-US"/>') + r(`日本語 ${i}`, twelve + '<w:lang w:val="ja-JP"/>'))).join(''), { defaults: dd(SEG, '<w:lang w:val="en-US" w:eastAsia="en-US" w:bidi="ar-SA"/>') });
add('a2y-valja-latinonly-run', Array.from({ length: 6 }, (_, i) => p(r(`Hxg abc ${i}`, twelve + '<w:lang w:val="ja-JP"/>'))).join(''), { defaults: dd(SEG, '<w:lang w:val="en-US" w:eastAsia="en-US" w:bidi="ar-SA"/>') });
