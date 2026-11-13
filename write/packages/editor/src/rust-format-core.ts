// Frente A1 da leva 7 do engine="rust" (Arquivo, Exibir, Fonte e Parágrafo; registro core/docs/modules/uo-bindings.md
// "Wave 7 — A1"): o estado da casca que o TS guarda no elemento (o Pincel, as linhas de grade, as marcas dos indicadores, o
// formato carregado) e o acesso comum dos módulos rust-format-*.ts ao motor (o binding das
// exportações de src/session/shell_format.rs). Sem sessão (o motor falso do teste de rotas), cada handler responde o neutro.
import { bindFormat, type FormatBinding } from '@universal-office/write-session';
import type { RustEditor } from './rust-engine.js';
import { Ctx, apiOf } from './rust-review-core.js';

export type Handlers = Record<string, (...args: never[]) => unknown>;

/** The shell state the TS element keeps for the A1 families (`painterProps`/`painterLock`, `docGrid`, `bookmarkMarksShown`); the
 *  `loadedFormat` is the session's (`loadedState`). */
export interface FormatState {
  /** the Format Painter is on (the session's handle keeps the painter itself: `CopyFormat`) */
  painter: boolean;
  /** locked by a double click (`painterLock`) */
  painterLock: boolean;
  /** View > Gridlines (`docGrid`) */
  grid: boolean;
  /** View > Bookmarks (`bookmarkMarksShown`) */
  bookmarks: boolean;
  /** a Manual Hyphenation in course (`manualHyph`): the session keeps it; the shell knows it started */
  hyph: boolean;
}

export class FormatCtx extends Ctx {
  readonly st: FormatState = { painter: false, painterLock: false, grid: false, bookmarks: false, hyph: false };
  /** the view family's `applyView(v)` (the shell engine's view; set by rust-format-view.ts): File > Save as XPS lays out the pages */
  viewApply: ((v: string) => void) | null = null;

  /** the format binding of the module of now (null without session or exports) */
  get fb(): FormatBinding | null {
    return formatBindingOf(this);
  }

  /** `loadedFormat` (save('auto')): the session's (`Session::loaded_format`) */
  get loaded(): 'docx' | 'odt' {
    return loadedState(this)?.format ?? 'docx';
  }

  /** a JSON read of the session (null without session or binding) */
  json<T>(what: number, args: readonly (string | number)[] = []): T | null {
    const s = this.s, fb = this.fb;
    if (!s || !fb) return null;
    try { return fb.json(s, what, args) as T; } catch (e) { this.api?.warnOnce(`fmt:${what}`, `[universal-write] engine="rust": read ${what} failed`, e); return null; }
  }

  /** a native-only command applied (the outcome, or null) */
  exec(what: number, args: readonly (string | number)[], name: string): ReturnType<FormatBinding['exec']> {
    const s = this.s, fb = this.fb, api = this.api;
    if (!s || !fb || !api) return null;
    const o = fb.exec(s, what, args);
    api.apply(o, name);
    return o;
  }

  /** the element's own (TS) method, run on the element (`proto[name].apply(el, args)`) */
  ts(name: string, args: unknown[]): unknown {
    const el = this.el;
    const f = (this.proto as Record<string, unknown>)[name];
    return el && typeof f === 'function' ? (f as (...a: unknown[]) => unknown).apply(el, args) : undefined;
  }

  constructor(ed: RustEditor, readonly proto: object) {
    super(ed, apiOf(ed));
  }
}

const bindings = new WeakMap<object, FormatBinding | null>();

/** The format binding (src/session/shell_format.rs) of a context's module, one per module (null without module or exports). */
export function formatBindingOf(c: Ctx): FormatBinding | null {
  const mod = c.api?.module as unknown as { exports: WebAssembly.Exports; binding: unknown } | undefined;
  if (!mod) return null;
  let b = bindings.get(mod);
  if (b === undefined) {
    try { b = bindFormat(mod.exports, mod.binding as never); } catch { b = null; }
    bindings.set(mod, b);
  }
  return b;
}

/** The session's `loadedFormat` and whether `listStyles` takes the `.odt` branch (universal-write.ts 4156); null without session. */
export function loadedState(c: Ctx): { format: 'docx' | 'odt'; odtStyles: boolean } | null {
  const s = c.s, fb = formatBindingOf(c);
  if (!s || !fb) return null;
  try { return fb.json(s, 18) as { format: 'docx' | 'odt'; odtStyles: boolean }; } catch { return null; }
}

/** The element's change notifications (`onSelectionChange?.(); chromeSync?.()`). */
export function sync(c: Ctx): void {
  const el = c.el;
  el?.onSelectionChange?.();
  el?.chromeSync?.();
}

/** `sectionColumns(sp, sec)` (universal-write.ts 492, a module function there: copied, declared): the columns of a section
 *  as the engine builds them. `sect` is the section's `SectionProps` (`colDefs`, `equalWidth`). */
export function sectionColumns(sp: SectPrJson, sect?: { colDefs?: { w: number; space: number }[]; equalWidth?: boolean } | null): { x: number; w: number }[] {
  const marL = sp.marginLeftTwips;
  const contentW = Math.max(720, sp.pageWidthTwips - marL - sp.marginRightTwips);
  const n = Math.max(1, sp.cols?.count ?? 1);
  const defs = sect?.colDefs;
  const out: { x: number; w: number }[] = [];
  if (n > 1 && defs && defs.length >= n && sect?.equalWidth === false) {
    let x = marL;
    for (const d of defs.slice(0, n)) { out.push({ x, w: d.w }); x += d.w + d.space; }
  } else if (n > 1) {
    const space = sp.cols?.spaceTwips ?? 720;
    const w = (contentW - space * (n - 1)) / n;
    for (let k = 0; k < n; k++) out.push({ x: marL + k * (w + space), w });
  } else out.push({ x: marL, w: contentW });
  return out;
}

/** The JSON of a `SectPr` (the session's model object). */
export interface SectPrJson {
  pageWidthTwips: number; pageHeightTwips: number; marginTopTwips: number; marginRightTwips: number; marginBottomTwips: number; marginLeftTwips: number;
  cols?: { count?: number; spaceTwips?: number; widths?: { widthTwips: number; spaceTwips: number }[] };
  type?: string;
}
