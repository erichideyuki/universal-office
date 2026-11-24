// A2 baseline: the frozen TS parser, printer and evaluator on the inputs of benches/a2.rs, ns per formula, medians of
// 7 rounds after one warm-up. Record core/docs/modules/uo-calc.md "A2 — as built".
//
//   node core/crates/uo-calc/js/bench-a2.mjs
// Inputs: the formulas of the oracle bank (`f` and `{f}` cells, files sorted, case order; their cell when known)
// and of the generated workbooks (calc-order sidecar), and 10,000 operator formulas (a pure function of the index)
// evaluated over a 100 × 10 grid of numbers.
process.env.TZ = 'UTC';
const { join } = await import('node:path');
const { readFileSync, readdirSync } = await import('node:fs');
const { gunzipSync } = await import('node:zlib');
const { pathToFileURL } = await import('node:url');
const C = await import('./lib/common.mjs');
const base = join(C.tsRoot(), 'sheet', 'packages', 'calc', 'dist');
const imp = (p) => import(pathToFileURL(join(base, p)).href);
const P = await imp('parse.js');
const PR = await imp('print.js');
const EV = await imp('evaluate.js');
const V = await imp('values.js');
const { EN_US } = await imp('fn.js');

function nsPer(n, f) {
  f();
  const t = [];
  for (let r = 0; r < 7; r++) { const t0 = process.hrtime.bigint(); f(); t.push(Number(process.hrtime.bigint() - t0) / n); }
  return t.sort((a, b) => a - b)[3];
}

// the formula list: written to the Rust side by `--dump` (the same order)
export function formulaList() {
  const list = [];
  const a1 = (ref) => { const m = /^([A-Z]+)(\d+)$/.exec(ref); if (!m) return [0, 0]; let c = 0; for (const ch of m[1]) c = c * 26 + ch.charCodeAt(0) - 64; return [+m[2] - 1, c - 1]; };
  for (const file of readdirSync(C.oracleDir).filter((x) => x.endsWith('.cases.json')).sort()) {
    const j = JSON.parse(readFileSync(join(C.oracleDir, file), 'utf8'));
    for (const c of Array.isArray(j) ? j : j.cases) {
      if (c.f) list.push([c.f.replace(/^=/, ''), 0, 0]);
      for (const [k, v] of Object.entries(c.cells ?? {})) if (v && typeof v === 'object' && 'f' in v) list.push([String(v.f).replace(/^=/, ''), ...a1(k)]);
    }
  }
  const side = gunzipSync(readFileSync(join(C.capturesDir, 'calc-order.side.gz'))).toString('utf8');
  for (const line of side.split('\n')) {
    const p = line.split('\t');
    if (p[1] !== 'wb') continue;
    for (const sh of JSON.parse(p.slice(2).join('\t')).sheets) for (const [r, c, cell] of sh.cells) if (cell.f?.text) list.push([cell.f.text, r, c]);
  }
  return list.filter(([f, r, c]) => { try { P.parseFormula(f, { row: r, col: c }); return true; } catch { return false; } });
}

export function evalFormula(i) {
  const r = 1 + (i % 100), s = 1 + ((i * 7) % 99);
  switch (i % 8) {
    case 0: return `A${r}+B${r}*2-C${r}/3`;
    case 1: return `(A${r}>B${s})*1+(A${r}=C${r})`;
    case 2: return `A${r}&"-"&B${s}`;
    case 3: return `-A${r}%+2^3`;
    case 4: return `LET(x,A${r},y,x*x,y+x)`;
    case 5: return `A${r}:B${s} B${r}:C${s}`;
    case 6: return `{1,2,3}*A${r}`;
    default: return `((A${r}+1)*(B${s}+2))^0.5`;
  }
}
export const gridValue = (r, c) => ((r * 31 + c * 7) % 97) / 4;

if (process.argv.includes('--dump')) {
  for (const [f, r, c] of formulaList()) console.log(JSON.stringify([f, r, c]));
  process.exit(0);
}

const list = formulaList();
const asts = list.map(([f, r, c]) => P.parseFormula(f, { row: r, col: c }));
const out = [];
out.push([`parse (${list.length} bank + generated formulas)`, nsPer(list.length, () => { let n = 0; for (const [f, r, c] of list) n += P.parseFormula(f, { row: r, col: c }).t.length; return n; })]);
out.push(['print (file form)', nsPer(list.length, () => { let n = 0; for (let i = 0; i < list.length; i++) n += PR.printFormula(asts[i], { row: list[i][1], col: list[i][2] }).length; return n; })]);
out.push(['parse + print', nsPer(list.length, () => { let n = 0; for (const [f, r, c] of list) n += PR.printFormula(P.parseFormula(f, { row: r, col: c }), { row: r, col: c }).length; return n; })]);

const grid = new Array(100 * 10);
for (let r = 0; r < 100; r++) for (let c = 0; c < 10; c++) grid[r * 10 + c] = gridValue(r, c);
const host = {
  locale: EN_US, date1904: false,
  sheetCount: () => 1, sheetName: () => 'Sheet1', sheetIndex: () => 0, externalSheet: () => undefined,
  cell: (s, r, c) => (r < 100 && c < 10 ? grid[r * 10 + c] : null),
  values(a) { return V.ArrayValue.fill(a.r2 - a.r1 + 1, a.c2 - a.c1 + 1, (r, c) => host.cell(a.sheet, a.r1 + r, a.c1 + c)); },
  forEachCell() {}, formulaText: () => undefined, isSubtotalCell: () => false, rowHidden: () => false, rowFiltered: () => false,
  now: () => 45658.5, random: () => 0.5, definedName: () => undefined, table: () => undefined, tableAt: () => undefined, spillArea: () => undefined,
};
const ev = new EV.Evaluator(host);
const N = 10000;
const evs = Array.from({ length: N }, (_, i) => { const row = 200 + (i % 50), col = 12; return [P.parseFormula(evalFormula(i), { row, col }), row, col]; });
out.push([`evaluate ${N} operator formulas`, nsPer(N, () => { let k = 0; for (const [n, r, c] of evs) { const v = ev.evalFormula(n, ev.context(0, r, c, true)); if (typeof v === 'number') k++; } return k; })]);
for (const [k, v] of out) console.log(`${k}: ${v.toFixed(1)} ns/formula`);
