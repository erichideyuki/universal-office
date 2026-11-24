// The JS side of `<universal-sheet engine="rust">` (read path, frontiers C4 and C4b; record core/docs/modules/uo-xlsx.md "C4 — as built"
// and "C4b — as built"): the module loader (`uo-sheet.wasm`, once per page; `uo-sheet-calc.wasm`, the same viewer with the calc
// engine, loaded only for a workbook whose open-recalc plan is not `none`), the workbook handle (`SheetBook`) and the painter that
// replays a viewport message (Slides list stream with `SheetFont`/`GlyphRun` and the drawings' items) on the element's canvas.
// Typed for the component by the ambient `sheet/packages/sheet/src/sheet-engine.d.ts` (alias `@universal-office/sheet-engine`).
// Erasable TypeScript only.
import { bindSheet, SheetError } from './sheet.mjs';
import { decodeSlidesList, newTables } from './gen/slides.ts';
import type { Tables } from './gen/slides.ts';
import { SlidesPainter } from './paint-slides.ts';
import type { PaintImage } from './paint-slides.ts';

export { SheetError };

type Bytes = Uint8Array | ArrayBuffer | string | URL | Response | Promise<Uint8Array | ArrayBuffer | Response>;

/** `wasm`: `uo-sheet.wasm`; `calcWasm`: `uo-sheet-calc.wasm` (the same viewer with the calc; absent = the values of the file). */
export interface LoadSheetOptions { wasm?: Bytes; calcWasm?: Bytes }

/** What the host (the component) gives the painter and the measurement loop. */
export interface SheetHost {
  /** the CSS font stack of a cell family (`FontResolver.cssStack` with the Sheet fallbacks); `family` may be `name|altName` */
  fontStack(family: string, bold: boolean, italic: boolean): string;
  /** the CSS font stack of a shape's text (`shapeFontStack` of the TS element: the shape fallbacks); default `fontStack` */
  shapeFontStack?(family: string, bold: boolean, italic: boolean): string;
  /** a canvas 2D context to measure with (never the painting one) */
  measureContext(): CanvasRenderingContext2D | OffscreenCanvasRenderingContext2D;
  /** the family whose GDI widths a font name takes: the name, or `arial` when the browser fell back to the default substitute (the
   * TS `cellFont`); default: the name */
  metricFamily?(name: string): string;
  /** something the frame needed arrived (a decoded picture, the calc module): paint again */
  invalidate?(): void;
}

interface FontDesc { sizePt: number; zoom: number; ppem: number; bold: boolean; italic: boolean; name: string; alt: string | null; metric: string }

interface Face { family: string; bold: boolean; italic: boolean }
type Req =
  | { kind: 'charAt1000'; face: Face; ch: string }
  | { kind: 'kernAt1000'; face: Face; text: string }
  | { kind: 'width' | 'metrics'; face: Face; sizePx: number; kerning: string; letterSpacingPx: number; text: string }
  | { kind: 'fontAvailable'; family: string }
  | { kind: 'imageSize'; media: number; part: string }
  | { kind: 'metaText'; font: string; text: string }
  | { kind: 'metaDecode'; key: string; data: Uint8Array };

/** The face tag of the charts (uo-xlsx `CHART_FACE_TAG`): their stack is `paintChart`'s, not the shapes'. */
const CHART_TAG = '\u0001';

async function fetchBytes(src: Bytes): Promise<Uint8Array | ArrayBuffer> {
  const s = await src;
  if (typeof s === 'string' || s instanceof URL) return new Uint8Array(await (await fetch(s)).arrayBuffer());
  if (typeof Response !== 'undefined' && s instanceof Response) return new Uint8Array(await s.arrayBuffer());
  return s;
}

type Uo = ReturnType<typeof bindSheet>;
let current: Uo | null = null;
let loading: Promise<Uo> | null = null;
let calcSource: Bytes | undefined;
let calc: Uo | null = null;
let calcLoading: Promise<Uo | null> | null = null;

async function instantiate(src: Bytes): Promise<Uo> {
  const { instance } = await WebAssembly.instantiate((await fetchBytes(src)) as BufferSource, {});
  return bindSheet(instance.exports);
}

/** Loads (once per page) the Sheet module; the calc module is only remembered (loaded on first need). */
export function loadSheetModule(opts: LoadSheetOptions): Promise<SheetModule> {
  if (opts.calcWasm !== undefined) calcSource ??= opts.calcWasm;
  if (current) return Promise.resolve(new SheetModule(current));
  loading ??= (async () => {
    if (opts.wasm === undefined) throw new Error('loadSheetModule: no wasm source');
    current = await instantiate(opts.wasm);
    return current;
  })().catch((e) => { loading = null; throw e; });
  return loading.then((u) => new SheetModule(u));
}

/** The calc module (`uo-sheet-calc.wasm`), loaded once on first need; null without a source or when it fails to load. */
function calcModule(): Promise<Uo | null> {
  if (calc) return Promise.resolve(calc);
  if (calcSource === undefined) return Promise.resolve(null);
  calcLoading ??= instantiate(calcSource).then((u) => (calc = u)).catch(() => null);
  return calcLoading;
}

/** The module when it is already instantiated (the workbook then opens synchronously), else null. */
export function currentSheetModule(): SheetModule | null { return current ? new SheetModule(current) : null; }

export class SheetModule {
  #uo: Uo;
  constructor(uo: Uo) { this.#uo = uo; }
  open(bytes: Uint8Array): SheetBook { return new SheetBook(this.#uo, bytes); }
}

type Book = ReturnType<Uo['open']>;

/** One workbook in the module: the sheets, the active sheet's frames, the stream decoder, the painter and the pictures. */
export class SheetBook {
  #b: Book;
  #tables: Tables = newTables();
  #painter: SlidesPainter | null = null;
  #host: SheetHost | null = null;
  #active = -1;
  #images = new Map<number, PaintImage | null>();
  #sizes = new Map<number, [number, number]>();
  #decoding = new Set<number>();
  #blobs = new Map<string, ImageBitmap | null>();

  constructor(uo: Uo, bytes: Uint8Array) {
    this.#b = uo.open(bytes);
    // the open-recalc needs the calc: the workbook moves to the calc module once it is here (the file's values are shown meanwhile)
    if (!uo.hasCalc() && this.#b.recalcPlan() !== 'none') {
      const copy = bytes.slice();
      void calcModule().then((c) => {
        if (!c || this.#b.freed) return;
        let nb: Book;
        try { nb = c.open(copy); } catch { return; }
        this.#b.free();
        this.#b = nb;
        this.#tables = newTables();
        this.#painter = null;
        this.#images.clear();
        this.#sizes.clear();
        this.#metricsSet = false;
        const a = this.#active;
        this.#active = -1;
        if (a >= 0 && this.#host) { try { this.activate(a, this.#host); } catch { /* the next paint activates */ } }
        this.#host?.invalidate?.();
      });
    }
  }
  get freed(): boolean { return this.#b.freed; }
  free(): void { this.#b.free(); }
  sheets(): { name: string; kind: 'sheet' | 'chart' | 'none'; state: number; active: boolean }[] { return this.#b.sheets(); }
  recalcPlan(): 'none' | 'partial' | 'full' { return this.#b.recalcPlan(); }
  extent(): { width: number; height: number } | null { return this.#b.extent(); }
  linkAt(sheet: number, col: number, row: number): string | null { return this.#b.linkAt(sheet, col, row); }
  setAccent(css: string): void { this.#b.setAccent(css); }
  setSpill(c1: number, r1: number, c2: number, r2: number): void { this.#b.setSpill(c1, r1, c2, r2); }
  /** The drawings and charts with the frozen TS rules (`true`; the parity harnesses) or the Office-measured ones (default). */
  setTsRules(on: boolean): void { this.#b.setTsRules(on); }

  /** The measurer of a host: the browser's `measureText` with the font of the module's table (the CellFont string). */
  #measurer(host: SheetHost) {
    const ctx = host.measureContext();
    const css = (f: FontDesc): string => {
      const alt = f.alt ? `${f.name}|${f.alt}` : f.name;
      return `${f.italic ? 'italic ' : ''}${f.bold ? '700 ' : '400 '}${f.ppem}px ${host.fontStack(alt, f.bold, f.italic)}`;
    };
    const use = (f: FontDesc): void => { const c = css(f); if (ctx.font !== c) ctx.font = c; };
    return {
      advance: (f: FontDesc, cp: number): number => { use(f); return Math.round(ctx.measureText(String.fromCodePoint(cp)).width); },
      width: (f: FontDesc, t: string): number => { use(f); return Math.round(ctx.measureText(t).width); },
      box: (f: FontDesc): [number, number] | null => {
        use(f);
        const m = ctx.measureText('Hg');
        return [m.fontBoundingBoxAscent ?? NaN, m.fontBoundingBoxDescent ?? NaN];
      },
    };
  }

  /** The stack of a drawing face: a chart face (tagged) takes `paintChart`'s stack, a shape face the host's shape stack. */
  static #stack(host: SheetHost, family: string, bold: boolean, italic: boolean): string {
    if (family.startsWith(CHART_TAG)) return `'${family.slice(1)}', Calibri, 'Liberation Sans', Arial, sans-serif`;
    return (host.shapeFontStack ?? host.fontStack)(family, bold, italic);
  }

  /** The decoded picture of `tables.media[index]` (null while it decodes or when it cannot be decoded). */
  #image(index: number, host: SheetHost): PaintImage | null {
    const hit = this.#images.get(index);
    if (hit !== undefined) return hit;
    this.#decode(index, host);
    return null;
  }

  #decode(index: number, host: SheetHost): void {
    if (this.#decoding.has(index) || this.#images.has(index)) return;
    const m = this.#b.media(index);
    if (!m || typeof createImageBitmap !== 'function') { this.#images.set(index, null); this.#sizes.set(index, [0, 0]); return; }
    this.#decoding.add(index);
    const ext = (m.part.split('.').pop() ?? '').toLowerCase();
    const type = ext === 'jpg' || ext === 'jpeg' ? 'image/jpeg' : ext === 'svg' ? 'image/svg+xml' : `image/${ext}`;
    void createImageBitmap(new Blob([m.bytes], { type })).then(
      (img) => { this.#images.set(index, img as unknown as PaintImage); this.#sizes.set(index, [img.width, img.height]); },
      () => { this.#images.set(index, null); this.#sizes.set(index, [0, 0]); },
    ).finally(() => {
      this.#decoding.delete(index);
      if (this.#b.freed) return;
      const s = this.#sizes.get(index) ?? [0, 0];
      this.#b.setImageSize(index, s[0], s[1]);
      host.invalidate?.();
    });
  }

  /** The answers of the drawings' requests (the Slides module's rules: `slides-engine.ts` `#answers`). */
  #answers(reqs: Req[], host: SheetHost): number[] {
    const ctx = host.measureContext() as CanvasRenderingContext2D & { fontKerning?: string; letterSpacing?: string };
    const kern = ctx.fontKerning !== undefined;
    const font = (f: Face, px: number): string => `${f.italic ? 'italic ' : ''}${f.bold ? '700 ' : '400 '}${px}px ${SheetBook.#stack(host, f.family, f.bold, f.italic)}`;
    const out: number[] = [];
    for (const r of reqs) {
      if (r.kind === 'charAt1000' || r.kind === 'kernAt1000') {
        ctx.font = font(r.face, 1000);
        const prev = ctx.fontKerning;
        if (kern) ctx.fontKerning = 'none';
        const w0 = ctx.measureText(r.kind === 'charAt1000' ? r.ch : r.text).width;
        if (r.kind === 'charAt1000') out.push(w0);
        else {
          if (kern) ctx.fontKerning = 'normal';
          out.push(w0, ctx.measureText(r.text).width);
        }
        if (kern) ctx.fontKerning = prev;
      } else if (r.kind === 'width' || r.kind === 'metrics') {
        // a chart question before any font was set on its canvas (CH-5): the canvas default
        ctx.font = r.face.family === '' ? '10px sans-serif' : font(r.face, r.sizePx);
        const prevK = ctx.fontKerning, prevL = ctx.letterSpacing;
        if (kern && r.kerning !== 'current') ctx.fontKerning = r.kerning;
        if (prevL !== undefined) ctx.letterSpacing = `${r.letterSpacingPx}px`;
        const m = ctx.measureText(r.text);
        if (kern && r.kerning !== 'current') ctx.fontKerning = prevK;
        if (prevL !== undefined) ctx.letterSpacing = prevL;
        if (r.kind === 'width') out.push(m.width);
        else out.push(m.width, m.actualBoundingBoxLeft, m.actualBoundingBoxRight, m.actualBoundingBoxAscent, m.actualBoundingBoxDescent, m.fontBoundingBoxAscent ?? NaN, m.fontBoundingBoxDescent ?? NaN);
      } else if (r.kind === 'metaText') {
        ctx.font = r.font;
        const m = ctx.measureText(r.text);
        out.push(m.width, m.fontBoundingBoxAscent ?? NaN, m.fontBoundingBoxDescent ?? NaN);
      } else if (r.kind === 'metaDecode') {
        // a compressed EMF+ bitmap: decoded asynchronously; NaN while it decodes or when it cannot be decoded
        const hit = this.#blobs.get(r.key);
        if (hit) out.push(hit.width, hit.height);
        else {
          out.push(NaN, NaN);
          if (hit === undefined && typeof createImageBitmap === 'function') {
            this.#blobs.set(r.key, null);
            void createImageBitmap(new Blob([r.data])).then((b) => { this.#blobs.set(r.key, b); host.invalidate?.(); }, () => {});
          }
        }
      } else if (r.kind === 'fontAvailable') {
        ctx.font = `40px ${r.family}`;
        const a = ctx.measureText('mmmmmmmmmmlli').width;
        ctx.font = '40px monospace';
        out.push(a !== ctx.measureText('mmmmmmmmmmlli').width ? 1 : 0);
      } else {
        // a picture's size: the decoded one, or 0 x 0 while it decodes (`setImageSize` tells the layout later)
        const s = this.#sizes.get(r.media);
        if (s) out.push(s[0], s[1]);
        else { out.push(0, 0); this.#decode(r.media, host); }
      }
    }
    return out;
  }

  #metricsSet = false;

  activate(index: number, host: SheetHost): void {
    this.#host = host;
    if (!this.#metricsSet && host.metricFamily) {
      this.#metricsSet = true;
      this.#b.setMetrics(this.#b.fontNames().map((n: string) => [n, host.metricFamily!(n)]));
    }
    this.#b.activate(index, this.#measurer(host));
    this.#active = index;
  }

  #painterFor(host: SheetHost): SlidesPainter {
    if (!this.#painter || this.#host !== host) {
      this.#host = host;
      this.#painter = new SlidesPainter({
        fontStack: (f, b, i) => SheetBook.#stack(host, f, b, i),
        cellFontStack: (f, b, i) => host.fontStack(f, b, i),
        media: (index) => this.#image(index, host),
        bitmap: (index) => { const t = this.#tables.bitmaps[index]; return t && !this.#b.isRecipe(t.data) ? this.#b.bitmap(t.data) : null; },
        recipe: (index) => { const t = this.#tables.bitmaps[index]; return t && this.#b.isRecipe(t.data) ? this.#b.bitmap(t.data) : null; },
        decodeBlob: (data) => this.#blobs.get(bitmapKeyOf(data)) ?? null,
      });
    }
    return this.#painter;
  }

  /** Paints one frame of sheet `q.sheet` on `ctx` (whose transform is the caller's identity). */
  paint(ctx: CanvasRenderingContext2D, q: { sheet: number; width: number; height: number; dpr?: number; scrollX: number; scrollY: number; zoom: number; selection: number[]; capture?: boolean; editing?: [number, number] }, host: SheetHost): void {
    if (this.#active !== q.sheet) this.activate(q.sheet, host);
    const bytes = this.#b.viewport(q, this.#measurer(host), (reqs: Req[]) => this.#answers(reqs, host));
    const list = decodeSlidesList(bytes, this.#tables);
    this.#painterFor(host).paint(ctx, list, this.#tables, undefined);
  }
}

/** `bitmapKey` of the metafile players (`length:fnv in base 36`, `slides-engine.ts`): the key of a compressed EMF+ bitmap. */
function bitmapKeyOf(d: Uint8Array): string {
  let h = 0x811c9dc5;
  const step = Math.max(1, Math.floor(d.length / 4096));
  for (let i = 0; i < d.length; i += step) { h ^= d[i]!; h = Math.imul(h, 16777619); }
  for (let i = Math.max(0, d.length - 64); i < d.length; i++) { h ^= d[i]!; h = Math.imul(h, 16777619); }
  return `${d.length}:${(h >>> 0).toString(36)}`;
}
