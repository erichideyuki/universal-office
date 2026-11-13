// Handlers da frente O da leva 5 do engine="rust" (imagens, formas/caixas de texto (objeto), configurar página, exibições, menu de contexto): cada chave é um nome de
// ROUTES_OBJECTS.public/private (engine-routes-objects.ts). Carregado só com o motor Rust (import estático de rust-engine.ts).
// Registro: core/docs/modules/uo-bindings.md "Wave 5 — O: objects and the page". Os módulos: rust-objects-core.ts (estado,
// geometria, conversões), rust-objects-page.ts (página, exibições), rust-objects-pictures.ts (imagens, formas, painel),
// rust-objects-pointer.ts (ponteiro, teclado, alças, arrastos, modos) e rust-objects-jobs.ts (trabalhos de duas fases).
import type { ExecOutcome } from '@universal-office/write-session';
import type { RustEditor } from './rust-engine.js';
import { stateOf } from './rust-objects-core.js';
import { runJobs } from './rust-objects-jobs.js';
import { afterCanvases, pageHandlers } from './rust-objects-page.js';
import { picturesHandlers } from './rust-objects-pictures.js';
import { hideTextSelection, modeHandlers, onHover, onKey, onPointer, onPointerFirst, paintPage } from './rust-objects-pointer.js';

const hooked = new WeakSet<RustEditor>();

export function objectsHandlers(ed: RustEditor): Record<string, (...args: never[]) => unknown> {
  const o = stateOf(ed);
  if (o.api && !hooked.has(ed) && (ed as { hooks?: unknown }).hooks) {
    hooked.add(ed);
    // O antes de S no mousedown: alças e objetos vencem a entrada na caixa de texto (a ordem do TS, 10186–10240)
    ed.hooks.pointer.unshift((e) => onPointer(o, e));
    // O first on the keys too: Esc in a box is S's, the selection of the box after it is O's (onKey returns false there)
    ed.hooks.key.unshift((e) => onKey(o, e));
    // the handles over the selection (the TS paintFloatAndCaret runs after the selection); the overlay hook is S's, the paint hook the fallback
    const hk = ed.hooks as typeof ed.hooks & { overlay?: ((ctx: CanvasRenderingContext2D, page: number, scale: number) => void)[]; hideSelection?: (() => boolean)[] };
    // wave 7 (B): tagged `front: 'O'` (as S's hooks) so the stories harness keeps the handles the TS paintFloatAndCaret draws too
    (hk.overlay ?? hk.paint).push(Object.assign((ctx: CanvasRenderingContext2D, page: number, scale: number) => paintPage(o, ctx, page, scale), { front: 'O' }));
    // an inline picture selected: no text selection highlight (the TS picSel)
    hk.hideSelection?.push(() => hideTextSelection(o));
    ed.hooks.adopt.push(() => { o.cache = null; o.crop = null; o.mode = null; o.drag = null; for (const b of o.bitmaps.values()) if (b !== 'loading') b.close(); o.bitmaps.clear(); });
    const el = ed.el;
    el.addEventListener('mousemove', (e) => onHover(o, e));
  }
  const all: Record<string, (...args: never[]) => unknown> = { ...pageHandlers(o), ...picturesHandlers(o), ...modeHandlers(o) };
  // without a session (before the first document, the simulated editor of the vitest) a handler answers its early return
  // value, never an exception from arguments it would have refused anyway
  for (const [k, f] of Object.entries(all)) {
    all[k] = (...a: never[]): unknown => {
      if (o.api?.session) return f(...a);
      try { const r = f(...a); return r instanceof Promise ? r.catch(() => undefined) : r; } catch { return undefined; }
    };
  }
  return all;
}


/** Gancho do `onMouseDown` (rust-engine.ts), antes do acerto no texto: as alças, o modo Cortar e Desenhar/Vincular. */
export function objectsPointerFirst(ed: RustEditor, e: MouseEvent): boolean {
  return onPointerFirst(stateOf(ed), e);
}

/** Gancho de `install`/`refreshView` (rust-engine.ts): o bloco do Modo de Leitura depois dos canvases. */
export function objectsAfterCanvases(ed: RustEditor): void {
  afterCanvases(stateOf(ed));
}

/** Gancho do `apply` (rust-engine.ts): um comando de duas fases que chega por outro caminho (a colagem HTML com imagens). */
export function objectsJobs(ed: RustEditor, o: ExecOutcome, what: string): boolean {
  const s = stateOf(ed);
  if (!s.api || o.t !== 'jobs') return false;
  void runJobs(s.api, o, what, true);
  return true;
}

/** Gancho do `onPaste` (rust-engine.ts): uma imagem sem texto nem HTML (a decodificação de duas fases). */
export async function objectsPasteImage(ed: RustEditor, file: File, plain: boolean): Promise<void> {
  const s = stateOf(ed);
  const session = s.api?.session;
  if (!session || s.api.locked()) return;
  const ext = ({ 'image/jpeg': 'jpeg', 'image/gif': 'gif', 'image/webp': 'webp', 'image/bmp': 'bmp' } as Record<string, string>)[file.type] ?? 'png';
  const data = new Uint8Array(await file.arrayBuffer());
  const out = session.command({ t: 'clipboard', v: { t: 'pasteData', text: '', image: { ext, data }, rich: 'withRich', ...(plain ? { mode: 'text' } : {}), layout: {} } });
  if (out?.t === 'jobs') await runJobs(s.api, out, 'paste', false);
  else s.api.apply(out, 'paste');
}
