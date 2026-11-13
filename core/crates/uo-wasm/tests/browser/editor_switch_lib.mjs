// The page of the editor switch harness (editor_switch.mjs; record core/docs/modules/uo-bindings.md "Milestone 2: the editor
// switch"): the editor's `src/index.ts` bundled by the frozen esbuild with the settings of tools/build-pkg.mjs (splitting,
// `chunks/[name]-[hash]`, the alias table, `@universal-office/write-engine` and `@universal-office/write-session`), served by a
// node:http server with `chunks/uo-write-editor.wasm` (the default wasm URL of the editor's engine, no `configureEngine`), the
// open fonts and the fixtures; a headless Chromium (UO_CHROMIUM or the installed chromium-headless-shell) with a fixed clock, a
// seeded `Math.random` (`window.__seed(n)`) and reduced motion (instant scrolls).
// Declared: the bundle, server and browser boilerplate is copy-adapted from viewer_switch.mjs (not shared: that file is not
// this frontier's).
import { createRequire } from 'node:module';
import { createServer } from 'node:http';
import { mkdirSync, readFileSync, writeFileSync, existsSync, readdirSync, copyFileSync, rmSync } from 'node:fs';
import { join, dirname, resolve, extname, basename } from 'node:path';
import { fileURLToPath } from 'node:url';
import { fixtureDocs, relDoc, NOW } from '../support/paint-common.mjs';

const here = dirname(fileURLToPath(import.meta.url));
export const core = resolve(here, '../../../..');
export const repo = resolve(core, '..');
export const fwd = (p) => p.replace(/\\/g, '/');
export { NOW, relDoc };

/** The esbuild of the frozen root (the worktree has no node_modules). */
function esbuildOf(root) {
  const p = join(root, 'write/node_modules/esbuild/lib/main.js');
  return createRequire(join(root, 'write/package.json'))(existsSync(p) ? p : 'esbuild');
}

const INTERNAL = {
  '@universal-office/fonts': 'shared/fonts/src/index.ts',
  '@universal-office/charts': 'shared/charts/src/index.ts',
  '@universal-office/drawingml': 'shared/drawingml/src/index.ts',
  '@universal-office/ui': 'shared/ui/src/index.ts',
  '@universal-office/spell': 'shared/spell/src/index.ts',
  '@universal-office/spell-sources': 'shared/spell/src/sources.ts',
  '@universal-office/docx-core': 'write/packages/docx-core/src/index.ts',
  '@universal-office/docx-layout': 'write/packages/docx-layout/src/index.ts',
  '@universal-office/compare': 'write/packages/compare/src/index.ts',
  '@universal-office/collation': 'sheet/packages/calc/src/collation.ts',
  '@universal-office/write-engine': 'core/crates/uo-wasm/js/viewer.ts',
  '@universal-office/write-session': 'core/crates/uo-wasm/js/session.mjs',
};

/**
 * Bundles the editor into `<outDir>/out`, copies the wasm, starts the server and the browser.
 * `extraDocs`: more documents served under /docs/ by name.
 * `entryExtra` (wave 5, frontier R): module code appended to the entry (editor_switch_review.mjs: the spelling sources).
 */
export async function setup({ root, wasmPath, outDir, stride = 1, only, extraDocs = {}, entryExtra = '' }) {
  const out = join(outDir, 'out');
  rmSync(out, { recursive: true, force: true });
  mkdirSync(join(out, 'chunks'), { recursive: true });
  const W = (p) => fwd(join(repo, p));
  writeFileSync(join(outDir, 'entry.mjs'), `
import { UniversalWriteElement } from '${W('write/packages/editor/src/index.ts')}';
export { UniversalWriteElement };
${entryExtra}
`);
  const alias = Object.fromEntries(Object.entries(INTERNAL).map(([n, f]) => [n, join(repo, f)]));
  alias['@universal-office/converter'] = join(root, 'converter/packages/converter/dist/index.js');
  const esbuild = esbuildOf(root);
  const built = await esbuild.build({
    entryPoints: { entry: join(outDir, 'entry.mjs') },
    outdir: out, bundle: true, format: 'esm', platform: 'browser', target: 'es2022', splitting: true, chunkNames: 'chunks/[name]-[hash]',
    minify: true, legalComments: 'none', alias, absWorkingDir: repo, nodePaths: [join(root, 'write/node_modules')], logLevel: 'error', metafile: true,
  });
  copyFileSync(wasmPath, join(out, 'chunks', 'uo-write-editor.wasm'));

  // the static closure of the entry (phase 0)
  const outs = built.metafile.outputs;
  const closure = (o0) => { const seen = new Set(); const visit = (o) => { if (seen.has(o) || !outs[o]) return; seen.add(o); for (const i of outs[o].imports) if (i.kind === 'import-statement') visit(i.path); }; visit(o0); return seen; };
  const entryOut = Object.keys(outs).find((o) => /(^|\/)entry\.js$/.test(fwd(o)));
  const staticSet = closure(entryOut);
  const staticInputs = [...staticSet].flatMap((o) => Object.keys(outs[o].inputs)).map((f) => fwd(resolve(repo, f)));
  const engineChunks = Object.keys(outs).filter((o) => /chunks\/rust-engine-/.test(fwd(o)));
  const kb = (n) => (n / 1024).toFixed(0);
  const bundle = {
    staticOutputs: staticSet.size,
    staticKB: kb([...staticSet].reduce((n, o) => n + outs[o].bytes, 0)),
    leaked: staticInputs.filter((f) => /core\/crates\/uo-wasm\/js\/|write\/packages\/editor\/src\/rust-engine\.ts$|write\/packages\/viewer\/src\/rust-engine\.ts$/.test(f)).map((f) => f.replace(fwd(repo) + '/', '')),
    engineChunks: engineChunks.map((o) => basename(o)),
    engineKB: kb(engineChunks.reduce((n, o) => n + outs[o].bytes, 0)),
    wasmKB: kb(readFileSync(wasmPath).length),
  };

  const fixtures = fixtureDocs().filter((p, i) => i % stride === 0 && (!only || new RegExp(only).test(p)));
  const docsByName = new Map(fixtures.map((p, i) => [`d${i}.docx`, p]));
  for (const [n, p] of Object.entries(extraDocs)) docsByName.set(n, p);
  const FONT_DIRS = [join(repo, 'site/assets/fonts'), join(repo, 'shared/fonts/files')];
  const requests = [];
  const MIME = { '.js': 'text/javascript', '.wasm': 'application/wasm', '.ttf': 'font/ttf', '.json': 'application/json', '.docx': 'application/octet-stream' };
  const server = createServer((req, res) => {
    const url = decodeURIComponent(req.url.split('?')[0]);
    requests.push(url);
    const send = (file) => res.writeHead(200, { 'content-type': MIME[extname(file)] ?? 'application/octet-stream' }).end(readFileSync(file));
    if (url === '/') return res.writeHead(200, { 'content-type': 'text/html' }).end('<!doctype html><meta charset="utf-8"><body style="margin:0"><script type="module">import * as e from "/out/entry.js"; window.__e = e; window.__ready = true;</script></body>');
    if (url.startsWith('/out/')) { const f = join(out, url.slice(5)); return f.startsWith(out) && existsSync(f) ? send(f) : res.writeHead(404).end(); }
    if (url.startsWith('/fonts/')) {
      const name = basename(url);
      if (/^[\w.-]+\.(ttf|json)$/.test(name)) for (const d of FONT_DIRS) if (existsSync(join(d, name))) return send(join(d, name));
      return res.writeHead(404).end();
    }
    if (url.startsWith('/docs/')) { const f = docsByName.get(basename(url)); return f ? send(f) : res.writeHead(404).end(); }
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
  const browser = await chromium.launch({ headless: true, executablePath: process.env.UO_CHROMIUM ?? installed, args: (process.env.UO_ES_CHROMIUM_ARGS ?? '').split(' ').filter(Boolean) });
  const context = await browser.newContext({ timezoneId: 'UTC', deviceScaleFactor: 1, viewport: { width: 1280, height: 900 }, reducedMotion: 'reduce' });
  await context.addInitScript(() => {
    let s = 1;
    Math.random = () => { s ^= s << 13; s >>>= 0; s ^= s >>> 17; s ^= s << 5; s >>>= 0; return s / 4294967296; };
    window.__seed = (n) => { s = n >>> 0 || 1; };
  });
  const errors = [];
  const pageRequests = [];
  let page = null;
  let cdp = null;
  async function openPage() {
    if (page) await page.close();
    page = await context.newPage();
    // wave 7c (S): a CDP session per page for `gc()`
    cdp = await context.newCDPSession(page);
    await page.clock.setFixedTime(NOW);
    page.on('crash', () => { console.log('PAGE CRASHED'); process.exit(2); });
    page.on('pageerror', (e) => { errors.push(e.message); console.log('PAGE ERROR', e.message); });
    page.on('console', (m) => { if (m.type() === 'error' && !/Failed to load resource/.test(m.text())) { errors.push(m.text()); console.log('CONSOLE', m.text()); } });
    page.on('request', (r) => pageRequests.push(r.url()));
    await page.goto(`http://127.0.0.1:${server.address().port}/`);
    await page.waitForFunction(() => window.__ready === true, null, { timeout: 60000 });
    return page;
  }
  /**
   * wave 7c (S): a removed `engine="rust"` element keeps its document until it is collected (as the TS element keeps its
   * model), so the memory checks collect garbage first: 3 × `HeapProfiler.collectGarbage` with a 30 ms task after each
   * (the FinalizationRegistry callbacks run in a later task), up to 10 rounds until `engineStats()` shows no session and
   * no shell engine. Answers the last `engineStats()` (null without the engine).
   */
  async function gc() {
    const stats = () => page.evaluate(() => customElements.get('universal-write')?.engineStats?.() ?? null);
    let s = null;
    for (let k = 0; k < 10; k++) {
      for (let j = 0; j < 3; j++) {
        await cdp.send('HeapProfiler.collectGarbage');
        await page.evaluate(() => new Promise((r) => setTimeout(r, 30)));
      }
      s = await stats();
      if (!s || s.fake || (s.sessions === 0 && s.shellEngines === 0)) return s;
    }
    return s;
  }
  return {
    bundle, fixtures, docsByName, requests, pageRequests, errors, openPage, gc,
    get page() { return page; },
    label: (n) => (fixtures.includes(docsByName.get(n)) ? relDoc(docsByName.get(n)) : n),
    close: async () => { await browser.close(); server.close(); },
  };
}
