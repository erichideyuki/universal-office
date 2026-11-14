// The English (US) hyphenation data of the Rust layout under Word's rules (WordCraft study, area "fields", item D1, option
// H; record core/docs/modules/uo-docx-layout.md "WordCraft study — fields"): WordCraft's CC0 patterns plus, as exceptions,
// the Moby Hyphenator II entries whose break points (minimums 2/3) differ from those patterns' — 96.0 % of Word's English
// paragraphs on the 1,150-word measurement (`hyphword1k`), against 88.3 % with hyph-utf8's `en-us`.
//
// Inputs (read-only, from the WordCraft repository's history; no download):
//   git -C <wordcraft> show 3110dcf0^:assets/hyphenation/en-us.pat > en-us.pat
//     sha256 0ad447633731c6c9faaf02ac1b327934eaac8aab23f658624d6c67ffe44b88b8
//     "DesignCraft US English hyphenation patterns", trained from the Moby Hyphenator II word list; CC0-1.0
//   git -C <wordcraft> show 3110dcf0^:assets/hyphenation/en-us.dic > en-us.dic
//     sha256 66ce9023847dd2cfc5c6ad14b83aaab94e58607fe95258f4f3ac431a23df63f1
//     Moby Hyphenator II by Grady Ward (public domain, Project Gutenberg #3204) as DesignCraft's DCHYPH1 (raw deflate,
//     front-coded lines: the first byte is the length of the prefix shared with the line before)
//
// Outputs: the uo-fonts pattern file (`crates/uo-fonts/data/hyphen/en-us-word.txt`, the format of `parse_patterns`) and
// the JS module the wasm shell fetches (`crates/uo-wasm/js/hyphen/en-us-word.mjs`).
//
//   node core/tools/hyphen/gen_en_word.mjs <en-us.pat> <en-us.dic> <out.txt> <out.mjs>
import { readFileSync, writeFileSync } from 'node:fs';
import { createHash } from 'node:crypto';
import { inflateRawSync, deflateRawSync, gzipSync, brotliCompressSync } from 'node:zlib';

const [patPath, dicPath, outTxt, outMjs] = process.argv.slice(2);
if (!outMjs) throw new Error('usage: gen_en_word.mjs <en-us.pat> <en-us.dic> <out.txt> <out.mjs>');
const patBytes = readFileSync(patPath);
const dicBytes = readFileSync(dicPath);
const sha = (b) => createHash('sha256').update(b).digest('hex');
const PAT_SHA = '0ad447633731c6c9faaf02ac1b327934eaac8aab23f658624d6c67ffe44b88b8';
const DIC_SHA = '66ce9023847dd2cfc5c6ad14b83aaab94e58607fe95258f4f3ac431a23df63f1';
if (sha(patBytes) !== PAT_SHA || sha(dicBytes) !== DIC_SHA) throw new Error('the inputs are not the recorded files (sha256)');

// patterns: every whitespace token outside % comments
const pats = patBytes.toString('utf8').split('\n').map((l) => l.replace(/%.*/, '')).join(' ').split(/\s+/).filter(Boolean);
const trie = new Map();
let maxLen = 0;
for (const t of pats) {
  let letters = '';
  const vals = [0];
  for (const ch of t) {
    if (ch >= '0' && ch <= '9') vals[vals.length - 1] = +ch;
    else { letters += ch; vals.push(0); }
  }
  trie.set(letters, vals);
  maxLen = Math.max(maxLen, [...letters].length);
}
const points = (word) => {
  const s = [...('.' + word.toLowerCase() + '.')];
  const v = new Array(s.length + 1).fill(0);
  for (let i = 0; i < s.length; i++) for (let j = i + 1; j <= Math.min(s.length, i + maxLen); j++) {
    const p = trie.get(s.slice(i, j).join(''));
    if (p) p.forEach((x, q) => { if (x > v[i + q]) v[i + q] = x; });
  }
  const n = s.length - 2;
  const out = [];
  for (let k = 1; k < n; k++) if (v[k + 1] & 1) out.push(k);
  return out;
};
const mins = (pts, n) => pts.filter((k) => k >= 2 && n - k >= 3);

// Moby: single-token entries, a lower-case entry winning over a capitalised one
const raw = inflateRawSync(dicBytes);
if (raw.subarray(0, 8).toString('latin1') !== 'DCHYPH1\n') throw new Error('not DCHYPH1');
const dict = new Map();
let cur = Buffer.alloc(0);
for (let i = 8; i < raw.length;) {
  const pre = raw[i];
  const e = raw.indexOf(10, i + 1);
  cur = Buffer.concat([cur.subarray(0, pre), raw.subarray(i + 1, e)]);
  i = e + 1;
  const s = cur.toString('utf8');
  if (/\s/.test(s)) continue;
  const w = s.replace(/-/g, '');
  const pts = [];
  let n = 0;
  for (const ch of s) { if (ch === '-') pts.push(n); else n++; }
  const key = w.toLowerCase();
  const lowerEntry = w === key;
  if (!dict.has(key) || (lowerEntry && !dict.get(key).lower)) dict.set(key, { pts, lower: lowerEntry });
}
// the exceptions: the entries whose points (minimums 2/3) the patterns get wrong
const exc = [];
for (const [w, { pts }] of dict) {
  const n = [...w].length;
  if (mins(pts, n).join(',') !== mins(points(w), n).join(',')) {
    const set = new Set(pts);
    exc.push([...w].map((c, i) => (set.has(i) ? '-' : '') + c).join(''));
  }
}
exc.sort();
const header = [
  "# English (US) hyphenation of the Rust layout under Word's rules (WordCraft study D1, option H; core/tools/hyphen/gen_en_word.mjs):",
  "# DesignCraft's CC0 patterns plus, as exceptions, the Moby Hyphenator II entries whose break points (minimums 2/3) differ from them.",
  '# Patterns: "DesignCraft US English hyphenation patterns", trained from the Moby Hyphenator II word list; public domain (CC0-1.0).',
  `#   source: WordCraft repository, 3110dcf0^:assets/hyphenation/en-us.pat — sha256 ${PAT_SHA}`,
  '# Exceptions: Moby Hyphenator II by Grady Ward (public domain, Project Gutenberg #3204), as converted by the DesignCraft contributors.',
  `#   source: WordCraft repository, 3110dcf0^:assets/hyphenation/en-us.dic — sha256 ${DIC_SHA}`,
  'id en-us-word', 'left 2', 'right 3', '[patterns]'];
const wrap = (list) => {
  const lines = [];
  let l = '';
  for (const t of list) {
    if (l.length + t.length + 1 > 120) { lines.push(l); l = t; } else l = l ? `${l} ${t}` : t;
  }
  if (l) lines.push(l);
  return lines;
};
const text = [...header, ...wrap(pats), '[exceptions]', ...wrap(exc), ''].join('\n');
writeFileSync(outTxt, text);
// (a legal comment: the bundler of the npm packages keeps it in the package's *.LEGAL.txt)
const mjs = [
  '/*! @license CC0-1.0 AND LicenseRef-Public-Domain — English hyphenation data of the Rust layout (UniversalOffice).',
  ' * Patterns: "DesignCraft US English hyphenation patterns", trained from the Moby Hyphenator II word list; released into the',
  ' * public domain (CC0-1.0) by the DesignCraft contributors. Exceptions: Moby Hyphenator II by Grady Ward (public domain,',
  ' * Project Gutenberg #3204), as converted by the DesignCraft contributors. Notices: core/crates/uo-fonts/data/hyphen/',
  ' * THIRD-PARTY-NOTICES.md. */',
  '// @generated by core/tools/hyphen/gen_en_word.mjs (WordCraft study D1) — do not edit.',
  `export default { id: 'en-us-word', left: 2, right: 3, patterns: ${JSON.stringify(pats.join(' '))}, exceptions: ${JSON.stringify(exc.join(' '))} };`,
  '',
].join('\n');
writeFileSync(outMjs, mjs);
const b = Buffer.from(text);
console.log(JSON.stringify({
  patterns: pats.length, moby: dict.size, exceptions: exc.length, raw: b.length, deflate9: deflateRawSync(b, { level: 9 }).length,
  gzip9: gzipSync(b, { level: 9 }).length, brotli: brotliCompressSync(b).length, mjsRaw: Buffer.byteLength(mjs),
  mjsGzip9: gzipSync(Buffer.from(mjs), { level: 9 }).length, sha256: sha(b),
}));
