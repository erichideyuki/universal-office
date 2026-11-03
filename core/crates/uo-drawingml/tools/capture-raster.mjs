// Parity capture of the raster and effects modules of uo-drawingml (frontier B of stage 3, phase 2):
// the frozen TS `flattenPath`, `fillPolygons`, `gaussianBlur`, `distanceTo`, `morph`, `shiftMask`,
// `warpMask`, `mulAffine`, `invertAffine` and the effects (`alignPoint`, `shadowAffine`,
// `outerShadowMask`, `innerShadowMask`, `glowMask`, `softEdgeMask`, `reflectionAlpha`) of
// <ts-root>/shared/drawingml/dist over the corpus below. Writes tests/captures/raster.capture,
// replayed by tests/raster_parity.rs, which mirrors every generator of this file.
//
//   node tools/capture-raster.mjs --ts-root <checkout of main>     (or UO_TS_ROOT)
//
// Options: --out <file>  write elsewhere (comparisons between platforms)
//
// Corpus (every input is reproducible from this file; the capture lines carry only digests):
//  - flatten: the outline of every preset at 5 sizes x 10 affines (identity, scales, rotation, skew,
//    a flip, determinant 0, a NaN skew, 1e6) x tol {0.35, 0, 1e-3, NaN, -0}; 4,000 generated
//    subpath lists with every segment kind and hostile values (NaN, +-Infinity, 1e308, -0);
//  - fill: 3,000 generated polygon sets (rectangles on and off the pixel grid, triangles,
//    pentagrams, opposite windings, horizontal edges, repeated vertices, far-away and sliver
//    polygons) on masks from 0x5 to 64x48, empty and pre-filled, even-odd and non-zero; every preset
//    outline flattened and filled on two mask sizes under both rules. A polygon with a non-finite
//    coordinate is NOT in the corpus (rule of the plan: excluded from equality);
//  - gaussianBlur: 16 sigmas (negative, 0, 0.04, 0.0499, 0.05, 0.3 ... 33.3, 1000, NaN, +-Infinity)
//    x 8 mask sizes (empty included) x 4 mask kinds (random, discs, binary, hostile values);
//  - distanceTo (both directions), morph (17 radii incl. +-0.005, +-0.01, NaN, +-Infinity), shiftMask
//    (14 offsets x 4 target sizes), warpMask (11 inverse affines x 3 target sizes), mulAffine and
//    invertAffine over a value pool;
//  - effects: alignPoint (every algn, undefined, '', unknown), shadowAffine (2,000 generated specs),
//    the four masks on generated silhouettes (discs, star5, heart) over generated specs, radii
//    (NaN and negative included) and pixel scales, reflectionAlpha over a grid with NaN.
//
// Math facets: none uses pow. exp (the blur kernel), sin, cos, tan are fdlibm in V8 and hypot is the
// V8 Torque builtin: identical on every Node x64. The committed file is taken in WSL (Node
// v24.15.0 linux-x64, GLIBC_TUNABLES=glibc.cpu.hwcaps=-FMA,-AVX2, as the pow reference
// environment) and a Windows x64 Node 24.15.0 run gives the same content line for line.
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { CaptureWriter, Rng, Stream, importFrom, tsRoot } from '../../uo-testkit/js/testkit.mjs';

const here = dirname(fileURLToPath(import.meta.url));
const argv = process.argv.slice(2);
const outIdx = argv.indexOf('--out');
const out = outIdx >= 0 ? argv[outIdx + 1] : join(here, '..', 'tests', 'captures', 'raster.capture');
const root = tsRoot(argv);
const dml = await importFrom(root, 'shared/drawingml/dist/index.js');
const {
  newMask, flattenPath, fillPolygons, gaussianBlur, distanceTo, morph, shiftMask, warpMask, invertAffine, mulAffine, IDENTITY,
  alignPoint, shadowAffine, outerShadowMask, innerShadowMask, glowMask, softEdgeMask, reflectionAlpha, presetOutline, PRESET_DEFS,
} = dml;

const canon = (v) => (v !== v ? NaN : v);
const f = (s, v) => s.f64(canon(v));
const dig = (fn) => { const s = new Stream(); fn(s); return `${s.items}:${s.finish().short}`; };
const U = undefined;

const f32bytes = (a) => {
  const c = new Float32Array(a);
  const u = new Uint32Array(c.buffer);
  for (let i = 0; i < c.length; i++) if (c[i] !== c[i]) u[i] = 0x7fc00000;
  return Buffer.from(c.buffer, c.byteOffset, c.byteLength);
};
const maskOut = (s, m) => { s.u32(m.w); s.u32(m.h); s.bytes(f32bytes(m.a)); };
const attempt = (s, fn) => {
  let res;
  try { res = fn(); } catch (e) { if (!(e instanceof RangeError)) throw e; s.tag('E'); return; }
  s.tag('K');
  return res;
};
const writeMask = (s, fn) => { const m = attempt(s, fn); if (m) maskOut(s, m); };
const affineOut = (s, a) => { for (const v of a) f(s, v); };
const polysOut = (s, polys) => { s.u32(polys.length); for (const p of polys) { s.u32(p.length); for (const v of p) f(s, v); } };

const cap = new CaptureWriter(['Parity capture of raster.ts and effects.ts (uo-drawingml frontier B): see tools/capture-raster.mjs']);
const put = (key, v) => cap.set(key, 'v', v);

// ── generators (mirrored in tests/raster_parity.rs) ─────────────────────────────────────────────

const MASK_DIMS = [[1, 1], [2, 2], [17, 9], [40, 40], [1, 30], [30, 1], [0, 0], [64, 48]];
const HOSTILE = [0, -0, 0.25, 0.5, 1, 2, -1, NaN, 0.5, 0.4999999];

function genMask(w, h, kind, seed) {
  const m = newMask(w, h);
  const r = new Rng(seed);
  switch (kind) {
    case 0: for (let i = 0; i < w * h; i++) m.a[i] = r.below(9) / 8; break;
    case 1: {
      const k = 1 + r.below(3);
      for (let d = 0; d < k; d++) {
        const cx = r.below(w * 2 + 1) / 2, cy = r.below(h * 2 + 1) / 2;
        const rad = 1 + r.below(Math.max(w, h) * 2 + 1) / 2;
        const r2 = rad * rad;
        for (let y = 0; y < h; y++) {
          for (let x = 0; x < w; x++) {
            const dx = x + 0.5 - cx, dy = y + 0.5 - cy;
            const d2 = dx * dx + dy * dy;
            if (d2 < r2) {
              const c = ((r2 - d2) / r2) * 4;
              const cov = c > 1 ? 1 : c;
              const i = y * w + x;
              m.a[i] = m.a[i] > cov ? m.a[i] : cov;
            }
          }
        }
      }
      break;
    }
    case 2: for (let i = 0; i < w * h; i++) m.a[i] = r.below(3) === 0 ? 1 : 0; break;
    case 3: for (let i = 0; i < w * h; i++) m.a[i] = HOSTILE[r.below(HOSTILE.length)]; break;
    default: throw new Error('kind');
  }
  return m;
}

const PENTA = [[0, -1], [0.9510565, -0.309017], [0.5877853, 0.809017], [-0.5877853, 0.809017], [-0.9510565, -0.309017]];

function genPolys(seed, w, h) {
  const r = new Rng(seed);
  const X = () => r.below((w + 4) * 8 + 1) / 8 - 2;
  const Y = () => r.below((h + 4) * 8 + 1) / 8 - 2;
  const polys = [];
  const kind = r.below(8);
  const np = 1 + r.below(3);
  for (let p = 0; p < np; p++) {
    const pts = [];
    switch (kind) {
      case 0: {
        const x0 = Math.floor(X()), y0 = Math.floor(Y());
        const x1 = x0 + 1 + r.below(w + 1), y1 = y0 + 1 + r.below(h + 1);
        pts.push(x0, y0, x1, y0, x1, y1, x0, y1);
        break;
      }
      case 1: {
        const x0 = X(), y0 = Y();
        const x1 = x0 + r.below(w * 8 + 1) / 8, y1 = y0 + r.below(h * 8 + 1) / 8;
        pts.push(x0, y0, x1, y0, x1, y1, x0, y1);
        break;
      }
      case 2: pts.push(X(), Y(), X(), Y(), X(), Y()); break;
      case 3: {
        const cx = X(), cy = Y(), R = 1 + r.below((w + h) * 4 + 1) / 8;
        for (const i of [0, 2, 4, 1, 3]) pts.push(cx + R * PENTA[i][0], cy + R * PENTA[i][1]);
        break;
      }
      case 4: {
        const x0 = X(), y0 = Y(), a = 1 + r.below(w * 4 + 1) / 8, dx = r.below(17) / 8, dy = r.below(17) / 8;
        pts.push(x0, y0, x0 + a, y0, x0 + a, y0 + a, x0, y0 + a);
        pts.push(x0 + dx, y0 + dy, x0 + dx, y0 + a + dy, x0 + a + dx, y0 + a + dy, x0 + a + dx, y0 + dy);
        break;
      }
      case 5: { const k = 3 + r.below(7); for (let i = 0; i < k; i++) pts.push(X(), Y()); break; }
      case 6: {
        const a = X(), b = Y(), c = X(), d = Y();
        pts.push(a, b, c, b, c, b, c, d, a, d, a, b);
        break;
      }
      case 7: {
        const y = Y();
        pts.push(-1e6, y, 1e6, y + 0.25, X(), 1e6, -1e6, -1e6);
        break;
      }
    }
    polys.push(pts);
  }
  return polys;
}

const FILL_DIMS = [[1, 1], [5, 3], [16, 16], [33, 17], [64, 48], [0, 5], [5, 0], [3, 40]];

// ── flatten ─────────────────────────────────────────────────────────────────────────────────────

const PRESET_SIZES = [[1, 1], [7, 13], [64, 64], [200, 100], [400, 150]];
const AFFINES = [
  [1, 0, 0, 1, 0, 0], [1.7, 0, 0, 1.7, 3.25, -2], [0.01, 0, 0, 0.01, 0, 0], [0.866025403784, 0.5, -0.5, 0.866025403784, 10, 20],
  [1, 0.3, 0.7, 1, 0, 0], [-1, 0, 0, 1, 100, 0], [1, 2, 2, 4, 0, 0], [1, 0, NaN, 1, 0, 0], [1e6, 0, 0, 1e6, 0, 0], [3, 0, 0, 0.25, -7.5, 11],
];
const TOLS = [0.35, 0, 1e-3, NaN, -0];
const names = Object.keys(PRESET_DEFS);

for (const name of names) {
  for (let si = 0; si < PRESET_SIZES.length; si++) {
    const [w, h] = PRESET_SIZES[si];
    const subs = presetOutline(name, w, h).subs;
    put(`flatten/preset/${name}/${si}`, dig((s) => {
      for (const m of AFFINES) for (const tol of TOLS) polysOut(s, flattenPath(subs, m, tol));
    }));
  }
}

const POOL = [0, 1, -1, 0.5, 2.5, 10, 100.25, 1e3, -37.5, 1e-3, NaN, Infinity, -Infinity, 1e308, -0];
function genSubs(seed) {
  const r = new Rng(seed);
  const val = () => (r.below(12) === 0 ? POOL[r.below(POOL.length)] : (r.below(2001) - 1000) / 8);
  const ang = () => (r.below(12) === 0 ? POOL[r.below(POOL.length)] : (r.below(1257) - 628) / 100);
  const rad = () => (r.below(12) === 0 ? POOL[r.below(POOL.length)] : r.below(801) / 8);
  const subs = [];
  const ns = 1 + r.below(4);
  for (let i = 0; i < ns; i++) {
    const sub = [];
    const k = 1 + r.below(8);
    for (let j = 0; j < k; j++) {
      switch (r.below(6)) {
        case 0: sub.push({ c: 'M', x: val(), y: val() }); break;
        case 1: sub.push({ c: 'L', x: val(), y: val() }); break;
        case 2: sub.push({ c: 'C', x1: val(), y1: val(), x2: val(), y2: val(), x: val(), y: val() }); break;
        case 3: sub.push({ c: 'Q', x1: val(), y1: val(), x: val(), y: val() }); break;
        case 4: sub.push({ c: 'E', cx: val(), cy: val(), rx: rad(), ry: rad(), a0: ang(), a1: ang(), ccw: r.below(2) === 1 }); break;
        default: sub.push({ c: 'Z' });
      }
    }
    subs.push(sub);
  }
  return subs;
}
const SUB_SEEDS = 4000, SUB_BLOCK = 100;
for (let b = 0; b < SUB_SEEDS / SUB_BLOCK; b++) {
  put(`flatten/rand/${b}`, dig((s) => {
    for (let i = b * SUB_BLOCK; i < (b + 1) * SUB_BLOCK; i++) {
      const m = AFFINES[i % 10], tol = TOLS[Math.floor(i / 10) % 5];
      s.u32(i);
      polysOut(s, flattenPath(genSubs(i + 1), m, tol));
    }
  }));
}

// ── fill ────────────────────────────────────────────────────────────────────────────────────────

const FILL_SEEDS = 3000, FILL_BLOCK = 60;
for (let b = 0; b < FILL_SEEDS / FILL_BLOCK; b++) {
  put(`fill/rand/${b}`, dig((s) => {
    for (let i = b * FILL_BLOCK; i < (b + 1) * FILL_BLOCK; i++) {
      const [w, h] = FILL_DIMS[i % FILL_DIMS.length];
      const m = newMask(w, h);
      if (i % 3 === 0) { const r = new Rng(i + 7); for (let k = 0; k < w * h; k++) m.a[k] = r.below(7) / 4; }
      fillPolygons(m, genPolys(i + 1, w, h), i % 2 === 1);
      s.u32(i);
      maskOut(s, m);
    }
  }));
}

const FILL_PRESET_SIZES = [[48, 32], [21, 21]];
function presetSil(name, w, h, evenOdd) {
  const polys = flattenPath(presetOutline(name, w, h).subs, IDENTITY, 0.35);
  const m = newMask(w, h);
  fillPolygons(m, polys, evenOdd);
  return m;
}
for (const name of names) {
  for (let si = 0; si < FILL_PRESET_SIZES.length; si++) {
    const [w, h] = FILL_PRESET_SIZES[si];
    for (const eo of [false, true]) {
      put(`fill/preset/${name}/${si}/${eo ? 'eo' : 'nz'}`, dig((s) => maskOut(s, presetSil(name, w, h, eo))));
    }
  }
}

// ── blur, distance, morph, shift, warp ──────────────────────────────────────────────────────────

const SIGMAS = [-1, 0, 0.04, 0.0499, 0.05, 0.3, 0.5, 1, 1.7, 2.5, 5, 10, 33.3, NaN, Infinity, -Infinity, 1000];
for (let si = 0; si < SIGMAS.length; si++) {
  put(`blur/${si}`, dig((s) => {
    for (const [w, h] of MASK_DIMS) {
      if (SIGMAS[si] === 1000 && w * h > 200) continue;
      for (let kind = 0; kind < 4; kind++) {
        const m = genMask(w, h, kind, 100 + si * 10 + kind);
        s.u32(w * 4 + kind);
        writeMask(s, () => gaussianBlur(m, SIGMAS[si]));
      }
    }
  }));
}

for (let kind = 0; kind < 4; kind++) {
  for (const inside of [true, false]) {
    put(`edt/${kind}/${inside ? 'in' : 'out'}`, dig((s) => {
      for (let i = 0; i < MASK_DIMS.length; i++) {
        const [w, h] = MASK_DIMS[i];
        for (let seed = 0; seed < 3; seed++) {
          const m = genMask(w, h, kind, 500 + i * 7 + seed);
          s.u32(i * 3 + seed);
          s.bytes(f32bytes(distanceTo(m, inside)));
        }
      }
    }));
  }
}

const RADII = [0, 0.005, -0.005, 0.0099, 0.01, -0.01, 0.5, -0.5, 1, -1, 3.7, -3.7, 20, -20, NaN, Infinity, -Infinity];
for (let ri = 0; ri < RADII.length; ri++) {
  put(`morph/${ri}`, dig((s) => {
    for (const [w, h] of MASK_DIMS) {
      if (w * h > 1600) continue;
      for (let kind = 0; kind < 4; kind++) {
        const m = genMask(w, h, kind, 900 + ri * 10 + kind);
        s.u32(w * 4 + kind);
        writeMask(s, () => morph(m, RADII[ri]));
      }
    }
  }));
}

const SHIFTS = [[0, 0], [1, 0], [0, -1], [0.5, 0.5], [-2.25, 3.75], [100, 100], [-1e3, 5], [0.001, -0.001], [NaN, 0], [0, NaN], [Infinity, 0], [0, -Infinity], [1e308, 0], [-0, -0]];
for (let ki = 0; ki < SHIFTS.length; ki++) {
  put(`shift/${ki}`, dig((s) => {
    for (const [w, h] of MASK_DIMS) {
      for (let kind = 0; kind < 4; kind++) {
        const m = genMask(w, h, kind, 1300 + ki * 10 + kind);
        const [dx, dy] = SHIFTS[ki];
        for (const size of [U, [w + 3, h + 2], [1, 1], [0, 3]]) {
          s.u32(w * 4 + kind);
          writeMask(s, () => (size ? shiftMask(m, dx, dy, size[0], size[1]) : shiftMask(m, dx, dy)));
        }
      }
    }
  }));
}

const WARPS = [
  [1, 0, 0, 1, 0, 0], [1, 0, 0, 1, 0.5, 0.5], [0.5, 0, 0, 0.5, 0, 0], [2, 0, 0, 2, -3, 1], [0.866025403784, -0.5, 0.5, 0.866025403784, 4, -6],
  [1, 0.3, 0.7, 1, 0, 0], [0, 0, 0, 0, 1, 2], [NaN, 0, 0, 1, 0, 0], [Infinity, 0, 0, 1, 0, 0], [1e308, 0, 0, 1e308, 0, 0], [-1, 0, 0, 1, 40, 0],
];
for (let ai = 0; ai < WARPS.length; ai++) {
  put(`warp/${ai}`, dig((s) => {
    for (const [w, h] of MASK_DIMS) {
      for (let kind = 0; kind < 4; kind++) {
        const m = genMask(w, h, kind, 1700 + ai * 10 + kind);
        for (const [tw, th] of [[w, h], [w + 5, h + 1], [1, 1], [0, 4]]) {
          s.u32(w * 4 + kind);
          writeMask(s, () => warpMask(m, WARPS[ai], tw, th));
        }
      }
    }
  }));
}

const APOOL = [0, 1, -1, 0.5, 2, -3.5, 1e-9, 1e9, NaN, Infinity, 0.3, -0];
put('affine/mul', dig((s) => {
  const r = new Rng(31);
  for (let i = 0; i < 3000; i++) {
    const m = [], n = [];
    for (let k = 0; k < 6; k++) m.push(APOOL[r.below(APOOL.length)]);
    for (let k = 0; k < 6; k++) n.push(APOOL[r.below(APOOL.length)]);
    affineOut(s, mulAffine(m, n));
  }
}));
put('affine/invert', dig((s) => {
  const r = new Rng(37);
  for (let i = 0; i < 3000; i++) {
    const m = [];
    for (let k = 0; k < 6; k++) m.push(APOOL[r.below(APOOL.length)]);
    affineOut(s, invertAffine(m));
  }
}));

// ── effects ─────────────────────────────────────────────────────────────────────────────────────

const ALGNS = [U, 'tl', 't', 'tr', 'l', 'ctr', 'r', 'bl', 'b', 'br', '', 'xx', 'TL'];
put('fx/alignPoint', dig((s) => {
  for (const a of ALGNS) {
    for (const [x, y, w, h] of [[0, 0, 100, 50], [10.5, -3, 7, 0], [1e3, 1e3, 1e-3, 1e9], [NaN, 0, 1, 1], [0, 0, Infinity, 2]]) {
      const [px, py] = alignPoint(a, x, y, w, h);
      f(s, px); f(s, py);
    }
  }
}));

function genSpec(r) {
  const pick = (a) => a[r.below(a.length)];
  const blurPt = pick([0, 1, 6, 12.5]);
  const distPt = pick([0, 3, -4, 7.25]);
  const dirDeg = pick([0, 45, 90, 135, 200, 360, -30, NaN]);
  const sx = pick([U, 1, 0.5, 1.5, 0, -1]), sy = pick([U, 1, 0.5, 1.5, 0, -1]);
  const kxDeg = pick([U, 0, 30, 89.9, -20, 90]), kyDeg = pick([U, 0, 30, 89.9, -20, 90]);
  const algn = pick(ALGNS);
  const box = { x: r.below(5), y: r.below(5), w: 10 + r.below(60), h: 10 + r.below(60) };
  const ppt = pick([1, 1.5, 2.5, 0.75]);
  return { spec: { blurPt, distPt, dirDeg, sx, sy, kxDeg, kyDeg, algn }, box, ppt };
}
put('fx/shadowAffine', dig((s) => {
  const r = new Rng(41);
  for (let i = 0; i < 2000; i++) {
    const { spec, box, ppt } = genSpec(r);
    affineOut(s, shadowAffine(spec, box, ppt));
  }
}));

const SILS = [
  ['blob1', () => genMask(40, 30, 1, 2001)],
  ['blob2', () => genMask(64, 48, 1, 2002)],
  ['star5', () => presetSil('star5', 48, 32, false)],
  ['heart', () => presetSil('heart', 33, 21, false)],
  ['binary', () => genMask(17, 9, 2, 2003)],
];
const RADS = [0, 0.004, 1, 3.7, 10, -5, NaN, Infinity];
const PPTS = [1, 2.5, 0.75];
for (const [name, mk] of SILS) {
  const sil = mk();
  put(`fx/outer/${name}`, dig((s) => {
    const r = new Rng(51);
    for (let i = 0; i < 40; i++) {
      const { spec, box, ppt } = genSpec(r);
      s.u32(i);
      writeMask(s, () => outerShadowMask(sil, box, spec, ppt));
    }
  }));
  put(`fx/inner/${name}`, dig((s) => {
    const r = new Rng(53);
    for (let i = 0; i < 40; i++) {
      const { spec, ppt } = genSpec(r);
      s.u32(i);
      writeMask(s, () => innerShadowMask(sil, { blurPt: spec.blurPt, distPt: spec.distPt, dirDeg: spec.dirDeg }, ppt));
    }
  }));
  put(`fx/glow/${name}`, dig((s) => {
    for (const rad of RADS) for (const ppt of PPTS) { s.u32(0); writeMask(s, () => glowMask(sil, rad, ppt)); }
  }));
  put(`fx/soft/${name}`, dig((s) => {
    for (const rad of RADS) for (const ppt of PPTS) { s.u32(0); writeMask(s, () => softEdgeMask(sil, rad, ppt)); }
  }));
}

put('fx/reflection', dig((s) => {
  const ts = [-1, 0, 0.1, 0.5, 0.9, 1, 1.5, NaN, Infinity];
  const pos = [0, 0.2, 0.5, 1, NaN, -0.1];
  const al = [0, 0.3, 1, -1, NaN];
  for (const t of ts) for (const stPos of pos) for (const endPos of pos) for (const stA of al) for (const endA of al) {
    f(s, reflectionAlpha(t, { stA, stPos, endA, endPos }));
  }
}));

cap.write(out);
console.log(`wrote ${out}: ${cap.records.size} records`);
