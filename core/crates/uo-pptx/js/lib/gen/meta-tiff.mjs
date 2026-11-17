// B5b synthetic TIFF (generator group `b5b`): tiny baseline TIFFs in code (little and big endian; strips and tiles; compression none,
// PackBits, LZW, deflate; horizontal predictor; 1/2/4/8/16 bits; gray, RGB(A), palette, CMYK) for the Slides decoder of `tiff.ts`.
// The oracle is the frozen TS `decodeTiff`; what is written does not have to be a TIFF a program would open.
import { deflateSync } from 'node:zlib';
import { Rand } from './rand.mjs';

function packBitsEnc(src) {
  const out = [];
  for (let i = 0; i < src.length;) {
    let run = 1;
    while (i + run < src.length && src[i + run] === src[i] && run < 128) run++;
    if (run >= 2) { out.push((257 - run) & 255, src[i]); i += run; continue; }
    let n = 1;
    while (i + n < src.length && n < 128 && src[i + n] !== src[i + n + 1]) n++;
    out.push(n - 1, ...src.slice(i, i + n));
    i += n;
  }
  return out;
}

/** TIFF LZW (MSB first, early change): clear at the start and when the table is nearly full. */
function lzwEnc(src) {
  const out = [];
  let cur = 0, nbits = 0;
  const put = (code, width) => { cur = (cur << width) | code; nbits += width; while (nbits >= 8) { out.push((cur >> (nbits - 8)) & 255); nbits -= 8; } cur &= (1 << nbits) - 1; };
  let table = new Map();
  let next = 258, width = 9;
  const reset = () => { table = new Map(); next = 258; width = 9; };
  put(256, width);
  let w = [];
  for (const c of src) {
    const wc = [...w, c];
    const key = wc.join(',');
    if (wc.length === 1 || table.has(key)) { w = wc; continue; }
    const code = w.length === 1 ? w[0] : table.get(w.join(','));
    put(code, width);
    table.set(key, next++);
    if (next === (1 << width) && width < 12) width++;
    if (next >= 4093) { put(256, width); reset(); }
    w = [c];
  }
  if (w.length) put(w.length === 1 ? w[0] : table.get(w.join(',')), width);
  put(257, width);
  if (nbits) out.push((cur << (8 - nbits)) & 255);
  return out;
}

export function genTiff(seed) {
  const r = new Rand(seed);
  const le = r.chance(0.7);
  const w = r.int(1, 14), h = r.int(1, 12);
  const bps = r.pick([8, 8, 8, 1, 2, 4, 16, 8, 3]);
  const photo = r.pick([2, 2, 1, 0, 3, 5, 2, 1, 6]);
  const spp = photo === 2 ? r.pick([3, 3, 4]) : photo === 5 ? 4 : photo === 6 ? 3 : r.pick([1, 1, 2]);
  const comp = r.pick([1, 1, 32773, 5, 8, 32946, 5, 32773, 7]);
  const pred = bps === 8 && comp !== 1 ? r.pick([1, 2]) : 1;
  const planar = r.chance(0.05) ? 2 : 1;
  const tiled = r.chance(0.15);
  const bw = tiled ? r.pick([4, 8, 16]) : w;
  const bh = tiled ? r.pick([4, 8]) : r.pick([h, Math.max(1, h >> 1), 1, 5]);
  const across = Math.ceil(w / bw);
  const down = Math.ceil(h / bh);
  const nBlocks = tiled ? across * down : down;
  const rowBytes = Math.ceil((bw * spp * bps) / 8);
  const blocks = [];
  for (let b = 0; b < nBlocks; b++) {
    let raw = Array.from({ length: rowBytes * bh }, () => r.pick([0, 255, r.below(256), 128]));
    if (pred === 2) for (let y = 0; y < bh; y++) for (let i = rowBytes - 1; i >= spp; i--) raw[y * rowBytes + i] = (raw[y * rowBytes + i] - raw[y * rowBytes + i - spp]) & 255;
    let enc = raw;
    if (comp === 32773) enc = packBitsEnc(raw);
    else if (comp === 5) enc = lzwEnc(raw);
    else if (comp === 8 || comp === 32946) enc = [...deflateSync(Buffer.from(raw))];
    else if (comp === 7) enc = raw; // an unknown compression
    if (r.chance(0.03)) enc = enc.slice(0, Math.max(0, enc.length - r.int(1, 4)));
    blocks.push(enc);
  }
  const buf = [];
  const put16 = (v) => (le ? buf.push(v & 255, (v >> 8) & 255) : buf.push((v >> 8) & 255, v & 255));
  const put32 = (v) => (le ? buf.push(v & 255, (v >>> 8) & 255, (v >>> 16) & 255, (v >>> 24) & 255) : buf.push((v >>> 24) & 255, (v >>> 16) & 255, (v >>> 8) & 255, v & 255));
  // header + blocks first, then the IFD at the end
  buf.push(...(le ? [0x49, 0x49] : [0x4d, 0x4d])); put16(42); put32(0);
  const offs = [], cnts = [];
  for (const b of blocks) { offs.push(buf.length); cnts.push(b.length); buf.push(...b); }
  const entries = [];
  const add = (tag, type, vals) => entries.push([tag, type, vals]);
  add(256, 3, [w]); add(257, 3, [h]); add(258, 3, spp > 1 ? Array(spp).fill(bps) : [bps]); add(259, 3, [comp]); add(262, 3, [photo]);
  add(tiled ? 324 : 273, 4, offs); add(277, 3, [spp]);
  if (!tiled) { add(278, 3, [bh]); add(279, 4, cnts); } else { add(322, 3, [bw]); add(323, 3, [bh]); add(325, 4, cnts); }
  if (planar !== 1 || r.chance(0.1)) add(284, 3, [planar]);
  if (pred !== 1) add(317, 3, [pred]);
  if (photo === 3) add(320, 3, Array.from({ length: 3 * (1 << Math.min(bps, 8)) }, () => r.below(65536)));
  if (spp === 2 || spp === 4) if (r.chance(0.6)) add(338, 3, [r.pick([1, 2])]);
  entries.sort((a, b) => a[0] - b[0]);
  const ifd = buf.length;
  const sizes = { 3: 2, 4: 4 };
  put16(entries.length);
  let extra = ifd + 2 + entries.length * 12 + 4;
  const tail = [];
  for (const [tag, type, vals] of entries) {
    put16(tag); put16(type); put32(vals.length);
    const sz = sizes[type] * vals.length;
    if (sz <= 4) { for (const v of vals) (type === 3 ? put16 : put32)(v); for (let i = sz; i < 4; i++) buf.push(0); }
    else { put32(extra); for (const v of vals) { if (type === 3) (le ? tail.push(v & 255, (v >> 8) & 255) : tail.push((v >> 8) & 255, v & 255)); else (le ? tail.push(v & 255, (v >>> 8) & 255, (v >>> 16) & 255, (v >>> 24) & 255) : tail.push((v >>> 24) & 255, (v >>> 16) & 255, (v >>> 8) & 255, v & 255)); } extra += sz; }
  }
  put32(0);
  buf.push(...tail);
  // the IFD offset
  const o = ifd;
  if (le) { buf[4] = o & 255; buf[5] = (o >> 8) & 255; buf[6] = (o >> 16) & 255; buf[7] = (o >>> 24) & 255; } else { buf[7] = o & 255; buf[6] = (o >> 8) & 255; buf[5] = (o >> 16) & 255; buf[4] = (o >>> 24) & 255; }
  return Uint8Array.from(buf);
}
