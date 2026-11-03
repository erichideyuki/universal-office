// The TS reference of the M3 budgets of uo-fonts: heap retained by a parsed `SfntFont` (the
// constructor reads GPOS/kern eagerly) and by a `Hyphenator`, measured with `--expose-gc` over the
// frozen dist of `main` (never a path in the repo). Compare with `cargo test --release -p uo-fonts
// --test stage4_memory -- m3_ --nocapture --test-threads=1`.
//
//   UO_TS_ROOT=<built checkout of main> node --expose-gc ts_budgets.mjs [<repo>/shared/fonts/files] [extra .ttf files...]
import { readdirSync, readFileSync } from 'node:fs';
import { join } from 'node:path';
import { pathToFileURL } from 'node:url';

const root = process.env.UO_TS_ROOT;
if (!root) throw new Error('set UO_TS_ROOT to a built checkout of main');
if (typeof gc !== 'function') throw new Error('run with node --expose-gc');
const url = (rel) => pathToFileURL(join(root, rel)).href;
const { SfntFont } = await import(url('write/packages/docx-layout/dist/typography/font/sfnt.js'));
const { Hyphenator } = await import(url('write/packages/docx-layout/dist/typography/hyphen/liang.js'));

const dir = process.argv[2] ?? join(root, 'shared/fonts/files');
const extra = process.argv.slice(3);

function settle() {
  for (let i = 0; i < 4; i++) gc();
  const m = process.memoryUsage();
  return m.heapUsed + m.arrayBuffers;
}

function retained(make) {
  const before = settle();
  const kept = make();
  const after = settle();
  return { kept, bytes: after - before };
}

const mb = (n) => (n / 1048576).toFixed(2);
const files = [
  ...readdirSync(dir).filter((f) => f.endsWith('.ttf')).sort().map((f) => join(dir, f)),
  ...extra,
];

console.log('# TS SfntFont retained heap (heapUsed + arrayBuffers after gc, the input bytes held outside), parse time');
console.log('# face | file KB | retained | parse ms');
for (const path of files) {
  const bytes = new Uint8Array(readFileSync(path));
  let font;
  const r = retained(() => (font = new SfntFont(bytes)));
  const t = performance.now();
  for (let i = 0; i < 20; i++) new SfntFont(bytes);
  const ms = (performance.now() - t) / 20;
  console.log(`# ${path.split(/[\\/]/).pop()} | ${Math.round(bytes.length / 1024)} | ${mb(r.bytes)} MB | ${ms.toFixed(2)}`);
  void font;
}

console.log('# TS Hyphenator retained heap (the pattern data module is loaded before the measure and counted separately)');
console.log('# id | pattern file chars | data module | Hyphenator');
for (const id of ['pt', 'en-us', 'es', 'fr', 'de-1996', 'it', 'nl']) {
  const before = settle();
  const data = (await import(url(`write/packages/docx-layout/dist/typography/hyphen/patterns/${id}.js`))).default;
  const dataBytes = settle() - before;
  const h = retained(() => new Hyphenator(data));
  console.log(`# ${id} | ${data.patterns.length + data.exceptions.length} | ${mb(dataBytes)} MB | ${mb(h.bytes)} MB`);
  void h;
}
