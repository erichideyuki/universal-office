// The TS painter of DECODED pages (uo-render frontier D): the canvas painter of the frozen
// write/packages/docx-layout/src/canvas.ts (`paintItems`, `paintPageBackground`, `paintGlyphs`,
// `paintGlyphsFx`, `paintShape`) over the page message the Rust layout encodes (`js/gen/render.ts`),
// with the SAME draw calls in the same order and the same arithmetic. What the TS computed from
// the font and the geometry functions arrives as the encoder's hints (`outlines`, `remap`, `joins`,
// `parts`, `gradGeom`, `gradSamples`, `box`, `fxFillGeom`, `fxOutlineGeom`); the zoom-dependent
// rasters and the masks of the picture layer are the wasm exports (`RasterSource`, built by
// `render.mjs` `rasters()`). Record: core/docs/modules/uo-render.md "Milestone 1 — frontier D".
//
//   import { decodePage, newTables } from './gen/render.ts';
//   import { paintItems, paintPageBackground } from './paint.ts';
//   const tables = newTables();
//   const rasters = layout.rasters();                       // render.mjs rasters(layout.paint)
//   const page = decodePage(layout.encodePage(i), tables);  // pages in order: the tables grow with each message
//   const p = { page, tables, index: i, rasters, owner: layout };
//   paintPageBackground(ctx, p, env, { devicePxTw, screen, paper });   // ctx already in twips
//   paintItems(ctx, p, env, { alpha });
//
// Erasable TypeScript only (the same flags as the generated code). It imports TYPES from the generated
// codec and nothing from `write.mjs`/`render.mjs` (their `.d.ts` cannot be typed from a `.ts` file:
// TS7016): the handle layer arrives through the structural interfaces below.

import type { Face, GlyphRun, ImageItem, Item, Page, PageFill, ResolvedFill, ShapeItem, Tables, GradGeom } from './gen/render.ts';
import { needsPicturePainter, paintPicture, paintShapeEffects } from './picture.ts';
import type { Layer } from './picture.ts';

export type { Face, Item, Page, Tables };
export { needsPicturePainter };

/** What the host (editor/viewer) gives the painter: loaded fonts, decoded images, colours. */
export interface PaintEnv {
  /** the CSS family (quoted, with fallback) + weight/style of a face of the page's tables */
  fontOf(face: Face): { family: string; weight: 400 | 700; italic: boolean };
  /** the decoded bitmap of image `id` of the tables (null: still decoding or unavailable) */
  image(id: number): CanvasImageSource | null;
  /** RRGGBB -> CSS colour (the editor inverts the lightness in the dark theme) */
  color(hex: string): string;
}

export interface FxLayer { x0: number; y0: number; w: number; h: number; data: Uint8Array }
export interface PatternRaster { x: number; y: number; cellsW: number; cellsH: number; w: number; h: number; data: Uint8Array }
export interface Raster { w: number; h: number; data: Uint8Array }
export interface Tile { x: number; y: number; w: number; h: number }
export interface Bounds { x0: number; y0: number; x1: number; y1: number }

/**
 * The decoded results of the wasm paint exports (render.mjs `rasters()`): `null` for "nothing to paint"
 * (status 1) and for a limit (status 4: paint nothing, keep going). Pages and items are the indices of the
 * view the exports run on.
 */
export interface RasterSource {
  fxLayer(page: number, item: number, ppt: number): FxLayer | null;
  /** the layers of a run's effects at `ppt` (one as `fxLayer` while every blur is under the Word's step, else a
   *  layer per effect at its own pixels per point); a source without it paints `fxLayer` */
  fxLayerSet?(page: number, item: number, ppt: number): (FxLayer & { ppt: number })[] | null;
  pattern(page: number, item: number): PatternRaster | null;
  rectGradient(page: number, item: number): Raster | null;
  blipTiles(page: number, item: number): Tile[] | null;
  pageFillTileTw(page: number, devicePxTw: number): { w: number; h: number } | null;
  pageFillRaster(page: number): Raster | null;
  pictureBounds(page: number, item: number): Bounds | null;
  picturePolys(page: number, item: number, ppt: number, ox: number, oy: number): Float64Array[] | null;
  pictureSilhouette(page: number, item: number, ppt: number, ox: number, oy: number, w: number, h: number): Float32Array | null;
  pictureBevel(page: number, item: number, ppt: number, ox: number, oy: number, w: number, h: number, rgba: Uint8ClampedArray): Uint8Array | null;
  pictureAdjust(page: number, item: number, rgba: Uint8ClampedArray): Uint8Array | null;
  outerShadow(page: number, item: number, sil: Float32Array, w: number, h: number, box: { x: number; y: number; w: number; h: number }, dirDeg: number, ppt: number): Float32Array | null;
  reflectionAlphas(page: number, item: number, top: number, hPpt: number, rows: number): Float64Array | null;
  innerShadow(sil: Float32Array, w: number, h: number, blurPt: number, distPt: number, dirDeg: number, ppt: number): Float32Array | null;
  glow(sil: Float32Array, w: number, h: number, radPt: number, ppt: number): Float32Array | null;
  softEdge(sil: Float32Array, w: number, h: number, radPt: number, ppt: number): Float32Array | null;
  gaussianBlur(m: Float32Array, w: number, h: number, sigma: number): Float32Array | null;
  coloredMask(mask: Float32Array, w: number, h: number, hex: string, alpha: number): Uint8Array | null;
  dash(name: string): number[] | null;
}

/** A decoded page with what the painter needs around it. */
export interface PaintPage {
  page: Page;
  tables: Tables;
  /** the index of the page in the view the rasters run on */
  index: number;
  rasters: RasterSource;
  /** the layout/view object: the key of the per-layout caches (a WeakMap, so a freed layout drops its entries) */
  owner: object;
}

const HEX6 = /^[0-9A-Fa-f]{6}$/;

/** Tracejados de borda (tree.ts:272, Word: multiples of the thickness): a painter constant. */
const BORDER_DASHES: Record<string, (w: number) => number[]> = {
  bdrDot: (w) => [w, w], bdrDash: (w) => [3 * w, w], bdrDashDot: (w) => [3 * w, w, w, w],
  bdrDashDotDot: (w) => [3 * w, w, w, w, w, w], bdrDashSmall: (w) => [(25 * w) / 6, (5 * w) / 6],
};

/** Tracejado DrawingML (w14:prstDash, textfx.ts:217) in multiples of the thickness: a painter constant. */
function dashPattern(dash: string | undefined): number[] {
  switch (dash) {
    case 'dot': return [1, 1];
    case 'sysDot': return [1, 1];
    case 'sysDash': return [3, 1];
    case 'dash': return [4, 3];
    case 'dashDot': return [4, 3, 1, 3];
    case 'lgDash': return [8, 3];
    case 'lgDashDot': return [8, 3, 1, 3];
    case 'lgDashDotDot': return [8, 3, 1, 3, 1, 3];
    case 'sysDashDot': return [3, 1, 1, 1];
    case 'sysDashDotDot': return [3, 1, 1, 1, 1, 1];
    default: return [];
  }
}

/** the effect layers of a run, painted at `ppt` (each layer with the pixels per point it was drawn at) */
interface FxEntry { ppt: number; layers: { ppt: number; img: CanvasImageSource; x0: number; y0: number; w: number; h: number }[] }

/** The per-layout caches (dropped with the layout: a WeakMap on the owner). */
class OwnerCaches {
  /** text-effect layers by `page:item` */
  fx = new Map<string, FxEntry>();
  /** picture layers by image id, then the TS key */
  layers = new Map<number, Map<string, Layer>>();
  /** colour-adjusted bitmaps by image id, then the TS key */
  adjusted = new Map<number, Map<string, CanvasImageSource>>();
}

/**
 * The caches of the painter, every one with a cap (memory gate M5). The TS kept `fillTiles` and the
 * shape layers at module level (caps 32), the picture layers per image object (cap 8 each) and the effect
 * layers in a WeakMap per item; here the per-layout ones hang from the `owner` of the page.
 */
export class PaintCaches {
  readonly capFillTiles: number;
  readonly capShape: number;
  readonly capFx: number;
  readonly capPictureImages: number;
  readonly capPicturePerImage: number;
  readonly capAdjustedPerImage: number;
  /** page-fill source tiles by fill key (canvas.ts `fillTiles`: cleared when over 32) */
  readonly fillTiles = new Map<string, CanvasImageSource>();
  /** shape-effect layers by the TS key (canvas-picture.ts `shapeCache`: cleared when over 32) */
  readonly shape = new Map<string, Layer>();
  #owners = new WeakMap<object, OwnerCaches>();

  constructor(caps: { fillTiles?: number; shape?: number; fx?: number; pictureImages?: number; picturePerImage?: number; adjustedPerImage?: number } = {}) {
    this.capFillTiles = caps.fillTiles ?? 32;
    this.capShape = caps.shape ?? 32;
    this.capFx = caps.fx ?? 256;
    this.capPictureImages = caps.pictureImages ?? 64;
    this.capPicturePerImage = caps.picturePerImage ?? 8;
    this.capAdjustedPerImage = caps.adjustedPerImage ?? 16;
  }

  /** the caches of one layout */
  of(owner: object): OwnerCaches {
    let o = this.#owners.get(owner);
    if (!o) { o = new OwnerCaches(); this.#owners.set(owner, o); }
    return o;
  }

  /** drops every entry (the global ones and every layout's) */
  clear(): void {
    this.fillTiles.clear();
    this.shape.clear();
    this.#owners = new WeakMap();
  }

  /** the number of entries: the global caches and, with `owner`, that layout's */
  sizes(owner?: object): { fillTiles: number; shape: number; fx: number; pictureImages: number; picture: number; adjusted: number } {
    const o = owner ? this.#owners.get(owner) : undefined;
    let picture = 0, adjusted = 0;
    if (o) {
      for (const m of o.layers.values()) picture = Math.max(picture, m.size);
      for (const m of o.adjusted.values()) adjusted = Math.max(adjusted, m.size);
    }
    return { fillTiles: this.fillTiles.size, shape: this.shape.size, fx: o?.fx.size ?? 0, pictureImages: o?.layers.size ?? 0, picture, adjusted };
  }
}

export const defaultCaches = new PaintCaches();

type Ctx2D = CanvasRenderingContext2D | OffscreenCanvasRenderingContext2D;

function rgbaCanvas(rgba: Uint8Array, w: number, h: number): CanvasImageSource | null {
  const c = typeof OffscreenCanvas !== 'undefined' ? new OffscreenCanvas(w, h)
    : typeof document !== 'undefined' ? Object.assign(document.createElement('canvas'), { width: w, height: h }) : null;
  const x = c?.getContext('2d') as Ctx2D | null | undefined;
  if (!c || !x) return null;
  const id = x.createImageData(w, h);
  id.data.set(rgba);
  x.putImageData(id, 0, 0);
  return c as CanvasImageSource;
}

/** The key of a page fill in `fillTiles`: the TS key (canvas.ts:93-100) over the decoded fill. */
function gradKey(f: PageFill): string {
  return `g|${JSON.stringify([f.type, f.c1, f.c2, f.stops, f.angle, f.focus, f.focusPosition, f.gradientCenter])}`;
}

/**
 * Fundo da página (Cor da Página) in a context ALREADY in twips: the solid colour and the fill effect as tiles of
 * "device pixels" anchored at the page corner (canvas.ts `paintPageBackground`). `devicePxTw` = twips per CSS pixel at
 * the current scale (screen: 15/zoom). On screen (`screen`) Word shows the background only with
 * w:displayBackgroundShape; without it, `paper`.
 */
export function paintPageBackground(
  ctx: CanvasRenderingContext2D, p: PaintPage, env: PaintEnv,
  opts: { devicePxTw: number; screen: boolean; paper: string },
  caches: PaintCaches = defaultCaches,
): void {
  const page = p.page;
  const show = !opts.screen || !!page.backgroundShow;
  const f = show && page.backgroundFill !== undefined ? p.tables.pageFills[page.backgroundFill] : undefined;
  const framed = !!f && (f.type === 'frame' || f.type === 'tile');
  const solid = show && page.background && !framed ? `#${page.background}` : framed ? '#ffffff' : opts.paper;
  ctx.fillStyle = solid;
  ctx.fillRect(0, 0, page.widthTw, page.heightTw);
  if (!f) return;
  const tile = p.rasters.pageFillTileTw(p.index, opts.devicePxTw);
  if (!tile) return;
  let src: CanvasImageSource | null = null;
  let srcW = 1, srcH = 1;
  if (f.type === 'gradient' || f.type === 'gradientRadial') {
    const key = gradKey(f);
    const r = caches.fillTiles.get(key) ? null : p.rasters.pageFillRaster(p.index);
    src = caches.fillTiles.get(key) ?? null;
    if (!src && r) { src = rgbaCanvas(r.data, r.w, r.h); if (src) caches.fillTiles.set(key, src); }
    // the tile size of the source: the raster's (gradientTilePx), or the cached one's
    srcW = r ? r.w : tilePx(src, 1);
    srcH = r ? r.h : tilePx(src, 0);
  } else if (f.type === 'pattern') {
    const key = `p|${f.c1}|${f.c2}|${f.pattern?.join(',')}`;
    src = caches.fillTiles.get(key) ?? null;
    if (!src) {
      const r = p.rasters.pageFillRaster(p.index);
      if (r) { src = rgbaCanvas(r.data, r.w, r.h); if (src) caches.fillTiles.set(key, src); }
    }
    srcW = 8; srcH = 8;
  } else if (f.image !== undefined) {
    src = env.image(f.image);
    srcW = f.imagePx?.w ?? 1; srcH = f.imagePx?.h ?? 1;
  }
  if (!src) return;
  if (caches.fillTiles.size > caches.capFillTiles) caches.fillTiles.clear();
  const pat = ctx.createPattern(src, 'repeat');
  if (!pat) return;
  pat.setTransform(new DOMMatrix([tile.w / srcW, 0, 0, tile.h / srcH, 0, 0]));
  ctx.save();
  // padrão de bits: sem suavizar (como o Word, 1 bit = 1 pixel)
  if (f.type === 'pattern') ctx.imageSmoothingEnabled = false;
  ctx.fillStyle = pat;
  ctx.fillRect(0, 0, page.widthTw, page.heightTw);
  ctx.restore();
}

/** the pixel size of a cached tile canvas (`width` when `which` is 1, else `height`) */
function tilePx(src: CanvasImageSource | null, which: 0 | 1): number {
  const c = src as { width?: number; height?: number } | null;
  return (which === 1 ? c?.width : c?.height) ?? 1;
}

/**
 * Paints `items[from, to)` of the page in the context (already with the twip -> pixel scale applied).
 * `alpha` multiplies the opacity (fading headers/footers on screen). Unknown items are ignored.
 */
export function paintItems(
  ctx: CanvasRenderingContext2D, p: PaintPage, env: PaintEnv,
  opts: { from?: number; to?: number; alpha?: number } = {},
  caches: PaintCaches = defaultCaches,
): void {
  const items = p.page.items;
  const from = opts.from ?? 0;
  const to = Math.min(items.length, opts.to ?? items.length);
  const base = opts.alpha ?? 1;
  // pilha de save/restore dos recortes e transformações (q/Q no PDF)
  let depth = 0;
  ctx.save();
  ctx.globalAlpha = base;
  for (let i = from; i < to; i++) {
    const it = items[i]!;
    switch (it.t) {
      case 'glyphs':
        // efeitos de texto (w14): a camada dos efeitos atrás, depois o texto e o contorno
        if (it.fx !== undefined) { paintGlyphsFx(ctx, p, i, it, env, base, caches); break; }
        // texto semitransparente (w14:textFill, marca-d'água): a opacidade do run, como no PDF
        if (it.alpha !== undefined) ctx.globalAlpha = base * it.alpha;
        paintGlyphs(ctx, p, it, env);
        ctx.globalAlpha = base;
        break;
      case 'rect': {
        if (!HEX6.test(it.fill)) break;
        // realces/sombreamentos vêm um retângulo por caractere: os contíguos de mesma cor viram um só
        let w = it.w;
        while (i + 1 < to) {
          const n = items[i + 1]!;
          if (n.t !== 'rect' || n.fill !== it.fill || n.alpha !== it.alpha || n.y !== it.y || n.h !== it.h || Math.abs(n.x - (it.x + w)) > 0.5) break;
          w = n.x + n.w - it.x;
          i++;
        }
        let x = it.x, y = it.y, h = it.h;
        // retângulo fino (borda de 0,5 pt): o mesmo ajuste das linhas
        const m = ctx.getTransform();
        if (m.b === 0 && m.c === 0 && m.a > 0 && m.d > 0) {
          [x, w] = snapThin(x, w, m.a, m.e);
          [y, h] = snapThin(y, h, m.d, m.f);
        }
        ctx.globalAlpha = base * (it.alpha ?? 1);
        ctx.fillStyle = env.color(it.fill);
        ctx.fillRect(x, y, w, h);
        ctx.globalAlpha = base;
        break;
      }
      case 'line': {
        if (!HEX6.test(it.color)) break;
        const w = it.widthPt;
        const dash = it.dash === 'dotted' ? [w, w * 2] : it.dash === 'dashed' ? [w * 4, w * 3]
          : it.dash === 'dashDot' ? [w * 4, w * 2, w, w * 2] : it.dash === 'dashDotDot' ? [w * 4, w * 2, w, w * 2, w, w * 2]
          : it.dash ? BORDER_DASHES[it.dash]!(w) : [];
        let { x1, y1, x2, y2 } = it;
        let lw = w * 20;
        // ajuste de traço: linha fina horizontal/vertical vai para o centro do pixel com largura inteira >= 1 px
        const m = ctx.getTransform();
        if (m.b === 0 && m.c === 0) {
          const horiz = Math.abs(y1 - y2) < 0.01, vert = Math.abs(x1 - x2) < 0.01;
          const dev = lw * (horiz ? m.d : m.a);
          if ((horiz || vert) && dev < 3 && dev > 0) {
            const dw = Math.max(1, Math.round(dev));
            const snap = (v: number, s: number, o: number) => {
              const d = v * s + o;
              return ((dw % 2 ? Math.floor(d) + 0.5 : Math.round(d)) - o) / s;
            };
            if (horiz) { y1 = y2 = snap(y1, m.d, m.f); lw = dw / m.d; } else { x1 = x2 = snap(x1, m.a, m.e); lw = dw / m.a; }
          }
        }
        ctx.strokeStyle = env.color(it.color);
        ctx.lineWidth = lw;
        ctx.lineCap = 'butt';
        ctx.setLineDash(dash.map((d) => d * 20));
        ctx.beginPath();
        ctx.moveTo(x1, y1);
        ctx.lineTo(x2, y2);
        ctx.stroke();
        ctx.setLineDash([]);
        break;
      }
      case 'image': {
        const bmp = env.image(it.image);
        if (!bmp) break;
        // Formato da Imagem (forma, contorno, efeitos, ajustes): camada composta
        if (needsPicturePainter(it, p.tables)) { paintPicture(ctx, p, i, it, bmp, caches); break; }
        paintPlainImage(ctx, it, bmp);
        break;
      }
      case 'shape': paintShape(ctx, p, i, it, env, base, caches, false); break;
      case 'path': {
        const fill = it.fill && HEX6.test(it.fill) ? env.color(it.fill) : undefined;
        const stroke = it.stroke && HEX6.test(it.stroke.color) ? it.stroke : undefined;
        if (!fill && !stroke) break;
        ctx.beginPath();
        tracePath(ctx, it.cmds);
        if (fill) { ctx.fillStyle = fill; ctx.fill('nonzero'); }
        if (stroke) strokeWith(ctx, env.color(stroke.color), stroke.widthPt, stroke.dash);
        break;
      }
      case 'clip':
        if (it.op === 'push') {
          ctx.save();
          depth++;
          ctx.beginPath();
          ctx.rect(it.x ?? 0, it.y ?? 0, it.w ?? 0, it.h ?? 0);
          ctx.clip();
        } else if (depth > 0) { ctx.restore(); depth--; }
        break;
      case 'xform': {
        // transformação afim (texto vertical de célula): ponto' = (a·x + c·y + e, b·x + d·y + f)
        if (it.op === 'push' && it.m) {
          ctx.save();
          depth++;
          ctx.transform(it.m[0]!, it.m[1]!, it.m[2]!, it.m[3]!, it.m[4]!, it.m[5]!);
        } else if (depth > 0) { ctx.restore(); depth--; }
        break;
      }
      default:
        break;
    }
  }
  while (depth-- > 0) ctx.restore();
  ctx.restore();
}

function paintPlainImage(ctx: CanvasRenderingContext2D, it: ImageItem, bmp: CanvasImageSource): void {
  let x = it.x, y = it.y, w = it.w, h = it.h;
  const crop = it.crop;
  const cropped = !!crop && !!(crop.l || crop.t || crop.r || crop.b);
  ctx.save();
  if (cropped) {
    ctx.beginPath();
    ctx.rect(x, y, w, h);
    ctx.clip();
    const fw = w / Math.max(0.01, 1 - crop!.l - crop!.r);
    const fh = h / Math.max(0.01, 1 - crop!.t - crop!.b);
    x -= crop!.l * fw; y -= crop!.t * fh; w = fw; h = fh;
  }
  if (it.rot || it.flipH || it.flipV) {
    // gira (horário) e espelha em torno do centro do retângulo da imagem
    ctx.translate(x + w / 2, y + h / 2);
    if (it.rot) ctx.rotate((it.rot * Math.PI) / 180);
    ctx.scale(it.flipH ? -1 : 1, it.flipV ? -1 : 1);
    ctx.drawImage(bmp, -w / 2, -h / 2, w, h);
  } else ctx.drawImage(bmp, x, y, w, h);
  ctx.restore();
}

/** Faixa [v, v+len) (twips) com menos de 3 px de dispositivo (escala `s`, deslocamento `o`): largura inteira >= 1 px,
 *  centrada onde estava e alinhada à grade de pixels. */
export function snapThin(v: number, len: number, s: number, o: number): [number, number] {
  const dev = len * s;
  if (!(dev > 0 && dev < 3)) return [v, len];
  const dw = Math.max(1, Math.round(dev));
  const start = Math.round((v + len / 2) * s + o - dw / 2);
  return [(start - o) / s, dw / s];
}

/** Run com efeitos de texto: sombra/brilho/reflexo (raster atrás), preenchimento (sólido, degradê ou nenhum) e o
 *  contorno com espessura, tracejado, ponta e junção. */
function paintGlyphsFx(ctx: CanvasRenderingContext2D, p: PaintPage, index: number, it: GlyphRun, env: PaintEnv, base: number, caches: PaintCaches): void {
  const fx = p.tables.textFx[it.fx!];
  if (!fx) return;
  if (!it.glyphs.length) return;
  const m = ctx.getTransform();
  const ppt = Math.max(0.5, Math.hypot(m.a, m.b) * 20);
  if (fx.shadow || fx.glow || fx.reflection) {
    const oc = caches.of(p.owner);
    const key = `${p.index}:${index}`;
    let c = oc.fx.get(key);
    if (!c || Math.abs(c.ppt - ppt) > 0.01) {
      const set = p.rasters.fxLayerSet ? p.rasters.fxLayerSet(p.index, index, ppt) : null;
      const one = p.rasters.fxLayerSet ? null : p.rasters.fxLayer(p.index, index, ppt);
      const ls = set ?? (one ? [{ ...one, ppt }] : []);
      const layers: FxEntry['layers'] = [];
      for (const L of ls) {
        const img = rgbaCanvas(L.data, L.w, L.h);
        if (img) layers.push({ ppt: L.ppt, img, x0: L.x0, y0: L.y0, w: L.w, h: L.h });
      }
      c = { ppt, layers };
      if (!oc.fx.has(key) && oc.fx.size >= caches.capFx) oc.fx.clear();
      oc.fx.set(key, c);
    }
    if (c.layers.length) {
      ctx.globalAlpha = base;
      // a layer below the asked resolution (a large blur, the Word's rule) is scaled up smoothly, as the Word's
      // screen shows it; a layer at the asked resolution lands 1:1 and the smoothing setting does not touch it
      for (const l of c.layers) {
        if (l.ppt === ppt) { ctx.drawImage(l.img, l.x0, l.y0, (l.w / l.ppt) * 20, (l.h / l.ppt) * 20); continue; }
        const smooth = ctx.imageSmoothingEnabled;
        ctx.imageSmoothingEnabled = true;
        ctx.drawImage(l.img, l.x0, l.y0, (l.w / l.ppt) * 20, (l.h / l.ppt) * 20);
        ctx.imageSmoothingEnabled = smooth;
      }
    }
  }
  const paintOf = (f: ResolvedFill, geom: GradGeom | undefined): string | CanvasGradient | null => {
    if (f.kind === 'none') return null;
    if (f.kind === 'solid') return env.color(f.hex);
    if (!geom) return null;
    const cg = geom.kind === 'axial' ? ctx.createLinearGradient(geom.x0, geom.y0, geom.x1, geom.y1) : ctx.createRadialGradient(geom.x0, geom.y0, 0, geom.x0, geom.y0, geom.r);
    for (const st of f.stops) cg.addColorStop(Math.max(0, Math.min(1, st.pos)), env.color(st.hex));
    return cg;
  };
  // o preenchimento (o sólido opaco é a cor do item; com alfa, a opacidade dele)
  const fill = fx.fill ? paintOf(fx.fill, it.fxFillGeom) : HEX6.test(it.color) ? env.color(it.color) : null;
  const fillAlpha = fx.fill?.kind === 'solid' ? fx.fill.alpha : fx.fill?.kind === 'grad' ? (fx.fill.stops[0]?.alpha ?? 1) : it.alpha ?? 1;
  if (fill && fillAlpha > 0) {
    ctx.globalAlpha = base * fillAlpha;
    paintGlyphs(ctx, p, it, env, { fill });
  }
  const o = fx.outline;
  if (o) {
    const stroke = paintOf(o.fill, it.fxOutlineGeom);
    const a = o.fill.kind === 'solid' ? o.fill.alpha : 1;
    if (stroke && a > 0) {
      ctx.globalAlpha = base * a;
      const w = Math.max(o.widthPt * 20, 1 / Math.max(1e-6, Math.hypot(m.a, m.b)));
      paintGlyphs(ctx, p, it, env, { stroke: { style: stroke, width: w, dash: dashPattern(o.dash).map((d) => d * w), cap: o.cap, join: o.join } });
    }
  }
  ctx.globalAlpha = base;
}

type GlyphMode = { fill?: string | CanvasGradient; stroke?: { style: string | CanvasGradient; width: number; dash: number[]; cap: CanvasLineCap; join: CanvasLineJoin } };

/** Escritas sem composição contextual (latim, grego, cirílico, pontuação): o avanço de cada glifo é o da fonte, então
 *  dá para desenhar em sequência (canvas.ts `batchable`). */
function batchable(cp: number): boolean {
  return (cp >= 0x20 && cp < 0x0300) || (cp >= 0x0370 && cp < 0x0590) || (cp >= 0x2000 && cp < 0x2070 && cp !== 0x200c && cp !== 0x200d) || (cp >= 0x20a0 && cp < 0x20d0);
}

/**
 * `paintGlyphs` over the encoder's hints (the contract of e_support/shell.rs): a glyph with an entry in `outlines` is
 * drawn from the outline (never a batch start); `remap` gives the drawn code point of a batch start or lone glyph;
 * `joins[i]` (only when the context has `fontKerning`) appends glyph `i` (ZWNJ + its raw code point) to the batch
 * begun at the nearest earlier glyph.
 */
function paintGlyphs(ctx: CanvasRenderingContext2D, p: PaintPage, it: GlyphRun, env: PaintEnv, mode?: GlyphMode): void {
  if (!it.glyphs.length || (!mode && !HEX6.test(it.color))) return;
  const face = p.tables.faces[it.face];
  if (!face) return;
  const f = env.fontOf(face);
  ctx.font = `${f.italic ? 'italic ' : ''}${f.weight} ${it.sizePt * 20}px ${f.family}`;
  const color = mode?.fill ?? (mode?.stroke ? 'transparent' : env.color(it.color));
  ctx.fillStyle = color;
  ctx.textAlign = 'left';
  ctx.textBaseline = 'alphabetic';
  // negrito sintético: preenche + contorna (traço proporcional ao corpo, como o PDF)
  if (it.synthBold) {
    ctx.strokeStyle = color;
    ctx.lineWidth = (it.sizePt / 35) * 20;
    ctx.lineJoin = 'miter';
  }
  // contorno do texto (w14:textOutline): o traço pedido
  if (mode?.stroke) {
    ctx.strokeStyle = mode.stroke.style;
    ctx.lineWidth = mode.stroke.width;
    ctx.lineJoin = mode.stroke.join;
    ctx.lineCap = mode.stroke.cap;
    ctx.setLineDash(mode.stroke.dash);
  }
  const sx = it.scaleX ?? 1;
  // itálico sintético: cisalhamento 0,21 (no PDF y cresce para cima)
  const skew = it.synthItalic ? -0.21 : 0;
  const plain = sx === 1 && skew === 0;
  const gl = it.glyphs;
  const batch = plain && 'fontKerning' in ctx;
  if (batch) ctx.fontKerning = 'none';
  const upem = face.upem;
  const outlines = it.outlines && it.outlines.length ? new Map(it.outlines.map((o) => [o.index, o.cmds] as const)) : undefined;
  const remap = it.remap && it.remap.length ? new Map(it.remap.map((r) => [r.index, r.cp] as const)) : undefined;
  const joins = it.joins;
  for (let i = 0; i < gl.length; i++) {
    const g = gl[i]!;
    // glifo por índice (substituição OpenType, EMF): o contorno da fonte
    const ol = outlines?.get(i);
    if (ol) {
      const s = (it.sizePt * 20) / upem, gx = s * sx;
      const k = it.synthItalic ? 0.21 : 0;
      ctx.beginPath();
      for (const c of ol) {
        if (c.op === 'M') ctx.moveTo(g.x + c.x * gx + k * c.y * s, it.y - c.y * s);
        else if (c.op === 'L') ctx.lineTo(g.x + c.x * gx + k * c.y * s, it.y - c.y * s);
        else if (c.op === 'Q') ctx.quadraticCurveTo(g.x + c.x1 * gx + k * c.y1 * s, it.y - c.y1 * s, g.x + c.x * gx + k * c.y * s, it.y - c.y * s);
        else ctx.closePath();
      }
      if (mode?.stroke) ctx.stroke(); else ctx.fill();
      continue;
    }
    const cp = remap?.get(i) ?? g.cp;
    let ch = String.fromCodePoint(cp);
    if (batch && batchable(cp)) {
      while (joins?.[i + 1]) {
        ch += '\u200C' + String.fromCodePoint(gl[i + 1]!.cp);
        i++;
      }
    }
    const sk = mode?.stroke;
    if (plain) {
      if (!sk) ctx.fillText(ch, g.x, it.y);
      if (it.synthBold || sk) ctx.strokeText(ch, g.x, it.y);
    } else {
      ctx.save();
      ctx.translate(g.x, it.y);
      ctx.transform(sx, 0, skew, 1, 0, 0);
      if (!sk) ctx.fillText(ch, 0, 0);
      if (it.synthBold || sk) ctx.strokeText(ch, 0, 0);
      ctx.restore();
    }
  }
  if (mode?.stroke) ctx.setLineDash([]);
}

function strokeWith(ctx: Ctx2D, color: string, widthPt: number, dash?: string): void {
  const w = Math.max(0.25, widthPt);
  ctx.strokeStyle = color;
  // contorno nunca abaixo de 1 px do dispositivo (como os rasterizadores de PDF)
  const m = ctx.getTransform();
  const px = Math.hypot(m.a, m.b) || 1;
  ctx.lineWidth = Math.max(w * 20, 1 / px);
  ctx.lineCap = 'butt';
  ctx.lineJoin = 'miter';
  ctx.setLineDash(dash ? [w * 3 * 20, w * 2 * 20] : []);
  ctx.stroke();
  ctx.setLineDash([]);
}

type Cmd =
  | { op: 'M' | 'L'; x: number; y: number }
  | { op: 'C'; x1: number; y1: number; x2: number; y2: number; x: number; y: number }
  | { op: 'Z' };

function tracePath(ctx: Ctx2D, cmds: readonly Cmd[]): void {
  for (const c of cmds) {
    if (c.op === 'M') ctx.moveTo(c.x, c.y);
    else if (c.op === 'L') ctx.lineTo(c.x, c.y);
    else if (c.op === 'C') ctx.bezierCurveTo(c.x1, c.y1, c.x2, c.y2, c.x, c.y);
    else ctx.closePath();
  }
}

const hex2 = (n: number): string => n.toString(16).padStart(2, '0').toUpperCase();

/** RGBA -> canvas (padrão; as cores passam pelo env.color do tema escuro). */
function rgbaCanvasThemed(data: Uint8Array, w: number, h: number, env: PaintEnv, pat: { fg: string; bg: string } | null): HTMLCanvasElement | OffscreenCanvas | null {
  const c: HTMLCanvasElement | OffscreenCanvas = typeof OffscreenCanvas !== 'undefined' ? new OffscreenCanvas(w, h) : Object.assign(document.createElement('canvas'), { width: w, height: h });
  const x = c.getContext('2d') as Ctx2D | null;
  if (!x) return null;
  const img = x.createImageData(w, h);
  img.data.set(data);
  // cores trocadas pelo tema escuro do editor (env.color): recolore frente/fundo
  const fg = pat ? env.color(pat.fg) : '', bg = pat ? env.color(pat.bg) : '';
  if (pat && (fg !== `#${pat.fg}` || bg !== `#${pat.bg}`)) {
    const parse = (s: string): number[] | null => { const m = /^#?([0-9a-f]{6})$/i.exec(s); return m ? [0, 2, 4].map((i) => parseInt(m[1]!.slice(i, i + 2), 16)) : null; };
    const f = parse(fg), b = parse(bg), f0 = parse(pat.fg)!;
    if (f && b) for (let i = 0; i < img.data.length; i += 4) { const on = img.data[i] === f0[0] && img.data[i + 1] === f0[1] && img.data[i + 2] === f0[2]; const c2 = on ? f : b; img.data[i] = c2[0]!; img.data[i + 1] = c2[1]!; img.data[i + 2] = c2[2]!; }
  }
  x.putImageData(img, 0, 0);
  return c;
}

/**
 * Forma (wps/VML) over the hints: the parts (`parts`), the gradient geometry and 64 samples (`gradGeom`, `gradSamples`) and
 * the rasters of the pattern / rectangular gradient / blip tiles (`rasters`). A shape with effects goes through
 * `paintShapeEffects` with the body painted by THIS item in `local` mode: its hints are already those of the local item
 * (x = y = 0, no rotation: E-D2), so only the shadow and the effects are skipped (canvas.ts:430).
 */
export function paintShape(ctx: Ctx2D, p: PaintPage, index: number, it: ShapeItem, env: PaintEnv, base: number, caches: PaintCaches, local: boolean): void {
  const st = it.style;
  // Efeitos de Forma: o corpo numa camada (no referencial da forma) com os efeitos
  if (!local && it.fx && st.geom !== 'line') {
    paintShapeEffects(ctx as CanvasRenderingContext2D, p, index, it, caches, (c, s) => { c.scale(s, s); paintShape(c, p, index, it, env, 1, caches, true); });
    return;
  }
  const fill = it.fill && HEX6.test(it.fill) ? env.color(it.fill) : undefined;
  const stroke = it.stroke && HEX6.test(it.stroke.color) ? it.stroke : undefined;
  if (!fill && !stroke && !it.grad && !it.pattern && !it.blip) return;
  const x = local ? 0 : it.x, y = local ? 0 : it.y, w = it.w, h = it.h;
  const rot = local ? 0 : (st.rot ?? 0);
  const parts = it.parts;
  const fillPart = (pt: (typeof parts)[number]): void => { ctx.beginPath(); tracePath(ctx, pt.cmds); ctx.fill(pt.evenOdd ? 'evenodd' : 'nonzero'); };
  const strokePart = (pt: (typeof parts)[number]): void => { ctx.beginPath(); tracePath(ctx, pt.cmds); strokeWith(ctx, env.color(stroke!.color), stroke!.widthPt, stroke!.dash); };
  if (!local && it.shadow && HEX6.test(it.shadow.color)) {
    ctx.save();
    ctx.translate(it.shadow.dx, it.shadow.dy);
    ctx.globalAlpha = base * (it.shadow.alpha ?? 1);
    ctx.fillStyle = env.color(it.shadow.color);
    for (const pt of parts) {
      if (pt.fill && (fill || it.grad || it.pattern || it.blip)) fillPart(pt);
      if (pt.stroke && stroke) { ctx.beginPath(); tracePath(ctx, pt.cmds); strokeWith(ctx, env.color(it.shadow.color), stroke.widthPt, stroke.dash); }
    }
    ctx.restore();
  }
  ctx.globalAlpha = base * (st.fillAlpha ?? 1);
  if (it.pattern || it.blip) {
    for (const pt of parts) {
      if (!pt.fill) continue;
      ctx.save();
      ctx.beginPath();
      tracePath(ctx, pt.cmds);
      ctx.clip(pt.evenOdd ? 'evenodd' : 'nonzero');
      if (it.pattern) {
        const pr = p.rasters.pattern(p.index, index);
        if (pr) {
          const c = rgbaCanvasThemed(pr.data, pr.w, pr.h, env, it.pattern);
          ctx.imageSmoothingEnabled = false;
          if (c) ctx.drawImage(c, pr.x, pr.y, pr.cellsW * 15, pr.cellsH * 15);
        }
      } else if (it.blip) {
        const bmp = env.image(it.blip.image);
        if (bmp) {
          ctx.globalAlpha = base * (it.blip.alpha ?? 1);
          for (const t of p.rasters.blipTiles(p.index, index) ?? []) ctx.drawImage(bmp, t.x, t.y, t.w, t.h);
        }
      }
      ctx.restore();
    }
  } else if (it.grad && !it.gradGeom) {
    // degradê em retângulos ("Do Centro"): raster no quadro, girado com a forma (hints.rs: `grad && !gradGeom` is `isRectGradient`)
    const r = p.rasters.rectGradient(p.index, index);
    const c = r ? rgbaCanvasThemed(r.data, r.w, r.h, env, null) : null;
    for (const pt of parts) {
      if (!pt.fill || !c) continue;
      ctx.save();
      ctx.beginPath();
      tracePath(ctx, pt.cmds);
      ctx.clip(pt.evenOdd ? 'evenodd' : 'nonzero');
      ctx.translate(x + w / 2, y + h / 2);
      if (rot) ctx.rotate((rot * Math.PI) / 180);
      ctx.scale(st.flipH ? -1 : 1, st.flipV ? -1 : 1);
      ctx.drawImage(c, -w / 2, -h / 2, w, h);
      ctx.restore();
    }
  } else if (it.grad) {
    const g = it.gradGeom!;
    const smp = it.gradSamples;
    const cg = g.kind === 'axial' ? ctx.createLinearGradient(g.x0, g.y0, g.x1, g.y1) : ctx.createRadialGradient(g.x0, g.y0, 0, g.x0, g.y0, g.r);
    if (smp) for (let i = 0; i < 64; i++) cg.addColorStop(i / 63, env.color(hex2(smp[i * 3] ?? 0) + hex2(smp[i * 3 + 1] ?? 0) + hex2(smp[i * 3 + 2] ?? 0)));
    ctx.fillStyle = cg;
    for (const pt of parts) if (pt.fill) fillPart(pt);
  } else if (fill) {
    ctx.fillStyle = fill;
    for (const pt of parts) if (pt.fill) fillPart(pt);
  }
  if (stroke) {
    ctx.globalAlpha = base * (st.lineAlpha ?? 1);
    for (const pt of parts) if (pt.stroke) strokePart(pt);
  }
  ctx.globalAlpha = base;
}
