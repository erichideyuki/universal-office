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

/** Wave 4, the read model: the accessible outline of a slide (the TS `SlideA11y` input). */
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

export declare class SlidesView {
  readonly ptr: number;
  bitmap(handle: number): Uint8Array | null;
  isRecipe(handle: number): boolean;
  clearStage(): void;
  free(): void;
}
export type StageResult =
  | { status: 0; flags: number; empty: true; animMs: number; list: null; frame: null }
  | { status: 0; flags: number; empty: false; newGeneration: boolean; animMs: number; list: Uint8Array | null; frame: Uint8Array }
  | { status: 12; requests: MeasureRequest[] }
  | { status: number };
export declare class SlidesShow {
  readonly ptr: number;
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
  readonly ptr: number;
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
  emitSuppress(view: SlidesView, index: number, scale: number, ox: number, oy: number, shape: number): EmitResult;
  answer(nums: Float64Array): void;
  outline(i: number): OutlineShape[];
  links(i: number, scale: number, ox: number, oy: number): LinkBoxWire[];
  assets(i: number): SlideAssets;
  mediaStates(src: SlidesShow | SlidesTimeline, at: number, which: 0 | 1, clock?: { latencyMs?: number; scale?: number }): MediaStates | null;
  fonts(): EmbeddedFontWire[];
  typefaces(): string[];
  showSettings(): ShowSettingsWire | null;
  stage(view: SlidesView, src: SlidesShow | SlidesTimeline, at: number, which: 0 | 1, scale: number, ox: number, oy: number,
    media?: boolean, clock?: { latencyMs?: number; scale?: number }): StageResult;
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
