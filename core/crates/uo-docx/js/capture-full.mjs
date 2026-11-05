// Parity capture of the `full` facet (frontier R5 of the D1a reader): the `FullDocument` the frozen TS
// `readFullDocument(pkg, { sources })` (docx-core fulldoc.ts) builds, in the canonical stream
// (js/canon.mjs `document`), and the `inspect.ts` queries over its body, for the 2,742 oracle documents
// of docx-layout, the docx-core test fixtures, the synthetic packages of js/quirk-sources.mjs (the
// R6 ones: sdt around rows, cells and paragraphs…, committed as tests/parity/sources.inputs) and the
// whole-document packages of js/quirk-full.mjs (committed as tests/parity/full.inputs).
//
//   UO_TS_ROOT=<built checkout of main> node core/crates/uo-docx/js/capture-full.mjs [--only <key prefix>] [--trace <key>]
//
// `--trace <key>` writes the readable traces of that input to `UO_PARITY_TRACE_DIR` (default `.`) as
// `ts-full*.trace` (the Rust side writes `rust-full*.trace`).
//
// Writes tests/parity/full.capture: one record per `<docx path from the repository root>#full` with the
// facets
//   full, order         the document read without sources, and its key-order facet;
//   full-src, order-src the document read with `{ sources: true }` (the sources themselves are the
//                       `sources` facet of capture-sources.mjs), and its key-order facet;
//   inspect             `{ empty, links, inlines, tag }` of documentIsEmpty / documentLinks /
//                       documentCustomInlines over the body (inspect.ts);
//   hyphlangs           `documentHyphenationLanguages(full)` and with `force` (docx-layout hyphenate.ts), as JSON;
//   error / error-src   `err` when the TS throws (without / with sources).
// Digests only. The model flattens a few TS objects (a run's `rpr`, a text box's content, a cell's
// `blocks`): the TS side is normalised so that the key-order facet is compared exactly (as R4c does,
// item 16 of its record).
import { Buffer } from 'node:buffer';
import { writeFileSync } from 'node:fs';
import { join } from 'node:path';
import { CaptureWriter, Stream } from '../../uo-testkit/js/testkit.mjs';
import { importFrom } from '../../uo-testkit/js/testkit.mjs';
import { crate, corpus, readBytes, ts, root, R2Canon } from './capture-r2-common.mjs';
import { quirkSources, QUIRK_SEED, QUIRK_COUNT } from './quirk-sources.mjs';
import { quirkFull, inputsText } from './quirk-full.mjs';

const argv = process.argv.slice(2);
const only = argv.includes('--only') ? argv[argv.indexOf('--only') + 1] : undefined;
const traceKey = argv.includes('--trace') ? argv[argv.indexOf('--trace') + 1] : undefined;
const traceDir = process.env.UO_PARITY_TRACE_DIR ?? '.';
const { DocxPackage } = await ts('package.js');
const { readFullDocument } = await ts('fulldoc.js');
const { documentIsEmpty, documentLinks, documentCustomInlines } = await ts('inspect.js');
const { documentHyphenationLanguages } = await importFrom(root, 'write/packages/docx-layout/dist/hyphenate.js');

/** Moves `rpr` after `props` in a run, a text box's `blocks` before `paras`, a cell's `blocks` after its
 *  other keys: the order the Rust model's digest writes (item 16 of R4c, and the cell). */
function normalize(v, seen = new Set()) {
  if (v === null || typeof v !== 'object' || v instanceof Uint8Array || seen.has(v)) return;
  seen.add(v);
  if (Array.isArray(v)) { for (const x of v) normalize(x, seen); return; }
  if (v instanceof Map) { for (const x of v.values()) normalize(x, seen); return; }
  if (typeof v.text === 'string' && v.props !== undefined) {
    const copy = { ...v };
    const rest = Object.keys(copy).filter((k) => k !== 'text' && k !== 'props' && k !== 'rpr');
    const hasRpr = Object.hasOwn(copy, 'rpr');
    for (const k of Object.keys(v)) delete v[k];
    v.text = copy.text; v.props = copy.props;
    if (hasRpr) v.rpr = copy.rpr;
    for (const k of rest) v[k] = copy[k];
  }
  if (Array.isArray(v.paras) && typeof v.widthTwips === 'number') {
    const { paras, blocks } = v;
    const hasBlocks = Object.hasOwn(v, 'blocks');
    delete v.paras;
    delete v.blocks;
    if (hasBlocks) v.blocks = blocks;
    v.paras = paras;
  }
  if (Array.isArray(v.paragraphs) && Array.isArray(v.tables) && typeof v.gridSpan === 'number' && Object.hasOwn(v, 'blocks')) {
    const blocks = v.blocks;
    delete v.blocks;
    v.blocks = blocks;
  }
  for (const k of Object.keys(v)) normalize(v[k], seen);
}

function inspectDigest(full) {
  const s = new Stream();
  new R2Canon(s).value({
    empty: documentIsEmpty(full.blocks),
    links: documentLinks(full.blocks),
    inlines: documentCustomInlines(full.blocks),
    tag: documentCustomInlines(full.blocks, 'clickcompliance.document-tag'),
  }, 'inspect');
  return s.finish().short;
}

function facets(pkg, key) {
  const out = {};
  for (const [sfx, sources] of [['', false], ['-src', true]]) {
    let full;
    try { full = readFullDocument(pkg, { sources }); } catch { out[`error${sfx}`] = 'err'; continue; }
    normalize(full);
    delete full.background?.fill?.image?.path;
    const traced = key === traceKey;
    const st = new Stream({ trace: traced });
    const order = new Stream({ trace: traced });
    new R2Canon(st, { order }).document(full);
    out[`full${sfx}`] = st.finish().short;
    out[`order${sfx}`] = order.finish().short;
    if (traced) {
      writeFileSync(join(traceDir, `ts-full${sfx}.trace`), st.trace());
      writeFileSync(join(traceDir, `ts-order${sfx}.trace`), order.trace());
    }
    if (!sources) {
      out.inspect = inspectDigest(full);
      // the TS walk visits the aliased views (a cell's `paragraphs` and `blocks`) both: exponential in the
      // nesting of tables, so the deep-nesting inputs have no such facet
      if (!key.includes('/nest-')) {
        const h = new Stream();
        h.tag('s').bytes(Buffer.from(JSON.stringify([documentHyphenationLanguages(full), documentHyphenationLanguages(full, 'pt-BR', true)]), 'utf16le'));
        out.hyphlangs = h.finish().short;
      }
    }
  }
  return out;
}

const cap = new CaptureWriter(['full facet of the D1a reader (frontier R5): digests of the TS FullDocument of readFullDocument and the inspect.ts queries, see js/capture-full.mjs']);
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
  record(`${rel}#full`, pkg);
}

const fromParts = (parts) => {
  const pkg = DocxPackage.empty();
  if (!parts.some(([path]) => path === 'word/document.xml')) pkg.removePart('word/document.xml');
  for (const [path, data] of parts) pkg.setPartText(path, data);
  return pkg;
};

for (const { key, parts } of quirkSources(QUIRK_SEED, QUIRK_COUNT)) record(`${key}#full`, fromParts(parts));
const full = quirkFull();
for (const { key, parts } of full) record(`${key}#full`, fromParts(parts));
if (!only) writeFileSync(join(crate, 'tests', 'parity', 'full.inputs'), inputsText(full));

cap.meta('corpus', `docx=${docs} skipped=${skipped} errors=${errors} quirk=${QUIRK_COUNT}+${full.length}`);
if (!only) cap.write(join(crate, 'tests', 'parity', 'full.capture'));
console.log(`records ${docs} (skipped ${skipped}), errors ${errors}`);
