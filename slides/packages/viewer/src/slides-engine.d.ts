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

  export interface LoadSlidesOptions { wasm?: Bytes; ppt?: Bytes }

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
  }

  export interface SlidePaintOptions { skipCharts?: boolean }
  export const CHARTS_LEFT: 15;

  export interface SlidePaintResult { status: number; charts: number; rounds: number }

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

  export class SlidesDocument {
    readonly count: number;
    readonly slideW: number;
    readonly slideH: number;
    readonly freed: boolean;
    readonly generation: number;
    notes(i: number): string;
    part(name: string): Uint8Array | null;
    fontsChanged(): void;
    imageReady(part: string, img: PaintImage): void;
    paint(ctx: CanvasRenderingContext2D, index: number, scale: number, ox: number, oy: number, host: SlidesPaintHost, o?: SlidePaintOptions): SlidePaintResult;
    clearCaches(): void;
    newShow(o?: { from?: number; now?: number; useTimings?: number; loop?: number; animations?: number; range?: [number, number] | null }): SlidesShowHandle;
    cacheSizes(): { derived: number; bitmaps: number };
    free(): void;
  }

  export class SlidesModule {
    readonly maxRounds: number;
    open(bytes: Uint8Array): Promise<SlidesDocument>;
  }

  export function loadSlidesModule(options: LoadSlidesOptions): Promise<SlidesModule>;
  export function slidesModuleLoaded(): boolean;
  export function slidesHandles(): number;
  export function slidesLiveBytes(): number;
  export function slidesMemoryBytes(): number;
}
