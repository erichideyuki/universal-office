// B5b synthetic QuickDraw PICT (generator group `b5b`): version 1 and 2 files with the opcodes `pict.ts` plays. Big endian.
// The TS and the Rust player read the same bytes, so the files only have to reach every opcode and branch (shapes and the
// "same" forms, pen and pattern state, colours, text with the Mac fonts, bitmaps of every depth and packing, comments).
import { Rand } from './rand.mjs';

class BE {
  constructor() { this.a = []; }
  get length() { return this.a.length; }
  u8(v) { this.a.push(v & 0xff); return this; }
  u16(v) { this.a.push((v >> 8) & 0xff, v & 0xff); return this; }
  u32(v) { this.a.push((v >>> 24) & 0xff, (v >>> 16) & 0xff, (v >>> 8) & 0xff, v & 0xff); return this; }
  rect(t, l, b, r) { return this.u16(t).u16(l).u16(b).u16(r); }
  bytes(x) { for (const v of x) this.a.push(v & 0xff); return this; }
  even() { if (this.a.length % 2) this.a.push(0); return this; }
}

const MAC_TEXT = [[72, 101, 108, 108, 111], [65, 66, 67, 32, 120, 121, 122], [0x8a, 0x8e, 0xa5, 0xc9, 0xd2, 0xd3], [0x20], [0xb0, 0xb1, 0xf0, 0x7f, 0x80], [84, 101, 120, 116, 111, 33]];

function packBits(row) {
  const out = [];
  let i = 0;
  while (i < row.length) {
    let run = 1;
    while (i + run < row.length && row[i + run] === row[i] && run < 128) run++;
    if (run >= 3) { out.push((257 - run) & 0xff, row[i]); i += run; continue; }
    const start = i;
    let n = 0;
    while (i < row.length && n < 128 && !(i + 2 < row.length && row[i] === row[i + 1] && row[i] === row[i + 2])) { i++; n++; }
    if (n === 0) { n = 1; i++; }
    out.push(n - 1, ...row.slice(start, start + n));
  }
  return out;
}

/** A PICT file. `ops` is a builder callback over the body; `v2` selects the version and the header. */
export function pictFile(body, { v2 = true, frame = [0, 0, 200, 300], src = null, file512 = false } = {}) {
  const b = new BE();
  if (file512) for (let i = 0; i < 512; i++) b.u8(0);
  b.u16(0).rect(...frame);
  if (v2) {
    b.u16(0x0011).u16(0x02ff).u16(0x0c00);
    b.u16(0xfffe).u16(0).u32(0x00480000).u32(0x00480000).rect(...(src ?? frame)).u32(0);
    // the TS reads the source rectangle at `body + 2 + 16`: write it there too
    b.bytes(body);
  } else {
    b.u8(0x11).u8(0x01);
    b.bytes(body);
  }
  b.u16(0x00ff);
  return new Uint8Array(b.a);
}

export function genPict(seed) {
  const r = new Rand(seed);
  const v2 = r.chance(0.8);
  const W = 300, H = 200;
  const body = new BE();
  const op = (n) => { if (v2) { if (body.length % 2) body.u8(0); body.u16(n); } else body.u8(n & 0xff); };
  const px = () => r.below(W);
  const py = () => r.below(H);
  const rc = () => { const t = py(), l = px(); return [t, l, t + r.int(1, 120), l + r.int(1, 160)]; };
  const pat = () => Array.from({ length: 8 }, () => r.pick([0, 0xff, 0xaa, 0x55, r.below(256)]));
  const n = r.int(4, 36);
  for (let i = 0; i < n; i++) {
    const k = r.weighted([['shape', 14], ['same', 5], ['pen', 4], ['pat', 3], ['color', 4], ['text', 8], ['line', 5], ['poly', 4], ['rgn', 2], ['clip', 3], ['bits', 6], ['font', 3], ['misc', 4], ['origin', 1], ['comment', 3], ['pixpat', 2], ['rot', 3]]);
    switch (k) {
      case 'shape': { const base = r.pick([0x30, 0x40, 0x50, 0x60]); op(base + r.below(8)); body.rect(...rc()); if (base === 0x60) body.u16(r.below(360)).u16(r.pick([90, -90, 180, 270, 45, 0xff00])); break; }
      case 'same': { const base = r.pick([0x38, 0x48, 0x58, 0x68]); op(base + r.below(8)); if (base === 0x68) body.u16(r.below(360)).u16(r.pick([90, -90, 180])); break; }
      case 'pen': { const o = r.pick([0x07, 0x08, 0x0b]); op(o); if (o === 0x08) body.u16(r.pick([8, 9, 10, 2, 14, 6, 0, 1])); else body.u16(r.below(6)).u16(r.below(6)); break; }
      case 'pat': op(r.pick([0x02, 0x09, 0x0a])); body.bytes(pat()); break;
      case 'color': r.chance(0.5) ? (op(r.pick([0x1a, 0x1b])), body.u16(r.below(65536)).u16(r.below(65536)).u16(r.below(65536))) : (op(r.pick([0x0e, 0x0f])), body.u32(r.pick([33, 30, 205, 341, 409, 273, 137, 69, 5]))); break;
      case 'text': {
        const t = r.pick(MAC_TEXT);
        const o = r.pick([0x28, 0x29, 0x2a, 0x2b]);
        op(o);
        if (o === 0x28) body.u16(py() + 20).u16(px());
        else if (o === 0x29) body.u8(r.below(100));
        else if (o === 0x2a) body.u8(r.below(100));
        else body.u8(r.below(100)).u8(r.below(50));
        body.u8(t.length).bytes(t);
        break;
      }
      case 'line': { const o = r.pick([0x20, 0x21, 0x22, 0x23]); op(o); if (o === 0x20) body.u16(py()).u16(px()).u16(py()).u16(px()); else if (o === 0x21) body.u16(py()).u16(px()); else if (o === 0x22) body.u16(py()).u16(px()).u8(r.below(40)).u8(r.below(40)); else body.u8(r.below(40)).u8(r.below(40)); break; }
      case 'poly': { const pts = Array.from({ length: r.int(2, 6) }, () => [py(), px()]); op(0x70 + r.below(8)); body.u16(10 + pts.length * 4).rect(...rc()); for (const [y, x] of pts) body.u16(y).u16(x); break; }
      case 'rgn': op(0x80 + r.below(8)); body.u16(10).rect(...rc()); break;
      case 'clip': op(0x01); body.u16(10).rect(...rc()); break;
      case 'bits': pictBits(r, op, body, W, H); break;
      case 'font': { if (r.chance(0.5)) { op(0x03); body.u16(r.pick([0, 1, 2, 3, 4, 20, 21, 22, 23, 99, 1000])); } else { op(0x2c); const nm = r.pick(['Palatino', 'Helvetica', 'Geneva', 'Monaco', 'Zapf Chancery', 'Avant Garde', 'Foo']); body.u16(5 + nm.length).u16(r.pick([1000, 3, 21])).u8(nm.length).bytes([...nm].map((c) => c.charCodeAt(0))); } break; }
      case 'misc': { const o = r.pick([0x04, 0x05, 0x0d, 0x00, 0x06, 0x10, 0x11, 0x15, 0x16, 0x17, 0x1c, 0x1d, 0x1e, 0x1f, 0x0c00]); op(o); if (o === 0x04) body.u8(r.below(8)); else if (o === 0x05 || o === 0x0d || o === 0x15 || o === 0x16) body.u16(r.below(20)); else if (o === 0x06) body.u32(0); else if (o === 0x10) body.bytes(new Array(8).fill(0)); else if (o === 0x11) body.u8(1); else if (o === 0x1d || o === 0x1f) body.bytes(new Array(6).fill(1)); else if (o === 0x0c00) body.bytes(new Array(24).fill(0)); break; }
      case 'origin': op(0x0c); body.u16(r.below(20)).u16(r.below(20)); break;
      case 'comment': {
        if (r.chance(0.5)) { op(0xa1); const kind = r.pick([150, 151, 154, 100]); const data = kind === 150 ? [0, 0, 0, r.pick([0, 45, 90, 20]), 0, 0, ...(r.chance(0.5) ? [0, 0x2d, 0, 0] : [])] : kind === 154 ? [0, 5, 0, 0, 0, 3, 0, 0] : [1, 2]; body.u16(kind).u16(data.length).bytes(data); if (data.length % 2) body.u8(0); } else { op(0xa0); body.u16(r.pick([151, 100])); }
        break;
      }
      case 'rot': {
        const ang = r.pick([20, 45, 90, 0, 270]);
        const fixedAng = r.chance(0.5) ? ang * 65536 : 0;
        op(0xa1); body.u16(150).u16(10).u16(0).u16(ang).u8(0).u8(0).u32(fixedAng);
        if (r.chance(0.6)) { op(0xa1); body.u16(154).u16(8).u32(r.below(20) * 65536).u32(r.below(30) * 65536); }
        const t = r.pick(MAC_TEXT);
        op(0x28); body.u16(py() + 20).u16(px()).u8(t.length).bytes(t);
        if (r.chance(0.5)) { op(0xa0); body.u16(151); } else { op(0xa1); body.u16(151).u16(0); }
        break;
      }
      case 'pixpat': {
        op(r.pick([0x12, 0x13, 0x14]));
        const t = r.pick([1, 2]);
        body.u16(t).bytes(pat());
        if (t === 2) body.u16(r.below(65536)).u16(r.below(65536)).u16(r.below(65536));
        else {
          const ps = r.pick([1, 2, 4, 8]);
          const rb = r.pick([1, 2, 10]);
          body.u16(0x8000 | rb).rect(0, 0, 4, 8).u16(0).u16(0).u32(0).u32(0x480000).u32(0x480000).u16(0).u16(ps).u16(1).u16(ps).u32(0).u32(0).u32(0);
          const n = 1 << Math.min(ps, 2);
          body.u32(0).u16(0).u16(n - 1);
          for (let c = 0; c < n; c++) body.u16(c).u16(r.below(65536)).u16(r.below(65536)).u16(r.below(65536));
          for (let y = 0; y < 4; y++) {
            const row = Array.from({ length: rb }, () => r.pick([0, 255, r.below(256)]));
            if (rb < 8) body.bytes(row); else { const d = packBits(row); body.u8(d.length).bytes(d); }
          }
        }
        break;
      }
      default: break;
    }
    // operands the 'pen'/'misc' cases did not write
    if (k === 'pen') { const last = body.a.length; void last; }
  }
  // a pen/size case writes its operand here (kept simple: a fixed one per opcode would need the opcode; random bytes are fine for parity)
  return pictFile(body.a, { v2, frame: [0, 0, H, W], src: r.chance(0.3) ? [0, 0, H * 2, W * 2] : null, file512: r.chance(0.2) });
}

/** BitsRect / PackBitsRect / DirectBitsRect (and the Rgn forms) of every depth. */
function pictBits(r, op, body, W, H) {
  const o = r.pick([0x90, 0x91, 0x98, 0x99, 0x9a, 0x9b, 0x98, 0x9a]);
  const direct = o === 0x9a || o === 0x9b;
  const bw = r.int(1, 24), bh = r.int(1, 12);
  const pixelSize = direct ? r.pick([16, 32]) : o === 0x90 || o === 0x91 ? 1 : r.pick([1, 2, 4, 8]);
  const pix = direct || o === 0x98 || o === 0x99 || r.chance(0.3);
  const packType = direct ? (pixelSize === 32 ? r.pick([0, 1, 2, 3, 4]) : r.pick([0, 1, 3])) : r.pick([0, 1, 0]);
  const cmp = pixelSize === 32 ? r.pick([3, 4]) : 1;
  const rowB = r.chance(0.8) ? (pixelSize === 32 ? bw * 4 : Math.ceil((bw * pixelSize) / 8)) : 0;
  op(o);
  if (direct) body.u32(0);
  body.u16((pix ? 0x8000 : 0) | rowB).rect(0, 0, bh, bw);
  if (pix) {
    body.u16(0).u16(packType).u32(0).u32(0x00480000).u32(0x00480000).u16(direct ? 16 : 0).u16(pixelSize).u16(cmp).u16(pixelSize === 16 ? 5 : 8).u32(0).u32(0).u32(0);
    if (!direct) {
      const n = Math.min(255, (1 << pixelSize) - 1);
      body.u32(0).u16(r.pick([0, 0x8000])).u16(n);
      for (let k = 0; k <= n; k++) body.u16(k).u16(r.below(65536)).u16(r.below(65536)).u16(r.below(65536));
    }
  }
  body.rect(0, 0, bh, bw).rect(r.below(H / 2), r.below(W / 2), r.below(H / 2) + 20, r.below(W / 2) + 20).u16(r.pick([0, 1, 3, 0x40]));
  if (o === 0x91 || o === 0x99 || o === 0x9b) body.u16(10).rect(0, 0, bh, bw);
  const rb = rowB || (pixelSize === 32 ? bw * 4 : Math.ceil((bw * pixelSize) / 8));
  const packed = o !== 0x90 && o !== 0x91 && rb >= 8 && packType !== 1 && !(pixelSize === 32 && packType === 2);
  for (let y = 0; y < bh; y++) {
    let rowLen = pixelSize === 32 && packType === 2 ? bw * 3 : pixelSize === 32 && packType === 4 ? bw * cmp : rb;
    const row = Array.from({ length: rowLen }, () => r.pick([0, 255, r.below(256), 0x80]));
    if (!packed) { body.bytes(row); continue; }
    let data = packBits(row);
    if (pixelSize === 16 && packType === 3) data = packBits(row.filter((_, i) => i % 2 === 0)).concat([]);
    if (rb > 250) body.u16(data.length); else body.u8(data.length);
    body.bytes(data);
  }
  body.even();
}
