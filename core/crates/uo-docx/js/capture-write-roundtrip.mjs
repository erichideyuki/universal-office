// Parity capture of the D1b no-edit round trip: docx-core's own lossless harness
// (write/packages/docx-core/test/lossless.test.ts, `open` + `save`, copied below as written) over the
// corpus, in two variants:
//   `none`  no header/footer part dirty (they stay; missing relationships/content types are added)
//   `all`   every header/footer part dirty (each goes through the lossless story path)
//   `gen`   read WITHOUT sources, every header/footer part dirty: the whole generating path
//           (`serializeBlocks`, `serializeHfPart`: the emitters of the frontiers W1–W3)
// Also the synthetic packages of R6 and R5 (`js/quirk-sources.mjs`, `js/quirk-full.mjs`; the same
// texts as tests/parity/sources.inputs and full.inputs, which the replay reads).
// Rust replays it with `write::roundtrip::{install, save}` (tests/parity_write_roundtrip.rs).
//
//   UO_TS_ROOT=<built checkout of main> node core/crates/uo-docx/js/capture-write-roundtrip.mjs
//
// Writes tests/parity/write-roundtrip.capture, one record per `<docx>#<variant>`:
//   `error`    `err` when the TS throws (then nothing else; Rust: any DocxError but an emitter stub)
//   `order`    digest of the part names of the saved package, in package order
//   `changed`  the names of the parts whose bytes differ from the input (or are new), in package
//              order, joined with `|`
//   `p:<name>` short SHA-256 of each changed part's (decompressed) bytes
import { writeFileSync } from 'node:fs';
import { join } from 'node:path';
import { createHash } from 'node:crypto';
import { CaptureWriter } from '../../uo-testkit/js/testkit.mjs';
import { corpus, crate, readBytes, ts } from './capture-r2-common.mjs';
import { quirkSources, QUIRK_SEED, QUIRK_COUNT } from './quirk-sources.mjs';
import { quirkFull } from './quirk-full.mjs';

const { DocxPackage } = await ts('package.js');
const { readFullDocument, markSectionBreaks } = await ts('fulldoc.js');
const { writeDocument } = await ts('serialize.js');

const sha = (u8) => createHash('sha256').update(u8).digest('hex').slice(0, 32);

// ── lossless.test.ts:15–49 (verbatim but for the imports) ──
function open(pkg, sources = true) {
  const original = undefined;
  const full = readFullDocument(pkg, { sources });
  markSectionBreaks(full);
  full.sources?.baseline();
  return { pkg, full, original };
}

function save(pkg, full, dirtyHf = []) {
  const last = full.sections[full.sections.length - 1];
  const hfParts = [];
  const seen = new Set();
  for (const s of full.sections) {
    for (const [kind, refs, set, blocks] of [
      ['header', s.headerRefs, s.headers, s.headerBlocks],
      ['footer', s.footerRefs, s.footers, s.footerBlocks],
    ]) {
      for (const r of refs) {
        if (seen.has(r.path) || !set[r.type]) continue;
        seen.add(r.path);
        hfParts.push({ kind, type: r.type, rid: r.rid, path: r.path, paragraphs: set[r.type], blocks: blocks?.[r.type], dirty: dirtyHf.includes(r.path) });
      }
    }
  }
  const lastRefs = [...last.headerRefs, ...last.footerRefs].map((r) => r.path);
  writeDocument(pkg, { paragraphs: paragraphsOf(full.blocks), sectPr: last.sectPr }, hfParts, {
    comments: full.comments.length ? full.comments : undefined,
    bodySectRefs: hfParts.filter((h) => lastRefs.includes(h.path)).map((h) => ({ kind: h.kind, type: h.type, rid: h.rid })),
    blocks: full.blocks,
    sources: full.sources,
  });
  return DocxPackage.load(pkg.save());
}

const paragraphsOf = (blocks) => blocks.filter((b) => b.type === 'paragraph').map((b) => b.paragraph);

const out = new CaptureWriter([
  'D1b no-edit round trip: docx-core lossless harness (open + save), parts compared decompressed',
  'facets: error | order, changed, p:<part> (short sha256 of each changed part)',
]);
let n = 0;
/** One round trip of a package (`make()` builds a fresh copy), both variants. */
function record(name, make) {
  for (const variant of ['none', 'all', 'gen']) {
    const key = `${name}#${variant}`;
    try {
      const input = make();
      const { pkg, full } = open(make(), variant !== 'gen');
      const dirty = variant !== 'none' ? full.sections.flatMap((s) => [...s.headerRefs, ...s.footerRefs].map((r) => r.path)) : [];
      const after = save(pkg, full, dirty);
      const names = after.partNames();
      out.set(key, 'order', sha(new TextEncoder().encode(names.join(String.fromCharCode(10)))));
      const changed = [];
      for (const name of names) {
        const a = after.getPart(name);
        const b = input.getPart(name);
        if (!b || a.length !== b.length || !a.every((x, i) => x === b[i])) changed.push(name);
      }
      if (changed.some((c) => /[\t\n\r|=]/.test(c))) throw new Error('part name not representable in the capture');
      out.set(key, 'changed', changed.join('|'));
      for (const c of changed) out.set(key, `p:${c}`, sha(after.getPart(c)));
    } catch (e) {
      out.set(key, 'error', 'err');
    }
    n++;
  }
}
for (const { rel, file } of corpus()) {
  const bytes = readBytes(file);
  record(rel, () => DocxPackage.load(bytes));
}
// the synthetic packages of R6 and R5 (tests/parity/sources.inputs, full.inputs), built as capture-full.mjs
// builds them: `DocxPackage.empty()` with the listed parts set over it
const fromParts = (parts) => {
  const pkg = DocxPackage.empty();
  if (!parts.some(([path]) => path === 'word/document.xml')) pkg.removePart('word/document.xml');
  for (const [path, data] of parts) pkg.setPartText(path, data);
  return pkg;
};
// `nest-*` deeper than 16 are left out: the TS signatures (`JSON.stringify` of a table or a run with a
// text box writes each nested content twice: `paragraphs`/`tables` and `blocks`) cost 2^depth there; the
// Rust signatures write one view (record, D1b decision W-S2) and tests/parity_write_roundtrip.rs runs
// those inputs for time and no panic only
const deep = (key) => { const m = /\/nest-[a-z]+-(\d+)\.docx$/.exec(key); return !!m && Number(m[1]) > 16; };
for (const { key, parts } of [...quirkSources(QUIRK_SEED, QUIRK_COUNT), ...quirkFull()]) if (!deep(key)) record(key, () => fromParts(parts));
const path = join(crate, 'tests', 'parity', 'write-roundtrip.capture');
out.write(path);
console.log(`write-roundtrip: ${n} records -> ${path}`);
