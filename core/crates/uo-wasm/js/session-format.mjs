// Wave 7, frontier A1 (record core/docs/modules/uo-bindings.md "Wave 7 — A1: File, View, Font and Paragraph"): the JS side of
// the exports of uo-wasm src/session/shell_format.rs — the JSON reads of the Paragraph and Font dialogs, the native-only
// commands (`insertCustomInline`, `ensureBuiltinStyle`, the plain `replaceAll`), the ODT/HTML/Markdown exports, the images
// the shell compresses and the `.odt` open. Re-exported by session.mjs; the editor shell (`write/packages/editor/src/
// rust-format-*.ts`) is its only user.
//
//   const fm = bindFormat(instance.exports, uo);          // uo = bindWrite(...)
//   fm.json(session, 1);                                  // getParagraphFlow()
//   fm.exec(session, 3, ['old', 'new']);                  // replaceAll: an ExecOutcome
//
// Statuses: 1 is null, other non-zero statuses and a trap throw `SessionError`.

import * as codec from './gen/session.ts';
import { SessionError, localNow } from './session.mjs';

const dec = new TextDecoder();
const enc = new TextEncoder();

/** `u32 count`, then per string `u32 units` + the UTF-16LE units (shell_format.rs `strings`) */
function packStrings(list) {
  let n = 4;
  for (const s of list) n += 4 + s.length * 2;
  const out = new Uint8Array(n);
  const d = new DataView(out.buffer);
  d.setUint32(0, list.length, true);
  let at = 4;
  for (const s of list) {
    d.setUint32(at, s.length, true);
    at += 4;
    for (let i = 0; i < s.length; i++) { d.setUint16(at, s.charCodeAt(i), true); at += 2; }
  }
  return out;
}

export function bindFormat(w, uo) {
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
  const arg = (bytes) => (bytes.length === 0 ? [0, 0] : [put(bytes), bytes.length]);
  const strs = (list) => arg(list.length ? packStrings(list) : new Uint8Array(0));
  const strsAlways = (list) => arg(packStrings(list));
  const utf16 = (data) => {
    const d = new DataView(data.buffer, data.byteOffset, data.byteLength);
    let s = '';
    const CH = 8192;
    for (let i = 0; i + 1 < data.length; i += CH * 2) {
      const units = [];
      for (let k = i; k < Math.min(data.length - 1, i + CH * 2); k += 2) units.push(d.getUint16(k, true));
      s += String.fromCharCode(...units);
    }
    return s;
  };
  return {
    /** a JSON read (`what` as `uo_shell_fmt_json`), `args` strings; null for status 1 */
    json(session, what, args = []) {
      const { status, data } = raw('fmtJson', () => w.uo_shell_fmt_json(session.ptr, what >>> 0, ...strs(args.map(String))));
      if (status === 1) return null;
      if (status !== 0) throw new SessionError(status, dec.decode(data), 'fmtJson');
      return JSON.parse(dec.decode(data));
    },
    /** a native-only command: an ExecOutcome */
    exec(session, what, args = []) {
      const { status, data } = raw('fmtExec', () => w.uo_shell_fmt_exec(session.ptr, what >>> 0, ...strsAlways(args.map(String))));
      if (status === 1) return null;
      if (status !== 0) throw new SessionError(status, dec.decode(data), 'fmtExec');
      return codec.decodeExecResult(data, codec.newTables()).outcome;
    },
    /** the environment's hyphenation patterns for the Manual Hyphenation (null env: none) */
    hyphenation(session, env) {
      raw('fmtHyphenation', () => { w.uo_shell_fmt_hyphenation(session.ptr, env ? env.ptr : 0); return 0; });
    },
    /** `exportOdt({ images })`: the .odt bytes */
    exportOdt(session, omitImages = false) {
      const { status, data } = raw('exportOdt', () => w.uo_shell_fmt_export(session.ptr, 0, ...strsAlways([omitImages ? 'omit' : 'embed'])));
      if (status !== 0) throw new SessionError(status, dec.decode(data), 'exportOdt');
      return data;
    },
    /** `exportHtml`/`exportMarkdown`: `uris` [drawing id, data URI][] */
    exportText(session, markdown, title, omit, uris = []) {
      const list = [title === undefined || title === null ? '\u0001' : String(title), omit ? 'omit' : 'data'];
      for (const [id, u] of uris) list.push(String(id), u);
      const { status, data } = raw('exportText', () => w.uo_shell_fmt_export(session.ptr, markdown ? 2 : 1, ...strsAlways(list)));
      if (status !== 0) throw new SessionError(status, dec.decode(data), 'exportText');
      return utf16(data);
    },
    /** the images the HTML/Markdown export resolves: `[{ id, ext, data }]` (`collectImages()`) */
    images(session) {
      const { status, data } = raw('images', () => w.uo_shell_fmt_export(session.ptr, 3, 0, 0));
      if (status !== 0) throw new SessionError(status, dec.decode(data), 'images');
      const d = new DataView(data.buffer, data.byteOffset, data.byteLength);
      const n = d.getUint32(0, true);
      const out = [];
      let at = 4;
      for (let i = 0; i < n; i++) {
        const id = d.getUint32(at, true); at += 4;
        const el = d.getUint32(at, true); at += 4;
        const ext = dec.decode(data.subarray(at, at + el)); at += el;
        const bl = d.getUint32(at, true); at += 4;
        out.push({ id, ext, data: data.slice(at, at + bl) });
        at += bl;
      }
      return out;
    },
    /**
     * `loadOdt(bytes)`: the session handle of an .odt (a `WriteSession` of `ss`, the binding of `bindSession`), `o` as
     * `ss.open`'s options. Throws the module's last error when the document does not open.
     */
    openOdt(ss, bytes, o = {}) {
      const m = { protected: o.protected ?? false, seed: o.seed ?? 1 };
      if (o.author !== undefined) m.author = o.author;
      if (o.lang !== undefined) m.lang = o.lang;
      if (o.clock !== undefined) m.clock = o.clock instanceof Date ? localNow(o.clock) : o.clock;
      const opts = codec.encodeSessionOpen(m, codec.newEncodeState(0));
      let p;
      try {
        p = w.uo_shell_open_odt(o.env ? o.env.ptr : 0, ...arg(opts), ...arg(bytes));
      } catch (e) {
        if (e instanceof WebAssembly.RuntimeError) throw new SessionError('trap', e.message, 'openOdt');
        throw e;
      }
      if (!p) throw uo.lastError('openOdt');
      return new ss.WriteSession(p);
    },
    /** the engine line of the focus for the ruler: `{ page, x, y, frame, pfmt }` or null */
    ruler(session, layout) {
      const { status, data } = raw('ruler', () => w.uo_shell_fmt_ruler(session.ptr, layout.ptr));
      if (status === 1) return null;
      if (status !== 0) throw new SessionError(status, dec.decode(data), 'ruler');
      return JSON.parse(dec.decode(data));
    },
    /** the view the shell engine lays out: 0 print, 1 web (`widthTw`), 2 draft, 3 outline */
    setView(engine, kind, widthTw = 0) {
      raw('setView', () => { w.uo_shell_engine_set_view(engine.ptr, kind >>> 0, widthTw); return 0; });
    },
    /** wave 7c (V): the editor's continuous view (1 Web, 2 Draft, 3 Outline, 0 Print): one sheet, the TS margin, Word's
     *  content (`tsStrict`: the frozen TS's rules where Word differs) */
    setContinuous(engine, kind, widthTw = 0, marginTw = 600, tsStrict = false) {
      raw('setContinuous', () => { w.uo_shell_engine_set_continuous(engine.ptr, kind >>> 0, widthTw, marginTw, tsStrict ? 1 : 0); return 0; });
    },
    /** the layout of the TOC gallery preview `id` (0 auto1, 1 auto2, 2 manual): a WriteLayout (free it) */
    tocTemplate(session, env, id, now = Date.now()) {
      let p;
      try {
        p = w.uo_shell_fmt_toc_template(session.ptr, env.ptr, id >>> 0, now instanceof Date ? now.getTime() : now, -new Date(now).getTimezoneOffset());
      } catch (e) {
        if (e instanceof WebAssembly.RuntimeError) throw new SessionError('trap', e.message, 'tocTemplate');
        throw e;
      }
      if (!p) throw uo.lastError('tocTemplate');
      return uo.wrapLayout(p);
    },
    /** the crop of the TOC preview `id`: `{ cropLeft, marginRight }` or null */
    tocCrop(session, id) {
      const { status, data } = raw('tocCrop', () => w.uo_shell_fmt_toc_crop(session.ptr, id >>> 0));
      if (status === 1) return null;
      if (status !== 0) throw new SessionError(status, dec.decode(data), 'tocCrop');
      return JSON.parse(dec.decode(data));
    },
    /**
     * File > Compare: `diffDocuments(a, b, opts)` (the compare package's) in Rust (uo-docx compare) over two `.docx`:
     * the `DocDiff` object (`{ edits, stats }`). `opts` as `DocDiffOptions` (`algorithm`, `intraWord`). Throws
     * `SessionError` (status 5) when a document does not read.
     */
    compare(a, b, opts = {}) {
      const flags = (opts.algorithm === 'myers' ? 1 : 0) | (opts.intraWord === false ? 2 : 0);
      const { status, data } = raw('compare', () => w.uo_shell_fmt_compare(...arg(a), ...arg(b), flags));
      if (status !== 0) throw new SessionError(status, dec.decode(data), 'compare');
      return JSON.parse(dec.decode(data));
    },
    /** File > Save as XPS (fix round 2, D-A1-2): the XPS package of a layout's pages, written in Rust (uo-pdf `xps`) */
    xps(layout) {
      const { status, data } = raw('xps', () => w.uo_shell_fmt_xps(layout.ptr));
      if (status !== 0) throw new SessionError(status, dec.decode(data), 'xps');
      return data.slice();
    },
    /** `TextEncoder` helper for the shell (UTF-8 lengths of the image ext) */
    utf8: (s) => enc.encode(s),
  };
}
