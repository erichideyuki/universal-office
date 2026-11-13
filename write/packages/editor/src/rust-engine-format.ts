// Handlers da frente A1 da leva 7 do engine="rust" (Arquivo, Exibir, Fonte e Parágrafo): cada chave é um nome de
// ROUTES_FORMAT.public/private (engine-routes-format.ts). Carregado só com o motor Rust (import estático de rust-engine.ts).
// Registro: core/docs/modules/uo-bindings.md "Wave 7 — A1". Os módulos rust-format-*.ts fazem o trabalho; aqui eles são
// juntados e os ganchos do motor (tecla, ponteiro, pintura da base, do conteúdo e por cima, documento novo) registrados uma
// vez. `prev` são os handlers das outras frentes (o setView da frente O para Imprimir e Leitura); `proto` o protótipo do
// elemento (o código TS de imprimir e das consultas da casca).
import type { RustEditor } from './rust-engine.js';
import { FormatCtx, type Handlers } from './rust-format-core.js';
import { fileHandlers } from './rust-format-file.js';
import { fxFromWire, textHandlers } from './rust-format-text.js';
import { viewHandlers } from './rust-format-view.js';
import { tiler } from './rust-format-tiles.js';

const hooked = new WeakSet<RustEditor>();
/** the marks painter of each editor (frontier R's `paintEngineMarks` calls it first: the TS order) */
const marksOf = new WeakMap<object, (ctx: CanvasRenderingContext2D, page: number) => void>();

/** The formatting marks of page `page` (the 1st half of the TS `paintEngineMarks`, ED-309): for rust-review-overlays.ts. */
export function paintFormattingMarks(ed: object, ctx: CanvasRenderingContext2D, page: number): void {
  marksOf.get(ed)?.(ctx, page);
}

export function formatHandlers(ed: RustEditor, prev: Record<string, unknown>, proto: object): Handlers {
  const c = new FormatCtx(ed, proto);
  const view = viewHandlers(c, prev);
  const out: Handlers = {
    ...textHandlers(c),
    ...fileHandlers(c, prev),
    ...view.handlers,
  };
  marksOf.set(ed, view.paintMarks);
  // File > Compare (D-A1-3): the Rust `diffDocuments` (uo-docx compare) over the two .docx; chrome.ts passes it to
  // <universal-write-diff>.loadPair as `diff` under engine="rust" (the element still reads both documents for its view)
  Object.defineProperty(ed, 'compareDiff', {
    configurable: true,
    value: (a: Uint8Array, b: Uint8Array): unknown => {
      const fb = c.fb;
      if (!fb) throw new Error('engine="rust": the compare exports are missing');
      return fb.compare(a, b);
    },
  });
  if (c.api && !hooked.has(ed) && (ed as { hooks?: unknown }).hooks) {
    hooked.add(ed);
    const hooks = ed.hooks;
    // Esc of the painter and of Select Objects first (universal-write.ts 10680: before the other fronts' Esc)
    hooks.key.unshift((e) => view.key(e));
    // the TOC tab: before the other fronts' pointer hooks (10181)
    hooks.pointer.unshift((e) => view.pointer(e));
    // the Format Painter at the end of the default text click (fix round 2: not on the clicks other fronts handle)
    hooks.clickUp.push(() => view.painterUp());
    // the document grid in the page base (under the content, never printed), keyed by its state
    hooks.under.push((ctx, page, scale) => view.paintBase(ctx, page, scale));
    hooks.baseKey.push(() => (c.st.grid ? '|grid' : ''));
    // the plugin inlines' own drawing: over the base, also in print (paintEngine 16710)
    hooks.content.push((ctx, page, scale) => view.paintContent(ctx, page, scale));
    // the TOC control and the lasso, over the selection (paintEngine 16724)
    hooks.overlay.push((ctx, page, scale) => view.paintOver(ctx, page, scale));
    hooks.adopt.push(() => view.adopt());
    // the Web/Draft/Outline sheet: shown to the end of the content (the viewer's webPageHeight), not the 633,600-twip sheet
    hooks.pageHeight = (i) => view.pageHeight(i);
    // wave 7c (V, fix round 2): a selection change in Web view lays it out again at the viewport's width when that changed
    // (the TS `refreshSelection` → `refresh()` without an engine line)
    if (hooks.selection) hooks.selection.push(() => view.selectionRelayout());
    // fix round 2: a sheet taller than a strip painted in strips of at most 4000 CSS px (the TS tiles, CONT_TILE_PX)
    const tl = tiler(c, () => { const v = (c.el as unknown as { view?: string } | null)?.view; return v === 'web' || v === 'draft' || v === 'outline'; });
    hooks.tiles = (i, cv) => tl.tiles(i, cv);
    hooks.adopt.push(() => tl.reset());
    c.el?.addEventListener('scroll', () => tl.scrolled(), { passive: true });
    Object.defineProperty(ed, 'continuousStrips', { configurable: true, value: (i: number) => tl.stripsOf(i) });
    // wave 7c (V): every strip cut of the continuous sheet (the TS tiles: `goToPage(n)` scrolls to tile n)
    Object.defineProperty(ed, 'continuousCuts', { configurable: true, value: (i: number) => tl.cutsOf(i) });
    // the toolbar state's text effects in the TS shape (`mods` as [name, value] pairs: the wire's tuples are { f0, f1 })
    hooks.state.push((v) => { const run = v['run'] as Record<string, unknown> | undefined; if (run?.['textFx']) run['textFx'] = fxFromWire(run['textFx']); });
  }
  // without a session (the simulated editor of the routing test) a handler answers its early return value
  const raw = new Set(['load', 'loadOdt', 'save', 'exportOdt', 'exportHtml', 'exportMarkdown', 'exportXps', 'print', 'printPages']);
  for (const [k, f] of Object.entries(out)) {
    if (raw.has(k)) continue; // these work (or throw, as the TS) without a document
    out[k] = (...a: never[]): unknown => {
      if (c.s) return f(...a);
      try { const r = f(...a); return r instanceof Promise ? r.catch(() => undefined) : r; } catch { return undefined; }
    };
  }
  return out;
}
