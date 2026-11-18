import {
  loadSlidesModule, currentSlidesModule, slidesHandles,
  type LoadSlidesOptions, type PaintImage, type SlidePaintResult, type SlidesDocument, type SlidesPaintHost,
  type SlidesShowHandle, type SlidesTimelineHandle, type StagePaintOptions,
} from '@universal-office/slides-engine';
import { lightTable, lightAt, lightFace, bevelPhi, patternRows } from '@universal-office/drawingml';

/**
 * `engine="rust"` do <universal-slides-viewer>: o lado do viewer da troca TS/Rust (registro em core/docs/modules/uo-bindings.md
 * "Slides viewer (B7)"). Carregado SÓ por `import()` dinâmico quando o atributo pede o motor Rust — um viewer sem `engine="rust"`
 * nunca baixa este módulo nem o .wasm.
 *
 * Um `RustSlides` por elemento: o deck aberto no módulo (uo-slides.wasm). O layout e a lista de desenho do slide são do Rust; as
 * medidas de texto (`measureText`), a decodificação de imagens, as fontes e o canvas continuam no navegador — o mesmo resolvedor
 * de fontes e o mesmo cache de imagens do motor TS. Animações, apresentação e reprodução (B4b): a lista do slide em cena tem
 * slots, o Rust avalia os ganchos de animação a cada quadro (`paintStage`) e o pintor aplica os valores. Desde a onda 4 o deck
 * abre só no Rust quando o módulo já está instanciado (`preloadSlidesEngine` + `openSync`): o modelo de leitura do documento
 * (`outline`, `links`, `assets`, `mediaStates`, `fonts`, `typefaces`, `showSettings`) substitui o modelo TS, e as máquinas de
 * estado do Rust dão os eventos (registro, "Wave 4").
 */

/** O .wasm do motor e o conversor de .ppt (carregado só ao abrir um .ppt): ao lado deste módulo no pacote publicado. */
const WASM_FILE = 'uo-slides.wasm';
const PPT_FILE = 'uo-slides-ppt.wasm';

/** O que o elemento passa à sessão. */
export interface RustSlidesHost {
  /** a pilha CSS de fontes de uma família (a do viewer: embutida no deck, ou a do resolvedor) */
  fontStack(family: string, bold: boolean, italic: boolean): string;
  /** a imagem decodificada de uma parte do pacote, ou null se ainda decodifica */
  image(part: string): PaintImage | null;
  /** o .wasm configurado (`configureEngine`); sem ele, o do pacote */
  source(): LoadSlidesOptions | undefined;
  /** uma imagem que o layout esperava (bitmap comprimido de EMF+, a fase de preparo) terminou de decodificar: repintar */
  prepared?(): void;
}

/** O .wasm de uma fonte configurada (ou o do pacote, ao lado deste módulo). */
function withDefaults(s: LoadSlidesOptions | undefined): LoadSlidesOptions {
  return { ...s, wasm: s?.wasm ?? new URL(WASM_FILE, import.meta.url), ppt: s?.ppt ?? new URL(PPT_FILE, import.meta.url) };
}

/** Instancia o módulo antes de haver um deck (o viewer com `engine="rust"` chama ao conectar): a abertura fica síncrona. */
export async function preloadSlidesEngine(source: LoadSlidesOptions | undefined): Promise<void> {
  await loadSlidesModule(withDefaults(source));
}

/** O módulo já está instanciado (`openSync` abre sem esperar). */
export function slidesEngineReady(): boolean { return currentSlidesModule(withDefaults(undefined)) !== null; }

/** Janela depois de uma troca de fontes em que o motor Rust mede a cada pintura (ver `RustSlides.fontSettleUntil`). */
const FONT_SETTLE_MS = 1000;
const now = (): number => (typeof performance !== 'undefined' ? performance.now() : Date.now());

let measureCtx: CanvasRenderingContext2D | OffscreenCanvasRenderingContext2D | null = null;
function scratchContext(): CanvasRenderingContext2D | OffscreenCanvasRenderingContext2D {
  if (measureCtx) return measureCtx;
  // a DOM canvas, as the TS engine measures on the canvases it paints (the page's font set, applied as on those); OffscreenCanvas only
  // where there is no document
  const c = typeof document !== 'undefined' ? document.createElement('canvas') : new OffscreenCanvas(1, 1);
  const ctx = c.getContext('2d');
  if (!ctx) throw new Error('uo-slides: no 2d context to measure text');
  measureCtx = ctx as CanvasRenderingContext2D;
  return measureCtx;
}

export class RustSlides {
  private doc: SlidesDocument | null = null;
  private readonly hostApi: RustSlidesHost;
  private disposed = false;
  private readonly paintHost: SlidesPaintHost;

  constructor(host: RustSlidesHost) {
    this.hostApi = host;
    this.paintHost = {
      fontStack: (f, b, i) => host.fontStack(f, b, i),
      image: (part) => host.image(part),
      get measureCtx() { return scratchContext(); },
      extra: { light: { lightTable, lightAt, lightFace, bevelPhi }, patternRows },
      prepared: () => host.prepared?.(),
    };
  }

  private source(): LoadSlidesOptions { return withDefaults(this.hostApi.source()); }

  /**
   * Abre um `.pptx` agora, com o módulo já instanciado: o documento, ou null quando o módulo ainda não carregou ou o arquivo é um
   * `.ppt` (o conversor é assíncrono). Lança `SlidesError` para um arquivo inválido; o deck anterior fica.
   */
  openSync(bytes: Uint8Array): SlidesDocument | null {
    const mod = currentSlidesModule(this.source());
    if (!mod || (bytes.length > 3 && bytes[0] === 0xd0 && bytes[1] === 0xcf && bytes[2] === 0x11 && bytes[3] === 0xe0)) return null;
    const doc = mod.openSync(bytes);
    this.doc?.free();
    this.doc = doc;
    this.prepareSpecials();
    return doc;
  }

  /** Abre o deck (`.pptx`; um `.ppt` passa antes pelo conversor, módulo à parte). Lança `SlidesError`; o deck anterior fica. */
  async open(bytes: Uint8Array): Promise<SlidesDocument> {
    const mod = await loadSlidesModule(this.source());
    const doc = await mod.open(bytes);
    if (this.disposed) { doc.free(); throw new Error('uo-slides: the viewer was disconnected'); }
    this.doc?.free();
    this.doc = doc;
    this.prepareSpecials();
    return doc;
  }

  /** Toma um documento já aberto (o editor abre o deck no módulo e o compartilha); o anterior é liberado. */
  adopt(doc: SlidesDocument): void {
    if (this.doc === doc) return;
    this.doc?.free();
    this.doc = doc;
    this.prepareSpecials();
  }

  /**
   * As the TS viewer's `warmImage` at open (`prepareSpecialImage` of every EMF+ picture with a compressed PNG/JPEG): their
   * decode starts now, for every slide, so a paint finds them ready instead of waiting (`PENDING`) for them.
   */
  private prepareSpecials(): void {
    const doc = this.doc;
    if (!doc || doc.freed) return;
    const seen = new Set<string>();
    try {
      for (let i = 0; i < doc.count; i++) {
        for (const im of doc.assets(i).images) if (im.special && !seen.has(im.part)) { seen.add(im.part); doc.prepareSpecial(im.part, this.paintHost); }
      }
    } catch { /* a slide that cannot be read: its paint reports it */ }
  }

  get document(): SlidesDocument | null { return this.doc; }
  get count(): number { return this.doc?.count ?? 0; }

  /** Uma fonte carregou (ou a pilha de uma família mudou): as medidas são pedidas de novo. */
  fontsChanged(): void {
    // a fresh measuring context: one that measured a font string before the face arrived must not answer from its cached face
    measureCtx = null;
    this.doc?.fontsChanged();
    this.fontSettleUntil = now() + FONT_SETTLE_MS;
  }

  /**
   * Até quando as medidas são pedidas de novo a CADA pintura (como o TS, que mede ao pintar). Uma face posta no `document.fonts`
   * pode chegar ao texto do canvas só depois (o Safari a aplica numa atualização de renderização posterior, sem evento): medidas
   * guardadas nesse intervalo seriam as da substituta, e o texto sairia disposto com as larguras dela e desenhado com a certa.
   */
  private fontSettleUntil = 0;
  private settleFonts(): void {
    if (this.fontSettleUntil && now() < this.fontSettleUntil) this.doc?.fontsChanged();
  }

  /** A imagem de `part` terminou de decodificar: o layout que a viu 0 x 0 aprende o tamanho. */
  imageReady(part: string, img: PaintImage): void { this.doc?.imageReady(part, img); }

  notes(i: number): string { return this.doc?.notes(i) ?? ''; }

  /** Pinta o slide `i` (índice do viewer); `status` 0 = pintou, outro = o chamador cai no motor TS. */
  paint(ctx: CanvasRenderingContext2D, i: number, scale: number, ox: number, oy: number, suppress?: number): SlidePaintResult {
    if (!this.doc || this.doc.freed) return { status: 11, charts: 0, rounds: 0 };
    // gráficos: a lista do Rust os pinta (uo-charts, C-6): nenhum slide fica com o motor TS por causa deles;
    // `suppress` = a forma cujo texto a edição inline cobre (fica fora da lista)
    this.settleFonts();
    return this.doc.paint(ctx, i, scale, ox, oy, this.paintHost, suppress !== undefined ? { suppressText: suppress } : {});
  }

  /** A apresentação espelhada no Rust (mesmas opções e mesmos comandos da `SlideShow` do TS); null sem deck aberto. */
  newShow(o: { from: number; now: number; useTimings: boolean; loop: boolean; animations: boolean; range: [number, number] | null }): SlidesShowHandle | null {
    if (!this.doc || this.doc.freed) return null;
    const b = (v: boolean): number => (v ? 1 : 0);
    return this.doc.newShow({ from: o.from, now: o.now, useTimings: b(o.useTimings), loop: b(o.loop), animations: b(o.animations), range: o.range });
  }

  /** A linha do tempo da reprodução espelhada no Rust (as opções de `buildPlaybackTimeline`); null sem deck aberto. */
  newTimeline(o: { useTimings?: boolean; defaultSlideMs?: number; tickMs?: number; transitionFps?: number }): SlidesTimelineHandle | null {
    if (!this.doc || this.doc.freed) return null;
    return this.doc.newTimeline({
      useTimings: o.useTimings === undefined ? -1 : o.useTimings ? 1 : 0,
      defaultSlideMs: o.defaultSlideMs ?? NaN, tickMs: o.tickMs ?? NaN, transitionFps: o.transitionFps ?? NaN,
    });
  }

  /** Pinta um quadro do palco (`which` 0 = slide em cena, 1 = origem da transição); `status` 0 = pintou, outro = cair no TS. */
  paintStage(ctx: CanvasRenderingContext2D, src: SlidesShowHandle | SlidesTimelineHandle, at: number, which: 0 | 1, scale: number, ox: number, oy: number, o: StagePaintOptions): SlidePaintResult {
    if (!this.doc || this.doc.freed) return { status: 11, charts: 0, rounds: 0 };
    if (which === 0) this.settleFonts();
    return this.doc.paintStage(ctx, src, at, which, scale, ox, oy, this.paintHost, o);
  }

  /** Handles vivos do módulo (testes de memória: 0 depois de fechar tudo). */
  static handles(): number { return slidesHandles(); }

  dispose(): void {
    this.disposed = true;
    this.doc?.free();
    this.doc = null;
  }
}
