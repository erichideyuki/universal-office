// The JS side of the Sheet EDITOR exports of uo-wasm (src/sheet_edit.rs, features `sheet` + `sheet-edit` = `uo-sheet-editor.wasm`;
// record core/docs/modules/uo-xlsx.md "C5b — as built"): the editing session handle with explicit free() and a
// FinalizationRegistry, the command line protocol (JSON answers), the save and the viewport with the two-phase measurement loop
// (the same layouts as js/sheet.mjs). Plain ES module (Node and WebKit tests import it as is).
//
//   const ue = bindSheetEditor((await WebAssembly.instantiate(bytes, {})).instance.exports);
//   const ed = ue.open(xlsxBytes, { locale: 'pt-BR' });     // throws SheetEditError; empty bytes = a blank workbook
//   ed.cmd('select', 'B2'); const r = ed.cmd('enter', '=SOMA(A1:A3)');   // r = { result, state }
//   const bytes = ed.viewport({ sheet: 0, width, height, zoom: 1, selection: [...] }, measurer);
//   const xlsx = ed.save(); ed.free();

import { bindRender } from './render.mjs';

export const STATUS = { 0: 'ok', 3: 'the workbook could not be read', 5: 'invalid argument', 6: 'no such sheet', 10: 'the list did not encode', 11: 'null handle', 12: 'measurements needed', 13: 'the save failed' };

/** An error of an editor export, or a command the session refused (`status` 0 then, `detail` its message). */
export class SheetEditError extends Error {
  constructor(status, what, detail) {
    super(`${what}: ${status === 0 ? 'refused' : STATUS[status] ?? status}${detail ? ` (${detail})` : ''}`);
    this.status = status;
    this.detail = detail ?? '';
  }
}

/** Whether a module instance has the editor exports (`uo-sheet-editor.wasm`, not the viewer's `uo-sheet.wasm`). */
export function hasSheetEditorExports(w) { return typeof w.uo_sheet_edit_open === 'function'; }

const dec = new TextDecoder();
const enc = new TextEncoder();

export function bindSheetEditor(w) {
  if (!hasSheetEditorExports(w)) throw new SheetEditError(5, 'bindSheetEditor', 'the module has no editor exports (uo-sheet-editor.wasm)');
  const r = bindRender(w);
  const { Buf, put } = r;
  const registry = new FinalizationRegistry((ptr) => w.uo_sheet_edit_free(ptr));
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

  class SheetEditor {
    #ptr;
    constructor(ptr) { this.#ptr = ptr; registry.register(this, ptr, this); }
    get freed() { return this.#ptr === 0; }
    free() { if (this.#ptr) { registry.unregister(this); w.uo_sheet_edit_free(this.#ptr); this.#ptr = 0; } }
    #live() { if (!this.#ptr) throw new SheetEditError(11, 'sheet editor handle'); return this.#ptr; }

    /** One command: `op` and its arguments (strings; numbers are written as text). Returns `{ result, state }`; a refusal throws. */
    cmd(op, ...args) {
      // the arguments are tab-separated: a backslash and a tab inside one are escaped (`\\`, `\t`)
      const esc = (a) => (a === undefined || a === null ? '' : String(a).replace(/\\/g, '\\\\').replace(/\t/g, '\\t'));
      const line = [op, ...args.map(esc)].join('\t');
      const [p, n] = arg(enc.encode(line));
      const b = take(w.uo_sheet_edit_cmd(this.#live(), p, n));
      if (b.status !== 0) throw new SheetEditError(b.status, op);
      const j = JSON.parse(dec.decode(b.data));
      if (!j.ok) { const e = new SheetEditError(0, op, j.error); e.state = j.state; throw e; }
      return { result: j.result, state: j.state };
    }

    /** The state of the session (selection, sheets, undo/redo, dirty, panes, grid lines, the formula bar, the editor). */
    state() { return this.cmd('state').state; }

    /** The `.xlsx` bytes (the surgical save). */
    save() {
      const b = take(w.uo_sheet_edit_save(this.#live()));
      if (b.status !== 0) throw new SheetEditError(b.status, 'save', dec.decode(b.data));
      return b.data;
    }

    setAccent(css) { const [p, n] = arg(enc.encode(css)); w.uo_sheet_edit_set_accent(this.#live(), p, n); }

    #needs() {
      const b = take(w.uo_sheet_edit_needs(this.#live()));
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
      const b = take(w.uo_sheet_edit_font(this.#live(), id));
      if (b.status !== 0) throw new SheetEditError(b.status, 'font');
      return readFont(b.data);
    }

    #answer(m) {
      const { texts, boxes } = this.#needs();
      const parts = [];
      const fonts = new Map();
      const fontOf = (id) => { let f = fonts.get(id); if (!f) { f = this.font(id); fonts.set(id, f); } return f; };
      for (const t of texts) {
        const f = fontOf(t.font);
        const cps = [...t.text];
        if (cps.length === 1 && t.text.length === cps[0].length) {
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
      const st = w.uo_sheet_edit_provide(this.#live(), p, n);
      if (st !== 0) throw new SheetEditError(st, 'provide');
    }

    /** One frame of view `q.sheet` (the session's sheet index): a message of the Slides list stream. */
    viewport(q, m) {
      const s = q.selection ?? [1, 1, 1, 1, 1, 1];
      for (let i = 0; i < MAX_ROUNDS; i++) {
        const b = take(w.uo_sheet_edit_viewport(this.#live(), q.sheet, q.width, q.height, q.dpr ?? 1, q.scrollX ?? 0, q.scrollY ?? 0, q.zoom ?? 1, s[0], s[1], s[2], s[3], s[4], s[5], q.capture ? 1 : 0, q.editing?.[0] ?? 0, q.editing?.[1] ?? 0));
        if (b.status === 0) return b.data;
        if (b.status !== 12) throw new SheetEditError(b.status, 'viewport');
        this.#answer(m);
      }
      throw new SheetEditError(12, 'viewport', 'does not settle');
    }

    /** `{ width, height }` of view `sheet` at zoom 1 (px). */
    extent(sheet, m) {
      for (let i = 0; i < MAX_ROUNDS; i++) {
        const b = take(w.uo_sheet_edit_extent(this.#live(), sheet));
        if (b.status === 0) { const d = dv(b.data); return { width: d.getFloat64(0, true), height: d.getFloat64(8, true) }; }
        if (b.status !== 12 || !m) return null;
        this.#answer(m);
      }
      return null;
    }

    /** The RGBA bytes of a bitmap handle of the stream. */
    bitmap(handle) { const b = take(w.uo_sheet_edit_bitmap(this.#live(), handle)); return b.status === 0 ? b.data : null; }
  }

  return {
    /** Opens `bytes` (empty = the blank workbook); `opts`: `locale` (the `formula-locale`), `clock`, `nowSerial`, `seed`. */
    open(bytes, opts = {}) {
      const c = opts.clock ?? { year: 2025, month: 1, day: 15, seconds: 43200 };
      const [p, n] = arg(bytes ?? new Uint8Array());
      const [lp, ln] = arg(enc.encode(opts.locale ?? 'pt-BR'));
      const ptr = w.uo_sheet_edit_open(p, n, lp, ln, c.year, c.month, c.day, c.seconds, opts.nowSerial ?? 45672.5, (opts.seed ?? 1) >>> 0);
      if (ptr === 0) { const e = take(w.uo_sheet_edit_error()); throw new SheetEditError(e.status, 'open', dec.decode(e.data)); }
      return new SheetEditor(ptr);
    },
    liveBytes: () => (w.uo_sheet_edit_live_bytes ? Number(w.uo_sheet_edit_live_bytes()) : 0),
  };
}
