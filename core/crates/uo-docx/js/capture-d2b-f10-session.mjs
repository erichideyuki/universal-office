// D2b-F10 (fields, TOC, table of figures, captions, cross-references, Manual Hyphenation, the date,
// document-info and alignment-tab insertions) in the D2 editor harness (G17, U5, U16): the frozen
// `<universal-write>` element in headless Chromium (Playwright), driven through its public API and the
// TS-private entry points the D2a harness uses. The page set-up is the one of `capture-d2b-f9-session.mjs`
// (the element initialises with the open default font so the layout pass runs, `Math.random` = the
// testkit xorshift32, `Date` fixed 2026-01-02 03:04 UTC, timezone UTC, locale pt-BR).
//
//   UO_TS_ROOT=<built checkout of main> node --experimental-transform-types core/crates/uo-docx/js/capture-d2b-f10-session.mjs [--docs N] [--steps N]
//   diagnostics (env): UO_F10_ONLY=<doc substring> UO_F10_OUT=<capture path> UO_F10_DUMP=<key substring> (the paragraphs after
//   the step) UO_F10_TRACE (the cross-reference items) UO_F10_PG (the pageLabelOf calls) UO_F10_UNK (the unnamed field runs)
//
// THE ORACLE USE (the Rust LayoutOracle answers what the TS layout answered): the element's private
// `refreshFieldCache`, `setFieldResult` and `pageLabelOf` are wrapped. Per step the capture keeps
//   i-fv  the calls of `refreshFieldCache`: scope (`A` or the `only` keys) and the (key, value) pairs that
//         `setFieldResult` received, in order (= the layout's `onField` values after the `only` filter);
//   i-pg  the `pageLabelOf` answers by paragraph ordinal (the ordinal the paragraph had at the START of the step);
//   i-mh  the word `nextManualHyphenation` proposed (`manualHyph.cur`), which is what the forced-hyphenation
//         layout answered;
// and the assertions the Rust replay makes about HOW it asked:
//   fvs   the number of `refreshFieldCache` calls and, per call, the scope: `A` or `R<n>:<sha of the sorted keys>`
//         (`R<n>:?` when `only` holds runs the body walk does not name: only the size is compared, F10-A2);
//   pgc   whether the page labels were asked (1/0);
//   pgv   `ok:<n>`: every paragraph the TS asked `pageLabelOf` about is in the Rust `shown_page_numbers` request;
//   mhc   `1` when the TS got a word (the Rust must have asked the layout), else `x`.
// A field run is keyed by its ordinal among the field runs of the body in document order (cells included),
// and `<ordinal>.<j>` for the j-th run of its `fieldResult`; a paragraph by its ordinal among the paragraphs
// of the body walk. A field outside that walk (header, footer, note, text box) is `?`: counted (`unk`),
// not replayed.
//
// Writes tests/parity/d2b-f10-session.capture: per `<doc>#<j>` the D2a facets (op, paras, sel, cursel, anc, ev,
// flags, docxml) and F10's: res, cap, mh, xri, toco, dtf (every 8th step), stylexml (with docxml), rs, fvs, pgc,
// pgv, mhc, unk, and the inputs above.
import { createRequire } from 'node:module';
import { createServer } from 'node:http';
import { readFileSync, existsSync, readdirSync } from 'node:fs';
import { join, extname, normalize } from 'node:path';
import { createHash } from 'node:crypto';
import { CaptureWriter, Rng, Stream } from '../../uo-testkit/js/testkit.mjs';
import { Canon } from './canon.mjs';
import { crate, corpus, readBytes, root, ts } from './capture-r2-common.mjs';

const Digest = { of: (b) => ({ short: createHash('sha256').update(b).digest('hex').slice(0, 32) }) };
const argv = process.argv.slice(2);
const opt = (k, d) => (argv.includes(k) ? Number(argv[argv.indexOf(k) + 1]) : d);
const DOCS = opt('--docs', 80);
const STEPS = opt('--steps', 60);
const { DocxPackage } = await ts('package.js');

const pwFrom = [join(root, 'slides', 'package.json'), join(root, 'sheet', 'package.json')].find((p) => existsSync(p));
const { chromium } = createRequire(pwFrom)('playwright');

const MIME = { '.js': 'text/javascript', '.mjs': 'text/javascript', '.html': 'text/html', '.json': 'application/json', '.wasm': 'application/wasm', '.css': 'text/css', '.ttf': 'font/ttf', '.otf': 'font/otf', '.woff2': 'font/woff2' };
const server = createServer((req, res) => {
  const url = decodeURIComponent((req.url ?? '/').split('?')[0]);
  if (url === '/__d2b.html') {
    res.writeHead(200, { 'content-type': 'text/html' });
    res.end('<!doctype html><meta charset="utf-8"><body><script type="module">import "/write/packages/editor/publish/index.js"; window.__ready = true;</script></body>');
    return;
  }
  const file = normalize(join(root, url));
  if (!file.startsWith(normalize(root)) || !existsSync(file)) { res.writeHead(404); res.end(); return; }
  res.writeHead(200, { 'content-type': MIME[extname(file)] ?? 'application/octet-stream' });
  res.end(readFileSync(file));
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
  let s = 1;
  Math.random = () => { s ^= s << 13; s >>>= 0; s ^= s >>> 17; s ^= s << 5; s >>>= 0; return s / 4294967296; };
  const T = Date.UTC(2026, 0, 2, 3, 4, 0);
  const D = Date;
  // eslint-disable-next-line no-global-assign
  Date = class extends D { constructor(...a) { if (a.length) super(...a); else super(T); } static now() { return T; } };
});

const EVENTS = ['uw-change', 'uw-dirty-change', 'uw-input', 'uw-track-changes', 'uw-cellmode', 'uw-notemode', 'uw-hfmode', 'uw-tbmode', 'uw-protected', 'uw-blocked', 'uw-styles-change', 'uw-caption-settings'];
const TEXTS = ['a', 'Olá ', ' ', 'ção', '😀', 'abc def', 'the', 'inter\u00adnacional'];
const hex = (t) => [...Array(t.length).keys()].map((i) => t.charCodeAt(i).toString(16)).join('.');
/** The tables the codes index; shared by index with tests/parity_d2b_f10_session.rs. */
const TYPES = ['numbered', 'heading', 'bookmark', 'footnote', 'endnote', 'caption'];
const KINDS = ['text', 'page', 'number', 'numberNoContext', 'numberFullContext', 'position', 'noteNumber', 'noteNumberFormatted', 'captionEntire', 'captionLabel', 'captionText'];
const NUMS = [undefined, { format: 'ARABIC' }, { format: 'ROMAN', chapter: { level: 1, separator: 'period' } }, { format: 'alphabetic' }, { format: 'ALPHABETIC', chapter: { level: 2, separator: 'hyphen' } }, { format: 'roman', chapter: { level: 1, separator: 'emDash' } }];
const DIALOGS = [{}, { pageNumbers: false }, { rightAlign: false, leader: 'hyphen' }, { hyperlinks: false, levels: 2 }, { format: 'classic', levels: 4 }, { useStyles: false, fields: true },
  { outlineLevels: false, leader: 'none', format: 'modern' }, { styles: [{ style: 'Quote', level: 2 }], levels: 1 }, { fields: true, fieldId: 'x', format: 'fancy' }, { leader: 'underscore', rightAlign: true, format: 'simple', levels: 9 }];
const TOFS = [{}, { includeLabel: false }, { pageNumbers: false }, { rightAlign: false, leader: 'hyphen' }, { hyperlinks: true, format: 'classic' }, { leader: 'none', format: 'centered' }, { format: 'formal', includeLabel: false, hyperlinks: false }];
const TPL = ['auto1', 'auto2', 'manual'];
const DTP = ['dd/MM/yyyy', 'M/d/yyyy', 'dddd, MMMM d, yyyy', 'HH:mm', 'h:mm AM/PM', 'yyyy-MM-dd', "d 'de' MMMM 'de' yyyy", '', 'MMM d', 'dddd', 'dd/MM/yyyy HH:mm'];
const LANGS = ['en', 'pt', 'es', undefined];
const INFOS = ['author', 'title', 'fileName', 'filePath'];
const ALIGN = ['left', 'center', 'right'];
const RELTO = ['margin', 'indent'];
const LEADER = ['none', 'dot', 'hyphen', 'underscore', 'middleDot'];
const MKIDS = [undefined, 'C', 'x', 'a b'];
const MKLVL = [undefined, 1, 2, 0, 9, 12];
const TLVL = [0, 1, 2, 3, 9, 12];
const TITLES = ['', ': t', ' - Gráfico', '"q" \\'];
const NEWLABELS = ['Gráfico', 'Chart', 'a b', 'q"uote', 'Figure'];
const CFGS = [{ labels: [], numbering: {} }, { labels: ['Gráfico', 'Chart'], numbering: { Gráfico: { format: 'roman' } }, auto: { table: { label: 'Table', position: 'above' } } },
  { labels: ['Chart'], numbering: { Chart: { format: 'ALPHABETIC', chapter: { level: 1, separator: 'colon' } } } }];

const revive = (v) => (v && typeof v === 'object' ? (v.__u8 !== undefined ? Uint8Array.from(Buffer.from(v.__u8, 'base64')) : (Array.isArray(v) ? v.map(revive) : Object.fromEntries(Object.entries(v).map(([k, x]) => [k, revive(x)])))) : v);

function digest(paras) {
  const s = new Stream();
  const c = new Canon(s);
  paras.forEach((p, i) => { s.tag('E').u32(i); c.value(p, 'paragraph'); });
  return s.finish().short;
}
const sha = (t) => Digest.of(Buffer.from(t, 'utf8')).short;

const out = new CaptureWriter([
  'D2b-F10 session in Chromium: the frozen <universal-write> driven by seeded scripts (fields, cross-references, captions, TOC, table of figures, Manual Hyphenation, date/time, document info, alignment tab)',
  'facets: op, paras, sel, cursel, anc, ev, flags, docxml, stylexml, res, cap, mh, xri, toco, dtf, rs, fvs, pgc, pgv, mhc, unk; inputs: i-seed, i-fv, i-pg, i-mh',
]);
out.meta('steps', STEPS);
out.meta('browser', `chromium ${browser.version()}`);
out.meta('bundle', Digest.of(readFileSync(join(root, 'write/packages/editor/publish/index.js'))).short);
out.meta('math-override', 'none (no F10 state value goes through math)');

/** Documents with headings, fields, bookmarks, tables or notes first, then the rest. */
function pick() {
  const all = corpus();
  const rich = [];
  const plain = [];
  for (const f of all) {
    let t = '';
    try { t = DocxPackage.load(readBytes(f.file)).getPartText('word/document.xml') ?? ''; } catch { continue; }
    (/<w:(fldSimple|fldChar|bookmarkStart|footnoteReference)\b|w:val="(Heading|Ttulo)\d/.test(t) ? rich : plain).push(f);
  }
  const take = (list, n, mod) => list.filter((_, i) => i % mod === 0).slice(0, n);
  return [...take(rich, Math.ceil(DOCS * 0.8), 2), ...take(plain, Math.floor(DOCS * 0.2), 9)].slice(0, DOCS);
}
const files = process.env.UO_F10_ONLY ? pick().filter((f) => f.rel.includes(process.env.UO_F10_ONLY)) : pick();

/** In the page: the walks, the wrappers around the oracle-facing private methods, and the state readers. */
const PAGE_SETUP = ([EVENTS]) => {
  const el = window.__el;
  window.__ev = [];
  for (const t of EVENTS) el.addEventListener(t, (e) => { const d = e.detail ?? {}; window.__ev.push(`${t}:${d.dirty ?? d.active ?? d.on ?? ''}`); });
  // the body walk: top-level paragraphs and tables interleaved by `beforePara`, cells in `blocks` order
  const walkTable = (t, f) => { for (const row of t.rows) for (const c of row.cells) walkCell(c, f); };
  const walkCell = (c, f) => {
    if (c.blocks) { for (const b of c.blocks) { if (b.type === 'paragraph') f(b.paragraph); else walkTable(b, f); } }
    else { c.paragraphs.forEach(f); c.tables.forEach((t) => walkTable(t, f)); }
  };
  const walk = (f) => {
    const st = el.bodyState();
    const ps = st.model.paragraphs;
    const anchors = st.tableAnchors;
    let ti = 0;
    for (let pi = 0; pi <= ps.length; pi++) {
      while (ti < anchors.length && (anchors[ti].beforePara <= pi || pi >= ps.length)) { walkTable(anchors[ti].table, f); ti++; }
      if (pi < ps.length) f(ps[pi]);
    }
  };
  // `liveEngineDoc()` brings the cell/note blocks up to date (the TS calls it at the start of the layout)
  const sync = () => { try { el.liveEngineDoc(); } catch { /* the walk goes on with what there is */ } };
  const paraOrd = () => { sync(); const m = new Map(); let n = 0; walk((p) => m.set(p, n++)); return m; };
  // `names(full).runs` of tools/lib/facets-f8.mjs (uo-docx-layout), on the engine doc
  const engNames = () => {
    const full = el.liveEngineDoc().full;
    const runs = new Map();
    const blocksOfCell = (c) => c.blocks ?? c.paragraphs.map((paragraph) => ({ type: 'paragraph', paragraph }));
    const blocksOfParas = (ps) => ps.map((paragraph) => ({ type: 'paragraph', paragraph }));
    const fr = (r, ref) => (r.fieldResult ?? []).forEach((q, j) => { const k = `${ref}/f${j}`; if (!runs.has(q)) runs.set(q, k); fr(q, k); });
    const walkB = (bs, story, base) => bs.forEach((b, i) => {
      const p = base ? `${base}/b${i}` : `b${i}`;
      if (b.type === 'paragraph') {
        b.paragraph.runs.forEach((r, k) => {
          const ref = `${story}:${p}#${k}`;
          if (!runs.has(r)) runs.set(r, ref);
          fr(r, ref);
          if (r.textbox) walkB(r.textbox.blocks ?? blocksOfParas(r.textbox.paras), `${ref}/tx`, '');
          (r.group?.items ?? []).forEach((it, j) => {
            const g = `${ref}/g${j}`;
            if (!runs.has(it.run)) runs.set(it.run, g);
            fr(it.run, g);
            if (it.run.textbox) walkB(it.run.textbox.blocks ?? blocksOfParas(it.run.textbox.paras), `${g}/tx`, '');
          });
        });
      } else b.rows.forEach((row, ri) => row.cells.forEach((c, ci) => walkB(blocksOfCell(c), story, `${p}/r${ri}/c${ci}`)));
    });
    walkB(full.blocks, 'b', '');
    const hf = new Map();
    for (const s of full.sections ?? []) {
      for (const [set, ps] of [[s.headerBlocks, s.headers], [s.footerBlocks, s.footers]]) {
        for (const k of ['default', 'first', 'even']) {
          const arr = set?.[k] ?? ps?.[k];
          if (!arr || hf.has(arr)) continue;
          hf.set(arr, `hf${hf.size}`);
          walkB(set?.[k] ?? blocksOfParas(arr), hf.get(arr), '');
        }
      }
    }
    for (const [kind, blocks, ps] of [['fn', full.footnoteBlocks, full.footnotes], ['en', full.endnoteBlocks, full.endnotes]]) {
      for (const [id, list] of ps ?? []) walkB(blocks?.get(id) ?? blocksOfParas(list), `${kind}${id}`, '');
    }
    return runs;
  };
  const fieldKeys = () => {
    sync();
    const m = new Map();
    const visit = (prefix, ctr) => (p) => {
      for (const r of p.runs) {
        if (!r.field) continue;
        const k = `${prefix}${ctr.n++}`;
        m.set(r, k);
        (r.fieldResult ?? []).forEach((x, j) => { if (x.field) m.set(x, `${k}.${j}`); });
      }
    };
    walk(visit('', { n: 0 }));
    // the notes: footnotes then endnotes, keyed `f<id>:<n>` / `e<id>:<n>`
    for (const [tag, map] of [['f', el.fullDoc?.footnotes], ['e', el.fullDoc?.endnotes]]) {
      for (const [id, ps] of map ?? []) { const ctr = { n: 0 }; ps.forEach(visit(`${tag}${id}:`, ctr)); }
    }
    // header/footer parts, text boxes, group items: the engine-doc names of `facets-f8.mjs names(full)` with an `x:`
    // prefix (the tail `#<run>`, `#<run>/f<j>` or `#<run>/g<j>`: what a RunRef can address)
    const nm = engNames();
    for (const [r, name] of nm) {
      if (!r.field || m.has(r)) continue;
      if (!/#\d+(\/[fg]\d+)?$/.test(name)) continue;
      if (name.startsWith('hf') || name.includes('/tx') || /\/g\d+$/.test(name)) m.set(r, `x:${name}`);
    }
    return m;
  };
  window.__reset = () => { window.__orc = { fv: [], pg: new Map(), pgCalls: 0, pgUnk: 0, unk: 0, unkc: 0 }; window.__pord = paraOrd(); };
  window.__reset();
  const origRefresh = el.refreshFieldCache.bind(el);
  el.refreshFieldCache = function (only) {
    const keys = fieldKeys();
    const call = { only: only ? [...only].map((r) => keys.get(r) ?? '?') : null, vals: [] };
    window.__orc.fv.push(call);
    if (only) for (const r of only) if (!keys.has(r)) (window.__unkInfo ??= []).push(`only:${r.field}|cell=${!!el.cellEdit}|note=${!!el.noteEdit}|tb=${!!el.tbEdit}|hf=${!!el.hfEdit}`);
    window.__keys = keys;
    window.__call = call;
    try { return origRefresh(only); } finally { window.__call = null; }
  };
  const origSet = el.setFieldResult.bind(el);
  el.setFieldResult = function (run, v) {
    const c = window.__call;
    if (c) c.vals.push([window.__keys.get(run) ?? '?', v]);
    const changed = origSet(run, v);
    if (c && !window.__keys.has(run)) { (window.__unkInfo ??= []).push(`${run.field}=>${v}`); if (changed) window.__orc.unkc++; }
    return changed;
  };
  const origPage = el.pageLabelOf.bind(el);
  el.pageLabelOf = function (p) {
    const r = origPage(p);
    const o = window.__orc;
    o.pgCalls++;
    const k = window.__pord.get(p);
    if (window.__trace) console.log(`pageLabelOf ord=${k} text=${JSON.stringify(p.runs.map((x) => x.text).join('').slice(0, 24))} -> ${r}`);
    if (k === undefined) o.pgUnk++; else if (!o.pg.has(k)) o.pg.set(k, r);
    return r;
  };
  window.__fieldKeys = fieldKeys;
  window.__trace = !!window.__traceOn;
};

let n = 0;
for (const { rel, file } of files) {
  const page = await context.newPage();
  await page.goto(`http://127.0.0.1:${port}/__d2b.html`);
  await page.waitForFunction(() => window.__ready === true);
  const bytes = readBytes(file);
  const ok = await page.evaluate(async ([b64]) => {
    const el = document.createElement('universal-write');
    el.setAttribute('fonts', '/write/packages/editor/demo/fonts');
    const ready = new Promise((r) => el.addEventListener('uw-ready', r, { once: true }));
    document.body.append(el);
    await ready;
    window.__el = el;
    try {
      el.author = 'A';
      el.loadDocx(Uint8Array.from(atob(b64), (c) => c.charCodeAt(0)), { protected: false });
      return !!el.fonts;
    } catch { return false; }
  }, [Buffer.from(bytes).toString('base64')]);
  if (!ok) { await page.close(); continue; }
  if (process.env.UO_F10_PG) { page.on('console', (m) => console.log(`PAGE ${m.text()}`)); await page.evaluate(() => { window.__traceOn = true; }); }
  await page.evaluate(PAGE_SETUP, [EVENTS]);
  let seed = 0xf10e;
  for (let i = 0; i < rel.length; i++) seed = (Math.imul(seed, 31) + rel.charCodeAt(i)) >>> 0;
  const rng = new Rng(seed);
  const info = () => page.evaluate(() => {
    const el = window.__el;
    const labels = el.getCaptionLabels().map((l) => l.name);
    const items = {};
    for (const t of ['numbered', 'heading', 'bookmark', 'footnote', 'endnote']) items[t] = el.getCrossReferenceItems(t).slice(0, 40).map((i) => i.id);
    items.caption = {};
    for (const l of labels) items.caption[l] = el.getCrossReferenceItems('caption', l).slice(0, 40).map((i) => i.id);
    return {
      lens: el.model.paragraphs.map((p) => p.runs.reduce((a, r) => a + r.text.length, 0)),
      labels,
      items,
      tables: el.tableAnchors.length,
      cell: !!el.cellEdit,
    };
  });
  for (let j = 0; j <= STEPS; j++) {
    const key = `${rel}#${j}`;
    let code = 'init';
    if (j > 0) {
      const I = await info();
      const L = I.lens;
      const pos = () => { const para = rng.below(L.length); return { para, offset: rng.below(L[para] + 1) }; };
      const pick1 = (a) => a[rng.below(a.length)];
      const k = rng.below(100);
      if (k < 8) code = `ty:${hex(TEXTS[rng.below(TEXTS.length)])}`;
      else if (k < 10) code = 'en';
      else if (k < 13) code = 'bs';
      else if (k < 15) code = 'de';
      else if (k < 24) { const a = pos(); const f = rng.below(3) === 0 ? a : pos(); code = `se:${a.para}:${a.offset}:${f.para}:${f.offset}`; }
      else if (k < 26) code = `tk:${rng.below(2)}`;
      else if (k < 29) code = `if:${rng.below(3)}`;
      else if (k < 32) code = `dt:${rng.below(DTP.length)}:${rng.below(2)}:${rng.below(LANGS.length)}`;
      else if (k < 34) code = `di:${rng.below(INFOS.length)}`;
      else if (k < 36) code = `at:${rng.below(3)}:${rng.below(2)}:${rng.below(5)}`;
      else if (k < 40) code = `uf:${rng.below(2)}:${rng.below(2)}`;
      else if (k < 48) {
        const t = rng.below(TYPES.length);
        const label = TYPES[t] === 'caption' && I.labels.length ? pick1(I.labels) : '';
        const ids = TYPES[t] === 'caption' ? (I.items.caption[label] ?? []) : I.items[TYPES[t]];
        const item = ids.length && rng.below(8) ? pick1(ids) : pick1(['x', 'p:99999', 'n:77']);
        code = `xr:${t}:${hex(label)}:${hex(item)}:${rng.below(KINDS.length)}:${rng.below(4)}`;
      } else if (k < 54) code = `ic:${hex(I.labels.length && rng.below(6) ? pick1(I.labels) : pick1(NEWLABELS))}:${hex(pick1(TITLES))}:${rng.below(3)}:${rng.below(3) === 0 ? 1 : 0}:${rng.below(NUMS.length)}`;
      else if (k < 57) code = rng.below(3) === 0 ? 'ut2' : `tf:${hex(I.labels.length && rng.below(6) ? pick1(I.labels) : 'Figure')}:${rng.below(TOFS.length)}`;
      else if (k < 61) code = `tc:${rng.below(DIALOGS.length)}:${rng.below(3)}`;
      else if (k < 63) code = `tt:${rng.below(3)}`;
      else if (k < 65) code = `ut:${rng.below(2)}`;
      else if (k < 67) code = 'rt';
      else if (k < 69) code = rng.below(2) ? `tl:${rng.below(TLVL.length)}` : `mk:${hex(pick1(['Entry', 'a"b', 'x y', '  ']))}:${rng.below(MKIDS.length)}:${rng.below(MKLVL.length)}`;
      else if (k < 72) {
        const w = rng.below(5);
        code = w === 0 ? `cl:${hex(pick1(NEWLABELS))}` : w === 1 ? `dl:${hex(I.labels.length ? pick1(I.labels) : 'x')}` : w === 2 ? `cn:${hex(I.labels.length ? pick1(I.labels) : 'x')}:${rng.below(NUMS.length)}`
          : w === 3 ? `ac:${rng.below(4) === 0 ? '' : hex(pick1(['Table', 'Figure', 'Gráfico']))}:${rng.below(2)}` : `cs:${rng.below(CFGS.length)}`;
      } else if (k < 75) code = `it:${1 + rng.below(3)}:${1 + rng.below(3)}`;
      else if (k < 80) code = pick1(['mn', 'mn', 'mn', `ma:${rng.below(9)}`, 'ms', 'me']);
      else if (k < 90) code = 'un';
      else code = 're';
      await page.evaluate(() => { window.__reset(); });
      const threw = await page.evaluate(async ([code, M]) => { try {
        const el = window.__el;
        window.__ev = [];
        const f = code.split(':');
        const s16 = (h) => String.fromCharCode(...h.split('.').filter(Boolean).map((x) => parseInt(x, 16)));
        const hx = (t) => [...Array(t.length).keys()].map((i) => t.charCodeAt(i).toString(16)).join('.');
        let res = '-';
        switch (f[0]) {
          case 'ty': el.type(s16(f[1])); break;
          case 'en': el.pressEnter(); break;
          case 'bs': el.pressBackspace(); break;
          case 'de': el.pressDelete(); break;
          case 'se': el.setSelection({ para: +f[1], offset: +f[2] }, { para: +f[3], offset: +f[4] }); break;
          case 'tk': el.toggleTrackChanges(f[1] === '1'); break;
          case 'if': el.insertField(['PAGE', 'NUMPAGES', 'DATE'][+f[1]]); break;
          case 'dt': el.insertDateTime(M.DTP[+f[1]], f[2] === '1', M.LANGS[+f[3]]); break;
          case 'di': el.insertDocumentInfo(M.INFOS[+f[1]], 'file.docx'); break;
          case 'at': el.insertAlignmentTab(M.ALIGN[+f[1]], M.RELTO[+f[2]], M.LEADER[+f[3]]); break;
          case 'uf': res = String(el.updateFields(['selection', 'all'][+f[1]], ['pages', 'all'][+f[2]])); break;
          case 'xr': { const o = { type: M.TYPES[+f[1]], item: s16(f[3]), kind: M.KINDS[+f[4]] }; if (f[2]) o.label = s16(f[2]); const fl = +f[5]; if (fl & 1) o.hyperlink = false; if (fl & 2) o.position = true; res = String(el.insertCrossReference(o)); break; }
          case 'ic': { const o = { label: s16(f[1]) }; if (f[2]) o.title = s16(f[2]); if (+f[3]) o.position = ['above', 'below'][+f[3] - 1]; if (+f[4]) o.excludeLabel = true; if (M.NUMS[+f[5]]) o.numbering = M.NUMS[+f[5]]; res = String(el.insertCaption(o)); break; }
          case 'tf': res = String(el.insertTableOfFigures({ label: s16(f[1]), ...M.TOFS[+f[2]] })); break;
          case 'ut2': res = String(el.updateTableOfFigures()); break;
          case 'tc': res = String(el.insertTableOfContents(M.DIALOGS[+f[1]], [undefined, true, false][+f[2]])); break;
          case 'tt': res = String(el.insertTocTemplate(M.TPL[+f[1]])); break;
          case 'ut': res = String(el.updateToc(['pages', 'all'][+f[1]])); break;
          case 'rt': res = String(el.removeTableOfContents()); break;
          case 'tl': el.setTocLevel(M.TLVL[+f[1]]); break;
          case 'mk': { const o = { text: s16(f[1]) }; if (M.MKIDS[+f[2]] !== undefined) o.identifier = M.MKIDS[+f[2]]; if (M.MKLVL[+f[3]] !== undefined) o.level = M.MKLVL[+f[3]]; res = String(el.markTocEntry(o)); break; }
          case 'cl': res = String(el.addCaptionLabel(s16(f[1]))); break;
          case 'dl': res = String(el.deleteCaptionLabel(s16(f[1]))); break;
          case 'cn': el.setCaptionNumbering(s16(f[1]), M.NUMS[+f[2]] ?? { format: 'ARABIC' }); break;
          case 'ac': el.setAutoCaption(f[1] ? { table: { label: s16(f[1]), position: ['above', 'below'][+f[2]] } } : undefined); break;
          case 'cs': el.captionSettings = JSON.parse(JSON.stringify(M.CFGS[+f[1]])); break;
          case 'it': el.insertTable(+f[1], +f[2]); break;
          case 'mn': {
            const r = await el.nextManualHyphenation();
            const c = el.manualHyph?.cur;
            res = r ? `${hx(r.word)}|${r.lang}|${r.points.join(',')}|${r.suggested}|${r.limit}` : 'null';
            window.__mhin = r && c ? `${c.para}:${c.start}:${c.end}:${r.limit}` : '-';
            break;
          }
          case 'ma': el.applyManualHyphenation(+f[1]); break;
          case 'ms': el.skipManualHyphenation(); break;
          case 'me': el.endManualHyphenation(); break;
          case 'un': el.undo(); break;
          case 're': el.redo(); break;
        }
        window.__res = res;
        return null;
      } catch (e) { return String(e); } }, [code, { DTP, LANGS, INFOS, ALIGN, RELTO, LEADER, TYPES, KINDS, NUMS, TOFS, DIALOGS, TPL, TLVL, MKIDS, MKLVL, CFGS }]);
      if (threw) {
        console.log(`${key} ${code}: ${threw}`);
        out.set(key, 'op', code);
        out.set(key, 'thrown', '1');
        break;
      }
    }
    out.set(key, 'op', code);
    const st = await page.evaluate(([j, code, STEPS]) => {
      const el = window.__el;
      const enc = (v) => (v instanceof Uint8Array ? { __u8: btoa(String.fromCharCode(...v)) } : v);
      const walk = (v) => (v && typeof v === 'object' && !(v instanceof Uint8Array) ? (Array.isArray(v) ? v.map(walk) : Object.fromEntries(Object.entries(v).map(([k, x]) => [k, walk(enc(x))]))) : enc(v));
      const state = el.parseState(el.stringifyState());
      const hx = (t) => [...Array(t.length).keys()].map((i) => t.charCodeAt(i).toString(16)).join('.');
      const o = window.__orc;
      // the scope descriptor of each refreshFieldCache call (the sha is made in Node)
      const fvs = o.fv.map((c) => (c.only ? { n: c.only.filter((k) => k !== '?').length, u: c.only.filter((k) => k === '?').length, keys: c.only.filter((k) => k !== '?').sort() } : null));
      const unk = o.fv.reduce((a, c) => a + c.vals.filter(([k]) => k === '?').length, 0);
      const fvIn = o.fv.map((c) => `${c.only ? `R${c.only.filter((k) => k !== '?').join(',')}^${c.only.filter((k) => k === '?').length}` : 'A'}!${c.vals.filter(([k]) => k !== '?').map(([k, v]) => `${k}:${hx(v)}`).join(';')}`).join('|');
      const pgIn = [...o.pg.entries()].map(([k, v]) => `${k}:${v}`).join(',');
      const ce = el.cellEdit;
      const labels = el.getCaptionLabels();
      const fmtN = (n) => `${n.format}${n.chapter ? `.${n.chapter.level}.${n.chapter.separator}` : ''}`;
      const a = el.getAutoCaption();
      const cap = `${labels.map((l) => `${hx(l.name)}~${l.builtIn ? 1 : 0}${l.position[0]}~${fmtN(el.getCaptionNumbering(l.name))}~${hx(el.nextCaptionNumber(l.name))}`).join(';')}|${a?.table ? `${hx(a.table.label)}${a.table.position[0]}` : (a ? 'e' : '-')}`;
      const m = el.manualHyph;
      const mh = m ? `${m.from.para}.${m.from.offset}-${(m.to ?? m.from).para}.${(m.to ?? m.from).offset}|${m.cur ? `${m.cur.para}.${m.cur.start}.${m.cur.end}.${m.cur.points.join(',')}.${m.cur.limit}` : '-'}|${[...m.skip.values()].reduce((s2, x) => s2 + x.size, 0)}` : '-';
      const items = [];
      for (const t of ['numbered', 'heading', 'bookmark', 'footnote', 'endnote']) items.push(`${t}=${el.getCrossReferenceItems(t).map((i) => `${i.id}~${hx(i.text)}`).join(';')}`);
      for (const l of labels) items.push(`caption:${hx(l.name)}=${el.getCrossReferenceItems('caption', l.name).map((i) => `${i.id}~${hx(i.text)}`).join(';')}`);
      const f1 = (v) => (v === undefined ? '-' : (typeof v === 'string' ? hx(v) : String(v)));
      const to = el.getTableOfContentsOptions();
      const tf = el.getTableOfFiguresOptions();
      const toco = `${el.hasTableOfContents() ? 1 : 0}${el.inTableOfContents() ? 1 : 0}|${el.getTocLevel()}|${to ? [f1(to.pageNumbers), f1(to.rightAlign), f1(to.leader), f1(to.hyperlinks), f1(to.format), f1(to.levels), f1(to.useStyles), to.styles ? to.styles.map((s2) => `${hx(s2.style)};${s2.level}`).join('/') : '-', f1(to.outlineLevels), f1(to.fields), f1(to.fieldId)].join(',') : 'null'}|${tf ? [hx(tf.label), f1(tf.includeLabel), f1(tf.pageNumbers), f1(tf.rightAlign), f1(tf.hyperlinks), f1(tf.leader)].join(',') : 'null'}`;
      const full = j % 8 === 0 || j === STEPS;
      const dtf = full ? ['en', 'pt', 'es', undefined].map((l) => el.dateTimeFormats(l).map((x) => `${hx(x.pattern)}=${hx(x.sample)}`).join(';')).join('|') : null;
      return {
        paras: walk(state.m.paragraphs),
        sel: JSON.stringify(state.s),
        cursel: JSON.stringify(el.selection),
        anc: JSON.stringify(state.anc ?? null),
        ev: window.__ev.join(' '),
        flags: `${el.isDirty() ? 1 : 0}${el.undoStack.length ? 1 : 0}${el.redoStack.length ? 1 : 0}:${el.editsSinceLoad}`,
        res: window.__res ?? '-',
        ctx: ce ? `c${ce.anchor}` : 'b',
        cap, mh, xri: items.join('\n'), toco, dtf,
        rs: String(el.refSeed.n),
        fvs, unk, unkc: o.unkc, fvIn, pgIn,
        pgKeys: [...o.pg.keys()].filter((k) => k !== '?'),
        pgCalls: o.pgCalls,
        mhin: window.__mhin ?? '-',
        unkInfo: window.__unkInfo ?? [],
        mhcode: code.startsWith('mn'),
      };
    }, [j, code, STEPS]);
    await page.evaluate(() => { window.__mhin = undefined; window.__res = undefined; window.__unkInfo = undefined; });
    out.set(key, 'paras', digest(revive(st.paras)));
    out.set(key, 'sel', st.sel);
    out.set(key, 'cursel', st.cursel);
    out.set(key, 'anc', st.anc);
    out.set(key, 'ev', st.ev || '-');
    out.set(key, 'flags', st.flags);
    out.set(key, 'res', st.res.length > 200 ? sha(st.res) : st.res);
    out.set(key, 'cap', st.cap.length > 200 ? sha(st.cap) : st.cap);
    out.set(key, 'mh', st.mh);
    if (process.env.UO_F10_TRACE && key.includes(process.env.UO_F10_TRACE)) console.log(`TS ${key} ${code}
${st.xri}
`);
    if (process.env.UO_F10_DUMP && key.includes(process.env.UO_F10_DUMP)) {
      const d = await page.evaluate(() => window.__el.model.paragraphs.map((p, i) => `DUMP ${i} s=${p.props.styleId ?? ''} ${p.runs.map((r) => JSON.stringify(r.text) + (r.field ? `[${r.field}]` : '') + (r.fieldBegin ? '{B}' : '') + (r.fieldEnd ? '{E}' : '')).join(' ').slice(0, 220)}`).concat([`DUMP anchors ${JSON.stringify(window.__el.tableAnchors.map((a) => a.beforePara))}`]));
      console.log(`TSDUMP ${key} ${code}`);
      console.log(d.join(String.fromCharCode(10)));
    }
    if (process.env.UO_F10_UNK && st.unkInfo.length) console.log(`UNK ${key} ${code}: ${st.unkInfo.join(' | ')}`);
    if (process.env.UO_F10_PG && key.includes(process.env.UO_F10_PG)) console.log(`PG ${key} ${code}: ${st.pgIn} | calls=${st.pgCalls}`);
    out.set(key, 'xri', sha(st.xri));
    out.set(key, 'toco', st.toco.length > 200 ? sha(st.toco) : st.toco);
    out.set(key, 'rs', st.rs);
    if (j === 0) out.set(key, 'i-seed', st.rs);
    if (j > 0) {
      out.set(key, 'fvs', `${st.fvs.length}${st.fvs.map((s2) => `;${s2 ? (s2.u ? `R${s2.n + s2.u}:?` : `R${s2.n}:${sha(s2.keys.join(',')).slice(0, 12)}`) : 'A'}`).join('')}`);
      out.set(key, 'pgc', st.pgCalls > 0 ? '1' : '0');
      out.set(key, 'pgv', `ok:${st.pgKeys.length}`);
      out.set(key, 'mhc', st.mhcode && st.mhin !== '-' ? '1' : 'x');
      out.set(key, 'unk', `${st.unk}.${st.unkc}`);
      out.set(key, 'i-fv', st.fvIn || '-');
      out.set(key, 'i-pg', st.pgIn || '-');
      out.set(key, 'i-mh', st.mhin);
    }
    if (st.dtf !== null) out.set(key, 'dtf', sha(st.dtf));
    if (j % 8 === 0 || j === STEPS) {
      const b64 = await page.evaluate(() => { try { const b = window.__el.saveDocx(); let s = ''; for (let i = 0; i < b.length; i += 0x8000) s += String.fromCharCode(...b.subarray(i, i + 0x8000)); return btoa(s); } catch { return null; } });
      let d = 'err';
      let sd = 'err';
      if (b64) {
        const pkg = DocxPackage.load(Uint8Array.from(Buffer.from(b64, 'base64')));
        d = Digest.of(Buffer.from(pkg.getPartText('word/document.xml') ?? '', 'utf8')).short;
        const t = pkg.getPartText('word/styles.xml');
        sd = t === undefined ? '-' : Digest.of(Buffer.from(t, 'utf8')).short;
      }
      out.set(key, 'docxml', d);
      out.set(key, 'stylexml', sd);
    }
  }
  await page.close();
  n++;
}
await browser.close();
server.close();
out.write(process.env.UO_F10_OUT ?? join(crate, 'tests', 'parity', 'd2b-f10-session.capture'));
console.log(`d2b-f10-session: ${n} documents x ${STEPS} steps`);
