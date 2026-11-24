// The Sheet EDITOR exports (frontier C5b, src/sheet_edit.rs, js/sheet-editor.mjs) at their JS boundary, on the editor module
// (`sheet` + `sheet-edit`, with `count`): every operation family through the command line, the state the shell reads, the
// viewport after an edit (decoded with the generated codec), the save (reopened by the module and by the viewer exports of the
// same instance), refusals and junk, and M4 (10,000 editors through free() and through the FinalizationRegistry, live bytes
// back) plus a soak of edits with undo/redo (live bytes flat after the history fills).
//
//   cargo build -p uo-wasm --no-default-features --features sheet,sheet-edit,count --target wasm32-unknown-unknown --profile wasm
//   UO_WASM_PATH=<that .wasm> [UO_SHEET_EDIT_SOAK=10000] node --expose-gc --test crates/uo-wasm/tests/sheet_edit.test.mjs
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { join, dirname } from 'node:path';
import { fileURLToPath } from 'node:url';
import { bindSheet } from '../js/sheet.mjs';
import { bindSheetEditor, hasSheetEditorExports, SheetEditError } from '../js/sheet-editor.mjs';
import { decodeSlidesList, newTables } from '../js/gen/slides.ts';

const here = dirname(fileURLToPath(import.meta.url));
const wasmPath = process.env.UO_WASM_PATH ?? join(here, '../../../target/c5b-wasm/uo-sheet-editor-count.wasm');
const { instance } = await WebAssembly.instantiate(readFileSync(wasmPath), {});
const w = instance.exports;
const ue = bindSheetEditor(w);
const uo = bindSheet(w);
const SOAK = Number(process.env.UO_SHEET_EDIT_SOAK ?? 10_000);
const SAMPLES = join(here, '../../../../sheet/samples');
const X1 = join(here, '../../uo-xlsx/tests/captures/workbooks/x1');
const sample = (n) => new Uint8Array(readFileSync(join(SAMPLES, n)));
const x1 = (i) => new Uint8Array(readFileSync(join(X1, `x1-${String(i).padStart(4, '0')}.xlsx`)));
const measurer = { advance: (_f, cp) => (cp > 0xffff ? 12 : 6), width: (_f, t) => 6 * t.length, box: () => [9, 3] };
const gc = () => { if (globalThis.gc) { globalThis.gc(); globalThis.gc(); } };

test('the module has the editor exports and the viewer exports', () => {
  assert.ok(hasSheetEditorExports(w));
  assert.equal(typeof w.uo_sheet_open, 'function');
});

test('entry, formulas, the bar, undo/redo (pt-BR)', () => {
  const ed = ue.open(x1(0), { locale: 'pt-BR' });
  ed.cmd('select', 'A1');
  let r = ed.cmd('enter', '1,5');
  assert.deepEqual(r.result, { ok: true, views: [0], grew: false });
  assert.equal(r.state.bar.edit, '1,5');
  assert.equal(r.state.canUndo, true);
  assert.equal(r.state.undoLabel, 'typing');
  ed.cmd('select', 'D1');
  r = ed.cmd('enter', '=SOMA(A1:A5)*2');
  assert.equal(r.state.bar.formula, '=SOMA(A1:A5)*2');
  r = ed.cmd('enter', '=SOMA(');
  assert.equal(r.result.ok, false);
  assert.match(r.result.message, /argumentos/);
  ed.cmd('select', 'E1');
  ed.cmd('enter', '=SEQUÊNCIA(3;2)');
  r = ed.cmd('bar_text', 6, 3);
  assert.equal(r.result.ghost, true);
  r = ed.cmd('undo');
  assert.equal(r.result, true);
  assert.equal(r.state.canRedo, true);
  r = ed.cmd('redo');
  assert.equal(r.result, true);
  assert.equal(ed.state().dirty, true);
  ed.free();
});

test('every operation family through the command line', () => {
  const ed = ue.open(sample('showcase.xlsx'), { locale: 'en-US' });
  const run = (op, ...a) => ed.cmd(op, ...a);
  run('select', 'B2:C4');
  run('font', 'bold=1', 'size=14', 'color=#C00000');
  run('fill', 'fill=#FFFF00');
  run('border', 'thickOutside', '#0000FF');
  run('format', 'bold=1', 'fill=#C6EFCE', 'h=center', 'border.bottom=thin:#000000');
  run('numfmt', 'kind:percent');
  run('numfmt', 'code:#,##0.000');
  run('decimals', '+1');
  run('align', 'h=left', 'indent=2', 'v=top', 'wrap=1');
  run('select', 'B:B');
  run('fill', 'fill=#DDEBF7');
  run('select', 'B2:C3');
  assert.equal(typeof run('merge_loses_data').result, 'boolean');
  run('merge', 'center');
  run('select', 'A6');
  run('unmerge');
  run('select', 'A1');
  run('enter', '=SUM(A2:A5)');
  run('select', 'A1:A5');
  run('fill_down');
  run('select', 'A1:D2');
  run('fill_right');
  run('select', 'A1:B2');
  const text = run('copy').result;
  assert.equal(typeof text, 'string');
  run('select', 'D6');
  run('paste', '0');
  run('select', 'H2');
  run('paste', '1');
  run('select', 'A9');
  run('paste', '0', '1\tabc\r\n2\t=A9*2\r\n');
  run('select', 'A1:C3');
  run('clear', 'formats');
  run('select', 'B2');
  run('enter', 'texto ção');
  assert.deepEqual(run('find', 'ção', 'a').result, { sheet: 0, ref: 'B2' });
  assert.equal(run('replace_all', 'ção', 'cao', '').result, 1);
  assert.deepEqual(run('freeze', '1', '1').result, { cols: 1, rows: 1 });
  assert.equal(run('gridlines', '0').result, false);
  run('select', 'B:C');
  run('col_width', '20');
  assert.equal(run('column_width', '2').result, 20);
  run('select', '2:3');
  run('row_height', '30');
  assert.equal(run('row_height_pt', '2').result, 30);
  run('autofit_rows');
  run('select', '4:5');
  run('hide', 'rows');
  run('select', '3:6');
  run('unhide', 'rows');
  run('resize_drag', 'col', '2', '100');
  run('select', 'A1:C4');
  assert.equal(run('autosum', 'SUM').result, true);
  run('select', 'A20');
  const t = run('autosum', 'SUM').result;
  assert.equal(typeof t, 'string');
  assert.equal(run('state').state.editing.text, t);
  run('commit');
  run('insert_function', 'SUM');
  run('editor_text', '=SUM(A1:A3)');
  assert.equal(run('commit').result, true);
  assert.ok(run('function_names').result.length > 400);
  run('recalc');
  const before = run('state').state.sheets.length;
  assert.equal(run('add_sheet', 'Resumo').result, 'Resumo');
  const st = run('state').state;
  assert.equal(st.sheets.length, before + 1);
  assert.equal(st.sheet, before);
  for (let i = 0; i < 5; i++) run('undo');
  for (let i = 0; i < 5; i++) run('redo');
  // the save reopens in a new editor and in the viewer of the same instance
  const bytes = ed.save();
  const again = ue.open(bytes, { locale: 'en-US' });
  assert.deepEqual(again.state().sheets, st.sheets);
  again.free();
  const book = uo.open(bytes);
  assert.ok(book.sheets().some((s) => s.name === 'Resumo'));
  book.free();
  run('new_workbook');
  assert.deepEqual(run('state').state.sheets, ['Sheet1']);
  ed.free();
});

test('the viewport shows the edit', () => {
  const ed = ue.open(sample('showcase.xlsx'), { locale: 'en-US' });
  const q = { sheet: 0, width: 800, height: 500, dpr: 1, scrollX: 0, scrollY: 0, zoom: 1, selection: [1, 1, 1, 1, 1, 1] };
  const tables = newTables();
  const before = decodeSlidesList(ed.viewport(q, measurer), tables);
  ed.cmd('select', 'A1');
  ed.cmd('enter', 'Zebra-Uniq-77');
  const tables2 = newTables();
  const after = decodeSlidesList(ed.viewport(q, measurer), tables2);
  const textOf = (list) => list.items.filter((i) => i.op === 'glyphRun').map((i) => list.texts.slice(i.text.start, i.text.start + i.text.len)).join('|');
  assert.ok(!textOf(before).includes('Zebra-Uniq-77'));
  assert.ok(textOf(after).includes('Zebra-Uniq-77'), 'the frame after the edit has the new text');
  assert.ok(ed.extent(0, measurer).width > 0);
  ed.free();
});

test('blank workbook, refusals, junk and null handles', () => {
  const ed = ue.open(new Uint8Array(), { locale: 'pt-BR' });
  assert.deepEqual(ed.state().sheets, ['Planilha1']);
  assert.throws(() => ed.cmd('nope'), SheetEditError);
  assert.throws(() => ed.cmd('select', '!!!'), SheetEditError);
  assert.throws(() => ed.cmd('format', 'size'), SheetEditError);
  assert.throws(() => ed.cmd('numfmt', 'kind:nothing'), SheetEditError);
  assert.throws(() => ed.cmd('set_cell', 'x'), SheetEditError);
  ed.cmd('select', 'XFD1048576');
  assert.equal(ed.cmd('enter', '1').result.ok, true);
  ed.free();
  assert.throws(() => ed.cmd('state'), (e) => e.status === 11);
  assert.throws(() => ue.open(new Uint8Array([1, 2, 3, 4])), (e) => e.status === 3);
});

test(`M4: ${SOAK} editors created and dropped (free and finalizer), live bytes back`, async () => {
  const bytes = x1(5);
  // warm-up (lazily built static tables of the calc and the grid)
  for (let i = 0; i < 3; i++) { const e = ue.open(bytes); e.cmd('select', 'A1'); e.cmd('enter', '=1+1'); e.save(); e.free(); }
  gc();
  const base = ue.liveBytes();
  for (let i = 0; i < SOAK; i++) ue.open(bytes).free();
  assert.equal(ue.liveBytes(), base, 'explicit free');
  // the finalizer path: handles dropped without free()
  const n = Math.min(SOAK, 2000);
  for (let i = 0; i < n; i++) ue.open(bytes);
  for (let k = 0; k < 50 && ue.liveBytes() !== base; k++) { gc(); await new Promise((r) => setTimeout(r, 20)); }
  assert.equal(ue.liveBytes(), base, 'finalizer');
});

test('M2: 600 edits with undo/redo keep live bytes flat once the history is full', () => {
  const ed = ue.open(x1(0), { locale: 'en-US' });
  const samples = [];
  for (let i = 0; i < 600; i++) {
    ed.cmd('select', `B${(i % 20) + 2}`);
    ed.cmd('enter', String(i % 7));
    if (i % 10 === 9) { ed.cmd('undo'); ed.cmd('redo'); }
    if (i >= 300 && i % 50 === 0) samples.push(ue.liveBytes());
  }
  const spread = Math.max(...samples) - Math.min(...samples);
  assert.ok(spread <= 64 * 1024, `live bytes spread ${spread} after warm-up`);
  ed.free();
});
