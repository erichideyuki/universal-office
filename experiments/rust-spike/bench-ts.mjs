// A carga do spike no motor TS (o SfntFont de verdade, docx-layout/dist): parse das 26 fontes
// PARSE_ITERS vezes; medição do texto com cada fonte MEASURE_ITERS vezes; paridade por fonte.
import { readFileSync } from 'node:fs';
import { join, resolve, dirname } from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';

const PARSE_ITERS = 10, MEASURE_ITERS = 20, SIZE_PT = 11;
const here = dirname(fileURLToPath(import.meta.url));
// UO_TS_ROOT: checkout whose TS engine (built dist) is measured — main, the frozen baseline
const root = process.env.UO_TS_ROOT ? resolve(process.env.UO_TS_ROOT) : resolve(here, '../..');
const { SfntFont } = await import(pathToFileURL(join(root, 'write/packages/docx-layout/dist/index.js')).href);
const dir = join(here, 'data');
const names = readFileSync(join(dir, 'fonts.txt'), 'utf8').split('\n').filter(Boolean);
const bytes = names.map((n) => new Uint8Array(readFileSync(join(dir, 'fonts', n))));
const text = [...readFileSync(join(dir, 'text.txt'), 'utf8')].map((c) => c.codePointAt(0));

const measure = (font, size) => {
  let total = 0, prev = -1;
  for (const cp of text) {
    const g = font.glyphId(cp);
    total += font.advanceTwips(cp, size);
    if (prev >= 0) total += font.kernGlyphsTwips(prev, g, size);
    prev = g;
  }
  return total;
};

let fonts = bytes.map((b) => new SfntFont(b.slice()));
for (const f of fonts) measure(f, SIZE_PT);

let t = performance.now();
for (let i = 0; i < PARSE_ITERS; i++) fonts = bytes.map((b) => new SfntFont(b.slice()));
const parseMs = (performance.now() - t) / PARSE_ITERS;

t = performance.now();
let total = 0;
for (let i = 0; i < MEASURE_ITERS; i++) {
  let s = 0;
  for (const f of fonts) s += measure(f, SIZE_PT);
  if (i === 0) total = s;
}
const measureMs = (performance.now() - t) / MEASURE_ITERS;

const parity = Object.fromEntries(fonts.map((f, i) => [names[i], [f.kernMap.size, f.kernClass0.size, f.kernInLatin ? 1 : 0, f.metrics.unitsPerEm, measure(f, SIZE_PT).toFixed(6)]]));
console.log(JSON.stringify({ impl: 'ts-node', parseMs: +parseMs.toFixed(2), measureMs: +measureMs.toFixed(2), glyphs: text.length * fonts.length, total: total.toFixed(6), parity }));
