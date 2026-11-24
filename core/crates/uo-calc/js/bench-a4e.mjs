// A4e baseline: the frozen TS Engine on the lookup/dynamic/conditional classes of benches/a4e.rs (the same cells,
// built here as the CalcWorkbook the engine takes). Per class: engine build, full recalc (`recalc()` on a built
// engine: median of 5 after one warm-up), retained heap of the engine (after GC), and the count + bits of the sum of
// the numeric results (the Rust runner prints the same). Record core/docs/modules/uo-calc.md "A4e — as built".
//
//   node --expose-gc --max-old-space-size=8192 core/crates/uo-calc/js/bench-a4e.mjs [--only <class>]
process.env.TZ = 'UTC';
const C = await import('./lib/common.mjs');
const { calc } = await C.loadCalc(C.tsRoot());
const a = C.args();
const only = a.opt('--only');

const f = (text, extra = {}) => ({ f: { text, ...extra } });
const dyn = (text, ref) => f(text, { t: 'array', dynamic: true, ref });
const colName = (c) => { let s = ''; for (let n = c + 1; n > 0; n = Math.floor((n - 1) / 26)) s = String.fromCharCode(65 + ((n - 1) % 26)) + s; return s; };
// the classes: [r, c, CellInput] lists (0-based), the same as `class()` in benches/a4e.rs
export const CLASSES = {
  // the TS baseline's lookup class (`gen-workbook.mjs` lookup-10kx10k)
  'lookup-10kx10k': () => Array.from({ length: 10000 }, (_, k) => {
    const i = k + 1;
    const t = i % 3 === 0 ? `VLOOKUP(${(i * 31) % 10007},$A$1:$B$10000,2,FALSE)` : i % 3 === 1 ? `MATCH(${(i * 17) % 10007},$A$1:$A$10000,0)` : `_xlfn.XLOOKUP(${(i * 13) % 10007},$A$1:$A$10000,$B$1:$B$10000,-1)`;
    return [[k, 0, { v: (i * 7919) % 10007 }], [k, 1, { v: i }], [k, 3, f(t)]];
  }).flat(),
  // the TS baseline's dynamic class (`gen-workbook.mjs` dynamic-1k): 20 spills of SORT/UNIQUE/FILTER/SEQUENCE
  'dynamic-1k': () => [
    ...Array.from({ length: 1000 }, (_, k) => [k, 0, { v: ((k + 1) * 7919) % 997 }]),
    ...Array.from({ length: 20 }, (_, k) => {
      const i = k + 1;
      const t = ['_xlfn._xlws.SORT($A$1:$A$1000)', '_xlfn.UNIQUE($A$1:$A$1000)', '_xlfn._xlws.FILTER($A$1:$A$1000,$A$1:$A$1000>500)', '_xlfn.SEQUENCE(1000)'][i % 4];
      return [0, 2 + k, dyn(t, `${colName(2 + k)}1`)];
    }),
  ],
  // 10,000 exact VLOOKUPs over 100,000 number keys
  'vlookup-exact-100k': () => [
    ...Array.from({ length: 100000 }, (_, k) => [[k, 0, { v: ((k + 1) * 7919) % 100003 }], [k, 1, { v: k + 1 }]]).flat(),
    ...Array.from({ length: 10000 }, (_, k) => [k, 3, f(`VLOOKUP(${((k + 1) * 31) % 100003},$A$1:$B$100000,2,FALSE)`)]),
  ],
  // 1,000 exact XLOOKUPs of texts over 100,000 text keys (linear in the TS: 10^8 collation compares)
  'xlookup-text-100k': () => [
    ...Array.from({ length: 100000 }, (_, k) => [[k, 0, { v: `k${((k + 1) * 7919) % 100003}` }], [k, 1, { v: k + 1 }]]).flat(),
    ...Array.from({ length: 1000 }, (_, k) => [[k, 2, { v: `K${((k + 1) * 13) % 100003}` }], [k, 3, f(`_xlfn.XLOOKUP(C${k + 1},$A$1:$A$100000,$B$1:$B$100000)`)]]).flat(),
  ],
  // 10,000 approximate MATCH / VLOOKUP TRUE (binary search) over 100,000 sorted numbers
  'match-approx-100k': () => [
    ...Array.from({ length: 100000 }, (_, k) => [[k, 0, { v: (k + 1) * 2 }], [k, 1, { v: k + 1 }]]).flat(),
    ...Array.from({ length: 10000 }, (_, k) => [k, 3, f(k % 2 ? `MATCH(${((k + 1) * 37) % 200000},$A$1:$A$100000,1)` : `VLOOKUP(${((k + 1) * 37) % 200000},$A$1:$B$100000,2,TRUE)`)]),
  ],
  // 200 COUNTIFS / SUMIFS / COUNTIF / AVERAGEIF over 100,000 rows (a text and a number column)
  'ifs-100k': () => [
    ...Array.from({ length: 100000 }, (_, k) => [[k, 0, { v: `c${(k + 1) % 50}` }], [k, 1, { v: ((k + 1) * 7919) % 1000 }]]).flat(),
    ...Array.from({ length: 200 }, (_, k) => {
      const t = [
        `COUNTIFS($A$1:$A$100000,"c${k % 50}",$B$1:$B$100000,">${(k * 7) % 1000}")`,
        `SUMIFS($B$1:$B$100000,$A$1:$A$100000,"c${k % 50}",$B$1:$B$100000,"<500")`,
        `COUNTIF($B$1:$B$100000,">${(k * 13) % 1000}")`,
        `AVERAGEIF($A$1:$A$100000,"c${k % 50}",$B$1:$B$100000)`,
      ][k % 4];
      return [k, 3, f(t)];
    }),
  ],
};

const median = (xs) => { const s = [...xs].sort((x, y) => x - y); return s[s.length >> 1]; };
const gc = () => { for (let i = 0; i < 3; i++) globalThis.gc?.(); };
const heap = () => { gc(); return process.memoryUsage().heapUsed; };
const opts = { locale: calc.PT_BR, now: () => C.NOW_SERIAL, random: () => 0.5 };
const f64bits = (x) => { const b = Buffer.alloc(8); b.writeDoubleBE(x); return b.toString('hex'); };

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
  // the numbers of the formula column(s) in row order
  let n = 0, errs = 0, sum = 0;
  e.forEachCell({ sheet: 0, r1: 0, c1: 2, r2: calc.MAX_ROW, c2: calc.MAX_COL }, (v, r, c) => {
    if (c === 2 && name !== 'dynamic-1k') return;
    if (typeof v === 'number') { n++; sum += v; } else if (v && typeof v === 'object') errs++;
  });
  console.log(JSON.stringify({ name, formulas: e.allFormulas().length, buildMs: build, recalcMs: median(ts), retainedMB: retained / 1048576, numbers: n, errors: errs, sumBits: f64bits(sum) }));
}
