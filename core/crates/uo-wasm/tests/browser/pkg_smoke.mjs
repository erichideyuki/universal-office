// Package smoke (wave 6, front P): the PUBLISHED @universal-office/write (write/packages/editor/publish, from `node tools/build-pkg.mjs
// font-files document-viewer write`) in a headless Chromium, the way a host page uses it: no esbuild of the sources, no harness entry.
//   (a) default: <universal-write> without engine loads and fetches no .wasm and no rust-engine chunk;
//   (b) editor first: engine="rust" fetches chunks/uo-write-editor.wasm ONCE (never uo-write.wasm), types, then
//       <universal-document-viewer engine="rust"> from viewer.js loads a document on the same module;
//   (c) viewer first: the viewer works, and the editor's engine="rust" initialize() REJECTS naming uo-write-editor.wasm.
// Tooling is copy-adapted (declared) from editor_switch_lib.mjs: the http server and the headless-shell finder. Playwright comes from
// UO_TS_ROOT (sheet/node_modules). Headless only, no visible window. Prints `pkg_smoke: ok`.
import { createRequire } from 'node:module';
import { createServer } from 'node:http';
import { readFileSync, existsSync, readdirSync } from 'node:fs';
import { join, resolve, extname, basename, dirname } from 'node:path';
import { fileURLToPath } from 'node:url';
import { fixtureDocs } from '../support/paint-common.mjs';

const here = dirname(fileURLToPath(import.meta.url));
const repo = resolve(here, '../../../../..');
const root = process.env.UO_TS_ROOT ? resolve(process.env.UO_TS_ROOT) : repo;
const PKG = process.env.UO_PKG_DIR ?? join(repo, 'write/packages/editor/publish');
const FONTS = process.env.UO_FONT_FILES_DIR ?? join(repo, 'shared/fonts/packages/font-files/publish');
for (const d of [PKG, FONTS]) if (!existsSync(d)) { console.error(`pkg_smoke: ${d} does not exist (node tools/build-pkg.mjs font-files document-viewer write)`); process.exit(1); }
const doc = fixtureDocs()[0];

const MIME = { '.js': 'text/javascript', '.wasm': 'application/wasm', '.ttf': 'font/ttf', '.json': 'application/json', '.docx': 'application/octet-stream' };
const requests = [];
const server = createServer((req, res) => {
  const url = decodeURIComponent(req.url.split('?')[0]);
  requests.push(url);
  const send = (file) => res.writeHead(200, { 'content-type': MIME[extname(file)] ?? 'application/octet-stream' }).end(readFileSync(file));
  const under = (base, rel) => { const f = resolve(join(base, rel)); return f.startsWith(resolve(base)) && existsSync(f) ? f : null; };
  if (url === '/') return res.writeHead(200, { 'content-type': 'text/html' }).end('<!doctype html><meta charset="utf-8"><body style="margin:0"></body>');
  if (url === '/doc.docx') return send(doc);
  if (url.startsWith('/pkg/')) { const f = under(PKG, url.slice(5)); return f ? send(f) : res.writeHead(404).end(); }
  if (url.startsWith('/font-files/')) { const f = under(FONTS, url.slice(12)); return f ? send(f) : res.writeHead(404).end(); }
  res.writeHead(404).end();
});
await new Promise((r) => server.listen(0, '127.0.0.1', r));

const { chromium } = createRequire(join(root, 'sheet/package.json'))('playwright');
const installed = (() => {
  const base = join(process.env.LOCALAPPDATA ?? '', 'ms-playwright');
  if (!existsSync(base)) return undefined;
  const dirs = readdirSync(base).filter((d) => d.startsWith('chromium_headless_shell-')).sort();
  const exe = dirs.length && join(base, dirs[dirs.length - 1], 'chrome-headless-shell-win64', 'chrome-headless-shell.exe');
  return exe && existsSync(exe) ? exe : undefined;
})();
const browser = await chromium.launch({ headless: true, executablePath: process.env.UO_CHROMIUM ?? installed });

let failed = 0;
const check = (ok, what) => { console.log(`${ok ? 'ok  ' : 'FAIL'} ${what}`); if (!ok) failed++; };
const count = (re) => requests.filter((u) => re.test(u)).length;

async function scenario(name, run) {
  requests.length = 0;
  const page = await (await browser.newContext({ viewport: { width: 1100, height: 800 } })).newPage();
  const errors = [];
  page.on('pageerror', (e) => errors.push(e.message));
  page.on('crash', () => { console.log('PAGE CRASHED'); process.exit(2); });
  await page.goto(`http://127.0.0.1:${server.address().port}/`);
  console.log(`-- ${name}`);
  await run(page, errors);
  await page.context().close();
}

await scenario('(a) default engine: nothing of the Rust engine is fetched', async (page, errors) => {
  const r = await page.evaluate(async () => {
    await import('/pkg/index.js');
    const el = document.createElement('universal-write');
    el.setAttribute('manual-init', ''); el.setAttribute('fonts', '/font-files');
    el.style.cssText = 'display:block;width:1000px;height:700px';
    document.body.append(el);
    await el.initialize();
    const bytes = new Uint8Array(await (await fetch('/doc.docx')).arrayBuffer());
    el.loadDocx(bytes, { protected: false });
    for (let i = 0; i < 200 && !(el.layoutPageCount() > 0); i++) await new Promise((s) => setTimeout(s, 100));
    return { pages: el.layoutPageCount(), stats: customElements.get('universal-write').engineStats() };
  });
  check(r.pages > 0, `ts editor lays out the document (${r.pages} page(s))`);
  check(r.stats === null, 'engineStats() is null without engine="rust"');
  check(count(/\.wasm$/) === 0, `0 .wasm requests (got ${count(/\.wasm$/)})`);
  check(count(/rust-engine-/) === 0, `0 rust-engine chunk requests (got ${count(/rust-engine-/)})`);
  check(errors.length === 0, `no page errors ${errors.join(' | ')}`);
});

await scenario('(b) editor first, then a viewer on the same module', async (page, errors) => {
  const r = await page.evaluate(async () => {
    await import('/pkg/index.js');
    const sleep = (ms) => new Promise((s) => setTimeout(s, ms));
    const el = document.createElement('universal-write');
    el.setAttribute('engine', 'rust'); el.setAttribute('manual-init', ''); el.setAttribute('fonts', '/font-files');
    el.style.cssText = 'display:block;width:1000px;height:700px';
    document.body.append(el);
    await el.initialize();
    const bytes = new Uint8Array(await (await fetch('/doc.docx')).arrayBuffer());
    el.loadDocx(bytes, { protected: false });
    for (let i = 0; i < 200 && !(el.layoutPageCount() > 0); i++) await sleep(100);
    const pages = el.layoutPageCount();
    const clean = el.isDirty();
    // type one character: the keyboard reaches the Rust session through the editor's input
    el.focus();
    el.setSelection?.({ para: 0, offset: 0 });
    const typed = (() => { try { el.insertText?.('x'); return typeof el.insertText === 'function'; } catch { return false; } })();
    await sleep(300);
    const stats = customElements.get('universal-write').engineStats();
    // the viewer of the same package, loaded after the editor
    await import('/pkg/viewer.js');
    const v = document.createElement('universal-document-viewer');
    v.setAttribute('engine', 'rust'); v.setAttribute('fonts', '/font-files');
    v.style.cssText = 'display:block;width:800px;height:600px';
    document.body.append(v);
    await v.loadDocx(bytes);
    await sleep(500);
    return { pages, clean, typed, dirty: el.isDirty(), stats: stats !== null, viewerPages: v.getPageCount() };
  });
  check(r.pages > 0, `rust editor lays out the document (${r.pages} page(s))`);
  check(r.clean === false, 'a freshly loaded document is not dirty');
  if (!r.typed) await keyboardType(page);
  const dirty = r.typed ? r.dirty : await page.evaluate(() => document.querySelector('universal-write').isDirty());
  check(dirty === true, 'typing one character makes the document dirty');
  check(r.stats, 'engineStats() is not null');
  check(r.viewerPages > 0, `the viewer (engine="rust") loads on the editor's module (${r.viewerPages} page(s))`);
  check(count(/uo-write-editor\.wasm$/) === 1, `uo-write-editor.wasm fetched once (got ${count(/uo-write-editor\.wasm$/)})`);
  check(count(/uo-write\.wasm$/) === 0, `uo-write.wasm not fetched (got ${count(/uo-write\.wasm$/)}): one engine module per page`);
  check(errors.length === 0, `no page errors ${errors.join(' | ')}`);
});

async function keyboardType(page) {
  const box = await page.locator('universal-write').boundingBox();
  await page.mouse.click(box.x + box.width / 2, box.y + 200);
  await page.keyboard.type('x');
  await page.waitForTimeout(300);
}

await scenario('(c) viewer first: the rust editor refuses, naming the editor module', async (page, errors) => {
  const r = await page.evaluate(async () => {
    const sleep = (ms) => new Promise((s) => setTimeout(s, ms));
    await import('/pkg/viewer.js');
    const bytes = new Uint8Array(await (await fetch('/doc.docx')).arrayBuffer());
    const v = document.createElement('universal-document-viewer');
    v.setAttribute('engine', 'rust'); v.setAttribute('fonts', '/font-files');
    v.style.cssText = 'display:block;width:800px;height:600px';
    document.body.append(v);
    await v.loadDocx(bytes);
    await sleep(500);
    const viewerPages = v.getPageCount();
    await import('/pkg/index.js');
    const el = document.createElement('universal-write');
    el.setAttribute('engine', 'rust'); el.setAttribute('manual-init', ''); el.setAttribute('fonts', '/font-files');
    document.body.append(el);
    let message = null;
    try { await el.initialize(); } catch (e) { message = String(e?.message ?? e); }
    return { viewerPages, message };
  });
  check(r.viewerPages > 0, `the viewer works (${r.viewerPages} page(s))`);
  check(r.message !== null && r.message.includes('uo-write-editor.wasm'), `the editor's initialize() rejects naming uo-write-editor.wasm (${String(r.message).slice(0, 160)})`);
  check(count(/uo-write\.wasm$/) === 1, `the viewer fetched uo-write.wasm once (got ${count(/uo-write\.wasm$/)})`);
  check(errors.length === 0, `no page errors apart from the awaited rejection ${errors.join(' | ')}`);
});

await browser.close();
server.close();
if (failed) { console.log(`pkg_smoke: ${failed} check(s) FAILED`); process.exit(1); }
console.log('pkg_smoke: ok');
