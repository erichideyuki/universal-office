// The JS side of the session exports of uo-wasm (src/session, feature `session`; record
// core/docs/modules/uo-bindings.md "Milestone 2: the session in wasm"): one `WriteSession` per open
// document (explicit free() and a FinalizationRegistry), the hot paths, exec / query / complete with the
// generated codec (./gen/session.ts, from core/schema/session.uos), the session's layouts (the viewer's
// `WriteLayout` of write.mjs), the save and the font save in two phases.
//
// ES module for Node and bundlers: it imports the generated TypeScript codec (erasable TypeScript, which
// Node runs as is and every bundler compiles); a page without a bundler serves a compiled copy.
//
//   const { imports, host } = writeImports();
//   const { instance } = await WebAssembly.instantiate(bytes, imports);
//   const uo = bindWrite(instance.exports, host);
//   const ss = bindSession(instance.exports, uo);
//   const s = ss.open(docx, { author: 'Ana', env });
//   s.type('Olá');                                  // { t: 'done', v: ChangeSet } | { t: 'jobs', v } | { t: 'error', v }
//   const layout = s.layout(env);                   // a WriteLayout (encodePage, pdf, ...)
//   const caret = s.caretRect(layout);              // { page, x, y, h, xf? } (twips) | null
//   s.command({ t: 'format', v: { t: 'toggleRunProp', key: 'bold', button: 'single' } });
//   const bytes = s.save();
//   layout.free(); s.free();
//
// The generated TS form: a variant whose payload is a struct carries the struct's fields next to its tag
// (`{ t: 'done', stories, selection, events, ... }`), any other payload is `v` (`{ t: 'command', v }`).
// Results are the generated objects: `ExecResult.outcome` (`done` with the change set, `jobs` for a
// two-phase command — answer each with `complete(job, result)` —, `error` with the command error);
// events are `change.events`, each `{ t: 'uw-change' | 'uw-dirty-change' | ... , ...detail }`, in the
// order the shell re-dispatches them. Decode, bind and null-handle failures throw `SessionError`.
//
// Paste: `ClipboardCmd.pasteData.html` and `JobResult.clipboard.htmlTree` carry the bytes of
// `encodePaste()` of core/crates/uo-docx/js/paste-tree.mjs (the shell's DOMParser + CSSOM tree).

import * as codec from './gen/session.ts';

const enc = new TextEncoder();
const dec = new TextDecoder();
const STATUS = { 1: 'none', 2: 'no pending font save', 3: 'the save failed', 5: 'invalid message or argument', 11: 'null handle' };

/** A failure of a session export: `code` (the UoBuf status, or 'trap'), `detail` (UTF-8 from the module). */
export class SessionError extends Error {
  constructor(code, detail, what) {
    super(`${what}: ${STATUS[code] ?? code}${detail ? ` (${detail})` : ''}`);
    this.code = code;
    this.detail = detail;
  }
}

const DIRS = ['left', 'right', 'up', 'down', 'home', 'end', 'wordLeft', 'wordRight'];

/** `{ local, utc }` date parts of a Date (the session's clock). */
export function localNow(d) {
  return {
    local: { year: d.getFullYear(), month: d.getMonth() + 1, day: d.getDate(), hour: d.getHours(), minute: d.getMinutes() },
    utc: { year: d.getUTCFullYear(), month: d.getUTCMonth() + 1, day: d.getUTCDate(), hour: d.getUTCHours(), minute: d.getUTCMinutes() },
  };
}

export function bindSession(w, uo) {
  const { Buf, put } = uo.render;
  const registry = new FinalizationRegistry((ptr) => w.uo_session_free(ptr));
  let dead = false;

  /** every call into the module: a trap (a panic, an allocation failure) kills the instance; never unwind through it */
  const call = (what, f) => {
    if (dead) throw new SessionError('trap', 'the module trapped before', what);
    try {
      return f();
    } catch (e) {
      if (e instanceof WebAssembly.RuntimeError) {
        dead = true;
        throw new SessionError('trap', e.message, what);
      }
      throw e;
    }
  };
  /** a consumed input buffer: `[ptr, len]` */
  const arg = (bytes) => (bytes.length === 0 ? [0, 0] : [put(bytes), bytes.length]);
  const text = (s) => arg(enc.encode(s ?? ''));
  const message = (encodeFn, msg) => encodeFn(msg, codec.newEncodeState(0));
  /** `{ status, data }` of a Buf */
  const take = (ptr) => new Buf(ptr).take();
  const answer = (ptr, decodeFn, what) => {
    const { status, data } = take(ptr);
    if (status === 1) return null;
    if (status !== 0) throw new SessionError(status, dec.decode(data), what);
    return decodeFn(data, codec.newTables());
  };
  const outcome = (ptr, what) => answer(ptr, codec.decodeExecResult, what)?.outcome ?? null;

  const openOptions = (o = {}) => {
    const m = { protected: o.protected ?? false, seed: o.seed ?? 1 };
    if (o.author !== undefined) m.author = o.author;
    if (o.tracking !== undefined) m.tracking = o.tracking;
    if (o.markup !== undefined) m.markup = o.markup;
    if (o.lang !== undefined) m.lang = o.lang;
    if (o.clock !== undefined) m.clock = o.clock instanceof Date ? localNow(o.clock) : o.clock;
    if (o.maxHistorySteps !== undefined) m.maxHistorySteps = o.maxHistorySteps;
    if (o.sessionId !== undefined) m.sessionId = o.sessionId;
    return message(codec.encodeSessionOpen, m);
  };

  class WriteSession {
    #ptr;
    constructor(ptr) {
      this.#ptr = ptr;
      registry.register(this, ptr, this);
    }
    get ptr() { return this.#ptr; }
    #live() { if (!this.#ptr) throw new SessionError(11, 'session freed', 'session'); return this.#ptr; }
    free() {
      if (!this.#ptr) return;
      registry.unregister(this);
      if (!dead) w.uo_session_free(this.#ptr);
      this.#ptr = 0;
    }
    /** the font environment (a WriteEnv of write.mjs, or null) of the layout family and the layouts */
    setEnv(env) { call('setEnv', () => w.uo_session_set_env(this.#live(), env ? env.ptr : 0)); }

    // ── hot paths: each answers an ExecOutcome ──
    type(s) {
      const u = new Uint8Array(s.length * 2);
      const d = new DataView(u.buffer);
      for (let i = 0; i < s.length; i++) d.setUint16(i * 2, s.charCodeAt(i), true);
      return call('type', () => outcome(w.uo_session_type(this.#live(), ...arg(u)), 'type'));
    }
    enter() { return call('enter', () => outcome(w.uo_session_enter(this.#live()), 'enter')); }
    backspace() { return call('backspace', () => outcome(w.uo_session_backspace(this.#live()), 'backspace')); }
    delete() { return call('delete', () => outcome(w.uo_session_delete(this.#live()), 'delete')); }
    setSelection(anchor, focus = anchor) {
      return call('setSelection', () => outcome(w.uo_session_set_selection(this.#live(), anchor.para, anchor.offset, focus.para, focus.offset), 'setSelection'));
    }
    /** `dir`: left | right | up | down | home | end | wordLeft | wordRight; `layout` (a WriteLayout) for up/down/home/end */
    moveCaret(dir, extend = false, layout = null) {
      const d = DIRS.indexOf(dir);
      return call('moveCaret', () => outcome(w.uo_session_move_caret(this.#live(), layout ? layout.ptr : 0, d < 0 ? 99 : d, extend ? 1 : 0), 'moveCaret'));
    }
    /** `{ para, offset }` at page twips `(x, y)` of `page`, or null */
    hitTest(layout, page, x, y) { return call('hitTest', () => answer(w.uo_session_hit_test(this.#live(), layout.ptr, page, x, y), codec.decodeHit, 'hitTest')); }
    /** `{ page, x, y, h, xf? }` of the focus, or null */
    caretRect(layout) { return call('caretRect', () => answer(w.uo_session_caret_rect(this.#live(), layout.ptr), codec.decodeCaret, 'caretRect')); }
    /** `[{ x, y, w, h, xf? }]` of the selection on `page` */
    selectionRects(layout, page) {
      return call('selectionRects', () => answer(w.uo_session_selection_rects(this.#live(), layout.ptr, page), codec.decodeSelRects, 'selectionRects')?.rects ?? []);
    }

    // ── everything else ──
    /** an ExecOp (`{ t: 'command', v }`, `{ t: 'undo' }`, ...) */
    exec(op) { return call('exec', () => outcome(w.uo_session_exec(this.#live(), ...arg(message(codec.encodeExec, { op }))), 'exec')); }
    /** a Command (`{ t: 'format', v: { t: 'toggleRunProp', ... } }`) */
    command(cmd) { return this.exec({ t: 'command', v: cmd }); }
    undo() { return this.exec({ t: 'undo' }); }
    redo() { return this.exec({ t: 'redo' }); }
    cancelPending() { return this.exec({ t: 'cancelPending' }); }
    setDirty(dirty) { return this.exec({ t: 'setDirty', v: dirty }); }
    applyPainter() { return this.exec({ t: 'applyPainter' }); }
    /** `{ embed?, subset?, skipCommonFonts? }` */
    setFontEmbedding(patch) { return this.exec({ t: 'setFontEmbedding', ...patch }); }
    setClock(date) { return this.exec({ t: 'setClock', ...(date instanceof Date ? localNow(date) : date) }); }
    /** the interface language of the style names and galleries: 'en' | 'pt' | 'es' */
    setUiLang(lang) { return this.exec({ t: 'setUiLang', v: lang }); }
    /** the answer to job `job` of the pending command (a JobResult) */
    complete(job, result) {
      return call('complete', () => outcome(w.uo_session_complete(this.#live(), ...arg(message(codec.encodeComplete, { job, result }))), 'complete'));
    }
    /** a QueryOp (`{ t: 'headings' }`, `{ t: 'bookmarks', hidden, byLocation }`): the QueryAnswer */
    query(q) { return call('query', () => answer(w.uo_session_query(this.#live(), ...arg(message(codec.encodeQuery, { q }))), codec.decodeQueryResult, 'query').r); }

    // ── layouts ──
    /** the layout of the session's view (a WriteLayout: free it); `o` as `WriteDocument.layout` (no cancelCheck) */
    layout(env, o = {}) {
      const now = o.now instanceof Date ? o.now.getTime() : (o.now ?? Date.now());
      const tz = o.tzOffsetMin ?? -new Date(now).getTimezoneOffset();
      let flags = ['on', 'off', 'force'].indexOf(o.hyphenation ?? 'on');
      flags |= [undefined, 'final', 'simple', 'all', 'original'].indexOf(o.revisions) << 4;
      if (o.hyphenationLanguages) flags |= 1 << 8;
      const p = call('layout', () => w.uo_session_layout(this.#live(), env.ptr, now, tz, flags, ...text(o.lang), ...text((o.hyphenationLanguages ?? []).join('\n')), o.maxPages ?? 0));
      if (!p) throw uo.lastError('layout');
      return uo.wrapLayout(p);
    }

    // ── save ──
    save() {
      const { status, data } = call('save', () => take(w.uo_session_save(this.#live())));
      if (status !== 0) throw uo.lastError('save');
      return data;
    }
    /** phase 1: `{ settings, requests: [{ family, variant }] }` */
    fontRequests() {
      const { status, data } = call('fontRequests', () => take(w.uo_session_font_requests(this.#live())));
      if (status !== 0) throw uo.lastError('fontRequests');
      return codec.decodeFontRequests(data, codec.newTables());
    }
    /** phase 2: `faces[i]` the bytes of request i (or undefined), 16 random bytes per request: `{ bytes, events }` */
    saveWithFonts(faces, guids = crypto.getRandomValues(new Uint8Array(16 * faces.length))) {
      const m = message(codec.encodeFontFaces, { faces: faces.map((b) => (b ? { bytes: b } : {})), guids });
      return call('saveWithFonts', () => answer(w.uo_session_save_with_fonts(this.#live(), ...arg(m)), codec.decodeSaveResult, 'saveWithFonts'));
    }
    clearCaches() { call('clearCaches', () => w.uo_session_clear_caches(this.#live())); }
  }

  return {
    WriteSession,
    SessionError,
    codec,
    /** opens docx bytes; `o`: `{ protected, author, tracking, markup, lang, clock, seed, maxHistorySteps, sessionId, env }` */
    open(bytes, o = {}) {
      const opts = openOptions(o);
      const p = call('open', () => w.uo_session_open(o.env ? o.env.ptr : 0, ...arg(opts), ...arg(bytes)));
      if (!p) throw uo.lastError('open');
      return new WriteSession(p);
    },
    /** a new document (`newDocument()`) */
    newBlank(o = {}) {
      const opts = openOptions(o);
      const p = call('newBlank', () => w.uo_session_new_blank(o.env ? o.env.ptr : 0, ...arg(opts)));
      if (!p) throw uo.lastError('newBlank');
      return new WriteSession(p);
    },
    /** empties the Office Clipboard of the instance */
    clipboardClear() { call('clipboardClear', () => w.uo_session_clipboard_clear()); },
    get dead() { return dead; },
  };
}
