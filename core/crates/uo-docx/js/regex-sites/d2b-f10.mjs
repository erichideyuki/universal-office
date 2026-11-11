// The regex sites of frontier D2b-F10 (editor references.ts, toc.ts, text-layout.ts and
// universal-write.ts) against V8, checked by tests/regex_d2b_f10.rs through
// `session::ops::references::rx::regex_site`. A row is `[id, function, 'fn']` (see
// js/regex-diff.mjs); the TS expressions are copied as written and the results returned as ASCII
// (UTF-16 units in hex, `.`-joined) so that the comparison is exact.
import { INPUTS as BASE } from './templates.mjs';

const hx = (t) => [...Array(t.length).keys()].map((i) => t.charCodeAt(i).toString(16)).join('.');
const U = (v) => (v === undefined ? '~' : hx(v));

// instructions: the grammar of the TOC/REF/SEQ/TC fields, then the hostile ones
const SW = ['\\o "1-3"', '\\o', '\\u', '\\h', '\\z', '\\n', '\\n "2-3"', '\\p " "', '\\p ""', '\\f', '\\f x', '\\f "a b"', '\\f ""', '\\l "1-2"', '\\t "Heading 1;1;Heading 2;2"', '\\t "a,1,b"', '\\t ";"',
  '\\c "Figure"', '\\a "Table"', '\\w', '\\r', '\\d "-"', '\\x', '\\O', '\\U', '\\Z', '\\H', '\\p', '\\o"1-2"', '\\o  "1 - 2"', '\\o "9-9"', '\\o "x"', '\\t "x;y"', '\\t "x;0;y;-3;z;99"'];
const HEAD = ['TOC', ' TOC ', 'toc', 'REF x', ' REF  _Ref123 ', 'PAGEREF a', 'NOTEREF a', 'NOTEREF', 'REFERENCE x', 'SEQ Figure', ' SEQ "My label" ', 'SEQ', 'seq   a', 'SEQ "" x', 'SEQ "a', 'SEQ \\* ARABIC',
  'TC "a b"', ' TC  text', 'TC "a \\"q\\" b"', 'TC "a\\\nb"', 'TC', 'TC "x', 'tc "A" \\f C', '= SUM(ABOVE)', '=1', '  =', 'PAGE', 'PAGEREFx', 'PAGEREF', ' PAGEREF a \\h', 'DATE'];
const rnd = (() => { let s = 0x10f; return (n) => { s ^= s << 13; s >>>= 0; s ^= s >>> 17; s ^= s << 5; s >>>= 0; return s % n; }; })();
const GEN = [];
for (let i = 0; i < 260; i++) {
  const n = rnd(4);
  let t = HEAD[rnd(HEAD.length)];
  for (let k = 0; k < n; k++) t += ` ${SW[rnd(SW.length)]}`;
  if (rnd(5) === 0) t += ' \\fx \\n9 \\f"z" \\l 2 \\l "3" \\n\\p';
  if (rnd(7) === 0) t = `  ${t}\t`;
  GEN.push(t);
}
export const INPUTS = [
  ...BASE, ...HEAD, ...SW, ...GEN,
  '', ' ', '\\', '\\f', '\\f x', '\\fx', '\\F "a" \\f b', '\\l', '\\l "2', '\\l 12', '\\l "x"', '\\n', '\\n.', '\\n_', '\\nx', '\\N"', '\\n ', '\\p"x"', '\\f\n"y"',
  'heading 1', 'Heading 9', 'heading 0', 'heading  1', 'HEADING 5', 'heading 10', 'heading 1 ', ' heading 1', 'Heading\u00a01',
  'p:1', 'p:', 'p:99999999999999999999', 'n:12', 'n:NaN', 'n:', 'p:1x', 'p:01', 'P:1', 'n:٣',
  '1-2', '1 - 2', '1-', '-2', '12-34', 'a1-2b', '1\u00a0-\u00a02', '1\n-\n2', '٣-٤',
  '<dc:creator>Eric</dc:creator>', '<dc:creator xml:lang="x">A &amp; B &lt;c&gt; &quot;d&quot;</dc:creator>', '<dc:creatorX>q</dc:creatorX>', '<dc:creator>', '<dc:creator', '<dc:creator>a',
  '<dc:title/><dc:title>T</dc:title>', '<dc:title a="b>c">T</dc:title>', '<dc:title>&amp;lt;</dc:title>', '<dc:title><b>x</b></dc:title>', '<dc:creator>1</dc:creator><dc:creator>2</dc:creator>',
  '\t\n\f\u000bx\r', 'a\tb\nc\fd\u000be\rf', ' \u00a0\ufeff x \u2028 ', 'a\\b"c', '"\\', '\\\\', '"a"',
];

/** [site id, the TS expression, 'fn']. */
export const SITES = [
  // references.ts:201 refFieldTarget
  ['d2b-f10.ref_target', (s) => { const m = /^\s*(?:REF|PAGEREF|NOTEREF)\s+([^\s\\]+)/i.exec(s); return m ? hx(m[1]) : '~'; }, 'fn'],
  // references.ts:251 seqLabel
  ['d2b-f10.seq_label', (s) => { const m = /^\s*SEQ\s+("([^"]+)"|\S+)/i.exec(s); return m ? hx(m[2] ?? m[1]) : '~'; }, 'fn'],
  // references.ts:288 q(sw), for every switch the TOC reads
  ...['o', 't', 'c', 'a', 'l', 'n', 'p', 'f', 'h', 'z'].map((sw) => [`d2b-f10.q_${sw}`, (s) => U(new RegExp(`\\\\${sw}\\s+"([^"]*)"`, 'i').exec(s)?.[1]), 'fn']),
  // references.ts:289 has(sw)
  ...['o', 'u', 'h', 'n', 'z', 'p'].map((sw) => [`d2b-f10.has_${sw}`, (s) => String(new RegExp(`\\\\${sw}(?![a-z"])`, 'i').test(s.replace(/"[^"]*"/g, '""'))), 'fn']),
  // references.ts:291/306 /(\d)\s*-\s*(\d)/
  ['d2b-f10.digit_range', (s) => { const m = /(\d)\s*-\s*(\d)/.exec(s); return m ? `${+m[1]},${+m[2]}` : '~'; }, 'fn'],
  // references.ts:304 the \f switch of a TOC
  ['d2b-f10.switch_f', (s) => { const f = /\\f(?:\s+(?:"([^"]*)"|([^\s\\"]+)))?/i.exec(s); return f ? `y|${U(f[1] ?? f[2])}` : '~'; }, 'fn'],
  // references.ts:336 /[\s"]/ and the split of \t
  ['d2b-f10.id_quoted', (s) => String(/[\s"]/.test(s)), 'fn'],
  ['d2b-f10.t_split', (s) => s.split(/[;,]/).map((x) => `${hx(x.trim())}:${parseInt(x, 10) || 1}`).join(' '), 'fn'],
  // references.ts:354/ universal-write.ts caption fields /\s/ and the entry text replace
  ['d2b-f10.has_space', (s) => String(/\s/.test(s)), 'fn'],
  ['d2b-f10.entry_text', (s) => hx(s.replace(/[\t\n\f\u000B\r]/g, ' ')), 'fn'],
  // toc.ts:127 parseTcInstr
  ['d2b-f10.tc', (s) => {
    const m = /^\s*TC\s+(?:"((?:[^"\\]|\\.)*)"|(\S+))/i.exec(s);
    if (!m) return '~';
    const text = (m[1] ?? m[2] ?? '').replace(/\\(.)/g, '$1');
    const rest = s.slice(m.index + m[0].length);
    const f = /\\f\s+(?:"([^"]*)"|([^\s\\"]+))/i.exec(rest);
    const l = /\\l\s+"?(\d+)/i.exec(rest);
    return `${hx(text)}|${f ? hx(f[1] ?? f[2] ?? 'C') : 'C'}|${l ? Math.min(9, Math.max(1, +l[1])) : 1}|${/\\n\b/i.test(rest)}`;
  }, 'fn'],
  // toc.ts:139 tcInstr
  ['d2b-f10.tc_escape', (s) => hx(s.replace(/(["\\])/g, '\\$1')), 'fn'],
  // universal-write.ts:6474 the field name
  ['d2b-f10.field_name', (s) => hx(/^\s*(\w+)/.exec(s)?.[1]?.toUpperCase() ?? ''), 'fn'],
  // :6482 and :6491
  ['d2b-f10.has_pnrw', (s) => String(/\\[pnrw]\b/i.test(s)), 'fn'],
  ['d2b-f10.has_f', (s) => String(/\\f\b/i.test(s)), 'fn'],
  // :6601, :7006, :6560
  ['d2b-f10.starts_toc', (s) => String(/^\s*TOC\b/i.test(s)), 'fn'],
  ['d2b-f10.starts_pageref', (s) => String(/^\s*PAGEREF\b/i.test(s)), 'fn'],
  ['d2b-f10.starts_eq', (s) => String(/^\s*=/.test(s)), 'fn'],
  // :6665 the caption text after the number, and :6667 the entry text
  ['d2b-f10.trim_start', (s) => hx(s.replace(/^\s+/, '')), 'fn'],
  ['d2b-f10.source_text', (s) => hx(s.replace(/[\t\n\f\u000B]/g, ' ').trim()), 'fn'],
  // :6330 and :6335 (the note and caption items)
  ['d2b-f10.note_text', (s) => hx(s.replace(/[\t\n]/g, ' ').trim()), 'fn'],
  ['d2b-f10.caption_text', (s) => hx(s.replace(/[\t\n\f]/g, ' ').trim()), 'fn'],
  // :6286, :6377 /^n:(\d+)$/ and /^p:(\d+)$/
  ['d2b-f10.note_item', (s) => { const m = /^n:(\d+)$/.exec(s); return m ? m[1] : '~'; }, 'fn'],
  ['d2b-f10.para_item', (s) => { const m = /^p:(\d+)$/.exec(s); return m ? m[1] : '~'; }, 'fn'],
  // :7330 /^heading [1-9]$/i
  ['d2b-f10.heading_name', (s) => String(/^heading [1-9]$/i.test(s)), 'fn'],
  // :11612 the core.xml property
  ...['dc:creator', 'dc:title'].map((tag) => [`d2b-f10.core_${tag.slice(3)}`, (s) => {
    const m = new RegExp('<' + tag + '[^>]*>([^<]*)<').exec(s);
    return hx(m ? m[1].replace(/&lt;/g, '<').replace(/&gt;/g, '>').replace(/&quot;/g, '"').replace(/&amp;/g, '&') : '');
  }, 'fn']),
  // text-layout.ts:191 LETTER over one code unit and :206 the soft hyphens of a word
  ['d2b-f10.letter', (s) => String(/^[\p{L}\p{M}]$/u.test(s[0] ?? '')), 'fn'],
  ['d2b-f10.shy', (s) => hx(s.replace(/\u00AD/g, '')), 'fn'],
  // :5499 pasteTextOnly
  ['d2b-f10.paste_lines', (s) => s.replace(/\r\n?/g, '\n').split('\n').map(hx).join('/'), 'fn'],
];
