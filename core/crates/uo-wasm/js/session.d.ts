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
  QueryOp,
  SaveResult,
  SelRect,
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
}

export interface SessionBinding {
  WriteSession: typeof WriteSession;
  SessionError: typeof SessionError;
  codec: typeof Codec;
  open(docx: Uint8Array, o?: SessionOpenOptions): WriteSession;
  newBlank(o?: SessionOpenOptions): WriteSession;
  clipboardClear(): void;
  readonly dead: boolean;
}

export declare function bindSession(exports: WebAssembly.Exports, uo: WriteBinding): SessionBinding;
