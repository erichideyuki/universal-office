// Parity capture of table-xml.ts and the legacy table generators of serialize.ts (D1b frontier W3):
// the frozen TS `patchTblPr`/`patchTblPrEx`/`patchTrPr`/`patchTcPr` and `tblPrXml`/`tblPrExXml`/
// `trPrXml`/`tcPrXml` over the distinct `w:tblPr`/`w:tblPrEx`/`w:trPr`/`w:tcPr` of the corpus (up to
// a cap per kind), each with every named change of the typed props (the same list is implemented by
// tests/parity_table_xml.rs); the same functions over seeded synthetic props (`synth*` records:
// `patch(gen(A), B)`); and `tableXml` (the legacy generators `tblPrXml`/`tcPrXml`/`borderSetXml`
// and the typed ones in place) over seeded synthetic tables.
//
//   UO_TS_ROOT=<built checkout of main> node core/crates/uo-docx/js/capture-table-xml.mjs
//
// Writes tests/parity/table-xml.inputs (`<kind>\t<escaped raw>` per element, in order) and
// tests/parity/table-xml.capture, one record per `<kind>#<element index>#<change>` (`patch`, `gen`:
// short SHA-256 of the result, or `err` when the TS throws; `gen` is `na` when there are no props),
// `synth<Kind>#<n>` (`gen`, `patch`) and `table#<n>` (`xml`).
import { writeFileSync } from 'node:fs';
import { join } from 'node:path';
import { createHash } from 'node:crypto';
import { CaptureWriter } from '../../uo-testkit/js/testkit.mjs';
import { corpus, crate, readBytes, ts } from './capture-r2-common.mjs';

const { DocxPackage } = await ts('package.js');
const { parseXml } = await ts('xml.js');
const { readTblPr, readTrPr, readTcPr } = await ts('ooxml/props.js');
const X = await ts('table-xml.js');
const { tableXml } = await ts('serialize.js');

const CAP = 500;
const SYNTH = 1500;
const TABLES = 800;
const sha = (s) => createHash('sha256').update(s).digest('hex').slice(0, 32);
const esc = (x) => x.replace(/\\/g, '\\\\').replace(/\n/g, '\\n').replace(/\r/g, '\\r').replace(/\t/g, '\\t');

// ───────────────────────── the named changes ─────────────────────────

const side = (b) => ({ val: 'single', sz: 4, space: 0, color: b });
export const TBL_MUTS = [
  ['none', () => {}],
  ['style', (p) => { p.tblStyle = 'TableGrid'; }],
  ['width', (p) => { p.tblW = { w: 5000, type: 'pct' }; }],
  ['jc', (p) => { p.jc = p.jc === 'center' ? 'right' : 'center'; }],
  ['borders', (p) => { p.tblBorders = { top: side('FF0000'), bottom: { val: 'nil' }, insideH: { val: 'dashed', sz: 8, themeColor: 'accent1', shadow: true } }; }],
  ['noborders', (p) => { delete p.tblBorders; }],
  ['emptyborders', (p) => { p.tblBorders = {}; }],
  ['layout', (p) => { p.tblLayout = 'fixed'; }],
  ['look', (p) => { p.tblLook = { firstRow: true, noHBand: true }; }],
  ['shd', (p) => { p.shd = { val: 'clear', fill: 'DDDDDD' }; }],
  ['mar', (p) => { p.tblCellMar = { top: { w: 50, type: 'dxa' }, left: { w: 108, type: 'dxa' } }; }],
  ['ind', (p) => { p.tblInd = { w: 720, type: 'dxa' }; p.tblCellSpacing = { w: 10, type: 'dxa' }; }],
  ['pos', (p) => { p.tblpPr = { leftFromText: 180, vertAnchor: 'text', tblpX: 100, tblpYSpec: 'top' }; }],
  ['misc', (p) => { p.tblCaption = 'a&b"<c>'; p.tblDescription = 'd'; p.bidiVisual = !p.bidiVisual; p.tblOverlap = 'never'; p.tblStyleRowBandSize = 2; p.tblStyleColBandSize = 3; }],
  ['unset', (p) => { for (const k of Object.keys(p)) delete p[k]; }],
  ['undefined', () => undefined],
];
export const TR_MUTS = [
  ['none', () => {}],
  ['height', (p) => { p.trHeight = { val: 300, hRule: 'exact' }; }],
  ['heightLeast', (p) => { p.trHeight = { val: 400, hRule: 'atLeast' }; }],
  ['header', (p) => { p.tblHeader = !p.tblHeader; }],
  ['split', (p) => { p.cantSplit = !p.cantSplit; }],
  ['hidden', (p) => { p.hidden = !p.hidden; }],
  ['grid', (p) => { p.gridBefore = 1; p.gridAfter = 2; p.wBefore = { w: 100, type: 'dxa' }; p.wAfter = { w: 200, type: 'pct' }; }],
  ['jc', (p) => { p.jc = 'center'; p.tblCellSpacing = { w: 20, type: 'dxa' }; }],
  ['cnf', (p) => { p.cnfStyle = '100000000000'; }],
  ['unset', (p) => { for (const k of Object.keys(p)) delete p[k]; }],
  ['undefined', () => undefined],
];
export const TC_MUTS = [
  ['none', () => {}],
  ['width', (p) => { p.tcW = { w: 3000, type: 'dxa' }; }],
  ['span', (p) => { p.gridSpan = 2; }],
  ['hmerge', (p) => { p.hMerge = 'restart'; p.vMerge = 'continue'; }],
  ['vmerge', (p) => { p.vMerge = 'restart'; p.hMerge = 'continue'; }],
  ['borders', (p) => { p.tcBorders = { top: side('00FF00'), left: { val: 'nil' }, tl2br: { val: 'single', sz: 6, frame: false } }; }],
  ['noborders', (p) => { delete p.tcBorders; p.tcBorders = {}; }],
  ['shd', (p) => { p.shd = { val: 'clear', color: 'auto', fill: 'FFAA00', themeFill: 'accent2' }; }],
  ['wrap', (p) => { p.noWrap = !p.noWrap; p.tcFitText = true; p.hideMark = false; }],
  ['mar', (p) => { p.tcMar = { top: { w: 20, type: 'dxa' }, right: { w: 30, type: 'dxa' } }; }],
  ['dir', (p) => { p.textDirection = 'btLr'; p.vAlign = 'center'; }],
  ['cnf', (p) => { p.cnfStyle = '001000000000'; }],
  ['unset', (p) => { for (const k of Object.keys(p)) delete p[k]; }],
  ['undefined', () => undefined],
];

// ───────────────────────── the seeded synthetic values ─────────────────────────

/** `Math.imul` generator: the same sequence in tests/parity_table_xml.rs. */
export function lcg(seed) {
  let s = Math.imul(seed, 2654435761) >>> 0;
  return (n) => { s = (Math.imul(s, 1664525) + 1013904223) >>> 0; return (s >>> 16) % n; };
}
const S = ['TableGrid', 'center', 'a&b', '"q"<x>', '', 'fixed'];
const N = [0, 1, 20, 108, -5, 5000, 1.5, 1e21, null];
const str = (r) => S[r(S.length)];
const num = (r) => N[r(N.length)];
const bool = (r) => r(2) === 1;
const opt = (r, f) => (r(3) === 0 ? undefined : f());
const put = (o, k, v) => { if (v !== undefined) o[k] = v; };
const width = (r) => ({ w: num(r), type: ['dxa', 'pct', 'auto', 'nil'][r(4)] });
function border(r) {
  const b = { val: ['single', 'nil', 'dashed', 'a"b'][r(4)] };
  put(b, 'sz', opt(r, () => num(r)));
  put(b, 'space', opt(r, () => num(r)));
  put(b, 'color', opt(r, () => ['FF0000', 'auto', 'a&b'][r(3)]));
  put(b, 'themeColor', opt(r, () => str(r)));
  put(b, 'themeTint', opt(r, () => str(r)));
  put(b, 'themeShade', opt(r, () => str(r)));
  put(b, 'shadow', opt(r, () => bool(r)));
  put(b, 'frame', opt(r, () => bool(r)));
  return b;
}
function borders8(r) {
  const o = {};
  for (const k of ['top', 'left', 'bottom', 'right', 'insideH', 'insideV', 'tl2br', 'tr2bl']) put(o, k, opt(r, () => border(r)));
  return o;
}
function shd(r) {
  const o = {};
  for (const k of ['val', 'color', 'fill', 'themeFill', 'themeFillTint', 'themeFillShade', 'themeColor']) put(o, k, opt(r, () => str(r)));
  return o;
}
function mar(r) {
  const o = {};
  for (const k of ['top', 'left', 'bottom', 'right']) put(o, k, opt(r, () => width(r)));
  return o;
}
function look(r) {
  const o = {};
  for (const k of ['firstRow', 'lastRow', 'firstColumn', 'lastColumn', 'noHBand', 'noVBand']) put(o, k, opt(r, () => bool(r)));
  return o;
}
function tblp(r) {
  const o = {};
  for (const k of ['leftFromText', 'rightFromText', 'topFromText', 'bottomFromText']) put(o, k, opt(r, () => num(r)));
  for (const k of ['vertAnchor', 'horzAnchor', 'tblpXSpec']) put(o, k, opt(r, () => str(r)));
  put(o, 'tblpX', opt(r, () => num(r)));
  put(o, 'tblpYSpec', opt(r, () => str(r)));
  put(o, 'tblpY', opt(r, () => num(r)));
  return o;
}
export function synthTbl(r) {
  const p = {};
  put(p, 'tblStyle', opt(r, () => str(r)));
  put(p, 'tblpPr', opt(r, () => tblp(r)));
  put(p, 'tblOverlap', opt(r, () => str(r)));
  put(p, 'bidiVisual', opt(r, () => bool(r)));
  put(p, 'tblStyleRowBandSize', opt(r, () => num(r)));
  put(p, 'tblStyleColBandSize', opt(r, () => num(r)));
  put(p, 'tblW', opt(r, () => width(r)));
  put(p, 'jc', opt(r, () => str(r)));
  put(p, 'tblCellSpacing', opt(r, () => width(r)));
  put(p, 'tblInd', opt(r, () => width(r)));
  put(p, 'tblBorders', opt(r, () => borders8(r)));
  put(p, 'shd', opt(r, () => shd(r)));
  put(p, 'tblLayout', opt(r, () => str(r)));
  put(p, 'tblCellMar', opt(r, () => mar(r)));
  put(p, 'tblLook', opt(r, () => look(r)));
  put(p, 'tblCaption', opt(r, () => str(r)));
  put(p, 'tblDescription', opt(r, () => str(r)));
  return p;
}
export function synthTr(r) {
  const p = {};
  put(p, 'cnfStyle', opt(r, () => str(r)));
  put(p, 'gridBefore', opt(r, () => num(r)));
  put(p, 'gridAfter', opt(r, () => num(r)));
  put(p, 'wBefore', opt(r, () => width(r)));
  put(p, 'wAfter', opt(r, () => width(r)));
  put(p, 'cantSplit', opt(r, () => bool(r)));
  put(p, 'trHeight', opt(r, () => ({ val: num(r), hRule: ['atLeast', 'exact', 'auto', 'x"y'][r(4)] })));
  put(p, 'tblHeader', opt(r, () => bool(r)));
  put(p, 'tblCellSpacing', opt(r, () => width(r)));
  put(p, 'jc', opt(r, () => str(r)));
  put(p, 'hidden', opt(r, () => bool(r)));
  return p;
}
export function synthTc(r) {
  const p = {};
  put(p, 'cnfStyle', opt(r, () => str(r)));
  put(p, 'tcW', opt(r, () => width(r)));
  put(p, 'gridSpan', opt(r, () => num(r)));
  put(p, 'hMerge', opt(r, () => ['restart', 'continue', 'x'][r(3)]));
  put(p, 'vMerge', opt(r, () => ['restart', 'continue', 'x'][r(3)]));
  put(p, 'tcBorders', opt(r, () => borders8(r)));
  put(p, 'shd', opt(r, () => shd(r)));
  put(p, 'noWrap', opt(r, () => bool(r)));
  put(p, 'tcMar', opt(r, () => mar(r)));
  put(p, 'textDirection', opt(r, () => str(r)));
  put(p, 'tcFitText', opt(r, () => bool(r)));
  put(p, 'vAlign', opt(r, () => str(r)));
  put(p, 'hideMark', opt(r, () => bool(r)));
  return p;
}

function edge(r) {
  const e = { style: ['single', 'a"b', 'double'][r(3)], sizeEighthPt: num(r), spacePt: num(r) };
  put(e, 'color', opt(r, () => ['FF0000', '', 'a&b'][r(3)]));
  return e;
}
function tableBorders(r) {
  const o = {};
  for (const k of ['top', 'left', 'bottom', 'right', 'insideH', 'insideV']) put(o, k, opt(r, () => edge(r)));
  return o;
}
export function synthTable(r) {
  const t = { type: 'table', rows: [], gridColsTwips: [] };
  for (let i = r(4); i > 0; i--) t.gridColsTwips.push(num(r));
  put(t, 'borders', opt(r, () => tableBorders(r)));
  put(t, 'tblPr', opt(r, () => synthTbl(r)));
  for (let i = 1 + r(3); i > 0; i--) {
    const row = { cells: [] };
    put(row, 'trPr', opt(r, () => synthTr(r)));
    put(row, 'tblPrEx', opt(r, () => synthTbl(r)));
    for (let j = 1 + r(3); j > 0; j--) {
      const c = { paragraphs: [], tables: [], gridSpan: [1, 2, 3, 0][r(4)] };
      put(c, 'widthTwips', opt(r, () => num(r)));
      put(c, 'borders', opt(r, () => tableBorders(r)));
      put(c, 'shadingHex', opt(r, () => ['FFAA00', '', 'a&b'][r(3)]));
      put(c, 'tcPr', opt(r, () => synthTc(r)));
      row.cells.push(c);
    }
    t.rows.push(row);
  }
  return t;
}

// ───────────────────────── the corpus elements ─────────────────────────

const KINDS = {
  tblPr: { tag: 'w:tblPr', read: readTblPr, patch: X.patchTblPr, gen: X.tblPrXml, muts: TBL_MUTS, synth: synthTbl },
  tblPrEx: { tag: 'w:tblPrEx', read: readTblPr, patch: X.patchTblPrEx, gen: X.tblPrExXml, muts: TBL_MUTS, synth: synthTbl },
  trPr: { tag: 'w:trPr', read: readTrPr, patch: X.patchTrPr, gen: X.trPrXml, muts: TR_MUTS, synth: synthTr },
  tcPr: { tag: 'w:tcPr', read: readTcPr, patch: X.patchTcPr, gen: X.tcPrXml, muts: TC_MUTS, synth: synthTc },
};
const raws = Object.fromEntries(Object.keys(KINDS).map((k) => [k, new Set()]));
const byTag = Object.fromEntries(Object.entries(KINDS).map(([k, v]) => [v.tag, k]));
const full = () => Object.values(raws).every((s) => s.size >= CAP);
for (const { file } of corpus()) {
  if (full()) break;
  let pkg;
  try { pkg = DocxPackage.load(readBytes(file)); } catch { continue; }
  for (const part of pkg.partNames().filter((p) => /^word\/(document|header\d*|footer\d*|styles)\.xml$/.test(p))) {
    const xml = pkg.getPartText(part);
    let root;
    try { root = parseXml(xml, { positions: true }); } catch { continue; }
    const walk = [root];
    while (walk.length) {
      const el = walk.pop();
      const kind = byTag[el.name];
      if (kind && raws[kind].size < CAP && el.s !== undefined) raws[kind].add(xml.slice(el.s, el.e));
      for (const c of el.children) if (typeof c !== 'string') walk.push(c);
    }
  }
}

const out = new CaptureWriter(['table-xml.ts patch*/ *Xml over corpus elements x named changes, seeded synthetic props and tableXml over seeded tables', 'facets: short sha256 of the result, `err` when the TS throws, `na` without props']);
const lines = ['# table-xml.ts inputs (js/capture-table-xml.mjs): `<kind>\\t<escaped raw element>`'];
const run = (f) => { try { return sha(f()); } catch { return 'err'; } };
let n = 0;
for (const [kind, K] of Object.entries(KINDS)) {
  [...raws[kind]].forEach((raw, i) => {
    lines.push(`${kind}\t${esc(raw)}`);
    for (const [m, f] of K.muts) {
      const key = `${kind}#${i}#${m}`;
      let cur = structuredClone(K.read(parseXml(raw, { positions: true })));
      if (m === 'undefined') cur = undefined;
      else f(cur);
      out.set(key, 'patch', run(() => K.patch(raw, cur)));
      out.set(key, 'gen', cur === undefined ? 'na' : run(() => K.gen(cur)));
      n++;
    }
  });
}
for (const [kind, K] of Object.entries(KINDS)) {
  for (let i = 0; i < SYNTH; i++) {
    const r = lcg(i + 1);
    const a = K.synth(r);
    const b = K.synth(r);
    const key = `synth${kind}#${i}`;
    let raw;
    try { raw = K.gen(a); } catch { raw = ''; }
    if (raw === '') raw = `<${K.tag}/>`;
    out.set(key, 'gen', run(() => K.gen(a)));
    out.set(key, 'patch', run(() => K.patch(raw, b)));
    n++;
  }
}
for (let i = 0; i < TABLES; i++) {
  const t = synthTable(lcg(100000 + i));
  out.set(`table#${i}`, 'xml', run(() => tableXml(t)));
  n++;
}
writeFileSync(join(crate, 'tests', 'parity', 'table-xml.inputs'), lines.join('\n') + '\n');
out.write(join(crate, 'tests', 'parity', 'table-xml.capture'));
console.log(`table-xml: ${Object.entries(raws).map(([k, s]) => `${s.size} ${k}`).join(', ')}, ${n} records`);
