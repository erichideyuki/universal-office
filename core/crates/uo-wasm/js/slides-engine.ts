// The engine side of the Slides viewer's TS/Rust switch (`<universal-slides-viewer engine="rust">`, frontier B7; record
// core/docs/modules/uo-bindings.md "Slides viewer (B7)"). One wasm module per page (`uo-slides.wasm`), one `SlidesDocument` per
// open deck (the wasm deck, one generation of the list stream, the decoder tables and the painter of `paint-slides.ts`). No DOM
// element logic here: that is `slides/packages/viewer/src/rust-engine.ts`, which reaches this module through the alias
// `@universal-office/slides-engine` (typed there by the ambient `slides/packages/viewer/src/slides-engine.d.ts`;
// `slides-engine.check.ts` fails tsc when the two drift).
//
// What it does for a paint: emit the slide's display list (the wasm layout), answer the layout's measurement requests with the
// canvas (`measureText` with the SAME font strings as the TS `layoutFontStr`), decode the list and run the painter. A call that
// traps the wasm (`WebAssembly.RuntimeError`) kills the instance: every handle refuses further calls and the next
// `loadSlidesModule` instantiates a fresh module (the same rule as the Write viewer module).
//
// Erasable TypeScript only (the flags of paint.ts and the generated codecs).

import { bindSlides, bindSlidesPpt, SlidesError } from './slides.mjs';
import type { MeasureRequest, SlidesBinding, SlidesDeck, SlidesShow, SlidesTimeline, SlidesView, ShowAction, ShowFrame } from './slides.mjs';
import { decodeSlidesList, newTables } from './gen/slides.ts';
import type { SlidesList, Tables } from './gen/slides.ts';
import { decodeSlotFrame, newTables as newSlotTables } from './gen/slots.ts';
import type { Tables as SlotTables } from './gen/slots.ts';
import { SlidesPainter } from './paint-slides.ts';
import type { PaintImage, SlidesEnv, SlotReplacement } from './paint-slides.ts';
import { imgH, imgW } from './slides-effects.ts';

export { SlidesError };
export type { MeasureRequest, ShowAction, ShowFrame };
export type { PaintImage };

type Bytes = Uint8Array | ArrayBuffer | string | URL | Response | Promise<Uint8Array | ArrayBuffer | Response>;

/** Where the module comes from: `wasm` the main module, `ppt` the lazily loaded `.ppt` converter. */
export interface LoadSlidesOptions { wasm?: Bytes; ppt?: Bytes }

const STOPPED = 'uo-wasm: the Slides engine instance stopped after a trap; open the deck again';

class Instance {
  dead = false;
  /** the live handles (decks, views, shows, timelines) of this instance */
  handles = 0;
  readonly uo: SlidesBinding;
  readonly exports: WebAssembly.Exports;
  constructor(uo: SlidesBinding, exports: WebAssembly.Exports) {
    this.uo = uo;
    this.exports = exports;
  }
}

let current: Instance | null = null;
let loading: Promise<Instance> | null = null;
let pptModule: Promise<{ convert(bytes: Uint8Array): Uint8Array }> | null = null;

const fetchBytes = async (src: Bytes): Promise<Uint8Array> => {
  const r = await src;
  if (r instanceof Uint8Array) return r;
  if (r instanceof ArrayBuffer) return new Uint8Array(r);
  const res = typeof r === 'string' || r instanceof URL ? await fetch(r) : r;
  if (!res.ok) throw new Error(`${res.url || 'wasm'}: HTTP ${res.status}`);
  return new Uint8Array(await res.arrayBuffer());
};

function kill(inst: Instance): void {
  inst.dead = true;
  inst.handles = 0;
  if (current === inst) { current = null; loading = null; }
}

function call<T>(inst: Instance, f: () => T): T {
  if (inst.dead) throw new Error(STOPPED);
  try {
    return f();
  } catch (e) {
    if (e instanceof WebAssembly.RuntimeError) kill(inst);
    throw e;
  }
}

/** Loads (once per page) the Slides module. */
export function loadSlidesModule(opts: LoadSlidesOptions): Promise<SlidesModule> {
  if (current && !current.dead) return Promise.resolve(new SlidesModule(current, opts));
  loading ??= (async () => {
    if (opts.wasm === undefined) throw new Error('loadSlidesModule: no wasm source');
    const { instance } = await WebAssembly.instantiate((await fetchBytes(opts.wasm)) as BufferSource, {});
    const inst = new Instance(bindSlides(instance.exports), instance.exports);
    current = inst;
    return inst;
  })().catch((e) => { loading = null; throw e; });
  return loading.then((inst) => new SlidesModule(inst, opts));
}

/** Whether a module is instantiated (the tests of "a viewer without engine=rust never loads it"). */
export function slidesModuleLoaded(): boolean { return current !== null && !current.dead; }

/** The live handles of the current instance (the M4 tests: 0 after everything was released). */
export function slidesHandles(): number { return current?.handles ?? 0; }

/** The live bytes of the module (a build with the `count` feature), or -1. */
export function slidesLiveBytes(): number { return current ? current.uo.liveBytes() : -1; }

/** The size of the module's linear memory in bytes (it never shrinks), or 0. */
export function slidesMemoryBytes(): number { return current ? (current.exports.memory as WebAssembly.Memory).buffer.byteLength : 0; }

/** Test-only (a `count` build): executes `unreachable` in the module. */
export function __trapForTest(): void {
  const inst = current;
  if (!inst) throw new Error('no instance');
  call(inst, () => (inst.exports.uo_test_trap as () => void)());
}

const isCfb = (b: Uint8Array): boolean => b.length > 3 && b[0] === 0xd0 && b[1] === 0xcf && b[2] === 0x11 && b[3] === 0xe0;

/** What the host gives a paint: the canvas that measures, the font stack, the decoded pictures and the light tables. */
export interface SlidesPaintHost {
  /** the CSS font-family stack of a face (the viewer's `fontStackFor`) */
  fontStack(family: string, bold: boolean, italic: boolean): string;
  /** the decoded picture of a package part, or null while it decodes / when it cannot */
  image(part: string): PaintImage | null | undefined;
  /** the context `measureText` runs on (a scratch canvas: its `font`, `fontKerning` and `letterSpacing` are changed) */
  measureCtx: CanvasRenderingContext2D | OffscreenCanvasRenderingContext2D;
  /** the optional painter hooks (`SlidesEnv`: the light tables of the bevel, the pattern rows, the canvas factory) */
  extra?: Pick<SlidesEnv, 'light' | 'patternRows' | 'createCanvas'>;
}

/** The status of a paint that left the slide to the TS painter because it has charts (the Rust list has no chart items yet). */
export const CHARTS_LEFT = 15;

/** Options of a paint. */
export interface SlidePaintOptions {
  /** do not paint a slide with chart frames: return `CHARTS_LEFT` and let the caller use the TS painter */
  skipCharts?: boolean;
}

/** B4b: options of a playback frame (`paintStage`). */
export interface StagePaintOptions {
  /** wire the media frames (`hasMedia` of `paintPlaybackSlide`: off for a frozen transition origin) */
  media?: boolean;
  /** the media clock of `showWhenStopped` (`MediaClock`: `{}` live; the video export's latency and scale) */
  clock?: { latencyMs?: number; scale?: number };
  /** the current video/GIF frame of shape `i` (the viewer's media player), or null for the poster */
  mediaFrame?(shape: number): { image: PaintImage; alpha: number } | null | undefined;
}

/** The result of a paint. */
export interface SlidePaintResult {
  /** 0 painted; else the wasm status (3: the slide could not be read, 6: no such slide ...) */
  status: number;
  /** chart frames the Rust list does not paint (shape index, box in px) */
  charts: number;
  /** measurement rounds the emission took (1 = the table had every answer) */
  rounds: number;
  /** B4b (`paintStage`): nothing is on that stage (the end screen, no transition origin); nothing was painted */
  empty?: boolean;
  /** B4b (`paintStage`): the frame's animation time and the shapes it re-emitted (colours, fonts, font scale) */
  animMs?: number;
  replaced?: number;
}

/** The binding handle behind a show or timeline handle of this module (`deck.stage` takes it). */
const RAW = new WeakMap<object, SlidesShow | SlidesTimeline>();

export class SlidesModule {
  readonly #inst: Instance;
  readonly #opts: LoadSlidesOptions;
  constructor(inst: Instance, opts: LoadSlidesOptions) {
    this.#inst = inst;
    this.#opts = opts;
  }

  get maxRounds(): number { return this.#inst.uo.maxRounds; }

  /**
   * Opens a deck (`.pptx`, or a `.ppt` through the lazily loaded converter module of `opts.ppt`). Throws `SlidesError`;
   * a corrupt file throws and nothing stays open.
   */
  async open(bytes: Uint8Array): Promise<SlidesDocument> {
    let src = bytes;
    if (isCfb(bytes)) {
      if (this.#opts.ppt === undefined) throw new SlidesError(14, 'open', 'no .ppt converter module configured');
      const src2 = this.#opts.ppt;
      pptModule ??= fetchBytes(src2).then(async (b) => bindSlidesPpt((await WebAssembly.instantiate(b as BufferSource, {})).instance.exports));
      const conv = await pptModule.catch((e) => { pptModule = null; throw e; });
      src = conv.convert(bytes);
    }
    const deck = call(this.#inst, () => this.#inst.uo.open(src));
    return new SlidesDocument(this.#inst, deck);
  }
}

const MIN_VIEW_GEN = 1;

/** One open deck: the wasm deck, the list stream of the view, the decoder tables and the painter. */
export class SlidesDocument {
  readonly #inst: Instance;
  #deck: SlidesDeck;
  #view: SlidesView;
  #tables: Tables = newTables();
  #painter: SlidesPainter | null = null;
  #host: SlidesPaintHost | null = null;
  #gen = MIN_VIEW_GEN;
  #fontGen = 0;
  #freed = false;
  #capsDone = false;
  /** B4b: the decoded lists of the two stages of the view (the slide on stage, the transition origin) and the frame stream */
  #stage: [SlidesList | null, SlidesList | null] = [null, null];
  #slotTables: SlotTables = newSlotTables();
  #mediaFrame: StagePaintOptions['mediaFrame'] | undefined = undefined;
  /** package part -> media ids whose size a request asked for (to answer them when the image finishes decoding) */
  readonly #sizeAsked = new Map<string, Set<number>>();
  readonly count: number;
  readonly slideW: number;
  readonly slideH: number;

  constructor(inst: Instance, deck: SlidesDeck) {
    this.#inst = inst;
    this.#deck = deck;
    const sz = deck.size();
    this.count = sz.count;
    this.slideW = sz.w;
    this.slideH = sz.h;
    this.#view = inst.uo.newView(this.#gen);
    inst.handles += 2;
  }

  get freed(): boolean { return this.#freed || this.#inst.dead; }

  #live(): SlidesDeck {
    if (this.#freed) throw new Error('uo-wasm: the Slides document was freed');
    if (this.#inst.dead) throw new Error(STOPPED);
    return this.#deck;
  }

  notes(i: number): string { return call(this.#inst, () => this.#live().notes(i)); }
  part(name: string): Uint8Array | null { return call(this.#inst, () => this.#live().part(name)); }

  /** A font loaded (or the font stack answers changed): every advance may change, the layout is asked again. */
  fontsChanged(): void {
    if (this.freed) return;
    call(this.#inst, () => this.#live().setGeneration(++this.#fontGen));
    this.#clearStage();
  }

  /** The stages are laid out with the old answers: the next frame emits them again. */
  #clearStage(): void {
    if (this.freed) return;
    call(this.#inst, () => this.#view.clearStage());
    this.#stage = [null, null];
  }

  /** The image of `part` finished decoding: the layout that was given 0 x 0 for it learns its size. */
  imageReady(part: string, img: PaintImage): void {
    if (this.freed) return;
    const ids = this.#sizeAsked.get(part);
    if (!ids) return;
    const w = imgW(img), h = imgH(img);
    call(this.#inst, () => { for (const id of ids) this.#live().setImageSize(id, w, h); });
    this.#sizeAsked.delete(part);
    this.#clearStage();
  }

  /** Starts a new generation of the list stream (the caches overflowed, or a decode failed and the stream is unusable). */
  #rotate(): void {
    this.#view.free();
    this.#view = this.#inst.uo.newView(++this.#gen);
    this.#tables = newTables();
    this.#stage = [null, null];
    this.#slotTables = newSlotTables();
    this.#painter?.clear();
  }

  #answers(reqs: MeasureRequest[], host: SlidesPaintHost): Float64Array {
    const ctx = host.measureCtx as CanvasRenderingContext2D & { fontKerning?: string; letterSpacing?: string };
    const kern = ctx.fontKerning !== undefined;
    const stack = (f: { family: string; bold: boolean; italic: boolean }): string => host.fontStack(f.family, f.bold, f.italic);
    const font = (f: { family: string; bold: boolean; italic: boolean }, px: number): string => `${f.italic ? 'italic ' : ''}${f.bold ? '700 ' : '400 '}${px}px ${stack(f)}`;
    const out: number[] = [];
    for (const r of reqs) {
      if (r.kind === 'charAt1000') {
        ctx.font = font(r.face, 1000);
        const prev = ctx.fontKerning;
        if (kern) ctx.fontKerning = 'none';
        out.push(ctx.measureText(r.ch).width);
        if (kern) ctx.fontKerning = prev;
      } else if (r.kind === 'kernAt1000') {
        ctx.font = font(r.face, 1000);
        const prev = ctx.fontKerning;
        if (kern) ctx.fontKerning = 'none';
        const w0 = ctx.measureText(r.text).width;
        if (kern) ctx.fontKerning = 'normal';
        const w1 = ctx.measureText(r.text).width;
        if (kern) ctx.fontKerning = prev;
        out.push(w0, w1);
      } else if (r.kind === 'width' || r.kind === 'metrics') {
        // a chart question before any font was set on its canvas (CH-5): the canvas default, whatever the size asked
        ctx.font = r.face.family === '' ? '10px sans-serif' : font(r.face, r.sizePx);
        const prevK = ctx.fontKerning, prevL = ctx.letterSpacing;
        if (kern && r.kerning !== 'current') ctx.fontKerning = r.kerning;
        if (prevL !== undefined) ctx.letterSpacing = `${r.letterSpacingPx}px`;
        const m = ctx.measureText(r.text);
        if (kern && r.kerning !== 'current') ctx.fontKerning = prevK;
        if (prevL !== undefined) ctx.letterSpacing = prevL;
        if (r.kind === 'width') out.push(m.width);
        else {
          const fa = (m as { fontBoundingBoxAscent?: number }).fontBoundingBoxAscent;
          const fd = (m as { fontBoundingBoxDescent?: number }).fontBoundingBoxDescent;
          out.push(m.width, m.actualBoundingBoxLeft, m.actualBoundingBoxRight, m.actualBoundingBoxAscent, m.actualBoundingBoxDescent, fa ?? NaN, fd ?? NaN);
        }
      } else if (r.kind === 'metaText') {
        // a metafile player's probe under a raw CSS font string: the width and the font bounding box
        ctx.font = r.font;
        const m = ctx.measureText(r.text);
        out.push(m.width, (m as { fontBoundingBoxAscent?: number }).fontBoundingBoxAscent ?? NaN, (m as { fontBoundingBoxDescent?: number }).fontBoundingBoxDescent ?? NaN);
      } else if (r.kind === 'metaDecode') {
        // decoding a compressed EMF+ bitmap is asynchronous in a browser: answered "cannot be decoded" in this part (listed gap)
        out.push(NaN, NaN);
      } else if (r.kind === 'fontAvailable') {
        ctx.font = `40px ${r.family}`;
        const a = ctx.measureText('mmmmmmmmmmlli').width;
        ctx.font = '40px monospace';
        out.push(a !== ctx.measureText('mmmmmmmmmmlli').width ? 1 : 0);
      } else {
        // an image size: the decoded picture's, or 0 x 0 while it decodes (`imageReady` tells the layout later)
        const img = host.image(r.part);
        if (img) out.push(imgW(img), imgH(img));
        else {
          out.push(0, 0);
          let s = this.#sizeAsked.get(r.part);
          if (!s) this.#sizeAsked.set(r.part, s = new Set());
          s.add(r.media);
        }
      }
    }
    return Float64Array.from(out);
  }

  #painterFor(host: SlidesPaintHost): SlidesPainter {
    if (!this.#painter || this.#host !== host) {
      const tables = (): Tables => this.#tables;
      const env: SlidesEnv = {
        fontStack: (f, b, i) => host.fontStack(f, b, i),
        media: (index) => {
          const m = tables().media[index];
          return m ? host.image(m.part) : null;
        },
        bitmap: (index) => {
          const b = tables().bitmaps[index];
          return b ? this.#view.bitmap(b.data) : null;
        },
        mediaFrame: (shape) => this.#mediaFrame?.(shape),
        ...host.extra,
      };
      this.#host = host;
      this.#painter = new SlidesPainter(env);
    }
    return this.#painter;
  }

  /**
   * Paints slide `index` into `ctx` at `scale` px per EMU, origin `(ox, oy)` px: emit (the measure loop), decode, interpret. The
   * context's transform is the caller's (the TS `paintSlideTo` is called under the same transform).
   */
  paint(ctx: CanvasRenderingContext2D, index: number, scale: number, ox: number, oy: number, host: SlidesPaintHost, o: SlidePaintOptions = {}): SlidePaintResult {
    const deck = this.#live();
    this.#caps(deck, host);
    const painter = this.#painterFor(host);
    const max = this.#inst.uo.maxRounds;
    for (let round = 1; round <= max + 1; round++) {
      const r = call(this.#inst, () => deck.emit(this.#view, index, scale, ox, oy));
      if (r.status === 12 && 'requests' in r) {
        if (round > max) return { status: 12, charts: 0, rounds: round };
        const nums = this.#answers(r.requests, host);
        call(this.#inst, () => deck.answer(nums));
        continue;
      }
      if (r.status !== 0 || !('message' in r)) return { status: r.status, charts: 0, rounds: round };
      let list: SlidesList;
      try {
        list = decodeSlidesList(r.message, this.#tables);
      } catch (e) {
        // the encoder advanced its marks but the decoder did not: the stream is unusable, start a new generation
        this.#rotate();
        throw e;
      }
      // charts are painted by the list since C-6 (`uo-charts`), so the frames list is empty; an older module that still reports
      // frames, with `skipCharts`, leaves the slide to the caller's TS painter (the list is decoded all the same: the stream of tables stays in step)
      const left = o.skipCharts === true && r.charts.length > 0;
      if (!left) painter.paint(ctx, list, this.#tables, undefined);
      if (r.newGeneration) this.#rotate();
      return { status: left ? CHARTS_LEFT : 0, charts: r.charts.length, rounds: round };
    }
    return { status: 12, charts: 0, rounds: max + 1 };
  }

  /** The canvas capabilities change the layout (the TS tests each at every site): the table keeps them (set once). */
  #caps(deck: SlidesDeck, host: SlidesPaintHost): void {
    if (this.#capsDone) return;
    const m = host.measureCtx;
    call(this.#inst, () => deck.setCaps({
      fontKerning: 'fontKerning' in m,
      letterSpacing: 'letterSpacing' in m,
      segmenter: typeof (Intl as { Segmenter?: unknown }).Segmenter === 'function',
      fontBoundingBox: typeof TextMetrics !== 'undefined' && 'fontBoundingBoxAscent' in TextMetrics.prototype,
    }));
    this.#capsDone = true;
  }

  /**
   * B4b: paints a playback frame — `src` a show handle at clock `at` (= now) or a timeline handle at `at` (= t); `which` 0 = the
   * slide on stage, 1 = the origin of the running transition (frozen at its animation time). The stage's list (with slots) is
   * emitted when the slide, the viewport or the hooks' structure changes; every frame evaluates the slots in Rust and the
   * painter applies the values (and paints the shapes the frame re-emitted).
   */
  paintStage(ctx: CanvasRenderingContext2D, src: SlidesShowHandle | SlidesTimelineHandle, at: number, which: 0 | 1, scale: number, ox: number, oy: number,
    host: SlidesPaintHost, o: StagePaintOptions = {}): SlidePaintResult {
    const deck = this.#live();
    const raw = RAW.get(src);
    if (!raw) throw new Error('uo-wasm: not a handle of this document');
    this.#caps(deck, host);
    const painter = this.#painterFor(host);
    const max = this.#inst.uo.maxRounds;
    for (let round = 1; round <= max + 1; round++) {
      const r = call(this.#inst, () => deck.stage(this.#view, raw, at, which, scale, ox, oy, o.media === true, o.clock ?? {}));
      if (r.status === 12 && 'requests' in r) {
        if (round > max) return { status: 12, charts: 0, rounds: round };
        const nums = this.#answers(r.requests, host);
        call(this.#inst, () => deck.answer(nums));
        continue;
      }
      if (r.status !== 0 || !('animMs' in r)) return { status: r.status, charts: 0, rounds: round };
      if (r.empty) {
        this.#stage[which] = null;
        return { status: 0, charts: 0, rounds: round, empty: true, animMs: r.animMs };
      }
      const replacements = new Map<number, SlotReplacement>();
      let frame;
      try {
        if (r.list) this.#stage[which] = decodeSlidesList(r.list, this.#tables);
        frame = decodeSlotFrame(r.frame, this.#slotTables);
        for (const rep of frame.replacements) replacements.set(rep.shape, { list: decodeSlidesList(rep.list, this.#tables), values: rep.values });
      } catch (e) {
        // the encoder advanced its marks but the decoder did not: the stream is unusable, start a new generation
        this.#rotate();
        throw e;
      }
      const list = this.#stage[which];
      if (!list) return { status: 10, charts: 0, rounds: round };
      this.#mediaFrame = o.mediaFrame;
      try {
        painter.paint(ctx, list, this.#tables, frame, replacements);
      } finally {
        this.#mediaFrame = undefined;
      }
      if (r.newGeneration) this.#rotate();
      return { status: 0, charts: 0, rounds: round, animMs: r.animMs, replaced: replacements.size };
    }
    return { status: 12, charts: 0, rounds: max + 1 };
  }

  /** Drops what a long session accumulated (the deck's caches, the painter's canvases). */

  clearCaches(): void {
    if (this.freed) return;
    call(this.#inst, () => this.#live().clearCaches());
    this.#painter?.clear();
  }

  /** A click slide show over the deck (`from` -1 = the first visible slide). */
  newShow(o?: Parameters<SlidesDeck['newShow']>[0]): SlidesShowHandle {
    const s = call(this.#inst, () => this.#live().newShow(o));
    this.#inst.handles++;
    return new SlidesShowHandle(this.#inst, s);
  }

  newTimeline(o?: Parameters<SlidesDeck['newTimeline']>[0]): SlidesTimelineHandle {
    const t = call(this.#inst, () => this.#live().newTimeline(o));
    this.#inst.handles++;
    return new SlidesTimelineHandle(this.#inst, t);
  }

  /** Sizes of the painter's caches (the memory tests). */
  cacheSizes(): { derived: number; bitmaps: number } { return this.#painter?.cacheSizes() ?? { derived: 0, bitmaps: 0 }; }

  /** The generation of the list stream (it grows when the caches overflow). */
  get generation(): number { return this.#gen; }

  free(): void {
    if (this.#freed) return;
    this.#freed = true;
    this.#painter?.clear();
    if (!this.#inst.dead) {
      this.#view.free();
      this.#deck.free();
      this.#inst.handles = Math.max(0, this.#inst.handles - 2);
    }
  }
}

export class SlidesShowHandle {
  readonly #inst: Instance;
  readonly #s: SlidesShow;
  #freed = false;
  constructor(inst: Instance, s: SlidesShow) { this.#inst = inst; this.#s = s; RAW.set(this, s); }
  next(now: number, mouse = false): ShowAction { return call(this.#inst, () => this.#s.next(now, mouse)); }
  prev(now: number): ShowAction { return call(this.#inst, () => this.#s.prev(now)); }
  goTo(i: number, now: number): ShowAction { return call(this.#inst, () => this.#s.goTo(i, now)); }
  first(now: number): ShowAction { return call(this.#inst, () => this.#s.first(now)); }
  last(now: number): ShowAction { return call(this.#inst, () => this.#s.last(now)); }
  update(now: number): ShowAction { return call(this.#inst, () => this.#s.update(now)); }
  dueAt(): number { return call(this.#inst, () => this.#s.dueAt()); }
  wakeAt(now: number): number { return call(this.#inst, () => this.#s.wakeAt(now)); }
  frame(now: number): ShowFrame { return call(this.#inst, () => this.#s.frame(now)); }
  free(): void {
    if (this.#freed) return;
    this.#freed = true;
    if (!this.#inst.dead) { this.#s.free(); this.#inst.handles = Math.max(0, this.#inst.handles - 1); }
  }
}

export class SlidesTimelineHandle {
  readonly #inst: Instance;
  readonly #t: SlidesTimeline;
  #freed = false;
  constructor(inst: Instance, t: SlidesTimeline) { this.#inst = inst; this.#t = t; RAW.set(this, t); }
  info(): ReturnType<SlidesTimeline['info']> { return call(this.#inst, () => this.#t.info()); }
  stateAt(t: number): ReturnType<SlidesTimeline['stateAt']> { return call(this.#inst, () => this.#t.stateAt(t)); }
  free(): void {
    if (this.#freed) return;
    this.#freed = true;
    if (!this.#inst.dead) { this.#t.free(); this.#inst.handles = Math.max(0, this.#inst.handles - 1); }
  }
}
