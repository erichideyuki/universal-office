// Wave 7 (A2): the table gestures and dialogs left by front S (universal-write.ts 19918–21010): the Border Painter, the
// Eraser and Draw Table (`tableToolClick`, `finishDraw`), the edge drag and the move/size handles with their adorners
// (`tableMouseDown`, `tableMouseMove`, `paintTableAdorners`, ED-219), the Quick Tables (the content built here as the TS
// builds it, the table edit in the session), the table style gallery and the New/Modify Table Style dialog's queries, Insert
// › Excel Spreadsheet. The edits go through the session's shell edits (`uo_shell_edit`); the geometry through front S's and
// this front's layout exports. Record core/docs/modules/uo-bindings.md "Wave 7 — A2".
import { pickLabels, uiLang, uiLocale } from '@universal-office/ui';
import type { StoryHop } from '@universal-office/write-session';
import { blankXlsx, editSpreadsheet, sheetAvailable } from './ole-sheet.js';
import { applyNoScroll, ask, command, cstate, edit, locked, pagePoint, pxTw, sb, session, sync, through, type CState, type TableDrag } from './rust-completion-core.js';
import { storyCtx, outerCtx } from './rust-objects-core.js';
import type { RustEditor } from './rust-engine.js';

type Handlers = Record<string, (...args: never[]) => unknown>;
type Tool = 'painter' | 'eraser' | 'draw' | null;
interface PageCell { anchor: number; path: StoryHop[]; r: number; c: number; x: number; y: number; w: number; h: number; span: number }
interface TableRows { rowHeights: number[]; boxes: { pg: number; r: number; c: number; x: number; y: number; w: number; h: number }[] }

interface El {
  view: string; printRender?: boolean; pageDark?: boolean; style: CSSStyleDeclaration; shadowRoot: ShadowRoot | null;
  focusEditor(): void; selectTable?(): void; setSelection?(a: unknown, f: unknown): void; insertImage(...a: unknown[]): Promise<void>;
  sheetLoader?: (() => Promise<unknown>) | null; selection: { anchor: { para: number; offset: number }; focus: { para: number; offset: number } };
  dispatchEvent(e: Event): boolean;
}
const elOf = (c: CState): El => c.ed.el as unknown as El;

// ───────────────────────── the layout's cells (front S's exports + this front's) ─────────────────────────

function mod(c: CState): { exports: Record<string, (...a: unknown[]) => unknown>; binding: { render: { Buf: new (p: number) => { take(): { status: number; data: Uint8Array } }; put(b: Uint8Array): number } } } | null {
  return (c.api?.module as never) ?? null;
}

/** A JSON export of src/session/shell_stories.rs (wave 7 block) over the session and the current layout. */
function layoutJson<T>(c: CState, name: string, ...args: unknown[]): T | null {
  const m = mod(c), s = session(c), dl = c.api?.dl;
  if (!m || !s || !dl) return null;
  const f = m.exports[name];
  if (typeof f !== 'function') return null;
  try {
    const { status, data } = new m.binding.render.Buf(f((s as unknown as { ptr: number }).ptr, (dl.inner as unknown as { ptr: number }).ptr, ...args) as number).take();
    return status === 0 ? (JSON.parse(new TextDecoder().decode(data)) as T) : null;
  } catch { return null; }
}

/** A UTF-8 input buffer for an export (`[ptr, len]`). */
function textArg(c: CState, s: string): [number, number] {
  const b = new TextEncoder().encode(s);
  return b.length ? [mod(c)!.binding.render.put(b), b.length] : [0, 0];
}

const hopsJson = (path: StoryHop[]): string => JSON.stringify(path.map((h) => [h.r, h.c, h.b ?? -1]));

function pageCells(c: CState, pg: number): PageCell[] {
  return layoutJson<PageCell[]>(c, 'uo_shell_page_cells', pg >>> 0) ?? [];
}

function tableRows(c: CState, anchor: number, path: StoryHop[]): TableRows | null {
  if (!mod(c)) return null;
  return layoutJson<TableRows>(c, 'uo_shell_table_rows', anchor >>> 0, ...textArg(c, hopsJson(path)));
}

/** `rowHeights(anchor, path, r1, r2)`. */
function rowHeights(c: CState, anchor: number, path: StoryHop[], r1: number, r2: number): number[] {
  const all = tableRows(c, anchor, path)?.rowHeights ?? [];
  const out: number[] = [];
  for (let r = r1; r <= r2; r++) out.push(all[r] ?? 0);
  return out;
}

/** `cellBoxAtPoint(pg, x, y)`: the innermost cell under the point. */
function cellAtPoint(c: CState, pg: number, x: number, y: number): PageCell | null {
  let best: PageCell | null = null;
  for (const b of pageCells(c, pg)) {
    if (x < b.x || x > b.x + b.w || y < b.y || y > b.y + b.h) continue;
    if (!best || b.path.length + 1 > best.path.length + 1) best = b;
  }
  return best;
}

interface TableEdge { kind: 'col' | 'row'; anchor: number; path: StoryHop[]; pg: number; r: number; c: number; boundary: number; pos: number }

/** `tableEdgeAt(e)` (universal-write.ts 20667). */
function tableEdgeAt(c: CState, e: MouseEvent): TableEdge | null {
  const p = pagePoint(c, e);
  if (!p) return null;
  const tol = 4 * pxTw(c);
  let best: (TableEdge & { d: number }) | null = null;
  for (const box of pageCells(c, p.pg)) {
    const base = { anchor: box.anchor, path: box.path, pg: p.pg, r: box.r, c: box.c };
    if (p.y >= box.y && p.y <= box.y + box.h) {
      for (const [x, b] of [[box.x, box.c], [box.x + box.w, box.c + box.span]] as const) {
        const d = Math.abs(p.x - x);
        if (d <= tol && (!best || d < best.d)) best = { ...base, kind: 'col', boundary: b, pos: x, d };
      }
    }
    if (p.x >= box.x && p.x <= box.x + box.w) {
      const y = box.y + box.h, d = Math.abs(p.y - y);
      if (d <= tol && (!best || d < best.d)) best = { ...base, kind: 'row', boundary: box.r, pos: y, d };
    }
  }
  if (!best) return null;
  const { d: _d, ...edge } = best;
  return edge;
}

/** `tableBBox(anchor)`: the first and last corner of a body table. */
function tableBBox(c: CState, anchor: number): { first: { pg: number; x: number; y: number }; last: { pg: number; x: number; y: number } } | null {
  const boxes = tableRows(c, anchor, [])?.boxes ?? [];
  if (!boxes.length) return null;
  const pg0 = Math.min(...boxes.map((b) => b.pg)), pg1 = Math.max(...boxes.map((b) => b.pg));
  const on0 = boxes.filter((b) => b.pg === pg0), on1 = boxes.filter((b) => b.pg === pg1);
  return {
    first: { pg: pg0, x: Math.min(...on0.map((b) => b.x)), y: Math.min(...on0.map((b) => b.y)) },
    last: { pg: pg1, x: Math.max(...on1.map((b) => b.x + b.w)), y: Math.max(...on1.map((b) => b.y + b.h)) },
  };
}

/** `tableHandleAt(e)`. */
function tableHandleAt(c: CState, e: MouseEvent): { kind: 'move' | 'size'; anchor: number } | null {
  if (c.tableHover === null) return null;
  const bb = tableBBox(c, c.tableHover);
  const p = pagePoint(c, e);
  if (!bb || !p) return null;
  const px = pxTw(c);
  if (p.pg === bb.first.pg && p.x >= bb.first.x - 20 * px && p.x <= bb.first.x - 4 * px && p.y >= bb.first.y - 20 * px && p.y <= bb.first.y - 4 * px) return { kind: 'move', anchor: c.tableHover };
  if (p.pg === bb.last.pg && p.x >= bb.last.x && p.x <= bb.last.x + 10 * px && p.y >= bb.last.y && p.y <= bb.last.y + 10 * px) return { kind: 'size', anchor: c.tableHover };
  return null;
}

const tool = (c: CState): Tool => (through(c, 'getTableTool', []) as Tool) ?? null;

/** The TS gate of `tableMouseDown`/`tableMouseMove`: the print view, no header/footer or text box in edit, a layout. */
function tableScope(c: CState, needEditable: boolean): boolean {
  const el = elOf(c);
  if (!c.api?.session || !c.api.dl || el.view !== 'print') return false;
  if (needEditable && c.api.locked()) return false;
  const ctx = storyCtx(c.o);
  return !ctx || (ctx.ctx.kind !== 'textbox' && outerCtx(ctx.ctx).kind !== 'hf');
}

// ───────────────────────── the gestures ─────────────────────────

function tableToolClick(c: CState, e: MouseEvent, t: Tool): void {
  const p = pagePoint(c, e);
  const hit = p ? cellAtPoint(c, p.pg, p.x, p.y) : null;
  if (!hit || !p) return;
  const ds: ['top' | 'bottom' | 'left' | 'right', number][] = [['top', Math.abs(p.y - hit.y)], ['bottom', Math.abs(p.y - hit.y - hit.h)], ['left', Math.abs(p.x - hit.x)], ['right', Math.abs(p.x - hit.x - hit.w)]];
  const side = ds.sort((a, z) => a[1] - z[1])[0]![0];
  if (t === 'painter') {
    const pen = (through(c, 'getBorderPen', []) as Record<string, unknown> | undefined) ?? { val: 'single', sz: 4, space: 0, color: 'auto' };
    edit(c, { t: 'paint', anchor: hit.anchor, path: hit.path, r: hit.r, c: hit.c, side, pen: Object.fromEntries(Object.entries(pen).filter(([, v]) => v !== undefined)) }, 'tableToolClick');
    return;
  }
  edit(c, { t: 'erase', anchor: hit.anchor, path: hit.path, r: hit.r, c: hit.c, side }, 'tableToolClick');
}

function finishTableDrag(c: CState, d: TableDrag, ev: MouseEvent): void {
  const dx = Math.round(d.x - d.sx), dy = Math.round(d.y - d.sy);
  if (d.kind === 'col' || d.kind === 'row') {
    const delta = d.kind === 'col' ? dx : dy;
    if (Math.abs(delta) < 2) { c.api.paint(); return; }
    const rh = d.kind === 'row' ? rowHeights(c, d.anchor, d.path, d.boundary, d.boundary)[0] ?? 0 : 0;
    if (!edit(c, { t: 'edge', col: d.kind === 'col', anchor: d.anchor, path: d.path, boundary: d.boundary, delta, rowHeight: rh }, 'resizeTableEdge')) c.api.paint();
    return;
  }
  if (d.kind === 'move') { edit(c, { t: 'move', anchor: d.anchor, dx, dy }, 'tableMove'); return; }
  if (d.kind === 'size') {
    const bb = tableBBox(c, d.anchor);
    if (!bb || (Math.abs(dx) < 2 && Math.abs(dy) < 2)) { c.api.paint(); return; }
    const rows = dy && bb.first.pg === bb.last.pg ? tableRows(c, d.anchor, [])?.rowHeights : undefined;
    edit(c, { t: 'size', anchor: d.anchor, dx, dy, ...(rows ? { rowHeights: rows } : {}) }, 'tableSize');
    return;
  }
  // `finishDraw`
  const w = Math.abs(d.x - d.sx), h = Math.abs(d.y - d.sy);
  if (w < 60 && h < 60) { c.api.paint(); return; }
  const start = cellAtPoint(c, d.pg, d.sx, d.sy);
  if (start) {
    const vertical = h >= w;
    const left = Math.max(100, Math.min(start.w - 100, Math.round((d.sx + d.x) / 2 - start.x)));
    edit(c, {
      t: 'drawSplit', anchor: start.anchor, path: start.path, r: start.r, c: start.c, vertical, left, boxW: start.w,
      rowHeights: rowHeights(c, start.anchor, start.path, start.r, start.r),
    }, 'finishDraw');
    return;
  }
  // outside a table: the caret at the point (not in a cell), then a fixed 1 × 1 table
  const ctx = storyCtx(c.o);
  if (!ctx || ctx.ctx.kind !== 'cell') {
    const pos = c.api.hitAt(ev);
    if (pos) elOf(c).setSelection?.(pos, pos);
  }
  edit(c, { t: 'drawNew', w, h }, 'finishDraw');
}

function trackTableDrag(c: CState, e0: MouseEvent): void {
  e0.preventDefault();
  const move = (ev: MouseEvent): void => {
    const d = c.tableDrag;
    const p = pagePoint(c, ev);
    if (!d || !p || p.pg !== d.pg) return;
    const clamp = (v: number): number => Math.max(d.min ?? -Infinity, Math.min(d.max ?? Infinity, v));
    if (d.kind === 'col') d.x = clamp(p.x);
    else if (d.kind === 'row') d.y = clamp(p.y);
    else { d.x = p.x; d.y = p.y; }
    c.api.paint();
  };
  const up = (ev: MouseEvent): void => {
    window.removeEventListener('mousemove', move);
    window.removeEventListener('mouseup', up);
    const d = c.tableDrag;
    c.tableDrag = null;
    if (!d) return;
    finishTableDrag(c, d, ev);
    sync(c);
  };
  window.addEventListener('mousemove', move);
  window.addEventListener('mouseup', up);
}

/** `tableMouseDown(e)` (universal-write.ts 20962): true = handled. Runs before the objects (the TS order). */
export function tableMouseDown(c: CState, e: MouseEvent): boolean {
  if (e.button !== 0 || !tableScope(c, true)) return false;
  const el = elOf(c);
  const t = tool(c);
  if (t === 'painter' || t === 'eraser') {
    e.preventDefault();
    el.focusEditor();
    tableToolClick(c, e, t);
    sync(c);
    return true;
  }
  if (t === 'draw') {
    const p = pagePoint(c, e);
    if (!p) return false;
    c.tableDrag = { kind: 'draw', anchor: -1, path: [], boundary: 0, pg: p.pg, sx: p.x, sy: p.y, x: p.x, y: p.y };
    el.focusEditor();
    trackTableDrag(c, e);
    return true;
  }
  const hd = tableHandleAt(c, e);
  if (hd) {
    const p = pagePoint(c, e)!;
    c.tableDrag = { kind: hd.kind, anchor: hd.anchor, path: [], boundary: 0, pg: p.pg, sx: p.x, sy: p.y, x: p.x, y: p.y };
    el.focusEditor();
    trackTableDrag(c, e);
    return true;
  }
  const edge = tableEdgeAt(c, e);
  if (edge) {
    el.focusEditor();
    // `startTableEdgeDrag`: the two columns next to it trade width (min 100 twips each); a row keeps 100
    const g = ask(c, 'tableGrid', (b, s) => b.tableGrid(s, edge.anchor, edge.path)) ?? [];
    let min = -Infinity, max = Infinity;
    const b = edge.boundary, MIN = 100;
    if (edge.kind === 'col') {
      if (b > 0) min = edge.pos - (g[b - 1] ?? 0) + MIN;
      if (b < g.length) max = edge.pos + (g[b] ?? 0) - MIN;
    } else {
      const h = rowHeights(c, edge.anchor, edge.path, b, b)[0] ?? 0;
      min = edge.pos - h + MIN;
    }
    c.tableDrag = { kind: edge.kind, anchor: edge.anchor, path: edge.path, boundary: b, pg: edge.pg, sx: edge.pos, sy: edge.pos, x: edge.pos, y: edge.pos, min, max };
    trackTableDrag(c, e);
    return true;
  }
  return false;
}

/** `tableMouseMove(e)` (universal-write.ts 20997): the hover table (handles) and the cursors; true = handled. */
export function tableMouseMove(c: CState, e: MouseEvent): boolean {
  // wave 7 (B, editor_switch_objects f7b__7__0|menu, f7b__7__8): the TS gate of the MOVE is `view === 'print' && !hfEdit &&
  // eng` — no `tbEdit` (only the mousedown has it): the hover table and its adorners follow the pointer in a text box too
  if (c.tableDrag || !tableScope(c, false)) {
    const ctx = storyCtx(c.o);
    if (c.tableDrag || ctx?.ctx.kind !== 'textbox' || outerCtx(ctx.ctx).kind === 'hf' || !c.api?.session || !c.api.dl || elOf(c).view !== 'print') return false;
  }
  const el = elOf(c);
  if (tool(c)) { el.style.cursor = 'crosshair'; return true; }
  const p = pagePoint(c, e);
  const hit = p ? cellAtPoint(c, p.pg, p.x, p.y) : null;
  const hover = hit && !hit.path.length ? hit.anchor : tableHandleAt(c, e)?.anchor ?? null;
  if (hover !== c.tableHover) { c.tableHover = hover; c.api.paint(); }
  if (c.api.locked()) return false;
  const hd = tableHandleAt(c, e);
  if (hd) { el.style.cursor = hd.kind === 'move' ? 'move' : 'nwse-resize'; return true; }
  const edge = tableEdgeAt(c, e);
  if (edge) { el.style.cursor = edge.kind === 'col' ? 'col-resize' : 'row-resize'; return true; }
  if (el.style.cursor === 'col-resize' || el.style.cursor === 'row-resize' || el.style.cursor === 'nwse-resize' || el.style.cursor === 'crosshair') el.style.cursor = '';
  return false;
}

/** `paintTableAdorners(ctx, pg)` (universal-write.ts 20716), in page twips. */
export function paintTableAdorners(c: CState, ctx: CanvasRenderingContext2D, pg: number): void {
  const el = elOf(c);
  const px = pxTw(c);
  const dark = !!el.pageDark;
  if (c.tableHover !== null && !el.printRender && !tool(c)) {
    const bb = tableBBox(c, c.tableHover);
    if (bb) {
      ctx.save();
      ctx.lineWidth = px;
      ctx.strokeStyle = dark ? '#9aa0a6' : '#5f6368';
      ctx.fillStyle = dark ? '#303134' : '#ffffff';
      if (bb.first.pg === pg) {
        const x = bb.first.x - 18 * px, y = bb.first.y - 18 * px, s = 13 * px;
        ctx.fillRect(x, y, s, s); ctx.strokeRect(x, y, s, s);
        const cx = x + s / 2, cy = y + s / 2, a = 4.5 * px;
        ctx.beginPath(); ctx.moveTo(cx - a, cy); ctx.lineTo(cx + a, cy); ctx.moveTo(cx, cy - a); ctx.lineTo(cx, cy + a); ctx.stroke();
      }
      if (bb.last.pg === pg) {
        const x = bb.last.x + 2 * px, y = bb.last.y + 2 * px, s = 7 * px;
        ctx.fillRect(x, y, s, s); ctx.strokeRect(x, y, s, s);
      }
      ctx.restore();
    }
  }
  const d = c.tableDrag;
  if (!d || d.pg !== pg) return;
  ctx.save();
  ctx.strokeStyle = dark ? '#8ab4f8' : '#1a73e8';
  ctx.lineWidth = px;
  ctx.setLineDash([3 * px, 3 * px]);
  ctx.beginPath();
  if (d.kind === 'col') { ctx.moveTo(d.x, 0); ctx.lineTo(d.x, 100000); } else if (d.kind === 'row') { ctx.moveTo(0, d.y); ctx.lineTo(100000, d.y); } else ctx.rect(Math.min(d.sx, d.x), Math.min(d.sy, d.y), Math.abs(d.x - d.sx), Math.abs(d.y - d.sy));
  ctx.stroke();
  ctx.restore();
}

/** Esc ends the table tool (universal-write.ts 10559). */
export function tableKey(c: CState, e: KeyboardEvent): boolean {
  if (e.key !== 'Escape' || !tool(c)) return false;
  e.preventDefault();
  through(c, 'setTableTool', [null]);
  return true;
}

// ───────────────────────── Quick Tables ─────────────────────────

/** The rows, style, merges of a Quick Table (`insertQuickTable`, universal-write.ts 20394: the same code). */
function quickTable(lang: 'en' | 'pt' | 'es', id: string): { rows: string[][]; style: string; merge: { r: number; c1: number; c2: number }[] } | null {
  const tr = (en: string, pt: string, es: string): string => pickLabels(lang, { en, pt, es });
  let rows: string[][];
  let style: string;
  let merge: { r: number; c1: number; c2: number }[] = [];
  switch (id) {
    case 'calendar': {
      const now = new Date();
      const y = now.getFullYear(), mo = now.getMonth();
      const month = new Intl.DateTimeFormat(uiLocale(lang), { month: 'long', year: 'numeric' }).format(now);
      const days = { pt: ['D', 'S', 'T', 'Q', 'Q', 'S', 'S'], en: ['S', 'M', 'T', 'W', 'T', 'F', 'S'], es: ['D', 'L', 'M', 'X', 'J', 'V', 'S'] }[lang];
      const first = new Date(y, mo, 1).getDay(), n = new Date(y, mo + 1, 0).getDate();
      rows = [[month.charAt(0).toUpperCase() + month.slice(1), '', '', '', '', '', ''], days];
      let week: string[] = Array(7).fill('');
      for (let d = 1, wd = first; d <= n; d++, wd++) {
        if (wd === 7) { rows.push(week); week = Array(7).fill(''); wd = 0; }
        week[wd] = String(d);
      }
      rows.push(week);
      merge = [{ r: 0, c1: 0, c2: 6 }];
      style = 'Grid Table 1 Light';
      break;
    }
    case 'tabular':
      rows = [[tr('Item', 'Item', 'Artículo'), tr('Needed', 'Necessário', 'Necesario')],
        [tr('Notebooks', 'Cadernos', 'Cuadernos'), '12'], [tr('Pens', 'Canetas', 'Bolígrafos'), '30'], [tr('Folders', 'Pastas', 'Carpetas'), '8'],
        [tr('Sticky notes', 'Blocos adesivos', 'Notas adhesivas'), '15'], [tr('Staplers', 'Grampeadores', 'Grapadoras'), '3']];
      style = 'List Table 4 Accent 1';
      break;
    case 'matrix':
      rows = [[tr('Region', 'Região', 'Región'), 'T1', 'T2', 'T3', 'T4'],
        [tr('North', 'Norte', 'Norte'), '120', '135', '150', '162'], [tr('South', 'Sul', 'Sur'), '98', '104', '117', '125'],
        [tr('East', 'Leste', 'Este'), '143', '139', '151', '170'], [tr('West', 'Oeste', 'Oeste'), '87', '96', '101', '110']];
      style = 'Grid Table 4 Accent 1';
      break;
    case 'subheads':
      rows = [[tr('Course', 'Curso', 'Curso'), tr('Students', 'Alunos', 'Alumnos'), tr('Change', 'Variação', 'Variación')],
        [tr('Undergraduate', 'Graduação', 'Grado'), '', ''], [tr('Engineering', 'Engenharia', 'Ingeniería'), '420', '+12'], [tr('Design', 'Design', 'Diseño'), '185', '+4'],
        [tr('Graduate', 'Pós-graduação', 'Posgrado'), '', ''], [tr('Data science', 'Ciência de dados', 'Ciencia de datos'), '96', '+21'], [tr('Management', 'Gestão', 'Gestión'), '133', '-3'],
        [tr('Total', 'Total', 'Total'), '834', '+34']];
      style = 'Plain Table 2';
      break;
    case 'double':
      rows = [[tr('Team', 'Equipe', 'Equipo'), tr('Points', 'Pontos', 'Puntos'), '', tr('Team', 'Equipe', 'Equipo'), tr('Points', 'Pontos', 'Puntos')],
        ['A', '18', '', 'E', '12'], ['B', '16', '', 'F', '11'], ['C', '14', '', 'G', '9'], ['D', '13', '', 'H', '7']];
      style = 'List Table 1 Light Accent 1';
      break;
    default: return null;
  }
  return { rows, style, merge };
}

/** The gallery's sort (`tableStyleListNow`: the plain group by rank, then `localeCompare`; the others keep their place). */
function sortStyles<T extends { name: string; group: string }>(out: T[]): T[] {
  const rank = (i: T): number => {
    if (i.group !== 'plain') return 0;
    return i.name === 'Table Grid' ? 0 : i.name === 'Grid Table Light' ? 1 : 2;
  };
  return out.sort((a, b) => (a.group === b.group && a.group === 'plain' ? rank(a) - rank(b) || a.name.localeCompare(b.name) : 0));
}

/** `tableStyleListNow()`: the gallery (the session's entries, sorted here). */
export function tableStyleListNow(c: CState): unknown[] | null {
  const v = ask(c, 'tableStyles', (b, s) => b.tableStyles(s));
  return v ? sortStyles(v) : null;
}

// ───────────────────────── the embedded spreadsheet (editEmbeddedObject 20345, oleRunAt 20367) ─────────────────────────

/**
 * `editEmbeddedObject(run)`: the spreadsheet at the body position `at` (the dblclick's hit: `oleRunAt`; a call without a
 * position takes the caret's) is edited in the `<universal-sheet>` dialog; OK writes the new package and preview
 * (`ShellEdit::ReplaceOle`: one step, the object written anew).
 */
async function editEmbedded(c: CState, at?: { para?: unknown; offset?: unknown } | null): Promise<boolean> {
  const el = elOf(c);
  const pos = at && typeof at.para === 'number' && typeof at.offset === 'number' ? { para: at.para, offset: at.offset } : el.selection?.anchor;
  if (!pos || locked(c)) return false;
  const o = ask(c, 'oleAt', (b, s) => b.oleAt(s, pos.para, pos.offset));
  if (!o || !/^Excel\.Sheet/i.test(o.progId) || o.ext !== 'xlsx') return false;
  const data = ask(c, 'oleData', (b, s) => b.oleData(s, pos.para, pos.offset));
  if (!data) return false;
  if (!(await sheetAvailable(el.sheetLoader ?? undefined))) return false;
  const layer = (el.shadowRoot?.querySelector('.uw-layer') as HTMLElement | null) ?? document.body;
  const r = await editSpreadsheet(layer, uiLang(c.ed.el) as 'en' | 'pt' | 'es', data);
  if (!r || !c.api?.session) return false;
  const out = ask(c, 'replaceOle', (b, s) => b.replaceOle(s, o.para, o.run, r.xlsx, r.png, r.widthTwips, r.heightTwips));
  return !!out && applyNoScroll(c, out, 'editEmbeddedObject');
}

/** The dblclick on an embedded spreadsheet (the TS dblclick listener 10466, before the pictures and the text). */
export function oleDblClick(c: CState, e: MouseEvent): boolean {
  const el = c.ed.el as unknown as { protectedMode?: boolean };
  if (!c.api?.session || el.protectedMode || storyCtx(c.o)?.ctx.kind === 'textbox') return false;
  const pos = c.api.hitAt(e);
  if (!pos) return false;
  const o = ask(c, 'oleAt', (b, s) => b.oleAt(s, pos.para, pos.offset));
  if (!o || !through(c, 'canEmbedSpreadsheet', [])) return false;
  e.preventDefault();
  // the TS object stays selected through the double click (its mousedowns selected it; the shell's second mousedown of a
  // fast double click reaches the text): the picture selected, then the dialog
  command(c, { t: 'drawing', v: { t: 'selectInline', para: o.para, run: o.run } } as never, 'selectInline');
  void editEmbedded(c, pos);
  return true;
}

export function tablesHandlers(ed: RustEditor): Handlers {
  const c = cstate(ed);
  return {
    // the tool is the shell's state (the TS body: cursor, `uw-tabletool`, chromeSync); its gestures are this module's
    setTableTool: ((t: Tool) => through(c, 'setTableTool', [t])) as never,
    quickTables: () => through(c, 'quickTables', []),
    insertQuickTable: (async (id: string) => {
      if (locked(c)) return false;
      const ctx = storyCtx(c.o);
      if (ctx && (ctx.ctx.kind === 'textbox' || outerCtx(ctx.ctx).kind === 'hf')) return false;
      const q = quickTable(uiLang(c.ed.el) as 'en' | 'pt' | 'es', id);
      if (!q) return false;
      return edit(c, { t: 'filledTable', rows: q.rows, style: q.style, firstColumnOff: id === 'double' || id === 'calendar', merges: q.merge, centerBody: id === 'calendar' }, 'insertQuickTable');
    }) as never,
    canEmbedSpreadsheet: () => through(c, 'canEmbedSpreadsheet', []),
    insertSpreadsheet: (async () => {
      const el = elOf(c);
      if (locked(c)) return false;
      const ctx = storyCtx(c.o);
      if (ctx && (ctx.ctx.kind === 'textbox' || outerCtx(ctx.ctx).kind === 'hf')) return false;
      if (!(await sheetAvailable(el.sheetLoader ?? undefined))) return false;
      const lang = uiLang(c.ed.el) as 'en' | 'pt' | 'es';
      const sel = { anchor: { ...el.selection.anchor }, focus: { ...el.selection.focus } };
      const layer = (el.shadowRoot?.querySelector('.uw-layer') as HTMLElement | null) ?? document.body;
      const r = await editSpreadsheet(layer, lang, blankXlsx(lang));
      if (!r || !c.api?.session) return false;
      el.setSelection?.(sel.anchor, sel.focus);
      await el.insertImage(r.png, 'png', { ole: { progId: 'Excel.Sheet.12', data: r.xlsx, ext: 'xlsx' }, widthTwips: r.widthTwips, heightTwips: r.heightTwips });
      return true;
    }) as never,
    editEmbeddedObject: ((run?: { para?: unknown; offset?: unknown } | null) => editEmbedded(c, run)) as never,
    tableStyleList: (async () => tableStyleListNow(c) ?? []) as never,
    tableStyleXml: (async (nameOrId: string) => ask(c, 'tableStyleXml', (b, s) => b.tableStyleXml(s, nameOrId))) as never,
    tableStyleBases: () => ask(c, 'tableStyleBases', (b, s) => b.tableStyleBases(s)) ?? [],
    newTableStyleId: ((name: string) => ask(c, 'newTableStyleId', (b, s) => b.newTableStyleId(s, name)) ?? name) as never,
    saveTableStyle: ((xml: string, opts: { apply?: boolean; basedOnName?: string } = {}) => {
      if (locked(c) || !/w:styleId="([^"]+)"/.test(xml ?? '')) return false;
      return command(c, { t: 'table', v: { t: 'saveTableStyle', xml, ...(opts?.apply ? { apply: true } : {}), ...(opts?.basedOnName ? { basedOnName: opts.basedOnName } : {}) } } as never, 'saveTableStyle');
    }) as never,
  };
}
