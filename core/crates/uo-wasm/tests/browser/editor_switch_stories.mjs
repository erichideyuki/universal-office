// Wave 5, frontier S of the editor switch (record core/docs/modules/uo-bindings.md "Wave 5 — S: stories"): tables,
// headers/footers, notes and the caret inside a text box of `<universal-write engine="rust">` against the TS engine in a
// real headless Chromium.
//
//   UO_TS_ROOT=<frozen checkout> UO_WASM_PATH=<session .wasm> [UO_CHROMIUM=...] [UO_ES_SOAK=30] [UO_ES_ONLY=<scenario regex>]
//   node crates/uo-wasm/tests/browser/editor_switch_stories.mjs --mode stories|memory|chrome|all
//
// `stories`: per scenario the TS element first, then the Rust one; the SAME script (element API calls, real CDP mouse and
// keyboard events at points computed once in the TS run as page twips, so both runs click the same place of the same
// layout; bar commands by id through the chrome's registry, their dialogs filled and accepted; `prep` builds a document
// in a TS run whose saved bytes both runs then load); after each step the facets (selection, caret rect relative to its page, the uw-* events with their detail, the
// toolbar state run/para AND table, the contexts editingTableCell/editingHeaderFooter/editingNote/editingTextbox, isDirty);
// at the end the page count and sizes, page thumbnails, the screen canvases (the TS overlays of other frontiers off) and the
// saved .docx part by part. Every difference not covered by DECLARED fails, and so does any page error.
// `memory` (count build): create → new document → table, cell typing, Tab, header, footnote → save → remove, UO_ES_SOAK
// cycles; sessions/shell engines/handles 0 after each remove, live bytes one value after warm-up, memory.buffer flat.
// `chrome`: TS against Rust in five contexts (body, cell, row selected, header, footnote): supported/enabled/checked of
// every routed command id of the frontier, the visible tabs, the state of named ribbon buttons (a missing button fails);
// the declared remainder (Draw Table, Eraser) disabled.
// Declared duplicate: the page helpers are copy-adapted from editor_switch.mjs (as editor_switch_review.mjs; gate item: move
// them into the lib once the three frontiers have landed).
import { createRequire } from 'node:module';
import { pathToFileURL } from 'node:url';
import { join } from 'node:path';
import { writeFileSync } from 'node:fs';
import { setup, core } from './editor_switch_lib.mjs';

const root = process.env.UO_TS_ROOT;
if (!root) throw new Error('set UO_TS_ROOT to the frozen TS checkout');
const arg = (name, dflt) => { const i = process.argv.indexOf(name); return i >= 0 ? process.argv[i + 1] : dflt; };
const MODE = arg('--mode', 'all');
const SOAK = Number(process.env.UO_ES_SOAK ?? 30);
const ONLY = process.env.UO_ES_ONLY ? new RegExp(process.env.UO_ES_ONLY) : null;
const target = process.env.CARGO_TARGET_DIR ?? join(core, 'target');
const wasmPath = process.env.UO_WASM_PATH ?? join(target, 'es-stories/session.wasm');
const outDir = join(target, 'es-stories-run');
const { unzipSync } = createRequire(join(root, 'write/package.json'))('fflate');

const FIX = 'f7b__7__1|f7b__7__4|f7b__7__5|f7b__7__6|f7b__7__7|f7b__7__9|f7d__7__8|f7c__7__13|f8__7__5|f8__7__3|f7d__7__0|f7d__7__1|f7b__7__10';
// the bar commands by id: the registry of the element's chrome (chrome.ts keeps it private) is caught when the chrome adds
// its commands (a harness-only patch of the shared CommandRegistry; the element's code is unchanged)
const entryExtra = `import { CommandRegistry } from '@universal-office/ui';
{ const add0 = CommandRegistry.prototype.add; CommandRegistry.prototype.add = function (...c) { window.__lastReg = this; (window.__adds ??= []).push(c.map((x) => x.id)); return add0.apply(this, c); }; }`;
const env = await setup({ root, wasmPath, outDir, stride: 1, only: FIX, entryExtra });
const fileOf = (n) => env.label(n).replace(/^.*\//, '').replace(/\.docx$/, '');
const docOf = (want) => { for (const n of env.docsByName.keys()) if (fileOf(n) === want) return n; throw new Error(`no document ${want}`); };
let failures = 0;
const fail = (m) => { failures++; console.log(`FAIL ${m}`); };

/** the page-side helpers (evaluated in each new page) */
const helpers = () => {
  const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
  const frame = () => new Promise((r) => requestAnimationFrame(() => r()));
  const fnv = (u8) => { let h = 0x811c9dc5; for (let i = 0; i < u8.length; i++) h = Math.imul(h ^ u8[i], 0x01000193); return (h >>> 0).toString(16).padStart(8, '0'); };
  const EVENTS = ['uw-change', 'uw-dirty-change', 'uw-input', 'uw-protected', 'uw-cellmode', 'uw-notemode', 'uw-hfmode', 'uw-tbmode',
    'uw-sort-error', 'uw-blocked', 'uw-styles-change', 'uw-borderpen'];
  /** the TS overlays the Rust shell does not paint (other frontiers' or declared): off on the TS element for the screen */
  const OVERLAYS = ['paintEngineMarks', 'paintBookmarkMarks', 'paintEngineSpelling', 'paintSearchHits', 'paintTableAdorners', 'paintTocControl', 'paintMultiSelection', 'paintEngineInlines'];
  const log = [];
  const canon = (v) => (Array.isArray(v) ? v.map(canon) : v && typeof v === 'object'
    ? Object.fromEntries(Object.entries(v).filter(([, x]) => x !== undefined && x !== null).sort(([a], [b]) => (a < b ? -1 : 1)).map(([k, x]) => [k, canon(x)]))
    : typeof v === 'number' ? +v.toFixed(3) : v);
  const J = (v) => JSON.stringify(canon(v));
  const fetchDoc = async (n) => new Uint8Array(await (await fetch(`/docs/${n}`)).arrayBuffer());
  const PX = 96 / 1440;
  const api = {
    el: null,
    reg: null,
    async make(engine, o = {}) {
      const el = document.createElement('universal-write');
      if (engine === 'rust') el.setAttribute('engine', 'rust');
      el.setAttribute('fonts', '/fonts');
      if (o.topbar) { el.setAttribute('topbar', ''); el.setAttribute('toolbar', 'ribbon'); el.setAttribute('lang', 'en'); }
      el.style.cssText = `display:block;width:1000px;height:${o.topbar ? 950 : 800}px`;
      const ready = new Promise((r) => el.addEventListener('uw-ready', r, { once: true }));
      window.__lastReg = null;
      window.__adds = [];
      // the bars' preferences (remembered table size, recents) start empty on both runs
      if (o.topbar) try { localStorage.clear(); } catch { /* none */ }
      document.body.appendChild(el);
      await ready;
      if (el.rust) el.rust.tsShading = true; // wave 7c (V, fix round 1): the TS field shading/comment look (ED-304: Word's is the default)
      for (const t of EVENTS) el.addEventListener(t, (e) => { log.push(`${t} ${JSON.stringify(e.detail ?? null)}`); });
      api.el = el;
      api.reg = null;
      if (o.topbar) {
        // the chrome is loaded on demand: its registry is the one created after this element
        for (let i = 0; i < 200 && !window.__lastReg; i++) await sleep(25);
        api.reg = window.__lastReg;
        if (!api.reg) throw new Error('no command registry (the chrome did not load)');
      }
      return true;
    },
    async settle() {
      const el = api.el;
      if (el.rust) await el.rust.idle();
      else { await document.fonts.ready; await sleep(300); }
      await frame();
    },
    async load(n, b64) {
      window.__seed(1);
      if (b64) api.el.loadDocx(Uint8Array.from(atob(b64), (c) => c.charCodeAt(0)), { protected: false });
      else if (n) api.el.loadDocx(await fetchDoc(n), { protected: false });
      else api.el.newDocument();
      await api.settle();
      api.el.refresh();
      await api.settle();
      api.el.scrollTop = 0;
      await frame();
      log.length = 0;
      return api.el.layoutPageCount();
    },
    remove() { api.el?.remove(); api.el = null; },
    drain() { const out = log.slice(); log.length = 0; return out; },
    focus() { api.el.focusEditor(); },
    /** page twips → client coordinates (the canvas of the page now: scroll and zoom of this run) */
    client(p) {
      const el = api.el;
      const c = el.canvases[p.page];
      if (!c) return null;
      if (p.scroll !== undefined) el.scrollTop = p.scroll;
      const r = c.getBoundingClientRect();
      return { x: r.left + p.x * PX * el.zoom, y: r.top + p.y * PX * el.zoom };
    },
    /** the TS run: page twips of a client point (the page holding it) */
    toPage(x, y) {
      const el = api.el;
      for (let pg = 0; pg < el.canvases.length; pg++) {
        const r = el.canvases[pg].getBoundingClientRect();
        if (y < r.top || y > r.bottom) continue;
        return { page: pg, x: (x - r.left) / (PX * el.zoom), y: (y - r.top) / (PX * el.zoom), scroll: el.scrollTop };
      }
      return null;
    },
    // ── the targets (TS run only: the TS engine's layout) ──
    cellPoint(r, c, nth = 0) {
      const el = api.el;
      const pages = el.eng?.result.pages ?? [];
      let k = 0;
      for (let pg = 0; pg < pages.length; pg++) {
        for (const b of pages[pg].cells ?? []) {
          const m = /^b(\d+)\/r(\d+)\/c(\d+)$/.exec(b.ref);
          if (!m || +m[2] !== r || +m[3] !== c) continue;
          if (k++ < nth) continue;
          return { page: pg, x: b.x + Math.min(b.w / 2, 240), y: b.y + Math.min(b.h / 2, 200) };
        }
      }
      return null;
    },
    /** the header (or footer) zone of page `pg`: the middle of the margin band */
    hfPoint(kind, pg = 0) {
      const el = api.el;
      const p = el.eng?.result.pages[pg];
      if (!p) return null;
      const sp = el.sectionOfPage(pg)?.sectPr ?? el.bodyState().model.sectPr;
      return kind === 'header' ? { page: pg, x: p.widthTw / 2, y: sp.marginTopTwips / 3 } : { page: pg, x: p.widthTw / 2, y: p.heightTw - sp.marginBottomTwips / 3 };
    },
    /** the middle of the body text column, a third down page `pg` */
    bodyPoint(pg = 0) {
      const el = api.el;
      const p = el.eng?.result.pages[pg];
      if (!p) return null;
      for (const line of el.eng.lines) if (line.page === pg && line.loc.kind === 'body') return { page: pg, x: line.line.x + Math.min(line.line.width, 2000) / 2, y: line.line.y + line.line.height / 2 };
      return { page: pg, x: p.widthTw / 2, y: p.heightTw / 3 };
    },
    /** the first note line (the notes area) */
    noteLinePoint() {
      const el = api.el;
      for (const line of el.eng?.lines ?? []) if (line.loc.kind === 'note') return { page: line.page, x: line.line.x + 200, y: line.line.y + line.line.height / 2 };
      return null;
    },
    /** the first note reference in the body: the middle of its character */
    noteRefPoint() {
      const el = api.el;
      const ps = el.bodyState?.().model.paragraphs ?? el.model.paragraphs;
      for (let p = 0; p < ps.length; p++) {
        let acc = 0;
        for (const run of ps[p].runs) {
          if (run.noteRef) {
            const a = el.caretRectAt({ para: p, offset: acc }), b = el.caretRectAt({ para: p, offset: acc + run.text.length });
            if (a && b) return api.toPage((a.x + b.x) / 2, a.y + a.h / 2);
          }
          acc += run.text.length;
        }
      }
      return null;
    },
    /** the first text box with content: inside it, near its top-left text */
    textboxPoint(nth = 0, edge = undefined) {
      const el = api.el;
      const pages = el.eng?.result.pages ?? [];
      let k = 0;
      for (let pg = 0; pg < pages.length; pg++) {
        for (const o of pages[pg].objects ?? []) {
          if (!o.run?.textbox || !o.content || o.hf) continue;
          // a box of a top-level body paragraph (the session enters no box inside a table cell: S-G2)
          if (!el.model.paragraphs.some((pp) => pp.runs.includes(o.run))) continue;
          if (k++ < nth) continue;
          // `edge`: px outside the left edge (negative: inside), at the middle of the height (the border band)
          if (edge !== undefined) return { page: pg, x: o.x - edge / ((96 / 1440) * el.zoom), y: o.y + o.h / 2 };
          return { page: pg, x: o.x + Math.min(o.w / 2, 400), y: o.y + Math.min(o.h / 2, 300) };
        }
      }
      return null;
    },
    /** the TS caret point of a body position (x + 0.5, the middle of the caret: editor_switch.mjs `clickTarget`) */
    caretPoint(para, offset) {
      const c = api.el.caretRectAt({ para, offset });
      return c ? api.toPage(c.x + 0.5, c.y + c.h / 2) : null;
    },
    templateId(gallery) { return api.el.headerFooterTemplates(gallery)[0]?.id ?? null; },
    async call(m, a) {
      const r = api.el[m](...(a ?? []));
      const v = r && typeof r.then === 'function' ? await r : r;
      await api.settle();
      return v === undefined ? null : J(v);
    },
    /** where the keyboard goes now (a probe step: the focused element through the shadow roots) */
    focusPath() { const out = []; let d = document; while (d && d.activeElement) { const a = d.activeElement; out.push(a.tagName.toLowerCase() + (a.className && typeof a.className === 'string' ? '.' + a.className.split(' ')[0] : '')); d = a.shadowRoot; } const i = api.el.input; let n = i, inert = false; while (n) { if (n.inert) inert = true; n = n.parentNode ?? n.host; } return out.join('>') + (i ? ` ro=${i.readOnly} dis=${i.disabled} inert=${inert} val=${JSON.stringify(i.value)}` : ''); },
    /** text through the proxy's input event (what the browser does on a key: used after the dialogs below) */
    async inputText(t) { const i = api.el.input; i.value = t; i.dispatchEvent(new Event('input', { bubbles: true })); await api.settle(); },
    /** the open dialogs of the element (the bars' dialogs) */
    openDialogs() { return [...api.el.shadowRoot.querySelectorAll('dialog')].filter((d) => d.open); },
    /** a dialog's form: its title, every field (name, value or checked, disabled) and the action buttons */
    formOf(d) {
      const fields = [...d.querySelectorAll('input,select,textarea')].map((i) => [i.name || i.type, (i.type === 'checkbox' || i.type === 'radio') ? `${i.value}:${i.checked}` : i.value, i.disabled ? 'off' : 'on']);
      const actions = [...d.querySelectorAll('[data-action]')].map((b) => b.dataset.action);
      return { title: d.querySelector('.uo-dialog-head')?.textContent.trim() ?? '', fields, actions };
    },
    /**
     * A bar command by id through the chrome's registry (as a button or a menu item: `reg.run`); `dlg` drives the dialog it
     * opens: `set` { field name → value (a radio: the value to check; a checkbox: true/false) }, `action` (default the
     * primary button). The answer: enabled/checked before the run, the dialog's form as opened, `no dialog` when none.
     */
    async cmd(id, dlg) {
      const reg = api.reg;
      if (!reg) return 'no registry';
      const before = { enabled: reg.enabled(id), checked: reg.checked(id), supported: reg.supported(id) };
      let r;
      try { r = reg.run(id); } catch (e) { return J({ ...before, threw: String(e?.message ?? e) }); }
      let form;
      if (dlg) {
        let d = null;
        for (let i = 0; i < 40 && !d; i++) { await sleep(25); d = api.openDialogs().pop() ?? null; }
        if (!d) form = 'no dialog';
        else {
          form = api.formOf(d);
          for (const [n, v] of Object.entries(dlg.set ?? {})) {
            const els = [...d.querySelectorAll(`[name="${n}"]`)];
            if (!els.length) { form.missing = (form.missing ?? []).concat(n); continue; }
            for (const x of els) {
              if (x.type === 'radio') { if (x.value !== String(v)) continue; x.checked = true; }
              else if (x.type === 'checkbox') x.checked = !!v;
              else x.value = String(v);
              x.dispatchEvent(new Event('input', { bubbles: true }));
              x.dispatchEvent(new Event('change', { bubbles: true }));
            }
          }
          const b = dlg.action ? d.querySelector(`[data-action="${dlg.action}"]`) : d.querySelector('.uo-pill-primary');
          if (!b) form.noAction = dlg.action ?? 'primary';
          else b.click();
          for (let i = 0; i < 80 && api.openDialogs().length; i++) await sleep(25);
          if (api.openDialogs().length) { form.leftOpen = api.openDialogs().map((x) => api.formOf(x).title); for (const x of api.openDialogs()) x.close(); }
        }
      }
      if (r && typeof r.then === 'function') await Promise.race([r, sleep(3000)]);
      await sleep(50);
      await api.settle();
      return J({ ...before, ...(form !== undefined ? { form } : {}) });
    },
    async facets() {
      await frame();
      const el = api.el;
      const sel = el.getSelection();
      const c = el.caretRectAt(sel.focus);
      let caret = null;
      if (c) {
        const cs = [...el.shadowRoot.querySelectorAll('canvas.page')];
        const i = cs.findIndex((cv) => { const r = cv.getBoundingClientRect(); return c.y >= r.top - 0.01 && c.y < r.bottom; });
        if (i >= 0) { const r = cs[i].getBoundingClientRect(); caret = { page: i, x: +(c.x - r.left).toFixed(3), y: +(c.y - r.top).toFixed(3), h: +c.h.toFixed(3) }; }
      }
      const st = el.getSelectionState();
      return {
        sel, caret, events: api.drain(), run: J(st.run ?? {}), para: J(st.para ?? {}), table: J(st.table ?? null),
        ctx: J({ cell: el.editingTableCell(), hf: el.editingHeaderFooter(), note: el.editingNote(), tb: el.editingTextbox() }),
        dirty: el.isDirty(),
      };
    },
    /**
     * The screen canvases now (also mid-scenario: a `{ k: 'screen' }` step, so the overlays of a mode — the header/footer
     * bands and the dim layer, Show Document Text off, a selection of cells, the gridlines — are compared in pixels): the
     * caret off, the TS overlays of the other frontiers off, on the Rust side only this frontier's paint hooks.
     */
    async screen() {
      const el = api.el;
      const cs = [...el.shadowRoot.querySelectorAll('canvas.page')];
      el.scrollTop = 0;
      await frame(); await frame();
      el.ensureVisiblePainted();
      window.clearInterval(el.caretTimer);
      el.caretVisible = false;
      if (!el.rust) for (const m of OVERLAYS) el[m] = () => {};
      // the Rust side: only this frontier's paint hooks (the other frontiers paint the overlays the TS has off here)
      const hk = el.rust?.hooks;
      const kept = hk ? { paint: hk.paint.slice(), overlay: hk.overlay.slice() } : null;
      // wave 7 (B): front O's handles too (the TS paintFloatAndCaret is not among the TS overlays turned off: ED-106 was this filter)
      if (hk) for (const k of ['paint', 'overlay']) hk[k].splice(0, hk[k].length, ...kept[k].filter((f) => f.front === 'S' || f.front === 'O'));
      el.paint();
      await frame();
      const screen = cs.map((cv) => (cv.width > 1 ? fnv(new Uint8Array(cv.getContext('2d').getImageData(0, 0, cv.width, cv.height).data.buffer)) : '-'));
      const shots = window.__dump ? cs.map((cv) => (cv.width > 1 ? cv.toDataURL('image/png') : null)) : [];
      if (hk) for (const k of ['paint', 'overlay']) hk[k].splice(0, hk[k].length, ...kept[k]);
      return { screen, shots };
    },
    async end() {
      const el = api.el;
      el.refresh();
      await api.settle();
      const cs = [...el.shadowRoot.querySelectorAll('canvas.page')];
      const sizes = cs.map((cv) => `${cv.style.width}x${cv.style.height}`);
      const thumbs = [];
      for (let i = 0; i < cs.length; i++) {
        const t = document.createElement('canvas');
        const ok = el.paintPageThumbnail(i, t, parseFloat(cs[i].style.width));
        thumbs.push(ok ? fnv(new Uint8Array(t.getContext('2d').getImageData(0, 0, t.width, t.height).data.buffer)) : 'none');
        t.width = t.height = 0;
      }
      const { screen, shots } = await api.screen();
      const bytes = el.saveDocx();
      let bin = '';
      for (let i = 0; i < bytes.length; i += 0x8000) bin += String.fromCharCode(...bytes.subarray(i, i + 0x8000));
      return { pages: cs.length, sizes, thumbs, screen, shots, docx: btoa(bin) };
    },
    stats() { return window.__e.UniversalWriteElement.engineStats(); },
    async soakOnce() {
      await api.make('rust');
      await api.load(null);
      const el = api.el;
      el.focusEditor();
      el.insertTable(2, 2);
      const key = (k, o = {}) => el.input.dispatchEvent(new KeyboardEvent('keydown', { key: k, code: k, bubbles: true, cancelable: true, ...o }));
      const type = (t) => { el.input.value = t; el.input.dispatchEvent(new Event('input', { bubbles: true })); };
      type('a'); key('Tab'); type('b');
      el.enterHeaderFooter('header', 0); type('h'); key('Escape');
      el.insertFootnote(); type('n'); key('Escape');
      el.saveDocx();
      const during = api.stats();
      api.remove();
      await sleep(0);
      return { during, after: api.stats() };
    },
    // the ribbon (chrome mode)
    button(id) {
      const find = (n) => { if (n.dataset?.a === id) return n; for (const c of n.children ?? []) { const f = find(c); if (f) return f; } if (n.shadowRoot) return find(n.shadowRoot); return null; };
      return find(api.el.shadowRoot);
    },
    buttonState(id) { const bt = api.button(id); return bt ? { disabled: !!(bt.disabled || bt.getAttribute?.('aria-disabled') === 'true' || bt.classList?.contains('disabled')) } : null; },
    tabs() {
      const out = [];
      const walk = (n) => { if (n.getAttribute?.('role') === 'tab' && !n.hidden && getComputedStyle(n).display !== 'none') out.push(n.textContent.trim()); for (const c of n.children ?? []) walk(c); if (n.shadowRoot) walk(n.shadowRoot); };
      walk(api.el.shadowRoot);
      return out;
    },
  };
  window.__es = api;
  window.addEventListener('error', (e) => { (window.__stacks ??= []).push(String(e.error?.stack ?? e.message).slice(0, 900)); });
  return true;
};
const ev = (f, a) => env.page.evaluate(f, a);

// ── the scenarios ───────────────────────────────────────────────────────────────────────────────
// A step: { k: 'api', m, a } | { k: 'press', v } | { k: 'type', v } | { k: 'click' | 'dblclick' | 'shiftclick', at } |
// { k: 'drag', at, to }; `at`/`to` are targets: { t: 'cell', r, c, nth } | { t: 'hf', kind, page } | { t: 'body', page } |
// { t: 'noteLine' } | { t: 'noteRef' } | { t: 'textbox', nth } | { t: 'template', gallery } (an id, for the API).
const T1 = [
  { k: 'type', v: 'Antes' }, { k: 'press', v: 'Enter' },
  { k: 'api', m: 'insertTable', a: [3, 3] },
  { k: 'type', v: 'a' }, { k: 'press', v: 'Tab' }, { k: 'type', v: 'b' }, { k: 'press', v: 'Tab' }, { k: 'press', v: 'Tab' },
  { k: 'press', v: 'Tab' }, { k: 'press', v: 'Tab' }, { k: 'press', v: 'Tab' }, { k: 'press', v: 'Tab' }, { k: 'press', v: 'Tab' },
  { k: 'type', v: 'z' }, { k: 'press', v: 'Tab' }, { k: 'type', v: 'nova' }, { k: 'press', v: 'Shift+Tab' },
  { k: 'press', v: 'ArrowRight' }, { k: 'press', v: 'ArrowDown' }, { k: 'press', v: 'ArrowDown' }, { k: 'press', v: 'ArrowUp' },
  { k: 'press', v: 'ArrowUp' }, { k: 'press', v: 'ArrowLeft' },
  { k: 'click', at: { t: 'cell', r: 1, c: 1 } }, { k: 'type', v: 'q' },
  { k: 'drag', at: { t: 'cell', r: 0, c: 0 }, to: { t: 'cell', r: 1, c: 1 } }, { k: 'screen' },
  { k: 'api', m: 'getTableState' }, { k: 'api', m: 'mergeCells' }, { k: 'api', m: 'splitCell', a: [2, 1] },
  { k: 'api', m: 'insertRowAbove' }, { k: 'api', m: 'insertColumnRight' }, { k: 'api', m: 'deleteRow' },
  { k: 'api', m: 'toggleRepeatHeaderRows' }, { k: 'api', m: 'autoFitTable', a: ['window'] }, { k: 'api', m: 'setCellShading', a: ['FFC000'] },
  { k: 'press', v: 'Control+z' }, { k: 'press', v: 'Control+z' }, { k: 'press', v: 'Control+z' }, { k: 'press', v: 'Control+y' },
  { k: 'click', at: { t: 'cell', r: 0, c: 0 } }, { k: 'press', v: 'Shift+ArrowRight' }, { k: 'press', v: 'Shift+ArrowDown' },
  { k: 'press', v: 'Escape' }, { k: 'api', m: 'selectRow' }, { k: 'screen' }, { k: 'api', m: 'getTableState' }, { k: 'press', v: 'Delete' },
  { k: 'press', v: 'Escape' }, { k: 'press', v: 'Control+End' },
  { k: 'click', at: { t: 'cell', r: 2, c: 2 } }, { k: 'api', m: 'insertFormula', a: ['=SUM(ABOVE)'] }, { k: 'api', m: 'deleteTable' },
];
const T2 = [
  { k: 'click', at: { t: 'cell', r: 0, c: 0 } }, { k: 'type', v: 'x' }, { k: 'press', v: 'Tab' }, { k: 'press', v: 'ArrowDown' },
  { k: 'press', v: 'ArrowDown' }, { k: 'press', v: 'ArrowUp' }, { k: 'press', v: 'PageDown' }, { k: 'press', v: 'PageUp' },
  { k: 'press', v: 'Shift+Tab' }, { k: 'press', v: 'Escape' }, { k: 'api', m: 'getTableState' },
];
const H1 = [
  { k: 'type', v: 'corpo' },
  { k: 'dblclick', at: { t: 'hf', kind: 'header', page: 0 } }, { k: 'type', v: 'cab' }, { k: 'screen' },
  { k: 'dblclick', at: { t: 'hf', kind: 'footer', page: 0 } }, { k: 'type', v: 'rod' },
  { k: 'api', m: 'getHeaderFooterOptions' }, { k: 'api', m: 'setPageNumberFormat', a: [{ format: 'upperRoman' }] },
  { k: 'api', m: 'getPageNumberFormat' }, { k: 'api', m: 'goToHeaderFooter', a: ['header'] }, { k: 'type', v: '!' },
  { k: 'api', m: 'setDifferentFirstPage', a: [true] }, { k: 'api', m: 'getHeaderFooterOptions' }, { k: 'type', v: 'p1' },
  { k: 'press', v: 'Escape' }, { k: 'click', at: { t: 'body', page: 0 } }, { k: 'type', v: 'x' },
  { k: 'api', m: 'enterHeaderFooter', a: ['footer', 0] }, { k: 'api', m: 'insertHeaderFooterTemplate', a: ['pageBottom', { t: 'template', gallery: 'pageBottom' }] },
  { k: 'dblclick', at: { t: 'body', page: 0 } }, { k: 'api', m: 'editingHeaderFooter' },
];
const N1 = [
  { k: 'type', v: 'texto' }, { k: 'press', v: 'Control+Alt+f' }, { k: 'type', v: 'nota' }, { k: 'press', v: 'Escape' },
  { k: 'type', v: ' mais' }, { k: 'dblclick', at: { t: 'noteRef' } }, { k: 'type', v: '2' }, { k: 'press', v: 'Escape' },
  { k: 'api', m: 'insertEndnote' }, { k: 'type', v: 'fim' }, { k: 'press', v: 'Escape' },
  { k: 'api', m: 'nextNote', a: ['footnote', 1] }, { k: 'api', m: 'noteCount', a: ['footnote'] },
  { k: 'api', m: 'setNoteOptions', a: ['footnote', { numberFormat: 'lowerRoman' }] }, { k: 'api', m: 'getNoteOptions', a: ['footnote'] },
  { k: 'api', m: 'convertNotes', a: ['toEndnotes'] }, { k: 'api', m: 'noteCount', a: ['endnote'] },
];
const N2 = [
  { k: 'dblclick', at: { t: 'noteRef' } }, { k: 'type', v: 'k' }, { k: 'press', v: 'Escape' },
  { k: 'click', at: { t: 'noteLine' } }, { k: 'type', v: 'j' }, { k: 'click', at: { t: 'body', page: 0 } }, { k: 'api', m: 'editingNote' },
];
/** saving with the caret inside a text box (the TS materializes empty PAGE/NUMPAGES/DATE fields on entering) */
const X2 = [{ k: 'click', at: { t: 'textbox', nth: 0 } }, { k: 'type', v: 'z' }];
const X1 = [
  { k: 'click', at: { t: 'textbox', nth: 0 } }, { k: 'type', v: 'tb' }, { k: 'press', v: 'Shift+ArrowLeft' },
  { k: 'click', at: { t: 'body', page: 0 } }, { k: 'type', v: 'c' },
  { k: 'dblclick', at: { t: 'textbox', nth: 0 } }, { k: 'type', v: 'd' }, { k: 'press', v: 'Escape' }, { k: 'api', m: 'editingTextbox' },
];
/**
 * X3 (f7b__7__10, editor_switch.mjs parity step 6): the caret preset where the parity harness's `clickTarget` leaves it in
 * the TS run, a click at that point (it lands on a plain ellipse beside the text: both engines select the shape, no box is
 * entered), then a click inside the text-bearing ellipse (entered in both), typing, Esc.
 */
const X3 = [
  { k: 'api', m: 'setSelection', a: [{ para: 2, offset: 256 }] }, { k: 'click', at: { t: 'caret', para: 2, offset: 256 } },
  { k: 'api', m: 'editingTextbox' }, { k: 'click', at: { t: 'textbox', nth: 0 } }, { k: 'type', v: 'tb' }, { k: 'press', v: 'Escape' },
  { k: 'api', m: 'editingTextbox' },
];
/**
 * X4 (f7b__7__10): the border band of a text box (`onObjectBorder`, 5 px both sides of the outline): a click 3 px INSIDE
 * the left edge and 3 px OUTSIDE it (both engines select a shape, no box entered: frontier O's hit test first, then S's
 * `onBorder`), then a click in the box (entered in both).
 */
const X4 = [
  { k: 'click', at: { t: 'textbox', nth: 0, edge: -3 } }, { k: 'api', m: 'editingTextbox' },
  { k: 'click', at: { t: 'textbox', nth: 0, edge: 3 } }, { k: 'api', m: 'editingTextbox' },
  { k: 'click', at: { t: 'textbox', nth: 0 } }, { k: 'api', m: 'editingTextbox' },
];
// ── bar commands (`{ k: 'cmd', id, dlg }`: the chrome's registry, the dialogs filled and accepted as a user does) ──
/** T3: the table commands of the Insert tab, Table Layout and Table Design and their dialogs */
const T3 = [
  { k: 'type', v: 'Antes' }, { k: 'press', v: 'Enter' },
  // after the Insert Table / Split Cells dialogs (number fields) headless Chromium inserts no typed text in EITHER engine
  // (keydown and keypress reach the proxy, no beforeinput/input; the frozen TS's chrome, not investigated): `input` steps
  { k: 'cmd', id: 'table:insertDlg', dlg: { set: { cols: 3, rows: 3 } } }, { k: 'probe' },
  { k: 'input', v: 'c' }, { k: 'press', v: 'ArrowDown' }, { k: 'input', v: 'a' }, { k: 'press', v: 'ArrowDown' }, { k: 'input', v: 'b' },
  { k: 'cmd', id: 'sort', dlg: {} }, { k: 'click', at: { t: 'cell', r: 0, c: 0 } }, { k: 'input', v: 'd' },
  { k: 'cmd', id: 'table:select:row' }, { k: 'cmd', id: 'cells:merge' },
  { k: 'cmd', id: 'cells:split', dlg: { set: { cols: 2, rows: 1 } } },
  { k: 'cmd', id: 'row:above' }, { k: 'cmd', id: 'row:below' }, { k: 'cmd', id: 'col:left' }, { k: 'cmd', id: 'col:right' },
  { k: 'cmd', id: 'cells:insertDlg', dlg: { set: { shift: 'row' } } },
  { k: 'cmd', id: 'table:propertiesDlg', dlg: { set: { talign: 'center' } } },
  { k: 'cmd', id: 'cell:margins', dlg: { set: { left: '0.2', allow: true, spacing: '0.05' } } },
  { k: 'cmd', id: 'rows:distribute' }, { k: 'cmd', id: 'cols:distribute' },
  { k: 'cmd', id: 'table:autofit:content' }, { k: 'cmd', id: 'table:autofit:window' }, { k: 'cmd', id: 'table:autofit:fixed' },
  { k: 'cmd', id: 'table:headerRows' }, { k: 'cmd', id: 'cell:textdir' }, { k: 'cmd', id: 'cell:textdir' }, { k: 'cmd', id: 'cell:textdir' },
  { k: 'cmd', id: 'table:style:clear' }, { k: 'cmd', id: 'cell:align:cc' }, { k: 'cmd', id: 'table:look:bandedRows' },
  { k: 'cmd', id: 'table:border:outside' }, { k: 'cmd', id: 'table:gridlines' }, { k: 'screen' }, { k: 'cmd', id: 'table:gridlines' },
  { k: 'click', at: { t: 'cell', r: 3, c: 0 } }, { k: 'cmd', id: 'table:formula', dlg: { set: { formula: '=COUNT(ABOVE)', format: '0.00' } } }, { k: 'cmd', id: 'fields:update' },
  { k: 'cmd', id: 'table:select:cell' }, { k: 'cmd', id: 'table:select:column' }, { k: 'cmd', id: 'table:select:table' },
  { k: 'cmd', id: 'table:toText', dlg: { set: { sep: 'comma' } } }, { k: 'api', m: 'getSelection' },
  { k: 'cmd', id: 'table:fromText', dlg: {} }, { k: 'api', m: 'getTableState' },
  { k: 'click', at: { t: 'cell', r: 1, c: 0 } }, { k: 'cmd', id: 'cells:deleteDlg', dlg: { set: { shift: 'up' } } }, { k: 'cmd', id: 'table:split' },
  { k: 'click', at: { t: 'cell', r: 0, c: 0 } }, { k: 'cmd', id: 'table:delete' },
  { k: 'cmd', id: 'table', dlg: { action: 'cancel' } },
];
/** T4: the table mutators no command of T1/T3 reaches with these arguments (element API) */
const BS = (val, sz, color) => ({ val, sz, color, space: 0 });
const T4 = [
  { k: 'type', v: 'x' }, { k: 'press', v: 'Enter' }, { k: 'api', m: 'insertTable', a: [3, 3] },
  { k: 'type', v: '3' }, { k: 'press', v: 'Tab' }, { k: 'type', v: 'q' }, { k: 'press', v: 'Tab' }, { k: 'press', v: 'Tab' },
  { k: 'type', v: '1' }, { k: 'press', v: 'Tab' }, { k: 'type', v: 'w' }, { k: 'press', v: 'Tab' }, { k: 'press', v: 'Tab' }, { k: 'type', v: '2' },
  { k: 'api', m: 'setTableStyle', a: ['Grid Table 4 Accent 1'] }, { k: 'api', m: 'getTableState' },
  { k: 'api', m: 'setTableStyleOptions', a: [{ bandedRows: false, firstColumn: false, totalRow: true }] },
  { k: 'api', m: 'setTableBorderSides', a: [{ top: BS('single', 12, 'FF0000'), insideH: null }, 'table'] },
  { k: 'api', m: 'setTableBorderSides', a: [{ left: BS('dashed', 8, '00B050') }, 'cell'] },
  { k: 'api', m: 'setCellBorders', a: ['outside', BS('double', 6, '0000FF')] }, { k: 'api', m: 'setCellBorders', a: ['tl2br', BS('single', 4, '000000')] },
  { k: 'api', m: 'setCellBorders', a: ['none'] },
  { k: 'api', m: 'setTableProperties', a: [{ table: { align: 'center' }, row: { height: { twips: 600, rule: 'exact' }, header: true }, cell: { vAlign: 'center' } }] },
  { k: 'api', m: 'setTableProperties', a: [{ table: { width: { w: 2500, type: 'pct' } }, column: { width: { w: 1800, type: 'dxa' } } }] },
  { k: 'api', m: 'setTableCellMargins', a: [{ top: 60, left: 150 }, 40, false] },
  { k: 'api', m: 'setCellAlignment', a: ['right', 'bottom'] }, { k: 'api', m: 'setTextDirection', a: ['tbRl'] },
  { k: 'api', m: 'setRowHeight', a: [500, 'atLeast'] }, { k: 'api', m: 'setColumnWidth', a: [2000] },
  { k: 'api', m: 'distributeRows' }, { k: 'api', m: 'distributeColumns' },
  { k: 'api', m: 'insertCells', a: ['right'] }, { k: 'api', m: 'deleteCells', a: ['up'] },
  { k: 'api', m: 'sortInfo' }, { k: 'api', m: 'sort', a: [{ keys: [{ column: 0, type: 'number', desc: true }], header: false }] },
  { k: 'api', m: 'suggestFormula' }, { k: 'api', m: 'previewFormula', a: ['=SUM(ABOVE)', '0.0'] },
  { k: 'click', at: { t: 'cell', r: 1, c: 1 } }, { k: 'api', m: 'splitTable' }, { k: 'api', m: 'getTableState' },
  { k: 'api', m: 'selectTable' }, { k: 'api', m: 'convertTableToText', a: ['comma'] },
  { k: 'api', m: 'textToTableGuess', a: ['comma'] }, { k: 'api', m: 'convertTextToTable', a: [{ separator: 'comma', cols: 3, autofit: 'window' }] },
  { k: 'api', m: 'getTableState' }, { k: 'api', m: 'deleteColumn' }, { k: 'api', m: 'insertColumnLeft' }, { k: 'api', m: 'insertRowBelow' },
];
/**
 * H2: two sections (built in the TS run by `prep`, saved; both engines load the same bytes): Link to Previous off/on,
 * Different First Page on section 2, previous/next across the sections, the page number format dialog, remove page
 * numbers / header / footer in header mode, the options dialog, Show Document Text, the distance, Close.
 */
const H2_PREP = [
  { k: 'type', v: 'um' }, { k: 'api', m: 'insertSectionBreak', a: ['nextPage'] }, { k: 'type', v: 'dois' },
  { k: 'api', m: 'enterHeaderFooter', a: ['header', 0] }, { k: 'type', v: 'Cab1' },
  { k: 'api', m: 'enterHeaderFooter', a: ['footer', 0] }, { k: 'api', m: 'insertHeaderFooterTemplate', a: ['pageBottom', { t: 'template', gallery: 'pageBottom' }] },
  { k: 'press', v: 'Escape' },
];
const H2 = [
  { k: 'dblclick', at: { t: 'hf', kind: 'header', page: 1 } }, { k: 'screen' }, { k: 'api', m: 'getHeaderFooterOptions' },
  { k: 'cmd', id: 'hf:link' }, { k: 'type', v: 'S2' }, { k: 'cmd', id: 'docinfo:author' }, { k: 'cmd', id: 'dlg:dateTime', dlg: {} },
  { k: 'cmd', id: 'dlg:alignTab', dlg: {} }, { k: 'api', m: 'getHeaderFooterOptions' },
  { k: 'cmd', id: 'hf:prev' }, { k: 'type', v: 'x' }, { k: 'cmd', id: 'hf:next' },
  { k: 'cmd', id: 'hf:firstPage' }, { k: 'api', m: 'getHeaderFooterOptions' }, { k: 'type', v: 'F' },
  { k: 'cmd', id: 'hf:firstPage' }, { k: 'cmd', id: 'hf:link' }, { k: 'api', m: 'getHeaderFooterOptions' },
  { k: 'cmd', id: 'hf:gotoFooter' }, { k: 'cmd', id: 'dlg:pageNumberFormat', dlg: { set: { fmt: 'upperRoman' } } }, { k: 'api', m: 'getPageNumberFormat' },
  { k: 'cmd', id: 'hf:removePageNumbers' }, { k: 'api', m: 'setLinkToPrevious', a: [false, 'footer'] }, { k: 'type', v: 'rod2' },
  { k: 'api', m: 'removeHeaderFooter', a: ['footer'] }, { k: 'api', m: 'removePageNumbers' },
  { k: 'cmd', id: 'hf:gotoHeader' }, { k: 'cmd', id: 'hf:removeHeader' },
  { k: 'cmd', id: 'dlg:hfOptions', dlg: { set: { oddEven: true } } }, { k: 'api', m: 'getHeaderFooterOptions' },
  { k: 'cmd', id: 'hf:showText' }, { k: 'screen' }, { k: 'cmd', id: 'hf:showText' }, { k: 'api', m: 'previousHeaderFooter' }, { k: 'api', m: 'nextHeaderFooter' },
  { k: 'api', m: 'setHeaderFooterDistance', a: [{ header: 1000 }] }, { k: 'api', m: 'getHeaderFooterOptions' },
  { k: 'cmd', id: 'hf:close' }, { k: 'api', m: 'editingHeaderFooter' },
  { k: 'cmd', id: 'header' }, { k: 'press', v: 'Escape' },
];
/** N3: the note commands of the References tab and their dialogs */
const N3 = [
  { k: 'type', v: 'texto' }, { k: 'cmd', id: 'footnote' }, { k: 'type', v: 'n1' }, { k: 'cmd', id: 'note:goRef' },
  { k: 'type', v: ' b' }, { k: 'cmd', id: 'endnote' }, { k: 'type', v: 'e1' }, { k: 'press', v: 'Escape' },
  { k: 'press', v: 'Control+Home' }, { k: 'cmd', id: 'note:next' }, { k: 'cmd', id: 'note:prev' }, { k: 'cmd', id: 'note:nextEnd' },
  { k: 'cmd', id: 'note:prevEnd' }, { k: 'cmd', id: 'note:show' }, { k: 'press', v: 'Escape' },
  { k: 'cmd', id: 'dlg:notes', dlg: { set: { format: 'upperLetter', startAt: 3 }, action: 'apply' } }, { k: 'api', m: 'getNoteOptions', a: ['footnote'] },
  { k: 'cmd', id: 'note:convert', dlg: {} }, { k: 'api', m: 'noteCount', a: ['footnote'] }, { k: 'api', m: 'noteCount', a: ['endnote'] },
  { k: 'api', m: 'openNote', a: ['endnote', 1] }, { k: 'cmd', id: 'note:toFootnote' }, { k: 'api', m: 'noteCount', a: ['footnote'] },
  { k: 'api', m: 'goToNoteRef', a: ['footnote', 1] }, { k: 'api', m: 'showNotes', a: ['footnote'] }, { k: 'press', v: 'Escape' },
];
const SCENARIOS = [
  { name: 'T1', doc: null, steps: T1 },
  ...['f7b__7__1', 'f7b__7__4', 'f7b__7__5', 'f7b__7__6', 'f7b__7__7', 'f7b__7__9', 'f7d__7__8'].map((f) => ({ name: `T2 ${f}`, doc: f, steps: T2 })),
  { name: 'H1', doc: null, steps: H1 },
  { name: 'N1', doc: null, steps: N1 },
  { name: 'N2 f7c__7__13', doc: 'f7c__7__13', steps: N2 },
  // (f7d__7__1: the frozen TS throws typing in its linked box, a TS-side error: not a parity case; record S-G4)
  ...['f8__7__5', 'f8__7__3', 'f7d__7__0'].map((f) => ({ name: `X1 ${f}`, doc: f, steps: X1 })),
  { name: 'X2 f8__7__5', doc: 'f8__7__5', steps: X2 },
  { name: 'X3 f7b__7__10', doc: 'f7b__7__10', steps: X3 },
  { name: 'X4 f7b__7__10', doc: 'f7b__7__10', steps: X4 },
  { name: 'T3 bar', doc: null, steps: T3, bar: true },
  { name: 'T4', doc: null, steps: T4 },
  { name: 'H2 two sections', doc: null, prep: H2_PREP, steps: H2, bar: true },
  { name: 'N3 bar', doc: null, steps: N3, bar: true },
].filter((s) => !ONLY || ONLY.test(s.name));

/**
 * The declared differences of `stories` (scenario name → { from, facets, end, id, why }), as editor_switch.mjs's DECLARED:
 * `from` the first step that may differ, `facets` the step facets ('*' all), `end` the end facets ('*' all).
 */
const DECLARED = {
  // wave 7 (B): ED-106 closed — the TS frame and handles of a box (after Esc, in edit) are front O's paint, which this
  // harness's Rust screen left out (it kept S's hooks only); with O's hooks kept (tagged front 'O') X1 ×3, X3, X4 and X2's screen
  // are equal. What stays: ED-108, a TS defect (Word opens the Rust-saved package; record "Wave 7 — B")
  // (WordCraft tables fix round: ED-108 joined with the row keep of f8__7__5, below — one key per scenario)
  'X2 f8__7__5': { from: 0, facets: '*', end: '*', pagesAfterLoad: true, id: 'Word manda sobre o TS (WordCraft tables B5, row keep; ED-108)', why: 'Word keeps a row with the next by the first paragraph of its first cell only (15 pages, the TS 16); saving with the caret inside the text box, the TS writes a document.xml of 8,180 bytes against 82,578 (a TS defect of saving inside a box: most of the body is missing)' },
  // wave 7c (W): Word's layout rules over the frozen TS, each document attributed natively to ONE rule (uo-docx-layout
  // word_w7c.rs attribute_documents_to_the_word_rules) and measured in Word 16 (record uo-bindings.md "Wave 7c — W").
  // Item 3: the first box of a chain of linked boxes keeps the chain's first lines (Word; the TS widow rule empties it) —
  // in f7d__7__0 X1's click lands in that box's text (the TS: on the body behind the empty box), so every step differs
  'X1 f7d__7__0': { from: 0, facets: '*', end: '*', id: 'Word manda sobre o TS (item 3, linked boxes)', why: 'Word keeps the chain\'s first lines in the first box; the scripted click lands in its text, the TS on the body under the empty box' },
  'T2 f7d__7__8': { end: ['thumbnail pixels', 'screen pixels'], id: 'Word manda sobre o TS (item 3, linked boxes)', why: 'Word keeps the chain\'s first lines in the first box; the TS widow rule empties it' },
  // Item 4: REF/NOTEREF to the unterminated bm3 = Word's bold error text (the TS: the start paragraph's text)
  'N2 f7c__7__13': { end: ['thumbnail pixels', 'screen pixels'], id: 'Word manda sobre o TS (item 4, REF error)', why: 'REF/NOTEREF to the unterminated bm3: Word writes its bold error text, the TS the paragraph text' },
  // WordCraft study — tables (record uo-docx-layout.md "WordCraft study — tables", "Harness documents"; editor_switch.mjs's
  // entries of the same documents): Word's table rules move these documents' lines and pages (the table scripts' targets
  // are resolved in the TS run, so the steps may differ too); `pagesAfterLoad` as in editor_switch.mjs: the rule is Word's,
  // the page count changes (f7b__7__5 14, f8__7__5 15; the TS and Word 15/16)
  ...Object.fromEntries(['f7b__7__6', 'f7b__7__5'].map((f) => [`T2 ${f}`, { from: 0, facets: '*', end: '*', pagesAfterLoad: f === 'f7b__7__5', id: 'Word manda sobre o TS (WordCraft tables B5, row keep)', why: 'Word keeps a row with the next by the first paragraph of its first cell only' }])),
  ...Object.fromEntries(['f7b__7__4', 'f7b__7__7', 'f7b__7__9'].map((f) => [`T2 ${f}`, { from: 0, facets: '*', end: '*', id: 'Word manda sobre o TS (WordCraft tables B15 B, gap word)', why: 'mode 15: a word that does not fit the strip beside a float goes below it in a cell too (the TS splits it)' }])),
  'X1 f8__7__3': { from: 0, facets: '*', end: '*', id: 'Word manda sobre o TS (WordCraft tables B12, floating-table edges)', why: 'Word\'s wrap edges of a floating table: the right border and max(fromText, 10 tw)' },
  'X1 f8__7__5': { from: 0, facets: '*', end: '*', pagesAfterLoad: true, id: 'Word manda sobre o TS (WordCraft tables B5, row keep)', why: 'Word keeps a row with the next by the first paragraph of its first cell only: 15 pages (the TS 16)' },
};
// WordCraft study (record uo-docx-layout.md "WordCraft study — harness declarations"): Word's rules over the frozen TS,
// attributed by this harness's runs with ONE area at Word's rules (local wasm builds with the other areas' ts_* switches on;
// all four at the TS's: every scenario equal but the tables entries above, stale then) and, inside an area, by a build with
// ONE switch at Word's rule (tables) or by uo-docx-layout's native attribution of the document (fields). C1b: a
// multiple-spaced line over runs of several faces gets Word's text height (the lines move a few hundredths of a point);
// L8 (lines): an alignment tab (w:ptab) whose left/centre target is left of the pen ends the line (Word), the TS sets the
// text at the target; tables B6 (a conditional table-style part's borders by REGION: T4's Grid Table 4 Accent 1)
// moves T4's rows from setTableProperties on (and so the distributed heights/widths it saves). A key that already holds a declaration gets an array.
const WC_C1B_S = { id: 'Word manda sobre o TS (WordCraft fields C1b)', why: 'Word\'s text height of multiple-spaced lines over several faces moves the lines' };
const WC_S = {
  'N2 f7c__7__13': { from: 2, facets: ['caret rect'], ...WC_C1B_S, id: 'Word manda sobre o TS (WordCraft fields C1b and B22)' },
  'T2 f7b__7__1': { end: ['thumbnail pixels', 'screen pixels'], ...WC_C1B_S },
  'T2 f7d__7__8': { from: 0, facets: ['caret rect'], ...WC_C1B_S },
  'X3 f7b__7__10': { from: 0, facets: ['caret rect'], end: ['thumbnail pixels', 'screen pixels'], ...WC_C1B_S },
  'X4 f7b__7__10': { end: ['thumbnail pixels', 'screen pixels'], ...WC_C1B_S },
  'H2 two sections': { steps: [7, 8], facets: ['caret rect'], id: 'Word manda sobre o TS (WordCraft lines L8, ptab)', why: 'the alignment tab inserted in the header (dlg:alignTab): its target left of the pen ends the line in Word, the caret goes to the next line' },
  'T4': { from: 22, facets: ['caret rect', 'state table'], end: ['thumbnail pixels', 'screen pixels', 'saved .docx part bytes'], id: 'Word manda sobre o TS (WordCraft tables B6, conditional table-style borders)', why: 'Word gives a table-style part its borders by region (the TS: the four sides of a part to every cell of it), so the rows of the Grid Table 4 Accent 1 table move and distributeRows/Columns save other sizes' },
};
for (const [k, d] of Object.entries(WC_S)) DECLARED[k] = DECLARED[k] ? [DECLARED[k], d].flat() : d;
const STEP_FACETS = ['selection', 'caret rect', 'uw-* events', 'state run', 'state para', 'state table', 'contexts', 'dirty', 'api answer', 'screen pixels (step)'];
const END_FACETS = ['page count/sizes', 'thumbnail pixels', 'screen pixels', 'saved .docx part names', 'saved .docx part bytes'];
const declaredFor = (name, facet, step) => {
  // (WordCraft study: a key may hold several declarations, an array; the first that declares the facet answers)
  for (const d of [DECLARED[name] ?? []].flat()) {
    // (as editor_switch.mjs: 'pages after load' only for a Word-authority entry with `pagesAfterLoad`)
    if (facet === 'pages after load') { if (d.pagesAfterLoad && /^Word manda sobre o TS/.test(d.id)) return d; continue; }
    if (step === undefined) { if (END_FACETS.includes(facet) && (d.end === '*' || (d.end ?? []).includes(facet))) return d; continue; }
    // `steps`: exactly these steps; `from`: every step from there
    const at = d.steps ? d.steps.includes(step) : d.from !== undefined && step >= d.from;
    if (at && (d.facets === '*' || (d.facets ?? []).includes(facet))) return d;
  }
  return null;
};

/** a target resolved in the TS run (page twips + the scroll), or a literal */
async function resolve(t) {
  if (!t || typeof t !== 'object' || !t.t) return t;
  if (t.t === 'template') return ev((g) => window.__es.templateId(g), t.gallery);
  const p = await ev((x) => {
    const a = window.__es;
    const el = a.el;
    const pt = x.t === 'cell' ? a.cellPoint(x.r, x.c, x.nth ?? 0) : x.t === 'hf' ? a.hfPoint(x.kind, x.page ?? 0) : x.t === 'body' ? a.bodyPoint(x.page ?? 0)
      : x.t === 'noteLine' ? a.noteLinePoint() : x.t === 'noteRef' ? a.noteRefPoint() : x.t === 'textbox' ? a.textboxPoint(x.nth ?? 0, x.edge)
      : x.t === 'caret' ? a.caretPoint(x.para, x.offset) : null;
    if (!pt) return null;
    // the page in view (the same scroll on both runs)
    const c = el.canvases[pt.page];
    if (pt.scroll === undefined && c) {
      const box = el.getBoundingClientRect(), r = c.getBoundingClientRect();
      const y = r.top + pt.y * (96 / 1440) * el.zoom;
      if (y < box.top + 20 || y > box.bottom - 20) el.scrollTop += y - (box.top + box.height / 2);
      pt.scroll = el.scrollTop;
    }
    return pt;
  }, t);
  return p;
}

let cdp = null;
async function runStep(step) {
  const page = env.page;
  const pt = async (p) => (p ? ev((x) => window.__es.client(x), p) : null);
  if (step.k === 'press') { await page.keyboard.press(step.v); return null; }
  if (step.k === 'type') { await page.keyboard.type(step.v); return null; }
  if (step.k === 'api') return ev(([m, a]) => window.__es.call(m, a), [step.m, step.args ?? step.a ?? []]);
  if (step.k === 'input') { await ev((t) => window.__es.inputText(t), step.v); return null; }
  if (step.k === 'probe') return ev(() => window.__es.focusPath());
  if (step.k === 'screen') return ev(() => window.__es.screen());
  if (step.k === 'cmd') return ev(([id, d]) => window.__es.cmd(id, d), [step.id, step.dlg ?? null]);
  const a = await pt(step.p);
  if (!a) return 'no target';
  if (step.k === 'click') await page.mouse.click(a.x, a.y);
  else if (step.k === 'dblclick') await page.mouse.dblclick(a.x, a.y);
  else if (step.k === 'shiftclick') { await page.keyboard.down('Shift'); await page.mouse.click(a.x, a.y); await page.keyboard.up('Shift'); }
  else if (step.k === 'drag') {
    const b = await pt(step.p2);
    if (!b) return 'no target';
    await page.mouse.move(a.x, a.y); await page.mouse.down(); await page.mouse.move(b.x, b.y, { steps: 6 }); await page.mouse.up();
  }
  return null;
}

// ── stories ─────────────────────────────────────────────────────────────────────────────────────
if (MODE === 'stories' || MODE === 'all') {
  const tally = {};
  const used = new Set();
  const undeclared = [];
  let name = '';
  let own = {};
  const count = (facet, same, where, step) => {
    const t = (tally[facet] ??= { same: 0, diff: 0, declared: 0, first: [] });
    if (same) { t.same++; return; }
    t.diff++;
    own[facet] = (own[facet] ?? 0) + 1;
    const d = declaredFor(name, facet, step);
    if (d) { t.declared++; used.add(name); return; }
    if (t.first.length < 8) t.first.push(where);
    undeclared.push(`${facet}: ${where}`);
  };
  let k = 0;
  for (const sc of SCENARIOS) {
    if (k++ % 4 === 0) {
      await env.openPage(); await env.page.evaluate(helpers);
      cdp = await env.page.context().newCDPSession(env.page);
      if (process.env.UO_ES_DUMP) await ev(() => { window.__dump = true; });
    }
    name = sc.name;
    own = {};
    const errs0 = env.errors.length;
    const docName = sc.doc ? docOf(sc.doc) : null;
    const mk = { topbar: !!sc.bar };
    // `prep`: steps run on a TS element, saved; both runs then load the SAME bytes (a document built in-harness)
    let bytes = null;
    if (sc.prep) {
      await ev(() => window.__es.make('ts'));
      await ev((x) => window.__es.load(x), docName);
      await ev(() => window.__es.focus());
      for (const s of sc.prep) {
        const step = { ...s };
        if (s.at) step.p = await resolve(s.at);
        if (s.a) step.args = await Promise.all(s.a.map(resolve));
        await runStep(step);
      }
      bytes = (await ev(() => window.__es.end())).docx;
      await ev(() => window.__es.remove());
    }
    // TS: resolve the targets while running
    await ev((o) => window.__es.make('ts', o), mk);
    const pagesTs = await ev(([x, b]) => window.__es.load(x, b), [docName, bytes]);
    await ev(() => window.__es.focus());
    const script = [];
    const ts = [];
    for (const s of sc.steps) {
      const step = { ...s };
      if (s.at) step.p = await resolve(s.at);
      if (s.to) step.p2 = await resolve(s.to);
      if (s.a) step.args = await Promise.all(s.a.map(resolve));
      script.push(step);
      const answer = await runStep(step);
      ts.push({ ...(await ev(() => window.__es.facets())), answer });
    }
    const tsEnd = await ev(() => window.__es.end());
    await ev(() => window.__es.remove());
    // Rust: the same steps
    await ev((o) => window.__es.make('rust', o), mk);
    const pagesRs = await ev(([x, b]) => window.__es.load(x, b), [docName, bytes]);
    await ev(() => window.__es.focus());
    const rs = [];
    for (const step of script) {
      const answer = await runStep(step);
      rs.push({ ...(await ev(() => window.__es.facets())), answer });
    }
    const rsEnd = await ev(() => window.__es.end());
    await ev(() => window.__es.remove());
    if (process.env.UO_ES_TRACE) {
      for (let i = 0; i < script.length; i++) {
        const a = ts[i], z = rs[i];
        console.log(`    ${i} ${script[i].k} ${script[i].v ?? script[i].m ?? script[i].id ?? ''} | ts ${JSON.stringify(a.sel.focus)} ${a.ctx} ${a.events.join(';')} | rust ${JSON.stringify(z.sel.focus)} ${z.ctx} ${z.events.join(';')}${a.answer ? ` | answer ${String(a.answer).slice(0, 400)}` : ''}`);
        if (a.table !== z.table) console.log(`      table ts   ${a.table}
      table rust ${z.table}`);
      }
    }
    count('pages after load', pagesTs === pagesRs, `${name}: ${pagesTs} vs ${pagesRs}`);
    for (let i = 0; i < script.length; i++) {
      const a = ts[i], z = rs[i], at = `${name} step ${i} (${script[i].k} ${script[i].v ?? script[i].m ?? script[i].id ?? ''})`;
      count('selection', JSON.stringify(a.sel) === JSON.stringify(z.sel), `${at}: ${JSON.stringify(a.sel)} vs ${JSON.stringify(z.sel)}`, i);
      count('caret rect', JSON.stringify(a.caret) === JSON.stringify(z.caret), `${at}: ${JSON.stringify(a.caret)} vs ${JSON.stringify(z.caret)}`, i);
      count('uw-* events', JSON.stringify(a.events) === JSON.stringify(z.events), `${at}: ${JSON.stringify(a.events)} vs ${JSON.stringify(z.events)}`.slice(0, 700), i);
      count('state run', a.run === z.run, `${at}: ${a.run} vs ${z.run}`.slice(0, 500), i);
      count('state para', a.para === z.para, `${at}: ${a.para} vs ${z.para}`.slice(0, 500), i);
      count('state table', a.table === z.table, `${at}: ${a.table} vs ${z.table}`.slice(0, 900), i);
      count('contexts', a.ctx === z.ctx, `${at}: ${a.ctx} vs ${z.ctx}`, i);
      count('dirty', a.dirty === z.dirty, `${at}: ${a.dirty} vs ${z.dirty}`, i);
      if (script[i].k === 'screen') {
        // a mid-scenario screen: per page, where both engines allocated the canvas (as the end screens)
        const sa = a.answer.screen, sz = z.answer.screen;
        for (let p = 0; p < Math.max(sa.length, sz.length); p++) {
          if (sa[p] === '-' && sz[p] === '-') continue;
          if (sa[p] === '-' || sz[p] === '-' || sa[p] === undefined || sz[p] === undefined) { (tally['screen page allocated (informative)'] ??= { same: 0, diff: 0, declared: 0, first: [] }).diff++; continue; }
          count('screen pixels (step)', sa[p] === sz[p], `${at} page ${p}`, i);
          if (sa[p] !== sz[p] && process.env.UO_ES_DUMP) for (const [e, r] of [['ts', a.answer], ['rust', z.answer]]) if (r.shots[p]) writeFileSync(join(outDir, `screen-${name.replace(/[^A-Za-z0-9]+/g, '_')}-s${i}-p${p}-${e}.png`), Buffer.from(r.shots[p].split(',')[1], 'base64'));
        }
        a.answer = z.answer = null;
      } else count('api answer', a.answer === z.answer, `${at}: ${a.answer} vs ${z.answer}`.slice(0, 700), i);
    }
    count('page count/sizes', tsEnd.pages === rsEnd.pages && JSON.stringify(tsEnd.sizes) === JSON.stringify(rsEnd.sizes), `${name}: ${tsEnd.pages} vs ${rsEnd.pages}`);
    for (let i = 0; i < Math.max(tsEnd.thumbs.length, rsEnd.thumbs.length); i++) count('thumbnail pixels', tsEnd.thumbs[i] === rsEnd.thumbs[i], `${name} page ${i}`);
    for (let i = 0; i < Math.max(tsEnd.screen.length, rsEnd.screen.length); i++) {
      if (tsEnd.screen[i] === '-' && rsEnd.screen[i] === '-') continue;
      // a page one engine's scroll virtualization did not allocate (the TS keeps far buffers the shell's rebuildCanvases
      // released: wave 4 "Process metrics"): informative, the pixels are compared where both painted
      if (tsEnd.screen[i] === '-' || rsEnd.screen[i] === '-') { (tally['screen page allocated (informative)'] ??= { same: 0, diff: 0, declared: 0, first: [] }).diff++; continue; }
      count('screen pixels', tsEnd.screen[i] === rsEnd.screen[i], `${name} page ${i}`);
      if (tsEnd.screen[i] !== rsEnd.screen[i] && process.env.UO_ES_DUMP) for (const [e, r] of [['ts', tsEnd], ['rust', rsEnd]]) if (r.shots[i]) writeFileSync(join(outDir, `screen-${name.replace(/[^A-Za-z0-9]+/g, '_')}-p${i}-${e}.png`), Buffer.from(r.shots[i].split(',')[1], 'base64'));
    }
    // wave 7 (B): UO_ES_DUMP also writes both saved packages (Word open checks of the TS-defect declarations)
    if (process.env.UO_ES_DUMP) for (const [e, r] of [['ts', tsEnd], ['rust', rsEnd]]) writeFileSync(join(outDir, `saved-${name.replace(/[^A-Za-z0-9]+/g, '_')}-${e}.docx`), Buffer.from(r.docx, 'base64'));
    const za = unzipSync(Buffer.from(tsEnd.docx, 'base64')), zb = unzipSync(Buffer.from(rsEnd.docx, 'base64'));
    const partsA = Object.keys(za).sort(), partsB = Object.keys(zb).sort();
    count('saved .docx part names', JSON.stringify(partsA) === JSON.stringify(partsB), `${name}: ${partsA.filter((p) => !zb[p]).join(',')} | ${partsB.filter((p) => !za[p]).join(',')}`);
    for (const p of partsA) if (zb[p]) {
      const same = Buffer.compare(Buffer.from(za[p]), Buffer.from(zb[p])) === 0;
      count('saved .docx part bytes', same, `${name} ${p}`);
      if (!same && process.env.UO_ES_DUMP) { writeFileSync(join(outDir, `part-${name.replace(/[^A-Za-z0-9]+/g, '_')}-ts.xml`), Buffer.from(za[p])); writeFileSync(join(outDir, `part-${name.replace(/[^A-Za-z0-9]+/g, '_')}-rust.xml`), Buffer.from(zb[p])); }
    }
    if (env.errors.length !== errs0) console.log(`    stacks: ${JSON.stringify(await ev(() => (window.__stacks ?? []).splice(0)))}`);
    count('page errors', env.errors.length === errs0, `${name}: ${env.errors.slice(errs0).join(' | ')}`);
    let first = '';
    for (let i = 0; i < script.length && !first; i++) {
      for (const f of ['sel', 'caret', 'events', 'run', 'para', 'table', 'ctx', 'dirty', 'answer']) {
        if (JSON.stringify(ts[i][f]) !== JSON.stringify(rs[i][f])) { first = `step ${i} (${script[i].k} ${script[i].v ?? script[i].m ?? script[i].id ?? ''}) ${f}: ${JSON.stringify(ts[i][f])} vs ${JSON.stringify(rs[i][f])}`.slice(0, 500); break; }
      }
    }
    const diffs = Object.entries(own).map(([f, c]) => `${f} ${c}`).join(', ');
    console.log(`  ${name}: ${script.length} steps, pages ${tsEnd.pages}/${rsEnd.pages}${first ? `; first divergence ${first}` : ''}${diffs ? `; differs: ${diffs}` : ''}`);
  }
  console.log(`stories: ${SCENARIOS.length} scenario(s)`);
  for (const [f, t] of Object.entries(tally)) {
    const tag = t.declared ? ` (declared ${t.declared}, undeclared ${t.diff - t.declared})` : '';
    console.log(`  ${f.padEnd(26)} equal ${t.same}, different ${t.diff}${tag}${t.first.length ? `\n      e.g. ${t.first.join('\n      e.g. ')}` : ''}`);
  }
  console.log(`  declared differences used: ${[...used].map((f) => `${f} (${[DECLARED[f]].flat().map((d) => d.id).join(' + ')})`).join(', ') || 'none'}`);
  const stale = Object.keys(DECLARED).filter((f) => SCENARIOS.some((s) => s.name === f) && !used.has(f));
  if (stale.length) fail(`stories: declared but equal now (stale): ${stale.join(', ')}`);
  for (const u of undeclared) fail(`stories: undeclared difference: ${u}`);
}

// ── tbhit (UO_ES_DOC=<fixture>, UO_ES_POS=<para>:<offset>): the text box hit at the TS caret point of a body position ──
if (MODE === 'tbhit') {
  await env.openPage(); await env.page.evaluate(helpers);
  const n = docOf(process.env.UO_ES_DOC ?? 'f7b__7__10');
  const [pp, po] = (process.env.UO_ES_POS ?? '2:256').split(':').map(Number);
  await ev(() => window.__es.make('ts'));
  await ev((x) => window.__es.load(x), n);
  const pt = await ev(([p, o]) => {
    const el = window.__es.el;
    el.setSelection({ para: p, offset: o });
    const c = el.caretRectAt({ para: p, offset: o });
    const x = c.x + 0.5, y = c.y + c.h / 2;
    const e = new MouseEvent('mousedown', { clientX: x, clientY: y, button: 0 });
    const hit = el.hitTextbox(e);
    const page = window.__es.toPage(x, y);
    const objs = (el.eng.result.pages[page.page].objects ?? []).map((o) => ({
      x: o.x, y: o.y, w: o.w, h: o.h, tb: !!o.run?.textbox, shape: !!o.run?.shape, content: !!o.content, hf: o.hf, layer: o.layer,
      top: el.model.paragraphs.some((q) => q.runs.includes(o.run)),
      mine: el.contextTextboxRuns(el.model).includes(o.run),
    }));
    return { page, hit: hit && { rect: hit.rect, page: hit.page, border: el.onObjectBorder(e, { page: hit.page, rect: hit.rect, rot: hit.hostRun.textbox?.rot }) }, float: el.hitFloat(e)?.kind ?? null, objs };
  }, [pp, po]);
  console.log('ts', JSON.stringify(pt));
  await ev(() => window.__es.remove());
  for (const engine of ['ts', 'rust']) {
    await ev((e) => window.__es.make(e), engine);
    await ev((x) => window.__es.load(x), n);
    await ev(([p, o]) => window.__es.el.setSelection({ para: p, offset: o }), [pp, po]);
    await ev(() => window.__es.drain());
    const c = await ev((p) => window.__es.client(p), pt.page);
    await env.page.mouse.click(c.x, c.y);
    await ev(() => window.__es.settle());
    console.log(engine, await ev(() => { const el = window.__es.el; return JSON.stringify({ sel: el.getSelection(), tb: el.editingTextbox(), ev: window.__es.drain() }); }));
    await ev(() => window.__es.remove());
  }
}

// ── debug (UO_ES_DOC=<fixture>): the stories answers at the first text box of the TS layout ──
if (MODE === 'debug') {
  await env.openPage(); await env.page.evaluate(helpers);
  const n = docOf(process.env.UO_ES_DOC ?? 'f8__7__5');
  await ev(() => window.__es.make('ts'));
  await ev((x) => window.__es.load(x), n);
  const pt = await resolve({ t: 'textbox', nth: 0 });
  await ev(() => window.__es.remove());
  await ev(() => window.__es.make('rust'));
  await ev((x) => window.__es.load(x), n);
  console.log('point', JSON.stringify(pt));
  console.log(await ev((p) => {
    const el = window.__es.el;
    const ed = el.rust;
    const c = window.__es.client(p);
    const mod = ed.mod;
    const out = { client: c, hooks: Object.fromEntries(Object.entries(ed.hooks).map(([k, v]) => [k, Array.isArray(v) ? v.length : typeof v])) };
    const e = new MouseEvent('mousedown', { clientX: c.x, clientY: c.y, button: 0, detail: 1, bubbles: true });
    out.srcs = ed.hooks.pointer.map((f) => String(f).slice(0, 80));
    out.answers = ed.hooks.pointer.map((f) => { try { return f(e, { para: 0, offset: 0 }); } catch (x) { return String(x); } });
    out.ctx = el.editingTextbox();
    return JSON.stringify(out);
  }, pt));
}

// ── memory ──────────────────────────────────────────────────────────────────────────────────────
if (MODE === 'memory' || MODE === 'all') {
  await env.openPage();
  await env.page.evaluate(helpers);
  const lives = [], buffers = [];
  let bad = 0;
  for (let i = 0; i < SOAK; i++) {
    const r = await ev(() => window.__es.soakOnce());
    // wave 7c (S): the removed element keeps its document until it is collected (as the TS element its model): GC first
    if (r.after && !r.after.fake) r.after = await env.gc();
    if (!r.after || r.after.fake) { fail('memory: engineStats() answers nothing (count build?)'); break; }
    // a build without the counters answers live -1: a flat -1 would pass vacuously (UO_WASM_PATH=<the count build>)
    if (!(r.after.live >= 0)) { fail(`memory: no live byte counter (${r.after.live}): run with the count build`); break; }
    if (r.after.sessions !== 0 || r.after.shellEngines !== 0 || r.after.handles !== 0) { bad++; if (bad < 4) fail(`memory: cycle ${i}: after remove ${JSON.stringify(r.after)}`); }
    if (r.during.sessions !== 1) fail(`memory: cycle ${i}: ${r.during.sessions} sessions while open`);
    lives.push(r.after.live);
    buffers.push(r.after.buffer);
  }
  const warm = Math.min(5, lives.length - 1);
  const tail = new Set(lives.slice(warm)), bufTail = new Set(buffers.slice(warm));
  console.log(`memory: ${lives.length} cycles; live bytes after each remove from cycle ${warm}: ${[...tail].join(', ')}; memory.buffer: ${[...bufTail].join(', ')}`);
  if (tail.size !== 1) fail(`memory: live bytes not flat after warm-up (${[...tail].join(', ')})`);
  if (bufTail.size !== 1) fail(`memory: memory.buffer grows (${[...bufTail].join(', ')})`);
}

// ── chrome ──────────────────────────────────────────────────────────────────────────────────────
if (MODE === 'chrome' || MODE === 'all') {
  // the routed command ids of the fronts (the data-only route tables, imported with node's type stripping) and EVERY id
  // the chrome registers from chrome-table.ts and chrome-hf.ts (the batches `registerTableCommands` and
  // `registerHfCommands` add, caught by the harness's registry patch): a registered id the routes miss is compared too,
  // so a newly disabled one is a difference unless it is in the declared remainder
  const routes = async (n) => (await import(pathToFileURL(join(core, `../write/packages/editor/src/${n}.ts`)).href));
  const { ROUTES_STORIES } = await routes('engine-routes-stories');
  const { ROUTES_OBJECTS } = await routes('engine-routes-objects');
  const { ROUTES_REVIEW } = await routes('engine-routes-review');
  // wave 7 (B): the wave-7 fronts' tables too (A2 `completion`, A1 `format`)
  const { ROUTES_COMPLETION } = await routes('engine-routes-completion');
  const { ROUTES_FORMAT } = await routes('engine-routes-format');
  const routedAll = new Set([...ROUTES_STORIES.commands, ...ROUTES_OBJECTS.commands, ...ROUTES_REVIEW.commands,
    ...(ROUTES_COMPLETION.commands ?? []), ...(ROUTES_FORMAT.commands ?? [])]);
  /** the declared remainder of these two files (disabled under engine="rust"; record D7). Wave 7 (B): empty — every id of
   *  it is routed (A2: table:excel/painter/draw/eraser/style:new/style:modify, paste:cells:*, dlg:pageBorders; A1: dlg:borders) */
  const REMAINDER = new Set([]);
  await env.openPage();
  await env.page.evaluate(helpers);
  await ev(() => window.__es.make('ts', { topbar: true }));
  const registered = await ev(() => { const b = window.__adds; const pick = (id) => b.find((x) => x.includes(id)) ?? []; return [...pick('table:insertDlg'), ...pick('hf:editHeader')]; });
  await ev(() => window.__es.remove());
  if (registered.length < 80) fail(`chrome: only ${registered.length} ids registered by chrome-table.ts/chrome-hf.ts caught`);
  const ROUTED = [...new Set([...ROUTES_STORIES.commands, ...registered.filter((id) => routedAll.has(id))])];
  const OFF = registered.filter((id) => !routedAll.has(id));
  for (const id of OFF) if (!REMAINDER.has(id)) fail(`chrome: ${id} (chrome-table.ts/chrome-hf.ts) is neither routed nor in the declared remainder`);
  for (const id of REMAINDER) if (!OFF.includes(id)) fail(`chrome: ${id} of the declared remainder is routed or no longer registered (stale)`);
  const ALL = [...ROUTED, ...OFF];
  /** the state of every routed id (registry: supported/enabled/checked) and of the ribbon buttons, per context */
  const contexts = async (engine) => {
    await ev((e) => window.__es.make(e, { topbar: true }), engine);
    await ev(() => window.__es.load(null));
    await ev(() => window.__es.focus());
    await env.page.keyboard.type('x');
    const out = {};
    const snap = async (label) => {
      out[label] = {
        ids: await ev((ids) => Object.fromEntries(ids.map((id) => [id, `${+window.__es.reg.supported(id)}${+window.__es.reg.enabled(id)}${+window.__es.reg.checked(id)}`])), ALL),
        buttons: await ev((ids) => Object.fromEntries(ids.map((id) => [id, window.__es.buttonState(id)])), ['row:above', 'cells:merge', 'table:headerRows', 'table:deleteMenu', 'table:draw', 'table:eraser', 'hf:close', 'hf:link', 'hf:firstPage']),
        tabs: await ev(() => window.__es.tabs()),
      };
    };
    await snap('body');
    await ev(() => window.__es.call('insertTable', [2, 2]));
    out.inCell = await ev(() => window.__es.el.editingTableCell());
    await snap('cell');
    await ev(() => window.__es.call('selectRow'));
    await snap('row selected');
    await ev(() => window.__es.call('enterHeaderFooter', ['header', 0]));
    await snap('header');
    await ev(() => window.__es.call('exitHeaderFooter'));
    await ev(() => window.__es.call('insertFootnote'));
    await snap('footnote');
    await ev(() => window.__es.remove());
    return out;
  };
  const ts = await contexts('ts');
  const rs = await contexts('rust');
  if (!rs.inCell) fail('chrome: insertTable left the caret outside the table');
  let equal = 0, diff = 0, offOk = 0;
  for (const label of Object.keys(ts)) {
    if (label === 'inCell') continue;
    for (const id of ROUTED) {
      if (ts[label].ids[id] === rs[label].ids[id]) { equal++; continue; }
      diff++;
      fail(`chrome: ${label}: routed ${id} supported/enabled/checked ${ts[label].ids[id]} (ts) vs ${rs[label].ids[id]} (rust)`);
    }
    // the declared remainder: supported as the TS, never enabled under rust
    for (const id of OFF) {
      const [t, r] = [ts[label].ids[id], rs[label].ids[id]];
      if (t[0] !== r[0] || r[1] !== '0') { diff++; fail(`chrome: ${label}: remainder ${id} ${t} (ts) vs ${r} (rust): supported must match and enabled be 0`); } else offOk++;
    }
    for (const [id, st] of Object.entries(rs[label].buttons)) {
      if (!st) { fail(`chrome: ${label}: no ribbon button ${id}`); continue; }
      const t = ts[label].buttons[id];
      if (JSON.stringify(st) !== JSON.stringify(t)) fail(`chrome: ${label}: button ${id} ${JSON.stringify(t)} (ts) vs ${JSON.stringify(st)} (rust)`);
    }
    if (JSON.stringify(ts[label].tabs) !== JSON.stringify(rs[label].tabs)) fail(`chrome: ${label}: tabs ${ts[label].tabs.join('|')} (ts) vs ${rs[label].tabs.join('|')} (rust)`);
  }
  console.log(`chrome: ${registered.length} ids registered by chrome-table.ts/chrome-hf.ts (${OFF.length} declared remainder: ${OFF.join(' ')}); ${ROUTED.length} routed ids × ${Object.keys(ts).length - 1} contexts: equal ${equal}, different ${diff}; remainder disabled ${offOk}; cell tabs ${rs.cell.tabs.join(' | ')}; header tabs ${rs.header.tabs.join(' | ')}`);
}

const pe = env.errors.length;
await env.close();
if (pe) fail(`${pe} page error(s)`);
console.log(failures ? `editor_switch_stories: ${failures} failure(s)` : 'editor_switch_stories: ok');
process.exit(failures ? 1 : 0);
