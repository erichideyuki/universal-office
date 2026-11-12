// A superfície da sessão wasm (core/crates/uo-wasm/js/session.mjs, tipada por session.d.ts) que o motor Rust do
// editor (`rust-engine.ts`) usa, declarada aqui para o `tsc` do editor não compilar o núcleo — como o
// `write-engine.d.ts` do viewer. O módulo real chega pelo alias `@universal-office/write-session` (vitest.config.ts,
// `build:demo`). `core/crates/uo-wasm/js/editor.check.ts` atribui o módulo real a esta declaração: divergência falha
// a linha de tsc do núcleo. As mensagens (Command, eventos) são as geradas de core/schema/session.uos, aqui no
// formato mínimo que o editor lê.
declare module '@universal-office/write-session' {
  import type { EngineEnv, EngineLayout } from '@universal-office/write-engine';

  export interface DocPos { para: number; offset: number }

  /** Um evento da sessão: `t` é o tipo do CustomEvent (`uw-change`…), os outros campos o `detail`. */
  export interface SessionEvent { t: string }

  export interface ChangeSet {
    stories: unknown[];
    selection: { anchor: DocPos; focus: DocPos };
    events: SessionEvent[];
    /** os booleanos falsos não viajam (ausente = false) */
    dirty?: boolean;
    canUndo?: boolean;
    canRedo?: boolean;
    formatting?: boolean;
  }

  export interface SessionJob { t: string; id: number }

  export type ExecOutcome =
    | ({ t: 'done' } & ChangeSet)
    | { t: 'jobs'; jobs: SessionJob[] }
    | { t: 'error'; v: unknown };

  /** Um comando da sessão (`{ t: 'format', v: { t: 'toggleRunProp', key: 'bold', button: 'single' } }`). */
  export interface SessionCommand { t: string; v?: unknown }

  export interface SessionRunProps { bold?: boolean; italic?: boolean; underline?: boolean; underlineStyle?: string; fontAscii?: string; szHalfPt?: number; charStyle?: string }
  export interface SessionParaProps { alignment?: string; styleId?: string }

  export interface Caret { page: number; x: number; y: number; h: number; xf?: number[] }
  export interface SelRect { x: number; y: number; w: number; h: number; xf?: number[] }

  export interface RelayoutReport {
    path: 'full' | 'incremental';
    reason: string | null;
    relaid: [number, number];
    passes: number;
    lineShift: number;
    memoHits: number;
    memoMisses: number;
    /** wave 6 (L): the relaid pages whose page changed (null from an older engine) */
    changed: number[] | null;
  }

  export interface SessionLayoutOptions {
    now?: Date | number;
    tzOffsetMin?: number;
    hyphenation?: 'on' | 'off' | 'force';
    hyphenationLanguages?: string[];
    lang?: string;
    maxPages?: number;
    /** wave 6 (L, ShellEngine.relayout): the previous relayout's layout, which gives the lent result back (stale after) */
    prev?: EngineLayout | null;
  }

  export interface WriteSession {
    readonly ptr: number;
    free(): void;
    type(text: string): ExecOutcome | null;
    enter(): ExecOutcome | null;
    backspace(): ExecOutcome | null;
    delete(): ExecOutcome | null;
    setSelection(anchor: DocPos, focus?: DocPos): ExecOutcome | null;
    moveCaret(dir: 'left' | 'right' | 'up' | 'down' | 'home' | 'end' | 'wordLeft' | 'wordRight' | 'docStart' | 'docEnd', extend?: boolean, layout?: EngineLayout | null): ExecOutcome | null;
    hitTest(layout: EngineLayout, page: number, x: number, y: number): DocPos | null;
    caretRect(layout: EngineLayout): Caret | null;
    selectionRects(layout: EngineLayout, page: number): SelRect[];
    command(cmd: SessionCommand): ExecOutcome | null;
    undo(): ExecOutcome | null;
    redo(): ExecOutcome | null;
    cancelPending(): ExecOutcome | null;
    setDirty(dirty: boolean): ExecOutcome | null;
    setUiLang(lang: 'en' | 'pt' | 'es'): ExecOutcome | null;
    complete(job: number, result: { t: string }): ExecOutcome | null;
    query(q: { t: string }): { t: string; v?: unknown };
    save(): Uint8Array;
    fontRequests(): { settings: unknown; requests: { family: string; variant: string }[] };
    saveWithFonts(faces: (Uint8Array | undefined)[], guids?: Uint8Array): { bytes: Uint8Array; events: SessionEvent[] } | null;
    storyInfo(): { paras: number; lastLength: number };
    paraText(i: number): string | null;
    selectionState(): { run: SessionRunProps; para: SessionParaProps; pending: boolean };
    hyphenationPatterns(env: EngineEnv, langs?: string[]): string[];
    copied(): { text: string; html: string } | null;
    setFontEmbedding(patch: { embed?: boolean; subset?: boolean; skipCommonFonts?: boolean }): ExecOutcome | null;
    // ── wave 5, frontier R (src/session/shell_review.rs) ──
    pageLines(layout: EngineLayout, page: number): ShellLine[];
    paraLines(layout: EngineLayout, para: number): ShellLine[];
    rangeRects(layout: EngineLayout, page: number, a: DocPos, f: DocPos): SelRect[];
    embeddedFamilies(): string[];
  }

  /** wave 5, frontier R: one line of the edit scope (page twips; the characters as offset/x/width columns) */
  export interface ShellLine {
    para: number; page: number; x: number; y: number; height: number; baseline: number; width: number;
    start: number; end: number; xf?: number[]; o: number[]; cx: number[]; cw: number[];
  }

  export interface ShellEngine {
    readonly ptr: number;
    relayout(session: WriteSession, env: EngineEnv, o?: SessionLayoutOptions): EngineLayout;
    report(): RelayoutReport | null;
    clear(): void;
    free(): void;
  }

  export interface SessionOpenOptions {
    protected?: boolean;
    author?: string;
    tracking?: boolean;
    lang?: string;
    clock?: Date | { local: unknown; utc: unknown };
    seed?: number;
    env?: EngineEnv;
  }

  export interface SessionBinding {
    open(docx: Uint8Array, o?: SessionOpenOptions): WriteSession;
    newBlank(o?: SessionOpenOptions): WriteSession;
    newShellEngine(): ShellEngine;
    clipboardClear(): void;
    readonly dead: boolean;
  }

  export class SessionError extends Error {
    readonly code: number | 'trap';
    readonly detail: string;
  }

  export function bindSession(exports: WebAssembly.Exports, uo: unknown): SessionBinding;

  /** O payload de um item `text/html` (a árvore do DOMParser, `pasteData.html`; paste-tree.mjs de uo-docx). */
  export function encodePaste(html: string, fragmentOf: (html: string) => string): { bytes: Uint8Array; nodes: number; depth: number };

}
