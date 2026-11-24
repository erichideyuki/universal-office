// The JS side of `<universal-sheet engine="rust">` EDITING (frontier C5b; record core/docs/modules/uo-xlsx.md "C5b — as built"):
// the loader of `uo-sheet-editor.wasm` (the viewer exports + `sheet-edit`, once per page), the editing session handle
// (`SheetEditBook`: commands, state, save) and the painter of its viewport (the same Slides list stream as the viewer's
// `SheetBook`). Typed for the component by the ambient `sheet/packages/sheet/src/sheet-engine.d.ts`
// (alias `@universal-office/sheet-editor-engine`). Erasable TypeScript only.
import { bindSheetEditor, SheetEditError } from './sheet-editor.mjs';
import { decodeSlidesList, newTables } from './gen/slides.ts';
import type { Tables } from './gen/slides.ts';
import { SlidesPainter } from './paint-slides.ts';

export { SheetEditError };

type Bytes = Uint8Array | ArrayBuffer | string | URL | Response | Promise<Uint8Array | ArrayBuffer | Response>;

export interface LoadSheetEditorOptions { wasm?: Bytes }

/** What the host (the component) gives the painter and the measurement loop (the viewer's `SheetHost`). */
export interface SheetHost {
  fontStack(family: string, bold: boolean, italic: boolean): string;
  measureContext(): CanvasRenderingContext2D | OffscreenCanvasRenderingContext2D;
}

/** The state of the session after a command (what the shell shows). */
export interface SheetEditState {
  sheet: number; col: number; row: number; range: [number, number, number, number];
  sheets: string[]; canUndo: boolean; canRedo: boolean; undoLabel: string | null; redoLabel: string | null;
  dirty: boolean; frozen: [number, number]; gridlines: boolean; clip: boolean;
  bar: { formula: string; edit: string; ghost: boolean };
  editing: { col: number; row: number; text: string } | null;
}

interface FontDesc { sizePt: number; zoom: number; ppem: number; bold: boolean; italic: boolean; name: string; alt: string | null; metric: string }

async function fetchBytes(src: Bytes): Promise<Uint8Array | ArrayBuffer> {
  const s = await src;
  if (typeof s === 'string' || s instanceof URL) return new Uint8Array(await (await fetch(s)).arrayBuffer());
  if (typeof Response !== 'undefined' && s instanceof Response) return new Uint8Array(await s.arrayBuffer());
  return s as Uint8Array | ArrayBuffer;
}

type Ue = ReturnType<typeof bindSheetEditor>;
let current: Ue | null = null;
let loading: Promise<Ue> | null = null;

/** Loads (once per page) the editor module. */
export function loadSheetEditorModule(opts: LoadSheetEditorOptions): Promise<SheetEditorModule> {
  if (current) return Promise.resolve(new SheetEditorModule(current));
  loading ??= (async () => {
    if (opts.wasm === undefined) throw new Error('loadSheetEditorModule: no wasm source');
    const { instance } = await WebAssembly.instantiate((await fetchBytes(opts.wasm)) as BufferSource, {});
    current = bindSheetEditor(instance.exports);
    return current;
  })().catch((e) => { loading = null; throw e; });
  return loading.then((u) => new SheetEditorModule(u));
}

/** The editor module when it is already instantiated, else null. */
export function currentSheetEditorModule(): SheetEditorModule | null { return current ? new SheetEditorModule(current) : null; }

export class SheetEditorModule {
  #ue: Ue;
  constructor(ue: Ue) { this.#ue = ue; }
  /** Opens a workbook for editing (`locale` = the `formula-locale`; empty bytes = the blank workbook). */
  open(bytes: Uint8Array, opts: { locale?: string; seed?: number; nowSerial?: number } = {}): SheetEditBook {
    return new SheetEditBook(this.#ue.open(bytes, opts));
  }
}

type Ed = ReturnType<Ue['open']>;

/** One workbook in the editor module: commands, state, save and the painter of its frames. */
export class SheetEditBook {
  #e: Ed;
  #tables: Tables = newTables();
  #painter: SlidesPainter | null = null;
  #host: SheetHost | null = null;
  constructor(e: Ed) { this.#e = e; }
  get freed(): boolean { return this.#e.freed; }
  free(): void { this.#e.free(); }

  /** One command (`op` + arguments); `{ result, state }`; a refusal throws `SheetEditError` (status 0, `state` attached). */
  cmd(op: string, ...args: (string | number | boolean | undefined)[]): { result: unknown; state: SheetEditState } {
    return this.#e.cmd(op, ...args.map((a) => (typeof a === 'boolean' ? (a ? '1' : '0') : a))) as { result: unknown; state: SheetEditState };
  }

  state(): SheetEditState { return this.#e.state() as SheetEditState; }
  save(): Uint8Array { return this.#e.save(); }
  setAccent(css: string): void { this.#e.setAccent(css); }

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

  /** The viewer's `activate` (the editor activates the sheet of each frame itself). */
  activate(_index: number, _host: SheetHost): void {}

  extent(sheet: number, host: SheetHost): { width: number; height: number } | null { return this.#e.extent(sheet, this.#measurer(host)); }

  #painterFor(host: SheetHost): SlidesPainter {
    if (!this.#painter || this.#host !== host) {
      this.#host = host;
      this.#painter = new SlidesPainter({
        fontStack: (f, b, i) => host.fontStack(f, b, i),
        media: () => null,
        bitmap: (index) => { const t = this.#tables.bitmaps[index]; return t ? this.#e.bitmap(t.data) : null; },
      });
    }
    return this.#painter;
  }

  /** Paints one frame of view `q.sheet` on `ctx` (the session's sheet index). */
  paint(ctx: CanvasRenderingContext2D, q: { sheet: number; width: number; height: number; dpr?: number; scrollX: number; scrollY: number; zoom: number; selection: number[]; capture?: boolean; editing?: [number, number] }, host: SheetHost): void {
    const bytes = this.#e.viewport(q, this.#measurer(host));
    const list = decodeSlidesList(bytes, this.#tables);
    this.#painterFor(host).paint(ctx, list, this.#tables, undefined);
  }
}
