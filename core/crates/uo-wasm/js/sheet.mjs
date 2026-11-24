// The JS side of the Sheet viewer exports of uo-wasm (src/sheet.rs, feature `sheet`; record core/docs/modules/uo-xlsx.md
// "C4 — as built"): the workbook handle with explicit free() and a FinalizationRegistry, the decoders of the result layouts and
// the two-phase measurement loop. Plain ES module (Node and WebKit tests import it as is).
//
//   const uo = bindSheet((await WebAssembly.instantiate(bytes, {})).instance.exports);
//   const book = uo.open(xlsxBytes);                   // throws SheetError
//   book.activate(0, measurer);                        // loads the sheet; measurer = { advance(font, cp), width(font, text), box(font) }
//   const bytes = book.viewport({ sheet: 0, width, height, dpr, scrollX, scrollY, zoom, selection: [1,1,1,1,1,1] }, measurer);
//   // `bytes` is one message of the Slides list stream: decodeSlidesList(bytes, tables) then SlidesPainter.paint
//   book.free();

import { bindRender } from './render.mjs';

export const STATUS = { 0: 'ok', 1: 'none', 3: 'the workbook could not be read', 5: 'invalid argument', 6: 'no such sheet', 10: 'the list did not encode', 11: 'null handle', 12: 'measurements needed' };

/** An error of a Sheet export: `status` (the table above) and the module's message. */
export class SheetError extends Error {
  constructor(status, what, detail) {
    super(`${what}: ${STATUS[status] ?? status}${detail ? ` (${detail})` : ''}`);
    this.status = status;
    this.detail = detail ?? '';
  }
}

const dec = new TextDecoder();
const enc = new TextEncoder();

export function bindSheet(w) {
  const r = bindRender(w);
  const { Buf, put } = r;
  const registry = new FinalizationRegistry((ptr) => w.uo_sheet_free(ptr));
  const take = (ptr) => new Buf(ptr).take();
  const dv = (d) => new DataView(d.buffer, d.byteOffset, d.byteLength);
  const arg = (bytes) => (bytes.length === 0 ? [0, 0] : [put(bytes), bytes.length]);
  const MAX_ROUNDS = 8;

  const readFont = (data) => {
    const d = dv(data);
    let at = 0;
    const f64 = () => { const v = d.getFloat64(at, true); at += 8; return v; };
    const u32 = () => { const v = d.getUint32(at, true); at += 4; return v; };
    const str = () => { const n = u32(); if (n === 0xffffffff) return null; const s = dec.decode(data.subarray(at, at + n)); at += n; return s; };
    const sizePt = f64(), zoom = f64(), ppem = d.getInt32(at, true); at += 4;
    const bold = data[at++] !== 0, italic = data[at++] !== 0;
    const name = str(), alt = str(), metric = str();
    return { sizePt, zoom, ppem, bold, italic, name, alt, metric };
  };

  class Book {
    #ptr;
    constructor(ptr) { this.#ptr = ptr; registry.register(this, ptr, this); }
    get freed() { return this.#ptr === 0; }
    free() { if (this.#ptr) { registry.unregister(this); w.uo_sheet_free(this.#ptr); this.#ptr = 0; } }
    #live() { if (!this.#ptr) throw new SheetError(11, 'sheet handle'); return this.#ptr; }

    /** The sheets: `{ name, kind: 'sheet'|'chart'|'none', state, active }`. */
    sheets() {
      const b = take(w.uo_sheet_sheets(this.#live()));
      if (b.status !== 0) throw new SheetError(b.status, 'sheets');
      const d = dv(b.data);
      let at = 0;
      const n = d.getUint32(at, true); at += 4;
      const out = [];
      for (let i = 0; i < n; i++) {
        const kind = b.data[at++], state = b.data[at++], active = b.data[at++] !== 0;
        const len = d.getUint32(at, true); at += 4;
        out.push({ name: dec.decode(b.data.subarray(at, at + len)), kind: ['sheet', 'chart', 'none'][kind] ?? 'none', state, active });
        at += len;
      }
      return out;
    }

    /** What the shell must measure now (after a status 12): `{ texts: [{ font, text }], boxes: [font] }`. */
    #needs() {
      const b = take(w.uo_sheet_needs(this.#live()));
      const d = dv(b.data);
      let at = 0;
      const texts = [];
      const nt = d.getUint32(at, true); at += 4;
      for (let i = 0; i < nt; i++) {
        const font = d.getUint32(at, true); at += 4;
        const n = d.getUint32(at, true); at += 4;
        const units = new Uint16Array(n);
        for (let k = 0; k < n; k++) { units[k] = d.getUint16(at, true); at += 2; }
        texts.push({ font, text: String.fromCharCode(...units) });
      }
      const boxes = [];
      const nb = d.getUint32(at, true); at += 4;
      for (let i = 0; i < nb; i++) { boxes.push(d.getUint32(at, true)); at += 4; }
      return { texts, boxes };
    }

    font(id) {
      const b = take(w.uo_sheet_font(this.#live(), id));
      if (b.status !== 0) throw new SheetError(b.status, 'font');
      return readFont(b.data);
    }

    /** Answers the pending measurements with `m` (`advance(font, cp)`, `width(font, text)`, `box(font) -> [asc, desc]`). */
    #answer(m) {
      const { texts, boxes } = this.#needs();
      const parts = [];
      const fonts = new Map();
      const fontOf = (id) => { let f = fonts.get(id); if (!f) { f = this.font(id); fonts.set(id, f); } return f; };
      for (const t of texts) {
        const f = fontOf(t.font);
        const cps = [...t.text];
        if (cps.length === 1 && t.text.length === cps[0].length && (cps[0].codePointAt(0) ?? 0) < 0x110000 && !t.complex) {
          const b = new Uint8Array(13); const d = dv(b);
          b[0] = 0; d.setUint32(1, t.font, true); d.setUint32(5, cps[0].codePointAt(0), true); d.setInt32(9, m.advance(f, cps[0].codePointAt(0)), true);
          parts.push(b);
        } else {
          const n = t.text.length;
          const b = new Uint8Array(1 + 4 + 4 + n * 2 + 4); const d = dv(b);
          b[0] = 1; d.setUint32(1, t.font, true); d.setUint32(5, n, true);
          for (let k = 0; k < n; k++) d.setUint16(9 + k * 2, t.text.charCodeAt(k), true);
          d.setInt32(9 + n * 2, m.width(f, t.text), true);
          parts.push(b);
        }
      }
      for (const id of boxes) {
        const b = new Uint8Array(21); const d = dv(b);
        const [a, de] = m.box(fontOf(id)) ?? [NaN, NaN];
        b[0] = 2; d.setUint32(1, id, true); d.setFloat64(5, a, true); d.setFloat64(13, de, true);
        parts.push(b);
      }
      const all = new Uint8Array(parts.reduce((s, p) => s + p.length, 0));
      let o = 0;
      for (const p of parts) { all.set(p, o); o += p.length; }
      const [p, n] = arg(all);
      const st = w.uo_sheet_provide(this.#live(), p, n);
      if (st !== 0) throw new SheetError(st, 'provide');
    }

    /** Makes sheet `index` the active one. */
    activate(index, m) {
      for (let i = 0; i < MAX_ROUNDS; i++) {
        const st = w.uo_sheet_activate(this.#live(), index);
        if (st === 0) return;
        if (st !== 12) throw new SheetError(st, 'activate');
        this.#answer(m);
      }
      throw new SheetError(12, 'activate', 'does not settle');
    }

    setAccent(css) { const [p, n] = arg(enc.encode(css)); w.uo_sheet_set_accent(this.#live(), p, n); }
    setSpill(c1, r1, c2, r2) { w.uo_sheet_set_spill(this.#live(), c1, r1, c2, r2); }

    /** One frame: the bytes of a message of the list stream. */
    viewport(q, m) {
      const sel = Uint32Array.from(q.selection ?? [1, 1, 1, 1, 1, 1]);
      const sp = put(new Uint8Array(sel.buffer));
      for (let i = 0; i < MAX_ROUNDS; i++) {
        const b = take(w.uo_sheet_viewport(this.#live(), q.sheet, q.width, q.height, q.dpr ?? 1, q.scrollX ?? 0, q.scrollY ?? 0, q.zoom ?? 1, sp, q.capture ? 1 : 0, q.editing?.[0] ?? 0, q.editing?.[1] ?? 0));
        if (b.status === 0) return b.data;
        if (b.status !== 12) throw new SheetError(b.status, 'viewport');
        this.#answer(m);
      }
      throw new SheetError(12, 'viewport', 'does not settle');
    }

    /** The RGBA bytes of a bitmap handle of the stream. */
    bitmap(handle) { const b = take(w.uo_sheet_bitmap(this.#live(), handle)); return b.status === 0 ? b.data : null; }
    /** `{ width, height }` of the active sheet at zoom 1 (px). */
    extent() { const b = take(w.uo_sheet_extent(this.#live())); if (b.status !== 0) return null; const d = dv(b.data); return { width: d.getFloat64(0, true), height: d.getFloat64(8, true) }; }
    /** The hyperlink target of a cell (http, https, mailto, tel; `#ref` inside the workbook) or null. */
    linkAt(sheet, col, row) { const b = take(w.uo_sheet_link_at(this.#live(), sheet, col, row)); return b.status === 0 ? dec.decode(b.data) : null; }
    /** The open-recalc plan: 'none' | 'partial' | 'full'. */
    recalcPlan() { const b = take(w.uo_sheet_recalc_plan(this.#live())); return ['none', 'partial', 'full'][b.data[0]] ?? 'none'; }
  }

  return {
    open(bytes, clock = { year: 2025, month: 1, day: 15, seconds: 43200 }) {
      const [p, n] = arg(bytes);
      const ptr = w.uo_sheet_open(p, n, clock.year, clock.month, clock.day, clock.seconds);
      if (ptr === 0) { const e = take(w.uo_sheet_error()); throw new SheetError(e.status, 'open', dec.decode(e.data)); }
      return new Book(ptr);
    },
    liveBytes: () => (w.uo_sheet_live_bytes ? Number(w.uo_sheet_live_bytes()) : 0),
  };
}
