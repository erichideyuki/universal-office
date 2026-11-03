// Parity capture of the codecs of uo-image: the frozen TS `jpegInfo`, `imagePixelSize`, `decodePng`,
// `decodeGif`, `decodeBmp`, `decodeTiff` and `decodeRaster` of <ts-root>/write/packages/docx-layout/dist/images.js
// over the corpus below. Writes tests/captures/image.capture (digests of the decoded pixel buffers and
// the header fields) and tests/corpus/generated.pack (the generated images, so that the Rust replay
// reads the very same bytes), both committed. tests/image_parity.rs replays them.
//
//   node tools/capture-image.mjs --ts-root <checkout of main>     (or UO_TS_ROOT; run from anywhere)
//
// Options: --out <file>   capture file elsewhere (comparisons between platforms)
//          --pack <file>  pack file elsewhere
//
// Corpus (every image is reproducible from this file, seeded with the xorshift32 of uo-testkit):
//  - oracle: every distinct image (by SHA-256) of the git-tracked OOXML packages (docx, xlsx, pptx and
//    templates: the 2,742 oracle documents of docx-layout among them; entries under /media/ or with an
//    image extension) and every tracked image file, up to 4 MB;
//  - gen/png: every colour type x every depth of the specification x Adam7 x sizes x tRNS (a key, a
//    partial table, none) x PLTE (full, short, missing) x a zlib stream cut short, plus the depths and
//    colour types the specification forbids (3, 5, 7, 12, 24, 32, 255 bits; types 1, 5, 7) and files with
//    split IDAT chunks, unknown chunks, no IEND; the scanline bytes are random (the filter bytes too: a
//    decoder cannot tell filtered from unfiltered data);
//  - gen/gif: LZW-encoded frames (every minimum code size 1-8 and 11, interlace, transparency, local and
//    global tables, a frame offset and a frame larger than the screen), and streams of random codes;
//  - gen/bmp: 1, 4, 8, 16, 24, 32 bpp, bottom-up and top-down, BITFIELDS (565, 555, 8888 with alpha, a
//    hostile mask), V3/V4/V5 headers, truncated pixel data;
//  - gen/tiff: little and big endian; gray (both photometrics), RGB, RGBA, 16 bit, 1/2/4 bit, palette,
//    CMYK, YCbCr (no subsampling and 2x2, 2x1, 1x2, 4x4); strips and tiles; none, LZW, PackBits and
//    deflate; the horizontal predictor at 8 and 16 bits;
//  - gen/jpeg: header-only streams (markers, fill bytes, restart markers, Adobe, cut-off SOF) and the
//    real JPEGs of the oracle;
//  - mut/...: five deterministic mutations (truncation, bit flips in the header, bit flips anywhere,
//    random bytes, hostile 4-byte fields in the header) of every image up to 200 KB and 128 Kpx. The mutator is
//    mirrored in tests/image_parity.rs.
// Per image: `in` (digest of the bytes), `jpeg`, `size`, `png`, `gif`, `bmp`, `tiff` (`decodeRaster` is the `??` chain of the
// last four: the replay derives it from them). A decoder
// result is `N` (null), `E` (the TS threw, or tried to allocate more than 64 MiB, 4 MiB for the mutations: the replay
// uses the same thresholds as limits) or `K:<w>x<h>:<digest of the RGBA bytes>`.
//
// The codecs have no floating-point library calls beyond Math.round and division: identical on every
// platform; the `@platform` line is informative.
import { execSync } from 'node:child_process';
import { createHash } from 'node:crypto';
import { readFileSync, writeFileSync } from 'node:fs';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';
import zlib from 'node:zlib';
import { CaptureWriter, Rng, importFrom, relPath, tsRoot } from '../../uo-testkit/js/testkit.mjs';

const here = dirname(fileURLToPath(import.meta.url));
const argv = process.argv.slice(2);
const opt = (name, dflt) => { const i = argv.indexOf(name); return i >= 0 ? argv[i + 1] : dflt; };
const outPath = opt('--out', join(here, '..', 'tests', 'captures', 'image.capture'));
const packPath = opt('--pack', join(here, '..', 'tests', 'corpus', 'generated.pack'));
const repo = resolve(here, '..', '..', '..', '..');
const root = tsRoot(argv);

// ── the TS under test, with its allocations limited like the Rust limits of the replay ───────────
// the allocation limit of the replayed TS: the oracle and generated images may use 64 MiB (16 Mpx), the mutations 4 MiB (1 Mpx;
// a mutated header that asks for more is mostly a test of the allocator and takes seconds in a debug build)
const LIMIT = 64 * 1024 * 1024;
const MUT_LIMIT = 4 * 1024 * 1024;
let limit = LIMIT;
const OrigU8 = Uint8Array;
globalThis.Uint8Array = new Proxy(OrigU8, {
  construct(target, args, newTarget) {
    if (typeof args[0] === 'number' && !(args[0] <= limit)) throw new RangeError('allocation above the replay limit');
    return Reflect.construct(target, args, newTarget === globalThis.Uint8Array ? target : newTarget);
  },
});
const { unzipSync } = await import(pathToFileURL(join(root, 'write/node_modules/fflate/esm/browser.js')).href);
const img = await importFrom(root, 'write/packages/docx-layout/dist/images.js');

// `Rng.bytes` of src/generate.rs
Rng.prototype.bytes = function bytes(n) { return OrigU8.from({ length: n }, () => this.next() & 0xff); };
const sha = (b) => createHash('sha256').update(b).digest('hex').slice(0, 32);
const fnv = (s) => { let h = 0x811c9dc5; for (const c of Buffer.from(s, 'utf8')) h = Math.imul(h ^ c, 0x01000193) >>> 0; return h; };

// ── byte builders ────────────────────────────────────────────────────────────────────────────────
class B {
  constructor() { this.a = []; }
  u8(v) { this.a.push(v & 0xff); return this; }
  u16(v, le = true) { return le ? this.u8(v).u8(v >> 8) : this.u8(v >> 8).u8(v); }
  u32(v, le = true) { return le ? this.u16(v & 0xffff, true).u16((v >>> 16) & 0xffff, true) : this.u16((v >>> 16) & 0xffff, false).u16(v & 0xffff, false); }
  bytes(b) { for (const x of b) this.a.push(x); return this; }
  get length() { return this.a.length; }
  done() { return OrigU8.from(this.a); }
}
const cat = (...parts) => { const b = new B(); for (const p of parts) b.bytes(p); return b.done(); };

// smooth-ish random content: compressible, like pictures
function content(rng, n) {
  const out = new OrigU8(n);
  let v = rng.below(256);
  for (let i = 0; i < n; i++) {
    if (rng.below(16) === 0) v = rng.below(256); else v = (v + rng.below(7) + 253) & 255;
    out[i] = v;
  }
  return out;
}

// ── PNG ──────────────────────────────────────────────────────────────────────────────────────────
const ADAM7 = [[0, 0, 8, 8], [4, 0, 8, 8], [0, 4, 4, 8], [2, 0, 4, 4], [0, 2, 2, 4], [1, 0, 2, 2], [0, 1, 1, 2]];
function pngChunk(type, data) {
  const t = Buffer.from(type, 'latin1');
  const crc = zlib.crc32(Buffer.concat([t, Buffer.from(data)]));
  return cat(new B().u32(data.length, false).done(), t, data, new B().u32(crc, false).done());
}
function pngRawLength(w, h, depth, ctype, interlace) {
  const ch = ctype === 0 ? 1 : ctype === 2 ? 3 : ctype === 3 ? 1 : ctype === 4 ? 2 : 4;
  const bitsPP = ch * depth;
  const passes = interlace ? ADAM7 : [[0, 0, 1, 1]];
  let n = 0;
  for (const [ox, oy, dx, dy] of passes) {
    const pw = Math.ceil((w - ox) / dx), ph = Math.ceil((h - oy) / dy);
    if (pw > 0 && ph > 0) n += ph * (1 + Math.ceil((pw * bitsPP) / 8));
  }
  return n;
}
function png(o) {
  const { w, h, depth, ctype, interlace = 0, plte, trns, seed, cut = 1, split = 1, extra = false, iend = true, filters = true } = o;
  const rng = new Rng(seed);
  const raw = content(rng, pngRawLength(w, h, depth, ctype, interlace));
  if (!filters) {
    // filter type 0 on every row: rows start at known offsets only without Adam7; keep random otherwise
    const stride = Math.ceil((w * (ctype === 0 ? 1 : ctype === 2 ? 3 : ctype === 3 ? 1 : ctype === 4 ? 2 : 4) * depth) / 8);
    if (!interlace) for (let y = 0; y < h; y++) raw[y * (1 + stride)] = 0;
  } else if (!interlace) {
    const stride = Math.ceil((w * (ctype === 0 ? 1 : ctype === 2 ? 3 : ctype === 3 ? 1 : ctype === 4 ? 2 : 4) * depth) / 8);
    for (let y = 0; y < h; y++) raw[y * (1 + stride)] = rng.below(5);
  }
  const z = zlib.deflateSync(raw.subarray(0, Math.floor(raw.length * cut)));
  const parts = [OrigU8.from([0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a])];
  parts.push(pngChunk('IHDR', new B().u32(w, false).u32(h, false).u8(depth).u8(ctype).u8(0).u8(0).u8(interlace).done()));
  if (extra) parts.push(pngChunk('tEXt', Buffer.from('Comment\0hello', 'latin1')));
  if (plte) parts.push(pngChunk('PLTE', plte));
  if (trns) parts.push(pngChunk('tRNS', trns));
  const per = Math.ceil(z.length / split);
  for (let i = 0; i < z.length || i === 0; i += per) parts.push(pngChunk('IDAT', z.subarray(i, i + per)));
  if (iend) parts.push(pngChunk('IEND', new OrigU8(0)));
  return cat(...parts);
}

function genPng(add) {
  const combos = [[0, 1], [0, 2], [0, 4], [0, 8], [0, 16], [2, 8], [2, 16], [3, 1], [3, 2], [3, 4], [3, 8], [4, 8], [4, 16], [6, 8], [6, 16]];
  const sizes = [[1, 1], [3, 5], [17, 9], [33, 20]];
  let seed = 100;
  for (const [ctype, depth] of combos) {
    for (const interlace of [0, 1]) {
      for (const [w, h] of sizes) {
        const base = { w, h, depth, ctype, interlace, seed: seed++ };
        const pal = (n) => { const r = new Rng(seed + 7); return r.bytes(3 * n); };
        const name = `gen/png/c${ctype}d${depth}i${interlace}-${w}x${h}`;
        if (ctype === 3) {
          const n = 1 << depth;
          add(`${name}-pal`, png({ ...base, plte: pal(n) }));
          add(`${name}-palA`, png({ ...base, plte: pal(n), trns: new Rng(seed + 9).bytes(Math.max(1, n >> 1)) }));
          if (w === 17) { add(`${name}-shortpal`, png({ ...base, plte: pal(Math.max(1, n >> 2)) })); add(`${name}-nopal`, png(base)); }
        } else if (ctype === 0) {
          add(name, png(base));
          add(`${name}-key`, png({ ...base, trns: depth === 16 ? OrigU8.from([0x12, 0x34]) : OrigU8.from([0, 1 % (1 << depth)]) }));
          if (depth === 8) add(`${name}-key0`, png({ ...base, trns: OrigU8.from([0, 0]) }));
        } else if (ctype === 2) {
          add(name, png(base));
          add(`${name}-key`, png({ ...base, trns: depth === 16 ? OrigU8.from([0, 1, 0, 2, 0, 3]) : OrigU8.from([0, 7, 0, 8, 0, 9]) }));
          add(`${name}-key0`, png({ ...base, trns: new OrigU8(6) }));
        } else add(name, png(base));
        if (w === 17 && h === 9) {
          add(`${name}-cut50`, png({ ...base, plte: ctype === 3 ? pal(1 << depth) : undefined, cut: 0.5 }));
          add(`${name}-cut0`, png({ ...base, plte: ctype === 3 ? pal(1 << depth) : undefined, cut: 0 }));
          add(`${name}-cut97`, png({ ...base, plte: ctype === 3 ? pal(1 << depth) : undefined, cut: 0.97 }));
          add(`${name}-split`, png({ ...base, plte: ctype === 3 ? pal(1 << depth) : undefined, split: 3, extra: true, iend: false }));
          add(`${name}-nofilter`, png({ ...base, plte: ctype === 3 ? pal(1 << depth) : undefined, filters: false }));
        }
      }
    }
  }
  // what the specification forbids: the TS runs the same expressions on it
  for (const depth of [3, 5, 7, 12, 24, 32, 255]) {
    for (const ctype of [0, 2, 3, 4, 6]) {
      for (const interlace of [0, 1]) {
        const base = { w: 5, h: 4, depth, ctype, interlace, seed: seed++ };
        const cap = (depth * 4 * 5 * 4) / 8 < 6000;
        if (cap) add(`gen/png/odd-c${ctype}d${depth}i${interlace}`, png({ ...base, plte: ctype === 3 ? new Rng(seed).bytes(3 * 16) : undefined, trns: ctype === 0 ? OrigU8.from([0, 1]) : undefined }));
      }
    }
  }
  for (const ctype of [1, 5, 7, 8, 255]) {
    for (const depth of [8, 16]) add(`gen/png/ctype${ctype}d${depth}`, png({ w: 6, h: 3, depth, ctype, seed: seed++ }));
  }
  // dimensions: a row of zero pixels, a 1-px column, tall and wide strips
  for (const [w, h] of [[300, 1], [1, 300], [2, 129], [129, 2]]) add(`gen/png/strip-${w}x${h}`, png({ w, h, depth: 8, ctype: 6, seed: seed++ }));
  add('gen/png/zero-width', png({ w: 0, h: 4, depth: 8, ctype: 2, seed: seed++ }));
  add('gen/png/zero-height', png({ w: 4, h: 0, depth: 8, ctype: 2, seed: seed++ }));
}

// ── GIF ──────────────────────────────────────────────────────────────────────────────────────────
function gifLzw(indices, minCode) {
  const clear = 1 << minCode, eoi = clear + 1;
  const bits = [];
  let size = minCode + 1;
  const put = (code) => { for (let k = 0; k < size; k++) bits.push((code >> k) & 1); };
  let dict = new Map();
  let next = eoi + 1;
  put(clear);
  let w = -1;
  for (const k of indices) {
    if (w < 0) { w = k; continue; }
    const key = w * 4096 + k;
    if (dict.has(key)) { w = dict.get(key); continue; }
    put(w);
    dict.set(key, next++);
    if (next > (1 << size) && size < 12) size++;
    if (next >= 4096) { put(clear); dict = new Map(); next = eoi + 1; size = minCode + 1; }
    w = k;
  }
  if (w >= 0) put(w);
  put(eoi);
  const out = [];
  for (let i = 0; i < bits.length; i += 8) { let v = 0; for (let k = 0; k < 8 && i + k < bits.length; k++) v |= bits[i + k] << k; out.push(v); }
  return out;
}
function gif(o) {
  const { sw, sh, iw, ih, ix = 0, iy = 0, minCode, writtenMin = minCode, gct = true, lct = false, interlace = false, transparent = -1, seed, random = false, noFrame = false } = o;
  const rng = new Rng(seed);
  const b = new B();
  b.bytes(Buffer.from('GIF89a', 'latin1')).u16(sw).u16(sh);
  const gbits = Math.min(8, Math.max(1, minCode));
  b.u8((gct ? 0x80 : 0) | 0x70 | (gbits - 1)).u8(0).u8(0);
  if (gct) b.bytes(rng.bytes(3 * (1 << gbits)));
  if (transparent >= 0) b.u8(0x21).u8(0xf9).u8(4).u8(1).u16(10).u8(transparent).u8(0);
  b.u8(0x21).u8(0xfe).u8(3).bytes(Buffer.from('abc', 'latin1')).u8(0);
  if (noFrame) { b.u8(0x3b); return b.done(); }
  b.u8(0x2c).u16(ix).u16(iy).u16(iw).u16(ih).u8((lct ? 0x80 : 0) | (interlace ? 0x40 : 0) | (lct ? gbits - 1 : 0));
  if (lct) b.bytes(rng.bytes(3 * (1 << gbits)));
  b.u8(writtenMin);
  let data;
  if (random) data = rng.bytes(Math.max(1, Math.floor((iw * ih) / 2)));
  else {
    // runs and noise in the colours of the table
    const idx = [];
    let v = rng.below(1 << minCode);
    for (let i = 0; i < iw * ih; i++) { if (rng.below(5) === 0) v = rng.below(1 << minCode); idx.push(v); }
    data = gifLzw(idx, minCode);
    o.indices = idx;
  }
  for (let i = 0; i < data.length; i += 255) { const n = Math.min(255, data.length - i); b.u8(n); for (let k = 0; k < n; k++) b.u8(data[i + k]); }
  b.u8(0).u8(0x3b);
  return b.done();
}
function genGif(add) {
  let seed = 5000;
  for (const minCode of [1, 2, 3, 4, 5, 6, 7, 8, 11]) {
    for (const [iw, ih] of [[1, 1], [5, 7], [16, 16], [33, 20]]) {
      for (const interlace of [false, true]) {
        const small = minCode > 8 ? iw * ih > 400 : false;
        if (small) continue;
        add(`gen/gif/m${minCode}-${iw}x${ih}${interlace ? 'i' : ''}`, gif({ sw: iw, sh: ih, iw, ih, minCode, interlace, seed: seed++ }));
      }
    }
    add(`gen/gif/m${minCode}-trans`, gif({ sw: 9, sh: 9, iw: 9, ih: 9, minCode, transparent: 1, seed: seed++ }));
    add(`gen/gif/m${minCode}-lct`, gif({ sw: 9, sh: 9, iw: 9, ih: 9, minCode, gct: false, lct: true, seed: seed++ }));
    add(`gen/gif/m${minCode}-nct`, gif({ sw: 9, sh: 9, iw: 9, ih: 9, minCode, gct: false, seed: seed++ }));
    add(`gen/gif/m${minCode}-offset`, gif({ sw: 12, sh: 12, iw: 8, ih: 8, ix: 3, iy: 2, minCode, seed: seed++ }));
    add(`gen/gif/m${minCode}-crop`, gif({ sw: 6, sh: 5, iw: 10, ih: 9, ix: 2, iy: 1, minCode, interlace: true, seed: seed++ }));
    add(`gen/gif/m${minCode}-random`, gif({ sw: 20, sh: 20, iw: 20, ih: 20, minCode, random: true, seed: seed++ }));
    add(`gen/gif/m${minCode}-bigrun`, gif({ sw: 64, sh: 64, iw: 64, ih: 64, minCode, seed: seed++ }));
  }
  for (const written of [0, 12, 13, 255]) add(`gen/gif/badmin${written}`, gif({ sw: 4, sh: 4, iw: 4, ih: 4, minCode: 2, writtenMin: written, seed: seed++ }));
  add('gen/gif/noframe', gif({ sw: 4, sh: 4, iw: 4, ih: 4, minCode: 2, noFrame: true, seed: seed++ }));
  add('gen/gif/zero-screen', gif({ sw: 0, sh: 0, iw: 3, ih: 3, minCode: 2, seed: seed++ }));
}

// ── BMP ──────────────────────────────────────────────────────────────────────────────────────────
function bmp(o) {
  const { w, h, bpp, comp = 0, hdr = 40, masks, seed, topDown = false, palette = true, cut = 1 } = o;
  const rng = new Rng(seed);
  const stride = Math.ceil(((w | 0) < 0 ? 4 * bpp : w * bpp) / 32) * 4;
  const colors = bpp <= 8 ? 1 << bpp : 0;
  const bf = comp === 3 && hdr === 40 ? 12 : 0;
  const palOff = 14 + hdr + bf;
  const off = palOff + (palette ? colors * 4 : 0);
  const pix = content(rng, stride * Math.abs(h));
  const b = new B();
  b.u8(0x42).u8(0x4d).u32(off + pix.length).u32(0).u32(off);
  b.u32(hdr).u32(w).u32(topDown ? (-Math.abs(h)) >>> 0 : h >>> 0).u16(1).u16(bpp).u32(comp).u32(pix.length);
  b.u32(2835).u32(2835).u32(colors).u32(0);
  let written = 40;
  if (hdr >= 56 || bf) {
    const m = masks ?? [0xff0000, 0xff00, 0xff, hdr >= 56 ? 0xff000000 : 0];
    b.u32(m[0] >>> 0).u32(m[1] >>> 0).u32(m[2] >>> 0); written += 12;
    if (hdr >= 56) { b.u32(m[3] >>> 0); written += 4; }
  }
  while (written < hdr) { b.u8(0); written++; }
  if (palette) b.bytes(rng.bytes(colors * 4));
  b.bytes(pix.subarray(0, Math.floor(pix.length * cut)));
  return b.done();
}
function genBmp(add) {
  let seed = 9000;
  for (const bpp of [1, 4, 8, 16, 24, 32]) {
    for (const [w, h] of [[1, 1], [5, 3], [17, 9], [33, 20]]) {
      for (const topDown of [false, true]) add(`gen/bmp/b${bpp}-${w}x${h}${topDown ? 't' : ''}`, bmp({ w, h, bpp, topDown, seed: seed++ }));
    }
    add(`gen/bmp/b${bpp}-cut`, bmp({ w: 17, h: 9, bpp, cut: 0.4, seed: seed++ }));
    add(`gen/bmp/b${bpp}-nopal`, bmp({ w: 17, h: 9, bpp, palette: false, seed: seed++ }));
  }
  for (const bpp of [16, 32]) {
    const sets = bpp === 16 ? [[0xf800, 0x7e0, 0x1f, 0], [0x7c00, 0x3e0, 0x1f, 0x8000], [0xf00, 0xf0, 0xf, 0xf000], [0, 0, 0, 0], [0x5555, 0xaaaa, 0xf0f0, 0x0f0f]]
      : [[0xff0000, 0xff00, 0xff, 0xff000000], [0xff, 0xff00, 0xff0000, 0], [0x3ff00000, 0xffc00, 0x3ff, 0xc0000000], [0xffffffff, 0, 0, 0], [0x80000000, 0x40000000, 1, 2]];
    let i = 0;
    for (const masks of sets) for (const hdr of [40, 56, 108, 124]) add(`gen/bmp/bf${bpp}-${i}-h${hdr}`, bmp({ w: 9, h: 5, bpp, comp: 3, hdr, masks, seed: seed++ })), i++;
  }
  for (const comp of [1, 2, 4, 5, 6]) add(`gen/bmp/comp${comp}`, bmp({ w: 5, h: 5, bpp: 8, comp, seed: seed++ }));
  for (const bpp of [0, 2, 3, 5, 12, 15, 48, 64]) add(`gen/bmp/odd-b${bpp}`, bmp({ w: 9, h: 3, bpp, seed: seed++ }));
  add('gen/bmp/h12', bmp({ w: 8, h: 4, bpp: 24, hdr: 12, seed: seed++ }));
  add('gen/bmp/zero-w', bmp({ w: 0, h: 4, bpp: 24, seed: seed++ }));
  add('gen/bmp/zero-h', bmp({ w: 4, h: 0, bpp: 24, seed: seed++ }));
  add('gen/bmp/neg-w', bmp({ w: -4 >>> 0, h: 4, bpp: 24, seed: seed++ }));
}

// ── TIFF ─────────────────────────────────────────────────────────────────────────────────────────
function tiffLzwEncode(data) {
  const out = [];
  let acc = 0, nacc = 0;
  const put = (code, width) => { acc = (acc << width) | code; nacc += width; while (nacc >= 8) { out.push((acc >> (nacc - 8)) & 0xff); nacc -= 8; } acc &= (1 << nacc) - 1; };
  let dict = new Map();
  let next = 258, width = 9;
  put(256, width);
  let w = -1;
  for (const k of data) {
    if (w < 0) { w = k; continue; }
    const key = w * 256 + k;
    if (dict.has(key)) { w = dict.get(key); continue; }
    put(w, width);
    dict.set(key, next++);
    if (next + 1 > (1 << width) && width < 12) width++;
    if (next >= 4093) { put(256, width); dict = new Map(); next = 258; width = 9; }
    w = k;
  }
  if (w >= 0) put(w, width);
  put(257, width);
  if (nacc) put(0, 8 - nacc);
  return OrigU8.from(out);
}
function packBitsEncode(data) {
  const out = [];
  let i = 0;
  while (i < data.length) {
    let run = 1;
    while (i + run < data.length && run < 128 && data[i + run] === data[i]) run++;
    if (run >= 2) { out.push((257 - run) & 0xff, data[i]); i += run; continue; }
    let lit = 1;
    while (i + lit < data.length && lit < 128 && !(i + lit + 1 < data.length && data[i + lit] === data[i + lit + 1])) lit++;
    out.push(lit - 1);
    for (let k = 0; k < lit; k++) out.push(data[i + k]);
    i += lit;
  }
  return OrigU8.from(out);
}
function compress(comp, data) {
  if (comp === 1) return data;
  if (comp === 5) return tiffLzwEncode(data);
  if (comp === 32773) return packBitsEncode(data);
  return zlib.deflateSync(data);
}
function predictorApply(raw, w, h, spp, bits, le) {
  const rowBytes = Math.ceil((w * spp * bits) / 8);
  const out = OrigU8.from(raw);
  for (let y = 0; y < h; y++) {
    if (bits === 8) for (let x = w * spp - 1; x >= spp; x--) out[y * rowBytes + x] = (raw[y * rowBytes + x] - raw[y * rowBytes + x - spp]) & 0xff;
    else if (bits === 16) {
      const get = (o) => (le ? raw[o] | (raw[o + 1] << 8) : (raw[o] << 8) | raw[o + 1]);
      for (let s = w * spp - 1; s >= spp; s--) {
        const o = y * rowBytes + s * 2, v = (get(o) - get(o - spp * 2)) & 0xffff;
        if (le) { out[o] = v & 0xff; out[o + 1] = v >> 8; } else { out[o] = v >> 8; out[o + 1] = v & 0xff; }
      }
    }
  }
  return out;
}
function tiff(o) {
  const { w, h, spp = 1, bits = 8, photo = 1, comp = 1, predictor = 1, le = true, rps, tile, cmap, sub, planar, seed, ycc = false, extraTags = [], rawLen, cut = 1 } = o;
  const rng = new Rng(seed);
  const rowBytes = Math.ceil((w * spp * bits) / 8);
  let raw;
  if (ycc && sub && (sub[0] > 1 || sub[1] > 1)) {
    const across = Math.ceil(w / sub[0]), unit = sub[0] * sub[1] + 2, blockRows = Math.ceil(h / sub[1]);
    raw = content(rng, across * unit * blockRows);
  } else raw = content(rng, rawLen ?? rowBytes * h);
  const pieces = [];
  if (ycc && sub && (sub[0] > 1 || sub[1] > 1)) {
    const across = Math.ceil(w / sub[0]), unit = sub[0] * sub[1] + 2, blockRow = across * unit;
    const r = rps ?? h;
    for (let y = 0; y < h; y += r) {
      const rows = Math.min(r, h - y), nb = Math.ceil(rows / sub[1]);
      pieces.push(compress(comp, raw.subarray((y / sub[1]) * blockRow, (y / sub[1]) * blockRow + nb * blockRow)));
    }
  } else if (tile) {
    const [tw, th] = tile;
    const tileRow = Math.ceil((tw * spp * bits) / 8);
    const src = predictor === 2 ? predictorApply(raw, w, h, spp, bits, le) : raw;
    for (let ty = 0; ty < h; ty += th) {
      for (let tx = 0; tx < w; tx += tw) {
        const t = new OrigU8(tileRow * th);
        for (let y = 0; y < th && ty + y < h; y++) {
          const from = (ty + y) * rowBytes + Math.floor((tx * spp * bits) / 8);
          const len = Math.min(tileRow, rowBytes - Math.floor((tx * spp * bits) / 8));
          t.set(src.subarray(from, from + len), y * tileRow);
        }
        pieces.push(compress(comp, t));
      }
    }
  } else {
    const src = predictor === 2 ? predictorApply(raw, w, h, spp, bits, le) : raw;
    const r = rps ?? h;
    for (let y = 0; y < h; y += r) pieces.push(compress(comp, src.subarray(y * rowBytes, Math.min(h, y + r) * rowBytes)));
  }
  const e = (v, n = 2) => (n === 2 ? new B().u16(v, le) : new B().u32(v, le));
  // layout: header, data, then the IFD with its out-of-line values
  const body = new B();
  const offs = [], counts = [];
  let at = 8;
  for (const p of pieces) { offs.push(at); const q = p.subarray(0, Math.floor(p.length * cut)); counts.push(q.length); body.bytes(q); at += q.length; if (at & 1) { body.u8(0); at++; } }
  const entries = [];
  const out = new B();
  const heap = new B();
  const heapAt = () => at + 2 + 12 * 32 + heap.length;
  const add = (tag, type, vals) => entries.push({ tag, type, vals });
  add(256, 4, [w]); add(257, 4, [h]);
  add(258, 3, Array(spp).fill(bits));
  add(259, 3, [comp]); add(262, 3, [photo]); add(277, 3, [spp]);
  if (tile) { add(322, 3, [tile[0]]); add(323, 3, [tile[1]]); add(324, 4, offs); add(325, 4, counts); }
  else { add(273, 4, offs); add(278, 4, [rps ?? h]); add(279, 4, counts); }
  if (predictor !== 1) add(317, 3, [predictor]);
  if (planar) add(284, 3, [planar]);
  if (cmap) add(320, 3, cmap);
  if (sub) add(530, 3, sub);
  for (const x of extraTags) add(x.tag, x.type, x.vals);
  entries.sort((a, b) => a.tag - b.tag);
  const size = { 1: 1, 3: 2, 4: 4, 5: 8 };
  const ifd = new B();
  ifd.u16(entries.length, le);
  for (const en of entries) {
    const bytes = new B();
    for (const v of en.vals) {
      if (en.type === 3) bytes.u16(v, le); else if (en.type === 4) bytes.u32(v, le); else if (en.type === 1) bytes.u8(v); else if (en.type === 5) { bytes.u32(v[0], le); bytes.u32(v[1], le); }
    }
    ifd.u16(en.tag, le).u16(en.type, le).u32(en.vals.length, le);
    if (bytes.length <= 4) { const a = bytes.a.slice(); while (a.length < 4) a.push(0); ifd.bytes(a); }
    else { if (heap.length & 1) heap.u8(0); const off = at + 2 + 12 * entries.length + 4 + heap.length; ifd.u32(off, le); heap.bytes(bytes.a); }
  }
  ifd.u32(0, le);
  const hdr = new B().bytes(le ? [0x49, 0x49] : [0x4d, 0x4d]).u16(42, le).u32(at, le);
  void e; void heapAt; void size;
  return cat(hdr.done(), body.done(), ifd.done(), heap.done());
}
function genTiff(add) {
  let seed = 20000;
  const sizes = [[1, 1], [7, 5], [16, 16], [33, 20]];
  const comps = [1, 5, 32773, 8, 32946];
  for (const le of [true, false]) {
    const L = le ? 'le' : 'be';
    for (const [w, h] of sizes) {
      for (const comp of comps) {
        for (const predictor of [1, 2]) {
          const name = `${L}-${w}x${h}-c${comp}-p${predictor}`;
          add(`gen/tiff/gray8-${name}`, tiff({ w, h, comp, predictor, le, seed: seed++, photo: 1, rps: Math.max(1, h >> 1) }));
          add(`gen/tiff/rgb8-${name}`, tiff({ w, h, spp: 3, photo: 2, comp, predictor, le, seed: seed++ }));
          if (w > 1 && h > 1) add(`gen/tiff/rgb16-${name}`, tiff({ w, h, spp: 3, bits: 16, photo: 2, comp, predictor, le, seed: seed++ }));
          if (w === 16) add(`gen/tiff/gray16-${name}`, tiff({ w, h, bits: 16, photo: 0, comp, predictor, le, seed: seed++ }));
        }
      }
      add(`gen/tiff/rgba8-${L}-${w}x${h}`, tiff({ w, h, spp: 4, photo: 2, le, seed: seed++ }));
      add(`gen/tiff/graya8-${L}-${w}x${h}`, tiff({ w, h, spp: 2, photo: 1, le, seed: seed++ }));
      add(`gen/tiff/white0-${L}-${w}x${h}`, tiff({ w, h, photo: 0, le, seed: seed++ }));
      for (const bits of [1, 2, 4]) add(`gen/tiff/gray${bits}-${L}-${w}x${h}`, tiff({ w, h, bits, photo: bits === 1 ? 0 : 1, le, seed: seed++ }));
      for (const bits of [1, 4, 8]) {
        const n = 1 << bits, r = new Rng(seed);
        const cmap = Array.from({ length: 3 * n }, () => r.below(65536));
        add(`gen/tiff/pal${bits}-${L}-${w}x${h}`, tiff({ w, h, bits, photo: 3, cmap, le, seed: seed++ }));
        if (w === 7) add(`gen/tiff/pal${bits}-short-${L}`, tiff({ w, h, bits, photo: 3, cmap: cmap.slice(0, n), le, seed: seed++ }));
      }
      add(`gen/tiff/cmyk-${L}-${w}x${h}`, tiff({ w, h, spp: 4, photo: 5, le, seed: seed++ }));
      add(`gen/tiff/cmyk3-${L}-${w}x${h}`, tiff({ w, h, spp: 3, photo: 5, le, seed: seed++ }));
      for (const sub of [[1, 1], [2, 2], [2, 1], [1, 2], [4, 4], [3, 2]]) {
        for (const comp of [1, 5, 8]) add(`gen/tiff/ycc${sub.join('x')}-c${comp}-${L}-${w}x${h}`, tiff({ w, h, spp: 3, photo: 6, ycc: true, sub, comp, rps: sub[1] * 2, le, seed: seed++ }));
      }
      for (const [tw, th] of [[16, 16], [32, 16], [8, 8]]) {
        for (const comp of [1, 5, 32773, 8]) {
          add(`gen/tiff/tile${tw}x${th}-c${comp}-${L}-${w}x${h}`, tiff({ w, h, spp: 3, photo: 2, comp, tile: [tw, th], le, seed: seed++ }));
          if (comp === 1) add(`gen/tiff/tile-gray-${tw}x${th}-${L}-${w}x${h}`, tiff({ w, h, tile: [tw, th], predictor: 2, le, seed: seed++ }));
        }
      }
    }
  }
  const ycc4 = tiff({ w: 16, h: 16, spp: 3, photo: 6, ycc: true, sub: [2, 2], rps: 4, seed: seed++ });
  add('gen/tiff/ycc-coef-default', ycc4);
  const coef = (vals) => ({ tag: 529, type: 5, vals });
  add('gen/tiff/ycc-coef-709', tiff({ w: 8, h: 8, spp: 3, photo: 6, ycc: true, sub: [1, 1], seed: seed++, extraTags: [coef([[2126, 10000], [7152, 10000], [722, 10000]])] }));
  add('gen/tiff/ycc-coef-zero', tiff({ w: 8, h: 8, spp: 3, photo: 6, ycc: true, sub: [1, 1], seed: seed++, extraTags: [coef([[0, 1], [0, 1], [0, 1]])] }));
  add('gen/tiff/ycc-tiled-sub', tiff({ w: 16, h: 16, spp: 3, photo: 6, ycc: true, sub: [2, 2], tile: [16, 16], seed: seed++ }));
  add('gen/tiff/planar2', tiff({ w: 8, h: 8, spp: 3, photo: 2, planar: 2, seed: seed++ }));
  add('gen/tiff/comp-unknown', tiff({ w: 8, h: 8, comp: 7, seed: seed++ }));
  add('gen/tiff/zero-w', tiff({ w: 0, h: 8, seed: seed++, rawLen: 0 }));
  add('gen/tiff/rps0', tiff({ w: 8, h: 8, seed: seed++, extraTags: [{ tag: 278, type: 4, vals: [0] }] }));
  add('gen/tiff/cut', tiff({ w: 16, h: 16, spp: 3, photo: 2, comp: 5, seed: seed++, cut: 0.5 }));
  add('gen/tiff/cut-deflate', tiff({ w: 16, h: 16, spp: 3, photo: 2, comp: 8, seed: seed++, cut: 0.5 }));
  add('gen/tiff/cut-pack', tiff({ w: 16, h: 16, spp: 3, photo: 2, comp: 32773, seed: seed++, cut: 0.5 }));
  add('gen/tiff/frac-width', tiff({ w: 8, h: 8, seed: seed++, extraTags: [{ tag: 256, type: 5, vals: [[15, 2]] }] }));
  add('gen/tiff/bits-tag-short-count', tiff({ w: 8, h: 8, spp: 3, photo: 2, seed: seed++ }));
  for (const bits of [3, 5, 6, 7, 12, 24, 32]) add(`gen/tiff/odd-bits${bits}`, tiff({ w: 7, h: 5, bits, photo: 1, rawLen: Math.ceil((7 * bits) / 8) * 5, seed: seed++ }));
  add('gen/tiff/photo-rgb-spp1', tiff({ w: 8, h: 8, photo: 2, spp: 1, seed: seed++ }));
  add('gen/tiff/bytes-bps', tiff({ w: 8, h: 8, seed: seed++, extraTags: [{ tag: 258, type: 1, vals: [8] }] }));
}

// ── JPEG headers ─────────────────────────────────────────────────────────────────────────────────
function jpegHeaders(add) {
  const seg = (m, payload) => cat(OrigU8.from([0xff, m]), new B().u16(payload.length + 2, false).done(), payload);
  const sof = (m, w, h, nc) => seg(m, cat(OrigU8.from([8]), new B().u16(h, false).u16(w, false).done(), OrigU8.from([nc]), new OrigU8(nc * 3)));
  const SOI = OrigU8.from([0xff, 0xd8]);
  const app0 = seg(0xe0, cat(Buffer.from('JFIF\0', 'latin1'), OrigU8.from([1, 1, 1, 0, 96, 0, 96, 0, 0])));
  const adobe = seg(0xee, cat(Buffer.from('Adobe', 'latin1'), OrigU8.from([0, 100, 0, 0, 0, 0, 2])));
  const dqt = seg(0xdb, new OrigU8(65));
  const dht = seg(0xc4, new OrigU8(20));
  const sos = seg(0xda, OrigU8.from([3, 1, 0, 2, 0x11, 3, 0x11, 0, 63, 0]));
  const eoi = OrigU8.from([0xff, 0xd9]);
  const fill = OrigU8.from([0xff, 0xff, 0xff]);
  add('gen/jpeg/baseline', cat(SOI, app0, dqt, sof(0xc0, 33, 20, 3), dht, sos, eoi));
  add('gen/jpeg/progressive', cat(SOI, app0, dqt, sof(0xc2, 640, 480, 3), dht, sos, eoi));
  add('gen/jpeg/adobe-cmyk', cat(SOI, adobe, dqt, sof(0xc0, 8, 9, 4), sos, eoi));
  add('gen/jpeg/gray', cat(SOI, dqt, sof(0xc0, 1, 1, 1), sos, eoi));
  add('gen/jpeg/fill-bytes', cat(SOI, fill, app0, fill, dqt, sof(0xc1, 7, 7, 3), eoi));
  add('gen/jpeg/restart-markers', cat(SOI, OrigU8.from([0xff, 0xd0, 0xff, 0xd7, 0xff, 0x01, 0xff, 0xd8]), dqt, sof(0xc3, 9, 9, 3), eoi));
  add('gen/jpeg/skip-dht-dac-jpg', cat(SOI, dht, seg(0xcc, new OrigU8(4)), seg(0xc8, new OrigU8(4)), sof(0xcf, 5, 6, 3), eoi));
  add('gen/jpeg/no-sof', cat(SOI, app0, dqt, sos, eoi));
  add('gen/jpeg/cut-sof', cat(SOI, sof(0xc0, 5, 5, 3).subarray(0, 8)));
  add('gen/jpeg/sof-last-byte', cat(SOI, sof(0xc0, 5, 5, 3).subarray(0, 10)));
  add('gen/jpeg/sof-exact', cat(SOI, sof(0xc0, 5, 5, 3).subarray(0, 11)));
  add('gen/jpeg/zero-length-seg', cat(SOI, OrigU8.from([0xff, 0xe1, 0, 0]), OrigU8.from([0xff, 0xe2, 0, 1]), sof(0xc0, 12, 13, 3)));
  add('gen/jpeg/junk-between', cat(SOI, OrigU8.from([1, 2, 3, 4]), sof(0xc0, 12, 13, 3)));
  add('gen/jpeg/adobe-short', cat(SOI, OrigU8.from([0xff, 0xee, 0, 4, 0x41]), sof(0xc0, 12, 13, 3)));
  add('gen/jpeg/big', cat(SOI, sof(0xc0, 65535, 65535, 3)));
  add('gen/jpeg/soi-only', SOI);
  add('gen/jpeg/empty', new OrigU8(0));
  add('gen/jpeg/png-size', cat(OrigU8.from([0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a, 0, 0, 0, 13, 0x49, 0x48, 0x44, 0x52]), new B().u32(0xfffffff0, false).u32(7, false).done(), OrigU8.from([8, 6, 0, 0, 0])));
  add('gen/jpeg/png-size-short', OrigU8.from([0x89, 0x50, 0x4e, 0x47, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 5, 0, 0, 0, 6]));
  add('gen/jpeg/gif-size', cat(Buffer.from('GIF89a', 'latin1'), new B().u16(300).u16(200).done(), OrigU8.from([0, 0, 0, 0])));
  add('gen/jpeg/gif-size-short', cat(Buffer.from('GIF89a', 'latin1'), OrigU8.from([1, 2, 3, 4])));
  add('gen/jpeg/bmp-size-neg', cat(OrigU8.from([0x42, 0x4d]), new OrigU8(16), new B().u32(100).u32(0x80000000).done(), new OrigU8(8)));
  add('gen/jpeg/bmp-size-short', cat(OrigU8.from([0x42, 0x4d]), new OrigU8(24)));
}

// ── the corpus ───────────────────────────────────────────────────────────────────────────────────
const cases = [];
const generated = [];
const addGen = (key, bytes) => { const b = OrigU8.from(bytes); cases.push({ key, bytes: b }); generated.push({ key, bytes: b }); };
genPng(addGen); genGif(addGen); genBmp(addGen); genTiff(addGen); jpegHeaders(addGen);

// oracle images
const OOXML = new Set(['docx', 'xlsx', 'pptx', 'docm', 'xlsm', 'pptm', 'potx', 'dotx', 'xltx', 'xlsb']);
const IMG = new Set(['png', 'jpg', 'jpeg', 'gif', 'bmp', 'tif', 'tiff']);
const tracked = execSync('git ls-files', { cwd: repo, encoding: 'utf8', maxBuffer: 1 << 28 }).split('\n').filter(Boolean).sort();
const seen = new Set();
const oracle = [];
for (const f of tracked) {
  const ext = f.split('.').pop().toLowerCase();
  if (OOXML.has(ext)) {
    let z;
    try { z = unzipSync(new OrigU8(readFileSync(join(repo, f)))); } catch { continue; }
    for (const [n, d] of Object.entries(z)) {
      if (!/\/media\//i.test(n) && !IMG.has(n.split('.').pop().toLowerCase())) continue;
      const h = sha(d);
      if (seen.has(h) || d.length > 4 << 20) continue;
      seen.add(h);
      oracle.push({ key: `oracle/${f}!${n}`, bytes: OrigU8.from(d) });
    }
  } else if (IMG.has(ext)) {
    const d = new OrigU8(readFileSync(join(repo, f)));
    const h = sha(d);
    if (seen.has(h) || d.length > 4 << 20) continue;
    seen.add(h);
    oracle.push({ key: `oracle/${f}`, bytes: d });
  }
}
cases.push(...oracle);

// ── mutations (mirrored in tests/image_parity.rs) ───────────────────────────────────────────────
const HOSTILE = [0, 0xffffffff, 0x7fffffff, 0x80000000, 1, 0x10000, 0xffff, 0x100, 0x40000000];
function mutate(b, name, m) {
  const rng = new Rng((fnv(name) ^ Math.imul(m + 1, 0x9e3779b1)) >>> 0);
  const n = b.length;
  const a = OrigU8.from(b);
  if (n === 0) return a;
  switch (m % 5) {
    case 0: return a.slice(0, rng.below(n));
    case 1: { const k = 1 + rng.below(3); for (let i = 0; i < k; i++) { const p = rng.below(Math.min(64, n)); a[p] ^= 1 << rng.below(8); } return a; }
    case 2: { const k = 1 + rng.below(3); for (let i = 0; i < k; i++) { const p = rng.below(n); a[p] ^= 1 << rng.below(8); } return a; }
    case 3: { const k = 1 + rng.below(3); for (let i = 0; i < k; i++) { const p = rng.below(n); a[p] = rng.below(256); } return a; }
    default: {
      const k = 1 + rng.below(2);
      for (let i = 0; i < k; i++) {
        const p = rng.below(Math.min(128, n)), v = HOSTILE[rng.below(HOSTILE.length)];
        for (let j = 0; j < 4; j++) if (p + j < n) a[p + j] = (v >>> (8 * j)) & 0xff;
      }
      return a;
    }
  }
}
const MUTATIONS = 5;
const mutated = [];
// images that decode to more than 128 Kpx are not mutated: with the 4 MiB limit of the mutations the buffers of fflate (which doubles
// its output buffer) fall on either side of it depending on the inflate, a test of the allocator and not of the decoder
const big = (b) => {
  const s = (() => { try { return img.imagePixelSize(b); } catch { return null; } })();
  if (s && s.width * s.height > 131072) return true;
  for (const f of [img.decodePng, img.decodeGif, img.decodeBmp, img.decodeTiff]) {
    try { const r = f(b); if (r && r.width * r.height > 131072) return true; } catch { /* not decodable: mutated like the rest */ }
  }
  return false;
};
for (const c of cases) {
  if (c.bytes.length > 200 * 1024 || big(c.bytes)) continue;
  for (let m = 0; m < MUTATIONS; m++) mutated.push({ key: `mut/${c.key}#${m}`, bytes: mutate(c.bytes, c.key, m) });
}
cases.push(...mutated);

// ── capture ──────────────────────────────────────────────────────────────────────────────────────
const result = (fn, b) => {
  let r;
  try { r = fn(b); } catch { return 'E'; }
  if (r === null) return 'N';
  return `K:${r.width}x${r.height}:${sha(r.data)}`;
};
const jpegOut = (b) => { try { const j = img.jpegInfo(b); return j ? `${j.width}x${j.height}x${j.components}${j.adobe ? 'a' : ''}` : 'N'; } catch { return 'E'; } };
const sizeOut = (b) => { try { const s = img.imagePixelSize(b); return s ? `${s.width}x${s.height}` : 'N'; } catch { return 'E'; } };

const cap = new CaptureWriter(['Parity capture of uo-image (images.ts): see tools/capture-image.mjs']);
const stats = { K: 0, N: 0, E: 0 };
for (const c of cases) {
  const { key, bytes } = c;
  const t0 = performance.now();
  limit = key.startsWith('mut/') ? MUT_LIMIT : LIMIT;
  cap.set(key, 'in', sha(bytes));
  cap.set(key, 'jpeg', jpegOut(bytes));
  cap.set(key, 'size', sizeOut(bytes));
  for (const [facet, fn] of [['png', img.decodePng], ['gif', img.decodeGif], ['bmp', img.decodeBmp], ['tiff', img.decodeTiff]]) {
    const r = result(fn, bytes);
    stats[r[0]]++;
    cap.set(key, facet, r);
  }
  const dt = performance.now() - t0;
  if (dt > 1000) console.error(`slow (${Math.round(dt)} ms): ${key}`);
}
cap.write(outPath);

// the pack of the generated images: "UOIMGPK1", u32 count, then (u16 name length, name, u32 length, bytes)*
const pack = new B();
pack.bytes(Buffer.from('UOIMGPK1', 'latin1')).u32(generated.length);
for (const g of generated) { const nm = Buffer.from(g.key, 'utf8'); pack.u16(nm.length).bytes(nm).u32(g.bytes.length).bytes(g.bytes); }
writeFileSync(packPath, pack.done());
console.log(`${cases.length} images (${generated.length} generated, ${oracle.length} oracle, ${mutated.length} mutated); decoder results ${JSON.stringify(stats)}; pack ${pack.length} bytes`);
console.log(`wrote ${relPath(repo, outPath)} and ${relPath(repo, packPath)}`);
