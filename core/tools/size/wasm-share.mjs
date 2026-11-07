// Marginal transfer cost of a set of functions (core/docs/modules/size-optimization.md): gzip -9 and brotli of all
// function bodies in module order, minus the same without the bodies whose demangled name matches the regex. It is
// what removing the set (replaced by ~nothing) saves on the wire; duplicates that gzip already folds count ~0.
// Checked: the 382 object_json_with sort copies predicted -20.2 KB gzip, the build measured -19.5 KB.
//   node core/tools/size/wasm-share.mjs <names-kept.wasm> <regex> [<regex>...]
import { gzipSync, brotliCompressSync, constants } from 'node:zlib';
import { readModule } from './wasm-read.mjs';

const [file, ...pats] = process.argv.slice(2);
const { functions } = readModule(file);
const gz = (b) => gzipSync(b, { level: 9 }).length;
const br = (b) => brotliCompressSync(b, { params: { [constants.BROTLI_PARAM_QUALITY]: 11, [constants.BROTLI_PARAM_LGWIN]: 24 } }).length;
const all = Buffer.concat(functions.map((f) => f.body));
const g0 = gz(all), b0 = br(all);
console.log(`code bodies: raw ${all.length}, gzip ${g0}, brotli ${b0}`);
for (const pat of pats) {
  const re = new RegExp(pat);
  const rest = functions.filter((f) => !re.test(f.name));
  const raw = all.length - rest.reduce((a, f) => a + f.body.length, 0);
  const r = Buffer.concat(rest.map((f) => f.body));
  console.log(`${pat}\n  ${functions.length - rest.length} fns, raw ${raw}, gzip share ${g0 - gz(r)}, brotli share ${b0 - br(r)}`);
}
