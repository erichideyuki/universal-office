// Wave 7 (A2): the lists of `<universal-write engine="rust">` (universal-write.ts 3626–4250): the 12 list mutators as the
// session's `ListCmd`, the five list queries from src/session/shell_completion.rs, `markerPreview` through the TS shell code
// (its body only reads the shell's font registry). Record core/docs/modules/uo-bindings.md "Wave 7 — A2".
import { ask, command, cstate, locked, through, type CState } from './rust-completion-core.js';
import type { RustEditor } from './rust-engine.js';

type Handlers = Record<string, (...args: never[]) => unknown>;
type Level = Record<string, unknown>;

/**
 * The shell's TS font registry (read by `markerPreview` through `canvasFont`/`liveFontProvider`) gets the families the Rust
 * engine asked for, as the TS layout registers every family it asks for (`applyResolvedFont`: synchronous when the shared
 * resolver already has the family, else asked and applied when it arrives) — the styles' and theme's families too, not only
 * the faces the layout ended up with (f2-3-0: TS Aptos, Calibri, Arial…; the layout's faces only Liberation Serif). Without it
 * the registry stays empty under the Rust engine and the preview's CSS stack falls back to the default face (ED-C1: TS
 * `"Carlito", "Liberation Sans"`, Rust `"Liberation Sans"`). The asked families are the page environment's (shared by the
 * Rust editors of the page; declared), plus the faces of this layout.
 */
function shellFonts(c: CState): void {
  const el = c.ed?.el;
  if (!el) return;
  const apply = (Object.getPrototypeOf(el) as { applyResolvedFont?: (f: string) => boolean }).applyResolvedFont;
  if (typeof apply !== 'function') return;
  const rs = (c.ed as unknown as { rs?: { pushedFamilies?: () => string[] } }).rs;
  const fams = new Set([...(rs?.pushedFamilies?.() ?? []), ...(c.api?.dl?.tables.faces ?? []).map((x) => x.family)]);
  for (const f of fams) apply.call(el, f);
}

/** A TS `ListLevelSpec` for the wire (the same keys; undefined members dropped). */
const wireLevel = (l: Level): Level => Object.fromEntries(Object.entries(l).filter(([, v]) => v !== undefined));
const wireLevels = (ls: readonly Level[] | undefined): Level[] => (ls ?? []).map(wireLevel);

/** `imageSize(b)` of universal-write.ts 704 (module-private there: a declared copy): the pixel size of a PNG, GIF or JPEG. */
function imageSize(b: Uint8Array): { w: number; h: number } | null {
  if (b.length > 24 && b[0] === 0x89 && b[1] === 0x50) return { w: (b[16]! << 24 | b[17]! << 16 | b[18]! << 8 | b[19]!) >>> 0, h: (b[20]! << 24 | b[21]! << 16 | b[22]! << 8 | b[23]!) >>> 0 };
  if (b.length > 10 && b[0] === 0x47 && b[1] === 0x49) return { w: b[6]! | b[7]! << 8, h: b[8]! | b[9]! << 8 };
  if (b[0] === 0xff && b[1] === 0xd8) {
    for (let i = 2; i + 9 < b.length;) {
      if (b[i] !== 0xff) { i++; continue; }
      const m = b[i + 1]!, len = b[i + 2]! << 8 | b[i + 3]!;
      if (m >= 0xc0 && m <= 0xcf && m !== 0xc4 && m !== 0xc8 && m !== 0xcc) return { h: b[i + 5]! << 8 | b[i + 6]!, w: b[i + 7]! << 8 | b[i + 8]! };
      i += 2 + len;
    }
  }
  return null;
}

function list(c: CState, v: Record<string, unknown>, what: string): void {
  if (locked(c)) return;
  command(c, { t: 'list', v } as never, what);
}

export function listsHandlers(ed: RustEditor): Handlers {
  const c = cstate(ed);
  const info = () => ask(c, 'listInfo', (b, s) => b.listInfo(s));
  const h: Record<string, (...a: never[]) => unknown> = {
    listInfo: () => {
      const i = info();
      if (!i) return null;
      // the TS `ListInfo` (canContinue is the binding's extra, for continueNumbering's answer)
      const { canContinue: _c, ...rest } = i;
      return rest;
    },
    documentLists: () => ask(c, 'documentLists', (b, s) => b.documentLists(s)) ?? [],
    listStylesInDocument: () => ask(c, 'listStyles', (b, s) => b.listStyles(s)) ?? [],
    numberingValueInfo: () => ask(c, 'numberingValue', (b, s) => b.numberingValue(s)),
    listLibrary: ((kind: 'bullet' | 'number' | 'multilevel') => ask(c, 'listLibrary', (b, s) => b.listLibrary(s, kind)) ?? []) as never,
    markerPreview: ((text: string, font?: string) => { shellFonts(c); return through(c, 'markerPreview', [text, font]); }) as never,
    applyListLibrary: ((kind: 'bullet' | 'number', index: number | null) =>
      list(c, { t: 'applyListLibrary', bullet: kind === 'bullet', ...(index === null || index === undefined ? {} : { index }) }, 'applyListLibrary')) as never,
    applyMultilevelLibrary: ((index: number | null) =>
      list(c, { t: 'applyMultilevelLibrary', ...(index === null || index === undefined ? {} : { index }) }, 'applyMultilevelLibrary')) as never,
    applyListLevels: ((levels: Level[]) => { if (levels?.length) list(c, { t: 'applyListLevels', levels: wireLevels(levels) }, 'applyListLevels'); }) as never,
    setListLevel: ((ilvl: number) => list(c, { t: 'setListLevel', ilvl }, 'setListLevel')) as never,
    restartNumbering: ((start?: number) => list(c, { t: 'restartNumbering', ...(start === undefined ? {} : { start }) }, 'restartNumbering')) as never,
    continueNumbering: () => {
      // the TS answer: false without a compatible list before (`previousList`), or when it cannot edit
      if (locked(c)) return false;
      const can = !!info()?.canContinue;
      if (!can) return false;
      list(c, { t: 'continueNumbering' }, 'continueNumbering');
      return true;
    },
    setNumberingValue: ((o: { mode: 'new' | 'continue'; value?: number }) =>
      list(c, { t: 'setNumberingValue', newList: o?.mode === 'new', ...(o?.value === undefined ? {} : { value: o.value }) }, 'setNumberingValue')) as never,
    defineListLevel: ((o: Level & { picture?: { data: Uint8Array; ext: string; name?: string } }) => {
      if (!o) return;
      const { picture: pic, ...rest } = o;
      if (pic) {
        // wave 7 (A2): a picture bullet (`ShellEdit::DefineListLevelPicture`): the natural size at 96 dpi, as the TS 4026
        if (locked(c)) return;
        const px = imageSize(pic.data);
        const out = ask(c, 'defineListLevelPicture', (b, s) => b.defineListLevelPicture(s, wireLevel(rest), {
          data: new Uint8Array(pic.data), ext: pic.ext, wPt: (px?.w ?? 96) * 0.75, hPt: (px?.h ?? 96) * 0.75, name: pic.name ?? 'bullet',
        }));
        if (out) c.api.apply(out, 'defineListLevel');
        return;
      }
      list(c, { t: 'defineListLevel', format: wireLevel(rest) }, 'defineListLevel');
    }) as never,
    defineMultilevelList: ((levels: Level[], o: { applyTo?: 'list' | 'forward' } = {}) =>
      list(c, { t: 'defineMultilevelList', levels: wireLevels(levels), ...(o?.applyTo === 'forward' ? { forward: true } : {}) }, 'defineMultilevelList')) as never,
    createListStyle: ((name: string, levels: Level[] = [], o: { apply?: boolean } = {}) => {
      if (locked(c) || !(name ?? '').trim()) return undefined;
      const before = new Set((ask(c, 'listStyles', (b, s) => b.listStyles(s)) ?? []).map((x) => x.id));
      list(c, { t: 'createListStyle', name, levels: wireLevels(levels), ...(o?.apply === false ? {} : { apply: true }) }, 'createListStyle');
      return (ask(c, 'listStyles', (b, s) => b.listStyles(s)) ?? []).find((x) => !before.has(x.id))?.id;
    }) as never,
    applyListStyle: ((styleId: string) => list(c, { t: 'applyListStyle', styleId }, 'applyListStyle')) as never,
    modifyListStyle: ((styleId: string, levels: Level[]) => list(c, { t: 'modifyListStyle', styleId, levels: wireLevels(levels) }, 'modifyListStyle')) as never,
  };
  return h;
}
