// V8 oracle of the string helpers of `uo_jsnum` that came from uo-fonts' private `js` module
// (stage 5, D2): the JS whitespace set, `trim`, the `lowerCp` rule of liang.ts (`text::lower_cp`),
// full-string `toLowerCase` (`text::to_lowercase`, Final_Sigma included), `parseInt(s, 16)` on
// UTF-16 code units (`parse::parse_int_utf16`), the `Uint8Array` store (`conv::to_uint8`) and
// `split(/\s+/)` (`text::split_js_space`, empty pieces skipped). Moved from
// `uo-fonts/tests/js_support/gen_js.mjs` unchanged but for these comments and the output path.
// Deterministic (fixed seed): the committed tests/captures/text_v8.txt is what `tests/text_v8.rs` replays.
//   node crates/uo-jsnum/js/capture-text-v8.mjs
import { writeFileSync } from 'node:fs';

const hex = (n) => n.toString(16);
const enc = (s) => (s.length === 0 ? '-' : [...s].map((c) => hex(c.codePointAt(0))).join(','));
const encUnits = (s) => (s.length === 0 ? '-' : Array.from({ length: s.length }, (_, i) => hex(s.charCodeAt(i))).join(','));
// numbers as their IEEE 754 bits (16 hex digits): the Rust side reads them with f64::from_bits,
// never with a float parser (the workspace bans those outside uo-jsnum)
const num = (x) => {
  const v = new DataView(new ArrayBuffer(8));
  v.setFloat64(0, x);
  return v.getBigUint64(0).toString(16).padStart(16, '0');
};
const lines = [];
const isSurrogate = (cp) => cp >= 0xd800 && cp <= 0xdfff;

lines.push(`@node ${process.version}`);
lines.push(`@unicode ${process.versions.unicode}`);

// whitespace: the `\s` class and `String.prototype.trim` over every code point (lone surrogates too)
const space = [];
const trimSet = [];
for (let cp = 0; cp <= 0x10ffff; cp++) {
  const ch = String.fromCodePoint(cp);
  space.push(/\s/.test(ch));
  trimSet.push(ch.trim() === '');
  if (space[cp] !== trimSet[cp]) throw new Error(`\\s and trim differ at ${hex(cp)}`);
}
const ranges = (set) => {
  const out = [];
  for (let cp = 0; cp <= 0x10ffff; cp++) {
    if (!set[cp]) continue;
    let end = cp;
    while (end + 1 <= 0x10ffff && set[end + 1]) end++;
    out.push(`${hex(cp)}-${hex(end)}`);
    cp = end;
  }
  return out.join(',');
};
lines.push(`space\t${ranges(space)}`);
lines.push(`trim\t${ranges(trimSet)}`);

// liang.ts lowerCp, copied: the lowercase of one code point, kept only when it is one code point
// or has the length of the original in UTF-16 units; otherwise the original
const lowerCp = (ch) => {
  const l = ch.toLowerCase();
  const cp = l.codePointAt(0);
  return l.length === ch.length || String.fromCodePoint(cp) === l ? cp : ch.codePointAt(0);
};
for (let cp = 0; cp <= 0x10ffff; cp++) {
  if (isSurrogate(cp)) continue;
  const ch = String.fromCodePoint(cp);
  const r = lowerCp(ch);
  if (r !== cp) lines.push(`lower\t${hex(cp)}\t${hex(r)}`);
  const l = ch.toLowerCase();
  if (l !== ch) lines.push(`lowerstr\t${hex(cp)}\t${enc(l)}`);
}

// Final_Sigma contexts: every string up to 4 symbols over 8 symbols (cased letters, a caseless
// digit, case-ignorable characters and separators) that contains a capital sigma, plus seeded
// longer ones over 11 symbols
const alphabet = ['A', 'a', '\u03a3', '\u03c3', '.', ' ', '\u0301', '1', '\u00ad', "'", '-'];
const withSigma = [];
const build = (prefix, left) => {
  if (prefix.includes('\u03a3') && prefix.length > 0) withSigma.push(prefix);
  if (left === 0) return;
  for (const a of alphabet.slice(0, 8)) build(prefix + a, left - 1);
};
build('', 4);
let seed = 0x2545f491;
const rnd = (n) => {
  seed ^= seed << 13;
  seed >>>= 0;
  seed ^= seed >>> 17;
  seed ^= seed << 5;
  seed >>>= 0;
  return seed % n;
};
for (let i = 0; i < 600; i++) {
  let s = '\u03a3';
  const n = 5 + rnd(12);
  for (let k = 0; k < n; k++) s += alphabet[rnd(alphabet.length)];
  withSigma.push(s);
  withSigma.push(s.split('').reverse().join(''));
}
const seenSigma = new Set();
for (const s of withSigma) {
  if (seenSigma.has(s)) continue;
  seenSigma.add(s);
  lines.push(`sigma\t${enc(s)}\t${enc(s.toLowerCase())}`);
}

// parseInt(s, 16) on UTF-16 code units: the first unit in a table of interesting ones, the second
// over all ASCII (2-unit strings, as the odttf keys slice them)
const firsts = [
  ...Array.from({ length: 128 }, (_, i) => i),
  0x85, 0xa0, 0xe9, 0x1680, 0x180e, 0x2000, 0x200a, 0x200b, 0x2028, 0x2029, 0x202f, 0x205f, 0x3000, 0xfeff, 0xd800, 0xd83d, 0xdbff, 0xdc00, 0xde00, 0xdfff, 0xff10,
];
for (const u1 of firsts) {
  const res = [];
  for (let u2 = 0; u2 < 128; u2++) res.push(num(parseInt(String.fromCharCode(u1, u2), 16)));
  lines.push(`pi2\t${hex(u1)}\t${res.join(',')}`);
}
const piCases = ['', '0', '-', '+', '-0', '+0', '0x', '0X', '0xg', '0x1', '0XfF', '-0x1f', '+0x1f', '0x-1', ' \t\n\u00a0\ufeff\u3000ff', '  -  1', '-\u00a01', 'ff ', 'f f', 'f.f', 'fg', 'FG', '1g', '0g', 'g1',
  '\u{1f600}', 'f\u{1f600}', '\ud83d', 'a\udc00', '\u0661\u0662', '\uff11', '\u00e9', 'e9', '0x\ud83d', ' 0x ',
  'ffffffffffffffff', 'fffffffffffffffff', '0'.repeat(40) + 'ff', '0'.repeat(40) + '0', '1' + '0'.repeat(40), '1' + '0'.repeat(300), 'f'.repeat(30), 'f'.repeat(31), 'f'.repeat(32), 'f'.repeat(33),
  'f'.repeat(255), 'f'.repeat(256), 'f'.repeat(257), 'f'.repeat(300), '-' + 'f'.repeat(300), '1' + '0'.repeat(255), '1' + '0'.repeat(256), '10000000000000000000000000000001', '1000000000000080000000000000000000000001',
  '100000000000008', '100000000000009', '10000000000000800000', '1000000000000080000000000000000000000000000000000000000000000000001',
  '1fffffffffffff', '20000000000001', '20000000000002', '20000000000003', '3fffffffffffff', '7ffffffffffffc', '7ffffffffffffd', '1fffffffffffffffffffffffffffff', '1fffffffffffffffffffffffffffff1'];
for (const s of piCases) lines.push(`pi\t${encUnits(s)}\t${num(parseInt(s, 16))}`);
const piAlphabet = '0123456789abcdefABCDEFxX+- gG\t.\u00a0';
for (let i = 0; i < 1500; i++) {
  let s = '';
  const n = rnd(36);
  for (let k = 0; k < n; k++) s += piAlphabet[rnd(piAlphabet.length)];
  lines.push(`pi\t${encUnits(s)}\t${num(parseInt(s, 16))}`);
}

// ToUint8 (the store of a number into a Uint8Array)
const xs = [0, -0, 1, -1, 255, 256, 257, -255, -256, -257, 255.9, 256.5, -0.5, -0.9, -1.5, -255.5, 0.5, 1e3, -1e3, 1e9, -1e9, 4294967295, 4294967296, 4294967297, -4294967296, -4294967297,
  2 ** 53, 2 ** 53 + 2, -(2 ** 53), 2 ** 53 - 1, 2 ** 62, 2 ** 62 + 2 ** 11, 2 ** 63 - 1024, 2 ** 63, 2 ** 63 + 2048, -(2 ** 63), -(2 ** 63) - 2048, 2 ** 64, 2 ** 64 + 4096, 1e20, -1e20, 1e21, 1e300,
  Number.MAX_VALUE, -Number.MAX_VALUE, Number.MIN_VALUE, -Number.MIN_VALUE, Infinity, -Infinity, NaN, 3e18 + 1e3, 9.223372036854775e18, -9.223372036854775e18, 1.5e19, 123456789012345680000];
for (const x of xs) lines.push(`u8\t${num(x)}\t${new Uint8Array([x])[0]}`);
for (let i = 0; i < 400; i++) {
  const x = (rnd(2000000) - 1000000) / (1 + rnd(8)) * 7 ** rnd(12);
  lines.push(`u8\t${num(x)}\t${new Uint8Array([x])[0]}`);
}

// split(/\s+/) with the empty tokens skipped
const spaceChars = ['\t', '\n', '\u000b', '\f', '\r', ' ', '\u00a0', '\u1680', '\u2000', '\u200a', '\u2028', '\u2029', '\u202f', '\u205f', '\u3000', '\ufeff'];
const other = ['a', 'b', '\u0085', '\u180e', '\u200b', '\u2060', '\u00e9', '\u{1f600}', '-', '\u0301'];
const pool = [...spaceChars, ...other];
for (const s of ['', ' ', 'a', ' a', 'a ', ' a ', 'a b', 'a  b', 'a\u00a0\u2028b', '\ufeffa\ufeff', ...spaceChars.map((c) => `a${c}b`), ...other.map((c) => ` ${c} `)]) {
  lines.push(`split\t${enc(s)}\t${s.split(/\s+/).filter(Boolean).map(enc).join(';') || '-'}`);
}
for (let i = 0; i < 700; i++) {
  let s = '';
  const n = rnd(14);
  for (let k = 0; k < n; k++) s += pool[rnd(pool.length)];
  lines.push(`split\t${enc(s)}\t${s.split(/\s+/).filter(Boolean).map(enc).join(';') || '-'}`);
}

writeFileSync(new URL('../tests/captures/text_v8.txt', import.meta.url), lines.join('\n') + '\n');
console.log(`${lines.length} lines, node ${process.version}, unicode ${process.versions.unicode}`);
