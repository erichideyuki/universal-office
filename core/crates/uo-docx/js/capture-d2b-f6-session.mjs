// D2b-F6 (review, comments, bookmarks and links) in the D2 editor harness (G17, U5, U16): the frozen
// `<universal-write>` element in headless Chromium (Playwright), driven through its public API and
// the TS-private entry points the D2a harness uses (`type`, `pressEnter`, `pressBackspace`,
// `pressDelete`, `stringifyState`, `parseState`, plus `markStories`). The page set-up is the one of
// `capture-d2a-session.mjs` (a script with top-level effects: it cannot be imported, so the server
// and browser launch are repeated here): the element initialises with the open default font so
// that the layout pass runs, `Math.random` = the testkit xorshift32, `Date` fixed (2026-01-02 03:04
// UTC), timezone UTC.
//
//   UO_TS_ROOT=<built checkout of main> node core/crates/uo-docx/js/capture-d2b-f6-session.mjs [--docs N] [--steps N]
//
// Writes tests/parity/d2b-f6-session.capture: per `<doc>#<j>` the D2a facets (op, paras, sel, anc,
// ev, flags, docxml) and F6's own:
//   rev    `revisionSpans()`: story, kind, author, dates, move, mark, para, start, end, text;
//   ra     `revisionAuthors()` with `authorColor`;
//   txt    every list of `markStories()` (body, cells, notes, header/footer parts) as the runs'
//          text with their revision, comment, bookmark and link marks (a digest per list);
//   cm     the comment list (`fullDoc.comments`);
//   bk     `getBookmarks({ hidden: true, sort: 'location' })`;
//   mo     `getMarkupOptions()` and `getMarkupMode()`.
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
const DOCS = opt('--docs', 60);
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

const EVENTS = ['uw-change', 'uw-dirty-change', 'uw-input', 'uw-track-changes', 'uw-cellmode', 'uw-notemode', 'uw-hfmode', 'uw-tbmode', 'uw-protected', 'uw-blocked', 'uw-revisions-changed', 'uw-comments-changed', 'uw-bookmarks-change', 'uw-markup'];
const TEXTS = ['a', 'Olá ', ' ', 'ção', '😀', '𝒜!', 'abc def'];
const NAMES = ['Alfa', 'alfa', 'Beta', '_oculto', 'Gama1', '1ruim', 'é2', 'x'.repeat(41), 'a b'];
const URLS = ['https://example.com/a', ' https://example.org ', 'mailto:a@b.c'];
const hex = (t) => [...Array(t.length).keys()].map((i) => t.charCodeAt(i).toString(16)).join('.');
const MARKUPS = ['{"insertionsDeletions":false}', '{"insertionsDeletions":true}', '{"authors":["A"]}', '{"authors":["B"]}', '{"authors":null}', '{"formatting":false}', '{"comments":false}', '{"balloons":"inline"}'];
const MODES = ['all', 'simple', 'none', 'original'];

const revive = (v) => (v && typeof v === 'object' ? (v.__u8 !== undefined ? Uint8Array.from(Buffer.from(v.__u8, 'base64')) : (Array.isArray(v) ? v.map(revive) : Object.fromEntries(Object.entries(v).map(([k, x]) => [k, revive(x)])))) : v);

function digest(paras) {
  const s = new Stream();
  const c = new Canon(s);
  paras.forEach((p, i) => { s.tag('E').u32(i); c.value(p, 'paragraph'); });
  return s.finish().short;
}
const sha = (t) => Digest.of(Buffer.from(t, 'utf8')).short;

const out = new CaptureWriter([
  'D2b-F6 session in Chromium: the frozen <universal-write> driven by seeded scripts (tracking, review commands, comments, bookmarks, links, markup options)',
  'facets: op, paras, sel, anc, ev, flags, docxml, rev, ra, txt, cm, bk, mo',
]);
out.meta('steps', STEPS);
out.meta('browser', `chromium ${browser.version()}`);
out.meta('bundle', Digest.of(readFileSync(join(root, 'write/packages/editor/publish/index.js'))).short);
out.meta('math-override', 'none (no F6 state value goes through math)');

/** Documents with something to review (revisions, comments, bookmarks, notes, headers) first, then the rest. */
function pick() {
  const all = corpus();
  const rich = [];
  const plain = [];
  for (const f of all) {
    let t = '';
    try { t = DocxPackage.load(readBytes(f.file)).getPartText('word/document.xml') ?? ''; } catch { continue; }
    (/<w:(ins|del|moveFrom|moveTo|bookmarkStart|commentRangeStart|footnoteReference)\b/.test(t) ? rich : plain).push(f);
  }
  const take = (list, n, mod) => list.filter((_, i) => i % mod === 0).slice(0, n);
  return [...take(rich, Math.ceil(DOCS * 0.6), 1), ...take(plain, Math.floor(DOCS * 0.4), 19)].slice(0, DOCS);
}
const files = pick();
let n = 0;
for (const { rel, file } of files) {
  const page = await context.newPage();
  await page.goto(`http://127.0.0.1:${port}/__d2b.html`);
  await page.waitForFunction(() => window.__ready === true);
  const bytes = readBytes(file);
  const hasHyper = /w:styleId="Hyperlink"/.test(DocxPackage.load(bytes).getPartText('word/styles.xml') ?? '');
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
  let seed = 0xf6f6;
  for (let i = 0; i < rel.length; i++) seed = (Math.imul(seed, 31) + rel.charCodeAt(i)) >>> 0;
  const rng = new Rng(seed);
  const info = () => page.evaluate(() => ({ lens: window.__el.model.paragraphs.map((p) => p.runs.reduce((a, r) => a + r.text.length, 0)), revs: window.__el.revisionSpans().length, comments: window.__el.fullDoc.comments.map((c) => c.id) }));
  for (let j = 0; j <= STEPS; j++) {
    const key = `${rel}#${j}`;
    let code = 'init';
    if (j > 0) {
      const I = await info();
      const L = I.lens;
      const pos = () => { const para = rng.below(L.length); return { para, offset: rng.below(L[para] + 1) }; };
      const k = rng.below(100);
      const tracking = await page.evaluate(() => window.__el.isTrackingChanges());
      if (k < 14) code = `ty:${hex(TEXTS[rng.below(TEXTS.length)])}`;
      else if (k < 18) code = 'en';
      else if (k < 24) code = 'bs';
      else if (k < 27) code = 'de';
      else if (k < 37) { const a = pos(); const f = rng.below(2) === 0 ? a : pos(); code = `se:${a.para}:${a.offset}:${f.para}:${f.offset}`; }
      else if (k < 41) code = `tk:${tracking ? 0 : 1}`;
      else if (k < 44) code = `au:${rng.below(2) ? 'A' : 'B'}`;
      else if (k < 50) code = `${rng.below(2) ? 'ac' : 'rj'}:${rng.below(I.revs + 1)}`;
      else if (k < 56) code = `${rng.below(2) ? 'ach' : 'rch'}:${rng.below(2)}`;
      else if (k < 60) code = `${rng.below(2) ? 'aal' : 'ral'}:${rng.below(2)}:${rng.below(2)}`;
      else if (k < 63) code = rng.below(2) ? 'nx' : 'pv';
      else if (k < 65) code = `sr:${rng.below(I.revs + 1)}`;
      else if (k < 68) code = `mo:${rng.below(MARKUPS.length)}`;
      else if (k < 70) code = `mm:${MODES[rng.below(4)]}`;
      else if (k < 74) code = `cm:${hex(['ok', ' nota ', '   ', 'ção 😀'][rng.below(4)])}`;
      else if (k < 76) code = `cr:${I.comments.length ? I.comments[rng.below(I.comments.length)] : 0}`;
      else if (k < 81) code = `bm:${rng.below(NAMES.length)}`;
      else if (k < 84) code = `bd:${rng.below(NAMES.length)}`;
      else if (k < 86 && hasHyper) code = `sl:${rng.below(URLS.length + 1)}`;
      else if (k < 89 && hasHyper) code = `hl:${rng.below(URLS.length)}:${rng.below(2)}:${rng.below(3)}`;
      else if (k < 90 && hasHyper) code = 'rl';
      else if (k < 96) code = 'un';
      else code = 're';
      await page.evaluate(([code, NAMES, URLS, MARKUPS]) => {
        const el = window.__el;
        window.__ev = [];
        const f = code.split(':');
        switch (f[0]) {
          case 'ty': el.type(String.fromCharCode(...f[1].split('.').map((h) => parseInt(h, 16)))); break;
          case 'en': el.pressEnter(); break;
          case 'bs': el.pressBackspace(); break;
          case 'de': el.pressDelete(); break;
          case 'se': el.setSelection({ para: +f[1], offset: +f[2] }, { para: +f[3], offset: +f[4] }); break;
          case 'tk': el.toggleTrackChanges(f[1] === '1'); break;
          case 'au': el.author = f[1]; break;
          case 'ac': el.acceptRevisionAt(+f[1]); break;
          case 'rj': el.rejectRevisionAt(+f[1]); break;
          case 'ach': el.acceptChange({ moveNext: f[1] === '1' }); break;
          case 'rch': el.rejectChange({ moveNext: f[1] === '1' }); break;
          case 'aal': el.acceptAllChanges({ shown: f[1] === '1', stopTracking: f[2] === '1' }); break;
          case 'ral': el.rejectAllChanges({ shown: f[1] === '1', stopTracking: f[2] === '1' }); break;
          case 'nx': el.nextChange(); break;
          case 'pv': el.previousChange(); break;
          case 'sr': { const e = el.revisionSpans()[+f[1]]; if (e && e.story === 'body' && e.list === el.model.paragraphs) el.selectRevision(+f[1]); break; }
          case 'mo': el.setMarkupOptions(JSON.parse(MARKUPS[+f[1]])); break;
          case 'mm': el.setMarkupMode(f[1]); break;
          case 'cm': el.addComment(String.fromCharCode(...f[1].split('.').filter(Boolean).map((h) => parseInt(h, 16)))); break;
          case 'cr': el.removeComment(+f[1]); break;
          case 'bm': el.insertBookmark(NAMES[+f[1]]); break;
          case 'bd': el.deleteBookmark(NAMES[+f[1]]); break;
          case 'sl': el.setLink(+f[1] < URLS.length ? URLS[+f[1]] : undefined); break;
          case 'hl': { const o = { address: URLS[+f[1]] }; if (f[2] === '1') o.tip = 'tip'; if (f[3] === '1') o.text = 'texto'; if (f[3] === '2') o.text = ''; el.insertHyperlink(o); break; }
          case 'rl': el.removeLink(); break;
          case 'un': el.undo(); break;
          case 're': el.redo(); break;
        }
      }, [code, NAMES, URLS, MARKUPS]);
    }
    out.set(key, 'op', code);
    const st = await page.evaluate(() => {
      const el = window.__el;
      const enc = (v) => (v instanceof Uint8Array ? { __u8: btoa(String.fromCharCode(...v)) } : v);
      const walk = (v) => (v && typeof v === 'object' && !(v instanceof Uint8Array) ? (Array.isArray(v) ? v.map(walk) : Object.fromEntries(Object.entries(v).map(([k, x]) => [k, walk(enc(x))]))) : enc(v));
      const state = el.parseState(el.stringifyState());
      const hx = (t) => [...Array(t.length).keys()].map((i) => t.charCodeAt(i).toString(16)).join('.');
      const runSig = (r) => `${hx(r.text)}|${r.props.ins ? r.props.ins.author : ''}|${r.props.del ? r.props.del.author : ''}|${r.props.fmt ? 1 : 0}|${(r.props.commentIds ?? []).join('.')}|${r.bookmark ? `${r.bookmark.end ? 'e' : 's'}${r.bookmark.id}${hx(r.bookmark.name)}` : ''}|${hx(r.props.link ?? '')}|${r.props.charStyle ?? ''}|${hx(r.props.linkTip ?? '')}`;
      const stories = el.markStories().map((s) => s.paras.map((p) => `${p.props.markIns ? 'i' : ''}${p.props.markDel ? 'd' : ''}${p.props.pfmt ? 'f' : ''}:${p.runs.map(runSig).join(',')}`).join(';'));
      const revs = el.revisionSpans().map((e) => [e.story, e.span.kind, e.span.author, e.span.date ?? '', e.span.dateUtc ?? '', e.span.move ? 1 : 0, e.span.mark ? 1 : 0, e.span.para, e.span.endPara ?? '', e.span.start, e.span.end, hx(e.span.text)].join('|'));
      const mk = el.getMarkupOptions();
      return {
        paras: walk(state.m.paragraphs),
        sel: JSON.stringify(state.s),
        anc: JSON.stringify(state.anc ?? null),
        ev: window.__ev.join(' '),
        flags: `${el.isDirty() ? 1 : 0}${el.undoStack.length ? 1 : 0}${el.redoStack.length ? 1 : 0}:${el.editsSinceLoad}`,
        rev: revs.join('\n'),
        ra: el.revisionAuthors().map((a) => `${hx(a)}=${el.authorColor(a)}`).join(','),
        stories,
        cm: el.fullDoc.comments.map((c) => `${c.id}|${hx(c.author ?? '')}|${hx(c.text)}`).join('\n'),
        bk: el.getBookmarks({ hidden: true, sort: 'location' }).map((b) => `${hx(b.name)}|${b.hidden ? 1 : 0}|${hx(b.text)}`).join('\n'),
        mo: `${el.getMarkupMode()}|${mk.insertionsDeletions ? 1 : 0}${mk.formatting ? 1 : 0}${mk.comments ? 1 : 0}|${mk.balloons}|${mk.authors ? mk.authors.map(hx).join(',') : 'null'}`,
      };
    });
    out.set(key, 'paras', digest(revive(st.paras)));
    out.set(key, 'sel', st.sel);
    out.set(key, 'anc', st.anc);
    out.set(key, 'ev', st.ev || '-');
    out.set(key, 'flags', st.flags);
    out.set(key, 'rev', sha(st.rev));
    out.set(key, 'ra', st.ra || '-');
    out.set(key, 'txt', sha(st.stories.join('\n#\n')));
    out.set(key, 'cm', sha(st.cm));
    out.set(key, 'bk', sha(st.bk));
    out.set(key, 'mo', st.mo);
    if (process.env.UO_F6_TEXT) { out.set(key, 'rev.text', st.rev); out.set(key, 'txt.text', st.stories.join('\n#\n')); out.set(key, 'cm.text', st.cm); out.set(key, 'bk.text', st.bk); }
    if (j % 8 === 0 || j === STEPS) {
      const b64 = await page.evaluate(() => { try { const b = window.__el.saveDocx(); let s = ''; for (let i = 0; i < b.length; i += 0x8000) s += String.fromCharCode(...b.subarray(i, i + 0x8000)); return btoa(s); } catch { return null; } });
      let d = 'err';
      if (b64) {
        const pkg = DocxPackage.load(Uint8Array.from(Buffer.from(b64, 'base64')));
        d = Digest.of(Buffer.from(pkg.getPartText('word/document.xml') ?? '', 'utf8')).short;
      }
      out.set(key, 'docxml', d);
    }
  }
  await page.close();
  n++;
}
await browser.close();
server.close();
out.write(join(crate, 'tests', 'parity', 'd2b-f6-session.capture'));
console.log(`d2b-f6-session: ${n} documents x ${STEPS} steps`);
