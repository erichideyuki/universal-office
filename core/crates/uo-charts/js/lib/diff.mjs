// Differential facets of the chart capture (`--diff`): the pure numeric engines of the frozen TS on seeded random inputs,
// extremes included (NaN, huge spans, tiny units, equal ends, logs): `niceStep`, `autoScale`, `tickValues`, `frac` (scale.ts),
// `fitTrend` (trend.ts) and `histogramBins` (chartex.ts). The inputs travel in the sidecar `charts-diff.cases.jsonl.gz`, one
// line per case:
//
//   <fn>\t<index>\t<canonical JSON of the argument array>\t<canonical JSON of the result>
//
// Numbers use the canonical JSON rules (`{"$num":"NaN"}` ...); `undefined` array elements are `null`; a function that throws is
// `{"$throw":"<message>"}`. `fitTrend` returns closures: the result is the fit minus `f`, plus `at`, `[[x, f(x) | "undefined"], ...]`
// at the probe points (the inputs `xs`, the extremes, zero and one). `histogramBins` takes its `layoutPr` as the canonical node
// `{ name: 'cx:layoutPr', attrs: {}, children: [ { name: 'cx:binning', attrs, children: [ {cx:binSize}, {cx:binCount} ] } ] }`.
// The capture facets are `<fn>.<index>` = the digest of the result text (and `<fn>.n`).
import { canonJson, textDigest } from '../../../uo-pptx/js/lib/canon-json.mjs';
import { Rand } from '../../../uo-pptx/js/lib/gen/rand.mjs';

export const DIFF_SEED = 790000;

const SPECIAL = [0, -0, 1, -1, NaN, Infinity, -Infinity, 1e-300, 1e300, 1e-9, 1e12, 0.1, 0.2, 5, 10, 100, 1000, 1e-3, 999.9999, 1e15, 2 ** 53];

function num(r, kind) {
  switch (kind) {
    case 'special': return r.pick(SPECIAL);
    case 'pow10': return 10 ** r.int(-12, 12);
    case 'int': return r.int(-1000, 1000);
    case 'dec': return r.int(-100000, 100000) / 1000;
    case 'wide': return (r.int(-1000, 1000) / 100) * 10 ** r.int(-8, 9);
    default: return r.int(0, 10000) / 100;
  }
}
const anyNum = (r) => num(r, r.weighted([['special', 2], ['pow10', 2], ['int', 3], ['dec', 3], ['wide', 2], ['pos', 3]]));

function run(fn) {
  try { return fn(); } catch (e) { return { $throw: String(e?.message ?? e) }; }
}

export function runDiff(ts, writer) {
  const lines = [];
  let cases = 0;
  const add = (name, i, args, result) => {
    const text = canonJson(result);
    lines.push(`${name}\t${i}\t${canonJson(args)}\t${text}`);
    writer.set(`diff/${name}`, String(i), textDigest(text));
    cases++;
  };
  const r = new Rand(DIFF_SEED);
  const { niceStep, autoScale, tickValues, frac } = ts.scale;

  for (let i = 0; i < 400; i++) {
    const span = r.chance(0.5) ? anyNum(r) : Math.abs(anyNum(r)), div = r.pick([0, 1, 2, 3, 4, 5, 6, 8, 10, 12, 20, 100, NaN, -1, 7.5]);
    add('niceStep', i, [span, div], run(() => niceStep(span, div)));
  }
  const scales = [];
  for (let i = 0; i < 700; i++) {
    const mode = r.pick(['normal', 'normal', 'equal', 'neg', 'special', 'log', 'tiny', 'huge']);
    let lo, hi;
    if (mode === 'equal') lo = hi = anyNum(r);
    else if (mode === 'neg') { lo = -Math.abs(anyNum(r)); hi = r.chance(0.5) ? -Math.abs(anyNum(r)) : Math.abs(anyNum(r)); }
    else if (mode === 'special') { lo = r.pick(SPECIAL); hi = r.pick(SPECIAL); }
    else if (mode === 'tiny') { lo = r.int(1, 99) / 1e8; hi = lo + r.int(1, 99) / 1e9; }
    else if (mode === 'huge') { lo = r.int(1, 9) * 1e12; hi = lo * r.pick([1.0001, 1.5, 100]); }
    else { lo = num(r, 'wide'); hi = r.chance(0.2) ? lo : num(r, 'wide'); }
    const fixed = {};
    if (r.chance(0.2)) fixed.min = anyNum(r);
    if (r.chance(0.2)) fixed.max = anyNum(r);
    if (r.chance(0.2)) fixed.major = r.pick([0.5, 1, 2, 10, 25, 1e-9, 1e12, 0, -3, NaN]);
    if (r.chance(0.1)) fixed.minor = r.pick([0.1, 1, 0]);
    if (mode === 'log' || r.chance(0.05)) fixed.log = r.pick([10, 2, 1000, 3, 1, 0, 1.5]);
    const div = r.pick([1, 2, 3, 5, 6, 8, 10, 12, 20, 4, 11]);
    const pad = r.pick([undefined, 0.05, 0, 0.1, 0.5, -0.1]);
    const args = pad === undefined ? [lo, hi, div, fixed] : [lo, hi, div, fixed, pad];
    const res = run(() => autoScale(...args));
    add('autoScale', i, args, res);
    if (!res.$throw) scales.push(res);
  }
  scales.push({ lo: 0, hi: 10, major: 2, minor: 0.4 }, { lo: 1, hi: 1000, major: 0, minor: 0, log: 10 }, { lo: -5, hi: 5, major: 0, minor: 1 }, { lo: 0, hi: 1, major: 1e-12, minor: 0 }, { lo: 1, hi: 1e6, major: 0, minor: 0, log: 1 }, { lo: NaN, hi: 1, major: 1, minor: 0 });
  for (let i = 0; i < 500; i++) {
    const s = r.pick(scales);
    const unit = r.pick([undefined, undefined, undefined, s.major, s.major / 5, 0, -1, 2, 10, 1e-9, NaN, 100]);
    const args = unit === undefined ? [s] : [s, unit];
    add('tickValues', i, args, run(() => tickValues(...args)));
  }
  for (let i = 0; i < 300; i++) {
    const s = r.pick(scales), v = r.pick([...SPECIAL, s.lo, s.hi, (s.lo + s.hi) / 2, s.lo * 2, -s.lo]);
    add('frac', i, [s, v], run(() => frac(s, v)));
  }

  const KINDS = ['linear', 'poly', 'exp', 'log', 'power', 'movingAvg', 'bogus'];
  for (let i = 0; i < 400; i++) {
    const kind = r.pick(KINDS), n = r.pick([0, 1, 2, 3, 5, 8, 20, 60]);
    const mode = r.pick(['lin', 'noisy', 'exp', 'pow', 'const', 'neg', 'special', 'big']);
    const xs = [], ys = [];
    for (let k = 0; k < n; k++) {
      const x = mode === 'neg' ? r.int(-20, 20) : mode === 'big' ? r.int(1, 30) * 1e5 : mode === 'const' ? 3 : k + r.int(0, 3) / 4;
      let y;
      if (mode === 'lin') y = 2 * x + 1; else if (mode === 'exp') y = 3 * Math.exp(0.2 * x); else if (mode === 'pow') y = 2 * Math.max(1e-3, x) ** 1.5; else if (mode === 'const') y = r.int(0, 9);
      else if (mode === 'special') y = r.pick([NaN, 1, Infinity, -2, 0, 1e300]); else if (mode === 'big') y = r.int(1, 99) * 1e-5; else y = r.int(-50, 50) / 4;
      xs.push(mode === 'special' && r.chance(0.2) ? NaN : x); ys.push(y);
    }
    const o = {};
    if (r.chance(0.5)) o.order = r.pick([2, 3, 4, 5, 6, 1, 9, 2.5, NaN]);
    if (r.chance(0.4)) o.period = r.pick([2, 3, 5, 1, 100, 2.4, NaN]);
    if (r.chance(0.3)) o.intercept = r.pick([0, 1, -2, 3.5, 1e6]);
    const res = run(() => {
      const fit = ts.trend.fitTrend(kind, xs, ys, o);
      if (!fit) return null;
      const probe = [...xs, Math.min(...xs) - 1, Math.max(...xs) + 1, 0, 1, -1].slice(0, 70);
      const out = { ...fit };
      delete out.f;
      out.at = probe.map((x) => { const y = fit.f(x); return [x, y === undefined ? 'undefined' : y]; });
      return out;
    });
    add('fitTrend', i, [kind, xs, ys, o], res);
  }
  for (let i = 0; i < 160; i++) {
    const x = i < 16 ? anyNum2(i, r) : anyNum(r) / r.pick([1, 1000, 1e6]);
    add('trendNum', i, [x], run(() => ts.trend.trendNum(x)));
  }

  const node = (name, attrs, children = []) => ({ name, attrs, children });
  for (let i = 0; i < 300; i++) {
    const n = r.pick([0, 1, 2, 5, 10, 40, 200, 1000]);
    const mode = r.pick(['normal', 'flat', 'int', 'neg', 'special', 'tail']);
    const vals = [];
    for (let k = 0; k < n; k++) {
      let v;
      if (mode === 'flat') v = 7; else if (mode === 'int') v = r.int(0, 20); else if (mode === 'neg') v = r.int(-500, 500) / 10; else if (mode === 'special') v = r.pick([NaN, 1, 2, Infinity, -Infinity, 3]);
      else if (mode === 'tail') v = k < n - 2 ? r.int(0, 10) : 1000 * r.int(1, 9); else v = r.int(0, 10000) / 100;
      vals.push(v);
    }
    const b = {};
    const kids = [];
    if (r.chance(0.35)) kids.push(node('cx:binSize', { val: String(r.pick([0.5, 1, 5, 10, 25, 0, -2, 0.05, "x"])) }));
    if (r.chance(0.3)) kids.push(node('cx:binCount', { val: String(r.pick([1, 4, 6, 10, 0, -3, 50, 9999])) }));
    if (r.chance(0.2)) b.intervalClosed = r.pick(['l', 'r']);
    if (r.chance(0.2)) b.underflow = String(r.pick([0, 5, 20, -10]));
    if (r.chance(0.2)) b.overflow = String(r.pick([50, 80, 100, 3]));
    // the TS allocates `(range / width)` bins (the hostile loop of the record's JS traps): keep the cases at <= 3,000 bins
    const fin = vals.filter(Number.isFinite);
    const span = fin.length ? Math.max(...fin) - Math.min(...fin) : 0;
    for (const k of kids) {
      if (k.name === 'cx:binSize' && span / Math.abs(Number(k.attrs.val) || 1) > 3000) k.attrs.val = '1';
      if (k.name === 'cx:binCount' && Number(k.attrs.val) > 3000) k.attrs.val = '50';
    }
    if (b.underflow !== undefined && Math.abs(Number(b.underflow)) > 1e6) delete b.underflow;
    const lp = r.chance(0.8) ? node('cx:layoutPr', {}, [node('cx:binning', b, kids)]) : node('cx:layoutPr', {});
    const fmt = r.pick([undefined, 'General', '0.0', '#,##0', '0%', '"R$" #,##0.00', 'm/d/yyyy']);
    const args = fmt === undefined ? [vals, lp] : [vals, lp, fmt];
    add('histogramBins', i, args, run(() => ts.chartex.histogramBins(...args)));
  }
  return { lines, cases };
}

// probe numbers for trendNum: the same generator, decoupled from the argument list order
function anyNum2(i, r) { return [0, -0, 1.23456789, -1.23456789, 1e-5, -1e-5, 0.00005, 123456789.123, NaN, Infinity, 2.5, -0.00004, 1e21, 1e-7, 0.5, 99999.99995][i % 16]; }
