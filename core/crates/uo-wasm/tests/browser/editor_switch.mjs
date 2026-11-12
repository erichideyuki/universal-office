// The TS/Rust switch of <universal-write> in a real Chromium (headless; record core/docs/modules/uo-bindings.md "Milestone 2:
// the editor switch"): `engine` absent (the TS engine) against `engine="rust"` (the TS shell over the wasm session).
//
//   UO_TS_ROOT=<frozen checkout: tools only> UO_WASM_PATH=<.wasm: the session build> [UO_CHROMIUM=<chrome-headless-shell.exe>]
//   [UO_ES_STRIDE=6] [UO_ES_STEPS=12] [UO_ES_SEED=1] [UO_ES_SOAK=100] [UO_ES_ONLY=<regex>]
//   node crates/uo-wasm/tests/browser/editor_switch.mjs --mode phase0|parity|memory|latency|keys|chrome|all
//
// - phase0: an editor without `engine` requests neither the .wasm nor the engine's chunk, and the entry's static closure holds no
//   code of core/crates/uo-wasm/js nor of either rust-engine.ts.
// - parity: per fixture, the TS element first, then the Rust one, both loaded with `loadDocx(bytes, { protected: false })` after
//   `__seed(1)`, in the same container; the TS run draws a script (seeded by the fixture's name) of REAL mouse and keyboard
//   events (clicks, Shift+clicks, drags, double and triple clicks on body positions; typing; IME composition through CDP;
//   Enter, Backspace, Delete, Ctrl+Backspace/Delete, Tab, Shift+Arrow, Up/Down, PageUp/PageDown, Home/End, Ctrl+Home/End,
//   Ctrl+Left/Right, Ctrl+Z/Y/B/E, Ctrl+A + typing; Ctrl+C/Ctrl+X, then Ctrl+V/Ctrl+Shift+V of a fixed plain text) and the Rust run replays
//   the same events. Facets after each step: selection, caret rectangle (relative to its page canvas), the uw-* event
//   sequence, the toolbar state (getSelectionState run/para), isDirty. At the end, after refresh(): page count and sizes, the
//   page thumbnails at two scales (pixels, tolerance 0), the screen canvases of the painted pages (caret hidden), the saved
//   .docx part by part (fflate), page errors. Every difference not covered by DECLARED (below) FAILS.
// - memory (count build): UO_ES_SOAK cycles of create (engine="rust") → load → type 20 characters → saveDocx → remove; sessions,
//   shell engines, handles 0 after each remove, one page environment, live bytes flat after warm-up, memory.buffer stops growing.
// - latency: the 50-page document of tools/bench/memory/gen_docs.mjs (seed 14, 310 paragraphs) per engine: 200 typed keys (the
//   synchronous `input` event: TS type → fast path → paint; Rust exec → relayout → paint), 20 Enter, 20 Backspace; median / p95,
//   the Rust breakdown and the incremental share. Recorded, not a gate.
// - keys: AltGr and Option characters (CDP) type the same in both engines; the browser's default of the keys the shell does not
//   run survives or not as in the TS (F5, F11, Esc… run; Shift+Tab, Alt+ArrowLeft, F9… are prevented).
// - chrome: compact and ribbon bars with engine="rust": routed buttons change the session state, a non-routed command is disabled;
//   with the focus in the document Ctrl+S reaches the bar's save, Ctrl+] grows the font, Ctrl+K (not routed) does nothing; the
//   Cut/Copy/Paste buttons are disabled; no page error.
import { existsSync, writeFileSync } from 'node:fs';
import { join } from 'node:path';
import { createRequire } from 'node:module';
import { setup, core, NOW } from './editor_switch_lib.mjs';

void NOW;
const root = process.env.UO_TS_ROOT;
if (!root) throw new Error('set UO_TS_ROOT to the frozen TS checkout (tools only: esbuild, playwright, fflate, the converter dist)');
const arg = (name, dflt) => { const i = process.argv.indexOf(name); return i >= 0 ? process.argv[i + 1] : dflt; };
const MODE = arg('--mode', 'all');
const STRIDE = Number(process.env.UO_ES_STRIDE ?? 6);
const STEPS = Number(process.env.UO_ES_STEPS ?? 12);
const SEED = Number(process.env.UO_ES_SEED ?? 1);
const SOAK = Number(process.env.UO_ES_SOAK ?? 100);
/** UO_ES_DUMP=1: the screen canvases that differ as PNG files in <target>/es-editor/ */
const DUMP = !!process.env.UO_ES_DUMP;
const target = process.env.CARGO_TARGET_DIR ?? join(core, 'target');
const wasmPath = process.env.UO_WASM_PATH ?? join(target, 'es/uo_wasm_session.wasm');
const outDir = join(target, 'es-editor');
const pages50 = join(target, 'es/pages50.docx');
const { unzipSync } = createRequire(join(root, 'write/package.json'))('fflate');

const env = await setup({
  root, wasmPath, outDir, stride: STRIDE, only: process.env.UO_ES_ONLY,
  extraDocs: existsSync(pages50) ? { 'pages50.docx': pages50 } : {},
});
let failures = 0;
const fail = (m) => { failures++; console.log(`FAIL ${m}`); };
const b = env.bundle;
console.log(`bundle: entry static closure ${b.staticOutputs} output(s) ${b.staticKB} KB; engine chunks ${b.engineChunks.length} (${b.engineKB} KB) + uo-write-editor.wasm ${b.wasmKB} KB`);

/** the page-side helpers (one function evaluated in each new page) */
const helpers = () => {
  const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
  const frame = () => new Promise((r) => requestAnimationFrame(() => r()));
  const fnv = (u8) => { let h = 0x811c9dc5; for (let i = 0; i < u8.length; i++) h = Math.imul(h ^ u8[i], 0x01000193); return (h >>> 0).toString(16).padStart(8, '0'); };
  const EVENTS = ['uw-change', 'uw-dirty-change', 'uw-input', 'uw-protected', 'uw-cellmode', 'uw-notemode', 'uw-hfmode', 'uw-tbmode',
    'uw-track-changes', 'uw-revisions-changed', 'uw-comments-changed', 'uw-bookmarks-change', 'uw-styles-change', 'uw-caption-settings',
    'uw-sort-error', 'uw-pasted', 'uw-custom-inline-inserted', 'uw-blocked', 'uw-font-embedding', 'uw-toc-update', 'uw-markup',
    'uw-paste-options', 'uw-before-input', 'uw-zoom'];
  const log = [];
  /** the TS screen overlays of paintEngine (field shading and comment highlight, marks, bookmarks, spelling, search hits,
   *  table selection and adorners, the TOC control, plugin inline renderers) */
  const OVERLAYS = ['paintEngineMarks', 'paintBookmarkMarks', 'paintEngineSpelling', 'paintSearchHits', 'paintTableAdorners', 'paintTocControl', 'paintMultiSelection', 'paintEngineInlines'];
  const fetchDoc = async (n) => new Uint8Array(await (await fetch(`/docs/${n}`)).arrayBuffer());
  const api = {
    el: null,
    async make(engine, o = {}) {
      const el = document.createElement('universal-write');
      if (engine === 'rust') el.setAttribute('engine', 'rust');
      el.setAttribute('fonts', '/fonts');
      if (o.topbar) el.setAttribute('topbar', '');
      if (o.toolbar) el.setAttribute('toolbar', o.toolbar);
      el.style.cssText = 'display:block;width:1000px;height:800px';
      const ready = new Promise((r) => el.addEventListener('uw-ready', r, { once: true }));
      document.body.appendChild(el);
      await ready;
      for (const t of EVENTS) el.addEventListener(t, (e) => { log.push(`${t} ${JSON.stringify(e.detail ?? null)}`); });
      api.el = el;
      return true;
    },
    async settle() {
      const el = api.el;
      if (el.rust) await el.rust.idle();
      else { await document.fonts.ready; await sleep(300); }
      await frame();
    },
    async load(n) {
      window.__seed(1);
      api.el.loadDocx(await fetchDoc(n), { protected: false });
      await api.settle();
      api.el.refresh();
      await api.settle();
      log.length = 0;
      return api.el.layoutPageCount();
    },
    remove() { api.el?.remove(); api.el = null; },
    drain() { const out = log.slice(); log.length = 0; return out; },
    /** the TS run: a click target on a body position drawn by `r1`, `r2` (the element's own paragraphs) */
    clickTarget(r1, r2) {
      const el = api.el;
      const ps = el.model.paragraphs;
      const keep = el.getSelection();
      // a position with a line on screen (hidden or empty-section paragraphs have none): up to 20 draws from (r1, r2)
      let a = r1, z = r2;
      for (let k = 0; k < 20; k++) {
        const p = Math.floor(a * ps.length);
        const len = ps[p].runs.reduce((n, r) => n + r.text.length, 0);
        const offset = Math.floor(z * (len + 1));
        el.setSelection({ para: p, offset });
        const c = el.caretRectAt({ para: p, offset });
        if (c) { api.drain(); return { x: c.x + 0.5, y: c.y + c.h / 2, scrollTop: el.scrollTop, para: p, offset }; }
        a = (a * 7.31 + 0.137) % 1; z = (z * 3.77 + 0.519) % 1;
      }
      // none: the selection as it was (the Rust run never saw these moves)
      el.setSelection(keep.anchor, keep.focus);
      api.drain();
      return null;
    },
    /**
     * The TS run: two positions of one body paragraph, both on screen at one scroll, WITHOUT moving the selection
     * (Shift+click keeps the anchor; a drag goes from the first to the second): `caretRectAt` answers any position.
     */
    targetPair(r1, r2, r3) {
      const el = api.el;
      const ps = el.model.paragraphs;
      const box = el.getBoundingClientRect();
      const inView = (c) => !!c && c.y >= box.top + 10 && c.y + c.h <= box.bottom - 10 && c.x >= box.left && c.x <= box.right - 20;
      let a = r1, z = r2, w = r3;
      for (let k = 0; k < 20; k++) {
        const p = Math.floor(a * ps.length);
        const len = ps[p].runs.reduce((n, r) => n + r.text.length, 0);
        const oa = Math.floor(z * (len + 1)), ob = Math.floor(w * (len + 1));
        let ca = el.caretRectAt({ para: p, offset: oa });
        if (ca && !inView(ca)) { el.scrollTop += ca.y - (box.top + box.height / 2); ca = el.caretRectAt({ para: p, offset: oa }); }
        const cb = el.caretRectAt({ para: p, offset: ob });
        if (inView(ca) && inView(cb)) {
          api.drain();
          return { scrollTop: el.scrollTop, x: ca.x + 0.5, y: ca.y + ca.h / 2, x2: cb.x + 0.5, y2: cb.y + cb.h / 2, para: p, offset: oa, offset2: ob };
        }
        a = (a * 7.31 + 0.137) % 1; z = (z * 3.77 + 0.519) % 1; w = (w * 5.13 + 0.271) % 1;
      }
      api.drain();
      return null;
    },
    scrollTo(top) { api.el.scrollTop = top; },
    /** the same clipboard at the start of both runs (the headless shell's clipboard lives across the page's runs) */
    /**
     * Between the copy (or cut) and the paste: reads the text/plain the copy left, then puts a fixed plain text on the
     * clipboard (no HTML): the TS keyboard paste then takes the plain-text path (its internal rich copy is valid only
     * while the system text is its own), the same text both engines paste through the keys.
     */
    async clipSwap() {
      await sleep(100);
      try { api.lastClip = await navigator.clipboard.readText(); } catch (e) { api.lastClip = `error ${e}`; }
      await navigator.clipboard.writeText('colado');
    },
    async clipReset() { try { await navigator.clipboard.writeText('ZZ'); return true; } catch (e) { return String(e); } },
    focus() { api.el.focusEditor(); },
    async facets(clip) {
      // the TS paste reads the clipboard asynchronously: let it land before the facets (both engines wait the same)
      if (clip) await sleep(300);
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
      // canonical JSON: keys sorted at every level, null/undefined dropped (the key order of a JS object is not a facet)
      const canon = (v) => (Array.isArray(v) ? v.map(canon) : v && typeof v === 'object'
        ? Object.fromEntries(Object.entries(v).filter(([, x]) => x !== undefined && x !== null).sort(([a], [b2]) => (a < b2 ? -1 : 1)).map(([k2, x]) => [k2, canon(x)]))
        : v);
      const clean = (o) => JSON.stringify(canon(o ?? {}));
      // after a clipboard step: the text/plain the copy or cut left (read by `clipSwap` before the paste)
      const clipText = clip ? api.lastClip : null;
      return { sel, caret, events: api.drain(), run: clean(st.run), para: clean(st.para), dirty: el.isDirty(), clip: clipText };
    },
    async end() {
      const el = api.el;
      el.refresh();
      await api.settle();
      const cs = [...el.shadowRoot.querySelectorAll('canvas.page')];
      const sizes = cs.map((cv) => `${cv.style.width}x${cv.style.height}`);
      const thumbs = [];
      for (let i = 0; i < cs.length; i++) {
        const w = parseFloat(cs[i].style.width);
        for (const k of [1, 2.5]) {
          const t = document.createElement('canvas');
          const ok = el.paintPageThumbnail(i, t, w * k);
          thumbs.push(ok ? fnv(new Uint8Array(t.getContext('2d').getImageData(0, 0, t.width, t.height).data.buffer)) : 'none');
          t.width = t.height = 0;
        }
      }
      // the screen with the caret hidden: the pages painted now (the same scroll on both)
      el.scrollTop = 0;
      // the scroll handler (virtualization: allocates the pages near the view, frees the far ones) before the capture
      await frame(); await frame();
      el.ensureVisiblePainted();
      // the blink timer flips the caret back: stopped for the capture
      window.clearInterval(el.caretTimer);
      el.caretVisible = false;
      el.paint();
      await frame();
      const grab = () => cs.map((cv) => (cv.width > 1 ? fnv(new Uint8Array(cv.getContext('2d').getImageData(0, 0, cv.width, cv.height).data.buffer)) : '-'));
      const withOverlays = grab();
      // the TS screen adornments the Rust shell does not paint in this wave (declared): off on the TS element, then again
      if (!el.rust) {
        for (const m of OVERLAYS) el[m] = () => {};
        el.paint();
        await frame();
      }
      // wave 5, frontier R: the shell paints the same overlays since wave 5 (routed under the TS names): off for the same capture
      if (el.rust) {
        for (const m of OVERLAYS) el[m] = () => {};
        el.paint();
        await frame();
      }
      const screen = grab();
      const shots = window.__dump ? cs.map((cv) => (cv.width > 1 ? cv.toDataURL('image/png') : null)) : [];
      const bytes = el.saveDocx();
      let bin = '';
      for (let i = 0; i < bytes.length; i += 0x8000) bin += String.fromCharCode(...bytes.subarray(i, i + 0x8000));
      return { pages: cs.length, sizes, thumbs, screen, withOverlays, shots, docx: btoa(bin) };
    },
    stats() { return window.__e.UniversalWriteElement.engineStats(); },
    // latency: the synchronous cost of one input event / one keydown on the hidden textarea
    async latency(n) {
      const el = api.el;
      el.setSelection({ para: 155, offset: 5 });
      el.focusEditor();
      const input = el.input;
      if (el.rust) el.rust.setMarks(true);
      const type = [], enter = [], back = [];
      for (let i = 0; i < n; i++) {
        input.value = 'a';
        const t0 = performance.now();
        input.dispatchEvent(new Event('input', { bubbles: true }));
        type.push(performance.now() - t0);
      }
      const typeMarks = el.rust ? el.rust.marks.slice() : [];
      for (const [list, key] of [[enter, 'Enter'], [back, 'Backspace']]) {
        for (let i = 0; i < 20; i++) {
          const t0 = performance.now();
          input.dispatchEvent(new KeyboardEvent('keydown', { key, code: key, bubbles: true, cancelable: true }));
          list.push(performance.now() - t0);
        }
      }
      return { type, enter, back, marks: typeMarks, pages: el.layoutPageCount() };
    },
    // memory: create → load → 20 typed characters → save → remove
    async soakOnce(n) {
      await api.make('rust');
      await api.load(n);
      const el = api.el;
      el.focusEditor();
      for (let i = 0; i < 20; i++) { el.input.value = 'x'; el.input.dispatchEvent(new Event('input', { bubbles: true })); }
      el.saveDocx();
      const during = api.stats();
      api.remove();
      await sleep(0);
      return { during, after: api.stats() };
    },
    // chrome: the bar buttons by id (data-a)
    button(id) {
      const find = (n) => { if (n.dataset?.a === id) return n; for (const c of n.children ?? []) { const f = find(c); if (f) return f; } if (n.shadowRoot) return find(n.shadowRoot); return null; };
      return find(api.el.shadowRoot);
    },
    clickButton(id) { const bt = api.button(id); if (!bt) return 'missing'; bt.dispatchEvent(new MouseEvent('mousedown', { bubbles: true, composed: true })); bt.click(); return 'clicked'; },
    buttonState(id) { const bt = api.button(id); return bt ? { disabled: !!(bt.disabled || bt.getAttribute?.('aria-disabled') === 'true' || bt.classList?.contains('disabled')) } : null; },
  };
  window.__es = api;
  return true;
};
const ev = (f, a) => env.page.evaluate(f, a);

// ── phase 0 ─────────────────────────────────────────────────────────────────────────────────────
if (MODE === 'phase0' || MODE === 'all') {
  const page = await env.openPage();
  await page.evaluate(helpers);
  const before = env.pageRequests.length;
  await ev(() => window.__es.make('ts'));
  await ev((n) => window.__es.load(n), [...env.docsByName.keys()][0]);
  const reqs = env.pageRequests.slice(before);
  const wasm = reqs.filter((u) => /\.wasm(\?|$)/.test(u));
  const engine = reqs.filter((u) => /rust-engine-/.test(u));
  console.log(`phase0: requests for .wasm ${wasm.length}, rust-engine chunk ${engine.length}; static closure leaks ${b.leaked.length}; engineStats() ${JSON.stringify(await ev(() => window.__es.stats()))}`);
  if (wasm.length || engine.length) fail(`phase0: an editor without engine requested ${[...wasm, ...engine].join(', ')}`);
  if (b.leaked.length) fail(`phase0: the entry's static closure holds the engine: ${b.leaked.join(', ')}`);
  await ev(() => window.__es.remove());
}

// ── parity ─────────────────────────────────────────────────────────────────────────────────────
function rng(seed) { let s = seed >>> 0 || 1; return () => { s ^= s << 13; s >>>= 0; s ^= s >>> 17; s ^= s << 5; s >>>= 0; return s / 4294967296; }; }
/** the seed of a fixture's script: from its name, so a fixture gets the same script at any stride or UO_ES_ONLY */
const nameSeed = (label) => { let h = 0x811c9dc5; for (let i = 0; i < label.length; i++) h = Math.imul(h ^ label.charCodeAt(i), 0x01000193); return h >>> 0; };
const WORDS = ['Olá', 'mundo', 'teste', 'abc', 'ação', 'x'];
const KEYS = ['Enter', 'Backspace', 'Delete', 'Shift+ArrowRight', 'Home', 'End', 'Control+ArrowLeft', 'Control+ArrowRight', 'Control+z', 'Control+y', 'Control+b', 'Control+e',
  // the routed paths of the record's table: line moves through the layout, page moves, by-word deletion (ED-4), Tab, the
  // story ends, extending by line
  'ArrowUp', 'ArrowDown', 'Shift+ArrowDown', 'PageUp', 'PageDown', 'Control+Backspace', 'Control+Delete', 'Tab', 'Control+Home', 'Control+End'];
/** the composed IME text (CDP `Input.imeSetComposition` updates, then `Input.insertText` commits: compositionstart/update/end) */
const IME = ['çã', 'ñ', 'é'];

let cdp = null;
async function runStep(step) {
  const page = env.page;
  const at = async (s) => ev((t) => window.__es.scrollTo(t), s.scrollTop);
  if (step.k === 'click') { await at(step); await page.mouse.click(step.x, step.y); }
  else if (step.k === 'shiftclick') { await at(step); await page.keyboard.down('Shift'); await page.mouse.click(step.x, step.y); await page.keyboard.up('Shift'); }
  else if (step.k === 'drag') { await at(step); await page.mouse.move(step.x, step.y); await page.mouse.down(); await page.mouse.move(step.x2, step.y2, { steps: 4 }); await page.mouse.up(); }
  else if (step.k === 'dblclick') { await at(step); await page.mouse.dblclick(step.x, step.y); }
  else if (step.k === 'triple') { await at(step); await page.mouse.click(step.x, step.y, { clickCount: 3 }); }
  else if (step.k === 'type') await page.keyboard.type(step.v);
  else if (step.k === 'press') await page.keyboard.press(step.v);
  else if (step.k === 'all') { await page.keyboard.press('Control+a'); await page.keyboard.type(step.v); }
  else if (step.k === 'ime') {
    for (let i = 1; i <= step.v.length; i++) await cdp.send('Input.imeSetComposition', { text: step.v.slice(0, i), selectionStart: i, selectionEnd: i });
    await cdp.send('Input.insertText', { text: step.v });
  } else if (step.k === 'clip') {
    // select a word to the left, copy (or cut), read the copied text, put the plain text "colado" on the clipboard
    // (clipSwap), paste (or paste as plain text) in place: over the copied selection, or where the cut text was (no caret
    // move in between: End would mix in the End-on-a-wrapped-line difference, gate item 2); the headless shell's
    // clipboard is in memory
    await page.keyboard.press('Control+Shift+ArrowLeft');
    await page.keyboard.press(/^cut/.test(step.v) ? 'Control+x' : 'Control+c');
    await ev(() => window.__es.clipSwap());
    await page.keyboard.press(/plain$/.test(step.v) ? 'Control+Shift+v' : 'Control+v');
  }
}

/**
 * The declared differences (seed 1, 12 steps, the scripts of `nameSeed`): a difference of the parity mode passes only
 * when one of these covers it, everything else FAILS. Keyed by the fixture's file name:
 * - `from`: the first differing step; `facets`: the step facets it may differ in from there on ('*' = all: the two
 *   engines edit different positions from that step on, so every later step and the end may differ);
 * - `end`: the end facets that may differ ('*' = all of page count/sizes, thumbnails, screen, saved parts);
 * - `id`: the declaration of the record (core/docs/modules/uo-bindings.md "Milestone 2: the editor switch").
 * 'pages after load' and page errors are never declarable; 'screen pixels (TS overlays on)'
 * is informative (ED-8).
 */
const DECLARED = {
  // ED-16 (a clipboard step, since fix round 2 only the copied text/plain: the TS joins paragraphs with CRLF, the shell
  // with LF; the keyboard paste goes through the session's pasteData and is equal)
  'f2-3-1': { end: ['thumbnail pixels', 'screen pixels (TS overlays off)'], id: 'gate-10', why: 'the same saved bytes, other pages (the automatic hyphen of gate item 10)' },
  'f2-3-2': { from: 0, facets: ['selection', 'caret rect', 'uw-* events'], end: ['thumbnail pixels', 'screen pixels (TS overlays off)'], id: 'gate-1', why: 'a click at the TS caret x of offset 131 hits offset 128 (x 366.3 vs 367.1 px): the caret/hit x mapping' },
  'f2-3-3': { from: 4, facets: ['selection', 'caret rect', 'uw-* events', 'state run', 'state para'], end: ['thumbnail pixels', 'screen pixels (TS overlays off)'], id: 'gate-1', why: 'a click at the TS caret x of paragraph 16 offset 162 hits offset 159: the caret/hit x mapping' },
  'f7a__7__1': { from: 5, facets: ['selection', 'caret rect', 'state run'], end: ['thumbnail pixels', 'screen pixels (TS overlays off)'], id: 'gate-1', why: 'PageUp: the 20 line moves over mixed-size lines land on another line (goal x = the caret x)' },
  'f7a__7__13': { from: 0, facets: ['selection', 'caret rect', 'uw-* events', 'state run', 'clipboard text'], end: ['thumbnail pixels', 'screen pixels (TS overlays off)', 'saved .docx part bytes'], id: 'gate-1', why: 'a click on a mixed-size line hits another offset' },
  'f7a__7__14': { from: 3, facets: ['clipboard text'], id: 'ED-16', why: 'clip copy over a paragraph mark: the TS text/plain has CRLF, the shell LF' },
  'f7a__7__2': { from: 0, facets: ['selection', 'caret rect', 'uw-* events', 'state run', 'state para', 'clipboard text'], end: ['page count/sizes', 'thumbnail pixels', 'screen pixels (TS overlays off)', 'saved .docx part bytes'], id: 'gate-1', why: 'a click on a mixed-size line hits another offset' },
  'f7a__7__3': { from: 4, facets: ['selection', 'caret rect'], end: ['thumbnail pixels', 'screen pixels (TS overlays off)'], id: 'gate-1', why: 'ArrowDown from a mixed-size line: offset 84 vs 82' },
  'f7a__7__4': { from: 0, facets: ['selection', 'caret rect', 'uw-* events', 'state run', 'state para', 'clipboard text'], end: ['thumbnail pixels', 'screen pixels (TS overlays off)', 'saved .docx part bytes'], id: 'gate-1', why: 'a click on a mixed-size line hits another offset' },
  // front O (wave 5): floats are selectable now; this click lands on a shape that carries text (an ellipse with a txbx
  // story). Fix round 1 (UO_ES_TRACE=1): the TS moves the caret to para 2 offset 256 (one uw-change), the shell selects the
  // shape (ED-202b's uw-change, the caret stays at para 5 offset 100); the later steps act at the other caret. Facets
  // narrowed to the ones that differ; the cause of the TS's caret is not isolated (front O gate list)
  'f7b__7__10': { from: 6, facets: ['selection', 'caret rect', 'state run', 'state para', 'uw-* events', 'clipboard text'], end: ['thumbnail pixels', 'screen pixels (TS overlays off)', 'saved .docx part bytes'], id: 'ED-216', why: 'a click on a text-bearing shape: the TS places the caret in the text (para 2 offset 256), the shell selects the shape; later steps follow' },
  'f7c__7__1': { end: ['thumbnail pixels'], id: 'gate-10', why: 'the same saved bytes, other pages (gate item 10)' },
  'f7c__7__5': { from: 6, facets: ['clipboard text'], end: ['thumbnail pixels'], id: 'ED-16; gate-10', why: 'clip cutplain: the TS text/plain has CRLF; end: the same saved bytes, other pages (gate item 10 family, not confirmed per page)' },
  'f7d__7__10': { from: 3, facets: ['selection', 'caret rect', 'state run', 'state para', 'uw-* events'], end: ['thumbnail pixels', 'screen pixels (TS overlays off)', 'saved .docx part bytes'], id: 'ED-216; wave-6', why: 'wave 5 (orchestrator, stride 1): a click on a text box or text-bearing shape: one engine enters the box, the other selects the object or keeps the caret (the TS hitTextbox rule vs the S/O hit order; S ED-102, O ED-216); fix in wave 6, then drop this entry' },
  'f7d__7__11': { from: 7, facets: ['selection', 'caret rect', 'state run', 'state para', 'uw-* events'], end: ['thumbnail pixels', 'screen pixels (TS overlays off)', 'saved .docx part bytes'], id: 'ED-216; wave-6', why: 'wave 5 (orchestrator, stride 1): a click on a text box or text-bearing shape: one engine enters the box, the other selects the object or keeps the caret (the TS hitTextbox rule vs the S/O hit order; S ED-102, O ED-216); fix in wave 6, then drop this entry' },
  'f7d__7__6': { from: 3, facets: ['selection', 'caret rect', 'state run', 'state para', 'uw-* events'], end: ['thumbnail pixels', 'screen pixels (TS overlays off)', 'saved .docx part bytes'], id: 'ED-216; wave-6', why: 'wave 5 (orchestrator, stride 1): a click on a text box or text-bearing shape: one engine enters the box, the other selects the object or keeps the caret (the TS hitTextbox rule vs the S/O hit order; S ED-102, O ED-216); fix in wave 6, then drop this entry' },
  'f7d__7__7': { from: 0, facets: ['selection', 'caret rect', 'state run', 'state para', 'uw-* events', 'clipboard text'], end: ['thumbnail pixels', 'screen pixels (TS overlays off)', 'saved .docx part bytes'], id: 'ED-216; wave-6', why: 'wave 5 (orchestrator, stride 1): a click on a text box or text-bearing shape: one engine enters the box, the other selects the object or keeps the caret (the TS hitTextbox rule vs the S/O hit order; S ED-102, O ED-216); fix in wave 6, then drop this entry' },
  'f7d__7__9': { end: ['screen pixels (TS overlays off)'], id: 'BUG-1', why: 'the final backward selection (focus before anchor) is not painted: uo_session_selection_rects does not normalize the range' },
  // frontier S (wave 5): the ED-7 entries of tables, notes and text boxes (f7b__7__1/4/5/6/7/9, f7c__7__13, f7d__7__8) are
  // equal since the stories routing (removed); f8__7__5 step 11 enters the text box in both engines now, the end differs:
  'f8__7__5': { from: 3, facets: ['clipboard text'], end: ['screen pixels (TS overlays off)', 'saved .docx part bytes'], id: 'ED-16; ED-106; ED-108', why: 'step 3: clip copyplain over a paragraph mark (CRLF text/plain); end: the TS paints the handles of the edited text box (frontier O) and saves a truncated document.xml with the caret inside the box (a TS defect, ED-108)' },
};
const STEP_FACETS = ['selection', 'caret rect', 'uw-* events', 'state run', 'state para', 'dirty', 'clipboard text'];
const END_FACETS = ['page count/sizes', 'thumbnail pixels', 'screen pixels (TS overlays off)', 'saved .docx part names', 'saved .docx part bytes'];
const INFORMATIVE = new Set(['screen pixels (TS overlays on)']);
const declaredFor = (file, facet, step) => {
  if (SEED !== 1 || STEPS !== 12) return null;
  const d = DECLARED[file];
  if (!d) return null;
  if (step === undefined) return END_FACETS.includes(facet) && (d.end === '*' || (d.end ?? []).includes(facet)) ? d : null;
  return d.from !== undefined && step >= d.from && (d.facets === '*' || (d.facets ?? []).includes(facet)) ? d : null;
};

if (MODE === 'parity' || MODE === 'all') {
  const rows = [];
  const tally = {};
  let own = {};
  let file = '';
  const used = new Set();
  const undeclared = [];
  /** one facet comparison; `step` undefined = an end facet */
  const count = (facet, same, where, step) => {
    const t = (tally[facet] ??= { same: 0, diff: 0, declared: 0, first: [] });
    if (same) { t.same++; return; }
    t.diff++;
    own[facet] = (own[facet] ?? 0) + 1;
    if (INFORMATIVE.has(facet)) return;
    const d = declaredFor(file, facet, step);
    if (d) { t.declared++; used.add(file); return; }
    if (t.first.length < 6) t.first.push(where);
    undeclared.push(`${facet}: ${where}`);
  };
  cdp = null;
  const names = [...env.docsByName.keys()].filter((n) => n !== 'pages50.docx');
  let k = 0;
  for (const n of names) {
    if (k++ % 6 === 0) {
      await env.openPage(); await env.page.evaluate(helpers);
      cdp = await env.page.context().newCDPSession(env.page);
      await env.page.context().grantPermissions(['clipboard-read', 'clipboard-write']);
      if (DUMP) await ev(() => { window.__dump = true; });
    }
    const label = env.label(n);
    file = label.replace(/^.*\//, '').replace(/\.docx$/, '');
    const errs0 = env.errors.length;
    own = {};
    const draw = rng(SEED * 7919 + nameSeed(file));
    // TS: build the concrete script while running it
    await ev(() => window.__es.make('ts'));
    const pagesTs = await ev((x) => window.__es.load(x), n);
    await ev(() => window.__es.focus());
    const clip0 = await ev(() => window.__es.clipReset());
    if (clip0 !== true) fail(`parity: the clipboard reset failed (${clip0})`);
    const script = [];
    const tsFacets = [];
    for (let i = 0; i < STEPS; i++) {
      const r = draw();
      let step;
      if (i === 0 || r < 0.2) {
        const t = await ev(([a, c]) => window.__es.clickTarget(a, c), [draw(), draw()]);
        step = t ? { k: 'click', ...t } : { k: 'press', v: 'End' };
      } else if (r < 0.3) {
        const kind = ['shiftclick', 'drag', 'dblclick', 'triple'][Math.floor(draw() * 4)];
        const t = await ev(([a, c, d]) => window.__es.targetPair(a, c, d), [draw(), draw(), draw()]);
        step = t ? { k: kind, ...t } : { k: 'press', v: 'Home' };
      } else if (r < 0.48) step = { k: 'type', v: WORDS[Math.floor(draw() * WORDS.length)] + (draw() < 0.5 ? ' ' : '') };
      else if (r < 0.53) step = { k: 'ime', v: IME[Math.floor(draw() * IME.length)] };
      else if (r < 0.58) step = { k: 'clip', v: ['copy', 'cut', 'copyplain', 'cutplain'][Math.floor(draw() * 4)] };
      else if (r < 0.97 || i < STEPS - 1) step = { k: 'press', v: KEYS[Math.floor(draw() * KEYS.length)] };
      else step = { k: 'all', v: 'Fim' };
      script.push(step);
      await runStep(step);
      tsFacets.push(await ev((c) => window.__es.facets(c), step.k === 'clip'));
    }
    const tsEnd = await ev(() => window.__es.end());
    await ev(() => window.__es.remove());
    // Rust: the same events
    await ev(() => window.__es.make('rust'));
    const pagesRs = await ev((x) => window.__es.load(x), n);
    await ev(() => window.__es.focus());
    await ev(() => window.__es.clipReset());
    const rsFacets = [];
    for (const step of script) { await runStep(step); rsFacets.push(await ev((c) => window.__es.facets(c), step.k === 'clip')); }
    const rsEnd = await ev(() => window.__es.end());
    await ev(() => window.__es.remove());
    // UO_ES_TRACE=1: every step of both runs (selection, clipboard text, events)
    if (process.env.UO_ES_TRACE) {
      for (let i = 0; i < script.length; i++) {
        const a = tsFacets[i], z = rsFacets[i];
        console.log(`    ${i} ${script[i].k} ${script[i].v ?? ''} | ts ${JSON.stringify(a.sel.focus)} ${JSON.stringify(a.clip)} ${a.events.length} ev ${a.events.map((x) => x.split(' ')[0]).join(',')} | rust ${JSON.stringify(z.sel.focus)} ${JSON.stringify(z.clip)} ${z.events.length} ev ${z.events.map((x) => x.split(' ')[0]).join(',')}`);
      }
    }
    // compare
    count('pages after load', pagesTs === pagesRs, `${label}: ${pagesTs} vs ${pagesRs}`);
    for (let i = 0; i < script.length; i++) {
      const a = tsFacets[i], z = rsFacets[i], at = `${label} step ${i} (${script[i].k} ${script[i].v ?? ''})`;
      count('selection', JSON.stringify(a.sel) === JSON.stringify(z.sel), `${at}: ${JSON.stringify(a.sel)} vs ${JSON.stringify(z.sel)}`, i);
      count('caret rect', JSON.stringify(a.caret) === JSON.stringify(z.caret), `${at}: ${JSON.stringify(a.caret)} vs ${JSON.stringify(z.caret)}`, i);
      count('uw-* events', JSON.stringify(a.events) === JSON.stringify(z.events), `${at}: ${JSON.stringify(a.events)} vs ${JSON.stringify(z.events)}`.slice(0, 600), i);
      count('state run', a.run === z.run, `${at}: ${a.run} vs ${z.run}`, i);
      count('state para', a.para === z.para, `${at}: ${a.para} vs ${z.para}`.slice(0, 600), i);
      count('dirty', a.dirty === z.dirty, `${at}: ${a.dirty} vs ${z.dirty}`, i);
      count('clipboard text', a.clip === z.clip, `${at}: ${JSON.stringify(a.clip)} vs ${JSON.stringify(z.clip)}`, i);
    }
    count('page count/sizes', tsEnd.pages === rsEnd.pages && JSON.stringify(tsEnd.sizes) === JSON.stringify(rsEnd.sizes), `${label}: ${tsEnd.pages} vs ${rsEnd.pages}`);
    for (let i = 0; i < Math.max(tsEnd.thumbs.length, rsEnd.thumbs.length); i++) count('thumbnail pixels', tsEnd.thumbs[i] === rsEnd.thumbs[i], `${label} page ${i >> 1} x${i & 1 ? '2.5' : '1'}`);
    for (let i = 0; i < Math.max(tsEnd.screen.length, rsEnd.screen.length); i++) {
      if (tsEnd.screen[i] === '-' && rsEnd.screen[i] === '-') continue;
      const same = tsEnd.screen[i] === rsEnd.screen[i];
      count('screen pixels (TS overlays off)', same, `${label} page ${i}`);
      count('screen pixels (TS overlays on)', tsEnd.withOverlays[i] === rsEnd.withOverlays[i], `${label} page ${i}`);
      if (!same && DUMP) for (const [e, r] of [['ts', tsEnd], ['rust', rsEnd]]) if (r.shots[i]) writeFileSync(join(outDir, `screen-${n}-p${i}-${e}.png`), Buffer.from(r.shots[i].split(',')[1], 'base64'));
    }
    const za = unzipSync(Buffer.from(tsEnd.docx, 'base64')), zb = unzipSync(Buffer.from(rsEnd.docx, 'base64'));
    const partsA = Object.keys(za).sort(), partsB = Object.keys(zb).sort();
    count('saved .docx part names', JSON.stringify(partsA) === JSON.stringify(partsB), `${label}: ${partsA.filter((p) => !zb[p]).join(',')} | ${partsB.filter((p) => !za[p]).join(',')}`);
    for (const p of partsA) if (zb[p]) count('saved .docx part bytes', Buffer.compare(Buffer.from(za[p]), Buffer.from(zb[p])) === 0, `${label} ${p}`);
    count('page errors', env.errors.length === errs0, `${label}: ${env.errors.slice(errs0).join(' | ')}`);
    rows.push(label);
    // the first step where a facet differs (the later ones usually follow from it)
    let first = '';
    for (let i = 0; i < script.length && !first; i++) {
      const a = tsFacets[i], z = rsFacets[i];
      for (const f of ['sel', 'caret', 'events', 'run', 'para', 'dirty', 'clip']) {
        if (JSON.stringify(a[f]) !== JSON.stringify(z[f])) { first = `step ${i} (${script[i].k} ${script[i].v ?? ''}) ${f}: ${JSON.stringify(a[f])} vs ${JSON.stringify(z[f])}`.slice(0, 400); break; }
      }
    }
    const diffs = Object.entries(own).filter(([f]) => f !== 'screen pixels (TS overlays on)').map(([f, c]) => `${f} ${c}`).join(', ');
    console.log(`  ${label}: ${script.length} steps, pages ${tsEnd.pages}/${rsEnd.pages}${first ? `; first divergence ${first}` : ''}${diffs ? `; differs: ${diffs}` : ''}`);
  }
  const other = SEED !== 1 || STEPS !== 12 ? ' (the declared table is for seed 1 and 12 steps: here every difference fails)' : '';
  console.log(`parity: ${rows.length} fixture(s), ${STEPS} steps each (seed ${SEED})${other}`);
  for (const [f, t] of Object.entries(tally)) {
    const tag = INFORMATIVE.has(f) ? ' (informative, ED-8)' : t.declared ? ` (declared ${t.declared}, undeclared ${t.diff - t.declared})` : '';
    console.log(`  ${f.padEnd(32)} equal ${t.same}, different ${t.diff}${tag}${t.first.length ? `\n      e.g. ${t.first.join('\n      e.g. ')}` : ''}`);
  }
  console.log(`  declared differences used: ${[...used].map((f) => `${f} (${DECLARED[f].id})`).join(', ') || 'none'}`);
  const ran = new Set(names.map((x) => env.label(x).replace(/^.*\//, '').replace(/\.docx$/, '')));
  const stale = Object.keys(DECLARED).filter((f) => ran.has(f) && !used.has(f));
  if (stale.length && !other) console.log(`  declared but equal now (remove from DECLARED): ${stale.join(', ')}`);
  for (const u of undeclared) fail(`parity: undeclared difference: ${u}`);
}

// ── memory ─────────────────────────────────────────────────────────────────────────────────────
if (MODE === 'memory' || MODE === 'all') {
  await env.openPage();
  await env.page.evaluate(helpers);
  const cycle = [...env.docsByName.keys()].filter((n) => n !== 'pages50.docx').slice(0, 5);
  const series = [];
  const t0 = Date.now();
  for (let i = 0; i < SOAK; i++) {
    const r = await ev((n) => window.__es.soakOnce(n), cycle[i % cycle.length]);
    series.push({ k: i, ...r.after, duringLayouts: r.during?.handles, duringEditors: r.during?.editors });
    if (i % 20 === 0 || i === SOAK - 1) console.log(`  #${String(i).padStart(3)} live ${r.after.live} buffer ${r.after.buffer} handles ${r.after.handles} (open: ${r.during?.handles}) sessions ${r.after.sessions} shells ${r.after.shellEngines} pageEnvs ${r.after.pageEnvs}`);
  }
  console.log(`memory: ${SOAK} create/load/type/save/remove cycles of ${cycle.length} documents in ${((Date.now() - t0) / 1000).toFixed(0)} s`);
  if (series.some((s) => s.handles !== 0 || s.sessions !== 0 || s.shellEngines !== 0)) fail(`memory: handles/sessions/engines alive after remove in ${series.filter((s) => s.handles || s.sessions || s.shellEngines).length} samples`);
  // `editors` = the connected engine="rust" elements: 1 while open, 0 after each remove (the disconnect parks the editor)
  const eds = [...new Set(series.map((s) => `${s.duringEditors}/${s.editors}`))];
  console.log(`  editors open/after remove: ${eds.join(', ')}`);
  if (series.some((s) => s.editors !== 0 || s.duringEditors !== 1)) fail(`memory: editors counter ${eds.join(', ')} (expected 1/0)`);
  const warm = cycle.length * 2;
  const after = series.slice(warm);
  if (after.some((s) => s.pageEnvs !== 1)) fail(`memory: page envs ${[...new Set(after.map((s) => s.pageEnvs))].join(', ')} (expected 1)`);
  // the live-bytes counter exists only in the count build (others answer null or -1): no claim without it
  const counted = series.every((s) => typeof s.live === 'number' && s.live >= 0);
  if (!counted) console.log('  live bytes: not a count build (UO_WASM_PATH=<--features session,count build>): the live-bytes check is skipped');
  const liveSet = new Set(after.filter((s) => counted && s.k % cycle.length === cycle.length - 1).map((s) => s.live));
  const bufSet = new Set(after.map((s) => s.buffer));
  console.log(`  after warm-up (${after.length} samples): live bytes at each cycle's end ${liveSet.size} distinct value(s) (${[...liveSet].slice(0, 4).join(', ')}), buffer ${bufSet.size} distinct (${[...bufSet].slice(0, 3).join(', ')})`);
  if (liveSet.size > 1) fail(`memory: live bytes not flat after warm-up (${[...liveSet].slice(0, 6).join(', ')})`);
  if (bufSet.size > 1) fail(`memory: the wasm memory grew after warm-up (${[...bufSet].join(', ')})`);
}

// ── latency ────────────────────────────────────────────────────────────────────────────────────
if (MODE === 'latency') {
  if (!existsSync(pages50)) fail(`latency: ${pages50} is absent (tools/bench/memory/gen_docs.mjs, seed 14, 310 paragraphs)`);
  else {
    const stat = (a) => { const s = [...a].sort((x, y) => x - y); return { median: +s[Math.floor(s.length / 2)].toFixed(2), p95: +s[Math.min(s.length - 1, Math.floor(s.length * 0.95))].toFixed(2) }; };
    for (const engine of ['ts', 'rust']) {
      await env.openPage();
      await env.page.evaluate(helpers);
      await ev((e) => window.__es.make(e), engine);
      await ev(() => window.__es.load('pages50.docx'));
      const r = await ev((n) => window.__es.latency(n), 200);
      console.log(`latency ${engine}: ${r.pages} pages; type ${JSON.stringify(stat(r.type))} ms; Enter ${JSON.stringify(stat(r.enter))} ms; Backspace ${JSON.stringify(stat(r.back))} ms`);
      if (engine === 'rust' && r.marks.length) {
        const m = (f) => stat(r.marks.map((x) => x[f]));
        const inc = r.marks.filter((x) => x.path === 'incremental').length;
        const reasons = {};
        for (const x of r.marks) if (x.reason) reasons[x.reason] = (reasons[x.reason] ?? 0) + 1;
        console.log(`  rust breakdown per typed key (median/p95 ms): exec ${JSON.stringify(m('exec'))}, relayout ${JSON.stringify(m('relayout'))}, adopt (handle + decode heads) ${JSON.stringify(m('adopt'))}, install + paint ${JSON.stringify(m('paint'))}; incremental ${inc}/${r.marks.length}; full reasons ${JSON.stringify(reasons)}`);
      }
      await ev(() => window.__es.remove());
    }
  }
}

// ── keys ───────────────────────────────────────────────────────────────────────────────────────
// The keys the shell does not run itself (ED-21), TS against Rust on the same document: characters typed with AltGr
// (CDP modifiers Ctrl+Alt = 3, as Windows reports AltGraph) and with the Mac Option key (Alt = 1) reach the document; then
// whether the browser's default of a key survives (a synthetic cancelable keydown on the hidden textarea: `dispatchEvent`
// answers false when a listener prevented it), for keys the TS leaves alone (F5, F11, Esc, Ctrl+P, Alt+F) and keys its
// `bindEvents` always prevents (Shift+Tab, Alt+ArrowLeft, F9, Ctrl+Enter, Ctrl+Alt+V). Every difference FAILS.
const ALTGR = [
  { mods: 3, key: '@', code: 'Digit2', vk: 50, text: '@' },
  { mods: 3, key: '€', code: 'KeyE', vk: 69, text: '€' },
  { mods: 1, key: '™', code: 'Digit2', vk: 50, text: '™' },
];
const DEFAULTS = [
  { key: 'F5', code: 'F5' }, { key: 'F11', code: 'F11' }, { key: 'Escape', code: 'Escape' }, { key: 'p', code: 'KeyP', ctrlKey: true },
  { key: 'f', code: 'KeyF', altKey: true }, { key: 'Tab', code: 'Tab', shiftKey: true }, { key: 'ArrowLeft', code: 'ArrowLeft', altKey: true },
  { key: 'F9', code: 'F9' }, { key: 'Enter', code: 'Enter', ctrlKey: true }, { key: 'v', code: 'KeyV', ctrlKey: true, altKey: true },
];
if (MODE === 'keys' || MODE === 'all') {
  const n = [...env.docsByName.keys()][0];
  const out = {};
  for (const engine of ['ts', 'rust']) {
    await env.openPage();
    await env.page.evaluate(helpers);
    const errs0 = env.errors.length;
    const session = await env.page.context().newCDPSession(env.page);
    await ev((e) => window.__es.make(e), engine);
    await ev((x) => window.__es.load(x), n);
    await ev(() => { const el = window.__es.el; el.setSelection({ para: 0, offset: 0 }); el.focusEditor(); window.__es.drain(); });
    const typed = [];
    for (const k of ALTGR) {
      await session.send('Input.dispatchKeyEvent', { type: 'keyDown', modifiers: k.mods, key: k.key, code: k.code, windowsVirtualKeyCode: k.vk, text: k.text, unmodifiedText: k.text });
      await session.send('Input.dispatchKeyEvent', { type: 'keyUp', modifiers: k.mods, key: k.key, code: k.code, windowsVirtualKeyCode: k.vk });
      typed.push(await ev(() => window.__es.el.getSelection().focus.offset));
    }
    const text = await ev(() => { const el = window.__es.el; el.setSelection({ para: 0, offset: 0 }, { para: 0, offset: 3 }); return el.selectedText(); });
    const defaults = await ev((list) => {
      const el = window.__es.el;
      el.setSelection({ para: 0, offset: 3 });
      el.focusEditor();
      return list.map((k) => (el.input.dispatchEvent(new KeyboardEvent('keydown', { ...k, bubbles: true, cancelable: true, composed: true })) ? 'default' : 'prevented'));
    }, DEFAULTS);
    out[engine] = { typed, text, defaults, errors: env.errors.length - errs0 };
    await ev(() => window.__es.remove());
  }
  const label = (k) => `${k.ctrlKey ? 'Ctrl+' : ''}${k.altKey ? 'Alt+' : ''}${k.shiftKey ? 'Shift+' : ''}${k.key}`;
  console.log(`keys: AltGr+2, AltGr+E, Option+2 → focus offsets ts ${JSON.stringify(out.ts.typed)} ${JSON.stringify(out.ts.text)}, rust ${JSON.stringify(out.rust.typed)} ${JSON.stringify(out.rust.text)}`);
  console.log(`keys: defaults ${DEFAULTS.map((k, i) => `${label(k)} ${out.ts.defaults[i]}/${out.rust.defaults[i]}`).join(', ')} (ts/rust)`);
  if (JSON.stringify(out.ts.typed) !== JSON.stringify(out.rust.typed) || out.ts.text !== out.rust.text) fail(`keys: AltGr/Option typing differs (ts ${JSON.stringify(out.ts)} rust ${JSON.stringify(out.rust)})`);
  if (out.rust.text !== '@€™') fail(`keys: the Rust run typed ${JSON.stringify(out.rust.text)} (expected "@€™")`);
  DEFAULTS.forEach((k, i) => { if (out.ts.defaults[i] !== out.rust.defaults[i]) fail(`keys: ${label(k)} ts ${out.ts.defaults[i]}, rust ${out.rust.defaults[i]}`); });
  if (out.ts.errors || out.rust.errors) fail(`keys: page errors ts ${out.ts.errors}, rust ${out.rust.errors}`);
}

// ── chrome ─────────────────────────────────────────────────────────────────────────────────────
if (MODE === 'chrome' || MODE === 'all') {
  const n = [...env.docsByName.keys()][0];
  for (const toolbar of ['compact', 'ribbon']) {
    await env.openPage();
    await env.page.evaluate(helpers);
    const errs0 = env.errors.length;
    await ev((t) => window.__es.make('rust', { topbar: true, toolbar: t }), toolbar);
    await ev((x) => window.__es.load(x), n);
    await env.page.waitForTimeout(500);
    const r = await ev(async () => {
      const es = window.__es;
      const el = es.el;
      el.setSelection({ para: 0, offset: 0 }, { para: 0, offset: 3 });
      const before = el.getSelectionState();
      const out = { boldBefore: !!before.run.bold, alignBefore: before.para.alignment ?? 'left' };
      out.bold = es.clickButton('bold');
      out.italic = es.clickButton('italic');
      out.center = es.clickButton('align:center');
      await new Promise((r2) => setTimeout(r2, 50));
      const after = el.getSelectionState();
      out.boldAfter = !!after.run.bold;
      out.italicAfter = !!after.run.italic;
      out.alignAfter = after.para.alignment ?? 'left';
      // a command the session does not take yet: the first of these the bar shows
      // wave 5, frontier R: link, comment and the clipboard buttons are routed now; the Format Painter and the font color are
      // routed by no frontier of the wave
      const off = ['painter', 'color'].find((id) => es.button(id));
      out.notRouted = off ? { id: off, ...es.buttonState(off) } : null;
      out.boldState = es.buttonState('bold');
      return out;
    });
    console.log(`chrome ${toolbar}: ${JSON.stringify(r)}; page errors ${env.errors.length - errs0}`);
    if (r.boldAfter === r.boldBefore || !r.italicAfter || r.alignAfter !== 'center') fail(`chrome ${toolbar}: the routed buttons did not change the session state`);
    if (!r.notRouted || !r.notRouted.disabled) fail(`chrome ${toolbar}: a non-routed command is not shown disabled (${JSON.stringify(r.notRouted)})`);
    // the bars' shortcuts with the focus in the document (real keys on the hidden textarea): a routed command runs, a
    // non-routed one stays disabled; the clipboard buttons are disabled (they need a clipboard event, ED-16)
    await ev(() => {
      const es = window.__es;
      const el = es.el;
      window.__sk = { save: 0, warns: [] };
      // the host takes the save (the bar's app.save asks it first): counts that Ctrl+S reached the bar's `save`
      el.addEventListener('uw-save-request', (e) => { window.__sk.save++; e.preventDefault(); });
      const warn = console.warn;
      console.warn = (...a) => { window.__sk.warns.push(String(a[0])); warn.apply(console, a); };
      el.setSelection({ para: 0, offset: 0 }, { para: 0, offset: 3 });
      el.focusEditor();
      window.__sk.size0 = el.getSelectionState().run.szHalfPt;
    });
    await env.page.keyboard.press('Control+s');
    await env.page.keyboard.press('Control+]');
    await env.page.keyboard.press('Control+k');
    await env.page.waitForTimeout(100);
    const sk = await ev(() => {
      const es = window.__es;
      const el = es.el;
      const dialogs = [...el.shadowRoot.querySelectorAll('dialog')].filter((d) => d.open).length;
      const clip = Object.fromEntries(['cut', 'copy', 'paste'].map((id) => [id, es.buttonState(id)]));
      return { ...window.__sk, size1: el.getSelectionState().run.szHalfPt, dialogs, clip, active: el.shadowRoot.activeElement?.className ?? null };
    });
    console.log(`chrome ${toolbar} shortcuts: ${JSON.stringify(sk)}`);
    if (sk.save !== 1) fail(`chrome ${toolbar}: Ctrl+S with the focus in the document did not reach the bar's save (${sk.save})`);
    if (!(sk.size1 > sk.size0)) fail(`chrome ${toolbar}: Ctrl+] did not grow the font (${sk.size0} → ${sk.size1})`);
    // wave 5, frontier R: Ctrl+K opens the hyperlink dialog (routed) and Cut/Copy/Paste are enabled with a selection (the
    // buttons go through navigator.clipboard, routed): the opposite of wave 4's assertions
    if (sk.dialogs !== 1 || sk.warns.some((w) => /link|Hyperlink/i.test(w))) fail(`chrome ${toolbar}: Ctrl+K did not open the hyperlink dialog (${sk.dialogs} dialog(s), ${sk.warns.join(' | ')})`);
    for (const [id, s] of Object.entries(sk.clip)) if (s && s.disabled) fail(`chrome ${toolbar}: the ${id} button is disabled (routed in wave 5)`);
    if (env.errors.length !== errs0) fail(`chrome ${toolbar}: ${env.errors.length - errs0} page error(s)`);
    await ev(() => window.__es.remove());
  }
}

await env.close();
console.log(failures ? `editor_switch: ${failures} FAILURE(S)` : 'editor_switch: ok');
process.exit(failures ? 1 : 0);
