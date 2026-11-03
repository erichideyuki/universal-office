// Differential check of the wasm32 build of uo-text against V8: loads the `size_probe_all` example (every family of
// functions behind one export) and compares it with the same computation done by the JS built-ins and by the
// TS collation, over 300,000 triples of random UTF-16 code units. Needs the example built with the wasm profile:
//   cargo build -p uo-text --features collation --example size_probe_all --target wasm32-unknown-unknown --profile wasm
//   node crates/uo-text/js/wasm-check.mjs core/target/text/wasm32-unknown-unknown/wasm/examples/size_probe_all.wasm
import { readFileSync } from 'node:fs';

const path = process.argv[2];
if (!path) throw new Error('usage: node wasm-check.mjs <size_probe_all.wasm>');
const { instance } = await WebAssembly.instantiate(readFileSync(path), {});
const probe = instance.exports.probe;
const col = await import(new URL('../../../../sheet/packages/calc/src/collation.ts', import.meta.url).href);

const classes = [/^\p{L}$/u, /^\p{M}$/u, /^\p{N}$/u, /^\p{P}$/u];
const mask = (cp) => {
  const c = String.fromCodePoint(cp);
  let m = 0;
  for (let i = 0; i < 4; i++) if (classes[i].test(c)) m |= 1 << i;
  return m;
};

let seed = 0x1234567;
const rnd = (n) => {
  seed ^= seed << 13;
  seed >>>= 0;
  seed ^= seed >>> 17;
  seed ^= seed << 5;
  seed >>>= 0;
  return seed % n;
};
const pool = [0x41, 0x61, 0x20, 0xdf, 0x3a3, 0x3c3, 0x130, 0x301, 0x300, 0x323, 0x302, 0x1100, 0x1161, 0x11a8, 0xac00, 0xd800, 0xdc00, 0xd83d, 0xde00, 0xfb01, 0xa0, 0xfeff, 0x2028, 0xe9, 0x2d, 0x27, 0xad];
const memory = instance.exports.memory;
probe(0x41, 0x301, 0x3a3);
const before = memory.buffer.byteLength;
let checked = 0;
for (let i = 0; i < 300000; i++) {
  const pick = () => (rnd(3) === 0 ? rnd(0x10000) : pool[rnd(pool.length)]);
  const [a, b, c] = [pick(), pick(), pick()];
  const s = String.fromCharCode(a, b, c);
  const other = String.fromCharCode(c, a);
  const order = col.compareText(s, other);
  const want = (s.normalize('NFC').length + s.toUpperCase().length + s.toLowerCase().length + mask(a) + s.trim().length + (order >>> 0)) >>> 0;
  const got = probe(a, b, c) >>> 0;
  if (got !== want) throw new Error(`wasm ${got} != V8 ${want} for ${[a, b, c].map((x) => x.toString(16))}`);
  checked++;
}
if (memory.buffer.byteLength !== before) throw new Error(`wasm memory grew from ${before} to ${memory.buffer.byteLength} bytes (M2)`);
console.log(`memory.buffer stayed at ${before} bytes`);
console.log(`wasm == V8 on ${checked} triples (${path})`);
