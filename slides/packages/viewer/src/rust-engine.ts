import {
  loadSlidesModule, slidesHandles,
  type LoadSlidesOptions, type PaintImage, type SlidePaintResult, type SlidesDocument, type SlidesPaintHost,
} from '@universal-office/slides-engine';
import { lightTable, lightAt, lightFace, bevelPhi, patternRows } from '@universal-office/drawingml';

/**
 * `engine="rust"` do <universal-slides-viewer>: o lado do viewer da troca TS/Rust (registro em core/docs/modules/uo-bindings.md
 * "Slides viewer (B7)"). Carregado SÓ por `import()` dinâmico quando o atributo pede o motor Rust — um viewer sem `engine="rust"`
 * nunca baixa este módulo nem o .wasm.
 *
 * Um `RustSlides` por elemento: o deck aberto no módulo (uo-slides.wasm). O layout e a lista de desenho do slide são do Rust; as
 * medidas de texto (`measureText`), a decodificação de imagens, as fontes e o canvas continuam no navegador — o mesmo resolvedor
 * de fontes e o mesmo cache de imagens do motor TS. Quem NÃO é do Rust ainda (animações, apresentação e reprodução, hiperlinks,
 * texto acessível, gráficos) segue no motor TS: o viewer mantém o modelo TS carregado para isso (registro, "Gaps").
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
}

let measureCtx: CanvasRenderingContext2D | OffscreenCanvasRenderingContext2D | null = null;
function scratchContext(): CanvasRenderingContext2D | OffscreenCanvasRenderingContext2D {
  if (measureCtx) return measureCtx;
  const c = typeof OffscreenCanvas === 'function' ? new OffscreenCanvas(1, 1) : document.createElement('canvas');
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
    };
  }

  private source(): LoadSlidesOptions {
    const s = this.hostApi.source();
    return { ...s, wasm: s?.wasm ?? new URL(WASM_FILE, import.meta.url), ppt: s?.ppt ?? new URL(PPT_FILE, import.meta.url) };
  }

  /** Abre o deck (`.pptx`; um `.ppt` passa antes pelo conversor, módulo à parte). Lança `SlidesError`; o deck anterior fica. */
  async open(bytes: Uint8Array): Promise<SlidesDocument> {
    const mod = await loadSlidesModule(this.source());
    const doc = await mod.open(bytes);
    if (this.disposed) { doc.free(); throw new Error('uo-slides: the viewer was disconnected'); }
    this.doc?.free();
    this.doc = doc;
    return doc;
  }

  get document(): SlidesDocument | null { return this.doc; }
  get count(): number { return this.doc?.count ?? 0; }

  /** Uma fonte carregou (ou a pilha de uma família mudou): as medidas são pedidas de novo. */
  fontsChanged(): void { this.doc?.fontsChanged(); }

  /** A imagem de `part` terminou de decodificar: o layout que a viu 0 x 0 aprende o tamanho. */
  imageReady(part: string, img: PaintImage): void { this.doc?.imageReady(part, img); }

  notes(i: number): string { return this.doc?.notes(i) ?? ''; }

  /** Pinta o slide `i` (índice do viewer); `status` 0 = pintou, outro = o chamador cai no motor TS. */
  paint(ctx: CanvasRenderingContext2D, i: number, scale: number, ox: number, oy: number): SlidePaintResult {
    if (!this.doc || this.doc.freed) return { status: 11, charts: 0, rounds: 0 };
    // gráficos: a lista do Rust os pinta (uo-charts, C-6): nenhum slide fica com o motor TS por causa deles
    return this.doc.paint(ctx, i, scale, ox, oy, this.paintHost);
  }

  /** Handles vivos do módulo (testes de memória: 0 depois de fechar tudo). */
  static handles(): number { return slidesHandles(); }

  dispose(): void {
    this.disposed = true;
    this.doc?.free();
    this.doc = null;
  }
}
