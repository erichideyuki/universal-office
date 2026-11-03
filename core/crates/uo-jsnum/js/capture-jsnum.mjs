// Parity capture of uo-jsnum (increment A, everything but pow, which has its own generator,
// capture-pow.mjs): record core/docs/modules/uo-jsnum.md, "Parity plan". The reference is V8
// itself, not the TS of an app: Node 24.15.0 (V8 13.6.233.17) x64. Its fdlibm `Math` functions
// are the same bits on every x64 OS, so this capture may be taken on Windows or Linux
// (`@platform` records where; `tests/differential.rs` requires V8 13.6.233.17 and x64).
//
//   node core/crates/uo-jsnum/js/capture-jsnum.mjs [--out <file>] [--n <full count>]
//
// writes core/crates/uo-jsnum/tests/captures/jsnum.capture, replayed by tests/differential.rs
// (digests), tests/special.rs (special values) and tests/strings.rs (strings). Options:
//
//   --out <file>     write elsewhere
//   --n <count>      full sample count of the 10^7 records (quick runs only; never committed)
//   --digests-only   print `key<TAB>digest at 10^5` lines and exit (the child of the tier check)
//   UO_JSNUM_DUMP=<key>   also write the raw samples (inputs then output, f64 LE) of that numeric
//                         record to core/target/jsnum-dump/<key with / as _>.f64 (UO_JSNUM_DUMP_N
//                         samples, default 10^5); tests/differential.rs prints the first 20
//                         differing inputs from it
//
// Inputs come from uo-testkit's Rng (xorshift32, one seed per record: FNV-1a of the key) with the
// same IEEE operations as the Rust side (tests/differential.rs mirrors the tables below);
// outputs are hashed with uo-testkit's Stream after NaN canonicalization. Everything runs twice,
// in the default tiers and with `--no-opt --no-maglev` (a child process), and the 10^5 digests
// must agree (`@tiers-agree`). The data of tests/captures/inputs.txt is shared with the Rust side.
import { spawnSync } from 'node:child_process';
import { mkdirSync, readFileSync, writeFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { CaptureWriter, Rng, Stream } from '../../uo-testkit/js/testkit.mjs';

const here = dirname(fileURLToPath(import.meta.url));
const crate = join(here, '..');
const argv = process.argv.slice(2);
const opt = (name) => { const i = argv.indexOf(name); return i >= 0 ? argv[i + 1] : undefined; };
const flag = (name) => argv.includes(name);

const SMALL = 100_000;
const FULL = Number(opt('--n') ?? 10_000_000);
const FULL_STRINGS = 1_000_000;
const SMALL_STRINGS = 100_000;
const FULL_TEXT = 100_000;
const SMALL_TEXT = 10_000;

const f64a = new Float64Array(1);
const u32a = new Uint32Array(f64a.buffer);
const u64a = new BigUint64Array(f64a.buffer);
const fromWords = (hi, lo) => { u32a[1] = hi; u32a[0] = lo; return f64a[0]; };
const fromHex = (h) => { u64a[0] = BigInt(`0x${h}`); return f64a[0]; };
const canon = (v) => (v !== v ? NaN : v);
const hex = (v) => { f64a[0] = canon(v); return u64a[0].toString(16).padStart(16, '0'); };
if (hex(NaN) !== '7ff8000000000000') throw new Error('the JS NaN is not the canonical quiet NaN');

const fnv = (s) => {
  let h = 0x811c9dc5;
  for (const b of Buffer.from(s)) { h ^= b; h = Math.imul(h, 0x01000193) >>> 0; }
  return h >>> 0;
};

// ── shared input data ───────────────────────────────────────────────────────────────────────────
const inputLines = readFileSync(join(crate, 'tests', 'captures', 'inputs.txt'), 'utf8').split('\n');
const SPECIALS = inputLines.filter((l) => l.startsWith('special ')).map((l) => fromHex(l.slice(8)));
const THRESHOLDS = inputLines.filter((l) => l.startsWith('threshold ')).map((l) => parseInt(l.slice(10), 16));

// ── generators (mirrored in tests/differential.rs `Gen`) ────────────────────────────────────────
const TWO53 = 9007199254740992;
const bits = (r) => { const a = r.next(); const b = r.next(); return fromWords(a, b); };
const range = (lo, hi) => (r) => {
  const a = r.next();
  const b = r.next();
  const u = ((a >>> 5) * 67108864 + (b >>> 6)) / TWO53;
  return lo + (hi - lo) * u;
};
const G = {
  bits,
  absbits: (r) => Math.abs(bits(r)),
  range,
  floorHalf: (lo, hi) => { const g = range(lo, hi); return (r) => Math.floor(g(r)) + 0.5; },
  floorRange: (lo, hi) => { const g = range(lo, hi); return (r) => Math.floor(g(r)); },
  mixed: (r) => { const a = r.next(); return a % 4 === 0 ? bits(r) : SPECIALS[(a >>> 2) % SPECIALS.length]; },
  below: (n) => (r) => r.next() % n,
};

// ── operations (mirrored in tests/differential.rs `ops`) ────────────────────────────────────────
const i32 = new Int32Array(1);
const u32 = new Uint32Array(1);
const i16 = new Int16Array(1);
const u16 = new Uint16Array(1);
const i8 = new Int8Array(1);
const u8 = new Uint8Array(1);
const u8c = new Uint8ClampedArray(1);
const store = (ta) => (a) => { ta[0] = a[0]; return ta[0]; };
const OPS = {
  sin: (a) => Math.sin(a[0]), cos: (a) => Math.cos(a[0]), tan: (a) => Math.tan(a[0]),
  asin: (a) => Math.asin(a[0]), acos: (a) => Math.acos(a[0]), atan: (a) => Math.atan(a[0]),
  atan2: (a) => Math.atan2(a[0], a[1]),
  exp: (a) => Math.exp(a[0]), expm1: (a) => Math.expm1(a[0]),
  log: (a) => Math.log(a[0]), log1p: (a) => Math.log1p(a[0]), log2: (a) => Math.log2(a[0]), log10: (a) => Math.log10(a[0]),
  sinh: (a) => Math.sinh(a[0]), cosh: (a) => Math.cosh(a[0]), tanh: (a) => Math.tanh(a[0]), atanh: (a) => Math.atanh(a[0]),
  cbrt: (a) => Math.cbrt(a[0]),
  hypot: (a) => Math.hypot(a[0], a[1]),
  hypot3: (a) => Math.hypot(a[0], a[1], a[2]),
  hypot_n4: (a) => Math.hypot(...a),
  round: (a) => Math.round(a[0]), sign: (a) => Math.sign(a[0]), trunc: (a) => Math.trunc(a[0]),
  floor: (a) => Math.floor(a[0]), ceil: (a) => Math.ceil(a[0]), abs: (a) => Math.abs(a[0]), sqrt: (a) => Math.sqrt(a[0]),
  max: (a) => Math.max(a[0], a[1]), min: (a) => Math.min(a[0], a[1]),
  max_n: (a) => Math.max(...a.slice(1, 1 + a[0])), min_n: (a) => Math.min(...a.slice(1, 1 + a[0])),
  imul: (a) => Math.imul(a[0], a[1]), clz32: (a) => Math.clz32(a[0]), fround: (a) => Math.fround(a[0]),
  rem: (a) => a[0] % a[1],
  to_int32: store(i32), to_uint32: store(u32), to_int16: store(i16), to_uint16: store(u16),
  to_int8: store(i8), to_uint8: store(u8), to_uint8_clamp: store(u8c),
  js_index: (a) => { // a = [len, i, floor flag]: Float64Array(len) holding its own indices
    const ta = new Float64Array(a[0]);
    for (let k = 0; k < a[0]; k++) ta[k] = k;
    const v = ta[a[2] === 1 ? Math.floor(a[1]) : a[1]];
    return v === undefined ? -1 : v;
  },
  truthy: (a) => (a[0] ? 1 : 0),
  is_integer: (a) => (Number.isInteger(a[0]) ? 1 : 0),
  is_safe_integer: (a) => (Number.isSafeInteger(a[0]) ? 1 : 0),
};

// ── the numeric records: op, distribution, one generator per argument ──────────────────────────
const NUMERIC = [];
const num = (op, dist, gens, full = FULL) => NUMERIC.push({ key: `${op}/${dist}`, op, gens, full });
const PI = Math.PI;
for (const op of ['sin', 'cos', 'tan']) {
  num(op, 'pi8', [G.range(-8 * PI, 8 * PI)]);
  num(op, 'big20', [G.range(-(2 ** 20), 2 ** 20)]);
  num(op, 'bits', [G.bits]);
}
for (const op of ['asin', 'acos', 'atanh']) {
  num(op, 'unit', [G.range(-1, 1)]);
  num(op, 'bits', [G.bits]);
}
num('atan', 'r200', [G.range(-200, 200)]);
num('atan', 'bits', [G.bits]);
num('atan2', 'r2e4', [G.range(-2e4, 2e4), G.range(-2e4, 2e4)]);
num('atan2', 'bits2', [G.bits, G.bits]);
num('exp', 'r745', [G.range(-745, 710)]);
num('exp', 'bits', [G.bits]);
num('expm1', 'r40', [G.range(-40, 710)]);
num('expm1', 'bits', [G.bits]);
for (const op of ['log', 'log2', 'log10']) {
  num(op, 'r1000', [G.range(0, 1000)]);
  num(op, 'absbits', [G.absbits]);
}
num('log1p', 'r1000', [G.range(-1, 1000)]);
num('log1p', 'bits', [G.bits]);
for (const op of ['sinh', 'cosh']) {
  num(op, 'r720', [G.range(-720, 720)]);
  num(op, 'bits', [G.bits]);
}
num('tanh', 'r25', [G.range(-25, 25)]);
num('tanh', 'bits', [G.bits]);
num('cbrt', 'bits', [G.bits]);
num('cbrt', 'r1000', [G.range(-1000, 1000)]);
num('hypot', 'r1e4', [G.range(-1e4, 1e4), G.range(-1e4, 1e4)]);
num('hypot', 'bits2', [G.bits, G.bits]);
num('hypot3', 'r1e4', [G.range(-1e4, 1e4), G.range(-1e4, 1e4), G.range(-1e4, 1e4)]);
num('hypot3', 'bits3', [G.bits, G.bits, G.bits]);
num('hypot_n4', 'r1e4', [G.range(-1e4, 1e4), G.range(-1e4, 1e4), G.range(-1e4, 1e4), G.range(-1e4, 1e4)]);
num('hypot_n4', 'bits4', [G.bits, G.bits, G.bits, G.bits]);
for (const op of ['round', 'sign', 'trunc', 'floor', 'ceil', 'abs', 'sqrt']) {
  num(op, 'bits', [G.bits]);
  num(op, 'r10', [G.range(-10, 10)]);
  num(op, 'half', [G.floorHalf(-TWO53, TWO53)]);
}
num('max', 'mixed2', [G.mixed, G.mixed]);
num('min', 'mixed2', [G.mixed, G.mixed]);
num('max_n', 'mixed', [G.below(9), ...Array.from({ length: 8 }, () => G.mixed)]);
num('min_n', 'mixed', [G.below(9), ...Array.from({ length: 8 }, () => G.mixed)]);
num('imul', 'r2p40', [G.range(-(2 ** 40), 2 ** 40), G.range(-(2 ** 40), 2 ** 40)]);
num('imul', 'bits2', [G.bits, G.bits]);
num('clz32', 'bits', [G.bits]);
num('clz32', 'r2p40', [G.range(-(2 ** 40), 2 ** 40)]);
num('fround', 'bits', [G.bits]);
num('fround', 'r1e6', [G.range(-1e6, 1e6)]);
num('rem', 'bits2', [G.bits, G.bits], 1_000_000);
num('rem', 'r1e6', [G.range(-1e6, 1e6), G.range(-1e6, 1e6)], 1_000_000);
for (const op of ['to_int32', 'to_uint32', 'to_int16', 'to_uint16', 'to_int8', 'to_uint8']) {
  num(op, 'bits', [G.bits]);
  num(op, 'r2p40', [G.range(-(2 ** 40), 2 ** 40)]);
}
num('to_uint8_clamp', 'bits', [G.bits]);
num('to_uint8_clamp', 'r300', [G.range(-10, 300)]);
num('to_uint8_clamp', 'half255', [G.floorHalf(-2, 258)]);
num('js_index', 'r22', [(r) => 1 + (r.next() % 20), G.range(-2, 22), G.below(2)]);
num('js_index', 'bits', [(r) => 1 + (r.next() % 20), G.bits, G.below(2)]);
num('truthy', 'mixed', [G.mixed]);
for (const op of ['is_integer', 'is_safe_integer']) {
  num(op, 'bits', [G.bits]);
  num(op, 'half', [G.floorHalf(-TWO53, TWO53)]);
  num(op, 'ints54', [G.floorRange(-(2 ** 54), 2 ** 54)]);
  num(op, 'mixed', [G.mixed]);
}

// ── string records ──────────────────────────────────────────────────────────────────────────────
const RADICES = [0, 2, 3, 7, 8, 10, 16, 32, 36, 1, 37, -1, 4294967312, 4294967298];
const TOK_JUNK = ['0', '1', '7', '9', 'a', 'f', 'z', 'x', 'b', 'o', 'e', 'E', '+', '-', '.', ' ', '\t', '\u00a0', '\ufeff', '\u0085', '\u180e', '_', 'Infinity', '0x', '\u2003', '\u0661'];
const TOK_NUM = ['0', '1', '2', '3', '4', '5', '6', '7', '8', '9', '9', '0', '.', 'e', '-', '+', ' ', 'x', 'a', 'f'];
const TOK_TEXT = ['a', 'b', '\u00e9', ' ', '\u00a0', '\ufeff', '\t', '\u{1f600}', '\u{10FFFF}', '\u4e2d', '\u2028', 'x'];
const randString = (r, tokens, maxTokens) => {
  const n = r.next() % maxTokens;
  let s = '';
  for (let i = 0; i < n; i++) s += tokens[r.next() % tokens.length];
  return s;
};
const STRING_RECORDS = [];
const strRec = (key, nSmall, nFull, produce) => STRING_RECORDS.push({ key, nSmall, nFull, produce });
strRec('rand/to_number_junk', SMALL_STRINGS, FULL_STRINGS, (r, s) => s.f64(canon(Number(randString(r, TOK_JUNK, 12)))));
strRec('rand/to_number_digits', SMALL_STRINGS, FULL_STRINGS, (r, s) => s.f64(canon(Number(randString(r, TOK_NUM, 16)))));
for (const radix of [0, 2, 7, 8, 10, 16, 36]) {
  strRec(`rand/parse_int_${radix}_junk`, SMALL_STRINGS, FULL_STRINGS, (r, s) => s.f64(canon(parseInt(randString(r, TOK_JUNK, 12), radix))));
  strRec(`rand/parse_int_${radix}_digits`, SMALL_STRINGS, FULL_STRINGS, (r, s) => s.f64(canon(parseInt(randString(r, TOK_NUM, 16), radix))));
}
strRec('rand/text', SMALL_TEXT, FULL_TEXT, (r, s) => {
  const t = randString(r, TOK_TEXT, 8);
  const len = t.length;
  s.f64(len).str(t.trim()).str(t.trimStart()).str(t.trimEnd());
  const parts = t.split(/\s+/);
  s.u32(parts.length);
  for (const p of parts) s.str(p);
  for (let a = -len - 1; a <= len + 1; a++) for (let b = -len - 1; b <= len + 1; b++) s.str(t.slice(a, b));
});

// ── running a record ────────────────────────────────────────────────────────────────────────────
const dumpKey = process.env.UO_JSNUM_DUMP;
const dumpN = Number(process.env.UO_JSNUM_DUMP_N ?? SMALL);

function runNumeric(rec, withFull) {
  const rng = new Rng(fnv(rec.key));
  const s = new Stream();
  const op = OPS[rec.op];
  const arity = rec.gens.length;
  const args = new Array(arity);
  const total = withFull ? rec.full : SMALL;
  const dump = withFull && dumpKey === rec.key ? new Float64Array(dumpN * (arity + 1)) : null;
  let small = null;
  for (let i = 0; i < total; i++) {
    for (let g = 0; g < arity; g++) args[g] = rec.gens[g](rng);
    const v = canon(op(args));
    s.f64(v);
    if (dump && i < dumpN) { for (let g = 0; g < arity; g++) dump[i * (arity + 1) + g] = args[g]; dump[i * (arity + 1) + arity] = v; }
    if (i + 1 === SMALL) small = s.digest().short;
  }
  if (dump) {
    const dir = join(crate, '..', '..', 'target', 'jsnum-dump');
    mkdirSync(dir, { recursive: true });
    writeFileSync(join(dir, `${rec.key.replace(/\//g, '_')}.f64`), new Uint8Array(dump.buffer, 0, Math.min(dumpN, total) * (arity + 1) * 8));
  }
  return { seed: fnv(rec.key), nSmall: SMALL, small, nFull: total, full: withFull ? s.finish().short : null };
}

function runStrings(rec, withFull) {
  const rng = new Rng(fnv(rec.key));
  const s = new Stream();
  const total = withFull ? rec.nFull : rec.nSmall;
  let small = null;
  for (let i = 0; i < total; i++) {
    rec.produce(rng, s);
    if (i + 1 === rec.nSmall) small = s.digest().short;
  }
  return { seed: fnv(rec.key), nSmall: rec.nSmall, small, nFull: total, full: withFull ? s.finish().short : null };
}

if (flag('--digests-only')) {
  for (const rec of NUMERIC) console.log(`${rec.key}\t${runNumeric(rec, false).small}`);
  for (const rec of STRING_RECORDS) console.log(`${rec.key}\t${runStrings(rec, false).small}`);
  process.exit(0);
}

// ── special values: outputs written in full ─────────────────────────────────────────────────────
const FDLIBM_UNARY = ['sin', 'cos', 'tan', 'asin', 'acos', 'atan', 'exp', 'expm1', 'log', 'log1p', 'log2', 'log10', 'sinh', 'cosh', 'tanh', 'atanh', 'cbrt'];
const OTHER_UNARY = ['round', 'sign', 'trunc', 'floor', 'ceil', 'abs', 'sqrt', 'clz32', 'fround', 'to_int32', 'to_uint32', 'to_int16', 'to_uint16', 'to_int8', 'to_uint8', 'to_uint8_clamp', 'truthy', 'is_integer', 'is_safe_integer'];
const BINARY = ['atan2', 'hypot', 'max', 'min', 'imul', 'rem'];
const thresholdInputs = [];
for (const t of THRESHOLDS) {
  for (const hi of [t - 1, t]) {
    for (const lo of [0, 1, 0xffffffff]) {
      thresholdInputs.push(fromWords(hi >>> 0, lo), fromWords((hi | 0x80000000) >>> 0, lo));
    }
  }
}
const S2 = SPECIALS.slice(0, 40);
const S3 = SPECIALS.slice(0, 12);
const S4 = SPECIALS.slice(0, 6);
const S8 = SPECIALS.slice(0, 8);
const specialRecords = [];
for (const op of FDLIBM_UNARY) specialRecords.push([op, [...SPECIALS, ...thresholdInputs].map((x) => OPS[op]([x]))]);
for (const op of OTHER_UNARY) specialRecords.push([op, SPECIALS.map((x) => OPS[op]([x]))]);
for (const op of BINARY) specialRecords.push([op, S2.flatMap((x) => S2.map((y) => OPS[op]([x, y])))]);
specialRecords.push(['hypot3', S3.flatMap((x) => S3.flatMap((y) => S3.map((z) => OPS.hypot3([x, y, z]))))]);
specialRecords.push(['hypot_n4', S4.flatMap((x) => S4.flatMap((y) => S4.flatMap((z) => S4.map((w) => OPS.hypot_n4([x, y, z, w])))))]);
const lists = [[]];
for (let n = 1; n <= 3; n++) for (const l of [...lists].filter((l) => l.length === n - 1)) for (const x of S8) lists.push([...l, x]);
for (const op of ['max_n', 'min_n']) specialRecords.push([op, lists.map((l) => OPS[op]([l.length, ...l]))]);

// ── the corpus of strings (written in the capture with its results) ─────────────────────────────
const SPACES = '\t\n\v\f\r \u00a0\u1680\u2000\u2001\u2002\u2003\u2004\u2005\u2006\u2007\u2008\u2009\u200a\u2028\u2029\u202f\u205f\u3000\ufeff';
const CORPUS = [
  '', ' ', SPACES, `${SPACES}12${SPACES}`, '\u0085', '\u180e', '\u008512', '12\u0085', '\u180e12', '12\u180e', '\u200b12', '\u00a012\ufeff', ' \t\n12.5\r ',
  '0', '-0', '+0', '00', '0.0', '-0.0', '0e0', '0e999', '1', '12', '123456789012345678901234567890', '0.1', '1.5', '-1.5', '+1.5',
  '0x', '0X', '0x0', '0x1a', '0X1A', '0xFF', '0xff ', ' 0xff', '-0x10', '+0x10', '0xg', '0x1g', '0x 1', '0x1_0',
  '0b', '0B', '0b101', '0B11', '0b2', '0b1 1', '0o', '0O', '0o17', '0O7', '0o8', '0b' + '1'.repeat(80), '0b' + '1'.repeat(53), '0b1' + '0'.repeat(53), '0b1' + '0'.repeat(52) + '1', '0o' + '7'.repeat(40),
  '0x20000000000000', '0x20000000000001', '0x20000000000002', '0x20000000000003', '0x20000000000005', '0x3fffffffffffff', '0x40000000000001', '0x40000000000002', '0x40000000000003', '0x40000000000005',
  '0x1fffffffffffff', '0x1fffffffffffff0', '0x' + 'f'.repeat(255), '0x' + 'f'.repeat(256), '0x' + 'f'.repeat(300), '0x' + '0'.repeat(500) + '1', '0x1' + '0'.repeat(300),
  '0x20000000000001' + '0'.repeat(40), '0x20000000000001' + '0'.repeat(40) + '1', '0x2000000000000180', '0x2000000000000100', '0x20000000000001f',
  'Infinity', '-Infinity', '+Infinity', 'infinity', 'INFINITY', 'Infinityx', 'Infinity ', ' -Infinity ', 'Infinit', '+-Infinity', 'NaN', 'nan', 'inf',
  '1e1000', '-1e1000', '1e-400', '-1e-400', '1e308', '1.7976931348623157e308', '1.7976931348623159e308', '1.797693134862315807e308', '5e-324', '2.4703282292062327e-324', '2.4703282292062328e-324', '4.9e-324', '1e99999999999999999999', '1e-99999999999999999999', '0e99999999999999999999',
  '.5', '5.', '.', '+.5e-3', '-.5', '+.', '-.', '1.e3', '.e3', 'e5', '1e', '1e+', '1e-', '1e5', '1E5', '1e+5', '1e-5', '1e5.5', '1.5.5', '1..5', '1 2', '1_000', '1,5', '\u0661\u0662\u0663',
  '9007199254740993', '9007199254740992', '9007199254740991', '9007199254740993.0000000000000000000000000001', `9007199254740993.${'0'.repeat(800)}1`, `9007199254740993.${'0'.repeat(800)}`, `1.${'0'.repeat(800)}1`, `0.${'0'.repeat(400)}5`, `${'1'.repeat(400)}`, `0.${'1'.repeat(800)}`, '9'.repeat(309), '9'.repeat(310), '1' + '0'.repeat(308), '1' + '0'.repeat(309),
  '12abc', 'abc', '--1', '+-1', '-+1', '1-', '0abc', '0 ', ' 0', '00012', '007', '08', '09.5', '0.', '-0.', 'x', 'X', '0x-1', '1x', 'e', 'E',
  '10', '11', '19', '1a', 'z', 'Z', 'zz', 'zzzzzzzzzzzz', 'ZZZZZZZZZZZZZZZZZZZZ', '-z', '+z', '0z', '0xz', 'ffffffffffffffff', 'FFFFFFFFFFFFFFFFFFFF', '1' + '0'.repeat(30), '777777777777777777777', '7' + '7'.repeat(40), '10000000000000000000000', '2'.repeat(30), '12345678901234567890123456789012345',
  'a\u{1f600}b', '\u{1f600}', ' \u2028x\u2029 ', 'x\u00a0y\u3000z', '\t', '\u00e9', 'ab cd  ef', ' a', 'a ', '  ', 'a\u00a0\ufeffb',
];

function jsonArray(parts) { return `[${parts.map((p) => JSON.stringify(p)).join(',')}]`; }

const w = new CaptureWriter([
  'uo-jsnum parity capture (increment A, without pow): written by core/crates/uo-jsnum/js/capture-jsnum.mjs, replayed by tests/differential.rs, tests/special.rs and tests/strings.rs.',
  'num/<op>/<dist>: seed = FNV-1a of the key (xorshift32), small = digest of the first 10^5 outputs, full = digest of n_full outputs (f64 bits, NaN canonicalized; integer results as numbers).',
  'special/<op>: out = the outputs in full (16 hex digits of the f64 bits, NaN canonicalized) for the inputs of tests/captures/inputs.txt, in the order tests/special.rs rebuilds.',
  'corpus/<i>: s = the input (JSON), num = Number(s), pi_<radix> = parseInt(s, radix), trim / trim_start / trim_end / split (s.split(/\\s+/)) / len (s.length).',
  'text/space_set: the code points of /\\s/ (equal to the trim set and to the Number() white space, checked here).',
  'rand/<name>: digests of results over random strings (token alphabets in the generator and in tests/strings.rs).',
]);
w.meta('generator', 'core/crates/uo-jsnum/js/capture-jsnum.mjs');

const tierOf = new Map();
const results = new Map();
for (const rec of NUMERIC) results.set(rec.key, runNumeric(rec, true));
for (const rec of STRING_RECORDS) results.set(rec.key, runStrings(rec, true));
const child = spawnSync(process.execPath, ['--no-opt', '--no-maglev', fileURLToPath(import.meta.url), '--digests-only'], { encoding: 'utf8', maxBuffer: 1 << 26 });
if (child.status !== 0) throw new Error(`tier child failed: ${child.stderr}`);
for (const line of child.stdout.split('\n').filter(Boolean)) { const [k, d] = line.split('\t'); tierOf.set(k, d); }
const disagree = [...results].filter(([k, r]) => tierOf.get(k) !== r.small).map(([k]) => k);
if (disagree.length || tierOf.size !== results.size) throw new Error(`tiers disagree on ${disagree.join(', ')} (${tierOf.size} vs ${results.size} records)`);
w.meta('tiers-agree', `${results.size} records at 10^5 (default and --no-opt --no-maglev)`);
for (const [key, r] of results) {
  const kind = key.startsWith('rand/') ? '' : 'num/';
  w.set(`${kind}${key}`, 'seed', r.seed.toString(16));
  w.set(`${kind}${key}`, 'n_small', r.nSmall);
  w.set(`${kind}${key}`, 'small', r.small);
  w.set(`${kind}${key}`, 'n_full', r.nFull);
  w.set(`${kind}${key}`, 'full', r.full);
}
for (const [op, outs] of specialRecords) {
  w.set(`special/${op}`, 'n', outs.length);
  w.set(`special/${op}`, 'out', outs.map(hex).join(''));
}
CORPUS.forEach((str, i) => {
  const key = `corpus/${String(i).padStart(3, '0')}`;
  w.set(key, 's', JSON.stringify(str));
  w.set(key, 'num', hex(Number(str)));
  for (const radix of RADICES) w.set(key, `pi_${radix}`, hex(parseInt(str, radix)));
  w.set(key, 'trim', JSON.stringify(str.trim()));
  w.set(key, 'trim_start', JSON.stringify(str.trimStart()));
  w.set(key, 'trim_end', JSON.stringify(str.trimEnd()));
  w.set(key, 'split', jsonArray(str.split(/\s+/)));
  w.set(key, 'len', str.length);
});

const spaceSet = [];
for (let cp = 0; cp <= 0x10ffff; cp++) {
  if (cp >= 0xd800 && cp <= 0xdfff) continue;
  const c = String.fromCodePoint(cp);
  const byRegex = /^\s$/.test(c);
  const byTrim = c.trim() === '';
  const byNumber = c !== '0' && Number(c) === 0;
  if (byRegex !== byTrim || byRegex !== byNumber) throw new Error(`white space sets differ at U+${cp.toString(16)}`);
  if (byRegex) spaceSet.push(cp.toString(16));
}
w.set('text/space_set', 'chars', spaceSet.join(','));

const out = opt('--out') ?? join(crate, 'tests', 'captures', 'jsnum.capture');
mkdirSync(dirname(out), { recursive: true });
w.write(out);
console.log(`wrote ${out}: ${results.size} digest records, ${specialRecords.length} special records, ${CORPUS.length} corpus strings`);
