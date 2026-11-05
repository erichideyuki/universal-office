// The regex sites of frontier D2b-F9 (editor find-engine.ts, navigation.ts and universal-write.ts
// `goTo`) against V8, checked by tests/regex_d2b_f9.rs through `find::regex_site`. A row is
// `[id, function, 'fn']` (see js/regex-diff.mjs); the TS expressions are copied as written and the
// results returned as ASCII (UTF-16 units in hex, `.`-joined) so that lone surrogates compare exactly.
// The RegExp the find engine BUILDS is not a site here: it is compared whole by the ops capture
// (compileQuery's source and V8's errors, findAll's hits).
import { INPUTS as BASE } from './templates.mjs';

const hx = (t) => [...Array(t.length).keys()].map((i) => t.charCodeAt(i).toString(16)).join('.');
const esc = (c) => c.replace(/[\\^$.*+?()[\]{}|/-]/g, (m) => `\\${m}`);

export const INPUTS = [
  ...BASE,
  '', 'a', 'A', 'e-mail', 'a/b', '\\', '^', '$', '.', '*', '+', '?', '(', ')', '[', ']', '{', '}', '|', '/', '-', '😀', '\uD83D', '\uDE00', 'é', 'ção',
  '0150', 'u8212', 'u', 'u1', '123456', '99999', '0', '00', 'x12', 'u0', '1114112',
  '{2}', '{2,3}', '{2;3}', '{2;}', '{2,}', '{}', '{a}', '{2', '{2;3', '{2;3x}', '{99999999999999999999}', '{2,;}', '{2;3;4}', '{ 2}',
  '(?<!x)', '(?!x)', '(?=x)', '(?:x)', '((?<', '(', '(?', '(?<=x)',
  "abc'", "'", "it's", 'it’s', 'Ação', 'a1', 'a b', 'ǅ', 'ͅ',
  ' ', '\t', ' ', ' ', '﻿', ' ', 'a᠎b',
  'KNIGHT', 'gnome', 'Wright', 'xylophone', 'whale', 'aeon', 'night', 'phone', 'Schmidt', 'thomas', 'judge', 'signed', 'sign', 'dumb', 'ghost', 'church',
  'boxes', 'cities', 'stopped', 'flying', 'better', 'went', 'mouse', 'try', 'run', 'stop', 'make', 'sh', 'ch', 'quiz',
  "word''", 'word’’', "word'’'", "'''",
  'en', 'en-US', 'EN', 'eng', 'en_', 'en1', 'pt-BR', 'en ', 'e', 'En-gb',
  'AÇÃO 1', 'ação!', '123', '😀a', 'ßtraße', 'ŉ',
  '\u001d', 'a\u001db\u001d',
  '+', '-', '+2', '-1', '+12x', '1', '42', '0042', '4 2', '+ 2', '', '٣', '1e3',
  ' PAGE \\* MERGEFORMAT', 'HYPERLINK "x"', '  ', '\tREF\tx', 'page', '',
  '\t\n\r\u000b\u000c￼', 'a\u000bb',
  '<w:drawingGridHorizontalSpacing w:val="120"/>', '<w:drawingGridHorizontalSpacing w:other="1" w:val="9" w:val="11"/>',
  '<w:drawingGridHorizontalSpacing xw:val="3"/>', '<w:drawingGridHorizontalSpacingX w:val="5"/><w:drawingGridHorizontalSpacing w:val="7"/>',
  '<w:drawingGridHorizontalSpacing w:val="12x"/><w:drawingGridHorizontalSpacing w:val="13"/>', '<w:drawingGridHorizontalSpacing w:val="-0"/>',
  '<w:drawingGridHorizontalSpacing w:val="4>6"/>', '<w:drawingGridHorizontalSpacing\nw:val="44"/>', '<w:drawingGridHorizontalSpacing w:val="99999999999999999999"/>',
  '<w:doNotUseMarginsForDrawingGridOrigin/>', '<w:doNotUseMarginsForDrawingGridOrigin w:val="0"/>', '<w:doNotUseMarginsForDrawingGridOrigin w:val="false"/>',
  '<w:doNotUseMarginsForDrawingGridOrigin w:val="off"/>', '<w:doNotUseMarginsForDrawingGridOrigin w:val="1"/>', '<w:doNotUseMarginsForDrawingGridOriginX/>',
  '<w:doNotUseMarginsForDrawingGridOrigin w:val="0"/><w:doNotUseMarginsForDrawingGridOrigin/>', '<w:doNotUseMarginsForDrawingGridOrigin x="w:val=&quot;0&quot;"/>',
];

const gridVal = (x) => { const m = new RegExp('<w:drawingGridHorizontalSpacing\\b[^>]*\\bw:val="(-?\\d+)"').exec(x); return m ? String(+m[1]) : 'none'; };
const gridFlag = (x) => String(new RegExp('<w:doNotUseMarginsForDrawingGridOrigin\\b(?![^>]*w:val="(?:0|false|off)")').test(x));

/** [site id, the TS expression, 'fn']. */
export const SITES = [
  // find-engine.ts:112 esc
  ['d2b-f9.esc', (s) => hx(esc(s)), 'fn'],
  // find-engine.ts:121 /^(u?)(\d{1,5})/
  ['d2b-f9.num', (s) => { const m = /^(u?)(\d{1,5})/.exec(s); return m ? `${m[1]}|${m[2]}` : 'null'; }, 'fn'],
  // find-engine.ts:198 /^\{(\d+)(?:([,;])(\d*))?\}/
  ['d2b-f9.interval', (s) => { const m = /^\{(\d+)(?:([,;])(\d*))?\}/.exec(s); return m ? `${m[0].length}|${m[1]}|${m[2] ?? '~'}|${m[3] ?? '~'}` : 'null'; }, 'fn'],
  // find-engine.ts:194 /^\(\?[<!=]/
  ['d2b-f9.lookaround_atom', (s) => String(/^\(\?[<!=]/.test(s)), 'fn'],
  // find-engine.ts:241 /^[\p{L}']+$/u
  ['d2b-f9.word_or_quote', (s) => String(/^[\p{L}']+$/u.test(s)), 'fn'],
  // find-engine.ts:245 /\p{P}/u over a token's character (here: the first code point of the input)
  ['d2b-f9.punct', (s) => { const c = s.length ? String.fromCodePoint(s.codePointAt(0)) : ''; return String(/\p{P}/u.test(c)); }, 'fn'],
  // find-engine.ts:251 /\s/
  ['d2b-f9.space', (s) => String(/\s/.test(s)), 'fn'],
  // find-engine.ts:442 /[\p{L}'’]+/gu and :443 /['’]+$/: the words of the sounds-like scan
  ['d2b-f9.words', (s) => [...s.matchAll(/[\p{L}'’]+/gu)].map((m) => `${m.index}:${hx(m[0].replace(/['’]+$/, ''))}`).join(' ') || '-', 'fn'],
  // find-engine.ts:450 /^en\b/i
  ['d2b-f9.en', (s) => String(/^en\b/i.test(s)), 'fn'],
  // find-engine.ts:491 /[^\p{L}]/gu (letters) and :495 /\p{L}/u (search)
  ['d2b-f9.letters', (s) => `${hx(s.replace(/[^\p{L}]/gu, ''))}|${s.search(/\p{L}/u)}`, 'fn'],
  // find-engine.ts:510 /[1-9]/ over one character
  ['d2b-f9.digit19', (s) => String(/[1-9]/.test(s.slice(0, 1))), 'fn'],
  // find-engine.ts:535 /\u001D/g
  ['d2b-f9.found_text', (s) => hx(s.replace(/\u001D/g, '\r')), 'fn'],
  // navigation.ts:128 WORD_CHAR over one character (text[k], a code unit; undefined reads "undefined")
  ['d2b-f9.word_char', (s) => String(/[\p{L}\p{N}_]/u.test(s[0])), 'fn'],
  // navigation.ts:145 clean
  ['d2b-f9.clean', (s) => hx(s.replace(/[\t\n\r\u000b\u000c￼]/g, ' ')), 'fn'],
  // navigation.ts:181 val() and :184 the flag of drawingGridOf
  ['d2b-f9.grid_val', gridVal, 'fn'],
  ['d2b-f9.grid_flag', gridFlag, 'fn'],
  // universal-write.ts:18313 goTo's target: rel and abs
  ['d2b-f9.goto_target', (s) => {
    const t = s.trim();
    const rel = /^[+-]\d*$/.test(t) ? (t.length > 1 ? parseInt(t, 10) : t[0] === '-' ? -1 : 1) : t === '' ? 1 : null;
    const abs = rel === null && /^\d+$/.test(t) ? parseInt(t, 10) : null;
    return `${rel}|${abs}`;
  }, 'fn'],
  // universal-write.ts:18407 the field name: (instr ?? '').trim().split(/\s+/)[0]?.toUpperCase()
  ['d2b-f9.field_name', (s) => hx(s.trim().split(/\s+/)[0]?.toUpperCase() ?? '~'), 'fn'],
];
