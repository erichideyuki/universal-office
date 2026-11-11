// D2b-F8 (the clipboard) in the D2 editor harness (G17, U16): the frozen `<universal-write>` in
// headless Chromium driven by seeded scripts of copy, cut, paste (system clipboard stubbed), pasteData
// of the rich copy / HTML / text, pasteText, repaste, pasteCells, Paste Special (with Paste Link),
// the Office Clipboard (item, all, remove, clear), the paste defaults and the document path, with the
// D2a text ops, cells and undo/redo. Four documents are loaded one after the other on one element
// (the Office Clipboard of the page persists: pastes between documents). Set-up as
// capture-d2b-f9-session.mjs (open default font, `Math.random` = xorshift32, `Date` fixed, UTC,
// pt-BR); the paste payloads are made in the page by js/paste-tree.mjs.
//
//   UO_TS_ROOT=<frozen built checkout> UO_CHROMIUM=<chrome-headless-shell.exe> \
//     node core/crates/uo-docx/js/capture-d2b-f8-session.mjs [--docs N] [--steps N]
//
// Writes tests/parity/d2b-f8-session.capture: per `<doc>#<j>` the D2a facets (op, paras, sel, anc,
// ev, flags, docxml) and F8's: `pt` (the payload of an HTML step, base64), `fx` (the PNG and size
// renderClipPng gave), `cbw` (what the copy wrote: text and HTML digests), `clip` (the rich copy:
// text digest, paragraphs, kind), `ring` (the Office Clipboard items without `at`), `po`
// (pasteOptions()), `psf` (pasteSpecialFormats()), `nv`/`sv` (numbering/styles part digests),
// `rc` (Math.random calls of the step), `hf` (an `eh` step: the part path entered and the part created);
// per document `#rs` (the random state after the load, `rs0` before it when the load drew from it, the
// group `g`, `x` for an extended group, `doc` the bytes of a synthetic document).
//
// Extended groups (`--extra N`, default 4; UO_F8_NO_EXTRA=1 for none): 4 synthetic documents built in the
// page by the TS API (custom inlines, notes, a header, a table, a text box) and corpus documents with
// floats, notes and headers, with EXT_OPS: the formatting policy (`fp`), a selected float then copy/cut
// (`so`), a range around a note reference / custom inline / object then copy/cut (`sm`), the note and
// header/footer contexts (`on`/`xn`, `eh`/`xh`); `xf+` drops the float selection before a step. Steps that
// reach another family's open behaviour are mapped to an exit of the part (see F8.md F8-O3/F8-O4).
// Debugging: UO_F8_ONLY=<rel part> with UO_F8_OUT=<file> (not the versioned capture), UO_F8_XML=<dir>
// (document.xml and numbering dumps), UO_F8_RING=1 (raw ring facet), UO_F8_DBG=1.
import { createRequire, stripTypeScriptTypes } from 'node:module';
import { createServer } from 'node:http';
import { readFileSync, existsSync, readdirSync, writeFileSync } from 'node:fs';
import { join, extname, normalize } from 'node:path';
import { createHash } from 'node:crypto';
import { CaptureWriter, Rng, Stream } from '../../uo-testkit/js/testkit.mjs';
import { Canon } from './canon.mjs';
import { crate, corpus, readBytes, root, ts } from './capture-r2-common.mjs';

const Digest = { of: (b) => ({ short: createHash('sha256').update(b).digest('hex').slice(0, 32) }) };
const argv = process.argv.slice(2);
const opt = (k, d) => (argv.includes(k) ? Number(argv[argv.indexOf(k) + 1]) : d);
const DOCS = opt('--docs', 40);
const STEPS = opt('--steps', 60);
const GROUP = 4;
const { DocxPackage } = await ts('package.js');

const pwFrom = [join(root, 'slides', 'package.json'), join(root, 'sheet', 'package.json')].find((p) => existsSync(p));
const { chromium } = createRequire(pwFrom)('playwright');

const pasteHtmlJs = stripTypeScriptTypes(readFileSync(join(root, 'write/packages/editor/src/paste-html.ts'), 'utf8'));
const treeJs = readFileSync(join(crate, 'js', 'paste-tree.mjs'), 'utf8');
const policyJs = stripTypeScriptTypes(readFileSync(join(root, 'write/packages/editor/src/formatting-policy.ts'), 'utf8'));
const MIME = { '.js': 'text/javascript', '.mjs': 'text/javascript', '.html': 'text/html', '.json': 'application/json', '.wasm': 'application/wasm', '.css': 'text/css', '.ttf': 'font/ttf', '.otf': 'font/otf', '.woff2': 'font/woff2' };
const server = createServer((req, res) => {
  const url = decodeURIComponent((req.url ?? '/').split('?')[0]);
  const send = (type, body) => { res.writeHead(200, { 'content-type': type }); res.end(body); };
  if (url === '/__d2b.html') {
    send('text/html', '<!doctype html><meta charset="utf-8"><body><script type="module">import "/write/packages/editor/publish/index.js"; import * as ph from "/__paste-html.js"; import * as pt from "/__paste-tree.js"; import * as fp from "/__fp.js"; window.__ph = ph; window.__ptree = pt; window.__fp = fp; window.__ready = true;</script></body>');
    return;
  }
  if (url === '/__paste-html.js') { send('text/javascript', pasteHtmlJs); return; }
  if (url === '/__paste-tree.js') { send('text/javascript', treeJs); return; }
  if (url === '/__fp.js') { send('text/javascript', policyJs); return; }
  const file = normalize(join(root, url));
  if (!file.startsWith(normalize(root)) || !existsSync(file)) { res.writeHead(404); res.end(); return; }
  send(MIME[extname(file)] ?? 'application/octet-stream', readFileSync(file));
});
await new Promise((r) => server.listen(0, '127.0.0.1', r));
const port = server.address().port;

const installed = (() => {
  const base = join(process.env.LOCALAPPDATA ?? '', 'ms-playwright');
  if (!existsSync(base)) return undefined;
  const dirs = readdirSync(base).filter((d) => d.startsWith('chromium_headless_shell-')).sort();
  const last = dirs[dirs.length - 1];
  const exe = last && join(base, last, 'chrome-headless-shell-win64', 'chrome-headless-shell.exe');
  return exe && existsSync(exe) ? exe : undefined;
})();
const browser = await chromium.launch({ headless: true, executablePath: process.env.UO_CHROMIUM ?? installed });
const context = await browser.newContext({ timezoneId: 'UTC', locale: 'pt-BR' });
await context.addInitScript(() => {
  window.__rs = 1;
  window.__rc = 0;
  Math.random = () => { let s = window.__rs; s ^= s << 13; s >>>= 0; s ^= s >>> 17; s ^= s << 5; s >>>= 0; window.__rs = s; window.__rc++; return s / 4294967296; };
  const T = Date.UTC(2026, 0, 2, 3, 4, 0);
  const D = Date;
  // eslint-disable-next-line no-global-assign
  Date = class extends D { constructor(...a) { if (a.length) super(...a); else super(T); } static now() { return T; } };
  // the system clipboard: writes are logged, reads give the step's items
  window.__cbw = null;
  window.__cbItems = [];
  const blob = (s, t) => new Blob([s], { type: t });
  const cb = {
    async write(items) { const it = items[0]; const t = await (await it.getType('text/plain')).text(); const h = it.types.includes('text/html') ? await (await it.getType('text/html')).text() : null; window.__cbw = { t, h }; },
    async writeText(t) { window.__cbw = { t, h: null }; },
    async read() { return window.__cbItems.map((d) => ({ types: Object.keys(d), getType: async (k) => blob(d[k], k) })); },
    async readText() { return window.__cbItems[0]?.['text/plain'] ?? ''; },
  };
  Object.defineProperty(navigator, 'clipboard', { value: cb, configurable: true });
});

const EVENTS = ['uw-change', 'uw-dirty-change', 'uw-input', 'uw-track-changes', 'uw-cellmode', 'uw-notemode', 'uw-hfmode', 'uw-tbmode', 'uw-protected', 'uw-blocked', 'uw-styles-change', 'uw-pasted'];
const TEXTS = ['a', 'Olá ', ' ', 'ção', 'abc def', '1. ', 'x'];
const MODES = ['keep', 'merge', 'text', 'picture', 'default', ''];
const LISTS = ['continue', 'new', 'merge', 'nomerge', ''];
const FORMATS = ['rtf', 'text', 'picture', 'html', 'unicode', 'image'];
const CELLS = ['overwrite', 'rows', 'nest', 'text'];
const PATHS = ['C:\\Docs\\fonte.docx', '/home/u/a b.docx', ''];
/** The second policy of the `fp` step (mirrored in tests/parity_d2b_f8_session.rs `f8_policy`). */
const F8_POLICY = {
  paragraphStyles: ['Normal', 'Heading1', 'Heading 2', 'Title'],
  lists: { kinds: ['bullet'], levels: 2 },
  character: ['bold', 'italic', 'color'],
  indentation: false,
  images: false,
  shapes: false,
  tables: false,
  font: false,
  fontSize: false,
};
const hex = (t) => [...Array(t.length).keys()].map((i) => t.charCodeAt(i).toString(16)).join('.');
/** The HTML pieces of the ph/pv steps (shared by index with tests/parity_d2b_f8_session.rs: none; the payload is in the capture). */
const HTMLS = [
  '<p>Plain <b>bold</b> and <i>italic</i></p>',
  '<h1>Title</h1><h2>Sub</h2><p>Body text</p>',
  '<ul><li>one</li><li>two<ul><li>nested</li></ul></li></ul><ol><li>first</li><li>second</li></ol>',
  '<p class=MsoListParagraph style="mso-list:l0 level1 lfo1"><![if !supportLists]><span style="mso-list:Ignore">1.<span> </span></span><![endif]>Word item</p>',
  '<table><tr><th>H</th><th>I</th></tr><tr><td>1</td><td>2</td></tr></table>',
  '<p>before</p><table><tr><td>a</td></tr></table>',
  '<p><a href="https://example.com/x">a link</a> after</p>',
  '<p style="text-align:center;color:#c00000;font-family:Georgia;font-size:14pt">styled</p><pre>code\n  block</pre>',
  'single line no block',
  '<div>one</div><div>two</div><p class=MsoTitle>Doc title</p><p class=MsoSubtitle>sub</p>',
];

/** The op table of the extended groups (null: the base table's pick stays). */
function extOp(rng, I, pos, md) {
  const k = rng.below(100);
  if (k < 35) return null;
  if (k < 40) return `fp:${rng.below(3)}`;
  // (a cell edit indexes the body's objects in the TS `contextDrawingRuns`: another family's matter)
  if (k < 52 && I.dr && (I.ctx === 'h' || (I.ctx === 'b' && !I.cell))) return `so:${rng.below(I.dr)}:${'cun'[rng.below(3)]}`;
  if (k < 62 && I.marks.length) {
    // a range around a note reference, a custom inline or an object (then copy or cut)
    const [p, o] = I.marks[rng.below(I.marks.length)];
    const L = I.lens[p] ?? 0;
    const a = Math.max(0, o - rng.below(4)), f = Math.min(L, o + 1 + rng.below(4));
    const b = I.lens.length > p + 1 && rng.below(3) === 0 ? { para: p + 1, offset: 0 } : { para: p, offset: f };
    return `sm:${p}:${a}:${b.para}:${b.offset}:${'cun'[rng.below(3)]}`;
  }
  // (a body table selection survives openNote/enterHeaderFooter in the TS: kept out, see F8.md F8-O3)
  const free = I.ctx === 'b' && !I.cell && !I.tsel;
  if (k < 70 && I.refs.length && free) { const [kind, id] = I.refs[rng.below(I.refs.length)].split(':'); return `on:${kind}:${id}`; }
  if (k < 72 && I.ctx === 'n') return 'xn';
  // (only a kind the document has: a part created on entry defines its style outside the undo
  // step, and the TS undo drops it with the snapshot: another family's matter, see F8.md F8-O3)
  if (k < 79 && free && I.hfk.length) return `eh:${I.hfk[rng.below(I.hfk.length)]}`;
  if (k < 82 && I.ctx === 'h') return 'xh';
  if (k < 90 && I.clip !== null) return `pr:${md()}:`;
  return null;
}

function digest(paras) {
  const s = new Stream();
  const c = new Canon(s);
  paras.forEach((p, i) => { s.tag('E').u32(i); c.value(p, 'paragraph'); });
  return s.finish().short;
}
const sha = (t) => Digest.of(Buffer.from(t ?? '', 'utf8')).short;
const revive = (v) => (v && typeof v === 'object' ? (v.__u8 !== undefined ? Uint8Array.from(Buffer.from(v.__u8, 'base64')) : (Array.isArray(v) ? v.map(revive) : Object.fromEntries(Object.entries(v).map(([k, x]) => [k, revive(x)])))) : v);

const out = new CaptureWriter([
  'D2b-F8 session in Chromium: the frozen <universal-write> driven by seeded clipboard scripts (four documents per page, one Office Clipboard)',
  'facets: op, paras, sel, anc, ev, flags, docxml, pt, fx, cbw, clip, ring, po, psf, nv, sv, rc',
]);
out.meta('steps', STEPS);
out.meta('group', GROUP);
out.meta('browser', `chromium ${browser.version()}`);
out.meta('bundle', Digest.of(readFileSync(join(root, 'write/packages/editor/publish/index.js'))).short);
out.meta('math-override', 'none (no F8 value goes through Math.sin/cos/…; Math.random = xorshift32)');

function pick() {
  const rich = [], plain = [];
  for (const f of corpus()) {
    let t = '';
    try { t = DocxPackage.load(readBytes(f.file)).getPartText('word/document.xml') ?? ''; } catch { continue; }
    if (t.length > 400000) continue;
    (/<w:(numPr|tbl|footnoteReference)\b/.test(t) ? rich : plain).push(f);
  }
  const take = (list, n, mod) => list.filter((_, i) => i % mod === 0).slice(0, n);
  return [...take(rich, Math.ceil(DOCS * 0.75), 4), ...take(plain, Math.floor(DOCS * 0.25), 29)].slice(0, DOCS);
}
const process_env_full = !!process.env.UO_F8_FULL;
const files = pick();
// ── the extended groups (fix round 1): the formatting policy, a selected float before copy/cut,
// the note and header/footer contexts, note references and custom inlines in copied ranges. The
// base groups above keep their op table (their scripts are unchanged); these use EXT_OPS. ──
const EXTRA = process.env.UO_F8_NO_EXTRA ? 0 : opt('--extra', 4);
/** Synthetic documents (built on the element by the TS API, saved, and loaded from those bytes; the
 * bytes are in the capture, `#rs` facet `doc`): custom inlines (a registered and an unregistered
 * type), footnotes and endnotes, a header, a table, a text box. */
const SYN = 4;
const groups = [];
for (let g = 0; g < files.length; g += GROUP) groups.push({ g: String(g), x: false, docs: files.slice(g, g + GROUP) });
if (EXTRA > 0) {
  groups.push({ g: 'x0', x: true, docs: [...Array(SYN).keys()].map((k) => ({ rel: `syn/f8-${k}.docx`, syn: k })) });
  // corpus documents with floating objects and notes (not in the base set)
  const taken = new Set(files.map((f) => f.rel));
  const floats = [], heads = [];
  for (const f of corpus()) {
    // (cellout/, txbxlink/: the element's load differs from the session's at the first step, not F8's: see F8.md F8-O3)
    if (taken.has(f.rel) || f.rel.includes('/cellout/') || f.rel.includes('/txbxlink/')) continue;
    let t = '';
    try { t = DocxPackage.load(readBytes(f.file)).getPartText('word/document.xml') ?? ''; } catch { continue; }
    if (t.length > 300000) continue;
    if (/<wp:anchor\b/.test(t) && /<w:(footnoteReference|tbl)\b/.test(t)) floats.push(f);
    else if (/<w:headerReference\b/.test(t) && /<w:footnoteReference\b/.test(t)) heads.push(f);
  }
  const every = (list, n) => list.filter((_, i) => i % Math.max(1, Math.floor(list.length / n)) === 0).slice(0, n);
  const more = [...every(floats, GROUP * (EXTRA - 1) - Math.floor(GROUP / 2)), ...every(heads, Math.floor(GROUP / 2))];
  for (let i = 0; i < more.length; i += GROUP) groups.push({ g: `x${1 + i / GROUP}`, x: true, docs: more.slice(i, i + GROUP) });
}
if (process.env.UO_F8_ONLY) for (const gr of groups) gr.docs = gr.docs.filter((d) => d.rel.includes(process.env.UO_F8_ONLY));
let n = 0;
for (const { g, x: ext, docs } of groups) {
  const page = await context.newPage();
  await page.goto(`http://127.0.0.1:${port}/__d2b.html`);
  await page.waitForFunction(() => window.__ready === true);
  await page.evaluate(async ([EVENTS, ext]) => {
    const el = document.createElement('universal-write');
    el.setAttribute('fonts', '/write/packages/editor/demo/fonts');
    const ready = new Promise((r) => el.addEventListener('uw-ready', r, { once: true }));
    document.body.append(el);
    await ready;
    window.__el = el;
    window.__ev = [];
    // the extended groups: a plugin with one inline type (the session's SetInlineTypes)
    if (ext) el.use({ id: 'f8', inlineRenderers: [{ type: 'f8.tag' }] });
    for (const t of EVENTS) el.addEventListener(t, (e) => { const d = e.detail ?? {}; window.__ev.push(`${t}:${d.dirty ?? d.active ?? d.on ?? ''}`); });
    // Paste as Picture: what renderClipPng gave, and the promise to wait for
    const rcp = el.renderClipPng.bind(el);
    el.renderClipPng = async (paras) => { const r = await rcp(paras); let bs = ''; if (r) for (let i = 0; i < r.data.length; i += 0x8000) bs += String.fromCharCode(...r.data.subarray(i, i + 0x8000)); window.__fx.push(r ? { b64: btoa(bs), w: r.w, h: r.h } : null); return r; };
    const pp = el.pastePicture.bind(el);
    el.pastePicture = (d) => { const p = pp(d); window.__wait.push(p); return p; };
    const ii = el.insertImage.bind(el);
    el.insertImage = (...a) => { const p = ii(...a); window.__wait.push(p); return p; };
  }, [EVENTS, ext]);
  for (const { rel, file, syn } of docs) {
    // a synthetic document: built on the element with the TS API, then saved; both sides load the bytes
    let bytes = file ? readBytes(file) : null;
    if (syn !== undefined) {
      const b64 = await page.evaluate(async (k) => {
        const el = window.__el;
        const step = (f) => { try { f(); } catch (e) { console.log(`syn ${k}: ${e}`); } };
        el.author = 'A';
        el.newDocument();
        step(() => el.type(k % 2 ? 'Alfa beta ' : 'Alpha beta '));
        step(() => el.insertCustomInline('f8.tag', { n: k, s: 'x' }, { fallbackText: '[tag]' }));
        step(() => el.type(' gamma delta'));
        step(() => { el.insertFootnote(k === 3 ? { mark: '*' } : {}); el.type('Note one text'); el.exitNote(); });
        step(() => el.type(' after the note.'));
        step(() => el.pressEnter());
        step(() => el.type('Second '));
        step(() => el.insertCustomInline('f8.other', ['a', k], { fallbackText: 'other' }));
        step(() => el.type(' end'));
        step(() => { el.insertEndnote(); el.type('End note'); el.exitNote(); });
        step(() => el.pressEnter());
        step(() => el.type('Third para with words'));
        if (k !== 1) step(() => { el.pressEnter(); el.insertTable(2, 2); });
        step(() => { if (el.cellEdit) el.exitCell(); });
        step(() => { el.enterHeaderFooter(k === 2 ? 'footer' : 'header', 0); el.type(`Head ${k} `); el.insertCustomInline('f8.tag', { h: k }, { fallbackText: '[h]' }); el.exitHeaderFooter(); });
        if (k >= 2) step(() => { el.insertTextbox('simple'); });
        step(() => { if (el.tbEdit) el.exitTextbox(); if (el.hfEdit) el.exitHeaderFooter(); if (el.noteEdit) el.exitNote(); if (el.cellEdit) el.exitCell(); });
        const b = el.saveDocx();
        let s = ''; for (let i = 0; i < b.length; i += 0x8000) s += String.fromCharCode(...b.subarray(i, i + 0x8000));
        return btoa(s);
      }, syn);
      bytes = Uint8Array.from(Buffer.from(b64, 'base64'));
    }
    const ok = await page.evaluate(async (b64) => {
      const el = window.__el;
      try {
        el.author = 'A';
        // the random state before the load (a load that numbers linked boxes draws from it)
        window.__rs0 = window.__rs;
        el.loadDocx(Uint8Array.from(atob(b64), (c) => c.charCodeAt(0)), { protected: false });
        // the Paste Special data of the previous document goes (a new document in the shell)
        el.specialData = null;
        return { ok: !!el.fonts, rs: window.__rs, rs0: window.__rs0 };
      } catch (e) { return { ok: false }; }
    }, Buffer.from(bytes).toString('base64'));
    if (!ok.ok) continue;
    out.set(`${rel}#rs`, 'rs', String(ok.rs));
    out.set(`${rel}#rs`, 'g', String(g));
    if (ok.rs0 !== ok.rs) out.set(`${rel}#rs`, 'rs0', String(ok.rs0));
    if (ext) out.set(`${rel}#rs`, 'x', '1');
    if (syn !== undefined) out.set(`${rel}#rs`, 'doc', Buffer.from(bytes).toString('base64'));
    let seed = 0xf8e8;
    for (let i = 0; i < rel.length; i++) seed = (Math.imul(seed, 31) + rel.charCodeAt(i)) >>> 0;
    const rng = new Rng(seed);
    const info = () => page.evaluate(() => {
      const el = window.__el;
      return {
        lens: el.model.paragraphs.map((p) => p.runs.reduce((a, r) => a + r.text.length, 0)),
        tables: el.tableAnchors.length,
        cell: !!el.cellEdit,
        ring: el.clipboardItems().length,
        clip: el.richClip ? el.richClip.text : null,
        rows: el.tableAnchors.map((a) => a.table.rows.length),
        ctx: el.hfEdit ? 'h' : el.noteEdit ? 'n' : el.tbEdit ? 't' : 'b',
        tsel: !!el.tableSel,
        fs: !!el.floatSel,
        cells: !!el.richClip?.cells,
        hfk: ['header', 'footer'].filter((kd) => [...el.hfParts.keys()].some((x) => x.startsWith(`${kd}:`))).map((kd) => kd[0]),
        dr: (() => { try { return el.contextDrawingRuns(el.model).length; } catch { return 0; } })(),
        refs: (() => { try { return el.noteRefs().map((r) => `${r.kind === 'endnote' ? 1 : 0}:${r.id}`); } catch { return []; } })(),
        // note references, custom inlines and objects in the context's paragraphs: [para, offset]
        marks: el.model.paragraphs.flatMap((p, pi) => { let o = 0; const m = []; for (const r of p.runs) { if (r.noteRef || r.custom || r.image || r.textbox || r.shape || r.group) m.push([pi, o]); o += r.text.length; } return m; }),
      };
    });
    for (let j = 0; j <= STEPS; j++) {
      const key = `${rel}#${j}`;
      let code = 'init';
      let html = null;
      if (j > 0) {
        const I = await info();
        const L = I.lens;
        const pos = () => { const para = rng.below(L.length); return { para, offset: rng.below(L[para] + 1) }; };
        const k = rng.below(100);
        const md = () => MODES[rng.below(MODES.length)];
        if (k < 7) code = `ty:${hex(TEXTS[rng.below(TEXTS.length)])}`;
        else if (k < 9) code = 'en';
        else if (k < 11) code = 'bs';
        else if (k < 22) { const a = pos(); const f = rng.below(4) === 0 ? a : pos(); code = `se:${a.para}:${a.offset}:${f.para}:${f.offset}`; }
        else if (k < 23) code = `tk:${rng.below(2)}`;
        else if (k < 33) code = 'co';
        else if (k < 37) code = 'cu';
        else if (k < 45 && I.clip !== null) code = `pr:${md()}:${LISTS[rng.below(LISTS.length)]}`;
        else if (k < 52) { code = `ph:${md()}`; html = HTMLS[rng.below(HTMLS.length)]; }
        else if (k < 55) code = `pt:${md()}:${hex(['a\nb', 'one two', 'x\r\ny\rz', ''][rng.below(4)])}`;
        else if (k < 57) code = `px:${hex(['pasted', 'p\nq', ''][rng.below(3)])}`;
        else if (k < 62) { code = `pv:${md()}:${rng.below(3)}`; html = HTMLS[rng.below(HTMLS.length)]; }
        else if (k < 66) code = `rp:${MODES[rng.below(5)]}:${LISTS[rng.below(LISTS.length)]}`;
        else if (k < 69 && I.tables) { const a = rng.below(I.tables); const rows = I.rows[a] || 1; code = `ts:${a}:${rng.below(rows)}:0:${rng.below(rows)}:${rng.below(3)}`; }
        else if (k < 72) code = `pc:${CELLS[rng.below(CELLS.length)]}`;
        else if (k < 74) { code = `sf:${rng.below(3)}`; html = HTMLS[rng.below(HTMLS.length)]; }
        else if (k < 77) code = `ps:${FORMATS[rng.below(5)]}:${rng.below(2)}`;
        else if (k < 79 && I.ring) code = `pi:${rng.below(I.ring)}:${md()}`;
        else if (k < 80) code = 'pa';
        else if (k < 81 && I.ring) code = `rm:${rng.below(I.ring)}`;
        else if (k < 82) code = 'cl';
        else if (k < 83) code = `pd:${MODES[rng.below(5)]}:${MODES[rng.below(5)]}:${MODES[rng.below(5)]}`;
        else if (k < 85) code = `dp:${rng.below(PATHS.length)}`;
        else if (k < 87 && I.tables && !I.cell) code = `ec:${rng.below(I.tables)}`;
        else if (k < 88) code = 'xc';
        else if (k < 95) code = 'un';
        else code = 're';
        if (ext) {
          code = extOp(rng, I, pos, md) ?? code;
          // the base table steps only in the body (a cell of a note or a header is another family's)
          // and no undo, redo or repaste (its undo) inside a note or a header: the TS undo leaves or
          // re-enters the part by the layout (F4's `undo_reentering`; F8.md F8-O4)
          if (I.ctx !== 'b' && /^(ec|ts|rp):|^(un|re)$/.test(code)) code = I.ctx === 'n' ? 'xn' : I.ctx === 'h' ? 'xh' : 'xc';
          // a range across paragraphs of a NOTE: the TS deletes the BODY's tables of those indexes
          // (`tableAnchors` is not swapped in a note; D2a's matter, see F8.md F8-O3): one paragraph
          // no table pasted into a NOTE: the TS puts it in the BODY's anchors and enters its cell with
          // the note edit still on (F3's `enterCellAt` from a note; F8.md F8-O3)
          if (I.ctx === 'n' && (/^(pc:(overwrite|rows|nest)|pi|pa)(:|$)/.test(code) || (/^pr:/.test(code) && I.cells))) code = 'xn';
          if (I.ctx === 'n' && html === HTMLS[4]) html = HTMLS[5];
          if (I.ctx === 'n' && /^(se|sm):/.test(code)) {
            const g = code.split(':');
            if (g[1] !== g[3]) { g[3] = g[1]; g[4] = String(Math.min(+g[4], I.lens[+g[1]] ?? 0)); code = g.join(':'); }
          }
          // a float selection (a pasted drawing stays selected) lasts until the next copy, cut or
          // object step: the TS keeps the run object across text edits, the session a path (F7's
          // matter, see F8.md F8-O3)
          if (I.fs && !/^(co|cu|so|un|re)(:|$)/.test(code)) code = `xf+${code}`;
        }
      }
      const r = await page.evaluate(async ([code, html, FORMATS, PATHS, F8_POLICY]) => { try {
        const el = window.__el;
        window.__ev = []; window.__cbw = null; window.__fx = []; window.__wait = []; window.__rc = 0; window.__ptb = null; window.__cbt = null; window.__psf = null; window.__hf = null;
        let body = code;
        if (code.startsWith('xf+')) { el.floatSel = null; el.floatMulti = []; body = code.slice(3); }
        const f = body.split(':');
        const s16 = (h) => String.fromCharCode(...h.split('.').filter(Boolean).map((x) => parseInt(x, 16)));
        const m = (x) => x || undefined;
        const enc = (h) => { const e = window.__ptree.encodePaste(h, window.__ph.fragmentOf); let s = ''; for (let i = 0; i < e.bytes.length; i += 0x8000) s += String.fromCharCode(...e.bytes.subarray(i, i + 0x8000)); return btoa(s); };
        const stripTags = (h) => h.replace(/<[^>]*>/g, '');
        switch (f[0]) {
          case 'ty': el.type(s16(f[1])); break;
          case 'en': el.pressEnter(); break;
          case 'bs': el.pressBackspace(); break;
          case 'se': el.setSelection({ para: +f[1], offset: +f[2] }, { para: +f[3], offset: +f[4] }); break;
          case 'tk': el.toggleTrackChanges(f[1] === '1'); break;
          case 'co': await el.copy(); break;
          case 'cu': { const p = el.cut(); window.__cutp = p; await p; break; }
          case 'pr': el.pasteData(el.withRich({ text: el.richClip?.text ?? '' }), m(f[1]), f[2] || null); break;
          case 'ph': window.__ptb = enc(html); window.__cbt = stripTags(html); el.pasteData({ text: stripTags(html), html }, m(f[1])); break;
          case 'pt': el.pasteData({ text: s16(f[2]) }, m(f[1])); break;
          case 'px': el.pasteText(s16(f[1])); break;
          case 'pv': {
            // 0: text + html; 1: the rich copy's text (+ html); 2: text only
            const t = f[2] === '1' ? (el.richClip?.text ?? '') : stripTags(html);
            const item = f[2] === '2' ? { 'text/plain': t } : { 'text/plain': t, 'text/html': html };
            if (item['text/html']) window.__ptb = enc(html);
            window.__cbItems = [item];
            window.__cbt = t;
            await el.paste(m(f[1]));
            break;
          }
          case 'rp': el.repaste(f[1], f[2] || undefined); break;
          case 'ts': { const a = +f[1]; el.tableSel = { anchor: a, path: [], rect: { r1: +f[2], c1: +f[3], r2: +f[4], c2: +f[5] }, origin: { r: +f[2], c: +f[3] } }; el.refresh?.(); break; }
          case 'pc': el.pasteCells(f[1]); break;
          case 'sf': {
            const t = f[1] === '1' ? (el.richClip?.text ?? '') : stripTags(html);
            const item = f[1] === '2' ? { 'text/plain': t } : { 'text/plain': t, 'text/html': html };
            if (item['text/html']) window.__ptb = enc(html);
            window.__cbItems = [item];
            window.__cbt = t;
            const r = await el.pasteSpecialFormats();
            window.__psf = `${r.formats.join(',')}|${r.link ? 1 : 0}|${r.source}`;
            break;
          }
          case 'ps': window.__cbItems = []; await el.pasteSpecial(f[1], { link: f[2] === '1' }); break;
          case 'pi': el.pasteClipboardItem(el.clipboardItems()[+f[1]].id, m(f[2])); break;
          case 'pa': el.pasteAllClipboardItems(); break;
          case 'rm': el.removeClipboardItem(el.clipboardItems()[+f[1]].id); break;
          case 'cl': el.clearClipboardItems(); break;
          case 'pd': el.pasteDefaults = { within: f[1], between: f[2], other: f[3] }; break;
          case 'dp': el.documentPath = PATHS[+f[1]] || undefined; break;
          case 'ec': el.enterCell(+f[1], [{ r: 0, c: el.tableAnchors[+f[1]]?.table.rows[0]?.trPr?.gridBefore ?? 0 }], { para: 0, offset: 0 }); break;
          case 'xc': el.exitCell(); break;
          case 'un': el.undo(); break;
          case 're': el.redo(); break;
          // ── the extended groups ──
          case 'fp': el.formattingPolicy = [null, window.__fp.FORMATTING_POLICY_PRESETS['template-document'], F8_POLICY][+f[1]]; break;
          case 'so': case 'sm': {
            if (f[0] === 'so') { const runs = el.contextDrawingRuns(el.model); el.floatSel = runs[+f[1]] ?? null; el.floatMulti = []; }
            else el.setSelection({ para: +f[1], offset: +f[2] }, { para: +f[3], offset: +f[4] });
            const a = f[f.length - 1];
            if (a === 'c') await el.copy(); else if (a === 'u') await el.cut();
            break;
          }
          case 'on': el.openNote(+f[1] === 1 ? 'endnote' : 'footnote', +f[2]); break;
          case 'xn': el.exitNote(); break;
          case 'eh': {
            const keys0 = [...el.hfParts.keys()];
            const kind = f[1] === 'h' ? 'header' : 'footer';
            el.enterHeaderFooter(kind, 0);
            // a part created by `enterHeaderFooter`: kind, section (index of the model's references) and variant (as capture-d2b-f4)
            let cr = '-';
            const fresh = [...el.hfParts.keys()].filter((k) => !keys0.includes(k));
            if (fresh.length === 1) {
              const [kd, rid] = fresh[0].split(':');
              const m = el.bodyModel();
              const count = m.paragraphs.filter((p) => p.props.sectionBreak).length + 1;
              for (let sIdx = 0; sIdx < count; sIdx++) {
                const holder = el.refsOfSection(m, sIdx, kd).find((r) => r.rid === rid);
                if (holder) { cr = `${kd}:${sIdx}:${holder.type}`; break; }
              }
            }
            window.__hf = `${el.hfEdit ? el.hfEdit.path : '-'}|${cr}`;
            break;
          }
          case 'xh': el.exitHeaderFooter(); break;
        }
        for (let i = 0; i < window.__wait.length; i++) await window.__wait[i];
        // an async paste (picture) refreshes after its await: let the queued layout pass (syncRunRPr) run
        if (window.__wait.length) await new Promise((r) => requestAnimationFrame(() => requestAnimationFrame(() => setTimeout(r, 0))));
        return null;
      } catch (e) { return String(e && e.stack || e); } }, [code, html, FORMATS, PATHS, F8_POLICY]);
      if (r) {
        console.log(`${key} ${code}: ${r.split('\n')[0]}`);
        out.set(key, 'op', code);
        out.set(key, 'thrown', '1');
        break;
      }
      out.set(key, 'op', code);
      if (process.env.UO_F8_DBG) console.log(key, code, await page.evaluate((full) => JSON.stringify([window.__cbw, window.__el.model.paragraphs.map((p) => p.runs.map((r) => r.text).join('')), full ? window.__el.model.paragraphs.flatMap((p) => p.runs.filter((r) => r.image).map((r) => [Object.getOwnPropertySymbols(r.props).length, !!r.rpr, Object.getOwnPropertySymbols(p.runs[0].props).length])) : 0, full ? window.__el.parseState(window.__el.stringifyState()).m.paragraphs.filter((p) => p.runs.some((r) => r.image)).map((p) => JSON.stringify(p, (k, v) => (v instanceof Uint8Array || k === 'data' ? '#bytes' : v))) : 0]), process_env_full));
      const st = await page.evaluate(() => {
        const el = window.__el;
        const b64 = (u8) => { let s = ''; for (let i = 0; i < u8.length; i += 0x8000) s += String.fromCharCode(...u8.subarray(i, i + 0x8000)); return btoa(s); };
        const enc = (v) => (v instanceof Uint8Array ? { __u8: b64(v) } : v);
        const walk = (v) => (v && typeof v === 'object' && !(v instanceof Uint8Array) ? (Array.isArray(v) ? v.map(walk) : Object.fromEntries(Object.entries(v).map(([k, x]) => [k, walk(enc(x))]))) : enc(v));
        const state = el.parseState(el.stringifyState());
        const hx = (t) => [...Array(t.length).keys()].map((i) => t.charCodeAt(i).toString(16)).join('.');
        const c = el.richClip;
        const po = el.pasteOptions();
        return {
          paras: walk(state.m.paragraphs),
          sel: JSON.stringify(state.s),
          anc: JSON.stringify(state.anc ?? null),
          ev: window.__ev.join(' '),
          flags: `${el.isDirty() ? 1 : 0}${el.undoStack.length ? 1 : 0}${el.redoStack.length ? 1 : 0}:${el.editsSinceLoad}`,
          cbw: window.__cbw ? `${hx(window.__cbw.t)}|${window.__cbw.h ?? '-'}` : '-',
          clip: c ? `${hx(c.text)}|${c.paras.length}|${c.drawing ? 'd' : c.cells ? `c${c.cells.rows ? 1 : 0}${c.cells.whole ? 1 : 0}` : 'p'}|${c.paras.map((p) => `${p.runs.length}${p.mark ? 'm' : ''}${p.marker ? `[${hx(p.marker)}]` : ''}`).join(',')}|${Object.keys(c.lists ?? {}).join(',')}|${c.tables?.length ?? 0}|${c.path ?? ''}|${c.src ? 1 : 0}` : '-',
          ring: el.clipboardItems().map((x) => `${hx(x.text)}:${x.kind}`).join(' ') || '-',
          po: `${po.modes.join(',')}|${po.list.join(',')}|${po.current ?? ''}|${po.currentList ?? ''}`,
          psf: window.__psf ?? '-',
          nv: el.numbering.xml ?? '',
          sv: el.docStyles.xml ?? '',
          rc: String(window.__rc),
          pt: window.__ptb ?? null,
          fx: JSON.stringify(window.__fx),
          cbt: window.__cbt ?? null,
          hf: window.__hf,
        };
      });
      out.set(key, 'paras', digest(revive(st.paras)));
      out.set(key, 'sel', st.sel);
      out.set(key, 'anc', st.anc);
      out.set(key, 'ev', st.ev || '-');
      out.set(key, 'flags', st.flags);
      out.set(key, 'cbw', st.cbw === '-' ? '-' : `${st.cbw.split('|')[0].length > 200 ? sha(st.cbw.split('|')[0]) : st.cbw.split('|')[0]}|${st.cbw.slice(st.cbw.indexOf('|') + 1) === '-' ? '-' : sha(st.cbw.slice(st.cbw.indexOf('|') + 1))}`);
      out.set(key, 'clip', st.clip.length > 300 ? sha(st.clip) : st.clip);
      out.set(key, 'ring', sha(st.ring));
      if (process.env.UO_F8_RING) out.set(key, 'ringraw', st.ring);
      out.set(key, 'po', st.po);
      out.set(key, 'psf', st.psf);
      out.set(key, 'nv', sha(st.nv));
      out.set(key, 'sv', sha(st.sv));
      out.set(key, 'rc', st.rc);
      if (st.pt) out.set(key, 'pt', st.pt);
      if (process.env.UO_F8_XML) writeFileSync(join(process.env.UO_F8_XML, `ts-nv-${n}-${j}.xml`), st.nv);
      if (process.env.UO_F8_XML && st.cbw !== '-') writeFileSync(join(process.env.UO_F8_XML, `ts-cbw-${n}-${j}.html`), st.cbw);
      if (st.cbt !== null) out.set(key, 'cbt', hex(st.cbt));
      if (st.hf !== null) out.set(key, 'hf', st.hf);
      if (st.fx !== '[]') out.set(key, 'fx', Buffer.from(st.fx, 'utf8').toString('base64'));
      if (j % 8 === 0 || j === STEPS) {
        const b64 = await page.evaluate(() => { try { const b = window.__el.saveDocx(); let s = ''; for (let i = 0; i < b.length; i += 0x8000) s += String.fromCharCode(...b.subarray(i, i + 0x8000)); return btoa(s); } catch { return null; } });
        let d = 'err';
        if (b64) {
          const pkg = DocxPackage.load(Uint8Array.from(Buffer.from(b64, 'base64')));
          const x = pkg.getPartText('word/document.xml') ?? '';
          d = Digest.of(Buffer.from(x, 'utf8')).short;
          // the same XML with the docPr ids masked (the save's fresh-id counter of the element carries across loads, G7)
          out.set(key, 'docxmlm', Digest.of(Buffer.from(x.replace(/(<wp:docPr\b[^>]*?\bid=")\d+"/g, '$1#"').replace(/rIdUwImg\d+/g, 'rIdUwImg#'), 'utf8')).short);
          if (process.env.UO_F8_XML) writeFileSync(join(process.env.UO_F8_XML, `ts-${n}-${j}.xml`), pkg.getPartText('word/document.xml') ?? '');
        }
        out.set(key, 'docxml', d);
      }
    }
    n++;
  }
  await page.close();
}
await browser.close();
server.close();
// (UO_F8_OUT: a debugging run, e.g. with UO_F8_ONLY, writes elsewhere)
out.write(process.env.UO_F8_OUT ?? join(crate, 'tests', 'parity', 'd2b-f8-session.capture'));
console.log(`d2b-f8-session: ${n} documents x ${STEPS} steps`);
