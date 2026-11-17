// B5b synthetic WMF (generator group `b5b`): placeable and plain [MS-WMF] files with the records `playWmf` reads.
import { Rand } from './rand.mjs';
import { Bytes, dib } from './meta-emf.mjs';

const wrec = (fn, build) => {
  const b = new Bytes();
  build?.(b);
  if (b.length % 2) b.u8(0);
  return new Bytes().u32(3 + b.length / 2).u16(fn).bytes(b.a);
};

/** Standard header + records + EOF; `placeable` adds the 22-byte Aldus header with the given box and inch. */
export function wmfFile(records, { placeable = true, box = [0, 0, 1000, 800], inch = 96 } = {}) {
  const body = new Bytes();
  for (const x of records) body.bytes(x.a);
  body.bytes(wrec(0).a);
  const size = 18 + body.length;
  const out = new Bytes();
  if (placeable) {
    out.u32(0x9ac6cdd7).u16(0);
    for (const v of box) out.u16(v);
    out.u16(inch).u32(0).u16(0);
  }
  out.u16(1).u16(9).u16(0x300).u32(size / 2).u16(8).u32(14).u16(0);
  out.bytes(body.a);
  return out.done();
}

/** The WMF an EMF+ metafile image keeps: the 22-byte header, two bytes of padding, then the standard header (`size` counts the WMF). */
export function wmfBare(seed) {
  const r = new Rand(seed);
  const recs = [W.polygon(Array.from({ length: 4 }, () => [r.below(100), r.below(100)])), W.rectangle(5, 5, 60, 60)];
  const whole = wmfFile(recs, { placeable: true, box: [0, 0, 100, 100], inch: 96 });
  const out = new Bytes().bytes([...whole.slice(0, 22)]).u16(0).bytes([...whole.slice(22)]);
  return out.done();
}

export const W = {
  mapMode: (m) => wrec(0x0103, (b) => b.u16(m)),
  winOrg: (x, y) => wrec(0x020b, (b) => b.u16(y).u16(x)),
  winExt: (x, y) => wrec(0x020c, (b) => b.u16(y).u16(x)),
  vpOrg: (x, y) => wrec(0x020d, (b) => b.u16(y).u16(x)),
  vpExt: (x, y) => wrec(0x020e, (b) => b.u16(y).u16(x)),
  offsetWin: (x, y) => wrec(0x020f, (b) => b.u16(y).u16(x)),
  scaleWin: (a, b2, c, d) => wrec(0x0410, (b) => b.u16(a).u16(b2).u16(c).u16(d)),
  scaleVp: (a, b2, c, d) => wrec(0x0412, (b) => b.u16(a).u16(b2).u16(c).u16(d)),
  bkMode: (m) => wrec(0x0102, (b) => b.u16(m)),
  polyFill: (m) => wrec(0x0106, (b) => b.u16(m)),
  rop2: (m) => wrec(0x0104, (b) => b.u16(m)),
  textAlign: (m) => wrec(0x012e, (b) => b.u16(m)),
  textColor: (c) => wrec(0x0209, (b) => b.u32(c)),
  bkColor: (c) => wrec(0x0201, (b) => b.u32(c)),
  saveDc: () => wrec(0x001e),
  restoreDc: (n) => wrec(0x0127, (b) => b.u16(n)),
  pen: (style, w, c) => wrec(0x02fa, (b) => b.u16(style).u16(w).u16(0).u32(c)),
  brush: (style, c, hatch = 0) => wrec(0x02fc, (b) => b.u16(style).u32(c).u16(hatch)),
  font: ({ height = -16, esc = 0, weight = 400, italic = 0, underline = 0, strike = 0, charset = 0, pitch = 0, face = 'Arial' } = {}) => wrec(0x02fb, (b) => {
    b.u16(height).u16(0).u16(esc).u16(0).u16(weight).u8(italic).u8(underline).u8(strike).u8(charset).u8(0).u8(0).u8(0).u8(pitch);
    for (const ch of face) b.u8(ch.charCodeAt(0));
    b.u8(0);
  }),
  patBrush: (d, style = 0) => wrec(0x0142, (b) => { b.u16(style).u16(0); b.bytes(d.bmi).bytes(d.bits); }),
  other: (fn) => wrec(fn),
  select: (i) => wrec(0x012d, (b) => b.u16(i)),
  del: (i) => wrec(0x01f0, (b) => b.u16(i)),
  moveTo: (x, y) => wrec(0x0214, (b) => b.u16(y).u16(x)),
  lineTo: (x, y) => wrec(0x0213, (b) => b.u16(y).u16(x)),
  polyline: (pts) => wrec(0x0325, (b) => { b.u16(pts.length); for (const [x, y] of pts) b.u16(x).u16(y); }),
  polygon: (pts) => wrec(0x0324, (b) => { b.u16(pts.length); for (const [x, y] of pts) b.u16(x).u16(y); }),
  polyPolygon: (polys) => wrec(0x0538, (b) => { b.u16(polys.length); for (const p of polys) b.u16(p.length); for (const p of polys) for (const [x, y] of p) b.u16(x).u16(y); }),
  rectangle: (l, t, r, bt) => wrec(0x041b, (b) => b.u16(bt).u16(r).u16(t).u16(l)),
  roundRect: (l, t, r, bt, w, h) => wrec(0x061c, (b) => b.u16(h).u16(w).u16(bt).u16(r).u16(t).u16(l)),
  ellipse: (l, t, r, bt) => wrec(0x0418, (b) => b.u16(bt).u16(r).u16(t).u16(l)),
  arc: (fn, l, t, r, bt, xs, ys, xe, ye) => wrec(fn, (b) => b.u16(ye).u16(xe).u16(ys).u16(xs).u16(bt).u16(r).u16(t).u16(l)),
  isectClip: (l, t, r, bt) => wrec(0x0416, (b) => b.u16(bt).u16(r).u16(t).u16(l)),
  exclClip: (l, t, r, bt) => wrec(0x0415, (b) => b.u16(bt).u16(r).u16(t).u16(l)),
  pixel: (x, y, c) => wrec(0x041f, (b) => b.u32(c).u16(y).u16(x)),
  clipRgn: () => wrec(0x012c, (b) => b.u16(0)),
  textOut: (x, y, text) => wrec(0x0521, (b) => { b.u16(text.length); for (const ch of text) b.u8(ch.charCodeAt(0) & 0xff); if (text.length % 2) b.u8(0); b.u16(y).u16(x); }),
  extText: (x, y, text, opts = 0, rect = [0, 0, 0, 0], dx = null) => wrec(0x0a32, (b) => {
    b.u16(y).u16(x).u16(text.length).u16(opts);
    if (opts & 6) for (const v of rect) b.u16(v);
    for (const ch of text) b.u8(ch.charCodeAt(0) & 0xff);
    if (text.length % 2) b.u8(0);
    if (dx) for (let i = 0; i < text.length; i++) b.u16(dx[i] ?? 8);
  }),
  dibStretchBlt: (rop, d, [xs, ys, sw, sh], [xd, yd, dw, dh]) => wrec(0x0b41, (b) => {
    b.u32(rop);
    if (d) { b.u16(sh).u16(sw).u16(ys).u16(xs).u16(dh).u16(dw).u16(yd).u16(xd).bytes(d.bmi).bytes(d.bits); }
    else { b.u16(sh).u16(sw).u16(ys).u16(xs).u16(0).u16(dh).u16(dw).u16(yd).u16(xd); }
  }),
  dibBitBlt: (rop, d, [xs, ys], [xd, yd, w, h]) => wrec(0x0940, (b) => {
    b.u32(rop);
    if (d) { b.u16(ys).u16(xs).u16(h).u16(w).u16(yd).u16(xd).bytes(d.bmi).bytes(d.bits); }
    else { b.u16(ys).u16(xs).u16(0).u16(h).u16(w).u16(yd).u16(xd); }
  }),
  patBlt: (rop, x, y, w, h) => wrec(0x061d, (b) => b.u32(rop).u16(h).u16(w).u16(y).u16(x)),
  stretchDib: (rop, d, [xs, ys, sw, sh], [xd, yd, dw, dh]) => wrec(0x0f43, (b) => { b.u32(rop).u16(0).u16(sh).u16(sw).u16(ys).u16(xs).u16(dh).u16(dw).u16(yd).u16(xd).bytes(d.bmi).bytes(d.bits); }),
  setDibToDev: (d, [xd, yd, w, h]) => wrec(0x0d33, (b) => { b.u16(0).u16(h).u16(0).u16(0).u16(0).u16(h).u16(w).u16(yd).u16(xd).bytes(d.bmi).bytes(d.bits); }),
};

export function genWmf(seed) {
  const r = new Rand(seed);
  const N = r.pick([1000, 400, 2000, 100]);
  const box = r.chance(0.12) ? [0, 0, 0, 0] : [0, 0, N, Math.round(N * r.pick([0.8, 1, 0.5]))];
  const placeable = r.chance(0.85);
  const recs = [];
  if (r.chance(0.5)) recs.push(W.mapMode(r.pick([8, 8, 7, 1])), W.winOrg(r.pick([0, 0, 50]), r.pick([0, 0, 50])), W.winExt(r.pick([N, N, 500]), r.pick([N, 400, 500])));
  if (r.chance(0.15)) recs.push(W.scaleVp(1, 2, 1, 2), W.scaleWin(2, 1, 1, 1), W.vpOrg(0, 0), W.vpExt(N, N), W.offsetWin(10, 5));
  const px = () => r.below(N);
  const col = () => (r.below(256) | (r.below(256) << 8) | (r.below(256) << 16)) >>> 0;
  const pal = ['Arial', 'Times New Roman', 'Courier New', 'Symbol', 'Wingdings', 'Tahoma', 'Helvetica', 'MS Sans Serif', 'Webdings'];
  const objs = [];
  let nObj = 0;
  const add = (x) => { recs.push(x); nObj++; return nObj - 1; };
  const n = r.int(4, 28);
  for (let i = 0; i < n; i++) {
    const k = r.weighted([['pen', 4], ['brush', 4], ['line', 4], ['rect', 6], ['round', 2], ['ell', 3], ['arc', 3], ['poly', 5], ['text', 8], ['font', 4], ['clip', 3], ['mode', 4], ['dc', 2], ['blit', 4], ['pixel', 1], ['del', 2], ['patbrush', 2], ['misc', 1]]);
    switch (k) {
      case 'pen': objs.push(add(W.pen(r.below(7), r.below(30), col()))); recs.push(W.select(r.pick(objs))); break;
      case 'brush': objs.push(add(W.brush(r.pick([0, 0, 1, 2, 3]), col(), r.below(7)))); recs.push(W.select(r.pick(objs))); break;
      case 'line': recs.push(W.moveTo(px(), px()), W.lineTo(px(), px())); break;
      case 'rect': recs.push(W.rectangle(px(), px(), px(), px())); break;
      case 'round': recs.push(W.roundRect(px(), px(), px(), px(), r.below(200), r.below(200))); break;
      case 'ell': recs.push(W.ellipse(px(), px(), px(), px())); break;
      case 'arc': recs.push(W.arc(r.pick([0x0817, 0x081a, 0x0830]), px(), px(), px(), px(), px(), px(), px(), px())); break;
      case 'poly': { const pts = Array.from({ length: r.int(2, 7) }, () => [px(), px()]); recs.push(r.pick([W.polyline(pts), W.polygon(pts), W.polyPolygon([pts, pts.slice(0, 3)])])); break; }
      case 'text': {
        const text = r.pick(['Hello', 'ABC xyz', 'Wörld', '€ “q”', 'Σ', 'short', 'x', 'The quick brown fox']);
        if (r.chance(0.4)) recs.push(W.textAlign(r.pick([0, 1, 2, 6, 8, 24, 1 | 6])));
        if (r.chance(0.3)) recs.push(W.textColor(col()), W.bkColor(col()), W.bkMode(r.int(1, 2)));
        recs.push(r.chance(0.4) ? W.textOut(px(), px(), text) : W.extText(px(), px(), text, r.pick([0, 2 | 4, 4, 0, 2]), [px(), px(), px(), px()], r.chance(0.6) ? Array.from({ length: text.length }, () => r.int(3, 20)) : null));
        break;
      }
      case 'font': objs.push(add(W.font({ height: r.pick([-12, -16, -30, 20, 0, -100]), esc: r.pick([0, 0, 900, 450]), weight: r.pick([400, 700]), italic: r.below(2), underline: r.below(2), strike: r.below(2), charset: r.pick([0, 0, 2, 1]), pitch: r.pick([0, 0x12, 0x22, 0x31]), face: r.pick(pal) }))); recs.push(W.select(r.pick(objs))); break;
      case 'clip': recs.push(r.pick([W.isectClip(px(), px(), px(), px()), W.exclClip(px(), px(), px(), px()), W.exclClip(px(), px(), px(), px()), W.clipRgn()])); break;
      case 'mode': recs.push(r.pick([W.bkMode(r.int(1, 2)), W.polyFill(r.int(1, 2)), W.rop2(r.int(1, 16)), W.mapMode(r.int(1, 8))])); break;
      case 'dc': recs.push(r.chance(0.55) ? W.saveDc() : W.restoreDc(r.pick([0xffff, 0xfffe, 1, 0]))); break;
      case 'blit': {
        const d = dib(r, r.int(1, 6), r.int(1, 6), r.pick([1, 4, 8, 24, 32, 16]), {});
        const rop = r.pick([0xcc0020, 0x8800c6, 0xee0086, 0x660046, 0x330008, 0xaa0029, 0xf00021, 0x5a0049, 0x550009, 0x000042]);
        recs.push(r.pick([
          W.dibStretchBlt(rop, d, [0, 0, 4, 4], [px(), px(), r.int(5, 200), r.int(5, 200)]), W.dibStretchBlt(rop, null, [0, 0, 4, 4], [px(), px(), r.int(5, 200), r.int(5, 200)]),
          W.dibBitBlt(rop, d, [0, 0], [px(), px(), r.int(5, 200), r.int(5, 200)]), W.dibBitBlt(rop, null, [0, 0], [px(), px(), r.int(5, 200), r.int(5, 200)]),
          W.patBlt(rop, px(), px(), r.int(5, 200), r.int(5, 200)), W.stretchDib(rop, d, [0, 0, 3, 3], [px(), px(), 80, 80]), W.setDibToDev(d, [px(), px(), 60, 60]),
        ]));
        break;
      }
      case 'pixel': recs.push(W.pixel(px(), px(), col())); break;
      case 'del': recs.push(W.del(r.below(Math.max(1, nObj)))); break;
      case 'patbrush': objs.push(add(W.patBrush(dib(r, r.pick([8, 8, 16]), r.pick([8, 8, 16]), r.pick([1, 1, 4, 24])), r.pick([0, 1])))); recs.push(W.select(r.pick(objs))); break;
      default: recs.push(W.other(r.pick([0x00f7, 0x01f9, 0x06ff, 0x0aaa]))); break;
    }
  }
  return wmfFile(recs, { placeable, box, inch: r.pick([96, 1440, 0, 1000]) });
}
