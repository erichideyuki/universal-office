// The JS side of `<universal-sheet engine="rust">` (read path, frontier C4; record core/docs/modules/uo-xlsx.md "C4 — as built"):
// the module loader (`uo-sheet.wasm`, once per page), the workbook handle (`SheetBook`) and the painter that replays a viewport
// message (Slides list stream with `SheetFont`/`GlyphRun`) on the element's canvas. Typed for the component by the ambient
// `sheet/packages/sheet/src/sheet-engine.d.ts` (alias `@universal-office/sheet-engine`). Erasable TypeScript only.
import { bindSheet, SheetError } from './sheet.mjs';
import { decodeSlidesList, newTables } from './gen/slides.ts';
import type { Tables } from './gen/slides.ts';
import { SlidesPainter } from './paint-slides.ts';

export { SheetError };

type Bytes = Uint8Array | ArrayBuffer | string | URL | Response | Promise<Uint8Array | ArrayBuffer | Response>;

export interface LoadSheetOptions { wasm?: Bytes }

/** What the host (the component) gives the painter and the measurement loop. */
export interface SheetHost {
  /** the CSS font stack of a family (`FontResolver.cssStack` with the Sheet fallbacks); `family` may be `name|altName` */
  fontStack(family: string, bold: boolean, italic: boolean): string;
  /** a canvas 2D context to measure with (never the painting one) */
  measureContext(): CanvasRenderingContext2D | OffscreenCanvasRenderingContext2D;
}

interface FontDesc { sizePt: number; zoom: number; ppem: number; bold: boolean; italic: boolean; name: string; alt: string | null; metric: string }

async function fetchBytes(src: Bytes): Promise<Uint8Array | ArrayBuffer> {
  const s = await src;
  if (typeof s === 'string' || s instanceof URL) return new Uint8Array(await (await fetch(s)).arrayBuffer());
  if (typeof Response !== 'undefined' && s instanceof Response) return new Uint8Array(await s.arrayBuffer());
  return s;
}

type Uo = ReturnType<typeof bindSheet>;
let current: Uo | null = null;
let loading: Promise<Uo> | null = null;

/** Loads (once per page) the Sheet module. */
export function loadSheetModule(opts: LoadSheetOptions): Promise<SheetModule> {
  if (current) return Promise.resolve(new SheetModule(current));
  loading ??= (async () => {
    if (opts.wasm === undefined) throw new Error('loadSheetModule: no wasm source');
    const { instance } = await WebAssembly.instantiate((await fetchBytes(opts.wasm)) as BufferSource, {});
    current = bindSheet(instance.exports);
    return current;
  })().catch((e) => { loading = null; throw e; });
  return loading.then((u) => new SheetModule(u));
}

/** The module when it is already instantiated (the workbook then opens synchronously), else null. */
export function currentSheetModule(): SheetModule | null { return current ? new SheetModule(current) : null; }

export class SheetModule {
  #uo: Uo;
  constructor(uo: Uo) { this.#uo = uo; }
  open(bytes: Uint8Array): SheetBook { return new SheetBook(this.#uo.open(bytes)); }
}

type Book = ReturnType<Uo['open']>;

/** One workbook in the module: the sheets, the active sheet's frames, the stream decoder and the painter. */
export class SheetBook {
  #b: Book;
  #tables: Tables = newTables();
  #painter: SlidesPainter | null = null;
  #host: SheetHost | null = null;
  #fonts = new Map<string, number>();
  constructor(b: Book) { this.#b = b; }
  get freed(): boolean { return this.#b.freed; }
  free(): void { this.#b.free(); }
  sheets(): { name: string; kind: 'sheet' | 'chart' | 'none'; state: number; active: boolean }[] { return this.#b.sheets(); }
  recalcPlan(): 'none' | 'partial' | 'full' { return this.#b.recalcPlan(); }
  extent(): { width: number; height: number } | null { return this.#b.extent(); }
  linkAt(sheet: number, col: number, row: number): string | null { return this.#b.linkAt(sheet, col, row); }
  setAccent(css: string): void { this.#b.setAccent(css); }
  setSpill(c1: number, r1: number, c2: number, r2: number): void { this.#b.setSpill(c1, r1, c2, r2); }

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

  activate(index: number, host: SheetHost): void { this.#b.activate(index, this.#measurer(host)); }

  #painterFor(host: SheetHost): SlidesPainter {
    if (!this.#painter || this.#host !== host) {
      this.#host = host;
      this.#painter = new SlidesPainter({
        fontStack: (f, b, i) => host.fontStack(f, b, i),
        media: () => null,
        bitmap: (index) => { const t = this.#tables.bitmaps[index]; return t ? this.#b.bitmap(t.data) : null; },
      });
    }
    return this.#painter;
  }

  /** Paints one frame of sheet `q.sheet` on `ctx` (whose transform is the caller's identity). */
  paint(ctx: CanvasRenderingContext2D, q: { sheet: number; width: number; height: number; dpr?: number; scrollX: number; scrollY: number; zoom: number; selection: number[]; capture?: boolean; editing?: [number, number] }, host: SheetHost): void {
    const bytes = this.#b.viewport(q, this.#measurer(host));
    const list = decodeSlidesList(bytes, this.#tables);
    this.#painterFor(host).paint(ctx, list, this.#tables, undefined);
  }
}
