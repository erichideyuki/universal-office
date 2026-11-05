// D2b-F9 (find, replace, go to, headings, the Navigation pane, the non-contiguous selection) in the D2
// editor harness (G17, U5, U16): the frozen `<universal-write>` element in headless Chromium
// (Playwright), driven through its public API and the TS-private entry points the D2a harness uses
// (`type`, `pressEnter`, `pressBackspace`, `pressDelete`, `stringifyState`, `parseState`, `enterCell`,
// `exitCell`). The page set-up is the one of `capture-d2a-session.mjs` (a script with top-level effects:
// it cannot be imported, so the server and browser launch are repeated here): the element initialises
// with the open default font so that the layout pass runs, `Math.random` = the testkit xorshift32,
// `Date` fixed (2026-01-02 03:04 UTC), timezone UTC, locale pt-BR (the query messages in Portuguese).
//
//   UO_TS_ROOT=<built checkout of main> node core/crates/uo-docx/js/capture-d2b-f9-session.mjs [--docs N] [--steps N]
//
// Writes tests/parity/d2b-f9-session.capture: per `<doc>#<j>` the D2a facets (op, paras, sel, anc, ev,
// flags, docxml) and F9's:
//   res    what the op returned (found/wrapped/replaced/count/error message, a boolean, a count, the
//          search results without their page);
//   ext    `getSelectionRanges()` (the extra ranges of a multi-selection and the main one);
//   ctx    the edit context (body, or the cell: anchor and hops);
//   hd     `getHeadings()` without the page (level, text, paragraph).
import { createRequire } from 'node:module';
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
const DOCS = opt('--docs', 80);
const STEPS = opt('--steps', 70);
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

const EVENTS = ['uw-change', 'uw-dirty-change', 'uw-input', 'uw-track-changes', 'uw-cellmode', 'uw-notemode', 'uw-hfmode', 'uw-tbmode', 'uw-protected', 'uw-blocked', 'uw-styles-change'];
const TEXTS = ['a', 'Olá ', ' ', 'ção', '😀', 'abc def', 'the'];
const hex = (t) => [...Array(t.length).keys()].map((i) => t.charCodeAt(i).toString(16)).join('.');
/** Shared by index with tests/parity_d2b_f9_session.rs (and d2b_f9_support). */
const SPECIAL_Q = ['^p', '^t', '^#', '^?', '^$', '^w', 'e-mail', '^x', '^d'];
const WILD_Q = ['<[a-z]@>', '[A-Z]*>', '(?)(?)', '<*>', '[0-9]{1;}', '^13', '<?@a>', '(<[!a-z ]@)'];
const GOTO = ['section', 'heading', 'bookmark', 'comment', 'footnote', 'endnote', 'field', 'table', 'graphic', 'equation', 'object'];
const TARGETS = ['', '1', '2', '+1', '-1', '+2', '0', '99', 'PAGE', 'HYPERLINK', 'A'];
const FLAGS = { c: 'matchCase', w: 'wholeWord', x: 'wildcards', s: 'soundsLike', a: 'allForms', p: 'prefix', f: 'suffix', u: 'ignorePunct', e: 'ignoreSpace' };

const revive = (v) => (v && typeof v === 'object' ? (v.__u8 !== undefined ? Uint8Array.from(Buffer.from(v.__u8, 'base64')) : (Array.isArray(v) ? v.map(revive) : Object.fromEntries(Object.entries(v).map(([k, x]) => [k, revive(x)])))) : v);

function digest(paras) {
  const s = new Stream();
  const c = new Canon(s);
  paras.forEach((p, i) => { s.tag('E').u32(i); c.value(p, 'paragraph'); });
  return s.finish().short;
}
const sha = (t) => Digest.of(Buffer.from(t, 'utf8')).short;

const out = new CaptureWriter([
  'D2b-F9 session in Chromium: the frozen <universal-write> driven by seeded scripts (find, replace, go to, headings, the Navigation pane, the non-contiguous selection)',
  'facets: op, paras, sel, anc, ev, flags, docxml, res, ext, ctx, hd',
]);
out.meta('steps', STEPS);
out.meta('browser', `chromium ${browser.version()}`);
out.meta('bundle', Digest.of(readFileSync(join(root, 'write/packages/editor/publish/index.js'))).short);
out.meta('math-override', 'none (no F9 state value goes through math)');

/** Documents with headings, tables, fields, bookmarks or notes first, then the rest. */
function pick() {
  const all = corpus();
  const rich = [];
  const plain = [];
  for (const f of all) {
    let t = '';
    try { t = DocxPackage.load(readBytes(f.file)).getPartText('word/document.xml') ?? ''; } catch { continue; }
    (/<w:(tbl|bookmarkStart|footnoteReference|fldSimple|fldChar|commentRangeStart)\b|w:val="(Heading|Ttulo)\d/.test(t) ? rich : plain).push(f);
  }
  const take = (list, n, mod) => list.filter((_, i) => i % mod === 0).slice(0, n);
  return [...take(rich, Math.ceil(DOCS * 0.7), 3), ...take(plain, Math.floor(DOCS * 0.3), 23)].slice(0, DOCS);
}
const files = process.env.UO_F9_ONLY ? pick().filter((f) => f.rel.includes(process.env.UO_F9_ONLY)) : pick();
let n = 0;
for (const { rel, file } of files) {
  const page = await context.newPage();
  await page.goto(`http://127.0.0.1:${port}/__d2b.html`);
  await page.waitForFunction(() => window.__ready === true);
  const bytes = readBytes(file);
  const ok = await page.evaluate(async ([b64, EVENTS]) => {
    const el = document.createElement('universal-write');
    el.setAttribute('fonts', '/write/packages/editor/demo/fonts');
    const ready = new Promise((r) => el.addEventListener('uw-ready', r, { once: true }));
    document.body.append(el);
    await ready;
    window.__el = el;
    window.__ev = [];
    for (const t of EVENTS) el.addEventListener(t, (e) => { const d = e.detail ?? {}; window.__ev.push(`${t}:${d.dirty ?? d.active ?? d.on ?? ''}`); });
    try {
      el.author = 'A';
      el.loadDocx(Uint8Array.from(atob(b64), (c) => c.charCodeAt(0)), { protected: false });
      return !!el.fonts;
    } catch { return false; }
  }, [Buffer.from(bytes).toString('base64'), EVENTS]);
  if (!ok) { await page.close(); continue; }
  let seed = 0xf9e9;
  for (let i = 0; i < rel.length; i++) seed = (Math.imul(seed, 31) + rel.charCodeAt(i)) >>> 0;
  const rng = new Rng(seed);
  const info = () => page.evaluate(() => {
    const el = window.__el;
    const words = [];
    for (const p of el.model.paragraphs) for (const m of p.runs.map((r) => r.text).join('').matchAll(/[\p{L}\p{N}]{2,}/gu)) { if (words.length < 400) words.push(m[0]); }
    return {
      lens: el.model.paragraphs.map((p) => p.runs.reduce((a, r) => a + r.text.length, 0)),
      heads: el.getHeadings().length,
      words,
      bms: el.getBookmarks({ hidden: true }).map((b) => b.name).slice(0, 10),
      tables: el.tableAnchors.length,
      search: el.searchList.length,
      cell: !!el.cellEdit,
    };
  });
  const q = (I) => {
    const k = rng.below(10);
    let f = '';
    let t;
    let fmt = 0;
    if (k < 5) { t = I.words.length ? I.words[rng.below(I.words.length)] : 'a'; if (rng.below(4) === 0) t = t.slice(0, 2); for (const c of 'cwpf') if (rng.below(4) === 0) f += c; }
    else if (k < 6) t = SPECIAL_Q[rng.below(SPECIAL_Q.length)];
    else if (k < 8) { t = WILD_Q[rng.below(WILD_Q.length)]; f = 'x'; }
    else if (k < 9) { t = ''; fmt = 1 + rng.below(23); }
    else { t = I.words.length ? I.words[rng.below(I.words.length)] : 'go'; f = rng.below(2) ? 's' : 'a'; }
    return `${f}:${hex(t)}:${fmt}`;
  };
  for (let j = 0; j <= STEPS; j++) {
    const key = `${rel}#${j}`;
    let code = 'init';
    if (j > 0) {
      const I = await info();
      const L = I.lens;
      const pos = () => { const para = rng.below(L.length); return { para, offset: rng.below(L[para] + 1) }; };
      const k = rng.below(100);
      if (k < 9) code = `ty:${hex(TEXTS[rng.below(TEXTS.length)])}`;
      else if (k < 11) code = 'en';
      else if (k < 15) code = 'bs';
      else if (k < 17) code = 'de';
      else if (k < 25) { const a = pos(); const f = rng.below(3) === 0 ? a : pos(); code = `se:${a.para}:${a.offset}:${f.para}:${f.offset}`; }
      else if (k < 27) code = `tk:${rng.below(2)}`;
      else if (k < 35) code = `fn:${q(I)}:${['down', 'up', 'all'][rng.below(3)]}`;
      else if (k < 41) code = `rm:${q(I)}:${rng.below(16)}:${rng.below(3) === 0 ? 1 + rng.below(23) : 0}`;
      else if (k < 45) code = `ra:${q(I)}:${rng.below(16)}:${rng.below(4) === 0 ? 1 : 0}:${rng.below(3) === 0 ? 1 + rng.below(23) : 0}`;
      else if (k < 49) code = `sm:${q(I)}:${rng.below(4) === 0 ? 1 : 0}`;
      else if (k < 51) code = `cm:${q(I)}:${rng.below(4) === 0 ? 1 : 0}`;
      else if (k < 52) code = `hm:${q(I)}:0`;
      else if (k < 54) code = 'sa';
      else if (k < 57) { const r = Array.from({ length: 1 + rng.below(3) }, () => { const a = pos(); const b = pos(); return `${a.para}.${a.offset}.${b.para}.${b.offset}`; }); code = `sr:${r.join(',')}`; }
      else if (k < 60) code = 'tb';
      else if (k < 66) code = `${['mh', 'ph', 'dh', 'ih', 'xh', 'sh', 'gh'][rng.below(7)]}:${rng.below(I.heads + 2) - 1}:${rng.below(I.heads + 2)}:${['before', 'after', 'sub'][rng.below(3)]}`;
      else if (k < 71) { const w = rng.below(GOTO.length); const t = GOTO[w] === 'bookmark' && I.bms.length ? hex(I.bms[rng.below(I.bms.length)]) : hex(TARGETS[rng.below(TARGETS.length)]); code = `go:${w}:${t}`; }
      else if (k < 74) code = `ns:${hex(I.words.length && rng.below(4) ? I.words[rng.below(I.words.length)] : ['a', 'de', 'e-mail', 'ção', ''][rng.below(5)])}:${rng.below(2)}${rng.below(2)}`;
      else if (k < 76) code = `nr:${rng.below(I.search + 2) - 1}`;
      else if (k < 77 && I.bms.length) code = `gb:${hex(I.bms[rng.below(I.bms.length)])}`;
      else if (k < 79 && I.tables) code = `ec:${rng.below(I.tables)}`;
      else if (k < 80) code = 'xc';
      else if (k < 90) code = 'un';
      else code = 're';
      const threw = await page.evaluate(([code, FLAGS, SPECIAL_Q, WILD_Q, GOTO]) => { try {
        const el = window.__el;
        window.__ev = [];
        const f = code.split(':');
        const s16 = (h) => String.fromCharCode(...h.split('.').filter(Boolean).map((x) => parseInt(x, 16)));
        const FORMATS = [undefined, { bold: true }, { italic: true }, { bold: false }, { underline: true }, { vertAlign: 'superscript' }, { vertAlign: null },
          { sizePt: 11 }, { sizePt: 12 }, { color: '000000' }, { highlight: true }, { highlight: false }, { font: 'Calibri' }, { paraStyle: 'Normal' },
          { paraStyle: 'Heading1' }, { charStyle: '' }, { bold: true, italic: true }, { allCaps: false, smallCaps: false, hidden: false }, { paraStyle: 'Ttulo1' },
          { strike: false, doubleStrike: false }, { sizePt: 10.5 }, { vertAlign: 'subscript' }, { color: 'ff0000' }, { font: 'arial' }];
        const REPLS = ['x', '', 'NOVO', 'a^pb', '^&^&', '\\1-\\2', '^c', '^t^l^m^n^s^-^~^+^=^^', '^0150^u8212^13^11^12^9^65', '\\\\x', '^', '^q', 'Ação ß ŉ', '😀x', '\\', 'ab\\9c^u55357'];
        const query = (fl, h, fm) => { const q = { text: s16(h) }; for (const c of fl) q[FLAGS[c]] = true; if (+fm) q.format = FORMATS[+fm]; return q; };
        const hx = (t) => [...Array(t.length).keys()].map((i) => t.charCodeAt(i).toString(16)).join('.');
        let res = '-';
        switch (f[0]) {
          case 'ty': el.type(s16(f[1])); break;
          case 'en': el.pressEnter(); break;
          case 'bs': el.pressBackspace(); break;
          case 'de': el.pressDelete(); break;
          case 'se': el.setSelection({ para: +f[1], offset: +f[2] }, { para: +f[3], offset: +f[4] }); break;
          case 'tk': el.toggleTrackChanges(f[1] === '1'); break;
          case 'fn': { const r = el.findNextMatch(query(f[1], f[2], f[3]), { direction: f[4] }); res = `${r.found ? 1 : 0}${r.wrapped ? 1 : 0}|${r.error ? hx(r.error) : ''}`; break; }
          case 'rm': { const r = el.replaceMatch(query(f[1], f[2], f[3]), REPLS[+f[4]], +f[5] ? { replaceFormat: FORMATS[+f[5]] } : {}); res = `${r.replaced ? 1 : 0}${r.found ? 1 : 0}${r.wrapped ? 1 : 0}|${r.error ? hx(r.error) : ''}`; break; }
          case 'ra': { const o = {}; if (f[5] === '1') o.within = 'selection'; if (+f[6]) o.replaceFormat = FORMATS[+f[6]]; const r = el.replaceAllMatches(query(f[1], f[2], f[3]), REPLS[+f[4]], o); res = `${r.count}|${r.error ? hx(r.error) : ''}`; break; }
          case 'sm': res = String(el.selectAllMatches(query(f[1], f[2], f[3]), f[4] === '1' ? { within: 'selection' } : {})); break;
          case 'cm': res = String(el.countMatches(query(f[1], f[2], f[3]), f[4] === '1' ? { within: 'selection' } : {})); break;
          case 'hm': res = String(el.highlightMatches(query(f[1], f[2], f[3]), {})); break;
          case 'sa': el.selectAll(); break;
          case 'sr': el.setSelectionRanges(f[1].split(',').map((r) => { const v = r.split('.').map(Number); return { start: { para: v[0], offset: v[1] }, end: { para: v[2], offset: v[3] } }; })); break;
          case 'tb': el.toggleRunProp('bold'); break;
          case 'mh': res = String(el.moveHeading(+f[1], +f[2])); break;
          case 'ph': res = String(el.promoteHeading(+f[1])); break;
          case 'dh': res = String(el.demoteHeading(+f[1])); break;
          case 'ih': res = String(el.insertHeading(+f[1], f[3])); break;
          case 'xh': res = String(el.deleteHeading(+f[1])); break;
          case 'sh': el.selectHeading(+f[1]); break;
          case 'gh': el.goToHeading(+f[1]); break;
          case 'go': res = String(el.goTo(GOTO[+f[1]], s16(f[2]))); break;
          case 'ns': res = el.search(s16(f[1]), { matchCase: f[2][0] === '1', wholeWord: f[2][1] === '1' }).map((r) => `${r.start},${r.end},${r.heading},${r.inTable ? 1 : 0},${hx(r.before)}|${hx(r.match)}|${hx(r.after)}`).join(';') || '-'; break;
          case 'nr': res = String(el.goToSearchResult(+f[1])); break;
          case 'gb': res = String(el.goToBookmark(s16(f[1]))); break;
          case 'ec': el.enterCell(+f[1], [{ r: 0, c: el.tableAnchors[+f[1]]?.table.rows[0]?.trPr?.gridBefore ?? 0 }], { para: 0, offset: 0 }); break;
          case 'xc': el.exitCell(); break;
          case 'un': el.undo(); break;
          case 're': el.redo(); break;
        }
        window.__res = res;
        return null;
      } catch (e) { return String(e); } }, [code, FLAGS, SPECIAL_Q, WILD_Q, GOTO]);
      if (threw) {
        // the TS threw (a stale position, a TS bug): the document's state is no longer comparable
        console.log(`${key} ${code}: ${threw}`);
        out.set(key, 'op', code);
        out.set(key, 'thrown', '1');
        break;
      }
    }
    out.set(key, 'op', code);
    const st = await page.evaluate(() => {
      const el = window.__el;
      const enc = (v) => (v instanceof Uint8Array ? { __u8: btoa(String.fromCharCode(...v)) } : v);
      const walk = (v) => (v && typeof v === 'object' && !(v instanceof Uint8Array) ? (Array.isArray(v) ? v.map(walk) : Object.fromEntries(Object.entries(v).map(([k, x]) => [k, walk(enc(x))]))) : enc(v));
      const state = el.parseState(el.stringifyState());
      const hx = (t) => [...Array(t.length).keys()].map((i) => t.charCodeAt(i).toString(16)).join('.');
      const ce = el.cellEdit;
      return {
        paras: walk(state.m.paragraphs),
        sel: JSON.stringify(state.s),
        cursel: JSON.stringify(el.selection),
        anc: JSON.stringify(state.anc ?? null),
        ev: window.__ev.join(' '),
        flags: `${el.isDirty() ? 1 : 0}${el.undoStack.length ? 1 : 0}${el.redoStack.length ? 1 : 0}:${el.editsSinceLoad}`,
        res: window.__res ?? '-',
        ext: JSON.stringify(el.getSelectionRanges()),
        ctx: ce ? `c${ce.anchor}:${ce.hops.map((h) => `${h.r}.${h.c}.${h.b ?? ''}`).join('/')}` : (el.noteEdit || el.hfEdit || el.tbEdit) ? 'o' : 'b',
        hd: el.getHeadings().map((h) => `${h.level}|${hx(h.text)}|${h.para}`).join('\n'),
      };
    });
    out.set(key, 'paras', digest(revive(st.paras)));
    out.set(key, 'sel', st.sel);
    out.set(key, 'cursel', st.cursel);
    out.set(key, 'anc', st.anc);
    out.set(key, 'ev', st.ev || '-');
    out.set(key, 'flags', st.flags);
    out.set(key, 'res', st.res.length > 200 ? sha(st.res) : st.res);
    out.set(key, 'ext', st.ext);
    out.set(key, 'ctx', st.ctx);
    out.set(key, 'hd', sha(st.hd));
    if (j % 8 === 0 || j === STEPS) {
      const b64 = await page.evaluate(() => { try { const b = window.__el.saveDocx(); let s = ''; for (let i = 0; i < b.length; i += 0x8000) s += String.fromCharCode(...b.subarray(i, i + 0x8000)); return btoa(s); } catch { return null; } });
      let d = 'err';
      if (b64) {
        const pkg = DocxPackage.load(Uint8Array.from(Buffer.from(b64, 'base64')));
        d = Digest.of(Buffer.from(pkg.getPartText('word/document.xml') ?? '', 'utf8')).short;
        if (process.env.UO_F9_XML) writeFileSync(join(process.env.UO_F9_XML, `ts-${j}.xml`), pkg.getPartText('word/document.xml') ?? '');
      }
      out.set(key, 'docxml', d);
    }
  }
  await page.close();
  n++;
}
await browser.close();
server.close();
out.write(join(crate, 'tests', 'parity', 'd2b-f9-session.capture'));
console.log(`d2b-f9-session: ${n} documents x ${STEPS} steps`);
