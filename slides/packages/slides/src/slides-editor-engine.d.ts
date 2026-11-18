// The surface of the Rust editor engine's JS side (core/crates/uo-wasm/js/slides-editor-engine.ts) that the editor's
// `rust-editor.ts` uses, declared here so the editor's own `tsc` build (rootDir `src`) does not compile the core. The real module
// is reached through the alias `@universal-office/slides-editor-engine` (tools/build-pkg.mjs `ENGINE`, the editor harness).
// `core/crates/uo-wasm/js/slides-editor-engine.check.ts` assigns the real module to this declaration: drift fails the core's tsc.
declare module '@universal-office/slides-editor-engine' {
  type Bytes = Uint8Array | ArrayBuffer | string | URL | Response | Promise<Uint8Array | ArrayBuffer | Response>;
  export interface LoadSlidesOptions { wasm?: Bytes; ppt?: Bytes }

  export class SlidesEditError extends Error {
    readonly status: number;
    readonly detail: string;
  }

  export interface Frame { x: number; y: number; cx: number; cy: number }
  export interface RunFormat { bold?: boolean; italic?: boolean; underline?: boolean; strike?: boolean; size?: number; font?: string; color?: string }
  export interface TransitionSpec { type: string; dir?: string; speed?: string; advanceOnClick?: boolean; advanceAfterMs?: number }
  export interface ShowSettingsSpec { loop?: boolean; useTimings?: boolean; showAnimation?: boolean; kiosk?: boolean }
  export interface DocProperties {
    title?: string; subject?: string; author?: string; keywords?: string; description?: string; lastModifiedBy?: string; created?: string; modified?: string;
  }
  export interface ClipRel { id: string; type: string; target: string; external?: boolean; bytes?: Uint8Array; contentType?: string }
  export interface ShapeClip { xml: string; rels: ClipRel[]; text: string; sourceId: number; slidePath: string }
  export type Target = { t: 'slide'; v: number } | { t: 'all' };
  export interface BlankDeckOptions { wide?: boolean; title?: string; lang: string; now: string }

  export type Op =
    | { t: 'addSlide'; layout?: string }
    | { t: 'duplicateSlide'; i: number }
    | { t: 'deleteSlide'; i: number }
    | { t: 'moveSlide'; from: number; to: number }
    | { t: 'setSlideHidden'; i: number; hidden?: boolean }
    | { t: 'setSlideLayout'; i: number; layout: string }
    | { t: 'setSlideBackground'; target: Target; color?: string }
    | { t: 'setSlideTransition'; target: Target; spec?: TransitionSpec }
    | ({ t: 'setShowSettings' } & ShowSettingsSpec)
    | { t: 'setShapeText'; shape: string; text: string }
    | { t: 'typing'; slide: number; shape: string; text: string }
    | { t: 'formatText'; shape: string; f: RunFormat }
    | { t: 'growFont'; shape: string; dir: number; base: number }
    | { t: 'alignText'; shape: string; algn: string }
    | { t: 'setBullets'; shape: string; kind: 'bullet' | 'number' | 'none' }
    | { t: 'clearFormatting'; shape: string }
    | { t: 'setLink'; shape: string; url?: string }
    | { t: 'insertTextBox'; text: string; frame?: Frame }
    | { t: 'insertShape'; prst: string; frame?: Frame }
    | { t: 'insertImage'; bytes: Uint8Array; mime: string; frame?: Frame; sizePx?: { f0: number; f1: number } }
    | { t: 'setShapeFrame'; shape: string; frame: Frame }
    | { t: 'setShapeFill'; shape: string; color?: string }
    | { t: 'setShapeOutline'; shape: string; color?: string }
    | { t: 'arrangeShape'; shape: string; mode: 'front' | 'back' | 'forward' | 'backward' }
    | { t: 'deleteShape'; shape: string }
    | { t: 'pasteShape'; clip: ShapeClip }
    | { t: 'setSlideNotes'; i: number; text: string }
    | ({ t: 'setProperties' } & DocProperties)
    | { t: 'replaceText'; query: string; replacement: string; matchCase?: boolean; hit?: { f0: number; f1: string } };

  export type Outcome =
    | { t: 'nothing' } | { t: 'bool'; v: boolean } | { t: 'index'; v: number } | { t: 'shape'; value?: string } | { t: 'count'; v: number };
  export type ApplyOutcome = { t: 'ok'; v: Outcome } | { t: 'err'; v: string };
  export interface UiState {
    index: number; selected?: string; canUndo?: boolean; canRedo?: boolean; undoLabel?: string; redoLabel?: string;
    dirty?: boolean; slides: number; embed?: boolean; subset?: boolean;
  }
  export interface PartChange { name: string; bytes?: Uint8Array }
  export interface EditResult { outcome: ApplyOutcome; state: UiState; changes: PartChange[] }
  export type QueryOp = { t: 'findText'; query: string; matchCase?: boolean } | { t: 'defaultNewLayout' } | { t: 'fontEmbedding' };
  export type QueryAnswer =
    | { t: 'hits'; v: { f0: number; f1: string }[] } | { t: 'layout'; value?: string } | { t: 'embedding'; embed?: boolean; subset?: boolean };
  export interface FontRequests { embed?: boolean; subset?: boolean; requests: { family: string; variant: 'regular' | 'bold' | 'italic' | 'boldItalic' }[] }
  export interface SaveResult { bytes: Uint8Array; embedded: string[]; skipped: { family: string; reason: 'common' | 'license' | 'unavailable' | 'alreadyEmbedded' }[] }

  /** One presentation open for editing: `doc` is the viewer document over the same wasm deck (opaque here). */
  export class SlidesEditSession {
    readonly doc: object;
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
    decodeClip(bytes: Uint8Array): ShapeClip;
    query(q: QueryOp): QueryAnswer;
    fontRequests(): FontRequests;
    saveWithFonts(faces: (Uint8Array | undefined)[]): SaveResult;
    save(): Uint8Array;
    free(): void;
  }

  export function openEditSession(bytes: Uint8Array, source: LoadSlidesOptions): Promise<SlidesEditSession>;
  export function blankPresentation(opts: BlankDeckOptions, source: LoadSlidesOptions): Promise<Uint8Array>;
  export function editSessions(): number;
}
