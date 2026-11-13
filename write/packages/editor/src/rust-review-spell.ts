// Frente R da leva 5 do engine="rust": a revisão ortográfica (registro uo-bindings.md "Wave 5 — R"). O motor é o
// mesmo do TS (shared/spell: Hunspell num Worker, `SpellChecker.shared`, carregado sob demanda pelo mesmo import()); os
// trechos por idioma vêm da sessão (`paraRuns`: o idioma e o `noProof` de cada run, o que não se revisa) e o cache por
// parágrafo é o `ParagraphSpell` do TS refeito sobre o índice do parágrafo da sessão (ED-307: ~60 linhas repetidas de
// spell.ts, cujo cache é por objeto `Paragraph`). O sublinhado é adorno de tela (o gancho de pintura), nunca no PDF.
import { spellcheckAvailable } from '@universal-office/spell-sources';
import { misspellingAt, wavePath, type Misspelling, type SpellEngine, type SpellSpan, DEFAULT_SPELL_LANG } from './spell.js';
import { Ctx, charExtent, onLine, storyLines, type Pos } from './rust-review-core.js';
import { runsOf } from './rust-review-revisions.js';

type Handlers = Record<string, (...args: never[]) => unknown>;

/** O `spellSpans` de spell.ts sobre o texto e os runs da sessão. */
export function sessionSpellSpans(text: string, runs: { a: number; z: number; spell?: boolean; lang: string }[]): SpellSpan[] {
  if (!/\p{L}/u.test(text)) return [];
  const byLang = new Map<string, string[]>();
  for (const r of runs) {
    if (!r.spell) continue;
    let arr = byLang.get(r.lang);
    if (!arr) { arr = Array.from({ length: text.length }, () => ' '); byLang.set(r.lang, arr); }
    for (let k = r.a; k < r.z && k < text.length; k++) arr[k] = text[k]!;
  }
  return [...byLang].map(([lang, chars]) => ({ lang, text: chars.join('') }));
}

const keyOf = (spans: SpellSpan[]): string => spans.map((s) => `${s.lang}\u0001${s.text}`).join('\u0002');

/** O cache dos erros por parágrafo (o `ParagraphSpell` do TS, chaveado pelo conteúdo revisável). */
export class SessionSpell {
  private byKey = new Map<string, Misspelling[]>();
  private queue = new Map<string, SpellSpan[]>();
  private timer: ReturnType<typeof setTimeout> | null = null;
  private generation = 0;
  private offChange: () => void;
  /** released: no check reaches the shared checker again (a call after its last `dispose` would start a new engine) */
  private disposed = false;
  /** pending batches (the harness waits for 0) */
  pending = 0;

  constructor(private engine: SpellEngine, private onUpdate: () => void, private delay = 120) {
    this.offChange = engine.onChange(() => this.invalidate());
  }

  invalidate(): void {
    this.generation++;
    this.byKey.clear();
    this.onUpdate();
  }

  dispose(): void {
    this.disposed = true;
    this.generation++;
    this.offChange();
    if (this.timer) { clearTimeout(this.timer); this.timer = null; this.pending--; }
    this.queue.clear();
    this.byKey.clear();
  }

  /** Erros dos trechos (síncrono): o guardado, ou agenda a revisão e devolve undefined. */
  get(spans: SpellSpan[]): Misspelling[] | undefined {
    if (!spans.length) return [];
    const key = keyOf(spans);
    const known = this.byKey.get(key);
    if (known) return known;
    this.queue.set(key, spans);
    this.schedule();
    return undefined;
  }

  async check(spans: SpellSpan[]): Promise<Misspelling[]> {
    if (!spans.length) return [];
    const key = keyOf(spans);
    const known = this.byKey.get(key);
    if (known) return known;
    const list = await this.run(spans);
    this.store(key, list);
    return list;
  }

  private schedule(): void {
    if (this.timer || this.disposed) return;
    this.pending++;
    this.timer = setTimeout(() => { this.timer = null; void this.flush().finally(() => { this.pending--; }); }, this.delay);
  }

  private async flush(): Promise<void> {
    const batch = [...this.queue];
    this.queue.clear();
    const gen = this.generation;
    let changedAny = false;
    for (const [key, spans] of batch) {
      let list: Misspelling[];
      try { list = await this.run(spans); } catch { continue; }
      if (gen !== this.generation) return;
      this.store(key, list);
      changedAny = true;
    }
    if (changedAny) this.onUpdate();
  }

  private store(key: string, list: Misspelling[]): void {
    if (this.byKey.size > 4000) this.byKey.clear();
    this.byKey.set(key, list);
  }

  private async run(spans: SpellSpan[]): Promise<Misspelling[]> {
    const out: Misspelling[] = [];
    for (const s of spans) {
      if (this.disposed) throw new Error('SessionSpell disposed');
      const dict = this.engine.resolve(s.lang);
      if (!dict) continue;
      for (const m of await this.engine.checkText(s.text, dict.id)) out.push({ ...m, lang: dict.id });
    }
    return out.sort((a, b) => a.start - b.start);
  }
}

interface SpellHost { spellOff?: boolean; protectedMode: boolean; view: string; isConnected: boolean; host?: { userDictionary?: unknown } | null; hasAttribute(n: string): boolean }

export interface SpellState { spell: SessionSpell | null; engine: SpellEngine | null; loading: boolean; raf: number }

export function spellHandlers(c: Ctx): { handlers: Handlers; paint: (ctx: CanvasRenderingContext2D, page: number) => void; state: SpellState; dispose: () => void } {
  const state: SpellState = { spell: null, engine: null, loading: false, raf: 0 };
  /** the TS `disconnectedCallback` (universal-write.ts 1747–1750): the cache's listener and the shared checker's reference
   *  are released (the Hunspell worker ends with the last editor); the next paint after a reconnect loads them again */
  const dispose = (): void => {
    if (state.raf) { cancelAnimationFrame(state.raf); state.raf = 0; }
    state.spell?.dispose();
    state.engine?.dispose();
    state.spell = null;
    state.engine = null;
  };
  const host = (): (SpellHost & HTMLElement) | null => c.el as unknown as (SpellHost & HTMLElement) | null;
  const active = (): boolean => {
    const el = host();
    return !!el && !el.spellOff && !el.hasAttribute('nospellcheck') && !el.protectedMode && el.view !== 'read';
  };
  const fallback = (): string => c.el?.spellDefaultLang() ?? DEFAULT_SPELL_LANG;
  const repaint = (): void => {
    if (state.raf) return;
    state.raf = requestAnimationFrame(() => { state.raf = 0; if (c.dl && host()?.isConnected) c.api?.paint(); });
  };
  const ensure = (): SessionSpell | null => {
    if (state.spell || state.loading) return state.spell;
    if (!spellcheckAvailable()) return null;
    state.loading = true;
    void import('@universal-office/spell').then(({ SpellChecker }) => {
      const el = host();
      if (!el?.isConnected) return;
      const store = el.host?.userDictionary;
      const engine = SpellChecker.shared(store ? { store } as never : undefined) as unknown as SpellEngine;
      state.engine = engine;
      state.spell = new SessionSpell(engine, repaint);
      repaint();
    }).catch(() => { /* sem o motor: sem revisão */ }).finally(() => { state.loading = false; });
    return null;
  };
  const spansOf = (para: number): SpellSpan[] => {
    const s = c.s;
    if (!s) return [];
    const text = s.paraText(para) ?? '';
    if (!/\p{L}/u.test(text)) return [];
    return sessionSpellSpans(text, runsOf(c, para, fallback()));
  };
  const getPara = (para: number): Misspelling[] | undefined => ensure()?.get(spansOf(para));

  const paint = (ctx: CanvasRenderingContext2D, page: number): void => {
    const el = c.el as unknown as { paintEngineSpelling?: (ctx: CanvasRenderingContext2D, pg: number) => void } | null;
    el?.paintEngineSpelling?.(ctx, page);
  };
  /** the TS `paintEngineSpelling` (16913) over EVERY story's lines (body, cells, headers/footers, notes) and the text boxes'
   *  content in their frame, whatever the edit scope (wave 7c, V: ED-304 closed) */
  const paintEngineSpelling = (ctx: CanvasRenderingContext2D, page: number): void => {
    if (!active() || !c.s || !c.dl) return;
    const spell = ensure();
    if (!spell) return;
    ctx.save();
    ctx.strokeStyle = c.el?.pageDark ? '#ff6b6b' : '#e81123';
    ctx.lineWidth = 11;
    const cache = new Map<string, Misspelling[] | undefined>();
    for (const l of storyLines(c, page, fallback())) {
      let list = cache.get(l.key);
      if (!cache.has(l.key)) {
        list = /\p{L}/u.test(l.text) ? spell.get(sessionSpellSpans(l.text, l.runs.map((r) => ({ a: r.a, z: r.z, spell: r.sp, lang: r.lang })))) : [];
        cache.set(l.key, list);
      }
      if (!list?.length) continue;
      onLine(ctx, l, () => {
        for (const m of list!) {
          const ext = charExtent(l, m.start, m.end);
          if (!ext) continue;
          wavePath(ctx, ext[0], ext[1], l.baseline + 50);
          ctx.stroke();
        }
      });
    }
    ctx.restore();
  };

  const handlers: Handlers = {
    paintEngineSpelling: paintEngineSpelling as never,
    toggleSpellCheck: ((on?: boolean) => {
      const el = host();
      if (!el) return;
      el.spellOff = on === undefined ? !el.spellOff : !on;
      c.api?.paint();
      c.el?.chromeSync?.();
    }) as never,
    spellingAtCaret: (() => {
      const el = c.el;
      if (!el || !c.s || !active()) return null;
      const pos = el.selection.focus;
      const m = misspellingAt(getPara(pos.para), pos.offset);
      return m ? { ...m, para: pos.para } : null;
    }) as never,
    spellingSuggestions: (async (m: Misspelling, limit = 5) => {
      if (!state.engine) return [];
      return (await state.engine.suggest(m.word, m.lang)).slice(0, limit);
    }) as never,
    /** ED-306: the session's `type` over the selected word (the TS: trackedDelete/deleteRange + insertTyped) */
    replaceSpelling: ((m: Misspelling & { para: number }, text: string) => {
      const s = c.s;
      if (!s || !c.api || c.locked()) return false;
      if ((s.paraText(m.para) ?? '').slice(m.start, m.end) !== m.word) return false;
      // the session's Replace over the selected word (its `replaceRange`: delete + `insertTyped` with the word's props, the
      // runs merged as the TS `deleteRange` + `insertTyped`; a plain `type` over the selection splits the run): match case,
      // `^` escaped, then the caret at the end of the new text (the TS)
      s.setSelection({ para: m.para, offset: m.start }, { para: m.para, offset: m.end });
      const d = s.delete();
      if (d?.t !== 'done') { c.api.apply(d, 'replaceSpelling'); return false; }
      return c.api.apply(s.type(text), 'replaceSpelling');
    }) as never,
    ignoreSpelling: ((word: string) => { state.engine?.ignore(word); }) as never,
    addToDictionary: (async (word: string) => { await state.engine?.add(word); }) as never,
    nextMisspelling: (async (skip: ReadonlySet<string> = new Set()) => {
      const s = c.s;
      const el = c.el;
      if (!s || !el || !spellcheckAvailable()) return null;
      ensure();
      for (let i = 0; i < 100 && !state.spell; i++) await new Promise((r) => setTimeout(r, 30));
      const spell = state.spell;
      if (!spell) return null;
      const n = s.storyInfo().paras;
      const { anchor, focus } = el.selection;
      const startP = Math.min(focus.para, n - 1);
      const end: Pos = anchor.para > focus.para || (anchor.para === focus.para && anchor.offset > focus.offset) ? anchor : focus;
      const startO = end.offset;
      for (let k = 0; k <= n; k++) {
        const pi = (startP + k) % n;
        const list = await spell.check(spansOf(pi));
        if (c.s !== s) return null;
        const m = list.find((x) => (k > 0 || pi !== startP || x.start >= startO) && !skip.has(`${pi}:${x.start}:${x.word}`));
        if (m) {
          c.api?.apply(s.setSelection({ para: pi, offset: m.start }, { para: pi, offset: m.end }), 'nextMisspelling');
          return { ...m, para: pi };
        }
      }
      return null;
    }) as never,
    /** the harness: the spelling cache's misspellings of a paragraph (undefined = not checked yet) and the pending batches */
    __spellProbe: ((para: number) => ({ list: state.spell ? getPara(para) : undefined, pending: state.spell?.pending ?? -1, loaded: !!state.spell })) as never,
  };
  return { handlers, paint, state, dispose };
}
