// Parity capture group `calc-tests` (plan section 4.3, SKILL.md "Capture sources" 4): the frozen TS's own tests that
// exercise the calc, run against the frozen sources with the public entry points wrapped by a recorder
// (calc-tests/: every `export function` of calc/src — evaluate, parse/print, applyFn/callFn, writeValues,
// fromSheetCore, open-recalc, save, i18n… — and the public methods of Engine, SheetCalc and XfPool), keeping every
// outermost call of a test with its arguments and result.
// Files: sheet/packages/calc/test/*.test.ts except oracle.test.ts (the `bank` group replays it case by case) +
// sheet/packages/sheet/test/{sheet-calc,style-pool,entry}.test.ts = 19 files.
//
//   node core/crates/uo-calc/js/capture-tests.mjs                 # CI -> tests/captures/calc-tests.{capture,calls.gz}
//   node core/crates/uo-calc/js/capture-tests.mjs --big --out core/target/sheet-cap/calc-tests-big.capture
//   options: --ts-root <dir>  --keep-raw (leave the per-file JSONL in core/target/sheet-cap/calc-tests-raw)
//
// Records `<test file>#<k>` (k = call index inside the file, after filtering), facets:
//   fn    `<module>:<function>` | `<module>:<Class>.new` | `<module>:<Class>#<method>`
//   test  index of the vitest test name (`@test.<i>` meta, a JSON string), `self` the receiver id (`#<k>` of the call that created it) or -
//   in    `len:sha256[0..16]` of the canonical arguments JSON, `ret` the canonical result JSON (clipped to
//         `#len:sha` past 160 units) or `throw:<Name>: <message>`;  `rnd` (when any) the Math.random values the call
//         consumed (count:digest; values in the sidecar). Host clock frozen at 2025-01-15 12:00 UTC, Math.random a seeded
//         xorshift32 reseeded per file (calc-tests/recorder-setup.mjs).
// Sidecar `.calls.gz`: the kept calls as JSON lines (the full arguments and results; canonical form in
// calc-tests/recorder-setup.mjs), what a replay feeds to the Rust entry points.
// Filtered: the registration calls made while modules load (register, def, numFn, atpFn, registerLanguage,
// setComputedCriterion), and in the CI subset pure calls beyond 200 per (test, function) (stride sample; --big
// keeps all). Calls made inside a recorded call are never kept (depth 0 only).
process.env.TZ = 'UTC';
const { execFileSync } = await import('node:child_process');
const { existsSync, mkdirSync, readdirSync, readFileSync, renameSync, writeFileSync } = await import('node:fs');
const { join, dirname } = await import('node:path');
const { gzipSync } = await import('node:zlib');
const { CaptureWriter } = await import('../../uo-testkit/js/testkit.mjs');
const C = await import('./lib/common.mjs');

const a = C.args();
const root = C.tsRoot(a);
const big = a.flag('--big');
const out = a.opt('--out', join(C.capturesDir, 'calc-tests.capture'));
const raw = join(C.bigDir, big ? 'calc-tests-raw-big' : 'calc-tests-raw');
const CAP = big ? Infinity : 200;
const REGISTRATION = /:(register\w*|def|numFn|atpFn|setComputedCriterion)$/;

// a fresh raw dir (renamed aside, never rm -rf: see the repo rules on junctions)
if (existsSync(raw)) renameSync(raw, `${raw}-old-${Date.now()}`);
mkdirSync(raw, { recursive: true });
const vitest = join(root, 'sheet', 'node_modules', 'vitest', 'vitest.mjs');
const config = join(C.crateDir, 'js', 'calc-tests', 'vitest.config.mjs');
const t0 = performance.now();
let summary = '';
try {
  summary = execFileSync(process.execPath, [vitest, 'run', '--config', config, '--reporter', 'dot'], {
    env: { ...process.env, UO_TS_ROOT: root, UO_REC_DIR: raw, TZ: 'UTC', FORCE_COLOR: '0', NO_COLOR: '1' },
    encoding: 'utf8', maxBuffer: 1 << 26,
  });
} catch (e) {
  summary = `${e.stdout ?? ''}\n${e.stderr ?? ''}`;
  console.error('vitest reported failures (the capture keeps what ran):');
}
const tests = /Tests\s+(.*)/.exec(summary)?.[1]?.trim() ?? '?';
const filesLine = /Test Files\s+(.*)/.exec(summary)?.[1]?.trim() ?? '?';
console.log(`vitest: files ${filesLine}; tests ${tests} (${((performance.now() - t0) / 1000).toFixed(1)} s)`);

const w = new CaptureWriter([
  'calc-tests (plan 4.3): calls of the TS calc tests through the recorded entry points, see core/crates/uo-calc/js/capture-tests.mjs and core/docs/modules/uo-calc.md "Captures"',
]);
C.stamp(w, root, { mode: big ? 'big' : 'ci', 'vitest-files': filesLine, 'vitest-tests': tests, cap: String(CAP) });
const kept = [];
const testIds = new Map();
const stats = { files: 0, calls: 0, kept: 0, registration: 0, capped: 0, throws: 0 };
for (const f of readdirSync(raw).filter((x) => x.endsWith('.jsonl')).sort()) {
  stats.files++;
  const file = f.replace(/\.jsonl$/, '').replace(/__/g, '/');
  const lines = readFileSync(join(raw, f), 'utf8').split('\n').filter(Boolean).map((l) => JSON.parse(l));
  stats.calls += lines.length;
  // pure calls (no receiver, no object argument) per (test, fn), for the stride cap
  const pure = (l) => l.self === null && !l.fn.endsWith('.new') && !JSON.stringify(l.args).includes('"$obj"');
  const count = new Map();
  for (const l of lines) if (pure(l)) { const key = `${l.t}\u0000${l.fn}`; count.set(key, (count.get(key) ?? 0) + 1); }
  const seen = new Map();
  let n = 0;
  for (const l of lines) {
    if (REGISTRATION.test(l.fn)) { stats.registration++; continue; }
    if (pure(l)) {
      const key = `${l.t}\u0000${l.fn}`;
      const total = count.get(key);
      const i = seen.get(key) ?? 0;
      seen.set(key, i + 1);
      if (total > CAP && Math.floor((i * CAP) / total) === Math.floor(((i - 1) * CAP) / total) && i !== 0) { stats.capped++; continue; }
    }
    const key = `${file}#${n++}`;
    const argsJson = JSON.stringify(l.args);
    w.set(key, 'fn', l.fn);
    if (!testIds.has(l.t)) { testIds.set(l.t, testIds.size); w.meta(`test.${testIds.size - 1}`, JSON.stringify(l.t)); }
    w.set(key, 'test', testIds.get(l.t));
    w.set(key, 'self', l.self ?? '-');
    w.set(key, 'in', C.digest(argsJson).slice(0, -16));
    if (l.throw !== undefined) { w.set(key, 'ret', `throw:${C.oneLine(l.throw, 200)}`); stats.throws++; }
    else w.set(key, 'ret', C.clip(JSON.stringify(l.ret), 160));
    if (l.rnd) w.set(key, 'rnd', `${l.rnd.length}:${C.sha(l.rnd.join(',')).slice(0, 32)}`);
    kept.push(JSON.stringify({ key, ...l }));
    stats.kept++;
  }
}
w.meta('stats', Object.entries(stats).map(([k, v]) => `${k}=${v}`).join(' '));
mkdirSync(dirname(out), { recursive: true });
w.write(out);
writeFileSync(out.replace(/\.capture$/, '.calls.gz'), gzipSync(Buffer.from(kept.join('\n') + '\n', 'utf8'), { level: 9 }));
console.log(`calc-tests${big ? ' (big)' : ''}: ${Object.entries(stats).map(([k, v]) => `${k} ${v}`).join(', ')} -> ${out}`);
