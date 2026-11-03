// P3 facets of frontier F5 (lines and paint; record "Wave 1 — F5"), recorded by facets.mjs during
// every layout of capture-layout.mjs, with their inputs dumped for the Rust replay
// (tests/f5_parity.rs), which runs the Rust `LineBreaker`/`paint_line_in` on the decoded inputs
// and rebuilds the same streams:
//
//   p3.lines  every `LineBreaker.next(frame)`, `reset(mark)` and `peekWordWidth()` (lines.ts), in
//             call order: one `bytes` item per call holding its raw encoding (below), so the Rust
//             side compares call by call and the facet digest covers the sequence;
//   p3.paint  every `paintLine(ctx, out, line, frameX, top)` (paint.ts:19): one `bytes` item per
//             call holding the P1 canonical stream (canon.mjs, version 1) of a page whose items
//             are the ones the call appended — faces as descriptors without file digests.
//
// Per-call raw encodings (the Sink conventions of sink.mjs: num = f64 LE with NaN canonical,
// u32 LE, bool = 1 byte, bytes = u64 LE length + bytes, u16s = UTF-16LE as bytes, opt* = presence
// bool + value):
//   next  := 'n' u32(breaker) num(frame.left right) bool(gap) optNum(next.left) optNum(next.right)
//            num(start end width xStart asc desc height baseline) bool(last) optU16s(breakAfter)
//            bool(clear) optU16s(gapTab) bool(spanned) bool(hrFrame?)[num(left right)]
//            u32(n) placed* bool(marker?)[u32(n) placed*]
//   placed:= ('p' u32(glyph index) | 'm' u32(marker glyph index) | 'h' num(offset) | '?')
//            num(x) num(w) optU16s(leader)
//   reset := 'r' u32(breaker) num(pos) num(lineIdx) bool(trailingBreak) num(hyphenRun)
//   peek  := 'k' u32(breaker) num(result)
//
// The dump `<key>.f5.bin` (a raw Sink stream) holds the inputs, each defined before its first use:
//   'F' u32(face) u16s(key family) bool(bold italic synthBold synthItalic standIn)
//       num(ascent descent underlinePos underlineThick) num(unitsPerEm usWinDescent descender)
//   'S' u32(style) style        'P' u32(para) para        'A' u32(face) num(cp sizePt result)
//   'V' u32(face) num(em) optNum(vdmx yMin)    'W' u32(face) num(cp advanceWidth unitsPerEm)
//   'C' u32(face) num(cp) bool(colr)
//   'B' u32(breaker) u32(para) optNum(grid) settings
//   'N' u32(breaker) num(frame.left right) bool(gap) optNum(next.left right) bytes(TS raw)
//   'R' u32(breaker) num(pos lineIdx) bool(trailingBreak) num(hyphenRun) bytes(TS raw)
//   'K' u32(breaker) bytes(TS raw)
//   'D' u32(para) num(frameX top baseline height) bool(hrFrame?)[num num] u32(n) placed*
//       bool(marker?)[u32(n) placed*] bytes(TS raw)              (placed as in `next`)
//   'X' u16s(why) bytes(TS raw)                                  (a paint call not replayable)
//   settings := num(defaultTabStop compatibilityMode) bool(doNotUseIndentAsNumberingTabStop
//               splitPgBreakAndParaMark useWord97LineBreakRules) optNum(consecutiveHyphenLimit
//               hyphenationZone)
//   style := num(sizePt) u16s(color) bool(underline?)[u16s(val) optU16s(color)] bool(strike dstrike)
//            optU16s(highlight shading) num(shift) optNum(alpha) bool(outline) bool(fx?)[textfx]
//            optU16s(fxKey) optNum(rpr.w rpr.spacing) optU16s(rpr.vertAlign) optNum(rpr.sz)
//            bool(textShadow?)[num(blur dist dir)] bool(!!fontOfSlot(rFonts, 'eastAsia', theme))
//   para  := num(indLeft indRight firstLine) u16s(jc lineRule) num(line) u32(n)(u16s(val) num(pos)
//            optU16s(leader))* u32(markStyle) u32(markFace)
//            bool(marker?)[num(width) u16s(suff jc) u32(n) glyph*] u32(n) glyph*
//   glyph := u16s(kind ch) num(cp adv asc desc) optNum(nAsc nDesc) num(offset runIdx)
//            bool(breakAfter glue clear fixed hang hyph) optNum(hr feShrink) num(sizePt)
//            u32(face) u32(style) bool(ptab?)[u16s(alignment) optU16s(leader)]
//            bool(obj?)[bool(shape?)[optNum(hrPct) num(widthTwips heightTwips) optU16s(hrAlign)
//            u16s(fill ?? 'A0A0A0') bool(hrNoShade)]] bool(box?)[num(side) bool(checked)]
//            optNum(subGid) bool(math?)[num(w) u32(n)(num(cp x y) u32(face) num(sizePt) u32(style))*
//            u32(n)(num(x y w h) u16s(color))*]
//   textfx := the canon.mjs `textfx` encoding
// Labels number the FontFace, RunStyle and PreparedPara objects and the breakers by first
// appearance. The helpers the recorder calls itself (advanceTwips, faceMetrics) run with the facet
// reporter off, so F1's streams only see the engine's calls.
import { createHash } from 'node:crypto';
import { canonLayout } from './canon.mjs';

/** Module functions F5 wraps (merged into facets.mjs WRAPS). */
export const F5_WRAPS = { 'paint.js': ['paintLine'] };

const T = (v) => v !== undefined && v !== null;

/** The raw byte encoding of sink.mjs, without the digest (one per call, and the dump). */
class Raw {
  constructor() { this.parts = []; }
  #b(n) { const b = Buffer.alloc(n); this.parts.push(b); return b; }
  tag(c) { this.parts.push(Buffer.from([c.charCodeAt(0)])); return this; }
  u32(v) { this.#b(4).writeUInt32LE(v >>> 0); return this; }
  num(v) { this.#b(8).writeDoubleLE(v !== v ? NaN : v); return this; }
  bool(v) { this.parts.push(Buffer.from([v ? 1 : 0])); return this; }
  bytes(u8) { this.#b(8).writeBigUInt64LE(BigInt(u8.length)); this.parts.push(Buffer.from(u8)); return this; }
  digestOf(u8) { this.#b(8).writeBigUInt64LE(BigInt(u8.length)); this.parts.push(createHash('sha256').update(u8).digest()); return this; }
  optNum(v) { const h = T(v); this.bool(h); if (h) this.num(v); return this; }
  u16s(str) {
    const s = String(str);
    const b = Buffer.alloc(s.length * 2);
    for (let i = 0; i < s.length; i++) b.writeUInt16LE(s.charCodeAt(i), 2 * i);
    return this.bytes(b);
  }
  optU16s(s) { const h = T(s); this.bool(h); if (h) this.u16s(s); return this; }
  finish() { return Buffer.concat(this.parts); }
}

const quiet = (fn) => {
  const f = globalThis.__uoFacet;
  globalThis.__uoFacet = undefined;
  try { return fn(); } finally { globalThis.__uoFacet = f; }
};

function textfx(s, fx) {
  const ltrb = (b) => { s.num(b.l).num(b.t).num(b.r).num(b.b); };
  const fill = (f) => {
    if (f.kind === 'none') { s.tag('n'); return; }
    if (f.kind === 'solid') { s.tag('s').u16s(f.hex).num(f.alpha); return; }
    s.tag('g').u32(f.stops.length);
    for (const st of f.stops) s.num(st.pos).u16s(st.hex).num(st.alpha);
    s.num(f.angleDeg).bool(!!f.scaled).optU16s(f.path).bool(T(f.fillTo)); if (T(f.fillTo)) ltrb(f.fillTo);
  };
  s.bool(T(fx.fill)); if (T(fx.fill)) fill(fx.fill);
  const o = fx.outline; s.bool(T(o)); if (T(o)) { s.num(o.widthPt); fill(o.fill); s.optU16s(o.dash).u16s(o.cap).u16s(o.join); }
  const h = fx.shadow; s.bool(T(h)); if (T(h)) s.num(h.blurPt).num(h.distPt).num(h.dirDeg).num(h.sx).num(h.sy).num(h.kxDeg).num(h.kyDeg).u16s(h.algn).u16s(h.hex).num(h.alpha);
  const g = fx.glow; s.bool(T(g)); if (T(g)) s.num(g.radPt).u16s(g.hex).num(g.alpha);
  const r = fx.reflection; s.bool(T(r)); if (T(r)) s.num(r.blurPt).num(r.stA).num(r.stPos).num(r.endA).num(r.endPos).num(r.distPt).num(r.dirDeg).u16s(r.algn);
}

export function f5Recorder({ LineBreaker, advanceTwips, faceMetrics, fontOfSlot }) {
  const orig = { next: LineBreaker.prototype.next, reset: LineBreaker.prototype.reset, peek: LineBreaker.prototype.peekWordWidth };
  let st = null;

  const label = (map, obj, define) => {
    let l = map.get(obj);
    if (l === undefined) { l = map.size; map.set(obj, l); define(l); }
    return l;
  };
  const face = (f) => label(st.faces, f, (l) => {
    const m = quiet(() => faceMetrics(f.font));
    const fm = f.font.metrics;
    st.d.tag('F').u32(l).u16s(f.key ?? '').u16s(f.family ?? '').bool(!!f.bold).bool(!!f.italic).bool(!!f.synthBold).bool(!!f.synthItalic).bool(!!f.standIn)
      .num(m.ascent).num(m.descent).num(m.underlinePos).num(m.underlineThick).num(fm.unitsPerEm).num(fm.usWinDescent).num(fm.descender);
  });
  const style = (s, ctx) => {
    let l = st.styles.get(s);
    if (l !== undefined) return l;
    const ea = quiet(() => !!fontOfSlot(s.rpr?.rFonts, 'eastAsia', ctx.fmt.theme));
    l = st.styles.size; st.styles.set(s, l);
    const d = st.d;
    d.tag('S').u32(l).num(s.sizePt).u16s(s.color);
    d.bool(T(s.underline)); if (T(s.underline)) d.u16s(s.underline.val).optU16s(s.underline.color);
    d.bool(!!s.strike).bool(!!s.dstrike).optU16s(s.highlight).optU16s(s.shading).num(s.shift).optNum(s.alpha).bool(!!s.outline);
    d.bool(T(s.fx)); if (T(s.fx)) textfx(d, s.fx);
    d.optU16s(s.fxKey);
    const r = s.rpr ?? {};
    d.optNum(r.w).optNum(r.spacing).optU16s(r.vertAlign).optNum(r.sz);
    d.bool(T(r.textShadow)); if (T(r.textShadow)) d.num(r.textShadow.blur).num(r.textShadow.dist).num(r.textShadow.dir);
    d.bool(ea);
    return l;
  };
  /** faces and styles a glyph references, defined before the paragraph record */
  const refs = (g, ctx) => {
    face(g.face); style(g.style, ctx);
    if (g.math) for (const it of g.math.items) { face(it.face); style(it.style, ctx); }
  };
  const adv = (ctx, f, cp, size) => {
    const fl = face(f);
    const key = `${fl}|${cp}|${size}`;
    if (st.advs.has(key)) return;
    st.advs.add(key);
    st.d.tag('A').u32(fl).num(cp).num(size).num(quiet(() => advanceTwips(ctx, f, cp, size)));
  };
  const glyph = (d, g) => {
    d.u16s(g.kind).u16s(g.ch ?? '').num(g.cp).num(g.adv).num(g.asc).num(g.desc).optNum(g.nAsc).optNum(g.nDesc).num(g.offset).num(g.runIdx)
      .bool(!!g.breakAfter).bool(!!g.glue).bool(!!g.clear).bool(!!g.fixed).bool(!!g.hang).bool(!!g.hyph)
      .optNum(g.hr).optNum(g.feShrink).num(g.sizePt).u32(st.faces.get(g.face)).u32(st.styles.get(g.style));
    d.bool(T(g.ptab)); if (T(g.ptab)) d.u16s(g.ptab.alignment).optU16s(g.ptab.leader);
    d.bool(T(g.obj));
    if (T(g.obj)) {
      const sh = g.obj.shape;
      d.bool(T(sh));
      if (T(sh)) d.optNum(sh.hrPct).num(sh.widthTwips).num(sh.heightTwips).optU16s(sh.hrAlign).u16s(sh.style?.fill ?? 'A0A0A0').bool(!!sh.hrNoShade);
    }
    d.bool(T(g.box)); if (T(g.box)) d.num(g.box.side).bool(!!g.box.checked);
    d.optNum(g.subGid);
    d.bool(T(g.math));
    if (T(g.math)) {
      const m = g.math;
      d.num(m.w).u32(m.items.length);
      for (const it of m.items) d.num(it.cp).num(it.x).num(it.y).u32(st.faces.get(it.face)).num(it.sizePt).u32(st.styles.get(it.style));
      d.u32(m.rules.length);
      for (const r of m.rules) d.num(r.x).num(r.y).num(r.w).num(r.h).u16s(r.color);
    }
  };
  /** the paragraph label; the paragraph, its faces, styles and the advances lines.ts asks for are
   *  defined on first use */
  const para = (p, ctx) => {
    let l = st.paras.get(p);
    if (l !== undefined) return l;
    style(p.markStyle, ctx); face(p.markFace);
    const mk = p.marker;
    for (const g of [...(mk?.glyphs ?? []), ...p.glyphs]) refs(g, ctx);
    for (const g of p.glyphs) adv(ctx, g.face, 0x2d, g.sizePt);
    if (mk) adv(ctx, mk.glyphs[0]?.face ?? p.markFace, 0x20, mk.glyphs[0]?.sizePt ?? p.markStyle.sizePt);
    l = st.paras.size;
    const idx = new Map(p.glyphs.map((g, i) => [g, i]));
    const midx = new Map((mk?.glyphs ?? []).map((g, i) => [g, i]));
    st.paras.set(p, l);
    st.index.set(p, { idx, midx });
    const d = st.d;
    d.tag('P').u32(l).num(p.indLeft).num(p.indRight).num(p.firstLine).u16s(p.jc).u16s(p.lineRule).num(p.line).u32(p.tabs.length);
    for (const t of p.tabs) d.u16s(t.val).num(t.pos).optU16s(t.leader);
    d.u32(st.styles.get(p.markStyle)).u32(st.faces.get(p.markFace));
    d.bool(T(mk));
    if (T(mk)) { d.num(mk.width).u16s(mk.suff).u16s(mk.jc).u32(mk.glyphs.length); for (const g of mk.glyphs) glyph(d, g); }
    d.u32(p.glyphs.length);
    for (const g of p.glyphs) glyph(d, g);
    return l;
  };
  const breaker = (br) => {
    let id = st.breakers.get(br);
    if (id !== undefined) return id;
    const pl = para(br.p, br.ctx);
    id = st.breakers.size;
    st.breakers.set(br, id);
    const s = br.ctx.fmt.settings;
    st.d.tag('B').u32(id).u32(pl).optNum(br.grid)
      .num(s.defaultTabStop).num(s.compatibilityMode).bool(!!s.compat?.doNotUseIndentAsNumberingTabStop)
      .bool(!!s.compat?.splitPgBreakAndParaMark).bool(!!s.compat?.useWord97LineBreakRules)
      .optNum(s.consecutiveHyphenLimit).optNum(s.hyphenationZone);
    return id;
  };
  /** `placed` of a line, with the glyph identity; false when a glyph is not the paragraph's */
  const placed = (r, p, list) => {
    const ix = st.index.get(p);
    let ok = true;
    r.u32(list.length);
    for (const q of list) {
      const i = ix.idx.get(q.g), j = ix.midx.get(q.g);
      if (i !== undefined) r.tag('p').u32(i);
      else if (j !== undefined) r.tag('m').u32(j);
      else if (q.g.autoHyphen) r.tag('h').num(q.g.offset);
      else { r.tag('?'); ok = false; }
      r.num(q.x).num(q.w).optU16s(q.leader);
    }
    return ok;
  };
  const frame = (r, f) => { r.num(f.left).num(f.right).bool(!!f.gap).optNum(f.next?.left).optNum(f.next?.right); };

  function next(f) {
    if (!st) return orig.next.call(this, f);
    const id = breaker(this);
    const l = orig.next.call(this, f);
    st.lineOwner.set(l, this.p);
    const r = new Raw();
    r.tag('n').u32(id); frame(r, f);
    r.num(l.start).num(l.end).num(l.width).num(l.xStart).num(l.asc).num(l.desc).num(l.height).num(l.baseline).bool(!!l.last)
      .optU16s(l.breakAfter).bool(!!l.clear).optU16s(l.gapTab).bool(!!l.spanned).bool(T(l.hrFrame));
    if (T(l.hrFrame)) r.num(l.hrFrame.left).num(l.hrFrame.right);
    placed(r, this.p, l.placed);
    r.bool(T(l.marker)); if (T(l.marker)) placed(r, this.p, l.marker);
    const raw = r.finish();
    st.d.tag('N').u32(id); frame(st.d, f); st.d.bytes(raw);
    st.lines.bytes(raw);
    return l;
  }
  function reset(m) {
    if (!st) return orig.reset.call(this, m);
    const id = breaker(this);
    orig.reset.call(this, m);
    const raw = new Raw().tag('r').u32(id).num(m.pos).num(m.lineIdx).bool(!!m.trailingBreak).num(m.hyphenRun).finish();
    st.d.tag('R').u32(id).num(m.pos).num(m.lineIdx).bool(!!m.trailingBreak).num(m.hyphenRun).bytes(raw);
    st.lines.bytes(raw);
  }
  function peekWordWidth() {
    if (!st) return orig.peek.call(this);
    const id = breaker(this);
    const w = orig.peek.call(this);
    const raw = new Raw().tag('k').u32(id).num(w).finish();
    st.d.tag('K').u32(id).bytes(raw);
    st.lines.bytes(raw);
    return w;
  }

  /** the font answers paintLine reads for the glyphs of a line */
  const paintInputs = (ctx, p, all) => {
    for (const q of all) {
      const g = q.g;
      const fl = face(g.face);
      const f = g.face.font;
      if (g.kind === 'char' || g.kind === 'nbsp' || g.kind === 'shy') {
        const k = `${fl}|${g.cp}`;
        if (!st.colr.has(k)) {
          st.colr.add(k);
          st.d.tag('C').u32(fl).num(g.cp).bool(quiet(() => !!f.hasColorGlyphs?.() && !!f.colorLayers?.(f.glyphId(g.cp))));
        }
      }
      if (g.kind === 'tab' && q.leader) {
        const ch = q.leader === 'hyphen' ? '-' : q.leader === 'underscore' || q.leader === 'heavy' ? '_' : q.leader === 'middleDot' ? '·' : '.';
        const cp = ch.codePointAt(0);
        const k = `${fl}|${cp}`;
        if (!st.lead.has(k)) { st.lead.add(k); st.d.tag('W').u32(fl).num(cp).num(f.advanceWidth(f.glyphId(cp))).num(f.metrics.unitsPerEm); }
      }
      const u = g.style.underline;
      if (u && u.val !== 'none') {
        const em = Math.round((g.style.sizePt * 600) / 72);
        const k = `${fl}|${em}`;
        if (!st.vdmx.has(k)) { st.vdmx.add(k); st.d.tag('V').u32(fl).num(em).optNum(f.vdmx?.(em)?.yMin); }
      }
    }
    void ctx; void p;
  };
  const proxies = new WeakMap();
  const proxy = (f) => {
    let x = proxies.get(f);
    if (!x) { x = { key: f.key, family: f.family, bold: f.bold, italic: f.italic, synthBold: f.synthBold, synthItalic: f.synthItalic, standIn: f.standIn }; proxies.set(f, x); }
    return x;
  };
  const paintOut = (items) => {
    const r = new Raw();
    const page = { widthTw: 0, heightTw: 0, section: 0, number: 0, items: items.map((it) => (it.t === 'glyphs' ? { ...it, face: proxy(it.face) } : it)), lines: [] };
    try { canonLayout({ pages: [page], warnings: [] }, null, r); } catch (e) { return null; }
    return r.finish();
  };

  return {
    pre(name, args) { return st && name === 'paintLine' ? args[1].length : undefined; },
    report(name, args, _r, pre) {
      if (!st || name !== 'paintLine' || pre === undefined) return;
      const [ctx, out, line, frameX, top] = args;
      const raw = paintOut(out.slice(pre));
      if (!raw) { st.paint.bytes(Buffer.alloc(0)); st.d.tag('X').u16s('canon').bytes(Buffer.alloc(0)); return; }
      st.paint.bytes(raw);
      const p = st.lineOwner.get(line);
      if (!p || !st.index.has(p)) { st.d.tag('X').u16s('line of no breaker').bytes(raw); return; }
      const all = [...(line.marker ?? []), ...line.placed];
      paintInputs(ctx, p, all);
      const pl = para(p, ctx);
      const d = new Raw();
      d.tag('D').u32(pl).num(frameX).num(top).num(line.baseline).num(line.height).bool(T(line.hrFrame));
      if (T(line.hrFrame)) d.num(line.hrFrame.left).num(line.hrFrame.right);
      const ok1 = placed(d, p, line.placed);
      d.bool(T(line.marker));
      const ok2 = T(line.marker) ? placed(d, p, line.marker) : true;
      if (!ok1 || !ok2) { st.d.tag('X').u16s('glyph of another paragraph').bytes(raw); return; }
      st.d.parts.push(d.finish());
      st.d.bytes(raw);
    },
    start() {
      st = {
        d: new Raw(), faces: new Map(), styles: new Map(), paras: new Map(), index: new Map(), breakers: new Map(),
        advs: new Set(), colr: new Set(), lead: new Set(), vdmx: new Set(), lineOwner: new WeakMap(),
        lines: null, paint: null,
      };
      LineBreaker.prototype.next = next;
      LineBreaker.prototype.reset = reset;
      LineBreaker.prototype.peekWordWidth = peekWordWidth;
    },
    /** sinks for the two facets (facets.mjs owns them) */
    bind(lines, paint) { st.lines = lines; st.paint = paint; },
    finish() {
      LineBreaker.prototype.next = orig.next;
      LineBreaker.prototype.reset = orig.reset;
      LineBreaker.prototype.peekWordWidth = orig.peek;
      const dump = st.d.tag('E').finish();
      st = null;
      return dump;
    },
  };
}
