// Parity capture of the pure model files of D2b-F1 (record uo-docx.md "D2b frontier briefs",
// docs/modules/uo-docx-d2b/F1.md): the frozen TS functions the character and paragraph formatting
// commands are built on, run in Node 24.15 (no DOM, no math in these paths):
//
//   text-case.ts          caseMapped            (Change Case, letter by letter, UTF-16 with lone surrogates)
//   text-effects-presets  TEXT_EFFECT_GALLERY, textGradient, textGlow, outline, the shadow presets
//   navigation.ts         tabInfo, tabsDirect, dedupeTabs, indentsDirect   (the ruler rules)
//   text-layout.ts        flowValue, flowDirect, spacingDirect, common, PARAGRAPH_LAYOUT_LIMITS
//   page-design.ts        themeTintShade
//   source.ts (regex)     plainTextSdt through its two regexes (the V8 rows are the regex-sites file)
//
//   UO_TS_ROOT=<built checkout of main> node core/crates/uo-docx/js/capture-d2b-f1-ops.mjs
//
// Writes tests/parity/d2b-f1-ops.capture: one record per case with the facets `in` (the encoded
// input; the Rust replay decodes it) and `out` (the JSON / hex result). Inputs are seeded
// (xorshift32, uo-testkit `Rng`).
import { join } from 'node:path';
import { CaptureWriter, Rng, importFrom } from '../../uo-testkit/js/testkit.mjs';
import { crate, root } from './capture-r2-common.mjs';

const ed = (f) => importFrom(root, `write/packages/editor/src/${f}`);
const { caseMapped } = await ed('text-case.ts');
const fxm = await ed('text-effects-presets.ts');
const nav = await ed('navigation.ts');
const tl = await ed('text-layout.ts');
const { themeTintShade } = await ed('page-design.ts');

const out = new CaptureWriter([
  'D2b-F1 pure model files: caseMapped, the text-effect presets, the ruler rules, the paragraph rules, themeTintShade',
  'facets: in, out',
]);
const rec = (key, input, output) => { out.set(key, 'in', input); out.set(key, 'out', output); };
const hex = (t) => [...Array(t.length).keys()].map((i) => t.charCodeAt(i).toString(16)).join('.');
const unhex = (h) => String.fromCharCode(...h.split('.').filter(Boolean).map((x) => parseInt(x, 16)));
const rng = new Rng(0xf1f1);

// ── caseMapped ──────────────────────────────────────────────────────────────────────────────
const ALPHA = ['a', 'B', 'z', 'Ω', 'σ', 'Σ', 'ς', 'ß', 'ǆ', 'ǅ', 'Ǆ', 'ﬁ', 'İ', 'ı', 'i', 'I', 'ŉ', 'é', 'É', ' ', ' ', '\n', '.', '!', '?', ',', '1', '9', '٣', '😀', '𝒜', '\ud800', '\udc00', 'ǰ', 'ᾳ', ' ', '　', '"', '-'];
const modes = ['upper', 'lower', 'sentence', 'title', 'toggle'];
for (let i = 0; i < 400; i++) {
  const n = rng.below(24);
  let s = '';
  for (let k = 0; k < n; k++) s += ALPHA[rng.below(ALPHA.length)];
  for (const m of modes) rec(`case#${i}:${m}`, `${m}:${hex(s)}`, hex(caseMapped(s, m)));
}

// ── text effects ────────────────────────────────────────────────────────────────────────────
fxm.TEXT_EFFECT_GALLERY.forEach((p, i) => rec(`gallery#${i}`, `${i}`, JSON.stringify(p.fx)));
fxm.TEXT_SHADOW_PRESETS.slice(0, 2).forEach((p, i) => rec(`shadow#${i}`, `${i}`, JSON.stringify(p.shadow)));
const COLORS = [{ rgb: '000000' }, { scheme: 'accent1' }, { rgb: 'FF8800', mods: [['lumMod', 75000]] }, { scheme: 'tx1', mods: [['alpha', 1000], ['lumOff', 5]] }];
COLORS.forEach((c, ci) => {
  for (const [ang, light] of [[90, true], [0, false], [45.5, true], [270.25, false], [-30, true]]) {
    rec(`grad#${ci}:${ang}:${light}`, `${JSON.stringify(c)}|${ang}|${light}`, JSON.stringify(fxm.textGradient(c, ang, light)));
  }
  for (const rad of [63500, 101600]) rec(`glow#${ci}:${rad}`, `${JSON.stringify(c)}|${rad}`, JSON.stringify(fxm.textGlow(rad, c)));
  for (const [w, d] of [[0.75, 'solid'], [0.5, 'dash'], [1.5, 'sysDot'], [2.25, 'solid']]) rec(`outline#${ci}:${w}:${d}`, `${JSON.stringify(c)}|${w}|${d}`, JSON.stringify(fxm.outline(c, w, d)));
});

// ── theme tint / shade ──────────────────────────────────────────────────────────────────────
const TS = [undefined, 0, 0.4, 0.8, 1, 1.5, -0.25, 0.25, 0.005, Infinity, -Infinity, NaN, 1e-9];
for (const tint of TS) for (const shade of TS) {
  const r = themeTintShade({ themeColor: 'accent1', ...(tint !== undefined ? { tint } : {}), ...(shade !== undefined ? { shade } : {}) });
  rec(`tint#${tint}:${shade}`, `${tint ?? '-'}|${shade ?? '-'}`, JSON.stringify(r));
}

// ── the ruler rules ─────────────────────────────────────────────────────────────────────────
const POS = [0, 360, 720, 1440.4, 1440.5, 2000, 3600, 4252, -360, 8504.6];
const VALS = ['left', 'center', 'right', 'decimal', 'bar', 'clear', 'num'];
const LEAD = [undefined, 'none', 'dot', 'hyphen', 'underscore'];
const ALIGNS = ['left', 'center', 'right', 'decimal', 'bar'];
const tabSpecs = (r) => Array.from({ length: r.below(5) }, () => ({ val: VALS[r.below(VALS.length)], pos: POS[r.below(POS.length)], ...(LEAD[r.below(LEAD.length)] ? { leader: LEAD[r.below(LEAD.length)] } : {}) }));
const tabInfos = (r) => Array.from({ length: r.below(5) }, () => ({ pos: POS[r.below(POS.length)], align: ALIGNS[r.below(ALIGNS.length)], ...(['dot', 'hyphen', undefined][r.below(3)] ? { leader: ['dot', 'hyphen'][r.below(2)] } : {}) }));
const encSpecs = (a) => a.map((t) => `${t.val},${t.pos},${t.leader ?? ''}`).join(';');
const encInfos = (a) => a.map((t) => `${t.pos},${t.align},${t.leader ?? ''}`).join(';');
for (let i = 0; i < 300; i++) {
  const inh = rng.below(6) === 0 ? undefined : tabSpecs(rng);
  const want = tabInfos(rng);
  const enc = `${inh === undefined ? '-' : encSpecs(inh)}|${encInfos(want)}`;
  rec(`tabs#${i}`, enc, JSON.stringify(nav.tabsDirect(inh, want) ?? null));
  rec(`dedupe#${i}`, encInfos(want), JSON.stringify(nav.dedupeTabs(want)));
  if (inh && inh.length) rec(`tabinfo#${i}`, encSpecs(inh), JSON.stringify(inh.filter((t) => t.val !== 'clear').map(nav.tabInfo)));
}
const IND = [undefined, 0, 360, 720, -90, 1000.5, 99.4];
for (let i = 0; i < 300; i++) {
  const inh = rng.below(5) === 0 ? undefined : { left: IND[rng.below(IND.length)], right: IND[rng.below(IND.length)], hanging: IND[rng.below(IND.length)], firstLine: IND[rng.below(IND.length)] };
  const want = { left: IND[1 + rng.below(IND.length - 1)], right: IND[1 + rng.below(IND.length - 1)], firstLine: [0, 360, -360, 720.5, -1000.4][rng.below(5)] };
  const e = (x) => (x === undefined ? '-' : String(x));
  const enc = `${inh ? [inh.left, inh.right, inh.hanging, inh.firstLine].map(e).join(',') : '-'}|${want.left},${want.right},${want.firstLine}`;
  rec(`ind#${i}`, enc, JSON.stringify(nav.indentsDirect(inh, want)));
}

// ── the paragraph rules ─────────────────────────────────────────────────────────────────────
for (const k of tl.FLOW_FLAGS) {
  for (const v of [undefined, true, false]) rec(`flowvalue#${k}:${v}`, `${k}|${v ?? '-'}`, JSON.stringify(tl.flowValue(v === undefined ? {} : { [k]: v }, k)));
}
for (const a of [true, false]) for (const b of [true, false]) rec(`flowdirect#${a}:${b}`, `${a}|${b}`, JSON.stringify(tl.flowDirect(a, b) ?? null));
const WANT = [0, 120, 240, 100.5, -5, 31680, 99999, 'auto', 0.4, 12.6];
for (let i = 0; i < 200; i++) {
  const inh = { twips: [0, 120, 240, 100][rng.below(4)], auto: rng.below(3) === 0 };
  const want = WANT[rng.below(WANT.length)];
  const side = rng.below(2) ? 'before' : 'after';
  rec(`spacing#${i}`, `${inh.twips},${inh.auto}|${want}|${side}`, JSON.stringify(tl.spacingDirect(inh, want, side)));
}
// PARAGRAPH_LAYOUT_LIMITS
rec('limits', '-', JSON.stringify(tl.PARAGRAPH_LAYOUT_LIMITS));

// ── plainTextSdt: the two regexes of source.ts (not exported; copied verbatim) ──────────────
const plainTextSdt = (open) => {
  if (!open || !/^<w:sdt[\s>]/.test(open)) return false;
  const end = open.indexOf('</w:sdtPr>');
  return /<w:(?:text|date|comboBox|dropDownList)\b/.test(end < 0 ? open : open.slice(0, end));
};
const SDT = [
  '', '<w:sdt>', '<w:sdt><w:sdtPr><w:text/></w:sdtPr>', '<w:sdt><w:sdtPr><w:text w:multiLine="1"/></w:sdtPr>', '<w:sdt><w:sdtPr><w:date/></w:sdtPr>',
  '<w:sdt><w:sdtPr><w:comboBox>', '<w:sdt><w:sdtPr><w:dropDownList>', '<w:sdt><w:sdtPr><w:textual/></w:sdtPr>', '<w:sdt><w:sdtPr><w:dateX/></w:sdtPr>',
  '<w:sdt><w:sdtPr><w:richText/></w:sdtPr><w:text/>', '<w:sdt><w:sdtPr><w:docPartObj/></w:sdtPr>', '<w:sdtX><w:sdtPr><w:text/></w:sdtPr>', '<w:sdt\n><w:text/>',
  '<w:sdt w:x="1"><w:sdtPr><w:text_/></w:sdtPr>', '<w:sdt><w:text', '<w:sdt><w:sdtPr><w:dropDownList/></w:sdtPr><w:text/>', '<w:sdt>\t<w:sdtPr><w:comboBox/>', ' <w:sdt><w:text/>',
  '<w:sdt><w:sdtPr><w:id w:val="1"/><w:text-x/></w:sdtPr>', '<w:sdt><w:sdtPr><w:textÉ/></w:sdtPr>', '<w:sdt><w:sdtPr><w:text9/>',
];
SDT.forEach((s, i) => rec(`sdt#${i}`, hex(s), String(plainTextSdt(s))));

out.write(join(crate, 'tests', 'parity', 'd2b-f1-ops.capture'));
console.log('d2b-f1-ops: written');
