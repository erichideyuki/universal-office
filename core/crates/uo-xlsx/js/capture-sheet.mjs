// Parity captures of the Sheet read/save path and the calc over workbooks (plan section 4.3 "Sheet", record
// core/docs/modules/uo-xlsx.md "Captures"), from the frozen TS run headless: the real `<universal-sheet>` element on the
// fake DOM the TS's own editing tests use (harness/build-harness.mjs), plus sheet-core's readers for the model.
// Host injection (decision C1): TZ=UTC, `Date` frozen at 2025-01-15 12:00 UTC (`new Date()` / `Date.now()`), and
// `Math.random` replaced by a seeded xorshift32 whose values are recorded in call order.
//
//   node core/crates/uo-xlsx/js/capture-sheet.mjs --samples         # sheet/samples (giants sliced) -> tests/captures/sheet-samples.*
//   node core/crates/uo-xlsx/js/capture-sheet.mjs --gen x1          # the committed generated workbooks -> tests/captures/sheet-x1.*
//   node --max-old-space-size=8192 core/crates/uo-xlsx/js/capture-sheet.mjs --gen x1-big --out core/target/sheet-cap/sheet-x1-big.capture
//   node core/crates/uo-xlsx/js/capture-sheet.mjs --dir <folder>    # any .xlsx/.xls folder (e.g. Excel COM SaveAs outputs)
//   options: --only <substring>  --steps <n> (edit steps per workbook, default 8)  --lang pt-BR|en-US (formula-locale)
//            --parts | --no-parts (the changed parts' XML in the sidecar; default on for CI groups, samples and --dir)
//            --slice <rows> (store/display rows kept in the sidecars of the giants, default 200)  --ts-root <dir>
//
// Facets of a workbook record (`samples/<file>` | `gen/<group>/<NNNN>` | `dir/<file>`); `len:sha256[0..32]` digests of
// canonical texts unless said otherwise; the texts themselves are in the gz sidecars (`<out>.<kind>.gz`, lines
// `<key>\t<facet>\t<text>`), except for the giants, whose sidecars keep only the first --slice rows:
//   input   `len:sha256[0..16]` of the file;  read  ok | err:<message>
//   wb styles theme sst     model JSON (sorted keys: uo-pptx/js/lib/canon-json.mjs) of readWorkbook, readStyles,
//                           colorContext().theme, readSharedStrings
//   ws<i>   readWorksheet(sheet i) WITHOUT `rows` (canonical JSON);  sheets  number of worksheets
//   st<i>   STORE facet of sheet i (uo-calc.md "Parity"): one canonical JSON line per cell position, the component's
//           index rule (row from `<row r>`, column from the ref, the LAST duplicate wins), sorted by (row, col):
//           {c, r, t, v, s, f, fa, cm, vm, is, ref (only when not the canonical A1 of (c, r))}; `<count>:<digest>`
//   fo<i>   file order: `sorted` or `<count>:<digest>` of the `row:col` sequence in file order (FileOrder side table)
//   rw<i>   row metadata lines {r, ht, customHeight, hidden, s, customFormat, spans, thickTop, thickBot}
//   plan    the open-recalc plan (shouldRecalcOnOpen: none | full | partial:<n cells>)
//   ord     evaluation order of the open (every Engine.evaluateRec as `sheet,row,col`; count:digest) and
//   rnd     the Math.random values consumed (f64 hex, count:digest);  circ  `Engine.circular` sorted;  ierr  internalErrors
//   pv<i>   the store facet AFTER the open (the model the component holds: writeBack of the recalc applied)
//   ev      engine values (when the open built the engine): every formula cell and spill cell, value text, sorted
//   d<i>    display text of sheet i (`debugCellDisplays()`: the TS Range.Text equivalent), lines `ref\ttext`
//   save0   surgical save after a no-op: `<parts>:<digest of name/len/sha lines>`; `chg0` parts whose bytes differ
//           from the input (decompressed comparison)
//   e<k>    edit step k (seeded script over the element's public API): the op;  e<k>.r its result;  e<k>.ord /
//           e<k>.rnd / e<k>.save / e<k>.chg / e<k>.d (active sheet display) after the step
// Not captured here: the viewport op stream over `paintRegion` (frontier C4 records it with its own recorder).
process.env.TZ = 'UTC';
const { readFileSync, readdirSync, mkdirSync, writeFileSync } = await import('node:fs');
const { join, dirname, basename } = await import('node:path');
const { createHash } = await import('node:crypto');
const { createGzip } = await import('node:zlib');
const { createWriteStream } = await import('node:fs');
const { CaptureWriter, Rng } = await import('../../uo-testkit/js/testkit.mjs');
const { canonJson } = await import('../../uo-pptx/js/lib/canon-json.mjs');
const C = await import('./lib/common.mjs');
const { unzip } = await import('./lib/unzip.mjs');
const { GROUPS, buildWorkbook, groupKey, workbookFile } = await import('./lib/gen-workbook.mjs');
const { loadHarness } = await import('./harness/build-harness.mjs');

const a = C.args();
const root = C.tsRoot(a);
const only = a.opt('--only');
const STEPS = Number(a.opt('--steps', 8));
const LANG = a.opt('--lang', 'pt-BR');
const SLICE = Number(a.opt('--slice', 200));
// the changed parts' XML in the sidecar: on for the CI groups, the samples and --dir
const PARTS = a.flag('--parts') || ((!a.opt('--gen') || GROUPS[a.opt('--gen')]?.ci) && !a.flag('--no-parts'));

// ---------------------------------------------------------------- host injection (C1)
const FIXED = Date.UTC(2025, 0, 15, 12, 0, 0);
const RealDate = Date;
class FixedDate extends RealDate {
  constructor(...x) { if (x.length === 0) super(FIXED); else super(...x); }
  static now() { return FIXED; }
}
globalThis.Date = FixedDate;
let rng = new Rng(1);
const RND = [];
Math.random = () => { const x = rng.next() / 4294967296; RND.push(x); return x; };

const h = await loadHarness(root);
const { sheetCore: SC, calc } = h;
const ORDER = [];
{
  const proto = calc.Engine.prototype;
  const impl = proto.evaluateRec;
  if (typeof impl !== 'function') throw new Error('Engine.prototype.evaluateRec not found: the evaluation-order facet needs it');
  proto.evaluateRec = function (f) { ORDER.push(`${f.sheet},${f.row},${f.col}`); return impl.call(this, f); };
}
const vt = makeValueText(calc);

// ---------------------------------------------------------------- helpers
const sha = (s) => createHash('sha256').update(s).digest('hex');
const dig = (s) => { const b = Buffer.from(s, 'utf8'); return `${b.length}:${sha(b).slice(0, 32)}`; };
const listDig = (lines) => `${lines.length}:${sha(Buffer.from(lines.join('\n'), 'utf8')).slice(0, 32)}`;
function makeValueText(cx) {
  const f64 = C.f64hex;
  const go = (v) => {
    if (v === null || v === undefined) return '_';
    if (typeof v === 'number') return `n${f64(v)}`;
    if (typeof v === 'string') return `s${JSON.stringify(v)}`;
    if (typeof v === 'boolean') return v ? 'b1' : 'b0';
    if (v instanceof cx.CalcError) return `e${v.code}`;
    return `?${v?.constructor?.name}`;
  };
  return go;
}

/** The store facet lines of a worksheet model + the file order + row metadata. */
function storeOf(ws, sliceRows = Infinity) {
  const map = new Map();
  const seq = [];
  let sorted = true, prev = -1;
  const rowMeta = [];
  for (const row of ws.rows) {
    const meta = { customFormat: row.customFormat, customHeight: row.customHeight, hidden: row.hidden, ht: row.height, r: row.r, s: row.style, spans: row.spans, thickBot: row.thickBot, thickTop: row.thickTop };
    if (Object.keys(meta).some((k) => k !== 'r' && meta[k] !== undefined)) rowMeta.push(canonJson(meta));
    for (const c of row.cells) {
      let col;
      try { col = SC.refToRowCol(c.ref).col; } catch { col = -1; }
      const k = row.r * 20000 + col;
      seq.push(`${row.r}:${col}`);
      if (k <= prev) sorted = false;
      prev = k;
      const canonRef = col > 0 ? SC.rowColToRef(col, row.r) : undefined;
      map.set(k, { c: col, cm: c.cm, f: c.formula, fa: c.formulaAttrs, is: c.inlineString, r: row.r, ref: c.ref !== canonRef ? c.ref : undefined, s: c.style, t: c.type, v: c.value, vm: c.vm });
    }
  }
  const keys = [...map.keys()].sort((x, y) => x - y);
  const h = createHash('sha256');
  const slice = [];
  keys.forEach((k, i) => {
    const line = canonJson(map.get(k));
    if (i) h.update('\n');
    h.update(line);
    if (map.get(k).r <= sliceRows) slice.push(line);
  });
  return {
    count: keys.length, digest: `${keys.length}:${h.digest('hex').slice(0, 32)}`, slice,
    order: sorted ? 'sorted' : listDig(seq), rowMeta,
  };
}

/** Parts of a package (name, decompressed bytes) as digest lines, and the names that differ from `base`. */
function partsOf(bytes, base) {
  const parts = unzip(bytes);
  const lines = parts.map((p) => `${p.name} ${p.data.length} ${sha(p.data).slice(0, 32)}`);
  const changed = base ? parts.filter((p) => !base.has(p.name) || !base.get(p.name).equals(p.data)).map((p) => p.name) : [];
  return { lines, changed, parts };
}

// ---------------------------------------------------------------- the scripted edits (public API only)
const TEXTS = { 'pt-BR': ['42', '-0,5', '1,5', '50%', '12/03/2024', 'R$ 10,00', 'texto ção', '😀 x', '  esp  ', "'007", '1E+21', 'VERDADEIRO', '0,1', '1.234,5', '3/4'],
  'en-US': ['42', '-0.5', '1.5', '50%', '3/12/2024', '$10.00', 'texto ção', '😀 x', '  esp  ', "'007", '1E+21', 'TRUE', '0.1', '1,234.5', '3/4'] };
const FORMULAS = { 'pt-BR': ['=SOMA(A1:A5)*2', '=SE(A2>1;"s";"n")', '=ALEATÓRIO()', '=A1&"x"', '=SEQUÊNCIA(3;2)', '=PROCV(1;A1:C10;2;FALSO)', '=HOJE()', '=1/0', '=B2+1', '=AGORA()', '=ÚNICO(A1:A6)', '=C1+D1', '=A1'],
  'en-US': ['=SUM(A1:A5)*2', '=IF(A2>1,"s","n")', '=RAND()', '=A1&"x"', '=SEQUENCE(3,2)', '=VLOOKUP(1,A1:C10,2,FALSE)', '=TODAY()', '=1/0', '=B2+1', '=NOW()', '=UNIQUE(A1:A6)', '=C1+D1', '=A1'] };
const col = (c) => SC.rowColToRef(c, 1).replace(/\d+$/, '');
function script(R, maxC, maxR) {
  const cell = () => `${col(R.int(1, Math.max(2, Math.min(maxC + 1, 12))))}${R.int(1, Math.max(2, Math.min(maxR + 1, 40)))}`;
  const range = () => { const c = R.int(1, Math.max(2, Math.min(maxC, 10))), r = R.int(1, Math.max(2, Math.min(maxR, 30))); return `${col(c)}${r}:${col(c + R.int(0, 2))}${r + R.int(0, 3)}`; };
  const ops = [
    () => { const c = cell(), t = R.pick(TEXTS[LANG]); return [`text ${c} ${JSON.stringify(t)}`, (el) => { el.selectRange(c); return el.setActiveCellText(t); }]; },
    () => { const c = cell(), f = R.pick(FORMULAS[LANG]); return [`formula ${c} ${JSON.stringify(f)}`, (el) => { el.selectRange(c); return el.setActiveCellText(f); }]; },
    () => { const c = cell(), f = R.pick(FORMULAS[LANG]); return [`formula ${c} ${JSON.stringify(f)}`, (el) => { el.selectRange(c); return el.setActiveCellText(f); }]; },
    () => { const g = range(), f = { bold: R.chance(0.7), italic: R.chance(0.3), size: R.pick([undefined, 9, 14]), color: R.pick([undefined, '#C00000', null]) }; return [`font ${g} ${JSON.stringify(f)}`, (el) => { el.selectRange(g); el.setFont(f); }]; },
    () => { const g = range(), k = R.pick(['percent', 'currency', 'shortDate', 'scientific', 'text', 'comma', { code: '0.000' }, { code: '#,##0.00' }, { code: 'dd/mm/yyyy' }]); return [`numfmt ${g} ${JSON.stringify(k)}`, (el) => { el.selectRange(g); el.setNumberFormat(k); }]; },
    () => { const g = range(), k = R.pick(['all', 'outside', 'thickOutside', 'none', 'bottom']); return [`borders ${g} ${k}`, (el) => { el.selectRange(g); el.setBorders(k); }]; },
    () => { const g = range(), fill = R.pick(['#FFFF00', '#C6EFCE', null]); return [`fill ${g} ${fill}`, (el) => { el.selectRange(g); el.setFormat({ fill }); }]; },
    () => { const g = range(); return [`merge ${g}`, (el) => { el.selectRange(g); el.merge(R.pick(['center', 'across', 'cells'])); }]; },
    () => { const g = range(), d = R.pick(['down', 'right']); return [`fill-${d} ${g}`, (el) => { el.selectRange(g); el.fill(d); }]; },
    () => { const g = range(), k = R.pick(['contents', 'formats', 'all']); return [`clear-${k} ${g}`, (el) => { el.selectRange(g); el.clear(k); }]; },
    () => { const x = R.int(0, 2), y = R.int(0, 3); return [`freeze ${x},${y}`, (el) => el.freezePanes(x, y)]; },
    () => { const g = range(), w = R.pick([3, 20, 45.5]); return [`width ${g} ${w}`, (el) => { el.selectRange(g); el.setColumnWidth(w); }]; },
    () => { const g = range(); return [`hide-rows ${g}`, (el) => { el.selectRange(g); el.setHidden('rows', true); }]; },
    () => { const q = R.pick(['a', 'ção', '1']), r = R.pick(['A', 'X', '']); return [`replaceAll ${JSON.stringify(q)} ${JSON.stringify(r)}`, (el) => el.replaceAll(q, r)]; },
    () => { const g = range(); return [`autosum ${g}`, (el) => { el.selectRange(g); el.autoSum('SUM'); }]; },
    () => { const g = range(), c = cell(); return [`copy ${g} paste ${c}`, (el) => { el.selectRange(g); el.copy(); el.selectRange(c); el.paste(); }]; },
    () => ['undo', (el) => el.undo()],
    () => ['redo', (el) => el.redo()],
    () => ['recalculate', (el) => el.recalculate()],
  ];
  return Array.from({ length: STEPS }, () => R.pick(ops)());
}

// ---------------------------------------------------------------- one workbook
function record(w, side, key, bytes, seedText) {
  const out = { key };
  w.set(key, 'input', `${bytes.length}:${sha(bytes).slice(0, 16)}`);
  const big = bytes.length > 400_000;
  const sx = (facet, text) => side.push(`${key}\t${facet}\t${text}`);
  // ---- model through sheet-core (fresh read; the element mutates its own copy)
  let pkg, wb, cx;
  try {
    pkg = SC.openSpreadsheet(bytes);
    wb = SC.readWorkbook(pkg);
    cx = SC.colorContext(pkg);
  } catch (e) {
    w.set(key, 'read', `err:${C.oneLine(e?.message ?? e)}`);
    return { ...out, err: true };
  }
  w.set(key, 'read', 'ok');
  const model = (facet, value) => { const t = canonJson(value); w.set(key, facet, dig(t)); if (!big) sx(facet, t); };
  model('wb', wb);
  model('styles', SC.readStyles(pkg, cx));
  model('theme', cx.theme);
  const sst = SC.readSharedStrings(pkg, cx);
  model('sst', sst);
  const worksheets = [];
  let n = 0;
  for (const ref of wb.sheets) {
    if (!ref.target) { worksheets.push(undefined); continue; }
    let ws;
    try { ws = SC.readWorksheet(pkg, ref.target, cx); } catch (e) { w.set(key, `ws${n}`, `err:${C.oneLine(e?.message ?? e)}`); worksheets.push(undefined); n++; continue; }
    worksheets.push(ws);
    const { rows, ...rest } = ws;
    model(`ws${n}`, rest);
    const st = storeOf(ws, big ? SLICE : Infinity);
    w.set(key, `st${n}`, st.digest);
    for (const l of st.slice) sx(`st${n}`, l);
    w.set(key, `fo${n}`, st.order);
    w.set(key, `rw${n}`, listDig(st.rowMeta));
    for (const l of st.rowMeta.slice(0, big ? SLICE : Infinity)) sx(`rw${n}`, l);
    n++;
  }
  w.set(key, 'sheets', n);
  try {
    const plan = calc.shouldRecalcOnOpen({ workbook: wb, worksheets });
    w.set(key, 'plan', plan.mode === 'partial' ? `partial:${plan.cells?.length ?? '?'}` : plan.mode);
  } catch (e) { w.set(key, 'plan', `err:${C.oneLine(e?.message ?? e)}`); }

  // ---- the element: open (open-recalc), display, save
  rng = new Rng((parseInt(sha(key).slice(0, 8), 16) >>> 0) || 1);
  RND.length = 0; ORDER.length = 0;
  const el = h.newSheet({ 'formula-locale': LANG });
  try {
    el.loadXlsx(bytes);
  } catch (e) {
    w.set(key, 'open', `err:${C.oneLine(e?.message ?? e)}`);
    el.remove();
    return out;
  }
  w.set(key, 'ord', listDig(ORDER));
  for (const l of ORDER.slice(0, big ? 2000 : Infinity)) sx('ord', l);
  w.set(key, 'rnd', listDig(RND.map(C.f64hex)));
  const eng = el.calc?.engine ?? null;
  w.set(key, 'engine', eng ? 'built' : 'none');
  if (eng) {
    w.set(key, 'circ', [...eng.circular].map((f) => `${f.sheet},${f.row},${f.col}`).sort().join(' ') || '-');
    w.set(key, 'ierr', eng.internalErrors.length ? eng.internalErrors.map((x) => `${x.sheet},${x.row},${x.col}:${C.oneLine(x.error?.message ?? x.error, 60)}`).join(' ') : '-');
    const ev = [];
    for (const f of [...eng.allFormulas()]) {
      const area = f.area && f.dynamic ? f.area : { sheet: f.sheet, r1: f.row, c1: f.col, r2: f.row, c2: f.col };
      for (let r = area.r1; r <= area.r2; r++) for (let c = area.c1; c <= area.c2; c++) ev.push(`${area.sheet ?? f.sheet},${r},${c}=${vt(eng.getValue(area.sheet ?? f.sheet, r, c))}`);
    }
    ev.sort();
    w.set(key, 'ev', listDig(ev));
    for (const l of ev.slice(0, big ? 2000 : Infinity)) sx('ev', l);
  }
  const views = el.views ?? [];
  views.forEach((v, i) => {
    const st = storeOf(v.worksheet, big ? SLICE : Infinity);
    w.set(key, `pv${i}`, st.digest);
    if (!big) for (const l of st.slice) sx(`pv${i}`, l);
  });
  const displays = (i, maxCol = Infinity, maxRow = Infinity) => {
    el.selectSheet(i);
    return el.debugCellDisplays(maxCol, maxRow).map((d) => `${d.ref}\t${d.display}`);
  };
  const active = el.activeSheet;
  for (let i = 0; i < views.length; i++) {
    const lines = big ? displays(i, 40, SLICE) : displays(i);
    w.set(key, `d${i}`, `${big ? 'slice:' : ''}${listDig(lines)}`);
    for (const l of lines) sx(`d${i}`, JSON.stringify(l));
  }
  el.selectSheet(active);
  const base = new Map(unzip(bytes).map((p) => [p.name, p.data]));
  const saveFacet = (prefix) => {
    const s = partsOf(el.saveXlsx(), base);
    w.set(key, prefix === 'save0' ? 'save0' : `${prefix}.save`, listDig(s.lines));
    w.set(key, prefix === 'save0' ? 'chg0' : `${prefix}.chg`, s.changed.join(' ') || '-');
    for (const l of s.lines) sx(prefix === 'save0' ? 'save0' : `${prefix}.save`, l);
    if (!big && PARTS) for (const p of s.parts) if (s.changed.includes(p.name) && /\.(xml|rels)$/.test(p.name) && p.data.length < 200_000) sx(`${prefix === 'save0' ? 'save0' : prefix + '.save'}:${p.name}`, JSON.stringify(p.data.toString('utf8')));
  };
  try { saveFacet('save0'); } catch (e) { w.set(key, 'save0', `err:${C.oneLine(e?.message ?? e)}`); }

  // ---- scripted edits (not on the giants: their open and save are the measured part)
  if (!big && STEPS > 0) {
    let maxC = 4, maxR = 10;
    const v0 = views[el.activeSheet];
    if (v0) { maxC = v0.maxCol ?? 4; maxR = v0.maxRow ?? 10; }
    const R = new (class { constructor(s) { this.r = new Rng(s || 1); } int(x, y) { return x + this.r.below(y - x + 1); } pick(l) { return l[this.r.below(l.length)]; } chance(p) { return this.r.below(1000) < p * 1000; } })(parseInt(sha(`${key}|edits|${seedText ?? ''}`).slice(0, 8), 16) >>> 0);
    script(R, maxC, maxR).forEach(([desc, run], k) => {
      const p = `e${k}`;
      w.set(key, p, JSON.stringify(desc));
      RND.length = 0; ORDER.length = 0;
      let res;
      try { res = run(el); res = res === undefined ? '-' : JSON.stringify(res, (kk, vv) => (vv instanceof Set ? [...vv] : vv)); } catch (e) { res = `throw:${C.oneLine(e?.message ?? e)}`; }
      w.set(key, `${p}.r`, C.oneLine(res ?? '-', 200));
      w.set(key, `${p}.ord`, listDig(ORDER));
      for (const l of ORDER) sx(`${p}.ord`, l);
      w.set(key, `${p}.rnd`, listDig(RND.map(C.f64hex)));
      try { saveFacet(p); } catch (e) { w.set(key, `${p}.save`, `err:${C.oneLine(e?.message ?? e)}`); }
      try {
        const lines = el.debugCellDisplays().map((d) => `${d.ref}\t${d.display}`);
        w.set(key, `${p}.d`, listDig(lines));
        for (const l of lines) sx(`${p}.d`, JSON.stringify(l));
      } catch (e) { w.set(key, `${p}.d`, `err:${C.oneLine(e?.message ?? e)}`); }
    });
  }
  el.remove();
  return out;
}

// ---------------------------------------------------------------- run
const w = new CaptureWriter([
  'Sheet read/save path + calc over workbooks (plan 4.3 "Sheet"): see core/crates/uo-xlsx/js/capture-sheet.mjs and core/docs/modules/uo-xlsx.md "Captures"',
]);
w.meta('ts-root-commit', h.commit);
w.meta('ts-root-state', C.tsClean(root));
w.meta('tz', process.env.TZ);
w.meta('clock', `Date frozen at ${new RealDate(FIXED).toISOString()}; Math.random = xorshift32(sha256(key)[0..8]) / 2^32, recorded`);
w.meta('formula-locale', LANG);
w.meta('canon', 'canon-json.mjs of uo-pptx: sorted keys, shortest round-trip numbers, -0 kept, Map as {"$map"}, strings over 4096 units digested');
const side = [];
const results = [];
let mode;
const t0 = performance.now();
if (a.flag('--samples')) {
  mode = 'samples';
  const all = C.trackedSamples();
  w.meta('corpus', `sheet/samples tracked files: ${all.length}`);
  for (const rel of all) {
    const key = `samples/${rel}`;
    if (only && !key.includes(only)) continue;
    const ts = performance.now();
    results.push(record(w, side, key, C.readSample(rel)));
    console.log(`  ${key}: ${((performance.now() - ts) / 1000).toFixed(1)} s`);
  }
} else if (a.opt('--gen')) {
  mode = a.opt('--gen');
  const g = GROUPS[mode];
  if (!g) throw new Error(`unknown group ${mode}`);
  w.meta(`group-${mode}`, `seeds ${g.seedBase}..${g.seedBase + g.count - 1} workbooks ${g.count}`);
  const dir = a.opt('--workbooks');
  for (let i = 0; i < g.count; i++) {
    const key = groupKey(mode, i);
    if (only && !key.includes(only)) continue;
    const d = buildWorkbook(mode, i);
    const bytes = dir ? new Uint8Array(readFileSync(join(dir, workbookFile(mode, i)))) : new Uint8Array(d.bytes);
    w.set(key, 'seed', d.seed);
    w.set(key, 'features', d.features.join(','));
    results.push(record(w, side, key, bytes, String(d.seed)));
  }
} else if (a.opt('--dir')) {
  mode = `dir-${basename(a.opt('--dir'))}`;
  for (const f of readdirSync(a.opt('--dir')).filter((x) => /\.(xlsx|xlsm|xls)$/i.test(x)).sort()) {
    const key = `dir/${f}`;
    if (only && !key.includes(only)) continue;
    results.push(record(w, side, key, new Uint8Array(readFileSync(join(a.opt('--dir'), f)))));
  }
} else {
  console.error('usage: capture-sheet.mjs (--samples | --gen <group> | --dir <folder>) [--out file] [--only s] [--steps n] [--lang pt-BR|en-US]');
  process.exit(2);
}
const out = a.opt('--out', join(C.capturesDir, `sheet-${mode}.capture`));
mkdirSync(dirname(out), { recursive: true });
w.write(out);
{
  // streamed: the sidecar of a large run is past the maximum string length
  const gz = createGzip({ level: 9 });
  const done = new Promise((res, rej) => gz.pipe(createWriteStream(out.replace(/\.capture$/, '.side.gz'))).on('finish', res).on('error', rej));
  for (let i = 0; i < side.length; i += 10000) gz.write(side.slice(i, i + 10000).join('\n') + '\n');
  gz.end();
  await done;
}
console.log(`sheet ${mode}: ${results.length} workbooks in ${((performance.now() - t0) / 1000).toFixed(1)} s -> ${out} (${results.filter((r) => r.err).length} read errors)`);
