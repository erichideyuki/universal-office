// Memory gate at the JS boundary of the render exports (M4 and M2, uo-render frontier E): 10,000 view
// handles and 10,000 result handles created and released through free() and through a
// FinalizationRegistry, 10,000 consumed input buffers (accepted and refused), and a soak of
// encode + paint exports on the rich fixture; the module's live bytes must return exactly and wasm memory
// (which never shrinks) must stop growing. The same file runs in WebKit through `browser/run.mjs` (without
// the finalizer test, which needs `gc()`).
//   cargo build -p uo-wasm --features count,render-fixture --target wasm32-unknown-unknown --profile wasm
//   UO_WASM_PATH=<that .wasm> node --expose-gc --test crates/uo-wasm/tests/render_boundary.test.mjs
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { join, dirname } from 'node:path';
import { fileURLToPath } from 'node:url';
import { bindRender } from '../js/render.mjs';
import { writeImports } from '../js/write.mjs';

const here = dirname(fileURLToPath(import.meta.url));
const wasmPath = process.env.UO_WASM_PATH ?? join(here, '../../../target/wasm32-unknown-unknown/wasm/uo_wasm.wasm');
const { instance } = await WebAssembly.instantiate(readFileSync(wasmPath), writeImports().imports);
const w = instance.exports;
const has = typeof w.uo_render_fixture === 'function';
const skip = has ? false : 'this build has no render-fixture exports';
const r = has ? bindRender(w) : null;
const COUNT = 10_000;
const live = () => w.uo_memory_live_bytes();
const settle = async () => {
  for (let i = 0; i < 5; i++) { globalThis.gc?.(); await new Promise((ok) => setTimeout(ok, 10)); }
  return live();
};

test('the module counts memory and has the render exports', { skip }, () => {
  assert.ok(live() >= 0, 'build uo-wasm with --features count,render-fixture');
});

test('free() releases every view and result (M4)', { skip }, async () => {
  const before = await settle();
  for (let i = 0; i < COUNT; i++) {
    const v = r.fixture(0);
    assert.equal(v.pageCount, 1);
    const b = v.encodePage(0);
    assert.equal(b.status, 0);
    assert.ok(b.length > 0);
    b.free();
    v.free();
  }
  assert.equal(await settle(), before);
});

test('the FinalizationRegistry releases views and results that were not freed (M4)', { skip: skip || (!globalThis.gc && 'needs node --expose-gc') }, async () => {
  const before = await settle();
  (() => { for (let i = 0; i < COUNT; i++) { const v = r.fixture(0); v.encodePage(0); } })();
  assert.ok(live() > before, 'the abandoned handles are counted while alive');
  assert.equal(await settle(), before);
});

test('consumed input buffers are released, accepted or refused (M4)', { skip }, async () => {
  const rich = r.fixture(0);
  const good = rich.encodePage(0).take().data;
  rich.free();
  const before = await settle();
  for (let i = 0; i < COUNT; i++) {
    const v = r.newView(1);
    assert.equal(v.pushMessage(good), 0);
    v.setApp(0, good); // a consumed buffer on a good handle
    assert.throws(() => v.pushMessage(good.subarray(0, 9)), { status: 3 });
    assert.throws(() => v.setApp(4, good), { status: 6 });
    assert.throws(() => v.setFaceBytes(0, good), { status: 9 }); // not a font
    v.free();
  }
  // a buffer given to a null handle is consumed too
  for (let i = 0; i < 1000; i++) {
    const p = w.uo_alloc_bytes(64);
    assert.equal(w.uo_render_view_push_message(0, p, 64), -11);
  }
  assert.equal(await settle(), before);
});

test('the raw buffer helpers release their inputs (M4)', { skip }, async () => {
  const mask = new Float32Array(64 * 64).fill(0.5);
  const before = await settle();
  for (let i = 0; i < COUNT / 10; i++) {
    assert.equal(r.coloredMask(mask, 64, 64, '336699', 0.5).length, 64 * 64 * 4);
    assert.deepEqual(r.dash('dash'), [4, 3]);
    assert.equal(r.dash('zzz'), null);
  }
  assert.equal(await settle(), before);
});

test('encode + paint exports on the rich fixture are flat (M2)', { skip }, async () => {
  const sizes = [];
  const lives = [];
  const v = r.fixture(1);
  const round = () => {
    for (let k = 0; k < 20; k++) {
      v.encodePage(0).free();
      v.fxLayers(0, 0, 2).free();
      v.pattern(0, 1).free();
      v.rectGradient(0, 2).free();
      v.blipTiles(0, 3).free();
      v.pageFillRaster(0).free();
      v.pageFillRaster(1).free();
      v.pictureBounds(0, 4).free();
      v.pictureSubs(0, 4).free();
    }
  };
  for (let i = 0; i < 20; i++) {
    round();
    sizes.push(w.memory.buffer.byteLength);
    lives.push(live());
  }
  // 200 iterations of encode + every paint export: the memory high-water mark and the live bytes are flat
  assert.equal(new Set(sizes.slice(2)).size, 1, `wasm memory kept growing: ${sizes.join(', ')}`);
  assert.equal(new Set(lives).size, 1, `live bytes drifted: ${lives.join(', ')}`);
  v.free();
  const after = await settle();
  const v2 = r.fixture(1);
  v2.free();
  assert.equal(await settle(), after);
  console.log(`render soak: memory.buffer ${(sizes.at(-1) / 1048576).toFixed(1)} MiB, live while the rich view is open ${lives[0]} B, after free ${after} B`);
});

test('a phone-sized cap on the effect layer refuses instead of growing', { skip }, async () => {
  const v = r.fixture(1);
  const open = live();
  v.setMaxFxPx(1000);
  const { status } = v.fxLayers(0, 0, 8).take();
  assert.ok(status === 1 || status === 4, `status ${status}`);
  assert.ok(live() <= open + 4096, `the refusal allocated: ${live() - open} B`);
  v.free();
});
