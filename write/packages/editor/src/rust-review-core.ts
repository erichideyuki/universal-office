// Frente R da leva 5 do engine="rust" (registro core/docs/modules/uo-bindings.md "Wave 5 — R"): o acesso comum dos
// módulos rust-review-*.ts ao motor (a sessão, o layout, o aplicador de respostas) e a geometria das linhas que as
// sobreposições de tela leem (as linhas do escopo de edição por página e por parágrafo, em cache por layout).
// Sem sessão (o motor falso de engine-switch.test.ts, ou antes do 1º documento), cada handler responde o neutro.
import type { DecodedLayout } from '@universal-office/write-engine';
import { bindStories, type ExecOutcome, type SessionCommand, type ShellLine, type StoriesBinding, type StoryCtx, type StoryLine, type StoryLines, type StoryPara, type WriteSession } from '@universal-office/write-session';
import type { FrontApi, RustEditor, RustHostElement } from './rust-engine.js';

export type Pos = { para: number; offset: number };

export const PX_PER_TWIP = 96 / 1440;

/** O acesso do módulo ao motor (null: motor falso sem `frontApi`, ex. o teste de rotas). */
export function apiOf(ed: RustEditor): FrontApi | null {
  const f = (ed as unknown as { frontApi?: () => FrontApi }).frontApi;
  return typeof f === 'function' ? f.call(ed) : null;
}

/** O contexto de cada handler: a API do motor, a sessão de agora (ou null) e o elemento. */
export class Ctx {
  constructor(readonly ed: RustEditor, readonly api: FrontApi | null) {}
  get s(): WriteSession | null { return this.api?.session ?? null; }
  get dl(): DecodedLayout | null { return this.api?.dl ?? null; }
  get el(): RustHostElement | null { return this.api?.el ?? null; }

  /** Uma consulta da sessão: `v` quando a resposta tem um valor, senão o objeto sem `t` (ou null sem sessão). */
  q<T = unknown>(op: Record<string, unknown> & { t: string }): T | null {
    const s = this.s;
    if (!s) return null;
    try {
      const a = s.query(op as { t: string }) as Record<string, unknown>;
      if ('v' in a) return a['v'] as T;
      const { t: _t, ...rest } = a;
      return rest as T;
    } catch (e) {
      this.api?.warnOnce(`query:${op.t}`, `[universal-write] engine="rust": query ${op.t} failed`, e);
      return null;
    }
  }

  /** Executa um comando e aplica a resposta; devolve a resposta (para o valor de retorno do TS). */
  run(cmd: SessionCommand, what: string): ExecOutcome | null {
    const s = this.s;
    if (!s || !this.api) return null;
    const o = s.command(cmd);
    this.api.apply(o, what);
    return o;
  }

  /** Executa um comando SEM aplicar (consultas de duas fases: `pasteSpecialFormats`). */
  raw(cmd: SessionCommand): ExecOutcome | null {
    return this.s?.command(cmd) ?? null;
  }

  locked(): boolean { return !this.api || this.api.locked(); }
}

/** A resposta mudou o documento (histórias, formatação) ou emitiu eventos? */
export function changed(o: ExecOutcome | null): boolean {
  return !!o && o.t === 'done' && (o.stories.length > 0 || !!o.formatting || o.events.length > 0);
}

/** A resposta mudou o DOCUMENTO (não só a seleção)? */
export function edited(o: ExecOutcome | null): boolean {
  return !!o && o.t === 'done' && (o.stories.length > 0 || !!o.formatting);
}

// ───────────────────────── geometria (linhas do escopo de edição) ─────────────────────────

const pageCache = new WeakMap<DecodedLayout, Map<number, ShellLine[]>>();
const paraCache = new WeakMap<DecodedLayout, Map<number, ShellLine[]>>();

/** As linhas do escopo de edição na página `page` do layout de agora (em cache por layout). */
export function pageLines(c: Ctx, page: number): ShellLine[] {
  const s = c.s, dl = c.dl;
  if (!s || !dl) return [];
  let m = pageCache.get(dl);
  if (!m) { m = new Map(); pageCache.set(dl, m); }
  let v = m.get(page);
  if (!v) {
    try { v = s.pageLines(dl.inner, page); } catch { v = []; }
    m.set(page, v);
  }
  return v;
}

/** As linhas do parágrafo `para` do escopo de edição (em cache por layout). */
export function paraLines(c: Ctx, para: number): ShellLine[] {
  const s = c.s, dl = c.dl;
  if (!s || !dl) return [];
  let m = paraCache.get(dl);
  if (!m) { m = new Map(); paraCache.set(dl, m); }
  let v = m.get(para);
  if (!v) {
    try { v = s.paraLines(dl.inner, para); } catch { v = []; }
    m.set(para, v);
  }
  return v;
}

// ───────────────────────── wave 7c (V, ED-304): as linhas de TODAS as histórias (o `eng.lines` do TS) ─────────────────────────

/** Uma linha de `storyLines` com o seu parágrafo (chave, texto, runs com os offsets `a`/`z`, marcas de indicador). */
export interface StoryLineP extends ShellLine {
  kind: StoryLine['kind'];
  key: string;
  text: string;
  runs: (StoryPara['runs'][number] & { a: number; z: number })[];
  bm: StoryPara['bm'];
  /** fix round 2 (V, ED-304): the line's natural text descent (twips; the engine's `Line::desc`) */
  desc: number;
}
const storyCache = new WeakMap<DecodedLayout, Map<string, StoryLineP[]>>();

/**
 * As linhas de toda história na página `page` (corpo, células, cabeçalhos/rodapés e notas — o `eng.lines` do TS,
 * installEngineResult 16515 — e, com `kind` 'tb', o conteúdo das caixas de texto no referencial delas), qualquer que seja
 * o escopo de edição; em cache por layout. ED-304 fechado: o sombreamento de campo, o realce de comentário, os colchetes
 * dos indicadores, os sublinhados da revisão e as ocorrências da busca seguem toda história, como no TS.
 */
export function storyLines(c: Ctx, page: number, fallback = 'pt-BR'): StoryLineP[] {
  const s = c.s, dl = c.dl;
  if (!s || !dl || typeof s.storyLines !== 'function') return [];
  let m = storyCache.get(dl);
  if (!m) { m = new Map(); storyCache.set(dl, m); }
  const k = `${page}|${fallback}`;
  let v = m.get(k);
  if (!v) {
    let r: StoryLines;
    try { r = s.storyLines(dl.inner, page, fallback); } catch { r = { lines: [], paras: [] }; }
    const paras = r.paras.map((p) => {
      let acc = 0;
      return { ...p, runs: p.runs.map((x) => { const a = acc; acc += x.n; return { ...x, a, z: acc }; }) };
    });
    v = r.lines.map((l) => {
      const p = paras[l.p]!;
      return { para: -1, page, x: l.x, y: l.y, height: l.height, baseline: l.baseline, width: l.width, start: l.start, end: l.end, ...(l.xf ? { xf: l.xf } : {}), o: l.o, cx: l.cx, cw: l.cw, kind: l.kind, key: p.key, text: p.text, runs: p.runs, bm: p.bm, desc: l.desc };
    });
    m.set(k, v);
  }
  return v;
}

/** A chave de uma localização de busca da sessão (`SearchLoc`), no formato das chaves de `storyLines`. */
export function searchLocKey(loc: { t: string; para?: number; anchor?: number; hops?: { row: number; col: number; block?: number }[] }): string {
  if (loc.t === 'cell') return `c${loc.anchor ?? 0}/${(loc.hops ?? []).map((h) => `${h.row}.${h.col}.${h.block ?? ''}`).join('/')}|${loc.para ?? 0}`;
  return `b${loc.para ?? 0}`;
}

/** A extensão em x dos caracteres de offset em [a, b) na linha (no resultado de um campo os offsets recomeçam: o
 *  mínimo e o máximo, como os laços do TS); null sem caractere. */
export function charExtent(l: ShellLine, a: number, b: number): [number, number] | null {
  let x1 = Infinity, x2 = -Infinity;
  for (let i = 0; i < l.o.length; i++) {
    const o = l.o[i]!;
    if (o < a || o >= b) continue;
    x1 = Math.min(x1, l.cx[i]!);
    x2 = Math.max(x2, l.cx[i]! + l.cw[i]!);
  }
  return x2 > x1 ? [x1, x2] : null;
}

/** Desenha numa linha (texto girado de uma caixa: a matriz dela). */
export function onLine(ctx: CanvasRenderingContext2D, l: ShellLine, draw: () => void): void {
  if (l.xf) { ctx.save(); ctx.transform(l.xf[0]!, l.xf[1]!, l.xf[2]!, l.xf[3]!, l.xf[4]!, l.xf[5]!); draw(); ctx.restore(); }
  else draw();
}

/** O topo (px no espaço de rolagem do elemento) e a altura de uma linha: canvas.offsetTop + y·escala. */
export function linePx(el: RustHostElement, l: ShellLine): { topPx: number; heightPx: number; page: number } | null {
  const canvas = el.canvases[l.page];
  if (!canvas) return null;
  const scale = PX_PER_TWIP * el.zoom;
  return { topPx: canvas.offsetTop + l.y * scale, heightPx: l.height * scale, page: l.page };
}

/** Rola até o começo de um parágrafo, a um terço da altura visível (o `scrollToSelection` do TS). */
export function scrollToPos(c: Ctx, at: Pos, fraction = 1 / 3): void {
  const el = c.el;
  if (!el) return;
  const lines = paraLines(c, at.para);
  const l = lines.find((x) => at.offset >= x.start && at.offset <= x.end) ?? lines[0];
  const px = l ? linePx(el, l) : null;
  if (!px) return;
  el.animateScrollTo(Math.max(0, px.topPx - (el.clientHeight || 900) * fraction));
}

/** A cada novo documento o cache de linhas segue o layout (chaves fracas); nada a limpar. */
export const SEL_FILL = 'rgba(58,114,196,0.35)';

// ───────────────────────── o contexto de edição (a leitura da frente S) ─────────────────────────

const storiesCache = new WeakMap<object, StoriesBinding | null>();

/** O contexto de edição da sessão (`cellEdit`, `noteEdit`, `hfEdit`, `tbEdit` do TS) pelo binding da frente S; null sem sessão. */
export function storyCtx(c: Ctx): StoryCtx | null {
  const api = c.api, s = c.s;
  if (!api || !s) return null;
  const mod = api.module as unknown as { exports: unknown; binding: unknown } | undefined;
  if (!mod) return null;
  let sb = storiesCache.get(mod);
  if (sb === undefined) {
    try { sb = bindStories(mod.exports as never, mod.binding as never); } catch { sb = null; }
    storiesCache.set(mod, sb);
  }
  if (!sb) return null;
  try { return sb.storyContext(s).ctx; } catch { return null; }
}

/** O contexto de fora das caixas de texto (o TS mantém `hfEdit` com uma caixa do cabeçalho aberta). */
export function outerCtx(x: StoryCtx | null): StoryCtx | null {
  return x && x.kind === 'textbox' ? outerCtx(x.outer) : x;
}

/** O `toBody()` do TS: sai da nota, da caixa de texto, do cabeçalho/rodapé e da célula (nesta ordem, uma vez cada). */
export function toBody(c: Ctx, what: string): void {
  const exit = (kind: StoryCtx['kind'], t: string): void => {
    if (storyCtx(c)?.kind === kind) c.run({ t: 'context', v: { t } } as SessionCommand, what);
  };
  exit('note', 'exitNote');
  exit('textbox', 'exitTextbox');
  exit('hf', 'exitHeaderFooter');
  exit('cell', 'exitCell');
}
