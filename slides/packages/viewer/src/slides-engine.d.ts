// The surface of the Rust engine's JS side (core/crates/uo-wasm/js/slides-engine.ts) that the viewer's `rust-engine.ts` uses,
// declared here so the viewer's own `tsc` build (rootDir `src`) does not compile the core. The real module is reached through
// the alias `@universal-office/slides-engine` (`build:demo`, tools/build-pkg.mjs `ENGINE`).
// `core/crates/uo-wasm/js/slides-engine.check.ts` assigns the real module to this declaration: drift fails the core's tsc line.
declare module '@universal-office/slides-engine' {
  type Bytes = Uint8Array | ArrayBuffer | string | URL | Response | Promise<Uint8Array | ArrayBuffer | Response>;

  /** The error of an export (`status`: 3 unreadable deck, 4 limit, 5 argument, 6 no such item, 14 a `.ppt` without its module). */
  export class SlidesError extends Error {
    readonly status: number;
    readonly detail: string;
  }

  export interface LoadSlidesOptions {
    wasm?: Bytes;
    ppt?: Bytes;
    /** every TS switch on the decks opened (SlidesRules::ts_strict): the TS-parity harnesses; default = PowerPoint's rules */
    tsStrict?: boolean;
    /** the language of date/time fields without one (default navigator.language) */
    fieldLang?: string;
    /** the clock of the date/time fields (default new Date() at open) */
    now?: () => Date;
  }

  /** A decoded picture the painter draws (an `HTMLImageElement`, `ImageBitmap`, canvas, video ...). */
  export type PaintImage = CanvasImageSource & { naturalWidth?: number; naturalHeight?: number; width?: unknown; height?: unknown };

  export interface LightTables {
    lightTable(rig: string | undefined, mat: string | undefined): Uint8Array | undefined;
    lightAt(t: Uint8Array, dir: string | undefined, thetaDeg: number, phiDeg: number, out: Float32Array): void;
    lightFace(t: Uint8Array, out: Float32Array): void;
    bevelPhi(prst: string | undefined, t: number, hw: number): number;
  }

  export interface SlidesPaintHost {
    fontStack(family: string, bold: boolean, italic: boolean): string;
    image(part: string): PaintImage | null | undefined;
    measureCtx: CanvasRenderingContext2D | OffscreenCanvasRenderingContext2D;
    extra?: { light?: LightTables; patternRows?: (prst: string) => readonly number[] | undefined | null };
    /** a picture the layout waited for (a compressed EMF+ bitmap) finished decoding: paint again (the paint returned PENDING) */
    prepared?(): void;
  }

  /** The status of a paint that waits for a picture to decode (`SlidesPaintHost.prepared` tells when to paint again). */
  export const PENDING: 13;

  /** Wave 4, the read model. */
  export interface OutlineCell { hMerge: boolean; vMerge: boolean; paragraphs: string[] }
  export interface OutlineShape {
    inherited: boolean; image: boolean; media: boolean; phType: string | undefined; descr: string | undefined;
    hasText: boolean; paragraphs: string[]; table: OutlineCell[][] | null;
  }
  export type LinkTargetWire = { url: string } | { slide: number } | { show: 'next' | 'previous' | 'first' | 'last' | 'lastViewed' | 'end' };
  export interface LinkBoxWire { shape: number; source: 'run' | 'shape' | 'jump'; x: number; y: number; w: number; h: number; target: LinkTargetWire }
  export interface MediaRefWire { kind: 'video' | 'audio'; part?: string; url?: string; mime: string; trim?: { startMs: number; endMs: number }; fade?: { inMs: number; outMs: number } }
  export interface MediaInfoWire { spid: string; kind: 'video' | 'audio'; vol?: number; mute?: boolean; showWhenStopped?: boolean; rewind?: boolean; loop?: boolean; fullScreen?: boolean }
  export interface MediaShapeWire { shape: number; inherited: boolean; spid: string; gifPart: string | null; media: MediaRefWire | null; info: MediaInfoWire | null }
  export interface SlideAssets { images: { part: string; mime: string; gif: boolean; special: boolean }[]; media: MediaShapeWire[] }
  export interface MediaStateWire { started: boolean; playing: boolean; timeSec: number; durMs?: number; ended?: boolean }
  export interface MediaStates { index: number; animMs: number; gifMs: number; states: Map<number, MediaStateWire> }
  export interface EmbeddedFontWire { family: string; variant: 'regular' | 'bold' | 'italic' | 'boldItalic'; data: Uint8Array; weight?: number; italic?: boolean }
  export interface ShowSettingsWire { loop?: boolean; useTimings?: boolean; showAnimation?: boolean; kiosk?: boolean; range?: [number, number] }

  export interface SlidePaintOptions { skipCharts?: boolean; suppressText?: number }
  export const CHARTS_LEFT: 15;

  export interface SlidePaintResult { status: number; charts: number; rounds: number; empty?: boolean; animMs?: number; replaced?: number }

  /** B4b: options of a playback frame (`paintStage`). */
  export interface StagePaintOptions {
    media?: boolean;
    clock?: { latencyMs?: number; scale?: number };
    mediaFrame?(shape: number): { image: PaintImage; alpha: number } | null | undefined;
  }

  export interface TransitionWire { type: string; ns: string; attrs: Record<string, string>; durMs: number; advTmMs?: number; advClick: boolean }
  export type ShowAction = 'none' | 'transition' | 'step' | 'slide' | 'end' | 'exit';
  export interface ShowFrame {
    index: number; step: number; steps: number; ended: boolean; animMs: number; transitionProgress: number; effects: number;
    from: { index: number; animMs: number; key: number; effects: number } | null;
    transition: TransitionWire | null;
  }

  export class SlidesShowHandle {
    next(now: number, mouse?: boolean): ShowAction;
    prev(now: number): ShowAction;
    goTo(i: number, now: number): ShowAction;
    first(now: number): ShowAction;
    last(now: number): ShowAction;
    update(now: number): ShowAction;
    dueAt(): number;
    wakeAt(now: number): number;
    frame(now: number): ShowFrame;
    free(): void;
  }

  export class SlidesTimelineHandle {
    info(): { durationMs: number; tickMs: number; mediaClock: number; slides: { index: number; startMs: number; endMs: number; animStartMs: number; transitionMs: number }[] };
    stateAt(t: number): { pos: number; index: number; from: number; transitionProgress: number; animMs: number; ended: boolean; effects: number; transition: TransitionWire | null };
    free(): void;
  }

  export class SlidesDocument {
    readonly count: number;
    readonly slideW: number;
    readonly slideH: number;
    readonly freed: boolean;
    readonly generation: number;
    /** the wasm deck pointer (the editor routing's own exports of the same instance take it) */
    readonly deckPtr: number;
    /** after an edit: the count and size read again, the stages laid out again */
    refresh(): void;
    notes(i: number): string;
    part(name: string): Uint8Array | null;
    fontsChanged(): void;
    imageReady(part: string, img: PaintImage): void;
    /** start decoding the compressed EMF+ bitmaps of the metafile picture in `part` (the prepare phase ahead of the paint) */
    prepareSpecial(part: string, host: SlidesPaintHost): void;
    paint(ctx: CanvasRenderingContext2D, index: number, scale: number, ox: number, oy: number, host: SlidesPaintHost, o?: SlidePaintOptions): SlidePaintResult;
    clearCaches(): void;
    newShow(o?: { from?: number; now?: number; useTimings?: number; loop?: number; animations?: number; range?: [number, number] | null }): SlidesShowHandle;
    newTimeline(o?: { useTimings?: number; defaultSlideMs?: number; tickMs?: number; transitionFps?: number }): SlidesTimelineHandle;
    paintStage(ctx: CanvasRenderingContext2D, src: SlidesShowHandle | SlidesTimelineHandle, at: number, which: 0 | 1, scale: number, ox: number, oy: number,
      host: SlidesPaintHost, o?: StagePaintOptions): SlidePaintResult;
    cacheSizes(): { derived: number; bitmaps: number };
    /** compressed EMF+ bitmaps still decoding (a paint that met them returned PENDING) */
    readonly preparing: number;
    /** compressed EMF+ bitmaps decoded so far: [decoded, failed] */
    readonly prepared: [number, number];
    outline(i: number): OutlineShape[];
    links(i: number, scale: number, ox: number, oy: number): LinkBoxWire[];
    assets(i: number): SlideAssets;
    mediaStates(src: SlidesShowHandle | SlidesTimelineHandle, at: number, which: 0 | 1, clock?: { latencyMs?: number; scale?: number }): MediaStates | null;
    fonts(): EmbeddedFontWire[];
    typefaces(): string[];
    showSettings(): ShowSettingsWire | null;
    free(): void;
  }

  export class SlidesModule {
    readonly maxRounds: number;
    readonly exports: WebAssembly.Exports;
    open(bytes: Uint8Array): Promise<SlidesDocument>;
    openSync(bytes: Uint8Array): SlidesDocument;
  }

  export function loadSlidesModule(options: LoadSlidesOptions): Promise<SlidesModule>;
  export function slidesModuleLoaded(): boolean;
  export function currentSlidesModule(options: LoadSlidesOptions): SlidesModule | null;
  export function slidesHandles(): number;
  export function slidesLiveBytes(): number;
  export function slidesMemoryBytes(): number;
}
