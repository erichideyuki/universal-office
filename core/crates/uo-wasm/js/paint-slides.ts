// The TS interpreter of the Slides display list (uo-render frontier R-S3): executes a decoded `SlidesList`
// (`js/gen/slides.ts`) plus the per-frame `SlotFrame` (`js/gen/slots.ts`) on a `CanvasRenderingContext2D`, issuing the
// SAME canvas calls the frozen TS painter (`slides/packages/slides-core/src/render.ts`) issues for the same slide, in the
// same order with the same arguments (the draw-op stream is the primary parity gate: record-ctx.mjs in uo-pptx).
// What the TS computed from model data arrives precomputed in the list; what it computed from canvas state (the current
// transform, device measureText, pixels, capability tests, layer availability) is resolved HERE. The shell code that
// reads pixels or measures on the device is MOVED from the frozen TS: `slides-effects.ts` (bevel, recolour, raster ops),
// `slides-warp.ts` (WordArt strips), `slides-reveal.ts` (reveal masks). Record: core/docs/modules/uo-render.md "R-S3/R-S4".
//
//   import { decodeSlidesList, newTables } from './gen/slides.ts';
//   import { SlidesPainter } from './paint-slides.ts';
//   const tables = newTables();
//   const list = decodeSlidesList(bytes, tables);             // messages of one stream in order: the tables grow
//   const painter = new SlidesPainter(env);                   // owns the caches (derived images, bitmaps, layer pool)
//   painter.paint(ctx, list, tables, frame);                  // `frame` = decodeSlotFrame(...) or undefined (static)
//
// Rules of the interpretation that the schema leaves to the painter (each recorded in the doc):
//  - slots: `SlotBegin` is a `save()`, `SlotEnd` the matching `restore()`; the producer emits no Save/Restore of its own around a
//    slotted part. A hidden value (or `applyPart` false) restores and skips to the item after the matching `SlotEnd`. Where the TS
//    tests the state before its `save()` (B4b: a hidden shape, an invisible paragraph or unit) the painter skips without saving; a
//    `unitGate` slot never saves (it only gates the bullet). A shape slot whose frame carries a replacement paints the
//    replacement's items (its own list and values) instead of its range. `globalAlpha` items inside a slot are relative to the
//    alpha the slot's value multiplied in (the producer wrote them for the static paint). The soft-reveal layer of a shape (a
//    chart part) is the `layerBegin` with id 15 (14) right after its slot's `slotBegin`.
//  - `mediaFrame`: the host's frame replaces the poster when opaque and fades over it when 0 < alpha < 1.
//  - layers: `LayerBegin.fallbackEnd` is the end of the else block that FOLLOWS the matching `LayerEnd`: with a layer, `LayerEnd`
//    continues at `fallbackEnd`; without one (or, for the layer item right after a shape slot's `SlotBegin`, with no soft
//    reveal pending) the painter continues just after the matching `LayerEnd`, i.e. runs the else block.
//  - guarded ranges: a `save` at relative depth 1 (a shape under the slide clip) opens a guarded range like the TS `guarded`: a throw
//    inside restores the pending saves, discards the layers acquired in it and resumes after the range's closing `restore`.
// Erasable TypeScript only (the same flags as the generated code).

import type {
  Effect, FaceKey, ImageSrc, Paint, PathOp, Rect4, SItem, SlidesList, SlotDef, Span, Stop, Tables, TextRun, WarpEffect, WarpLine as WireWarpLine,
  WarpPaint, MetafileRopEffect, TileParams,
} from './gen/slides.ts';
import type { PartState, SlotFrame, SlotValue } from './gen/slots.ts';
import { applyReveal, applySoftReveal, softRevealFilter } from './slides-reveal.ts';
import { decodeFxRecipe, replayRecipe } from './meta-player.ts';
import type { MetaCanvas } from './meta-player.ts';
import { paintWarpedText } from './slides-warp.ts';
import type { PatternRowsOf, WarpLine, WarpRun } from './slides-warp.ts';
import { applyBevel, fxImage, imgH, imgW, pathGradient, replayPath, scratchCanvas, tilePattern, toTsFx, xorRect } from './slides-effects.ts';
import type { LightTables, PaintImage, ScratchFactory } from './slides-effects.ts';

export type { PaintImage, LightTables, ScratchFactory, PatternRowsOf };

/** What the host (component, test recorder) gives the painter. */
export interface SlidesEnv {
  /** the CSS font-family stack of a face (T2); default: the TS's fixed stack */
  fontStack?(family: string, bold: boolean, italic: boolean): string;
  /** the decoded picture of `tables.media[index]` (null/undefined: still decoding or unavailable: the draw is skipped) */
  media(index: number): PaintImage | null | undefined;
  /** the RGBA bytes (`w * h * 4`) of `tables.bitmaps[index]` (the wasm side holds them by handle) */
  bitmap(index: number): Uint8Array | null | undefined;
  /**
   * B5b: the recipe of `tables.bitmaps[index]` when it is a metafile or PICT picture (`SlidesResources::is_recipe`): the canvas calls
   * of the TS rasterizer, replayed by `meta-player.ts` on a canvas of the bitmap's size; null/undefined = not a recipe (RGBA bytes)
   */
  recipe?(index: number): Uint8Array | null | undefined;
  /** the decoded image of a compressed EMF+ bitmap the recipe draws (prepared by the shell); null = not available */
  decodeBlob?(data: Uint8Array): CanvasImageSource | null | undefined;
  /** the canvas factory of the recipe replays (default: `createCanvas`; the parity harness gives them a recorder of their own) */
  metaCanvas?: ScratchFactory;
  /** the current video/GIF frame of a shape (`MediaFrame` items) */
  mediaFrame?(shape: number): { image: PaintImage; alpha: number } | null | undefined;
  /** the host's canvas factory (default as the TS: `OffscreenCanvas`, else a `<canvas>`) */
  createCanvas?: ScratchFactory;
  /** the 8x8 rows of a preset pattern, for patterned WordArt letters (uo-drawingml `patternRows`) */
  patternRows?: PatternRowsOf;
  /** the light tables of the bevel (uo-drawingml `light3d`); without them `Bevel` effects are skipped */
  light?: LightTables;
}

/** Caps of the painter's caches (M5): each entry is a canvas. */
export const MAX_DERIVED_CACHE = 256;
export const MAX_BITMAP_CACHE = 128;

/**
 * The canvas of a metafile replay: a `<canvas>` element, as the TS viewer's `warmImage` (`document.createElement('canvas')`)
 * gave `rasterizeSpecialImage`. Chromium anti-aliases an `OffscreenCanvas` differently from an element canvas (measured: the
 * b5b decks differed on path edges inside the pictures), so the element is the parity choice; no document (a worker, Node):
 * the scratch canvas.
 */
function elementCanvas(w: number, h: number): (HTMLCanvasElement | OffscreenCanvas) | undefined {
  const doc = (globalThis as { document?: Document }).document;
  if (!doc) return scratchCanvas(w, h);
  const c = doc.createElement('canvas'); c.width = w; c.height = h; return c;
}

const DEFAULT_FONT_STACK = (family: string): string => `'${family}', 'Aptos', Calibri, 'Liberation Sans', Arial, sans-serif`;

type LayerCtx = CanvasRenderingContext2D & { canvas: { width: number; height: number } };
const layerPool = new WeakMap<object, LayerCtx[]>();
const layerBusy = new WeakSet<object>();

/** Revelação com borda esfumada pendente (aplicada como máscara numa camada). */
interface SoftReveal { filter: string; q: number }

interface SlidePx { ox: number; oy: number; sw: number; sh: number }

/** A paint target: the main context or a layer, with the saves the items made on it and the clips of its save levels. */
interface Target { ctx: CanvasRenderingContext2D; saves: number; clips: number[]; layer: number }

interface SlotRun { def: SlotDef; soft: SoftReveal[]; begin: number; noSave?: boolean; target?: Target; k: number }

/** A shape re-emitted for this frame (`SlotFrame.replacements`, decoded): its list and the values of its own slots. */
export interface SlotReplacement { list: SlidesList; values: SlotValue[] }

interface LayerInfo { lc: LayerCtx; fallbackEnd: number; begin: number; soft?: SlotRun }

function at<T>(a: readonly T[], i: number, what: string): T {
  const v = a[i];
  if (v === undefined) throw new RangeError(`${what} ${i} out of range (${a.length})`);
  return v;
}

function spanOf<T>(a: readonly T[], s: Span, what: string): T[] {
  if (s.start + s.len > a.length || s.start < 0 || s.len < 0) throw new RangeError(`${what} span ${s.start}+${s.len} out of range (${a.length})`);
  return a.slice(s.start, s.start + s.len);
}

/** `ctx.font` for a face and a pixel size, as the TS `layoutFontStr`: `[italic ]{700|400} <size>px <stack>`. */
function fontStr(env: SlidesEnv, face: FaceKey, sizePx: number): string {
  const bold = !!face.bold, italic = !!face.italic;
  return `${italic ? 'italic ' : ''}${bold ? '700 ' : '400 '}${sizePx}px ${(env.fontStack ?? DEFAULT_FONT_STACK)(face.family, bold, italic)}`;
}

/**
 * `drawCellText` (text-metrics.ts:104-130) over a `GlyphRun`: complex scripts are one whole-string `fillText`; otherwise one
 * `fillText(ch, cx, y)` per non-space code point at the integer advances (stop when `cx > maxX`, skip `cx + w < minX`); a
 * mirrored (RTL) sheet draws the piece unmirrored in the mirrored box: `save`, `translate(x + W, 0)`, `scale(-1, 1)`, `restore`.
 */
function paintGlyphRun(c: CanvasRenderingContext2D, list: SlidesList, g: { text: Span; x: number; y: number; advances: Span; minX: number; maxX: number; width: number; complex?: boolean; mirrored?: boolean }): void {
  if (g.text.start + g.text.len > list.texts.length || g.advances.start + g.advances.len > list.advances.length) throw new RangeError('glyph run span out of range');
  const s = list.texts.slice(g.text.start, g.text.start + g.text.len);
  c.textAlign = 'left';
  if (g.mirrored) {
    c.save();
    c.translate(g.x + g.width, 0);
    c.scale(-1, 1);
    if (g.complex) c.fillText(s, 0, g.y);
    else { let cx = 0, k = g.advances.start; for (const ch of s) { if (ch !== ' ') c.fillText(ch, cx, g.y); cx += list.advances[k++] ?? 0; } }
    c.restore();
    return;
  }
  if (g.complex) { c.fillText(s, g.x, g.y); return; }
  let cx = g.x, k = g.advances.start;
  for (const ch of s) {
    const w = list.advances[k++] ?? 0;
    if (cx > g.maxX) break;
    if (cx + w >= g.minX && ch !== ' ') c.fillText(ch, cx, g.y);
    cx += w;
  }
}

/** `font()` of the chart engine (render.ts 177-180): `[italic ][bold ]<size.toFixed(2)>px <stack>` (not `fontStr`'s `700 <size>px`). */
function chartFontStr(env: SlidesEnv, face: FaceKey, sizePx: number): string {
  const bold = !!face.bold, italic = !!face.italic;
  return `${italic ? 'italic ' : ''}${bold ? 'bold ' : ''}${sizePx.toFixed(2)}px ${(env.fontStack ?? DEFAULT_FONT_STACK)(face.family, bold, italic)}`;
}

/**
 * Aplica o estado animado de uma parte ao contexto (`applyPart` of render.ts 116-140): opacidade, deslocamento /
 * escala / rotação em torno do centro da caixa e as máscaras de revelação dos filtros. false = invisível.
 * Adaptation: `st` is the wire `PartState`, `base` the slot's `base` rectangle, the filters carry `out` instead of the
 * transition string.
 */
function applyPart(ctx: CanvasRenderingContext2D, st: PartState, base: Rect4,
  x: number, y: number, w: number, h: number, m: SlidePx, baseRotRad = 0, soft?: SoftReveal[]): boolean {
  if (!st.visible || st.opacity <= 0) return false;
  const kx = (base.w > 0 ? st.w / base.w : 1) * st.sx;
  const ky = (base.h > 0 ? st.h / base.h : 1) * st.sy;
  if (!Number.isFinite(kx) || !Number.isFinite(ky) || Math.abs(kx) < 1e-4 || Math.abs(ky) < 1e-4) return false;
  ctx.globalAlpha = ctx.globalAlpha * st.opacity;
  const bcx = x + w / 2, bcy = y + h / 2;
  const ncx = m.ox + (st.x + st.mx) * m.sw, ncy = m.oy + (st.y + st.my) * m.sh;
  if (Math.abs(ncx - bcx) > 1e-3 || Math.abs(ncy - bcy) > 1e-3 || st.rot || Math.abs(kx - 1) > 1e-6 || Math.abs(ky - 1) > 1e-6) {
    ctx.translate(ncx, ncy);
    if (st.rot) ctx.rotate((st.rot * Math.PI) / 180);
    if (baseRotRad && (kx !== 1 || ky !== 1)) { ctx.rotate(baseRotRad); ctx.scale(kx, ky); ctx.rotate(-baseRotRad); }
    else ctx.scale(kx, ky);
    ctx.translate(-bcx, -bcy);
  }
  for (const f of st.filters) {
    const q = f.out ? 1 - f.p : f.p;
    // borda esfumada: quem chama pinta numa camada e aplica a máscara
    if (soft && q > 0 && q < 1 && softRevealFilter(f.filter)) { soft.push({ filter: f.filter, q }); continue; }
    ctx.globalAlpha = ctx.globalAlpha * applyReveal(ctx, f.filter, q, x, y, w, h);
    if (ctx.globalAlpha <= 0) return false;
  }
  return true;
}

/** The interpreter and its caches. One per host (component); `paint` is synchronous and reentrant-free. */
export class SlidesPainter {
  readonly env: SlidesEnv;
  private derived = new Map<string, PaintImage | null>();
  private bitmaps = new Map<string, PaintImage | null>();

  constructor(env: SlidesEnv) { this.env = env; }

  /** Drop the cached derived images and bitmap canvases (a new generation or a resize). */
  clear(): void { this.derived.clear(); this.bitmaps.clear(); }

  /** Entries held by the caches (for the memory tests). */
  cacheSizes(): { derived: number; bitmaps: number } { return { derived: this.derived.size, bitmaps: this.bitmaps.size }; }

  private scratch: ScratchFactory = (w, h) => (this.env.createCanvas ?? scratchCanvas)(w, h);

  // ------------------------------------------------------------------ layers (render.ts 145-200, copied; the canvas factory is the env's)
  /**
   * Camada do tamanho do canvas de `ctx`, limpa e com a MESMA transformação
   * (as coordenadas de pintura valem nas duas). null sem canvas/OffscreenCanvas
   * (ex.: testes) — quem chama cai no recorte seco.
   */
  private acquireLayer(ctx: CanvasRenderingContext2D): LayerCtx | null {
    const c = (ctx as { canvas?: { width: number; height: number } }).canvas;
    if (!c || !c.width || !c.height || typeof ctx.getTransform !== 'function') return null;
    let pool = layerPool.get(c);
    if (!pool) layerPool.set(c, pool = []);
    let lc = pool.find((l) => !layerBusy.has(l) && l.canvas.width === c.width && l.canvas.height === c.height);
    if (!lc) {
      // camadas livres de OUTRO tamanho são de antes de um redimensionamento:
      // saem do pool (cada uma é um canvas do tamanho da tela — 8–33 MB)
      for (let k = pool.length - 1; k >= 0; k--) {
        const l = pool[k]!;
        if (!layerBusy.has(l) && (l.canvas.width !== c.width || l.canvas.height !== c.height)) pool.splice(k, 1);
      }
      const cv = this.scratch(c.width, c.height);
      const got = cv?.getContext('2d') as LayerCtx | null | undefined;
      if (!got) return null;
      lc = got;
      pool.push(lc);
    }
    layerBusy.add(lc);
    lc.setTransform(1, 0, 0, 1, 0, 0);
    lc.globalAlpha = 1;
    lc.globalCompositeOperation = 'source-over';
    lc.clearRect(0, 0, c.width, c.height);
    lc.setTransform(ctx.getTransform());
    return lc;
  }

  /** Camada que falhou no meio da pintura (pilha save/restore desequilibrada): sai do pool. */
  private discardLayer(ctx: CanvasRenderingContext2D, lc: LayerCtx): void {
    const c = (ctx as { canvas?: object }).canvas;
    const pool = c ? layerPool.get(c) : undefined;
    if (pool) { const k = pool.indexOf(lc); if (k >= 0) pool.splice(k, 1); }
    layerBusy.delete(lc);
  }

  /** Aplica as máscaras esfumadas na camada, compõe em `ctx` (alfa e recorte de `ctx` valem) e a devolve ao pool. */
  private compositeLayer(ctx: CanvasRenderingContext2D, lc: LayerCtx, soft: SoftReveal[], x: number, y: number, w: number, h: number): void {
    for (const s of soft) applySoftReveal(lc, s.filter, s.q, x, y, w, h);
    ctx.save();
    ctx.setTransform(1, 0, 0, 1, 0, 0);
    ctx.drawImage(lc.canvas as unknown as CanvasImageSource, 0, 0);
    ctx.restore();
    layerBusy.delete(lc);
  }

  // ------------------------------------------------------------------ resources
  private media(index: number): PaintImage | null {
    return this.env.media(index) ?? null;
  }

  private bitmapCanvas(tables: Tables, index: number): PaintImage | null {
    const key = String(index);
    if (this.bitmaps.has(key)) return this.bitmaps.get(key) ?? null;
    const b = at(tables.bitmaps, index, 'bitmap');
    const recipe = this.env.recipe?.(index);
    const bytes = recipe ? null : this.env.bitmap(index);
    let out: PaintImage | null = null;
    const derived = recipe ? decodeFxRecipe(recipe) : null;
    if (derived) {
      // B5b-G1: a metafile picture with colour effects = fxImage over the base picture's raster (the TS's fxImage over its canvas)
      const base = derived.base !== index ? this.bitmapCanvas(tables, derived.base) : null;
      out = base ? fxImage(base, derived.fx, this.scratch) : null;
    } else if (recipe) {
      // B5b: a metafile or PICT: the canvas the TS rasterizer drew on
      const mk = (w: number, h: number): MetaCanvas | undefined => (this.env.metaCanvas ?? this.env.createCanvas ?? elementCanvas)(w, h) as unknown as MetaCanvas | undefined;
      out = replayRecipe(recipe, { createCanvas: mk, ...(this.env.decodeBlob ? { decode: this.env.decodeBlob } : {}) }) as unknown as PaintImage | null;
    } else if (bytes && b.w > 0 && b.h > 0 && bytes.length >= b.w * b.h * 4) {
      const cv = this.scratch(b.w, b.h);
      const c2 = cv?.getContext('2d') as CanvasRenderingContext2D | null | undefined;
      if (cv && c2) {
        const id = c2.createImageData(b.w, b.h);
        id.data.set(bytes.subarray(0, b.w * b.h * 4));
        c2.putImageData(id, 0, 0);
        out = cv as unknown as PaintImage;
      }
    }
    if (this.bitmaps.size >= MAX_BITMAP_CACHE) this.bitmaps.delete(this.bitmaps.keys().next().value as string);
    this.bitmaps.set(key, out);
    return out;
  }

  private image(tables: Tables, s: ImageSrc): PaintImage | null {
    if (s.kind === 'media') return this.media(s.v);
    if (s.kind === 'bitmap') return this.bitmapCanvas(tables, s.v);
    const key = String(s.v);
    const hit = this.derived.get(key);
    if (hit !== undefined) return hit;
    const d = at(tables.derived, s.v, 'derived');
    const base = this.media(d.image);
    const out = base ? fxImage(base, toTsFx(d.fx), this.scratch) : null;
    // an image still decoding is not cached as missing
    if (base) {
      if (this.derived.size >= MAX_DERIVED_CACHE) this.derived.delete(this.derived.keys().next().value as string);
      this.derived.set(key, out);
    }
    return out;
  }

  private gradientStops(g: CanvasGradient, list: SlidesList, span: Span): void {
    for (const s of spanOf(list.stops, span, 'stops')) g.addColorStop(s.offset, s.color);
  }

  /** A `Paint` as the canvas value to assign (undefined: an unavailable picture: the assignment is skipped). */
  private paintValue(ctx: CanvasRenderingContext2D, list: SlidesList, tables: Tables, p: Paint): string | CanvasGradient | CanvasPattern | undefined {
    switch (p.kind) {
      case 'css': return p.v;
      case 'linear': {
        const g = at(list.gradients, p.v, 'gradient');
        const cg = ctx.createLinearGradient(g.g0, g.g1, g.g2, g.g3);
        this.gradientStops(cg, list, g.stops);
        return cg;
      }
      case 'radial': {
        const g = at(list.gradients, p.v, 'gradient');
        const cg = ctx.createRadialGradient(g.g0, g.g1, g.g2, g.g3, g.g4, g.g5);
        this.gradientStops(cg, list, g.stops);
        return cg;
      }
      case 'conic': {
        const g = at(list.gradients, p.v, 'gradient');
        const cg = ctx.createConicGradient(g.g0, g.g1, g.g2);
        this.gradientStops(cg, list, g.stops);
        return cg;
      }
      case 'pattern': {
        const pr = at(list.patterns, p.v, 'pattern');
        const img = pr.src.kind === 'bitmap' ? this.bitmapCanvas(tables, pr.src.v) : this.image(tables, pr.src.v);
        if (!img) return undefined;
        if (pr.tile) return this.tilePatternOf(ctx, img, pr.tile.params, pr.tile.scale, pr.tile.bx, pr.tile.nat) ?? undefined;
        const pat = ctx.createPattern(img as CanvasImageSource, pr.repeat);
        if (!pat) return undefined;
        const t = pr.transform;
        if (t && typeof (pat as { setTransform?: unknown }).setTransform === 'function' && typeof DOMMatrix !== 'undefined') {
          pat.setTransform(new DOMMatrix([t.a, t.b, t.c, t.d, t.e, t.f]));
        }
        return pat;
      }
    }
  }

  /** `imageTile` (render.ts 522-530): the tile parameters of the list with the decoded size and the live transform. */
  private tilePatternOf(ctx: CanvasRenderingContext2D, img: PaintImage, t: TileParams, scale: number, box: Rect4, nat?: { x: number; y: number }): CanvasPattern | null {
    const iw = imgW(img), ih = imgH(img);
    // a chart tile (C-6, CT-1): the natural size arrives computed by the engine (`(wPt ?? w * 0.75) * S`) and `scale` is 1
    return tilePattern(ctx, img as CanvasImageSource, iw, ih, nat ? nat.x : iw * t.unitX * scale, nat ? nat.y : ih * t.unitY * scale,
      { sx: t.sx, sy: t.sy, tx: t.tx * scale, ty: t.ty * scale, ...(t.algn ? { algn: t.algn } : {}), ...(t.flip ? { flip: t.flip as 'x' | 'y' | 'xy' } : {}) },
      { x: box.x, y: box.y, w: box.w, h: box.h });
  }

  private pathGradCache = new Map<string, PaintImage>();

  // ------------------------------------------------------------------ text (drawText of render.ts 1638-1707)
  private drawText(ctx: CanvasRenderingContext2D, list: SlidesList, tables: Tables, r: TextRun): void {
    const text = list.texts.slice(r.text.start, r.text.start + r.text.len);
    if (r.text.start + r.text.len > list.texts.length) throw new RangeError('text span out of range');
    ctx.font = fontStr(this.env, at(tables.faces, r.font.face, 'face'), r.font.sizePx);
    const tx = r.x, by = r.y;
    if (r.bullet) {
      // a bullet: `ctx.font`, `fillStyle` and `fillText`, nothing else (render.ts 1785-1787)
      const bfs = this.paintValue(ctx, list, tables, r.fill);
      if (bfs !== undefined) ctx.fillStyle = bfs;
      ctx.fillText(text, tx, by);
      return;
    }
    const ls = r.letterSpacingPx;
    const lsCtx = ctx as unknown as { letterSpacing?: string; fontKerning?: string };
    const prevLs = lsCtx.letterSpacing;
    // kerning do desenho = o do layout (o run liga ou não — `kern`)
    const prevKern = lsCtx.fontKerning;
    if (prevKern !== undefined) lsCtx.fontKerning = r.kern ? 'normal' : 'none';
    // run inteiro: a diferença entre a largura natural do canvas e a do layout (avanços
    // do app em 1/8 pt, sem ligaduras) se espalha pelos caracteres
    let fit = 0;
    if (!ls && r.fitAllowed && r.laidW !== undefined && prevLs !== undefined) {
      const n = [...text].length;
      if (n > 1) { const d = r.laidW - ctx.measureText(text).width; if (Math.abs(d) > 0.25) fit = d / n; }
    }
    if ((ls || fit) && prevLs !== undefined) lsCtx.letterSpacing = `${ls || fit}px`;
    const fs = this.paintValue(ctx, list, tables, r.fill);
    if (fs !== undefined) ctx.fillStyle = fs;
    const prevA = ctx.globalAlpha;
    if (r.alphaMul !== undefined) ctx.globalAlpha = prevA * r.alphaMul;
    if (r.fillText === false) { /* letras sem preenchimento: só o contorno */ } else if (r.shadow) {
      const sc = r.shadow;
      ctx.save();
      ctx.shadowColor = sc.color;
      ctx.shadowBlur = sc.blur;
      ctx.shadowOffsetX = sc.dx;
      ctx.shadowOffsetY = sc.dy;
      ctx.fillText(text, tx, by);
      ctx.restore();
    } else {
      ctx.fillText(text, tx, by);
    }
    ctx.globalAlpha = prevA;
    // contorno das letras (<a:ln> do rPr)
    if (r.outline) {
      ctx.save();
      ctx.strokeStyle = r.outline.color; ctx.lineWidth = r.outline.width; ctx.lineJoin = 'round';
      ctx.strokeText(text, tx, by);
      ctx.restore();
    }
    if (r.underline) {
      ctx.strokeStyle = r.underline.color; ctx.lineWidth = r.underline.width;
      const uy = r.underline.y;
      const tw = r.laidW ?? ctx.measureText(text).width;
      ctx.beginPath(); ctx.moveTo(tx, uy); ctx.lineTo(tx + tw, uy); ctx.stroke();
    }
    if (r.strike) {
      ctx.strokeStyle = r.strike.color; ctx.lineWidth = r.strike.width;
      const sy = r.strike.y;
      const tw = r.laidW ?? ctx.measureText(text).width;
      ctx.beginPath(); ctx.moveTo(tx, sy); ctx.lineTo(tx + tw, sy); ctx.stroke();
    }
    if ((ls || fit) && prevLs !== undefined) lsCtx.letterSpacing = prevLs;
    if (prevKern !== undefined) lsCtx.fontKerning = prevKern;
  }

  // ------------------------------------------------------------------ effects
  /**
   * `letterFill` (render.ts 1840-1854) on the raster box of the warp: `k` is the raster's px per pt. Gradients are
   * `makeLinearGradient` (the producer expanded the PowerPoint sub-stops), a path gradient is rasterized here from the
   * model stops (`pathGradient`), pictures are tiles (`imageTile`) or a stretched no-repeat pattern.
   */
  private letterPaint(c: CanvasRenderingContext2D, tables: Tables, wp: WarpPaint, box: [number, number, number, number], k: number): CanvasGradient | CanvasPattern | null {
    const [bx, by, bw, bh] = box;
    if (!(bw > 0) || !(bh > 0)) return null;
    if (wp.kind === 'linear' || wp.kind === 'radial' || wp.kind === 'path') {
      if (wp.kind === 'path' && wp.path) {
        const raw = wp.raw.map((r) => (r.alpha === 1 ? { pos: r.pos, color: r.color } : { pos: r.pos, color: r.color, alpha: r.alpha }));
        const pat = pathGradient(c, this.scratch, this.pathGradCache, wp.path, wp.fillTo, raw, bx, by, bw, bh);
        if (pat) return pat as unknown as CanvasGradient;
      }
      if (wp.kind !== 'linear') {
        const cx = bx + bw / 2, cy = by + bh / 2;
        const g = c.createRadialGradient(cx, cy, 0, cx, cy, Math.max(bw, bh) / 2);
        for (const s of wp.stops) g.addColorStop(s.offset, s.color);
        return g;
      }
      const ang = (wp.angle ?? 0) * Math.PI / 180;
      let dx = Math.cos(ang), dy = Math.sin(ang);
      if (wp.scaled) {
        dx = Math.cos(ang) * bh; dy = Math.sin(ang) * bw;
        const len = Math.hypot(dx, dy) || 1;
        dx /= len; dy /= len;
      }
      const cx = bx + bw / 2, cy = by + bh / 2;
      const half = (Math.abs(dx) * bw + Math.abs(dy) * bh) / 2;
      const g = c.createLinearGradient(cx - dx * half, cy - dy * half, cx + dx * half, cy + dy * half);
      for (const s of wp.stops) g.addColorStop(s.offset, s.color);
      return g;
    }
    const img = wp.image ? this.image(tables, wp.image) : null;
    if (!img) return null;
    if (wp.tile) return this.tilePatternOf(c, img, wp.tile, k / 12700, { x: bx, y: by, w: bw, h: bh });
    const fr = wp.fillRect;
    const rr = fr ? { x: bx + fr.l * bw, y: by + fr.t * bh, w: bw * (1 - fr.l - fr.r), h: bh * (1 - fr.t - fr.b) } : { x: bx, y: by, w: bw, h: bh };
    const pat = c.createPattern(img as CanvasImageSource, 'no-repeat');
    if (pat && typeof pat.setTransform === 'function' && typeof DOMMatrix !== 'undefined' && imgW(img) > 0 && imgH(img) > 0) {
      pat.setTransform(new DOMMatrix([rr.w / imgW(img), 0, 0, rr.h / imgH(img), rr.x, rr.y]));
    }
    return pat;
  }

  private warpLines(tables: Tables, lines: readonly WireWarpLine[]): WarpLine[] {
    return lines.map((l) => {
      const runs: WarpRun[] = l.runs.map((r) => {
        const face = at(tables.faces, r.font.face, 'face');
        const run: WarpRun = {
          text: r.text, font: (k: number) => fontStr(this.env, face, r.font.sizePt * k), x: r.x, color: r.color,
        };
        if (r.stroke) run.stroke = { color: r.stroke.color, width: r.stroke.width };
        if (r.pattern) run.pattern = { prst: r.pattern.prst, fg: r.pattern.fg, bg: r.pattern.bg };
        if (r.spc !== undefined) run.spc = r.spc;
        const wp = r.paint;
        if (wp) run.paint = (c, box, k) => this.letterPaint(c, tables, wp, box, k);
        if (r.alpha !== undefined) run.alpha = r.alpha;
        return run;
      });
      const o: WarpLine = { runs, baseline: l.baseline, sizePt: l.sizePt, align: l.align };
      if (l.height !== undefined) o.height = l.height;
      if (l.lnPct !== undefined) o.lnPct = l.lnPct;
      if (l.trailing !== undefined) o.trailing = l.trailing;
      return o;
    });
  }

  /** `paintWarp` from the laid-out lines on (render.ts 1923-1957): the layer, the tiled picture and the shadow composite. */
  private paintWarp(ctx: CanvasRenderingContext2D, tables: Tables, e: WarpEffect): void {
    const { x, y, w, h } = e.bx;
    const lines = this.warpLines(tables, e.lines);
    const tileImg = e.tile ? this.image(tables, e.tile.image) : null;
    const layer = e.shadow || tileImg ? this.acquireLayer(ctx) : null;
    const target = layer ?? ctx;
    const rowsOf: PatternRowsOf = this.env.patternRows ?? (() => undefined);
    paintWarpedText(target, e.prst, e.adj, !!e.fromWordArt, lines, x, y, w, h, e.pxPerPt, (cw, ch) => this.scratch(cw, ch), rowsOf);
    if (layer && tileImg && e.tile) {
      const pat = this.tilePatternOf(layer, tileImg, e.tile.tile.params, e.tile.tile.scale, e.tile.tile.bx);
      if (pat) {
        layer.save();
        layer.globalCompositeOperation = 'source-in';
        layer.fillStyle = pat;
        layer.fillRect(x - 4 * w, y - 4 * h, 9 * w, 9 * h);
        layer.restore();
      }
    }
    if (layer) {
      ctx.save();
      ctx.setTransform(1, 0, 0, 1, 0, 0);
      if (e.shadow) {
        ctx.shadowColor = e.shadow.color;
        ctx.shadowBlur = e.shadow.blur;
        ctx.shadowOffsetX = e.shadow.dx;
        ctx.shadowOffsetY = e.shadow.dy;
      }
      ctx.drawImage(layer.canvas as unknown as CanvasImageSource, 0, 0);
      ctx.restore();
      layerBusy.delete(layer);
    }
  }

  /** The raster-op path of metafile.ts (1372-1385 `patBlt`): the operand bitmap is the brush (pixel 0), PATINVERT is the exact XOR. */
  private metafileRop(ctx: CanvasRenderingContext2D, tables: Tables, e: MetafileRopEffect, clipped: boolean): void {
    const bytes = this.env.bitmap(e.operand);
    at(tables.bitmaps, e.operand, 'bitmap');
    const rgb: [number, number, number] = bytes && bytes.length >= 3 ? [bytes[0]!, bytes[1]!, bytes[2]!] : [0, 0, 0];
    const css = `#${rgb.map((v) => v.toString(16).padStart(2, '0')).join('')}`;
    const { x, y, w, h } = e.rect;
    const x0 = x, y0 = y, x1 = x + w, y1 = y + h;
    const r3 = (e.rop >>> 16) & 0xff;
    if (r3 === 0x5a && xorRect(ctx, rgb, x0, y0, x1, y1, clipped)) return;
    const mix = r3 === 0x00 ? { color: '#000000' } : r3 === 0xff ? { color: '#ffffff' } : r3 === 0x55 ? { color: '#ffffff', op: 'difference' as const }
      : r3 === 0xf0 ? { color: css } : r3 === 0x5a ? { color: css, op: 'difference' as const } : undefined;
    if (!mix) return;
    ctx.save();
    if ('op' in mix && mix.op) ctx.globalCompositeOperation = mix.op;
    ctx.fillStyle = mix.color;
    ctx.fillRect(Math.min(x0, x1), Math.min(y0, y1), Math.abs(x1 - x0), Math.abs(y1 - y0));
    ctx.restore();
  }

  private effect(ctx: CanvasRenderingContext2D, list: SlidesList, tables: Tables, e: Effect, clipped: boolean): void {
    switch (e.kind) {
      case 'bevel': {
        if (!this.env.light) return;
        applyBevel(ctx, e, spanOf(list.paths, e.path, 'paths'), this.scratch, this.env.light);
        return;
      }
      case 'warp': this.paintWarp(ctx, tables, e); return;
      case 'metafileRop': this.metafileRop(ctx, tables, e, clipped); return;
    }
  }

  // ------------------------------------------------------------------ the interpreter
  /**
   * Run the list. `frame` gives the slot values (positional over `list.slots`; missing = static). A throw inside a guarded
   * range is contained (that shape is skipped); a throw outside every range propagates after the saves are undone.
   */
  paint(ctx: CanvasRenderingContext2D, list: SlidesList, tables: Tables, frame?: SlotFrame | { values: SlotValue[] },
    replacements?: ReadonlyMap<number, SlotReplacement>): void {
    const items = list.items;
    const n = items.length;
    const view = list.view;
    const m: SlidePx = { ox: view.ox, oy: view.oy, sw: view.sw, sh: view.sh };
    const main: Target = { ctx, saves: 0, clips: [0], layer: -1 };
    let t: Target = main;
    const stack: Target[] = [];
    const layers: (LayerInfo | undefined)[] = [];
    const regs: (DOMMatrix | undefined)[] = [];
    const slots: SlotRun[] = [];
    let itemDepth = 0;
    let range: { itemDepth: number; mainSaves: number; layersAcquired: LayerInfo[]; stackLen: number; slotLen: number; pc: number } | undefined;
    const acquired: LayerInfo[] = [];
    let pc = 0;

    const skipTo = (from: number): number => {
      // the index after the closer of the bracket whose opener is at `from - 1` (depth counted over saves, restores and slots)
      let d = 1;
      for (let j = from; j < n; j++) {
        const op = items[j]!.op;
        if (op === 'save' || op === 'slotBegin') d++;
        else if (op === 'restore' || op === 'slotEnd') { d--; if (d === 0) return j + 1; }
      }
      return n;
    };
    const closeRange = (): void => { if (range && itemDepth === 1) range = undefined; };
    const openRange = (at0: number): void => {
      if (itemDepth === 1 && !range) range = { itemDepth: 1, mainSaves: main.saves, layersAcquired: [...acquired], stackLen: stack.length, slotLen: slots.length, pc: at0 };
    };
    const afterLayerEnd = (from: number, layer: number): number => {
      for (let j = from; j < n; j++) {
        const it = items[j]!;
        if (it.op === 'layerEnd' && it.layer === layer) return j + 1;
      }
      return n;
    };
    // the alpha factor the open slots of target `tt` multiplied in (1 when every value is static)
    const alphaK = (tt: Target): number => {
      let k = 1;
      for (const s of slots) if (s.target === tt && s.k !== 1) k *= s.k;
      return k;
    };
    const repair = (r: NonNullable<typeof range>): void => {
      // the TS `guarded`: undo the saves left pending on the main context, discard the layers of the range
      while (main.saves > r.mainSaves) { main.ctx.restore(); main.saves--; }
      for (let i = acquired.length - 1; i >= 0; i--) {
        const li = acquired[i]!;
        if (r.layersAcquired.includes(li)) continue;
        this.discardLayer(main.ctx, li.lc);
        acquired.splice(i, 1);
      }
      stack.length = r.stackLen;
      slots.length = r.slotLen;
      t = stack.length ? stack[stack.length - 1]! : main;
      main.clips.length = main.saves + 1;
      itemDepth = r.itemDepth;
    };

    while (pc < n) {
      try {
        while (pc < n) {
          const at0 = pc;
          const it: SItem = items[pc++]!;
          const c = t.ctx;
          switch (it.op) {
            case 'save': {
              openRange(at0);
              c.save(); t.saves++; t.clips.push(t.clips[t.clips.length - 1] ?? 0); itemDepth++;
              break;
            }
            case 'restore': {
              c.restore(); if (t.saves > 0) { t.saves--; t.clips.pop(); } itemDepth--;
              closeRange();
              break;
            }
            case 'translate': c.translate(it.x, it.y); break;
            case 'rotate': c.rotate(it.v); break;
            case 'scale': c.scale(it.x, it.y); break;
            case 'transform': c.transform(it.a, it.b, it.c, it.d, it.e, it.f); break;
            case 'setTransform': c.setTransform(it.a, it.b, it.c, it.d, it.e, it.f); break;
            case 'resetTransform': c.resetTransform(); break;
            case 'captureCtm': {
              if (it.v < 0 || it.v > 3) throw new RangeError(`register ${it.v}`);
              regs[it.v] = c.getTransform();
              break;
            }
            case 'transformByInverseOf': {
              const r = regs[it.v];
              if (!r) throw new RangeError(`register ${it.v} not captured`);
              const inv = r.inverse();
              c.transform(inv.a, inv.b, inv.c, inv.d, inv.e, inv.f);
              break;
            }
            case 'transformUnshape': {
              const r = regs[it.v];
              if (!r) throw new RangeError(`register ${it.v} not captured`);
              const u = c.getTransform().inverse().multiply(r);
              c.transform(u.a, u.b, u.c, u.d, u.e, u.f);
              break;
            }
            case 'setTransformReg': {
              const r = regs[it.v];
              if (!r) throw new RangeError(`register ${it.v} not captured`);
              c.setTransform(r);
              break;
            }
            case 'globalAlpha': { const k = slots.length ? alphaK(t) : 1; c.globalAlpha = k === 1 ? it.v : it.v * k; break; }
            case 'globalAlphaMul': c.globalAlpha = c.globalAlpha * it.v; break;
            case 'composite': c.globalCompositeOperation = it.v; break;
            case 'filterSet': c.filter = it.v; break;
            case 'filterNone': c.filter = 'none'; break;
            case 'fillStyle': { const v = this.paintValue(c, list, tables, it.v); if (v !== undefined) c.fillStyle = v; break; }
            case 'strokeStyle': { const v = this.paintValue(c, list, tables, it.v); if (v !== undefined) c.strokeStyle = v; break; }
            case 'lineWidth': c.lineWidth = it.v; break;
            case 'lineJoin': c.lineJoin = it.v; break;
            case 'lineCap': c.lineCap = it.v; break;
            case 'miterLimit': c.miterLimit = it.v; break;
            case 'lineDash': c.setLineDash(spanOf(list.dashes, it, 'dashes')); break;
            case 'shadowColor': c.shadowColor = it.v; break;
            case 'shadowBlur': c.shadowBlur = it.v; break;
            case 'shadowOffset': {
              const mt = c.getTransform();
              c.shadowOffsetX = it.x.add + (it.x.axis === 'a' ? mt.a : mt.b) * it.x.k;
              c.shadowOffsetY = it.y.add + (it.y.axis === 'a' ? mt.a : mt.b) * it.y.k;
              break;
            }
            case 'font': c.font = fontStr(this.env, at(tables.faces, it.face, 'face'), it.sizePx); break;
            case 'chartFont': c.font = chartFontStr(this.env, at(tables.faces, it.face, 'face'), it.sizePx); break;
            // Sheet lists (frontier C4): `text-metrics.ts` CellFont string, or a literal font string (the headers)
            case 'sheetFont': {
              if (it.css !== undefined) c.font = it.css;
              else if (it.font) c.font = fontStr(this.env, at(tables.faces, it.font.face, 'face'), it.font.sizePx).replace(/^(italic )?/, it.italic ? 'italic ' : '');
              break;
            }
            // `drawCellText` (text-metrics.ts:104-130) of one piece; the font and `textAlign` were set by the items before it
            case 'glyphRun': { paintGlyphRun(c, list, it); break; }
            case 'fillText': {
              if (it.text.start + it.text.len > list.texts.length) throw new RangeError('text span out of range');
              c.fillText(list.texts.slice(it.text.start, it.text.start + it.text.len), it.x, it.y);
              break;
            }
            case 'textAlign': c.textAlign = it.v; break;
            case 'textBaseline': c.textBaseline = it.v; break;
            case 'imageSmoothing': c.imageSmoothingEnabled = it.v; break;
            case 'beginPath': c.beginPath(); break;
            case 'appendPath': replayPath(c, spanOf(list.paths, it, 'paths')); break;
            case 'fill': c.fill(it.v); break;
            case 'stroke': c.stroke(); break;
            case 'clip': c.clip(it.v); t.clips[t.clips.length - 1] = (t.clips[t.clips.length - 1] ?? 0) + 1; break;
            case 'fillRect': c.fillRect(it.x, it.y, it.w, it.h); break;
            case 'strokeRect': c.strokeRect(it.x, it.y, it.w, it.h); break;
            case 'clearRect': c.clearRect(it.x, it.y, it.w, it.h); break;
            case 'drawImage': {
              const img = this.image(tables, it.src);
              if (!img) break;
              if (it.s) c.drawImage(img as CanvasImageSource, it.s.x, it.s.y, it.s.w, it.s.h, it.d.x, it.d.y, it.d.w, it.d.h);
              else c.drawImage(img as CanvasImageSource, it.d.x, it.d.y, it.d.w, it.d.h);
              break;
            }
            case 'prepare': this.image(tables, it.v); break;
            case 'text': this.drawText(c, list, tables, at(list.runs, it.v, 'run')); break;
            case 'layerBegin': {
              if (it.layer < 0 || it.layer >= 16) throw new RangeError(`layer ${it.layer}`);
              const prev = items[at0 - 1];
              const shapeSlot = prev && prev.op === 'slotBegin' ? slots[slots.length - 1] : undefined;
              // the soft-reveal layers of a playback list have reserved ids (uo-dml-render SOFT_LAYER_SHAPE / SOFT_LAYER_CHART)
              const softLayer = shapeSlot !== undefined
                && ((shapeSlot.def.kind === 'shape' && it.layer === 15) || (shapeSlot.def.kind === 'chartPart' && it.layer === 14));
              const pending = softLayer ? shapeSlot.soft : undefined;
              let lc: LayerCtx | null = null;
              if (!softLayer || (pending && pending.length)) lc = this.acquireLayer(c);
              if (!lc) {
                if (pending && pending.length) {
                  const bx = shapeSlot!.def.bx;
                  for (const s of pending) c.globalAlpha = c.globalAlpha * applyReveal(c, s.filter, s.q, bx.x, bx.y, bx.w, bx.h);
                }
                pc = Math.min(afterLayerEnd(pc, it.layer), n);
                break;
              }
              const old = layers[it.layer];
              if (old) { this.discardLayer(c, old.lc); }
              // the soft-reveal layer of a slot composites with that slot's pending reveals; every other layer with none
              const li: LayerInfo = { lc, fallbackEnd: it.fallbackEnd, begin: at0, ...(softLayer ? { soft: shapeSlot } : {}) };
              layers[it.layer] = li;
              acquired.push(li);
              stack.push(t);
              t = { ctx: lc, saves: 0, clips: [0], layer: it.layer };
              break;
            }
            case 'layerEnd': {
              const li = layers[it.layer];
              if (!li || t.layer !== it.layer) break; // no layer was opened for it (the else block ran)
              t = stack.pop() ?? main;
              if (it.then === 'composite') {
                const sh = li.soft;
                const bx = sh ? sh.def.bx : { x: 0, y: 0, w: 0, h: 0 };
                this.compositeLayer(t.ctx, li.lc, sh ? sh.soft : [], bx.x, bx.y, bx.w, bx.h);
                layers[it.layer] = undefined;
                const k = acquired.indexOf(li); if (k >= 0) acquired.splice(k, 1);
              }
              pc = Math.max(pc, Math.min(li.fallbackEnd, n));
              break;
            }
            case 'drawLayer': {
              const li = layers[it.v];
              if (li) c.drawImage(li.lc.canvas as unknown as CanvasImageSource, 0, 0);
              break;
            }
            case 'releaseLayer': {
              const li = layers[it.v];
              if (li) {
                layerBusy.delete(li.lc);
                layers[it.v] = undefined;
                const k = acquired.indexOf(li); if (k >= 0) acquired.splice(k, 1);
              }
              break;
            }
            case 'effect': this.effect(c, list, tables, at(list.effects, it.v, 'effect'), (t.clips[t.clips.length - 1] ?? 0) > 0); break;
            case 'slotBegin': {
              const def = at(list.slots, it.v, 'slot');
              const value: SlotValue = frame?.values[it.v] ?? { t: 'static' };
              // the TS tests these before its `save()`: nothing is drawn, nothing is saved
              if (value.t === 'hidden' || (value.t === 'state' && !value.visible && (def.kind === 'paragraph' || def.kind === 'unit'))) {
                pc = skipTo(pc);
                break;
              }
              if (def.kind === 'unitGate') {
                // the bullet follows its paragraph's first unit: drawn unless that unit is invisible (render.ts 1768-1776)
                if (value.t === 'state' && !(value.visible && value.opacity > 0)) { pc = skipTo(pc); break; }
                slots.push({ def, soft: [], begin: at0, noSave: true, target: t, k: 1 });
                break;
              }
              if (def.kind === 'boundsPass') {
                // the `chartPartBounds` probe: the first frame of a stage only (the producer hides it afterwards)
                slots.push({ def, soft: [], begin: at0, noSave: true, target: t, k: 1 });
                break;
              }
              const rep = def.kind === 'shape' ? replacements?.get(def.shape) : undefined;
              if (rep) {
                // the shape again with this frame's colours, fonts or font scale: its own items, slots and values
                openRange(at0);
                this.paint(c, rep.list, tables, { values: rep.values });
                closeRange();
                pc = skipTo(pc);
                break;
              }
              // a chart part applies its value on the part's own save (the `save` item right before it)
              const own = def.kind !== 'chartPart';
              if (own) openRange(at0);
              const before = c.globalAlpha;
              if (own) { c.save(); t.saves++; t.clips.push(t.clips[t.clips.length - 1] ?? 0); itemDepth++; }
              const run: SlotRun = { def, soft: [], begin: at0, target: t, k: 1, ...(own ? {} : { noSave: true }) };
              const soft = def.kind === 'shape' || def.kind === 'chartPart' ? run.soft : undefined;
              let ok = true;
              if (value.t === 'shape') {
                if (value.op <= 0) ok = false;
                else {
                  if (value.op < 1) c.globalAlpha = c.globalAlpha * value.op;
                  if (value.all) ok = applyPart(c, value.all, def.base, def.bx.x, def.bx.y, def.bx.w, def.bx.h, m, def.baseRotRad, soft);
                }
              } else if (value.t === 'state') {
                ok = applyPart(c, value, def.base, def.bx.x, def.bx.y, def.bx.w, def.bx.h, m, def.baseRotRad, soft);
              }
              if (!ok) {
                if (own) { c.restore(); t.saves--; t.clips.pop(); itemDepth--; closeRange(); }
                pc = skipTo(pc);
                break;
              }
              const after = c.globalAlpha;
              if (after !== before && before > 0) run.k = after / before;
              slots.push(run);
              break;
            }
            case 'slotEnd': {
              if (!slots.length) throw new RangeError('slotEnd without slotBegin');
              const run = slots.pop()!;
              if (run.noSave) break;
              c.restore(); if (t.saves > 0) { t.saves--; t.clips.pop(); } itemDepth--;
              closeRange();
              break;
            }
            case 'mediaFrame': {
              const f = this.env.mediaFrame?.(it.shape);
              const drawCropped = (im: PaintImage): void => {
                const cr = it.crop;
                if (cr) {
                  const iw = imgW(im), ih = imgH(im);
                  c.drawImage(im as CanvasImageSource, cr.l * iw, cr.t * ih, iw - (cr.l + cr.r) * iw, ih - (cr.t + cr.b) * ih, it.d.x, it.d.y, it.d.w, it.d.h);
                } else c.drawImage(im as CanvasImageSource, it.d.x, it.d.y, it.d.w, it.d.h);
              };
              if (f && f.alpha >= 1) { drawCropped(f.image); break; }
              const p = it.poster;
              const poster = p ? this.image(tables, p.src) : null;
              if (!p || !poster) break;
              if (p.s) c.drawImage(poster as CanvasImageSource, p.s.x, p.s.y, p.s.w, p.s.h, p.d.x, p.d.y, p.d.w, p.d.h);
              else c.drawImage(poster as CanvasImageSource, p.d.x, p.d.y, p.d.w, p.d.h);
              // a fading video: the frame over the poster
              if (f && f.alpha > 0 && f.alpha < 1) { c.save(); c.globalAlpha = c.globalAlpha * f.alpha; drawCropped(f.image); c.restore(); }
              break;
            }
          }
        }
      } catch (e) {
        if (!range) {
          // outside every guarded range: undo what is pending and let the caller see the error
          repair({ itemDepth: 0, mainSaves: 0, layersAcquired: [], stackLen: 0, slotLen: 0, pc: 0 });
          throw e;
        }
        const r = range;
        range = undefined;
        repair(r);
        pc = skipTo(r.pc + 1);
      }
    }
    // layers left open by a malformed list never leak into the pool
    for (const li of acquired) layerBusy.delete(li.lc);
  }
}

export type { Stop, PathOp, SItem };
