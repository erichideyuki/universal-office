// Parity capture of frontier A of uo-drawingml (stage 3, phase 2): the frozen TS colour
// modifiers (`applyColorMods`, `srgbToLin`, `linToSrgb`), preset patterns (`PRESET_PATTERNS`,
// `patternRows`, `patternDensity`), image adjustments (`adjustSaturation`, `adjustTemperature`,
// `adjustBrightnessContrast`, `adjustSharpen`) and Word blip effects (`effectLine`,
// `applyImageEffects`, `effectDecode`, `applyPictureAdjust`). Writes
// tests/captures/frontier-a.capture, replayed by tests/frontier_a_parity.rs.
//
//   node tools/capture-frontier-a.mjs --ts-root <checkout of main>     (or UO_TS_ROOT)
//
// The pow facets (the sRGB curve of shade/tint, the saturation curve) are references only from
// Node 24.15.0 linux-x64 with the FMA variant of glibc's pow masked (the pow decision of the
// uo-jsnum gate), so the committed file is taken in WSL:
//
//   GLIBC_TUNABLES=glibc.cpu.hwcaps=-FMA,-AVX2 node tools/capture-frontier-a.mjs --ts-root <...>
//
// Options: --out <file>  write elsewhere.
//
// Corpus (mirrored in tests/frontier_a_parity.rs; the generators use the xorshift32 `Rng` of the
// testkit and exact arithmetic only, so both sides build the same inputs):
//  - colour: 20,000 sequences of 0..6 modifiers (the 16 kinds, unknown names, raw values undefined,
//    NaN, +-Infinity, +-0, huge, negative, fractional and random) over random, palette, grey and
//    out-of-range colours; srgbToLin/linToSrgb over a grid, ulp neighbours of the knees, edge
//    values, 20,000 uniform values and 20,000 random bit patterns;
//  - patterns: every name in key order, unknown names, density;
//  - adjust: 60 generated images (gradient, noise, flat, edges, smooth; 1x1 up to 300x200, several
//    narrower than the blur kernel) x the saturation, temperature and sharpen/soften value lists, and
//    the 25x25 brightness x contrast grid on an image holding all 256 levels per channel; every call
//    runs on a Uint8Array and on a Uint8ClampedArray and the script fails when they differ;
//  - blip: effectLine and effectDecode grids, applyImageEffects over 2,304 effect sets x 3 images,
//    applyPictureAdjust over 11 alphas x 12 duotones x 12 colour changes x 3 images (hex strings
//    valid, short, signed, junk, with a 0x prefix, non-ASCII and split surrogate pairs).
//    applyPictureAdjust runs on a Uint8ClampedArray (the canvas path; Rust clamps, G7). The
//    Uint8Array run (the PDF path wraps) is compared too: where no store leaves 0..255 it must
//    be equal (the script fails otherwise), and the number of combinations where it differs is
//    written in the metadata (those are the accepted differences of G7).
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { CaptureWriter, Rng, Stream, importFrom, tsRoot } from '../../uo-testkit/js/testkit.mjs';

const here = dirname(fileURLToPath(import.meta.url));
const argv = process.argv.slice(2);
const outIdx = argv.indexOf('--out');
const out = outIdx >= 0 ? argv[outIdx + 1] : join(here, '..', 'tests', 'captures', 'frontier-a.capture');
const root = tsRoot(argv);
const dml = await importFrom(root, 'shared/drawingml/dist/index.js');
const imgs = await importFrom(root, 'write/packages/docx-layout/dist/images.js');

const canon = (v) => (v !== v ? NaN : v);
const f = (s, v) => s.f64(canon(v));
const summary = (s) => { const n = s.items; return `${n}:${s.finish().short}`; };
const rngF = (rng) => rng.next() / 4294967296;
const bytesOf = (a) => new Uint8Array(a.buffer, a.byteOffset, a.byteLength);

const cap = new CaptureWriter([
  'uo-drawingml parity capture of frontier A (colour, patterns, adjust, blip): written by core/crates/uo-drawingml/tools/capture-frontier-a.mjs, replayed by tests/frontier_a_parity.rs.',
  'color/mods.b<k>: 500 modifier sequences each; color/lin.b<k>: srgbToLin and linToSrgb over a block of inputs; patterns/*; adjust/<fn>.i<k>: all value sets on generated image k (adjust/bc.b<k>: bright value k x 25 contrasts);',
  'blip/line, blip/decode; blip/effects.i<k> and blip/adjust.i<k>: every parameter set on image k.',
]);
cap.meta('ts-dist', 'shared/drawingml/dist and write/packages/docx-layout/dist/images.js (frozen main, 0.9.x)');

// ── colour ──────────────────────────────────────────────────────────────────────────────────────

const CH_PALETTE = [0, 1, 0.5, 0.25, 0.75, 0.04045, 0.0031308, 0.0031309, 0.04046, 0.999999, 1e-9, 0.2126, 0.7152, 0.0722, 2 / 3, 1 / 3, 1 / 6];
const CH_EDGE = [-0, -0.25, 1.5, 2, NaN, Infinity, -Infinity, 1e308, 5e-324];
const MOD_NAMES = ['gamma', 'invGamma', 'comp', 'inv', 'gray', 'shade', 'tint', 'lumMod', 'lumOff', 'lum', 'satMod', 'satOff', 'sat', 'hueMod', 'hueOff', 'hue', 'red', 'blueMod', 'bogus', ''];

function genChannel(rng, prev) {
  const k = rng.below(20);
  if (k < 10) return rngF(rng);
  if (k < 16) return CH_PALETTE[rng.below(CH_PALETTE.length)];
  if (k < 18) return prev;
  if (k === 18) return CH_EDGE[rng.below(CH_EDGE.length)];
  return 1 - rngF(rng);
}

function genRaw(rng) {
  const m = rng.below(24);
  switch (m) {
    case 0: return undefined;
    case 1: return NaN;
    case 2: return Infinity;
    case 3: return -Infinity;
    case 4: return 0;
    case 5: return -0;
    case 6: return 100000;
    case 7: return 50000;
    case 8: return 75000;
    case 9: return 25000;
    case 10: return 200000;
    case 11: return -50000;
    case 12: return 1e9;
    case 13: return 1e-9;
    case 14: return 12345.678;
    case 15: return 21600000;
    case 16: return 5400000;
    case 17: return 1e308;
    case 18: case 19: case 20: return rng.below(300001);
    default: return (rngF(rng) - 0.5) * 400000;
  }
}

function genSequence(i) {
  const rng = new Rng(0xc0100000 + i);
  const c0 = genChannel(rng, 0.5);
  const c1 = genChannel(rng, c0);
  const c2 = genChannel(rng, c1);
  const n = rng.below(7);
  const mods = [];
  for (let k = 0; k < n; k++) {
    const name = MOD_NAMES[rng.below(MOD_NAMES.length)];
    mods.push([name, genRaw(rng)]);
  }
  return { c: [c0, c1, c2], mods };
}

const MOD_SEQUENCES = 20000, MOD_BLOCK = 500;
for (let b = 0; b < MOD_SEQUENCES / MOD_BLOCK; b++) {
  const s = new Stream();
  for (let i = b * MOD_BLOCK; i < (b + 1) * MOD_BLOCK; i++) {
    const { c, mods } = genSequence(i);
    const r = dml.applyColorMods(c, mods);
    for (const v of r) f(s, v);
  }
  cap.set('color/mods', `b${b}`, summary(s));
}

const f64buf = new Float64Array(1), u32buf = new Uint32Array(f64buf.buffer);
function fromBits(hi, lo) { u32buf[0] = lo; u32buf[1] = hi; return f64buf[0]; }
function toBits(v) { f64buf[0] = v; return [u32buf[1], u32buf[0]]; }
function ulpStep(v, k) {
  const [hi, lo] = toBits(v);
  let big = (BigInt(hi) << 32n) | BigInt(lo);
  big += BigInt(k);
  return fromBits(Number(big >> 32n), Number(big & 0xffffffffn));
}

function linInputs() {
  const v = [];
  for (let i = 0; i <= 1000; i++) v.push(i / 1000);
  for (const base of [0.04045, 0.0031308]) for (let k = -4; k <= 4; k++) v.push(ulpStep(base, k));
  v.push(0, -0, 1, NaN, Infinity, -Infinity, -1, 2, 1e-320, 1e308, 5e-324, 0.5);
  const rng = new Rng(0x11111111);
  for (let i = 0; i < 20000; i++) v.push(rngF(rng));
  const rb = new Rng(0x22222222);
  for (let i = 0; i < 20000; i++) { const hi = rb.next(); const lo = rb.next(); v.push(fromBits(hi, lo)); }
  return v;
}
{
  const inputs = linInputs();
  const BLOCK = 5000;
  for (let b = 0; b * BLOCK < inputs.length; b++) {
    const s = new Stream();
    for (const x of inputs.slice(b * BLOCK, (b + 1) * BLOCK)) { f(s, dml.srgbToLin(x)); f(s, dml.linToSrgb(x)); }
    cap.set('color/lin', `b${b}`, summary(s));
  }
}

// ── patterns ────────────────────────────────────────────────────────────────────────────────────

{
  const s = new Stream();
  for (const name of Object.keys(dml.PRESET_PATTERNS)) {
    s.str(name);
    for (const b of dml.patternRows(name)) s.u32(b);
    f(s, dml.patternDensity(name));
    s.str(dml.PRESET_PATTERNS[name]);
  }
  cap.set('patterns', 'all', summary(s));
  const k = new Stream();
  for (const name of Object.keys(dml.PRESET_PATTERNS)) k.str(name);
  cap.set('patterns', 'keys', summary(k));
  const u = new Stream();
  for (const name of ['', 'Pct5', ' pct5', 'pct5 ', 'nope', 'zigZagX', 'PCT5', 'pct', 'horz\u0000', '5pct']) {
    const rows = dml.patternRows(name);
    if (rows === undefined) u.tag('n'); else { u.tag('r'); for (const b of rows) u.u32(b); }
    f(u, dml.patternDensity(name));
  }
  cap.set('patterns', 'unknown', summary(u));
  const proto = [];
  for (const name of ['constructor', '__proto__', 'toString', 'hasOwnProperty', 'valueOf']) {
    let r;
    try { r = String(dml.patternRows(name)); } catch (e) { r = `throws ${e.constructor.name}`; }
    proto.push(`${name}:${r.replace(/\s+/g, '_')}`);
  }
  cap.meta('patterns-prototype-names', proto.join(','));
}

// ── adjust ──────────────────────────────────────────────────────────────────────────────────────

function genImage(kind, w, h, seed) {
  const rng = new Rng(seed);
  const d = new Uint8Array(w * h * 4);
  const wm = Math.max(1, w - 1), hm = Math.max(1, h - 1);
  const c0 = [rng.below(256), rng.below(256), rng.below(256)];
  for (let y = 0; y < h; y++) {
    for (let x = 0; x < w; x++) {
      const i = (y * w + x) * 4;
      if (kind === 0) {
        d[i] = Math.floor((x * 255) / wm); d[i + 1] = Math.floor((y * 255) / hm); d[i + 2] = (x * 37 + y * 91) & 255; d[i + 3] = 255;
      } else if (kind === 1) {
        for (let c = 0; c < 4; c++) d[i + c] = rng.below(256);
      } else if (kind === 2) {
        if (seed & 1) { d[i] = c0[0]; d[i + 1] = c0[1]; d[i + 2] = c0[2]; } else { d[i] = c0[0]; d[i + 1] = c0[0]; d[i + 2] = c0[0]; }
        d[i + 3] = 255;
      } else if (kind === 3) {
        const col = y % 5 === 4 ? [250, 250, 250] : x * 2 < w ? [200, 30, 30] : [30, 180, 40];
        d[i] = col[0]; d[i + 1] = col[1]; d[i + 2] = col[2]; d[i + 3] = 200;
      } else {
        const v = (x * x + y * 3 + seed) % 256;
        d[i] = v; d[i + 1] = (v * 5 + x) & 255; d[i + 2] = (255 - v + y) & 255; d[i + 3] = 255 - ((x + y) & 7);
      }
    }
  }
  return d;
}

const IMG_SIZES = [[1, 1], [2, 2], [2, 3], [3, 2], [5, 5], [7, 3], [9, 9], [16, 16], [33, 17], [64, 48], [130, 70]];
function imageList() {
  const list = [];
  let n = 0;
  for (const [w, h] of IMG_SIZES) for (let kind = 0; kind < 5; kind++) list.push({ kind, w, h, seed: 0xad000000 + n++ });
  for (const kind of [1, 3]) list.push({ kind, w: 300, h: 200, seed: 0xad000000 + n++ });
  return list;
}
// every level in every channel (a permutation each), alpha = level
function levelsImage() {
  const d = new Uint8Array(256 * 4);
  for (let v = 0; v < 256; v++) { d[v * 4] = v; d[v * 4 + 1] = 255 - v; d[v * 4 + 2] = (v * 37) & 255; d[v * 4 + 3] = v; }
  return d;
}

const SAT = [0, 0.25, 0.5, 0.75, 0.9999995, 1, 1.0000001, 1.5, 2, 3, 5, 1e9, -1, NaN, Infinity, -Infinity, 1e-7];
const TEMP = [3000, 4700, 5000, 5300, 5900, 6000, 6499.5, 6500, 6501, 7000, 7200, 8000, 8800, 10000, 11200, 20000, 500, 1000, 0, -5, NaN, Infinity, -Infinity];
const SHARP = [0, -0, NaN, 0.01, 0.1, 0.25, 0.5, 0.75, 1, 1.7, -0.01, -0.1, -0.5, -1, -3, Infinity, -Infinity, 1e-300, -1e-300, 1e-8, 0.3333];
const BC = [-1.5, -1, -0.9, -0.8, -0.75, -0.5, -0.3, -0.25, -0.1, -5e-7, 0, -0, 0.05, 0.1, 0.15, 0.25, 0.5, 0.7, 0.8, 0.9, 1, 1.5, NaN, Infinity, -Infinity];

let kindChecks = 0;
// runs fn on a Uint8Array and on a Uint8ClampedArray copy of `src`; both must give the same bytes
function runBoth(src, fn) {
  const a = Uint8Array.from(src), b = Uint8ClampedArray.from(src);
  fn(a); fn(b);
  const bb = bytesOf(b);
  for (let i = 0; i < a.length; i++) if (a[i] !== bb[i]) throw new Error(`Uint8Array and Uint8ClampedArray differ at ${i}`);
  kindChecks++;
  return a;
}

{
  const images = imageList();
  images.forEach(({ kind, w, h, seed }, k) => {
    const src = genImage(kind, w, h, seed);
    const sat = new Stream(), temp = new Stream(), sharp = new Stream();
    for (const v of SAT) sat.bytes(runBoth(src, (d) => dml.adjustSaturation(d, v)));
    for (const v of TEMP) temp.bytes(runBoth(src, (d) => dml.adjustTemperature(d, v)));
    for (const v of SHARP) sharp.bytes(runBoth(src, (d) => dml.adjustSharpen(d, w, h, v)));
    cap.set('adjust/sat', `i${k}`, summary(sat));
    cap.set('adjust/temp', `i${k}`, summary(temp));
    cap.set('adjust/sharp', `i${k}`, summary(sharp));
  });
  cap.meta('adjust-images', String(images.length));
  const lv = levelsImage();
  BC.forEach((bright, bi) => {
    const s = new Stream();
    for (const contrast of BC) s.bytes(runBoth(lv, (d) => dml.adjustBrightnessContrast(d, bright, contrast)));
    cap.set('adjust/bc', `b${bi}`, summary(s));
  });
}

// ── blip ────────────────────────────────────────────────────────────────────────────────────────

const GAINS = [undefined, 0, 1, 1.5, -1, NaN, Infinity, 0.3];
const BRIGHTS = [undefined, 0, -0, NaN, 0.1, -0.3, 1, -1, 1e9];
const BILEVELS = [undefined, 0, 0.3, 0.5, 1, NaN, -1, 2];
const defined = (o) => { for (const k of Object.keys(o)) if (o[k] === undefined) delete o[k]; return o; };

{
  const s = new Stream();
  for (const gain of GAINS) for (const bright of BRIGHTS) {
    const r = imgs.effectLine(defined({ gain, bright }));
    if (r === null) s.tag('n'); else { s.tag('l'); f(s, r.slope); f(s, r.off); }
  }
  cap.set('blip/line', 'v', summary(s));
  const d = new Stream();
  for (const components of [1, 3, 4, 2, 0]) for (const gray of [false, true]) for (const biLevel of [undefined, 0.5]) for (const gain of GAINS) for (const bright of BRIGHTS) {
    const r = imgs.effectDecode(defined({ gray, gain, bright, biLevel }), components);
    if (r === null) d.tag('n'); else { d.tag('d'); d.u32(r.length); for (const v of r) f(d, v); }
  }
  cap.set('blip/decode', 'v', summary(d));
}

function blipImages() {
  return [levelsImage(), genImage(1, 32, 32, 0xb1000001), genImage(0, 17, 13, 0xb1000002)];
}

{
  blipImages().forEach((src, k) => {
    const s = new Stream();
    for (const gray of [false, true]) for (const floor of [false, true]) for (const gain of GAINS) for (const bright of BRIGHTS) for (const biLevel of BILEVELS) {
      const a = Uint8Array.from(src);
      imgs.applyImageEffects(a, defined({ gray, gain, bright, floor, biLevel }));
      s.bytes(a);
    }
    cap.set('blip/effects', `i${k}`, summary(s));
  });
}

const ALPHAS = [undefined, 0, 0.5, 1, 1 / 3, 1.5, -0.5, NaN, 2, 1e9, 0.002];
const DUOTONES = [
  undefined, ['000000', 'ffffff'], ['ff0000', '0000ff'], ['12', 'abcdef'], ['-f0000', 'zz'], ['0x1a2b3c', 'gg1122'], ['  ', 'ÄÖ12'],
  ['\u{1F600}ff00ff', '00ff00'], ['ffff', 'ff'], ['FFaa00', '+f+f+f'], ['00ff00', ' f f f'], ['', ''],
];
const CLRCHANGES = [
  undefined, { from: 'ff0000', to: '00ff00', toAlpha: 0 }, { from: '808080', to: 'ffffff', toAlpha: 1 }, { from: '000000', to: '-f0000', toAlpha: 0.5 },
  { from: 'FfFfFf', to: '123456', toAlpha: 0.25 }, { from: 'zz', to: '00ff00', toAlpha: 0 }, { from: 'ff0000', to: 'zz', toAlpha: 1 },
  { from: '808080', to: '0x0000', toAlpha: 1.5 }, { from: '808080', to: 'f0f0f0', toAlpha: -1 }, { from: '000000', to: '+f+f+f', toAlpha: NaN },
  { from: 'ffffff', to: '\u{1F600}\u{1F600}', toAlpha: 0.7 }, { from: '', to: '', toAlpha: 1 },
];
const hexPair = (s, i) => parseInt(s.slice(i, i + 2), 16);
function adjustInRange(o) {
  if (o.alpha !== undefined && !(o.alpha >= 0 && o.alpha <= 1)) return false;
  if (o.clrChange) {
    const c = o.clrChange;
    if (!(c.toAlpha >= 0 && c.toAlpha <= 1)) return false;
    for (const i of [0, 2, 4]) { const v = hexPair(c.to, i); if (v === v && !(v >= 0 && v <= 255)) return false; }
  }
  if (o.duotone) {
    for (const col of o.duotone) for (const i of [0, 2, 4]) { const v = hexPair(col, i); if (v === v && !(v >= 0 && v <= 255)) return false; }
  }
  return true;
}
// pixels whose channels sit within +-3 of the colours the colour changes look for
function pictureImage() {
  const rng = new Rng(0xb2000001);
  const near = [[255, 0, 0], [128, 128, 128], [0, 0, 0], [255, 255, 255], [18, 52, 86]];
  const d = new Uint8Array(16 * 16 * 4);
  for (let p = 0; p < 256; p++) {
    if (rng.below(2) === 0) {
      const c = near[rng.below(near.length)];
      for (let k = 0; k < 3; k++) d[p * 4 + k] = Math.max(0, Math.min(255, c[k] + rng.below(7) - 3));
    } else {
      for (let k = 0; k < 3; k++) d[p * 4 + k] = rng.below(256);
    }
    d[p * 4 + 3] = rng.below(256);
  }
  return d;
}

{
  let combos = 0, differs = 0, inRangeChecked = 0;
  const sources = [pictureImage(), genImage(1, 16, 16, 0xb1000003), levelsImage()];
  sources.forEach((src, k) => {
    const s = new Stream();
    for (const alpha of ALPHAS) for (const duotone of DUOTONES) for (const clrChange of CLRCHANGES) {
      const o = defined({ alpha, duotone, clrChange });
      const clamped = new Uint8ClampedArray(src), wrapped = new Uint8Array(src);
      imgs.applyPictureAdjust(clamped, o);
      imgs.applyPictureAdjust(wrapped, o);
      const cb = bytesOf(clamped);
      let same = true;
      for (let i = 0; i < cb.length; i++) if (cb[i] !== wrapped[i]) { same = false; break; }
      combos++;
      if (!same) differs++;
      if (adjustInRange(o)) { inRangeChecked++; if (!same) throw new Error(`in-range combination differs between Uint8Array and Uint8ClampedArray: ${JSON.stringify(o)}`); }
      s.bytes(cb);
    }
    cap.set('blip/adjust', `i${k}`, summary(s));
  });
  cap.meta('blip-adjust-combinations', String(combos));
  cap.meta('blip-adjust-uint8array-differs', `${differs} (G7: Rust clamps what the PDF path wraps; ${inRangeChecked} in-range combinations were equal)`);
}
cap.meta('uint8-kind-checks', `${kindChecks} adjust calls identical on Uint8Array and Uint8ClampedArray`);

cap.write(out);
console.log(`${out}: ${cap.records.size} records`);
