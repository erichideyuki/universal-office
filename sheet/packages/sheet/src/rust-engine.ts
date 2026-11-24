import { loadSheetModule, currentSheetModule, type LoadSheetOptions, type SheetBook } from '@universal-office/sheet-engine';

/**
 * `engine="rust"` of `<universal-sheet>` (read path: open, scroll, zoom, sheet tabs, selection display, frozen panes): the side of
 * the component of the TS/Rust switch (record core/docs/modules/uo-xlsx.md "C4 — as built"). Loaded ONLY by a dynamic `import()`
 * when the attribute asks for the Rust engine — a sheet without it never downloads this module or the .wasm. The Rust side
 * lays out and emits the display list of the viewport; text measures (`measureText`), fonts and the canvas stay in the browser,
 * as do the scrollbars and the empty-state text. Editing still goes through the TS engine (frontier C5).
 */
const WASM_FILE = 'uo-sheet.wasm';

function withDefaults(s: LoadSheetOptions | undefined): LoadSheetOptions {
  return { ...s, wasm: s?.wasm ?? new URL(WASM_FILE, import.meta.url) };
}

/** Instantiates the module before there is a workbook (the component with `engine="rust"` calls it on connect). */
export async function preloadSheetEngine(source: LoadSheetOptions | undefined): Promise<void> {
  await loadSheetModule(withDefaults(source));
}

/** The workbook in the module, or null while the module is not instantiated (the TS engine paints meanwhile). */
export function openSheetBook(bytes: Uint8Array): SheetBook | null {
  return currentSheetModule()?.open(bytes) ?? null;
}
