// The surface of the Rust engine's JS side (core/crates/uo-wasm/js/sheet-engine.ts) that the component's `rust-engine.ts` uses,
// declared here so the package's own `tsc` build (rootDir `src`) does not compile the core. The real module is reached through the
// alias `@universal-office/sheet-engine` (`build:demo`, tools/build-pkg.mjs `ENGINE`).
declare module '@universal-office/sheet-engine' {
  type Bytes = Uint8Array | ArrayBuffer | string | URL | Response | Promise<Uint8Array | ArrayBuffer | Response>;

  /** The error of an export (`status`: 3 unreadable workbook, 5 argument, 6 no such sheet, 12 measurements that do not settle). */
  export class SheetError extends Error {
    readonly status: number;
    readonly detail: string;
  }

  /** `wasm`: `uo-sheet.wasm`; `calcWasm`: `uo-sheet-calc.wasm`, loaded for a workbook whose open-recalc plan is not `none` (C4b). */
  export interface LoadSheetOptions { wasm?: Bytes; calcWasm?: Bytes }

  export interface SheetHost {
    /** the CSS font stack of a cell family (`name` or `name|altName`) */
    fontStack(family: string, bold: boolean, italic: boolean): string;
    /** the CSS font stack of a shape's text (C4b; default `fontStack`) */
    shapeFontStack?(family: string, bold: boolean, italic: boolean): string;
    /** a canvas 2D context to measure with (never the painting one) */
    measureContext(): CanvasRenderingContext2D | OffscreenCanvasRenderingContext2D;
    /** the GDI-width family of a font name (`arial` when the browser falls back to the default substitute), C4b */
    metricFamily?(name: string): string;
    /** a decoded picture or the calc module arrived: paint again (C4b) */
    invalidate?(): void;
  }

  export interface SheetViewportQuery {
    sheet: number; width: number; height: number; dpr?: number; scrollX: number; scrollY: number; zoom: number;
    /** `[col, row, c1, r1, c2, r2]`, 1-based */
    selection: number[];
    capture?: boolean;
    editing?: [number, number];
  }

  export class SheetBook {
    readonly freed: boolean;
    free(): void;
    sheets(): { name: string; kind: 'sheet' | 'chart' | 'none'; state: number; active: boolean }[];
    recalcPlan(): 'none' | 'partial' | 'full';
    extent(): { width: number; height: number } | null;
    linkAt(sheet: number, col: number, row: number): string | null;
    setAccent(css: string): void;
    setSpill(c1: number, r1: number, c2: number, r2: number): void;
    activate(index: number, host: SheetHost): void;
    paint(ctx: CanvasRenderingContext2D, q: SheetViewportQuery, host: SheetHost): void;
  }

  export class SheetModule { open(bytes: Uint8Array): SheetBook }
  export function loadSheetModule(opts: LoadSheetOptions): Promise<SheetModule>;
  export function currentSheetModule(): SheetModule | null;
}

// The EDITOR side (frontier C5b: core/crates/uo-wasm/js/sheet-editor-engine.ts, alias `@universal-office/sheet-editor-engine`):
// the editing session of `uo-sheet-editor.wasm` that `rust-engine.ts` opens when the component edits with `engine="rust"`.
declare module '@universal-office/sheet-editor-engine' {
  import type { SheetHost } from '@universal-office/sheet-engine';
  type Bytes = Uint8Array | ArrayBuffer | string | URL | Response | Promise<Uint8Array | ArrayBuffer | Response>;

  /** A refused command (`status` 0: `detail` is the session's message, `state` the state after it) or an export error. */
  export class SheetEditError extends Error {
    readonly status: number;
    readonly detail: string;
    readonly state?: SheetEditState;
  }

  export interface LoadSheetEditorOptions { wasm?: Bytes }

  export interface SheetEditState {
    sheet: number; col: number; row: number; range: [number, number, number, number];
    sheets: string[]; canUndo: boolean; canRedo: boolean; undoLabel: string | null; redoLabel: string | null;
    dirty: boolean; frozen: [number, number]; gridlines: boolean; clip: boolean;
    bar: { formula: string; edit: string; ghost: boolean };
    editing: { col: number; row: number; text: string } | null;
  }

  export class SheetEditBook {
    readonly freed: boolean;
    free(): void;
    cmd(op: string, ...args: (string | number | boolean | undefined)[]): { result: unknown; state: SheetEditState };
    state(): SheetEditState;
    save(): Uint8Array;
    setAccent(css: string): void;
    activate(index: number, host: SheetHost): void;
    extent(sheet: number, host: SheetHost): { width: number; height: number } | null;
    paint(ctx: CanvasRenderingContext2D, q: {
      sheet: number; width: number; height: number; dpr?: number; scrollX: number; scrollY: number; zoom: number;
      selection: number[]; capture?: boolean; editing?: [number, number];
    }, host: SheetHost): void;
  }

  export class SheetEditorModule { open(bytes: Uint8Array, opts?: { locale?: string; seed?: number; nowSerial?: number }): SheetEditBook }
  export function loadSheetEditorModule(opts: LoadSheetEditorOptions): Promise<SheetEditorModule>;
  export function currentSheetEditorModule(): SheetEditorModule | null;
}
