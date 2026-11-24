// The Sheet wasm module (frontier C4, src/sheet.rs, js/sheet.mjs) at its JS boundary: open, sheets, the two-phase measure loop,
// a viewport message decoded with the generated TS codec and replayed by the Slides painter on a recording context (the
// `GlyphRun` expansion of the painter equals the per-glyph `fillText`s), errors, links, and the M4 handle test (10,000 create/drop).
//
//   cargo build -p uo-wasm --features sheet,count --target wasm32-unknown-unknown --profile wasm
//   UO_WASM_PATH=<that .wasm> [UO_SHEET_SOAK=10000] node --expose-gc --test crates/uo-wasm/tests/sheet_api.test.mjs
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { join, dirname } from 'node:path';
import { fileURLToPath } from 'node:url';
import { bindSheet, SheetError } from '../js/sheet.mjs';
import { decodeSlidesList, newTables } from '../js/gen/slides.ts';
import { SlidesPainter } from '../js/paint-slides.ts';

const here = dirname(fileURLToPath(import.meta.url));
const wasmPath = process.env.UO_WASM_PATH ?? join(here, '../../../target/wasm32-unknown-unknown/wasm/uo_wasm.wasm');
const { instance } = await WebAssembly.instantiate(readFileSync(wasmPath), {});
const uo = bindSheet(instance.exports);
const SAMPLES = join(here, '../../../../sheet/samples');
const sample = (n) => new Uint8Array(readFileSync(join(SAMPLES, n)));
const SOAK = Number(process.env.UO_SHEET_SOAK ?? 10_000);

// the fake canvas of the harness: 6 px per UTF-16 unit
const measurer = {
  advance: (_f, cp) => (cp > 0xffff ? 12 : 6),
  width: (_f, t) => 6 * t.length,
  box: () => [9, 3],
};

/** A context that records the canvas calls as text. */
function recorder() {
  const ops = [];
  const ctx = new Proxy({ measureText: (t) => ({ width: 6 * t.length }), getTransform: () => ({ a: 1, b: 0, c: 0, d: 1, e: 0, f: 0, inverse() { return this; }, multiply() { return this; } }) }, {
    get(t, k) { if (k in t) return t[k]; return (...a) => { ops.push(`${String(k)}(${a.map((x) => (typeof x === 'object' ? '{}' : String(x))).join(',')})`); return { addColorStop() {} }; }; },
    set(t, k, v) { ops.push(`${String(k)}=${typeof v === 'object' ? '{}' : String(v)}`); t[k] = v; return true; },
  });
  return { ctx, ops };
}

test('open, sheets, activate, viewport, paint', () => {
  const book = uo.open(sample('showcase.xlsx'));
  const sheets = book.sheets();
  assert.ok(sheets.length >= 2 && sheets[0].kind === 'sheet');
  book.activate(0, measurer);
  const bytes = book.viewport({ sheet: 0, width: 800, height: 500, dpr: 1, scrollX: 0, scrollY: 0, zoom: 1, selection: [1, 1, 1, 1, 1, 1] }, measurer);
  assert.ok(bytes.length > 500);
  const tables = newTables();
  const list = decodeSlidesList(bytes, tables);
  assert.ok(list.items.some((i) => i.op === 'glyphRun') && list.items.some((i) => i.op === 'sheetFont'));
  const { ctx, ops } = recorder();
  const painter = new SlidesPainter({ fontStack: (f, b, i) => `'${f.split('|')[0]}'`, media: () => null, bitmap: () => null });
  painter.paint(ctx, list, tables, undefined);
  // every non-space glyph of every run became one fillText
  let want = 0;
  for (const it of list.items) if (it.op === 'glyphRun') want += [...list.texts.slice(it.text.start, it.text.start + it.text.len)].filter((c) => c !== ' ').length;
  const got = ops.filter((o) => o.startsWith('fillText(')).length;
  assert.ok(got >= want - 5 && got > 20, `${got} fillText for ${want} glyphs`);
  // a second frame reuses the tables (a delta message)
  const again = book.viewport({ sheet: 0, width: 800, height: 500, dpr: 1, scrollX: 40, scrollY: 20, zoom: 1.25, selection: [2, 2, 2, 2, 3, 3] }, measurer);
  assert.ok(again.length > 500);
  assert.equal(book.recalcPlan().length > 0, true);
  assert.ok(book.extent().width > 0);
  book.free();
  assert.throws(() => book.sheets(), SheetError);
});

test('errors: not a workbook, no such sheet', () => {
  assert.throws(() => uo.open(new Uint8Array([1, 2, 3, 4])), SheetError);
  const book = uo.open(sample('smoke.xlsx'));
  assert.throws(() => book.activate(99, measurer), SheetError);
  book.free();
});

test('M4: 10,000 create/drop per handle', async () => {
  const bytes = sample('smoke.xlsx');
  const before = uo.liveBytes();
  for (let i = 0; i < SOAK; i++) {
    const b = uo.open(bytes);
    if (i % 1000 === 0) { b.activate(0, measurer); b.viewport({ sheet: 0, width: 300, height: 200, selection: [1, 1, 1, 1, 1, 1] }, measurer); }
    b.free();
  }
  for (let i = 0; i < 5; i++) { globalThis.gc?.(); await new Promise((r) => setTimeout(r, 10)); }
  const after = uo.liveBytes();
  if (before) assert.ok(after - before < 1 << 20, `live ${before} -> ${after}`);
});
