// TS side of the memory benchmark: ONE process per document (run.mjs spawns it with --expose-gc).
// Retained = heapUsed + arrayBuffers after GC with the outputs alive, minus the baseline taken after a
// warm-up pass (faces parsed, hyphenation loaded, caches filled). Peak = process maxRSS minus idle RSS.
//   UO_TS_ROOT=<frozen TS checkout> node --expose-gc ts_mem.mjs <file.docx>
// Prints one JSON line.
import { readFileSync } from 'node:fs';
import { join, resolve, dirname } from 'node:path';
import { pathToFileURL, fileURLToPath } from 'node:url';

const here = dirname(fileURLToPath(import.meta.url));
const root = resolve(process.env.UO_TS_ROOT ?? '');
if (!process.env.UO_TS_ROOT) throw new Error('set UO_TS_ROOT');
if (typeof gc !== 'function') throw new Error('run with node --expose-gc');
const file = process.argv[2];
const importFrom = (rel) => import(pathToFileURL(join(root, rel)).href);

const idleRss = process.memoryUsage().rss;
// open fonts only (as the capture tool's 'ci' mode): nothing installed is indexed
process.env.WINDIR = join(here, '__no_system_fonts__');
process.env.LOCALAPPDATA = join(here, '__no_system_fonts__');
process.env.EXTRA_FONT_DIRS = [join(root, 'shared/fonts/files'), join(root, 'site/assets/fonts')].join(';');
const core = await importFrom('write/packages/docx-core/dist/index.js');
const eng = await importFrom('write/packages/docx-layout/dist/index.js');
const { createNodeFontProvider } = await importFrom('write/tools/corpus-lab/node-fonts.mjs');
const provider = await createNodeFontProvider();
await eng.loadHyphenation(eng.HYPHENATION_PATTERN_IDS);
const NOW = new Date(Date.UTC(2026, 0, 15, 12, 0, 0));

const bytes = new Uint8Array(readFileSync(file));
const settle = () => { for (let i = 0; i < 4; i++) gc(); };
const mem = () => { settle(); const m = process.memoryUsage(); return { heap: m.heapUsed, ab: m.arrayBuffers, total: m.heapUsed + m.arrayBuffers }; };
const run = () => {
  const pkg = core.DocxPackage.load(bytes);
  const full = core.readFullDocument(pkg);
  let embedded = [];
  try { embedded = core.readEmbeddedFonts(pkg); } catch { /* none */ }
  const prov = eng.withEmbeddedFonts(provider, embedded, (b) => new eng.SfntFont(b));
  return { pkg, full, prov };
};
// warm-up with the same document (faces, GPOS caches), outputs dropped; then a tiny document, so no
// module-level state still holds the warm-up's pass when the baseline is taken
{
  const w = run();
  eng.layoutDocument(w.full, w.prov, { now: NOW });
}
{
  const tiny = new Uint8Array(readFileSync(join(here, '../../../crates/uo-docx-layout/tests/captures/f8/docs', file.includes('f8__7__0') ? 'f8__7__3.docx' : 'f8__7__0.docx')));
  const pkg = core.DocxPackage.load(tiny);
  eng.layoutDocument(core.readFullDocument(pkg), provider, { now: NOW });
}
const m0 = mem();
const t0 = performance.now();
const a = run();
const t1 = performance.now();
const m1 = mem();
const t2 = performance.now();
const res = eng.layoutDocument(a.full, a.prov, { now: NOW });
const t3 = performance.now();
const m2 = mem();
const chars = res.pages.reduce((n, p) => n + [...(p.lines ?? []), ...(p.contentLines ?? [])].reduce((m, l) => m + (l.chars?.length ?? 0), 0), 0);
const lines = res.pages.reduce((n, p) => n + (p.lines?.length ?? 0), 0);
const ru = process.resourceUsage();
console.log(JSON.stringify({
  docx_bytes: bytes.length,
  pages: res.pages.length,
  lines,
  chars,
  read: { retained: m1.total - m0.total, heap: m1.heap - m0.heap, arrayBuffers: m1.ab - m0.ab, ms: +(t1 - t0).toFixed(1) },
  layout: { retained: m2.total - m1.total, heap: m2.heap - m1.heap, arrayBuffers: m2.ab - m1.ab, ms: +(t3 - t2).toFixed(1) },
  total_retained: m2.total - m0.total,
  baseline: m0.total,
  peak_rss: ru.maxRSS * 1024,
  idle_rss: idleRss,
  peak_over_idle: ru.maxRSS * 1024 - idleRss,
  keep: [a.pkg && 1, res && 1].length,
}));
