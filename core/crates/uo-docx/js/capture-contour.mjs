// Parity capture of the Word-only DrawingML code moved into uo-docx (uo-drawingml gate decision
// G3): `wrapContour` (shared/drawingml/src/contour.ts) and `SHAPE_STYLE_PRESETS`
// (shared/drawingml/src/shape-styles.ts). V8 against `uo_docx::drawing::{wrap_contour,
// shape_style_preset, find_shape_style}`:
//
//   UO_TS_ROOT=<checkout of main> node core/crates/uo-docx/js/capture-contour.mjs
//
// UO_TS_ROOT is a checkout of the frozen TS with `shared/drawingml/dist` built (the preset shapes
// come from dist; `contour.ts` and `shape-styles.ts` are imported from the sources through Node's
// type stripping, and the script checks that the dist build of `wrapContour` gives the same
// result on every case). Deterministic: the seed is fixed, the corpus is the committed
// `tests/parity/contour.inputs.txt` (written here) and the captures are
// `tests/parity/contour.capture` and `tests/parity/shape-styles.capture`.
//
// contour.inputs.txt: one case per line, `kind \t w \t h \t params… [\t trunc=N] [\t extra=N]`.
// The RGBA buffer of a case is built by a rule that both sides implement with integer arithmetic
// only (the facet `in` is the digest of the bytes, so a difference between the two generators is
// told apart from a difference in the contour):
//   rect x0 y0 x1 y1 a          alpha `a` inside the box [x0,x1) x [y0,y1)
//   ellipse x0 y0 x1 y1 a       inside the ellipse of the box (integer test on doubled coordinates)
//   ring x0 y0 x1 y1 k a        the ellipse minus the one of ratio k/8
//   tri x0 y0 x1 y1 x2 y2 a     a triangle (integer cross products, either winding)
//   rects a x0,y0,x1,y1;…       union of boxes
//   noise seed zeroPerMille     xorshift32 per pixel: alpha 0 with that probability, else 1..255
//   sparse seed n               n opaque pixels at xorshift32 positions
//   frame t a                   border of thickness t
//   checker c a                 c x c squares
//   full a                      one alpha everywhere
//   preset name ox oy           the preset shape (presetOutline, flattenPath, fillPolygons of the
//                               TS drawingml; alpha = round(coverage * 255)) inside the box
//                               inset by ox, oy
// RGB bytes are (5x + 3y + 11c) & 255. `trunc=N` cuts the buffer to N bytes after building (the TS
// reads the missing bytes as `undefined`, which is not 0: opaque; the Rust function refuses it and
// the replay compares the TS result with the Rust one on the buffer padded with 255), `extra=N`
// appends N bytes of 255 (ignored by both).
import { writeFileSync } from 'node:fs';
import { join, dirname } from 'node:path';
import { fileURLToPath } from 'node:url';
import { CaptureWriter, Stream, Rng, importFrom, tsRoot } from '../../uo-testkit/js/testkit.mjs';

const here = dirname(fileURLToPath(import.meta.url));
const root = tsRoot();
const { wrapContour } = await importFrom(root, 'shared/drawingml/src/contour.ts');
const { SHAPE_STYLE_PRESETS } = await importFrom(root, 'shared/drawingml/src/shape-styles.ts');
const dist = await importFrom(root, 'shared/drawingml/dist/index.js');
const { presetOutline, flattenPath, fillPolygons, newMask, PRESET_DEFS } = dist;

const rng = new Rng(0xc0470a);
const int = (n) => rng.below(n);
const pick = (a) => a[int(a.length)];
const chance = (n) => int(n) === 0;
const range = (lo, hi) => lo + int(hi - lo + 1);

function xorshift(seed) {
  let s = seed >>> 0 || 1;
  return () => { s ^= s << 13; s >>>= 0; s ^= s >>> 17; s ^= s << 5; s >>>= 0; return s; };
}

/** The integer rules of the header, 1:1 with `tests/parity_contour.rs`. */
function build(spec) {
  const [kind, ws, hs, ...p] = spec;
  const w = Number(ws), h = Number(hs);
  const n = w * h * 4;
  const buf = new Uint8Array(n);
  const set = (x, y, a) => {
    const i = (y * w + x) * 4;
    buf[i] = (x * 5 + y * 3) & 255; buf[i + 1] = (x * 5 + y * 3 + 11) & 255; buf[i + 2] = (x * 5 + y * 3 + 22) & 255; buf[i + 3] = a;
  };
  const num = (i) => Number(p[i]);
  const each = (f) => { for (let y = 0; y < h; y++) for (let x = 0; x < w; x++) set(x, y, f(x, y)); };
  switch (kind) {
    case 'rect': { const [x0, y0, x1, y1, a] = [0, 1, 2, 3, 4].map(num); each((x, y) => (x >= x0 && x < x1 && y >= y0 && y < y1 ? a : 0)); break; }
    case 'ellipse': {
      const [x0, y0, x1, y1, a] = [0, 1, 2, 3, 4].map(num);
      const A = x1 - x0, B = y1 - y0;
      each((x, y) => { const dx = 2 * x + 1 - (x0 + x1), dy = 2 * y + 1 - (y0 + y1); return dx * dx * B * B + dy * dy * A * A <= A * A * B * B ? a : 0; });
      break;
    }
    case 'ring': {
      const [x0, y0, x1, y1, k, a] = [0, 1, 2, 3, 4, 5].map(num);
      const A = x1 - x0, B = y1 - y0;
      each((x, y) => {
        const dx = 2 * x + 1 - (x0 + x1), dy = 2 * y + 1 - (y0 + y1);
        const v = dx * dx * B * B + dy * dy * A * A;
        return v <= A * A * B * B && v * 64 > k * k * A * A * B * B ? a : 0;
      });
      break;
    }
    case 'tri': {
      const [x0, y0, x1, y1, x2, y2, a] = [0, 1, 2, 3, 4, 5, 6].map(num);
      const cr = (ax, ay, bx, by, px, py) => (bx - ax) * (py - ay) - (by - ay) * (px - ax);
      each((x, y) => {
        const c0 = cr(x0, y0, x1, y1, x, y), c1 = cr(x1, y1, x2, y2, x, y), c2 = cr(x2, y2, x0, y0, x, y);
        return (c0 >= 0 && c1 >= 0 && c2 >= 0) || (c0 <= 0 && c1 <= 0 && c2 <= 0) ? a : 0;
      });
      break;
    }
    case 'rects': {
      const a = num(0);
      const boxes = p[1].split(';').map((b) => b.split(',').map(Number));
      each((x, y) => (boxes.some(([x0, y0, x1, y1]) => x >= x0 && x < x1 && y >= y0 && y < y1) ? a : 0));
      break;
    }
    case 'noise': {
      const next = xorshift(num(0)), zpm = num(1);
      for (let y = 0; y < h; y++) for (let x = 0; x < w; x++) {
        const r = next();
        const i = (y * w + x) * 4;
        buf[i] = (r >>> 8) & 255; buf[i + 1] = (r >>> 16) & 255; buf[i + 2] = (r >>> 24) & 255;
        buf[i + 3] = r % 1000 < zpm ? 0 : 1 + ((r >>> 10) % 255);
      }
      break;
    }
    case 'sparse': {
      const next = xorshift(num(0));
      each(() => 0);
      for (let i = 0; i < num(1); i++) { const x = next() % w, y = next() % h; set(x, y, 255); }
      break;
    }
    case 'frame': { const t = num(0), a = num(1); each((x, y) => (x < t || y < t || x >= w - t || y >= h - t ? a : 0)); break; }
    case 'checker': { const c = num(0), a = num(1); each((x, y) => ((Math.floor(x / c) + Math.floor(y / c)) & 1 ? a : 0)); break; }
    case 'full': { const a = num(0); each(() => a); break; }
    case 'preset': {
      const name = p[0], ox = num(1), oy = num(2);
      const bw = w - 2 * ox, bh = h - 2 * oy;
      each(() => 0);
      if (bw > 0 && bh > 0) {
        const o = presetOutline(name, bw, bh);
        const polys = flattenPath(o.subs, [1, 0, 0, 1, ox, oy]);
        const m = fillPolygons(newMask(w, h), polys);
        for (let i = 0; i < w * h; i++) buf[i * 4 + 3] = Math.round(m.a[i] * 255);
      }
      break;
    }
    default: throw new Error(`unknown kind ${kind}`);
  }
  return buf;
}

/** The buffer of a full spec line (with the `trunc=`/`extra=` modifiers). */
function bufferOf(fields) {
  const mods = fields.filter((f) => /^(trunc|extra)=/.test(f));
  const spec = fields.filter((f) => !/^(trunc|extra)=/.test(f));
  let buf = build(spec);
  for (const m of mods) {
    const [k, v] = m.split('=');
    if (k === 'trunc') buf = buf.subarray(0, Math.min(buf.length, Number(v)));
    else { const nb = new Uint8Array(buf.length + Number(v)); nb.set(buf); nb.fill(255, buf.length); buf = nb; }
  }
  return { buf, w: Number(spec[1]), h: Number(spec[2]), truncated: mods.some((m) => m.startsWith('trunc=')) };
}

// ───────────── the corpus ─────────────
const lines = [];
const add = (...f) => lines.push(f.map(String));

const SIZES = [1, 2, 3, 4, 5, 7, 8, 13, 31, 47, 48, 64, 95, 96, 97, 100, 127, 128, 191, 192, 193, 240, 300, 480];
const dims = () => {
  const r = int(10);
  if (r < 4) { const s = pick(SIZES); return [s, s]; }
  if (r < 7) return [pick(SIZES), pick(SIZES)];
  if (r < 8) return [range(1, 40), range(200, 700)];
  if (r < 9) return [range(200, 700), range(1, 40)];
  return [range(20, 400), range(20, 400)];
};
const ALPHAS = [255, 255, 255, 1, 128, 254];

// 1. every shape family over generated sizes and boxes
for (let i = 0; i < 900; i++) {
  const [w, h] = dims();
  const kind = pick(['rect', 'ellipse', 'ring', 'tri', 'rects', 'noise', 'sparse', 'frame', 'checker']);
  const a = pick(ALPHAS);
  const bx = () => { const x0 = range(0, w - 1), y0 = range(0, h - 1); return [x0, y0, range(x0, w), range(y0, h)]; };
  switch (kind) {
    case 'rect': add('rect', w, h, ...bx(), a); break;
    case 'ellipse': add('ellipse', w, h, ...bx(), a); break;
    case 'ring': add('ring', w, h, 0, 0, w, h, range(1, 7), a); break;
    case 'tri': add('tri', w, h, range(0, w), range(0, h), range(0, w), range(0, h), range(0, w), range(0, h), a); break;
    case 'rects': {
      const k = range(1, 4);
      add('rects', w, h, a, Array.from({ length: k }, () => bx().join(',')).join(';'));
      break;
    }
    case 'noise': add('noise', w, h, range(1, 0xffffff), pick([0, 100, 500, 900, 990, 999, 1000])); break;
    case 'sparse': add('sparse', w, h, range(1, 0xffffff), range(1, 12)); break;
    case 'frame': add('frame', w, h, range(1, Math.max(1, Math.min(w, h) >> 3)), a); break;
    default: add('checker', w, h, range(1, 40), a);
  }
}
// 2. the filled/empty decisions and the dilation reaching the whole grid: boxes inset by k cells
for (const [w, h] of [[96, 96], [192, 192], [480, 480], [96, 64], [64, 96], [200, 100], [100, 200], [97, 97], [95, 95], [480, 360]]) {
  for (let k = 0; k <= 7; k++) {
    const x0 = Math.floor(k * w / 96), y0 = Math.floor(k * h / 96);
    add('rect', w, h, x0, y0, w - x0, h - y0, 255);
    add('ellipse', w, h, x0, y0, w - x0, h - y0, 255);
    add('rect', w, h, x0 + 1, y0 + 1, w - x0, h - y0, 200);
  }
  add('full', w, h, 255); add('full', w, h, 0); add('full', w, h, 1);
  add('rect', w, h, 0, 0, 1, 1, 255); add('rect', w, h, w - 1, h - 1, w, h, 255);
  add('rect', w, h, Math.floor(w / 2), 0, Math.floor(w / 2) + 1, h, 255);
  add('rect', w, h, 0, Math.floor(h / 2), w, Math.floor(h / 2) + 1, 255);
  add('rects', w, h, 255, `0,0,${w},${Math.max(1, h >> 3)};0,0,${Math.max(1, w >> 3)},${h}`);
  add('rects', w, h, 255, `0,0,${Math.max(1, w >> 2)},${Math.max(1, h >> 2)};${w - Math.max(1, w >> 2)},${h - Math.max(1, h >> 2)},${w},${h}`);
}
// 3. the cases of the TS test (circle 240 with the 96-cell grid, the L with straight corners)
add('ellipse', 240, 240, 24, 24, 216, 216, 255);
add('rects', 240, 240, 255, '24,24,96,216;24,144,216,216');
add('ring', 240, 240, 0, 0, 240, 240, 5, 255);
// 4. hostile sizes
for (const [w, h] of [[0, 0], [0, 5], [5, 0], [0, 100000], [100000, 0]]) { add('full', w, h, 255); add('rect', w, h, 0, 0, 1, 1, 255); }
for (const [w, h] of [[1, 1], [1, 2], [2, 1], [1, 100000], [100000, 1], [3, 99999], [99999, 3], [7, 20000], [20000, 7], [1, 97], [97, 1], [96, 1], [1, 96]]) {
  add('full', w, h, 255);
  add('rect', w, h, 0, 0, Math.max(1, w >> 1), Math.max(1, h >> 1), 255);
  add('noise', w, h, range(1, 0xffffff), 500);
  add('sparse', w, h, range(1, 0xffffff), 4);
  add('frame', w, h, 1, 255);
}
add('ellipse', 1500, 1500, 100, 100, 1400, 1400, 255);
add('rects', 2000, 1200, 255, '10,10,400,1190;1500,3,1990,900');
// 5. buffers: truncated (missing bytes read as undefined: opaque in the TS) and longer than needed
for (let i = 0; i < 80; i++) {
  const [w, h] = [range(1, 120), range(1, 120)];
  const base = pick([['rect', w, h, 0, 0, Math.max(1, w >> 1), Math.max(1, h >> 1), 255], ['noise', w, h, range(1, 0xffffff), 700], ['ellipse', w, h, 0, 0, w, h, 255]]);
  const total = w * h * 4;
  const cut = pick([0, 1, 3, 4, total - 1, total - 4, total >> 1, range(0, total - 1), (total >> 2) * 3 + 3]);
  add(...base, `trunc=${Math.max(0, Math.min(total - 1, cut))}`);
}
for (let i = 0; i < 20; i++) { const [w, h] = [range(1, 100), range(1, 100)]; add('ellipse', w, h, 0, 0, w, h, 255, `extra=${pick([1, 3, 4, 17, 1000])}`); add('full', w, h, 255, `extra=${range(1, 9)}`); }
// 6. every preset shape at five sizes, with and without a margin
const names = Object.keys(PRESET_DEFS);
const PS = [[96, 96, 0, 0], [240, 240, 12, 12], [320, 180, 4, 9], [100, 300, 3, 3], [480, 360, 30, 20]];
for (const name of names) for (const [w, h, ox, oy] of PS) add('preset', w, h, name, ox, oy);
for (const name of ['', 'rect', 'nope', 'ellipse']) add('preset', 200, 200, name, 5, 5);

// ───────────── contour.inputs.txt and the facets ─────────────
const file = join(here, '..', 'tests', 'parity', 'contour.inputs.txt');
writeFileSync(file, lines.map((l) => l.join('\t')).join('\n') + '\n');

const cap = new CaptureWriter(['contour facet of the Word-only drawingml code moved into uo-docx (uo-drawingml G3): digests of the TS wrapContour result over the generated cases of tests/parity/contour.inputs.txt, see js/capture-contour.mjs']);
let nulls = 0, polys = 0, vertices = 0, maxVertices = 0, distChecked = 0;
lines.forEach((fields, i) => {
  const { buf, w, h } = bufferOf(fields);
  const key = String(i);
  const s0 = new Stream();
  s0.digestOf(buf);
  cap.set(key, 'in', s0.finish().short);
  const r = wrapContour(buf, w, h);
  const d = dist.wrapContour(buf, w, h);
  if (JSON.stringify(r) !== JSON.stringify(d)) throw new Error(`dist build differs from the source on case ${i}`);
  distChecked++;
  if (r === null) { nulls++; cap.set(key, 'out', 'null'); cap.set(key, 'len', '0'); return; }
  const s = new Stream();
  s.u64(r.length);
  for (const [x, y] of r) {
    if (!Number.isInteger(x) || !Number.isInteger(y) || x < 0 || y < 0 || x > 21600 || y > 21600 || Object.is(x, -0) || Object.is(y, -0)) throw new Error(`case ${i}: a vertex is not an integer in 0..21600`);
    s.f64(x).f64(y);
  }
  polys++; vertices += r.length; maxVertices = Math.max(maxVertices, r.length);
  cap.set(key, 'out', s.finish().short);
  cap.set(key, 'len', String(r.length));
});
cap.meta('corpus', `cases=${lines.length} null=${nulls} polygons=${polys} vertices=${vertices} maxVertices=${maxVertices} presets=${names.length}`);
cap.write(join(here, '..', 'tests', 'parity', 'contour.capture'));
console.log(`contour: ${lines.length} cases, ${nulls} null, ${polys} polygons, ${vertices} vertices (max ${maxVertices}), dist build identical on ${distChecked}`);

// ───────────── shape styles ─────────────
// inputs: `n <hex of the f64 bits>` (the gallery number: `SHAPE_STYLE_PRESETS[n - 1]`), `find i v`
// (the comparison loop of `shapeStyleIndex` over the pair built from entry i by variant v: 0 the
// entry itself, 1 spPr and style swapped, 2 spPr of i with the style of i + 1, 3 the spPr of
// i + 1 with the style of i, 4 spPr plus a space, 5 `<wps:style >` for `<wps:style>`) and
// `findx <hex of utf8 spPr> <hex of utf8 style>` (explicit pairs; `-` is the empty string)
const f64bits = new DataView(new ArrayBuffer(8));
const bitsOf = (x) => { f64bits.setFloat64(0, x); return f64bits.getBigUint64(0).toString(16).padStart(16, '0'); };
const hex = (s) => Buffer.from(s, 'utf8').toString('hex') || '-';
const ssLines = [];
const NS = [NaN, Infinity, -Infinity, 0, -0, 1, 2, 42, 43, 76, 77, 78, 79, 100, 1.5, 0.5, 1.0000000000000002, 76.99999999999999, 77.00000000000001, 4294967296, 4294967297, 2 ** 53, -1, -77, 1e300, -1e300, 5e-324, 2 ** 31, 2 ** 32 + 1];
for (let i = 1; i <= 80; i++) NS.push(i);
for (const x of NS) ssLines.push(`n\t${bitsOf(x)}`);
for (let i = 0; i < 200; i++) ssLines.push(`n\t${bitsOf(rng.below(100000) / 1000)}`);
const T = SHAPE_STYLE_PRESETS;
const variant = (i, v) => {
  const [p, s] = T[i], [p1, s1] = T[(i + 1) % T.length];
  return [[p, s], [s, p], [p, s1], [p1, s], [p + ' ', s], [p, s.replace('<wps:style>', '<wps:style >')]][v];
};
for (let i = 0; i < T.length; i++) for (let v = 0; v < 6; v++) ssLines.push(`find\t${i}\t${v}`);
ssLines.push(`findx\t-\t-`, `findx\t${hex('<a:ln/>')}\t-`, `findx\t-\t${hex(T[0][1])}`, `findx\t${hex('<a:ln/>')}\t${hex(T[0][1])}`, `findx\t${hex('<a:ln/>')}\t${hex(T[1][1])}`);
writeFileSync(join(here, '..', 'tests', 'parity', 'shape-styles.inputs.txt'), ssLines.join('\n') + '\n');

const un = (h) => (h === '-' ? '' : Buffer.from(h, 'hex').toString('utf8'));
const scap = new CaptureWriter(['shape styles facet of the Word-only drawingml code moved into uo-docx (uo-drawingml G3): the table and the lookups of the TS SHAPE_STYLE_PRESETS, see js/capture-contour.mjs']);
{
  const s = new Stream();
  s.u64(T.length);
  for (const [p, st] of T) s.str(p).str(st);
  scap.set('table', 'digest', s.finish().short);
  scap.set('table', 'len', String(T.length));
}
let hits = 0, finds = 0;
ssLines.forEach((line, i) => {
  const f = line.split('\t');
  const key = String(i);
  if (f[0] === 'n') {
    f64bits.setBigUint64(0, BigInt(`0x${f[1]}`));
    const n = f64bits.getFloat64(0);
    const e = T[n - 1];
    if (e === undefined) return void scap.set(key, 'pick', 'undef');
    const s = new Stream();
    s.str(e[0]).str(e[1]);
    scap.set(key, 'pick', s.finish().short);
    hits++;
  } else {
    const [p, st] = f[0] === 'find' ? variant(Number(f[1]), Number(f[2])) : [un(f[1]), un(f[2])];
    let found = 0;
    for (let k = 0; k < T.length; k++) { const [a, b] = T[k]; if (b === st && a === p) { found = k + 1; break; } }
    scap.set(key, 'find', String(found));
    if (found) finds++;
  }
});
scap.meta('corpus', `lines=${ssLines.length} picked=${hits} found=${finds}`);
scap.write(join(here, '..', 'tests', 'parity', 'shape-styles.capture'));
console.log(`shape styles: ${ssLines.length} lines, ${hits} picked, ${finds} found`);
