// The regex sites of frontier D2b-F8 (universal-write.ts paste/copy, paste-html.ts,
// numbering-store.ts importAbstract) against V8, checked by tests/regex_d2b_f8.rs. A row is
// `[id, function, 'fn']` (see js/regex-diff.mjs); the TS expressions are copied as written.
import { INPUTS as BASE } from './templates.mjs';

const J = JSON.stringify;
// universal-write.ts:5021, 5506
const nl = (t) => t.replace(/\r\n?/g, '\n');
// 5592
const lines = (t) => J(t.split(/\r\n?|\n/));
// 5076–5077
const blank = (h) => /^\s*$/.test(h);
const tableTag = (h) => /<table[\s>]/i.test(h);
const stripTables = (h) => h.replace(/<table[\s\S]*?<\/table>/gi, '');
const blockTag = (h) => /<(?:p|h[1-6]|li|div)[\s>]/i.test(h);
// 5189, 5228
const startsWord = (t) => /^[\p{L}\p{N}]/u.test(t);
const endsWord = (t) => /[\p{L}\p{N}]$/u.test(t);
const endsWordPunct = (t) => /[\p{L}\p{N}.,;:!?)\]}"'”’]$/u.test(t);
// 5123
const docPrName = (x) => x.replace(/(<wp:docPr\b[^>]*?\bname=")([^"]*?)\s*\d*"/, (_m, a, n) => `${a}${n} 77"`);
// 5285 (the dynamic regex, with several ilvl numbers)
const lvl = (ilvl) => (raw) => new RegExp(`<w:lvl\\b[^>]*w:ilvl="${ilvl}"[\\s\\S]*?<w:numFmt\\s+w:val="([^"]+)"`).exec(raw)?.[1] ?? null;
// 5684
const bs = (p) => p.replace(/\\/g, '\\\\');
// 20457, 20458, 20564
const esc = (s) => s.replace(/&/g, '&amp;').replace(/</g, '&lt;').replace(/>/g, '&gt;');
const nlSpace = (t) => t.replace(/\n/g, ' ');
const squash = (t) => t.replace(/\s+/g, ' ').trim();
// paste-html.ts:48–52
const hex3 = (s) => /^#([0-9a-f]{3})$/.exec(s)?.[1] ?? null;
const hex6 = (s) => /^#([0-9a-f]{6})/.exec(s)?.[1] ?? null;
const rgba = (s) => { const m = /^rgba?\(\s*(\d+)[\s,]+(\d+)[\s,]+(\d+)(?:[\s,/]+([\d.]+%?))?\s*\)$/.exec(s); return m ? J([m[1], m[2], m[3], m[4] ?? null]) : null; };
// 65, 74, 75, 100
const sizeUnit = (s) => { const m = /^([\d.]+)(pt|px)$/.exec(s); return m ? J([m[1], m[2]]) : null; };
const quotes = (f) => f.trim().replace(/^["']|["']$/g, '');
const generic = (n) => /^(serif|sans-serif|monospace|cursive|fantasy|system-ui|ui-\w+|-apple-system|blinkmacsystemfont|inherit|initial)$/i.test(n);
const js = (h) => /^javascript:/i.test(h);
// 124, 127
const dataUrl = (s) => { const m = /^data:image\/(png|jpe?g|gif|bmp|webp);base64,([\s\S]+)$/i.exec(s); return m ? J([m[1], m[2]]) : null; };
const noSpace = (s) => s.replace(/\s+/g, '');
// 145–148
const listDefs = (html) => {
  const out = [];
  const re = /@list\s+l(\d+):level(\d+)\s*\{([^}]*)\}/gi;
  let m;
  while ((m = re.exec(html))) out.push([m[1], m[2], /mso-level-number-format:\s*([\w-]+)/i.exec(m[3])?.[1]?.toLowerCase() ?? null]);
  return J(out);
};
// 156, 162–163
const listOf = (s) => { const m = /mso-list:\s*l(\d+)\s+level(\d+)\s+lfo(\d+)/i.exec(s); return m ? J([m[1], m[2], m[3]]) : null; };
const ordered = (t) => /^\(?\d+[.)]?$/.test(t) || /^\(?[a-z]{1,4}[.)]$/i.test(t);
// 173–174, 180, 245
const supportLists = (v) => /^\[if !supportLists\]/i.test(v);
const endif = (v) => /^\[endif\]/i.test(v);
const ignore = (s) => /mso-list:\s*ignore/i.test(s);
// 203, 208
const collapse = (t) => t.replace(/[ \t\r\n\f]+/g, ' ');
const endSpace = (t) => /[ \n]$/.test(t);
const lead = (t) => t.replace(/^ +/, '');
// 275, 280–283
const hTag = (t) => /^H([1-6])$/.exec(t)?.[1] ?? null;
const msoH = (c) => /(?:^|\s)MsoHeading([1-9])(?:\s|$)/.exec(c)?.[1] ?? null;
const msoT = (c) => /(?:^|\s)MsoTitle(?:\s|$)/.test(c);
const msoS = (c) => /(?:^|\s)MsoSubtitle(?:\s|$)/.test(c);
// numbering-store.ts:234–237
const absOpen = (raw) => raw.replace(/<w:abstractNum\b[^>]*>/, '<w:abstractNum w:abstractNumId="7">');
const styleLinks = (raw) => raw.replace(/<w:(?:styleLink|numStyleLink) w:val="[^"]*"\/>/g, '');

/** [site id, the TS expression, operation]. */
export const SITES = [
  ['d2b-f8.nl', nl, 'fn'], ['d2b-f8.lines', lines, 'fn'], ['d2b-f8.blank', blank, 'fn'], ['d2b-f8.table_tag', tableTag, 'fn'],
  ['d2b-f8.strip_tables', stripTables, 'fn'], ['d2b-f8.block_tag', blockTag, 'fn'], ['d2b-f8.starts_word', startsWord, 'fn'],
  ['d2b-f8.ends_word', endsWord, 'fn'], ['d2b-f8.ends_word_punct', endsWordPunct, 'fn'], ['d2b-f8.doc_pr_name', docPrName, 'fn'],
  ['d2b-f8.lvl_0', lvl(0), 'fn'], ['d2b-f8.lvl_1', lvl(1), 'fn'], ['d2b-f8.lvl_1.5', lvl(1.5), 'fn'], ['d2b-f8.lvl_NaN', lvl(NaN), 'fn'],
  ['d2b-f8.lvl_-0', lvl(-0), 'fn'], ['d2b-f8.lvl_1e+21', lvl(1e21), 'fn'], ['d2b-f8.lvl_null', lvl(null), 'fn'],
  ['d2b-f8.backslashes', bs, 'fn'], ['d2b-f8.esc', esc, 'fn'], ['d2b-f8.nl_space', nlSpace, 'fn'], ['d2b-f8.squash', squash, 'fn'],
  ['d2b-f8.hex3', hex3, 'fn'], ['d2b-f8.hex6', hex6, 'fn'], ['d2b-f8.rgba', rgba, 'fn'], ['d2b-f8.size_unit', sizeUnit, 'fn'],
  ['d2b-f8.quotes', quotes, 'fn'], ['d2b-f8.generic', generic, 'fn'], ['d2b-f8.javascript', js, 'fn'], ['d2b-f8.data_url', dataUrl, 'fn'],
  ['d2b-f8.no_space', noSpace, 'fn'], ['d2b-f8.list_defs', listDefs, 'fn'], ['d2b-f8.list_of', listOf, 'fn'], ['d2b-f8.ordered', ordered, 'fn'],
  ['d2b-f8.support_lists', supportLists, 'fn'], ['d2b-f8.endif', endif, 'fn'], ['d2b-f8.ignore', ignore, 'fn'], ['d2b-f8.collapse', collapse, 'fn'],
  ['d2b-f8.end_space', endSpace, 'fn'], ['d2b-f8.lead', lead, 'fn'], ['d2b-f8.h_tag', hTag, 'fn'], ['d2b-f8.mso_h', msoH, 'fn'],
  ['d2b-f8.mso_t', msoT, 'fn'], ['d2b-f8.mso_s', msoS, 'fn'], ['d2b-f8.abs_open', absOpen, 'fn'], ['d2b-f8.style_links', styleLinks, 'fn'],
];

export const INPUTS = [
  ...BASE,
  '', ' ', '\t\n', 'a\r\nb\rc\nd', '\r\r\n\n', 'x\u2028y',
  '<table>x</table>', '<TABLE a>y</Table>z', '<table', '<tablex>', '<table\n>', 'a<table>b</table>c<table>d', '<p>', '<P\t>', '<pre>', '<h7>', '<h1>', '<li>', '<lix>', '<div>', '<DIV ', '<x><p >',
  'abc', 'ção', '1', '😀', 'a.', 'a)', '"', '”', '’', ']', '}', '?', '-', ' a', 'a ',
  '<wp:docPr id="3" name="Imagem 12"/>', '<wp:docPr id="3" name="Pic 1 2"/>', '<wp:docPr name="12"/>', '<wp:docPr name="a  "/>', '<wp:docPrX name="a 1"/><wp:docPr id="2" name="Shape 9"/>',
  '<wp:docPr id="1" xname="a" name="b 4"/>', '<wp:docPr id="1"><x name="a 2"/>', '<wp:docPr name="unclosed',
  '<w:lvl w:ilvl="0"><w:numFmt w:val="bullet"/></w:lvl><w:lvl w:ilvl="1"><w:numFmt w:val="decimal"/>', '<w:lvl w:ilvl="1x5"><w:numFmt  w:val="lowerRoman"/>', '<w:lvl w:ilvl="NaN" w:x="1"><w:numFmt\nw:val="x"/>',
  '<w:lvl w:ilvl="1eee21"><w:numFmt w:val="y"/>', '<w:lvlText w:ilvl="0"/><w:lvl w:ilvl="0" w:ilvl="1"><w:numFmt w:val=""/><w:numFmt w:val="z"/>', '<w:lvl w:ilvl="null"><w:numFmt w:val="n"/>',
  '<w:lvl w:ilvl="0">', '<w:lvl\tw:ilvl="1" a="w:ilvl="0""><w:numFmt w:val="q"/>',
  'C:\\docs\\a.docx', '\\\\server\\x', 'a & b < c > d', '  a \n\t b  ', '\u00a0x\u00a0',
  '#abc', '#ABC', '#abcd', '#abcdef', '#abcdef00', '#12345', 'rgb(1, 2, 3)', 'rgba(1,2,3,0)', 'rgb(1 2 3 / 0.5)', 'rgb(1,2,3 )', 'rgb(1,2)', 'rgba(1,2,3,,0%)', 'rgb(1,2,3,4,5)', 'rgb( 1 , 2 , 3 / . )',
  '12pt', '10.5px', '.5pt', '1..2pt', 'pt', '12PT', '12 pt', '1e3pt',
  '"Arial"', "'Times New Roman'", '  "x" ', '"', '""', 'serif', 'SANS-SERIF', 'ui-monospace', 'ui-', 'ui-x y', '-apple-system', 'BlinkMacSystemFont', 'Calibri',
  'javascript:x', 'JavaScript:', ' javascript:', 'https://e.com', '#top',
  'data:image/png;base64,iVBOR w0K', 'DATA:IMAGE/JPEG;BASE64,/9j/', 'data:image/jpg;base64,', 'data:image/gif;base64,R0l\nGOD', 'data:image/svg;base64,PHN2', 'data:text/html;base64,x',
  '@list l0:level1 { mso-level-number-format:bullet; }', '@LIST L1:LEVEL2{mso-level-number-format: image}@list l2:level1 {}', '@list l3:level1 {', '@listl0:level1{}', '@list l01:level09 {mso-level-number-format:-x_1}',
  'mso-list:l0 level1 lfo1', 'MSO-LIST: L1 LEVEL2 LFO3', 'mso-list:l0 level1', 'x mso-list:l2 level1 lfo1 mso-list:l3 level2 lfo2', 'mso-list:Ignore', 'mso-list: IGNORE', 'mso-list:\tignorex',
  '1.', '(1)', '1)', 'a)', 'iv.', '(iv)', 'abcd.', 'abcde.', '·', '1.)', '(a', 'A.', '12',
  '[if !supportLists]', '[IF !SUPPORTLISTS] x', '[endif]', '[ENDIF]--', ' [endif]',
  'H1', 'H6', 'H7', 'h1', 'H12', 'MsoHeading3', 'x MsoHeading9 y', 'MsoHeading0', 'MsoHeading12', 'MsoTitle', 'MsoTitleX', 'a\tMsoSubtitle', 'MsoSubtitle\n',
  '<w:abstractNum w:abstractNumId="3" w15:restartNumberingAfterBreak="0"><w:nsid w:val="ABCDEF01"/><w:styleLink w:val="x"/><w:numStyleLink w:val="y"/></w:abstractNum>',
  '<w:abstractNumId w:val="1"/><w:abstractNum>', '<w:abstractNum', '<w:styleLink w:val="a"/x', '<w:numStyleLink  w:val="a"/>',
];
