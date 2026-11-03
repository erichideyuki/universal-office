// Runs the wasm tests of uo-wasm (the `*.test.mjs` next to this folder) inside real browser
// engines with Playwright: WebKit (the engine of Safari) by default, Chromium and Firefox on
// request. The tests stay single-source: `node:test`, `node:assert/strict`, `node:fs`,
// `node:path` and `node:url` are mapped to the small shims in ./shims, and the repository is
// served read-only by a local HTTP server on 127.0.0.1 (a synchronous XHR, which `readFileSync`
// needs, bypasses Playwright's request interception). Tests that need `--expose-gc` are skipped (no
// browser exposes `gc()`).
//
//   cargo build -p uo-wasm --features count,opc-memtest --target wasm32-unknown-unknown --profile wasm
//   node crates/uo-wasm/tests/browser/run.mjs [--browser webkit|chromium|firefox ...] [test files]
//
// Playwright is resolved from UO_PLAYWRIGHT (a path to a `playwright` package folder) or as the
// bare `playwright` package; its browsers must be installed (`npx playwright install webkit`).
// UO_OPC_SOAK, UO_JSNUM_SOAK, UO_JSNUM_FULL, UO_FONTS_SOAK and UO_WASM_PATH (a .wasm inside the repository, served instead of the default) are forwarded to the page.
import { readFile } from 'node:fs/promises';
import { createServer } from 'node:http';
import { resolve, relative, sep, dirname, basename, extname } from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';

const here = dirname(fileURLToPath(import.meta.url));
const repo = resolve(here, '../../../../..');

const args = process.argv.slice(2);
const browsers = [];
const files = [];
for (let i = 0; i < args.length; i++) {
  if (args[i] === '--browser') browsers.push(args[++i]);
  else files.push(args[i]);
}
if (browsers.length === 0) browsers.push('webkit');
if (files.length === 0) files.push(resolve(here, '../boundary.test.mjs'), resolve(here, '../opc_memory.test.mjs'));

const playwright = process.env.UO_PLAYWRIGHT
  ? await import(pathToFileURL(resolve(process.env.UO_PLAYWRIGHT, 'index.mjs')).href)
  : await import('playwright');

const types = { '.mjs': 'text/javascript', '.js': 'text/javascript', '.wasm': 'application/wasm', '.html': 'text/html' };
let current = '';
const server = createServer(async (req, res) => {
  const path = decodeURIComponent(new URL(req.url, 'http://localhost').pathname);
  if (req.method !== 'GET') return res.writeHead(405).end();
  if (path === '/__runner.html') return res.writeHead(200, { 'content-type': 'text/html' }).end(page(current));
  const target = resolve(repo, '.' + path);
  if (target !== repo && !target.startsWith(repo + sep)) return res.writeHead(403).end();
  try {
    const body = await readFile(target);
    res.writeHead(200, { 'content-type': types[extname(target)] ?? 'application/octet-stream' }).end(body);
  } catch {
    res.writeHead(404).end();
  }
});
await new Promise((ok) => server.listen(0, '127.0.0.1', ok));
const origin = `http://127.0.0.1:${server.address().port}`;

const urlOf = (file) => `${origin}/${relative(repo, resolve(file)).split(sep).join('/')}`;
const shim = (name) => `${urlOf(resolve(here, 'shims', name))}`;

function page(testUrl) {
  const imports = {
    'node:test': shim('test.mjs'),
    'node:assert/strict': shim('assert.mjs'),
    'node:fs': shim('fs.mjs'),
    'node:path': shim('path.mjs'),
    'node:url': shim('url.mjs'),
  };
  const wasm = process.env.UO_WASM_PATH && '/' + relative(repo, resolve(process.env.UO_WASM_PATH)).split(sep).join('/');
  const env = { UO_OPC_SOAK: process.env.UO_OPC_SOAK, UO_JSNUM_SOAK: process.env.UO_JSNUM_SOAK, UO_JSNUM_FULL: process.env.UO_JSNUM_FULL, UO_FONTS_SOAK: process.env.UO_FONTS_SOAK, UO_WASM_PATH: wasm };
  return `<!doctype html><meta charset="utf-8">
<script type="importmap">${JSON.stringify({ imports })}</script>
<script type="module">
globalThis.global = globalThis;
globalThis.process = { env: ${JSON.stringify(env)} };
try {
  const { run } = await import('node:test');
  await import(${JSON.stringify(testUrl)});
  window.__results = await run();
} catch (e) {
  window.__results = [{ name: 'load', status: 'fail', error: String(e?.stack ?? e) }];
}
</script>`;
}

let failed = 0;
for (const name of browsers) {
  const browser = await playwright[name].launch();
  for (const file of files) {
    const context = await browser.newContext();
    const tab = await context.newPage();
    tab.on('console', (m) => console.log(`  ${m.text()}`));
    tab.on('pageerror', (e) => console.log(`  page error: ${e.message}`));
    current = urlOf(file);
    console.log(`${name} ${browser.version()} — ${basename(file)}`);
    await tab.goto(`${origin}/__runner.html`);
    await tab.waitForFunction(() => window.__results, null, { timeout: 0, polling: 500 });
    for (const r of await tab.evaluate(() => window.__results)) {
      if (r.status === 'fail') failed++;
      const detail = r.status === 'pass' ? `(${r.ms} ms)` : r.status === 'skip' ? `# ${r.reason}` : `\n${r.error}`;
      console.log(`  ${r.status === 'pass' ? 'ok' : r.status === 'skip' ? 'skip' : 'FAIL'} ${r.name} ${detail}`);
    }
    await context.close();
  }
  await browser.close();
}
server.close();
process.exit(failed ? 1 : 0);
