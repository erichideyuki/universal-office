// The memory gate (M4, M5) of the TS painter of decoded pages (uo-render frontier D) over the wasm module:
//   M4  UO_PAINT_M4 pages (default 2,000; the record run uses 10,000) cycled through encodePage -> decodePage ->
//       paintPageBackground + paintItems on a recording context: the native live bytes are IDENTICAL before and after while
//       the layouts live, and back to the pre-layout value after free(); `memory.buffer` stays flat after a warm-up of 200
//       pages. 10,000 abandoned result handles (never taken or freed) return to the baseline through the
//       FinalizationRegistry.
//   M5  every cache of `PaintCaches` is filled past its cap through constructed pages and never exceeds it.
//   cargo build -p uo-wasm --features count,write,render-fixture --target wasm32-unknown-unknown --profile wasm
//   UO_WASM_PATH=<that .wasm> [UO_PAINT_M4=10000] node --expose-gc --test --test-concurrency=1 crates/uo-wasm/tests/paint_memory.test.mjs
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { join, dirname } from 'node:path';
import { fileURLToPath } from 'node:url';
import { writeImports, bindWrite } from '../js/write.mjs';
import { PaintCaches, paintItems, paintPageBackground } from '../js/paint.ts';
import { decodePage, newTables } from '../js/gen/render.ts';
import { Recording } from './support/recording-context.mjs';
import { CONFIGS, fixtureDocs, openEnv, wasmOptions } from './support/paint-common.mjs';
import { envD, paintPageD } from './support/paint-d.mjs';

const here = dirname(fileURLToPath(import.meta.url));
const wasmPath = process.env.UO_WASM_PATH ?? join(here, '../../../target/wasm32-unknown-unknown/wasm/uo_wasm.wasm');
const { imports, host } = writeImports();
const { instance } = await WebAssembly.instantiate(readFileSync(wasmPath), imports);
const w = instance.exports;
const uo = bindWrite(w, host);
const counted = typeof w.uo_memory_live_bytes === 'function' && typeof w.uo_write_open === 'function';
const skip = counted ? false : 'build with --features count,write,render-fixture';
const live = () => w.uo_memory_live_bytes();
const settle = async () => { for (let i = 0; i < 4; i++) { globalThis.gc?.(); await new Promise((r) => setImmediate(r)); } };
const M4 = Number(process.env.UO_PAINT_M4 ?? 2000);

/** a sink that counts and keeps nothing */
const countSink = { calls: 0, push() { this.calls++; } };

test('M4: painting pages leaks no native bytes and does not grow the memory', { skip }, async () => {
  assert.ok(globalThis.gc, 'run with --expose-gc');
  const docs = fixtureDocs().filter((_, i) => i % 9 === 0).slice(0, 6);
  // process-wide tables (the hyphenation patterns, the font registries) are built once on first use and live as long as the module:
  // one throwaway cycle (environment, layout, free) makes the starting value the steady one
  { const e0 = openEnv(uo); const d0 = uo.open(readFileSync(docs[0])); d0.layout(e0, wasmOptions()).free(); d0.free(); e0.free(); }
  await settle();
  const base00 = live();
  const env = openEnv(uo);
  // the environment parses the fonts it is asked for once and keeps them: one throwaway layout warms it, so the baseline below is the steady state
  for (const file of docs) { const d = uo.open(readFileSync(file)); d.layout(env, wasmOptions()).free(); d.free(); }
  await settle();
  const base0 = live();
  const rec = new Recording(countSink).install();
  const caches = new PaintCaches();
  const open = [];
  try {
    for (const file of docs) {
      const doc = uo.open(readFileSync(file));
      const layout = doc.layout(env, wasmOptions());
      const tables = newTables();
      // the first pass decodes every page in order (the tables grow with each message)
      const pages = [];
      for (let i = 0; i < layout.pageCount; i++) pages.push(decodePage(layout.encodePage(i), tables));
      open.push({ doc, layout, tables, pages, rasters: layout.rasters(), env: envD((id) => layout.imageBytes(id)) });
    }
    const pool = open.flatMap((o, d) => o.pages.map((_, i) => [d, i]));
    assert.ok(pool.length > 10, `${pool.length} pages in the pool`);
    const paintOne = (n) => {
      const [d, i] = pool[n % pool.length];
      const o = open[d];
      // every page goes through the wire again: the encoder (no new resources after the first pass) and the decoder
      const page = decodePage(o.layout.encodePage(i), o.tables);
      const cfg = CONFIGS[n % CONFIGS.length];
      rec.kerning = cfg.kerning; rec.scale = cfg.scale; rec.sink = countSink;
      paintPageD(rec.pageContext(), { page, tables: o.tables, index: i, rasters: o.rasters, owner: o.layout }, o.env, cfg, caches);
    };
    let n = 0;
    for (; n < Math.min(200, M4); n++) paintOne(n);
    await settle();
    const liveWarm = live();
    const bufWarm = w.memory.buffer.byteLength;
    for (; n < M4; n++) {
      paintOne(n);
      if (n % 500 === 0) await settle();
    }
    await settle();
    console.log(`M4: ${M4} pages painted over ${open.length} layouts (${pool.length} distinct pages), ${countSink.calls} draw calls; live ${liveWarm} -> ${live()} B, memory.buffer ${bufWarm} -> ${w.memory.buffer.byteLength} B`);
    assert.equal(live(), liveWarm, 'the live bytes after N pages equal the live bytes after the warm-up');
    assert.equal(w.memory.buffer.byteLength, bufWarm, 'memory.buffer is flat after the warm-up');
    for (const o of open) { o.layout.free(); o.doc.free(); }
    open.length = 0;
  } finally {
    rec.uninstall();
    for (const o of open) { o.layout.free(); o.doc.free(); }
  }
  await settle();
  assert.equal(live(), base0, 'after free() of the layouts and documents the live bytes are the pre-layout value');
  env.free();
  await settle();
  // what stays after the environment is freed is process-wide (the hyphenation patterns and font tables of the languages the documents used,
  // built on first use and kept for the life of the module): reported, not the painter's
  console.log('after env.free(): ' + live() + ' B live (process-wide tables; ' + base00 + ' B before the cycle)');
});

test('M4: 10,000 abandoned result handles return to the baseline (finalizer path)', { skip }, async () => {
  assert.ok(globalThis.gc, 'run with --expose-gc');
  const view = uo.render.fixture(1);
  const ops = view.paint;
  // the face's outline cache and the encoder's mark settle on the first use of each operation
  for (const b of [ops.pageFillTileTw(0, 15), ops.pictureBounds(0, 4), ops.blipTiles(0, 3), ops.pattern(0, 1), ops.fxLayers(0, 0, 2), ops.pictureAdjust(0, 4, new Uint8ClampedArray(64 * 64 * 4))]) b.take();
  await settle();
  const base = live();
  for (let i = 0; i < 10000; i++) {
    // results never taken or freed: only the FinalizationRegistry can release them
    ops.pageFillTileTw(0, 15);
    ops.pictureBounds(0, 4);
    ops.blipTiles(0, 3);
    if (i % 20 === 0) ops.pattern(0, 1);
    if (i % 100 === 0) ops.fxLayers(0, 0, 2);
    if (i % 2000 === 0) await settle();
  }
  // the raw-buffer inputs of the mask exports are consumed by the export itself
  const rgba = new Uint8ClampedArray(64 * 64 * 4);
  for (let i = 0; i < 2000; i++) ops.pictureAdjust(0, 4, rgba);
  for (let i = 0; i < 200; i++) { await settle(); if (live() === base) break; }
  console.log(`finalizer path: live ${live()} B, baseline ${base} B`);
  assert.equal(live(), base, 'the live bytes are back to the baseline');
  view.free();
});

// ── M5: the caps ────────────────────────────────────────────────────────────────────────────────────────

/** RasterSource answers of the right shape and small size (the caps are the subject, not the pixels) */
const stubRasters = {
  fxLayer: () => ({ x0: 0, y0: 0, w: 2, h: 2, data: new Uint8Array(16) }),
  pattern: () => ({ x: 0, y: 0, cellsW: 2, cellsH: 2, w: 2, h: 2, data: new Uint8Array(16) }),
  rectGradient: () => ({ w: 2, h: 2, data: new Uint8Array(16) }),
  blipTiles: () => [{ x: 0, y: 0, w: 10, h: 10 }],
  pageFillTileTw: () => ({ w: 15, h: 15 }),
  pageFillRaster: () => ({ w: 2, h: 2, data: new Uint8Array(16) }),
  pictureBounds: () => ({ x0: 0, y0: 0, x1: 10, y1: 10 }),
  picturePolys: () => [Float64Array.of(0, 0, 10, 0, 10, 10, 0, 10)],
  pictureSilhouette: (p, i, ppt, ox, oy, W, H) => new Float32Array(W * H).fill(1),
  pictureBevel: () => null,
  pictureAdjust: (p, i, rgba) => new Uint8Array(rgba),
  outerShadow: (p, i, sil) => new Float32Array(sil.length),
  reflectionAlphas: (p, i, top, hPpt, rows) => new Float64Array(rows),
  innerShadow: (sil) => new Float32Array(sil.length),
  glow: (sil) => new Float32Array(sil.length),
  softEdge: (sil) => new Float32Array(sil.length),
  gaussianBlur: (m) => new Float32Array(m.length),
  coloredMask: (m) => new Uint8Array(m.length * 4),
  dash: () => [4, 3],
};

const baseTables = () => {
  const t = newTables();
  t.faces.push({ key: 'k', family: 'F', upem: 1000 });
  for (let i = 0; i < 80; i++) t.images.push({ ext: 'png', byteLength: 1, widthTwips: 10, heightTwips: 10, ...(i % 2 ? { effects: { gray: true } } : {}) });
  return t;
};
const ENV = { fontOf: () => ({ family: '"F"', weight: 400, italic: false }), image: () => ({ __tok: 'img', width: 8, height: 8 }), color: (h) => `#${h}` };
const page = (items, extra = {}) => ({ widthTw: 12240, heightTw: 15840, items, app: new Uint8Array(0), ...extra });

test('M5: every cache of the painter is capped and covered past its cap', { skip }, () => {
  const rec = new Recording(countSink).install();
  try {
    const caps = { fillTiles: 8, shape: 8, fx: 16, pictureImages: 8, picturePerImage: 4, adjustedPerImage: 4 };
    const caches = new PaintCaches(caps);
    const owner = {};
    const ctx = () => { rec.kerning = true; rec.scale = 1 / 15; return rec.pageContext(); };
    const max = { fillTiles: 0, shape: 0, fx: 0, pictureImages: 0, picture: 0, adjusted: 0 };
    const watch = () => { const s = caches.sizes(owner); for (const k of Object.keys(max)) max[k] = Math.max(max[k], s[k]); };

    // page-fill tiles: 40 distinct gradient fills
    for (let i = 0; i < 40; i++) {
      const t = baseTables();
      t.pageFills.push({ type: 'gradient', c1: `00${(i % 256).toString(16).padStart(2, '0')}00`, c2: 'FFFFFF', angle: i, focus: 0 });
      paintPageBackground(ctx(), { page: page([], { backgroundFill: 0, backgroundShow: true }), tables: t, index: 0, rasters: stubRasters, owner }, ENV, { devicePxTw: 15, screen: false, paper: '#fff' }, caches);
      watch();
    }
    // text-effect layers: 60 runs with fx (the cache key is page:item)
    {
      const t = baseTables();
      t.textFx.push({ shadow: { blurPt: 1, distPt: 1, dirDeg: 0, sx: 1, sy: 1, kxDeg: 0, kyDeg: 0, algn: 'b', hex: '000000', alpha: 1 } });
      const items = Array.from({ length: 60 }, (_, i) => ({ t: 'glyphs', face: 0, sizePt: 12, color: '000000', y: 100 + i, glyphs: [{ cp: 65, x: i }], fx: 0, box: { x: 0, y: 0, w: 10, h: 10 } }));
      // the cache key is page:item, so each run is painted as item i of its own page
      items.forEach((it, i) => {
        paintItems(ctx(), { page: page(items), tables: t, index: 0, rasters: stubRasters, owner }, ENV, { from: i, to: i + 1 }, caches);
        watch();
      });
    }
    // shape-effect layers: 40 shapes with distinct sizes
    {
      const t = baseTables();
      const items = Array.from({ length: 40 }, (_, i) => ({
        t: 'shape', x: 0, y: 0, w: 1000 + i, h: 500, style: { geom: 'rect' }, fill: 'FF0000', fx: { glow: { color: 'FF0000', alpha: 0.5, radPt: 2 } }, parts: [],
      }));
      items.forEach((it, i) => { paintItems(ctx(), { page: page(items), tables: t, index: 0, rasters: stubRasters, owner }, ENV, { from: i, to: i + 1 }, caches); watch(); });
    }
    // picture layers (per image and images) and adjusted bitmaps (per image): 30 images x 7 looks, odd images carry effects
    {
      const t = baseTables();
      const items = [];
      for (let img = 0; img < 30; img++) for (let k = 0; k < 7; k++) items.push({ t: 'image', x: 0, y: 0, w: 1000 + k, h: 800, image: img, pic: { geom: { prst: 'rect' }, alpha: 0.5 + k / 100 } });
      items.forEach((it, i) => { paintItems(ctx(), { page: page(items), tables: t, index: 0, rasters: stubRasters, owner }, ENV, { from: i, to: i + 1 }, caches); watch(); });
    }
    const s = caches.sizes(owner);
    console.log(`M5: caps ${JSON.stringify(caps)}; maxima seen ${JSON.stringify(max)}; final ${JSON.stringify(s)}`);
    // the TS rule clears when the size is OVER the cap, before the insert: the size may be cap + 1 for an instant
    assert.ok(max.fillTiles <= caps.fillTiles + 1, 'fill tiles');
    assert.ok(max.shape <= caps.shape + 1, 'shape layers');
    assert.ok(max.fx <= caps.fx, 'effect layers');
    assert.ok(max.pictureImages <= caps.pictureImages, 'images with picture layers');
    assert.ok(max.picture <= caps.picturePerImage + 1, 'picture layers per image');
    assert.ok(max.adjusted <= caps.adjustedPerImage, 'adjusted bitmaps per image');
    // and each of them was actually filled up to (about) its cap
    assert.ok(max.fillTiles >= caps.fillTiles && max.shape >= caps.shape && max.fx >= caps.fx && max.pictureImages >= caps.pictureImages, 'the caches were filled');
    caches.clear();
    assert.deepEqual(caches.sizes(owner), { fillTiles: 0, shape: 0, fx: 0, pictureImages: 0, picture: 0, adjusted: 0 });
  } finally { rec.uninstall(); }
});

test('M4: the picture layer (mask exports) painted 48 times at 8 scales leaves the live bytes unchanged', { skip }, async () => {
  assert.ok(globalThis.gc, 'run with --expose-gc');
  const view = uo.render.fixture(2);
  const tables = newTables();
  const msg = view.paint.encodePage(0).take();
  assert.equal(msg.status, 0);
  const page = decodePage(msg.data, tables);
  const rasters = uo.render.rasters(view.paint);
  const rec = new Recording(countSink).install();
  const caches = new PaintCaches();
  const env = envD(() => new Uint8Array(16));
  const owner = {};
  const run = (n) => {
    for (let i = 0; i < n; i++) {
      const cfg = { kerning: true, scale: (1 + (i % 8) * 0.12) / 15, screen: false, alpha: undefined };
      rec.kerning = true; rec.scale = cfg.scale; rec.sink = countSink;
      paintPageD(rec.pageContext(), { page, tables, index: 0, rasters, owner }, env, cfg, caches);
    }
  };
  try {
    run(16);
    await settle();
    const before = live(), buf = w.memory.buffer.byteLength;
    run(48);
    await settle();
    console.log(`M4 pictures: live ${before} -> ${live()} B, memory.buffer ${buf} -> ${w.memory.buffer.byteLength} B, caches ${JSON.stringify(caches.sizes(owner))}`);
    assert.equal(live(), before);
    assert.equal(w.memory.buffer.byteLength, buf);
  } finally { rec.uninstall(); view.free(); }
});
