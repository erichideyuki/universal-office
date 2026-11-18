import type { Slide, Shape } from '@universal-office/slides-core';
import { LiveRegion, SR_ONLY } from '@universal-office/ui';
import { pickLabels, type UiLang } from '@universal-office/ui';

/**
 * Acessibilidade do <universal-slides-viewer>: o slide é desenhado num
 * canvas, então o leitor de tela recebe uma versão em TEXTO do slide em cena
 * (visualmente oculta): o título como heading, os textos das formas na ordem
 * de leitura do PowerPoint (a ordem das formas), tabelas e o texto alternativo
 * das imagens/formas (cNvPr/@descr). O canvas se apresenta como "Slide N de M:
 * título" e cada troca de slide é anunciada (aria-live).
 */

const LABELS = {
  pt: {
    slide: (n: number, m: number) => `Slide ${n} de ${m}`, untitled: 'Sem título',
    presentation: 'Apresentação', content: 'Conteúdo do slide', roledesc: 'slide', image: 'Imagem',
  },
  es: {
    slide: (n: number, m: number) => `Diapositiva ${n} de ${m}`, untitled: 'Sin título',
    presentation: 'Presentación', content: 'Contenido de la diapositiva', roledesc: 'diapositiva', image: 'Imagen',
  },
  en: {
    slide: (n: number, m: number) => `Slide ${n} of ${m}`, untitled: 'Untitled',
    presentation: 'Presentation', content: 'Slide content', roledesc: 'slide', image: 'Image',
  },
};
type Labels = typeof LABELS.en;

const TITLE_PH = new Set(['title', 'ctrTitle']);
/** Placeholders de rodapé (data, número do slide, rodapé): o leitor não precisa deles a cada slide. */
const SKIP_PH = new Set(['sldNum', 'dt', 'ftr', 'hdr']);

/** Texto alternativo que é só o nome do arquivo (geradores gravam isso): não descreve nada. */
const usefulAlt = (d: string | undefined): string | undefined =>
  d && !/^[\w .()-]+\.(png|jpe?g|gif|svg|emf|wmf|bmp|tiff?|webp)$/i.test(d.trim()) ? d.trim() : undefined;

/** O texto de um parágrafo como o leitor o recebe: os textos das corridas juntos, espaços normalizados. */
const norm = (t: string): string => t.replace(/\s+/g, ' ').trim();
const joinRuns = (p: { runs: { text: string }[] }): string => p.runs.map((r) => r.text).join('');

/**
 * O que o texto acessível lê de cada forma (a mesma coisa do modelo TS e do modelo de leitura do motor Rust,
 * `SlidesDocument.outline`): os parágrafos são o texto das corridas juntas, sem normalizar.
 */
export interface A11yShape {
  inherited: boolean;
  phType: string | undefined;
  descr: string | undefined;
  image: boolean;
  media: boolean;
  /** a forma tem corpo de texto (`txBody`) */
  hasText: boolean;
  paragraphs: string[];
  table: { hMerge: boolean; vMerge: boolean; paragraphs: string[] }[][] | null;
}

/** As formas de um slide do modelo TS como o texto acessível as lê. */
export function a11yShapesOf(slide: Slide): A11yShape[] {
  return slide.shapes.map((s: Shape) => ({
    inherited: !!s.inherited,
    phType: s.phType,
    descr: s.descr,
    image: !!s.image,
    media: !!s.media,
    hasText: !!s.txBody,
    paragraphs: (s.txBody?.paragraphs ?? []).map(joinRuns),
    table: s.table ? s.table.rows.map((row) => row.cells.map((cell) => ({
      hMerge: !!(cell as { hMerge?: boolean }).hMerge,
      vMerge: !!(cell as { vMerge?: boolean }).vMerge,
      paragraphs: (cell.txBody?.paragraphs ?? []).map(joinRuns),
    }))) : null,
  }));
}

/** Título do slide: o placeholder de título; sem ele, a 1ª linha de texto. */
export function slideTitle(shapes: readonly A11yShape[] | undefined): string {
  if (!shapes) return '';
  const own = shapes.filter((s) => !s.inherited);
  const t = own.find((s) => s.phType && TITLE_PH.has(s.phType) && s.hasText);
  const text = t?.paragraphs.map(norm).filter(Boolean).join(' ');
  if (text) return text;
  for (const s of own) for (const p of s.paragraphs) { const x = norm(p); if (x) return x; }
  return '';
}

export class SlideA11y {
  /** conteúdo do slide em cena (oculto, na árvore de acessibilidade) */
  readonly content = document.createElement('div');
  readonly live = new LiveRegion();
  private labels: Labels = LABELS.en;
  private shown = -1;
  private skipText: string | null = null;
  private count = 0;

  constructor(private readonly canvas: HTMLCanvasElement, private readonly wrap: HTMLElement) {
    this.content.style.cssText = SR_ONLY;
    this.content.setAttribute('role', 'group');
    this.content.setAttribute('part', 'slide-text');
    canvas.setAttribute('role', 'img');
    wrap.setAttribute('role', 'region');
    wrap.append(this.content, this.live.el);
  }

  setLang(lang: UiLang): void {
    this.labels = pickLabels(lang, LABELS);
    this.canvas.setAttribute('aria-roledescription', this.labels.roledesc);
    this.shown = -1;
  }

  /** Nome da região do viewer (o arquivo, quando houver). */
  setName(fileName: string | null): void {
    this.wrap.setAttribute('aria-label', fileName ? `${this.labels.presentation}: ${fileName}` : this.labels.presentation);
  }

  /**
   * Slide em cena mudou (ou o deck): refaz o texto e, se `announce`, anuncia. `shapesOf(i)` dá as formas do slide `i` (do modelo
   * TS ou do modelo de leitura do Rust); só é chamado quando o slide ou a contagem mudou.
   */
  show(count: number, index: number, shapesOf: (i: number) => A11yShape[] | undefined, announce: boolean): void {
    if (index === this.shown && count === this.count) return;
    const first = this.shown < 0;
    this.shown = index;
    this.count = count;
    const L = this.labels;
    const slide = index >= 0 && index < count ? shapesOf(index) : undefined;
    if (!slide) {
      this.canvas.setAttribute('aria-label', L.presentation);
      this.content.textContent = '';
      return;
    }
    const title = slideTitle(slide);
    const where = L.slide(index + 1, count);
    const label = `${where}: ${title || L.untitled}`;
    this.canvas.setAttribute('aria-label', label);
    this.content.setAttribute('aria-label', `${L.content} — ${where}`);
    this.content.textContent = '';
    const h = document.createElement('h2');
    h.textContent = label;
    this.content.appendChild(h);
    // título tirado da 1ª linha (sem placeholder de título): não repete como parágrafo
    this.skipText = slide.some((s) => !s.inherited && s.phType && TITLE_PH.has(s.phType) && s.hasText) ? null : title;
    for (const s of slide) this.appendShape(s);
    if (announce && !first) this.live.announce(label);
  }

  private appendShape(s: A11yShape): void {
    if (s.inherited || (s.phType && (SKIP_PH.has(s.phType) || TITLE_PH.has(s.phType)))) return;
    const alt = usefulAlt(s.descr);
    if (alt && (s.image || s.media || !s.paragraphs.some((p) => norm(p)))) {
      const img = document.createElement('div');
      img.setAttribute('role', 'img');
      img.setAttribute('aria-label', alt);
      this.content.appendChild(img);
    }
    for (const p of s.paragraphs) {
      const t = norm(p);
      if (!t) continue;
      if (t === this.skipText) { this.skipText = null; continue; }
      const el = document.createElement('p');
      el.textContent = t;
      this.content.appendChild(el);
    }
    if (s.table) {
      const table = document.createElement('table');
      for (const row of s.table) {
        const tr = document.createElement('tr');
        for (const cell of row) {
          if (cell.hMerge || cell.vMerge) continue;
          const td = document.createElement('td');
          td.textContent = cell.paragraphs.map(norm).filter(Boolean).join(' ');
          tr.appendChild(td);
        }
        table.appendChild(tr);
      }
      this.content.appendChild(table);
    }
  }
}
