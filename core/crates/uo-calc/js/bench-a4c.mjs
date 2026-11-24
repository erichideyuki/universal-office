// A4c baseline: the frozen TS Engine on the finance/date classes of benches/a4c.rs (the same cells, built here as the
// CalcWorkbook the engine takes). Per class: engine build, full recalc (`recalc()` on a built engine: median of 5 after
// one warm-up), retained heap of the engine (after GC). Record core/docs/modules/uo-calc.md "A4c — as built".
//
//   node --expose-gc --max-old-space-size=8192 core/crates/uo-calc/js/bench-a4c.mjs [--only <class>]
//
// UO_A4C_ONLY=<j>: one formula kind of the class (per-function timing).
process.env.TZ = 'UTC';
const C = await import('./lib/common.mjs');
const { calc } = await C.loadCalc(C.tsRoot());
const a = C.args();
const only = a.opt('--only');

const f = (text) => ({ f: { text } });
const pick = (k, n) => (process.env.UO_A4C_ONLY === undefined ? k % n : Number(process.env.UO_A4C_ONLY));
// the classes: [r, c, CellInput] lists (0-based), the same as `class()` in benches/a4c.rs
export const CLASSES = {
  // 50,000 formulas over a column of flows A (one in 7 negative) and of rates B: PMT, FV, PV, RATE, NPV and IRR over
  // 20-value windows, MIRR, IPMT/PPMT, DB/DDB, a coupon PRICE
  'finance-50k': () => {
    const cells = [];
    for (let k = 0; k < 50000; k++) {
      const i = k + 1, w = `A${Math.max(1, i - 19)}:A${i}`;
      const t = [`PMT(B${i}/12,360,A${i}*1000)`, `FV(B${i},10,-100,A${i},1)`, `PV(B${i},20,-A${i})`, `RATE(48,-200,MOD(ABS(A${i}),900)+8000)`,
        `NPV(B${i},${w})`, `IRR(${w})`, `MIRR(${w},0.1,0.12)`, `IPMT(B${i},3,10,A${i})+PPMT(B${i},3,10,A${i})`,
        `DB(10000,1000,10,MOD(${i},10)+1)+DDB(10000,1000,10,MOD(${i},10)+1)`, `PRICE(45000,45000+MOD(${i},5000)+400,0.05,B${i},100,2,0)`];
      cells.push([k, 0, { v: i % 7 === 1 ? -5000 - (i % 13) : ((i * 7919) % 1000) + 0.37 }], [k, 1, { v: 0.01 + ((i * 31) % 90) / 1000 }],
        [k, 2, f(t[pick(k, t.length)])]);
    }
    return { cells };
  },
  // 50,000 date formulas over a column of dates A (2020–2026): DATE, EDATE, EOMONTH, NETWORKDAYS, YEARFRAC (bases 0
  // and 1), WEEKDAY/WEEKNUM, DATEDIF, WORKDAY, DAYS360, YEAR/MONTH/DAY
  'dates-50k': () => {
    const cells = [];
    for (let k = 0; k < 50000; k++) {
      const i = k + 1;
      const t = [`DATE(2024,MOD(A${i},12)+1,MOD(A${i},28)+1)`, `EDATE(A${i},MOD(${i},25)-12)`, `EOMONTH(A${i},MOD(${i},7)-3)`,
        `NETWORKDAYS(A${i},A${i}+MOD(${i},400))`, `YEARFRAC(A${i},A${i}+MOD(${i},900),1)`, `YEARFRAC(A${i},A${i}+MOD(${i},900),0)`,
        `WEEKDAY(A${i},2)+WEEKNUM(A${i},21)`, `DATEDIF(A${i},A${i}+MOD(${i},2000),"YD")`, `WORKDAY(A${i},MOD(${i},60))`,
        `DAYS360(A${i},A${i}+MOD(${i},700))+YEAR(A${i})+MONTH(A${i})+DAY(A${i})`];
      cells.push([k, 0, { v: 43831 + ((i * 7919) % 2400) }], [k, 2, f(t[pick(k, t.length)])]);
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
  // the results, for the comparison with the Rust run: numbers, errors and the bits of their sum in row order
  let nums = 0, errs = 0, sum = 0;
  for (let r = 0; r < 50000; r++) { const v = e.getValue(0, r, 2); if (typeof v === 'number') { nums++; sum += v; } else errs++; }
  const b = Buffer.alloc(8); b.writeDoubleBE(sum);
  console.log(JSON.stringify({ name, formulas: e.allFormulas().length, buildMs: build, recalcMs: median(ts), retainedMB: retained / 1048576, nums, errs, sum: b.toString('hex') }));
}
