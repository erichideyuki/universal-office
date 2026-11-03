// Parity capture of uo_jsnum::math::pow (Math.pow / **): record core/docs/modules/uo-jsnum.md,
// "Parity plan" and "Stage 3 — pow (option a)". The reference is Node 24.15.0 (V8 13.6.233.17)
// linux-x64 with glibc 2.39 and its non-FMA pow forced (gate 1, G1 (a) and G2), so the committed
// capture is taken in WSL Ubuntu-24.04:
//
//   GLIBC_TUNABLES=glibc.cpu.hwcaps=-FMA,-AVX2 node core/crates/uo-jsnum/js/capture-pow.mjs
//
// It writes core/crates/uo-jsnum/tests/captures/pow.capture, replayed by tests/pow.rs. Options:
//
//   --out <file>          write elsewhere (required with --any-platform)
//   --any-platform        run on another platform, for comparison only (never the committed file)
//   --verify-wasm <file>  instead of capturing: load a wasm module exporting `pow(f64, f64) -> f64`
//                         (no imports), recompute everything with it and compare with the capture
//   --quick               10^5 samples per distribution only (no 10^7 digests; for --verify-wasm)
//   UO_JSNUM_DUMP=pow     also write the raw outputs of each distribution (f64 LE, NaN
//                         canonicalized) to core/target/jsnum-dump/pow-<dist>.f64
//   --dump-dir <dir>      write those dumps to <dir> instead (comparisons between platforms)
//
// Inputs come from uo-testkit's Rng (xorshift32) with the same IEEE operations as the Rust side;
// outputs are hashed with uo-testkit's Stream after NaN canonicalization. The capture is made twice,
// in the default tiers and with --no-opt --no-maglev, and must agree (@tiers-agree).
import { execFileSync } from 'node:child_process';
import { mkdirSync, readFileSync, writeFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { CaptureWriter, Rng, Stream, platform } from '../../uo-testkit/js/testkit.mjs';

const here = dirname(fileURLToPath(import.meta.url));
const crate = join(here, '..');
const committed = join(crate, 'tests', 'captures', 'pow.capture');
const argv = process.argv.slice(2);
const opt = (name) => { const i = argv.indexOf(name); return i >= 0 ? argv[i + 1] : undefined; };
const flag = (name) => argv.includes(name);

export const N5 = 100_000;
export const N7 = 10_000_000;
const TWO53 = 9007199254740992;
const MASK = 'glibc.cpu.hwcaps=-FMA,-AVX2';

// ── inputs (mirrored in tests/pow.rs) ───────────────────────────────────────────────────────────
const dv = new DataView(new ArrayBuffer(8));
const fromBits = (hi, lo) => { dv.setUint32(0, lo, true); dv.setUint32(4, hi, true); return dv.getFloat64(0, true); };
const hexOf = (v) => { dv.setFloat64(0, v, true); return dv.getBigUint64(0, true).toString(16).padStart(16, '0'); };
const canon = (v) => (v !== v ? NaN : v);
/** Uniform in [0, 1) with 53 bits: two draws, a then b. */
const u53 = (rng) => { const a = rng.next(); const b = rng.next(); return ((a >>> 5) * 67108864 + (b >>> 6)) / TWO53; };
const range = (rng, lo, hi) => lo + (hi - lo) * u53(rng);
const bits = (rng) => { const hi = rng.next(); const lo = rng.next(); return fromBits(hi, lo); };

const GAMMA_Y = [2.4, 1 / 2.4, 2.2, 1 / 2.2, 3, -1, 10, 0.5, 2];

/** name, seed, (rng) => [x, y]. The order of the draws is part of the contract. */
export const DISTRIBUTIONS = [
  ['gamma', 0x706f7701, (r) => { const x = range(r, 0, 2); return [x, GAMMA_Y[r.below(GAMMA_Y.length)]]; }],
  ['range', 0x706f7702, (r) => { const x = range(r, 0, 100); return [x, range(r, -5, 5)]; }],
  ['inty', 0x706f7703, (r) => { const x = 1e3 - 1e3 * u53(r); return [x, r.below(41) - 20]; }],
  ['negint', 0x706f7704, (r) => { const x = range(r, -1e3, 0); return [x, r.below(81) - 40]; }],
  ['near1', 0x706f7705, (r) => { const x = 1 + range(r, -(2 ** -20), 2 ** -20); return [x, range(r, -(2 ** 30), 2 ** 30)]; }],
  ['wide', 0x706f7706, (r) => { const x = range(r, 0.25, 4); return [x, range(r, -1100, 1100)]; }],
  ['subx', 0x706f7707, (r) => { const hi = r.next(); const lo = r.next(); return [fromBits(hi & 0x800fffff, lo), range(r, -2, 2)]; }],
  ['bits', 0x706f7708, (r) => { const x = bits(r); return [x, bits(r)]; }],
];

/** Special values: every pair (x, y) of this list is written out in full. */
export function specials() {
  const v = [0, -0, Infinity, -Infinity, NaN, 5e-324, -5e-324, Number.MAX_VALUE, -Number.MAX_VALUE,
    2.2250738585072014e-308, -2.2250738585072014e-308, 1, -1, 0.5, -0.5, 2, -2, 3, -3, 10, 0.1, 1e22, 2 ** 1023,
    Math.PI / 4, 2.4, 1 / 2.4, 1 + 2 ** -52, 1 - 2 ** -53, -(1 + 2 ** -52), 2 ** 53, 2 ** 53 + 2, 2 ** 52 + 1, -(2 ** 52 + 1),
    2 ** 63, 2 ** 64, 1e300, -1e300, 1075, 1100, -1074, -1075, 0.75, 1.5];
  // high-word thresholds of the core (|y| < 2^-65 and |y| >= 2^63, the topx test) with low words
  // 0, 1, 0xffffffff, both signs
  for (const hi of [0x3be00000, 0x3bdfffff, 0x43e00000, 0x43dfffff, 0x00100000, 0x000fffff]) {
    for (const lo of [0, 1, 0xffffffff]) { v.push(fromBits(hi, lo), fromBits(hi | 0x80000000, lo)); }
  }
  return [...new Map(v.map((x) => [hexOf(x), x])).values()]; // first occurrence of each value
}

const POW10 = [-350, 350];

/** The reference of G1 (a) / G2. */
function pinned() {
  const header = process.report.getReport().header;
  return process.version === 'v24.15.0' && process.versions.v8.startsWith('13.6.233.17') && process.arch === 'x64'
    && process.platform === 'linux' && header.glibcVersionRuntime === '2.39' && process.env.GLIBC_TUNABLES === MASK
    && process.execArgv.length === 0;
}

// ── run ─────────────────────────────────────────────────────────────────────────────────────────
/** Every result with `pow`, as { facets: Map<key, [facet, value][]>, dumps }. */
export function run(pow, { n7 = true, dump = false } = {}) {
  const out = new Map();
  const dumps = new Map();
  for (const [name, seed, gen] of DISTRIBUTIONS) {
    const rng = new Rng(seed);
    const total = n7 ? N7 : N5;
    const in5 = new Stream(); const s5 = new Stream(); const s7 = new Stream();
    const raw = dump ? new Float64Array(total) : null;
    for (let i = 0; i < total; i++) {
      const [x, y] = gen(rng);
      const r = canon(pow(x, y));
      if (i < N5) { in5.f64(canon(x)).f64(canon(y)); s5.f64(r); }
      if (n7) s7.f64(r);
      if (raw) raw[i] = r;
    }
    const facets = [['seed', `0x${seed.toString(16)}`], ['in5', in5.finish().short], ['n5', `${N5}:${s5.finish().short}`]];
    if (n7) facets.push(['n7', `${N7}:${s7.finish().short}`]);
    out.set(`dist/${name}`, facets);
    if (raw) dumps.set(name, raw);
  }
  const sp = specials();
  for (const x of sp) out.set(`special/${hexOf(x)}`, [['y', sp.map((y) => hexOf(canon(pow(x, y)))).join(',')]]);
  for (let n = POW10[0]; n <= POW10[1]; n++) {
    const r = pow(10, n);
    out.set(`pow10/${n}`, [['r', hexOf(canon(r))], ['lit', r === Number(`1e${n}`) ? '1' : '0']]);
  }
  return { out, dumps };
}

const jsPow = (x, y) => Math.pow(x, y);

function capture() {
  const any = flag('--any-platform');
  const target = opt('--out') ?? committed;
  if (!any && !pinned()) {
    throw new Error(`not the pinned platform (Node v24.15.0 / V8 13.6.233.17 linux-x64, glibc 2.39, GLIBC_TUNABLES=${MASK}): ${platform()}`);
  }
  if (any && target === committed) throw new Error('--any-platform needs --out (the committed capture is pinned)');
  const dump = (process.env.UO_JSNUM_DUMP ?? '').split(',').includes('pow');
  const t0 = Date.now();
  const { out, dumps } = run(jsPow, { dump });
  const t1 = Date.now();
  const child = JSON.parse(execFileSync(process.execPath, ['--no-opt', '--no-maglev', fileURLToPath(import.meta.url), '--child'],
    { encoding: 'utf8', maxBuffer: 1 << 28, env: process.env }));
  const disagree = [...out].filter(([k, f]) => JSON.stringify(f) !== JSON.stringify(child[k])).map(([k]) => k);
  if (disagree.length) throw new Error(`tiers disagree (default vs --no-opt --no-maglev): ${disagree.slice(0, 20).join(' ')}`);
  const w = new CaptureWriter([
    'uo-jsnum pow parity capture: written by core/crates/uo-jsnum/js/capture-pow.mjs, replayed by tests/pow.rs.',
    'dist/<name>: seed, in5 = digest of the first 10^5 inputs (x, y), n5/n7 = count:digest of the outputs.',
    'special/<x bits>: y = pow(x, y) bits for every y of the special list, in list order.',
    'pow10/<n>: r = Math.pow(10, n) bits, lit = 1 when it equals Number("1e" + n).',
    'NaN outputs canonicalized to 7ff8000000000000 before hashing and writing.',
  ]);
  w.meta('tiers-agree', 'yes (default and --no-opt --no-maglev)');
  w.meta('pow-specials', specials().map(hexOf).join(','));
  w.meta('pow10-not-literal', [...out].filter(([k, f]) => k.startsWith('pow10/') && f[1][1] === '0').map(([k]) => k.slice(6)).join(',') || 'none');
  for (const [k, facets] of out) for (const [f, v] of facets) w.set(k, f, v);
  mkdirSync(dirname(target), { recursive: true });
  w.write(target);
  if (dumps.size) {
    const dir = opt('--dump-dir') ?? join(crate, '..', '..', 'target', 'jsnum-dump');
    mkdirSync(dir, { recursive: true });
    for (const [name, raw] of dumps) writeFileSync(join(dir, `pow-${name}.f64`), new Uint8Array(raw.buffer));
    console.log(`dumps in ${dir}`);
  }
  console.log(`wrote ${target} (${((t1 - t0) / 1000).toFixed(1)} s per tier)\n@platform ${platform()}`);
}

async function verifyWasm(file) {
  const { instance } = await WebAssembly.instantiate(readFileSync(file), {});
  const wasmPow = instance.exports.pow;
  if (typeof wasmPow !== 'function') throw new Error(`${file} exports no pow`);
  const text = readFileSync(opt('--capture') ?? committed, 'utf8');
  const cap = new Map();
  for (const line of text.split('\n')) {
    if (!line || line.startsWith('#') || line.startsWith('@')) continue;
    const [key, ...fs] = line.split('\t');
    cap.set(key, fs.map((f) => { const i = f.indexOf('='); return [f.slice(0, i), f.slice(i + 1)]; }));
  }
  const quick = flag('--quick');
  const { out } = run(wasmPow, { n7: !quick });
  let checked = 0; const bad = [];
  for (const [k, facets] of out) {
    const want = cap.get(k);
    for (const [f, v] of facets) {
      checked++;
      const w = want?.find(([g]) => g === f)?.[1];
      if (w !== v) bad.push(`${k} ${f}: wasm ${v} capture ${w}`);
    }
  }
  console.log(`wasm ${file}: ${checked - bad.length}/${checked} facets equal${quick ? ' (quick: n7 not checked)' : ''}`);
  for (const b of bad.slice(0, 20)) console.log(`  ${b}`);
  if (bad.length) process.exitCode = 1;
}

if (process.argv[1] === fileURLToPath(import.meta.url)) {
  if (flag('--child')) {
    process.stdout.write(JSON.stringify(Object.fromEntries(run(jsPow).out)));
  } else if (opt('--verify-wasm')) {
    await verifyWasm(opt('--verify-wasm'));
  } else {
    capture();
  }
}
