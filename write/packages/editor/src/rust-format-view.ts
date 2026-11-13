// Frente A1 da leva 7 do engine="rust": Exibir e os adornos de tela (registro uo-bindings.md "Wave 7 — A1"). A régua
// (`getRulerInfo` sobre a linha do motor do foco: quadro e formato do parágrafo, a seção da página, as colunas e a tabela),
// as Linhas de Grade do documento, as marcas dos indicadores, as marcas de formatação (a 1ª metade do `paintEngineMarks`
// do TS, ED-309), os modos Web/Rascunho/Estrutura de Tópicos (a vista do motor do shell, D-A1-1), o controle do sumário
// (moldura e aba) e os inlines de plugin desenhados pelo plugin. Cada um é o método do TS
// com o MESMO nome; a pintura usa os ganchos do RustEditor (twips da página).
import { bindStories, type ExecOutcome, type ShellLine, type StoriesBinding } from '@universal-office/write-session';
import { pickLabels } from '@universal-office/ui';
import { FormatCtx, sectionColumns, sync, type Handlers, type SectPrJson } from './rust-format-core.js';
import { pfmtTab } from './rust-format-text.js';
import { PX_PER_TWIP, onLine, outerCtx, pageLines, paraLines, storyCtx, storyLines, type Pos } from './rust-review-core.js';
import { webPageHeight, type WebPage } from '../../viewer/src/views.js';

/** universal-write.ts DEFAULT_FONT_CSS (321) */
const DEFAULT_FONT_CSS = '"Liberation Sans"';

/** universal-write.ts BREAK_MARK_LABELS (324), copied (a module constant there) */
const BREAK_MARK_LABELS = {
  en: { page: 'Page Break', column: 'Column Break', section: 'Section Break', nextPage: 'Next Page', continuous: 'Continuous', evenPage: 'Even Page', oddPage: 'Odd Page', nextColumn: 'Next Column' },
  pt: { page: 'Quebra de página', column: 'Quebra de coluna', section: 'Quebra de seção', nextPage: 'Próxima página', continuous: 'Contínua', evenPage: 'Página par', oddPage: 'Página ímpar', nextColumn: 'Próxima coluna' },
  es: { page: 'Salto de página', column: 'Salto de columna', section: 'Salto de sección', nextPage: 'Página siguiente', continuous: 'Continua', evenPage: 'Página par', oddPage: 'Página impar', nextColumn: 'Columna siguiente' },
};

interface SectionJson { sectPr: SectPrJson; sect: { colDefs?: { w: number; space: number }[]; equalWidth?: boolean; type?: string } | null }

export interface ViewState {
  sb: StoriesBinding | null;
  sections: Map<number, SectionJson | null>;
  sectionsOf: unknown;
}

export function viewHandlers(c: FormatCtx, prev: Record<string, unknown>): { handlers: Handlers; pageHeight(i: number): number | null; paintBase(ctx: CanvasRenderingContext2D, page: number, scale: number): void; paintOver(ctx: CanvasRenderingContext2D, page: number, scale: number): void; paintContent(ctx: CanvasRenderingContext2D, page: number, scale: number): void; pointer(e: MouseEvent): boolean; painterUp(): void; key(e: KeyboardEvent): boolean; adopt(): void; paintMarks(ctx: CanvasRenderingContext2D, page: number): void; selectionRelayout(): boolean } {
  let zoomHooked = false;
  const vs: ViewState = { sb: null, sections: new Map(), sectionsOf: null };
  const mod = (): { exports: WebAssembly.Exports; binding: unknown } | null => (c.api?.module as unknown as { exports: WebAssembly.Exports; binding: unknown } | undefined) ?? null;
  const sb = (): StoriesBinding | null => {
    const m = mod();
    if (!m) return null;
    if (!vs.sb) { try { vs.sb = bindStories(m.exports, m.binding); } catch { vs.sb = null; } }
    return vs.sb;
  };
  /** section `i` of the document (cached per layout: a section edit lays out again) */
  const section = (i: number): SectionJson | null => {
    if (vs.sectionsOf !== c.dl) { vs.sections.clear(); vs.sectionsOf = c.dl; }
    if (!vs.sections.has(i)) vs.sections.set(i, c.json<SectionJson>(12, [i]));
    return vs.sections.get(i) ?? null;
  };
  const sectionOfPage = (pg: number): number => {
    const dl = c.dl;
    try { return dl && pg < dl.pages.length ? dl.inner.pageInfo(pg).section : 0; } catch { return 0; }
  };
  const el = (): (HTMLElement & { view: string; zoom: number; pageDark: boolean; showMarks: boolean; selection: { anchor: Pos; focus: Pos }; printRender: boolean; refresh(): void; chromeSync: (() => void) | null; scrollTop: number; pagesViewportWidth(): number; canvases: HTMLCanvasElement[] }) | null =>
    c.el as never;

  // ── a régua ──
  const getRulerInfo = (): unknown => {
    const e = el(), s = c.s, dl = c.dl, fb = c.fb;
    if (!e || !s || !dl || !fb || e.view === 'read' || e.view === 'outline') return null;
    let rl: ReturnType<typeof fb.ruler> = null;
    try { rl = fb.ruler(s, dl.inner); } catch { rl = null; }
    const pg = rl?.page ?? 0;
    // the continuous views (Web/Draft) answer as the TS there: the document's (last) section, one column = the text frame
    const continuous = !!viewKind(e.view);
    const sec = section(continuous ? (c.json<number[]>(17) ?? []).length : sectionOfPage(pg));
    const sp = sec?.sectPr;
    if (!sp) return null;
    const margins = { top: sp.marginTopTwips, right: sp.marginRightTwips, bottom: sp.marginBottomTwips, left: sp.marginLeftTwips };
    let columns = sectionColumns(sp, sec?.sect);
    let frame: { x0: number; x1: number };
    let indents: { left: number; right: number; firstLine: number };
    let tabs: ReturnType<typeof pfmtTab>[];
    if (rl?.frame && rl.pfmt) {
      frame = { x0: rl.x + rl.frame[0], x1: rl.x + rl.frame[1] };
      indents = { left: rl.pfmt.left, right: rl.pfmt.right, firstLine: rl.pfmt.firstLine };
      tabs = rl.pfmt.tabs.map(pfmtTab);
    } else {
      // no engine line (an empty story, a frame-less line): the margins and the effective indents (the TS legacy branch)
      const x0 = sp.marginLeftTwips;
      frame = { x0, x1: x0 + Math.max(720, sp.pageWidthTwips - sp.marginLeftTwips - sp.marginRightTwips) };
      const h = e as unknown as { getParagraphIndents(): { left: number; right: number; firstLine: number }; getTabStops(): { tabs: ReturnType<typeof pfmtTab>[] } };
      indents = h.getParagraphIndents();
      tabs = h.getTabStops().tabs;
    }
    // wave 7c (V, fix round 2): Web's frame is the TS's own (17141: `contMarginTwips()` + `contentWidthTwips()`, read from the
    // element's width at the call — the TS legacy layout has no engine line), equal to the engine line's once the sheet
    // follows the width (the Word default lays it out again on a resize; `tsStrict` at the next selection change, as the TS)
    if (e.view === 'web') { const m = contMargin(); frame = { x0: m, x1: m + webWidth() - 2 * m }; }
    if (continuous) columns = [{ x: frame.x0, w: frame.x1 - frame.x0 }];
    const k = storyCtx(c);
    const o = outerCtx(k);
    const where = o?.kind === 'hf' ? o.hfKind : k?.kind === 'textbox' ? 'textbox' : k?.kind === 'cell' ? 'cell' : 'body';
    let table: unknown = null;
    if (k?.kind === 'cell') {
      const b = sb();
      const last = k.hops[k.hops.length - 1];
      const outerPath = JSON.stringify(k.hops.slice(0, -1).map((h) => [h.r, h.c]));
      if (b && last) {
        const boxes: { r: number; c: number; span: number; x: number; y: number; w: number; h: number }[] = [];
        for (const bx of b.cellBoxes(dl.inner, pg)) {
          const cell = b.cellAt(s, dl.inner, pg, bx.x + bx.w / 2, bx.y + bx.h / 2);
          if (!cell || cell.anchor !== k.anchor || JSON.stringify(cell.path.slice(0, -1).map((h) => [h.r, h.c])) !== outerPath) continue;
          if (boxes.some((x) => x.r === cell.r && x.c === cell.c)) continue;
          boxes.push({ r: cell.r, c: cell.c, span: cell.span, x: cell.x, y: cell.y, w: cell.w, h: cell.h });
        }
        const inRow = boxes.filter((x) => x.r === last.r).sort((a, b2) => a.x - b2.x);
        const cols: { x: number; boundary: number }[] = inRow.map((x) => ({ x: x.x, boundary: x.c }));
        const end = inRow[inRow.length - 1];
        if (end) cols.push({ x: end.x + end.w, boundary: end.c + end.span });
        const rows = new Map<number, { top: number; bottom: number; row: number }>();
        for (const x of boxes) {
          const r = rows.get(x.r);
          if (r) { r.top = Math.min(r.top, x.y); r.bottom = Math.max(r.bottom, x.y + x.h); } else rows.set(x.r, { top: x.y, bottom: x.y + x.h, row: x.r });
        }
        table = { cols, rows: [...rows.values()].sort((a, b2) => a.top - b2.top), current: inRow.findIndex((x) => x.c === last.c) };
      }
    }
    return {
      page: pg, pageWidthTwips: sp.pageWidthTwips, pageHeightTwips: sp.pageHeightTwips, margins, columns, frame,
      indents, tabs, defaultTabTwips: c.json<number>(4) ?? 708, where, table,
      lineTop: rl?.y ?? margins.top, readOnly: c.locked() || e.classList.contains('uw-comparing'),
    };
  };

  // ── Linhas de Grade do documento (na base da página, nunca na impressão) ──
  const paintDocGrid = (ctx: CanvasRenderingContext2D, pg: number, scale: number): void => {
    const dl = c.dl, e = el();
    if (!c.st.grid || !dl || !e || e.printRender) return;
    const p = dl.pages[pg];
    if (!p) return;
    const g = c.json<{ box: [number, number, number, number]; xs: number[]; ys: number[] }>(11, [sectionOfPage(pg), p.widthTw, p.heightTw]);
    if (!g) return;
    const [x0, y0, x1, y1] = g.box;
    ctx.save();
    ctx.strokeStyle = e.pageDark ? 'rgba(255,255,255,0.16)' : 'rgba(0,0,0,0.14)';
    ctx.lineWidth = 1 / scale;
    ctx.beginPath();
    for (const x of g.xs) { ctx.moveTo(x, y0); ctx.lineTo(x, y1); }
    for (const y of g.ys) { ctx.moveTo(x0, y); ctx.lineTo(x1, y); }
    ctx.stroke();
    ctx.restore();
  };

  // ── marcas dos indicadores ──
  const caretXIn = (l: ShellLine, at: number): number => {
    // the x of offset `at` on the line (the TS `caretXIn`): the char starting at `at`, else the end of the last one before
    let x: number | null = null, end = l.x;
    for (let i = 0; i < l.o.length; i++) {
      if (l.o[i] === at) { x = l.cx[i]!; break; }
      if (l.o[i]! < at) end = Math.max(end, l.cx[i]! + l.cw[i]!);
    }
    return x ?? end;
  };
  const paintBookmarkMarks = (ctx: CanvasRenderingContext2D, pg: number): void => {
    const e = el(), s = c.s;
    if (!c.st.bookmarks || !e || !s || !c.dl) return;
    ctx.save();
    ctx.strokeStyle = e.pageDark ? '#9aa0a6' : '#808080';
    ctx.lineWidth = 14;
    // wave 7c (V, ED-304): every story's lines (the TS `eng.lines`, 6268: body, cells, headers/footers, notes; the kind
    // 'other' and the text boxes' content skipped), whatever the edit scope
    for (const l of storyLines(c, pg)) {
      if (l.kind === 'tb') continue;
      const ms = l.bm;
      if (!ms.length) continue;
      const plen = l.text.length;
      onLine(ctx, l, () => {
        for (let i = 0; i < ms!.length; i++) {
          const b = ms![i]!;
          if (b.hidden) continue;
          if (b.at < l.start || b.at > l.end || (b.at === l.end && b.at < plen && !b.end)) continue;
          const x = caretXIn(l, b.at);
          const y0 = l.y + 10, y1 = l.y + l.height - 10, w = 50;
          const nx = ms![i + 1];
          const point = !b.end && nx?.end && nx.id === b.id && nx.at === b.at;
          ctx.beginPath();
          ctx.moveTo(x, y0); ctx.lineTo(x, y1);
          if (point) {
            ctx.moveTo(x - w, y0); ctx.lineTo(x + w, y0); ctx.moveTo(x - w, y1); ctx.lineTo(x + w, y1);
            i++;
          } else {
            const d = b.end ? -w : w;
            ctx.moveTo(x, y0); ctx.lineTo(x + d, y0); ctx.moveTo(x, y1); ctx.lineTo(x + d, y1);
          }
          ctx.stroke();
        }
      });
    }
    ctx.restore();
  };

  // ── marcas de formatação (· ° → ¶ e as quebras; a 1ª metade do paintEngineMarks do TS) ──
  const paintBreakMark = (ctx: CanvasRenderingContext2D, x0: number, x1: number, baseline: number, fs: number, kind: string, double: boolean): void => {
    const L = pickLabels(c.el as HTMLElement, BREAK_MARK_LABELS) as Record<string, string>;
    const label = kind === 'page' ? L['page']! : kind === 'column' ? L['column']! : `${L['section']} (${L[kind] ?? kind})`;
    const right = Math.max(x1, x0 + 600);
    ctx.save();
    ctx.font = `400 ${fs * 0.8}px ${DEFAULT_FONT_CSS}`;
    ctx.textAlign = 'center';
    const tw = ctx.measureText(label).width + 120;
    const mid = (x0 + right) / 2;
    const y = baseline - fs * 0.3;
    ctx.strokeStyle = ctx.fillStyle as string;
    ctx.lineWidth = 8;
    ctx.setLineDash([16, 24]);
    for (const dy of double ? [-14, 14] : [0]) {
      ctx.beginPath();
      if (tw < right - x0) {
        ctx.moveTo(x0, y + dy); ctx.lineTo(mid - tw / 2, y + dy);
        ctx.moveTo(mid + tw / 2, y + dy); ctx.lineTo(right, y + dy);
      } else { ctx.moveTo(x0, y + dy); ctx.lineTo(right, y + dy); }
      ctx.stroke();
    }
    if (tw < right - x0) ctx.fillText(label, mid, baseline);
    ctx.restore();
  };
  const columnRightAt = (secIdx: number, x: number): number => {
    const sec = section(secIdx);
    if (!sec) return x;
    const cols = sectionColumns(sec.sectPr, sec.sect);
    const f = cols.find((r) => x <= r.x + r.w + 30) ?? cols[cols.length - 1]!;
    return f.x + f.w;
  };
  /**
   * Wave 7c (V): Word's Outline symbols (measured, record "Wave 7c — V": a heading's ⊕ — ⊖ without text under it — and the
   * body text's ○, 18 px left of the paragraph's first line). The frozen TS has none (`tsStrict`).
   */
  const paintOutlineSymbols = (ctx: CanvasRenderingContext2D, pg: number): void => {
    const e = el(), s = c.s;
    if (!e || e.view !== 'outline' || tsStrict() || !s || !c.dl || storyCtx(c)?.kind !== 'body') return;
    const heads = new Map((c.q<{ level: number; para: number }[]>({ t: 'headings' }) ?? []).map((h) => [h.para, h.level]));
    const n = s.storyInfo().paras;
    const seen = new Set<number>();
    // fix round 2 — measured (word-v3.ps1, Outline at zoom 100 and 200, PrintWindow; they scale with the zoom): a
    // heading's symbol a circle 14 px across (210 twips), a body paragraph's 8 px (120), both filled E1E1E1 with a 1-px
    // 777777 ring, centred 17.5 px (262.5 twips) left of the first line's text and 1 px above the line's middle; the
    // heading's plus (minus without text under it) 444444, 2 px thick, 8 px long
    ctx.save();
    for (const l of pageLines(c, pg)) {
      if (seen.has(l.para) || l.start !== 0) continue;
      seen.add(l.para);
      const lvl = heads.get(l.para);
      const r = lvl !== undefined ? 105 : 60;
      const left = l.o.length ? l.cx[0]! : l.x;
      const cx = left - 262.5, cy = l.y + l.height / 2 - 15;
      ctx.beginPath();
      ctx.arc(cx, cy, r - 7.5, 0, Math.PI * 2);
      ctx.fillStyle = '#e1e1e1';
      ctx.fill();
      ctx.strokeStyle = '#777777';
      ctx.lineWidth = 15;
      ctx.stroke();
      if (lvl !== undefined) {
        // ⊕ when text follows it before the next heading of its level or higher, else ⊖
        const next = l.para + 1 < n ? heads.get(l.para + 1) : 0;
        const sub = l.para + 1 < n && (next === undefined || next > lvl);
        ctx.strokeStyle = '#444444';
        ctx.lineWidth = 30;
        ctx.beginPath();
        ctx.moveTo(cx - 60, cy); ctx.lineTo(cx + 60, cy);
        if (sub) { ctx.moveTo(cx, cy - 60); ctx.lineTo(cx, cy + 60); }
        ctx.stroke();
      }
    }
    ctx.restore();
  };
  /**
   * Wave 7c (V, fix round 2) — Word manda sobre o TS: Word's Draft and Outline draw a page, column or section break as a
   * dotted rule across the whole pane with its label centred on it, with or without the formatting marks (measured,
   * word-v3.ps1, PrintWindow at zoom 100 and 200: the rule 1-px dots, every 2 px for a page break, every 4 px for a column
   * break, two rows 2 px apart for a section break, black; its row 2.3 px (34 twips, scaled) above the break line's
   * baseline; the label "Quebra de página" / "Quebra de coluna" / "Quebra de seção (Próxima página)" in 9-pt Segoe UI,
   * NOT scaled by the zoom, its baseline 2 px under the rule, the dots stopping 3 px either side of it). The engine gives
   * each break a line of its own there (`BreakSettings::break_own_line`; the section break the line after its
   * paragraph). The frozen TS draws `paintBreakMark` with the formatting marks only (`tsStrict`).
   */
  const paintWordBreak = (ctx: CanvasRenderingContext2D, pg: number, yTw: number, kind: string): void => {
    const e = el(), p = c.dl?.pages[pg];
    if (!e || !p) return;
    const L = pickLabels(c.el as HTMLElement, BREAK_MARK_LABELS) as Record<string, string>;
    const label = kind === 'page' ? L['page']! : kind === 'column' ? L['column']! : `${L['section']} (${L[kind] ?? kind})`;
    const m = ctx.getTransform();
    const k = m.a / (PX_PER_TWIP * e.zoom); // device pixels per CSS pixel
    const d = Math.max(1, Math.round(k));
    const x0 = m.e, x1 = m.a * p.widthTw + m.e;
    const y = Math.round(m.d * yTw + m.f);
    ctx.save();
    ctx.setTransform(1, 0, 0, 1, 0, 0);
    ctx.font = `400 ${12 * k}px "Segoe UI", system-ui, sans-serif`;
    const mid = (x0 + x1) / 2, half = ctx.measureText(label).width / 2 + 3 * k;
    ctx.fillStyle = e.pageDark ? '#e8eaed' : '#000000';
    const step = (kind === 'column' ? 4 : 2) * d;
    for (const yy of kind === 'page' || kind === 'column' ? [y] : [y, y + 2 * d]) {
      for (let x = Math.ceil(x0); x + d <= x1; x += step) if (x + d <= mid - half || x >= mid + half) ctx.fillRect(x, yy, d, d);
    }
    ctx.textAlign = 'center';
    ctx.textBaseline = 'alphabetic';
    ctx.fillText(label, mid, y + 2 * k);
    ctx.restore();
  };
  const paintWordBreaks = (ctx: CanvasRenderingContext2D, pg: number): void => {
    const s = c.s;
    if (!s || !c.dl || storyCtx(c)?.kind !== 'body') return;
    const texts = new Map<number, string>();
    const textOf = (p: number): string => { let t = texts.get(p); if (t === undefined) { t = s.paraText(p) ?? ''; texts.set(p, t); } return t; };
    const ends = c.json<number[]>(17) ?? [];
    const secOfPara = (p: number): number => ends.filter((x) => x < p).length;
    for (const l of pageLines(c, pg)) {
      const text = textOf(l.para);
      const ch = l.end - l.start === 1 ? text[l.start] : undefined;
      // a page or column break: its own line
      if (ch === '\f' || ch === '\u000B') { paintWordBreak(ctx, pg, l.baseline - 34, ch === '\f' ? 'page' : 'column'); continue; }
      if (!ends.includes(l.para)) continue;
      const all = paraLines(c, l.para);
      const last = all[all.length - 1];
      if (last !== l && !(last && last.y === l.y && last.start === l.start)) continue;
      const type = section(secOfPara(l.para) + 1)?.sectPr.type ?? 'nextPage';
      // the section break: the paragraph's own line when it has no text, else the line after its last
      paintWordBreak(ctx, pg, (text.length ? l.y + l.height : l.y) + (l.baseline - l.y) - 34, type);
    }
  };
  const paintMarks = (ctx: CanvasRenderingContext2D, pg: number): void => {
    const e = el(), s = c.s;
    paintOutlineSymbols(ctx, pg);
    const wordBreaks = !!e && (e.view === 'draft' || e.view === 'outline') && !tsStrict();
    if (wordBreaks) paintWordBreaks(ctx, pg);
    if (!e?.showMarks || !s || !c.dl || !e) return;
    ctx.save();
    ctx.fillStyle = e.pageDark ? '#9aa0a6' : '#888';
    ctx.textAlign = 'center';
    const secIdx = sectionOfPage(pg);
    const lines = pageLines(c, pg);
    const texts = new Map<number, string>();
    const textOf = (p: number): string => { let t = texts.get(p); if (t === undefined) { t = s.paraText(p) ?? ''; texts.set(p, t); } return t; };
    const body = storyCtx(c)?.kind === 'body';
    // `sectionIndexForPara(pi)`: the sections start after the paragraphs that end one (the body's; else the page's section)
    const ends = body ? c.json<number[]>(17) ?? [] : [];
    const secOfPara = (p: number): number => (body ? ends.filter((e) => e < p).length : secIdx);
    const breaks = new Map<number, boolean>();
    const sectionBreakOf = (p: number): boolean => {
      let b = breaks.get(p);
      if (b === undefined) { b = !!c.q<{ sectionBreak?: unknown }>({ t: 'paraPropsAt', para: p })?.sectionBreak; breaks.set(p, b); }
      return b;
    };
    for (const l of lines) {
      const text = textOf(l.para);
      const fs = Math.max(160, l.height * 0.6);
      onLine(ctx, l, () => {
        ctx.font = `400 ${fs}px ${DEFAULT_FONT_CSS}`;
        for (let i = 0; i < l.o.length; i++) {
          const ch = text[l.o[i]!];
          const cx = l.cx[i]! + l.cw[i]! / 2;
          if (ch === ' ' || ch === ' ') ctx.fillText(ch === ' ' ? '·' : '°', cx, l.baseline);
          else if (ch === '\t') ctx.fillText('→', cx, l.baseline);
        }
        const breakOnly = text === '\f' || text === '\u000B';
        for (let o = breakOnly ? 0 : Math.max(1, l.start); o < Math.min(l.end, text.length); o++) {
          const ch = text[o];
          if (ch !== '\f' && ch !== '\u000B') continue;
          const bx = caretXIn(l, o);
          if (!wordBreaks) paintBreakMark(ctx, bx, columnRightAt(secOfPara(l.para), bx), l.baseline, fs, ch === '\f' ? 'page' : 'column', false);
        }
        const all = paraLines(c, l.para);
        if (all[all.length - 1] === l || (all.length && all[all.length - 1]!.page === l.page && all[all.length - 1]!.y === l.y && all[all.length - 1]!.start === l.start)) {
          ctx.textAlign = 'left';
          const x = caretXIn(l, text.length);
          // the paragraph that ends a section: the break, with the next section's type (the TS `nextType`)
          const nextType = (k: number): string => section(secOfPara(k) + 1)?.sectPr.type ?? 'nextPage';
          const sx = secOfPara(l.para);
          if (body && sectionBreakOf(l.para)) {
            if (!wordBreaks) paintBreakMark(ctx, x + 20, columnRightAt(sx, x), l.baseline, fs, nextType(l.para), true);
          } else if (!breakOnly) {
            ctx.fillText('¶', x + 20, l.baseline);
            // the EMPTY next paragraph carries only the break (a continuous section: no line): its mark after this ¶
            const x0 = x + 20 + ctx.measureText('¶').width + 60;
            const nt = textOf(l.para + 1);
            if (body && sectionBreakOf(l.para + 1) && !paraLines(c, l.para + 1).length) {
              if (!wordBreaks) paintBreakMark(ctx, x0, columnRightAt(sx, x), l.baseline, fs, nextType(l.para + 1), true);
            } else {
              // the next one opens with a page/column break (as Word inserts it): the mark where this text ended
              const c0 = nt.length > 1 ? nt[0] : undefined;
              if (!wordBreaks && (c0 === '\f' || c0 === '\u000B')) paintBreakMark(ctx, x0, columnRightAt(sx, x), l.baseline, fs, c0 === '\f' ? 'page' : 'column', false);
            }
          }
          ctx.textAlign = 'center';
        }
      });
    }
    ctx.restore();
  };

  // ── o controle do sumário (moldura e aba com o cursor dentro; só Layout de Impressão, no corpo) ──
  let tocMeasure: CanvasRenderingContext2D | null = null;
  const tocBoxes = (): { pg: number; x: number; y: number; w: number; h: number }[] => {
    const e = el();
    if (!e || !c.s || !c.dl || e.printRender || e.view !== 'print' || storyCtx(c)?.kind !== 'body') return [];
    const r = c.json<[number, number] | null>(14, [e.selection.focus.para]);
    if (!r) return [];
    const by = new Map<number, { y0: number; y1: number }>();
    for (let pi = r[0]; pi <= r[1]; pi++) {
      for (const l of paraLines(c, pi)) {
        const b = by.get(l.page);
        by.set(l.page, b ? { y0: Math.min(b.y0, l.y), y1: Math.max(b.y1, l.y + l.height) } : { y0: l.y, y1: l.y + l.height });
      }
    }
    return [...by].sort((a, b) => a[0] - b[0]).flatMap(([pg, b]) => {
      const sp = section(sectionOfPage(pg))?.sectPr;
      if (!sp) return [];
      const x = sp.marginLeftTwips;
      return [{ pg, x, y: b.y0, w: sp.pageWidthTwips - sp.marginRightTwips - x, h: b.y1 - b.y0 }];
    });
  };
  const tocTabGeom = (): { pg: number; th: number; fs: number; label: string; menu: [number, number, number, number]; update: [number, number, number, number] } | null => {
    const e = el();
    const b = tocBoxes()[0];
    if (!b || !e) return null;
    const px = 1 / (PX_PER_TWIP * e.zoom);
    const pad = 3 * px, th = 20 * px, fs = 11.5 * px;
    const label = pickLabels(e, { en: 'Update Table…', pt: 'Atualizar Sumário…', es: 'Actualizar tabla…' });
    tocMeasure ??= document.createElement('canvas').getContext('2d');
    let tw = label.length * fs * 0.55;
    if (tocMeasure) { tocMeasure.font = `400 ${fs}px ${DEFAULT_FONT_CSS}`; tw = tocMeasure.measureText(label).width; }
    const x0 = b.x - pad, y0 = b.y - pad - th;
    return { pg: b.pg, th, fs, label, menu: [x0, y0, th, th], update: [x0 + th, y0, tw + 14 * px, th] };
  };
  const paintTocControl = (ctx: CanvasRenderingContext2D, pg: number): void => {
    const e = el();
    if (!e) return;
    const boxes = tocBoxes();
    const b = boxes.find((x) => x.pg === pg);
    if (!b) return;
    const px = 1 / (PX_PER_TWIP * e.zoom);
    const dark = e.pageDark;
    const line = dark ? '#5f6b7a' : '#a9bcd0', bg = dark ? '#2b3138' : '#eef3f8', ink = dark ? '#e3e3e3' : '#3c4043';
    ctx.save();
    ctx.lineWidth = px;
    ctx.strokeStyle = line;
    const pad = 3 * px;
    ctx.strokeRect(b.x - pad, b.y - pad, b.w + 2 * pad, b.h + 2 * pad);
    const g = boxes[0] === b ? tocTabGeom() : null;
    if (g) {
      for (const r of [g.menu, g.update]) { ctx.fillStyle = bg; ctx.fillRect(...r); ctx.strokeRect(...r); }
      const [mx, my, mw] = g.menu;
      ctx.strokeStyle = ink;
      ctx.beginPath();
      const ls = [[0.25, 0.3, 0.75], [0.33, 0.45, 0.75], [0.33, 0.6, 0.75], [0.25, 0.75, 0.75]] as const;
      for (const [x1, y, x2] of ls) { ctx.moveTo(mx + mw * x1, my + mw * y); ctx.lineTo(mx + mw * x2, my + mw * y); }
      ctx.stroke();
      ctx.fillStyle = ink;
      ctx.font = `400 ${g.fs}px ${DEFAULT_FONT_CSS}`;
      ctx.textBaseline = 'middle';
      ctx.fillText(g.label, g.update[0] + 7 * px, g.update[1] + g.th / 2);
    }
    ctx.restore();
  };
  /** the pointer in page twips (the TS `engPoint`) */
  const engPoint = (e: MouseEvent): { pg: number; x: number; y: number } | null => {
    const h = el();
    if (!h) return null;
    const scale = PX_PER_TWIP * h.zoom;
    for (let pg = 0; pg < h.canvases.length; pg++) {
      const r = h.canvases[pg]!.getBoundingClientRect();
      if (e.clientX >= r.left && e.clientX <= r.right && e.clientY >= r.top && e.clientY <= r.bottom) return { pg, x: (e.clientX - r.left) / scale, y: (e.clientY - r.top) / scale };
    }
    return null;
  };
  const tocTabMouseDown = (e: MouseEvent): boolean => {
    if (e.button !== 0) return false;
    const g = tocTabGeom();
    const p = g ? engPoint(e) : null;
    if (!g || !p || p.pg !== g.pg) return false;
    const inR = (r: [number, number, number, number]): boolean => p.x >= r[0] && p.x <= r[0] + r[2] && p.y >= r[1] && p.y <= r[1] + r[3];
    if (inR(g.update)) {
      e.preventDefault();
      if (!c.locked()) {
        // `requestTocUpdate('caret')`: the bar cancels the event and asks; without one the whole table
        const ev = new CustomEvent('uw-toc-update', { cancelable: true, detail: { scope: 'caret' } });
        if (c.el?.dispatchEvent(ev)) { (c.el as unknown as { updateToc(m: string): void }).updateToc('all'); sync(c); }
      }
      return true;
    }
    if (inR(g.menu)) {
      e.preventDefault();
      c.el?.dispatchEvent(new CustomEvent('uw-toc-menu', { detail: { x: e.clientX, y: e.clientY } }));
      return true;
    }
    return false;
  };

  // ── inlines de plugin com desenho próprio (sobre a caixa do texto de reserva; também na impressão) ──
  const paintEngineInlines = (ctx: CanvasRenderingContext2D, pg: number, scale: number): void => {
    const h = c.el as unknown as { inlineRenderers?: Map<string, { renderer?: { draw?: (a: Record<string, unknown>) => void; toText?: (d: unknown) => string } }>; pageDark: boolean; canvasFont?: unknown } | null;
    if (!h?.inlineRenderers?.size || !c.s || !c.dl) return;
    const inl = new Map<number, { at: number; custom: { type: string; data: unknown; fallbackText: string }; props: Record<string, unknown> }[]>();
    ctx.save();
    ctx.setTransform(1, 0, 0, 1, 0, 0);
    for (const l of pageLines(c, pg)) {
      let list = inl.get(l.para);
      if (!list) { list = c.json(15, [l.para]) ?? []; inl.set(l.para, list); }
      for (const it of list) {
        const renderer = h.inlineRenderers.get(it.custom.type)?.renderer;
        const k = l.o.indexOf(it.at);
        if (!renderer?.draw || k < 0) continue;
        const cx = l.cx[k]!, cw = l.cw[k]!;
        const sizePt = ((it.props['szHalfPt'] as number | undefined) ?? 22) / 2;
        const family = (it.props['fontAscii'] as string | undefined) ?? 'Liberation Sans';
        const css = { family: `"${family}", ${DEFAULT_FONT_CSS}`, weight: it.props['bold'] ? 700 : 400, italic: !!it.props['italic'] };
        const color = it.props['color'] as string | undefined;
        const ink = color && color !== 'auto' ? `#${color}` : h.pageDark ? '#e8eaed' : '#000000';
        const fallbackText = renderer.toText?.(it.custom.data) ?? it.custom.fallbackText;
        const measure = document.createElement('canvas').getContext('2d');
        const measureText = (text: string): number => {
          if (!measure) return text.length * sizePt * 10;
          measure.font = `${css.italic ? 'italic ' : ''}${css.weight} ${sizePt * 20}px ${css.family}`;
          return measure.measureText(text).width;
        };
        onLine(ctx, { ...l, xf: l.xf ? [l.xf[0]!, l.xf[1]!, l.xf[2]!, l.xf[3]!, l.xf[4]! * scale, l.xf[5]! * scale] : undefined } as ShellLine, () => {
          ctx.fillStyle = h.pageDark ? '#1e1e1e' : '#ffffff';
          ctx.fillRect(cx * scale, l.y * scale, cw * scale, l.height * scale);
          ctx.font = `${css.italic ? 'italic ' : ''}${css.weight} ${sizePt * 20 * scale}px ${css.family}`;
          ctx.fillStyle = ink;
          renderer.draw!({
            data: it.custom.data, fallbackText, props: it.props, sizePt, measureText, ctx,
            xTwips: cx, yTwips: l.y, baselineTwips: l.baseline, widthTwips: cw, heightTwips: l.height,
            scale, dark: h.pageDark, color: ink,
          });
        });
      }
    }
    ctx.restore();
  };

  const pointer = (e: MouseEvent): boolean => {
    if (e.button === 0 && tocTabMouseDown(e)) return true;
    return false;
  };
  /**
   * The Format Painter applies when the DEFAULT text click ends (the TS `up` of 10311: `if (this.painterProps) this.applyPainter()`,
   * after the drag and the Ctrl sentence), from the shell's `clickUp` hook: a click another front handles (a floating object, a
   * picture, Ctrl+click on a link, a cell selection) leaves the painter armed, as the TS
   */
  const painterUp = (): void => {
    const s = c.s, api = c.api;
    if (!s || !api || !c.st.painter) return;
    if (!c.st.painterLock) c.st.painter = false;
    api.apply((s as unknown as { applyPainter(): ExecOutcome | null }).applyPainter(), 'applyPainter');
    sync(c);
  };
  const key = (e: KeyboardEvent): boolean => {
    if (e.key !== 'Escape') return false;
    // Esc: the painter (universal-write.ts 10680; Select Objects' Esc is front A2's)
    if (c.st.painter) { e.preventDefault(); c.st.painter = false; c.st.painterLock = false; sync(c); return true; }
    return false;
  };

  // ── os modos de exibição Web/Rascunho/Estrutura (a vista do motor do shell; Imprimir e Leitura são da frente O) ──
  const viewKind = (v: string): number => (v === 'web' ? 1 : v === 'draft' ? 2 : v === 'outline' ? 3 : 0);
  /** the TS `contMarginTwips()` (universal-write.ts 10764: 600, 240 on a narrow element) and `contGutterPx()` (48 / 0): the
   *  element's own methods (the shell IS the element) */
  const contMargin = (): number => {
    const e = el() as unknown as { contMarginTwips?: () => number } | null;
    return typeof e?.contMarginTwips === 'function' ? e.contMarginTwips() : 600;
  };
  const contGutter = (): number => {
    const e = el() as unknown as { contGutterPx?: () => number } | null;
    return typeof e?.contGutterPx === 'function' ? e.contGutterPx() : 48;
  };
  /** the TS Web canvas width in twips: `contentWidthTwips() + 2 × contMarginTwips()` (10777, 21559) */
  const webWidth = (): number => {
    const e = el();
    if (!e) return 0;
    const m = contMargin();
    return Math.max(2000, (e.pagesViewportWidth() - contGutter()) / (PX_PER_TWIP * e.zoom) - 2 * m) + 2 * m;
  };
  /** wave 7c (V): the harness's strict replays against the frozen TS (`ed.tsStrict = true`): the TS rules where Word differs */
  const tsStrict = (): boolean => !!(c.ed as unknown as { tsStrict?: boolean }).tsStrict;
  /**
   * Wave 7c (V, ED-707/708 closed): the editor's continuous views are ONE sheet (`ViewTransform::Continuous`, uo-wasm
   * `uo_shell_engine_set_continuous`): the TS margin (`contMarginTwips`), the TS sheet width (Web: the viewport minus the
   * gutter; Draft/Outline: the page width, as the TS canvas), no new sheet at a section, page or column break; the content
   * as Word lays it out in those views (each section at its own text width, Word's outline levels; `tsStrict` = the TS's).
   */
  /** the Web sheet width (twips) the engine was last laid out at (null: not in Web view) */
  let appliedWeb: number | null = null;
  const applyView = (v: string): void => {
    const eng = c.api?.shellEngine, fb = c.fb;
    if (!eng || !fb) return;
    const k = viewKind(v);
    appliedWeb = k === 1 ? webWidth() : null;
    const f = fb as unknown as { setContinuous?: (e: unknown, k: number, w: number, m: number, t: boolean) => void };
    if (k && typeof f.setContinuous === 'function') f.setContinuous(eng, k, k === 1 ? webWidth() : 0, contMargin(), tsStrict());
    else fb.setView(eng, k, k === 1 ? webWidth() : 0);
  };
  c.viewApply = applyView;
  /**
   * Wave 7c (V, fix round 2): the Web view laid out again at the viewport's width when it changed — the TS lays a continuous
   * view out on every `refresh()` and every selection change (`refreshSelection` without an engine line → `refresh()`,
   * 21379; its `contentWidthTwips()` reads the element's width then). True when it laid the document out (and painted).
   */
  const webRelayout = (): boolean => {
    const e = el();
    if (!e || e.view !== 'web' || !c.s || !c.api) return false;
    if (appliedWeb !== null && Math.abs(webWidth() - appliedWeb) < 0.5) return false;
    applyView('web');
    c.api.relayoutNow();
    return true;
  };
  const setView = (v: string): void => {
    const e = el();
    if (!e || !c.api?.session) return;
    const base = prev['setView'] as ((v: string) => void) | undefined;
    if (v === 'print' || v === 'read') {
      // wave 7 (F1): the TS (2886) leaves the cell for every view but Print — the Read view too (`uw-cellmode`, the body
      // selection back); found by editor_switch_fidelity.mjs 'read view paste'
      if (v === 'read' && storyCtx(c)?.kind === 'cell') c.run({ t: 'context', v: { t: 'exitCell' } } as never, 'setView');
      applyView('print');
      base?.(v);
      return;
    }
    if (v !== 'web' && v !== 'draft' && v !== 'outline') return;
    // as the TS (2921): out of the cell first; the Read view's zoom restored
    if (storyCtx(c)?.kind === 'cell') c.run({ t: 'context', v: { t: 'exitCell' } } as never, 'setView');
    const wasRead = e.view === 'read';
    if (wasRead) {
      // the Read view off (the session's read flag, the zoom): front O's print path, then this view
      base?.('print');
    }
    const z0 = e.zoom;
    e.view = v;
    e.classList.remove('view-print', 'view-web', 'view-draft', 'view-read', 'view-outline');
    e.classList.add(`view-${v}`);
    applyView(v);
    e.refresh();
    e.scrollTop = 0;
    if (e.zoom !== z0) e.dispatchEvent(new CustomEvent('uw-zoom', { detail: { zoom: e.zoom } }));
    e.chromeSync?.();
  };

  /**
   * `paintTocTemplate(id, canvas, cssWidth)` (universal-write.ts 7123): the preview mini-document of the TOC gallery laid out by
   * the session (uo_shell_fmt_toc_template) and painted by the shared painter, cropped as the TS (the lines ± 160 twips,
   * the margins ± 240).
   */
  const paintTocTemplate = (id: string, canvas: HTMLCanvasElement, cssWidth: number): boolean => {
    const s = c.s, api = c.api, fb = c.fb, env = api?.pageEnv;
    const idx = ['auto1', 'auto2', 'manual'].indexOf(id);
    if (!s || !api || !fb || !env || idx < 0) return false;
    const crop = fb.tocCrop(s, idx);
    let dl: ReturnType<typeof api.module.adopt> | null = null;
    try {
      dl = api.module.adopt(fb.tocTemplate(s, env.inner, idx, new Date()));
      const page = dl.pages[0];
      const lines = page?.lines ?? [];
      if (!page || !lines.length || !crop) return false;
      const top = Math.max(0, Math.min(...lines.map((l) => l.y)) - 160);
      const bottom = Math.max(...lines.map((l) => l.y + l.height)) + 160;
      const left = Math.max(0, crop.cropLeft), right = Math.min(page.widthTw, page.widthTw - crop.marginRight + 240);
      const dpr = window.devicePixelRatio || 1;
      const scale = cssWidth / (right - left);
      canvas.width = Math.max(1, Math.round((right - left) * scale * dpr));
      canvas.height = Math.max(1, Math.round((bottom - top) * scale * dpr));
      canvas.style.width = `${Math.round((right - left) * scale)}px`;
      canvas.style.height = `${Math.round((bottom - top) * scale)}px`;
      const ctx = canvas.getContext('2d')!;
      ctx.setTransform(1, 0, 0, 1, 0, 0);
      ctx.fillStyle = el()?.pageDark ? '#1e1e1e' : '#ffffff';
      ctx.fillRect(0, 0, canvas.width, canvas.height);
      ctx.setTransform(scale * dpr, 0, 0, scale * dpr, -left * scale * dpr, -top * scale * dpr);
      const penv = api.paintEnv(dl);
      if (penv) dl.paintItems(ctx, 0, penv, {});
      return true;
    } catch {
      return false;
    } finally {
      dl?.free();
    }
  };

  return {
    handlers: {
      getRulerInfo: getRulerInfo as never,
      /** `refresh()`: in Web view the sheet follows the viewport's width first (the TS relays the Web view out on refresh: side
       *  panels, 1586), then the shell's refresh */
      refresh: (() => {
        const e = el();
        if (e?.view === 'web') applyView('web');
        (c.ed as unknown as { refresh(): void }).refresh();
      }) as never,
      showGridlines: ((on: boolean) => {
        if (c.st.grid === !!on) return;
        c.st.grid = !!on;
        c.api?.invalidateBase();
        c.api?.paint();
        c.el?.chromeSync?.();
      }) as never,
      gridlinesShown: (() => c.st.grid) as never,
      paintTocTemplate: paintTocTemplate as never,
      showBookmarks: ((on?: boolean) => {
        c.st.bookmarks = on ?? !c.st.bookmarks;
        (c.el as unknown as { refreshView?: () => void } | null)?.refreshView?.();
        return c.st.bookmarks;
      }) as never,
      bookmarksShown: (() => c.st.bookmarks) as never,
      setView: setView as never,
      // the private paint methods (an `el[m] = () => {}` switches them off in both engines)
      paintBookmarkMarks: paintBookmarkMarks as never,
      paintTocControl: paintTocControl as never,
      paintEngineInlines: paintEngineInlines as never,
      paintDocGrid: paintDocGrid as never,
      tocTabMouseDown: tocTabMouseDown as never,
    },
    paintBase: (ctx, page, scale) => {
      const m = (c.el as unknown as { paintDocGrid?: (ctx: CanvasRenderingContext2D, p: number, s: number) => void } | null)?.paintDocGrid;
      m?.call(c.el, ctx, page, scale);
    },
    paintOver: (ctx, page, scale) => {
      const h = c.el as unknown as { paintTocControl?: (ctx: CanvasRenderingContext2D, p: number) => void } | null;
      h?.paintTocControl?.call(c.el, ctx, page);
    },
    paintContent: (ctx, page, scale) => {
      const h = c.el as unknown as { paintEngineInlines?: (ctx: CanvasRenderingContext2D, p: number, s: number) => void } | null;
      h?.paintEngineInlines?.call(c.el, ctx, page, scale);
    },
    /**
     * The shown height of sheet `i` in the Web/Draft/Outline views (null in Print/Read): the engine lays each section out
     * on one 633,600-twip sheet (`WEB_PAGE_H`, D-A1-1); the canvas ends at the body's content plus the 288-twip margin, as
     * the viewer's Web view (`webPageHeight`, the same function: views.ts 62) — the TS legacy view ends its strip at the
     * content plus 600 (its margin, ED-707). Hit-testing reads the canvas box (`engPoint`/`hitAt`), so it follows.
     */
    pageHeight: (i) => {
      const e = el(), p = c.dl?.pages[i];
      if (!e || !p || !viewKind(e.view)) return null;
      // wave 7c (V): the one sheet down to the content (the body lines and the objects next to them, `webPageHeight`'s
      // rule) plus the TS margin (`continuousHeight = y + 600`, 12804), not the viewer's 288
      const m = contMargin();
      return Math.min(p.heightTw, Math.max(2 * m, webPageHeight(p as unknown as WebPage) - 288 + m));
    },
    pointer,
    painterUp,
    key,
    adopt: () => {
      if (!zoomHooked && c.el) {
        zoomHooked = true;
        // a zoom change in Web view: the sheet width in twips follows (the TS legacy Web view fills the viewport at any zoom)
        c.el.addEventListener('uw-zoom', () => { if (el()?.view === 'web' && c.s) { applyView('web'); (c.ed as unknown as { refresh(): void }).refresh(); } });
        // wave 7c (V, fix round 2) — Word manda sobre o TS: Word's Web view wraps again at once when its window is resized
        // (measured: word-v3.ps1, window 1100 → 900 with no view, zoom or selection change: the right-aligned paragraph's
        // end 1045 → 845 px, the long paragraph 2 → 3 lines); the frozen TS keeps its sheet until its next `refresh()` or
        // selection change (`measureSides` returns when no side panel changed) — `tsStrict` keeps that
        if (typeof ResizeObserver !== 'undefined') {
          new ResizeObserver(() => { if (!tsStrict()) webRelayout(); }).observe(c.el);
        }
      }
      c.st.painter = false; c.st.painterLock = false; c.st.hyph = false;
      vs.sections.clear();
      // a new document: a new shell engine, laid out in the element's view
      const e = el();
      if (e && viewKind(e.view)) applyView(e.view);
    },
    paintMarks,
    selectionRelayout: () => webRelayout(),
  };
}

