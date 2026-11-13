// Handlers da frente A2 da leva 7 do engine="rust" (listas, colar, tabelas, objetos e a página): cada chave é um nome de
// ROUTES_COMPLETION.public (engine-routes-completion.ts). Carregado só com o motor Rust (import estático de rust-engine.ts).
// Registro: core/docs/modules/uo-bindings.md "Wave 7 — A2: lists, paste, tables, objects and the page". Os módulos:
// rust-completion-core.ts (estado, sessão), -lists.ts, -paste.ts, -tables.ts, -objects.ts. Os ganchos (ponteiro antes dos
// objetos, teclas, sobreposições, pintura das alças) entram uma vez por editor.
import type { FrontApi, RustEditor } from './rust-engine.js';
import { cstate, type CState } from './rust-completion-core.js';
import { listsHandlers } from './rust-completion-lists.js';
import {
  objectsHandlersA2, objectsKey, paintObjects, paintWrapHandle, pickMouseDown, selectObjectsMouseDown, wrapPointAt, wrapPointsMouseDown,
} from './rust-completion-objects.js';
import { completionClipKinds, installDocumentPath, pasteHandlers, pushDocumentPath } from './rust-completion-paste.js';
import { completionTableStyleThumb as tableStyleThumb, previewHandlers } from './rust-completion-previews.js';
import { oleDblClick, paintTableAdorners, tableKey, tableMouseDown, tableMouseMove, tableStyleListNow, tablesHandlers } from './rust-completion-tables.js';
import { a2Hooks } from './rust-objects-pointer.js';

const hooked = new WeakSet<RustEditor>();

function install(c: CState): void {
  const ed = c.ed;
  a2Hooks.set(c.o, {
    first: (e) => pickMouseDown(c, e) || tableMouseDown(c, e) || selectObjectsMouseDown(c, e),
    wrap: (e) => {
      if (c.wrap === null) return false;
      if (wrapPointsMouseDown(c, e)) return true;
      // a click elsewhere ends the mode (the TS `endEditWrapPoints()`)
      c.wrap = null;
      c.wrapLive = null;
      return false;
    },
    key: (e) => tableKey(c, e) || objectsKey(c, e),
    paintHandle: (h, ctx, scale, dpr) => paintWrapHandle(c, h, ctx, scale, dpr),
    wrapHover: (e) => {
      if (wrapPointAt(c, e)) { (ed.el as unknown as HTMLElement).style.cursor = 'move'; return true; }
      return false;
    },
    tableHover: (e) => tableMouseMove(c, e),
  });
  // the table adorners, the lasso and the outlines of several objects: over the selection, under front O's handles
  const hk = ed.hooks as typeof ed.hooks & { overlay?: ((ctx: CanvasRenderingContext2D, page: number, scale: number) => void)[] };
  // wave 7 (B, SCR-1): the adorners go through the TS name on the element (instance shadowing, as front R's overlays), so a
  // host or harness that turns `paintTableAdorners` off on the element turns off the shell's too
  const elx = ed.el as unknown as { paintTableAdorners: (ctx: CanvasRenderingContext2D, page: number) => void };
  elx.paintTableAdorners = (ctx, page) => paintTableAdorners(c, ctx, page);
  (hk.overlay ?? hk.paint).unshift((ctx, page) => { elx.paintTableAdorners(ctx, page); paintObjects(c, ctx, page); });
  // the dblclick on an embedded spreadsheet runs before the other double clicks (the TS listener 10466)
  ed.hooks.dbl.unshift((e) => oleDblClick(c, e));
  ed.hooks.adopt.push(() => {
    c.tableHover = null; c.tableDrag = null; c.wrap = null; c.wrapLive = null; c.picking = false; c.lasso = null; c.multiDrag = null;
    c.pushedPath = null;
    pushDocumentPath(c);
  });
  installDocumentPath(c);
}

export function completionHandlers(ed: RustEditor): Record<string, (...args: never[]) => unknown> {
  const c = cstate(ed);
  if (c.api && !hooked.has(ed) && (ed as { hooks?: unknown }).hooks) {
    hooked.add(ed);
    install(c);
  }
  const all: Record<string, (...args: never[]) => unknown> = { ...listsHandlers(ed), ...pasteHandlers(ed), ...tablesHandlers(ed), ...objectsHandlersA2(ed), ...previewHandlers(ed) };
  // without a session (before the first document, the simulated editor of the vitest) a handler answers its early return
  for (const [k, f] of Object.entries(all)) {
    all[k] = (...a: never[]): unknown => {
      if (c.api?.session) return f(...a);
      try { const r = f(...a); return r instanceof Promise ? r.catch(() => undefined) : r; } catch { return undefined; }
    };
  }
  return all;
}

/** Front S's `tableStyleListNow` (rust-engine-stories.ts): the session's gallery, sorted as the TS. */
export function completionTableStyles(ed: RustEditor): unknown[] | null {
  return tableStyleListNow(cstate(ed));
}

/** Front R's `pasteSpecial` with `link` from another editor (decision 3). */
export { completionPasteLink } from './rust-completion-paste.js';

/** Front S's `paintHeaderFooterTemplate` (rust-engine-stories.ts, decision 5): the session's mini-document. */
export { completionHfThumb } from './rust-completion-previews.js';

/** Front S's `renderTableStyleThumb` (rust-engine-stories.ts): the session's mini-document (rust-completion-previews.ts). */
export const completionTableStyleThumb = tableStyleThumb;

/** Front S's `getTableTool` (rust-engine-stories.ts): the shell's tool (the TS state). */
export function completionTableTool(ed: RustEditor): 'painter' | 'eraser' | 'draw' | null {
  const el = ed?.el as unknown as Record<string, unknown> | undefined;
  if (!el) return null;
  const proto = Object.getPrototypeOf(el) as { getTableTool?: () => 'painter' | 'eraser' | 'draw' | null };
  try { return proto.getTableTool?.call(el) ?? null; } catch { return null; }
}

/** Front R's `clipboardItems` (rust-review-clipboard.ts): the kind of each ring item (drawing, table, image, text). */
export function completionRingKinds<T extends { id: number; kind: string }>(api: FrontApi | null | undefined, items: T[]): T[] {
  if (!api?.module) return items;
  completionClipKinds({ api } as CState, items);
  return items;
}
