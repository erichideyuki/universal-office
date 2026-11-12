// Frente R da leva 5 do engine="rust": os adornos de tela da revisão (registro uo-bindings.md "Wave 5 — R"): o
// sombreamento de campo e o realce de comentário (a 2ª metade do `paintEngineMarks` do TS, 16805–16825), o realce das
// ocorrências da busca (`paintSearchHits`, 18477), pintados pelo gancho de pintura (twips da página), sob a seleção, nunca
// no PDF/impressão; cada um é o método privado do elemento com o MESMO nome do TS (`el[m] = () => {}` desliga os dois
// motores). Os trechos extras de uma seleção não contígua vão com a seleção, por cima dela (`paintExtraRanges`).
import { Ctx, SEL_FILL, charExtent, onLine, pageLines, type Pos } from './rust-review-core.js';
import type { FindView } from './rust-review-find.js';
import { runsOf } from './rust-review-revisions.js';

type Handlers = Record<string, (...args: never[]) => unknown>;

/** The overlays' element methods, in the TS paint order (paintEngine 16714–16724). */
export const OVERLAY_METHODS = ['paintEngineMarks', 'paintEngineSpelling', 'paintSearchHits'] as const;

export function overlayHandlers(c: Ctx, view: FindView): Handlers {
  return {
    /** field shading and comment highlight (formatting marks: not painted, ED-309) */
    paintEngineMarks: ((ctx: CanvasRenderingContext2D, page: number) => {
      if (!c.s || !c.dl) return;
      const dark = !!c.el?.pageDark;
      const runs = new Map<number, ReturnType<typeof runsOf>>();
      for (const l of pageLines(c, page)) {
        let rs = runs.get(l.para);
        if (!rs) { rs = runsOf(c, l.para).filter((r) => r.field || r.commentIds?.length); runs.set(l.para, rs); }
        if (!rs.length) continue;
        onLine(ctx, l, () => {
          for (const r of rs!) {
            const ext = charExtent(l, r.a, r.z);
            if (!ext) continue;
            ctx.fillStyle = r.commentIds?.length ? 'rgba(255, 187, 0, 0.28)' : dark ? 'rgba(255,255,255,0.14)' : 'rgba(0,0,0,0.10)';
            ctx.fillRect(ext[0], l.y, ext[1] - ext[0], l.height);
          }
        });
      }
    }) as never,
    /** the search hits (the Navigation pane's results, else the last `highlightMatches`), body scope */
    paintSearchHits: ((ctx: CanvasRenderingContext2D, page: number) => {
      if (!view.show || !c.s || !c.dl) return;
      let hits: { para: number; start: number; end: number }[] = view.results;
      if (!hits.length) {
        const spans = c.q<{ f0: { t: string; para?: number }; f1: number; f2: number }[]>({ t: 'highlightSpans' }) ?? [];
        hits = spans.filter((s) => s.f0.t === 'body').map((s) => ({ para: s.f0.para ?? 0, start: s.f1, end: s.f2 }));
      }
      if (!hits.length) return;
      const dark = !!c.el?.pageDark;
      ctx.save();
      ctx.fillStyle = dark ? 'rgba(255, 213, 0, 0.45)' : 'rgba(255, 230, 0, 0.6)';
      ctx.globalCompositeOperation = dark ? 'screen' : 'multiply';
      for (const l of pageLines(c, page)) {
        for (const h of hits) {
          if (h.para !== l.para || h.end <= l.start || h.start >= l.end) continue;
          onLine(ctx, l, () => {
            const ext = charExtent(l, h.start, h.end);
            if (ext) ctx.fillRect(ext[0], l.y, ext[1] - ext[0], l.height);
          });
        }
      }
      ctx.restore();
    }) as never,
  };
}

/**
 * The extra ranges of a non-contiguous selection (the main one is the shell's selection), painted with the selection (the
 * `overlay` hook, over it) as the TS paints `extras()` in `paintEngineSelection`. Not under a TS private name: the TS
 * `paintMultiSelection` is the floating objects' multi-selection and lasso (frontier O).
 */
export function paintExtraRanges(c: Ctx, ctx: CanvasRenderingContext2D, page: number): void {
  const s = c.s, dl = c.dl, el = c.el;
  if (!s || !dl || !el) return;
  const ranges = c.q<{ start: Pos; end: Pos }[]>({ t: 'selectionRanges' }) ?? [];
  if (ranges.length < 2) return;
  const { anchor, focus } = el.selection;
  const same = (r: { start: Pos; end: Pos }): boolean => {
    const a = (p: Pos, q: Pos): boolean => p.para === q.para && p.offset === q.offset;
    return (a(r.start, anchor) && a(r.end, focus)) || (a(r.start, focus) && a(r.end, anchor));
  };
  ctx.save();
  ctx.fillStyle = SEL_FILL;
  for (const r of ranges) {
    if (same(r)) continue;
    for (const x of s.rangeRects(dl.inner, page, r.start, r.end)) {
      if (x.xf) { ctx.save(); ctx.transform(x.xf[0]!, x.xf[1]!, x.xf[2]!, x.xf[3]!, x.xf[4]!, x.xf[5]!); }
      ctx.fillRect(x.x, x.y, Math.max(0, x.w), x.h);
      if (x.xf) ctx.restore();
    }
  }
  ctx.restore();
}
