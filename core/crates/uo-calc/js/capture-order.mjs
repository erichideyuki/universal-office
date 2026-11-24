// Parity capture group `calc-order` (plan section 4.3 and section 16 item 4: the evaluation-order facet): the frozen TS
// Engine (calc dist) over GENERATED workbooks (the seeded .xlsx of core/crates/uo-xlsx/js/lib/gen-workbook.mjs, read
// by sheet-core's dist and turned into the engine input by `fromSheetCore`), with the clock fixed and a recorded
// seeded random source (decision C1). Per run it keeps the sequence of evaluated cells (every
// Engine.evaluateRec, which is what the `Map`/`Set` order sites of uo-calc.md S7 decide), the random values consumed
// (the RAND sequence), `circular` sorted, `internalErrors`, and every cell value; then scripted incremental edits
// (setCell + calculate) and the check incremental == full recalc of the final state.
//
//   node core/crates/uo-calc/js/capture-order.mjs                          # CI: group x1 + ops -> tests/captures/calc-order.*
//   node --max-old-space-size=8192 core/crates/uo-calc/js/capture-order.mjs --gen x1-big --heavy 20000 --ops 400 \
//        --out core/target/sheet-cap/calc-order-big.capture                 # local
//   options: --edits <n> (default 12)  --only <substring>  --ts-root <dir>  --ops <n> (operator-only workbooks,
//            default 40; 0 = none)  --ops-rows <n> (rows of the operator-only grids, default 24)
//
// Group `ops` (frontier A3): CalcWorkbooks built here directly (no .xlsx) whose formulas use only operators,
// references, names, tables, external references, array constants and ANCHORARRAY — no function body (A4) and no
// legacy `@` rewrite (A5) — so every value of the Rust engine is comparable before A4/A5: chains, cycles (iterative
// and not), CSE areas, dynamic spills (blocked by values, merges and tables), shared formulas, `ca` volatiles, data
// tables, cached values with and without ignoreCache, duplicated and out-of-order cells.
//
// Records `gen/<group>/<NNNN>`, `heavy/<rows>`, `ops/<NNNN>`; facets (`count:digest` of the line lists in the sidecar
// `<out>.side.gz`, lines `<key>\t<facet>\t<line>`):
//   seed, input (`len:sha16` of the .xlsx), wb (digest of the canonical JSON of the CalcWorkbook = the engine input,
//   kept in the sidecar for a replay without the reader), cells, formulas, opts (engine options),
//   legacy (formulas whose evaluated tree got the legacy `@`, exec !== node: frontier A5)
//   r.ord  evaluation order of `recalc()` (`sheet,row,col` per evaluateRec);  r.rnd  random values (f64 hex);
//   r.circ `circular` sorted;  r.ierr internalErrors;  r.vals every stored cell `sheet,row,col=<value text>` sorted
//   c.*    the same for a `calculate()` right after (volatiles and their dependents only)
//   i<k>   edit k (`set s,r,c <input JSON>` | `del s,r,c`) then `calculate()`: i<k>.ord, i<k>.rnd, i<k>.vals
//   full   values of a fresh Engine over the final inputs + `recalc()`;  incfull  equal | differ:<n> (cells whose
//          incremental value differs from the full recalc: volatile cells and cycles may legitimately differ)
process.env.TZ = 'UTC';
const { mkdirSync, writeFileSync } = await import('node:fs');
const { join, dirname } = await import('node:path');
const { gzipSync } = await import('node:zlib');
const { pathToFileURL } = await import('node:url');
const { CaptureWriter, Rng } = await import('../../uo-testkit/js/testkit.mjs');
const { canonJson } = await import('../../uo-pptx/js/lib/canon-json.mjs');
const C = await import('./lib/common.mjs');
const { GROUPS, buildWorkbook, buildCalcHeavy, groupKey } = await import('../../uo-xlsx/js/lib/gen-workbook.mjs');

const a = C.args();
const root = C.tsRoot(a);
const { calc } = await C.loadCalc(root);
const SC = await import(pathToFileURL(join(root, 'sheet', 'packages', 'sheet-core', 'dist', 'index.js')).href);
const vt = C.makeValueText(calc);
const group = a.opt('--gen', 'x1');
const EDITS = Number(a.opt('--edits', 12));
const heavy = a.opt('--heavy') ? Number(a.opt('--heavy')) : 0;
const only = a.opt('--only');
const out = a.opt('--out', join(C.capturesDir, 'calc-order.capture'));
const OPS = Number(a.opt('--ops', 40));
const OPS_ROWS = Number(a.opt('--ops-rows', 24));

const ORDER = [];
{
  const proto = calc.Engine.prototype;
  const impl = proto.evaluateRec;
  if (typeof impl !== 'function') throw new Error('Engine.prototype.evaluateRec not found');
  proto.evaluateRec = function (f) { ORDER.push(`${f.sheet},${f.row},${f.col}`); return impl.call(this, f); };
}
const hash32 = (s) => parseInt(C.sha(s).slice(0, 8), 16) >>> 0;
const listDig = (lines) => `${lines.length}:${C.sha(lines.join('\n')).slice(0, 32)}`;

function engineOf(wb, rnd, ignoreCache = false) {
  return new calc.Engine(wb, { locale: calc.PT_BR, now: () => C.NOW_SERIAL, random: () => rnd.next(), fileName: 'Pasta1.xlsx', ...(ignoreCache ? { ignoreCache: true } : {}) });
}
function valuesOf(eng, nSheets) {
  const out = [];
  for (let s = 0; s < nSheets; s++) {
    eng.forEachCell({ sheet: s, r1: 0, c1: 0, r2: calc.MAX_ROW, c2: calc.MAX_COL }, (v, r, c) => out.push(`${s},${r},${c}=${vt(v)}`));
  }
  return out.sort();
}

const w = new CaptureWriter([
  'calc-order (plan 4.3, 16 item 4): TS Engine evaluation order + RAND sequence + values over generated workbooks, see core/crates/uo-calc/js/capture-order.mjs and core/docs/modules/uo-calc.md "Captures"',
]);
C.stamp(w, root, { group, edits: EDITS, ops: OPS, 'ops-rows': OPS_ROWS, 'random-source': 'xorshift32(sha256(key)[0..8]) / 2^32, recorded', locale: 'pt-BR', 'file-name': 'Pasta1.xlsx' });
const side = [];
const stats = { workbooks: 0, evaluated: 0, rand: 0, differ: 0, ierr: 0, circ: 0 };

const FORMS_X1 = (s, r, c) => [`A1+${r}`, 'RAND()', `SUM(A1:A${r + 1})`, `IF(A1>2,"x",1/0)`, `B${r + 1}*2`, `_xlfn.SEQUENCE(2,2)`, `INDIRECT("A"&${r + 1})`, `${String.fromCharCode(65 + (c % 26))}${r + 2}+1`]
  .map((text) => (/SEQUENCE/.test(text) ? { text, t: 'array', dynamic: true } : { text }));

function run(key, bytes, extra = {}) {
  for (const [k, v] of Object.entries(extra)) w.set(key, k, v);
  w.set(key, 'input', `${bytes.length}:${C.sha(bytes).slice(0, 16)}`);
  let wb;
  try {
    const pkg = SC.openSpreadsheet(bytes);
    const book = SC.readWorkbook(pkg);
    const cx = SC.colorContext(pkg);
    const sst = SC.readSharedStrings(pkg, cx);
    const worksheets = book.sheets.map((s) => (s.target ? SC.readWorksheet(pkg, s.target, cx) : undefined));
    wb = calc.fromSheetCore({ workbook: book, worksheets, sharedStrings: sst });
  } catch (e) {
    w.set(key, 'read', `err:${C.oneLine(e?.message ?? e)}`);
    return;
  }
  runWb(key, wb, bytes.length >= 400_000, FORMS_X1, false);
}

function runWb(key, wb, big, FORMS, ignoreCache) {
  stats.workbooks++;
  const sx = (facet, lines) => { for (const l of lines) side.push(`${key}\t${facet}\t${l}`); };
  const wbText = canonJson(wb);
  w.set(key, 'wb', C.digest(wbText));
  if (!big) side.push(`${key}\twb\t${wbText}`);
  w.set(key, 'opts', ignoreCache ? 'ignoreCache' : '-');
  const nSheets = wb.sheets.length;
  w.set(key, 'cells', wb.sheets.reduce((n, s) => n + s.cells.length, 0));
  w.set(key, 'formulas', wb.sheets.reduce((n, s) => n + s.cells.filter((c) => c[2].f).length, 0));
  const rnd = new C.RecordedRandom(hash32(key) || 1);
  const step = (p, eng, fn) => {
    ORDER.length = 0;
    rnd.take();
    let err;
    try { fn(); } catch (e) { err = `throw:${C.oneLine(e?.message ?? e)}`; }
    const ord = ORDER.slice();
    const rs = rnd.take().map(C.f64hex);
    stats.evaluated += ord.length;
    stats.rand += rs.length;
    w.set(key, `${p}.ord`, listDig(ord));
    w.set(key, `${p}.rnd`, listDig(rs));
    if (err) w.set(key, `${p}.err`, err);
    const vals = valuesOf(eng, nSheets);
    w.set(key, `${p}.vals`, listDig(vals));
    sx(`${p}.ord`, big ? ord.slice(0, 5000) : ord);
    sx(`${p}.rnd`, rs);
    if (!big) sx(`${p}.vals`, vals);
    return vals;
  };
  let eng;
  try { eng = engineOf(wb, rnd, ignoreCache); } catch (e) { w.set(key, 'engine', `err:${C.oneLine(e?.message ?? e)}`); return; }
  const legacy = eng.allFormulas().filter((f) => f.exec && f.exec !== f.node).map((f) => `${f.sheet},${f.row},${f.col}`).sort();
  w.set(key, 'legacy', legacy.join(' ') || '-');
  step('r', eng, () => eng.recalc());
  const circ = [...eng.circular].map((f) => `${f.sheet},${f.row},${f.col}`).sort();
  w.set(key, 'r.circ', circ.join(' ') || '-');
  if (circ.length) stats.circ++;
  w.set(key, 'r.ierr', eng.internalErrors.length ? eng.internalErrors.map((x) => `${x.sheet},${x.row},${x.col}:${C.oneLine(x.error?.message ?? x.error, 80)}`).join(' ') : '-');
  stats.ierr += eng.internalErrors.length;
  step('c', eng, () => eng.calculate());
  // ---- incremental edits on the used area, then calculate()
  const R = new Rng(hash32(`${key}|edits`) || 1);
  const cells = wb.sheets.map((s) => s.cells.map(([r, c]) => [r, c]));
  const final = wb.sheets.map((s) => new Map(s.cells.map(([r, c, inp]) => [`${r},${c}`, inp])));
  const TEXTS = ['1', '2.5', '-0', 'abc', 'ção', '', '1e21'];
  for (let k = 0; k < EDITS && nSheets; k++) {
    const s = R.below(nSheets);
    const pool = cells[s];
    const [r, c] = pool.length && R.below(4) ? pool[R.below(pool.length)] : [R.below(30), R.below(12)];
    const kind = R.below(10);
    let inp;
    if (kind < 4) { const t = TEXTS[R.below(TEXTS.length)]; const n = Number(t); inp = { v: t !== '' && !Number.isNaN(n) ? n : t }; }
    else if (kind < 8) { const fs = FORMS(s, r, c); inp = { f: fs[R.below(fs.length)] }; }
    else inp = undefined;
    const desc = inp === undefined ? `del ${s},${r},${c}` : `set ${s},${r},${c} ${canonJson(inp)}`;
    w.set(key, `i${k}`, JSON.stringify(desc));
    if (inp === undefined) final[s].delete(`${r},${c}`); else final[s].set(`${r},${c}`, inp);
    step(`i${k}`, eng, () => { eng.setCell(s, r, c, inp); eng.calculate(); });
  }
  const inc = valuesOf(eng, nSheets);
  // ---- full recalc of the final inputs (fresh engine, fresh random stream)
  const wb2 = { ...wb, sheets: wb.sheets.map((sh, s) => ({ ...sh, cells: [...final[s]].map(([k, inp]) => { const [r, c] = k.split(',').map(Number); return [r, c, inp]; }) })) };
  const rnd2 = new C.RecordedRandom((hash32(`${key}|full`) || 1));
  let full;
  try { const e2 = engineOf(wb2, rnd2, ignoreCache); e2.recalc(); full = valuesOf(e2, nSheets); } catch (e) { w.set(key, 'full', `err:${C.oneLine(e?.message ?? e)}`); return; }
  w.set(key, 'full', listDig(full));
  if (!big) sx('full.vals', full);
  const fm = new Map(full.map((l) => [l.slice(0, l.indexOf('=')), l]));
  const im = new Set(inc.map((l) => l.slice(0, l.indexOf('='))));
  const diff = inc.filter((l) => fm.get(l.slice(0, l.indexOf('='))) !== l).length + full.filter((l) => !im.has(l.slice(0, l.indexOf('=')))).length;
  w.set(key, 'incfull', diff ? `differ:${diff}` : 'equal');
  if (diff) stats.differ++;
}

// ---- group ops: operator-only CalcWorkbooks (no function body, no legacy @)
function buildOps(i) {
  const R = new Rng((0xa3 * 7919 + i * 104729 + 1) >>> 0);
  const pick = (xs) => xs[R.below(xs.length)];
  const col = (c) => String.fromCharCode(65 + c);
  const nS = 1 + R.below(3);
  const sheetNames = ['Plan1', 'Dados', 'Résumé'].slice(0, nS);
  const H = 6 + R.below(OPS_ROWS), W = 4 + R.below(5);
  const E = (code) => calc.CalcError.of(code);
  const VALUES = [1, 2, 3, -1, 0, 2.5, 0.1, 1e21, -0, 'abc', 'ção', '1,5', '', 'TRUE', true, false, E('#N/A'), E('#DIV/0!'), 7, 100];
  const ref = (rr, cc, sheet) => `${sheet !== undefined ? `${sheet}!` : ''}${R.below(5) === 0 ? '$' : ''}${col(cc)}${R.below(5) === 0 ? '$' : ''}${rr + 1}`;
  const anyRef = (si) => (nS > 1 && R.below(6) === 0 ? ref(R.below(H), R.below(W), `'${sheetNames[(si + 1) % nS]}'`) : ref(R.below(H), R.below(W)));
  const area = (r1, c1, h, w) => `${col(c1)}${r1 + 1}:${col(c1 + w - 1)}${r1 + h}`;
  const names = [
    { name: 'Taxa', formula: 'Plan1!$A$1' },
    { name: 'Meio', formula: '0.5' },
    { name: 'Faixa', formula: 'Plan1!$A$1:$A$4' },
    { name: 'Local', formula: 'Plan1!$B$2', sheet: 0 },
    { name: 'Quebrado', formula: '1+' },
  ];
  const scalarForm = (si) => {
    const x = anyRef(si), y = anyRef(si), n = pick([1, 2, 0.5, 3, 10]);
    return pick([`${x}+${y}`, `${x}*${n}-${y}`, `-${x}%`, `${x}&"x"`, `${x}=${y}`, `(${x}+1)^2`, `${x}/${y}`, `"a"&${x}`, `${x}<>${y}`, `TRUE+${x}`,
      `Taxa+${x}`, `Meio*${x}`, `Local&${x}`, `${x}`, `${x}>=${y}`, '1/0', '#N/A', `${x}+[1]Ext!A1`, 'Quebrado', `${x}+${y}+${anyRef(si)}`, `--${x}`, `${x}^${n}`]);
  };
  const arrayForm = (si, h, w) => {
    const r1 = R.below(H), c1 = R.below(W);
    return pick([`${area(r1, c1, h, w)}*2`, `${area(r1, c1, h, w)}+${area(R.below(H), R.below(W), h, w)}`, '{1,2;3,4}', `${area(0, 0, 3, 3)} ${area(0, 1, 5, 1)}`,
      'Faixa*1', `-(${area(r1, c1, h, w)})`, `${area(r1, c1, h, w)}&"-"`, `${area(r1, c1, h, w)}=${anyRef(si)}`, 'Tabela1[Valor]*1', '{"a";TRUE;#N/A}']);
  };
  const sheets = sheetNames.map((name, si) => {
    const cells = [];
    const merges = [];
    let shared = 0;
    for (let r = 0; r < H; r++) {
      for (let c = 0; c < W; c++) {
        const k = R.below(20);
        if (c < 2 || k < 7) {
          const v = pick(VALUES);
          if (v === '' && R.below(2)) continue;
          cells.push([r, c, { v }]);
        } else if (k < 15) {
          const f = { text: scalarForm(si) };
          if (R.below(12) === 0) f.ca = true;
          cells.push([r, c, { f, v: R.below(4) === 0 ? pick([5, 'old', true, E('#REF!')]) : null }]);
        } else if (k < 16 && r + 3 < H) {
          const si2 = shared++;
          const n = 2 + R.below(3);
          cells.push([r, c, { f: { t: 'shared', si: si2, ref: `${col(c)}${r + 1}:${col(c)}${r + n}`, text: scalarForm(si) }, v: null }]);
          for (let j = 1; j < n; j++) cells.push([r + j, c, { f: { t: 'shared', si: si2, text: '' }, v: null }]);
        } else if (k < 17) {
          const h = 1 + R.below(3), w = 1 + R.below(2);
          cells.push([r, c, { f: { t: 'array', ref: area(r, c, h, w), text: arrayForm(si, h, w) }, v: null }]);
        } else if (k < 19) {
          const h = 1 + R.below(4), w = 1 + R.below(2);
          const text = R.below(5) === 0 ? `_xlfn.ANCHORARRAY(${col(R.below(W))}${R.below(H) + 1})*1` : arrayForm(si, h, w);
          cells.push([r, c, { f: { t: 'array', dynamic: true, ref: `${col(c)}${r + 1}`, text }, v: null }]);
        } else {
          cells.push([r, c, { f: { t: 'dataTable', ref: area(r, c, 2, 1), text: 'A1' }, v: R.below(2) ? 9 : null }]);
        }
      }
    }
    if (R.below(3) === 0) merges.push(area(R.below(H), 2 + R.below(W - 2), 2, 1));
    // a few cells out of order and one duplicate (real files have both)
    for (let j = 0; j < 3 && cells.length > 2; j++) { const x = R.below(cells.length), y = R.below(cells.length); [cells[x], cells[y]] = [cells[y], cells[x]]; }
    if (R.below(4) === 0 && cells.length) cells.push(cells[R.below(cells.length)]);
    return { name, cells, merges };
  });
  const tables = R.below(2) ? [{ name: 'Tabela1', sheet: 0, ref: `${col(W)}1:${col(W + 1)}4`, headerRowCount: 1, totalsRowCount: 0, columns: ['Id', 'Valor'] }] : [];
  if (tables.length) for (let r = 0; r < 4; r++) sheets[0].cells.push([r, W, { v: r ? r : 'Id' }], [r, W + 1, { v: r ? r * 10 : 'Valor' }]);
  const externals = [{ sheets: [{ name: 'Ext', cells: [[0, 0, 42], [1, 0, 'ext']] }] }];
  const calcOpts = R.below(3) === 0 ? { iterate: true, iterateCount: pick([1, 5, 100]), iterateDelta: pick([0.001, 0.1]) } : {};
  const wb = { sheets, names, tables, externals, calc: calcOpts };
  const forms = (s, r, c) => [{ text: scalarForm(s) }, { text: scalarForm(s) }, { text: `${col(c)}${r + 2}+1` }, { text: arrayForm(s, 2, 2), t: 'array', dynamic: true },
    { text: arrayForm(s, 2, 1), t: 'array', ref: area(r, c, 2, 1) }, { text: `${anyRef(s)}*2`, ca: true }];
  return { wb, forms, ignoreCache: i % 5 === 4 };
}

const t0 = performance.now();
const g = GROUPS[group];
if (!g) throw new Error(`unknown group ${group}`);
w.meta(`group-${group}`, `seeds ${g.seedBase}..${g.seedBase + g.count - 1} workbooks ${g.count}`);
for (let i = 0; i < g.count; i++) {
  const key = groupKey(group, i);
  if (only && !key.includes(only)) continue;
  const d = buildWorkbook(group, i);
  run(key, new Uint8Array(d.bytes), { seed: d.seed });
}
if (heavy) run(`heavy/${heavy}`, new Uint8Array(buildCalcHeavy(heavy)), { seed: 'calc-heavy' });
for (let i = 0; i < OPS; i++) {
  const key = `ops/${String(i).padStart(4, '0')}`;
  if (only && !key.includes(only)) continue;
  const { wb, forms, ignoreCache } = buildOps(i);
  w.set(key, 'seed', `ops-${i}`);
  runWb(key, wb, false, forms, ignoreCache);
}
w.meta('stats', Object.entries(stats).map(([k, v]) => `${k}=${v}`).join(' '));
mkdirSync(dirname(out), { recursive: true });
w.write(out);
// line by line: a big run's sidecar is longer than V8's longest string
writeFileSync(out.replace(/\.capture$/, '.side.gz'), gzipSync(Buffer.concat(side.map((l) => Buffer.from(`${l}\n`, 'utf8'))), { level: 9 }));
console.log(`calc-order ${group}${heavy ? ` + heavy ${heavy}` : ''} + ops ${OPS}: ${Object.entries(stats).map(([k, v]) => `${k} ${v}`).join(', ')} in ${((performance.now() - t0) / 1000).toFixed(1)} s -> ${out}`);
