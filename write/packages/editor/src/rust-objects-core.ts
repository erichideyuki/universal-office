// Frente O da leva 5 do engine="rust" (registro core/docs/modules/uo-bindings.md "Wave 5 — O: objects and the page"): o
// estado por editor, a geometria dos objetos (`bindObjects` de core/crates/uo-wasm/js/session.mjs: o port de
// `enumFloats`/`enumInlinePics`), os fatos da seção do cursor e as conversões entre as formas do TS (DmlColor, PicLine…)
// e as do fio (core/schema/session.uos). Carregado só com o motor Rust.
import {
  bindObjects, bindStories, type ExecOutcome, type ObjectsBinding, type SessionCommand, type ShellFloat, type ShellInlinePic, type ShellObjectFrame,
  type ShellObjects, type ShellSectionInfo, type StoriesBinding, type StoryContext, type StoryCtx,
} from '@universal-office/write-session';
import type { FrontApi, RustEditor } from './rust-engine.js';
import { runJobs } from './rust-objects-jobs.js';

export const PX_PER_TWIP = 96 / 1440;

/** Um objeto com alças (o `FloatHandle` do TS): flutuante ou imagem em linha. */
export interface Handle {
  kind: 'image' | 'textbox' | 'shape' | 'group';
  /** o id do desenho (a identidade do run do TS) */
  id: number;
  page: number;
  rect: { x: number; y: number; w: number; h: number };
  rot: number;
  inline: boolean;
  /** flutuante: o índice de `SelectObject`; em linha: `SelectInline` */
  index?: number;
  at?: { para: number; run: number };
  frame: ShellObjectFrame | null;
  editable: boolean;
  /** a imagem do layout (`imageBytes`) */
  image?: number;
  /** a chave do Painel de Seleção */
  key?: string;
  /** a caixa de texto em edição (o `tbEdit` do TS: contorno tracejado) */
  editing?: boolean;
}

/** O estado da frente O de um editor. */
export interface ObjState {
  readonly ed: RustEditor;
  readonly api: FrontApi;
  ob: ObjectsBinding | null;
  /** the stories binding (front S's session exports, read-only here: the edit context and the header/footer page) */
  sb: StoriesBinding | null;
  cache: { dl: unknown; gen: number; v: ShellObjects } | null;
  /** Desenhar Caixa de Texto (vertical?) / Criar Vínculo */
  mode: null | { t: 'draw'; vertical: boolean } | { t: 'link' };
  /** o modo Cortar (o id do desenho) */
  crop: number | null;
  /** o arrasto em curso (o fantasma pintado pelo gancho de pintura, ED-203) */
  drag: null | { page: number; rect: { x: number; y: number; w: number; h: number }; rot: number; dashed?: boolean };
  /** o zoom antes do Modo de Leitura */
  zoomBeforeRead: number | null;
  readFirst: number;
  /** as imagens decodificadas (tamanho natural, prévia), por id do desenho; 'loading' enquanto decodifica */
  bitmaps: Map<number, ImageBitmap | 'loading'>;
}

const states = new WeakMap<RustEditor, ObjState>();

export function stateOf(ed: RustEditor): ObjState {
  let s = states.get(ed);
  if (!s) {
    const api = typeof (ed as { frontApi?: unknown }).frontApi === 'function' ? ed.frontApi() : (null as unknown as FrontApi);
    s = { ed, api, ob: null, sb: null, cache: null, mode: null, crop: null, drag: null, zoomBeforeRead: null, readFirst: 0, bitmaps: new Map() };
    states.set(ed, s);
  }
  return s;
}

/** `undefined` members dropped (the wire's absent fields). */
export const omit = (o: Record<string, unknown>): Record<string, unknown> => Object.fromEntries(Object.entries(o).filter(([, v]) => v !== undefined));

/** No session or a locked document (`locked()` of the TS: read-only, protection, a pending job). */
export const locked = (o: ObjState): boolean => !o.api?.session || o.api.locked();

function binding(o: ObjState): ObjectsBinding | null {
  if (o.ob) return o.ob;
  const mod = o.api?.module;
  if (!mod) return null;
  try { o.ob = bindObjects(mod.exports, mod.binding); } catch { o.ob = null; }
  return o.ob;
}

function stories(o: ObjState): StoriesBinding | null {
  if (o.sb) return o.sb;
  const mod = o.api?.module;
  if (!mod) return null;
  try { o.sb = bindStories(mod.exports, mod.binding); } catch { o.sb = null; }
  return o.sb;
}

/** The session's edit context (`cellEdit`, `noteEdit`, `hfEdit`, `tbEdit` of the TS). */
export function storyCtx(o: ObjState): StoryContext | null {
  const s = o.api?.session, sb = stories(o);
  if (!s || !sb) return null;
  try { return sb.storyContext(s); } catch { return null; }
}

/** The context outside a text box (the TS keeps `noteEdit`/`hfEdit`/`cellEdit` with a box open inside). */
export function outerCtx(c: StoryCtx): StoryCtx {
  return c.kind === 'textbox' ? outerCtx(c.outer) : c;
}

/** The page of the header/footer in edit (the TS `hfActivePage`), −1 outside a header/footer. */
export function hfPageNow(o: ObjState): number {
  const c = storyCtx(o);
  const x = c ? outerCtx(c.ctx) : null;
  return x?.kind === 'hf' ? x.page : -1;
}

/** The section of the page in header/footer edit (what the TS's section queries read there), −1 outside. */
function hfSectionNow(o: ObjState): number {
  const pg = hfPageNow(o);
  const s = o.api?.session, dl = o.api?.dl, sb = stories(o);
  if (pg < 0 || !s || !dl || !sb) return -1;
  try { return sb.hfPage(s, dl.inner, pg)?.section ?? -1; } catch { return -1; }
}

/** `enumFloats` + `enumInlinePics` do layout de agora (em cache por layout e geração do editor). */
export function objects(o: ObjState): ShellObjects | null {
  const api = o.api;
  const dl = api?.dl, s = api?.session;
  const ob = binding(o);
  if (!dl || !s || !ob) return null;
  const gen = api.gen;
  if (o.cache && o.cache.dl === dl && o.cache.gen === gen) return o.cache.v;
  let v: ShellObjects;
  // a header/footer context: only the objects of the page in edit (the TS `hfActivePage`)
  try { v = ob.objects(s, dl.inner, hfPageNow(o)); } catch { return null; }
  o.cache = { dl, gen, v };
  return v;
}

/** Os flutuantes mesmo dentro de uma caixa de texto (sem cache): a caixa que acabou de ser desenhada e já está em edição. */
export function objectsInBox(o: ObjState): ShellObjects | null {
  const dl = o.api?.dl, s = o.api?.session;
  const ob = binding(o);
  if (!dl || !s || !ob) return null;
  try { return ob.objects(s, dl.inner, hfPageNow(o), true); } catch { return null; }
}

/** The placed cell boxes of `page` (the layout's `page.cells`, front S's export). */
export function cellBoxesOn(o: ObjState, page: number): { x: number; y: number; w: number; h: number }[] {
  const dl = o.api?.dl, sb = stories(o);
  if (!dl || !sb) return [];
  try { return sb.cellBoxes(dl.inner, page); } catch { return []; }
}

/** `hitTextbox(e)` of the TS through the session (`uo_shell_textbox_at`, front S's export): the text box with laid-out
 *  content under the point, of the context's story, 5 px of tolerance outside its border, the top one winning (front
 *  layer, then relativeHeight). `rect`/`rot`/`page` of the hit box. */
export function textboxHit(o: ObjState, page: number, x: number, y: number, tol: number): { page: number; rect: { x: number; y: number; w: number; h: number }; rot: number } | null {
  const s = o.api?.session, dl = o.api?.dl, sb = stories(o);
  if (!s || !dl || !sb) return null;
  try { return sb.textboxAt(s, dl.inner, page, x, y, tol); } catch { return null; }
}

export function floatHandle(f: ShellFloat): Handle {
  return { kind: f.kind, id: f.id, page: f.page, rect: { x: f.x, y: f.y, w: f.w, h: f.h }, rot: f.rot, inline: false, index: f.i, frame: f.frame, editable: f.editable, ...(f.image !== null ? { image: f.image } : {}), ...(f.key !== null ? { key: f.key } : {}) };
}

export function inlineHandle(p: ShellInlinePic): Handle {
  return { kind: 'image', id: p.id, page: p.page, rect: { x: p.x, y: p.y, w: p.w, h: p.h }, rot: p.rot, inline: true, at: { para: p.para, run: p.run }, frame: null, editable: true, image: p.image };
}

/** `selectedFloat()`: o objeto selecionado (flutuante, ou a imagem em linha sob a seleção). */
export function selectedHandle(o: ObjState): Handle | null {
  const v = objects(o);
  const id = v?.sel[0];
  if (!v || id === undefined) return null;
  const f = v.floats.find((x) => x.id === id);
  if (f) return floatHandle(f);
  const p = v.inline.find((x) => x.id === id);
  return p ? inlineHandle(p) : null;
}

/** `selectedFloats()`: os objetos da seleção múltipla (o selecionado primeiro). */
export function selectedHandles(o: ObjState): Handle[] {
  const v = objects(o);
  if (!v) return [];
  const out: Handle[] = [];
  for (const id of v.sel) {
    const f = v.floats.find((x) => x.id === id);
    if (f) out.push(floatHandle(f));
    else { const p = v.inline.find((x) => x.id === id); if (p) out.push(inlineHandle(p)); }
  }
  return out;
}

/** Os fatos da seção do cursor (`getSection` e companhia). */
export function sectionInfo(o: ObjState): ShellSectionInfo | null {
  const s = o.api?.session;
  const ob = binding(o);
  if (!s || !ob) return null;
  // a header/footer context: the section of the page in edit (the TS reads `sectPrForSavedPage(hfActivePage)`)
  try { return ob.sectionInfo(s, hfSectionNow(o)); } catch { return null; }
}

/** Manda um comando; trabalhos de duas fases vão ao executor (decodificar só o tamanho ou os pixels). */
export function send(o: ObjState, cmd: SessionCommand, what: string, pixels = true): Promise<ExecOutcome | null> | boolean {
  const s = o.api?.session;
  if (!s) return false;
  const out = s.command(cmd);
  if (out?.t === 'jobs') return runJobs(o.api, out, what, pixels);
  return o.api.apply(out, what);
}

/** Um comando sem trabalhos (o resultado do apply). */
export function cmd(o: ObjState, c: SessionCommand, what: string): boolean {
  const r = send(o, c, what);
  return typeof r === 'boolean' ? r : true;
}

// ───────────────────────── conversões TS ⇄ fio ─────────────────────────

type Dml = { kind: string; val: string; lastClr?: string; mods?: [string, number][] };

/** DmlColor → PicColor (os modificadores como pares `{ f0, f1 }`). */
export function wireColor(c: Dml): Record<string, unknown> {
  return { kind: c.kind, val: c.val, ...(c.lastClr !== undefined ? { lastClr: c.lastClr } : {}), ...(c.mods ? { mods: c.mods.map(([f0, f1]) => ({ f0, f1 })) } : {}) };
}

/** `T | null | undefined` → `Nullable<T>` do fio (`{ t: 'null' }` / `{ t: 'val', … }`), ausente fica ausente. */
export function nullable<T>(x: T | null | undefined, f: (x: T) => Record<string, unknown>): Record<string, unknown> | undefined {
  if (x === undefined) return undefined;
  return x === null ? { t: 'null' } : { t: 'val', ...f(x) };
}

const omitUndef = (o: Record<string, unknown>): Record<string, unknown> => Object.fromEntries(Object.entries(o).filter(([, v]) => v !== undefined));

export function wireLine(l: Record<string, unknown>): Record<string, unknown> {
  const { fill, ...rest } = l;
  return omitUndef({ ...rest, fill: nullable(fill as Dml | null | undefined, wireColor) });
}

function withColor(x: Record<string, unknown>): Record<string, unknown> {
  const { color, ...rest } = x;
  return { ...rest, color: wireColor(color as Dml) };
}

function wire3d(t: Record<string, unknown>): Record<string, unknown> {
  const { extrusionClr, contourClr, ...rest } = t;
  return omitUndef({ ...rest, extrusionClr: extrusionClr ? wireColor(extrusionClr as Dml) : undefined, contourClr: contourClr ? wireColor(contourClr as Dml) : undefined });
}

/** O argumento de `setImageFormat`/`setShapeFormat` (`FormatArg`). */
export function wireFormat(o: Record<string, unknown>): Record<string, unknown> {
  return omitUndef({
    line: nullable(o['line'] as Record<string, unknown> | null | undefined, wireLine),
    shadow: nullable(o['shadow'] as Record<string, unknown> | null | undefined, withColor),
    reflection: nullable(o['reflection'] as Record<string, unknown> | null | undefined, (x) => x),
    glow: nullable(o['glow'] as Record<string, unknown> | null | undefined, withColor),
    softEdge: o['softEdge'] === undefined ? undefined : o['softEdge'] === null ? { t: 'null' } : { t: 'val', v: o['softEdge'] },
    threeD: nullable(o['threeD'] as Record<string, unknown> | null | undefined, wire3d),
  });
}

/** ShapeFill do TS (`kind`) → ShapeFill do fio (`t`). */
export function wireShapeFill(f: Record<string, unknown>): Record<string, unknown> {
  const { kind, ...r } = f;
  switch (kind) {
    case 'solid': return { t: 'solid', ...wireColor(r['color'] as Dml) };
    case 'gradient': {
      const lin = r['lin'] as { ang: number; scaled: boolean } | undefined;
      const path = r['path'] as { path: string; fillToRect?: unknown } | undefined;
      return omitUndef({
        t: 'gradient',
        stops: (r['stops'] as { pos: number; color: Dml }[]).map((s) => ({ pos: s.pos, color: wireColor(s.color) })),
        lin: lin ? omitUndef({ f0: lin.ang, f1: lin.scaled || undefined }) : undefined,
        path: path ? omitUndef({ f0: path.path, f1: path.fillToRect }) : undefined,
        rotWithShape: r['rotWithShape'], flip: r['flip'], tileRect: r['tileRect'],
      });
    }
    case 'pattern': return { t: 'pattern', prst: r['prst'], fg: wireColor(r['fg'] as Dml), bg: wireColor(r['bg'] as Dml) };
    case 'picture': return omitUndef({ t: 'picture', embed: r['embed'], tile: r['tile'], alpha: r['alpha'] });
    case 'raw': return { t: 'raw', v: r['xml'] };
    default: return { t: String(kind) };
  }
}

/**
 * O valor do fio de volta à forma do TS (o `format` de `selectedImage`/`selectedShape`): `Nullable` → `null`/valor,
 * pares `{ f0, f1 }` → tuplas, `adj` → registro. Os booleanos falsos não viajam (ausente = false).
 */
export function fromWire(v: unknown, key = ''): unknown {
  if (Array.isArray(v)) {
    if (key === 'adj') return Object.fromEntries(v.map((p: { f0: string; f1: number }) => [p.f0, p.f1]));
    return v.map((x) => fromWire(x));
  }
  if (!v || typeof v !== 'object' || v instanceof Uint8Array) return v;
  const o = v as Record<string, unknown>;
  if (o['t'] === 'null' && Object.keys(o).length === 1) return null;
  if (o['t'] === 'val') {
    const { t: _t, ...rest } = o;
    void _t;
    return 'v' in rest && Object.keys(rest).length === 1 ? rest['v'] : fromWire(rest);
  }
  const keys = Object.keys(o);
  if (keys.length > 0 && keys.every((k) => k === 'f0' || k === 'f1')) return [fromWire(o['f0']), fromWire(o['f1'])];
  const out: Record<string, unknown> = {};
  for (const k of keys) out[k] = fromWire(o[k], k);
  return out;
}

/** A língua da interface para os comandos de galeria (`PageLang`). */
export function pageLang(l: string): 'en' | 'pt' | 'es' { return l === 'pt' || l === 'es' ? l : 'en'; }
