// Painter parity of uo-render frontier V: the TS painter of decoded pages (js/paint.ts, frontier D) against the frozen canvas
// painter (canvas.ts), on a recording context (tests/support/recording-context.mjs).
//
//   Part A (always; needs no frozen TS): the committed fixture documents laid out by the wasm engine (open fonts, all
//     hyphenation patterns, no `hyphenationLanguages`), every page encoded -> decoded -> painted by the D painter in the four
//     configurations of paint-common.mjs; the digest of each document's draw-call log must equal the capture of the frozen
//     painter (tests/captures/v_paint.capture, taken by tools/capture-v.mjs on win32 Node 24.15).
//   Part B (UO_TS_ROOT): call-by-call diff on the generated pages of uo-pdf's `genSpec` (every painter branch) through the TS
//     adapter (R-P2c: frozen LayoutResult -> render.uos -> the Rust view -> encodePage -> decodePage -> the D painter), and,
//     with UO_V_ORACLE=1, on the Word oracle documents laid out by the frozen engine with the installed fonts.
//   Part C (UO_TS_ROOT): the export bytes against the frozen functions on the same items.
//
//   cargo build -p uo-wasm --features count,write,render-fixture --target wasm32-unknown-unknown --profile wasm
//   UO_WASM_PATH=<that .wasm> node --expose-gc --test crates/uo-wasm/tests/paint_parity.test.mjs
//   UO_TS_ROOT=<frozen> UO_V_SEEDS=500 UO_V_ORACLE=1 node --max-old-space-size=8192 crates/uo-wasm/tests/paint_parity.test.mjs
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync, readdirSync } from 'node:fs';
import { join, dirname } from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';
import { Stream, readCapture } from '../../uo-testkit/js/testkit.mjs';
import { writeImports, bindWrite } from '../js/write.mjs';
import { PaintCaches } from '../js/paint.ts';
import { decodePage, newTables } from '../js/gen/render.ts';
import { adaptLayout, pushInto } from '../../uo-render/tools/lib/ts-adapter.mjs';
import { Recording, DigestSink, ListSink, bitmapOf, fontOf, colorOf } from './support/recording-context.mjs';
import { CONFIGS, NOW, core, repo, fixtureDocs, relDoc, openEnv, wasmOptions } from './support/paint-common.mjs';
import { envD, decodedPages, paintPageD } from './support/paint-d.mjs';

const here = dirname(fileURLToPath(import.meta.url));
const wasmPath = process.env.UO_WASM_PATH ?? join(here, '../../../target/wasm32-unknown-unknown/wasm/uo_wasm.wasm');
const { imports, host } = writeImports();
const { instance } = await WebAssembly.instantiate(readFileSync(wasmPath), imports);
const uo = bindWrite(instance.exports, host);
const hasWrite = typeof instance.exports.uo_write_open === 'function';
const skip = hasWrite ? false : 'this build has no write exports';
const capturePath = join(core, 'crates/uo-render/tests/captures/v_paint.capture');

test('Part A: the D painter equals the frozen painter on the committed fixture documents', { skip }, (t) => {
  const cap = readCapture(capturePath);
  // the replay depends only on the Rust hints/exports (wasm) and on Math.round/hypot/sin/cos, none of which depends on the OS:
  // the capture (taken once on win32) is valid on any platform (run on Linux/WSL too, see the record)
  const env = openEnv(uo);
  const rec = new Recording(null).install();
  const caches = new PaintCaches();
  const t0 = performance.now();
  let docs = 0, pages = 0, items = 0, calls = 0;
  const mismatches = [];
  try {
    for (const file of fixtureDocs()) {
      const key = relDoc(file);
      const want = cap.records.get(key);
      assert.ok(want, `${key} is in the capture`);
      const doc = uo.open(readFileSync(file));
      const layout = doc.layout(env, wasmOptions());
      const pp = decodedPages(layout);
      const denv = envD((id) => layout.imageBytes(id));
      const streams = CONFIGS.map(() => { const s = new Stream(); return { s, sink: new DigestSink(s) }; });
      let docItems = 0;
      pp.forEach((p0, pi) => {
        let p = p0;
        docItems += p0.page.items.length;
        CONFIGS.forEach((cfg, ci) => {
          const { s, sink } = streams[ci];
          s.str('page'); s.u32(pi);
          rec.sink = sink; rec.kerning = cfg.kerning; rec.scale = cfg.scale;
          // UO_V_NEGATIVE=1 perturbs the painter on purpose: the test must then fail (the digests are sensitive)
          if (process.env.UO_V_NEGATIVE) p = { ...p, page: { ...p.page, items: p.page.items.map((it, k) => (k === 3 && it.t === 'glyphs' ? { ...it, y: it.y + 1 } : it)) } };
          paintPageD(rec.pageContext(), p, denv, cfg, caches);
        });
      });
      if (String(pp.length) !== want.get('pages')) mismatches.push(`${key}: pages ${pp.length} != ${want.get('pages')}`);
      if (String(docItems) !== want.get('items')) mismatches.push(`${key}: items ${docItems} != ${want.get('items')}`);
      CONFIGS.forEach((cfg, ci) => {
        const got = `${streams[ci].sink.calls}:${streams[ci].s.finish().short}`;
        if (got !== want.get(cfg.name)) mismatches.push(`${key} ${cfg.name}: ${got} != ${want.get(cfg.name)}`);
      });
      docs++; pages += pp.length; items += docItems; calls += streams[0].sink.calls;
      layout.free(); doc.free();
    }
  } finally {
    rec.uninstall();
    env.free();
  }
  const ms = Math.round(performance.now() - t0);
  console.log(`Part A: ${docs} documents, ${pages} pages, ${items} items, ${calls} draw calls (k1s1) x ${CONFIGS.length} configurations, ${mismatches.length} mismatches, ${ms} ms (${globalThis.navigator?.userAgent ?? process.version})`);
  for (const m of mismatches.slice(0, 20)) console.log('  MISMATCH', m);
  assert.equal(String(docs), cap.metas.get('documents'));
  assert.equal(String(pages), cap.metas.get('pages'));
  assert.equal(String(items), cap.metas.get('items'));
  assert.equal(mismatches.length, 0, 'every digest equals the frozen painter\'s');
});

// ───────────────────────────────────────────── Parts B and C (UO_TS_ROOT) ─────────────────────────────────────────────

const tsRoot = process.env.UO_TS_ROOT;
const skipTs = !hasWrite ? 'this build has no write exports' : tsRoot ? false : 'set UO_TS_ROOT to the frozen TS checkout for the call-by-call parity';

let F;
/** the frozen modules (canvas painter, geometry, text effects, page fills, images) and the generated corpus of uo-pdf */
async function frozen() {
  if (F) return F;
  const { importFrom } = await import('../../uo-testkit/js/testkit.mjs');
  const D = 'write/packages/docx-layout/dist/';
  const [canvas, geometry, textfx, pageFill, images] = await Promise.all(['canvas', 'geometry', 'textfx', 'page-fill', 'images'].map((m) => importFrom(tsRoot, `${D}${m}.js`)));
  const cp = await import(pathToFileURL(join(core, 'crates/uo-pdf/tests/parity_support/capture_painter.mjs')).href);
  F = { canvas, geometry, textfx, pageFill, images, genSpec: cp.genSpec, materialize: cp.materialize };
  return F;
}

const jn = (v) => JSON.stringify(v, (k, x) => (typeof x === 'number' && !Number.isFinite(x) ? `#${x}` : Object.is(x, -0) ? '#-0' : x));
const HINTS = ['joins', 'remap', 'outlines', 'box', 'fxFillGeom', 'fxOutlineGeom', 'parts', 'gradGeom', 'gradSamples', 'text'];
const strip = (page) => { delete page.app; for (const it of page.items) for (const k of HINTS) delete it[k]; return page; };

/** the first differing entry of two logs: `{ i, a, b, context }` or null */
function diffLogs(a, b) {
  const n = Math.min(a.length, b.length);
  for (let i = 0; i < n; i++) if (jn(a[i]) !== jn(b[i])) return { i, a: a[i], b: b[i], context: a.slice(Math.max(0, i - 3), i) };
  if (a.length !== b.length) return { i: n, a: a[n], b: b[n], context: a.slice(Math.max(0, n - 3), n) };
  return null;
}

/** paints with `fn(ctx)` on `rec` in configuration `cfg`; the log and the exception, if any */
function runSide(rec, cfg, fn) {
  const sink = new ListSink();
  rec.sink = sink; rec.kerning = cfg.kerning; rec.scale = cfg.scale;
  rec.install();
  let err = null;
  try { fn(rec.pageContext()); } catch (e) { err = e; } finally { rec.uninstall(); }
  return { log: sink.log, err };
}

const newStats = () => ({
  label: '', docs: 0, pages: 0, items: 0, calls: 0, logMismatch: 0, errMismatch: 0, bothThrow: 0, wireDiffs: 0, wireMsgs: [], mismatches: [], skipped: 0,
  kinds: {}, facets: {},
});
const facet = (S, name) => (S.facets[name] ??= { equal: 0, mismatch: 0, bothNull: 0, refused: 0, frozenOnly: 0, rustOnly: 0, samples: [] });

/** R-P2c: the adapted messages against what the Rust view re-encodes (decoded by the TS decoder, hints stripped) */
function checkWire(view, a) {
  const tA = newTables(), tB = newTables();
  const diffs = [], pages = [];
  for (let i = 0; i < a.messages.length; i++) {
    const pa = strip(decodePage(a.messages[i], tA));
    const { status, data } = view.encodePage(i).take();
    if (status !== 0) { diffs.push(`page ${i}: the view refused to encode (status ${status})`); pages.push(null); continue; }
    const full = decodePage(data, tB);
    pages.push(full);
    const pb = strip(structuredClone(full));
    if (jn(pa) !== jn(pb)) {
      const k = pa.items.findIndex((it, j) => jn(it) !== jn(pb.items[j]));
      diffs.push(`page ${i}: item ${k} differs: ${jn(pa.items[k]).slice(0, 240)} | ${jn(pb.items[k]).slice(0, 240)}`);
    }
  }
  for (const t of ['faces', 'images', 'textFx', 'pageFills', 'cust']) if (jn(tA[t]) !== jn(tB[t])) diffs.push(`table ${t} differs: ${jn(tA[t]).slice(0, 200)} | ${jn(tB[t]).slice(0, 200)}`);
  return { pages, tables: tB, diffs };
}

const same = (x, y) => (x instanceof Uint8Array || x instanceof Uint8ClampedArray) && (y instanceof Uint8Array || y instanceof Uint8ClampedArray)
  ? x.length === y.length && x.every((v, i) => v === y[i]) : jn(x) === jn(y);

/** Part C: the export bytes against the frozen functions on the items of one layout */
function exportFacets(f, result, view, rasters, S, label) {
  const g = f.geometry, tfx = f.textfx, pf = f.pageFill;
  const cmp = (name, frozenFn, exportFn, norm) => {
    const F_ = facet(S, name);
    let a = null, b = null, bErr = null;
    try { a = frozenFn(); } catch (e) { a = { threw: String(e?.message ?? e) }; }
    try { b = exportFn(); } catch (e) { bErr = e; }
    if (bErr) { F_.refused++; if (F_.samples.length < 3) F_.samples.push(`${label}: ${bErr.message}`); return; }
    if (a === null && b === null) { F_.bothNull++; return; }
    if (a?.threw) { F_.frozenOnly++; return; }
    if (a === null || b === null) { (a === null ? F_.rustOnly++ : F_.frozenOnly++); if (F_.samples.length < 3) F_.samples.push(`${label}: ${a === null ? 'only the export answers' : 'only the frozen function answers'}`); return; }
    if (same(norm(a), norm(b))) F_.equal++; else { F_.mismatch++; if (F_.samples.length < 3) F_.samples.push(`${label}: bytes differ`); }
  };
  const naturalOf = (img) => f.images.imagePixelSize(img.data);
  result.pages.forEach((page, pi) => {
    if (page.backgroundFill) {
      const fl = page.backgroundFill;
      cmp('pageFillTileTw', () => pf.pageFillTileTw(fl, 15, 15), () => rasters.pageFillTileTw(pi, 15), (x) => ({ w: x.w, h: x.h }));
      if (fl.type === 'gradient' || fl.type === 'gradientRadial') {
        cmp('gradientTileRgba', () => { const t = pf.gradientTilePx(fl, false); return { w: t.w, h: t.h, data: pf.gradientTileRgba(fl, t.w, t.h) }; }, () => rasters.pageFillRaster(pi), (x) => x);
      } else if (fl.type === 'pattern') cmp('patternRgba', () => ({ w: 8, h: 8, data: pf.patternRgba(fl) }), () => rasters.pageFillRaster(pi), (x) => x);
    }
    page.items.forEach((it, k) => {
      if (it.t === 'glyphs' && it.fx) {
        for (const ppt of [1.5, 3.3333]) {
          cmp(`fxLayers@${ppt}`, () => tfx.fxLayers(it, it.fx, ppt, it.color), () => rasters.fxLayer(pi, k, ppt), (x) => ({ x0: x.x0, y0: x.y0, w: x.w, h: x.h, data: new Uint8Array(x.data) }));
        }
      } else if (it.t === 'shape') {
        const local = !!it.fx && it.style.geom !== 'line';
        const [x, y] = local ? [0, 0] : [it.x, it.y];
        if (it.pattern) {
          cmp('patternRaster', () => g.patternRaster({ ...it.pattern }, x, y, it.w, it.h, 15), () => rasters.pattern(pi, k),
            (r) => ({ x: r.x, y: r.y, cw: r.cells?.w ?? r.cellsW, ch: r.cells?.h ?? r.cellsH, w: r.w, h: r.h, data: new Uint8Array(r.data) }));
        } else if (it.blip) {
          cmp('blipTiles', () => g.blipTiles(it.blip, x, y, it.w, it.h, 1 / 635, naturalOf(it.blip.image)), () => rasters.blipTiles(pi, k), (r) => r.map((t) => [t.x, t.y, t.w, t.h]));
        } else if (it.grad && g.isRectGradient(it.grad)) {
          cmp('rectGradientRaster', () => g.rectGradientRaster(it.grad, it.w / 20, it.h / 20, 2), () => rasters.rectGradient(pi, k), (r) => ({ w: r.w, h: r.h, data: new Uint8Array(r.data) }));
        }
      }
    });
  });
}

/** one frozen layout through the adapter, the Rust view and both painters */
function processResult(f, label, result, S, { facets = true } = {}) {
  const a = adaptLayout(result, { imagePixelSize: (d) => { const n = f.images.imagePixelSize(d); return n ? { w: n.width, h: n.height } : undefined; } });
  const view = uo.render.newView(1);
  S.docs++;
  try {
    pushInto(view, a);
    const { pages: dpages, tables, diffs } = checkWire(view, a);
    S.wireDiffs += diffs.length;
    for (const d of diffs.slice(0, 2)) if (S.wireMsgs.length < 8) S.wireMsgs.push(`${label}: ${d}`);
    const rasters = uo.render.rasters(view.paint);
    const denv = envD((id) => a.images[id]?.bytes ?? null);
    const fenv = { fontOf, image: (img) => bitmapOf(img.data), color: colorOf };
    result.pages.forEach((page, pi) => {
      S.pages++; S.items += page.items.length;
      for (const it of page.items) S.kinds[it.t] = (S.kinds[it.t] ?? 0) + 1;
      const dp = dpages[pi];
      if (!dp) return;
      const p = { page: dp, tables, index: pi, rasters, owner: view };
      for (const cfg of CONFIGS) {
        const A = runSide(S.recF, cfg, (ctx) => {
          f.canvas.paintPageBackground(ctx, page, fenv, { devicePxTw: 1 / cfg.scale, screen: cfg.screen, paper: '#ffffff' });
          f.canvas.paintItems(ctx, page.items, fenv, cfg.alpha === undefined ? {} : { alpha: cfg.alpha });
        });
        const B = runSide(S.recD, cfg, (ctx) => paintPageD(ctx, p, denv, cfg, S.cachesD));
        S.calls += A.log.length;
        if (A.err && B.err) S.bothThrow++;
        const d = diffLogs(A.log, B.log);
        if (!!A.err !== !!B.err) { S.errMismatch++; if (S.mismatches.length < 12) S.mismatches.push(`${label} page ${pi} ${cfg.name}: ${A.err ? `frozen threw (${A.err.message})` : `D threw (${B.err.message})`}`); } else if (d) {
          S.logMismatch++;
          if (S.mismatches.length < 12) S.mismatches.push(`${label} page ${pi} ${cfg.name}: call ${d.i} of ${A.log.length}/${B.log.length}: frozen ${jn(d.a)?.slice(0, 200)} | D ${jn(d.b)?.slice(0, 200)} after ${jn(d.context)?.slice(0, 200)}`);
        }
      }
    });
    if (facets) exportFacets(f, result, view, rasters, S, label);
  } finally { view.free(); }
}

function report(S) {
  console.log(`${S.label}: ${S.docs} layouts, ${S.pages} pages, ${S.items} items (${JSON.stringify(S.kinds)}), ${S.calls} frozen draw calls x ${CONFIGS.length} configurations: ${S.logMismatch} log mismatches, ${S.errMismatch} exception mismatches (${S.bothThrow} both threw), R-P2c ${S.wireDiffs} wire differences${S.skipped ? `, ${S.skipped} skipped` : ''}`);
  for (const [n, v] of Object.entries(S.facets)) console.log(`  facet ${n}: equal ${v.equal}, mismatch ${v.mismatch}, both null ${v.bothNull}, refused by the export ${v.refused}, frozen only ${v.frozenOnly}, export only ${v.rustOnly}${v.samples.length ? ` e.g. ${v.samples.join(' ; ')}` : ''}`);
  for (const m of S.wireMsgs) console.log('  WIRE', m);
  for (const m of S.mismatches) console.log('  MISMATCH', m);
}

const mkStats = (label) => ({ ...newStats(), label, recF: new Recording(null), recD: new Recording(null), cachesD: new PaintCaches() });
const unexplained = (S) => S.logMismatch + S.errMismatch + S.wireDiffs + Object.values(S.facets).reduce((n, v) => n + v.mismatch + (v.refused ?? 0) + (v.frozenOnly ?? 0) + (v.rustOnly ?? 0), 0);

test('Part B: the generated pages (every painter branch) through the adapter, the Rust view and both painters', { skip: skipTs }, async () => {
  const f = await frozen();
  const N = Number(process.env.UO_V_SEEDS ?? 200);
  const S = mkStats('genSpec');
  const fonts = new Map();
  const fromSpec = (spec) => f.materialize(spec, (fc) => {
    if (!fonts.has(fc.file)) fonts.set(fc.file, new Uint8Array(readFileSync(join(repo, 'shared/fonts/files', fc.file))));
    return fonts.get(fc.file);
  }, (im) => new Uint8Array(Buffer.from(im.b64, 'base64')));
  const t0 = performance.now();
  for (let i = 0; i < N; i++) {
    const spec = f.genSpec(0x2000 + i * 7919);
    processResult(f, `seed ${i}`, fromSpec(spec).result, S);
  }
  report(S);
  console.log(`  (${Math.round(performance.now() - t0)} ms)`);
  assert.equal(unexplained(S), 0, 'no unexplained mismatch');
});

test('Part B (oracle): the Word oracle documents laid out by the frozen engine with the installed fonts', { skip: skipTs || (process.env.UO_V_ORACLE ? false : 'set UO_V_ORACLE=1') }, async () => {
  const f = await frozen();
  const { importFrom, walk } = await import('../../uo-testkit/js/testkit.mjs');
  const D = 'write/packages/docx-layout/dist/';
  process.env.TZ = 'UTC';
  const tscore = await importFrom(tsRoot, 'write/packages/docx-core/dist/index.js');
  const eng = await importFrom(tsRoot, `${D}index.js`);
  const { createNodeFontProvider } = await importFrom(tsRoot, 'write/tools/corpus-lab/node-fonts.mjs');
  const provider = await createNodeFontProvider();
  await eng.loadHyphenation(eng.HYPHENATION_PATTERN_IDS);
  const dir = join(tsRoot, D, '../test/oracle');
  const groups = readdirSync(dir).sort();
  const DRAWING = new Set(['runfx', 'effext', 'emfpat', 'emfrop', 'wmftext', 'objects', 'cellpic', 'hfimg', 'imgbr', 'imgfit', 'imgline', 'imgsmall', 'imgul', 'txgeom', 'wrapcontour', 'wrappoly', 'pbdrshadow', 'vmlhr', 'dgmspace', 'pichidden']);
  const stride = Number(process.env.UO_V_ORACLE_STRIDE ?? 13);
  const picked = groups.filter((g, i) => i % stride === 0 || DRAWING.has(g));
  const S = mkStats('oracle');
  const t0 = performance.now();
  for (const g of picked) {
    for (const file of walk(join(dir, g), (p) => /\.docx$/i.test(p))) {
      const label = `${g}/${file.split(/[\\/]/).pop()}`;
      let result;
      try {
        const pkg = tscore.DocxPackage.load(new Uint8Array(readFileSync(file)));
        const full = tscore.readFullDocument(pkg);
        let embedded = [];
        try { embedded = tscore.readEmbeddedFonts(pkg); } catch { /* none */ }
        result = eng.layoutDocument(full, eng.withEmbeddedFonts(provider, embedded, (b) => new eng.SfntFont(b)), { now: new Date(NOW), ...(label.includes('/mk-') ? { revisions: { mode: 'all' } } : {}) });
      } catch { S.skipped++; continue; }
      processResult(f, label, result, S);
    }
  }
  report(S);
  console.log(`  (${picked.length} groups, ${Math.round(performance.now() - t0)} ms)`);
  assert.equal(unexplained(S), 0, 'no unexplained mismatch');
});
