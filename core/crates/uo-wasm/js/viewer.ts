// The engine side of the viewer's TS/Rust switch (`<universal-document-viewer engine="rust">`; record
// core/docs/modules/uo-bindings.md "The TS/Rust switch of the viewer"). One wasm module per page, one page-lifetime `ViewerEnv`
// per fonts base (shared by every viewer element, record "Open speed"), one `ViewerDocument` per open document, one `DecodedLayout` per layout: every page of the layout decoded once (the
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
import { changedBand } from './band.ts';
import type { BandPage } from './band.ts';
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

/**
 * One instantiated wasm module. A `WebAssembly.RuntimeError` (a trap: `panic = abort`, an allocation failure, an
 * `unreachable`) stops Rust in the middle of a call, so the allocator, the borrow flags and the font registry's locks
 * can be left inconsistent, and the page environment lives in that module: the instance is DEAD from then on, every
 * object of it refuses further calls, and the next `loadViewerModule` instantiates a fresh module. (The wasm2js engine
 * traps as a plain JS error and is not detected.)
 */
class Instance {
  dead = false;
  /** the live per-document handles (documents, layouts, element-lifetime environments): 0 after everything was freed */
  handles = 0;
  /** the live page-lifetime environments (`newEnv({ page: true })`): fonts and hyphenation shared by every viewer of the page */
  pageEnvs = 0;
  readonly exports: WebAssembly.Exports;

  constructor(exports: WebAssembly.Exports) {
    this.exports = exports;
  }
}

const STOPPED = 'uo-wasm: the engine instance stopped after a trap; open the document again';

/** Marks the instance dead (its handles are gone with it) and lets the next `loadViewerModule` start a fresh module. */
function kill(inst: Instance): void {
  inst.dead = true;
  inst.handles = 0;
  inst.pageEnvs = 0;
  if (current === inst) {
    current = null;
    loading = null;
    loaded = false;
  }
}

/** Runs a call into the module: refused on a dead instance; a `WebAssembly.RuntimeError` kills the instance and is rethrown. */
function call<T>(inst: Instance, f: () => T): T {
  if (inst.dead) throw new Error(STOPPED);
  try {
    return f();
  } catch (e) {
    if (e instanceof WebAssembly.RuntimeError) kill(inst);
    throw e;
  }
}

export class ViewerEnv {
  readonly inner: WriteEnv;
  readonly #inst: Instance;
  readonly #page: boolean;
  #freed = false;

  constructor(inst: Instance, inner: WriteEnv, page = false) {
    this.inner = inner;
    this.#inst = inst;
    this.#page = page;
    if (page) inst.pageEnvs++; else inst.handles++;
  }

  pushAsset(family: string, variant: Variant, bytes: Uint8Array, advancesJson?: string): void {
    call(this.#inst, () => this.inner.pushAsset(family, variant, bytes, advancesJson));
  }

  pushFamily(family: string, o?: { source?: FontSource; embeddable?: boolean; faces?: HostFaceInput[] }): void {
    call(this.#inst, () => this.inner.pushFamily(family, o));
  }

  takeMissing(): { family: string; variants: Variant[] }[] {
    return call(this.#inst, () => this.inner.takeMissing());
  }

  registerHyphenation(patternText: string): void {
    call(this.#inst, () => this.inner.registerHyphenation(patternText));
  }

  clearFonts(): void {
    call(this.#inst, () => this.inner.clearFonts());
  }

  /** freed, or its instance stopped after a trap (the caller makes a new environment on the fresh module) */
  get freed(): boolean {
    return this.#freed || this.#inst.dead;
  }

  free(): void {
    if (this.#freed) return;
    this.#freed = true;
    if (this.#inst.dead) return;
    if (this.#page) this.#inst.pageEnvs--; else this.#inst.handles--;
    call(this.#inst, () => this.inner.free());
  }
}

export class ViewerDocument {
  readonly inner: WriteDocument;
  readonly #inst: Instance;
  #freed = false;

  constructor(inst: Instance, inner: WriteDocument) {
    this.inner = inner;
    this.#inst = inst;
    inst.handles++;
  }

  hyphenationPatterns(env: ViewerEnv, langs?: string[]): string[] {
    return call(this.#inst, () => this.inner.hyphenationPatterns(env.inner, langs));
  }

  /** Lays the document out and decodes every page. Throws a `WriteError` (code 10 when `opts.cancelCheck` stopped it). */
  layout(env: ViewerEnv, opts?: LayoutOptions): DecodedLayout {
    return new DecodedLayout(this.#inst, call(this.#inst, () => this.inner.layout(env.inner, opts)));
  }

  get freed(): boolean {
    return this.#freed || this.#inst.dead;
  }

  free(): void {
    if (this.#freed) return;
    this.#freed = true;
    if (this.#inst.dead) return;
    this.#inst.handles--;
    call(this.#inst, () => this.inner.free());
  }
}

/**
 * A page of a layout whose items, lines and paragraphs are decoded the first time somebody reads them (or paints the
 * page): the size and the background come from `pageHeads`, so `applyZoom` and the page boxes cost no decode.
 */
class LazyPage implements ViewerPage {
  readonly index: number;
  readonly widthTw: number;
  readonly heightTw: number;
  readonly background?: string;
  readonly #owner: DecodedLayout;

  constructor(owner: DecodedLayout, index: number, head: { w: number; h: number; background?: string }) {
    this.#owner = owner;
    this.index = index;
    this.widthTw = head.w;
    this.heightTw = head.h;
    if (head.background !== undefined) this.background = head.background;
  }

  get items(): Item[] { return this.#owner.decoded(this.index).items; }
  get lines(): LaidOutLine[] { return this.#owner.decoded(this.index).lines; }
  get paras(): TextPara[] { return this.#owner.decoded(this.index).paras; }
}

/** what a page that failed to decode (or a freed layout) reads as; frozen, so no caller can put items into the shared object */
const NO_PAGE = Object.freeze({
  items: Object.freeze([] as Item[]) as Item[],
  lines: Object.freeze([] as LaidOutLine[]) as LaidOutLine[],
  paras: Object.freeze([] as TextPara[]) as TextPara[],
}) as { items: Item[]; lines: LaidOutLine[]; paras: TextPara[] };

/** wave 6 (L, D-L4): a decoded page kept for `DecodedLayout.bandSince` (opaque to the caller) */
export interface BandSnap extends BandPage {
  readonly serial: number;
}

let layoutSerials = 0;

/** One layout; its pages are decoded on demand (the page messages with one set of tables for each of the two decoders). */
export class DecodedLayout {
  /** the discriminant of the viewer's `LayoutResult | DecodedLayout` union */
  readonly engine = 'rust' as const;
  readonly pages: ViewerPage[] = [];
  readonly tables: Tables = newTables();
  readonly inner: WriteLayout;
  readonly #inst: Instance;
  #paint: (PaintPage | undefined)[] = [];
  /** pages whose decode failed: never encoded again (the encoder's resource deltas already advanced) */
  #failed = new Set<number>();
  #lines: { lines: LaidOutLine[]; paras: TextPara[] }[] = [];
  #wt = newWriteTables();
  #rasters: PaintPage['rasters'] | null = null;
  #warned = false;
  #faceKeys = new Map<number, string | null>();
  /** the bytes `faceKey` fetched, until `faceBytes` hands them over once (one copy out of the wasm memory, not two) */
  #faceHeld = new Map<number, Uint8Array>();
  #navigation: Navigation | null = null;
  #freed = false;
  /** wave 6 (L, D-L4): this layout's serial and the one of the layout it was adopted after (`inner.shared` is against it) */
  readonly #serial = ++layoutSerials;
  readonly #prevSerial: number;

  constructor(inst: Instance, inner: WriteLayout, prev?: DecodedLayout | null) {
    this.inner = inner;
    this.#inst = inst;
    this.#prevSerial = prev ? prev.#serial : -1;
    inst.handles++;
    try {
      this.#rasters = call(inst, () => inner.rasters());
      call(inst, () => inner.pageHeads()).forEach((h, i) => this.pages.push(new LazyPage(this, i, h)));
    } catch (e) {
      this.free();
      throw e;
    }
    // wave 6 (L): the font keys of the faces whose bytes are `prev`'s of the same index (the same `Arc`, said by the engine:
    // `inner.shared`) are `prev`'s — no copy of the file out of the wasm memory, no hash again
    const shared = inner.shared?.faces;
    if (prev && shared) for (const [i, k] of prev.#faceKeys) if (shared[i]) this.#faceKeys.set(i, k);
  }

  /** freed, its instance dead, or STALE (wave 6, L: an editor shell layout whose pages went back to the engine) */
  get freed(): boolean {
    return this.#freed || this.#inst.dead || this.inner.stale === true;
  }

  /** wave 6 (L): whether image `i`'s bytes are those of the previous layout's image `i` (`inner.shared`) */
  sharedImage(i: number): boolean {
    return this.inner.shared?.images[i] === true;
  }

  /**
   * Page `i` decoded (once): its render message and Write section, encoded and decoded together so the tables see the
   * messages in encoder order (the encoder serves the pages in any order; the resource deltas go up to the highest id).
   * A failure is logged once per layout and the page is empty (an error at paint time, not at layout time).
   */
  decoded(i: number): { items: Item[]; lines: LaidOutLine[]; paras: TextPara[] } {
    const p = this.#decode(i);
    if (!p) return NO_PAGE;
    const sec = this.#lines[i]!;
    return { items: p.page.items, lines: sec.lines, paras: sec.paras };
  }

  #decode(i: number): PaintPage | undefined {
    const had = this.#paint[i];
    if (had || this.freed || i < 0 || i >= this.pages.length || this.#failed.has(i)) return had;
    try {
      const page: Page = decodePage(call(this.#inst, () => this.inner.encodePage(i)), this.tables);
      const sec = decodeWritePage(page.app, this.#wt);
      page.app = new Uint8Array(0); // decoded: the bytes are not needed again
      this.#lines[i] = { lines: sec.lines, paras: sec.paras };
      return (this.#paint[i] = { page, tables: this.tables, index: i, rasters: this.#rasters!, owner: this });
    } catch (e) {
      // a trap kills the instance (`call`); it is not a page that failed to decode
      if (e instanceof WebAssembly.RuntimeError) throw e;
      this.#failed.add(i);
      if (!this.#warned) { this.#warned = true; console.error('uo-wasm: decoding page', i, 'failed', e); }
      return undefined;
    }
  }

  /**
   * wave 6 (L, D-L4): page `i` as decoded so far, kept for `bandSince` of the next layout (null when it was not decoded; never
   * decodes). `hf`: the page's header/footer item count.
   */
  bandSnap(i: number, hf: number): BandSnap | null {
    const p = this.#paint[i];
    return p ? { page: p.page, tables: this.tables, hf, serial: this.#serial } : null;
  }

  /**
   * wave 6 (L, D-L4): the band (twips) of page `i` that paints differently from `prev` (a `bandSnap` of the layout this one
   * was adopted after): `'same'`, the band, or null (unknown: repaint the whole page). Decodes page `i`. `slackTw`: the
   * caller's margin around the band, in twips (no picture, shape or path may reach into it).
   */
  bandSince(i: number, prev: BandSnap, hf: number, slackTw = 0): { y0: number; y1: number } | 'same' | null {
    const shared = this.inner.shared;
    if (prev.serial !== this.#prevSerial || !shared) return null;
    const p = this.#decode(i);
    if (!p) return null;
    return changedBand(prev, { page: p.page, tables: this.tables, hf }, shared, slackTw);
  }

  /** the page background (colour and fill effect) in a context already in twips; a no-op after `free()` */
  paintBackground(ctx: CanvasRenderingContext2D, i: number, env: PaintEnv, opts: { devicePxTw: number; screen: boolean; paper: string }): void {
    const p = this.#decode(i);
    if (p) paintPageBackground(ctx, p, env, opts, defaultCaches);
  }

  /** the page items in a context already in twips; a no-op after `free()` */
  paintItems(ctx: CanvasRenderingContext2D, i: number, env: PaintEnv, opts: { from?: number; to?: number; alpha?: number } = {}): void {
    const p = this.#decode(i);
    if (p) paintItems(ctx, p, env, opts, defaultCaches);
  }

  /** the index of a face of the page's tables (the `face` of its glyph runs) */
  faceIndex(face: Face): number {
    return this.tables.faces.indexOf(face);
  }

  /** a content key of the font file of face `i` (FNV-1a of the bytes and their length), null when it has no file */
  faceKey(i: number): string | null {
    if (this.freed) return null;
    let k = this.#faceKeys.get(i);
    if (k === undefined) {
      const b = call(this.#inst, () => this.inner.faceBytes(i));
      k = b ? `${fnv(b)}-${b.length}` : null;
      if (b) this.#faceHeld.set(i, b);
      this.#faceKeys.set(i, k);
    }
    return k;
  }

  /** drops the bytes `faceKey` kept for face `i` (the caller already has a font entry for its key) */
  dropFaceBytes(i: number): void {
    this.#faceHeld.delete(i);
  }

  faceBytes(i: number): Uint8Array | null {
    if (this.freed) return null;
    const held = this.#faceHeld.get(i);
    if (held) { this.#faceHeld.delete(i); return held; }
    return call(this.#inst, () => this.inner.faceBytes(i));
  }

  imageBytes(i: number): Uint8Array | null {
    return this.freed ? null : call(this.#inst, () => this.inner.imageBytes(i));
  }

  imageSvg(i: number): Uint8Array | null {
    return this.freed ? null : call(this.#inst, () => this.inner.imageSvg(i));
  }

  annotations(): Annotations {
    return call(this.#inst, () => this.inner.annotations());
  }

  runBoxes(kind: 'links' | 'inlines'): RunBox[] {
    return call(this.#inst, () => this.inner.runBoxes(kind));
  }

  navigation(): Navigation {
    return (this.#navigation ??= call(this.#inst, () => this.inner.navigation()));
  }

  blockLine(block: number): { page: number; y: number } | null {
    return call(this.#inst, () => this.inner.blockLine(block));
  }

  /** the PDF of this layout (compressed, as the TS `renderPdf`), the chunks joined */
  pdf(o: { compress?: boolean; untrusted?: boolean } = {}): Uint8Array {
    const { chunks, length } = call(this.#inst, () => this.inner.pdf(o));
    const out = new Uint8Array(length);
    let at = 0;
    for (const c of chunks) { out.set(c, at); at += c.length; }
    return out;
  }

  /** Frees the layout handle (idempotent); every paint call afterwards is a no-op. */
  free(): void {
    if (this.#freed) return;
    this.#freed = true;
    this.#paint = [];
    this.#lines = [];
    this.#failed.clear();
    this.#faceHeld.clear();
    if (this.#inst.dead) return;
    this.#inst.handles--;
    call(this.#inst, () => this.inner.free());
  }
}

export interface ViewerModule {
  readonly engine: 'wasm' | 'js';
  /** `true` once a `WebAssembly.RuntimeError` stopped this instance: every call refuses, load a new module with `loadViewerModule` */
  readonly dead: boolean;
  /** `page: true`: the environment lives as long as the page (the viewers share its fonts); it counts in `pageEnvs`, not in `handles` */
  newEnv(o?: { page?: boolean }): ViewerEnv;
  /** Opens a .docx; throws a `WriteError` on invalid input before anything else changes. */
  open(docx: Uint8Array): ViewerDocument;
  /** the pattern file text of a hyphenation id (the TS pattern modules), null for an unknown id */
  patternText(id: string): Promise<string | null>;
  /** `buffer`: the wasm memory in bytes; `live`: the module's live allocation (the `count` build only); `handles`: documents + layouts (+ element-lifetime environments) not freed; `pageEnvs`: page-lifetime environments */
  stats(): { buffer: number; live: number | null; handles: number; pageEnvs: number };
  // the editor switch (record "Milestone 2: the editor switch"): what `<universal-write engine="rust">` binds its session on
  /** the instance's exports (for `bindSession` of js/session.mjs) */
  readonly exports: WebAssembly.Exports;
  /** the write binding of the instance (for `bindSession`) */
  readonly binding: WriteBinding;
  /**
   * a layout made outside this module (a session's) adopted as a `DecodedLayout`: it counts in `handles` and is freed with it;
   * `prev` (wave 6, L): the layout it replaces, whose font keys it inherits for the faces the engine says are shared
   */
  adopt(inner: WriteLayout, prev?: DecodedLayout | null): DecodedLayout;
  /** marks the instance dead after a trap seen outside this module (the session's own calls) */
  kill(): void;
}

let loading: Promise<ViewerModule> | null = null;
/** the live instance of this page (null before the first load and after a trap) */
let current: Instance | null = null;
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
      const inst = new Instance(exports);
      current = inst;
      loaded = true;
      return makeModule(inst, uo, engine, exports);
    });
    loading = p;
    p.catch(() => { if (loading === p) loading = null; });
  }
  return loading;
}

function makeModule(inst: Instance, uo: WriteBinding, engine: 'wasm' | 'js', exports: WebAssembly.Exports): ViewerModule {
  const memory = exports['memory'] as WebAssembly.Memory;
  const live = exports['uo_memory_live_bytes'] as (() => bigint | number) | undefined;
  return {
    engine,
    get dead() { return inst.dead; },
    newEnv: (o) => {
      const page = o?.page === true;
      const inner = call(inst, () => uo.newEnv());
      return new ViewerEnv(inst, inner, page);
    },
    open: (docx) => new ViewerDocument(inst, call(inst, () => uo.open(docx))),
    patternText: hyphenationPatternText,
    stats: () => ({
      buffer: memory.buffer.byteLength,
      live: !inst.dead && typeof live === 'function' ? Number(live()) : null,
      handles: inst.handles,
      pageEnvs: inst.pageEnvs,
    }),
    exports,
    binding: uo,
    adopt: (inner, prev) => new DecodedLayout(inst, inner, prev),
    kill: () => kill(inst),
  };
}

/**
 * Test hook (never declared in the d.ts): traps the current instance through the count build's `uo_test_trap`
 * export, as a panic or an allocation failure would. Throws a `WebAssembly.RuntimeError`, and the instance is dead.
 */
export function __trapForTest(): void {
  const inst = current;
  if (!inst) throw new Error('uo-wasm: no live module');
  const trap = inst.exports['uo_test_trap'] as (() => void) | undefined;
  if (typeof trap !== 'function') throw new Error('uo-wasm: this build has no uo_test_trap (the count build only)');
  call(inst, trap);
}
