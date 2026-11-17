// Which lines of the frozen TS rasterizers (`metafile.js`, `emfplus.js`, `pict.js`, `tiff.js`, `eot.js` of slides-core `dist`) the B5b input set
// reaches: run a capture under `NODE_V8_COVERAGE=<dir>` and print the lines with a zero count (a byte is covered when any import of the module
// covers it; the metafile module is imported fresh per case).
//
//   NODE_V8_COVERAGE=<dir> node core/crates/uo-pptx/js/capture-meta.mjs --out <stem> && node core/crates/uo-pptx/js/coverage-meta.mjs <dir> [file ...]
import { readFileSync, readdirSync } from 'node:fs';
import { join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { DEFAULT_TS_ROOT } from './lib/common.mjs';

const dir = process.argv[2];
const want = process.argv.slice(3).length ? process.argv.slice(3) : ['metafile.js', 'emfplus.js', 'pict.js', 'tiff.js', 'eot.js'];
const cov = new Map(); // module name -> Uint8Array of "covered"
for (const f of readdirSync(dir).filter((n) => n.endsWith('.json'))) {
  const j = JSON.parse(readFileSync(join(dir, f), 'utf8'));
  for (const sc of j.result) {
    const name = want.find((w) => sc.url.includes(`/dist/${w}`));
    if (!name) continue;
    const path = fileURLToPath(sc.url.split('?')[0]);
    const src = readFileSync(path, 'utf8');
    const mine = new Int32Array(src.length).fill(-1);
    // ranges are nested: paint outer functions first (they come first in the list), the inner ones override
    for (const fn of sc.functions) for (const r of fn.ranges) for (let i = r.startOffset; i < Math.min(r.endOffset, src.length); i++) mine[i] = r.count;
    let all = cov.get(name);
    if (!all) { all = { src, cov: new Uint8Array(src.length) }; cov.set(name, all); }
    for (let i = 0; i < src.length; i++) if (mine[i] > 0) all.cov[i] = 1;
  }
}
for (const [name, { src, cov: c }] of cov) {
  const lines = src.split('\n');
  let off = 0;
  const unc = [];
  let total = 0;
  for (let n = 0; n < lines.length; n++) {
    const l = lines[n];
    const code = l.trim();
    if (code && !code.startsWith('//') && !code.startsWith('*') && !code.startsWith('/*') && code !== '}' && code !== '});') {
      total++;
      let any = false;
      for (let i = off; i < off + l.length; i++) if (c[i] && src[i] !== ' ') { any = true; break; }
      if (!any) unc.push(`${n + 1}: ${code.slice(0, 150)}`);
    }
    off += l.length + 1;
  }
  console.log(`== ${name}: ${total - unc.length} / ${total} code lines reached; ${unc.length} not`);
  for (const u of unc) console.log('  ' + u);
}
void DEFAULT_TS_ROOT;
