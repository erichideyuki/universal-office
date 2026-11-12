// Wave 5, frontier S (record core/docs/modules/uo-bindings.md "Wave 5 — S: stories"): the JS side of the stories exports of
// uo-wasm (src/session/shell_stories.rs): where a click lands (a table cell, a header/footer, a note, a text box), the
// table state and navigation, the header/footer facts of a page, the note run under a position. Re-exported by
// session.mjs; the editor shell (`write/packages/editor/src/rust-stories-*.ts`) is its only user.
//
//   const st = bindStories(instance.exports, uo);       // uo = bindWrite(...)
//   st.locateAt(session, layout, page, x, y);           // { kind: 'cell', anchor, hops, ... } | null
//   st.tableNavigate(session, layout, 'down', false);   // an ExecOutcome (the TS `true`) | null (the default move)
//
// JSON answers are parsed; `ExecResult` answers are the generated `ExecOutcome`; status 1 is null; other statuses and a
// trap throw `SessionError`.

import * as codec from './gen/session.ts';
import { SessionError } from './session.mjs';

const enc = new TextEncoder();
const dec = new TextDecoder();
const DIRS = { left: 0, right: 1, up: 2, down: 3 };

export function bindStories(w, uo) {
  const { Buf, put } = uo.render;
  const raw = (what, f) => {
    let ptr;
    try {
      ptr = f();
    } catch (e) {
      if (e instanceof WebAssembly.RuntimeError) throw new SessionError('trap', e.message, what);
      throw e;
    }
    return new Buf(ptr).take();
  };
  const json = (what, f) => {
    const { status, data } = raw(what, f);
    if (status === 1) return null;
    if (status !== 0) throw new SessionError(status, dec.decode(data), what);
    return JSON.parse(dec.decode(data));
  };
  const outcome = (what, f) => {
    const { status, data } = raw(what, f);
    if (status === 1) return null;
    if (status !== 0) throw new SessionError(status, dec.decode(data), what);
    return codec.decodeExecResult(data, codec.newTables()).outcome;
  };
  const quads = (what, f) => {
    const { status, data } = raw(what, f);
    if (status !== 0) throw new SessionError(status, '', what);
    const d = new DataView(data.buffer, data.byteOffset, data.byteLength);
    const out = [];
    for (let i = 0; i + 32 <= data.length; i += 32) out.push({ x: d.getFloat64(i, true), y: d.getFloat64(i + 8, true), w: d.getFloat64(i + 16, true), h: d.getFloat64(i + 24, true) });
    return out;
  };
  /** a consumed UTF-8 input buffer: `[ptr, len]` */
  const text = (s) => {
    const b = enc.encode(s ?? '');
    return b.length === 0 ? [0, 0] : [put(b), b.length];
  };
  const sepText = (sep) => (sep === undefined || sep === null ? '' : typeof sep === 'object' ? `char:${sep.char}` : String(sep));

  return {
    /** `{ ctx: { kind, ... }, tableSel, locked }` */
    storyContext(s) { return json('storyContext', () => w.uo_shell_story_context(s.ptr)); },
    /** the line under page twips `(x, y)` of `page` and where it is (`engLineAt` + `locateRef`), or null */
    locateAt(s, layout, page, x, y) { return json('locateAt', () => w.uo_shell_locate_at(s.ptr, layout.ptr, page >>> 0, x, y)); },
    /** the innermost table cell box under `(x, y)` (`cellBoxAtPoint`), or null */
    cellAt(s, layout, page, x, y) { return json('cellAt', () => w.uo_shell_cell_at(s.ptr, layout.ptr, page >>> 0, x, y)); },
    /** `getTableState()` (+ `anchor`, `path`, `layoutIn`), or null */
    tableState(s, layout) { return json('tableState', () => w.uo_shell_table_state(s.ptr, layout ? layout.ptr : 0)); },
    /** the boxes of the selection of cells on `page` */
    tableSelRects(s, layout, page) { return quads('tableSelRects', () => w.uo_shell_table_sel_rects(s.ptr, layout.ptr, page >>> 0)); },
    /** every placed cell box on `page` (gridlines) */
    cellBoxes(layout, page) { return quads('cellBoxes', () => w.uo_shell_cell_boxes(layout.ptr, page >>> 0)); },
    /** `tableNavigate(dir, extend)`: an ExecOutcome when the key is handled, null for the default move */
    tableNavigate(s, layout, dir, extend = false) {
      const d = DIRS[dir];
      if (d === undefined) return null;
      return outcome('tableNavigate', () => w.uo_shell_table_navigate(s.ptr, layout.ptr, d, extend ? 1 : 0));
    },
    /** `moveCell(dir)` (Tab, Shift+Tab): an ExecOutcome, or null when nothing moves */
    moveCell(s, layout, dir) { return outcome('moveCell', () => w.uo_shell_move_cell(s.ptr, layout.ptr, dir < 0 ? -1 : 1)); },
    /** the drag of a selection of cells from the cell being edited (`dragCellSel`): an ExecOutcome when it changed, else null */
    dragCellSel(s, layout, page, x, y) { return outcome('dragCellSel', () => w.uo_shell_drag_cell_sel(s.ptr, layout.ptr, page >>> 0, x, y)); },
    /** the header and footer facts of `page`, or null */
    hfPage(s, layout, page) { return json('hfPage', () => w.uo_shell_hf_page(s.ptr, layout.ptr, page >>> 0)); },
    /** the note run at a position of the edited story (`noteRunAt`), or null */
    noteAt(s, para, offset) { return json('noteAt', () => w.uo_shell_note_at(s.ptr, para >>> 0, offset >>> 0)); },
    /** the text box under `(x, y)` (`hitTextbox`; `tol` twips of border), or null */
    textboxAt(s, layout, page, x, y, tol) { return json('textboxAt', () => w.uo_shell_textbox_at(s.ptr, layout.ptr, page >>> 0, x, y, tol)); },
    /** `sortInfo(separator)`, or null */
    sortInfo(s, separator) {
      const ch = separator === 'comma' ? ',' : separator && typeof separator === 'object' ? separator.char : '';
      return json('sortInfo', () => w.uo_shell_sort_info(s.ptr, ...text(ch)));
    },
    /** `textToTableGuess(separator, cols)`, or null */
    textToTableGuess(s, separator, cols) { return json('textToTableGuess', () => w.uo_shell_text_to_table_guess(s.ptr, ...text(sepText(separator)), cols ?? 0)); },
  };
}
