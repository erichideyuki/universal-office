// Frente R da leva 5 do engine="rust": localizar/substituir, o Painel de Navegação, campos e referências (sumário,
// legendas, referência cruzada, índice de ilustrações), indicadores e hiperlinks sobre a sessão (registro
// uo-bindings.md "Wave 5 — R"). As respostas na forma do TS; os números de volta (contagens, achou/não achou) vêm
// das consultas `navigationResult`/`referencesResult` da sessão depois do comando.
import { openExternalLink } from './external-link.js';
import { validBookmarkName } from './references.js';
import { Ctx, changed, linePx, paraLines, scrollToPos, storyCtx, toBody, type Pos } from './rust-review-core.js';
import { runsOf } from './rust-review-revisions.js';

type Handlers = Record<string, (...args: never[]) => unknown>;

type QueryErr = { t: 'pattern' } | { t: 'special'; v: string } | { t: 'syntax'; v: string } | { t: 'unsupported' };
interface WireFind { found?: boolean; wrapped?: boolean; error?: QueryErr; budget?: boolean }
type F9 = { t: 'bool'; v: boolean } | { t: 'count'; v: number } | ({ t: 'find' } & WireFind) | { t: 'replace'; find: WireFind; replaced?: boolean; count: number };
type F10 = { t: 'unit' } | { t: 'bool'; v: boolean } | { t: 'count'; v: number } | { t: 'hyphenation' };
interface WireHeading { index: number; level: number; text: string; para: number }
interface WireResult { index: number; start: number; end: number; snippet: { before: string; match: string; after: string }; heading: number; inTable?: boolean; loc: { t: string; para?: number } }
type FindRunOptions = { direction?: 'down' | 'up' | 'all'; within?: 'document' | 'selection'; replaceFormat?: unknown };

/** A view of the overlays the paint hook draws (rust-review-overlays.ts reads it). */
export interface FindView { show: boolean; results: { para: number; start: number; end: number }[] }

/** The TS `findMsg`: the Find What box's message (pt by the element's or the browser's language, else en). */
function errorText(el: HTMLElement | null, e: QueryErr): string {
  const pt = /^pt\b/i.test(el?.getAttribute('lang') ?? navigator.language ?? '');
  if (e.t === 'pattern') return pt ? 'O texto em "Localizar" contém uma expressão não válida para o padrão coincidente.' : 'The Find What text contains a Pattern Match expression which is not valid.';
  const v = e.t === 'special' || e.t === 'syntax' ? e.v : '';
  return pt ? `"${v}" não é um caractere especial válido para a caixa Localizar.` : `"${v}" is not a valid special character for the Find What box.`;
}

export function findHandlers(c: Ctx, view: FindView): Handlers {
  const nav = (): F9 | null => c.q<{ value?: F9 }>({ t: 'navigationResult' })?.value ?? null;
  const ref = (): F10 | null => c.q<{ value?: F10 }>({ t: 'referencesResult' })?.value ?? null;
  const query = (q: Record<string, unknown>): Record<string, unknown> => ({ ...q, text: String(q['text'] ?? '') });
  const outcome = (f: WireFind | undefined): { found: boolean; wrapped?: boolean; error?: string } => {
    if (!f) return { found: false };
    if (f.error) return { found: false, error: errorText(c.el, f.error) };
    return { found: !!f.found, ...(f.wrapped ? { wrapped: true } : {}) };
  };
  const headings = (): WireHeading[] => c.q<WireHeading[]>({ t: 'headings' }) ?? [];
  const boolOf = (r: F9 | F10 | null, o: unknown): boolean => (r?.t === 'bool' ? r.v : changed(o as never));
  const scrollSel = (): void => { const el = c.el; if (el) scrollToPos(c, el.selection.anchor); };
  const setSel = (a: Pos, f: Pos, what: string): void => { if (c.s && c.api) c.api.apply(c.s.setSelection(a, f), what); };

  const goToBookmark = (name: string): boolean => {
    const o = c.run({ t: 'find', v: { t: 'goToBookmark', name } }, 'goToBookmark');
    return boolOf(nav(), o);
  };
  const navigateTo = (link: string): boolean => {
    if (typeof link !== 'string' || !link.startsWith('#') || !c.s) return false;
    // the TS `toBody()` first (out of the note, the text box, the header/footer and the cell)
    if (/^#_top$/i.test(link)) { toBody(c, 'navigateTo'); setSel({ para: 0, offset: 0 }, { para: 0, offset: 0 }, 'navigateTo'); c.el?.animateScrollTo(0); return true; }
    if (!/^#para:\d+$/.test(link) && goToBookmark(link.slice(1))) return true;
    // a body paragraph: out of the cell being edited (the TS `exitCell()`, one level)
    if (storyCtx(c)?.kind === 'cell') c.run({ t: 'context', v: { t: 'exitCell' } }, 'navigateTo');
    const pm = /^#para:(\d+)$/.exec(link);
    if (!pm) return false;
    const para = parseInt(pm[1]!, 10);
    const l = paraLines(c, para)[0];
    const px = l && c.el ? linePx(c.el, l) : null;
    if (!px) return false;
    c.el!.animateScrollTo(Math.max(0, px.topPx - 60));
    return true;
  };
  /** the TS `linkGroupAt`: the hyperlink run around the caret (or the selection's first character), widened to its neighbours */
  const linkGroupAt = (): { para: number; from: number; to: number; link: string; tip?: string } | null => {
    const el = c.el;
    if (!el || !c.s) return null;
    const { anchor, focus } = el.selection;
    const at = anchor.para < focus.para || (anchor.para === focus.para && anchor.offset <= focus.offset) ? anchor : focus;
    const spans = runsOf(c, at.para).map((r) => ({ a: r.a, z: r.z, link: r.link, tip: r.linkTip }));
    const hit = spans.find((s) => s.link && s.a <= at.offset && at.offset <= s.z && s.z > s.a);
    if (!hit) return null;
    let from = hit.a, to = hit.z;
    for (const s of spans) if (s.link === hit.link) { if (s.z === from || (s.a < from && s.z >= from)) from = Math.min(from, s.a); }
    for (const s of spans) if (s.link === hit.link && s.a <= to && s.z > to) to = s.z;
    return { para: at.para, from, to, link: hit.link!, ...(hit.tip ? { tip: hit.tip } : {}) };
  };
  const getBookmarks = (opts: { hidden?: boolean; sort?: 'name' | 'location' } = {}): { name: string; hidden: boolean; text: string }[] => {
    const list = (c.q<{ name: string; hidden?: boolean; text: string }[]>({ t: 'bookmarks', hidden: !!opts.hidden, byLocation: opts.sort === 'location' }) ?? [])
      .map((b) => ({ name: b.name, hidden: !!b.hidden, text: b.text }));
    if (opts.sort !== 'location') list.sort((a, b) => a.name.localeCompare(b.name, 'pt-BR', { sensitivity: 'accent' }));
    return list;
  };
  const getHeadings = (): (WireHeading & { page: number })[] => headings().map((h) => ({ ...h, page: paraLines(c, h.para)[0]?.page ?? -1 }));

  return {
    // ── localizar e substituir ──
    findNextMatch: ((q: Record<string, unknown>, o: FindRunOptions = {}) => {
      if (!c.s) return { found: false };
      c.run({ t: 'find', v: { t: 'findNext', query: query(q), direction: o.direction ?? 'all' } }, 'findNextMatch');
      const r = nav();
      return outcome(r?.t === 'find' ? r : undefined);
    }) as never,
    replaceMatch: ((q: Record<string, unknown>, replacement: string, o: FindRunOptions = {}) => {
      if (!c.s) return { found: false, replaced: false };
      c.run({ t: 'find', v: { t: 'replaceMatch', query: query(q), replacement, direction: o.direction ?? 'all', ...(o.replaceFormat ? { replaceFormat: o.replaceFormat } : {}) } }, 'replaceMatch');
      const r = nav();
      if (r?.t === 'replace') return { ...outcome(r.find), replaced: !!r.replaced };
      return { ...outcome(r?.t === 'find' ? r : undefined), replaced: false };
    }) as never,
    replaceAllMatches: ((q: Record<string, unknown>, replacement: string, o: FindRunOptions = {}) => {
      if (!c.s) return { count: 0 };
      c.run({ t: 'find', v: { t: 'replaceAll', query: query(q), replacement, within: o.within ?? 'document', ...(o.replaceFormat ? { replaceFormat: o.replaceFormat } : {}) } }, 'replaceAllMatches');
      const r = nav();
      if (r?.t === 'replace') { const e = outcome(r.find).error; return e ? { count: 0, error: e } : { count: r.count }; }
      if (r?.t === 'find' && r.error) return { count: 0, error: errorText(c.el, r.error) };
      return { count: r?.t === 'count' ? r.v : 0 };
    }) as never,
    highlightMatches: ((q: Record<string, unknown>, o: FindRunOptions = {}) => {
      if (!c.s) return 0;
      c.run({ t: 'find', v: { t: 'highlightMatches', query: query(q), within: o.within ?? 'document' } }, 'highlightMatches');
      view.results = [];
      view.show = true;
      c.api?.paint();
      const r = nav();
      return r?.t === 'count' ? r.v : 0;
    }) as never,
    selectAllMatches: ((q: Record<string, unknown>, o: FindRunOptions = {}) => {
      if (!c.s) return 0;
      c.run({ t: 'find', v: { t: 'selectAllMatches', query: query(q), within: o.within ?? 'document' } }, 'selectAllMatches');
      const r = nav();
      return r?.t === 'count' ? r.v : 0;
    }) as never,
    goTo: ((what: string, target = '') => {
      if (!c.s) return false;
      // page and line need the layout (the session has none): the TS rules (universal-write.ts 18332) in the shell
      if (what === 'page' || what === 'line') {
        const el = c.el!, dl = c.dl;
        if (!dl) return false;
        const t = target.trim();
        const rel = /^[+-]\d*$/.test(t) ? (t.length > 1 ? parseInt(t, 10) : t[0] === '-' ? -1 : 1) : t === '' ? 1 : null;
        const abs = rel === null && /^\d+$/.test(t) ? parseInt(t, 10) : null;
        if (what === 'page') {
          const n = dl.pages.length;
          if (!n) return false;
          const cur = c.api?.caret()?.page ?? 0;
          const page = abs !== null ? Math.min(n, Math.max(1, abs)) - 1 : Math.min(n - 1, Math.max(0, cur + (rel ?? 1)));
          (c.el as unknown as { goToPage(p: number): void }).goToPage(page);
          return true;
        }
        const lines = Array.from({ length: dl.pages.length }, (_, p) => c.s!.pageLines(dl.inner, p)).flat();
        if (!lines.length) return false;
        const f = el.selection.focus;
        const own = lines.filter((l) => l.para === f.para);
        const ci = lines.findIndex((l) => l.para === f.para && f.offset >= l.start && (f.offset < l.end || l === own[own.length - 1]));
        const cur = ci < 0 ? -0.5 : ci;
        let l;
        if (abs !== null) l = lines[Math.min(lines.length, Math.max(1, abs)) - 1];
        else {
          const r = rel ?? 1;
          const i = r > 0 ? Math.floor(cur) + r : Math.ceil(cur) + r;
          l = lines[Math.min(lines.length - 1, Math.max(0, i))];
        }
        if (!l) return false;
        setSel({ para: l.para, offset: l.start }, { para: l.para, offset: l.start }, 'goTo');
        return true;
      }
      const o = c.run({ t: 'find', v: { t: 'goTo', what, target } }, 'goTo');
      return boolOf(nav(), o);
    }) as never,
    goToPage: ((page: number) => {
      const el = c.el;
      const cv = el?.canvases[page];
      if (!el || !cv) return;
      el.scrollTop = Math.max(0, cv.offsetTop - 8);
      const first = c.s ? c.s.pageLines(c.dl!.inner, page)[0] : undefined;
      if (first) setSel({ para: first.para, offset: first.start }, { para: first.para, offset: first.start }, 'goToPage');
    }) as never,
    getSelectionRanges: (() => c.q<{ start: Pos; end: Pos }[]>({ t: 'selectionRanges' }) ?? []) as never,
    setSelectionRanges: ((ranges: { start: Pos; end: Pos }[]) => { c.run({ t: 'navigation', v: { t: 'setSelectionRanges', ranges } }, 'setSelectionRanges'); }) as never,
    // ── painel de navegação ──
    search: ((text: string, opts: { matchCase?: boolean; wholeWord?: boolean; highlight?: boolean } = {}) => {
      if (!c.s) return [];
      if (!text) { c.run({ t: 'navigation', v: { t: 'clearSearch' } }, 'clearSearch'); view.results = []; view.show = false; c.api?.paint(); return []; }
      c.run({ t: 'navigation', v: { t: 'search', text, opts: { ...(opts.matchCase ? { matchCase: true } : {}), ...(opts.wholeWord ? { wholeWord: true } : {}) } } }, 'search');
      const rs = c.q<WireResult[]>({ t: 'searchResults' }) ?? [];
      view.results = rs.filter((r) => r.loc.t === 'body').map((r) => ({ para: r.loc.para ?? 0, start: r.start, end: r.end }));
      view.show = opts.highlight !== false;
      c.api?.paint();
      return rs.map((r) => {
        const page = r.loc.t === 'body' ? (paraLines(c, r.loc.para ?? 0).filter((l) => l.start <= r.start).pop()?.page ?? -1) : -1;
        return { index: r.index, start: r.start, end: r.end, ...r.snippet, page, heading: r.heading, inTable: !!r.inTable };
      });
    }) as never,
    clearSearch: (() => { if (!c.s) return; c.run({ t: 'navigation', v: { t: 'clearSearch' } }, 'clearSearch'); view.results = []; view.show = false; c.api?.paint(); }) as never,
    goToSearchResult: ((index: number) => {
      const o = c.run({ t: 'navigation', v: { t: 'goToSearchResult', index } }, 'goToSearchResult');
      return boolOf(nav(), o);
    }) as never,
    getHeadings: (() => getHeadings()) as never,
    currentHeading: (() => c.q<number>({ t: 'currentHeading' }) ?? -1) as never,
    goToHeading: ((index: number) => {
      const h = headings()[index];
      c.run({ t: 'navigation', v: { t: 'goToHeading', index } }, 'goToHeading');
      if (h) scrollToPos(c, { para: h.para, offset: 0 }, 0);
    }) as never,
    selectHeading: ((index: number) => { c.run({ t: 'navigation', v: { t: 'selectHeading', index } }, 'selectHeading'); }) as never,
    promoteHeading: ((index: number) => changed(c.run({ t: 'navigation', v: { t: 'promoteHeading', index } }, 'promoteHeading'))) as never,
    demoteHeading: ((index: number) => changed(c.run({ t: 'navigation', v: { t: 'demoteHeading', index } }, 'demoteHeading'))) as never,
    moveHeading: ((index: number, before: number) => changed(c.run({ t: 'navigation', v: { t: 'moveHeading', index, before } }, 'moveHeading'))) as never,
    insertHeading: ((index: number, at: string) => changed(c.run({ t: 'navigation', v: { t: 'insertHeading', index, at } }, 'insertHeading'))) as never,
    deleteHeading: ((index: number) => changed(c.run({ t: 'navigation', v: { t: 'deleteHeading', index } }, 'deleteHeading'))) as never,

    // ── campos ──
    insertField: ((kind: string) => {
      const k = ({ PAGE: 'page', NUMPAGES: 'numPages', DATE: 'date' } as Record<string, string>)[kind];
      if (k) c.run({ t: 'reference', v: { t: 'insertField', kind: k } }, 'insertField');
    }) as never,
    insertDateTime: ((pattern: string, update = false, lang?: string) => { c.run({ t: 'reference', v: { t: 'insertDateTime', pattern, update, ...(lang ? { lang } : {}) } }, 'insertDateTime'); }) as never,
    dateTimeFormats: ((lang?: string) => (c.q<{ f0: string; f1: string }[]>({ t: 'dateTimeFormats', ...(lang ? { lang } : {}) }) ?? []).map((x) => ({ pattern: x.f0, sample: x.f1 }))) as never,
    insertDocumentInfo: ((kind: string, fileName = '') => { c.run({ t: 'reference', v: { t: 'insertDocumentInfo', kind, fileName } }, 'insertDocumentInfo'); }) as never,
    insertAlignmentTab: ((alignment: string, relativeTo = 'margin', leader = 'none') => { c.run({ t: 'reference', v: { t: 'insertAlignmentTab', alignment, relativeTo, leader } }, 'insertAlignmentTab'); }) as never,
    updateFields: ((scope: 'selection' | 'all' = 'all', toc: 'pages' | 'all' = 'all') => {
      if (!c.s || c.locked()) return 0;
      c.run({ t: 'reference', v: { t: 'updateFields', scope, toc } }, 'updateFields');
      const r = ref();
      return r?.t === 'count' ? r.v : 0;
    }) as never,
    canInsertField: (() => c.q<boolean>({ t: 'canInsertField' }) ?? false) as never,
    // ── sumário ──
    insertTableOfContents: ((opts: Record<string, unknown> = {}, replace?: boolean) => boolOf(ref(), c.run({ t: 'reference', v: { t: 'insertTableOfContents', opts, ...(replace !== undefined ? { replace } : {}) } }, 'insertTableOfContents'))) as never,
    insertTocTemplate: ((id: string) => boolOf(ref(), c.run({ t: 'reference', v: { t: 'insertTocTemplate', id } }, 'insertTocTemplate'))) as never,
    updateToc: ((mode: 'pages' | 'all' = 'all') => boolOf(ref(), c.run({ t: 'reference', v: { t: 'updateToc', mode } }, 'updateToc'))) as never,
    removeTableOfContents: (() => boolOf(ref(), c.run({ t: 'reference', v: { t: 'removeTableOfContents' } }, 'removeTableOfContents'))) as never,
    setTocLevel: ((level: number) => { c.run({ t: 'reference', v: { t: 'setTocLevel', level } }, 'setTocLevel'); }) as never,
    markTocEntry: ((o: { text: string; identifier?: string; level?: number }) => boolOf(ref(), c.run({ t: 'reference', v: { t: 'markTocEntry', text: o.text, ...(o.identifier !== undefined ? { identifier: o.identifier } : {}), ...(o.level !== undefined ? { level: o.level } : {}) } }, 'markTocEntry'))) as never,
    getTocLevel: (() => c.q<number>({ t: 'tocLevel' }) ?? 0) as never,
    hasTableOfContents: (() => c.q<boolean>({ t: 'hasTableOfContents' }) ?? false) as never,
    inTableOfContents: (() => c.q<boolean>({ t: 'inTableOfContents' }) ?? false) as never,
    getTableOfContentsOptions: (() => c.q<{ value?: unknown }>({ t: 'tableOfContentsOptions' })?.value ?? null) as never,
    tocTemplates: (() => (c.q<{ f0: string; f1: string }[]>({ t: 'tocTemplates' }) ?? []).map((x) => ({ id: x.f0, name: x.f1 }))) as never,
    // ── legendas, referência cruzada, índice de ilustrações ──
    getCaptionLabels: (() => c.q<unknown[]>({ t: 'captionLabels' }) ?? []) as never,
    addCaptionLabel: ((name: string) => boolOf(ref(), c.run({ t: 'reference', v: { t: 'addCaptionLabel', name } }, 'addCaptionLabel'))) as never,
    deleteCaptionLabel: ((name: string) => boolOf(ref(), c.run({ t: 'reference', v: { t: 'deleteCaptionLabel', name } }, 'deleteCaptionLabel'))) as never,
    getCaptionNumbering: ((label: string) => c.q({ t: 'captionNumbering', label })) as never,
    setCaptionNumbering: ((label: string, numbering: unknown) => { c.run({ t: 'reference', v: { t: 'setCaptionNumbering', label, numbering } }, 'setCaptionNumbering'); }) as never,
    getAutoCaption: (() => {
      const v = c.q<{ value?: { f0: string; f1: string } }>({ t: 'autoCaption' })?.value;
      return v ? { table: { label: v.f0, position: v.f1 } } : undefined;
    }) as never,
    setAutoCaption: ((auto: { table?: { label: string; position: string } } | undefined) => {
      c.run({ t: 'reference', v: { t: 'setAutoCaption', ...(auto?.table ? { auto: { f0: auto.table.label, f1: auto.table.position } } : {}) } }, 'setAutoCaption');
    }) as never,
    captionTarget: (() => c.q<{ value?: string }>({ t: 'captionTarget' })?.value ?? null) as never,
    nextCaptionNumber: ((label: string) => c.q<string>({ t: 'nextCaptionNumber', label }) ?? '') as never,
    insertCaption: ((opts: Record<string, unknown>) => boolOf(ref(), c.run({ t: 'reference', v: { t: 'insertCaption', ...opts } }, 'insertCaption'))) as never,
    crossReferenceKinds: ((type: string) => c.q<unknown[]>({ t: 'crossReferenceKinds', ty: type }) ?? []) as never,
    getCrossReferenceItems: ((type: string, label?: string) => c.q<unknown[]>({ t: 'crossReferenceItems', ty: type, ...(label !== undefined ? { label } : {}) }) ?? []) as never,
    insertCrossReference: ((opts: Record<string, unknown> & { type?: string }) => {
      const { type, ...rest } = opts;
      return boolOf(ref(), c.run({ t: 'reference', v: { t: 'insertCrossReference', ty: type ?? rest['ty'], ...rest } }, 'insertCrossReference'));
    }) as never,
    getTableOfFiguresOptions: (() => c.q<{ value?: unknown }>({ t: 'tableOfFiguresOptions' })?.value ?? null) as never,
    insertTableOfFigures: ((opts: Record<string, unknown>) => boolOf(ref(), c.run({ t: 'reference', v: { t: 'insertTableOfFigures', ...opts } }, 'insertTableOfFigures'))) as never,
    updateTableOfFigures: (() => boolOf(ref(), c.run({ t: 'reference', v: { t: 'updateTableOfFigures' } }, 'updateTableOfFigures'))) as never,
    // ── indicadores ──
    getBookmarks: ((opts?: { hidden?: boolean; sort?: 'name' | 'location' }) => getBookmarks(opts)) as never,
    hasBookmark: ((name: string) => c.q<boolean>({ t: 'hasBookmark', name }) ?? false) as never,
    insertBookmark: ((name: string) => {
      if (!c.s || c.locked() || !validBookmarkName(name)) return false;
      return c.run({ t: 'bookmark', v: { t: 'insert', name } }, 'insertBookmark')?.t === 'done';
    }) as never,
    deleteBookmark: ((name: string) => {
      if (!c.s || c.locked() || !c.q<boolean>({ t: 'hasBookmark', name })) return false;
      return c.run({ t: 'bookmark', v: { t: 'delete', name } }, 'deleteBookmark')?.t === 'done';
    }) as never,
    goToBookmark: ((name: string) => goToBookmark(name)) as never,
    // ── hiperlinks ──
    insertHyperlink: ((opts: { address?: string; anchor?: string; tip?: string; text?: string }) => {
      if (!c.s || c.locked()) return false;
      const o: Record<string, unknown> = {};
      for (const k of ['address', 'anchor', 'tip', 'text'] as const) if (opts[k] !== undefined) o[k] = opts[k];
      return changed(c.run({ t: 'bookmark', v: { t: 'insertHyperlink', ...o } }, 'insertHyperlink'));
    }) as never,
    setLink: ((url: string | undefined) => { c.run({ t: 'bookmark', v: { t: 'setLink', ...(url ? { url } : {}) } }, 'setLink'); }) as never,
    removeLink: (() => changed(c.run({ t: 'bookmark', v: { t: 'removeLink' } }, 'removeLink'))) as never,
    getLink: (() => {
      const g = linkGroupAt();
      if (!g || !c.s) return null;
      const text = (c.s.paraText(g.para) ?? '').slice(g.from, g.to);
      return { ...(g.link.startsWith('#') ? { anchor: g.link.slice(1) } : { address: g.link }), ...(g.tip ? { tip: g.tip } : {}), text };
    }) as never,
    getLinkTargets: (() => ({
      headings: getHeadings().map((h) => ({ id: `p:${h.para}`, level: h.level, text: h.text })),
      bookmarks: getBookmarks({ sort: 'name' }).map((b) => b.name),
    })) as never,
    navigateTo: ((link: string) => navigateTo(link)) as never,
    /** Ctrl+click on a hyperlink (pointer hook, universal-write.ts 10297): the external link or the internal target */
    __ctrlClick: ((e: MouseEvent, hit: Pos) => {
      if (!e.ctrlKey || e.button !== 0 || !c.s) return false;
      for (const r of runsOf(c, hit.para)) {
        if (hit.offset >= r.a && hit.offset <= r.z && r.link) {
          if (openExternalLink(r.link)) return true;
          if (navigateTo(r.link)) return true;
        }
      }
      return false;
    }) as never,
    __scrollSel: (() => scrollSel()) as never,
  };
}
