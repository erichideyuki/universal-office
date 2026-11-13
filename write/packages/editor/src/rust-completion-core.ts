// Wave 7 (A2) of the editor switch (record core/docs/modules/uo-bindings.md "Wave 7 — A2: lists, paste, tables, objects and
// the page"): the per-editor state of the front and its access to the session (`bindCompletion` of
// core/crates/uo-wasm/js/session-completion.mjs, front S's stories binding, front O's object state). Loaded only with the Rust
// engine.
import {
  bindCompletion, bindStories, type CompletionBinding, type CompletionEdit, type ExecOutcome, type SessionCommand, type StoriesBinding,
  type StoryHop,
} from '@universal-office/write-session';
import type { FrontApi, RustEditor } from './rust-engine.js';
import { stateOf as objState, type ObjState } from './rust-objects-core.js';

export const PX_PER_TWIP = 96 / 1440;

/** A table drag in progress (the TS `tableDrag`). */
export interface TableDrag {
  kind: 'col' | 'row' | 'move' | 'size' | 'draw';
  anchor: number;
  path: StoryHop[];
  boundary: number;
  pg: number;
  sx: number; sy: number; x: number; y: number;
  min?: number; max?: number;
}

/** The front's state of one editor. */
export interface CState {
  readonly ed: RustEditor;
  readonly api: FrontApi;
  readonly o: ObjState;
  cb: CompletionBinding | null;
  sb: StoriesBinding | null;
  /** the body table under the pointer (the move/size handles; `tableHover`) */
  tableHover: number | null;
  tableDrag: TableDrag | null;
  /** Edit Wrap Points: the drawing id of the picture (`wrapEditRun`) */
  wrap: number | null;
  /** the live polygon of a point drag (painted before the commit) */
  wrapLive: [number, number][] | null;
  /** Set Transparent Color: the next click picks (`pickingTransparent`) */
  picking: boolean;
  /** Select Objects: the lasso (twips of `page`) and the drag of several objects */
  lasso: { page: number; x: number; y: number; w: number; h: number } | null;
  multiDrag: { dx: number; dy: number } | null;
  /** the document path (`documentPath`: Paste Link) pushed to the session */
  pushedPath: string | null;
}

const states = new WeakMap<RustEditor, CState>();

export function cstate(ed: RustEditor): CState {
  let s = states.get(ed);
  if (!s) {
    const api = typeof (ed as { frontApi?: unknown }).frontApi === 'function' ? ed.frontApi() : (null as unknown as FrontApi);
    s = { ed, api, o: objState(ed), cb: null, sb: null, tableHover: null, tableDrag: null, wrap: null, wrapLive: null, picking: false, lasso: null, multiDrag: null, pushedPath: null };
    states.set(ed, s);
  }
  return s;
}

/** The completion binding (null before a module or in the vitest's simulated editor). */
export function cb(c: CState): CompletionBinding | null {
  if (c.cb) return c.cb;
  const mod = c.api?.module;
  if (!mod) return null;
  try { c.cb = bindCompletion(mod.exports, mod.binding); } catch { c.cb = null; }
  return c.cb;
}

/** Front S's stories binding (read-only here). */
export function sb(c: CState): StoriesBinding | null {
  if (c.sb) return c.sb;
  const mod = c.api?.module;
  if (!mod) return null;
  try { c.sb = bindStories(mod.exports, mod.binding); } catch { c.sb = null; }
  return c.sb;
}

export const session = (c: CState) => c.api?.session ?? null;
export const locked = (c: CState): boolean => !c.api?.session || c.api.locked();

/** A session command applied (`apply`: selection, history, relayout, events, bars); true when it ran. */
export function command(c: CState, cmd: SessionCommand, what: string): boolean {
  if (!c.api?.session) return false;
  return c.api.command(cmd, what);
}

/** A shell edit applied; true when it ran (`done`). Only the edits whose TS path goes through `insertTable` (the Quick
 * Tables, Draw Table's new table) scroll the caret into view: the TS table gestures (`editTableAt` + `refresh`), the drag
 * commits, the wrap points, the multi-selection and the OLE edit do not scroll (f2-3-0|tables step 24: the shell's scroll
 * after the size drag moved the page under the move handle's pointer). */
export function edit(c: CState, e: CompletionEdit, what: string): boolean {
  const s = c.api?.session, b = cb(c);
  if (!s || !b) return false;
  let o: ExecOutcome | null = null;
  try { o = b.edit(s, e); } catch (err) { c.api.warnOnce(`edit:${what}`, `[universal-write] engine="rust": ${what} failed`, err); return false; }
  const t = (e as { t?: string }).t;
  // wave 7c (V): the mousedown and the no-move mouseup of a live gesture are the TS `snapshot()`/`discardSnapshot()`: the
  // dirty state, the undo flags and the events only — no paint, no proxy move, no `onSelectionChange`/`chromeSync` (the
  // frame-by-frame drags: the TS paints at its first `refresh()` of a move)
  if (t === 'liveBegin' || t === 'liveCancel') return applyBare(c, o, what);
  return t === 'filledTable' || t === 'drawNew' ? c.api.apply(o, what) : applyNoScroll(c, o, what);
}

/** `apply` with the paint, the proxy, the caret scroll and the selection callbacks shadowed for the call. */
function applyBare(c: CState, o: ExecOutcome | null, what: string): boolean {
  const ed = c.ed as unknown as Record<string, unknown> | undefined;
  const el = c.ed?.el as unknown as Record<string, unknown> | undefined;
  if (!ed || !el) return c.api.apply(o, what);
  const shadow: [Record<string, unknown>, string][] = [[ed, 'paint'], [ed, 'positionProxy'], [ed, 'scrollCaretIntoView'], [el, 'onSelectionChange'], [el, 'chromeSync']];
  const kept = shadow.map(([obj, k]) => [Object.prototype.hasOwnProperty.call(obj, k), obj[k]] as const);
  for (const [obj, k] of shadow) obj[k] = k === 'onSelectionChange' || k === 'chromeSync' ? null : () => {};
  try { return c.api.apply(o, what); } finally {
    shadow.forEach(([obj, k], i) => { const [own, v] = kept[i]!; if (own) obj[k] = v; else delete obj[k]; });
  }
}

/** `apply` without the caret scroll (`scrollCaretIntoView` → `animateScrollTo` shadowed for the call): the TS paths that
 * only `refresh()`. */
export function applyNoScroll(c: CState, o: ExecOutcome | null, what: string): boolean {
  const el = c.ed?.el as unknown as { animateScrollTo?: (top: number) => void } | undefined;
  if (!el) return c.api.apply(o, what);
  const own = Object.prototype.hasOwnProperty.call(el, 'animateScrollTo') ? el.animateScrollTo : undefined;
  el.animateScrollTo = () => {};
  try { return c.api.apply(o, what); } finally { if (own) el.animateScrollTo = own; else delete el.animateScrollTo; }
}

/** A query of the binding, null without a session or on a failure (warned once). */
export function ask<T>(c: CState, what: string, f: (b: CompletionBinding, s: NonNullable<FrontApi['session']>) => T): T | null {
  const s = c.api?.session, b = cb(c);
  if (!s || !b) return null;
  try { return f(b, s); } catch (err) { c.api.warnOnce(`ask:${what}`, `[universal-write] engine="rust": ${what} failed`, err); return null; }
}

/** The TS method on the element (`shellThrough` of front S): its body only reads the shell's state or calls routed methods. */
export function through(c: CState, name: string, args: unknown[]): unknown {
  const el = c.ed?.el;
  if (!el) return undefined;
  const proto = Object.getPrototypeOf(el) as Record<string, (...a: unknown[]) => unknown>;
  const f = proto[name];
  try {
    return typeof f === 'function' ? f.apply(el, args) : undefined;
  } catch (e) {
    c.api?.warnOnce(`through:${name}`, `[universal-write] engine="rust": ${name} (the TS shell code) failed`, e);
    return undefined;
  }
}

/** The page point of an event (twips; `engPoint`/`pagePointTw` of the TS), or null off the pages. */
export function pagePoint(c: CState, e: MouseEvent): { pg: number; x: number; y: number } | null {
  const el = c.ed.el as unknown as { canvases: HTMLCanvasElement[]; zoom: number };
  const scale = PX_PER_TWIP * el.zoom;
  for (let pg = 0; pg < el.canvases.length; pg++) {
    const rc = el.canvases[pg]!.getBoundingClientRect();
    if (e.clientY < rc.top || e.clientY > rc.bottom || e.clientX < rc.left || e.clientX > rc.right) continue;
    return { pg, x: (e.clientX - rc.left) / scale, y: (e.clientY - rc.top) / scale };
  }
  return null;
}

/** One pixel in page twips at the current zoom. */
export const pxTw = (c: CState): number => 1 / (PX_PER_TWIP * (c.ed.el as unknown as { zoom: number }).zoom);

/** `onSelectionChange` + `chromeSync` of the element. */
export function sync(c: CState): void {
  const el = c.ed.el as unknown as { onSelectionChange?: (() => void) | null; chromeSync?: (() => void) | null };
  el.onSelectionChange?.();
  el.chromeSync?.();
}
