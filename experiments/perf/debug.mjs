// Abre uma página no Chrome sem tela, guarda o console e um print (diagnóstico do harness).
//   node debug.mjs <url> <arquivo.png> [espera ms]
import { join, resolve, dirname } from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';
import { createServer } from 'node:http';
import { readFileSync, existsSync, statSync } from 'node:fs';

const here = dirname(fileURLToPath(import.meta.url));
// UO_TS_ROOT: checkout whose TS engine (built dist) is measured — main, the frozen baseline
const root = process.env.UO_TS_ROOT ? resolve(process.env.UO_TS_ROOT) : resolve(here, '../..');
const { chromium } = await import(pathToFileURL(join(root, 'sheet/node_modules/playwright-core/index.mjs')).href);
let url = process.argv[2];
if (url.startsWith('site:')) {
  const srv = createServer((req, res) => {
    const f = join(root, 'site', decodeURIComponent(new URL(req.url, 'http://x').pathname));
    if (!existsSync(f) || statSync(f).isDirectory()) { res.writeHead(404); res.end(); return; }
    const t = f.endsWith('.js') ? 'text/javascript' : f.endsWith('.html') ? 'text/html' : f.endsWith('.css') ? 'text/css' : f.endsWith('.wasm') ? 'application/wasm' : 'application/octet-stream';
    res.writeHead(200, { 'content-type': t }); res.end(readFileSync(f));
  });
  await new Promise((r) => srv.listen(0, '127.0.0.1', r));
  url = `http://127.0.0.1:${srv.address().port}${url.slice(5)}`;
}
const browser = await chromium.launch({ executablePath: 'C:/Program Files/Google/Chrome/Application/chrome.exe', headless: true });
const page = await browser.newPage({ viewport: { width: 1366, height: 768 } });
page.on('console', (m) => console.log('console', m.type(), m.text().slice(0, 300)));
page.on('pageerror', (e) => console.log('pageerror', e.message.slice(0, 300)));
page.on('requestfailed', (r) => console.log('failed', r.url().slice(0, 120), r.failure()?.errorText));
await page.goto(url, { waitUntil: 'load' });
await page.waitForTimeout(Number(process.argv[4] ?? 6000));
await page.screenshot({ path: join(here, 'out', process.argv[3]) });
console.log(await page.evaluate(() => {
  const ed = document.querySelector('universal-write');
  return { title: document.title, hasEd: !!ed, shadow: !!ed?.shadowRoot, canvases: ed?.shadowRoot?.querySelectorAll('canvas').length ?? null };
}));
await browser.close();
process.exit(0);
