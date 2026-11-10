// A recording CanvasRenderingContext2D for the painter parity of uo-render frontier V: the frozen TS `paintItems`
// (canvas.ts) and the TS painter of decoded pages (js/paint.ts) run on it and every call and every property set is logged
// in the encoding of tools/capture-e.mjs (name, arguments as n / s / a), so two runs are equal when their logs are.
//
// It keeps a REAL affine stack (save/restore/translate/scale/rotate/transform/setTransform, `getTransform` answers like a
// browser), logs the methods and the property sets (font, fillStyle, strokeStyle, lineWidth, globalAlpha, ...), hands out
// tokens for gradients, patterns and canvases, and installs `OffscreenCanvas` and `DOMMatrix` stubs while painting. A stub
// canvas has a creation-ordered token (`canvas#k`) and an RGBA backing initialised from (w, h, k) with a deterministic
// pseudo-random pattern (so `getImageData` answers non-trivial alpha); `putImageData` writes the backing, `drawImage` logs
// the source token and the FNV-1a of its backing but does NOT rasterise. Limitation (declared in the record): the pixels
// the picture layer composes through drawImage/clip are not computed here; Chromium (tests/browser/paint_pixels.mjs) covers
// them.
import { createHash } from 'node:crypto';

export const fnv = (bytes) => {
  let h = 0x811c9dc5;
  for (let i = 0; i < bytes.length; i++) h = Math.imul(h ^ bytes[i], 0x01000193);
  return (h >>> 0).toString(16).padStart(8, '0');
};

const canon = (v) => (v !== v ? NaN : v);

/** The log encoding of capture-e.mjs `arg`/`logOut` on a testkit Stream. */
export function arg(s, v) {
  if (typeof v === 'number') { s.tag('n'); s.f64(canon(v)); } else if (typeof v === 'string') {
    const cps = [...v].map((c) => c.codePointAt(0));
    s.tag('s'); s.u32(cps.length); for (const c of cps) s.u32(c);
  } else if (Array.isArray(v)) { s.tag('a'); s.u32(v.length); for (const e of v) arg(s, e); } else s.tag('?');
}
export function logEntry(s, name, a) {
  s.str(name); s.u32(a.length); for (const v of a) arg(s, v);
}

/** A sink that digests the log into a testkit Stream and counts the calls. */
export class DigestSink {
  constructor(stream) { this.s = stream; this.calls = 0; }
  push(name, args) { this.calls++; logEntry(this.s, name, args); }
}
/** A sink that keeps the entries (for the call-by-call diff). */
export class ListSink {
  constructor() { this.log = []; }
  push(name, args) { this.log.push([name, args]); }
}

const LOGGED_METHODS = [
  'save', 'restore', 'translate', 'scale', 'rotate', 'transform', 'setTransform', 'beginPath', 'moveTo', 'lineTo', 'quadraticCurveTo',
  'bezierCurveTo', 'closePath', 'rect', 'clip', 'fill', 'stroke', 'fillRect', 'fillText', 'strokeText', 'setLineDash', 'drawImage',
  'createImageData', 'putImageData', 'getImageData',
];
const PROPS = ['font', 'fillStyle', 'strokeStyle', 'lineWidth', 'lineCap', 'lineJoin', 'globalAlpha', 'textAlign', 'textBaseline', 'fontKerning', 'imageSmoothingEnabled', 'imageSmoothingQuality', 'globalCompositeOperation'];

/**
 * An installation: the global stubs, the token counters and the context factory. One per process run; `install()` /
 * `uninstall()` bracket the painting. Canvas tokens are creation-ordered across the whole installation, so two runs that
 * paint the same pages in the same order get the same tokens.
 */
export class Recording {
  constructor(sink, { kerning = true, scale = 1, screen = false } = {}) {
    this.sink = sink;
    this.kerning = kerning;
    this.scale = scale;
    this.canvases = 0;
    this.tokens = 0;
    this.saved = undefined;
  }

  install() {
    const rec = this;
    class DOMMatrixStub {
      constructor(init) {
        const a = Array.isArray(init) ? init : [1, 0, 0, 1, 0, 0];
        [this.a, this.b, this.c, this.d, this.e, this.f] = a;
      }
    }
    class OffscreenCanvasStub {
      constructor(w, h) {
        this.width = w; this.height = h;
        this.token = `canvas#${rec.canvases++}`;
        this.k = rec.canvases - 1;
        this.backing = null;
        this.ctx = null;
      }
      getContext(kind) {
        if (kind !== '2d') return null;
        if (!this.ctx) this.ctx = rec.context(this);
        return this.ctx;
      }
      /** the RGBA backing, created on first use: a deterministic pattern of (w, h, k) */
      pixels() {
        const n = Math.max(0, Math.floor(this.width) * Math.floor(this.height) * 4) | 0;
        if (!this.backing || this.backing.length !== n) {
          const b = new Uint8ClampedArray(n);
          let x = (Math.imul(this.width | 0, 73856093) ^ Math.imul(this.height | 0, 19349663) ^ Math.imul(this.k + 1, 83492791)) >>> 0 || 1;
          for (let i = 0; i < n; i++) { x ^= x << 13; x >>>= 0; x ^= x >>> 17; x ^= x << 5; x >>>= 0; b[i] = i % 4 === 3 ? (x % 5 === 0 ? 0 : 128 + (x % 128)) : x & 255; }
          this.backing = b;
        }
        return this.backing;
      }
    }
    this.saved = { OffscreenCanvas: globalThis.OffscreenCanvas, DOMMatrix: globalThis.DOMMatrix };
    globalThis.OffscreenCanvas = OffscreenCanvasStub;
    globalThis.DOMMatrix = DOMMatrixStub;
    this.OffscreenCanvasStub = OffscreenCanvasStub;
    return this;
  }

  uninstall() {
    if (!this.saved) return;
    if (this.saved.OffscreenCanvas === undefined) delete globalThis.OffscreenCanvas; else globalThis.OffscreenCanvas = this.saved.OffscreenCanvas;
    if (this.saved.DOMMatrix === undefined) delete globalThis.DOMMatrix; else globalThis.DOMMatrix = this.saved.DOMMatrix;
    this.saved = undefined;
  }

  token(kind) { return `${kind}#${this.tokens++}`; }

  /** a value as a loggable argument: tokens for objects */
  val(v) {
    if (v === null || v === undefined) return String(v);
    if (typeof v === 'number' || typeof v === 'string') return v;
    if (typeof v === 'boolean') return v ? 'true' : 'false';
    if (Array.isArray(v)) return v.map((e) => this.val(e));
    if (ArrayBuffer.isView(v)) return `view:${v.byteLength}:${fnv(new Uint8Array(v.buffer, v.byteOffset, v.byteLength))}`;
    if (v.token) return v.token;
    if (v.__tok) return v.__tok;
    if (typeof v === 'object' && 'data' in v && 'width' in v) return `imagedata:${v.width}x${v.height}:${fnv(new Uint8Array(v.data.buffer, v.data.byteOffset, v.data.byteLength))}`;
    return '?obj';
  }

  /** the log of a source image of drawImage / createPattern: its token and, for a canvas, the digest of its pixels */
  src(s) {
    if (s && s.pixels) return [s.token, `px:${fnv(new Uint8Array(s.pixels().buffer))}`];
    return [this.val(s)];
  }

  /** a recording context; `canvas` is the stub canvas it belongs to (undefined for the page context). Sub-contexts log with the canvas token as prefix. */
  context(canvas) {
    const rec = this;
    const prefix = canvas ? `${canvas.token}:` : '';
    const log = (name, args) => rec.sink.push(prefix + name, args);
    let m = canvas || rec.initial === undefined ? [1, 0, 0, 1, 0, 0] : rec.initial.slice();
    const stack = [];
    const mul = (n) => { // m = m * n  (n applied first)
      const [a, b, c, d, e, f] = m;
      m = [a * n[0] + c * n[1], b * n[0] + d * n[1], a * n[2] + c * n[3], b * n[2] + d * n[3], a * n[4] + c * n[5] + e, b * n[4] + d * n[5] + f];
    };
    const ctx = {
      canvas: canvas ?? null,
      getTransform: () => ({ a: m[0], b: m[1], c: m[2], d: m[3], e: m[4], f: m[5] }),
      save: () => { log('save', []); stack.push(m.slice()); },
      restore: () => { log('restore', []); if (stack.length) m = stack.pop(); },
      translate: (x, y) => { log('translate', [x, y]); mul([1, 0, 0, 1, x, y]); },
      scale: (x, y) => { log('scale', [x, y]); mul([x, 0, 0, y, 0, 0]); },
      rotate: (r) => { log('rotate', [r]); const c = Math.cos(r), s = Math.sin(r); mul([c, s, -s, c, 0, 0]); },
      transform: (a, b, c, d, e, f) => { log('transform', [a, b, c, d, e, f]); mul([a, b, c, d, e, f]); },
      setTransform: (a, b, c, d, e, f) => { log('setTransform', [a, b, c, d, e, f]); m = [a, b, c, d, e, f]; },
      beginPath: () => log('beginPath', []),
      moveTo: (x, y) => log('moveTo', [x, y]),
      lineTo: (x, y) => log('lineTo', [x, y]),
      quadraticCurveTo: (a, b, c, d) => log('quadraticCurveTo', [a, b, c, d]),
      bezierCurveTo: (a, b, c, d, e, f) => log('bezierCurveTo', [a, b, c, d, e, f]),
      closePath: () => log('closePath', []),
      rect: (x, y, w, h) => log('rect', [x, y, w, h]),
      clip: (rule) => log('clip', rule === undefined ? [] : [rule]),
      fill: (rule) => log('fill', rule === undefined ? [] : [rule]),
      stroke: () => log('stroke', []),
      fillRect: (x, y, w, h) => log('fillRect', [x, y, w, h]),
      fillText: (t, x, y) => log('fillText', [t, x, y]),
      strokeText: (t, x, y) => log('strokeText', [t, x, y]),
      setLineDash: (d) => log('setLineDash', [d]),
      drawImage: (s, ...a) => log('drawImage', [...rec.src(s), ...a]),
      createLinearGradient: (...a) => { log('createLinearGradient', a); return rec.gradient(log); },
      createRadialGradient: (...a) => { log('createRadialGradient', a); return rec.gradient(log); },
      createPattern: (s, rep) => {
        log('createPattern', [...rec.src(s), rep]);
        const token = rec.token('pattern');
        return { token, setTransform: (mt) => log('pattern.setTransform', [token, mt.a, mt.b, mt.c, mt.d, mt.e, mt.f]) };
      },
      createImageData: (w, h) => {
        log('createImageData', [w, h]);
        return { width: w, height: h, data: new Uint8ClampedArray(Math.max(0, Math.floor(w) * Math.floor(h) * 4)) };
      },
      putImageData: (img, x, y) => {
        log('putImageData', [img.width, img.height, x, y, fnv(new Uint8Array(img.data.buffer, img.data.byteOffset, img.data.byteLength))]);
        if (canvas && x === 0 && y === 0 && img.width === canvas.width && img.height === canvas.height) canvas.pixels().set(img.data);
      },
      getImageData: (x, y, w, h) => {
        log('getImageData', [x, y, w, h]);
        const out = new Uint8ClampedArray(Math.max(0, Math.floor(w) * Math.floor(h) * 4));
        if (canvas) {
          const px = canvas.pixels();
          for (let r = 0; r < h; r++) for (let c = 0; c < w; c++) {
            const sx = x + c, sy = y + r;
            if (sx < 0 || sy < 0 || sx >= canvas.width || sy >= canvas.height) continue;
            const si = (sy * canvas.width + sx) * 4, di = (r * w + c) * 4;
            out[di] = px[si]; out[di + 1] = px[si + 1]; out[di + 2] = px[si + 2]; out[di + 3] = px[si + 3];
          }
        } else for (let i = 0; i < out.length; i++) out[i] = (i * 31 + x + y) & 255;
        return { width: w, height: h, data: out };
      },
    };
    for (const p of PROPS) {
      let v;
      Object.defineProperty(ctx, p, { get: () => v, set: (nv) => { v = nv; log(`set:${p}`, [rec.val(nv)]); }, enumerable: true, configurable: true });
    }
    if (!rec.kerning && !canvas) delete ctx.fontKerning;
    else if (canvas) delete ctx.fontKerning; // browsers' 2D contexts have it; the painters only batch on the page context
    return ctx;
  }

  gradient(log) {
    const token = this.token('grad');
    return { token, addColorStop: (...b) => log('addColorStop', [token, ...b]) };
  }

  /** the page context: the scale pre-applied (not logged), `fontKerning` present or not */
  pageContext() {
    this.initial = [this.scale, 0, 0, this.scale, 0, 0];
    const ctx = this.context(undefined);
    this.initial = undefined;
    return ctx;
  }
}

/** a recorded context for one page: `new Recording(sink, { kerning, scale }).install().pageContext()` */

/** The PNG / BMP pixel size of image bytes (16 x 16 for anything else): what the stub bitmap token reports. */
export function imageDims(bytes) {
  if (bytes.length > 24 && bytes[0] === 0x89 && bytes[1] === 0x50) {
    const u32 = (i) => ((bytes[i] << 24) | (bytes[i + 1] << 16) | (bytes[i + 2] << 8) | bytes[i + 3]) >>> 0;
    return { width: u32(16), height: u32(20) };
  }
  if (bytes.length > 26 && bytes[0] === 0x42 && bytes[1] === 0x4d) {
    const dv = new DataView(bytes.buffer, bytes.byteOffset, bytes.byteLength);
    return { width: Math.abs(dv.getInt32(18, true)), height: Math.abs(dv.getInt32(22, true)) };
  }
  return { width: 16, height: 16 };
}

export const sha = (bytes) => createHash('sha256').update(bytes).digest('hex').slice(0, 16);

/** A bitmap token for image bytes: the stub of `env.image` (the same on both sides). */
export function bitmapOf(bytes) {
  const { width, height } = imageDims(bytes);
  return { __tok: `img:${sha(bytes)}`, width, height };
}

/** The `fontOf` both painters use (the key of a face differs by design: a provider path vs `Family|variant`). */
export const fontOf = (face) => ({
  family: JSON.stringify([face.family, !!face.bold, !!face.italic, !!face.synthBold, !!face.synthItalic]),
  weight: face.bold ? 700 : 400,
  italic: !!face.italic,
});
export const colorOf = (h) => `#${h}`;
export { LOGGED_METHODS };
