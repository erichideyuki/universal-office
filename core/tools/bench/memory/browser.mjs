// Browser-tab side of the memory benchmark: the TS viewer 0.9.0 (published build) and the Rust wasm path showing the
// same generated 100-page document in Chromium (Playwright). One browser and one page per scenario, never overlapping.
//
//   export UO_TS_VIEWER=<folder with the published viewer: index.js, chunks/, fonts/>   (main checkout:
//          D:/Projects/OfficeNext/write/packages/viewer/publish, version 0.9.0, read-only)
//   export UO_PLAYWRIGHT=<folder of the playwright package>   (default: the sheet/node_modules one of the main checkout)
//   node core/tools/bench/memory/browser.mjs [--doc pages100] [--webkit] [--out <file.json>]
//
// What is measured (Chromium; the numbers are the page of the tab, not the browser process):
//   js_heap_used       CDP Performance.getMetrics JSHeapUsedSize after HeapProfiler.collectGarbage (x2)
//   js_heap_total      JSHeapTotalSize (committed V8 heap)
//   uas_bytes          performance.measureUserAgentSpecificMemory().bytes (needs cross-origin isolation, which the
//                      local server sets with COOP/COEP): JS + DOM + wasm memory + ArrayBuffers of the page
//   wasm_bytes         memory.buffer.byteLength of the Rust module (the high-water; never shrinks)
// Limits: no Rust painter exists yet (frontier D), so the Rust page lays out, encodes every page (keeping the first five
// messages as a viewer's visible pages) and does not decode or paint: its numbers are the engine side only, a lower bound
// for the Rust viewer; the TS viewer's include the DOM and the canvases it painted. WebKit has no memory API: only the
// wasm byteLength is read there.
import { createServer } from 'node:http';
import { readFile } from 'node:fs/promises';
import { writeFileSync, readdirSync } from 'node:fs';
import { resolve, join, dirname, sep, extname } from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';

const here = dirname(fileURLToPath(import.meta.url));
const core = resolve(here, '../../..');
const repo = resolve(core, '..');
const args = process.argv.slice(2);
const opt = (k, d) => { const i = args.indexOf(k); return i >= 0 ? args[i + 1] : d; };
const docName = opt('--doc', 'pages100');
const target = resolve(process.env.CARGO_TARGET_DIR ?? join(core, 'target/agent-membench'));
const tsViewer = resolve(process.env.UO_TS_VIEWER ?? 'D:/Projects/OfficeNext/write/packages/viewer/publish');
const pwDir = resolve(process.env.UO_PLAYWRIGHT ?? 'D:/Projects/OfficeNext/sheet/node_modules/playwright');
const { chromium, webkit } = await import(pathToFileURL(join(pwDir, 'index.mjs')).href);

// installed browsers (a newer Playwright than the downloaded revisions: point at the executables, nothing is downloaded)
const pw = join(process.env.LOCALAPPDATA ?? '', 'ms-playwright');
const exe = (prefix, rel) => { try { const d = readdirSync(pw).filter((n) => n.startsWith(prefix)).sort().at(-1); return d ? join(pw, d, rel) : undefined; } catch { return undefined; } };
const CHROMIUM = process.env.UO_CHROMIUM ?? exe('chromium-', 'chrome-win64/chrome.exe');
const WEBKIT = process.env.UO_WEBKIT ?? exe('webkit-', 'Playwright.exe');

const MIME = { '.js': 'text/javascript', '.mjs': 'text/javascript', '.wasm': 'application/wasm', '.html': 'text/html', '.ttf': 'font/ttf', '.json': 'application/json', '.txt': 'text/plain', '.png': 'image/png' };
const MAP = [
  ['/docs/', join(target, 'docs')],
  ['/ts/', tsViewer],
  ['/js/', join(core, 'crates/uo-wasm/js')],
  ['/wasm/', target],
  ['/fonts/site/', join(repo, 'site/assets/fonts')],
  ['/fonts/shared/', join(repo, 'shared/fonts/files')],
  ['/hyph/', join(core, 'crates/uo-fonts/data/hyphen')],
];
const RUST_PAGE = `<!doctype html><meta charset="utf-8"><title>rust</title><body><script type="module">
import { writeImports, bindWrite } from '/js/write.mjs';
const FONTS = [['Liberation Sans','site','liberationsans'],['Liberation Serif','shared','liberationserif'],['Liberation Mono','shared','liberationmono'],['Carlito','shared','carlito'],['Caladea','shared','caladea']];
const VARIANTS = ['regular','bold','italic','boldItalic'];
const get = async (u) => new Uint8Array(await (await fetch(u)).arrayBuffer());
window.__keep = null;
window.__run = async (docUrl, wasmUrl, phone) => {
  const { imports, host } = writeImports();
  const { instance } = await WebAssembly.instantiate(await get(wasmUrl), imports);
  const w = instance.exports, uo = bindWrite(w, host);
  const env = uo.newEnv({ phone });
  for (const [family, dir, stem] of FONTS) {
    const faces = [];
    for (const [i, s] of ['regular','bold','italic','bolditalic'].entries()) faces.push({ variant: VARIANTS[i], bytes: await get('/fonts/' + dir + '/' + stem + '-' + s + '.ttf') });
    env.pushFamily(family, { source: 'installed', embeddable: true, faces });   // the JS copies die with this scope
  }
  const doc = uo.open(await get(docUrl));
  const hyph = doc.hyphenationPatterns(env);
  for (const id of hyph) env.registerHyphenation(await (await fetch('/hyph/' + id + '.txt')).text());
  const layout = doc.layout(env, { generation: 1, hyphenationLanguages: ['pt-BR','en','es'] });
  const n = layout.pageCount, keep = [];
  let total = 0;
  for (let i = 0; i < n; i++) { const m = layout.encodePage(i); total += m.length; if (i < 5) keep.push(m); }
  window.__keep = { env, doc, layout, keep };
  return { pages: n, encodedBytes: total, wasmBytes: w.memory.buffer.byteLength, hyph, crossOriginIsolated };
};
</script>`;

const server = createServer(async (req, res) => {
  const path = decodeURIComponent(new URL(req.url, 'http://x').pathname);
  const head = { 'cross-origin-opener-policy': 'same-origin', 'cross-origin-embedder-policy': 'require-corp', 'cross-origin-resource-policy': 'same-origin' };
  if (path === '/rust.html') return res.writeHead(200, { ...head, 'content-type': 'text/html' }).end(RUST_PAGE);
  if (path === '/ts.html') {
    return res.writeHead(200, { ...head, 'content-type': 'text/html' }).end(`<!doctype html><meta charset="utf-8"><style>html,body{margin:0;height:100%}universal-document-viewer{display:block;height:100vh}</style><script type="module">import '/ts/index.js';</script><universal-document-viewer id="v" src="/docs/${docName}.docx" fonts="/ts/fonts" lang="en"></universal-document-viewer>`);
  }
  for (const [prefix, dir] of MAP) {
    if (!path.startsWith(prefix)) continue;
    const file = resolve(dir, '.' + path.slice(prefix.length - 1));
    if (file !== dir && !file.startsWith(dir + sep)) return res.writeHead(403).end();
    let body;
    try { body = await readFile(file); } catch { return res.writeHead(404).end(); }
    return res.writeHead(200, { ...head, 'content-type': MIME[extname(file)] ?? 'application/octet-stream' }).end(body);
  }
  res.writeHead(404).end();
});
await new Promise((ok) => server.listen(0, '127.0.0.1', ok));
const origin = `http://127.0.0.1:${server.address().port}`;
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

import { execFileSync } from 'node:child_process';
/** working set and private bytes of the renderer processes (CDP SystemInfo on the browser target + PowerShell/ps) */
async function rendererMemory(bcdp) {
  const infos = (await bcdp.send('SystemInfo.getProcessInfo')).processInfo.filter((p) => p.type === 'renderer');
  if (process.platform !== 'win32' || infos.length === 0) return null;
  const ids = infos.map((p) => p.id).join(',');
  const out = execFileSync('powershell', ['-NoProfile', '-NonInteractive', '-Command', `Get-Process -Id ${ids} | ForEach-Object { "$($_.Id) $($_.WorkingSet64) $($_.PrivateMemorySize64)" }`], { encoding: 'utf8', windowsHide: true });
  const rows = out.trim().split(String.fromCharCode(10)).map((l) => l.trim().split(' ').map(Number));
  return { renderers: rows.length, working_set_max: Math.max(...rows.map((r) => r[1])), private_max: Math.max(...rows.map((r) => r[2])) };
}

async function tabMemory(page, cdp, bcdp) {
  await sleep(1500);
  for (let i = 0; i < 2; i++) { await cdp.send('HeapProfiler.collectGarbage'); await sleep(100); }
  const m = Object.fromEntries((await cdp.send('Performance.getMetrics')).metrics.map((x) => [x.name, x.value]));
  let uas = null;
  try { uas = await page.evaluate(async () => (await performance.measureUserAgentSpecificMemory()).bytes); } catch (e) { uas = `n/a: ${String(e.message).slice(0, 80)}`; }
  return { js_heap_used: m.JSHeapUsedSize, js_heap_total: m.JSHeapTotalSize, uas_bytes: uas, renderer: await rendererMemory(bcdp) };
}

async function scenario(kind, { phone = false, engine = chromium } = {}) {
  const browser = await engine.launch({ executablePath: engine === chromium ? CHROMIUM : WEBKIT, args: engine === chromium ? ['--enable-precise-memory-info', '--js-flags=--expose-gc'] : [] });
  const ctx = await browser.newContext({ viewport: { width: 1280, height: 900 } });
  const page = await ctx.newPage();
  page.on('response', (r) => { if (r.status() === 404) console.error('404', r.url().replace(origin, '')); });
  page.on('pageerror', (e) => console.error('pageerror', String(e).slice(0, 200)));
  const cdp = engine === chromium ? await ctx.newCDPSession(page) : null;
  const bcdp = engine === chromium ? await browser.newBrowserCDPSession() : null;
  if (cdp) { await cdp.send('Performance.enable'); await cdp.send('HeapProfiler.enable'); }
  const out = { kind, phone, browser: engine === chromium ? 'chromium' : 'webkit' };
  try {
    if (kind === 'rust') {
      await page.goto(`${origin}/rust.html`);
      if (cdp) out.before = await tabMemory(page, cdp, bcdp);
      const r = await page.evaluate(([d, w, p]) => window.__run(d, w, p), [`/docs/${docName}.docx`, '/wasm/uo_wasm_ship.wasm', phone]);
      Object.assign(out, r);
      if (cdp) out.after = await tabMemory(page, cdp, bcdp);
    } else {
      await page.goto(`${origin}/rust.html`);   // the blank page: the baseline of an empty tab
      if (cdp) out.before = await tabMemory(page, cdp, bcdp);
      await page.goto(`${origin}/ts.html`);
      await page.waitForFunction(() => { const v = document.getElementById('v'); return v && typeof v.getPageCount === 'function' && v.getPageCount() > 0; }, null, { timeout: 120000 });
      await sleep(1500);
      out.pages = await page.evaluate(() => document.getElementById('v').getPageCount());
      out.after_first = await tabMemory(page, cdp, bcdp);
      // scroll through every page (the viewer paints the visible ones), come back to the first
      for (let i = 0; i < out.pages; i += 1) { await page.evaluate((k) => document.getElementById('v').goToPage(k), i); await sleep(60); }
      await page.evaluate(() => document.getElementById('v').goToPage(0));
      await sleep(1000);
      out.after = await tabMemory(page, cdp, bcdp);
    }
  } catch (e) {
    out.error = String(e.message ?? e).slice(0, 300);
  }
  await browser.close();
  return out;
}

const results = { date: new Date().toISOString(), doc: docName, scenarios: [] };
const todo = [['ts', {}], ['rust', {}], ['rust', { phone: true }]];
for (const [kind, o] of todo) { console.error(`== ${kind} ${o.phone ? 'phone' : ''}`); results.scenarios.push(await scenario(kind, o)); }
if (args.includes('--webkit')) {
  console.error('== rust webkit');
  results.scenarios.push(await scenario('rust', { engine: webkit }));
}
server.close();
const outFile = opt('--out', join(here, 'results', `browser-${new Date().toISOString().slice(0, 10)}.json`));
writeFileSync(outFile, JSON.stringify(results, null, 1));
console.log(JSON.stringify(results));
