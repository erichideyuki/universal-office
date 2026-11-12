// Frente O da leva 5 do engine="rust" (registro core/docs/modules/uo-bindings.md "Wave 5 — O: objects and the page"): o
// PONTEIRO e o TECLADO dos objetos — selecionar por clique (flutuante ou em linha), as alças (redimensionar, girar), mover
// arrastando, setas, Esc, Delete, o modo Cortar, Desenhar Caixa de Texto, Criar Vínculo — e a pintura das alças. Port de
// universal-write.ts (`hitFloat`, `onObjectBorder`, `handleCenters`, `handleAt`, `cursorForHandle`, `startFloatDrag`,
// `commit*Drag`, `posFor`/`refRect`, `applyResize`, o ramo de objetos do `mousedown` e do `keydown`, `paintFloatAndCaret`,
// `cropGeom`/`cropHit`/`paintCropOverlay`, `drawMouseDown`, `linkMouseDown`). O fim de um arrasto vai à sessão como UM
// comando (`setLayout` com a posição nas mesmas referências e o tamanho, ou `setRotation`; ED-202); durante o arrasto a
// casca pinta um contorno fantasma em vez do relayout ao vivo do TS (ED-203).
import { uiLang } from '@universal-office/ui';
import type { ShellObjectFrame } from '@universal-office/write-session';
import { jobsBusy } from './rust-objects-jobs.js';
import { bitmapOf } from './rust-objects-pictures.js';
import {
  cellBoxesOn, cmd, floatHandle, inlineHandle, objects, objectsInBox, outerCtx, PX_PER_TWIP, selectedHandle, selectedHandles, storyCtx, textboxHit,
  type Handle, type ObjState,
} from './rust-objects-core.js';

type Pos = { para: number; offset: number };
type Rect = { x: number; y: number; w: number; h: number };
interface El {
  canvases: HTMLCanvasElement[];
  zoom: number;
  view: string;
  protectedMode: boolean;
  dirty: boolean;
  style: CSSStyleDeclaration;
  chromeSync: (() => void) | null;
  onSelectionChange?: (() => void) | null;
  selection: { anchor: Pos; focus: Pos };
  focusEditor(): void;
  setSelection(a: Pos, f?: Pos): void;
  insertPageBreak(): void;
  insertColumnBreak(): void;
  nudgeRead?(dir: 1 | -1): void;
  dispatchEvent(e: Event): boolean;
}

const FLOAT_HANDLES = ['nw', 'n', 'ne', 'e', 'se', 's', 'sw', 'w'] as const;
const ROT_HANDLE_GAP = 22;
const elOf = (o: ObjState): El => o.ed.el as unknown as El;
const scaleOf = (o: ObjState): number => PX_PER_TWIP * elOf(o).zoom;
const dprOf = (): number => window.devicePixelRatio || 1;

// ───────────────────────── geometria (port 1:1) ─────────────────────────

/** Ponto da página (twips) no referencial do objeto: relativo ao centro, sem o giro. */
export function localPoint(h: { rect: Rect; rot?: number }, xT: number, yT: number): { x: number; y: number } {
  const a = (-(h.rot ?? 0) * Math.PI) / 180;
  const dx = xT - (h.rect.x + h.rect.w / 2), dy = yT - (h.rect.y + h.rect.h / 2);
  return { x: dx * Math.cos(a) - dy * Math.sin(a), y: dx * Math.sin(a) + dy * Math.cos(a) };
}

/** Origem/extensão de uma referência de posicionamento (`refRect` do TS). */
export function refRect(from: string, axis: 'h' | 'v', f: ShellObjectFrame): { o: number; span: number } {
  if (axis === 'h') {
    switch (from) {
      case 'page': return { o: 0, span: f.pageW };
      case 'margin': return { o: f.marL, span: f.pageW - f.marL - f.marR };
      case 'leftMargin': case 'insideMargin': return { o: 0, span: f.marL };
      case 'rightMargin': case 'outsideMargin': return { o: f.pageW - f.marR, span: f.marR };
      default: return { o: f.colX, span: f.colW };
    }
  }
  switch (from) {
    case 'page': return { o: 0, span: f.pageH };
    case 'margin': return { o: f.bodyTop, span: f.pageH - f.bodyTop - f.marB };
    case 'topMargin': case 'insideMargin': return { o: 0, span: f.marT };
    case 'bottomMargin': case 'outsideMargin': return { o: f.pageH - f.marB, span: f.marB };
    default: return { o: f.paraTop, span: 0 };
  }
}

/**
 * `posFor(cur, axis, xy, frame)` do TS como o `LayoutPos` da sessão (`setLayout`): o deslocamento em pontos (a sessão
 * grava `round(offsetPt × 12700)` = o `round((xy − o) × 635)` do TS) ou, para uma posição em %, o % (a sessão grava
 * `round(pct × 1000)` e a reserva `round((o + pct/100 × span) × 635)`). ED-202: a mesma conta em outra ordem de ponto
 * flutuante — idêntica em twips inteiros; com as frações do layout, ±1 EMU (ou 1 milésimo de %) num .5 de fronteira
 * (engine-objects.test.ts mede contra o `posFor` real do elemento TS).
 */
export function layoutPosFor(cur: { relativeFrom: string; align?: string; pct?: number } | undefined, axis: 'h' | 'v', xy: number, frame: ShellObjectFrame): Record<string, unknown> | undefined {
  if (!cur) return undefined;
  const ref = refRect(cur.relativeFrom, axis, frame);
  if (cur.pct !== undefined && cur.align === undefined) {
    const span = axis === 'h' ? frame.pageW : frame.pageH;
    return { relativeFrom: cur.relativeFrom, pct: ((xy - ref.o) / span) * 100 };
  }
  return { relativeFrom: cur.relativeFrom, offsetPt: (xy - ref.o) / 20 };
}

/** `applyResize` do TS: o retângulo novo (largura, altura, deslocamento do canto) de um arrasto de alça. */
export function resizeRect(
  d: { handle: string; startW: number; startH: number }, dxT: number, dyT: number, lockAspect: boolean, fromCenter: boolean, rotDeg: number,
): { w: number; h: number; dx: number; dy: number } {
  const MIN = 120;
  const hn = d.handle;
  let L = -d.startW / 2, R = d.startW / 2, T = -d.startH / 2, B = d.startH / 2;
  if (hn.includes('e')) { R += dxT; if (fromCenter) L -= dxT; }
  if (hn.includes('w')) { L += dxT; if (fromCenter) R -= dxT; }
  if (hn.includes('s')) { B += dyT; if (fromCenter) T -= dyT; }
  if (hn.includes('n')) { T += dyT; if (fromCenter) B -= dyT; }
  if (lockAspect && hn.length === 2 && d.startW > 0 && d.startH > 0) {
    const rw = (R - L) / d.startW, rh = (B - T) / d.startH;
    const r = Math.abs(rw - 1) >= Math.abs(rh - 1) ? rw : rh;
    const nw = d.startW * r, nh = d.startH * r;
    if (fromCenter) { L = -nw / 2; R = nw / 2; T = -nh / 2; B = nh / 2; } else {
      if (hn.includes('w')) L = R - nw; else R = L + nw;
      if (hn.includes('n')) T = B - nh; else B = T + nh;
    }
  }
  const w = Math.max(MIN, R - L), h = Math.max(MIN, B - T);
  const a = (rotDeg * Math.PI) / 180;
  const cxl = (L + R) / 2, cyl = (T + B) / 2;
  const cx = cxl * Math.cos(a) - cyl * Math.sin(a), cy = cxl * Math.sin(a) + cyl * Math.cos(a);
  return { w: Math.round(w), h: Math.round(h), dx: Math.round(d.startW / 2 + cx - w / 2), dy: Math.round(d.startH / 2 + cy - h / 2) };
}

function cursorForHandle(hn: string, rot = 0): string {
  if (hn === 'rot') return 'grab';
  const base: Record<string, number> = { e: 0, se: 45, s: 90, sw: 135, w: 180, nw: 225, n: 270, ne: 315 };
  const ang = (((base[hn] ?? 0) + rot) % 180 + 180) % 180;
  const k = Math.round(ang / 45) % 4;
  return ['ew-resize', 'nwse-resize', 'ns-resize', 'nesw-resize'][k]!;
}

// ───────────────────────── acertos ─────────────────────────

function pagePoint(o: ObjState, page: number, e: MouseEvent): { x: number; y: number; rect: DOMRect } | null {
  const cv = elOf(o).canvases[page];
  if (!cv) return null;
  const rect = cv.getBoundingClientRect();
  const s = scaleOf(o);
  return { x: (e.clientX - rect.left) / s, y: (e.clientY - rect.top) / s, rect };
}

/** `hitFloat(e, kind?)`: o objeto flutuante sob o ponteiro (o último pintado; fundos de página inteira são vazados). */
function hitFloat(o: ObjState, e: MouseEvent, kind?: Handle['kind']): Handle | null {
  const v = objects(o);
  if (!v || v.scope === 'tb') return null;
  const s = scaleOf(o);
  let best: Handle | null = null;
  for (const f of v.floats) {
    if (kind && f.kind !== kind) continue;
    const p = pagePoint(o, f.page, e);
    if (!p) continue;
    const pageW = p.rect.width / s, pageH = p.rect.height / s;
    if (f.w * f.h >= 0.55 * pageW * pageH) continue;
    const h = floatHandle(f);
    const l = localPoint(h, p.x, p.y);
    if (Math.abs(l.x) <= f.w / 2 && Math.abs(l.y) <= f.h / 2) best = h;
  }
  return best;
}

type BoxHit = { page: number; rect: Rect; rot: number };

/**
 * `hitTextbox(e)` (universal-write.ts 13338, the engine branch): the pages whose canvas holds the point, the session's
 * port of the per-object test (`textboxHit`: laid-out content, the context's story, ±5 px outside the border in the
 * box's turned frame, the top one by front layer then relativeHeight). Front S enters the box with the same export.
 */
function hitTextbox(o: ObjState, e: MouseEvent): BoxHit | null {
  const v = objects(o);
  if (!v || v.scope === 'tb') return null;
  const el = elOf(o);
  const s = scaleOf(o);
  for (let pg = 0; pg < el.canvases.length; pg++) {
    const cr = el.canvases[pg]!.getBoundingClientRect();
    if (e.clientY < cr.top || e.clientY > cr.bottom || e.clientX < cr.left || e.clientX > cr.right) continue;
    const hit = textboxHit(o, pg, (e.clientX - cr.left) / s, (e.clientY - cr.top) / s, 5 / s);
    if (hit) return { page: hit.page, rect: hit.rect, rot: hit.rot };
  }
  return null;
}

/** The float of a hit box (same page and placed rectangle: both come from the layout's `page.objects`). */
function floatOfBox(o: ObjState, b: BoxHit): Handle | null {
  const f = objects(o)?.floats.find((x) => x.kind === 'textbox' && x.page === b.page && x.x === b.rect.x && x.y === b.rect.y && x.w === b.rect.w && x.h === b.rect.h);
  return f ? floatHandle(f) : null;
}

/**
 * `tableEdgeAt(e)` under the conditions of `tableMouseDown` (10141): the TS takes a mousedown 4 px around a cell's left,
 * right or bottom edge for the table edge drag BEFORE the objects (an object handle lying on a table edge does not
 * resize the object). Frontier S does not route the edge drag (its declared remainder); this guard keeps front O's object
 * branch off such a point, so the object stays unchanged as in the TS. The table move/size handles (`tableHandleAt`,
 * hover state) are not guarded (ED-218).
 */
function onTableEdge(o: ObjState, e: MouseEvent): boolean {
  const el = elOf(o);
  if (e.button !== 0 || o.api.locked() || el.view !== 'print') return false;
  const c = storyCtx(o);
  if (c && (c.ctx.kind === 'textbox' || outerCtx(c.ctx).kind === 'hf')) return false;
  const s = scaleOf(o);
  const tol = 4 / s;
  for (let pg = 0; pg < el.canvases.length; pg++) {
    const cr = el.canvases[pg]!.getBoundingClientRect();
    if (e.clientY < cr.top || e.clientY > cr.bottom || e.clientX < cr.left || e.clientX > cr.right) continue;
    const x = (e.clientX - cr.left) / s, y = (e.clientY - cr.top) / s;
    for (const b of cellBoxesOn(o, pg)) {
      if (y >= b.y && y <= b.y + b.h && (Math.abs(x - b.x) <= tol || Math.abs(x - (b.x + b.w)) <= tol)) return true;
      if (x >= b.x && x <= b.x + b.w && Math.abs(y - (b.y + b.h)) <= tol) return true;
    }
    return false;
  }
  return false;
}

/** `hitInlinePic(e)`. */
function hitInline(o: ObjState, e: MouseEvent): Handle | null {
  const v = objects(o);
  if (!v || v.scope === 'tb') return null;
  for (const p of v.inline) {
    const q = pagePoint(o, p.page, e);
    if (q && q.x >= p.x && q.x <= p.x + p.w && q.y >= p.y && q.y <= p.y + p.h) return inlineHandle(p);
  }
  return null;
}

/** `onObjectBorder(e, h)`: o ponteiro na faixa de alguns px dos dois lados do contorno. */
function onBorder(o: ObjState, e: MouseEvent, h: { page: number; rect: Rect; rot?: number }): boolean {
  const p = pagePoint(o, h.page, e);
  if (!p) return false;
  const s = scaleOf(o);
  const l = localPoint(h, p.x, p.y);
  const tol = 5 / s;
  const ax = Math.abs(l.x), ay = Math.abs(l.y), hw = h.rect.w / 2, hh = h.rect.h / 2;
  if (ax > hw + tol || ay > hh + tol) return false;
  return ax >= hw - tol || ay >= hh - tol;
}

/** `handleCenters(h)` (client px): as 8 alças giradas com o objeto e a de girar. */
function handleCenters(o: ObjState, h: Handle): Record<string, { cx: number; cy: number }> | null {
  const cv = elOf(o).canvases[h.page];
  if (!cv) return null;
  const rect = cv.getBoundingClientRect();
  const s = scaleOf(o);
  const w = h.rect.w * s, hh = h.rect.h * s;
  const cx0 = rect.left + h.rect.x * s + w / 2, cy0 = rect.top + h.rect.y * s + hh / 2;
  const a = (h.rot * Math.PI) / 180, cos = Math.cos(a), sin = Math.sin(a);
  const at = (dx: number, dy: number): { cx: number; cy: number } => ({ cx: cx0 + dx * cos - dy * sin, cy: cy0 + dx * sin + dy * cos });
  const out: Record<string, { cx: number; cy: number }> = {
    nw: at(-w / 2, -hh / 2), n: at(0, -hh / 2), ne: at(w / 2, -hh / 2), e: at(w / 2, 0),
    se: at(w / 2, hh / 2), s: at(0, hh / 2), sw: at(-w / 2, hh / 2), w: at(-w / 2, 0),
  };
  if (h.kind === 'image' ? !h.editing : !h.inline && h.editable) out['rot'] = at(0, -hh / 2 - ROT_HANDLE_GAP);
  return out;
}

function handleAt(o: ObjState, e: MouseEvent): string | null {
  const h = activeHandle(o);
  if (!h) return null;
  const centers = handleCenters(o, h);
  if (!centers) return null;
  const R = 7 * dprOf();
  for (const name of [...FLOAT_HANDLES, 'rot']) {
    const c = centers[name];
    if (c && Math.abs(e.clientX - c.cx) <= R && Math.abs(e.clientY - c.cy) <= R) return name;
  }
  return null;
}

/** O objeto com alças (`activeFloat()`): a caixa de texto em edição (contorno tracejado; o texto dela é da frente S) ou o selecionado. */
function activeHandle(o: ObjState): Handle | null {
  const v = objects(o);
  if (!v) return null;
  if (v.scope === 'tb') { const f = v.floats.find((x) => x.id === v.box); return f ? { ...floatHandle(f), editing: true } : null; }
  return selectedHandle(o);
}

// ───────────────────────── seleção ─────────────────────────

function selectHandle(o: ObjState, h: Handle): void {
  const v = objects(o);
  if (v?.sel[0] === h.id && v.sel.length === 1) return;
  if (h.inline && h.at) cmd(o, { t: 'drawing', v: { t: 'selectInline', para: h.at.para, run: h.at.run } }, 'selectInline');
  else cmd(o, { t: 'drawing', v: { t: 'selectObject', index: h.index } }, 'selectObject');
}

function deselect(o: ObjState): void {
  const v = objects(o);
  if (v && v.sel.length) cmd(o, { t: 'drawing', v: { t: 'selectObject' } }, 'selectObject');
}

/**
 * O clique sem arrasto num objeto: o TS toma e descarta um passo de desfazer (`snapshot()` + `discardSnapshot()`), o que
 * dispara `uw-dirty-change` (true), `uw-change` e `uw-dirty-change` (false) num documento limpo — reproduzido aqui sem
 * mexer no estado (ED-202b).
 */
function snapshotDiscardEvents(o: ObjState): void {
  const el = elOf(o);
  if (!el.dirty) el.dispatchEvent(new CustomEvent('uw-dirty-change', { detail: { dirty: true } }));
  el.dispatchEvent(new CustomEvent('uw-change'));
  if (!el.dirty) el.dispatchEvent(new CustomEvent('uw-dirty-change', { detail: { dirty: false } }));
}

// ───────────────────────── arrastos ─────────────────────────

interface StateFmt { cx: number; cy: number; noChangeAspect?: boolean; posH?: { relativeFrom: string; align?: string; pct?: number }; posV?: { relativeFrom: string; align?: string; pct?: number }; sizeRelH?: { pct?: number }; sizeRelV?: { pct?: number }; inline?: boolean; srcRect?: { l: number; t: number; r: number; b: number }; flipH?: boolean; flipV?: boolean }

function formatNow(o: ObjState, h: Handle): StateFmt | null {
  const a = o.api.session?.query({ t: 'selectedObjectState', ...(h.kind === 'image' ? { wantImage: true } : {}) } as { t: string }) as { value?: { format: StateFmt } } | undefined;
  return a?.value?.format ?? null;
}

/** `startFloatDrag(e, mode, handle)`: o fantasma durante o arrasto; o fim é UM comando da sessão. */
function startFloatDrag(o: ObjState, e: MouseEvent, mode: 'move' | 'resize' | 'rotate', handle: string): void {
  const h = activeHandle(o);
  if (!h) return;
  e.preventDefault();
  // em linha: arrastar move NO TEXTO no TS (startInlineDrag); não roteado nesta onda (ED-204)
  if (h.inline && mode === 'move') return;
  const f = formatNow(o, h);
  if (!f) return;
  const s = scaleOf(o);
  const startW = f.cx / 635, startH = f.cy / 635;
  const startRot = h.rot;
  const sx = e.clientX, sy = e.clientY;
  const cr = elOf(o).canvases[h.page]?.getBoundingClientRect();
  const center = cr ? { x: cr.left + (h.rect.x + h.rect.w / 2) * s, y: cr.top + (h.rect.y + h.rect.h / 2) * s } : { x: sx, y: sy };
  const a0 = Math.atan2(sy - center.y, sx - center.x);
  let moved = false;
  let rot = startRot;
  let res = { w: startW, h: startH, dx: 0, dy: 0 };
  const lock0 = h.kind === 'image' ? (f.noChangeAspect ?? true) : !!f.noChangeAspect;
  const move = (ev: MouseEvent): void => {
    if (Math.abs(ev.clientX - sx) + Math.abs(ev.clientY - sy) > 2) moved = true;
    if (!moved) return;
    if (mode === 'rotate') {
      let r = startRot + ((Math.atan2(ev.clientY - center.y, ev.clientX - center.x) - a0) * 180) / Math.PI;
      if (ev.shiftKey) r = Math.round(r / 15) * 15;
      rot = ((r % 360) + 360) % 360;
    } else if (mode === 'move') {
      res = { w: startW, h: startH, dx: Math.round((ev.clientX - sx) / s), dy: Math.round((ev.clientY - sy) / s) };
    } else {
      const rx = (ev.clientX - sx) / s, ry = (ev.clientY - sy) / s;
      const ar = (-startRot * Math.PI) / 180;
      const dxT = rx * Math.cos(ar) - ry * Math.sin(ar), dyT = rx * Math.sin(ar) + ry * Math.cos(ar);
      res = resizeRect({ handle, startW, startH }, dxT, dyT, lock0 !== ev.shiftKey, ev.ctrlKey, startRot);
    }
    o.drag = { page: h.page, rect: { x: h.rect.x + res.dx, y: h.rect.y + res.dy, w: res.w, h: res.h }, rot };
    o.api.paint();
  };
  const up = (): void => {
    window.removeEventListener('mousemove', move);
    window.removeEventListener('mouseup', up);
    o.drag = null;
    if (!moved) { snapshotDiscardEvents(o); o.api.paint(); return; }
    commitDrag(o, h, f, mode, rot, res);
  };
  window.addEventListener('mousemove', move);
  window.addEventListener('mouseup', up);
}

/** `commitPictureDrag`/`commitDrawingDrag` do TS como `setRotation` ou `setLayout` (posição nas mesmas referências, tamanho). */
function commitDrag(o: ObjState, h: Handle, f: StateFmt, mode: string, rot: number, res: { w: number; h: number; dx: number; dy: number }): void {
  const wrap = (v: Record<string, unknown>): { t: string; v: unknown } => (h.kind === 'image' ? { t: 'picture', v } : { t: 'drawing', v: { t: 'onShape', v } });
  if (mode === 'rotate') { cmd(o, wrap({ $t: 'setRotation', deg: rot }), 'dragRotate'); return; }
  const lo: Record<string, unknown> = {};
  if (mode === 'resize') {
    lo['widthPt'] = res.w / 20;
    lo['heightPt'] = res.h / 20;
    if (f.sizeRelH?.pct) lo['relWidth'] = {};
    if (f.sizeRelV?.pct) lo['relHeight'] = {};
  }
  if (h.frame && !h.inline && !f.inline) {
    const nh = layoutPosFor(f.posH, 'h', h.rect.x + res.dx, h.frame), nv = layoutPosFor(f.posV, 'v', h.rect.y + res.dy, h.frame);
    if (nh) lo['posH'] = nh;
    if (nv) lo['posV'] = nv;
  }
  cmd(o, wrap({ $t: 'setLayout', o: lo, facts: { rect: { ...h.rect }, ...(h.frame ? { frame: h.frame } : {}) } }), mode === 'move' ? 'dragMove' : 'dragResize');
}

/** As setas com um objeto flutuante selecionado (90 tw; Ctrl: 15 tw), nas mesmas referências. */
function nudge(o: ObjState, dx: number, dy: number): void {
  for (const h of selectedHandles(o)) {
    if (h.inline || !h.frame) continue;
    const f = h.kind === 'image' ? formatNow(o, h) : formatNow(o, h);
    if (!f) continue;
    const lo: Record<string, unknown> = {};
    const nh = layoutPosFor(f.posH, 'h', h.rect.x + dx, h.frame), nv = layoutPosFor(f.posV, 'v', h.rect.y + dy, h.frame);
    if (dx && nh) lo['posH'] = nh;
    if (dy && nv) lo['posV'] = nv;
    const v = { $t: 'setLayout', o: lo, facts: { rect: { ...h.rect }, frame: h.frame } };
    cmd(o, h.kind === 'image' ? { t: 'picture', v } : { t: 'drawing', v: { t: 'onShape', v } as unknown }, 'nudge');
    break; // a seleção múltipla anda junta no TS (moveSelectedFloatsBy): só o selecionado aqui (ED-211)
  }
}

// ───────────────────────── modo Cortar ─────────────────────────

function cropGeom(h: Handle, f: StateFmt): { L: number; R: number; T: number; B: number; fL: number; fT: number; fW: number; fH: number } {
  const s = f.srcRect ?? { l: 0, t: 0, r: 0, b: 0 };
  const c = { l: s.l / 100000, t: s.t / 100000, r: s.r / 100000, b: s.b / 100000 };
  const fx = f.flipH ? { a: c.r, b: c.l } : { a: c.l, b: c.r };
  const fy = f.flipV ? { a: c.b, b: c.t } : { a: c.t, b: c.b };
  const w = h.rect.w, hh = h.rect.h;
  const fW = w / Math.max(0.001, 1 - fx.a - fx.b), fH = hh / Math.max(0.001, 1 - fy.a - fy.b);
  return { L: -w / 2, R: w / 2, T: -hh / 2, B: hh / 2, fL: -w / 2 - fx.a * fW, fT: -hh / 2 - fy.a * fH, fW, fH };
}

function cropHandle(o: ObjState): Handle | null {
  const h = activeHandle(o);
  if (o.crop === null || !h || h.id !== o.crop) { o.crop = null; return null; }
  return h;
}

function cropHit(o: ObjState, e: MouseEvent): string | null {
  const h = cropHandle(o);
  const f = h && formatNow(o, h);
  const cv = h && elOf(o).canvases[h.page];
  if (!h || !f || !cv) return null;
  const r = cv.getBoundingClientRect(), s = scaleOf(o);
  const a = (-h.rot * Math.PI) / 180;
  const dx = e.clientX - (r.left + (h.rect.x + h.rect.w / 2) * s), dy = e.clientY - (r.top + (h.rect.y + h.rect.h / 2) * s);
  const lx = (dx * Math.cos(a) - dy * Math.sin(a)) / s, ly = (dx * Math.sin(a) + dy * Math.cos(a)) / s;
  const g = cropGeom(h, f);
  const tol = (9 * dprOf()) / s;
  const xs: Record<string, number> = { w: g.L, c: 0, e: g.R }, ys: Record<string, number> = { n: g.T, c: 0, s: g.B };
  for (const hn of FLOAT_HANDLES) {
    const hx = xs[hn.includes('w') ? 'w' : hn.includes('e') ? 'e' : 'c']!, hy = ys[hn.includes('n') ? 'n' : hn.includes('s') ? 's' : 'c']!;
    if (Math.abs(lx - hx) <= tol && Math.abs(ly - hy) <= tol) return hn;
  }
  const inFull = lx >= g.fL && lx <= g.fL + g.fW && ly >= g.fT && ly <= g.fT + g.fH;
  const inFrame = lx >= g.L && lx <= g.R && ly >= g.T && ly <= g.B;
  return inFull || inFrame ? 'pan' : null;
}

/** `startCropDrag`: o fim grava o recorte (`setCrop`: a:srcRect e o tamanho; a posição não anda, ED-215). */
function startCropDrag(o: ObjState, e: MouseEvent, handle: string): void {
  const h = cropHandle(o);
  const f = h && formatNow(o, h);
  if (!h || !f) return;
  e.preventDefault();
  const s = scaleOf(o);
  const g0 = cropGeom(h, f);
  const rot = h.rot;
  const sx = e.clientX, sy = e.clientY;
  const MIN = 120;
  let moved = false;
  let crop = { l: 0, t: 0, r: 0, b: 0 };
  const move = (ev: MouseEvent): void => {
    if (!moved && Math.abs(ev.clientX - sx) + Math.abs(ev.clientY - sy) <= 2) return;
    moved = true;
    const rx = (ev.clientX - sx) / s, ry = (ev.clientY - sy) / s;
    const ar = (-rot * Math.PI) / 180;
    const dx = rx * Math.cos(ar) - ry * Math.sin(ar), dy = rx * Math.sin(ar) + ry * Math.cos(ar);
    let { L, R, T, B, fL, fT } = g0;
    if (handle === 'pan') { fL += dx; fT += dy; } else {
      if (handle.includes('w')) L = Math.min(R - MIN, L + dx);
      if (handle.includes('e')) R = Math.max(L + MIN, R + dx);
      if (handle.includes('n')) T = Math.min(B - MIN, T + dy);
      if (handle.includes('s')) B = Math.max(T + MIN, B + dy);
    }
    const dl = (L - fL) / g0.fW, dr = (fL + g0.fW - R) / g0.fW, dt = (T - fT) / g0.fH, db = (fT + g0.fH - B) / g0.fH;
    crop = { l: f.flipH ? dr : dl, r: f.flipH ? dl : dr, t: f.flipV ? db : dt, b: f.flipV ? dt : db };
    o.drag = { page: h.page, rect: { x: h.rect.x + h.rect.w / 2 + L, y: h.rect.y + h.rect.h / 2 + T, w: R - L, h: B - T }, rot, dashed: true };
    o.api.paint();
  };
  const up = (): void => {
    window.removeEventListener('mousemove', move);
    window.removeEventListener('mouseup', up);
    o.drag = null;
    if (!moved) { snapshotDiscardEvents(o); o.api.paint(); return; }
    cmd(o, { t: 'picture', v: { $t: 'setCrop', ...crop } }, 'cropDrag');
  };
  window.addEventListener('mousemove', move);
  window.addEventListener('mouseup', up);
}

// ───────────────────────── Desenhar Caixa de Texto, Criar Vínculo ─────────────────────────

function drawMouseDown(o: ObjState, e: MouseEvent): boolean {
  const dm = o.mode?.t === 'draw' ? o.mode : null;
  if (!dm || e.button !== 0) return false;
  const el = elOf(o);
  const s = scaleOf(o);
  const page = el.canvases.findIndex((c) => { const r = c.getBoundingClientRect(); return e.clientX >= r.left && e.clientX <= r.right && e.clientY >= r.top && e.clientY <= r.bottom; });
  if (page < 0) { cancelDraw(o); return true; }
  e.preventDefault();
  const cr = el.canvases[page]!.getBoundingClientRect();
  const at = (ev: MouseEvent): { x: number; y: number } => ({ x: (ev.clientX - cr.left) / s, y: (ev.clientY - cr.top) / s });
  const p0 = at(e);
  let p1 = p0;
  const anchor = o.api.session && o.api.dl ? o.api.session.hitTest(o.api.dl.inner, page, p0.x, p0.y) : null;
  const show = (): void => {
    o.drag = { page, rect: { x: Math.min(p0.x, p1.x), y: Math.min(p0.y, p1.y), w: Math.abs(p1.x - p0.x), h: Math.abs(p1.y - p0.y) }, rot: 0 };
    o.api.paint();
  };
  show();
  const move = (ev: MouseEvent): void => { p1 = at(ev); show(); };
  const up = (): void => {
    window.removeEventListener('mousemove', move);
    window.removeEventListener('mouseup', up);
    o.drag = null;
    o.mode = null;
    el.style.cursor = '';
    let x = Math.min(p0.x, p1.x), y = Math.min(p0.y, p1.y), w = Math.abs(p1.x - p0.x), h = Math.abs(p1.y - p0.y);
    if (w < 60 && h < 60) { w = 1440; h = 1440; x = p0.x; y = p0.y; }
    const before = new Set((objectsInBox(o)?.floats ?? []).map((f) => f.id));
    const ok = cmd(o, { t: 'drawing', v: { t: 'createDrawnTextbox', page, rect: { x, y, w, h }, ...(dm.vertical ? { vertical: true } : {}), ...(anchor ? { anchorPara: anchor.para } : {}), lang: uiLang(o.ed.el) } }, 'createDrawnTextbox');
    // com a âncora colocada, as referências do layout dão o deslocamento exato (o `placeAt` da sessão)
    const now = ok ? (objectsInBox(o)?.floats ?? []).find((f) => !before.has(f.id)) : undefined;
    if (now?.frame) cmd(o, { t: 'drawing', v: { t: 'placeAt', frame: now.frame, x, y } }, 'placeAt');
    el.chromeSync?.();
  };
  window.addEventListener('mousemove', move);
  window.addEventListener('mouseup', up);
  return true;
}

function cancelDraw(o: ObjState): void {
  if (o.mode?.t !== 'draw') return;
  o.mode = null;
  elOf(o).style.cursor = '';
  o.api?.paint();
  elOf(o).chromeSync?.();
}

function linkMouseDown(o: ObjState, e: MouseEvent): boolean {
  if (o.mode?.t !== 'link' || e.button !== 0) return false;
  e.preventDefault();
  // `this.hitTextbox(e)?.hostRun ?? this.hitFloat(e, 'textbox')?.run` (14947)
  const box = hitTextbox(o, e);
  const hit = (box && floatOfBox(o, box)) ?? hitFloat(o, e, 'textbox');
  o.mode = null;
  const el = elOf(o);
  el.style.cursor = '';
  if (hit) {
    if (hit.key === undefined) el.dispatchEvent(new CustomEvent('uw-blocked', { detail: { reason: 'linkTarget' } }));
    else {
      const ok = cmd(o, { t: 'drawing', v: { t: 'linkTextboxTo', key: hit.key } }, 'linkTextboxTo');
      void ok;
    }
  }
  el.chromeSync?.();
  return true;
}

// ───────────────────────── ganchos ─────────────────────────

/** O `mousedown` nas páginas: o acerto de modos e alças vem antes de tudo (`onPointerFirst`, também fora do texto), depois
 *  objetos e bordas antes do caminho do corpo e da frente S (a entrada na caixa), como no TS. */
export function onPointerFirst(o: ObjState, e: MouseEvent): boolean {
  if (!o.api?.session) return false;
  if (jobsBusy(o.api)) { e.preventDefault(); return true; }
  const el = elOf(o);
  if (e.button !== 0 || el.protectedMode) return false;
  // the TS `tableMouseDown` comes first (10141): a mousedown on a table edge starts the edge drag there, which frontier S
  // does not route: a warned no-op here (no object handle, no caret move: ED-219)
  if (onTableEdge(o, e)) {
    e.preventDefault();
    el.focusEditor();
    o.api.warnOnce('tableEdgeDrag', '[universal-write] engine="rust": the table edge drag is not routed in this wave (disabled)');
    return true;
  }
  if (drawMouseDown(o, e) || linkMouseDown(o, e)) { el.focusEditor(); return true; }
  if (o.crop !== null) {
    const ch = cropHit(o, e);
    if (ch) { el.focusEditor(); startCropDrag(o, e, ch); return true; }
    endCrop(o);
  }
  const hn = handleAt(o, e);
  if (hn) { el.focusEditor(); startFloatDrag(o, e, hn === 'rot' ? 'rotate' : 'resize', hn); return true; }
  return false;
}

/** The rest of the object branch of the `mousedown` (on the text hit): borders, objects, inline pictures, deselection. */
export function onPointer(o: ObjState, e: MouseEvent): boolean {
  if (!o.api?.session || jobsBusy(o.api)) return false;
  const el = elOf(o);
  if (e.button === 0 && !el.protectedMode) {
    const v = objects(o);
    if (!v) return false;
    // the box in edit: its border moves it (the TS mousedown step 2); a click inside is the text's (front S)
    if (v.scope === 'tb') {
      const ab = activeHandle(o);
      if (ab && onBorder(o, e, ab)) { el.focusEditor(); startFloatDrag(o, e, 'move', ''); return true; }
      return false;
    }
    // wave 6 (C): the session's `hitTextbox` answers every box of `contextTextboxRuns` (groups, cells, boxes in boxes:
    // S ED-102 closed), so the first cut's fallback to a content-bearing text-box float is gone
    let tb = hitTextbox(o, e);
    const img = tb ? hitFloat(o, e, 'image') : null;
    if (tb && img && img.rect.w * img.rect.h < tb.rect.w * tb.rect.h) tb = null;
    if (tb && onBorder(o, e, tb)) {
      // `floatSel = hostRun`: the box among the floats (a box the floats do not list, e.g. inside a group, selects
      // nothing the TS can drag either: its `startFloatDrag` finds no `activeFloat()`)
      const fh = floatOfBox(o, tb);
      if (fh) selectHandle(o, fh);
      el.focusEditor();
      startFloatDrag(o, e, 'move', '');
      return true;
    }
    // o miolo de uma caixa de texto: a entrada na caixa é da frente S
    if (tb) return false;
    const fh = img ?? hitFloat(o, e) ?? hitInline(o, e);
    if (fh) {
      selectHandle(o, fh);
      el.focusEditor();
      startFloatDrag(o, e, 'move', '');
      return true;
    }
    deselect(o);
    return false;
  }
  if (e.button === 2) {
    const v = objects(o);
    if (!v || v.scope === 'tb') return false;
    const f2 = hitFloat(o, e);
    const ph = (f2 && f2.kind !== 'textbox' ? f2 : null) ?? hitInline(o, e);
    if (ph) { selectHandle(o, ph); el.focusEditor(); return true; }
  }
  return false;
}

/** O `keydown` dos objetos (o ramo `floatSel` do `bindEvents` do TS), as quebras (Ctrl+Enter) e o Modo de Leitura. */
export function onKey(o: ObjState, e: KeyboardEvent): boolean {
  if (!o.api?.session) return false;
  const el = elOf(o);
  const k = e.key;
  const ctrl = e.ctrlKey || e.metaKey;
  const done = (): boolean => { e.preventDefault(); return true; };
  if (jobsBusy(o.api)) return done();
  if (k === 'Escape' && o.mode) { if (o.mode.t === 'draw') cancelDraw(o); else { o.mode = null; el.style.cursor = ''; el.chromeSync?.(); } return done(); }
  if (el.view === 'read') {
    // the TS Read-mode `keydown` listener of the element: the shell stops the arrows and PageUp/PageDown (they move the
    // caret too, as the TS input listener does before the element's), so they nudge here; Space reaches the element as
    // the forwarded copy and the TS listener (not shadowed) nudges once there
    if (k === 'ArrowRight' || k === 'PageDown') { e.preventDefault(); el.nudgeRead?.call(el, 1); }
    else if (k === 'ArrowLeft' || k === 'PageUp') { e.preventDefault(); el.nudgeRead?.call(el, -1); }
    return false;
  }
  const v = objects(o);
  // Esc in a text box: front S leaves the text; the box stays selected as a shape (the TS `Escape && tbEdit` branch:
  // `exitTextbox()` then `floatSel = run` when the box is among the floats)
  if (k === 'Escape' && v?.scope === 'tb' && v.box !== null) {
    const box = v.box;
    queueMicrotask(() => {
      const now = objects(o);
      const f = now && now.scope !== 'tb' ? now.floats.find((x) => x.id === box) : undefined;
      if (f) { cmd(o, { t: 'drawing', v: { t: 'selectObject', index: f.i } }, 'selectObject'); el.chromeSync?.(); }
    });
    return false;
  }
  const sel = v && v.scope !== 'tb' ? selectedHandle(o) : null;
  if (sel) {
    // caixa de texto selecionada: digitar entra no texto dela (a frente S)
    if (sel.kind === 'textbox' && k.length === 1 && !ctrl && !e.altKey) return false;
    if ((k === 'Escape' || k === 'Enter') && o.crop !== null) { endCrop(o); return done(); }
    if (k === 'Escape') {
      cmd(o, { t: 'drawing', v: { t: 'selectObject' } }, 'selectObject');
      if (sel.inline) {
        const { anchor, focus } = el.selection;
        const end = anchor.para > focus.para || (anchor.para === focus.para && anchor.offset > focus.offset) ? anchor : focus;
        el.setSelection(end, end);
      }
      el.chromeSync?.();
      return done();
    }
    // wave 6 (C): the TS Ctrl block (10563-10578: moveWord, deleteWord) runs before the `floatSel` branch, so
    // Ctrl+Delete/Backspace and Ctrl+Left/Right go on to the shell's word keys with an object selected
    if ((k === 'Delete' || k === 'Backspace') && !ctrl) {
      if (!o.api.locked()) {
        // um comando por objeto selecionado (o TS apaga todos num passo: ED-211)
        const all = selectedHandles(o);
        for (let i = 0; i < Math.max(1, all.length); i++) {
          const cur = selectedHandle(o);
          if (!cur) break;
          cmd(o, cur.kind === 'image' ? { t: 'picture', v: { $t: 'deleteSelectedImage' } } : { t: 'drawing', v: { t: 'deleteSelectedShape' } }, 'deleteSelectedFloats');
        }
      }
      return done();
    }
    if (!sel.inline && (k === 'ArrowUp' || k === 'ArrowDown' || (!ctrl && (k === 'ArrowLeft' || k === 'ArrowRight')))) {
      e.preventDefault();
      if (o.api.locked()) return true;
      const step = e.ctrlKey ? 15 : 90;
      nudge(o, k === 'ArrowLeft' ? -step : k === 'ArrowRight' ? step : 0, k === 'ArrowUp' ? -step : k === 'ArrowDown' ? step : 0);
      return true;
    }
  }
  if (k === 'Enter' && ctrl && !e.altKey) {
    // the TS order: in a cell Ctrl+Shift+Enter is `splitTable` (front S's key hook, after this one), and the generic
    // `nav.Enter` comes last: no break in a note (`ctrl && this.noteEdit ? undefined`; the shell forwards the key, owned)
    const c = storyCtx(o);
    if (c && (outerCtx(c.ctx).kind === 'note' || (c.ctx.kind === 'cell' && e.shiftKey && !c.locked))) return false;
    e.preventDefault();
    if (e.shiftKey) el.insertColumnBreak(); else el.insertPageBreak();
    return true;
  }
  return false;
}

/** O cursor do ponteiro sobre alças e objetos (o `mousemove` do TS). */
export function onHover(o: ObjState, e: MouseEvent): void {
  if (!o.api?.session || o.drag || o.mode) return;
  const el = elOf(o);
  if (o.crop !== null) {
    const ch = cropHit(o, e);
    if (ch) { el.style.cursor = ch === 'pan' ? 'move' : cursorForHandle(ch, activeHandle(o)?.rot ?? 0); return; }
  }
  const hn = handleAt(o, e);
  if (hn) { el.style.cursor = cursorForHandle(hn, activeHandle(o)?.rot ?? 0); return; }
  if (hitFloat(o, e)) { el.style.cursor = 'move'; return; }
  if (el.style.cursor === 'move' || el.style.cursor.endsWith('-resize') || el.style.cursor === 'grab') el.style.cursor = '';
}

/** Uma imagem em linha selecionada esconde o realce da seleção de texto (o `picSel` do TS). */
export function hideTextSelection(o: ObjState): boolean {
  const v = objects(o);
  const id = v?.sel[0];
  return id !== undefined && !!v && v.scope !== 'tb' && v.inline.some((p) => p.id === id);
}

/** A pintura das alças, do modo Cortar e dos fantasmas na página `page` (o `paintFloatAndCaret` do TS). */
export function paintPage(o: ObjState, ctx: CanvasRenderingContext2D, page: number, scale: number): void {
  const el = elOf(o);
  if (el.view === 'read') return;
  const dpr = dprOf();
  ctx.save();
  ctx.setTransform(1, 0, 0, 1, 0, 0);
  try {
    const h = activeHandle(o);
    if (h && h.page === page) {
      const crop = o.crop !== null && h.id === o.crop && h.kind === 'image' ? formatNow(o, h) : null;
      if (crop) paintCrop(o, ctx, h, crop, scale, dpr);
      else paintHandles(ctx, h, scale, dpr, (h.kind === 'image' && !h.editing) || h.editable);
    }
    const g = o.drag;
    if (g && g.page === page) {
      ctx.save();
      ctx.translate((g.rect.x + g.rect.w / 2) * scale, (g.rect.y + g.rect.h / 2) * scale);
      if (g.rot) ctx.rotate((g.rot * Math.PI) / 180);
      ctx.strokeStyle = '#2b579a';
      ctx.lineWidth = Math.max(1, dpr);
      ctx.setLineDash([4 * dpr, 3 * dpr]);
      ctx.strokeRect((-g.rect.w / 2) * scale, (-g.rect.h / 2) * scale, g.rect.w * scale, g.rect.h * scale);
      ctx.restore();
    }
  } finally { ctx.restore(); }
}

function paintHandles(ctx: CanvasRenderingContext2D, h: Handle, scale: number, dpr: number, round: boolean): void {
  const w = h.rect.w * scale, hh = h.rect.h * scale;
  const cx = h.rect.x * scale + w / 2, cy = h.rect.y * scale + hh / 2;
  ctx.save();
  ctx.translate(cx, cy);
  if (h.rot) ctx.rotate((h.rot * Math.PI) / 180);
  ctx.strokeStyle = '#2b579a';
  // the text of the box in edit: a dashed outline; a selected shape: a solid one
  if (h.editing) ctx.setLineDash([4, 3]);
  ctx.lineWidth = Math.max(1, dpr);
  ctx.strokeRect(-w / 2, -hh / 2, w, hh);
  ctx.setLineDash([]);
  const hs = 6 * dpr;
  const pts: [number, number][] = [[-w / 2, -hh / 2], [0, -hh / 2], [w / 2, -hh / 2], [w / 2, 0], [w / 2, hh / 2], [0, hh / 2], [-w / 2, hh / 2], [-w / 2, 0]];
  ctx.fillStyle = '#ffffff';
  for (const [px, py] of pts) {
    if (round) { ctx.beginPath(); ctx.arc(px, py, hs / 2 + 0.5 * dpr, 0, Math.PI * 2); ctx.fill(); ctx.stroke(); } else { ctx.fillRect(px - hs / 2, py - hs / 2, hs, hs); ctx.strokeRect(px - hs / 2, py - hs / 2, hs, hs); }
  }
  if (round) {
    const gy = -hh / 2 - ROT_HANDLE_GAP * dpr;
    const r = 5 * dpr;
    ctx.beginPath();
    ctx.arc(0, gy, r, -Math.PI * 0.15, Math.PI * 1.55);
    ctx.lineWidth = 1.6 * dpr;
    ctx.stroke();
    const ex = r * Math.cos(-Math.PI * 0.15), ey = gy + r * Math.sin(-Math.PI * 0.15);
    ctx.beginPath();
    ctx.moveTo(ex - 3 * dpr, ey - 2.5 * dpr);
    ctx.lineTo(ex + 0.8 * dpr, ey + 0.5 * dpr);
    ctx.lineTo(ex + 2.5 * dpr, ey - 3.5 * dpr);
    ctx.stroke();
  }
  ctx.restore();
}

function paintCrop(o: ObjState, ctx: CanvasRenderingContext2D, h: Handle, f: StateFmt, scale: number, dpr: number): void {
  const g = cropGeom(h, f);
  const s = (v: number): number => v * scale;
  ctx.save();
  ctx.translate(s(h.rect.x + h.rect.w / 2), s(h.rect.y + h.rect.h / 2));
  if (h.rot) ctx.rotate((h.rot * Math.PI) / 180);
  const bmp = bitmapOf(o, h);
  if (bmp) {
    ctx.save();
    ctx.beginPath();
    ctx.rect(s(g.fL), s(g.fT), s(g.fW), s(g.fH));
    ctx.rect(s(g.L), s(g.T), s(g.R - g.L), s(g.B - g.T));
    ctx.clip('evenodd');
    ctx.globalAlpha = 0.45;
    ctx.translate(s(g.fL + g.fW / 2), s(g.fT + g.fH / 2));
    ctx.scale(f.flipH ? -1 : 1, f.flipV ? -1 : 1);
    ctx.drawImage(bmp, -s(g.fW) / 2, -s(g.fH) / 2, s(g.fW), s(g.fH));
    ctx.restore();
  }
  ctx.lineWidth = Math.max(1, dpr);
  ctx.strokeStyle = '#7f7f7f';
  ctx.strokeRect(s(g.fL), s(g.fT), s(g.fW), s(g.fH));
  ctx.strokeStyle = '#2b579a';
  ctx.strokeRect(s(g.L), s(g.T), s(g.R - g.L), s(g.B - g.T));
  const len = 12 * dpr, th = 4 * dpr;
  const L = s(g.L), R = s(g.R), T = s(g.T), B = s(g.B), mx = (L + R) / 2, my = (T + B) / 2;
  const bars: [number, number, number, number][] = [
    [L, T, len, th], [L, T, th, len], [R - len, T, len, th], [R - th, T, th, len],
    [L, B - th, len, th], [L, B - len, th, len], [R - len, B - th, len, th], [R - th, B - len, th, len],
    [mx - len / 2, T, len, th], [mx - len / 2, B - th, len, th], [L, my - len / 2, th, len], [R - th, my - len / 2, th, len],
  ];
  ctx.fillStyle = '#ffffff';
  for (const [x, y, w, hh] of bars) ctx.fillRect(x - dpr, y - dpr, w + 2 * dpr, hh + 2 * dpr);
  ctx.fillStyle = '#000000';
  for (const [x, y, w, hh] of bars) ctx.fillRect(x, y, w, hh);
  ctx.restore();
}

function endCrop(o: ObjState): void {
  if (o.crop === null) return;
  o.crop = null;
  o.api?.paint();
  elOf(o).chromeSync?.();
}

/** Os métodos públicos dos modos (Cortar, Desenhar Caixa de Texto, Criar Vínculo). */
export function modeHandlers(o: ObjState): Record<string, (...args: never[]) => unknown> {
  const h: Record<string, (...args: any[]) => unknown> = {
    startImageCrop: () => {
      const s = selectedHandle(o);
      if (!s || s.kind !== 'image' || !o.api?.session || o.api.locked()) return;
      o.crop = s.id;
      o.api.paint();
      elOf(o).chromeSync?.();
    },
    endImageCrop: () => endCrop(o),
    croppingImage: () => !!cropHandle(o),
    drawTextbox: (vertical = false) => {
      const el = elOf(o);
      if (!o.api?.session || o.api.locked() || el.view !== 'print') return;
      deselect(o);
      o.mode = { t: 'draw', vertical };
      el.style.cursor = 'crosshair';
      el.chromeSync?.();
    },
    drawingTextbox: () => o.mode?.t === 'draw',
    cancelDrawTextbox: () => cancelDraw(o),
    createTextboxLink: () => {
      const st = o.api?.session?.query({ t: 'selectedObjectState' }) as { value?: { kind: string; linkNext?: boolean } } | undefined;
      if (!st?.value || st.value.kind !== 'textbox' || st.value.linkNext || o.api.locked()) return false;
      o.mode = { t: 'link' };
      const el = elOf(o);
      el.style.cursor = 'copy';
      el.chromeSync?.();
      return true;
    },
    linkingTextbox: () => o.mode?.t === 'link',
    cancelTextboxLink: () => {
      if (o.mode?.t !== 'link') return;
      o.mode = null;
      const el = elOf(o);
      el.style.cursor = '';
      el.chromeSync?.();
    },
  };
  return h as Record<string, (...args: never[]) => unknown>;
}
