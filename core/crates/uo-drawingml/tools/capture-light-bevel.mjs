// Parity capture of light3d and bevel of uo-drawingml (frontier C of phase 2): the frozen TS
// `lightTable`, `lightAt`, `lightFace`, `bevelPhi` and `shadeBevel` (shared/drawingml, built in
// <ts-root>/shared/drawingml/dist) over the corpus below. Writes tests/captures/light_bevel.capture,
// replayed by tests/light_bevel_parity.rs.
//
//   node tools/capture-light-bevel.mjs --ts-root <checkout of main>     (or UO_TS_ROOT)
//
// Options: --out <file>  write elsewhere (comparisons between platforms)
//
// The light facets call no Math.pow (sin/cos/tan/atan/atan2/sqrt only: fdlibm in V8), so they are
// the same on every Node x64. The bevel facets use SRGB_LIN / LIN_SRGB, which bevel.js evaluates
// with Math.pow at load time: take the committed file in Node 24.15.0 linux-x64 with
//   GLIBC_TUNABLES=glibc.cpu.hwcaps=-FMA,-AVX2
// (the pow reference of the uo-jsnum gate; the Rust tables were generated there).
//
// Corpus (mirrored in tests/light_bevel_parity.rs; every input is reproducible from this file):
//  - light/table: lightTable over every rig x material of RIGS x MATS (including unknown names,
//    '' and undefined), the bytes of each table;
//  - light/at: lightAt over every table key x DIRS x THETAS x PHIS (NaN, +-Infinity, -0, huge,
//    off-grid, exact knots), light/face: lightFace of every table;
//  - light/phi: bevelPhi over PRSTS x TS x HWS;
//  - bevel/*: shadeBevel on generated silhouettes (analytic coverage with + - * / sqrt only, noise
//    of the seeded Rng) and random RGBA, over the Word tables (every preset and every condition
//    that selects or rejects the Word table), the light rigs x materials x directions x presets,
//    larger masks, and a silhouette with NaN coverage. Every call is made on a Uint8ClampedArray
//    and on a Uint8Array and the two results must be identical (checked here, counted in the
//    header).
//
// Object.prototype names (constructor, __proto__...) are not in the corpus: the TS reads them as
// keys of the tables (G7: the Rust port treats them as unknown).
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { CaptureWriter, Rng, Stream, importFrom, tsRoot } from '../../uo-testkit/js/testkit.mjs';

const here = dirname(fileURLToPath(import.meta.url));
const argv = process.argv.slice(2);
const outIdx = argv.indexOf('--out');
const out = outIdx >= 0 ? argv[outIdx + 1] : join(here, '..', 'tests', 'captures', 'light_bevel.capture');
const root = tsRoot(argv);
const { lightTable, lightAt, lightFace, bevelPhi } = await importFrom(root, 'shared/drawingml/dist/light3d.js');
const { shadeBevel } = await importFrom(root, 'shared/drawingml/dist/bevel.js');

const canon = (v) => (v !== v ? NaN : v);
const f = (s, v) => s.f64(canon(v));
const summary = (s) => { const n = s.items; return `${n}:${s.finish().short}`; };
const label = (x) => (x === undefined ? 'u' : x === '' ? 'e' : String(x));

// ── the corpus (mirrored in tests/light_bevel_parity.rs) ────────────────────────────────────────

const RIGS = [undefined, '', 'threePt', 'balanced', 'soft', 'harsh', 'glow', 'twoPt', 'freezing', 'contrasting', 'flood', 'chilly', 'morning',
  'sunrise', 'sunset', 'brightRoom', 'flat', 'nope', 'threePt|matte', 'Soft'];
const MATS = [undefined, '', 'warmMatte', 'matte', 'plastic', 'metal', 'flat', 'softEdge', 'powder', 'dkEdge', 'softmetal', 'nope', 'clear', 'Matte'];
const DIRS = [undefined, '', 't', 'tl', 'l', 'bl', 'b', 'br', 'r', 'tr', 'nope', 'T'];
const KEYS = [['threePt', undefined], ['threePt', 'matte'], ['threePt', 'plastic'], ['threePt', 'metal'], ['threePt', 'flat'], ['threePt', 'softEdge'],
  ['threePt', 'powder'], ['balanced', undefined], ['soft', undefined], ['harsh', undefined], ['glow', undefined], ['twoPt', undefined],
  ['freezing', undefined], ['contrasting', undefined], ['flood', undefined], ['chilly', undefined], ['morning', undefined], ['sunrise', undefined],
  ['sunset', undefined], ['threePt', 'dkEdge'], ['threePt', 'softmetal'], ['brightRoom', undefined], ['flat', undefined]];
const WORD_PRSTS = ['relaxedInset', 'circle', 'slope', 'cross', 'angle', 'softRound', 'convex', 'coolSlant', 'divot', 'riblet', 'hardEdge', 'artDeco'];
const PRSTS = [undefined, '', ...WORD_PRSTS, 'nope', 'Circle'];

const THETAS = [];
for (let k = -50; k <= 50; k++) THETAS.push(k * 7.3);
for (let k = -2; k <= 25; k++) THETAS.push(k * 15);
THETAS.push(NaN, Infinity, -Infinity, -0, 1e300, 359.99999, 360, -1e-20, -1e-300, 1e-320, 720.0000001);
const PHIS = [NaN, Infinity, -Infinity, -0, 0, -1, 1, 5, 10, 10.0000001, 20, 33.3, 40, 50, 60, 70, 74.999, 75, 80, 84.999, 85, 85.0001, 90, 180, 1e9, 1e300, -1e300];
for (let k = 0; k <= 36; k++) PHIS.push(k * 2.5);

const PHI_TS = [NaN, Infinity, -Infinity, -0, 0, 1e-4, 1e-3, 2, 1e9, -1, 1];
for (let k = 0; k <= 60; k++) PHI_TS.push(k / 50);
for (let k = 0; k < 24; k++) PHI_TS.push((k + 0.5) / 24);
const HWS = [0, 0.25, 0.5, 1, 2, 4, NaN, -1, Infinity, 1e-9, 1e9];

// silhouettes: [label, kind, W, H, seed]
const MASKS = {
  e1: ['ellipse', 1, 1, 0], e2: ['ellipse', 2, 2, 0], e3: ['ellipse', 3, 5, 0], e17: ['ellipse', 17, 9, 0], e40: ['ellipse', 40, 40, 0],
  e64: ['ellipse', 64, 48, 0], rr: ['rrect', 50, 30, 0], ring: ['ring', 60, 60, 0], blobs: ['blobs', 70, 50, 7], full: ['full', 24, 18, 0],
  empty: ['empty', 10, 10, 0], noise: ['noise', 32, 24, 3], line: ['ellipse', 100, 1, 0], col: ['ellipse', 1, 100, 0],
  big_e: ['ellipse', 256, 256, 0], big_blobs: ['blobs', 200, 150, 11], big_rr: ['rrect', 300, 120, 0], nan: ['noisenan', 24, 18, 5],
};
const NOISE_PAL = [0, 0.25, 0.5, 1, 1, 1, 1.5, -1];
const NAN_PAL = [0, 0.25, 0.5, 1, 1, 1, NaN, -1];

const ell = (x, y, ex, ey, erx, ery) => {
  const dx = (x + 0.5 - ex) / erx, dy = (y + 0.5 - ey) / ery;
  const d = Math.sqrt(dx * dx + dy * dy);
  return Math.max(0, Math.min(1, (1 - d) * Math.min(erx, ery) + 0.5));
};

function genMask(name) {
  const [kind, W, H, seed] = MASKS[name];
  const a = new Float32Array(W * H);
  const cx = W / 2, cy = H / 2;
  const rx = Math.max(W / 2 - 0.5, 0.5), ry = Math.max(H / 2 - 0.5, 0.5);
  const rng = new Rng(seed);
  if (kind === 'blobs') {
    const bl = [];
    for (let k = 0; k < 3; k++) {
      const ex = rng.below(W), ey = rng.below(H);
      const erx = 2 + rng.below(Math.max(1, W >> 1)), ery = 2 + rng.below(Math.max(1, H >> 1));
      bl.push([ex, ey, erx, ery]);
    }
    for (let y = 0; y < H; y++) for (let x = 0; x < W; x++) {
      let v = 0;
      for (const [ex, ey, erx, ery] of bl) v = Math.max(v, ell(x, y, ex, ey, erx, ery));
      a[y * W + x] = v;
    }
  } else if (kind === 'noise' || kind === 'noisenan') {
    const pal = kind === 'noise' ? NOISE_PAL : NAN_PAL;
    for (let i = 0; i < W * H; i++) a[i] = pal[rng.below(pal.length)];
  } else {
    const r = Math.min(W, H) / 4;
    for (let y = 0; y < H; y++) for (let x = 0; x < W; x++) {
      let v = 0;
      if (kind === 'ellipse') v = ell(x, y, cx, cy, rx, ry);
      else if (kind === 'ring') v = ell(x, y, cx, cy, rx, ry) * (1 - ell(x, y, cx, cy, rx / 2, ry / 2));
      else if (kind === 'rrect') {
        const qx = Math.max(Math.abs(x + 0.5 - cx) - (cx - r), 0), qy = Math.max(Math.abs(y + 0.5 - cy) - (cy - r), 0);
        v = Math.max(0, Math.min(1, 0.5 - (Math.sqrt(qx * qx + qy * qy) - r)));
      } else if (kind === 'full') v = 1;
      a[y * W + x] = v;
    }
  }
  return { w: W, h: H, a };
}

function genRgba(W, H, seed) {
  const rng = new Rng(seed);
  const d = new Uint8Array(W * H * 4);
  for (let i = 0; i < d.length; i++) d[i] = (rng.next() >>> 8) & 255;
  return d;
}

const cap = new CaptureWriter([
  'uo-drawingml parity capture of light3d and bevel: written by core/crates/uo-drawingml/tools/capture-light-bevel.mjs, replayed by tests/light_bevel_parity.rs.',
  'light/table.r_<rig> = lightTable bytes over MATS; light/at.k_<rig>_<mat> = lightAt over DIRS x THETAS x PHIS; light/face.all; light/phi.p_<prst> = bevelPhi over TS x HWS.',
  'bevel/word.p_<prst>, bevel/light.r_<rig>, bevel/dir.d_<dir>, bevel/big.all, bevel/nan.all = shadeBevel output bytes (calls in corpus order, per-call RGBA seed = 1000 + 7919 * call number).',
]);
cap.meta('ts-dist', 'shared/drawingml/dist (frozen main, 0.9.x)');

// ── light ───────────────────────────────────────────────────────────────────────────────────────

for (const rig of RIGS) {
  const s = new Stream();
  for (const mat of MATS) {
    const t = lightTable(rig, mat);
    if (t === undefined) s.tag('u'); else s.bytes(t);
  }
  cap.set('light/table', `r_${label(rig)}`, summary(s));
}

const L = new Float32Array(4);
{
  const sf = new Stream();
  for (const [rig, mat] of KEYS) {
    const t = lightTable(rig, mat);
    const s = new Stream();
    for (const dir of DIRS) for (const th of THETAS) for (const ph of PHIS) {
      lightAt(t, dir, th, ph, L);
      for (let c = 0; c < 4; c++) f(s, L[c]);
    }
    cap.set('light/at', `k_${rig}_${label(mat)}`, summary(s));
    lightFace(t, L);
    for (let c = 0; c < 4; c++) f(sf, L[c]);
  }
  cap.set('light/face', 'all', summary(sf));
}

for (const prst of PRSTS) {
  const s = new Stream();
  for (const t of PHI_TS) for (const hw of HWS) f(s, bevelPhi(prst, t, hw));
  cap.set('light/phi', `p_${label(prst)}`, summary(s));
}

// ── bevel ───────────────────────────────────────────────────────────────────────────────────────

let call = 0;
let kindsChecked = 0;
const maskCache = new Map();
const maskOf = (name) => { let m = maskCache.get(name); if (!m) { m = genMask(name); maskCache.set(name, m); } return m; };

function shade(s, maskName, spec, checkKinds) {
  const sil = maskOf(maskName);
  const rgba = genRgba(sil.w, sil.h, 1000 + 7919 * call);
  call++;
  const clamped = new Uint8ClampedArray(rgba);
  shadeBevel(clamped, sil, spec);
  if (checkKinds) {
    const plain = new Uint8Array(rgba);
    shadeBevel(plain, sil, spec);
    if (Buffer.compare(Buffer.from(plain), Buffer.from(clamped.buffer)) !== 0) throw new Error(`Uint8Array and Uint8ClampedArray differ at call ${call - 1}`);
    kindsChecked++;
  }
  s.u32(call - 1).bytes(new Uint8Array(clamped.buffer));
}

{
  // A: the Word tables, and every condition that selects or rejects them
  const COND = [[undefined, undefined, undefined], ['threePt', 't', 'warmMatte'], ['', '', ''], ['balanced', undefined, undefined],
    [undefined, 'tl', undefined], [undefined, undefined, 'matte']];
  const WS = [0.4, 0.6, 3, 12, 80, NaN];
  for (const prst of PRSTS) {
    const s = new Stream();
    for (const [rig, dir, material] of COND) for (const w of WS) for (const hr of [1, 0.5]) for (const m of ['e17', 'e40', 'rr', 'ring', 'blobs', 'noise']) {
      shade(s, m, { w, h: w * hr, prst, rig, dir, material, word: true }, true);
    }
    cap.set('bevel/word', `p_${label(prst)}`, summary(s));
  }
}

{
  // B: the light rigs x materials x presets, directions cycling
  const BM = ['e17', 'ring', 'blobs', 'noise'];
  RIGS.forEach((rig, ri) => {
    const s = new Stream();
    PRSTS.forEach((prst, pi) => MATS.forEach((material, mi) => {
      const w = [3, 12][(pi + mi) % 2];
      const hr = [1, 0.5, 2][(ri + mi) % 3];
      shade(s, BM[(pi * 14 + mi) % 4], { w, h: w * hr, prst, rig, dir: DIRS[(ri + pi + mi) % 12], material, word: false }, true);
    }));
    cap.set('bevel/light', `r_${label(rig)}`, summary(s));
  });
}

{
  // B2: every direction x preset on small masks and several rigs
  DIRS.forEach((dir) => {
    const s = new Stream();
    PRSTS.forEach((prst, pi) => ['e17', 'blobs', 'noise'].forEach((m, mi) => [3, 12].forEach((w) => {
      shade(s, m, { w, h: w, prst, rig: RIGS[(pi * 3 + mi) % 20], dir, material: undefined, word: false }, true);
    })));
    cap.set('bevel/dir', `d_${label(dir)}`, summary(s));
  });
}

{
  // C: larger masks, Word table on and off
  const s = new Stream();
  for (const m of ['big_e', 'big_blobs', 'big_rr']) for (const prst of [undefined, 'circle', 'relaxedInset', 'angle', 'softRound', 'convex']) for (const word of [false, true]) for (const w of [6, 24, 100]) {
    shade(s, m, { w, h: w, prst, rig: undefined, dir: undefined, material: undefined, word }, false);
  }
  cap.set('bevel/big', 'all', summary(s));
}

{
  // D: a silhouette with NaN coverage (the distance transform and the blur see NaN)
  const s = new Stream();
  for (const word of [false, true]) for (const prst of [undefined, 'relaxedInset', 'angle']) for (const w of [3, 12]) {
    shade(s, 'nan', { w, h: w, prst, rig: undefined, dir: undefined, material: undefined, word }, true);
  }
  cap.set('bevel/nan', 'all', summary(s));
}

cap.meta('bevel-calls', call);
cap.meta('uint8-vs-clamped-checked', kindsChecked);
cap.write(out);
console.log(`wrote ${out}: ${cap.records.size} keys, ${call} bevel calls, ${kindsChecked} Uint8/Uint8Clamped comparisons equal`);
