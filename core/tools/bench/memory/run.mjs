// Memory benchmark main x rust-core, orchestrator (record core/docs/modules/uo-docx-layout.md,
// "Benchmarks main x rust-core - memory (measured)"). ONE measurement process at a time, never overlapping.
//
//   export UO_TS_ROOT=<frozen TS checkout, dist built>      (d0f45bc5: write/packages/docx-layout, docx-core)
//   export CARGO_TARGET_DIR=<own target dir>
//   node core/tools/bench/memory/run.mjs [--classes small,typical,tables,images,pages100,pages500]
//        [--no-build] [--skip ts,native,wasm,phone] [--out <dir>] [--tag <name>]
//   UO_MEMBENCH_CORPUS=<n>  also runs <n> documents of the Word oracle corpus (every k-th, sorted; default off)
//
// Needs: cargo (native example `bench_memory` + the wasm module with feature count,write), node >= 22.
import { execFileSync } from 'node:child_process';
import { existsSync, mkdirSync, writeFileSync, readdirSync, statSync, copyFileSync } from 'node:fs';
import { join, resolve, dirname, relative } from 'node:path';
import { fileURLToPath } from 'node:url';
import { buildDocx, CLASSES, loadFflate } from './gen_docs.mjs';

const here = dirname(fileURLToPath(import.meta.url));
const core = resolve(here, '../../..');
const args = process.argv.slice(2);
const opt = (k, d) => { const i = args.indexOf(k); return i >= 0 ? args[i + 1] : d; };
const root = process.env.UO_TS_ROOT && resolve(process.env.UO_TS_ROOT);
if (!root || !existsSync(join(root, 'write/packages/docx-layout/dist/index.js'))) throw new Error('set UO_TS_ROOT to the frozen TS checkout (dist built)');
const target = resolve(process.env.CARGO_TARGET_DIR ?? join(core, 'target/agent-membench'));
const skip = new Set((opt('--skip', '') || '').split(',').filter(Boolean));
const classes = (opt('--classes', 'small,typical,tables,images,pages100,pages500')).split(',');
const outDir = resolve(opt('--out', join(here, 'results')));
const tag = opt('--tag', new Date().toISOString().slice(0, 10));
const docsDir = join(target, 'docs');
mkdirSync(docsDir, { recursive: true });
mkdirSync(outDir, { recursive: true });
const env = { ...process.env, UO_TS_ROOT: root, CARGO_TARGET_DIR: target };
const exe = process.platform === 'win32' ? '.exe' : '';

const sh = (cmd, a, extra = {}) => execFileSync(cmd, a, { cwd: core, env, encoding: 'utf8', stdio: ['ignore', 'pipe', 'inherit'], maxBuffer: 1 << 26, ...extra });
const json = (out) => JSON.parse(out.trim().split('\n').filter((l) => l.startsWith('{')).at(-1));

if (!args.includes('--no-build')) {
  console.error('building the native example and the wasm module (cargo -j 4) ...');
  sh('cargo', ['build', '--release', '-j', '4', '-p', 'uo-docx-layout', '--example', 'bench_memory'], { stdio: 'inherit' });
  sh('cargo', ['build', '-p', 'uo-wasm', '--features', 'count,write', '--target', 'wasm32-unknown-unknown', '--profile', 'wasm', '-j', '4'], { stdio: 'inherit' });
  copyFileSync(join(target, 'wasm32-unknown-unknown/wasm/uo_wasm.wasm'), join(target, 'uo_wasm_count.wasm'));
}
const nativeExe = join(target, 'release/examples/bench_memory' + exe);
const wasmCount = join(target, 'uo_wasm_count.wasm');

// the documents
const files = {};
const fflate = await loadFflate(root);
for (const c of classes) {
  if (c === 'small') files[c] = join(core, 'crates/uo-docx-layout/tests/captures/f8/docs/f8__7__0.docx');
  else if (CLASSES[c]) {
    files[c] = join(docsDir, `${c}.docx`);
    writeFileSync(files[c], buildDocx({ fflate, ...CLASSES[c] }));
  } else throw new Error(`unknown class ${c}`);
}
const corpusN = Number(process.env.UO_MEMBENCH_CORPUS ?? 0);
if (corpusN > 0) {
  const dir = join(root, 'write/packages/docx-layout/test/oracle');
  const all = [];
  const go = (d) => { for (const n of readdirSync(d).sort()) { const p = join(d, n); if (statSync(p).isDirectory()) go(p); else if (/\.docx$/i.test(p)) all.push(p); } };
  go(dir);
  const k = Math.max(1, Math.floor(all.length / corpusN));
  all.filter((_, i) => i % k === 0).slice(0, corpusN).forEach((p) => { files[`corpus:${relative(dir, p).split('\\').join('/')}`] = p; });
}

const results = { date: new Date().toISOString(), node: process.version, ts_root: root, classes: {} };
const flags = ['--expose-gc'];
for (const [name, file] of Object.entries(files)) {
  const r = { file };
  console.error(`== ${name}`);
  try {
    if (!skip.has('ts')) r.ts = json(sh('node', [...flags, join(here, 'ts_mem.mjs'), file]));
    if (!skip.has('native')) r.native = json(sh(nativeExe, [file]));
    if (!skip.has('wasm')) r.wasm = json(sh('node', [...flags, join(here, 'wasm_mem.mjs'), file, wasmCount, '--pdf']));
    if (!skip.has('phone') && !skip.has('wasm')) r.wasm_phone = json(sh('node', [...flags, join(here, 'wasm_mem.mjs'), file, wasmCount]));
  } catch (e) {
    r.error = String(e.message ?? e).slice(0, 500);
    console.error(r.error);
  }
  results.classes[name] = r;
}

// tables
const NL = String.fromCharCode(10);
const mib = (n) => (n == null ? '-' : (n / 1048576).toFixed(2));
const ratio = (a, b) => (a != null && b ? (a / b).toFixed(2) : '-');
const wasmRssPeak = (w) => w && Math.max(...Object.values(w.steps).map((s) => s.rss)) - w.node.before_rss;
const table = (head, rows) => [`| ${head.join(' | ')} |`, `|${head.map(() => '---').join('|')}|`, ...rows.map((r) => `| ${r.join(' | ')} |`)].join(NL);
const ok = Object.entries(results.classes).filter(([, r]) => !r.error);
const retained = table(['class', 'pages', 'chars', 'TS layout MiB', 'Rust layout MiB', 'Rust/TS', 'Rust B/char', 'wasm layout MiB', 'wasm B/char', 'TS read+layout MiB', 'Rust read+layout MiB', 'Rust/TS'], ok.map(([name, r]) => {
  const chars = r.native?.chars ?? r.ts?.chars;
  return [name, r.native?.pages ?? r.ts?.pages, chars, mib(r.ts?.layout.retained), mib(r.native?.layout.retained), ratio(r.native?.layout.retained, r.ts?.layout.retained),
    r.native && (r.native.layout.retained / chars).toFixed(1), mib(r.wasm?.live_retained.layout), r.wasm && (r.wasm.live_retained.layout / chars).toFixed(1),
    mib(r.ts?.total_retained), mib(r.native?.total_retained), ratio(r.native?.total_retained, r.ts?.total_retained)];
}));
const peaks = table(['class', 'TS peak RSS - idle MiB', 'Rust layout call peak MiB (uo-alloc)', 'peak / retained', 'wasm high-water MiB', 'wasm peak RSS - before MiB', 'wasm high-water + JS face bytes MiB', 'instance collected', 'RSS back after drop MiB'], ok.map(([name, r]) => [
  name, mib(r.ts?.peak_over_idle), mib(r.native?.call_peak), ratio(r.native?.call_peak, r.native?.layout.retained), mib(r.wasm?.highwater_final), mib(wasmRssPeak(r.wasm)),
  mib(r.wasm && r.wasm.highwater_final + r.wasm.js_face_bytes), r.wasm?.instance_collected, r.wasm && mib(r.wasm.node.after_drop_rss - r.wasm.node.before_rss)]));
const md = `${retained}

${peaks}`;
console.log(md);
const outFile = join(outDir, `memory-${tag}.json`);
writeFileSync(outFile, JSON.stringify(results, null, 1));
writeFileSync(join(outDir, `memory-${tag}.md`), md + NL);
console.error(`saved ${outFile}`);
