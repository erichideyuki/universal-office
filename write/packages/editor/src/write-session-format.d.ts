// Wave 7, frontier A1 (File, View, Font and Paragraph; record core/docs/modules/uo-bindings.md "Wave 7 — A1"): the format
// binding of the session (core/crates/uo-wasm/js/session-format.mjs, typed by session-format.d.ts), declared for the
// editor's tsc in a file of its own (an augmentation of write-session.d.ts, as the other fronts do).
declare module '@universal-office/write-session' {
  import type { EngineEnv, EngineLayout } from '@universal-office/write-engine';

  export interface RulerLine { page: number; x: number; y: number; frame: [number, number] | null; pfmt: { left: number; right: number; firstLine: number; tabs: { val: string; pos: number; leader?: string }[] } | null }
  export interface FormatImage { id: number; ext: string; data: Uint8Array }
  export interface FormatBinding {
    json(session: WriteSession, what: number, args?: readonly (string | number)[]): unknown;
    exec(session: WriteSession, what: number, args?: readonly (string | number)[]): ExecOutcome | null;
    hyphenation(session: WriteSession, env: EngineEnv | null): void;
    exportOdt(session: WriteSession, omitImages?: boolean): Uint8Array;
    exportText(session: WriteSession, markdown: boolean, title: string | undefined, omit: boolean, uris?: readonly [number, string][]): string;
    images(session: WriteSession): FormatImage[];
    openOdt(ss: SessionBinding, bytes: Uint8Array, o?: SessionOpenOptions): WriteSession;
    ruler(session: WriteSession, layout: EngineLayout): RulerLine | null;
    setView(engine: { readonly ptr: number }, kind: number, widthTw?: number): void;
    /** wave 7c (V): the editor's continuous view (1 Web, 2 Draft, 3 Outline, 0 Print) */
    setContinuous(engine: { readonly ptr: number }, kind: number, widthTw?: number, marginTw?: number, tsStrict?: boolean): void;
    tocTemplate(session: WriteSession, env: EngineEnv, id: number, now?: number | Date): EngineLayout;
    tocCrop(session: WriteSession, id: number): { cropLeft: number; marginRight: number } | null;
    /** File > Compare: the Rust diffDocuments over two .docx (the compare package's DocDiff: { edits, stats }) */
    compare(a: Uint8Array, b: Uint8Array, opts?: { algorithm?: 'histogram' | 'myers'; intraWord?: boolean }): { edits: unknown[]; stats: { inserted: number; deleted: number; changed: number; formatOnly: number; equal: number } };
    /** File > Save as XPS (fix round 2, D-A1-2): the XPS package of a layout's pages, written in Rust (uo-pdf `xps`) */
    xps(layout: EngineLayout): Uint8Array;
    utf8(s: string): Uint8Array;
  }
  export function bindFormat(exports: WebAssembly.Exports, uo: unknown): FormatBinding;
}
