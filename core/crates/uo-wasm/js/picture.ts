// The picture layer of the canvas painter over DECODED pages (uo-render frontier D): `canvas-picture.ts` of the frozen TS
// engine, line by line — Cortar para Forma, colour adjustments, soft edges, inner shadow, outline (all outside), glow, outer
// shadow and reflection composed on a layer in the image's frame (before the rotation) with masks, and the layer drawn
// rotated; the same for a shape with Shape Effects. Every `@universal-office/drawingml` call of the TS is a wasm export of
// the `RasterSource` (render.mjs `rasters()`); the browser compositing (drawImage, clip, getImageData loops, the typed-array
// stores) stays here verbatim. Layers are cached per image + look + resolution (caps in `PaintCaches`).
//
// Erasable TypeScript; imports types only.

import type { ImageItem, ShapeItem, Tables } from './gen/render.ts';
import type { PaintPage } from './paint.ts';

type AnyCanvas = HTMLCanvasElement | OffscreenCanvas;
type Ctx2D = CanvasRenderingContext2D | OffscreenCanvasRenderingContext2D;

function canvas(w: number, h: number): AnyCanvas {
  if (typeof OffscreenCanvas !== 'undefined') return new OffscreenCanvas(w, h);
  const c = document.createElement('canvas');
  c.width = w; c.height = h;
  return c;
}
const ctx2d = (c: AnyCanvas): Ctx2D => c.getContext('2d') as Ctx2D;

export interface Layer {
  c: AnyCanvas;
  /** canto da camada relativo ao canto da caixa da imagem (pt) */
  x0: number; y0: number;
  /** pixels por ponto */
  ppt: number;
}

/** The per-layout part of the caches (`PaintCaches.of(owner)`). */
export interface OwnerPictureCaches {
  /** picture layers by image id, then the TS key */
  layers: Map<number, Map<string, Layer>>;
  /** colour-adjusted bitmaps by image id, then the TS key */
  adjusted: Map<number, Map<string, CanvasImageSource>>;
}

/** What the picture painter needs of `PaintCaches` (structural: paint.ts imports this file, not the reverse). */
export interface PictureCaches {
  readonly capShape: number;
  readonly capPictureImages: number;
  readonly capPicturePerImage: number;
  readonly capAdjustedPerImage: number;
  /** shape-effect layers by the TS key (module level in the TS) */
  readonly shape: Map<string, Layer>;
  of(owner: object): OwnerPictureCaches;
}

/** Precisa do pintor da imagem formatada? (senão o caminho rápido de sempre) */
export function needsPicturePainter(it: ImageItem, tables: Tables): boolean {
  // (recorte com giro/espelho também: o quadro visível gira junto com a imagem)
  return !!it.pic || !!tables.images[it.image]?.effects || !!(it.crop && (it.crop.l || it.crop.t || it.crop.r || it.crop.b) && (it.rot || it.flipH || it.flipV));
}

function tracePath(c: Ctx2D, polys: readonly ArrayLike<number>[]): void {
  c.beginPath();
  for (const p of polys) {
    c.moveTo(p[0]!, p[1]!);
    for (let i = 2; i < p.length; i += 2) c.lineTo(p[i]!, p[i + 1]!);
    c.closePath();
  }
}

/** Pinta uma máscara colorida numa camada (the bytes are the export's `coloredMask`, the store of `ImageData`). */
function maskToCanvas(p: PaintPage, m: Float32Array, w: number, h: number, hex: string, alpha: number): AnyCanvas {
  const c = canvas(w, h);
  const x = ctx2d(c);
  const img = x.createImageData(w, h);
  const rgba = p.rasters.coloredMask(m, w, h, hex, alpha);
  if (rgba) img.data.set(rgba);
  x.putImageData(img, 0, 0);
  return c;
}

const asBytes = (d: Uint8ClampedArray): Uint8ClampedArray => d;

/** Bitmap com os ajustes de cor do a:blip aplicados (em cache por imagem). */
function adjustedBitmap(p: PaintPage, index: number, it: ImageItem, bmp: CanvasImageSource & { width: number; height: number }, caches: PictureCaches): CanvasImageSource {
  const look = it.pic, fx = p.tables.images[it.image]?.effects;
  if (!fx && look?.alpha === undefined && !look?.duotone && !look?.clrChange) return bmp;
  const key = JSON.stringify([fx, look?.alpha, look?.duotone, look?.clrChange]);
  const oc = caches.of(p.owner);
  let m = oc.adjusted.get(it.image);
  if (!m) { m = new Map(); oc.adjusted.set(it.image, m); }
  const hit = m.get(key);
  if (hit) return hit;
  const c = canvas(Math.max(1, bmp.width), Math.max(1, bmp.height));
  const x = ctx2d(c);
  x.drawImage(bmp, 0, 0);
  const img = x.getImageData(0, 0, c.width, c.height);
  const out = p.rasters.pictureAdjust(p.index, index, asBytes(img.data));
  if (out) img.data.set(out);
  x.putImageData(img, 0, 0);
  if (m.size >= caches.capAdjustedPerImage) m.clear();
  m.set(key, c as CanvasImageSource);
  return c as CanvasImageSource;
}

function buildLayer(p: PaintPage, index: number, it: ImageItem, bmp: CanvasImageSource & { width: number; height: number }, ppt0: number, caches: PictureCaches): Layer {
  const look = it.pic;
  const R = p.rasters;
  const w = it.w / 20, h = it.h / 20;
  const bb = R.pictureBounds(p.index, index) ?? { x0: 0, y0: 0, x1: w, y1: h };
  let ppt = ppt0;
  const area = (bb.x1 - bb.x0) * (bb.y1 - bb.y0) * ppt * ppt;
  if (area > 12e6) ppt *= Math.sqrt(12e6 / area);
  const W = Math.max(1, Math.ceil((bb.x1 - bb.x0) * ppt)), H = Math.max(1, Math.ceil((bb.y1 - bb.y0) * ppt));
  const ox = -bb.x0 * ppt, oy = -bb.y0 * ppt;
  const polys = R.picturePolys(p.index, index, ppt, ox, oy) ?? [];

  // a imagem (recorte, espelho, ajustes) recortada pela forma
  const pic = canvas(W, H);
  const pc = ctx2d(pic);
  pc.save();
  tracePath(pc, polys);
  pc.clip();
  const crop = it.crop ?? { l: 0, t: 0, r: 0, b: 0 };
  const fw = w / Math.max(0.01, 1 - crop.l - crop.r), fh = h / Math.max(0.01, 1 - crop.t - crop.b);
  pc.translate(ox + (w / 2) * ppt, oy + (h / 2) * ppt);
  pc.scale(it.flipH ? -1 : 1, it.flipV ? -1 : 1);
  const src = adjustedBitmap(p, index, it, bmp, caches);
  pc.imageSmoothingEnabled = true;
  pc.imageSmoothingQuality = 'high';
  pc.drawImage(src, (-w / 2 - crop.l * fw) * ppt, (-h / 2 - crop.t * fh) * ppt, fw * ppt, fh * ppt);
  pc.restore();
  // chanfro: a luz 3D sobre a imagem dentro da forma
  if (look?.bevel) {
    const img = pc.getImageData(0, 0, W, H);
    const shaded = R.pictureBevel(p.index, index, ppt, ox, oy, W, H, asBytes(img.data));
    if (shaded) img.data.set(shaded);
    pc.putImageData(img, 0, 0);
  }

  // silhueta: a forma (e a transparência da imagem)
  const sil = R.pictureSilhouette(p.index, index, ppt, ox, oy, W, H) ?? new Float32Array(W * H);
  if (look?.softEdgePt || look?.shadow || look?.glow) {
    const a = pc.getImageData(0, 0, W, H).data;
    for (let i = 0; i < sil.length; i++) sil[i] = Math.min(sil[i]!, a[i * 4 + 3]! / 255 / (look?.alpha ?? 1));
  }
  const box = { x: ox, y: oy, w: w * ppt, h: h * ppt };

  // bordas suaves: o alfa da forma inteira (imagem + contorno)
  const shape = canvas(W, H);
  const sc = ctx2d(shape);
  sc.drawImage(pic, 0, 0);
  if (look?.shadow?.kind === 'inner') {
    const s = look.shadow;
    const m = R.innerShadow(sil, W, H, s.blurPt, s.distPt, s.dirDeg, ppt);
    if (m) sc.drawImage(maskToCanvas(p, m, W, H, s.color, s.alpha), 0, 0);
  }
  if (look?.line && look.line.widthPt > 0) {
    // contorno todo do lado de fora: traço de 2w no contorno e a parte de dentro apagada
    const ln = look.line;
    const lc = canvas(W, H), lx = ctx2d(lc);
    tracePath(lx, polys);
    lx.lineWidth = 2 * ln.widthPt * ppt;
    lx.lineJoin = ln.join === 'round' ? 'round' : ln.join === 'bevel' ? 'bevel' : 'miter';
    lx.lineCap = ln.cap === 'rnd' ? 'round' : ln.cap === 'sq' ? 'square' : 'butt';
    const dash = ln.dash ? R.dash(ln.dash) : undefined;
    if (dash) lx.setLineDash(dash.map((v) => v * ln.widthPt * ppt));
    lx.strokeStyle = `#${ln.color}`;
    lx.globalAlpha = ln.alpha;
    lx.stroke();
    lx.globalAlpha = 1;
    lx.globalCompositeOperation = 'destination-out';
    tracePath(lx, polys);
    lx.fill();
    sc.drawImage(lc, 0, 0);
  }
  if (look?.softEdgePt) {
    const m = R.softEdge(sil, W, H, look.softEdgePt, ppt);
    const img = sc.getImageData(0, 0, W, H);
    if (m) for (let i = 0; i < m.length; i++) img.data[i * 4 + 3] = Math.round(img.data[i * 4 + 3]! * m[i]!);
    sc.putImageData(img, 0, 0);
  }

  // camada final: sombra, brilho, reflexo e a forma
  const out = canvas(W, H);
  const o = ctx2d(out);
  const s = look?.shadow;
  if (s?.kind === 'outer') {
    // sem rotWithShape, o deslocamento é na página: volta ao referencial da imagem
    const dir = s.rotWithShape === false || s.rotWithShape === undefined ? s.dirDeg - (it.rot ?? 0) : s.dirDeg;
    const m = R.outerShadow(p.index, index, sil, W, H, box, dir, ppt);
    if (m) o.drawImage(maskToCanvas(p, m, W, H, s.color, s.alpha), 0, 0);
  }
  if (look?.glow) {
    const m = R.glow(sil, W, H, look.glow.radPt, ppt);
    if (m) o.drawImage(maskToCanvas(p, m, W, H, look.glow.color, look.glow.alpha), 0, 0);
  }
  const r = look?.reflection;
  if (r) {
    // cópia espelhada da forma, alfa em degradê, desfocada, a `dist` na direção `dir`
    const rc = canvas(W, H), rx = ctx2d(rc);
    const a = (r.dirDeg * Math.PI) / 180;
    const dx = r.distPt * Math.cos(a) * ppt, dy = r.distPt * Math.sin(a) * ppt;
    rx.translate(dx, 2 * (oy + h * ppt) + dy);
    rx.scale(1, -1);
    rx.drawImage(shape, 0, 0);
    const img = rx.getImageData(0, 0, W, H);
    const top = oy + h * ppt + dy;
    reflect(p, index, img, W, H, top, h * ppt, r.blurPt, ppt);
    rx.setTransform(1, 0, 0, 1, 0, 0);
    rx.putImageData(img, 0, 0);
    o.drawImage(rc, 0, 0);
  }
  o.drawImage(shape, 0, 0);
  return { c: out, x0: bb.x0, y0: bb.y0, ppt };
}

/** The alpha gradient of the reflection and its blur, on the mirrored copy's pixels (canvas-picture.ts:240-262, 334-358). */
function reflect(p: PaintPage, index: number, img: ImageData, W: number, H: number, top: number, hPpt: number, blurPt: number, ppt: number): void {
  const alphas = p.rasters.reflectionAlphas(p.index, index, top, hPpt, H);
  for (let y = 0; y < H; y++) {
    const al = alphas ? alphas[y]! : 0;
    for (let x = 0; x < W; x++) img.data[(y * W + x) * 4 + 3] = Math.round(img.data[(y * W + x) * 4 + 3]! * al);
  }
  if (blurPt > 0) {
    // desfoque das 4 componentes (pré-multiplicadas) com o mesmo σ = blurRad/3
    const ch = [0, 1, 2, 3].map((k) => {
      const m = new Float32Array(W * H);
      for (let i = 0; i < m.length; i++) m[i] = (k === 3 ? img.data[i * 4 + 3]! : img.data[i * 4 + k]! * img.data[i * 4 + 3]! / 255) / 255;
      return p.rasters.gaussianBlur(m, W, H, (blurPt / 3) * ppt);
    });
    if (ch.every((c) => c)) {
      for (let i = 0; i < W * H; i++) {
        const al = ch[3]![i]!;
        img.data[i * 4 + 3] = Math.round(al * 255);
        for (let k = 0; k < 3; k++) img.data[i * 4 + k] = al > 0 ? Math.round((ch[k]![i]! / al) * 255) : 0;
      }
    }
  }
}

/**
 * Forma com Efeitos de Forma no canvas: o corpo (preenchimento e contorno, desenhado por `drawBody` no referencial da
 * forma, antes do giro) numa camada; a silhueta é o alfa do corpo; sombra interna e bordas suaves sobre ele; sombra
 * externa, brilho e reflexo atrás — os mesmos modelos das imagens. `drawBody(c, s)`: desenha com 1 twip = `s` px, a
 * forma no canto (0, 0).
 */
export function paintShapeEffects(ctx: CanvasRenderingContext2D, p: PaintPage, index: number, it: ShapeItem, caches: PictureCaches, drawBody: (c: Ctx2D, scale: number) => void): void {
  const fx = it.fx ?? {};
  const R = p.rasters;
  const m0 = ctx.getTransform();
  let ppt = Math.max(0.5, Math.min(8, Math.hypot(m0.a, m0.b) * 20));
  const w = it.w / 20, h = it.h / 20;
  // the TS key, with `style.cust` replaced by the geometry it names (an id is per layout) and the blip's size by the table's
  const { cust, ...styleRest } = it.style;
  const style = { ...styleRest, cust: cust === undefined ? undefined : p.tables.cust[cust] };
  const key = JSON.stringify([style, it.fill, it.stroke, it.grad, it.pattern, fx, it.w, it.h, it.blip ? (p.tables.images[it.blip.image]?.byteLength ?? 0) : 0, Math.round(ppt * 50)]);
  let layer = caches.shape.get(key);
  if (!layer) {
    const bb = R.pictureBounds(p.index, index) ?? { x0: 0, y0: 0, x1: w, y1: h };
    const area = (bb.x1 - bb.x0) * (bb.y1 - bb.y0) * ppt * ppt;
    if (area > 8e6) ppt *= Math.sqrt(8e6 / area);
    const W = Math.max(1, Math.ceil((bb.x1 - bb.x0) * ppt)), H = Math.max(1, Math.ceil((bb.y1 - bb.y0) * ppt));
    const ox = -bb.x0 * ppt, oy = -bb.y0 * ppt;
    // o corpo
    const body = canvas(W, H);
    const bc = ctx2d(body);
    bc.translate(ox, oy);
    drawBody(bc, ppt / 20);
    bc.setTransform(1, 0, 0, 1, 0, 0);
    const sil = new Float32Array(W * H);
    {
      const d = bc.getImageData(0, 0, W, H).data;
      for (let i = 0; i < sil.length; i++) sil[i] = d[i * 4 + 3]! / 255;
    }
    const box = { x: ox, y: oy, w: w * ppt, h: h * ppt };
    const s = fx.shadow;
    // sombra interna: dentro do preenchimento (sem preenchimento o Word não a mostra)
    if (s?.kind === 'inner' && (it.fill || it.grad || it.pattern || it.blip)) {
      const m = R.innerShadow(sil, W, H, s.blurPt, s.distPt, s.dirDeg, ppt);
      bc.globalCompositeOperation = 'source-atop';
      if (m) bc.drawImage(maskToCanvas(p, m, W, H, s.color, s.alpha) as CanvasImageSource, 0, 0);
      bc.globalCompositeOperation = 'source-over';
    }
    if (fx.softEdgePt) {
      const m = R.softEdge(sil, W, H, fx.softEdgePt, ppt);
      const img = bc.getImageData(0, 0, W, H);
      if (m) for (let i = 0; i < m.length; i++) img.data[i * 4 + 3] = Math.round(img.data[i * 4 + 3]! * m[i]!);
      bc.putImageData(img, 0, 0);
    }
    const out = canvas(W, H);
    const o = ctx2d(out);
    if (s?.kind === 'outer') {
      const dir = s.rotWithShape === false || s.rotWithShape === undefined ? s.dirDeg - (it.style.rot ?? 0) : s.dirDeg;
      const m = R.outerShadow(p.index, index, sil, W, H, box, dir, ppt);
      if (m) o.drawImage(maskToCanvas(p, m, W, H, s.color, s.alpha) as CanvasImageSource, 0, 0);
    }
    if (fx.glow) {
      const m = R.glow(sil, W, H, fx.glow.radPt, ppt);
      if (m) o.drawImage(maskToCanvas(p, m, W, H, fx.glow.color, fx.glow.alpha) as CanvasImageSource, 0, 0);
    }
    const r = fx.reflection;
    if (r) {
      const rc = canvas(W, H), rx = ctx2d(rc);
      const a = (r.dirDeg * Math.PI) / 180;
      const dx = r.distPt * Math.cos(a) * ppt, dy = r.distPt * Math.sin(a) * ppt;
      rx.translate(dx, 2 * (oy + h * ppt) + dy);
      rx.scale(1, -1);
      rx.drawImage(body as CanvasImageSource, 0, 0);
      rx.setTransform(1, 0, 0, 1, 0, 0);
      const img = rx.getImageData(0, 0, W, H);
      const top = oy + h * ppt + dy;
      reflect(p, index, img, W, H, top, h * ppt, r.blurPt, ppt);
      rx.putImageData(img, 0, 0);
      o.drawImage(rc as CanvasImageSource, 0, 0);
    }
    o.drawImage(body as CanvasImageSource, 0, 0);
    layer = { c: out, x0: bb.x0, y0: bb.y0, ppt };
    if (caches.shape.size > caches.capShape) caches.shape.clear();
    caches.shape.set(key, layer);
  }
  ctx.save();
  ctx.translate(it.x + it.w / 2, it.y + it.h / 2);
  if (it.style.rot) ctx.rotate((it.style.rot * Math.PI) / 180);
  const sc = 20 / layer.ppt;
  ctx.drawImage(layer.c as CanvasImageSource, layer.x0 * 20 - it.w / 2, layer.y0 * 20 - it.h / 2, layer.c.width * sc, layer.c.height * sc);
  ctx.restore();
}

/** Desenha a imagem formatada no contexto (espaço de TWIPS, como paintItems). */
export function paintPicture(ctx: CanvasRenderingContext2D, p: PaintPage, index: number, it: ImageItem, bmp: CanvasImageSource, caches: PictureCaches): void {
  const b = bmp as CanvasImageSource & { width: number; height: number };
  const m = ctx.getTransform();
  // pixels por ponto no dispositivo (20 twips/pt)
  const ppt = Math.max(0.5, Math.min(12, Math.hypot(m.a, m.b) * 20));
  const key = JSON.stringify([it.pic, it.crop, it.flipH, it.flipV, it.w, it.h, it.rot ?? 0, Math.round(ppt * 50)]);
  const oc = caches.of(p.owner);
  let byImg = oc.layers.get(it.image);
  if (!byImg) {
    if (oc.layers.size >= caches.capPictureImages) oc.layers.clear();
    byImg = new Map();
    oc.layers.set(it.image, byImg);
  }
  let layer = byImg.get(key);
  if (!layer) {
    if (byImg.size > caches.capPicturePerImage) byImg.clear();
    layer = buildLayer(p, index, it, b, ppt, caches);
    byImg.set(key, layer);
  }
  ctx.save();
  // giro em volta do centro da caixa
  ctx.translate(it.x + it.w / 2, it.y + it.h / 2);
  if (it.rot) ctx.rotate((it.rot * Math.PI) / 180);
  const s = 20 / layer.ppt;
  ctx.drawImage(layer.c as CanvasImageSource, layer.x0 * 20 - it.w / 2, layer.y0 * 20 - it.h / 2, layer.c.width * s, layer.c.height * s);
  ctx.restore();
}
