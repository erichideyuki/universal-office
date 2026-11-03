// Parity capture of the `sources-blocks` facet (frontier R6 of the D1a reader): the table, wrapper
// and story part of the source capture, which does not need the full-document reader (R5):
//
//   body  `readBlocksOf(body, ctx)` of fulldoc.ts with sources on (paragraphs, tables with rows,
//         cells and `unwrapRaw`'s wrapper pieces, sdt/customXml flattened) over `word/document.xml`;
//   hf    `readHeaderFooterPart(pkg, path, store)` over every header and footer part: the story
//         (`storyOf`) and one `groupOf` per top-level child, with the tables inside.
//
// No block reader is injected for the body (`readBlocks` undefined: a textbox keeps its paragraphs
// only); header and footer parts with a `w:txbxContent` are left out (the TS injects the block reader
// there, which only R5 wires in Rust). The digests are those of the store after each read.
//
//   UO_TS_ROOT=<built checkout of main> node core/crates/uo-docx/js/capture-sources-blocks.mjs [--only <key prefix>] [--trace <key>]
//
// Writes tests/parity/sources-blocks.capture: one record per `<docx path from the repository root>#sources-blocks`
// (synthetic: the packages of tests/parity/sources.inputs) with the facets `body`, `body.n` (blocks
// read), `body.<member>` and, when the package has header/footer parts to read, `hf` (the whole
// store), `hf.parts` (how many) and `hf.<member>`, with members `next`, `paras`, `runs`, `tables`,
// `trs`, `tcs`, `stories`; or `error=err` when the TS throws. Digests only.
import { writeFileSync } from 'node:fs';
import { join } from 'node:path';
import { CaptureWriter, Stream } from '../../uo-testkit/js/testkit.mjs';
import { crate, corpus, readBytes, ts, R2Canon } from './capture-r2-common.mjs';
import { quirkSources, QUIRK_SEED, QUIRK_COUNT } from './quirk-sources.mjs';

const argv = process.argv.slice(2);
const only = argv.includes('--only') ? argv[argv.indexOf('--only') + 1] : undefined;
const traceKey = argv.includes('--trace') ? argv[argv.indexOf('--trace') + 1] : undefined;
const traceDir = process.env.UO_PARITY_TRACE_DIR ?? '.';
const { DocxPackage } = await ts('package.js');
const { parseXml, firstChild } = await ts('xml.js');
const { buildReadContext } = await ts('document.js');
const { readBlocksOf, readHeaderFooterPart } = await ts('fulldoc.js');
const { SourceStore } = await ts('source.js');

const MEMBERS = ['next', 'paras', 'runs', 'tables', 'trs', 'tcs', 'stories'];
const HF = /^word\/(header|footer)\d*\.xml$/;

function memberDigest(store, k, parts) {
  const s = new Stream();
  const c = new R2Canon(s, { parts });
  const v = store[k];
  if (v instanceof Map) {
    s.tag('M').u32(v.size);
    for (const [mk, mv] of v) { c.value(mk, `${k}{k}`); c.sourceValue(mv, k); }
  } else c.value(v, k);
  return s.finish().short;
}

function digests(store, prefix, parts, out, trace) {
  const st = new Stream({ trace: trace !== undefined });
  new R2Canon(st, { parts }).sources(store);
  if (trace !== undefined) writeFileSync(join(traceDir, `ts-sources-blocks-${prefix}.trace`), st.trace());
  out[prefix] = st.finish().short;
  for (const m of MEMBERS) out[`${prefix}.${m}`] = memberDigest(store, m, parts);
}

function facets(pkg, key) {
  const out = {};
  const traced = key === traceKey;
  try {
    const xml = pkg.documentXml();
    const store = new SourceStore();
    const root = parseXml(xml, { positions: true });
    const body = firstChild(root, 'w:body') ?? root;
    const ctx = buildReadContext(pkg, 'word/_rels/document.xml.rels');
    ctx.sources = store;
    ctx.sourceXml = xml;
    out['body.n'] = String(readBlocksOf(body, ctx).length);
    digests(store, 'body', [xml], out, traced ? 'body' : undefined);
    const names = pkg.partNames().filter((p) => HF.test(p)).sort();
    const texts = names.map((p) => pkg.getPartText(p)).filter((t) => t !== undefined && !t.includes('txbxContent'));
    const keep = names.filter((p) => { const t = pkg.getPartText(p); return t !== undefined && !t.includes('txbxContent'); });
    if (keep.length) {
      const hf = new SourceStore();
      for (const p of keep) readHeaderFooterPart(pkg, p, hf);
      out['hf.parts'] = String(keep.length);
      digests(hf, 'hf', texts, out, traced ? 'hf' : undefined);
    }
  } catch { return { error: 'err' }; }
  return out;
}

const cap = new CaptureWriter(['sources-blocks facet of the D1a reader (frontier R6): digests of the TS SourceStore after readBlocksOf over the body and readHeaderFooterPart over the headers and footers, see js/capture-sources-blocks.mjs']);
let docs = 0, skipped = 0, errors = 0;
const record = (key, pkg) => {
  if (only && !key.startsWith(only)) return;
  const f = facets(pkg, key);
  for (const [facet, value] of Object.entries(f)) cap.set(key, facet, value);
  if (f.error) errors++;
  docs++;
};

for (const { rel, file } of corpus()) {
  let pkg;
  try { pkg = DocxPackage.load(readBytes(file)); } catch { skipped++; continue; }
  record(`${rel}#sources-blocks`, pkg);
}
for (const { key, parts } of quirkSources(QUIRK_SEED, QUIRK_COUNT)) {
  const pkg = DocxPackage.empty();
  for (const [path, data] of parts) pkg.setPartText(path, data);
  record(`${key}#sources-blocks`, pkg);
}

cap.meta('corpus', `docx=${docs} skipped=${skipped} errors=${errors} quirk=${QUIRK_COUNT}`);
if (!only) cap.write(join(crate, 'tests', 'parity', 'sources-blocks.capture'));
console.log(`records ${docs} (skipped ${skipped}), errors ${errors}`);
