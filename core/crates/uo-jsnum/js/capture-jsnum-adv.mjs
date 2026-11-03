// Adversarial parity capture of the string parsers and the integer conversions of uo-jsnum (`to_number`, `parse_int`, ToIntN), taken
// by the parity verifier of increment A (record core/docs/modules/uo-jsnum.md, "Stage 4 — parity").
// The reference is V8 13.6.233.17 (Node 24.15.0) x64, like capture-jsnum.mjs; no math function is
// involved, so the capture may be taken on any x64 OS.
//
//   node core/crates/uo-jsnum/js/capture-jsnum-adv.mjs [--out <file>] [--n <full count>]
//
// writes core/crates/uo-jsnum/tests/captures/adv.capture, replayed by tests/adv.rs:
//
// - `adv/cat<k>` and `adv/mixed`: digests over strings from a structured generator (long digit
//   runs, many leading zeros, the boundaries of the double range, exponent overflow, subnormals,
//   `0x`/`0o`/`0b` runs with ties at bit 53, `Infinity` variants, JS white space at both ends, dot
//   forms, 309/310/311-digit integers) of `Number(s)` and `parseInt(s, radix)` for nine radices.
//   The generator is mirrored line by line in tests/adv.rs (same xorshift32 draws, same
//   order); the category is fixed per record so a failure names its family.
// - `hard/<i>`: strings written in full, the exact decimal expansion of the midpoint between two
//   neighbouring doubles (normal and subnormal, exact, a hair above and a hair below), with
//   `Number(s)` and `parseInt(s, 10)`: the inputs a correctly rounded parser must get right.
import { mkdirSync, readFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { CaptureWriter, Rng, Stream } from '../../uo-testkit/js/testkit.mjs';

const here = dirname(fileURLToPath(import.meta.url));
const crate = join(here, '..');
const argv = process.argv.slice(2);
const opt = (name) => { const i = argv.indexOf(name); return i >= 0 ? argv[i + 1] : undefined; };

const N_SMALL = 20_000;
const N_FULL = Number(opt('--n') ?? 200_000);
const N_MIXED_SMALL = 100_000;
const N_MIXED_FULL = Number(opt('--n') ?? 2_000_000);

const f64a = new Float64Array(1);
const u64a = new BigUint64Array(f64a.buffer);
const canon = (v) => (v !== v ? NaN : v);
const hex = (v) => { f64a[0] = canon(v); return u64a[0].toString(16).padStart(16, '0'); };
const fnv = (s) => {
  let h = 0x811c9dc5;
  for (const b of Buffer.from(s)) { h ^= b; h = Math.imul(h, 0x01000193) >>> 0; }
  return h >>> 0;
};

// ── the generator (mirrored in tests/adv.rs) ────────────────────────────────────────────
const LENS = [0, 1, 1, 2, 3, 5, 8, 15, 16, 17, 18, 19, 22, 40, 120, 330];
const SIGN = ['', '', '+', '-'];
const EXPLENS = [1, 1, 2, 2, 3, 3, 4, 5, 10, 20];
const WS_ALL = ['\t', '\n', '\u000b', '\u000c', '\r', ' ', '\u00a0', '\u1680', '\u2000', '\u2001', '\u2002', '\u2003', '\u2004', '\u2005', '\u2006', '\u2007', '\u2008', '\u2009', '\u200a', '\u2028', '\u2029', '\u202f', '\u205f', '\u3000', '\ufeff'];
const WS_NEG = ['\u0085', '\u180e', '\u200b', '\u2060', '\u00ad', '\u0000'];
const pick = (r, a) => a[r.next() % a.length];
const lenpick = (r) => LENS[r.next() % 16];
const digitsRand = (r, n) => { let s = ''; for (let i = 0; i < n; i++) s += String(r.next() % 10); return s; };
const zeroish = (r, n) => {
  let s = '';
  for (let i = 0; i < n; i++) {
    if (r.next() % 16 === 0) s += String(r.next() % 10); else s += '0';
  }
  return s;
};
const nd = (r, max) => { const n = 1 + (r.next() % max); return digitsRand(r, n); };

function pad(r) {
  if (r.next() % 2 === 0) return '';
  const n = r.next() % 4;
  let s = '';
  for (let i = 0; i < n; i++) {
    if (r.next() % 16 === 0) s += pick(r, WS_NEG); else s += pick(r, WS_ALL);
  }
  return s;
}

function decimal(r, zeroHeavy) {
  const gen = zeroHeavy ? zeroish : digitsRand;
  let s = pick(r, SIGN);
  let n = lenpick(r);
  s += gen(r, n);
  if (r.next() % 2 === 1) {
    s += '.';
    n = lenpick(r);
    s += gen(r, n);
  }
  if (r.next() % 3 === 0) {
    s += pick(r, ['e', 'E']);
    s += pick(r, ['', '', '+', '-']);
    n = pick(r, EXPLENS);
    s += digitsRand(r, n);
  }
  return s;
}

const BOUND = [
  ['17976931348623157', 292], ['17976931348623158', 292], ['17976931348623159', 292], ['22250738585072014', -324],
  ['22250738585072011', -324], ['49406564584124654', -340], ['24703282292062328', -340], ['24703282292062327', -340],
  ['24703282292062329', -340], ['9007199254740993', 0], ['9007199254740995', 0], ['9007199254740992', 0],
  ['18014398509481985', 0], ['9', 23], ['1', 22], ['1', 23], ['5', -324], ['1', -323], ['3', -324], ['2', -324],
];
function boundary(r) {
  const [mant, e] = pick(r, BOUND);
  let m = mant;
  let exp = e;
  if (r.next() % 3 !== 0) {
    const k = lenpick(r);
    m += '0'.repeat(k);
    exp -= k;
    if (r.next() % 2 === 1) { m += '1'; exp -= 1; }
  }
  let s = pick(r, SIGN) + m;
  s += pick(r, ['e', 'E']);
  if (exp >= 0 && r.next() % 2 === 1) s += '+';
  s += String(exp);
  return s;
}

const PREFIX = ['0x', '0X', '0o', '0O', '0b', '0B'];
const RADIX_OF = [16, 16, 8, 8, 2, 2];
function prefixed(r) {
  const i = r.next() % 6;
  const radix = RADIX_OF[i];
  const upper = r.next() % 4 === 0;
  const ch = (d) => { const c = '0123456789abcdef'[d]; return upper ? c.toUpperCase() : c; };
  let s = '';
  if (r.next() % 8 === 0) s += pick(r, ['+', '-']);
  s += PREFIX[i];
  const mode = r.next() % 3;
  if (mode === 0) {
    const n = lenpick(r);
    for (let j = 0; j < n; j++) s += ch(r.next() % radix);
  } else if (mode === 1) {
    s += ch(1 + (r.next() % (radix - 1)));
    let n = lenpick(r);
    s += '0'.repeat(n);
    s += ch(r.next() % radix);
    n = lenpick(r);
    s += '0'.repeat(n);
    if (r.next() % 2 === 1) s += '1';
  } else {
    const n = lenpick(r);
    s += ch(radix - 1).repeat(n);
  }
  if (r.next() % 8 === 0) {
    s += pick(r, ['g', '_', ' ', '.', '8', '9', '\u00a0', 'x', 'z']);
    if (r.next() % 2 === 1) {
      const n = lenpick(r);
      for (let j = 0; j < n; j++) s += ch(r.next() % radix);
    }
  }
  return s;
}

const INF = ['Infinity', 'Infinity', 'Infinity', 'infinity', 'INFINITY', 'Infinit', 'Infinityx', 'Infinity ', 'Infinity.0', 'Infinity1', 'Infinity\u00a0', 'Inf', 'InfinityInfinity'];
function infinity(r) {
  let s = pick(r, SIGN);
  s += pick(r, INF);
  return s;
}

const TOKS = ['0', '1', '5', '9', 'a', 'f', 'z', 'x', 'b', 'o', 'e', 'E', '+', '-', '.', ' ', '\t', '\u00a0', '\ufeff', '\u0085', '\u180e', '_', 'Infinity', '0x', '0b', '0o', '\u2003', '\u0661', '\uff11', '1e5', '.5', '5.', 'e+', 'e-', 'NaN', '\u200b'];
function soup(r) {
  const n = r.next() % 14;
  let s = '';
  for (let i = 0; i < n; i++) s += pick(r, TOKS);
  return s;
}

function exponent(r) {
  let s = pick(r, SIGN);
  let n = 1 + (r.next() % 20);
  s += digitsRand(r, n);
  if (r.next() % 2 === 1) {
    n = 1 + (r.next() % 20);
    s += '.';
    s += digitsRand(r, n);
  }
  const kind = r.next() % 4;
  if (kind === 3) {
    s += pick(r, ['e', 'E']);
    s += pick(r, ['', '+', '-']);
    n = 6 + (r.next() % 20);
    s += digitsRand(r, n);
  } else {
    let e;
    if (kind === 0) e = 300 + (r.next() % 15);
    else if (kind === 1) e = -(300 + (r.next() % 35));
    else e = (r.next() % 2000) - 1000;
    s += 'e';
    s += String(e);
  }
  return s;
}

function subnormal(r) {
  let s = pick(r, SIGN);
  const kind = r.next() % 3;
  if (kind === 0) {
    const z = 300 + (r.next() % 30);
    s += '0.';
    s += '0'.repeat(z);
    s += nd(r, 20);
  } else if (kind === 1) {
    s += nd(r, 20);
    s += 'e-';
    s += String(300 + (r.next() % 30));
  } else {
    s += nd(r, 20);
    s += '.';
    s += nd(r, 20);
    s += 'e-';
    s += String(300 + (r.next() % 30));
  }
  return s;
}

function dots(r) {
  let s = pick(r, SIGN);
  const k = r.next() % 8;
  if (k === 0) { s += '.'; s += nd(r, 5); }
  else if (k === 1) { s += nd(r, 5); s += '.'; }
  else if (k === 2) { s += '.'; }
  else if (k === 3) { s += '.e'; s += nd(r, 3); }
  else if (k === 4) { s += nd(r, 3); s += '.e'; s += nd(r, 3); }
  else if (k === 5) { s += '.'; s += nd(r, 3); s += 'e'; s += pick(r, ['', '+', '-']); s += nd(r, 3); }
  else if (k === 6) { s += nd(r, 3); s += '.'; s += nd(r, 3); s += '.'; s += nd(r, 3); }
  else { s += nd(r, 3); s += pick(r, ['e', 'e+', 'E-']); }
  return s;
}

const TOTAL = [305, 308, 309, 309, 310, 310, 311, 312, 320, 400, 800];
const MANT9 = ['17976931348623157', '17976931348623158', '17976931348623159', '18', '1', '9', '99', '12345678901234567', '17976931348623156'];
const TAIL9 = ['', '', '.5', 'e5', 'e-5', 'x', ' 7', '.', 'e+400', '_1'];
function longInteger(r) {
  const mant = pick(r, MANT9);
  const total = pick(r, TOTAL);
  let s = mant;
  if (r.next() % 3 === 0) s += '9'.repeat(total - mant.length);
  else s += zeroish(r, total - mant.length);
  if (r.next() % 4 === 0) {
    const n = lenpick(r);
    s = '0'.repeat(n) + s;
  }
  s += pick(r, TAIL9);
  return pick(r, SIGN) + s;
}

const SEPS = [' ', '\u00a0', '\t', '_', ',', '\'', ' e ', '-', '+', '\u2028', '\u0085', '\ufeff'];
function separated(r) {
  let s = nd(r, 5);
  s += pick(r, SEPS);
  s += nd(r, 5);
  if (r.next() % 2 === 1) s += pick(r, SEPS);
  return s;
}

const UDIGITS = ['\u0661', '\uff11', '\u0663', '\u06f4', '\u0966'];
function unicodeDigits(r) {
  const n = 1 + (r.next() % 8);
  let s = '';
  for (let i = 0; i < n; i++) {
    if (r.next() % 2 === 0) s += digitsRand(r, 1); else s += pick(r, UDIGITS);
  }
  return s;
}

const CATS = [
  (r) => decimal(r, false), (r) => decimal(r, true), boundary, prefixed, infinity, soup,
  exponent, subnormal, dots, longInteger, separated, unicodeDigits,
];
const NCATS = CATS.length;
const RADICES = [0, 10, 16, 2, 8, 36, 7, 32, 3];

function sample(r, fixedCat) {
  const cat = fixedCat ?? r.next() % NCATS;
  const lead = pad(r);
  const core = CATS[cat](r);
  const trail = pad(r);
  return lead + core + trail;
}

function run(key, fixedCat, nSmall, nFull) {
  const rng = new Rng(fnv(key));
  const s = new Stream();
  let small = null;
  for (let i = 0; i < nFull; i++) {
    const str = sample(rng, fixedCat);
    s.f64(canon(Number(str)));
    for (const radix of RADICES) s.f64(canon(parseInt(str, radix)));
    if (i + 1 === nSmall) small = s.digest().short;
  }
  return { seed: fnv(key), nSmall, small, nFull, full: s.finish().short };
}

// ── the hard corpus: exact midpoints between neighbouring doubles ───────────────────────────────
const f64b = new Float64Array(1);
const u64b = new BigUint64Array(f64b.buffer);
const fromBits = (b) => { u64b[0] = b; return f64b[0]; };
function midpointStrings(bits) {
  const exp = Number((bits >> 52n) & 0x7ffn);
  const frac = bits & 0xfffffffffffffn;
  const mant = exp === 0 ? frac : frac | (1n << 52n);
  const e = (exp === 0 ? 1 : exp) - 1075;
  const num = 2n * mant + 1n;
  const shift = e - 1;
  let digits;
  let places;
  if (shift >= 0) { digits = (num << BigInt(shift)).toString(); places = 0; }
  else { places = -shift; digits = (num * 5n ** BigInt(places)).toString(); }
  const place = (d, p) => {
    if (p === 0) return d;
    const padded = d.padStart(p + 1, '0');
    return `${padded.slice(0, padded.length - p)}.${padded.slice(padded.length - p)}`;
  };
  const exact = BigInt(digits);
  const out = [place(digits, places)];
  for (const j of [7]) {
    out.push(place((exact * 10n ** BigInt(j) + 1n).toString(), places + j));
    out.push(place((exact * 10n ** BigInt(j) - 1n).toString(), places + j));
  }
  return out;
}

function hardInputs() {
  const rng = new Rng(fnv('hard'));
  const bits = [];
  const draw = (lo, hi) => lo + (BigInt(rng.next()) << 32n | BigInt(rng.next())) % (hi - lo);
  for (let i = 0; i < 60; i++) bits.push(draw(0n, 0x10_0000_0000_0000n)); // subnormals
  for (let i = 0; i < 40; i++) bits.push(draw(0x0010_0000_0000_0000n, 0x0100_0000_0000_0000n)); // tiny normals
  for (let i = 0; i < 60; i++) bits.push(draw(0x3fe0_0000_0000_0000n, 0x4040_0000_0000_0000n)); // 0.5 .. 32
  for (let i = 0; i < 40; i++) bits.push(draw(0x4330_0000_0000_0000n, 0x4350_0000_0000_0000n)); // 2^52 .. 2^54
  for (let i = 0; i < 120; i++) bits.push(draw(0x4350_0000_0000_0000n, 0x7fef_ffff_ffff_ffffn)); // huge, up to the last double below MAX
  bits.push(0x7fef_ffff_ffff_fffen, 0x0000_0000_0000_0000n, 0x0000_0000_0000_0001n, 0x000f_ffff_ffff_ffffn, 0x0010_0000_0000_0000n, 0x7fef_ffff_ffff_ffffn);
  bits.push(0x4340_0000_0000_0000n, 0x433f_ffff_ffff_ffffn, 0x3ff0_0000_0000_0000n, 0x3fef_ffff_ffff_ffffn);
  const strings = [];
  for (const b of bits) {
    for (const s of midpointStrings(b)) strings.push(s);
  }
  return strings;
}

// ── the math functions on the arguments where the algorithms branch ─────────────────────────────
// The random distributions of capture-jsnum.mjs leave the rare branches of the argument reduction
// (the second and third pass of rem_pio2 need |x - n*pi/2| ~ 2^-16 and 2^-49 of x), the threshold
// words of every function (value and neighbour, random low word), the multiples of ln2 and the
// neighbourhood of 1 nearly unvisited; these distributions aim at them (mirrored in tests/adv.rs).
const THRESHOLDS = readFileSync(join(crate, 'tests', 'captures', 'inputs.txt'), 'utf8').split('\n')
  .filter((l) => l.startsWith('threshold ')).map((l) => parseInt(l.slice(10), 16));
const f64x = new Float64Array(1);
const u32x = new Uint32Array(f64x.buffer);
const u64x = new BigUint64Array(f64x.buffer);
const fromWords = (hi, lo) => { u32x[1] = hi >>> 0; u32x[0] = lo >>> 0; return f64x[0]; };
const nudge = (x, j) => { f64x[0] = x; u64x[0] = u64x[0] + BigInt(j); return f64x[0]; };
const pow2 = (e) => fromWords((1023 + e) << 20, 0);
const jitter = (r) => (r.next() % 7) - 3;
const TWO53 = 9007199254740992;
const urange = (lo, hi) => (r) => {
  const a = r.next();
  const b = r.next();
  const u = ((a >>> 5) * 67108864 + (b >>> 6)) / TWO53;
  return lo + (hi - lo) * u;
};
const PIO2 = 1.5707963267948966;
const signed = (g) => (r) => { const x = g(r); return (r.next() & 1) === 1 ? -x : x; };
const EG = {
  pio2k20: (r) => { const k = 1 + (r.next() % 1048576); const j = jitter(r); return nudge(k * PIO2, j); },
  pio2kbig: (r) => { const k = Math.floor(urange(1, 1152921504606846976)(r)); const j = jitter(r); return nudge(k * PIO2, j); },
  ln2k: (r) => { const k = Math.floor(urange(-1075, 1025)(r)); const j = jitter(r); return nudge(k * Math.LN2, j); },
  ln2half: (r) => { const k = Math.floor(urange(-1075, 1025)(r)); const j = jitter(r); return nudge((k + 0.5) * Math.LN2, j); },
  thr: (r) => {
    const hi = THRESHOLDS[r.next() % THRESHOLDS.length];
    const d = r.next() % 2;
    const lo = r.next();
    const neg = r.next() & 1;
    return fromWords(((hi - d) | (neg ? 0x80000000 : 0)) >>> 0, lo);
  },
  near1: (r) => { const k = 1 + (r.next() % 52); const u = urange(-1, 1)(r); return 1 + u * pow2(-k); },
  nearm1: (r) => { const k = 1 + (r.next() % 52); const u = urange(0, 1)(r); return -1 + pow2(-k) * (1 + u); },
  nearsqrt2: (r) => {
    const e = (r.next() % 2001) - 1000;
    const half = r.next() % 2 === 1;
    const j = jitter(r);
    return nudge(half ? Math.SQRT1_2 : Math.SQRT2, j) * pow2(e);
  },
  pow2nudge: (r) => { const e = (r.next() % 2001) - 1000; const j = jitter(r); return nudge(pow2(e), j); },
  tinyk: (r) => { const k = 1 + (r.next() % 60); const u = urange(-1, 1)(r); return u * pow2(-k); },
  scaled: (r) => { const e = (r.next() % 2041) - 1020; const u = urange(1, 2)(r); return u * pow2(e); },
  cubes: (r) => { const n = 1 + (r.next() % 131072); return n * n * n; },
  exphi: urange(709.7, 709.8),
  explo: urange(-745.2, -745.1),
};
EG.pio2k20s = signed(EG.pio2k20);
EG.pio2kbigs = signed(EG.pio2kbig);
EG.cubess = signed(EG.cubes);
EG.scaleds = signed(EG.scaled);
const EXT_OPS = {
  sin: (a) => Math.sin(a[0]), cos: (a) => Math.cos(a[0]), tan: (a) => Math.tan(a[0]),
  asin: (a) => Math.asin(a[0]), acos: (a) => Math.acos(a[0]), atan: (a) => Math.atan(a[0]), atan2: (a) => Math.atan2(a[0], a[1]),
  exp: (a) => Math.exp(a[0]), expm1: (a) => Math.expm1(a[0]),
  log: (a) => Math.log(a[0]), log1p: (a) => Math.log1p(a[0]), log2: (a) => Math.log2(a[0]), log10: (a) => Math.log10(a[0]),
  sinh: (a) => Math.sinh(a[0]), cosh: (a) => Math.cosh(a[0]), tanh: (a) => Math.tanh(a[0]), atanh: (a) => Math.atanh(a[0]),
  cbrt: (a) => Math.cbrt(a[0]),
  hypot: (a) => Math.hypot(a[0], a[1]), hypot3: (a) => Math.hypot(a[0], a[1], a[2]), hypot_n4: (a) => Math.hypot(...a),
};
const FD_UNARY = ['sin', 'cos', 'tan', 'asin', 'acos', 'atan', 'exp', 'expm1', 'log', 'log1p', 'log2', 'log10', 'sinh', 'cosh', 'tanh', 'atanh', 'cbrt'];
const EXT = [];
const ext = (op, dists) => { for (const d of dists) EXT.push({ key: `ext/${op}/${d.join('+')}`, op, gens: d }); };
for (const op of ['sin', 'cos', 'tan']) ext(op, [['pio2k20s'], ['pio2kbigs']]);
for (const op of FD_UNARY) ext(op, [['thr']]);
for (const op of ['exp', 'expm1', 'sinh', 'cosh', 'tanh']) ext(op, [['ln2k'], ['ln2half']]);
ext('exp', [['exphi'], ['explo']]);
ext('expm1', [['exphi']]);
for (const op of ['log', 'log2', 'log10']) ext(op, [['near1'], ['nearsqrt2'], ['pow2nudge']]);
ext('log1p', [['tinyk'], ['nearm1'], ['near1']]);
for (const op of ['asin', 'acos']) ext(op, [['near1']]);
ext('atanh', [['near1'], ['tinyk']]);
ext('atan', [['scaleds']]);
ext('atan2', [['thr', 'thr'], ['scaleds', 'scaleds']]);
ext('hypot', [['scaleds', 'scaleds']]);
ext('hypot3', [['scaleds', 'scaleds', 'scaleds']]);
ext('hypot_n4', [['scaleds', 'scaleds', 'scaleds', 'scaleds']]);
ext('cbrt', [['cubess'], ['scaleds']]);

function runExt(rec, nSmall, nFull) {
  const rng = new Rng(fnv(rec.key));
  const s = new Stream();
  const op = EXT_OPS[rec.op];
  const gens = rec.gens.map((g) => EG[g]);
  const args = new Array(gens.length);
  let small = null;
  for (let i = 0; i < nFull; i++) {
    for (let g = 0; g < gens.length; g++) args[g] = gens[g](rng);
    s.f64(canon(op(args)));
    if (i + 1 === nSmall) small = s.digest().short;
  }
  return { seed: fnv(rec.key), nSmall, small, nFull, full: s.finish().short };
}

// ── typed-array element access by a number (js_index) ──────────────────────────────────────────
const SPECIAL_VALUES = readFileSync(join(crate, 'tests', 'captures', 'inputs.txt'), 'utf8').split('\n')
  .filter((l) => l.startsWith('special ')).map((l) => { u64x[0] = BigInt(`0x${l.slice(8)}`); return f64x[0]; });
const INDEX_EXTRAS = [-0, 0, 0.5, 1.5, 2.9999999999999996, 3, 4294967295, 4294967296, 9007199254740992, 9007199254740994, 1e21, 1e300, 5e-324, -5e-324, -1, -2, 1 / 3, 19, 20, 21, 19.5, 20.5];
function jsIndexSweep() {
  const s = new Stream();
  let n = 0;
  for (const len of [0, 1, 2, 3, 20]) {
    for (const i of [...SPECIAL_VALUES, ...INDEX_EXTRAS]) {
      const ta = new Float64Array(len);
      for (let k = 0; k < len; k++) ta[k] = k + 1;
      const v = ta[i];
      s.f64(v === undefined ? -1 : v);
      ta[i] = 99;
      let sum = 0;
      for (let k = 0; k < len; k++) sum += ta[k];
      s.f64(sum);
      n++;
    }
  }
  return { n, digest: s.finish().short };
}

// ── the integer conversions over every exponent ────────────────────────────────────────────────
const ta = { i32: new Int32Array(1), u32: new Uint32Array(1), i16: new Int16Array(1), u16: new Uint16Array(1), i8: new Int8Array(1), u8: new Uint8Array(1), u8c: new Uint8ClampedArray(1) };
const SWEEP_MANTS = [0n, 1n, 2n, 0x7ffffffffffffn, 0x8000000000000n, 0x8000000000001n, 0xfffffffffffffn, 0xffffffffffffen, 0x0000100000000n, 0x00000ffffffffn, 0x0000200000001n];
function convSweep() {
  const rng = new Rng(fnv('conv/exp_sweep'));
  const mants = [...SWEEP_MANTS];
  for (let i = 0; i < 8; i++) { const hi = BigInt(rng.next() & 0xfffff); mants.push((hi << 32n) | BigInt(rng.next())); }
  const s = new Stream();
  let n = 0;
  for (let e = 0; e < 2048; e++) {
    for (const m of mants) {
      for (const sign of [0n, 1n]) {
        const x = fromBits((sign << 63n) | (BigInt(e) << 52n) | m);
        for (const t of Object.values(ta)) { t[0] = x; s.f64(t[0]); }
        n++;
      }
    }
  }
  return { n, digest: s.finish().short };
}

// ── write ───────────────────────────────────────────────────────────────────────────────────────
const w = new CaptureWriter([
  'uo-jsnum adversarial string parity capture: written by core/crates/uo-jsnum/js/capture-jsnum-adv.mjs, replayed by tests/adv.rs.',
  'adv/<cat|mixed>: seed = FNV-1a of the key (xorshift32), small / full = digests of the first n_small / n_full samples; per sample the results of Number(s) and parseInt(s, r) for r in 0, 10, 16, 2, 8, 36, 7, 32, 3 (f64 bits, NaN canonicalized).',
  'ext/<op>/<gens>: math functions over the targeted distributions of the generator (near multiples of pi/2, the threshold words, multiples of ln2, near 1, scaled operands); seed, n_small, small, n_full, full as adv/.',
  'js_index/sweep: digest of ta[i] (-1 for undefined) and of the sum of the array after ta[i] = 99, for lengths 0, 1, 2, 3, 20 and the special values of inputs.txt plus -0, fractions, 2^32, 2^53, 1e21 and the lengths around 20; n = the cases.',
  'conv/exp_sweep: digest of ToInt32, ToUint32, ToInt16, ToUint16, ToInt8, ToUint8 and ToUint8Clamp (typed-array stores) of every exponent (0..2047) x 19 mantissas x both signs; n = the values.',
  'hard/<i>: s = the input (JSON), num = Number(s), pi_10 = parseInt(s, 10); the exact decimal midpoint of two neighbouring doubles and a hair above and below it.',
]);
w.meta('generator', 'core/crates/uo-jsnum/js/capture-jsnum-adv.mjs');
let total = 0;
for (let c = 0; c < NCATS; c++) {
  const key = `adv/cat${c}`;
  const r = run(key, c, N_SMALL, N_FULL);
  w.set(key, 'seed', r.seed.toString(16)); w.set(key, 'n_small', r.nSmall); w.set(key, 'small', r.small); w.set(key, 'n_full', r.nFull); w.set(key, 'full', r.full);
  total += r.nFull;
}
{
  const key = 'adv/mixed';
  const r = run(key, undefined, N_MIXED_SMALL, N_MIXED_FULL);
  w.set(key, 'seed', r.seed.toString(16)); w.set(key, 'n_small', r.nSmall); w.set(key, 'small', r.small); w.set(key, 'n_full', r.nFull); w.set(key, 'full', r.full);
  total += r.nFull;
}
const hard = hardInputs();
hard.forEach((str, i) => {
  const key = `hard/${String(i).padStart(4, '0')}`;
  w.set(key, 's', JSON.stringify(str));
  w.set(key, 'num', hex(Number(str)));
  w.set(key, 'pi_10', hex(parseInt(str, 10)));
});
const N_EXT_FULL = Number(opt('--n') ?? 2_000_000);
for (const rec of EXT) {
  const r = runExt(rec, 100_000, N_EXT_FULL);
  w.set(rec.key, 'seed', r.seed.toString(16)); w.set(rec.key, 'n_small', r.nSmall); w.set(rec.key, 'small', r.small); w.set(rec.key, 'n_full', r.nFull); w.set(rec.key, 'full', r.full);
  total += r.nFull;
}
{
  const r = jsIndexSweep();
  w.set('js_index/sweep', 'n', r.n);
  w.set('js_index/sweep', 'digest', r.digest);
}
{
  const r = convSweep();
  w.set('conv/exp_sweep', 'n', r.n);
  w.set('conv/exp_sweep', 'digest', r.digest);
}
const out = opt('--out') ?? join(crate, 'tests', 'captures', 'adv.capture');
mkdirSync(dirname(out), { recursive: true });
w.write(out);
console.log(`wrote ${out}: ${total} generated strings (each: Number + parseInt in ${RADICES.length} radices), ${hard.length} hard strings`);
