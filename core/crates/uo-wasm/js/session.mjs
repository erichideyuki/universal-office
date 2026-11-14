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
import { Reader } from '../../uo-codec/js/runtime.ts';

// The encoder of that tree, for the editor's shell (`<universal-write engine="rust">`, keyboard paste; record
// uo-bindings.md "Milestone 2: the editor switch", ED-16): additive, pure DOM at call time.
export { encodePaste } from '../../uo-docx/js/paste-tree.mjs';

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

// docStart/docEnd (wave 6, C): Ctrl+Home/End, the selection assigned as the TS (no float selection or pending props cleared)
const DIRS = ['left', 'right', 'up', 'down', 'home', 'end', 'wordLeft', 'wordRight', 'docStart', 'docEnd'];

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
    /** `dir`: left | right | up | down | home | end | wordLeft | wordRight | docStart | docEnd; `layout` (a WriteLayout) for up/down/home/end */
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
      const p = call('layout', () => w.uo_session_layout(this.#live(), env.ptr, ...layoutArgs(o)));
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

    // ── the editor shell (src/session/shell.rs; record "Milestone 2: the editor switch") ──
    /** `{ paras, lastLength }` of the edit story */
    storyInfo() {
      const { status, data } = call('storyInfo', () => take(w.uo_shell_story_info(this.#live())));
      if (status !== 0) throw new SessionError(status, '', 'storyInfo');
      const d = new DataView(data.buffer, data.byteOffset, data.byteLength);
      return { paras: d.getUint32(0, true), lastLength: d.getUint32(4, true) };
    }
    /** the text of paragraph `i` of the edit story, or null */
    paraText(i) {
      const { status, data } = call('paraText', () => take(w.uo_shell_para_text(this.#live(), i >>> 0)));
      if (status === 1) return null;
      if (status !== 0) throw new SessionError(status, '', 'paraText');
      return utf16(data, 0, data.length);
    }
    /** the toolbar state at the selection: `{ run, para, pending }` (`getSelectionState()` without view, marks, table) */
    selectionState() {
      const { status, data } = call('selectionState', () => take(w.uo_shell_selection_state(this.#live())));
      if (status !== 0) throw new SessionError(status, dec.decode(data), 'selectionState');
      const n = new DataView(data.buffer, data.byteOffset, data.byteLength).getUint32(0, true);
      const run = codec.readRunProps(new Reader(data.subarray(4, 4 + n)), codec.newTables());
      const para = codec.readParaProps(new Reader(data.subarray(4 + n, data.length - 1)), codec.newTables());
      return { run, para, pending: (data[data.length - 1] & 1) === 1 };
    }
    /** the hyphenation pattern ids the document needs and `env` lacks (the viewer's `hyphenationPatterns`) */
    hyphenationPatterns(env, langs = [], o = {}) {
      // (`o.tsHyphenation`: the frozen TS's English `en-us`, parity tests only; else Word's `en-us-word`, WordCraft study D1)
      const flags = o.tsHyphenation ? 1 << 16 : 0;
      const { status, data } = call('hyphenationPatterns', () => take(w.uo_shell_hyphenation_patterns(this.#live(), env.ptr, ...text(langs.join('\n')), flags)));
      if (status !== 0) throw new SessionError(status, '', 'hyphenationPatterns');
      const s = dec.decode(data);
      return s ? s.split('\n') : [];
    }
    /** `{ text, html }` of the last copy or cut, or null */
    copied() {
      const { status, data } = call('copied', () => take(w.uo_shell_copied(this.#live())));
      if (status === 1) return null;
      if (status !== 0) throw new SessionError(status, '', 'copied');
      const n = new DataView(data.buffer, data.byteOffset, data.byteLength).getUint32(0, true);
      return { text: utf16(data, 4, 4 + n * 2), html: dec.decode(data.subarray(4 + n * 2)) };
    }

    // ── wave 5, frontier R (src/session/shell_review.rs; record uo-bindings.md "Wave 5 — R") ──
    /** the lines of the edit scope on `page` of `layout`: `[{ para, page, x, y, height, baseline, width, start, end, xf?, o, cx, cw }]` */
    pageLines(layout, page) {
      return call('pageLines', () => answer(w.uo_shell_page_lines(this.#live(), layout.ptr, page >>> 0), codec.decodeShellLines, 'pageLines')?.lines ?? []);
    }
    /** the lines of paragraph `para` of the edit scope in `layout` (same shape) */
    paraLines(layout, para) {
      return call('paraLines', () => answer(w.uo_shell_para_lines(this.#live(), layout.ptr, para >>> 0), codec.decodeShellLines, 'paraLines')?.lines ?? []);
    }
    /** `[{ x, y, w, h, xf? }]` of the range `a`–`f` (either direction) on `page` */
    rangeRects(layout, page, a, f) {
      return call('rangeRects', () => answer(w.uo_shell_range_rects(this.#live(), layout.ptr, page >>> 0, a.para, a.offset, f.para, f.offset), codec.decodeSelRects, 'rangeRects')?.rects ?? []);
    }
    /** wave 7c (V, ED-304): the lines of EVERY story on `page` (the TS `eng.lines`: body, cells, headers/footers, notes;
     *  then the text boxes' content in their frame) with their paragraphs: `{ lines: [{ p, kind, x, y, height, baseline,
     *  width, start, end, xf?, o, cx, cw }], paras: [{ key, text, runs: [{ n, f, c, sp, lang }], bm: [{ at, end, id,
     *  hidden }] }] }` (`fallback`: the spelling language of a run without one) */
    storyLines(layout, page, fallback = 'pt-BR') {
      const { status, data } = call('storyLines', () => take(w.uo_shell_story_lines(this.#live(), layout.ptr, page >>> 0, ...text(fallback))));
      if (status === 1) return { lines: [], paras: [] };
      if (status !== 0) throw new SessionError(status, '', 'storyLines');
      return JSON.parse(dec.decode(data));
    }
    /** the families of the document's embedded fonts */
    embeddedFamilies() {
      const { status, data } = call('embeddedFamilies', () => take(w.uo_shell_embedded_families(this.#live())));
      if (status !== 0) throw new SessionError(status, '', 'embeddedFamilies');
      const s = dec.decode(data);
      return s ? s.split('\n') : [];
    }
  }

  /** UTF-16LE code units of `data[from, to)` as a string */
  const utf16 = (data, from, to) => {
    const d = new DataView(data.buffer, data.byteOffset, data.byteLength);
    let s = '';
    for (let i = from; i + 1 < to; i += 2) s += String.fromCharCode(d.getUint16(i, true));
    return s;
  };
  /** the layout arguments of `WriteSession.layout` and `ShellEngine.relayout` (now, tz, flags, lang, hyphenation languages, max pages) */
  const layoutArgs = (o) => {
    const now = o.now instanceof Date ? o.now.getTime() : (o.now ?? Date.now());
    const tz = o.tzOffsetMin ?? -new Date(now).getTimezoneOffset();
    let flags = ['on', 'off', 'force'].indexOf(o.hyphenation ?? 'on');
    flags |= [undefined, 'final', 'simple', 'all', 'original'].indexOf(o.revisions) << 4;
    if (o.hyphenationLanguages) flags |= 1 << 8;
    return [now, tz, flags, ...text(o.lang), ...text((o.hyphenationLanguages ?? []).join('\n')), o.maxPages ?? 0];
  };
  const REASONS = ['noBase', 'disabled', 'view', 'options', 'provider', 'document', 'context', 'chains', 'shape', 'prologue',
    'noCheckpoint', 'passInput', 'unstable', 'tables', 'balancing', 'memory', 'failed', 'lent'];
  const engines = new FinalizationRegistry((ptr) => { if (!dead) w.uo_shell_engine_free(ptr); });

  /** The incremental layout engine of one edited document (`relayout` lays out only what an edit changed, LA7). */
  class ShellEngine {
    #ptr;
    constructor(ptr) {
      this.#ptr = ptr;
      engines.register(this, ptr, this);
    }
    get ptr() { return this.#ptr; }
    #live() { if (!this.#ptr) throw new SessionError(11, 'engine freed', 'engine'); return this.#ptr; }
    /** the layout of `session`'s view (a WriteLayout: free it); `o` as `WriteSession.layout` */
    relayout(session, env, o = {}) {
      // wave 6 (L): with `o.prev` (the previous relayout's WriteLayout, or null) the engine LENDS its result to the new
      // layout instead of copying it, and `prev` gives the previous result back first: `prev` is then STALE (`prev.stale`:
      // no pages, no lines) until it is freed
      if ('prev' in o) {
        const prev = o.prev ?? null;
        const p = call('relayout', () => w.uo_shell_relayout2(this.#live(), prev ? prev.ptr : 0, session.ptr, env.ptr, ...layoutArgs(o)));
        if (prev && w.uo_layout_page_count(prev.ptr) === 0) prev.stale = true;
        if (!p) throw uo.lastError('relayout');
        const l = uo.wrapLayout(p);
        // which faces and images of the new layout are `prev`'s of the same index (the viewer keeps their keys and bitmaps)
        const { status, data } = take(w.uo_layout_shared_resources(p));
        if (status === 0) {
          const d = new DataView(data.buffer, data.byteOffset, data.byteLength);
          const nf = d.getUint32(0, true), ni = d.getUint32(4 + nf, true);
          l.shared = { faces: Array.from(data.subarray(4, 4 + nf), (x) => x === 1), images: Array.from(data.subarray(8 + nf, 8 + nf + ni), (x) => x === 1) };
        }
        return l;
      }
      const p = call('relayout', () => w.uo_shell_relayout(this.#live(), session.ptr, env.ptr, ...layoutArgs(o)));
      if (!p) throw uo.lastError('relayout');
      return uo.wrapLayout(p);
    }
    /** what the last relayout did, or null before the first */
    report() {
      const { status, data } = call('report', () => take(w.uo_shell_relayout_report(this.#live())));
      if (status === 1) return null;
      if (status !== 0) throw new SessionError(status, '', 'report');
      const d = new DataView(data.buffer, data.byteOffset, data.byteLength);
      return {
        path: d.getUint8(0) === 1 ? 'incremental' : 'full',
        reason: d.getUint8(1) === 255 ? null : (REASONS[d.getUint8(1)] ?? d.getUint8(1)),
        relaid: [d.getUint32(2, true), d.getUint32(6, true)],
        passes: d.getUint32(10, true),
        lineShift: d.getInt32(14, true),
        memoHits: d.getUint32(18, true),
        memoMisses: d.getUint32(22, true),
        // wave 6 (L): the relaid pages whose page changed (the shell repaints only those)
        changed: data.byteLength >= 30 ? Array.from({ length: d.getUint32(26, true) }, (_, i) => d.getUint32(30 + 4 * i, true)) : null,
      };
    }
    /** drops the incremental state (the next relayout is a full one) */
    clear() { call('clear', () => w.uo_shell_engine_clear(this.#live())); }
    free() {
      if (!this.#ptr) return;
      engines.unregister(this);
      if (!dead) w.uo_shell_engine_free(this.#ptr);
      this.#ptr = 0;
    }
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
    /** a new incremental layout engine for one edited document (the editor shell) */
    newShellEngine() { return new ShellEngine(call('newShellEngine', () => w.uo_shell_engine_new())); },
    ShellEngine,
    get dead() { return dead; },
  };
}

// ── wave 5, front O (objects and the page; src/session/shell_objects.rs; record uo-bindings.md "Wave 5 — O") ──
/**
 * The object geometry and the section facts of a session for the editor shell: `objects(session, layout,
 * activePage)` (the TS `enumFloats` + `enumInlinePics`: `{ scope, fs, sel, floats, inline }`) and
 * `sectionInfo(session, hfSection)` (`{ index, count, touched, pageBorderSection, firstSect, suppressed }`, the
 * model objects as their `JSON.stringify`). Both answer JSON text; a trap rethrows as `SessionError('trap')`.
 */
export function bindObjects(w, uo) {
  const { Buf } = uo.render;
  const json = (what, f) => {
    let ptr;
    try {
      ptr = f();
    } catch (e) {
      if (e instanceof WebAssembly.RuntimeError) throw new SessionError('trap', e.message, what);
      throw e;
    }
    const { status, data } = new Buf(ptr).take();
    if (status !== 0) throw new SessionError(status, '', what);
    return JSON.parse(dec.decode(data));
  };
  return {
    objects(session, layout, activePage = -1, inTextbox = false) {
      return json('objects', () => w.uo_shell_objects(session.ptr, layout.ptr, activePage | 0, inTextbox ? 1 : 0));
    },
    sectionInfo(session, hfSection = -1) {
      return json('sectionInfo', () => w.uo_shell_section_info(session.ptr, hfSection | 0));
    },
  };
}

// ── wave 5, frontier S (stories: tables, headers/footers, notes, text boxes; src/session/shell_stories.rs) ──
export { bindStories } from './session-stories.mjs';

// ── wave 7 (A2): lists, paste, tables, objects and the page (src/session/shell_completion.rs) ──
export { bindCompletion } from './session-completion.mjs';

// ── wave 7 (A1): File, View, Font and Paragraph (src/session/shell_format.rs) ──
export { bindFormat } from './session-format.mjs';
