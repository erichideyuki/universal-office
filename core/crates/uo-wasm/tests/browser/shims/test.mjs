// `node:test` for the browser runner: tests are collected in order and run one after the other
// by `run()`; `{ skip }` is honoured (a string is the reason).
const tests = [];

export function test(name, options, fn) {
  if (typeof options === 'function') [fn, options] = [options, {}];
  tests.push({ name, fn, skip: options?.skip });
}

export async function run() {
  const results = [];
  for (const t of tests) {
    if (t.skip) {
      results.push({ name: t.name, status: 'skip', reason: typeof t.skip === 'string' ? t.skip : '' });
      continue;
    }
    const start = performance.now();
    try {
      await t.fn();
      results.push({ name: t.name, status: 'pass', ms: Math.round(performance.now() - start) });
    } catch (e) {
      results.push({ name: t.name, status: 'fail', error: String(e?.stack ?? e) });
    }
  }
  return results;
}
