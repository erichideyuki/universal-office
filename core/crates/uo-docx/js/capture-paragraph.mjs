// Parity capture of the `paragraph` facets (frontier R4c of the D1a reader): the frozen TS
// `readParagraph` (docx-core `document.ts`: runs, fields, hyperlinks, bookmarks, comments,
// revisions, content controls, math, `w:sym`, notes, `w:ptab`, ruby, the TOC mark) over every
// paragraph of every story part of the corpus, in canonical streams (A6):
//
//   UO_TS_ROOT=<built checkout of main> node core/crates/uo-docx/js/capture-paragraph.mjs [--only <key prefix>]
//
// Corpus: the 2,742 oracle documents of docx-layout, the docx-core test fixtures (parts
// `word/document.xml`, `header*`, `footer*`, `footnotes`, `endnotes`, `comments`), and the
// paragraph quirk corpus (js/quirk-paragraph.mjs, regenerated here and committed as
// tests/parity/paragraph.quirks.txt; its relationships are `QUIRK_RELS`).
//
// One record per `<docx path from the repository root>#<part>` (quirk cases: `quirk/<name>#word/document.xml`).
// The paragraphs of the part that are not inside another paragraph are read in document order
// with one `ReadContext` (`buildReadContext`: the field stack, the comment ranges and the bookmark
// ids cross paragraphs as in `readFullDocument`), without a block reader (`readBlocks` unset);
// the content of textboxes is read by `readDrawingRun` through the paragraph (frontier R4b).
// Facets:
//   n               paragraphs read
//   paragraph       digest of the canonical stream of every `Paragraph`
//   order           the key-order stream (insertion order of the keys, compared too)
//   paragraph-src, order-src, sources
//                   the same read with `{ sources: true }` (positions, `SourceStore`): the
//                   paragraphs, their key order, and the `SourceStore` after the part
//   err             the TS threw (the class of the error); `xml=err` when the part does not parse
// Digests only. Two normalisations of the TS side: the keys of a `Run` are `text, props, <rare
// fields>, rpr` in the TS (the reader assigns `rpr` after the rare fields; the runs of a group
// item have `props` after them) and `text, props, rpr, <rare fields>` in the model (`RunExt` is
// flattened after the run's own keys, A2): the walk moves `props` and `rpr` right after `text` in
// every run before the digest, and `blocks` and `paras` after the
// other keys of a textbox (the model keeps the content of a textbox outside its keys and the digest
// writes it last): declared in the record, item 9.
//
// `UO_PARITY_TRACE=<key>` writes the traced streams of that input to `UO_PARITY_TRACE_DIR`
// (default `.`) as `ts-<facet>.trace`.
import { readFileSync, writeFileSync } from 'node:fs';
import { join, dirname } from 'node:path';
import { fileURLToPath } from 'node:url';
import { CaptureWriter, Stream, importFrom, tsRoot, walk, relPath } from '../../uo-testkit/js/testkit.mjs';
import { Canon } from './canon.mjs';
import { generate, writeCorpus, QUIRK_RELS } from './quirk-paragraph.mjs';

const here = dirname(fileURLToPath(import.meta.url));
const repo = join(here, '..', '..', '..', '..');
const root = tsRoot();
const argv = process.argv.slice(2);
const only = argv.includes('--only') ? argv[argv.indexOf('--only') + 1] : undefined;
const traceKey = process.env.UO_PARITY_TRACE;
const traceDir = process.env.UO_PARITY_TRACE_DIR ?? '.';

const { DocxPackage } = await importFrom(root, 'write/packages/docx-core/dist/package.js');
const { parseXml } = await importFrom(root, 'write/packages/docx-core/dist/xml.js');
const { buildReadContext, readParagraph } = await importFrom(root, 'write/packages/docx-core/dist/document.js');
const { SourceStore } = await importFrom(root, 'write/packages/docx-core/dist/source.js');

export const isStoryPart = (name) => /^word\/(document|footnotes|endnotes|comments)\.xml$/.test(name) || /^word\/(header|footer)\d*\.xml$/.test(name);

/** The paragraphs not inside another paragraph, in document order (iterative). */
function* topParagraphs(rootEl) {
  const stack = [rootEl];
  while (stack.length) {
    const e = stack.pop();
    if (e.name === 'w:p') { yield e; continue; }
    for (let i = e.children.length - 1; i >= 0; i--) if (typeof e.children[i] !== 'string') stack.push(e.children[i]);
  }
}

/** Moves `rpr` after `props` in every run-like object below `v` (see the header). */
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
  for (const k of Object.keys(v)) normalize(v[k], seen);
}

const stubPackage = { getPartText: (n) => (n === 'word/_rels/document.xml.rels' ? QUIRK_RELS : undefined), getPart: () => undefined };

/** One read of a part: `{ xml: 'err' }`, `{ err }` or the digests. */
function readPart(text, pkg, relsPath, withSources, key) {
  let rootEl;
  try { rootEl = parseXml(text, { positions: withSources }); } catch { return { xml: 'err' }; }
  const traced = traceKey === key;
  const s = new Stream({ trace: traced });
  const o = new Stream({ trace: traced });
  const canon = new Canon(s, { order: o, parts: [text] });
  const ctx = buildReadContext(pkg, relsPath);
  let store;
  if (withSources) { store = new SourceStore(); ctx.sources = store; ctx.sourceXml = text; }
  let n = 0;
  try {
    for (const p of topParagraphs(rootEl)) {
      const para = readParagraph(p, ctx);
      normalize(para);
      s.tag('E').u32(n);
      o.tag('E').u32(n);
      n++;
      canon.value(para, 'paragraph');
    }
  } catch (e) {
    return { err: String(e?.constructor?.name ?? 'Error') };
  }
  const out = { n, main: s.finish().short, order: o.finish().short };
  if (traced) {
    const sfx = withSources ? '-src' : '';
    writeFileSync(join(traceDir, `ts-paragraph${sfx}.trace`), s.trace());
    writeFileSync(join(traceDir, `ts-order${sfx}.trace`), o.trace());
  }
  if (store) {
    const ss = new Stream({ trace: traced });
    new Canon(ss, { parts: [text] }).sources(store);
    out.sources = ss.finish().short;
    if (traced) writeFileSync(join(traceDir, 'ts-sources.trace'), ss.trace());
  }
  return out;
}

const cap = new CaptureWriter(['paragraph facets of the D1a reader (frontier R4c): digests of the TS readParagraph output, see js/capture-paragraph.mjs']);
let docs = 0, parts = 0, paragraphs = 0, errors = 0, skipped = 0;

function record(key, text, pkg, relsPath) {
  if (only && !key.startsWith(only)) return;
  const plain = readPart(text, pkg, relsPath, false, key);
  if (plain.xml) { cap.set(key, 'xml', 'err'); errors++; parts++; return; }
  if (plain.err) { cap.set(key, 'err', plain.err); errors++; parts++; return; }
  cap.set(key, 'n', plain.n);
  cap.set(key, 'paragraph', plain.main);
  cap.set(key, 'order', plain.order);
  paragraphs += plain.n;
  const src = readPart(text, pkg, relsPath, true, key);
  if (src.err) cap.set(key, 'err-src', src.err);
  else {
    cap.set(key, 'paragraph-src', src.main);
    cap.set(key, 'order-src', src.order);
    cap.set(key, 'sources', src.sources);
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
  for (const name of pkg.partNames().filter(isStoryPart).sort()) {
    const text = pkg.getPartText(name);
    if (text === undefined) continue;
    record(`${rel}#${name}`, text, pkg, `word/_rels/${name.split('/').pop()}.rels`);
  }
}
const cases = generate();
writeCorpus(cases);
for (const [key, text] of cases) record(`${key}#word/document.xml`, text, stubPackage, 'word/_rels/document.xml.rels');

cap.meta('corpus', `docx=${docs} skipped=${skipped} parts=${parts} paragraphs=${paragraphs} errors=${errors}`);
if (!only) cap.write(join(here, '..', 'tests', 'parity', 'paragraph.capture'));
console.log(`docx ${docs} (skipped ${skipped}), parts ${parts}, paragraphs ${paragraphs}, errors ${errors}`);
