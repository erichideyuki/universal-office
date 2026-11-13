// Frente R da leva 5 do engine="rust": os adornos de tela da revisão (registro uo-bindings.md "Wave 5 — R"): o
// sombreamento de campo e o realce de comentário (a 2ª metade do `paintEngineMarks` do TS, 16805–16825), o realce das
// ocorrências da busca (`paintSearchHits`, 18477), pintados pelo gancho de pintura (twips da página), sob a seleção, nunca
// no PDF/impressão; cada um é o método privado do elemento com o MESMO nome do TS (`el[m] = () => {}` desliga os dois
// motores). Os trechos extras de uma seleção não contígua vão com a seleção, por cima dela (`paintExtraRanges`).
import { Ctx, SEL_FILL, charExtent, searchLocKey, storyCtx, storyLines, type Pos, type StoryLineP } from './rust-review-core.js';
import type { FindView } from './rust-review-find.js';
import { paintFormattingMarks } from './rust-engine-format.js'; // wave 7 (A1)
import { runsOf } from './rust-review-revisions.js'; // wave 7c (V, fix round 1)

type Handlers = Record<string, (...args: never[]) => unknown>;

/** The overlays' element methods, in the TS paint order (paintEngine 16714–16724). */
export const OVERLAY_METHODS = ['paintEngineMarks', 'paintBookmarkMarks', 'paintEngineSpelling', 'paintSearchHits'] as const; // wave 7 (A1): the bookmark marks (ED-310 closed)

export function overlayHandlers(c: Ctx, view: FindView): Handlers {
  return {
    /** the formatting marks (wave 7, A1: ED-309 closed), the field shading and the comment highlight — over EVERY story's
     *  lines, the TS `eng.lines` (16770–16795; wave 7c, V: ED-304 closed), the text boxes' content excluded as in the TS */
    paintEngineMarks: ((ctx: CanvasRenderingContext2D, page: number) => {
      if (!c.s || !c.dl) return;
      paintFormattingMarks(c.ed, ctx, page);
      // wave 7c (V, fix round 1): Word's look is painted under the text (`paintWordMarks`); the TS's here (`tsShading`)
      if (!tsShading(c)) return;
      const dark = !!c.el?.pageDark;
      for (const l of storyLines(c, page)) {
        if (l.kind === 'tb') continue;
        for (const r of l.runs) {
          const comment = !!r.c?.length;
          if (!r.f && !comment) continue;
          const ext = charExtent(l, r.a, r.z);
          if (!ext) continue;
          ctx.fillStyle = comment ? 'rgba(255, 187, 0, 0.28)' : dark ? 'rgba(255,255,255,0.14)' : 'rgba(0,0,0,0.10)';
          ctx.fillRect(ext[0], l.y, ext[1] - ext[0], l.height);
        }
      }
    }) as never,
    /** the search hits (the Navigation pane's results, else the last `highlightMatches`), keyed by the story paragraph as
     *  the TS `searchHits` (a `Paragraph` → ranges map, 18440): body and table cells, whatever the edit scope */
    paintSearchHits: ((ctx: CanvasRenderingContext2D, page: number) => {
      if (!view.show || !c.s || !c.dl) return;
      let hits: { key: string; start: number; end: number }[] = view.results;
      if (!hits.length) {
        const spans = c.q<{ f0: { t: string; para?: number; anchor?: number; hops?: { row: number; col: number; block?: number }[] }; f1: number; f2: number }[]>({ t: 'highlightSpans' }) ?? [];
        hits = spans.map((s) => ({ key: searchLocKey(s.f0), start: s.f1, end: s.f2 }));
      }
      if (!hits.length) return;
      const dark = !!c.el?.pageDark;
      ctx.save();
      ctx.fillStyle = dark ? 'rgba(255, 213, 0, 0.45)' : 'rgba(255, 230, 0, 0.6)';
      ctx.globalCompositeOperation = dark ? 'screen' : 'multiply';
      for (const l of storyLines(c, page)) {
        if (l.kind === 'tb') continue;
        for (const h of hits) {
          if (h.key !== l.key || h.end <= l.start || h.start >= l.end) continue;
          const ext = charExtent(l, h.start, h.end);
          if (ext) ctx.fillRect(ext[0], l.y, ext[1] - ext[0], l.height);
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

// ───────────────────────── wave 7c (V, fix round 1): Word's field shading and comment highlight (ED-304) ─────────────────────────
//
// Word manda sobre o TS (measured: Word 16, the screen through PrintWindow, record "Wave 7c — V", fix round 1): the field
// shading is opaque C0C0C0 BEHIND the glyphs (on a white page and on a dark #202020 page alike), only while the
// selection is in the field (View > Field shading "When selected", Word's default), over the field result's characters
// and the line's natural text height from the line top (17 px for 11-pt Liberation Sans, with 1.5 or 1.08 line spacing
// alike; 31 px when a 20-pt run shares the line); a commented range is a band of E8E8E8 (787878 on the dark page) of the
// same height between two 1-px brackets (424242; white on the dark page) at its first and last character. The frozen TS
// paints rgba(0,0,0,.10) over the whole line height for every field and rgba(255,187,0,.28) for comments, over the text:
// kept under `tsShading` (the strict replays against the TS captures).

/** the TS look (`ed.tsStrict` or `ed.tsShading`, set by the harnesses' strict replays) */
export function tsShading(c: Ctx): boolean {
  const e = c.ed as unknown as { tsStrict?: boolean; tsShading?: boolean };
  return !!(e.tsStrict || e.tsShading);
}

/**
 * the story paragraph key of the selection's paragraph and its range there, or null — body and table cells by their key;
 * wave 7c (V, fix round 2): a header/footer, a footnote/endnote and a text box by the story line the caret is on (the
 * story's own lines of `uo_shell_story_lines`, kinds 'hf', 'note', 'tb': the line of the caret's page and frame that holds
 * the caret point and the focus offset)
 */
function selectionKey(c: Ctx): { key: string; a: number; z: number } | null {
  const el = c.el;
  if (!el) return null;
  const { anchor, focus } = el.selection;
  if (anchor.para !== focus.para) return null;
  const a = Math.min(anchor.offset, focus.offset), z = Math.max(anchor.offset, focus.offset);
  const x = storyCtx(c);
  if (!x || x.kind === 'body') return { key: `b${focus.para}`, a, z };
  if (x.kind === 'cell') return { key: searchLocKey({ t: 'cell', para: focus.para, anchor: x.anchor, hops: x.hops.map((h) => ({ row: h.r, col: h.c, ...(h.b !== undefined ? { block: h.b } : {}) })) }), a, z };
  const kind = x.kind === 'hf' ? 'hf' : x.kind === 'note' ? 'note' : 'tb';
  const car = c.api?.caret();
  if (!car) return null;
  const sameXf = (p?: number[], q?: number[]): boolean => (!p && !q) || (!!p && !!q && p.every((v, i) => Math.abs(v - q[i]!) < 0.01));
  const mid = car.y + car.h / 2;
  const l = storyLines(c, car.page).find((ln) => ln.kind === kind && sameXf(ln.xf, car.xf) && mid >= ln.y - 0.5 && mid <= ln.y + ln.height + 0.5
    && focus.offset >= ln.start && focus.offset <= ln.end && car.x >= ln.x - 15 && car.x <= ln.x + ln.width + 15);
  return l ? { key: l.key, a, z } : null;
}

/** the field run the selection is in ("When selected"): a caret inside it, or a range that overlaps it */
function selectedField(c: Ctx, l: StoryLineP, sel: { key: string; a: number; z: number } | null): number {
  if (!sel || sel.key !== l.key) return -1;
  return l.runs.findIndex((r) => r.f && (sel.a === sel.z ? sel.a >= r.a && sel.a < r.z : sel.a < r.z && sel.z > r.a));
}

/**
 * the line's natural text height from its top (Word's band): the ascent (baseline − top: multiple spacing puts its extra
 * below the text, at-least spacing above it, `line_box`) plus the line's natural text descent (fix round 2: the engine's
 * `Line::desc`, `desc` of the story lines — no longer estimated), at most what is below the baseline; the whole line for
 * exact spacing (baseline at 0.8 × height)
 */
function bandHeight(l: StoryLineP): number {
  const asc = l.baseline - l.y;
  if (Math.abs(asc - l.height * 0.8) < 0.01) return l.height;
  const below = l.height - asc;
  return asc + Math.min(below, Number.isFinite(l.desc) ? l.desc : below);
}

/** the base key of Word's marks: the field the selection is in (the page bases are painted again when it changes) */
const keyedSel = new WeakMap<Ctx, { key: string; a: number; z: number } | null>();
export function wordMarksKey(c: Ctx): string {
  if (tsShading(c) || !c.s || !c.dl) return '';
  const sel = selectionKey(c);
  // fix round 2 (V): the base is painted with the selection that keyed it (`baseOf` reads the key right before the paint)
  keyedSel.set(c, sel);
  const para = c.el?.selection.focus.para;
  if (!sel || para === undefined) return '|fs:-';
  // the field run of the selection's paragraph: moving the caret inside it keeps the base. Fix round 2 (V): the runs of the
  // story paragraph the key names (the story lines' — `runsOf` caches by layout and paragraph index only, so after the
  // body's paragraph 0 a footer's paragraph 0 read the body's runs)
  const car = c.api?.caret();
  const line = car ? storyLines(c, car.page).find((l) => l.key === sel.key) : undefined;
  const runs = line ? line.runs.map((r) => ({ field: r.f, a: r.a, z: r.z })) : runsOf(c, para);
  const i = runs.findIndex((r) => r.field && (sel.a === sel.z ? sel.a >= r.a && sel.a < r.z : sel.a < r.z && sel.z > r.a));
  return i < 0 ? '|fs:-' : `|fs:${sel.key}:${i}`;
}

/** Word's field shading and comment highlight, UNDER the text (the `under` hook of the page base; page twips) */
export function paintWordMarks(c: Ctx, ctx: CanvasRenderingContext2D, page: number, scale: number): void {
  if (tsShading(c) || !c.s || !c.dl || c.el?.printRender) return;
  const dark = !!c.el?.pageDark;
  const sel = keyedSel.has(c) ? keyedSel.get(c)! : selectionKey(c);
  const px = (v: number): number => Math.round(v * scale) / scale;
  const one = 1 / scale;
  ctx.save();
  // fix round 2 (V): every story's lines, the text boxes' content too (Word shades a field and marks a comment wherever the
  // text is; the TS look keeps its exclusion of the boxes, `paintEngineMarks` above)
  for (const l of storyLines(c, page)) {
    const y0 = px(l.y), y1 = px(l.y + bandHeight(l));
    const band = (a: number, z: number, fill: string): [number, number] | null => {
      const ext = charExtent(l, a, z);
      if (!ext) return null;
      const x0 = px(ext[0]), x1 = px(ext[1]);
      ctx.fillStyle = fill;
      ctx.fillRect(x0, y0, x1 - x0, y1 - y0);
      return [x0, x1];
    };
    const fi = selectedField(c, l, sel);
    l.runs.forEach((r, i) => {
      if (r.c?.length) {
        const ext = band(r.a, r.z, dark ? '#787878' : '#e8e8e8');
        if (ext) {
          ctx.fillStyle = dark ? '#ffffff' : '#424242';
          const prev = l.runs[i - 1], next = l.runs[i + 1];
          const startsHere = !prev?.c?.length && r.a >= l.start, endsHere = !next?.c?.length && r.z <= l.end;
          if (startsHere) ctx.fillRect(ext[0], y0, one, y1 - y0);
          if (endsHere) ctx.fillRect(ext[1] - one, y0, one, y1 - y0);
        }
      }
      if (i === fi) band(r.a, r.z, '#c0c0c0');
    });
  }
  ctx.restore();
}
