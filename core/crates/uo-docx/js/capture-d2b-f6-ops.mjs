// Parity capture of D2b-F6's pure model rules (record `core/docs/modules/uo-docx-d2b/F6.md`): the
// bookmark and link functions of the frozen editor `references.ts` (`bookmarksIn`, `findBookmark`,
// `removeBookmarkMarks`, `addBookmarkMarks`, `textBetween`, `unlinkRuns`) and the guarded
// `applyRunProps` that sets a link, over the body paragraphs of corpus documents, with the editor's
// `syncRunRPr` after every op and the JSON round trip of an undo step — as `capture-d2a-ops.mjs`.
// Node 24.15 x64.
//
//   UO_TS_ROOT=<built checkout of main> node core/crates/uo-docx/js/capture-d2b-f6-ops.mjs [--docs N] [--ops N]
//
// Writes tests/parity/d2b-f6-ops.capture: one record per `<doc>#<j>` with the facets
//   op     the op, encoded (the Rust replay runs this);
//   ret    what the op returned (bookmarks as `name:id:p:o:p:o`, a boolean, a text in hex) or `err`;
//   state  the canonical digest (js/canon.mjs) of the paragraph list after the op and the settle;
//   json   the digest of the JSON round trip of that list.
// and one `<doc>#init` record: `state`, `json` (+ `a3` for lone surrogates, compared from there on).
import { writeFileSync } from 'node:fs';
import { join } from 'node:path';
import { CaptureWriter, Rng, Stream, importFrom } from '../../uo-testkit/js/testkit.mjs';
import { Canon } from './canon.mjs';
import { crate, corpus, readBytes, root, ts } from './capture-r2-common.mjs';

const argv = process.argv.slice(2);
const opt = (k, d) => (argv.includes(k) ? Number(argv[argv.indexOf(k) + 1]) : d);
const DOCS = opt('--docs', 200);
const OPS = opt('--ops', 60);

const { DocxPackage } = await ts('package.js');
const { readFullDocument } = await ts('fulldoc.js');
const mo = await ts('model-ops.js');
const { guardRuns } = await importFrom(root, 'write/packages/editor/src/edit-guard.ts');
const refs = await importFrom(root, 'write/packages/editor/src/references.ts');

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

function digest(paras) {
  const s = new Stream();
  const c = new Canon(s);
  paras.forEach((p, i) => { s.tag('E').u32(i); c.value(p, 'paragraph'); });
  return s.finish().short;
}

const lone = (t) => /[\uD800-\uDBFF](?![\uDC00-\uDFFF])|(?<![\uD800-\uDBFF])[\uDC00-\uDFFF]/.test(t);
function hasLone(paras) {
  for (const p of paras) for (const r of p.runs) {
    if (lone(r.text)) return true;
    if (r.textbox && hasLone(r.textbox.paras)) return true;
  }
  return false;
}

const NAMES = ['Alfa', 'alfa', 'Beta', '_oculto', 'Gama1', 'é2'];
const URLS = ['https://example.com/a', 'mailto:a@b.c'];
const hex = (t) => [...Array(t.length).keys()].map((i) => t.charCodeAt(i).toString(16)).join('.');
const P = (p) => `${p.para}:${p.offset}`;
const R = (r) => `${P(r.start)}:${P(r.end)}`;

function pick(rng, m) {
  const ps = m.paragraphs;
  const para = rng.below(ps.length);
  return { para, offset: rng.below(mo.paraLength(ps[para]) + 1) };
}
function pickRange(rng, m) {
  const a = pick(rng, m);
  let b;
  const r = rng.below(10);
  if (r < 6) { const len = mo.paraLength(m.paragraphs[a.para]); b = { para: a.para, offset: Math.min(len, a.offset + rng.below(8)) }; }
  else if (r < 8) { const p2 = Math.min(m.paragraphs.length - 1, a.para + rng.below(3)); b = { para: p2, offset: rng.below(mo.paraLength(m.paragraphs[p2]) + 1) }; }
  else { b = a; }
  return rng.below(4) === 0 ? { start: b, end: a } : { start: a, end: b };
}

const encBookmarks = (list) => list.map((b) => `${hex(b.name)}:${b.id}:${P(b.start)}:${P(b.end)}`).join('|') || '-';

/** One random op: [encoded, run(m) → the returned value as text]. */
function nextOp(rng, m, nextId) {
  const k = rng.below(100);
  if (k < 28) {
    const range = pickRange(rng, m); const ni = rng.below(NAMES.length);
    // the editor drops a bookmark of that name first (insertBookmark)
    return [`ab:${R(range)}:${ni}`, () => {
      refs.removeBookmarkMarks(m.paragraphs, NAMES[ni]);
      const { start, end } = mo.normalizeRange(range);
      refs.addBookmarkMarks(m.paragraphs, NAMES[ni], nextId.n++, start, end);
      return 't';
    }];
  }
  if (k < 40) { const ni = rng.below(NAMES.length); return [`rb:${ni}`, () => (refs.removeBookmarkMarks(m.paragraphs, NAMES[ni]) ? 't' : 'f')]; }
  if (k < 52) return ['bi', () => encBookmarks(refs.bookmarksIn(m.paragraphs))];
  if (k < 60) { const ni = rng.below(NAMES.length); return [`fb:${ni}`, () => { const b = refs.findBookmark(m.paragraphs, NAMES[ni]); return b ? encBookmarks([b]) : 'u'; }]; }
  if (k < 70) {
    const range = pickRange(rng, m);
    return [`tb:${R(range)}`, () => { const { start, end } = mo.normalizeRange(range); return hex(refs.textBetween(m.paragraphs, start, end)) || '-'; }];
  }
  if (k < 84) {
    const range = pickRange(rng, m); const ui = rng.below(URLS.length);
    return [`lk:${R(range)}:${ui}`, () => {
      const r = mo.normalizeRange(range);
      guardRuns(m, r.start.para, r.end.para, () => mo.applyRunProps(m, range, { link: URLS[ui], charStyle: 'Hyperlink' }));
      return 'u';
    }];
  }
  if (k < 94) {
    const range = pickRange(rng, m);
    return [`ul:${R(range)}`, () => {
      const { start, end } = mo.normalizeRange(range);
      guardRuns(m, start.para, end.para, () => {
        for (let pi = start.para; pi <= end.para; pi++) {
          const p = m.paragraphs[pi];
          const from = pi === start.para ? start.offset : 0;
          const to = pi === end.para ? end.offset : mo.paraLength(p);
          refs.unlinkRuns(p, from, to, 'Hyperlink');
        }
      });
      return 'u';
    }];
  }
  return ['rt', () => { m.paragraphs = roundTrip(m.paragraphs); return 't'; }];
}

const out = new CaptureWriter([
  'D2b-F6 pure model rules: editor references.ts bookmark/link functions through edit-guard, syncRunRPr after each, JSON round trip (undo) digest',
  'facets: op (encoded), ret, state, json; init record: state, json, a3',
]);
out.meta('ops', OPS);
const files = corpus().filter((_, i) => i % 5 === 1).slice(0, DOCS);
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
  out.set(`${rel}#init`, 'state', digest(m.paragraphs));
  out.set(`${rel}#init`, 'json', digest(roundTrip(m.paragraphs)));
  if (a3) out.set(`${rel}#init`, 'a3', '1');
  let seed = 0xf6;
  for (let i = 0; i < rel.length; i++) seed = (Math.imul(seed, 31) + rel.charCodeAt(i)) >>> 0;
  const rng = new Rng(seed);
  const nextId = { n: refs.maxBookmarkId(m.paragraphs, []) + 1 };
  out.set(`${rel}#init`, 'next', String(nextId.n));
  for (let j = 0; j < OPS; j++) {
    const [code, run] = nextOp(rng, m, nextId);
    const key = `${rel}#${j}`;
    out.set(key, 'op', code);
    let ret;
    try { ret = run(); } catch { ret = 'err'; }
    syncRunRPr(m.paragraphs);
    out.set(key, 'ret', ret);
    out.set(key, 'state', digest(m.paragraphs));
    out.set(key, 'json', digest(roundTrip(m.paragraphs)));
    if (ret === 'err') break;
  }
  n++;
}
out.write(join(crate, 'tests', 'parity', 'd2b-f6-ops.capture'));
console.log(`d2b-f6-ops: ${n} documents x ${OPS} ops`);
