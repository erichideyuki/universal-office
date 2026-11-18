import {
  blankPresentation, openEditSession,
  type BlankDeckOptions, type EditResult, type LoadSlidesOptions, type Op, type ShapeClip as WireClip, type SlidesEditSession,
} from '@universal-office/slides-editor-engine';
import type { PptxPackage } from '@universal-office/slides-core';
import type { FontResolver } from '@universal-office/fonts';
import type { ShapeClip } from './deck-edit.js';

/**
 * `engine="rust"` do <universal-slides>: o lado do editor da troca TS/Rust (registro em core/docs/modules/uo-bindings.md
 * "Slides editor (B8 routing)"). Carregado SÓ por `import()` dinâmico quando o atributo pede o motor Rust — um editor sem
 * `engine="rust"` nunca baixa este módulo nem o .wasm.
 *
 * O pacote, as operações, o desfazer/refazer e o salvar (com as fontes) são do Rust (`SlidesEditSession`: o editor sobre o
 * MESMO deck do viewer, que emite as listas de desenho); o elemento guarda uma CÓPIA do pacote no `PptxPackage` do TS,
 * mantida em dia pelas partes que cada resposta diz que mudaram — dela o elemento relê o modelo TS que a seleção, as alças,
 * os testes de clique, a barra e o texto acessível ainda usam (registro, "Left on the TS path").
 */

export type { SlidesEditSession, EditResult, LoadSlidesOptions, Op };

/** O .wasm do editor (as exportações do viewer + a edição): ao lado deste módulo no pacote publicado. */
const WASM_FILE = 'uo-slides-editor.wasm';
const PPT_FILE = 'uo-slides-ppt.wasm';

/** De onde vem o motor: o configurado (`configureEngine`), senão os arquivos do pacote. */
export function engineSource(s: LoadSlidesOptions | undefined): LoadSlidesOptions {
  return { ...s, wasm: s?.wasm ?? new URL(WASM_FILE, import.meta.url), ppt: s?.ppt ?? new URL(PPT_FILE, import.meta.url) };
}

export async function openSession(bytes: Uint8Array, source: LoadSlidesOptions | undefined): Promise<SlidesEditSession> {
  return openEditSession(bytes, engineSource(source));
}

export async function blankBytes(opts: BlankDeckOptions, source: LoadSlidesOptions | undefined): Promise<Uint8Array> {
  return blankPresentation(opts, engineSource(source));
}

/** Aplica à cópia do pacote do elemento as partes que uma resposta do Rust diz que mudaram. */
export function applyChanges(pkg: PptxPackage, r: EditResult): void {
  for (const c of r.changes) {
    if (c.bytes) pkg.setPart(c.name, c.bytes);
    else pkg.removePart(c.name);
  }
}

/** Marca dos recortes que vieram do motor Rust: os bytes do `Clip` (colam em qualquer editor Rust da página). */
const RUST_CLIP = Symbol('uo-rust-clip');
type TaggedClip = ShapeClip & { [RUST_CLIP]?: Uint8Array };

/** O recorte de `copyShape` no formato público (`ShapeClip`), com os bytes do Rust anexados. */
export function clipFromRust(bytes: Uint8Array, s: SlidesEditSession, source: PptxPackage): ShapeClip {
  const c = s.decodeClip(bytes);
  const clip: TaggedClip = {
    xml: c.xml,
    rels: c.rels.map((r) => ({
      id: r.id, type: r.type, target: r.target, external: !!r.external,
      ...(r.bytes ? { bytes: r.bytes } : {}), ...(r.contentType !== undefined ? { contentType: r.contentType } : {}),
    })),
    text: c.text,
    source,
    slidePath: c.slidePath,
  };
  clip[RUST_CLIP] = bytes;
  return clip;
}

/**
 * O recorte para `pasteShape` no formato do Rust: o do próprio Rust como veio; um recorte de um editor com o motor TS
 * entra como de OUTRA apresentação (identidade 0: as partes vão com os bytes que o recorte leva).
 */
export function clipToWire(clip: ShapeClip, s: SlidesEditSession): WireClip {
  const raw = (clip as TaggedClip)[RUST_CLIP];
  if (raw) return s.decodeClip(raw);
  return {
    xml: clip.xml,
    rels: clip.rels.map((r) => ({
      id: r.id, type: r.type, target: r.target, external: r.external,
      ...(r.bytes ? { bytes: r.bytes } : {}), ...(r.contentType !== undefined ? { contentType: r.contentType } : {}),
    })),
    text: clip.text,
    sourceId: 0,
    slidePath: clip.slidePath,
  };
}

/**
 * Fase 2 do salvar com fontes: a resposta de cada pedido do Rust com o arquivo da fonte, pela MESMA regra do
 * `planFontEmbedding` do TS (o resolvedor resolve as famílias; só fonte com arquivo próprio — do host, do Local Font Access
 * ou embarcada — vai para o arquivo; a variante que falta é carregada).
 */
export async function answerFontRequests(resolver: FontResolver, requests: { family: string; variant: string }[]): Promise<(Uint8Array | undefined)[]> {
  if (!requests.length) return [];
  const families = [...new Map(requests.map((r) => [r.family.toLowerCase(), r.family])).values()];
  const resolved = new Map((await resolver.resolve(families)).map((r) => [r.family.toLowerCase(), r]));
  const out: (Uint8Array | undefined)[] = [];
  for (const q of requests) {
    const r = resolved.get(q.family.toLowerCase());
    const own = r && (r.source === 'host' || r.source === 'local-access' || r.source === 'bundled');
    const v = q.variant as keyof NonNullable<typeof r>['faces'];
    let bytes: Uint8Array | undefined;
    try { bytes = own ? r.faces[v] ?? (await resolver.loadFace(r, v)) : undefined; } catch { bytes = undefined; }
    out.push(bytes ?? undefined);
  }
  return out;
}
