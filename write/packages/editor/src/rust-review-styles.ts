// Frente R da leva 5 do engine="rust": a lista de estilos (galeria, painel, menus), a aparência de cada estilo, o
// Inspetor de Estilo e criar/modificar/renomear/excluir estilos sobre a sessão (registro uo-bindings.md "Wave 5 — R").
// A sessão responde a lista 'all' com o uso (`listStyles`), a aparência (`styleLook`) e os padrões; o filtro, a ordem
// (prioridade e a colação do idioma da interface: ED-302) e a política de formatação ficam aqui, como no TS.
import { uiLang, uiLocale } from '@universal-office/ui';
import { styleKey, type ResolvedPolicy } from './formatting-policy.js';
import type { RustEditor } from './rust-engine.js';
import { Ctx, changed, outerCtx, storyCtx, type Pos } from './rust-review-core.js';
import type { StyleFilter, StyleInfo, StyleLook } from './styles.js';

type Handlers = Record<string, (...args: never[]) => unknown>;

interface WireStyle {
  id: string; name: string; rawName: string; kind: string; builtIn?: boolean; defined?: boolean; quick?: boolean; hidden?: boolean;
  priority: number; inUse?: boolean; isDefault?: boolean; basedOn?: string; next?: string; link?: string; linkName?: string; autoUpdate?: boolean;
}
type Wire = Record<string, unknown>;

const cmpPos = (a: Pos, b: Pos): number => a.para - b.para || a.offset - b.offset;

/** A StyleFormat do TS (valor = definir, null = herdar, ausente = manter) na forma do fio (`Ch*`). */
function chFormat(f: Record<string, unknown> | undefined): Wire {
  const keys = ['fontAscii', 'szHalfPt', 'bold', 'italic', 'underline', 'strike', 'allCaps', 'smallCaps', 'color', 'charSpacingTwips',
    'alignment', 'spaceBeforeTwips', 'spaceAfterTwips', 'lineTwentieths', 'indentLeftTwips', 'indentRightTwips', 'indentFirstLineTwips'];
  const out: Wire = {};
  for (const k of keys) {
    const v = f?.[k];
    out[k] = v === undefined ? { t: 'keep' } : v === null ? { t: 'null' } : { t: 'val', v };
  }
  return out;
}

const ch = (v: unknown): Wire => (v === undefined ? { t: 'keep' } : v === null ? { t: 'null' } : { t: 'val', v });

/** O policyStyleOk do TS (universal-write.ts 2094). */
function policyStyleOk(s: { id: string; rawName: string; kind: string }, rp: ResolvedPolicy): boolean {
  const set = s.kind === 'character' ? rp.charStyles : rp.styles;
  return !set || set.has(styleKey(s.rawName)) || set.has(styleKey(s.id));
}

export function styleHandlers(c: Ctx): Handlers {
  const all = (): StyleInfo[] => (c.q<WireStyle[]>({ t: 'listStyles' }) ?? []).map((s) => ({
    id: s.id, name: s.name, rawName: s.rawName, kind: s.kind as StyleInfo['kind'], builtIn: !!s.builtIn, defined: !!s.defined,
    quick: !!s.quick, hidden: !!s.hidden, priority: s.priority, inUse: !!s.inUse, isDefault: !!s.isDefault,
    basedOn: s.basedOn, next: s.next, link: s.link, autoUpdate: !!s.autoUpdate,
  }));
  /** `listStyles(ds, lang, inUse, filter)`'s last lines (styles.ts 217–240) and the element's policy filter */
  const listStyles = (filter: StyleFilter = 'recommended'): StyleInfo[] => {
    const el = c.el;
    if (!el || !c.s) return [];
    const lang = uiLang(el);
    const out = all();
    const headingInUse = (n: number): boolean => out.some((s) => s.rawName.toLowerCase() === `heading ${n}` && s.inUse);
    const nextHeading = (s: StyleInfo): boolean => {
      const h = /^heading (\d)$/.exec(s.rawName.toLowerCase());
      return !!h && +h[1]! > 1 && headingInUse(+h[1]! - 1);
    };
    const keep = (s: StyleInfo): boolean => {
      switch (filter) {
        case 'gallery': return s.quick && (!s.hidden || nextHeading(s));
        case 'recommended': return !s.hidden || s.inUse || nextHeading(s);
        case 'inUse': return s.inUse;
        case 'document': return s.defined;
        default: return true;
      }
    };
    const coll = new Intl.Collator(uiLocale(lang));
    const list = out.filter(keep).sort((a, b) => a.priority - b.priority || coll.compare(a.name, b.name));
    const rp = (el as unknown as { resolvedFormattingPolicy?: () => ResolvedPolicy | null }).resolvedFormattingPolicy?.() ?? null;
    return rp && filter !== 'all' && (rp.styles || rp.charStyles) ? list.filter((s) => policyStyleOk(s, rp)) : list;
  };
  const look = (id: string): StyleLook | null => {
    const v = c.q<{ value?: Wire }>({ t: 'styleLook', id })?.value;
    if (!v) return null;
    const b = (k: string): boolean => !!v[k];
    return {
      fontFamily: v['fontFamily'] as string, sizePt: v['sizePt'] as number,
      bold: b('bold'), italic: b('italic'), underline: b('underline'), strike: b('strike'), allCaps: b('allCaps'), smallCaps: b('smallCaps'),
      color: v['color'] as string | undefined, charSpacingTwips: v['charSpacingTwips'] as number,
      alignment: v['alignment'] as StyleLook['alignment'],
      spaceBeforeTwips: v['spaceBeforeTwips'] as number, spaceAfterTwips: v['spaceAfterTwips'] as number,
      lineTwentieths: v['lineTwentieths'] as number | undefined,
      indentLeftTwips: v['indentLeftTwips'] as number, indentRightTwips: v['indentRightTwips'] as number, indentFirstLineTwips: v['indentFirstLineTwips'] as number,
      shadingHex: v['shadingHex'] as string | undefined, borderTop: v['borderTop'] as string | undefined, borderBottom: v['borderBottom'] as string | undefined,
    } as StyleLook;
  };
  const flat = (o: Wire | undefined): Wire => {
    const r: Wire = { ...(o ?? {}) };
    delete r['t'];
    const src = r['src'];
    if (src && typeof src === 'object' && 'value' in src) r['src'] = (src as { value: unknown }).value;
    return r;
  };
  /** the run props of the selection's first character (or the caret's), raw (`propsAt`, not the toolbar's filled font) */
  const firstRun = (): Wire => {
    const el = c.el!;
    const { anchor, focus } = el.selection;
    const has = anchor.para !== focus.para || anchor.offset !== focus.offset;
    const start = cmpPos(anchor, focus) <= 0 ? anchor : focus;
    if (has) return flat(c.q<Wire>({ t: 'runPropsAt', pos: { para: start.para, offset: start.offset + 1 } }) ?? {});
    const st = c.s!.selectionState();
    if (st.pending) return flat(st.run as Wire);
    return flat(c.q<Wire>({ t: 'runPropsAt', pos: focus }) ?? {});
  };
  /** the stored props of paragraph `para` (not the toolbar's, whose alignment comes from the style) */
  const paraProps = (para: number): Wire => flat(c.q<Wire>({ t: 'paraPropsAt', para }) ?? {});
  /** `ds.meta(id)`'s display name: a listed style, or the character twin of a linked one */
  const nameOf = (id: string): string => {
    const w = c.q<WireStyle[]>({ t: 'listStyles' }) ?? [];
    return w.find((s) => s.id === id)?.name ?? w.find((s) => s.link === id && s.linkName)?.linkName ?? id;
  };

  return {
    listStyles: ((filter?: StyleFilter) => listStyles(filter)) as never,
    paragraphStyles: (() => listStyles('recommended').filter((s) => s.kind !== 'character').map((s) => ({ id: s.id, name: s.name }))) as never,
    styleFormatting: ((id: string) => (c.s ? look(id) : null)) as never,
    /** ED-303: the TS key uses its undo stacks' lengths; here the session's flags (the key changes when the list can) */
    stylesKey: (() => {
      const el = c.el;
      if (!el || !c.s) return '';
      const ed = c.ed as unknown as { canUndo(): boolean; canRedo(): boolean };
      return `${c.q<number>({ t: 'stylesVersion' }) ?? 0}:${c.q<number>({ t: 'editsSinceLoad' }) ?? 0}:${ed.canUndo() ? 1 : 0}:${ed.canRedo() ? 1 : 0}:${outerCtx(storyCtx(c))?.kind === 'hf' ? 1 : 0}:${uiLang(el)}`;
    }) as never,
    /** the wave-4 `currentStyles` (ED-23) with the document's default paragraph style instead of `Normal` (gate item 15) */
    currentStyles: (() => {
      const r = (c.ed as RustEditor).currentStyles();
      if (!c.s) return r;
      const own = c.s.selectionState().para.styleId;
      return own ? r : { ...r, paragraph: c.q<string>({ t: 'defaultParagraphStyle' }) ?? r.paragraph };
    }) as never,
    inspectStyle: (() => {
      const el = c.el;
      if (!el || !c.s) return { paragraph: { id: 'Normal', name: 'Normal', direct: {} }, text: { id: '', name: '', direct: {} } };
      const pp = paraProps(el.selection.focus.para);
      const run = firstRun();
      const pid = (pp['styleId'] as string | undefined) ?? c.q<string>({ t: 'defaultParagraphStyle' }) ?? 'Normal';
      const cid = (run['charStyle'] as string | undefined) ?? c.q<string>({ t: 'defaultCharacterStyle' }) ?? '';
      const pd: Wire = {};
      for (const k of ['alignment', 'spaceBeforeTwips', 'spaceAfterTwips', 'lineTwentieths', 'indentLeftTwips', 'indentRightTwips',
        'indentFirstLineTwips', 'numbering', 'borders', 'shadingHex', 'tabStops', 'pageBreakBefore']) {
        const v = pp[k];
        if (v !== undefined && v !== false) pd[k] = v;
      }
      const rd: Wire = {};
      for (const k of ['fontAscii', 'szHalfPt', 'bold', 'italic', 'underline', 'strike', 'doubleStrike', 'allCaps', 'smallCaps',
        'color', 'highlight', 'vertAlign', 'charSpacingTwips']) {
        const v = run[k];
        if (v === undefined || v === false) continue;
        if (run['link'] && ((k === 'underline' && !run['underlineStyle']) || (k === 'color' && v === '0563C1'))) continue;
        rd[k] = v;
      }
      return { paragraph: { id: pid, name: nameOf(pid), direct: pd }, text: { id: cid, name: nameOf(cid), direct: rd } };
    }) as never,
    /** ED-313: the TS `selectionFormat('linked')` composed from the raw first run props and the toolbar's paragraph props */
    selectionStyleFormat: (() => {
      if (!c.s || !c.el) return {};
      const run = firstRun();
      const { anchor, focus } = c.el.selection;
      const pp = paraProps(Math.min(anchor.para, focus.para));
      const f: Wire = {};
      if (run['fontAscii']) f['fontAscii'] = run['fontAscii'];
      if (run['szHalfPt'] !== undefined) f['szHalfPt'] = run['szHalfPt'];
      for (const k of ['bold', 'italic', 'strike', 'allCaps', 'smallCaps']) if (run[k]) f[k] = true;
      if (run['underline'] && !run['link']) f['underline'] = true;
      if (run['color'] && !(run['link'] && run['color'] === '0563C1')) f['color'] = run['color'];
      if (run['charSpacingTwips']) f['charSpacingTwips'] = run['charSpacingTwips'];
      if (pp['alignment'] && pp['alignment'] !== 'left') f['alignment'] = pp['alignment'];
      for (const k of ['spaceBeforeTwips', 'spaceAfterTwips', 'lineTwentieths']) if (pp[k] !== undefined) f[k] = pp[k];
      for (const k of ['indentLeftTwips', 'indentRightTwips', 'indentFirstLineTwips']) if (pp[k]) f[k] = pp[k];
      return f;
    }) as never,
    createStyle: ((opts: { name: string; type?: string; basedOn?: string; next?: string; quick?: boolean; autoUpdate?: boolean; format?: Record<string, unknown>; fromSelection?: boolean; apply?: boolean }) => {
      if (!c.s || c.locked()) return null;
      const before = new Set(all().map((s) => s.id));
      const spec: Wire = { name: opts.name };
      if (opts.type !== undefined) spec['kind'] = opts.type;
      for (const k of ['basedOn', 'next', 'quick', 'autoUpdate', 'fromSelection', 'apply'] as const) if (opts[k] !== undefined) spec[k] = opts[k];
      if (opts.format !== undefined) spec['format'] = chFormat(opts.format);
      const o = c.run({ t: 'style', v: { t: 'createStyle', spec } }, 'createStyle');
      if (!changed(o)) return null;
      return all().find((s) => s.defined && !before.has(s.id))?.id ?? null;
    }) as never,
    modifyStyle: ((id: string, opts: { name?: string; basedOn?: string; next?: string | null; quick?: boolean; autoUpdate?: boolean; format?: Record<string, unknown> }) => {
      const spec: Wire = { next: ch(opts.next), format: chFormat(opts.format) };
      for (const k of ['name', 'basedOn', 'quick', 'autoUpdate'] as const) if (opts[k] !== undefined) spec[k] = opts[k];
      return changed(c.run({ t: 'style', v: { t: 'modifyStyle', id, spec } }, 'modifyStyle'));
    }) as never,
    setDefaultFont: ((f: { fontFamily?: string; sizePt?: number; bold?: boolean; italic?: boolean; underline?: string; strike?: boolean; allCaps?: boolean; smallCaps?: boolean; color?: string | null; spacingTwips?: number }) => {
      const font: Wire = { color: f.color === undefined ? { t: 'keep' } : f.color === null ? { t: 'null' } : { t: 'val', v: String(f.color).replace(/^#/, '').toUpperCase() } };
      for (const k of ['fontFamily', 'sizePt', 'bold', 'italic', 'strike', 'allCaps', 'smallCaps', 'spacingTwips'] as const) if (f[k] !== undefined) font[k] = f[k];
      if (f.underline !== undefined) font['underline'] = f.underline !== 'none';
      return changed(c.run({ t: 'style', v: { t: 'setDefaultFont', font } }, 'setDefaultFont'));
    }) as never,
    updateStyleFromSelection: ((id?: string) => changed(c.run({ t: 'style', v: { t: 'updateStyleFromSelection', ...(id !== undefined ? { id } : {}) } }, 'updateStyleFromSelection'))) as never,
    renameStyle: ((id: string, name: string) => changed(c.run({ t: 'style', v: { t: 'renameStyle', id, name } }, 'renameStyle'))) as never,
    deleteStyle: ((id: string) => changed(c.run({ t: 'style', v: { t: 'deleteStyle', id } }, 'deleteStyle'))) as never,
    setStyleInGallery: ((id: string, on: boolean) => { c.run({ t: 'style', v: { t: 'setStyleInGallery', id, on } }, 'setStyleInGallery'); }) as never,
    clearParagraphStyle: (() => { c.run({ t: 'style', v: { t: 'clearParagraphStyle' } }, 'clearParagraphStyle'); }) as never,
    clearCharacterStyle: (() => { c.run({ t: 'style', v: { t: 'clearCharacterStyle' } }, 'clearCharacterStyle'); }) as never,
    /** the shell's families (the installed fonts) and the document's embedded ones (gate item 14) */
    knownFontFamilies: (() => {
      const el = c.el;
      const base = el ? (Object.getPrototypeOf(el) as { knownFontFamilies?: () => string[] }).knownFontFamilies?.call(el) ?? [] : [];
      let emb: string[] = [];
      try { emb = c.s?.embeddedFamilies() ?? []; } catch { /* an old module without the export */ }
      return [...new Set([...base, ...emb])].sort((a, b) => a.localeCompare(b));
    }) as never,
  };
}
