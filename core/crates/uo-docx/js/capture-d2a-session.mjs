// The D2 editor harness (G17, U5, U16; record uo-docx.md "D2a — session (architecture)", "Parity"):
// the frozen `<universal-write>` element in headless Chromium (Playwright), driven through its
// public API and its TS-private entry points (`type`, `pressEnter`, `pressBackspace`,
// `pressDelete`, `stringifyState`, `parseState` — reachable at run time, names kept by the
// bundle), one element per page load, with `Math.random` replaced by the testkit xorshift32 and
// `Date` fixed (2026-01-02 03:04 UTC, timezone UTC) by `addInitScript`.
//
//   UO_TS_ROOT=<built checkout of main> node core/crates/uo-docx/js/capture-d2a-session.mjs [--docs N] [--steps N]
//
// The math override of G17 (Math.sin/cos/tan/atan2/pow/hypot/exp/log → uo-jsnum wasm) is not
// installed: no value of the D2a scripts' state goes through math (typing, Enter, Backspace,
// Delete, the selection, undo/redo, tracking; the layout runs in the page but only its
// layout-independent mutations — syncSections, syncRunRPr — reach the compared state). The D2b
// harness installs it before any family whose model values use math (pictures, shapes, formulas).
//
// Writes tests/parity/d2a-session.capture: per `<doc>#<j>` (step j) the facets
//   op      the step, encoded (the Rust replay runs it);
//   paras   the canonical digest (js/canon.mjs) of `parseState(stringifyState()).m.paragraphs`
//           (the body as an undo would restore it: the JSON round trip of the state of now);
//   sel     the selection `s` (JSON);
//   anc     `anc.t` and the bookmark paragraphs `anc.b` (JSON);
//   ev      the model-derived events the step dispatched (`uw-*` types with their detail flag);
//   flags   `isDirty()`, can undo / can redo, `editsSinceLoad`;
//   docxml  every 8 steps and at the end: the short SHA-256 of `word/document.xml` of `saveDocx()`.
import { createRequire } from 'node:module';
import { createServer } from 'node:http';
import { readFileSync, existsSync, readdirSync } from 'node:fs';
import { join, extname, normalize } from 'node:path';
import { createHash } from 'node:crypto';
import { CaptureWriter, Rng, Stream } from '../../uo-testkit/js/testkit.mjs';
const Digest = { of: (b) => ({ short: createHash('sha256').update(b).digest('hex').slice(0, 32) }) };
import { Canon } from './canon.mjs';
import { crate, corpus, readBytes, root, ts } from './capture-r2-common.mjs';

const argv = process.argv.slice(2);
const opt = (k, d) => (argv.includes(k) ? Number(argv[argv.indexOf(k) + 1]) : d);
const DOCS = opt('--docs', 60);
const STEPS = opt('--steps', 60);
// `--dump <doc>#<step>:<para>`: prints the JSON of that paragraph of the restored state and stops
const DUMP = argv.includes('--dump') ? argv[argv.indexOf('--dump') + 1] : undefined;
const { DocxPackage } = await ts('package.js');

const pwFrom = [join(root, 'slides', 'package.json'), join(root, 'sheet', 'package.json')].find((p) => existsSync(p));
const { chromium } = createRequire(pwFrom)('playwright');

const MIME = { '.js': 'text/javascript', '.mjs': 'text/javascript', '.html': 'text/html', '.json': 'application/json', '.wasm': 'application/wasm', '.css': 'text/css', '.ttf': 'font/ttf', '.otf': 'font/otf', '.woff2': 'font/woff2' };
const server = createServer((req, res) => {
  const url = decodeURIComponent((req.url ?? '/').split('?')[0]);
  if (url === '/__d2a.html') {
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

// the Chromium build installed with Playwright; `UO_CHROMIUM` overrides (a Playwright newer than
// the installed browsers looks for a build that is not there)
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
  // the testkit xorshift32 (seed 1), as `FixedEnv::default()`
  let s = 1;
  Math.random = () => { s ^= s << 13; s >>>= 0; s ^= s >>> 17; s ^= s << 5; s >>>= 0; return s / 4294967296; };
  const T = Date.UTC(2026, 0, 2, 3, 4, 0);
  const D = Date;
  // eslint-disable-next-line no-global-assign
  Date = class extends D { constructor(...a) { if (a.length) super(...a); else super(T); } static now() { return T; } };
});

const EVENTS = ['uw-change', 'uw-dirty-change', 'uw-input', 'uw-track-changes', 'uw-cellmode', 'uw-notemode', 'uw-hfmode', 'uw-tbmode', 'uw-protected', 'uw-blocked'];
const TEXTS = ['a', 'Olá ', ' ', 'ção', '😀', '𝒜!', 'abc def'];
const hex = (t) => [...Array(t.length).keys()].map((i) => t.charCodeAt(i).toString(16)).join('.');

/** Uint8Arrays across the page boundary. */
const revive = (v) => (v && typeof v === 'object' ? (v.__u8 !== undefined ? Uint8Array.from(Buffer.from(v.__u8, 'base64')) : (Array.isArray(v) ? v.map(revive) : Object.fromEntries(Object.entries(v).map(([k, x]) => [k, revive(x)])))) : v);

function digest(paras) {
  const s = new Stream();
  const c = new Canon(s);
  paras.forEach((p, i) => { s.tag('E').u32(i); c.value(p, 'paragraph'); });
  return s.finish().short;
}

const out = new CaptureWriter([
  'D2a session in Chromium: the frozen <universal-write> driven by seeded scripts (typing, Enter, Backspace, Delete, selection, undo, redo, tracking)',
  'facets: op, paras, sel, anc, ev, flags, docxml',
]);
out.meta('steps', STEPS);
out.meta('browser', `chromium ${browser.version()}`);
out.meta('bundle', Digest.of(readFileSync(join(root, 'write/packages/editor/publish/index.js'))).short);
out.meta('math-override', 'none (no D2a state value goes through math)');

const files = corpus().filter((_, i) => i % 19 === 5).slice(0, DOCS).filter((f) => !DUMP || DUMP.startsWith(`${f.rel}#`));
let n = 0;
for (const { rel, file } of files) {
  const page = await context.newPage();
  await page.goto(`http://127.0.0.1:${port}/__d2a.html`);
  await page.waitForFunction(() => window.__ready === true);
  const bytes = readBytes(file);
  // the element initialises with the open default font (Liberation Sans) so that the layout runs
  // after every step, as in the editor (its model mutations, syncSections and syncRunRPr, are part
  // of the compared state: G21a)
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
  let seed = 0x5e55;
  for (let i = 0; i < rel.length; i++) seed = (Math.imul(seed, 31) + rel.charCodeAt(i)) >>> 0;
  const rng = new Rng(seed);
  const lens = () => page.evaluate(() => window.__el.model.paragraphs.map((p) => p.runs.reduce((a, r) => a + r.text.length, 0)));
  for (let j = 0; j <= STEPS; j++) {
    const key = `${rel}#${j}`;
    let code = 'init';
    if (j > 0) {
      const L = await lens();
      const pos = () => { const para = rng.below(L.length); return { para, offset: rng.below(L[para] + 1) }; };
      const k = rng.below(100);
      if (k < 32) code = `ty:${hex(TEXTS[rng.below(TEXTS.length)])}`;
      else if (k < 42) code = 'en';
      else if (k < 56) code = 'bs';
      else if (k < 62) code = 'de';
      else if (k < 76) { const a = pos(); const f = rng.below(2) === 0 ? a : pos(); code = `se:${a.para}:${a.offset}:${f.para}:${f.offset}`; }
      else if (k < 80) code = `tk:${rng.below(2)}`;
      else if (k < 91) code = 'un';
      else code = 're';
      await page.evaluate((code) => {
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
          case 'un': el.undo(); break;
          case 're': el.redo(); break;
        }
      }, code);
    }
    out.set(key, 'op', code);
    const st = await page.evaluate(() => {
      const el = window.__el;
      const enc = (v) => (v instanceof Uint8Array ? { __u8: btoa(String.fromCharCode(...v)) } : v);
      const walk = (v) => (v && typeof v === 'object' && !(v instanceof Uint8Array) ? (Array.isArray(v) ? v.map(walk) : Object.fromEntries(Object.entries(v).map(([k, x]) => [k, walk(enc(x))]))) : enc(v));
      const state = el.parseState(el.stringifyState());
      return {
        paras: walk(state.m.paragraphs),
        sel: JSON.stringify(state.s),
        anc: JSON.stringify(state.anc ?? null),
        ev: window.__ev.join(' '),
        flags: `${el.isDirty() ? 1 : 0}${el.undoStack.length ? 1 : 0}${el.redoStack.length ? 1 : 0}:${el.editsSinceLoad}`,
      };
    });
    if (DUMP && DUMP.startsWith(`${key}:`) && !DUMP.endsWith(":xml")) {
      const para = Number(DUMP.slice(key.length + 1));
      console.log(JSON.stringify(revive(st.paras)[para], (k, v) => (v instanceof Uint8Array ? `#${v.length}` : v)));
      process.exit(0);
    }
    out.set(key, 'paras', digest(revive(st.paras)));
    out.set(key, 'sel', st.sel);
    out.set(key, 'anc', st.anc);
    out.set(key, 'ev', st.ev || '-');
    out.set(key, 'flags', st.flags);
    if (j % 8 === 0 || j === STEPS) {
      const b64 = await page.evaluate(() => { try { const b = window.__el.saveDocx(); let s = ''; for (let i = 0; i < b.length; i += 0x8000) s += String.fromCharCode(...b.subarray(i, i + 0x8000)); return btoa(s); } catch { return null; } });
      let d = 'err';
      if (b64) {
        const pkg = DocxPackage.load(Uint8Array.from(Buffer.from(b64, 'base64')));
        d = Digest.of(Buffer.from(pkg.getPartText('word/document.xml') ?? '', 'utf8')).short;
        if (DUMP === `${key}:xml`) { console.log(pkg.getPartText('word/document.xml')); process.exit(0); }
      }
      out.set(key, 'docxml', d);
    }
  }
  await page.close();
  n++;
}
await browser.close();
server.close();
out.write(join(crate, 'tests', 'parity', 'd2a-session.capture'));
console.log(`d2a-session: ${n} documents x ${STEPS} steps`);
