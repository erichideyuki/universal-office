// A5 baseline: the frozen TS translator (`toLocal`/`fromLocal`), the legacy `@` rewrite and the open-recalc decision on
// the inputs of benches/a5.rs, medians of 7 rounds after one warm-up. Record core/docs/modules/uo-calc.md "A5".
//
//   node core/crates/uo-calc/js/bench-a5.mjs
// Inputs (a pure function of the index, the same in benches/a5.rs):
// - 100,000 canonical formulas `formula(k)` (15 templates: functions, ranges, sheets, strings, arrays, errors,
//   structured references, LET) → `toLocal(·, {lang:'pt-BR'})`; their pt-BR texts → `fromLocal(·, {lang:'pt-BR'})`;
// - legacyToDynamic + printFormula(ui) of the 15 templates parsed once each × 100,000 calls (the engine rewrites once
//   per hash-consed tree: this is the per-tree cost);
// - shouldRecalcOnOpen over (a) giant-1m (10,000 × 100 numbers, no formula, calcId 191029), (b) 100,000 `A{r}+1`
//   formulas with values, (c) the same + one NOW() without ca at the end.
process.env.TZ = 'UTC';
const { join } = await import('node:path');
const { pathToFileURL } = await import('node:url');
const C = await import('./lib/common.mjs');
const base = join(C.tsRoot(), 'sheet', 'packages', 'calc', 'dist');
const imp = (p) => import(pathToFileURL(join(base, p)).href);
const calc = await imp('index.js');
const TR = await imp('i18n/translate.js');
const IM = await imp('implicit.js');
const OR = await imp('open-recalc.js');

function median(f, rounds = 7) {
  f();
  const t = [];
  for (let r = 0; r < rounds; r++) { const t0 = process.hrtime.bigint(); f(); t.push(Number(process.hrtime.bigint() - t0)); }
  return t.sort((a, b) => a - b)[Math.floor(rounds / 2)];
}

export const TEMPLATES = [
  'SUM(A{i}:B{j})*2', 'IF(A{i}>0,"yes","no")', 'VLOOKUP(A{i},Sheet2!$A$1:$C$100,3,FALSE)', 'AVERAGE(A{i}:A{j})+MAX(B{i}:B{j})',
  'IFERROR(INDEX(C:C,MATCH(A{i},B:B,0)),#N/A)', 'ROUND(A{i}*1.5,2)', 'TEXT(A{i},"0.00")&" items"', 'LET(x,A{i},y,x*2,x+y)',
  'SUMIFS(C:C,A:A,">="&A{i},B:B,"x")', 'COUNTIF(A1:A{j},A{i})>1', '{1.5,2;3,4}*A{i}', 'Tabela1[[#This Row],[Qtd]]*A{i}',
  'NOW()-A{i}', 'CONCATENATE(A{i}," ",B{i})', 'AND(A{i}>0,B{i}<10,OR(C{i}=1,D{i}=2))',
];
export function formula(k) {
  const i = 1 + (k % 1000);
  return '=' + TEMPLATES[k % TEMPLATES.length].replaceAll('{i}', String(i)).replaceAll('{j}', String(i + 10));
}
const N = 100_000;
const canon = Array.from({ length: N }, (_, k) => formula(k));
const local = canon.map((t) => TR.toLocal(t, { lang: 'pt-BR' }));
const rows = [];
const tl = median(() => { for (const t of canon) TR.toLocal(t, { lang: 'pt-BR' }); });
rows.push(['toLocal pt-BR, 100k formulas (ns/formula)', tl / N]);
const fl = median(() => { for (const t of local) TR.fromLocal(t, { lang: 'pt-BR' }); });
rows.push(['fromLocal pt-BR, 100k formulas (ns/formula)', fl / N]);
const trees = TEMPLATES.map((t) => calc.parseFormula(t.replaceAll('{i}', '5').replaceAll('{j}', '15')));
const lg = median(() => { for (let k = 0; k < N; k++) calc.printFormula(IM.legacyToDynamic(trees[k % trees.length]), { ui: true }); });
rows.push(['legacyToDynamic + print(ui), per tree (ns)', lg / N]);

const book = (cells) => ({ workbook: { appName: 'xl', calcPr: { calcId: 191029 }, sheets: [{ name: 'S' }] }, worksheets: [{ rows: cells }], sharedStrings: [] });
const giant = [];
for (let r = 0; r < 10_000; r++) { const cells = []; for (let c = 0; c < 100; c++) cells.push({ ref: 'X', value: String((r + 1) * 1000 + c + 1) }); giant.push({ r: r + 1, cells }); }
const fbook = [];
for (let r = 0; r < N; r++) fbook.push({ r: r + 1, cells: [{ ref: `B${r + 1}`, formula: `A${r + 1}+1`, value: '1' }] });
const vbook = fbook.concat([{ r: N + 1, cells: [{ ref: `B${N + 1}`, formula: 'NOW()', value: '1' }] }]);
for (const [name, b] of [['giant-1m (no formula)', book(giant)], ['100k formulas', book(fbook)], ['100k formulas + NOW() at the end', book(vbook)]]) {
  let plan;
  const t = median(() => { plan = OR.shouldRecalcOnOpen(b); });
  rows.push([`shouldRecalcOnOpen ${name} (ms) → ${plan.mode}/${plan.reason}`, t / 1e6]);
}
for (const [k, v] of rows) console.log(`${k.padEnd(70)} ${v.toFixed(1)}`);
