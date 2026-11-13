// Frente S da leva 5 do engine="rust" (registro core/docs/modules/uo-bindings.md "Wave 5 — S: stories"): a entrada das
// histórias — o clique que entra/sai de uma célula, nota, caixa de texto ou cabeçalho/rodapé (o `mousedown` e o
// `dblclick` do `bindEvents` do TS, na mesma ordem), as setas entre as células (`tableNavigate`), Tab/Shift+Tab
// (`moveCell`), as teclas da seleção de células (`tableSelKey`) e o Esc de cada contexto. Carregado só com o motor Rust.
import type { ExecOutcome } from '@universal-office/write-session';
import {
  apply, ctxOf, exec, hfEditOf, hfPageOf, live, outerOf, pageAt, pointOn, query, run, sameHops, wireHops,
  type HfKindName, type StState,
} from './rust-stories.js';

/** Os eventos de uma resposta trocados (o `uw-hfmode` do TS leva `kind`; a sessão não, ED-104). */
export function mapEvents(o: ExecOutcome | null, f: (ev: { t: string } & Record<string, unknown>) => ({ t: string } & Record<string, unknown>) | null): ExecOutcome | null {
  if (!o || o.t !== 'done') return o;
  const events = o.events.map((e) => f(e as { t: string } & Record<string, unknown>)).filter((e): e is { t: string } & Record<string, unknown> => e !== null);
  return { ...o, events };
}

// ───────────────────────── os contextos (enter/exit) ─────────────────────────

/** `enterHeaderFooter(kind, page, silent)` (2624): a parte que a página mostra (criada na seção que a define, se falta). */
export function enterHf(st: StState, kind: HfKindName, page: number, silent = false): boolean {
  const l = live(st);
  const c = ctxOf(st);
  if (!l || !c || hfEditOf(st) || c.locked) return false;
  const n = l.dl.pages.length;
  const pg = Math.max(0, Math.min(page, n - 1));
  let hp = hfPageOf(st, pg);
  if (!hp) return false;
  let side = kind === 'header' ? hp.header : hp.footer;
  if (side.part === null) {
    const o = run(st, { t: 'headerFooter', v: { t: 'createHeaderFooterPart', kind, section: side.section, slot: side.slot } }, 'enterHeaderFooter');
    apply(st, o, 'enterHeaderFooter');
    hp = hfPageOf(st, pg);
    if (!hp) return false;
    side = kind === 'header' ? hp.header : hp.footer;
    if (side.part === null) return false;
  }
  const o = run(st, { t: 'context', v: { t: 'enterHeaderFooter', part: side.part, page: pg } }, 'enterHeaderFooter');
  return apply(st, mapEvents(o, (ev) => (ev.t === 'uw-hfmode' ? (silent ? null : { ...ev, active: true, kind }) : ev)), 'enterHeaderFooter');
}

/** `exitHeaderFooter(silent)` (2717). */
export function exitHf(st: StState, silent = false): boolean {
  if (!hfEditOf(st)) return false;
  const o = run(st, { t: 'context', v: { t: 'exitHeaderFooter' } }, 'exitHeaderFooter');
  return apply(st, mapEvents(o, (ev) => (ev.t === 'uw-hfmode' ? (silent ? null : { t: 'uw-hfmode', active: false }) : ev)), 'exitHeaderFooter');
}

/** `switchHfFocus(kind, page)` (13302): sai e entra na outra região/página. */
function switchHf(st: StState, kind: HfKindName, page: number): void {
  const hf = hfEditOf(st);
  if (!hf || (hf.kind === kind && hf.page === page)) return;
  exitHf(st);
  enterHf(st, kind, page);
}

/** `hfZoneAt` (13232) / `hfRegionAtClick` (13279): a faixa do cabeçalho/rodapé sob o ponteiro. */
function hfZone(st: StState, e: MouseEvent, inclusive: boolean): { kind: HfKindName; page: number } | null {
  const p = pageAt(st, e);
  if (!p) return null;
  const hp = hfPageOf(st, p.page);
  if (!hp) return null;
  if (inclusive ? p.y <= hp.header.bottom : p.y < hp.header.bottom) return { kind: 'header', page: p.page };
  if (inclusive ? p.y >= hp.footer.top : p.y > hp.footer.top) return { kind: 'footer', page: p.page };
  return null;
}

/** A caixa de texto sob o ponteiro (`hitTextbox`). */
function textboxAt(st: StState, e: MouseEvent): ReturnType<NonNullable<StState['sb']>['textboxAt']> {
  const l = live(st);
  const p = pageAt(st, e);
  if (!l || !p) return null;
  const hf = hfEditOf(st);
  if (hf && p.page !== hf.page) return null;
  const tol = 5 / (96 / 1440 * st.ed.el.zoom);
  try { return l.sb.textboxAt(l.s, l.dl.inner, p.page, p.x, p.y, tol); } catch { return null; }
}

/** O retângulo da caixa em edição (o `tbEdit.rectTwips` do TS) e a página. */
const tbRects = new WeakMap<StState, { page: number; rect: { x: number; y: number; w: number; h: number } }>();

/** `pointInTbRect(e)` (13451): dentro da caixa em edição, com 30 twips de folga. */
function pointInTb(st: StState, e: MouseEvent): boolean {
  const r = tbRects.get(st);
  if (!r || ctxOf(st)?.ctx.kind !== 'textbox') return false;
  const p = pointOn(st, e, r.page);
  const c = st.ed.el.canvases[r.page];
  if (!p || !c) return false;
  const cr = c.getBoundingClientRect();
  if (e.clientY < cr.top || e.clientY > cr.bottom) return false;
  const tol = 30;
  return p.x >= r.rect.x - tol && p.x <= r.rect.x + r.rect.w + tol && p.y >= r.rect.y - tol && p.y <= r.rect.y + r.rect.h + tol;
}

/** `enterTextbox(run, rect, page)` (2737): a caixa (a cabeça da cadeia) vira o escopo; sai da célula antes. */
export function enterTextbox(st: StState, tb: NonNullable<ReturnType<typeof textboxAt>>): boolean {
  const c = ctxOf(st);
  if (!c || c.locked || c.ctx.kind === 'textbox') return false;
  if (c.ctx.kind === 'cell') exec(st, { t: 'context', v: { t: 'exitCell' } }, 'exitCell');
  if (ctxOf(st)?.ctx.kind === 'note') exec(st, { t: 'context', v: { t: 'exitNote' } }, 'exitNote');
  // wave 6 (C): the TS `enterTextbox(focusRun)` by the clicked box's order in `contextTextboxRuns` (the session finds the
  // chain's head: boxes in cells, groups and boxes too — ED-102 closed)
  const o = run(st, { t: 'context', v: { t: 'enterTextboxAt', focus: tb.focus, page: tb.page } }, 'enterTextbox');
  const ok = apply(st, o, 'enterTextbox');
  if (ok && ctxOf(st)?.ctx.kind === 'textbox') tbRects.set(st, { page: tb.page, rect: tb.rect });
  st.ed.focusEditor();
  return ok;
}

/** `exitTextbox()` (2890). */
export function exitTextbox(st: StState): boolean {
  if (ctxOf(st)?.ctx.kind !== 'textbox') return false;
  tbRects.delete(st);
  return exec(st, { t: 'context', v: { t: 'exitTextbox' } }, 'exitTextbox');
}

/** `routeEngineClick(e)` (18753): a linha clicada decide a história (nota, célula, corpo). */
function routeClick(st: StState, e: MouseEvent): void {
  const c = ctxOf(st);
  const l = live(st);
  if (!c || !l || c.locked) return;
  const o = outerOf(c.ctx);
  if (o.kind === 'hf' || c.ctx.kind === 'textbox') return;
  const p = pageAt(st, e);
  if (!p) return;
  let at;
  try { at = l.sb.locateAt(l.s, l.dl.inner, p.page, p.x, p.y); } catch { at = null; }
  if (!at) return;
  if (at.kind === 'note') {
    const cur = c.ctx;
    if (cur.kind !== 'note' || cur.footnote !== at.footnote || cur.id !== at.id) {
      exec(st, { t: 'context', v: { t: 'enterNote', footnote: at.footnote, id: at.id } }, 'enterNote');
    }
    return;
  }
  if (c.ctx.kind === 'note') exec(st, { t: 'context', v: { t: 'exitNote' } }, 'exitNote');
  const now = ctxOf(st)?.ctx;
  if (at.kind === 'cell') {
    if (now && now.kind === 'cell' && now.anchor === at.anchor && sameHops(now.hops, at.hops)) return;
    exec(st, { t: 'context', v: { t: 'enterCell', anchor: at.anchor, hops: wireHops(at.hops) } }, 'enterCell');
  } else if (at.kind === 'body' && now?.kind === 'cell') {
    exec(st, { t: 'context', v: { t: 'exitCell' } }, 'exitCell');
  }
}

/** `dragCellSel(start, ev)` (21013): true quando há seleção de células depois. */
function dragCellSel(st: StState, e: MouseEvent): boolean {
  const l = live(st);
  const p = pageAt(st, e);
  if (!l) return false;
  if (p) {
    let o: ExecOutcome | null = null;
    try { o = l.sb.dragCellSel(l.s, l.dl.inner, p.page, p.x, p.y); } catch { o = null; }
    if (o) apply(st, o, 'dragCellSel');
  }
  return !!ctxOf(st)?.tableSel;
}

/**
 * O clique comum no escopo de agora (o fim do `mousedown` do TS): o da casca (`frontApi().defaultClick`: cursor, Shift,
 * triplo clique, arrasto); um arrasto que começa numa célula e passa para outra vira seleção de células.
 */
function defaultClick(st: StState, e: MouseEvent): void {
  const startCell = ctxOf(st)?.ctx.kind === 'cell';
  st.api?.defaultClick(e, startCell ? (ev) => dragCellSel(st, ev) : undefined);
}

// ───────────────────────── o mousedown ─────────────────────────

/**
 * O passo 3 do `mousedown` do TS (10200): clique numa caixa de texto entra nela (1 clique); fora da caixa em edição,
 * sai. A borda é da frente O (selecionar a forma). Quando muda o escopo, o resto do clique (o comum) é feito aqui.
 */
export function textboxPointer(st: StState, e: MouseEvent): boolean {
  const c = ctxOf(st);
  if (!c || e.button !== 0 || st.ed.el.protectedMode) return false;
  if (c.ctx.kind === 'textbox' && pointInTb(st, e)) return false;
  let changedScope = false;
  if (c.ctx.kind === 'textbox') { exitTextbox(st); changedScope = true; }
  const tb = textboxAt(st, e);
  if (tb && !tb.onBorder) {
    if (enterTextbox(st, tb)) changedScope = true;
  }
  if (!changedScope) return false;
  // o resto do mousedown no escopo novo (a região do cabeçalho, as células, a linha clicada, o cursor)
  lateRest(st, e);
  return true;
}

/** Os passos do `mousedown` do TS depois dos objetos (10264–10290) e o clique comum. */
function lateRest(st: StState, e: MouseEvent): void {
  const c = ctxOf(st);
  if (!c) return;
  const hf = hfEditOf(st);
  if (e.button === 0 && hf && c.ctx.kind !== 'textbox') {
    const region = hfZone(st, e, true);
    if (region && (region.kind !== hf.kind || region.page !== hf.page)) {
      switchHf(st, region.kind, region.page);
      const pos = st.api?.hitAt(e);
      if (pos) st.ed.setSelection(pos);
      st.ed.focusEditor();
      e.preventDefault();
      return;
    }
  }
  if (e.button === 0 && e.shiftKey && c.ctx.kind === 'cell') {
    if (dragCellSel(st, e)) { e.preventDefault(); st.ed.focusEditor(); return; }
  }
  // wave 7 (A2, f2-3-0|tables step 24): the TS mousedown clears the cell selection and enters/leaves the cell without a
  // scroll (`tableSel = null`, `exitCell`/`enterCellAt`); the shell's `apply` scrolled to the caret of the left cell's body
  // selection BEFORE the click's own hit, which then landed on another page
  noScroll(st, () => {
    if (e.button === 0 && ctxOf(st)?.tableSel && !e.shiftKey) exec(st, { t: 'table', v: { t: 'clearTableSelection' } }, 'clearTableSelection');
    if (e.button === 0 && !e.shiftKey) routeClick(st, e);
  });
  defaultClick(st, e);
}

/** `fn` with the shell's caret scroll (`scrollCaretIntoView` → `animateScrollTo`) shadowed. */
function noScroll(st: StState, fn: () => void): void {
  const el = st.ed.el as unknown as { animateScrollTo?: (top: number) => void };
  const own = Object.prototype.hasOwnProperty.call(el, 'animateScrollTo') ? el.animateScrollTo : undefined;
  el.animateScrollTo = () => {};
  try { fn(); } finally { if (own) el.animateScrollTo = own; else delete el.animateScrollTo; }
}

/**
 * Depois dos objetos (frente O): a região do cabeçalho/rodapé, Shift+clique em outra célula, a seleção de células
 * desfeita, a história da linha clicada. Só trata o clique quando há uma história em jogo (contexto fora do corpo,
 * seleção de células, ou a linha clicada é de célula/nota): o clique no corpo segue o da casca.
 */
export function latePointer(st: StState, e: MouseEvent): boolean {
  const c = ctxOf(st);
  const l = live(st);
  if (!c || !l) return false;
  let involved = c.ctx.kind !== 'body' || !!c.tableSel;
  if (!involved && e.button === 0 && !e.shiftKey && !c.locked) {
    const p = pageAt(st, e);
    let at = null;
    try { at = p ? l.sb.locateAt(l.s, l.dl.inner, p.page, p.x, p.y) : null; } catch { at = null; }
    involved = !!at && (at.kind === 'cell' || at.kind === 'note');
  }
  if (!involved) return false;
  lateRest(st, e);
  return true;
}

// ───────────────────────── o dblclick ─────────────────────────

/** O `dblclick` do TS (10456), na ordem: a região do cabeçalho, a caixa de texto, a zona do cabeçalho/rodapé, a nota. */
export function dblHook(st: StState, e: MouseEvent): boolean {
  const c = ctxOf(st);
  const ed = st.ed;
  if (!c) return false;
  const hf = hfEditOf(st);
  if (hf && c.ctx.kind !== 'textbox') {
    const region = hfZone(st, e, true);
    if (region && (region.kind !== hf.kind || region.page !== hf.page)) switchHf(st, region.kind, region.page);
  }
  const now = ctxOf(st);
  if (!now) return false;
  if (now.ctx.kind !== 'textbox' && !ed.el.protectedMode) {
    const tb = textboxAt(st, e);
    if (tb && enterTextbox(st, tb)) {
      const pos = st.api?.hitAt(e);
      if (pos) ed.setSelection(pos);
      return true;
    }
  }
  if (now.ctx.kind === 'textbox' && !pointInTb(st, e)) { exitTextbox(st); return true; }
  if (ed.el.view === 'print' && !hfEditOf(st) && now.ctx.kind !== 'textbox') {
    const zone = hfZone(st, e, false);
    if (zone) {
      if (ed.el.protectedMode) { ed.el.dispatchEvent(new CustomEvent('uw-blocked', { detail: { reason: 'protected' } })); return true; }
      enterHf(st, zone.kind, zone.page);
      const pos = hfEditOf(st) ? st.api?.hitAt(e) : null;
      if (pos) ed.setSelection(pos);
      return true;
    }
  }
  if (hfEditOf(st) && !hfZone(st, e, true)) { exitHf(st); return true; }
  // a referência abre a nota; o número da nota volta à referência
  const hit = st.api?.hitAt(e);
  const l = live(st);
  if (!hit || !l) return false;
  let nr = null;
  try { nr = l.sb.noteAt(l.s, hit.para, hit.offset); } catch { nr = null; }
  const cur = ctxOf(st)?.ctx;
  if (nr?.ref && cur?.kind !== 'note') { openNote(st, nr.ref.footnote, nr.ref.id); return true; }
  if (nr && cur?.kind === 'note' && nr.mark) { goToNoteRef(st, cur.footnote, cur.id); return true; }
  return false;
}

/** `openNote(kind, id)` (19155). */
export function openNote(st: StState, footnote: boolean, id: number): boolean {
  const c = ctxOf(st);
  if (!c || c.locked) return false;
  const o = run(st, { t: 'note', v: { t: 'open', footnote, id } }, 'openNote');
  apply(st, o, 'openNote');
  const n = ctxOf(st)?.ctx;
  return !!n && n.kind === 'note' && n.id === id && n.footnote === footnote;
}

/** `goToNoteRef(kind, id)` (19165). */
export function goToNoteRef(st: StState, footnote: boolean, id: number): boolean {
  // the TS: leave the note, then false when the kind has no reference with that id (the session's step is a no-op then)
  const refs = query<{ v?: { id: number }[] }>(st, { t: 'noteRefs', kind: footnote })?.v;
  if (refs && !refs.some((r) => r.id === id)) {
    if (ctxOf(st)?.ctx.kind === 'note') exec(st, { t: 'context', v: { t: 'exitNote' } }, 'exitNote');
    return false;
  }
  const before = JSON.stringify(st.ed.el.selection);
  const o = run(st, { t: 'note', v: { t: 'goToRef', footnote, id } }, 'goToNoteRef');
  apply(st, o, 'goToNoteRef');
  return !!o && o.t === 'done' && (JSON.stringify(st.ed.el.selection) !== before || ctxOf(st)?.ctx.kind === 'body');
}

// ───────────────────────── o teclado ─────────────────────────

const done = (e: KeyboardEvent): true => { e.preventDefault(); e.stopImmediatePropagation(); return true; };

/** As teclas da frente que o TS trata antes das outras (`tableSelKey`, as da célula, Tab, Ctrl+Home/End, notas). */
export function keyHook(st: StState, e: KeyboardEvent): boolean {
  const c = ctxOf(st);
  if (!c) return false;
  const ctrl = e.ctrlKey || e.metaKey;
  const k = e.key;
  // células selecionadas: setas/Shift+setas, Delete/Backspace, Esc (Tab desfaz a seleção e segue)
  if (c.tableSel) {
    const arrows: Record<string, 'left' | 'right' | 'up' | 'down'> = { ArrowLeft: 'left', ArrowRight: 'right', ArrowUp: 'up', ArrowDown: 'down' };
    if (arrows[k] && !e.ctrlKey && !e.altKey) {
      done(e);
      exec(st, { t: 'table', v: { t: 'tableSelArrow', key: arrows[k]!, shift: e.shiftKey } }, 'tableSelArrow');
      return true;
    }
    if (k === 'Escape') { done(e); exec(st, { t: 'table', v: { t: 'clearTableSelection' } }, 'clearTableSelection'); return true; }
    if (k === 'Delete' || k === 'Backspace') {
      done(e);
      exec(st, { t: 'table', v: { t: 'tableSelDelete', backspace: k === 'Backspace' } }, 'tableSelDelete');
      return true;
    }
    if (k === 'Tab') exec(st, { t: 'table', v: { t: 'clearTableSelection' } }, 'clearTableSelection');
  }
  const now = ctxOf(st)?.ctx;
  if (!now) return false;
  if (now.kind === 'cell' && !c.locked) {
    // Ctrl+Shift+Enter: dividir a tabela; Alt+Shift+↑/↓: mover a linha
    if (ctrl && e.shiftKey && k === 'Enter') { done(e); exec(st, { t: 'table', v: { t: 'splitTable' } }, 'splitTable'); return true; }
    if (e.altKey && e.shiftKey && (k === 'ArrowUp' || k === 'ArrowDown')) {
      done(e);
      exec(st, { t: 'table', v: { t: 'moveTableRow', dir: k === 'ArrowUp' ? -1 : 1 } }, 'moveTableRow');
      return true;
    }
  }
  // Ctrl+Alt+F / Ctrl+Alt+D: nota de rodapé / de fim
  if (ctrl && e.altKey && !e.shiftKey && (k.toLowerCase() === 'f' || k.toLowerCase() === 'd')) {
    done(e);
    insertNote(st, k.toLowerCase() === 'f');
    return true;
  }
  // Ctrl+Home/End numa célula (sem Shift): sai das células; o resto é o Ctrl+Home/End da casca
  if (ctrl && (k === 'Home' || k === 'End') && now.kind === 'cell' && !e.shiftKey) {
    exec(st, { t: 'context', v: { t: 'exitCell' } }, 'exitCell');
    return false;
  }
  // célula de tabela: Tab/Shift+Tab vai para a próxima/anterior célula
  if (k === 'Tab' && !ctrl && now.kind === 'cell') {
    done(e);
    const l = live(st);
    if (!l) return true;
    let o: ExecOutcome | null = null;
    try { o = l.sb.moveCell(l.s, l.dl.inner, e.shiftKey ? -1 : 1); } catch { o = null; }
    if (o) apply(st, o, 'moveCell');
    return true;
  }
  return false;
}

/** O Esc de cada contexto (depois das teclas da frente O: objeto selecionado). */
export function lateKeyHook(st: StState, e: KeyboardEvent): boolean {
  if (e.key !== 'Escape') return false;
  const c = ctxOf(st);
  if (!c) return false;
  if (c.ctx.kind === 'textbox') { done(e); exitTextbox(st); return true; }
  if (c.ctx.kind === 'note') { done(e); goToNoteRef(st, c.ctx.footnote, c.ctx.id); return true; }
  if (hfEditOf(st)) { done(e); exitHf(st); return true; }
  if (c.ctx.kind === 'cell') { done(e); exec(st, { t: 'context', v: { t: 'exitCell' } }, 'exitCell'); return true; }
  return false;
}

/** `moveCaret` do TS: `tableNavigate` primeiro (as setas entre células e entre o corpo e as tabelas). */
export function moveHook(st: StState, dir: string, extend: boolean): boolean {
  if (dir !== 'left' && dir !== 'right' && dir !== 'up' && dir !== 'down') return false;
  const l = live(st);
  if (!l) return false;
  let o: ExecOutcome | null = null;
  try { o = l.sb.tableNavigate(l.s, l.dl.inner, dir, extend); } catch { o = null; }
  if (!o) return false;
  apply(st, o, 'tableNavigate');
  return true;
}

/** `insertFootnote`/`insertEndnote` (19072): o id da nota nova, ou null. */
export function insertNote(st: StState, footnote: boolean, mark?: string): number | null {
  const o = run(st, { t: 'note', v: { t: 'insert', footnote, ...(mark !== undefined ? { mark } : {}) } }, footnote ? 'insertFootnote' : 'insertEndnote');
  if (!apply(st, o, footnote ? 'insertFootnote' : 'insertEndnote') || !changed2(o)) return null;
  const c = ctxOf(st)?.ctx;
  return c && c.kind === 'note' ? c.id : null;
}

function changed2(o: ExecOutcome | null): boolean {
  return !!o && o.t === 'done' && o.stories.length > 0;
}
