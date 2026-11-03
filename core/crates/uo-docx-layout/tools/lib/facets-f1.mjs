// P3 facets of frontier F1 (fonts and the 600-DPI width model; record "Wave 1 — F1"), recorded
// by facets.mjs during every layout of capture-layout.mjs, with their inputs dumped for the Rust
// replay (tests/f1_parity.rs), which recomputes every value from the inputs and rebuilds the same
// streams:
//
//   p3.metrics  `faceMetrics(font)` (fonts.ts:57), the first call per font object:
//               tag 'm', u32 font label [u16s sha256 the first time in this stream], ascent,
//               descent, lineHeight, underlinePos, underlineThick, bareAscent, optNum supOffset;
//   p3.advance  `advanceTwips(ctx, face, cp, sizePt, w)` (para.ts:378), every distinct input in
//               first-call order (the function is pure under the fixed options, LQ12):
//               tag 'a', u32 font label [sha], num cp, num sizePt, optNum w, num result;
//   p3.kern     `SfntFont.kernGlyphsTwips(g1, g2, sizePt)` (sfnt.ts:188, K7; `kernTwips` goes
//               through it), every distinct input: tag 'k', u32 font label [sha], num g1, num g2,
//               num sizePt, num result;
//   p3.faces    `FaceResolver.get(family, bold, italic)` (fonts.ts:235), the first call of every
//               distinct request (family as passed, case kept) over all passes — the resolver
//               cache is per layout call in Rust (K4, LQ19), so a repeat is a cache hit there:
//               tag 'r', optU16s family, bool bold, bool italic, u32 face label (first appearance
//               of the returned FontFace object, 0 = threw) and, the first time, the face: u16s key,
//               u16s family, bool bold, italic, synthBold, synthItalic, standIn, u32 font label
//               [sha].
//
// Font labels number the LayoutFont objects by first appearance; the sha256 is of the font file
// bytes the object reads (`font.view`). The dump `<key>.f1.tsv` (tab-separated, numbers as the 16
// hex digits of the f64 bits, strings `-` = absent or `:` + encodeURIComponent):
//   F <label> <sha256>                                   a font object
//   M <label> <asc> <desc> <lineHeight> <ulPos> <ulThick> <bareAsc> <supOffset|->
//   A <label> <cp> <sizePt> <w|-> <result>              (g1, g2 of K are integers)
//   K <label> <g1> <g2> <sizePt> <result>
//   T <name> <altName> <family> <panose1> <charset> <sig.usb0>   the font table, once, key order
//   R <family> <bold> <italic> <face label>
//   D <face label> <key> <family> <bold> <italic> <synthBold> <synthItalic> <standIn> <font label>
import { createHash } from 'node:crypto';
import { Sink } from './sink.mjs';

/** Module functions F1 wraps (merged into facets.mjs WRAPS). */
export const F1_WRAPS = { 'fonts.js': ['faceMetrics'], 'para.js': ['advanceTwips'] };

const TAB = String.fromCharCode(9), NL = String.fromCharCode(10);
const b8 = Buffer.alloc(8);
const hex = (x) => { b8.writeDoubleBE(x !== x ? NaN : x); return b8.toString('hex'); };
const enc = (s) => {
  if (s === undefined || s === null) return '-';
  try { return ':' + encodeURIComponent(String(s)); } catch { return ':' + encodeURIComponent(String(s).toWellFormed()); }
};
const flag = (v) => (v ? '1' : '0');

export function f1Recorder({ FaceResolver, SfntFont }) {
  const digest = new WeakMap();
  const shaOf = (font) => {
    let h = digest.get(font);
    if (h === undefined) {
      const v = font.view;
      h = createHash('sha256').update(new Uint8Array(v.buffer, v.byteOffset, v.byteLength)).digest('hex');
      digest.set(font, h);
    }
    return h;
  };
  let st = null;
  const fontLabel = (font) => {
    let l = st.fonts.get(font);
    if (l === undefined) {
      l = st.fonts.size + 1;
      st.fonts.set(font, l);
      st.dump.push(['F', l, shaOf(font)].join(TAB));
    }
    return l;
  };
  // u32(label) [+ u16s(sha) the first time the label appears in this stream]
  const fontRef = (s, seen, font) => {
    const l = fontLabel(font);
    s.u32(l);
    if (!seen.has(l)) { seen.add(l); s.u16s(shaOf(font)); }
  };
  const metrics = (font, m) => {
    const l = fontLabel(font);
    if (st.metricsDone.has(l)) return;
    st.metricsDone.add(l);
    const s = st.metrics.tag('m');
    fontRef(s, st.metricsSeen, font);
    s.num(m.ascent).num(m.descent).num(m.lineHeight).num(m.underlinePos).num(m.underlineThick).num(m.bareAscent).optNum(m.supOffset);
    st.dump.push(['M', l, hex(m.ascent), hex(m.descent), hex(m.lineHeight), hex(m.underlinePos), hex(m.underlineThick), hex(m.bareAscent),
      m.supOffset === undefined ? '-' : hex(m.supOffset)].join(TAB));
  };
  const advance = (args, r) => {
    const font = args[1].font, cp = args[2], size = args[3], w = args[4];
    const l = fontLabel(font);
    const key = `${l}|${hex(cp)}|${hex(size)}|${w === undefined ? '-' : hex(w)}`;
    if (st.advanceDone.has(key)) return;
    st.advanceDone.add(key);
    const s = st.advance.tag('a');
    fontRef(s, st.advanceSeen, font);
    s.num(cp).num(size).optNum(w).num(r);
    st.dump.push(['A', l, hex(cp), hex(size), w === undefined ? '-' : hex(w), hex(r)].join(TAB));
  };
  const kern = (font, g1, g2, size, r) => {
    const l = fontLabel(font);
    const key = `${l}|${g1}|${g2}|${hex(size)}`;
    if (st.kernDone.has(key)) return;
    st.kernDone.add(key);
    const s = st.kern.tag('k');
    fontRef(s, st.kernSeen, font);
    s.num(g1).num(g2).num(size).num(r);
    st.dump.push(['K', l, g1, g2, hex(size), hex(r)].join(TAB));
  };
  const resolved = (resolver, family, bold, italic, face) => {
    if (!st.tableDone) {
      st.tableDone = true;
      for (const [name, info] of Object.entries(resolver.fontTable ?? {})) {
        st.dump.push(['T', enc(name), enc(info?.altName), enc(info?.family), enc(info?.panose1), enc(info?.charset), enc(info?.sig?.usb0)].join(TAB));
      }
    }
    const rk = `${enc(family)}|${flag(bold)}|${flag(italic)}`;
    if (st.facesDone.has(rk)) return;
    st.facesDone.add(rk);
    let label = 0, fresh = false;
    if (face) {
      label = st.faces.get(face);
      if (label === undefined) { label = st.faces.size + 1; st.faces.set(face, label); fresh = true; }
    }
    const s = st.facesSink.tag('r').optU16s(family).bool(!!bold).bool(!!italic).u32(label);
    st.dump.push(['R', enc(family), flag(bold), flag(italic), label].join(TAB));
    if (fresh) {
      s.u16s(face.key).u16s(face.family).bool(!!face.bold).bool(!!face.italic).bool(!!face.synthBold).bool(!!face.synthItalic).bool(!!face.standIn);
      fontRef(s, st.facesSeen, face.font);
      st.dump.push(['D', label, enc(face.key), enc(face.family), flag(face.bold), flag(face.italic), flag(face.synthBold), flag(face.synthItalic),
        flag(face.standIn), fontLabel(face.font)].join(TAB));
    }
  };

  const origKern = SfntFont.prototype.kernGlyphsTwips;
  const patchedKern = function (g1, g2, size) {
    const r = origKern.call(this, g1, g2, size);
    if (st) kern(this, g1, g2, size, r);
    return r;
  };
  const origGet = FaceResolver.prototype.get;
  const patchedGet = function (family, bold, italic) {
    let face;
    try { face = origGet.call(this, family, bold, italic); } catch (e) { if (st) resolved(this, family, bold, italic, null); throw e; }
    if (st) resolved(this, family, bold, italic, face);
    return face;
  };

  return {
    /** The loader-hook reports (`__uoFacet`) F1 handles. */
    report(name, args, r) {
      if (!st) return;
      if (name === 'faceMetrics') metrics(args[0], r);
      else if (name === 'advanceTwips') advance(args, r);
    },
    start() {
      st = {
        fonts: new Map(), faces: new Map(), dump: [], tableDone: false,
        metrics: new Sink(), metricsSeen: new Set(), metricsDone: new Set(),
        advance: new Sink(), advanceSeen: new Set(), advanceDone: new Set(),
        kern: new Sink(), kernSeen: new Set(), kernDone: new Set(),
        facesSink: new Sink(), facesSeen: new Set(), facesDone: new Set(),
      };
      SfntFont.prototype.kernGlyphsTwips = patchedKern;
      FaceResolver.prototype.get = patchedGet;
    },
    /** `{ facets, dump }`: the facet values and the text of `<key>.f1.tsv`. */
    finish() {
      SfntFont.prototype.kernGlyphsTwips = origKern;
      FaceResolver.prototype.get = origGet;
      const s = st;
      st = null;
      return {
        facets: {
          'p3.metrics': s.metrics.finish().value, 'p3.advance': s.advance.finish().value,
          'p3.kern': s.kern.finish().value, 'p3.faces': s.facesSink.finish().value,
        },
        dump: s.dump.join(NL) + NL,
      };
    },
  };
}
