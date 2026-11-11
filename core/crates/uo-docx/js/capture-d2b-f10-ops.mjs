// Parity capture of D2b-F10's pure model files (record `core/docs/modules/uo-docx-d2b/F10.md`): the
// frozen editor's `references.ts` (parseTocInstr/tocInstr, crossRefKinds/crossRefInstr,
// refFieldTarget, captionRange, captionFields, seqLabel, tocEntryParagraphs, hiddenBookmarkName,
// runsBetween), `toc.ts` (tocFormatStyle, parseTcInstr, tcInstr, tcInToc, tocOptionsFrom,
// dialogOptionsFrom, addTextPlan), `text-layout.ts` (wordAround, manualPoints, offsetAfterLetters)
// and `hf-templates.ts` DATE_TIME_FORMATS with docx-layout's `formatWordDate` (dateTimeFormats),
// over seeded inputs: grammars plus hostile cases, and the body paragraphs of corpus documents.
//
//   UO_TS_ROOT=<built checkout> node --experimental-transform-types --no-warnings \
//     core/crates/uo-docx/js/capture-d2b-f10-ops.mjs [--docs N]
//
// Writes tests/parity/d2b-f10-ops.capture. Keys `pure#<code><j>` (facets `op`, `ret`) and
// `<doc>#rb<j>` (runsBetween). Every string is UTF-16 in hex (`.`-joined), numbers as JS prints them.
process.env.TZ = 'UTC';
import { join } from 'node:path';
import { CaptureWriter, Rng, Stream, importFrom } from '../../uo-testkit/js/testkit.mjs';
import { Canon } from './canon.mjs';
import { crate, corpus, readBytes, root, ts } from './capture-r2-common.mjs';

const argv = process.argv.slice(2);
const opt = (k, d) => (argv.includes(k) ? Number(argv[argv.indexOf(k) + 1]) : d);
const DOCS = opt('--docs', 60);

const { DocxPackage } = await ts('package.js');
const { readFullDocument } = await ts('fulldoc.js');
const refs = await importFrom(root, 'write/packages/editor/src/references.ts');
const toc = await importFrom(root, 'write/packages/editor/src/toc.ts');
const tl = await importFrom(root, 'write/packages/editor/src/text-layout.ts');
const hf = await importFrom(root, 'write/packages/editor/src/hf-templates.ts');
const { formatWordDate } = await importFrom(root, 'write/packages/docx-layout/dist/datefmt.js');

const hex = (t) => [...Array(t.length).keys()].map((i) => t.charCodeAt(i).toString(16)).join('.');
const unhex = (h) => (h === '' ? '' : h.split('.').map((x) => String.fromCharCode(parseInt(x, 16))).join(''));
const num = (n) => String(n);

function digest(paras) {
  const s = new Stream();
  const c = new Canon(s);
  paras.forEach((p, i) => { s.tag('E').u32(i); c.value(p, 'paragraph'); });
  return s.finish().short;
}

/** The canonical text of a TocOptions (fixed key order; absent keys and empty `styles` omitted). */
function canonOpts(o) {
  const parts = [];
  if (o.outline) parts.push(`o=${o.outline[0]},${o.outline[1]}`);
  if (o.outlineLevels) parts.push('u');
  if (o.styles?.length) parts.push(`t=${o.styles.map((s) => `${hex(s.style)}:${s.level}`).join(',')}`);
  if (o.caption !== undefined) parts.push(`c=${hex(o.caption)}`);
  if (o.includeLabel !== undefined) parts.push(`il=${o.includeLabel ? 1 : 0}`);
  if (o.fields) parts.push('f');
  if (o.fieldId !== undefined) parts.push(`fid=${hex(o.fieldId)}`);
  if (o.fieldLevels) parts.push(`l=${o.fieldLevels[0]},${o.fieldLevels[1]}`);
  if (o.hyperlinks) parts.push('h');
  if (o.noPageNumbers) parts.push('n');
  if (o.noPageLevels) parts.push(`nl=${o.noPageLevels[0]},${o.noPageLevels[1]}`);
  if (o.separator !== undefined) parts.push(`p=${hex(o.separator)}`);
  if (o.hideInWeb) parts.push('z');
  return parts.join(';') || '-';
}

/** The canonical text of the dialog options. */
function canonDialog(d) {
  const f = (k, v) => (v === undefined ? null : `${k}=${typeof v === 'boolean' ? (v ? 1 : 0) : typeof v === 'number' ? v : typeof v === 'string' ? hex(v) : v}`);
  return [
    f('pn', d.pageNumbers), f('ra', d.rightAlign), f('hy', d.hyperlinks), f('lv', d.levels), f('us', d.useStyles),
    d.styles === undefined ? null : `st=${d.styles.map((s) => `${hex(s.style)}:${s.level}`).join(',')}`,
    f('ol', d.outlineLevels), f('fl', d.fields), f('fi', d.fieldId),
  ].filter((x) => x !== null).join(';') || '-';
}

const rng = new Rng(0xf10);
const out = new CaptureWriter([
  'D2b-F10 pure model files: editor references.ts, toc.ts, text-layout.ts, hf-templates.ts + docx-layout formatWordDate',
  'facets: op (encoded), ret; <doc>#rb<j>: runsBetween over corpus paragraphs',
]);
let j = 0;
const rec = (code, op, ret) => { const key = `pure#${code}${j++}`; out.set(key, 'op', `${code}:${op}`); out.set(key, 'ret', ret); };

// ── instructions ─────────────────────────────────────────────────────────────────────────────
const SW = ['\\o "1-3"', '\\o', '\\u', '\\h', '\\z', '\\n', '\\n "2-3"', '\\p " "', '\\p ""', '\\f', '\\f x', '\\f "a b"', '\\f ""', '\\l "1-2"', '\\t "Heading 1;1;Heading 2;2"', '\\t "a,1,b"', '\\t ";"',
  '\\c "Figure"', '\\a "Table"', '\\x', '\\O', '\\U', '\\Z', '\\H', '\\p', '\\o"1-2"', '\\o  "1 - 2"', '\\o "9-9"', '\\o "x"', '\\t "x;y"', '\\t "x;0;y;-3;z;99"', '\\c "a b" \\a "c"', '\\n "x"', '\\l "9-1"'];
const HEAD = ['TOC', ' TOC ', 'toc', 'TOC \\o "1-3" \\h \\z \\u', '', ' ', 'x', 'TOC "', '\\\\"""'];
const toc_instrs = [];
for (const h of HEAD) toc_instrs.push(h);
for (let i = 0; i < 360; i++) {
  let t = HEAD[rng.below(HEAD.length)];
  for (let k = rng.below(5); k > 0; k--) t += ` ${SW[rng.below(SW.length)]}`;
  if (rng.below(6) === 0) t += ' \\fx \\n9 \\f"z" \\l 2';
  toc_instrs.push(t);
}
for (const t of toc_instrs) {
  const o = refs.parseTocInstr(t);
  rec('pi', hex(t), `${canonOpts(o)}|${hex(refs.tocInstr(o))}`);
}

// ── dialog options ↔ field options ───────────────────────────────────────────────────────────
const B = [undefined, true, false];
const LV = [undefined, 1, 2, 3, 9, 0, -4, 1.5, NaN, Infinity, 1e300];
const STY = [undefined, [], [{ style: 'Quote', level: 2 }], [{ style: 'a b', level: 1 }, { style: 'c', level: 0 }, { style: 'd', level: 10 }, { style: 'e', level: 9 }]];
const FID = [undefined, '', 'x', 'a b', 'q"r'];
for (let i = 0; i < 400; i++) {
  const d = {};
  const set = (k, v) => { if (v !== undefined) d[k] = v; };
  set('pageNumbers', B[rng.below(3)]); set('rightAlign', B[rng.below(3)]); set('hyperlinks', B[rng.below(3)]);
  set('levels', LV[rng.below(LV.length)]); set('useStyles', B[rng.below(3)]); set('styles', STY[rng.below(STY.length)]);
  set('outlineLevels', B[rng.below(3)]); set('fields', B[rng.below(3)]); set('fieldId', FID[rng.below(FID.length)]);
  const o = toc.tocOptionsFrom(d);
  const back = toc.dialogOptionsFrom(o);
  rec('to', canonDialog(d), `${canonOpts(o)}|${hex(refs.tocInstr(o))}|${canonDialog(back)}`);
}

// ── cross-references ─────────────────────────────────────────────────────────────────────────
const TYPES = ['numbered', 'heading', 'bookmark', 'footnote', 'endnote', 'caption'];
const KINDS = ['text', 'page', 'number', 'numberNoContext', 'numberFullContext', 'position', 'noteNumber', 'noteNumberFormatted', 'captionEntire', 'captionLabel', 'captionText'];
TYPES.forEach((t, ti) => rec('ck', String(ti), refs.crossRefKinds(t).join(',')));
const NAMES = ['_Ref123456789', 'Alfa', 'a b', '', '_Toc1'];
const SEPS = [undefined, '', '-', '.', ' ', '"'];
const HYP = [undefined, true, false];
for (const n of NAMES) for (let k = 0; k < KINDS.length; k++) for (let h = 0; h < 3; h++) for (let s = 0; s < SEPS.length; s++) {
  if ((k + h + s) % 3 !== 0) continue;
  rec('ri', `${hex(n)}:${k}:${h}:${SEPS[s] === undefined ? '~' : hex(SEPS[s])}`, hex(refs.crossRefInstr(n, KINDS[k], { hyperlink: HYP[h], separator: SEPS[s] })));
}
const INSTR_WORDS = ['REF', 'PAGEREF', 'NOTEREF', 'SEQ', 'TC', 'ref', 'pageref', 'seq', 'REF x', 'REF  x', 'REF x \\h', 'REF \\h x', 'PAGEREF _Ref1 \\h', 'NOTEREF a \\f', 'SEQ Figure', 'SEQ "My fig" \\* ARABIC', 'SEQ "" x', 'SEQ "a', 'SEQ  F', 'TC "a b" \\f C \\l "2"', 'TC x', 'TC "a\\"b" \\f "x y" \\n', 'TC "a\\\nb"', 'TC "x', ' TC  y \\l 99', ' \tREF\tz', 'REFx', 'XREF y'];
for (const t of INSTR_WORDS) {
  rec('rt', hex(t), hex(refs.refFieldTarget(t)));
  rec('sl', hex(t), hex(refs.seqLabel(t)));
  const tc = toc.parseTcInstr(t);
  rec('pc', hex(t), tc ? `${hex(tc.text)}|${hex(tc.id)}|${tc.level}|${tc.noPage ? 1 : 0}` : '~');
}
rec('rt', hex('REF x'), hex(refs.refFieldTarget('REF x')));
for (const t of ['a', 'a "b"', 'a\\b', '', 'q "uo" \\', 'é']) for (const id of [undefined, '', ' ', 'x', 'a b', ' y ']) for (const lv of [undefined, 0, 1, 2, 9, 12, 2.5, NaN]) {
  rec('tc', `${hex(t)}:${id === undefined ? '~' : hex(id)}:${lv === undefined ? '~' : String(lv)}`, hex(toc.tcInstr({ text: t, id, level: lv })));
}
const TOCS = [' TOC \\f ', ' TOC \\f x ', ' TOC \\f C \\l "1-2" ', ' TOC \\o "1-3" ', ' TOC \\f "a b" ', ' TOC \\l "2-3" \\f '];
const TCS = [' TC "a" ', ' TC "a" \\f x \\l "2" ', ' TC "a" \\f X \\l "3" ', ' TC "a" \\f C \\l "1" ', ' TC "a" \\f "a b" '];
for (const a of TOCS) for (const b of TCS) {
  const tc = toc.parseTcInstr(b);
  rec('ti', `${hex(a)}:${hex(b)}`, String(toc.tcInToc(refs.parseTocInstr(a), tc)));
}

// ── captions ─────────────────────────────────────────────────────────────────────────────────
const FORMATS = ['ARABIC', 'ALPHABETIC', 'alphabetic', 'ROMAN', 'roman'];
const SEPN = ['hyphen', 'period', 'colon', 'emDash', 'enDash'];
for (const label of ['Figure', 'My fig', 'Tabela', '', 'a\tb']) for (let f = 0; f < 5; f++) for (const lv of [undefined, 1, 2, 9, 0, NaN, 1.5, 1e21]) for (let s = 0; s < 5; s += 2) {
  const n = { format: FORMATS[f], ...(lv === undefined ? {} : { chapter: { level: lv, separator: SEPN[s] } }) };
  const r = refs.captionFields(label, n);
  rec('cf', `${hex(label)}:${f}:${lv === undefined ? '~' : String(lv)}:${s}`, `${r.chapter === undefined ? '~' : hex(r.chapter)}|${hex(r.seq)}`);
}
for (let s = 0; s < 5; s++) rec('cs', String(s), hex(refs.CHAPTER_SEPARATOR_TEXT[SEPN[s]]));
for (const k of ['captionEntire', 'captionLabel', 'captionText', 'text']) for (const len of [0, 1, 10]) for (const se of [0, 3, 10, 11, 99]) {
  const r = refs.captionRange(k, len, se);
  rec('cr', `${KINDS.indexOf(k)}:${len}:${se}`, `${r[0]},${r[1]}`);
}

// ── formats and Add Text ─────────────────────────────────────────────────────────────────────
const FMTS = ['template', 'classic', 'distinctive', 'fancy', 'modern', 'formal', 'simple'];
for (let f = 0; f < FMTS.length; f++) for (const lv of [0, 1, 2, 3, 4, 5, 9, 10, 1.5, -1, NaN, 2.5, 1e9]) {
  const r = toc.tocFormatStyle(FMTS[f], lv);
  rec('tf', `${f}:${String(lv)}`, `${hex(r.pPr)}|${hex(r.rPr)}`);
}
for (const lv of [-1, 0, 1, 5, 9, NaN, 0.5]) for (const k of ['normal', 'heading', 'other']) {
  const r = toc.addTextPlan(lv, k);
  rec('ap', `${String(lv)}:${k}`, `${r.heading ?? '~'}|${r.normal ? 1 : 0}|${r.outline === undefined ? 'u' : r.outline === null ? 'n' : r.outline}`);
}
for (const lang of ['en', 'pt', 'es']) for (const lv of [1, 2, 3, 1.5]) rec('mp', `${lang}:${lv}`, hex(toc.manualPlaceholder(lang, lv)));
rec('ms', '0', `${toc.tocSdtPr(-123)}|${toc.tocSdtPr(5)}`.split('').map((c) => c.charCodeAt(0).toString(16)).join('.'));
for (const id of ['auto1', 'auto2', 'manual']) { const t = toc.TOC_TEMPLATES.find((x) => x.id === id); rec('tt', id, `${hex(t.name.en)}|${hex(t.name.pt)}|${hex(t.name.es)}|${hex(t.heading.en)}|${hex(t.heading.pt)}|${hex(t.heading.es)}|${t.manual ? 1 : 0}`); }

// ── hyphenation helpers ──────────────────────────────────────────────────────────────────────
const TEXTS = ['international', 'inter­national', 'a b', 'ação-teste', '­­x', 'café́s', '😀ab😀', 'ab\uD83D', 'x', '', ' wonderful ', 'e­e', 'ab1cd', 'İstanbul', 'ǅǅ'];
for (const t of TEXTS) for (const at of [0, 1, 3, 5, 8, 99]) {
  const r = tl.wordAround(t, at);
  rec('wa', `${hex(t)}:${at}`, `${r[0]},${r[1]}`);
}
const PTS = [[], [2], [3, 6, 6], [0, 1, 99], [5, 2, 4]];
for (const t of TEXTS) for (const p of PTS) {
  const r = tl.manualPoints(t, p);
  rec('mp2', `${hex(t)}:${p.join(',') || '-'}`, `${hex(r.word)}|${r.points.join(',') || '-'}`);
}
for (const t of TEXTS) for (const start of [0, 2]) for (const l of [0, 1, 3, 6, 50, -1, 1.5, NaN]) rec('oa', `${hex(t)}:${start}:${String(l)}`, String(tl.offsetAfterLetters(t, start, l)));

// ── hidden bookmark names ────────────────────────────────────────────────────────────────────
for (const prefix of ['_Ref', '_Toc']) for (const seed of [0, 5, 123456789, 243000000, 999999999, 1000000000, 123456789012]) for (const takenN of [0, 1, 3]) {
  const taken = new Set(Array.from({ length: takenN }, (_, i) => `${prefix}${String(seed + i).padStart(9, '0').slice(-9)}`));
  const s = { n: seed };
  const name = refs.hiddenBookmarkName(prefix, (x) => taken.has(x), s);
  rec('hn', `${prefix}:${seed}:${takenN}`, `${name}|${s.n}`);
}

// ── tocEntryParagraphs ───────────────────────────────────────────────────────────────────────
const TXT = ['Intro', 'A\tb', 'Ação\nx', '', 'x\u000by\fz\r', '😀'];
for (let i = 0; i < 160; i++) {
  const instr = toc_instrs[rng.below(toc_instrs.length)];
  const o = refs.parseTocInstr(instr);
  const n = rng.below(4);
  const sources = [];
  const enc = [];
  for (let k = 0; k < n; k++) {
    const level = [1, 2, 3, 9, 1.5][rng.below(5)];
    const text = TXT[rng.below(TXT.length)];
    const number = rng.below(3) === 0 ? ['1.', '2.1', ''][rng.below(3)] : undefined;
    const page = String(1 + rng.below(30));
    const bookmark = `_Toc${100000000 + rng.below(1000)}`;
    const noPage = rng.below(5) === 0;
    sources.push({ level, text, ...(number ? { number } : {}), page, bookmark, ...(noPage ? { noPage: true } : {}) });
    enc.push([level, hex(text), number === undefined ? '-' : hex(number), page, hex(bookmark), noPage ? 1 : 0].join('~'));
  }
  const hl = o.hyperlinks ? 'Hyperlink' : '';
  const tabPos = [8494, 9000, 1.5][rng.below(3)];
  const leader = [undefined, 'dot', 'none', 'hyphen'][rng.below(4)];
  const paras = refs.tocEntryParagraphs(o, sources, { styleFor: (l) => `S${l}`, hyperlinkStyle: hl, tabPos, ...(leader ? { leader } : {}) });
  rec('tp', `${hex(instr)}:${hex(hl)}:${tabPos}:${leader ?? '~'}:${enc.join('|') || '-'}`, digest(paras));
}

// ── dateTimeFormats ──────────────────────────────────────────────────────────────────────────
const LOC = { en: 'en-US', pt: 'pt-BR', es: 'es-ES' };
for (const lang of ['en', 'pt', 'es']) {
  const inst = [[2026, 1, 2, 3, 4], [2026, 12, 31, 23, 59], [2024, 2, 29, 12, 0], [1999, 7, 4, 0, 0], [2030, 11, 15, 13, 5], [2026, 5, 9, 9, 9], [2000, 1, 1, 12, 30], [2026, 3, 8, 15, 45]];
  for (const [y, m, d, h, mi] of inst) {
    const now = new Date(y, m - 1, d, h, mi, 0);
    const rows = hf.DATE_TIME_FORMATS[lang].map((pattern) => `${hex(pattern)}=${hex(formatWordDate(now, pattern, LOC[lang]))}`);
    rec('dt', `${lang}:${y}:${m}:${d}:${h}:${mi}`, rows.join('|'));
  }
}

// ── runsBetween over corpus paragraphs ───────────────────────────────────────────────────────
let docs = 0;
for (const { rel, file } of corpus().filter((_, i) => i % 7 === 3)) {
  if (docs >= DOCS) break;
  let full;
  try { full = readFullDocument(DocxPackage.load(readBytes(file)), { sources: true }); } catch { continue; }
  const ps = full.blocks.filter((b) => b.type === 'paragraph').map((b) => b.paragraph);
  if (!ps.length) continue;
  const r = new Rng(docs * 7919 + 1);
  for (let k = 0; k < 12; k++) {
    const pi = r.below(ps.length);
    const len = ps[pi].runs.reduce((n, x) => n + x.text.length, 0);
    const a = r.below(len + 2), b = r.below(3) === 0 ? a + r.below(4) : len + r.below(2);
    const key = `${rel}#rb${k}`;
    out.set(key, 'op', `${pi}:${a}:${b}`);
    out.set(key, 'ret', digest([{ runs: refs.runsBetween(ps[pi], a, b), props: {} }]));
  }
  docs++;
}
out.meta('docs', docs);
out.write(join(crate, 'tests', 'parity', 'd2b-f10-ops.capture'));
console.log(`d2b-f10-ops: ${j} pure records, ${docs} documents`);
