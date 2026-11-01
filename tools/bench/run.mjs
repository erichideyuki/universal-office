// Benchmarks main × rust-core (rust-port workflow, gate 3). The TS engine is measured from a
// checkout of `main` (frozen 0.9.x, built: npm install + build), the Rust and C# from this
// branch. Each suite prints a table; everything is saved in tools/bench/results/.
//
//   node tools/bench/run.mjs --main <path of a main checkout> [--suites fonts,lines,engine,opc] [--browser] [--tag <name>]
//
// Suites (more arrive with the modules — each module adds the one that measures it):
//   fonts   spike 1: parse 26 fonts and measure text with kerning (TS, C#, Rust native, Rust wasm)
//   lines   spike 2: replay of the TS line breaker over the Word oracle (time, CPU, memory)
//   engine  the whole TS layout of the site sample (the Rust one joins when layoutDocument exists)
//   opc     uo-opc (stage 5): open + read + parse of the 2,742 oracle docx and of giant-1m.xlsx,
//           inflate throughput, save of an edited package (TS, Rust native, Rust wasm)
//   browser (--browser) the editor in Chrome with the CPU slowed 1× and 4× (main's dist/site)
import { execFileSync } from 'node:child_process';
import { mkdirSync, writeFileSync, existsSync } from 'node:fs';
import { join, resolve, dirname } from 'node:path';
import { fileURLToPath } from 'node:url';

const here = dirname(fileURLToPath(import.meta.url));
const branch = resolve(here, '../..');
const args = process.argv.slice(2);
const opt = (k, d) => { const i = args.indexOf(k); return i >= 0 ? args[i + 1] : d; };
const main = opt('--main') && resolve(opt('--main'));
if (!main || !existsSync(join(main, 'write/packages/docx-layout/dist/index.js'))) {
  console.error('--main <path>: a checkout of main with the TS engine built (write/packages/*/dist)');
  process.exit(1);
}
const suites = opt('--suites', 'fonts,lines,engine').split(',').concat(args.includes('--browser') ? ['browser'] : []);
const env = { ...process.env, UO_TS_ROOT: main };
const spike = join(branch, 'experiments/rust-spike');
const sh = (cmd, a, cwd, extra = {}) => execFileSync(cmd, a, { cwd, env: { ...env, ...extra }, encoding: 'utf8', stdio: ['ignore', 'pipe', 'inherit'], shell: process.platform === 'win32', maxBuffer: 1 << 26 });
const json = (out) => JSON.parse(out.trim().split('\n').filter((l) => l.startsWith('{')).at(-1));
const sha = (dir) => execFileSync('git', ['rev-parse', '--short', 'HEAD'], { cwd: dir, encoding: 'utf8' }).trim();

// a run on uncommitted changes says so (results are named by the commit they were measured on)
const dirty = execFileSync('git', ['status', '--porcelain', '--untracked-files=no', '--', 'core', 'tools'], { cwd: branch, encoding: 'utf8' }).trim() !== '';
const report = { date: new Date().toISOString(), main: { sha: sha(main) }, branch: { sha: sha(branch), dirty }, suites: {} };
const tables = [];
const table = (title, head, rows) => {
  const line = (r) => `| ${r.join(' | ')} |`;
  const t = [`### ${title}`, '', line(head), line(head.map(() => '---')), ...rows.map(line), ''].join('\n');
  tables.push(t);
  console.log(`\n${t}`);
};

if (suites.includes('fonts')) {
  sh('node', ['prep.mjs'], spike);
  const ts = json(sh('node', ['bench-ts.mjs'], spike));
  sh('cargo', ['build', '--release', '--bin', 'bench'], spike);
  const rn = json(sh(join(spike, 'target/release/bench'), [], spike));
  sh('cargo', ['build', '--lib', '--target', 'wasm32-unknown-unknown', '--profile', 'wasm-fast'], spike);
  const rw = json(sh('node', ['bench-wasm.mjs', 'wasm-fast'], spike));
  const cs = json(sh('dotnet', ['run', '-c', 'Release', '--project', 'csharp/FontBench', '--', spike], spike));
  report.suites.fonts = { ts, csharpNative: cs, rustNative: rn, rustWasm: rw };
  table('Font core (26 fonts, 230k glyphs)', ['', 'TS (V8)', 'C# native', 'Rust native', 'Rust wasm'], [
    ['parse (ms)', ts.parseMs, cs.parseMs, rn.parseMs, rw.parseMs],
    ['measure (ms)', ts.measureMs, cs.measureMs, rn.measureMs, rw.measureMs],
    ['same totals as TS', '—', String(Number(cs.total) === Number(ts.total)), String(Number(rn.total) === Number(ts.total)), String(Number(rw.total) === Number(ts.total))],
  ]);
}

if (suites.includes('lines')) {
  sh('node', ['lines/capture.mjs'], spike);
  sh('node', ['lines/pack.mjs'], spike);
  const ts = json(sh('node', ['--expose-gc', 'lines/replay-ts.mjs', '30'], spike));
  sh('cargo', ['build', '--release', '--bin', 'lines'], spike);
  const rmem = json(sh(join(spike, 'target/release/lines'), ['30'], spike));
  sh('cargo', ['build', '--release', '--no-default-features', '--bin', 'lines'], spike);
  const rtime = json(sh(join(spike, 'target/release/lines'), ['30'], spike));
  sh('cargo', ['build', '--release', '--bin', 'lines'], spike);
  sh('cargo', ['build', '--lib', '--target', 'wasm32-unknown-unknown', '--profile', 'wasm-fast'], spike);
  const rw = json(sh('node', ['lines/replay-wasm.mjs', 'wasm-fast', '30'], spike));
  const cs = json(sh('dotnet', ['run', '-c', 'Release', '--project', 'csharp/LinesBench', '--', spike, '30'], spike));
  report.suites.lines = { ts, csharpNative: cs, rustNative: { ...rmem, bestMs: rtime.bestMs, medianMs: rtime.medianMs, cpuMsPerIter: rtime.cpuMsPerIter }, rustWasm: rw };
  table(`Line breaking (${ts.lines} lines, ${ts.glyphs} glyphs, Word oracle)`, ['', 'TS (V8)', 'C# native', 'Rust native', 'Rust wasm'], [
    ['mismatches vs TS', ts.mismatches, cs.mismatches, rmem.mismatches, rw.mismatches],
    ['best (ms)', ts.bestMs, cs.bestMs, rtime.bestMs, rw.bestMs],
    ['median (ms)', ts.medianMs, cs.medianMs, rtime.medianMs, rw.medianMs],
    ['CPU per pass (ms)', ts.cpuMsPerIter, cs.cpuMsPerIter, rtime.cpuMsPerIter, '—'],
    ['allocated per pass (MB)', ts.allocatedMB, cs.allocatedMB, rmem.allocatedMB, rw.allocatedMB],
    ['retained by the lines (MB)', ts.retainedMB, cs.retainedMB, rmem.retainedMB, rw.retainedMB],
  ]);
}

if (suites.includes('engine')) {
  const poc = join(branch, 'experiments/wasm-poc');
  const ts = json(sh('node', ['bench-ts.mjs'], poc));
  report.suites.engine = { ts };
  table('Whole layout of the site sample (8 pages)', ['', 'TS (V8)', 'Rust'], [
    ['read .docx (ms)', ts.readMs, '—'],
    ['first layout (ms)', ts.firstLayoutMs, '—'],
    ['warm layout, best (ms)', ts.warmBestMs, '—'],
    ['PDF (ms)', ts.pdfMs, '—'],
  ]);
}

if (suites.includes('browser')) {
  const perf = join(branch, 'experiments/perf');
  const ready = '(() => { const e = document.querySelector(\'universal-write\'); const c = e?.shadowRoot?.querySelectorAll(\'canvas.page\'); return !!c && c.length >= 8 && /de 8|of 8/.test(e.shadowRoot.textContent); })()';
  sh('node', ['harness.mjs', 'site:/live.html', 'bench-editor', '--site', join(main, 'dist/site'), '--rates', '1,4', '--runs', '3', '--type', '--ready', ready], perf);
  const r = (await import(`file:///${join(perf, 'out/bench-editor.json').replace(/\\/g, '/')}`, { with: { type: 'json' } })).default.byRate;
  report.suites.browser = r;
  table('Editor in Chrome (main dist/site)', ['', '1×', '4×'], [
    ['transferred (MB)', r['1x'].load.bytesMB, r['4x'].load.bytesMB],
    ['interactive (s)', (r['1x'].load.tti / 1000).toFixed(1), (r['4x'].load.tti / 1000).toFixed(1)],
    ['blocking (s)', (r['1x'].load.tbt / 1000).toFixed(2), (r['4x'].load.tbt / 1000).toFixed(2)],
    ['JS heap (MB)', r['1x'].memory.jsHeapMB, r['4x'].memory.jsHeapMB],
    ['keys over 50 ms', r['1x'].typing.over50, r['4x'].typing.over50],
  ]);
}

if (suites.includes('opc')) {
  const core = join(branch, 'core');
  const runs = '5';
  const ts = json(sh('node', ['--expose-gc', join(here, 'opc/ts.mjs'), runs], branch));
  sh('cargo', ['build', '--release', '-p', 'uo-opc', '--example', 'opc_bench'], core);
  const rn = json(sh(join(core, 'target/release/examples/opc_bench'), [runs], core));
  sh('cargo', ['build', '-p', 'uo-wasm', '--features', 'opc-bench', '--target', 'wasm32-unknown-unknown', '--profile', 'wasm'], core);
  const rw = json(sh('node', [join(here, 'opc/wasm.mjs'), runs], branch));
  report.suites.opc = { ts, rustNative: rn, rustWasm: rw };
  const mb = (b) => (b / 1e6).toFixed(1);
  const rows = (s, extra = []) => [
    ['best (ms)', ts[s].ms, rn[s].ms, rw[s].ms],
    ['allocated (MB)', '—', mb(rn[s].allocated), mb(rw[s].allocated)],
    ['peak (MB; TS: max RSS growth)', mb(ts[s].peakRss), mb(rn[s].peak), mb(rw[s].peak)],
    ['kept, outputs alive (MB)', mb(ts[s].kept), mb(rn[s].kept), mb(rw[s].kept)],
    ['wasm memory growth (MB)', '—', '—', mb(rw[s].memoryGrowth)],
    ...extra,
  ];
  const head = ['', 'TS (V8)', 'Rust native', 'Rust wasm'];
  const sum = (t) => t.parts + t.elements + t.bytes;
  const same = (s) => ['same totals as TS', '—', String(sum(rn[s]) === sum(ts[s])), String(rw[s].checksum === sum(ts[s]))];
  table(`uo-opc: open + read + parse of the oracle (${ts.corpus.docs} docx, ${ts.corpus.parts} parts, ${ts.corpus.elements} elements; Write preset with positions; memory per document)`, head, rows('corpus', [same('corpus')]));
  table(`uo-opc: open + read + parse of giant-1m.xlsx (${ts.giant.elements} elements; Sheet presets)`, head, rows('giant', [same('giant')]));
  const mbs = (r) => (ts.inflate.bytes / 1e6 / (r.ms / 1000)).toFixed(0);
  table(`uo-opc: inflate of giant-1m.xlsx sheet1.xml (${mb(ts.inflate.bytes)} MB out)`, head, rows('inflate', [['MB/s', mbs(ts.inflate), mbs(rn.inflate), mbs(rw.inflate)]]));
  table('uo-opc: edit + save of sw-16-8.docx (TS deflates every part, Rust copies the untouched ones raw)', head, rows('saveDocx', [['output (B)', ts.saveDocx.bytes, rn.saveDocx.bytes, rw.saveDocx.checksum]]));
  table('uo-opc: edit + save of giant-1m.xlsx with sheet1.xml edited (deflated again)', head, rows('saveGiant', [['output (B)', ts.saveGiant.bytes, rn.saveGiant.bytes, rw.saveGiant.checksum]]));
  table('uo-opc: wasm module (feature opc-bench)', ['', 'bytes'], [['uo_wasm.wasm', rw.wasmBytes]]);
}

const dir = join(here, 'results');
mkdirSync(dir, { recursive: true });
const tag = opt('--tag');
const name = `${report.date.slice(0, 10)}-${report.branch.sha}${dirty ? '-dirty' : ''}${tag ? `-${tag}` : ''}`;
writeFileSync(join(dir, `${name}.json`), JSON.stringify(report, null, 1));
writeFileSync(join(dir, `${name}.md`), [`# Benchmarks ${report.date.slice(0, 10)}`, '', `main ${report.main.sha} × rust-core ${report.branch.sha}${dirty ? ' (with uncommitted changes)' : ''}`, '', ...tables].join('\n'));
console.log(`\nresults: tools/bench/results/${name}.md`);
