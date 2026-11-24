// Seeded mutants of `.xls` files for the hostile differential run of the converter (frontier C2): truncations, byte flips,
// zeroed blocks, a swapped pair of blocks, a 0xFF-filled run — over the files of one or more folders. Each mutant is captured by
// `capture-xls.mjs` (the frozen TS) and replayed by `tests/xls_parity.rs` (Rust): both must give the same parts or the same
// class of error.
//
//   node core/crates/uo-xlsx/js/mutate-xls.mjs --out <folder> --per <n> [--seed <s>] [--max <bytes>] <folder|file>...
import { mkdirSync, readFileSync, readdirSync, statSync, writeFileSync } from 'node:fs';
import { basename, join } from 'node:path';
import { args } from './lib/common.mjs';

const a = args();
const out = a.opt('--out');
const per = Number(a.opt('--per', 6));
let seed = Number(a.opt('--seed', 1)) >>> 0 || 1;
const max = Number(a.opt('--max', 400_000));
const inputs = a.argv.filter((x, i, v) => !x.startsWith('--') && !['--out', '--per', '--seed', '--max'].includes(v[i - 1]));
const rnd = () => { seed ^= seed << 13; seed >>>= 0; seed ^= seed >>> 17; seed ^= seed << 5; seed >>>= 0; return seed / 4294967296; };
const int = (n) => Math.floor(rnd() * n);

mkdirSync(out, { recursive: true });
const files = [];
for (const p of inputs) {
  if (statSync(p).isDirectory()) for (const f of readdirSync(p).sort()) { if (/\.xls$/i.test(f)) files.push(join(p, f)); }
  else files.push(p);
}
let n = 0;
for (const f of files) {
  const src = readFileSync(f);
  if (src.length > max) continue;
  const name = basename(f, '.xls');
  for (let k = 0; k < per; k++) {
    let b = Buffer.from(src);
    const kind = k % 6;
    if (kind === 0) b = b.subarray(0, Math.max(8, int(b.length)));
    else if (kind === 1) b = b.subarray(0, Math.max(8, Math.floor(b.length * (0.8 + rnd() * 0.19))));
    else if (kind === 2) for (let i = 0; i < 1 + int(40); i++) b[512 + int(b.length - 512)] ^= 1 << int(8);
    else if (kind === 3) { const at = 512 + int(Math.max(1, b.length - 1024)); b.fill(0, at, at + 64 + int(512)); }
    else if (kind === 4) { const at = 512 + int(Math.max(1, b.length - 1024)); b.fill(0xff, at, at + 16 + int(200)); }
    else { const x = 512 + int(Math.max(1, b.length - 1100)); const y = 512 + int(Math.max(1, b.length - 1100)); const t = Buffer.from(b.subarray(x, x + 256)); b.copy(b, x, y, y + 256); t.copy(b, y); }
    writeFileSync(join(out, `${name}-m${k}.xls`), b);
    n++;
  }
}
console.log(`${n} mutants of ${files.length} files -> ${out}`);
