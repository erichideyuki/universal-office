// TS runner of the uo-opc benchmark (rust-port stage 5): the scenarios of
// core/crates/uo-opc/tests/bench_support on the frozen 0.9.x packages (each app's own package and
// XML parser), each scenario in a fresh node process.
//
//   UO_TS_ROOT=<built checkout of main> node --expose-gc tools/bench/opc/ts.mjs [runs]
//
// Per scenario: `ms` = best of the runs; on the first run `peakRss` = growth of the process's
// maximum resident set (an upper bound of the peak: V8 does not expose allocated bytes) and
// `kept` = heapUsed + arrayBuffers after a full GC with the outputs alive, minus the same before
// (the memory verifier's method, core/crates/uo-opc/tests/memory_support/ts_budgets.mjs).
import { execFileSync } from 'node:child_process';
import { readFileSync } from 'node:fs';
import { join, resolve, dirname } from 'node:path';
import { fileURLToPath } from 'node:url';
import { importFrom, tsRoot, walk } from '../../../core/crates/uo-testkit/js/testkit.mjs';

const here = dirname(fileURLToPath(import.meta.url));
const repo = resolve(here, '../../..');
const root = tsRoot();
const runs = Number(process.argv.find((a, i) => i > 1 && /^\d+$/.test(a)) ?? 5);
const scenarios = ['corpus', 'giant', 'inflate', 'saveDocx', 'saveGiant'];
const scenario = process.argv.find((a) => scenarios.includes(a));
if (!scenario) {
  const out = {};
  for (const s of scenarios) {
    const args = ['--expose-gc', '--max-old-space-size=8192', fileURLToPath(import.meta.url), s, String(runs)];
    out[s] = JSON.parse(execFileSync(process.execPath, args, { env: process.env, maxBuffer: 1 << 20 }).toString());
  }
  console.log(JSON.stringify(out));
  process.exit(0);
}

const { DocxPackage } = await importFrom(root, 'write/packages/docx-core/dist/package.js');
const docxXml = await importFrom(root, 'write/packages/docx-core/dist/xml.js');
const { XlsxPackage } = await importFrom(root, 'sheet/packages/sheet-core/dist/package.js');
const sheetXml = await importFrom(root, 'sheet/packages/sheet-core/dist/xml.js');
const charts = await importFrom(root, 'shared/charts/dist/xml.js');
const fflate = await importFrom(root, 'sheet/node_modules/fflate/esm/index.mjs');

const file = (rel) => new Uint8Array(readFileSync(join(repo, rel)));
const ORACLE = 'write/packages/docx-layout/test/oracle';
const GIANT = 'sheet/samples/giant-1m.xlsx';
const GIANT_SHEET = 'xl/worksheets/sheet1.xml';
const DOCX = 'write/packages/docx-layout/test/oracle/justify4/sw-16-8.docx';
const XML_EXT = ['.xml', '.rels', '.vml', '.xaml', '.fdseq', '.fdoc', '.fpage'];
// uo_opc::limits::is_xml_like
const isXmlLike = (name) => {
  const base = name.slice(name.lastIndexOf('/') + 1).replace(/[A-Z]/g, (c) => c.toLowerCase());
  return base === '[content_types].xml' || XML_EXT.some((e) => base.endsWith(e));
};
const count = (el) => 1 + el.children.reduce((n, c) => (typeof c === 'string' ? n : n + count(c)), 0);
const live = () => { const m = process.memoryUsage(); return m.heapUsed + m.arrayBuffers; };
const settle = () => { for (let i = 0; i < 4; i++) global.gc(); return live(); };

// bench_support::read_and_parse: XML-like parts parsed with the app's preset, the others read
const apps = {
  docx: { load: (b) => DocxPackage.load(b), parse: (t) => docxXml.parseXml(t, { positions: true }) },
  xlsx: { load: (b) => XlsxPackage.load(b), parse: (t, n) => (n.includes('/charts/') ? charts.canonicalNs(sheetXml.parseXml(t)) : sheetXml.parseXml(t)) },
};
function readAndParse(app, pkg, totals) {
  totals.docs++;
  for (const n of pkg.partNames()) {
    totals.parts++;
    if (isXmlLike(n)) {
      const tree = app.parse(pkg.getPartText(n), n);
      if (totals.count) totals.elements += count(tree);
    } else totals.bytes += pkg.getPart(n).length;
  }
}
const totals = (countElements) => ({ docs: 0, parts: 0, elements: 0, bytes: 0, count: countElements });
// ts_budgets.mjs `edit`
function edit(pkg, main) {
  const t = pkg.getPartText(main);
  const i = t.lastIndexOf('</');
  pkg.setPartText(main, t.slice(0, i) + '<uo:soak xmlns:uo="urn:uo" n="1"/>' + t.slice(i));
  pkg.setPartText('customXml/soak.xml', '<soak n="1"/>');
}

// setup(): the inputs (outside the timing); run(input): the operation, returning its outputs
const defs = {
  corpus: {
    setup: () => walk(join(repo, ORACLE), (p) => p.endsWith('.docx')).map((p) => new Uint8Array(readFileSync(p))),
    run: (files, t = totals(false)) => { for (const b of files) readAndParse(apps.docx, apps.docx.load(b), t); return t; },
  },
  giant: {
    setup: () => file(GIANT),
    run: (b, t = totals(false)) => { const pkg = apps.xlsx.load(b); readAndParse(apps.xlsx, pkg, t); return [pkg, t]; },
  },
  inflate: {
    setup: () => file(GIANT),
    run: (b) => fflate.unzipSync(b, { filter: (f) => f.name === GIANT_SHEET })[GIANT_SHEET],
  },
  saveDocx: {
    setup: () => DocxPackage.load(file(DOCX)),
    run: (pkg) => { edit(pkg, 'word/document.xml'); return pkg.save(); },
  },
  saveGiant: {
    setup: () => XlsxPackage.load(file(GIANT)),
    run: (pkg) => { edit(pkg, GIANT_SHEET); return pkg.save(); },
  },
};

const def = defs[scenario];
let input = def.setup();
const before = settle();
const rss0 = process.resourceUsage().maxRSS * 1024;
let t0 = performance.now();
let result = def.run(input);
let ms = performance.now() - t0;
const rss1 = process.resourceUsage().maxRSS * 1024;
const kept = settle() - before;
const out = { peakRss: rss1 - rss0, kept };
result = null;
for (let i = 1; i < runs; i++) {
  input = scenario.startsWith('save') ? def.setup() : input;
  settle();
  t0 = performance.now();
  result = def.run(input);
  ms = Math.min(ms, performance.now() - t0);
  result = null;
}
out.ms = Math.round(ms * 10) / 10;
// checksums (untimed): the same totals as the Rust runners
if (scenario === 'corpus') Object.assign(out, def.run(def.setup(), totals(true)));
else if (scenario === 'giant') Object.assign(out, def.run(input, totals(true))[1]);
else if (scenario === 'inflate') out.bytes = def.run(input).length;
else out.bytes = def.run(def.setup()).length;
delete out.count;
console.log(JSON.stringify(out));
