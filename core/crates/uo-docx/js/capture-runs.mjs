// Parity capture of the D1b frontier W1 (serialize.ts: `runXml`, `fieldOrRunXml`, `nestedFieldXml`,
// `fieldCodeXml`, `revisionRuns`, `genRuns`, `paraXmlInner`, `textXml`, `withNoProof`, `linkRelId` of the
// generating path), the frozen TS `writeDocument` over:
//
//  1. `<docx>#p<i>`: single paragraphs of corpus documents read WITHOUT sources (the path with no
//     source: `serializeBlocks` of a one-paragraph body into an empty package), picked by the features
//     they exercise (fields, revisions, custom inlines, note marks, borders, tabs, …; up to CAP per
//     feature, plus a thin sample of plain ones);
//  2. `quirk/runs/<seed>-<n>.docx#<variant>`: synthetic packages (js/quirk-runs.mjs, tests/parity/
//     runs.inputs), whole documents, in three variants:
//       `gen`   read without sources (the generating path, `active` off)
//       `actr`  read with sources, the `props.src` of every run of the body paragraphs cleared (new
//               runs in a source paragraph: the generated runs of the lossless save, `active` on:
//               hyperlinks, `ptab`, `sym`, math)
//       `actp`  as `actr` and the paragraphs' own `props.src` cleared too (new paragraphs: the
//               `pPr` from the props, `genRuns` of every run)
//  3. `<docx>#actr` / `<docx>#actp` of every 9th corpus document, the same.
//
// Facets as the round trip's: `error` (`err` when the TS throws), `order`, `changed` (the parts that
// differ from the input) and `p:<part>` (short SHA-256 of each changed part, decompressed).
//
//   UO_TS_ROOT=<built checkout of main> node core/crates/uo-docx/js/capture-runs.mjs
//
// Writes tests/parity/runs.capture and tests/parity/runs.inputs; the replay is tests/parity_runs_w1.rs.
import { writeFileSync } from 'node:fs';
import { join } from 'node:path';
import { createHash } from 'node:crypto';
import { CaptureWriter } from '../../uo-testkit/js/testkit.mjs';
import { corpus, crate, readBytes, ts } from './capture-r2-common.mjs';
import { quirkRuns, runsInputsText } from './quirk-runs.mjs';

const { DocxPackage } = await ts('package.js');
const { readFullDocument, markSectionBreaks } = await ts('fulldoc.js');
const { writeDocument } = await ts('serialize.js');

const sha = (u8) => createHash('sha256').update(u8).digest('hex').slice(0, 32);
const SECT = { pageWidthTwips: 12240, pageHeightTwips: 15840, marginTopTwips: 1440, marginRightTwips: 1440, marginBottomTwips: 1440, marginLeftTwips: 1440 };
const CAP = 30;
const PER_DOC = 12;
const PLAIN_EVERY = 40;
const ACT_EVERY = 9;

/** Facets of a saved package against its input (as capture-write-roundtrip.mjs). */
function setFacets(out, key, input, after) {
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
}

// ── lossless.test.ts:15–49 (verbatim but for the imports and `mutate`) ──
function open(pkg, sources = true) {
  const full = readFullDocument(pkg, { sources });
  markSectionBreaks(full);
  full.sources?.baseline();
  return { pkg, full };
}
const paragraphsOf = (blocks) => blocks.filter((b) => b.type === 'paragraph').map((b) => b.paragraph);
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

/** The mutation of the `act*` variants: new runs (and, for `actp`, new paragraphs) in the body. */
function mutate(full, variant) {
  for (const p of paragraphsOf(full.blocks)) {
    for (const r of p.runs) {
      r.props = { ...r.props };
      delete r.props.src;
    }
    if (variant === 'actp') {
      p.props = { ...p.props };
      delete p.props.src;
    }
  }
}

const out = new CaptureWriter([
  'D1b W1: generated runs, fields, revisions, hyperlinks and paragraphs (docx-core serialize.ts, the generating path)',
  'facets: error | order, changed, p:<part> (short sha256 of each changed part)',
]);
let n = 0;

/** One whole-document round trip in a variant. */
function recordDoc(key, make, variant) {
  try {
    const input = make();
    const { pkg, full } = open(make(), variant === 'gen' ? false : true);
    if (variant !== 'gen') mutate(full, variant);
    const after = save(pkg, full, []);
    setFacets(out, key, input, after);
  } catch {
    out.set(key, 'error', 'err');
  }
  n++;
}

// ── 1. paragraphs of the corpus, by feature ──
const RUN_KEYS = ['field', 'fieldCode', 'fieldResult', 'custom', 'noteRef', 'ptab', 'symFont', 'math', 'bookmark', 'fieldBegin', 'fieldEnd', 'image', 'textbox', 'shape', 'group', 'formField'];
const RUN_PROPS = ['ins', 'del', 'link', 'linkTip', 'commentIds', 'isMath', 'fmt', 'charStyle', 'noProof', 'hidden'];
const PARA_KEYS = ['styleId', 'numbering', 'borders', 'boxBorder', 'shadingHex', 'tabStops', 'markIns', 'markDel', 'pfmt', 'sectionBreak', 'spaceBeforeAuto', 'spaceAfterAuto',
  'indentLeftTwips', 'indentRightTwips', 'indentFirstLineTwips', 'keepNext', 'keepLines', 'pageBreakBefore', 'widowControl', 'suppressLineNumbers', 'suppressAutoHyphens',
  'lineTwentieths', 'spaceAfterTwips', 'spaceBeforeTwips', 'alignment', 'textboxTightWrap', 'blockSdt'];
const TEXT_CHARS = [['\t', 'tab'], ['\n', 'nl'], ['\f', 'ff'], ['\u000B', 'vt'], ['‑', 'nbh'], ['­', 'shy'], [' ', 'ls'], ['&', 'amp'], ['<', 'lt'], ['  ', 'spaces'], ['￼', 'obj']];
function featuresOf(p) {
  const f = new Set();
  for (const k of PARA_KEYS) if (p.props[k] !== undefined) f.add(`p:${k}`);
  if (p.props.numbering && p.props.numbering.numId === undefined) f.add('p:numbering-gen');
  if (p.props.borders?.bar) f.add('p:bar');
  if (p.props.indentFirstLineTwips < 0) f.add('p:hanging');
  for (const r of p.runs) {
    for (const k of RUN_KEYS) if (r[k] !== undefined) f.add(`r:${k}`);
    for (const k of RUN_PROPS) if (r.props[k] !== undefined) f.add(`rp:${k}`);
    if (r.props.ins && r.props.del) f.add('rp:insdel');
    if (r.field && /^\s*=/.test(r.field)) f.add('r:formula');
    if (r.field && /\\h\b/i.test(r.field)) f.add('r:field-h');
    if (r.field && /^\s*(TC|XE|RD)\b/i.test(r.field)) f.add('r:field-marker');
    if (r.field && (r.fieldResult?.length ?? 0) > 1) f.add('r:field-multi');
    for (const [c, name] of TEXT_CHARS) if (r.text.includes(c)) f.add(`t:${name}`);
    if (r.text !== r.text.trim()) f.add('t:trim');
  }
  return [...f];
}
const seen = new Map();
let plain = 0;
let picked = 0;
let docIndex = 0;
for (const { rel, file } of corpus()) {
  docIndex++;
  let pkg;
  let full;
  try {
    pkg = DocxPackage.load(readBytes(file));
    full = readFullDocument(pkg, { sources: false });
  } catch { continue; }
  const paras = paragraphsOf(full.blocks);
  const takeAct = docIndex % ACT_EVERY === 0;
  let taken = 0;
  for (let i = 0; i < paras.length && taken < PER_DOC; i++) {
    const p = paras[i];
    const feats = featuresOf(p);
    const fresh = feats.filter((x) => (seen.get(x) ?? 0) < CAP);
    const take = fresh.length > 0 || (feats.length === 0 && ++plain % PLAIN_EVERY === 0);
    if (!take) continue;
    for (const x of feats) seen.set(x, (seen.get(x) ?? 0) + 1);
    taken++;
    picked++;
    const key = `${rel}#p${i}`;
    try {
      const input = DocxPackage.empty();
      const pkg2 = DocxPackage.empty();
      writeDocument(pkg2, { paragraphs: [p], sectPr: SECT }, [], { blocks: [{ type: 'paragraph', paragraph: p }] });
      setFacets(out, key, input, pkg2);
    } catch {
      out.set(key, 'error', 'err');
    }
    n++;
  }
  if (takeAct) {
    const bytes = readBytes(file);
    for (const variant of ['actr', 'actp']) recordDoc(`${rel}#${variant}`, () => DocxPackage.load(bytes), variant);
  }
}

// ── 2. the synthetic packages ──
const inputs = quirkRuns();
const fromParts = (parts) => {
  const pkg = DocxPackage.empty();
  for (const [path, data] of parts) pkg.setPartText(path, data);
  return pkg;
};
for (const { key, parts } of inputs) for (const variant of ['gen', 'actr', 'actp']) recordDoc(`${key}#${variant}`, () => fromParts(parts), variant);

writeFileSync(join(crate, 'tests', 'parity', 'runs.inputs'), runsInputsText(inputs));
const path = join(crate, 'tests', 'parity', 'runs.capture');
out.write(path);
console.log(`runs: ${picked} corpus paragraphs, ${n} records -> ${path}`);
