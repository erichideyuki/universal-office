// fontUsage() of the frozen `<universal-write>` (universal-write.ts 2317, LQ15) in headless Chromium,
// for the port in uo-docx `session::font_usage` (record uo-bindings.md "Milestone 2: the session in
// wasm", fontUsage). Boilerplate of capture-d2a-session.mjs (fixed `Date`, xorshift `Math.random`).
//
//   UO_TS_ROOT=<frozen checkout> UO_CHROMIUM=<chrome.exe> node core/crates/uo-docx/js/capture-font-usage.mjs [--docs N]
//
// Writes tests/parity/font-usage.capture: per document four points `<doc>#<k>`:
//   #0 after the open;
//   #1 after selectAll(), toggleRunProp('bold'), the caret at the end and typing "Olá ß";
//   #2 after entering the header/footer part shown on page 0 (`enterHeaderFooter('header', 0)`) and
//      typing "Olá ß" in it — only when that part exists in the file (`hfpath` names it), else '-';
//   #3 after leaving it, entering the first cell of the first body table (`enterCell(0, [{ r: 0, c: 0 }])`)
//      and typing "Olá ß" — only when the body has a table;
// facets: `fu` the short SHA-256 of `JSON.stringify(fontUsage())`, `n` its length, and on #2 `hfpath`.
import { createRequire } from 'node:module';
import { createServer } from 'node:http';
import { readFileSync, existsSync, readdirSync } from 'node:fs';
import { join, extname, normalize } from 'node:path';
import { createHash } from 'node:crypto';
import { CaptureWriter } from '../../uo-testkit/js/testkit.mjs';
import { crate, corpus, readBytes, root } from './capture-r2-common.mjs';

const Digest = { of: (b) => ({ short: createHash('sha256').update(b).digest('hex').slice(0, 32) }) };
const argv = process.argv.slice(2);
const opt = (k, d) => (argv.includes(k) ? Number(argv[argv.indexOf(k) + 1]) : d);
const DOCS = opt('--docs', 100);
// `--dump <doc>#<k>`: prints that point's fontUsage() JSON and stops
const DUMP = argv.includes('--dump') ? argv[argv.indexOf('--dump') + 1] : undefined;

const pwFrom = [join(root, 'slides', 'package.json'), join(root, 'sheet', 'package.json')].find((p) => existsSync(p));
const { chromium } = createRequire(pwFrom)('playwright');

const MIME = { '.js': 'text/javascript', '.mjs': 'text/javascript', '.html': 'text/html', '.json': 'application/json', '.wasm': 'application/wasm', '.css': 'text/css', '.ttf': 'font/ttf', '.otf': 'font/otf', '.woff2': 'font/woff2' };
const server = createServer((req, res) => {
  const url = decodeURIComponent((req.url ?? '/').split('?')[0]);
  if (url === '/__fu.html') {
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

const out = new CaptureWriter([
  'fontUsage() of the frozen <universal-write> in Chromium (LQ15): open, select all + bold + typing, a header/footer part, a body cell',
  'facets: fu (short SHA-256 of JSON.stringify(fontUsage())), n (entries), hfpath (#2)',
]);
out.meta('browser', `chromium ${browser.version()}`);
out.meta('bundle', Digest.of(readFileSync(join(root, 'write/packages/editor/publish/index.js'))).short);
out.meta('typed', 'Olá ß');

const files = corpus().filter((_, i) => i % 7 === 3).slice(0, DOCS).filter((f) => !DUMP || DUMP.startsWith(`${f.rel}#`));
let n = 0;
for (const { rel, file } of files) {
  const page = await context.newPage();
  await page.goto(`http://127.0.0.1:${port}/__fu.html`);
  await page.waitForFunction(() => window.__ready === true);
  const ok = await page.evaluate(async (b64) => {
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
  }, Buffer.from(readBytes(file)).toString('base64'));
  if (!ok) { await page.close(); continue; }
  const point = async (k, f) => {
    const key = `${rel}#${k}`;
    const r = await page.evaluate(f);
    if (DUMP === key) { console.log(r.json); process.exit(0); }
    if (r.skip) { out.set(key, 'fu', '-'); return; }
    out.set(key, 'fu', Digest.of(Buffer.from(r.json, 'utf8')).short);
    out.set(key, 'n', String(r.n));
    if (r.hfpath !== undefined) out.set(key, 'hfpath', r.hfpath);
  };
  const usage = 'const u = el.fontUsage(); return { json: JSON.stringify(u), n: u.length };';
  await point(0, new Function(`const el = window.__el; ${usage}`));
  await point(1, new Function(`const el = window.__el;
    el.selectAll(); el.toggleRunProp('bold');
    const ps = el.model.paragraphs; const last = ps.length - 1;
    const len = ps[last].runs.reduce((a, r) => a + r.text.length, 0);
    el.setSelection({ para: last, offset: len }, { para: last, offset: len });
    el.type('Olá ß');
    ${usage}`));
  await point(2, new Function(`const el = window.__el;
    const before = new Set(); for (const s of el.fullDoc.sections) for (const set of [s.headers, s.footers]) for (const k of Object.keys(set ?? {})) before.add(k);
    const paths = (el.fullDoc.sections ?? []).flatMap((s) => [...(s.headerRefs ?? []), ...(s.footerRefs ?? [])].map((r) => r.path));
    if (!paths.length) return { skip: true };
    el.enterHeaderFooter('header', 0);
    if (!el.hfEdit || !paths.includes(el.hfEdit.path)) { if (el.hfEdit) el.exitHeaderFooter(); return { skip: true }; }
    const path = el.hfEdit.path;
    el.setSelection({ para: 0, offset: 0 }, { para: 0, offset: 0 });
    el.type('Olá ß');
    const u = el.fontUsage();
    el.exitHeaderFooter();
    return { json: JSON.stringify(u), n: u.length, hfpath: path };`));
  await point(3, new Function(`const el = window.__el;
    if (!el.tableAnchors.length) return { skip: true };
    if (!el.enterCell(0, [{ r: 0, c: 0 }])) return { skip: true };
    el.type('Olá ß');
    ${usage}`));
  await page.close();
  n++;
}
await browser.close();
server.close();
out.write(join(crate, 'tests', 'parity', 'font-usage.capture'));
console.log(`font-usage: ${n} documents x 4 points`);
