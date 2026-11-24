// A4f baseline: the frozen TS Engine on the logical/information class of benches/a4f.rs (the same cells, built here
// as the CalcWorkbook the engine takes). Per class: engine build, full recalc (`recalc()` on a built engine: median
// of 5 after one warm-up), retained heap of the engine (after GC). Record core/docs/modules/uo-calc.md "A4f — as built".
//
//   node --expose-gc --max-old-space-size=8192 core/crates/uo-calc/js/bench-a4f.mjs [--only <class>]
//
// UO_A4F_ONLY=<j>: one formula kind of the class (per-function timing).
process.env.TZ = 'UTC';
const C = await import('./lib/common.mjs');
const { calc } = await C.loadCalc(C.tsRoot());
const a = C.args();
const only = a.opt('--only');

const f = (text) => ({ f: { text } });
const pick = (k, n) => (process.env.UO_A4F_ONLY === undefined ? k % n : Number(process.env.UO_A4F_ONLY));
// the classes: [r, c, CellInput] lists (0-based), the same as `class()` in benches/a4f.rs
export const CLASSES = {
  // 100,000 formulas over a column of numbers A (one in 9 empty, one in 11 #DIV/0!) and booleans B: IF, AND, IFERROR,
  // ISNUMBER, SWITCH, IFS, OR/NOT/XOR, CHOOSE, ISBLANK/ISEVEN, IFNA/TYPE
  'logical-100k': () => {
    const cells = [];
    for (let k = 0; k < 100000; k++) {
      const i = k + 1;
      const t = [`IF(A${i}>500,A${i}*2,A${i}/2)`, `AND(A${i}>100,A${i}<900,B${i})`, `IFERROR(1/(A${i}-500),-1)`, `ISNUMBER(A${i})`,
        `SWITCH(MOD(A${i},5),0,"zero",1,"one",2,"two","many")`, `IFS(A${i}<100,1,A${i}<500,2,A${i}<900,3,TRUE,4)`,
        `OR(A${i}<10,NOT(B${i}),XOR(A${i}>5,B${i}))`, `CHOOSE(MOD(A${i},3)+1,A${i},-A${i},0)`, `IF(ISBLANK(A${i}),0,ISEVEN(A${i}))`,
        `IFNA(IF(A${i}>990,NA(),A${i}),TYPE(A${i}))`];
      if (i % 9 !== 0) cells.push([k, 0, { v: i % 11 === 0 ? calc.ERR.DIV0 : (i * 7919) % 1000 }]);
      cells.push([k, 1, { v: i % 2 === 0 }], [k, 2, f(t[pick(k, t.length)])]);
    }
    return { cells };
  },
};

const wbOf = (cls) => ({ sheets: [{ name: 'Plan1', cells: cls.cells }] });
const median = (xs) => { const s = [...xs].sort((x, y) => x - y); return s[s.length >> 1]; };
const gc = () => { for (let i = 0; i < 3; i++) globalThis.gc?.(); };
const heap = () => { gc(); return process.memoryUsage().heapUsed; };
const opts = { locale: calc.PT_BR, now: () => C.NOW_SERIAL, random: () => 0.5 };

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
  // the results, for the comparison with the Rust run: numbers and booleans (as 0/0.5), the rest, the bits of the sum
  let nums = 0, errs = 0, sum = 0;
  for (let r = 0; r < 100000; r++) {
    const v = e.getValue(0, r, 2);
    if (typeof v === 'number') { nums++; sum += v; } else if (typeof v === 'boolean') { nums++; sum += v ? 0.5 : 0; } else errs++;
  }
  const b = Buffer.alloc(8); b.writeDoubleBE(sum);
  console.log(JSON.stringify({ name, formulas: e.allFormulas().length, buildMs: build, recalcMs: median(ts), retainedMB: retained / 1048576, nums, errs, sum: b.toString('hex') }));
}
