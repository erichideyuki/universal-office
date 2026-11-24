// A3 baseline: the frozen TS Engine on the operator-only calc classes of benches/a3.rs (the same cells, built here as
// the CalcWorkbook the engine takes; no .xlsx read). Per class: engine build, full recalc (`recalc()` on a built
// engine: median of 5 after one warm-up), `calculate()` where volatiles exist, retained heap of the engine (after GC),
// and the edit latency on a 20k chain (setCell on its head + calculate) and on an isolated cell (p50 / p95 of 20).
// Record core/docs/modules/uo-calc.md "A3 — as built".
//
//   node --expose-gc --max-old-space-size=8192 core/crates/uo-calc/js/bench-a3.mjs [--only <class>]
process.env.TZ = 'UTC';
const C = await import('./lib/common.mjs');
const { calc } = await C.loadCalc(C.tsRoot());
const a = C.args();
const only = a.opt('--only');

const f = (text, extra = {}) => ({ f: { text, ...extra } });
// the classes: [r, c, CellInput] lists (0-based), the same as `classes()` in benches/a3.rs
export const CLASSES = {
  'chain-100k': () => ({ cells: Array.from({ length: 100000 }, (_, i) => [i, 0, i === 0 ? { v: 1 } : f(`A${i}+1`)]) }),
  'sum3-100k': () => ({
    cells: Array.from({ length: 100000 }, (_, i) => [[i, 0, { v: (i * 7919) % 1000 }], [i, 1, f(`A${i + 1}+A${i + 2}+A${i + 3}*2`)]]).flat(),
  }),
  'text-50k': () => ({ cells: Array.from({ length: 50000 }, (_, i) => [[i, 0, { v: `texto ${i + 1} ção` }], [i, 1, f(`A${i + 1}&"x"&${i + 1}`)]]).flat() }),
  'dynamic-1k': () => ({
    cells: [
      ...Array.from({ length: 1000 }, (_, i) => [i, 0, { v: (i * 7919) % 997 }]),
      ...Array.from({ length: 20 }, (_, k) => [0, 2 + k, f(['$A$1:$A$1000*2', '-$A$1:$A$1000', '$A$1:$A$1000&"x"', '$A$1:$A$1000>500'][k % 4], { t: 'array', dynamic: true, ref: `${String.fromCharCode(67 + k)}1` })]),
    ],
  }),
  'volatile-10k': () => ({ cells: Array.from({ length: 10000 }, (_, i) => [[i, 0, f(`${i + 1}*1.5`, { ca: true })], [i, 1, f(`A${i + 1}*2+${i + 1}`)]]).flat() }),
  'cycles-iterate': () => ({
    cells: Array.from({ length: 1000 }, (_, i) => [[i, 0, f(`B${i + 1}*0.5+1`)], [i, 1, f(`A${i + 1}*0.5`)]]).flat(),
    calc: { iterate: true, iterateCount: 100, iterateDelta: 0.001 },
  }),
  'shared-1m': () => ({
    cells: [[0, 0, { v: 1 }], [0, 1, f('A1*2', { t: 'shared', si: 0, ref: 'B1:B1000000' })],
      ...Array.from({ length: 999999 }, (_, j) => [[j + 1, 0, { v: j + 2 }], [j + 1, 1, f('', { t: 'shared', si: 0 })]]).flat()],
  }),
};

const wbOf = (cls) => ({ sheets: [{ name: 'Plan1', cells: cls.cells }], ...(cls.calc ? { calc: cls.calc } : {}) });
const median = (xs) => { const s = [...xs].sort((x, y) => x - y); return s[s.length >> 1]; };
const gc = () => { for (let i = 0; i < 3; i++) globalThis.gc?.(); };
const heap = () => { gc(); return process.memoryUsage().heapUsed; };
const opts = { locale: calc.PT_BR, now: () => C.NOW_SERIAL, random: () => 0.5 };

const rows = [];
for (const [name, mk] of Object.entries(CLASSES)) {
  if (only && name !== only) continue;
  const wb = wbOf(mk());
  const h0 = heap();
  let t = performance.now();
  const e = new calc.Engine(wb, opts);
  const build = performance.now() - t;
  e.recalc();
  const retained = heap() - h0;
  const ts = [];
  for (let r = 0; r < 5; r++) { t = performance.now(); e.recalc(); ts.push(performance.now() - t); }
  const row = { name, formulas: e.allFormulas().length, buildMs: build, recalcMs: median(ts), retainedMB: retained / 1048576 };
  if (name === 'volatile-10k') {
    const cs = [];
    for (let r = 0; r < 5; r++) { t = performance.now(); e.calculate(); cs.push(performance.now() - t); }
    row.calculateMs = median(cs);
  }
  rows.push(row);
}
// edit latency: a 20k chain (setCell on A1 + calculate dirties all) and an isolated cell
if (!only || only === 'edit') {
  const e = new calc.Engine(wbOf({ cells: [...Array.from({ length: 20000 }, (_, i) => [i, 0, i === 0 ? { v: 1 } : f(`A${i}+1`)]), [0, 25, { v: 1 }]] }), opts);
  e.recalc();
  const lat = (row, col) => {
    const xs = [];
    for (let k = 0; k < 20; k++) { const t = performance.now(); e.setCell(0, row, col, { v: k + 3 }); e.calculate(); xs.push(performance.now() - t); }
    const s = [...xs].sort((x, y) => x - y);
    return { p50: s[10], p95: s[19] };
  };
  rows.push({ name: 'edit chain-20k head', ...lat(0, 0) });
  rows.push({ name: 'edit isolated', ...lat(0, 25) });
}
for (const r of rows) console.log(JSON.stringify(r));
