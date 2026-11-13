// Wave 5, front O of the editor switch (record core/docs/modules/uo-bindings.md "Wave 5 — O: objects and the page"):
// pictures, shapes and text boxes as objects, the page (setup, columns, breaks, line numbers, hyphenation, page color,
// watermark, cover page, page borders), the views (Read mode) and the context menu of `<universal-write engine="rust">`
// against the TS engine in a real headless Chromium.
//
//   UO_TS_ROOT=<frozen checkout> UO_WASM_PATH=<session .wasm> [UO_CHROMIUM=...] [UO_EO_SOAK=30] [UO_EO_ONLY=<regex of docs>]
//   [UO_EO_DUMP=1] node crates/uo-wasm/tests/browser/editor_switch_objects.mjs --mode page|objects|pictures|menu|views|keys|memory|all
//   Measuring (fix round 1): UO_EO_COVERED=1|2 lists the declared differences met (2: with both values), UO_EO_MEASURE=1
//   counts every difference as declared (to write `at` entries from), UO_EO_TRACE=<methods> wraps those (private) methods of
//   the TS element and prints their calls per step in the non-compared `__diag` facet, UO_EO_ANSWERS=1 the step answers.
//
// Per mode and document: the TS element first, then the Rust one (`author="Ana"`, `lang="pt"`, the ribbon), loaded with
// `loadDocx(bytes, { protected: false })` after `__seed(1)`; the SAME script on both: element API calls (`js`), real keys and
// mouse events through Playwright (`key`, `mouse` — the pointer target is computed ONCE on the TS element from its own
// `enumFloats`/`enumInlinePics`/`handleCenters` and replayed at the same client point on the Rust one), bar buttons by id
// (`button`, with the dialog's OK when asked), pastes (`paste`: a synthetic ClipboardEvent with a DataTransfer on the hidden
// textarea). After each step the objects/page facets (canonical JSON: keys sorted, null/undefined/false dropped — the wire
// drops false booleans), the selection and the uw-* events; at the end the page count and sizes, the page thumbnails (×1),
// the screen canvases with the caret hidden and the TS overlays of other fronts off, and the saved .docx part by part. Every
// difference not covered by DECLARED_O fails, and so does any page error.
// Declared duplicate: the page helpers are copy-adapted from editor_switch_review.mjs / editor_switch.mjs (gate item: move
// them into the lib).
import { createRequire } from 'node:module';
import { mkdirSync, writeFileSync } from 'node:fs';
import { join } from 'node:path';
import { setup, core } from './editor_switch_lib.mjs';

const root = process.env.UO_TS_ROOT;
if (!root) throw new Error('set UO_TS_ROOT to the frozen TS checkout');
const arg = (name, dflt) => { const i = process.argv.indexOf(name); return i >= 0 ? process.argv[i + 1] : dflt; };
const MODE = arg('--mode', 'all');
const SOAK = Number(process.env.UO_EO_SOAK ?? 30);
const DUMP = !!process.env.UO_EO_DUMP;
const target = process.env.CARGO_TARGET_DIR ?? join(core, 'target');
const wasmPath = process.env.UO_WASM_PATH ?? join(target, 'es-o/uo_wasm_session.wasm');
const outDir = join(target, 'es-o/harness');
const dumpDir = join(target, 'es-o/dump');
mkdirSync(outDir, { recursive: true });
const { unzipSync } = createRequire(join(root, 'write/package.json'))('fflate');

// UO_EO_PROBE=<docx>: `--mode probe` loads that document in both engines, no steps (a saved file of a failing run)
const env = await setup({ root, wasmPath, outDir, stride: 1, extraDocs: process.env.UO_EO_PROBE ? { 'probe.docx': process.env.UO_EO_PROBE } : {}, only: 'f7a__7__(0|1|5|10)\\.|f7b__7__(0|1|3|8|10)\\.|f7c__7__(1|2|8)\\.|f7d__7__(0|3|10)\\.|f8__7__(5|11)\\.|f2-3-0\\.' });
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
  // wave 7 (B): `paintTableAdorners` is painted by the Rust shell since wave 7 (A2's table handles): compared, as editor_switch_completion.mjs
  const OTHERS = ['paintBookmarkMarks', 'paintTocControl', 'paintEngineInlines'];
  const log = [];
  const canon = (v) => (Array.isArray(v) ? v.map(canon) : v instanceof Uint8Array ? `#bytes:${v.length}:${fnv(v)}` : v && typeof v === 'object'
    ? Object.fromEntries(Object.entries(v).filter(([, x]) => x !== undefined && x !== null && x !== false).sort(([a], [b]) => (a < b ? -1 : 1)).map(([k, x]) => [k, canon(x)]))
    : typeof v === 'number' ? +v.toFixed(3) : v);
  const J = (v) => JSON.stringify(canon(v));
  const imgCache = {};
  const api = {
    el: null,
    async make(engine) {
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
      // UO_EO_TRACE=<names>: the TS element's (private) methods traced in the diagnostic facet (ED-218 measurements)
      if (engine !== 'rust' && window.__trace) for (const m of window.__trace) { const f0 = el[m]; if (typeof f0 === 'function') el[m] = function (...a) { const r = f0.apply(this, a); (api.traced ??= []).push(`${m}(${a.map((x) => (x instanceof Event ? x.type : typeof x === 'object' ? '{}' : String(x))).join(',')})=${r && typeof r === 'object' ? '{}' : String(r)}`); return r; }; }
      api.el = el;
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
      // wave 7 (B, LOAD-1): the TS settles its fonts as before an export (`settleDocumentFonts`: the families and faces the
      // document and the engine asked for resolved, the layout redone with them) — the counterpart of `rust.idle()`; the
      // fixed 300 ms after `document.fonts.ready` alone left the TS at 12 pages of f7a__7__5 (14 once settled) under load
      if (!api.el.rust && typeof api.el.settleDocumentFonts === 'function') await api.el.settleDocumentFonts();
      api.el.refresh();
      await api.settle();
      // the TS draws its random ids (wp:docPr, w14:anchorId, w:id of a content control) from Math.random, the session from its
      // own xorshift32 seeded 1 at open: the same generator as this page's Math.random. Seeded again once the document is
      // open, the TS draws what the session draws as long as both take the same number of values per command (ED-217)
      window.__seed(1);
      log.length = 0;
      // UO_EO_LOADTRACE=1 (wave 7, B, LOAD-1): the page count at the settle point and after 1.5 s more and a refresh (an
      // engine whose fonts arrive late changes it), with the fonts the page's FontFaceSet and the TS resolver hold
      if (window.__loadTrace) {
        const el = api.el;
        const at0 = el.layoutPageCount();
        const fontsNow = () => [...document.fonts].filter((f) => f.status === 'loaded').map((f) => `${f.family}/${f.weight}/${f.style}`).sort().join(',');
        const f0 = fontsNow();
        await sleep(1500);
        el.refresh();
        await api.settle();
        window.__loadTraceOut = `LOADTRACE ${el.rust ? 'rust' : 'ts'} pages ${at0} -> ${el.layoutPageCount()}; fonts at settle [${f0}]; after [${fontsNow()}]`;
      }
      return api.el.layoutPageCount();
    },
    remove() { api.el?.remove(); api.el = null; },
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
        // UO_EO_FDIAG: the canvases' size and place (printed with the frames, not compared)
        ...(window.__fdiag ? { diag: cs.map((cv) => (cv.width > 1 ? `${cv.width}x${cv.height} ${cv.style.width} ${cv.getBoundingClientRect().left},${cv.getBoundingClientRect().top}` : '-')).join(' | ') + ` tb ${JSON.stringify(el.editingTextbox?.())} cnt ${el.selectedObjectCount?.()} st ${el.scrollTop}` } : {}),
        // UO_EO_DUMP: the frame's page canvases as PNG (written for the frames that differ)
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
      if (k === 'inline') {
        const h = el.enumInlinePics()[Number(a)];
        return h ? { ...at(h.page, h.rect.x + h.rect.w / 2, h.rect.y + h.rect.h / 2), kind: 'inline' } : null;
      }
      if (k === 'text') {
        const c = el.caretRectAt({ para: Number(a), offset: Number(b) });
        return c ? { x: c.x + 0.5, y: c.y + c.h / 2, scrollTop: el.scrollTop } : null;
      }
      return null;
    },
    scrollTo(top) { api.el.scrollTop = top; },
    button(id) {
      const find = (n) => { if (n.dataset?.a === id) return n; for (const c of n.children ?? []) { const f = find(c); if (f) return f; } if (n.shadowRoot) return find(n.shadowRoot); return null; };
      return find(api.el.shadowRoot);
    },
    async clickButton(id) {
      const bt = api.button(id);
      if (!bt) return 'missing';
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
      const ok = [...d.querySelectorAll('button')].find((b) => b.dataset?.id === 'ok' || /^(OK|Ok)$/.test(b.textContent?.trim() ?? ''));
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
    /** the objects/page facets after a step */
    async facets() {
      const el = api.el;
      const f = { sel: el.getSelection(), events: api.drain(), dirty: el.isDirty(), pages: el.layoutPageCount() };
      el.selectedImage?.();
      await sleep(30);
      const img = el.selectedImage();
      f.image = img;
      // ED-213: the wire ObjectState has no shape fill nor text body: their own facet (always declared)
      const shp = el.selectedShape();
      if (shp?.format) { const { fill, body, story, textbox, ...rest } = shp.format; f.shape = { ...shp, format: rest }; f.shapeFillBody = { fill, body, story, textbox }; } else { f.shape = shp; f.shapeFillBody = null; }
      f.count = el.selectedObjectCount();
      f.pane = el.selectionPaneItems();
      f.section = el.getSection();
      f.view = el.getView();
      f.zoom = el.getZoom();
      f.bg = el.pageBackground();
      const wm = el.watermark();
      f.watermark = wm;
      f.cover = el.hasCoverPage();
      f.lines = el.getLineNumbering();
      f.suppressed = el.lineNumbersSuppressed();
      f.hyph = el.getHyphenation();
      f.borders = el.getPageBorders();
      f.modes = { crop: el.croppingImage(), draw: el.drawingTextbox(), link: el.linkingTextbox() };
      if (window.__fdiag) { const ta = el.shadowRoot.querySelector('textarea'); f.__proxy = ta ? [ta.style.left, ta.style.top, ta.style.height].join(',') : null; }
      // the TS run only, not compared: the float selection's identity (ED-218: is `floatSel` still a run of the model's floats?)
      if (!el.rust) { const fs0 = el.floatSel; f.__diag = { floatSel: !!fs0, selectedFloat: !!el.selectedFloat(), inFloats: !!fs0 && el.enumFloats().some((h) => h.run === fs0), tbEdit: !!el.tbEdit, raw: !!fs0 && !!el.constructor.rawOf?.(fs0), format: !!fs0 && !!el.formatOf?.(fs0), objW: fs0 ? (fs0.shape ?? fs0.textbox ?? fs0.group ?? fs0.image)?.widthTwips : undefined, trace: api.traced?.splice(0) }; }
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
const SCRIPTS = {
  page: {
    docs: ['f7a__7__0.docx', 'f7a__7__1.docx', 'f7a__7__5.docx', 'f7a__7__10.docx', 'f7c__7__1.docx', 'f7c__7__2.docx', 'f7c__7__8.docx', 'blank'],
    steps: [
      { js: 'return null;' },
      { js: `el.setSelection(${P(0, 0)}); el.setPageSetup({ orientation: 'landscape' }); return null;` },
      { js: `el.setPageSetup({ margins: 'narrow' }); return null;` },
      { js: `el.setPageSetup({ size: 'A4' }); return null;` },
      { js: `el.setPageSetup({ orientation: 'portrait', customMargins: { top: 1000, right: 1100, bottom: 1200, left: 1300 } }); return null;` },
      { js: `el.setColumns('two'); return null;` },
      { js: `el.setColumns({ count: 3, sep: true, spaceTwips: 500 }); return null;` },
      { js: `el.setSelection(${P(0, 1)}); el.focusEditor(); return null;` },
      { key: 'Control+Enter' },
      { key: 'Control+Shift+Enter' },
      { js: `el.insertSectionBreak('continuous'); return null;` },
      { js: `el.insertSectionBreak('evenPage'); return null;` },
      { button: 'sectionbreak:oddPage' },
      { js: `el.setLineNumbering('continuous'); return el.getLineNumbering();` },
      { button: 'ln:suppress' },
      { js: `el.setLineNumberingOptions({ on: true, startAt: 3, countBy: 2, distanceTwips: 360, restart: 'section' }, 'document'); return null;` },
      { button: 'hyph:auto' },
      { js: `el.setHyphenationOptions({ caps: false, zoneTwips: 500, limit: 2 }); return el.getHyphenation();` },
      { js: `el.setPageColor({ themeColor: 'accent1', tint: 0.4 }); return el.pageBackground();` },
      { js: `el.setPageColor('#FFEECC'); return null;` },
      { js: `el.setPageFill({ type: 'gradient', colors: 'two', color: 'FF0000', color2: '0000FF', style: 'horizontal', variant: 1 }); return null;` },
      { js: `el.setWatermark('confidential-1'); return el.watermark();` },
      { js: `el.setWatermark({ kind: 'text', text: 'RASCUNHO R', font: 'Liberation Sans', color: 'FF0000', semitransparent: true, layout: 'horizontal' }); return el.watermark();` },
      { button: 'watermark:remove' },
      { js: `el.setSelection(${P(0, 0)}); el.insertCoverPage(el.coverPages()[0].id); return el.hasCoverPage();` },
      { button: 'cover:remove' },
      { button: 'blankPage' },
      { js: `el.setVerticalAlignment('center'); return el.getSection().verticalAlignment;` },
      { js: `el.setSectionStart('continuous'); return el.getSection().type;` },
      { js: `el.setPageBorders({ offsetFrom: 'page', top: { val: 'single', sz: 4, space: 24, color: 'auto' }, left: { val: 'single', sz: 4, space: 24, color: 'auto' }, bottom: { val: 'single', sz: 4, space: 24, color: 'auto' }, right: { val: 'single', sz: 4, space: 24, color: 'auto' } }); return el.getPageBorders();` },
      { button: 'columns:one' },
      { key: 'Control+z' },
      { key: 'Control+z' },
    ],
  },
  objects: {
    docs: ['f7b__7__0.docx', 'f7b__7__1.docx', 'f7b__7__3.docx', 'f7b__7__8.docx', 'f7b__7__10.docx', 'f7d__7__0.docx', 'f7d__7__3.docx', 'f7d__7__10.docx', 'f8__7__5.docx', 'f8__7__11.docx'],
    steps: [
      { js: 'return null;' },
      { target: 'float:0', mouse: 'click' },
      { target: 'float:0', mouse: 'drag', dx: 40, dy: 30, frames: true },
      { target: 'float:0', mouse: 'click' },
      { target: 'handle:0:se', mouse: 'drag', dx: 30, dy: 20, frames: true },
      { target: 'float:0', mouse: 'click' },
      { target: 'handle:0:nw', mouse: 'drag', dx: -20, dy: -10, frames: true },
      { target: 'float:0', mouse: 'click' },
      { target: 'handle:0:rot', mouse: 'drag', dx: 40, dy: 25, frames: true },
      { target: 'float:0', mouse: 'click' },
      { key: 'ArrowRight' },
      { key: 'Control+ArrowDown' },
      { button: 'shp:fill:none' },
      { button: 'shp:outline:none' },
      { button: 'shp:front' },
      { button: 'shp:back' },
      { button: 'shp:rotate:right' },
      { button: 'shp:flip:h' },
      { button: 'shp:wrap:square' },
      { button: 'shp:pos:top-left' },
      { button: 'shp:align:center' },
      { js: `const k = el.selectionPaneItems()[0]?.key; if (k) { el.renamePaneItem(k, 'Nome R'); el.setPaneItemHidden(k, true); el.setPaneItemHidden(k, false); } return el.selectionPaneItems();` },
      { target: 'float:0', mouse: 'rclick' },
      { key: 'Escape' },
      { target: 'float:0', mouse: 'click' },
      { key: 'Delete' },
      { target: 'border:0', mouse: 'click' },
      { button: 'shp:anchor:ctr' },
      { button: 'shp:dir:vert' },
      { key: 'Escape' },
      { js: `el.setSelection(${P(0, 0)}); return el.insertTextbox('simple');` },
      { key: 'Escape' },
      { js: 'el.drawTextbox(false); return el.drawingTextbox();' },
      { target: 'text:0:0', mouse: 'drag', dx: 120, dy: 80 },
      { key: 'Control+z' },
    ],
  },
  pictures: {
    docs: ['blank', 'f2-3-0.docx'],
    steps: [
      { js: `el.setSelection(${P(0, 0)}); await el.insertImage(await es.image('png'), 'png'); return null;` },
      { js: `el.setSelection(${P(0, 1)}); await el.insertImage(await es.image('jpeg'), 'jpeg'); return null;` },
      { target: 'inline:0', mouse: 'click' },
      { button: 'pic:wrap:square' },
      { js: 'el.setImagePosition(\'top-right\'); return null;' },
      { js: 'el.setImageWrap(\'tight\'); await new Promise((r) => setTimeout(r, 300)); return null;' },
      { button: 'pic:front' },
      { button: 'pic:rotate:right' },
      { button: 'pic:flip:v' },
      { js: 'el.setImageSize({ widthPt: 120, lockAspect: true }); return null;' },
      { button: 'pic:crop:fill' },
      { js: 'el.cropImageToAspect(4, 3); return null;' },
      { js: 'el.cropImageToShape(\'ellipse\'); return null;' },
      { js: 'el.setImageBorder({ color: { kind: \'srgbClr\', val: \'FF0000\' }, weightPt: 2, dash: \'dash\' }); return null;' },
      { js: 'el.setImageEffect(\'shadow\', \'outer:bottomRight\'); return null;' },
      { js: 'el.setImageFormat({ glow: { rad: 63500, color: { kind: \'schemeClr\', val: \'accent1\', mods: [[\'alpha\', 40000]] } }, softEdge: null }); return null;' },
      { js: 'await el.setImageAdjust({ bright: 0.2, contrast: -0.1 }); await new Promise((r) => setTimeout(r, 300)); return null;' },
      { js: 'el.setImageTransparency(30); return null;' },
      { js: 'el.setImageRecolor(\'grayscale\'); return null;' },
      { js: 'el.setImageAltText({ text: \'Imagem R\', decorative: false }); return null;' },
      { button: 'pic:reset' },
      { button: 'pic:resetSize' },
      { js: 'const n = await el.compressImages({ ppi: 96, deleteCropped: true }); return n;' },
      { js: 'el.startImageCrop(); return el.croppingImage();' },
      { key: 'Escape' },
      { key: 'Delete' },
      { js: `el.setSelection(${P(0, 0)}); return null;` },
      { paste: 'image' },
      { paste: 'html' },
      { key: 'Control+z' },
    ],
  },
  menu: {
    docs: ['f7b__7__0.docx', 'f7d__7__3.docx'],
    steps: [
      { js: 'return null;' },
      { target: 'text:0:2', mouse: 'rclick' },
      { menu: true },
      { target: 'float:0', mouse: 'rclick' },
      { menu: true },
      { key: 'Escape' },
      { target: 'text:1:0', mouse: 'rclick' },
      { menu: true },
    ],
  },
  // the break keys in the other stories (the TS order: a cell's Ctrl+Shift+Enter splits the table, front S; no break in a note)
  keys: {
    docs: ['blank'],
    steps: [
      { js: 'el.insertTable(2, 2); return el.getSelection();' },
      { key: 'Control+Shift+Enter' },
      { js: 'return [el.getSelection(), !!el.getTableState?.()];' },
      { key: 'Control+Enter' },
      { js: 'el.insertTable(1, 1); return null;' },
      { key: 'Control+Enter' },
      { js: `el.setSelection(${P(0, 0)}); el.insertFootnote(); return el.getSelection();` },
      { key: 'Control+Enter' },
      { key: 'Control+Shift+Enter' },
      { type: 'x' },
      { key: 'Escape' },
      { key: 'Control+Shift+Enter' },
    ],
  },
  probe: {
    docs: ['probe.docx'],
    // UO_EO_PROBE_STEPS: the probe's steps as JSON (a diagnosis; the default is the bare load)
    steps: process.env.UO_EO_PROBE_STEPS ? JSON.parse(process.env.UO_EO_PROBE_STEPS) : [{ js: 'return null;' }],
  },
  views: {
    docs: ['f7a__7__1.docx', 'f2-3-0.docx'],
    steps: [
      { js: 'return null;' },
      { button: 'view:read' },
      { js: 'return [...el.shadowRoot.querySelectorAll(\'canvas.page\')].map((c) => c.style.display || \'shown\');' },
      { key: 'ArrowRight' },
      { js: 'return [...el.shadowRoot.querySelectorAll(\'canvas.page\')].map((c) => c.style.display || \'shown\');' },
      // Space reaches the TS Read-mode listener once (the shell forwards it); PageDown is stopped by the shell, nudged by O
      { key: 'Space' },
      { js: 'return [el.readFirstPage, [...el.shadowRoot.querySelectorAll(\'canvas.page\')].map((c) => c.style.display || \'shown\')];' },
      { key: 'PageDown' },
      { js: 'return el.readFirstPage;' },
      { type: 'abc' },
      { key: 'PageUp' },
      { button: 'view:print' },
      { js: 'return [...el.shadowRoot.querySelectorAll(\'canvas.page\')].map((c) => c.style.display || \'shown\');' },
      { js: 'return [\'view:web\', \'view:draft\', \'view:outline\'].map((id) => es.button(id)?.getAttribute(\'aria-disabled\') ?? (es.button(id)?.disabled ? \'disabled\' : \'?\'));' },
    ],
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
const DECLARED_O = {
  // ── page ──
  // wave 7 (B): gate-O1 (the cover page's text box not painted) and gate-O2 (f7a__7__1: 11 pages against 10 after three
  // columns) are equal in the wave-7 runs (f7a__7__0/1/5/10, f7c__7__1, blank): the cover's fill is a theme colour and these
  // packages have no theme part, so the Rust layout resolved it over an empty theme (no fill) until gate-O3's Word theme
  // (uo-docx-layout theme_word.rs); gate-O2 equal with the same build (record "Wave 7 — B", Causes closed)
  // (f7c__7__2/8 page, f2-3-0 pictures/views and blank|keys: the field shading of the headers, footers and notes — ED-304
  // closed in wave 7c, V: the shell's overlays read every story's lines, uo_shell_story_lines)
  // (LOAD-1, f7a__7__5 at 12 pages in the TS against 14 once in five runs: closed in the review fix round — the TS run was not
  // settled; both engines have 14 pages at the settle point and 1.5 s later in 10 of 10 traced runs, and `load` now waits
  // for the TS's `settleDocumentFonts`)
  // ── objects (steps: 1 click, 2 move, 4 se, 6 nw, 8 rotate, 10 ArrowRight, 11 Ctrl+ArrowDown, 12… the Shape Format commands);
  //    exact steps (`at`), every one measured with UO_EO_MEASURE=1 UO_EO_COVERED=2 and the TS traced (UO_EO_TRACE) ──
  // wave 7 (B): ED-202 (the drag's effectExtent) and ED-213 (the group's blip/srcRect, the shape fill/body facet) are equal
  // since A2's wave-7 objects work: the f7b__7__0/3 and f7d__7__10 entries and GLOBAL_O.shapeFillBody removed (stale in the run)
  // wave 6 (C): gate-O4 (Ctrl+ArrowUp/Down) and ED-216 (S ED-102, f7d__7__3) closed; what is left of three of their documents:
  // wave 7 (B): gate-C1 (f7b__7__8/10, f8__7__5) closed — not a z-order: the inserted text box's fill is the scheme colour
  // lt1 and these packages have no theme part; the Rust layout resolved it over an empty theme and painted FFC000 under
  // the text, Word (PDF of the TS-saved bytes) and the TS paint FFFFFF, the Rust since gate-O3's Word theme too.
  // ED-219 (f8__7__11) equal since A2 routes the table edge drag.
  // wave 7 (B): with the TS paintTableAdorners compared (A2's request), the end screen of page 0 differs in the hovered
  // table's move handle (⊞, top-left) and size handle (□, bottom-right): the TS paints them, the shell does not (two runs out
  // of two; dumped crops). The shell's `tableMouseMove` now has the TS gate (no text-box exclusion) — not the cause; the
  // hover state at the end differs (front O/A2's hover chain): cross-front, owner A2
  // wave 7 (B, re-verification): f7b__7__8|objects retired — equal with A2's later tree ("declared but equal now")
  // (f7b__7__0|menu, the same ED-219b, retired too: equal in the three latest runs with A2's later tree)
  // ── keys ──
  // ── pictures, views ──
  // wave 7 (B): gate-O3 closed — the Strong style carries w:color C00000 + themeColor accent2 and the package has no theme
  // part: Word paints E97132 (the Office 2023 accent2, measured in Word's PDF), the Rust layout too since wave 7
  // (uo-docx-layout theme_word.rs), and the TS editor as well (the end screens are equal there: the only differing pixels,
  // x 338–404 y 1046–1065, were the footer's field shading: ED-304 closed in wave 7c). ED-209 closed: the views are routed
  // and answer@13 is equal
  // ── wave 7c (W): Word's layout rules over the frozen TS (record uo-bindings.md "Wave 7c — W"; each document attributed
  //    natively, uo-docx-layout word_w7c.rs attribute_documents_to_the_word_rules, with the harness's columns applied to the
  //    package for the page family: core/target/m2-wsession/w7c-w/cols3.py) ──
  // item 6: in a section of several columns (mode 15) a keepLines paragraph taller than the column goes to the next PAGE and
  // each of its columns ends with a page break (Word, oracle keeptall kt-kl70-cols…): more pages from the setColumns steps on
  'f7a__7__0|page': { from: 5, facets: '*', end: '*', id: 'Word manda sobre o TS (item 6, keepLines)', why: 'keepLines paragraphs taller than a column go to the next page in Word (mode 15): 12/13 pages after two/three columns, the TS 11' },
  'f7c__7__2|page': { from: 6, facets: '*', end: '*', id: 'Word manda sobre o TS (items 6 and 4)', why: 'keepLines in three columns (Word: the next page) and the REF to the unterminated bm2 (Word\'s bold error text)' },
  'f7c__7__8|page': { from: 6, facets: '*', end: '*', id: 'Word manda sobre o TS (item 6, keepLines)', why: 'keepLines paragraphs taller than a column go to the next page in Word (mode 15): 11 pages after three columns, the TS 9' },
  // item 3: the first box of a chain of linked boxes keeps the chain's first lines (Word; the TS widow rule empties it): the
  // scripted clicks on the boxes land on other text and the end screens differ
  'f7d__7__0|objects': { end: ['thumbnail pixels', 'screen pixels'], id: 'Word manda sobre o TS (item 3, linked boxes)', why: 'Word keeps the chain\'s first lines in the first box' },
  'f7d__7__3|objects': { from: 1, facets: '*', end: '*', id: 'Word manda sobre o TS (item 3, linked boxes)', why: 'Word keeps the chain\'s first lines in the first box: the scripted clicks land on other text' },
  'f7d__7__10|objects': { from: 1, facets: '*', end: '*', id: 'Word manda sobre o TS (item 3, linked boxes)', why: 'Word keeps the chain\'s first lines in the first box: the scripted clicks land on other text' },
  'f7d__7__3|menu': { end: ['thumbnail pixels', 'screen pixels'], id: 'Word manda sobre o TS (item 3, linked boxes)', why: 'Word keeps the chain\'s first lines in the first box' },
};
/** declared for every document (the facet itself is the declared gap) */
const GLOBAL_O = {};
/** '2,6-9' → [2, 6, 7, 8, 9] */
const stepsOf = (spec) => String(spec).split(',').flatMap((p) => { const [a, b] = p.split('-').map(Number); return b === undefined ? [a] : Array.from({ length: b - a + 1 }, (_, i) => a + i); });
/** an entry: `at` = { facet: '2,6-9' } (exact steps; every one must still differ), or `from` + `facets` (a range), `end` = end facets;
 *  `intermittent`: seen in some runs only (not checked for staleness; its cause is measured, not front O's) */
const entriesOf = (key) => [DECLARED_O[key] ?? []].flat();
const declared = (key, facet, step) => {
  if (GLOBAL_O[facet]) return GLOBAL_O[facet];
  // UO_EO_MEASURE=1: every difference counted as declared and listed (with UO_EO_COVERED), to write the declarations from
  if (process.env.UO_EO_MEASURE) return { id: 'measure' };
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
/** wave 7c (V): the `frames: true` drags are compared frame by frame (on by default since the fix round 1; UO_EO_FRAMES=0 runs them as
 *  plain six-step drags) */
const FRAMES = process.env.UO_EO_FRAMES !== '0';
async function runStep(st, i, targets) {
  let answer = null;
  if (st.js) answer = await ev((c) => window.__es.js(c), st.js);
  else if (st.type) { await ev(() => window.__es.focus()); await env.page.keyboard.type(st.type); await ev(() => window.__es.settle(150)); }
  else if (st.key) { await ev(() => window.__es.focus()); await env.page.keyboard.press(st.key); await ev(() => window.__es.settle(150)); }
  else if (st.button) {
    answer = await ev((id) => window.__es.clickButton(id), st.button);
    const dl = await ev(() => window.__es.dialogs());
    if (dl.length) answer += ` dialogs ${dl.length}`;
    if (st.ok) answer += ` ${await ev((e) => window.__es.dialogOk(e), st.edit ?? '')}`;
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
      if (process.env.UO_EO_COVERED) console.log(`  point @${i} ${JSON.stringify(t)} under ${await ev((p) => { const sr = window.__es.el.shadowRoot; const n = sr.elementFromPoint(p.x, p.y); return n ? `${n.tagName.toLowerCase()}.${[...n.classList].join('.')}` : 'none'; }, t)}`);
      const m = env.page.mouse;
      if (st.mouse === 'click') await m.click(t.x, t.y);
      else if (st.mouse === 'rclick') await m.click(t.x, t.y, { button: 'right' });
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
  const s = SCRIPTS[fam];
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
      if (process.env.UO_EO_FDIAG) await ev(() => { window.__fdiag = true; });
      if (process.env.UO_EO_TRACE) await ev((t) => { window.__trace = t.split(','); }, process.env.UO_EO_TRACE);
      if (process.env.UO_EO_LOADTRACE) await ev(() => { window.__loadTrace = true; });
      const errs0 = env.errors.length;
      await ev((e) => window.__es.make(e), engine);
      const pages = await ev((x) => window.__es.load(x), n);
      if (process.env.UO_EO_LOADTRACE) console.log(`  ${key} ${await ev(() => window.__loadTraceOut)}`);
      const steps = [];
      for (let i = 0; i < s.steps.length; i++) {
        const answer = await runStep(s.steps[i], i, targets);
        steps.push({ answer, facets: JSON.parse(await ev(() => window.__es.facets())), frames: lastFrames });
        lastFrames = null;
      }
      const end = await ev(() => window.__es.end());
      runs[engine] = { pages, steps, end, errors: env.errors.length - errs0 };
      await ev(() => window.__es.remove());
    }
    const a = runs.ts, b = runs.rust;
    const diffs = [];
    /** the declared differences met (UO_EO_COVERED=1 prints them: the declarations narrowed to what differs) */
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
          if (process.env.UO_EO_COVERED === '2') console.log(`  declared ${facet}@${step}\n    ts   ${JSON.stringify(x)?.slice(0, 8000)}\n    rust ${JSON.stringify(y)?.slice(0, 8000)}`);
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
        if (process.env.UO_EO_FDIAG && (fa?.diag || fb?.diag)) console.log(`  frame ${k} @${i} ts ${fa?.diag} ${JSON.stringify(fa?.proxy)} ${JSON.stringify(fa?.sel)}; rust ${fb?.diag} ${JSON.stringify(fb?.proxy)} ${JSON.stringify(fb?.sel)}`);
        if (DUMP && JSON.stringify(fa?.pages) !== JSON.stringify(fb?.pages)) {
          mkdirSync(dumpDir, { recursive: true });
          for (const [e, fr] of [['ts', fa], ['rust', fb]]) (fr?.shots ?? []).forEach((u, p) => { if (u && fa?.pages?.[p] !== fb?.pages?.[p]) writeFileSync(join(dumpDir, `${key.replace(/\|/g, '-')}-s${i}-f${k}-p${p}-${e}.png`), Buffer.from(u.split(',')[1], 'base64')); });
        }
      });
      check('answer', sa.answer, sb.answer, i);
      for (const k of new Set([...Object.keys(sa.facets), ...Object.keys(sb.facets)])) if (!k.startsWith("__")) check(k, sa.facets[k], sb.facets[k], i);
      if (process.env.UO_EO_FDIAG) console.log(`  proxy @${i} ts ${sa.facets.__proxy} rust ${sb.facets.__proxy}`);
      if (process.env.UO_EO_COVERED && sa.facets.__diag) console.log(`  ts diag @${i} ${JSON.stringify(sa.facets.__diag)}`);
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
    if (process.env.UO_EO_COVERED && covered.length) console.log(`  declared met: ${covered.join(" ")}`);
    const stale = staleAt(key, new Set(covered));
    if (stale.length) fail(`${key}: declared but equal now: ${stale.join(" ")}`);
    if (process.env.UO_EO_ANSWERS) for (let i = 0; i < a.steps.length; i++) console.log(`  @${i} ts ${String(a.steps[i].answer).slice(0, 400)}
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

/** memory (count build): create → load → insert a picture → select → drag → adjust → compress → save → remove */
async function runMemory() {
  await env.openPage();
  await env.page.evaluate(helpers);
  const n = docOf('f7b__7__1.docx');
  const lives = [], bufs = [];
  for (let i = 0; i < SOAK; i++) {
    const r = await ev(async (x) => {
      const es = window.__es;
      await es.make('rust');
      await es.load(x);
      const el = es.el;
      el.setSelection({ para: 0, offset: 0 });
      await el.insertImage(await es.image('png'), 'png');
      el.setSelection({ para: 0, offset: 0 }, { para: 0, offset: 1 });
      el.setImageWrap('square');
      el.setImageSize({ widthPt: 100 });
      await el.setImageAdjust({ bright: 0.1 });
      await el.compressImages({ ppi: 96, deleteCropped: true });
      el.selectedImage(); el.getSection(); el.selectionPaneItems();
      el.saveDocx();
      const during = es.stats();
      es.remove();
      await new Promise((r2) => setTimeout(r2, 0));
      return { during, after: es.stats() };
    }, n);
    // wave 7c (S): the removed element keeps its document until it is collected (as the TS element its model): GC first
    r.after = await env.gc();
    lives.push(r.after.live);
    bufs.push(r.after.buffer);
    if (r.after.sessions || r.after.shellEngines || r.after.handles) fail(`memory: cycle ${i}: after remove ${JSON.stringify(r.after)}`);
    if (i === 0 || i === SOAK - 1) console.log(`memory: cycle ${i}: during ${JSON.stringify(r.during)} after ${JSON.stringify(r.after)}`);
  }
  const warm = lives.slice(Math.min(5, lives.length - 1));
  const flat = warm.every((x) => x === warm[0]);
  // memory.buffer stops growing: one value over the last third of the cycles (a single growth step of the allocator after the
  // warm-up is not a leak; the live bytes of the count build are the leak check)
  const bufWarm = new Set(bufs.slice(Math.floor((bufs.length * 2) / 3)));
  console.log(`memory: ${SOAK} cycles; live bytes after warm-up ${flat ? `flat at ${warm[0]}` : `NOT flat: ${warm.join(', ')}`}; memory.buffer ${[...bufWarm].join(', ')}`);
  if (!flat && warm[0] !== null && warm[0] !== -1) fail('memory: live bytes not flat');
  if (bufWarm.size > 1) fail('memory: the wasm memory grew after warm-up');
}

const only = process.env.UO_EO_ONLY ? new RegExp(process.env.UO_EO_ONLY) : null;
if (only) for (const s of Object.values(SCRIPTS)) s.docs = s.docs.filter((d) => only.test(d));
const fams = MODE === 'all' ? Object.keys(SCRIPTS).filter((k) => k !== 'probe') : Object.keys(SCRIPTS).includes(MODE) ? [MODE] : [];
const t0 = Date.now();
const usedAll = new Set();
for (const f of fams) for (const k of await runFamily(f)) usedAll.add(k);
if (MODE === 'memory' || MODE === 'all') await runMemory();
const ranKeys = new Set(fams.flatMap((f) => SCRIPTS[f].docs.map((d) => `${d.replace(/\.docx$/, '')}|${f}`)));
const stale = Object.keys(DECLARED_O).filter((k) => ranKeys.has(k) && !usedAll.has(k) && !entriesOf(k).every((d) => d.intermittent));
if (stale.length) fail(`declared but equal now (remove from DECLARED_O): ${stale.join(', ')}`);
writeFileSync(join(outDir, `diffs-${MODE}.json`), JSON.stringify(allDiffs, null, 1));
await env.close();
console.log(`editor_switch_objects (${MODE}): ${((Date.now() - t0) / 1000).toFixed(0)} s; ${failures ? `${failures} FAILURE(S)` : 'ok'}`);
process.exit(failures ? 1 : 0);
