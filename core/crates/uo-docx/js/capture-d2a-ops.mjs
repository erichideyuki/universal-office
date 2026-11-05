// Parity capture of D2a's text operations (record uo-docx.md "D2a — session (architecture)",
// "Parity"): the frozen TS model ops the editor calls (docx-core model-ops.ts and revisions.ts,
// through the editor's guard, editor/src/edit-guard.ts, exactly as universal-write.ts 899-931 and
// 8253 wrap them), the editor's `syncRunRPr` after every op (the settle step, with its two symbol
// keys) and the JSON round trip of an undo step (`JSON.parse(JSON.stringify(state))` with the
// image pool), over the body paragraphs of corpus documents. Node 24.15 x64 (no math in these
// paths: G17 needs Chromium only for the element).
//
//   UO_TS_ROOT=<built checkout of main> node core/crates/uo-docx/js/capture-d2a-ops.mjs [--docs N] [--ops N]
//
// Writes tests/parity/d2a-ops.capture: one record per `<doc>#<j>` (j = op index) with the facets
//   op     the op, encoded (the Rust replay runs this, not its own generator);
//   ret    what the op returned (a position, a boolean, the merged list) or `err` when the TS threw;
//   state  the canonical digest (js/canon.mjs, A6) of the paragraph list after the op and the settle;
//   json   the digest of the JSON round trip of that list (what an undo restores, U4).
// and one `<doc>#init` record: `state` and `json` of the list as read (+ `a3` when the document holds
// lone surrogates from character references: R1's named difference, compared only from there on).
// The ops are seeded (xorshift32 per document, uo-testkit `Rng`), positions chosen from the TS state.
import { writeFileSync } from 'node:fs';
import { join } from 'node:path';
import { CaptureWriter, Rng, Stream, importFrom } from '../../uo-testkit/js/testkit.mjs';
import { Canon } from './canon.mjs';
import { crate, corpus, readBytes, root, ts } from './capture-r2-common.mjs';

const argv = process.argv.slice(2);
const opt = (k, d) => (argv.includes(k) ? Number(argv[argv.indexOf(k) + 1]) : d);
const DOCS = opt('--docs', 240);
const OPS = opt('--ops', 60);

const { DocxPackage } = await ts('package.js');
const { readFullDocument } = await ts('fulldoc.js');
const mo = await ts('model-ops.js');
const rv = await ts('revisions.js');
const { guardRuns } = await importFrom(root, 'write/packages/editor/src/edit-guard.ts');

// the editor's guarded ops (universal-write.ts 899-931)
const G = {
  insertText: (m, pos, text, props) => guardRuns(m, pos.para, pos.para, () => mo.insertText(m, pos, text, props)),
  deleteRange: (m, range) => { const r = mo.normalizeRange(range); return guardRuns(m, r.start.para, r.end.para, () => mo.deleteRange(m, range)); },
  splitParagraph: (m, pos) => guardRuns(m, pos.para, pos.para, () => mo.splitParagraph(m, pos)),
  mergeWithPrevious: (m, para) => guardRuns(m, para - 1, para, () => mo.mergeWithPrevious(m, para)),
  applyRunProps: (m, range, patch) => { const r = mo.normalizeRange(range); guardRuns(m, r.start.para, r.end.para, () => mo.applyRunProps(m, range, patch)); },
  trackInsertText: (m, pos, text, rev, props) => guardRuns(m, pos.para, pos.para, () => rv.trackInsertText(m, pos, text, rev, props)),
  trackSplitParagraph: (m, pos, rev) => guardRuns(m, pos.para, pos.para, () => rv.trackSplitParagraph(m, pos, rev)),
  trackDelete: (m, range, rev) => { const r = mo.normalizeRange(range); return guardRuns(m, r.start.para, r.end.para, () => rv.trackDelete(m, range, rev)); },
  trackApplyRunProps: (m, range, patch, rev) => { const r = mo.normalizeRange(range); guardRuns(m, r.start.para, r.end.para, () => rv.trackApplyRunProps(m, range, patch, rev)); },
  // applyRevisionScope (8248): the whole list guarded
  revisions: (m, scope, accept) => guardRuns(m, 0, m.paragraphs.length - 1, () => (accept ? rv.acceptRevisions : rv.rejectRevisions)(m.paragraphs, scope, () => {})),
};

// syncRunRPr (universal-write.ts 16068) with its symbol keys
const RPR_REF = Symbol('rprRef');
const RPR_EDITED = Symbol('rprEdited');
function syncRunRPr(paras) {
  for (const p of paras) {
    for (const r of p.runs) {
      const props = r.props;
      if (r.rpr) {
        if (props[RPR_REF] !== r.rpr) props[RPR_REF] = r.rpr;
        if (r.rprEdited) props[RPR_EDITED] = true; else if (props[RPR_EDITED]) r.rprEdited = true;
      } else if (props[RPR_REF]) { r.rpr = props[RPR_REF]; if (props[RPR_EDITED]) r.rprEdited = true; }
      if (r.textbox) syncRunRPr(r.textbox.paras);
    }
  }
}

// the undo JSON round trip with the image pool (stringifyState / parseState)
function roundTrip(paras) {
  const pool = [];
  const idx = new Map();
  const json = JSON.stringify(paras, (k, v) => {
    if (v instanceof Uint8Array) {
      let i = idx.get(v);
      if (i === undefined) { i = pool.length; pool.push(v); idx.set(v, i); }
      return { __imgPool: i };
    }
    return v;
  });
  return JSON.parse(json, (k, v) => (v && typeof v === 'object' && '__imgPool' in v ? pool[v.__imgPool] : v));
}

function digest(paras, traceTo) {
  const s = new Stream({ trace: !!traceTo });
  const c = new Canon(s);
  paras.forEach((p, i) => { s.tag('E').u32(i); c.value(p, 'paragraph'); });
  if (traceTo) writeFileSync(traceTo, s.trace());
  return s.finish().short;
}
const traceKey = argv.includes('--trace') ? argv[argv.indexOf('--trace') + 1] : undefined;
const traceDir = process.env.UO_PARITY_TRACE_DIR ?? '.';

const lone = (t) => /[\uD800-\uDBFF](?![\uDC00-\uDFFF])|(?<![\uD800-\uDBFF])[\uDC00-\uDFFF]/.test(t);
function hasLone(paras) {
  for (const p of paras) for (const r of p.runs) {
    if (lone(r.text)) return true;
    if (r.textbox && hasLone(r.textbox.paras)) return true;
  }
  return false;
}

const TEXTS = ['a', 'Olá ', ' ', 'x', 'ção', '😀', '𝒜!', 'abc def', '\t', ' '];
const PATCHES = { b: { bold: true }, i: { italic: true }, s: { szHalfPt: 28 }, u: { underline: undefined }, c: { color: 'FF0000' }, n: { bold: false, italic: undefined } };
const PK = Object.keys(PATCHES);
const REVS = [
  { author: 'A', date: '2026-01-02T03:04:00Z', dateUtc: '2026-01-02T03:04:00Z' },
  { author: 'B', date: '2026-01-02T05:06:00Z', dateUtc: '2026-01-02T08:06:00Z' },
];
const hex = (t) => [...Array(t.length).keys()].map((i) => t.charCodeAt(i).toString(16)).join('.');

function pick(rng, m) {
  const ps = m.paragraphs;
  const para = rng.below(ps.length);
  const len = mo.paraLength(ps[para]);
  return { para, offset: rng.below(len + 1) };
}
function pickRange(rng, m) {
  const a = pick(rng, m);
  // mostly short ranges inside a paragraph, sometimes across paragraphs, sometimes reversed
  let b;
  const r = rng.below(10);
  if (r < 6) { const len = mo.paraLength(m.paragraphs[a.para]); b = { para: a.para, offset: Math.min(len, a.offset + rng.below(8)) }; }
  else if (r < 8) { const p2 = Math.min(m.paragraphs.length - 1, a.para + rng.below(3)); b = { para: p2, offset: rng.below(mo.paraLength(m.paragraphs[p2]) + 1) }; }
  else { b = a; }
  return rng.below(4) === 0 ? { start: b, end: a } : { start: a, end: b };
}
const P = (p) => `${p.para}:${p.offset}`;
const R = (r) => `${P(r.start)}:${P(r.end)}`;

/** One random op: [encoded, run(m) → returned value]. */
function nextOp(rng, m) {
  const k = rng.below(100);
  const rev = REVS[rng.below(2)];
  const ri = REVS.indexOf(rev);
  if (k < 22) {
    const pos = pick(rng, m); const t = TEXTS[rng.below(TEXTS.length)];
    return [`it:${P(pos)}:${hex(t)}`, () => G.insertText(m, pos, t)];
  }
  if (k < 34) {
    // a one-unit Backspace (lone surrogates from emoji, U2) or a short range
    const pos = pick(rng, m);
    const range = pos.offset > 0 && rng.below(2) === 0 ? { start: { para: pos.para, offset: pos.offset - 1 }, end: pos } : pickRange(rng, m);
    return [`dr:${R(range)}`, () => G.deleteRange(m, range)];
  }
  if (k < 42) { const pos = pick(rng, m); return [`sp:${P(pos)}`, () => G.splitParagraph(m, pos)]; }
  if (k < 47) {
    if (m.paragraphs.length < 2) return nextOp(rng, m);
    const para = 1 + rng.below(m.paragraphs.length - 1);
    return [`mg:${para}`, () => G.mergeWithPrevious(m, para)];
  }
  if (k < 55) { const range = pickRange(rng, m); const pk = PK[rng.below(PK.length)]; return [`rp:${R(range)}:${pk}`, () => G.applyRunProps(m, range, { ...PATCHES[pk] })]; }
  if (k < 58) { const range = pickRange(rng, m); return [`cr:${R(range)}`, () => mo.clearRunFormatting(m, range)]; }
  if (k < 60) { const range = pickRange(rng, m); return [`cp:${R(range)}`, () => mo.clearParaFormatting(m, range)]; }
  if (k < 70) {
    const pos = pick(rng, m); const t = TEXTS[rng.below(TEXTS.length)];
    return [`ti:${P(pos)}:${hex(t)}:${ri}`, () => G.trackInsertText(m, pos, t, rev)];
  }
  if (k < 78) { const range = pickRange(rng, m); return [`td:${R(range)}:${ri}`, () => G.trackDelete(m, range, rev)]; }
  if (k < 82) { const pos = pick(rng, m); return [`ts:${P(pos)}:${ri}`, () => G.trackSplitParagraph(m, pos, rev)]; }
  if (k < 87) { const range = pickRange(rng, m); const pk = PK[rng.below(PK.length)]; return [`tr:${R(range)}:${pk}:${ri}`, () => G.trackApplyRunProps(m, range, { ...PATCHES[pk] }, rev)]; }
  if (k < 94) {
    const spans = rv.collectRevisions(m.paragraphs);
    const accept = rng.below(2) === 0;
    if (!spans.length || rng.below(4) === 0) return [`${accept ? 'aa' : 'ra'}`, () => G.revisions(m, {}, accept)];
    const i = rng.below(spans.length);
    return [`${accept ? 'ac' : 'rj'}:${i}`, () => G.revisions(m, rv.spanScope(spans[i]), accept)];
  }
  // an undo step restored: the list becomes its JSON copy (U4: cleared keys, NaN, symbols, ids)
  return ['rt', () => { m.paragraphs = roundTrip(m.paragraphs); return true; }];
}

const enc = (v) => {
  if (v === undefined) return 'u';
  if (typeof v === 'boolean') return v ? 't' : 'f';
  if (v && typeof v === 'object' && 'para' in v) return P(v);
  if (v && typeof v === 'object' && 'merged' in v) return `${P(v.start)}:${P(v.end)}:${v.merged.join('.')}`;
  return 'x';
};

const out = new CaptureWriter([
  'D2a text ops: docx-core model-ops/revisions through edit-guard, syncRunRPr after each, JSON round trip (undo) digest',
  'facets: op (encoded), ret, state, json; init record: state, json, a3',
]);
out.meta('ops', OPS);
const files = corpus().filter((_, i) => i % 7 === 0).slice(0, DOCS);
let n = 0;
for (const { rel, file } of files) {
  let full;
  try { full = readFullDocument(DocxPackage.load(readBytes(file)), { sources: true }); } catch { continue; }
  const paragraphs = [];
  for (const b of full.blocks) if (b.type === 'paragraph') paragraphs.push(b.paragraph);
  if (!paragraphs.length) paragraphs.push({ runs: [], props: {} });
  const m = { paragraphs, sectPr: {} };
  syncRunRPr(m.paragraphs);
  const a3 = hasLone(m.paragraphs);
  const tr = traceKey === rel;
  out.set(`${rel}#init`, 'state', digest(m.paragraphs, tr && join(traceDir, 'ts-d2a-state.trace')));
  out.set(`${rel}#init`, 'json', digest(roundTrip(m.paragraphs), tr && join(traceDir, 'ts-d2a-json.trace')));
  if (a3) out.set(`${rel}#init`, 'a3', '1');
  let seed = 0x2d2a;
  for (let i = 0; i < rel.length; i++) seed = (Math.imul(seed, 31) + rel.charCodeAt(i)) >>> 0;
  const rng = new Rng(seed);
  for (let j = 0; j < OPS; j++) {
    const [code, run] = nextOp(rng, m);
    const key = `${rel}#${j}`;
    out.set(key, 'op', code);
    let ret;
    try { ret = enc(run()); } catch { ret = 'err'; }
    syncRunRPr(m.paragraphs);
    out.set(key, 'ret', ret);
    const tj = tr && traceKey && process.env.UO_TRACE_OP === String(j);
    out.set(key, 'state', digest(m.paragraphs, tj && join(traceDir, 'ts-d2a-op-state.trace')));
    out.set(key, 'json', digest(roundTrip(m.paragraphs), tj && join(traceDir, 'ts-d2a-op-json.trace')));
    if (ret === 'err') break;
  }
  n++;
}
if (!traceKey) out.write(join(crate, 'tests', 'parity', 'd2a-ops.capture'));
console.log(`d2a-ops: ${n} documents x ${OPS} ops`);
