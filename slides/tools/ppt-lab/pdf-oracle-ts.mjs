// The TS side of the PowerPoint-PDF oracle (pdf-oracle.mjs ts): the frozen TS viewer (<universal-slides-viewer>, engine ts, of the
// 0.9.2 reference checkout) paints every slide of decks.tsv with renderSlideToCanvas at 96 dpi (slide width in pt x 4/3 px) in
// headless Chromium; the PNGs go to $UO_PPT_ORACLE_OUT/ts/<id>/<n>.png (n from 1). The browser draws the installed fonts by name
// (the TS Slides font policy T2) and the repository's open fonts through `fonts="/fonts"`, so the TS canvas uses the same files as
// PowerPoint and the Rust `system` PDF.
//
//   [UO_TS_ROOT=<frozen checkout, default .claude/worktrees/ts-ref-092>] [UO_PPT_ORACLE_ONLY=id,id] node pdf-oracle-ts.mjs
import { createRequire } from 'node:module';
import { createServer } from 'node:http';
import { existsSync, mkdirSync, readFileSync, readdirSync, writeFileSync } from 'node:fs';
import { basename, dirname, extname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

const here = dirname(fileURLToPath(import.meta.url));
const repo = resolve(here, '../../..');
const root = resolve(process.env.UO_TS_ROOT ?? join(repo, '..', 'ts-ref-092'));
const out = resolve(process.env.UO_PPT_ORACLE_OUT ?? join(repo, 'core/target/w4-oracle/ppt-oracle'));
const build = join(out, 'ts-bundle');
mkdirSync(build, { recursive: true });
const fwd = (p) => p.replace(/\\/g, '/');

const rows = readFileSync(join(repo, 'core/crates/uo-pptx/tests/oracle_ppt/decks.tsv'), 'utf8').split('\n').filter((l) => l && !l.startsWith('#')).map((l) => l.split('\t'));
const only = process.env.UO_PPT_ORACLE_ONLY ? new Set(process.env.UO_PPT_ORACLE_ONLY.split(',')) : null;
const decks = rows.filter(([id]) => !only || only.has(id)).map(([id, path]) => ({ id, file: join(repo, path) }));

// the bundle of the frozen viewer (the alias table of tools/build-pkg.mjs, as slides_switch.mjs)
const R = (p) => fwd(join(root, p));
writeFileSync(join(build, 'entry.mjs'), `export { UniversalSlidesViewerElement } from '${R('slides/packages/viewer/src/index.ts')}';\n`);
const INTERNAL = {
  '@universal-office/fonts': 'shared/fonts/src/index.ts',
  '@universal-office/charts': 'shared/charts/src/index.ts',
  '@universal-office/drawingml': 'shared/drawingml/src/index.ts',
  '@universal-office/ui': 'shared/ui/src/index.ts',
  '@universal-office/slides-core': 'slides/packages/slides-core/src/index.ts',
  '@universal-office/converter': 'converter/packages/converter/src/index.ts',
  '@universal-office/docs-processing': 'converter/packages/docs-processing/src/index.ts',
  '@universal-office/sheets-processing': 'converter/packages/sheets-processing/src/index.ts',
  '@universal-office/slides-processing': 'converter/packages/slides-processing/src/index.ts',
};
const alias = Object.fromEntries(Object.entries(INTERNAL).map(([n, f]) => [n, join(root, f)]));
const esbuildPath = join(root, 'write/node_modules/esbuild/lib/main.js');
const esbuild = createRequire(join(root, 'write/package.json'))(existsSync(esbuildPath) ? esbuildPath : 'esbuild');
const bundleOut = join(build, 'out');
await esbuild.build({
  entryPoints: { entry: join(build, 'entry.mjs') }, outdir: bundleOut, bundle: true, format: 'esm', platform: 'browser', target: 'es2022',
  splitting: true, chunkNames: 'chunks/[name]-[hash]', minify: true, legalComments: 'none', alias, absWorkingDir: root,
  external: ['pdfjs-dist', 'pdfjs-dist/*'], logLevel: 'warning',
  nodePaths: [join(root, 'write/node_modules'), join(root, 'slides/node_modules'), join(root, 'sheet/node_modules')],
});

const FONT_DIRS = [join(repo, 'shared/fonts/files'), join(repo, 'site/assets/fonts')];
const byName = new Map(decks.map((d, i) => [`k${i}${extname(d.file)}`, d]));
const MIME = { '.js': 'text/javascript', '.ttf': 'font/ttf', '.json': 'application/json' };
const server = createServer((req, res) => {
  const url = decodeURIComponent(req.url.split('?')[0]);
  const send = (file) => res.writeHead(200, { 'content-type': MIME[extname(file)] ?? 'application/octet-stream' }).end(readFileSync(file));
  if (url === '/') return res.writeHead(200, { 'content-type': 'text/html' }).end('<!doctype html><meta charset="utf-8"><body style="margin:0"><script type="module">import * as e from "/out/entry.js"; window.__e = e; window.__ready = true;</script></body>');
  if (url.startsWith('/out/')) { const f = join(bundleOut, url.slice(5)); return f.startsWith(bundleOut) && existsSync(f) ? send(f) : res.writeHead(404).end(); }
  if (url.startsWith('/fonts/')) {
    const name = basename(url);
    if (/^[\w.-]+\.(ttf|json)$/.test(name)) for (const d of FONT_DIRS) if (existsSync(join(d, name))) return send(join(d, name));
    return res.writeHead(404).end();
  }
  if (url.startsWith('/decks/')) { const d = byName.get(basename(url)); return d ? send(d.file) : res.writeHead(404).end(); }
  res.writeHead(404).end();
});
await new Promise((r) => server.listen(0, '127.0.0.1', r));

const { chromium } = createRequire(join(root, 'slides/package.json'))('playwright');
const shell = (() => {
  const base = join(process.env.LOCALAPPDATA ?? '', 'ms-playwright');
  if (!existsSync(base)) return undefined;
  const dirs = readdirSync(base).filter((d) => d.startsWith('chromium_headless_shell-')).sort();
  const exe = dirs.length && join(base, dirs[dirs.length - 1], 'chrome-headless-shell-win64', 'chrome-headless-shell.exe');
  return exe && existsSync(exe) ? exe : undefined;
})();
const browser = await chromium.launch({ headless: true, executablePath: process.env.UO_CHROMIUM ?? shell });
const context = await browser.newContext({ timezoneId: 'UTC', deviceScaleFactor: 1, viewport: { width: 1400, height: 900 } });
const page = await context.newPage();
page.on('pageerror', (e) => console.log('PAGE ERROR', e.message));
await page.goto(`http://127.0.0.1:${server.address().port}/`);
await page.waitForFunction(() => window.__ready === true, null, { timeout: 60000 });

let slides = 0;
for (const [name, d] of byName) {
  const dir = join(out, 'ts', d.id);
  mkdirSync(dir, { recursive: true });
  const r = await page.evaluate(async (name) => {
    const sleep = (ms) => new Promise((res) => setTimeout(res, ms));
    const el = new window.__e.UniversalSlidesViewerElement();
    el.setAttribute('fonts', '/fonts');
    el.setAttribute('nocontrols', '');
    el.style.cssText = 'display:block;width:1280px;height:720px;position:absolute;left:0;top:0';
    document.body.appendChild(el);
    try {
      el.loadPptx(new Uint8Array(await (await fetch(`/decks/${name}`)).arrayBuffer()));
      await el.fontsReady();
      const t0 = performance.now();
      while (performance.now() - t0 < 6000 && [...el.imageCache.values()].some((e) => !e.ready)) await sleep(20);
      await sleep(60);
      await el.fontsReady();
      const n = el.getSlideCount();
      const size = el.pres ? { width: el.pres.slideW, height: el.pres.slideH } : null;
      const pngs = [];
      for (let i = 0; i < n; i++) {
        const cv = document.createElement('canvas');
        // width in px at 96 dpi: slide width in EMU / 9525
        const wpx = size ? Math.round(size.width / 9525) : 1280;
        el.renderSlideToCanvas(i, cv, wpx);
        await sleep(0);
        el.renderSlideToCanvas(i, cv, wpx);
        pngs.push(cv.toDataURL('image/png'));
      }
      return { n, size, pngs };
    } catch (e) {
      return { error: String(e?.message ?? e) };
    } finally {
      el.remove();
    }
  }, name);
  if (r.error) { console.log(`${d.id}: ERROR ${r.error}`); writeFileSync(join(dir, 'error.txt'), r.error); continue; }
  r.pngs.forEach((u, i) => writeFileSync(join(dir, `${i + 1}.png`), Buffer.from(u.slice(u.indexOf(',') + 1), 'base64')));
  writeFileSync(join(dir, 'info.json'), JSON.stringify({ slides: r.n, size: r.size }));
  slides += r.n;
}
console.log(`ts: ${byName.size} decks, ${slides} slides -> ${join(out, 'ts')}`);
await browser.close();
server.close();
