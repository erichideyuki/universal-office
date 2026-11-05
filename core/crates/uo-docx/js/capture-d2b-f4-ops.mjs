// Parity capture of the pure model files of D2b-F4 (record uo-docx.md "D2b frontier briefs",
// docs/modules/uo-docx-d2b/F4.md): the frozen TS functions the section, page and text-layout
// commands are built on, run in Node 24.15 (no DOM):
//
//   section-ops.ts   columnsFor, presetColumns, columnsPresetOf, columnWidths, maxColumns,
//                    contentWidth, setOrientation, sectionRanges' arithmetic is the session's (the
//                    session capture covers it)
//   text-layout.ts   lineNumberingInfo, withLineNumberMode, lineNumberingFromOptions,
//                    hyphenation JSON order (the comparison `setHyphenationOptions` makes)
//   page-design.ts   gradientGeometry, pageFillFrom (the gradients and their presets)
//
//   UO_TS_ROOT=<built checkout of main> node core/crates/uo-docx/js/capture-d2b-f4-ops.mjs
//
// Writes tests/parity/d2b-f4-ops.capture: one record per case with the facets `in` (the encoded
// input; the Rust replay decodes it) and `out` (the JSON result). Inputs are seeded (xorshift32,
// uo-testkit `Rng`); numbers travel as `String(x)` so that NaN and the infinities do.
import { CaptureWriter, Rng, importFrom } from '../../uo-testkit/js/testkit.mjs';
import { root } from './capture-r2-common.mjs';

const ed = (f) => importFrom(root, `write/packages/editor/src/${f}`);
const { ts } = await import('./capture-r2-common.mjs');
const so = await ts('section-ops.js');
const tl = await ed('text-layout.ts');
const pd = await ed('page-design.ts');

const out = new CaptureWriter([
  'D2b-F4 pure model files: section-ops (columns, orientation), text-layout (line numbering), page-design (gradients)',
  'facets: in, out',
]);
const rec = (key, input, output) => { out.set(key, 'in', input); out.set(key, 'out', output); };
const rng = new Rng(0xf4f4);
const J = (v) => String(JSON.stringify(v));

const WIDTHS = [11906, 12240, 16838, 9000, 3000, 1440, 720, 0, -100, 1e9, NaN, Infinity];
const MARGINS = [1701, 1417, 720, 0, 2880, 600, NaN];
const SPACES = [708, 0, 300, 5000, -50, NaN, 1e300];
const num = (a) => a[rng.below(a.length)];
const sect = () => {
  const sp = { pageWidthTwips: num(WIDTHS), pageHeightTwips: num(WIDTHS), marginLeftTwips: num(MARGINS), marginRightTwips: num(MARGINS), marginTopTwips: num(MARGINS), marginBottomTwips: num(MARGINS) };
  if (rng.below(3) > 0) {
    sp.cols = { count: [1, 2, 3, 4, 2.5, NaN][rng.below(6)], spaceTwips: num(SPACES), sep: rng.below(2) === 0 };
    if (rng.below(3) === 0) sp.cols.widths = [{ widthTwips: num(WIDTHS), spaceTwips: num(SPACES) }, { widthTwips: num(WIDTHS), spaceTwips: num(SPACES) }];
  }
  return sp;
};
const enc = (sp) => [sp.pageWidthTwips, sp.pageHeightTwips, sp.marginLeftTwips, sp.marginRightTwips, sp.marginTopTwips, sp.marginBottomTwips].map(String).join(',')
  + (sp.cols ? `;${sp.cols.count},${sp.cols.spaceTwips},${sp.cols.sep ? 1 : 0}${sp.cols.widths ? `;${sp.cols.widths.map((w) => `${w.widthTwips}/${w.spaceTwips}`).join(',')}` : ''}` : '');

// ── columns and orientation ────────────────────────────────────────────────────────────────
for (let i = 0; i < 1200; i++) {
  const sp = sect();
  const spec = {
    count: [1, 2, 3, 4, 5, 1.5, 0, -1, NaN, 100][rng.below(10)],
    ...(rng.below(2) ? { spaceTwips: num(SPACES) } : {}),
    sep: rng.below(2) === 0,
    ...(rng.below(3) === 0 ? { widths: [{ widthTwips: num(WIDTHS), spaceTwips: num(SPACES) }, { widthTwips: num(WIDTHS), spaceTwips: num(SPACES) }, { widthTwips: 100, spaceTwips: 7 }] } : {}),
  };
  const specEnc = `${spec.count},${spec.spaceTwips ?? '-'},${spec.sep ? 1 : 0}${spec.widths ? `,${spec.widths.map((w) => `${w.widthTwips}/${w.spaceTwips}`).join('/')}` : ''}`;
  rec(`cols#${i}`, `${enc(sp)}|${specEnc}`, J(so.columnsFor(sp, spec)) + `|${so.maxColumns(sp)}|${so.contentWidth(sp)}`);
}
for (let i = 0; i < 600; i++) {
  const sp = sect();
  for (const p of ['one', 'two', 'three', 'left', 'right']) {
    const spec = so.presetColumns(sp, p);
    rec(`preset#${i}:${p}`, enc(sp), J(spec) + `|${J(so.columnsFor(sp, spec))}`);
  }
  rec(`presetof#${i}`, enc(sp), `${so.columnsPresetOf(sp)}|${J(so.columnWidths(sp))}`);
}
for (let i = 0; i < 300; i++) {
  const sp = sect();
  for (const o of ['landscape', 'portrait']) {
    const c = { ...sp };
    so.setOrientation(c, o);
    rec(`orient#${i}:${o}`, enc(sp), [c.pageWidthTwips, c.pageHeightTwips, c.marginTopTwips, c.marginRightTwips, c.marginBottomTwips, c.marginLeftTwips].map(String).join(','));
  }
}

// ── line numbering ─────────────────────────────────────────────────────────────────────────
const LN = () => {
  if (rng.below(4) === 0) return undefined;
  const o = {};
  if (rng.below(5) > 0) o.countBy = [0, 1, 2, 5, 100, NaN, -1][rng.below(7)];
  if (rng.below(2)) o.start = [0, 1, 4, 32766][rng.below(4)];
  if (rng.below(2)) o.distance = [0, 300, 720][rng.below(3)];
  if (rng.below(2)) o.restart = ['continuous', 'newSection', 'newPage'][rng.below(3)];
  return o;
};
const lnEnc = (l) => (l ? [l.countBy, l.start, l.distance, l.restart].map((x) => (x === undefined ? '-' : String(x))).join(',') : '-');
for (let i = 0; i < 500; i++) {
  const sp = { lnNumType: LN() };
  rec(`lninfo#${i}`, lnEnc(sp.lnNumType), J(tl.lineNumberingInfo(sp)));
  const memory = rng.below(2) ? { ...(rng.below(2) ? { start: [0, 3, NaN][rng.below(3)] } : {}), ...(rng.below(2) ? { distance: [0, 300][rng.below(2)] } : {}) } : undefined;
  for (const m of ['none', 'continuous', 'page', 'section']) {
    rec(`lnmode#${i}:${m}`, `${lnEnc(sp.lnNumType)}|${memory ? `${memory.start ?? '-'},${memory.distance ?? '-'}` : 'x'}`, J(tl.withLineNumberMode(sp, m, memory)));
  }
  const o = { on: rng.below(4) > 0, startAt: [0, 1, 5, 40000, NaN, 2.5][rng.below(6)], countBy: [0, 1, 3, 500, NaN][rng.below(5)], distanceTwips: [0, 1, 300, 99999, -5, NaN][rng.below(6)], restart: ['page', 'section', 'continuous'][rng.below(3)] };
  rec(`lnopt#${i}`, `${o.on ? 1 : 0},${o.startAt},${o.countBy},${o.distanceTwips},${o.restart}`, J(tl.lineNumberingFromOptions(o)));
}

// ── gradients ──────────────────────────────────────────────────────────────────────────────
const STYLES = ['horizontal', 'vertical', 'diagonalUp', 'diagonalDown', 'fromCorner', 'fromCenter'];
for (const [si, st] of STYLES.entries()) {
  for (const v of [1, 2, 3, 4, 5, 0, -3, 2.5, NaN, Infinity]) rec(`geo#${st}:${v}`, `${si},${v}`, J(pd.gradientGeometry(st, v)));
}
for (let i = 0; i < 200; i++) {
  const st = STYLES[rng.below(6)];
  const v = 1 + rng.below(4);
  const deg = [0, 0.25, 0.5, 0.75, 1, 2, -1, NaN][rng.below(8)];
  const inputs = [
    { type: 'gradient', colors: 'one', color: '4472C4', degree: deg, style: st, variant: v },
    { type: 'gradient', colors: 'two', color: '4472C4', color2: 'FFC000', style: st, variant: v },
    { type: 'gradient', colors: 'preset', preset: ['mist', 'sand', 'aurora', 'nope'][rng.below(4)], style: st, variant: v },
  ];
  inputs.forEach((inp, k) => rec(`fill#${i}:${k}`, `${k},${inp.color ?? inp.preset},${inp.degree ?? inp.color2 ?? '-'},${STYLES.indexOf(st)},${v}`, J(pd.pageFillFrom(inp, 'en', () => null))));
}

import('node:path').then(async ({ join }) => {
  const { crate } = await import('./capture-r2-common.mjs');
  out.write(join(crate, 'tests', 'parity', 'd2b-f4-ops.capture'));
  console.log('d2b-f4-ops: written');
});
