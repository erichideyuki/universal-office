// The shell side of the metafile pictures (B5b of the Slides port): replays a recipe (`uo_dml_render::metafile::rec::Rec::encode`,
// "UOMF" version 1) on canvases. The Rust producer cannot paint, so it records every canvas call the frozen TS made while it
// rasterized a metafile, a PICT (`metafile.ts`, `emfplus.ts`, `pict.ts` over the canvas `raster.ts` creates) and this file plays
// them back one to one: the canvas it returns is the picture. `xorRect` (the exact PATINVERT on the pixels) is the TS code
// of `slides-effects.ts`. Erasable TypeScript only.
import { xorRect } from './slides-effects.ts';

/** A canvas as the player needs it (an `OffscreenCanvas`, a `<canvas>`, or the recorder's canvas). */
export interface MetaCanvas {
  width: number;
  height: number;
  getContext(kind: '2d'): unknown;
}

/** What the host gives the player. */
export interface MetaEnv {
  /** a new canvas `w` x `h` (index 0 of the recipe is made with it too) */
  createCanvas(w: number, h: number): MetaCanvas | undefined;
  /** the decoded image of a compressed EMF+ bitmap (PNG/JPEG...), prepared by the shell; null = not available */
  decode?(data: Uint8Array): CanvasImageSource | null | undefined;
}

type Ctx = CanvasRenderingContext2D;

class Reader {
  p = 0;
  readonly dv: DataView;
  readonly b: Uint8Array;
  constructor(b: Uint8Array) {
    this.b = b;
    this.dv = new DataView(b.buffer, b.byteOffset, b.byteLength);
  }
  u8(): number { return this.b[this.p++]!; }
  u32(): number { const v = this.dv.getUint32(this.p, true); this.p += 4; return v; }
  f64(): number { const v = this.dv.getFloat64(this.p, true); this.p += 8; return v; }
  /** a JS string of UTF-16 code units (a lone surrogate survives) */
  str(): string {
    const n = this.u32();
    let s = '';
    for (let i = 0; i < n; i += 4096) {
      const m = Math.min(4096, n - i);
      const units = new Array<number>(m);
      for (let k = 0; k < m; k++) units[k] = this.dv.getUint16(this.p + (i + k) * 2, true);
      s += String.fromCharCode(...units);
    }
    this.p += n * 2;
    return s;
  }
  f64s(n: number): number[] { const a: number[] = []; for (let i = 0; i < n; i++) a.push(this.f64()); return a; }
}

const COMP = ['source-over', 'difference', 'multiply', 'lighten'] as const;
const CAP = ['butt', 'round', 'square'] as const;
const JOIN = ['round', 'bevel', 'miter'] as const;
const RULE = ['nonzero', 'evenodd'] as const;

/** Header of a recipe: the size of the layer canvas and whether the recording was cut by a cap. */
export function recipeInfo(bytes: Uint8Array): { w: number; h: number; truncated: boolean } | null {
  if (bytes.length < 14 || bytes[0] !== 0x55 || bytes[1] !== 0x4f || bytes[2] !== 0x4d || bytes[3] !== 0x46 || bytes[4] !== 1) return null;
  const dv = new DataView(bytes.buffer, bytes.byteOffset, bytes.byteLength);
  return { w: dv.getUint32(5, true), h: dv.getUint32(9, true), truncated: bytes[13] !== 0 };
}

/** The colour effects of a derived recipe, in the form `fxImage` (`slides-effects.ts`) takes. */
export interface RecipeFx {
  duotone?: [string, string];
  gray?: boolean;
  biLevel?: number;
  bright?: number;
  contrast?: number;
  clrChange?: { from: string; to: string; toAlpha: number };
}

/**
 * A derived recipe ("UOMF" version 2, `uo_dml_render::metafile::rec::encode_fx_recipe`, gap B5b-G1): a metafile picture with
 * the blip's colour effects = `fxImage` over the raster of bitmap `base` (the TS applied `fxImage` to the rasterized canvas).
 * Null for any other bytes.
 */
export function decodeFxRecipe(bytes: Uint8Array): { base: number; fx: RecipeFx } | null {
  if (bytes.length < 19 || bytes[0] !== 0x55 || bytes[1] !== 0x4f || bytes[2] !== 0x4d || bytes[3] !== 0x46 || bytes[4] !== 2) return null;
  const r = new Reader(bytes);
  r.p = 14;
  const base = r.u32();
  const mask = r.u8();
  const fx: RecipeFx = {};
  if (mask & 1) fx.duotone = [r.str(), r.str()];
  if (mask & 2) fx.gray = true;
  if (mask & 4) fx.biLevel = r.f64();
  if (mask & 8) fx.bright = r.f64();
  if (mask & 16) fx.contrast = r.f64();
  if (mask & 32) { const from = r.str(), to = r.str(); fx.clrChange = { from, to, toAlpha: r.f64() }; }
  return { base, fx };
}

/**
 * Plays a recipe: returns the layer canvas (index 0), or null for bytes that are no recipe or a canvas the host refuses.
 * A throw inside (a canvas refusing a call, as the TS caught at its `try`) ends the replay and keeps what was drawn.
 */
export function replayRecipe(bytes: Uint8Array, env: MetaEnv): MetaCanvas | null {
  const info = recipeInfo(bytes);
  if (!info) return null;
  const r = new Reader(bytes);
  r.p = 14;
  const nBlobs = r.u32();
  const blobs: Uint8Array[] = [];
  for (let i = 0; i < nBlobs; i++) { const n = r.u32(); blobs.push(bytes.subarray(r.p, r.p + n)); r.p += n; }
  const nOps = r.u32();
  const root = env.createCanvas(info.w, info.h);
  if (!root) return null;
  const canvases: MetaCanvas[] = [root];
  const ctxs: Ctx[] = [];
  const ctxOf = (k: number): Ctx => (ctxs[k] ??= canvases[k]!.getContext('2d') as Ctx);
  const grads: CanvasGradient[] = [];
  const pats: (CanvasPattern | null)[] = [];
  let cur: Ctx = ctxOf(0);
  const style = (): string | CanvasGradient | CanvasPattern => {
    const t = r.u8();
    if (t === 0) return r.str();
    const i = r.u32();
    return (t === 1 ? grads[i] : pats[i]) as CanvasGradient | CanvasPattern;
  };
  try {
    for (let n = 0; n < nOps; n++) {
      const op = r.u8();
      if ((globalThis as { __metaTrace?: boolean }).__metaTrace) console.error('op', n, op);
      switch (op) {
        case 0: cur = ctxOf(r.u32()); break;
        case 1: { const w = r.u32(), h = r.u32(); const c = env.createCanvas(w, h); if (!c) throw new Error('canvas'); canvases.push(c); break; }
        case 2: cur.save(); break;
        case 3: cur.restore(); break;
        case 4: { const [x, y] = r.f64s(2); cur.translate(x!, y!); break; }
        case 5: cur.rotate(r.f64()); break;
        case 6: { const [x, y] = r.f64s(2); cur.scale(x!, y!); break; }
        case 7: { const m = r.f64s(6); cur.transform(m[0]!, m[1]!, m[2]!, m[3]!, m[4]!, m[5]!); break; }
        case 8: cur.fillStyle = style(); break;
        case 9: cur.strokeStyle = style(); break;
        case 10: cur.lineWidth = r.f64(); break;
        case 11: cur.lineCap = CAP[r.u8()]!; break;
        case 12: cur.lineJoin = JOIN[r.u8()]!; break;
        case 13: cur.miterLimit = r.f64(); break;
        case 14: cur.lineDashOffset = r.f64(); break;
        case 15: cur.globalAlpha = r.f64(); break;
        case 16: cur.globalCompositeOperation = COMP[r.u8()]!; break;
        case 17: cur.font = r.str(); break;
        case 18: cur.textBaseline = 'alphabetic'; break;
        case 19: cur.imageSmoothingEnabled = r.u8() !== 0; break;
        case 20: (cur as { imageSmoothingQuality?: string }).imageSmoothingQuality = 'high'; break;
        case 21: { const m = r.u32(); cur.setLineDash(r.f64s(m)); break; }
        case 22: cur.beginPath(); break;
        case 23: cur.closePath(); break;
        case 24: { const [x, y] = r.f64s(2); cur.moveTo(x!, y!); break; }
        case 25: { const [x, y] = r.f64s(2); cur.lineTo(x!, y!); break; }
        case 26: { const m = r.f64s(6); cur.bezierCurveTo(m[0]!, m[1]!, m[2]!, m[3]!, m[4]!, m[5]!); break; }
        case 27: { const m = r.f64s(4); cur.rect(m[0]!, m[1]!, m[2]!, m[3]!); break; }
        case 28: { const m = r.f64s(5); cur.arc(m[0]!, m[1]!, m[2]!, m[3]!, m[4]!, r.u8() !== 0); break; }
        case 29: { const m = r.f64s(7); cur.ellipse(m[0]!, m[1]!, m[2]!, m[3]!, m[4]!, m[5]!, m[6]!, r.u8() !== 0); break; }
        case 30: cur.fill(RULE[r.u8()]!); break;
        case 31: cur.stroke(); break;
        case 32: cur.clip(RULE[r.u8()]!); break;
        case 33: { const m = r.f64s(4); cur.fillRect(m[0]!, m[1]!, m[2]!, m[3]!); break; }
        case 34: { const t = r.str(); const [x, y] = r.f64s(2); cur.fillText(t, x!, y!); break; }
        case 35: {
          const kind = r.u8(), k = r.u32(), n = r.u8();
          const a = r.f64s(n - 1);
          let src: CanvasImageSource | null | undefined;
          if (kind === 0) src = canvases[k] as unknown as CanvasImageSource;
          else src = env.decode?.(blobs[k]!);
          if (!src) break; // a bitmap the shell could not decode: the TS drew nothing (it skipped the image)
          if (n === 3) cur.drawImage(src, a[0]!, a[1]!);
          else if (n === 5) cur.drawImage(src, a[0]!, a[1]!, a[2]!, a[3]!);
          else cur.drawImage(src, a[0]!, a[1]!, a[2]!, a[3]!, a[4]!, a[5]!, a[6]!, a[7]!);
          break;
        }
        case 36: { const m = r.f64s(4); grads.push(cur.createLinearGradient(m[0]!, m[1]!, m[2]!, m[3]!)); break; }
        case 37: { const g = r.u32(), off = r.f64(), c = r.str(); grads[g]!.addColorStop(off, c); break; }
        case 38: pats.push(cur.createPattern(canvases[r.u32()] as unknown as CanvasImageSource, 'repeat')); break;
        case 39: {
          const p = r.u32(), m = r.f64s(6);
          const pat = pats[p];
          if (pat && typeof DOMMatrix !== 'undefined') pat.setTransform(new DOMMatrix(m));
          break;
        }
        case 40: {
          const blob = r.u32(), w = r.u32(), h = r.u32();
          const [x, y] = r.f64s(2);
          const id = cur.createImageData(w, h);
          id.data.set(blobs[blob]!.subarray(0, w * h * 4));
          cur.putImageData(id, x!, y!);
          break;
        }
        case 41: {
          const c = r.u32();
          const [x0, y0, x1, y1] = r.f64s(4);
          const skip = r.u32();
          if (xorRect(cur, [(c >> 16) & 0xff, (c >> 8) & 0xff, c & 0xff], x0!, y0!, x1!, y1!, false)) {
            // the exact XOR is done: the TS skipped the approximation that follows
            for (let i = 0; i < skip; i++) skipOp(r);
            n += skip;
          }
          break;
        }
        default: throw new Error(`bad recipe op ${op}`);
      }
    }
  } catch (e) {
    if ((globalThis as { __metaDebug?: boolean }).__metaDebug) console.error('meta-player:', e);
    // a canvas refused a call (the TS caught it where it rasterized): what was drawn stays
  }
  return root;
}

/** Skips one op (the approximation after a successful `xorRect`). */
function skipOp(r: Reader): void {
  const op = r.u8();
  if ((globalThis as { __metaTrace?: boolean }).__metaTrace) console.error('  skip', op);
  const f = (n: number): void => { r.p += n * 8; };
  const s = (): void => { const n = r.u32(); r.p += n * 2; };
  const st = (): void => { const t = r.u8(); if (t === 0) s(); else r.p += 4; };
  switch (op) {
    case 0: r.p += 4; break;
    case 1: r.p += 8; break;
    case 2: case 3: case 18: case 20: case 22: case 23: case 31: break;
    case 4: case 6: case 24: case 25: f(2); break;
    case 5: case 10: case 13: case 14: case 15: f(1); break;
    case 7: case 26: f(6); break;
    case 8: case 9: st(); break;
    case 11: case 12: case 16: case 19: case 30: case 32: r.p += 1; break;
    case 17: s(); break;
    case 21: { const n = r.u32(); f(n); break; }
    case 27: case 33: case 36: f(4); break;
    case 28: f(5); r.p += 1; break;
    case 29: f(7); r.p += 1; break;
    case 34: s(); f(2); break;
    case 35: { r.p += 1 + 4; const n = r.u8(); f(n - 1); break; }
    case 37: r.p += 4; f(1); s(); break;
    case 38: r.p += 4; break;
    case 39: r.p += 4; f(6); break;
    case 40: r.p += 12; f(2); break;
    case 41: r.p += 4; f(4); r.p += 4; break;
    default: throw new Error(`bad recipe op ${op}`);
  }
}
