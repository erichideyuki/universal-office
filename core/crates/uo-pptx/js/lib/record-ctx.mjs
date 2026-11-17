// R-S4: the recording CanvasRenderingContext2D of the Slides port (plan section 4.3, "draw-op stream as the primary
// gate"). A fake 2D context that answers like a canvas where the painters need an answer (transform state, gradients,
// patterns, getImageData, measureText) and records every call and property set into a canonical op stream.
// It runs over (a) the frozen TS `paintSlideTo` / `paintShapesTo` and (b) `uo-wasm/js/paint-slides.ts`, so the two
// streams can be compared line by line. No DOM and no real canvas are needed.
//
// The stream (one text line per call or property set; the format is documented in
// core/docs/modules/uo-render.md "R-S3/R-S4"):
//
//   set <name> <value>                 property assignment; numbers by `fmt`, strings JSON-quoted, gradients `@gN`,
//                                      patterns `@pN`
//   save | restore | ctm a b c d e f   the current transform is re-stated after every transform change and restore
//   beginPath | closePath | moveTo X Y | lineTo X Y | bezierCurveTo ... | quadraticCurveTo ... | rect P0 P1 P2 P3 |
//   arc cx cy ux uy vx vy a0 a1 ccw | ellipse ... | arcTo X1 Y1 X2 Y2 r a b c d
//                                      path geometry in DEVICE space (the current transform applied at the call);
//                                      arc/ellipse as the device image of the unit circle (centre, the two axis
//                                      vectors), which is exact for any affine transform
//   fill RULE | stroke | clip RULE | fillRect P0..P3 | strokeRect | clearRect | fillText TEXT X Y m=a,b,c,d | strokeText |
//   drawImage SRC [s=x,y,w,h] P0..P3   each followed by `| fs=.. ss=.. alpha=.. comp=.. filter=.. shadow=.. clip=N ...`
//                                      the resolved state the call was made under (gradient and pattern geometry in
//                                      device space; `lw=`/`dash=` scaled by the transform)
//   gradient @gN kind args | stop @gN offset "color" | pattern @pN SRC repeat | patternTransform @pN a b c d e f
//   getImageData x y w h | putImageData x y w h SHA | measure FONT TEXT   (the measure width is NOT in the stream: it goes to
//                                      the measure log; the stream only depends on the stub through control flow)
//   @cvK <line>                        a line issued on the K-th offscreen canvas (layers, scratch canvases)
//
// -0 prints as 0. Image sources are hashed: a decoded image by its content, an offscreen canvas by the running
// hash of the lines recorded on it (a recipe hash) and its size.
import { createHash } from 'node:crypto';

/** Canonical number: JS shortest round-trip, -0 as 0. */
export const fmt = (x) => {
  x = +x;
  return Object.is(x, -0) ? '0' : String(x);
};
const q = (s) => JSON.stringify(String(s));
const sha = (data) => createHash('sha1').update(data).digest('hex').slice(0, 12);

/** A DOMMatrix subset: what the painters call (`a`..`f`, `inverse`, `multiply`). */
export class RecDOMMatrix {
  constructor(init) {
    const v = Array.isArray(init) ? init : [1, 0, 0, 1, 0, 0];
    [this.a, this.b, this.c, this.d, this.e, this.f] = v;
  }
  get is2D() { return true; }
  multiply(o) {
    return new RecDOMMatrix([
      this.a * o.a + this.c * o.b, this.b * o.a + this.d * o.b,
      this.a * o.c + this.c * o.d, this.b * o.c + this.d * o.d,
      this.a * o.e + this.c * o.f + this.e, this.b * o.e + this.d * o.f + this.f,
    ]);
  }
  inverse() {
    const det = this.a * this.d - this.b * this.c;
    if (!det || !Number.isFinite(det)) return new RecDOMMatrix([NaN, NaN, NaN, NaN, NaN, NaN]);
    return new RecDOMMatrix([
      this.d / det, -this.b / det, -this.c / det, this.a / det,
      (this.c * this.f - this.d * this.e) / det, (this.b * this.e - this.a * this.f) / det,
    ]);
  }
}

/** The measurement stub: a deterministic advance table (em fractions), bold 6 % wider, a few kerning pairs. */
/** Kerning pairs of the stub (em fraction), applied unless `fontKerning` is `none`. */
const STUB_KERN = new Map(['AV', 'AW', 'AY', 'Av', 'Aw', 'Ay', 'FA', 'LT', 'LV', 'LY', 'P.', 'P,', 'Ta', 'Te', 'To', 'Tr', 'Tu', 'Tw', 'Ty', 'VA', 'Va', 'Ve', 'Vo', 'Wa', 'We', 'Wo', 'Ya', 'Ye', 'Yo', 'r.', 'r,', 'y.', 'y,'].map((p) => [p, -0.07]));

export function stubMeasure(font, text, kerning = 'auto') {
  const m = /(\d+(?:\.\d+)?)px/.exec(font);
  const size = m ? parseFloat(m[1]) : 10;
  const bold = /(^|\s)(bold|[6-9]00)(\s|$)/.test(font);
  let em = 0, prev = '';
  for (const ch of String(text)) {
    if (kerning !== 'none') em += STUB_KERN.get(prev + ch) ?? 0;
    prev = ch;
    const c = ch.codePointAt(0);
    let a;
    if (ch === ' ') a = 0.278;
    else if (/[.,:;'|!ijlI]/.test(ch)) a = 0.26;
    else if (/[mwMW]/.test(ch)) a = 0.82;
    else if (/[A-Z]/.test(ch)) a = 0.66;
    else if (/[0-9]/.test(ch)) a = 0.556;
    else if (c >= 0x2e80) a = 1; // CJK and wide scripts
    else a = 0.5;
    em += a;
  }
  if (bold) em *= 1.06;
  return { width: em * size, size };
}

/** What a recording run pins about the canvas (the TS tests these; plan 4.3 "capability set"). */
export const DEFAULT_CAPS = {
  /** `OffscreenCanvas` exists: layers, scratch canvases (patterns, fxImage, bevel, warp) are real */
  layers: true,
  /** `CanvasPattern.setTransform` and `DOMMatrix` exist */
  patternTransform: true,
  /** `ctx.letterSpacing` exists */
  letterSpacing: true,
  /** `ctx.fontKerning` exists */
  fontKerning: true,
};

class Root {
  constructor(opts) {
    this.lines = [];
    this.measures = new Map(); // `font\ttext` -> { width, n }
    this.caps = { ...DEFAULT_CAPS, ...(opts.caps ?? {}) };
    this.measure = opts.measure ?? stubMeasure;
    this.canvases = 0;
    this.gradients = 0;
    this.patterns = 0;
    this.counts = new Map();
    this.width = opts.width ?? 1280;
    this.height = opts.height ?? 720;
    this.imageDatas = 0;
  }
}

class RecGradient {
  constructor(root, kind, args) {
    this.root = root; this.kind = kind; this.args = args; this.stops = [];
    this.id = `g${++root.gradients}`;
  }
  addColorStop(offset, color) {
    if (!(offset >= 0 && offset <= 1)) throw new RangeError('IndexSizeError: color stop offset');
    this.stops.push([+offset, String(color)]);
    this.owner.emit(`stop @${this.id} ${fmt(offset)} ${q(color)}`);
  }
}

class RecPattern {
  constructor(root, src, repeat) {
    this.root = root; this.src = src; this.repeat = repeat; this.t = [1, 0, 0, 1, 0, 0];
    this.id = `p${++root.patterns}`;
  }
  setTransform(m) {
    if (!this.root.caps.patternTransform) return;
    this.t = [m.a, m.b, m.c, m.d, m.e, m.f];
    this.owner.emit(`patternTransform @${this.id} ${this.t.map(fmt).join(' ')}`);
  }
}

class RecCanvas {
  constructor(root, w, h, index) {
    this.root = root; this.width = w; this.height = h; this.index = index;
    this.hash = createHash('sha1');
    this.ctx = null;
  }
  getContext(kind) {
    if (kind !== '2d') return null;
    return (this.ctx ??= new RecContext(this.root, this, this.index));
  }
  /** `<w>x<h>:<recipe hash>`: the identity of this canvas as an image source. */
  sourceHash() {
    return `canvas(${this.width}x${this.height}:${this.hash.copy().digest('hex').slice(0, 12)})`;
  }
}

const DEFAULT_STATE = () => ({
  fillStyle: '#000000', strokeStyle: '#000000', globalAlpha: 1, globalCompositeOperation: 'source-over', filter: 'none',
  lineWidth: 1, lineCap: 'butt', lineJoin: 'miter', miterLimit: 10, lineDash: [], lineDashOffset: 0,
  shadowColor: 'rgba(0, 0, 0, 0)', shadowBlur: 0, shadowOffsetX: 0, shadowOffsetY: 0,
  font: '10px sans-serif', textAlign: 'start', textBaseline: 'alphabetic', letterSpacing: '0px', fontKerning: 'auto',
  imageSmoothingEnabled: true,
});

export class RecContext {
  constructor(root, canvas, index) {
    this.root = root;
    this.canvas = canvas;
    this._i = index; // 0 = the main canvas
    this._m = [1, 0, 0, 1, 0, 0];
    this._s = DEFAULT_STATE();
    this._stack = [];
    this._clips = 0;
    this._path = false;
    if (!root.caps.letterSpacing) Object.defineProperty(this, 'letterSpacing', { value: undefined, writable: true, enumerable: false, configurable: true });
    if (!root.caps.fontKerning) Object.defineProperty(this, 'fontKerning', { value: undefined, writable: true, enumerable: false, configurable: true });
  }

  // ------------------------------------------------------------------ recording plumbing
  emit(line) {
    const r = this.root;
    const op = line.slice(0, line.indexOf(' ') < 0 ? undefined : line.indexOf(' '));
    r.counts.set(op, (r.counts.get(op) ?? 0) + 1);
    this.canvas.hash.update(line + '\n');
    r.lines.push(this._i ? `@cv${this._i} ${line}` : line);
  }
  _pt(x, y) {
    const [a, b, c, d, e, f] = this._m;
    return [a * x + c * y + e, b * x + d * y + f];
  }
  _P(x, y) { const [X, Y] = this._pt(x, y); return `${fmt(X)} ${fmt(Y)}`; }
  _rect4(x, y, w, h) { return [this._P(x, y), this._P(x + w, y), this._P(x + w, y + h), this._P(x, y + h)].join(' '); }
  _det() { const [a, b, c, d] = this._m; return Math.sqrt(Math.abs(a * d - b * c)); }

  /** gradient / pattern / colour as it paints NOW (device space). */
  _paint(p) {
    if (typeof p === 'string') return q(p);
    const [a, b, c, d, e, f] = this._m;
    const T = (x, y) => `${fmt(a * x + c * y + e)},${fmt(b * x + d * y + f)}`;
    if (p instanceof RecGradient) {
      const s = p.args;
      const st = p.stops.map(([o, col]) => `${fmt(o)}:${col}`).join(';');
      if (p.kind === 'linear') return `lin(${T(s[0], s[1])},${T(s[2], s[3])}|${st})`;
      if (p.kind === 'radial') { const k = this._det(); return `rad(${T(s[0], s[1])},${fmt(s[2] * k)},${T(s[3], s[4])},${fmt(s[5] * k)}|${st})`; }
      return `conic(${fmt(s[0])},${T(s[1], s[2])}|${st})`;
    }
    if (p instanceof RecPattern) {
      const [pa, pb, pc, pd, pe, pf] = p.t;
      const m = [a * pa + c * pb, b * pa + d * pb, a * pc + c * pd, b * pc + d * pd, a * pe + c * pf + e, b * pe + d * pf + f];
      return `pat(${p.src},${p.repeat},${m.map(fmt).join(',')})`;
    }
    return q(String(p));
  }
  _fx() {
    const s = this._s;
    const parts = [`alpha=${fmt(s.globalAlpha)}`, `comp=${s.globalCompositeOperation}`];
    if (s.filter !== 'none') parts.push(`filter=${q(s.filter)}`);
    if (s.shadowColor !== 'rgba(0, 0, 0, 0)' && (s.shadowBlur || s.shadowOffsetX || s.shadowOffsetY)) {
      parts.push(`shadow=${q(s.shadowColor)},${fmt(s.shadowBlur)},${fmt(s.shadowOffsetX)},${fmt(s.shadowOffsetY)}`);
    }
    if (this._clips) parts.push(`clip=${this._clips}`);
    return parts.join(' ');
  }
  _stroke() {
    const s = this._s, k = this._det();
    let t = `ss=${this._paint(s.strokeStyle)} lw=${fmt(s.lineWidth * k)} cap=${s.lineCap} join=${s.lineJoin} miter=${fmt(s.miterLimit)}`;
    if (s.lineDash.length) t += ` dash=${s.lineDash.map((v) => fmt(v * k)).join(',')}@${fmt(s.lineDashOffset * k)}`;
    return t;
  }

  // ------------------------------------------------------------------ properties
  _set(name, v) {
    this._s[name] = v;
    this.emit(`set ${name} ${typeof v === 'number' ? fmt(v) : typeof v === 'boolean' ? String(v) : q(v)}`);
  }
  get globalAlpha() { return this._s.globalAlpha; }
  set globalAlpha(v) { if (typeof v === 'number' && v >= 0 && v <= 1) this._set('globalAlpha', v); }
  get globalCompositeOperation() { return this._s.globalCompositeOperation; }
  set globalCompositeOperation(v) { this._set('globalCompositeOperation', String(v)); }
  get filter() { return this._s.filter; }
  set filter(v) { this._set('filter', String(v)); }
  get lineWidth() { return this._s.lineWidth; }
  set lineWidth(v) { if (Number.isFinite(v) && v > 0) this._set('lineWidth', +v); }
  get lineCap() { return this._s.lineCap; }
  set lineCap(v) { this._set('lineCap', String(v)); }
  get lineJoin() { return this._s.lineJoin; }
  set lineJoin(v) { this._set('lineJoin', String(v)); }
  get miterLimit() { return this._s.miterLimit; }
  set miterLimit(v) { if (Number.isFinite(v) && v > 0) this._set('miterLimit', +v); }
  get lineDashOffset() { return this._s.lineDashOffset; }
  set lineDashOffset(v) { if (Number.isFinite(v)) this._set('lineDashOffset', +v); }
  get shadowColor() { return this._s.shadowColor; }
  set shadowColor(v) { this._set('shadowColor', String(v)); }
  get shadowBlur() { return this._s.shadowBlur; }
  set shadowBlur(v) { if (Number.isFinite(v) && v >= 0) this._set('shadowBlur', +v); }
  get shadowOffsetX() { return this._s.shadowOffsetX; }
  set shadowOffsetX(v) { if (Number.isFinite(v)) this._set('shadowOffsetX', +v); }
  get shadowOffsetY() { return this._s.shadowOffsetY; }
  set shadowOffsetY(v) { if (Number.isFinite(v)) this._set('shadowOffsetY', +v); }
  get font() { return this._s.font; }
  set font(v) { this._set('font', String(v)); }
  get textAlign() { return this._s.textAlign; }
  set textAlign(v) { this._set('textAlign', String(v)); }
  get textBaseline() { return this._s.textBaseline; }
  set textBaseline(v) { this._set('textBaseline', String(v)); }
  get letterSpacing() { return this._s.letterSpacing; }
  set letterSpacing(v) { this._set('letterSpacing', String(v)); }
  get fontKerning() { return this._s.fontKerning; }
  set fontKerning(v) { this._set('fontKerning', String(v)); }
  get imageSmoothingEnabled() { return this._s.imageSmoothingEnabled; }
  set imageSmoothingEnabled(v) { this._set('imageSmoothingEnabled', !!v); }
  get fillStyle() { return this._s.fillStyle; }
  set fillStyle(v) { this._style('fillStyle', v); }
  get strokeStyle() { return this._s.strokeStyle; }
  set strokeStyle(v) { this._style('strokeStyle', v); }
  _style(name, v) {
    this._s[name] = v;
    this.emit(`set ${name} ${v instanceof RecGradient ? '@' + v.id : v instanceof RecPattern ? '@' + v.id : q(v)}`);
  }

  // ------------------------------------------------------------------ state and transform
  save() {
    this._stack.push({ s: { ...this._s, lineDash: [...this._s.lineDash] }, m: [...this._m], clips: this._clips });
    this.emit('save');
  }
  restore() {
    const t = this._stack.pop();
    if (t) { this._s = t.s; this._m = t.m; this._clips = t.clips; }
    this.emit('restore');
    if (t) this._ctm();
  }
  _ctm() { this.emit(`ctm ${this._m.map(fmt).join(' ')}`); }
  translate(x, y) {
    if (Number.isFinite(x) && Number.isFinite(y)) { const [a, b, c, d, e, f] = this._m; this._m = [a, b, c, d, a * x + c * y + e, b * x + d * y + f]; }
    this._ctm();
  }
  rotate(t) {
    if (Number.isFinite(t)) {
      const [a, b, c, d, e, f] = this._m, co = Math.cos(t), si = Math.sin(t);
      this._m = [a * co + c * si, b * co + d * si, -a * si + c * co, -b * si + d * co, e, f];
    }
    this._ctm();
  }
  scale(x, y) {
    if (Number.isFinite(x) && Number.isFinite(y)) { const [a, b, c, d, e, f] = this._m; this._m = [a * x, b * x, c * y, d * y, e, f]; }
    this._ctm();
  }
  transform(a2, b2, c2, d2, e2, f2) {
    if ([a2, b2, c2, d2, e2, f2].every(Number.isFinite)) {
      const [a, b, c, d, e, f] = this._m;
      this._m = [a * a2 + c * b2, b * a2 + d * b2, a * c2 + c * d2, b * c2 + d * d2, a * e2 + c * f2 + e, b * e2 + d * f2 + f];
    }
    this._ctm();
  }
  setTransform(a, b, c, d, e, f) {
    const v = typeof a === 'object' && a ? [a.a, a.b, a.c, a.d, a.e, a.f] : [a, b, c, d, e, f];
    if (v.every(Number.isFinite)) this._m = v;
    this._ctm();
  }
  resetTransform() { this._m = [1, 0, 0, 1, 0, 0]; this._ctm(); }
  getTransform() { return new RecDOMMatrix([...this._m]); }
  setLineDash(seg) {
    const v = Array.from(seg ?? []);
    if (!v.every((x) => Number.isFinite(x) && x >= 0)) return;
    this._s.lineDash = v.length % 2 ? [...v, ...v] : v;
    this.emit(`setLineDash ${v.map(fmt).join(',')}`);
  }
  getLineDash() { return [...this._s.lineDash]; }

  // ------------------------------------------------------------------ path
  beginPath() { this.emit('beginPath'); }
  closePath() { this.emit('closePath'); }
  moveTo(x, y) { this.emit(`moveTo ${this._P(x, y)}`); }
  lineTo(x, y) { this.emit(`lineTo ${this._P(x, y)}`); }
  bezierCurveTo(x1, y1, x2, y2, x, y) { this.emit(`bezierCurveTo ${this._P(x1, y1)} ${this._P(x2, y2)} ${this._P(x, y)}`); }
  quadraticCurveTo(x1, y1, x, y) { this.emit(`quadraticCurveTo ${this._P(x1, y1)} ${this._P(x, y)}`); }
  rect(x, y, w, h) { this.emit(`rect ${this._rect4(x, y, w, h)}`); }
  arc(cx, cy, r, a0, a1, ccw = false) {
    if (r < 0) throw new RangeError('IndexSizeError: negative radius');
    const [a, b, c, d] = this._m;
    this.emit(`arc ${this._P(cx, cy)} ${fmt(a * r)} ${fmt(b * r)} ${fmt(c * r)} ${fmt(d * r)} ${fmt(a0)} ${fmt(a1)} ${ccw ? 1 : 0}`);
  }
  ellipse(cx, cy, rx, ry, rot, a0, a1, ccw = false) {
    if (rx < 0 || ry < 0) throw new RangeError('IndexSizeError: negative radius');
    const [a, b, c, d] = this._m, co = Math.cos(rot), si = Math.sin(rot);
    // device image of the unit circle: u = M Rot (rx, 0), v = M Rot (0, ry)
    const ux = co * rx, uy = si * rx, vx = -si * ry, vy = co * ry;
    this.emit(`ellipse ${this._P(cx, cy)} ${fmt(a * ux + c * uy)} ${fmt(b * ux + d * uy)} ${fmt(a * vx + c * vy)} ${fmt(b * vx + d * vy)} ${fmt(a0)} ${fmt(a1)} ${ccw ? 1 : 0}`);
  }
  arcTo(x1, y1, x2, y2, r) {
    if (r < 0) throw new RangeError('IndexSizeError: negative radius');
    const [a, b, c, d] = this._m;
    this.emit(`arcTo ${this._P(x1, y1)} ${this._P(x2, y2)} ${fmt(r)} ${fmt(a)} ${fmt(b)} ${fmt(c)} ${fmt(d)}`);
  }

  // ------------------------------------------------------------------ painting
  fill(rule = 'nonzero') { this.emit(`fill ${rule} | fs=${this._paint(this._s.fillStyle)} ${this._fx()}`); }
  stroke() { this.emit(`stroke | ${this._stroke()} ${this._fx()}`); }
  clip(rule = 'nonzero') { this._clips++; this.emit(`clip ${rule}`); }
  fillRect(x, y, w, h) { this.emit(`fillRect ${this._rect4(x, y, w, h)} | fs=${this._paint(this._s.fillStyle)} ${this._fx()}`); }
  strokeRect(x, y, w, h) { this.emit(`strokeRect ${this._rect4(x, y, w, h)} | ${this._stroke()} ${this._fx()}`); }
  clearRect(x, y, w, h) { this.emit(`clearRect ${this._rect4(x, y, w, h)}`); }
  _text() {
    const s = this._s, [a, b, c, d] = this._m;
    return `m=${[a, b, c, d].map(fmt).join(',')} font=${q(s.font)} align=${s.textAlign} base=${s.textBaseline} ls=${q(s.letterSpacing)} kern=${s.fontKerning}`;
  }
  fillText(t, x, y) { this.emit(`fillText ${q(t)} ${this._P(x, y)} | ${this._text()} fs=${this._paint(this._s.fillStyle)} ${this._fx()}`); }
  strokeText(t, x, y) { this.emit(`strokeText ${q(t)} ${this._P(x, y)} | ${this._text()} ${this._stroke()} ${this._fx()}`); }
  measureText(text) {
    const s = this._s;
    const { width, size } = this.root.measure(s.font, text, s.fontKerning);
    const key = `${q(s.font)}\t${q(text)}`;
    const e = this.root.measures.get(key);
    if (e) e.n++; else this.root.measures.set(key, { width, n: 1 });
    this.emit(`measure ${q(s.font)} ${q(text)}`);
    return {
      width, actualBoundingBoxLeft: 0, actualBoundingBoxRight: width, actualBoundingBoxAscent: 0.72 * size,
      actualBoundingBoxDescent: 0.21 * size, fontBoundingBoxAscent: 0.9 * size, fontBoundingBoxDescent: 0.21 * size,
    };
  }
  _src(img) {
    if (img instanceof RecCanvas) return img.sourceHash();
    if (img && typeof img.__h === 'string') return `img(${img.__h})`;
    return 'img(?)';
  }
  drawImage(img, a1, a2, a3, a4, a5, a6, a7, a8) {
    const src = this._src(img);
    let s = '', d;
    if (a3 === undefined) d = this._rect4(a1, a2, img.width, img.height);
    else if (a5 === undefined) d = this._rect4(a1, a2, a3, a4);
    else { s = ` s=${[a1, a2, a3, a4].map(fmt).join(',')}`; d = this._rect4(a5, a6, a7, a8); }
    this.emit(`drawImage ${src}${s} ${d} | smooth=${this._s.imageSmoothingEnabled ? 1 : 0} ${this._fx()}`);
  }
  createLinearGradient(...args) { return this._grad('linear', args); }
  createRadialGradient(...args) { return this._grad('radial', args); }
  createConicGradient(...args) { return this._grad('conic', args); }
  _grad(kind, args) {
    const g = new RecGradient(this.root, kind, args);
    g.owner = this;
    this.emit(`gradient @${g.id} ${kind} ${args.map(fmt).join(' ')}`);
    return g;
  }
  createPattern(img, repeat) {
    const p = new RecPattern(this.root, this._src(img), repeat ?? 'repeat');
    p.owner = this;
    this.emit(`pattern @${p.id} ${p.src} ${p.repeat}`);
    return p;
  }
  createImageData(w, h) {
    return { width: w, height: h, data: new Uint8ClampedArray(w * h * 4), colorSpace: 'srgb' };
  }
  getImageData(x, y, w, h) {
    this.emit(`getImageData ${fmt(x)} ${fmt(y)} ${fmt(w)} ${fmt(h)}`);
    return this.createImageData(w, h);
  }
  putImageData(id, x, y) {
    this.emit(`putImageData ${fmt(x)} ${fmt(y)} ${id.width} ${id.height} ${sha(id.data)}`);
  }
  isPointInPath() { return false; }
}

/**
 * A recording session: the context of the main canvas, the stream, the measure log, and the globals the TS tests
 * (`DOMMatrix`, `OffscreenCanvas` when `caps.layers`) installed for the duration of the run.
 */
export function createRecorder(opts = {}) {
  const root = new Root(opts);
  const main = new RecCanvas(root, root.width, root.height, 0);
  const ctx = main.getContext('2d');
  const saved = {};
  const mkCanvas = (w, h) => new RecCanvas(root, w, h, ++root.canvases);
  const api = {
    ctx,
    canvas: main,
    root,
    /** an offscreen canvas of the session (the host's `createCanvas` for paint-slides.ts) */
    createCanvas: mkCanvas,
    /** a decoded image of the resources: identified by its content hash */
    image(bytes, w, h) { return { width: w, height: h, naturalWidth: w, naturalHeight: h, __h: sha(bytes) }; },
    get lines() { return root.lines; },
    /** the stream as one text (LF-terminated lines) */
    text() { return root.lines.length ? root.lines.join('\n') + '\n' : ''; },
    /** the measure log: `"font"<TAB>"text"<TAB>width<TAB>count` lines (font and text JSON-quoted), sorted */
    measureLog() {
      return [...root.measures].sort((x, y) => (x[0] < y[0] ? -1 : x[0] > y[0] ? 1 : 0))
        .map(([k, v]) => `${k}\t${fmt(v.width)}\t${v.n}`);
    },
    counts() { return Object.fromEntries([...root.counts].sort()); },
    /** forget the recorded lines (not the state) */
    reset() { root.lines.length = 0; root.counts.clear(); root.measures.clear(); },
    install() {
      saved.DOMMatrix = Object.getOwnPropertyDescriptor(globalThis, 'DOMMatrix');
      saved.OffscreenCanvas = Object.getOwnPropertyDescriptor(globalThis, 'OffscreenCanvas');
      if (root.caps.patternTransform) Object.defineProperty(globalThis, 'DOMMatrix', { value: RecDOMMatrix, configurable: true, writable: true });
      if (root.caps.layers) {
        Object.defineProperty(globalThis, 'OffscreenCanvas', {
          value: class { constructor(w, h) { return mkCanvas(w, h); } }, configurable: true, writable: true,
        });
      }
      return api;
    },
    uninstall() {
      for (const k of ['DOMMatrix', 'OffscreenCanvas']) {
        if (saved[k]) Object.defineProperty(globalThis, k, saved[k]); else delete globalThis[k];
      }
    },
  };
  return api;
}
