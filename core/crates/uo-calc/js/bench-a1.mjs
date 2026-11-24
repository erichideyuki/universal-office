// A1 baseline: the frozen TS of the A1 hot helpers on the inputs of benches/a1.rs (each input a pure function of its
// index), ns per call, medians of 7 rounds after one warm-up. Record core/docs/modules/uo-calc.md "A1".
//
//   node core/crates/uo-calc/js/bench-a1.mjs
process.env.TZ = 'UTC';
const { join } = await import('node:path');
const { pathToFileURL } = await import('node:url');
const C = await import('./lib/common.mjs');
const base = join(C.tsRoot(), 'sheet', 'packages', 'calc', 'dist');
const imp = (p) => import(pathToFileURL(join(base, p)).href);
const coerce = await imp('coerce.js');
const criteria = await imp('criteria.js');
const collation = await imp('collation.js');
const libm = await imp('functions/libm.js');
const { PT_BR } = await imp('fn.js');

function nsPer(n, f) {
  f();
  const t = [];
  for (let r = 0; r < 7; r++) { const t0 = process.hrtime.bigint(); f(); t.push(Number(process.hrtime.bigint() - t0) / n); }
  return t.sort((a, b) => a - b)[3];
}
const grouped = (v) => String(v).replace(/\B(?=(\d{3})+(?!\d))/g, '.');
const textOf = (i) => {
  switch (i % 5) {
    case 0: return grouped((i * 7919) % 1000000);
    case 1: return `${i % 1000},${i % 97}`;
    case 2: return `abc${String.fromCharCode(97 + (i % 26))}`;
    case 3: return `${String(1 + (i % 28)).padStart(2, '0')}/${String(1 + (i % 12)).padStart(2, '0')}/2024`;
    default: return `R$ ${i % 5000},${String(i % 100).padStart(2, '0')}`;
  }
};
const cellOf = (i) => (i % 4 === 0 ? i % 100 : i % 4 === 1 ? `item${i % 50}` : i % 4 === 2 ? (i % 8 === 2 ? 'apple' : 'Abc') : (i % 7) / 2);
const word = (k) => { let s = 'w'; k %= 17576; for (let j = 0; j < 3; j++) { s += String.fromCharCode(97 + (k % 26)); k = Math.floor(k / 26); } return s; };
const N = 100000;
const out = [];
const texts = Array.from({ length: N }, (_, i) => textOf(i));
out.push(['parseNumberText pt-BR mixed', nsPer(N, () => texts.filter((t) => coerce.parseNumberText(t, PT_BR) !== undefined).length)]);
const cells = Array.from({ length: N }, (_, i) => cellOf(i));
for (const crit of ['>=50', '=1', 'a*', 'abc']) {
  out.push([`criteria ${crit} match`, nsPer(N, () => { const c = criteria.parseCriterion(crit, PT_BR); return cells.filter((v) => c(v)).length; })]);
}
const ascii = Array.from({ length: N }, (_, i) => [word(i * 31), word(i * 17)]);
out.push(['compareText ASCII pairs', nsPer(N, () => ascii.filter(([a, b]) => collation.compareText(a, b) < 0).length)]);
const acc = Array.from({ length: N }, (_, i) => [`é${word(i * 31)}`, `e${word(i * 17)}á`]);
out.push(['compareText Latin-1 pairs', nsPer(N, () => acc.filter(([a, b]) => collation.compareText(a, b) < 0).length)]);
const words = Array.from({ length: 20000 }, (_, i) => word(i * 7919));
out.push(['sort 20k ASCII words by compareText', nsPer(20000, () => [...words].sort(collation.compareText).length)]);
const xs = Array.from({ length: N }, (_, i) => ((i * 7919) % 1000003) / 7);
out.push(['numberToText', nsPer(N, () => xs.map((x) => coerce.numberToText(x, PT_BR)).filter((s) => s.length > 5).length)]);
const ys = Array.from({ length: 1000000 }, (_, i) => ((i % 100000) + 1) / 1000);
out.push(['crLog', nsPer(ys.length, () => { let s = 0; for (const x of ys) s += libm.crLog(x); return s; })]);
out.push(['xlExp', nsPer(ys.length, () => { let s = 0; for (const x of ys) s += libm.xlExp(x); return s; })]);
for (const [k, v] of out) console.log(`${k}: ${v.toFixed(1)} ns/call`);
