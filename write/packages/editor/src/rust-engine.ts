import { convertDocToDocx, isWordBinary } from '@universal-office/docx-core';
import { FontResolver, type FontVariant } from '@universal-office/fonts';
import { uiLang } from '@universal-office/ui';
import { loadViewerModule, type DecodedLayout, type LoadWriteOptions, type ViewerEnv, type ViewerModule } from '@universal-office/write-engine';
import { bindSession, encodePaste, type Caret, type ExecOutcome, type RelayoutReport, type SessionBinding, type SessionCommand, type SessionEvent, type ShellEngine, type WriteSession } from '@universal-office/write-session';
import { RustSession, type EngineDocument } from '../../viewer/src/rust-engine.js';
import { NEUTRAL_QUERIES, ROUTED_PRIVATE, ROUTED_PUBLIC, SHELL, isMutator, neutralOf } from './engine-routes.js';
import { PUBLIC_METHODS } from './engine-public.js';
import { objectsHandlers } from './rust-engine-objects.js';
import { reviewHandlers } from './rust-engine-review.js';
import { storiesHandlers } from './rust-engine-stories.js';
import { fragmentOf } from './paste-html.js';

/**
 * `engine="rust"` do <universal-write>: o editor como CASCA TS sobre a sessão wasm (registro
 * core/docs/modules/uo-bindings.md "Milestone 2: the editor switch"). Carregado SÓ por import() dinâmico quando o
 * atributo pede o motor Rust — sem ele o editor nunca baixa este módulo nem o .wasm.
 *
 * O modelo, a edição, o layout (incremental, LA7) e a pintura das páginas vêm da sessão (`WriteSession` +
 * `ShellEngine` de core/crates/uo-wasm/js/session.mjs); fica no TS só o que o registro lista em "Stays in the TS
 * shell": entrada (teclado, IME, ponteiro, a textarea oculta), os eventos DOM re-despachados dos eventos da sessão,
 * as barras, a pintura em canvas das páginas decodificadas (pelo `RustSession` do viewer: o mesmo laço de fontes, o
 * mesmo ambiente da página, o mesmo pintor), rolagem e zoom.
 *
 * As rotas: `installRoutes` sombreia na INSTÂNCIA os métodos de `engine-routes.ts` (ROUTED → este módulo; SHELL → o
 * TS de sempre; o resto fechado por padrão). O TS do elemento não muda: sem `engine="rust"`, nada daqui roda.
 */

/** O .wasm do editor (a sessão: o build `--features session`), ao lado deste módulo (`chunks/uo-write-editor.wasm`). */
const WASM_FILE = 'uo-write-editor.wasm';
const LANGS = ['pt-BR', 'en', 'es'];
const PX_PER_TWIP = 96 / 1440;
const HF_DIM_ALPHA = 0.5;
const SEL_FILL = 'rgba(58,114,196,0.35)';
const VARIANTS: readonly FontVariant[] = ['regular', 'bold', 'italic', 'boldItalic'];

type Pos = { para: number; offset: number };
type Sel = { anchor: Pos; focus: Pos };

/** O que o motor lê e escreve do elemento (membros privados do TS, alcançados em tempo de execução). */
export interface RustHostElement extends HTMLElement {
  canvases: HTMLCanvasElement[];
  container: HTMLDivElement;
  input: HTMLTextAreaElement;
  zoom: number;
  view: string;
  caretVisible: boolean;
  allocatedPages: Set<number>;
  selection: Sel;
  pageDark: boolean;
  printRender: boolean;
  showMarks: boolean;
  protectedMode: boolean;
  dirty: boolean;
  fontBytes: Record<string, Partial<Record<FontVariant, Uint8Array>>>;
  chromeSync: (() => void) | null;
  onSelectionChange?: (() => void) | null;
  readonly author: string;
  ensureVisiblePainted(): void;
  pagesViewportWidth(): number;
  animateScrollTo(target: number): void;
  spellDefaultLang(): string | undefined;
  cancelQueuedRefresh(): void;
}

/**
 * Contadores da página (o modo memory do harness lê por `UniversalWriteElement.engineStats()`). `editors` = os
 * elementos engine="rust" CONECTADOS (desconectar estaciona e desconta; reconectar volta a contar).
 */
const live = { sessions: 0, shellEngines: 0, editors: 0 };
let lastModule: ViewerModule | null = null;

const samePos = (a: Pos, b: Pos): boolean => a.para === b.para && a.offset === b.offset;
const cmpPos = (a: Pos, b: Pos): number => a.para - b.para || a.offset - b.offset;

/** Uma marca de tempo do teclado (o modo latency do harness lê `engineStats().marks`). */
interface KeyMark { exec: number; relayout: number; adopt: number; paint: number; path: string; reason: string | null }

export class RustEditor {
  readonly el: RustHostElement;
  private readonly mod: ViewerModule;
  private readonly ss: SessionBinding;
  private rs: RustSession;
  private readonly source: () => LoadWriteOptions | undefined;
  private session: WriteSession | null = null;
  private shell: ShellEngine | null = null;
  private dl: DecodedLayout | null = null;
  /** o relógio dos campos de data: fixo por época (aberto, ou o último refresh(); D-LA7-24) */
  private clock = new Date();
  /** os bytes abertos (PDF sem edições, como o TS) */
  private loaded: Uint8Array | null = null;
  /** o documento guardado ao desconectar (reabre ao reconectar) */
  private parked: { bytes: Uint8Array; protected: boolean } | null = null;
  private epoch = 0;
  private fontLoop: Promise<void> = Promise.resolve();
  private base = new Map<number, { key: string; canvas: HTMLCanvasElement }>();
  private hfItems: (number | undefined)[] = [];
  private caretCache: { dl: DecodedLayout; sel: string; caret: Caret | null } | null = null;
  private stateCache: { gen: number; v: unknown } | null = null;
  private gen = 0;
  /** a geração do TEXTO do documento (stats(): palavras e caracteres em cache) */
  private textGen = 0;
  private textStats: { gen: number; words: number; chars: number } | null = null;
  private canUndoV = false;
  private canRedoV = false;
  private ulButton = 'single';
  private warned = new Set<string>();
  private pushedFonts = new WeakSet<ViewerEnv>();
  private composing = false;
  private drag: { anchor: Pos; move: (e: MouseEvent) => void; up: () => void } | null = null;
  /** o elemento está conectado (desconectado: estacionado, fora de `live.editors`) */
  private attached = true;
  /** o próximo evento paste é o de Ctrl+Shift+V (o `plainPasteNext` do TS) */
  private plainNext = false;
  readonly marks: KeyMark[] = [];
  private markOn = false;

  constructor(el: RustHostElement, mod: ViewerModule, ss: SessionBinding, source: () => LoadWriteOptions | undefined) {
    this.el = el;
    this.mod = mod;
    this.ss = ss;
    this.source = source;
    this.rs = this.newRustSession();
    live.editors++;
    this.listen();
  }

  /** O lado do viewer (laço de fontes, ambiente da página, pintura); um descartado não volta, então cada vida tem o seu. */
  private newRustSession(): RustSession {
    return new RustSession({
      fontsBase: () => this.el.getAttribute('fonts'),
      resolver: FontResolver.shared(),
      repaint: () => { this.base.clear(); this.paint(); },
      source: this.source,
    });
  }

  /** Um método público ainda sem rota: no-op, avisado uma vez. */
  warnDisabled(name: string): void {
    this.warnOnce(`disabled:${name}`, `[universal-write] engine="rust": ${name}() is not routed in this wave (disabled)`);
  }

  // ───────────────────────── documento ─────────────────────────

  /** O documento que o `RustSession` diagrama: a sessão (o layout pelo motor incremental). */
  private adapter(session: WriteSession, shell: ShellEngine): EngineDocument {
    return {
      layout: (env) => {
        this.pushElementFonts(env);
        return this.mod.adopt(shell.relayout(session, env.inner, { hyphenationLanguages: LANGS, now: this.clock }));
      },
      hyphenationPatterns: (env, langs) => session.hyphenationPatterns(env.inner, langs),
      free: () => {
        session.free();
        shell.free();
        live.sessions--;
        live.shellEngines--;
        if (this.session === session) { this.session = null; this.shell = null; }
      },
    };
  }

  /** Sem base de fontes (`fonts`), as fontes que o elemento instalou (initialize({ fonts })) vão ao ambiente. */
  private pushElementFonts(env: ViewerEnv): void {
    if (this.pushedFonts.has(env) || this.el.getAttribute('fonts') !== null) return;
    this.pushedFonts.add(env);
    for (const [family, faces] of Object.entries(this.el.fontBytes)) {
      for (const v of VARIANTS) {
        const b = faces[v];
        if (b) try { env.pushAsset(family, v, b); } catch { /* face recusada: a substituta */ }
      }
    }
  }

  private adopt(session: WriteSession, bytes: Uint8Array | null, prot: boolean): void {
    const shell = this.ss.newShellEngine();
    live.sessions++;
    live.shellEngines++;
    this.epoch++;
    this.rs.adoptDocument(this.mod, this.adapter(session, shell));
    this.session = session;
    this.shell = shell;
    this.dl = null;
    this.base.clear();
    this.hfItems = [];
    this.caretCache = null;
    this.loaded = bytes;
    this.gen++;
    this.textGen++;
    this.canUndoV = false;
    this.canRedoV = false;
    session.setUiLang(uiLang(this.el));
    const el = this.el;
    el.protectedMode = prot;
    el.selection = { anchor: { para: 0, offset: 0 }, focus: { para: 0, offset: 0 } };
    if (el.dirty) { el.dirty = false; el.dispatchEvent(new CustomEvent('uw-dirty-change', { detail: { dirty: false } })); }
    // 1ª passada já, se o ambiente da página existe (outro documento já preparou as fontes); o laço de fontes depois
    if (this.rs.env && !this.rs.env.freed) {
      // (sem as fontes ainda empurradas, a passada falha: o laço de fontes faz a 1ª)
      try {
        const dl = this.rs.layoutSync();
        if (dl) this.install(dl, null);
      } catch { /* o laço de fontes */ }
    }
    this.runFontLoop();
    el.dispatchEvent(new CustomEvent('uw-protected', { detail: { active: prot, hasFields: false } }));
  }

  /** O laço de fontes do viewer (passadas até as fontes pedidas chegarem). */
  private runFontLoop(): void {
    const epoch = this.epoch;
    const stale = (): boolean => epoch !== this.epoch || !this.attached;
    const run = this.fontLoop.then(() => this.rs.relayout(stale, { view: 'print', webWidthTw: 0 }, (dl) => this.install(dl, this.shell?.report() ?? null)));
    this.fontLoop = run.catch((e: unknown) => { console.error('[universal-write] engine="rust": layout failed', e); });
  }

  /** Espera o laço de fontes e o layout ficarem quietos (o harness usa). */
  async idle(): Promise<void> {
    let seen: Promise<void> | null = null;
    while (seen !== this.fontLoop) { seen = this.fontLoop; await seen; }
  }

  loadDocx(bytes: Uint8Array, opts: { protected?: boolean } = {}): void {
    const prot = opts.protected ?? true;
    if (isWordBinary(bytes)) bytes = convertDocToDocx(bytes);
    const session = this.ss.open(bytes, { protected: prot, author: this.el.author, clock: this.clock = new Date(), seed: 1 });
    this.adopt(session, bytes, prot);
  }

  load(bytes: Uint8Array, format = 'auto', opts: { protected?: boolean } = {}): void {
    if (format === 'odt') { this.loadOdt(); return; }
    this.loadDocx(bytes, { protected: opts.protected ?? false });
  }

  loadOdt(): void {
    this.warnOnce('odt', 'engine="rust": .odt is not routed in this wave');
  }

  newDocument(): void {
    const session = this.ss.newBlank({ protected: false, author: this.el.author, lang: this.el.spellDefaultLang() ?? 'pt-BR', clock: this.clock = new Date(), seed: 1 });
    this.adopt(session, null, false);
  }

  /** Desconectado: guarda o documento (bytes) e solta a sessão, o motor e os layouts. */
  park(): void {
    if (this.attached) { this.attached = false; live.editors--; }
    if (!this.session) return;
    try { this.parked = { bytes: this.session.save(), protected: this.el.protectedMode }; } catch { this.parked = null; }
    this.epoch++;
    this.release();
  }

  /** Reconectado: reabre o documento guardado (o histórico de desfazer não volta). */
  unpark(): void {
    if (!this.attached) { this.attached = true; live.editors++; }
    const p = this.parked;
    this.parked = null;
    if (p && !this.session) this.loadDocx(p.bytes, { protected: p.protected });
  }

  private release(): void {
    if (this.dl) { this.rs.release(this.dl); this.dl = null; }
    this.rs.dispose();
    this.rs = this.newRustSession();
    // `dispose` libera o documento (a sessão e o motor, pelo adaptador); um RustSession descartado não aceita outro
    this.session = null;
    this.shell = null;
    this.base.clear();
  }

  // ───────────────────────── layout e pintura ─────────────────────────

  /** O layout depois de uma edição: síncrono, pelo motor incremental; as páginas refeitas perdem a pintura em cache. */
  private relayoutNow(): void {
    const { session, shell } = this;
    const env = this.rs.env;
    if (!session || !shell || !env || env.freed) { this.runFontLoop(); return; }
    const t0 = performance.now();
    let inner;
    try {
      inner = shell.relayout(session, env.inner, { hyphenationLanguages: LANGS, now: this.clock });
    } catch (e) {
      // um trap mata a instância (o próximo uso carrega outra); sem fontes ainda, o laço de fontes refaz
      if (e instanceof WebAssembly.RuntimeError) { this.mod.kill(); throw e; }
      this.runFontLoop();
      return;
    }
    const t1 = performance.now();
    const dl = this.mod.adopt(inner);
    const report = shell.report();
    const t2 = performance.now();
    this.install(dl, report);
    if (this.markOn) {
      const m = this.marks[this.marks.length - 1];
      if (m) { m.relayout = t1 - t0; m.adopt = t2 - t1; m.paint = performance.now() - t2; m.path = report?.path ?? 'full'; m.reason = report?.reason ?? null; }
    }
    if (env.takeMissing().length) this.runFontLoop();
  }

  private install(dl: DecodedLayout, report: RelayoutReport | null): void {
    const old = this.dl;
    this.dl = dl;
    this.hfItems = [];
    this.caretCache = null;
    if (!report || report.path === 'full' || !old || old.pages.length !== dl.pages.length) this.base.clear();
    else for (let i = report.relaid[0]; i < report.relaid[1]; i++) this.base.delete(i);
    if (old) this.rs.release(old);
    this.rebuildCanvases();
    // the visible pages: allocated and painted (from the page cache, but for the pages laid out again)
    this.el.ensureVisiblePainted();
    this.positionProxy();
  }

  rebuildCanvases(): void {
    const el = this.el;
    const dl = this.dl;
    if (!dl) return;
    const scale = PX_PER_TWIP * el.zoom;
    const css = (i: number): { w: string; h: string } => {
      const p = dl.pages[i]!;
      return { w: `${Math.round(p.widthTw * scale)}px`, h: `${Math.round(p.heightTw * scale)}px` };
    };
    const n = dl.pages.length;
    const same = el.canvases.length === n && el.canvases.every((c, i) => { const s = css(i); return c.style.width === s.w && c.style.height === s.h; });
    if (same) {
      // as the TS `rebuildCanvases`: the same canvases (the dblclick target survives), the buffers released; the visible pages
      // are allocated again by ensureVisiblePainted and drawn from the page cache (the virtualization stays the TS one)
      for (const i of el.allocatedPages) { const c = el.canvases[i]; if (c) { c.width = 1; c.height = 1; } }
      el.allocatedPages.clear();
      return;
    }
    el.container.replaceChildren();
    el.canvases = [];
    el.allocatedPages.clear();
    this.base.clear();
    for (let i = 0; i < n; i++) {
      const s = css(i);
      const c = document.createElement('canvas');
      c.className = 'page';
      c.width = 1;
      c.height = 1;
      c.style.width = s.w;
      c.style.height = s.h;
      el.container.append(c);
      el.canvases.push(c);
    }
  }

  private hfOf(i: number): number {
    let h = this.hfItems[i];
    if (h === undefined && this.dl) { h = this.dl.inner.pageInfo(i).hfItems ?? 0; this.hfItems[i] = h; }
    return h ?? 0;
  }

  /** A página `i` (fundo + itens, o cabeçalho/rodapé esmaecido) num canvas em cache, como o `engineBase` do TS. */
  private baseOf(i: number, w: number, h: number, scale: number): HTMLCanvasElement {
    const el = this.el;
    const dl = this.dl!;
    const key = `${w}x${h}|${scale.toFixed(6)}|${el.printRender ? 1 : 0}|${el.pageDark ? 1 : 0}`;
    const hit = this.base.get(i);
    if (hit && hit.key === key) return hit.canvas;
    const off = hit?.canvas ?? document.createElement('canvas');
    off.width = w;
    off.height = h;
    const ctx = off.getContext('2d')!;
    ctx.imageSmoothingEnabled = true;
    ctx.imageSmoothingQuality = 'high';
    ctx.setTransform(scale, 0, 0, scale, 0, 0);
    const env = this.rs.paintEnv(dl);
    const page = dl.pages[i]!;
    if (el.printRender) { ctx.fillStyle = '#ffffff'; ctx.fillRect(0, 0, page.widthTw, page.heightTw); }
    else dl.paintBackground(ctx, i, env, { devicePxTw: 1 / (PX_PER_TWIP * el.zoom), screen: true, paper: el.pageDark ? '#1e1e1e' : '#ffffff' });
    const n = page.items.length;
    const hf = Math.min(this.hfOf(i), n);
    const dim = el.printRender ? 1 : HF_DIM_ALPHA;
    if (hf > 0) dl.paintItems(ctx, i, env, { from: 0, to: hf, alpha: dim });
    if (n > hf) dl.paintItems(ctx, i, env, { from: hf, to: n, alpha: 1 });
    this.base.set(i, { key, canvas: off });
    return off;
  }

  /** O cursor da seleção no layout de agora (cache por layout e seleção). */
  private caret(): Caret | null {
    const { dl, session } = this;
    if (!dl || !session) return null;
    const sel = JSON.stringify(this.el.selection);
    if (this.caretCache && this.caretCache.dl === dl && this.caretCache.sel === sel) return this.caretCache.caret;
    const caret = session.caretRect(dl.inner);
    this.caretCache = { dl, sel, caret };
    return caret;
  }

  paint(filter?: Set<number>): void {
    const el = this.el;
    const dl = this.dl;
    if (!dl || !this.session || el.canvases.length === 0) return;
    const active = filter ?? el.allocatedPages;
    const dpr = window.devicePixelRatio || 1;
    const { anchor, focus } = el.selection;
    const hasSel = !samePos(anchor, focus);
    const caret = !hasSel && el.caretVisible && !el.printRender && el.view !== 'read' ? this.caret() : null;
    for (const i of active) {
      const c = el.canvases[i];
      if (!c || c.width <= 1 || !dl.pages[i]) continue;
      const bs = c.width / (parseFloat(c.style.width) || c.width);
      const scale = PX_PER_TWIP * el.zoom * bs;
      const ctx = c.getContext('2d')!;
      ctx.setTransform(1, 0, 0, 1, 0, 0);
      ctx.clearRect(0, 0, c.width, c.height);
      ctx.drawImage(this.baseOf(i, c.width, c.height, scale), 0, 0);
      if (hasSel && !el.printRender) {
        ctx.save();
        ctx.setTransform(scale, 0, 0, scale, 0, 0);
        ctx.fillStyle = SEL_FILL;
        for (const r of this.session.selectionRects(dl.inner, i)) {
          if (r.xf) { ctx.save(); ctx.transform(r.xf[0]!, r.xf[1]!, r.xf[2]!, r.xf[3]!, r.xf[4]!, r.xf[5]!); }
          ctx.fillRect(r.x, r.y, Math.max(0, r.w), r.h);
          if (r.xf) ctx.restore();
        }
        ctx.restore();
      }
      if (caret && caret.page === i) {
        ctx.fillStyle = el.pageDark ? '#e8eaed' : '#000';
        const xf = caret.xf;
        if (xf) {
          const ap = (x: number, y: number): [number, number] => [xf[0]! * x + xf[2]! * y + xf[4]!, xf[1]! * x + xf[3]! * y + xf[5]!];
          const [ax, ay] = ap(caret.x, caret.y), [bx, by] = ap(caret.x, caret.y + caret.h);
          ctx.save();
          ctx.strokeStyle = ctx.fillStyle;
          ctx.lineWidth = Math.max(1, dpr);
          ctx.beginPath(); ctx.moveTo(ax * scale, ay * scale); ctx.lineTo(bx * scale, by * scale); ctx.stroke();
          ctx.restore();
        } else ctx.fillRect(caret.x * scale, caret.y * scale, Math.max(1, dpr), caret.h * scale);
      }
    }
  }

  /** A textarea junto ao cursor (o navegador rola o elemento focado; o IME aparece no lugar). */
  positionProxy(): void {
    const el = this.el;
    const c = this.caret();
    const canvas = c ? el.canvases[c.page] : undefined;
    if (!c || !canvas) { el.input.style.top = '0px'; el.input.style.left = '0px'; return; }
    const scale = PX_PER_TWIP * el.zoom;
    el.input.style.left = `${Math.round(canvas.offsetLeft + c.x * scale)}px`;
    el.input.style.top = `${Math.round(canvas.offsetTop + c.y * scale)}px`;
    el.input.style.height = `${Math.max(8, Math.round(c.h * scale))}px`;
  }

  scrollCaretIntoView(): void {
    const el = this.el;
    const c = this.caret();
    const canvas = c ? el.canvases[c.page] : undefined;
    if (!c || !canvas) return;
    const scale = PX_PER_TWIP * el.zoom;
    const top = canvas.offsetTop + c.y * scale;
    const bottom = top + c.h * scale;
    const viewH = el.clientHeight || 900;
    const MARGIN = 8;
    if (top >= el.scrollTop + MARGIN && bottom <= el.scrollTop + viewH - MARGIN) return;
    el.animateScrollTo(Math.max(0, top < el.scrollTop + MARGIN ? top - MARGIN - 32 : bottom - viewH + MARGIN + 32));
  }

  refresh(): void {
    this.el.cancelQueuedRefresh();
    if (!this.session) return;
    // nova época do relógio (D-LA7-24): os campos de data andam; o layout seguinte é completo
    this.clock = new Date();
    this.relayoutNow();
    this.el.onSelectionChange?.(); this.el.chromeSync?.();
  }

  refreshView(): void {
    this.rebuildCanvases();
    this.base.clear();
    for (const i of this.el.allocatedPages) { const c = this.el.canvases[i]; if (c) { c.width = 1; c.height = 1; } }
    this.el.allocatedPages.clear();
    this.el.ensureVisiblePainted();
    this.positionProxy();
    this.el.onSelectionChange?.(); this.el.chromeSync?.();
  }

  refreshSelection(): void {
    this.paint();
    this.positionProxy();
    this.el.onSelectionChange?.(); this.el.chromeSync?.();
  }

  repaint(): void {
    this.base.clear();
    this.paint();
  }

  layoutPageCount(): number { return this.dl?.pages.length ?? 0; }

  /** A página visível (centro da vista), 1-based: o `currentPage` do TS sobre as páginas do layout da sessão. */
  currentPage(): number {
    const el = this.el;
    const center = el.scrollTop + (el.clientHeight || 900) / 2;
    let best = 0;
    let bestDist = Infinity;
    el.canvases.forEach((c, i) => {
      const d = Math.abs(c.offsetTop + c.offsetHeight / 2 - center);
      if (d < bestDist) { bestDist = d; best = i; }
    });
    return Math.max(1, Math.min(best + 1, this.layoutPageCount()));
  }

  /**
   * O zoom de "Largura da página" (o `pageWidthZoom` do TS): a largura da página do cursor (a da seção do foco no
   * modo de impressão, o único desta onda) em vez de `sect().pageWidthTwips`.
   */
  pageWidthZoom(): number {
    const el = this.el;
    const dl = this.dl;
    if (!dl || dl.pages.length === 0) return el.zoom;
    const page = dl.pages[this.caret()?.page ?? 0] ?? dl.pages[0]!;
    const cs = getComputedStyle(el.container);
    const pad = (parseFloat(cs.paddingLeft) || 0) + (parseFloat(cs.paddingRight) || 0);
    const avail = el.pagesViewportWidth() - pad - 2;
    return Math.min(5, Math.max(0.1, Math.floor((avail / (page.widthTw * PX_PER_TWIP)) * 100) / 100));
  }

  /**
   * A barra de status (o `stats` do TS): palavras e caracteres pelo texto dos parágrafos do corpo da sessão
   * (`paraText`; ED-20: a marca de referência de nota e o texto de um inline de plugin contam como o texto do run),
   * em cache até a próxima mudança do documento.
   */
  stats(): { pages: number; words: number; chars: number; paragraph: number; page: number } {
    if (!this.textStats || this.textStats.gen !== this.textGen) {
      let words = 0, chars = 0;
      const s = this.session;
      const n = s?.storyInfo().paras ?? 0;
      for (let i = 0; i < n; i++) {
        const t = s!.paraText(i) ?? '';
        chars += t.length;
        words += (t.match(/\S+/g) ?? []).length;
      }
      this.textStats = { gen: this.textGen, words, chars };
    }
    return {
      pages: this.layoutPageCount(),
      page: this.currentPage(),
      words: this.textStats.words,
      chars: this.textStats.chars,
      paragraph: this.el.selection.focus.para + 1,
    };
  }

  paintPageThumbnail(page: number, canvas: HTMLCanvasElement, cssWidth: number): boolean {
    const dl = this.dl;
    const p = dl?.pages[page];
    if (!dl || !p) return false;
    const dpr = window.devicePixelRatio || 1;
    const scale = cssWidth / p.widthTw;
    canvas.width = Math.max(1, Math.round(p.widthTw * scale * dpr));
    canvas.height = Math.max(1, Math.round(p.heightTw * scale * dpr));
    canvas.style.width = `${Math.round(p.widthTw * scale)}px`;
    canvas.style.height = `${Math.round(p.heightTw * scale)}px`;
    const ctx = canvas.getContext('2d')!;
    ctx.setTransform(scale * dpr, 0, 0, scale * dpr, 0, 0);
    const env = this.rs.paintEnv(dl);
    dl.paintBackground(ctx, page, env, { devicePxTw: 1 / scale, screen: true, paper: this.el.pageDark ? '#1e1e1e' : '#ffffff' });
    dl.paintItems(ctx, page, env, {});
    return true;
  }

  /** A fonte resolvida mudou (o resolvedor único): o laço de fontes de novo. */
  onFontsResolved(): void {
    if (this.session) this.runFontLoop();
  }

  queueRefresh(): void {
    if (this.session) this.runFontLoop();
  }

  // ───────────────────────── edição ─────────────────────────

  private warnOnce(key: string, msg: string, detail?: unknown): void {
    if (this.warned.has(key)) return;
    this.warned.add(key);
    console.warn(msg, detail ?? '');
  }

  /** Aplica a resposta da sessão: seleção, histórico, layout (só se algo mudou), eventos, barras. */
  private apply(o: ExecOutcome | null, what = 'command'): boolean {
    if (!o) return false;
    if (o.t === 'error') { this.warnOnce(`error:${what}`, `[universal-write] engine="rust": ${what} refused`, o.v); return false; }
    if (o.t === 'jobs') {
      // um comando de duas fases que esta onda não atende (decodificar imagem…): cancelado
      this.session?.cancelPending();
      this.warnOnce(`jobs:${what}`, `[universal-write] engine="rust": ${what} needs jobs, not routed`);
      return false;
    }
    const el = this.el;
    const cs = o;
    el.selection = { anchor: { ...cs.selection.anchor }, focus: { ...cs.selection.focus } };
    el.dirty = cs.dirty ?? false;
    this.canUndoV = cs.canUndo ?? false;
    this.canRedoV = cs.canRedo ?? false;
    this.gen++;
    el.caretVisible = true;
    const changed = cs.stories.length > 0 || !!cs.formatting;
    if (changed && this.markOn) this.marks.push({ exec: 0, relayout: 0, adopt: 0, paint: 0, path: '', reason: null });
    if (changed) { this.textGen++; this.relayoutNow(); }
    else { this.caretCache = null; this.paint(); this.positionProxy(); }
    for (const ev of cs.events) this.dispatch(ev);
    this.scrollCaretIntoView();
    el.onSelectionChange?.(); el.chromeSync?.();
    return true;
  }

  private dispatch(ev: SessionEvent): void {
    // `uw-pasted` travels without detail (F8-D3): the shell answers it with `pasteOptions()`, as the TS `pasteOptions(d)`
    if (ev.t === 'uw-pasted') { this.el.dispatchEvent(new CustomEvent('uw-pasted', { detail: this.pasteOptions() })); return; }
    const { t, ...rest } = ev as SessionEvent & Record<string, unknown>;
    this.el.dispatchEvent(Object.keys(rest).length ? new CustomEvent(t, { detail: rest }) : new CustomEvent(t));
  }

  private command(cmd: SessionCommand, what: string): boolean {
    if (!this.session) return false;
    return this.apply(this.session.command(cmd), what);
  }

  private locked(): boolean { return this.el.protectedMode || !this.session; }

  type(text: string): void {
    if (this.locked()) return;
    const before = new CustomEvent('uw-before-input', { cancelable: true, detail: { text } });
    if (!this.el.dispatchEvent(before)) return;
    const t0 = performance.now();
    const o = this.session!.type(text);
    const exec = performance.now() - t0;
    this.apply(o, 'type');
    if (this.markOn) { const m = this.marks[this.marks.length - 1]; if (m) m.exec = exec; }
  }

  pressEnter(): void { if (!this.locked()) this.apply(this.session!.enter(), 'enter'); }

  /**
   * Tab / Shift+Tab (the TS `bindEvents`): at the start of a list item (or with several paragraphs selected) the list
   * level changes; otherwise Tab types a tab and Shift+Tab does nothing. The session's `changeListLevel` answers no
   * change when no selected paragraph is a list item (the TS `false`): then the tab. ED-26: the TS asks whether the
   * FOCUS paragraph is a list item, the session whether any selected one is (they differ only for a multi-paragraph
   * selection whose focus paragraph is not in a list).
   */
  private tab(shift: boolean): void {
    const s = this.session;
    if (!s || this.locked()) return;
    const { anchor, focus } = this.el.selection;
    const start = cmpPos(anchor, focus) <= 0 ? anchor : focus;
    if (anchor.para !== focus.para || start.offset === 0) {
      const o = s.command({ t: 'list', v: { t: 'changeListLevel', delta: shift ? -1 : 1 } });
      if (o?.t === 'done' && (o.stories.length > 0 || o.events.length > 0 || o.formatting)) { this.apply(o, 'changeListLevel'); return; }
    }
    if (!shift) this.type('\t');
  }

  moveCaret(dir: 'left' | 'right' | 'up' | 'down' | 'home' | 'end' | 'wordLeft' | 'wordRight', extend: boolean): void {
    if (!this.session) return;
    this.apply(this.session.moveCaret(dir, extend, this.dl?.inner ?? null), 'moveCaret');
  }

  moveWord(dir: -1 | 1, extend: boolean): void { this.moveCaret(dir < 0 ? 'wordLeft' : 'wordRight', extend); }

  /** Ctrl+Backspace/Delete (o `deleteWord` do TS composto: sem seleção, seleciona até a palavra e apaga; ED-deleteWord). */
  deleteWord(dir: -1 | 1): void {
    const s = this.session;
    if (!s || this.locked()) return;
    const { anchor, focus } = this.el.selection;
    if (samePos(anchor, focus)) {
      const r = s.moveCaret(dir < 0 ? 'wordLeft' : 'wordRight', false, null);
      if (r?.t !== 'done') return;
      const to = r.selection.focus;
      // as the TS: no word to delete (the story's start or end) = nothing at all (no snapshot, no event)
      if (samePos(to, focus)) return;
      s.setSelection(focus, to);
    }
    this.apply(dir < 0 ? s.backspace() : s.delete(), 'deleteWord');
  }

  undo(): void { if (this.session) this.apply(this.session.undo(), 'undo'); }
  redo(): void { if (this.session) this.apply(this.session.redo(), 'redo'); }
  popHistory(): void { /* o histórico é da sessão */ }
  canUndo(): boolean { return this.canUndoV; }
  canRedo(): boolean { return this.canRedoV; }

  isDirty(): boolean { return this.el.dirty; }
  setDirty(dirty: boolean): void { if (this.session) this.apply(this.session.setDirty(dirty), 'setDirty'); }
  isProtected(): boolean { return this.el.protectedMode; }

  enableEditing(): void {
    if (!this.el.protectedMode || !this.session) return;
    const o = this.session.command({ t: 'settings', v: { t: 'setProtected', on: false } });
    this.el.protectedMode = false;
    const had = o?.t === 'done' && o.events.some((e) => e.t === 'uw-protected');
    this.apply(o, 'enableEditing');
    if (!had) this.el.dispatchEvent(new CustomEvent('uw-protected', { detail: { active: false, hasFields: false } }));
    this.el.input.focus({ preventScroll: true });
  }

  isEmpty(): boolean {
    const i = this.session?.storyInfo();
    return !i || (i.paras <= 1 && i.lastLength === 0);
  }

  setSelection(anchor: Pos, focus: Pos = anchor): void {
    if (!this.session) return;
    this.apply(this.session.setSelection(anchor, focus), 'setSelection');
  }

  selectAll(): void { this.command({ t: 'navigation', v: { t: 'selectAll' } }, 'selectAll'); }

  selectedText(): string {
    if (!this.session || samePos(this.el.selection.anchor, this.el.selection.focus)) return '';
    const a = this.session.query({ t: 'selectedText' });
    return typeof a.v === 'string' ? a.v : '';
  }

  /** Retângulo do cursor (coordenadas da tela): só o do foco da seleção (outra posição: null, ED-caretRectAt). */
  caretRectAt(pos: Pos): { x: number; y: number; h: number } | null {
    if (!samePos(pos, this.el.selection.focus)) return null;
    const c = this.caret();
    const canvas = c ? this.el.canvases[c.page] : undefined;
    if (!c || !canvas) return null;
    const scale = PX_PER_TWIP * this.el.zoom;
    const rc = canvas.getBoundingClientRect();
    return { x: rc.left + c.x * scale, y: rc.top + c.y * scale, h: c.h * scale };
  }

  getSelectionState(): unknown {
    if (this.stateCache && this.stateCache.gen === this.gen) return this.stateCache.v;
    const s = this.session?.selectionState();
    // `src` (the lossless source id) travels as an option of an option (`{ value }`): the TS form is the number
    const flat = (o: object): Record<string, unknown> => {
      const r: Record<string, unknown> = { ...o };
      const src = r['src'];
      if (src && typeof src === 'object' && 'value' in src) r['src'] = (src as { value: unknown }).value;
      return r;
    };
    const v = { run: flat(s?.run ?? {}), para: flat(s?.para ?? {}), view: this.el.view, marks: this.el.showMarks, table: null };
    this.stateCache = { gen: this.gen, v };
    return v;
  }

  // ── formatação (comandos da sessão; o estado de casca que o TS guarda é replicado: o estilo do botão Sublinhado) ──
  toggleRunProp(key: string): void {
    if (key === 'underline') { this.toggleUnderline(); return; }
    this.command({ t: 'format', v: { t: 'toggleRunProp', key, button: this.ulButton } }, 'toggleRunProp');
  }
  toggleUnderline(style: string = this.ulButton): void { this.command({ t: 'format', v: { t: 'toggleUnderline', style, button: this.ulButton } }, 'toggleUnderline'); }
  setUnderline(style: string): void {
    if (style !== 'none') this.ulButton = style;
    this.command({ t: 'format', v: { t: 'setUnderline', style } }, 'setUnderline');
  }
  underlineButtonStyle(): string { return this.ulButton; }
  setFontFamily(name: string): void { this.command({ t: 'format', v: { t: 'setFontFamily', name } }, 'setFontFamily'); }
  setFontSize(pt: number): void { this.command({ t: 'format', v: { t: 'setFontSize', pt } }, 'setFontSize'); }
  growFont(delta: number): void { this.command({ t: 'format', v: { t: 'growFont', delta } }, 'growFont'); }
  setColor(color: string | { themeColor: string; tint?: number; shade?: number } | undefined): void {
    const c = color === undefined ? undefined : typeof color === 'string' ? { t: 'hex', v: color.replace(/^#/, '').toUpperCase() } : { t: 'theme', ...color };
    this.command({ t: 'format', v: { t: 'setColor', ...(c ? { color: c } : {}) } }, 'setColor');
  }
  setHighlight(name: string | undefined): void { this.command({ t: 'format', v: { t: 'setHighlight', ...(name !== undefined ? { name } : {}) } }, 'setHighlight'); }
  clearFormatting(): void { this.command({ t: 'format', v: { t: 'clearFormatting' } }, 'clearFormatting'); }
  clearCharacterFormatting(): void { this.command({ t: 'style', v: { t: 'clearCharacterFormatting' } }, 'clearCharacterFormatting'); }
  changeCase(mode: string): void { this.command({ t: 'format', v: { t: 'changeCase', mode } }, 'changeCase'); }
  toggleVertAlign(kind: string): void { this.command({ t: 'format', v: { t: 'toggleVertAlign', kind } }, 'toggleVertAlign'); }
  setCharSpacing(twips: number | undefined): void { this.command({ t: 'format', v: { t: 'setCharSpacing', ...(twips !== undefined ? { twips } : {}) } }, 'setCharSpacing'); }
  setAlignment(alignment: string): void { this.command({ t: 'paragraph', v: { t: 'setAlignment', alignment } }, 'setAlignment'); }
  indent(delta: number): void { this.command({ t: 'paragraph', v: { t: 'indent', delta } }, 'indent'); }
  setLineSpacing(line: number): void { this.command({ t: 'paragraph', v: { t: 'setLineSpacing', line } }, 'setLineSpacing'); }
  setSpaceBefore(twips: number | undefined): void { this.command({ t: 'paragraph', v: { t: 'setSpaceBefore', ...(twips !== undefined ? { twips } : {}) } }, 'setSpaceBefore'); }
  setSpaceAfter(twips: number | undefined): void { this.command({ t: 'paragraph', v: { t: 'setSpaceAfter', ...(twips !== undefined ? { twips } : {}) } }, 'setSpaceAfter'); }
  setShading(hex: string | undefined): void { this.command({ t: 'paragraph', v: { t: 'setShading', ...(hex !== undefined ? { hex } : {}) } }, 'setShading'); }
  clearParagraphFormatting(): void { this.command({ t: 'paragraph', v: { t: 'clearParagraphFormatting' } }, 'clearParagraphFormatting'); }
  toggleList(kind: string): void { this.command({ t: 'list', v: { t: 'toggleList', bullet: kind === 'bullet' } }, 'toggleList'); }
  removeList(): void { this.command({ t: 'list', v: { t: 'removeList' } }, 'removeList'); }
  changeListLevel(delta: number): boolean { return this.command({ t: 'list', v: { t: 'changeListLevel', delta } }, 'changeListLevel'); }
  applyStyle(styleId: string | undefined): void { this.command({ t: 'style', v: { t: 'applyStyle', ...(styleId !== undefined ? { styleId } : {}) } }, 'applyStyle'); }

  /**
   * `currentStyles()` (o TS: o estilo do parágrafo do foco, senão o padrão do documento; o de caractere de
   * `pendingProps ?? currentProps()`) pelo estado da barra da sessão. ED-23: sem estilo no parágrafo, `Normal` (a
   * sessão não expõe o `w:default` de parágrafo do documento).
   */
  currentStyles(): { paragraph: string; character?: string } {
    const s = this.session?.selectionState();
    return { paragraph: s?.para.styleId ?? 'Normal', character: s?.run.charStyle };
  }

  // ── fontes no arquivo (as opções do DOCUMENTO: lidas da sessão, que as leu do pacote ao abrir) ──
  getFontEmbedding(): { embed: boolean; subset: boolean; skipCommonFonts: boolean } {
    const a = this.session?.query({ t: 'fontEmbedding' }) as { embed?: boolean; subset?: boolean; skipCommonFonts?: boolean } | undefined;
    return { embed: !!a?.embed, subset: !!a?.subset, skipCommonFonts: !!a?.skipCommonFonts };
  }

  /** O TS: nada quando nada muda; senão as opções novas e o documento alterado (a sessão: `set fontEmbedding`). */
  setFontEmbedding(patch: { embed?: boolean; subset?: boolean; skipCommonFonts?: boolean }): void {
    const s = this.session;
    if (!s) return;
    const cur = this.getFontEmbedding();
    const keys = ['embed', 'subset', 'skipCommonFonts'] as const;
    if (keys.every((k) => patch[k] === undefined || patch[k] === cur[k])) return;
    const p: { embed?: boolean; subset?: boolean; skipCommonFonts?: boolean } = {};
    for (const k of keys) if (patch[k] !== undefined) p[k] = !!patch[k];
    this.apply(s.setFontEmbedding(p), 'setFontEmbedding');
  }

  // ── área de transferência (recortar/copiar: texto e HTML do recorte; colar pelo teclado: `pasteData`, ED-16) ──
  private clip(e: ClipboardEvent, cut: boolean): void {
    if (!this.session) return;
    e.preventDefault();
    if (samePos(this.el.selection.anchor, this.el.selection.focus)) return;
    const o = this.session.command({ t: 'clipboard', v: { t: cut ? 'cut' : 'copy' } });
    const c = this.session.copied();
    if (c && e.clipboardData) {
      e.clipboardData.setData('text/plain', c.text);
      if (c.html) e.clipboardData.setData('text/html', c.html);
    }
    if (cut) this.apply(o, 'cut');
  }
  // the TS methods are `async` (the bars chain `.then` on them)
  async copy(): Promise<void> { this.warnOnce('copy()', '[universal-write] engine="rust": copy() without a clipboard event is not routed (Ctrl+C works)'); }
  async cut(): Promise<void> { this.warnOnce('cut()', '[universal-write] engine="rust": cut() without a clipboard event is not routed (Ctrl+X works)'); }
  async paste(): Promise<void> { this.warnOnce('paste()', '[universal-write] engine="rust": paste() without a clipboard event is not routed (Ctrl+V works)'); }
  async pastePlain(): Promise<void> { await this.paste(); }
  pasteText(text: string): void { if (!this.locked()) this.command({ t: 'clipboard', v: { t: 'pasteText', text } }, 'pasteText'); }

  /**
   * O evento paste (Ctrl+V, Ctrl+Shift+V): o ouvinte `paste` do `bindEvents` do TS — `text/plain`, `text/html` (a
   * árvore do DOMParser codificada por `encodePaste`, a fronteira D2b-F8), `withRich` (a cópia interna, se o texto do
   * sistema ainda é o dela) e o modo `text` depois de Ctrl+Shift+V — pela `clipboard.pasteData` da sessão (o port de
   * `pasteData`: o anel, `uw-pasted`). A largura do conteúdo vai ausente: a da seção do corpo (F8-D3; ED-16). Uma imagem
   * sem texto nem HTML (a decodificação de duas fases) não é roteada nesta onda.
   */
  private onPaste(e: ClipboardEvent): void {
    e.preventDefault();
    const cd = e.clipboardData;
    const plain = this.plainNext;
    this.plainNext = false;
    const text = cd?.getData('text/plain') ?? '';
    const html = cd?.getData('text/html') ?? '';
    const file = cd ? [...cd.files].find((x) => /^image\/(png|jpeg|gif|bmp|webp)$/.test(x.type)) : undefined;
    if (file && !text && !html) {
      this.warnOnce('paste-image', '[universal-write] engine="rust": pasting an image is not routed in this wave (two-phase job)');
      return;
    }
    let tree: Uint8Array | undefined;
    if (html) {
      try { tree = encodePaste(html, fragmentOf).bytes; } catch (err) { this.warnOnce('paste-tree', '[universal-write] engine="rust": the HTML paste tree failed (pasting the text)', err); }
    }
    this.command({
      t: 'clipboard',
      v: { t: 'pasteData', text, ...(tree ? { html: tree } : {}), rich: 'withRich', ...(plain ? { mode: 'text' } : {}), layout: {} },
    }, 'paste');
  }

  /**
   * As Opções de Colagem da última colagem (`pasteOptions()` da sessão, na forma do TS: `nomerge`, `current` e
   * `currentList` só enquanto a última colagem pode ser refeita).
   */
  pasteOptions(): { modes: string[]; list: string[]; current?: string; currentList?: string | null } {
    const a = this.session?.query({ t: 'pasteOptions' }) as { modes?: string[]; list?: string[]; current?: { f0: string; f1?: string } } | undefined;
    const lst = (l: string): string => (l === 'noMerge' ? 'nomerge' : l);
    const o = { modes: a?.modes ?? [], list: (a?.list ?? []).map(lst) };
    return a?.current ? { ...o, current: a.current.f0, currentList: a.current.f1 !== undefined ? lst(a.current.f1) : null } : o;
  }

  /** Refaz a última colagem com outra opção (o botão e o menu Ctrl das Opções de Colagem: K, M, T, U; listas). */
  repaste(mode: string, list?: string): void {
    if (this.locked()) return;
    const l = list === undefined ? undefined : list === 'nomerge' ? 'noMerge' : list;
    this.command({ t: 'clipboard', v: { t: 'repaste', mode, ...(l !== undefined ? { list: l } : {}), layout: {} } }, 'repaste');
  }

  /** Onde fica o botão das Opções de Colagem: abaixo do fim da última colagem (só quando o cursor está nele, ED-5). */
  pasteButtonRect(): { x: number; y: number } | null {
    const r = this.session?.query({ t: 'lastPasteRange' }) as { value?: { end: Pos } } | undefined;
    const c = r?.value ? this.caretRectAt(r.value.end) : null;
    return c ? { x: c.x + 4, y: c.y + c.h + 2 } : null;
  }

  // os formatos que esta onda não exporta: um erro claro (o TS de um mutador desligado devolveria undefined: o
  // Salvar Como .odt da barra gravaria nada; ED-18)
  exportOdt(): Uint8Array { throw new Error('engine="rust": exporting .odt is not routed in this wave'); }
  exportXps(): Uint8Array { throw new Error('engine="rust": exporting .xps is not routed in this wave'); }
  async exportHtml(): Promise<string> { throw new Error('engine="rust": exporting HTML is not routed in this wave'); }
  async exportMarkdown(): Promise<string> { throw new Error('engine="rust": exporting Markdown is not routed in this wave'); }

  // ── salvar ──
  saveDocx(): Uint8Array {
    if (!this.session) throw new Error('document not loaded');
    return this.session.save();
  }

  async saveDocxWithFonts(): Promise<Uint8Array> {
    const s = this.session;
    if (!s) throw new Error('document not loaded');
    const req = s.fontRequests();
    const resolver = FontResolver.shared();
    const faces = await Promise.all(req.requests.map(async (r) => {
      try {
        const [res] = await resolver.resolveWithFile([r.family]);
        if (res && !res.faces[r.variant as FontVariant]) await resolver.loadFace(res, r.variant as FontVariant);
        return res?.faces[r.variant as FontVariant] ?? undefined;
      } catch { return undefined; }
    }));
    const out = s.saveWithFonts(faces);
    if (!out) throw new Error('save failed');
    for (const ev of out.events) this.dispatch(ev);
    return out.bytes;
  }

  async exportPdf(): Promise<Uint8Array> {
    if (!this.session) throw new Error('document not loaded');
    await this.idle();
    // sem edições: os bytes ORIGINAIS (a regra do TS, D12)
    const edits = this.session.query({ t: 'editsSinceLoad' }).v;
    const env = this.rs.env;
    if (edits === 0 && this.loaded && env && !env.freed) {
      const doc = this.mod.open(this.loaded);
      try {
        const dl = doc.layout(env, { hyphenationLanguages: LANGS, view: 'print' });
        try { return dl.pdf(); } finally { dl.free(); }
      } finally { doc.free(); }
    }
    if (!this.dl) throw new Error('no layout');
    return this.dl.pdf();
  }

  async save(format = 'auto'): Promise<Uint8Array | string> {
    if (format === 'pdf') return this.exportPdf();
    if (format === 'auto' || format === 'docx') return this.saveDocxWithFonts();
    throw new Error(`engine="rust": saving as ${format} is not routed in this wave`);
  }

  focusEditor(): void { this.el.input.focus({ preventScroll: true }); }
  hitTest(): null { return null; }
  hitTestRaw(): null { return null; }
  updateTouchHandles(): void { /* toque: não roteado nesta onda */ }
  layout(): void { /* o layout é da sessão (relayoutNow) */ }

  // ───────────────────────── entrada ─────────────────────────

  /** A posição sob o ponteiro (a página pela regra do `hitTestRaw` do TS). */
  private hitAt(e: MouseEvent): Pos | null {
    const el = this.el;
    const dl = this.dl;
    if (!dl || !this.session) return null;
    const order = [...el.canvases.keys()].sort((a, b) => {
      const ra = el.canvases[a]!.getBoundingClientRect(), rb = el.canvases[b]!.getBoundingClientRect();
      return Math.max(0, ra.left - e.clientX, e.clientX - ra.right) - Math.max(0, rb.left - e.clientX, e.clientX - rb.right);
    });
    for (const pg of order) {
      const rect = el.canvases[pg]!.getBoundingClientRect();
      if (e.clientY < rect.top || e.clientY > rect.bottom) continue;
      const scale = PX_PER_TWIP * el.zoom;
      return this.session.hitTest(dl.inner, pg, (e.clientX - rect.left) / scale, (e.clientY - rect.top) / scale);
    }
    return null;
  }

  /**
   * Um evento de ponteiro nas páginas: o caminho passa pelo contêiner das páginas, ou pela textarea oculta (posicionada no
   * cursor, 1 px de largura: o 2º e o 3º clique de um duplo/triplo clique no cursor caem nela, como no TS).
   */
  private inPages(e: Event): boolean { const p = e.composedPath(); return p.includes(this.el.container) || p.includes(this.el.input); }

  private listen(): void {
    const el = this.el;
    const input = el.input;
    const own = (e: Event): void => { e.stopImmediatePropagation(); };
    input.addEventListener('compositionstart', (e) => { if (!this.session) return; own(e); this.composing = true; }, true);
    input.addEventListener('compositionend', (e) => {
      if (!this.session) return;
      own(e);
      this.composing = false;
      input.value = '';
      if (e.data) this.type(e.data);
    }, true);
    input.addEventListener('input', (e) => {
      if (!this.session) return;
      own(e);
      if (this.composing) return;
      const v = input.value;
      if (v) { input.value = ''; this.type(v); }
    }, true);
    input.addEventListener('keydown', (e) => { if (this.session) this.onKey(e); }, true);
    input.addEventListener('copy', (e) => { if (this.session) { own(e); this.clip(e, false); } }, true);
    input.addEventListener('cut', (e) => { if (this.session) { own(e); this.clip(e, true); } }, true);
    input.addEventListener('paste', (e) => { if (this.session) { own(e); this.onPaste(e); } }, true);
    el.addEventListener('mousedown', (e) => { if (this.session && this.inPages(e)) { own(e); this.onMouseDown(e); } }, true);
    el.addEventListener('dblclick', (e) => { if (this.session && this.inPages(e)) { own(e); this.onDblClick(e); } }, true);
    el.addEventListener('contextmenu', (e) => { if (this.session && this.inPages(e)) { own(e); e.preventDefault(); } }, true);
  }

  private onMouseDown(e: MouseEvent): void {
    const hit = this.hitAt(e);
    if (!hit) return;
    const el = this.el;
    if (e.button !== 0) {
      const { anchor, focus } = el.selection;
      const [a, b] = cmpPos(anchor, focus) <= 0 ? [anchor, focus] : [focus, anchor];
      if (cmpPos(hit, a) < 0 || cmpPos(hit, b) > 0) this.setSelection(hit);
      this.focusEditor();
      return;
    }
    this.focusEditor();
    e.preventDefault();
    const anchor = e.shiftKey ? el.selection.anchor : hit;
    this.setSelection(anchor, hit);
    if (e.detail === 3) {
      const text = this.session?.paraText(hit.para) ?? '';
      this.setSelection({ para: hit.para, offset: 0 }, { para: hit.para, offset: text.length });
      return;
    }
    if (this.drag) { window.removeEventListener('mousemove', this.drag.move); window.removeEventListener('mouseup', this.drag.up); }
    const move = (ev: MouseEvent): void => {
      const h = this.hitAt(ev);
      if (h && !samePos(h, el.selection.focus)) this.setSelection(anchor, h);
    };
    const up = (): void => {
      window.removeEventListener('mousemove', move);
      window.removeEventListener('mouseup', up);
      this.drag = null;
    };
    this.drag = { anchor, move, up };
    window.addEventListener('mousemove', move);
    window.addEventListener('mouseup', up);
  }

  private onDblClick(e: MouseEvent): void {
    const hit = this.hitAt(e);
    if (!hit || !this.session) return;
    const text = this.session.paraText(hit.para) ?? '';
    let a = hit.offset, b = hit.offset;
    while (a > 0 && !/[\s]/.test(text[a - 1]!)) a--;
    while (b < text.length && !/[\s]/.test(text[b]!)) b++;
    this.setSelection({ para: hit.para, offset: a }, { para: hit.para, offset: b });
  }

  /**
   * A tecla que a casca não trata: os ouvintes TS da textarea (bindEvents) não a veem, mas os do elemento sim (os
   * atalhos das barras de chrome.ts, `bindDocumentFeatures`, e os do hospedeiro): uma cópia sintética despachada no
   * elemento (a textarea fica dentro da shadow root; a propagação do original para nela). Se alguém a tratou
   * (`preventDefault`), o original também. `owned` = uma tecla que o `bindEvents` do TS sempre previne (`tsOwned`):
   * o padrão do navegador fica de fora também aqui. O resto segue o padrão, como no TS — AltGr (Ctrl+Alt) e Option
   * digitam pelo evento input, F5/F11 agem. Declarado (ED-21 do registro).
   */
  private forward(e: KeyboardEvent, owned: boolean): void {
    e.stopImmediatePropagation();
    const copy = new KeyboardEvent('keydown', {
      key: e.key, code: e.code, location: e.location, repeat: e.repeat,
      ctrlKey: e.ctrlKey, metaKey: e.metaKey, shiftKey: e.shiftKey, altKey: e.altKey,
      bubbles: true, cancelable: true, composed: true,
    });
    this.el.dispatchEvent(copy);
    if (owned || copy.defaultPrevented) e.preventDefault();
  }

  /**
   * As teclas que o ouvinte `keydown` do `bindEvents` do TS previne sempre (fora dos contextos que a casca não abre,
   * ED-7: Esc de recorte, tabela, nota…): a navegação (setas, Home/End, PageUp/Down, com qualquer modificador),
   * Enter, Backspace, Delete, Tab, F9, Ctrl+Alt+F/D, Ctrl+Alt+V, Ctrl+hífen e os mapas de Ctrl (`map[k]`) e
   * Ctrl+Shift (`sh[e.code]`). A casca roda as que roteia; as outras (funções desligadas) não deixam o padrão do
   * navegador agir, como no TS (Alt+Seta não volta a página, Shift+Tab não tira o foco).
   */
  private tsOwned(e: KeyboardEvent): boolean {
    const k = e.key;
    const lk = k.toLowerCase();
    const ctrl = e.ctrlKey || e.metaKey;
    if (/^(Arrow(Left|Right|Up|Down)|Home|End|PageUp|PageDown|Enter|Backspace|Delete|Tab)$/.test(k)) return true;
    if (k === 'F9' && !ctrl && !e.altKey && !e.shiftKey) return true;
    if (k === 'F3' && e.shiftKey && !ctrl && !e.altKey) return true;
    if (!ctrl) return false;
    if (e.altKey && !e.shiftKey && (lk === 'f' || lk === 'd')) return true;
    if (e.altKey && lk === 'v' && e.code === 'KeyV') return true;
    if (!e.altKey && (e.code === 'Minus' || e.code === 'NumpadSubtract')) return true;
    if (e.shiftKey && !e.altKey && /^Key[DWHKA]$/.test(e.code)) return true;
    return /^[biuzyaelrj]$/.test(lk);
  }

  private onKey(e: KeyboardEvent): void {
    const ctrl = e.ctrlKey || e.metaKey;
    const k = e.key;
    const done = (): void => { e.preventDefault(); e.stopImmediatePropagation(); };
    if (k === 'Control' || k === 'Shift' || k === 'Alt' || k === 'Meta' || this.composing || e.isComposing) { e.stopImmediatePropagation(); return; }
    const arrows: Record<string, 'left' | 'right' | 'up' | 'down'> = { ArrowLeft: 'left', ArrowRight: 'right', ArrowUp: 'up', ArrowDown: 'down' };
    if (arrows[k] && !e.altKey) {
      done();
      if (ctrl && (k === 'ArrowLeft' || k === 'ArrowRight')) this.moveCaret(k === 'ArrowLeft' ? 'wordLeft' : 'wordRight', e.shiftKey);
      else if (!ctrl) this.moveCaret(arrows[k]!, e.shiftKey);
      return;
    }
    if ((k === 'Home' || k === 'End') && !e.altKey) {
      done();
      if (ctrl) {
        const s = this.session!;
        const info = s.storyInfo();
        const to = k === 'Home' ? { para: 0, offset: 0 } : { para: Math.max(0, info.paras - 1), offset: info.lastLength };
        this.setSelection(e.shiftKey ? this.el.selection.anchor : to, to);
      } else this.moveCaret(k === 'Home' ? 'home' : 'end', e.shiftKey);
      return;
    }
    if ((k === 'PageUp' || k === 'PageDown') && !ctrl && !e.altKey) {
      done();
      // as the TS (bindEvents): 0.9 of the view, then ~one screen of line moves
      this.el.scrollTop += this.el.clientHeight * 0.9 * (k === 'PageUp' ? -1 : 1);
      for (let i = 0; i < 20; i++) this.moveCaret(k === 'PageUp' ? 'up' : 'down', e.shiftKey);
      return;
    }
    if (k === 'Enter' && !ctrl && !e.altKey) { done(); this.pressEnter(); return; }
    if (k === 'Backspace' && !e.altKey) { done(); if (ctrl) this.deleteWord(-1); else if (!this.locked()) this.apply(this.session!.backspace(), 'backspace'); return; }
    if (k === 'Delete' && !e.altKey) { done(); if (ctrl) this.deleteWord(1); else if (!this.locked()) this.apply(this.session!.delete(), 'delete'); return; }
    if (k === 'Tab' && !ctrl && !e.altKey) { done(); this.tab(e.shiftKey); return; }
    if (k === 'F3' && e.shiftKey && !ctrl && !e.altKey) { done(); if (!this.locked()) this.changeCase('next'); return; }
    if (ctrl && !e.altKey) {
      // copiar, recortar, colar: o navegador dispara os eventos (tratados acima); Ctrl+Shift+V = o próximo paste é só
      // texto (o `plainPasteNext = e.shiftKey` do TS a cada Ctrl+V)
      if (e.code === 'KeyV') this.plainNext = e.shiftKey;
      if ((!e.shiftKey && (e.code === 'KeyC' || e.code === 'KeyX' || e.code === 'KeyV')) || (e.shiftKey && e.code === 'KeyV')) { e.stopImmediatePropagation(); return; }
      const shiftMap: Record<string, () => void> = {
        KeyD: () => this.toggleUnderline('double'), KeyW: () => this.toggleUnderline('words'),
        KeyH: () => this.toggleRunProp('hidden'), KeyK: () => this.toggleRunProp('smallCaps'), KeyA: () => this.toggleRunProp('allCaps'),
        KeyZ: () => this.redo(),
      };
      const map: Record<string, () => void> = {
        KeyB: () => this.toggleRunProp('bold'), KeyI: () => this.toggleRunProp('italic'), KeyU: () => this.toggleUnderline(),
        KeyE: () => this.setAlignment('center'), KeyL: () => this.setAlignment('left'), KeyR: () => this.setAlignment('right'), KeyJ: () => this.setAlignment('both'),
        KeyZ: () => this.undo(), KeyY: () => this.redo(), KeyA: () => this.selectAll(),
      };
      const fn = (e.shiftKey ? shiftMap : map)[e.code];
      // the rest (Ctrl+S, Ctrl+O, Ctrl+], Ctrl+K…): to the bars' shortcuts (a command not routed is disabled there, D7)
      if (!fn) { this.forward(e, this.tsOwned(e)); return; }
      done();
      if (this.locked() && e.code !== 'KeyA') return;
      fn();
      return;
    }
    // the rest (Alt and AltGr combinations, F-keys, Esc, printable keys): the element sees a copy; the browser's default
    // runs unless the TS owns the key or a listener prevented the copy (AltGr/Option characters type through `input`)
    this.forward(e, this.tsOwned(e));
  }

  // ───────────────────────── medidas ─────────────────────────

  /** Liga as marcas de tempo do teclado (modo latency do harness). */
  setMarks(on: boolean): void { this.markOn = on; this.marks.length = 0; }

  engineCounts(): { sessions: number; shellEngines: number; editors: number; layout: boolean } {
    return { ...live, layout: this.dl !== null };
  }
}

/** As estatísticas do motor da página (módulo, handles, sessões). */
export function engineStats(): { buffer: number; live: number | null; handles: number; pageEnvs: number; sessions: number; shellEngines: number; editors: number } | null {
  const m = lastModule?.stats();
  return m ? { ...m, ...live } : null;
}

/**
 * Sombreia na instância os métodos de `engine-routes.ts`: ROUTED → `editor`; os públicos não classificados ficam
 * fechados (mutador: no-op; consulta: o TS com o modelo TS vazio, exceção = `neutralOf`; as de NEUTRAL_QUERIES
 * respondem o neutro direto). Devolve as contagens.
 */
export function installRoutes(el: HTMLElement, editor: RustEditor, proto: object): { routed: number; shell: number; disabledMutators: number; disabledQueries: number } {
  const rec = editor as unknown as Record<string, unknown>;
  const define = (name: string, value: unknown): void => { Object.defineProperty(el, name, { value, configurable: true, writable: true }); };
  // os handlers das frentes da leva 5 (rust-engine-<frente>.ts) vêm antes dos métodos do RustEditor
  const fronts: Record<string, unknown> = { ...storiesHandlers(editor), ...objectsHandlers(editor), ...reviewHandlers(editor) };
  for (const name of [...ROUTED_PUBLIC, ...ROUTED_PRIVATE]) {
    const g = fronts[name];
    const f = rec[name];
    if (typeof g === 'function') define(name, g);
    else if (typeof f === 'function') define(name, (f as (...a: unknown[]) => unknown).bind(editor));
    // roteado sem handler: fechado (no-op avisado), nunca o código TS
    else define(name, () => { editor.warnDisabled(name); return undefined; });
  }
  const routed = new Set([...ROUTED_PUBLIC, ...ROUTED_PRIVATE]);
  const shell = new Set(SHELL);
  const neutral = new Set(NEUTRAL_QUERIES);
  let disabledMutators = 0, disabledQueries = 0;
  const present = new Set(publicMethods(proto));
  for (const name of PUBLIC_METHODS) {
    if (!present.has(name)) continue;
    if (routed.has(name) || shell.has(name)) continue;
    if (isMutator(name)) {
      disabledMutators++;
      // an `async` method of the TS answers a promise (the bars chain `.then`/`.catch` on it)
      const ts = (proto as Record<string, unknown>)[name] as { constructor: { name: string } };
      const isAsync = ts.constructor.name === 'AsyncFunction';
      define(name, () => { editor.warnDisabled(name); return isAsync ? Promise.resolve(undefined) : undefined; });
    } else if (neutral.has(name)) {
      disabledQueries++;
      define(name, () => neutralOf(name));
    } else {
      disabledQueries++;
      const ts = (proto as Record<string, unknown>)[name] as (...a: unknown[]) => unknown;
      define(name, (...args: unknown[]) => { try { return ts.apply(el, args); } catch { return neutralOf(name); } });
    }
  }
  return { routed: routed.size, shell: shell.size, disabledMutators, disabledQueries };
}

/** Os métodos do protótipo (públicos e privados: o TS marca `private` só no tipo; os públicos são os de PUBLIC_METHODS). */
export function publicMethods(proto: object): string[] {
  const out: string[] = [];
  for (const name of Object.getOwnPropertyNames(proto)) {
    const d = Object.getOwnPropertyDescriptor(proto, name);
    if (!d || typeof d.value !== 'function' || name === 'constructor') continue;
    out.push(name);
  }
  return out;
}

/** Carrega o módulo (um por página), liga a sessão, instala as rotas e os ouvintes. */
export async function startRustEditor(el: HTMLElement, proto: object, source: () => LoadWriteOptions | undefined): Promise<RustEditor> {
  const src = source();
  const mod = await loadViewerModule(!src || (src.wasm === undefined && src.js === undefined) ? { ...src, wasm: new URL(WASM_FILE, import.meta.url) } : src);
  if (typeof (mod.exports as Record<string, unknown>)['uo_session_open'] !== 'function') {
    throw new Error('engine="rust": the engine module of this page has no session exports (load the editor build, uo-write-editor.wasm, before any viewer)');
  }
  lastModule = mod;
  const ss = bindSession(mod.exports, mod.binding);
  const editor = new RustEditor(el as RustHostElement, mod, ss, source);
  installRoutes(el, editor, proto);
  return editor;
}
