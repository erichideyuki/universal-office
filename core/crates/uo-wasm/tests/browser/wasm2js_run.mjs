// Runs the wasm2js checks of uo-wasm in Chromium and WebKit with Playwright: the same page script
// (wasm2js_page.mjs) under three Content-Security-Policy headers, and the byte comparison and the timings.
//
//   node crates/uo-wasm/tests/browser/wasm2js_run.mjs --wasm <.wasm> --js <wasm2js .js> [--browser chromium|webkit ...]
//        [--mode csp|correct|speed ...] [--runs 5]
//
// --wasm/--js must be inside the repository (served read-only by a local HTTP server). Playwright is resolved from
// UO_PLAYWRIGHT (a `playwright` package folder) or as the bare package, as browser/run.mjs does; UO_CHROMIUM is an
// optional chrome executable (when the installed build is not the one this Playwright version wants).
//   csp     the page is served with `Content-Security-Policy: script-src 'self'` (no 'wasm-unsafe-eval'): the wasm
//           engine must be refused, the js engine must run and give the PDF the native build gives; a control page
//           with `script-src 'self' 'wasm-unsafe-eval'` shows the wasm engine loads when the policy allows it
//   correct 12 fixtures, every page message, annotation, navigation, link box, face and PDF compared byte for byte
//           between the two engines in the same page (no CSP)
//   speed   layout + all pages + PDF per fixture, median of --runs, one fresh page per engine
import { readFile } from 'node:fs/promises';
import { createServer } from 'node:http';
import { resolve, relative, sep, dirname, extname } from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';

const here = dirname(fileURLToPath(import.meta.url));
const repo = resolve(here, '../../../../..');
const args = process.argv.slice(2);
const multi = (name) => args.flatMap((a, i) => (a === name ? [args[i + 1]] : []));
const one = (name, d) => multi(name)[0] ?? d;
const browsers = multi('--browser'); if (!browsers.length) browsers.push('chromium', 'webkit');
const modes = multi('--mode'); if (!modes.length) modes.push('csp', 'correct', 'speed');
const runs = one('--runs', '5');
const rel = (p) => relative(repo, resolve(p)).split(sep).join('/');
const wasm = rel(one('--wasm')), js = rel(one('--js'));

const playwright = process.env.UO_PLAYWRIGHT ? await import(pathToFileURL(resolve(process.env.UO_PLAYWRIGHT, 'index.mjs')).href) : await import('playwright');
const types = { '.mjs': 'text/javascript', '.js': 'text/javascript', '.wasm': 'application/wasm', '.html': 'text/html' };
const CSP = { strict: "script-src 'self'", allowed: "script-src 'self' 'wasm-unsafe-eval'" };
const server = createServer(async (req, res) => {
  const url = new URL(req.url, 'http://localhost');
  const path = decodeURIComponent(url.pathname);
  if (path === '/__page.html') {
    const headers = { 'content-type': 'text/html', 'cache-control': 'no-store' };
    if (CSP[url.searchParams.get('csp')]) headers['content-security-policy'] = CSP[url.searchParams.get('csp')];
    return res.writeHead(200, headers).end(`<!doctype html><meta charset="utf-8"><script type="module" src="/${rel(resolve(here, 'wasm2js_page.mjs'))}"></script>`);
  }
  const target = resolve(repo, '.' + path);
  if (req.method !== 'GET' || (target !== repo && !target.startsWith(repo + sep))) return res.writeHead(403).end();
  let body; try { body = await readFile(target); } catch { return res.writeHead(404).end(); }
  res.writeHead(200, { 'content-type': types[extname(target)] ?? 'application/octet-stream' }).end(body);
});
await new Promise((ok) => server.listen(0, '127.0.0.1', ok));
const origin = `http://127.0.0.1:${server.address().port}`;

async function page(browser, mode, extra, csp) {
  const context = await browser.newContext();
  const tab = await context.newPage();
  const logs = [];
  tab.on('console', (m) => logs.push(m.text().slice(0, 300)));
  tab.on('pageerror', (e) => logs.push(`page error: ${e.message.slice(0, 300)}`));
  const query = new URLSearchParams({ mode, wasm, js, csp: csp ?? '', ...extra });
  await tab.goto(`${origin}/__page.html?${query}`);
  await tab.waitForFunction(() => window.__result, null, { timeout: 600000, polling: 500 });
  const result = await tab.evaluate(() => window.__result);
  await context.close();
  return { result, logs };
}

for (const name of browsers) {
  const browser = await playwright[name].launch(name === 'chromium' && process.env.UO_CHROMIUM ? { executablePath: process.env.UO_CHROMIUM } : {});
  console.log(`\n== ${name} ${browser.version()}`);
  for (const mode of modes) {
    if (mode === 'csp') {
      for (const csp of ['strict', 'allowed']) {
        const { result, logs } = await page(browser, 'csp', {}, csp);
        console.log(`csp [${CSP[csp]}]`, JSON.stringify(result, null, 1), logs.length ? `\nconsole: ${logs.join(' | ')}` : '');
      }
    } else if (mode === 'correct') {
      const { result, logs } = await page(browser, 'correct', {});
      console.log('correct', JSON.stringify(result.ok ? { rows: result.rows.map((r) => `${r.key} ${r.pages}p ${r.pdf}B diff=${r.diff} pinned=${r.pinned}`) } : result, null, 1), logs.length ? `\nconsole: ${logs.join(' | ')}` : '');
    } else if (mode === 'speed') {
      for (const engine of ['wasm', 'js']) {
        const { result } = await page(browser, 'speed', { engine, runs });
        console.log('speed', JSON.stringify(result));
      }
    }
  }
  await browser.close();
}
server.close();
