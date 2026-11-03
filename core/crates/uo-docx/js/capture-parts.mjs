// Parity capture of the `parts` facet (frontier R2 of the D1a reader): the frozen TS
// `readDocFormatting` (docx-core ooxml/parts.ts: `partByRelType`, `readStyleSheet`,
// `readNumbering`, `numberingImages`, `readTheme`, `readSettings`, `readFontTable`, with the
// `readSpPr` of drawing.ts and the property readers of props.ts) over the corpus: the 2,742 oracle
// documents of docx-layout, the docx-core test fixtures, and the synthetic formatting parts of
// js/quirk-parts.mjs (committed as tests/parity/parts.inputs, so the Rust replay reads the same text).
//
//   UO_TS_ROOT=<built checkout of main> node core/crates/uo-docx/js/capture-parts.mjs [--only <key prefix>] [--trace <key>]
//
// `--trace <key>` (a record key) writes the readable traces of that input to `UO_PARITY_TRACE_DIR` (default `.`) as
// `ts-formatting.trace` and `ts-formatting.order.trace`; the Rust side writes `rust-formatting.trace` the same way.
//
// Writes tests/parity/parts.capture: one record per `<docx path from the repository root>#formatting`
// (synthetic: `quirk/parts/<seed>-<n>.docx#formatting`) with the facets
//   `formatting`  digest of the canonical stream (js/canon.mjs) of the whole `DocFormatting`
//   `formatting.order`  the key-order stream (compared too)
//   `styles` `numbering` `theme` `settings` `fontTable`  the same per member, to point a mismatch
// or `error=err` when the TS throws (malformed part). Digests only.
//
// Named acceptance (see R2.md): lone-surrogate character references (R2Canon).
import { writeFileSync } from 'node:fs';
import { join } from 'node:path';
import { CaptureWriter } from '../../uo-testkit/js/testkit.mjs';
import { crate, corpus, digestOf, readBytes, ts } from './capture-r2-common.mjs';
import { quirkParts, inputsText, QUIRK_SEED, QUIRK_COUNT } from './quirk-parts.mjs';

const argv = process.argv.slice(2);
const only = argv.includes('--only') ? argv[argv.indexOf('--only') + 1] : undefined;
const traceKey = argv.includes('--trace') ? argv[argv.indexOf('--trace') + 1] : undefined;
const traceDir = process.env.UO_PARITY_TRACE_DIR ?? '.';
const { DocxPackage } = await ts('package.js');
const { readDocFormatting } = await ts('ooxml/parts.js');

const MEMBERS = ['styles', 'numbering', 'theme', 'settings', 'fontTable'];

function facets(pkg, key) {
  let f;
  try { f = readDocFormatting(pkg); } catch { return { error: 'err' }; }
  const whole = digestOf(f, 'formatting', key === traceKey);
  if (whole.traces) {
    writeFileSync(join(traceDir, 'ts-formatting.trace'), whole.traces.full);
    writeFileSync(join(traceDir, 'ts-formatting.order.trace'), whole.traces.order);
  }
  const out = { formatting: whole.full, 'formatting.order': whole.order };
  for (const m of MEMBERS) out[m] = digestOf(f[m], m).full;
  return out;
}

const cap = new CaptureWriter(['parts facet of the D1a reader (frontier R2): digests of the TS readDocFormatting output, see js/capture-parts.mjs']);
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
  record(`${rel}#formatting`, pkg);
}

const inputs = quirkParts(QUIRK_SEED, QUIRK_COUNT);
for (const { key, parts } of inputs) {
  const pkg = DocxPackage.empty();
  for (const [path, data] of parts) {
    if (typeof data === 'string') pkg.setPartText(path, data); else pkg.setPart(path, data);
  }
  record(`${key}#formatting`, pkg);
}
if (!only) writeFileSync(join(crate, 'tests', 'parity', 'parts.inputs'), inputsText(inputs));

cap.meta('corpus', `docx=${docs} skipped=${skipped} errors=${errors} quirk=${QUIRK_COUNT}`);
if (!only) cap.write(join(crate, 'tests', 'parity', 'parts.capture'));
console.log(`records ${docs} (skipped ${skipped}), errors ${errors}`);
