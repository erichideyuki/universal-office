// Handlers da frente R da leva 5 do engine="rust" (comentários, revisões, campos/referências, localizar, ortografia, estilos, área de transferência sem evento, teclas Alt): cada chave é um nome de
// ROUTES_REVIEW.public/private (engine-routes-review.ts). Carregado só com o motor Rust (import estático de rust-engine.ts).
// Registro: core/docs/modules/uo-bindings.md "Wave 5 — R". Os módulos rust-review-*.ts fazem o trabalho; aqui eles são
// juntados e os ganchos do motor (tecla, ponteiro, pintura, documento novo, desconectar, cópia pelo teclado) registrados uma vez.
import type { RustEditor } from './rust-engine.js';
import { clipboardHandlers, keyHook } from './rust-review-clipboard.js';
import { Ctx, apiOf, type Pos } from './rust-review-core.js';
import { findHandlers, type FindView } from './rust-review-find.js';
import { OVERLAY_METHODS, overlayHandlers, paintExtraRanges } from './rust-review-overlays.js';
import { authorSync, commentHandlers, revisionHandlers, runsOf } from './rust-review-revisions.js';
import { spellHandlers } from './rust-review-spell.js';
import { styleHandlers } from './rust-review-styles.js';

type Handlers = Record<string, (...args: never[]) => unknown>;

export function reviewHandlers(ed: RustEditor): Record<string, (...args: never[]) => unknown> {
  const api = apiOf(ed);
  const c = new Ctx(ed, api);
  const view: FindView = { show: false, results: [] };
  const author = authorSync(c);
  const find = findHandlers(c, view);
  const spell = spellHandlers(c);
  const clip = clipboardHandlers(c);
  const out: Handlers = {
    ...revisionHandlers(c, author.sync),
    ...commentHandlers(c),
    ...find,
    ...styleHandlers(c),
    ...spell.handlers,
    ...clip.handlers,
    ...overlayHandlers(c, view),
  };
  const ctrlClick = out['__ctrlClick'] as unknown as (e: MouseEvent, hit: Pos) => boolean;
  delete out['__ctrlClick'];
  delete out['__scrollSel'];
  // the harness's probe (editor_switch_review.mjs): on the engine object, not on the element
  const probe = out['__spellProbe'];
  delete out['__spellProbe'];
  if (!api) return out;
  Object.defineProperty(ed, 'reviewProbe', { value: { spell: probe, spellState: spell.state, view }, configurable: true });
  // the hooks (wave 5 fronts: rust-engine.ts `hooks`)
  const hooks = (ed as unknown as { hooks?: RustEditor['hooks'] }).hooks;
  if (hooks) {
    // the element's author before any key or click that can record a revision (first: the other fronts' keys edit too)
    hooks.key.unshift(() => { author.sync(); return false; });
    hooks.pointer.unshift(() => { author.sync(); return false; });
    hooks.key.push(keyHook(c, out, clip.pushDefaults));
    hooks.pointer.push((e, hit) => {
      if (ctrlClick(e, hit)) { e.preventDefault(); return true; }
      // a click on commented text tells the comments pane (the TS `uw-comment-click`), then the click goes on
      if (e.button === 0 && c.s) {
        const r = runsOf(c, hit.para).find((x) => hit.offset >= x.a && hit.offset < x.z && x.commentIds?.length);
        if (r) c.el?.dispatchEvent(new CustomEvent('uw-comment-click', { detail: { ids: r.commentIds } }));
      }
      return false;
    });
    hooks.paint.push((ctx, page) => {
      // the TS order (paintEngine): marks, (bookmarks: ED-310), spelling, search hits, (selection)
      const el = c.el as unknown as Record<string, ((ctx: CanvasRenderingContext2D, pg: number) => void) | undefined> | null;
      for (const m of OVERLAY_METHODS) el?.[m]?.(ctx, page);
    });
    // the extra ranges of a non-contiguous selection: with the selection, over it (the TS `extras()` in paintEngineSelection)
    hooks.overlay.push((ctx, page) => paintExtraRanges(c, ctx, page));
    hooks.adopt.push(() => { view.show = false; view.results = []; clip.adopt(); author.adopt(); });
    // the TS disconnectedCallback: the spell checker's reference and listener go
    hooks.park.push(() => spell.dispose());
    // a keyboard copy/cut (the clipboard event): the Office Clipboard pane's listeners (the TS `rememberClip`)
    hooks.copied.push(() => clip.notify());
  }
  author.install();
  clip.installDefaults();
  return out;
}
