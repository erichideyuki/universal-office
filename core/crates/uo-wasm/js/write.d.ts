// Types of js/write.mjs (the Write exports of uo-wasm, feature `write`; record
// core/docs/modules/uo-bindings.md). Units: page geometry, run boxes and block lines in twips;
// navigation in points from the page top (as the PDF writes it).

export type Variant = 'regular' | 'bold' | 'italic' | 'boldItalic';
export type FontSource = 'host' | 'localAccess' | 'bundled' | 'page' | 'installed' | 'substitute' | 'default';

export declare class WriteError extends Error {
  /** 1 argument · 2 invalid operation · 3 invalid data · 4 XML · 5 limit · 10 cancelled · 11 no font · 12 internal · 13 output refused */
  readonly code: number;
  readonly stage: '' | 'read' | 'layout' | 'pdf';
  /** the maximum of the limit that was hit, when known (else 0) */
  readonly max: number;
  readonly detail: string;
}

export interface WriteHost {
  memory: WebAssembly.Memory | null;
  sink: ((chunk: Uint8Array) => void) | null;
  /** the callback of `LayoutOptions.cancelCheck` while a layout runs (G4) */
  poll: (() => boolean) | null;
}

/** The import object of the module (`uo_host.pdf_chunk` + `uo_host.layout_poll`) and the slot `bindWrite` fills. */
export declare function writeImports(): { imports: WebAssembly.Imports; host: WriteHost };

export interface HostFaceInput {
  variant?: Variant;
  bytes: Uint8Array;
  /** the face to take from a `.ttc` */
  ttcIndex?: number;
}

export interface PageInfo {
  w: number;
  h: number;
  section: number;
  /** the PAGE number as shown */
  number: number;
  /** items at the start of the page that are the header/footer layers */
  hfItems?: number;
}

export type NavLink =
  | { page: number; x: number; y: number; w: number; h: number; uri: string }
  | { page: number; x: number; y: number; w: number; h: number; dest: { page: number; top: number } };

export interface Navigation {
  links: NavLink[];
  outline: { title: string; level: number; page: number; top: number }[];
}

export interface RunBox {
  page: number;
  x: number;
  y: number;
  w: number;
  h: number;
  baseline: number;
  /** the link, or the plugin inline's type */
  value: string;
}

/** A rectangle of commented text (twips of page `page`). */
export interface AnnotationRect {
  page: number;
  x: number;
  y: number;
  w: number;
  h: number;
}

/** The viewer's comments (reading order) and headings (`collectAnnotations`). */
export interface Annotations {
  comments: { id: number; author?: string; text: string; rects: AnnotationRect[] }[];
  headings: { level: number; text: string; page: number; y: number }[];
}

export interface LayoutOptions {
  /** the instant DATE/TIME fields show (default: now) */
  now?: Date | number;
  /** minutes east of UTC at that instant (default: the browser's) */
  tzOffsetMin?: number;
  hyphenation?: 'on' | 'off' | 'force';
  /** e.g. `['pt-BR', 'en', 'es']` (the viewer's list) */
  hyphenationLanguages?: string[];
  view?: 'print' | 'web' | 'draft';
  webWidthTw?: number;
  revisions?: 'final' | 'simple' | 'all' | 'original';
  /** numbers and dates (default `pt-BR`) */
  lang?: string;
  /** 0 or absent: the default page limit */
  maxPages?: number;
  /** the layout id stamped on its page messages (0 or absent: a counter of the module) */
  generation?: number;
  /**
   * Polled during the layout (the first time at once, then every few ms of work); return `true`
   * to stop it: the call throws a `WriteError` with code 10 (cancelled). A synchronous call cannot
   * see an `AbortSignal` fire, so use a deadline, e.g. `() => performance.now() > end`. It must
   * not call into the module; an exception counts as `true`. (G4)
   */
  cancelCheck?: () => boolean;
}

/** A result buffer of render.mjs (`take()` = status + a copy of the bytes, then free). */
export interface Buf {
  readonly status: number;
  readonly length: number;
  bytes(): Uint8Array;
  take(): { status: number; data: Uint8Array };
  free(): void;
}

export interface PaintOps {
  /** the render message alone (no Write section), on the same resource mark as `WriteLayout.encodePage` */
  encodePage(page: number): Buf;
  fxLayers(page: number, item: number, ppt: number): Buf;
  fxLayerSet(page: number, item: number, ppt: number): Buf;
  pattern(page: number, item: number): Buf;
  rectGradient(page: number, item: number): Buf;
  blipTiles(page: number, item: number): Buf;
  pageFillTileTw(page: number, devicePxTw: number): Buf;
  pageFillRaster(page: number): Buf;
  pictureBounds(page: number, item: number): Buf;
  pictureSubs(page: number, item: number): Buf;
  /** the picture-layer masks (frontier D; see src/render.rs "the picture-layer masks"); `rgba` is consumed copy-in, masks are `w * h` `Float32Array` */
  picturePolys(page: number, item: number, ppt: number, ox: number, oy: number): Buf;
  pictureSilhouette(page: number, item: number, ppt: number, ox: number, oy: number, w: number, h: number): Buf;
  pictureBevel(page: number, item: number, ppt: number, ox: number, oy: number, w: number, h: number, rgba: Uint8ClampedArray | Uint8Array): Buf;
  pictureAdjust(page: number, item: number, rgba: Uint8ClampedArray | Uint8Array): Buf;
  outerShadow(page: number, item: number, sil: Float32Array, w: number, h: number, box: { x: number; y: number; w: number; h: number }, dirDeg: number, ppt: number): Buf;
  reflectionAlphas(page: number, item: number, top: number, hPpt: number, rows: number): Buf;
  innerShadow(sil: Float32Array, w: number, h: number, blurPt: number, distPt: number, dirDeg: number, ppt: number): Buf;
  glow(sil: Float32Array, w: number, h: number, radPt: number, ppt: number): Buf;
  softEdge(sil: Float32Array, w: number, h: number, radPt: number, ppt: number): Buf;
  gaussianBlur(m: Float32Array, w: number, h: number, sigma: number): Buf;
  setMaxFxPx(px: number): number;
}

/**
 * The decoded results of the paint operations (`WriteLayout.rasters()`, render.mjs `rasters`): `null` for "nothing to paint"
 * (status 1) and for a limit (status 4). It is the `RasterSource` of js/paint.ts, the TS painter of decoded pages:
 *
 *     const tables = newTables();                                  // js/gen/render.ts
 *     const rasters = layout.rasters();
 *     const page = decodePage(layout.encodePage(i), tables);       // pages in order
 *     const p = { page, tables, index: i, rasters, owner: layout };
 *     paintPageBackground(ctx, p, env, { devicePxTw, screen, paper });
 *     paintItems(ctx, p, env);
 */
export interface RasterSource {
  fxLayer(page: number, item: number, ppt: number): { x0: number; y0: number; w: number; h: number; data: Uint8Array } | null;
  /** the screen's layers (the Word's lower resolution for large blurs, a layer per effect then), each with its pixels per point */
  fxLayerSet(page: number, item: number, ppt: number): { ppt: number; x0: number; y0: number; w: number; h: number; data: Uint8Array }[] | null;
  pattern(page: number, item: number): { x: number; y: number; cellsW: number; cellsH: number; w: number; h: number; data: Uint8Array } | null;
  rectGradient(page: number, item: number): { w: number; h: number; data: Uint8Array } | null;
  blipTiles(page: number, item: number): { x: number; y: number; w: number; h: number }[] | null;
  pageFillTileTw(page: number, devicePxTw: number): { w: number; h: number } | null;
  pageFillRaster(page: number): { w: number; h: number; data: Uint8Array } | null;
  pictureBounds(page: number, item: number): { x0: number; y0: number; x1: number; y1: number } | null;
  picturePolys(page: number, item: number, ppt: number, ox: number, oy: number): Float64Array[] | null;
  pictureSilhouette(page: number, item: number, ppt: number, ox: number, oy: number, w: number, h: number): Float32Array | null;
  pictureBevel(page: number, item: number, ppt: number, ox: number, oy: number, w: number, h: number, rgba: Uint8ClampedArray): Uint8Array | null;
  pictureAdjust(page: number, item: number, rgba: Uint8ClampedArray): Uint8Array | null;
  outerShadow(page: number, item: number, sil: Float32Array, w: number, h: number, box: { x: number; y: number; w: number; h: number }, dirDeg: number, ppt: number): Float32Array | null;
  reflectionAlphas(page: number, item: number, top: number, hPpt: number, rows: number): Float64Array | null;
  innerShadow(sil: Float32Array, w: number, h: number, blurPt: number, distPt: number, dirDeg: number, ppt: number): Float32Array | null;
  glow(sil: Float32Array, w: number, h: number, radPt: number, ppt: number): Float32Array | null;
  softEdge(sil: Float32Array, w: number, h: number, radPt: number, ppt: number): Float32Array | null;
  gaussianBlur(m: Float32Array, w: number, h: number, sigma: number): Float32Array | null;
  coloredMask(mask: Float32Array, w: number, h: number, hex: string, alpha: number): Uint8Array | null;
  dash(name: string): number[] | null;
}

export declare class WriteEnv {
  readonly ptr: number;
  free(): void;
  pushFamily(family: string, o?: { source?: FontSource; embeddable?: boolean; faces?: HostFaceInput[] }): void;
  pushAsset(family: string, variant: Variant, bytes: Uint8Array, advancesJson?: string): void;
  clearFonts(): void;
  takeMissing(): { family: string; variants: Variant[] }[];
  registerHyphenation(patternText: string): void;
}

export declare class WriteLayout {
  readonly pageCount: number;
  free(): void;
  pageInfo(i: number): PageInfo;
  /** the size (twips) and background colour of every page, without decoding any page */
  pageHeads(): { w: number; h: number; background?: string }[];
  /** the page message (decode with js/gen/render.ts `decodePage`); its `app` is the Write section
   *  (js/gen/write.ts `decodeWritePage`: lines, chars, formats, objects, cells, `paras` for the text layer) */
  encodePage(i: number): Uint8Array;
  /** `collectAnnotations(full, layout)` of the viewer */
  annotations(): Annotations;
  readonly paint: PaintOps;
  /** the decoded facade of `paint` (the `RasterSource` of js/paint.ts) */
  rasters(): RasterSource;
  navigation(): Navigation;
  runBoxes(kind?: 'links' | 'inlines'): RunBox[];
  blockLine(block: number): { page: number; y: number } | null;
  warnings(): string[];
  missingFaces(): { family: string; bold: boolean; italic: boolean }[];
  faceBytes(face: number): Uint8Array | null;
  imageBytes(image: number): Uint8Array | null;
  /** the SVG version of image `image`, or null */
  imageSvg(image: number): Uint8Array | null;
  pdf(o?: { compress?: boolean; untrusted?: boolean; chunk?: number }): { chunks: Uint8Array[]; length: number };
}

export declare class WriteDocument {
  free(): void;
  hyphenationPatterns(env: WriteEnv, appLanguages?: string[]): string[];
  layout(env: WriteEnv, o?: LayoutOptions): WriteLayout;
}

export interface WriteBinding {
  WriteEnv: typeof WriteEnv;
  WriteDocument: typeof WriteDocument;
  WriteLayout: typeof WriteLayout;
  /** the render handles of render.mjs (decoders, Buf) */
  render: unknown;
  newEnv(o?: { phone?: boolean }): WriteEnv;
  open(docx: Uint8Array): WriteDocument;
}

export declare function bindWrite(exports: WebAssembly.Exports, host: WriteHost): WriteBinding;

/** `true` when the page may compile WebAssembly (false under a CSP without 'wasm-unsafe-eval'/'unsafe-eval'). */
export declare function canCompileWasm(): boolean;

/** The module built by `tools/build-wasm2js.mjs`: `export default function uoWasm2js(imports)`. */
export type Wasm2jsFactory = (imports: { uo_host: { pdf_chunk(ptr: number, len: number): number; layout_poll(): number } }) => WebAssembly.Exports;
type MaybePromise<T> = T | Promise<T>;

export interface LoadWriteOptions {
  /** `'auto'` (default): WebAssembly when the page may compile it, else `js` (also when the compile throws) */
  engine?: 'auto' | 'wasm' | 'js';
  /** the .wasm: bytes, a URL, a `Response` or a promise of one */
  wasm?: Uint8Array | ArrayBuffer | string | URL | Response | Promise<Uint8Array | ArrayBuffer | Response>;
  /** the wasm2js build: its module URL, the namespace, the factory, or a function returning either */
  js?: string | URL | Wasm2jsFactory | { default: Wasm2jsFactory } | (() => MaybePromise<{ default: Wasm2jsFactory } | Wasm2jsFactory>);
}

/** Loads the Write module with the chosen engine and binds it (`loadWrite` in js/write.mjs). */
export declare function loadWrite(options?: LoadWriteOptions): Promise<{ uo: WriteBinding; engine: 'wasm' | 'js'; exports: WebAssembly.Exports }>;
