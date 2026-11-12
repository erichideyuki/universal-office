// Wave 6 (L, D-L4): the band of a page that a relayout changed — the vertical extent, in twips, of the items that differ
// between the previous layout's decoded page and this layout's, so the editor repaints only that band of the page's cached
// base canvas (record: core/docs/modules/uo-bindings.md "Wave 6 — L"). Conservative by construction: any difference this
// module cannot bound (the page background, the header/footer layer split, a clip or transform pushed or popped in the
// changed items, a changed item under a pushed transform, an item kind without a plain geometry, text effects) answers
// `null`, and the caller repaints the whole page as before. Erasable TypeScript only (the flags of paint.ts).

import type { Item, Page, Tables } from './gen/render.ts';

/** a decoded page as the band compares it: its render message, the tables it was decoded with, its header/footer item count */
export interface BandPage {
  page: Page;
  tables: Tables;
  hf: number;
}

/** per face and image index: whether this layout's bytes are the previous layout's (`WriteLayout.shared`) */
export interface BandShared {
  faces: boolean[];
  images: boolean[];
}

/** the item fields that are table indices, and their tables */
const REFS: Record<string, 'faces' | 'images' | 'textFx' | 'pageFills' | 'cust'> = {
  face: 'faces',
  image: 'images',
  fx: 'textFx',
  backgroundFill: 'pageFills',
  cust: 'cust',
};

class Cmp {
  readonly #memo = new Map<string, boolean>();
  readonly a: Tables;
  readonly b: Tables;
  readonly shared: BandShared;
  constructor(a: Tables, b: Tables, shared: BandShared) {
    this.a = a;
    this.b = b;
    this.shared = shared;
  }

  /** entry `i` of table `t` paints the same in both layouts (faces and images: the same bytes too) */
  ref(t: 'faces' | 'images' | 'textFx' | 'pageFills' | 'cust', i: number): boolean {
    const k = `${t}:${i}`;
    const had = this.#memo.get(k);
    if (had !== undefined) return had;
    this.#memo.set(k, false); // a cycle answers "different"
    const x = (this.a[t] as unknown[])[i], y = (this.b[t] as unknown[])[i];
    const bytes = t === 'faces' ? this.shared.faces[i] === true : t === 'images' ? this.shared.images[i] === true : true;
    const r = bytes && x !== undefined && y !== undefined && this.eq(x, y);
    this.#memo.set(k, r);
    return r;
  }

  /** structural equality; a table index is equal when it is the same index of an entry that paints the same */
  eq(a: unknown, b: unknown): boolean {
    if (a === b) return true;
    if (typeof a !== 'object' || typeof b !== 'object' || a === null || b === null) return false;
    if (Array.isArray(a)) {
      if (!Array.isArray(b) || a.length !== b.length) return false;
      for (let i = 0; i < a.length; i++) if (!this.eq(a[i], b[i])) return false;
      return true;
    }
    if (ArrayBuffer.isView(a)) {
      if (!ArrayBuffer.isView(b) || a.constructor !== b.constructor) return false;
      const x = a as unknown as ArrayLike<unknown>, y = b as unknown as ArrayLike<unknown>;
      if (x.length !== y.length) return false;
      for (let i = 0; i < x.length; i++) if (x[i] !== y[i]) return false;
      return true;
    }
    if (Array.isArray(b) || ArrayBuffer.isView(b)) return false;
    const oa = a as Record<string, unknown>, ob = b as Record<string, unknown>;
    const ka = Object.keys(oa);
    if (ka.length !== Object.keys(ob).length) return false;
    for (const k of ka) {
      if (!Object.prototype.hasOwnProperty.call(ob, k)) return false;
      const va = oa[k], vb = ob[k];
      const t = REFS[k];
      if (t !== undefined && typeof va === 'number') {
        if (va !== vb || !this.ref(t, va)) return false;
        continue;
      }
      if (k === 'glyphs' && Array.isArray(va) && Array.isArray(vb)) {
        // the glyph columns (cp, x, gid): the bulk of a page, compared without the generic walk
        if (va.length !== vb.length) return false;
        for (let i = 0; i < va.length; i++) {
          const g = va[i] as { cp: number; x: number; gid?: number }, h = vb[i] as { cp: number; x: number; gid?: number };
          if (g.cp !== h.cp || g.x !== h.x || g.gid !== h.gid || Object.keys(g).length !== Object.keys(h).length) return false;
        }
        continue;
      }
      if (!this.eq(va, vb)) return false;
    }
    return true;
  }
}

/** the vertical extent (twips) of an item's ink, or null when it has no plain one */
function extent(it: Item): [number, number] | null {
  switch (it.t) {
    case 'glyphs': {
      if (it.fx !== undefined) return null; // text effects (shadow, glow, reflection): no plain extent
      const em = it.sizePt * 20;
      return [it.y - 2 * em, it.y + em];
    }
    case 'rect':
      return [Math.min(it.y, it.y + it.h), Math.max(it.y, it.y + it.h)];
    case 'line': {
      const lw = it.widthPt * 20;
      return [Math.min(it.y1, it.y2) - lw, Math.max(it.y1, it.y2) + lw];
    }
    default:
      return null;
  }
}

/** the largest `y`-like coordinate spread of a path's commands (M/L points, cubic control points), or null */
function pathExtent(cmds: readonly object[]): [number, number] | null {
  let lo = Infinity, hi = -Infinity;
  for (const c of cmds) {
    for (const [k, v] of Object.entries(c)) {
      if (k[0] === 'y' && typeof v === 'number') { if (v < lo) lo = v; if (v > hi) hi = v; }
    }
  }
  return lo <= hi ? [lo, hi] : null;
}

/**
 * The vertical extent (twips) that a picture, shape or path can paint, or null when unknown (effects, picture looks, a
 * shadow): measured, Chromium resamples a picture differently when it is drawn under a clip that cuts it, so a band must
 * not cut one of these.
 */
function objectExtent(it: Item): [number, number] | null {
  switch (it.t) {
    case 'image': {
      if (it.pic !== undefined) return null;
      const r = Math.hypot(it.w, it.h) / 2, cy = it.y + it.h / 2;
      return [cy - r, cy + r];
    }
    case 'shape': {
      if (it.shadow !== undefined || it.fx !== undefined) return null;
      const r = Math.hypot(it.w, it.h) / 2 + (it.stroke ? it.stroke.widthPt * 20 : 0), cy = it.y + it.h / 2;
      return [cy - r, cy + r];
    }
    case 'path': {
      const e = pathExtent(it.cmds);
      const sw = it.stroke ? it.stroke.widthPt * 20 : 0;
      return e ? [e[0] - sw, e[1] + sw] : null;
    }
    default:
      return null;
  }
}

/**
 * The band (twips, page coordinates) that holds every pixel `b` paints differently from `a`: `'same'` when the pages paint
 * the same, `null` when it cannot be bounded (repaint the whole page). `a` is the previous layout's page of the same index.
 * `slackTw`: what the caller adds around the band (its pixel margin, in twips): no picture, shape or path may reach into
 * the band so widened (they are painted whole or not at all), nor sit under a transform.
 */
export function changedBand(a: BandPage, b: BandPage, shared: BandShared, slackTw = 0): { y0: number; y1: number } | 'same' | null {
  const pa = a.page, pb = b.page;
  if (pa.widthTw !== pb.widthTw || pa.heightTw !== pb.heightTw || pa.background !== pb.background || !!pa.backgroundShow !== !!pb.backgroundShow) return null;
  if (a.hf !== b.hf) return null;
  const c = new Cmp(a.tables, b.tables, shared);
  if (pa.backgroundFill !== pb.backgroundFill || (pa.backgroundFill !== undefined && !c.ref('pageFills', pa.backgroundFill))) return null;
  const A = pa.items, B = pb.items;
  const m = A.length, n = B.length;
  // the save stack of paintItems over the equal prefix (clip and transform pushes; any other stack item pops)
  const stack: boolean[] = [];
  let p = 0;
  while (p < m && p < n && c.eq(A[p], B[p])) {
    const it = A[p]!;
    if (it.t === 'clip') { if (it.op === 'push') stack.push(false); else stack.pop(); }
    else if (it.t === 'xform') { if (it.op === 'push' && it.m) stack.push(true); else stack.pop(); }
    p++;
  }
  if (p === m && p === n) return 'same';
  let s = 0;
  while (s < m - p && s < n - p && c.eq(A[m - 1 - s], B[n - 1 - s])) s++;
  // the header/footer layer (its own alpha) or a transform around the changed items: the whole page
  if (p < a.hf || stack.includes(true)) return null;
  let y0 = Infinity, y1 = -Infinity;
  for (const [items, end] of [[A, m - s], [B, n - s]] as const) {
    for (let i = p; i < end; i++) {
      const e = extent(items[i]!);
      if (!e || !Number.isFinite(e[0]) || !Number.isFinite(e[1])) return null;
      if (e[0] < y0) y0 = e[0];
      if (e[1] > y1) y1 = e[1];
    }
  }
  if (!(y0 <= y1)) return null;
  // the pictures, shapes and paths of the page (the equal ones; the changed ones answered null above): none under a
  // transform, none reaching into the widened band
  const lo = y0 - slackTw, hi = y1 + slackTw;
  let xf = 0;
  const xs: boolean[] = [];
  for (const it of B) {
    if (it.t === 'clip') { if (it.op === 'push') xs.push(false); else if (xs.pop()) xf--; continue; }
    if (it.t === 'xform') { if (it.op === 'push' && it.m) { xs.push(true); xf++; } else if (xs.pop()) xf--; continue; }
    if (it.t !== 'image' && it.t !== 'shape' && it.t !== 'path') continue;
    if (xf > 0) return null;
    const e = objectExtent(it);
    if (!e || !Number.isFinite(e[0]) || !Number.isFinite(e[1])) return null;
    if (e[1] >= lo && e[0] <= hi) return null;
  }
  return { y0, y1 };
}
