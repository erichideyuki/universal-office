// M3 references of uo-opc in the frozen TS (the record's "Stage 4 — memory" table): the same
// scenarios as package_budgets.rs, each in a fresh node process.
//
//   UO_TS_ROOT=<checkout of the frozen 0.9.x with built dist/> node --expose-gc ts_budgets.mjs
//
// Per scenario: `retained` = heapUsed + arrayBuffers after a full GC with the outputs alive,
// minus the same before (what the operation keeps); `peakRss` = growth of the process's
// maximum resident set (an upper bound of the peak, V8 does not expose allocated bytes).
import { readFileSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join, resolve, dirname } from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';
import { execFileSync } from 'node:child_process';
import { createRequire } from 'node:module';

const here = dirname(fileURLToPath(import.meta.url));
const repo = resolve(here, '../../../../..');
const root = process.env.UO_TS_ROOT ? resolve(process.env.UO_TS_ROOT) : repo;
const file = (rel) => new Uint8Array(readFileSync(join(repo, rel)));
const imp = (p) => import(pathToFileURL(join(root, p)).href);

const scenarios = ['giant-open-read-parse', 'giant-save-untouched', 'docx-open-read-parse', 'docx-edit-save', 'deck-open-parse', 'deck-edit-save'];
const scenario = process.argv[2];
// the deck is built once by the parent: building it in a child would raise that child's peak RSS
const deckPath = join(tmpdir(), 'uo-opc-m3-deck.pptx');
if (!scenario) {
  const { zipSync, unzipSync } = await loadFflate();
  writeFileSync(deckPath, buildDeck(zipSync, unzipSync));
  const out = {};
  for (const s of scenarios) {
    out[s] = JSON.parse(execFileSync(process.execPath, ['--expose-gc', '--max-old-space-size=8192', fileURLToPath(import.meta.url), s], { env: process.env, maxBuffer: 1 << 20 }).toString());
  }
  console.log(JSON.stringify(out, null, 2));
  process.exit(0);
}

const { XlsxPackage } = await imp('sheet/packages/sheet-core/dist/package.js');
const sheetXml = await imp('sheet/packages/sheet-core/dist/xml.js');
const { DocxPackage } = await imp('write/packages/docx-core/dist/package.js');
const docxXml = await imp('write/packages/docx-core/dist/xml.js');
const { PptxPackage } = await imp('slides/packages/slides-core/dist/package.js');
const slidesXml = await imp('slides/packages/slides-core/dist/xml.js');

const deck = () => new Uint8Array(readFileSync(deckPath));
const isXml = (n) => /(\.xml|\.rels|\.vml)$/i.test(n);
const live = () => { const m = process.memoryUsage(); return m.heapUsed + m.arrayBuffers; };
const settle = () => { for (let i = 0; i < 4; i++) global.gc(); return live(); };

function noise(len, seed) {
  let x = BigInt(seed | 1);
  const out = new Uint8Array(len);
  const m = (1n << 64n) - 1n;
  for (let o = 0; o < len; o += 8) {
    x ^= (x << 13n) & m; x ^= x >> 7n; x ^= (x << 17n) & m;
    for (let k = 0; k < 8 && o + k < len; k++) out[o + k] = Number((x >> BigInt(8 * k)) & 0xffn);
  }
  return out;
}
async function loadFflate() {
  const require = createRequire(join(root, 'write/packages/docx-core/package.json'));
  return import(pathToFileURL(require.resolve('fflate')).href);
}
function buildDeck(zipSync, unzipSync) {
  const parts = unzipSync(file('slides/samples/reference.pptx'));
  parts['ppt/media/zeros.bin'] = new Uint8Array(64 << 20);
  parts['ppt/media/video.mp4'] = [noise(16 << 20, 7), { level: 0 }];
  return zipSync(parts, { level: 6 });
}
function parseAll(pkg, parse) {
  let n = 0;
  for (const name of pkg.partNames()) {
    if (isXml(name)) { const t = pkg.getPartText(name); if (parse(t, name)) n++; } else pkg.getPart(name);
  }
  return n;
}
function edit(pkg, main) {
  const t = pkg.getPartText(main);
  const i = t.lastIndexOf('</');
  pkg.setPartText(main, t.slice(0, i) + '<uo:soak xmlns:uo="urn:uo" n="1"/>' + t.slice(i));
  pkg.setPartText('customXml/soak.xml', '<soak n="1"/>');
}

const setup = {
  'giant-open-read-parse': () => ({ input: file('sheet/samples/giant-1m.xlsx') }),
  'giant-save-untouched': () => ({ pkg: XlsxPackage.load(file('sheet/samples/giant-1m.xlsx')) }),
  'docx-open-read-parse': () => ({ input: file('write/packages/docx-layout/test/oracle/justify4/sw-16-8.docx') }),
  'docx-edit-save': () => ({ pkg: DocxPackage.load(file('write/packages/docx-layout/test/oracle/justify4/sw-16-8.docx')) }),
  'deck-open-parse': () => ({ input: deck() }),
  'deck-edit-save': () => ({ pkg: PptxPackage.load(deck()) }),
};
const run = {
  'giant-open-read-parse': ({ input }) => { const p = XlsxPackage.load(input); parseAll(p, (t) => sheetXml.parseXml(t)); return p; },
  'giant-save-untouched': ({ pkg }) => pkg.save(),
  'docx-open-read-parse': ({ input }) => { const p = DocxPackage.load(input); return [p, docxXml.parseXml(p.getPartText('word/document.xml'), { positions: true })]; },
  'docx-edit-save': ({ pkg }) => { edit(pkg, 'word/document.xml'); return pkg.save(); },
  'deck-open-parse': ({ input }) => { const p = PptxPackage.load(input); parseAll(p, (t) => slidesXml.parseXml(t)); return p; },
  'deck-edit-save': ({ pkg }) => { edit(pkg, 'ppt/slides/slide1.xml'); return pkg.save(); },
};

const ctx = setup[scenario]();
const before = settle();
const rss0 = process.resourceUsage().maxRSS * 1024;
const t = performance.now();
const result = run[scenario](ctx);
const ms = performance.now() - t;
const rss1 = process.resourceUsage().maxRSS * 1024;
const after = settle();
console.log(JSON.stringify({ retained: after - before, peakRss: rss1 - rss0, ms: Math.round(ms), size: result?.length ?? null }));
