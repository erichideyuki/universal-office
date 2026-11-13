// Types of js/session-format.mjs (wave 7, A1; record core/docs/modules/uo-bindings.md "Wave 7 — A1: File, View, Font and
// Paragraph"). The JSON answers are those of src/session/shell_format.rs; `ExecOutcome` is the generated one.

import type { ExecOutcome } from './gen/session.ts';
import type { WriteBinding, WriteEnv, WriteLayout } from './write.js';
import type { SessionBinding, WriteSession } from './session.js';

export interface RulerLine { page: number; x: number; y: number; frame: [number, number] | null; pfmt: { left: number; right: number; firstLine: number; tabs: { val: string; pos: number; leader?: string }[] } | null }
export interface FormatImage { id: number; ext: string; data: Uint8Array }

export interface FormatBinding {
  /** 1 flow, 2 layout, 3 borders, 4 default tab, 5 theme colours, 6 external fields, 7 links, 8 custom inlines, 9 style key,
   *  10 built-in style id, 11 drawing grid, 12 a section's sectPr, 18 the loaded format `{ format, odtStyles }` */
  json(session: WriteSession, what: number, args?: readonly (string | number)[]): unknown;
  /** 1 insertCustomInline(type, dataJson, fallback), 2 ensureBuiltinStyle(name), 3 replaceAll(needle, replacement) */
  exec(session: WriteSession, what: number, args?: readonly (string | number)[]): ExecOutcome | null;
  hyphenation(session: WriteSession, env: WriteEnv | null): void;
  exportOdt(session: WriteSession, omitImages?: boolean): Uint8Array;
  exportText(session: WriteSession, markdown: boolean, title: string | undefined, omit: boolean, uris?: readonly [number, string][]): string;
  images(session: WriteSession): FormatImage[];
  openOdt(ss: SessionBinding, bytes: Uint8Array, o?: { protected?: boolean; author?: string; lang?: string; clock?: Date; seed?: number; env?: WriteEnv }): WriteSession;
  ruler(session: WriteSession, layout: WriteLayout): RulerLine | null;
  setView(engine: { readonly ptr: number }, kind: number, widthTw?: number): void;
  /** wave 7c (V): the editor's continuous view (1 Web, 2 Draft, 3 Outline, 0 Print) */
  setContinuous(engine: { readonly ptr: number }, kind: number, widthTw?: number, marginTw?: number, tsStrict?: boolean): void;
  tocTemplate(session: WriteSession, env: WriteEnv, id: number, now?: number | Date): WriteLayout;
  tocCrop(session: WriteSession, id: number): { cropLeft: number; marginRight: number } | null;
  /** File > Compare: the Rust diffDocuments over two .docx (the compare package's DocDiff: { edits, stats }) */
  compare(a: Uint8Array, b: Uint8Array, opts?: { algorithm?: 'histogram' | 'myers'; intraWord?: boolean }): { edits: unknown[]; stats: { inserted: number; deleted: number; changed: number; formatOnly: number; equal: number } };
  /** File > Save as XPS (fix round 2, D-A1-2): the XPS package of a layout's pages, written in Rust (uo-pdf `xps`) */
  xps(layout: WriteLayout): Uint8Array;
  utf8(s: string): Uint8Array;
}
export declare function bindFormat(exports: WebAssembly.Exports, uo: WriteBinding): FormatBinding;
