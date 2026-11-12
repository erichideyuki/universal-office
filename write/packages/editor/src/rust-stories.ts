// Frente S da leva 5 do engine="rust" (registro core/docs/modules/uo-bindings.md "Wave 5 — S: stories"): o cursor em
// outra história ou escopo — células de tabela, cabeçalho/rodapé, notas, o texto de uma caixa de texto. O estado por
// editor, os ganchos de entrada/pintura (os `hooks` do RustEditor) e as portas do TS que leem a tela (as regras de
// página do clique, as faixas do cabeçalho/rodapé). O que lê o layout e o modelo é do núcleo (`bindStories`:
// core/crates/uo-wasm/js/session-stories.mjs → src/session/shell_stories.rs). Carregado só com o motor Rust.
import {
  bindStories,
  type ExecOutcome,
  type SessionCommand,
  type StoriesBinding,
  type StoryContext,
  type StoryHfPage,
  type StoryHop,
  type StoryTableState,
} from '@universal-office/write-session';
import type { DecodedLayout } from '@universal-office/write-engine';
import { uiLang } from '@universal-office/ui';
import type { FrontApi, RustEditor } from './rust-engine.js';
import { hfBandLabel, sameAsPreviousLabel } from './hf-templates.js';
import { dblHook, keyHook, lateKeyHook, latePointer, moveHook, textboxPointer } from './rust-stories-input.js';

export const PX_PER_TWIP = 96 / 1440;
const TWIPS_PER_PT = 20;
const SEL_FILL = 'rgba(58,114,196,0.35)';
/** `DEFAULT_FONT_CSS` of universal-write.ts */
const DEFAULT_FONT_CSS = '"Liberation Sans"';

export type Pos = { para: number; offset: number };
export type HfKindName = 'header' | 'footer';

/** O estado da frente S de um editor. */
export interface StState {
  readonly ed: RustEditor;
  readonly api: FrontApi | null;
  sb: StoriesBinding | null;
  /** Mostrar Texto do Documento desligado (o `hideDocText` do TS) */
  hideDocText: boolean;
  /** o contexto em cache (por geração do editor e sessão) */
  ctxCache: { gen: number; session: unknown; v: StoryContext } | null;
  /** os ganchos já registrados */
  hooked: boolean;
  lateHooked: boolean;
}

const states = new WeakMap<RustEditor, StState>();


export function stOf(ed: RustEditor): StState {
  let s = states.get(ed);
  if (!s) {
    // o RustEditor sem sessão do teste de rotas não tem frontApi
    const api: FrontApi | null = typeof ed.frontApi === 'function' ? ed.frontApi() : null;
    s = { ed, api, sb: null, hideDocText: false, ctxCache: null, hooked: false, lateHooked: false };
    states.set(ed, s);
  }
  return s;
}

/** O binding das histórias (preguiçoso: o módulo do editor). */
export function sbOf(st: StState): StoriesBinding | null {
  if (st.sb) return st.sb;
  const mod = st.api?.module;
  if (!mod) return null;
  try { st.sb = bindStories(mod.exports, mod.binding); } catch { st.sb = null; }
  return st.sb;
}

/** Sessão, layout e binding de agora (null sem documento). */
export function live(st: StState): { api: FrontApi; s: NonNullable<FrontApi['session']>; dl: DecodedLayout; sb: StoriesBinding } | null {
  const api = st.api;
  const s = api?.session;
  const dl = api?.dl;
  const sb = sbOf(st);
  if (!api || !s || !dl || !sb) return null;
  return { api, s, dl, sb };
}

/** O contexto da sessão (`cellEdit`, `noteEdit`, `hfEdit`, `tbEdit`, `tableSel`), em cache por geração. */
export function ctxOf(st: StState): StoryContext | null {
  const api = st.api;
  const s = api?.session;
  const sb = sbOf(st);
  if (!s || !sb) return null;
  const gen = api.gen;
  if (st.ctxCache && st.ctxCache.gen === gen && st.ctxCache.session === s) return st.ctxCache.v;
  const v = sb.storyContext(s);
  st.ctxCache = { gen, session: s, v };
  return v;
}

/** O contexto de fora de uma caixa de texto (o TS mantém `hfEdit` com a caixa aberta dentro do cabeçalho). */
export function outerOf(c: StoryContext['ctx']): StoryContext['ctx'] {
  return c.kind === 'textbox' ? outerOf(c.outer) : c;
}

/** O cabeçalho/rodapé em edição (também com uma caixa de texto dele aberta). */
export function hfEditOf(st: StState): { kind: HfKindName; part: number; page: number } | null {
  const c = ctxOf(st);
  if (!c) return null;
  const o = outerOf(c.ctx);
  return o.kind === 'hf' ? { kind: o.hfKind, part: o.part, page: o.page } : null;
}

export function sameHops(a: readonly StoryHop[], b: readonly StoryHop[]): boolean {
  return a.length === b.length && a.every((h, i) => h.r === b[i]!.r && h.c === b[i]!.c && (i === a.length - 1 || h.b === b[i]!.b));
}

/** Os saltos do TS no formato do fio (`{ row, col, block? }`). */
export function wireHops(hops: readonly StoryHop[]): { row: number; col: number; block?: number }[] {
  return hops.map((h) => (h.b !== undefined ? { row: h.r, col: h.c, block: h.b } : { row: h.r, col: h.c }));
}

/** A página que contém o ponto (x E y dentro do canvas: a regra de `engLineAt`/`hfZoneAt`), em twips. */
export function pageAt(st: StState, e: MouseEvent): { page: number; x: number; y: number } | null {
  const el = st.ed.el;
  const scale = PX_PER_TWIP * el.zoom;
  for (let pg = 0; pg < el.canvases.length; pg++) {
    const r = el.canvases[pg]!.getBoundingClientRect();
    if (e.clientY < r.top || e.clientY > r.bottom || e.clientX < r.left || e.clientX > r.right) continue;
    return { page: pg, x: (e.clientX - r.left) / scale, y: (e.clientY - r.top) / scale };
  }
  return null;
}

/** O ponto em twips na página `pg` (mesmo fora do canvas). */
export function pointOn(st: StState, e: MouseEvent, pg: number): { x: number; y: number } | null {
  const c = st.ed.el.canvases[pg];
  if (!c) return null;
  const r = c.getBoundingClientRect();
  const scale = PX_PER_TWIP * st.ed.el.zoom;
  return { x: (e.clientX - r.left) / scale, y: (e.clientY - r.top) / scale };
}

/** Uma resposta da sessão aplicada (seleção, eventos, barras). */
export function apply(st: StState, o: ExecOutcome | null, what: string): boolean {
  if (!st.api) return false;
  st.ctxCache = null;
  return st.api.apply(noteEvents(o), what);
}

/**
 * The `uw-notemode` detail of the TS (`{ active: true, kind, id }`, `{ active: false }`) from the session's event
 * (`{ active, footnote, id }`): the shell's form of the event (ED-103).
 */
function noteEvents(o: ExecOutcome | null): ExecOutcome | null {
  if (!o || o.t !== 'done' || !o.events.some((e) => e.t === 'uw-notemode')) return o;
  const events = o.events.map((e) => {
    if (e.t !== 'uw-notemode') return e;
    const v = e as { t: string; active?: boolean; footnote?: boolean; id?: number };
    return v.active ? { t: 'uw-notemode', active: true, kind: v.footnote ? 'footnote' : 'endnote', id: v.id ?? 0 } : { t: 'uw-notemode', active: false };
  });
  return { ...o, events };
}

/** Um comando da sessão, a resposta (sem aplicar). Erros de codificação viram null (avisados uma vez). */
export function run(st: StState, cmd: SessionCommand, what: string): ExecOutcome | null {
  const s = st.api?.session;
  if (!s) return null;
  try { return s.command(cmd); } catch (err) { st.api?.warnOnce(`stories:${what}`, `[universal-write] engine="rust": ${what} failed`, err); return null; }
}

/** Um comando aplicado. */
export function exec(st: StState, cmd: SessionCommand, what: string): boolean {
  return apply(st, run(st, cmd, what), what);
}

/** Uma consulta da sessão (null quando falha). */
export function query<T = Record<string, unknown>>(st: StState, q: { t: string } & Record<string, unknown>): T | null {
  const s = st.api?.session;
  if (!s) return null;
  try { return s.query(q) as unknown as T; } catch { return null; }
}

/** A mudança de documento de uma resposta. */
export function changed(o: ExecOutcome | null): boolean {
  return !!o && o.t === 'done' && (o.stories.length > 0 || !!o.formatting);
}

/** O estado da tabela (`getTableState`), em cache por geração. */
const tableCache = new WeakMap<StState, { gen: number; dl: unknown; v: StoryTableState | null }>();
export function tableStateOf(st: StState): StoryTableState | null {
  const api = st.api;
  const s = api?.session;
  const sb = sbOf(st);
  if (!s || !sb) return null;
  const gen = api?.gen ?? 0;
  const dl = api?.dl ?? null;
  const c = tableCache.get(st);
  if (c && c.gen === gen && c.dl === dl) return c.v;
  let v: StoryTableState | null = null;
  try { v = sb.tableState(s, dl ? dl.inner : null); } catch { v = null; }
  tableCache.set(st, { gen, dl, v });
  return v;
}

/** O `TableState` do TS (sem os argumentos que a casca usa: âncora, caminho, alturas exibidas). */
export function tsTableState(v: StoryTableState | null): Record<string, unknown> | null {
  if (!v) return null;
  const { anchor: _a, path: _p, layoutIn: _l, ...rest } = v;
  return rest;
}

/** O `TableLayoutIn` dos comandos de tabela: as alturas exibidas das linhas da tabela alvo. */
export function layoutIn(st: StState): { rowHeights: number[] } {
  return { rowHeights: tableStateOf(st)?.layoutIn.rowHeights ?? [] };
}

/** Os fatos do cabeçalho/rodapé da página `pg`. */
export function hfPageOf(st: StState, pg: number): StoryHfPage | null {
  const l = live(st);
  if (!l) return null;
  try { return l.sb.hfPage(l.s, l.dl.inner, pg); } catch { return null; }
}

/** A página do cursor (no corpo) ou a página em edição (o `caretPageIndex` do TS). */
export function caretPage(st: StState): number {
  if (!st.api) return 0;
  const hf = hfEditOf(st);
  if (hf) return hf.page;
  const n = st.api?.dl?.pages.length ?? 1;
  const c = ctxOf(st)?.ctx;
  // na célula ou na nota: a página da linha do corpo não está no escopo (o TS usa a página vista)
  const p = c && c.kind === 'body' ? st.api?.caret()?.page : undefined;
  return Math.max(0, Math.min(p ?? st.ed.currentPage() - 1, n - 1));
}

// ───────────────────────── pintura ─────────────────────────

/** As faixas do cabeçalho/rodapé no modo de edição (`paintHfBands` 12427), em pixels do dispositivo. */
function paintHfBands(st: StState, ctx: CanvasRenderingContext2D, scale: number, pg: number, hf: { kind: HfKindName; page: number }): void {
  const hp = hfPageOf(st, pg);
  if (!hp) return;
  const el = st.ed.el;
  const dark = el.pageDark;
  const dpr = window.devicePixelRatio || 1;
  const pageW = hp.width;
  const mL = hp.marginLeft ?? 1440, mR = hp.marginRight ?? 1440;
  const lang = uiLang(el);
  const draw = (kind: HfKindName): void => {
    const side = kind === 'header' ? hp.header : hp.footer;
    const lineY = kind === 'header' ? side.bottom : side.top;
    const bandTop = kind === 'header' ? 0 : lineY;
    const bandBot = kind === 'header' ? lineY : hp.height;
    const active = kind === hf.kind && pg === hf.page;
    ctx.save();
    ctx.fillStyle = active ? (dark ? 'rgba(80,130,200,0.16)' : 'rgba(43,87,154,0.09)') : (dark ? 'rgba(120,120,120,0.10)' : 'rgba(120,120,120,0.06)');
    ctx.fillRect(0, bandTop * scale, pageW * scale, (bandBot - bandTop) * scale);
    ctx.strokeStyle = active ? (dark ? '#5b8def' : '#2b579a') : (dark ? '#777' : '#9aa0a6');
    ctx.setLineDash([6, 4]);
    ctx.lineWidth = Math.max(1, dpr);
    ctx.beginPath();
    ctx.moveTo(mL * scale, lineY * scale);
    ctx.lineTo((pageW - mR) * scale, lineY * scale);
    ctx.stroke();
    ctx.setLineDash([]);
    // as etiquetas (`hfBandLabels`): a variante da página pela regra do Word, a seção, "Mesmo que o Anterior"
    const name = hfBandLabel(kind, hp.type, hp.evenOdd, Math.min(hp.section, hp.sections - 1), hp.sections, lang);
    const same = side.linked ? sameAsPreviousLabel(lang) : null;
    ctx.font = `${8 * TWIPS_PER_PT * scale}px ${DEFAULT_FONT_CSS}`;
    const th = 13 * TWIPS_PER_PT * scale, pad = 4 * TWIPS_PER_PT * scale;
    const ty = kind === 'header' ? lineY * scale : lineY * scale - th;
    const tag = (text: string, x: number, right: boolean): void => {
      const w = ctx.measureText(text).width + pad * 2;
      const x0 = right ? x - w : x;
      ctx.fillStyle = dark ? '#2d2f33' : '#eef2f8';
      ctx.fillRect(x0, ty, w, th);
      ctx.strokeStyle = active ? (dark ? '#5b8def' : '#2b579a') : (dark ? '#777' : '#9aa0a6');
      ctx.lineWidth = Math.max(1, dpr) * 0.75;
      ctx.strokeRect(x0, ty, w, th);
      ctx.fillStyle = active ? (dark ? '#9bc0ff' : '#2b579a') : (dark ? '#c4c7c5' : '#5f6368');
      ctx.textBaseline = 'middle';
      ctx.fillText(text, x0 + pad, ty + th / 2);
    };
    tag(name, mL * scale, false);
    if (same) tag(same, (pageW - mR) * scale, true);
    ctx.restore();
  };
  draw('header');
  draw('footer');
}

/** As linhas de grade das tabelas (Exibir Linhas de Grade): na base da página, sob o conteúdo, como o TS. */
function paintGridlines(st: StState, ctx: CanvasRenderingContext2D, pg: number, scale: number): void {
  const l = live(st);
  if (!l) return;
  const boxes = l.sb.cellBoxes(l.dl.inner, pg);
  if (!boxes.length) return;
  // `paintGridlines` (17129): the context is in page twips, `scale` the device pixels per twip
  ctx.save();
  ctx.strokeStyle = st.ed.el.pageDark ? 'rgba(120,170,230,0.75)' : 'rgba(80,140,210,0.75)';
  ctx.lineWidth = 1 / scale;
  ctx.setLineDash([2 / scale, 2 / scale]);
  ctx.beginPath();
  for (const b of boxes) ctx.rect(b.x, b.y, b.w, b.h);
  ctx.stroke();
  ctx.restore();
}

// ───────────────────────── os ganchos ─────────────────────────


/** A paint function of this frontier, tagged (`front: 'S'`: the harness keeps only these in its captures). */
function mine<F extends (...a: never[]) => unknown>(f: F): F {
  return Object.assign(f, { front: 'S' });
}

/** Registra os ganchos da frente (uma vez por editor). */
export function hook(st: StState): void {
  if (st.hooked) return;
  st.hooked = true;
  const ed = st.ed;
  const hooks = ed.hooks as RustEditor['hooks'] | undefined;
  if (!hooks || !st.api) return;
  // a ordem do TS: as teclas da seleção de células antes das da frente O; o Esc depois (ver `lateKeyHook`)
  hooks.key.push((e) => keyHook(st, e));
  // o clique numa caixa de texto (a entrada vem antes da seleção de objetos da frente O)
  hooks.pointer.push((e) => textboxPointer(st, e));
  hooks.pointerLate.push((e) => latePointer(st, e));
  hooks.dbl.push((e) => dblHook(st, e));
  hooks.move.push((dir, extend) => moveHook(st, dir, extend));
  hooks.state.push((v) => { v['table'] = tsTableState(tableStateOf(st)); });
  hooks.hideSelection.push(() => !!ctxOf(st)?.tableSel);
  hooks.baseKey.push(() => {
    const hf = hfEditOf(st);
    return hf ? `|hf${st.hideDocText ? 2 : 1}` : '';
  });
  const prevLayers = hooks.layers;
  hooks.layers = (page, hf, n, dim) => {
    if (hfEditOf(st)) {
      // o Word esmaece o corpo ao editar o cabeçalho/rodapé; Mostrar Texto do Documento desligado: o corpo some
      const out: [number, number, number][] = [[0, hf, 1]];
      if (!(st.hideDocText && !st.ed.el.printRender)) out.push([hf, n, dim]);
      return out;
    }
    return prevLayers ? prevLayers(page, hf, n, dim) : null;
  };
  // as linhas de grade das tabelas: na base da página, sobre o fundo e sob o conteúdo (o `engineBase` do TS 16639)
  const gridOn = (): boolean => !!(st.ed.el as unknown as { gridlines?: boolean }).gridlines;
  hooks.baseKey.push(() => (gridOn() ? '|grid' : ''));
  hooks.under.push((ctx: CanvasRenderingContext2D, page: number, scale: number) => { if (gridOn()) paintGridlines(st, ctx, page, scale); });
  hooks.overlay.push(mine((ctx: CanvasRenderingContext2D, page: number, scale: number) => {
    const c = ctxOf(st);
    if (!c) return;
    const l = live(st);
    if (c.tableSel && l) {
      ctx.fillStyle = SEL_FILL;
      for (const b of l.sb.tableSelRects(l.s, l.dl.inner, page)) ctx.fillRect(b.x, b.y, b.w, b.h);
    }
    const hf = hfEditOf(st);
    if (hf) {
      // the TS paints the bands in device pixels (after its overlays: `paintEngine`)
      ctx.save();
      ctx.setTransform(1, 0, 0, 1, 0, 0);
      paintHfBands(st, ctx, scale, page, hf);
      ctx.restore();
    }
  }));
  // depois das fábricas das outras frentes (o 1º documento): o Esc da caixa/nota/cabeçalho/célula depois do da
  // seleção de objetos (frente O), como no TS
  hooks.adopt.push(() => {
    st.ctxCache = null;
    if (st.lateHooked) return;
    st.lateHooked = true;
    hooks.key.push((e) => lateKeyHook(st, e));
  });
}
