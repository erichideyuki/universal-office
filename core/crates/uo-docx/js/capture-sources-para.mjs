// Parity capture of the `sources-para` facet (frontier R6 of the D1a reader): the paragraph part of
// the source capture, which does not need the full-document reader (R5). The frozen TS
// `readParagraph` (document.ts, with `captureParagraph` and source.ts) over every paragraph of
// `word/document.xml`, read in document order with one context and one `SourceStore`, the way the
// body is read: a `w:p` is read where it is met and its subtree is not walked again (what sits
// inside it, the paragraphs of a textbox, is read by the paragraph itself). No block reader is
// injected (`readBlocks` undefined, as in a header-less `readParagraph` call), so textboxes keep
// their paragraphs only. The store's paragraphs, runs and ids are digested.
//
//   UO_TS_ROOT=<built checkout of main> node core/crates/uo-docx/js/capture-sources-para.mjs [--only <key prefix>] [--trace <key>]
//
// Writes tests/parity/sources-para.capture: one record per `<docx path from the repository root>#sources-para`
// (synthetic: `quirk/sources/<seed>-<n>.docx#sources-para`, the packages of tests/parity/sources.inputs)
// with the facets `sources` (the whole store), `paras`, `runs`, `next` and `n` (the number of
// paragraphs read), or `error=err` when the TS throws. Digests only.
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
const { parseXml, childElements, firstChild } = await ts('xml.js');
const { readParagraph, buildReadContext } = await ts('document.js');
const { SourceStore } = await ts('source.js');

const MEMBERS = ['next', 'paras', 'runs'];

/** The `w:p` roots of a tree in document order (pre-order; a paragraph's subtree is not entered). */
function paragraphs(root) {
  const out = [];
  const stack = [root];
  while (stack.length) {
    const el = stack.pop();
    if (el.name === 'w:p') { out.push(el); continue; }
    const kids = childElements(el);
    for (let i = kids.length - 1; i >= 0; i--) stack.push(kids[i]);
  }
  return out;
}

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

function facets(pkg, key) {
  const xml = pkg.documentXml();
  if (xml === undefined) return { error: 'err' };
  const store = new SourceStore();
  let n = 0;
  try {
    const root = parseXml(xml, { positions: true });
    const body = firstChild(root, 'w:body') ?? root;
    const ctx = buildReadContext(pkg, 'word/_rels/document.xml.rels');
    ctx.sources = store;
    ctx.sourceXml = xml;
    for (const p of paragraphs(body)) { readParagraph(p, ctx); n++; }
  } catch { return { error: 'err' }; }
  const traced = key === traceKey;
  const st = new Stream({ trace: traced });
  new R2Canon(st, { parts: [xml] }).sources(store);
  if (traced) writeFileSync(join(traceDir, 'ts-sources-para.trace'), st.trace());
  const out = { sources: st.finish().short, n: String(n) };
  for (const m of MEMBERS) out[m] = memberDigest(store, m, [xml]);
  return out;
}

const cap = new CaptureWriter(['sources-para facet of the D1a reader (frontier R6): digests of the TS SourceStore after readParagraph over the paragraphs of word/document.xml, see js/capture-sources-para.mjs']);
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
  record(`${rel}#sources-para`, pkg);
}
for (const { key, parts } of quirkSources(QUIRK_SEED, QUIRK_COUNT)) {
  const pkg = DocxPackage.empty();
  for (const [path, data] of parts) pkg.setPartText(path, data);
  record(`${key}#sources-para`, pkg);
}

cap.meta('corpus', `docx=${docs} skipped=${skipped} errors=${errors} quirk=${QUIRK_COUNT}`);
if (!only) cap.write(join(crate, 'tests', 'parity', 'sources-para.capture'));
console.log(`records ${docs} (skipped ${skipped}), errors ${errors}`);
