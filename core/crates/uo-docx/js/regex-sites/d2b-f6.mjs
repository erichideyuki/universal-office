// The regex sites of frontier D2b-F6 (editor references.ts `validBookmarkName`, `maxBookmarkId`;
// universal-write.ts `newBookmarkId`, `paraOfItem`; source.ts `plainTextSdt`) against V8, checked by
// tests/regex_d2b_f6.rs. A row is `[id, function, 'fn']` (see js/regex-diff.mjs); the TS expressions
// are copied as written.
import { INPUTS as BASE } from './templates.mjs';

// references.ts:23
const validName = (name) => name.length > 0 && name.length <= 40 && /^\p{L}[\p{L}\p{N}_]*$/u.test(name);
// universal-write.ts:6167 (the part filter) and references.ts:133 (the scan of the raw XML)
const bookmarkPart = (p) => /^word\/(document|header\d*|footer\d*|footnotes|endnotes|comments)\.xml$/.test(p);
const startIds = (x) => JSON.stringify([...x.matchAll(/<w:bookmarkStart\b[^>]*\bw:id="(\d+)"/g)].map((m) => m[1]));
// universal-write.ts:6349
const paraOfItem = (id) => { const m = /^p:(\d+)$/.exec(id); return m ? String(+m[1]) : '-1'; };
// source.ts:323
const plainTextSdt = (open) => {
  if (!open || !/^<w:sdt[\s>]/.test(open)) return false;
  const end = open.indexOf('</w:sdtPr>');
  return /<w:(?:text|date|comboBox|dropDownList)\b/.test(end < 0 ? open : open.slice(0, end));
};

/** [site id, the TS expression, operation]. */
export const SITES = [
  ['d2b-f6.bookmark_name', validName, 'fn'],
  ['d2b-f6.bookmark_part', bookmarkPart, 'fn'],
  ['d2b-f6.bookmark_start_ids', startIds, 'fn'],
  ['d2b-f6.para_of_item', paraOfItem, 'fn'],
  ['d2b-f6.plain_text_sdt', plainTextSdt, 'fn'],
];

export const INPUTS = [
  ...BASE,
  // bookmark names: letters, digits, underscore, accents, non-BMP letters, the 40/41 limit
  'Alfa', 'alfa_1', '_oculto', '1ruim', 'é2', 'É_x', 'a b', 'a-b', 'a.b', 'ǅ', 'ǆa1', 'ª', 'º2', 'ʰx', 'ⅷ', 'a²', 'a½', 'aⅷ', 'a٣', 'a٣_',
  '𝒜', '𝒜b', 'a𝒜', 'a😀', '日本語', 'ひら1', 'x'.repeat(40), 'x'.repeat(41), 'é'.repeat(40), 'é'.repeat(41), '𝒜'.repeat(20), '𝒜'.repeat(21), 'á', 'a_', '_', 'a\u0000', 'a\n',
  // part names
  'word/document.xml', 'word/header1.xml', 'word/header.xml', 'word/header12.xml', 'word/headerX.xml', 'word/footer2.xml', 'word/footnotes.xml', 'word/endnotes.xml',
  'word/comments.xml', 'word/commentsExtended.xml', 'word/styles.xml', 'word/_rels/document.xml.rels', 'word/document.xml\n', 'xword/document.xml', 'word/document.xmlx', 'word/Document.xml',
  'word/header٣.xml', 'word//document.xml',
  // raw XML: the last w:id of the tag wins, \b around the tag name and the attribute name
  '<w:bookmarkStart w:id="1" w:name="a"/>', '<w:bookmarkStart w:id="1" w:name="a"/><w:bookmarkStart w:id="22" w:name="b"/>', '<w:bookmarkStart w:name="a" w:id="7"/>',
  '<w:bookmarkStartX w:id="3"/>', '<w:bookmarkStart_ w:id="3"/>', '<w:bookmarkStart-x w:id="3"/>', '<w:bookmarkStart\nw:id="5"/>', '<w:bookmarkStart w:id="1" x:w:id="2"/>',
  '<w:bookmarkStart w:id="1" xw:id="2"/>', '<w:bookmarkStart w:id="1" w:id="2"/>', '<w:bookmarkStart w:id="abc"/>', '<w:bookmarkStart w:id=""/>', '<w:bookmarkStart w:id="12x"/>',
  '<w:bookmarkStart w:id="٣"/>', '<w:bookmarkStart w:id=\'9\'/>', '<w:bookmarkStart/>', '<w:bookmarkStart', '<w:bookmarkStart w:id="4"', '<w:bookmarkStart w:id="4" ',
  '<w:bookmarkStart w:name="w:id=&quot;8&quot;" w:id="9"/>', '<w:bookmarkStart w:name=">" w:id="6"/>', '<w:bookmarkStart w:id="1"><w:bookmarkStart w:id="2"/>',
  '<w:bookmarkStart w:id="99999999999999999999"/>', '<w:bookmarkStart w:id="0012"/>', '<w:bookmarkStart w:id="1" w:id="2" w:id="3"/>', '<w:bookmarkstart w:id="3"/>',
  '<w:bookmarkEnd w:id="3"/>', 'x<w:bookmarkStart w:id="3"/>y<w:bookmarkStart w:id="4"/>z', '<w:bookmarkStart w:id="1" w:id="2" <w:bookmarkStart w:id="3"/>',
  '<w:bookmarkStart w:id="1" w:colFirst="0"/>', '<w:bookmarkStart w:id="1"/>\n<w:bookmarkStart w:id="2"/>',
  // paraOfItem
  'p:0', 'p:12', 'p:007', 'p:', 'p', 'p:1x', 'p:-1', 'p: 1', 'P:1', 'p:٣', 'p:4294967295', 'p:4294967296', 'p:99999999999999999999', 'p:1e3', 'p:1\n', 'xp:1', 'p:1.5',
  // plainTextSdt openings (also F1's site): a few of them, with the sdtPr cut
  '<w:sdt>', '<w:sdt><w:sdtPr><w:text/></w:sdtPr>', '<w:sdt><w:sdtPr><w:date/></w:sdtPr>', '<w:sdt><w:sdtPr><w:comboBox>', '<w:sdt><w:sdtPr><w:dropDownList>',
  '<w:sdt><w:sdtPr><w:textual/></w:sdtPr>', '<w:sdt><w:sdtPr><w:text9/>', '<w:sdt><w:sdtPr><w:text_/>', '<w:sdt><w:sdtPr><w:text-x/>', '<w:sdtX><w:text/>',
  '<w:sdt\n><w:text/>', '<w:sdt\t><w:date/>', '<w:sdt ><w:date/>', '<w:sdt ><w:date/>', '<w:sdt﻿><w:date/>', ' <w:sdt><w:text/>',
  '<w:sdt><w:sdtPr><w:richText/></w:sdtPr><w:text/>', '<w:sdt><w:sdtPr><w:id w:val="1"/><w:dropDownList w:lastValue="x"/></w:sdtPr>', '<w:sdt><w:sdtPr><w:text',
  '<w:sdt><w:sdtPr><w:Text/>', '<w:sdt><w:sdtPr></w:sdtPr><w:text/>', '<w:sdt><w:sdtPr><w:date/>', '<w:sdt><w:sdtPr><w:comboBox',
];
