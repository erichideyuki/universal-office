// Types of js/slides.mjs (the JS side of the Slides exports of uo-wasm, src/slides.rs).

export declare const STATUS: Record<number, string>;
export declare class SlidesError extends Error {
  readonly status: number;
  readonly detail: string;
  constructor(status: number, what: string, detail?: string);
}

export interface FaceReq { family: string; bold: boolean; italic: boolean }
interface KeyReq { face: FaceReq; sizePx: number; kerning: 'current' | 'none' | 'normal'; letterSpacingPx: number; text: string }
export type MeasureRequest =
  | { kind: 'charAt1000'; face: FaceReq; ch: string }
  | { kind: 'kernAt1000'; face: FaceReq; text: string }
  | ({ kind: 'width' } & KeyReq)
  | ({ kind: 'metrics' } & KeyReq)
  | { kind: 'fontAvailable'; family: string }
  | { kind: 'imageSize'; media: number; part: string }
  | { kind: 'metaText'; font: string; text: string }
  | { kind: 'metaDecode'; key: string; data: Uint8Array };

export interface ChartFrame { shape: number; x: number; y: number; w: number; h: number }
export type EmitResult =
  | { status: 0; flags: number; newGeneration: boolean; charts: ChartFrame[]; message: Uint8Array }
  | { status: 12; requests: MeasureRequest[] }
  | { status: number };

export interface TransitionWire { type: string; ns: string; attrs: Record<string, string>; durMs: number; advTmMs?: number; advClick: boolean }
export interface ShowFrame {
  index: number; step: number; steps: number; ended: boolean; animMs: number; transitionProgress: number; effects: number;
  from: { index: number; animMs: number; key: number; effects: number } | null;
  transition: TransitionWire | null;
}
export type ShowAction = 'none' | 'transition' | 'step' | 'slide' | 'end' | 'exit';

export declare class SlidesView {
  readonly ptr: number;
  bitmap(handle: number): Uint8Array | null;
  free(): void;
}
export declare class SlidesShow {
  next(now: number, mouse?: boolean): ShowAction;
  prev(now: number): ShowAction;
  goTo(index: number, now: number): ShowAction;
  first(now: number): ShowAction;
  last(now: number): ShowAction;
  update(now: number): ShowAction;
  dueAt(): number;
  wakeAt(now: number): number;
  frame(now: number): ShowFrame;
  free(): void;
}
export declare class SlidesTimeline {
  info(): { durationMs: number; tickMs: number; mediaClock: number; slides: { index: number; startMs: number; endMs: number; animStartMs: number; transitionMs: number }[] };
  stateAt(t: number): { pos: number; index: number; from: number; transitionProgress: number; animMs: number; ended: boolean; effects: number; transition: TransitionWire | null };
  free(): void;
}
export declare class SlidesDeck {
  readonly ptr: number;
  readonly count: number;
  size(): { w: number; h: number; count: number };
  notes(i: number): string;
  part(name: string): Uint8Array | null;
  setCaps(c: { fontKerning: boolean; letterSpacing: boolean; segmenter: boolean; fontBoundingBox: boolean }): void;
  setGeneration(g: number): void;
  setImageSize(media: number, w: number, h: number): void;
  clearCaches(): void;
  emit(view: SlidesView, index: number, scale: number, ox: number, oy: number): EmitResult;
  answer(nums: Float64Array): void;
  newShow(o?: { from?: number; now?: number; useTimings?: number; loop?: number; animations?: number; range?: [number, number] | null }): SlidesShow;
  newTimeline(o?: { useTimings?: number; defaultSlideMs?: number; tickMs?: number; transitionFps?: number }): SlidesTimeline;
  free(): void;
}
export interface SlidesBinding {
  open(bytes: Uint8Array): SlidesDeck;
  newView(generation: number): SlidesView;
  readonly maxRounds: number;
  liveBytes(): number;
  SlidesDeck: typeof SlidesDeck;
  SlidesView: typeof SlidesView;
}
export declare function bindSlides(exports: WebAssembly.Exports): SlidesBinding;
export declare function bindSlidesPpt(exports: WebAssembly.Exports): { convert(bytes: Uint8Array): Uint8Array };
