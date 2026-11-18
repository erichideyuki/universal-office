// The TS/Rust switch of the Slides EDITOR <universal-slides> in a real Chromium (headless): `engine="ts"` against `engine="rust"`
// driven through the element's public API by the same seeded scripts (record core/docs/modules/uo-bindings.md "Slides editor (B8
// routing)").
//
//   [UO_TS_ROOT=<frozen checkout: tools only, default the 0.9.2 reference>] [UO_CHROMIUM=<chrome-headless-shell.exe>]
//   [UO_WASM_PATH=<uo-slides-editor.wasm: the slides-edit,count build>] [UO_PPT_WASM_PATH=<uo-slides-ppt.wasm>]
//   [UO_SE_DECKS=<n of b1a decks, default 24>] [UO_SE_B6A=<n of b6a decks, default 8>] [UO_SE_STEPS=<steps per script, default 40>]
//   [UO_SE_LIMIT=<first n decks only>] [UO_SE_SOAK=300] node crates/uo-wasm/tests/browser/slides_edit_switch.mjs [--mode all|phase0|parity|memory|faults]
//
// The page is the real thing: the editor's `src/index.ts`, the viewer's source and both engine JS sides bundled by the frozen esbuild
// (splitting, the alias table of tools/build-pkg.mjs: `@universal-office/slides-engine`, `@universal-office/slides-editor-engine`),
// served with `chunks/uo-slides-editor.wasm` and `chunks/uo-slides-ppt.wasm` at the DEFAULT URLs, the open fonts and the decks.
//   phase0  an editor without engine="rust" requests neither the wasm nor the engine chunks
//   parity  per deck, a TS element and a Rust element run the same script (every editing method of the element, the inline text
//           overlay, clip copy/paste, undo/redo, notes, properties, replace, font option); after EVERY step: the package (part names,
//           order and bytes of the element's package), the UI state (slide, selection, undo/redo flags, dirty, the shape info and text
//           format of the selection, notes, the font option), the pixels of the current slide (`renderSlideToCanvas`, 960 px) and of
//           the main canvas; every 8 steps and at the end the bytes `savePptx()` writes (read back part by part); tolerance 0
//   memory  300 edit/undo steps on the Rust element: the module's live bytes and the wasm buffer flat; handles and sessions 0 after removal
//   faults  a trap of the instance mid-session (the element keeps editing on the TS engine from the same package), a DOM move then undo
//           (the session and its history survive), the engine attribute switched with the document open
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
const NB1A = Number(process.env.UO_SE_DECKS ?? 24);
const NB6A = Number(process.env.UO_SE_B6A ?? 8);
const STEPS = Number(process.env.UO_SE_STEPS ?? 40);
const SOAK = Number(process.env.UO_SE_SOAK ?? 300);
const wasmPath = process.env.UO_WASM_PATH ?? join(core, 'target/w4-editor/uo-slides-editor-count.wasm');
const pptPath = process.env.UO_PPT_WASM_PATH ?? join(core, 'target/w4-editor/uo-slides-ppt.wasm');
const outDir = join(process.env.CARGO_TARGET_DIR ?? join(core, 'target'), 'slides-editor');
const out = join(outDir, 'out');
const fwd = (p) => p.replace(/\\/g, '/');
rmSync(out, { recursive: true, force: true });
mkdirSync(join(out, 'chunks'), { recursive: true });

// ── the bundle: the settings and the alias table of tools/build-pkg.mjs buildProduct ─────────────────
const W = (p) => fwd(join(repo, p));
writeFileSync(join(outDir, 'entry.mjs'), `
import { UniversalSlidesElement } from '${W('slides/packages/slides/src/index.ts')}';
export { UniversalSlidesElement };
// Slides wave 5: TS against Rust runs every TS switch (SlidesRules::ts_strict); the product default is PowerPoint's rules
UniversalSlidesElement.configureEngine({ tsStrict: true });
export { PptxPackage } from '${W('slides/packages/slides-core/src/index.ts')}';
const statsUrl = new URL('./stats.js', import.meta.url).href;
export const engineModule = () => import(statsUrl);
`);
writeFileSync(join(outDir, 'stats.mjs'), `export { slidesModuleLoaded, slidesHandles, slidesLiveBytes, slidesMemoryBytes, __trapForTest } from '${W('core/crates/uo-wasm/js/slides-engine.ts')}';
export { editSessions } from '${W('core/crates/uo-wasm/js/slides-editor-engine.ts')}';\n`);
const INTERNAL = {
  '@universal-office/fonts': 'shared/fonts/src/index.ts',
  '@universal-office/charts': 'shared/charts/src/index.ts',
  '@universal-office/drawingml': 'shared/drawingml/src/index.ts',
  '@universal-office/ui': 'shared/ui/src/index.ts',
  '@universal-office/slides-core': 'slides/packages/slides-core/src/index.ts',
  '@universal-office/slides-viewer': 'slides/packages/viewer/src/index.ts',
  '@universal-office/converter': 'converter/packages/converter/src/index.ts',
  '@universal-office/docs-processing': 'converter/packages/docs-processing/src/index.ts',
  '@universal-office/sheets-processing': 'converter/packages/sheets-processing/src/index.ts',
  '@universal-office/slides-processing': 'converter/packages/slides-processing/src/index.ts',
};
const alias = Object.fromEntries(Object.entries(INTERNAL).map(([n, f]) => [n, join(repo, f)]));
alias['@universal-office/slides-engine'] = join(repo, 'core/crates/uo-wasm/js/slides-engine.ts');
alias['@universal-office/slides-editor-engine'] = join(repo, 'core/crates/uo-wasm/js/slides-editor-engine.ts');
const esbuildPath = join(root, 'write/node_modules/esbuild/lib/main.js');
const esbuild = createRequire(join(root, 'write/package.json'))(existsSync(esbuildPath) ? esbuildPath : 'esbuild');
const built = await esbuild.build({
  entryPoints: { entry: join(outDir, 'entry.mjs'), stats: join(outDir, 'stats.mjs') },
  outdir: out, bundle: true, format: 'esm', platform: 'browser', target: 'es2022', splitting: true, chunkNames: 'chunks/[name]-[hash]',
  minify: true, legalComments: 'none', alias, absWorkingDir: repo, external: ['pdfjs-dist', 'pdfjs-dist/*'],
  nodePaths: [join(root, 'write/node_modules'), join(root, 'slides/node_modules'), join(root, 'sheet/node_modules')], logLevel: 'warning', metafile: true,
});
copyFileSync(wasmPath, join(out, 'chunks', 'uo-slides-editor.wasm'));
if (existsSync(pptPath)) copyFileSync(pptPath, join(out, 'chunks', 'uo-slides-ppt.wasm'));

const outs = built.metafile.outputs;
const closure = (o0) => { const seen = new Set(); const visit = (o) => { if (seen.has(o) || !outs[o]) return; seen.add(o); for (const i of outs[o].imports) if (i.kind === 'import-statement') visit(i.path); }; visit(o0); return seen; };
const entryOut = Object.keys(outs).find((o) => /(^|\/)entry\.js$/.test(fwd(o)));
const staticSet = closure(entryOut);
const staticInputs = [...staticSet].flatMap((o) => Object.keys(outs[o].inputs)).map((f) => fwd(resolve(repo, f)));
const leaked = staticInputs.filter((f) => /core\/crates\/uo-wasm\/js\/|\/rust-editor\.ts$|\/rust-engine\.ts$/.test(f));
const kb = (n) => (n / 1024).toFixed(0);
console.log(`bundle: entry static closure ${staticSet.size} output(s) ${kb([...staticSet].reduce((n, o) => n + outs[o].bytes, 0))} KB; uo-slides-editor.wasm ${kb(readFileSync(wasmPath).length)} KB`);
let failures = 0;
const fail = (m) => { failures++; console.log(`FAIL ${m}`); };
if (leaked.length) fail(`the entry's static closure holds the engines' code: ${leaked.map((f) => f.replace(`${fwd(repo)}/`, '')).join(', ')}`);

// ── the decks ──────────────────────────────────────────────────────────────────────────────────────
const tracked = execFileSync('git', ['-C', repo, 'ls-files', 'slides/samples'], { encoding: 'utf8' }).split('\n').map((s) => s.trim()).filter((s) => /\.pptx$/i.test(s));
const group = (g, n) => { const d = join(core, 'crates/uo-pptx/tests/captures/decks', g); return existsSync(d) ? readdirSync(d).filter((f) => f.endsWith('.pptx')).sort().slice(0, n).map((f) => join(d, f)) : []; };
const decks = [...tracked.map((p) => join(repo, p)), ...group('b1a', NB1A), ...group('b6a', NB6A)];
const deckByName = new Map(decks.map((p, i) => [`k${i}${extname(p)}`, p]));
const FONT_DIRS = [join(repo, 'shared/fonts/files'), join(repo, 'site/assets/fonts')];
const requests = [];
const MIME = { '.js': 'text/javascript', '.wasm': 'application/wasm', '.ttf': 'font/ttf', '.json': 'application/json', '.png': 'image/png' };
// a 2x2 PNG for insertImage
const PNG = Buffer.from('89504e470d0a1a0a0000000d49484452000000020000000208060000007224d1d40000001649444154789c63f8cfc0f01f0418c0c0c0f0ff3f0300209a04fc1b3a0e9d0000000049454e44ae426082', 'hex');
const server = createServer((req, res) => {
  const url = decodeURIComponent(req.url.split('?')[0]);
  requests.push(url);
  const send = (file) => res.writeHead(200, { 'content-type': MIME[extname(file)] ?? 'application/octet-stream' }).end(readFileSync(file));
  if (url === '/') return res.writeHead(200, { 'content-type': 'text/html' }).end('<!doctype html><meta charset="utf-8"><body style="margin:0"><script type="module">import * as e from "/out/entry.js"; window.__e = e; window.__ready = true;</script></body>');
  if (url === '/img.png') return res.writeHead(200, { 'content-type': 'image/png' }).end(PNG);
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
const browser = await chromium.launch({ headless: true, executablePath: process.env.UO_CHROMIUM ?? installed });
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

// the page-side driver (a function evaluated in the page)
const driverFn = () => {
  const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
  const El = window.__e.UniversalSlidesElement;
  const Pkg = window.__e.PptxPackage;
  const fnv = (u8) => { let h = 0x811c9dc5; for (let i = 0; i < u8.length; i++) h = Math.imul(h ^ u8[i], 0x01000193); return (h >>> 0).toString(16); };
  const mk = (engine) => {
    const el = new El();
    el.setAttribute('fonts', '/fonts');
    if (engine === 'rust') el.setAttribute('engine', 'rust');
    el.style.cssText = 'display:block;width:960px;height:600px;position:absolute;left:0;top:0';
    document.body.appendChild(el);
    return el;
  };
  const settle = async (el, wait = 4000) => {
    await el.fontsReady();
    await el.rustReady();
    const t0 = performance.now();
    while (performance.now() - t0 < wait && [...el.imageCache.values()].some((e) => !e.ready)) await sleep(20);
    await sleep(30);
    await el.fontsReady();
    await new Promise((r) => requestAnimationFrame(() => requestAnimationFrame(r)));
  };
  const pixels = (cv) => cv.getContext('2d').getImageData(0, 0, cv.width, cv.height).data;
  const diff = (a, b) => {
    if (a.length !== b.length) return -1;
    let n = 0;
    for (let i = 0; i < a.length; i += 4) if (a[i] !== b[i] || a[i + 1] !== b[i + 1] || a[i + 2] !== b[i + 2] || a[i + 3] !== b[i + 3]) n++;
    return n;
  };
  const render = (el, i) => { const cv = document.createElement('canvas'); el.renderSlideToCanvas(i, cv, 960); return pixels(cv); };
  /** the element's package: names in order and a digest of each part */
  const pkgFacet = (el) => { const p = el.pkg; if (!p) return '-'; return [...p.partNames()].map((n) => `${n}=${fnv(p.getPart(n))}`).join('|'); };
  const savedFacet = (bytes) => { const p = Pkg.load(bytes); return [...p.partNames()].map((n) => `${n}=${fnv(p.getPart(n))}`).join('|'); };
  const state = (el) => {
    const i = el.getCurrentSlide ? el.getCurrentSlide() : el.index;
    const sel = el.getSelectedShape();
    return JSON.stringify({
      i, n: el.getSlideCount(), sel, u: el.canUndo(), r: el.canRedo(), d: el.isDirty(), info: el.getShapeInfo(), fmt: el.getTextFormat(),
      notes: el.getSlideNotes(i), emb: el.getFontEmbedding(), tr: el.getSlideTransition(i), hid: el.isSlideHidden(i), bg: el.getSlideBackgroundColor(i),
      props: el.getProperties(), show: el.getShowSettings(), editing: el.getEditingShape(),
    });
  };
  // xorshift32 (the scripts are the same for both elements: generated from the TS element's state)
  const rng = (seed) => { let x = seed >>> 0 || 1; const next = () => { x ^= x << 13; x >>>= 0; x ^= x >>> 17; x ^= x << 5; x >>>= 0; return x; }; return { below: (n) => (n <= 0 ? 0 : next() % n), pick: (a) => a[next() % a.length] }; };
  const COLORS = ['#FF0000', '#00B050', '#1F4E79', '#FFC000', null];
  const TEXTS = ['Olá mundo', 'Line one\nLine two', '', 'Ação & <tag>', 'x'];
  const PRESETS = ['rect', 'ellipse', 'roundRect', 'rightArrow', 'line', 'star5'];
  let png = null;

  /** one step for both elements: returns the description of the step */
  const step = async (ts, rs, r, clip) => {
    const n = ts.getSlideCount();
    const shapes = (el) => (el.slides[el.index]?.shapes ?? []).map((s, k) => ({ s, k })).filter(({ s }) => !s.inherited);
    const own = shapes(ts);
    const pickShape = () => (own.length ? r.pick(own).k : null);
    // both elements, each in its own try: a command that throws in one engine must throw in the other too
    const both = async (f) => {
      let a, b, ea = null, eb = null;
      try { a = await f(ts); } catch (e) { ea = String(e?.message ?? e); }
      try { b = await f(rs); } catch (e) { eb = String(e?.message ?? e); }
      if (ea !== null || eb !== null) throw Object.assign(new Error(`ts: ${ea} | rust: ${eb}`), { mismatch: (ea === null) !== (eb === null) });
      return [a, b];
    };
    const choices = [
      [3, async () => { const k = pickShape(); await both((el) => el.selectShape(k)); return `select ${k}`; }],
      [2, async () => { await both((el) => el.addSlide()); return 'addSlide'; }],
      [1, async () => { if (!n) return null; const i = r.below(n); await both((el) => el.duplicateSlide(i)); return `duplicateSlide ${i}`; }],
      [1, async () => { if (n < 2) return null; const i = r.below(n); await both((el) => el.deleteSlide(i)); return `deleteSlide ${i}`; }],
      [1, async () => { if (n < 2) return null; const a = r.below(n), b = r.below(n); await both((el) => el.moveSlide(a, b)); return `moveSlide ${a} ${b}`; }],
      [1, async () => { if (!n) return null; const h = !!r.below(2); await both((el) => el.setSlideHidden(h)); return `hide ${h}`; }],
      [1, async () => { const l = ts.getLayouts(); if (!l.length || !n) return null; const p = r.pick(l).path; await both((el) => el.setSlideLayout(p)); return `layout ${p}`; }],
      [1, async () => { const c = r.pick(COLORS); const t = r.below(4) ? undefined : 'all'; await both((el) => el.setSlideBackground(c, t)); return `background ${c} ${t}`; }],
      [1, async () => { const t = r.below(5) ? { type: r.pick(['fade', 'push', 'wipe', 'split', 'cover']), dir: r.pick(['l', 'r', 'u', 'd', undefined]), speed: r.pick(['fast', 'med', 'slow']), advanceOnClick: r.pick([true, false]), ...(r.below(2) ? { advanceAfterMs: 2500 } : {}) } : null; const at = r.below(4) ? undefined : 'all'; await both((el) => el.setSlideTransition(t, at)); return `transition ${JSON.stringify(t)} ${at}`; }],
      [1, async () => { const s = r.pick([{ loop: true }, { useTimings: false }, { showAnimation: false, kiosk: true }]); await both((el) => el.setShowSettings(s)); return `show ${JSON.stringify(s)}`; }],
      [3, async () => { const t = r.pick(TEXTS); await both((el) => el.setShapeText(t)); return `text ${JSON.stringify(t)}`; }],
      [3, async () => { const f = r.pick([{ bold: true }, { italic: true, size: 28 }, { font: 'Carlito' }, { color: '#C00000', underline: true }, { strike: true, size: 11 }]); await both((el) => el.formatText(f)); return `format ${JSON.stringify(f)}`; }],
      [1, async () => { const k = r.pick(['bold', 'italic', 'underline', 'strike']); await both((el) => el.toggleTextFormat(k)); return `toggle ${k}`; }],
      [1, async () => { const d = r.pick([1, -1]); await both((el) => el.growFont(d)); return `grow ${d}`; }],
      [1, async () => { const a = r.pick(['l', 'ctr', 'r', 'just']); await both((el) => el.alignText(a)); return `align ${a}`; }],
      [1, async () => { const b = r.pick(['bullet', 'number', 'none']); await both((el) => el.setBullets(b)); return `bullets ${b}`; }],
      [1, async () => { await both((el) => el.clearFormatting()); return 'clearFormatting'; }],
      [1, async () => { const u = r.pick(['https://example.com/', null]); await both((el) => el.setLink(u)); return `link ${u}`; }],
      [2, async () => { const t = r.pick(TEXTS); await both((el) => el.insertTextBox(t)); return `textBox ${JSON.stringify(t)}`; }],
      [2, async () => { const p = r.pick(PRESETS); await both((el) => el.insertShape(p)); return `shape ${p}`; }],
      [1, async () => { png ??= new Uint8Array(await (await fetch('/img.png')).arrayBuffer()); await both((el) => el.insertImage(png, 'image/png')); return 'image'; }],
      [2, async () => { const f = { x: 12700 * r.below(400), y: 12700 * r.below(300), cx: 12700 * (20 + r.below(300)), cy: 12700 * (10 + r.below(200)) }; await both((el) => el.setShapeFrame(f)); return `frame ${JSON.stringify(f)}`; }],
      [1, async () => { const c = r.pick(COLORS); await both((el) => el.setShapeFill(c)); return `fill ${c}`; }],
      [1, async () => { const c = r.pick(COLORS); await both((el) => el.setShapeOutline(c)); return `outline ${c}`; }],
      [1, async () => { const m = r.pick(['front', 'back', 'forward', 'backward']); await both((el) => el.arrangeShape(m)); return `arrange ${m}`; }],
      [1, async () => { await both((el) => el.deleteShape()); return 'deleteShape'; }],
      [1, async () => { const [a, b] = await both((el) => el.copyShape()); clip.ts = a; clip.rs = b; return `copy ${!!a}`; }],
      [1, async () => { if (!clip.ts || !clip.rs) return null; await ts.pasteShape(clip.ts); await rs.pasteShape(clip.rs); return 'paste'; }],
      [1, async () => { await both((el) => el.duplicateShape()); return 'duplicateShape'; }],
      [1, async () => { if (!n) return null; const t = r.pick(TEXTS); await both((el) => el.setSlideNotes(t)); return `notes ${JSON.stringify(t)}`; }],
      [1, async () => { const p = r.pick([{ title: 'T & <1>' }, { author: 'Eric', subject: 'S' }, { keywords: 'a,b' }]); await both((el) => el.setProperties(p)); return `props ${JSON.stringify(p)}`; }],
      [1, async () => { const q = r.pick(['e', 'Line', 'o']); const rep = r.pick(['X', '', 'ção']); const mc = !!r.below(2); await both((el) => el.replaceText(q, rep, { matchCase: mc })); return `replace ${q}`; }],
      [2, async () => {
        const info = ts.getShapeInfo();
        if (!info?.text) return null;
        const t = r.pick(TEXTS);
        // the overlay commits on blur (its own handler); commitEditShape() while it has the focus would commit twice (a TS quirk)
        await both((el) => { el.beginEditShape(info.index); if (!el.editor) return; el.editor.innerText = t; el.editor.blur(); if (el.getEditingShape() !== null) el.commitEditShape(); });
        return `typing ${JSON.stringify(t)}`;
      }],
      [3, async () => { await both((el) => el.undo()); return 'undo'; }],
      [2, async () => { await both((el) => el.redo()); return 'redo'; }],
      [1, async () => { await both((el) => el.markClean()); return 'markClean'; }],
      [1, async () => { if (!n) return null; const i = r.below(n); await both((el) => el.selectSlide(i)); return `goto ${i}`; }],
      [1, async () => { const e = !!r.below(2), s = !!r.below(2); await both((el) => el.setFontEmbedding({ embed: e, subset: s })); return `embed ${e} ${s}`; }],
    ];
    const total = choices.reduce((s, [w]) => s + w, 0);
    for (let tries = 0; tries < 20; tries++) {
      let x = r.below(total);
      for (const [wgt, f] of choices) {
        if (x < wgt) {
          let errA = null, errB = null;
          try { const d = await f(); if (d === null) break; return { d }; } catch (e) { errA = String(e?.message ?? e); errB = !!e?.mismatch; }
          return { d: 'error', errA, errB };
        }
        x -= wgt;
      }
    }
    await both((el) => el.undo());
    return { d: 'undo' };
  };

  /** one deck: both elements, `steps` scripted steps, everything compared after each */
  const session = async (name, seed, steps) => {
    const ts = mk('ts'), rs = mk('rust');
    const res = { name, engine: 'ts', steps: 0, throwDiff: 0, pkgDiff: 0, stateDiff: 0, pixelDiff: 0, mainDiff: 0, saveDiff: 0, saves: 0, errors: [], first: null };
    const errs = [];
    rs.addEventListener('us-engine-error', (e) => errs.push(String(e.detail?.error?.message ?? e.detail?.error)));
    try {
      const bytes = new Uint8Array(await (await fetch(`/decks/${name}`)).arrayBuffer());
      ts.loadPptx(bytes); rs.loadPptx(bytes);
      await settle(ts); await settle(rs);
      res.engine = rs.getEngine();
      if (res.engine !== 'rust') { res.errors.push(...errs); return res; }
      // the creation ids of copies (`freshCreationId`, the only consumer of Math.random in the editing code): the TS takes
      // Math.random, the Rust session its own xorshift32; both start from seed 1 (the B8 capture rule)
      let xs = 1;
      Math.random = () => { xs ^= xs << 13; xs >>>= 0; xs ^= xs >>> 17; xs ^= xs << 5; xs >>>= 0; return xs / 4294967296; };
      rs.rx.setRandomSeed(1);
      const r = rng(seed);
      const clip = { ts: null, rs: null };
      const check = async (k, d) => {
        const pa = pkgFacet(ts), pb = pkgFacet(rs);
        if (pa !== pb) {
          res.pkgDiff++;
          if (!res.first) {
            const A = pa.split('|'), B = pb.split('|');
            res.first = { k, d, what: 'package', onlyTs: A.filter((x) => !B.includes(x)).slice(0, 6), onlyRust: B.filter((x) => !A.includes(x)).slice(0, 6), order: A.length === B.length && A.every((x) => B.includes(x)) };
            const n = res.first.onlyTs[0]?.split('=')[0];
            if (n) {
              const dec = new TextDecoder();
              const a = dec.decode(ts.pkg.getPart(n) ?? new Uint8Array()), b = dec.decode(rs.pkg.getPart(n) ?? new Uint8Array());
              let at = 0;
              while (at < a.length && a[at] === b[at]) at++;
              res.first.at = at;
              res.first.tsText = a.slice(Math.max(0, at - 120), at + 300);
              res.first.rustText = b.slice(Math.max(0, at - 120), at + 300);
            }
          }
        }
        const sa = state(ts), sb = state(rs);
        if (sa !== sb) { res.stateDiff++; res.first ??= { k, d, what: 'state', sa, sb }; }
        await settle(ts, 800); await settle(rs, 800);
        const n = ts.getSlideCount();
        if (n) {
          const i = Math.min(ts.index, n - 1);
          const px = diff(render(ts, i), render(rs, i));
          if (px !== 0) { res.pixelDiff++; res.first ??= { k, d, what: `pixels ${px} on slide ${i}` }; }
        }
        ts.paint(); rs.paint();
        const m = diff(pixels(ts.canvas), pixels(rs.canvas));
        if (m !== 0) { res.mainDiff++; res.first ??= { k, d, what: `main canvas ${m}` }; }
      };
      const save = async (k) => {
        const [a, b] = [await ts.savePptx(), await rs.savePptx()];
        res.saves++;
        if (savedFacet(a) !== savedFacet(b)) { res.saveDiff++; res.first ??= { k, what: 'saved package' }; }
      };
      await check(0, 'load');
      for (let k = 1; k <= steps; k++) {
        const s = await step(ts, rs, r, clip);
        res.steps++;
        if (s.d === 'error') { res.errors.push(`step ${k}: ${s.errA}`); if (s.errB) { res.throwDiff++; res.first ??= { k, what: 'only one engine threw', e: s.errA }; } }
        await check(k, s.d);
        if (k % 8 === 0) await save(k);
      }
      await save(steps);
      res.errors.push(...errs);
    } catch (e) {
      res.errors.push(`exception ${e?.stack ?? e}`);
    } finally {
      ts.remove(); rs.remove();
    }
    return res;
  };
  return { session, mk, settle, sleep, fnv, pkgFacet, state, diff, pixels, render };
};
const install = () => page.evaluate(`window.__d = (${driverFn.toString()})()`);
// UO_SE_LIMIT: only the first n decks (a quick run)
const names = [...deckByName.keys()].slice(0, Number(process.env.UO_SE_LIMIT ?? Infinity));

// ── phase 0 ──────────────────────────────────────────────────────────────────────────────────────────
if (MODE === 'all' || MODE === 'phase0') {
  await openPage(); await install();
  const before = requests.length;
  await page.evaluate(async (n) => {
    const el = window.__d.mk('ts');
    el.loadPptx(new Uint8Array(await (await fetch(`/decks/${n}`)).arrayBuffer()));
    await window.__d.settle(el);
    el.insertTextBox('x'); el.undo();
    el.remove();
  }, names[0]);
  const asked = requests.slice(before).filter((u) => /uo-slides|rust-engine|rust-editor|stats-/.test(u));
  if (asked.length) fail(`phase 0: a TS editor requested ${asked.join(', ')}`);
  else console.log('phase 0: an editor without engine="rust" requested neither the wasm nor the engine chunks (0 requests)');
}

// ── parity ──────────────────────────────────────────────────────────────────────────────────────────
if (MODE === 'all' || MODE === 'parity') {
  const totals = { decks: 0, rust: 0, steps: 0, throwDiff: 0, pkgDiff: 0, stateDiff: 0, pixelDiff: 0, mainDiff: 0, saves: 0, saveDiff: 0, errors: 0 };
  const t0 = Date.now();
  for (let k = 0; k < names.length; k++) {
    if (k % 6 === 0) { await openPage(); await install(); }
    const r = await page.evaluate(([n, seed, steps]) => window.__d.session(n, seed, steps), [names[k], 1000 + k, STEPS]);
    totals.decks++;
    if (r.engine === 'rust') totals.rust++;
    else fail(`${basename(deckByName.get(names[k]))}: engine ${r.engine} (${r.errors.join('; ')})`);
    for (const f of ['steps', 'throwDiff', 'pkgDiff', 'stateDiff', 'pixelDiff', 'mainDiff', 'saves', 'saveDiff']) totals[f] += r[f];
    const bad = r.throwDiff + r.pkgDiff + r.stateDiff + r.pixelDiff + r.mainDiff + r.saveDiff;
    if (bad) fail(`${basename(deckByName.get(names[k]))}: one-engine throws ${r.throwDiff}, package ${r.pkgDiff}, state ${r.stateDiff}, pixels ${r.pixelDiff}, main ${r.mainDiff}, saves ${r.saveDiff}; first ${JSON.stringify(r.first)}`);
    if (r.errors.length) { totals.errors += r.errors.length; console.log(`  ${basename(deckByName.get(names[k]))}: ${r.errors.slice(0, 3).join(' | ')}`); }
  }
  console.log(`parity: ${totals.decks} decks (${totals.rust} on the Rust engine), ${totals.steps} steps; after every step package ${totals.steps + totals.decks - totals.pkgDiff}/${totals.steps + totals.decks} equal, state ${totals.steps + totals.decks - totals.stateDiff}, slide pixels ${totals.steps + totals.decks - totals.pixelDiff}, main canvas ${totals.steps + totals.decks - totals.mainDiff}; saves ${totals.saves - totals.saveDiff}/${totals.saves} equal; ${totals.errors} step errors (both engines threw); ${((Date.now() - t0) / 1000).toFixed(0)} s`);
}

// ── memory ──────────────────────────────────────────────────────────────────────────────────────────
if (MODE === 'all' || MODE === 'memory') {
  await openPage(); await install();
  const r = await page.evaluate(async ([n, soak]) => {
    const d = window.__d;
    const stats = await window.__e.engineModule();
    const el = d.mk('rust');
    el.loadPptx(new Uint8Array(await (await fetch(`/decks/${n}`)).arrayBuffer()));
    await d.settle(el);
    if (el.getEngine() !== 'rust') return { error: 'not on the Rust engine' };
    const live = [], mem = [];
    for (let i = 0; i < soak; i++) {
      el.insertTextBox(`soak ${i % 5}`);
      el.formatText({ bold: i % 2 === 0 });
      el.setShapeFrame({ x: 12700 * (i % 40), y: 12700 * 20, cx: 12700 * 200, cy: 12700 * 50 });
      el.undo(); el.undo(); el.redo(); el.undo(); el.undo();
      el.paint();
      if (i % 50 === 49) { await d.sleep(5); live.push(stats.slidesLiveBytes()); mem.push(stats.slidesMemoryBytes()); }
    }
    el.remove();
    await d.sleep(20);
    // removing the element keeps the session (the TS rule: the document and its history survive a DOM move); loading another deck frees it
    const keep = stats.editSessions();
    document.body.appendChild(el);
    el.loadPptx(new Uint8Array(await (await fetch(`/decks/${n}`)).arrayBuffer()));
    await d.settle(el);
    el.setAttribute('engine', 'ts');
    await d.sleep(20);
    return { live, mem, keep, sessions: stats.editSessions(), handles: stats.slidesHandles() };
  }, [names[names.length - NB6A - 1] ?? names[0], SOAK]);
  if (r.error) fail(`memory: ${r.error}`);
  else {
    const tail = r.live.slice(2);
    const spread = Math.max(...tail) - Math.min(...tail);
    console.log(`memory: ${SOAK} edit/undo cycles: live bytes ${r.live.join(', ')} (spread after warm-up ${spread}); wasm buffer ${r.mem.join(', ')}; sessions while detached ${r.keep}, after switching to ts ${r.sessions}, handles ${r.handles}`);
    if (spread > 256 * 1024) fail(`memory: live bytes spread ${spread}`);
    if (r.mem[r.mem.length - 1] !== r.mem[2]) fail('memory: the wasm buffer keeps growing');
    if (r.sessions !== 0 || r.handles !== 0) fail(`memory: ${r.sessions} sessions, ${r.handles} handles left`);
  }
}

// ── faults ──────────────────────────────────────────────────────────────────────────────────────────
if (MODE === 'all' || MODE === 'faults') {
  await openPage(); await install();
  const r = await page.evaluate(async (n) => {
    const d = window.__d;
    const stats = await window.__e.engineModule();
    const out = {};
    const bytes = new Uint8Array(await (await fetch(`/decks/${n}`)).arrayBuffer());
    // 1. a DOM move keeps the Rust session and its history
    const el = d.mk('rust');
    el.loadPptx(bytes);
    await d.settle(el);
    el.insertTextBox('moved');
    const before = d.pkgFacet(el);
    el.remove();
    await d.sleep(10);
    document.body.appendChild(el);
    await d.settle(el);
    out.moveEngine = el.getEngine();
    out.moveCanUndo = el.canUndo();
    el.undo();
    out.moveUndone = d.pkgFacet(el) !== before && !el.canUndo();
    // 2. the engine attribute switched with the document open: the package moves to the other engine
    el.insertShape('rect');
    const pkgA = d.pkgFacet(el);
    el.setAttribute('engine', 'ts');
    out.toTs = el.getEngine() === 'ts' && d.pkgFacet(el) === pkgA;
    el.setAttribute('engine', 'rust');
    await d.settle(el);
    out.toRust = el.getEngine() === 'rust' && d.pkgFacet(el) === pkgA;
    // 3. a trap mid-session: the element drops the module and keeps editing on the TS engine from the same package
    const errs = [];
    el.addEventListener('us-engine-error', (e) => errs.push(String(e.detail?.error?.message ?? e.detail?.error)));
    try { stats.__trapForTest(); } catch { /* the trap */ }
    const pkgB = d.pkgFacet(el);
    try { el.insertTextBox('after the trap'); out.afterTrap = d.pkgFacet(el) !== pkgB && el.getEngine() === 'ts'; } catch (e) { out.afterTrap = `threw ${e.message}`; }
    out.trapEvent = errs.length > 0;
    el.remove();
    return out;
  }, names[0]);
  console.log(`faults: ${JSON.stringify(r)}`);
  if (r.moveEngine !== 'rust' || !r.moveCanUndo || !r.moveUndone) fail('faults: the session or its history did not survive a DOM move');
  if (!r.toTs || !r.toRust) fail('faults: the engine switch lost the package');
  if (r.afterTrap !== true || !r.trapEvent) fail(`faults: after a trap ${r.afterTrap} (event ${r.trapEvent})`);
}

await browser.close();
server.close();
console.log(failures ? `${failures} FAILURE(S)` : 'slides_edit_switch: ok');
process.exit(failures ? 1 : 0);
