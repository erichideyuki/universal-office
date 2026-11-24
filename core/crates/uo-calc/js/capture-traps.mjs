// Parity capture group `calc-traps` (plan section 4.3 "Per-parameter traps", SKILL.md "Capture sources" 3): for every
// function of the frozen TS catalogue (`allFns()`, 535) and every parameter position, the values where JS and Rust
// semantics part, called through the TS function entry point (`callFn` of testing.ts = `applyFn` with the argument
// modes, as the evaluator) over the mock context of the bank (`mockContext`), with the clock fixed and a recorded
// seeded random source (decision C1).
//
//   node core/crates/uo-calc/js/capture-traps.mjs                         # CI subset -> tests/captures/calc-traps.capture
//   node --max-old-space-size=8192 core/crates/uo-calc/js/capture-traps.mjs --big \
//        --out core/target/sheet-cap/calc-traps-big.capture               # local: every trap x pt-BR and en-US + pairs
//   options: --seed <n> (default 0x7a9c0001)  --only <FN[,FN]>  --pairs <n per function, --big only; default 24>
//            --timeout <ms per call, default 5000>: the calls run in a child process; a call that does not return in
//            time is recorded as `timeout:<ms>` (`@timeouts`), one that kills the process (a V8 fatal error) as
//            `crash:<exit>` (`@crashes`), and the child restarts at the next call
//
// The generator is seeded and pure (no Date, no Math.random): the baseline arguments of a function are a seeded pick
// among the bank's plain-argument cases with the most arguments (`case`; functions without one get `2` per required
// argument), then each position p in 0..min(n, 6) (n = max(args, minArgs), plus the next optional position when
// maxArgs allows) is replaced by each trap value. The trap table is written in the capture (`@trap.<id>`), so the
// replay needs no generator.
//
// Records:
//   `<FN>`                 case=<bank id>|-  base=<JSON list of encodings>  n=<positions>
//   `<FN>#p<i>[@en-US]`    one facet per trap id: the canonical value text of the result (lib/common.mjs), or
//                          `throw:<message>`; `<id>.rnd` = the random values consumed (f64 hex, comma separated)
//   `<FN>#x<k>[@loc]`      (--big) pairs: `at=<i>,<j>` `t=<idA>,<idB>` `v=<value text>`
// Encodings (`@trap.*`, `base`): a value text (n/s/b/e/_), `R<ref>` (A1 reference or union on the mock sheet),
// `A<JSON rows of value texts>` (array literal; may be ragged), `O` (omitted argument).
// CI subset: positions 0..3, the CORE traps, pt-BR (plus the text traps in en-US). --big: every trap, both locales,
// positions 0..5, seeded pairs.
process.env.TZ = 'UTC';
const { readFileSync, readdirSync, mkdirSync } = await import('node:fs');
const { join, dirname } = await import('node:path');
const { CaptureWriter, Rng } = await import('../../uo-testkit/js/testkit.mjs');
const { fork } = await import('node:child_process');
const { fileURLToPath } = await import('node:url');
const IS_CHILD = process.env.UO_TRAPS_CHILD === '1';
const C = await import('./lib/common.mjs');

const a = C.args();
const root = C.tsRoot(a);
const { calc, testing } = await C.loadCalc(root);
const vt = C.makeValueText(calc);
const big = a.flag('--big');
const SEED = Number(a.opt('--seed', 0x7a9c0001)) >>> 0;
const PAIRS = Number(a.opt('--pairs', 24));
const TIMEOUT = Number(a.opt('--timeout', 5000));
const only = a.opt('--only') ? new Set(a.opt('--only').split(',').map((s) => s.toUpperCase())) : null;
const out = a.opt('--out', join(C.capturesDir, big ? 'calc-traps-big.capture' : 'calc-traps.capture'));
const { ERR } = calc;

// ---------------------------------------------------------------- trap table
const S = (s) => ({ kind: 's', v: s });
const N = (x) => ({ kind: 'n', v: x });
const TRAPS = [
  // number -> text switch points, signed zero, non-finite, integer precision, digit counts, subnormals
  ['nan', N(NaN)], ['nzero', N(-0)], ['zero', N(0)], ['one', N(1)], ['mone', N(-1)], ['half', N(0.5)], ['neg', N(-2.5)],
  ['inf', N(Infinity)], ['ninf', N(-Infinity)],
  ['e21', N(1e21)], ['e21m', N(999999999999999900000)], ['e21n', N(-1e21)], ['em7', N(1e-7)], ['em7b', N(1.5e-7)], ['em7m', N(9.99e-8)],
  ['p53m', N(2 ** 53 - 1)], ['p53', N(2 ** 53)], ['p53p', N(2 ** 53 + 2)], ['np53', N(-(2 ** 53) - 2)],
  ['d15', N(123456789012345)], ['d16', N(1234567890123456)], ['d17', N(0.30000000000000004)], ['d17b', N(12345678901234568)],
  ['d15f', N(0.123456789012345)], ['d17f', N(1.2345678901234567)],
  ['max', N(1.7976931348623157e308)], ['minnorm', N(2.2250738585072014e-308)], ['denorm', N(5e-324)], ['denorm2', N(1e-310)],
  ['big', N(1e15 + 0.5)], ['frac', N(2.675)], ['date', N(45658.75)], ['ndate', N(-1)], ['huge', N(1e100)],
  // strings: empty, surrogates, astral, combining, non-ASCII \w \b and case folding, white space
  ['empty', S('')], ['lone', S('\uD800')], ['lone2', S('a\uDC00b')], ['astral', S('\u{1D4B3}\u{1F600}x')], ['comb', S('éä')],
  ['nonascii', S('ÄÖÜß straße Ñandú')], ['turk', S('İıIi')], ['title', S('ǅǈ ﬃ')], ['greek', S('ΣΑΣ σας ς')], ['cjk', S('漢字かな')],
  ['ws', S(' 42 ')], ['spaces', S('   ')], ['nbsp', S(' 1 ')], ['tab', S('a\tb\nc')], ['long', S('x'.repeat(300))],
  ['quote', S('a"b\'c')], ['wild', S('a*?~b')], ['regex', S('(a|b)+$')],
  // text-numbers in pt-BR and en-US
  ['ptnum', S('1.234,56')], ['ptdec', S('1,5')], ['ptcur', S('R$ 10,00')], ['ptdate', S('12/03/2024')], ['ptpct', S('50%')], ['pttrue', S('VERDADEIRO')],
  ['ennum', S('1,234.56')], ['endec', S('1.5')], ['encur', S('$10.00')], ['endate', S('3/12/2024')], ['entrue', S('TRUE')],
  ['sci', S('1e5')], ['sciE', S('1E+21')], ['hexs', S('0x1A')], ['time', S('12:30')], ['negp', S('(5)')], ['plus', S('+3')],
  // booleans, empty, omitted
  ['true', { kind: 'b', v: true }], ['false', { kind: 'b', v: false }], ['null', { kind: '_' }], ['omit', { kind: 'O' }],
  // errors as arguments
  ...Object.entries(ERR).map(([k, e]) => [`err${k}`, { kind: 'e', v: e }]),
  // ranges on the mock sheet (trap cells in AX:AZ) and literal arrays
  ['rEmpty', { kind: 'R', v: 'AA1:AA3' }], ['rCell', { kind: 'R', v: 'AX1' }], ['rTextCell', { kind: 'R', v: 'AX2' }],
  ['rMixed', { kind: 'R', v: 'AX1:AX6' }], ['rRow', { kind: 'R', v: 'AX1:AZ1' }], ['rBlock', { kind: 'R', v: 'AX1:AZ3' }],
  ['rUnion', { kind: 'R', v: '(AX1:AX2,AY1:AY3)' }], ['rCol', { kind: 'R', v: 'AX:AX' }], ['rEmptyCol', { kind: 'R', v: 'BA:BA' }],
  ['a11', { kind: 'A', v: [[N(5)]] }], ['a22', { kind: 'A', v: [[N(1), S('a')], [{ kind: 'b', v: true }, { kind: '_' }]] }],
  ['aRagged', { kind: 'A', v: [[N(1), N(2)], [N(3)]] }], ['aEmpty', { kind: 'A', v: [[]] }], ['aErr', { kind: 'A', v: [[N(1), { kind: 'e', v: ERR.NA ?? Object.values(ERR)[0] }]] }],
];
/** trap cells of the mock sheet (AX1:AZ6) */
const TRAP_CELLS = { AX1: 7, AX2: 't', AX3: true, AX4: '#N/A', AX5: -0, AX6: '1,5', AY1: 2.5, AY2: '', AY3: 1e21, AZ1: '\uD800', AZ2: 'ß', AZ3: 0.1 };
const CORE = new Set(['nan', 'nzero', 'inf', 'ninf', 'e21', 'em7', 'p53p', 'd17', 'denorm', 'empty', 'lone', 'astral', 'comb', 'nonascii', 'turk',
  'ptnum', 'ptdec', 'ennum', 'endec', 'true', 'null', 'omit', 'errNA', 'errDIV0', 'errVALUE', 'rEmpty', 'rCell', 'rUnion', 'rCol', 'a22', 'aRagged']);
const TEXTISH = new Set(TRAPS.filter(([, t]) => t.kind === 's').map(([id]) => id));

const enc = (t) => {
  switch (t.kind) {
    case 'n': case 's': return vt(t.v);
    case 'b': return t.v ? 'b1' : 'b0';
    case '_': return '_';
    case 'O': return 'O';
    case 'e': return `e${t.v.code}`;
    case 'R': return `R${t.v}`;
    case 'A': return `A${JSON.stringify(t.v.map((row) => row.map(enc)))}`;
    default: throw new Error(`trap kind ${t.kind}`);
  }
};
const scalarOf = (t) => (t.kind === 'b' ? t.v : t.kind === '_' ? null : t.v);
/** trap -> TestArg of testing.ts */
const toArg = (t) => {
  switch (t.kind) {
    case 'O': return { omitted: true };
    case 'R': return { ref: t.v };
    case 'A': return t.v.map((row) => row.map(scalarOf));
    default: return scalarOf(t);
  }
};

// ---------------------------------------------------------------- baselines from the bank
const caseScalar = (x) => (x !== null && typeof x === 'object' ? ('e' in x ? calc.CalcError.of(x.e) : x.s) : x);
const fromCaseArg = (x) => {
  if (x !== null && typeof x === 'object') {
    if ('omit' in x) return { kind: 'O' };
    if ('ref' in x) return { kind: 'R', v: x.ref };
    if ('arr' in x) return { kind: 'A', v: x.arr.map((row) => row.map((y) => fromScalar(caseScalar(y)))) };
    if ('lambda' in x) return null; // not representable as a trap baseline
  }
  return fromScalar(caseScalar(x));
};
function fromScalar(v) {
  if (v === null) return { kind: '_' };
  if (typeof v === 'number') return N(v);
  if (typeof v === 'string') return S(v);
  if (typeof v === 'boolean') return { kind: 'b', v };
  return { kind: 'e', v };
}
const plain = new Map();
for (const f of readdirSync(C.oracleDir).filter((x) => x.endsWith('.cases.json')).sort()) {
  for (const c of JSON.parse(readFileSync(join(C.oracleDir, f), 'utf8'))) {
    const isF = c.f !== undefined || c.sheets || c.names || c.tables || c.legacy || c.date1904 || c.hiddenRows || c.filteredRows || c.formulas
      || Object.values(c.cells ?? {}).some((x) => x && typeof x === 'object' && 'f' in x);
    if (isF || c.args.some((x) => x && typeof x === 'object' && 'lambda' in x)) continue;
    const k = c.fn.toUpperCase();
    if (!plain.has(k)) plain.set(k, []);
    plain.get(k).push(c);
  }
}
const hash32 = (s) => { let h = 0x811c9dc5; for (let i = 0; i < s.length; i++) { h ^= s.charCodeAt(i); h = Math.imul(h, 0x01000193) >>> 0; } return h; };
function baseline(def) {
  const cases = plain.get(def.name) ?? [];
  if (cases.length) {
    const most = Math.max(...cases.map((c) => c.args.length));
    const pool = cases.filter((c) => c.args.length === most);
    const r = new Rng((hash32(def.name) ^ SEED) >>> 0 || 1);
    const c = pool[r.below(pool.length)];
    return { id: c.id, args: c.args.map(fromCaseArg), cells: c.cells ?? {} };
  }
  return { id: '-', args: Array.from({ length: def.minArgs }, () => N(2)), cells: {} };
}

// ---------------------------------------------------------------- the call (testing.ts runCase, plain path)
const LOC = { 'pt-BR': calc.PT_BR, 'en-US': calc.EN_US };
function call(def, argTraps, cells, loc, seed) {
  const sheet = {};
  for (const [k, v] of Object.entries({ ...TRAP_CELLS, ...cells })) {
    sheet[k] = typeof v === 'string' && /^#[A-Z/0-9!?_]+$/.test(v) && Object.values(ERR).some((e) => e.code === v) ? calc.CalcError.of(v) : caseScalar(v);
  }
  const ctx = testing.mockContext({ sheets: { Sheet1: sheet }, at: 'Z100', locale: LOC[loc], now: C.NOW_SERIAL, arrayContext: true });
  const rnd = new C.RecordedRandom(seed);
  ctx.random = () => rnd.next();
  let text;
  try {
    let v = testing.callFn(def.name, argTraps.map(toArg), ctx, true);
    if (v instanceof calc.RefValue) {
      const ar = v.single;
      if (!ar) v = ERR.VALUE;
      else if (ar.r1 === ar.r2 && ar.c1 === ar.c2) v = ctx.cell(ar.sheet, ar.r1, ar.c1);
      else v = ctx.values(v);
    }
    text = C.clip(vt(v));
  } catch (e) {
    text = `throw:${C.oneLine(e?.constructor?.name + ': ' + (e?.message ?? e))}`;
  }
  return { text, rnd: rnd.seq.length ? rnd.text() : null };
}

// ---------------------------------------------------------------- jobs (computed the same way in the main process and the child)
const traps = TRAPS.filter(([id]) => big || CORE.has(id));
const MAXPOS = big ? 6 : 4;
const fns = calc.allFns().filter((d) => !only || only.has(d.name));
/** The calls of one function: { b (baseline), npos, jobs: [{ key, facet, xs, loc, seed, pair? }] }. */
function jobsOf(def) {
  const b = baseline(def);
  if (b.args.some((x) => x === null)) return null;
  const n = Math.max(b.args.length, def.minArgs);
  const npos = Math.min(n + (n < def.maxArgs ? 1 : 0), MAXPOS);
  const full = (i) => { const xs = b.args.slice(); while (xs.length < n) xs.push(N(2)); if (i >= xs.length) xs.length = i; return xs; };
  const jobs = [];
  for (const loc of ['pt-BR', 'en-US']) {
    for (let p = 0; p < npos; p++) {
      const key = `${def.name}#p${p}${loc === 'pt-BR' ? '' : '@' + loc}`;
      for (const [id, t] of traps) {
        if (!big && loc !== 'pt-BR' && !TEXTISH.has(id)) continue;
        const xs = full(p);
        xs[p] = t;
        jobs.push({ key, facet: id, xs, loc, seed: hash32(`${key}|${id}`) ^ SEED });
      }
    }
    if (big && npos >= 2) {
      // seeded pairs: two positions trapped at once
      const r = new Rng((hash32(`${def.name}|pairs|${loc}`) ^ SEED) >>> 0 || 1);
      for (let k = 0; k < PAIRS; k++) {
        const i = r.below(npos);
        let j = r.below(npos - 1); if (j >= i) j++;
        const [ia, ta] = traps[r.below(traps.length)];
        const [ib, tb] = traps[r.below(traps.length)];
        const xs = full(Math.max(i, j));
        xs[i] = ta; xs[j] = tb;
        const key = `${def.name}#x${k}${loc === 'pt-BR' ? '' : '@' + loc}`;
        jobs.push({ key, facet: 'v', pair: { at: `${i},${j}`, t: `${ia},${ib}` }, xs, loc, seed: hash32(key) ^ SEED });
      }
    }
  }
  return { b, npos, jobs };
}

if (IS_CHILD) {
  // ---------------------------------------------------------------- child process: runs the jobs of one function from an index
  let cached = null;
  process.on('message', ({ fn, from }) => {
    const def = calc.lookupFn(fn);
    if (cached?.fn !== fn) cached = { fn, J: jobsOf(def) };
    const { b, jobs } = cached.J;
    for (let i = from; i < jobs.length; i++) {
      process.send({ i, start: true });
      const j = jobs[i];
      const r = call(def, j.xs, b.cells, j.loc, j.seed);
      process.send({ i, text: r.text, rnd: r.rnd });
    }
    process.send({ done: true });
  });
} else {
  // ---------------------------------------------------------------- main: the capture, with a watchdog per call
  const w = new CaptureWriter([
    'calc-traps (plan 4.3): TS callFn per function x parameter x trap value, see core/crates/uo-calc/js/capture-traps.mjs and core/docs/modules/uo-calc.md "Captures"',
  ]);
  C.stamp(w, root, { seed: `0x${SEED.toString(16)}`, mode: big ? 'big' : 'ci', 'random-source': 'xorshift32(seed of the record) / 2^32, recorded per call', 'call-timeout-ms': TIMEOUT });
  for (const [id, t] of traps) w.meta(`trap.${id}`, enc(t));
  w.meta('trap-cells', JSON.stringify(Object.fromEntries(Object.entries(TRAP_CELLS).map(([k, v]) => [k, typeof v === 'string' && v.startsWith('#') ? `e${v}` : vt(v)]))));
  const stats = { fns: 0, calls: 0, throws: 0, rnd: 0, timeouts: [], crashes: [] };
  const t0 = performance.now();
  let worker = null;
  const spawn = () => fork(fileURLToPath(import.meta.url), process.argv.slice(2), {
    env: { ...process.env, UO_TRAPS_CHILD: '1' }, execArgv: ['--max-old-space-size=6144'], serialization: 'advanced', stdio: ['ignore', 'ignore', 'ignore', 'ipc'],
  });
  /**
   * Runs the `n` jobs of `fn` in the child; resolves with the results by index. A call that does not return in
   * TIMEOUT ms, or that kills the child (V8 fatal errors such as "invalid table size" abort the process), is recorded
   * as `timeout:<ms>` / `crash:<exit>` and the child restarts at the next call.
   */
  const runFn = (fn, n) => new Promise((resolve) => {
    const res = new Array(n);
    const go = (from) => {
      if (from >= n) { resolve(res); return; }
      worker ??= spawn();
      const child = worker;
      let timer = null, current = from, settled = false;
      const stop = (text, list) => {
        if (settled) return;
        settled = true;
        clearTimeout(timer);
        child.off('message', onMsg);
        child.off('exit', onExit);
        child.kill();
        if (worker === child) worker = null;
        res[current] = { text };
        list.push(`${fn}[${current}]`);
        go(current + 1);
      };
      const onExit = (code, signal) => stop(`crash:${code ?? signal}`, stats.crashes);
      const onMsg = (m) => {
        if (m.done) { settled = true; clearTimeout(timer); child.off('message', onMsg); child.off('exit', onExit); resolve(res); return; }
        if (m.start) { current = m.i; clearTimeout(timer); timer = setTimeout(() => stop(`timeout:${TIMEOUT}ms`, stats.timeouts), TIMEOUT); return; }
        res[m.i] = m;
      };
      child.on('message', onMsg);
      child.on('exit', onExit);
      timer = setTimeout(() => stop(`timeout:${TIMEOUT}ms`, stats.timeouts), TIMEOUT + 30000); // start-up of a fresh child
      child.send({ fn, from });
    };
    go(0);
  });
  for (const def of fns) {
    stats.fns++;
    const tf = performance.now();
    const J = jobsOf(def);
    if (!J) continue;
    w.set(def.name, 'case', J.b.id);
    w.set(def.name, 'base', JSON.stringify(J.b.args.map(enc)));
    w.set(def.name, 'n', J.npos);
    const res = await runFn(def.name, J.jobs.length);
    J.jobs.forEach((j, i) => {
      const r = res[i];
      stats.calls++;
      if (r.text.startsWith('throw:')) stats.throws++;
      if (j.pair) { w.set(j.key, 'at', j.pair.at); w.set(j.key, 't', j.pair.t); }
      w.set(j.key, j.facet, r.text);
      if (r.rnd) { w.set(j.key, j.pair ? 'rnd' : `${j.facet}.rnd`, r.rnd.length > 4000 ? C.clip(r.rnd, 0) : r.rnd); stats.rnd++; }
    });
    const df = performance.now() - tf;
    if (process.env.UO_TRAPS_PROGRESS || df > 3000) console.log(`  ${def.name}: ${(df / 1000).toFixed(1)} s`);
  }
  worker?.kill();
  w.meta('stats', `functions=${stats.fns} calls=${stats.calls} throws=${stats.throws} random-records=${stats.rnd} timeouts=${stats.timeouts.length} crashes=${stats.crashes.length}`);
  if (stats.timeouts.length) w.meta('timeouts', stats.timeouts.join(' '));
  if (stats.crashes.length) w.meta('crashes', stats.crashes.join(' '));
  mkdirSync(dirname(out), { recursive: true });
  w.write(out);
  console.log(`calc-traps${big ? ' (big)' : ''}: ${stats.fns} functions, ${stats.calls} calls, ${stats.throws} threw, ${stats.timeouts.length} timed out, in ${((performance.now() - t0) / 1000).toFixed(1)} s -> ${out}`);
  if (stats.timeouts.length) console.log(`  timeouts (the TS call did not return in ${TIMEOUT} ms): ${stats.timeouts.join(' ')}`);
  if (stats.crashes.length) console.log(`  crashes (the TS call killed the process): ${stats.crashes.join(' ')}`);
}
