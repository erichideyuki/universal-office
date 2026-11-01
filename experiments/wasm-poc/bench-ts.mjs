// O motor TypeScript (docx-core + docx-layout, dist) no Node, com as mesmas fontes, a mesma
// tabela de substitutas (shared/fonts/src/catalog.ts) e o mesmo documento da POC .NET.
//   node bench-ts.mjs [arquivo.docx]
import { readFileSync } from 'node:fs';
import { join, resolve, dirname } from 'node:path';
import { fileURLToPath } from 'node:url';
import { FONT_FILES } from './fonts.mjs';

const here = dirname(fileURLToPath(import.meta.url));
// UO_TS_ROOT: checkout whose TS engine (built dist) is measured — main, the frozen baseline
const root = process.env.UO_TS_ROOT ? resolve(process.env.UO_TS_ROOT) : resolve(here, '../..');
const core = await import(new URL('file:///' + join(root, 'write/packages/docx-core/dist/index.js').replace(/\\/g, '/')).href);
const eng = await import(new URL('file:///' + join(root, 'write/packages/docx-layout/dist/index.js').replace(/\\/g, '/')).href);
const catalog = readFileSync(join(root, 'shared/fonts/src/catalog.ts'), 'utf8');
const subs = new Map();
let inSubs = false;
for (const line of catalog.split('\n')) {
  if (line.startsWith('export const METRIC_SUBSTITUTES')) { inSubs = true; continue; }
  if (!inSubs) continue;
  if (line.startsWith('};')) break;
  const m = /^\s+'?([^':]+?)'?:\s*'([^']+)',?\s*(\/\/.*)?$/.exec(line.replace(/\r$/, ''));
  if (m) subs.set(m[1].toLowerCase(), m[2]);
}

const fontsDir = join(root, 'site/assets/fonts');
const files = new Map();
for (const f of FONT_FILES) {
  const k = f.family.toLowerCase();
  if (!files.has(k)) files.set(k, []);
  files.get(k).push({ ...f, bytes: new Uint8Array(readFileSync(join(fontsDir, f.path))) });
}
const cache = new Map();
const base = {
  face(family, bold, italic) {
    const key = `${family.toLowerCase()}|${bold}|${italic}`;
    if (cache.has(key)) return cache.get(key);
    const list = files.get(family.toLowerCase());
    let face = null;
    if (list) {
      const best = list.reduce((a, e) => ((e.bold === bold ? 0 : 4) + (e.italic === italic ? 0 : 2)) < ((a.bold === bold ? 0 : 4) + (a.italic === italic ? 0 : 2)) ? e : a);
      best.font ??= new eng.SfntFont(best.bytes);
      face = { key: `${best.family}|${best.bold ? 'b' : ''}${best.italic ? 'i' : ''}`, family: best.family, font: best.font, bytes: best.bytes, bold, italic, synthBold: bold && !best.bold, synthItalic: italic && !best.italic };
    }
    cache.set(key, face);
    return face;
  },
};
const provider = {
  face(family, bold, italic) {
    const f = base.face(family, bold, italic);
    if (f) return f;
    const sub = subs.get(family.trim().toLowerCase());
    const s = sub ? base.face(sub, bold, italic) : null;
    return s ? { ...s, standIn: true } : null;
  },
};

const bytes = new Uint8Array(readFileSync(resolve(here, process.argv[2] ?? '../../site/assets/samples/demo.docx')));
let t = performance.now();
const pkg = core.DocxPackage.load(bytes);
const full = core.readFullDocument(pkg);
const readMs = performance.now() - t;
const layoutOnce = () => {
  let embedded = [];
  try { embedded = core.readEmbeddedFonts(pkg); } catch { /* sem fontes embutidas */ }
  return eng.layoutDocument(full, eng.withEmbeddedFonts(provider, embedded, (b) => new eng.SfntFont(b)));
};
t = performance.now();
let res = layoutOnce();
const firstMs = performance.now() - t;
const warm = [];
for (let i = 0; i < 10; i++) { t = performance.now(); res = layoutOnce(); warm.push(performance.now() - t); }
t = performance.now();
const pdf = eng.renderPdf(res);
const pdfMs = performance.now() - t;
const r = (v) => Math.round(v * 10) / 10;
console.log(JSON.stringify({ build: 'typescript-node', readMs: r(readMs), firstLayoutMs: r(firstMs), pages: res.pages.length, warmBestMs: r(Math.min(...warm)), warmMedianMs: r([...warm].sort((a, b) => a - b)[5]), pdfMs: r(pdfMs), pdfKB: Math.round(pdf.length / 1024) }));
