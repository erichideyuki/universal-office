// The regex sites of the diff (D5a: compare diff.ts `wordTokens`, `intraWordMarks`) against V8, checked by
// tests/regex_d5a_compare.rs. A row is `[id, function, 'fn']` (see js/regex-diff.mjs). Offsets are UTF-16
// code units, as in the TS.
const wordTokens = (text) => {
  const out = [];
  const re = /\S+|\s+/g;
  let m;
  while ((m = re.exec(text)) !== null) out.push([m.index, m.index + m[0].length]);
  return out;
};
const hasWord = (text) => /\S/.test(text);

/** [site id, the TS expression, operation]. */
export const SITES = [
  ['d5a.word_tokens', wordTokens, 'fn'],
  ['d5a.has_word', hasWord, 'fn'],
];

export const INPUTS = [
  '', ' ', 'a', 'ab', 'a b', ' a', 'a ', ' a ', 'the quick brown fox', 'the  quick\tbrown\nfox\r\n', '  ', '\t\n\r', '   a   b   ',
  ' ', '﻿', '\u0085', '᠎', '​', ' ', ' ', ' ', '　', 'a b', 'a﻿b', 'a\u0085b', 'a᠎b', 'a​b',
  '😀', 'a😀b', '😀 😀', '日本語 😀 emoji', 'é è', 'é x', 'x‍y z',
  '*bold* _it_', 'a,b.c;d', 'a-b c_d', "it's", '<u>x</u>', 'p|q r', '1. item', '- item',
  'one two three four five six seven eight nine ten', 'x'.repeat(40), ' '.repeat(40), 'ab '.repeat(20),
];
