// The regex sites of frontier D2b-F1 (source.ts `plainTextSdt`, text-case.ts, universal-write.ts
// `colorPick`, `changeCase`) against V8, checked by tests/regex_d2b_f1.rs. A row is
// `[id, regex | function, op]` (see js/regex-diff.mjs). The TS sites that apply a regex to ONE
// character (`/\s/.test(c)`, `/\d/.test(c)`) are rows over the whole input: the Rust side asks the
// same question of every UTF-16 unit.
import { INPUTS as BASE } from './templates.mjs';

const plainTextSdt = (open) => {
  if (!open || !/^<w:sdt[\s>]/.test(open)) return false;
  const end = open.indexOf('</w:sdtPr>');
  return /<w:(?:text|date|comboBox|dropDownList)\b/.test(end < 0 ? open : open.slice(0, end));
};

export const INPUTS = [
  ...BASE,
  '<w:sdt>', '<w:sdt><w:sdtPr><w:text/></w:sdtPr>', '<w:sdt><w:sdtPr><w:date/></w:sdtPr>', '<w:sdt><w:sdtPr><w:comboBox>', '<w:sdt><w:sdtPr><w:dropDownList>',
  '<w:sdt><w:sdtPr><w:textual/></w:sdtPr>', '<w:sdt><w:sdtPr><w:text9/>', '<w:sdt><w:sdtPr><w:text_/>', '<w:sdt><w:sdtPr><w:text-x/>', '<w:sdt><w:sdtPr><w:textÉ/>',
  '<w:sdtX><w:text/>', '<w:sdt\n><w:text/>', '<w:sdt\t><w:date/>', '<w:sdt ><w:date/>', '<w:sdt﻿><w:date/>', '<w:sdt\u0085><w:date/>', ' <w:sdt><w:text/>',
  '<w:sdt><w:sdtPr><w:richText/></w:sdtPr><w:text/>', '<w:sdt><w:sdtPr><w:id w:val="1"/><w:dropDownList w:lastValue="x"/></w:sdtPr>', '<w:sdt><w:sdtPr><w:text',
  '<w:sdt><w:sdtPr><w:comboBoxes/>', '<w:sdt><w:sdtPr><x:w:text/>', '<w:sdt><w:sdtPr><w:Text/>',
  '#ff0000', '##ff', '#', 'FF00AA', ' #a', '.', '!', '?', 'a.b', 'abc', '3', '٣', '\n', ' ', '᠎', ' ', 'ǆ',
];

/** [site id, the TS regex (or function), operation]. */
export const SITES = [
  ['d2b-f1.sdt_open', /^<w:sdt[\s>]/, 'test'],
  ['d2b-f1.sdt_kind', /<w:(?:text|date|comboBox|dropDownList)\b/, 'test'],
  ['d2b-f1.plain_text_sdt', plainTextSdt, 'fn'],
  ['d2b-f1.space', /\s/, 'test'],
  ['d2b-f1.digit', /\d/, 'test'],
  ['d2b-f1.sentence_end', /[.!?]/, 'test'],
  ['d2b-f1.strip_hash', (s) => s.replace(/^#/, ''), 'fn'],
];
