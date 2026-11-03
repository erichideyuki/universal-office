// The regex sites of frontier R4c (document.ts `readSingleRun`, `readParagraph`,
// `parseCustomInlineTag`), checked by tests/regex_r4c.rs: the Rust of each site (matcher template
// 5 `Cur`, template 2 `find_word`, template 6 `replace_char`/`newlines_to_space`, template 4
// `all_in`) against V8.
import { INPUTS as BASE } from './templates.mjs';

/** [site id, the TS regex (or [regex, replacement] for `replace`), operation]. */
export const SITES = [
  // document.ts:1863: a run inside the result of a TOC field (the instruction of an open field)
  ['c1.toc_instr', /^\s*TOC\b/i, 'test'],
  // document.ts:2107: a TOC instruction anywhere in the paragraph's instrText
  ['c2.toc_word', /\bTOC\b/, 'test'],
  ['c2.toc_word_at', /\bTOC\b/, 'index'],
  // document.ts:1937: a literal soft hyphen is a non-breaking hyphen
  ['c3.shy_to_nb', [/­/g, '‑'], 'replace'],
  // document.ts:1937: literal line breaks in text are spaces
  ['c4.newlines', [/\r\n|[\r\n]/g, ' '], 'replace'],
  // document.ts:1766: the type of a custom-inline tag
  ['c5.inline_type', /^[a-zA-Z0-9._-]+$/, 'test'],
];

export const INPUTS = [...new Set([
  ...BASE,
  ' TOC \\o "1-3" \\h \\z \\u ', 'TOC', 'toc', 'Toc', 'TOCX', 'XTOC', 'TOC_', '_TOC', 'TOC1', '1TOC', '  TOC', '\tTOC', '\n TOC', ' TOC', '﻿TOC', ' TOC',
  'TOCé', 'éTOC', 'ſ TOC', 'K TOC', 'a TOC b', 'a-TOC-b', 'a.TOC.b', 'tOc', 'TOC TOC', 'xTOC TOC', 'TOCTOC', ' IF TOC ', '=TOC=',
  'a­b', '­', '­­', 'x‑y', 'a\r\nb', 'a\rb', 'a\nb', '\r\n\r\n', '\n\r', '\r', '\n', 'a\r\r\nb', 'a b', 'a\u0085b',
  'clickcompliance.document-tag', 'a.b-c_d9', 'a b', 'a/b', 'a:b', '-', '_', '.', '9', 'é', 'aé', 'A', 'z', 'a\n', 'a ', '😀', 'a😀',
])];
