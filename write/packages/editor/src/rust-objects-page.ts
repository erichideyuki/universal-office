// Frente O da leva 5 do engine="rust" (registro core/docs/modules/uo-bindings.md "Wave 5 — O: objects and the page"): a
// PÁGINA — Configurar Página, colunas, quebras, números de linha, hifenização, Cor da Página, marca-d'água, folha de rosto,
// bordas de página — e as EXIBIÇÕES (Modo de Leitura). Cada método é o do TS (universal-write.ts) sobre os comandos da
// sessão (SectionCmd, PageCmd, TextLayoutCmd, SettingsCmd); as consultas leem `sectionInfo` (os objetos do modelo da
// sessão no JSON do TS) com as MESMAS funções puras de docx-core que o TS usa.
import { COLUMN_SPACE_DEFAULT, columnsPresetOf, columnWidths, contentWidth, maxColumns } from '@universal-office/docx-core';
import { imagePixelInfo } from '@universal-office/docx-layout';
import { uiLang } from '@universal-office/ui';
import { ask, cstate as a2State } from './rust-completion-core.js'; // wave 7 (A2): the watermark's measured box (ED-208)
import { lineNumberingInfo, type LineNumberingInfo } from './text-layout.js';
import { cmd, locked, omit, pageLang, PX_PER_TWIP, sectionInfo, type ObjState } from './rust-objects-core.js';

type SectPr = Parameters<typeof contentWidth>[0];
type Dict = Record<string, unknown>;

const lang = (o: ObjState): 'en' | 'pt' | 'es' => (o.api ? pageLang(uiLang(o.ed.el)) : 'en');

/** `getSection()` (universal-write.ts) sobre o `sectPr` da sessão. */
export function getSection(o: ObjState): Dict | null {
  const si = sectionInfo(o);
  const sp = si?.touched[0] as unknown as SectPr | undefined;
  if (!si || !sp) return null;
  const cols = (sp as { cols?: { count?: number; spaceTwips?: number; sep?: boolean; widths?: unknown } }).cols;
  return {
    index: si.index,
    count: si.count,
    type: (sp as { type?: string }).type ?? 'nextPage',
    orientation: sp.pageWidthTwips > sp.pageHeightTwips ? 'landscape' : 'portrait',
    pageWidthTwips: sp.pageWidthTwips, pageHeightTwips: sp.pageHeightTwips,
    margins: { top: sp.marginTopTwips, right: sp.marginRightTwips, bottom: sp.marginBottomTwips, left: sp.marginLeftTwips },
    columns: {
      count: cols?.count ?? 1,
      spaceTwips: cols?.spaceTwips ?? COLUMN_SPACE_DEFAULT,
      sep: !!cols?.sep,
      equalWidth: !cols?.widths,
      widths: columnWidths(sp),
      preset: columnsPresetOf(sp),
      max: maxColumns(sp),
      contentWidthTwips: contentWidth(sp),
    },
    verticalAlignment: (sp as { vAlign?: string }).vAlign ?? 'top',
    lineNumbering: lineNumberingInfo(sp as unknown as Parameters<typeof lineNumberingInfo>[0]),
  };
}

/** A largura do conteúdo (o `contentWidthTwips()` do TS no modo de impressão: `sect()`, o sectPr final do corpo). */
export function contentWidthTwips(o: ObjState): number | undefined {
  const sp = sectionInfo(o)?.lastSect as unknown as SectPr | undefined;
  return sp ? sp.pageWidthTwips - sp.marginLeftTwips - sp.marginRightTwips : undefined;
}

/** A página do cursor e a variante do cabeçalho dela (`hfContext()` / `hfTypeOfPage` do TS). */
function hfAt(o: ObjState): { section: number; slot: 'default' | 'first' | 'even' } {
  const dl = o.api.dl;
  const page = Math.max(0, Math.min(o.api.caret()?.page ?? 0, (dl?.pages.length ?? 1) - 1));
  const si = sectionInfo(o);
  const info = dl?.inner.pageInfo(page);
  const section = Math.min(info?.section ?? 0, Math.max(0, (si?.count ?? 1) - 1));
  if (!dl || !info) return { section, slot: page === 0 && si?.titlePg[section] ? 'first' : 'default' };
  let sectPage = 0;
  for (let i = page - 1; i >= 0 && dl.inner.pageInfo(i).section === info.section; i--) sectPage++;
  const slot = sectPage === 0 && si?.titlePg[info.section] ? 'first' : si?.evenOdd && info.number % 2 === 0 ? 'even' : 'default';
  return { section, slot };
}

/** O Modo de Leitura (o `fitReadZoom` do TS: `sect()`, o sectPr final do corpo). */
function fitReadZoom(o: ObjState): void {
  const el = o.ed.el as unknown as { zoom: number; pagesViewportWidth(): number; clientHeight: number; readPagesPerView(): number };
  const sp = sectionInfo(o)?.lastSect as unknown as SectPr | undefined;
  if (!sp) return;
  const n = el.readPagesPerView();
  const availW = (el.pagesViewportWidth() || 1200) - 8;
  const availH = (el.clientHeight || 800) - 8;
  el.zoom = Math.max(0.1, Math.min(5, availW / (n * sp.pageWidthTwips * PX_PER_TWIP), availH / (sp.pageHeightTwips * PX_PER_TWIP)));
}

/** Depois de (re)criar os canvases: o bloco do Modo de Leitura, ou todos visíveis (o `refresh`/`refreshView` do TS). */
export function afterCanvases(o: ObjState): void {
  const el = o.ed.el as unknown as { view: string; canvases: HTMLCanvasElement[]; applyReadSpread?: () => void };
  if (el.view === 'read') el.applyReadSpread?.call(el);
  else for (const c of el.canvases) if (c.style.display === 'none') c.style.display = '';
}

export function pageHandlers(o: ObjState): Record<string, (...args: never[]) => unknown> {
  const sec = (v: Dict, what: string): void => { if (!locked(o)) cmd(o, { t: 'section', v }, what); };
  const page = (v: Dict, what: string): void => { if (!locked(o)) cmd(o, { t: 'page', v }, what); };
  const textLayout = (v: Dict, what: string): void => { if (!locked(o)) cmd(o, { t: 'textLayout', v }, what); };
  const h: Record<string, (...args: any[]) => unknown> = {
    setPageSetup: (opts: { orientation?: string; size?: 'A4' | 'Letter'; margins?: string; customMargins?: { top: number; right: number; bottom: number; left: number }; sectionStart?: string; verticalAlignment?: string; lineNumbering?: Dict; applyTo?: string }) => {
      const m = opts.customMargins;
      sec(omit({
        t: 'setPageSetup', orientation: opts.orientation, size: opts.size ? (opts.size === 'A4' ? 'a4' : 'letter') : undefined, margins: opts.margins,
        customMargins: m ? [m.top, m.right, m.bottom, m.left] : undefined, sectionStart: opts.sectionStart, verticalAlignment: opts.verticalAlignment,
        lineNumbering: opts.lineNumbering, applyTo: opts.applyTo,
      }), 'setPageSetup');
    },
    setColumns: (spec: string | { count: number; spaceTwips?: number; widths?: { widthTwips: number; spaceTwips: number }[]; sep?: boolean; applyTo?: string; startNewColumn?: boolean }) => {
      if (typeof spec === 'string') { sec({ t: 'setColumns', v: { t: 'preset', v: spec } }, 'setColumns'); return; }
      const { applyTo, startNewColumn, widths, ...rest } = spec;
      sec({ t: 'setColumns', v: omit({ t: 'spec', spec: omit({ ...rest, widths: widths?.map((w) => ({ f0: w.widthTwips, f1: w.spaceTwips })) }), applyTo, startNewColumn: startNewColumn || undefined }) }, 'setColumns');
    },
    insertPageBreak: () => sec({ t: 'insertPageBreak' }, 'insertPageBreak'),
    insertColumnBreak: () => sec({ t: 'insertColumnBreak' }, 'insertColumnBreak'),
    insertSectionBreak: (kind = 'nextPage') => sec({ t: 'insertSectionBreak', kind }, 'insertSectionBreak'),
    insertBlankPage: () => sec({ t: 'insertBlankPage' }, 'insertBlankPage'),
    setSectionStart: (kind: string, applyTo = 'section') => sec({ t: 'setSectionStart', kind, applyTo }, 'setSectionStart'),
    setVerticalAlignment: (v: string, applyTo = 'section') => sec({ t: 'setVerticalAlignment', v, applyTo }, 'setVerticalAlignment'),
    setLineNumbering: (mode: string) => sec({ t: 'setLineNumbering', mode }, 'setLineNumbering'),
    setLineNumberingOptions: (opts: Dict, applyTo = 'section') => sec({ t: 'setLineNumberingOptions', o: opts, applyTo }, 'setLineNumberingOptions'),
    toggleLineNumberSuppression: () => sec({ t: 'toggleLineNumberSuppression' }, 'toggleLineNumberSuppression'),
    lineNumbersSuppressed: () => !!sectionInfo(o)?.suppressed,
    getLineNumbering: (): (LineNumberingInfo & { mixed: boolean }) | null => {
      const si = sectionInfo(o);
      if (!si || !si.touched.length) return null;
      const infos = si.touched.map((sp) => lineNumberingInfo(sp as unknown as Parameters<typeof lineNumberingInfo>[0]));
      return { ...infos[0]!, mixed: infos.some((i) => i.mode !== infos[0]!.mode) };
    },
    getSection: () => getSection(o),
    setPageBorders: (pb: Dict | null, applyTo: 'section' | 'document' = 'document') => sec(omit({ t: 'setPageBorders', borders: pb ?? undefined, documentWide: applyTo === 'document' || undefined }), 'setPageBorders'),
    getPageBorders: () => {
      const si = sectionInfo(o);
      if (!si) return null;
      const s = si.pageBorderSection as { pgBorders?: unknown };
      const pb = s.pgBorders === undefined ? (si.firstSect as { pgBorders?: unknown } | null)?.pgBorders : s.pgBorders;
      return pb ? JSON.parse(JSON.stringify(pb)) : null;
    },
    setHyphenation: (mode: 'none' | 'auto') => textLayout({ t: 'setHyphenation', auto: mode === 'auto' || undefined }, 'setHyphenation'),
    setHyphenationOptions: (opts: { auto?: boolean; caps?: boolean; zoneTwips?: number; limit?: number }) => textLayout({ t: 'setHyphenationOptions', o: omit(opts) }, 'setHyphenationOptions'),
    getHyphenation: () => {
      const a = o.api?.session?.query({ t: 'hyphenation' }) as { auto?: boolean; caps?: boolean; zoneTwips?: number; limit?: number } | undefined;
      return { auto: !!a?.auto, caps: !!a?.caps, zoneTwips: a?.zoneTwips ?? 425, limit: a?.limit ?? 0 };
    },
    setPageColor: (color: string | { themeColor: string; tint?: number; shade?: number } | null) => {
      const c = color === null ? undefined : typeof color === 'string' ? { t: 'hex', v: color.replace(/^#/, '').toUpperCase() } : omit({ t: 'theme', ...color });
      page(omit({ t: 'setPageColor', color: c }), 'setPageColor');
    },
    setPageFill: (fill: Dict | null) => {
      let f: Dict | undefined;
      if (fill) {
        const { type, colors, ...r } = fill as { type: string; colors?: string } & Dict;
        if (type === 'gradient') f = { t: colors === 'one' ? 'gradientOne' : colors === 'two' ? 'gradientTwo' : 'gradientPreset', ...r };
        else if (type === 'pattern') f = { t: 'pattern', ...r };
        else f = { t: 'picture', ...(type === 'texture' ? { texture: true } : {}), data: r['data'], ext: r['ext'], name: r['name'] };
      }
      page(omit({ t: 'setPageFill', fill: f, lang: lang(o) }), 'setPageFill');
    },
    pageBackground: () => {
      const si = sectionInfo(o);
      const bg = si?.background as { color?: string; themeColor?: string; themeTint?: string; themeShade?: string; fill?: Dict & { image?: unknown } } | null | undefined;
      if (!si || !bg) return null;
      const t = bg.themeTint ? 1 - parseInt(bg.themeTint, 16) / 255 : undefined;
      const sh = bg.themeShade ? 1 - parseInt(bg.themeShade, 16) / 255 : undefined;
      const { image, ...fill } = bg.fill ?? ({} as Dict & { image?: unknown });
      return {
        ...(bg.color ? { color: bg.color.toUpperCase() } : {}),
        ...(bg.themeColor ? { themeColor: bg.themeColor } : {}),
        ...(t !== undefined ? { tint: Math.round(t * 100) / 100 } : {}),
        ...(sh !== undefined ? { shade: Math.round(sh * 100) / 100 } : {}),
        ...(bg.fill ? { fill: { ...fill, ...(image ? { hasImage: true } : {}) } } : {}),
        shown: si.bgShown,
      };
    },
    setWatermark: (spec: string | (Dict & { kind: string })) => {
      if (locked(o)) return;
      let arg: Dict;
      if (typeof spec === 'string') arg = { t: 'gallery', v: spec };
      else if (spec.kind === 'text') {
        const { kind: _k, ...s } = spec;
        void _k;
        // wave 7 (A2, ED-208 closed): an explicit size measures the text with the session's fonts (`watermarkTextBox`)
        const env = o.api?.pageEnv, ss = o.api?.session;
        let measuredBox: { widthPt: number; heightPt: number } | null | undefined;
        if (s['sizePt'] && env && ss) {
          const c = a2State(o.ed);
          measuredBox = ask(c, 'watermarkTextBox', (b, x) => b.watermarkTextBox(x, env.inner as never, String(s['text'] ?? ''), String(s['font'] ?? ''), Number(s['sizePt'])));
        }
        arg = omit({ t: 'text', spec: omit(s), measuredBox: measuredBox ?? undefined });
      } else {
        const { kind: _k, ...s } = spec;
        void _k;
        const info = imagePixelInfo(s['data'] as Uint8Array);
        arg = omit({ t: 'picture', spec: omit(s), pixelInfo: info ? [info.w, info.h, info.dpi] : undefined });
      }
      page({ t: 'setWatermark', spec: arg, at: hfAt(o), lang: lang(o) }, 'setWatermark');
    },
    removeWatermark: () => page({ t: 'removeWatermark' }, 'removeWatermark'),
    watermark: () => {
      const s = o.api?.session;
      if (!s) return null;
      const a = s.query({ t: 'watermark', at: hfAt(o) } as { t: string }) as { value?: { f0: Dict & { t: string }; f1?: Uint8Array } };
      if (!a?.value) return null;
      const { t: _t, ...info } = a.value.f0;
      void _t;
      return a.value.f1 ? { ...info, image: a.value.f1 } : info;
    },
    watermarkTexts: () => (o.api?.session?.query({ t: 'watermarkTexts', lang: lang(o) } as { t: string })?.v as string[] | undefined) ?? [],
    watermarkGallery: () => {
      const v = (o.api?.session?.query({ t: 'watermarkGalleryEntries', lang: lang(o) } as { t: string })?.v ?? []) as { item: Dict; label: string; categoryLabel: string }[];
      return v.map(({ item, label, categoryLabel }) => {
        const { textPt, textEn, textEs, boxPt, boxEn, boxEs, ...rest } = item;
        return { ...rest, text: { en: textEn, pt: textPt, es: textEs }, box: { en: boxEn, pt: boxPt, es: boxEs }, label, categoryLabel };
      });
    },
    insertCoverPage: (id: string) => page({ t: 'insertCoverPage', id, lang: lang(o) }, 'insertCoverPage'),
    removeCoverPage: () => {
      if (locked(o) || !o.api.session!.query({ t: 'hasCoverPage' }).v) return false;
      return cmd(o, { t: 'page', v: { t: 'removeCoverPage' } }, 'removeCoverPage');
    },
    hasCoverPage: () => !!o.api?.session?.query({ t: 'hasCoverPage' }).v,
    coverPages: () => ((o.api?.session?.query({ t: 'coverPages', lang: lang(o) } as { t: string })?.v ?? []) as { f0: string; f1: string }[]).map((x) => ({ id: x.f0, name: x.f1 })),
    /**
     * `setView` (print e read; web/draft/outline não roteados nesta onda: ED-209). O Modo de Leitura é casca (raias de
     * N páginas, zoom que cabe) + `settings.setReadView` (a sessão recusa edições).
     */
    setView: (v: string) => {
      const el = o.ed.el as unknown as { view: string; zoom: number; scrollTop: number; classList: DOMTokenList; refresh(): void; chromeSync: (() => void) | null; readFirstPage: number; dispatchEvent(e: Event): boolean };
      if (v !== 'print' && v !== 'read') { o.api?.warnOnce(`view:${v}`, `[universal-write] engine="rust": the ${v} view is not routed in this wave`); return; }
      if (!o.api?.session) return;
      const prev = el.view;
      const z0 = el.zoom;
      if (v === 'read' && prev !== 'read') o.zoomBeforeRead = el.zoom;
      el.view = v;
      el.classList.remove('view-print', 'view-web', 'view-draft', 'view-read', 'view-outline');
      el.classList.add(`view-${v}`);
      if (v === 'read') { el.readFirstPage = 0; fitReadZoom(o); } else if (prev === 'read' && o.zoomBeforeRead !== null) el.zoom = o.zoomBeforeRead;
      const s = o.api.session;
      const out = s.command({ t: 'settings', v: { t: 'setReadView', on: v === 'read' } });
      if (out?.t === 'done') o.api.bumpGen();
      el.refresh();
      el.scrollTop = 0;
      if (el.zoom !== z0) el.dispatchEvent(new CustomEvent('uw-zoom', { detail: { zoom: el.zoom } }));
      el.chromeSync?.();
    },
  };
  return h as Record<string, (...args: never[]) => unknown>;
}
