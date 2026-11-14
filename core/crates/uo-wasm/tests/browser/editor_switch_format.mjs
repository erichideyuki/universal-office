// Wave 7, frontier A1 of the editor switch (record core/docs/modules/uo-bindings.md "Wave 7 — A1: File, View, Font and
// Paragraph"): the families wave 6 left disabled — the Font dialog and effects, the Format Painter, the Paragraph/Tabs/Borders
// dialogs and the Borders button, the plain find/replace and the document reads, the Manual Hyphenation, the views and screen
// adornments (gridlines, bookmark and formatting marks, the TOC control and gallery preview, plugin inlines), and the File
// exports and the .odt open — of `<universal-write engine="rust">` against the TS engine in a real headless Chromium.
//
//   UO_TS_ROOT=<frozen checkout> UO_WASM_PATH=<session .wasm> [UO_CHROMIUM=...] [UO_EF_SOAK=30] [UO_EF_ONLY=<regex of docs>]
//   node crates/uo-wasm/tests/browser/editor_switch_format.mjs --mode font|para|ruler|text|view|bar|file|memory|all
//
// Per family and document: the TS element first, then the Rust one (`author="Ana"`, the ribbon), loaded with
// `loadDocx(bytes, { protected: false })`; the SAME script (element calls, real keys and clicks, bar buttons by id with their
// dialogs driven to OK) on both; after each step the family's facets (canonical JSON), the selection and the uw-* events; at
// the end the page count, the screen with A1's overlays on and the others off, and the saved .docx part by part. The `file`
// family compares the exports (HTML and Markdown as strings, the .odt part by part, the printed pages' pixels; XPS: the Rust
// package's structure and text, declared ED-704) and reopens the TS .odt in both engines. Every difference not covered by
// DECLARED (A1's own, decision D-A1-10) fails, and so does any page error.
// Declared duplicate: the page helpers are copy-adapted from editor_switch_review.mjs (gate item: move them into the lib).
import { createRequire } from 'node:module';
import { join } from 'node:path';
import { writeFileSync } from 'node:fs';
import { setup, core, NOW } from './editor_switch_lib.mjs';

const root = process.env.UO_TS_ROOT;
if (!root) throw new Error('set UO_TS_ROOT to the frozen TS checkout');
const arg = (name, dflt) => { const i = process.argv.indexOf(name); return i >= 0 ? process.argv[i + 1] : dflt; };
const MODE = arg('--mode', 'all');
const SOAK = Number(process.env.UO_EF_SOAK ?? 30);
const target = process.env.CARGO_TARGET_DIR ?? join(core, 'target');
const wasmPath = process.env.UO_WASM_PATH ?? join(target, 'session.wasm');
const outDir = join(target, 'es-format');
const { unzipSync } = createRequire(join(root, 'write/package.json'))('fflate');
const D3 = join(core, 'crates/uo-docx/tests/parity/d3/fixtures');
const extraDocs = Object.fromEntries(['fields', 'many-fields', 'hyperlinks', 'styles', 'headers', 'inlines', 'sections', 'tables', 'pictures'].map((n) => [`${n}.docx`, join(D3, `${n}.docx`)]));
// F2 round 2 (ED-713): Word's document with chapter page numbering (w:chapStyle="1", lowerRoman) and a TOC it wrote
extraDocs['toc-chap-chapter-roman.docx'] = join(core, 'crates/uo-docx-layout/tests/fidelity/toc-chap-chapter-roman.docx');
// wave 7c (S): the same probe's lowerRoman (no chapter) and chapter (decimal) documents, for updateFields' return value
extraDocs['toc-chap-roman.docx'] = join(core, 'crates/uo-docx-layout/tests/fidelity/toc-chap-roman.docx');
extraDocs['toc-chap-chapter.docx'] = join(core, 'crates/uo-docx-layout/tests/fidelity/toc-chap-chapter.docx');

// the bar commands no layout shows (`borders`): run by id through the chrome's registry, caught as editor_switch_stories.mjs does
// (a harness-only patch of the shared CommandRegistry; the element's code is unchanged)
const entryExtra = `import { CommandRegistry } from '@universal-office/ui';
{ const add0 = CommandRegistry.prototype.add; CommandRegistry.prototype.add = function (...c) { window.__lastReg = this; return add0.apply(this, c); }; }`;
const env = await setup({ root, wasmPath, outDir, stride: 1, only: 'f2-3-0|f2-3-2|f7a__7__0|f7b__7__0|f7c__7__0|f8__7__0', extraDocs, entryExtra });
const fixtureName = (p) => p.replace(/\\/g, '/').replace(/^.*\//, '');
const docOf = (want) => { for (const [n, p] of env.docsByName) if (fixtureName(p) === want || n === want) return n; throw new Error(`no document ${want}`); };
let failures = 0;
const fail = (m) => { failures++; console.log(`FAIL ${m}`); };

/** the page-side helpers (evaluated in each new page) */
const helpers = () => {
  const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
  const frame = () => new Promise((r) => requestAnimationFrame(() => r()));
  const fnv = (u8) => { let h = 0x811c9dc5; for (let i = 0; i < u8.length; i++) h = Math.imul(h ^ u8[i], 0x01000193); return (h >>> 0).toString(16).padStart(8, '0'); };
  const EVENTS = ['uw-change', 'uw-dirty-change', 'uw-custom-inline-inserted', 'uw-toc-update', 'uw-toc-menu', 'uw-select-objects', 'uw-protected', 'uw-zoom'];
  /** A1's overlays (on in the end capture for both engines) and the other frontiers' (off) */
  const OTHERS = ['paintEngineSpelling', 'paintSearchHits', 'paintTableAdorners', 'paintMultiSelection'];
  const log = [];
  const canon = (v) => (Array.isArray(v) ? v.map(canon) : v && typeof v === 'object'
    ? Object.fromEntries(Object.entries(v).filter(([, x]) => x !== undefined && x !== null).sort(([a], [b]) => (a < b ? -1 : 1)).map(([k, x]) => [k, canon(x)]))
    : typeof v === 'number' ? +v.toFixed(3) : v);
  const J = (v) => JSON.stringify(canon(v));
  const api = {
    el: null,
    async make(engine) {
      const el = document.createElement('universal-write');
      if (engine === 'rust') el.setAttribute('engine', 'rust');
      el.setAttribute('fonts', '/fonts');
      el.setAttribute('author', 'Ana');
      el.setAttribute('lang', 'pt');
      el.setAttribute('topbar', '');
      el.setAttribute('toolbar', window.__toolbar ?? 'ribbon');
      el.style.cssText = 'display:block;width:1100px;height:850px';
      const ready = new Promise((r) => el.addEventListener('uw-ready', r, { once: true }));
      window.__lastReg = null;
      document.body.appendChild(el);
      await ready;
      if (el.rust) el.rust.tsShading = true; // wave 7c (V, fix round 1): the TS field shading/comment look (ED-304: Word's is the default)
      for (let i = 0; i < 200 && !window.__lastReg; i++) await sleep(25);
      api.reg = window.__lastReg;
      for (const t of EVENTS) el.addEventListener(t, (e) => { log.push(`${t} ${J(e.detail ?? null)}`); });
      // a plugin inline renderer (insertCustomInline / paintEngineInlines)
      el.use({ id: 'a1', inlineRenderers: [{ type: 'tag', toText: (d) => `#${d.name}`, draw: (a) => { a.ctx.fillStyle = '#c00'; a.ctx.fillRect(a.xTwips * a.scale, a.yTwips * a.scale, Math.max(1, a.widthTwips * a.scale), Math.max(1, a.heightTwips * a.scale)); } }] });
      // the `bar` family: the host hooks the File commands end in (export, print, compare), recorded as facets
      if (window.__toolbar === 'compact') {
        window.__exp = [];
        window.__printed = null;
        const hashPages = (pages) => Promise.all(pages.map(async (u) => {
          const img = new Image(); img.src = u; await img.decode();
          const c = document.createElement('canvas'); c.width = img.width; c.height = img.height;
          const x = c.getContext('2d'); x.drawImage(img, 0, 0);
          return `${img.width}x${img.height}:${fnv(new Uint8Array(x.getImageData(0, 0, c.width, c.height).data.buffer))}`;
        }));
        el.host = {
          // html/md: the text compared; odt (zip), xps (ED-704) and pdf: produced (their content: the `file` mode)
          exportFile: async (kind, name, data) => {
            const d = await data();
            const u8 = typeof d === 'string' ? new TextEncoder().encode(d) : d;
            window.__exp.push(kind === 'html' || kind === 'md' ? `${kind} ${name} ${u8.length} ${fnv(u8)}` : `${kind} ${name} ${u8.length > 0}`);
            return name;
          },
          print: async ({ pages, title }) => { if (window.__dump) window.__printedUrls = pages; window.__printed = [title, ...(await hashPages(pages))]; },
          openFileForCompare: async () => ({ name: 'outro.docx', bytes: new Uint8Array(await (await fetch(`/docs/${window.__other}`)).arrayBuffer()) }),
        };
      }
      // the bar's Compare under engine="rust" must run the Rust diffDocuments (counted; checked by the node side)
      window.__rustDiffCalls = 0;
      const f0 = el.rust?.compareDiff;
      if (f0) Object.defineProperty(el.rust, 'compareDiff', { configurable: true, value: (a, b) => { window.__rustDiffCalls++; return f0(a, b); } });
      api.el = el;
      return true;
    },
    async settle(ms = 300) {
      const el = api.el;
      if (el.rust) await el.rust.idle();
      else { await document.fonts.ready; await sleep(ms); }
      await frame();
    },
    async load(n, odtBase64) {
      window.__seed(1);
      if (odtBase64) api.el.loadOdt(Uint8Array.from(atob(odtBase64), (c) => c.charCodeAt(0)), { protected: false });
      else api.el.loadDocx(new Uint8Array(await (await fetch(`/docs/${n}`)).arrayBuffer()), { protected: false });
      await api.settle();
      api.el.refresh();
      await api.settle();
      log.length = 0;
      return api.el.layoutPageCount();
    },
    // (wave 7c, S: the harness's own references to the element's command registry go too — a removed element is freed
    // when it is collected, and the registry's commands close over it)
    remove() { api.el?.remove(); api.el = null; api.reg = null; window.__lastReg = null; },
    drain() { const out = log.slice(); log.length = 0; return out; },
    focus() { api.el.focusEditor(); },
    async js(code) {
      const el = api.el;
      // eslint-disable-next-line no-new-func
      const f = new Function('el', 'es', `return (async () => { ${code} })();`);
      let r;
      try { r = await f(el, api); } catch (e) { r = `THROW ${e && e.message}`; }
      await api.settle(150);
      return J(r === undefined ? null : r);
    },
    button(id) {
      const find = (n) => { if (n.dataset?.a === id) return n; for (const c of n.children ?? []) { const f = find(c); if (f) return f; } if (n.shadowRoot) return find(n.shadowRoot); return null; };
      return find(api.el.shadowRoot);
    },
    /** every element under the element's shadow root (nested shadow roots too) matching `sel` */
    all(sel) {
      const out = [];
      const walk = (n) => { if (n.matches?.(sel)) out.push(n); for (const c of n.children ?? []) walk(c); if (n.shadowRoot) walk(n.shadowRoot); };
      walk(api.el.shadowRoot);
      return out;
    },
    closeMenus() { document.dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape', bubbles: true })); for (const m of api.all('.uo-menu')) m.remove(); },
    /** a command that lives in a menu (a split button's, a menu tab's, the compact bar's): the menus opened one by one (and
     *  their submenus, one level) until the item shows; null when no menu has it */
    async viaMenus(id) {
      const openers = api.all('[aria-haspopup="menu"], .uo-split-arrow, .uo-tab-file');
      for (const o of openers) {
        api.closeMenus();
        o.dispatchEvent(new MouseEvent('mousedown', { bubbles: true, composed: true }));
        o.click();
        await frame();
        let it = api.button(id);
        if (!it) {
          for (const s of api.all('.uo-menu [aria-haspopup="menu"]')) {
            if (s._openSub) s._openSub(false); else s.click();
            await frame();
            it = api.button(id);
            if (it) break;
          }
        }
        if (it) return it;
      }
      api.closeMenus();
      return null;
    },
    async clickButton(id) {
      let bt = api.button(id);
      let via = '';
      if (!bt) { bt = await api.viaMenus(id); via = 'menu '; }
      if (!bt) {
        // in no layout: through the registry (a command the element does not support stays `missing`)
        const reg = api.reg;
        if (!reg?.supported(id)) return 'missing';
        const on = reg.enabled(id);
        if (on) reg.run(id);
        await api.settle(250);
        return `registry ${on ? 'run' : 'disabled'}`;
      }
      const off = !!(bt.disabled || bt.getAttribute?.('aria-disabled') === 'true' || bt.classList?.contains('disabled'));
      bt.dispatchEvent(new MouseEvent('mousedown', { bubbles: true, composed: true }));
      bt.click();
      await api.settle(250);
      api.closeMenus();
      return via + (off ? 'disabled' : 'clicked');
    },
    clickPoint() {
      const el = api.el;
      const p = window.__clickAt;
      if (!p) return null;
      const keep = el.getSelection();
      // fix round 2 (V): the point is measured with the caret at `p` (each engine's caretRectAt answers the focus), then the
      // selection goes back — which scrolls the caret into view in both engines (the TS lays the continuous views out
      // again and scrolls on every selection change): the view is put back at the scroll the point was measured at, so
      // the click lands on `p`'s own caret point (fix round 1 clicked at a stale point: another paragraph in each engine)
      el.setSelection(p);
      let r = el.caretRectAt(p);
      const box = el.getBoundingClientRect();
      if (r && (r.y < box.top + 60 || r.y + r.h > box.bottom - 60)) { el.scrollTop += r.y - (box.top + box.height / 2); r = el.caretRectAt(p); }
      const top = el.scrollTop;
      el.setSelection(keep.anchor, keep.focus);
      el.scrollTop = top;
      return r ? { x: r.x + 0.5, y: r.y + r.h / 2 } : null;
    },
    dialogs() { return [...api.el.shadowRoot.querySelectorAll('dialog')].filter((d) => d.open).map((d) => d.innerText.replace(/\s+/g, ' ').trim().slice(0, 300)); },
    /** real input in the open dialog before OK: `code` runs with `d` (the dialog), `es` (this api), `pick(button, hex)` (a
     *  colour button opened and a swatch of its palette clicked) and `set(field, value)` (value + input/change events) */
    async fill(code) {
      const d = [...api.el.shadowRoot.querySelectorAll('dialog')].find((x) => x.open);
      if (!d) return 'no dialog';
      const set = (f, v) => { if (!f) throw new Error('no field'); f.value = v; f.dispatchEvent(new Event('input', { bubbles: true })); f.dispatchEvent(new Event('change', { bubbles: true })); };
      const pick = async (btn, hex) => {
        if (!btn) throw new Error('no colour button');
        btn.click();
        await frame();
        const sw = api.all(`[data-color="${hex}"]`)[0] ?? document.querySelector(`[data-color="${hex}"]`);
        if (!sw) throw new Error(`no swatch ${hex}`);
        sw.click();
        await frame();
      };
      // eslint-disable-next-line no-new-func
      const f = new Function('d', 'es', 'pick', 'set', `return (async () => { ${code} })();`);
      try { return J((await f(d, api, pick, set)) ?? 'filled'); } catch (e) { return `THROW ${e && e.message}`; }
    },
    /** the screen of the pages now (A1's overlays on, the other frontiers' off, no caret): the canvas hashes */
    async shot() {
      const el = api.el;
      el.ensureVisiblePainted();
      window.clearInterval(el.caretTimer);
      el.caretVisible = false;
      // the other frontiers' overlays off for the capture, then their own (instance) definitions put back
      const own = Object.fromEntries(OTHERS.map((m) => [m, Object.getOwnPropertyDescriptor(el, m)]));
      for (const m of OTHERS) Object.defineProperty(el, m, { value: () => {}, configurable: true, writable: true });
      el.paint();
      await frame();
      const cs = [...el.shadowRoot.querySelectorAll('canvas.page')];
      const out = cs.map((cv) => (cv.width > 1 ? `${cv.width}x${cv.height}:${fnv(new Uint8Array(cv.getContext('2d').getImageData(0, 0, cv.width, cv.height).data.buffer))}` : '-'));
      if (window.__dump) window.__shots = (window.__shots ?? []).concat([cs.map((cv) => (cv.width > 1 ? cv.toDataURL('image/png') : null))]);
      for (const m of OTHERS) { if (own[m]) Object.defineProperty(el, m, own[m]); else delete el[m]; }
      el.paint();
      return out;
    },
    /**
     * A drag on the ruler (chrome-ruler.ts: client x = page canvas left + twips × zoom × 96/1440; the band height decides the
     * marker under the pointer, ruler.ts `markerAt`): `kind` first-line | hanging | left-indent | right-indent | tab-add | tab
     * | tab-off | m-right | m-top | cell | column; answers { from, to } client points or null when the marker is not there
     */
    rulerDrag(kind, dTw) {
      const el = api.el;
      const info = el.getRulerInfo();
      if (!info) return null;
      const view = el.getView();
      const cs = el.shadowRoot.querySelectorAll('canvas.page');
      const pc = cs[view === 'print' ? info.page : 0] ?? cs[0];
      const hc = api.all('.uw-hruler .uo-ruler-canvas')[0];
      const vc = api.all('.uw-vruler .uo-ruler-canvas')[0];
      if (!pc || !hc) return null;
      const pr = pc.getBoundingClientRect(), hr = hc.getBoundingClientRect();
      const scale = (el.getZoom() * 96) / 1440;
      const { frame: fr, indents, margins, tabs, columns, table } = info;
      const H = (tw, c) => ({ x: pr.left + tw * scale, y: hr.top + hr.height * c });
      const left = fr.x0 + indents.left;
      let at = null, c = 0.5;
      if (kind === 'first-line') { at = left + indents.firstLine; c = 0.2; }
      else if (kind === 'hanging') { at = left; c = 0.57; }
      else if (kind === 'left-indent') { at = left; c = 0.86; }
      else if (kind === 'right-indent') { at = fr.x1 - indents.right; c = 0.86; }
      else if (kind === 'tab-add') { at = fr.x0 + Math.round((fr.x1 - fr.x0) * 0.43); c = 0.5; }
      else if (kind === 'tab' || kind === 'tab-off') { if (!tabs.length) return null; at = fr.x0 + tabs[0].pos; c = 0.86; }
      else if (kind === 'm-right') { if (view !== 'print' || info.where === 'cell') return null; at = info.pageWidthTwips - margins.right; c = 0.2; }
      else if (kind === 'cell') { if (!table || table.cols.length < 2) return null; at = table.cols[1].x; c = 0.5; }
      else if (kind === 'column') { if (info.where === 'cell' || columns.length < 2) return null; at = columns[0].x + columns[0].w; c = 0.5; }
      else if (kind === 'm-top') {
        if (!vc || view !== 'print') return null;
        const vr = vc.getBoundingClientRect();
        const from = { x: vr.left + vr.width * 0.5, y: pr.top + margins.top * scale };
        return { from, to: { x: from.x, y: from.y + dTw * scale } };
      }
      if (at === null) return null;
      const from = H(at, c);
      return { from, to: kind === 'tab-off' ? { x: from.x, y: hr.bottom + 40 } : { x: from.x + dTw * scale, y: from.y } };
    },
    /** the page canvases' CSS size and buffer size (the continuous views' sheet, the buffer scale) */
    canvases() {
      return [...api.el.shadowRoot.querySelectorAll('canvas.page')].map((c) => [c.style.width, c.style.height, c.width, c.height]);
    },
    /** wave 7c (V): the text's left inset from the first sheet (the margin the view gives a paragraph without indent; the TS
     *  `contMarginTwips`), from the proxy's left at a caret put at (0, 0) and restored */
    firstInset() {
      const el = api.el;
      const ta = el.shadowRoot.querySelector('textarea');
      if (!el.shadowRoot.querySelector('canvas.page') || !ta) return null;
      const keep = el.getSelection();
      el.setSelection({ para: 0, offset: 0 });
      // fix round 2 (V): the sheet read AFTER the selection change — the TS lays a continuous view out again on every
      // selection change (`refreshSelection` without an engine → `refresh()`, 21379) and rebuilds its canvases, so a
      // canvas taken before it is detached (offsetLeft 0: the TS "inset 64" of fix round 1 was that stale canvas)
      const cv = el.shadowRoot.querySelector('canvas.page');
      const x = Math.round(parseFloat(ta.style.left) - cv.offsetLeft);
      el.setSelection(keep.anchor ?? keep, keep.focus ?? keep);
      return x;
    },
    /** wave 7c (V): the strips of the continuous sheet (Rust): contiguous from the sheet's top, none over 4000 CSS px, the
     *  last at the sheet's end — `null` when the sheet has none (the TS tiles are page canvases) */
    stripCheck() {
      const el = api.el;
      const cv = el.shadowRoot.querySelector('canvas.page');
      const ss = [...el.shadowRoot.querySelectorAll('canvas.page-strip')];
      if (!cv || !ss.length) return null;
      const tops = ss.map((x) => [parseFloat(x.style.top) - cv.offsetTop, parseFloat(x.style.height)]).sort((a, b) => a[0] - b[0]);
      const bad = [];
      for (let i = 1; i < tops.length; i++) if (Math.abs(tops[i][0] - (tops[i - 1][0] + tops[i - 1][1])) > 0.5) bad.push(`gap at ${tops[i][0]}`);
      for (const t of tops) if (t[1] > 4000.5) bad.push(`strip ${t[1]} px`);
      return bad.length ? bad : null;
    },
    /** the continuous views' strips (fix round 2): CSS and buffer sizes */
    strips() {
      return [...api.el.shadowRoot.querySelectorAll('canvas.page-strip')].map((c) => [c.style.width, c.style.height, c.width, c.height]);
    },
    /** the open dialog driven to OK (its primary button), the others closed */
    async okDialogs() {
      for (const d of api.el.shadowRoot.querySelectorAll('dialog')) {
        if (!d.open) continue;
        const ok = d.querySelector('.uo-pill-primary');
        if (ok) ok.click(); else d.close();
        await api.settle(150);
      }
      for (const d of api.el.shadowRoot.querySelectorAll('dialog')) if (d.open) d.close();
    },
    facets(fam) {
      const el = api.el;
      const f = { sel: el.getSelection(), events: api.drain(), dirty: el.isDirty() };
      const st = el.getSelectionState();
      const run = st.run;
      f.run = { bold: run.bold, italic: run.italic, underline: run.underline, underlineStyle: run.underlineStyle, underlineColor: run.underlineColor,
        color: run.color, szHalfPt: run.szHalfPt, fontAscii: run.fontAscii, strike: run.strike, doubleStrike: run.doubleStrike, vertAlign: run.vertAlign,
        smallCaps: run.smallCaps, allCaps: run.allCaps, hidden: run.hidden, scalePct: run.scalePct, charSpacingTwips: run.charSpacingTwips,
        positionHalfPt: run.positionHalfPt, kernHalfPt: run.kernHalfPt, textFx: run.textFx, openType: run.openType };
      f.para = st.para;
      if (fam === 'font') {
        f.fx = el.textEffects();
        f.theme = el.documentThemeColors();
        f.painter = [el.painterActive(), el.painterSticky()];
        f.ul = el.underlineButtonStyle();
      } else if (fam === 'para') {
        f.indents = el.getParagraphIndents();
        f.tabs = el.getTabStops();
        f.defTab = el.getDefaultTabStop();
        f.flow = el.getParagraphFlow();
        f.layout = el.getParagraphLayout();
        f.borders = el.getParagraphBorders();
        f.sides = el.paragraphBorderSides();
        f.ruler = el.getRulerInfo();
        f.ranges = el.getSelectionRanges();
      } else if (fam === 'text') {
        f.links = el.getLinks();
        f.inlines = el.getCustomInlines();
        f.ext = el.hasExternalFields();
        f.count = el.countMatches({ text: 'a' });
        f.stats = el.stats();
      } else if (fam === 'view') {
        f.view = el.getView();
        f.grid = el.gridlinesShown();
        f.bm = el.bookmarksShown();
        // wave 7c (V): the ruler split — its frame, page geometry, indents and tabs (the presentation: equal) and the focus
        // line's top (content geometry: the line positions are Word's in the continuous views, declared below)
        const ru = el.getRulerInfo();
        if (ru) { const { lineTop, page, ...frame } = ru; f.ruler = frame; f.rulerLine = { lineTop, page }; } else { f.ruler = null; f.rulerLine = null; }
        // the content geometry: the sheet heights (the TS tiles at its legacy line positions, the Rust sheet at Word's)
        f.canvases = [...api.canvases(), ...api.strips().map((x) => ['strip', ...x])];
        // wave 7c (V): the presentation of the views (TS 1:1): the sheet's CSS width and its left in the element, the left
        // and top insets of the text (the TS contMarginTwips), the logical page count and the current page
        const caret = el.view !== 'print' && el.view !== 'read' && el.shadowRoot.querySelector('canvas.page') ? api.firstInset() : null;
        // fix round 2 (V): the sheet read after `firstInset` (its selection change lays the TS continuous view out again and
        // replaces the canvases: a canvas read before it was a detached one — the TS `left 0` of fix round 1)
        const cv0 = el.shadowRoot.querySelector('canvas.page');
        f.present = { w: cv0?.style.width ?? null, left: cv0 ? cv0.offsetLeft : null, inset: caret, cur: el.currentPage() };
        f.stripCheck = api.stripCheck();
        f.pages = [el.layoutPageCount(), el.stats()];
      } else if (fam === 'ruler') {
        f.indents = el.getParagraphIndents();
        f.tabs = el.getTabStops();
        f.ruler = el.getRulerInfo();
        f.table = el.getTableState();
        f.section = el.getSection();
      } else if (fam === 'bar') {
        f.exported = window.__exp.splice(0);
        // the printed pages: taken by the step that waits for host.print (both engines then compared, never a race)
        f.printed = window.__printedTaken ?? null; window.__printedTaken = null;
        const wd = el.shadowRoot.querySelector('.uw-compare universal-write-diff');
        // the diff itself (the Rust diffDocuments under engine="rust", the TS one in the TS element) and its view
        f.compare = wd ? { comparing: el.classList.contains('uw-comparing'), stats: wd.stats, diff: wd._diff ?? null, text: fnv(new TextEncoder().encode(wd.shadowRoot?.textContent ?? '')) } : null;
        f.protected = el.isProtected();
        f.tabs = el.getTabStops();
        f.sides = el.paragraphBorderSides();
        f.embedding = el.getFontEmbedding?.();
      }
      // wave 7c (W, item 2, fix round 1): the document's default tab stop is ONE facet, `defTab` (getDefaultTabStop and the
      // copies getTabStops/getRulerInfo carry, which must agree within the engine): a package whose settings give no
      // w:defaultTabStop is Word's 708 tw (measured, oracle nosettings) and the frozen TS model's 720, declared per document
      // on `defTab` alone, so the tabs and the ruler stay compared strictly
      const dts = [];
      if (f.defTab !== undefined) dts.push(f.defTab);
      for (const k of ['tabs', 'ruler']) {
        const o = f[k];
        if (o && typeof o === 'object' && 'defaultTabTwips' in o) { const { defaultTabTwips, ...rest } = o; dts.push(defaultTabTwips); f[k] = rest; }
      }
      if (dts.length) f.defTab = dts.every((x) => x === dts[0]) ? dts[0] : { disagree: dts };
      f.selected = el.selectedText();
      return J(f);
    },
    async end() {
      const el = api.el;
      el.refresh();
      await api.settle();
      const cs = [...el.shadowRoot.querySelectorAll('canvas.page')];
      el.scrollTop = 0;
      await frame(); await frame();
      el.ensureVisiblePainted();
      window.clearInterval(el.caretTimer);
      el.caretVisible = false;
      const keepSel = el.getSelection();
      el.caretVisible = false;
      for (const m of OTHERS) el[m] = () => {};
      el.paint();
      await sleep(300);
      el.paint();
      await frame();
      const screen = cs.map((cv) => (cv.width > 1 ? fnv(new Uint8Array(cv.getContext('2d').getImageData(0, 0, cv.width, cv.height).data.buffer)) : '-'));
      const shots = window.__dump ? cs.map((cv) => (cv.width > 1 ? cv.toDataURL('image/png') : null)) : [];
      const bytes = el.saveDocx();
      let bin = '';
      for (let i = 0; i < bytes.length; i += 0x8000) bin += String.fromCharCode(...bytes.subarray(i, i + 0x8000));
      return { pages: cs.length, screen, shots, sel: keepSel, docx: btoa(bin), rustDiffCalls: window.__rustDiffCalls ?? 0, midShots: window.__shots ?? [] };
    },
    async exports() {
      const el = api.el;
      const b64 = (u8) => { let s = ''; for (let i = 0; i < u8.length; i += 0x8000) s += String.fromCharCode(...u8.subarray(i, i + 0x8000)); return btoa(s); };
      const out = {};
      try { out.html = await el.exportHtml('T'); } catch (e) { out.html = `THROW ${e.message}`; }
      try { out.htmlOmit = await el.exportHtml(undefined, { images: 'omit' }); } catch (e) { out.htmlOmit = `THROW ${e.message}`; }
      try { out.md = await el.exportMarkdown('T'); } catch (e) { out.md = `THROW ${e.message}`; }
      try { out.odt = b64(el.exportOdt()); } catch (e) { out.odt = `THROW ${e.message}`; }
      try { out.saveOdt = b64(await el.save('odt')); } catch (e) { out.saveOdt = `THROW ${e.message}`; }
      try { out.xps = b64(el.exportXps()); } catch (e) { out.xps = `THROW ${e.message}`; }
      // the text the session's layout painted on each page (every glyph run of the display list the XPS was written from, in
      // item order: the shown field results, list numbers, headers/footers; Rust only)
      const dl = el.rust?.dl;
      out.pageText = dl ? dl.pages.map((p) => p.items.filter((it) => it.t === 'glyphs').map((g) => String.fromCodePoint(...g.glyphs.map((q) => q.cp))).join('')) : null;
      // fix round 2: what each page of the XPS must hold (uo-pdf xps.rs): a Glyphs element per written run (one per glyph when the
      // run goes back), an ImageBrush per picture at least, an Opacity per glyph run or hex rectangle with an alpha under 1 at least
      const blank = (g) => g.glyphs.every((q) => q.cp < 0x20 || q.cp === 0xad || q.cp === 0xfffc || q.cp === 0x200b || /^\s$/u.test(String.fromCodePoint(q.cp)));
      out.pageCounts = dl ? dl.pages.map((p) => {
        const runs = p.items.filter((it) => it.t === 'glyphs' && it.glyphs.length && it.sizePt > 0 && !blank(it));
        const glyphs = runs.reduce((n, g) => n + (g.glyphs.every((q, i) => i === 0 || q.x >= g.glyphs[i - 1].x) ? 1 : g.glyphs.length), 0);
        const images = p.items.filter((it) => it.t === 'image').length;
        const alpha = runs.filter((g) => g.alpha !== undefined && g.alpha < 1).length
          + p.items.filter((it) => it.t === 'rect' && /^[0-9A-Fa-f]{6}$/.test(it.fill) && it.alpha !== undefined && it.alpha < 1).length;
        return { glyphs, images, alpha };
      }) : null;
      try { out.saveMd = await el.save('markdown', { title: 'X' }); } catch (e) { out.saveMd = `THROW ${e.message}`; }
      try {
        const pages = await el.printPages();
        out.print = await Promise.all(pages.map(async (u) => {
          const img = new Image(); img.src = u; await img.decode();
          const c = document.createElement('canvas'); c.width = img.width; c.height = img.height;
          const x = c.getContext('2d'); x.drawImage(img, 0, 0);
          return `${img.width}x${img.height}:${fnv(new Uint8Array(x.getImageData(0, 0, c.width, c.height).data.buffer))}`;
        }));
      } catch (e) { out.print = `THROW ${e.message}`; }
      // wave 7 (F2): the decode state of every image of the Rust layout after the print (the pictures path: none failed)
      const dl2 = el.rust?.dl; if (dl2 && el.rust.frontApi) { const fa = el.rust.frontApi(); out.imageStates = (dl2.tables?.images ?? []).map((_, i) => fa.imageState(i)); }
      return out;
    },
    stats() { return window.__e.UniversalWriteElement.engineStats(); },
  };
  window.__es = api;
  return true;
};
const ev = (f, a) => Promise.race([env.page.evaluate(f, a), new Promise((_, rej) => setTimeout(() => rej(new Error(`evaluate timed out: ${String(f).slice(0, 120)}`)), 120000))]);
const VERBOSE = !!process.env.UO_EF_VERBOSE;

const P = (p, o) => `{ para: ${p}, offset: ${o} }`;
// real input in the dialogs (`fill`, run before OK; `d` the dialog, `pick` a palette swatch, `set` a field's value)
const FILL = {
  // More Underlines (the Font dialog): a wavy underline in red
  underline: `const s = [...d.querySelectorAll('select')].find((x) => [...x.options].some((o) => o.value === 'wave'));
    set(s, 'wave');
    const btn = [...d.querySelectorAll('button')].find((b) => (s.compareDocumentPosition(b) & Node.DOCUMENT_POSITION_FOLLOWING) && b.textContent.includes('▾'));
    await pick(btn, 'FF0000');
    return [s.value, btn.textContent];`,
  // the Font dialog: size and font colour typed in the fields
  font: `const size = [...d.querySelectorAll('input')].find((x) => /^\\d+([.,]\\d+)?$/.test(x.value) && x.type !== 'checkbox');
    set(size, '13');
    const btn = [...d.querySelectorAll('button')].find((b) => b.textContent.includes('▾'));
    await pick(btn, '0070C0');
    return [size.value, btn.textContent];`,
  // the Paragraph dialog: left 1,5 cm, hanging 0,5 cm, 6 pt before
  paragraph: `set(d.querySelector('input[name="left"]'), '1,5');
    set(d.querySelector('select[name="special"]'), 'hanging');
    set(d.querySelector('input[name="by"]'), '0,5');
    set(d.querySelector('input[name="before"]'), '6');
    return null;`,
  // Tabs: a centred dotted stop at 3 cm (set by OK, as Word) and another at 7 cm by the Set button
  tabs: `const pos = d.querySelector('input[name="pos"]');
    const radio = (n, v) => { const r = d.querySelector('input[name="' + n + '"][value="' + v + '"]'); r.checked = true; r.dispatchEvent(new Event('change', { bubbles: true })); };
    set(pos, '7'); radio('align', 'right'); radio('leader', 'hyphen');
    [...d.querySelectorAll('button')].find((b) => /^(Definir|Set)$/.test(b.textContent.trim())).click();
    set(pos, '3'); radio('align', 'center'); radio('leader', 'dot');
    return [...d.querySelector('select[name="list"]').options].map((o) => o.textContent);`,
  // Borders and Shading: a red Box of 1½ pt (colour and width first: the setting reads the pen)
  borders: `const color = [...d.querySelectorAll('button')].find((b) => b.textContent.includes('▾'));
    await pick(color, 'FF0000');
    set(d.querySelector('select[name="b-width"]'), '12');
    [...d.querySelectorAll('button.uo-btn-lbl')].find((b) => /Caixa|Box/.test(b.textContent)).click();
    return color.textContent;`,
};
const SCRIPTS = {
  font: {
    // f7a__7__0, not f2-3-0: f2-3-0's theme colours differ on screen (ED-314, front B's gate-O3); every screen here compared
    docs: ['f7a__7__0.docx', 'styles.docx'],
    steps: [
      { js: 'return null;' },
      { js: `el.setSelection(${P(0, 0)}, ${P(0, 4)}); el.setFontFormat({ bold: true, italic: true, underline: 'double', sizePt: 15, color: '1F4E79', strike: true, smallCaps: true, scalePct: 120, spacingTwips: 20, positionHalfPt: 2, kernMinHalfPt: 24 }); return null;` },
      { js: `el.setFontFormat({ underlineColor: { themeColor: 'accent2' }, color: null, superscript: true }); return el.underlineButtonStyle();` },
      { js: `el.setUnderlineColor('FF0000'); return null;` },
      { js: `el.setUnderlineColor(null); return null;` },
      { js: `el.setTextEffects({ glow: { rad: 63500, color: { scheme: 'accent1', mods: [['alpha', 60000]] } } }); return el.textEffects();` },
      // the text effects on screen while they are applied (glow, outline, preset, gradient); the glow's colour as RGB for the
      // capture: no harness document has a theme part, and a scheme colour then resolves to Word's 2023 scheme in the session
      // (ED-314, front B's gate-O3) — the scheme glow above is compared by its answer
      { js: `el.setTextEffects({ glow: { rad: 63500, color: { rgb: '4472C4', mods: [['alpha', 60000]] } } }); return el.textEffects();` },
      // the glow's size: the session scales a text run's effects by its font size as the Word does (milestone 1, "Text effects
      // scale with the font size", uo-render `fx_for_word`; the frozen TS draws the 72 pt size at any size): declared ED-710
      { shot: true, tag: 'glow-shot' },
      { js: `el.setTextEffects({ glow: null, outline: { w: 9525, fill: { type: 'solid', color: { rgb: 'FF0000' } } } }); return el.textEffects();` },
      { shot: true },
      { js: `el.applyTextEffectPreset(2); return el.textEffects();` },
      { shot: true },
      { js: `el.applyTextEffectPreset(null); return el.textEffects();` },
      { js: `el.setOpenType({ ligatures: 'standardContextual', numForm: 'oldStyle', stylisticSets: [1, 3], cntxtAlts: true }); return el.textEffects();` },
      { js: `el.setOpenType({ numForm: 'default', stylisticSets: null }); return el.textEffects();` },
      { js: `el.setTextGradient({ angleDeg: 90, light: true }); return el.textEffects();` },
      { shot: true },
      { js: `el.setTextGradient(null); return el.textEffects();` },
      { js: `el.setSelection(${P(0, 2)}); el.setFontFormat({ bold: false, fontFamily: 'Liberation Serif' }); return null;` },
      { type: 'xy' },
      { key: 'Control+z' },
      { js: `el.setSelection(${P(0, 0)}, ${P(0, 3)}); return null;` },
      { button: 'dlg:font', ok: true },
      { button: 'underline:more', ok: true },
      // the dialogs with real input before OK
      { js: `el.setSelection(${P(1, 0)}, ${P(1, 5)}); return null;` },
      { button: 'underline:more', fill: FILL.underline, ok: true },
      { button: 'dlg:font', fill: FILL.font, ok: true },
      { shot: true },
      { button: 'color' },
      { button: 'color:gradient' },
      // the Format Painter: copy at the start of the paragraph, then a real click elsewhere applies it (the mouseup)
      { js: `el.setSelection(${P(0, 1)}); el.copyFormat(); window.__clickAt = ${P(1, 2)}; return [el.painterActive(), el.painterSticky()];` },
      { click: true },
      { js: `el.copyFormat(true); window.__clickAt = ${P(1, 4)}; return el.painterSticky();` },
      { click: true },
      // wave 7 (F2): the sticky painter applied a SECOND time (it stays armed after the first apply), then Esc turns it off
      { js: `window.__clickAt = ${P(1, 1)}; return [el.painterActive(), el.painterSticky()];` },
      { click: true },
      { js: 'return [el.painterActive(), el.painterSticky()];' },
      { key: 'Escape' },
      { js: 'return [el.painterActive(), el.painterSticky()];' },
      { button: 'painter' },
      { js: 'el.cancelFormatPainter(); return el.painterActive();' },
      { button: 'fx:pane' },
    ],
  },
  para: {
    docs: ['f7a__7__0.docx', 'headers.docx'],
    steps: [
      { js: 'return null;' },
      { js: `el.setSelection(${P(0, 1)}); el.setParagraphIndents({ left: 720, firstLine: -360 }); return null;` },
      { js: `el.setSpecialIndent('firstLine'); el.setIndentRight(567); return null;` },
      { js: `el.setTabStops([{ pos: 1440, align: 'center' }, { pos: 4320, align: 'right', leader: 'dot' }]); return null;` },
      { js: `el.setSelection(${P(0, 0)}, ${P(1, 0)}); el.setTabStops([], { add: [{ pos: 2880, align: 'decimal' }], remove: [1440] }); return null;` },
      { js: `el.setDefaultTabStop(567); return el.getDefaultTabStop();` },
      { js: `el.setParagraphFlow({ keepNext: true, widowControl: false, pageBreakBefore: false, suppressAutoHyphens: true }); return el.getParagraphFlow();` },
      { js: `el.setParagraphLayout({ left: 360, right: 180, before: 'auto', after: 240 }); return el.getParagraphLayout();` },
      { js: `el.setSelection(${P(1, 0)}); el.setParagraphBorders({ top: { val: 'single', sz: 8, color: 'FF0000' }, bottom: { val: 'double' } }); return el.getParagraphBorders();` },
      { js: `el.setParagraphBorders({ top: null }); return el.paragraphBorderSides();` },
      { js: `el.paragraphBorderPreset('outside'); return el.paragraphBorderSides();` },
      { js: `el.paragraphBorderPreset('none'); el.toggleBorder(); return el.paragraphBorderSides();` },
      { js: `el.setParagraphBorders(null); return el.getParagraphBorders();` },
      { button: 'home:border:all' },
      { button: 'home:border:inside' },
      { button: 'home:borders' },
      { button: 'borders' },
      { js: `el.setSelection(${P(2, 1)}); return null;` },
      { button: 'home:hrule' },
      { key: 'Control+z' },
      { js: `el.setSelection(${P(0, 0)}); el.insertHorizontalLine(); return null;` },
      { js: `el.setSelection(${P(1, 0)}, ${P(1, 2)}); return el.selectSimilarFormatting();` },
      { button: 'select:similar' },
      { js: `el.setSelection(${P(0, 0)}); return null;` },
      { button: 'dlg:paragraph', ok: true },
      { button: 'dlg:tabs', ok: true },
      { button: 'dlg:borders', ok: true },
      { key: 'Control+z' },
      // the dialogs with real input before OK (paragraph 2: left/hanging/before, two tab stops, a red Box)
      { js: `el.setSelection(${P(2, 0)}); return null;` },
      { button: 'dlg:paragraph', fill: FILL.paragraph, ok: true },
      { button: 'dlg:tabs', fill: FILL.tabs, ok: true },
      { button: 'dlg:borders', fill: FILL.borders, ok: true },
      { shot: true },
    ],
  },
  // the ruler's drags with the mouse (real pointer events on the ruler canvases: indents, tabs added/moved/removed, the
  // right and top margins, a table column, a section column), over the ribbon with the ruler on
  ruler: {
    docs: ['f7a__7__0.docx', 'tables.docx', 'sections.docx'],
    steps: [
      { js: 'return null;' },
      { button: 'view:ruler' },
      { js: `el.setSelection(${P(1, 1)}); return null;` },
      { ruler: 'left-indent', d: 567 },
      { ruler: 'first-line', d: 283 },
      { ruler: 'hanging', d: 283 },
      { ruler: 'right-indent', d: -850 },
      { ruler: 'tab-add', d: 0 },
      { ruler: 'tab', d: 567 },
      { ruler: 'tab-off', d: 0 },
      { ruler: 'm-right', d: -567 },
      { ruler: 'm-top', d: 283 },
      { ruler: 'column', d: -283 },
      { key: 'Control+z' },
      // into the first table (the cell drag) when the document has one
      { js: `el.setSelection(${P(0, 0)}); return null;` },
      { seek: 'table' },
      { ruler: 'cell', d: -400 },
      { ruler: 'left-indent', d: 283 },
      { key: 'Control+z' },
    ],
  },
  text: {
    docs: ['hyperlinks.docx', 'fields.docx', 'f2-3-2.docx'],
    steps: [
      { js: 'return [el.getLinks(), el.hasExternalFields(), el.styleKeyOf("Heading1"), el.styleKeyOf("Ttulo1"), el.styleKeyOf("x"), el.validBookmarkName("Ab_1"), el.validBookmarkName("1x"), el.hasAuthor()];' },
      { js: `el.setSelection(${P(0, 0)}); return [el.findNext('e'), el.getSelection(), el.findNext('E'), el.getSelection(), el.findNext('zzzq')];` },
      { js: `return [el.tocEntrySuggestion(), el.countMatches({ text: 'o', matchCase: true }), el.countMatches({ text: '(' , wildcards: true }), el.countMatches({ text: 'e' }, { within: 'selection' })];` },
      { js: `return el.replaceAll('e', 'É');` },
      { key: 'Control+z' },
      { js: `el.toggleTrackChanges(true); const n = el.replaceAll('a', 'ä'); el.toggleTrackChanges(false); return n;` },
      { js: `el.setSelection(${P(0, 1)}); el.insertCustomInline('tag', { name: 'cli' }); return el.getCustomInlines();` },
      { js: `el.setSelection(${P(0, 0)}, ${P(0, 1)}); el.insertCustomInline('tag', { name: 'x' }, { fallbackText: 'FB' }); return el.getCustomInlines('tag');` },
      // data that is not plain JSON (a Date, an undefined member): the session keeps the JSON text (ED-709)
      { js: `el.setSelection(${P(0, 2)}); el.insertCustomInline('tag', { name: 'd', when: new Date(0), gone: undefined }); return el.getCustomInlines('tag');`, tag: 'inline-json' },
      { key: 'Control+z' },
      { js: `return [el.ensureBuiltinStyle('caption'), el.ensureBuiltinStyle('Hyperlink'), el.ensureBuiltinStyle('table of figures')];` },
      { js: `el.setSelection(${P(0, 0)}); const w = await el.nextManualHyphenation(); return w;` },
      { js: `const w = await el.nextManualHyphenation(); if (w) el.skipManualHyphenation(); return w;` },
      { js: `const w = await el.nextManualHyphenation(); if (w && w.points.length) el.applyManualHyphenation(w.points[w.points.length - 1]); return w;` },
      { js: `el.endManualHyphenation(); return null;` },
      { js: `el.setSelection(${P(0, 0)}); return null;` },
      { button: 'hyph:manual', ok: true },
      { button: 'wordCount' },
      { key: 'Control+Shift+G' },
    ],
  },
  view: {
    docs: ['fields.docx', 'f7a__7__0.docx', 'many-fields.docx', 'sections.docx'],
    steps: [
      // wave 7c (V): the strict replay against the frozen TS (`ed.tsStrict`: the TS rules where Word differs in the views)
      { js: 'if (el.rust) el.rust.tsStrict = true; return null;' },
      { js: 'el.showGridlines(true); return el.gridlinesShown();' },
      { js: 'return el.showBookmarks();' },
      { js: `el.setSelection(${P(0, 0)}); el.insertBookmark('MarcaA1'); el.setSelection(${P(1, 0)}, ${P(1, 3)}); el.insertBookmark('MarcaB1'); return el.getBookmarks().map((b) => b.name);` },
      { js: 'el.toggleMarks(); return null;' },
      { js: `el.setSelection(${P(0, 0)}); el.insertTableOfContents(); el.setSelection(${P(1, 0)}); return el.inTableOfContents();` },
      { js: `const c = document.createElement('canvas'); const ok = el.paintTocTemplate('auto1', c, 300); return [ok, c.width, c.height];` },
      { js: `const c = document.createElement('canvas'); return el.paintTocTemplate('manual', c, 300);` },
      { js: `el.setView('web'); return el.getView();`, tag: 'web' },
      { shot: true, tag: 'web-shot' },
      // wave 7c (V): editing in the continuous view (the model, the selection and the events equal; the geometry is Word's)
      { js: `el.setSelection(${P(0, 0)}); el.focusEditor(); return null;`, tag: 'web' },
      { type: 'ab', tag: 'web' },
      { key: 'Enter', tag: 'web' },
      { key: 'Backspace', tag: 'web' },
      { js: 'el.setZoom(1.5); return el.getZoom();', tag: 'web' },
      { js: 'el.setZoom(1); return el.getZoom();', tag: 'web' },
      // wave 7c (V, fix round 1, V5): the interaction in the continuous view — a real click at a caret point (each engine's
      // own caretRectAt, so the hit test answers the position the caret geometry drew), a Shift+click at the last
      // paragraph (the far strip of a tall sheet), PageDown, End there, and an element resize (the Web view lays out again)
      { js: `window.__clickAt = ${P(1, 2)}; return null;`, v5: true, tag: 'web' },
      { click: true, v5: true, tag: 'web' },
      { js: `const n = el.getParagraphCount ? el.getParagraphCount() : null; el.selectAll(); const e = el.getSelection().focus; window.__clickAt = { para: e.para, offset: 0 }; el.setSelection(${P(1, 2)}); return [n, e.para];`, v5: true, tag: 'web' },
      { click: true, shift: true, v5: true, tag: 'web' },
      { js: `el.setSelection(${P(0, 0)}); el.focusEditor(); return null;`, tag: 'web' },
      { key: 'PageDown', v5: true, tag: 'web-pgdn' },
      { key: 'Control+End', v5: true, tag: 'web' },
      { js: `el.style.width = '800px'; await new Promise((r) => setTimeout(r, 400)); return null;`, v5: true, tag: 'web' },
      { js: `el.style.width = '1100px'; await new Promise((r) => setTimeout(r, 400)); return null;`, v5: true, tag: 'web' },
      { js: `el.setView('draft'); return el.getView();`, tag: 'draft' },
      { js: `window.__clickAt = ${P(1, 2)}; return null;`, v5: true, tag: 'draft' },
      { click: true, v5: true, tag: 'draft' },
      { js: `const n = el.getParagraphCount ? el.getParagraphCount() : null; el.selectAll(); const e = el.getSelection().focus; window.__clickAt = { para: e.para, offset: 0 }; el.setSelection(${P(1, 2)}); return [n, e.para];`, v5: true, tag: 'draft' },
      { click: true, shift: true, v5: true, tag: 'draft' },
      { js: `el.setSelection(${P(0, 0)}); el.focusEditor(); return null;`, v5: true, tag: 'draft' },
      { key: 'PageDown', v5: true, tag: 'draft-pgdn' },
      { key: 'Control+End', v5: true, tag: 'draft' },
      { js: `el.style.width = '800px'; await new Promise((r) => setTimeout(r, 400)); return null;`, v5: true, tag: 'draft' },
      { js: `el.style.width = '1100px'; await new Promise((r) => setTimeout(r, 400)); return null;`, v5: true, tag: 'draft' },
      // (wave 7c, W fix round 1: the ruler's default tab stop is the `defTab` facet, compared on its own)
      { js: `el.setView('outline'); const r = el.getRulerInfo(); if (r) delete r.defaultTabTwips; return [el.getView(), r];`, tag: 'outline' },
      { js: `window.__clickAt = ${P(1, 2)}; return null;`, v5: true, tag: 'outline' },
      { click: true, v5: true, tag: 'outline' },
      { js: `const n = el.getParagraphCount ? el.getParagraphCount() : null; el.selectAll(); const e = el.getSelection().focus; window.__clickAt = { para: e.para, offset: 0 }; el.setSelection(${P(1, 2)}); return [n, e.para];`, v5: true, tag: 'outline' },
      { click: true, shift: true, v5: true, tag: 'outline' },
      { js: `el.setSelection(${P(0, 0)}); el.focusEditor(); return null;`, v5: true, tag: 'outline' },
      { key: 'PageDown', v5: true, tag: 'outline-pgdn' },
      { key: 'Control+End', v5: true, tag: 'outline' },
      { js: `el.style.width = '800px'; await new Promise((r) => setTimeout(r, 400)); return null;`, v5: true, tag: 'outline' },
      { js: `el.style.width = '1100px'; await new Promise((r) => setTimeout(r, 400)); return null;`, v5: true, tag: 'outline' },
      { js: `el.setView('print'); return el.getView();` },
      { button: 'view:gridlines' },
      { button: 'view:bookmarks' },
      { button: 'view:ruler' },
      { js: `el.setSelection(${P(0, 0)}); el.insertCustomInline('tag', { name: 'pinta' }); return null;` },
      // the view commands of the bar (the Web/Draft/Outline steps: ED-707/708, as the setView ones above)
      { button: 'view:web', tag: 'web' },
      { button: 'view:draft', tag: 'draft' },
      { button: 'view:outline', tag: 'outline' },
      { button: 'view:print' },
    ],
  },
  // the compact bar (the commands that live only in its menus) and the File commands that end in the host: export, print,
  // compare (the Rust diffDocuments under engine="rust", D-A1-3), fonts in the file, word count, font access, mail merge
  // (hidden in both: no such methods), Enable Editing on a protected load
  bar: {
    docs: ['f7a__7__0.docx', 'hyperlinks.docx'],
    toolbar: 'compact',
    other: 'styles.docx',
    steps: [
      { js: 'return null;' },
      { js: `el.setSelection(${P(0, 1)}); return null;` },
      { button: 'borders' },
      { button: 'borders' },
      { button: 'dlg:tabs', ok: true },
      { button: 'dlg:borders', ok: true },
      { button: 'export:html' },
      { button: 'export:md' },
      { button: 'export:odt' },
      { button: 'export:xps' },
      { button: 'print' },
      // the pages host.print received, once both engines printed (no time cap race: 120 s, then an explicit failure)
      { js: 'for (let i = 0; !window.__printed; i++) { if (i > 2400) throw new Error("host.print not called in 120 s"); await new Promise((r) => setTimeout(r, 50)); } window.__printedTaken = window.__printed; window.__printed = null; return null;' },
      { button: 'fontsInFile', ok: true },
      { button: 'wordCount' },
      { button: 'fontAccess' },
      { button: 'mailmerge' },
      { button: 'mergefield' },
      { button: 'compare' },
      { js: `for (let i = 0; !el.shadowRoot.querySelector('.uw-compare universal-write-diff')?.stats; i++) { if (i > 2400) throw new Error('compare not ready in 120 s'); await new Promise((r) => setTimeout(r, 50)); } return el.classList.contains('uw-comparing');` },
      { js: `el.shadowRoot.querySelector('.uw-compare universal-write-diff')?.dispatchEvent(new CustomEvent('close-request', { bubbles: true, composed: true })); return el.classList.contains('uw-comparing');` },
      { js: `el.loadDocx(new Uint8Array(await (await fetch('/docs/' + window.__doc)).arrayBuffer()), { protected: true }); await es.settle(); return el.isProtected();` },
      { button: 'enableEditing' },
      { js: `return [el.isProtected(), el.layoutPageCount()];` },
    ],
  },
};

/**
 * Declared differences (A1's own table, D-A1-10): `doc|family` → { tags (the steps' `tag`), facets, end, id, why }.
 * Everything else fails.
 */
/**
 * wave 7c (V, fix round 2): ED-707/708. The Web, Draft and Outline views are ONE sheet with the TS presentation (the sheet
 * width and place, the 600-twip margin, the ruler frame read from the element's width, the Web view laid out again at a
 * selection change after a resize, the logical page: the `present`, `ruler` and `pages` facets, strict, with
 * `ed.tsStrict`). The line geometry inside it is the ENGINE's: the frozen TS legacy canvas layout of these views is not
 * ported (LQ2, uo-docx-layout.md "Not ported (G19, LQ2)"). Declared, per step tag, only the facets that follow from where
 * the lines fall: the focus line's top (`rulerLine`), the sheet and strip heights (`canvases`), the Web screen, and the
 * caret after PageDown (the TS's own rule in both engines — 0.9 of the view and 20 line moves, universal-write.ts 10597 —
 * over each engine's lines). Evidence of the engine's geometry: uo-docx-layout tests/views_w7c.rs
 * `continuous_views_wrap_and_place_lines_as_word` (every line start of 6 documents in Web, Draft and Outline equal to
 * Word's: 171 lines, the x within 1.5 px, the pitch within 1 px) and `section_break_takes_a_line_in_draft_and_outline`.
 * NOT Word's on these harness documents where they were not measured; on f7a__7__0 the engine differs from Word in Print
 * as well (`f7a_against_word_probe`: 13 of Word's 326 Print lines equal) — a layout item of that generated document, not
 * of the views. Word's own PageDown (`Selection.MoveDown(wdScreen)`, measured) is neither engine's: open (record "Wave 7c —
 * V, fix round 2").
 */
const LQ2_VIEWS = {
  tags: {
    web: ['rulerLine', 'canvases'], draft: ['rulerLine', 'canvases'], outline: ['canvases'], 'web-shot': ['answer', 'rulerLine', 'canvases'],
    'web-pgdn': ['sel', 'selected', 'para', 'run', 'pages', 'rulerLine', 'canvases'],
    'draft-pgdn': ['sel', 'selected', 'para', 'run', 'pages', 'rulerLine', 'canvases'],
    'outline-pgdn': ['sel', 'selected', 'para', 'run', 'pages', 'canvases'],
  },
  end: [],
  id: 'LQ2 (the engine\'s line geometry in the continuous views)',
  why: 'continuous views: the engine\'s line positions (the TS legacy canvas layout not ported, LQ2; equal to Word\'s where measured, views_w7c); the presentation facets are compared',
};
/** wave 7c (W, item 4): Word's error text of a REF/NOTEREF to an undefined bookmark in fields.docx (measured in Word 16, oracle
 *  refbm: "Erro! Fonte de referência não encontrada." / "Erro! Indicador não definido.", bold; the TS the paragraph text) */
const W7C_FIELDS_REF = { from: 5, facets: ['pages'], end: ['end pages', 'screen (A1 overlays on)', 'saved word/document.xml'], id: 'Word manda sobre o TS (item 4, REF error)', why: 'REF/NOTEREF to an undefined bookmark: the bold error text of Word' };
/** wave 7c (W, item 2, fix round 1): the default tab stop of a package whose settings give no w:defaultTabStop (every d3
 *  fixture of this harness; styles.docx has a settings part without it): Word's 708 tw (COM DefaultTabStop 35.4 pt, oracle
 *  nosettings) in getDefaultTabStop / getTabStops / getRulerInfo, the frozen TS model's 720 — the `defTab` facet alone */
const W7C_DEFTAB = { from: 0, facets: ['defTab'], end: [], id: 'Word manda sobre o TS (item 2, default tab stop 708)', why: 'no w:defaultTabStop: Word 708 tw (measured), the TS 720' };
/** wave 7c (W, item 2, fix round 1; reviewer W/2): tables.docx has no settings part, so Word lays it out in mode 12 — the first
 *  cell's text at the margin (x 1701; the TS's mode 15: 1809) — and the ruler line of the caret (steps 0–2, the caret in the
 *  first table) is the cell's frame 1701..3485 against the TS's 1701..10205. Attributed: uo-wasm tests/w7c_item2_native.rs
 *  (the shell's ruler line with ONLY the layout bit 13, ts_missing_settings_mode15, = the TS's answer) and uo-docx-layout
 *  word_w7c.rs item2_attribution_of_the_editor_harness_documents (only that switch gives the TS's layout); the end screen
 *  shows the table placed as Word places it. Word's own ruler over that cell is not measured. */
const W7C_TABLES_RULER = { from: 0, to: 2, facets: ['ruler'], end: ['screen (A1 overlays on)'], id: 'Word manda sobre o TS (item 2, mode 12 table)', why: 'a package without settings: the mode-12 cell geometry (the ruler frame of the caret cell)' };
// WordCraft study — fields (record uo-docx-layout.md "WordCraft study — harness declarations"): f7a__7__0, f8__7__0 and
// f7b__7__0 are moved by C1b (a multiple-spaced line over runs of several faces: Word's text height, a few hundredths of a
// point per line) and f7a__7__0, f8__7__0 by B22 (Word's \* format switches: f7a__7__0's two `PAGE \* ROMAN` fields are
// Word's upper-case roman, the TS's lower-case), each attributed natively (uo-docx-layout attribute_documents_to_the_fields_
// rules, the editor's read too) and by this harness's run with only the fields area at Word's rules (local wasm build; all
// four areas at the TS's: every family and extra equal). The facets below are that run's.
const WC_F7A0 = { id: 'Word manda sobre o TS (WordCraft fields C1b and B22)', why: 'Word\'s text height of multiple-spaced lines over several faces and the PAGE \\* ROMAN results move the lines and pages' };
/** f7a__7__0's two `PAGE \* ROMAN` results (after "coração" ending its paragraph, after "documento"): Word's upper case (B22) */
const wcRoman = (t) => (typeof t === 'string' ? t.replace(/(coração|documento)([ivxlc]+)(?=\s)/g, (_, w, r) => w + r.toUpperCase()) : t);
/** the printed pages of the ED-713 block (f7a__7__0, a heading and the TOC added) that C1b and B22 change */
const WC_ED713_PRINTED = [0, 1, 2, 3, 5, 6];
const DECLARED = {
  'headers|para': W7C_DEFTAB,
  'tables|ruler': [W7C_DEFTAB, W7C_TABLES_RULER],
  'sections|ruler': W7C_DEFTAB,
  'hyperlinks|bar': W7C_DEFTAB,
  // the pages printed by File > Print of f2-3-0 (the `file` family): the run coloured by a theme colour of a document without a
  // theme part — TS C00000 (`w:color`), Rust accent2 of Word's 2023 scheme E97132 (front B's gate-O3, theme_word.rs: Word decides)
  'f2-3-0|file': { end: ['printed pages'], id: 'ED-314 / B gate-O3', why: 'theme colors of a document without a theme part (Word 2023 scheme)' },
  // (the footer's field shading of f2-3-2: ED-304 closed in wave 7c, V — the shell's overlays read every story's lines)
  'f2-3-2|text': { tags: { 'inline-json': ['answer', 'inlines'] }, end: [], id: 'ED-709', why: 'custom inline data as JSON' },
  // custom inline data that is not plain JSON: the session stores the data as JSON text (as the .docx does), the TS element
  // keeps the object until it saves (ED-709)
  'hyperlinks|text': { tags: { 'inline-json': ['answer', 'inlines'] }, end: [], id: 'ED-709', why: 'custom inline data kept as JSON text in the session' },
  'fields|text': [
    { tags: { 'inline-json': ['answer', 'inlines'] }, end: [], id: 'ED-709', why: 'as hyperlinks|text' },
    // wave 7c (W, item 4): fields.docx's REF/NOTEREF to an undefined bookmark is Word's bold error text in the editor's read
    // (uo-docx-layout word_w7c.rs attribute_documents_to_the_word_rules, UO_W7C_ATTR_SOURCES: only ts_ref_bookmarks changes
    // its pages; record uo-bindings.md "Wave 7c — W"): the end screen with the overlays (the field shading over that text)
    W7C_FIELDS_REF,
  ],
  // the glow on screen: the session's text effects scale with the font size (the Word's measured rule, milestone 1); only the
  // capture taken while the glow is on (the outline, preset and gradient captures are compared)
  'f7a__7__0|font': [
    { tags: { 'glow-shot': ['answer'] }, end: [], id: 'ED-710', why: 'text glow scaled by the font size (Word rule, uo-render fx_for_word)' },
    { ...WC_F7A0, from: 9, facets: ['answer'], end: ['screen (A1 overlays on)'] },
  ],
  'f7a__7__0|para': [{ ...WC_F7A0, from: 4, facets: ['ruler'], end: ['screen (A1 overlays on)'] }, { ...WC_F7A0, from: 32, facets: ['answer'], end: [] }],
  'f7a__7__0|ruler': { ...WC_F7A0, from: 2, facets: ['ruler'], end: ['screen (A1 overlays on)'] },
  'f7a__7__0|bar': { ...WC_F7A0, from: 11, facets: ['printed'], end: ['screen (A1 overlays on)'] },
  'f8__7__0|file': { end: ['printed pages'], id: 'Word manda sobre o TS (WordCraft fields C1b and B22)', why: 'the printed pages: Word\'s text height of multiple-spaced lines over several faces, Word\'s \\* switch results' },
  'f7b__7__0|file': { end: ['printed pages'], id: 'Word manda sobre o TS (WordCraft fields C1b)', why: 'the printed pages: Word\'s text height of multiple-spaced lines over several faces' },
  'styles|font': { tags: { 'glow-shot': ['answer'] }, end: [], id: 'ED-710', why: 'as f7a__7__0|font' },
  // Web/Draft/Outline: the TS draws them with the legacy continuous layout, not ported (LQ2); the session lays them out with the
  // engine: the line geometry's facets of those steps (LQ2_VIEWS above)
  // wave 7c (W, item 4): the TOC inserted at step 5 lists the headings with their REF/NOTEREF results — Word's error text, longer
  // than the TS's start-paragraph text (the TOC statistics of every later step, the end pages and screen)
  // (the V5 Shift+click selects across the TOC: the selected text holds the same REF/NOTEREF results)
  'fields|view': [LQ2_VIEWS, { ...W7C_FIELDS_REF, facets: ['pages', 'selected'] }, W7C_DEFTAB],
  'f7a__7__0|view': [LQ2_VIEWS, { ...WC_F7A0, from: 3, facets: ['rulerLine', 'selected', 'canvases'], end: ['screen (A1 overlays on)', 'saved word/document.xml'] }],
  'many-fields|view': [LQ2_VIEWS, W7C_DEFTAB],
  'sections|view': [LQ2_VIEWS, W7C_DEFTAB],
};
const declared = (key, facet, step, at) => {
  const d = DECLARED[key] ?? DECLARED[`${key.split('|')[0]}|*`];
  if (!d) return null;
  // wave 7c (W): a key may hold several declarations (an array); `from` + `facets` declare those step facets from that step
  // on whatever the tag (the Word-authority entries of W's layout rules)
  if (Array.isArray(d)) return d.map((x) => declaredOne(x, facet, step, at)).find(Boolean) ?? null;
  return declaredOne(d, facet, step, at);
};
const declaredOne = (d, facet, step, at) => {
  if (step === undefined) return d.end === '*' || (d.end ?? []).includes(facet) ? d : null;
  // (W fix round 1: `to` bounds the steps of a `from` declaration)
  if (d.from !== undefined && at !== undefined && at >= d.from && (d.to === undefined || at <= d.to) && (d.facets === '*' || (d.facets ?? []).includes(facet))) return d;
  const tag = step?.tag;
  return tag && d.tags?.[tag]?.includes(facet) ? d : null;
};
const parts = (b64) => {
  const z = unzipSync(new Uint8Array(Buffer.from(b64, 'base64')));
  return Object.fromEntries(Object.entries(z).map(([k, v]) => [k, new TextDecoder().decode(v)]));
};

async function runFamily(fam) {
  const s = SCRIPTS[fam];
  const tally = { same: 0, diff: 0, declared: 0 };
  const used = new Set();
  for (const want of s.docs) {
    const n = docOf(want);
    const key = `${want.replace(/\.docx$/, '')}|${fam}`;
    const runs = {};
    for (const engine of ['ts', 'rust']) {
      await env.openPage();
      await env.page.evaluate(helpers);
      const errs0 = env.errors.length;
      if (process.env.UO_EF_DUMP) await ev(() => { window.__dump = true; });
      await ev(([t, o, d]) => { window.__toolbar = t; window.__other = o; window.__doc = d; }, [s.toolbar ?? null, s.other ? docOf(s.other) : null, n]);
      await ev((e) => window.__es.make(e), engine);
      const pages = await ev((x) => window.__es.load(x), n);
      const steps = [];
      for (const st of s.steps) {
        if (VERBOSE) console.log(`  ${key} ${engine} step ${JSON.stringify(st)}`);
        let answer = null;
        if (st.js) answer = await ev((c) => window.__es.js(c), st.js);
        else if (st.type) { await ev(() => window.__es.focus()); await env.page.keyboard.type(st.type); await ev(() => window.__es.settle(150)); }
        else if (st.key) { await ev(() => window.__es.focus()); await env.page.keyboard.press(st.key); await ev(() => window.__es.settle(150)); const dl = await ev(() => window.__es.dialogs()); if (dl.length) answer = `dialogs ${JSON.stringify(dl)}`; await ev(() => window.__es.okDialogs()); }
        else if (st.click) {
          const at = await ev(() => window.__es.clickPoint());
          // wave 7c (V, fix round 1): `shift: true` extends the selection (Shift+click)
          if (at && st.shift) { await env.page.keyboard.down('Shift'); await env.page.mouse.click(at.x, at.y); await env.page.keyboard.up('Shift'); }
          else if (at) await env.page.mouse.click(at.x, at.y);
          await ev(() => window.__es.settle(150));
          answer = at ? 'clicked' : 'no point';
        } else if (st.button) {
          answer = await ev((id) => window.__es.clickButton(id), st.button);
          const dl = await ev(() => window.__es.dialogs());
          if (dl.length) answer += ` dialogs ${JSON.stringify(dl)}`;
          // real input in the dialog before OK
          if (st.fill) answer += ` fill ${await ev((c) => window.__es.fill(c), st.fill)}`;
          await ev(() => window.__es.okDialogs());
        } else if (st.shot) {
          answer = await ev(() => window.__es.shot());
        } else if (st.ruler) {
          // a real mouse drag on the ruler (pointer events: down, moves, up)
          await ev(() => window.__es.settle(100));
          const p = await ev(([k, d]) => window.__es.rulerDrag(k, d), [st.ruler, st.d ?? 0]);
          if (p) {
            await env.page.mouse.move(p.from.x, p.from.y);
            await env.page.mouse.down();
            for (let k = 1; k <= 4; k++) await env.page.mouse.move(p.from.x + ((p.to.x - p.from.x) * k) / 4, p.from.y + ((p.to.y - p.from.y) * k) / 4);
            await env.page.mouse.up();
            await ev(() => window.__es.settle(150));
          }
          answer = p ? 'dragged' : 'no marker';
        } else if (st.seek === 'table') {
          // the caret walked down with the keyboard into the first table (ArrowDown, at most 60)
          await ev(() => window.__es.focus());
          let k = 0;
          for (; k < 60 && !(await ev(() => !!window.__es.el.getTableState())); k++) { await env.page.keyboard.press('ArrowDown'); await ev(() => window.__es.settle(60)); }
          answer = k < 60 ? `table after ${k}` : 'no table';
        }
        steps.push({ answer, facets: JSON.parse(await ev((f) => window.__es.facets(f), fam)) });
      }
      const end = await ev(() => window.__es.end());
      runs[engine] = { pages, steps, end, errors: env.errors.length - errs0, printedUrls: process.env.UO_EF_DUMP ? await ev(() => window.__printedUrls ?? []) : [] };
      await ev(() => window.__es.remove());
    }
    const a = runs.ts, b = runs.rust;
    // the bar steps' answers (clicked / disabled / missing, the dialogs they opened): printed once per document
    console.log(`  ${key} bar steps: ${s.steps.map((st, i) => (st.button || st.key ? `${st.button ?? st.key}=${String(b.steps[i].answer).slice(0, 60)}` : null)).filter(Boolean).join('; ')}`);
    if (process.env.UO_EF_ANSWERS) s.steps.forEach((st, i) => console.log(`  ${key} @${i} ts ${String(a.steps[i].answer).slice(0, 160)} | rust ${String(b.steps[i].answer).slice(0, 160)}`));
    const diffs = [];
    const check = (facet, x, y, step) => {
      const same = JSON.stringify(x) === JSON.stringify(y);
      if (same) { tally.same++; return; }
      tally.diff++;
      if (declared(key, facet, step === undefined ? undefined : s.steps[step], step)) { tally.declared++; used.add(key); return; }
      diffs.push({ facet, step, ts: x, rust: y });
    };
    if (a.errors || b.errors) fail(`${key}: page errors ts ${a.errors}, rust ${b.errors}`);
    // checks no declaration covers: the page canvases keep the full buffer scale (no blurred, upscaled page: the continuous
    // views' sheet ends at the content), and in the continuous views the Rust sheets are not taller than the TS strip + 15 % + 40 px (the 633,600-twip sheet would be)
    const cssH = (cv) => cv.filter((c) => c[0] !== 'strip').reduce((t, c) => t + parseFloat(c[1]), 0);
    b.steps.forEach((sb, i) => {
      const cv = sb.facets.canvases;
      if (!cv) return;
      for (const c of cv) if (c[0] !== 'strip' && c[2] > 1 && Math.abs(c[2] / parseFloat(c[0]) - 1) > 0.01) { fail(`${key}: @${i} canvas buffer ${c[2]}x${c[3]} for CSS ${c[0]}x${c[1]} (buffer scale dropped)`); break; }
      // the strips of a tall sheet (fix round 2) at the full scale too, and none above 4000 CSS px
      for (const c of cv.filter((x) => x[0] === 'strip').map((x) => x.slice(1))) if (Math.abs(c[2] / parseFloat(c[0]) - 1) > 0.01 || Math.abs(c[3] / parseFloat(c[1]) - 1) > 0.01 || parseFloat(c[1]) > 4000) { fail(`${key}: @${i} strip buffer ${c[2]}x${c[3]} for CSS ${c[0]}x${c[1]}`); break; }
      const tag = s.steps[i].tag;
      if (sb.facets.stripCheck?.length) fail(`${key}: @${i} the strips of the sheet: ${sb.facets.stripCheck.join(', ')}`);
      const ta = a.steps[i].facets.canvases;
      if (ta && (tag === 'web' || tag === 'draft' || tag === 'outline') && cssH(cv) > cssH(ta) * 1.15 + 40) fail(`${key}: @${i} ${tag} sheet ${cssH(cv)} px tall, the TS strip ${cssH(ta)} px`);
    });
    if (fam === 'bar' && !b.end.rustDiffCalls) fail(`${key}: the bar's Compare did not run the Rust diffDocuments under engine="rust"`);
    if (fam === 'bar' && a.end.rustDiffCalls) fail(`${key}: the TS element ran the Rust diff`);
    if (process.env.UO_EF_DUMP) for (const [e, r] of Object.entries(runs)) (r.end.midShots ?? []).forEach((sh, k) => sh.forEach((u, i) => { if (u) writeFileSync(join(outDir, `${key.replace(/[|]/g, '_')}-${e}-shot${k}-p${i}.png`), Buffer.from(u.split(',')[1], 'base64')); }));
    check('pages after load', a.pages, b.pages);
    a.steps.forEach((sa, i) => {
      const sb = b.steps[i];
      check('answer', sa.answer, sb.answer, i);
      for (const k of new Set([...Object.keys(sa.facets), ...Object.keys(sb.facets)])) check(k, sa.facets[k], sb.facets[k], i);
    });
    if (process.env.UO_EF_DUMP) for (const [e, r] of Object.entries(runs)) (r.printedUrls ?? []).forEach((u, i) => writeFileSync(join(outDir, `${key.replace(/[|]/g, '_')}-${e}-print${i}.png`), Buffer.from(u.split(',')[1], 'base64')));
    if (process.env.UO_EF_DUMP) for (const [e, r] of Object.entries(runs)) r.end.shots.forEach((u, i) => { if (u) writeFileSync(join(outDir, `${key.replace(/[|]/g, '_')}-${e}-p${i}.png`), Buffer.from(u.split(',')[1], 'base64')); });
    check('end pages', a.end.pages, b.end.pages);
    check('screen (A1 overlays on)', a.end.screen, b.end.screen);
    const pa = parts(a.end.docx), pb = parts(b.end.docx);
    check('saved part names', Object.keys(pa).sort(), Object.keys(pb).sort());
    for (const p of Object.keys(pa)) if (p in pb) check(`saved ${p}`, pa[p], pb[p]);
    console.log(`${key}: ${s.steps.length} steps; ${diffs.length} undeclared difference(s)`);
    for (const d of diffs.slice(0, 14)) {
      const cut = (v) => { const t = typeof v === 'string' ? v : JSON.stringify(v); return t && t.length > 600 ? `${t.slice(0, 600)}…` : t; };
      let [x, y] = [d.ts, d.rust];
      if (typeof x === 'string' && typeof y === 'string' && x.length > 200) { let i = 0; while (i < x.length && x[i] === y[i]) i++; x = `@${i} …${x.slice(Math.max(0, i - 80), i + 200)}`; y = `@${i} …${y.slice(Math.max(0, i - 80), i + 200)}`; }
      if (x && y && typeof x === 'object' && typeof y === 'object' && !Array.isArray(x)) {
        const ks = [...new Set([...Object.keys(x), ...Object.keys(y)])].filter((k) => JSON.stringify(x[k]) !== JSON.stringify(y[k]));
        x = Object.fromEntries(ks.map((k) => [k, x[k]])); y = Object.fromEntries(ks.map((k) => [k, y[k]]));
      }
      console.log(`  DIFF ${d.facet}${d.step !== undefined ? ` @${d.step} ${JSON.stringify(s.steps[d.step])}` : ''}\n    ts   ${cut(x)}\n    rust ${cut(y)}`);
    }
    if (diffs.length) fail(`${key}: ${diffs.length} undeclared difference(s)`);
  }
  console.log(`${fam}: facets ${tally.same} equal, ${tally.diff} different (${tally.declared} declared: ${[...used].join(', ') || 'none'})`);
}

/** the File family: the exports compared, the TS .odt reopened in both engines */
async function runFile() {
  // fix round 2: f7b__7__0 (shapes) and pictures.docx (pictures): the XPS Path, ImageBrush and Opacity facets
  for (const want of ['f2-3-0.docx', 'hyperlinks.docx', 'f8__7__0.docx', 'f7b__7__0.docx', 'pictures.docx']) {
    const n = docOf(want);
    const out = {};
    for (const engine of ['ts', 'rust']) {
      await env.openPage();
      await env.page.evaluate(helpers);
      const errs0 = env.errors.length;
      await ev((e) => window.__es.make(e), engine);
      await ev((x) => window.__es.load(x), n);
      const x = await ev(() => window.__es.exports());
      out[engine] = { x, errors: env.errors.length - errs0 };
      await ev(() => window.__es.remove());
    }
    const a = out.ts.x, b = out.rust.x;
    const same = (p, q) => JSON.stringify(p) === JSON.stringify(q);
    const res = [];
    const cmp = (name, p, q) => {
      const ok = same(p, q);
      const dk = declared(`${want.replace(/.docx$/, '')}|file`, name);
      res.push(`${name} ${ok ? 'equal' : dk ? `different (declared ${dk.id})` : 'DIFFERENT'}`);
      if (!ok && !dk) {
        if (typeof p === 'string' && typeof q === 'string') { let i = 0; while (i < p.length && p[i] === q[i]) i++; console.log(`  ${want} ${name} @${i} of ${p.length}/${q.length}\n    ts   …${p.slice(Math.max(0, i - 120), i + 200)}\n    rust …${q.slice(Math.max(0, i - 120), i + 200)}`); }
        else console.log(`  ${want} ${name}\n    ts   ${JSON.stringify(p).slice(0, 400)}\n    rust ${JSON.stringify(q).slice(0, 400)}`);
        fail(`${want}: ${name} differs`);
      }
    };
    cmp('html', a.html, b.html);
    cmp('html (images omitted)', a.htmlOmit, b.htmlOmit);
    cmp('markdown', a.md, b.md);
    cmp('save(markdown)', a.saveMd, b.saveMd);
    for (const [name, k] of [['odt', 'odt'], ['save(odt)', 'saveOdt']]) {
      if (a[k].startsWith('THROW') || b[k].startsWith('THROW')) { cmp(name, a[k], b[k]); continue; }
      const pa = parts(a[k]), pb = parts(b[k]);
      cmp(`${name} part names`, Object.keys(pa), Object.keys(pb));
      for (const p of Object.keys(pa)) if (p in pb) cmp(`${name} ${p}`, pa[p], pb[p]);
    }
    // pictures.docx: its 70-byte truncated PNG, which the browser refuses, is decoded by the engine in both elements (the TS
    // editor's decodeRaster fallback; wave 7 F2: `RustHost.rasterFallback`, uo-image through `imageRgba`), so it is printed
    cmp('printed pages', a.print, b.print);
    if (want === 'pictures.docx') {
      // (F2 round 2: the states must be there — the 3 pictures at least — before they are compared)
      const st = b.imageStates;
      if (!Array.isArray(st) || st.length < 3) fail(`pictures.docx: the Rust image states are missing (${JSON.stringify(st)})`);
      else cmp('picture states (rust: every image decoded)', st.map(() => 'ready'), st);
    }
    // XPS (ED-704): the TS writes its legacy layout; the Rust package is checked for its structure and its text
    if (b.xps.startsWith('THROW')) fail(`${want}: exportXps threw ${b.xps}`);
    else {
      const px = unzipSync(new Uint8Array(Buffer.from(b.xps, 'base64')));
      const names = Object.keys(px);
      const pagesX = names.filter((p) => /\.fpage$/.test(p));
      const text = pagesX.map((p) => [...new TextDecoder().decode(px[p]).matchAll(/UnicodeString="([^"]*)"/g)].map((m) => m[1]).join('')).join('|');
      const ok = names.includes('[Content_Types].xml') && pagesX.length > 0 && names.some((p) => /Resources\/Fonts\//.test(p));
      res.push(`xps ${pagesX.length} page(s), ${names.length} parts, ${text.length} chars ${ok ? 'well-formed' : 'MALFORMED'}`);
      if (!ok) fail(`${want}: XPS structure`);
      // each page's text: the page's UnicodeString runs, in order, are the text of every glyph run the session's layout
      // painted on that page (no run dropped, none moved: the shown field results, list numbers and header/footer included);
      // whitespace, controls, soft hyphens and U+FFFC dropped on both sides (a run of spaces only is not written)
      const unxml = (t) => t.replace(/&lt;/g, '<').replace(/&gt;/g, '>').replace(/&quot;/g, '"').replace(/&apos;/g, "'").replace(/&amp;/g, '&');
      const squeeze = (t) => [...t].filter((ch) => !/[\s\u0000-\u001f\u00ad\ufffc\u200b]/u.test(ch)).join('');
      const order = pagesX.slice().sort((p, q) => Number(/(\d+)\.fpage$/.exec(p)?.[1]) - Number(/(\d+)\.fpage$/.exec(q)?.[1]));
      const xpsPages = order.map((p) => squeeze(unxml([...new TextDecoder().decode(px[p]).matchAll(/UnicodeString="([^"]*)"/g)].map((m) => m[1]).join(''))));
      const sp = (b.pageText ?? []).map(squeeze);
      if (sp.length !== xpsPages.length) fail(`${want}: XPS ${xpsPages.length} page(s), the session ${sp.length}`);
      const bad = sp.map((t, i) => (t === xpsPages[i] ? -1 : i)).filter((i) => i >= 0);
      const chars = sp.reduce((n, t) => n + [...t].length, 0);
      res.push(`xps text: ${sp.length - bad.length}/${sp.length} page(s) equal to the session's painted text (${chars} chars)`);
      for (const i of bad.slice(0, 2)) { const x = sp[i], y = xpsPages[i] ?? ''; let k = 0; while (k < x.length && x[k] === y[k]) k++; console.log(`  ${want} xps page ${i} @${k}\n    session \u2026${x.slice(Math.max(0, k - 60), k + 120)}\n    xps     \u2026${y.slice(Math.max(0, k - 60), k + 120)}`); }
      if (bad.length) fail(`${want}: XPS text differs from the session's painted text on ${bad.length} page(s)`);
      // fix round 2: per page, the Glyphs elements (= the runs written), the ImageBrush (>= the pictures) and Opacity (>= the alphas)
      const counts = order.map((p) => { const x = new TextDecoder().decode(px[p]); return { glyphs: (x.match(/<Glyphs /g) ?? []).length, images: (x.match(/<ImageBrush /g) ?? []).length, alpha: (x.match(/ Opacity="/g) ?? []).length, paths: (x.match(/<Path[ >]/g) ?? []).length }; });
      const want2 = b.pageCounts ?? [];
      const badC = want2.map((w, i) => { const c = counts[i]; return c && c.glyphs === w.glyphs && c.images >= w.images && c.alpha >= w.alpha ? -1 : i; }).filter((i) => i >= 0);
      res.push(`xps elements: ${counts.length - badC.length}/${counts.length} page(s) as the display list (Glyphs ${counts.reduce((n, c) => n + c.glyphs, 0)}, Path ${counts.reduce((n, c) => n + c.paths, 0)}, ImageBrush ${counts.reduce((n, c) => n + c.images, 0)}, Opacity ${counts.reduce((n, c) => n + c.alpha, 0)})`);
      for (const i of badC.slice(0, 2)) console.log(`  ${want} xps page ${i}: ${JSON.stringify(counts[i])}, the display list ${JSON.stringify(want2[i])}`);
      if (badC.length) fail(`${want}: XPS elements differ from the display list on ${badC.length} page(s)`);
    }
    if (out.ts.errors || out.rust.errors) fail(`${want}: page errors ts ${out.ts.errors}, rust ${out.rust.errors}`);
    console.log(`file ${want}: ${res.join('; ')}`);
    // the TS .odt reopened in both engines (loadOdt; load('auto') sniffs it)
    if (!a.odt.startsWith('THROW')) {
      const reopen = {};
      for (const engine of ['ts', 'rust']) {
        await env.openPage();
        await env.page.evaluate(helpers);
        const errs0 = env.errors.length;
        await ev((e) => window.__es.make(e), engine);
        const pages = await ev(([x, o]) => window.__es.load(x, o), [n, a.odt]);
        const facets = await ev(() => { const el = window.__es.el; el.selectAll(); const t = el.selectedText(); el.setSelection({ para: 0, offset: 0 }); return { t, styles: el.currentStyles(), pages: el.layoutPageCount() }; });
        // fix round 2: the .odt has no editable style sheet (stylesEditable(), universal-write.ts 4582): the ODF's styles listed
        // (4156), styleFormatting null (4176), the style commands refused, a built-in applied without defining it; then saved
        const styles = await ev(async () => {
          const el = window.__es.el, es = window.__es;
          const ids = (l) => l.map((x) => x.id);
          const o = {};
          const t = (k, f) => { try { o[k] = f(); } catch (e) { o[k] = `THROW ${e.message}`; } };
          t('all', () => el.listStyles('all').map((x) => [x.id, x.name, x.rawName, x.kind, x.builtIn, x.defined, x.quick, x.hidden, x.inUse]));
          t('recommended', () => ids(el.listStyles()));
          t('gallery', () => ids(el.listStyles('gallery')));
          t('inUse', () => ids(el.listStyles('inUse')));
          t('document', () => ids(el.listStyles('document')));
          t('paragraphStyles', () => el.paragraphStyles());
          const first = el.listStyles('all')[0]?.id ?? 'Normal';
          t('fmtNormal', () => el.styleFormatting('Normal'));
          t('fmtFirst', () => el.styleFormatting(first));
          t('create', () => el.createStyle({ name: 'Meu Estilo' }));
          t('rename', () => el.renameStyle(first, 'Outro Nome'));
          t('modify', () => el.modifyStyle(first, { format: { bold: true } }));
          t('delete', () => el.deleteStyle(first));
          t('defaultFont', () => el.setDefaultFont({ sizePt: 14 }));
          t('update', () => el.updateStyleFromSelection());
          t('ensure', () => el.ensureBuiltinStyle('heading 2'));
          el.setSelection({ para: 0, offset: 0 });
          t('apply', () => { el.applyStyle('Ttulo1'); return el.currentStyles(); });
          await es.settle();
          t('allAfter', () => ids(el.listStyles('all')));
          const b = await el.save(); let s = ''; for (let i = 0; i < b.length; i += 0x8000) s += String.fromCharCode(...b.subarray(i, i + 0x8000));
          o.saved = btoa(s);
          return o;
        });
        const auto = await ev(async (o) => { const el = window.__es.el; el.load(Uint8Array.from(atob(o), (c) => c.charCodeAt(0))); await window.__es.settle(); return el.layoutPageCount(); }, a.odt);
        const saved = await ev(async () => { const b = await window.__es.el.save(); let s = ''; for (let i = 0; i < b.length; i += 0x8000) s += String.fromCharCode(...b.subarray(i, i + 0x8000)); return btoa(s); });
        reopen[engine] = { pages, facets, styles, auto, saved, errors: env.errors.length - errs0 };
        await ev(() => window.__es.remove());
      }
      const ra = reopen.ts, rb = reopen.rust;
      const st = [];
      // the style facets (fix round 2) one by one, the saved .odt after them part by part
      const sa = { ...ra.styles }, sb = { ...rb.styles };
      const savedA = sa.saved, savedB = sb.saved;
      delete sa.saved; delete sb.saved;
      const badStyles = Object.keys(sa).filter((k) => !same(sa[k], sb[k]));
      st.push(`styles (listStyles all/recommended/gallery/inUse/document, paragraphStyles, styleFormatting, create/rename/modify/delete, setDefaultFont, updateStyleFromSelection, ensureBuiltinStyle, applyStyle of a built-in) ${badStyles.length ? `DIFFERENT: ${badStyles.join(', ')}` : `equal (${(sb.all ?? []).length} ODF styles)`}`);
      if (badStyles.length) { fail(`${want}: reopened .odt style facets ${badStyles.join(', ')}`); for (const k of badStyles.slice(0, 3)) console.log(`    ${k}
      ts   ${JSON.stringify(sa[k]).slice(0, 300)}
      rust ${JSON.stringify(sb[k]).slice(0, 300)}`); }
      const sp1 = parts(savedA), sp2 = parts(savedB);
      const badSaved = [...new Set([...Object.keys(sp1), ...Object.keys(sp2)])].filter((p) => sp1[p] !== sp2[p]);
      st.push(`saved after the style commands: parts different ${badSaved.length}`);
      if (badSaved.length) fail(`${want}: reopened .odt saved after the style commands: ${badSaved.join(', ')}`);
      for (const k of ['pages', 'facets', 'auto']) { const ok = same(ra[k], rb[k]); st.push(`${k} ${ok ? 'equal' : 'DIFFERENT'}`); if (!ok) { fail(`${want}: reopened .odt ${k}`); console.log(`    ts   ${JSON.stringify(ra[k]).slice(0, 400)}\n    rust ${JSON.stringify(rb[k]).slice(0, 400)}`); } }
      // save('auto') of an .odt is an .odt in both
      const pa = parts(ra.saved), pb = parts(rb.saved);
      const diffParts = Object.keys(pa).filter((p) => pa[p] !== pb[p]);
      st.push(`save('auto') parts different ${diffParts.length}`);
      if (diffParts.length) { fail(`${want}: reopened .odt save('auto') parts ${diffParts.join(', ')}`); for (const p of diffParts.slice(0, 2)) { const x = pa[p] ?? '', y = pb[p] ?? ''; let i = 0; while (i < x.length && x[i] === y[i]) i++; console.log(`    ${p} @${i}\n      ts   …${x.slice(Math.max(0, i - 100), i + 200)}\n      rust …${y.slice(Math.max(0, i - 100), i + 200)}`); } }
      if (ra.errors || rb.errors) fail(`${want}: reopened .odt page errors`);
      console.log(`file ${want} (.odt reopened): ${st.join('; ')}`);
    }
  }
}

/**
 * Fix round 2 (the review's minor findings): what no family exercised.
 * - ED-711: the exports while the header is being edited (Rust: the body, as Word; the TS: the edited story — declared);
 * - the TOC control's tab clicked with the real mouse ("Update Table…" and the menu button): the events compared;
 * - File > Print of a page holding a plugin inline: the printed pages compared;
 * - the Format Painter armed, then a Ctrl+click on a link and a click on a floating text box: armed in both (the TS applies
 *   it only on the default text click), then a plain text click applies it;
 * - a section taller than 16384 px at zoom 250 % and DPR 2 in the Web view: the Rust strips keep the full buffer scale.
 */
async function runExtra() {
  const same = (p, q) => JSON.stringify(p) === JSON.stringify(q);
  const both = async (want, body, arg) => {
    const out = {};
    for (const engine of ['ts', 'rust']) {
      await env.openPage();
      await env.page.evaluate(helpers);
      const errs0 = env.errors.length;
      await ev((e) => window.__es.make(e), engine);
      await ev((x) => window.__es.load(x), docOf(want));
      out[engine] = { r: await body(engine, arg), errors: env.errors.length - errs0 };
      await ev(() => window.__es.remove());
    }
    if (out.ts.errors || out.rust.errors) fail(`extra ${want}: page errors ts ${out.ts.errors}, rust ${out.rust.errors}`);
    return [out.ts.r, out.rust.r];
  };

  // ── wave 7c (V, fix round 2): Word's Web view wraps again at once on a resize (the Rust default; Word measured,
  // word-v3.ps1: a window 1100 → 900 px re-wraps with no view or selection change); `tsStrict` = the TS: at the next
  // selection change ──
  {
    const [, r] = await both('f7a__7__0.docx', (engine) => (engine === 'ts' ? null : ev(async () => {
      const el = window.__es.el, es = window.__es;
      const sheet = () => { const cv = el.shadowRoot.querySelector('canvas.page'); return [cv.style.width, el.getRulerInfo()?.frame?.x1]; };
      el.setView('web'); await es.settle();
      const o = { start: sheet() };
      el.style.width = '800px'; await new Promise((x) => setTimeout(x, 300)); await es.settle();
      o.resized = sheet();
      el.style.width = '1100px'; await new Promise((x) => setTimeout(x, 300)); await es.settle();
      o.back = sheet();
      el.rust.tsStrict = true;
      el.style.width = '800px'; await new Promise((x) => setTimeout(x, 300)); await es.settle();
      o.strict = sheet();
      el.setSelection({ para: 0, offset: 1 }); await es.settle();
      o.strictSel = sheet();
      el.style.width = '1100px';
      return o;
    })));
    const want = { start: ['1052px', 15180], resized: ['752px', 10680], back: ['1052px', 15180], strict: ['1052px', 10680], strictSel: ['752px', 10680] };
    if (!same(r, want)) fail(`extra web resize: ${JSON.stringify(r)} (want ${JSON.stringify(want)})`);
    else console.log('extra web resize: the Word default wraps at once, tsStrict at the next selection change');
  }

  // ── ED-711: exports while the header is edited ──
  {
    const [a, b] = await both('headers.docx', () => ev(async () => {
      const el = window.__es.el, es = window.__es;
      const o = {};
      const t = async (k, f) => { try { o[k] = await f(); } catch (e) { o[k] = `THROW ${e.message}`; } };
      await t('bodyHtml', () => el.exportHtml('T'));
      await t('bodyMd', () => el.exportMarkdown('T'));
      await t('bodyOdt', () => el.exportOdt().length);
      el.enterHeaderFooter('header', 0); await es.settle();
      await t('editing', () => el.editingHeaderFooter());
      await t('hfHtml', () => el.exportHtml('T'));
      await t('hfMd', () => el.exportMarkdown('T'));
      await t('hfOdt', () => el.exportOdt().length);
      el.exitHeaderFooter(); await es.settle();
      return o;
    }));
    const rustBody = b.hfHtml === b.bodyHtml && b.hfMd === b.bodyMd && b.hfOdt === b.bodyOdt;
    if (!b.editing) fail('extra ED-711: the header was not entered (Rust)');
    if (!rustBody) fail('extra ED-711: the Rust export from the header is not the body export');
    const tsStory = a.hfHtml !== a.bodyHtml || a.hfMd !== a.bodyMd;
    console.log(`extra headers (ED-711): rust exports the body from the header ${rustBody ? 'yes' : 'NO'}; ts exports the story ${tsStory ? 'yes (declared ED-711)' : 'no (equal)'}; body HTML/Markdown ${same([a.bodyHtml, a.bodyMd], [b.bodyHtml, b.bodyMd]) ? 'equal' : 'DIFFERENT'}`);
    if (!same([a.bodyHtml, a.bodyMd], [b.bodyHtml, b.bodyMd])) {
      fail('extra ED-711: the body exports differ');
      for (const k of ['bodyHtml', 'bodyMd']) { const p = String(a[k]), q = String(b[k]); if (p === q) continue; let i = 0; while (i < p.length && p[i] === q[i]) i++; console.log(`    ${k} @${i} of ${p.length}/${q.length}
      ts   …${p.slice(Math.max(0, i - 120), i + 160)}
      rust …${q.slice(Math.max(0, i - 120), i + 160)}`); }
    }
  }

  // ── the TOC control's tab, clicked ──
  {
    // the tab's place by the TS rule (tocTabGeom, universal-write.ts 7420): 20 CSS px tall, 3 px above the box of the TOC's lines,
    // the menu square first, then "Update Table…"; the box's top-left from the first TOC line's caret rectangle
    const geoOf = () => ev(async () => {
      const el = window.__es.el, es = window.__es;
      // the gallery's TOC (a block sdt "Table of Contents": the control the TS frames, tocSdtRange)
      el.setSelection({ para: 0, offset: 0 }); el.insertTocTemplate('auto1'); await es.settle();
      // wave 7 (F2): the document text right after the insertion (the entries' page numbers: ED-712)
      const k0 = el.getSelection(); el.selectAll(); const inserted = el.selectedText(); el.setSelection(k0.anchor, k0.focus);
      el.scrollTop = 0; await es.settle();
      el.setSelection({ para: 0, offset: 0 });
      const r = el.caretRectAt({ para: 0, offset: 0 });
      el.setSelection({ para: 0, offset: 1 }); await es.settle();
      window.__tocEv = [];
      for (const t of ['uw-toc-update', 'uw-toc-menu']) el.addEventListener(t, (e) => { window.__tocEv.push(t); if (!window.__tocApply) e.preventDefault(); });
      return { inToc: el.inTableOfContents(), r, inserted };
    });
    const tocRun = (which) => both('f7a__7__0.docx', async () => {
      await ev((x) => { window.__tocApply = x; }, which === 'apply');
      const g = await geoOf();
      const o = { inToc: g.inToc, painted: !!g.r, inserted: g.inserted };
      if (o.painted) {
        // "Update Table…" or the menu button (one click per load: after the first, the second never reaches the tab in either engine)
        const ty = g.r.y - 3 - 10;
        await env.page.mouse.click(which !== 'menu' ? g.r.x - 3 + 20 + 30 : g.r.x - 3 + 10, ty);
        await ev(() => window.__es.settle(150));
        o.events = await ev(() => window.__tocEv);
        o.sel = await ev(() => window.__es.el.getSelection());
        // the update let through: the bar's dialog (Update entire table) confirmed, the TOC updated (the document text after it)
        if (which === 'apply') { o.dialogs = await ev(() => window.__es.dialogs()); await ev(() => window.__es.okDialogs()); await ev(() => window.__es.settle(300)); }
        if (which === 'apply') o.text = await ev(() => { const el = window.__es.el; const k = el.getSelection(); el.selectAll(); const t = el.selectedText(); el.setSelection(k.anchor, k.focus); return t; });
      }
      return o;
    });
    const [a1, b1] = await tocRun('update');
    const [a2, b2] = await tocRun('menu');
    const [a3, b3] = await tocRun('apply');
    // wave 7 (F2), ED-712 (Word manda sobre o TS): right after insertTocTemplate('auto1') the TS writes the first entry's page
    // number `vi`, the session `5`; Word (Sumário Automático 1 inserted at the start of f7a__7__0 and updated, measured through
    // COM: PAGEREF = 5, the heading in section 1, arabic) writes 5. The only difference allowed: TS `\tvi` where Rust has `\t5`
    const insDecl = [];
    // WordCraft study (fields B22; record uo-docx-layout.md "WordCraft study — harness declarations"): wcRoman below
    const romanDecl = new Set();
    for (const [k, x] of [['update', a1], ['menu', a2], ['apply', a3]]) for (const f of ['inserted', 'text']) {
      if (typeof x[f] !== 'string') continue;
      const t = wcRoman(x[f]);
      if (t !== x[f]) { romanDecl.add(`${k}.${f}`); x[f] = t; }
    }
    for (const [k, x, y] of [['update', a1, b1], ['menu', a2, b2], ['apply', a3, b3]]) {
      if (x.inserted === y.inserted) continue;
      const ok712 = typeof x.inserted === 'string' && typeof y.inserted === 'string' && x.inserted.replace('\tvi', '\t5') === y.inserted && y.inserted.includes('\t5');
      if (ok712) insDecl.push(k);
      else { fail(`extra TOC: the text after insertTocTemplate('auto1') differs (${k}), not as declared ED-712`); const p = JSON.stringify(x.inserted) ?? '', q = JSON.stringify(y.inserted) ?? ''; let i = 0; while (i < p.length && p[i] === q[i]) i++; console.log(`    ${k}.inserted @${i}\n      ts   …${p.slice(Math.max(0, i - 100), i + 160)}\n      rust …${q.slice(Math.max(0, i - 100), i + 160)}`); }
      x.inserted = y.inserted = null;
    }
    console.log(`extra f7a__7__0 (TOC inserted from the gallery): text ${insDecl.length ? `different as declared ED-712 (TS 'vi', Rust and Word '5') in ${insDecl.join(', ')}` : 'equal'}${romanDecl.size ? `; the PAGE \\* ROMAN results different as declared WordCraft fields B22 (TS lower-case, Rust and Word upper-case) in ${[...romanDecl].join(', ')}` : ''}`);
    if (!romanDecl.size) fail('extra TOC: the declared WordCraft fields B22 (PAGE \\* ROMAN) difference is not there (stale?)');
    const a = { update: a1, menu: a2, apply: a3 }, b = { update: b1, menu: b2, apply: b3 };
    const ok = same(a, b) && b1.painted && same(b1.events, ['uw-toc-update']) && same(b2.events, ['uw-toc-menu']);
    console.log(`extra f7a__7__0 (TOC tab clicked): ts ${JSON.stringify([a1.events, a2.events])} rust ${JSON.stringify([b1.events, b2.events])} ${ok ? 'equal' : 'DIFFERENT'}`);
    if (!ok) {
      fail('extra TOC tab: the events or the update differ (or no tab painted)');
      for (const k of ['update', 'menu', 'apply']) for (const f2 of ['events', 'sel', 'text']) {
        const p = JSON.stringify(a[k][f2]) ?? '', q = JSON.stringify(b[k][f2]) ?? '';
        if (p === q) continue;
        let i = 0; while (i < p.length && p[i] === q[i]) i++;
        console.log(`    ${k}.${f2} @${i}\n      ts   …${p.slice(Math.max(0, i - 100), i + 160)}\n      rust …${q.slice(Math.max(0, i - 100), i + 160)}`);
      }
    }
  }

  // ── ED-713 (F2 round 2, Word manda sobre o TS): a TOC entry whose heading is in f7a__7__0's lowerRoman section ──
  {
    // the session's paragraph 31 (Word's 28) made Heading 1 ("Titulo1"), the gallery's Automatic Table 1 at the start, then
    // updateToc('all'). Word (toc-roman-probe.ps1 -Para 28, invisible COM under word-lock: core/target/m2-wsession/f2-toc/
    // result-roman-28.txt) writes PAGEREF 5 and viii (the heading on page 8 of the section numbered lowerRoman, continuing).
    // Measured here: after updateToc('all') both engines write 5 and viii (the PAGEREF results come from the layout's bookmark
    // pages, `refreshFieldCache`, which format by the page's section); right after the insertion the TS writes `v` for the
    // first entry (ED-712, `vi` on the unchanged document). The insertion text is compared with that one replacement, the
    // text after the update, the printed pages and the dirty flag as they are.
    const [a, b] = await both('f7a__7__0.docx', () => ev(async () => {
      const el = window.__es.el, es = window.__es;
      const text = () => { const k = el.getSelection(); el.selectAll(); const t = el.selectedText(); el.setSelection(k.anchor, k.focus); return t; };
      el.setSelection({ para: 31, offset: 0 }); el.applyStyle('Titulo1'); await es.settle();
      el.setSelection({ para: 0, offset: 0 }); el.insertTocTemplate('auto1'); await es.settle();
      const inserted = text();
      el.setSelection({ para: 0, offset: 1 }); const ok = el.updateToc('all'); await es.settle(300);
      const updated = text();
      const pages = await el.printPages();
      const print = await Promise.all(pages.map(async (u) => {
        const img = new Image(); img.src = u; await img.decode();
        const c = document.createElement('canvas'); c.width = img.width; c.height = img.height;
        const x = c.getContext('2d'); x.drawImage(img, 0, 0);
        const d = x.getImageData(0, 0, c.width, c.height).data;
        let h = 0x811c9dc5; for (let i = 0; i < d.length; i++) h = Math.imul(h ^ d[i], 0x01000193);
        return `${img.width}x${img.height}:${(h >>> 0).toString(16)}`;
      }));
      return { inserted, updated, ok, print, dirty: el.isDirty() };
    }));
    const entry = (t) => (typeof t === 'string' ? (t.match(/\t[0-9ivxlcdm-]+(?=[\r\n]|$)/g) ?? []) : []);
    const decl = (t) => (typeof t === 'string' ? t.replace(/\t(v|vi)(?=[\r\n]|$)/, '\t5') : t);
    // WordCraft study (record uo-docx-layout.md "WordCraft study — harness declarations"): the two PAGE \* ROMAN results
    // (fields B22, wcRoman) in the texts, and the printed pages C1b and B22 move (WC_ED713_PRINTED; every other page equal)
    const romanWas = [a.inserted, a.updated];
    a.inserted = wcRoman(a.inserted); a.updated = wcRoman(a.updated);
    if (a.inserted === romanWas[0] && a.updated === romanWas[1]) fail('extra ED-713: the declared WordCraft fields B22 (PAGE \\* ROMAN) difference is not there (stale?)');
    const okIns = a.inserted === b.inserted || (decl(a.inserted) === b.inserted && entry(b.inserted)[0] === '\t5');
    const okUpd = a.updated === b.updated;
    const okWord = JSON.stringify(entry(b.updated).slice(0, 2)) === JSON.stringify(['\t5', '\tviii']);
    const printOff = Array.isArray(a.print) && Array.isArray(b.print) && a.print.length === b.print.length ? a.print.map((x, i) => (x === b.print[i] ? null : i)).filter((i) => i !== null) : null;
    const okRest = same([a.ok, a.dirty], [b.ok, b.dirty]) && same(printOff, WC_ED713_PRINTED);
    if (printOff) console.log(`extra f7a__7__0 (ED-713) printed pages: ${a.print.length}, different ${JSON.stringify(printOff)} (declared WordCraft fields C1b and B22: ${JSON.stringify(WC_ED713_PRINTED)})`);
    console.log(`extra f7a__7__0 (TOC entry in the lowerRoman section): entries after the insertion ts ${JSON.stringify(entry(a.inserted))} rust ${JSON.stringify(entry(b.inserted))}, after updateToc('all') ts ${JSON.stringify(entry(a.updated))} rust ${JSON.stringify(entry(b.updated))} (Word ["\\t5","\\tviii"]); text after the insertion ${a.inserted === b.inserted ? 'equal' : okIns ? 'different as declared ED-712' : 'DIFFERENT'}, after the update ${okUpd ? 'equal' : 'DIFFERENT'}; printed pages ${same(a.print, b.print) ? `equal (${b.print.length})` : 'DIFFERENT'}`);
    if (!okIns || !okUpd) {
      fail('extra ED-713: the TOC text differs beyond the declared ED-712 label');
      for (const k of ['inserted', 'updated']) { const p = JSON.stringify(a[k]) ?? '', q = JSON.stringify(b[k]) ?? ''; if (p === q) continue; let i = 0; while (i < p.length && p[i] === q[i]) i++; console.log(`    ${k} @${i}\n      ts   …${p.slice(Math.max(0, i - 100), i + 160)}\n      rust …${q.slice(Math.max(0, i - 100), i + 160)}`); }
    }
    if (!okWord) fail(`extra ED-713: the Rust entries are not Word's 5 and viii: ${JSON.stringify(entry(b.updated))}`);
    if (!okRest) fail(`extra ED-713: updateToc result, printed pages or dirty flag differ: ts ${JSON.stringify([a.ok, a.print, a.dirty])} rust ${JSON.stringify([b.ok, b.print, b.dirty])}`);
  }

  // ── ED-713 (F2 round 2, Word manda sobre o TS): PAGEREF with chapter page numbering ──
  {
    // Word's document (toc-chapter-probe.ps1, result-chapter.txt): Heading 1 numbered "%1", w:pgNumType fmt="lowerRoman"
    // chapStyle="1", one chapter per page and the TOC Word wrote: 1-i, 2-ii, 3-iii. The frozen TS leaves the chapter out of a
    // PAGEREF (bookmarkPages, engine.ts:638): on screen and in what updateToc writes, `i ii iii`; the session follows Word. The
    // text is compared with only `\t<n>-<roman>` (Rust, Word) ↔ `\t<roman>` (TS) declared, the printed pages are not equal by
    // that declaration (the TOC page) and must be equal after it (the other pages).
    const [a, b] = await both('toc-chap-chapter-roman.docx', () => ev(async () => {
      const el = window.__es.el, es = window.__es;
      const text = () => { const k = el.getSelection(); el.selectAll(); const t = el.selectedText(); el.setSelection(k.anchor, k.focus); return t; };
      const print = async () => Promise.all((await el.printPages()).map(async (u) => {
        const img = new Image(); img.src = u; await img.decode();
        const c = document.createElement('canvas'); c.width = img.width; c.height = img.height;
        const x = c.getContext('2d'); x.drawImage(img, 0, 0);
        const d = x.getImageData(0, 0, c.width, c.height).data;
        let h = 0x811c9dc5; for (let i = 0; i < d.length; i++) h = Math.imul(h ^ d[i], 0x01000193);
        return `${img.width}x${img.height}:${(h >>> 0).toString(16)}`;
      }));
      const loaded = text();
      const shown0 = await print();
      el.setSelection({ para: 0, offset: 1 }); const okPages = el.updateToc('pages'); await es.settle(300);
      const pages = text();
      el.setSelection({ para: 0, offset: 1 }); const okAll = el.updateToc('all'); await es.settle(300);
      const all = text();
      return { loaded, pages, all, ok: [okPages, okAll], shown0, shown: await print(), dirty: el.isDirty() };
    }));
    const entry = (t) => (typeof t === 'string' ? (t.match(/\t[0-9ivxlcdm-]+(?=[\r\n]|$)/g) ?? []) : []);
    const word = ['\t1-i', '\t2-ii', '\t3-iii'], tsWant = ['\ti', '\tii', '\tiii'];
    // (both sides without the chapter numbers: the TS updateToc('pages') leaves the first entry's PAGEREF — nested in the TOC
    // field's result — as Word wrote it, `1-i`, and rewrites the other two without the chapter)
    const decl = (t) => (typeof t === 'string' ? t.replace(/\t[0-9]+-([ivxlcdm]+)(?=[\r\n]|$)/g, '\t$1') : t);
    const rows = ['loaded', 'pages', 'all'].map((k) => [k, entry(a[k]), entry(b[k])]);
    console.log(`extra toc-chap-chapter-roman (PAGEREF with chapter numbers): ${rows.map(([k, p, q]) => `${k} ts ${JSON.stringify(p)} rust ${JSON.stringify(q)}`).join('; ')}; Word ${JSON.stringify(word)}`);
    if (!same(entry(b.loaded), word) || !same(entry(b.pages), word) || !same(entry(b.all), word)) fail('extra ED-713: the Rust entries are not Word\'s');
    if (a.loaded !== b.loaded) fail('extra ED-713: the loaded text differs');
    for (const k of ['pages', 'all']) {
      if (a[k] === b[k]) continue;
      if (decl(b[k]) === decl(a[k]) && same(entry(decl(a[k])), tsWant)) continue;
      fail(`extra ED-713: the text after updateToc('${k}') differs beyond the declared chapter numbers`);
      const p = JSON.stringify(a[k]) ?? '', q = JSON.stringify(b[k]) ?? ''; let i = 0; while (i < p.length && p[i] === q[i]) i++;
      console.log(`    ${k} @${i}\n      ts   …${p.slice(Math.max(0, i - 100), i + 160)}\n      rust …${q.slice(Math.max(0, i - 100), i + 160)}`);
    }
    if (!same([a.ok, a.dirty], [b.ok, b.dirty])) fail(`extra ED-713: updateToc results or dirty flag differ: ts ${JSON.stringify([a.ok, a.dirty])} rust ${JSON.stringify([b.ok, b.dirty])}`);
    // the printed pages: the TOC page (the first) differs as declared (the chapter numbers); every other page is equal
    for (const k of ['shown0', 'shown']) {
      if (!Array.isArray(a[k]) || !Array.isArray(b[k]) || a[k].length !== b[k].length) { fail(`extra ED-713: printed page counts differ (${k})`); continue; }
      const off = a[k].map((x, i) => (x === b[k][i] ? null : i)).filter((i) => i !== null);
      console.log(`extra toc-chap-chapter-roman printed (${k}): ${a[k].length} pages, different ${JSON.stringify(off)} (declared ED-713: the TOC page 0)`);
      if (off.some((i) => i !== 0)) fail(`extra ED-713: printed pages other than the TOC page differ (${k}: ${JSON.stringify(off)})`);
    }
  }

  // ── wave 7c (S): updateFields' return value (F2 review major) ──
  {
    // `updateFields(scope, toc)` answers how many fields its refresh changed (TS universal-write.ts 6544–6565, `n +=
    // refreshFieldCache`; Rust fields.rs). Both engines write the TOC entry label as `String(page.number)` (pageLabelOf 6623;
    // F2's formatted session label reverted in wave 7c) and the refresh rewrites it with the layout's PAGEREF. Called twice
    // (n1, n2) on a fresh load each for 'all' and 'pages'. Equal everywhere but two consequences of Word-authority items,
    // declared with their exact numbers (anything else fails):
    //  - f7a__7__0 'pages' (paragraph 31 made Titulo1, then insertTocTemplate('auto1')): TS 1/0, Rust 0/0 — ED-712: the TS
    //    insertion leaves a stale first-entry label (`v`), which its refresh rewrites to Word's `5`; Rust already wrote `5`;
    //  - toc-chap-chapter 'all': TS 1/1, Rust 4/4 — ED-713: the TS layout's PAGEREF has no chapter, so the rebuilt entries'
    //    "1" stays "1" (only the TOC field counts); the Rust layout writes Word's "1-1" in each of the three entries
    //    (`ts_pageref_without_chapter` gives the TS's 1: uo-docx-layout tests/toc_pages_w7.rs).
    const DECL = {
      'f7a__7__0.docx|pages': { ts: [1, 0], rust: [0, 0], id: 'ED-712 (Word manda sobre o TS: the TS insertion\'s stale first label)' },
      'toc-chap-chapter.docx|all': { ts: [1, 1], rust: [4, 4], id: 'ED-713 (Word manda sobre o TS: the chapter in the layout\'s PAGEREF)' },
    };
    const used = new Set();
    for (const [want, prep] of [['f7a__7__0.docx', true], ['toc-chap-chapter-roman.docx', false], ['toc-chap-roman.docx', false], ['toc-chap-chapter.docx', false]]) {
      for (const mode of ['all', 'pages']) {
        const [a, b] = await both(want, (_e, arg) => ev(async ([prep, mode]) => {
          const el = window.__es.el, es = window.__es;
          const text = () => { const k = el.getSelection(); el.selectAll(); const t = el.selectedText(); el.setSelection(k.anchor, k.focus); return (t.match(/\t[0-9ivxlcdm-]+(?=[\r\n]|$)/g) ?? []).join(' '); };
          if (prep) {
            el.setSelection({ para: 31, offset: 0 }); el.applyStyle('Titulo1'); await es.settle();
            el.setSelection({ para: 0, offset: 0 }); el.insertTocTemplate('auto1'); await es.settle();
          }
          el.setSelection({ para: 0, offset: 0 });
          const n1 = el.updateFields('all', mode); await es.settle(300);
          const t1 = text();
          const n2 = el.updateFields('all', mode); await es.settle(300);
          return { n: [n1, n2], t1, t2: text() };
        }, arg), [prep, mode]);
        const key = `${want}|${mode}`;
        const d = DECL[key];
        const eq = same(a.n, b.n);
        let verdict = eq ? 'equal' : 'DIFFERENT';
        if (d) {
          if (same(a.n, d.ts) && same(b.n, d.rust)) { verdict = `different as declared ${d.id}`; used.add(key); }
          else fail(`extra updateFields ${key}: ts ${JSON.stringify(a.n)} rust ${JSON.stringify(b.n)}, declared ts ${JSON.stringify(d.ts)} rust ${JSON.stringify(d.rust)}`);
        } else if (!eq) fail(`extra updateFields ${key}: the counts differ (ts ${JSON.stringify(a.n)}, rust ${JSON.stringify(b.n)})`);
        console.log(`extra updateFields('all','${mode}') on ${want}: ts ${JSON.stringify(a.n)} rust ${JSON.stringify(b.n)} — ${verdict}; entries ts ${JSON.stringify([a.t1, a.t2])} rust ${JSON.stringify([b.t1, b.t2])}`);
      }
    }
    for (const k of Object.keys(DECL)) if (!used.has(k)) fail(`extra updateFields: declared ${k} not used (equal now? remove it)`);
  }

  // ── wave 7c (S): the parts a save writes, and after save → undo → save (item 2; record "Wave 7c — S") ──
  {
    // A table style taken where the style sheet is not editable (the .odt) or absent (a .docx without a styles part) is
    // written by `addStylesXml` (TS 8641); a style, a list or a table undone after a save. Every save before the undo (and
    // after the redo) must be part for part byte-equal to the TS. After the undo, Word manda sobre o TS (measured, Word 16.0
    // pt-BR, invisible COM: the undone edit's style, table style and numbering are gone at the next save): the TS keeps the
    // first save's parts (its pending map outside the history; its reused package), Rust writes the document of now — the
    // declared shape: the TS's parts after the undo = its parts after the edit, Rust's = its parts before the edit, the body
    // byte-equal. The .odt list (F) is equal (the TS saves an .odt into a fresh package each time, Rust too). Anything else fails.
    const dec = new TextDecoder();
    const unz = (b64) => unzipSync(Buffer.from(b64, 'base64'));
    const summ = (z) => {
      const t = (p) => (z[p] ? dec.decode(z[p]) : null);
      const st = t('word/styles.xml'), nb = t('word/numbering.xml') ?? '';
      return {
        names: Object.keys(z).sort().join(','),
        styleIds: st ? [...st.matchAll(/w:styleId="([^"]+)"/g)].map((m) => m[1]).join(',') : null,
        nums: (nb.match(/<w:num /g) ?? []).length,
        abs: (nb.match(/<w:abstractNum /g) ?? []).length,
        ct: /\/word\/styles\.xml/.test(t('[Content_Types].xml') ?? ''),
        rel: /relationships\/styles"/.test(t('word/_rels/document.xml.rels') ?? ''),
      };
    };
    const body = (z) => { const d = z['word/document.xml'] ? dec.decode(z['word/document.xml']) : ''; return { tblStyle: [...d.matchAll(/<w:tblStyle w:val="([^"]+)"/g)].map((m) => m[1]).join(','), numPr: (d.match(/<w:numPr>/g) ?? []).length }; };
    const partDiff = (x, y) => [...new Set([...Object.keys(x), ...Object.keys(y)])].filter((p) => !x[p] || !y[p] || Buffer.compare(Buffer.from(x[p]), Buffer.from(y[p])) !== 0);
    // the TS .odt export of hyperlinks.docx (as the file family reopens it)
    await env.openPage();
    await env.page.evaluate(helpers);
    await ev((e) => window.__es.make(e), 'ts');
    await ev((x) => window.__es.load(x), docOf('hyperlinks.docx'));
    const odt = await ev(() => { const u8 = window.__es.el.exportOdt(); let s = ''; for (let i = 0; i < u8.length; i += 0x8000) s += String.fromCharCode(...u8.subarray(i, i + 0x8000)); return btoa(s); });
    await ev(() => window.__es.remove());
    const CASES = [
      { name: 'B hyperlinks as .odt: insertTable → save → undo → save → redo + insertTable → save', doc: 'hyperlinks.docx', odt: true, op: 'table', redo: true, word: true },
      { name: 'D hyperlinks.docx (no styles part): insertTable → save → undo → save', doc: 'hyperlinks.docx', op: 'table', word: true },
      { name: "E styles.docx: createStyle('Meu') → save → undo → save", doc: 'styles.docx', op: 'style', word: true },
      // (a document without a numbering part: styles.docx has one whose list toggleList reuses, nothing to take back)
      { name: "E toc-chap-roman.docx (no numbering part): toggleList('decimal') → save → undo → save", doc: 'toc-chap-roman.docx', op: 'list', word: true },
      { name: "E' styles.docx (a numbering part): toggleList('decimal') → save → undo → save", doc: 'styles.docx', op: 'list', word: false },
      { name: "F hyperlinks as .odt: toggleList('decimal') → save → undo → save", doc: 'hyperlinks.docx', odt: true, op: 'list', word: false },
    ];
    for (const c of CASES) {
      const out = {};
      for (const engine of ['ts', 'rust']) {
        await env.openPage();
        await env.page.evaluate(helpers);
        const errs0 = env.errors.length;
        await ev((e) => window.__es.make(e), engine);
        await ev(([x, o]) => window.__es.load(x, o), [docOf(c.doc), c.odt ? odt : undefined]);
        const r = await ev(async ([op, redo]) => {
          const el = window.__es.el, es = window.__es;
          const b64 = (u8) => { let s = ''; for (let i = 0; i < u8.length; i += 0x8000) s += String.fromCharCode(...u8.subarray(i, i + 0x8000)); return btoa(s); };
          const s0 = b64(await el.save('docx'));
          if (op === 'table') { el.setSelection({ para: 0, offset: 0 }); el.insertTable(2, 2); }
          else { el.setSelection({ para: 0, offset: 0 }, { para: 0, offset: 3 }); if (op === 'style') el.createStyle({ name: 'Meu', type: 'paragraph' }); else el.toggleList('decimal'); }
          await es.settle();
          const s1 = b64(await el.save('docx'));
          el.undo(); await es.settle();
          const s2 = b64(await el.save('docx'));
          let s3 = null;
          if (redo) { el.redo(); await es.settle(); el.insertTable(1, 1); await es.settle(); s3 = b64(await el.save('docx')); }
          return { s0, s1, s2, s3 };
        }, [c.op, !!c.redo]);
        out[engine] = { z: Object.fromEntries(Object.entries(r).map(([k, v]) => [k, v ? unz(v) : null])), errors: env.errors.length - errs0 };
        await ev(() => window.__es.remove());
      }
      const a = out.ts.z, b = out.rust.z;
      if (out.ts.errors || out.rust.errors) fail(`extra parts ${c.name}: page errors ts ${out.ts.errors}, rust ${out.rust.errors}`);
      const res = [];
      for (const k of ['s0', 's1', ...(c.redo ? ['s3'] : []), ...(c.word ? [] : ['s2'])]) {
        const d = partDiff(a[k], b[k]);
        res.push(`${k} ${d.length ? `DIFFERENT (${d.join(', ')})` : `equal (${Object.keys(a[k]).length} parts)`}`);
        if (d.length) fail(`extra parts ${c.name}: ${k} parts differ ${d.join(', ')}`);
      }
      if (c.word) {
        const sa = summ(a.s2), sb = summ(b.s2);
        const okTs = same(sa, summ(a.s1)), okRust = same(sb, summ(b.s0)), okBody = same(body(a.s2), body(b.s2)) && partDiff({ d: a.s2['word/document.xml'] }, { d: b.s2['word/document.xml'] }).length === 0;
        const differs = !same(sa, sb);
        if (okTs && okRust && okBody && differs) res.push(`s2 different as declared (Word manda sobre o TS): ts ${JSON.stringify(sa)}, rust ${JSON.stringify(sb)} (= before the edit), body equal`);
        else if (!differs && okBody) { res.push('s2 EQUAL (the declaration is stale)'); fail(`extra parts ${c.name}: after the undo the TS and Rust parts are equal — remove the declaration`); }
        else { res.push(`s2 DIFFERENT: ts ${JSON.stringify(sa)} (after the edit ${JSON.stringify(summ(a.s1))}), rust ${JSON.stringify(sb)} (before ${JSON.stringify(summ(b.s0))}), body ts ${JSON.stringify(body(a.s2))} rust ${JSON.stringify(body(b.s2))}`); fail(`extra parts ${c.name}: after the undo, not the declared Word-authority shape`); }
      }
      console.log(`extra parts ${c.name}: ${res.join('; ')}`);
    }
  }

  // ── File > Print of a page holding a plugin inline ──
  {
    const [a, b] = await both('hyperlinks.docx', () => ev(async () => {
      const el = window.__es.el, es = window.__es;
      el.setSelection({ para: 0, offset: 1 }); el.insertCustomInline('tag', { name: 'imprime' }); await es.settle();
      const pages = await el.printPages();
      return Promise.all(pages.map(async (u) => {
        const img = new Image(); img.src = u; await img.decode();
        const c = document.createElement('canvas'); c.width = img.width; c.height = img.height;
        const x = c.getContext('2d'); x.drawImage(img, 0, 0);
        const d = x.getImageData(0, 0, c.width, c.height).data;
        // the plugin's red box (#c00) is on the printed page
        let red = 0; for (let i = 0; i < d.length; i += 4) if (d[i] === 0xcc && d[i + 1] === 0 && d[i + 2] === 0) red++;
        let h = 0x811c9dc5; for (let i = 0; i < d.length; i++) h = Math.imul(h ^ d[i], 0x01000193);
        return `${img.width}x${img.height}:${(h >>> 0).toString(16)}:red ${red > 0}`;
      }));
    }));
    const ok = same(a, b) && b.length > 0 && /red true/.test(b[0]);
    console.log(`extra hyperlinks (print with a plugin inline): ${ok ? 'equal, the inline printed' : 'DIFFERENT'}`);
    if (!ok) { fail('extra print with a plugin inline'); console.log(`    ts   ${JSON.stringify(a)}\n    rust ${JSON.stringify(b)}`); }
  }

  // ── the Format Painter: a Ctrl+click on a link and a click on a floating text box leave it armed ──
  {
    const pointOf = async (code) => JSON.parse(await ev((c) => window.__es.js(c), code));
    const [a, b] = await both('hyperlinks.docx', async () => {
      const o = {};
      await pointOf('window.open = () => null; return null;');
      const link = await pointOf(`const l = el.getLinks()[0]; if (!l) return null; el.setSelection({ para: 0, offset: 0 }); if (!el.findNext(l.text)) return null; const s = el.getSelection(); const p = { para: s.anchor.para, offset: Math.min(s.anchor.offset, s.focus.offset) + 1 }; el.setSelection(p); const r = el.caretRectAt(p); el.setSelection({ para: 0, offset: 0 }, { para: 0, offset: 2 }); el.copyFormat(); return r ? { x: r.x + 1, y: r.y + r.h / 2 } : null;`);
      o.armed = await pointOf('return [el.painterActive(), el.painterSticky()];');
      if (link) {
        await env.page.keyboard.down('Control');
        await env.page.mouse.click(link.x, link.y);
        await env.page.keyboard.up('Control');
        await ev(() => window.__es.settle(150));
        o.afterCtrlLink = await pointOf('return [el.painterActive(), el.getSelection()];');
      }
      // a floating text box: inserted (it takes the selection), its inside point, then out of it with the painter armed again
      const box = await pointOf(`el.setSelection({ para: 1, offset: 0 }); if (!el.insertTextbox()) return null; await es.settle(); const s = el.getSelection(); el.setSelection(s.focus); const r = el.caretRectAt(s.focus); el.exitTextbox?.(); el.setSelection({ para: 0, offset: 0 }, { para: 0, offset: 2 }); el.copyFormat(); await es.settle(); return r ? { x: r.x + 3, y: r.y + r.h / 2 } : null;`);
      if (box) {
        await env.page.mouse.click(box.x, box.y);
        await ev(() => window.__es.settle(150));
        o.afterBox = await pointOf('return [el.painterActive(), el.selectedObjectCount?.() ?? null];');
      }
      // a plain text click applies it (and turns it off)
      const plain = await pointOf(`el.exitTextbox?.(); const keep = el.getSelection(); const p = { para: 0, offset: 4 }; el.setSelection(p); const r = el.caretRectAt(p); el.setSelection(keep.anchor, keep.focus); return r ? { x: r.x + 1, y: r.y + r.h / 2 } : null;`);
      if (plain) {
        await env.page.mouse.click(plain.x, plain.y);
        await ev(() => window.__es.settle(150));
        o.afterText = await pointOf('return [el.painterActive(), el.getSelection()];');
      }
      o.events = await ev(() => window.__es.drain().filter((x) => !/^uw-(change|dirty-change)/.test(x)));
      return o;
    });
    // equal in both engines; the Ctrl+click on the link leaves the painter armed (the TS applies it only on the default text click)
    const ok = same(a, b) && b.armed?.[0] === true && b.afterCtrlLink?.[0] === true && !!b.afterText;
    console.log(`extra hyperlinks (painter: Ctrl+click link, text box, text): ${ok ? 'equal' : 'DIFFERENT'} rust ${JSON.stringify({ link: b.afterCtrlLink?.[0], box: b.afterBox?.[0], text: b.afterText?.[0] })}`);
    if (!ok) { fail('extra Format Painter clicks'); console.log(`    ts   ${JSON.stringify(a)}\n    rust ${JSON.stringify(b)}`); }
  }

  // ── the XPS of a page with transparency (a text watermark, semi-transparent): the alphas written as Opacity ──
  {
    await env.openPage();
    await env.page.evaluate(helpers);
    const errs0 = env.errors.length;
    await ev((e) => window.__es.make(e), 'rust');
    await ev((x) => window.__es.load(x), docOf('hyperlinks.docx'));
    const r = await ev(async () => {
      const el = window.__es.el, es = window.__es;
      el.setWatermark({ kind: 'text', text: 'CONFIDENCIAL', font: 'Liberation Sans', color: 'C0C0C0', semitransparent: true, layout: 'diagonal' }); await es.settle(); el.refresh(); await es.settle();
      const dl = el.rust.dl;
      const items = dl.pages.flatMap((p) => p.items);
      const alpha = items.filter((it) => (it.t === 'glyphs' || it.t === 'rect') && it.alpha !== undefined && it.alpha < 1).length
        + items.filter((it) => it.t === 'shape' && ((it.geom?.fillAlpha ?? 1) < 1 || (it.geom?.lineAlpha ?? 1) < 1)).length;
      const x = el.exportXps(); let s = ''; for (let i = 0; i < x.length; i += 0x8000) s += String.fromCharCode(...x.subarray(i, i + 0x8000));
      const wm = items.filter((it) => it.t === 'glyphs' && String.fromCodePoint(...it.glyphs.map((q) => q.cp)).includes('CONF')).map((g) => ({ ...g, glyphs: g.glyphs.length }));
      return { alpha, wm, kinds: [...new Set(items.map((it) => it.t))], xps: btoa(s) };
    });
    const px = unzipSync(new Uint8Array(Buffer.from(r.xps, 'base64')));
    const xml = Object.keys(px).filter((p) => /\.fpage$/.test(p)).map((p) => new TextDecoder().decode(px[p])).join('');
    const op = (xml.match(/ Opacity="/g) ?? []).length;
    const ok = r.alpha > 0 && op >= r.alpha;
    console.log(`extra hyperlinks (XPS of a watermarked page): display-list items with an alpha ${r.alpha}, Opacity attributes ${op} ${ok ? 'ok' : 'MISSING'} (items ${r.kinds.join(', ')}; the watermark runs ${JSON.stringify(r.wm).slice(0, 300)})`);
    if (!ok) fail('extra XPS: the alphas of the watermark are not written as Opacity');
    if (env.errors.length - errs0) fail('extra XPS watermark: page errors');
    await ev(() => window.__es.remove());
  }

  // ── wave 7 (F2): exportXps from each view — its side effects against the TS (`withLegacyLayout`, 15996) ──
  // A TIME field (HH:mm:ss) is inserted at the page clock; the clock then moves 1 h 2 min 3 s and the XPS is exported. Compared
  // TS vs Rust: the view, the selection, the dirty flag, the cell-edit events, whether the export moved the date fields on
  // screen (the TS `refresh()` after its export in the Print view; no engine, so no refresh, in Web/Draft/Outline), the XPS's
  // page count and the field's time written in it; in the Print view the screen after the export too. Rust only: the XPS
  // bytes are the same from every view (the pages are laid out for the export). `print-cell`: a cell being edited (a table
  // just inserted), which the TS leaves before a Print-view export (`exitCell`).
  {
    const fnvB64 = (b64) => { const u8 = Buffer.from(b64, 'base64'); let h = 0x811c9dc5; for (let i = 0; i < u8.length; i++) h = Math.imul(h ^ u8[i], 0x01000193); return (h >>> 0).toString(16); };
    const rustXps = {};
    let bad = 0;
    for (const variant of ['print', 'web', 'draft', 'outline', 'print-cell']) {
      const view = variant.replace('-cell', '');
      const [a, b] = await both('hyperlinks.docx', async () => {
        const read = () => ev(async () => {
          const el = window.__es.el, es = window.__es;
          await es.settle();
          el.paint();
          const cs = [...el.shadowRoot.querySelectorAll('canvas.page')];
          const fnv = (u8) => { let h = 0x811c9dc5; for (let i = 0; i < u8.length; i++) h = Math.imul(h ^ u8[i], 0x01000193); return (h >>> 0).toString(16); };
          const screen = cs.map((cv) => (cv.width > 1 ? fnv(new Uint8Array(cv.getContext('2d').getImageData(0, 0, cv.width, cv.height).data.buffer)) : '-')).join(',');
          return { view: el.getView(), sel: el.getSelection(), dirty: el.isDirty(), screen, cellmode: window.__cellmode.splice(0) };
        });
        await ev(async ([v, cell]) => {
          const el = window.__es.el, es = window.__es;
          window.__cellmode = [];
          el.addEventListener('uw-cellmode', (e) => window.__cellmode.push(JSON.stringify(e.detail)));
          el.setSelection({ para: 0, offset: 0 }); el.insertDateTime('HH:mm:ss', true); await es.settle();
          el.setView(v); await es.settle();
          if (cell) { el.setSelection({ para: 0, offset: 0 }); el.insertTable(2, 2); await es.settle(); }
          else el.setSelection({ para: 0, offset: 2 }, { para: 0, offset: 5 });
          window.__es.drain();
        }, [view, variant.endsWith('-cell')]);
        const before = await read();
        await env.page.clock.setFixedTime(NOW + 3_723_000);
        const x = await ev(async () => {
          const el = window.__es.el;
          try { const u8 = el.exportXps(); let s = ''; for (let i = 0; i < u8.length; i += 0x8000) s += String.fromCharCode(...u8.subarray(i, i + 0x8000)); return btoa(s); } catch (e) { return `THROW ${e && e.message}`; }
        });
        const after = await read();
        const r = { before: { view: before.view, sel: before.sel, dirty: before.dirty, cellmode: before.cellmode }, after: { view: after.view, sel: after.sel, dirty: after.dirty, cellmode: after.cellmode }, moved: before.screen !== after.screen };
        if (view === 'print') r.screen = after.screen;
        r.events = await ev(() => window.__es.drain().filter((s) => !/^uw-(change|dirty-change)/.test(s)));
        if (x.startsWith('THROW')) r.xps = x;
        else {
          const px = unzipSync(new Uint8Array(Buffer.from(x, 'base64')));
          const fp = Object.keys(px).filter((p) => /\.fpage$/.test(p));
          r.xps = { pages: fp.length, time: fp.map((p) => (/\d\d:\d\d:\d\d/.exec([...new TextDecoder().decode(px[p]).matchAll(/UnicodeString="([^"]*)"/g)].map((m) => m[1]).join('')) ?? [null])[0]) };
          r.fnv = fnvB64(x);
        }
        return r;
      });
      if (!variant.endsWith('-cell') && b.fnv) rustXps[variant] = b.fnv;
      const strip = (r) => { const { fnv: _f, ...rest } = r; return rest; };
      // wave 7c (W, item 2, fix round 1; reviewer W/2): hyperlinks.docx has no settings part — Word lays the inserted table out
      // in mode 12 (its first cell's text at the margin, x 1701; the TS's mode 15: 1814), so the Print screen after the
      // export differs; everything else is compared. Attributed: uo-docx-layout word_w7c.rs
      // item2_attribution_of_the_editor_harness_documents (this document + a 2×2 table at its start: only
      // ts_missing_settings_mode15 gives the TS's pages)
      const screenOnly = variant === 'print-cell' && a.screen !== b.screen && same(strip({ ...a, screen: null }), strip({ ...b, screen: null }));
      if (screenOnly) console.log(`extra hyperlinks (exportXps side effects, ${variant}): the Print screen differs as declared Word manda sobre o TS (item 2, mode 12 table): ts ${a.screen} rust ${b.screen}`);
      const ok = (screenOnly || same(strip(a), strip(b))) && typeof b.xps === 'object' && b.xps.pages > 0;
      console.log(`extra hyperlinks (exportXps side effects, ${variant}): ${ok ? (screenOnly ? 'equal but the declared screen' : 'equal') : 'DIFFERENT'} (date fields moved by the export: ts ${a.moved}, rust ${b.moved}; xps ${JSON.stringify(b.xps)}${variant.endsWith('-cell') ? `; cell events ${JSON.stringify(b.after.cellmode)}` : ''})`);
      if (!ok) { bad++; console.log(`    ts   ${JSON.stringify(strip(a))}\n    rust ${JSON.stringify(strip(b))}`); }
    }
    const xs = Object.values(rustXps);
    const sameBytes = xs.length === 4 && xs.every((h) => h === xs[0]);
    console.log(`extra hyperlinks (exportXps from Print/Web/Draft/Outline, rust): XPS bytes ${sameBytes ? 'equal' : 'DIFFERENT'} ${JSON.stringify(rustXps)}`);
    if (bad) fail(`extra exportXps side effects: ${bad} view(s) differ`);
    if (!sameBytes) fail('extra exportXps: the Rust XPS differs between the views');
  }

  // ── a section taller than 16384 px at zoom 250 % and DPR 2 (Web view): the strips keep the full buffer scale ──
  {
    const res = {};
    for (const engine of ['ts', 'rust']) {
      await env.openPage();
      await env.page.evaluate(helpers);
      const cdp = await env.page.context().newCDPSession(env.page);
      await cdp.send('Emulation.setDeviceMetricsOverride', { width: 1280, height: 900, deviceScaleFactor: 2, mobile: false });
      const errs0 = env.errors.length;
      await ev((e) => window.__es.make(e), engine);
      await ev((x) => window.__es.load(x), docOf('f7a__7__0.docx'));
      const read = () => ev(async () => {
        const el = window.__es.el, es = window.__es;
        await es.settle();
        const pages = [...el.shadowRoot.querySelectorAll('canvas.page')].map((c) => [parseFloat(c.style.width), parseFloat(c.style.height), c.width, c.height]);
        const strips = [...el.shadowRoot.querySelectorAll('canvas.page-strip')].map((c) => {
          const d = c.getContext('2d').getImageData(0, 0, c.width, Math.min(c.height, 400)).data;
          let ink = 0; for (let i = 3; i < d.length; i += 4) if (d[i]) ink++;
          return [parseFloat(c.style.width), parseFloat(c.style.height), c.width, c.height, parseFloat(c.style.top), ink > 0];
        });
        return { dpr: devicePixelRatio, pages, strips, scrollTop: el.scrollTop, view: el.getView(), zoom: el.getZoom() };
      });
      const r = {};
      r.top = await ev(async () => {
        const el = window.__es.el, es = window.__es;
        const text = Array.from({ length: 30 }, (_, i) => `Linha ${i} de um parágrafo comprido para a folha contínua passar do limite do canvas.`).join(' ');
        el.setSelection({ para: 0, offset: 0 });
        el.pasteText(Array.from({ length: 160 }, () => text).join('\n') + '\n');
        await es.settle();
        el.setView('web'); el.setZoom(2.5); await es.settle(); el.scrollTop = 0; await es.settle();
        return null;
      });
      r.atTop = await read();
      // the middle of the tall sheet: new strips there (the scroll), painted
      await ev(async () => { const el = window.__es.el; const c = el.shadowRoot.querySelector('canvas.page'); el.scrollTop = c.offsetTop + parseFloat(c.style.height) / 2; await new Promise((q) => setTimeout(q, 100)); });
      r.middle = await read();
      // a click there lands in the tall sheet (the hit test reads the page canvas box)
      const at = await ev(() => { const el = window.__es.el; const b = el.getBoundingClientRect(); return { x: b.left + 300, y: b.top + 400 }; });
      await env.page.mouse.click(at.x, at.y);
      await ev(() => window.__es.settle(150));
      r.clickSel = await ev(() => window.__es.el.getSelection());
      r.errors = env.errors.length - errs0;
      await ev(() => window.__es.remove());
      await cdp.send('Emulation.clearDeviceMetricsOverride');
      res[engine] = r;
    }
    const b = res.rust;
    const tall = b.middle.pages.find((p) => p[1] * 2 > 16384);
    const scaleOk = (s) => Math.abs(s[2] / s[0] - 2) < 0.01 && Math.abs(s[3] / s[1] - 2) < 0.01 && s[1] <= 4000;
    const okStrips = b.middle.strips.length > 0 && b.middle.strips.every(scaleOk) && b.middle.strips.some((s) => s[5]) && b.atTop.strips.every(scaleOk);
    const deep = b.clickSel.focus.para > 20;
    console.log(`extra tall Web sheet (zoom ${b.middle.zoom}, DPR ${b.middle.dpr}): rust sheet ${tall ? `${tall[1]} CSS px` : 'NOT TALL'}, strips at the top ${b.atTop.strips.length}, in the middle ${b.middle.strips.length} (${b.middle.strips.map((s) => `${s[0]}x${s[1]}@${s[2]}x${s[3]}`).join(' ')}), full scale ${okStrips ? 'yes' : 'NO'}; click in the middle → paragraph ${b.clickSel.focus.para}; ts tiles ${res.ts.middle.pages.length} (${res.ts.middle.pages.slice(0, 3).map((p) => `${p[0]}x${p[1]}@${p[2]}x${p[3]}`).join(' ')})`);
    if (!tall) fail('extra tall sheet: the Rust Web sheet is not taller than the canvas limit');
    if (!okStrips) fail('extra tall sheet: the strips are missing, blank or below the full buffer scale');
    if (!deep) fail('extra tall sheet: the click in the middle did not land deep in the document');
    if (res.ts.errors || b.errors) fail(`extra tall sheet: page errors ts ${res.ts.errors}, rust ${b.errors}`);
  }
}

/** memory (count build): create → load → the families' calls → remove; handles/sessions back to 0, live bytes flat */
async function runMemory() {
  await env.openPage();
  await env.page.evaluate(helpers);
  const n = docOf('f2-3-0.docx');
  const lives = [];
  for (let i = 0; i < SOAK; i++) {
    const r = await ev(async (x) => {
      const es = window.__es;
      await es.make('rust');
      await es.load(x);
      const el = es.el;
      el.setSelection({ para: 0, offset: 0 }, { para: 0, offset: 3 });
      el.setFontFormat({ bold: true, color: 'FF0000' });
      el.setTextEffects({ glow: { rad: 63500, color: { rgb: 'FF0000' } } });
      el.setParagraphBorders({ top: { val: 'single' } });
      el.getRulerInfo(); el.getParagraphFlow(); el.getLinks(); el.documentThemeColors();
      el.replaceAll('a', 'b');
      el.copyFormat(); el.cancelFormatPainter();
      await el.exportHtml(); await el.exportMarkdown(); el.exportOdt(); el.exportXps();
      const c = document.createElement('canvas'); el.paintTocTemplate('auto1', c, 200);
      el.showGridlines(true); el.showBookmarks(true); el.paint();
      const during = es.stats();
      es.remove();
      await new Promise((r2) => setTimeout(r2, 0));
      return { during, after: es.stats() };
    }, n);
    // wave 7c (S): the removed element keeps its document until it is collected (as the TS element its model): GC first
    r.after = await env.gc();
    lives.push(r.after.live);
    if (r.after.sessions || r.after.shellEngines || r.after.handles) fail(`memory: cycle ${i}: after remove ${JSON.stringify(r.after)}`);
    if (i === 0 || i === SOAK - 1) console.log(`memory: cycle ${i}: during ${JSON.stringify(r.during)} after ${JSON.stringify(r.after)}`);
  }
  const warm = lives.slice(Math.min(5, lives.length - 1));
  const flat = warm.every((x) => x === warm[0]);
  console.log(`memory: ${SOAK} cycles; live bytes after warm-up ${flat ? `flat at ${warm[0]}` : `NOT flat: ${warm.join(', ')}`}`);
  if (!flat && warm[0] !== null) fail('memory: live bytes not flat');
}

// wave 7c (V, fix round 1): the V5 interaction steps of the continuous views (`v5: true`) run by default; UO_EF_V5=0 leaves them
// out (they find open differences: record "Wave 7c — V, fix round 1")
if (process.env.UO_EF_V5 === '0') for (const s of Object.values(SCRIPTS)) s.steps = s.steps.filter((st) => !st.v5);
const only = process.env.UO_EF_ONLY ? new RegExp(process.env.UO_EF_ONLY) : null;
if (only) for (const s of Object.values(SCRIPTS)) s.docs = s.docs.filter((d) => only.test(d));
if (process.env.UO_EF_STEPS) for (const s of Object.values(SCRIPTS)) s.steps = s.steps.slice(0, Number(process.env.UO_EF_STEPS));
if (process.env.UO_EF_SKIP) { const re = new RegExp(process.env.UO_EF_SKIP); for (const s of Object.values(SCRIPTS)) s.steps = s.steps.filter((st) => !re.test(JSON.stringify(st))); }
const fams = MODE === 'all' ? Object.keys(SCRIPTS) : Object.keys(SCRIPTS).includes(MODE) ? [MODE] : [];
const t0 = Date.now();
for (const f of fams) await runFamily(f);
if (MODE === 'file' || MODE === 'all') await runFile();
if (MODE === 'extra' || MODE === 'all') await runExtra();
if (MODE === 'memory' || MODE === 'all') await runMemory();
await env.close();
console.log(`editor_switch_format (${MODE}): ${((Date.now() - t0) / 1000).toFixed(0)} s; ${failures ? `${failures} FAILURE(S)` : 'ok'}`);
process.exit(failures ? 1 : 0);
