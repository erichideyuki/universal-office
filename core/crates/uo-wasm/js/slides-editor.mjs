// The JS side of the Slides EDITOR exports of uo-wasm (src/slides_edit.rs, feature `slides-edit`; record
// core/docs/modules/uo-bindings.md "Slides editor (B8 routing)"): one `SlidesEditor` per open document, over the deck
// handle of the viewer binding (slides.mjs) in the SAME instance, with explicit free() and a FinalizationRegistry, and
// the generated codec (./gen/slides-edit.ts, from core/schema/slides_edit.uos).
//
//   const uo = bindSlides(exports);                         // the viewer binding (decks, views, emission)
//   const ue = bindSlidesEditor(exports, uo);
//   const deck = uo.open(pptx);
//   const ed = ue.newEditor(deck.ptr);
//   let r = ed.apply({ t: 'insertTextBox', text: 'Hi' });    // { outcome, state, changes }
//   r = ed.undo();                                          // outcome { t: 'ok', v: { t: 'bool', v: true } }
//   const bytes = ed.save();
//   ed.free(); deck.free();
//
// Every editing call answers `{ outcome, state, changes }`: `outcome` `{ t: 'ok', v: Outcome }` or `{ t: 'err', v:
// message }` (the package is unchanged), `state` the UI state the shell mirrors, `changes` the parts changed since the
// previous answer (`{ name, bytes }`, `bytes` absent = removed): the shell applies them to its own copy of the package.
//
// ES module for Node and bundlers (erasable TypeScript codec, as session.mjs).

import * as codec from './gen/slides-edit.ts';
import { bindRender } from './render.mjs';

const enc = new TextEncoder();
const dec = new TextDecoder();
// the messages carry whole files (the saved package, inserted pictures, changed media parts): the limits of src/slides_edit/codec
const LIMITS = { maxDepth: 32, maxListLen: 2 ** 30, maxTotal: 2 ** 30 };
const STATUS = { 1: 'none', 3: 'the package could not be written', 5: 'invalid message or argument', 7: 'the command failed', 11: 'null handle or another deck' };

/** A failure of an editor export: `status` (the UoBuf status) and `detail` (UTF-8 from the module). */
export class SlidesEditError extends Error {
  constructor(status, detail, what) {
    super(`${what}: ${STATUS[status] ?? status}${detail ? ` (${detail})` : ''}`);
    this.status = status;
    this.detail = detail;
  }
}

/** The editor exports of an instance (false: a module built without `slides-edit`, e.g. the viewer's `uo-slides.wasm`). */
export function hasEditorExports(w) {
  return typeof w.uo_slides_editor_new === 'function';
}

export function bindSlidesEditor(w, uo) {
  const r = bindRender(w);
  const { Buf, put } = r;
  const registry = new FinalizationRegistry((ptr) => w.uo_slides_editor_free(ptr));
  const arg = (bytes) => (bytes.length === 0 ? [0, 0] : [put(bytes), bytes.length]);
  const text = (s) => arg(enc.encode(s ?? ''));
  const message = (encodeFn, msg) => encodeFn(msg, codec.newEncodeState(0));
  const take = (ptr) => new Buf(ptr).take();
  const answer = (ptr, decodeFn, what) => {
    const { status, data } = take(ptr);
    if (status === 1) return null;
    if (status !== 0) throw new SlidesEditError(status, dec.decode(data), what);
    return decodeFn(data, codec.newTables(), LIMITS);
  };
  const bytesOf = (ptr, what) => {
    const { status, data } = take(ptr);
    if (status !== 0) throw new SlidesEditError(status, dec.decode(data), what);
    return data;
  };

  class SlidesEditor {
    #ptr;
    #deck;
    constructor(ptr, deck) {
      if (!ptr) throw new SlidesEditError(11, '', 'editor_new');
      this.#ptr = ptr;
      this.#deck = deck;
      registry.register(this, ptr, this);
    }
    #live() {
      if (this.#ptr === 0) throw new SlidesEditError(11, 'the editor was freed', 'editor');
      return this.#ptr;
    }
    get freed() { return this.#ptr === 0; }
    #result(ptr, what) { return answer(ptr, codec.decodeEditResult, what); }
    /** one editing command (`Op` of the codec) */
    apply(op) { return this.#result(w.uo_slides_editor_apply(this.#live(), this.#deck, ...arg(message(codec.encodeApply, { op }))), 'apply'); }
    undo() { return this.#result(w.uo_slides_editor_history(this.#live(), this.#deck, 0), 'undo'); }
    redo() { return this.#result(w.uo_slides_editor_history(this.#live(), this.#deck, 1), 'redo'); }
    /** the shell's current slide and selected shape id (null = none) before a command */
    setUi(index, selected) {
      const has = selected !== null && selected !== undefined;
      return this.#result(w.uo_slides_editor_set_ui(this.#live(), this.#deck, index >>> 0, has ? 1 : 0, ...text(has ? selected : '')), 'set_ui');
    }
    state() { return this.#result(w.uo_slides_editor_state(this.#live(), this.#deck), 'state'); }
    markClean() { return this.#result(w.uo_slides_editor_mark_clean(this.#live(), this.#deck), 'mark_clean'); }
    /** `{ embed?, subset? }` (absent keeps the value) */
    setFontEmbedding(s) {
      const v = (b) => (b === undefined ? -1 : b ? 1 : 0);
      return this.#result(w.uo_slides_editor_set_font_embedding(this.#live(), this.#deck, v(s.embed), v(s.subset)), 'set_font_embedding');
    }
    setRandomSeed(seed) { w.uo_slides_editor_set_random_seed(this.#live(), seed >>> 0); }
    setTextLang(lang) { w.uo_slides_editor_set_text_lang(this.#live(), ...text(lang)); }
    /** the clip bytes of a shape of the current slide (a `Clip` message), or null when it cannot be copied */
    copyShape(id) {
      const { status, data } = take(w.uo_slides_editor_copy_shape(this.#live(), this.#deck, ...text(id)));
      if (status === 1) return null;
      if (status !== 0) throw new SlidesEditError(status, dec.decode(data), 'copy_shape');
      return data;
    }
    /** a query (`QueryOp` of the codec): the `QueryAnswer` */
    query(q) { return answer(w.uo_slides_editor_query(this.#live(), this.#deck, ...arg(message(codec.encodeQuery, { q }))), codec.decodeQueryResult, 'query').r; }
    /** phase 1 of the save with fonts: `{ embed, subset, requests: [{ family, variant }] }` */
    fontRequests() { return answer(w.uo_slides_editor_font_requests(this.#live(), this.#deck), codec.decodeFontRequests, 'font_requests'); }
    /** phase 2: one `Uint8Array | undefined` per request, in order: `{ bytes, embedded, skipped }` */
    saveWithFonts(faces) {
      const msg = message(codec.encodeFontFaces, { faces: faces.map((f) => (f ? { value: f } : {})) });
      return answer(w.uo_slides_editor_save_with_fonts(this.#live(), this.#deck, ...arg(msg)), codec.decodeSaveResult, 'save_with_fonts');
    }
    /** the `.pptx` bytes as they are */
    save() { return bytesOf(w.uo_slides_editor_save(this.#live(), this.#deck), 'save'); }
    free() {
      if (this.#ptr === 0) return;
      registry.unregister(this);
      w.uo_slides_editor_free(this.#ptr);
      this.#ptr = 0;
    }
  }

  return {
    /** an editor over the open deck handle `deckPtr` (`SlidesDeck.ptr`) */
    newEditor(deckPtr) { return new SlidesEditor(w.uo_slides_editor_new(deckPtr), deckPtr); },
    /** the bytes of a blank presentation: `{ wide, title?, lang, now }` */
    blank(opts) { return bytesOf(w.uo_slides_editor_blank(...arg(message(codec.encodeBlank, { opts }))), 'blank'); },
    /** the shape clip of a `copyShape` answer (its `text` is the plain text for the system clipboard) */
    decodeClip(bytes) { return codec.decodeClip(bytes, codec.newTables(), LIMITS).clip; },
    uo,
    SlidesEditor,
  };
}
