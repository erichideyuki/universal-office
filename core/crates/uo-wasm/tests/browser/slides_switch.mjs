// The TS/Rust switch of <universal-slides-viewer> in a real Chromium (headless): `engine="ts"` against `engine="rust"` over the
// committed samples and the generated `b1a` decks (record core/docs/modules/uo-bindings.md "Slides viewer (B7)").
//
//   [UO_TS_ROOT=<frozen checkout: tools only, default the 0.9.2 reference>] [UO_CHROMIUM=<chrome-headless-shell.exe>]
//   [UO_WASM_PATH=<uo-slides.wasm: the count,slides build>] [UO_PPT_WASM_PATH=<uo-slides-ppt.wasm>]
//   [UO_SS_DECKS=<n of b1a decks, default 40>] [UO_SS_C1D=<n of c1d chart decks, default 20>] [UO_SS_B6A=<n of b6a animated decks, default 40>]
//   [UO_SS_B5B=<n of b5b metafile decks, default 40>] [UO_SS_ONLY=<regex over the deck paths: a focused run>] [UO_SS_DEBUG=1: PNGs of differing slides]
//   [UO_SS_SOAK=200] node crates/uo-wasm/tests/browser/slides_switch.mjs [--mode all|parity|memory|faults|phase0|anim|opencost|serve]
//   serve only: [UO_SS_PORT=8767] [UO_SS_STRIDE=<every n-th tracked deck, default 5>] [UO_SS_ANIM=<n of animated decks, default 6>] (UO_SS_SOAK as above)
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
//   serve    real Safari (macOS, iOS) on BrowserStack, which cannot reach localhost: expose UO_SS_PORT (8767, 127.0.0.1) with a
//           temporary tunnel and delete it after. No browser is launched: `/` is a page that runs by itself (no Playwright on the
//           page side) the same `compare` (pixels, main canvas, thumbnails, notes, a11y text, link hit-tests; tolerance 0, the
//           device's dpr) on every UO_SS_STRIDE-th git-tracked deck of the parity set (samples, b1a, c1d, b5b, b6a), the same
//           `animate` (play/present under the element's own virtual clock `el.now`) on UO_SS_ANIM animated decks of the stride
//           (8 steps), a soak of UO_SS_SOAK slide changes (count build: module live bytes flat, 0 handles), and POSTs every result to
//           `/result` (appended to <outDir>/serve-results.jsonl, one line printed per deck); it then walks `/csp?v=strict|wasm|eval`
//           (does wasm compile without 'wasm-unsafe-eval') to `/done`. Every page has `#status` (progress; `done …` with the totals
//           on `/done`) for WebDriver. The page fixes `Date` to NOW, releases each comparison canvas after reading it, and resumes
//           after a tab crash (recorded with the deck and step that was running). `/?solo=ts|rust&decks=k3.pptx,k9.ppt`: one viewer
//           of one engine per deck, no comparison (crash triage); `/?decks=k3.pptx,…` compares only those; `/?debug=1` sends the
//           PNGs of differing slides (written next to the results); `/?nooffscreen=1` hides OffscreenCanvas (a Safari < 16.4 in a
//           newer browser); `/?nocsp=1` stays on the page at the end; `/?fresh=1` starts a new run in the same tab (without it a
//           second navigation mid-run resumes, as after a crash); `/?fontdelay=<ms>` delays every font file (served from
//           /fonts/d<ms>/), `/?fontlag=<ms>` defers `FontFaceSet.add` (Safari applies a new face to canvas text late), the two
//           reproduce the font race of real Safari 26; `/?tsts=1` compares TS against TS (a control); the `start` record carries a
//           canvas readback-noise probe (`readback`). The verdict is PASS, FAIL, or PASS-EXCEPT-KNOWN: without
//           OffscreenCanvas the TS metafile/EMF+/PICT players skip embedded bitmaps and pattern brushes (the b5b decks) while the
//           Rust path draws them; such pixel-only rows are flagged `known` (nothing else may differ). The a11y read waits 80 ms
//           after `selectSlide` here (the live region's 60 ms announcement timer races on slow devices). Only git-tracked (open)
//           fonts and decks are served. The Rust engine runs with `tsStrict: true` like every other mode.
import { createRequire } from 'node:module';
import { createServer } from 'node:http';
import { mkdirSync, readFileSync, writeFileSync, existsSync, readdirSync, rmSync, copyFileSync, appendFileSync } from 'node:fs';
import { join, dirname, resolve, extname, basename } from 'node:path';
import { fileURLToPath } from 'node:url';
import { execFileSync } from 'node:child_process';
import { NOW } from '../support/paint-common.mjs';

const here = dirname(fileURLToPath(import.meta.url));
const core = resolve(here, '../../../..');
const repo = resolve(core, '..');
const root = resolve(process.env.UO_TS_ROOT ?? 'D:/Projects/OfficeNext/.claude/worktrees/ts-ref-092');
const arg = (name, dflt) => { const i = process.argv.indexOf(name); return i >= 0 ? process.argv[i + 1] : dflt; };
const MODE = arg('--mode', 'all');
const SERVE = MODE === 'serve';
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
const FONT_DIRS = [join(repo, 'shared/fonts/files'), join(repo, 'site/assets/fonts')];
// serve: only git-tracked decks and fonts (the open ones: nothing local or proprietary leaves the machine through the tunnel)
const gitTracked = SERVE ? new Set(execFileSync('git', ['-C', repo, 'ls-files', '--', 'slides/samples', 'core/crates/uo-pptx/tests/captures/decks', ...FONT_DIRS.map((d) => fwd(d).replace(`${fwd(repo)}/`, ''))], { encoding: 'utf8' })
  .split('\n').map((s) => s.trim()).filter(Boolean).map((p) => fwd(join(repo, p)))) : null;
const STRIDE = Number(process.env.UO_SS_STRIDE ?? 5);
const decks = [...tracked.map((p) => join(repo, p)), ...b1a, ...c1d, ...b5b, ...(MODE === 'all' || MODE === 'anim' || SERVE ? b6a : [])].filter((p) => !only || only.test(fwd(p)))
  .filter((p) => !SERVE || gitTracked.has(fwd(p))).filter((p, i) => !SERVE || i % STRIDE === 0);
const deckByName = new Map(decks.map((p, i) => [`k${i}${extname(p)}`, p]));
const TRACKED_FONTS = SERVE ? new Set([...gitTracked].filter((p) => FONT_DIRS.some((d) => p.startsWith(`${fwd(d)}/`))).map((p) => basename(p))) : null;
const requests = [];
const MIME = { '.js': 'text/javascript', '.wasm': 'application/wasm', '.ttf': 'font/ttf', '.json': 'application/json', '.pptx': 'application/octet-stream', '.ppt': 'application/octet-stream' };
const resultsFile = join(outDir, 'serve-results.jsonl');
const CSP = { strict: "default-src 'self'; script-src 'self'", wasm: "default-src 'self'; script-src 'self' 'wasm-unsafe-eval'", eval: "default-src 'self'; script-src 'self' 'unsafe-eval'" };
const CSP_NEXT = { strict: '/csp?v=wasm', wasm: '/csp?v=eval', eval: '/done' };
const serveState = new Map(); // per user agent: the last `end` record and the CSP answers, shown by /done
const server = createServer((req, res) => {
  const url = decodeURIComponent(req.url.split('?')[0]);
  if (!SERVE) requests.push(url);
  const send = (file) => res.writeHead(200, { 'content-type': MIME[extname(file)] ?? 'application/octet-stream', ...(SERVE ? { 'cache-control': 'no-store' } : {}) }).end(readFileSync(file));
  if (SERVE) {
    const q = new URL(req.url, 'http://x').searchParams;
    const ua = req.headers['user-agent'] ?? '';
    if (!/^\/(out|fonts|decks)\//.test(url)) console.log(new Date().toISOString(), req.method, req.url, ua.slice(0, 120));
    const st = serveState.get(ua) ?? serveState.set(ua, { end: null, csp: {} }).get(ua);
    if (req.method === 'POST' && url === '/result') {
      let body = '';
      req.on('data', (c) => { body += c; if (body.length > 64e6) req.destroy(); });
      req.on('end', () => {
        let o = null;
        try { o = JSON.parse(body); } catch { /* printed raw below */ }
        // ?debug=1: the PNGs of the differing slides (TS, Rust) are written next to the results, not into the JSONL
        const pngs = o?.row?.png ?? o?.row?.firstDiff?.png;
        if (pngs) {
          if (o.row.png) { for (const [i, ...urls] of o.row.png) urls.forEach((u, j) => writeFileSync(join(outDir, `serve-diff-${o.row.name}-s${i}-${j ? 'rust' : 'ts'}.png`), Buffer.from(u.split(',')[1], 'base64'))); delete o.row.png; }
          else { pngs.forEach((u, j) => writeFileSync(join(outDir, `serve-anim-diff-${o.row.name}-${['ts', 'rust', 'ts-from', 'rust-from'][j]}.png`), Buffer.from(u.split(',')[1], 'base64'))); delete o.row.firstDiff.png; }
          body = JSON.stringify(o);
        }
        const line = body.replace(/\n/g, ' ');
        appendFileSync(resultsFile, line + '\n');
        if (o?.kind === 'row') {
          const r = o.row;
          console.log(`deck ${o.i + 1}/${o.of} ${o.deck}: ${r.slides} slides, ${r.slideDiff.length} differ, main ${r.mainDiff}/${r.mainChecked}, thumbs ${r.thumbDiff}, notes ${r.notesDiff}, a11y ${r.a11yDiff}/${r.a11yChecked}, links ${r.linksDiff}/${r.linkProbes}, engine ${r.engine}, tsFree ${r.tsFree}, errors ${r.errors.length}${o.ok ? '' : (o.known ? ` KNOWN (${o.known})` : '') + ' MISMATCH ' + JSON.stringify({ diffs: r.slideDiff.slice(0, 4), errors: r.errors.slice(0, 3), statuses: r.statuses, a11y: r.a11yFirst })} (${Math.round(o.ms)} ms, dpr ${o.dpr})`);
        } else if (o?.kind === 'anim') {
          const r = o.row;
          console.log(`anim ${o.i + 1}/${o.of} ${o.deck}: ${r.frames} frames, ${r.diff} differ, ${r.rust} Rust stage paints, media ${r.mediaDiff}/${r.mediaFrames}, engine ${r.engine}, tsFree ${r.tsFree}, errors ${r.errors.length}${o.ok ? '' : ' MISMATCH ' + JSON.stringify({ first: r.firstDiff, media: r.firstMedia, errors: r.errors.slice(0, 3) })}`);
        } else {
          if (o?.kind === 'end') st.end = o;
          if (o?.kind === 'csp') st.csp[o.v] = o.compile;
          console.log('RESULT', line.slice(0, 800));
        }
        res.writeHead(204).end();
      });
      return;
    }
    if (url === '/') return res.writeHead(200, { 'content-type': 'text/html; charset=utf-8', 'cache-control': 'no-store' }).end(servePage());
    if (url === '/csp' && CSP[q.get('v')]) return res.writeHead(200, { 'content-type': 'text/html; charset=utf-8', 'cache-control': 'no-store', 'content-security-policy': CSP[q.get('v')] }).end(`<!doctype html><meta charset="utf-8"><meta name="viewport" content="width=device-width"><body><div id="status">csp ${q.get('v')}…</div><script src="/csp.js?v=${q.get('v')}&next=${encodeURIComponent(CSP_NEXT[q.get('v')])}"></script></body>`);
    if (url === '/csp.js') return res.writeHead(200, { 'content-type': 'text/javascript', 'cache-control': 'no-store' }).end(cspProbe);
    if (url === '/done') {
      const e = st.end;
      const csp = ['strict', 'wasm', 'eval'].map((v) => `${v}=${String(st.csp[v] ?? 'not run').split(' ')[0]}`).join(' ');
      const text = e ? `done ${e.verdict ?? (e.ok ? 'PASS' : 'FAIL')}: ${e.summary}; csp ${csp}` : `done (no end record from this browser); csp ${csp}`;
      return res.writeHead(200, { 'content-type': 'text/html; charset=utf-8', 'cache-control': 'no-store' }).end(`<!doctype html><meta charset="utf-8"><meta name="viewport" content="width=device-width"><body style="font:24px -apple-system,sans-serif"><div id="status">${text.replace(/[<&]/g, (c) => (c === '<' ? '&lt;' : '&amp;'))}</div></body>`);
    }
  }
  if (url === '/') return res.writeHead(200, { 'content-type': 'text/html' }).end('<!doctype html><meta charset="utf-8"><body style="margin:0"><script type="module">import * as e from "/out/entry.js"; window.__e = e; window.__ready = true;</script></body>');
  if (url.startsWith('/out/')) { const f = join(out, url.slice(5)); return f.startsWith(out) && existsSync(f) ? send(f) : res.writeHead(404).end(); }
  if (url.startsWith('/fonts/')) {
    const name = basename(url);
    // serve ?fontdelay=<ms>: the page's viewers use the base /fonts/d<ms>/ and every font file answers that much later (a slow
    // network: the faces are still loading when the first measurements are asked)
    const delay = SERVE ? Number(/^\/fonts\/d(\d+)\//.exec(url)?.[1] ?? 0) : 0;
    if (/^[\w.-]+\.(ttf|json)$/.test(name) && (!TRACKED_FONTS || TRACKED_FONTS.has(name))) for (const d of FONT_DIRS) if (existsSync(join(d, name))) {
      if (delay && name.endsWith('.ttf')) { setTimeout(() => send(join(d, name)), delay); return; }
      return send(join(d, name));
    }
    return res.writeHead(404).end();
  }
  if (url.startsWith('/decks/')) { const f = deckByName.get(basename(url)); return f ? send(f) : res.writeHead(404).end(); }
  res.writeHead(404).end();
});
await new Promise((r) => server.listen(SERVE ? Number(process.env.UO_SS_PORT ?? 8767) : 0, '127.0.0.1', r));

// ── Chromium ────────────────────────────────────────────────────────────────────────────────────────
const { chromium } = createRequire(join(root, 'slides/package.json'))('playwright');
const installed = (() => {
  const base = join(process.env.LOCALAPPDATA ?? '', 'ms-playwright');
  if (!existsSync(base)) return undefined;
  const dirs = readdirSync(base).filter((d) => d.startsWith('chromium_headless_shell-')).sort();
  const exe = dirs.length && join(base, dirs[dirs.length - 1], 'chrome-headless-shell-win64', 'chrome-headless-shell.exe');
  return exe && existsSync(exe) ? exe : undefined;
})();
const browser = SERVE ? null : await chromium.launch({ headless: true, executablePath: process.env.UO_CHROMIUM ?? installed, args: ['--js-flags=--expose-gc', '--enable-precise-memory-info'] });
const context = SERVE ? null : await browser.newContext({ timezoneId: 'UTC', deviceScaleFactor: 1, viewport: { width: 1280, height: 900 } });
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
if (!SERVE) await openPage();
const names = [...deckByName.keys()];

// the page-side driver (a function evaluated in the page; everything it needs is passed by argument)
const driverFn = () => {
  const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
  const El = window.__e.UniversalSlidesViewerElement;
  const fnv = (u8) => { let h = 0x811c9dc5; for (let i = 0; i < u8.length; i++) h = Math.imul(h ^ u8[i], 0x01000193); return (h >>> 0).toString(16); };
  const mk = (engine) => {
    const el = new El();
    el.setAttribute('fonts', window.__ssFontBase ?? '/fonts');
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
  // each comparison canvas is released after its pixels are read (getImageData copies): Safari's canvas memory budget (iOS) is small
  const release = (cv) => { cv.width = 0; cv.height = 0; };
  const render = (el, i, w) => { const cv = document.createElement('canvas'); el.renderSlideToCanvas(i, cv, w); const d = pixels(cv); release(cv); return d; };
  const thumb = (el, i) => { const cv = document.createElement('canvas'); cv.style.cssText = 'width:200px;height:112px'; document.body.appendChild(cv); el.renderThumbnail(i, cv); const d = pixels(cv); cv.remove(); release(cv); return d; };
  const mainCanvas = (el) => el.canvas;

  /** TS element against Rust element on one deck */
  const compare = async (name) => {
    const ts = mk('ts'), rs = mk(window.__ssTsTs ? 'ts' : 'rust'); // serve ?tsts=1: a control, TS against TS
    const r = { name, slides: 0, engine: 'ts', slideDiff: [], mainDiff: 0, mainChecked: 0, thumbDiff: 0, notesDiff: 0, a11yDiff: 0, a11yChecked: 0, linksDiff: 0, linkProbes: 0, linkHits: 0, linkExtra: 0, tsFree: false, tsLoads: 0, charts: 0, pixels: 0, errors: [], painted: [0, 0] };
    try {
      const errs = [];
      rs.addEventListener('us-engine-error', (e) => errs.push(String(e.detail?.error?.message ?? e.detail?.error)));
      window.__step = `${name} load ts`;
      await load(ts, name);
      window.__step = `${name} load rust`;
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
        window.__step = `${name} slide ${i}`;
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
          // serve: after a font change each viewer measures again and repaints its main canvas later (`remeasureLater`: two frames,
          // then 1.1 s); read both after those repaints, not one element before and the other after
          if (window.__ssLiveSettle) {
            const t0 = performance.now();
            const pending = (el) => el.remeasureFrame || el.remeasureTimer;
            while ((pending(ts) || pending(rs)) && performance.now() - t0 < 1500) await sleep(20);
          }
          const ma = pixels(mainCanvas(ts)), mb = pixels(mainCanvas(rs));
          r.mainChecked++;
          if (diff(ma, mb) !== 0) {
            r.mainDiff++;
            if (window.__ssDebug) (r.png ??= []).push([`main${i}`, mainCanvas(ts).toDataURL(), mainCanvas(rs).toDataURL()]);
          }
        }
        if (ts.getSlideNotes(i) !== rs.getSlideNotes(i)) r.notesDiff++;
        // the accessible text of the slide (heading, paragraphs, tables, alternative texts) and the link hit-tests on it
        ts.selectSlide(i); rs.selectSlide(i);
        // serve: the live region's announcement lands 60 ms after each selectSlide (LiveRegion.announce, a timer); on a slow device
        // one element's timer has fired and the other's not when the text is read. 80 ms after both, both have (timers in deadline order)
        if (window.__ssLiveSettle) await sleep(80);
        const a11y = (el) => (el.shadowRoot?.textContent ?? '') + '|' + (el.canvas.getAttribute('aria-label') ?? '');
        r.a11yChecked++;
        if (a11y(ts) !== a11y(rs)) {
          r.a11yDiff++;
          if (!r.a11yFirst) { // the first difference, around the first differing character
            const x = a11y(ts), y = a11y(rs);
            let k = 0;
            while (k < x.length && x[k] === y[k]) k++;
            r.a11yFirst = { i, at: k, ts: x.slice(Math.max(0, k - 80), k + 120), rust: y.slice(Math.max(0, k - 80), k + 120) };
          }
        }
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
      window.__step = `${name} anim load`;
      await load(ts, name);
      await load(rs, name);
      window.__step = `${name} anim frames`;
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
// ── serve: the page runs the driver itself (real Safari) ─────────────────────────────────────────────
function servePage() {
  const labels = Object.fromEntries(names.map((n) => [n, relative(deckByName.get(n))]));
  // the animated decks: round-robin over the playback samples, the b6a decks and the b1a decks of the stride
  const groups = [/\/samples\/playback\//, /\/b6a\//, /\/b1a\//].map((re) => names.filter((n) => re.test(fwd(deckByName.get(n)))));
  const animNames = [];
  for (let k = 0; animNames.length < Number(process.env.UO_SS_ANIM ?? 6) && groups.some((g) => g.length > k); k++) for (const g of groups) if (g[k] && animNames.length < Number(process.env.UO_SS_ANIM ?? 6)) animNames.push(g[k]);
  return `<!doctype html><meta charset="utf-8"><meta name="viewport" content="width=1280">
<title>Slides viewer TS/Rust switch</title>
<body style="margin:0"><div id="status" style="position:fixed;right:0;bottom:0;z-index:10;background:#fff;color:#000;font:24px -apple-system,sans-serif;padding:8px;border:1px solid #888">running…</div>
<pre id="log" style="margin:740px 0 0;font-size:11px;white-space:pre-wrap;padding:8px"></pre>
<script>
// ?nooffscreen=1: hide OffscreenCanvas before the bundle loads (a Safari < 16.4 in a browser that has it: the TS metafile, EMF+ and
// PICT players draw embedded bitmaps and pattern brushes only through OffscreenCanvas, the Rust path falls back to a DOM canvas)
if (/[?&]nooffscreen=1/.test(location.search)) window.OffscreenCanvas = undefined;
// ?fontlag=<ms>: a face added to document.fonts reaches canvas text only that much later, as real Safari does it (at a later rendering
// update): a measurement asked in the same task as the add still gets the fallback face
(function () { var m = /[?&]fontlag=(\\d+)/.exec(location.search); if (!m || typeof FontFaceSet === 'undefined') return; var add = FontFaceSet.prototype.add, ms = Number(m[1]);
  FontFaceSet.prototype.add = function (f) { var set = this; setTimeout(function () { add.call(set, f); }, ms); return this; }; })();
// the fixed clock (date/time fields must not differ between the two loads); the viewers' own timing uses performance.now()
(function () { var N = ${NOW}, RD = Date; var FD = function (y, m, d, h, mi, s, ms) { if (!(this instanceof FD)) return new RD(N).toString(); var a = arguments.length === 0 ? [N] : Array.prototype.slice.call(arguments); return new (Function.prototype.bind.apply(RD, [null].concat(a)))(); }; FD.prototype = RD.prototype; FD.now = function () { return N; }; FD.UTC = RD.UTC; FD.parse = RD.parse; window.Date = FD; })();
var post = function (o) { o.ua = navigator.userAgent; return fetch('/result', { method: 'POST', body: JSON.stringify(o) }).catch(function () {}); };
window.onerror = function (m, src, l, c) { post({ kind: 'fatal', error: String(m) + ' ' + src + ':' + l + ':' + c }); };
window.addEventListener('unhandledrejection', function (e) { post({ kind: 'fatal', error: 'unhandled ' + String(e.reason && e.reason.stack || e.reason) }); });
</script>
<script type="module">
import * as e from '/out/entry.js';
window.__e = e;
const api = (${driverFn.toString()})();
const qs = new URLSearchParams(location.search);
// ?decks=k3.pptx,k9.ppt: compare (and animate) only those decks; ?debug=1: the PNGs of the differing slides go to the server
const allNames = ${JSON.stringify(names)}, labels = ${JSON.stringify(labels)}, SOAK = ${SOAK};
const subset = qs.get('decks') && !qs.get('solo') ? qs.get('decks').split(',') : null;
const names = subset ? allNames.filter((n) => subset.includes(n)) : allNames;
const animNames = ${JSON.stringify(animNames)}.filter((n) => !subset || subset.includes(n));
if (qs.has('debug')) window.__ssDebug = true;
window.__ssLiveSettle = true;
if (qs.get('tsts') === '1') window.__ssTsTs = true;
// ?fontdelay=<ms>: every font file answers that much later (the server reads it from the base /fonts/d<ms>/)
if (Number(qs.get('fontdelay')) > 0) window.__ssFontBase = '/fonts/d' + Number(qs.get('fontdelay'));
// ?fresh=1 (or another run URL than the stored one): a NEW run, the saved state of a previous run is dropped; \`fresh\` then leaves the
// address, so a reload after a tab crash resumes this run instead of starting over
{
  const fresh = qs.get('fresh') === '1';
  if (qs.has('fresh')) { qs.delete('fresh'); history.replaceState(null, '', location.pathname + (qs.toString() ? '?' + qs.toString() : '')); }
  const stored = sessionStorage.getItem('uo-ss-url');
  if (fresh || (stored !== null && stored !== location.href)) for (const k of Object.keys(sessionStorage)) if (k.startsWith('uo-ss-')) sessionStorage.removeItem(k);
  sessionStorage.setItem('uo-ss-url', location.href);
}
const show = (t) => { document.getElementById('status').textContent = t; };
const log = (t) => { document.getElementById('log').textContent += t + String.fromCharCode(10); };
const dpr = devicePixelRatio;
const nextFrame = () => new Promise((r) => requestAnimationFrame(() => r()));
// /?solo=ts|rust&decks=k3.pptx,k9.ppt: ONE viewer of one engine per deck (no comparison), to tell an engine's memory from the
// harness's two viewers side by side; each deck opens, settles, shows and renders every slide, and closes
if (qs.get('solo')) {
  const engine = qs.get('solo');
  for (const n of (qs.get('decks') ?? qs.get('docs') ?? '').split(',').filter(Boolean)) {
    const key = 'uo-ss-solo-' + engine + '-' + n;
    if (sessionStorage.getItem(key)) { await post({ kind: 'solo', engine, n, deck: labels[n], crashed: true }); continue; }
    sessionStorage.setItem(key, '1');
    show('solo ' + engine + ' ' + n);
    const t0 = performance.now();
    let err = null, slides = 0, got = null;
    const el = api.mk(engine);
    try {
      await api.load(el, n);
      got = el.getEngine();
      slides = el.getSlideCount();
      for (let i = 0; i < slides; i++) { el.selectSlide(i); await nextFrame(); api.render(el, i, 1280); }
    } catch (e2) { err = String(e2 && e2.stack || e2); }
    el.remove();
    await post({ kind: 'solo', engine, n, deck: labels[n], slides, ms: performance.now() - t0, err, opened: got, dpr });
    await api.sleep(500);
  }
  await post({ kind: 'solo-end', engine });
  show('done solo ' + engine);
  await new Promise(() => {}); // stop here: the comparison below does not run
}
// feature detection
const sim = new Uint8Array([0,97,115,109,1,0,0,0,1,4,1,96,0,0,3,2,1,0,10,23,1,21,0,253,12,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,26,11]);
const c2d = CanvasRenderingContext2D.prototype;
const features = { simd: WebAssembly.validate(sim), offscreenCanvas: typeof OffscreenCanvas, createImageBitmap: typeof createImageBitmap, dpr,
  filter: 'filter' in c2d, roundRect: 'roundRect' in c2d, letterSpacing: 'letterSpacing' in c2d, fontKerning: 'fontKerning' in c2d, reset: 'reset' in c2d,
  finalizationRegistry: typeof FinalizationRegistry, viewport: [innerWidth, innerHeight], tz: Intl.DateTimeFormat().resolvedOptions().timeZone,
  readback: (() => {
    // does the browser alter canvas readbacks (Safari's fingerprinting protection adds noise)? The same drawing read twice, read on a
    // twin canvas drawn by the same code, and copied with drawImage: any difference is the browser's, not an engine's
    try {
      const draw = () => { const cv = document.createElement('canvas'); cv.width = 96; cv.height = 32; const c = cv.getContext('2d'); c.fillStyle = '#f60'; c.fillRect(0, 0, 96, 32);
        c.fillStyle = '#069'; c.font = '14px sans-serif'; c.fillText('Wq0 ação', 2, 20); c.strokeStyle = '#0a0'; c.beginPath(); c.arc(80, 16, 10, 0, 6.28); c.stroke(); return cv; };
      const a = draw(), b = draw(), k = document.createElement('canvas'); k.width = 96; k.height = 32; k.getContext('2d').drawImage(a, 0, 0);
      const px = (cv) => cv.getContext('2d').getImageData(0, 0, 96, 32).data;
      const d = (x, y) => { let n = 0; for (let i = 0; i < x.length; i++) if (x[i] !== y[i]) n++; return n; };
      const pa = px(a), r = { repeat: d(pa, px(a)), twin: d(pa, px(b)), copy: d(pa, px(k)) };
      for (const cv of [a, b, k]) { cv.width = 0; cv.height = 0; }
      return r;
    } catch (err) { return String(err); }
  })() };
// a tab that the OS kills (iOS memory) reloads the page: resume after the deck that was running, recorded as a crash with its last
// step (window.__step of the driver, mirrored to sessionStorage and to the server as it changes); the totals live in sessionStorage
const zero = { decks: 0, rust: 0, slides: 0, diffSlides: 0, pixels: 0, mainChecked: 0, mainDiff: 0, thumbDiff: 0, notesDiff: 0, a11yChecked: 0, a11yDiff: 0,
  linkProbes: 0, linkHits: 0, linksDiff: 0, linkExtra: 0, tsFree: 0, tsLoads: 0, errors: 0, badDecks: 0, animDecks: 0, animFrames: 0, animDiff: 0, animRust: 0,
  mediaDiff: 0, animBad: 0, crashes: 0, best: null, soak: null, knownDecks: 0, knownSlides: 0 };
let tot = JSON.parse(sessionStorage.getItem('uo-ss-tot') ?? 'null') ?? zero;
const save = () => sessionStorage.setItem('uo-ss-tot', JSON.stringify(tot));
let phase = 'compare', from = 0;
const at = sessionStorage.getItem('uo-ss-at');
if (at === null) { tot = { ...zero }; save(); await post({ kind: 'start', ...features, nooffscreen: qs.get('nooffscreen') === '1', tsts: qs.get('tsts') === '1', fontdelay: Number(qs.get('fontdelay')) || 0, fontlag: Number(qs.get('fontlag')) || 0, decks: names.length, anim: animNames.length, soak: SOAK, tsStrict: true }); }
else {
  const step = sessionStorage.getItem('uo-ss-step');
  const [ph, k] = at.split(':');
  await post({ kind: 'crash', at, deck: k !== undefined ? labels[(ph === 'a' ? animNames : names)[Number(k)]] : null, step, ...features });
  tot.crashes++; save();
  if (ph === 'c') { phase = 'compare'; from = Number(k) + 1; } else if (ph === 'a') { phase = 'anim'; from = Number(k) + 1; } else phase = 'end';
}
let lastStep = null;
setInterval(() => { if (window.__step !== lastStep) { lastStep = window.__step; sessionStorage.setItem('uo-ss-step', String(lastStep)); post({ kind: 'step', step: lastStep }); } }, 250);
const progress = () => tot.decks + '/' + names.length + ' decks, ' + tot.slides + ' slides, ' + tot.badDecks + ' differ' + (tot.crashes ? ', ' + tot.crashes + ' crashes' : '');
if (phase === 'compare') {
  for (let i = from; i < names.length; i++) {
    sessionStorage.setItem('uo-ss-at', 'c:' + i);
    show('compare ' + (i + 1) + '/' + names.length + ' · ' + progress());
    const t0 = performance.now();
    let r;
    try { r = await api.compare(names[i]); } catch (err) { r = { name: names[i], slides: 0, engine: '?', slideDiff: [], errors: ['exception ' + String(err && err.stack || err)], painted: [0, 0] }; }
    const ok = !(r.slideDiff.length || r.errors.length || r.engine !== 'rust' || r.mainDiff || r.thumbDiff || r.notesDiff || r.a11yDiff || r.linksDiff || !r.tsFree || r.tsLoads);
    tot.decks++; if (r.engine === 'rust') tot.rust++;
    for (const f of ['slides', 'pixels', 'mainChecked', 'mainDiff', 'thumbDiff', 'notesDiff', 'a11yChecked', 'a11yDiff', 'linkProbes', 'linkHits', 'linksDiff', 'linkExtra', 'tsLoads']) tot[f] += r[f] ?? 0;
    // a known difference, not an engine finding: without OffscreenCanvas the TS metafile/EMF+/PICT players skip embedded bitmaps
    // and pattern brushes (b5b decks), the Rust path draws them; only pixels may differ (main canvas and thumbnails follow them)
    const known = !ok && typeof OffscreenCanvas === 'undefined' && /[/]b5b[/]/.test(labels[names[i]]) && !r.errors.length && r.engine === 'rust'
      && !r.notesDiff && !r.a11yDiff && !r.linksDiff && r.tsFree && !r.tsLoads ? 'ts-metafile-needs-offscreencanvas' : null;
    tot.diffSlides += r.slideDiff.length; tot.errors += r.errors.length; if (r.tsFree) tot.tsFree++; if (!ok) tot.badDecks++;
    if (known) { tot.knownDecks++; tot.knownSlides += r.slideDiff.length; }
    if (/[.]pptx$/.test(names[i]) && r.engine === 'rust' && (!tot.best || r.slides > tot.best[1])) tot.best = [names[i], r.slides];
    save();
    log(labels[names[i]] + ' ' + r.slides + ' slides ' + (ok ? 'ok' : 'MISMATCH ' + JSON.stringify({ d: r.slideDiff.slice(0, 3), e: r.errors.slice(0, 2) })));
    await post({ kind: 'row', i, of: names.length, deck: labels[names[i]], ok, known, ms: performance.now() - t0, dpr, row: r });
  }
  phase = 'anim'; from = 0;
}
if (phase === 'anim') {
  for (let i = from; i < animNames.length; i++) {
    sessionStorage.setItem('uo-ss-at', 'a:' + i);
    show('anim ' + (i + 1) + '/' + animNames.length + ' · ' + progress());
    let r;
    try { r = await api.animate(animNames[i], 8); } catch (err) { r = { name: animNames[i], engine: '?', frames: 0, diff: 0, rust: 0, mediaDiff: 0, errors: ['exception ' + String(err && err.stack || err)] }; }
    const ok = !(r.diff || r.errors.length || r.engine !== 'rust' || r.mediaDiff || !r.tsFree);
    tot.animDecks++; tot.animFrames += r.frames; tot.animDiff += r.diff; tot.animRust += r.rust; tot.mediaDiff += r.mediaDiff ?? 0; if (!ok) tot.animBad++;
    save();
    log('anim ' + labels[animNames[i]] + ' ' + r.frames + ' frames ' + (ok ? 'ok' : 'MISMATCH ' + JSON.stringify({ first: r.firstDiff, e: r.errors.slice(0, 2) })));
    await post({ kind: 'anim', i, of: animNames.length, deck: labels[animNames[i]], ok, dpr, row: r });
  }
  phase = 'soak';
}
if (phase === 'soak') {
  // the soak: slide changes and exports of the largest deck of the stride with engine="rust" (count build), module live bytes and
  // handles sampled; the element is removed at the end (handles back to 0)
  sessionStorage.setItem('uo-ss-at', 'soak');
  window.__step = 'soak';
  show('soak · ' + progress());
  let soak;
  try {
    const st = await e.engineModule();
    const n = tot.best ? tot.best[0] : names.find((x) => /[.]pptx$/.test(x));
    const el = api.mk('rust');
    await api.load(el, n);
    const count = el.getSlideCount();
    const cv = document.createElement('canvas');
    const mem = () => ({ live: st.slidesLiveBytes(), buffer: st.slidesMemoryBytes(), handles: st.slidesHandles() });
    for (let i = 0; i < 10; i++) { el.selectSlide(i % count); el.renderSlideToCanvas(i % count, cv, 640); }
    const samples = [mem()];
    const t0 = performance.now();
    for (let i = 0; i < SOAK; i++) {
      el.selectSlide((i * 7) % count);
      el.renderSlideToCanvas(i % count, cv, 640 + (i % 5));
      if (i % 20 === 19) { await api.sleep(30); samples.push(mem()); }
    }
    const ms = performance.now() - t0;
    cv.width = 0; cv.height = 0;
    const engine = el.getEngine();
    el.remove();
    await api.sleep(200);
    const lives = samples.map((s) => s.live);
    const spread = Math.max(...lives) - Math.min(...lives);
    const after = mem();
    soak = { deck: labels[n], slides: count, changes: SOAK, ms, engine, samples, spread, handlesAfter: after.handles, liveAfter: after.live,
      ok: engine === 'rust' && spread <= 4 * 1024 * 1024 && after.handles === 0 };
  } catch (err) { soak = { ok: false, error: String(err && err.stack || err) }; }
  tot.soak = soak; save();
  await post({ kind: 'soak', ...soak });
  phase = 'end';
}
sessionStorage.removeItem('uo-ss-at');
window.__step = 'end';
const ok = tot.badDecks === 0 && tot.animBad === 0 && tot.crashes === 0 && !!tot.soak?.ok && tot.decks === names.length;
const okExceptKnown = !ok && tot.badDecks === tot.knownDecks && tot.animBad === 0 && tot.crashes === 0 && !!tot.soak?.ok && tot.decks === names.length;
const verdict = ok ? 'PASS' : okExceptKnown ? 'PASS-EXCEPT-KNOWN' : 'FAIL';
const summary = tot.decks + '/' + names.length + ' decks (' + tot.rust + ' in Rust), ' + tot.slides + ' slides, ' + tot.diffSlides + ' slides differ, '
  + tot.badDecks + ' decks with a difference' + (tot.knownDecks ? ' (' + tot.knownDecks + ' decks / ' + tot.knownSlides + ' slides known: TS metafile player without OffscreenCanvas)' : '') + '; main ' + tot.mainDiff + '/' + tot.mainChecked + ', thumbs ' + tot.thumbDiff + ', notes ' + tot.notesDiff
  + ', a11y ' + tot.a11yDiff + '/' + tot.a11yChecked + ', links ' + tot.linksDiff + '/' + tot.linkProbes + '; anim ' + tot.animDecks + ' decks '
  + tot.animFrames + ' frames ' + tot.animDiff + ' differ, media ' + tot.mediaDiff + '; soak ' + (tot.soak?.ok ? 'ok' : 'FAIL') + ' (spread '
  + tot.soak?.spread + ' B, handles ' + tot.soak?.handlesAfter + '); crashes ' + tot.crashes + '; dpr ' + dpr;
await post({ kind: 'end', ok, verdict, summary, totals: tot, dpr, features });
sessionStorage.removeItem('uo-ss-tot');
show('finished ' + verdict + ': ' + summary + (qs.has('nocsp') ? '' : ' (CSP probe next)'));
if (!qs.has('nocsp')) setTimeout(() => { location.href = '/csp?v=strict'; }, 1500);
</script></body>`;
}
const cspProbe = `(function () {
  var p = new URLSearchParams(document.currentScript.src.split('?')[1]);
  var v = p.get('v'), next = p.get('next');
  var bytes = new Uint8Array([0,97,115,109,1,0,0,0,1,5,1,96,0,1,127,3,2,1,0,7,5,1,1,102,0,0,10,6,1,4,0,65,42,11]);
  var done = function (o) { o.kind = 'csp'; o.v = v; o.ua = navigator.userAgent; document.getElementById('status').textContent = 'csp ' + v + ': ' + JSON.stringify(o);
    fetch('/result', { method: 'POST', body: JSON.stringify(o) }).catch(function () {}).then(function () { setTimeout(function () { location.href = next; }, 800); }); };
  var violations = [];
  document.addEventListener('securitypolicyviolation', function (e) { violations.push(e.violatedDirective + ' ' + e.blockedURI); });
  var r = {};
  try { r.validate = WebAssembly.validate(bytes); } catch (e) { r.validate = 'threw ' + e; }
  WebAssembly.compile(bytes).then(function (m) { return WebAssembly.instantiate(m); })
    .then(function (i) { r.compile = 'ok ' + i.exports.f(); }, function (e) { r.compile = 'refused ' + e; })
    .then(function () { return WebAssembly.instantiate(bytes).then(function (x) { r.instantiateBytes = 'ok ' + x.instance.exports.f(); }, function (e) { r.instantiateBytes = 'refused ' + e; }); })
    .then(function () { r.violations = violations; done(r); });
})();
`;
if (SERVE) {
  console.log(`serve: http://127.0.0.1:${server.address().port}/ (${names.length} decks, stride ${STRIDE}; results in ${resultsFile}); Ctrl+C to stop`);
  await new Promise(() => {});
}
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
