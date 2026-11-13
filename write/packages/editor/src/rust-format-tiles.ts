// Frente A1 da leva 7 do engine="rust" (correção 2): as vistas contínuas (Web, Rascunho, Estrutura) pintam a folha alta em
// FAIXAS de no máximo 4000 px CSS, como o layout legado do TS (CONT_TILE_PX, universal-write.ts 12516): uma tela só com a
// altura do documento passa do limite de ~16k px do navegador (buffer reduzido = borrado) e custa memória. O canvas da
// página fica com a caixa (acerto do clique, cursor, rolagem) e sem pixels; as faixas, por cima e sem eventos de ponteiro,
// existem só na janela visível (a mesma de ensureVisiblePainted) e recebem a pintura da página inteira: o contexto de cada
// faixa desloca toda transformação pela altura dela, então base, ganchos, seleção e cursor desenham como na página.
// Registro: core/docs/modules/uo-bindings.md "Wave 7 — A1".
import { PX_PER_TWIP } from './rust-review-core.js';
import type { FormatCtx } from './rust-format-core.js';

/** `CONT_TILE_PX` (universal-write.ts 12516): the height of a strip, CSS px */
export const STRIP_PX = 4000;
/** Chromium's canvas limits (`bufferScaleFor`, universal-write.ts 21662) */
const MAX_DIM = 16384;

interface Strip { canvas: HTMLCanvasElement; ctx: CanvasRenderingContext2D; base: HTMLCanvasElement; baseCtx: CanvasRenderingContext2D; k: number; oy: number; bs: number; cssW: number; cssH: number }
type El = HTMLElement & { view: string; zoom: number; canvases: HTMLCanvasElement[]; allocatedPages: Set<number>; scrollTop: number };

/** A context whose transforms are all offset by `oy` device rows (the strip's top in the page): the page's painting lands on the strip. */
function offsetContext(ctx: CanvasRenderingContext2D, oy: number): CanvasRenderingContext2D {
  const set = ctx.setTransform.bind(ctx) as (a: number, b: number, c: number, d: number, e: number, f: number) => void;
  const get = ctx.getTransform.bind(ctx);
  const patched = ctx as CanvasRenderingContext2D & { setTransform: unknown; resetTransform: unknown; getTransform: unknown };
  patched.setTransform = (a?: number | DOMMatrix2DInit, b?: number, c?: number, d?: number, e?: number, f?: number): void => {
    if (typeof a === 'number') { set(a, b!, c!, d!, e!, f! - oy); return; }
    const m = DOMMatrix.fromMatrix(a ?? {});
    set(m.a, m.b, m.c, m.d, m.e, m.f - oy);
  };
  patched.resetTransform = (): void => set(1, 0, 0, 1, 0, -oy);
  patched.getTransform = (): DOMMatrix => { const m = get(); m.f += oy; return m; };
  ctx.setTransform(1, 0, 0, 1, 0, 0);
  return ctx;
}

/** `bufferScaleFor(canvas)` of the element (universal-write.ts 21662) */
function bufferScale(cssW: number, cssH: number): number {
  const dpr = window.devicePixelRatio || 1;
  return Math.min(dpr, MAX_DIM / cssW, MAX_DIM / cssH, Math.sqrt((MAX_DIM * MAX_DIM) / (cssW * cssH)));
}

export interface Tiler {
  /** the shell's `tiles` hook */
  tiles(i: number, cv: HTMLCanvasElement): { ref: object; ctx: CanvasRenderingContext2D; base: HTMLCanvasElement; baseCtx: CanvasRenderingContext2D; w: number; h: number; oy: number; scale: number }[] | null;
  /** a scroll: the tall pages whose visible strips changed are painted again */
  scrolled(): void;
  /** a new document or view: every strip released */
  reset(): void;
  /** the strips of page `i` (the harness's canvas facet): CSS and buffer sizes */
  stripsOf(i: number): { cssW: number; cssH: number; w: number; h: number }[];
  /** wave 7c (V): every strip cut of page `i` (CSS px `[top, bottom)`, also the ones not allocated now) */
  cutsOf(i: number): [number, number][];
}

export function tiler(c: FormatCtx, continuous: () => boolean): Tiler {
  const strips = new Map<number, Strip[]>();
  /** the page canvases left without pixels (their buffer given back when they stop being tiled) */
  const emptied = new WeakSet<HTMLCanvasElement>();
  const el = (): El | null => (c.el as unknown as El | null) ?? null;
  const release = (i: number): void => {
    for (const s of strips.get(i) ?? []) s.canvas.remove();
    strips.delete(i);
  };
  const tall = (cv: HTMLCanvasElement): boolean => (parseFloat(cv.style.height) || 0) > STRIP_PX;
  /**
   * Wave 7c (V): the strips of sheet `i` cut as the TS cuts its tiles (`contTileBreak`, universal-write.ts 12516–12524): a
   * strip ends at the top of the first line (below its own top) whose bottom passes `CONT_TILE_PX` from the strip's top, so
   * no line crosses a strip boundary; the strips are contiguous and the last one ends at the sheet's end (the content plus
   * the TS margin). CSS px `[top, bottom)` in the sheet.
   */
  const cutsOf = (i: number, cssH: number): [number, number][] => {
    const e = el(), p = (c.dl?.pages as unknown as { lines?: { y: number; height: number }[] }[] | undefined)?.[i];
    const s = PX_PER_TWIP * (e?.zoom ?? 1);
    const lines = (p?.lines ?? []).map((l) => [l.y * s, (l.y + l.height) * s] as const).sort((a, b) => a[0] - b[0]);
    const out: [number, number][] = [];
    let start = 0, prev = 0;
    for (const [y0, y1] of lines) {
      if (y0 >= cssH) break;
      // the line does not fit the strip: the strip ends at its top (or, past a gap wider than a strip, at the bottom of the
      // line before it; with neither, a plain cut)
      while (y1 - start > STRIP_PX && y0 > start) {
        const cut = y0 - start <= STRIP_PX ? y0 : prev > start ? prev : start + STRIP_PX;
        out.push([start, cut]);
        start = cut;
      }
      prev = Math.max(prev, y1);
    }
    // past the last line (an object below it, the bottom margin): plain cuts, none over `CONT_TILE_PX`
    while (cssH - start > STRIP_PX) { out.push([start, start + STRIP_PX]); start += STRIP_PX; }
    out.push([start, cssH]);
    return out;
  };
  /** the strips page `i` needs now (the window of ensureVisiblePainted: one screen before, two after) */
  const wanted = (e: El, cv: HTMLCanvasElement, i: number): number[] => {
    const cssH = parseFloat(cv.style.height) || 0;
    const viewH = e.clientHeight || 900;
    const from = e.scrollTop - viewH, to = e.scrollTop + viewH * 2;
    const out: number[] = [];
    cutsOf(i, cssH).forEach(([a, b], k) => {
      const y0 = cv.offsetTop + a, y1 = cv.offsetTop + b;
      if (y1 >= from && y0 <= to) out.push(k);
    });
    return out;
  };
  const tiles: Tiler['tiles'] = (i, cv) => {
    const e = el();
    if (!e || !continuous() || !tall(cv) || cv.style.display === 'none') {
      if (strips.has(i)) release(i);
      if (emptied.has(cv)) {
        // tiled no longer (shorter now, or another view with the same canvas): its own buffer back
        emptied.delete(cv);
        const w = parseFloat(cv.style.width) || 1, h = parseFloat(cv.style.height) || 1;
        const bs = bufferScale(w, h);
        cv.width = Math.round(w * bs);
        cv.height = Math.round(h * bs);
      }
      return null;
    }
    const cssW = parseFloat(cv.style.width) || 1, cssH = parseFloat(cv.style.height) || 1;
    // the page canvas keeps its box (the hit tests, the caret, the scrolling) without pixels
    if (cv.width > 1 || cv.height > 1) { cv.width = 1; cv.height = 1; }
    emptied.add(cv);
    const want = wanted(e, cv, i);
    const cuts = cutsOf(i, cssH);
    const have = (strips.get(i) ?? []).filter((s) => {
      const ok = s.canvas.isConnected && s.cssW === cssW && want.includes(s.k)
        && s.canvas.style.top === `${cv.offsetTop + (cuts[s.k]?.[0] ?? -1)}px` && s.cssH === (cuts[s.k]![1] - cuts[s.k]![0]);
      if (!ok) s.canvas.remove();
      return ok;
    });
    let after: Element = have.length ? have[have.length - 1]!.canvas : cv;
    for (const k of want) {
      if (have.some((s) => s.k === k)) continue;
      const [top, bottom] = cuts[k]!;
      const h = bottom - top;
      const bs = bufferScale(cssW, Math.max(h, 1));
      const canvas = document.createElement('canvas');
      canvas.className = 'page-strip';
      canvas.setAttribute('aria-hidden', 'true');
      const st = canvas.style;
      st.position = 'absolute';
      st.pointerEvents = 'none';
      st.left = `${cv.offsetLeft}px`;
      st.top = `${cv.offsetTop + top}px`;
      st.width = `${cssW}px`;
      st.height = `${h}px`;
      st.margin = '0';
      canvas.width = Math.max(1, Math.round(cssW * bs));
      canvas.height = Math.max(1, Math.round(h * bs));
      const oy = Math.round(top * bs);
      after.after(canvas);
      after = canvas;
      // the strip's base cache (the shell paints the page base on it once per layout and key), offset as the strip
      const base = document.createElement('canvas');
      base.width = canvas.width;
      base.height = canvas.height;
      have.push({ canvas, ctx: offsetContext(canvas.getContext('2d')!, oy), base, baseCtx: offsetContext(base.getContext('2d')!, oy), k, oy, bs, cssW, cssH: h });
    }
    have.sort((a, b) => a.k - b.k);
    strips.set(i, have);
    return have.map((s) => ({ ref: s, ctx: s.ctx, base: s.base, baseCtx: s.baseCtx, w: s.canvas.width, h: s.canvas.height, oy: s.oy, scale: PX_PER_TWIP * e.zoom * s.bs }));
  };
  const scrolled = (): void => {
    const e = el();
    if (!e || !continuous()) return;
    const again = new Set<number>();
    e.canvases.forEach((cv, i) => {
      if (!tall(cv)) return;
      if (!e.allocatedPages.has(i)) { if (strips.has(i)) release(i); return; }
      const want = wanted(e, cv, i);
      const have = strips.get(i) ?? [];
      if (want.length !== have.length || want.some((k) => !have.some((s) => s.k === k))) again.add(i);
    });
    if (again.size) (c.ed as unknown as { paint(f?: Set<number>): void }).paint(again);
  };
  return {
    tiles,
    scrolled,
    reset: () => { for (const i of [...strips.keys()]) release(i); },
    stripsOf: (i) => (strips.get(i) ?? []).map((s) => ({ cssW: s.cssW, cssH: s.cssH, w: s.canvas.width, h: s.canvas.height })),
    cutsOf: (i) => { const cv = el()?.canvases[i]; return cv && tall(cv) ? cutsOf(i, parseFloat(cv.style.height) || 0) : []; },
  };
}
