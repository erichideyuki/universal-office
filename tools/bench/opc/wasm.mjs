// Wasm runner of the uo-opc benchmark (rust-port stage 5): the scenarios of
// core/crates/uo-opc/tests/bench_support through the bench-only exports of uo-wasm (feature
// `opc-bench`, core/crates/uo-wasm/src/opc_bench.rs), in Node. The inputs are copied into the
// module once (outside the timings) and only read by the exports. Memory: the module's counting
// allocator (allocated bytes, highest peak of one operation, kept bytes) and the growth of
// `memory.buffer.byteLength` over the scenario (wasm memory never shrinks: a high-water mark).
//
//   cargo build -p uo-wasm --features opc-bench --target wasm32-unknown-unknown --profile wasm
//   node tools/bench/opc/wasm.mjs [runs]
import { readFileSync } from 'node:fs';
import { join, resolve, dirname } from 'node:path';
import { fileURLToPath } from 'node:url';
import { walk } from '../../../core/crates/uo-testkit/js/testkit.mjs';

const here = dirname(fileURLToPath(import.meta.url));
const repo = resolve(here, '../../..');
const wasmPath = join(repo, 'core/target/wasm32-unknown-unknown/wasm/uo_wasm.wasm');
const runs = Number(process.argv[2] ?? 5);
const scenarios = ['corpus', 'giant', 'inflate', 'saveDocx', 'saveGiant'];

const ORACLE = 'write/packages/docx-layout/test/oracle';
const GIANT = 'sheet/samples/giant-1m.xlsx';
const GIANT_SHEET = 'xl/worksheets/sheet1.xml';
const DOCX = 'write/packages/docx-layout/test/oracle/justify4/sw-16-8.docx';
const file = (rel) => new Uint8Array(readFileSync(join(repo, rel)));

// each scenario in a fresh instance, so its byteLength growth is its own
async function run(scenario) {
  const { instance } = await WebAssembly.instantiate(readFileSync(wasmPath));
  const w = instance.exports;
  if (typeof w.uo_opc_bench_corpus !== 'function') throw new Error('build uo-wasm with --features opc-bench');
  const put = (bytes) => {
    const p = w.uo_alloc_bytes(bytes.length);
    new Uint8Array(w.memory.buffer, p, bytes.length).set(bytes);
    return [p, bytes.length];
  };
  const name = (s) => put(new TextEncoder().encode(s));
  let call;
  let reps = runs;
  if (scenario === 'corpus') {
    const files = walk(join(repo, ORACLE), (p) => p.endsWith('.docx')).map((p) => readFileSync(p));
    const framed = new Uint8Array(files.reduce((n, f) => n + 4 + f.length, 0));
    let at = 0;
    for (const f of files) { new DataView(framed.buffer).setUint32(at, f.length, true); framed.set(f, at + 4); at += 4 + f.length; }
    const [p, n] = put(framed);
    call = () => w.uo_opc_bench_corpus(p, n, 0);
    reps = Math.min(runs, 3);
  } else if (scenario === 'giant') {
    const [p, n] = put(file(GIANT));
    call = () => w.uo_opc_bench_package(p, n, 1);
    reps = Math.min(runs, 3);
  } else if (scenario === 'inflate') {
    const [p, n] = put(file(GIANT));
    const [q, m] = name(GIANT_SHEET);
    call = () => w.uo_opc_bench_inflate(p, n, q, m);
  } else {
    const [rel, kind, main] = scenario === 'saveDocx' ? [DOCX, 0, 'word/document.xml'] : [GIANT, 1, GIANT_SHEET];
    const [p, n] = put(file(rel));
    const [q, m] = name(main);
    // the package is opened outside the timing and freed after
    call = (time) => {
      const pkg = w.uo_opc_bench_open(p, n, kind);
      const t = performance.now();
      const r = w.uo_opc_bench_save(pkg, q, m);
      time(performance.now() - t);
      w.uo_opc_bench_package_free(pkg);
      return r;
    };
    if (scenario === 'saveGiant') reps = Math.min(runs, 3);
  }
  const bytes0 = w.memory.buffer.byteLength;
  let ms = Infinity;
  let first;
  for (let i = 0; i < reps; i++) {
    let one;
    const t = performance.now();
    const r = call((x) => { one = x; });
    ms = Math.min(ms, one ?? performance.now() - t);
    if (i === 0) first = { checksum: r, allocated: w.uo_opc_bench_cost(0), peak: w.uo_opc_bench_cost(1), kept: w.uo_opc_bench_cost(2) };
  }
  return { ms: Math.round(ms * 10) / 10, ...first, memoryGrowth: w.memory.buffer.byteLength - bytes0 };
}

const out = {};
for (const s of scenarios) out[s] = await run(s);
out.wasmBytes = readFileSync(wasmPath).length;
console.log(JSON.stringify(out));
