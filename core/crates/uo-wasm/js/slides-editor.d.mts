// Types of js/slides-editor.mjs (the JS side of the Slides editor exports of uo-wasm, src/slides_edit.rs).

import type {
  ApplyOutcome, BlankDeckOptions, EditResult, FontRequests, Op, QueryAnswer, QueryOp, SaveResult, ShapeClip,
} from './gen/slides-edit.ts';
import type { SlidesBinding } from './slides.mjs';

export type { ApplyOutcome, BlankDeckOptions, EditResult, FontRequests, Op, QueryAnswer, QueryOp, SaveResult, ShapeClip };

export declare class SlidesEditError extends Error {
  readonly status: number;
  readonly detail: string;
  constructor(status: number, detail: string, what: string);
}

export declare function hasEditorExports(w: WebAssembly.Exports): boolean;

export declare class SlidesEditor {
  readonly freed: boolean;
  apply(op: Op): EditResult;
  undo(): EditResult;
  redo(): EditResult;
  setUi(index: number, selected: string | null): EditResult;
  state(): EditResult;
  markClean(): EditResult;
  setFontEmbedding(s: { embed?: boolean; subset?: boolean }): EditResult;
  setRandomSeed(seed: number): void;
  setTextLang(lang: string): void;
  copyShape(id: string): Uint8Array | null;
  query(q: QueryOp): QueryAnswer;
  fontRequests(): FontRequests;
  saveWithFonts(faces: (Uint8Array | undefined)[]): SaveResult;
  save(): Uint8Array;
  free(): void;
}

export interface SlidesEditorBinding {
  newEditor(deckPtr: number): SlidesEditor;
  blank(opts: BlankDeckOptions): Uint8Array;
  decodeClip(bytes: Uint8Array): ShapeClip;
  uo: SlidesBinding;
  SlidesEditor: typeof SlidesEditor;
}

export declare function bindSlidesEditor(w: WebAssembly.Exports, uo: SlidesBinding | null): SlidesEditorBinding;
