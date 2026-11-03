// Parity capture of uo-jsnum increment B: the formatters (`String(x)`, `toFixed`, `toPrecision`,
// `toExponential`, `toString(radix)` of integers) and `parseFloat`; record core/docs/modules/uo-jsnum.md,
// "Stage 3 - port (increment B)". The reference is V8 13.6.233.17 (Node 24.15.0) x64. No math function
// is involved (V8's digit routines are integer arithmetic), so the capture may be taken on any x64 OS.
//
//   node core/crates/uo-jsnum/js/capture-jsnum-b.mjs [--out <file>] [--n <full count>]
//
// writes core/crates/uo-jsnum/tests/captures/format.capture, replayed by tests/format.rs. Options:
//
//   --out <file>     write elsewhere
//   --n <count>      full sample count of every digest record (quick runs only; never committed)
//   --digests-only   print `key<TAB>digest at 10^5` lines and exit (the child of the tier check)
//   UO_JSNUM_DUMP=<key>   also write `x(hex)<TAB>n<TAB>output` lines of that record (the first
//                         UO_JSNUM_DUMP_N samples, default 10^5) to core/target/jsnum-dump/<key>.tsv;
//                         tests/format.rs prints the first 20 differing samples from it
//
// Record kinds:
//   fmt/<op>/<dist>   seed (FNV-1a of the key, xorshift32), n_small / small (digest of the first 10^5
//                     outputs), n_full / full; the inputs are drawn with the same PRNG and IEEE
//                     operations as tests/support/format.rs (draw kinds `X` and `N` below).
//   edge/<op>/<i>     x = the input (hex bits), then the outputs written in full for every digits
//                     argument 0..=105, 1000 and 4294967295 (`RangeError` where JS throws), and for
//                     `edge/js` / `edge/radix` the String(x) and toString(radix) results.
//   sweep/<op>/<set>  digest over every power of two (and of ten, with the neighbouring doubles)
//                     times every digits argument.
//   pf/*              parseFloat of the corpus (strings come from jsnum.capture, adv.capture or are
//                     written in the record) and rand/pf_*: digests over random strings.
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
const FULL = Number(opt('--n') ?? 1_000_000);
const FULL_JS = Number(opt('--n') ?? 10_000_000);
const SMALL_STRINGS = 100_000;
const FULL_STRINGS = Number(opt('--n') ?? 1_000_000);

const f64a = new Float64Array(1);
const u32a = new Uint32Array(f64a.buffer);
const u64a = new BigUint64Array(f64a.buffer);
const fromWords = (hi, lo) => { u32a[1] = hi; u32a[0] = lo; return f64a[0]; };
const fromBits = (b) => { u64a[0] = b; return f64a[0]; };
const bitsOf = (v) => { f64a[0] = v; return u64a[0]; };
const canon = (v) => (v !== v ? NaN : v);
const hex = (v) => { f64a[0] = canon(v); return u64a[0].toString(16).padStart(16, '0'); };
const fnv = (s) => {
  let h = 0x811c9dc5;
  for (const b of Buffer.from(s)) { h ^= b; h = Math.imul(h, 0x01000193) >>> 0; }
  return h >>> 0;
};

// -- generators (mirrored in tests/support/format.rs) --------------------------------------------
const TWO53 = 9007199254740992;
const POW10 = [1e0, 1e1, 1e2, 1e3, 1e4, 1e5, 1e6, 1e7, 1e8, 1e9, 1e10, 1e11, 1e12, 1e13, 1e14, 1e15, 1e16, 1e17, 1e18, 1e19, 1e20, 1e21, 1e22];
const POW2 = [1];
for (let i = 0; i < 64; i++) POW2.push(POW2[i] * 2);
const bits = (r) => { const a = r.next(); const b = r.next(); return fromWords(a, b); };
const range = (r, lo, hi) => {
  const a = r.next();
  const b = r.next();
  const u = ((a >>> 5) * 67108864 + (b >>> 6)) / TWO53;
  return lo + (hi - lo) * u;
};
const digitCount = (v) => { let n = 1; for (let i = 1; i < POW10.length && v >= POW10[i]; i++) n++; return n; };

// x kinds: [x, aux]
const X = {
  bits: (r) => [bits(r), 0],
  r21: (r) => [range(r, -1e21, 1e21), 0],
  r1e6: (r) => [range(r, -1e6, 1e6), 0],
  r1: (r) => [range(r, -1, 1), 0],
  kd: (r) => { const k = r.next(); const d = r.next() % 19; const s = r.next() & 1; const x = k / POW10[d]; return [s ? -x : x, 0]; },
  int53: (r) => [Math.floor(range(r, -TWO53, TWO53)), 0],
  intsafe: (r) => [Math.floor(range(r, -(TWO53 - 1), TWO53 - 1)), 0],
  int1e6: (r) => [Math.floor(range(r, -1e6, 1e6)), 0],
  half: (r) => { const k = Math.floor(range(r, -1e15, 1e15)); return [k >= 0 ? k + 0.5 : k - 0.5, digitCount(Math.abs(k))]; },
  dyadic: (r) => { const j = 1 + (r.next() % 40); const k = r.next(); const s = r.next() & 1; const x = k / POW2[j]; return [s ? -x : x, j]; },
};
// n kinds: the digits / precision / radix argument
const N = {
  r0_100: (r) => r.next() % 101,
  r1_100: (r) => 1 + (r.next() % 100),
  r0_20: (r) => r.next() % 21,
  r1_21: (r) => 1 + (r.next() % 21),
  r2_36: (r) => 2 + (r.next() % 35),
  zero: () => 0,
  aux: (r, a) => a,
  aux_m1: (r, a) => Math.max(a - 1, 0),
};
const constant = (v) => () => v;

const rangeOr = (f) => (x, n) => { try { return f(x, n); } catch (e) { if (e instanceof RangeError) return 'RangeError'; throw e; } };
const OPS = {
  fixed: rangeOr((x, n) => x.toFixed(n)),
  precision: rangeOr((x, n) => x.toPrecision(n)),
  exponential: rangeOr((x, n) => x.toExponential(n)),
  radix: rangeOr((x, n) => x.toString(n)),
  js: (x) => String(x),
};

const RECORDS = [];
const rec = (op, dist, xk, nk, full = FULL) => RECORDS.push({ key: `${op}/${dist}`, op, x: X[xk], n: nk, full });
rec('js', 'bits', 'bits', null, FULL_JS);
rec('js', 'kd', 'kd', null, FULL_JS);
rec('js', 'r21', 'r21', null);
rec('js', 'r1e6', 'r1e6', null);
rec('js', 'int53', 'int53', null);
rec('js', 'dyadic', 'dyadic', null);
rec('js', 'small', 'r1', null);

rec('fixed', 'bits_r', 'bits', N.r0_100);
rec('fixed', 'r21_r', 'r21', N.r0_100);
rec('fixed', 'r1e6_r', 'r1e6', N.r0_20);
rec('fixed', 'kd_r', 'kd', N.r0_20);
rec('fixed', 'int53_r', 'int53', N.r0_100);
rec('fixed', 'small_r', 'r1', N.r0_20);
rec('fixed', 'half_0', 'half', N.zero);
rec('fixed', 'dyadic_j', 'dyadic', N.aux_m1);
for (const d of [0, 1, 2, 3, 6, 20, 21, 100]) rec('fixed', `r1e6_d${d}`, 'r1e6', constant(d));

rec('precision', 'bits_r', 'bits', N.r1_100);
rec('precision', 'r21_r', 'r21', N.r1_100);
rec('precision', 'r1e6_r', 'r1e6', N.r1_21);
rec('precision', 'kd_r', 'kd', N.r1_21);
rec('precision', 'int53_r', 'int53', N.r1_100);
rec('precision', 'half_p', 'half', N.aux);
rec('precision', 'dyadic_r', 'dyadic', N.r1_21);
for (const p of [1, 2, 12, 15, 17, 21, 100]) rec('precision', `r1e6_p${p}`, 'r1e6', constant(p));

rec('exponential', 'bits_r', 'bits', N.r0_100);
rec('exponential', 'r21_r', 'r21', N.r0_100);
rec('exponential', 'r1e6_r', 'r1e6', N.r0_20);
rec('exponential', 'kd_r', 'kd', N.r0_20);
rec('exponential', 'int53_r', 'int53', N.r0_100);
rec('exponential', 'half_p', 'half', N.aux_m1);
rec('exponential', 'dyadic_r', 'dyadic', N.r0_20);
for (const d of [0, 1, 14, 20, 100]) rec('exponential', `r1e6_d${d}`, 'r1e6', constant(d));

rec('radix', 'intsafe_r', 'intsafe', N.r2_36);
rec('radix', 'int1e6_r', 'int1e6', N.r2_36);

// -- random strings for parseFloat (tokens mirrored in tests/format.rs) --------------------------
const TOK_JUNK = ['0', '1', '7', '9', 'a', 'f', 'z', 'x', 'b', 'o', 'e', 'E', '+', '-', '.', ' ', '\t', '\u00a0', '\ufeff', '\u0085', '\u180e', '_', 'Infinity', '0x', '\u2003', '\u0661'];
const TOK_NUM = ['0', '1', '2', '3', '4', '5', '6', '7', '8', '9', '9', '0', '.', 'e', '-', '+', ' ', 'x', 'a', 'f'];
const randString = (r, tokens, maxTokens) => {
  const n = r.next() % maxTokens;
  let s = '';
  for (let i = 0; i < n; i++) s += tokens[r.next() % tokens.length];
  return s;
};
const STRING_RECORDS = [];
const strRec = (key, produce) => STRING_RECORDS.push({ key, produce });
strRec('rand/pf_junk', (r) => parseFloat(randString(r, TOK_JUNK, 12)));
strRec('rand/pf_digits', (r) => parseFloat(randString(r, TOK_NUM, 16)));
strRec('rand/pf_tail', (r) => { const head = randString(r, TOK_NUM, 12); return parseFloat(head + randString(r, TOK_JUNK, 4)); });

// -- running a record ----------------------------------------------------------------------------
const dumpKey = process.env.UO_JSNUM_DUMP;
const dumpN = Number(process.env.UO_JSNUM_DUMP_N ?? SMALL);

function runFormat(rec, withFull) {
  const rng = new Rng(fnv(`fmt/${rec.key}`));
  const s = new Stream();
  const op = OPS[rec.op];
  const total = withFull ? rec.full : SMALL;
  const dump = withFull && dumpKey === `fmt/${rec.key}` ? [] : null;
  let small = null;
  for (let i = 0; i < total; i++) {
    const [x, aux] = rec.x(rng);
    const n = rec.n ? rec.n(rng, aux) : 0;
    const out = op(x, n);
    s.str(out);
    if (dump && i < dumpN) dump.push(`${hex(x)}\t${n}\t${out}`);
    if (i + 1 === SMALL) small = s.digest().short;
  }
  if (dump) {
    const dir = join(crate, '..', '..', 'target', 'jsnum-dump');
    mkdirSync(dir, { recursive: true });
    writeFileSync(join(dir, `fmt_${rec.key.replace(/\//g, '_')}.tsv`), `${dump.join('\n')}\n`);
  }
  return { seed: fnv(`fmt/${rec.key}`), nSmall: SMALL, small, nFull: total, full: withFull ? s.finish().short : null };
}

function runStrings(rec, withFull) {
  const rng = new Rng(fnv(rec.key));
  const s = new Stream();
  const total = withFull ? FULL_STRINGS : SMALL_STRINGS;
  let small = null;
  for (let i = 0; i < total; i++) {
    s.f64(canon(rec.produce(rng)));
    if (i + 1 === SMALL_STRINGS) small = s.digest().short;
  }
  return { seed: fnv(rec.key), nSmall: SMALL_STRINGS, small, nFull: total, full: withFull ? s.finish().short : null };
}

if (flag('--digests-only')) {
  for (const rec of RECORDS) console.log(`fmt/${rec.key}\t${runFormat(rec, false).small}`);
  for (const rec of STRING_RECORDS) console.log(`${rec.key}\t${runStrings(rec, false).small}`);
  process.exit(0);
}

// -- edge values: outputs written in full --------------------------------------------------------
const lit = (...v) => v;
const EDGE = [
  ...lit(0, -0, 1, -1, 0.5, -0.5, 1.5, -1.5, 2.5, -2.5, 3.5, 0.125, 0.375, 0.0625, 1.25, 1.35, 1.005, 1.45, 8.345, 10.235, 2.675, 0.1, 0.2, 0.3, 0.7, 0.1 + 0.2),
  ...lit(123.456, 1234.5678, 0.00001234, 0.000001, 0.0000001, 1.5e-7, 1e-6, 9.5e-7, 123456789.123456789, 1 / 3, 2 / 3, 1 / 7, Math.PI, Math.E, Math.SQRT2, 1e21 / 3),
  ...lit(9.5, 99.5, 99.99, 999.5, 9.995, 0.0099999, 0.00999999, 999999.5, 9999999.5, 0.95, 0.995, 0.9995, 25, 35, 45, 15, 55, 1e-10, -1e-10, 4.35, 4.45, 0.045, 5e-7, 1e-5),
  ...lit(5e-324, -5e-324, 2.2250738585072014e-308, 2.225073858507201e-308, 1.7976931348623157e308, -1.7976931348623157e308, 1e-300, 1e-100, 1e100, 1e22, 1e23),
  ...lit(1e21, -1e21, 999999999999999868928, 999999999999999900000, 123456789012345680000, 1e20, 4294967295, 4294967296, 2147483648, -2147483648, 1e15, 1e16, 123456789012345678),
  ...lit(9007199254740991, 9007199254740992, 9007199254740994, -9007199254740991, 18014398509481984, 9223372036854775808, 18446744073709551616, 3e21, 0.5e21),
  ...lit(NaN, Infinity, -Infinity),
];
const INTEGER_EDGE = [0, -0, 1, -1, 2, 10, 15, 16, 35, 36, 255, 256, 1295, 65535, 4294967295, 4294967296, 123456789, 9007199254740991, -9007199254740991, 9007199254740990, 4503599627370496, 3 ** 30, 7 ** 17, 36 ** 10, 2 ** 52 + 1];
const NS = [...Array.from({ length: 106 }, (_, i) => i), 1000, 4294967295];

// -- sweeps --------------------------------------------------------------------------------------
function pow2(k) { return k >= -1022 ? fromBits(BigInt(k + 1023) << 52n) : fromBits(1n << BigInt(k + 1074)); }
function sweepValues(set) {
  const out = [];
  if (set === 'pow2') {
    for (let k = -1074; k <= 1023; k++) { out.push(pow2(k)); out.push(-pow2(k)); }
  } else {
    for (let k = -323; k <= 308; k++) {
      const v = Number(`1e${k}`);
      const b = bitsOf(v);
      out.push(fromBits(b - 1n), v, fromBits(b + 1n));
    }
  }
  return out;
}
const SWEEP_OPS = { fixed: [0, 100], precision: [1, 100], exponential: [0, 100] };
function runSweep(op, set) {
  const s = new Stream();
  const [lo, hi] = SWEEP_OPS[op];
  const values = sweepValues(set);
  for (const x of values) for (let n = lo; n <= hi; n++) s.str(OPS[op](x, n));
  return { count: values.length * (hi - lo + 1), digest: s.finish().short };
}

// -- parseFloat: the corpus ----------------------------------------------------------------------
function readCapture(name) {
  const records = new Map();
  for (const line of readFileSync(join(crate, 'tests', 'captures', name), 'utf8').split('\n')) {
    if (line === '' || line.startsWith('#') || line.startsWith('@')) continue;
    const [key, ...facets] = line.split('\t');
    const r = {};
    for (const f of facets) { const i = f.indexOf('='); r[f.slice(0, i)] = f.slice(i + 1); }
    records.set(key, r);
  }
  return records;
}
const PF_OWN = [
  '', ' ', '+', '-', '.', '+.', '-.', 'e', 'e5', '5e', '1e', '1e+', '1e-', '1e5', '1E5', '1e+5', '1e-5', '1e5x', '1e5.5', '1e-5e', '1.', '1.e', '1.e3', '1.5e', '.5', '.5.', '.5e', '.5e1', '.e1', '..5',
  '0', '-0', '+0', '-0.0', '0e5', '-0e-5', '00', '007', '0x10', '0X1f', '0b11', '0o17', '0xg', '-0x10', '0.0x', '0_1', '1_0', '1,5', '1 5', '1\t5', '1\u00a05', '1\u20035',
  'Infinity', '-Infinity', '+Infinity', 'Infinityx', '-Infinity1', 'Infinit', 'infinity', 'INFINITY', '+-1', '-+1', '--1', '++1', '- 1', '+ 1', 'Infinity5', 'InfinityInfinity', ' Infinity', '\tInfinity',
  'NaN', 'nan', 'inf', 'abc', '12abc', '12.5abc', '12.5.5', '12e3e4', '1e1000', '-1e1000', '1e-1000', '-1e-1000', '1e99999999999999999999', '1e-99999999999999999999', '5e-324', '2.4703282292062327e-324', '2.4703282292062328e-324',
  '\u0085', '\u180e', '\u200b', '\u0085 1', '\u180e1', '\ufeff1', '\u00a01', '\u20281', '\u20291', '\u30001', ' \t\n\r\u000b\u000c1.5 ', '\u2003-.5e-3x', '1\u0085', '\u0661', '1\u0661',
  '123456789012345678901234567890', '0.1', '0.30000000000000004', '123.456e-2xyz', '9007199254740993', '9007199254740993.0000001', `9007199254740993.${'0'.repeat(800)}1`, `${'1'.repeat(400)}`, '9'.repeat(309), '9'.repeat(400),
  `0.${'0'.repeat(400)}5`, `1.${'0'.repeat(800)}1`, `${'0'.repeat(500)}1`, `1${'0'.repeat(500)}x`, `0.${'1'.repeat(800)}z`, '1' + '0'.repeat(308), '1' + '0'.repeat(309), `1e${'9'.repeat(30)}`, `1e-${'9'.repeat(30)}`,
  '3.14abc', '2.5E+3.5', '-.5', '+.5e+1', '6.02e23', '-1.6e-19', '1.7976931348623157e308', '1.7976931348623158e308', '1.797693134862315807e308', '4.9406564584124654e-324',
];

// -- assemble ------------------------------------------------------------------------------------
const w = new CaptureWriter([
  'uo-jsnum parity capture (increment B): written by core/crates/uo-jsnum/js/capture-jsnum-b.mjs, replayed by tests/format.rs.',
  'fmt/<op>/<dist>: seed = FNV-1a of the key (xorshift32), small = digest of the first 10^5 outputs (strings, NaN etc. as text), full = digest of n_full outputs;',
  '  ops js (String(x)), fixed, precision, exponential, radix (x.toString(n)); the draws are in the generator and in tests/support/format.rs.',
  'edge/<op>/<i>: x = the input (hex bits), n<k> = the output for digits (precision, radix) k, RangeError where JS throws; edge/js has the String(x) in `js`.',
  'sweep/<op>/<set>: digest over the powers of two (both signs) or of ten (with neighbours) x every digits argument.',
  'pf/<i>: s = the input (JSON), pf = parseFloat(s) as the bits; pf/corpus/<i> and pf/hard/<i> take s from jsnum.capture and adv.capture.',
  'rand/pf_<name>: digests of parseFloat over random strings (token alphabets in the generator and in tests/format.rs).',
]);
w.meta('generator', 'core/crates/uo-jsnum/js/capture-jsnum-b.mjs');

const results = new Map();
for (const rec of RECORDS) results.set(`fmt/${rec.key}`, runFormat(rec, true));
for (const rec of STRING_RECORDS) results.set(rec.key, runStrings(rec, true));
const child = spawnSync(process.execPath, ['--no-opt', '--no-maglev', fileURLToPath(import.meta.url), '--digests-only'], { encoding: 'utf8', maxBuffer: 1 << 26 });
if (child.status !== 0) throw new Error(`tier child failed: ${child.stderr}`);
const tierOf = new Map();
for (const line of child.stdout.split('\n').filter(Boolean)) { const [k, d] = line.split('\t'); tierOf.set(k, d); }
const disagree = [...results].filter(([k, r]) => tierOf.get(k) !== r.small).map(([k]) => k);
if (disagree.length || tierOf.size !== results.size) throw new Error(`tiers disagree on ${disagree.join(', ')} (${tierOf.size} vs ${results.size} records)`);
w.meta('tiers-agree', `${results.size} records at 10^5 (default and --no-opt --no-maglev)`);
for (const [key, r] of results) {
  w.set(key, 'seed', r.seed.toString(16));
  w.set(key, 'n_small', r.nSmall);
  w.set(key, 'small', r.small);
  w.set(key, 'n_full', r.nFull);
  w.set(key, 'full', r.full);
}

const edgeOps = { fixed: 'fixed', precision: 'precision', exponential: 'exponential' };
for (const [op, name] of Object.entries(edgeOps)) {
  EDGE.forEach((x, i) => {
    const key = `edge/${name}/${String(i).padStart(3, '0')}`;
    w.set(key, 'x', hex(x));
    for (const n of NS) w.set(key, `n${n}`, OPS[op](x, n));
  });
}
EDGE.forEach((x, i) => {
  const key = `edge/js/${String(i).padStart(3, '0')}`;
  w.set(key, 'x', hex(x));
  w.set(key, 'js', String(x));
});
INTEGER_EDGE.forEach((x, i) => {
  const key = `edge/radix/${String(i).padStart(3, '0')}`;
  w.set(key, 'x', hex(x));
  for (let n = 2; n <= 36; n++) w.set(key, `n${n}`, OPS.radix(x, n));
});
for (const op of Object.keys(SWEEP_OPS)) {
  for (const set of ['pow2', 'pow10']) {
    const r = runSweep(op, set);
    w.set(`sweep/${op}/${set}`, 'count', r.count);
    w.set(`sweep/${op}/${set}`, 'digest', r.digest);
  }
}

const jsnum = readCapture('jsnum.capture');
const adv = readCapture('adv.capture');
for (const [k, r] of jsnum) {
  if (!k.startsWith('corpus/')) continue;
  w.set(`pf/${k}`, 'pf', hex(parseFloat(JSON.parse(r.s))));
}
for (const [k, r] of adv) {
  if (!k.startsWith('hard/')) continue;
  w.set(`pf/${k}`, 'pf', hex(parseFloat(JSON.parse(r.s))));
}
PF_OWN.forEach((str, i) => {
  const key = `pf/own/${String(i).padStart(3, '0')}`;
  w.set(key, 's', JSON.stringify(str));
  w.set(key, 'pf', hex(parseFloat(str)));
});

const out = opt('--out') ?? join(crate, 'tests', 'captures', 'format.capture');
mkdirSync(dirname(out), { recursive: true });
w.write(out);
console.log(`wrote ${out}: ${results.size} digest records, ${EDGE.length} edge values, ${PF_OWN.length} own parseFloat strings`);
