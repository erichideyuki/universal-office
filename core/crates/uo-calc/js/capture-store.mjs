// Parity capture group `store` (frontier S0, record core/docs/modules/uo-calc.md "Parity" / "Store facet"): the TS
// adapter that turns the sheet-core model into the STORE FACET, plus the input a reader would stream into the Rust
// store (S0 has no xlsx reader yet: the Rust test feeds the store through a test-only loader from these lines).
//
//   node core/crates/uo-calc/js/capture-store.mjs                    # CI: sheet/samples (no giants) + generated x1
//   node --max-old-space-size=8192 core/crates/uo-calc/js/capture-store.mjs --big --out core/target/sheet-cap/store-big.capture
//                                                                    # local: + the giants + generated x1-big
//   options: --only <substring>  --ts-root <dir>
//
// Records `samples/<file>` and `gen/<group>/<NNNN>` (the keys of uo-xlsx's sheet-*.capture, which holds the same
// st/fo/rw facets made by capture-sheet.mjs: the Rust test checks both), facets (`count:digest`, lines in the sidecar
// `<out>.side.gz`, `<key>\t<facet>\t<line>`):
//   input    `len:sha16` of the .xlsx;  date1904  0 | 1;  sheets  worksheet count
//   sst      one JSON string per shared-string entry: textOf(entry) (`text ?? runs.join`), the calc text
//   in<i>    the model of sheet i in FILE order: `R<canonJson(row attrs + r)>` per <row>, `C<col> <canonJson(cell)>`
//            per <c> (col = refToRowCol(ref).col; the cell's model fields as read: ref t v s f fa cm vm is)
//   st<i>    the store facet: one canonical JSON line per position, the LAST duplicate wins, sorted by (row, col):
//            {c, cm, f, fa, is, r, ref (only when not the canonical A1 of (c, r)), s, t, v, vm}
//   fo<i>    `sorted` or `count:digest` of the `row:col` sequence in file order;  rw<i>  row attribute lines
//   cv<i>    `cellValue(cell, sst, date1904)` of every st<i> cell: `row,col=<value text>` (0-based, as the engine)
//   traps    the per-parameter traps of numberLiteral / numberText / fileCellValue / errorOf / cellValue
//            (`<fn> <canonical args>=<result>` lines)
process.env.TZ = 'UTC';
const { readFileSync, mkdirSync, existsSync, createWriteStream } = await import('node:fs');
const { join, dirname } = await import('node:path');
const { createHash } = await import('node:crypto');
const { createGzip } = await import('node:zlib');
const { pathToFileURL } = await import('node:url');
const { CaptureWriter } = await import('../../uo-testkit/js/testkit.mjs');
const { canonJson } = await import('../../uo-pptx/js/lib/canon-json.mjs');
const C = await import('./lib/common.mjs');
const X = await import('../../uo-xlsx/js/lib/common.mjs');
const { GROUPS, buildWorkbook, groupKey, workbookFile } = await import('../../uo-xlsx/js/lib/gen-workbook.mjs');

const a = C.args();
const root = C.tsRoot(a);
const big = a.flag('--big');
const only = a.opt('--only');
const out = a.opt('--out', join(C.capturesDir, 'store.capture'));
const { calc } = await C.loadCalc(root);
const SC = await import(pathToFileURL(join(root, 'sheet', 'packages', 'sheet-core', 'dist', 'index.js')).href);
const vt = C.makeValueText(calc);

const sha = (s) => createHash('sha256').update(s).digest('hex');
const listDig = (lines) => `${lines.length}:${sha(Buffer.from(lines.join('\n'), 'utf8')).slice(0, 32)}`;
const textOf = (t) => t?.text ?? t?.runs?.map((r) => r.text).join('') ?? '';

const w = new CaptureWriter([
  'store facet (frontier S0): sheet-core model -> store facet + the file-order input of the test-only loader, see core/crates/uo-calc/js/capture-store.mjs and core/docs/modules/uo-calc.md "Parity"',
]);
C.stamp(w, root, { canon: 'canon-json.mjs of uo-pptx (sorted keys, String(n), -0, {"$num"}, strings over 4096 units digested)' });
const side = [];

/** The store facet of one worksheet model (the capture-sheet.mjs `storeOf` rule) + the loader input. */
function sheetFacets(ws, sst, date1904) {
  const map = new Map();
  const seq = [];
  const input = [];
  const rowMeta = [];
  let sorted = true, prev = -1;
  for (const row of ws.rows) {
    const meta = { customFormat: row.customFormat, customHeight: row.customHeight, hidden: row.hidden, ht: row.height, r: row.r, s: row.style, spans: row.spans, thickBot: row.thickBot, thickTop: row.thickTop };
    if (Object.keys(meta).some((k) => k !== 'r' && meta[k] !== undefined)) rowMeta.push(canonJson(meta));
    input.push(`R${canonJson(meta)}`);
    for (const c of row.cells) {
      let col;
      try { col = SC.refToRowCol(c.ref).col; } catch { col = -1; }
      input.push(`C${col} ${canonJson({ cm: c.cm, f: c.formula, fa: c.formulaAttrs, is: c.inlineString, ref: c.ref, s: c.style, t: c.type, v: c.value, vm: c.vm })}`);
      const k = row.r * 20000 + col;
      seq.push(`${row.r}:${col}`);
      if (k <= prev) sorted = false;
      prev = k;
      const canonRef = col > 0 ? SC.rowColToRef(col, row.r) : undefined;
      map.set(k, { cell: c, line: { c: col, cm: c.cm, f: c.formula, fa: c.formulaAttrs, is: c.inlineString, r: row.r, ref: c.ref !== canonRef ? c.ref : undefined, s: c.style, t: c.type, v: c.value, vm: c.vm } });
    }
  }
  const keys = [...map.keys()].sort((x, y) => x - y);
  const st = keys.map((k) => canonJson(map.get(k).line));
  const cv = keys.map((k) => { const { cell, line } = map.get(k); return `${line.r - 1},${line.c - 1}=${vt(calc.cellValue(cell, sst, date1904))}`; });
  return { input, st, cv, order: sorted ? 'sorted' : listDig(seq), rowMeta };
}

function record(key, bytes) {
  w.set(key, 'input', `${bytes.length}:${sha(bytes).slice(0, 16)}`);
  const sx = (facet, lines) => { for (const l of lines) side.push(`${key}\t${facet}\t${l}`); };
  let pkg, wb, cx;
  try { pkg = SC.openSpreadsheet(bytes); wb = SC.readWorkbook(pkg); cx = SC.colorContext(pkg); } catch (e) { w.set(key, 'read', `err:${X.oneLine(e?.message ?? e)}`); return; }
  const sst = SC.readSharedStrings(pkg, cx);
  const date1904 = !!wb.date1904;
  w.set(key, 'date1904', date1904 ? 1 : 0);
  const sstLines = sst.map((e) => JSON.stringify(textOf(e)));
  w.set(key, 'sst', listDig(sstLines));
  sx('sst', sstLines);
  let n = 0;
  for (const ref of wb.sheets) {
    if (!ref.target) continue;
    let ws;
    try { ws = SC.readWorksheet(pkg, ref.target, cx); } catch (e) { w.set(key, `in${n}`, `err:${X.oneLine(e?.message ?? e)}`); n++; continue; }
    const f = sheetFacets(ws, sst, date1904);
    w.set(key, `in${n}`, listDig(f.input)); sx(`in${n}`, f.input);
    w.set(key, `st${n}`, listDig(f.st)); sx(`st${n}`, f.st);
    w.set(key, `fo${n}`, f.order);
    w.set(key, `rw${n}`, listDig(f.rowMeta)); sx(`rw${n}`, f.rowMeta);
    w.set(key, `cv${n}`, listDig(f.cv)); sx(`cv${n}`, f.cv);
    n++;
  }
  w.set(key, 'sheets', n);
}

// ---------------------------------------------------------------- per-parameter traps (SKILL.md capture source 3)
function traps() {
  const lines = [];
  const num = (x) => canonJson(x);
  const NUMS = [0, -0, 1, -1, 0.1, 0.5, 1 / 3, 1e20, 1e21, -1e21, 1.5e21, 1e-6, 1e-7, 1.5e-7, 123456789012345680000, 2 ** 31 - 1, 2 ** 31, -(2 ** 31), -(2 ** 31) - 1,
    2 ** 32, 2 ** 53, 2 ** 53 + 2, 9007199254740993, 5e-324, 2.2250738585072014e-308, 1.7976931348623157e308, NaN, Infinity, -Infinity, 0.1 + 0.2, 100, 1e300, -1.5e-300];
  for (const x of NUMS) {
    lines.push(`numberLiteral ${num(x)}=${JSON.stringify(calc.numberLiteral(x))}`);
    lines.push(`numberText ${num(x)}=${JSON.stringify(calc.numberText(x))}`);
    lines.push(`fileCellValue ${num(x)}=${canonJson(calc.fileCellValue(x))}`);
  }
  for (const v of ['', 'x', '\uD800', '😀', 'ção', true, false, null]) lines.push(`fileCellValue ${canonJson(v)}=${canonJson(calc.fileCellValue(v))}`);
  for (const code of Object.keys(calc.ERROR_TYPE)) lines.push(`fileCellValue ${JSON.stringify('#err ' + code)}=${canonJson(calc.fileCellValue(calc.CalcError.of(code)))}`);
  const ERRS = [...Object.keys(calc.ERROR_TYPE), '#n/a', ' #N/A ', '#N/A ', '#dıv/0!', '#ﬁeld!', '#buſy!', '#DIV/0', 'x', '', ' #NULL!﻿', '#GETTING_DATA!', '#ref!', '#Value!'];
  for (const e of ERRS) lines.push(`errorOf ${JSON.stringify(e)}=${vt(calc.errorOf(e))}`);
  const SST = [{ text: 'zero' }, { runs: [{ text: 'r' }, { text: 'uns' }] }, {}, { text: '' }];
  const TYPES = [undefined, 'n', 's', 'str', 'inlineStr', 'b', 'e', 'd', 'foo', ''];
  const VALUES = [undefined, '', ' ', '0', '1', '-0', ' 5', '5 ', '1e3', '1E+21', '0x10', '0b11', '0o7', '+5', '.5', '5.', 'Infinity', '-Infinity', 'NaN', 'abc', '1.5',
    '3', '-1', '1.0', '4294967296', '#N/A', ' #n/a ', '#dıv/0!', 'true', 'TRUE', 'false', ' 7', '1_000',
    '2024-01-15', '2024-01-15T12:30', '2024-01-15T12:30:15.5', '2024-01-15T12', '0099-13-45', '1900-02-29', '1900-03-01', '1899-12-31', '0000-01-01', '9999-12-31T23:59:59.999', '2024-1-5', '٢٠٢٤-01-15', '2024-01-15junk', '\uD800'];
  const ISS = [undefined, { text: 'inl' }, { runs: [{ text: 'a' }, { text: 'b' }] }, {}];
  for (const t of TYPES) for (const v of VALUES) for (const f of [undefined, 'A1']) for (const is of (t === 'inlineStr' || t === undefined ? ISS : [undefined])) for (const d1904 of [false, true]) {
    if (d1904 && t !== 'd') continue;
    const c = { ref: 'A1', ...(t !== undefined ? { type: t } : {}), ...(v !== undefined ? { value: v } : {}), ...(f !== undefined ? { formula: f } : {}), ...(is !== undefined ? { inlineString: is } : {}) };
    lines.push(`cellValue ${canonJson({ d1904, f, is, t, v })}=${vt(calc.cellValue(c, SST, d1904))}`);
  }
  return lines;
}

// ---------------------------------------------------------------- run
const t0 = performance.now();
let count = 0;
for (const rel of X.trackedSamples()) {
  const key = `samples/${rel}`;
  if (only && !key.includes(only)) continue;
  const bytes = X.readSample(rel);
  if (!big && bytes.length > 400_000) continue;
  record(key, bytes);
  count++;
}
for (const group of big ? ['x1', 'x1-big'] : ['x1']) {
  const g = GROUPS[group];
  const dir = join(X.capturesDir, 'workbooks', group);
  for (let i = 0; i < g.count; i++) {
    const key = groupKey(group, i);
    if (only && !key.includes(only)) continue;
    const file = join(dir, workbookFile(group, i));
    record(key, existsSync(file) ? new Uint8Array(readFileSync(file)) : new Uint8Array(buildWorkbook(group, i).bytes));
    count++;
  }
}
const tl = traps();
w.set('traps', 'traps', listDig(tl));
for (const l of tl) side.push(`traps\ttraps\t${l}`);
w.meta('stats', `workbooks=${count} traps=${tl.length}`);
mkdirSync(dirname(out), { recursive: true });
w.write(out);
{
  const gz = createGzip({ level: 9 });
  const done = new Promise((res, rej) => gz.pipe(createWriteStream(out.replace(/\.capture$/, '.side.gz'))).on('finish', res).on('error', rej));
  for (let i = 0; i < side.length; i += 10000) gz.write(side.slice(i, i + 10000).join('\n') + '\n');
  gz.end();
  await done;
}
console.log(`store: ${count} workbooks, ${tl.length} traps in ${((performance.now() - t0) / 1000).toFixed(1)} s -> ${out}`);
