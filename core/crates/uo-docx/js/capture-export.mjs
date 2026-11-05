// Parity capture of the `export` facet (frontier D5a): the strings the frozen TS `toHtml` and `toMarkdown`
// (docx-core export/html.ts, markdown.ts) make of `exportDocFromFullDocument(readFullDocument(pkg))`, for the
// 2,742 oracle documents of docx-layout, the docx-core test fixtures and the synthetic packages of
// js/quirk-d5a.mjs (committed as tests/parity/export.inputs).
//
//   UO_TS_ROOT=<built checkout of main> node core/crates/uo-docx/js/capture-export.mjs [--only <key prefix>]
//
// Writes tests/parity/export.capture: one record per `<docx path from the repository root>#export` with the
// facets (each the hex digest of the output string, UTF-8 with lone surrogates as U+FFFD, a `uo_testkit`
// `Stream` `str`):
//   html         toHtml(doc)                                 (full document, images as data URIs)
//   html-frag    toHtml(doc, { fullDocument: false, images: 'omit' })
//   html-title   toHtml(doc, { title: 'T <&> "x"' })
//   md           toMarkdown(doc)
//   md-omit      toMarkdown(doc, { images: 'omit', title: 'Doc' })
//   error-md     `err` when the TS throws (the list level `'  '.repeat(ilvl)`), instead of md / md-omit
//   error        `err` when readFullDocument throws
import { writeFileSync } from 'node:fs';
import { join } from 'node:path';
import { CaptureWriter, Stream, fffd } from '../../uo-testkit/js/testkit.mjs';
import { crate, corpus, readBytes, ts } from './capture-r2-common.mjs';
import { quirkExport, inputsText } from './quirk-d5a.mjs';

const argv = process.argv.slice(2);
const only = argv.includes('--only') ? argv[argv.indexOf('--only') + 1] : undefined;
const { DocxPackage } = await ts('package.js');
const { readFullDocument } = await ts('fulldoc.js');
const { toHtml } = await ts('export/html.js');
const { toMarkdown } = await ts('export/markdown.js');
const { exportDocFromFullDocument } = await ts('export/from-docx.js');

const digest = (s) => new Stream().str(fffd(s)).finish().short;

function facets(pkg) {
  const out = {};
  let full;
  try { full = readFullDocument(pkg); } catch { out.error = 'err'; return out; }
  const doc = exportDocFromFullDocument(full);
  out.html = digest(toHtml(doc));
  out['html-frag'] = digest(toHtml(doc, { fullDocument: false, images: 'omit' }));
  out['html-title'] = digest(toHtml(doc, { title: 'T <&> "x"' }));
  try {
    out.md = digest(toMarkdown(doc));
    out['md-omit'] = digest(toMarkdown(doc, { images: 'omit', title: 'Doc' }));
  } catch { out['error-md'] = 'err'; }
  return out;
}

const cap = new CaptureWriter(['export facet of D5a: digests of the strings of toHtml and toMarkdown over exportDocFromFullDocument(readFullDocument(pkg)), see js/capture-export.mjs']);
let docs = 0, skipped = 0, errors = 0, mdErrors = 0;
const record = (key, pkg) => {
  if (only && !key.startsWith(only)) return;
  const f = facets(pkg);
  for (const [facet, value] of Object.entries(f)) cap.set(key, facet, value);
  if (f.error) errors++;
  if (f['error-md']) mdErrors++;
  docs++;
};

for (const { rel, file } of corpus()) {
  let pkg;
  try { pkg = DocxPackage.load(readBytes(file)); } catch { skipped++; continue; }
  record(`${rel}#export`, pkg);
}

const fromParts = (parts) => {
  const pkg = DocxPackage.empty();
  for (const [path, data] of parts) pkg.setPartText(path, data);
  return pkg;
};
const synthetic = quirkExport();
for (const { key, parts } of synthetic) record(`${key}#export`, fromParts(parts));
if (!only) writeFileSync(join(crate, 'tests', 'parity', 'export.inputs'), inputsText(synthetic, 'export'));

cap.meta('corpus', `docx=${docs} skipped=${skipped} errors=${errors} md-errors=${mdErrors} synthetic=${synthetic.length}`);
if (!only) cap.write(join(crate, 'tests', 'parity', 'export.capture'));
console.log(`records ${docs} (skipped ${skipped}), errors ${errors}, markdown errors ${mdErrors}`);
