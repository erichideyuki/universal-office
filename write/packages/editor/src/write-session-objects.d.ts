// Wave 5, front O of the editor switch (record core/docs/modules/uo-bindings.md "Wave 5 — O: objects and the page"):
// the object geometry and section facts of the session (core/crates/uo-wasm/js/session.mjs `bindObjects`, typed by
// session.d.ts), declared for the editor's tsc in a file of its own — the ambient declarations of
// '@universal-office/write-session' merge with write-session.d.ts (no edit of the shared file).
declare module '@universal-office/write-session' {
  import type { EngineLayout } from '@universal-office/write-engine';

  export interface ShellObjectFrame { pageW: number; pageH: number; marL: number; marR: number; marT: number; marB: number; colX: number; colW: number; bodyTop: number; paraTop: number }
  export interface ShellFloat {
    i: number;
    kind: 'image' | 'textbox' | 'shape' | 'group';
    id: number;
    page: number;
    x: number;
    y: number;
    w: number;
    h: number;
    rot: number;
    behind: boolean;
    editable: boolean;
    content: boolean;
    key: string | null;
    image: number | null;
    frame: ShellObjectFrame | null;
  }
  export interface ShellInlinePic { para: number; run: number; id: number; page: number; x: number; y: number; w: number; h: number; rot: number; image: number }
  export interface ShellObjects { scope: 'body' | 'hf' | 'tb'; box: number | null; fs: number | null; sel: number[]; floats: ShellFloat[]; boxFloat?: ShellFloat | null; inline: ShellInlinePic[] }
  export interface ShellSectionInfo {
    index: number;
    count: number;
    touched: Record<string, unknown>[];
    pageBorderSection: Record<string, unknown>;
    firstSect: Record<string, unknown> | null;
    suppressed: boolean;
    titlePg: boolean[];
    background: Record<string, unknown> | null;
    lastSect: Record<string, unknown> | null;
    bgShown: boolean;
    evenOdd: boolean;
  }
  export interface ObjectsBinding {
    objects(session: WriteSession, layout: EngineLayout, activePage?: number, inTextbox?: boolean): ShellObjects;
    sectionInfo(session: WriteSession, hfSection?: number): ShellSectionInfo;
  }
  export function bindObjects(exports: WebAssembly.Exports, uo: unknown): ObjectsBinding;
}
