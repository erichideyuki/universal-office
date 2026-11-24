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

// C4b: the drawings and charts of a frame (`uo-dml-render`'s requests answered by the shell), the pictures of the list, the chart
// faces tagged U+0001, and the calc module (`--features sheet-calc`: the open applies the open-recalc of the file).
const DR1 = join(here, '../../uo-xlsx/tests/captures/workbooks/dr1');
/** A stub of the shell's answers (`sheet-engine.ts` `#answers`), 6 px per unit. */
const dmlAnswer = (reqs) => reqs.flatMap((r) => {
  if (r.kind === 'charAt1000') return [500];
  if (r.kind === 'kernAt1000') return [500 * r.text.length, 500 * r.text.length];
  if (r.kind === 'width') return [6 * r.text.length];
  if (r.kind === 'metrics') return [6 * r.text.length, 0, 6 * r.text.length, 8, 2, 9, 3];
  if (r.kind === 'imageSize') return [40, 30];
  if (r.kind === 'metaText') return [10, 9, 3];
  if (r.kind === 'metaDecode') return [NaN, NaN];
  return [0];
});

test('drawings and charts: requests answered, items, media, chart faces', () => {
  let shapes = 0, charts = 0, pictures = 0;
  for (const f of ['dr1-0000.xlsx', 'dr1-0001.xlsx', 'dr1-0004.xlsx', 'dr1-0007.xlsx']) {
    const book = uo.open(new Uint8Array(readFileSync(join(DR1, f))));
    const chartsBefore = charts;
    const sheets = book.sheets();
    const tables = newTables();
    for (let si = 0; si < sheets.length; si++) {
      book.activate(si, measurer);
      let asked = 0;
      const bytes = book.viewport({ sheet: si, width: 900, height: 600, dpr: 1, scrollX: 0, scrollY: 0, zoom: 1, selection: [1, 1, 1, 1, 1, 1] }, measurer, (reqs) => { asked += reqs.length; return dmlAnswer(reqs); });
      const list = decodeSlidesList(bytes, tables);
      shapes += list.items.filter((i) => i.op === 'text').length;
      charts += list.items.filter((i) => i.op === 'chartFont').length;
      for (const it of list.items) if (it.op === 'drawImage' && it.src.kind === 'media') pictures++;
      if (asked) assert.ok(list.items.length > 0);
    }
    for (let m = 0; m < tables.media.length; m++) {
      const md = book.media(m);
      assert.ok(md && md.bytes.length > 8 && md.part.startsWith('xl/media/'), `media ${m}`);
    }
    // the chart faces carry the tag of `paintChart`'s stack
    const chartFaces = tables.faces.filter((fc) => String(fc.family).startsWith('\u0001')).length;
    if (charts > chartsBefore) assert.ok(chartFaces > 0, `${f}: a tagged chart face`);
    book.free();
  }
  assert.ok(shapes > 5, `${shapes} text runs`);
  assert.ok(charts > 5, `${charts} chart fonts`);
  assert.ok(pictures > 0, `${pictures} pictures`);
});

test('the calc module applies the open-recalc (sheet-calc)', { skip: !uo.hasCalc() }, () => {
  // x1 workbooks whose plan is not `none` open with the engine; the viewer paints their values
  const X1 = join(here, '../../uo-xlsx/tests/captures/workbooks/x1');
  let recalculated = 0;
  for (let i = 0; i < 30; i++) {
    const book = uo.open(new Uint8Array(readFileSync(join(X1, `x1-${String(i).padStart(4, '0')}.xlsx`))));
    if (book.recalcPlan() !== 'none') { assert.equal(book.recalcStatus(), 0, `x1-${i}`); recalculated++; }
    book.activate(0, measurer);
    book.viewport({ sheet: 0, width: 640, height: 360, selection: [1, 1, 1, 1, 1, 1] }, measurer, dmlAnswer);
    book.free();
  }
  assert.ok(recalculated > 5);
});
