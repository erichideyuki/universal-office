// Chromium pixels of the painter parity (uo-render frontier V), only for the paths that READ PIXELS BACK or draw through the
// browser (picture layers, shape effects, text-effect layers, page fills — the Node recording context does not rasterise):
// the frozen canvas painter (canvas.js, bundled by the frozen esbuild) and the TS painter of decoded pages (js/paint.ts) paint
// the same generated pages (uo-pdf `genSpec`, the pages whose numbers are plain) on two OffscreenCanvases at two scales in a
// real Chromium, and `getImageData` is compared byte for byte. The D painter's page comes from the frozen LayoutResult through
// the TS adapter, the Rust view (wasm) and `encodePage` -> `decodePage`; its rasters and masks are the wasm exports.
//
//   UO_TS_ROOT=<frozen> UO_CHROMIUM=<chrome-headless-shell.exe> [UO_WASM_PATH=<.wasm>] [UO_V_PIXEL_SEEDS=60] [UO_PLAYWRIGHT=<playwright package>] node crates/uo-wasm/tests/browser/paint_pixels.mjs [--browser chromium|webkit]
//
// The bundle goes to `$CARGO_TARGET_DIR/v-pixels/` (a build output; no repo copy, no junction). Fonts are registered as FontFace
// from the same bytes on both sides; images are decoded once with createImageBitmap (an image the browser cannot decode — the
// generated fake JPEGs — paints nothing on both sides).
import { createRequire } from 'node:module';
import { createServer } from 'node:http';
import { mkdirSync, readFileSync, writeFileSync, existsSync, readdirSync } from 'node:fs';
import { join, dirname, resolve } from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';

const here = dirname(fileURLToPath(import.meta.url));
const core = resolve(here, '../../../..');
const repo = resolve(core, '..');
const root = process.env.UO_TS_ROOT;
if (!root) throw new Error('set UO_TS_ROOT to the frozen TS checkout');
const wasmPath = process.env.UO_WASM_PATH ?? join(core, 'target/wasm32-unknown-unknown/wasm/uo_wasm.wasm');
const outDir = join(process.env.CARGO_TARGET_DIR ?? join(core, 'target'), 'v-pixels');
mkdirSync(outDir, { recursive: true });
const SEEDS = Number(process.env.UO_V_PIXEL_SEEDS ?? 60);
const fwd = (p) => p.replace(/\\/g, '/');

// ── the page script, bundled ────────────────────────────────────────────────────────────────────
const D = fwd(join(root, 'write/packages/docx-layout/dist'));
const entry = join(outDir, 'entry.mjs');
writeFileSync(entry, `
import * as canvas from '${D}/canvas.js';
import { SfntFont } from '${D}/typography/font/sfnt.js';
import { imagePixelSize } from '${D}/images.js';
import { paintItems, paintPageBackground, PaintCaches } from '${fwd(core)}/crates/uo-wasm/js/paint.ts';
import { decodePage, newTables } from '${fwd(core)}/crates/uo-wasm/js/gen/render.ts';
import { adaptLayout, pushInto } from '${fwd(core)}/crates/uo-render/tools/lib/ts-adapter.mjs';
import { writeImports, bindWrite } from '${fwd(core)}/crates/uo-wasm/js/write.mjs';

const bytesOf = (b64) => Uint8Array.from(atob(b64), (c) => c.charCodeAt(0));
const key = (f) => 'F' + [...(f.key ?? '')].reduce((h, c) => (Math.imul(h, 31) + c.charCodeAt(0)) | 0, 7).toString(16).replace('-', 'n') + (f.bold ? 'b' : '') + (f.italic ? 'i' : '');

export async function run({ wasmB64, specs, fonts, scales }) {
  const { imports, host } = writeImports();
  const { instance } = await WebAssembly.instantiate(bytesOf(wasmB64), imports);
  const uo = bindWrite(instance.exports, host);
  const fontBytes = new Map(Object.entries(fonts).map(([k, v]) => [k, bytesOf(v)]));
  const registered = new Set();
  const out = [];
  const caches = new PaintCaches();
  const mime = (ext) => ({ png: 'image/png', jpeg: 'image/jpeg', jpg: 'image/jpeg', bmp: 'image/bmp', gif: 'image/gif' })[ext] ?? 'image/png';
  for (const { seed, spec } of specs) {
    // frozen objects (materialize of capture_painter.mjs)
    const faces = [];
    for (const f of spec.faces) {
      const bytes = fontBytes.get(f.file);
      const font = new SfntFont(bytes);
      if (f.adv) font.setAdvanceTable(f.adv);
      faces.push({ key: f.key, family: f.family, font, ...(f.bytes === false ? {} : { bytes }), bold: f.bold, italic: f.italic });
      const fam = key(faces[faces.length - 1]);
      if (!registered.has(fam)) { registered.add(fam); const ff = new FontFace(fam, bytes.slice().buffer); await ff.load(); document.fonts.add(ff); }
    }
    const images = spec.images.map((im) => ({ data: bytesOf(im.b64), ext: im.ext, widthTwips: 0, heightTwips: 0, ...(im.fx ? { effects: { ...im.fx } } : {}) }));
    const bitmaps = new Map();
    for (const img of images) { try { bitmaps.set(img, await createImageBitmap(new Blob([img.data], { type: mime(img.ext) }))); } catch { bitmaps.set(img, null); } }
    const fillOf = (f) => ({ ...f, ...(f.image !== undefined ? { image: images[f.image] } : {}) });
    const pages = spec.pages.map((p) => ({
      widthTw: p.widthTw, heightTw: p.heightTw, section: 0, number: 1, lines: [],
      ...(p.background !== undefined ? { background: p.background } : {}),
      ...(p.backgroundFill ? { backgroundFill: fillOf(p.backgroundFill) } : {}),
      items: p.items.map((it) => {
        if (it.t === 'glyphs') return { ...it, face: faces[it.face], text: '', glyphs: it.glyphs.map(([cp, x, gid]) => (gid === undefined ? { cp, x } : { cp, x, gid })) };
        if (it.t === 'image') return { ...it, image: images[it.image] };
        if (it.t === 'shape' && it.blip) return { ...it, blip: { ...it.blip, image: images[it.blip.image] } };
        return it;
      }),
    }));
    const result = { pages, warnings: [] };
    const a = adaptLayout(result, { imagePixelSize: (d) => { const n = imagePixelSize(d); return n ? { w: n.width, h: n.height } : undefined; } });
    const view = uo.render.newView(1);
    pushInto(view, a);
    const tables = newTables();
    const rasters = uo.render.rasters(view.paint);
    // the table id of an image is its first use on the page stream, not its index in the spec: look it up by the bytes
    const byData = new Map(images.map((img) => [img.data, bitmaps.get(img) ?? null]));
    const fenv = { fontOf: (f) => ({ family: '"' + key(f) + '"', weight: f.bold ? 700 : 400, italic: !!f.italic }), image: (img) => bitmaps.get(img) ?? null, color: (h) => '#' + h };
    const denv = { fontOf: (f) => ({ family: '"' + key(f) + '"', weight: f.bold ? 700 : 400, italic: !!f.italic }), image: (id) => byData.get(a.images[id]?.bytes) ?? null, color: (h) => '#' + h };
    for (let pi = 0; pi < pages.length; pi++) {
      const status = view.encodePage(pi).take();
      const dpage = decodePage(status.data, tables);
      const page = pages[pi];
      const features = {
        pic: page.items.some((it) => it.t === 'image' && (it.pic || it.image.effects)),
        shapeFx: page.items.some((it) => it.t === 'shape' && it.fx && it.style.geom !== 'line'),
        textFx: page.items.some((it) => it.t === 'glyphs' && it.fx),
        pageFill: !!page.backgroundFill,
        shapeRaster: page.items.some((it) => it.t === 'shape' && (it.pattern || it.blip || it.grad)),
        text: page.items.some((it) => it.t === 'glyphs'),
      };
      for (const s of scales) {
        const W = Math.max(1, Math.min(2400, Math.ceil(page.widthTw * s))), H = Math.max(1, Math.min(3200, Math.ceil(page.heightTw * s)));
        const draw = (fn) => { const c = typeof OffscreenCanvas !== 'undefined' ? new OffscreenCanvas(W, H) : Object.assign(document.createElement('canvas'), { width: W, height: H }); const x = c.getContext('2d', { willReadFrequently: true }); x.setTransform(s, 0, 0, s, 0, 0); fn(x); return x.getImageData(0, 0, W, H).data; };
        let A, B, err = null;
        try {
          A = draw((x) => { canvas.paintPageBackground(x, page, fenv, { devicePxTw: 1 / s, screen: false, paper: '#ffffff' }); canvas.paintItems(x, page.items, fenv); });
          B = draw((x) => { const p = { page: dpage, tables, index: pi, rasters, owner: view }; paintPageBackground(x, p, denv, { devicePxTw: 1 / s, screen: false, paper: '#ffffff' }, caches); paintItems(x, p, denv, {}, caches); });
        } catch (e) { err = String(e?.message ?? e).slice(0, 200); }
        if (err) { out.push({ seed, pi, s, features, err }); continue; }
        let diff = 0, first = -1, ink = 0;
        for (let i = 0; i < A.length; i += 4) {
          // ink: a pixel of the frozen render that is not the paper (so a render where both sides paint nothing is visible)
          if (A[i] !== 255 || A[i + 1] !== 255 || A[i + 2] !== 255 || A[i + 3] !== 255) ink++;
          if (A[i] !== B[i] || A[i + 1] !== B[i + 1] || A[i + 2] !== B[i + 2] || A[i + 3] !== B[i + 3]) { diff++; if (first < 0) first = i / 4; }
        }
        let items = null;
        if (diff) {
          // which items differ when each is painted alone (fresh canvases, the caches as they are)
          items = [];
          const one = (k) => {
            const dA = draw((x) => canvas.paintItems(x, page.items, fenv, { from: k, to: k + 1 }));
            const dB = draw((x) => paintItems(x, { page: dpage, tables, index: pi, rasters, owner: view }, denv, { from: k, to: k + 1 }, caches));
            let n = 0; for (let i = 0; i < dA.length; i++) if (dA[i] !== dB[i]) n++;
            return n;
          };
          for (let k = 0; k < page.items.length; k++) { let n = 0; try { n = one(k); } catch (e) { n = -1; } if (n) items.push({ k, t: page.items[k].t, n, pic: !!page.items[k].pic, fx: !!page.items[k].fx, item: JSON.stringify(page.items[k], (kk, v) => (kk === 'image' ? { ext: v.ext, effects: v.effects, len: v.data.length } : v)).slice(0, 900) }); }
        }
        out.push({ seed, pi, s, features, W, H, pixels: W * H, diff, first, ink, items });
      }
    }
    view.free();
  }
  return out;
}
`);

const esbuildPath = join(root, 'write/node_modules/esbuild/lib/main.js');
const esbuild = createRequire(join(root, 'write/package.json'))(existsSync(esbuildPath) ? esbuildPath : 'esbuild');
await esbuild.build({
  entryPoints: [entry], bundle: true, format: 'esm', platform: 'browser', target: 'es2022', outfile: join(outDir, 'bundle.mjs'),
  nodePaths: [join(root, 'write/node_modules'), join(root, 'node_modules')], logLevel: 'warning', loader: { '.ts': 'ts' },
});

// ── the corpus: pages of genSpec with plain numbers ─────────────────────────────────────────────
const cp = await import(pathToFileURL(join(core, 'crates/uo-pdf/tests/parity_support/capture_painter.mjs')).href);
const plain = (v) => {
  if (typeof v === 'number') return Number.isFinite(v) && Math.abs(v) < 40000;
  if (Array.isArray(v)) return v.every(plain);
  if (v && typeof v === 'object') return Object.values(v).every(plain);
  return true;
};
const fontDir = join(repo, 'shared/fonts/files');
const fonts = {};
for (const f of readdirSync(fontDir).filter((x) => x.endsWith('.ttf'))) fonts[f] = readFileSync(join(fontDir, f)).toString('base64');
const specs = [];
for (let i = 0; specs.length < SEEDS && i < SEEDS * 6; i++) {
  const spec = cp.genSpec(0x2000 + i * 7919);
  spec.pages = spec.pages.filter((p) => plain(p));
  if (!spec.pages.length || !plain(spec.faces) || !plain(spec.images)) continue;
  specs.push({ seed: i, spec: JSON.parse(JSON.stringify(spec)) });
}

// ── Chromium ────────────────────────────────────────────────────────────────────────────────────
// `--browser chromium|webkit` (default chromium). UO_PLAYWRIGHT (a path to a `playwright` package folder, as run.mjs) wins over the
// frozen root's sheet/node_modules/playwright. WebKit on Windows has no OffscreenCanvas: the harness (and both painters) then use an
// HTMLCanvasElement, the path of Safari 15.
const browserName = process.argv.includes('--browser') ? process.argv[process.argv.indexOf('--browser') + 1] : 'chromium';
if (!['chromium', 'webkit'].includes(browserName)) throw new Error(`--browser chromium|webkit, got ${browserName}`);
const playwright = process.env.UO_PLAYWRIGHT
  ? await import(pathToFileURL(resolve(process.env.UO_PLAYWRIGHT, 'index.mjs')).href)
  : createRequire(join(root, 'sheet/package.json'))('playwright');
const installed = (() => {
  const base = join(process.env.LOCALAPPDATA ?? '', 'ms-playwright');
  if (!existsSync(base)) return undefined;
  const dirs = readdirSync(base).filter((d) => d.startsWith('chromium_headless_shell-')).sort();
  const exe = dirs.length && join(base, dirs[dirs.length - 1], 'chrome-headless-shell-win64', 'chrome-headless-shell.exe');
  return exe && existsSync(exe) ? exe : undefined;
})();
const server = createServer((req, res) => {
  if (req.url === '/') return res.writeHead(200, { 'content-type': 'text/html' }).end('<!doctype html><meta charset="utf-8"><body><script type="module">import { run } from "/bundle.mjs"; window.__run = run; window.__ready = true;</script></body>');
  if (req.url === '/bundle.mjs') return res.writeHead(200, { 'content-type': 'text/javascript' }).end(readFileSync(join(outDir, 'bundle.mjs')));
  res.writeHead(404).end();
});
await new Promise((r) => server.listen(0, '127.0.0.1', r));
const browser = await playwright[browserName].launch(browserName === 'chromium' ? { headless: true, executablePath: process.env.UO_CHROMIUM ?? installed } : { headless: true });
const page = await browser.newPage();
page.on('pageerror', (e) => console.log('PAGE ERROR', e.message));
page.on('console', (m) => { if (m.type() === 'error') console.log('CONSOLE', m.text()); });
await page.goto(`http://127.0.0.1:${server.address().port}/`);
await page.waitForFunction(() => window.__ready === true, null, { timeout: 60000 });
const t0 = Date.now();
const paths = await page.evaluate(() => ({ offscreen: typeof OffscreenCanvas !== 'undefined', kerning: 'fontKerning' in CanvasRenderingContext2D.prototype }));
const rows = await page.evaluate(({ wasmB64, specs, fonts, scales }) => window.__run({ wasmB64, specs, fonts, scales }), { wasmB64: readFileSync(wasmPath).toString('base64'), specs, fonts, scales: [1 / 15, 2.5 / 15] });
const browserVersion = `${browserName} ${browser.version()}`;
await browser.close();
server.close();

const cats = ['pic', 'shapeFx', 'textFx', 'pageFill', 'shapeRaster', 'text'];
const tally = Object.fromEntries([...cats, 'all'].map((c) => [c, { pages: 0, pixels: 0, diffPages: 0, diffPixels: 0, errors: 0, inkPixels: 0, inkRenders: 0 }]));
for (const r of rows) for (const c of ['all', ...cats.filter((k) => r.features[k])]) {
  const t = tally[c];
  t.pages++;
  if (r.err) { t.errors++; continue; }
  t.pixels += r.pixels; t.inkPixels += r.ink; if (r.ink) t.inkRenders++; t.diffPixels += r.diff; if (r.diff) t.diffPages++;
}
console.log(`paint_pixels: ${browserVersion} (OffscreenCanvas ${paths.offscreen ? 'yes' : 'no: HTMLCanvasElement'}, ctx.fontKerning ${paths.kerning ? 'yes' : 'no'}): ${specs.length} generated layouts, ${rows.length} page renders (2 scales) in ${Date.now() - t0} ms`);
for (const [c, t] of Object.entries(tally)) console.log(`  ${c.padEnd(12)} ${String(t.pages).padStart(4)} renders  ${String(t.pixels).padStart(11)} pixels  ${t.diffPixels} differing pixels in ${t.diffPages} renders  ${t.errors} errors  ${t.inkRenders} renders with ink (${t.inkPixels} px)`);
for (const r of rows.filter((x) => x.err || x.diff).slice(0, 12)) console.log('  DIFF', JSON.stringify({ seed: r.seed, page: r.pi, scale: r.s, features: Object.keys(r.features).filter((k) => r.features[k]), diff: r.diff, first: r.first, err: r.err, items: r.items }));
// a category with renders but none with any ink compared nothing: fail
const blank = cats.filter((c) => tally[c].pages > 0 && tally[c].inkRenders === 0);
if (blank.length) console.log('  BLANK categories (no render drew a pixel):', blank.join(', '));
process.exitCode = tally.all.diffPixels || tally.all.errors || blank.length ? 1 : 0;
