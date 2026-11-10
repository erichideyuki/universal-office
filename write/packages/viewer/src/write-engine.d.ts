// The surface of the Rust engine's JS side (core/crates/uo-wasm/js/viewer.ts) that the viewer's `rust-engine.ts` uses, declared
// here so the viewer's own `tsc` build (rootDir `src`) does not compile the core. The real module is reached through the
// alias `@universal-office/write-engine` (vitest.config.ts, `build:demo`, tools/build-pkg.mjs `ENGINE`).
// `core/crates/uo-wasm/js/viewer.check.ts` assigns the real module to this declaration: drift fails the core's tsc line.
declare module '@universal-office/write-engine' {
  export type Variant = 'regular' | 'bold' | 'italic' | 'boldItalic';
  export type FontSource = 'host' | 'localAccess' | 'bundled' | 'page' | 'installed' | 'substitute' | 'default';

  /** The `WriteError` of the engine (code 10 = cancelled). */
  export class WriteError extends Error {
    readonly code: number;
    readonly stage: '' | 'read' | 'layout' | 'pdf';
    readonly max: number;
    readonly detail: string;
  }

  export interface HostFaceInput { variant?: Variant; bytes: Uint8Array; ttcIndex?: number }

  export interface LayoutOptions {
    now?: Date | number;
    tzOffsetMin?: number;
    hyphenation?: 'on' | 'off' | 'force';
    hyphenationLanguages?: string[];
    view?: 'print' | 'web' | 'draft';
    webWidthTw?: number;
    revisions?: 'final' | 'simple' | 'all' | 'original';
    lang?: string;
    maxPages?: number;
    generation?: number;
    cancelCheck?: () => boolean;
  }

  export interface LoadWriteOptions {
    engine?: 'auto' | 'wasm' | 'js';
    wasm?: Uint8Array | ArrayBuffer | string | URL | Response | Promise<Uint8Array | ArrayBuffer | Response>;
    /** the wasm2js build: its module URL */
    js?: string | URL;
  }

  /** The items the viewer reads (`webPageHeight`); the others are only painted. */
  export type ViewerItem =
    | { t: 'rect' | 'image' | 'shape'; y: number; h: number }
    | { t: 'line'; y1: number; y2: number }
    | { t: 'glyphs'; y: number }
    | { t: 'path' | 'clip' | 'xform' };

  export interface ViewerLine {
    ref: string;
    start: number;
    end: number;
    x: number;
    y: number;
    height: number;
    width: number;
    para?: number;
    chars?: { o: number; x: number; w: number }[];
  }

  export interface ViewerPage {
    index: number;
    widthTw: number;
    heightTw: number;
    background?: string;
    items: ViewerItem[];
    lines: ViewerLine[];
    paras: { text: string; level?: number }[];
  }

  export interface Face {
    family: string;
    bold?: boolean;
    italic?: boolean;
    synthBold?: boolean;
    synthItalic?: boolean;
    hasBytes?: boolean;
  }

  export interface ImageEntry { ext: string; byteLength: number; hasSvg?: boolean; widthTwips: number; heightTwips: number }

  export interface PaintEnv {
    fontOf(face: Face): { family: string; weight: 400 | 700; italic: boolean };
    image(id: number): CanvasImageSource | null;
    color(hex: string): string;
  }

  export interface RunBox { page: number; x: number; y: number; w: number; h: number; baseline: number; value: string }

  export interface Annotations {
    comments: { id: number; author?: string; text: string; rects: { page: number; x: number; y: number; w: number; h: number }[] }[];
    headings: { level: number; text: string; page: number; y: number }[];
  }

  export interface Navigation {
    links: ({ page: number; x: number; y: number; w: number; h: number } & ({ uri: string } | { dest: { page: number; top: number } }))[];
    outline: { title: string; level: number; page: number; top: number }[];
  }

  export interface ViewerEnv {
    readonly freed: boolean;
    pushAsset(family: string, variant: Variant, bytes: Uint8Array, advancesJson?: string): void;
    pushFamily(family: string, o?: { source?: FontSource; embeddable?: boolean; faces?: HostFaceInput[] }): void;
    takeMissing(): { family: string; variants: Variant[] }[];
    registerHyphenation(patternText: string): void;
    clearFonts(): void;
    free(): void;
  }

  export interface DecodedLayout {
    readonly engine: 'rust';
    readonly pages: ViewerPage[];
    readonly tables: { faces: Face[]; images: ImageEntry[] };
    readonly freed: boolean;
    paintBackground(ctx: CanvasRenderingContext2D, i: number, env: PaintEnv, opts: { devicePxTw: number; screen: boolean; paper: string }): void;
    paintItems(ctx: CanvasRenderingContext2D, i: number, env: PaintEnv, opts?: { from?: number; to?: number; alpha?: number }): void;
    faceIndex(face: Face): number;
    faceKey(i: number): string | null;
    faceBytes(i: number): Uint8Array | null;
    imageBytes(i: number): Uint8Array | null;
    imageSvg(i: number): Uint8Array | null;
    annotations(): Annotations;
    runBoxes(kind: 'links' | 'inlines'): RunBox[];
    navigation(): Navigation;
    blockLine(block: number): { page: number; y: number } | null;
    pdf(o?: { compress?: boolean; untrusted?: boolean }): Uint8Array;
    free(): void;
  }

  export interface ViewerDocument {
    readonly freed: boolean;
    hyphenationPatterns(env: ViewerEnv, langs?: string[]): string[];
    layout(env: ViewerEnv, opts?: LayoutOptions): DecodedLayout;
    free(): void;
  }

  export interface ViewerModule {
    readonly engine: 'wasm' | 'js';
    newEnv(): ViewerEnv;
    open(docx: Uint8Array): ViewerDocument;
    patternText(id: string): Promise<string | null>;
    stats(): { buffer: number; live: number | null; handles: number };
  }

  export function loadViewerModule(src?: LoadWriteOptions): Promise<ViewerModule>;
  export function viewerModuleLoaded(): boolean;
}
