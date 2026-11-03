// The template rows of read/matchers.rs (G9), checked by tests/regex_templates.rs. A frontier
// adds its own file here (`r1.mjs`…, same exports) and its own Rust test (`tests/regex_r1.rs`).

/** [site id, the TS regex (or [regex, replacement] for `replace`), operation]. */
export const SITES = [
  ['t1.one_of', /^(1|true)$/, 'test'],
  ['t2.word', /\bTOC\b/, 'index'],
  ['t2.word_ci', /\bTOC\b/i, 'index'],
  ['t2.lit_boundary', /<a:blipFill\b/, 'index'],
  ['t4.non_digits', [/\D/g, ''], 'replace'],
  ['t4.all_in', /^[a-zA-Z0-9._-]+$/, 'test'],
  ['t5.toc_prefix', /^\s*TOC\b/i, 'test'],
  ['t5.val_int', /^val\s+(-?\d+)/, 'group'],
  ['t6.shy', [/­/g, '‑'], 'replace'],
  ['t6.newlines', [/\r\n|[\r\n]/g, ' '], 'replace'],
  ['t6.self_closing', [/\s*\/>$/, '>'], 'replace'],
  ['t7.element_text', new RegExp('<' + 'dcterms:created' + '[^>]*>([^<]*)<'), 'group'],
];

/** Inputs every row runs on (the reader's strings come from XML: no lone surrogates). */
export const INPUTS = [
  '', ' ', '1', 'true', 'TRUE', ' 1', '1 ', 'TOC', ' TOC \\o "1-3"', 'xTOC TOC', 'TOC_', 'toc', 'Toc\\h', ' TOC', '﻿TOC', 'é toc',
  'ſ toc', 'K TOC', 'TOCé', '_TOC', 'TOC1', '\tTOC\n', 'val 12', 'val -3x', 'val-3', 'val  ', 'val\t+5', 'VAL 1',
  '<a:blipFillx/><a:blipFill r:embed="x"/>', '<a:blipFill', '<a:blipFill>', '<a:blipFill_/>', 'a\r\nb\rc\nd\n\r', '<w:p w14:paraId="1" \t\n/>',
  '<w:p/>x', '<w:p>', '/>', '  />', '<dcterms:created xsi:type="x">2024-01-01T00:00:00Z</dcterms:created>', '<dcterms:createdX>v<',
  '<dcterms:created>no end', '<x/><dcterms:created a="<">t<', '_x0000_s1026', 'pic.a-b_1', 'pic a', 'ab­cd­', 'é😀',
];
