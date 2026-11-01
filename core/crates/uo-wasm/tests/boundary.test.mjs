// Memory gate at the JS boundary of the wasm module (M4 and M2): 10,000 native documents
// created and released through free() and through a FinalizationRegistry; the module's live
// bytes must return exactly, and wasm memory (which never shrinks) must stop growing.
//   cargo build -p uo-wasm --features count --target wasm32-unknown-unknown --profile wasm
//   node --expose-gc --test crates/uo-wasm/tests/boundary.test.mjs
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { join, dirname } from 'node:path';
import { fileURLToPath } from 'node:url';

const here = dirname(fileURLToPath(import.meta.url));
const wasmPath = join(here, '../../../target/wasm32-unknown-unknown/wasm/uo_wasm.wasm');
const { instance } = await WebAssembly.instantiate(readFileSync(wasmPath));
const w = instance.exports;
const COUNT = 10_000;
const payload = Uint8Array.from({ length: 256 }, (_, i) => i);

// the JS side of a handle: explicit free(), and a FinalizationRegistry for the ones the
// caller forgets (the pattern the real bindings will use)
const registry = new FinalizationRegistry((ptr) => w.uo_document_free(ptr));
class NativeDocument {
  #ptr = w.uo_document_new();
  constructor() { registry.register(this, this.#ptr, this); }
  append(bytes) {
    const p = w.uo_alloc_bytes(bytes.length);
    new Uint8Array(w.memory.buffer, p, bytes.length).set(bytes);
    if (w.uo_document_append_owned(this.#ptr, p, bytes.length) !== 0) throw new Error('append failed');
  }
  get length() { return w.uo_document_len(this.#ptr); }
  free() { registry.unregister(this); w.uo_document_free(this.#ptr); this.#ptr = 0; }
}

const live = () => w.uo_memory_live_bytes();
const settle = async () => {
  for (let i = 0; i < 5; i++) { global.gc(); await new Promise((r) => setTimeout(r, 10)); }
  return live();
};

test('the module counts memory', () => {
  assert.ok(live() >= 0, 'build uo-wasm with --features count');
});

test('free() releases every native document (M4)', async () => {
  const before = await settle();
  for (let i = 0; i < COUNT; i++) {
    const d = new NativeDocument();
    d.append(payload);
    assert.equal(d.length, payload.length);
    d.free();
  }
  assert.equal(await settle(), before);
});

test('the FinalizationRegistry releases documents that were not freed (M4)', async () => {
  const before = await settle();
  (() => { for (let i = 0; i < COUNT; i++) new NativeDocument().append(payload); })();
  assert.ok(live() > before, 'the abandoned documents are counted while alive');
  assert.equal(await settle(), before);
});

test('wasm memory stops growing in a long scenario (M2)', async () => {
  const sizes = [];
  for (let round = 0; round < 20; round++) {
    for (let i = 0; i < 1000; i++) { const d = new NativeDocument(); d.append(payload); d.free(); }
    sizes.push(w.memory.buffer.byteLength);
  }
  // after the first rounds (warm-up) the high-water mark is flat
  assert.equal(new Set(sizes.slice(2)).size, 1, `wasm memory kept growing: ${sizes.join(', ')}`);
  assert.equal(await settle(), await settle());
});
