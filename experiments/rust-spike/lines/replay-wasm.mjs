// Réplica do spike 2 no Rust compilado para WebAssembly, no Node (o V8 do Chrome): as mesmas
// métricas das outras réplicas, com a memória contada pelo alocador do próprio módulo e o
// tamanho da memória linear do WebAssembly (que só cresce).
//   node replay-wasm.mjs [wasm|wasm-fast] [iterações]
import { readFileSync } from 'node:fs';
import { join, dirname } from 'node:path';
import { fileURLToPath } from 'node:url';

const here = dirname(fileURLToPath(import.meta.url));
const profile = process.argv[2] ?? 'wasm-fast';
const ITERS = Number(process.argv[3] ?? 20);
const { instance } = await WebAssembly.instantiate(readFileSync(join(here, `../target/wasm32-unknown-unknown/${profile}/uofonts.wasm`)));
const w = instance.exports;
const bin = readFileSync(join(here, '../data/lines.bin'));
const p = w.alloc(bin.length);
new Uint8Array(w.memory.buffer, p, bin.length).set(bin);
const nLines = w.lines_load(p, bin.length);
const statsPtr = w.alloc(24);
const stats = () => { w.mem_stats(statsPtr); const v = new Float64Array(w.memory.buffer, statsPtr, 3); return { total: v[0], live: v[1], peak: v[2] }; };

const bad = w.lines_replay(0);
w.lines_bench(1);
const times = [];
for (let i = 0; i < ITERS; i++) { const t = performance.now(); w.lines_bench(1); times.push(performance.now() - t); }
times.sort((a, b) => a - b);

const s0 = stats();
w.mem_reset_peak();
w.lines_replay(1);
const s1 = stats();
w.lines_drop_kept();
const mb = (b) => Math.round((b / 1048576) * 100) / 100;
const r = (v) => Math.round(v * 100) / 100;
console.log(JSON.stringify({
  impl: `rust-${profile}-node`, lines: nLines, mismatches: bad, bestMs: r(times[0]), medianMs: r(times[Math.floor(times.length / 2)]),
  // lines_replay(1) também confere as linhas: o alocado inclui a conferência (sem alocações próprias)
  allocatedMB: mb(s1.total - s0.total), peakAboveBaseMB: mb(s1.peak - s0.live), retainedMB: mb(s1.live - s0.live),
  wasmMemoryMB: mb(w.memory.buffer.byteLength), peakRssMB: r(process.resourceUsage().maxRSS / 1024),
}));
