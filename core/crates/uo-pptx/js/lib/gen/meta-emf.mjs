// B5b synthetic EMF and EMF+ (generator group `b5b`): seeded files built record by record from the layouts of [MS-EMF] and
// [MS-EMFPLUS] the frozen TS reads (`metafile.ts` playEmf, `emfplus.ts`). The TS and the Rust player read the SAME bytes, so a file
// does not have to be one a program would write: what counts is that every record type and branch of the players is reached with a
// range of values (coordinates, map modes, transforms, ROPs, brushes, pens, fonts, bitmaps, clips). Truncated and bit-flipped variants
// are made by `mutate` (`meta-cases.mjs`).
import { Rand } from './rand.mjs';
import { wmfBare } from './meta-wmf.mjs';

/** A little-endian byte writer. */
export class Bytes {
  constructor() { this.a = []; }
  get length() { return this.a.length; }
  u8(v) { this.a.push(v & 0xff); return this; }
  u16(v) { this.a.push(v & 0xff, (v >> 8) & 0xff); return this; }
  u32(v) { this.a.push(v & 0xff, (v >>> 8) & 0xff, (v >>> 16) & 0xff, (v >>> 24) & 0xff); return this; }
  i32(v) { return this.u32(v | 0); }
  f32(v) { const b = new DataView(new ArrayBuffer(4)); b.setFloat32(0, v, true); for (let i = 0; i < 4; i++) this.a.push(b.getUint8(i)); return this; }
  bytes(arr) { for (const x of arr) this.a.push(x & 0xff); return this; }
  pad4() { while (this.a.length % 4) this.a.push(0); return this; }
  utf16(s, n = 0) { for (let i = 0; i < (n || s.length); i++) this.u16(i < s.length ? s.charCodeAt(i) : 0); return this; }
  done() { return new Uint8Array(this.a); }
}

const rec = (type, build) => {
  const b = new Bytes();
  build?.(b);
  b.pad4();
  return new Bytes().u32(type).u32(8 + b.length).bytes(b.a);
};

/** EMF header + records + EOF. `frame` in 0.01 mm; the reference device is 1920 x 1080 px on 508 x 285.75 mm. */
export function emfFile(records, { frame = [0, 0, 21000, 29700], bounds = [0, 0, 794, 1123], dev = [1920, 1080], mm = [508, 286] } = {}) {
  const body = new Bytes();
  for (const r of records) body.bytes(r.a);
  body.bytes(rec(14, (b) => b.u32(0).u32(16).u32(20)).a);
  const h = new Bytes().u32(1).u32(88);
  for (const v of bounds) h.i32(v);
  for (const v of frame) h.i32(v);
  h.u32(0x464d4520).u32(0x10000).u32(88 + body.length).u32(records.length + 2).u16(8).u16(0).u32(0).u32(0).u32(0).i32(dev[0]).i32(dev[1]).i32(mm[0]).i32(mm[1]);
  return new Uint8Array([...h.a, ...body.a]);
}

export const colorref = (r, g, b) => (r | (g << 8) | (b << 16)) >>> 0;
const rect = (b, l, t, r, bt) => b.i32(l).i32(t).i32(r).i32(bt);

// --- record builders (the layouts are the TS's offsets) ---
export const R = {
  mapMode: (m) => rec(17, (b) => b.u32(m)),
  winExt: (x, y) => rec(9, (b) => b.i32(x).i32(y)),
  winOrg: (x, y) => rec(10, (b) => b.i32(x).i32(y)),
  vpExt: (x, y) => rec(11, (b) => b.i32(x).i32(y)),
  vpOrg: (x, y) => rec(12, (b) => b.i32(x).i32(y)),
  scaleVp: (a, b2, c, d) => rec(31, (b) => b.i32(a).i32(b2).i32(c).i32(d)),
  scaleWin: (a, b2, c, d) => rec(32, (b) => b.i32(a).i32(b2).i32(c).i32(d)),
  setWorld: (m) => rec(35, (b) => m.forEach((v) => b.f32(v))),
  modWorld: (m, mode) => rec(36, (b) => { m.forEach((v) => b.f32(v)); b.u32(mode); }),
  saveDc: () => rec(33),
  restoreDc: (n) => rec(34, (b) => b.i32(n)),
  bkMode: (m) => rec(18, (b) => b.u32(m)),
  polyFill: (m) => rec(19, (b) => b.u32(m)),
  rop2: (m) => rec(20, (b) => b.u32(m)),
  textAlign: (m) => rec(22, (b) => b.u32(m)),
  textColor: (c) => rec(24, (b) => b.u32(c)),
  bkColor: (c) => rec(25, (b) => b.u32(c)),
  createPen: (id, style, w, c) => rec(38, (b) => b.u32(id).u32(style).i32(w).i32(0).u32(c)),
  extPen: (id, style, w, c) => rec(95, (b) => { b.u32(id).u32(0).u32(0).u32(0).u32(0).u32(0).u32(style).u32(w).u32(0).u32(c).u32(0).u32(0); }),
  brush: (id, style, c, hatch = 0) => rec(39, (b) => b.u32(id).u32(style).u32(c).u32(hatch)),
  monoBrush: (type, id, dib) => rec(type, (b) => { b.u32(id).u32(0).u32(8 + 24).u32(dib.bmi.length).u32(8 + 24 + dib.bmi.length).u32(dib.bits.length); b.bytes(dib.bmi).bytes(dib.bits); }),
  font: (id, { height = -16, esc = 0, weight = 400, italic = 0, underline = 0, strike = 0, charset = 0, pitch = 0, face = 'Arial' } = {}) => rec(82, (b) => {
    b.u32(id).i32(height).i32(0).i32(esc).i32(0).i32(weight).u8(italic).u8(underline).u8(strike).u8(charset).u8(0).u8(0).u8(0).u8(pitch).utf16(face, 32);
    for (let i = 0; i < 64 + 64 + 8; i++) b.u8(0);
  }),
  select: (h) => rec(37, (b) => b.u32(h)),
  del: (h) => rec(40, (b) => b.u32(h)),
  moveTo: (x, y) => rec(27, (b) => b.i32(x).i32(y)),
  lineTo: (x, y) => rec(54, (b) => b.i32(x).i32(y)),
  rectangle: (l, t, r, bt) => rec(43, (b) => rect(b, l, t, r, bt)),
  roundRect: (l, t, r, bt, w, h) => rec(44, (b) => { rect(b, l, t, r, bt); b.i32(w).i32(h); }),
  ellipse: (l, t, r, bt) => rec(42, (b) => rect(b, l, t, r, bt)),
  arc: (type, l, t, r, bt, xs, ys, xe, ye) => rec(type, (b) => { rect(b, l, t, r, bt); b.i32(xs).i32(ys).i32(xe).i32(ye); }),
  poly16: (type, pts) => rec(type, (b) => { rect(b, 0, 0, 0, 0); b.u32(pts.length); for (const [x, y] of pts) b.u16(x).u16(y); }),
  poly32: (type, pts) => rec(type, (b) => { rect(b, 0, 0, 0, 0); b.u32(pts.length); for (const [x, y] of pts) b.i32(x).i32(y); }),
  polyPoly: (type, polys, wide) => rec(type, (b) => {
    rect(b, 0, 0, 0, 0); b.u32(polys.length); b.u32(polys.reduce((s, p) => s + p.length, 0));
    for (const p of polys) b.u32(p.length);
    for (const p of polys) for (const [x, y] of p) { if (wide) b.i32(x).i32(y); else b.u16(x).u16(y); }
  }),
  beginPath: () => rec(59), endPath: () => rec(60), closeFigure: () => rec(61), abortPath: () => rec(68),
  fillPath: () => rec(62, (b) => rect(b, 0, 0, 0, 0)), strokeFill: () => rec(63, (b) => rect(b, 0, 0, 0, 0)), strokePath: () => rec(64, (b) => rect(b, 0, 0, 0, 0)),
  clipPath: (mode) => rec(67, (b) => b.u32(mode)),
  isectClip: (l, t, r, bt) => rec(30, (b) => rect(b, l, t, r, bt)),
  exclClip: (l, t, r, bt) => rec(29, (b) => rect(b, l, t, r, bt)),
  pixel: (x, y, c) => rec(15, (b) => b.i32(x).i32(y).u32(c)),
  selClipRgn: (mode, rects) => rec(75, (b) => {
    b.u32(32 + rects.length * 16).u32(mode).u32(32).u32(1).u32(rects.length).u32(rects.length * 16); rect(b, 0, 0, 0, 0);
    for (const q of rects) rect(b, ...q);
  }),
  text: (type, { x, y, text, opts = 0, align, dx = null, rcl = [0, 0, 0, 0], withDx = true }) => rec(type, (b) => {
    const wide = type === 84;
    const n = text.length;
    const offStr = 76 + 8;
    const strBytes = wide ? n * 2 : n;
    b.i32(0).i32(0).i32(0).i32(0).u32(1).u32(0x3f800000).u32(0x3f800000);
    b.i32(x).i32(y).u32(n).u32(offStr).u32(opts); rect(b, ...rcl);
    const offDx = dx || withDx ? offStr + ((strBytes + 3) & ~3) : 0;
    b.u32(dx === false ? 0 : offDx);
    if (wide) b.utf16(text); else b.bytes([...text].map((c) => c.charCodeAt(0) & 0xff));
    b.pad4();
    if (offDx) for (let i = 0; i < n; i++) b.i32(Array.isArray(dx) ? dx[i] : 10);
    void align;
  }),
  gradient: (verts, idx, mode) => rec(118, (b) => {
    rect(b, 0, 0, 0, 0); b.u32(verts.length).u32(idx.length).u32(mode);
    for (const v of verts) b.i32(v.x).i32(v.y).u16(v.r << 8).u16(v.g << 8).u16(v.b << 8).u16(0);
    for (const i of idx) for (const k of i) b.u32(k);
  }),
  comment: (data) => rec(70, (b) => b.u32(data.length).bytes(data)),
};

/** A packed DIB (BITMAPINFOHEADER + palette + bits) `w` x `h` at `bpp`, bottom-up unless `topDown`. */
export function dib(r, w, h, bpp, { topDown = false, comp = 0, masks = null, core = false } = {}) {
  if (core) {
    // the OS/2 BITMAPCOREHEADER: size 12, width, height (u16), planes, bpp; palette entries of 3 bytes
    const bmi = new Bytes().u32(12).u16(w).u16(h).u16(1).u16(bpp);
    if (bpp <= 8) for (let i = 0; i < 1 << bpp; i++) bmi.u8(r.below(256)).u8(r.below(256)).u8(r.below(256));
    const stride = Math.ceil((w * bpp) / 32) * 4;
    return { bmi: bmi.a, bits: Array.from({ length: stride * h }, () => r.below(256)) };
  }
  if (comp === 1 || comp === 2) {
    const bmi = new Bytes().u32(40).i32(w).i32(topDown ? -h : h).u16(1).u16(comp === 1 ? 8 : 4).u32(comp).u32(0).i32(0).i32(0).u32(0).u32(0);
    for (let i = 0; i < (comp === 1 ? 256 : 16); i++) bmi.u8(r.below(256)).u8(r.below(256)).u8(r.below(256)).u8(0);
    // random RLE: encoded runs, absolute runs, end of line, delta, end of bitmap
    const bits = [];
    for (let i = 0; i < r.int(4, 30); i++) {
      const k = r.weighted([['run', 6], ['abs', 3], ['eol', 2], ['delta', 1], ['eob', 1]]);
      if (k === 'run') bits.push(r.int(1, 8), r.below(256));
      else if (k === 'abs') { const n = r.int(3, 6); bits.push(0, n); for (let j = 0; j < (comp === 1 ? n : (n + 1) >> 1); j++) bits.push(r.below(256)); if ((comp === 1 ? n : (n + 1) >> 1) % 2) bits.push(0); }
      else if (k === 'eol') bits.push(0, 0);
      else if (k === 'delta') bits.push(0, 2, r.below(4), r.below(3));
      else bits.push(0, 1);
    }
    return { bmi: bmi.a, bits };
  }
  const bmi = new Bytes().u32(40).i32(w).i32(topDown ? -h : h).u16(1).u16(bpp).u32(comp).u32(0).i32(0).i32(0).u32(0).u32(0);
  let palN = bpp <= 8 ? 1 << bpp : 0;
  if (bpp <= 8) for (let i = 0; i < palN; i++) bmi.u8(r.below(256)).u8(r.below(256)).u8(r.below(256)).u8(0);
  if (comp === 3 && masks) for (const m of masks) bmi.u32(m);
  void palN;
  const stride = Math.ceil((w * bpp) / 32) * 4;
  const bits = [];
  for (let i = 0; i < stride * h; i++) bits.push(r.below(256));
  return { bmi: bmi.a, bits };
}

/** EMF with a DIB record of the given kind. */
function blitRec(r, kind, d, x, y, cx, cy) {
  const rop = r.pick([0xcc0020, 0x8800c6, 0xee0086, 0x660046, 0x330008, 0xaa0029, 0xf00021, 0x5a0049, 0x550009, 0x000042, 0xff0062]);
  if (kind === 'bitblt' || kind === 'stretchblt') {
    return rec(kind === 'bitblt' ? 76 : 77, (b) => {
      rect(b, 0, 0, 0, 0); b.i32(x).i32(y).i32(cx).i32(cy).u32(rop).i32(r.below(4)).i32(r.below(4));
      for (let i = 0; i < 6; i++) b.f32(i === 0 || i === 3 ? 1 : 0);
      b.u32(0).u32(0);
      const hasDib = d !== null;
      const offBmi = 100 + (kind === 'stretchblt' ? 8 : 0) - 8 + 8;
      b.u32(hasDib ? offBmi : 0).u32(hasDib ? d.bmi.length : 0).u32(hasDib ? offBmi + d.bmi.length : 0).u32(hasDib ? d.bits.length : 0);
      if (kind === 'stretchblt') b.i32(r.pick([0, 8, 16])).i32(r.pick([0, 8, 16]));
      if (hasDib) b.bytes(d.bmi).bytes(d.bits);
    });
  }
  return null;
}

const COLORS = () => [];

/** A random drawing session of `n` records in a `W` x `H` logical area. */
function drawing(r, W, H, n, st) {
  const recs = [];
  const px = () => r.below(W);
  const py = () => r.below(H);
  const pts = (k, wide) => Array.from({ length: k }, () => [px(), py()]).map(([x, y]) => (wide ? [x, y] : [x & 0xffff, y & 0xffff]));
  const col = () => colorref(r.below(256), r.below(256), r.below(256));
  const palette = ['Arial', 'Times New Roman', 'Courier New', 'Symbol', 'Wingdings', 'Segoe UI', 'Helvetica', 'Tahoma', 'MS Sans Serif', 'Webdings', 'Cambria Math', 'Calibri'];
  let nextH = st.nextH ?? 1;
  const newObj = () => nextH++;
  for (let i = 0; i < n; i++) {
    const k = r.weighted([
      ['pen', 5], ['brush', 5], ['line', 6], ['rect', 7], ['round', 3], ['ellipse', 4], ['arc', 3], ['poly16', 5], ['poly32', 3], ['polyline', 3], ['bez', 3], ['polypoly', 3],
      ['path', 4], ['text', 8], ['font', 4], ['clip', 3], ['mode', 3], ['dc', 2], ['bit', 3], ['pixel', 1], ['world', 3], ['rop2', 1], ['grad', 2], ['stock', 2], ['monobrush', 1], ['rgn', 1],
    ]);
    switch (k) {
      case 'pen': { const h = newObj(); recs.push(r.chance(0.4) ? R.extPen(h, r.pick([0, 1, 2, 3, 4, 5, 0x100, 0x200 | 0, 0x1000, 0x2000, 0x100 | 0x1000, 0x2 | 0x200 | 0x2000]), r.below(30), col()) : R.createPen(h, r.below(7), r.below(40), col()), R.select(h)); break; }
      case 'brush': { const h = newObj(); recs.push(R.brush(h, r.pick([0, 0, 1, 2, 2, 3]), col(), r.below(7)), R.select(h)); break; }
      case 'line': recs.push(R.moveTo(px(), py()), R.lineTo(px(), py())); break;
      case 'rect': recs.push(R.rectangle(px(), py(), px(), py())); break;
      case 'round': recs.push(R.roundRect(px(), py(), px(), py(), r.below(400), r.below(400))); break;
      case 'ellipse': recs.push(R.ellipse(px(), py(), px(), py())); break;
      case 'arc': recs.push(R.arc(r.pick([45, 46, 47, 55]), px(), py(), px(), py(), px(), py(), px(), py())); break;
      case 'poly16': recs.push(R.poly16(r.pick([86, 87, 85, 88, 89]), pts(r.int(2, 9), false))); break;
      case 'poly32': recs.push(R.poly32(r.pick([3, 4, 2, 5, 6]), pts(r.int(2, 9), true))); break;
      case 'polyline': recs.push(R.poly32(4, pts(r.int(2, 6), true))); break;
      case 'bez': recs.push(R.poly32(r.pick([2, 5]), pts(r.pick([4, 7, 10, 5]), true))); break;
      case 'polypoly': { const polys = Array.from({ length: r.int(1, 4) }, () => pts(r.int(3, 6), true)); const t = r.pick([8, 7, 91, 90]); recs.push(R.polyPoly(t, polys, t < 10)); break; }
      case 'path': {
        recs.push(R.beginPath(), R.moveTo(px(), py()), R.lineTo(px(), py()), R.poly32(5, pts(3, true)), R.poly32(6, pts(2, true)));
        if (r.chance(0.5)) recs.push(R.closeFigure());
        if (r.chance(0.3)) recs.push(R.arc(55, px(), py(), px(), py(), px(), py(), px(), py()));
        if (r.chance(0.2)) recs.push(R.rectangle(px(), py(), px(), py()));
        recs.push(R.endPath());
        recs.push(r.pick([R.fillPath(), R.strokeFill(), R.strokePath(), R.clipPath(5), R.abortPath(), R.clipPath(1)]));
        break;
      }
      case 'text': {
        const wide = r.chance(0.75);
        const words = ['Hello', 'Universal Office', 'ABC xyz 123', 'ΑΒΓ αβγ', 'Wörld ñ é', 'abc•def', '€ – “q”', 'ab😀cd', ' ', '', 'A', 'Σ∑∫', '', 'short', 'The quick brown fox'];
        const text = r.pick(words) || 'x';
        const dxs = r.chance(0.5) ? Array.from({ length: text.length }, () => r.int(4, 30)) : null;
        const al = r.pick([0, 0, 1, 2, 6, 8, 24, 24 | 2, 1 | 6, 8 | 6]);
        if (r.chance(0.6)) recs.push(R.textAlign(al));
        if (r.chance(0.4)) recs.push(R.textColor(col()));
        if (r.chance(0.2)) recs.push(R.bkColor(col()), R.bkMode(r.pick([1, 2])));
        recs.push(R.text(wide ? 84 : 83, { x: px(), y: py(), text, opts: r.pick([0, 0, 2, 0x10, 4, 2 | 4]), dx: dxs, withDx: r.chance(0.5), rcl: [px(), py(), px(), py()] }));
        break;
      }
      case 'font': { const h = newObj(); recs.push(R.font(h, { height: r.pick([-12, -16, -24, -40, 20, 30, -8, 0, -100]), esc: r.pick([0, 0, 0, 900, 450, -300, 1800]), weight: r.pick([400, 700, 600, 300]), italic: r.below(2), underline: r.below(2), strike: r.below(2), charset: r.pick([0, 0, 0, 2, 1, 128]), pitch: r.pick([0, 0x12, 0x22, 0x31, 0x20]), face: r.pick(palette) }), R.select(h)); break; }
      case 'clip': recs.push(r.pick([R.isectClip(px(), py(), px(), py()), R.exclClip(px(), py(), px(), py()), R.exclClip(px(), py(), px(), py()), R.isectClip(0, 0, W, H)])); break;
      case 'mode': recs.push(r.pick([R.bkMode(r.int(1, 2)), R.polyFill(r.int(1, 2)), R.rop2(r.int(1, 16)), R.mapMode(r.int(1, 8)), R.textAlign(r.below(30))])); break;
      case 'dc': recs.push(r.chance(0.55) ? R.saveDc() : R.restoreDc(r.pick([-1, -1, -2, 1, 2, 0]))); break;
      case 'rop2': recs.push(R.rop2(r.int(1, 16))); break;
      case 'world': recs.push(r.pick([R.setWorld([r.pick([1, 0.5, 2, 0.8]), r.pick([0, 0, 0.3, -0.2]), r.pick([0, 0, 0.2]), r.pick([1, 0.5, 1.5]), r.below(200), r.below(200)]), R.modWorld([1, 0, 0, 1, r.below(100), r.below(100)], r.int(1, 4)), R.modWorld([r.pick([1, 0.7071]), r.pick([0, 0.7071]), r.pick([0, -0.7071]), r.pick([1, 0.7071]), 0, 0], r.int(2, 3))])); break;
      case 'bit': {
        const bpp = r.pick([1, 4, 8, 16, 24, 32, 24, 32]);
        const w = r.int(1, 6), h = r.int(1, 6);
        const variant = r.pick(['plain', 'plain', 'plain', 'rle', 'fields', 'core', 'fields52']);
        const masks = bpp === 16 ? r.pick([[0xf800, 0x07e0, 0x001f, 0], [0x7c00, 0x03e0, 0x001f, 0]]) : [0xff0000, 0xff00, 0xff, r.pick([0, 0xff000000])];
        let d = dib(r, w, h, bpp, { topDown: r.chance(0.3) });
        if (variant === 'rle') d = dib(r, w, h, 8, { comp: r.pick([1, 2]), topDown: r.chance(0.3) });
        else if (variant === 'core') d = dib(r, w, h, r.pick([1, 4, 8, 24]), { core: true });
        else if ((variant === 'fields' || variant === 'fields52') && (bpp === 16 || bpp === 32)) {
          d = dib(r, w, h, bpp, { comp: 3, masks: variant === 'fields' ? masks.slice(0, 3) : masks });
          if (variant === 'fields52') { const bm = d.bmi.slice(0, 40); bm[0] = r.pick([52, 56]); bm.push(...[0, 0, 0, 0].map(() => 0)); for (let i = 40; i < 52; i++) bm[i] = 0; d = { bmi: [...bm.slice(0, 40), ...new Bytes().u32(masks[0]).u32(masks[1]).u32(masks[2]).u32(masks[3]).a], bits: d.bits }; d.bmi[0] = 56; }
        }
        const t = r.pick(['bitblt', 'stretchblt', 'bitblt', 'dibits', 'sdibits', 'alpha', 'bitblt-nosrc']);
        if (t === 'bitblt' || t === 'stretchblt') recs.push(blitRec(r, t, d, px(), py(), r.int(5, 400), r.int(5, 400)));
        else if (t === 'bitblt-nosrc') recs.push(blitRec(r, 'bitblt', null, px(), py(), r.int(5, 400), r.int(5, 400)));
        else if (t === 'dibits' || t === 'sdibits') {
          recs.push(rec(t === 'dibits' ? 81 : 80, (b) => {
            rect(b, 0, 0, 0, 0); b.i32(px()).i32(py()).i32(r.below(3)).i32(r.below(3)).i32(w).i32(h);
            b.u32(80 + (t === 'dibits' ? 20 : 12)).u32(d.bmi.length).u32(80 + (t === 'dibits' ? 20 : 12) + d.bmi.length).u32(d.bits.length).u32(0);
            if (t === 'dibits') b.u32(r.pick([0xcc0020, 0x8800c6, 0xee0086])).i32(r.int(10, 300)).i32(r.int(10, 300));
            else b.u32(0).u32(h);
            b.bytes(d.bmi).bytes(d.bits);
          }));
        } else {
          recs.push(rec(114, (b) => {
            rect(b, 0, 0, 0, 0); b.i32(px()).i32(py()).i32(r.int(5, 300)).i32(r.int(5, 300)); b.u8(0).u8(0).u8(r.pick([255, 128, 64, 0, 200])).u8(r.pick([0, 1]));
            b.i32(0).i32(0); for (let i = 0; i < 6; i++) b.f32(i === 0 || i === 3 ? 1 : 0); b.u32(0).u32(0);
            b.u32(108).u32(d.bmi.length).u32(108 + d.bmi.length).u32(d.bits.length).i32(w).i32(h); b.bytes(d.bmi).bytes(d.bits);
          }));
        }
        break;
      }
      case 'pixel': recs.push(R.pixel(px(), py(), col())); break;
      case 'grad': {
        const mode = r.pick([0, 1, 2]);
        const verts = Array.from({ length: mode === 2 ? 3 : 2 }, () => ({ x: px(), y: py(), r: r.below(256), g: r.below(256), b: r.below(256) }));
        recs.push(R.gradient(verts, mode === 2 ? [[0, 1, 2]] : [[0, 1]], mode));
        break;
      }
      case 'stock': recs.push(R.select(0x80000000 | r.pick([0, 1, 2, 3, 4, 5, 6, 7, 8, 10, 11, 17, 99]))); break;
      case 'monobrush': { const h = newObj(); const d = dib(r, r.pick([8, 16, 32]), r.pick([8, 8, 16]), r.pick([1, 1, 4, 24])); recs.push(R.monoBrush(r.pick([93, 94]), h, d), R.select(h)); break; }
      case 'rgn': recs.push(R.selClipRgn(r.pick([1, 5, 2, 5]), Array.from({ length: r.int(0, 3) }, () => [px(), py(), px(), py()]))); break;
      default: break;
    }
  }
  void COLORS;
  st.nextH = nextH;
  return recs;
}

/** A GDI-only EMF. */
export function genEmf(seed) {
  const r = new Rand(seed);
  const W = r.pick([21000, 8000, 30000, 12000, 500]);
  const H = r.pick([29700, 6000, 20000, 9000, 700]);
  const dev = r.chance(0.8) ? [1920, 1080] : r.pick([[1024, 768], [4800, 3600], [96, 96], [0, 0]]);
  const mm = r.chance(0.8) ? [508, 286] : r.pick([[270, 203], [254, 254], [0, 0]]);
  const recs = [];
  if (r.chance(0.5)) recs.push(R.mapMode(r.pick([8, 8, 7, 1, 2, 3, 4, 5, 6])));
  if (r.chance(0.6)) recs.push(R.winOrg(r.pick([0, 0, 100, -50]), r.pick([0, 0, 100, -50])), R.winExt(r.pick([W, 1000, 4000, 100]), r.pick([H, 1000, 3000, 100])));
  if (r.chance(0.4)) recs.push(R.vpOrg(0, 0), R.vpExt(r.pick([W, 800, 1000]), r.pick([H, 600, 1000])));
  if (r.chance(0.1)) recs.push(R.scaleVp(1, 2, 3, 4), R.scaleWin(2, 1, 1, 3));
  const useW = r.chance(0.7) ? W : 1000;
  const useH = r.chance(0.7) ? H : 1000;
  recs.push(...drawing(r, useW, useH, r.int(4, 30), {}));
  const frame = r.chance(0.12) ? [0, 0, 0, 0] : [0, 0, W, H];
  return emfFile(recs, { frame, dev, mm, bounds: [0, 0, Math.max(1, Math.round(W / 26)), Math.max(1, Math.round(H / 26))] });
}

// ───────────────────────────── EMF+ ─────────────────────────────

const prec = (type, flags, build) => {
  const d = new Bytes();
  build?.(d);
  while (d.length % 4) d.u8(0);
  return new Bytes().u16(type).u16(flags).u32(12 + d.length).u32(d.length).bytes(d.a);
};

const argb = (r, g, b, a = 255) => ((a << 24) | (r << 16) | (g << 8) | b) >>> 0;

/** An EMF+ object record: type in flags bits 8..14, id in 0..7. */
const obj = (type, id, build) => prec(0x4008, (type << 8) | id, build);

function plusBrush(r, b, ft) {
  const t = ft ?? r.pick([0, 0, 1, 4, 4, 3, 2]);
  b.u32(0xdbc01001).u32(t);
  if (t === 0) b.u32(argb(r.below(256), r.below(256), r.below(256), r.pick([255, 255, 128, 200])));
  else if (t === 1) b.u32(r.int(0, 55)).u32(argb(...Array.from({ length: 3 }, () => r.below(256)))).u32(argb(...Array.from({ length: 3 }, () => r.below(256)), r.pick([255, 0, 128])));
  else if (t === 4) {
    const flags = r.pick([0, 0, 2, 4, 8, 0x80, 0x80 | 8, 2 | 4, 2 | 8 | 0x80]);
    b.u32(flags).u32(r.int(0, 4)).f32(0).f32(0).f32(r.pick([100, 200, 50, 0, 1])).f32(r.pick([100, 50])).u32(argb(r.below(256), 0, 0)).u32(argb(0, 0, r.below(256))).u32(0).u32(0);
    if (flags & 2) [1, 0, 0, 1, r.below(20), r.below(20)].forEach((v) => b.f32(v));
    if (flags & 4) { const n = r.int(2, 4); b.u32(n); for (let i = 0; i < n; i++) b.f32(i / (n - 1)); for (let i = 0; i < n; i++) b.u32(argb(r.below(256), r.below(256), r.below(256))); } else if (flags & 8) { const n = r.int(2, 4); b.u32(n); for (let i = 0; i < n; i++) b.f32(i / (n - 1)); for (let i = 0; i < n; i++) b.f32(r.below(100) / 100); }
  } else if (t === 3) {
    const flags = r.pick([0, 0, 2, 4, 0x40, 0x80, 1]);
    b.u32(flags).u32(0).u32(argb(255, 255, 0)).f32(r.below(100)).f32(r.below(100)).u32(1).u32(argb(0, 0, r.below(256)));
    if (flags & 1) { const path = new Bytes(); plusPath(r, path, 4); b.u32(path.length).bytes(path.a); } else { const n = r.int(3, 6); b.u32(n); for (let i = 0; i < n; i++) b.f32(r.below(200)).f32(r.below(200)); }
    if (flags & 2) [1, 0, 0, 1, 5, 5].forEach((v) => b.f32(v));
    if (flags & 4) { const n = r.int(2, 3); b.u32(n); for (let i = 0; i < n; i++) b.f32(i / (n - 1)); for (let i = 0; i < n; i++) b.u32(argb(r.below(256), r.below(256), r.below(256))); }
    if (flags & 0x40) b.u32(0).f32(0.3).f32(0.4);
  } else { b.u32(0); }
}

function plusPath(r, b, n) {
  const flags = r.pick([0x4000, 0, 0x4000 | 0x1000, 0x800, 0x2000 | 0x4000, 0x4000]);
  b.u32(0xdbc01001).u32(n).u32(flags);
  const pts = Array.from({ length: n }, () => [r.below(300), r.below(300)]);
  if (flags & 0x800) { for (const [x, y] of pts) { for (const v of [x, y]) { if (r.chance(0.4)) { const w = (v * 40 - 4000) & 0x7fff; b.u8(0x80 | ((w >> 8) & 0x7f)); b.u8(w & 0xff); } else b.u8(v & 0x7f); } } }
  else if (flags & 0x4000) for (const [x, y] of pts) b.u16(x).u16(y);
  else for (const [x, y] of pts) b.f32(x).f32(y);
  const types = Array.from({ length: n }, (_, i) => (i === 0 ? 0 : r.pick([1, 1, 3, 3, 3, 1 | 0x80, 3 | 0x80])));
  if (flags & 0x1000) { for (let i = 0; i < n; i += 1) { b.u8(1).u8(types[i]); } } else for (const t of types) b.u8(t);
  while (b.length % 4) b.u8(0);
}

function plusPen(r, b, ff) {
  const flags = ff ?? r.pick([0, 0, 0x02, 0x04 | 0x02, 0x08, 0x10, 0x20, 0x80 | 0x100, 0x100, 0x800, 0x1000, 0x800 | 0x1000, 0x20 | 0x08 | 0x10]);
  b.u32(0xdbc01001).u32(0).u32(flags).u32(r.pick([0, 2, 3, 6])).f32(r.pick([1, 2, 5, 0.5, 8, 0]));
  if (flags & 1) for (let i = 0; i < 6; i++) b.f32(0);
  if (flags & 2) b.i32(r.pick([0xff, 0xff, 1, 2, 0x12, 0x13]));
  if (flags & 4) b.i32(r.pick([0xff, 0xff, 0, 2, 0x11, 0x14]));
  if (flags & 8) b.i32(r.below(3));
  if (flags & 0x10) b.f32(r.pick([10, 3, 0]));
  if (flags & 0x20) b.i32(r.below(6));
  if (flags & 0x80) b.f32(r.below(3));
  if (flags & 0x100) { const n = r.int(1, 4); b.u32(n); for (let i = 0; i < n; i++) b.f32(r.pick([1, 2, 3, 0, 0.5])); }
  for (const bit of [0x800, 0x1000]) {
    if (!(flags & bit)) continue;
    const c = new Bytes();
    c.u32(0xdbc01001).u32(r.pick([1, 0]));
    if (c.a[4] === 1) c.f32(5).f32(8).f32(0).u32(r.below(2)).u32(0).u32(0).u32(0).u32(0).f32(1);
    else { c.u32(3).u32(0).f32(1).u32(0).u32(0).u32(0).u32(0).u32(0).f32(1); const p = new Bytes(); plusPath(r, p, 3); c.u32(p.length).bytes(p.a); }
    b.u32(c.length).bytes(c.a);
  }
  plusBrush(r, b);
}

function plusRegion(r, b) {
  b.u32(0xdbc01001).u32(r.int(0, 2));
  const node = (d) => {
    const t = r.pick(d > 1 ? [0x10000000, 0x10000001, 0x10000002, 0x10000003] : [1, 4, 5, 2, 3, 0x10000000, 0x10000001, 0x10000002, 0x10000003]);
    b.u32(t);
    if (t >= 1 && t <= 5) { node(d + 1); node(d + 1); }
    else if (t === 0x10000000) b.f32(r.below(100)).f32(r.below(100)).f32(r.below(200)).f32(r.below(200));
    else if (t === 0x10000001) { const p = new Bytes(); plusPath(r, p, 4); b.u32(p.length).bytes(p.a); }
  };
  node(0);
}

function plusImage(r, b, nested) {
  b.u32(0xdbc01001);
  const t = r.pick(nested ? [0, 0, 0, 1, 1] : [0, 0, 0, 0, 0, 1, 2, 2, 3]);
  if (t === 0) {
    // an uncompressed EmfPlusBitmap: width@8 height@12 stride@16 format@20 type@24 data@28 (the palette first when indexed)
    const fmt = r.pick([0x26200a, 0x21808, 0x22009, 0x30101 | 0x10000, 0x30402 | 0x10000, 0x30803 | 0x10000, 0x21005, 0x21006, 0x26200a | 0x80000, 0x1c0e | 0x40000, 0x21004, 0x30803 | 0x10000 | 0x40000]);
    const w = r.int(1, 8), h = r.int(1, 8);
    const bpp = (fmt >>> 8) & 0xff;
    const stride = Math.max(1, Math.ceil((w * bpp) / 8)) + r.pick([0, 0, 1, 3]);
    b.u32(1).i32(w).i32(h).i32(stride).u32(fmt).u32(0);
    if (fmt & 0x10000) { b.u32(0).u32(1 << Math.min(bpp, 8)); for (let i = 0; i < 1 << Math.min(bpp, 8); i++) b.u32(argb(r.below(256), r.below(256), r.below(256), r.pick([255, 255, 0, 100]))); }
    for (let i = 0; i < stride * h; i++) b.u8(r.below(256));
  } else if (t === 1) {
    // compressed (PNG): the TS needs the shell to decode it first
    b.u32(1).i32(r.pick([0, 20])).i32(r.pick([0, 20])).i32(0).i32(0).u32(1);
    b.bytes([0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a, ...Array.from({ length: r.int(8, 40) }, () => r.below(256))]);
  } else if (t === 2 || t === 3) {
    const inner = t === 2 ? genEmfBare(r.below(1e6)) : wmfBare(r.below(1e6));
    b.u32(2).u32(t === 2 ? 3 : 2).u32(t === 3 ? inner.length - 24 : inner.length);
    b.bytes(inner);
  }
}

function genEmfBare(seed) {
  const r = new Rand(seed);
  return emfFile(drawing(r, 1000, 800, r.int(2, 8), {}), { frame: [0, 0, 2000, 1600], bounds: [0, 0, 80, 60] });
}

/** An EMF+ file: the EMF header, a dual/only EMF+ stream, with GDI fallback records. */
export function genEmfPlus(seed) {
  const r = new Rand(seed);
  const dual = r.chance(0.55);
  const W = 21000, H = 14000;
  const D = 300;
  const wrap = (records) => {
    const d = new Bytes();
    for (const x of records) d.bytes(x.a);
    return R.comment([0x45, 0x4d, 0x46, 0x2b, ...d.a]);
  };
  const header = prec(0x4001, (dual ? 1 : 0) | 0, (b) => b.u32(0xdbc01001).u32(dual ? 1 : 0).u32(r.pick([96, 300, 600, 0])).u32(96));
  const recs = [header];
  const objs = [];
  const px = () => r.below(D);
  const rects = (c) => (c ? [px(), px(), r.below(200), r.below(200)] : [px(), px(), r.below(200), r.below(200)]);
  const nObj = r.int(2, 6);
  for (let i = 0; i < nObj; i++) {
    const t = r.pick([1, 1, 2, 2, 3, 4, 5, 5]);
    const id = i;
    recs.push(obj(t, id, (b) => (t === 1 ? plusBrush(r, b) : t === 2 ? plusPen(r, b) : t === 3 ? plusPath(r, b, r.int(3, 8)) : t === 4 ? plusRegion(r, b) : plusImage(r, b, false))));
    objs.push([t, id]);
  }
  const pick = (t) => { const c = objs.filter((o) => o[0] === t); return c.length ? r.pick(c)[1] : r.below(4); };
  const nRec = r.int(4, 24);
  for (let i = 0; i < nRec; i++) {
    const c16 = r.chance(0.4);
    const rcB = (b) => { if (c16) b.u16(px()).u16(px()).u16(r.below(200)).u16(r.below(200)); else b.f32(px()).f32(px()).f32(r.below(200)).f32(r.below(200)); };
    const ptsB = (b, n, rel = false) => {
      for (let k = 0; k < n; k++) { if (rel) { b.u8(r.below(0x3f)); b.u8(r.below(0x3f)); } else if (c16) b.u16(px()).u16(px()); else b.f32(px()).f32(px()); }
    };
    const solid = r.chance(0.35);
    const brFl = solid ? 0x8000 : 0;
    const k = r.weighted([
      ['clear', 1], ['fillrects', 3], ['drawrects', 2], ['fillpoly', 3], ['drawlines', 3], ['fillell', 2], ['drawell', 2], ['fillpie', 2], ['drawpie', 1], ['drawarc', 2], ['fillrgn', 2], ['fillpath', 4],
      ['drawpath', 4], ['drawbez', 2], ['drawimg', 3], ['drawimgpts', 2], ['save', 3], ['container', 2], ['xform', 4], ['page', 1], ['clip', 4], ['offset', 1], ['getdc', 2], ['other', 1],
    ]);
    const bcol = argb(r.below(256), r.below(256), r.below(256), r.pick([255, 255, 128]));
    switch (k) {
      case 'clear': recs.push(prec(0x4009, 0, (b) => b.u32(bcol))); break;
      case 'fillrects': recs.push(prec(0x400a, brFl | (c16 ? 0x4000 : 0), (b) => { b.u32(solid ? bcol : pick(1)).u32(r.int(1, 3)); for (let q = 0; q < 3; q++) rcB(b); })); break;
      case 'drawrects': recs.push(prec(0x400b, (c16 ? 0x4000 : 0) | pick(2), (b) => { b.u32(r.int(1, 3)); for (let q = 0; q < 3; q++) rcB(b); })); break;
      case 'fillpoly': { const n = r.int(3, 6); recs.push(prec(0x400c, brFl | (c16 ? 0x4000 : 0) | (r.chance(0.2) ? 0x800 : 0), (b) => { b.u32(solid ? bcol : pick(1)).u32(n); ptsB(b, n, false); })); break; }
      case 'drawlines': { const n = r.int(2, 6); recs.push(prec(0x400d, (c16 ? 0x4000 : 0) | (r.chance(0.3) ? 0x2000 : 0) | pick(2), (b) => { b.u32(n); ptsB(b, n); })); break; }
      case 'fillell': recs.push(prec(0x400e, brFl | (c16 ? 0x4000 : 0), (b) => { b.u32(solid ? bcol : pick(1)); rcB(b); })); break;
      case 'drawell': recs.push(prec(0x400f, (c16 ? 0x4000 : 0) | pick(2), (b) => { rcB(b); })); break;
      case 'fillpie': recs.push(prec(0x4010, brFl | (c16 ? 0x4000 : 0), (b) => { b.u32(solid ? bcol : pick(1)).f32(r.below(360)).f32(r.pick([90, -90, 360, 270, 45])); rcB(b); })); break;
      case 'drawpie': case 'drawarc': recs.push(prec(k === 'drawpie' ? 0x4011 : 0x4012, (c16 ? 0x4000 : 0) | pick(2), (b) => { b.f32(r.below(360)).f32(r.pick([90, -90, 360, 270, 45])); rcB(b); })); break;
      case 'fillrgn': recs.push(prec(0x4013, brFl | pick(4), (b) => b.u32(solid ? bcol : pick(1)))); break;
      case 'fillpath': recs.push(prec(0x4014, brFl | pick(3), (b) => b.u32(solid ? bcol : pick(1)))); break;
      case 'drawpath': recs.push(prec(0x4015, pick(3), (b) => b.u32(pick(2)))); break;
      case 'drawbez': { const n = r.pick([4, 7]); recs.push(prec(0x4019, (c16 ? 0x4000 : 0) | pick(2), (b) => { b.u32(n); ptsB(b, n); })); break; }
      case 'drawimg': recs.push(prec(0x401a, (c16 ? 0x4000 : 0) | pick(5), (b) => { b.u32(0).u32(2).f32(r.pick([0, 1])).f32(r.pick([0, 1])).f32(r.pick([0, 8, 4])).f32(r.pick([0, 8, 4])); rcB(b); })); break;
      case 'drawimgpts': recs.push(prec(0x401b, (c16 ? 0x4000 : 0) | pick(5), (b) => { b.u32(0).u32(2).f32(0).f32(0).f32(r.pick([0, 8])).f32(r.pick([0, 8])).u32(0).u32(3); ptsB(b, 3); })); break;
      case 'save': recs.push(r.chance(0.6) ? prec(0x4025, 0, (b) => b.u32(r.int(1, 3))) : prec(0x4026, 0, (b) => b.u32(r.int(1, 3)))); break;
      case 'container': recs.push(r.pick([prec(0x4027, r.pick([2, 3, 4]), (b) => { b.f32(0).f32(0).f32(r.pick([100, 50])).f32(r.pick([100, 50])).f32(0).f32(0).f32(r.pick([100, 0])).f32(r.pick([100, 20])).u32(r.int(4, 6)); }), prec(0x4028, 0, (b) => b.u32(r.int(4, 6))), prec(0x4029, 0, (b) => b.u32(r.int(4, 6)))])); break;
      case 'xform': recs.push(r.pick([prec(0x402a, 0, (b) => { [r.pick([1, 0.5, 2]), r.pick([0, 0.2]), r.pick([0, -0.2]), r.pick([1, 0.5]), r.below(50), r.below(50)].forEach((v) => b.f32(v)); }), prec(0x402b, 0), prec(0x402c, r.pick([0, 0x2000]), (b) => { [1.1, 0, 0, 0.9, 4, 6].forEach((v) => b.f32(v)); }), prec(0x402d, r.pick([0, 0x2000]), (b) => b.f32(r.below(50)).f32(r.below(50))), prec(0x402e, r.pick([0, 0x2000]), (b) => b.f32(r.pick([0.5, 2, 1.5])).f32(r.pick([0.5, 2]))), prec(0x402f, r.pick([0, 0x2000]), (b) => b.f32(r.pick([30, 90, 45, -20]))) ])); break;
      case 'page': recs.push(prec(0x4030, r.pick([2, 3, 4, 5, 6]), (b) => b.f32(r.pick([1, 0.5, 2, 0])))); break;
      case 'clip': recs.push(r.pick([prec(0x4031, 0), prec(0x4032, r.below(6) << 8, (b) => { b.f32(px()).f32(px()).f32(r.below(300)).f32(r.below(300)); }), prec(0x4033, (r.below(6) << 8) | pick(3)), prec(0x4034, (r.below(6) << 8) | pick(4))])); break;
      case 'offset': recs.push(prec(0x4035, 0, (b) => b.f32(r.below(20)).f32(r.below(20)))); break;
      case 'getdc': recs.push(prec(0x4004, 0)); recs.push(null); break; // a GDI record follows (see below)
      default: recs.push(prec(r.pick([0x401e, 0x401f, 0x4020, 0x4021, 0x4022, 0x4023, 0x4024, 0x403a, 0x4039]), 0, (b) => b.u32(0).u32(0))); break;
    }
  }
  recs.push(prec(0x4002, 0));
  // split into EMF comments of a few records each; GDI fallbacks (a random drawing) between them, GetDC makes them count
  const emfRecs = [];
  let batch = [];
  const flush = () => { if (batch.length) { emfRecs.push(wrap(batch)); batch = []; } };
  for (const x of recs) {
    if (x === null) { flush(); emfRecs.push(...drawing(r, W, H, r.int(1, 3), {})); continue; }
    batch.push(x);
    if (batch.length >= r.int(2, 6)) flush();
    if (dual && r.chance(0.1)) { flush(); emfRecs.push(R.rectangle(r.below(W), r.below(H), r.below(W), r.below(H))); }
  }
  flush();
  return emfFile(emfRecs, { frame: [0, 0, W, H], bounds: [0, 0, 794, 529] });
}

/**
 * An EMF+ file built around the features the random one reaches rarely: every brush kind used by every fill record, pens with
 * custom and anchor caps, raw bitmaps and nested metafile images, objects split in continued records.
 */
export function genEmfPlusFocused(seed) {
  const r = new Rand(seed);
  const dual = r.chance(0.3);
  const recs = [prec(0x4001, dual ? 1 : 0, (b) => b.u32(0xdbc01001).u32(dual ? 1 : 0).u32(96).u32(96))];
  const c16 = r.chance(0.5);
  const pt = (b) => { if (c16) b.u16(r.below(300)).u16(r.below(300)); else b.f32(r.below(300)).f32(r.below(300)); };
  const fl16 = c16 ? 0x4000 : 0;
  const push = (rcd) => recs.push(rcd);
  // objects: ids 0..: brushes (solid, hatch, linear wraps, path gradients), pens, paths, images
  let id = 0;
  const brushIds = [], penIds = [], pathIds = [], imgIds = [];
  const mkObj = (type, build) => {
    const my = id++;
    const d = new Bytes();
    build(d);
    while (d.length % 4) d.u8(0);
    // a few objects arrive split in two records (the continued form: flag 0x8000, the total size first)
    if (r.chance(0.25) && d.length > 8) {
      const cut = r.int(4, d.length - 4) & ~3;
      push(new Bytes().u16(0x4008).u16(0x8000 | (type << 8) | my).u32(12 + 4 + cut).u32(4 + cut).u32(d.length).bytes(d.a.slice(0, cut)));
      push(new Bytes().u16(0x4008).u16(0x8000 | (type << 8) | my).u32(12 + 4 + d.length - cut).u32(4 + d.length - cut).u32(d.length).bytes(d.a.slice(cut)));
    } else push(obj(type, my, (b) => b.bytes(d.a)));
    return my;
  };
  for (const t of [0, 1, 4, 4, 3, 3, 2]) brushIds.push(mkObj(1, (b) => plusBrush(r, b, t)));
  for (let i = 0; i < 4; i++) penIds.push(mkObj(2, (b) => plusPen(r, b, r.pick([0x800 | 0x2 | 0x4, 0x1000 | 0x2 | 0x4, 0x800 | 0x1000 | 0x2 | 0x4, 0x800, 0x1000, 0x800 | 0x1000, 0x2 | 0x4, 0x20 | 0x2, 0x100 | 0x80, 0x8 | 0x10 | 0x2]))));
  for (let i = 0; i < 3; i++) pathIds.push(mkObj(3, (b) => plusPath(r, b, r.int(3, 8))));
  for (let i = 0; i < 3; i++) imgIds.push(mkObj(5, (b) => plusImage(r, b, false)));
  const rcB = (b) => { if (c16) b.u16(r.below(200)).u16(r.below(200)).u16(r.int(10, 150)).u16(r.int(10, 150)); else b.f32(r.below(200)).f32(r.below(200)).f32(r.int(10, 150)).f32(r.int(10, 150)); };
  for (const bid of brushIds) {
    push(prec(0x400a, fl16 | 0, (b) => { b.u32(bid).u32(1); rcB(b); }));
    push(prec(0x400e, fl16, (b) => { b.u32(bid); rcB(b); }));
    push(prec(0x4014, pathIds[r.below(pathIds.length)], (b) => b.u32(bid)));
    push(prec(0x400c, fl16, (b) => { b.u32(bid).u32(4); for (let k = 0; k < 4; k++) pt(b); }));
  }
  for (const pid of penIds) {
    push(prec(0x400d, fl16 | pid, (b) => { b.u32(3); for (let k = 0; k < 3; k++) pt(b); }));
    push(prec(0x4015, pathIds[r.below(pathIds.length)], (b) => b.u32(pid)));
    push(prec(0x4012, fl16 | pid, (b) => { b.f32(r.below(360)).f32(r.pick([90, 200, -120])); rcB(b); }));
    push(prec(0x4019, fl16 | pid, (b) => { b.u32(4); for (let k = 0; k < 4; k++) pt(b); }));
  }
  for (const iid of imgIds) {
    push(prec(0x401a, fl16 | iid, (b) => { b.u32(0).u32(2).f32(0).f32(0).f32(r.pick([0, 8, 4])).f32(r.pick([0, 8, 4])); rcB(b); }));
    push(prec(0x401b, fl16 | iid, (b) => { b.u32(0).u32(2).f32(0).f32(0).f32(r.pick([0, 8])).f32(r.pick([0, 8])).u32(0).u32(3); for (let k = 0; k < 3; k++) pt(b); }));
  }
  push(prec(0x4002, 0));
  const wrap = (records) => { const d = new Bytes(); for (const x of records) d.bytes(x.a); return R.comment([0x45, 0x4d, 0x46, 0x2b, ...d.a]); };
  const out = [];
  for (let i = 0; i < recs.length; i += r.int(2, 8)) out.push(wrap(recs.slice(i, i + 8)));
  void out;
  // one comment per run of records (a split object's two halves may fall in different comments: the TS keeps the pending part)
  const comments = [];
  let i = 0;
  while (i < recs.length) { const n = r.int(2, 9); comments.push(wrap(recs.slice(i, i + n))); i += n; }
  return emfFile(comments, { frame: [0, 0, 21000, 14000], bounds: [0, 0, 794, 529] });
}
