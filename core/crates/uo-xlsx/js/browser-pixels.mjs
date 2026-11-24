// The Sheet viewer in a real browser (frontier C4b, record core/docs/modules/uo-xlsx.md "C4b — as built"): headless Chromium (and
// WebKit with `--browser webkit`) through Playwright, one page that holds the FROZEN TS `<universal-sheet>` (a browser bundle of the
// ts-ref-092 sources, as the captures' harness) and the Rust path of `<universal-sheet engine="rust">` (`uo-sheet.wasm` through
// `uo-wasm/js/sheet-engine.ts`, the painter `paint-slides.ts`), both painting the same viewport of the same workbook with the same
// fonts (the TS element's font resolver gives both stacks). The canvases are compared pixel by pixel. Headless only: no window,
// no screen capture.
//
//   node core/crates/uo-xlsx/js/browser-pixels.mjs [--browser chromium|webkit] [--wasm <uo-sheet.wasm in the repo>] [--files a.xlsx,b.xlsx]
//        [--out <dir for PNG pairs of the differing frames>]
//   UO_PLAYWRIGHT=<playwright package dir> (default: the main checkout's sheet/node_modules/playwright)
//
// The TS element opens without its open-recalc (`shouldRecalcOnOpen = () => false`): `uo-sheet.wasm` shows the file's values (the
// calc module is `uo-sheet-calc.wasm`).
import { readFile, stat, mkdir, writeFile } from 'node:fs/promises';
import { existsSync } from 'node:fs';
import { createServer } from 'node:http';
import { execFileSync } from 'node:child_process';
import { stripTypeScriptTypes } from 'node:module';
import { join, resolve, extname, relative, sep } from 'node:path';
import { pathToFileURL } from 'node:url';
import * as C from './lib/common.mjs';

const a = C.args();
const browserName = a.opt('--browser', 'chromium');
const repo = C.repoDir;
const root = C.tsRoot(a);
const wasmPath = resolve(a.opt('--wasm', join(C.coreDir, 'target', 'c4b-uo-sheet.wasm')));
const outDir = a.opt('--out');
const files = (a.opt('--files') ?? '').split(',').filter(Boolean);
const pwDir = process.env.UO_PLAYWRIGHT ?? 'D:/Projects/OfficeNext/sheet/node_modules/playwright';
const playwright = await import(pathToFileURL(join(pwDir, 'index.mjs')).href);

// the frozen TS element as a BROWSER bundle (the harness's entry, platform browser)
const bundleDir = join(C.coreDir, 'target', 'sheet-cap', 'browser');
await mkdir(bundleDir, { recursive: true });
const bundle = join(bundleDir, 'sheet-browser.mjs');
if (!existsSync(bundle)) {
  const sp = join(root, 'sheet', 'packages');
  const alias = {
    '@universal-office/sheet-core': join(sp, 'sheet-core/src/index.ts'),
    '@universal-office/sheet-calc': join(sp, 'calc/src/index.ts'),
    '@universal-office/fonts': join(root, 'shared/fonts/src/index.ts'),
    '@universal-office/ui': join(root, 'shared/ui/src/index.ts'),
    '@universal-office/charts': join(root, 'shared/charts/src/index.ts'),
    '@universal-office/drawingml': join(root, 'shared/drawingml/src/index.ts'),
    '@universal-office/slides-core': join(root, 'slides/packages/slides-core/src/index.ts'),
  };
  const entry = join(bundleDir, 'entry.ts');
  await writeFile(entry, [
    `export * from ${JSON.stringify(join(sp, 'sheet/src/universal-sheet.ts').replace(/\\/g, '/'))};`,
    `export { variantOf } from ${JSON.stringify(join(root, 'shared/fonts/src/types.ts').replace(/\\/g, '/'))};`,
    '',
  ].join('\n'));
  const esbuild = join(root, 'sheet', 'node_modules', 'esbuild', 'bin', 'esbuild');
  execFileSync(process.execPath, [esbuild, entry, '--bundle', '--format=esm', '--platform=browser', '--log-level=warning', '--legal-comments=none',
    ...Object.entries(alias).map(([k, v]) => `--alias:${k}=${v}`), `--outfile=${bundle}`], { cwd: join(sp, 'sheet'), stdio: ['ignore', 'inherit', 'inherit'] });
}

// a read-only server of the repository (and of the frozen checkout under /ts/), `.ts` served with the types stripped
const types = { '.mjs': 'text/javascript', '.js': 'text/javascript', '.ts': 'text/javascript', '.wasm': 'application/wasm', '.html': 'text/html', '.xlsx': 'application/octet-stream' };
const server = createServer(async (req, res) => {
  try {
    const url = decodeURIComponent(new URL(req.url, 'http://x').pathname);
    if (url === '/') { res.writeHead(200, { 'content-type': 'text/html' }); res.end('<!doctype html><meta charset=utf-8><body></body>'); return; }
    const file = resolve(repo, '.' + url);
    if (relative(repo, file).split(sep)[0] === '..') { res.writeHead(403); res.end(); return; }
    let body = await readFile(file);
    const ext = extname(file);
    if (ext === '.ts') body = Buffer.from(stripTypeScriptTypes(body.toString('utf8')));
    res.writeHead(200, { 'content-type': types[ext] ?? 'application/octet-stream' });
    res.end(body);
  } catch { res.writeHead(404); res.end(); }
});
await new Promise((r) => server.listen(0, '127.0.0.1', r));
const base = `http://127.0.0.1:${server.address().port}`;
const rel = (p) => '/' + relative(repo, p).split(sep).join('/');

const samples = files.length ? files.map((f) => resolve(f)) : C.trackedSamples().filter((s) => !/giant/.test(s)).map((s) => join(repo, 'sheet', 'samples', s))
  .concat([0, 1, 2, 4, 7].map((i) => join(C.capturesDir, 'workbooks', 'dr1', `dr1-${String(i).padStart(4, '0')}.xlsx`)));

// UO_PX_EXE: an installed browser build when the package's own is not downloaded (headless only)
const browser = await playwright[browserName].launch({ headless: true, ...(process.env.UO_PX_EXE ? { executablePath: process.env.UO_PX_EXE } : {}) });
const page = await browser.newPage({ viewport: { width: 1000, height: 700 }, deviceScaleFactor: 1 });
page.on('pageerror', (e) => console.log('page error:', e.message));
await page.goto(base + '/');
await page.addScriptTag({ type: 'module', content: `
  import * as M from '${rel(bundle)}';
  import { loadSheetModule } from '/core/crates/uo-wasm/js/sheet-engine.ts';
  window.__M = M;
  window.__load = () => loadSheetModule({ wasm: '${rel(wasmPath)}' });
  window.__dbg = ${process.env.UO_PX_DEBUG ? 'true' : 'false'};
  window.__tsRules = ${process.env.UO_PX_OFFICE ? 'false' : 'true'};
  window.__ready = true;
` });
await page.waitForFunction(() => window.__ready === true, null, { timeout: 60000 });

const W = 900, H = 560;
const STATES = [
  { z: 1, sx: 0, sy: 0, sel: [1, 1, 1, 1, 1, 1] },
  { z: 1.25, sx: 37.5, sy: 21.25, sel: [3, 3, 2, 2, 4, 5] },
  { z: 0.75, sx: 120, sy: 60, sel: [2, 2, 2, 2, 2, 2] },
];
const results = [];
for (const f of samples) {
  const r = await page.evaluate(async ({ url, W, H, STATES }) => {
    const M = window.__M;
    const mod = await window.__load();
    const bytes = new Uint8Array(await (await fetch(url)).arrayBuffer());
    const el = new M.UniversalSheetElement();
    el.setAttribute('formula-locale', 'pt-BR');
    el.style.cssText = `display:block;width:${W}px;height:${H}px`;
    document.body.appendChild(el);
    el.shouldRecalcOnOpen = () => false;
    el.loadXlsx(bytes);
    await el.drawingsReady?.();
    await new Promise((r) => setTimeout(r, 50));
    const book = mod.open(bytes);
    const host = {
      fontStack: (family, b, i) => { const [n, alt] = family.split('|'); return el.fontResolver.cssStack(n, alt ? [alt, 'Arial'] : ['Arial'], M.variantOf(b, i)); },
      shapeFontStack: (family, b, i) => el.shapeFontStack(family, b, i),
      metricFamily: (name) => { const r = el.fontResolver.get(name); return r && r.source === 'default' ? 'arial' : name; },
      measureContext: () => (window.__mc ??= document.createElement('canvas').getContext('2d')),
      invalidate: () => {},
    };
    book.setAccent(el.accent());
    // the frozen TS rules of the drawings and charts (the comparison is with the TS; the product default is the Office-measured set)
    if (window.__tsRules) book.setTsRules(true);
    const out = [];
    const sheets = book.sheets();
    for (let si = 0; si < sheets.length; si++) {
      if (sheets[si].kind === 'none') continue;
      el.selectSheet(si);
      for (let k = 0; k < STATES.length; k++) {
        const st = STATES[k];
        el.zoom = st.z; el.scrollX = st.sx; el.scrollY = st.sy;
        el.sel = { col: st.sel[0], row: st.sel[1] };
        el.range = { c1: st.sel[2], r1: st.sel[3], c2: st.sel[4], r2: st.sel[5] };
        el.recalcGrid();
        el.paintScrollbars = () => {};
        const fontsOf = (cx) => { const seen = new Set(); const d = Object.getOwnPropertyDescriptor(CanvasRenderingContext2D.prototype, 'font'); Object.defineProperty(cx, 'font', { configurable: true, get() { return d.get.call(this); }, set(v) { seen.add(v); d.set.call(this, v); } }); return seen; };
        const tsFonts = window.__dbg ? fontsOf(el.canvas.getContext('2d')) : null;
        el.paint();
        const tc = el.canvas;
        const ts = tc.getContext('2d').getImageData(0, 0, tc.width, tc.height);
        const rc = document.createElement('canvas');
        rc.width = tc.width; rc.height = tc.height;
        const ctx = rc.getContext('2d');
        const rustFonts = window.__dbg ? fontsOf(ctx) : null;
        const q = { sheet: si, width: tc.clientWidth || W, height: tc.clientHeight || H, dpr: window.devicePixelRatio || 1, scrollX: st.sx, scrollY: st.sy, zoom: st.z, selection: st.sel };
        let err = null;
        try {
          book.paint(ctx, q, host);
          // pictures decode asynchronously: paint again once they are in
          await new Promise((r) => setTimeout(r, 80));
          ctx.setTransform(1, 0, 0, 1, 0, 0);
          ctx.clearRect(0, 0, rc.width, rc.height);
          book.paint(ctx, q, host);
        } catch (e) { err = String(e?.message ?? e); }
        const rs = ctx.getImageData(0, 0, rc.width, rc.height);
        let diff = 0, maxd = 0;
        const box = [Infinity, Infinity, -1, -1];
        for (let p = 0; p < ts.data.length; p += 4) {
          const d = Math.max(Math.abs(ts.data[p] - rs.data[p]), Math.abs(ts.data[p + 1] - rs.data[p + 1]), Math.abs(ts.data[p + 2] - rs.data[p + 2]), Math.abs(ts.data[p + 3] - rs.data[p + 3]));
          if (d > 0) {
            diff++;
            const x = (p / 4) % tc.width, y = Math.floor(p / 4 / tc.width);
            box[0] = Math.min(box[0], x); box[1] = Math.min(box[1], y); box[2] = Math.max(box[2], x); box[3] = Math.max(box[3], y);
          }
          if (d > maxd) maxd = d;
        }
        out.push({ si, k, w: tc.width, h: tc.height, diff, maxd, err, box, ts: diff ? tc.toDataURL() : null, rust: diff ? rc.toDataURL() : null,
          fonts: window.__dbg ? { ts: [...tsFonts].slice(0, 6), rust: [...rustFonts].slice(0, 6) } : undefined });
      }
    }
    book.free();
    el.remove();
    return out;
  }, { url: rel(f), W, H, STATES });
  for (const x of r) {
    results.push({ file: relative(repo, f), ...x });
    if (x.diff && outDir) {
      await mkdir(outDir, { recursive: true });
      const stem = join(outDir, `${relative(repo, f).replace(/[\\/]/g, '_')}-s${x.si}-k${x.k}`);
      await writeFile(`${stem}-ts.png`, Buffer.from(x.ts.split(',')[1], 'base64'));
      await writeFile(`${stem}-rust.png`, Buffer.from(x.rust.split(',')[1], 'base64'));
    }
  }
}
await browser.close();
server.close();
let equal = 0;
for (const x of results) {
  if (x.fonts) console.log(JSON.stringify(x.fonts));
  if (x.diff === 0 && !x.err) equal++;
  else console.log(`${x.file} sheet ${x.si} state ${x.k}: ${x.diff} px differ (max channel delta ${x.maxd}, box ${x.box?.join(',')})${x.err ? ` error ${x.err}` : ''}`);
}
console.log(`${browserName}: ${results.length} frames, ${equal} pixel-identical, ${results.length - equal} differ`);
