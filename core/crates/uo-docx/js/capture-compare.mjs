// Parity capture of the `compare` facet (frontier D5a): `JSON.stringify(diffDocuments(a, b, opts))` of the
// frozen TS compare package (write/packages/compare diff.ts, over docx-core `readFullDocument` and
// `finalBlocks`), as digests, for pairs of documents:
//   - every oracle document / docx-core fixture with itself (`<rel>|<rel>#diff`);
//   - every document with the next one of the corpus order (`<rel>|<next rel>#diff`): variants of one
//     case in the same directory, then neighbours that have nothing in common;
//   - every 5th document with the 7th after it;
//   - the generated pairs of js/quirk-d5a.mjs (`quirk/d5a-pair/<name>#diff`, committed as
//     tests/parity/compare.inputs: each pair is two packages, `<key>#a` and `<key>#b`): the same
//     document, one edit (insert, delete, change and move paragraphs, change text inside runs, split
//     runs, change formatting, table rows and cells, images resized or replaced), several edits,
//     unrelated documents, repeated paragraphs, reversed order, words, tracked changes.
//
//   UO_TS_ROOT=<built checkout of main> node core/crates/uo-docx/js/capture-compare.mjs [--only <key prefix>]
//
// Writes tests/parity/compare.capture: facets per pair (each the digest of the JSON string):
//   diff          diffDocuments(a, b)                  (histogram, word refinement)
//   diff-myers    diffDocuments(a, b, { algorithm: 'myers' })   (only pairs of at most 800 blocks together)
//   diff-nointra  diffDocuments(a, b, { intraWord: false })
//   error         `err` when readFullDocument or the diff throws
// Pairs above 3,000 blocks together (4,000 for a document with itself) are skipped: the TS needs
// O((N+M)·D) memory for Myers and O(N·M·N) time for the histogram on repetitive text; the Rust diff
// caps its work for them (`compare::budget`), see the record.
import { writeFileSync } from 'node:fs';
import { join } from 'node:path';
import { CaptureWriter, Stream } from '../../uo-testkit/js/testkit.mjs';
import { crate, corpus, readBytes, ts, root } from './capture-r2-common.mjs';
import { importFrom } from '../../uo-testkit/js/testkit.mjs';
import { quirkPairs, inputsText } from './quirk-d5a.mjs';

const argv = process.argv.slice(2);
const only = argv.includes('--only') ? argv[argv.indexOf('--only') + 1] : undefined;
const { DocxPackage } = await ts('package.js');
const { readFullDocument } = await ts('fulldoc.js');
const { diffDocuments } = await importFrom(root, 'write/packages/compare/dist/diff.js');

const digest = (s) => new Stream().str(s).finish().short;

const MAX_PAIR = 3000;
const MAX_SELF = 4000;
const MAX_MYERS = 800;

function facets(a, b, self) {
  const out = {};
  const n = a.blocks.length + b.blocks.length;
  if (self ? a.blocks.length > MAX_SELF : n > MAX_PAIR) return null;
  try {
    out.diff = digest(JSON.stringify(diffDocuments(a, b)));
    if (n <= MAX_MYERS) out['diff-myers'] = digest(JSON.stringify(diffDocuments(a, b, { algorithm: 'myers' })));
    out['diff-nointra'] = digest(JSON.stringify(diffDocuments(a, b, { intraWord: false })));
  } catch { return { error: 'err' }; }
  return out;
}

const cap = new CaptureWriter(['compare facet of D5a: digests of JSON.stringify(diffDocuments(a, b)) over pairs of documents, see js/capture-compare.mjs']);
let pairs = 0, skipped = 0, unreadable = 0, errors = 0;
const record = (key, a, b, self = false) => {
  if (only && !key.startsWith(only)) return;
  const f = facets(a, b, self);
  if (!f) { skipped++; return; }
  for (const [facet, value] of Object.entries(f)) cap.set(key, facet, value);
  if (f.error) errors++;
  pairs++;
};

const docs = [];
for (const { rel, file } of corpus()) {
  try { docs.push({ rel, full: readFullDocument(DocxPackage.load(readBytes(file))) }); } catch { unreadable++; docs.push({ rel, full: null }); }
}
const key2 = (x, y) => `${x.rel}|${y.rel}#diff`;
docs.forEach((d, i) => {
  if (!d.full) return;
  record(key2(d, d), d.full, d.full, true);
  const next = docs[i + 1];
  if (next?.full) record(key2(d, next), d.full, next.full);
  const far = docs[i + 7];
  if (i % 5 === 0 && far?.full) record(key2(d, far), d.full, far.full);
});

const fromParts = (parts) => {
  const pkg = DocxPackage.empty();
  for (const [path, data] of parts) pkg.setPartText(path, data);
  return pkg;
};
const synthetic = quirkPairs();
for (const { key, a, b } of synthetic) record(`${key}#diff`, readFullDocument(fromParts(a)), readFullDocument(fromParts(b)));
if (!only) {
  writeFileSync(join(crate, 'tests', 'parity', 'compare.inputs'), inputsText(synthetic.flatMap(({ key, a, b }) => [{ key: `${key}#a`, parts: a }, { key: `${key}#b`, parts: b }]), 'compare'));
}

cap.meta('corpus', `pairs=${pairs} skipped=${skipped} unreadable=${unreadable} errors=${errors} synthetic=${synthetic.length}`);
if (!only) cap.write(join(crate, 'tests', 'parity', 'compare.capture'));
console.log(`pairs ${pairs} (skipped ${skipped}, unreadable ${unreadable}), errors ${errors}`);
