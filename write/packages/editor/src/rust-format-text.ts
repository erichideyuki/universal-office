// Frente A1 da leva 7 do engine="rust": Fonte e Parágrafo (registro uo-bindings.md "Wave 7 — A1"). O diálogo Fonte, os
// efeitos de texto, OpenType, a cor do sublinhado, o degradê, as cores do tema, o Pincel de Formatação; os diálogos
// Parágrafo, Tabulação e Bordas e Sombreamento, o botão Bordas e a linha horizontal; Selecionar Texto Semelhante; a
// Hifenização Manual; o localizar/substituir simples, a contagem, os links, os inlines de plugin, os campos externos e os
// estilos internos. Cada handler é o método público do TS com o MESMO nome; os comandos são os da sessão (FormatCmd,
// ParagraphCmd, TextLayoutCmd, ReferenceCmd) ou os nativos de shell_format.rs.
import { validBookmarkName } from './references.js';
import type { SessionCommand } from '@universal-office/write-session';
import { dedupeTabs, tabInfo } from './navigation.js';
import { FormatCtx, sync, type Handlers } from './rust-format-core.js';

type ThemeRef = { themeColor: string; tint?: number; shade?: number };
type Color = string | ThemeRef;

/** `string | ThemeColorRef` → the wire `ColorArg` (RustEditor.setColor's mapping). */
export function colorArg(c: Color): { t: 'hex'; v: string } | ({ t: 'theme' } & ThemeRef) {
  return typeof c === 'string' ? { t: 'hex', v: c.replace(/^#/, '').toUpperCase() } : { t: 'theme', ...c };
}

/** `undefined` absent, `null` = Automatic (`{}`), a colour = `{ value }` (the wire `OptColorArg`). */
function optColor(c: Color | null | undefined): Record<string, unknown> | undefined {
  if (c === undefined) return undefined;
  return c === null ? {} : { value: colorArg(c) };
}

const FX_KEYS = ['fill', 'outline', 'shadow', 'reflection', 'glow', 'scene3d', 'props3d'] as const;

/** The colour modifiers of the effects between the TS shape (`mods: [name, value][]`) and the wire's (`{ f0, f1 }`), deep. */
export function fxToWire(v: unknown): unknown {
  if (Array.isArray(v)) return v.map(fxToWire);
  if (!v || typeof v !== 'object') return v;
  const o: Record<string, unknown> = {};
  for (const [k, x] of Object.entries(v)) {
    o[k] = k === 'mods' && Array.isArray(x) ? x.map((m) => (Array.isArray(m) ? { f0: m[0], f1: m[1] } : m)) : fxToWire(x);
  }
  return o;
}
export function fxFromWire(v: unknown): unknown {
  if (Array.isArray(v)) return v.map(fxFromWire);
  if (!v || typeof v !== 'object') return v;
  const o: Record<string, unknown> = {};
  for (const [k, x] of Object.entries(v)) {
    o[k] = k === 'mods' && Array.isArray(x) ? x.map((m) => (m && typeof m === 'object' && 'f0' in m ? [(m as { f0: unknown }).f0, (m as { f1: unknown }).f1] : m)) : fxFromWire(x);
  }
  return o;
}
const OT_KEYS = ['ligatures', 'numForm', 'numSpacing', 'stylisticSets', 'cntxtAlts'] as const;

export function textHandlers(c: FormatCtx): Handlers {
  const cmd = (v: SessionCommand, what: string): void => { c.run(v, what); };
  const fmt = (v: Record<string, unknown>, what: string): void => cmd({ t: 'format', v } as SessionCommand, what);
  const para = (v: Record<string, unknown>, what: string): void => cmd({ t: 'paragraph', v } as SessionCommand, what);
  const btn = (): string => { const el = c.el as unknown as { underlineButtonStyle?: () => string } | null; return el?.underlineButtonStyle?.() ?? 'single'; };
  const runState = (): Record<string, unknown> => {
    const el = c.el as unknown as { getSelectionState?: () => { run?: Record<string, unknown> } } | null;
    return el?.getSelectionState?.()?.run ?? {};
  };
  const F10 = (): { t: string; v?: unknown; value?: unknown } | null => c.q<{ value?: { t: string; v?: unknown; value?: unknown } }>({ t: 'referencesResult' })?.value ?? null;
  let statsCache: { dl: unknown; words: number; chars: number } | null = null;
  const F9 = (): { t: string; v?: unknown } | null => c.q<{ value?: { t: string; v?: unknown } }>({ t: 'navigationResult' })?.value ?? null;

  return {
    // ── Fonte ──
    setUnderlineColor: ((color: Color | null) => {
      fmt({ t: 'setUnderlineColor', ...(color === null || color === undefined ? {} : { color: colorArg(color) }), button: btn() }, 'setUnderlineColor');
    }) as never,
    setTextEffects: ((patch: Record<string, unknown>) => {
      const parts = Object.entries(patch ?? {}).filter(([k]) => (FX_KEYS as readonly string[]).includes(k))
        .map(([k, v]) => (v === null || v === undefined ? { t: k } : { t: k, value: fxToWire(v) }));
      fmt({ t: 'setTextEffects', patch: parts }, 'setTextEffects');
    }) as never,
    applyTextEffectPreset: ((index: number | null) => {
      fmt({ t: 'applyTextEffectPreset', ...(index === null || index === undefined ? {} : { index }) }, 'applyTextEffectPreset');
    }) as never,
    setOpenType: ((patch: Record<string, unknown>) => {
      const parts = Object.entries(patch ?? {}).filter(([k]) => (OT_KEYS as readonly string[]).includes(k))
        .map(([k, v]) => (v === null || v === undefined ? { t: k } : { t: k, value: v }));
      fmt({ t: 'setOpenType', patch: parts }, 'setOpenType');
    }) as never,
    setTextGradient: ((v: { angleDeg: number; light: boolean } | null) => {
      fmt({ t: 'setTextGradient', ...(v ? { v: { angleDeg: v.angleDeg, light: !!v.light } } : {}) }, 'setTextGradient');
    }) as never,
    setFontFormat: ((f: Record<string, unknown>) => {
      if (!f) return;
      const el = c.el as unknown as { ensureFontLoaded?: (n: string) => Promise<unknown> } | null;
      if (typeof f['fontFamily'] === 'string') void el?.ensureFontLoaded?.(f['fontFamily']);
      // the TS keeps the Underline button's style (`this.ulButton = f.underline`)
      if (typeof f['underline'] === 'string' && f['underline'] !== 'none') c.api?.setUlButton(f['underline']);
      const w: Record<string, unknown> = {};
      for (const k of ['fontFamily', 'sizePt', 'bold', 'italic', 'underline', 'strike', 'doubleStrike', 'superscript', 'subscript', 'smallCaps', 'allCaps', 'hidden', 'scalePct', 'spacingTwips', 'positionHalfPt', 'kernMinHalfPt']) {
        if (f[k] !== undefined) w[k] = f[k];
      }
      const uc = optColor(f['underlineColor'] as Color | null | undefined), col = optColor(f['color'] as Color | null | undefined);
      if (uc) w['underlineColor'] = uc;
      if (col) w['color'] = col;
      fmt({ t: 'setFontFormat', f: w }, 'setFontFormat');
    }) as never,
    /** `textEffects()`: the effects and typography at the caret (pending props first, as the toolbar state) */
    textEffects: (() => {
      const r = runState();
      const fx = r['textFx'], ot = r['openType'];
      return { ...(fx ? { fx: fxFromWire(structuredClone(fx)) } : {}), ...(ot ? { ot: { ...(ot as object) } } : {}) };
    }) as never,
    documentThemeColors: (() => c.json<Record<string, string>>(5) ?? {
      dk1: '000000', lt1: 'FFFFFF', dk2: '0E2841', lt2: 'E8E8E8', accent1: '156082', accent2: 'E97132',
      accent3: '196B24', accent4: '0F9ED5', accent5: 'A02B93', accent6: '4EA72E', hlink: '467886', folHlink: '96607D',
    }) as never,

    // ── Pincel de Formatação (o pincel fica no handle da sessão: CopyFormat; a trava e o estado na casca, como no TS) ──
    copyFormat: ((sticky = false) => {
      if (!c.s) return;
      const ok = c.q<boolean>({ t: 'copyFormat' });
      c.st.painter = ok !== false;
      c.st.painterLock = !!sticky;
      sync(c);
    }) as never,
    painterActive: (() => c.st.painter) as never,
    painterSticky: (() => c.st.painter && c.st.painterLock) as never,
    cancelFormatPainter: (() => {
      if (!c.st.painter) return;
      c.st.painter = false;
      c.st.painterLock = false;
      sync(c);
    }) as never,

    // ── Parágrafo ──
    getParagraphIndents: (() => {
      const a = c.q<{ left: number; right: number; firstLine: number }>({ t: 'paragraphIndents' });
      return a ? { left: a.left, right: a.right, firstLine: a.firstLine } : { left: 0, right: 0, firstLine: 0 };
    }) as never,
    setParagraphIndents: ((ind: Record<string, number | undefined>) => {
      const p: Record<string, number> = {};
      for (const k of ['left', 'right', 'firstLine']) if (ind?.[k] !== undefined) p[k] = ind[k]!;
      para({ t: 'setParagraphIndents', ind: p }, 'setParagraphIndents');
    }) as never,
    setSpecialIndent: ((kind: string) => para({ t: 'setSpecialIndent', kind }, 'setSpecialIndent')) as never,
    setIndentRight: ((twips: number | undefined) => para({ t: 'setIndentRight', ...(twips !== undefined ? { twips } : {}) }, 'setIndentRight')) as never,
    getTabStops: (() => {
      const tabs = (c.q<{ pos: number; align: string; leader?: string }[]>({ t: 'paragraphTabStops' }) ?? [])
        .map((t) => ({ pos: t.pos, align: t.align, ...(t.leader !== undefined ? { leader: t.leader } : {}) }));
      return { tabs: dedupeTabs(tabs as never), defaultTabTwips: c.json<number>(4) ?? 708 };
    }) as never,
    setTabStops: ((tabs: { pos: number; align: string; leader?: string }[], only?: { add?: { pos: number; align: string; leader?: string }[]; remove?: number[] }) => {
      const t = (x: { pos: number; align: string; leader?: string }): Record<string, unknown> => ({ pos: x.pos, align: x.align, ...(x.leader !== undefined ? { leader: x.leader } : {}) });
      const o = only ? { ...(only.add ? { add: only.add.map(t) } : {}), ...(only.remove ? { remove: only.remove } : {}) } : undefined;
      para({ t: 'setTabStops', tabs: (tabs ?? []).map(t), ...(o ? { only: o } : {}) }, 'setTabStops');
    }) as never,
    getDefaultTabStop: (() => c.json<number>(4) ?? 708) as never,
    setDefaultTabStop: ((twips: number) => cmd({ t: 'textLayout', v: { t: 'setDefaultTabStop', twips } } as SessionCommand, 'setDefaultTabStop')) as never,
    getParagraphFlow: (() => (c.s ? c.json(1) : null)) as never,
    setParagraphFlow: ((f: Record<string, unknown>) => {
      const w: Record<string, unknown> = {};
      for (const k of ['widowControl', 'keepNext', 'keepLines', 'pageBreakBefore', 'suppressLineNumbers', 'suppressAutoHyphens', 'textboxTightWrap']) if (f?.[k] !== undefined) w[k] = f[k];
      para({ t: 'setParagraphFlow', f: w }, 'setParagraphFlow');
    }) as never,
    getParagraphLayout: (() => (c.s ? c.json(2) : null)) as never,
    setParagraphLayout: ((v: { left?: number; right?: number; before?: number | 'auto'; after?: number | 'auto' }) => {
      const sp = (x: number | 'auto'): Record<string, unknown> => (x === 'auto' ? { t: 'auto' } : { t: 'twips', v: x });
      const w: Record<string, unknown> = {};
      if (v?.left !== undefined) w['left'] = v.left;
      if (v?.right !== undefined) w['right'] = v.right;
      if (v?.before !== undefined) w['before'] = sp(v.before);
      if (v?.after !== undefined) w['after'] = sp(v.after);
      para({ t: 'setParagraphLayout', v: w }, 'setParagraphLayout');
    }) as never,
    getParagraphBorders: (() => c.json<Record<string, unknown>>(3) ?? {}) as never,
    paragraphBorderSides: (() => c.q<string[]>({ t: 'paragraphBorderSides' }) ?? []) as never,
    setParagraphBorders: ((sides: Record<string, Record<string, unknown> | null | undefined> | null) => {
      if (!sides) { para({ t: 'setParagraphBorders' }, 'setParagraphBorders'); return; }
      const list = Object.entries(sides).filter(([, b]) => b !== undefined).map(([side, b]) => (b === null ? { f0: side } : { f0: side, f1: b }));
      para({ t: 'setParagraphBorders', sides: list }, 'setParagraphBorders');
    }) as never,
    paragraphBorderPreset: ((kind: string) => para({ t: 'paragraphBorderPreset', kind }, 'paragraphBorderPreset')) as never,
    toggleBorder: (() => para({ t: 'toggleBorder' }, 'toggleBorder')) as never,
    insertHorizontalLine: (() => para({ t: 'insertHorizontalLine' }, 'insertHorizontalLine')) as never,

    // ── Selecionar Texto com Formatação Semelhante ──
    selectSimilarFormatting: (() => {
      if (!c.s) return 0;
      const o = c.run({ t: 'format', v: { t: 'selectSimilarFormatting' } } as SessionCommand, 'selectSimilarFormatting');
      if (!o || o.t !== 'done') return 0;
      const ranges = c.q<unknown[]>({ t: 'selectionRanges' }) ?? [];
      return Math.max(ranges.length, 1);
    }) as never,

    // ── Hifenização Manual (a palavra proposta pela sessão, os padrões do ambiente da página) ──
    nextManualHyphenation: (async () => {
      const s = c.s, api = c.api, fb = c.fb;
      if (!s || !api || !fb) return null;
      // the TS `loadHyphenation(langs)`: the patterns of the engine languages the page environment lacks, through the module's
      // loader, then the session reads the environment's registry (F10-D3)
      const env = api.pageEnv;
      if (env) {
        try {
          for (const id of s.hyphenationPatterns(env.inner, ['pt-BR', 'en', 'es'])) {
            const text = await api.module.patternText(id);
            if (text) env.registerHyphenation(text);
          }
        } catch { /* no patterns: only the optional hyphens, as the TS */ }
        if (c.s !== s) return null;
        fb.hyphenation(s, env.inner as never);
      }
      c.st.hyph = true;
      c.run({ t: 'reference', v: { t: 'nextManualHyphenation' } } as SessionCommand, 'nextManualHyphenation');
      const r = F10();
      const v = r?.t === 'hyphenation' ? (r as { value?: { word: string; lang: string; points: number[]; suggested: number; limit: number } }).value : undefined;
      if (!v) { c.st.hyph = false; return null; }
      return { word: v.word, lang: v.lang, points: [...v.points], suggested: v.suggested, limit: v.limit };
    }) as never,
    applyManualHyphenation: ((letters: number) => cmd({ t: 'reference', v: { t: 'applyManualHyphenation', letters } } as SessionCommand, 'applyManualHyphenation')) as never,
    skipManualHyphenation: (() => cmd({ t: 'reference', v: { t: 'skipManualHyphenation' } } as SessionCommand, 'skipManualHyphenation')) as never,
    endManualHyphenation: (() => { c.st.hyph = false; cmd({ t: 'reference', v: { t: 'endManualHyphenation' } } as SessionCommand, 'endManualHyphenation'); }) as never,

    // ── localizar / substituir simples (universal-write.ts 4741/4764: o texto dos parágrafos do modelo em edição) ──
    findNext: ((needle: string) => {
      const s = c.s, el = c.el;
      if (!s || !el || !needle) return false;
      const lower = String(needle).toLowerCase();
      const { para: startP, offset: startO } = el.selection.focus;
      const n = s.storyInfo().paras;
      for (let k = 0; k <= n; k++) {
        const pi = (startP + k) % n;
        const text = (s.paraText(pi) ?? '').toLowerCase();
        const idx = text.indexOf(lower, k === 0 ? startO : 0);
        if (idx >= 0) {
          // the TS `this.selection = …; refreshSelection(); scrollCaretIntoView()`
          c.api?.apply(s.setSelection({ para: pi, offset: idx }, { para: pi, offset: idx + needle.length }), 'findNext');
          return true;
        }
      }
      return false;
    }) as never,
    replaceAll: ((needle: string, replacement: string) => {
      if (!c.s || !needle) return 0;
      c.exec(3, [String(needle), String(replacement ?? '')], 'replaceAll');
      const r = F9();
      return r?.t === 'count' ? (r.v as number) : 0;
    }) as never,
    countMatches: ((q: Record<string, unknown>, o: { within?: string } = {}) => {
      if (!c.s || !q) return 0;
      return c.q<number>({ t: 'countMatches', query: { ...q, text: String(q['text'] ?? '') }, within: o.within ?? 'document' }) ?? 0;
    }) as never,
    getLinks: (() => c.json<unknown[]>(7) ?? []) as never,
    getCustomInlines: ((type?: string) => c.json<unknown[]>(8, type === undefined ? [] : [type]) ?? []) as never,
    hasExternalFields: (() => c.json<boolean>(6) ?? false) as never,
    styleKeyOf: ((id: string) => c.json<string>(9, [String(id)]) ?? String(id)) as never,
    ensureBuiltinStyle: ((name: string) => {
      if (!c.s) return name;
      c.exec(2, [String(name)], 'ensureBuiltinStyle');
      return c.json<string>(10, [String(name)]) ?? name;
    }) as never,
    insertCustomInline: ((type: string, data: unknown, options: { fallbackText?: string } = {}) => {
      const el = c.el as unknown as { inlineRenderers?: Map<string, { renderer?: { toText?: (d: unknown) => string } }> } | null;
      if (!c.s || c.locked()) return;
      const renderer = el?.inlineRenderers?.get(type)?.renderer;
      const fallbackText = options?.fallbackText ?? renderer?.toText?.(data) ?? String(data ?? '');
      let json = 'null';
      try { json = JSON.stringify(data) ?? 'null'; } catch { json = 'null'; }
      // the session's event carries only the type: the TS detail { type, data, fallbackText } is dispatched here
      const s = c.s, fb = c.fb, api = c.api;
      if (!fb || !api) return;
      const o = fb.exec(s, 1, [String(type), json, fallbackText]);
      if (o && o.t === 'done') o.events = o.events.filter((e) => e.t !== 'uw-custom-inline-inserted');
      if (!api.apply(o, 'insertCustomInline')) return;
      c.el?.dispatchEvent(new CustomEvent('uw-custom-inline-inserted', { detail: { type, data, fallbackText } }));
    }) as never,

    /** `stats()` (8919): pages, page and paragraph of the shell (RustEditor), the words and characters over `plainTextSlice` (a
     *  plugin inline counts as its alternative text, note marks not: the session's plain text, wave 7 — ED-20 superseded) */
    stats: (() => {
      const base = (c.ed as unknown as { stats(): { pages: number; words: number; chars: number; paragraph: number; page: number } }).stats();
      const dl = c.dl;
      if (!statsCache || statsCache.dl !== dl) {
        let words = 0, chars = 0;
        for (const t2 of c.json<string[]>(16) ?? []) { chars += t2.length; words += (t2.match(/\S+/g) ?? []).length; }
        statsCache = { dl, words, chars };
      }
      // the Web, Draft and Outline views: one logical page (the TS continuous layout, 12804: `pageCount = 1`; fix round 2)
      const cont = continuousView(c);
      return { ...base, ...(cont ? { pages: 1, page: 1 } : {}), words: statsCache.words, chars: statsCache.chars };
    }) as never,
    /** `layoutPageCount()` (18539): the engine layout's pages; the TS Web/Draft/Outline views have no engine layout (0) */
    layoutPageCount: (() => (continuousView(c) ? 0 : (c.ed as unknown as { layoutPageCount(): number }).layoutPageCount())) as never,
    /** `currentPage()` (8831): one logical page in the continuous views */
    currentPage: (() => (continuousView(c) ? 1 : (c.ed as unknown as { currentPage(): number }).currentPage())) as never,

    // ── consultas que o TS responde do estado da casca (o código TS no elemento) ──
    hasAuthor: (() => !!c.ts('hasAuthor', [])) as never,
    validBookmarkName: ((name: string) => validBookmarkName(name)) as never,
    tocEntrySuggestion: (() => {
      const el = c.el as unknown as { selectedText?: () => string } | null;
      return (el?.selectedText?.() ?? '').replace(/[\r\n\t\u000B\f]+/g, ' ').trim();
    }) as never,
  };
}

/** The element shows the Web, Draft or Outline view (the session's continuous transforms). */
function continuousView(c: FormatCtx): boolean {
  const v = (c.el as unknown as { view?: string } | null)?.view;
  return v === 'web' || v === 'draft' || v === 'outline';
}

/** `tabInfo` of an engine line's `pfmt` tab (the ruler). */
export function pfmtTab(t: { val: string; pos: number; leader?: string }): ReturnType<typeof tabInfo> {
  return tabInfo(t as never);
}
