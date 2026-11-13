// Wave 7 (A2): the object modes and gestures left by front O (universal-write.ts 9183–9440, 14215–14400, 15605–15640,
// 14586–14660): Edit Wrap Points (the red dashed polygon, drag/add/delete points; the commit is the session's shell edit),
// Set Transparent Color (the next click picks the picture's pixel; the routed `setImageTransparentColor`), Select Objects
// (the lasso, Shift/Ctrl+click, the drag, Delete and the arrows of several objects: ED-211), Edit Text of a selected box, and
// the methods whose TS bodies only read the shell's state or call routed methods (the text box gallery's storage, the page
// fill presets, the gradient variations, the DrawingML colour). Record uo-bindings.md "Wave 7 — A2".
import { DEFAULT_WRAP_POLYGON } from '@universal-office/docx-core';
import type { CompletionObjFacts } from '@universal-office/write-session';
import { ask, command, cstate, edit, locked, pagePoint, PX_PER_TWIP, sync, through, type CState } from './rust-completion-core.js';
import { cmd as oCmd, objects, selectedHandle, selectedHandles, storyCtx, type Handle } from './rust-objects-core.js';
import { factsOf } from './rust-objects-pictures.js';
import type { RustEditor } from './rust-engine.js';

type Handlers = Record<string, (...args: never[]) => unknown>;
interface El {
  view: string; zoom: number; canvases: HTMLCanvasElement[]; protectedMode: boolean; style: CSSStyleDeclaration;
  focusEditor(): void; selectedImage(): { inline: boolean; wrap: string; flipH?: boolean; flipV?: boolean; format?: { polygon?: { pts?: [number, number][] }; srcRect?: { l: number; t: number; r: number; b: number } } } | null;
  setImageWrap(k: string): void; endImageCrop(): void; setImageTransparentColor(hex: string): void; selectionPaneItems?(): { key: string }[];
  dispatchEvent(e: Event): boolean;
}
const elOf = (c: CState): El => c.ed.el as unknown as El;

// ───────────────────────── Edit Wrap Points ─────────────────────────

/** `wrapPoints(img)`: the polygon (0..21600) without the repeated closing point. */
function wrapPoints(c: CState): [number, number][] {
  const pts = (elOf(c).selectedImage()?.format?.polygon?.pts ?? DEFAULT_WRAP_POLYGON).map(([x, y]): [number, number] => [x, y]);
  const a = pts[0], b = pts[pts.length - 1];
  if (pts.length > 1 && a && b && a[0] === b[0] && a[1] === b[1]) pts.pop();
  return pts;
}

/** The wrap-points mode is still on the selected picture (`editingWrapPoints`: another selection ends it). */
function wrapHandle(c: CState): Handle | null {
  if (c.wrap === null) return null;
  const h = selectedHandle(c.o);
  if (!h || h.id !== c.wrap || h.kind !== 'image') { c.wrap = null; c.wrapLive = null; return null; }
  return h;
}

/** `wrapLocal(e, h)`: the pointer in the picture's polygon units. */
function wrapLocal(c: CState, e: MouseEvent, h: Handle): { u: number; v: number; scale: number } | null {
  const el = elOf(c);
  const cv = el.canvases[h.page];
  if (!cv) return null;
  const r = cv.getBoundingClientRect(), scale = PX_PER_TWIP * el.zoom;
  const a = (-(h.rot ?? 0) * Math.PI) / 180;
  const dx = e.clientX - (r.left + (h.rect.x + h.rect.w / 2) * scale), dy = e.clientY - (r.top + (h.rect.y + h.rect.h / 2) * scale);
  const lx = (dx * Math.cos(a) - dy * Math.sin(a)) / scale + h.rect.w / 2, ly = (dx * Math.sin(a) + dy * Math.cos(a)) / scale + h.rect.h / 2;
  return { u: (lx / h.rect.w) * 21600, v: (ly / h.rect.h) * 21600, scale };
}

/** `wrapPointsMouseDown(e)` (universal-write.ts 14264): true = handled. */
export function wrapPointsMouseDown(c: CState, e: MouseEvent): boolean {
  const h = wrapHandle(c);
  if (!h || e.button !== 0) return false;
  const at = wrapLocal(c, e, h);
  if (!at) return false;
  const pts = wrapPoints(c);
  const dpr = window.devicePixelRatio || 1;
  const tu = ((6 * dpr) / at.scale / h.rect.w) * 21600, tv = ((6 * dpr) / at.scale / h.rect.h) * 21600;
  const vi = pts.findIndex(([x, y]) => Math.abs(x - at.u) <= tu && Math.abs(y - at.v) <= tv);
  const commit = (next: [number, number][]): void => { edit(c, { t: 'wrapPolygon', pts: next }, 'editWrapPoints'); };
  if (e.ctrlKey || e.metaKey) {
    e.preventDefault();
    if (vi >= 0) { if (pts.length > 3) commit(pts.filter((_, i) => i !== vi)); return true; }
    let best = -1, bd = Infinity;
    for (let i = 0; i < pts.length; i++) {
      const [ax, ay] = pts[i]!, [bx, by] = pts[(i + 1) % pts.length]!;
      const sx = (bx - ax) / tu, sy = (by - ay) / tv, px = (at.u - ax) / tu, py = (at.v - ay) / tv;
      const t = Math.max(0, Math.min(1, (px * sx + py * sy) / (sx * sx + sy * sy || 1)));
      const d = Math.hypot(px - t * sx, py - t * sy);
      if (d < bd) { bd = d; best = i; }
    }
    if (best >= 0 && bd <= 1.5) { const next = [...pts]; next.splice(best + 1, 0, [Math.round(at.u), Math.round(at.v)]); commit(next); return true; }
    return false;
  }
  if (vi < 0) return false;
  e.preventDefault();
  // wave 7c (V, A2's declaration 3 closed): the TS `snapshot()` at the mousedown (14258), the anchor's wrapPoly/wrapBox and
  // the format's polygon written on every move and the document laid out once per animation frame (14264–14274: the text
  // follows LIVE), the commit at the mouseup in the same step (14280), `discardSnapshot()` without a move
  edit(c, { t: 'liveBegin' }, 'wrapPointsBegin');
  const live = [...pts];
  const sx = e.clientX, sy = e.clientY;
  let moved = false, frame = 0;
  const move = (ev: MouseEvent): void => {
    if (!moved && Math.abs(ev.clientX - sx) + Math.abs(ev.clientY - sy) <= 2) return;
    moved = true;
    const p = wrapLocal(c, ev, h);
    if (!p) return;
    live[vi] = [Math.round(p.u), Math.round(p.v)];
    c.wrapLive = [...live];
    if (!frame) frame = requestAnimationFrame(() => { frame = 0; edit(c, { t: 'liveSet', wrap: [...live] }, 'wrapPointsLive'); });
  };
  const up = (): void => {
    window.removeEventListener('mousemove', move);
    window.removeEventListener('mouseup', up);
    cancelAnimationFrame(frame);
    frame = 0;
    c.wrapLive = null;
    if (!moved) { edit(c, { t: 'liveCancel' }, 'wrapPointsCancel'); c.api.paint(); return; }
    edit(c, { t: 'liveEnd', commit: { t: 'wrapPolygon', pts: live } }, 'editWrapPoints');
  };
  window.addEventListener('mousemove', move);
  window.addEventListener('mouseup', up);
  return true;
}

/** `paintWrapPoints(ctx, h, scale, dpr)` (universal-write.ts 14338): drawn INSTEAD of the handles (device pixels). */
export function paintWrapHandle(c: CState, h: Handle, ctx: CanvasRenderingContext2D, scale: number, dpr: number): boolean {
  const w = wrapHandle(c);
  if (!w || w.id !== h.id) return false;
  const pts = c.wrapLive ?? wrapPoints(c);
  const W = h.rect.w * scale, H = h.rect.h * scale;
  ctx.save();
  ctx.translate((h.rect.x + h.rect.w / 2) * scale, (h.rect.y + h.rect.h / 2) * scale);
  if (h.rot) ctx.rotate((h.rot * Math.PI) / 180);
  ctx.translate(-W / 2, -H / 2);
  ctx.strokeStyle = '#2b579a';
  ctx.lineWidth = Math.max(1, dpr);
  ctx.strokeRect(0, 0, W, H);
  ctx.beginPath();
  pts.forEach(([x, y], i) => { const px = (x / 21600) * W, py = (y / 21600) * H; if (i) ctx.lineTo(px, py); else ctx.moveTo(px, py); });
  ctx.closePath();
  ctx.strokeStyle = '#e00000';
  ctx.setLineDash([4 * dpr, 3 * dpr]);
  ctx.stroke();
  ctx.setLineDash([]);
  const s = 6 * dpr;
  ctx.fillStyle = '#000000';
  for (const [x, y] of pts) ctx.fillRect((x / 21600) * W - s / 2, (y / 21600) * H - s / 2, s, s);
  ctx.restore();
  return true;
}

/** `wrapPointAt(e)`: a polygon point under the pointer (the move cursor). */
export function wrapPointAt(c: CState, e: MouseEvent): boolean {
  const h = wrapHandle(c);
  if (!h) return false;
  const at = wrapLocal(c, e, h);
  if (!at) return false;
  const dpr = window.devicePixelRatio || 1;
  const tu = ((6 * dpr) / at.scale / h.rect.w) * 21600, tv = ((6 * dpr) / at.scale / h.rect.h) * 21600;
  return wrapPoints(c).some(([x, y]) => Math.abs(x - at.u) <= tu && Math.abs(y - at.v) <= tv);
}

// ───────────────────────── Set Transparent Color ─────────────────────────

/** `pictureColorAt(e)` (universal-write.ts 15618): the pixel of the selected picture under the pointer (RRGGBB). */
async function pictureColorAt(c: CState, e: MouseEvent): Promise<string | null> {
  const el = elOf(c);
  const h = selectedHandle(c.o);
  const st = el.selectedImage();
  if (!h || !st || h.image === undefined) return null;
  const cv = el.canvases[h.page];
  if (!cv) return null;
  const r = cv.getBoundingClientRect(), scale = PX_PER_TWIP * el.zoom;
  const a = (-(h.rot ?? 0) * Math.PI) / 180;
  const cx = r.left + (h.rect.x + h.rect.w / 2) * scale, cy = r.top + (h.rect.y + h.rect.h / 2) * scale;
  const dx = e.clientX - cx, dy = e.clientY - cy;
  const lx = (dx * Math.cos(a) - dy * Math.sin(a)) / scale + h.rect.w / 2, ly = (dx * Math.sin(a) + dy * Math.cos(a)) / scale + h.rect.h / 2;
  const s = st.format?.srcRect ?? { l: 0, t: 0, r: 0, b: 0 };
  const visW = 1 - (s.l + s.r) / 100000, visH = 1 - (s.t + s.b) / 100000;
  let u = s.l / 100000 + (lx / h.rect.w) * visW, v = s.t / 100000 + (ly / h.rect.h) * visH;
  if (st.flipH) u = 1 - u;
  if (st.flipV) v = 1 - v;
  if (u < 0 || v < 0 || u > 1 || v > 1) return null;
  const bytes = c.api.dl?.imageBytes(h.image);
  if (!bytes) return null;
  // `decodePicture(img)`: the whole picture's pixels
  const bmp = await createImageBitmap(new Blob([new Uint8Array(bytes)]));
  const oc = new OffscreenCanvas(bmp.width, bmp.height);
  const x = oc.getContext('2d')!;
  x.drawImage(bmp, 0, 0);
  const id = x.getImageData(0, 0, bmp.width, bmp.height);
  bmp.close();
  const k = (Math.min(id.height - 1, Math.floor(v * id.height)) * id.width + Math.min(id.width - 1, Math.floor(u * id.width))) * 4;
  return [id.data[k]!, id.data[k + 1]!, id.data[k + 2]!].map((n) => n.toString(16).padStart(2, '0')).join('').toUpperCase();
}

/** The click of the pick mode (universal-write.ts 10171): true = handled. */
export function pickMouseDown(c: CState, e: MouseEvent): boolean {
  if (!c.picking || e.button !== 0) return false;
  e.preventDefault();
  c.picking = false;
  elOf(c).style.cursor = '';
  void pictureColorAt(c, e).then((hex) => { if (hex) elOf(c).setImageTransparentColor(hex); });
  return true;
}

// ───────────────────────── Select Objects and the multi-selection ─────────────────────────

const selectMode = (c: CState): boolean => !!through(c, 'isSelectObjectsMode', []);

/** The TS `selectedFloats()` facts in selection order (the session's `ObjFacts`). */
function factsAll(c: CState): CompletionObjFacts[] {
  return selectedHandles(c.o).map((h) => (h.inline ? {} : (factsOf(h) as CompletionObjFacts)));
}

/** `moveSelectedFloatsBy(dx, dy)`: every selected floating object, one step. */
export function moveSelectedBy(c: CState, dx: number, dy: number): void {
  if (locked(c) || (!dx && !dy)) return;
  edit(c, { t: 'moveSelected', dx, dy, facts: factsAll(c) }, 'moveSelectedFloatsBy');
}

/** `deleteSelectedFloats()`: every selected object, one step. */
export function deleteSelected(c: CState): void {
  if (locked(c)) return;
  edit(c, { t: 'deleteSelected' }, 'deleteSelectedFloats');
}

/** The pane key of a float (`selectPaneItem(key, add)` toggles it in the selection: `toggleFloatInSelection`). */
function keyOf(h: Handle): string | undefined { return h.key; }

/** The float under the pointer (`hitFloat(e)`, the floats of the page). */
function floatAt(c: CState, e: MouseEvent): Handle | null {
  const p = pagePoint(c, e);
  const v = objects(c.o);
  if (!p || !v || v.scope === 'tb') return null;
  for (let i = v.floats.length - 1; i >= 0; i--) {
    const f = v.floats[i]!;
    if (f.page === p.pg && p.x >= f.x && p.x <= f.x + f.w && p.y >= f.y && p.y <= f.y + f.h) {
      return { kind: f.kind, id: f.id, page: f.page, rect: { x: f.x, y: f.y, w: f.w, h: f.h }, rot: f.rot, inline: false, index: f.i, frame: f.frame, editable: f.editable, ...(f.key ? { key: f.key } : {}) } as Handle;
    }
  }
  return null;
}

/** `selectObjectsMouseDown(e)` (universal-write.ts 9361): true = handled. */
export function selectObjectsMouseDown(c: CState, e: MouseEvent): boolean {
  const el = elOf(c);
  if (e.button !== 0 || !c.api?.dl || el.view !== 'print' || el.protectedMode) return false;
  const fh = floatAt(c, e);
  const sel = selectedHandles(c.o);
  const on = selectMode(c);
  if (fh && (e.shiftKey || e.ctrlKey) && (sel.length || on)) {
    e.preventDefault();
    const k = keyOf(fh);
    if (k) command(c, { t: 'drawing', v: { t: 'selectPaneItem', key: k, add: true } } as never, 'toggleFloatInSelection');
    el.focusEditor();
    sync(c);
    return true;
  }
  if (fh && sel.length > 1 && sel.some((h) => h.id === fh.id)) {
    e.preventDefault();
    el.focusEditor();
    const scale = PX_PER_TWIP * el.zoom;
    const x0 = e.clientX, y0 = e.clientY;
    c.multiDrag = { dx: 0, dy: 0 };
    const move = (ev: MouseEvent): void => { c.multiDrag = { dx: Math.round((ev.clientX - x0) / scale), dy: Math.round((ev.clientY - y0) / scale) }; c.api.paint(); };
    const up = (): void => {
      window.removeEventListener('mousemove', move);
      window.removeEventListener('mouseup', up);
      const d = c.multiDrag;
      c.multiDrag = null;
      if (d && (Math.abs(d.dx) > 30 || Math.abs(d.dy) > 30)) moveSelectedBy(c, d.dx, d.dy);
      else c.api.paint();
    };
    window.addEventListener('mousemove', move);
    window.addEventListener('mouseup', up);
    return true;
  }
  if (!on) return false;
  e.preventDefault();
  el.focusEditor();
  if (fh) {
    // in the mode the click only selects (no entry in a box's text) and a drag moves: front O's handle drag after the select
    if (fh.index !== undefined) oCmd(c.o, { t: 'drawing', v: { t: 'selectObject', index: fh.index } }, 'selectObject');
    sync(c);
    return false;
  }
  const p0 = pagePoint(c, e);
  if (!p0) return true;
  const add = e.shiftKey || e.ctrlKey;
  if (!add && sel.length) oCmd(c.o, { t: 'drawing', v: { t: 'selectObject' } }, 'selectObject');
  c.lasso = { page: p0.pg, x: p0.x, y: p0.y, w: 0, h: 0 };
  const move = (ev: MouseEvent): void => {
    const p = pagePoint(c, ev);
    if (!p || p.pg !== p0.pg || !c.lasso) return;
    c.lasso = { page: p0.pg, x: Math.min(p0.x, p.x), y: Math.min(p0.y, p.y), w: Math.abs(p.x - p0.x), h: Math.abs(p.y - p0.y) };
    c.api.paint();
  };
  const up = (): void => {
    window.removeEventListener('mousemove', move);
    window.removeEventListener('mouseup', up);
    const l = c.lasso;
    c.lasso = null;
    if (l && l.w > 20 && l.h > 20) {
      // `selectFloatsIn(page, l, add)`: the whole objects inside, after the selection kept with `add`
      const v = objects(c.o);
      const inside = (v?.floats ?? []).filter((f) => f.page === l.page && f.x >= l.x && f.y >= l.y && f.x + f.w <= l.x + l.w && f.y + f.h <= l.y + l.h);
      const keep = add ? selectedHandles(c.o).map((h) => h.key).filter((k): k is string => !!k) : [];
      const keys = [...new Set([...keep, ...inside.map((f) => f.key).filter((k): k is string => !!k)])];
      if (keys.length) command(c, { t: 'drawing', v: { t: 'selectPaneItems', keys } } as never, 'selectFloatsIn');
    }
    c.api.paint();
    sync(c);
  };
  window.addEventListener('mousemove', move);
  window.addEventListener('mouseup', up);
  c.api.paint();
  return true;
}

/** `paintMultiSelection(ctx, pg)` (universal-write.ts 9312), page twips. */
function paintMulti(c: CState, ctx: CanvasRenderingContext2D, pg: number): void {
  const lw = 15;
  const all = selectedHandles(c.o);
  if (all.length > 1 || c.multiDrag) {
    const d = c.multiDrag ?? { dx: 0, dy: 0 };
    for (const h of all) {
      if (h.page !== pg) continue;
      ctx.save();
      ctx.strokeStyle = '#3a72c4';
      ctx.lineWidth = lw;
      ctx.setLineDash([60, 40]);
      ctx.strokeRect(h.rect.x + d.dx, h.rect.y + d.dy, h.rect.w, h.rect.h);
      ctx.setLineDash([]);
      ctx.fillStyle = '#ffffff';
      for (const [x, y] of [[0, 0], [1, 0], [0, 1], [1, 1]] as const) {
        const cx = h.rect.x + d.dx + x * h.rect.w, cy = h.rect.y + d.dy + y * h.rect.h;
        ctx.fillRect(cx - 50, cy - 50, 100, 100);
        ctx.strokeRect(cx - 50, cy - 50, 100, 100);
      }
      ctx.restore();
    }
  }
  if (c.lasso && c.lasso.page === pg) {
    const r = c.lasso;
    ctx.save();
    ctx.fillStyle = 'rgba(58,114,196,0.12)';
    ctx.fillRect(r.x, r.y, r.w, r.h);
    ctx.strokeStyle = '#3a72c4';
    ctx.lineWidth = lw;
    ctx.setLineDash([45, 30]);
    ctx.strokeRect(r.x, r.y, r.w, r.h);
    ctx.restore();
  }
}

/** The overlays of this module on page `pg` (page twips). */
export function paintObjects(c: CState, ctx: CanvasRenderingContext2D, pg: number): void {
  paintMulti(c, ctx, pg);
}

/** The keys: Esc ends Select Objects without a selection (10681); Delete/arrows with several objects (10699–10703). */
export function objectsKey(c: CState, e: KeyboardEvent): boolean {
  const k = e.key;
  const ctrl = e.ctrlKey || e.metaKey;
  if (k === 'Escape' && c.picking) { c.picking = false; elOf(c).style.cursor = ''; }
  if ((k === 'Escape' || k === 'Enter') && wrapHandle(c)) { e.preventDefault(); c.wrap = null; c.wrapLive = null; c.api.paint(); (elOf(c) as unknown as { chromeSync?: (() => void) | null }).chromeSync?.(); return true; }
  const sel = selectedHandles(c.o);
  if (k === 'Escape' && selectMode(c) && !sel.length) { e.preventDefault(); through(c, 'setSelectObjectsMode', [false]); return true; }
  if (k === 'Escape' && sel.length && selectMode(c)) {
    // the TS floatSel branch (10698): the selection goes and the mode ends (front O's Esc deselects next)
    through(c, 'setSelectObjectsMode', [false]);
    return false;
  }
  if (sel.length < 2) return false;
  if ((k === 'Delete' || k === 'Backspace') && !ctrl) { e.preventDefault(); deleteSelected(c); return true; }
  if (k === 'ArrowUp' || k === 'ArrowDown' || (!ctrl && (k === 'ArrowLeft' || k === 'ArrowRight'))) {
    e.preventDefault();
    const step = e.ctrlKey ? 15 : 90;
    moveSelectedBy(c, k === 'ArrowLeft' ? -step : k === 'ArrowRight' ? step : 0, k === 'ArrowUp' ? -step : k === 'ArrowDown' ? step : 0);
    return true;
  }
  return false;
}

export function objectsHandlersA2(ed: RustEditor): Handlers {
  const c = cstate(ed);
  return {
    startEditWrapPoints: () => {
      const el = elOf(c);
      const st = el.selectedImage();
      const h = selectedHandle(c.o);
      if (!st || !h || locked(c) || st.inline) return;
      if (st.wrap !== 'tight' && st.wrap !== 'through') el.setImageWrap('tight');
      el.endImageCrop();
      c.wrap = h.id;
      c.api.paint();
      (el as unknown as { chromeSync?: (() => void) | null }).chromeSync?.();
    },
    endEditWrapPoints: () => {
      if (c.wrap === null) return;
      c.wrap = null;
      c.wrapLive = null;
      c.api.paint();
      (elOf(c) as unknown as { chromeSync?: (() => void) | null }).chromeSync?.();
    },
    editingWrapPoints: () => !!wrapHandle(c),
    startTransparentColorPick: () => {
      if (!elOf(c).selectedImage()) return;
      c.picking = true;
      elOf(c).style.cursor = 'crosshair';
    },
    editSelectedTextbox: () => {
      const h = selectedHandle(c.o);
      if (!h || h.kind !== 'textbox' || locked(c)) return false;
      command(c, { t: 'drawing', v: { t: 'editSelectedTextbox', page: h.page } } as never, 'editSelectedTextbox');
      sync(c);
      return storyCtx(c.o)?.ctx.kind === 'textbox';
    },
    // the TS bodies read the host's storage, the language of the interface, or call routed methods
    // the shell's state (the TS bodies: the mode flag, the cursor, `uw-select-objects`, the bars; its lasso is this module's)
    setSelectObjectsMode: ((on: boolean) => { c.lasso = null; through(c, 'setSelectObjectsMode', [on]); }) as never,
    isSelectObjectsMode: () => selectMode(c),
    textboxTemplates: () => through(c, 'textboxTemplates', []),
    // `saveSelectionToTextboxGallery(name)` 14636: the session writes the box's drawing with its content (`serializeBlocks`);
    // the item and the host's storage are the TS code's (`userTextboxes`/`storeUserTextboxes`)
    saveSelectionToTextboxGallery: ((name?: string) => {
      const it = ask(c, 'textboxGalleryItem', (b, s) => b.textboxGalleryItem(s));
      if (!it) return false;
      const items = ((through(c, 'userTextboxes', []) as { id: string; name: string; xml: string }[] | undefined) ?? []).slice();
      items.push({ id: `user:${Date.now().toString(36)}`, name: (name ?? '').trim() || it.name || 'Text Box', xml: it.xml });
      through(c, 'storeUserTextboxes', [items]);
      return true;
    }) as never,
    removeTextboxGalleryItem: ((id: string) => through(c, 'removeTextboxGalleryItem', [id])) as never,
    pageFillOptions: () => through(c, 'pageFillOptions', []),
    shapeGradientVariations: () => through(c, 'shapeGradientVariations', []),
    resolveDmlColor: ((col: unknown) => through(c, 'resolveDmlColor', [col])) as never,
  };
}
