// The paint exports of uo-render in wasm (frontier E): every item-addressed operation over the
// fixture views, differential against the native run (the FNV-1a checksums below are printed by
// `cargo test -p uo-wasm --features render-fixture --test render_native -- --nocapture`, which asserts
// the same fixture natively), the layouts of the results, the refusals and the page message
// decoding back into a view. The same file runs in WebKit through `browser/run.mjs`.
//   cargo build -p uo-wasm --features count,render-fixture --target wasm32-unknown-unknown --profile wasm
//   UO_WASM_PATH=<that .wasm> node --test crates/uo-wasm/tests/render_paint.test.mjs
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { join, dirname } from 'node:path';
import { fileURLToPath } from 'node:url';
import { bindRender, fnv } from '../js/render.mjs';
import { writeImports } from '../js/write.mjs';

const here = dirname(fileURLToPath(import.meta.url));
const wasmPath = process.env.UO_WASM_PATH ?? join(here, '../../../target/wasm32-unknown-unknown/wasm/uo_wasm.wasm');
const { instance } = await WebAssembly.instantiate(readFileSync(wasmPath), writeImports().imports);
const w = instance.exports;
const has = typeof w.uo_render_fixture === 'function';
const skip = has ? false : 'this build has no render-fixture exports';
const r = has ? bindRender(w) : null;

// [export, args, status, bytes, checksum] of the rich fixture (page 0 items: 0 fx run, 1 pattern, 2 rect
// gradient, 3 blip tiles, 4 picture look, 5 shape with effects, 6 axial gradient)
const GOLD = {
  fx_layers: [0, 64024, '2ded0349'],
  pattern: [0, 53640, '4381663f'],
  rect_gradient: [0, 57608, '5c744041'],
  blip_tiles: [0, 388, 'b078aa77'],
  page_fill_tile_tw: [0, 16, '3b2b5dfd'],
  'page_fill_raster.gradient': [0, 3145736, 'cb27df98'],
  'page_fill_raster.pattern': [0, 264, '36a6db65'],
  'picture_bounds.image': [0, 32, 'ea1ffa4e'],
  'picture_bounds.shape_fx': [0, 32, '870dfebe'],
  picture_subs: [0, 226, '1aba3625'],
  'encode_page.0': [0, 1476, '09a95638'],
  'encode_page.1': [0, 131, '3486c922'],
};

function check(name, buf) {
  const { status, data } = buf.take();
  const [s, len, sum] = GOLD[name];
  assert.equal(status, s, `${name} status`);
  assert.equal(data.length, len, `${name} length`);
  assert.equal(fnv(data), sum, `${name} bytes`);
  return data;
}

test('the build has the render fixture exports', { skip }, () => {
  assert.ok(w.uo_memory_live_bytes() >= 0, 'build uo-wasm with --features count,render-fixture');
});

test('every paint export equals the native run (R-P3 differential)', { skip }, () => {
  const v = r.fixture(1);
  assert.equal(v.pageCount, 2);
  const fx = r.decoders.fxLayer(check('fx_layers', v.fxLayers(0, 0, 2)));
  assert.ok(fx.w > 0 && fx.h > 0 && fx.data.length === fx.w * fx.h * 4);
  const pat = r.decoders.pattern(check('pattern', v.pattern(0, 1)));
  assert.equal(pat.data.length, pat.w * pat.h * 4);
  assert.equal(pat.cellsW, pat.w, 'one pixel per cell at k = 1');
  const rg = r.decoders.raster(check('rect_gradient', v.rectGradient(0, 2)));
  assert.deepEqual([rg.w, rg.h], [144, 100]);
  const tiles = r.decoders.blipTiles(check('blip_tiles', v.blipTiles(0, 3)));
  assert.ok(tiles.length > 1 && tiles.every((t) => t.w > 0 && t.h > 0));
  const tile = r.decoders.pair(check('page_fill_tile_tw', v.pageFillTileTw(0, 15)));
  assert.equal(tile.w, 1024 * 15);
  assert.equal(tile.h, 768 * 15);
  const g = r.decoders.raster(check('page_fill_raster.gradient', v.pageFillRaster(0)));
  assert.deepEqual([g.w, g.h], [1024, 768]);
  const p = r.decoders.raster(check('page_fill_raster.pattern', v.pageFillRaster(1)));
  assert.deepEqual([p.w, p.h], [8, 8]);
  const b = r.decoders.bounds(check('picture_bounds.image', v.pictureBounds(0, 4)));
  assert.ok(b.x0 < 0 && b.x1 > 120, 'the glow and the shadow grow the box');
  check('picture_bounds.shape_fx', v.pictureBounds(0, 5));
  const subs = r.decoders.subpaths(check('picture_subs', v.pictureSubs(0, 4)));
  assert.ok(subs.length >= 1 && subs[0].length >= 4);
  v.free();
});

test('refusals have codes and never trap', { skip }, () => {
  const v = r.fixture(1);
  const code = (b) => b.take().status;
  assert.equal(code(v.fxLayers(0, 1, 2)), 7, 'fx layers of a shape');
  assert.equal(code(v.pattern(0, 2)), 7, 'pattern of a gradient shape');
  assert.equal(code(v.pattern(0, 99)), 6, 'no such item');
  assert.equal(code(v.pattern(9, 0)), 6, 'no such page');
  assert.equal(code(v.pageFillRaster(9)), 6);
  assert.equal(code(v.rectGradient(0, 6)), 0, 'a gradient shape rasterizes (the caller decides if it is the rectangular one)');
  assert.equal(code(v.encodePage(0)), 0);
  assert.equal(code(v.encodePage(5)), 6);
  assert.ok([0, 1].includes(code(v.fxLayers(0, 0, NaN))), 'a NaN scale answers (a layer of NaN size is none)');
  v.setMaxFxPx(10);
  const { status } = v.fxLayers(0, 0, 2).take();
  assert.ok(status === 1 || status === 4, `a lowered cap refuses or answers none: ${status}`);
  assert.throws(() => v.setMaxFxPx(NaN));
  v.free();
  v.free(); // a second free is a no-op
});

test('a page message decodes into a view that paints the same', { skip }, () => {
  const rich = r.fixture(1);
  const a = rich.encodePage(0).take().data;
  const b = rich.encodePage(1).take().data;
  const view = r.newView(0);
  assert.equal(view.pushMessage(a), 0);
  assert.equal(view.pushMessage(b), 1);
  assert.equal(view.pageCount, 2);
  // the wire carries no font: attach it, and the layer is the same bytes as the original's
  assert.equal(view.fxLayers(0, 0, 2).take().status, 8, 'no font yet: a resource is missing');
  view.setFaceBytes(0, new Uint8Array(readFileSync(join(here, '../../../../shared/fonts/files/liberationserif-regular.ttf'))));
  const same = (name, x, y) => assert.equal(fnv(x.take().data), fnv(y.take().data), name);
  same('fx layers', view.fxLayers(0, 0, 2), rich.fxLayers(0, 0, 2));
  same('pattern', view.pattern(0, 1), rich.pattern(0, 1));
  same('rect gradient', view.rectGradient(0, 2), rich.rectGradient(0, 2));
  same('page fill', view.pageFillRaster(0), rich.pageFillRaster(0));
  same('picture bounds', view.pictureBounds(0, 4), rich.pictureBounds(0, 4));
  view.setImageBytes(0, new Uint8Array(16), 64, 48);
  same('blip tiles', view.blipTiles(0, 3), rich.blipTiles(0, 3));
  // the decoded view encodes again (its generation is its own, and the wire does not carry the byte lengths
  // or the font's units per em, so the bytes differ from the original's) into a message another view takes
  const again = view.encodePage(0).take();
  assert.equal(again.status, 0);
  const third = r.newView(3);
  assert.equal(third.pushMessage(again.data), 0);
  third.free();
  // malformed and stale messages are refused with codes, the view unchanged
  assert.throws(() => view.pushMessage(a.subarray(0, 20)), { status: 3 });
  const stale = a.slice();
  stale[8] ^= 0xff;
  assert.throws(() => view.pushMessage(stale), { status: 2 });
  assert.equal(view.pageCount, 2);
  view.free();
  rich.free();
});

test('the app section travels through the view', { skip }, () => {
  const v = r.fixture(0);
  const section = Uint8Array.from([1, 2, 3, 4, 5, 0, 255]);
  v.setApp(0, section);
  const msg = v.encodePage(0).take().data;
  assert.ok(msg.length > section.length);
  assert.deepEqual([...msg.subarray(msg.length - section.length)], [...section], 'the section closes the message');
  assert.throws(() => v.setApp(3, section), { status: 6 });
  v.free();
});

test('the picture helpers over raw buffers', { skip }, () => {
  const mask = new Float32Array([0, 0.5, 1, 0.25]);
  const rgba = r.coloredMask(mask, 2, 2, 'FF8000', 1);
  assert.deepEqual([...rgba], [255, 128, 0, 0, 255, 128, 0, 128, 255, 128, 0, 255, 255, 128, 0, 64]);
  assert.throws(() => r.coloredMask(mask, 3, 3, 'FF8000', 1), { status: 5 }, 'a mask whose size is not w x h');
});

test('dash patterns', { skip }, () => {
  assert.deepEqual(r.dash('sysDot'), [1, 1]);
  assert.deepEqual(r.dash('lgDashDot'), [8, 3, 1, 3]);
  assert.equal(r.dash('solid'), null);
  assert.equal(r.dash(''), null);
});

// the picture-layer exports (frontier D) on fixture 2, the same inputs as `picture_pass` of render_native.rs; the
// checksums are the ones that test prints with --nocapture
const PIC = {
  polys: [0, 4488, 'cdbaaacd'],
  silhouette: [0, 374976, 'a9af21dc'],
  bevel: [0, 374976, '691efc5e'],
  adjust: [0, 374976, '8d05a3e5'],
  outer_shadow: [0, 374976, 'b51bc9e8'],
  reflection: [0, 2976, 'cf5e2cca'],
  inner_shadow: [0, 374976, '5a49b4ca'],
  glow: [0, 374976, 'b1420741'],
  soft_edge: [0, 374976, 'e34f4565'],
  blur: [0, 374976, '4b9441ad'],
};

test('the picture-layer exports equal the native run', { skip }, () => {
  const v = r.fixture(2);
  const ops = v.paint;
  const pin = (name, buf) => {
    const { status, data } = buf.take();
    const [s, len, sum] = PIC[name];
    assert.equal(status, s, `${name} status`);
    assert.equal(data.length, len, `${name} length`);
    assert.equal(fnv(data), sum, `${name} bytes`);
    return data;
  };
  const b = r.decoders.bounds(r.ok(ops.pictureBounds(0, 0), 'bounds'));
  const ppt = 2;
  const W = Math.max(1, Math.ceil((b.x1 - b.x0) * ppt)), H = Math.max(1, Math.ceil((b.y1 - b.y0) * ppt));
  const ox = -b.x0 * ppt, oy = -b.y0 * ppt;
  pin('polys', ops.picturePolys(0, 0, ppt, ox, oy));
  pin('silhouette', ops.pictureSilhouette(0, 0, ppt, ox, oy, W, H));
  const sil = r.decoders.mask(r.ok(ops.pictureSilhouette(0, 1, ppt, ox, oy, W, H), 'silhouette'));
  const rgba = Uint8ClampedArray.from({ length: W * H * 4 }, (_, i) => (i * 7) % 251);
  pin('bevel', ops.pictureBevel(0, 0, ppt, ox, oy, W, H, rgba));
  pin('adjust', ops.pictureAdjust(0, 0, rgba));
  pin('outer_shadow', ops.outerShadow(0, 1, sil, W, H, { x: ox, y: oy, w: (2400 / 20) * ppt, h: (1800 / 20) * ppt }, 30, ppt));
  pin('reflection', ops.reflectionAlphas(0, 0, oy + 90 * ppt, 90 * ppt, H));
  pin('inner_shadow', ops.innerShadow(sil, W, H, 4, 3, 45, ppt));
  pin('glow', ops.glow(sil, W, H, 5, ppt));
  pin('soft_edge', ops.softEdge(sil, W, H, 3, ppt));
  pin('blur', ops.gaussianBlur(sil, W, H, 2.5));
  // refusals: status codes, nothing traps
  assert.equal(ops.pictureBevel(0, 1, ppt, ox, oy, W, H, rgba).take().status, 7, 'no bevel');
  assert.equal(ops.pictureAdjust(0, 2, rgba).take().status, 7, 'a shape');
  assert.equal(ops.gaussianBlur(new Float32Array(3), 2, 2, 1).take().status, 5, 'bad mask size');
  assert.equal(ops.reflectionAlphas(0, 1, 0, 10, 4).take().status, 7, 'no reflection');
  v.free();
});

test('the rasters facade returns decoded values and nulls', { skip }, () => {
  const v = r.fixture(2);
  const R = r.rasters(v.paint);
  const b = R.pictureBounds(0, 0);
  assert.ok(b && b.x1 > b.x0);
  const polys = R.picturePolys(0, 0, 2, 0, 0);
  assert.ok(polys.length >= 1 && polys[0] instanceof Float64Array && polys[0].length >= 6);
  const sil = R.pictureSilhouette(0, 0, 2, 10, 10, 200, 150);
  assert.ok(sil instanceof Float32Array && sil.length === 200 * 150);
  assert.throws(() => R.pictureAdjust(0, 2, new Uint8ClampedArray(16)), /wrong item/);
  assert.deepEqual(R.dash('dash'), [4, 3]);
  assert.equal(R.dash('nope'), null);
  assert.throws(() => R.rectGradient(0, 2), /wrong item/);
  v.free();
});
