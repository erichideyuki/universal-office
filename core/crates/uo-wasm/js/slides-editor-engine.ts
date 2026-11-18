// The engine side of the Slides EDITOR's TS/Rust switch (`<universal-slides engine="rust">`, B8 routing; record
// core/docs/modules/uo-bindings.md "Slides editor (B8 routing)"). One wasm module per page, the editor build
// `uo-slides-editor.wasm` (the viewer's exports + `slides-edit`), loaded through the viewer's loader (`slides-engine.ts`), so
// a viewer and an editor of the same page share the instance. One `SlidesEditSession` per open document: the viewer's
// `SlidesDocument` (emission and painting of the slides, stills and stage) and the `SlidesEditor` over THE SAME wasm deck.
// No DOM element logic here: that is `slides/packages/slides/src/rust-editor.ts`, which reaches this module through the alias
// `@universal-office/slides-editor-engine` (typed there by `slides/packages/slides/src/slides-editor-engine.d.ts`;
// `slides-editor-engine.check.ts` fails tsc when the two drift).
//
// A call that traps the module (`WebAssembly.RuntimeError`) marks the session dead (every call throws, the element falls
// back to the TS engine), as the viewer does for its documents.
//
// Erasable TypeScript only.

import { loadSlidesModule, type LoadSlidesOptions, type SlidesDocument } from './slides-engine.ts';
import { bindSlidesEditor, hasEditorExports, SlidesEditError } from './slides-editor.mjs';
import type { EditResult, FontRequests, Op, QueryAnswer, QueryOp, SaveResult, ShapeClip, SlidesEditor, SlidesEditorBinding } from './slides-editor.mjs';
import type { BlankDeckOptions } from './gen/slides-edit.ts';

export { SlidesEditError };
export type { EditResult, FontRequests, Op, QueryAnswer, QueryOp, SaveResult, ShapeClip, BlankDeckOptions };

const STOPPED = 'uo-wasm: the Slides editor instance stopped after a trap; open the presentation again';
const NO_EDITOR = 'uo-wasm: the Slides module of this page has no editor exports (a viewer loaded uo-slides.wasm first: configure uo-slides-editor.wasm)';

const bindings = new WeakMap<WebAssembly.Exports, SlidesEditorBinding>();
let liveSessions = 0;

async function binding(opts: LoadSlidesOptions): Promise<{ b: SlidesEditorBinding; open(bytes: Uint8Array): Promise<SlidesDocument> }> {
  const mod = await loadSlidesModule(opts);
  const ex = mod.exports;
  if (!hasEditorExports(ex)) throw new SlidesEditError(11, NO_EDITOR, 'load');
  let b = bindings.get(ex);
  if (!b) {
    b = bindSlidesEditor(ex, null);
    bindings.set(ex, b);
  }
  return { b, open: (bytes) => mod.open(bytes) };
}

/** The bytes of a blank presentation (`newPresentation`): `{ wide, title?, lang, now }` (`now` = ISO date without ms). */
export async function blankPresentation(opts: BlankDeckOptions, source: LoadSlidesOptions): Promise<Uint8Array> {
  const { b } = await binding(source);
  return b.blank(opts);
}

/** Opens a presentation for editing: the viewer document and the editor over its deck. */
export async function openEditSession(bytes: Uint8Array, source: LoadSlidesOptions): Promise<SlidesEditSession> {
  const { b, open } = await binding(source);
  const doc = await open(bytes);
  let ed: SlidesEditor;
  try {
    ed = b.newEditor(doc.deckPtr);
  } catch (e) {
    doc.free();
    throw e;
  }
  return new SlidesEditSession(doc, ed, b);
}

/** Live editor sessions of the page (the M4 tests: 0 after everything was released). */
export function editSessions(): number { return liveSessions; }

/** One presentation open for editing (see the module comment). */
export class SlidesEditSession {
  readonly doc: SlidesDocument;
  #ed: SlidesEditor;
  #b: SlidesEditorBinding;
  #dead = false;
  #freed = false;

  constructor(doc: SlidesDocument, ed: SlidesEditor, b: SlidesEditorBinding) {
    this.doc = doc;
    this.#ed = ed;
    this.#b = b;
    liveSessions++;
  }

  get freed(): boolean { return this.#freed || this.#dead || this.doc.freed; }

  #call<T>(f: (ed: SlidesEditor) => T): T {
    if (this.#freed) throw new Error('uo-wasm: the Slides edit session was freed');
    if (this.#dead || this.doc.freed) throw new Error(STOPPED);
    try {
      return f(this.#ed);
    } catch (e) {
      if (e instanceof WebAssembly.RuntimeError) this.#dead = true;
      throw e;
    }
  }

  /** An editing answer: the document re-reads its slide list when the package changed. */
  #edit(f: (ed: SlidesEditor) => EditResult): EditResult {
    const r = this.#call(f);
    if (r.changes.length > 0) this.doc.refresh();
    return r;
  }

  apply(op: Op): EditResult { return this.#edit((ed) => ed.apply(op)); }
  undo(): EditResult { return this.#edit((ed) => ed.undo()); }
  redo(): EditResult { return this.#edit((ed) => ed.redo()); }
  setUi(index: number, selected: string | null): EditResult { return this.#edit((ed) => ed.setUi(index, selected)); }
  state(): EditResult { return this.#edit((ed) => ed.state()); }
  markClean(): EditResult { return this.#edit((ed) => ed.markClean()); }
  setFontEmbedding(s: { embed?: boolean; subset?: boolean }): EditResult { return this.#edit((ed) => ed.setFontEmbedding(s)); }
  setRandomSeed(seed: number): void { this.#call((ed) => ed.setRandomSeed(seed)); }
  setTextLang(lang: string): void { this.#call((ed) => ed.setTextLang(lang)); }
  /** the clip of a shape of the current slide (bytes that paste in any editor session of the page), or null */
  copyShape(id: string): Uint8Array | null { return this.#call((ed) => ed.copyShape(id)); }
  /** the shape clip inside clip bytes (its `text` is the plain text of the shape) */
  decodeClip(bytes: Uint8Array): ShapeClip { return this.#b.decodeClip(bytes); }
  query(q: QueryOp): QueryAnswer { return this.#call((ed) => ed.query(q)); }
  fontRequests(): FontRequests { return this.#call((ed) => ed.fontRequests()); }
  saveWithFonts(faces: (Uint8Array | undefined)[]): SaveResult { return this.#call((ed) => ed.saveWithFonts(faces)); }
  save(): Uint8Array { return this.#call((ed) => ed.save()); }

  /** Frees the editor, then the document (its deck). */
  free(): void {
    if (this.#freed) return;
    this.#freed = true;
    liveSessions--;
    try { if (!this.#dead) this.#ed.free(); } catch { /* the instance stopped: nothing to free */ }
    try { this.doc.free(); } catch { /* idem */ }
  }
}
