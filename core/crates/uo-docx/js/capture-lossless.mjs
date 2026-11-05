// Parity capture of lossless.ts (the D1b surgery on raw w:rPr / w:pPr / w:sectPr): the frozen TS
// `patchRPr`, `patchPPr` and `patchSectPr` over the distinct elements of the corpus (the first docx of
// the oracle corpus and the docx-core fixtures, up to a cap per kind), each with every named mutation
// of the editing props (the same list is implemented by tests/parity_lossless.rs).
//
//   UO_TS_ROOT=<built checkout of main> node core/crates/uo-docx/js/capture-lossless.mjs
//
// Writes tests/parity/lossless.inputs (`<kind>\t<escaped raw>` per element, in order) and
// tests/parity/lossless.capture, one record per `<kind>#<element index>#<mutation>`: `out` = short
// SHA-256 of the result (or `err` when the TS throws).
import { writeFileSync } from 'node:fs';
import { join } from 'node:path';
import { createHash } from 'node:crypto';
import { CaptureWriter } from '../../uo-testkit/js/testkit.mjs';
import { corpus, crate, readBytes, ts } from './capture-r2-common.mjs';

const { DocxPackage } = await ts('package.js');
const { parseXml } = await ts('xml.js');
const { readRPr, readPPr } = await ts('ooxml/props.js');
const { editBaseProps, paraPropsFromPPr, readSectPr } = await ts('document.js');
const { patchRPr, patchPPr, patchSectPr } = await ts('lossless.js');

const CAP = 1500;
const sha = (s) => createHash('sha256').update(s).digest('hex').slice(0, 32);
const esc = (x) => x.replace(/\\/g, '\\\\').replace(/\n/g, '\\n').replace(/\r/g, '\\r').replace(/\t/g, '\\t');

export const R_MUTS = [
  ['none', () => {}],
  ['bold', (p) => { p.bold = !p.bold; }],
  ['italic', (p) => { p.italic = !p.italic; }],
  ['sz', (p) => { p.szHalfPt = 31; }],
  ['color', (p) => { p.color = 'FF0000'; delete p.colorTheme; }],
  ['font', (p) => { p.fontAscii = 'Arial'; }],
  ['underline', (p) => { p.underline = !p.underline; }],
  ['super', (p) => { p.vertAlign = 'superscript'; }],
  ['hidden', (p) => { p.hidden = !p.hidden; }],
  ['style', (p) => { p.charStyle = 'Strong'; }],
  ['scale', (p) => { p.scalePct = 150; p.kernHalfPt = 0; p.positionHalfPt = 6; }],
  ['spacing', (p) => { p.charSpacingTwips = 20; p.highlight = 'yellow'; }],
  ['unset', (p) => { delete p.szHalfPt; delete p.color; delete p.fontAscii; }],
];
export const P_MUTS = [
  ['none', () => {}],
  ['center', (p) => { p.alignment = 'center'; }],
  ['indent', (p) => { p.indentLeftTwips = 720; p.indentFirstLineTwips = -360; }],
  ['space', (p) => { p.spaceBeforeTwips = 120; p.lineTwentieths = 360; }],
  ['style', (p) => { p.styleId = 'Heading1'; }],
  ['keep', (p) => { p.keepNext = !p.keepNext; }],
  ['nonum', (p) => { delete p.numbering; }],
  ['tabs', (p) => { p.tabStops = []; p.shadingHex = 'DDDDDD'; }],
  ['auto', (p) => { p.spaceAfterAuto = true; }],
  ['outline', (p) => { p.outlineLvl = 2; }],
];
export const S_MUTS = [
  ['none', () => undefined],
  ['swap', (s) => { const w = s.pageWidthTwips; s.pageWidthTwips = s.pageHeightTwips; s.pageHeightTwips = w; }],
  ['margins', (s) => { s.marginTopTwips += 100; s.marginLeftTwips += 100; s.headerDistanceTwips = 500; }],
  ['title', (s) => { s.titlePg = !s.titlePg; }],
  ['cols', (s) => { s.cols = { count: 2, spaceTwips: 708, sep: true }; }],
  ['valign', (s) => { s.vAlign = 'center'; s.type = 'continuous'; }],
  ['pgnum', (s) => { s.pgNumStart = 3; s.pgNumFmt = 'lowerRoman'; }],
  ['refs', () => [{ kind: 'header', type: 'default', rid: 'rIdX' }]],
  ['lnnum', (s) => { s.lnNumType = { countBy: 5 }; }],
];

// the distinct raw elements, by kind
const raws = { rPr: new Set(), pPr: new Set(), sectPr: new Set() };
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
      const kind = el.name === 'w:rPr' ? 'rPr' : el.name === 'w:pPr' ? 'pPr' : el.name === 'w:sectPr' ? 'sectPr' : null;
      if (kind && raws[kind].size < CAP && el.s !== undefined) raws[kind].add(xml.slice(el.s, el.e));
      for (const c of el.children) if (typeof c !== 'string') walk.push(c);
    }
  }
}

const out = new CaptureWriter(['lossless.ts patchRPr/patchPPr/patchSectPr over corpus elements x named mutations', 'facet out: short sha256 of the result, or err']);
const lines = ['# lossless.ts inputs (js/capture-lossless.mjs): `<kind>\\t<escaped raw element>`'];
let n = 0;
for (const kind of ['rPr', 'pPr', 'sectPr']) {
  [...raws[kind]].forEach((raw, i) => {
    lines.push(`${kind}\t${esc(raw)}`);
    const el = parseXml(raw, { positions: true });
    const muts = kind === 'rPr' ? R_MUTS : kind === 'pPr' ? P_MUTS : S_MUTS;
    for (const [m, f] of muts) {
      const key = `${kind}#${i}#${m}`;
      try {
        let res;
        if (kind === 'rPr') {
          const run = { text: 'x', props: { ...editBaseProps({ text: 'x', props: {} }, readRPr(el)) } };
          f(run.props);
          res = patchRPr(raw, run);
        } else if (kind === 'pPr') {
          const para = { runs: [], props: { ...paraPropsFromPPr(readPPr(el)) } };
          f(para.props);
          res = patchPPr(raw, para, undefined).xml;
        } else {
          const cur = { ...readSectPr(el) };
          const refs = f(cur);
          res = patchSectPr(raw, cur, refs);
        }
        out.set(key, 'out', sha(res));
      } catch {
        out.set(key, 'out', 'err');
      }
      n++;
    }
  });
}
writeFileSync(join(crate, 'tests', 'parity', 'lossless.inputs'), lines.join('\n') + '\n');
out.write(join(crate, 'tests', 'parity', 'lossless.capture'));
console.log(`lossless: ${raws.rPr.size} rPr, ${raws.pPr.size} pPr, ${raws.sectPr.size} sectPr, ${n} records`);
