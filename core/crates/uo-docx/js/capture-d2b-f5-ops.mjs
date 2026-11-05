// Parity capture of D2b-F5's pure model rules (record `core/docs/modules/uo-docx-d2b/F5.md`): the
// functions of the frozen editor `notes.ts` (`effectiveNoteOptions`, `notePropsOf`,
// `ensureNoteStyles`, `nextNoteId`, `convertNoteContent`, `cloneBlocks`) over the notes, settings
// and styles of corpus documents, and `notePropsOf` over generated options. Node 24.15 x64.
// (The emitters of docx-core `notes.ts` are W4's and have their own capture.)
//
//   UO_TS_ROOT=<built checkout of main> node core/crates/uo-docx/js/capture-d2b-f5-ops.mjs [--docs N]
//
// Writes tests/parity/d2b-f5-ops.capture. Per document `<doc>#init`:
//   nid         `nextNoteId` of both kinds;
//   eff         `effectiveNoteOptions` of both kinds from the settings' and the last section's props;
//   sty         `ensureNoteStyles` of both kinds, the second on the sheet the first changed (as
//               `convertNotes` does): the short SHA of each text and the ids;
// and per note `<doc>#cv:<kind>:<id>` the canonical digest of the note's top-level paragraphs after
// `convertNoteContent` to the other kind. Records `props#<i>`: `op` (the options) and `ret` (the
// JSON of `notePropsOf`, `-` for undefined).
import { createHash } from 'node:crypto';
import { join } from 'node:path';
import { CaptureWriter, Rng, Stream, importFrom } from '../../uo-testkit/js/testkit.mjs';
import { Canon } from './canon.mjs';
import { crate, corpus, readBytes, root, ts } from './capture-r2-common.mjs';

const argv = process.argv.slice(2);
const opt = (k, d) => (argv.includes(k) ? Number(argv[argv.indexOf(k) + 1]) : d);
const DOCS = opt('--docs', 400);
const sha = (s) => createHash('sha256').update(Buffer.from(s, 'utf8')).digest('hex').slice(0, 32);

const { DocxPackage } = await ts('package.js');
const { readFullDocument } = await ts('fulldoc.js');
const nt = await importFrom(root, 'write/packages/editor/src/notes.ts');
const st = await importFrom(root, 'write/packages/editor/src/styles.ts');

function digest(paras) {
  const s = new Stream();
  const c = new Canon(s);
  paras.forEach((p, i) => { s.tag('E').u32(i); c.value(p, 'paragraph'); });
  return s.finish().short;
}
const optText = (o) => `${o.position},${o.numberFormat},${o.startAt},${o.restart},${o.columns ?? '-'}`;

const out = new CaptureWriter([
  'D2b-F5 pure model rules: editor notes.ts (effectiveNoteOptions, notePropsOf, ensureNoteStyles, nextNoteId, convertNoteContent) over corpus notes, settings and styles',
  'facets: init (nid, eff, sty), cv:<kind>:<id> (digest), props#i (op, ret)',
]);

// ── notePropsOf over generated options ──
const POS = ['pageBottom', 'beneathText', 'docEnd', 'sectEnd', 'x'];
const FMT = ['decimal', 'lowerRoman', 'upperRoman', 'lowerLetter', 'chicago', ''];
const START = [1, 2, 5, 0, -3, 2.5, NaN, 1e300, Infinity];
const RESTART = ['continuous', 'eachSect', 'eachPage', 'x'];
const rng0 = new Rng(0xf5);
for (let i = 0; i < 600; i++) {
  const kind = rng0.below(2) ? 'footnote' : 'endnote';
  const o = {
    position: POS[rng0.below(POS.length)], numberFormat: FMT[rng0.below(FMT.length)],
    startAt: START[rng0.below(START.length)], restart: RESTART[rng0.below(RESTART.length)],
  };
  const withPos = rng0.below(2) === 0;
  const p = nt.notePropsOf(kind, o, withPos);
  out.set(`props#${i}`, 'op', `${kind}|${o.position}|${o.numberFormat}|${Object.is(o.startAt, NaN) ? 'NaN' : o.startAt}|${o.restart}|${withPos ? 1 : 0}`);
  out.set(`props#${i}`, 'ret', p ? JSON.stringify(p) : '-');
}

const files = corpus().filter((_, i) => i % 3 === 0).slice(0, DOCS);
let n = 0;
for (const { rel, file } of files) {
  let pkg;
  let fd;
  try { pkg = DocxPackage.load(readBytes(file)); fd = readFullDocument(pkg, { sources: true }); } catch { continue; }
  const key = `${rel}#init`;
  out.set(key, 'nid', `${nt.nextNoteId(fd, 'footnote')},${nt.nextNoteId(fd, 'endnote')}`);
  const s = fd.formatting?.settings ?? {};
  const sp = fd.sections.length ? fd.sections[fd.sections.length - 1].sectPr : (fd.sectPr ?? {});
  out.set(key, 'eff', [
    optText(nt.effectiveNoteOptions('footnote', s.footnotePr, sp.footnotePr, sp.footnoteColumns)),
    optText(nt.effectiveNoteOptions('endnote', s.endnotePr, sp.endnotePr, undefined)),
  ].join('#'));
  // the sheets, as `convertNotes` computes them
  const xml0 = pkg.getPartText('word/styles.xml') ?? null;
  const ds0 = new st.DocStyles(xml0);
  const fn = nt.ensureNoteStyles(ds0, 'footnote');
  const ds1 = new st.DocStyles(fn.xml);
  const en = nt.ensureNoteStyles(ds1, 'endnote');
  out.set(key, 'sty', `${sha(fn.xml)},${fn.text},${fn.ref}#${sha(en.xml)},${en.text},${en.ref}`);
  const styles = { footnote: fn, endnote: en };
  for (const kind of ['footnote', 'endnote']) {
    const map = kind === 'footnote' ? fd.footnotes : fd.endnotes;
    let k = 0;
    for (const id of [...map.keys()].filter((i) => i > 0).slice(0, 4)) {
      const blocks = nt.cloneBlocks(nt.noteBlocks(fd, kind, id) ?? []);
      const to = kind === 'footnote' ? 'endnote' : 'footnote';
      nt.convertNoteContent(blocks, to, { from: styles[kind], to: styles[to] });
      out.set(`${rel}#cv:${kind}:${id}`, 'digest', digest(blocks.filter((b) => b.type === 'paragraph').map((b) => b.paragraph)));
      k++;
    }
  }
  n++;
}
out.write(join(crate, 'tests', 'parity', 'd2b-f5-ops.capture'));
console.log(`d2b-f5-ops: ${n} documents`);
