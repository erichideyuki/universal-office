import { FontResolver, variantOf } from '@universal-office/fonts';
import {
  PptxPackage, readPresentation, readSlide, paintSlideTo,
  readEmbeddedFonts, collectDeckTypefaces, readSlideNotes,
  buildPlaybackTimeline, playbackStateAt, shapeAnimHooks, paintTransition, SlideShow, rasterizeSpecialImage,
  specialImageNeedsPrepare, prepareSpecialImage, tableImages,
  type ChartModel, type PaintImage, type PaintSlideOptions,
  type Presentation, type Slide, type Shape, type ImageRef,
  type PlaybackOptions, type PlaybackTimeline, type PlaybackSlide, type SlideShowAction, type SlideTimingModel, type SlideMediaInfo,
  type SlideTransitionInfo, type EmbeddedFont,
} from '@universal-office/slides-core';
import { MediaPlayer, type MediaFrame } from './media-player.js';
import { ViewerControls, CONTROL_ITEMS, type ControlsHost } from './controls.js';
import { SlideA11y, a11yShapesOf, type A11yShape } from './a11y.js';
import { onBrowserLangChange, pickLabels, uiLang } from '@universal-office/ui';
import type { RustSlides, RustSlidesHost } from './rust-engine.js';
import type {
  LoadSlidesOptions, SlidesShowHandle, SlidesTimelineHandle, SlidesDocument, ShowFrame, ShowAction, LinkBoxWire, SlideAssets, PaintImage as EnginePaintImage,
} from '@universal-office/slides-engine';

/** B4b: de onde o palco do motor Rust tira o quadro: a apresentação espelhada (`at` = agora) ou a linha do tempo (`at` = t). */
interface RustStage { h: SlidesShowHandle | SlidesTimelineHandle; at: number }

/**
 * Esquemas de hiperlink que o viewer abre (a regra do viewer do Write): `http`, `https` e `mailto`. Vale com `engine="rust"`;
 * `link-schemes="any"` volta à regra do motor TS (qualquer endereço do arquivo, `window.open` sem `noreferrer`).
 */
const SAFE_LINK = /^(https?:|mailto:)/i;

/** Textos da interface desenhados no canvas (o fim da apresentação é o do PowerPoint em cada idioma). */
const TEXT = {
  en: { endOfShow: 'End of slide show, click to exit.', empty: 'No .pptx loaded' },
  pt: { endOfShow: 'Fim da apresentação de slides. Clique para sair.', empty: 'Nenhum .pptx carregado' },
  es: { endOfShow: 'Fin de la presentación. Haz clic para salir.', empty: 'No se ha cargado ningún .pptx' },
};

/** Opções de `play()` — reprodução automática. */
export interface PlayOptions extends PlaybackOptions {
  /** Slide (índice no deck) onde começar. Default 0. */
  from?: number;
  /** Recomeça do início ao terminar (quiosque). Default false. */
  loop?: boolean;
  /**
   * Mídia (vídeo/áudio/GIF): `realtime` toca de verdade (com som) e corrige
   * deriva; `seek` não toca — `syncMedia()` busca o quadro exato antes de cada
   * pintura (captura determinística). Default `realtime`.
   */
  media?: 'realtime' | 'seek';
}

/**
 * Opções de `present()` — apresentação por clique. Sem a opção, vale o
 * "Configurar apresentação" do arquivo (`<p:showPr>`) e, sem ele, o default.
 */
export interface PresentOptions {
  /**
   * Usa os intervalos gravados (`advTm`): nesses slides os passos "ao clicar"
   * disparam sozinhos e o slide avança sozinho (como no app). Default true.
   */
  useTimings?: boolean;
  /** Depois do último slide volta ao primeiro (em vez da tela de fim). Default false. */
  loop?: boolean;
  /** Pede tela cheia ao começar; sair dela encerra a apresentação. Default true. */
  fullscreen?: boolean;
  /** false = apresentar sem animação (slides no estado final). Default true. */
  animations?: boolean;
  /** Quiosque: clique/teclas não avançam (só intervalos e hiperlinks); Esc sai; repete. Default false. */
  kiosk?: boolean;
}

const NO_TIMING: SlideTimingModel = { steps: [], interactive: [], hidden: false };

/** A paint of the Rust engine waiting for a picture to decode (`PENDING` of the engine: painted again when it is ready). */
const RUST_PENDING = 13;

/** Todos os slides do modelo TS (um slide ilegível vira vazio, como sempre). */
function readAllSlides(pkg: PptxPackage, pres: Presentation): { slides: Slide[]; paths: string[] } {
  const slides: Slide[] = [];
  const paths: string[] = [];
  for (const ref of pres.slides) {
    if (!ref.target) continue;
    paths.push(ref.target);
    try { slides.push(readSlide(pkg, ref.target)); } catch { slides.push({ shapes: [] }); }
  }
  return { slides, paths };
}

/** O slide no palco: o `PlaybackSlide` do TS, ou só o índice e a transição (o palco do Rust, sem o modelo TS). */
type StageSlide = { index: number; transition?: SlideTransitionInfo | null | undefined };

/** Um hiperlink sob o ponteiro: endereço, slide (índice do viewer) ou posição da apresentação. */
type LinkHit = { url: string } | { slide: number } | { show: 'next' | 'previous' | 'first' | 'last' | 'end' };

/** O que o viewer usa da máquina de estados da apresentação: a `SlideShow` do TS ou a do Rust (`RustShowMachine`). */
interface ShowMachine {
  readonly index: number;
  readonly step: number;
  readonly steps: number;
  readonly ended: boolean;
  next(now: number, mouse?: boolean): SlideShowAction;
  prev(now: number): SlideShowAction;
  goTo(index: number, now: number): SlideShowAction;
  first(now: number): SlideShowAction;
  last(now: number): SlideShowAction;
  update(now: number): SlideShowAction;
  wakeAt(now: number): number;
}

/** A apresentação do Rust como máquina de estados do viewer (deck aberto só no Rust): os eventos saem dela. */
class RustShowMachine implements ShowMachine {
  private f: ShowFrame;
  constructor(readonly h: SlidesShowHandle, now: number) { this.f = h.frame(now); }
  private after(now: number, a: ShowAction): SlideShowAction { this.f = this.h.frame(now); return a as SlideShowAction; }
  get index(): number { return this.f.index; }
  get step(): number { return this.f.step; }
  get steps(): number { return this.f.steps; }
  get ended(): boolean { return this.f.ended; }
  next(now: number, mouse = false): SlideShowAction { return this.after(now, this.h.next(now, mouse)); }
  prev(now: number): SlideShowAction { return this.after(now, this.h.prev(now)); }
  goTo(index: number, now: number): SlideShowAction { return this.after(now, this.h.goTo(index, now)); }
  first(now: number): SlideShowAction { return this.after(now, this.h.first(now)); }
  last(now: number): SlideShowAction { return this.after(now, this.h.last(now)); }
  update(now: number): SlideShowAction { return this.after(now, this.h.update(now)); }
  wakeAt(now: number): number { return this.h.wakeAt(now); }
  frame(now: number): ShowFrame { this.f = this.h.frame(now); return this.f; }
}

/** Tamanho (px a 96 dpi) da imagem INTEIRA de um p:pic na página — o recorte (srcRect) a amplia. */
function picDisplaySize(sh: Shape): { w: number; h: number } | undefined {
  const x = sh.xfrm;
  if (!x || !(x.cx > 0) || !(x.cy > 0)) return undefined;
  const c = sh.srcRect;
  const fw = c ? Math.max(0.05, 1 - c.l - c.r) : 1, fh = c ? Math.max(0.05, 1 - c.t - c.b) : 1;
  return { w: x.cx / 9525 / fw, h: x.cy / 9525 / fh };
}

/**
 * Substituto do PowerPoint para família que não existe em lugar nenhum (nem
 * embutida, nem no SO): medido no oráculo (interest-calculator: "Bellota Text"
 * e "Archivo" ausentes saem com as métricas da Calibri; Liberation Sans/Arial
 * quebra as linhas em outro lugar). É a Calibri mesmo com o tema em Aptos
 * (syn-b-e28: "Gill Sans"/"Bellota Text"/"Foo Bar Sans" medem 332,28 pt nos
 * dois temas = Calibri; a Aptos mede 348,20). Sem Calibri na máquina, a Carlito
 * (métrico-compatível); a Aptos só se nem essa houver.
 */
const MISSING_FONT_FALLBACKS = ['Calibri', 'Aptos'];

/** Teclas do apresentador (as do app de referência). */
const NEXT_KEYS = new Set(['ArrowRight', 'ArrowDown', 'PageDown', ' ', 'Enter', 'n', 'N']);
const PREV_KEYS = new Set(['ArrowLeft', 'ArrowUp', 'PageUp', 'Backspace', 'p', 'P']);

/**
 * <universal-slides-viewer> — webcomponent de VISUALIZAÇÃO pura de
 * apresentações .pptx, fiel ao app de referência. É a abstração embarcável do
 * modo de exibição do Universal Slides: carrega bytes (ou `src` por URL),
 * navega, pinta cada slide em canvas, apresenta (animações e transições),
 * abre hyperlinks e renderiza miniaturas. NÃO edita — o `<universal-slides>`
 * (editor) herda desta classe e acrescenta a edição inline.
 *
 * A pintura em si vive na função PURA `paintSlideTo` (slides-core/render.ts),
 * compartilhada com miniaturas e export de imagens: uma só fonte de verdade.
 *
 * Três modos:
 *  - NAVEGAÇÃO (padrão): o slide parado, como foi desenhado (o modo Normal do
 *    app de referência) — sem animação nem transição;
 *  - APRESENTAÇÃO (`present()`): o F5 — o apresentador avança passo a passo;
 *  - REPRODUÇÃO AUTOMÁTICA (`play()`): tudo anda sozinho (export de vídeo /
 *    quiosque).
 *
 * Controles (como os do <video>): visíveis por padrão; `nocontrols` desliga
 * todos e um atributo booleano por item tira só ele (`nodownload`,
 * `nofullscreen`, `nothumbnails`… — ver
 * controls.ts: variáveis CSS `--upv-*` e `::part()`).
 *
 * API: atributos `src`, `fonts`, `nocontrols`, `no<item>`; `loadPptx(bytes)`, `fontsReady()`,
 * `getSlideCount/getSlideIndex`, `selectSlide/next/prev`,
 * `present(from?, opts?)`/`stopPresentation()`/`isPresenting()`,
 * `play(opts?)`/`stopPlayback()`/`isPlaying()`, `renderThumbnail(i, canvas)`,
 * `renderSlideToCanvas(i, canvas, pxWidth)`.
 * Eventos: `us-loaded`, `us-slide`, `us-step`, `us-image`, `us-present-end`,
 * `us-playback-end`, `us-download` (cancelável: o host entrega o arquivo).
 */
export class UniversalSlidesViewerElement extends HTMLElement {
  protected host = this.attachShadow({ mode: 'open' });
  protected canvas = document.createElement('canvas');
  protected badge = document.createElement('div');
  /** Wrapper que hospeda canvas + badge (+ overlays de subclasses). */
  protected wrap = document.createElement('div');

  protected pkg: PptxPackage | null = null;
  protected pres: Presentation | null = null;
  protected slides: Slide[] = [];
  protected index = 0;
  /**
   * Cache de decodificacao de imagens (`<p:pic>` + bg image). Chave = `ImageRef.part`
   * (caminho unico no pacote). Reutilizamos entre slides — uma logo repetida em
   * varios slides decodifica UMA vez. Pintamos sem bloquear: enquanto a imagem
   * ainda nao carregou, deixamos o retangulo vazio; um repaint e disparado quando
   * `onload` chega.
   */
  protected imageCache = new Map<string, { img: PaintImage; ready: boolean }>();
  /** Cache de ChartModel parseado, por XML cru (passado a paintSlideTo). */
  protected chartCache = new Map<string, ChartModel>();
  /**
   * Reprodução automática em curso (`play()`): linha do tempo PURA
   * (slides-core/playback.ts) + relógio. `null` = sem reprodução.
   */
  protected auto: { tl: PlaybackTimeline; t0: number; loop: boolean; rafId: number; pos: number; ended: boolean; rtl: SlidesTimelineHandle | null } | null = null;
  /**
   * Apresentação em curso (`present()`): máquina de estados PURA
   * (slides-core/slideshow.ts) + agendamento do próximo quadro. `null` = sem
   * apresentação.
   */
  protected show: { ss: ShowMachine; rs: SlidesShowHandle | null; mirror: boolean; rafId: number; timer: number; fullscreen: boolean; kiosk: boolean } | null = null;
  // ───────────────────────── motor Rust (engine="rust") ─────────────────────────
  // O motor TS continua inteiro por trás do atributo (padrão `ts`). Com `engine="rust"` e o módulo já instanciado (o viewer o
  // pré-carrega ao conectar; `src` espera por ele) o deck abre SÓ no Rust: sem `PptxPackage`/`readSlide`, `this.pkg` fica null e
  // tudo vem do módulo — layout e lista de desenho (paradas, miniaturas, palco com animações), o modelo de leitura (texto
  // acessível, hiperlinks, imagens a decodificar, mídia, fontes embutidas e famílias, configuração da apresentação), a
  // apresentação e a reprodução (as máquinas de estado do Rust dão os eventos) e o estado da mídia de cada quadro. O modelo TS
  // só é montado (`ensureTsModel`, contado em `tsModelLoads`) se o módulo falhar ou o atributo voltar a `ts`. Um `loadPptx` antes
  // de o módulo carregar (ou um .ppt) abre no TS e o Rust por trás; quando o Rust abre, o modelo TS sai (`dropTsModel`).
  // Registro: uo-bindings.md "Slides viewer (B7)", "Wave 4".
  /** a sessão do motor Rust (carregada sob demanda) e o módulo que a define */
  private rust: RustSlides | null = null;
  private rustMod: Promise<typeof import('./rust-engine.js')> | null = null;
  /** a sessão veio do editor (`adoptRust`): sobrevive a `disconnectedCallback` (mover o elemento no DOM mantém o histórico, como no TS) */
  private rustAdopted = false;
  /** a abertura do deck no motor Rust em andamento ou concluída (`rustReady`) */
  private rustOpening: Promise<void> = Promise.resolve();
  private rustSeq = 0;
  private static engineSource: LoadSlidesOptions | undefined;
  /** o código do motor já carregado (`loadRustEngine` resolvido): a abertura síncrona de `loadPptx` precisa dele */
  private rustModLoaded: typeof import('./rust-engine.js') | null = null;
  /** vezes em que o modelo TS foi montado para um deck aberto só no Rust (falha do módulo, troca de motor): o harness conta */
  protected tsModelLoads = 0;
  /** identidade do deck carregado: o que termina depois (fontes embutidas, abertura no Rust) de um deck anterior não vale */
  private deckToken: object = {};

  /**
   * De onde vem o motor Rust (`engine="rust"`): o .wasm do viewer e o do conversor de .ppt (bytes, URL ou Response). Sem isto, os
   * do pacote (`chunks/uo-slides.wasm`, `chunks/uo-slides-ppt.wasm`, ao lado do código do motor). Há UM módulo por página: vale o
   * primeiro carregamento. O viewer sem `engine="rust"` nunca baixa o .wasm.
   */
  static configureEngine(source: LoadSlidesOptions): void {
    UniversalSlidesViewerElement.engineSource = source;
  }

  /** `rust` só quando o atributo `engine` diz `rust`; qualquer outro valor é o motor TS. */
  protected engineAttr(): 'ts' | 'rust' {
    // o editor (subclasse sem a barra embutida) edita o pacote: o deck aberto no módulo ficaria velho. Só o viewer troca de motor.
    if (!this.controlsBuiltIn()) return 'ts';
    return (this.getAttribute('engine') ?? '').trim().toLowerCase() === 'rust' ? 'rust' : 'ts';
  }

  /** O motor que pinta agora: `rust` quando o deck está aberto no módulo wasm, senão `ts` (carregando, falha, atributo). */
  getEngine(): 'ts' | 'rust' { return this.rust?.document && !this.rust.document.freed ? 'rust' : 'ts'; }

  /** Resolve quando a abertura do deck no motor Rust terminou (com ou sem sucesso; `getEngine()` diz qual). */
  rustReady(): Promise<void> { return this.rustOpening; }

  /** O lado do motor de engine="rust" (importado sob demanda, compartilhado com o openRust do próprio viewer). */
  protected loadRustEngine(): Promise<typeof import('./rust-engine.js')> {
    return (this.rustMod ??= import('./rust-engine.js').then((m) => { this.rustModLoaded = m; return m; }));
  }

  /** engine="rust": carrega o código do motor e instancia o .wasm antes de o deck chegar (a abertura fica síncrona, sem o TS). */
  private preloadRust(): Promise<void> {
    if (this.engineAttr() !== 'rust') return Promise.resolve();
    return this.loadRustEngine().then((m) => m.preloadSlidesEngine(UniversalSlidesViewerElement.engineSource)).catch(() => {});
  }

  /** O que a sessão do Rust pede ao elemento: pilha de fontes, imagens decodificadas, o .wasm e o aviso de imagem preparada. */
  private rustHost(): RustSlidesHost {
    return {
      fontStack: this.fontStackFor,
      image: (part) => { const e = this.imageCache.get(part); return e?.ready ? e.img as EnginePaintImage : null; },
      source: () => UniversalSlidesViewerElement.engineSource,
      prepared: () => { if (this.isConnected) { this.paint(); this.controls?.renderThumbs(); } },
    };
  }

  /**
   * engine="rust" com o módulo já instanciado: abre o deck SÓ no Rust. null = não dá para abrir assim (o código ou o .wasm ainda
   * não carregou, um .ppt — o conversor é assíncrono); lança quando o Rust recusa o arquivo.
   */
  private openRustOnly(bytes: Uint8Array): RustSlides | null {
    const mod = this.rustModLoaded;
    if (this.engineAttr() !== 'rust' || !mod || !mod.slidesEngineReady()) return null;
    const rs = new mod.RustSlides(this.rustHost());
    try {
      if (!rs.openSync(bytes)) { rs.dispose(); return null; }
    } catch (e) { rs.dispose(); throw e; }
    return rs;
  }

  /** O documento aberto no Rust (ou null). */
  private rustDoc(): SlidesDocument | null { const d = this.rust?.document; return d && !d.freed ? d : null; }

  /** O deck está aberto só no Rust (sem o modelo TS). */
  protected rustOnly(): boolean { return !this.pkg && this.rustDoc() !== null; }

  /** Número de slides do deck aberto (modelo TS ou Rust). */
  protected slideCount(): number {
    if (!this.pkg) this.healRust();
    return this.pkg ? this.slides.length : (this.rustDoc()?.count ?? 0);
  }

  /** Tamanho do slide em EMU (modelo TS ou Rust), ou null sem deck. */
  protected slideSize(): { w: number; h: number } | null {
    if (!this.pkg) this.healRust();
    if (this.pres) return { w: this.pres.slideW, h: this.pres.slideH };
    const d = this.rustDoc();
    return d ? { w: d.slideW, h: d.slideH } : null;
  }

  /** Monta o modelo TS do arquivo aberto (deck que estava só no Rust: o módulo falhou, ou o atributo voltou a `ts`). */
  protected ensureTsModel(): boolean {
    if (this.pkg) return true;
    const bytes = this.fileBytes;
    if (!bytes) return false;
    try {
      const pkg = PptxPackage.load(bytes);
      const pres = readPresentation(pkg);
      const { slides, paths } = readAllSlides(pkg, pres);
      this.pkg = pkg; this.pres = pres; this.slides = slides; this.slidePaths = paths;
    } catch { return false; }
    this.tsModelLoads++;
    for (const s of this.slides) this.warmSlideImages(s);
    return true;
  }

  /** O Rust abriu o deck que o TS já tinha aberto (um `loadPptx` antes de o módulo carregar): o modelo TS sai. */
  private dropTsModel(): void {
    if (!this.pkg || this.show || this.auto || !this.controlsBuiltIn() || this.engineAttr() !== 'rust') return;
    this.pkg = null; this.pres = null; this.slides = []; this.slidePaths = [];
    this.chartCache.clear();
  }

  /**
   * O módulo morreu (um trap) sob um deck aberto só no Rust: o modelo TS assume já (o aviso diz por quê) — antes de qualquer
   * pergunta de contagem ou tamanho, que sem isso responderia "sem deck".
   */
  private healRust(): void {
    if (!this.rust || this.rustAdopted || this.rustDoc() || !this.rust.document) return;
    this.rustFailed(new Error('uo-slides: the engine instance stopped'));
    this.ensureTsModel();
  }

  /** O módulo falhou (um trap): o deck segue no TS (montado se preciso) e o aviso sai uma vez. */
  private rustFailed(error: unknown): void {
    this.disposeRust();
    this.dispatchEvent(new CustomEvent('us-engine-error', { detail: { engine: 'rust', error } }));
  }

  private openRust(bytes: Uint8Array): void {
    const seq = ++this.rustSeq;
    const tok = this.deckToken;
    this.disposeRust();
    this.rustOpening = (async () => {
      try {
        const mod = await this.loadRustEngine();
        const rs = new mod.RustSlides(this.rustHost());
        await rs.open(bytes);
        if (seq !== this.rustSeq || this.deckToken !== tok) { rs.dispose(); return; }
        this.rust = rs;
        this.notesCache = [];
        this.dropTsModel();
        this.paint();
        this.controls?.renderThumbs();
        this.dispatchEvent(new CustomEvent('us-engine', { detail: { engine: 'rust' } }));
      } catch (error) {
        if (seq !== this.rustSeq) return;
        // o deck segue no motor TS: nada para o usuário, só o evento
        this.dispatchEvent(new CustomEvent('us-engine-error', { detail: { engine: 'rust', error } }));
      }
    })();
  }

  /**
   * O editor (subclasse) abre o deck no módulo e entrega a sessão: o viewer pinta por ela (paradas, miniaturas, anotações,
   * apresentação e reprodução). null = volta ao motor TS.
   */
  protected adoptRust(rs: RustSlides | null): void {
    this.rustSeq++;
    this.disposeRust();
    this.rust = rs;
    this.rustAdopted = rs !== null;
    this.notesCache = [];
    this.paint();
    this.controls?.renderThumbs();
    if (rs) this.dispatchEvent(new CustomEvent('us-engine', { detail: { engine: 'rust' } }));
  }

  /** A sessão do motor Rust em uso (ou null). */
  protected rustSession(): RustSlides | null { return this.rust; }

  private disposeRust(): void {
    // os espelhos do palco são do deck que sai: apresentação e reprodução seguem pintando pelo TS
    if (this.show) { this.show.rs?.free(); this.show.rs = null; }
    if (this.auto) { this.auto.rtl?.free(); this.auto.rtl = null; }
    this.rust?.dispose();
    this.rust = null;
    this.rustAdopted = false;
  }

  /** Uma fonte carregou ou a pilha de fontes mudou: o motor Rust pede as medidas de novo; repinta. */
  protected fontsChanged(): void {
    this.rust?.fontsChanged();
    this.paint();
  }

  /**
   * Pinta o slide PARADO `i` (navegação, miniatura, exportação): pelo motor Rust quando o deck está nele, senão `paintSlideTo`.
   * A edição inline (`suppressTextShapeIdx`) vai ao Rust como `suppress`; qualquer falha do módulo cai no motor TS.
   */
  private paintStill(ctx: CanvasRenderingContext2D, i: number, scale: number, opts: PaintSlideOptions): void {
    const rs = this.rust;
    if (rs) {
      try {
        const r = rs.paint(ctx, i, scale, opts.ox ?? 0, opts.oy ?? 0, opts.suppressTextShapeIdx);
        // PENDING: uma imagem do slide ainda decodifica (a fase de preparo); `prepared` repinta
        if (r.status === 0 || r.status === RUST_PENDING) return;
        // 11: o módulo parou (um trap derrubou a instância): o deck segue no TS e o aviso sai uma vez
        if (r.status === 11) this.rustFailed(new Error('uo-slides: the engine instance stopped'));
      } catch (error) {
        // um trap do módulo o derruba; o deck segue no TS
        this.rustFailed(error);
      }
    }
    // o motor TS: o deck aberto só no Rust monta o modelo TS agora (contado)
    if (!this.ensureTsModel()) return;
    const slide = this.slides[i];
    if (!slide || !this.pres) return;
    paintSlideTo(ctx, slide, { slideW: this.pres.slideW, slideH: this.pres.slideH, images: this.readyImages(slide), charts: this.chartCache }, scale, opts);
  }

  /** Texto da tela de fim da apresentação (o do PowerPoint, no idioma da interface). */
  protected get endOfShowText(): string { return pickLabels(this, TEXT).endOfShow; }
  /** Canvases de trabalho da transição (origem/destino) na reprodução automática. */
  private fromCanvas: HTMLCanvasElement | null = null;
  private toCanvas: HTMLCanvasElement | null = null;
  private fromKey = '';
  /** Vídeo/áudio/GIF da reprodução automática. */
  protected media = new MediaPlayer(() => this.paint());

  static get observedAttributes(): string[] {
    return ['src', 'nocontrols', ...CONTROL_ITEMS.map((it) => `no${it}`), 'lang', 'engine', 'link-schemes'];
  }

  /**
   * Barra de controles embutida. O editor (subclasse) tem a própria interface:
   * desliga, e mantém o selo "n / N" no lugar dela.
   */
  protected controlsBuiltIn(): boolean { return true; }
  protected controls: ViewerControls | null = null;
  /** Texto acessível do slide em cena + anúncios (leitores de tela). */
  protected a11y: SlideA11y | null = null;
  /** Bytes do arquivo carregado (botão de download). */
  protected fileBytes: Uint8Array | null = null;
  /** Caminho da parte de cada slide (anotações) e anotações já lidas. */
  private slidePaths: string[] = [];
  private notesCache: (string | undefined)[] = [];
  private anyNotes: boolean | null = null;

  constructor() {
    super();
    this.canvas.style.cssText = 'display:block;width:100%;height:100%;outline:none;background:#3c3c3c;';
    this.canvas.tabIndex = 0;
    this.badge.style.cssText =
      'position:absolute;right:10px;bottom:8px;padding:2px 8px;border-radius:10px;' +
      'background:rgba(0,0,0,.55);color:#fff;font:600 12px/1.4 Segoe UI,Arial;pointer-events:none;';
    this.wrap.style.cssText = 'position:relative;width:100%;height:100%;overflow:hidden;';
    this.wrap.appendChild(this.canvas);
    this.wrap.appendChild(this.badge);
    this.host.appendChild(this.wrap);
    this.canvas.addEventListener('keydown', this.onKeyDown);
    // Hyperlinks: click simples sobre shape cujo txBody contém run.hyperlink
    // abre URL em nova aba. Na navegação o clique NUNCA avança o slide; na
    // apresentação, avança (fora de hyperlink).
    this.canvas.addEventListener('click', this.onClick);
    this.canvas.addEventListener('mousemove', this.onMouseMove);
    if (this.controlsBuiltIn()) {
      this.badge.style.display = 'none';
      // player: o slide encosta nas bordas; o que sobra (letterbox) é o fundo do elemento
      this.canvas.style.background = 'var(--upv-background, var(--uo-background, #000))';
      this.controls = new ViewerControls(this.controlsHost(), this.host, this.wrap);
      this.a11y = new SlideA11y(this.canvas, this.wrap);
      // foco visível no canvas (só pelo teclado)
      this.canvas.style.outline = '';
      const focusCss = document.createElement('style');
      focusCss.textContent = 'canvas:focus-visible { outline: 2px solid var(--upv-focus-color, var(--uo-focus-color, #4ea1ff)); outline-offset: -3px; }';
      this.host.appendChild(focusCss);
      this.wrap.addEventListener('pointermove', () => this.controls?.wake());
      this.wrap.addEventListener('pointerdown', () => this.controls?.wake());
      this.wrap.addEventListener('pointerleave', (e) => { if (e.pointerType === 'mouse') this.controls?.leave(); });
      this.addEventListener('us-image', () => this.controls?.renderThumbs());
    }
  }

  /** Liga a barra conforme os atributos — no connect e quando eles mudam. */
  private configureControls(): void {
    const lang = uiLang(this);
    this.a11y?.setLang(lang);
    this.a11y?.setName(this.fileName());
    this.controls?.configure(!this.hasAttribute('nocontrols'), CONTROL_ITEMS.filter((it) => this.hasAttribute(`no${it}`)), lang);
  }

  /** Ponte da barra de controles com a API pública do viewer. */
  private controlsHost(): ControlsHost {
    return {
      count: () => this.slideCount(),
      index: () => this.index,
      goTo: (i) => this.selectSlide(i),
      next: () => this.next(),
      prev: () => this.prev(),
      isPlaying: () => this.isPlaying(),
      playbackFraction: () => {
        const a = this.auto;
        if (!a || a.ended || a.tl.durationMs <= 0) return null;
        return Math.max(0, Math.min(1, (this.now() - a.t0) / a.tl.durationMs));
      },
      togglePlay: () => {
        if (this.isPlaying()) { this.stopPlayback(); return; }
        this.play({ from: this.index >= this.slideCount() - 1 ? 0 : this.index });
      },
      playFrom: (f) => {
        const a = this.auto;
        if (!a || a.ended) return;
        // o quadro é função pura do tempo: buscar = mover a origem do relógio
        a.t0 = this.now() - f * a.tl.durationMs;
        a.pos = -1;
        this.fromKey = '';
      },
      hasMedia: () => this.deckHasMedia(),
      isMuted: () => this.media.muted,
      setMuted: (m) => { this.media.setMuted(m); this.paint(); },
      notes: (i) => this.slideNotes(i),
      hasNotes: () => (this.anyNotes ??= Array.from({ length: this.slideCount() }, (_, i) => i).some((i) => this.slideNotes(i) !== '')),
      download: () => this.downloadFile(),
      isPresenting: () => this.isPresenting(),
      togglePresent: () => (this.show ? this.stopPresentation() : this.present(this.index)),
      isFullscreen: () => this.host.fullscreenElement === this.wrap,
      toggleFullscreen: () => this.toggleFullscreen(),
      renderThumbnail: (i, c) => this.renderThumbnail(i, c),
      slideAspect: () => { const z = this.slideSize(); return z ? z.w / z.h : 16 / 9; },
    };
  }

  /** Anotações do orador do slide `i` (texto puro; lidas sob demanda). */
  getSlideNotes(i: number): string { return this.slideNotes(i); }

  /** Há vídeo/áudio em algum slide (o botão de som). */
  private deckHasMedia(): boolean {
    if (this.pkg) return this.slides.some((s) => s.shapes.some((sh) => sh.media));
    const doc = this.rustDoc();
    if (!doc) return false;
    for (let i = 0; i < doc.count; i++) {
      try { if (doc.assets(i).media.some((m) => m.media)) return true; } catch { /* slide ilegível */ }
    }
    return false;
  }

  private slideNotes(i: number): string {
    let t = this.notesCache[i];
    if (t === undefined) {
      const path = this.slidePaths[i];
      try { t = this.rust?.document ? this.rust.notes(i) : this.pkg && path ? readSlideNotes(this.pkg, path) : ''; } catch { t = ''; }
      this.notesCache[i] = t;
    }
    return t;
  }

  /** Tela cheia do viewer (sem apresentar). Na apresentação, sair dela a encerra. */
  toggleFullscreen(): void {
    if (this.host.fullscreenElement === this.wrap) document.exitFullscreen().catch(() => {});
    else if (document.fullscreenEnabled && this.wrap.requestFullscreen) this.wrap.requestFullscreen().catch(() => {});
  }

  /**
   * Botão de download: emite `us-download` (cancelável, com os bytes e o
   * nome); se ninguém cancelar, baixa o arquivo original.
   */
  private downloadFile(): void {
    const bytes = this.fileBytes;
    if (!bytes) return;
    const ole = bytes[0] === 0xd0 && bytes[1] === 0xcf; // .ppt (binário)
    const src = this.getAttribute('src');
    let name = '';
    try { name = src ? decodeURIComponent(src.split(/[?#]/)[0]!.split('/').pop() ?? '') : ''; } catch { /* nome ilegível */ }
    if (!/\.(pptx|pptm|ppsx|ppsm|potx|ppt|pps)$/i.test(name)) name = `apresentacao.${ole ? 'ppt' : 'pptx'}`;
    const ev = new CustomEvent('us-download', { cancelable: true, detail: { bytes, filename: name } });
    if (!this.dispatchEvent(ev)) return;
    const type = ole ? 'application/vnd.ms-powerpoint' : 'application/vnd.openxmlformats-officedocument.presentationml.presentation';
    const url = URL.createObjectURL(new Blob([new Uint8Array(bytes)], { type }));
    const a = document.createElement('a');
    a.href = url; a.download = name;
    a.click();
    setTimeout(() => URL.revokeObjectURL(url), 1000);
  }

  private onAnyFullscreenChange = (): void => { this.controls?.wake(); this.paint(); };
  /** `document.fonts` terminou de carregar faces (as medidas do motor Rust dependem delas). */
  private onFontsDone = (): void => { if (this.isConnected) this.fontsChanged(); };

  private resizeObs: ResizeObserver | null = null;
  /** Mudança de devicePixelRatio (janela levada a outro monitor): o tamanho CSS não muda, o backing store sim. */
  private dprWatch: { mq: MediaQueryList; onChange: () => void } | null = null;
  /** Geração do carregamento: resposta de `src` antiga não sobrescreve um deck mais novo. */
  private loadSeq = 0;
  /** `src` já carregado/pedido (o upgrade do elemento dispara attributeChanged E connected). */
  private srcUrl: string | null = null;

  connectedCallback(): void {
    this.resizeObs ??= new ResizeObserver(() => { this.resize(); this.paint(); });
    this.resizeObs.observe(this.canvas);
    this.watchDpr();
    this.restoreParkedFonts();
    this.unsubscribeFonts ??= this.fontResolver.onChange(() => this.fontsChanged());
    document.fonts?.addEventListener?.('loadingdone', this.onFontsDone);
    this.unsubscribeLang ??= onBrowserLangChange(() => { this.configureControls(); this.paint(); });
    this.configureControls();
    document.addEventListener('fullscreenchange', this.onAnyFullscreenChange);
    this.resize(); this.paint();
    (document as unknown as { fonts?: { ready: Promise<unknown> } }).fonts?.ready.then(() => this.fontsChanged()).catch(() => {});
    // engine="rust": o módulo carrega já (a abertura do deck fica síncrona, só no Rust); a desconexão liberou os handles do
    // módulo: reabre do arquivo guardado (na hora, se o módulo está pronto)
    if (this.engineAttr() === 'rust') {
      void this.preloadRust();
      if (this.fileBytes && !this.rust) {
        let rs: RustSlides | null = null;
        try { rs = this.openRustOnly(this.fileBytes); } catch { rs = null; }
        if (rs) { this.rust = rs; this.rustOpening = Promise.resolve(); this.notesCache = []; }
        else { if (!this.pkg) this.ensureTsModel(); this.openRust(this.fileBytes); }
      }
    }
    const src = this.getAttribute('src');
    if (src && src !== this.srcUrl) void this.loadSrc(src);
  }

  /**
   * Saiu do documento: nada pode continuar rodando — laços de quadro,
   * timers, áudio/vídeo, URLs de mídia, observadores e as fontes embutidas
   * que ESTE viewer registrou (o editor cria um viewer por apresentação).
   */
  disconnectedCallback(): void {
    this.resizeObs?.disconnect();
    this.unwatchDpr();
    document.removeEventListener('fullscreenchange', this.onAnyFullscreenChange);
    this.controls?.dispose();
    this.stopPresentation();
    this.stopPlayback();
    this.media.dispose();
    this.parkFonts();
    document.fonts?.removeEventListener?.('loadingdone', this.onFontsDone);
    // a sessão do editor (adotada) fica: o histórico de desfazer vive no editor wasm sobre esse deck; sai com loadPptx/adoptRust(null)/troca de motor ou no FinalizationRegistry
    if (!this.rustAdopted) this.disposeRust();
    this.unsubscribeFonts?.();
    this.unsubscribeFonts = null;
    this.unsubscribeLang?.();
    this.unsubscribeLang = null;
  }

  attributeChangedCallback(name: string, oldV: string | null, newV: string | null): void {
    if (name === 'src' && newV && newV !== oldV && this.isConnected && newV !== this.srcUrl) void this.loadSrc(newV);
    else if (name === 'engine') {
      if (this.engineAttr() === 'rust') void this.preloadRust();
      if (this.fileBytes && newV !== oldV) {
        if (this.engineAttr() === 'rust') { if (!this.rustDoc()) this.openRust(this.fileBytes); }
        else { this.ensureTsModel(); this.rustSeq++; this.disposeRust(); this.notesCache = []; this.paint(); this.controls?.renderThumbs(); }
      }
    } else if (name !== 'src' && name !== 'link-schemes' && this.isConnected) {
      this.configureControls();
      if (name === 'lang') this.paint();
    }
  }

  /**
   * Carrega um .pptx por URL (mesma origem ou CORS liberado). Só a carga MAIS
   * RECENTE vale (`src` trocado de novo ou `loadPptx` direto no meio do
   * download descarta a resposta antiga). Falha → evento `us-error`.
   */
  protected async loadSrc(url: string): Promise<void> {
    const seq = ++this.loadSeq;
    this.srcUrl = url;
    try {
      // engine="rust": o módulo carrega junto com o arquivo, para o deck abrir só no Rust
      const [res] = await Promise.all([fetch(url), this.preloadRust()]);
      if (!res.ok) throw new Error(`HTTP ${res.status}`);
      const bytes = new Uint8Array(await res.arrayBuffer());
      if (seq !== this.loadSeq) return;
      this.loadPptx(bytes);
    } catch (error) {
      if (seq !== this.loadSeq) return;
      this.dispatchEvent(new CustomEvent('us-error', { detail: { src: url, error } }));
    }
  }

  private watchDpr(): void {
    this.unwatchDpr();
    const mq = window.matchMedia?.(`(resolution: ${window.devicePixelRatio || 1}dppx)`);
    if (!mq) return;
    const onChange = (): void => { this.resize(); this.paint(); this.watchDpr(); };
    mq.addEventListener('change', onChange);
    this.dprWatch = { mq, onChange };
  }

  private unwatchDpr(): void {
    if (!this.dprWatch) return;
    this.dprWatch.mq.removeEventListener('change', this.dprWatch.onChange);
    this.dprWatch = null;
  }

  // ───────────────────────── FONTES ─────────────────────────
  // Fontes EMBUTIDAS no arquivo (p:embeddedFontLst) são deste viewer (ciclo de
  // vida do deck); todo o resto — padrão (Liberation Sans, base do atributo
  // `fonts`), fontes do SO/host, instaladas, substitutas abertas — vem do
  // resolvedor ÚNICO dos apps (shared/fonts), o mesmo do Write e do Sheet.

  /** Famílias com FontFace carregado (embutidas/SO/default). */
  protected loadedFamilyNames = new Set<string>();
  private defaultFontsInstalled = false;

  /** Fonte padrão (Liberation Sans) pela base do atributo `fonts` — via o
   *  resolvedor único (sem base configurada: o host já registrou @font-face). */
  protected async installDefaultFonts(): Promise<void> {
    if (this.defaultFontsInstalled) return;
    this.defaultFontsInstalled = true;
    this.fontResolver.suggestBaseUrl(this.getAttribute('fonts'));
    const faces = await this.fontResolver.bundledFaces();
    if (!Object.keys(faces).length) return;
    this.loadedFamilyNames.add('Liberation Sans');
    this.fontsChanged();
  }

  /** Carregamentos das fontes embutidas do deck atual (ver `fontsReady`). */
  protected embeddedFontLoads: Promise<unknown>[] = [];
  /** Decodificação/registro das fontes embutidas (adiado para depois da 1ª pintura). */
  protected embeddedFontsTask: Promise<void> = Promise.resolve();

  /**
   * Faces EMBUTIDAS que este viewer pôs no `document.fonts` (do deck atual).
   * Saem ao trocar de deck (o subset "só caracteres usados" do deck A não
   * pode servir ao deck B) e ao desconectar (voltam se reconectar).
   */
  private ownFaces = new Set<FontFace>();
  private embeddedFamilies = new Set<string>();
  private parkedFaces: FontFace[] = [];

  /** Registra as fontes EMBUTIDAS no pacote (FontFace) e repinta ao carregar. */
  protected registerEmbeddedFonts(pkg: PptxPackage): void {
    this.registerFontList(readEmbeddedFonts(pkg), this.deckToken);
  }

  /** Registra fontes embutidas (do modelo TS ou do Rust); `tok` = o deck delas (uma face que carrega depois da troca é velha). */
  private registerFontList(fonts: readonly EmbeddedFont[], tok: object): void {
    this.embeddedFontLoads = [];
    for (const f of fonts) {
      // peso/itálico REAIS da fonte (não a vaga): a vaga "bold" que traz a
      // regular fica 400 e o negrito sai sintético, como no Windows
      const weight = f.weight !== undefined ? String(f.weight) : f.variant === 'bold' || f.variant === 'boldItalic' ? '700' : '400';
      const style = (f.italic ?? (f.variant === 'italic' || f.variant === 'boldItalic')) ? 'italic' : 'normal';
      try {
        const buf = f.data.slice().buffer;
        const face = new FontFace(f.family, buf, { weight, style });
        this.embeddedFontLoads.push(face.load().then(() => {
          if (this.deckToken !== tok) return; // outro deck já entrou: esta face é velha
          document.fonts.add(face);
          this.ownFaces.add(face);
          this.embeddedFamilies.add(f.family);
          this.loadedFamilyNames.add(f.family);
          this.fontsChanged();
        }).catch(() => {}));
      } catch { /* face inválida: ignora */ }
    }
  }

  /** Tira do documento as faces embutidas deste viewer (troca de deck). */
  private releaseFonts(): void {
    for (const f of this.ownFaces) document.fonts.delete(f);
    this.ownFaces.clear();
    for (const fam of this.embeddedFamilies) this.loadedFamilyNames.delete(fam);
    this.embeddedFamilies.clear();
    this.parkedFaces = [];
  }

  /** Desconectado: as faces saem do documento, guardadas para uma reconexão. */
  private parkFonts(): void {
    for (const f of this.ownFaces) document.fonts.delete(f);
    this.parkedFaces = [...this.ownFaces];
  }

  private restoreParkedFonts(): void {
    for (const f of this.parkedFaces) document.fonts.add(f);
    this.parkedFaces = [];
  }

  /** Resolvedor de fontes ÚNICO dos apps (shared/fonts): host, Local Font
   *  Access, instaladas pelo nome, substitutas abertas, padrão. */
  protected readonly fontResolver = FontResolver.shared();
  private unsubscribeFonts: (() => void) | null = null;
  private unsubscribeLang: (() => void) | null = null;

  /** Pilha CSS de uma família do texto: a EMBUTIDA no deck, se houver;
   *  senão a que o resolvedor escolheu (vai em `PaintSlideOptions.fontStack`). */
  protected readonly fontStackFor = (family: string, bold = false, italic = false): string =>
    this.embeddedFamilies.has(family)
      ? `"${family.replace(/["\\]/g, '\\$&')}", "Liberation Sans", sans-serif`
      : this.fontResolver.cssStack(family, MISSING_FONT_FALLBACKS, variantOf(bold, italic));

  /**
   * Resolve quando as fontes EMBUTIDAS do deck terminaram de carregar (ou
   * falharam) e o conjunto de fontes do documento assentou — antes disso o
   * texto sai na fonte substituta (miniaturas, export, captura).
   */
  async fontsReady(): Promise<void> {
    await this.embeddedFontsTask;
    await Promise.allSettled(this.embeddedFontLoads);
    await (document as unknown as { fonts?: { ready: Promise<unknown> } }).fonts?.ready;
  }

  /**
   * Resolve as fontes usadas pelo deck (resolvedor único: do host, instaladas,
   * substitutas abertas…); as embutidas no arquivo têm precedência (ver
   * fontStackFor). O aviso de onChange repinta quando chegarem.
   */
  protected async ensureDeckFonts(pkg: PptxPackage): Promise<void> {
    await this.ensureFamilies(collectDeckTypefaces(pkg), this.deckToken);
  }

  private async ensureFamilies(families: readonly string[], tok: object): Promise<void> {
    const wanted = families.filter((fam) => !this.embeddedFamilies.has(fam));
    if (!wanted.length) return;
    await this.fontResolver.resolve(wanted);
    if (this.deckToken === tok) this.fontsChanged();
  }

  /** True quando uma subclasse está com edição inline aberta (viewer: nunca). */
  protected isEditing(): boolean { return false; }
  /** Índice de shape cujo texto NÃO deve ser pintado (edição inline). */
  protected suppressTextIdx(): number | undefined { return undefined; }

  /**
   * Carrega bytes .pptx (ou .ppt) e monta os slides. Arquivo inválido LANÇA e
   * o deck atual fica intacto (tudo é lido antes de trocar o estado).
   */
  loadPptx(bytes: Uint8Array): void {
    // engine="rust" com o módulo pronto: o deck abre só no Rust (sem o modelo TS); um arquivo que o Rust recusa tenta o TS
    let rs: RustSlides | null = null;
    try { rs = this.openRustOnly(bytes); } catch { rs = null; }
    if (rs) { this.installDeck(null, bytes, rs); return; }
    const pkg = PptxPackage.load(bytes);
    const pres = readPresentation(pkg);
    const { slides, paths } = readAllSlides(pkg, pres);
    this.installDeck({ pkg, pres, slides, paths }, bytes, null);
  }

  /** Troca o estado para o deck novo, já válido: o modelo TS (`ts`) ou a sessão do Rust (`rs`, deck aberto só no Rust). */
  private installDeck(ts: { pkg: PptxPackage; pres: Presentation; slides: Slide[]; paths: string[] } | null, bytes: Uint8Array, rs: RustSlides | null): void {
    this.loadSeq++; // um `src` ainda baixando não sobrescreve este deck
    this.srcUrl = this.getAttribute('src');
    // apresentação/reprodução/mídia/fontes embutidas são do deck anterior
    this.stopPresentation();
    this.stopPlayback();
    this.media.dispose();
    this.releaseFonts();
    this.chartCache.clear();
    const tok = this.deckToken = {};
    this.pkg = ts?.pkg ?? null;
    this.pres = ts?.pres ?? null;
    this.slides = ts?.slides ?? [];
    this.slidePaths = ts?.paths ?? [];
    this.notesCache = [];
    this.anyNotes = null;
    this.fileBytes = bytes;
    if (rs) { this.rustSeq++; this.disposeRust(); this.rust = rs; this.rustOpening = Promise.resolve(); }
    else if (this.engineAttr() === 'rust') this.openRust(bytes);
    else { this.rustSeq++; this.disposeRust(); }
    // fontes: default embarcada + embutidas no arquivo + SO sob demanda
    void this.installDefaultFonts();
    // fontes embutidas DEPOIS da 1ª pintura: decodificar EOT/MTX custa ~0,2 s
    // num deck grande, e as fontes carregam de forma assíncrona de todo jeito
    this.embeddedFontsTask = new Promise<void>((resolve) => setTimeout(() => {
      try { if (this.deckToken === tok) this.registerFontList(this.embeddedFontList(), tok); }
      catch { /* fontes embutidas ilegíveis: o deck segue com as substitutas */ }
      finally { resolve(); }
    }, 0));
    void this.ensureFamilies(this.deckFamilies(), tok);
    this.imageCache.clear();
    // pre-aquece o cache: todas as imagens deste deck (bg + pic) decodificam em
    // paralelo. Quando cada uma fica pronta, dispara repaint UMA vez.
    this.warmAllImages();
    this.index = 0;
    this.resize();
    this.paint();
    this.controls?.renderThumbs();
    this.a11y?.setName(this.fileName());
    this.dispatchEvent(new CustomEvent('us-loaded', { detail: { count: this.slideCount() } }));
    if (rs) this.dispatchEvent(new CustomEvent('us-engine', { detail: { engine: 'rust' } }));
  }

  /** As fontes embutidas do deck aberto (modelo TS ou Rust). */
  private embeddedFontList(): EmbeddedFont[] {
    if (this.pkg) return readEmbeddedFonts(this.pkg);
    return this.rustDoc()?.fonts() ?? [];
  }

  /** As famílias que o deck nomeia (modelo TS ou Rust). */
  private deckFamilies(): string[] {
    if (this.pkg) return collectDeckTypefaces(this.pkg);
    try { return this.rustDoc()?.typefaces() ?? []; } catch { return []; }
  }

  /** As imagens de todos os slides no cache (modelo TS ou o modelo de leitura do Rust). */
  private warmAllImages(): void {
    if (this.pkg) { for (const s of this.slides) this.warmSlideImages(s); return; }
    const n = this.slideCount();
    for (let i = 0; i < n; i++) this.warmRustSlide(i);
  }

  /**
   * As imagens do slide `i` do deck aberto só no Rust: as que o navegador decodifica. Metarquivos, PICT e TIFF o Rust rasteriza
   * sozinho (receita), sem o `rasterizeSpecialImage` do TS.
   */
  private warmRustSlide(i: number): void {
    const doc = this.rustDoc();
    if (!doc) return;
    let a: SlideAssets;
    try { a = doc.assets(i); } catch { return; }
    for (const im of a.images) if (!im.special) this.warmPart(im.part, im.mime, () => doc.part(im.part));
  }

  /** Os GIFs animados do slide `i` (modelo TS ou Rust) no player de mídia. */
  private warmGifsOf(i: number): void {
    if (this.pkg) { const s = this.slides[i]; if (s) this.media.warmGifs(s); return; }
    const doc = this.rustDoc();
    if (!doc) return;
    let a: SlideAssets;
    try { a = doc.assets(i); } catch { return; }
    for (const m of a.media) { const gp = m.gifPart; if (gp) this.media.warmGif(gp, () => doc.part(gp)); }
  }

  /** Imagens de um slide (fundo, pic, preenchimentos, tabelas, gráficos, marcadores) no cache. */
  private warmSlideImages(s: Slide): void {
    if (s.bgImage) this.warmImage(s.bgImage);
    for (const sh of s.shapes) {
      if (sh.image) this.warmImage(sh.image, picDisplaySize(sh));
      if (sh.fillImage) this.warmImage(sh.fillImage);
      if (sh.table) for (const r of tableImages(sh.table)) this.warmImage(r);
      for (const r of Object.values(sh.chartImages ?? {})) this.warmImage(r);
      for (const p of sh.txBody?.paragraphs ?? []) {
        if (p.bulletImage) this.warmImage(p.bulletImage);
        for (const r of p.runs) if (r.fillImage) this.warmImage(r.fillImage);
      }
    }
  }

  // ─────────────── releitura (o editor mudou partes do pacote) ───────────────

  /** Caminho da parte do slide `i` no pacote (o editor grava nela). */
  protected slidePartPath(i: number): string | undefined { return this.slidePaths[i]; }

  /**
   * Relê a lista e TODOS os slides do pacote atual depois de uma edição
   * (slide novo, excluído, movido; desfazer). Fontes, imagens já decodificadas
   * e o estado de reprodução ficam; o slide atual passa a ser `index`.
   */
  protected rereadDeck(index = this.index): void {
    const pkg = this.pkg;
    if (!pkg) return;
    const pres = readPresentation(pkg);
    const { slides, paths } = readAllSlides(pkg, pres);
    this.pres = pres;
    this.slides = slides;
    this.slidePaths = paths;
    this.notesCache = [];
    this.anyNotes = null;
    for (const s of slides) this.warmSlideImages(s);
    this.index = Math.max(0, Math.min(index, slides.length - 1));
    this.resize();
    this.paint();
    this.controls?.renderThumbs();
  }

  /** Relê só o slide `i` (texto ou forma editados). */
  protected rereadSlide(i: number): void {
    const pkg = this.pkg, path = this.slidePaths[i];
    if (!pkg || !path) return;
    try { this.slides[i] = readSlide(pkg, path); } catch { this.slides[i] = { shapes: [] }; }
    this.notesCache[i] = undefined;
    this.anyNotes = null;
    this.warmSlideImages(this.slides[i]!);
    this.paint();
  }

  /** Cor da mesa ao redor do slide no modo de edição (subclasse com tema troca). */
  protected deskColor(): string { return '#3c3c3c'; }

  // ───────────────────────── APRESENTAÇÃO ─────────────────────────

  /**
   * APRESENTAÇÃO — o F5 do app de referência: tela cheia, fundo preto, mesmo
   * motor de animação e transições da reprodução automática, mas quem avança
   * é o apresentador (semântica medida no app: `SlideShow`, slides-core/slideshow.ts):
   *  - clique / Espaço / → / ↓ / Enter / PageDown / N (e `next()`): dispara
   *    o próximo passo "ao clicar"; sem passo pendente, o próximo slide;
   *    durante uma transição, conclui a transição;
   *  - ← / ↑ / PageUp / Backspace / P (e `prev()`): desfaz o último passo
   *    (slide com intervalo: rebobina); no início do slide, o anterior no
   *    estado final, pela transição do que sai no sentido contrário;
   *  - Home/End (e `selectSlide(i)`): pula; Esc (ou sair da tela cheia): sai;
   *  - slide com intervalo gravado (`useTimings`): passos e avanço sozinhos;
   *  - depois do último slide, a tela de fim; o próximo comando sai.
   * `from` = slide inicial: índice (um oculto também, se pedido), `'start'` =
   * o F5 "do começo" (1º slide VISÍVEL da faixa) ou ausente = o atual. Emite
   * `us-slide` a cada slide, `us-step` a cada passo e `us-present-end` ao
   * sair (por qualquer motivo). Chamar a partir de um gesto do usuário (a
   * tela cheia exige).
   */
  present(from?: number | 'start', opts: PresentOptions = {}): void {
    if (!this.slideCount()) return;
    this.stopPlayback();
    const full = opts.fullscreen ?? true;
    const already = this.host.fullscreenElement === this.wrap;
    this.endShow(full && already); // recomeço já em tela cheia: continua nela
    // "Configurar apresentação" do arquivo: faixa de slides (fora dela = oculto), repetir, intervalos…
    const doc = this.pkg ? null : this.rustDoc();
    const cfg = (doc ? doc.showSettings() : this.pres?.show) ?? {};
    const range = cfg.range;
    const kiosk = opts.kiosk ?? cfg.kiosk ?? false;
    const now = this.now();
    const showOpts = {
      useTimings: opts.useTimings ?? cfg.useTimings ?? true,
      loop: opts.loop ?? ((cfg.loop || kiosk) || false),
      animations: opts.animations ?? cfg.showAnimation ?? true,
    };
    let ss: ShowMachine, rs: SlidesShowHandle | null, mirror: boolean;
    if (doc) {
      // deck só no Rust: a apresentação do Rust é A máquina de estados (eventos, agendamento e quadros do palco)
      rs = this.rust?.newShow({ from: from === 'start' ? -1 : from ?? this.index, now, ...showOpts, range: range ? [range[0], range[1]] : null }) ?? null;
      if (!rs) return;
      ss = new RustShowMachine(rs, now);
      mirror = false;
    } else {
      const models = this.slides.map((s, i) => (range && (i < range[0] || i > range[1])
        ? { ...(s.timing ?? NO_TIMING), hidden: true } : s.timing));
      const start = from === 'start' ? SlideShow.firstVisible(models) : from ?? this.index;
      ss = new SlideShow(models, start, now, showOpts);
      // engine="rust": a mesma apresentação no Rust (mesmas opções, mesmos comandos, mesmo relógio) dá os quadros do palco
      rs = this.rust?.newShow({ from: start, now, ...showOpts, range: range ? [range[0], range[1]] : null }) ?? null;
      mirror = rs !== null;
    }
    this.media.mode = 'realtime';
    this.media.clock = {};
    for (let i = 0; i < this.slideCount(); i++) this.warmGifsOf(i);
    this.show = { ss, rs, mirror, rafId: 0, timer: 0, fullscreen: full, kiosk };
    this.fromKey = '';
    this.badge.style.display = 'none';
    if (full) {
      document.addEventListener('fullscreenchange', this.onFullscreenChange);
      if (!already && document.fullscreenEnabled && this.wrap.requestFullscreen) this.wrap.requestFullscreen().catch(() => {});
    }
    this.canvas.focus();
    this.afterShowCommand(-1, -1);
  }

  /** Encerra a apresentação (sai da tela cheia); o viewer fica no slide em cena. */
  stopPresentation(): void { this.endShow(false); }

  /** Encerra a apresentação; `keepFullscreen` = outra começa em seguida (sem piscar a tela). */
  private endShow(keepFullscreen: boolean): void {
    const s = this.show;
    if (!s) return;
    if (s.rafId) cancelAnimationFrame(s.rafId);
    if (s.timer) clearTimeout(s.timer);
    s.rs?.free();
    this.show = null;
    document.removeEventListener('fullscreenchange', this.onFullscreenChange);
    if (!keepFullscreen && this.host.fullscreenElement === this.wrap) document.exitFullscreen().catch(() => {});
    this.media.stop();
    if (!this.controls) this.badge.style.display = '';
    this.paint();
    this.dispatchEvent(new CustomEvent('us-present-end', { detail: { index: this.index } }));
  }

  isPresenting(): boolean { return this.show !== null; }

  /** Saiu da tela cheia (Esc do navegador…) → a apresentação acaba junto. */
  private onFullscreenChange = (): void => {
    if (this.show?.fullscreen && this.host.fullscreenElement !== this.wrap) this.stopPresentation();
  };

  private now(): number { return (window.performance ?? Date).now(); }

  /** Aplica um comando do apresentador e emite os eventos do que mudou. */
  protected showCommand(cmd: (ss: ShowMachine, now: number) => SlideShowAction): void {
    const s = this.show;
    if (!s) return;
    const index = s.ss.index, step = s.ss.step;
    const now = this.now();
    const action = cmd(s.ss, now);
    // o espelho do Rust recebe o mesmo comando no mesmo instante (os métodos têm os nomes e argumentos da `SlideShow`)
    if (s.rs && s.mirror) cmd(s.rs as unknown as ShowMachine, now);
    if (action === 'exit') { this.stopPresentation(); return; }
    this.afterShowCommand(index, step);
  }

  /** Eventos `us-slide`/`us-step` (comparando com o estado de antes) e próximo quadro. */
  private afterShowCommand(index: number, step: number): void {
    const s = this.show;
    if (!s) return;
    const ss = s.ss;
    const detail = { index: ss.index, step: ss.step, steps: ss.steps };
    if (ss.index !== index) {
      if (ss.index >= 0) {
        this.index = ss.index;
        this.dispatchEvent(new CustomEvent('us-slide', { detail }));
      }
    } else if (ss.step !== step) {
      this.dispatchEvent(new CustomEvent('us-step', { detail }));
    }
    this.wakeShow();
  }

  /** Pinta já no próximo quadro (cancela a espera agendada). */
  private wakeShow(): void {
    const s = this.show;
    if (!s || s.rafId) return;
    if (s.timer) { clearTimeout(s.timer); s.timer = 0; }
    s.rafId = requestAnimationFrame(this.showTick);
  }

  /**
   * Quadro da apresentação: avanço automático vencido, pintura e o próximo
   * agendamento — a cada quadro enquanto algo anima (transição, efeito,
   * mídia), um timer até a próxima mudança agendada, ou nada (parado).
   */
  private showTick = (): void => {
    const s = this.show;
    if (!s) return;
    s.rafId = 0;
    const now = this.now();
    const index = s.ss.index, step = s.ss.step;
    s.ss.update(now);
    if (s.mirror) s.rs?.update(now);
    if (s.ss.index !== index || s.ss.step !== step) { this.afterShowCommand(index, step); return; }
    this.paint();
    const media = this.slideHasMedia(s.ss.index);
    const at = media ? now : s.ss.wakeAt(now);
    if (at <= now) s.rafId = requestAnimationFrame(this.showTick);
    else if (Number.isFinite(at)) s.timer = window.setTimeout(() => { s.timer = 0; this.wakeShow(); }, at - now);
  };

  /**
   * Inicia a decodificacao de uma ImageRef e dispara um repaint quando carrega.
   * Reentrante: chamadas repetidas para o mesmo `part` reaproveitam o HTMLImageElement.
   */
  protected warmImage(ref: ImageRef, display?: { w: number; h: number }): void {
    if (this.imageCache.has(ref.part)) return;
    // WMF/EMF e TIFF: o navegador não decodifica — rasterizamos num canvas
    // (síncrono, uma vez por parte) e ele vira a imagem; `display` = tamanho na
    // página (metarquivo: escala do raster e traço mínimo de 1 px)
    const mk = (w: number, h: number): HTMLCanvasElement => { const c = document.createElement('canvas'); c.width = w; c.height = h; return c; };
    if (specialImageNeedsPrepare(ref.bytes)) {
      // EMF+ com PNG/JPEG embutido: decodifica antes (assíncrono) e só então rasteriza
      const pending = { img: mk(1, 1) as PaintImage, ready: false };
      this.imageCache.set(ref.part, pending);
      void prepareSpecialImage(ref.bytes).catch(() => undefined).then(() => {
        const c = rasterizeSpecialImage(ref.bytes, mk, display);
        if (c) pending.img = c;
        pending.ready = true;
        this.rust?.imageReady(ref.part, pending.img);
        this.paint();
        this.dispatchEvent(new CustomEvent('us-image', { detail: { part: ref.part } }));
      });
      return;
    }
    const meta = rasterizeSpecialImage(ref.bytes, mk, display);
    if (meta) {
      this.imageCache.set(ref.part, { img: meta, ready: true });
      return;
    }
    this.warmPart(ref.part, ref.mime, () => ref.bytes);
  }

  /** Decodifica uma imagem do pacote (bytes do modelo TS ou do Rust) e repinta quando carrega. */
  protected warmPart(part: string, mime: string, bytes: () => Uint8Array | null | undefined): void {
    if (this.imageCache.has(part)) return;
    const b = bytes();
    if (!b) return;
    const img = new Image();
    const entry = { img, ready: false };
    this.imageCache.set(part, entry);
    // copiamos para um Uint8Array proprio para garantir buffer estavel + tipo de Blob
    const buf = new Uint8Array(b);
    const blob = new Blob([buf], { type: mime });
    const url = URL.createObjectURL(blob);
    img.onload = () => {
      entry.ready = true; this.rust?.imageReady(part, img); this.paint(); URL.revokeObjectURL(url);
      // notifica consumidores (ex.: sidebar de miniaturas) p/ repintar.
      this.dispatchEvent(new CustomEvent('us-image', { detail: { part } }));
    };
    img.onerror = () => { URL.revokeObjectURL(url); };
    img.src = url;
  }

  /** O slide `i` tem vídeo, áudio ou GIF (o palco anima a cada quadro). */
  private slideHasMedia(i: number): boolean {
    if (this.pkg) { const slide = this.slides[i]; return !!slide?.shapes.some((sh) => sh.media || sh.image?.mime === 'image/gif'); }
    const doc = this.rustDoc();
    if (!doc || i < 0 || i >= doc.count) return false;
    try { return doc.assets(i).media.length > 0; } catch { return false; }
  }

  /**
   * Mapa `part → imagem decodificada` para o `paintSlideTo`. Só entram as
   * PRONTAS (paint pula as pendentes; onload dispara repaint). Aproveita para
   * aquecer imagens do slide que ainda não entraram no cache (path raro).
   */
  protected readyImages(slide: Slide): Map<string, PaintImage> {
    if (slide.bgImage) this.warmImage(slide.bgImage);
    for (const sh of slide.shapes) {
      if (sh.image) this.warmImage(sh.image, picDisplaySize(sh));
      if (sh.fillImage) this.warmImage(sh.fillImage);
      if (sh.table) for (const r of tableImages(sh.table)) this.warmImage(r);
      for (const r of Object.values(sh.chartImages ?? {})) this.warmImage(r);
      for (const p of sh.txBody?.paragraphs ?? []) {
        if (p.bulletImage) this.warmImage(p.bulletImage);
        for (const r of p.runs) if (r.fillImage) this.warmImage(r.fillImage);
      }
    }
    const map = new Map<string, PaintImage>();
    for (const [part, e] of this.imageCache) if (e.ready) map.set(part, e.img);
    return map;
  }

  getSlideCount(): number { return this.slideCount(); }
  getSlideIndex(): number { return this.index; }

  /**
   * Vai ao slide `i`. Na apresentação, pula para ele (entra pela transição);
   * na reprodução automática, para a reprodução; na navegação, só troca.
   */
  selectSlide(i: number): void {
    if (this.show) { this.showCommand((ss, now) => ss.goTo(i, now)); return; }
    // reprodução (em curso ou terminada no palco) para ANTES — inclusive no
    // mesmo slide / no último (antes ficava presa no palco preto)
    if (this.auto) this.stopPlayback();
    if (i < 0 || i >= this.slideCount() || i === this.index) return;
    this.index = i;
    this.paint();
    this.dispatchEvent(new CustomEvent('us-slide', { detail: { index: i } }));
  }

  /** Próximo — na apresentação, o próximo passo "ao clicar" (ou slide). */
  next(): void {
    if (this.show) this.showCommand((ss, now) => ss.next(now));
    else this.selectSlide(Math.min(this.slideCount() - 1, this.index + 1));
  }

  /** Anterior — na apresentação, desfaz o último passo (ou volta um slide). */
  prev(): void {
    if (this.show) this.showCommand((ss, now) => ss.prev(now));
    else this.selectSlide(Math.max(0, this.index - 1));
  }

  // ───────────────────────── REPRODUÇÃO AUTOMÁTICA ─────────────────────────

  /**
   * REPRODUÇÃO AUTOMÁTICA (quiosque / "usar intervalos"): a apresentação toca
   * sozinha — transição de entrada, animações (passos por clique disparam no
   * tempo do export de vídeo do app de referência), avanço por `advTm` (ou
   * `defaultSlideMs` sem intervalo), slides ocultos pulados. Pinta em modo
   * slideshow (fundo preto, slide encaixado sem margem, sem selo). O estado
   * de cada quadro é função PURA de `performance.now() − início`
   * (`playbackStateAt`) — determinístico sob relógio virtual.
   * Emite `us-slide` a cada troca e `us-playback-end` ao terminar.
   */
  play(opts: PlayOptions = {}): PlaybackTimeline {
    this.stopPresentation();
    this.stopPlayback();
    let tl: PlaybackTimeline, rtl: SlidesTimelineHandle | null;
    if (this.rustOnly()) {
      // deck só no Rust: a linha do tempo do Rust dá os quadros, o fim e os slides (o objeto devolvido tem os tempos de cada
      // slide — índice, início, fim, início das animações, transição —, não os efeitos colocados do TS)
      rtl = this.rust?.newTimeline(opts) ?? null;
      const info = rtl?.info() ?? { durationMs: 0, tickMs: 0, mediaClock: 1, slides: [] };
      tl = { slides: info.slides as unknown as PlaybackSlide[], durationMs: info.durationMs, tickMs: info.tickMs, mediaClock: info.mediaClock };
    } else {
      tl = buildPlaybackTimeline(this.slides.map((s) => s.timing), opts);
      // engine="rust": a mesma linha do tempo no Rust dá os quadros do palco (o instante é o mesmo `t`)
      rtl = this.rust?.newTimeline(opts) ?? null;
    }
    this.media.mode = opts.media ?? 'realtime';
    // export emulado (tickMs): mídia parte 2 ticks após o comando e anda no relógio das transições
    this.media.clock = tl.tickMs > 0 ? { latencyMs: 2 * tl.tickMs, scale: tl.mediaClock } : {};
    for (let i = 0; i < this.slideCount(); i++) this.warmGifsOf(i);
    const from = opts.from ?? 0;
    const first = tl.slides.find((s) => s.index >= from);
    const now = (window.performance ?? Date).now();
    this.auto = { tl, t0: now - (first?.startMs ?? 0), loop: opts.loop ?? false, rafId: 0, pos: -1, ended: false, rtl };
    this.fromKey = '';
    this.badge.style.display = 'none';
    // acabou: fica no ÚLTIMO quadro (o palco), mas não está mais "reproduzindo" —
    // qualquer navegação ou Esc volta ao modo normal
    const finish = (a: NonNullable<typeof this.auto>): void => {
      a.rafId = 0;
      a.ended = true;
      this.media.stop();
      this.dispatchEvent(new CustomEvent('us-playback-end'));
    };
    const tick = (): void => {
      const a = this.auto;
      if (!a) return;
      const t = (window.performance ?? Date).now() - a.t0;
      if (t >= a.tl.durationMs && a.loop && a.tl.durationMs > 0) a.t0 += a.tl.durationMs;
      this.paint();
      if (t >= a.tl.durationMs && (!a.loop || a.tl.durationMs <= 0)) { finish(a); return; }
      a.rafId = requestAnimationFrame(tick);
    };
    this.paint();
    // nada a tocar (todos ocultos): termina já — com `loop`, o laço girava para sempre
    if (tl.durationMs <= 0) { const a = this.auto; queueMicrotask(() => { if (this.auto === a) finish(a); }); return tl; }
    this.auto.rafId = requestAnimationFrame(tick);
    return tl;
  }

  /** Para a reprodução automática; o viewer fica no slide em cena. */
  stopPlayback(): void {
    if (!this.auto) return;
    if (this.auto.rafId) cancelAnimationFrame(this.auto.rafId);
    this.auto.rtl?.free();
    this.auto = null;
    this.media.stop();
    if (!this.controls) this.badge.style.display = '';
    this.paint();
  }

  /** Reproduzindo agora (depois do fim, o último quadro fica no palco mas isto é false). */
  isPlaying(): boolean { return this.auto !== null && !this.auto.ended; }

  /** Linha do tempo da reprodução em curso (debug/lab de playback). */
  getPlaybackTimeline(): PlaybackTimeline | null { return this.auto?.tl ?? null; }

  /** Tempo (ms) da reprodução em curso, ou null. */
  getPlaybackTime(): number | null {
    return this.auto ? (window.performance ?? Date).now() - this.auto.t0 : null;
  }

  /**
   * Modo `media: 'seek'` (captura): posiciona vídeos/GIFs do quadro atual no
   * instante exato e espera os quadros decodificados. Devolve uma assinatura
   * das posições (muda quando o quadro de mídia muda).
   */
  async syncMedia(): Promise<string> {
    if (!this.auto) return '';
    this.media.resetWanted();
    this.paint(); // quadro "seco": registra as posições pedidas
    await this.media.seekAll();
    return this.media.signature();
  }

  /** Vídeo em tela cheia tocando: cobre o slide inteiro (barras pretas se a proporção diferir). */
  private paintFullScreenVideo(ctx: CanvasRenderingContext2D, fit: { ox: number; oy: number; scale: number }): void {
    const v = this.media.fullScreen;
    const z = this.slideSize();
    if (!v || !z) return;
    const w = z.w * fit.scale, h = z.h * fit.scale;
    ctx.fillStyle = '#000';
    ctx.fillRect(fit.ox, fit.oy, w, h);
    const vw = v.videoWidth || w, vh = v.videoHeight || h;
    const k = Math.min(w / vw, h / vh);
    ctx.drawImage(v, fit.ox + (w - vw * k) / 2, fit.oy + (h - vh * k) / 2, vw * k, vh * k);
  }

  /** Canvas de trabalho do tamanho do canvas principal. */
  private workCanvas(c: HTMLCanvasElement | null): HTMLCanvasElement {
    const cv = c ?? document.createElement('canvas');
    if (cv.width !== this.canvas.width) cv.width = this.canvas.width;
    if (cv.height !== this.canvas.height) cv.height = this.canvas.height;
    return cv;
  }

  /**
   * Pinta um slide da linha do tempo num contexto, no tempo de animação
   * `animMs`. `media = false`: vídeo/GIF ficam na capa (imagem congelada).
   */
  private paintPlaybackSlide(ctx: CanvasRenderingContext2D, st: StageSlide, animMs: number,
    fit: { ox: number; oy: number; scale: number }, media = true, stage?: RustStage, which: 0 | 1 = 0): void {
    // deck só no Rust: o palco do Rust, com a mídia do modelo de leitura e os estados do Rust
    if (!this.pkg) { this.paintStageRustOnly(ctx, st.index, fit, media, stage, which); return; }
    const ps = st as PlaybackSlide;
    const slide = this.slides[ps.index];
    if (!slide || !this.pres || !ps.effects) return;
    const bg = new Set(slide.timing?.animBg ?? []);
    const actx = { animBg: (spid: string) => bg.has(spid) };
    const mediaInfo = new Map((slide.timing?.media ?? []).map((m) => [m.spid, m]));
    const bytes = (part: string): Uint8Array | undefined => this.pkg?.getPart(part);
    const hasMedia = media && slide.shapes.some((sh) => sh.media || sh.image?.mime === 'image/gif');
    this.readyImages(slide);
    if (stage && this.rust && this.paintStageRust(ctx, slide, ps, animMs, fit, hasMedia, stage, which, mediaInfo, bytes)) return;
    paintSlideTo(ctx, slide, {
      slideW: this.pres.slideW, slideH: this.pres.slideH,
      images: this.readyImages(slide), charts: this.chartCache,
    }, fit.scale, {
      ox: fit.ox, oy: fit.oy, fontStack: this.fontStackFor,
      // motor SMIL: cada forma toca os comportamentos gravados no arquivo
      shapeAnim: (sh) => (sh.inherited || !ps.effects.length ? null
        : shapeAnimHooks(ps.effects, sh.groupIds ? [sh.id, ...sh.groupIds] : [sh.id], animMs,
          sh.dgmId ? { ...actx, dgmId: sh.dgmId } : actx)),
      // vídeo tocando / quadro do GIF no lugar da capa
      ...(hasMedia ? {
        mediaFrame: (sh: Shape) => (sh.inherited ? null : this.media.frame(sh, ps, animMs, bytes, mediaInfo.get(sh.id))),
        // "ocultar enquanto não estiver tocando" (cMediaNode showWhenStopped="0")
        shapeOpacity: (sh: Shape) => {
          const mi = sh.media ? mediaInfo.get(sh.id) : undefined;
          return mi?.showWhenStopped === false && !this.media.isPlaying(sh, ps, animMs, !!mi.loop) ? 0 : 1;
        },
      } : {}),
    });
  }

  /**
   * B4b: o quadro do palco pelo motor Rust — a lista do slide com slots, os ganchos avaliados no Rust no mesmo instante da
   * máquina de estados espelhada, o pintor aplicando os valores. O player de mídia continua o do TS: cada quadro pergunta a ele
   * o vídeo/GIF de cada forma com mídia (como o `mediaFrame` do `paintSlideTo`). false = o chamador pinta pelo TS.
   */
  private paintStageRust(ctx: CanvasRenderingContext2D, slide: Slide, ps: PlaybackSlide, animMs: number, fit: { ox: number; oy: number; scale: number },
    hasMedia: boolean, stage: RustStage, which: 0 | 1, mediaInfo: Map<string, SlideMediaInfo>,
    bytes: (part: string) => Uint8Array | undefined): boolean {
    const rs = this.rust;
    if (!rs) return false;
    // the media player is asked when the painter reaches the shape's media frame (the TS painter calls its `mediaFrame` hook
    // there): a shape the frame does not draw creates no element and asks for no position
    const frameOf = (i: number): { image: PaintImage; alpha: number } | null => {
      const sh = slide.shapes[i];
      if (!sh || sh.inherited || !(sh.media || sh.image?.mime === 'image/gif')) return null;
      const mf = this.media.frame(sh, ps, animMs, bytes, mediaInfo.get(sh.id));
      return mf && 'image' in mf && 'alpha' in mf ? mf : mf ? { image: mf as PaintImage, alpha: 1 } : null;
    };
    try {
      const r = rs.paintStage(ctx, stage.h, stage.at, which, fit.scale, fit.ox, fit.oy, {
        media: hasMedia, clock: this.media.clock, mediaFrame: (i) => (hasMedia ? frameOf(i) : null),
      });
      if (r.status === 0 && !r.empty) return true;
      if (r.status === 11) {
        this.disposeRust();
        this.dispatchEvent(new CustomEvent('us-engine-error', { detail: { engine: 'rust', error: new Error('uo-slides: the engine instance stopped') } }));
      }
    } catch (error) {
      this.disposeRust();
      this.dispatchEvent(new CustomEvent('us-engine-error', { detail: { engine: 'rust', error } }));
    }
    return false;
  }

  /**
   * O quadro do palco de um deck aberto só no Rust: a lista com slots e os ganchos avaliados no Rust, e o player de mídia
   * alimentado pelo modelo de leitura (mídia da forma, entrada do modelo de tempo) e pelos estados que o Rust calcula no mesmo
   * instante (`mediaStates`, o `mediaStateAt` do TS). Sem modelo TS não há como cair no pintor TS: uma falha deixa o quadro e avisa.
   */
  private paintStageRustOnly(ctx: CanvasRenderingContext2D, index: number, fit: { ox: number; oy: number; scale: number },
    media: boolean, stage: RustStage | undefined, which: 0 | 1): void {
    const rs = this.rust, doc = this.rustDoc();
    if (!rs || !doc || !stage) return;
    let assets: SlideAssets | null = null;
    try { assets = doc.assets(index); } catch { assets = null; }
    const hasMedia = media && !!assets && assets.media.length > 0;
    try {
      // the media states of the frame (asked once, before painting); the player is asked when the painter reaches a shape's
      // media frame, as the TS painter calls its hook
      const ms = hasMedia ? doc.mediaStates(stage.h, stage.at, which, this.media.clock) : null;
      const byShape = new Map((assets?.media ?? []).map((m) => [m.shape, m]));
      const bytes = (part: string): Uint8Array | null => doc.part(part);
      const frameOf = (i: number): { image: EnginePaintImage; alpha: number } | null => {
        const m = byShape.get(i);
        if (!m || m.inherited) return null;
        let mf: MediaFrame | null = null;
        if (m.media) { const st = ms?.states.get(m.shape); if (st) mf = this.media.mediaFrame(m.media, st, bytes, m.info); }
        else if (m.gifPart) mf = this.media.gifFrame(m.gifPart, ms?.gifMs ?? 0);
        return mf && typeof mf === 'object' && 'image' in mf && 'alpha' in mf
          ? { image: mf.image as EnginePaintImage, alpha: mf.alpha } : mf ? { image: mf as EnginePaintImage, alpha: 1 } : null;
      };
      const r = rs.paintStage(ctx, stage.h, stage.at, which, fit.scale, fit.ox, fit.oy, {
        media: hasMedia, clock: this.media.clock, mediaFrame: (i) => (hasMedia ? frameOf(i) : null),
      });
      if (r.status === 11) this.rustFailed(new Error('uo-slides: the engine instance stopped'));
    } catch (error) {
      this.rustFailed(error);
    }
  }

  /** Quadro da reprodução automática: o instante atual da linha do tempo, no palco. */
  protected paintPlayback(ctx: CanvasRenderingContext2D): void {
    const a = this.auto!;
    const t = this.now() - a.t0;
    if (!this.pkg) {
      // deck só no Rust: o estado da linha do tempo do Rust (o mesmo `playbackStateAt`)
      if (!a.rtl) { this.paintStage(ctx, null, 0, 1, null); return; }
      const st = a.rtl.stateAt(t);
      if (st.index >= 0 && st.pos !== a.pos) {
        a.pos = st.pos;
        this.index = st.index;
        const detail = { index: st.index };
        queueMicrotask(() => this.dispatchEvent(new CustomEvent('us-slide', { detail })));
      }
      this.paintStage(ctx, st.index >= 0 ? { index: st.index, transition: st.transition } : null, st.animMs, st.transitionProgress,
        st.from >= 0 ? { slide: { index: st.from, transition: null }, animMs: Infinity, key: `a${st.from}`, media: true } : null, { h: a.rtl, at: t });
      return;
    }
    const st = this.pres ? playbackStateAt(a.tl, t) : null;
    const rstage: RustStage | undefined = a.rtl ? { h: a.rtl, at: t } : undefined;
    if (st?.slide && st.pos !== a.pos) {
      a.pos = st.pos;
      this.index = st.slide.index;
      // fora da pintura: um ouvinte que pare a reprodução não tem o quadro repintado por cima
      const detail = { index: st.slide.index };
      queueMicrotask(() => this.dispatchEvent(new CustomEvent('us-slide', { detail })));
    }
    // origem da transição: o anterior no estado final (preto no 1º)
    this.paintStage(ctx, st?.slide ?? null, st?.animMs ?? 0, st?.transitionProgress ?? 1,
      st?.from ? { slide: st.from, animMs: Infinity, key: `a${st.from.index}`, media: true } : null, rstage);
  }

  /** Quadro da apresentação, no palco; na tela de fim, o aviso sobre o preto. */
  protected paintShow(ctx: CanvasRenderingContext2D): void {
    const s = this.show!;
    const now = this.now();
    if (s.ss instanceof RustShowMachine) {
      // deck só no Rust: o quadro da apresentação do Rust
      const rf = s.ss.frame(now);
      const ro = rf.from;
      this.paintStage(ctx, rf.index >= 0 && !rf.ended ? { index: rf.index, transition: rf.transition } : null, rf.animMs, rf.transitionProgress,
        ro ? { slide: { index: ro.index, transition: null }, animMs: ro.animMs, key: `p${ro.key}`, media: false } : null, { h: s.ss.h, at: now });
      if (rf.ended) this.paintEndOfShow(ctx);
      return;
    }
    const f = (s.ss as SlideShow).frame(now);
    const o = f.from;
    // origem congelada: vídeo/GIF na capa (não volta a tocar o som do slide que saiu)
    this.paintStage(ctx, f.slide, f.animMs, f.transitionProgress,
      o ? { slide: o.slide, animMs: o.animMs, key: `p${o.key}`, media: false } : null, s.rs ? { h: s.rs, at: now } : undefined);
    if (!f.ended) return;
    this.paintEndOfShow(ctx);
  }

  /** O aviso da tela de fim da apresentação, sobre o preto. */
  private paintEndOfShow(ctx: CanvasRenderingContext2D): void {
    // medido: aviso de interface pequeno (texto de 11 px numa tela de 1080), no topo, centralizado
    const W = this.canvas.clientWidth;
    ctx.save();
    ctx.fillStyle = '#fff';
    ctx.font = '600 12px "Segoe UI", Arial, sans-serif';
    ctx.textAlign = 'center';
    ctx.textBaseline = 'top';
    ctx.fillText(this.endOfShowText, W / 2, 5);
    ctx.restore();
    this.media.syncRealtime(); // nada pedido neste quadro: pausa o que ainda tocava
  }

  /**
   * PALCO da apresentação e da reprodução: fundo preto, slide encaixado sem
   * margem, no tempo de animação `animMs`. Durante a transição compõe ORIGEM
   * (`from`; null = preto) × DESTINO — `thruBlk` passa pelo preto; corte seco
   * é instantâneo (progresso 1). `from.key` identifica a imagem de origem
   * (pintada uma vez por passagem).
   */
  private paintStage(ctx: CanvasRenderingContext2D, slide: StageSlide | null, animMs: number, progress: number,
    from: { slide: StageSlide; animMs: number; key: string; media: boolean } | null, rstage?: RustStage): void {
    const dpr = window.devicePixelRatio || 1;
    const W = this.canvas.clientWidth, H = this.canvas.clientHeight;
    ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
    ctx.fillStyle = '#000'; ctx.fillRect(0, 0, W, H);
    const fit = this.fit();
    const z = this.slideSize();
    if (!slide || !fit || !z) return;
    this.media.fullScreen = null;
    if (progress >= 1) {
      this.paintPlaybackSlide(ctx, slide, animMs, fit, true, rstage, 0);
      this.paintFullScreenVideo(ctx, fit);
      if (this.media.mode === 'realtime') this.media.syncRealtime();
      return;
    }

    // transição em curso: ORIGEM × DESTINO
    const to = this.toCanvas = this.workCanvas(this.toCanvas);
    const tctx = to.getContext('2d')!;
    tctx.setTransform(dpr, 0, 0, dpr, 0, 0);
    tctx.fillStyle = '#000'; tctx.fillRect(0, 0, W, H);
    this.paintPlaybackSlide(tctx, slide, animMs, fit, true, rstage, 0);
    const fromCv = this.fromCanvas = this.workCanvas(this.fromCanvas);
    const key = `${from?.key ?? 'preto'}:${fromCv.width}x${fromCv.height}`;
    if (key !== this.fromKey) {
      const fctx = fromCv.getContext('2d')!;
      fctx.setTransform(dpr, 0, 0, dpr, 0, 0);
      fctx.fillStyle = '#000'; fctx.fillRect(0, 0, W, H);
      if (from) this.paintPlaybackSlide(fctx, from.slide, from.animMs, fit, from.media, rstage, 1);
      this.fromKey = key;
    }
    ctx.save();
    ctx.setTransform(1, 0, 0, 1, 0, 0);
    const tr = slide.transition ?? { type: 'fade', ns: 'p', attrs: {}, durMs: 0, advClick: true };
    // retângulo do slide em px de canvas (o resto é letterbox preto)
    const rect = {
      x: fit.ox * dpr, y: fit.oy * dpr,
      w: z.w * fit.scale * dpr, h: z.h * fit.scale * dpr,
    };
    paintTransition(ctx, fromCv, to, tr, progress, rect, this.canvas.width, this.canvas.height);
    ctx.restore();
    if (this.media.mode === 'realtime') this.media.syncRealtime();
  }

  /**
   * Pinta o slide `i` num canvas externo (uso: miniaturas da sidebar). Usa o
   * MESMO `paintSlideTo` — uma so fonte de verdade entre slide principal
   * e thumbnail. Retorna `false` se a imagem do slide ainda nao decodificou —
   * o chamador pode escutar `us-loaded`/`us-image` para repintar.
   */
  renderThumbnail(i: number, target: HTMLCanvasElement): boolean {
    const z = this.slideSize();
    if (!z || i < 0 || i >= this.slideCount()) return false;
    const ctx = target.getContext('2d');
    if (!ctx) return false;
    const dpr = window.devicePixelRatio || 1;
    const cssW = target.clientWidth || 200;
    const cssH = target.clientHeight || Math.round(cssW * z.h / z.w);
    if (target.width !== Math.round(cssW * dpr)) target.width = Math.round(cssW * dpr);
    if (target.height !== Math.round(cssH * dpr)) target.height = Math.round(cssH * dpr);
    ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
    ctx.clearRect(0, 0, cssW, cssH);
    const scale = Math.min(cssW / z.w, cssH / z.h);
    const sw = z.w * scale, sh = z.h * scale;
    const ox = (cssW - sw) / 2, oy = (cssH - sh) / 2;
    this.paintStill(ctx, i, scale, { ox, oy, fontStack: this.fontStackFor });
    return true;
  }

  /**
   * Pinta o slide `i` num canvas externo em TAMANHO EXATO: o canvas é
   * redimensionado para `pxWidth × (proporção do slide)` e o slide preenche
   * 100% da área (sem margens/chrome). Uso: export, comparação 1:1 com o
   * app de referência, impressão.
   */
  renderSlideToCanvas(i: number, target: HTMLCanvasElement, pxWidth = 1280): boolean {
    const z = this.slideSize();
    if (!z || i < 0 || i >= this.slideCount()) return false;
    const ctx = target.getContext('2d');
    if (!ctx) return false;
    const pxHeight = Math.round(pxWidth * z.h / z.w);
    if (target.width !== pxWidth) target.width = pxWidth;
    if (target.height !== pxHeight) target.height = pxHeight;
    ctx.setTransform(1, 0, 0, 1, 0, 0);
    ctx.clearRect(0, 0, pxWidth, pxHeight);
    this.paintStill(ctx, i, pxWidth / z.w, { fontStack: this.fontStackFor });
    return true;
  }

  protected onKeyDown = (e: KeyboardEvent): void => {
    if (this.isEditing()) return; // teclado pertence ao editor da subclasse
    if (this.show) {
      if (e.ctrlKey || e.metaKey || e.altKey) return; // atalhos do navegador/host
      const k = e.key;
      if (k === 'Escape') this.stopPresentation();
      else if (k === 'F5') { /* já apresentando: não recarrega a página */ }
      else if (this.show.kiosk) return; // quiosque: só intervalos/hiperlinks avançam
      else if (NEXT_KEYS.has(k)) this.next();
      else if (PREV_KEYS.has(k)) this.prev();
      else if (k === 'Home') this.showCommand((ss, now) => ss.first(now));
      else if (k === 'End') this.showCommand((ss, now) => ss.last(now));
      else return;
      e.preventDefault();
      return;
    }
    if (this.auto && e.key === 'Escape') { this.stopPlayback(); e.preventDefault(); return; }
    if ((e.key === 'f' || e.key === 'F') && !e.ctrlKey && !e.metaKey && !e.altKey) { this.toggleFullscreen(); e.preventDefault(); return; }
    if (e.key === 'ArrowRight' || e.key === 'PageDown' || e.key === ' ') { this.next(); e.preventDefault(); }
    else if (e.key === 'ArrowLeft' || e.key === 'PageUp') { this.prev(); e.preventDefault(); }
    else if (e.key === 'Home') { this.selectSlide(0); e.preventDefault(); }
    else if (e.key === 'End') { this.selectSlide(this.slideCount() - 1); e.preventDefault(); }
  };

  /**
   * Detecta hyperlink na posição (canvas-relativa): abre o endereço, ou (motor Rust) salta ao slide / à posição da apresentação.
   * v1 do TS: o link é "shape-wide" (a primeira run com hyperlink no txBody manda).
   */
  protected onClick = (e: MouseEvent): void => {
    if (this.isEditing()) return;
    const hit = this.hitTestLink(e);
    if (hit) {
      if ('url' in hit) {
        // engine="rust": só http, https e mailto abrem (a regra do viewer do Write); `link-schemes="any"` = a do motor TS
        if (this.engineAttr() !== 'rust' || this.getAttribute('link-schemes') === 'any') window.open(hit.url, '_blank', 'noopener');
        else if (SAFE_LINK.test(hit.url.trim())) window.open(hit.url.trim(), '_blank', 'noopener,noreferrer');
      } else this.followJump(hit);
      e.preventDefault();
      return;
    }
    // apresentação: o clique avança (passo ou slide) — no quiosque, não
    if (this.show && !this.show.kiosk && e.button === 0) this.showCommand((ss, now) => ss.next(now, true));
  };

  protected onMouseMove = (e: MouseEvent): void => {
    if (this.isEditing()) return;
    this.canvas.style.cursor = this.hitTestLink(e) ? 'pointer' : 'default';
  };

  /** Segue um salto (motor Rust: `ppaction://hlinksldjump` / `hlinkshowjump`): na apresentação pelo `goTo`, fora dela trocando o slide. */
  private followJump(t: { slide: number } | { show: 'next' | 'previous' | 'first' | 'last' | 'end' }): void {
    const n = this.slideCount();
    const cur = this.show ? this.show.ss.index : this.index;
    let to: number;
    if ('slide' in t) to = t.slide;
    else if (t.show === 'end') { if (this.show) this.stopPresentation(); return; }
    else to = t.show === 'next' ? cur + 1 : t.show === 'previous' ? cur - 1 : t.show === 'first' ? 0 : n - 1;
    if (to < 0 || to >= n) return;
    this.selectSlide(to);
  }

  /** Devolve a primeira URL de hyperlink sob (clientX, clientY) ou null. */
  protected hitTestHyperlink(e: MouseEvent): string | null {
    const h = this.hitTestLink(e);
    return h && 'url' in h ? h.url : null;
  }

  /**
   * O hiperlink sob o ponteiro. Motor Rust: as caixas do modelo de leitura (`SlidesDocument.links`: a mesma geometria do TS para o
   * endereço da run, mais o clique da forma e os saltos), da forma de cima para baixo, na forma o endereço da run primeiro. Motor
   * TS: o laço do modelo TS.
   */
  protected hitTestLink(e: MouseEvent): LinkHit | null {
    if (this.show?.ss.ended) return null; // tela de fim: nada clicável
    const fit = this.fit();
    if (!fit || this.index < 0 || this.index >= this.slideCount()) return null;
    const rect = this.canvas.getBoundingClientRect();
    const x = e.clientX - rect.left;
    const y = e.clientY - rect.top;
    const doc = this.rustDoc();
    if (doc) {
      let boxes: LinkBoxWire[];
      try { boxes = doc.links(this.index, fit.scale, fit.ox, fit.oy); } catch { boxes = []; }
      let hit: LinkBoxWire | null = null;
      for (let k = boxes.length - 1; k >= 0; k--) {
        const b = boxes[k]!;
        if (hit && b.shape !== hit.shape) break;
        if (x < b.x || x > b.x + b.w || y < b.y || y > b.y + b.h) continue;
        const t = b.target;
        if ('show' in t && (t.show === 'lastViewed' || (t.show === 'end' && !this.show))) continue;
        hit = b; // a mais cedo da forma (run, depois clique, depois salto) fica
      }
      if (!hit) return null;
      const t = hit.target;
      return 'url' in t ? { url: t.url } : 'slide' in t ? { slide: t.slide } : { show: t.show as 'next' | 'previous' | 'first' | 'last' | 'end' };
    }
    const slide = this.slides[this.index];
    if (!slide) return null;
    for (let i = slide.shapes.length - 1; i >= 0; i--) {
      const sh = slide.shapes[i]!;
      if (!sh.xfrm || !sh.txBody) continue;
      const sx = fit.ox + sh.xfrm.x * fit.scale;
      const sy = fit.oy + sh.xfrm.y * fit.scale;
      const sw = sh.xfrm.cx * fit.scale;
      const sh2 = sh.xfrm.cy * fit.scale;
      if (x < sx || x > sx + sw || y < sy || y > sy + sh2) continue;
      for (const p of sh.txBody.paragraphs) {
        for (const r of p.runs) {
          if (r.hyperlink) return { url: r.hyperlink };
        }
      }
    }
    return null;
  }

  protected resize(): void {
    const dpr = window.devicePixelRatio || 1;
    const w = this.canvas.clientWidth || 800, h = this.canvas.clientHeight || 600;
    this.canvas.width = Math.round(w * dpr);
    this.canvas.height = Math.round(h * dpr);
  }

  /**
   * Geometria de encaixe do slide no canvas (origem + escala EMU→px). No
   * palco (apresentação/reprodução) o slide encosta nas bordas.
   */
  protected fit(): { ox: number; oy: number; scale: number; sw: number; sh: number } | null {
    const z = this.slideSize();
    if (!z) return null;
    const W = this.canvas.clientWidth, H = this.canvas.clientHeight;
    const margin = this.auto || this.show || this.controlsBuiltIn() ? 0 : 16;
    const aw = W - margin * 2, ah = H - margin * 2;
    if (aw <= 0 || ah <= 0) return null; // sem área (oculto/desmontado): nada a pintar
    const scale = Math.min(aw / z.w, ah / z.h);
    const sw = z.w * scale, sh = z.h * scale;
    return { ox: (W - sw) / 2, oy: (H - sh) / 2, scale, sw, sh };
  }

  protected paint(): void {
    this.paintFrame();
    this.controls?.sync();
    this.a11y?.show(this.slideCount(), this.index, (i) => this.a11yShapes(i), true);
  }

  /** As formas do slide `i` para o texto acessível (modelo TS ou o modelo de leitura do Rust). */
  private a11yShapes(i: number): A11yShape[] | undefined {
    if (this.pkg) { const s = this.slides[i]; return s ? a11yShapesOf(s) : undefined; }
    try { return this.rustDoc()?.outline(i); } catch { return undefined; }
  }

  /** Nome do arquivo (do `src`), para o leitor de tela e o download. */
  private fileName(): string | null {
    const src = this.getAttribute('src');
    try { return src ? decodeURIComponent(src.split(/[?#]/)[0]!.split('/').pop() ?? '') || null : null; } catch { return null; }
  }

  private paintFrame(): void {
    const ctx = this.canvas.getContext('2d');
    if (!ctx) return;
    if (this.auto) { this.paintPlayback(ctx); return; }
    if (this.show) { this.paintShow(ctx); return; }
    // NAVEGAÇÃO: o slide parado, como foi desenhado (sem animação nem transição)
    const dpr = window.devicePixelRatio || 1;
    ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
    const W = this.canvas.clientWidth, H = this.canvas.clientHeight;
    // editor: moldura cinza com o slide destacado; viewer: fundo do elemento (--upv-background)
    if (this.controlsBuiltIn()) ctx.clearRect(0, 0, W, H);
    else { ctx.fillStyle = this.deskColor(); ctx.fillRect(0, 0, W, H); }

    const fit = this.fit();
    if (!fit || this.index < 0 || this.index >= this.slideCount()) {
      ctx.fillStyle = '#bbb'; ctx.font = '14px Segoe UI, Arial';
      ctx.fillText(pickLabels(this, TEXT).empty, 20, 30);
      this.badge.textContent = '';
      return;
    }

    // conteúdo do slide (fundo + shapes) — função pura compartilhada
    const paintOpts: PaintSlideOptions = { ox: fit.ox, oy: fit.oy, fontStack: this.fontStackFor };
    const suppress = this.suppressTextIdx();
    if (suppress !== undefined) paintOpts.suppressTextShapeIdx = suppress;
    this.paintStill(ctx, this.index, fit.scale, paintOpts);

    // sombra/borda do slide (só na moldura do editor)
    if (!this.controlsBuiltIn()) {
      ctx.strokeStyle = 'rgba(0,0,0,.35)'; ctx.lineWidth = 1;
      ctx.strokeRect(fit.ox + 0.5, fit.oy + 0.5, fit.sw - 1, fit.sh - 1);
    }

    this.badge.textContent = `${this.index + 1} / ${this.slideCount()}`;
  }
}

customElements.define('universal-slides-viewer', UniversalSlidesViewerElement);
