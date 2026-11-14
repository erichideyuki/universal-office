// WordCraft study (2026-11-13), planner "fields" — A2 aside, last question (run with --fresh): Latin text drawn in the
// East Asian font (MS Mincho) happened with the DOCUMENT default w:lang ja-JP / zh-* / ko-KR (wcfonts2 a2x-valja-*),
// not with a RUN-level w:lang ja-JP (wcfonts3 a2y-valja-latinonly-run). Which level decides: the paragraph mark, the
// paragraph style, the document default (and does a run-level en-US undo it)?
import { strToU8 } from 'fflate';
import { buildDocx, p, r } from '../docx-builder.mjs';

const W_NS = 'xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"';
const fontTable = strToU8(`<?xml version="1.0" encoding="UTF-8" standalone="yes"?><w:fonts ${W_NS}><w:font w:name="Calibri"><w:panose1 w:val="020F0502020204030204"/><w:charset w:val="00"/><w:family w:val="swiss"/><w:pitch w:val="variable"/></w:font></w:fonts>`);
const dd = (lang) => `<w:docDefaults><w:rPrDefault><w:rPr><w:rFonts w:ascii="Segoe UI" w:hAnsi="Segoe UI" w:eastAsia="Segoe UI" w:cs="Segoe UI"/><w:sz w:val="24"/><w:szCs w:val="24"/><w:lang w:val="${lang}" w:eastAsia="en-US" w:bidi="ar-SA"/></w:rPr></w:rPrDefault><w:pPrDefault><w:pPr><w:spacing w:after="0" w:line="240" w:lineRule="auto"/></w:pPr></w:pPrDefault></w:docDefaults>`;
const JA = '<w:lang w:val="ja-JP"/>';
const EN = '<w:lang w:val="en-US"/>';
const styles = `<w:style w:type="paragraph" w:customStyle="1" w:styleId="JaPara"><w:name w:val="Ja Para"/><w:basedOn w:val="Normal"/><w:rPr>${JA}</w:rPr></w:style>`;
const lines = (mk) => Array.from({ length: 6 }, (_, i) => mk(i)).join('');
export const cases = [];
const add = (name, body, lang) => cases.push({ name, docx: buildDocx({ body, styles, defaults: dd(lang), extraParts: { 'word/fontTable.xml': fontTable } }) });
add('a2z-mark-ja', lines((i) => p(r(`Hxg abc ${i}`), `<w:rPr>${JA}</w:rPr>`)), 'en-US');
add('a2z-style-ja', lines((i) => p(r(`Hxg abc ${i}`), '<w:pStyle w:val="JaPara"/>')), 'en-US');
add('a2z-style-ja-run-en', lines((i) => p(r(`Hxg abc ${i}`, EN), '<w:pStyle w:val="JaPara"/>')), 'en-US');
add('a2z-default-ja-run-en', lines((i) => p(r(`Hxg abc ${i}`, EN))), 'ja-JP');
add('a2z-default-ja-cjk-run-en', lines((i) => p(r(`Hxg 日本語 ${i}`, EN))), 'ja-JP');
add('a2z-run-ja-mark-ja', lines((i) => p(r(`Hxg abc ${i}`, JA), `<w:rPr>${JA}</w:rPr>`)), 'en-US');
