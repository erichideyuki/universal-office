// A carga do spike no Rust compilado para WebAssembly, no Node (o mesmo V8 do Chrome).
//   node bench-wasm.mjs [wasm|wasm-fast]
import { readFileSync } from 'node:fs';
import { join, dirname } from 'node:path';
import { fileURLToPath } from 'node:url';

const PARSE_ITERS = 10, MEASURE_ITERS = 20, SIZE_PT = 11;
const here = dirname(fileURLToPath(import.meta.url));
const profile = process.argv[2] ?? 'wasm';
const dir = join(here, 'data');
const names = readFileSync(join(dir, 'fonts.txt'), 'utf8').split('\n').filter(Boolean);

const t0 = performance.now();
const { instance } = await WebAssembly.instantiate(readFileSync(join(here, `target/wasm32-unknown-unknown/${profile}/uofonts.wasm`)));
const startMs = performance.now() - t0;
const w = instance.exports;
const put = (bytes) => {
  const p = w.alloc(bytes.length);
  new Uint8Array(w.memory.buffer, p, bytes.length).set(bytes);
  return p;
};
for (const n of names) {
  const b = readFileSync(join(dir, 'fonts', n));
  w.add_font(put(b), b.length);
}
const textBytes = readFileSync(join(dir, 'text.txt'));
w.set_text(put(textBytes), textBytes.length);
const glyphs = [...textBytes.toString('utf8')].length * names.length;

w.parse_all(1);
w.measure_all(SIZE_PT, 1);
let t = performance.now();
if (w.parse_all(PARSE_ITERS) !== 0) throw new Error('falha ao ler uma fonte');
const parseMs = (performance.now() - t) / PARSE_ITERS;
t = performance.now();
const total = w.measure_all(SIZE_PT, MEASURE_ITERS);
const measureMs = (performance.now() - t) / MEASURE_ITERS;

const out = w.alloc(5 * 8);
const parity = Object.fromEntries(names.map((n, i) => {
  w.font_info(i, SIZE_PT, out);
  const v = new Float64Array(w.memory.buffer, out, 5);
  return [n, [v[0], v[1], v[2], v[3], v[4].toFixed(6)]];
}));
console.log(JSON.stringify({ impl: `rust-${profile}-node`, startMs: +startMs.toFixed(2), parseMs: +parseMs.toFixed(2), measureMs: +measureMs.toFixed(2), glyphs, total: total.toFixed(6), parity }));
