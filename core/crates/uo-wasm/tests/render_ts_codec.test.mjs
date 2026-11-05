// R-P2b for the canvas encoder (frontier E), Node only (it imports the generated TypeScript codec,
// which a browser cannot): a page message encoded by the Rust encoder inside the wasm module is decoded
// by the generated TS decoder, re-encoded by the generated TS encoder and must give the SAME bytes, and
// the hints (box, parts, joins, outlines, gradient samples) arrive as plain objects with the tree.ts names.
//   cargo build -p uo-wasm --features count,render-fixture --target wasm32-unknown-unknown --profile wasm
//   UO_WASM_PATH=<that .wasm> node --test crates/uo-wasm/tests/render_ts_codec.test.mjs
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { join, dirname } from 'node:path';
import { fileURLToPath } from 'node:url';
import { bindRender } from '../js/render.mjs';
import { decodePage, encodePage, newTables, newEncodeState, tableLens } from '../js/gen/render.ts';

const here = dirname(fileURLToPath(import.meta.url));
const wasmPath = process.env.UO_WASM_PATH ?? join(here, '../../../target/wasm32-unknown-unknown/wasm/uo_wasm.wasm');
const { instance } = await WebAssembly.instantiate(readFileSync(wasmPath));
const w = instance.exports;
const has = typeof w.uo_render_fixture === 'function';
const skip = has ? false : 'this build has no render-fixture exports';
const r = has ? bindRender(w) : null;

test('Rust encode -> TS decode -> TS encode gives the same bytes (first message of a view)', { skip }, () => {
  const v = r.fixture(1);
  const msg = v.encodePage(0).take().data;
  const tables = newTables();
  const page = decodePage(msg, tables);
  const back = encodePage(page, newEncodeState(7, tables));
  assert.deepEqual([...back], [...msg]);
  v.free();
});

test('the hints reach the TS side with the tree.ts names', { skip }, () => {
  const v = r.fixture(1);
  const tables = newTables();
  const a = decodePage(v.encodePage(0).take().data, tables);
  const b = decodePage(v.encodePage(1).take().data, tables);
  assert.equal(a.widthTw, 12240);
  assert.equal(b.items.length, 0);
  assert.ok(b.backgroundFill !== undefined && tables.pageFills.length === 2);
  const run = a.items[0];
  assert.equal(run.t, 'glyphs');
  assert.ok(run.box && run.box.w > 0, 'the run box of an fx run');
  assert.equal(run.glyphs.length, 5);
  assert.equal(tables.faces[run.face].upem, 2048);
  const shapes = a.items.filter((i) => i.t === 'shape');
  assert.equal(shapes.length, 5);
  assert.ok(shapes.every((s) => s.parts.length >= 1 && s.parts[0].cmds.length >= 4), 'shape parts in twips');
  const axial = shapes[4];
  assert.equal(axial.gradGeom.kind, 'axial');
  assert.equal(axial.gradSamples.length, 64 * 3);
  assert.equal(shapes[1].gradGeom, undefined, 'the rectangular gradient is a raster export, not a geometry');
  // the effects shape carries the parts of the local item (x = y = 0)
  assert.equal(shapes[3].parts[0].cmds[0].x, 0);
  v.free();
});
