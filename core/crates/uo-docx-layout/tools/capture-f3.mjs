// Differential metafile capture of frontier F3 (record "Wave 1 — F3"): seeded random EMF (GDI and
// EMF+ only), and WMF files, played by the frozen TS `renderEmf`/`renderWmf` with a fresh
// `FaceResolver` over the repository's open fonts (node-fonts with no system fonts, as the CI mode
// of capture-layout.mjs), written in the `f3.jsonl` dump format of tools/lib/facets-f3.mjs, so
// tests/f3_facets.rs replays every case (`v` is the per-call value, or `error` when the TS threw).
// The corpus has 26 EMF and 2 WMF calls and no EMF+; this covers every record type the players
// read, the transforms, the bitmap paths, text alignment and the malformed cases (reads past the
// end, short records, huge counts).
//
//   node tools/capture-f3.mjs --ts-root <built checkout of main> [--seed 1] [--emf 320] [--wmf 160]
//        [--out tests/captures/f3/metafile-gen.f3.jsonl]
const { writeFileSync, mkdirSync } = await import('node:fs');
const { join, dirname } = await import('node:path');
const { fileURLToPath } = await import('node:url');
const { importFrom, tsRoot } = await import('../../uo-testkit/js/testkit.mjs');
const { f3Value, faceLine } = await import('./lib/facets-f3.mjs');

const here = dirname(fileURLToPath(import.meta.url));
const argv = process.argv.slice(2);
const opt = (name, d) => { const i = argv.indexOf(name); return i >= 0 ? argv[i + 1] : d; };
const root = tsRoot(argv);
process.env.WINDIR = join(here, '__no_system_fonts__');
process.env.LOCALAPPDATA = join(here, '__no_system_fonts__');
// the fonts tracked by THIS repository (a checkout of main can hold ignored local fonts, e.g. Calibri)
const repo = join(here, '..', '..', '..', '..');
process.env.EXTRA_FONT_DIRS = [join(repo, 'shared/fonts/files'), join(repo, 'site/assets/fonts')].join(';');

const { renderEmf } = await importFrom(root, 'write/packages/docx-layout/dist/emf.js');
const { renderWmf } = await importFrom(root, 'write/packages/docx-layout/dist/wmf.js');
const { FaceResolver } = await importFrom(root, 'write/packages/docx-layout/dist/fonts.js');
const { createNodeFontProvider } = await importFrom(root, 'write/tools/corpus-lab/node-fonts.mjs');
const provider = await createNodeFontProvider();

/** mulberry32 */
function rng(seed) {
  let a = seed >>> 0;
  const next = () => { a = (a + 0x6d2b79f5) >>> 0; let t = a; t = Math.imul(t ^ (t >>> 15), t | 1); t ^= t + Math.imul(t ^ (t >>> 7), t | 61); return ((t ^ (t >>> 14)) >>> 0) / 4294967296; };
  const int = (lo, hi) => lo + Math.floor(next() * (hi - lo + 1));
  return { next, int, pick: (a) => a[int(0, a.length - 1)], chance: (p) => next() < p };
}

class W {
  constructor() { this.b = []; }
  u8(v) { this.b.push(v & 0xff); return this; }
  u16(v) { this.u8(v); this.u8(v >>> 8); return this; }
  u32(v) { this.u16(v & 0xffff); this.u16((v >>> 16) & 0xffff); return this; }
  i32(v) { return this.u32(v | 0); }
  f32(v) { const d = new DataView(new ArrayBuffer(4)); d.setFloat32(0, v, true); for (let k = 0; k < 4; k++) this.u8(d.getUint8(k)); return this; }
  bytes(a) { for (const x of a) this.u8(x); return this; }
  pad4() { while (this.b.length % 4) this.u8(0); return this; }
  get length() { return this.b.length; }
}

const FACES = ['Arial', 'Times New Roman', 'Courier New', '', 'Liberation Sans', 'Symbol', 'NoSuchFont', 'Calibri'];

/** A small DIB (BITMAPINFOHEADER + bits). */
function dib(r, bpp) {
  const w = r.int(1, 4), h = r.int(1, 3);
  const row = Math.ceil((w * bpp) / 32) * 4;
  const hdr = new W().u32(40).i32(w).i32(r.chance(0.3) ? -h : h).u16(1).u16(bpp).u32(0).u32(row * h).i32(2835).i32(2835).u32(0).u32(0);
  const bits = [];
  for (let k = 0; k < row * h; k++) bits.push(r.int(0, 255));
  return { bmi: hdr.b, bits };
}

/** One EMF record (type, body). */
function emfRecord(r, state) {
  const pt = () => r.int(-200, 1200);
  const body = new W();
  const t = r.pick([17, 9, 10, 11, 12, 35, 36, 33, 34, 18, 19, 22, 24, 25, 38, 95, 39, 82, 37, 37, 40, 27, 54, 54, 43, 44, 42, 86, 87, 3, 4, 89, 6, 85, 2, 88, 5, 91, 8, 90, 7,
    59, 60, 61, 62, 63, 64, 67, 68, 84, 84, 84, 76, 77, 114, 81, 70, 999]);
  const handle = () => r.int(1, 4);
  switch (t) {
    case 17: body.u32(r.int(1, 9)); break;
    case 9: case 11: body.i32(r.chance(0.1) ? 0 : r.int(-2000, 2000)).i32(r.chance(0.1) ? 0 : r.int(-2000, 2000)); break;
    case 10: case 12: body.i32(r.int(-500, 500)).i32(r.int(-500, 500)); break;
    case 35: case 36: {
      const a = r.next() * 2 * Math.PI;
      body.f32(Math.cos(a) * (0.5 + r.next())).f32(Math.sin(a)).f32(-Math.sin(a)).f32(Math.cos(a)).f32(r.int(-100, 100)).f32(r.int(-100, 100));
      if (t === 36) body.u32(r.int(1, 4));
      break;
    }
    case 33: break;
    case 34: body.i32(r.chance(0.2) ? r.int(-5, 3) : -1); break;
    case 18: case 19: body.u32(r.int(1, 2)); break;
    case 22: body.u32(r.int(0, 31)); break;
    case 24: case 25: body.u32(r.int(0, 0xffffff)); break;
    case 38: body.u32(handle()).u32(r.int(0, 6)).i32(r.int(0, 30)).i32(0).u32(r.int(0, 0xffffff)); break;
    case 95: body.u32(handle()).u32(0).u32(0).u32(0).u32(0).u32(r.int(0, 6)).u32(r.int(0, 30)).u32(0).u32(r.int(0, 0xffffff)).u32(0).u32(0); break;
    case 39: body.u32(handle()).u32(r.int(0, 2)).u32(r.int(0, 0xffffff)).u32(0); break;
    case 82: {
      body.u32(handle()).i32(r.chance(0.5) ? -r.int(6, 60) : r.int(0, 60)).i32(0).i32(r.chance(0.2) ? 900 : 0).i32(0).i32(r.pick([400, 700, 600, 300]));
      body.u8(r.chance(0.3) ? 1 : 0).u8(r.chance(0.1) ? 1 : 0).u8(0).u8(0).u32(0).u32(0);
      const face = r.pick(FACES);
      for (let k = 0; k < 32; k++) body.u16(k < face.length ? face.charCodeAt(k) : 0);
      break;
    }
    case 37: body.u32(r.chance(0.4) ? (0x80000000 | r.pick([0, 1, 4, 5, 6, 7, 8, 13, 17, 9])) >>> 0 : handle()); break;
    case 40: body.u32(handle()); break;
    case 27: case 54: body.i32(pt()).i32(pt()); break;
    case 43: case 44: case 42: body.i32(pt()).i32(pt()).i32(pt()).i32(pt()); if (t === 44) body.i32(10).i32(10); break;
    case 86: case 87: case 3: case 4: case 89: case 6: case 85: case 2: case 88: case 5: {
      const short = [86, 87, 89, 85, 88].includes(t);
      const n = r.chance(0.03) ? 100000 : r.int(0, 9);
      body.i32(0).i32(0).i32(0).i32(0).u32(n);
      for (let k = 0; k < Math.min(n, 9); k++) short ? body.u16(pt() & 0xffff).u16(pt() & 0xffff) : body.i32(pt()).i32(pt());
      break;
    }
    case 91: case 8: case 90: case 7: {
      const short = t === 91 || t === 90;
      const polys = r.int(0, 3);
      const counts = Array.from({ length: polys }, () => r.int(0, 5));
      body.i32(0).i32(0).i32(0).i32(0).u32(polys).u32(counts.reduce((a, b) => a + b, 0));
      for (const c of counts) body.u32(c);
      for (const c of counts) for (let k = 0; k < c; k++) short ? body.u16(pt() & 0xffff).u16(pt() & 0xffff) : body.i32(pt()).i32(pt());
      break;
    }
    case 59: case 60: case 61: case 62: case 63: case 64: case 67: case 68: if (t === 62 || t === 63 || t === 64) body.i32(0).i32(0).i32(0).i32(0); if (t === 67) body.u32(5); break;
    case 84: {
      const text = r.pick(['Hello', 'EMF text', '  ', 'Ü€ ok', '', 'A', 'Πλ']);
      const n = text.length;
      const withDx = r.chance(0.6), byGid = r.chance(0.15);
      // EMR_EXTTEXTOUTW: bounds(16) graphicsMode(4) exScale(4) eyScale(4) + EMRTEXT: ref(8) nChars offString options rect(16) offDx
      const offStr = 8 + 16 + 12 + 40 + 0;
      const offDx = offStr + Math.ceil((n * 2) / 4) * 4;
      body.i32(0).i32(0).i32(0).i32(0).u32(1).f32(1).f32(1);
      body.i32(pt()).i32(pt()).u32(r.chance(0.02) ? 50000 : n).u32(offStr).u32((byGid ? 0x10 : 0) | (r.chance(0.3) ? 4 : 0)).i32(0).i32(0).i32(0).i32(0).u32(withDx ? offDx : 0);
      for (let k = 0; k < n; k++) body.u16(byGid ? r.int(1, 60) : text.charCodeAt(k));
      body.pad4();
      if (withDx) for (let k = 0; k < n; k++) body.i32(r.int(0, 40));
      break;
    }
    case 76: case 77: case 81: case 114: {
      const withBits = t === 81 || t === 114 || r.chance(0.6);
      const bpp = r.pick([24, 32]);
      const d = withBits ? dib(r, bpp) : null;
      const rop = r.pick([0x00cc0020, 0x008800c6, 0x00660046, 0x00f00021, 0x00000042, 0x00ff0062, 0x00aa0029, 0x005a0049]);
      if (t === 81) {
        // bounds(16) xDest yDest xSrc ySrc cxSrc cySrc offBmi cbBmi offBits cbBits usage rop cxDest cyDest
        const offBmi = 8 + 72, offBits = offBmi + d.bmi.length;
        body.i32(0).i32(0).i32(0).i32(0).i32(pt()).i32(pt()).i32(0).i32(0).i32(4).i32(3).u32(offBmi).u32(d.bmi.length).u32(offBits).u32(d.bits.length).u32(0).u32(rop).i32(r.int(-300, 300)).i32(r.int(-300, 300));
        body.bytes(d.bmi).bytes(d.bits);
      } else {
        // bounds(16) xDest yDest cxDest cyDest rop(BitBlt)/blendfn(AlphaBlend) xSrc ySrc xform(24) bk usage offBmi cbBmi offBits cbBits [cxSrc cySrc]
        const fixed = 8 + 92 + (t === 76 ? 0 : 8);
        const offBmi = d ? fixed : 0, offBits = d ? fixed + d.bmi.length : 0;
        body.i32(0).i32(0).i32(0).i32(0).i32(pt()).i32(pt()).i32(r.int(-300, 300)).i32(r.int(-300, 300));
        if (t === 114) body.u8(0).u8(0).u8(255).u8(r.chance(0.7) ? 1 : 0); else body.u32(rop);
        body.i32(0).i32(0);
        for (let k = 0; k < 6; k++) body.f32(k === 0 || k === 3 ? 1 : 0);
        body.u32(0).u32(0).u32(offBmi).u32(d ? d.bmi.length : 0).u32(offBits).u32(d ? d.bits.length : 0);
        if (t !== 76) body.i32(4).i32(3);
        if (d) body.bytes(d.bmi).bytes(d.bits);
      }
      break;
    }
    case 70: body.u32(4).u32(0x12345678); break; // a GDI comment (not EMF+)
    default: body.u32(r.int(0, 100)); break; // unknown record
  }
  body.pad4();
  let size = 8 + body.length;
  if (r.chance(0.02)) size = r.pick([4, size + 4000, size - 4]);
  return { t, size, body: body.b };
}

function emfHeader(r, extra = 0) {
  const h = new W();
  const size = 108 + extra;
  h.u32(1).u32(size).i32(0).i32(0).i32(1000).i32(800);
  h.i32(r.int(-500, 0)).i32(r.int(-500, 0)).i32(r.int(2000, 30000)).i32(r.int(2000, 30000));
  h.u32(0x464d4520).u32(0x10000).u32(0).u32(0).u16(0).u16(0).u32(0).u32(0).u32(0);
  h.i32(r.pick([1920, 1024, 96])).i32(r.pick([1080, 768, 96])).i32(r.pick([508, 320, 0])).i32(r.pick([285, 240, 0]));
  h.u32(0).u32(0).u32(0).i32(0).i32(0);
  while (h.length < size) h.u8(0);
  return h.b;
}

function emf(r) {
  const out = [...emfHeader(r)];
  const n = r.int(1, 40);
  for (let k = 0; k < n; k++) {
    const rec = emfRecord(r);
    const w = new W().u32(rec.t).u32(rec.size).bytes(rec.body);
    out.push(...w.b);
  }
  if (r.chance(0.8)) out.push(...new W().u32(14).u32(20).u32(0).u32(16).u32(20).b);
  if (r.chance(0.05)) out.length = r.int(45, out.length);
  return Uint8Array.from(out);
}

/** EMF+ only: the header comment (not dual), image objects, draws, transforms. */
function emfPlus(r) {
  const out = [...emfHeader(r)];
  const plusRec = (type, flags, data) => new W().u16(type).u16(flags).u32(12 + data.length).u32(data.length).bytes(data).b;
  const comment = (recs) => { const d = [...new W().u32(0x2b464d45).b, ...recs]; return new W().u32(70).u32(12 + d.length).u32(d.length).bytes(d).b; };
  out.push(...comment(plusRec(0x4001, r.chance(0.9) ? 0 : 1, new W().u32(0xdbc01002).u32(0).u32(r.pick([96, 0, 150])).u32(96).b)));
  const n = r.int(1, 12);
  for (let k = 0; k < n; k++) {
    const t = r.pick([0x4008, 0x4008, 0x401a, 0x401b, 0x402a, 0x402b, 0x402c, 0x402d, 0x402e, 0x402f, 0x4030, 0x4025, 0x4026]);
    let rec;
    if (t === 0x4008) {
      const id = r.int(0, 2);
      const kind = r.chance(0.7) ? 1 : 2;
      let img;
      if (r.chance(0.3)) img = new W().u32(0xdbc01002).u32(1).i32(2).i32(2).i32(0).u32(0).u32(1).bytes([0x89, 0x50, 0x4e, 0x47, 1, 2, 3, 4]).b; // compressed
      else {
        const w = r.int(1, 3), h = r.int(1, 3), bpp = r.pick([32, 24]);
        const stride = r.chance(0.1) ? r.pick([0, -4]) : Math.ceil((w * bpp) / 32) * 4;
        const fmt = (bpp << 8) | (bpp === 32 ? r.pick([0x40000, 0x80000, 0]) : 0) | (r.chance(0.05) ? 0x10000 : 0);
        const px = Array.from({ length: Math.max(0, stride) * h }, () => r.int(0, 255));
        img = new W().u32(0xdbc01002).u32(kind).i32(w).i32(h).i32(stride).u32(fmt).u32(0).bytes(px).b;
      }
      if (r.chance(0.3) && img.length > 8) {
        const cut = r.int(1, img.length - 1);
        rec = [...plusRec(t, 0x8000 | (5 << 8) | id, [...new W().u32(img.length).b, ...img.slice(0, cut)]),
          ...plusRec(t, 0x8000 | (5 << 8) | id, [...new W().u32(img.length).b, ...img.slice(cut)])];
      } else rec = plusRec(t, (5 << 8) | id, img);
    } else if (t === 0x401a || t === 0x401b) {
      const intPts = r.chance(0.3);
      const d = new W().u32(0).i32(r.pick([2, 2, 1])).f32(0).f32(0).f32(r.int(1, 3)).f32(r.int(1, 3));
      if (t === 0x401a) intPts ? d.u16(r.int(0, 50)).u16(r.int(0, 50)).u16(r.int(-20, 60) & 0xffff).u16(r.int(-20, 60) & 0xffff) : d.f32(r.int(0, 50)).f32(r.int(0, 50)).f32(r.int(-20, 60)).f32(r.int(-20, 60));
      else { d.u32(3); for (let k = 0; k < 3; k++) intPts ? d.u16(r.int(0, 60)).u16(r.int(0, 60)) : d.f32(r.int(0, 60)).f32(r.int(0, 60)); }
      rec = plusRec(t, (intPts ? 0x4000 : 0) | (r.chance(0.1) ? 0x0800 : 0) | r.int(0, 2), d.b);
    } else if (t === 0x4030) rec = plusRec(t, r.int(0, 7), new W().f32(r.pick([1, 0, 2.5])).b);
    else if (t === 0x4025 || t === 0x4026) rec = plusRec(t, 0, new W().u32(r.int(0, 2)).b);
    else {
      const d = new W();
      for (let k = 0; k < 6; k++) d.f32(r.int(-3, 3) * 0.5 + (k === 0 || k === 3 ? 1 : 0));
      rec = plusRec(t, r.chance(0.5) ? 0x2000 : 0, d.b);
    }
    out.push(...comment(rec));
  }
  out.push(...new W().u32(14).u32(20).u32(0).u32(16).u32(20).b);
  return Uint8Array.from(out);
}

function wmf(r) {
  const b = new W();
  const placeable = r.chance(0.7);
  if (placeable) { b.u32(0x9ac6cdd7).u16(0).u16(r.int(-100, 100) & 0xffff).u16(r.int(-100, 100) & 0xffff).u16(r.int(200, 3000)).u16(r.int(200, 3000)).u16(1440).u32(0).u16(0); }
  b.u16(r.pick([1, 2])).u16(9).u16(0x300).u32(0).u16(4).u32(0).u16(0);
  const rec = (fn, words) => { b.u32(3 + words.length).u16(fn); for (const w of words) b.u16(w & 0xffff); };
  const p = () => r.int(-100, 3000);
  const n = r.int(1, 30);
  for (let k = 0; k < n; k++) {
    switch (r.pick(['org', 'ext', 'align', 'color', 'save', 'restore', 'pen', 'brush', 'font', 'other', 'select', 'del', 'move', 'line', 'pline', 'pgon', 'rect', 'text', 'ext', 'etext', 'etext', 'junk'])) {
      case 'org': rec(0x020b, [p(), p()]); break;
      case 'ext': rec(0x020c, [r.chance(0.1) ? 0 : r.int(100, 4000), r.chance(0.1) ? 0 : r.int(100, 4000)]); break;
      case 'align': rec(0x012e, [r.int(0, 31)]); break;
      case 'color': rec(0x0209, [r.int(0, 0xffff), r.int(0, 0xff)]); break;
      case 'save': rec(0x001e, []); break;
      case 'restore': rec(0x0127, [r.int(-4, 1)]); break;
      case 'pen': rec(0x02fa, [r.int(0, 6), r.int(0, 30), 0, r.int(0, 0xffff), r.int(0, 0xff)]); break;
      case 'brush': rec(0x02fc, [r.int(0, 2), r.int(0, 0xffff), r.int(0, 0xff), 0]); break;
      case 'font': {
        const face = r.pick(FACES);
        const bytes = [];
        for (let i = 0; i < Math.min(face.length, 31); i++) bytes.push(face.charCodeAt(i) & 0xff);
        bytes.push(0);
        if (bytes.length % 2) bytes.push(0);
        const charset = r.chance(0.2) ? 2 : 0;
        const words = [r.chance(0.5) ? -r.int(6, 300) : r.int(1, 300), 0, 0, 0, r.pick([400, 700]), (r.chance(0.3) ? 1 : 0), charset << 8, 0, 0];
        for (let i = 0; i < bytes.length; i += 2) words.push(bytes[i] | (bytes[i + 1] << 8));
        rec(0x02fb, words);
        break;
      }
      case 'other': rec(r.pick([0x00f7, 0x0142, 0x01f9, 0x06ff]), [0, 0]); break;
      case 'select': rec(0x012d, [r.int(0, 5)]); break;
      case 'del': rec(0x01f0, [r.int(0, 5)]); break;
      case 'move': rec(0x0214, [p(), p()]); break;
      case 'line': rec(0x0213, [p(), p()]); break;
      case 'pline': case 'pgon': { const c = r.chance(0.03) ? 30000 : r.int(0, 6); const w = [c]; for (let i = 0; i < Math.min(c, 6); i++) w.push(p(), p()); rec(r.chance(0.5) ? 0x0325 : 0x0324, w); break; }
      case 'rect': rec(0x041b, [p(), p(), p(), p()]); break;
      case 'text': {
        const s = r.pick(['Hi', 'P = Po', 'x', '  ', '\x93ab\x94', 'abc']);
        const w = [s.length];
        for (let i = 0; i < s.length; i += 2) w.push(s.charCodeAt(i) | ((s.charCodeAt(i + 1) || 0) << 8));
        w.push(p(), p());
        rec(0x0521, w);
        break;
      }
      case 'etext': {
        const s = r.pick(['abc', 'Q', 'xyz12', ' ']);
        const opt = r.pick([0, 2, 4, 6]);
        const w = [p(), p(), s.length, opt];
        if (opt & 6) w.push(0, 0, 100, 100);
        for (let i = 0; i < s.length; i += 2) w.push(s.charCodeAt(i) | ((s.charCodeAt(i + 1) || 0) << 8));
        if (r.chance(0.6)) for (let i = 0; i < s.length; i++) w.push(r.int(0, 300));
        rec(0x0a32, w);
        break;
      }
      default: rec(r.int(0x0100, 0x0900), [r.int(0, 100)]);
    }
  }
  if (r.chance(0.9)) rec(0, []);
  while (b.length < 40) b.u8(0);
  return Uint8Array.from(b.b);
}

const seed = Number(opt('--seed', '1'));
const r = rng(seed);
const out = opt('--out', join(here, '..', 'tests', 'captures', 'f3', 'metafile-gen.f3.jsonl'));
mkdirSync(dirname(out), { recursive: true });
const lines = [`{"f":"note","text":"tools/capture-f3.mjs --seed ${seed}: generated EMF, EMF+ and WMF through the frozen renderEmf/renderWmf (open fonts)"}`];
const seenFaces = new Set();
let errors = 0;
const stats = new Map();
const cases = [
  ...Array.from({ length: Number(opt('--emf', '320')) }, () => ['emf', r.chance(0.2) ? emfPlus(r) : emf(r)]),
  ...Array.from({ length: Number(opt('--wmf', '160')) }, () => ['wmf', wmf(r)]),
];
for (const [kind, bytes] of cases) {
  const ctx = { faces: new FaceResolver(provider) };
  const dest = { x: r.int(0, 5000), y: r.int(0, 5000), w: r.int(100, 9000), h: r.int(100, 9000) };
  let v;
  try {
    const items = (kind === 'emf' ? renderEmf : renderWmf)(ctx, bytes, dest);
    for (const it of items) { const k = `${kind}:${it.t}${it.t === 'glyphs' && it.asPath ? '(asPath)' : ''}${it.t === 'image' && it.crop ? '(crop)' : ''}`; stats.set(k, (stats.get(k) ?? 0) + 1); }
    v = f3Value(kind === 'emf' ? 'renderEmf' : 'renderWmf', [ctx, bytes, dest], items);
  } catch { v = 'error'; errors++; }
  for (const [req, a] of ctx.faces.cache) if (!seenFaces.has(req)) { seenFaces.add(req); lines.push(faceLine(req, a)); }
  const hex = Buffer.from(bytes).toString('hex');
  lines.push(`{"f":"${kind}","hex":"${hex}","dest":[${dest.x},${dest.y},${dest.w},${dest.h}],"v":"${v}"}`);
}
writeFileSync(out, lines.join('\n') + '\n');
console.log(`${cases.length} metafiles (${errors} TS errors) -> ${out}`);
console.log([...stats].sort().map(([k, v]) => `${k} ${v}`).join(', '));
