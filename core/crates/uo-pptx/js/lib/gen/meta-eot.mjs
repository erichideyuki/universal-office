// B5b synthetic EOT (generator group `b5b`): Embedded OpenType files with synthetic fonts built in code (no font files at all): plain,
// XOR-encrypted and MicroType Express compressed (an encoder for the three LZCOMP blocks with the adaptive Huffman, the LZ77 copies, the
// DUP2/4/6 symbols and the optional RLE, and the CTF form of the glyphs: triplets, composites, push data in hop codes, `cvt ` deltas).
// The oracle is the frozen TS `eotToSfnt`; the files only have to be what the decoder reads, not fonts a program would open.
import { Rand } from './rand.mjs';

class Bits {
  constructor() { this.a = []; this.cur = 0; this.n = 0; }
  bit(b) { this.cur = (this.cur << 1) | (b & 1); if (++this.n === 8) { this.a.push(this.cur); this.cur = 0; this.n = 0; } }
  value(v, n) { for (let i = n - 1; i >= 0; i--) this.bit(Math.floor(v / 2 ** i) & 1); }
  done() { const a = [...this.a]; if (this.n) a.push((this.cur << (8 - this.n)) & 0xff); return a; }
}

/** The adaptive Huffman of LZCOMP, encoder side (the same tree as the decoder's). */
class AHuffEnc {
  constructor(bits, range) {
    this.bits = bits;
    const n = 2 * range;
    this.up = new Int32Array(n); this.left = new Int32Array(n); this.right = new Int32Array(n); this.code = new Int32Array(n);
    this.weight = new Float64Array(n); this.symbolIndex = new Int32Array(range);
    for (let i = 2; i < n; i++) { this.up[i] = i >> 1; this.weight[i] = 1; }
    for (let i = 1; i < range; i++) { this.left[i] = 2 * i; this.right[i] = 2 * i + 1; }
    for (let i = 0; i < range; i++) { this.code[i] = -1; this.code[range + i] = i; this.left[range + i] = this.right[range + i] = -1; this.symbolIndex[i] = range + i; }
    this.weight[0] = Number.MAX_SAFE_INTEGER;
    this.initWeight(1);
    if (range > 256 && range < 512) {
      this.update(this.symbolIndex[256]); this.update(this.symbolIndex[257]);
      for (let i = 0; i < 12; i++) this.update(this.symbolIndex[range - 3]);
      for (let i = 0; i < 6; i++) this.update(this.symbolIndex[range - 2]);
    } else for (let j = 0; j < 2; j++) for (let i = 0; i < range; i++) this.update(this.symbolIndex[i]);
  }
  initWeight(a) { if (this.code[a] < 0) this.weight[a] = this.initWeight(this.left[a]) + this.initWeight(this.right[a]); return this.weight[a]; }
  swap(a, b) {
    const { up, left, right, code, weight, symbolIndex } = this;
    const upa = up[a], upb = up[b];
    [left[a], left[b]] = [left[b], left[a]]; [right[a], right[b]] = [right[b], right[a]];
    [code[a], code[b]] = [code[b], code[a]]; [weight[a], weight[b]] = [weight[b], weight[a]];
    up[a] = upa; up[b] = upb;
    for (const k of [a, b]) { if (code[k] < 0) { up[left[k]] = k; up[right[k]] = k; } else symbolIndex[code[k]] = k; }
  }
  update(a) {
    const w = this.weight;
    for (; a !== 1; a = this.up[a]) {
      const wa = w[a];
      let b = a - 1;
      if (w[b] === wa) { do { b--; } while (w[b] === wa); b++; if (b > 1) { this.swap(a, b); a = b; } }
      w[a] = wa + 1;
    }
    w[1] += 1;
  }
  encode(sym) {
    const leaf = this.symbolIndex[sym];
    const path = [];
    for (let a = leaf; a !== 1; a = this.up[a]) path.push(this.right[this.up[a]] === a ? 1 : 0);
    for (let i = path.length - 1; i >= 0; i--) this.bits.bit(path[i]);
    this.update(leaf);
  }
}

const PRELOAD_LEN = 2 * 32 * 96 + 4 * 256;

/** One LZCOMP block of `data` (bytes) in the given MTX version; `rle` the run-length layer. */
export function lzcompPack(data, version, rle, r) {
  let bytes = data;
  if (rle && version !== 1) {
    const esc = r.below(256);
    bytes = [esc];
    for (let i = 0; i < data.length;) {
      let run = 1;
      while (i + run < data.length && data[i + run] === data[i] && run < 255) run++;
      if (run >= 4) { bytes.push(esc, run, data[i]); i += run; } else { for (let k = 0; k < run; k++) { if (data[i] === esc) bytes.push(esc, 0); else bytes.push(data[i]); } i += run; }
    }
  }
  const outLen = bytes.length;
  const bits = new Bits();
  if (version !== 1) bits.bit(rle ? 1 : 0);
  const dist = new AHuffEnc(bits, 8), len = new AHuffEnc(bits, 8);
  bits.value(outLen, 24);
  let numDist = 1;
  while (1 + 2 ** (3 * numDist) - 1 < outLen) numDist++;
  const DUP2 = 256 + 8 * numDist, DUP4 = DUP2 + 1, DUP6 = DUP4 + 1;
  const sym = new AHuffEnc(bits, DUP6 + 1);
  let pos = 0;
  while (pos < outLen) {
    // a copy: the longest earlier run (not overlapping `pos`), at least 3 bytes
    let bestL = 0, bestS = 0;
    for (let s = Math.max(0, pos - 2048); s < pos; s++) {
      let L = 0;
      while (pos + L < outLen && s + L < pos && bytes[s + L] === bytes[pos + L] && L < 40) L++;
      if (L > bestL) { bestL = L; bestS = s; }
    }
    const useCopy = bestL >= 3 && r.chance(0.85);
    if (useCopy) {
      const L = bestL;
      let distance = pos - (bestS + L - 1);
      const coded = distance >= 512 ? L - 3 : L - 2;
      if (coded >= 0) {
        const digits = [];
        let c = coded;
        do { digits.unshift(c & 3); c >>= 2; } while (c > 0);
        const dd = [];
        let dv = distance - 1;
        do { dd.unshift(dv & 7); dv = Math.floor(dv / 8); } while (dv > 0);
        const k = digits.length - 1;
        sym.encode(256 + 8 * (dd.length - 1) + ((k > 0 ? 4 : 0) | digits[0]));
        for (let i = 1; i <= k; i++) len.encode((i < k ? 4 : 0) | digits[i]);
        for (const d of dd) dist.encode(d);
        pos += Math.min(distance >= 512 ? L : L, outLen - pos);
        continue;
      }
    }
    const v = bytes[pos];
    if (pos >= 6 && v === bytes[pos - 6] && r.chance(0.5)) sym.encode(DUP6);
    else if (pos >= 4 && v === bytes[pos - 4] && r.chance(0.5)) sym.encode(DUP4);
    else if (pos >= 2 && v === bytes[pos - 2] && r.chance(0.5)) sym.encode(DUP2);
    else sym.encode(v);
    pos++;
  }
  void PRELOAD_LEN;
  return bits.done();
}

const u255 = (v) => (v < 253 ? [v] : v < 506 ? [255, v - 253] : v < 762 ? [254, v - 506] : [253, v >> 8, v & 255]);
const s255 = (v) => {
  if (v >= 0 && v < 250) return [v];
  if (v < 0 && -v < 250) return [250, -v];
  if (v >= 250 && v < 500) return [255, v - 250];
  if (v >= 500 && v < 750) return [254, v - 500];
  if (v < 0 && -v < 500) return [250, 255, -v - 250];
  if (v < 0 && -v < 750) return [250, 254, -v - 500];
  return [253, (v >> 8) & 255, v & 255];
};

/** The triplet of one point delta (the form is chosen by the magnitudes, a random one among those that fit). */
function triplet(r, dx, dy, onCurve) {
  const off = onCurve ? 0 : 0x80;
  const ax = Math.abs(dx), ay = Math.abs(dy);
  const xp = dx >= 0 ? 1 : 0, yp = dy >= 0 ? 1 : 0;
  const forms = [];
  if (dx === 0 && ay < 1280) forms.push(() => { const k = Math.floor(ay / 256); return [off | (2 * k + yp), ay - k * 256]; });
  if (dy === 0 && ax < 1280) forms.push(() => { const k = Math.floor(ax / 256); return [off | (10 + 2 * k + xp), ax - k * 256]; });
  if (ax >= 1 && ax <= 64 && ay >= 1 && ay <= 64) forms.push(() => { const gx = (ax - 1) >> 4, hx = (ax - 1) & 15, gy = (ay - 1) >> 4, ly = (ay - 1) & 15; return [off | (20 + (gx << 4) + (gy << 2) + xp + 2 * yp), (hx << 4) | ly]; });
  if (ax >= 1 && ax <= 768 && ay >= 1 && ay <= 768) forms.push(() => { const gx = (ax - 1) >> 8, gy = (ay - 1) >> 8; return [off | (84 + 12 * gx + 4 * gy + xp + 2 * yp), (ax - 1) & 255, (ay - 1) & 255]; });
  if (ax < 4096 && ay < 4096) forms.push(() => [off | (120 + xp + 2 * yp), ax >> 4, ((ax & 15) << 4) | (ay >> 8), ay & 255]);
  forms.push(() => [off | (124 + xp + 2 * yp), ax >> 8, ax & 255, ay >> 8, ay & 255]);
  return r.pick(forms)();
}

/** A synthetic font description: glyphs (simple or composite), tables with random content, `cvt ` values. */
function synthFont(r) {
  const nGlyphs = r.int(2, 14);
  const glyphs = [];
  for (let g = 0; g < nGlyphs; g++) {
    if (g > 0 && r.chance(0.2)) {
      glyphs.push({ composite: true, comps: Array.from({ length: r.int(1, 3) }, () => ({ idx: r.below(nGlyphs), args: [r.below(100), r.below(100)], wide: r.chance(0.3), scale: r.pick([0, 0, 1, 2, 3]) })), instr: r.chance(0.3) });
    } else if (r.chance(0.12)) glyphs.push({ empty: true });
    else {
      const contours = Array.from({ length: r.int(1, 3) }, () => Array.from({ length: r.int(1, 7) }, () => ({ x: r.int(-300, 1500), y: r.int(-300, 1500), on: r.chance(0.7) })));
      glyphs.push({ contours, explicitBox: r.chance(0.15), push: r.chance(0.6) ? Array.from({ length: r.int(1, 14) }, () => r.pick([r.below(250), r.below(250), -r.below(240), 250 + r.below(250), 500 + r.below(250), r.below(30000), -300])) : [], code: Array.from({ length: r.below(6) }, () => r.below(256)) });
    }
  }
  const cvt = Array.from({ length: r.pick([0, 3, 10, 40]) }, () => r.pick([r.int(-30, 30), r.int(-3000, 3000), r.int(-20000, 20000), 0, 238, -238, 500]));
  const head = Array.from({ length: 54 }, () => r.below(256));
  head[50] = 0; head[51] = r.pick([0, 0, 1]);
  const maxp = [0, 1, 0, 0, (nGlyphs >> 8) & 255, nGlyphs & 255, ...Array.from({ length: 26 }, () => 0)];
  const tables = [['head', head], ['maxp', maxp]];
  for (const t of r.pickN(['OS/2', 'cmap', 'name', 'post', 'hhea', 'hmtx', 'fpgm', 'prep', 'hdmx', 'VDMX', 'GDEF'], r.int(2, 6))) tables.push([t, Array.from({ length: r.int(4, 120) }, () => r.below(256))]);
  return { nGlyphs, glyphs, cvt, tables };
}

const be16 = (v) => [(v >> 8) & 255, v & 255];
const be32 = (v) => [(v >>> 24) & 255, (v >>> 16) & 255, (v >>> 8) & 255, v & 255];

/** The three streams of the MTX of a synthetic font: the CTF, the push data, the instruction code. */
function ctfStreams(r, f) {
  const push = [], code = [];
  const glyf = [];
  for (const g of f.glyphs) {
    if (g.empty) { glyf.push(...be16(0)); continue; }
    if (g.composite) {
      glyf.push(...be16(0xffff), ...Array.from({ length: 8 }, () => r.below(256)));
      g.comps.forEach((c, i) => {
        const last = i === g.comps.length - 1;
        const flags = (c.wide ? 1 : 0) | (c.scale === 1 ? 8 : c.scale === 2 ? 0x40 : c.scale === 3 ? 0x80 : 0) | (last ? (g.instr ? 0x100 : 0) : 0x20) | 2;
        glyf.push(...be16(flags), ...be16(c.idx), ...(c.wide ? [...be16(c.args[0]), ...be16(c.args[1])] : [c.args[0], c.args[1]]));
        glyf.push(...Array.from({ length: c.scale === 1 ? 2 : c.scale === 2 ? 4 : c.scale === 3 ? 8 : 0 }, () => r.below(256)));
      });
      if (g.instr) { const n = r.below(6); glyf.push(...u255(1)); push.push(...s255(r.below(200))); glyf.push(...u255(n)); for (let i = 0; i < n; i++) code.push(r.below(256)); }
      continue;
    }
    const pts = g.contours.flat();
    const nPts = pts.length;
    if (g.explicitBox) glyf.push(...be16(0x7fff), ...be16(g.contours.length), ...[0, 0, 0, 0].flatMap(() => be16(r.below(2000)))); else glyf.push(...be16(g.contours.length));
    let total = 0;
    g.contours.forEach((c, i) => { total += c.length; glyf.push(...u255(i === 0 ? total - 1 : c.length)); });
    // flags then the triplet bytes, point by point (deltas)
    const flagBytes = [], tripBytes = [];
    let px = 0, py = 0;
    for (const p of pts) { const t = triplet(r, p.x - px, p.y - py, p.on); flagBytes.push(t[0]); tripBytes.push(...t.slice(1)); px = p.x; py = p.y; }
    glyf.push(...flagBytes, ...tripBytes);
    // instructions: PUSH data (values and hop codes) and code
    const toks = [];
    for (const v of g.push) toks.push(s255(v));
    let count = g.push.length;
    if (g.push.length >= 2 && r.chance(0.5)) { toks.push([0xfb, ...s255(r.below(100))]); count += 3; }
    if (g.push.length >= 2 && r.chance(0.4)) { toks.push([0xfc, ...s255(r.below(100)), ...s255(r.below(100))]); count += 5; }
    glyf.push(...u255(count));
    for (const t of toks) push.push(...t);
    glyf.push(...u255(g.code.length));
    code.push(...g.code);
    void nPts;
  }
  // the CTF: sfnt header, the table records, the data (glyf stream, cvt deltas and the other tables)
  const recs = [];
  const data = [];
  const all = [...f.tables.map(([t, d]) => [t, d]), ['glyf', glyf]];
  if (f.cvt.length) {
    const cv = [...be16(f.cvt.length)];
    let last = 0;
    for (const v of f.cvt) {
      const d = ((v - last + 0x8000) & 0xffff) - 0x8000;
      last = v;
      const a = Math.abs(d);
      if (d >= 0 && d < 238) cv.push(d);
      else if (d >= 238 && d < 238 * 9) { const k = Math.floor(d / 238); cv.push(247 + k, d - 238 * k); }
      else if (d < 0 && a < 238 * 9) { const k = Math.floor(a / 238); cv.push(239 + k, a - 238 * k); }
      else cv.push(238, ...be16(d & 0xffff));
    }
    all.push(['cvt ', cv]);
  }
  const hdr = 12 + 16 * all.length;
  let off = hdr;
  for (const [t, d] of all) { recs.push([t, off, d.length]); data.push(...d); off += d.length; }
  const ctf = [...be32(0x00010000), ...be16(all.length), 0, 0, 0, 0, 0, 0];
  for (const [t, o, l] of recs) ctf.push(...[...t.padEnd(4, ' ')].map((c) => c.charCodeAt(0)), ...be32(0), ...be32(o), ...be32(l));
  ctf.push(...data);
  return [ctf, push, code];
}

/** MTX container: version byte, three offsets, then the LZCOMP blocks (the header is 10 bytes). */
function mtxPack(r, f, version, rle) {
  const streams = ctfStreams(r, f);
  const blocks = streams.map((s) => lzcompPack(s, version, rle, r));
  const o2 = 10 + blocks[0].length, o3 = o2 + blocks[1].length;
  return [version, 0, 0, 0, (o2 >> 16) & 255, (o2 >> 8) & 255, o2 & 255, (o3 >> 16) & 255, (o3 >> 8) & 255, o3 & 255, ...blocks.flat()];
}

/** The EOT around font data: the 82-byte fixed header (+ a little name data), flags, and the data as the LAST bytes. */
function eotWrap(font, flags, pad) {
  const head = new Array(82).fill(0);
  const put32 = (o, v) => { for (let i = 0; i < 4; i++) head[o + i] = (v >>> (8 * i)) & 255; };
  const total = 82 + pad + font.length;
  put32(0, total); put32(4, font.length); put32(8, 0x00020002); put32(12, flags);
  head[34] = 0x4c; head[35] = 0x50;
  return Uint8Array.from([...head, ...new Array(pad).fill(0), ...font]);
}

export function genEot(seed) {
  const r = new Rand(seed);
  const kind = r.pick(['plain', 'xor', 'mtx', 'mtx', 'mtx-xor', 'mtx']);
  if (kind === 'plain' || kind === 'xor') {
    const font = Array.from({ length: r.int(10, 400) }, () => r.below(256));
    const flags = kind === 'xor' ? 0x10000000 : 0;
    const f = kind === 'xor' ? font.map((b) => b ^ 0x50) : font;
    return eotWrap(f, flags, r.pick([0, 20, 130]));
  }
  const f = synthFont(r);
  const version = r.pick([1, 2, 3]);
  const mtx = mtxPack(r, f, version, r.chance(0.4));
  const xor = kind === 'mtx-xor';
  return eotWrap(xor ? mtx.map((b) => b ^ 0x50) : mtx, 0x4 | (xor ? 0x10000000 : 0), r.pick([0, 20, 90]));
}
