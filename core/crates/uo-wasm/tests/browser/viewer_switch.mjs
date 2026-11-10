// The TS/Rust switch of <universal-document-viewer> in a real Chromium (headless): `engine="ts"` against `engine="rust"` on the
// committed fixture documents (record core/docs/modules/uo-bindings.md "The TS/Rust switch of the viewer"), and the memory of
// opening and closing documents with `engine="rust"`.
//
//   UO_TS_ROOT=<frozen checkout: tools only> [UO_CHROMIUM=<chrome-headless-shell.exe>] [UO_WASM_PATH=<.wasm: count,write build>]
//   [UO_SW_STRIDE=1] [UO_SW_SOAK=200] node crates/uo-wasm/tests/browser/viewer_switch.mjs [--mode parity|memory|all|phase0]
//
// The page is the real thing: the viewer's `src/index.ts` and the Rust engine's JS bundled by the frozen esbuild with the settings
// of tools/build-pkg.mjs (`splitting`, `chunks/[name]-[hash]`, the same alias table, `@universal-office/write-engine`), served by a
// node:http server together with `chunks/uo-write.wasm` (the DEFAULT wasm URL, no `configureEngine`), the open fonts and the
// fixtures. Phase 0 proves that a viewer without `engine="rust"` requests neither the .wasm nor the engine's chunk and that the
// entry's static closure holds none of the engine's code. The parity phase compares, per fixture: page count and sizes, headings,
// comments, link boxes, the text layer (innerHTML), find hits, canvas pixels at two scales (byte for byte, tolerance 0) and the
// uncompressed PDF of the print layout. The memory phase opens and closes documents with `engine="rust"` and checks that the
// module's live bytes return to a flat line and no handle stays alive.
import { createRequire } from 'node:module';
import { createServer } from 'node:http';
import { mkdirSync, readFileSync, writeFileSync, existsSync, readdirSync, copyFileSync, rmSync } from 'node:fs';
import { join, dirname, resolve, extname, basename } from 'node:path';
import { fileURLToPath } from 'node:url';
import { fixtureDocs, relDoc, NOW } from '../support/paint-common.mjs';

const here = dirname(fileURLToPath(import.meta.url));
const core = resolve(here, '../../../..');
const repo = resolve(core, '..');
const root = process.env.UO_TS_ROOT;
if (!root) throw new Error('set UO_TS_ROOT to the frozen TS checkout (tools only: esbuild, playwright, the converter dist)');
const arg = (name, dflt) => { const i = process.argv.indexOf(name); return i >= 0 ? process.argv[i + 1] : dflt; };
const MODE = arg('--mode', 'all');
const STRIDE = Number(process.env.UO_SW_STRIDE ?? 1);
const SOAK = Number(process.env.UO_SW_SOAK ?? 200);
const wasmPath = process.env.UO_WASM_PATH ?? join(core, 'target/agent-sw/wasm32-unknown-unknown/wasm/uo_wasm.wasm');
const outDir = join(process.env.CARGO_TARGET_DIR ?? join(core, 'target'), 'sw-viewer');
const out = join(outDir, 'out');
const fwd = (p) => p.replace(/\\/g, '/');
rmSync(out, { recursive: true, force: true });
mkdirSync(join(out, 'chunks'), { recursive: true });

// ── the bundle: the settings and the alias table of tools/build-pkg.mjs buildProduct ───────────────
const W = (p) => fwd(join(repo, p));
const driver = `
import { UniversalDocumentViewerElement } from '${W('write/packages/viewer/src/index.ts')}';
import { renderPdf, pdfNavigation } from '${W('write/packages/docx-layout/src/index.ts')}';
export { UniversalDocumentViewerElement, renderPdf, pdfNavigation };
const statsUrl = new URL('./stats.js', import.meta.url).href;
export const engineModule = () => import(statsUrl);
`;
const statsEntry = `export { loadViewerModule, viewerModuleLoaded } from '${W('core/crates/uo-wasm/js/viewer.ts')}';\n`;
writeFileSync(join(outDir, 'entry.mjs'), driver);
writeFileSync(join(outDir, 'stats.mjs'), statsEntry);
const INTERNAL = {
  '@universal-office/fonts': 'shared/fonts/src/index.ts',
  '@universal-office/charts': 'shared/charts/src/index.ts',
  '@universal-office/drawingml': 'shared/drawingml/src/index.ts',
  '@universal-office/ui': 'shared/ui/src/index.ts',
  '@universal-office/docx-core': 'write/packages/docx-core/src/index.ts',
  '@universal-office/docx-layout': 'write/packages/docx-layout/src/index.ts',
};
const alias = Object.fromEntries(Object.entries(INTERNAL).map(([n, f]) => [n, join(repo, f)]));
alias['@universal-office/write-engine'] = join(repo, 'core/crates/uo-wasm/js/viewer.ts');
alias['@universal-office/converter'] = join(root, 'converter/packages/converter/dist/index.js'); // the worktree has no node_modules: the converter's dist of the frozen root
const esbuildPath = join(root, 'write/node_modules/esbuild/lib/main.js');
const esbuild = createRequire(join(root, 'write/package.json'))(existsSync(esbuildPath) ? esbuildPath : 'esbuild');
const built = await esbuild.build({
  entryPoints: { entry: join(outDir, 'entry.mjs'), stats: join(outDir, 'stats.mjs') },
  outdir: out, bundle: true, format: 'esm', platform: 'browser', target: 'es2022', splitting: true, chunkNames: 'chunks/[name]-[hash]',
  minify: true, legalComments: 'none', alias, absWorkingDir: repo, nodePaths: [join(root, 'write/node_modules')], logLevel: 'warning', metafile: true,
});
copyFileSync(wasmPath, join(out, 'chunks', 'uo-write.wasm'));

// the closure the entry loads statically, and what only the dynamic import reaches
const outs = built.metafile.outputs;
const rel = (o) => fwd(o);
const closure = (o0) => { const seen = new Set(); const visit = (o) => { if (seen.has(o) || !outs[o]) return; seen.add(o); for (const i of outs[o].imports) if (i.kind === 'import-statement') visit(i.path); }; visit(o0); return seen; };
const entryOut = Object.keys(outs).find((o) => /(^|\/)entry\.js$/.test(rel(o)));
const staticSet = closure(entryOut);
const staticInputs = [...staticSet].flatMap((o) => Object.keys(outs[o].inputs)).map((f) => fwd(resolve(repo, f)));
const ENGINE_CODE = /core\/crates\/uo-wasm\/js\/(viewer|paint|picture|write|render|hyphen-patterns)\.(ts|mjs)$|core\/crates\/uo-wasm\/js\/gen\//;
const leaked = staticInputs.filter((f) => ENGINE_CODE.test(f));
const engineChunk = Object.keys(outs).find((o) => /chunks\/rust-engine-/.test(rel(o)));
const onlyDynamic = new Set([...closure(engineChunk)].filter((o) => !staticSet.has(o)));
const kb = (n) => (n / 1024).toFixed(0);
console.log(`bundle: entry static closure ${staticSet.size} output(s) ${kb([...staticSet].reduce((n, o) => n + outs[o].bytes, 0))} KB; reachable only by import("./rust-engine"): ${onlyDynamic.size} output(s) ${kb([...onlyDynamic].reduce((n, o) => n + outs[o].bytes, 0))} KB + uo-write.wasm ${kb(readFileSync(wasmPath).length)} KB`);
let failures = 0;
const fail = (m) => { failures++; console.log(`FAIL ${m}`); };
if (leaked.length) fail(`the entry's static closure holds the engine's code: ${leaked.map((f) => relative(f)).join(', ')}`);
function relative(f) { return f.replace(fwd(repo) + '/', ''); }

// ── the server ──────────────────────────────────────────────────────────────────────────────────
const ONLY = process.env.UO_SW_ONLY; // a substring of the fixture path: run those documents only
const fixtures = fixtureDocs().filter((p, i) => i % STRIDE === 0 && (!ONLY || new RegExp(ONLY).test(p)));
const docsByName = new Map(fixtures.map((p, i) => [`d${i}.docx`, p]));
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

// ── Chromium ────────────────────────────────────────────────────────────────────────────────────
const { chromium } = createRequire(join(root, 'sheet/package.json'))('playwright');
const installed = (() => {
  const base = join(process.env.LOCALAPPDATA ?? '', 'ms-playwright');
  if (!existsSync(base)) return undefined;
  const dirs = readdirSync(base).filter((d) => d.startsWith('chromium_headless_shell-')).sort();
  const exe = dirs.length && join(base, dirs[dirs.length - 1], 'chrome-headless-shell-win64', 'chrome-headless-shell.exe');
  return exe && existsSync(exe) ? exe : undefined;
})();
const browser = await chromium.launch({ headless: true, executablePath: process.env.UO_CHROMIUM ?? installed });
const context = await browser.newContext({ timezoneId: 'UTC', deviceScaleFactor: 1, viewport: { width: 1280, height: 900 } });
let page = null;
const pageRequests = [];
/** a fresh page (a new module instance, a clean heap): the parity phase restarts it every BATCH documents, because the page
 *  accumulates canvases and FontFaces of both engines and a long run on one page slows down until it stops answering */
async function openPage() {
  if (page) await page.close();
  page = await context.newPage();
  await page.clock.setFixedTime(NOW);
  page.on('crash', () => { console.log('PAGE CRASHED'); process.exit(2); });
  page.on('pageerror', (e) => console.log('PAGE ERROR', e.message));
  page.on('console', (m) => { if (m.type() === 'error' && !/Failed to load resource/.test(m.text())) console.log('CONSOLE', m.text()); });
  page.on('request', (r) => pageRequests.push(r.url()));
  await page.goto(`http://127.0.0.1:${server.address().port}/`);
  await page.waitForFunction(() => window.__ready === true, null, { timeout: 60000 });
}
const BATCH = Number(process.env.UO_SW_BATCH ?? 0);
await openPage();
const names = [...docsByName.keys()];
const label = (n) => relDoc(docsByName.get(n));

// the page-side driver (a function evaluated in the page; everything it needs is passed by argument)
const driverFn = () => {
  const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
  const fnv = (u8) => { let h = 0x811c9dc5; for (let i = 0; i < u8.length; i++) h = Math.imul(h ^ u8[i], 0x01000193); return (h >>> 0).toString(16).padStart(8, '0'); };
  const make = (engine) => {
    const el = document.createElement('universal-document-viewer');
    el.setAttribute('engine', engine);
    el.setAttribute('fonts', '/fonts');
    el.setAttribute('zoom', '100%');
    el.setAttribute('nocontrols', '');
    el.style.cssText = 'display:block;width:1000px;height:800px';
    document.body.appendChild(el);
    return el;
  };
  const fetchDoc = async (n) => new Uint8Array(await (await fetch(`/docs/${n}`)).arrayBuffer());
  const settle = async (el) => {
    let last = -1, stable = 0;
    for (let n = 0; n < 80 && stable < 4; n++) {
      await sleep(40);
      if (el.layoutVersion === last) stable++; else { stable = 0; last = el.layoutVersion; }
    }
  };
  const textOf = (el, i) => {
    const div = document.createElement('div');
    el.fillText(i, { text: div, top: 0, h: Math.round(el.pageHeightTw(i) * el.pxPerTw(i)) }, true);
    return div;
  };
  const render = async (el, i, s) => {
    const p = el.layout.pages[i];
    const W = Math.max(1, Math.round(p.widthTw * s)), H = Math.min(3200, Math.max(1, Math.round(p.heightTw * s)));
    let last = null, data = null, settled = false;
    for (let round = 0; round < 12; round++) {
      const cv = document.createElement('canvas');
      el.drawPage(i, cv, W, H, W / p.widthTw, 0, false);
      data = cv.getContext('2d').getImageData(0, 0, W, H).data;
      const h = fnv(new Uint8Array(data.buffer, data.byteOffset, data.byteLength));
      if (h === last) { settled = true; break; }
      last = h;
      if (round > 0) await sleep(40);
    }
    return { data, W, H, settled };
  };
  const loadedLayoutEngine = (el) => el.layout?.engine ?? 'ts';
  return {
    async phase0(names) {
      const out = [];
      for (const n of names) {
        const el = make('ts');
        const t0 = performance.now();
        await el.loadDocx(await fetchDoc(n));
        out.push({ n, pages: el.getPageCount(), ms: performance.now() - t0, engine: loadedLayoutEngine(el) });
        el.remove();
      }
      return out;
    },
    async compare(n, scales) {
      window.__step = `${n}: fetch`;
      const bytes = await fetchDoc(n);
      const ts = make('ts'), rust = make('rust');
      const res = { n, mismatches: [], facets: {} };
      const bad = (facet, detail) => res.mismatches.push({ facet, detail });
      try {
        let t0 = performance.now();
        window.__step = `${n}: ts.loadDocx`;
        await ts.loadDocx(bytes);
        res.tsMs = performance.now() - t0;
        t0 = performance.now();
        window.__step = `${n}: rust.loadDocx`;
        await rust.loadDocx(bytes);
        window.__step = `${n}: settle`;
        res.rustMs = performance.now() - t0;
        await document.fonts.ready;
        await Promise.all([settle(ts), settle(rust)]);
        if (loadedLayoutEngine(ts) !== 'ts' || loadedLayoutEngine(rust) !== 'rust') bad('engine', `${loadedLayoutEngine(ts)} / ${loadedLayoutEngine(rust)}`);
        const A = ts.layout.pages, B = rust.layout.pages;
        res.pages = A.length;
        // (1) page count and sizes
        if (A.length !== B.length) bad('pages', `${A.length} vs ${B.length}`);
        else for (let i = 0; i < A.length; i++) if (A[i].widthTw !== B[i].widthTw || A[i].heightTw !== B[i].heightTw) bad('sizes', `page ${i}: ${A[i].widthTw}x${A[i].heightTw} vs ${B[i].widthTw}x${B[i].heightTw}`);
        res.facets.sizes = A.length;
        const n0 = Math.min(A.length, B.length);
        // (2) headings, comments, link boxes
        const j = (v) => JSON.stringify(v);
        if (j(ts.getHeadings()) !== j(rust.getHeadings())) bad('headings', `${j(ts.getHeadings()).slice(0, 200)} vs ${j(rust.getHeadings()).slice(0, 200)}`);
        if (j(ts.getComments()) !== j(rust.getComments())) bad('comments', 'differ');
        const boxes = (el) => el.linkBoxes.map((pg) => pg.map(({ page, x, y, w, h, value }) => ({ page, x, y, w, h, value })));
        if (j(boxes(ts)) !== j(boxes(rust))) bad('linkBoxes', `${boxes(ts).flat().length} vs ${boxes(rust).flat().length}`);
        res.facets.headings = ts.getHeadings().length; res.facets.comments = ts.getComments().length; res.facets.linkBoxes = boxes(ts).flat().length;
        // (3) the text layer and (4) find hits
        const texts = [[], []];
        let textLen = 0;
        for (let i = 0; i < n0; i++) {
          const a = textOf(ts, i), b = textOf(rust, i);
          texts[0].push(a.textContent.toLowerCase()); texts[1].push(b.textContent.toLowerCase());
          if (a.innerHTML !== b.innerHTML) bad('textLayer', `page ${i}: ${a.innerHTML.length} vs ${b.innerHTML.length} chars of markup`);
          textLen += a.textContent.length;
        }
        res.facets.textChars = textLen;
        const freq = new Map();
        for (const t of texts[0]) for (const w of t.match(/\p{L}{4,}/gu) ?? []) freq.set(w, (freq.get(w) ?? 0) + 1);
        const queries = [...freq.entries()].sort((x, y) => y[1] - x[1] || (x[0] < y[0] ? -1 : 1)).slice(0, 5).map((e) => e[0]);
        const accent = [...freq.keys()].sort().find((w) => /[^\x00-\x7f]/.test(w));
        if (accent) queries.push(accent);
        const count = (t, q) => { let c = 0, at = -1; while ((at = t.indexOf(q, at + 1)) >= 0) c++; return c; };
        let hits = 0;
        for (const q of queries) for (let i = 0; i < n0; i++) {
          const a = count(texts[0][i], q), b = count(texts[1][i], q);
          hits += a;
          if (a !== b) bad('find', `"${q}" page ${i}: ${a} vs ${b}`);
        }
        res.facets.queries = queries.length; res.facets.hits = hits;
        window.__step = `${n}: pixels`;
        // (5) pixels: warm up (images and faces decode asynchronously), then two settled renders per page and scale
        for (const s of scales) for (let i = 0; i < n0; i++) { await render(ts, i, s); await render(rust, i, s); }
        await sleep(120);
        let px = 0, ink = 0, renders = 0, unsettled = 0;
        for (const s of scales) for (let i = 0; i < n0; i++) {
          const a = await render(ts, i, s), b = await render(rust, i, s);
          renders++;
          if (!a.settled || !b.settled) unsettled++;
          px += a.W * a.H;
          let diff = 0, first = -1;
          for (let k = 0; k < a.data.length; k += 4) {
            if (a.data[k] !== 255 || a.data[k + 1] !== 255 || a.data[k + 2] !== 255) ink++;
            if (a.data[k] !== b.data[k] || a.data[k + 1] !== b.data[k + 1] || a.data[k + 2] !== b.data[k + 2] || a.data[k + 3] !== b.data[k + 3]) { diff++; if (first < 0) first = k / 4; }
          }
          if (diff) bad('pixels', `page ${i} scale ${s.toFixed(4)}: ${diff} of ${a.W * a.H} px differ (first at ${first % a.W},${Math.floor(first / a.W)})${a.settled && b.settled ? '' : ' [unsettled]'}`);
        }
        res.facets.pixels = px; res.facets.ink = ink; res.facets.renders = renders; res.facets.unsettled = unsettled;
        window.__step = `${n}: pdf`;
        // (6) the uncompressed PDF of the print layout
        const lay = ts.printLayout();
        const pdfTs = renderPdfFn(lay, { compress: false, navigation: pdfNavFn(ts.full, lay) });
        const pdfRust = rust.printLayout().pdf({ compress: false });
        res.facets.pdfBytes = pdfTs.length;
        if (pdfTs.length !== pdfRust.length || fnv(pdfTs) !== fnv(pdfRust)) bad('pdf', `${pdfTs.length} B (${fnv(pdfTs)}) vs ${pdfRust.length} B (${fnv(pdfRust)})`);
      } catch (e) {
        bad('exception', String(e?.stack ?? e).slice(0, 400));
      } finally {
        ts.remove(); rust.remove();
      }
      return res;
    },
    async soak(names, count) {
      const engine = await engineModuleFn();
      let mod = null; // the memoized module: available once the first viewer loaded it (the default source is the viewer's own)
      const series = [];
      for (let k = 0; k < count; k++) {
        window.__soakK = k;
        const el = make('rust');
        await el.loadDocx(await fetchDoc(names[k % names.length]));
        mod ??= await engine.loadViewerModule();
        const pages = el.getPageCount();
        const during = mod.stats();
        el.remove();
        const s = mod.stats();
        series.push({ k, pages, buffer: s.buffer, live: s.live, handles: s.handles, duringHandles: during.handles });
      }
      return series;
    },
  };
};
const evalDriver = () => page.evaluateHandle(`(() => { const renderPdfFn = window.__e.renderPdf, pdfNavFn = window.__e.pdfNavigation, engineModuleFn = window.__e.engineModule; return (${driverFn.toString()})(); })()`);

const t00 = Date.now();
let api = await evalDriver();
const sumBy = (rows, f) => rows.reduce((n, r) => n + (f(r) ?? 0), 0);

// ── phase 0 ─────────────────────────────────────────────────────────────────────────────────────
if (MODE === 'phase0' || MODE === 'parity' || MODE === 'all') {
  const rows = await api.evaluate((a, ns) => a.phase0(ns), names.slice(0, 3));
  const wasmReq = [...requests, ...pageRequests].filter((u) => /\.wasm($|\?)/.test(u) || /rust-engine/.test(u) || /\/stats\.js/.test(u));
  console.log(`phase 0: ${rows.length} documents with engine="ts" (${rows.map((r) => `${r.pages}p ${r.ms.toFixed(0)} ms`).join(', ')}); requests ${requests.length}, for the wasm / the engine chunk: ${wasmReq.length}`);
  if (wasmReq.length) fail(`a viewer without engine="rust" requested ${wasmReq.join(', ')}`);
  if (rows.some((r) => r.engine !== 'ts')) fail('phase 0: a layout is not the TS one');
}

// ── parity ──────────────────────────────────────────────────────────────────────────────────────
if (MODE === 'parity' || MODE === 'all') {
  const scales = [1 / 15, 2 / 15];
  const rows = [];
  let inBatch = 0;
  for (const n of names) {
    if (BATCH > 0 && inBatch++ >= BATCH) { await openPage(); api = await evalDriver(); inBatch = 1; }
    let timer = null;
    const r = await Promise.race([
      api.evaluate((a, [n, s]) => a.compare(n, s), [n, scales]),
      new Promise((_, rej) => { timer = setTimeout(async () => {
        // the page is not answering: pause it through CDP and print where it is
        const cdp = await context.newCDPSession(page);
        cdp.on('Debugger.paused', (e) => { console.log(`PAUSED in ${label(n)}:`); for (const f of e.callFrames.slice(0, 8)) console.log(`   ${f.functionName || '(anon)'} ${f.url.split('/').slice(-2).join('/')}:${f.location.lineNumber}:${f.location.columnNumber}`); process.exit(3); });
        await cdp.send('Debugger.enable');
        await cdp.send('Debugger.pause');
        setTimeout(() => rej(new Error(`timeout on ${label(n)}`)), 20000);
      }, 150000); }),
    ]);
    clearTimeout(timer);
    rows.push(r);
    if (process.env.UO_SW_VERBOSE) console.log(`  ${label(n)}: ${r.pages}p TS ${r.tsMs?.toFixed(0)} ms Rust ${r.rustMs?.toFixed(0)} ms${r.mismatches.length ? ' MISMATCH' : ''}`);
    if (r.mismatches.length) fail(`${label(n)}: ${r.mismatches.slice(0, 3).map((m) => `${m.facet}: ${m.detail}`).join(' | ')}`);
  }
  const by = (facet) => rows.filter((r) => r.mismatches.some((m) => m.facet === facet)).length;
  const F = (k) => sumBy(rows, (r) => r.facets[k]);
  console.log(`parity: ${rows.length} documents, ${sumBy(rows, (r) => r.pages)} pages in ${((Date.now() - t00) / 1000).toFixed(0)} s`);
  console.log(`  page count + sizes   ${by('pages') + by('sizes')} documents differ (${F('sizes')} pages compared)`);
  console.log(`  headings / comments / link boxes  ${by('headings') + by('comments') + by('linkBoxes')} differ (${F('headings')} headings, ${F('comments')} comments, ${F('linkBoxes')} link boxes)`);
  console.log(`  text layer           ${by('textLayer')} differ (${F('textChars')} characters)`);
  console.log(`  find hits            ${by('find')} differ (${F('queries')} queries, ${F('hits')} hits)`);
  console.log(`  pixels               ${by('pixels')} documents differ (${F('renders')} renders, ${F('pixels')} px, ${F('ink')} ink px, ${F('unsettled')} unsettled renders), tolerance 0`);
  console.log(`  PDF (print layout)   ${by('pdf')} differ (${F('pdfBytes')} B)`);
  console.log(`  exceptions           ${by('exception')}`);
  const tsMs = sumBy(rows, (r) => r.tsMs), rustMs = sumBy(rows, (r) => r.rustMs);
  const med = (k) => { const v = rows.map((r) => r[k]).sort((x, y) => x - y); return v[Math.floor(v.length / 2)]; };
  console.log(`  loadDocx (open + layout, incl. fonts): TS ${tsMs.toFixed(0)} ms total, median ${med('tsMs').toFixed(0)} ms; Rust ${rustMs.toFixed(0)} ms total, median ${med('rustMs').toFixed(0)} ms`);
}

// ── memory ──────────────────────────────────────────────────────────────────────────────────────
if (MODE === 'memory' || MODE === 'all') {
  const cycle = names.slice(0, 10);
  const t0 = Date.now();
  const poll = setInterval(async () => {
    const k = await Promise.race([page.evaluate(() => window.__soakK).catch(() => 'err'), new Promise((r) => setTimeout(() => r('no answer'), 5000))]);
    console.log(`  ... soak at ${k} (${((Date.now() - t0) / 1000).toFixed(0)} s)`);
  }, 15000);
  const series = await api.evaluate((a, [ns, k]) => a.soak(ns, k), [cycle, SOAK]);
  clearInterval(poll);
  const warm = cycle.length; // the first cycle warms the env tables, the hyphenation, the font caches
  console.log(`memory: ${series.length} open/close cycles of ${cycle.length} documents with engine="rust" in ${((Date.now() - t0) / 1000).toFixed(0)} s`);
  for (const s of series) if (s.k % 20 === 0 || s.k === series.length - 1) console.log(`  #${String(s.k).padStart(3)}  pages ${String(s.pages).padStart(2)}  live ${s.live}  buffer ${s.buffer}  handles after close ${s.handles} (while open ${s.duringHandles})`);
  if (series.some((s) => s.handles !== 0)) fail(`handles left alive after close: ${series.filter((s) => s.handles !== 0).length} samples`);
  if (series.some((s) => s.live === null)) fail('the module has no uo_memory_live_bytes export: build with --features count,write');
  const after = series.slice(warm);
  const liveSet = new Set(after.map((s) => s.live)), bufSet = new Set(after.map((s) => s.buffer));
  console.log(`  after warm-up (${after.length} samples): live bytes distinct values ${liveSet.size} (${[...liveSet].slice(0, 4).join(', ')}), buffer distinct ${bufSet.size} (${[...bufSet].slice(0, 4).join(', ')}), first-cycle live ${series[warm - 1]?.live}`);
  if (liveSet.size !== 1) fail(`live bytes are not a flat line after warm-up (${liveSet.size} distinct values: ${[...liveSet].slice(0, 6).join(', ')})`);
  if (bufSet.size !== 1) fail(`the wasm memory grew after warm-up (${[...bufSet].join(', ')})`);
}

await browser.close();
server.close();
console.log(failures ? `viewer_switch: ${failures} FAILURE(S)` : 'viewer_switch: ok');
process.exit(failures ? 1 : 0);
