// V8 oracles of uo-text: what Node 24.15 computes for the functions the crate implements, written to
// tests/captures/ and replayed by tests/*.rs (no Node needed at test time). Deterministic: a fixed seed and
// no clock, so a rerun on Node 24.15 is byte-identical (`--check` compares instead of writing).
//   node crates/uo-text/js/capture-oracle.mjs
//
// text_v8.txt        NFC/NFD of every code point, of every first x second pair, of random sequences, the
//                    property classes, toUpperCase/toLowerCase of every code point, the Final_Sigma
//                    contexts, the `/i` Canonicalize, JS whitespace helpers over random UTF-16 strings
// collation_v8.txt   the collation keys of every code point (exceptions to a stated default plus a digest for
//                    Hangul), pairwise comparisons, random pairs and sorts, run on the TS collation.ts itself
import { readFileSync, writeFileSync } from 'node:fs';

const check = process.argv.includes('--check');
const here = (p) => new URL(p, import.meta.url);
if (!/^v24\.15\./.test(process.version)) throw new Error(`the oracles are pinned to Node 24.15, this is ${process.version}`);

const MAX = 0x10ffff;
const hex = (n) => n.toString(16);
const str = (cp) => String.fromCodePoint(cp);
const isSurrogate = (cp) => cp >= 0xd800 && cp <= 0xdfff;
const isHangul = (cp) => cp >= 0xac00 && cp <= 0xd7a3;
const unitsOf = (s) => Array.from({ length: s.length }, (_, i) => s.charCodeAt(i));
const enc = (s) => (s.length === 0 ? '-' : unitsOf(s).map(hex).join(','));
const fromUnits = (a) => String.fromCharCode(...a);
const cpsOf = (s) => [...s].map((c) => c.codePointAt(0));

// FNV-1a 64 over the code units of strings (two bytes each, little-endian, 0xFFFF after each string)
const P64 = 0x100000001b3n;
const M64 = (1n << 64n) - 1n;
function fnvStrings(list) {
  let h = 0xcbf29ce484222325n;
  const byte = (b) => {
    h = ((h ^ BigInt(b)) * P64) & M64;
  };
  for (const s of list) {
    for (let i = 0; i < s.length; i++) {
      const u = s.charCodeAt(i);
      byte(u & 0xff);
      byte(u >> 8);
    }
    byte(0xff);
    byte(0xff);
  }
  return h.toString(16).padStart(16, '0');
}

let seed = 0x2545f491;
const rnd = (n) => {
  seed ^= seed << 13;
  seed >>>= 0;
  seed ^= seed >>> 17;
  seed ^= seed << 5;
  seed >>>= 0;
  return seed % n;
};

const ranges = (set) => {
  const out = [];
  for (let cp = 0; cp <= MAX; cp++) {
    if (!set[cp]) continue;
    let end = cp;
    while (end + 1 <= MAX && set[end + 1]) end++;
    out.push(`${hex(cp)}-${hex(end)}`);
    cp = end;
  }
  return out.join(',') || '-';
};

// ================================================================== text_v8.txt
const L = [];
L.push(`@node ${process.version}`);
L.push(`@unicode ${process.versions.unicode}`);
L.push(`@icu ${process.versions.icu}`);

// --- NFC / NFD of every code point (lines only where the result differs), lone surrogates unchanged
for (let cp = 0; cp <= MAX; cp++) {
  const c = str(cp);
  const nfc = c.normalize('NFC');
  const nfd = c.normalize('NFD');
  if (isSurrogate(cp)) {
    if (nfc !== c || nfd !== c) throw new Error(`a lone surrogate changed at ${hex(cp)}`);
    continue;
  }
  if (nfc !== c) L.push(`nfc1\t${hex(cp)}\t${enc(nfc)}`);
  if (nfd !== c && !isHangul(cp)) L.push(`nfd1\t${hex(cp)}\t${enc(nfd)}`);
}
{
  const syl = [];
  const syl2 = [];
  for (let cp = 0xac00; cp <= 0xd7a3; cp++) {
    syl.push(str(cp).normalize('NFD'));
    syl2.push(str(cp).normalize('NFD').normalize('NFC'));
  }
  L.push(`hangul-nfd\t${fnvStrings(syl)}`);
  L.push(`hangul-nfc\t${fnvStrings(syl2)}`);
  // every L V and L V T jamo sequence composes
  const seq = [];
  for (let l = 0x1100; l <= 0x1112; l++)
    for (let v = 0x1161; v <= 0x1175; v++) {
      seq.push((str(l) + str(v)).normalize('NFC'));
      for (let t = 0x11a8; t <= 0x11c2; t++) seq.push((str(l) + str(v) + str(t)).normalize('NFC'));
    }
  L.push(`hangul-jamo\t${fnvStrings(seq)}`);
}

// --- every first x second pair of the composition tables (and the composites as firsts): which pairs change
{
  const nfdMap = new Map();
  const comps = [];
  for (let cp = 0; cp <= MAX; cp++) {
    if (isSurrogate(cp) || isHangul(cp)) continue;
    const c = str(cp);
    const d = c.normalize('NFD');
    if (d === c) continue;
    const dc = cpsOf(d);
    if (dc.length >= 2 && c.normalize('NFC') === c) comps.push(cp);
    nfdMap.set(cp, dc);
  }
  const firsts = new Set();
  const seconds = new Set();
  for (const cp of comps) {
    const d = nfdMap.get(cp);
    seconds.add(d[d.length - 1]);
    firsts.add(cpsOf(d.slice(0, -1).map(str).join('').normalize('NFC'))[0]);
    firsts.add(cp);
  }
  const A = [...firsts].sort((a, b) => a - b);
  const B = [...seconds].sort((a, b) => a - b);
  L.push(`setA\t${A.map(hex).join(',')}`);
  L.push(`setB\t${B.map(hex).join(',')}`);
  for (const a of A)
    for (const b of B) {
      const s = str(a) + str(b);
      const r = s.normalize('NFC');
      if (r !== s) L.push(`nfcpair\t${hex(a)}\t${hex(b)}\t${enc(r)}`);
    }
}

// --- random sequences: bases, every kind of mark, Hangul jamo, Indic and Tibetan composites, astral, lone surrogates
const seqAlphabet = [
  0x41, 0x61, 0x43, 0x63, 0x45, 0x65, 0x4f, 0x6f, 0x55, 0x75, 0x49, 0x69, 0x53, 0x73, 0x3b1, 0x3b7, 0x391, 0x3a9, 0x410, 0x438, 0x1100, 0x1161, 0x11a8, 0xac00, 0xac01, 0x300, 0x301, 0x302, 0x303,
  0x306, 0x307, 0x308, 0x309, 0x30a, 0x30b, 0x30c, 0x31b, 0x323, 0x324, 0x327, 0x328, 0x331, 0x334, 0x338, 0x345, 0x35c, 0x483, 0x591, 0x5b0, 0x64b, 0x93c, 0x9be, 0x9c7, 0x9d7, 0xb3e, 0xb47,
  0xbc6, 0xbbe, 0xc46, 0xcc2, 0xd46, 0xd3e, 0xdd9, 0xdcf, 0xf71, 0xf72, 0xf74, 0xf80, 0x1b05, 0x1b35, 0x3099, 0x309a, 0x304b, 0x30ab, 0x212b, 0x2126, 0x1e9b, 0x1ebf, 0x1ea0, 0xc5, 0xe9, 0x1f80,
  0x11099, 0x110ba, 0x1d157, 0x1d165, 0x1d1bb, 0x1d16e, 0x11347, 0x1133e, 0x1f600, 0xd800, 0xdc00, 0xdbff, 0xdfff, 0xfb1d, 0xfb2a, 0x2adc, 0xf900, 0x2f800, 0x44, 0x20, 0x1e0c, 0x1e0b,
].filter((x, i, a) => a.indexOf(x) === i);
for (let i = 0; i < 2000; i++) {
  const n = 1 + rnd(9);
  const cps = [];
  for (let k = 0; k < n; k++) cps.push(seqAlphabet[rnd(seqAlphabet.length)]);
  const s = fromUnits(cps.flatMap((cp) => unitsOf(str(cp))));
  L.push(`nfcseq\t${enc(s)}\t${enc(s.normalize('NFC'))}\t${enc(s.normalize('NFD'))}`);
}

// long runs of marks (more than 24: the counting sort of the reordering), after a base that composes with some of them
const markPool = [0x300, 0x301, 0x302, 0x303, 0x308, 0x30a, 0x316, 0x31b, 0x323, 0x327, 0x328, 0x331, 0x334, 0x345, 0x35c, 0x5b0, 0x64b, 0x93c, 0x3099, 0x1d165, 0x1d16e, 0x1d17b];
for (let i = 0; i < 160; i++) {
  const n = 25 + rnd(60);
  const base = [0x61, 0x65, 0x43, 0x3b1, 0x4b, 0x1100, 0xac00][rnd(7)];
  let s = str(base);
  for (let k = 0; k < n; k++) s += str(markPool[rnd(markPool.length)]);
  L.push(`nfcseq\t${enc(s)}\t${enc(s.normalize('NFC'))}\t${enc(s.normalize('NFD'))}`);
}

// --- property classes over every code point
{
  const sets = { L: [], M: [], N: [], P: [] };
  const res = { L: /^\p{L}$/u, M: /^\p{M}$/u, N: /^\p{N}$/u, P: /^\p{P}$/u };
  for (let cp = 0; cp <= MAX; cp++) {
    const c = str(cp);
    for (const k of Object.keys(sets)) sets[k][cp] = res[k].test(c);
  }
  for (const k of Object.keys(sets)) L.push(`prop\t${k}\t${ranges(sets[k])}`);
}

// --- case mapping of every code point (full mapping, root locale); lone surrogates map to themselves
for (let cp = 0; cp <= MAX; cp++) {
  const c = str(cp);
  const u = c.toUpperCase();
  const l = c.toLowerCase();
  if (isSurrogate(cp)) {
    if (u !== c || l !== c) throw new Error(`a lone surrogate changed case at ${hex(cp)}`);
    continue;
  }
  if (u !== c) L.push(`upper\t${hex(cp)}\t${enc(u)}`);
  if (l !== c) L.push(`lower\t${hex(cp)}\t${enc(l)}`);
}

// --- Final_Sigma: the outcome for the sigma in `c S`, `A c S`, `A S c`, `A S c a` and `A S c c` over every code point c
{
  const forms = {
    f1: (c) => c + '\u03a3',
    f2: (c) => 'A' + c + '\u03a3',
    f3: (c) => 'A\u03a3' + c,
    f4: (c) => 'A\u03a3' + c + 'a',
    f5: (c) => 'A\u03a3' + c + c,
    f6: (c) => c + '\u03a3' + c,
  };
  const sigmaAt = {
    f1: (r) => r.length - 1,
    f2: (r) => r.length - 1,
    f3: () => 1,
    f4: () => 1,
    f5: () => 1,
    f6: (r, c) => c.toLowerCase().length,
  };
  for (const k of Object.keys(forms)) {
    const set = [];
    for (let cp = 0; cp <= MAX; cp++) {
      const c = str(cp);
      const r = forms[k](c).toLowerCase();
      const at = sigmaAt[k](r, c);
      const u = r.charCodeAt(at);
      if (u !== 0x3c2 && u !== 0x3c3) throw new Error(`sigma not found for ${k} at ${hex(cp)}`);
      set[cp] = u === 0x3c2;
    }
    L.push(`sigma\t${k}\t${ranges(set)}`);
  }
}

// --- RegExp /i without u: Canonicalize of every code unit as V8 matches it (the spec algorithm over V8's
// toUpperCase, cross-checked against RegExp), listed where it differs from the unit
{
  const canon = (u) => {
    const s = String.fromCharCode(u);
    const up = s.toUpperCase();
    if (up.length !== 1) return u;
    const cu = up.charCodeAt(0);
    if (u >= 128 && cu < 128) return u;
    return cu;
  };
  const esc = (u) => `\\u${u.toString(16).padStart(4, '0')}`;
  const classes = new Map();
  for (let u = 0; u < 0x10000; u++) {
    const cu = canon(u);
    if (!classes.has(cu)) classes.set(cu, []);
    classes.get(cu).push(u);
  }
  for (const [cu, members] of classes) {
    // a regexp of one member must match every other member and nothing outside the class
    if (members.length < 2 && members[0] === cu) continue;
    for (const u of members) {
      const re = new RegExp(`^${esc(u)}$`, 'i');
      for (const w of members) if (!re.test(String.fromCharCode(w))) throw new Error(`/i: ${hex(u)} should match ${hex(w)}`);
    }
  }
  // negatives: random pairs from different classes never match
  for (let i = 0; i < 200000; i++) {
    const a = rnd(0x10000);
    const b = rnd(0x10000);
    const re = new RegExp(`^${esc(a)}$`, 'i');
    if (re.test(String.fromCharCode(b)) !== (canon(a) === canon(b))) throw new Error(`/i: ${hex(a)} vs ${hex(b)}`);
  }
  for (let u = 0; u < 0x10000; u++) if (canon(u) !== u) L.push(`canon\t${hex(u)}\t${hex(canon(u))}`);
}

// --- random UTF-16 strings through every string operation (an `=` means "the same as the input")
const alphabet = [
  ...[0x41, 0x62, 0x5a, 0x7a, 0x30, 0x39, 0x20, 0x2e, 0x27, 0x2d, 0x9, 0xa, 0xb, 0xc, 0xd, 0xa0, 0x1680, 0x2000, 0x200a, 0x200b, 0x2028, 0x2029, 0x202f, 0x205f, 0x3000, 0xfeff, 0x85, 0x180e, 0x2060],
  ...[0xdf, 0x1e9e, 0x149, 0x1f0, 0x390, 0x3b0, 0x587, 0x1e96, 0xfb00, 0xfb01, 0xfb03, 0xfb13, 0x130, 0x131, 0x49, 0x69, 0x3a3, 0x3c3, 0x3c2, 0x386, 0x3ac, 0x1f80, 0x1fb3, 0x1fbc, 0x1ff7, 0x2c, 0x10d0, 0x1c90, 0x13a0, 0xab70],
  ...[0xe9, 0x65, 0x301, 0x300, 0x323, 0x302, 0x1ea0, 0x1100, 0x1161, 0x11a8, 0xac00, 0x2126, 0x212a, 0x212b, 0x17f, 0x1c4, 0x1c5, 0x1c6, 0x3d0, 0x3f4, 0x2c6f, 0x2c7e, 0xa7ae, 0x1c7f, 0x345],
  0xd83d, 0xde00, 0x1f600, 0x10400, 0x10428, 0x1e900, 0x1e922, 0xd800, 0xdc00, 0xdbff, 0xdfff, 0x2f800, 0x1d15e, 0x1d157, 0x1d165,
];
const randomString = (maxLen) => {
  const n = rnd(maxLen + 1);
  let s = '';
  for (let i = 0; i < n; i++) s += str(alphabet[rnd(alphabet.length)]);
  // a sprinkling of raw lone surrogate units next to pairs
  if (rnd(8) === 0) s += String.fromCharCode(0xd800 + rnd(0x800));
  if (rnd(8) === 0) s = String.fromCharCode(0xd800 + rnd(0x800)) + s;
  return s;
};
const fixed = ['', ' ', 'a', 'ASS', '\u03a3', 'A\u03a3', 'A\u03a3 ', 'A.\u03a3', '\u03a3A', 'A\u03a3\u03a3', 'a\u03a3\u0301', 'A\u00ad\u03a3', 'a\u2019\u03a3\u2019b', '\u00df', 'stra\u00dfe', '\u0130stanbul', 'I\u0131i\u0130', '\ufb01', '\u01c5', ' \ufeff x \u00a0', '\u180e a \u0085'];
const same = (r, s) => (r === s ? '=' : enc(r));
const pieces = (s) => s.split(/\s+/).map((p) => (p.length ? enc(p) : '-')).join(';');
for (let i = 0; i < 1800; i++) {
  const s = i < fixed.length ? fixed[i] : randomString(i % 3 === 0 ? 20 : 9);
  L.push(
    [
      'str',
      enc(s),
      same(s.normalize('NFC'), s),
      same(s.normalize('NFD'), s),
      same(s.toUpperCase(), s),
      same(s.toLowerCase(), s),
      same(s.trim(), s),
      same(s.trimStart(), s),
      same(s.trimEnd(), s),
      pieces(s),
      same(s.replace(/\s/g, '--'), s),
      same(s.replace(/\s+/g, '--'), s),
      same(s.replace(/\s+/g, ''), s),
    ].join('\t'),
  );
}

// ================================================================== collation_v8.txt (run on the TS source itself)
const C = [];
C.push(`@node ${process.version}`);
C.push(`@unicode ${process.versions.unicode}`);
const col = await import(new URL('../../../../sheet/packages/calc/src/collation.ts', import.meta.url).href);
const { compareText, collationKey } = col;
const OTHER = 100000;
const CJK = 400000;
const isCjk = (cp) => (cp >= 0x3400 && cp <= 0x9fff) || (cp >= 0xf900 && cp <= 0xfaff) || (cp >= 0x20000 && cp <= 0x3ffff);
const defaultKey = (cp) => `${(isCjk(cp) ? CJK : OTHER) + cp}|0|0|`;
{
  const hangul = [];
  for (let cp = 0; cp <= MAX; cp++) {
    const k = collationKey(str(cp));
    if (isHangul(cp)) {
      hangul.push(k);
      continue;
    }
    if (k !== defaultKey(cp)) C.push(`key\t${hex(cp)}\t${k}`);
  }
  C.push(`key-hangul\t${fnvStrings(hangul)}`);
}
const X = [];
{
  for (let cp = 0x20; cp < 0x7f; cp++) X.push(str(cp));
  const more = [
    0x01, 0x1f, 0x7f, 0x80, 0xa0, 0xa1, 0xa2, 0xa3, 0xa4, 0xa5, 0xa7, 0xa9, 0xaa, 0xab, 0xac, 0xad, 0xae, 0xb0, 0xb1, 0xb2, 0xb3, 0xb5, 0xb6, 0xb9, 0xba, 0xbc, 0xbd, 0xbe, 0xbf, 0xc0, 0xc1, 0xc2, 0xc3, 0xc4,
    0xc5, 0xc6, 0xc7, 0xc8, 0xc9, 0xd0, 0xd1, 0xd7, 0xd8, 0xdf, 0xe0, 0xe1, 0xe2, 0xe3, 0xe4, 0xe5, 0xe6, 0xe7, 0xe8, 0xe9, 0xea, 0xeb, 0xf0, 0xf1, 0xf7, 0xf8, 0x100, 0x110, 0x111, 0x126, 0x127, 0x130, 0x131,
    0x141, 0x142, 0x149, 0x152, 0x153, 0x17f, 0x1c4, 0x1c5, 0x2b0, 0x300, 0x301, 0x302, 0x308, 0x327, 0x338, 0x36f, 0x391, 0x3a3, 0x3b1, 0x3c2, 0x3c3, 0x410, 0x430, 0x5d0, 0x627, 0x1e9e, 0x2000, 0x200a, 0x2010,
    0x2011, 0x2013, 0x2018, 0x2019, 0x20ac, 0x212a, 0x212b, 0x2126, 0x2153, 0x2460, 0x2605, 0x3000, 0x3041, 0x3042, 0x30a1, 0x30a2, 0x30ac, 0x30f6, 0x3400, 0x4e00, 0x9fff, 0xac00, 0xd7a3, 0xf900, 0xfb01, 0xfb00,
    0xff21, 0xff41, 0xff71, 0xff10, 0xfdfa, 0x1f600, 0x20000, 0x1d400, 0x1e900, 0xd800, 0xdc00, 0xfffd,
  ];
  for (const cp of more) X.push(str(cp));
  X.push('e\u0301', 'e\u0300', 'e\u0302', 'e\u0308', 'a\u030a', 'n\u0303', 'c\u0327', 'e\u0301\u0300', 'e\u0300\u0301', '\u00e9', '\u00e8', '\ud83d\ude00', '\ud800\udc00', '\ud83d', '\ude00x');
}
C.push(`xset\t${X.map(enc).join(';')}`);
for (const a of X) {
  let row = '';
  for (const b of X) row += compareText(a, b) < 0 ? '<' : compareText(a, b) > 0 ? '>' : '=';
  C.push(`row\t${row}`);
}
const colAlphabet = [
  ...'abcxyzABCXYZ0129 !"#$%&()*,./:;?@[\\]^_`{|}~+<=>-\''.split(''),
  ...[0xe9, 0xe8, 0xea, 0xe0, 0xe1, 0xe4, 0xe5, 0xe3, 0xf1, 0xe7, 0xdf, 0xe6, 0x153, 0xf8, 0x142, 0x111, 0xaa, 0xba, 0xb9, 0xbc, 0xad, 0x2010, 0x2011, 0xa0, 0x301, 0x300, 0x308, 0x3b1, 0x3a3, 0x3c2, 0x430, 0x30a2, 0x3042, 0xff71, 0xff21, 0xfb01, 0x4e00, 0xac00, 0x1f600, 0xd800, 0xdc00, 0x130, 0x131, 0x212a, 0x20ac, 0xa3, 0xb1, 0xd7, 0xa7].map(str),
];
const colString = (max) => {
  const n = 1 + rnd(max);
  let s = '';
  for (let i = 0; i < n; i++) s += colAlphabet[rnd(colAlphabet.length)];
  return s;
};
for (let i = 0; i < 6000; i++) {
  const a = colString(6);
  // half of the pairs are near-misses: the same text with one edit, to hit the lower levels
  let b = colString(6);
  if (i % 2 === 0) {
    const k = rnd(a.length + 1);
    const ins = colAlphabet[rnd(colAlphabet.length)];
    b = rnd(3) === 0 ? a.slice(0, k) + ins + a.slice(k) : rnd(2) === 0 ? a.slice(0, k) + a.slice(k + 1) : a.slice(0, k) + ins + a.slice(k + 1);
  }
  const r = compareText(a, b);
  C.push(`pair\t${enc(a)}\t${enc(b)}\t${r < 0 ? '<' : r > 0 ? '>' : '='}`);
}
for (let i = 0; i < 1200; i++) {
  const s = colString(8);
  C.push(`ckey\t${enc(s)}\t${collationKey(s)}`);
}
for (let i = 0; i < 24; i++) {
  const list = [];
  for (let k = 0; k < 80; k++) list.push(colString(6));
  const idx = list.map((_, k) => k);
  idx.sort((x, y) => compareText(list[x], list[y]));
  C.push(`sort\t${list.map(enc).join(';')}\t${idx.join(',')}`);
}

// ================================================================== output
const outputs = [
  ['../tests/captures/text_v8.txt', L.join('\n') + '\n'],
  ['../tests/captures/collation_v8.txt', C.join('\n') + '\n'],
];
let bad = false;
for (const [path, text] of outputs) {
  if (check) {
    let have = '';
    try {
      have = readFileSync(here(path), 'utf8');
    } catch {
      have = '';
    }
    if (have !== text) {
      console.error(`${path} differs from what the generator produces`);
      bad = true;
    }
  } else writeFileSync(here(path), text);
  console.log(`${path}: ${text.split('\n').length - 1} lines, ${text.length} bytes`);
}
process.exit(bad ? 1 : 0);
