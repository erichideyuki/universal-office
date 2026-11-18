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
//           canvas readback-noise probe (`readback`) and probes of the JS engine (`clamp`/`clampHot`: Uint8ClampedArray stores cold
//           and hot, `fma`, a `pow` fingerprint, `spell`: equal canvas calls spelled differently). Second pass (2026-11-18): a slide
//           paints once on both elements and the faces in flight settle before the compared renders; a differing picture loads a
//           Rust twin (3rd element) and a TS twin (4th) and is `same-engine-variance` when TS and Rust disagree only at pixels where
//           an engine disagrees with itself (`?twin=0` turns it off); an animated deck that differs runs a second pass with both
//           twins; `/?v8math=1` replaces the page's Math with V8's (uo_jsnum::math of the jsnum-memtest build, UO_JSMATH_PATH) and
//           serves the bundle with `**` lowered; `/?trace=k16.pptx:0,…` POSTs the 2D-context calls of those renders
//           (`?tracemode=raw|alone`); `/?nofeat=letterSpacing,…` / `?safari15=1` remove canvas members; `/?elw=640` smaller
//           elements (iOS canvas memory); a deck whose transfer is broken is fetched again. The verdict is PASS, FAIL, or
//           PASS-EXCEPT-KNOWN: without
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
// serve `?v8math=1`: the same bundle with `a ** b` lowered to Math.pow (esbuild `supported`), so the page's Math can be replaced by
// V8's (UO_JSMATH_PATH: the `jsnum-memtest` build of uo-wasm, whose `uo_jsnum_memtest_call` is uo_jsnum::math = V8's fdlibm/pow)
const outV8 = join(outDir, 'out-v8');
const jsmathPath = process.env.UO_JSMATH_PATH ?? join(outDir, '..', 'uo-jsmath.wasm');
if (SERVE) {
  rmSync(outV8, { recursive: true, force: true });
  await esbuild.build({
    entryPoints: { entry: join(outDir, 'entry.mjs'), stats: join(outDir, 'stats.mjs') },
    outdir: outV8, bundle: true, format: 'esm', platform: 'browser', target: 'es2022', splitting: true, chunkNames: 'chunks/[name]-[hash]',
    minify: true, legalComments: 'none', alias, absWorkingDir: repo, external: ['pdfjs-dist', 'pdfjs-dist/*'], supported: { 'exponent-operator': false },
    nodePaths: [join(root, 'write/node_modules'), join(root, 'slides/node_modules'), join(root, 'sheet/node_modules')], logLevel: 'warning',
  });
  copyFileSync(wasmPath, join(outV8, 'chunks', 'uo-slides.wasm'));
  if (existsSync(pptPath)) copyFileSync(pptPath, join(outV8, 'chunks', 'uo-slides-ppt.wasm'));
}

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
        if (o?.kind === 'trace') {
          const tag = ((m) => (m ? (m[2] ? 'ios' : 'saf') + m[1] : /Chrome\//.test(ua) ? 'chr' : 'wk'))(/Version\/([\d.]+)( Mobile)?/.exec(ua));
          for (const l of o.logs ?? []) if (l.png) { writeFileSync(join(outDir, `trace-${tag}-${o.name}-s${o.slide}-${l.engine}.png`), Buffer.from(l.png.split(',')[1], 'base64')); delete l.png; }
          writeFileSync(join(outDir, `trace-${tag}-${o.name}-s${o.slide}.json`), JSON.stringify(o));
          console.log(`trace ${tag} ${o.deck} s${o.slide}: ${o.error ?? o.logs.map((l) => `${l.engine} ${l.log.length} calls px ${l.px}`).join(' / ')}`);
          return res.writeHead(204).end();
        }
        const pngs = o?.row?.png ?? o?.row?.firstDiff?.png;
        if (pngs) {
          // the browser in the name (two devices may run the same decks at once): saf<version>, ios<version>, chr, wk
          const tag = ((m) => (m ? (m[2] ? 'ios' : 'saf') + m[1] : /Chrome\//.test(ua) ? 'chr' : 'wk'))(/Version\/([\d.]+)( Mobile)?/.exec(ua));
          if (o.row.png) { for (const [i, ...urls] of o.row.png) urls.forEach((u, j) => writeFileSync(join(outDir, `serve-diff-${tag}-${o.row.name}-s${i}-${j ? 'rust' : 'ts'}.png`), Buffer.from(u.split(',')[1], 'base64'))); delete o.row.png; }
          else { pngs.forEach((u, j) => writeFileSync(join(outDir, `serve-anim-diff-${tag}-${o.row.name}-${['ts', 'rust', 'ts-from', 'rust-from'][j]}.png`), Buffer.from(u.split(',')[1], 'base64'))); delete o.row.firstDiff.png; }
          body = JSON.stringify(o);
        }
        const line = body.replace(/\n/g, ' ');
        appendFileSync(resultsFile, line + '\n');
        if (o?.kind === 'row') {
          const r = o.row;
          console.log(`deck ${o.i + 1}/${o.of} ${o.deck}: ${r.slides} slides, ${r.slideDiff.length} differ, main ${r.mainDiff}/${r.mainChecked}, thumbs ${r.thumbDiff}, notes ${r.notesDiff}, a11y ${r.a11yDiff}/${r.a11yChecked}, links ${r.linksDiff}/${r.linkProbes}, engine ${r.engine}, tsFree ${r.tsFree}, errors ${r.errors.length}${o.ok ? '' : (o.known ? ` KNOWN (${o.known})` : '') + ' MISMATCH ' + JSON.stringify({ diffs: r.slideDiff.slice(0, 4), errors: r.errors.slice(0, 3), statuses: r.statuses, a11y: r.a11yFirst })} (${Math.round(o.ms)} ms, dpr ${o.dpr})`);
        } else if (o?.kind === 'anim') {
          const r = o.row;
          console.log(`anim ${o.i + 1}/${o.of} ${o.deck}: ${r.frames} frames, ${r.diff} differ, ${r.rust} Rust stage paints, media ${r.mediaDiff}/${r.mediaFrames}, engine ${r.engine}, tsFree ${r.tsFree}, errors ${r.errors.length}${o.ok ? '' : (o.known ? ` KNOWN (${o.known}, ${r.noise} noise frames; first pass ${r.firstPass?.diff})` : '') + ' MISMATCH ' + JSON.stringify({ first: r.firstDiff, media: r.firstMedia, errors: r.errors.slice(0, 3) })}`);
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
  if (SERVE && url.startsWith('/out-v8/')) { const f = join(outV8, url.slice(8)); return f.startsWith(outV8) && existsSync(f) ? send(f) : res.writeHead(404).end(); }
  if (SERVE && url === '/jsmath.wasm') return existsSync(jsmathPath) ? send(jsmathPath) : res.writeHead(404).end();
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
    // serve ?elw=<css px>: smaller elements (a phone at dpr 3 holds up to four viewers of 3840 x 2160 device px otherwise)
    const w = window.__ssElW ?? 1280;
    el.style.cssText = `display:block;width:${w}px;height:${Math.round((w * 9) / 16)}px;position:absolute;left:0;top:0`;
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
    // a deck is a ZIP (PK) or a Compound File (D0 CF): anything else is a broken transfer (the serve tunnel), fetched again (3 tries)
    let bytes = null;
    for (let k = 0; k < 3; k++) {
      try {
        const res = await fetch(`/decks/${name}`);
        const u8 = new Uint8Array(await res.arrayBuffer());
        const len = Number(res.headers.get('content-length') ?? u8.length);
        if (res.ok && u8.length === len && ((u8[0] === 0x50 && u8[1] === 0x4b) || (u8[0] === 0xd0 && u8[1] === 0xcf))) { bytes = u8; break; }
      } catch { /* retried */ }
      await sleep(500);
    }
    if (!bytes) throw new Error(`deck ${name}: transfer failed 3 times`);
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
  /** serve: until no face is loading (the page's font set, the shared resolver) and no viewer has a font repaint pending (≤ 3 s) */
  const fontsIdle = async (els) => {
    const t0 = performance.now();
    const busy = () => document.fonts?.status === 'loading' || els.some((el) => el.fontResolver?.pending?.size || el.fontResolver?.pendingFile?.size || el.remeasureFrame || el.remeasureTimer);
    while (busy() && performance.now() - t0 < 3000) await sleep(20);
  };
  const compare = async (name) => {
    const ts = mk('ts'), rs = mk(window.__ssTsTs ? 'ts' : 'rust'); // serve ?tsts=1: a control, TS against TS
    // serve: once a picture differs, a third element (TS) and a fourth (Rust) are loaded and paint the same picture. The difference
    // is the browser's (counted apart: `noise` on the slide, `mainNoise`, `thumbNoise`) when one engine disagrees with ITSELF across
    // elements (TS against TS, or Rust against Rust) while a TS picture and a Rust picture are equal bit for bit: on Safari an
    // element's pictures can differ from another's of the same engine (measured with ?tsts=1)
    let twin = null, twin2 = null;
    const twinEl = async () => { if (!twin) { twin = mk('ts'); await load(twin, name); } return twin; };
    const rustEl = async () => { if (!twin2) { twin2 = mk(window.__ssTsTs ? 'ts' : 'rust'); await load(twin2, name); } return twin2; };
    /**
     * pic(el) paints the picture in question on an element; a = the TS element's, b = the Rust element's (they differ). t = a TS twin's,
     * q = a Rust twin's. `unexplained` = the pixels where TS and Rust disagree although each engine agrees with itself there
     * (a = t and b = q); the difference is the browser's when that is 0 and an engine disagrees with itself somewhere
     */
    const classify = async (pic, a, b) => {
      // the Rust twin is the third element and the TS twin the fourth: on Safari an element's pictures can depend on its place in
      // the page's order (measured: the 2nd element differs from the 1st and the 3rd), so each engine gets an odd and an even place
      const r2 = await rustEl();
      await pic(r2); await fontsIdle([r2]);
      const q = await pic(r2);
      const tw = await twinEl();
      await pic(tw); await fontsIdle([tw]);
      const t = await pic(tw);
      const o = { twin: diff(t, b), tsts: diff(a, t), rr: diff(b, q), rust2: diff(a, q), unexplained: 0 };
      if (a.length === b.length && a.length === t.length && a.length === q.length) {
        const ne = (x, y, k) => x[k] !== y[k] || x[k + 1] !== y[k + 1] || x[k + 2] !== y[k + 2] || x[k + 3] !== y[k + 3];
        for (let k = 0; k < a.length; k += 4) if (ne(a, b, k) && !ne(a, t, k) && !ne(b, q, k)) o.unexplained++;
      } else o.unexplained = -1;
      o.noise = o.unexplained === 0 && (o.tsts > 0 || o.rr > 0);
      return o;
    };
    const mainPic = (i) => async (el) => {
      el.selectSlide(i);
      await new Promise((rr) => requestAnimationFrame(rr));
      const t0 = performance.now();
      while ((el.remeasureFrame || el.remeasureTimer) && performance.now() - t0 < 1500) await sleep(20);
      return pixels(mainCanvas(el));
    };
    const r = { name, slides: 0, engine: 'ts', slideDiff: [], mainDiff: 0, mainNoise: 0, mainChecked: 0, thumbDiff: 0, thumbNoise: 0, notesDiff: 0, a11yDiff: 0, a11yChecked: 0, linksDiff: 0, linkProbes: 0, linkHits: 0, linkExtra: 0, tsFree: false, tsLoads: 0, charts: 0, pixels: 0, errors: [], painted: [0, 0] };
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
        // serve: a slide's first paint may ask for a face (a bold or italic variant fetched on first use); on a slow network or on
        // Safari (the face reaches canvas text a rendering update later) the next render of the other element would see another font
        // state. Both elements paint the slide once, then the faces in flight and the viewers' font repaints settle, then the renders
        // that are compared (both engines at the same font state)
        if (window.__ssLiveSettle) { render(ts, i, 1280); render(rs, i, 1280); await fontsIdle([ts, rs]); }
        const a = render(ts, i, 1280), b = render(rs, i, 1280);
        const d = diff(a, b);
        r.pixels += a.length / 4;
        if (d !== 0) {
          const e = { i, d, chart: hasChart };
          if (window.__ssTwin) Object.assign(e, await classify(async (el) => render(el, i, 1280), a, b));
          r.slideDiff.push(e);
        }
        if (d !== 0 && window.__ssDebug) {
          const url = (el) => { const cv = document.createElement('canvas'); el.renderSlideToCanvas(i, cv, 1280); return cv.toDataURL(); };
          (r.png ??= []).push([i, url(ts), url(rs)]);
        }
        if (i < 6) {
          const ta = thumb(ts, i), tb = thumb(rs, i);
          if (diff(ta, tb) !== 0) {
            r.thumbDiff++;
            if (window.__ssTwin && (await classify(async (el) => thumb(el, i), ta, tb)).noise) r.thumbNoise++;
          }
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
            if (window.__ssTwin && (await classify(mainPic(i), ma, mb)).noise) r.mainNoise++;
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
      ts.remove(); rs.remove(); twin?.remove(); twin2?.remove();
    }
    return r;
  };
  /** B4b: TS element against Rust element on the animated frames of one deck, under a virtual clock */
  const animate = async (name, steps, withTwin = false) => {
    const ts = mk('ts'), rs = mk(window.__ssTsTs ? 'ts' : 'rust');
    // serve, a second pass after a difference: a TS twin and a Rust twin painted with the other two; a differing frame is the
    // browser's (`noise`) when TS and Rust disagree only at pixels where an engine also disagrees with itself (see `classify`)
    const rw = withTwin ? mk(window.__ssTsTs ? 'ts' : 'rust') : null, tw = withTwin ? mk('ts') : null; // places 3 (Rust) and 4 (TS), see `classify`
    const els = tw ? [ts, rs, rw, tw] : [ts, rs];
    let clock = 0;
    for (const el of els) el.now = () => clock;
    const r = { name, engine: 'ts', frames: 0, diff: 0, noise: 0, twin: withTwin, rust: 0, firstDiff: null, errors: [], mediaFrames: 0, mediaReq: 0, mediaDiff: 0, firstMedia: null, tsFree: false };
    try {
      const errs = [];
      rs.addEventListener('us-engine-error', (e) => errs.push(String(e.detail?.error?.message ?? e.detail?.error)));
      window.__step = `${name} anim load`;
      await load(ts, name);
      await load(rs, name);
      if (tw) { await load(rw, name); await load(tw, name); }
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
        for (const el of els) el.paint();
        const ma = mediaReq(ts, rs), mb = mediaReq(rs, ts);
        if (ma !== mb || ts.media.wanted.size + ts.media.gifShown.size > 0) {
          r.mediaFrames++;
          r.mediaReq += ts.media.wanted.size + ts.media.gifShown.size;
          if (ma !== mb) { r.mediaDiff++; r.firstMedia ??= { what, ts: ma, rust: mb }; }
        }
        const pa = pixels(ts.canvas), pb = pixels(rs.canvas);
        const d = diff(pa, pb);
        r.frames++;
        if (d !== 0) {
          r.diff++;
          if (tw) {
            const pt = pixels(tw.canvas), pq = pixels(rw.canvas);
            const ne = (x, y, k) => x[k] !== y[k] || x[k + 1] !== y[k + 1] || x[k + 2] !== y[k + 2] || x[k + 3] !== y[k + 3];
            let un = 0, self = 0;
            for (let k = 0; k < pa.length; k += 4) { const s1 = ne(pa, pt, k) || ne(pb, pq, k); if (s1) self++; if (ne(pa, pb, k) && !s1) un++; }
            if (un === 0 && self > 0) r.noise++;
          }
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
      for (const el of els) el.play({ media: 'seek' });
      for (const el of els) halt(el);
      // `play()` takes its start from the real clock: put both on the virtual one (t = clock)
      for (const el of els) el.auto.t0 = -(el.auto.tl.slides[0]?.startMs ?? 0);
      const dur = ts.auto.tl.durationMs;
      for (let k = 0; k <= steps; k++) { clock = Math.min(dur, (dur * k) / steps); frame(`play t=${clock}`); }
      for (const el of els) el.stopPlayback();
      // presentation: clicks, the clock stepping through the transitions and the effects
      clock = 1e6;
      for (const el of els) el.present(0, { fullscreen: false });
      for (let k = 0; k < steps && ts.show; k++) {
        for (const el of els) halt(el);
        for (const dt of [0, 180, 420, 900]) { clock += dt; frame(`show step ${k} +${dt}`); for (const el of els) halt(el); }
        for (const el of els) el.next();
      }
      for (const el of els) el.stopPresentation();
      r.tsFree = rs.pkg === null && rs.tsModelLoads === 0;
      r.errors = errs;
    } catch (e) {
      r.errors.push(`exception ${e?.message ?? e}`);
    } finally {
      ts.remove(); rs.remove(); tw?.remove(); rw?.remove();
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
// ?nofeat=letterSpacing,fontKerning,roundRect,reset,filter: remove those members of the 2D context prototypes before the bundle
// loads (the canvas of Safari 15 in a newer browser; ?safari15=1 = all five + nooffscreen)
(function () { var q = location.search, m = /[?&]nofeat=([\\w,]+)/.exec(q), list = m ? m[1].split(',') : [];
  if (/[?&]safari15=1/.test(q)) { list = ['letterSpacing', 'fontKerning', 'roundRect', 'reset', 'filter']; window.OffscreenCanvas = undefined; }
  [window.CanvasRenderingContext2D, window.OffscreenCanvasRenderingContext2D].forEach(function (C) { if (C) list.forEach(function (k) { delete C.prototype[k]; }); }); })();
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
// ?v8math=1: the page's transcendental Math functions are V8's (uo_jsnum::math through the jsnum-memtest module) before the bundle
// loads, and the bundle has no ** left: the TS engine then computes as in Chromium, so what remains between TS and Rust is not
// the browser's libm (JavaScriptCore calls the system's; the Rust engine is V8-exact everywhere)
const v8math = /[?&]v8math=1/.test(location.search);
if (v8math) {
  const { instance } = await WebAssembly.instantiate(await (await fetch('/jsmath.wasm')).arrayBuffer(), {});
  const call = instance.exports.uo_jsnum_memtest_call;
  const K = { sin: 0, cos: 1, tan: 2, asin: 3, acos: 4, atan: 5, exp: 7, expm1: 8, log: 9, log1p: 10, log2: 11, log10: 12, sinh: 13, cosh: 14, tanh: 15, atanh: 16, cbrt: 17 };
  for (const [k, n] of Object.entries(K)) Math[k] = (x) => call(n, +x, 0, 0, 0);
  Math.atan2 = (y, x) => call(6, +y, +x, 0, 0);
  Math.pow = (x, y) => call(18, +x, +y, 0, 0);
  const hyp = Math.hypot;
  Math.hypot = function (a, b) { return arguments.length === 2 ? call(29, +a, +b, 0, 0) : hyp.apply(Math, arguments); };
}
const e = await import(v8math ? '/out-v8/entry.js' : '/out/entry.js');
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
// a difference in pictures is checked against a third element (TS): Rust equal to it while TS != TS is the browser's noise
window.__ssTwin = qs.get('twin') !== '0';
if (Number(qs.get('elw')) > 0) window.__ssElW = Number(qs.get('elw'));
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
// /?trace=k16.pptx:0,k17.pptx:2: the 2D-context calls of renderSlideToCanvas (1280 px) of each listed slide, TS then Rust, logged
// on every context (scratch canvases too: putImageData logs the FNV-1a of its pixels, measureText its width) and POSTed whole (the
// server writes trace-<browser>-<deck>-s<i>.json): where the two engines' calls part on a browser where only the pixels tell them apart
if (qs.get('trace')) {
  const ser0 = (v, idOf) => {
    if (typeof v === 'number') return Object.is(v, -0) ? '-0' : String(v);
    if (typeof v === 'string') return JSON.stringify(v);
    if (v === null || v === undefined || typeof v === 'boolean') return String(v);
    if (typeof ImageData !== 'undefined' && v instanceof ImageData) return 'ImageData ' + v.width + 'x' + v.height + ' ' + api.fnv(v.data);
    // a canvas source: its size and the FNV-1a of its pixels (read while the wrappers do not log)
    const px = (cv) => { try { return cv.width * cv.height <= 4e6 && cv.width > 0 && cv.height > 0 ? ' ' + api.fnv(cv.getContext('2d').getImageData(0, 0, cv.width, cv.height).data) : ''; } catch (err) { return ' ?'; } };
    if (typeof HTMLCanvasElement !== 'undefined' && v instanceof HTMLCanvasElement) return 'canvas#' + idOf(v) + ' ' + v.width + 'x' + v.height + px(v);
    if (typeof OffscreenCanvas !== 'undefined' && v instanceof OffscreenCanvas) return 'ocanvas#' + idOf(v) + ' ' + v.width + 'x' + v.height + px(v);
    if (typeof HTMLImageElement !== 'undefined' && v instanceof HTMLImageElement) return 'img ' + v.naturalWidth + 'x' + v.naturalHeight + ' ' + String(v.src).slice(-24);
    if (Array.isArray(v)) return '[' + v.map((x) => ser0(x, idOf)).join(',') + ']';
    if (typeof DOMMatrix !== 'undefined' && v instanceof DOMMatrix) return 'M(' + [v.a, v.b, v.c, v.d, v.e, v.f].join(',') + ')';
    if (typeof v === 'object') return (v.constructor && v.constructor.name || 'object') + '#' + idOf(v);
    return typeof v;
  };
  let log = null, ids = new WeakMap(), nid = 0;
  const idOf = (o) => { if (!ids.has(o)) ids.set(o, ++nid); return ids.get(o); };
  const ser = (v) => ser0(v, idOf);
  // default: the drawing calls only, each with the context state it draws under (transform, paint, alpha, font, ...), so the
  // two engines' different measuring and save/restore patterns drop out; ?tracemode=raw logs every call and property set
  const RAW = qs.get('tracemode') === 'raw';
  const DRAW = new Set(['fill', 'stroke', 'fillRect', 'strokeRect', 'clearRect', 'fillText', 'strokeText', 'drawImage', 'putImageData', 'clip']);
  const KEEP = new Set(['beginPath', 'moveTo', 'lineTo', 'bezierCurveTo', 'quadraticCurveTo', 'arc', 'arcTo', 'ellipse', 'rect', 'roundRect', 'closePath',
    'createPattern', 'createLinearGradient', 'createRadialGradient', 'createConicGradient', 'addColorStop', 'setTransform', 'getImageData', 'createImageData']);
  const STATE = ['fillStyle', 'strokeStyle', 'globalAlpha', 'globalCompositeOperation', 'font', 'lineWidth', 'lineJoin', 'lineCap', 'miterLimit', 'lineDashOffset',
    'shadowColor', 'shadowBlur', 'shadowOffsetX', 'shadowOffsetY', 'textAlign', 'textBaseline', 'direction', 'imageSmoothingEnabled', 'imageSmoothingQuality',
    'filter', 'letterSpacing', 'wordSpacing', 'fontKerning', 'fontStretch', 'fontVariantCaps', 'textRendering'];
  let inside = false;
  const state = (c) => { const t = c.getTransform ? c.getTransform() : null; return ' {' + (t ? 'T(' + [t.a, t.b, t.c, t.d, t.e, t.f].join(',') + ')' : '') + (c.getLineDash ? ' dash[' + c.getLineDash().join(',') + ']' : '')
    + STATE.filter((k) => k in c).map((k) => ' ' + k + '=' + ser(c[k])).join('') + '}'; };
  const wrap = (P, label) => {
    if (!P) return;
    for (const k of Object.getOwnPropertyNames(P)) {
      const d = Object.getOwnPropertyDescriptor(P, k);
      if (k === 'constructor') continue;
      if (typeof d.value === 'function') {
        const f = d.value;
        if (!RAW && !DRAW.has(k) && !KEEP.has(k)) continue;
        Object.defineProperty(P, k, { ...d, value: function (...a) {
          const r = f.apply(this, a);
          if (log && !inside) {
            inside = true;
            try {
              log.push(label + idOf(this.canvas ?? this) + ' ' + k + '(' + a.map(ser).join(',') + ')' + (k === 'measureText' ? '=' + r.width : k === 'getImageData' ? '=' + api.fnv(r.data) : '')
                + (/^create/.test(k) && r && typeof r === 'object' ? ' -> ' + ser(r) : '') + (!RAW && DRAW.has(k) && label !== 'g' && label !== 'p' ? state(this) : ''));
            } finally { inside = false; }
          }
          return r;
        } });
      } else if (d.set && RAW) {
        const s = d.set;
        Object.defineProperty(P, k, { ...d, set: function (v) { if (log) log.push(label + idOf(this.canvas ?? this) + ' ' + k + '=' + ser(v)); s.call(this, v); } });
      }
    }
  };
  wrap(window.CanvasRenderingContext2D && CanvasRenderingContext2D.prototype, 'c');
  wrap(window.OffscreenCanvasRenderingContext2D && OffscreenCanvasRenderingContext2D.prototype, 'o');
  wrap(window.CanvasGradient && CanvasGradient.prototype, 'g');
  wrap(window.CanvasPattern && CanvasPattern.prototype, 'p');
  for (const spec of qs.get('trace').split(',')) {
    const [n, si] = spec.split(':');
    show('trace ' + spec);
    const out = { kind: 'trace', deck: labels[n], name: n, slide: Number(si), logs: [] };
    // each engine logged from its load (the main canvas's first paints fill the raster caches) to the traced render; by default
    // as the compare runs them (TS loaded, then Rust, then the two renders; the logs of the two loads interleave by element),
    // ?tracemode=alone: each engine alone (loaded, rendered and removed before the other)
    const alone = qs.get('tracemode') === 'alone';
    const els = [], logs = [[], []];
    try {
      for (const [k, engine] of ['ts', 'rust'].entries()) {
        const el = api.mk(engine); els.push(el);
        ids = new WeakMap(); nid = 0; log = logs[k];
        await api.load(el, n);
        log = null;
        if (alone) { await renderOne(k); el.remove(); }
      }
      if (!alone) for (const k of [0, 1]) await renderOne(k);
    } catch (err) { out.error = String(err && err.stack || err); log = null; }
    async function renderOne(k) {
      const el = els[k];
      ids = new WeakMap(); nid = 0; log = logs[k];
      log.push('--- renderSlideToCanvas');
      const cv = document.createElement('canvas');
      el.renderSlideToCanvas(Number(si), cv, 1280);
      log = null;
      const px = api.fnv(cv.getContext('2d').getImageData(0, 0, cv.width, cv.height).data);
      out.logs.push({ engine: el.getEngine(), px, log: logs[k], png: qs.has('debug') ? cv.toDataURL() : undefined });
      cv.width = 0; cv.height = 0;
    }
    for (const el of els) el.remove();
    await post(out);
  }
  show('done trace');
  await new Promise(() => {});
}
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
  // how the JS engine stores a double into a Uint8ClampedArray (the spec: round half to even) and a fingerprint of Math.pow (the TS
  // rasters compute in JS, the Rust ones in wasm)
  clamp: (() => { const k = [91.3, 91.5, 91.7, 92.5, 0.5, 1.5, 254.6, -3, 300]; const u = new Uint8ClampedArray(k.length); k.forEach((v, i) => { u[i] = v; });
    const id = new ImageData(1, 3); id.data.set([91.7, 92.5, 0.5, 255, 91.3, 1.5, 254.6, 255]); return { set: [...u], from: [...Uint8ClampedArray.from(k)], imageData: [...id.data.slice(0, 8)] }; })(),
  // the same stores once the storing function is hot (the optimizing tiers compile the clamped store themselves): a raster loop of
  // the TS engine writes doubles into ImageData.data this way
  clampHot: (() => {
    const st = (u, i, v) => { u[i] = v; };
    const u = new Uint8ClampedArray(8), vals = [91.3, 91.5, 91.7, 92.5, 0.5, 1.5, 254.6, 120.49999];
    const id = new ImageData(64, 64), d = id.data;
    for (let k = 0; k < 2e6; k++) { st(u, k & 7, vals[k & 7]); d[k & 16383] = vals[k & 7] + (k & 1 ? 0.25 : 0); }
    for (let k = 0; k < 8; k++) st(u, k, vals[k]);
    return { set: [...u], imageData: [...d.slice(0, 8)] };
  })(),
  // calls that are equal by the spec but spelled differently by the two engines' painters: do they paint the same pixels?
  // ellipse(..., a1) against ellipse(..., a1, false), fill() against fill('nonzero'), clip() against clip('nonzero')
  spell: (() => {
    try {
      const draw = (explicit) => { const cv = document.createElement('canvas'); cv.width = 400; cv.height = 300; const c = cv.getContext('2d');
        c.fillStyle = '#fff'; c.fillRect(0, 0, 400, 300); c.fillStyle = '#3a506b'; c.strokeStyle = '#8c026d'; c.lineWidth = 2.37;
        c.beginPath(); c.rect(10.3, 10.7, 380.2, 280.1); if (explicit) c.clip('nonzero'); else c.clip();
        c.beginPath(); if (explicit) c.ellipse(200.0525517643628, 150.1088947214931, 158.25447652376783, 97.62846310877808, 0, 0, 6.283185307179586, false); else c.ellipse(200.0525517643628, 150.1088947214931, 158.25447652376783, 97.62846310877808, 0, 0, 6.283185307179586);
        if (explicit) c.fill('nonzero'); else c.fill(); c.stroke();
        c.beginPath(); c.moveTo(40.9, 250.8); c.lineTo(60.1, 250.8); if (explicit) c.ellipse(100.9, 250.8, 40.71052055993, 27.85441090696997, 0, 3.141592653589793, 0.13122929446270573, true); else c.ellipse(100.9, 250.8, 40.71052055993, 27.85441090696997, 0, 3.141592653589793, 0.13122929446270573, true);
        c.closePath(); if (explicit) c.fill('nonzero'); else c.fill();
        const d = c.getImageData(0, 0, 400, 300).data; cv.width = 0; cv.height = 0; return d; };
      const a = draw(false), b = draw(true); let n = 0; for (let i = 0; i < a.length; i += 4) if (a[i] !== b[i] || a[i + 1] !== b[i + 1] || a[i + 2] !== b[i + 2] || a[i + 3] !== b[i + 3]) n++;
      return n;
    } catch (err) { return String(err); }
  })(),
  // does the JIT fuse a*b+c into one rounding (FMA)? a*b+c with a = 1+2^-30, b = 1-2^-30, c = -1 is 0 in IEEE double arithmetic and
  // -2^-60 when fused: the result of a cold call and of the same function once hot (the optimizing tiers)
  fma: (() => {
    const f = (x, y, z) => x * y + z;
    const a = new Float64Array([1 + 2 ** -30, 1.5, 2.5]), b = new Float64Array([1 - 2 ** -30, 0.5, 0.25]), c = new Float64Array([-1, 0, 0]);
    const cold = f(a[0], b[0], c[0]);
    let fused = 0;
    for (let i = 0; i < 3e6; i++) { const r = f(a[i % 3], b[i % 3], c[i % 3]); if (i % 3 === 0 && r !== 0) fused++; }
    return { cold, hot: f(a[0], b[0], c[0]), fused };
  })(),
  pow: (() => { let h = 0x811c9dc5; for (let i = 0; i < 256; i++) for (const e of [2.2, 1 / 2.2, 2.4, 1 / 2.4]) { const f = new Float64Array([Math.pow(i / 255, e)]); const b = new Uint8Array(f.buffer); for (const x of b) h = Math.imul(h ^ x, 0x01000193); } return (h >>> 0).toString(16); })(),
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
  mediaDiff: 0, animBad: 0, crashes: 0, best: null, soak: null, knownDecks: 0, knownSlides: 0, metaDecks: 0, metaSlides: 0, noiseDecks: 0, noiseSlides: 0,
  mainNoise: 0, thumbNoise: 0, animKnown: 0, animNoise: 0 };
let tot = JSON.parse(sessionStorage.getItem('uo-ss-tot') ?? 'null') ?? zero;
const save = () => sessionStorage.setItem('uo-ss-tot', JSON.stringify(tot));
let phase = 'compare', from = 0;
const at = sessionStorage.getItem('uo-ss-at');
if (at === null) { tot = { ...zero }; save(); await post({ kind: 'start', ...features, nooffscreen: qs.get('nooffscreen') === '1', tsts: qs.get('tsts') === '1', v8math, fontdelay: Number(qs.get('fontdelay')) || 0, fontlag: Number(qs.get('fontlag')) || 0, decks: names.length, anim: animNames.length, soak: SOAK, tsStrict: true }); }
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
    const pixOnly = !r.errors.length && r.engine === 'rust' && !r.notesDiff && !r.a11yDiff && !r.linksDiff && r.tsFree && !r.tsLoads;
    // the browser's own noise (serve, the TS twin): every differing picture of the row is one where the Rust picture equals a second
    // TS element's bit for bit while the two TS elements differ (TS against TS)
    const noiseSlides = r.slideDiff.filter((x) => x.noise).length;
    const unexplained = r.slideDiff.length - noiseSlides + (r.mainDiff - (r.mainNoise ?? 0)) + (r.thumbDiff - (r.thumbNoise ?? 0));
    const known = ok || !pixOnly ? null : typeof OffscreenCanvas === 'undefined' && /[/]b5b[/]/.test(labels[names[i]]) ? 'ts-metafile-needs-offscreencanvas'
      : unexplained === 0 ? 'same-engine-variance' : null;
    tot.diffSlides += r.slideDiff.length; tot.errors += r.errors.length; if (r.tsFree) tot.tsFree++; if (!ok) tot.badDecks++;
    tot.mainNoise += r.mainNoise ?? 0; tot.thumbNoise += r.thumbNoise ?? 0;
    if (known) { tot.knownDecks++; tot.knownSlides += r.slideDiff.length; }
    if (known === 'ts-metafile-needs-offscreencanvas') { tot.metaDecks++; tot.metaSlides += r.slideDiff.length; }
    if (known === 'same-engine-variance') { tot.noiseDecks++; tot.noiseSlides += noiseSlides; }
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
    const run = async (twin) => { try { return await api.animate(animNames[i], 8, twin); } catch (err) { return { name: animNames[i], engine: '?', frames: 0, diff: 0, noise: 0, rust: 0, mediaDiff: 0, errors: ['exception ' + String(err && err.stack || err)] }; } };
    let r = await run(false);
    const ok = !(r.diff || r.errors.length || r.engine !== 'rust' || r.mediaDiff || !r.tsFree);
    // frames that differ in pixels only: a second pass with a TS twin painted alongside; the deck is the browser's noise when every
    // frame that differs there has the Rust frame equal to the twin's while the two TS elements differ
    let known = null;
    if (!ok && r.diff && !r.errors.length && r.engine === 'rust' && !r.mediaDiff && r.tsFree) {
      const first = r;
      r = await run(true);
      r.firstPass = { frames: first.frames, diff: first.diff, firstDiff: first.firstDiff };
      if (!r.errors.length && r.engine === 'rust' && !r.mediaDiff && r.tsFree && r.diff === r.noise) known = 'same-engine-variance';
    }
    tot.animDecks++; tot.animFrames += r.frames; tot.animDiff += r.diff; tot.animRust += r.rust; tot.mediaDiff += r.mediaDiff ?? 0;
    if (known) { tot.animKnown++; tot.animNoise += r.noise; } else if (!ok) tot.animBad++;
    save();
    log('anim ' + labels[animNames[i]] + ' ' + r.frames + ' frames ' + (ok ? 'ok' : 'MISMATCH ' + JSON.stringify({ first: r.firstDiff, e: r.errors.slice(0, 2) })));
    await post({ kind: 'anim', i, of: animNames.length, deck: labels[animNames[i]], ok, known, dpr, row: r });
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
const ok = tot.badDecks === 0 && tot.animBad === 0 && !tot.animKnown && tot.crashes === 0 && !!tot.soak?.ok && tot.decks === names.length;
const okExceptKnown = !ok && tot.badDecks === tot.knownDecks && tot.animBad === 0 && tot.crashes === 0 && !!tot.soak?.ok && tot.decks === names.length;
const verdict = ok ? 'PASS' : okExceptKnown ? 'PASS-EXCEPT-KNOWN' : 'FAIL';
const summary = tot.decks + '/' + names.length + ' decks (' + tot.rust + ' in Rust), ' + tot.slides + ' slides, ' + tot.diffSlides + ' slides differ, '
  + tot.badDecks + ' decks with a difference' + (tot.metaDecks ? ' (' + tot.metaDecks + ' decks / ' + tot.metaSlides + ' slides known: TS metafile player without OffscreenCanvas)' : '')
  + (tot.noiseDecks ? ' (' + tot.noiseDecks + ' decks / ' + tot.noiseSlides + ' slides same-engine variance: TS != TS or Rust != Rust, TS = Rust wherever each engine is stable)' : '') + '; main ' + tot.mainDiff + '/' + tot.mainChecked + (tot.mainNoise ? ' (' + tot.mainNoise + ' noise)' : '')
  + ', thumbs ' + tot.thumbDiff + (tot.thumbNoise ? ' (' + tot.thumbNoise + ' noise)' : '') + ', notes ' + tot.notesDiff
  + ', a11y ' + tot.a11yDiff + '/' + tot.a11yChecked + ', links ' + tot.linksDiff + '/' + tot.linkProbes + '; anim ' + tot.animDecks + ' decks '
  + tot.animFrames + ' frames ' + tot.animDiff + ' differ' + (tot.animKnown ? ' (' + tot.animKnown + ' decks / ' + tot.animNoise + ' frames same-engine variance)' : '') + ', media ' + tot.mediaDiff + '; soak ' + (tot.soak?.ok ? 'ok' : 'FAIL') + ' (spread '
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
