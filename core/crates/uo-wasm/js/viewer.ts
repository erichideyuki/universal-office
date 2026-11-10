// The engine side of the viewer's TS/Rust switch (`<universal-document-viewer engine="rust">`; record
// core/docs/modules/uo-bindings.md "The TS/Rust switch of the viewer"). One wasm module per page, one `ViewerEnv` per viewer
// element, one `ViewerDocument` per open document, one `DecodedLayout` per layout: every page of the layout decoded once (the
// render message for the painter of js/paint.ts, its Write section for the text layer), the painter over them, the queries the
// viewer needs (annotations, links, navigation, block lines, faces, images) and the PDF. No DOM element logic and no font
// resolver here: that is `write/packages/viewer/src/rust-engine.ts`, which reaches this module through the alias
// `@universal-office/write-engine` (typed there by the ambient `write/packages/viewer/src/write-engine.d.ts`; `viewer.check.ts`
// fails tsc when the two drift).
//
// Erasable TypeScript only (the flags of paint.ts and the generated codecs).

import { loadWrite } from './write.mjs';
import type { Annotations, FontSource, HostFaceInput, LayoutOptions, LoadWriteOptions, Navigation, RunBox, Variant, WriteBinding, WriteDocument, WriteEnv, WriteLayout } from './write.mjs';
import { decodePage, newTables } from './gen/render.ts';
import type { Face, Image as ImageEntry, Item, Page, Tables } from './gen/render.ts';
import { decodeWritePage, newTables as newWriteTables } from './gen/write.ts';
import type { LaidOutLine, TextPara } from './gen/write.ts';
import { defaultCaches, paintItems, paintPageBackground } from './paint.ts';
import type { PaintEnv, PaintPage } from './paint.ts';
import { fnv } from './render.mjs';
import { hyphenationPatternText } from './hyphen-patterns.mjs';

export { WriteError } from './write.mjs';
export type { Annotations, FontSource, HostFaceInput, LayoutOptions, LoadWriteOptions, Navigation, RunBox, Variant };
export type { Face, ImageEntry, Item, LaidOutLine, PaintEnv, TextPara };

/** What the viewer reads of a laid-out page. */
export interface ViewerPage {
  index: number;
  widthTw: number;
  heightTw: number;
  background?: string;
  items: Item[];
  lines: LaidOutLine[];
  paras: TextPara[];
}

/** The live handles of this page's module (environments, documents, layouts): 0 after everything was freed. */
let handles = 0;

export class ViewerEnv {
  readonly inner: WriteEnv;
  #freed = false;

  constructor(inner: WriteEnv) {
    this.inner = inner;
    handles++;
  }

  pushAsset(family: string, variant: Variant, bytes: Uint8Array, advancesJson?: string): void {
    this.inner.pushAsset(family, variant, bytes, advancesJson);
  }

  pushFamily(family: string, o?: { source?: FontSource; embeddable?: boolean; faces?: HostFaceInput[] }): void {
    this.inner.pushFamily(family, o);
  }

  takeMissing(): { family: string; variants: Variant[] }[] {
    return this.inner.takeMissing();
  }

  registerHyphenation(patternText: string): void {
    this.inner.registerHyphenation(patternText);
  }

  clearFonts(): void {
    this.inner.clearFonts();
  }

  get freed(): boolean {
    return this.#freed;
  }

  free(): void {
    if (this.#freed) return;
    this.#freed = true;
    handles--;
    this.inner.free();
  }
}

export class ViewerDocument {
  readonly inner: WriteDocument;
  #freed = false;

  constructor(inner: WriteDocument) {
    this.inner = inner;
    handles++;
  }

  hyphenationPatterns(env: ViewerEnv, langs?: string[]): string[] {
    return this.inner.hyphenationPatterns(env.inner, langs);
  }

  /** Lays the document out and decodes every page. Throws a `WriteError` (code 10 when `opts.cancelCheck` stopped it). */
  layout(env: ViewerEnv, opts?: LayoutOptions): DecodedLayout {
    return new DecodedLayout(this.inner.layout(env.inner, opts));
  }

  get freed(): boolean {
    return this.#freed;
  }

  free(): void {
    if (this.#freed) return;
    this.#freed = true;
    handles--;
    this.inner.free();
  }
}

/** One layout with its pages decoded (the page messages in order, one set of tables for each of the two decoders). */
export class DecodedLayout {
  /** the discriminant of the viewer's `LayoutResult | DecodedLayout` union */
  readonly engine = 'rust' as const;
  readonly pages: ViewerPage[] = [];
  readonly tables: Tables = newTables();
  readonly inner: WriteLayout;
  #paint: PaintPage[] = [];
  #rasters: PaintPage['rasters'] | null = null;
  #faceKeys = new Map<number, string | null>();
  #navigation: Navigation | null = null;
  #freed = false;

  constructor(inner: WriteLayout) {
    this.inner = inner;
    handles++;
    try {
      const wt = newWriteTables();
      this.#rasters = inner.rasters();
      for (let i = 0; i < inner.pageCount; i++) {
        // the render message and its Write section, in encode order (the tables grow with each message)
        const page: Page = decodePage(inner.encodePage(i), this.tables);
        const sec = decodeWritePage(page.app, wt);
        page.app = new Uint8Array(0); // decoded: the bytes are not needed again
        this.pages.push({
          index: i, widthTw: page.widthTw, heightTw: page.heightTw, ...(page.background !== undefined ? { background: page.background } : {}),
          items: page.items, lines: sec.lines, paras: sec.paras,
        });
        this.#paint.push({ page, tables: this.tables, index: i, rasters: this.#rasters, owner: this });
      }
    } catch (e) {
      this.free();
      throw e;
    }
  }

  get freed(): boolean {
    return this.#freed;
  }

  /** the page background (colour and fill effect) in a context already in twips; a no-op after `free()` */
  paintBackground(ctx: CanvasRenderingContext2D, i: number, env: PaintEnv, opts: { devicePxTw: number; screen: boolean; paper: string }): void {
    const p = this.#freed ? undefined : this.#paint[i];
    if (p) paintPageBackground(ctx, p, env, opts, defaultCaches);
  }

  /** the page items in a context already in twips; a no-op after `free()` */
  paintItems(ctx: CanvasRenderingContext2D, i: number, env: PaintEnv, opts: { from?: number; to?: number; alpha?: number } = {}): void {
    const p = this.#freed ? undefined : this.#paint[i];
    if (p) paintItems(ctx, p, env, opts, defaultCaches);
  }

  /** the index of a face of the page's tables (the `face` of its glyph runs) */
  faceIndex(face: Face): number {
    return this.tables.faces.indexOf(face);
  }

  /** a content key of the font file of face `i` (FNV-1a of the bytes and their length), null when it has no file */
  faceKey(i: number): string | null {
    if (this.#freed) return null;
    let k = this.#faceKeys.get(i);
    if (k === undefined) {
      const b = this.inner.faceBytes(i);
      k = b ? `${fnv(b)}-${b.length}` : null;
      this.#faceKeys.set(i, k);
    }
    return k;
  }

  faceBytes(i: number): Uint8Array | null {
    return this.#freed ? null : this.inner.faceBytes(i);
  }

  imageBytes(i: number): Uint8Array | null {
    return this.#freed ? null : this.inner.imageBytes(i);
  }

  imageSvg(i: number): Uint8Array | null {
    return this.#freed ? null : this.inner.imageSvg(i);
  }

  annotations(): Annotations {
    return this.inner.annotations();
  }

  runBoxes(kind: 'links' | 'inlines'): RunBox[] {
    return this.inner.runBoxes(kind);
  }

  navigation(): Navigation {
    return (this.#navigation ??= this.inner.navigation());
  }

  blockLine(block: number): { page: number; y: number } | null {
    return this.inner.blockLine(block);
  }

  /** the PDF of this layout (compressed, as the TS `renderPdf`), the chunks joined */
  pdf(o: { compress?: boolean; untrusted?: boolean } = {}): Uint8Array {
    const { chunks, length } = this.inner.pdf(o);
    const out = new Uint8Array(length);
    let at = 0;
    for (const c of chunks) { out.set(c, at); at += c.length; }
    return out;
  }

  /** Frees the layout handle (idempotent); every paint call afterwards is a no-op. */
  free(): void {
    if (this.#freed) return;
    this.#freed = true;
    handles--;
    this.#paint = [];
    this.inner.free();
  }
}

export interface ViewerModule {
  readonly engine: 'wasm' | 'js';
  newEnv(): ViewerEnv;
  /** Opens a .docx; throws a `WriteError` on invalid input before anything else changes. */
  open(docx: Uint8Array): ViewerDocument;
  /** the pattern file text of a hyphenation id (the TS pattern modules), null for an unknown id */
  patternText(id: string): Promise<string | null>;
  /** `buffer`: the wasm memory in bytes; `live`: the module's live allocation (the `count` build only); `handles`: environments + documents + layouts not freed */
  stats(): { buffer: number; live: number | null; handles: number };
}

let loading: Promise<ViewerModule> | null = null;
let loaded = false;

/** `true` once the module of this page was loaded (a viewer without `engine="rust"` never loads it). */
export function viewerModuleLoaded(): boolean {
  return loaded;
}

/**
 * The module of this page: ONE per page, memoized, so the first `src` wins and later ones are ignored. A failed load is
 * forgotten, so a later call retries.
 */
export function loadViewerModule(src: LoadWriteOptions = {}): Promise<ViewerModule> {
  if (!loading) {
    const p = loadWrite(src).then(({ uo, engine, exports }): ViewerModule => {
      loaded = true;
      return makeModule(uo, engine, exports);
    });
    loading = p;
    p.catch(() => { if (loading === p) loading = null; });
  }
  return loading;
}

function makeModule(uo: WriteBinding, engine: 'wasm' | 'js', exports: WebAssembly.Exports): ViewerModule {
  const memory = exports['memory'] as WebAssembly.Memory;
  const live = exports['uo_memory_live_bytes'] as (() => bigint | number) | undefined;
  return {
    engine,
    newEnv: () => new ViewerEnv(uo.newEnv()),
    open: (docx) => new ViewerDocument(uo.open(docx)),
    patternText: hyphenationPatternText,
    stats: () => ({ buffer: memory.buffer.byteLength, live: typeof live === 'function' ? Number(live()) : null, handles }),
  };
}
