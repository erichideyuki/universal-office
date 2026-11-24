// Speed + memory benchmark, `sheet` suite (plan section 16 item 14; records core/docs/modules/uo-calc.md "Performance
// budgets" and uo-xlsx.md T13): the baseline the Rust frontiers must beat. Today it measures the frozen TS only
// (engine `ts`, Node); the rust-core engines (native, wasm) join here as the frontiers land (C1a open, A3 recalc,
// C5 edit), on the same workbooks and with the same scenario names.
//
// Every scenario runs in its own child process (`node --expose-gc`), one untimed warm-up then --rounds measured
// rounds (median and min reported); memory = heap used after a forced GC with the result alive, minus the heap
// before the load (retained), and the process RSS after the load.
//   open-core   sheet-core dist: openSpreadsheet + readWorkbook + readSharedStrings + readStyles + every readWorksheet
//   open-el     the <universal-sheet> element (headless, core/crates/uo-xlsx/js/harness) loadXlsx: the product open
//               (cell index, geometry, open-recalc rule)
//   recalc      calc dist: fromSheetCore + new Engine (build) and Engine.recalc() (full) on the generated calc classes of
//               uo-calc.md (gen-workbook.mjs CALC_CLASSES: chain-100k, sum-ranges-100k, lookup-10kx10k, text-50k,
//               dynamic-1k, volatile-10k, cycles-iterate, shared-1m) and a mixed formula-heavy workbook (buildCalcHeavy 20k)
//   edit        the element: setActiveCellText latency, (a) a cell at the head of a 20k-cell chain (dirties every
//               chained formula) and (b) an isolated cell; p50 / p95 over --edits edits each
//   save        the element: one edit then saveXlsx (surgical save of the edited sheet)
//
//   export UO_TS_ROOT=<frozen TS checkout, dist built>
//   node core/tools/bench/speed/sheet.mjs [--rounds 3] [--edits 20] [--only open-core,open-el,recalc,edit,save] [--tag <name>]
// Results: core/tools/bench/speed/results/sheet-<tag>.{json,md}.
import { execFileSync, spawnSync } from 'node:child_process';
import { existsSync, mkdirSync, readFileSync, writeFileSync } from 'node:fs';
import { join, resolve, dirname } from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';

const here = dirname(fileURLToPath(import.meta.url));
const core = resolve(here, '../../..');
const repo = resolve(core, '..');
const argv = process.argv.slice(2);
const opt = (k, d) => { const i = argv.indexOf(k); return i >= 0 ? argv[i + 1] : d; };
const root = resolve(process.env.UO_TS_ROOT || 'D:/Projects/OfficeNext/.claude/worktrees/ts-ref-092');
const work = join(core, 'target', 'sheet-cap', 'bench');
mkdirSync(work, { recursive: true });
const median = (xs) => { const s = [...xs].sort((a, b) => a - b); return s.length % 2 ? s[s.length >> 1] : (s[s.length / 2 - 1] + s[s.length / 2]) / 2; };
const pct = (xs, p) => { const s = [...xs].sort((a, b) => a - b); return s[Math.min(s.length - 1, Math.floor(p * s.length))]; };
const MB = (b) => b / 1048576;

// ── child: one scenario ─────────────────────────────────────────────────────────────────────────
if (argv[0] === '--child') {
  const [, scenario, file, roundsS, editsS] = argv;
  const rounds = Number(roundsS), edits = Number(editsS);
  const bytes = new Uint8Array(readFileSync(file));
  const gc = () => { for (let i = 0; i < 3; i++) globalThis.gc(); };
  const ms = (f) => { const t = performance.now(); const r = f(); return [performance.now() - t, r]; };
  const out = { scenario, file: file.replace(/\\/g, '/').split('/').pop(), bytes: bytes.length, rounds };
  if (scenario === 'open-core' || scenario === 'recalc') {
    const SC = await import(pathToFileURL(join(root, 'sheet/packages/sheet-core/dist/index.js')).href);
    const calc = await import(pathToFileURL(join(root, 'sheet/packages/calc/dist/index.js')).href);
    const read = () => {
      const pkg = SC.openSpreadsheet(bytes);
      const workbook = SC.readWorkbook(pkg);
      const cx = SC.colorContext(pkg);
      const sharedStrings = SC.readSharedStrings(pkg, cx);
      const styles = SC.readStyles(pkg, cx);
      const worksheets = workbook.sheets.map((s) => (s.target ? SC.readWorksheet(pkg, s.target, cx) : undefined));
      return { pkg, workbook, sharedStrings, styles, worksheets };
    };
    if (scenario === 'open-core') {
      read();
      const times = [];
      let keep;
      gc();
      const base = process.memoryUsage().heapUsed;
      for (let r = 0; r < rounds; r++) { keep = undefined; gc(); const [t, m] = ms(read); times.push(t); keep = m; }
      gc();
      out.ms = { median: median(times), min: Math.min(...times), all: times };
      out.retainedMB = MB(process.memoryUsage().heapUsed - base);
      out.rssMB = MB(process.memoryUsage().rss);
      out.cells = keep.worksheets.reduce((n, ws) => n + (ws?.rows.reduce((k, row) => k + row.cells.length, 0) ?? 0), 0);
    } else {
      const m = read();
      const build = () => new calc.Engine(calc.fromSheetCore({ workbook: m.workbook, worksheets: m.worksheets, sharedStrings: m.sharedStrings }), { locale: calc.PT_BR, now: () => 45658.5, random: () => 0.5 });
      { const e = build(); e.recalc(); }
      const tb = [], tr = [];
      let keep;
      gc();
      const base = process.memoryUsage().heapUsed;
      for (let r = 0; r < rounds; r++) {
        keep = undefined; gc();
        const [t1, e] = ms(build); tb.push(t1);
        const [t2] = ms(() => e.recalc()); tr.push(t2);
        keep = e;
      }
      gc();
      out.buildMs = { median: median(tb), min: Math.min(...tb), all: tb };
      out.recalcMs = { median: median(tr), min: Math.min(...tr), all: tr };
      out.engineRetainedMB = MB(process.memoryUsage().heapUsed - base);
      out.formulas = keep.allFormulas().length;
      out.rssMB = MB(process.memoryUsage().rss);
    }
  } else {
    const { loadHarness } = await import(pathToFileURL(join(core, 'crates/uo-xlsx/js/harness/build-harness.mjs')).href);
    const h = await loadHarness(root);
    const open = () => { const el = h.newSheet({ 'formula-locale': 'pt-BR' }); el.loadXlsx(bytes); return el; };
    if (scenario === 'open-el') {
      open().remove();
      const times = [];
      let keep;
      gc();
      const base = process.memoryUsage().heapUsed;
      for (let r = 0; r < rounds; r++) { keep?.remove(); keep = undefined; gc(); const [t, el] = ms(open); times.push(t); keep = el; }
      gc();
      out.ms = { median: median(times), min: Math.min(...times), all: times };
      out.retainedMB = MB(process.memoryUsage().heapUsed - base);
      out.rssMB = MB(process.memoryUsage().rss);
      out.engine = keep.calc?.engine ? 'built' : 'none';
    } else if (scenario === 'edit') {
      const el = open();
      el.recalculate(); // the engine exists (an editor session after the first edit)
      const lat = (ref, texts) => texts.map((t) => { el.selectRange(ref); const [d] = ms(() => el.setActiveCellText(t)); return d; });
      lat('A2', ['7', '8']); lat('Z1', ['1', '2']); // warm-up
      const chain = lat('A2', Array.from({ length: edits }, (_, i) => String(i + 3)));
      const iso = lat('Z1', Array.from({ length: edits }, (_, i) => String(i + 3)));
      out.chainMs = { p50: pct(chain, 0.5), p95: pct(chain, 0.95), min: Math.min(...chain), n: chain.length };
      out.isolatedMs = { p50: pct(iso, 0.5), p95: pct(iso, 0.95), min: Math.min(...iso), n: iso.length };
      out.rssMB = MB(process.memoryUsage().rss);
    } else if (scenario === 'save') {
      const el = open();
      el.selectRange('B2'); el.setActiveCellText('123');
      el.saveXlsx();
      const times = [];
      let len = 0;
      for (let r = 0; r < rounds; r++) { el.selectRange('B2'); el.setActiveCellText(String(200 + r)); const [t, b] = ms(() => el.saveXlsx()); times.push(t); len = b.length; }
      out.ms = { median: median(times), min: Math.min(...times), all: times };
      out.outBytes = len;
      out.rssMB = MB(process.memoryUsage().rss);
    } else throw new Error(`unknown scenario ${scenario}`);
  }
  process.stdout.write(JSON.stringify(out) + '\n');
  process.exit(0);
}

// ── orchestrator ────────────────────────────────────────────────────────────────────────────────
const rounds = Number(opt('--rounds', 3));
const edits = Number(opt('--edits', 20));
const only = new Set(opt('--only', 'open-core,open-el,recalc,edit,save').split(','));
const tag = opt('--tag', `ts-${new Date().toISOString().slice(0, 10)}`);
const { buildCalcHeavy, CALC_CLASSES } = await import(pathToFileURL(join(core, 'crates/uo-xlsx/js/lib/gen-workbook.mjs')).href);
const heavy = {};
for (const rows of [20000]) {
  const p = join(work, `calc-heavy-${rows}.xlsx`);
  if (!existsSync(p)) writeFileSync(p, buildCalcHeavy(rows));
  heavy[rows] = p;
}
const classes = (opt('--classes', Object.keys(CALC_CLASSES).join(','))).split(',').map((name) => {
  const p = join(work, `${name}.xlsx`);
  if (!existsSync(p)) writeFileSync(p, CALC_CLASSES[name]());
  return p;
});
const giant1m = join(repo, 'sheet/samples/giant-1m.xlsx');
const giant200k = join(repo, 'sheet/samples/giant-200k.xlsx');
const plan = [
  ['open-core', giant1m], ['open-core', giant200k], ['open-core', heavy[20000]],
  ['open-el', giant1m], ['open-el', giant200k], ['open-el', heavy[20000]],
  ['recalc', heavy[20000]], ...classes.map((p) => ['recalc', p]),
  ['edit', heavy[20000]],
  ['save', giant1m], ['save', giant200k],
].filter(([s]) => only.has(s));
// build the harness once (the children reuse it)
execFileSync(process.execPath, [join(core, 'crates/uo-xlsx/js/harness/build-harness.mjs'), '--ts-root', root], { stdio: 'inherit' });
const results = [];
for (const [s, f] of plan) {
  const r = spawnSync(process.execPath, ['--expose-gc', '--max-old-space-size=8192', fileURLToPath(import.meta.url), '--child', s, f, String(rounds), String(edits)], { encoding: 'utf8', env: { ...process.env, UO_TS_ROOT: root }, maxBuffer: 1 << 24 });
  if (r.status !== 0) { console.error(`${s} ${f}: failed\n${r.stderr}`); continue; }
  const line = r.stdout.trim().split('\n').pop();
  const x = JSON.parse(line);
  results.push(x);
  console.error(`${s.padEnd(10)} ${x.file.padEnd(24)} ${JSON.stringify(Object.fromEntries(Object.entries(x).filter(([k]) => !['scenario', 'file', 'rounds'].includes(k)).map(([k, v]) => [k, typeof v === 'object' ? (v.median ?? v.p50) : v])))}`);
}
const commit = (() => { try { return execFileSync('git', ['-C', root, 'rev-parse', '--short', 'HEAD'], { encoding: 'utf8' }).trim(); } catch { return 'unknown'; } })();
const report = { date: new Date().toISOString(), node: process.version, tsCommit: commit, engine: 'ts', rounds, edits, results };
const outDir = join(here, 'results');
mkdirSync(outDir, { recursive: true });
writeFileSync(join(outDir, `sheet-${tag}.json`), JSON.stringify(report, null, 1));
const f1 = (x) => (x === undefined ? '-' : x >= 100 ? x.toFixed(0) : x.toFixed(1));
const md = [`# Sheet suite ${tag} (frozen TS ${commit}, Node ${process.version}, ${rounds} rounds)`, '',
  '| scenario | workbook | time ms (median) | min | other | retained MB | RSS MB |', '|---|---|---|---|---|---|---|'];
for (const x of results) {
  const t = x.ms ?? x.recalcMs ?? x.chainMs;
  const other = x.buildMs ? `build ${f1(x.buildMs.median)} ms, ${x.formulas} formulas` : x.chainMs ? `chain p50 ${f1(x.chainMs.p50)} / p95 ${f1(x.chainMs.p95)}; isolated p50 ${f1(x.isolatedMs.p50)} / p95 ${f1(x.isolatedMs.p95)}` : x.cells !== undefined ? `${x.cells} cells` : x.engine ? `engine ${x.engine}` : x.outBytes ? `${x.outBytes} B out` : '';
  md.push(`| ${x.scenario} | ${x.file} | ${f1(t?.median ?? t?.p50)} | ${f1(t?.min)} | ${other} | ${f1(x.retainedMB ?? x.engineRetainedMB)} | ${f1(x.rssMB)} |`);
}
writeFileSync(join(outDir, `sheet-${tag}.md`), md.join('\n') + '\n');
console.log(md.join('\n'));
