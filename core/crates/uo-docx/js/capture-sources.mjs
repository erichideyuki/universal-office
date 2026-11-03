// Parity capture of the `sources` facet (frontier R6 of the D1a reader): the `SourceStore` the
// frozen TS `readFullDocument(pkg, { sources: true })` (docx-core fulldoc.ts, document.ts
// `captureParagraph`, source.ts) builds, in the canonical stream (js/canon.mjs `sources`), over the
// corpus: the 2,742 oracle documents of docx-layout, the docx-core test fixtures, and the
// synthetic packages of js/quirk-sources.mjs (committed as tests/parity/sources.inputs, so the Rust
// replay reads the same text).
//
//   UO_TS_ROOT=<built checkout of main> node core/crates/uo-docx/js/capture-sources.mjs [--only <key prefix>] [--trace <key>]
//
// `--trace <key>` writes the readable traces of that input to `UO_PARITY_TRACE_DIR` (default `.`) as
// `ts-sources.trace` (the Rust side writes `rust-sources.trace`).
//
// Writes tests/parity/sources.capture: one record per `<docx path from the repository root>#sources`
// with the facets `sources` (the whole store), one per member to point a mismatch (`next`, `paras`,
// `runs`, `tables`, `trs`, `tcs`, `stories`, `txbxGroups`, `noteParts`, `commentIds`, `commentsSig`)
// and `n` (the number of source ids), or `error=err` when the TS throws. Digests only. The key-order
// facet is not compared: the records of the store are never stringified.
//
// Named acceptances: none of this facet; the lone-surrogate character references reach it only
// through attribute strings (`link`, `tip`, ids), where R2Canon replaces them as the others do.
import { writeFileSync } from 'node:fs';
import { join } from 'node:path';
import { CaptureWriter, Stream } from '../../uo-testkit/js/testkit.mjs';
import { crate, corpus, readBytes, ts, R2Canon } from './capture-r2-common.mjs';
import { quirkSources, inputsText, QUIRK_SEED, QUIRK_COUNT } from './quirk-sources.mjs';

const argv = process.argv.slice(2);
const only = argv.includes('--only') ? argv[argv.indexOf('--only') + 1] : undefined;
const traceKey = argv.includes('--trace') ? argv[argv.indexOf('--trace') + 1] : undefined;
const traceDir = process.env.UO_PARITY_TRACE_DIR ?? '.';
const { DocxPackage } = await ts('package.js');
const { readFullDocument } = await ts('fulldoc.js');

const MEMBERS = ['next', 'paras', 'runs', 'tables', 'trs', 'tcs', 'stories', 'txbxGroups', 'noteParts', 'commentIds', 'commentsSig'];
const PART = /^word\/[^/]+\.xml$/;

/** The part texts the spans point into (every `word/*.xml`; the digest finds the one `raw` sits in). */
const partTexts = (pkg) => pkg.partNames().filter((p) => PART.test(p)).sort().map((p) => pkg.getPartText(p)).filter((t) => t !== undefined);

/** The digest of one member of the store, written as `Canon.sources` writes it. */
function memberDigest(store, k, parts) {
  const s = new Stream();
  const c = new R2Canon(s, { parts });
  const v = store[k];
  if (v === undefined) s.tag('U');
  else if (v instanceof Map) {
    s.tag('M').u32(v.size);
    for (const [mk, mv] of v) { c.value(mk, `${k}{k}`); c.sourceValue(mv, k); }
  } else if (k === 'txbxGroups') {
    s.tag('[').u32(v.length);
    for (const g of v) { s.tag('[').u32(g.length); for (const x of g) c.object(x, 'groups[]'); }
  } else c.value(v, k);
  return s.finish().short;
}

function facets(pkg, key) {
  let full;
  try { full = readFullDocument(pkg, { sources: true }); } catch { return { error: 'err' }; }
  const store = full.sources;
  const parts = partTexts(pkg);
  const traced = key === traceKey;
  const st = new Stream({ trace: traced });
  new R2Canon(st, { parts }).sources(store);
  if (traced) writeFileSync(join(traceDir, 'ts-sources.trace'), st.trace());
  const out = { sources: st.finish().short, n: String(store.next - 1) };
  for (const m of MEMBERS) out[m] = memberDigest(store, m, parts);
  return out;
}

const cap = new CaptureWriter(['sources facet of the D1a reader (frontier R6): digests of the TS SourceStore of readFullDocument(pkg, { sources: true }), see js/capture-sources.mjs']);
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
  record(`${rel}#sources`, pkg);
}

const inputs = quirkSources(QUIRK_SEED, QUIRK_COUNT);
for (const { key, parts } of inputs) {
  const pkg = DocxPackage.empty();
  for (const [path, data] of parts) pkg.setPartText(path, data);
  record(`${key}#sources`, pkg);
}
if (!only) writeFileSync(join(crate, 'tests', 'parity', 'sources.inputs'), inputsText(inputs));

cap.meta('corpus', `docx=${docs} skipped=${skipped} errors=${errors} quirk=${QUIRK_COUNT}`);
if (!only) cap.write(join(crate, 'tests', 'parity', 'sources.capture'));
console.log(`records ${docs} (skipped ${skipped}), errors ${errors}`);
