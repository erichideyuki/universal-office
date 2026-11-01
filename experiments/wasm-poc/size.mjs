// Tamanho do que o navegador baixa de um build (.wasm, .js, sem os .br/.gz): bruto e Brotli.
//   node size.mjs out/interp [out/aot ...]
import { readdirSync, statSync, existsSync } from 'node:fs';
import { join, resolve, dirname } from 'node:path';
import { fileURLToPath } from 'node:url';

const here = dirname(fileURLToPath(import.meta.url));
for (const b of process.argv.slice(2)) {
  const dir = resolve(here, b, 'wwwroot/_framework');
  let raw = 0, br = 0;
  const rows = [];
  for (const f of readdirSync(dir)) {
    if (f.endsWith('.br') || f.endsWith('.gz')) continue;
    const s = statSync(join(dir, f)).size;
    const c = existsSync(join(dir, `${f}.br`)) ? statSync(join(dir, `${f}.br`)).size : s;
    raw += s; br += c;
    rows.push([c, s, f.replace(/\.[a-z0-9]{10}\./, '.')]);
  }
  rows.sort((a, b2) => b2[0] - a[0]);
  const mb = (v) => (v / 1048576).toFixed(2);
  console.log(`${b}: ${mb(raw)} MB, ${mb(br)} MB brotli  (${rows.slice(0, 4).map(([c, , f]) => `${f} ${Math.round(c / 1024)} KB`).join(', ')})`);
}
