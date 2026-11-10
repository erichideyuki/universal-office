// The TS adapter of R-P2c (uo-render frontier V): a frozen-TS `LayoutResult` (tree.ts pages) becomes the render.uos page
// messages the Rust view reads, through the generated TS encoder (js/gen/render.ts `encodePage`). The adapter is the TS twin
// of src/encode/to_wire.rs: the same field names and semantics, starting from tree.ts objects, WITHOUT the canvas hints
// (`joins`, `remap`, `outlines`, `box`, `fxFillGeom`, `fxOutlineGeom`, `parts`, `gradGeom`, `gradSamples`) — the Rust view
// recomputes them (`RenderView::encode_page` for pushed messages) — and without `text`.
//
//   const a = adaptLayout(result, { imagePixelSize });          // { messages, faces, images }
//   const view = r.newView(1); pushInto(view, a);               // the view paints the frozen layout through the exports
//
// What the adapter decides (declared in the record, uo-render.md frontier V): tables fill in first-use order across the
// whole layout (faces by FontFace object identity, images by InlineImage identity — RQ3 —, text effects, page fills and
// custom geometries by object identity); `hasBytes` is `!!face.bytes`; the font file given to the view for a face is
// `face.bytes`, or the sfnt the TS font was parsed from (`font.view`) for a face that is not embeddable, so the view
// can compute the glyph hints (E-D9) the frozen painter computed from `face.font`.
import { newEncodeState, encodePage } from '../../../uo-wasm/js/gen/render.ts';

const has = (v) => v !== undefined && v !== null;
/** copies the defined fields of `src` named in `keys` */
const pick = (src, keys) => { const o = {}; for (const k of keys) if (has(src[k])) o[k] = src[k]; return o; };
const set = (o, k, v) => { if (v !== undefined && v !== null) o[k] = v; };
const ltrb = (b) => ({ l: b.l, t: b.t, r: b.r, b: b.b });
const adjList = (a) => Object.keys(a).map((k) => ({ name: k, value: a[k] }));

export function adaptLayout(result, { imagePixelSize = () => undefined, generation = 1 } = {}) {
  const state = newEncodeState(generation);
  const t = state.tables;
  const faceIx = new Map(), imageIx = new Map(), fxIx = new Map(), fillIx = new Map(), custIx = new Map();
  const faceSrc = [];
  const imageSrc = [];

  const face = (f) => {
    let i = faceIx.get(f);
    if (i !== undefined) return i;
    i = t.faces.length;
    faceIx.set(f, i);
    const fv = f.font?.view;
    faceSrc.push(f.bytes ?? (fv ? new Uint8Array(fv.buffer, fv.byteOffset, fv.byteLength) : null));
    t.faces.push({
      key: f.key ?? '', family: f.family ?? '', bold: !!f.bold, italic: !!f.italic, synthBold: !!f.synthBold, synthItalic: !!f.synthItalic,
      standIn: !!f.standIn, hasBytes: !!f.bytes, upem: f.font?.metrics?.unitsPerEm || 1000,
    });
    return i;
  };
  const image = (img) => {
    let i = imageIx.get(img);
    if (i !== undefined) return i;
    i = t.images.length;
    imageIx.set(img, i);
    const natural = imagePixelSize(img.data);
    imageSrc.push({ bytes: img.data, natural });
    const e = { ext: img.ext ?? '', byteLength: img.data.length, widthTwips: img.widthTwips, heightTwips: img.heightTwips };
    if (img.svg) e.hasSvg = true;
    if (natural) e.natural = { w: natural.w, h: natural.h };
    if (img.effects) e.effects = pick(img.effects, ['gray', 'gain', 'bright', 'floor', 'biLevel']);
    t.images.push(e);
    return i;
  };
  const fill = (f) => {
    if (f.kind === 'none') return { kind: 'none' };
    if (f.kind === 'solid') return { kind: 'solid', hex: f.hex, alpha: f.alpha };
    const o = { kind: 'grad', stops: f.stops.map((s) => ({ pos: s.pos, hex: s.hex, alpha: s.alpha })), angleDeg: f.angleDeg };
    if (f.scaled) o.scaled = true;
    set(o, 'path', f.path);
    if (f.fillTo) o.fillTo = ltrb(f.fillTo);
    return o;
  };
  const textFx = (fx) => {
    let i = fxIx.get(fx);
    if (i !== undefined) return i;
    i = t.textFx.length;
    fxIx.set(fx, i);
    const e = {};
    if (fx.fill) e.fill = fill(fx.fill);
    if (fx.outline) {
      const o = fx.outline;
      e.outline = { widthPt: o.widthPt, fill: fill(o.fill), cap: o.cap, join: o.join };
      set(e.outline, 'dash', o.dash);
    }
    if (fx.shadow) e.shadow = pick(fx.shadow, ['blurPt', 'distPt', 'dirDeg', 'sx', 'sy', 'kxDeg', 'kyDeg', 'algn', 'hex', 'alpha']);
    if (fx.glow) e.glow = pick(fx.glow, ['radPt', 'hex', 'alpha']);
    if (fx.reflection) e.reflection = pick(fx.reflection, ['blurPt', 'stA', 'stPos', 'endA', 'endPos', 'distPt', 'dirDeg', 'algn']);
    t.textFx.push(e);
    return i;
  };
  const pageFill = (f) => {
    let i = fillIx.get(f);
    if (i !== undefined) return i;
    i = t.pageFills.length;
    fillIx.set(f, i);
    const e = { type: f.type, c1: f.c1, c2: f.c2, angle: f.angle, focus: f.focus };
    if (f.stops) e.stops = f.stops.map((s) => ({ pos: s.pos, color: s.color }));
    if (f.focusPosition) e.focusPosition = { 0: f.focusPosition[0], 1: f.focusPosition[1] };
    if (f.gradientCenter) e.gradientCenter = true;
    if (f.image) e.image = image(f.image);
    if (f.imagePx) e.imagePx = { w: f.imagePx.w, h: f.imagePx.h, dpi: f.imagePx.dpi };
    if (f.pattern) e.pattern = f.pattern.slice();
    t.pageFills.push(e);
    return i;
  };
  const cust = (c) => {
    let i = custIx.get(c);
    if (i !== undefined) return i;
    i = t.cust.length;
    custIx.set(c, i);
    t.cust.push({
      paths: c.map((p) => {
        const o = { w: p.w, h: p.h, cmds: p.cmds.map((k) => {
          if (k.op === 'M' || k.op === 'L') return { op: k.op, x: k.x, y: k.y };
          if (k.op === 'C' || k.op === 'Q') return { op: k.op, pts: k.pts.map(([x, y]) => ({ x, y })) };
          if (k.op === 'A') return { op: 'A', wR: k.wR, hR: k.hR, st: k.st, sw: k.sw };
          return { op: 'Z' };
        }) };
        if (p.fill) o.fill = true;
        if (p.stroke) o.stroke = true;
        return o;
      }),
    });
    return i;
  };

  const shadow = (h) => {
    const o = pick(h, ['kind', 'color', 'alpha', 'blurPt', 'distPt', 'dirDeg', 'sx', 'sy', 'kxDeg', 'kyDeg', 'algn']);
    if (h.rotWithShape) o.rotWithShape = true;
    return o;
  };
  const glow = (g) => ({ color: g.color, alpha: g.alpha, radPt: g.radPt });
  const reflection = (r) => {
    const o = pick(r, ['blurPt', 'stA', 'stPos', 'endA', 'endPos', 'distPt', 'dirDeg', 'sy', 'algn']);
    if (r.rotWithShape) o.rotWithShape = true;
    return o;
  };
  const look = (p) => {
    const o = {};
    if (p.geom) { o.geom = { prst: p.geom.prst }; if (p.geom.adj) o.geom.adj = adjList(p.geom.adj); }
    if (p.line) o.line = pick(p.line, ['color', 'alpha', 'widthPt', 'dash', 'cmpd', 'cap', 'join']);
    if (p.shadow) o.shadow = shadow(p.shadow);
    if (p.glow) o.glow = glow(p.glow);
    set(o, 'softEdgePt', p.softEdgePt);
    if (p.bevel) o.bevel = pick(p.bevel, ['wPt', 'hPt', 'prst', 'rig', 'dir', 'material']);
    if (p.reflection) o.reflection = reflection(p.reflection);
    set(o, 'alpha', p.alpha);
    if (p.duotone) o.duotone = { 0: p.duotone[0], 1: p.duotone[1] };
    if (p.clrChange) o.clrChange = { from: p.clrChange.from, to: p.clrChange.to, toAlpha: p.clrChange.toAlpha };
    return o;
  };
  const stroke = (s) => pick(s, ['color', 'widthPt', 'dash']);
  const shape = (it) => {
    const st = it.style ?? {};
    const style = { geom: st.geom ?? '' };
    if (st.adj) style.adj = adjList(st.adj);
    if (st.cust) style.cust = cust(st.cust);
    if (st.flipH) style.flipH = true;
    if (st.flipV) style.flipV = true;
    set(style, 'rot', st.rot);
    set(style, 'fillAlpha', st.fillAlpha);
    set(style, 'lineAlpha', st.line?.alpha);
    const o = { x: it.x, y: it.y, w: it.w, h: it.h, style, parts: [] };
    set(o, 'fill', it.fill);
    if (it.stroke) o.stroke = stroke(it.stroke);
    if (it.line) o.line = { x1: it.line.x1, y1: it.line.y1, x2: it.line.x2, y2: it.line.y2 };
    if (it.grad) {
      const g = it.grad;
      o.grad = { stops: g.stops.map((s) => ({ pos: s.pos, color: s.color })), angleDeg: g.angleDeg };
      if (g.scaled) o.grad.scaled = true;
      // gradientGeometry: `g.rotWithShape !== false` (absent = rotates with the shape)
      if (g.rotWithShape !== false) o.grad.rotWithShape = true;
      set(o.grad, 'path', g.path);
      if (g.fillTo) o.grad.fillTo = ltrb(g.fillTo);
    }
    if (it.shadow) o.shadow = pick(it.shadow, ['dx', 'dy', 'color', 'alpha', 'blur']);
    if (it.pattern) o.pattern = { prst: it.pattern.prst, fg: it.pattern.fg, bg: it.pattern.bg };
    if (it.blip) {
      o.blip = { image: image(it.blip.image) };
      if (it.blip.tile) o.blip.tile = pick(it.blip.tile, ['tx', 'ty', 'sx', 'sy', 'algn', 'flip']);
      set(o.blip, 'alpha', it.blip.alpha);
    }
    if (it.fx) {
      o.fx = {};
      if (it.fx.shadow) o.fx.shadow = shadow(it.fx.shadow);
      if (it.fx.glow) o.fx.glow = glow(it.fx.glow);
      set(o.fx, 'softEdgePt', it.fx.softEdgePt);
      if (it.fx.reflection) o.fx.reflection = reflection(it.fx.reflection);
    }
    return o;
  };
  const cmd = (c) => (c.op === 'M' || c.op === 'L' ? { op: c.op, x: c.x, y: c.y }
    : c.op === 'C' ? { op: 'C', x1: c.x1, y1: c.y1, x2: c.x2, y2: c.y2, x: c.x, y: c.y } : { op: 'Z' });

  const item = (it) => {
    switch (it.t) {
      case 'glyphs': {
        const o = { t: 'glyphs', face: face(it.face), sizePt: it.sizePt, color: it.color, y: it.y,
          glyphs: it.glyphs.map((g) => (has(g.gid) ? { cp: g.cp, x: g.x, gid: g.gid } : { cp: g.cp, x: g.x })) };
        set(o, 'scaleX', it.scaleX);
        if (it.synthBold) o.synthBold = true;
        if (it.synthItalic) o.synthItalic = true;
        set(o, 'alpha', it.alpha);
        if (it.asPath) o.asPath = true;
        if (it.colr) o.colr = true;
        if (it.fx) o.fx = textFx(it.fx);
        return o;
      }
      case 'rect': { const o = { t: 'rect', x: it.x, y: it.y, w: it.w, h: it.h, fill: it.fill }; set(o, 'alpha', it.alpha); return o; }
      case 'line': { const o = { t: 'line', x1: it.x1, y1: it.y1, x2: it.x2, y2: it.y2, color: it.color, widthPt: it.widthPt }; set(o, 'dash', it.dash); return o; }
      case 'image': {
        const o = { t: 'image', x: it.x, y: it.y, w: it.w, h: it.h, image: image(it.image) };
        if (it.crop) o.crop = ltrb(it.crop);
        set(o, 'rot', it.rot);
        if (it.flipH) o.flipH = true;
        if (it.flipV) o.flipV = true;
        if (it.pic) o.pic = look(it.pic);
        return o;
      }
      case 'shape': return { t: 'shape', ...shape(it) };
      case 'path': {
        const o = { t: 'path', cmds: it.cmds.map(cmd) };
        set(o, 'fill', it.fill);
        if (it.stroke) o.stroke = stroke(it.stroke);
        if (it.evenOdd) o.evenOdd = true;
        return o;
      }
      case 'clip': {
        const o = { t: 'clip', op: it.op };
        if (it.op === 'push') for (const k of ['x', 'y', 'w', 'h']) set(o, k, it[k]);
        return o;
      }
      case 'xform': { const o = { t: 'xform', op: it.op }; if (it.m) o.m = it.m.slice(); return o; }
      default: return null;
    }
  };

  const messages = [];
  for (const pg of result.pages) {
    const items = [];
    for (const it of pg.items) { const w = item(it); if (w) items.push(w); }
    const msg = { widthTw: pg.widthTw, heightTw: pg.heightTw, items, app: new Uint8Array(0) };
    set(msg, 'background', pg.background);
    if (pg.backgroundFill) msg.backgroundFill = pageFill(pg.backgroundFill);
    if (pg.backgroundShow) msg.backgroundShow = true;
    messages.push({ msg, bytes: encodePage(msg, state) });
  }
  return {
    messages: messages.map((m) => m.bytes),
    decodedInputs: messages.map((m) => m.msg),
    faces: faceSrc.map((bytes, id) => ({ id, bytes })).filter((f) => f.bytes),
    images: imageSrc.map((im, id) => ({ id, bytes: im.bytes, natural: im.natural })),
  };
}

/** Feeds a Rust view (render.mjs `RenderView`) with the adapted pages, the font files and the image bytes. */
export function pushInto(view, adapted) {
  for (const m of adapted.messages) view.pushMessage(m);
  for (const f of adapted.faces) view.setFaceBytes(f.id, f.bytes);
  for (const im of adapted.images) view.setImageBytes(im.id, im.bytes, im.natural?.w ?? 0, im.natural?.h ?? 0);
}
