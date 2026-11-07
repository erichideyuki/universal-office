// Size of a wasm module (core/docs/modules/size-optimization.md): raw / gzip -9 / brotli 11 of the module, its
// sections, and the code grouped by owning crate, by generic function, or by sort instantiation.
//   node core/tools/size/wasm-size.mjs <file.wasm> [--group owner|generic|sort|name] [--filter <regex>] [--top N]
// Names need CARGO_PROFILE_WASM_STRIP=false (the shipped module has none: sizes only) and `wasm-tools` on PATH.
// Node's gzip -9 differs from the gzip CLI by ~0.6%; the records use `gzip -9 -c f | wc -c`.
import { gzipSync, brotliCompressSync, constants } from 'node:zlib';
import { readFileSync } from 'node:fs';
import { readModule, collapseGenerics, ownerCrate } from './wasm-read.mjs';

const args = process.argv.slice(2);
const opt = (n, d) => { const i = args.indexOf(n); return i < 0 ? d : args[i + 1]; };
const file = args[0];
const group = opt('--group', 'owner'), top = Number(opt('--top', '30')), filter = opt('--filter', null);
const m = readModule(file);
const raw = readFileSync(file);
const br = (b) => brotliCompressSync(b, { params: { [constants.BROTLI_PARAM_QUALITY]: 11, [constants.BROTLI_PARAM_SIZE_HINT]: b.length } }).length;
console.log(`${file}: raw ${raw.length}${m.hasNames ? ' (names kept: strip with wasm-tools strip --all for the shipped size)' : ''}, gzip ${gzipSync(raw, { level: 9 }).length}, brotli ${br(raw)}`);
for (const s of [...m.sections].sort((a, b) => b.size - a.size).slice(0, 8)) console.log(`  ${s.label.padEnd(24)} ${String(s.size).padStart(9)}`);
if (!m.hasNames) { console.log('(no name section: build with CARGO_PROFILE_WASM_STRIP=false for the groups)'); process.exit(0); }

const re = filter ? new RegExp(filter) : null;
const keyOf = {
  owner: (n) => ownerCrate(n),
  generic: (n) => collapseGenerics(n),
  name: (n) => n,
  // one key per sort instantiation: the element type and the caller (closure) it was instantiated for
  sort: (n) => {
    if (!/^core::slice::sort::/.test(n)) return null;
    const at = n.indexOf('::<');
    const inner = at < 0 ? n : n.slice(at + 3);
    const cl = inner.match(/[A-Za-z_][A-Za-z0-9_:]*(?:<[^<>]*(?:<[^<>]*>)*[^<>]*>)?(?:::[A-Za-z_][A-Za-z0-9_]*)*::\{closure#\d+\}/g);
    return cl ? `${cl[cl.length - 1]}` : `Ord: ${inner.split(',')[0]}`;
  },
}[group];
const g = new Map(); let total = 0, count = 0;
for (const f of m.functions) {
  if (re && !re.test(f.name)) continue;
  const k = keyOf(f.name);
  if (k === null) continue;
  total += f.body.length; count++;
  const c = g.get(k) ?? [0, 0]; c[0] += f.body.length; c[1]++; g.set(k, c);
}
console.log(`\ncode bodies: ${total} B in ${count} functions${re ? ` matching ${filter}` : ''}, by ${group}`);
for (const [k, [s, c]] of [...g].sort((a, b) => b[1][0] - a[1][0]).slice(0, top)) console.log(`${String(s).padStart(9)} ${String(c).padStart(5)}  ${k.slice(0, 220)}`);
