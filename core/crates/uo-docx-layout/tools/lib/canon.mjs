// The P1 canonical walker of uo-docx-layout (record uo-docx-layout.md §5 and "Milestone 1 —
// F0"): the frozen TS `LayoutResult` as one canonical stream, exact bits. It extends
// tools/gen-parity-layout.mjs `normalizeLayout` (same field coverage, plus everything it left
// out: lines' chars/hyph/rev/frame/pfmt, objects, cells, hfItems, text effects, line boxes) with
// f64 bits instead of the 0.01 rounding. src/canon.rs writes the same stream from a Rust
// `Layout`; any change here changes it there (CANON_VERSION on both sides).
//
// Stream (Sink encodings: num = f64 bits with NaN canonical; optNum/optU16s = presence bool then
// the value; u16s = UTF-16 code units; every optional boolean of the TS is written as its
// truthiness, RA1.2):
//
//   layout  := 'L' u32(CANON_VERSION) u32(n) page* u32(n) u16s(warning)*
//   page    := 'P' num(widthTw) num(heightTw) num(section) num(number) optNum(hfItems)
//              optU16s(background) bool(backgroundShow) bool(fill?)[pagefill]
//              u32(n) item* u32(n) line* bool(objects?)[u32(n) object*] bool(cells?)[u32(n) cell*]
//   item    := 'g' face num(sizePt) u16s(color) num(y) u32(n)(num(cp) num(x) optNum(gid))*
//                  optNum(scaleX) u16s(text) bool(synthBold) bool(synthItalic) optNum(alpha)
//                  bool(asPath) bool(colr) bool(fx?)[textfx]
//                  bool(lineBox?)[num(asc) num(desc) num(lead) u32(label of lineBox.line)] bool(lineNum)
//            | 'r' num(x y w h) u16s(fill) optNum(alpha)
//            | 'l' num(x1 y1 x2 y2) u16s(color) num(widthPt) optU16s(dash)
//            | 'i' num(x y w h) image bool(crop?)[num(l t r b)] optNum(rot) bool(flipH) bool(flipV) bool(pic?)[pic]
//            | 's' num(x y w h) style optU16s(fill) bool(stroke?)[stroke] bool(line?)[num(x1 y1 x2 y2)]
//                  bool(grad?)[grad] bool(shadow?)[num(dx dy) u16s(color) optNum(alpha) optNum(blur)]
//                  bool(pattern?)[u16s(prst fg bg)] bool(blip?)[image bool(tile?)[num(tx ty sx sy) u16s(algn flip)] optNum(alpha)]
//                  bool(fx?)[shapefx]
//            | 'p' u32(n) cmd* optU16s(fill) bool(stroke?)[stroke] bool(evenOdd)
//            | 'c' bool(push) [push: optNum(x y w h)] bool(src?)[image]
//            | 'x' bool(push) bool(m?)[num*6]
//            | '?' u16s(t)
//   cmd     := 'M'|'L' num(x y) | 'C' num(x1 y1 x2 y2 x y) | 'Z'
//   stroke  := u16s(color) num(widthPt) optU16s(dash)
//   style   := u16s(geom) bool(adj?)[adj] bool(cust?)[u32(n) custpath*] bool(flipH) bool(flipV)
//              optNum(rot) optNum(fillAlpha) optNum(line?.alpha)                     (the RQ1 spec)
//   adj     := u32(n) (u16s(key) num(value))*                        (Object.keys order)
//   custpath:= num(w h) bool(fill) bool(stroke) u32(n) custcmd*
//   custcmd := 'M'|'L' num(x y) | 'C'|'Q' u32(n) num(px py)* | 'A' num(wR hR st sw) | 'Z'
//   grad    := u32(n)(num(pos) u16s(color))* num(angleDeg) bool(scaled) bool(rotWithShape)
//              optU16s(path) bool(fillTo?)[num(l t r b)]
//   face    := u32(label) [first time: u16s(key) u16s(family) bool(bold italic synthBold synthItalic standIn)
//              bool(bytes?)[digestOf(bytes)]]               (label = first appearance of the object)
//   image   := u32(label) [first time: u16s(ext) digestOf(data) bool(svg?)[digestOf(svg)]
//              num(widthTwips heightTwips) bool(effects?)[bool(gray) optNum(gain) optNum(bright) bool(floor) optNum(biLevel)]]
//   textfx  := bool(fill?)[fill] bool(outline?)[num(widthPt) fill optU16s(dash) u16s(cap) u16s(join)]
//              bool(shadow?)[num(blurPt distPt dirDeg sx sy kxDeg kyDeg) u16s(algn hex) num(alpha)]
//              bool(glow?)[num(radPt) u16s(hex) num(alpha)]
//              bool(reflection?)[num(blurPt stA stPos endA endPos distPt dirDeg) u16s(algn)]
//   fill    := 'n' | 's' u16s(hex) num(alpha)
//            | 'g' u32(n)(num(pos) u16s(hex) num(alpha))* num(angleDeg) bool(scaled) optU16s(path) bool(fillTo?)[num(l t r b)]
//   pic     := bool(geom?)[u16s(prst) bool(adj?)[adj]] bool(line?)[u16s(color) num(alpha widthPt) optU16s(dash cmpd cap join)]
//              bool(shadow?)[shadow] bool(glow?)[glow] optNum(softEdgePt) bool(bevel?)[num(wPt hPt) optU16s(prst rig dir material)]
//              bool(reflection?)[reflection] optNum(alpha) bool(duotone?)[u16s u16s] bool(clrChange?)[u16s(from to) num(toAlpha)]
//   shadow  := u16s(kind color) num(alpha blurPt distPt dirDeg) optNum(sx sy kxDeg kyDeg) optU16s(algn) bool(rotWithShape)
//   glow    := u16s(color) num(alpha radPt)
//   reflection := num(blurPt stA stPos endA endPos distPt dirDeg sy) optU16s(algn) bool(rotWithShape)
//   shapefx := bool(shadow?)[shadow] bool(glow?)[glow] optNum(softEdgePt) bool(reflection?)[reflection]
//   pagefill:= u16s(type c1 c2) bool(stops?)[u32(n)(num(pos) u16s(color))*] num(angle focus)
//              bool(focusPosition?)[num num] bool(gradientCenter) bool(image?)[image]
//              bool(imagePx?)[num(w h dpi)] bool(pattern?)[u32(n) num*]
//   line    := 'n' u16s(ref) num(start end x y height baseline width) bool(chars?)[u32(n)(num(o x w))*]
//              optNum(hyph) bool(rev?)[num(before after)] bool(frame?)[num(l r)]
//              bool(pfmt?)[num(left right firstLine) u32(n)(u16s(val) num(pos) optU16s(leader))*]
//   object  := 'o' optU16s(run ref, absent for a synthetic run) num(x y w h) u16s(layer) bool(hf)
//              bool(frame?)[num(colX colW paraTop pageW pageH marL marR marT marB bodyTop)]
//              bool(content?)[u32(n) line* num*6]
//   cell    := 'k' u16s(ref) num(x y w h span)
//
// Run refs (PlacedObject.run): the model position of the run object, `runRefs(full)` below, the
// same text uo-docx-layout prints from a `RunRef` (src/canon.rs): `<story>/<block path>#<run>`
// with story `b` (body: `b3`, `b3/r0/c1/b0` with the cell's index in its row, not the grid column),
// `hf<n>` (header/footer part by first appearance over sections x [hdr default first even, ftr
// default first even]), `fn<id>`/`en<id>`, textbox content `<run ref of the textbox>/tx/b<k>`
// and group members `<run ref of the group>/g<j>`.
export const CANON_VERSION = 1;

import { createHash } from 'node:crypto';

const T = (v) => (v !== undefined && v !== null);

export function runRefs(full) {
  const out = new Map();
  const blocksOf = (c) => c.blocks ?? c.paragraphs?.map((paragraph) => ({ type: 'paragraph', paragraph })) ?? [];
  const runs = (rs, base) => {
    rs.forEach((r, i) => {
      const ref = `${base}#${i}`;
      if (!out.has(r)) out.set(r, ref);
      if (r.textbox) walk(r.textbox.blocks ?? r.textbox.paras.map((paragraph) => ({ type: 'paragraph', paragraph })), `${ref}/tx`);
      (r.group?.items ?? []).forEach((it, j) => runs([it.run], `${ref}/g${j}`));
    });
  };
  const walk = (bs, base) => {
    bs.forEach((b, i) => {
      const p = `${base}/b${i}`;
      if (b.type === 'paragraph') runs(b.paragraph.runs, p);
      else b.rows.forEach((row, ri) => row.cells.forEach((c, ci) => walk(blocksOf(c), `${p}/r${ri}/c${ci}`)));
    });
  };
  walk(full.blocks, '');
  const hf = new Map();
  for (const s of full.sections ?? []) {
    for (const [set, paras] of [[s.headerBlocks, s.headers], [s.footerBlocks, s.footers]]) {
      for (const k of ['default', 'first', 'even']) {
        const arr = set?.[k] ?? paras?.[k];
        if (!arr || hf.has(arr)) continue;
        hf.set(arr, `hf${hf.size}`);
        const bs = set?.[k] ?? arr.map((paragraph) => ({ type: 'paragraph', paragraph }));
        walk(bs, hf.get(arr));
      }
    }
  }
  for (const [kind, blocks, paras] of [['fn', full.footnoteBlocks, full.footnotes], ['en', full.endnoteBlocks, full.endnotes]]) {
    for (const [id, ps] of paras ?? []) walk(blocks?.get(id) ?? ps.map((paragraph) => ({ type: 'paragraph', paragraph })), `${kind}${id}`);
  }
  // the body walk prefixes '' — drop the leading '/'
  for (const [r, ref] of out) if (ref.startsWith('/')) out.set(r, ref.slice(1));
  return out;
}

/** `media` (optional): a Map filled with sha256 hex -> bytes of every image payload written (the
 *  dump keeps them so the Rust adapter check can rebuild the images). */
export function canonLayout(res, full, sink, media = null) {
  const s = sink;
  const faces = new Map(), images = new Map(), lineLabels = new Map();
  const refs = full ? runRefs(full) : new Map();
  const face = (f) => {
    let l = faces.get(f);
    const first = l === undefined;
    if (first) { l = faces.size; faces.set(f, l); }
    s.u32(l);
    if (!first) return;
    s.u16s(f?.key ?? '').u16s(f?.family ?? '').bool(!!f?.bold).bool(!!f?.italic).bool(!!f?.synthBold).bool(!!f?.synthItalic).bool(!!f?.standIn);
    s.bool(T(f?.bytes)); if (T(f?.bytes)) s.digestOf(f.bytes);
  };
  const image = (img) => {
    let l = images.get(img);
    const first = l === undefined;
    if (first) { l = images.size; images.set(img, l); }
    s.u32(l);
    if (!first) return;
    const keep = (u8) => { if (media && u8) media.set(createHash('sha256').update(u8).digest('hex'), u8); };
    keep(img?.data); if (T(img?.svg)) keep(img.svg);
    s.u16s(img?.ext ?? '').digestOf(img?.data ?? new Uint8Array(0));
    s.bool(T(img?.svg)); if (T(img?.svg)) s.digestOf(img.svg);
    s.num(img?.widthTwips).num(img?.heightTwips);
    const e = img?.effects;
    s.bool(T(e)); if (T(e)) s.bool(!!e.gray).optNum(e.gain).optNum(e.bright).bool(!!e.floor).optNum(e.biLevel);
  };
  const adj = (a) => { const ks = Object.keys(a); s.u32(ks.length); for (const k of ks) s.u16s(k).num(a[k]); };
  const stroke = (st) => { s.u16s(st.color).num(st.widthPt).optU16s(st.dash); };
  const ltrb = (b) => { s.num(b.l).num(b.t).num(b.r).num(b.b); };
  const fill = (f) => {
    if (f.kind === 'none') { s.tag('n'); return; }
    if (f.kind === 'solid') { s.tag('s').u16s(f.hex).num(f.alpha); return; }
    s.tag('g').u32(f.stops.length);
    for (const st of f.stops) s.num(st.pos).u16s(st.hex).num(st.alpha);
    s.num(f.angleDeg).bool(!!f.scaled).optU16s(f.path).bool(T(f.fillTo)); if (T(f.fillTo)) ltrb(f.fillTo);
  };
  const textfx = (fx) => {
    s.bool(T(fx.fill)); if (T(fx.fill)) fill(fx.fill);
    const o = fx.outline; s.bool(T(o)); if (T(o)) { s.num(o.widthPt); fill(o.fill); s.optU16s(o.dash).u16s(o.cap).u16s(o.join); }
    const h = fx.shadow; s.bool(T(h)); if (T(h)) s.num(h.blurPt).num(h.distPt).num(h.dirDeg).num(h.sx).num(h.sy).num(h.kxDeg).num(h.kyDeg).u16s(h.algn).u16s(h.hex).num(h.alpha);
    const g = fx.glow; s.bool(T(g)); if (T(g)) s.num(g.radPt).u16s(g.hex).num(g.alpha);
    const r = fx.reflection; s.bool(T(r)); if (T(r)) s.num(r.blurPt).num(r.stA).num(r.stPos).num(r.endA).num(r.endPos).num(r.distPt).num(r.dirDeg).u16s(r.algn);
  };
  const shadow = (h) => {
    s.u16s(h.kind).u16s(h.color).num(h.alpha).num(h.blurPt).num(h.distPt).num(h.dirDeg)
      .optNum(h.sx).optNum(h.sy).optNum(h.kxDeg).optNum(h.kyDeg).optU16s(h.algn).bool(!!h.rotWithShape);
  };
  const glow = (g) => { s.u16s(g.color).num(g.alpha).num(g.radPt); };
  const reflection = (r) => {
    s.num(r.blurPt).num(r.stA).num(r.stPos).num(r.endA).num(r.endPos).num(r.distPt).num(r.dirDeg).num(r.sy).optU16s(r.algn).bool(!!r.rotWithShape);
  };
  const opt = (v, f) => { s.bool(T(v)); if (T(v)) f(v); };
  const pic = (p) => {
    opt(p.geom, (g) => { s.u16s(g.prst); opt(g.adj, adj); });
    opt(p.line, (l) => { s.u16s(l.color).num(l.alpha).num(l.widthPt).optU16s(l.dash).optU16s(l.cmpd).optU16s(l.cap).optU16s(l.join); });
    opt(p.shadow, shadow); opt(p.glow, glow); s.optNum(p.softEdgePt);
    opt(p.bevel, (b) => { s.num(b.wPt).num(b.hPt).optU16s(b.prst).optU16s(b.rig).optU16s(b.dir).optU16s(b.material); });
    opt(p.reflection, reflection); s.optNum(p.alpha);
    opt(p.duotone, (d) => { s.u16s(d[0]).u16s(d[1]); });
    opt(p.clrChange, (c) => { s.u16s(c.from).u16s(c.to).num(c.toAlpha); });
  };
  const shapefx = (f) => { opt(f.shadow, shadow); opt(f.glow, glow); s.optNum(f.softEdgePt); opt(f.reflection, reflection); };
  const custcmd = (k) => {
    if (k.op === 'M' || k.op === 'L') { s.tag(k.op).num(k.x).num(k.y); return; }
    if (k.op === 'C' || k.op === 'Q') { s.tag(k.op).u32(k.pts.length); for (const [px, py] of k.pts) s.num(px).num(py); return; }
    if (k.op === 'A') { s.tag('A').num(k.wR).num(k.hR).num(k.st).num(k.sw); return; }
    s.tag('Z');
  };
  const style = (st) => {
    s.u16s(st.geom ?? ''); opt(st.adj, adj);
    opt(st.cust, (cs) => { s.u32(cs.length); for (const p of cs) { s.num(p.w).num(p.h).bool(!!p.fill).bool(!!p.stroke).u32(p.cmds.length); p.cmds.forEach(custcmd); } });
    s.bool(!!st.flipH).bool(!!st.flipV).optNum(st.rot).optNum(st.fillAlpha).optNum(st.line?.alpha);
  };
  const grad = (g) => {
    s.u32(g.stops.length); for (const st of g.stops) s.num(st.pos).u16s(st.color);
    s.num(g.angleDeg).bool(!!g.scaled).bool(!!g.rotWithShape).optU16s(g.path); opt(g.fillTo, ltrb);
  };
  const cmd = (c) => {
    if (c.op === 'M' || c.op === 'L') s.tag(c.op).num(c.x).num(c.y);
    else if (c.op === 'C') s.tag('C').num(c.x1).num(c.y1).num(c.x2).num(c.y2).num(c.x).num(c.y);
    else s.tag('Z');
  };
  const item = (it) => {
    switch (it.t) {
      case 'glyphs': {
        s.tag('g'); face(it.face); s.num(it.sizePt).u16s(it.color).num(it.y).u32(it.glyphs.length);
        for (const g of it.glyphs) s.num(g.cp).num(g.x).optNum(g.gid);
        s.optNum(it.scaleX).u16s(it.text).bool(!!it.synthBold).bool(!!it.synthItalic).optNum(it.alpha).bool(!!it.asPath).bool(!!it.colr);
        opt(it.fx, textfx);
        opt(it.lineBox, (b) => {
          let l = lineLabels.get(b.line);
          if (l === undefined) { l = lineLabels.size; lineLabels.set(b.line, l); }
          s.num(b.asc).num(b.desc).num(b.lead).u32(l);
        });
        s.bool(!!it.lineNum);
        return;
      }
      case 'rect': s.tag('r').num(it.x).num(it.y).num(it.w).num(it.h).u16s(it.fill).optNum(it.alpha); return;
      case 'line': s.tag('l').num(it.x1).num(it.y1).num(it.x2).num(it.y2).u16s(it.color).num(it.widthPt).optU16s(it.dash); return;
      case 'image':
        s.tag('i').num(it.x).num(it.y).num(it.w).num(it.h); image(it.image);
        opt(it.crop, ltrb); s.optNum(it.rot).bool(!!it.flipH).bool(!!it.flipV); opt(it.pic, pic);
        return;
      case 'shape':
        s.tag('s').num(it.x).num(it.y).num(it.w).num(it.h); style(it.style ?? {});
        s.optU16s(it.fill); opt(it.stroke, stroke);
        opt(it.line, (l) => { s.num(l.x1).num(l.y1).num(l.x2).num(l.y2); });
        opt(it.grad, grad);
        opt(it.shadow, (h) => { s.num(h.dx).num(h.dy).u16s(h.color).optNum(h.alpha).optNum(h.blur); });
        opt(it.pattern, (p) => { s.u16s(p.prst).u16s(p.fg).u16s(p.bg); });
        opt(it.blip, (b) => { image(b.image); opt(b.tile, (t) => { s.num(t.tx).num(t.ty).num(t.sx).num(t.sy).u16s(t.algn).u16s(t.flip); }); s.optNum(b.alpha); });
        opt(it.fx, shapefx);
        return;
      case 'path':
        s.tag('p').u32(it.cmds.length); it.cmds.forEach(cmd); s.optU16s(it.fill); opt(it.stroke, stroke); s.bool(!!it.evenOdd);
        return;
      case 'clip':
        s.tag('c').bool(it.op === 'push');
        if (it.op === 'push') s.optNum(it.x).optNum(it.y).optNum(it.w).optNum(it.h);
        opt(it.src, image);
        return;
      case 'xform':
        s.tag('x').bool(it.op === 'push'); opt(it.m, (m) => { for (let k = 0; k < 6; k++) s.num(m[k]); });
        return;
      default: s.tag('?').u16s(String(it.t));
    }
  };
  const line = (ln) => {
    s.tag('n').u16s(ln.ref).num(ln.start).num(ln.end).num(ln.x).num(ln.y).num(ln.height).num(ln.baseline).num(ln.width);
    opt(ln.chars, (cs) => { s.u32(cs.length); for (const c of cs) s.num(c.o).num(c.x).num(c.w); });
    s.optNum(ln.hyph);
    opt(ln.rev, (r) => { s.num(r.before).num(r.after); });
    opt(ln.frame, (f) => { s.num(f.l).num(f.r); });
    opt(ln.pfmt, (p) => { s.num(p.left).num(p.right).num(p.firstLine).u32(p.tabs.length); for (const t of p.tabs) s.u16s(t.val).num(t.pos).optU16s(t.leader); });
  };
  const pagefill = (f) => {
    s.u16s(f.type).u16s(f.c1).u16s(f.c2);
    opt(f.stops, (st) => { s.u32(st.length); for (const x of st) s.num(x.pos).u16s(x.color); });
    s.num(f.angle).num(f.focus);
    opt(f.focusPosition, (p) => { s.num(p[0]).num(p[1]); });
    s.bool(!!f.gradientCenter); opt(f.image, image);
    opt(f.imagePx, (p) => { s.num(p.w).num(p.h).num(p.dpi); });
    opt(f.pattern, (rows) => { s.u32(rows.length); for (const r of rows) s.num(r); });
  };
  const object = (o) => {
    s.tag('o').optU16s(refs.get(o.run)).num(o.x).num(o.y).num(o.w).num(o.h).u16s(o.layer).bool(!!o.hf);
    opt(o.frame, (f) => { for (const k of ['colX', 'colW', 'paraTop', 'pageW', 'pageH', 'marL', 'marR', 'marT', 'marB', 'bodyTop']) s.num(f[k]); });
    opt(o.content, (c) => { s.u32(c.lines.length); c.lines.forEach(line); for (let k = 0; k < 6; k++) s.num(c.xf[k]); });
  };
  const cell = (c) => { s.tag('k').u16s(c.ref).num(c.x).num(c.y).num(c.w).num(c.h).num(c.span); };

  s.tag('L').u32(CANON_VERSION).u32(res.pages.length);
  for (const p of res.pages) {
    s.tag('P').num(p.widthTw).num(p.heightTw).num(p.section).num(p.number).optNum(p.hfItems);
    s.optU16s(p.background).bool(!!p.backgroundShow); opt(p.backgroundFill, pagefill);
    s.u32(p.items.length); p.items.forEach(item);
    s.u32(p.lines.length); p.lines.forEach(line);
    opt(p.objects, (os) => { s.u32(os.length); os.forEach(object); });
    opt(p.cells, (cs) => { s.u32(cs.length); cs.forEach(cell); });
  }
  s.u32(res.warnings.length); for (const w of res.warnings) s.u16s(w);
  return { faces: faces.size, images: images.size, lines: lineLabels.size };
}
