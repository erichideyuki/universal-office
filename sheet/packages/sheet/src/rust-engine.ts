import { loadSheetModule, currentSheetModule, type LoadSheetOptions, type SheetBook } from '@universal-office/sheet-engine';
import { loadSheetEditorModule, currentSheetEditorModule, type LoadSheetEditorOptions, type SheetEditBook } from '@universal-office/sheet-editor-engine';

/**
 * `engine="rust"` of `<universal-sheet>` (read path: open, scroll, zoom, sheet tabs, selection display, frozen panes): the side of
 * the component of the TS/Rust switch (record core/docs/modules/uo-xlsx.md "C4 — as built"). Loaded ONLY by a dynamic `import()`
 * when the attribute asks for the Rust engine — a sheet without it never downloads this module or the .wasm. The Rust side
 * lays out and emits the display list of the viewport; text measures (`measureText`), fonts and the canvas stay in the browser,
 * as do the scrollbars and the empty-state text. Editing still goes through the TS engine (frontier C5).
 */
const WASM_FILE = 'uo-sheet.wasm';
/** The same viewer with the calc engine (C4b): fetched only for a workbook whose open-recalc plan is not `none`. */
const CALC_WASM_FILE = 'uo-sheet-calc.wasm';

function withDefaults(s: LoadSheetOptions | undefined): LoadSheetOptions {
  return { ...s, wasm: s?.wasm ?? new URL(WASM_FILE, import.meta.url), calcWasm: s?.calcWasm ?? new URL(CALC_WASM_FILE, import.meta.url) };
}

/** Instantiates the module before there is a workbook (the component with `engine="rust"` calls it on connect). */
export async function preloadSheetEngine(source: LoadSheetOptions | undefined): Promise<void> {
  await loadSheetModule(withDefaults(source));
}

/** The workbook in the module, or null while the module is not instantiated (the TS engine paints meanwhile). */
export function openSheetBook(bytes: Uint8Array): SheetBook | null {
  return currentSheetModule()?.open(bytes) ?? null;
}

// ───────────────────────── editor (frontier C5b) ─────────────────────────
// With `engine="rust"` the EDIT operations go to the editing session of `uo-sheet-editor.wasm` (its own instance: the viewer's
// exports + `sheet-edit`); its viewport replaces the viewer's frames once a workbook is open in it. The TS engine stays the
// default and takes over again when the module is missing or a command fails (the element reloads its model from the editor's
// last good save).

export type { SheetEditBook };

const EDITOR_WASM_FILE = 'uo-sheet-editor.wasm';

/** Instantiates the editor module (the component calls it with `engine="rust"`); rejects when the file is not there. */
export async function preloadSheetEditor(source: LoadSheetEditorOptions | undefined): Promise<void> {
  await loadSheetEditorModule({ ...source, wasm: source?.wasm ?? new URL(EDITOR_WASM_FILE, import.meta.url) });
}

/** The workbook in the editor module (`locale` = the `formula-locale`), or null while the module is not instantiated. */
export function openSheetEditor(bytes: Uint8Array, locale: string): SheetEditBook | null {
  return currentSheetEditorModule()?.open(bytes, { locale, seed: (Math.random() * 0xffffffff) >>> 0 || 1 }) ?? null;
}
