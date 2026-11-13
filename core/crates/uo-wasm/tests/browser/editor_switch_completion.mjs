// Wave 7, frontier A2 of the editor switch (record core/docs/modules/uo-bindings.md "Wave 7 — A2: lists, paste, tables,
// objects and the page"): the lists (gallery, levels, list styles, numbering value, the list dialogs), the paste buttons and the
// cell paste options, the table tools (Border Painter, Eraser, Draw Table), the table edge drag and the move/size handles, the
// Quick Tables, the table style gallery and a document-defined style, the wrap points, Set Transparent Color, Select Objects
// (lasso, Shift+click, the moves and Delete of several), Edit Text, Page Borders and the TOC levels of
// `<universal-write engine="rust">` against the TS engine in a real headless Chromium. Built from editor_switch_objects.mjs (its
// runner, page helpers and comparison: a declared duplicate, as that harness's own).
//
//   UO_TS_ROOT=<frozen checkout> UO_WASM_PATH=<session .wasm> [UO_CHROMIUM=...] [UO_EC_SOAK=40] [UO_EC_ONLY=<regex of docs>]
//   [UO_EC_DUMP=1] node crates/uo-wasm/tests/browser/editor_switch_completion.mjs --mode lists|paste|tables|objects|page|memory|all
//   UO_EC_COVERED=1|2 lists the declared differences met, UO_EC_MEASURE=1 counts every difference as declared, UO_EC_ANSWERS=1
//   prints the step answers.
//
// Per mode and document: the TS element first, then the Rust one (`author="Ana"`, `lang="pt"`, the ribbon), the same script on
// both (element API calls, real keys and mouse events at the client point computed once on the TS element, bar buttons by id with
// the dialog's OK when asked); after each step the A2 facets (canonical JSON), at the end the page thumbnails, the screen
// canvases and the saved .docx part by part. Every difference not covered by DECLARED_C fails, and so does any page error.
import { createRequire } from 'node:module';
import { mkdirSync, writeFileSync } from 'node:fs';
import { join } from 'node:path';
import { setup, core } from './editor_switch_lib.mjs';

const root = process.env.UO_TS_ROOT;
if (!root) throw new Error('set UO_TS_ROOT to the frozen TS checkout');
const arg = (name, dflt) => { const i = process.argv.indexOf(name); return i >= 0 ? process.argv[i + 1] : dflt; };
const MODE = arg('--mode', 'all');
const SOAK = Number(process.env.UO_EC_SOAK ?? 30);
const DUMP = !!process.env.UO_EC_DUMP;
const target = process.env.CARGO_TARGET_DIR ?? join(core, 'target');
const wasmPath = process.env.UO_WASM_PATH ?? join(target, 'es-c/uo_wasm_session.wasm');
const outDir = join(target, 'es-c/harness');
const dumpDir = join(target, 'es-c/dump');
mkdirSync(outDir, { recursive: true });
const { unzipSync } = createRequire(join(root, 'write/package.json'))('fflate');
/** the chrome's command registry of the page (chrome.ts keeps it private): caught when the chrome adds its commands */
const ENTRY_EXTRA = `import { CommandRegistry } from '@universal-office/ui';
{ const add0 = CommandRegistry.prototype.add; CommandRegistry.prototype.add = function (...c) { window.__lastReg = this; return add0.apply(this, c); }; }`;

// UO_EC_PROBE=<docx>: `--mode probe` loads that document in both engines, no steps (a saved file of a failing run)
const env0 = await setup({ root, wasmPath, outDir, stride: 1, entryExtra: ENTRY_EXTRA, extraDocs: process.env.UO_EC_PROBE ? { 'probe.docx': process.env.UO_EC_PROBE } : {}, only: 'f7a__7__1\\.|f7b__7__(0|1|3)\\.|f7d__7__(3|10)\\.|f8__7__11\\.|f2-3-(0|1)\\.|f7c__7__2\\.' });
const env = env0;
await env.openPage(); await env.page.context().grantPermissions(['clipboard-read', 'clipboard-write']);
const fixtureName = (p) => p.replace(/\\/g, '/').replace(/^.*\//, '');
const docOf = (want) => { for (const [n, p] of env.docsByName) if (fixtureName(p) === want || n === want) return n; throw new Error(`no document ${want}`); };
let failures = 0;
const fail = (m) => { failures++; console.log(`FAIL ${m}`); };

/** the page-side helpers (evaluated in each new page) */
const helpers = () => {
  const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
  const frame = () => new Promise((r) => requestAnimationFrame(() => r()));
  const fnv = (u8) => { let h = 0x811c9dc5; for (let i = 0; i < u8.length; i++) h = Math.imul(h ^ u8[i], 0x01000193); return (h >>> 0).toString(16).padStart(8, '0'); };
  const EVENTS = ['uw-change', 'uw-dirty-change', 'uw-pasted', 'uw-protected', 'uw-tbmode', 'uw-cellmode', 'uw-hfmode', 'uw-blocked', 'uw-zoom'];
  /** the TS screen overlays no Rust front paints yet (off in the end capture; front R paints the marks, field shading,
   *  squiggles, search hits and the multi-selection, so those stay on, as in editor_switch_review.mjs) */
  const OTHERS = ['paintBookmarkMarks', 'paintTocControl', 'paintEngineInlines'];
  const log = [];
  const canon = (v) => (Array.isArray(v) ? v.map(canon) : v instanceof Uint8Array ? `#bytes:${v.length}:${fnv(v)}` : v && typeof v === 'object'
    ? Object.fromEntries(Object.entries(v).filter(([, x]) => x !== undefined && x !== null && x !== false).sort(([a], [b]) => (a < b ? -1 : 1)).map(([k, x]) => [k, canon(x)]))
    : typeof v === 'number' ? +v.toFixed(3) : v);
  const J = (v) => JSON.stringify(canon(v));
  const imgCache = {};
  // (wave 7c, S: defined outside `make` — a class created inside it closes over make's context, whose `el` kept the first
  // element alive for good: a removed element is freed when it is collected)
  const defineSheetStub = () => {
    if (!customElements.get('universal-sheet')) {
      customElements.define('universal-sheet', class extends HTMLElement {
        loadXlsx(b) { this.b = new Uint8Array(b); this.n = (window.__sheetLoads = (window.__sheetLoads ?? 0) + 1); }
        saveXlsx() { const out = new Uint8Array(this.b.length + 1); out.set(this.b); out[this.b.length] = 7; return out; }
        renderRegionToCanvas(_i, c, cols, rows) {
          c.width = 40 * cols; c.height = 20 * rows;
          const x = c.getContext('2d'); x.fillStyle = '#ffffff'; x.fillRect(0, 0, c.width, c.height);
          x.fillStyle = '#217346'; x.fillRect(0, 0, c.width, 20); x.fillRect(0, 0, 40, c.height);
          return { w: c.width, h: c.height };
        }
        debugCellDisplays() { return [{ ref: 'B2', display: '42' }]; }
      });
    }
  };
  const api = {
    el: null,
    async make(engine) {
      // the user's text box gallery lives in the page's storage: each run starts empty (the TS run must not feed the Rust one)
      try { localStorage.removeItem('uw:textbox-gallery'); } catch { /* none */ }
      // decision 6: a deterministic <universal-sheet> (the same answers for both engines): Insert/Edit Excel Spreadsheet
      defineSheetStub();
      const el = document.createElement('universal-write');
      if (engine === 'rust') el.setAttribute('engine', 'rust');
      el.setAttribute('fonts', '/fonts');
      el.setAttribute('author', 'Ana');
      el.setAttribute('lang', 'pt');
      el.setAttribute('topbar', '');
      el.setAttribute('toolbar', 'ribbon');
      el.style.cssText = 'display:block;width:1100px;height:850px';
      const ready = new Promise((r) => el.addEventListener('uw-ready', r, { once: true }));
      document.body.appendChild(el);
      await ready;
      if (el.rust) el.rust.tsShading = true; // wave 7c (V, fix round 1): the TS field shading/comment look (ED-304: Word's is the default)
      for (const t of EVENTS) el.addEventListener(t, (e) => { log.push(`${t} ${J(e.detail ?? null)}`); });
      // wave 7c (V): the onSelectionChange calls (the frames of a live drag count them)
      api.selChanges = 0;
      { const prev = el.onSelectionChange; el.onSelectionChange = function (...a) { api.selChanges++; return prev?.apply(this, a); }; }
      // UO_EC_TRACE=<names>: the TS element's (private) methods traced in the diagnostic facet (ED-218 measurements)
      if (engine !== 'rust' && window.__trace) for (const m of window.__trace) { const f0 = el[m]; if (typeof f0 === 'function') el[m] = function (...a) { const r = f0.apply(this, a); (api.traced ??= []).push(`${m}(${a.map((x) => (x instanceof Event ? x.type : typeof x === 'object' ? '{}' : String(x))).join(',')})=${r && typeof r === 'object' ? '{}' : String(r)}`); return r; }; }
      // UO_EC_TRACE: who scrolls the element (both engines; the stack of every scrollTop write)
      if (window.__trace) {
        const d = Object.getOwnPropertyDescriptor(Element.prototype, 'scrollTop');
        Object.defineProperty(el, 'scrollTop', { configurable: true, get() { return d.get.call(this); }, set(v) { if (Math.abs(v - d.get.call(this)) > 2 && !window.__harnessScroll) (window.__a2trace ??= []).push(`scrollTop ${d.get.call(this)}→${v} ${new Error().stack.split('\n').slice(2, 7).map((s) => s.trim().replace(/\(.*\//, '(')).join(' < ')}`); d.set.call(this, v); } });
      }
      api.el = el;
      for (let i = 0; i < 200 && !window.__lastReg; i++) await sleep(25);
      api.reg = window.__lastReg ?? null;
      return true;
    },
    async settle(ms = 300) {
      const el = api.el;
      if (el.rust) { await el.rust.idle(); await sleep(60); }
      else { await document.fonts.ready; await sleep(ms); }
      await frame();
    },
    async load(n) {
      window.__seed(1);
      if (n === 'blank') api.el.newDocument();
      else api.el.loadDocx(new Uint8Array(await (await fetch(`/docs/${n}`)).arrayBuffer()), { protected: false });
      await api.settle();
      api.el.refresh();
      await api.settle();
      // the TS draws its random ids (wp:docPr, w14:anchorId, w:id of a content control) from Math.random, the session from its
      // own xorshift32 seeded 1 at open: the same generator as this page's Math.random. Seeded again once the document is
      // open, the TS draws what the session draws as long as both take the same number of values per command (ED-217)
      window.__seed(1);
      log.length = 0;
      return api.el.layoutPageCount();
    },
    // (wave 7c, S: the harness's own references to the element's command registry go too — a removed element is freed
    // when it is collected, and the registry's commands close over it)
    remove() { api.el?.remove(); api.el = null; api.reg = null; window.__lastReg = null; },
    /** wave 7c (V): one frame of a live pointer drag (`frames: true` steps), after two animation frames (and the Rust
     *  engine's idle): the selection, the proxy textarea's left/top/height, the `onSelectionChange` calls and the uw-*
     *  events since the drag began, and the hash of every page canvas (the caret hidden, the other fronts' TS overlays off
     *  as at the end) — compared TS vs Rust frame by frame */
    /** the counters at the drag's start (the frames report what the drag itself did) */
    dragStart() { api.dragBase = api.selChanges ?? 0; api.dragLog = log.length; },
    async dragFrame() {
      const el = api.el;
      await frame(); await frame();
      if (el.rust) { await el.rust.idle(); await frame(); }
      window.clearInterval(el.caretTimer);
      el.caretVisible = false;
      if (!el.rust) for (const m of OTHERS) el[m] = () => {};
      // wave 7c (V, fix round 1): every allocated page buffer allocated again before the frame's paint, in both engines. Measured
      // (f7b__7__0 nw, f8__7__11 rot: the table-edge guide at x 246.27 / 246.0 px, coverage 59.5/255 and 127.5/255): the
      // same stroke with the same context state (transform, width, dash, alpha, composite, clip, attributes) rounds 1/255 apart
      // on a canvas whose buffer outlived several paints (the shell keeps the buffers across relayouts, D-L3) and on a freshly
      // allocated one (the TS allocates them again at every refresh); a fresh buffer in both makes the pixels history-free
      for (const cv of el.shadowRoot.querySelectorAll('canvas.page')) if (cv.width > 1) { const w = cv.width, h = cv.height; cv.width = 1; cv.width = w; cv.height = h; }
      el.paint();
      await frame();
      const sr = el.shadowRoot;
      const ta = sr.querySelector('textarea');
      const cs = [...sr.querySelectorAll('canvas.page')];
      return {
        sel: el.getSelection(),
        proxy: ta ? [ta.style.left, ta.style.top, ta.style.height] : null,
        selChanges: (api.selChanges ?? 0) - (api.dragBase ?? 0),
        events: log.slice(api.dragLog ?? 0),
        pages: cs.map((cv) => (cv.width > 1 ? fnv(new Uint8Array(cv.getContext('2d').getImageData(0, 0, cv.width, cv.height).data.buffer)) : '-')),
        // UO_EC_DUMP: the frame's page canvases as PNG (written for the frames that differ)
        ...(window.__dump ? { shots: cs.map((cv) => (cv.width > 1 ? cv.toDataURL() : null)) } : {}),
      };
    },
    drain() { const out = log.slice(); log.length = 0; return out; },
    focus() { api.el.focusEditor(); },
    /** a picture made in the page (the same bytes in both runs of the page's browser): PNG with an alpha region, or JPEG */
    async image(kind) {
      if (imgCache[kind]) return imgCache[kind];
      const [w, h] = kind === 'png' ? [300, 200] : [600, 40];
      const c = new OffscreenCanvas(w, h);
      const x = c.getContext('2d');
      x.fillStyle = '#2b579a'; x.fillRect(0, 0, w, h);
      x.fillStyle = '#f0c040'; x.fillRect(w / 4, h / 4, w / 2, h / 2);
      if (kind === 'png') x.clearRect(0, 0, w / 5, h / 5);
      const blob = await c.convertToBlob(kind === 'png' ? { type: 'image/png' } : { type: 'image/jpeg', quality: 0.9 });
      imgCache[kind] = new Uint8Array(await blob.arrayBuffer());
      return imgCache[kind];
    },
    /** a script step: JS over the element (`el`), the answer kept as a facet */
    async js(code) {
      const el = api.el;
      // eslint-disable-next-line no-new-func
      const f = new Function('el', 'es', `return (async () => { ${code} })();`);
      let r;
      try { r = await f(el, api); } catch (e) { r = `THROW ${e && e.message}`; }
      await api.settle(200);
      return J(r === undefined ? null : r);
    },
    /**
     * The TS run only: a client point of an object of the TS element — `what` = 'float:<i>' (the i-th of `enumFloats`, its
     * centre), 'handle:<i>:<name>' (an alça of the i-th float once selected), 'inline:<i>' (an inline picture), 'border:<i>'
     * (a point on the left border of the i-th float), 'text:<para>:<offset>' (a caret position). The scroll goes with it.
     */
    target(what) {
      const el = api.el;
      const [k, a, b] = what.split(':');
      const scale = (96 / 1440) * el.zoom;
      const at = (page, x, y) => {
        const cv = el.canvases[page];
        if (!cv) return null;
        const r0 = cv.getBoundingClientRect();
        const box = el.getBoundingClientRect();
        const cy = r0.top + y * scale;
        if (cy < box.top + 10 || cy > box.bottom - 10) el.scrollTop += cy - (box.top + box.height / 2);
        const r = cv.getBoundingClientRect();
        return { x: r.left + x * scale, y: r.top + y * scale, scrollTop: el.scrollTop };
      };
      if (k === 'float' || k === 'border' || k === 'handle') {
        // (the full-page backgrounds are click-through; `float`/`handle` count the objects that are not text boxes — a click in a box
        // enters its text, front S —, `border` the text boxes)
        const fl = el.enumFloats().filter((h) => h.rect.w * h.rect.h < 0.55 * (el.canvases[h.page].getBoundingClientRect().width / scale) * (el.canvases[h.page].getBoundingClientRect().height / scale))
          .filter((h) => (k === 'border') === (h.kind === 'textbox'));
        const h = fl[Number(a)];
        if (!h) return null;
        if (k === 'float') return { ...at(h.page, h.rect.x + h.rect.w / 2, h.rect.y + h.rect.h / 2), kind: h.kind };
        // 3 px inside the left edge: on the border for both engines (5 px tolerance), inside the box for both hit tests
        if (k === 'border') return { ...at(h.page, h.rect.x + 3 / scale, h.rect.y + h.rect.h / 2), kind: h.kind };
        at(h.page, h.rect.x + h.rect.w / 2, h.rect.y + h.rect.h / 2);
        const c = el.handleCenters({ ...h, ...(el.constructor.runRot ? {} : {}) })?.[b];
        return c ? { x: c.cx, y: c.cy, scrollTop: el.scrollTop, kind: h.kind } : null;
      }
      // wave 7 (A2): 'cell:<k>:<where>' (the k-th body cell box of page 0 of the TS layout: centre, left, right, bottom edge),
      // 'tbl:move' / 'tbl:size' (the handles of the first body table), 'wrap:<i>' (the i-th wrap point of the selected picture)
      if (k === 'cell') {
        const boxes = (el.eng?.result.pages[0]?.cells ?? []);
        const bx = boxes[Number(a)];
        if (!bx) return null;
        const w = b ?? 'center';
        const x = w === 'left' ? bx.x : w === 'right' ? bx.x + bx.w : bx.x + bx.w / 2;
        const y = w === 'bottom' ? bx.y + bx.h : bx.y + bx.h / 2;
        return { ...at(0, x, y), kind: 'cell' };
      }
      if (k === 'tbl') {
        const bb = el.tableBBox(0);
        if (!bb) return null;
        const px = 1 / scale;
        if (a === 'move') return { ...at(bb.first.pg, bb.first.x - 11 * px, bb.first.y - 11 * px), kind: 'move' };
        return { ...at(bb.last.pg, bb.last.x + 5 * px, bb.last.y + 5 * px), kind: 'size' };
      }
      if (k === 'wrap') {
        const h = el.selectedFloat();
        if (!h) return null;
        const pts = el.wrapPoints(h.obj);
        const p = pts[Number(a)];
        if (!p) return null;
        return { ...at(h.page, h.rect.x + (p[0] / 21600) * h.rect.w, h.rect.y + (p[1] / 21600) * h.rect.h), kind: 'wrap' };
      }
      if (k === 'page') {
        // a point of page 0 in twips: 'page:<x>:<y>'
        return { ...at(0, Number(a), Number(b)), kind: 'page' };
      }
      if (k === 'inline') {
        const all = el.enumInlinePics();
        const h = a === 'last' ? all[all.length - 1] : all[Number(a)];
        return h ? { ...at(h.page, h.rect.x + h.rect.w / 2, h.rect.y + h.rect.h / 2), kind: 'inline' } : null;
      }
      if (k === 'text') {
        const c = el.caretRectAt({ para: Number(a), offset: Number(b) });
        return c ? { x: c.x + 0.5, y: c.y + c.h / 2, scrollTop: el.scrollTop } : null;
      }
      return null;
    },
    /** wave 7 (A2): the gallery thumbnails painted twice (the second pass after the fonts and pictures settle), as hashes */
    async previews(kind) {
      const el = api.el;
      const hash = (c) => (c.width > 0 && c.height > 0 ? `${c.width}x${c.height}:${fnv(new Uint8Array(c.getContext('2d').getImageData(0, 0, c.width, c.height).data.buffer))}` : 'empty');
      const shot = (c, name) => { if (window.__dump && c.width > 0) (api.previewShots ??= {})[name] = c.toDataURL('image/png'); };
      const pass = async () => {
        const out = [];
        const each = async (name, f, w = 0, h = 0) => {
          const c = document.createElement('canvas');
          if (w) { c.width = w; c.height = h; }
          let ok;
          try { ok = await f(c); } catch (e) { ok = `THROW ${e?.message}`; }
          out.push(`${name}=${ok === false ? 'false' : ok === true || ok === undefined ? hash(c) : String(ok)}`);
          shot(c, name);
          c.width = c.height = 0;
        };
        if (kind === 'cover') for (const t of el.coverPages()) await each(t.id, (c) => el.paintCoverTemplate(t.id, c, 120));
        if (kind === 'watermark') for (const t of el.watermarkGallery()) await each(t.id, (c) => el.paintWatermarkTemplate(t.id, c, 120));
        if (kind === 'toc') for (const t of el.tocTemplates()) await each(t.id, (c) => el.paintTocTemplate(t.id, c, 160));
        if (kind === 'hf') for (const g of ['header', 'footer', 'pageTop', 'pageBottom', 'pageMargin', 'pageCurrent']) for (const t of el.headerFooterTemplates(g)) await each(`${g}:${t.id}`, (c) => el.paintHeaderFooterTemplate(g, t.id, c, 200));
        if (kind === 'tableStyle') for (const t of (await el.tableStyleList()).slice(0, 24)) await each(t.name, (c) => el.renderTableStyleThumb(c, t.name, { firstRow: true, firstColumn: true, noVBand: true }), 72, 54);
        if (kind === 'tableStyleXml') for (const n of ['Grid Table 4 Accent 1', 'Table Grid', 'List Table 3 Accent 2']) {
          const x = await el.tableStyleXml(n);
          if (x) await each(n, (c) => el.renderTableStyleXmlThumb(c, x.xml, { firstRow: true, lastRow: true, firstColumn: true }), 360, 180);
        }
        return out;
      };
      await pass();
      await api.settle(500);
      return pass();
    },
    async dumpPreviews(prefix) { const o = api.previewShots ?? {}; api.previewShots = {}; return Object.entries(o).map(([k, v]) => [`${prefix}-${k}`, v]); },
    scrollTo(top) { window.__harnessScroll = true; api.el.scrollTop = top; window.__harnessScroll = false; },
    button(id) {
      const find = (n) => { if (n.dataset?.a === id) return n; for (const c of n.children ?? []) { const f = find(c); if (f) return f; } if (n.shadowRoot) return find(n.shadowRoot); return null; };
      return find(api.el.shadowRoot);
    },
    async clickButton(id) {
      const bt = api.button(id);
      if (!bt) {
        // a menu item (no button): the chrome registry runs it as the menu does
        const reg = api.reg;
        if (!reg) return 'missing';
        const en = reg.enabled(id);
        if (en) { try { const r = reg.run(id); if (r && typeof r.then === 'function') await r; } catch (e) { return `threw ${e?.message}`; } }
        await api.settle(250);
        return en ? 'ran' : 'disabled';
      }
      const off = !!(bt.disabled || bt.getAttribute?.('aria-disabled') === 'true' || bt.classList?.contains('disabled'));
      bt.dispatchEvent(new MouseEvent('mousedown', { bubbles: true, composed: true }));
      bt.click();
      await api.settle(200);
      return off ? 'disabled' : 'clicked';
    },
    /** the open dialogs (the bars' dialogs reached by a command); `ok`: press the dialog's primary button */
    dialogs() { return [...api.el.shadowRoot.querySelectorAll('dialog')].filter((d) => d.open).map((d) => d.innerText.replace(/\s+/g, ' ').trim().slice(0, 160)); },
    async dialogOk(edit) {
      const d = [...api.el.shadowRoot.querySelectorAll('dialog')].find((x) => x.open);
      if (!d) return 'no dialog';
      if (edit) {
        // eslint-disable-next-line no-new-func
        await new Function('d', `return (async () => { ${edit} })();`)(d);
      }
      const ok = d.querySelector('.uo-pill-primary') ?? [...d.querySelectorAll('button')].find((b) => b.dataset?.id === 'ok' || /^(OK|Ok)$/.test(b.textContent?.trim() ?? ''));
      if (!ok) { d.close(); return 'no ok'; }
      ok.click();
      await api.settle(250);
      return 'ok';
    },
    closeDialogs() { for (const d of api.el.shadowRoot.querySelectorAll('dialog')) if (d.open) d.close(); },
    /** the menu the context menu opened (labels + disabled) and close it */
    menu() {
      const items = [...api.el.shadowRoot.querySelectorAll('.uo-menu .uo-menu-item')].filter((n) => n.getClientRects().length > 0);
      return items.map((n) => `${n.textContent.replace(/\s+/g, ' ').trim()}${n.getAttribute('aria-disabled') === 'true' || n.classList.contains('disabled') || n.disabled ? ' [off]' : ''}`);
    },
    // outside the element (a mousedown on the page body): the menu closes, the element's own mousedown listeners do not run
    closeMenu() { document.dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape', bubbles: true })); document.body.dispatchEvent(new MouseEvent('mousedown', { bubbles: true, composed: true })); },
    /** a paste on the hidden textarea: an image-only item (`image`) or an HTML fragment with a data-URI picture (`html`) */
    async paste(kind) {
      const el = api.el;
      const dt = new DataTransfer();
      const png = await api.image('png');
      if (kind === 'image') dt.items.add(new File([png], 'p.png', { type: 'image/png' }));
      else {
        let bin = '';
        for (let i = 0; i < png.length; i++) bin += String.fromCharCode(png[i]);
        dt.setData('text/html', `<p>antes <img src="data:image/png;base64,${btoa(bin)}" width="60" height="40"> depois</p>`);
        dt.setData('text/plain', 'antes  depois');
      }
      el.focusEditor();
      el.input.dispatchEvent(new ClipboardEvent('paste', { clipboardData: dt, bubbles: true, cancelable: true, composed: true }));
      await sleep(400);
      await api.settle(300);
      return true;
    },
    /** the A2 facets after a step */
    async facets() {
      const el = api.el;
      const f = { sel: el.getSelection(), events: api.drain(), dirty: el.isDirty(), pages: el.layoutPageCount() };
      await sleep(30);
      f.list = el.listInfo();
      f.docLists = el.documentLists();
      f.listStyles = el.listStylesInDocument();
      f.value = el.numberingValueInfo();
      f.table = el.getTableState();
      f.tool = el.getTableTool();
      f.cells = el.hasCellsClip();
      f.ring = el.clipboardItems().map((x) => [x.kind, x.text]);
      f.selectMode = el.isSelectObjectsMode();
      f.count = el.selectedObjectCount();
      f.wrap = el.editingWrapPoints();
      f.image = el.selectedImage();
      const shp = el.selectedShape();
      // wave 7 (A2): the whole ShapeFormat (fill, style, text box, body, story: ED-213 closed)
      f.shape = shp;
      f.tb = el.editingTextbox();
      f.toc = el.getTocLevel();
      f.borders = el.getPageBorders();
      // UO_EC_TRACE: the TS methods traced (above) and the Rust shell's pointer trace (`window.__a2trace`, the table hover)
      if (window.__trace) f.__diag = [...(api.traced?.splice(0) ?? []), ...(window.__a2trace?.splice(0) ?? [])];
      return J(f);
    },
    async end() {
      const el = api.el;
      el.refresh();
      await api.settle();
      const cs = [...el.shadowRoot.querySelectorAll('canvas.page')];
      const sizes = cs.map((cv) => `${cv.style.width}x${cv.style.height}`);
      // a first pass starts the image decodes of the pages (the Rust painter decodes pictures asynchronously, as the viewer)
      for (let i = 0; i < cs.length; i++) { const t = document.createElement('canvas'); el.paintPageThumbnail(i, t, parseFloat(cs[i].style.width)); t.width = t.height = 0; }
      await sleep(400);
      const thumbs = [];
      for (let i = 0; i < cs.length; i++) {
        const t = document.createElement('canvas');
        const ok = el.paintPageThumbnail(i, t, parseFloat(cs[i].style.width));
        thumbs.push(ok ? fnv(new Uint8Array(t.getContext('2d').getImageData(0, 0, t.width, t.height).data.buffer)) : 'none');
        if (window.__dump && ok) (api.thumbShots ??= [])[i] = t.toDataURL('image/png');
        t.width = t.height = 0;
      }
      el.scrollTop = 0;
      await frame(); await frame();
      el.ensureVisiblePainted();
      window.clearInterval(el.caretTimer);
      el.caretVisible = false;
      if (!el.rust) for (const m of OTHERS) el[m] = () => {};
      el.paint();
      await frame();
      const screen = cs.map((cv) => (cv.width > 1 ? fnv(new Uint8Array(cv.getContext('2d').getImageData(0, 0, cv.width, cv.height).data.buffer)) : '-'));
      const shots = window.__dump ? cs.map((cv) => (cv.width > 1 ? cv.toDataURL('image/png') : null)) : [];
      const bytes = el.saveDocx();
      let bin = '';
      for (let i = 0; i < bytes.length; i += 0x8000) bin += String.fromCharCode(...bytes.subarray(i, i + 0x8000));
      const thumbShots = api.thumbShots ?? [];
      api.thumbShots = [];
      return { pages: cs.length, sizes, thumbs, screen, shots, thumbShots, docx: btoa(bin) };
    },
    stats() { return window.__e.UniversalWriteElement.engineStats(); },
  };
  window.__es = api;
  return true;
};
const ev = (f, a) => env.page.evaluate(f, a);

// ── the scripts ──
// step: { js } | { key } | { type } | { button, ok?, edit? } | { target, mouse: 'click'|'rclick'|'shift'|'drag', dx?, dy? } |
//       { paste: 'image'|'html' } | { menu: true } (after a right click: the opened menu's items)
const P = (p, o) => `{ para: ${p}, offset: ${o} }`;
const LV = (ilvl, numFmt, lvlText, indLeft, hanging) => `{ ilvl: ${ilvl}, numFmt: '${numFmt}', lvlText: '${lvlText}', indLeft: ${indLeft}, hanging: ${hanging} }`;
const SCRIPTS = {
  lists: {
    docs: ['f2-3-0.docx', 'f2-3-1.docx', 'f7c__7__2.docx', 'blank'],
    steps: [
      { js: 'return [el.listInfo(), el.documentLists().length];' },
      { js: `el.setSelection(${P(1, 0)}); return [el.listInfo(), el.numberingValueInfo()];` },
      { js: `el.applyListLibrary('bullet', 2); return el.listInfo();` },
      { js: `el.applyListLibrary('number', 1); return el.listInfo();` },
      { js: 'el.applyMultilevelLibrary(1); return el.listInfo();' },
      { js: 'el.setListLevel(1); return el.listInfo();' },
      { js: 'el.restartNumbering(3); return el.numberingValueInfo();' },
      { js: 'return el.continueNumbering();' },
      { js: `el.setNumberingValue({ mode: 'continue', value: 5 }); return el.numberingValueInfo();` },
      { js: `el.defineListLevel({ numFmt: 'upperRoman', lvlText: '%1)', align: 'right', bold: true }); return el.listInfo();` },
      { js: `el.defineListLevel({ bullet: '»', font: 'Liberation Sans', color: 'FF0000' }); return el.listInfo();` },
      { js: `el.defineMultilevelList([${LV(0, 'decimal', '%1.', 360, 360)}, ${LV(1, 'lowerLetter', '%1.%2', 720, 360)}]); return el.listInfo();` },
      { js: `return el.createListStyle('Estilo R', [${LV(0, 'decimal', '(%1)', 400, 400)}]);` },
      { js: 'return el.listStylesInDocument();' },
      { js: `const s = el.listStylesInDocument()[0]; if (s) el.modifyListStyle(s.id, [${LV(0, 'upperLetter', '%1.', 500, 300)}]); return el.listStylesInDocument();` },
      { js: `el.setSelection(${P(0, 0)}); const s = el.listStylesInDocument()[0]; if (s) el.applyListStyle(s.id); return el.listInfo();` },
      { js: `el.applyListLevels(el.listLibrary('number')[3]); return el.listInfo();` },
      { js: `return [el.listLibrary('bullet').length, el.listLibrary('multilevel')[2]];` },
      { js: `return el.markerPreview('\\uF0B7', 'Symbol');` },
      // wave 7 (A2): a picture bullet (the session's DefineListLevelPicture)
      { js: `el.defineListLevel({ picture: { data: await es.image('png'), ext: 'png', name: 'bola' } }); return el.listInfo();` },
      { button: 'list:restart' },
      { button: 'list:continue' },
      { button: 'list:setValue', ok: true },
      { button: 'list:defineBullet', ok: true },
      { button: 'list:defineNumber', ok: true },
      { button: 'list:defineMultilevel', ok: true },
      { button: 'list:defineStyle', ok: true, edit: `const i = d.querySelector('input[type=text], input:not([type])'); if (i) { i.value = 'Lista do Dialogo'; i.dispatchEvent(new Event('input', { bubbles: true })); }` },
      { button: 'list:indents', ok: true },
      { key: 'Control+z' },
      { key: 'Control+z' },
    ],
  },
  paste: {
    docs: ['f2-3-0.docx', 'blank'],
    steps: [
      { js: `el.setSelection(${P(0, 0)}); el.focusEditor(); return null;` },
      { type: 'Texto copiado' },
      { key: 'Shift+Home' },
      { key: 'Control+c' },
      { js: `el.setSelection(${P(0, 0)}); return el.hasCellsClip();` },
      { button: 'paste:keep' },
      { button: 'paste:merge' },
      { button: 'paste:text' },
      // wave 7 (A2, fix round): in the body (before the cell steps): the Office Clipboard's IMAGE item pasted from the ring, then
      // Change Picture › From Clipboard (the system clipboard's PNG)
      { js: `el.setSelection(${P(0, 0)}); await el.insertImage(await es.image('png'), 'png'); return null;` },
      { target: 'inline:0', mouse: 'click' },
      { key: 'Control+c' },
      { js: `const it = el.clipboardItems().find((x) => x.kind === 'image'); el.setSelection(${P(0, 0)}); if (it) el.pasteClipboardItem(it.id); return [it?.kind ?? null, el.clipboardItems().map((x) => x.kind), el.getSelection()];` },
      { js: `const c = new OffscreenCanvas(120, 80); const x = c.getContext('2d'); x.fillStyle = '#c00000'; x.fillRect(0, 0, 120, 80); x.fillStyle = '#ffffff'; x.fillRect(30, 20, 60, 40); const b = await c.convertToBlob({ type: 'image/png' }); let ok = true; try { await navigator.clipboard.write([new ClipboardItem({ 'image/png': b })]); } catch (e) { ok = String(e?.name); } return ok;` },
      { target: 'inline:0', mouse: 'click' },
      { button: 'pic:change:clip' },
      { js: `return [el.selectedImage()?.widthPt ?? null, el.selectedImage()?.heightPt ?? null];` },
      { js: 'el.insertTable(2, 3); el.selectTable(); return el.getTableState()?.rows;' },
      { key: 'Control+c' },
      { js: 'return [el.hasCellsClip(), el.clipboardItems().map((x) => x.kind)];' },
      { js: `el.setSelection(${P(0, 0)}); return el.pasteCells('text');` },
      { js: `el.setSelection(${P(0, 0)}); return null;` },
      { button: 'paste:cells:overwrite' },
      { js: 'el.insertTable(1, 2); return el.pasteCells(\'rows\');' },
      { button: 'paste:cells:nest' },
      { button: 'paste:cells:overwrite' },
      { js: `el.setSelection(${P(0, 0)}); el.pasteData({ text: 'dados R', html: '<p><b>dados</b> R</p>' }); return null;` },
      { js: `el.pasteData({ text: 'so texto' }, 'text'); return null;` },
      { key: 'Control+z' },
      // wave 7 (A2): Paste as Picture (the session's renderClip job), Paste Special › Paste Link in the same element and from a
      // second element of the page (decision 3)
      { js: `el.setSelection(${P(0, 0)}); el.focusEditor(); return null;` },
      { type: 'Imagem colada' },
      { key: 'Shift+Home' },
      { key: 'Control+c' },
      { key: 'End' },
      { button: 'paste:picture' },
      { js: `return [el.selectedImage()?.widthPt ?? null, el.selectedImage()?.heightPt ?? null];` },
      { js: `el.documentPath = 'C:\\\\docs\\\\origem.docx'; el.setSelection(${P(0, 0)}); el.focusEditor(); return null;` },
      { type: 'Vinculado' },
      { key: 'Shift+Home' },
      { key: 'Control+c' },
      { key: 'End' },
      { js: `const f = await el.pasteSpecialFormats(); await el.pasteSpecial('rtf', { link: f.link }); return [f.link, f.formats, el.getBookmarks({ hidden: true }).map((b) => b.name)];` },
      { js: `const el2 = document.createElement('universal-write'); if (el.getAttribute('engine')) el2.setAttribute('engine', el.getAttribute('engine')); el2.setAttribute('fonts', '/fonts'); el2.style.cssText = 'display:block;width:600px;height:300px'; const ready = new Promise((r) => el2.addEventListener('uw-ready', r, { once: true })); document.body.appendChild(el2); await ready; el2.newDocument(); await new Promise((r) => setTimeout(r, 300)); el2.documentPath = 'C:\\\\docs\\\\outro.docx'; el2.pasteData({ text: 'Texto da origem' }, 'text'); el2.setSelection({ para: 0, offset: 0 }, { para: 0, offset: 15 }); await el2.copy(); await new Promise((r) => setTimeout(r, 200)); el.setSelection(${P(0, 0)}); el.focusEditor(); const f = await el.pasteSpecialFormats(); await el.pasteSpecial('text', { link: f.link }); const marks = el2.getBookmarks({ hidden: true }).map((b) => b.name); el2.remove(); return [f.link, f.source, marks];` },
      { key: 'Control+z' },
      // wave 7 (A2, fix round): the Paste Options re-paste (repaste after a paste: Text Only, Merge, Keep Source)
      { js: `el.setSelection(${P(0, 0)}); el.pasteData({ text: 'opcoes R', html: '<p><b>opcoes</b> <i>R</i></p>' }); return el.pasteOptions();` },
      { js: `el.repaste('text'); return el.pasteOptions();` },
      { js: `el.repaste('merge'); return el.pasteOptions();` },
      { js: `el.repaste('keep'); return el.pasteOptions();` },
    ],
  },
  tables: {
    docs: ['blank', 'f2-3-0.docx'],
    steps: [
      { js: 'return [el.quickTables(), el.canEmbedSpreadsheet()];' },
      { key: 'Control+End' },
      { js: `return await el.insertQuickTable('calendar');` },
      { key: 'Control+End' },
      { js: `return await el.insertQuickTable('tabular');` },
      { key: 'Control+End' },
      { js: `return await el.insertQuickTable('matrix');` },
      { key: 'Control+End' },
      { js: `return await el.insertQuickTable('subheads');` },
      { key: 'Control+End' },
      { js: `return await el.insertQuickTable('double');` },
      { button: 'table:painter' },
      { target: 'cell:3:bottom', mouse: 'click' },
      { key: 'Escape' },
      { button: 'table:eraser' },
      { target: 'cell:4:right', mouse: 'click' },
      { key: 'Escape' },
      { button: 'table:draw' },
      { target: 'cell:9:center', mouse: 'drag', dx: 0, dy: 40 },
      { key: 'Escape' },
      { target: 'cell:1:right', mouse: 'hoverdrag', dx: 30, dy: 0 },
      { target: 'cell:2:bottom', mouse: 'hoverdrag', dx: 0, dy: 20 },
      { target: 'cell:0:left', mouse: 'hoverdrag', dx: 25, dy: 0 },
      { target: 'tbl:size', mouse: 'hoverdrag', dx: 40, dy: 30 },
      { target: 'tbl:move', mouse: 'handledrag', dx: 2, dy: 1 },
      { target: 'tbl:move', mouse: 'handledrag', dx: 80, dy: 60 },
      { js: 'const l = await el.tableStyleList(); return [l.length, l.slice(0, 6).map((x) => x.name), el.tableStyleListNow()?.length];' },
      { js: `const x = await el.tableStyleXml('Grid Table 4 Accent 1'); const id = el.newTableStyleId('Meu Estilo R'); el.setSelection(${P(0, 0)}); return [x?.inDoc, id, el.tableStyleBases().length];` },
      { key: 'Control+End' },
      { js: 'el.insertTable(2, 2); return null;' },
      { js: `const x = await el.tableStyleXml('Grid Table 4 Accent 1'); const id = el.newTableStyleId('Meu Estilo R'); return el.saveTableStyle(x.xml.replace(/w:styleId="[^"]*"/, 'w:styleId="' + id + '"').replace(/<w:name w:val="[^"]*"\\/>/, '<w:name w:val="Meu Estilo R"/>'), { apply: true });` },
      { js: `return [(await el.tableStyleList()).filter((x) => x.group === 'custom'), el.tableStyleBases().map((x) => x.name)];` },
      { js: `el.setTableStyle('Meu Estilo R'); return el.getTableState()?.style;` },
      { key: 'Control+z' },
      { key: 'Control+z' },
      // wave 7 (A2): the New / Modify Table Style dialogs to OK (their preview: renderTableStyleXmlThumb), Insert › Excel
      // Spreadsheet and its double-click edit (the deterministic <universal-sheet> of the page, decision 6)
      { button: 'table:style:new', ok: true, edit: `const i = d.querySelector('input[type=text], input:not([type])'); if (i) { i.value = 'Estilo do Dialogo'; i.dispatchEvent(new Event('input', { bubbles: true })); }` },
      { js: `return (await el.tableStyleList()).filter((x) => x.group === 'custom').map((x) => x.name);` },
      { button: 'table:style:modify', ok: true },
      { key: 'Control+End' },
      { button: 'table:excel', ok: true },
      { js: 'return el.getSelection();' },
      { target: 'inline:0', mouse: 'dblclick' },
      { dialogOk: true },
      { js: `return [window.__sheetLoads ?? 0, el.selectedImage()?.widthPt ?? null];` },
      { key: 'Control+z' },
    ],
  },
  objects: {
    docs: ['blank', 'f7d__7__10.docx', 'f7b__7__3.docx'],
    steps: [
      { js: `el.setSelection(${P(0, 0)}); await el.insertImage(await es.image('png'), 'png'); return null;` },
      { target: 'inline:0', mouse: 'click' },
      { button: 'pic:wrap:square' },
      { button: 'pic:wrap:editPoints' },
      { js: 'return el.editingWrapPoints();' },
      { target: 'wrap:1', mouse: 'drag', dx: 25, dy: 15, frames: true },
      { target: 'wrap:2', mouse: 'ctrl' },
      { key: 'Escape' },
      { js: 'return el.editingWrapPoints();' },
      { button: 'pic:transparentColor' },
      { target: 'float:0', mouse: 'click' },
      { js: 'return [el.pageFillOptions().presets.length, el.shapeGradientVariations().length, el.resolveDmlColor({ kind: \'schemeClr\', val: \'accent2\', mods: [[\'lumMod\', 75000]] })];' },
      { js: `el.setSelection(${P(0, 0)}); el.insertTextbox('simple'); return el.editingTextbox();` },
      { key: 'Escape' },
      { button: 'shp:editText' },
      { js: 'return el.editingTextbox();' },
      { key: 'Escape' },
      { key: 'Escape' },
      { button: 'select:objects' },
      { target: 'page:200:200', mouse: 'drag', dx: 700, dy: 600 },
      { js: 'return [el.isSelectObjectsMode(), el.selectedObjectCount()];' },
      { key: 'ArrowRight' },
      { key: 'ArrowDown' },
      { key: 'Escape' },
      { target: 'float:0', mouse: 'click' },
      { target: 'float:1', mouse: 'shift' },
      { js: 'return el.selectedObjectCount();' },
      { key: 'ArrowLeft' },
      { key: 'Delete' },
      { key: 'Control+z' },
      { js: 'return [el.textboxTemplates().length, el.removeTextboxGalleryItem(\'user:none\') ?? null];' },
      // wave 7 (A2): Save Selection to Text Box Gallery, the user's item inserted and removed
      { js: `el.setSelection(${P(0, 0)}); el.insertTextbox('simple'); return el.editingTextbox();` },
      { key: 'Escape' },
      { js: `const ok = el.saveSelectionToTextboxGallery('Minha Caixa'); const t = el.textboxTemplates().filter((x) => x.user); return [ok, t.map((x) => x.name)];` },
      { key: 'Escape' },
      { key: 'Escape' },
      { js: `el.setSelection(${P(0, 0)}); const u = el.textboxTemplates().find((x) => x.user); const r = u ? el.insertTextbox(u.id) : null; return [r, el.editingTextbox()];` },
      { key: 'Escape' },
      { js: `const u = el.textboxTemplates().find((x) => x.user); if (u) el.removeTextboxGalleryItem(u.id); return el.textboxTemplates().filter((x) => x.user).length;` },
      // wave 7 (A2): the drag commits (ED-202: move, resize, rotate), the Crop drag (ED-215), the inline picture drag (ED-204)
      { key: 'Escape' },
      { target: 'float:0', mouse: 'click' },
      { target: 'float:0', mouse: 'drag', dx: 40, dy: 25, frames: true },
      { target: 'handle:0:se', mouse: 'drag', dx: 30, dy: 20, frames: true },
      { target: 'handle:0:rot', mouse: 'drag', dx: 40, dy: 10, frames: true },
      { key: 'Control+z' },
      { target: 'float:0', mouse: 'click' },
      { button: 'pic:crop:mode' },
      { target: 'handle:0:w', mouse: 'drag', dx: 25, dy: 0, frames: true },
      { target: 'handle:0:n', mouse: 'drag', dx: 0, dy: 15, frames: true },
      { key: 'Escape' },
      // wave 7 (A2, fix round): an EXISTING text box (the document's, or the one inserted above): its border selects the shape,
      // Edit Text enters it, Esc goes back to the shape selected (the S/O scope), Esc again leaves it
      { key: 'Escape' },
      { target: 'border:0', mouse: 'click' },
      { js: 'return [el.editingTextbox(), el.selectedObjectCount()];' },
      { button: 'shp:editText' },
      { js: 'return [el.editingTextbox(), el.selectedObjectCount()];' },
      { key: 'Escape' },
      { js: 'return [el.editingTextbox(), el.selectedObjectCount()];' },
      { key: 'Escape' },
    ],
  },
  // wave 7 (A2): dragging an inline picture in the text (ED-204): a drop past the picture, a drop on itself, a click
  inline: {
    docs: ['blank', 'f2-3-0.docx'],
    steps: [
      { js: `el.setSelection(${P(0, 0)}); el.focusEditor(); return null;` },
      { type: 'Texto antes e depois da imagem arrastada no meio da linha de texto.' },
      { js: `el.setSelection(${P(0, 0)}); await el.insertImage(await es.image('png'), 'png'); return el.getSelection();` },
      { target: 'inline:0', mouse: 'click' },
      { js: 'return [el.getSelection(), el.selectedObjectCount()];' },
      { target: 'inline:0', mouse: 'drag', dx: 320, dy: 60, frames: true },
      { js: 'return [el.getSelection(), el.selectedObjectCount()];' },
      { target: 'inline:0', mouse: 'drag', dx: 20, dy: 0, frames: true },
      { js: 'return [el.getSelection(), el.selectedObjectCount()];' },
      { key: 'Control+z' },
      { key: 'Control+y' },
    ],
  },
  page: {
    docs: ['f7a__7__1.docx'],
    steps: [
      { js: `el.setSelection(${P(0, 0)}); return el.getTocLevel();` },
      { button: 'toc:level1' },
      { button: 'toc:level3' },
      { button: 'toc:level0' },
      { button: 'dlg:pageBorders', ok: true },
      { js: 'return el.getPageBorders();' },
      { key: 'Control+z' },
      { js: 'return [el.sectPrAt(0), el.sectPrAt(99)];' },
      { js: `const x = el.headerFooterTemplateXml('header', el.headerFooterTemplates('header')[1].id); return [typeof x, x?.length ?? 0, x?.replace(/(id|anchorId|editId)="[^"]*"/g, '')];` },
      { js: `return el.headerFooterTemplateXml('footer', 'none:at:all');` },
      { js: `el.setWatermark({ kind: 'text', text: 'RASCUNHO', font: 'Calibri', sizePt: 72, color: 'C0C0C0', semitransparent: true, layout: 'diagonal' }); return el.watermark();` },
      { js: `el.setWatermark({ kind: 'text', text: 'Minuta', font: 'Liberation Serif', sizePt: 54, color: 'FF0000', semitransparent: false, layout: 'horizontal' }); return el.watermark();` },
    ],
  },
  // wave 7 (A2): the gallery thumbnails (covers, watermarks, TOC, header/footer, table styles and the dialog's preview)
  previews: {
    docs: ['f7b__7__0.docx', 'f2-3-0.docx', 'blank'],
    steps: [
      { js: `return await es.previews('cover');` },
      { js: `return await es.previews('watermark');` },
      { js: `return await es.previews('toc');` },
      { js: `return await es.previews('hf');` },
      { js: `return await es.previews('tableStyle');` },
      { js: `return await es.previews('tableStyleXml');` },
    ],
  },
  probe: {
    docs: ['probe.docx'],
    steps: [{ js: 'return null;' }],
  },
};

/** the docx parts of a saved document (base64 → { name: fnv }) */
const parts = (b64) => {
  const z = unzipSync(new Uint8Array(Buffer.from(b64, 'base64')));
  const fnv = (u8) => { let h = 0x811c9dc5; for (let i = 0; i < u8.length; i++) h = Math.imul(h ^ u8[i], 0x01000193); return (h >>> 0).toString(16); };
  return Object.fromEntries(Object.entries(z).map(([k, v]) => [k, fnv(v)]));
};

/**
 * Declared differences: `doc|mode` → { from: first step, facets: the keys of the step facets (or '*'), end: end facets ('*'),
 * id (the ED of the record), why }. Everything else fails.
 */
// wave 7 (B): measured on the wave-7 final runs (UO_EC_DUMP crops). The two items B left undeclared for A2 are closed: f2-3-0|tables
// step 24 (the move-handle drag: equal since A2's fix round; only the footer shading band of the end screens is left, declared
// below) and blank|objects (the end thumbnail: equal in the re-verification runs with A2's later tree).
// wave 7c (V): ED-304 closed — the shell's field shading, comment highlight, bookmark brackets, squiggles and search hits
// read the lines of every story (uo_shell_story_lines, the TS `eng.lines`); FIELD_SHADING and its seven entries removed
const DECLARED_C = {
  // wave 7 (B, re-verification): MARKER_CSS (ED-C1) retired — equal on the four documents since A2's shellFonts
  // (rust-completion-lists.ts: the shell's font registry gets the page environment's families)
  // wave 7c (W): Word's layout rules over the frozen TS, each document attributed natively to ONE rule (uo-docx-layout
  // word_w7c.rs attribute_documents_to_the_word_rules) and measured in Word 16 (record uo-bindings.md "Wave 7c — W").
  // Item 4: f7c__7__2's REF to the unterminated bm2 (page 10) is Word's bold error text, the TS the paragraph text
  'f7c__7__2|lists': { end: ['thumbnail pixels'], id: 'Word manda sobre o TS (item 4, REF error)', why: 'REF to the unterminated bm2 on page 10: the bold error text of Word' },
  // Item 3: the first box of f7d__7__10's chain of linked boxes keeps the chain's first lines (Word; the TS widow rule
  // empties it): the frames and screens of the drags over that page differ from the first drag on
  'f7d__7__10|objects': { from: 5, facets: '*', end: '*', id: 'Word manda sobre o TS (item 3, linked boxes)', why: 'Word keeps the chain\'s first lines in the first box' },
};
/** declared for every document (the facet itself is the declared gap) */
const GLOBAL_O = {};
/** '2,6-9' → [2, 6, 7, 8, 9] */
const stepsOf = (spec) => String(spec).split(',').flatMap((p) => { const [a, b] = p.split('-').map(Number); return b === undefined ? [a] : Array.from({ length: b - a + 1 }, (_, i) => a + i); });
/** an entry: `at` = { facet: '2,6-9' } (exact steps; every one must still differ), or `from` + `facets` (a range), `end` = end facets;
 *  `intermittent`: seen in some runs only (not checked for staleness; its cause is measured, not front O's) */
const entriesOf = (key) => [DECLARED_C[key] ?? []].flat();
const declared = (key, facet, step) => {
  if (GLOBAL_O[facet]) return GLOBAL_O[facet];
  // UO_EC_MEASURE=1: every difference counted as declared and listed (with UO_EC_COVERED), to write the declarations from
  if (process.env.UO_EC_MEASURE) return { id: 'measure' };
  for (const d of entriesOf(key)) {
    if (step === undefined) { if (d.end === '*' || (d.end ?? []).includes(facet)) return d; continue; }
    if (d.at && d.at[facet] !== undefined && stepsOf(d.at[facet]).includes(step)) return d;
    if (d.from !== undefined && step >= d.from && (d.facets === '*' || (d.facets ?? []).includes(facet))) return d;
  }
  return null;
};
/** the exact (`at`) declarations of `key` not met by a run: stale (the declaration says more than what differs) */
const staleAt = (key, met) => entriesOf(key).filter((d) => !d.intermittent).flatMap((d) => [
  ...Object.entries(d.at ?? {}).flatMap(([f, spec]) => stepsOf(spec).map((s) => `${f}@${s}`)),
  ...(Array.isArray(d.end) ? d.end : []),
]).filter((x) => !met.has(x));

/** wave 7c (V): the frames of the last `frames: true` drag (taken by runFamily into its step) */
let lastFrames = null;
/** wave 7c (V): the `frames: true` drags are compared frame by frame (on by default since the fix round 1; UO_EC_FRAMES=0 runs them as
 *  plain six-step drags) */
const FRAMES = process.env.UO_EC_FRAMES !== '0';
async function runStep(st, i, targets) {
  let answer = null;
  if (st.js) answer = await ev((c) => window.__es.js(c), st.js);
  else if (st.type) { await ev(() => window.__es.focus()); await env.page.keyboard.type(st.type); await ev(() => window.__es.settle(150)); }
  else if (st.key) { await ev(() => window.__es.focus()); await env.page.keyboard.press(st.key); await ev(() => window.__es.settle(150)); }
  else if (st.button) {
    answer = await ev((id) => window.__es.clickButton(id), st.button);
    if (st.ok) for (let k = 0; k < 10 && !(await ev(() => window.__es.dialogs())).length; k++) await ev(() => window.__es.settle(100));
    const dl = await ev(() => window.__es.dialogs());
    if (dl.length) answer += ` dialogs ${dl.length}`;
    if (st.ok) answer += ` ${await ev((e) => window.__es.dialogOk(e), st.edit ?? '')}`;
    await ev(() => window.__es.closeDialogs());
  } else if (st.dialogOk) {
    // a dialog opened by an earlier step (the spreadsheet dialog of a double click): its OK
    for (let k = 0; k < 20 && !(await ev(() => window.__es.dialogs())).length; k++) await ev(() => window.__es.settle(100));
    answer = await ev((e) => window.__es.dialogOk(e), st.edit ?? '');
    await ev(() => window.__es.settle(300));
    await ev(() => window.__es.closeDialogs());
  } else if (st.paste) answer = await ev((k) => window.__es.paste(k), st.paste);
  else if (st.menu) { answer = JSON.stringify(await ev(() => window.__es.menu())); await ev(() => window.__es.closeMenu()); }
  else if (st.mouse) {
    // the target of the TS run (its own objects), replayed at the same client point on the Rust run
    if (!(i in targets)) targets[i] = await ev((w) => window.__es.target(w), st.target);
    const t = targets[i];
    answer = t ? `${st.mouse} ${t.kind ?? ''}` : 'no target';
    if (t) {
      await ev((s) => window.__es.scrollTo(s), t.scrollTop);
      // the scrolled pages painted (and the Rust object list of the new view built) before the pointer comes
      await ev(() => window.__es.settle(100));
      if (process.env.UO_EC_COVERED) console.log(`  point @${i} ${JSON.stringify(t)} under ${await ev((p) => { const sr = window.__es.el.shadowRoot; const n = sr.elementFromPoint(p.x, p.y); return n ? `${n.tagName.toLowerCase()}.${[...n.classList].join('.')}` : 'none'; }, t)}`);
      const m = env.page.mouse;
      if (st.mouse === 'click') await m.click(t.x, t.y);
      else if (st.mouse === 'rclick') await m.click(t.x, t.y, { button: 'right' });
      else if (st.mouse === 'dblclick') await m.dblclick(t.x, t.y);
      else if (st.mouse === 'shift' || st.mouse === 'ctrl') { const k = st.mouse === 'shift' ? 'Shift' : 'Control'; await env.page.keyboard.down(k); await m.click(t.x, t.y); await env.page.keyboard.up(k); }
      else if (st.mouse === 'hoverdrag') { await m.move(t.x - 30, t.y - 30); await m.move(t.x, t.y, { steps: 4 }); await ev(() => window.__es.settle(100)); await m.down(); await m.move(t.x + st.dx, t.y + st.dy, { steps: 6 }); await m.up(); }
      else if (st.mouse === 'handledrag') { await m.move(t.x + 40, t.y + 40); await m.move(t.x, t.y, { steps: 4 }); await ev(() => window.__es.settle(100)); await m.down(); await m.move(t.x + st.dx, t.y + st.dy, { steps: 6 }); await m.up(); }
      else if (st.mouse === 'drag' && st.frames && FRAMES) {
        // wave 7c (V): the same six pointer moves as `{ steps: 6 }`, a frame captured after the down and after each move
        lastFrames = [];
        await m.move(t.x, t.y);
        await ev(() => window.__es.dragStart());
        await m.down();
        lastFrames.push(await ev(() => window.__es.dragFrame()));
        for (let k = 1; k <= 6; k++) { await m.move(t.x + (st.dx * k) / 6, t.y + (st.dy * k) / 6); lastFrames.push(await ev(() => window.__es.dragFrame())); }
        await m.up();
      }
      else if (st.mouse === 'drag') { await m.move(t.x, t.y); await m.down(); await m.move(t.x + st.dx, t.y + st.dy, { steps: 6 }); await m.up(); }
      await ev(() => window.__es.settle(200));
    }
  }
  return answer;
}

const allDiffs = {};
async function runFamily(fam) {
  // UO_EC_UPTO=<n>: only the first n steps (a bisection of an end difference; the declarations are not checked for staleness)
  const s = process.env.UO_EC_UPTO ? { ...SCRIPTS[fam], steps: SCRIPTS[fam].steps.slice(0, Number(process.env.UO_EC_UPTO)) } : SCRIPTS[fam];
  const tally = { same: 0, diff: 0, declared: 0 };
  const used = new Set();
  for (const want of s.docs) {
    const n = want === 'blank' ? 'blank' : docOf(want);
    const key = `${want.replace(/\.docx$/, '')}|${fam}`;
    const runs = {};
    const targets = {};
    for (const engine of ['ts', 'rust']) {
      await env.openPage();
      await env.page.evaluate(helpers);
      if (DUMP) await ev(() => { window.__dump = true; });
      if (process.env.UO_EC_TRACE) await ev((t) => { window.__trace = t.split(','); }, process.env.UO_EC_TRACE);
      const errs0 = env.errors.length;
      await ev((e) => window.__es.make(e), engine);
      const pages = await ev((x) => window.__es.load(x), n);
      const steps = [];
      for (let i = 0; i < s.steps.length; i++) {
        const answer = await runStep(s.steps[i], i, targets);
        steps.push({ answer, facets: JSON.parse(await ev(() => window.__es.facets())), frames: lastFrames });
        lastFrames = null;
      }
      if (DUMP && fam === 'previews') {
        mkdirSync(dumpDir, { recursive: true });
        for (const [k, u] of await ev((pf) => window.__es.dumpPreviews(pf), `${want.replace(/\.docx$/, '')}-${engine}`)) writeFileSync(join(dumpDir, `prev-${k.replace(/[^\w.-]+/g, '_')}.png`), Buffer.from(u.split(',')[1], 'base64'));
      }
      // UO_EC_ITEMS=1: the image/shape items of page 0 of the layout each engine paints (the thumbnail's geometry)
      if (process.env.UO_EC_ITEMS) console.log(`  items ${engine} ${await ev(() => { const el = window.__es.el; const p = el.rust ? el.rust.dl?.pages[0] : el.eng?.result.pages[0]; return JSON.stringify((p?.items ?? []).filter((x) => x.t === 'image').map((x) => { const env = el.rust ? el.rust.rs.paintEnv(el.rust.dl) : el.enginePaintEnv(); const bm = env.image(x.image); return { x: x.x, y: x.y, w: x.w, h: x.h, crop: x.crop, rot: x.rot, pic: x.pic ? Object.keys(x.pic) : null, src: bm ? `${bm.constructor.name} ${bm.width}x${bm.height}` : null }; })); })}`);
      const end = await ev(() => window.__es.end());
      runs[engine] = { pages, steps, end, errors: env.errors.length - errs0 };
      await ev(() => window.__es.remove());
    }
    const a = runs.ts, b = runs.rust;
    const diffs = [];
    /** the declared differences met (UO_EC_COVERED=1 prints them: the declarations narrowed to what differs) */
    const covered = [];
    const check = (facet, x, y, step) => {
      const same = JSON.stringify(x) === JSON.stringify(y);
      if (same) { tally.same++; return; }
      tally.diff++;
      const d = declared(key, facet, step);
      if (d) {
        tally.declared++;
        if (!GLOBAL_O[facet]) {
          used.add(key);
          covered.push(step === undefined ? facet : `${facet}@${step}`);
          if (process.env.UO_EC_COVERED === '2') console.log(`  declared ${facet}@${step}\n    ts   ${JSON.stringify(x)?.slice(0, 8000)}\n    rust ${JSON.stringify(y)?.slice(0, 8000)}`);
        }
        return;
      }
      diffs.push({ facet, step, ts: x, rust: y });
    };
    if (a.errors || b.errors) fail(`${key}: page errors ts ${a.errors}, rust ${b.errors}`);
    check('pages after load', a.pages, b.pages);
    a.steps.forEach((sa, i) => {
      const sb = b.steps[i];
      // wave 7c (V): a live drag compared frame by frame (caret, selection, events, page pixels)
      if (sa.frames || sb.frames) (sa.frames ?? sb.frames).forEach((_, k) => {
        const fa = sa.frames?.[k], fb = sb.frames?.[k];
        for (const f of ['sel', 'proxy', 'selChanges', 'events', 'pages']) check(`frame ${k} ${f}`, fa?.[f], fb?.[f], i);
        if (DUMP && JSON.stringify(fa?.pages) !== JSON.stringify(fb?.pages)) {
          mkdirSync(dumpDir, { recursive: true });
          for (const [e, fr] of [['ts', fa], ['rust', fb]]) (fr?.shots ?? []).forEach((u, p) => { if (u && fa?.pages?.[p] !== fb?.pages?.[p]) writeFileSync(join(dumpDir, `${key.replace(/\|/g, '-')}-s${i}-f${k}-p${p}-${e}.png`), Buffer.from(u.split(',')[1], 'base64')); });
        }
      });
      check('answer', sa.answer, sb.answer, i);
      for (const k of new Set([...Object.keys(sa.facets), ...Object.keys(sb.facets)])) if (!k.startsWith("__")) check(k, sa.facets[k], sb.facets[k], i);
      if (process.env.UO_EC_COVERED && sa.facets.__diag?.length) console.log(`  ts diag @${i} ${JSON.stringify(sa.facets.__diag)}`);
      if (process.env.UO_EC_COVERED && sb.facets.__diag?.length) console.log(`  rust diag @${i} ${JSON.stringify(sb.facets.__diag)}`);
    });
    check('end pages', [a.end.pages, a.end.sizes], [b.end.pages, b.end.sizes]);
    check('thumbnail pixels', a.end.thumbs, b.end.thumbs);
    check('screen pixels', a.end.screen, b.end.screen);
    if (DUMP && (JSON.stringify(a.end.screen) !== JSON.stringify(b.end.screen) || JSON.stringify(a.end.thumbs) !== JSON.stringify(b.end.thumbs))) {
      mkdirSync(dumpDir, { recursive: true });
      for (const [e, r] of [['ts', a.end], ['rust', b.end]]) writeFileSync(join(dumpDir, `${key.replace(/\|/g, '-')}-${e}.docx`), Buffer.from(r.docx, 'base64'));
      for (const [e, r] of [['ts', a.end], ['rust', b.end]]) r.thumbShots.forEach((u, i) => { if (u) writeFileSync(join(dumpDir, `${key.replace(/[|]/g, '-')}-thumb${i}-${e}.png`), Buffer.from(u.split(',')[1], 'base64')); });
      for (const [e, r] of [['ts', a.end], ['rust', b.end]]) r.shots.forEach((u, i) => { if (u) writeFileSync(join(dumpDir, `${key.replace(/\|/g, '-')}-p${i}-${e}.png`), Buffer.from(u.split(',')[1], 'base64')); });
    }
    const pa = parts(a.end.docx), pb = parts(b.end.docx);
    check('saved part names', Object.keys(pa).sort(), Object.keys(pb).sort());
    for (const p of Object.keys(pa)) if (p in pb) check(`saved ${p}`, pa[p], pb[p]);
    console.log(`${key}: ${s.steps.length} steps; ${diffs.length} undeclared difference(s)`);
    if (process.env.UO_EC_COVERED && covered.length) console.log(`  declared met: ${covered.join(" ")}`);
    const stale = staleAt(key, new Set(covered));
    if (stale.length) fail(`${key}: declared but equal now: ${stale.join(" ")}`);
    if (process.env.UO_EC_ANSWERS) for (let i = 0; i < a.steps.length; i++) console.log(`  @${i} ts ${String(a.steps[i].answer).slice(0, 400)}
     rust ${String(b.steps[i].answer).slice(0, 400)}`);
    for (const d of diffs.slice(0, 14)) {
      const cut = (v) => { const t = typeof v === 'string' ? v : JSON.stringify(v); return t && t.length > 600 ? `${t.slice(0, 600)}…` : t; };
      console.log(`  DIFF ${d.facet}${d.step !== undefined ? ` @${d.step} ${JSON.stringify(s.steps[d.step])}` : ''}\n    ts   ${cut(d.ts)}\n    rust ${cut(d.rust)}`);
    }
    if (diffs.length) fail(`${key}: ${diffs.length} undeclared difference(s)`);
    allDiffs[key] = diffs.map((d) => ({ facet: d.facet, step: d.step }));
  }
  console.log(`${fam}: facets ${tally.same} equal, ${tally.diff} different (${tally.declared} declared: ${[...used].map((k) => `${k} ${entriesOf(k).map((d) => d.id).join(' + ')}`).join(', ') || 'none'})`);
  return used;
}

/** memory (count build): create → load → lists → a Quick Table → the cell paste → a table edge edit → save → remove */
async function runMemory() {
  await env.openPage();
  await env.page.evaluate(helpers);
  const n = docOf('f7b__7__1.docx');
  const lives = [], bufs = [];
  for (let i = 0; i < SOAK; i++) {
    // wave 7 (A2, fix round): first an editor on a blank document for the pointer edits (a picture at 0:0, nothing over it),
    // removed and counted; then the editor of the document for the lists, tables and previews
    await ev(async (x) => {
      const es = window.__es;
      await es.make('rust');
      // wave 7 (F1): newDocument() right after a load (the load's refresh() installed a layout of the synchronous relayout,
      // which `adopt` did not release: one handle per cycle before the fix in rust-engine.ts)
      await es.load(x);
      await es.load('blank');
      const el = es.el;
      el.setSelection({ para: 0, offset: 0 });
      await el.insertImage(await es.image('png'), 'png');
      await es.settle(100);
      return null;
    }, n);
    // the pointer edits of the shell (MoveInlineRun, then CommitDrag of the picture made floating): real mouse drags
    // (the targets are engine-neutral here: the picture inserted at 0:0, found by the caret after it — the harness's
    // `inline`/`float` targets read the TS layout)
    const pic = async () => ev(() => { const el = window.__es.el; el.scrollTop = 0; const c = el.caretRectAt({ para: 0, offset: 1 }); return c ? { x: c.x - 40, y: c.y + c.h / 2 } : null; });
    const drag = async (t, dx, dy) => {
      if (!t) return false;
      const m = env.page.mouse;
      await m.move(t.x, t.y); await m.down(); await m.move(t.x + dx, t.y + dy, { steps: 6 }); await m.up();
      await ev(() => window.__es.settle(150));
      return true;
    };
    const sel = async () => ev(() => window.__es.el.selectedObjectCount());
    const t0 = await pic();
    if (t0) { await env.page.mouse.click(t0.x, t0.y); await ev(() => window.__es.settle(100)); }
    const picked = await sel();
    const moved = await drag(t0, 160, 30);
    await ev(() => window.__es.el.undo());
    await ev(() => window.__es.settle(100));
    // (the undo put the picture back at 0:0: the same point)
    const t1 = (await pic()) ?? t0;
    if (t1) { await env.page.mouse.click(t1.x, t1.y); await ev(() => window.__es.settle(100)); await ev(() => window.__es.clickButton('pic:wrap:square')); await ev(() => window.__es.settle(150)); }
    const floated = await sel();
    const dragged = await drag(t1, 40, 25);
    if (i === 0) console.log(`memory: cycle 0: inline picked ${picked}, inline drag ${moved}, square-wrapped selection ${floated}, float drag ${dragged}`);
    await ev(async () => {
      const es = window.__es;
      await es.settle(100);
      es.el.saveDocx();
      es.remove();
      await new Promise((r2) => setTimeout(r2, 0));
    });
    // wave 7c (S): the removed element keeps its document until it is collected (as the TS element its model): GC first
    const r0 = await env.gc();
    if (r0.sessions || r0.shellEngines || r0.handles) fail(`memory: cycle ${i}: after the pointer editor's remove ${JSON.stringify(r0)}`);
    await ev(async (x) => {
      const es = window.__es;
      await es.make('rust');
      await es.load(x);
      const el = es.el;
      el.setSelection({ para: 0, offset: 0 });
      el.applyListLibrary('number', 1);
      el.listInfo(); el.documentLists(); el.listStylesInDocument(); el.numberingValueInfo(); el.listLibrary('multilevel');
      el.createListStyle('Mem', []);
      await el.insertQuickTable('matrix');
      el.selectTable();
      await el.copy();
      el.setSelection({ para: 0, offset: 0 });
      el.pasteCells('overwrite');
      await el.tableStyleList(); await el.tableStyleXml('Grid Table 4 Accent 1'); el.tableStyleBases();
      // wave 7 (A2, fix round): the preview paints (a cover, a watermark, a table-style thumbnail: the session's preview layouts)
      const c0 = document.createElement('canvas');
      for (const t of el.coverPages().slice(0, 2)) el.paintCoverTemplate(t.id, c0, 120);
      const wm = el.watermarkGallery()[0];
      if (wm) el.paintWatermarkTemplate(wm.id, c0, 120);
      el.renderTableStyleThumb(c0, 'Grid Table 4 Accent 1', { firstRow: true, firstColumn: true });
      c0.width = c0.height = 0;
    }, n);
    const r = await ev(async () => {
      const es = window.__es;
      const el = es.el;
      await es.settle(100);
      el.saveDocx();
      const during = es.stats();
      es.remove();
      await new Promise((r2) => setTimeout(r2, 0));
      return { during, after: es.stats() };
    });
    // wave 7c (S): the removed element keeps its document until it is collected (as the TS element its model): GC first
    r.after = await env.gc();
    lives.push(r.after.live);
    bufs.push(r.after.buffer);
    if (r.after.sessions || r.after.shellEngines || r.after.handles) fail(`memory: cycle ${i}: after remove ${JSON.stringify(r.after)}`);
    if (i === 0 || i === SOAK - 1) console.log(`memory: cycle ${i}: during ${JSON.stringify(r.during)} after ${JSON.stringify(r.after)}`);
  }
  const warm = lives.slice(Math.min(5, lives.length - 1));
  const flat = warm.every((x) => x === warm[0]);
  const bufWarm = new Set(bufs.slice(Math.floor((bufs.length * 2) / 3)));
  console.log(`memory: ${SOAK} cycles; live bytes after warm-up ${flat ? `flat at ${warm[0]}` : `NOT flat: ${warm.join(', ')}`}; memory.buffer ${[...bufWarm].join(', ')}`);
  if (!flat && warm[0] !== null && warm[0] !== -1) fail('memory: live bytes not flat');
  if (bufWarm.size > 1) fail('memory: the wasm memory grew after warm-up');
}

const only = process.env.UO_EC_ONLY ? new RegExp(process.env.UO_EC_ONLY) : null;
if (only) for (const s of Object.values(SCRIPTS)) s.docs = s.docs.filter((d) => only.test(d));
const fams = MODE === 'all' ? Object.keys(SCRIPTS).filter((k) => k !== 'probe') : Object.keys(SCRIPTS).includes(MODE) ? [MODE] : [];
const t0 = Date.now();
const usedAll = new Set();
for (const f of fams) for (const k of await runFamily(f)) usedAll.add(k);
if (MODE === 'memory' || MODE === 'all') await runMemory();
const ranKeys = new Set(fams.flatMap((f) => SCRIPTS[f].docs.map((d) => `${d.replace(/\.docx$/, '')}|${f}`)));
const stale = Object.keys(DECLARED_C).filter((k) => ranKeys.has(k) && !usedAll.has(k) && !entriesOf(k).every((d) => d.intermittent));
if (stale.length) fail(`declared but equal now (remove from DECLARED_C): ${stale.join(', ')}`);
writeFileSync(join(outDir, `diffs-${MODE}.json`), JSON.stringify(allDiffs, null, 1));
await env.close();
console.log(`editor_switch_completion (${MODE}): ${((Date.now() - t0) / 1000).toFixed(0)} s; ${failures ? `${failures} FAILURE(S)` : 'ok'}`);
process.exit(failures ? 1 : 0);
