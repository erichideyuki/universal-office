// MOVED CODE (Slides port R-S3): the shell-executed `Effect` code of the frozen TS `slides/packages/slides-core/src/render.ts`
// (ts-ref-092 b92b60fa): `fxImage` (448-503), `applyBevel` with `edt1d` and the sRGB tables (1295-1382), the PATINVERT
// rectangle of `metafile.ts` (`xorRect`, 307-330), the canvas-size helpers `imgW`/`imgH`/`scratchCanvas` (406-446).
// The comments are the frozen TS's own (Portuguese), kept so the bodies diff against the source; the list of adaptations
// is in core/docs/modules/uo-render.md "R-S3/R-S4". Erasable TypeScript only (the generated codec's flags).
//
// Adaptations (inputs only, no behaviour change):
//  - `fxImage(img, fx, scratch)`: the canvas factory is a parameter (`scratchCanvas` of the env); the `ImageFx` is the list's
//    (`ImageFx` of gen/slides.ts, converted by the caller to the TS model shape); the cache is the same WeakMap.
//  - `applyBevel(ctx, e, ops, scratch, light)`: `sh.bevel` is the effect payload (`w`, `h`, `prst`, `rig`, `dir`, `mat`), `x y w h`
//    are `e.bx`, `scale` is `e.scale`; `shapePath(m2, geom, ...)` is the replay of the silhouette path ops (`ops`) with the
//    fill rule of the effect (`rule || fill()` is `fill(rule)`); the light tables (`lightTable` ...) are injected (`light`).
//  - `xorRect(ctx, rgb, ...)`: the colour is the operand bitmap's pixel (solid operand) instead of a `#RRGGBB` string.
import type { BevelEffect, ImageFx as WireFx, PathOp } from './gen/slides.ts';

/** Imagem já decodificada, pronta para `ctx.drawImage` (as in render.ts). */
export type PaintImage = CanvasImageSource & {
  naturalWidth?: number;
  naturalHeight?: number;
  width?: unknown;
  height?: unknown;
};

/** The canvas factory of the host (an `OffscreenCanvas`, a `<canvas>`, or the test recorder's canvas). */
export type ScratchFactory = (w: number, h: number) => (HTMLCanvasElement | OffscreenCanvas) | undefined;

/** The TS `ImageFx` of the model (hex colours without the list's symbol table). */
export interface TsImageFx {
  duotone?: [string, string];
  gray?: boolean;
  biLevel?: number;
  bright?: number;
  contrast?: number;
  clrChange?: { from: string; to: string; toAlpha: number };
}

/** The wire `ImageFx` as the TS model shape. */
export function toTsFx(fx: WireFx): TsImageFx {
  const o: TsImageFx = {};
  if (fx.duotone) o.duotone = [fx.duotone.first, fx.duotone.second];
  if (fx.gray) o.gray = true;
  if (fx.biLevel !== undefined) o.biLevel = fx.biLevel;
  if (fx.bright !== undefined) o.bright = fx.bright;
  if (fx.contrast !== undefined) o.contrast = fx.contrast;
  if (fx.clrChange) o.clrChange = { from: fx.clrChange.from, to: fx.clrChange.to, toAlpha: fx.clrChange.toAlpha };
  return o;
}

/** Light tables of uo-drawingml (`lightTable`, `lightAt`, `lightFace`, `bevelPhi`), injected by the host. */
export interface LightTables {
  lightTable(rig: string | undefined, mat: string | undefined): Uint8Array | undefined;
  lightAt(t: Uint8Array, dir: string | undefined, thetaDeg: number, phiDeg: number, out: Float32Array): void;
  lightFace(t: Uint8Array, out: Float32Array): void;
  bevelPhi(prst: string | undefined, t: number, hw: number): number;
}

/** Largura natural de uma PaintImage (HTMLImageElement usa naturalWidth; vídeo, videoWidth; ImageBitmap, width). */
export function imgW(img: PaintImage): number {
  const vw = (img as { videoWidth?: unknown }).videoWidth;
  if (typeof vw === 'number' && vw > 0) return vw;
  return typeof img.naturalWidth === 'number' && img.naturalWidth > 0
    ? img.naturalWidth
    : typeof img.width === 'number' ? img.width : 0;
}
export function imgH(img: PaintImage): number {
  const vh = (img as { videoHeight?: unknown }).videoHeight;
  if (typeof vh === 'number' && vh > 0) return vh;
  return typeof img.naturalHeight === 'number' && img.naturalHeight > 0
    ? img.naturalHeight
    : typeof img.height === 'number' ? img.height : 0;
}

/** Canvas auxiliar (OffscreenCanvas ou <canvas>); sem nenhum (Node sem fábrica) = undefined. */
export function scratchCanvas(w: number, h: number): (HTMLCanvasElement | OffscreenCanvas) | undefined {
  if (typeof OffscreenCanvas !== 'undefined') return new OffscreenCanvas(w, h);
  const doc = (globalThis as { document?: Document }).document;
  if (!doc) return undefined;
  const c = doc.createElement('canvas'); c.width = w; c.height = h; return c;
}

const fxCache = new WeakMap<object, Map<string, PaintImage>>();
const hexRgb = (h: string): [number, number, number] => { const n = parseInt(h.replace('#', ''), 16); return [(n >> 16) & 255, (n >> 8) & 255, n & 255]; };

/**
 * Imagem com os efeitos de cor do `<a:blip>` (duotone, escala de cinza,
 * brilho/contraste, preto-e-branco, troca de cor) — processada uma vez por
 * (imagem, efeito) e guardada. `alpha` NÃO entra aqui (vira globalAlpha).
 */
export function fxImage(img: PaintImage, fx: TsImageFx | undefined, scratch: ScratchFactory): PaintImage {
  if (!fx || !(fx.duotone || fx.gray || fx.biLevel !== undefined || fx.bright || fx.contrast || fx.clrChange)) return img;
  const key = JSON.stringify([fx.duotone, fx.gray, fx.biLevel, fx.bright, fx.contrast, fx.clrChange]);
  let m = fxCache.get(img as object);
  if (!m) { m = new Map(); fxCache.set(img as object, m); }
  const hit = m.get(key);
  if (hit) return hit;
  const w = imgW(img), h = imgH(img);
  const cv = w > 0 && h > 0 ? scratch(w, h) : undefined;
  const c2 = cv?.getContext('2d') as CanvasRenderingContext2D | OffscreenCanvasRenderingContext2D | null | undefined;
  if (!cv || !c2) return img;
  try {
    c2.drawImage(img, 0, 0, w, h);
    const id = c2.getImageData(0, 0, w, h);
    const d = id.data;
    const from = fx.clrChange ? hexRgb(fx.clrChange.from) : undefined;
    const to = fx.clrChange ? hexRgb(fx.clrChange.to) : undefined;
    const d0 = fx.duotone ? hexRgb(fx.duotone[0]) : undefined, d1 = fx.duotone ? hexRgb(fx.duotone[1]) : undefined;
    const b = fx.bright ?? 0, c = fx.contrast ?? 0;
    const k = c >= 0 ? 1 / Math.max(0.0001, 1 - c) : 1 + c;
    for (let i = 0; i < d.length; i += 4) {
      let r = d[i]!, g = d[i + 1]!, bl = d[i + 2]!;
      if (from && to && Math.abs(r - from[0]) <= 2 && Math.abs(g - from[1]) <= 2 && Math.abs(bl - from[2]) <= 2) {
        d[i] = to[0]; d[i + 1] = to[1]; d[i + 2] = to[2]; d[i + 3] = Math.round(d[i + 3]! * fx.clrChange!.toAlpha);
        continue;
      }
      if (b || c) {
        // contraste em volta do meio-tom e o brilho escalado por (1+k)/2 (medido, syn-lum:
        // o pivô do contraste anda b/2 com o brilho; k = 1/(1−c) se c ≥ 0, senão 1+c)
        const adj = (v: number): number => ((v / 255 - 0.5) * k + 0.5 + b * (1 + k) / 2) * 255;
        r = adj(r); g = adj(g); bl = adj(bl);
      }
      // luminância Rec. 709 sobre o sRGB (sem linearizar) — medido no oráculo com
      // grayscl/biLevel/duotone: vermelho 54, verde 182, azul 18
      const y = (0.2126 * r + 0.7152 * g + 0.0722 * bl) / 255;
      if (fx.biLevel !== undefined) { r = g = bl = y >= fx.biLevel ? 255 : 0; }
      else if (d0 && d1) { r = d0[0] + (d1[0] - d0[0]) * y; g = d0[1] + (d1[1] - d0[1]) * y; bl = d0[2] + (d1[2] - d0[2]) * y; }
      else if (fx.gray) { r = g = bl = y * 255; }
      d[i] = r; d[i + 1] = g; d[i + 2] = bl;
    }
    c2.putImageData(id, 0, 0);
  } catch { return img; } // origem cruzada / canvas sem getImageData
  const out = cv as unknown as PaintImage;
  m.set(key, out);
  return out;
}

/** Replays path ops on a context (`ctx.beginPath()` is the caller's). */
export function replayPath(ctx: CanvasRenderingContext2D, ops: readonly PathOp[]): void {
  for (const p of ops) {
    switch (p.op) {
      case 'moveTo': ctx.moveTo(p.x, p.y); break;
      case 'lineTo': ctx.lineTo(p.x, p.y); break;
      case 'bezierTo': ctx.bezierCurveTo(p.x1, p.y1, p.x2, p.y2, p.x, p.y); break;
      case 'quadTo': ctx.quadraticCurveTo(p.x1, p.y1, p.x, p.y); break;
      case 'arc': ctx.arc(p.cx, p.cy, p.r, p.a0, p.a1, !!p.ccw); break;
      case 'arcTo': ctx.arcTo(p.x1, p.y1, p.x2, p.y2, p.r); break;
      case 'ellipse': ctx.ellipse(p.x, p.y, p.rx, p.ry, p.rot, p.a0, p.a1, !!p.ccw); break;
      case 'rect': ctx.rect(p.x, p.y, p.w, p.h); break;
      case 'close': ctx.closePath(); break;
    }
  }
}

const SRGB_LIN = new Float32Array(256).map((_, i) => { const v = i / 255; return v <= 0.04045 ? v / 12.92 : ((v + 0.055) / 1.055) ** 2.4; });
const LIN_SRGB = new Uint8ClampedArray(4096).map((_, i) => { const v = i / 4095; return Math.round(255 * (v <= 0.0031308 ? 12.92 * v : 1.055 * v ** (1 / 2.4) - 0.055)); });

/** Distância euclidiana ao quadrado (Felzenszwalb, 1D) — `f` = 0 fora, ∞ dentro. */
function edt1d(f: Float64Array, n: number, d: Float64Array, v: Int32Array, z: Float64Array): void {
  let k = 0;
  v[0] = 0; z[0] = -Infinity; z[1] = Infinity;
  for (let q = 1; q < n; q++) {
    let s = (f[q]! + q * q - (f[v[k]!]! + v[k]! * v[k]!)) / (2 * q - 2 * v[k]!);
    while (s <= z[k]!) { k--; s = (f[q]! + q * q - (f[v[k]!]! + v[k]! * v[k]!)) / (2 * q - 2 * v[k]!); }
    k++; v[k] = q; z[k] = s; z[k + 1] = Infinity;
  }
  k = 0;
  for (let q = 0; q < n; q++) { while (z[k + 1]! < q) k++; const dq = q - v[k]!; d[q] = dq * dq + f[v[k]!]!; }
}

/**
 * Sombreado do chanfro (orthographicFront): distância de cada pixel à borda da
 * forma → inclinação do perfil (bevelSlope) → normal (azimute na tela, ângulo
 * polar pela razão altura/largura) → luz medida: em luz linear cor·D + S. A face
 * plana também muda (D da face: threePt +17%, harsh −33%…).
 */
export function applyBevel(ctx: CanvasRenderingContext2D, e: BevelEffect & { dir?: string }, ops: readonly PathOp[],
  scratch: ScratchFactory, light: LightTables): void {
  const b = { w: e.w, h: e.h, prst: e.prst, rig: e.rig, dir: e.dir, mat: e.mat };
  const { x, y, w, h } = e.bx;
  const scale = e.scale;
  const tab = light.lightTable(b.rig, b.mat);
  if (!tab) return;
  const M = ctx.getTransform();
  const kpx = Math.sqrt(Math.abs(M.a * M.d - M.b * M.c)) || 1;
  const bw = b.w * scale * kpx; // largura do chanfro em px do dispositivo
  const hw = b.h / b.w;
  const cv = (ctx as { canvas?: { width: number; height: number } }).canvas;
  if (!cv || !(bw > 0.5)) return;
  const pts = [[x, y], [x + w, y], [x, y + h], [x + w, y + h]].map(([px, py]) => [M.a * px! + M.c * py! + M.e, M.b * px! + M.d * py! + M.f]);
  const x0 = Math.max(0, Math.floor(Math.min(...pts.map((p) => p[0]!))) - 1), y0 = Math.max(0, Math.floor(Math.min(...pts.map((p) => p[1]!))) - 1);
  const x1 = Math.min(cv.width, Math.ceil(Math.max(...pts.map((p) => p[0]!))) + 1), y1 = Math.min(cv.height, Math.ceil(Math.max(...pts.map((p) => p[1]!))) + 1);
  const W = x1 - x0, H = y1 - y0;
  if (W <= 2 || H <= 2 || W * H > 6e6) return;
  // máscara da forma (mesma geometria do preenchimento)
  const mc = scratch(W, H);
  const m2 = mc?.getContext('2d') as CanvasRenderingContext2D | null | undefined;
  if (!m2) return;
  m2.setTransform(M.a, M.b, M.c, M.d, M.e - x0, M.f - y0);
  m2.beginPath();
  replayPath(m2, ops);
  m2.fillStyle = '#fff';
  m2.fill(e.rule);
  const mask = m2.getImageData(0, 0, W, H).data;
  // distância (px) até fora da forma
  const N = Math.max(W, H) + 2, f = new Float64Array(N), dd = new Float64Array(N), vv = new Int32Array(N), zz = new Float64Array(N + 1);
  const D2 = new Float64Array(W * H);
  const INF = 1e20;
  for (let c = 0; c < W; c++) {
    f[0] = 0; for (let r = 0; r < H; r++) f[r + 1] = mask[(r * W + c) * 4 + 3]! > 127 ? INF : 0; f[H + 1] = 0;
    edt1d(f, H + 2, dd, vv, zz);
    for (let r = 0; r < H; r++) D2[r * W + c] = dd[r + 1]!;
  }
  const dist = new Float32Array(W * H);
  for (let r = 0; r < H; r++) {
    f[0] = 0; for (let c = 0; c < W; c++) f[c + 1] = D2[r * W + c]!; f[W + 1] = 0;
    edt1d(f, W + 2, dd, vv, zz);
    for (let c = 0; c < W; c++) dist[r * W + c] = Math.sqrt(dd[c + 1]!);
  }
  const img = ctx.getImageData(x0, y0, W, H), px = img.data;
  const L = new Float32Array(4), face = new Float32Array(4);
  light.lightFace(tab, face);
  for (let r = 0; r < H; r++) for (let c = 0; c < W; c++) {
    const i = r * W + c, a = mask[i * 4 + 3]! / 255;
    if (a <= 0) continue;
    const d = dist[i]!;
    let lt = face;
    if (d < bw) {
      // direção para FORA = −∇d (diferenças centrais)
      const dl = c > 0 ? dist[i - 1]! : 0, dr = c < W - 1 ? dist[i + 1]! : 0, du = r > 0 ? dist[i - W]! : 0, dn = r < H - 1 ? dist[i + W]! : 0;
      const gx = dl - dr, gy = du - dn;
      const th = (Math.atan2(gy, gx) * 180) / Math.PI;
      const ph = light.bevelPhi(b.prst, d / bw, hw);
      // inclinada para dentro: a normal aponta para o lado oposto
      light.lightAt(tab, b.dir, ph < 0 ? th + 180 : th, Math.abs(ph), L);
      lt = L;
    }
    const o = i * 4;
    for (let k = 0; k < 3; k++) {
      const v = SRGB_LIN[px[o + k]!]! * lt[k]! + lt[3]!;
      const nv = LIN_SRGB[Math.max(0, Math.min(4095, Math.round(v * 4095)))]!;
      px[o + k] = a >= 1 ? nv : Math.round(px[o + k]! + (nv - px[o + k]!) * a);
    }
  }
  ctx.putImageData(img, x0, y0);
}

/**
 * PATINVERT (destino XOR pincel) EXATO num retângulo, nos pixels do canvas. Os
 * exportadores de degradê do Office recortam o degradê no formato da figura
 * com XOR → figura preta → XOR de novo: fora da figura o destino volta
 * idêntico — com 'difference' sobrava resíduo. Sem recorte ativo e com
 * transformação só de escala/translação; senão devolve false (aproximação).
 */
export function xorRect(ctx: CanvasRenderingContext2D, rgb: readonly [number, number, number], x0: number, y0: number, x1: number, y1: number, clipped: boolean): boolean {
  if (clipped || typeof ctx.getImageData !== 'function') return false;
  const t = ctx.getTransform();
  if (t.b || t.c) return false;
  const W = ctx.canvas.width, H = ctx.canvas.height;
  const X0 = Math.max(0, Math.round(Math.min(x0 * t.a, x1 * t.a) + t.e)), X1 = Math.min(W, Math.round(Math.max(x0 * t.a, x1 * t.a) + t.e));
  const Y0 = Math.max(0, Math.round(Math.min(y0 * t.d, y1 * t.d) + t.f)), Y1 = Math.min(H, Math.round(Math.max(y0 * t.d, y1 * t.d) + t.f));
  if (X1 <= X0 || Y1 <= Y0) return true;
  const [r, g, b] = rgb;
  const img = ctx.getImageData(X0, Y0, X1 - X0, Y1 - Y0), d = img.data;
  for (let i = 0; i < d.length; i += 4) { d[i] = d[i]! ^ r; d[i + 1] = d[i + 1]! ^ g; d[i + 2] = d[i + 2]! ^ b; d[i + 3] = 255; }
  ctx.putImageData(img, X0, Y0);
  return true;
}
