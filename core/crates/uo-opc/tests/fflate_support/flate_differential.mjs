// Generator of the constants BASES and EXPECTED in tests/flate_differential.rs.
//
// Run: node tests/fflate_support/flate_differential.mjs <fflate location>
//   e.g. <fflate location> = a UniversalOffice checkout with write/node_modules installed.
// Prints the two Rust `const` lines (rustfmt may wrap BASES in the test file; the values are
// what must match). The Rust test repeats the same xorshift32 mutations.

import { existsSync, readFileSync } from 'node:fs';
import { join, resolve } from 'node:path';
import { pathToFileURL } from 'node:url';

// Loads fflate from the directory given as the first argument: the fflate package itself, a
// directory with node_modules/fflate, or a checkout whose write/node_modules has it.
async function loadFflate(arg) {
  if (!arg) {
    console.error('usage: node <script> <fflate package dir | dir with node_modules | UniversalOffice checkout>');
    process.exit(2);
  }
  const base = resolve(arg);
  for (const dir of [base, join(base, 'node_modules', 'fflate'), join(base, 'write', 'node_modules', 'fflate')]) {
    const entry = join(dir, 'esm', 'index.mjs');
    if (existsSync(entry)) {
      const { version } = JSON.parse(readFileSync(join(dir, 'package.json'), 'utf8'));
      if (version !== '0.8.3') console.error(`warning: fflate ${version}; the fixtures were made with 0.8.3`);
      return import(pathToFileURL(entry).href);
    }
  }
  console.error(`fflate not found under ${base}`);
  process.exit(2);
}
const f = await loadFflate(process.argv[2]);
const hex = (u) => Buffer.from(u).toString('hex');
let x = 0x9E3779B9 >>> 0;
const rnd = () => { x ^= x << 13; x >>>= 0; x ^= x >>> 17; x ^= x << 5; x >>>= 0; return x; };
const crc = (d) => { let c = -1; for (const b of d) { c ^= b; for (let k = 0; k < 8; k++) c = (c & 1) ? (c >>> 1) ^ 0xEDB88320 : c >>> 1; } return (~c) >>> 0; };
const t1 = new TextEncoder().encode('<w:p><w:r><w:t>Hello, world! 0123456789</w:t></w:r></w:p>'.repeat(6) + 'zyxwvutsrq'.repeat(3));
const bases = [f.deflateSync(t1, { level: 6 }), f.deflateSync(new TextEncoder().encode('short text'), { level: 6 }), f.deflateSync(t1, { level: 0 }).subarray(0, 120), f.deflateSync(t1.subarray(0, 60), { level: 0 })];
const r = (fn) => { try { const o = fn(); return 'ok:' + o.length + ':' + crc(o).toString(16); } catch (e) { return 'err:' + e.message; } };
const out = [];
for (let i = 0; i < 2000; i++) {
  const b = bases[i % 4]; let d = b.slice();
  const m = 1 + rnd() % 3;
  for (let j = 0; j < m; j++) d[rnd() % d.length] = rnd() & 255;
  if (rnd() % 5 == 0) d = d.subarray(0, rnd() % d.length);
  const decl = rnd() % 700;
  out.push(r(() => f.inflateSync(d)) + '|' + r(() => f.inflateSync(d, { out: new Uint8Array(decl) })));
}
console.log('const BASES: [&str; 4] = [' + bases.map(b => '"' + hex(b) + '"').join(', ') + '];');
console.log('const EXPECTED: &str = "' + out.join('\n') + '";');
