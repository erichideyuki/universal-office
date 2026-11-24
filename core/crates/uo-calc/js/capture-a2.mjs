// Parity capture group `a2` (frontier A2, record core/docs/modules/uo-calc.md "A2 — as built"): the parser, the
// printer, the A1 helpers, the operators, `applyFn` and the evaluator of the frozen TS, called directly on the calc
// dist (clock frozen at 2025-01-15 12:00 UTC).
//
//   node core/crates/uo-calc/js/capture-a2.mjs                    # CI -> tests/captures/a2.capture.gz
//   node core/crates/uo-calc/js/capture-a2.mjs --big --out core/target/sheet-cap/a2-big.capture.gz   # 10x generated
//
// Formulas: the oracle bank's (`f` and the `{f}` cells), the generated workbooks' (calc-order sidecar `wb`), a
// seeded grammar over every syntax of parse.ts (A1/R1C1 references, sheets quoted/3D/external, structured
// references, names, _xlpm/_xlop/_xleta, prefixes, arrays, operators incl. TRIMRANGE, white space kinds) and
// seeded mutations of all of them (syntax errors, hostile text).
//
// Records (facets):
//   p#<i>   in = {"f": text, "o": ParseOptions}; out = canon(AST) (the calc-tests recorder's canonical JSON) or
//           `throw:<Class>:<pos>`; pr0..pr3 = printFormula at the options' cell (file A1 | ui A1 | file R1C1 | ui R1C1)
//           as JSON strings; pr4/pr5 = file/ui A1 at another cell; rt = canon(parse(pr0, same cell)) or throw
//   e#<i>   in = {"f", "at": [sheet,row,col], "arr": bool, "loc": "en"|"pt"}; out = value text (`A4` = a function body
//           was called: the TS def.call is replaced by a marker; `range` = RangeError; `throw:<Class>`)
//   o#<i>   ops.ts calls: in = [fn, args...] (value texts), out = value text
//   a#<i>   a1.ts calls; lv#<i> numberLiteralValue; ap#<i> applyFn over synthetic defs (log of the body's calls)
// The mock EvalHost of the `e` records is described in @meta mock (the Rust replay builds the same).
process.env.TZ = 'UTC';
const { join, dirname } = await import('node:path');
const { mkdirSync, writeFileSync, readFileSync, readdirSync } = await import('node:fs');
const { gzipSync, gunzipSync } = await import('node:zlib');
const { pathToFileURL } = await import('node:url');
const { CaptureWriter, Rng } = await import('../../uo-testkit/js/testkit.mjs');
const C = await import('./lib/common.mjs');

const a = C.args();
const root = C.tsRoot(a);
const big = a.flag('--big');
const out = a.opt('--out', join(C.capturesDir, 'a2.capture.gz'));
const SCALE = big ? 10 : 1;

const FIXED = Date.UTC(2025, 0, 15, 12, 0, 0);
const RealDate = Date;
globalThis.Date = class extends RealDate {
  constructor(...x) { if (x.length === 0) super(FIXED); else super(...x); }
  static now() { return FIXED; }
};

const base = join(root, 'sheet', 'packages', 'calc', 'dist');
const imp = (p) => import(pathToFileURL(join(base, p)).href);
const calc = await imp('index.js');
const P = await imp('parse.js');
const PR = await imp('print.js');
const A1 = await imp('a1.js');
const OPS = await imp('ops.js');
const AP = await imp('apply.js');
const EV = await imp('evaluate.js');
const FN = await imp('fn.js');
const T = await imp('testing.js');
const { CalcError, ArrayValue, RefValue, LambdaValue } = calc;
const vt0 = C.makeValueText(calc);
const vt = (v) => {
  if (v instanceof LambdaValue) return `L${JSON.stringify([...v.params])}${typeof v.body === 'function' ? '*' : ''}`;
  return v === undefined ? 'u' : vt0(v);
};

// ---------------------------------------------------------------- the recorder's canonical JSON (calc-tests)
function canon(v) {
  if (v === null) return null;
  switch (typeof v) {
    case 'number':
      if (Object.is(v, -0)) return { $num: '-0' };
      if (!Number.isFinite(v)) return { $num: String(v) };
      return v;
    case 'string': case 'boolean': return v;
    case 'undefined': return { $undef: true };
    default: break;
  }
  if (v instanceof CalcError) return { $err: v.code };
  if (Array.isArray(v)) return v.map(canon);
  const o = {};
  for (const k of Object.keys(v).sort()) o[k] = canon(v[k]);
  return o;
}
const cj = (v) => JSON.stringify(canon(v));

const w = new CaptureWriter([
  'a2 (frontier A2): parse/print/a1/ops/applyFn/evaluate of the frozen TS over bank, generated and seeded formulas, see core/crates/uo-calc/js/capture-a2.mjs',
]);
const counts = new Map();
let total = 0;
function key(name) {
  const i = counts.get(name) ?? 0;
  counts.set(name, i + 1);
  total++;
  return `${name}#${i}`;
}
const rng = new Rng(0xa2a2_0002);
const pick = (xs) => xs[rng.below(xs.length)];
const errText = (e) => `throw:${e?.constructor?.name ?? 'Error'}:${e?.pos ?? ''}`;

// ---------------------------------------------------------------- formulas
const formulas = [];
const seen = new Set();
const addF = (f, at) => { if (typeof f !== 'string') return; const k = f; if (seen.has(k)) return; seen.add(k); formulas.push({ f, at }); };

// bank
const oracleDir = C.oracleDir;
for (const file of readdirSync(oracleDir).filter((x) => x.endsWith('.cases.json')).sort()) {
  const j = JSON.parse(readFileSync(join(oracleDir, file), 'utf8'));
  for (const c of Array.isArray(j) ? j : j.cases) {
    if (c.f) addF(c.f.replace(/^=/, ''));
    for (const [k, v] of Object.entries(c.cells ?? {})) if (v && typeof v === 'object' && 'f' in v) addF(String(v.f).replace(/^=/, ''), k);
  }
}
const bankCount = formulas.length;
// generated workbooks (calc-order sidecar)
try {
  const side = gunzipSync(readFileSync(join(C.capturesDir, 'calc-order.side.gz'))).toString('utf8');
  for (const line of side.split('\n')) {
    const p = line.split('\t');
    if (p[1] !== 'wb') continue;
    const wb = JSON.parse(p.slice(2).join('\t'));
    for (const n of wb.names ?? []) addF(n.formula ?? n.text ?? n.f);
    for (const sh of wb.sheets) for (const [r, c, cell] of sh.cells) if (cell.f?.text) addF(cell.f.text, [r, c]);
  }
} catch (e) { console.warn('calc-order sidecar not read:', e.message); }
const genCount = formulas.length - bankCount;

// seeded grammar
const SHEETS = ['Sheet1', 'Data', "'My Sheet'", 'Sheet1:Data', "'Sheet1:My Sheet'", '[1]Ext', "'[1]Ext'", '[0]Sheet1', 'Nope', "'It''s'", 'sheet1', '!'];
const A1REFS = ['A1', '$B$2', 'C$3', '$D4', 'XFD1048576', 'XFE1', 'A0', 'A1048577', 'A:C', '$A:$B', 'b:b', '1:3', '$2:$5', '1:1048577', 'A1:B3',
  'B2:A1', 'F20', 'E5:F6', 'AA10', 'zz9', 'A1:A20', 'H1:J6', 'L1', 'M1', 'A1#', 'L1#', 'A1:.A10', 'A1.:A10', 'A1.:.A10', 'A:.C', 'A.:C', '1:.3',
  'Tbl[Val]', 'Tbl[[#Headers],[Id]:[Name]]', 'Tbl[@Val]', '[@Name]', 'Tbl[#All]', 'Tbl[]', 'Tbl[[#This Row],[Val]]', 'Tbl[[#Data],[#Totals],[Val]]',
  'tbl[#totals]', "Tbl[Na'[me]", 'Tbl[[Id]:[Val]]', 'Tbl[ Val ]', 'Tbl[#Headers]', 'Data2[B]', 'Data2[[#All],[A]]', 'Tbl[@[Name]]', 'Tbl[@]', 'Tbl[[#ALL]]',
  'Tbl[Nope]', 'Nope[x]', 'Tbl[[#Headers],[#Data]]', 'Tbl[[#Headers],[#Totals]]', 'A1 B2', 'A1:C3 B2:D4', '(A1,B2)', '(A1:A3,C1)', 'A1:B2:C3', 'R1C1'];
const R1C1REFS = ['R1C1', 'R[-1]C[2]', 'RC', 'R2', 'C3', 'R1:R3', 'C[1]:C[2]', 'r[1]c', 'R1C', 'RC1', 'R0C1', 'R[-0]C', 'R1048576C16384', 'R1048577C1',
  'L1C1', 'L[1]C[-1]', 'LC', 'l2:l4', 'R[99999999999]C', 'R2:R', 'C:C[1]', 'R1C1:R2C2', 'R[1]C[1]A'];
const NAMES = ['Rate', 'rng', 'Twice', 'Self', 'Txt', 'Rel', 'Err', 'Local', 'Data!Local', 'Sheet1!Local', 'Unknown', '_xlpm.x', '_xleta.SUM', '_xleta.FOO',
  'ação', 'x.y', 'Tbl', 'data2', '_xlop.b', 'TRUE', 'false', 'Twice(4)', '[1]!Rate', '[0]!Rate', '!Rate', 'Nope!Rate', 'ArrN', 'Sum', '_xlfn.SINGLE(Rng)'];
const NUMS = ['0', '1', '42', '3.5', '.5', '5.', '1e5', '1E+300', '1e-300', '1e-400', '1e400', '12345678901234567890', '0.1234567890123456789', '00012',
  '1.000000000000000000001', '9007199254740993', '2.2250738585072014e-308', '1E-310', '1e0', '1e', '1e+', '0.000', '123456789012345.6', '4294967296'];
const STRS = ['"a"', '""', '"a""b"', '"10"', '"1,5"', '"1.5"', '"TRUE"', '"ação"', '"\ud800x"', '"😀"', '"ß"', '" 3 "', '"1/2/2024"', '"12:30"', '"R$ 1.234,56"', '"#N/A"'];
const LITS = ['TRUE', 'false', 'True', '#N/A', '#div/0!', '#VALUE!', '#REF!', '#NAME?', '#NUM!', '#NULL!', '#ſpill!', '#CALC!', '#GETTING_DATA', '#BUSY!',
  '{1,2;3,4}', '{-1,"a";TRUE,#N/A}', '{--1}', '{1;2,3}', '{+"a"}', '{"x"}', '{1,,2}', '{1e5,-0,.5}', '{true;FALSE}', '{1;2;3}', '{-"a"}', '{A1}'];
const FUNCS = ['FOO(1)', '_xludf.MYFN(1)', '_xll.F(2)', 'SUM(1,2)', '_xlfn.LET(x,1,x+1)', 'LET(x,2,y,x*3,x+y)', 'LAMBDA(x,x+1)(2)', 'LAMBDA(x,y,x&y)("a","b")',
  'LAMBDA(x,[y],ISOMITTED(y))(1)', '_xlfn.LAMBDA(_xlpm.x,_xlpm.x*2)(3)', 'LET(f,LAMBDA(n,n+1),f(1))', 'ANCHORARRAY(L1)', '_xlfn.ANCHORARRAY(M1)', 'ANCHORARRAY(A1:A2)',
  '_xlfn.SINGLE(A1:A3)', '@A1:A3', '@B1:B20', '_xlfn._TRO_ALL(A1:A10)', '_xlfn._TRO_TRAILING(A1:C20)', '_TRO_LEADING(B1:E20)', 'LET(x,,1)', 'LET(x,1)', 'LET(1,2,3)',
  'LAMBDA()', 'LAMBDA(x)', 'LAMBDA(x,x)(1,2)', 'LAMBDA(x,y,x)(1)', 'LAMBDA(x,y,x)(1,)', 'LET(f,LAMBDA(x,x*2),f)', 'LAMBDA(x,x)', 'Twice(5)', 'Twice', 'Self',
  'ISOMITTED(A1)', 'LET(g,_xleta.SUM,g(1))', 'IF(A1,1,2)', '_xlfn._xlws.SORT(A1:A3)', 'Sheet1!Twice(3)', 'Data!FOO(1)', '_xlpm.f(1)', 'NA()', 'Rate(1)',
  'LET(x,Tbl[Val],x)', 'LET(x,{1,2},x+1)', 'LAMBDA(x,LAMBDA(y,x+y))(1)(2)', '(LAMBDA(x,x))(7)', 'LET(_xlpm.a,3,_xlpm.a)', 'Rng', 'ArrN+1', 'Rel+1', 'Err', 'Txt&"!"'];
const BINOPS = ['+', '-', '*', '/', '^', '&', '=', '<>', '<', '<=', '>', '>=', ':', ' ', ',', ' + ', '\n*', ' -', '\t&'];
const UN = ['-', '+', '--', '@', ''];
const POST = ['%', '%%', '#', ''];

function atom(depth) {
  switch (rng.below(depth > 2 ? 6 : 9)) {
    case 0: return pick(A1REFS);
    case 1: return `${pick(SHEETS)}!${pick(A1REFS.slice(0, 22))}`;
    case 2: return pick(NUMS);
    case 3: return pick(STRS);
    case 4: return pick(LITS);
    case 5: return pick(NAMES);
    case 6: return pick(FUNCS);
    case 7: return `(${expr(depth + 1)})`;
    default: {
      const n = rng.below(4);
      const args = Array.from({ length: n }, () => (rng.below(6) ? expr(depth + 1) : ''));
      return `${pick(['FOO', 'IF', 'SUM', 'LET', 'LAMBDA', '_xlfn.LET', 'Twice', 'CHOOSE', 'INDEX', 'ISOMITTED', 'ANCHORARRAY', '_xlfn._TRO_ALL', 'Self', 'X'])}(${args.join(pick([',', ', ', ' ,']))})`;
    }
  }
}
function expr(depth = 0) {
  let e = `${pick(UN)}${atom(depth)}${pick(POST)}`;
  const n = depth > 2 ? 0 : rng.below(3);
  for (let i = 0; i < n; i++) e += `${pick(BINOPS)}${pick(UN)}${atom(depth)}${pick(POST)}`;
  return e;
}
for (const x of [...A1REFS, ...NUMS, ...STRS, ...LITS, ...NAMES, ...FUNCS]) addF(x);
for (const r of R1C1REFS) addF(r);
for (let i = 0; i < 3000 * SCALE; i++) addF(`${pick(['', '', '=', ' =', '= '])}${expr()}`);
// mutations of everything so far (delete/insert/duplicate a unit, swap two)
const ALPH = ['(', ')', '"', "'", '[', ']', '{', '}', ',', ';', ':', '!', '#', '@', '$', '.', ' ', '-', 'A', '1', 'R', 'C', ' ', '　', '\ud83d', 'é', 'e', '%', '='];
const pool = formulas.map((x) => x.f);
for (let i = 0; i < 3000 * SCALE; i++) {
  let f = pick(pool);
  const k = 1 + rng.below(3);
  for (let j = 0; j < k; j++) {
    const p = rng.below(f.length + 1);
    switch (rng.below(4)) {
      case 0: f = f.slice(0, p) + f.slice(p + 1); break;
      case 1: f = f.slice(0, p) + pick(ALPH) + f.slice(p); break;
      case 2: f = f.slice(0, p) + f.slice(p, p + 3) + f.slice(p); break;
      default: { const q = rng.below(f.length + 1); const [x, y] = [Math.min(p, q), Math.max(p, q)]; f = f.slice(0, x) + f.slice(y, y + 1) + f.slice(x + 1, y) + f.slice(x, x + 1) + f.slice(y + 1); }
    }
  }
  addF(f);
}
// hostile shapes (bounded in the TS too)
for (const n of [10, 100, 500]) {
  addF('('.repeat(n) + '1' + ')'.repeat(n));
  addF('1' + '+1'.repeat(n));
  addF('-'.repeat(n) + '1');
  addF('F('.repeat(Math.min(n, 300)) + '1' + ')'.repeat(Math.min(n, 300)));
  addF('LET(x,1,'.repeat(Math.min(n, 200)) + 'x' + ')'.repeat(Math.min(n, 200)));
}
addF('9'.repeat(400));
addF('1e' + '9'.repeat(30));
addF('"' + 'a'.repeat(40000) + '"');
addF('A1&"' + 'b'.repeat(32766) + '"');
addF('(' + 'A1,'.repeat(50) + 'A1)');

const OPTS = [{}, { row: 5, col: 3 }, { row: 1048575, col: 16383 }, { r1c1: true }, { r1c1: true, r1c1Letters: { row: 'L', col: 'C' } }, { r1c1: true, row: 2, col: 2 },
  { r1c1: true, r1c1Letters: { row: 'r', col: 'c' } }, { r1c1: true, r1c1Letters: { row: 'Z', col: 'K' } }];

// ---------------------------------------------------------------- parse / print / round trip
const parseOk = [];
for (const { f, at } of formulas) {
  const opts = new Set([0, rng.below(OPTS.length)]);
  if (Array.isArray(at)) opts.add(-1);
  for (const oi of opts) {
    const o = oi === -1 ? { row: at[0], col: at[1] } : OPTS[oi];
    const k = key('p');
    w.set(k, 'in', JSON.stringify({ f, o }));
    let ast;
    try { ast = P.parseFormula(f, o); } catch (e) { w.set(k, 'out', errText(e)); continue; }
    w.set(k, 'out', C.clip(cj(ast), 4096));
    const row = o.row ?? 0, col = o.col ?? 0;
    const pr = (po) => { try { return C.clip(JSON.stringify(PR.printFormula(ast, po)), 4096); } catch (e) { return errText(e); } };
    w.set(k, 'pr0', pr({ row, col }));
    w.set(k, 'pr1', pr({ row, col, ui: true }));
    w.set(k, 'pr2', pr({ r1c1: true }));
    w.set(k, 'pr3', pr({ r1c1: true, ui: true }));
    w.set(k, 'pr4', pr({ row: row + 7, col: col + 2 }));
    w.set(k, 'pr5', pr({ row: 3, col: 1, ui: true }));
    try {
      const t = PR.printFormula(ast, { row, col });
      try { w.set(k, 'rt', C.clip(cj(P.parseFormula(t, { row, col })), 4096)); } catch (e) { w.set(k, 'rt', errText(e)); }
    } catch (e) { w.set(k, 'rt', errText(e)); }
    if (!o.r1c1) parseOk.push(f);
  }
}

// ---------------------------------------------------------------- the mock host of the `e` records
const MOCK = {
  sheets: ['Sheet1', 'Data', 'My Sheet', 'Sheet4'],
  ext: { book: 1, sheet: 'Ext', index: 1 << 20 },
  cells: [],
  names: [
    [-1, 'Rate', '0.1'], [-1, 'Rng', 'Sheet1!$A$1:$A$5'], [-1, 'Twice', '_xlfn.LAMBDA(_xlpm.x,_xlpm.x*2)'], [-1, 'Self', 'Self+1'], [-1, 'Txt', '"hello"'],
    [-1, 'Rel', 'A1'], [-1, 'Err', '#DIV/0!'], [1, 'Local', '42'], [0, 'Local', '7'], [-1, 'ArrN', '{1,2;3,4}'], [-1, 'Broken', null], [-1, 'Sum', '3'],
    [-1, 'Lam2', '_xlfn.LAMBDA(_xlpm.a,_xlpm.b,_xlpm.a&_xlpm.b)'],
  ],
  tables: [
    { name: 'Tbl', sheet: 0, area: [0, 7, 5, 9], headerRows: 1, totalsRows: 1, columns: ['Id', 'Name', 'Val'] },
    { name: 'Data2', sheet: 1, area: [0, 0, 3, 1], headerRows: 1, totalsRows: 0, columns: ['A', 'B'] },
  ],
  spills: [[0, 0, 11, [0, 0, 11, 2, 11]]],
  formulaCells: [[0, 0, 12]],
};
const VALS = [1, 2.5, -3, 0, 10, '10', 'abc', '', 'ação', '1,5', '1.5', 'TRUE', true, false, null, '#N/A', 1e300, 0.1, 'x\ud800', '😀', 45658, '01/02/2025', '3%'];
const cellMap = new Map();
for (let s = 0; s < 3; s++) {
  for (let r = 0; r < 20; r++) for (let c = 0; c < 6; c++) {
    if ((r * 7 + c * 3 + s) % 5 === 4) continue;
    let v = VALS[(r * 6 + c + s * 11) % VALS.length];
    if (v === '#N/A' && (r + c) % 2) v = { e: '#DIV/0!' };
    else if (v === '#N/A') v = { e: '#N/A' };
    MOCK.cells.push([s, r, c, v]);
  }
}
// the table body (Sheet1!H1:J6): header texts, data, totals
for (let c = 0; c < 3; c++) MOCK.cells.push([0, 0, 7 + c, ['Id', 'Name', 'Val'][c]]);
for (let r = 1; r < 5; r++) { MOCK.cells.push([0, r, 7, r]); MOCK.cells.push([0, r, 8, `n${r}`]); MOCK.cells.push([0, r, 9, r * 1.5]); }
MOCK.cells.push([0, 5, 9, 15]);
MOCK.cells.push([0, 0, 11, 1], [0, 1, 11, 2], [0, 2, 11, 3], [0, 0, 12, 9]);
MOCK.cells.push([MOCK.ext.index, 0, 0, 'ext'], [MOCK.ext.index, 1, 0, 5]);
for (const [s, r, c, v] of MOCK.cells) cellMap.set(`${s},${r},${c}`, v && typeof v === 'object' ? CalcError.of(v.e) : v);
const maxRow = new Map(), maxCol = new Map();
for (const [s, r, c] of MOCK.cells) { maxRow.set(s, Math.max(maxRow.get(s) ?? 0, r)); maxCol.set(s, Math.max(maxCol.get(s) ?? 0, c)); }

const nameNodes = new Map();
for (const [scope, name, text] of MOCK.names) {
  let node = null;
  if (text !== null) node = P.parseFormula(text);
  nameNodes.set(`${scope}|${name.toUpperCase()}`, node);
}
let bodyHit = false;
class BodyHit extends Error {}
for (const d of FN.allFns()) { d.call = () => { bodyHit = true; throw new BodyHit('A4'); }; }

function mockHost(locale) {
  return {
    locale, date1904: false,
    sheetCount: () => MOCK.sheets.length,
    sheetName: (i) => MOCK.sheets[i] ?? '',
    sheetIndex: (n) => { const i = MOCK.sheets.findIndex((x) => x.toUpperCase() === n.toUpperCase()); return i < 0 ? undefined : i; },
    externalSheet: (ext, name) => (ext === MOCK.ext.book && name.toUpperCase() === MOCK.ext.sheet.toUpperCase() ? MOCK.ext.index : undefined),
    cell: (s, r, c) => cellMap.get(`${s},${r},${c}`) ?? null,
    values(a) {
      let { r2, c2 } = a;
      const rows0 = a.r2 - a.r1 + 1, cols0 = a.c2 - a.c1 + 1;
      // mock rule (not the engine's): a big area is clipped to the sheet's used rows and columns
      if (rows0 * cols0 > 1 << 16) { r2 = Math.max(a.r1, Math.min(a.r2, maxRow.get(a.sheet) ?? 0)); c2 = Math.max(a.c1, Math.min(a.c2, maxCol.get(a.sheet) ?? 0)); }
      const rows = r2 - a.r1 + 1, cols = c2 - a.c1 + 1;
      return ArrayValue.fill(rows, cols, (r, c) => cellMap.get(`${a.sheet},${a.r1 + r},${a.c1 + c}`) ?? null);
    },
    forEachCell(a, fn) {
      const hits = [];
      for (const [s, r, c] of MOCK.cells) if (s === a.sheet && r >= a.r1 && r <= a.r2 && c >= a.c1 && c <= a.c2) hits.push([r, c]);
      hits.sort((x, y) => x[0] - y[0] || x[1] - y[1]);
      for (const [r, c] of hits) fn(cellMap.get(`${a.sheet},${r},${c}`), r, c);
    },
    formulaText: () => undefined,
    isSubtotalCell: () => false, rowHidden: () => false, rowFiltered: () => false,
    now: () => C.NOW_SERIAL, random: () => 0.5,
    definedName(name, sheet) {
      const u = name.toUpperCase();
      const local = nameNodes.get(`${sheet}|${u}`);
      if (local !== undefined) return local ?? { t: 'err', v: '#NAME?' };
      const global = nameNodes.get(`-1|${u}`);
      if (global !== undefined) return global ?? { t: 'err', v: '#NAME?' };
      if (MOCK.tables.some((t) => t.name.toUpperCase() === u)) return { t: 'table', table: name, items: [] };
      return undefined;
    },
    table: (name) => { const t = MOCK.tables.find((x) => x.name.toUpperCase() === name.toUpperCase()); return t && tdef(t); },
    tableAt: (s, r, c) => { const t = MOCK.tables.find((x) => x.sheet === s && r >= x.area[0] && r <= x.area[2] && c >= x.area[1] && c <= x.area[3]); return t && tdef(t); },
    spillArea(s, r, c) {
      const sp = MOCK.spills.find((x) => x[0] === s && x[1] === r && x[2] === c);
      if (sp) { const [sh, r1, c1, r2, c2] = sp[3]; return { sheet: sh, r1, c1, r2, c2 }; }
      if (MOCK.formulaCells.some((x) => x[0] === s && x[1] === r && x[2] === c)) return { sheet: s, r1: r, c1: c, r2: r, c2: c };
      return undefined;
    },
  };
}
const tdef = (t) => ({ name: t.name, sheet: t.sheet, area: { sheet: t.sheet, r1: t.area[0], c1: t.area[1], r2: t.area[2], c2: t.area[3] }, headerRows: t.headerRows, totalsRows: t.totalsRows, columns: t.columns });
const HOSTS = { en: mockHost(FN.EN_US), pt: mockHost(FN.PT_BR) };
const EVS = { en: new EV.Evaluator(HOSTS.en), pt: new EV.Evaluator(HOSTS.pt) };
w.meta('mock', JSON.stringify(MOCK));

// ---------------------------------------------------------------- evaluate
const ATS = [[0, 999, 25], [0, 2, 1], [1, 1, 2], [0, 2, 8], [0, 7, 0], [2, 0, 0]];
for (const f of parseOk) {
  const n = big ? 3 : 1 + rng.below(2);
  for (let i = 0; i < n; i++) {
    const at = pick(ATS), arr = rng.below(2) === 0, loc = pick(['en', 'pt']);
    const k = key('e');
    w.set(k, 'in', JSON.stringify({ f, at, arr, loc }));
    let r;
    bodyHit = false;
    try {
      const node = P.parseFormula(f, { row: at[1], col: at[2] });
      const ev = EVS[loc];
      r = vt(ev.evalFormula(node, ev.context(at[0], at[1], at[2], arr)));
    } catch (e) {
      r = e instanceof BodyHit || bodyHit ? 'A4' : e instanceof RangeError ? 'range' : `throw:${e?.constructor?.name}`;
    }
    w.set(k, 'out', C.clip(r, 4096));
  }
}

// ---------------------------------------------------------------- ops
const TRAPV = [null, 0, -0, 1, -1, 0.1, 0.2, 0.3, 1e21, 1e-7, 2 ** 53 + 2, 5e-324, 2.2250738585072014e-308, 1.7976931348623157e308, Infinity, NaN, 1e308, 10, 3, -8, 0.5,
  '', 'a', 'A', 'abc', 'ABC', '10', '1,5', '1.5', ' 3 ', 'TRUE', 'ação', 'acao', '\ud800', '😀', '1/2/2025', '50%', true, false,
  CalcError.of('#N/A'), CalcError.of('#DIV/0!'), CalcError.of('#VALUE!')];
const vtArg = (v) => vt0(v);
const LOCS = { en: FN.EN_US, pt: FN.PT_BR };
for (const x of TRAPV) for (const y of TRAPV) {
  const loc = pick(['en', 'pt']);
  for (const op of ['+', '-', '*', '/', '^']) {
    const top = rng.below(2) === 0;
    const k = key('o');
    w.set(k, 'in', JSON.stringify(['arith', op, vtArg(x), vtArg(y), loc, top]));
    w.set(k, 'out', vt(OPS.arith(op, x, y, LOCS[loc], top, false)));
  }
  for (const op of ['=', '<>', '<', '<=', '>', '>=']) {
    if (rng.below(3)) continue;
    const k = key('o');
    w.set(k, 'in', JSON.stringify(['compare', op, vtArg(x), vtArg(y)]));
    w.set(k, 'out', vt(OPS.compare(op, x, y)));
  }
  const k = key('o');
  w.set(k, 'in', JSON.stringify(['concat', vtArg(x), vtArg(y), loc]));
  w.set(k, 'out', vt(OPS.concat(x, y, LOCS[loc])));
}
for (const x of TRAPV) for (const loc of ['en', 'pt']) {
  for (const fn of ['negate', 'percent']) {
    const k = key('o');
    w.set(k, 'in', JSON.stringify([fn, vtArg(x), loc]));
    w.set(k, 'out', vt(OPS[fn](x, LOCS[loc], false)));
  }
}
for (const x of TRAPV) if (typeof x === 'number') for (const fn of ['numResult']) { const k = key('o'); w.set(k, 'in', JSON.stringify([fn, vtArg(x)])); w.set(k, 'out', vt(OPS.numResult(x))); }

// ---------------------------------------------------------------- a1 helpers and numberLiteralValue
const A1T = ['A1', '$B$2', 'a1', 'XFD1048576', 'XFE1', 'A0', 'A1048577', '$A$', 'AAAA1', 'A12345678', 'B', '1', '', 'A1:B2', ' A1', 'A1 ', 'Ä1', 'A١', '$$A1', 'A$$1', 'Z0001'];
const AREAT = ['A1:B3', 'B3:A1', 'A:A', 'c:A', '1:1', '3:2', 'C5', 'A1:B2:C3', '$A$1:$B$2', 'A:', ':', '1:1048577', '0:1', 'A1:', 'XFD:XFD', '$1:$3', 'A1:1', '1:A'];
for (const s of A1T) { const k = key('a'); w.set(k, 'in', JSON.stringify(['a1ToCell', s])); w.set(k, 'out', cj(A1.a1ToCell(s))); }
for (const s of [...A1T, ...AREAT]) { const k = key('a'); w.set(k, 'in', JSON.stringify(['a1ToArea', s])); w.set(k, 'out', cj(A1.a1ToArea(s, 2))); }
for (const s of ['A', 'XFD', 'XFE', 'a', 'zz', 'AAAA', '', '1', 'Ä']) { const k = key('a'); w.set(k, 'in', JSON.stringify(['colFromLetters', s])); w.set(k, 'out', cj(A1.colFromLetters(s))); }
for (const c of [0, 1, 25, 26, 27, 701, 702, 16383, 16384]) { const k = key('a'); w.set(k, 'in', JSON.stringify(['colToLetters', c])); w.set(k, 'out', JSON.stringify(A1.colToLetters(c))); }
for (const ar of [[0, 0, 0, 0], [0, 0, 1048575, 2], [3, 0, 5, 16383], [0, 0, 1048575, 16383], [2, 3, 9, 7], [1, 1, 1, 1]]) {
  const k = key('a'); w.set(k, 'in', JSON.stringify(['areaToA1', ar])); w.set(k, 'out', JSON.stringify(A1.areaToA1({ sheet: 0, r1: ar[0], c1: ar[1], r2: ar[2], c2: ar[3] })));
}
for (const n of ['Sheet1', 'My Sheet', "It's", 'A1', 'AB12', 'R1C1', 'r2', 'C', 'RC', 'TRUE', 'false', '1abc', 'ação', '_x', '\\x', 'a.b', 'a-b', '', '😀', 'Ab١', 'x\ud800', 'R1C1C', 'ABCD1', 'Ǆ']) {
  const k = key('a'); w.set(k, 'in', JSON.stringify(['quoteSheetName', n])); w.set(k, 'out', JSON.stringify(A1.quoteSheetName(n)));
}
for (const r of [-1, 0, 5, 1048576, 1048577, -1048577, 2 ** 40, -(2 ** 40), 1e300, -0]) {
  for (const fn of ['wrapRow', 'wrapCol']) { const k = key('a'); w.set(k, 'in', JSON.stringify([fn, vtArg(r)])); w.set(k, 'out', vt(A1[fn](r))); }
}
const LV = ['0', '1', '1.5', '.5', '5.', '1e5', '1E+300', '1e-300', '1e-310', '1e309', '12345678901234567890', '0.1234567890123456789', '000123.4500', '1.99999999999999999',
  '1e', 'e5', '', '.', '1.2.3', '1e99999999999999999999', '1e-99999999999999999999', '0e999', '9'.repeat(320), '0.' + '0'.repeat(320) + '1', '1e+5', '1E-0', '2.2250738585072014e-308', '2.2250738585072011e-308'];
for (const s of LV) { const k = key('lv'); w.set(k, 'in', JSON.stringify(s)); w.set(k, 'out', cj(P.numberLiteralValue(s))); }

// ---------------------------------------------------------------- applyFn over synthetic definitions
// body: logs its arguments (thunks forced) and returns the first argument when it is a scalar, else the count
const log = [];
const MODESETS = [['value'], ['array'], ['ref'], ['any'], ['lazy'], ['value', 'array'], ['value', 'lazy', 'value'], ['ref', 'value']];
const mctx = T.mockContext({ sheets: { Sheet1: { A1: 1, A2: 'x', A3: true, B1: 2, B2: CalcError.of('#N/A'), C3: 5 }, S2: { B2: 7 } }, at: 'B2' });
const mctxLegacy = T.mockContext({ sheets: { Sheet1: { A1: 1, A2: 'x', A3: true, B1: 2, B2: CalcError.of('#N/A'), C3: 5 }, S2: { B2: 7 } }, at: 'B2', arrayContext: false });
const ARGS = [1, 'a', null, true, CalcError.of('#DIV/0!'), new RefValue([{ sheet: 0, r1: 0, c1: 0, r2: 0, c2: 0 }]), new RefValue([{ sheet: 0, r1: 0, c1: 0, r2: 2, c2: 0 }]),
  new RefValue([{ sheet: 0, r1: 0, c1: 0, r2: 2, c2: 2 }]), new RefValue([{ sheet: 0, r1: 0, c1: 0, r2: 0, c2: 0 }, { sheet: 0, r1: 1, c1: 1, r2: 1, c2: 1 }]),
  new RefValue([{ sheet: 1, r1: 0, c1: 0, r2: 2, c2: 2 }]), ArrayValue.of([[1, 2], [3, 4]]), ArrayValue.of([[1, 2, 3]]), ArrayValue.of([[1], [2]]),
  new ArrayValue(0, 0, []), ArrayValue.of([[CalcError.of('#N/A'), 'b']]), ArrayValue.fill(9, 9, (r, c) => (r * c) % 3),
  new LambdaValue(['X'], () => 1, undefined)];
const argText = (v) => (v instanceof LambdaValue ? 'L' : vt(v));
for (let i = 0; i < 1500 * SCALE; i++) {
  const modes = pick(MODESETS), lift = rng.below(2) === 0, acceptsErrors = rng.below(3) === 0, volatile = rng.below(4) === 0;
  const min = rng.below(2), max = min + rng.below(4);
  const n = rng.below(5);
  const raw = [], desc = [];
  for (let j = 0; j < n; j++) {
    const v = pick(ARGS);
    const thunk = rng.below(4) === 0;
    desc.push([thunk ? 'T' : 'V', argText(v)]);
    if (thunk) { const t = () => v; Object.defineProperty(t, 'omitted', { value: false }); raw.push(t); } else raw.push(v);
  }
  const omitted = rng.below(3) === 0 && n ? [rng.below(n)] : undefined;
  const arrayContext = rng.below(3) !== 0;
  const def = {
    name: 'T', minArgs: min, maxArgs: max, args: (k) => modes[Math.min(k, modes.length - 1)], lift, volatile, acceptsErrors,
    call(xs) {
      const seenArgs = xs.map((x) => (typeof x === 'function' ? `T${x.omitted ? 'o' : ''}:${argText(x())}` : argText(x)));
      log.push(`${seenArgs.join('|')}${FN.isOmitted(xs, 0) ? '!o0' : ''}${FN.isOmitted(xs, 1) ? '!o1' : ''}`);
      const f = xs[0];
      if (f === null || typeof f !== 'object' && typeof f !== 'function') return f ?? null;
      if (f instanceof CalcError) return f;
      return xs.length;
    },
  };
  log.length = 0;
  let r;
  try { r = vt(AP.applyFn(def, raw, arrayContext ? mctx : mctxLegacy, omitted ? { arrayContext, omitted } : { arrayContext })); } catch (e) { r = `throw:${e?.constructor?.name}`; }
  const k = key('ap');
  w.set(k, 'in', JSON.stringify({ modes, lift, acceptsErrors, volatile, min, max, args: desc, omitted, arrayContext }));
  w.set(k, 'out', C.clip(r, 4096));
  w.set(k, 'log', C.clip(JSON.stringify(log), 4096));
}

w.meta('stats', `records=${total} formulas=${formulas.length} bank=${bankCount} generated=${genCount}`);
mkdirSync(dirname(out), { recursive: true });
writeFileSync(out, gzipSync(Buffer.from(w.text(), 'utf8'), { level: 9 }));
console.log(`a2${big ? ' (big)' : ''}: ${total} records (${[...counts].map(([k, v]) => `${k} ${v}`).join(', ')}), ${formulas.length} formulas -> ${out}`);
