// Types of js/session.mjs (the session exports of uo-wasm, feature `session`; record
// core/docs/modules/uo-bindings.md "Milestone 2: the session in wasm"). The message types are the
// generated ones of ./gen/session.ts (core/schema/session.uos).

import type {
  Caret,
  Command,
  EmbeddingPatch,
  ExecOp,
  ExecOutcome,
  FontRequests,
  Hit,
  JobResult,
  LocalNow,
  MarkupMode,
  QueryAnswer,
  ParaProps,
  QueryOp,
  RunProps,
  SaveResult,
  SelRect,
  ShellLine,
} from './gen/session.ts';
import type * as Codec from './gen/session.ts';
import type { WriteBinding, WriteEnv, WriteLayout, LayoutOptions } from './write.js';

/** A failure of a session export: the UoBuf status (1 none, 2 no pending font save, 3 save failed, 5 invalid
 *  message or argument, 11 null handle) or `'trap'` (the module trapped: the instance is dead). */
export declare class SessionError extends Error {
  readonly code: number | 'trap';
  readonly detail: string;
}

/** `{ local, utc }` date parts of a Date (the session's clock). */
export declare function localNow(d: Date): LocalNow;

export type CaretDir = 'left' | 'right' | 'up' | 'down' | 'home' | 'end' | 'wordLeft' | 'wordRight';

export interface SessionOpenOptions {
  /** Modo Protegido (default false here; the TS `loadDocx` default is true) */
  protected?: boolean;
  author?: string;
  tracking?: boolean;
  markup?: MarkupMode;
  /** the language of a new document */
  lang?: string;
  clock?: Date | LocalNow;
  /** the seed of the `Math.random` sites (default 1) */
  seed?: number;
  maxHistorySteps?: number;
  sessionId?: number;
  /** the font environment of the layout family and the layouts */
  env?: WriteEnv;
}

export declare class WriteSession {
  readonly ptr: number;
  free(): void;
  setEnv(env: WriteEnv | null): void;
  // hot paths (null: nothing happened)
  type(text: string): ExecOutcome | null;
  enter(): ExecOutcome | null;
  backspace(): ExecOutcome | null;
  delete(): ExecOutcome | null;
  setSelection(anchor: { para: number; offset: number }, focus?: { para: number; offset: number }): ExecOutcome | null;
  moveCaret(dir: CaretDir, extend?: boolean, layout?: WriteLayout | null): ExecOutcome | null;
  hitTest(layout: WriteLayout, page: number, x: number, y: number): Hit | null;
  caretRect(layout: WriteLayout): Caret | null;
  selectionRects(layout: WriteLayout, page: number): SelRect[];
  // everything else
  exec(op: ExecOp): ExecOutcome | null;
  command(cmd: Command): ExecOutcome | null;
  undo(): ExecOutcome | null;
  redo(): ExecOutcome | null;
  cancelPending(): ExecOutcome | null;
  setDirty(dirty: boolean): ExecOutcome | null;
  applyPainter(): ExecOutcome | null;
  setFontEmbedding(patch: EmbeddingPatch): ExecOutcome | null;
  setClock(date: Date | LocalNow): ExecOutcome | null;
  setUiLang(lang: 'en' | 'pt' | 'es'): ExecOutcome | null;
  /** the answer to a job of the pending command. Paste: `{ t: 'clipboard', htmlTree }` carries the bytes of
   *  `encodePaste()` of core/crates/uo-docx/js/paste-tree.mjs (as `ClipboardCmd` `pasteData.html`). */
  complete(job: number, result: JobResult): ExecOutcome | null;
  query(q: QueryOp): QueryAnswer;
  // layouts
  layout(env: WriteEnv, o?: Omit<LayoutOptions, 'cancelCheck' | 'view' | 'webWidthTw' | 'generation'>): WriteLayout;
  // save
  save(): Uint8Array;
  fontRequests(): FontRequests;
  saveWithFonts(faces: (Uint8Array | undefined)[], guids?: Uint8Array): SaveResult | null;
  clearCaches(): void;
  // the editor shell (src/session/shell.rs; record "Milestone 2: the editor switch")
  /** the edit story: paragraph count and the UTF-16 length of the last paragraph */
  storyInfo(): { paras: number; lastLength: number };
  /** the text of paragraph `i` of the edit story, or null */
  paraText(i: number): string | null;
  /** the toolbar state at the selection (`getSelectionState()` without view, marks and table) */
  selectionState(): SelectionState;
  /** the hyphenation pattern ids the document needs and `env` does not have yet */
  hyphenationPatterns(env: WriteEnv, langs?: string[]): string[];
  /** the text and HTML of the last copy or cut, or null */
  copied(): { text: string; html: string } | null;
  // wave 5, frontier R (src/session/shell_review.rs; record uo-bindings.md "Wave 5 — R")
  /** the lines of the edit scope on `page` (every paragraph, in order) */
  pageLines(layout: WriteLayout, page: number): ShellLine[];
  /** the lines of paragraph `para` of the edit scope */
  paraLines(layout: WriteLayout, para: number): ShellLine[];
  /** the selection rectangles of a range (either direction) on `page` */
  rangeRects(layout: WriteLayout, page: number, a: { para: number; offset: number }, f: { para: number; offset: number }): SelRect[];
  /** the families of the document's embedded fonts */
  embeddedFamilies(): string[];
}

export interface SelectionState {
  run: RunProps;
  para: ParaProps;
  /** the run props are the pending ones (typed text gets them) */
  pending: boolean;
}

export type RelayoutReason =
  | 'noBase' | 'disabled' | 'view' | 'options' | 'provider' | 'document' | 'context' | 'chains' | 'shape' | 'prologue'
  | 'noCheckpoint' | 'passInput' | 'unstable' | 'tables' | 'balancing' | 'memory' | 'failed';

export interface RelayoutReport {
  path: 'full' | 'incremental';
  /** why the path is full */
  reason: RelayoutReason | null;
  /** the pages laid out again: [start, end) */
  relaid: [number, number];
  passes: number;
  lineShift: number;
  memoHits: number;
  memoMisses: number;
}

/** The incremental layout engine of one edited document (LA7): free it. */
export declare class ShellEngine {
  readonly ptr: number;
  /** the layout of `session`'s view, laid out again only where the edits changed it (a WriteLayout: free it) */
  relayout(session: WriteSession, env: WriteEnv, o?: Omit<LayoutOptions, 'cancelCheck' | 'view' | 'webWidthTw' | 'generation'>): WriteLayout;
  report(): RelayoutReport | null;
  clear(): void;
  free(): void;
}

export interface SessionBinding {
  WriteSession: typeof WriteSession;
  SessionError: typeof SessionError;
  codec: typeof Codec;
  open(docx: Uint8Array, o?: SessionOpenOptions): WriteSession;
  newBlank(o?: SessionOpenOptions): WriteSession;
  clipboardClear(): void;
  newShellEngine(): ShellEngine;
  ShellEngine: typeof ShellEngine;
  readonly dead: boolean;
}

export declare function bindSession(exports: WebAssembly.Exports, uo: WriteBinding): SessionBinding;

/** The payload of a `text/html` clipboard item (`ClipboardCmd` `pasteData.html`), re-exported from
 *  core/crates/uo-docx/js/paste-tree.mjs; `fragmentOf` is the editor's paste-html.ts one. Browser only (DOMParser). */
export declare function encodePaste(html: string, fragmentOf: (html: string) => string): { bytes: Uint8Array; nodes: number; depth: number };

// ── wave 5, front O (objects and the page; src/session/shell_objects.rs) ──
/** One floating object of `ObjectsBinding.objects` (the TS `enumFloats`, engine branch). */
export interface ShellFloat {
  /** the `SelectObject { index }` index (`contextDrawingRuns`) */
  i: number;
  kind: 'image' | 'textbox' | 'shape' | 'group';
  /** the drawing id */
  id: number;
  page: number;
  x: number;
  y: number;
  w: number;
  h: number;
  /** degrees, clockwise (`runRot`) */
  rot: number;
  behind: boolean;
  /** `rawOf(r) && formatOf(r)` */
  editable: boolean;
  /** a text box with laid-out content */
  content: boolean;
  /** the selection pane key (paneRuns), if listed */
  key: string | null;
  /** a picture: its layout image index (imageBytes) */
  image: number | null;
  frame: { pageW: number; pageH: number; marL: number; marR: number; marT: number; marB: number; colX: number; colW: number; bodyTop: number; paraTop: number } | null;
}
/** One inline picture of `ObjectsBinding.objects` (the TS `enumInlinePics`). */
export interface ShellInlinePic {
  para: number;
  run: number;
  id: number;
  page: number;
  x: number;
  y: number;
  w: number;
  h: number;
  rot: number;
  /** the layout's image index (`WriteLayout.imageBytes`) */
  image: number;
}
export interface ShellObjects {
  scope: 'body' | 'hf' | 'tb';
  /** the drawing id of the text box in edit (its dashed outline and handles) */
  box: number | null;
  /** `floatSelection()` */
  fs: number | null;
  /** the selected drawing ids (the selected object first) */
  sel: number[];
  floats: ShellFloat[];
  inline: ShellInlinePic[];
}
export interface ShellSectionInfo {
  index: number;
  count: number;
  /** `JSON.stringify` of `sectionSectPr(m, k)` for the sections the selection touches */
  touched: Record<string, unknown>[];
  pageBorderSection: Record<string, unknown>;
  firstSect: Record<string, unknown> | null;
  suppressed: boolean;
  titlePg: boolean[];
  background: Record<string, unknown> | null;
  /** the body's final sectPr (the TS `sect()`) */
  lastSect: Record<string, unknown> | null;
  bgShown: boolean;
  evenOdd: boolean;
}
export interface ObjectsBinding {
  objects(session: WriteSession, layout: WriteLayout, activePage?: number, inTextbox?: boolean): ShellObjects;
  sectionInfo(session: WriteSession, hfSection?: number): ShellSectionInfo;
}
export declare function bindObjects(exports: WebAssembly.Exports, uo: WriteBinding): ObjectsBinding;

// ── wave 5, frontier S (stories; js/session-stories.d.ts) ──
export * from './session-stories.js';
