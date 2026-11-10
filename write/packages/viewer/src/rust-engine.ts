import { DocxPackage, convertDocToDocx, isWordBinary, readFullDocument, type FullDocument } from '@universal-office/docx-core';
import { isFontFile, type FontResolver, type FontVariant } from '@universal-office/fonts';
import {
  WriteError, loadViewerModule,
  type DecodedLayout, type Face, type FontSource, type LoadWriteOptions, type PaintEnv, type ViewerDocument, type ViewerEnv, type ViewerModule,
} from '@universal-office/write-engine';
import { ASSET_FILES, advancesFile } from './fonts.js';
import { decodeImage } from './image-decode.js';
import type { TextLine } from './text-layer.js';

/**
 * `engine="rust"` do <universal-document-viewer>: o lado do viewer da troca TS/Rust (registro em
 * core/docs/modules/uo-bindings.md "The TS/Rust switch of the viewer"). Carregado SÓ por `import()` dinâmico quando o
 * atributo pede o motor Rust — um viewer sem `engine="rust"` nunca baixa este módulo nem o .wasm.
 *
 * Um `RustSession` por elemento: um ambiente do motor (fontes, padrões de hifenização), o documento aberto e os
 * layouts vivos. As fontes continuam no resolvedor único de shared/fonts: o motor Rust PEDE (takeMissing), o
 * resolvedor responde (getWithFile) e o resultado é empurrado (pushFamily) — a mesma política do `ViewerFonts` do motor TS.
 */

/** O .wasm do motor: ao lado deste módulo (no pacote publicado, `chunks/uo-write.wasm`). */
const WASM_FILE = 'uo-write.wasm';
const LANGS = ['pt-BR', 'en', 'es'];
/** Passadas de layout por carga: 1ª sem nada empurrado + as que as fontes pedidas refazem (o TS tem 4). */
const MAX_PASSES = 5;
const VARIANTS: readonly FontVariant[] = ['regular', 'bold', 'italic', 'boldItalic'];

/** O que o elemento passa à sessão. */
export interface RustHost {
  /** a base dos arquivos de fonte (atributo `fonts` ou o pacote irmão) */
  fontsBase(): string | null;
  resolver: FontResolver;
  /** a fonte ou a imagem que faltava chegou: repintar */
  repaint(): void;
  /** o .wasm configurado (`configureEngine`); sem ele, o do pacote */
  source(): LoadWriteOptions | undefined;
}

export interface RelayoutOptions { view: 'print' | 'web'; webWidthTw: number }

/** Um arquivo opcional da base de fontes (os mesmos do motor TS), em bytes. */
interface AssetFile { family: string; variant: FontVariant; bytes: Uint8Array; advances: string | undefined }

/** Arquivos já baixados por base (200 aberturas não rebaixam as fontes). */
const assetFiles = new Map<string, Promise<AssetFile[]>>();

function fetchAssets(base: string): Promise<AssetFile[]> {
  let p = assetFiles.get(base);
  if (!p) {
    p = Promise.all(ASSET_FILES.map(async ([family, file, variant]): Promise<AssetFile | null> => {
      const resp = await fetch(`${base}/${file}`).catch(() => null);
      if (!resp?.ok) return null;
      try {
        const bytes = new Uint8Array(await resp.arrayBuffer());
        if (!isFontFile(bytes)) return null; // 200 com o HTML de fallback de uma SPA: ausente
        let advances: string | undefined;
        try {
          const r = await fetch(`${base}/${advancesFile(family, variant)}`);
          if (r.ok) advances = await r.text();
        } catch { /* tabela opcional */ }
        return { family, variant, bytes, advances };
      } catch { return null; }
    })).then((all) => all.filter((x): x is AssetFile => x !== null));
    assetFiles.set(base, p);
  }
  return p;
}

const sourceOf = (s: string): FontSource => (s === 'local-access' ? 'localAccess' : s as FontSource);

/** Uma FontFace por ARQUIVO (conteúdo), para a página inteira: vários elementos e documentos compartilham. */
interface FaceEntry { name: string; ready: boolean; loaded: Promise<void> }
const faceEntries = new Map<string, FaceEntry>();
let faceSeq = 0;

/** O texto de uma página para a camada de texto: o parágrafo de cada `ref` (null: sem texto). */
export interface TextParagraph { runs: { text: string }[]; level: number | undefined }
interface PageText { para: (ref: string) => TextParagraph | null }

export class RustSession {
  private mod: ViewerModule | null = null;
  private env: ViewerEnv | null = null;
  private doc: ViewerDocument | null = null;
  private docx: Uint8Array | null = null;
  private readonly layouts = new Set<DecodedLayout>();
  /** família (minúsculas) → chave da resposta do resolvedor já empurrada, e o nome como o motor a pediu */
  private readonly pushed = new Map<string, string>();
  private readonly pushedNames = new Map<string, string>();
  private readonly patterns = new Set<string>();
  private assetsPushed = false;
  private model: FullDocument | null = null;
  private disposed = false;
  private readonly envs = new WeakMap<DecodedLayout, PaintEnv>();
  private readonly texts = new WeakMap<DecodedLayout, Map<number, PageText>>();
  private readonly images = new WeakMap<DecodedLayout, Map<number, CanvasImageSource | 'loading' | null>>();
  private readonly notified = new Set<string>();

  constructor(private readonly host: RustHost) {}

  get hasDocument(): boolean { return this.doc !== null; }

  stats(): ReturnType<ViewerModule['stats']> | null { return this.mod?.stats() ?? null; }

  /** De onde vem o módulo: o configurado; só `js` = o build wasm2js; nada = o .wasm do pacote, ao lado deste código. */
  private moduleSource(): LoadWriteOptions {
    const src = this.host.source();
    if (!src || (src.wasm === undefined && src.js === undefined)) return { ...src, wasm: new URL(WASM_FILE, import.meta.url) };
    return src.wasm === undefined && src.engine === undefined ? { ...src, engine: 'js' } : src;
  }

  /** Abre o documento (.docx ou .doc). LANÇA em arquivo inválido ANTES de mudar qualquer estado. `false`: a sessão acabou no meio. */
  async open(bytes: Uint8Array): Promise<boolean> {
    const docx = isWordBinary(bytes) ? convertDocToDocx(bytes) : bytes;
    const mod = this.mod ??= await loadViewerModule(this.moduleSource());
    const doc = mod.open(docx);
    if (this.disposed) { doc.free(); return false; }
    this.releaseLayouts();
    this.doc?.free();
    this.doc = doc;
    this.docx = docx;
    this.model = null;
    this.env ??= mod.newEnv();
    return true;
  }

  /** O modelo do documento no motor TS (só para o que o motor Rust ainda não entrega: indicadores, links, inlines). */
  tsModel(): FullDocument | null {
    if (!this.docx) return null;
    return (this.model ??= readFullDocument(DocxPackage.load(this.docx)));
  }

  /** Libera um layout (idempotente). */
  release(dl: DecodedLayout): void {
    this.layouts.delete(dl);
    dl.free();
  }

  releaseLayouts(): void {
    for (const dl of this.layouts) dl.free();
    this.layouts.clear();
  }

  /** Libera tudo: layouts, documento e ambiente (idempotente). */
  dispose(): void {
    this.disposed = true;
    this.releaseLayouts();
    this.doc?.free();
    this.doc = null;
    this.env?.free();
    this.env = null;
    this.docx = null;
    this.model = null;
  }

  private async prepare(env: ViewerEnv, doc: ViewerDocument): Promise<void> {
    const base = this.host.fontsBase();
    this.host.resolver.suggestBaseUrl(base);
    if (!this.assetsPushed && base !== null) {
      const files = await fetchAssets(base);
      for (const f of files) {
        try { env.pushAsset(f.family, f.variant, f.bytes, f.advances); } catch { /* arquivo que o motor recusa: pula */ }
      }
      this.assetsPushed = true;
    }
    try {
      for (const id of doc.hyphenationPatterns(env, LANGS)) {
        if (this.patterns.has(id)) continue;
        const text = await this.mod!.patternText(id);
        if (text) { env.registerHyphenation(text); this.patterns.add(id); }
      }
    } catch { /* sem padrões: sem hifenização */ }
  }

  /** Empurra a resposta do resolvedor para `family` se ela mudou; true se empurrou. */
  private push(env: ViewerEnv, family: string): boolean {
    const r = this.host.resolver.getWithFile(family);
    if (!r) return false;
    const faces = VARIANTS.flatMap((variant) => { const bytes = r.faces[variant]; return bytes ? [{ variant, bytes }] : []; });
    const key = [r.source, r.embeddable ? 1 : 0, ...faces.map((f) => `${f.variant}:${f.bytes.length}`)].join('|');
    const k = family.toLowerCase();
    if (this.pushed.get(k) === key) return false;
    env.pushFamily(family, { source: sourceOf(r.source), embeddable: r.embeddable, faces });
    this.pushed.set(k, key);
    this.pushedNames.set(k, family);
    return true;
  }

  /** Responde o que o motor pediu e não havia; true se algo novo foi empurrado (refazer o layout). */
  private async supply(env: ViewerEnv, missing: { family: string; variants: FontVariant[] }[]): Promise<boolean> {
    let any = false;
    const later: typeof missing = [];
    for (const m of missing) { if (this.push(env, m.family)) any = true; else later.push(m); }
    if (!later.length) return any;
    const resolver = this.host.resolver;
    try {
      const want = new Map(later.map((m) => [m.family, new Set(m.variants)]));
      const rs = await resolver.resolveWithFile(want.keys());
      await Promise.all(rs.flatMap((r) => [...(want.get(r.family) ?? [])].filter((v) => !r.faces[v]).map((v) => resolver.loadFace(r, v))));
    } catch { return any; }
    for (const m of later) if (this.push(env, m.family)) any = true;
    return any;
  }

  /**
   * Diagrama e repete enquanto o motor pede fontes que chegam. Cada passada entrega o layout a `onLayout(dl, pass)`, que
   * assume a posse (libera o anterior por `release`). Para quando `stale()` ou o documento muda; o layout em andamento é
   * cancelado pelo `cancelCheck` do motor (G4).
   */
  async relayout(stale: () => boolean, o: RelayoutOptions, onLayout: (dl: DecodedLayout, pass: number) => void): Promise<void> {
    const { env, doc } = this;
    if (!env || !doc) return;
    await this.prepare(env, doc);
    for (let pass = 0; pass < MAX_PASSES; pass++) {
      if (stale() || this.doc !== doc || this.disposed) return;
      for (const name of [...this.pushedNames.values()]) this.push(env, name); // o resolvedor mudou (onChange)
      let dl: DecodedLayout;
      try {
        dl = doc.layout(env, {
          hyphenationLanguages: LANGS, view: o.view, ...(o.view === 'web' ? { webWidthTw: o.webWidthTw } : {}),
          cancelCheck: () => stale() || this.doc !== doc || this.disposed,
        });
      } catch (e) {
        if (e instanceof WriteError && e.code === 10) return;
        throw e;
      }
      this.layouts.add(dl);
      onLayout(dl, pass);
      const missing = env.takeMissing();
      if (!missing.length || !(await this.supply(env, missing))) return;
    }
  }

  /** Uma passada só, sem o laço de fontes: o layout de impressão sob demanda (como o `printLayout` do motor TS). */
  layoutSync(): DecodedLayout | null {
    const { env, doc } = this;
    if (!env || !doc) return null;
    const dl = doc.layout(env, { hyphenationLanguages: LANGS, view: 'print' });
    this.layouts.add(dl);
    return dl;
  }

  // ───────────────────────── pintura ─────────────────────────

  paintEnv(dl: DecodedLayout): PaintEnv {
    let e = this.envs.get(dl);
    if (!e) {
      e = { fontOf: (face) => this.fontOf(dl, face), image: (id) => this.imageOf(dl, id), color: (hex) => `#${hex}` };
      this.envs.set(dl, e);
    }
    return e;
  }

  private fontOf(dl: DecodedLayout, face: Face): { family: string; weight: 400 | 700; italic: boolean } {
    const i = face.hasBytes ? dl.faceIndex(face) : -1;
    const key = i >= 0 ? dl.faceKey(i) : null;
    if (key === null) {
      return { family: `"${face.family.replace(/["\\]/g, '')}", "Liberation Sans", sans-serif`, weight: face.bold && !face.synthBold ? 700 : 400, italic: !!face.italic && !face.synthItalic };
    }
    let entry = faceEntries.get(key);
    if (!entry) {
      const name = `udv-rface-${++faceSeq}`;
      entry = { name, ready: false, loaded: Promise.resolve() };
      faceEntries.set(key, entry);
      const bytes = dl.faceBytes(i);
      try {
        if (!bytes) throw new Error('no bytes');
        const ff = new FontFace(name, bytes.slice().buffer);
        document.fonts.add(ff);
        const e = entry;
        entry.loaded = ff.load().then(() => { e.ready = true; }, () => {});
      } catch { /* face inválida para o navegador */ }
    }
    if (!entry.ready && !this.notified.has(key)) {
      this.notified.add(key);
      void entry.loaded.then(() => { if (!this.disposed) this.host.repaint(); });
    }
    return { family: `"${entry.name}", "Liberation Sans", sans-serif`, weight: 400, italic: false };
  }

  private imageOf(dl: DecodedLayout, id: number): CanvasImageSource | null {
    let m = this.images.get(dl);
    if (!m) this.images.set(dl, (m = new Map()));
    const c = m.get(id);
    if (c === 'loading' || c === null) return null;
    if (c) return c;
    m.set(id, 'loading');
    const done = (b: ImageBitmap | HTMLCanvasElement | null): void => { m.set(id, b); if (b && !dl.freed && !this.disposed) this.host.repaint(); };
    const entry = dl.tables.images[id];
    if (!entry) { done(null); return null; }
    const svg = entry.hasSvg ? dl.imageSvg(id) : null;
    const data = svg ? new Uint8Array(0) : dl.imageBytes(id);
    if (!svg && !data) { done(null); return null; }
    decodeImage({ data: data ?? new Uint8Array(0), ext: entry.ext, svg: svg ?? undefined, widthTwips: entry.widthTwips, heightTwips: entry.heightTwips }, done);
    return null;
  }

  // ───────────────────────── texto ─────────────────────────

  /** As linhas da página `i` e o resolvedor de parágrafo por `ref` para `fillTextLayer`. */
  textOf(dl: DecodedLayout, i: number): { lines: readonly TextLine[]; para: (ref: string) => TextParagraph | null } {
    const page = dl.pages[i]!;
    let m = this.texts.get(dl);
    if (!m) this.texts.set(dl, (m = new Map()));
    let t = m.get(i);
    if (!t) {
      const objs: (TextParagraph | undefined)[] = [];
      const byRef = new Map<string, TextParagraph>();
      for (const l of page.lines) {
        if (l.para === undefined || byRef.has(l.ref)) continue;
        const src = page.paras[l.para];
        if (!src) continue;
        const o = objs[l.para] ??= { runs: [{ text: src.text }], level: src.level };
        byRef.set(l.ref, o);
      }
      t = { para: (ref) => byRef.get(ref) ?? null };
      m.set(i, t);
    }
    return { lines: page.lines, para: t.para };
  }

  /** O texto sob a caixa de um link: os caracteres das linhas da página que caem nela (o texto da execução inteira, no motor TS). */
  boxText(dl: DecodedLayout, box: { page: number; x: number; y: number; w: number; h: number }): string {
    const page = dl.pages[box.page];
    if (!page) return '';
    let out = '';
    for (const l of page.lines) {
      if (l.para === undefined || !l.chars || !(l.y < box.y + box.h && l.y + l.height > box.y)) continue;
      const text = page.paras[l.para]?.text;
      if (text === undefined) continue;
      for (const c of l.chars) {
        if (c.x >= box.x - 0.5 && c.x + c.w <= box.x + box.w + 0.5) out += text[c.o] ?? '';
      }
    }
    return out;
  }
}
