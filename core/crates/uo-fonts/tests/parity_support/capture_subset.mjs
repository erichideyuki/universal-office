// Captures the frozen TS `subsetTrueType` (shared/fonts/src/subset.ts) and `sfntToEot`
// (shared/fonts/src/eot.ts) over the generated cases of tests/frontier_d_support/subset_cases.rs
// and writes tests/parity/subset.cap, the file tests/subset_parity.rs replays. Digests only,
// never font bytes.
//
//   1. UO_FONTS_CASES_DIR=<empty dir> cargo test -p uo-fonts --test subset_parity dump_subset_cases
//   2. UO_TS_ROOT=<built checkout of main> node tests/parity_support/capture_subset.mjs --cases <dir>
//
// Each case runs in a worker thread watched by the main thread: the TS has no exit from the
// NaN-offset component loop of S2 (it never returns), so a case that is silent for 3 s is
// recorded as `hang` and the worker is restarted after it.
import { execSync } from 'node:child_process';
import { createHash } from 'node:crypto';
import { readFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { Worker, isMainThread, parentPort, workerData } from 'node:worker_threads';
import { CaptureWriter, importFrom, tsRoot } from '../../../uo-testkit/js/testkit.mjs';

const here = dirname(fileURLToPath(import.meta.url));
const sha = (u8) => createHash('sha256').update(u8).digest('hex').slice(0, 32);

function loadCases(dir) {
  const rows = readFileSync(join(dir, 'manifest.tsv'), 'utf8').split('\n').filter(Boolean);
  return rows.map((row) => {
    const [id, keep, gids, file] = row.split('\t');
    return { id, keepLayout: keep === '1', gids: gids ? gids.split(',').map(Number) : [], file };
  });
}

if (!isMainThread) {
  const { root, dir, start } = workerData;
  const { subsetTrueType } = await importFrom(root, 'shared/fonts/dist/subset.js');
  const { sfntToEot } = await importFrom(root, 'shared/fonts/dist/eot.js');
  const cases = loadCases(dir);
  for (let i = start; i < cases.length; i++) {
    const c = cases[i];
    parentPort.postMessage({ begin: i });
    const bytes = new Uint8Array(readFileSync(join(dir, c.file)));
    const eot = [false, true].map((subsetted) => {
      try {
        const e = sfntToEot(bytes, { subsetted });
        return `${e.length}:${sha(e)}`;
      } catch {
        return 'throw';
      }
    });
    parentPort.postMessage({ eot: i, values: eot });
    let result;
    try {
      const out = subsetTrueType(bytes, c.gids, { keepLayout: c.keepLayout });
      result = out === bytes ? 'same' : `out:${out.length}:${sha(out)}`;
    } catch (e) {
      result = `throw:${String(e).replace(/[\t\r\n=]/g, ' ')}`;
    }
    parentPort.postMessage({ done: i, result });
  }
  parentPort.postMessage({ end: true });
} else {
  const argv = process.argv.slice(2);
  const root = tsRoot(argv);
  const dir = argv[argv.indexOf('--cases') + 1];
  if (!dir) throw new Error('pass --cases <folder written by dump_subset_cases>');
  const out = argv.includes('--out') ? argv[argv.indexOf('--out') + 1] : join(here, '..', 'parity', 'subset.cap');
  const cases = loadCases(dir);
  const cap = new CaptureWriter(['shared/fonts/src/subset.ts subsetTrueType over tests/frontier_d_support/subset_cases.rs; digests only']);
  const results = new Array(cases.length);
  const eots = new Array(cases.length);
  let next = 0;
  while (next < cases.length) {
    await new Promise((resolve) => {
      const worker = new Worker(fileURLToPath(import.meta.url), { workerData: { root, dir, start: next } });
      let current = next;
      let last = Date.now();
      const tick = setInterval(() => {
        if (Date.now() - last > 3000) {
          clearInterval(tick);
          results[current] = 'hang';
          next = current + 1;
          worker.terminate().then(resolve);
        }
      }, 200);
      worker.on('message', (m) => {
        last = Date.now();
        if (m.begin !== undefined) current = m.begin;
        if (m.eot !== undefined) eots[m.eot] = m.values;
        if (m.done !== undefined) { results[m.done] = m.result; next = m.done + 1; }
        if (m.end) { clearInterval(tick); next = cases.length; worker.terminate().then(resolve); }
      });
      worker.on('error', (e) => { clearInterval(tick); console.error(e); process.exit(1); });
    });
  }
  let hangs = 0;
  cases.forEach((c, i) => {
    if (results[i] === 'hang') hangs++;
    const bytes = readFileSync(join(dir, c.file));
    cap.set(c.id, 'in', `${bytes.length}:${sha(bytes)}`);
    cap.set(c.id, 'subset', results[i]);
    cap.set(c.id, 'eot0', eots[i]?.[0] ?? 'missing');
    cap.set(c.id, 'eot1', eots[i]?.[1] ?? 'missing');
  });
  cap.meta('ts-source', 'shared/fonts/src/subset.ts');
  cap.meta('ts-commit', execSync('git rev-parse --short=8 HEAD', { cwd: root }).toString().trim());
  cap.write(out);
  console.log(`${out}: ${cases.length} cases, ${hangs} hang`);
}
