// Wave 7 (A2) of the editor switch (record core/docs/modules/uo-bindings.md "Wave 7 — A2: lists, paste, tables, objects and
// the page"): the completion binding of the session (core/crates/uo-wasm/js/session-completion.mjs, typed by
// session-completion.d.ts), declared for the editor's tsc in a file of its own (an augmentation of write-session.d.ts, as the
// wave-5 fronts do).
declare module '@universal-office/write-session' {
  export interface CompletionLevel {
    ilvl: number; numFmt: string; lvlText: string; font?: string; bold?: boolean; italic?: boolean; sizePt?: number; color?: string;
    start: number; pStyle?: string; indLeft: number; hanging: number; jc: 'left' | 'center' | 'right'; suff: string; tabPos?: number;
    isLgl?: boolean; lvlRestart?: number;
  }
  export interface CompletionListInfo {
    numId: number; ilvl: number; absId: number; kind: 'bullet' | 'number'; level?: CompletionLevel; multilevel: boolean;
    levels: CompletionLevel[]; canContinue: boolean;
  }
  export interface CompletionDocumentList { numId: number; kind: 'bullet' | 'number'; level: CompletionLevel; levels: CompletionLevel[] }
  export interface CompletionListStyle { id: string; name: string; numId: number; levels: CompletionLevel[] }
  export interface CompletionTableStyle { name: string; label: { en: string; pt: string; es: string }; group: 'custom' | 'plain' | 'grid' | 'list'; id?: string }
  export interface CompletionObjFacts {
    rect?: { x: number; y: number; w: number; h: number };
    frame?: { pageW: number; pageH: number; marL: number; marR: number; marT: number; marB: number; colX: number; colW: number; bodyTop: number; paraTop: number };
  }
  export type CompletionEdit =
    | { t: 'filledTable'; rows: string[][]; style: string; firstColumnOff?: boolean; merges?: { r: number; c1: number; c2: number }[]; centerBody?: boolean }
    | { t: 'edge'; col: boolean; anchor: number; path: StoryHop[]; boundary: number; delta: number; rowHeight?: number }
    | { t: 'move'; anchor: number; dx: number; dy: number }
    | { t: 'size'; anchor: number; dx: number; dy: number; rowHeights?: number[] }
    | { t: 'paint'; anchor: number; path: StoryHop[]; r: number; c: number; side: 'top' | 'bottom' | 'left' | 'right'; pen: Record<string, unknown> }
    | { t: 'erase'; anchor: number; path: StoryHop[]; r: number; c: number; side: 'top' | 'bottom' | 'left' | 'right' }
    | { t: 'drawSplit'; anchor: number; path: StoryHop[]; r: number; c: number; vertical: boolean; left: number; boxW: number; rowHeights: number[] }
    | { t: 'drawNew'; w: number; h: number }
    | { t: 'wrapPolygon'; pts: [number, number][] }
    | { t: 'moveSelected'; dx: number; dy: number; facts: CompletionObjFacts[] }
    | { t: 'deleteSelected' }
    | { t: 'moveInline'; para: number; run: number; toPara: number; toOffset: number }
    | { t: 'commitDrag'; rot?: number; size?: [number, number]; resetRel?: boolean; srcRect?: [number, number, number, number]; at?: [number, number]; frame?: CompletionObjFacts['frame']; groupStart?: [number, number] }
    // wave 7c (V): the live object gestures (mousedown, each move, mouseup with or without a move)
    | { t: 'liveBegin' }
    | { t: 'liveSet'; wrap?: [number, number][]; size?: [number, number]; offset?: [number, number]; pageAt?: [number, number]; rot?: number; crop?: [number, number, number, number] }
    | { t: 'liveEnd'; commit: { t: 'wrapPolygon'; pts: [number, number][] } | { t: 'commitDrag'; rot?: number; size?: [number, number]; resetRel?: boolean; srcRect?: [number, number, number, number]; at?: [number, number]; frame?: CompletionObjFacts['frame']; groupStart?: [number, number] } }
    | { t: 'liveCancel' };
  export interface CompletionBinding {
    listInfo(s: WriteSession): CompletionListInfo | null;
    documentLists(s: WriteSession): CompletionDocumentList[];
    listStyles(s: WriteSession): CompletionListStyle[];
    numberingValue(s: WriteSession): { canContinue: boolean; value: number; numFmt: string } | null;
    listLibrary(s: WriteSession, kind: 'bullet' | 'number' | 'multilevel'): CompletionLevel[][];
    tableStyles(s: WriteSession): CompletionTableStyle[];
    tableStyleXml(s: WriteSession, nameOrId: string): { id: string; xml: string; inDoc: boolean } | null;
    tableStyleBases(s: WriteSession): { id: string; name: string }[];
    newTableStyleId(s: WriteSession, name: string): string;
    tableGrid(s: WriteSession, anchor: number, path: StoryHop[]): number[] | null;
    hasCellsClip(): boolean;
    clipboardKinds(): Map<number, 'text' | 'table' | 'drawing' | 'image'>;
    preview(s: WriteSession, env: never, kind: 'cover' | 'watermark' | 'tableStyleXml' | 'tableStyle' | 'renderClip' | 'headerFooter', args?: Record<string, unknown>, now?: number | Date): unknown;
    renderClipMargins(s: WriteSession): { left: number; right: number } | null;
    hfTemplateXml(s: WriteSession, gallery: string, id: string, lang: 'en' | 'pt' | 'es'): string | null;
    sectPrAt(s: WriteSession, section: number): Record<string, unknown> | null;
    linkOrigin(s: WriteSession): { me: string; clip: string | null; link: boolean } | null;
    shapeParts(s: WriteSession): Record<string, unknown> | null;
    defineListLevelPicture(s: WriteSession, format: Record<string, unknown>, pic: { data: Uint8Array; ext: string; wPt: number; hPt: number; name: string }): ExecOutcome | null;
    watermarkTextBox(s: WriteSession, env: never, text: string, font: string, sizePt: number): { widthPt: number; heightPt: number } | null;
    textboxGalleryItem(s: WriteSession): { xml: string; name: string | null } | null;
    oleAt(s: WriteSession, para: number, offset: number): { para: number; run: number; progId: string; ext: string } | null;
    oleData(s: WriteSession, para: number, offset: number): Uint8Array | null;
    replaceOle(s: WriteSession, para: number, run: number, data: Uint8Array, png: Uint8Array, widthTwips: number, heightTwips: number): ExecOutcome | null;
    edit(s: WriteSession, e: CompletionEdit): ExecOutcome | null;
  }
  export function bindCompletion(exports: WebAssembly.Exports, uo: unknown): CompletionBinding;
}
