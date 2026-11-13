// Wave 5, frontier R of the editor switch (record core/docs/modules/uo-bindings.md "Wave 5 — R"): review and document
// (tracked changes, comments, styles, find/navigation, fields and references, spelling, the clipboard buttons, the Alt
// keys) of `<universal-write engine="rust">` against the TS engine in a real headless Chromium.
//
//   UO_TS_ROOT=<frozen checkout> UO_WASM_PATH=<session .wasm> [UO_CHROMIUM=...] [UO_ER_SOAK=30] [UO_ER_ONLY=<regex of docs>]
//   node crates/uo-wasm/tests/browser/editor_switch_review.mjs --mode revisions|comments|styles|find|fields|spell|clipboard|keys|memory|all
//
// Per mode and document: the TS element first, then the Rust one, both with `author="Ana"` and the ribbon (the bar commands
// by id), loaded with `loadDocx(bytes, { protected: false })`; the SAME script (element API calls, real keys through
// Playwright, bar buttons by id) on both; after each step the facets of the family (canonical JSON) plus the selection and
// the uw-* events; at the end the page count, the screen canvases with R's overlays on (field shading, comment highlight,
// squiggles, search hits; every other TS overlay off) and off, the overlay mask (where R's overlays change the page, between
// the margins), and the saved .docx part by part. Steps: element calls (`js`), real keys (`key`, `type`), real clicks
// (`click`, at the position a step put in `window.__clickAt`; `ctrl` holds Control) and bar buttons (`button`). Every
// difference not covered by DECLARED fails, and so does any page error.
// Declared duplicate: the page helpers are copy-adapted from editor_switch.mjs (gate item: move them into the lib).
import { createRequire } from 'node:module';
import { join } from 'node:path';
import { writeFileSync } from 'node:fs';
import { setup, core, repo, fwd } from './editor_switch_lib.mjs';

const root = process.env.UO_TS_ROOT;
if (!root) throw new Error('set UO_TS_ROOT to the frozen TS checkout');
const arg = (name, dflt) => { const i = process.argv.indexOf(name); return i >= 0 ? process.argv[i + 1] : dflt; };
const MODE = arg('--mode', 'all');
const SOAK = Number(process.env.UO_ER_SOAK ?? 30);
const target = process.env.CARGO_TARGET_DIR ?? join(core, 'target');
const wasmPath = process.env.UO_WASM_PATH ?? join(target, 'session.wasm');
const outDir = join(target, 'es-review');
const { unzipSync } = createRequire(join(root, 'write/package.json'))('fflate');
const D3 = join(core, 'crates/uo-docx/tests/parity/d3/fixtures');
const extraDocs = Object.fromEntries(['comments', 'revisions', 'fields', 'many-fields', 'hyperlinks', 'styles', 'headers'].map((n) => [`${n}.docx`, join(D3, `${n}.docx`)]));
// wave 7c (V, fix round 1): the document Word drew its field shading and comment highlight on (made by the oracle script)
extraDocs['field-shading.docx'] = join(core, 'crates/uo-docx-layout/tests/fidelity/field-shading.docx');
// wave 7c (V, fix round 2): a field in the footer and one in a footnote (ED-304, "When selected" in every story)
extraDocs['field-shading-stories.docx'] = join(core, 'crates/uo-docx-layout/tests/fidelity/field-shading-stories.docx');

const env = await setup({
  root, wasmPath, outDir, stride: 1, only: 'f2-3-0|f2-3-2|f7a__7__0|f7c__7__0|f8__7__0', extraDocs,
  entryExtra: `import { registerBuiltinSources } from '${fwd(join(repo, 'shared/spell/src/builtin.ts'))}';\nregisterBuiltinSources();`,
});
const fixtureName = (p) => p.replace(/\\/g, '/').replace(/^.*\//, '');
const docOf = (want) => { for (const [n, p] of env.docsByName) if (fixtureName(p) === want || n === want) return n; throw new Error(`no document ${want}`); };
let failures = 0;
const fail = (m) => { failures++; console.log(`FAIL ${m}`); };

/** the page-side helpers (evaluated in each new page) */
const helpers = () => {
  const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
  const frame = () => new Promise((r) => requestAnimationFrame(() => r()));
  const fnv = (u8) => { let h = 0x811c9dc5; for (let i = 0; i < u8.length; i++) h = Math.imul(h ^ u8[i], 0x01000193); return (h >>> 0).toString(16).padStart(8, '0'); };
  const EVENTS = ['uw-change', 'uw-dirty-change', 'uw-track-changes', 'uw-revisions-changed', 'uw-comments-changed', 'uw-bookmarks-change',
    'uw-styles-change', 'uw-caption-settings', 'uw-pasted', 'uw-toc-update', 'uw-markup', 'uw-paste-special', 'uw-comment-click', 'uw-protected'];
  /** R's overlays (on for both engines in the end capture) and the TS ones that belong to other frontiers (off) */
  const MINE = ['paintEngineMarks', 'paintEngineSpelling', 'paintSearchHits'];
  // (`paintMultiSelection` is the TS floating objects' multi-selection and lasso: frontier O's)
  const OTHERS = ['paintBookmarkMarks', 'paintTableAdorners', 'paintTocControl', 'paintEngineInlines', 'paintMultiSelection'];
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
      if (!window.__nolang) el.setAttribute('lang', 'pt');
      el.setAttribute('topbar', '');
      el.setAttribute('toolbar', 'ribbon');
      el.style.cssText = 'display:block;width:1100px;height:850px';
      const ready = new Promise((r) => el.addEventListener('uw-ready', r, { once: true }));
      document.body.appendChild(el);
      await ready;
      if (el.rust) el.rust.tsShading = true; // wave 7c (V, fix round 1): the TS field shading/comment look (ED-304: Word's is the default)
      for (const t of EVENTS) el.addEventListener(t, (e) => { log.push(`${t} ${J(e.detail ?? null)}`); });
      // the TS asks before replacing a TOC (uw-toc-update): no host answers, the default runs
      api.el = el;
      return true;
    },
    async settle(ms = 300) {
      const el = api.el;
      if (el.rust) await el.rust.idle();
      else { await document.fonts.ready; await sleep(ms); }
      await frame();
    },
    async load(n) {
      window.__seed(1);
      api.el.loadDocx(new Uint8Array(await (await fetch(`/docs/${n}`)).arrayBuffer()), { protected: false });
      await api.settle();
      api.el.refresh();
      await api.settle();
      log.length = 0;
      return api.el.layoutPageCount();
    },
    remove() { api.el?.remove(); api.el = null; },
    drain() { const out = log.slice(); log.length = 0; return out; },
    focus() { api.el.focusEditor(); },
    /** a script step: JS over the element (`el`), the answer kept as a facet */
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
    async clickButton(id) {
      const bt = api.button(id);
      if (!bt) return 'missing';
      const off = !!(bt.disabled || bt.getAttribute?.('aria-disabled') === 'true' || bt.classList?.contains('disabled'));
      bt.dispatchEvent(new MouseEvent('mousedown', { bubbles: true, composed: true }));
      bt.click();
      await api.settle(200);
      return off ? 'disabled' : 'clicked';
    },
    /** the viewport point of `window.__clickAt` (a position a script step chose), scrolled into view first */
    clickPoint() {
      const el = api.el;
      const p = window.__clickAt;
      if (!p) return null;
      // the shell answers `caretRectAt` for the selection's focus only (ED-caretRectAt): the caret goes there and back on
      // both engines
      const keep = el.getSelection();
      const rect = () => { el.setSelection(p); const r = el.caretRectAt(p); el.setSelection(keep.anchor, keep.focus); return r; };
      let r = rect();
      if (!r) return null;
      const box = el.getBoundingClientRect();
      if (r.y < box.top + 60 || r.y + r.h > box.bottom - 60) { el.scrollTop += r.y - (box.top + box.height / 2); r = rect(); }
      return r ? { x: r.x + 0.5, y: r.y + r.h / 2 } : null;
    },
    /** the open dialogs' text (the bars' dialogs reached by a command) */
    dialogs() { return [...api.el.shadowRoot.querySelectorAll('dialog')].filter((d) => d.open).map((d) => d.innerText.replace(/\s+/g, ' ').trim().slice(0, 300)); },
    closeDialogs() { for (const d of api.el.shadowRoot.querySelectorAll('dialog')) if (d.open) d.close(); },
    /** the family facets after a step */
    facets(fam) {
      const el = api.el;
      const f = { sel: el.getSelection(), events: api.drain(), dirty: el.isDirty() };
      const st = el.getSelectionState();
      f.state = { run: { bold: st.run.bold, italic: st.run.italic, charStyle: st.run.charStyle }, para: { styleId: st.para.styleId, alignment: st.para.alignment } };
      if (fam === 'revisions') {
        f.revisions = el.getRevisions();
        f.markup = el.getMarkupOptions();
        f.mode = el.getMarkupMode();
        f.tracking = el.isTrackingChanges();
        f.authors = el.revisionAuthors();
        f.atCaret = el.revisionAtCaret();
        f.anchors = el.revisionAnchors();
      } else if (fam === 'comments') {
        f.comments = el.getComments();
        f.anchors = f.comments.map((c) => el.commentAnchorPx(c.id));
      } else if (fam === 'styles') {
        f.lists = Object.fromEntries(['gallery', 'recommended', 'inUse', 'document', 'all'].map((k) => [k, el.listStyles(k).map((s) => `${s.id}|${s.name}|${s.kind}|${s.inUse ? 1 : 0}|${s.quick ? 1 : 0}|${s.hidden ? 1 : 0}|${s.priority}|${s.basedOn ?? ''}|${s.next ?? ''}|${s.link ?? ''}`)]));
        f.paragraphStyles = el.paragraphStyles();
        f.looks = Object.fromEntries(el.listStyles('gallery').map((s) => [s.id, el.styleFormatting(s.id)]));
        f.current = el.currentStyles();
        f.inspect = el.inspectStyle();
      } else if (fam === 'find') {
        f.headings = el.getHeadings();
        f.currentHeading = el.currentHeading();
        f.ranges = el.getSelectionRanges();
      } else if (fam === 'fields') {
        f.bookmarks = el.getBookmarks({ hidden: true, sort: 'location' });
        f.link = el.getLink();
        f.targets = el.getLinkTargets();
        f.canField = el.canInsertField();
        f.toc = { has: el.hasTableOfContents(), in: el.inTableOfContents(), opts: el.getTableOfContentsOptions(), level: el.getTocLevel() };
        f.captions = el.getCaptionLabels();
      } else if (fam === 'clipboard') {
        f.items = el.clipboardItems().map((x) => x.text);
      }
      f.selected = el.selectedText();
      return J(f);
    },
    /** the spelling lists of every paragraph once both engines have checked them (polled until stable) */
    async spellLists(n) {
      const el = api.el;
      const get = (p) => {
        if (el.rust) return el.rust.reviewProbe.spell(p).list;
        const para = el.model.paragraphs[p];
        return el.spell ? el.spell.get(para) : undefined;
      };
      el.paint();
      let last = '';
      for (let i = 0; i < 200; i++) {
        await sleep(150);
        el.paint();
        const lists = Array.from({ length: n }, (_, p) => get(p));
        const s = J(lists.map((l) => l?.map((m) => `${m.start}-${m.end}:${m.word}:${m.lang}`) ?? 'pending'));
        if (!s.includes('pending') && s === last) return s;
        last = s;
      }
      return `UNSTABLE ${last}`;
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
      // the selection is not part of the capture (both engines draw it; BUG-1 and gate item 1 are the main harness's)
      el.setSelection({ para: 0, offset: 0 });
      el.caretVisible = false;
      if (!el.rust) for (const m of OTHERS) el[m] = () => {};
      // the squiggles come from the asynchronous spell checker (120 ms batches, a Worker): the capture waits until two
      // paints 400 ms apart are equal (both engines the same way)
      const grabNow = () => { el.paint(); return cs.map((cv) => (cv.width > 1 ? fnv(new Uint8Array(cv.getContext('2d').getImageData(0, 0, cv.width, cv.height).data.buffer)) : '-')).join(','); };
      let prev = grabNow();
      for (let i = 0; i < 25; i++) { await sleep(400); const now = grabNow(); if (now === prev) break; prev = now; }
      el.paint();
      await frame();
      const screen = cs.map((cv) => (cv.width > 1 ? fnv(new Uint8Array(cv.getContext('2d').getImageData(0, 0, cv.width, cv.height).data.buffer)) : '-'));
      const shots = window.__dump ? cs.map((cv) => (cv.width > 1 ? cv.toDataURL('image/png') : null)) : [];
      const dataOf = () => cs.map((cv) => (cv.width > 1 ? cv.getContext('2d').getImageData(0, 0, cv.width, cv.height).data : null));
      const on = dataOf();
      for (const m of MINE) el[m] = () => {};
      // the other frontiers' overlays (their paint hooks) off too: the plain capture is the page base
      if (el.rust) el.rust.hooks.paint.length = 0;
      el.paint();
      await frame();
      const plain = cs.map((cv) => (cv.width > 1 ? fnv(new Uint8Array(cv.getContext('2d').getImageData(0, 0, cv.width, cv.height).data.buffer)) : '-'));
      // where R's overlays changed the page (the pixels that differ between the two captures: count and positions), so
      // the overlays are compared even where the page base differs by a declared cause (ED-314: theme colors). The whole page:
      // the header/footer and note overlays included (wave 7c, V: ED-304 closed — the shell paints every story's lines)
      const off = dataOf();
      const maskRows = [];
      const overlayMask = on.map((a, i) => {
        const b = off[i];
        if (!a || !b) return '-';
        const cv = cs[i];
        const y0 = 0, y1 = cv.height;
        let h = 0x811c9dc5, n = 0;
        const rows = new Map();
        for (let k = y0 * cv.width * 4; k < Math.min(a.length, y1 * cv.width * 4); k += 4) {
          if (a[k] !== b[k] || a[k + 1] !== b[k + 1] || a[k + 2] !== b[k + 2] || a[k + 3] !== b[k + 3]) {
            h = Math.imul(h ^ (k >> 2), 0x01000193); n++;
            const y = Math.floor((k >> 2) / cv.width), x = (k >> 2) % cv.width;
            const r = rows.get(y); if (r) { r[0] = Math.min(r[0], x); r[1] = Math.max(r[1], x); r[2]++; } else rows.set(y, [x, x, 1]);
          }
        }
        // (diagnostics: per row, the x range and the count, printed when the masks differ)
        maskRows.push(Object.fromEntries(rows));
        return `${n}:${(h >>> 0).toString(16)}`;
      });
      const bytes = el.saveDocx();
      let bin = '';
      for (let i = 0; i < bytes.length; i += 0x8000) bin += String.fromCharCode(...bytes.subarray(i, i + 0x8000));
      return { pages: cs.length, screen, plain, overlayMask, maskRows, overlaysPainted: screen.filter((h, i) => h !== plain[i]).length, shots, docx: btoa(bin) };
    },
    stats() { return window.__e.UniversalWriteElement.engineStats(); },
  };
  window.__es = api;
  return true;
};
/** page.evaluate with a deadline (a hung step fails the run instead of hanging it) */
const ev = (f, a) => Promise.race([env.page.evaluate(f, a), new Promise((_, rej) => setTimeout(() => rej(new Error(`evaluate timed out: ${String(f).slice(0, 120)}`)), 90000))]);
const VERBOSE = !!process.env.UO_ER_VERBOSE;

// ── the scripts (one per family; every step runs on both engines) ──
// step: { js } | { key } | { type } | { button } — the facets of the family after each
const P = (p, o) => `{ para: ${p}, offset: ${o} }`;
const SCRIPTS = {
  revisions: {
    docs: ['revisions.docx', 'f7a__7__0.docx'],
    steps: [
      { js: 'return null;' },
      { js: `el.setSelection(${P(0, 0)}); el.toggleTrackChanges(true); return el.isTrackingChanges();` },
      { type: 'Ana ' },
      { key: 'Delete' }, { key: 'Delete' },
      // the author changes mid-script (attribute, then the property without the attribute): the next revisions carry it
      { js: `el.setAttribute('author', 'Bia'); return el.author;` }, { type: 'xy' },
      { js: `el.removeAttribute('author'); el.author = 'Caio'; return el.author;` }, { type: 'z' },
      { js: `el.setAttribute('author', 'Ana'); return el.author;` },
      { js: `el.setSelection(${P(0, 0)}, ${P(0, 3)}); el.toggleRunProp('bold'); return null;` },
      { js: `el.setSelection(${P(0, 0)}); return el.nextChange();` },
      { js: 'return el.revisionAtCaret();' },
      { js: `el.setMarkupMode('simple'); return el.getMarkupMode();` },
      { js: `el.setMarkupMode('original'); return el.getMarkupMode();` },
      { js: `el.setMarkupMode('none'); return el.getMarkupMode();` },
      { js: `el.setMarkupMode('all'); return el.getMarkupMode();` },
      { js: `el.setMarkupOptions({ formatting: false }); return el.getMarkupOptions();` },
      { js: `el.setMarkupOptions({ authors: [] }); return el.getMarkupOptions();` },
      { js: `el.setMarkupOptions({ authors: null, formatting: true }); return el.getMarkupOptions();` },
      { button: 'change:next' },
      { js: 'return el.acceptChange();' },
      { js: 'return el.rejectChange({ moveNext: false });' },
      { js: 'return el.previousChange();' },
      { js: 'return el.selectRevision(0);' },
      { js: 'return el.acceptRevisionAt(0);' },
      { js: 'return el.rejectRevisionAt(0);' },
      { button: 'trackChanges' },
      { js: 'return el.rejectAllChanges({ shown: true });' },
      { js: 'return el.acceptAllChanges({ stopTracking: true });' },
      { key: 'Control+z' },
      { button: 'change:acceptAll' },
    ],
  },
  comments: {
    docs: ['comments.docx', 'f2-3-0.docx'],
    steps: [
      { js: 'return null;' },
      { js: `el.setSelection(${P(0, 0)}, ${P(0, 4)}); return el.addComment('Nota R');` },
      // a click on commented text (the pointer: `uw-comment-click` in the events facet)
      { js: `const c = el.getComments()[0]; if (!c) return null; el.goToComment(c.id); const s = el.getSelection(); const a = s.anchor.para * 1e6 + s.anchor.offset <= s.focus.para * 1e6 + s.focus.offset ? s.anchor : s.focus; window.__clickAt = { para: a.para, offset: a.offset + 1 }; el.setSelection(${P(0, 0)}); return window.__clickAt;` },
      { click: true },
      { js: `el.setSelection(${P(0, 1)}); return el.addComment('sem seleção');` },
      { js: 'const c = el.getComments()[0]; if (c) el.goToComment(c.id); return c ? c.id : null;' },
      { js: `el.setSelection(${P(1, 0)}, ${P(1, 2)}); return el.addComment('Outra', 'Bia');` },
      { js: 'const c = el.getComments()[0]; if (c) el.removeComment(c.id); return el.getComments().length;' },
      { key: 'Control+z' },
      { button: 'comment:deleteAll' },
    ],
  },
  styles: {
    docs: ['styles.docx', 'f2-3-0.docx', 'headers.docx'],
    steps: [
      { js: 'return el.knownFontFamilies().length > 0;' },
      { js: `el.setSelection(${P(0, 0)}, ${P(0, 3)}); return el.createStyle({ name: 'Meu Estilo R' });` },
      { js: `return el.modifyStyle(el.currentStyles().paragraph, { format: { bold: true, alignment: 'center' } });` },
      { js: `return el.renameStyle(el.currentStyles().paragraph, 'Outro Nome R');` },
      { js: 'el.setStyleInGallery(el.currentStyles().paragraph, false); return null;' },
      { button: 'style:h1' },
      { js: 'return el.selectionStyleFormat();' },
      { js: `el.setSelection(${P(0, 0)}, ${P(0, 2)}); el.toggleRunProp('italic'); return el.updateStyleFromSelection();` },
      { js: 'el.clearParagraphStyle(); return el.currentStyles();' },
      { js: `return el.deleteStyle(el.listStyles('all').find((s) => s.name === 'Outro Nome R')?.id ?? 'x');` },
      { js: `return el.setDefaultFont({ fontFamily: 'Liberation Serif', sizePt: 12 });` },
      { key: 'Control+z' },
    ],
  },
  find: {
    docs: ['f2-3-0.docx', 'f2-3-2.docx', 'headers.docx'],
    steps: [
      { js: `el.setSelection(${P(0, 0)}); return el.findNextMatch({ text: 'de' });` },
      { js: `return el.findNextMatch({ text: 'de' });` },
      { js: `return el.findNextMatch({ text: 'de' }, { direction: 'up' });` },
      { js: `return el.findNextMatch({ text: 'zzqq' });` },
      { js: `return el.findNextMatch({ text: '[', wildcards: true });` },
      { js: `return el.replaceMatch({ text: 'de' }, 'DE');` },
      { js: `return el.highlightMatches({ text: 'a' });` },
      { js: `return el.selectAllMatches({ text: 'que' });` },
      { js: `return el.replaceAllMatches({ text: 'o', matchCase: true, wholeWord: true }, '0');` },
      { js: `return el.search('a').slice(0, 20);` },
      { js: 'return el.goToSearchResult(1);' },
      { js: `return el.goTo('page', '2');` },
      { js: `return el.goTo('heading', '');` },
      { js: 'el.clearSearch(); return null;' },
      { js: `const h = el.getHeadings(); if (h.length) el.goToHeading(0); return h.length;` },
      { js: `return el.getHeadings().length ? el.demoteHeading(0) : null;` },
      { key: 'Control+z' },
    ],
  },
  fields: {
    docs: ['fields.docx', 'many-fields.docx', 'hyperlinks.docx', 'f2-3-0.docx'],
    steps: [
      { js: `return [el.tocTemplates(), el.dateTimeFormats('pt').length, el.getAutoCaption() ?? null, el.captionTarget()];` },
      { js: `el.setSelection(${P(0, 0)}, ${P(0, 3)}); return el.insertBookmark('MarcaR');` },
      { js: `el.setSelection(${P(0, 5)}); return [el.hasBookmark('marcar'), el.goToBookmark('MarcaR')];` },
      { js: `el.setSelection(${P(0, 0)}, ${P(0, 4)}); return el.insertHyperlink({ address: 'https://example.com/x', tip: 'dica' });` },
      // Ctrl+click on the link (the pointer; window.open stubbed)
      { js: `window.__opened = []; window.open = (u, t) => { window.__opened.push(u + ' ' + t); return null; }; window.__clickAt = ${P(0, 2)}; el.setSelection(${P(0, 0)}); return null;` },
      { click: true, ctrl: true },
      { js: 'return window.__opened;' },
      { js: `el.setSelection(${P(0, 1)}); return el.getLink();` },
      { js: 'return el.removeLink();' },
      { js: `el.setSelection(${P(0, 0)}); el.insertField('PAGE'); el.insertField('NUMPAGES'); return null;` },
      { js: `return el.updateFields('all');` },
      { key: 'F9' },
      { js: `el.setSelection(${P(0, 0)}); return el.insertTableOfContents({});` },
      { js: `return el.updateToc('pages');` },
      { js: 'return el.removeTableOfContents();' },
      { js: `el.setSelection(${P(0, 0)}); return el.insertCaption({ label: el.getCaptionLabels()[0]?.name ?? 'Figura', title: ' R' });` },
      { js: `return el.deleteBookmark('MarcaR');` },
      { js: `return el.navigateTo('#_top');` },
      { key: 'Control+z' },
    ],
  },
  clipboard: {
    docs: ['f2-3-0.docx'],
    steps: [
      { js: `el.setSelection(${P(0, 0)}, ${P(0, 5)}); return null;` },
      { button: 'copy' },
      { js: 'return await navigator.clipboard.readText();' },
      { js: `el.setSelection(${P(1, 0)}); return null;` },
      { button: 'paste' },
      { js: `el.setSelection(${P(1, 0)}, ${P(1, 3)}); return null;` },
      { button: 'cut' },
      { js: 'return await navigator.clipboard.readText();' },
      { js: `await navigator.clipboard.writeText('colado R'); el.setSelection(${P(0, 2)}); await el.pastePlain(); return null;` },
      { js: `el.setSelection(${P(0, 2)}); await el.paste('text'); return null;` },
      { js: `el.pasteDefaults = { within: 'text', between: 'keep', other: 'text' }; return el.pasteDefaults.within;` },
      // the Office Clipboard pane's listeners on a KEYBOARD copy/cut (the clipboard event), then the bars' in-place change of
      // the defaults (chrome-clipboard.ts: Object.assign) before a button paste and a keyboard paste
      { js: `window.__notes = 0; el.onClipboardChange(() => { window.__notes++; }); el.setSelection(${P(0, 0)}, ${P(0, 6)}); return null;` },
      { key: 'Control+c' },
      { js: 'return [window.__notes, el.clipboardItems().length];' },
      { key: 'Control+x' },
      { js: 'return [window.__notes, el.clipboardItems().length];' },
      { js: `Object.assign(el.pasteDefaults, { within: 'keep' }); el.setSelection(${P(2, 0)}); return el.pasteDefaults.within;` },
      { button: 'paste' },
      { key: 'Control+v' },
      { js: 'return [window.__notes, el.clipboardItems().length];' },
    ],
  },
  keys: {
    docs: ['f2-3-0.docx'],
    steps: [
      { js: `el.setSelection(${P(1, 5)}); el.focusEditor(); return null;` },
      { key: 'Alt+ArrowRight' }, { key: 'Alt+Shift+ArrowRight' }, { key: 'Alt+ArrowLeft' }, { key: 'Alt+ArrowDown' }, { key: 'Alt+ArrowUp' },
      { key: 'Alt+End' }, { key: 'Alt+Home' }, { key: 'Alt+Enter' }, { key: 'Alt+Backspace' }, { key: 'Alt+Delete' },
      { key: 'Control+Alt+ArrowRight' }, { key: 'Control+Alt+Shift+ArrowLeft' }, { key: 'Control+Alt+Backspace' }, { key: 'Control+Alt+Delete' },
      { key: 'Control+Alt+b' }, { key: 'Control+Alt+e' }, { key: 'Control+Alt+z' }, { key: 'Control+Alt+v' },
      { key: 'Control+Minus' }, { key: 'F9' },
      // the TS rules without an Alt check: Ctrl(+Alt)+Home/End = the document's edges, PageUp/PageDown with any modifier,
      // Ctrl+Alt(+Shift)+Enter = page (column) break, Tab with Ctrl or Alt
      { js: `el.setSelection(${P(3, 5)}); return null;` }, { key: 'Control+Alt+Home' },
      { js: `el.setSelection(${P(3, 5)}); return null;` }, { key: 'Control+Alt+End' }, { key: 'Control+Alt+Shift+Home' },
      { js: `el.setSelection(${P(3, 5)}); return null;` }, { key: 'Alt+PageDown' }, { key: 'Alt+PageUp' }, { key: 'Control+PageDown' },
      { key: 'Control+Alt+Shift+PageUp' },
      { js: `el.setSelection(${P(3, 5)}); return el.layoutPageCount();` }, { key: 'Control+Alt+Enter' }, { js: 'return el.layoutPageCount();' },
      { key: 'Control+Alt+Shift+Enter' }, { js: 'return el.layoutPageCount();' }, { key: 'Control+Tab' }, { key: 'Alt+Tab' },
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
 * Declared differences: `doc|family` → { from: first step, facets: the keys of the step facets (or '*'), end: end facets,
 * id, why }. Everything else fails.
 */
const DECLARED = {
  // wave 7 (B): ED-314 / R-1 closed for the colours — a document without a theme part is laid out with the Office 2023
  // colours Word paints (gate-O3, uo-docx-layout theme_word.rs), which the TS editor paints too; R-2 (Update Style to Match
  // Selection) and R-6 (the hyphens) are equal in the wave-7 runs (screens with the overlays off equal). What is left, each
  // measured with UO_ER_COVERED=1 and the dumped screens:
  // (f2-3-0|*, the f2-3-0|styles footer part, f2-3-2|find and f8__7__0|spell: ED-304 closed in wave 7c, V — the shell's
  // field shading, squiggles, bookmark brackets and search hits read every story's lines, and search() answers the page of
  // a hit in a table cell from them)
  // the gallery look of the latent Subtle Reference (w:color 5A5A5A, themeColor text1, themeTint A5): the TS computes 595959,
  // the session's look 5A5A5A — Word paints 5A5A5A (measured: text1 tint A5 with and without a theme part, record "Wave 7 — B")
  'styles|styles': { looks: ['RefernciaSutil'], id: 'Word authority over TS (Subtle Reference look)', why: 'Subtle Reference: TS 595959, session and Word 5A5A5A' },
  'headers|styles': { looks: ['RefernciaSutil'], id: 'Word authority over TS (Subtle Reference look)', why: 'as styles|styles' },
  'f2-3-0|styles': { looks: ['RefernciaSutil'], id: 'Word authority over TS (Subtle Reference look)', why: 'as styles|styles' },
  // wave 7c (W, item 4; record uo-bindings.md "Wave 7c — W"): fields.docx's REF marca and NOTEREF n1 name bookmarks the
  // document does not define: Word's result is its bold error text ("Erro! Fonte de referência não encontrada." / "Erro!
  // Indicador não definido.", oracle refbm), the TS the start paragraph's text or the cached result. Attributed natively
  // (word_w7c.rs attribute_documents_to_the_word_rules with UO_W7C_ATTR_SOURCES: only ts_ref_bookmarks changes its pages):
  // updateFields('all') rewrites those two results too (11 against the TS's 9), and the screens and saved body follow
  'fields|fields': { from: 10, facets: ['answer'], end: ['screen (R overlays on)', 'screen (overlays off)', 'overlay mask', 'saved word/document.xml'], id: 'Word manda sobre o TS (item 4, REF error)', why: 'REF/NOTEREF to an undefined bookmark: the bold error text of Word' },
};
/** the style ids whose look is left out of the 'looks' facet (declared per id) */
const looksOut = (key) => (DECLARED[key] ?? DECLARED[`${key.split('|')[0]}|*`])?.looks ?? [];
const declared = (key, facet, step) => {
  const d = DECLARED[key] ?? DECLARED[`${key.split('|')[0]}|*`];
  if (!d) return null;
  if (step === undefined) return d.end === '*' || (d.end ?? []).includes(facet) ? d : null;
  return d.from !== undefined && step >= d.from && (d.facets === '*' || (d.facets ?? []).includes(facet)) ? d : null;
};

/** where two overlay masks differ (the first rows: page, y, the TS and Rust x range and count) */
const maskDiff = (ra, rb) => {
  const out = [];
  ra.forEach((pa, i) => {
    const pb = rb[i] ?? {};
    for (const y of [...new Set([...Object.keys(pa), ...Object.keys(pb)])].sort((u, v) => u - v)) {
      if (JSON.stringify(pa[y]) !== JSON.stringify(pb[y])) out.push(`p${i} y${y}: ts ${JSON.stringify(pa[y] ?? null)} rust ${JSON.stringify(pb[y] ?? null)}`);
    }
  });
  console.log(`  overlay mask: ${out.length} row(s) differ${out.length ? `; first: ${out.slice(0, 8).join('; ')}` : ''}`);
};

async function runFamily(fam) {
  const s = SCRIPTS[fam];
  const tally = { steps: 0, same: 0, diff: 0, declared: 0 };
  const used = new Set();
  for (const want of s.docs) {
    const n = docOf(want);
    const key = `${want.replace(/\.docx$/, '')}|${fam}`;
    const runs = {};
    for (const engine of ['ts', 'rust']) {
      await env.openPage();
      await env.page.evaluate(helpers);
      await env.page.context().grantPermissions(['clipboard-read', 'clipboard-write']);
      if (process.env.UO_ER_DUMP) await ev(() => { window.__dump = true; });
      if (process.env.UO_ER_NOLANG) await ev(() => { window.__nolang = true; });
      const errs0 = env.errors.length;
      await ev((e) => window.__es.make(e), engine);
      const pages = await ev((x) => window.__es.load(x), n);
      await ev(() => navigator.clipboard.writeText('ZZ').catch(() => null));
      const steps = [];
      for (const st of s.steps) {
        if (VERBOSE) console.log(`  ${key} ${engine} step ${JSON.stringify(st)}`);
        let answer = null;
        if (st.js) answer = await ev((c) => window.__es.js(c), st.js);
        else if (st.type) { await ev(() => window.__es.focus()); await env.page.keyboard.type(st.type); await ev(() => window.__es.settle(150)); }
        else if (st.key) { await ev(() => window.__es.focus()); await env.page.keyboard.press(st.key); await ev(() => window.__es.settle(150)); }
        else if (st.click) {
          // a real click (Ctrl held for `ctrl`) at the position the previous step chose
          const at = await ev(() => window.__es.clickPoint());
          if (at) {
            if (st.ctrl) await env.page.keyboard.down('Control');
            await env.page.mouse.click(at.x, at.y);
            if (st.ctrl) await env.page.keyboard.up('Control');
          }
          await ev(() => window.__es.settle(150));
          answer = at ? 'clicked' : 'no point';
        } else if (st.button) { answer = await ev((id) => window.__es.clickButton(id), st.button); const dl = await ev(() => window.__es.dialogs()); if (dl.length) answer += ` dialogs ${JSON.stringify(dl)}`; await ev(() => window.__es.closeDialogs()); }
        steps.push({ answer, facets: JSON.parse(await ev((f) => window.__es.facets(f), fam)) });
      }
      const end = await ev(() => window.__es.end());
      runs[engine] = { pages, steps, end, errors: env.errors.length - errs0 };
      await ev(() => window.__es.remove());
    }
    const a = runs.ts, b = runs.rust;
    const diffs = [];
    const check = (facet, x, y, step) => {
      tally[step === undefined ? 'end' : 'steps'] = (tally[step === undefined ? 'end' : 'steps'] ?? 0) + 1;
      const same = JSON.stringify(x) === JSON.stringify(y);
      if (same) { tally.same++; return; }
      tally.diff++;
      const d = declared(key, facet, step);
      // wave 7 (B): UO_ER_COVERED=1 lists the declared differences met (to narrow or retire a declaration)
      if (d) { tally.declared++; used.add(key); if (process.env.UO_ER_COVERED) console.log(`  declared ${key} ${facet}@${step ?? 'end'} (${d.id})`); return; }
      diffs.push({ facet, step, ts: x, rust: y });
    };
    if (a.errors || b.errors) fail(`${key}: page errors ts ${a.errors}, rust ${b.errors}`);
    check('pages after load', a.pages, b.pages);
    a.steps.forEach((sa, i) => {
      const sb = b.steps[i];
      check('answer', sa.answer, sb.answer, i);
      for (const k of new Set([...Object.keys(sa.facets), ...Object.keys(sb.facets)])) {
        const out = k === 'looks' ? looksOut(key) : [];
        if (out.length && sa.facets[k] && sb.facets[k]) {
          const strip = (v) => Object.fromEntries(Object.entries(v).filter(([id]) => !out.includes(id)));
          const [x, y] = [strip(sa.facets[k]), strip(sb.facets[k])];
          if (JSON.stringify(sa.facets[k]) !== JSON.stringify(sb.facets[k]) && JSON.stringify(x) === JSON.stringify(y)) { tally.diff++; tally.declared++; used.add(key); if (process.env.UO_ER_COVERED) console.log(`  declared ${key} looks@${i} (the declared looks only)`); continue; }
          check(k, x, y, i);
          continue;
        }
        check(k, sa.facets[k], sb.facets[k], i);
      }
    });
    check('end pages', a.end.pages, b.end.pages);
    check('screen (R overlays on)', a.end.screen, b.end.screen);
    check('screen (overlays off)', a.end.plain, b.end.plain);
    check('overlay mask', a.end.overlayMask, b.end.overlayMask);
    if (JSON.stringify(a.end.overlayMask) !== JSON.stringify(b.end.overlayMask)) maskDiff(a.end.maskRows, b.end.maskRows);
    if (process.env.UO_ER_DUMP) for (const [e, r] of Object.entries(runs)) r.end.shots.forEach((u, i) => { if (u) writeFileSync(join(outDir, `${key.replace(/[|]/g, '_')}-${e}-p${i}.png`), Buffer.from(u.split(',')[1], 'base64')); });
    const pa = parts(a.end.docx), pb = parts(b.end.docx);
    check('saved part names', Object.keys(pa).sort(), Object.keys(pb).sort());
    for (const p of Object.keys(pa)) if (p in pb) check(`saved ${p}`, pa[p], pb[p]);
    console.log(`${key}: ${s.steps.length} steps; overlays painted ts ${a.end.overlaysPainted} / rust ${b.end.overlaysPainted} page(s); ${diffs.length} undeclared difference(s)`);
    for (const d of diffs.slice(0, 12)) {
      // answers are JSON text: parsed; arrays: the first differing element
      const parse = (v) => { try { return typeof v === 'string' ? JSON.parse(v) : v; } catch { return v; } };
      const [pa1, pb1] = [parse(d.ts), parse(d.rust)];
      if (Array.isArray(pa1) && Array.isArray(pb1)) {
        const i = pa1.findIndex((x, k) => JSON.stringify(x) !== JSON.stringify(pb1[k]));
        d.ts = { [`[${i}] of ${pa1.length}`]: pa1[i] ?? null };
        d.rust = { [`[${i}] of ${pb1.length}`]: pb1[i] ?? null };
      }
      // objects: only the keys that differ (one level down)
      if (d.ts && d.rust && typeof d.ts === 'object' && typeof d.rust === 'object' && !Array.isArray(d.ts)) {
        const ks = [...new Set([...Object.keys(d.ts), ...Object.keys(d.rust)])].filter((k) => JSON.stringify(d.ts[k]) !== JSON.stringify(d.rust[k]));
        d.ts = Object.fromEntries(ks.map((k) => [k, d.ts[k]]));
        d.rust = Object.fromEntries(ks.map((k) => [k, d.rust[k]]));
      }
      const cut = (v) => { const t = typeof v === 'string' ? v : JSON.stringify(v); return t && t.length > 700 ? `${t.slice(0, 700)}…` : t; };
      console.log(`  DIFF ${d.facet}${d.step !== undefined ? ` @${d.step} ${JSON.stringify(s.steps[d.step])}` : ''}\n    ts   ${cut(d.ts)}\n    rust ${cut(d.rust)}`);
    }
    if (diffs.length) fail(`${key}: ${diffs.length} undeclared difference(s)`);
  }
  console.log(`${fam}: facets ${tally.same} equal, ${tally.diff} different (${tally.declared} declared: ${[...used].map((k) => `${k} ${(DECLARED[k] ?? DECLARED[`${k.split('|')[0]}|*`]).id}`).join(', ') || 'none'})`);
}

/** spelling: the misspellings of every paragraph on both engines (after both caches settle), then the F7 path */
/**
 * wave 7c (V, fix round 1, ED-304): Word's field shading and comment highlight (the Rust default; the TS look stays under
 * `tsShading`), checked against Word's screen (record "Wave 7c — V", fix round 1: field-shading.docx, Word 16 at 96 dpi):
 * "When selected" — no C0C0C0 with the caret outside the fields, the field the caret is in shaded alone; the band opaque
 * C0C0C0 under the glyphs from the line top, 17 px for the 11-pt lines (1.5 and single spacing) and 31 px for the line with
 * the 20-pt run; the comment band E8E8E8 of 17 px between 1-px brackets 424242; the band's x = the field's characters.
 */
async function runWordMarks() {
  await env.openPage();
  await env.page.evaluate(helpers);
  await ev(() => window.__es.make('rust'));
  await ev(() => { window.__es.el.rust.tsShading = false; });
  await ev((x) => window.__es.load(x), docOf('field-shading.docx'));
  const probe = (where) => ev(async (w) => {
    const el = window.__es.el;
    const sr = el.shadowRoot;
    // the caret: 'end' (the last paragraph) or 'f<k>' (2 characters into the k-th field result: 'Um campo |campo A…',
    // 'Um campo |campo B…', 'Grande XX e campo |campo C…')
    const pos = { end: [4, 3], f1: [0, 11], f2: [1, 11], f3: [2, 20] }[w];
    el.setSelection({ para: pos[0], offset: pos[1] });
    await window.__es.settle(200);
    window.clearInterval(el.caretTimer); el.caretVisible = false; el.paint();
    await new Promise((r) => requestAnimationFrame(() => r()));
    const cv = sr.querySelectorAll('canvas.page')[0];
    const g = cv.getContext('2d');
    const { width: W, height: H } = cv;
    const px = g.getImageData(0, 0, W, H).data;
    const at = (x, y) => { const i = (y * W + x) * 4; return [px[i], px[i + 1], px[i + 2]]; };
    const is = (c, v) => c[0] === v && c[1] === v && c[2] === v;
    // bands: runs of rows with >= 4 pixels of the colour (the glyphs cover the rest of a row); their x extent
    const bands = (v) => {
      const rows = [];
      for (let y = 0; y < H; y++) { let n = 0, x0 = W, x1 = -1; for (let x = 0; x < W; x++) if (is(at(x, y), v)) { n++; if (x < x0) x0 = x; x1 = x; } if (n >= 4) rows.push([y, x0, x1]); }
      const out = [];
      for (const r of rows) { const b = out[out.length - 1]; if (b && r[0] === b.y1 + 1) { b.y1 = r[0]; b.x0 = Math.min(b.x0, r[1]); b.x1 = Math.max(b.x1, r[2]); } else out.push({ y0: r[0], y1: r[0], x0: r[1], x1: r[2] }); }
      return out.map((b) => ({ ...b, h: b.y1 - b.y0 + 1 }));
    };
    return { c0: bands(192), e8: bands(232) };
  }, where);
  const out = {};
  for (const w of ['end', 'f1', 'f2', 'f3']) out[w] = await probe(w);
  await ev(() => window.__es.remove());
  console.log(`wordmarks: ${JSON.stringify(out)}`);
  const want = { f1: 17, f2: 17, f3: 31 };
  if (out.end.c0.length) fail(`wordmarks: C0C0C0 with the caret outside the fields: ${JSON.stringify(out.end.c0)}`);
  for (const [w, h] of Object.entries(want)) {
    const b = out[w].c0;
    if (b.length !== 1) fail(`wordmarks ${w}: ${b.length} C0C0C0 band(s), want 1`);
    else if (b[0].h !== h) fail(`wordmarks ${w}: band ${b[0].h} px, Word ${h} px`);
  }
  const cm = out.end.e8;
  if (cm.length !== 1 || cm[0].h !== 17) fail(`wordmarks: comment band ${JSON.stringify(cm)}, Word one band of 17 px`);
  console.log(`wordmarks: ${failures ? 'checked (failures above)' : 'Word bands equal (C0C0C0 17/17/31 px, "When selected"; comment E8E8E8 17 px)'}`);
  // wave 7c (V, fix round 2): "When selected" in the other stories — a QUOTE field in the footer and one in a footnote
  // (field-shading-stories.docx, 11-pt Liberation Sans, single spacing): no band with the caret outside the field, one
  // C0C0C0 band of the line's natural text height (17 px, as in the body) with the caret 2 characters into it
  const f0 = failures;
  await env.openPage();
  await env.page.evaluate(helpers);
  await ev(() => window.__es.make('rust'));
  await ev(() => { window.__es.el.rust.tsShading = false; });
  await ev((x) => window.__es.load(x), docOf('field-shading-stories.docx'));
  // the band found as the rows that change between the caret outside the field and inside it (the header/footer and note
  // edit tints their story over the page base: the band is C0C0C0 under that tint, not C0C0C0 itself)
  const story = (where) => ev(async (w) => {
    const el = window.__es.el;
    const sr = el.shadowRoot;
    const snap = async (off) => {
      // the footer's paragraph: 2 ('Ro|dape') / 15 (2 into 'campo R'); the note's: 3 / 15 (' Nota campo |campo N')
      if (w === 'ftr') { if (!el.editingHeaderFooter()) el.enterHeaderFooter('footer', 0); el.setSelection({ para: 0, offset: off ? 15 : 2 }); }
      else { if (el.editingHeaderFooter()) el.exitHeaderFooter(); if (!el.editingNote()) el.openNote('footnote', 1); el.setSelection({ para: 0, offset: off ? 15 : 3 }); }
      await window.__es.settle(200);
      window.clearInterval(el.caretTimer); el.caretVisible = false; el.paint();
      await new Promise((r) => requestAnimationFrame(() => r()));
      const cv = sr.querySelectorAll('canvas.page')[0];
      return { W: cv.width, H: cv.height, px: cv.getContext('2d').getImageData(0, 0, cv.width, cv.height).data };
    };
    // out, in, out again: the band's rows change in → out both ways; a row that also changes out → out again is something
    // else arriving meanwhile (the spelling's squiggles come from a worker), left out
    const a = await snap(false), b = await snap(true), a2 = await snap(false);
    const diff = (p, q, y) => { let n = 0; for (let x = 0; x < p.W; x++) { const i = (y * p.W + x) * 4; if (p.px[i] !== q.px[i] || p.px[i + 1] !== q.px[i + 1] || p.px[i + 2] !== q.px[i + 2]) n++; } return n; };
    const rows = [];
    for (let y = 0; y < a.H; y++) if (diff(a, b, y) >= 4 && diff(a2, b, y) >= 4 && diff(a, a2, y) === 0) rows.push(y);
    const bands = [];
    for (const y of rows) { const q = bands[bands.length - 1]; if (q && y === q.y1 + 1) q.y1 = y; else bands.push({ y0: y, y1: y }); }
    // and the out capture has no C0C0C0 (no field shading with the caret outside)
    let grey = 0;
    for (let i = 0; i < a.px.length; i += 4) if (a.px[i] === 192 && a.px[i + 1] === 192 && a.px[i + 2] === 192) grey++;
    return { ctx: el.editingHeaderFooter() ? 'hf' : el.editingNote() ? 'note' : 'body', bands: bands.map((q) => q.y1 - q.y0 + 1), greyOut: grey };
  }, where);
  const so = {};
  for (const w of ['ftr', 'note']) so[w] = await story(w);
  await ev(() => window.__es.remove());
  console.log(`wordmarks stories: ${JSON.stringify(so)}`);
  for (const [w, k] of [['ftr', 'hf'], ['note', 'note']]) {
    if (so[w].ctx !== k) fail(`wordmarks ${w}: editing ${so[w].ctx}, want ${k}`);
    else if (so[w].bands.length !== 1 || so[w].bands[0] !== 17) fail(`wordmarks ${w}: bands ${JSON.stringify(so[w].bands)}, want one of 17 px`);
  }
  if (failures === f0) console.log('wordmarks stories: footer and note fields shaded only with the caret in them (17 px)');
}

async function runSpell() {
  for (const want of ['f2-3-0.docx', 'f8__7__0.docx']) {
    const n = docOf(want);
    const out = {};
    for (const engine of ['ts', 'rust']) {
      await env.openPage();
      await env.page.evaluate(helpers);
      const errs0 = env.errors.length;
      if (process.env.UO_ER_DUMP) await ev(() => { window.__dump = true; });
      await ev((e) => window.__es.make(e), engine);
      await ev((x) => window.__es.load(x), n);
      const count = await ev(() => { const el = window.__es.el; el.setSelection({ para: 0, offset: 0 }); el.selectAll(); return el.getSelection().focus.para + 1; });
      const lists = await ev((c) => window.__es.spellLists(c), count);
      const f7 = await ev(async () => {
        const el = window.__es.el;
        el.setSelection({ para: 0, offset: 0 });
        const m = await el.nextMisspelling();
        if (!m) return { m: null };
        const at = el.spellingAtCaret();
        const sug = await el.spellingSuggestions(m, 3);
        const rep = el.replaceSpelling(m, sug[0] ?? m.word);
        const m2 = await el.nextMisspelling();
        el.ignoreSpelling(m2?.word ?? 'x');
        return { m, at, sug, rep, m2, sel: el.getSelection() };
      });
      // the caches settle again after the replacement and the Ignore All (the end capture paints the squiggles)
      await ev((c) => window.__es.spellLists(c), count);
      const end = await ev(() => window.__es.end());
      out[engine] = { count, lists, f7, end, errors: env.errors.length - errs0 };
      await ev(() => window.__es.remove());
    }
    const a = out.ts, b = out.rust;
    const same = (x, y) => JSON.stringify(x) === JSON.stringify(y);
    const pa = parts(a.end.docx), pb = parts(b.end.docx);
    const partDiff = Object.keys(pa).filter((p) => pa[p] !== pb[p]);
    const words = (a.lists.match(/:[a-zA-Z-]+"/g) ?? []).length;
    console.log(`  spell ${want}: ${words} misspelling(s) over the paragraphs; F7 word ${JSON.stringify(a.f7.m?.word ?? null)} → ${JSON.stringify(a.f7.sug ?? [])}`);
    for (const p of partDiff) {
      const x = new TextDecoder().decode(unzipSync(new Uint8Array(Buffer.from(a.end.docx, 'base64')))[p]);
      const y = new TextDecoder().decode(unzipSync(new Uint8Array(Buffer.from(b.end.docx, 'base64')))[p]);
      let i = 0;
      while (i < x.length && x[i] === y[i]) i++;
      console.log(`  ${p} first difference at ${i}:\n    ts   …${x.slice(Math.max(0, i - 200), i + 300)}\n    rust …${y.slice(Math.max(0, i - 200), i + 300)}`);
    }
    console.log(`spell ${want}: ${a.count} paragraphs; lists ${same(a.lists, b.lists) ? 'equal' : 'DIFFERENT'}; F7 ${same(a.f7, b.f7) ? 'equal' : 'DIFFERENT'}; squiggles (screen) ${same(a.end.screen, b.end.screen) ? 'equal' : 'DIFFERENT'}; saved parts different ${partDiff.length}; page errors ${a.errors}/${b.errors}`);
    if (!same(a.lists, b.lists)) { fail(`spell ${want}: misspelling lists differ`); console.log(`  ts   ${a.lists.slice(0, 600)}\n  rust ${b.lists.slice(0, 600)}`); }
    if (!same(a.f7, b.f7)) { fail(`spell ${want}: F7 path differs`); console.log(`  ts   ${JSON.stringify(a.f7)}\n  rust ${JSON.stringify(b.f7)}`); }
    if (process.env.UO_ER_DUMP) for (const [e, r] of Object.entries(out)) r.end.shots.forEach((u, i) => { if (u) writeFileSync(join(outDir, `spell-${want}-${e}-p${i}.png`), Buffer.from(u.split(',')[1], 'base64')); });
    console.log(`  spell ${want}: page base (squiggles off) ${same(a.end.plain, b.end.plain) ? 'equal' : 'DIFFERENT'}`);
    const dk = `${want.replace(/[.]docx$/, '')}|spell`;
    if (!same(a.end.screen, b.end.screen)) { if (declared(dk, 'screen (R overlays on)')) console.log(`  declared: ${dk} ${(DECLARED[dk] ?? DECLARED[`${dk.split('|')[0]}|*`]).id}`); else fail(`spell ${want}: screen with squiggles differs`); }
    // the squiggles' positions (the overlay mask) are compared even where the page base differs (ED-314)
    console.log(`  spell ${want}: overlay mask ${same(a.end.overlayMask, b.end.overlayMask) ? 'equal' : 'DIFFERENT'}`);
    if (!same(a.end.overlayMask, b.end.overlayMask)) { if (declared(dk, 'overlay mask')) console.log(`  declared: ${dk} overlay mask`); else fail(`spell ${want}: overlay mask differs`); }
    if (partDiff.length) fail(`spell ${want}: saved parts differ ${partDiff.join(', ')}`);
    if (a.errors || b.errors) fail(`spell ${want}: page errors`);
  }
}

/** memory (count build): create → load → the families' calls → remove; handles/sessions back to 0, live bytes flat */
async function runMemory() {
  await env.openPage();
  await env.page.evaluate(helpers);
  // UO_ER_TRACE=1 (wave 7c S, fix round 1): the module's exports wrapped (outermost calls only) to attribute each cycle's
  // net live bytes to the export that left them; a cycle whose live bytes differ from the one before prints the exports
  // whose net differs
  const TRACE = process.env.UO_ER_TRACE === '1';
  // wave 7c S, fix round 1 (the reviewer's major): the ribbon's Table Styles gallery lays its thumbnails out in batches of
  // 8 per frame until the element is removed, so how far a cycle gets is timing. The first layout that uses a face
  // variant (Table Classic 1: italic; Table Classic 3: bold italic; …) notes it missing, the host pushes it at the next
  // relayout, and the first layout after that parses it (uo-fonts `Pushed::face`, a OnceLock into the shared
  // `FaceCache`): +1,358 / +1,806 bytes once per page, in the page environment (`pageEnvs`, never an element's), bounded
  // by the pushed faces (4 variants a family). Measured: `s7d/bisect-thumb.mjs` (TC1: 86 B noted missing; after the push
  // TC1 again +1,358, a third time 0; TC3 the same +1,806) and the UO_ER_TRACE pairs (the soak's steps of +1,384, +1,376,
  // +1,816 are such first layouts, never repeated for a style). One more: cycle 0 pushes "Calibri" twice (the second push,
  // with the faces the thumbnails noted, REPLACES the family: `push_family`), and the element's live layout keeps the
  // first answer (uo-fonts `AnswerCache` holds an answer while a layout does), so the replaced Bold is parsed again
  // (+1,336 + its 24-byte argument = 1,360) by the first bold thumbnail of a LATER element — Plain Table 1 at cycle 2 in
  // one traced run, cycle 18 in another (`s7d/trace-font-*.log`: the pushes and the take_missing notes per cycle; no
  // push after cycle 0). So the warm-up runs in cycles 0 and 1 (a new element after every push): it lays every
  // thumbnail out, lets the host push, and lays them out again; UO_ER_WARM_THUMBS=0 runs the old soak.
  const WARM_THUMBS = process.env.UO_ER_WARM_THUMBS !== '0';
  if (TRACE) await env.page.evaluate(() => {
    const orig = WebAssembly.instantiate;
    window.__wtrace = new Map();
    // a handle or buffer an export returned (pointer → [export, net of that call]); its free closes the pair, and a
    // pair whose two nets do not cancel is memory the producer left outside the handle (a shared cache)
    const pending = new Map();
    window.__wresid = [];
    WebAssembly.instantiate = async (...a) => {
      const r = await orig.apply(WebAssembly, a);
      if (!r.instance) return r;
      const ex = r.instance.exports;
      const live = ex.uo_memory_live_bytes;
      if (typeof live !== 'function') return r;
      let depth = 0;
      const w = {};
      for (const [k, f] of Object.entries(ex)) {
        if (typeof f !== 'function' || k === 'uo_memory_live_bytes') { w[k] = f; continue; }
        const isFree = /_free$|^uo_free_bytes$/.test(k);
        w[k] = (...args) => {
          if (depth) return f(...args);
          depth++;
          const b = live();
          // the preview's JSON argument (the thumbnail it lays out), read before the call consumes it
          const str = (p, n) => new TextDecoder().decode(new Uint8Array(ex.memory.buffer, p, n));
          const tag = k === 'uo_shell_preview' ? `(${args[2]} ${str(args[3], args[4])})` : '';
          // the font traffic: the families pushed, the (family, variants) the engine noted missing
          if (k === 'uo_write_env_push_family') window.__wresid.push(`push ${str(args[1], args[2])}`);
          let ret;
          try { ret = f(...args); return ret; } finally {
            depth--;
            const d = live() - b;
            if (k === 'uo_write_env_take_missing' && ret) {
              const m = str(ex.uo_buf_ptr(ret), ex.uo_buf_len(ret)).trim();
              if (m) window.__wresid.push(`missing ${m.replace(/\n/g, ' / ').replace(/\t/g, ':')}`);
            }
            if (isFree) {
              const p = pending.get(args[0]);
              if (p) { pending.delete(args[0]); if (p[1] + d !== 0) window.__wresid.push(`${p[0]}${p[2]}→${k} ${p[1] + d}`); }
            } else if (typeof ret === 'number' && ret > 65536 && d > 0) pending.set(ret, [k, d, tag]);
            const e = window.__wtrace.get(k) ?? [0, 0];
            e[0]++; e[1] += d;
            window.__wtrace.set(k, e);
          }
        };
      }
      return { module: r.module, instance: { exports: w } };
    };
  });
  const traces = [];
  const n = docOf('revisions.docx');
  const lives = [];
  // the TS reference (first, in the same page: it also warms the spell engine's module): its disconnectedCallback releases
  // the shared checker the same way; a remove while the checker's engine is still starting leaves the worker of the
  // released instance alive on both engines (shared/spell's startup race, gate R-7), so the warm cycles are the check
  const tsRef = await ev(async (x) => {
    const es = window.__es;
    await es.make('ts');
    await es.load(x);
    const el = es.el;
    el.paint();
    for (let k = 0; k < 200 && !el.spellEngine; k++) { await new Promise((r2) => setTimeout(r2, 25)); el.paint(); }
    const eng = el.spellEngine;
    // the checks reach the worker (a dictionary loaded) before the remove
    for (let k = 0; k < 400 && eng && !eng.loaded.size; k++) await new Promise((r2) => setTimeout(r2, 25));
    await new Promise((r2) => setTimeout(r2, 300));
    const during = eng ? { refs: eng.refs, listeners: eng.listeners.size } : null;
    es.remove();
    await new Promise((r2) => setTimeout(r2, 0));
    return { during, after: eng ? { refs: eng.refs, listeners: eng.listeners.size, transport: !!eng.transport } : null };
  }, n);
  console.log(`memory: TS reference: spell checker during ${JSON.stringify(tsRef.during)} after ${JSON.stringify(tsRef.after)}`);
  for (let i = 0; i < SOAK; i++) {
    const r = await ev(async ({ x, warmThumbs }) => {
      const es = window.__es;
      await es.make('rust');
      await es.load(x);
      const el = es.el;
      el.setSelection({ para: 0, offset: 0 }, { para: 0, offset: 3 });
      el.addComment('m');
      el.acceptAllChanges();
      el.replaceAllMatches({ text: 'a' }, 'b');
      el.listStyles('all');
      el.search('e');
      el.getRevisions();
      el.paint();
      // the spell checker (shared, a Worker) loads on the first paint: waited for, so the remove has to release it
      const st = el.rust.reviewProbe.spellState;
      for (let k = 0; k < 200 && !st.engine; k++) { await new Promise((r2) => setTimeout(r2, 25)); el.paint(); }
      const eng = st.engine;
      // past the checker's engine startup (a dictionary loaded; gate R-7: a remove during it leaves the released instance's worker)
      for (let k = 0; k < 400 && eng && !eng.loaded.size; k++) await new Promise((r2) => setTimeout(r2, 25));
      const spellDuring = eng ? { refs: eng.refs, listeners: eng.listeners.size } : null;
      if (warmThumbs) {
        // every Table Styles gallery thumbnail laid out, the faces they noted missing pushed by the host (a relayout's
        // take_missing), and laid out again (see WARM_THUMBS)
        const all = async () => {
          for (const s of el.tableStyleListNow() ?? []) {
            const cv = document.createElement('canvas');
            cv.width = 66; cv.height = 46;
            await el.renderTableStyleThumb(cv, s.name);
          }
        };
        await all();
        el.refresh();
        await es.settle();
        await all();
      }
      const during = es.stats();
      es.remove();
      await new Promise((r2) => setTimeout(r2, 0));
      const spellAfter = eng ? { refs: eng.refs, listeners: eng.listeners.size, transport: !!eng.transport, held: !!st.engine || !!st.spell } : null;
      return { during, after: es.stats(), spellDuring, spellAfter };
    }, { x: n, warmThumbs: WARM_THUMBS && i < 2 });
    // wave 7c (S): the removed element keeps its document until it is collected (as the TS element its model): GC first
    r.after = await env.gc();
    lives.push(r.after.live);
    if (TRACE) {
      const t = await env.page.evaluate(() => { const o = Object.fromEntries(window.__wtrace); window.__wtrace.clear(); return { o, res: window.__wresid.splice(0) }; });
      traces.push(t.o);
      // the producer/free pairs that left memory (a positive sum: what a call kept outside its handle or buffer; a negative
      // one is the call's consumed input buffer)
      const kept = t.res.filter((x) => !/ -\d+$/.test(x));
      if (kept.length) console.log(`memory trace: cycle ${i}: ${kept.length} pair(s) left memory: ${kept.join('; ')}`);
      if (i > 0 && lives[i] !== lives[i - 1]) {
        const a = traces[i - 1], b = traces[i];
        const diffs = [...new Set([...Object.keys(a), ...Object.keys(b)])]
          .map((k) => [k, a[k] ?? [0, 0], b[k] ?? [0, 0]])
          .filter(([, x, y]) => x[1] !== y[1] || x[0] !== y[0])
          .map(([k, x, y]) => `${k} calls ${x[0]}→${y[0]} net ${x[1]}→${y[1]}`);
        console.log(`memory trace: cycle ${i}: live ${lives[i - 1]} → ${lives[i]} (${lives[i] - lives[i - 1]}): ${diffs.join('; ')}`);
      }
    }
    if (r.after.sessions || r.after.shellEngines || r.after.handles) fail(`memory: cycle ${i}: after remove ${JSON.stringify(r.after)}`);
    if (!r.spellAfter) fail(`memory: cycle ${i}: the spell checker never loaded`);
    else if (r.spellAfter.refs || r.spellAfter.listeners || r.spellAfter.transport || r.spellAfter.held) fail(`memory: cycle ${i}: the spell checker is held after remove ${JSON.stringify(r.spellAfter)}`);
    if (i === 0 || i === SOAK - 1) console.log(`memory: cycle ${i}: spell checker during ${JSON.stringify(r.spellDuring)} after ${JSON.stringify(r.spellAfter)}`);
    if (i === 0 || i === SOAK - 1) console.log(`memory: cycle ${i}: during ${JSON.stringify(r.during)} after ${JSON.stringify(r.after)}`);
  }
  if (TRACE) console.log(`memory trace: per cycle live/previews: ${lives.map((l, i) => `${l}/${traces[i]?.uo_shell_preview?.[0] ?? 0}`).join(' ')}`);
  const warm = lives.slice(Math.min(5, lives.length - 1));
  const flat = warm.every((x) => x === warm[0]);
  console.log(`memory: ${SOAK} cycles; live bytes after warm-up ${flat ? `flat at ${warm[0]}` : `NOT flat: ${warm.join(', ')}`}`);
  if (!flat && warm[0] !== null) fail('memory: live bytes not flat');
}

const only = process.env.UO_ER_ONLY ? new RegExp(process.env.UO_ER_ONLY) : null;
if (only) for (const s of Object.values(SCRIPTS)) s.docs = s.docs.filter((d) => only.test(d));
// UO_ER_STEPS=n: only the first n steps of each script (bisecting a difference)
if (process.env.UO_ER_STEPS) for (const s of Object.values(SCRIPTS)) s.steps = s.steps.slice(0, Number(process.env.UO_ER_STEPS));
const fams = MODE === 'all' ? Object.keys(SCRIPTS) : Object.keys(SCRIPTS).includes(MODE) ? [MODE] : [];
const t0 = Date.now();
for (const f of fams) await runFamily(f);
if (MODE === 'spell' || MODE === 'all') await runSpell();
if (MODE === 'wordmarks' || MODE === 'all') await runWordMarks();
if (MODE === 'memory') await runMemory();
await env.close();
console.log(`editor_switch_review (${MODE}): ${((Date.now() - t0) / 1000).toFixed(0)} s; ${failures ? `${failures} FAILURE(S)` : 'ok'}`);
process.exit(failures ? 1 : 0);
