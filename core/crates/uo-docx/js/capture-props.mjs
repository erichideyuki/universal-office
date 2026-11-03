// Parity capture of the `props` facet (frontier R1 of the D1a reader): the frozen TS readers of
// docx-core `ooxml/props.ts` (`readRPr`, `readPPr`, `readTblPr`, `readTrPr`, `readTcPr`; they
// call `readBorder`, `readShading`, `readWidth`, `measureTwips`, `onOffVal`, `readRevisionInfo`,
// `readTextEffects`, `elementXml`) over every `w:rPr`, `w:pPr`, `w:tblPr`, `w:tblPrEx`,
// `w:trPr` and `w:tcPr` of the parts of the corpus, in document order:
//
//   word/document.xml, styles.xml, numbering.xml, footnotes.xml, endnotes.xml, header*.xml, footer*.xml
//
// of the 2,742 oracle documents of docx-layout, the docx-core test fixtures, and the quirk corpus
// (js/quirk-corpus.mjs, committed as tests/parity/props.quirks.txt).
//
//   UO_TS_ROOT=<built checkout of main> node core/crates/uo-docx/js/capture-props.mjs [--only <key prefix>]
//
// Writes tests/parity/props.capture. One record per `<docx path from the repository root>#<part>`
// (quirk cases: `quirk/<name>#word/document.xml`) with, per property kind present, two facets:
//   `<kind>`        `count:digest` of the canonical stream (js/canon.mjs) of every object read, in order
//   `<kind>.order`  the same for the key-order stream (insertion order of the keys, compared too)
// and `xml=err` when the part does not parse (depth past 1,000, malformed). Digests only.
//
// `UO_PARITY_TRACE=<key>` (`<docx>#<part>`) writes the traced streams of that input to
// `UO_PARITY_TRACE_DIR` (default `.`) as `ts-<kind>.trace` / `ts-<kind>.order.trace`.
import { readFileSync, writeFileSync } from 'node:fs';
import { join, dirname } from 'node:path';
import { fileURLToPath } from 'node:url';
import { CaptureWriter, Stream, importFrom, tsRoot, walk, relPath } from '../../uo-testkit/js/testkit.mjs';
import { Canon } from './canon.mjs';
import { readCorpus } from './quirk-corpus.mjs';

const here = dirname(fileURLToPath(import.meta.url));
const repo = join(here, '..', '..', '..', '..');
const root = tsRoot();
const argv = process.argv.slice(2);
const only = argv.includes('--only') ? argv[argv.indexOf('--only') + 1] : undefined;
const traceKey = process.env.UO_PARITY_TRACE;
const traceDir = process.env.UO_PARITY_TRACE_DIR ?? '.';

const { DocxPackage } = await importFrom(root, 'write/packages/docx-core/dist/package.js');
const { parseXml } = await importFrom(root, 'write/packages/docx-core/dist/xml.js');
const props = await importFrom(root, 'write/packages/docx-core/dist/ooxml/props.js');

const KINDS = [
  ['rPr', 'w:rPr', props.readRPr],
  ['pPr', 'w:pPr', props.readPPr],
  ['tblPr', 'w:tblPr', props.readTblPr],
  ['tblPrEx', 'w:tblPrEx', props.readTblPr],
  ['trPr', 'w:trPr', props.readTrPr],
  ['tcPr', 'w:tcPr', props.readTcPr],
];
const BY_ELEMENT = new Map(KINDS.map((k) => [k[1], k]));

export const isPropertyPart = (name) => /^word\/(document|styles|numbering|footnotes|endnotes)\.xml$/.test(name) || /^word\/(header|footer)\d*\.xml$/.test(name);

function* elements(rootEl) {
  const stack = [rootEl];
  while (stack.length) {
    const e = stack.pop();
    yield e;
    for (let i = e.children.length - 1; i >= 0; i--) if (typeof e.children[i] !== 'string') stack.push(e.children[i]);
  }
}


/** The facets of one part text: `{ xml: 'err' }` or `{ kind: 'n:digest', 'kind.order': … }`. */
function facetsOf(text, key) {
  let rootEl;
  try { rootEl = parseXml(text); } catch { return { xml: 'err' }; }
  const traced = traceKey === key;
  const acc = new Map(KINDS.map(([k]) => {
    const st = new Stream({ trace: traced });
    const order = new Stream({ trace: traced });
    return [k, { st, order, canon: new Canon(st, { order }), n: 0 }];
  }));
  for (const el of elements(rootEl)) {
    const kind = BY_ELEMENT.get(el.name);
    if (!kind) continue;
    const a = acc.get(kind[0]);
    const value = kind[2](el);
    a.canon.value(value, kind[0]);
    a.n++;
  }
  const out = {};
  for (const [k, a] of acc) {
    if (!a.n) continue;
    if (traced) {
      writeFileSync(join(traceDir, `ts-${k}.trace`), a.st.trace());
      writeFileSync(join(traceDir, `ts-${k}.order.trace`), a.order.trace());
    }
    out[k] = `${a.n}:${a.st.finish().short}`;
    out[`${k}.order`] = `${a.n}:${a.order.finish().short}`;
  }
  return out;
}

const cap = new CaptureWriter(['props facet of the D1a reader (frontier R1): digests of the TS readRPr/readPPr/readTblPr/readTrPr/readTcPr output, see js/capture-props.mjs']);
let docs = 0, parts = 0, objects = 0, errors = 0, skipped = 0;

function record(key, text) {
  if (only && !key.startsWith(only)) return;
  const f = facetsOf(text, key);
  if (!Object.keys(f).length) return;
  for (const [facet, value] of Object.entries(f)) {
    cap.set(key, facet, value);
    if (facet === 'xml') errors++;
    else if (!facet.endsWith('.order')) objects += Number(value.split(':')[0]);
  }
  parts++;
}

const corpus = [
  ...walk(join(repo, 'write', 'packages', 'docx-layout', 'test', 'oracle'), (p) => p.endsWith('.docx')),
  ...walk(join(repo, 'write', 'packages', 'docx-core', 'test', 'fixtures'), (p) => p.endsWith('.docx')),
];
for (const file of corpus) {
  const rel = relPath(repo, file);
  let pkg;
  try { pkg = DocxPackage.load(new Uint8Array(readFileSync(file))); } catch { skipped++; continue; }
  docs++;
  for (const name of pkg.partNames().filter(isPropertyPart).sort()) {
    const text = pkg.getPartText(name);
    if (text === undefined) continue;
    record(`${rel}#${name}`, text);
  }
}
for (const [key, text] of readCorpus()) record(`${key}#word/document.xml`, text);

cap.meta('corpus', `docx=${docs} skipped=${skipped} parts=${parts} objects=${objects} xml-errors=${errors}`);
if (!only) cap.write(join(here, '..', 'tests', 'parity', 'props.capture'));
console.log(`docx ${docs} (skipped ${skipped}), parts ${parts}, objects ${objects}, xml errors ${errors}`);
