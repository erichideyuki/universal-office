// Parity capture of the `styles` facet (frontier R2 of the D1a reader): the frozen TS `readStyles`
// (docx-core styles.ts: `readStyleRunProps`, `readStyleParaProps`, the `basedOn` resolution, with
// `readBorderEdge` of document.ts) over `word/styles.xml` of the corpus (oracle documents, docx-core
// fixtures) and of the synthetic inputs of js/quirk-parts.mjs (tests/parity/parts.inputs, written
// by capture-parts.mjs).
//
//   UO_TS_ROOT=<built checkout of main> node core/crates/uo-docx/js/capture-styles.mjs [--only <key prefix>]
//
// Writes tests/parity/styles.capture: per `<docx path from the repository root>#styles` the facets
//   `styles`        `count:digest` of the canonical stream of the `StylesMap` (a `Map` in insertion order)
//   `styles.order`  the key-order stream (compared too)
// or `error=err` when the TS throws. Digests only.
import { join } from 'node:path';
import { CaptureWriter } from '../../uo-testkit/js/testkit.mjs';
import { crate, corpus, digestOf, readBytes, ts } from './capture-r2-common.mjs';
import { quirkParts, QUIRK_SEED, QUIRK_COUNT } from './quirk-parts.mjs';

const argv = process.argv.slice(2);
const only = argv.includes('--only') ? argv[argv.indexOf('--only') + 1] : undefined;
const { DocxPackage } = await ts('package.js');
const { readStyles } = await ts('styles.js');

const cap = new CaptureWriter(['styles facet of the D1a reader (frontier R2): digests of the TS readStyles output, see js/capture-styles.mjs']);
let docs = 0, skipped = 0, errors = 0, styles = 0;
const record = (key, pkg) => {
  if (only && !key.startsWith(only)) return;
  let m;
  try { m = readStyles(pkg); } catch { cap.set(key, 'error', 'err'); errors++; docs++; return; }
  const d = digestOf(m, 'styles');
  cap.set(key, 'styles', `${m.size}:${d.full}`);
  cap.set(key, 'styles.order', `${m.size}:${d.order}`);
  styles += m.size;
  docs++;
};

for (const { rel, file } of corpus()) {
  let pkg;
  try { pkg = DocxPackage.load(readBytes(file)); } catch { skipped++; continue; }
  record(`${rel}#styles`, pkg);
}
for (const { key, parts } of quirkParts(QUIRK_SEED, QUIRK_COUNT)) {
  const pkg = DocxPackage.empty();
  for (const [path, data] of parts) {
    if (typeof data === 'string') pkg.setPartText(path, data); else pkg.setPart(path, data);
  }
  record(`${key}#styles`, pkg);
}
cap.meta('corpus', `docx=${docs} skipped=${skipped} errors=${errors} styles=${styles}`);
if (!only) cap.write(join(crate, 'tests', 'parity', 'styles.capture'));
console.log(`records ${docs} (skipped ${skipped}), errors ${errors}, styles ${styles}`);
