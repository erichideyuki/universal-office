// Handlers da frente S da leva 5 do engine="rust" (tabelas, cabeçalho/rodapé, notas, cursor na caixa de texto): cada chave é um nome de
// ROUTES_STORIES.public/private (engine-routes-stories.ts). Carregado só com o motor Rust (import estático de rust-engine.ts).
// Registro: core/docs/modules/uo-bindings.md "Wave 5 — S: stories". Cada mutador vira um comando da sessão e termina no
// `apply` do RustEditor (seleção, histórico, layout, eventos, barras); sem sessão (o motor falso do teste de rotas) cada um
// responde o valor neutro do TS.
import type { StoryTextSeparator } from '@universal-office/write-session';
import { uiLang } from '@universal-office/ui';
import type { RustEditor } from './rust-engine.js';
import { galleryKind, type HfGallery } from './hf-templates.js';
import {
  apply, caretPage, changed, ctxOf, exec, hfEditOf, hfPageOf, hook, layoutIn, live, query, run, stOf, tableStateOf,
  tsTableState, type HfKindName, type StState,
} from './rust-stories.js';
import { enterHf, exitHf, exitTextbox, goToNoteRef, insertNote, mapEvents, openNote } from './rust-stories-input.js';

type NoteKind = 'footnote' | 'endnote';
type Width = { w: number; type: string } | null | undefined;

const opt = <T>(v: T | null | undefined): { value?: T } | undefined => (v === undefined ? undefined : v === null ? {} : { value: v });

/** `TextSeparator` do TS → o do fio (`{ t: 'tab' }`, `{ t: 'char', v }`). */
function sepWire(s: StoryTextSeparator | undefined): { t: string; v?: string } | undefined {
  if (s === undefined) return undefined;
  return typeof s === 'object' ? { t: 'char', v: s.char } : { t: s };
}

/** `TablePropertiesInput` do TS → o do fio (opcionais de opcionais: `{ value }`). */
function propsWire(p: {
  table?: { width?: Width; align?: string; indentTwips?: number; wrap?: string; position?: Record<string, unknown>; overlap?: boolean; title?: string; description?: string };
  row?: { height?: { twips: number; rule: string } | null; allowBreak?: boolean; header?: boolean };
  column?: { width?: Width };
  cell?: { width?: Width; vAlign?: string; margins?: Record<string, number | undefined> | null; wrap?: boolean; fitText?: boolean };
}): Record<string, unknown> {
  const clean = (o: Record<string, unknown>): Record<string, unknown> => Object.fromEntries(Object.entries(o).filter(([, v]) => v !== undefined));
  const out: Record<string, unknown> = {};
  if (p.table) out['table'] = clean({ ...p.table, width: opt(p.table.width) });
  if (p.row) out['row'] = clean({ ...p.row, height: p.row.height === undefined ? undefined : p.row.height === null ? {} : { value: { f0: p.row.height.twips, f1: p.row.height.rule } } });
  if (p.column && p.column.width !== undefined) out['columnWidth'] = opt(p.column.width);
  if (p.cell) out['cell'] = clean({ ...p.cell, width: opt(p.cell.width), margins: opt(p.cell.margins) });
  return out;
}

/** A seção e a variante do cabeçalho/rodapé em foco (`hfContext()`: a página do cursor ou a em edição). */
function hfAt(st: StState): { page: number; at: { section: number; slot: string }; kind: HfKindName | null } | null {
  const page = caretPage(st);
  const hp = hfPageOf(st, page);
  if (!hp) return null;
  const hf = hfEditOf(st);
  const side = hf ? (hf.kind === 'header' ? hp.header : hp.footer) : null;
  return { page, at: { section: Math.min(hp.section, hp.sections - 1), slot: side?.slot ?? hp.type }, kind: hf?.kind ?? null };
}

/**
 * Uma opção de seção/documento do cabeçalho/rodapé (`hfSectionOp` 11257): um passo da sessão; no modo cabeçalho a
 * sessão volta à parte editada antes, e se a página passa a mostrar outra parte a casca entra nela (ED-104, duas etapas).
 */
function hfOp(st: StState, cmd: { t: string } & Record<string, unknown>, what: string): void {
  const hf = hfEditOf(st);
  const o = run(st, { t: 'headerFooter', v: cmd }, what);
  apply(st, mapEvents(o, (ev) => (ev.t === 'uw-hfmode' ? null : ev)), what);
  if (!hf) return;
  const now = hfEditOf(st);
  const hp = hfPageOf(st, Math.min(hf.page, Math.max(0, (st.api?.dl?.pages.length ?? 1) - 1)));
  const shown = hp ? (hf.kind === 'header' ? hp.header.part : hp.footer.part) : null;
  // the page shows another part now (or none of the variant yet: enterHf creates it, as the TS enterHeaderFooter)
  if (now && shown !== now.part) {
    exitHf(st, true);
    enterHf(st, hf.kind, now.page, true);
  }
  st.ed.el.dispatchEvent(new CustomEvent('uw-hfmode', { detail: { active: !!hfEditOf(st), kind: hf.kind } }));
}

/** Uma consulta de nota/cabeçalho com valor `v`. */
const qv = <T>(st: StState, q: { t: string } & Record<string, unknown>): T | undefined => query<{ v?: T }>(st, q)?.v;

/** O método do TS no elemento (só estado da casca: a caneta das bordas, as linhas de grade, a galeria de estilos). */
function shellThrough(st: StState, name: string, args: unknown[]): unknown {
  const el = st.ed?.el;
  if (!el) return undefined;
  const proto = Object.getPrototypeOf(el) as Record<string, (...a: unknown[]) => unknown>;
  const f = proto[name];
  try {
    return typeof f === 'function' ? f.apply(el, args) : undefined;
  } catch (e) {
    // the TS body over the empty TS model failed: warned once, the neutral answer (never silent)
    st.api?.warnOnce(`shell:${name}`, `[universal-write] engine="rust": ${name} (shell state through the TS code) failed`, e);
    return undefined;
  }
}

/** The Border pen now (`borderPen`, through the TS `getBorderPen`), without the unset keys, for the wire. */
function penOf(st: StState): Record<string, unknown> | null {
  const p = shellThrough(st, 'getBorderPen', []) as Record<string, unknown> | undefined;
  return p ? Object.fromEntries(Object.entries(p).filter(([, v]) => v !== undefined)) : null;
}

export function storiesHandlers(ed: RustEditor): Record<string, (...args: never[]) => unknown> {
  const st = stOf(ed);
  hook(st);
  const t = (v: { t: string } & Record<string, unknown>, what: string): boolean => exec(st, { t: 'table', v }, what);
  const L = () => layoutIn(st);
  const h: Record<string, (...args: never[]) => unknown> = {
    // ───────────── tabelas ─────────────
    insertTable: (rows: number, cols: number, opts: { autofit?: string; colWidthTwips?: number } = {}) => {
      t({ t: 'insertTable', rows, cols, ...(opts.autofit ? { autofit: opts.autofit } : {}), ...(opts.colWidthTwips !== undefined ? { colWidthTwips: opts.colWidthTwips } : {}), layout: { rowHeights: [] } }, 'insertTable');
    },
    insertRowAbove: () => { t({ t: 'insertRowAbove' }, 'insertRowAbove'); },
    insertRowBelow: () => { t({ t: 'insertRowBelow' }, 'insertRowBelow'); },
    insertColumnLeft: () => { t({ t: 'insertColumnLeft' }, 'insertColumnLeft'); },
    insertColumnRight: () => { t({ t: 'insertColumnRight' }, 'insertColumnRight'); },
    insertCells: (shift: string = 'down') => { t({ t: 'insertCells', shift }, 'insertCells'); },
    deleteCells: (shift: string = 'left') => { t({ t: 'deleteCells', shift }, 'deleteCells'); },
    deleteRow: () => { t({ t: 'deleteRow' }, 'deleteRow'); },
    deleteColumn: () => { t({ t: 'deleteColumn' }, 'deleteColumn'); },
    deleteTable: () => { t({ t: 'deleteTable' }, 'deleteTable'); },
    mergeCells: () => { t({ t: 'mergeCells' }, 'mergeCells'); },
    splitCell: (cols = 2, rows = 1, mergeFirst = true) => { t({ t: 'splitCell', cols, rows, mergeFirst, layout: L() }, 'splitCell'); },
    splitTable: () => { t({ t: 'splitTable' }, 'splitTable'); },
    selectCell: () => { t({ t: 'selectCell' }, 'selectCell'); },
    selectRow: () => { t({ t: 'selectRow' }, 'selectRow'); },
    selectColumn: () => { t({ t: 'selectColumn' }, 'selectColumn'); },
    selectTable: () => { t({ t: 'selectTable' }, 'selectTable'); },
    distributeRows: () => { t({ t: 'distributeRows', layout: L() }, 'distributeRows'); },
    distributeColumns: () => { t({ t: 'distributeColumns' }, 'distributeColumns'); },
    setRowHeight: (twips: number, rule: string = 'atLeast') => { t({ t: 'setRowHeight', twips, exact: rule === 'exact' }, 'setRowHeight'); },
    setColumnWidth: (twips: number) => { t({ t: 'setColumnWidth', twips }, 'setColumnWidth'); },
    autoFitTable: (mode: string) => { t({ t: 'autoFitTable', mode, layout: L() }, 'autoFitTable'); },
    setCellAlignment: (horizontal: string, vertical: string) => { t({ t: 'setCellAlignment', horizontal, vertical }, 'setCellAlignment'); },
    setTextDirection: (dir: string) => { t({ t: 'setTextDirection', dir }, 'setTextDirection'); },
    cycleTextDirection: () => { t({ t: 'cycleTextDirection' }, 'cycleTextDirection'); },
    toggleRepeatHeaderRows: () => { t({ t: 'toggleRepeatHeaderRows' }, 'toggleRepeatHeaderRows'); },
    setTableCellMargins: (m: { top?: number; left?: number; bottom?: number; right?: number }, spacingTwips?: number, autoResize?: boolean) => {
      t({ t: 'setTableCellMargins', margins: { ...m }, ...(spacingTwips !== undefined ? { spacingTwips } : {}), ...(autoResize !== undefined ? { autoResize } : {}) }, 'setTableCellMargins');
    },
    sort: (req: { keys: { column: number; type: string; desc?: boolean }[]; header?: boolean; caseSensitive?: boolean; separator?: 'tab' | 'comma' | { char: string } }) => {
      if (!req?.keys?.length) return false;
      const o = run(st, {
        t: 'table',
        v: {
          t: 'sort',
          keys: req.keys.map((k) => ({ f0: k.column, f1: k.type, ...(k.desc ? { f2: true } : {}) })),
          ...(req.header !== undefined ? { header: req.header } : {}),
          ...(req.caseSensitive !== undefined ? { caseSensitive: req.caseSensitive } : {}),
          ...(req.separator !== undefined ? { separator: sepWire(req.separator) } : {}),
        },
      }, 'sort');
      apply(st, o, 'sort');
      return changed(o);
    },
    sortInfo: (separator?: 'tab' | 'comma' | { char: string }) => {
      const l = live(st);
      if (!l) return null;
      try { return l.sb.sortInfo(l.s, separator); } catch { return null; }
    },
    convertTableToText: (separator: StoryTextSeparator = 'tab') => { t({ t: 'convertTableToText', separator: sepWire(separator) }, 'convertTableToText'); },
    convertTextToTable: (o: { separator?: StoryTextSeparator; cols?: number; autofit?: string; colWidthTwips?: number } = {}) => {
      const r = run(st, {
        t: 'table',
        v: {
          t: 'convertTextToTable',
          ...(o.separator !== undefined ? { separator: sepWire(o.separator) } : {}),
          ...(o.cols !== undefined ? { cols: o.cols } : {}),
          ...(o.autofit !== undefined ? { autofit: o.autofit } : {}),
          ...(o.colWidthTwips !== undefined ? { colWidthTwips: o.colWidthTwips } : {}),
          layout: { rowHeights: [] },
        },
      }, 'convertTextToTable');
      apply(st, r, 'convertTextToTable');
      return changed(r);
    },
    textToTableGuess: (separator?: StoryTextSeparator, cols?: number) => {
      const l = live(st);
      if (!l) return null;
      try { return l.sb.textToTableGuess(l.s, separator, cols); } catch { return null; }
    },
    setTableStyle: async (name: string | null) => { t({ t: 'setTableStyle', ...(name ? { name } : {}) }, 'setTableStyle'); },
    setTableStyleOptions: (o: Record<string, boolean | undefined>) => { t({ t: 'setTableStyleOptions', ...o }, 'setTableStyleOptions'); },
    setCellShading: (color: string | { themeColor: string; tint?: string; shade?: string } | null, pattern?: { val: string; color?: string } | null, applyTo?: string) => {
      const c = color === null || color === undefined ? undefined : typeof color === 'string' ? { t: 'hex', v: color } : { t: 'theme', ...color };
      t({ t: 'setCellShading', ...(c ? { color: c } : {}), ...(pattern ? { pattern } : {}), ...(applyTo ? { applyTo } : {}) }, 'setCellShading');
    },
    setTableBorderSides: (sides: Record<string, Record<string, unknown> | null | undefined>, applyTo: string = 'cell') => {
      const list = Object.entries(sides).filter(([, v]) => v !== undefined).map(([k, v]) => (v ? { f0: k, f1: v } : { f0: k }));
      t({ t: 'setTableBorderSides', sides: list, applyTo }, 'setTableBorderSides');
    },
    setCellBorders: (kind: string, spec?: Record<string, unknown> | null) => {
      // `setCellBorders` 20164: no spec = the pen (`borderPen`, shell state through the TS code: ED-105); the session takes
      // the resolved pen (`None` = no border)
      const pen = spec === undefined ? penOf(st) : spec;
      t({ t: 'setCellBorders', kind, ...(pen ? { spec: pen } : {}) }, 'setCellBorders');
    },
    setTableProperties: (p: Parameters<typeof propsWire>[0]) => { t({ t: 'setTableProperties', input: propsWire(p ?? {}), layout: L() }, 'setTableProperties'); },
    insertFormula: (formula?: string, numberFormat?: string) => {
      const o = run(st, { t: 'table', v: { t: 'insertFormula', ...(formula !== undefined ? { formula } : {}), ...(numberFormat ? { numberFormat } : {}) } }, 'insertFormula');
      apply(st, o, 'insertFormula');
      return changed(o);
    },
    suggestFormula: () => qv<string>(st, { t: 'suggestFormula' }) ?? '=',
    previewFormula: (formula: string, numberFormat?: string) =>
      query<{ value?: string }>(st, { t: 'previewFormula', formula, ...(numberFormat ? { numberFormat } : {}) })?.value ?? '',
    resizeTableColumn: (boundary: number, deltaTwips: number) => {
      const o = run(st, { t: 'table', v: { t: 'resizeTableColumn', boundary, deltaTwips } }, 'resizeTableColumn');
      apply(st, o, 'resizeTableColumn');
      return changed(o);
    },
    resizeTableRow: (row: number, deltaTwips: number) => {
      const o = run(st, { t: 'table', v: { t: 'resizeTableRow', row, deltaTwips, layout: L() } }, 'resizeTableRow');
      apply(st, o, 'resizeTableRow');
      return changed(o);
    },
    editingTableCell: () => ctxOf(st)?.ctx.kind === 'cell',
    getTableState: () => tsTableState(tableStateOf(st)),
    // shell-through (estado da casca; ED-105): a caneta das bordas, a ferramenta, as linhas de grade, a galeria
    getBorderPen: () => shellThrough(st, 'getBorderPen', []),
    setBorderPen: (p: unknown) => shellThrough(st, 'setBorderPen', [p]),
    getTableTool: () => null,
    tableGridlinesShown: () => shellThrough(st, 'tableGridlinesShown', []),
    showTableGridlines: (on: boolean) => shellThrough(st, 'showTableGridlines', [on]),
    tableStyleListNow: () => shellThrough(st, 'tableStyleListNow', []),
    renderTableStyleThumb: (canvas: HTMLCanvasElement, name: string, look?: unknown) => shellThrough(st, 'renderTableStyleThumb', [canvas, name, look]),
    // the session's style-sheet counter (frontier R's query): a cache key of the galleries (ED-105)
    stylesVersion: () => qv<number>(st, { t: 'stylesVersion' }) ?? 0,

    // ───────────── cabeçalho e rodapé ─────────────
    enterHeaderFooter: (kind: HfKindName, activePage = 0, silent = false) => { enterHf(st, kind, activePage, silent); },
    exitHeaderFooter: (silent = false) => { exitHf(st, silent); },
    editingHeaderFooter: () => hfEditOf(st)?.kind ?? null,
    getHeaderFooterOptions: () => {
      const c = hfAt(st);
      if (!c) return null;
      const a = query<{ section: number; sectionCount: number; differentFirstPage?: boolean; differentOddEven?: boolean; headerDistance: number; footerDistance: number; linkToPrevious?: boolean }>(st, { t: 'headerFooterOptions', at: c.at });
      if (!a) return null;
      return {
        section: a.section, sectionCount: a.sectionCount, page: c.page,
        part: c.kind ? { kind: c.kind, type: c.at.slot } : null,
        differentFirstPage: !!a.differentFirstPage, differentOddEven: !!a.differentOddEven,
        headerDistance: a.headerDistance, footerDistance: a.footerDistance,
        linkToPrevious: c.kind && a.section > 0 ? !!a.linkToPrevious : false,
        documentTextShown: !st.hideDocText,
      };
    },
    getPageNumberFormat: () => {
      const c = hfAt(st);
      if (!c) return null;
      const a = query<{ format: string; chapter?: { f0: number; f1: string }; start?: number }>(st, { t: 'pageNumberFormat', at: c.at });
      if (!a) return null;
      return { format: a.format, chapter: a.chapter ? { level: a.chapter.f0, separator: a.chapter.f1 } : null, start: a.start ?? null };
    },
    setDifferentFirstPage: (on: boolean) => { const c = hfAt(st); if (c && !ctxOf(st)?.locked) hfOp(st, { t: 'setDifferentFirstPage', on, at: c.at }, 'setDifferentFirstPage'); },
    setDifferentOddEven: (on: boolean) => { if (!ctxOf(st)?.locked) hfOp(st, { t: 'setDifferentOddEven', on }, 'setDifferentOddEven'); },
    setHeaderFooterDistance: (d: { header?: number; footer?: number }) => {
      const c = hfAt(st);
      if (c && !ctxOf(st)?.locked) hfOp(st, { t: 'setHeaderFooterDistance', ...(d.header !== undefined ? { header: d.header } : {}), ...(d.footer !== undefined ? { footer: d.footer } : {}), at: c.at }, 'setHeaderFooterDistance');
    },
    setLinkToPrevious: (on: boolean, kind?: HfKindName) => {
      const c = hfAt(st);
      if (!c || c.at.section === 0 || ctxOf(st)?.locked) return;
      hfOp(st, { t: 'setLinkToPrevious', on, ...(kind ?? c.kind ? { kind: kind ?? c.kind } : {}), at: c.at }, 'setLinkToPrevious');
    },
    removeHeaderFooter: (kind: HfKindName) => {
      const c = hfAt(st);
      if (c && !ctxOf(st)?.locked) exec(st, { t: 'headerFooter', v: { t: 'removeHeaderFooter', kind, at: c.at } }, 'removeHeaderFooter');
    },
    removePageNumbers: () => {
      const c = hfAt(st);
      if (c && !ctxOf(st)?.locked) exec(st, { t: 'headerFooter', v: { t: 'removePageNumbers', at: c.at } }, 'removePageNumbers');
    },
    setPageNumberFormat: (f: { format?: string; chapter?: { level: number; separator: string } | null; start?: number | null }) => {
      const c = hfAt(st);
      if (!c || ctxOf(st)?.locked) return;
      const patch: Record<string, unknown> = {};
      if (f.format !== undefined) patch['format'] = f.format;
      if (f.chapter !== undefined) patch['chapter'] = f.chapter ? { value: { f0: f.chapter.level, f1: f.chapter.separator } } : {};
      if (f.start !== undefined) patch['start'] = f.start === null ? {} : { value: f.start };
      hfOp(st, { t: 'setPageNumberFormat', f: patch, at: c.at }, 'setPageNumberFormat');
    },
    headerFooterTemplates: (gallery: HfGallery) => shellThrough(st, 'headerFooterTemplates', [gallery]),
    insertHeaderFooterTemplate: (gallery: HfGallery, id: string) => {
      const c = ctxOf(st);
      if (!c || c.locked) return;
      const kind = galleryKind(gallery);
      const lang = uiLang(st.ed.el);
      if (kind) {
        const hf = hfEditOf(st);
        if (!hf || hf.kind !== kind) {
          const page = caretPage(st);
          if (hf) exitHf(st, true);
          enterHf(st, kind, page, true);
          st.ed.el.dispatchEvent(new CustomEvent('uw-hfmode', { detail: { active: !!hfEditOf(st), kind } }));
        }
        if (!hfEditOf(st)) return;
        exec(st, { t: 'headerFooter', v: { t: 'insertTemplate', gallery, id, lang } }, 'insertHeaderFooterTemplate');
        st.ed.focusEditor();
        return;
      }
      const pg = caretPage(st);
      const n = st.api?.dl?.pages.length ?? 1;
      const num = hfPageOf(st, pg)?.number ?? pg + 1;
      exec(st, { t: 'headerFooter', v: { t: 'insertTemplate', gallery, id, lang, pageNumbers: { f0: n, f1: num } } }, 'insertHeaderFooterTemplate');
    },
    // sem miniaturas da galeria (a sessão não diagrama uma parte de prévia): a galeria mostra os nomes (ED-106)
    paintHeaderFooterTemplate: () => false,
    goToHeaderFooter: (kind?: HfKindName) => {
      const c = ctxOf(st);
      if (!c || c.locked) return;
      const hf = hfEditOf(st);
      if (hf) {
        const k = kind ?? (hf.kind === 'header' ? 'footer' : 'header');
        if (k !== hf.kind) {
          exitHf(st, true);
          enterHf(st, k, hf.page, true);
          st.ed.el.dispatchEvent(new CustomEvent('uw-hfmode', { detail: { active: true, kind: k } }));
        }
      } else enterHf(st, kind ?? 'header', caretPage(st));
      st.ed.focusEditor();
    },
    nextHeaderFooter: () => { stepHf(st, 1); },
    previousHeaderFooter: () => { stepHf(st, -1); },
    showDocumentText: (on: boolean) => {
      if (st.hideDocText === !on) return;
      st.hideDocText = !on;
      st.api?.repaint();
    },
    documentTextShown: () => !st.hideDocText,

    // ───────────── notas ─────────────
    insertFootnote: (o: { mark?: string } = {}) => insertNote(st, true, o.mark),
    insertEndnote: (o: { mark?: string } = {}) => insertNote(st, false, o.mark),
    openNote: (kind: NoteKind, id: number) => openNote(st, kind !== 'endnote', id),
    goToNoteRef: (kind: NoteKind, id: number) => goToNoteRef(st, kind !== 'endnote', id),
    nextNote: (kind: NoteKind = 'footnote', dir: 1 | -1 = 1) => {
      const before = JSON.stringify(st.ed.el.selection);
      const o = run(st, { t: 'note', v: { t: 'next', footnote: kind !== 'endnote', forward: dir > 0 } }, 'nextNote');
      apply(st, o, 'nextNote');
      return !!o && o.t === 'done' && JSON.stringify(st.ed.el.selection) !== before;
    },
    showNotes: (kind?: NoteKind) => {
      const before = JSON.stringify(ctxOf(st)?.ctx ?? null) + JSON.stringify(st.ed.el.selection);
      const o = run(st, { t: 'note', v: { t: 'show', ...(kind ? { footnote: kind !== 'endnote' } : {}) } }, 'showNotes');
      apply(st, o, 'showNotes');
      return !!o && o.t === 'done' && JSON.stringify(ctxOf(st)?.ctx ?? null) + JSON.stringify(st.ed.el.selection) !== before;
    },
    setNoteOptions: (kind: NoteKind, o: Record<string, unknown>, scope: 'section' | 'document' = 'document') => {
      exec(st, { t: 'note', v: { t: 'setOptions', footnote: kind !== 'endnote', patch: { ...o }, scope } }, 'setNoteOptions');
    },
    getNoteOptions: (kind: NoteKind = 'footnote') => {
      const a = query<{ position: string; numberFormat: string; startAt: number; restart: string; columns?: number }>(st, { t: 'noteOptions', footnote: kind !== 'endnote' });
      if (!a) return null;
      return { position: a.position, numberFormat: a.numberFormat, startAt: a.startAt, restart: a.restart, ...(a.columns !== undefined ? { columns: a.columns } : {}) };
    },
    convertNotes: (mode: 'toEndnotes' | 'toFootnotes' | 'swap', only?: { kind: NoteKind; id: number }) => {
      const o = run(st, { t: 'note', v: { t: 'convert', mode, ...(only ? { only: { f0: only.kind !== 'endnote', f1: only.id } } : {}) } }, 'convertNotes');
      apply(st, o, 'convertNotes');
      return qv<number>(st, { t: 'lastConvertedNotes' }) ?? 0;
    },
    noteCount: (kind: NoteKind) => qv<number>(st, { t: 'noteCount', footnote: kind !== 'endnote' }) ?? 0,
    canInsertNote: () => !!qv<boolean>(st, { t: 'canInsertNote' }),
    editingNote: () => {
      const c = ctxOf(st)?.ctx;
      return c && c.kind === 'note' ? { kind: c.footnote ? 'footnote' : 'endnote', id: c.id } : null;
    },

    // ───────────── a caixa de texto (o texto; o objeto é da frente O) ─────────────
    exitTextbox: () => { exitTextbox(st); },
    editingTextbox: () => ctxOf(st)?.ctx.kind === 'textbox',
  };
  return h;
}

/** `stepHeaderFooter(dir)` (11415): o cabeçalho (ou rodapé) seguinte na ordem do Word que alguma página mostra. */
function stepHf(st: StState, dir: 1 | -1): void {
  const c = ctxOf(st);
  if (!c || c.locked) return;
  const n = st.api?.dl?.pages.length ?? 0;
  const kind = hfEditOf(st)?.kind ?? 'header';
  const order = (t: string): number => (t === 'first' ? 0 : t === 'even' ? 1 : 2);
  const stops = new Map<string, { sec: number; type: string; page: number }>();
  for (let pg = 0; pg < n; pg++) {
    const hp = hfPageOf(st, pg);
    if (!hp) continue;
    const sec = hp.section, type = hp.type;
    if (!stops.has(`${sec}:${type}`)) stops.set(`${sec}:${type}`, { sec, type, page: pg });
  }
  const list = [...stops.values()].sort((a, b) => a.sec - b.sec || order(a.type) - order(b.type));
  const page = caretPage(st);
  const hp = hfPageOf(st, page);
  if (!hp) return;
  const hf = hfEditOf(st);
  const curType = hf ? (hf.kind === 'header' ? hp.header.slot : hp.footer.slot) : hp.type;
  const curSec = Math.min(hp.section, hp.sections - 1);
  const at = list.findIndex((s) => s.sec === curSec && s.type === curType);
  const next = list[at < 0 ? (dir > 0 ? 0 : list.length - 1) : at + dir];
  if (!next) return;
  if (hf) exitHf(st, true);
  enterHf(st, kind, next.page, true);
  st.ed.el.dispatchEvent(new CustomEvent('uw-hfmode', { detail: { active: !!hfEditOf(st), kind } }));
  const cv = st.ed.el.canvases[next.page];
  const el = st.ed.el;
  if (cv) {
    const top = cv.offsetTop, bottom = top + cv.offsetHeight;
    if (top < el.scrollTop || bottom > el.scrollTop + el.clientHeight) el.scrollTop = Math.max(0, top - 24);
  }
  st.ed.focusEditor();
}

