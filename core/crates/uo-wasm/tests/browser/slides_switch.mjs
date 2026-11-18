// The TS/Rust switch of <universal-slides-viewer> in a real Chromium (headless): `engine="ts"` against `engine="rust"` over the
// committed samples and the generated `b1a` decks (record core/docs/modules/uo-bindings.md "Slides viewer (B7)").
//
//   [UO_TS_ROOT=<frozen checkout: tools only, default the 0.9.2 reference>] [UO_CHROMIUM=<chrome-headless-shell.exe>]
//   [UO_WASM_PATH=<uo-slides.wasm: the count,slides build>] [UO_PPT_WASM_PATH=<uo-slides-ppt.wasm>]
//   [UO_SS_DECKS=<n of b1a decks, default 40>] [UO_SS_C1D=<n of c1d chart decks, default 20>] [UO_SS_B6A=<n of b6a animated decks, default 40>]
//   [UO_SS_B5B=<n of b5b metafile decks, default 40>] [UO_SS_ONLY=<regex over the deck paths: a focused run>] [UO_SS_DEBUG=1: PNGs of differing slides]
//   [UO_SS_SOAK=200] node crates/uo-wasm/tests/browser/slides_switch.mjs [--mode all|parity|memory|faults|phase0|anim|opencost]
//
// The page is the real thing: the viewer's `src/index.ts` and the Rust engine's JS bundled by the frozen esbuild (splitting, the alias
// table of tools/build-pkg.mjs, `@universal-office/slides-engine`), served by a node:http server together with `chunks/uo-slides.wasm`
// and `chunks/uo-slides-ppt.wasm` (the DEFAULT wasm URLs, no `configureEngine`), the open fonts and the decks.
//   phase0  a viewer without engine="rust" requests neither wasm nor the engine's chunk and its static closure holds none of the engine
//   parity  per deck, TS element against Rust element: slide count and size, notes, the pixels of `renderSlideToCanvas` of every slide
//           at 1280 px and of the main canvas (selectSlide), the thumbnails; the a11y text of every slide; the link hit-tests over a
//           grid of points on every slide; tolerance 0. Wave 4: the Rust element opens WITHOUT the TS model (`pkg` null, no
//           `ensureTsModel`), so every answer above comes from the Rust read model
//   memory  200 slide changes with engine="rust" (count build): live bytes of the module and the wasm buffer flat, no handle left
//   faults  a corrupt deck (both engines throw alike), a forced trap of the instance (the viewer drops the module, keeps painting with
//           the TS engine, and the next load instantiates a fresh module), the hyperlink scheme filter
//   anim    B4b: the animated decks (the playback samples, the b6a decks, the first b1a decks with effects) under a virtual clock:
//           `play({ media: 'seek' })` sampled across the timeline and `present()` driven by `next()` with the clock stepping
//           through the animations, the main canvas of the TS element against the Rust element (whose stage frames come from
//           the wasm: slots evaluated in Rust), tolerance 0, and the media player's requests of every frame (video positions,
//           play/pause, volume, GIF frames: from the TS `mediaStateAt` against the Rust media states); then 300 frames of one deck
//           with the module's live bytes sampled
//   opencost wave 4: per deck in a fresh page, the open cost of the TS element (the TS model) against the Rust element without the
//           TS model (JS heap after gc, the module's live bytes, the time of `loadPptx` and of the first paint)
import { createRequire } from 'node:module';
import { createServer } from 'node:http';
import { mkdirSync, readFileSync, writeFileSync, existsSync, readdirSync, rmSync, copyFileSync } from 'node:fs';
import { join, dirname, resolve, extname, basename } from 'node:path';
import { fileURLToPath } from 'node:url';
import { execFileSync } from 'node:child_process';

const here = dirname(fileURLToPath(import.meta.url));
const core = resolve(here, '../../../..');
const repo = resolve(core, '..');
const root = resolve(process.env.UO_TS_ROOT ?? 'D:/Projects/OfficeNext/.claude/worktrees/ts-ref-092');
const arg = (name, dflt) => { const i = process.argv.indexOf(name); return i >= 0 ? process.argv[i + 1] : dflt; };
const MODE = arg('--mode', 'all');
const NB1A = Number(process.env.UO_SS_DECKS ?? 40);
const SOAK = Number(process.env.UO_SS_SOAK ?? 200);
const wasmPath = process.env.UO_WASM_PATH ?? join(core, 'target/b7/uo-slides-count.wasm');
const pptPath = process.env.UO_PPT_WASM_PATH ?? join(core, 'target/b7/uo-slides-ppt.wasm');
const outDir = join(process.env.CARGO_TARGET_DIR ?? join(core, 'target'), 'slides-viewer');
const out = join(outDir, 'out');
const fwd = (p) => p.replace(/\\/g, '/');
rmSync(out, { recursive: true, force: true });
mkdirSync(join(out, 'chunks'), { recursive: true });

// ── the bundle: the settings and the alias table of tools/build-pkg.mjs buildProduct ─────────────────
const W = (p) => fwd(join(repo, p));
const driver = `
import { UniversalSlidesViewerElement } from '${W('slides/packages/viewer/src/index.ts')}';
export { UniversalSlidesViewerElement };
// Slides wave 5: TS against Rust runs every TS switch (SlidesRules::ts_strict); the product default is PowerPoint's rules
UniversalSlidesViewerElement.configureEngine({ tsStrict: true });
const statsUrl = new URL('./stats.js', import.meta.url).href;
export const engineModule = () => import(statsUrl);
`;
const statsEntry = `export { loadSlidesModule, slidesModuleLoaded, slidesHandles, slidesLiveBytes, slidesMemoryBytes, __trapForTest } from '${W('core/crates/uo-wasm/js/slides-engine.ts')}';\n`;
writeFileSync(join(outDir, 'entry.mjs'), driver);
writeFileSync(join(outDir, 'stats.mjs'), statsEntry);
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
const alias = Object.fromEntries(Object.entries(INTERNAL).map(([n, f]) => [n, join(repo, f)]));
alias['@universal-office/slides-engine'] = join(repo, 'core/crates/uo-wasm/js/slides-engine.ts');
const esbuildPath = join(root, 'write/node_modules/esbuild/lib/main.js');
const esbuild = createRequire(join(root, 'write/package.json'))(existsSync(esbuildPath) ? esbuildPath : 'esbuild');
const built = await esbuild.build({
  entryPoints: { entry: join(outDir, 'entry.mjs'), stats: join(outDir, 'stats.mjs') },
  outdir: out, bundle: true, format: 'esm', platform: 'browser', target: 'es2022', splitting: true, chunkNames: 'chunks/[name]-[hash]',
  minify: true, legalComments: 'none', alias, absWorkingDir: repo, external: ['pdfjs-dist', 'pdfjs-dist/*'],
  nodePaths: [join(root, 'write/node_modules'), join(root, 'slides/node_modules'), join(root, 'sheet/node_modules')], logLevel: 'warning', metafile: true,
});
copyFileSync(wasmPath, join(out, 'chunks', 'uo-slides.wasm'));
if (existsSync(pptPath)) copyFileSync(pptPath, join(out, 'chunks', 'uo-slides-ppt.wasm'));

const outs = built.metafile.outputs;
const closure = (o0) => { const seen = new Set(); const visit = (o) => { if (seen.has(o) || !outs[o]) return; seen.add(o); for (const i of outs[o].imports) if (i.kind === 'import-statement') visit(i.path); }; visit(o0); return seen; };
const entryOut = Object.keys(outs).find((o) => /(^|\/)entry\.js$/.test(fwd(o)));
const staticSet = closure(entryOut);
const staticInputs = [...staticSet].flatMap((o) => Object.keys(outs[o].inputs)).map((f) => fwd(resolve(repo, f)));
const leaked = staticInputs.filter((f) => /core\/crates\/uo-wasm\/js\//.test(f));
const engineChunk = Object.keys(outs).find((o) => /chunks\/rust-engine-/.test(fwd(o)));
const onlyDynamic = engineChunk ? new Set([...closure(engineChunk)].filter((o) => !staticSet.has(o))) : new Set();
const kb = (n) => (n / 1024).toFixed(0);
console.log(`bundle: entry static closure ${staticSet.size} output(s) ${kb([...staticSet].reduce((n, o) => n + outs[o].bytes, 0))} KB; reachable only by import("./rust-engine"): ${onlyDynamic.size} output(s) ${kb([...onlyDynamic].reduce((n, o) => n + outs[o].bytes, 0))} KB + uo-slides.wasm ${kb(readFileSync(wasmPath).length)} KB`);
let failures = 0;
const fail = (m) => { failures++; console.log(`FAIL ${m}`); };
if (leaked.length) fail(`the entry's static closure holds the engine's code: ${leaked.map((f) => f.replace(`${fwd(repo)}/`, '')).join(', ')}`);

// ── the decks ──────────────────────────────────────────────────────────────────────────────────────
const tracked = execFileSync('git', ['-C', repo, 'ls-files', 'slides/samples'], { encoding: 'utf8' }).split('\n').map((s) => s.trim()).filter((s) => /\.(pptx|ppt)$/i.test(s));
const b1aDir = join(core, 'crates/uo-pptx/tests/captures/decks/b1a');
const b1a = existsSync(b1aDir) ? readdirSync(b1aDir).filter((f) => f.endsWith('.pptx')).sort().slice(0, NB1A).map((f) => join(b1aDir, f)) : [];
// C-6: the generated chart decks (c1d): charts are painted by the Rust list now, so their slides are compared like any other
const NC1D = Number(process.env.UO_SS_C1D ?? 20);
const c1dDir = join(core, 'crates/uo-pptx/tests/captures/decks/c1d');
const c1d = existsSync(c1dDir) ? readdirSync(c1dDir).filter((f) => f.endsWith('.pptx')).sort().slice(0, NC1D).map((f) => join(c1dDir, f)) : [];
// B4b: the generated animated decks (b6a): every effect class, iterate by letter/word, charts by parts, media
const NB6A = Number(process.env.UO_SS_B6A ?? 40);
const b6aDir = join(core, 'crates/uo-pptx/tests/captures/decks/b6a');
const b6a = existsSync(b6aDir) ? readdirSync(b6aDir).filter((f) => f.endsWith('.pptx')).sort().slice(0, NB6A).map((f) => join(b6aDir, f)) : [];
// wave 4: the generated metafile decks (b5b: EMF, EMF+, WMF, PICT, TIFF pictures replayed from Rust recipes); EMF+ pictures with
// compressed bitmaps go through the `MetaDecode` prepare phase, started when the deck opens (record uo-bindings.md "Wave 4b")
const NB5B = Number(process.env.UO_SS_B5B ?? 40);
const b5bDir = join(core, 'crates/uo-pptx/tests/captures/decks/b5b');
const b5b = existsSync(b5bDir) ? readdirSync(b5bDir).filter((f) => f.endsWith('.pptx')).sort().slice(0, NB5B).map((f) => join(b5bDir, f)) : [];
// UO_SS_ONLY=<regex>: keep only the decks whose path matches (a focused run, e.g. `/b5b/`)
const only = process.env.UO_SS_ONLY ? new RegExp(process.env.UO_SS_ONLY) : null;
const decks = [...tracked.map((p) => join(repo, p)), ...b1a, ...c1d, ...b5b, ...(MODE === 'all' || MODE === 'anim' ? b6a : [])].filter((p) => !only || only.test(fwd(p)));
const deckByName = new Map(decks.map((p, i) => [`k${i}${extname(p)}`, p]));
const FONT_DIRS = [join(repo, 'shared/fonts/files'), join(repo, 'site/assets/fonts')];
const requests = [];
const MIME = { '.js': 'text/javascript', '.wasm': 'application/wasm', '.ttf': 'font/ttf', '.json': 'application/json' };
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
  if (url.startsWith('/decks/')) { const f = deckByName.get(basename(url)); return f ? send(f) : res.writeHead(404).end(); }
  res.writeHead(404).end();
});
await new Promise((r) => server.listen(0, '127.0.0.1', r));

// ── Chromium ────────────────────────────────────────────────────────────────────────────────────────
const { chromium } = createRequire(join(root, 'slides/package.json'))('playwright');
const installed = (() => {
  const base = join(process.env.LOCALAPPDATA ?? '', 'ms-playwright');
  if (!existsSync(base)) return undefined;
  const dirs = readdirSync(base).filter((d) => d.startsWith('chromium_headless_shell-')).sort();
  const exe = dirs.length && join(base, dirs[dirs.length - 1], 'chrome-headless-shell-win64', 'chrome-headless-shell.exe');
  return exe && existsSync(exe) ? exe : undefined;
})();
const browser = await chromium.launch({ headless: true, executablePath: process.env.UO_CHROMIUM ?? installed, args: ['--js-flags=--expose-gc', '--enable-precise-memory-info'] });
const context = await browser.newContext({ timezoneId: 'UTC', deviceScaleFactor: 1, viewport: { width: 1280, height: 900 } });
let page = null;
async function openPage() {
  if (page) await page.close();
  page = await context.newPage();
  page.on('crash', () => { console.log('PAGE CRASHED'); process.exit(2); });
  page.on('pageerror', (e) => console.log('PAGE ERROR', e.message));
  page.on('console', (m) => { if (m.type() === 'error' && !/Failed to load resource/.test(m.text())) console.log('CONSOLE', m.text()); });
  await page.goto(`http://127.0.0.1:${server.address().port}/`);
  await page.waitForFunction(() => window.__ready === true, null, { timeout: 60000 });
}
await openPage();
const names = [...deckByName.keys()];

// the page-side driver (a function evaluated in the page; everything it needs is passed by argument)
const driverFn = () => {
  const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
  const El = window.__e.UniversalSlidesViewerElement;
  const fnv = (u8) => { let h = 0x811c9dc5; for (let i = 0; i < u8.length; i++) h = Math.imul(h ^ u8[i], 0x01000193); return (h >>> 0).toString(16); };
  const mk = (engine) => {
    const el = new El();
    el.setAttribute('fonts', '/fonts');
    if (engine === 'rust') el.setAttribute('engine', 'rust');
    el.style.cssText = 'display:block;width:1280px;height:720px;position:absolute;left:0;top:0';
    document.body.appendChild(el);
    return el;
  };
  const settle = async (el) => {
    await el.fontsReady();
    if (el.getAttribute('engine') === 'rust') await el.rustReady();
    const t0 = performance.now();
    // the images decode asynchronously (both engines share the viewer's cache): wait until every one is ready
    while (performance.now() - t0 < 4000 && ([...el.imageCache.values()].some((e) => !e.ready) || (el.rust?.document?.preparing ?? 0) > 0)) await sleep(20);
    await sleep(60);
    await el.fontsReady();
    await new Promise((r) => requestAnimationFrame(() => requestAnimationFrame(r)));
  };
  const load = async (el, name) => {
    const bytes = new Uint8Array(await (await fetch(`/decks/${name}`)).arrayBuffer());
    // engine="rust": the module is instantiated before the bytes (as `src` does), so the deck opens only in Rust
    if (el.getAttribute('engine') === 'rust') await el.preloadRust();
    el.loadPptx(bytes);
    await settle(el);
    return bytes.length;
  };
  const pixels = (cv) => { const c = cv.getContext('2d'); return c.getImageData(0, 0, cv.width, cv.height).data; };
  const diff = (a, b) => {
    if (a.length !== b.length) return -1;
    let n = 0;
    for (let i = 0; i < a.length; i += 4) if (a[i] !== b[i] || a[i + 1] !== b[i + 1] || a[i + 2] !== b[i + 2] || a[i + 3] !== b[i + 3]) n++;
    return n;
  };
  const render = (el, i, w) => { const cv = document.createElement('canvas'); el.renderSlideToCanvas(i, cv, w); return pixels(cv); };
  const thumb = (el, i) => { const cv = document.createElement('canvas'); cv.style.cssText = 'width:200px;height:112px'; document.body.appendChild(cv); el.renderThumbnail(i, cv); const d = pixels(cv); cv.remove(); return d; };
  const mainCanvas = (el) => el.canvas;

  /** TS element against Rust element on one deck */
  const compare = async (name) => {
    const ts = mk('ts'), rs = mk('rust');
    const r = { name, slides: 0, engine: 'ts', slideDiff: [], mainDiff: 0, mainChecked: 0, thumbDiff: 0, notesDiff: 0, a11yDiff: 0, a11yChecked: 0, linksDiff: 0, linkProbes: 0, linkHits: 0, linkExtra: 0, tsFree: false, tsLoads: 0, charts: 0, pixels: 0, errors: [], painted: [0, 0] };
    try {
      const errs = [];
      rs.addEventListener('us-engine-error', (e) => errs.push(String(e.detail?.error?.message ?? e.detail?.error)));
      await load(ts, name);
      await load(rs, name);
      r.engine = rs.getEngine();
      // C-6: count what the Rust engine painted and what it left to the TS painter (a status other than 0), chart slides apart
      const eng = rs.rust;
      if (eng && typeof eng.paint === 'function') {
        const orig = eng.paint.bind(eng);
        eng.paint = (ctx, i, ...rest) => { const x = orig(ctx, i, ...rest); r.painted[x.status === 0 ? 0 : 1]++; if (x.status !== 0) (r.statuses ??= []).push([i, x.status]); return x; };
      }
      r.errors = errs;
      r.slides = ts.getSlideCount();
      if (rs.getSlideCount() !== r.slides) r.errors.push(`count ${rs.getSlideCount()} != ${r.slides}`);
      for (let i = 0; i < r.slides; i++) {
        const hasChart = ts.slides[i].shapes.some((s) => s.chartXml);
        if (hasChart) r.charts++;
        const a = render(ts, i, 1280), b = render(rs, i, 1280);
        const d = diff(a, b);
        r.pixels += a.length / 4;
        if (d !== 0) r.slideDiff.push({ i, d, chart: hasChart });
        if (d !== 0 && window.__ssDebug) {
          const url = (el) => { const cv = document.createElement('canvas'); el.renderSlideToCanvas(i, cv, 1280); return cv.toDataURL(); };
          (r.png ??= []).push([i, url(ts), url(rs)]);
        }
        if (i < 6) {
          const ta = thumb(ts, i), tb = thumb(rs, i);
          if (diff(ta, tb) !== 0) r.thumbDiff++;
          ts.selectSlide(i); rs.selectSlide(i);
          await new Promise((rr) => requestAnimationFrame(rr));
          const ma = pixels(mainCanvas(ts)), mb = pixels(mainCanvas(rs));
          r.mainChecked++;
          if (diff(ma, mb) !== 0) r.mainDiff++;
        }
        if (ts.getSlideNotes(i) !== rs.getSlideNotes(i)) r.notesDiff++;
        // the accessible text of the slide (heading, paragraphs, tables, alternative texts) and the link hit-tests on it
        ts.selectSlide(i); rs.selectSlide(i);
        const a11y = (el) => (el.shadowRoot?.textContent ?? '') + '|' + (el.canvas.getAttribute('aria-label') ?? '');
        r.a11yChecked++;
        if (a11y(ts) !== a11y(rs)) r.a11yDiff++;
        const box = ts.canvas.getBoundingClientRect();
        for (let gy = 0; gy < 24; gy++) for (let gx = 0; gx < 40; gx++) {
          const ev = { clientX: box.left + (gx + 0.5) * box.width / 40, clientY: box.top + (gy + 0.5) * box.height / 24 };
          const a = ts.hitTestLink(ev), b = rs.hitTestLink(ev);
          r.linkProbes++;
          if (a) r.linkHits++;
          if (!a && b && !('url' in b)) { r.linkExtra++; continue; } // a slide jump or a shape click: Rust only (declared)
          if (JSON.stringify(a) !== JSON.stringify(b)) r.linksDiff++;
        }
      }
      r.tsFree = rs.pkg === null;
      r.tsLoads = rs.tsModelLoads;
      r.metaDecoded = rs.rust?.document?.prepared ?? [0, 0];
    } catch (e) {
      r.errors.push(`exception ${e?.message ?? e}`);
    } finally {
      ts.remove(); rs.remove();
    }
    return r;
  };
  /** B4b: TS element against Rust element on the animated frames of one deck, under a virtual clock */
  const animate = async (name, steps) => {
    const ts = mk('ts'), rs = mk(window.__ssTsTs ? 'ts' : 'rust');
    let clock = 0;
    for (const el of [ts, rs]) el.now = () => clock;
    const r = { name, engine: 'ts', frames: 0, diff: 0, rust: 0, firstDiff: null, errors: [], mediaFrames: 0, mediaReq: 0, mediaDiff: 0, firstMedia: null, tsFree: false };
    try {
      const errs = [];
      rs.addEventListener('us-engine-error', (e) => errs.push(String(e.detail?.error?.message ?? e.detail?.error)));
      await load(ts, name);
      await load(rs, name);
      r.engine = rs.getEngine();
      const eng = rs.rust;
      if (eng) { const o = eng.paintStage.bind(eng); eng.paintStage = (...a) => { const x = o(...a); if (x.status === 0 && !x.empty) r.rust++; return x; }; }
      // the media player's requests of a frame: video positions, play/pause, volume, mute, full screen, GIF frames
      // per media element (by its part or address); an element that failed to load (`error`: the generated decks hold fake media
      // bytes) is skipped on both sides, since the player stops asking for it at a moment the browser decides
      const mediaReq = (el, other) => JSON.stringify({
        v: [...el.media.els.entries()].filter(([k, e]) => !e.error && !other.media.els.get(k)?.error).map(([k, e]) => [k, el.media.wanted.get(e) ?? null]),
        g: [...el.media.gifShown.entries()], full: !!el.media.fullScreen });
      const frame = (what) => {
        ts.media.resetWanted(); rs.media.resetWanted();
        ts.paint(); rs.paint();
        const ma = mediaReq(ts, rs), mb = mediaReq(rs, ts);
        if (ma !== mb || ts.media.wanted.size + ts.media.gifShown.size > 0) {
          r.mediaFrames++;
          r.mediaReq += ts.media.wanted.size + ts.media.gifShown.size;
          if (ma !== mb) { r.mediaDiff++; r.firstMedia ??= { what, ts: ma, rust: mb }; }
        }
        const d = diff(pixels(ts.canvas), pixels(rs.canvas));
        r.frames++;
        if (d !== 0) {
          r.diff++;
          if (!r.firstDiff) {
            const cvd = (a, b) => (a && b ? diff(pixels(a), pixels(b)) : 'none');
            r.firstDiff = { what, d, to: cvd(ts.toCanvas, rs.toCanvas), from: cvd(ts.fromCanvas, rs.fromCanvas), keys: [ts.fromKey, rs.fromKey],
              ...(window.__ssDebug ? { png: [ts.canvas.toDataURL(), rs.canvas.toDataURL(), ts.fromCanvas?.toDataURL(), rs.fromCanvas?.toDataURL()].filter(Boolean) } : {}) };
          }
        }
      };
      const halt = (el) => {
        if (el.auto?.rafId) { cancelAnimationFrame(el.auto.rafId); el.auto.rafId = 0; }
        if (el.show?.rafId) { cancelAnimationFrame(el.show.rafId); el.show.rafId = 0; }
        if (el.show?.timer) { clearTimeout(el.show.timer); el.show.timer = 0; }
      };
      // automatic playback: the timeline sampled at `steps` instants (and its end)
      ts.play({ media: 'seek' }); rs.play({ media: 'seek' });
      halt(ts); halt(rs);
      // `play()` takes its start from the real clock: put both on the virtual one (t = clock)
      for (const el of [ts, rs]) el.auto.t0 = -(el.auto.tl.slides[0]?.startMs ?? 0);
      const dur = ts.auto.tl.durationMs;
      for (let k = 0; k <= steps; k++) { clock = Math.min(dur, (dur * k) / steps); frame(`play t=${clock}`); }
      ts.stopPlayback(); rs.stopPlayback();
      // presentation: clicks, the clock stepping through the transitions and the effects
      clock = 1e6;
      ts.present(0, { fullscreen: false }); rs.present(0, { fullscreen: false });
      for (let k = 0; k < steps && ts.show; k++) {
        halt(ts); halt(rs);
        for (const dt of [0, 180, 420, 900]) { clock += dt; frame(`show step ${k} +${dt}`); halt(ts); halt(rs); }
        ts.next(); rs.next();
      }
      ts.stopPresentation(); rs.stopPresentation();
      r.tsFree = rs.pkg === null && rs.tsModelLoads === 0;
      r.errors = errs;
    } catch (e) {
      r.errors.push(`exception ${e?.message ?? e}`);
    } finally {
      ts.remove(); rs.remove();
    }
    return r;
  };
  return { compare, animate, mk, load, settle, fnv, sleep, render, diff, pixels, El };
};
const install = () => page.evaluate(`window.__d = (${driverFn.toString()})()`);
await install();

const bytesOf = (n) => readFileSync(deckByName.get(n));

// ── phase 0 ──────────────────────────────────────────────────────────────────────────────────────────
if (MODE === 'all' || MODE === 'phase0') {
  await openPage(); await install();
  const before = requests.length;
  await page.evaluate(async (n) => { const el = window.__d.mk('ts'); await window.__d.load(el, n); el.remove(); }, names[0]);
  const asked = requests.slice(before).filter((u) => /uo-slides|rust-engine|stats-/.test(u));
  if (asked.length) fail(`phase 0: a TS viewer requested ${asked.join(', ')}`);
  else console.log('phase 0: a viewer without engine="rust" requested neither the wasm nor the engine chunk (0 requests)');
}

// ── parity ──────────────────────────────────────────────────────────────────────────────────────────
const totals = { rustPaints: 0, leftToTs: 0, decks: 0, rust: 0, slides: 0, equalSlides: 0, diffSlides: 0, chartSlides: 0, chartDiffs: 0, nonChartDiffs: 0, pixels: 0, mainDiff: 0, mainChecked: 0, thumbDiff: 0, notesDiff: 0, a11yDiff: 0, a11yChecked: 0, linksDiff: 0, linkProbes: 0, linkHits: 0, linkExtra: 0, tsFree: 0, tsLoads: 0, metaDecoded: [0, 0], errors: 0 };
if (MODE === 'all' || MODE === 'parity') {
  const BATCH = Number(process.env.UO_SS_BATCH ?? 12);
  const t0 = Date.now();
  const bad = [];
  for (let k = 0; k < names.length; k++) {
    if (k % BATCH === 0) { await openPage(); await install(); }
    if (process.env.UO_SS_DEBUG) await page.evaluate(() => { window.__ssDebug = true; });
    const r = await page.evaluate((n) => window.__d.compare(n), names[k]);
    if (r.png) {
      for (const [i, ...urls] of r.png) urls.forEach((u, j) => writeFileSync(join(outDir, `parity-diff-${k}-s${i}-${j ? 'rust' : 'ts'}.png`), Buffer.from(u.split(',')[1], 'base64')));
      delete r.png;
    }
    totals.decks++;
    if (r.engine === 'rust') totals.rust++;
    totals.slides += r.slides;
    totals.pixels += r.pixels;
    totals.chartSlides += r.charts;
    totals.rustPaints += r.painted[0];
    totals.leftToTs += r.painted[1];
    totals.diffSlides += r.slideDiff.length;
    totals.equalSlides += r.slides - r.slideDiff.length;
    totals.chartDiffs += r.slideDiff.filter((s) => s.chart).length;
    totals.nonChartDiffs += r.slideDiff.filter((s) => !s.chart).length;
    totals.mainDiff += r.mainDiff; totals.mainChecked += r.mainChecked; totals.thumbDiff += r.thumbDiff;
    totals.notesDiff += r.notesDiff; totals.a11yDiff += r.a11yDiff; totals.a11yChecked += r.a11yChecked; totals.errors += r.errors.length;
    totals.metaDecoded[0] += r.metaDecoded?.[0] ?? 0; totals.metaDecoded[1] += r.metaDecoded?.[1] ?? 0;
    totals.linksDiff += r.linksDiff; totals.linkProbes += r.linkProbes; totals.linkHits += r.linkHits; totals.linkExtra += r.linkExtra;
    if (r.tsFree) totals.tsFree++;
    totals.tsLoads += r.tsLoads;
    const nc = r.slideDiff.filter((s) => !s.chart);
    if (r.slideDiff.length || r.errors.length || r.engine !== 'rust' || r.mainDiff || r.thumbDiff || r.notesDiff || r.a11yDiff || r.linksDiff || !r.tsFree || r.tsLoads) bad.push({ deck: relative(deckByName.get(r.name)), engine: r.engine, errors: r.errors, diffs: process.env.UO_SS_ONLY ? r.slideDiff : r.slideDiff.slice(0, 4), statuses: r.statuses, main: r.mainDiff, thumb: r.thumbDiff, notes: r.notesDiff, a11y: r.a11yDiff, links: r.linksDiff, tsFree: r.tsFree, tsLoads: r.tsLoads });
  }
  console.log(`parity: ${totals.decks} decks (${totals.rust} opened in the Rust engine), ${totals.slides} slides, ${totals.equalSlides} equal / ${totals.diffSlides} different (${totals.chartDiffs} of them with a chart frame, ${totals.nonChartDiffs} without; ${totals.chartSlides} slides have charts; the Rust list painted ${totals.rustPaints} slide paints, ${totals.leftToTs} were left to the TS painter), ${totals.pixels} pixels compared, tolerance 0`);
  console.log(`        main canvas: ${totals.mainChecked} compared, ${totals.mainDiff} different; thumbnails: ${totals.thumbDiff} different; notes: ${totals.notesDiff} different; a11y text: ${totals.a11yChecked} slides, ${totals.a11yDiff} different; errors ${totals.errors}; ${((Date.now() - t0) / 1000).toFixed(0)} s`);
  console.log(`        link hit-tests: ${totals.linkProbes} points, ${totals.linkHits} on a TS link, ${totals.linksDiff} different, ${totals.linkExtra} Rust-only jumps/shape clicks; decks opened without the TS model: ${totals.tsFree}/${totals.decks}, TS model loads ${totals.tsLoads}; EMF+ compressed bitmaps through the prepare phase: ${totals.metaDecoded[0]} decoded, ${totals.metaDecoded[1]} undecodable`);
  for (const b of bad.slice(0, 25)) console.log('  DIFF', JSON.stringify(b));
  if (totals.a11yDiff || totals.linksDiff) fail(`parity: ${totals.a11yDiff} a11y texts and ${totals.linksDiff} link hit-tests differ`);
  if (totals.tsFree !== totals.decks || totals.tsLoads) fail(`parity: ${totals.decks - totals.tsFree} decks kept the TS model, ${totals.tsLoads} TS model loads`);
  if (totals.diffSlides || totals.errors || totals.rust !== totals.decks) fail(`parity: ${totals.diffSlides} slides differ (${totals.chartDiffs} with charts), ${totals.errors} errors, ${totals.decks - totals.rust} decks stayed on the TS engine`);
}
function relative(f) { return fwd(f).replace(`${fwd(repo)}/`, ''); }

// ── anim (B4b) ───────────────────────────────────────────────────────────────────────────────────────
if (MODE === 'all' || MODE === 'anim') {
  const isAnim = (n) => { const p = fwd(deckByName.get(n)); return /\/samples\/playback\//.test(p) || /\/b6a\//.test(p) || /\/b1a\//.test(p); };
  const only = process.env.UO_SS_ONLY;
  const animNames = names.filter(isAnim).filter((n) => !only || fwd(deckByName.get(n)).includes(only));
  const flags = () => page.evaluate(({ dbg, tsts }) => { window.__ssDebug = dbg; window.__ssTsTs = tsts; }, { dbg: !!process.env.UO_SS_DEBUG, tsts: !!process.env.UO_SS_TSTS });
  const t0 = Date.now();
  const tot = { decks: 0, rust: 0, frames: 0, diff: 0, rustFrames: 0, errors: 0, mediaFrames: 0, mediaReq: 0, mediaDiff: 0, tsFree: 0 };
  const bad = [];
  for (let k = 0; k < animNames.length; k++) {
    if (k % 10 === 0) { await openPage(); await install(); await flags(); }
    const r = await page.evaluate(({ n, steps }) => window.__d.animate(n, steps), { n: animNames[k], steps: 12 });
    if (r.firstDiff?.png) {
      r.firstDiff.png.forEach((u, i) => writeFileSync(join(outDir, `anim-diff-${k}-${['ts', 'rust', 'ts-from', 'rust-from'][i]}.png`), Buffer.from(u.split(',')[1], 'base64')));
      delete r.firstDiff.png;
    }
    tot.decks++; if (r.engine === 'rust') tot.rust++;
    tot.frames += r.frames; tot.diff += r.diff; tot.rustFrames += r.rust; tot.errors += r.errors.length;
    tot.mediaFrames += r.mediaFrames; tot.mediaReq += r.mediaReq; tot.mediaDiff += r.mediaDiff; if (r.tsFree) tot.tsFree++;
    if (r.diff || r.errors.length || r.engine !== 'rust' || r.mediaDiff || !r.tsFree) bad.push({ deck: relative(deckByName.get(r.name)), engine: r.engine, diff: r.diff, first: r.firstDiff, media: r.firstMedia, tsFree: r.tsFree, errors: r.errors });
  }
  console.log(`anim: ${tot.decks} animated decks (${tot.rust} in the Rust engine, ${tot.tsFree} without the TS model), ${tot.frames} frames (play + present), ${tot.frames - tot.diff} pixel-equal / ${tot.diff} different, ${tot.rustFrames} stage paints by the Rust list, errors ${tot.errors}; ${((Date.now() - t0) / 1000).toFixed(0)} s`);
  console.log(`      media: ${tot.mediaFrames} frames with media requests (${tot.mediaReq} requests), ${tot.mediaDiff} different`);
  for (const b of bad.slice(0, 25)) console.log('  DIFF', JSON.stringify(b));
  if (tot.mediaDiff || tot.tsFree !== tot.decks) fail(`anim: ${tot.mediaDiff} frames with different media requests, ${tot.decks - tot.tsFree} decks with the TS model`);
  if (tot.diff || tot.errors || tot.rust !== tot.decks || tot.rustFrames === 0) fail(`anim: ${tot.diff} frames differ, ${tot.errors} errors, ${tot.decks - tot.rust} decks stayed on the TS engine`);
  // memory of the playback: 300 frames of one animated deck, the module's live bytes flat
  await openPage(); await install();
  const m = await page.evaluate(async ({ n }) => {
    const d = window.__d;
    const st = await window.__e.engineModule();
    const el = d.mk('rust');
    let clock = 0;
    el.now = () => clock;
    await d.load(el, n);
    el.play({ media: 'seek', loop: true });
    if (el.auto?.rafId) { cancelAnimationFrame(el.auto.rafId); el.auto.rafId = 0; }
    el.auto.t0 = 0;
    const dur = Math.max(1, el.auto.tl.durationMs);
    const samples = [];
    for (let i = 0; i < 300; i++) {
      clock = (i * 97) % dur;
      el.paint();
      if (i % 50 === 49) samples.push(st.slidesLiveBytes());
    }
    el.stopPlayback();
    const engine = el.getEngine();
    el.remove();
    await d.sleep(100);
    return { samples, engine, handles: st.slidesHandles(), live: st.slidesLiveBytes() };
  }, { n: animNames.find((x) => /b6a/.test(fwd(deckByName.get(x)))) ?? animNames[0] });
  const spread = Math.max(...m.samples) - Math.min(...m.samples);
  console.log(`anim memory: engine ${m.engine}, 300 playback frames, module live bytes ${m.samples.join(', ')} (spread ${spread}); handles after the element left ${m.handles}, live bytes ${m.live}`);
  if (spread > 256 * 1024) fail(`anim memory: live bytes spread ${spread}`);
  if (m.handles !== 0) fail(`anim memory: ${m.handles} handles left`);
}

// ── memory ───────────────────────────────────────────────────────────────────────────────────────────
if (MODE === 'all' || MODE === 'memory') {
  await openPage(); await install();
  const r = await page.evaluate(async ({ cands, soak }) => {
    const d = window.__d;
    const st = await window.__e.engineModule();
    // the deck with the most slides among the candidates (the TS element counts them)
    let best = cands[0], bestN = 0;
    for (const c of cands) { const t = d.mk('ts'); await d.load(t, c); const k = t.getSlideCount(); t.remove(); if (k > bestN) { best = c; bestN = k; } }
    const el = d.mk('rust');
    await d.load(el, best);
    const count = el.getSlideCount();
    const cv = document.createElement('canvas');
    const live = () => st.slidesLiveBytes();
    const mem = () => ({ live: live(), buffer: st.slidesMemoryBytes(), heap: performance.memory?.usedJSHeapSize ?? 0 });
    const samples = [];
    for (let i = 0; i < 20; i++) { el.selectSlide(i % count); el.renderSlideToCanvas(i % count, cv, 640); }
    samples.push(mem());
    const t0 = performance.now();
    for (let i = 0; i < soak; i++) {
      el.selectSlide((i * 7) % count);
      el.renderSlideToCanvas(i % count, cv, 640 + (i % 5));
      if (i % 50 === 49) { await d.sleep(30); samples.push(mem()); }
    }
    const ms = performance.now() - t0;
    const engine = el.getEngine();
    el.remove();
    await d.sleep(100);
    return { count, samples, ms, engine, handlesAfterRemove: st.slidesHandles(), liveAfterRemove: live() };
  }, { cands: names.filter((k) => k.endsWith('.pptx')).slice(0, 14), soak: SOAK });
  const lives = r.samples.map((s) => s.live);
  const spread = Math.max(...lives) - Math.min(...lives);
  const bufs = r.samples.map((x) => x.buffer);
  console.log(`memory: engine ${r.engine}, ${SOAK} slide changes (+${SOAK} exports) of a ${r.count}-slide deck in ${(r.ms / 1000).toFixed(1)} s (${(r.ms / SOAK).toFixed(1)} ms per change+export); module live bytes ${lives.join(', ')} (spread ${spread}); wasm buffer ${bufs[0]} -> ${bufs[bufs.length - 1]} B; handles after the element left: ${r.handlesAfterRemove}, live bytes ${r.liveAfterRemove}`);
  if (r.engine !== 'rust') fail('memory: the deck did not open in the Rust engine');
  if (spread > 4 * 1024 * 1024) fail(`memory: live bytes spread ${spread}`);
  if (r.handlesAfterRemove !== 0) fail(`memory: ${r.handlesAfterRemove} handles left after the element was removed`);
}

// ── faults ───────────────────────────────────────────────────────────────────────────────────────────
if (MODE === 'all' || MODE === 'faults') {
  await openPage(); await install();
  const good = names.find((k) => k.endsWith('.pptx'));
  const r = await page.evaluate(async ({ n }) => {
    const d = window.__d;
    const st = await window.__e.engineModule();
    const res = {};
    const bytes = new Uint8Array(await (await fetch(`/decks/${n}`)).arrayBuffer());
    // a corrupt deck: both engines throw the same way and keep the previous deck
    for (const engine of ['ts', 'rust']) {
      const el = d.mk(engine);
      el.loadPptx(bytes);
      await d.settle(el);
      const count = el.getSlideCount();
      const bad = [bytes.slice(0, 40), new Uint8Array(0), bytes.map((b, i) => (i % 7 === 0 ? b ^ 0xff : b)).slice(0, 2000)];
      const msgs = [];
      for (const b of bad) { try { el.loadPptx(b); msgs.push('no error'); } catch (e) { msgs.push('threw'); } }
      await d.settle(el);
      res[engine] = { count, afterCount: el.getSlideCount(), msgs, engine: el.getEngine() };
      el.remove();
    }
    // a trap of the instance: the viewer drops the module, goes on with the TS engine, and the next load starts a fresh module
    const el = d.mk('rust');
    await d.load(el, n);
    res.beforeTrap = el.getEngine();
    const errs = [];
    el.addEventListener('us-engine-error', (e) => errs.push(String(e.detail?.error?.message ?? e.detail?.error)));
    try { st.__trapForTest(); } catch (e) { res.trapThrew = e instanceof WebAssembly.RuntimeError; }
    const cv = document.createElement('canvas');
    el.renderSlideToCanvas(0, cv, 640);
    const px = d.pixels(cv);
    res.paintedAfterTrap = px.some((v, i) => i % 4 !== 3 && v !== 0);
    res.afterTrap = el.getEngine();
    res.trapEvents = errs.length;
    res.loaded = st.slidesModuleLoaded();
    await d.load(el, n);
    res.reload = el.getEngine();
    res.loadedAgain = st.slidesModuleLoaded();
    el.remove();
    return res;
  }, { n: good });
  console.log('faults:', JSON.stringify(r));
  if (JSON.stringify(r.ts.msgs) !== JSON.stringify(r.rust.msgs)) fail('faults: the engines disagree on a corrupt deck');
  if (r.ts.count !== r.ts.afterCount || r.rust.count !== r.rust.afterCount) fail('faults: a corrupt deck replaced the open one');
  if (r.rust.engine !== 'rust') fail('faults: the Rust element lost its engine after a corrupt load');
  if (!(r.trapThrew && r.paintedAfterTrap && r.afterTrap === 'ts' && r.reload === 'rust' && r.loadedAgain)) fail('faults: the trap recovery did not behave');
  // the hyperlink scheme filter
  const links = await page.evaluate(async () => {
    const d = window.__d;
    const opened = [];
    const realOpen = window.open;
    window.open = (u, t, f) => { opened.push([u, f]); return null; };
    const out = {};
    for (const engine of ['ts', 'rust']) {
      const el = d.mk(engine);
      el.slides = [{ shapes: [] }];
      el.pres = { slideW: 9144000, slideH: 6858000, slides: [] };
      el.hitTestLink = () => ({ url: window.__link });
      for (const u of ['https://example.com/a', 'mailto:a@b.c', 'javascript:alert(1)', 'data:text/html,x', 'file:///c:/x', ' HTTP://X.org']) {
        window.__link = u;
        el.onClick({ preventDefault() {}, button: 0, clientX: 0, clientY: 0 });
      }
      out[engine] = opened.splice(0);
      el.remove();
    }
    window.open = realOpen;
    return out;
  });
  console.log('links:', JSON.stringify(links));
  const allowed = links.rust.map((o) => o[0]);
  if (!(allowed.length === 3 && !allowed.some((u) => /^(javascript|data|file):/i.test(u)) && links.rust.every((o) => /noreferrer/.test(o[1])))) fail('faults: the Rust viewer\'s link filter');
  if (links.ts.length !== 6) fail('faults: the TS viewer must keep opening every address (frozen behaviour)');
}

// ── open cost (wave 4) ───────────────────────────────────────────────────────────────────────────────
if (MODE === 'all' || MODE === 'opencost') {
  const pick = names.filter((k) => k.endsWith('.pptx')).map((k) => [k, readFileSync(deckByName.get(k)).length]).sort((a, b) => b[1] - a[1]).slice(0, Number(process.env.UO_SS_COST ?? 12)).map((x) => x[0]);
  const rows = [];
  for (const n of pick) {
    const one = {};
    for (const engine of ['ts', 'rust']) {
      await openPage(); await install();
      one[engine] = await page.evaluate(async ({ n, engine }) => {
        const d = window.__d;
        const st = await window.__e.engineModule();
        const bytes = new Uint8Array(await (await fetch(`/decks/${n}`)).arrayBuffer());
        const el = d.mk(engine);
        if (engine === 'rust') await el.preloadRust();
        const heap = async () => { for (let i = 0; i < 4; i++) { window.gc?.(); await d.sleep(20); } return performance.memory?.usedJSHeapSize ?? 0; };
        const live0 = st.slidesLiveBytes();
        const h0 = await heap();
        const t0 = performance.now();
        el.loadPptx(bytes);
        const t1 = performance.now();
        await new Promise((r) => requestAnimationFrame(() => r(undefined)));
        const t2 = performance.now();
        await d.settle(el);
        const h1 = await heap();
        const res = { heap: h1 - h0, live: engine === 'rust' ? st.slidesLiveBytes() - Math.max(0, live0) : 0, loadMs: t1 - t0, firstPaintMs: t2 - t0,
          tsFree: el.pkg === null, slides: el.getSlideCount(), bytes: bytes.length };
        el.remove();
        return res;
      }, { n, engine });
    }
    rows.push({ deck: relative(deckByName.get(n)), ...one });
  }
  const sum = (f) => rows.reduce((a, r) => a + f(r), 0);
  for (const r of rows) console.log(`  opencost ${r.deck} (${r.ts.slides} slides, ${(r.ts.bytes / 1024).toFixed(0)} KB): TS heap ${(r.ts.heap / 1048576).toFixed(2)} MB, load ${r.ts.loadMs.toFixed(1)} ms | Rust heap ${(r.rust.heap / 1048576).toFixed(2)} MB + module ${(r.rust.live / 1048576).toFixed(2)} MB, load ${r.rust.loadMs.toFixed(1)} ms, TS-free ${r.rust.tsFree}`);
  console.log(`opencost: ${rows.length} decks; JS heap after open: TS ${(sum((r) => r.ts.heap) / 1048576).toFixed(2)} MB, Rust ${(sum((r) => r.rust.heap) / 1048576).toFixed(2)} MB + module live ${(sum((r) => r.rust.live) / 1048576).toFixed(2)} MB` +
    ` (before wave 4 the Rust element held both: ${((sum((r) => r.ts.heap) + sum((r) => r.rust.live)) / 1048576).toFixed(2)} MB); loadPptx: TS ${sum((r) => r.ts.loadMs).toFixed(0)} ms, Rust ${sum((r) => r.rust.loadMs).toFixed(0)} ms; first frame: TS ${sum((r) => r.ts.firstPaintMs).toFixed(0)} ms, Rust ${sum((r) => r.rust.firstPaintMs).toFixed(0)} ms`);
  if (rows.some((r) => !r.rust.tsFree)) fail('opencost: a Rust element kept the TS model');
}

await browser.close();
server.close();
console.log(failures ? `${failures} failure(s)` : 'slides_switch: all green');
process.exit(failures ? 1 : 0);
