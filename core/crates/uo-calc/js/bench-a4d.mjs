// A4d baseline: the frozen TS Engine on the text classes of benches/a4d.rs (the same cells, built here as the
// CalcWorkbook the engine takes). Per class: engine build, full recalc (`recalc()` on a built engine: median of 5
// after one warm-up), retained heap of the engine (after GC). Record core/docs/modules/uo-calc.md "A4d — as built".
//
//   node --expose-gc --max-old-space-size=8192 core/crates/uo-calc/js/bench-a4d.mjs [--only <class>]
process.env.TZ = 'UTC';
const C = await import('./lib/common.mjs');
const { calc } = await C.loadCalc(C.tsRoot());
const a = C.args();
const only = a.opt('--only');

const f = (text) => ({ f: { text } });
const range = (n, fn) => Array.from({ length: n }, (_, k) => fn(k + 1, k)).flat();
// the classes: [r, c, CellInput] lists (0-based), the same as `class()` in benches/a4d.rs
export const CLASSES = {
  'text-ts-50k': () => range(50000, (i, k) => [
    [k, 0, { v: `texto ${i} ção` }],
    [k, 1, f([`UPPER(A${i})`, `LEN(A${i})&"x"`, `SUBSTITUTE(A${i},"o","0")`, `TEXT(${i}/7,"0.00")`, `LEFT(A${i},5)&RIGHT(A${i},3)`][i % 5])],
  ]),
  'text-fn-50k': () => range(50000, (i, k) => [
    [k, 0, { v: `  Linha ${i} de texto com ação  ` }],
    [k, 1, { v: ((i * 7919) % 100000) / 7 }],
    [k, 2, f([`LEFT(A${i},4)`, `MID(A${i},3,5)`, `FIND("o",A${i})`, `SUBSTITUTE(A${i},"o","0")`, `UPPER(A${i})`, `TEXT(B${i},"#.##0,00")`,
      `CONCAT(A${i},"-",B${i})`, `TEXTJOIN(";",TRUE,A${i},B${i})`, `LOWER(A${i})`, `TRIM(A${i})`][i % 10])],
  ]),
  'regex-5k': () => range(5000, (i, k) => [
    [k, 0, { v: `pedido ${i}: cliente${i % 97} <x${i % 13}@ex.com> total ${i * 3}.${i % 100}` }],
    [k, 1, f([`REGEXTEST(A${i},"\\d{3,}")`, `REGEXEXTRACT(A${i},"[\\w.]+@[\\w.]+")`, `REGEXREPLACE(A${i},"[aeiou]","*")`,
      `REGEXTEST(A${i},"^pedido\\s+\\d+",1)`, `REGEXEXTRACT(A${i},"(\\d+)\\.(\\d+)",2)`, `REGEXREPLACE(A${i},"(\\w+)@","$1 at ")`][i % 6])],
  ]),
};

const median = (xs) => { const s = [...xs].sort((x, y) => x - y); return s[s.length >> 1]; };
const gc = () => { for (let i = 0; i < 3; i++) globalThis.gc?.(); };
const heap = () => { gc(); return process.memoryUsage().heapUsed; };
const opts = { locale: calc.PT_BR, now: () => C.NOW_SERIAL, random: () => 0.5 };

for (const [name, mk] of Object.entries(CLASSES)) {
  if (only && name !== only) continue;
  const wb = { sheets: [{ name: 'Plan1', cells: mk() }] };
  const h0 = heap();
  let t = performance.now();
  const e = new calc.Engine(wb, opts);
  const build = performance.now() - t;
  e.recalc();
  const retained = heap() - h0;
  const ts = [];
  for (let r = 0; r < 5; r++) { t = performance.now(); e.recalc(); ts.push(performance.now() - t); }
  console.log(JSON.stringify({ name, formulas: e.allFormulas().length, buildMs: build, recalcMs: median(ts), retainedMB: retained / 1048576 }));
}
