// Frente R da leva 5 do engine="rust": controle de alterações e comentários sobre a sessão (registro
// uo-bindings.md "Wave 5 — R"). Cada método é o do TS (universal-write.ts 7829–8356) com a mesma forma de resposta;
// o que a sessão faz (aceitar, rejeitar, marcar, comentar) é dela, e o shell só traduz as formas e rola a tela.
import type { ExecOutcome } from '@universal-office/write-session';
import { uiLang } from '@universal-office/ui';
import { formatDescription, type MarkupOptions, type WriteRevision } from './revisions.js';
import { Ctx, changed, linePx, paraLines, scrollToPos, type Pos } from './rust-review-core.js';

type Handlers = Record<string, (...args: never[]) => unknown>;

interface WireSpan {
  kind: string; author: string; date?: string; dateUtc?: string; move?: boolean; mark?: boolean;
  para: number; endPara?: number; start: number; end: number; text: string;
  old?: Record<string, unknown> & { t: string }; now?: Record<string, unknown> & { t: string };
}
interface WireEntry { span: WireSpan; story: WriteRevision['story']; loc: { t: string } }
interface WireMarkup { insDel?: boolean; formatting?: boolean; comments?: boolean; balloons: MarkupOptions['balloons']; authors?: string[] }

const cmpPos = (a: Pos, b: Pos): number => a.para - b.para || a.offset - b.offset;

/** As props de um lado da revisão de formatação na forma do TS (sem a etiqueta do fio; `src` achatado). */
function propsOf(p: (Record<string, unknown> & { t: string }) | undefined): Record<string, unknown> | undefined {
  if (!p) return undefined;
  const { t: _t, ...rest } = p;
  const src = rest['src'];
  if (src && typeof src === 'object' && 'value' in src) rest['src'] = (src as { value: unknown }).value;
  return rest;
}

/** `uw-markup` da sessão viaja sem detalhe: o TS manda `{ mode }`. */
function withMarkupDetail(o: ExecOutcome | null, mode: string): ExecOutcome | null {
  if (!o || o.t !== 'done') return o;
  return { ...o, events: o.events.map((e) => (e.t === 'uw-markup' ? { ...e, mode } : e)) } as ExecOutcome;
}

/**
 * The element's author in the session. The TS reads `this.author` live for every new revision (`newRev`) and comment;
 * the session keeps the name it was given. So the name is pushed (`settings.setAuthor`, not an edit: no undo step, no
 * dirty flag) whenever it can have changed: after each open (decision 6), when the `author` or `lang` attribute (the
 * default name is per interface language) or the `author` property changes, and before each key or click (the same task
 * as an attribute change: the observer's callback is a microtask later).
 */
export function authorSync(c: Ctx): { sync: () => void; adopt: () => void; install: () => void } {
  let pushed: string | null = null;
  const sync = (): void => {
    const el = c.el;
    if (!el || !c.s) return;
    const a = el.author;
    if (a === pushed) return;
    pushed = a;
    c.raw({ t: 'settings', v: { t: 'setAuthor', name: a } });
  };
  const adopt = (): void => { pushed = null; sync(); };
  const install = (): void => {
    const el = c.el as unknown as HTMLElement | null;
    if (!el) return;
    try { new MutationObserver(sync).observe(el, { attributes: true, attributeFilter: ['author', 'lang'] }); } catch { /* no DOM (tests) */ }
    // the `author` property: the prototype's accessor, then the push
    let proto: object | null = Object.getPrototypeOf(el);
    let d: PropertyDescriptor | undefined;
    while (proto && !(d = Object.getOwnPropertyDescriptor(proto, 'author'))) proto = Object.getPrototypeOf(proto);
    const get = d?.get, set = d?.set;
    if (!get || !set || Object.getOwnPropertyDescriptor(el, 'author')) return;
    Object.defineProperty(el, 'author', {
      configurable: true,
      enumerable: !!d?.enumerable,
      get: () => get.call(el) as string,
      set: (v: string) => { set.call(el, v); sync(); },
    });
  };
  return { sync, adopt, install };
}

export function revisionHandlers(c: Ctx, syncAuthor: () => void = () => {}): Handlers {
  const tracking = (): boolean => c.q<boolean>({ t: 'tracking' }) ?? false;
  const markupMode = (): string => c.q<string>({ t: 'markupMode' }) ?? 'all';
  const entries = (): WireEntry[] => c.q<WireEntry[]>({ t: 'revisionEntries' }) ?? [];
  const revCount = (): number => c.q<number>({ t: 'revisionCount' }) ?? 0;
  const authorColor = (a: string): string => c.q<string>({ t: 'authorColorOf', author: a }) ?? '';
  const markupOptions = (): MarkupOptions => {
    const m = c.q<WireMarkup>({ t: 'markupOptions' });
    if (!m) return { insertionsDeletions: true, formatting: true, comments: true, balloons: 'mixed', authors: null };
    return { insertionsDeletions: !!m.insDel, formatting: !!m.formatting, comments: !!m.comments, balloons: m.balloons, authors: m.authors ? [...m.authors] : null };
  };
  const styleNames = (): Map<string, string> => {
    const ps = (c.el as unknown as { paragraphStyles?: () => { id: string; name: string }[] } | null)?.paragraphStyles?.() ?? [];
    return new Map(ps.map((s) => [s.id, s.name]));
  };
  const getRevisions = (): WriteRevision[] => {
    const el = c.el;
    if (!el || !c.s) return [];
    const lang = uiLang(el);
    const names = styleNames();
    const colors = new Map<string, string>();
    return entries().map(({ span: s, story }) => {
      let color = colors.get(s.author);
      if (color === undefined) { color = authorColor(s.author); colors.set(s.author, color); }
      const fmt = s.kind === 'format' || s.kind === 'paraFormat';
      const span = { ...s, old: propsOf(s.old), now: propsOf(s.now) } as unknown as Parameters<typeof formatDescription>[0];
      return {
        kind: s.kind as WriteRevision['kind'], author: s.author,
        ...(s.date ? { date: s.date } : {}), ...(s.dateUtc ? { dateUtc: s.dateUtc } : {}),
        ...(s.move ? { move: true } : {}), ...(s.mark ? { mark: true } : {}),
        text: s.text,
        ...(fmt ? { description: formatDescription(span, lang, (id) => names.get(id) ?? id) } : {}),
        story,
        color,
      };
    });
  };
  /** a revisão do corpo cujo trecho contém o foco (a regra `revisionAt` do TS: início ≤ foco ≤ fim) */
  const atCaret = (): number => {
    const el = c.el;
    if (!el) return -1;
    const f = el.selection.focus;
    // `revisionAt` of docx-core revisions.ts over the body entries: the one containing the focus, else the one
    // ending at it, else the paragraph's format change
    const all = entries();
    const body = all.map((e, i) => ({ s: e.span, i, body: e.loc.t === 'body' })).filter((x) => x.body);
    const text = body.filter((x) => x.s.kind !== 'paraFormat');
    const end = (s: WireSpan): Pos => ({ para: s.endPara ?? s.para, offset: s.mark ? s.end + 1 : s.end });
    const hit = text.find((x) => cmpPos({ para: x.s.para, offset: x.s.start }, f) <= 0 && cmpPos(f, end(x.s)) < 0)
      ?? text.find((x) => cmpPos(f, { para: x.s.endPara ?? x.s.para, offset: x.s.end }) === 0)
      ?? body.find((x) => x.s.para === f.para && x.s.kind === 'paraFormat');
    return hit ? hit.i : -1;
  };
  const scrollSel = (): void => { const el = c.el; if (el) scrollToPos(c, el.selection.anchor); };

  return {
    toggleTrackChanges: ((on?: boolean) => {
      if (!c.s) return;
      const next = on ?? !tracking();
      if (next === tracking()) return;
      syncAuthor();
      c.run({ t: 'settings', v: { t: 'setTracking', on: next } }, 'toggleTrackChanges');
    }) as never,
    isTrackingChanges: (() => tracking()) as never,
    setMarkupMode: ((mode: string) => {
      if (!c.s || mode === markupMode()) return;
      const o = withMarkupDetail(c.s.command({ t: 'settings', v: { t: 'setMarkup', mode } }), mode);
      c.api!.apply(o, 'setMarkupMode');
      // the view changed: a new layout of the session's revision view (the session answers no story change)
      if (!(o?.t === 'done' && (o.stories.length > 0 || o.formatting))) { c.api!.relayoutNow(); c.api!.bumpGen(); }
    }) as never,
    getMarkupMode: (() => markupMode()) as never,
    setMarkupOptions: ((opts: Partial<MarkupOptions>) => {
      if (!c.s) return;
      const p: Record<string, unknown> = {};
      if (opts.insertionsDeletions !== undefined) p['insDel'] = opts.insertionsDeletions;
      if (opts.formatting !== undefined) p['formatting'] = opts.formatting;
      if (opts.comments !== undefined) p['comments'] = opts.comments;
      if (opts.balloons !== undefined) p['balloons'] = opts.balloons;
      if (opts.authors !== undefined) p['authors'] = opts.authors === null ? {} : { value: [...opts.authors] };
      const o = withMarkupDetail(c.s.command({ t: 'revision', v: { t: 'setMarkupOptions', ...p } }), markupMode());
      c.api!.apply(o, 'setMarkupOptions');
      if (!(o?.t === 'done' && (o.stories.length > 0 || o.formatting))) { c.api!.relayoutNow(); c.api!.bumpGen(); }
    }) as never,
    getMarkupOptions: (() => markupOptions()) as never,
    revisionAuthors: (() => c.q<string[]>({ t: 'revisionAuthorsNow' }) ?? []) as never,
    authorColor: ((a: string) => authorColor(a)) as never,
    getRevisions: (() => getRevisions()) as never,
    /** the TS `RevisionEntry` holds model objects (`para`, `list`, `owner`): the session's entries in the wire form (ED-312) */
    revisionSpans: (() => entries()) as never,
    acceptRevisionAt: ((i: number) => changed(c.run({ t: 'revision', v: { t: 'acceptAt', index: i } }, 'acceptRevisionAt'))) as never,
    rejectRevisionAt: ((i: number) => changed(c.run({ t: 'revision', v: { t: 'rejectAt', index: i } }, 'rejectRevisionAt'))) as never,
    acceptChange: ((opts: { moveNext?: boolean } = {}) => {
      const o = c.run({ t: 'revision', v: { t: 'accept', moveNext: opts.moveNext ?? true } }, 'acceptChange');
      if (o?.t === 'done') scrollSel();
      return changed(o);
    }) as never,
    rejectChange: ((opts: { moveNext?: boolean } = {}) => {
      const o = c.run({ t: 'revision', v: { t: 'reject', moveNext: opts.moveNext ?? true } }, 'rejectChange');
      if (o?.t === 'done') scrollSel();
      return changed(o);
    }) as never,
    acceptAllChanges: ((opts: { shown?: boolean; stopTracking?: boolean } = {}) => {
      if (!c.s || c.locked()) return 0;
      const n = revCount();
      syncAuthor();
      c.run({ t: 'revision', v: { t: 'acceptAll', shown: !!opts.shown, stopTracking: !!opts.stopTracking } }, 'acceptAllChanges');
      return n - revCount();
    }) as never,
    rejectAllChanges: ((opts: { shown?: boolean; stopTracking?: boolean } = {}) => {
      if (!c.s || c.locked()) return 0;
      const n = revCount();
      c.run({ t: 'revision', v: { t: 'rejectAll', shown: !!opts.shown, stopTracking: !!opts.stopTracking } }, 'rejectAllChanges');
      return n - revCount();
    }) as never,
    nextChange: (() => {
      const o = c.run({ t: 'revision', v: { t: 'nextChange' } }, 'nextChange');
      const ok = !!o && o.t === 'done' && entries().some((e) => e.loc.t === 'body');
      if (ok) scrollSel();
      return ok;
    }) as never,
    previousChange: (() => {
      const o = c.run({ t: 'revision', v: { t: 'previousChange' } }, 'previousChange');
      const ok = !!o && o.t === 'done' && entries().some((e) => e.loc.t === 'body');
      if (ok) scrollSel();
      return ok;
    }) as never,
    selectRevision: ((i: number) => {
      const e = entries()[i];
      if (!e || !c.s) return false;
      if (e.loc.t === 'body') {
        c.run({ t: 'revision', v: { t: 'selectRevision', index: i } }, 'selectRevision');
        scrollSel();
        return true;
      }
      return false;
    }) as never,
    revisionAtCaret: (() => {
      const i = atCaret();
      if (i < 0) return null;
      const r = getRevisions()[i];
      return r ? { ...r, index: i } : null;
    }) as never,
    /** balloons: the first line of each BODY entry's paragraph that holds the span (cells, headers/footers, notes: none, ED-304) */
    revisionAnchors: (() => {
      const el = c.el;
      const out: { index: number; topPx: number; heightPx: number; page: number }[] = [];
      if (!el || !c.dl) return out;
      entries().forEach((e, index) => {
        if (e.loc.t !== 'body') return;
        const s = e.span;
        for (const l of paraLines(c, s.para)) {
          // the ENGINE line's range (the TS `el.line.start/end`: its visible characters; hidden revision text is not in it)
          let ls = l.start, le = l.end;
          if (l.o.length) { ls = Math.min(...l.o); le = Math.max(...l.o) + 1; }
          if (!s.mark && s.kind !== 'paraFormat' && (le < s.start || ls > Math.max(s.start, s.end - 1))) continue;
          const px = linePx(el, l);
          if (px) out.push({ index, ...px });
          break;
        }
      });
      return out;
    }) as never,
  };
}

interface WireComment { id: number; author?: string; text: string }
interface WireRun { len: number; link?: string; linkTip?: string; field?: boolean; commentIds?: number[]; del?: boolean; spell?: boolean; lang: string; charStyle?: string }

/** Os runs de um parágrafo do escopo de edição (a consulta `paraRuns`), com o offset de cada um. */
const runsCache = new WeakMap<object, Map<string, (WireRun & { a: number; z: number })[]>>();
export function runsOf(c: Ctx, para: number, fallback = 'pt-BR'): (WireRun & { a: number; z: number })[] {
  // in cache per layout (every document change lays out again; the paint of each caret blink reads them)
  const dl = c.dl;
  const key = `${para}|${fallback}`;
  let m = dl ? runsCache.get(dl) : undefined;
  if (dl && !m) { m = new Map(); runsCache.set(dl, m); }
  const hit = m?.get(key);
  if (hit) return hit;
  const rs = c.q<WireRun[]>({ t: 'paraRuns', para, fallback }) ?? [];
  let acc = 0;
  const out = rs.map((r) => { const a = acc; acc += r.len; return { ...r, a, z: acc }; });
  m?.set(key, out);
  return out;
}

export function commentHandlers(c: Ctx): Handlers {
  const comments = (): WireComment[] => (c.q<WireComment[]>({ t: 'comments' }) ?? []).map((x) => ({ id: x.id, ...(x.author !== undefined ? { author: x.author } : {}), text: x.text }));
  /** the first paragraph of the body story carrying comment `id`, and its first run's range */
  const firstRun = (id: number): { para: number; a: number; z: number } | null => {
    const n = c.s?.storyInfo().paras ?? 0;
    for (let p = 0; p < n; p++) {
      const r = runsOf(c, p).find((x) => x.commentIds?.includes(id));
      if (r) return { para: p, a: r.a, z: r.z };
    }
    return null;
  };
  return {
    getComments: (() => comments()) as never,
    addComment: ((text: string, author?: string) => {
      const el = c.el;
      if (!el || !c.s || !text.trim() || c.locked()) return null;
      const { anchor, focus } = el.selection;
      if (anchor.para === focus.para && anchor.offset === focus.offset) return null;
      const before = new Set(comments().map((x) => x.id));
      c.run({ t: 'comment', v: { t: 'add', text, author: author ?? el.author } }, 'addComment');
      const added = comments().find((x) => !before.has(x.id));
      return added ? added.id : null;
    }) as never,
    removeComment: ((id: number) => { c.run({ t: 'comment', v: { t: 'remove', id } }, 'removeComment'); }) as never,
    goToComment: ((id: number) => {
      const r = firstRun(id);
      if (!r || !c.s || !c.api) return;
      c.api.apply(c.s.setSelection({ para: r.para, offset: r.a }, { para: r.para, offset: r.z }), 'goToComment');
      // navigateTo('#para:N'): the paragraph's first line 60 px under the top
      const el = c.el!;
      const l = paraLines(c, r.para)[0];
      const px = l ? linePx(el, l) : null;
      if (px) el.animateScrollTo(Math.max(0, px.topPx - 60));
    }) as never,
    commentAnchorPx: ((id: number) => {
      const el = c.el;
      if (!el || !c.dl) return null;
      const r = firstRun(id);
      if (!r) return null;
      const l = paraLines(c, r.para)[0];
      return l ? linePx(el, l) : null;
    }) as never,
  };
}
