// The regex sites of frontier R6 (fulldoc.ts `openOf`, document.ts `captureParagraph`: both
// `.replace(/\s*\/>$/, '>')` over the text of a start tag, a self-closing element or a tag cut
// from the source) against V8, checked by tests/regex_r6.rs. A row is `[id, regex, op]`.

/** [site id, the TS regex (or [regex, replacement] for `replace`), operation]. */
export const SITES = [
  ['s1.open_of', [/\s*\/>$/, '>'], 'replace'],
  ['s2.para_open', [/\s*\/>$/, '>'], 'replace'],
];

const tags = [
  '<w:p/>', '<w:p />', '<w:p  \t\n/>', '<w:p\r\n/>', '<w:p w14:paraId="0A0B0C0D"/>', '<w:p w14:paraId="0A0B0C0D" />', '<w:p a="1"\n\t/>',
  '<w:p>', '<w:p a="1">', '<w:p a="/>">', "<w:p a='/>'>", '<w:p a="x/>', '<w:p a="x" / >', '<w:p a="x"/ >', '<w:p/> ', ' <w:p/>', '<w:p/>\n', '<w:p//>', '<w:p / />',
  '<w:p\u00a0/>', '<w:p\u2028/>', '<w:p\u3000/>', '<w:p\ufeff/>', '<w:p\u0085/>', '<w:p\u200b/>', '<w:p\u00a0\u2003/>',
  '<w:tc/>', '<w:tc />', '<w:tr w14:paraId="1"/>', '<w:tbl a="é"/>', '<w:tbl a="😀" />', '<w:tbl>', '/>', ' />', '', '>', '\n/>', 'x/>', 'x />',
  '<w:p/>/>', '<w:p/> />', '<w:p />\t', '<w:p / \n />',
];
export const INPUTS = tags;
