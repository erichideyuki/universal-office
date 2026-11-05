// Parity capture of shape-xml.ts (the D1b frontier W2): the frozen TS `patchShape`, `vmlFallback`,
// `textboxAlternateContent`, the write functions (`shapeFillXml`, `styleRefsXml`, `bodyPrXml`,
// `newTextboxDrawingXml`, `hiddenLineXml`, `vmlPt`, `vmlColorOf`), `txbxContentOf` and the read half
// (`readShapeFormat`, checked through the XML it writes back) over every `w:drawing` with a
// `wps:wsp` of the corpus (the first docx of the oracle corpus and the docx-core fixtures), each with the
// named changes below (the same lists are implemented by tests/parity_shape_xml.rs); plus the private
// `shapeColor` of serialize.ts (copied below verbatim) over the colours of those shapes and a seeded grid.
//
//   UO_TS_ROOT=<built checkout of main> node core/crates/uo-docx/js/capture-shape-xml.mjs
//
// Writes tests/parity/shape-xml.inputs (`<escaped raw drawing>` per line), tests/parity/shape-xml.colors
// (`kind|val|lastClr|mod:val,mod:val` per line) and tests/parity/shape-xml.capture, one record per
// `<kind>#<index>#<variant>`: `out` = short SHA-256 of the result (or `err` when the TS throws).
import { writeFileSync } from 'node:fs';
import { join } from 'node:path';
import { createHash } from 'node:crypto';
import { CaptureWriter } from '../../uo-testkit/js/testkit.mjs';
import { corpus, crate, readBytes, ts } from './capture-r2-common.mjs';

const { DocxPackage } = await ts('package.js');
const S = await ts('shape-xml.js');

const sha = (s) => createHash('sha256').update(s).digest('hex').slice(0, 32);
const esc = (x) => x.replace(/\\/g, '\\\\').replace(/\n/g, '\\n').replace(/\r/g, '\\r').replace(/\t/g, '\\t');

// serialize.ts:238, 275–297 (verbatim: `shapeTheme` is the default theme of `WriteCtx`)
const shapeTheme = { dk1: '000000', lt1: 'FFFFFF', dk2: '0E2841', lt2: 'E8E8E8', accent1: '156082', accent2: 'E97132', accent3: '196B24', accent4: '0F9ED5', accent5: 'A02B93', accent6: '4EA72E', hlink: '467886', folHlink: '96607D' };
function shapeColor(c) {
  const base = c.kind === 'srgbClr' ? c.val : c.kind === 'prstClr' ? ({ black: '000000', white: 'FFFFFF', red: 'FF0000' })[c.val] ?? '000000'
    : c.kind === 'sysClr' ? c.lastClr ?? (c.val === 'window' ? 'FFFFFF' : '000000')
      : shapeTheme[({ tx1: 'dk1', bg1: 'lt1', tx2: 'dk2', bg2: 'lt2' })[c.val] ?? c.val] ?? '000000';
  let [r, g, b] = [0, 2, 4].map((i) => parseInt(base.slice(i, i + 2), 16) / 255);
  for (const [n, v0] of c.mods ?? []) {
    const v = v0 / 100000;
    if (n === 'shade') { r *= v; g *= v; b *= v; } else if (n === 'tint') { r = 1 - (1 - r) * v; g = 1 - (1 - g) * v; b = 1 - (1 - b) * v; } else if (n === 'lumMod' || n === 'lumOff') {
      const mx = Math.max(r, g, b), mn = Math.min(r, g, b);
      let l = (mx + mn) / 2;
      const d = mx - mn;
      const s = d === 0 ? 0 : d / (1 - Math.abs(2 * l - 1));
      let hh = d === 0 ? 0 : mx === r ? ((g - b) / d) % 6 : mx === g ? (b - r) / d + 2 : (r - g) / d + 4;
      hh = (hh * 60 + 360) % 360;
      l = n === 'lumMod' ? l * v : Math.min(1, l + v);
      const C = (1 - Math.abs(2 * l - 1)) * s, X = C * (1 - Math.abs(((hh / 60) % 2) - 1)), m = l - C / 2;
      const [a1, b1, c1] = hh < 60 ? [C, X, 0] : hh < 120 ? [X, C, 0] : hh < 180 ? [0, C, X] : hh < 240 ? [0, X, C] : hh < 300 ? [X, 0, C] : [C, 0, X];
      [r, g, b] = [a1 + m, b1 + m, c1 + m];
    }
  }
  return [r, g, b].map((x) => Math.round(Math.max(0, Math.min(1, x)) * 255).toString(16).padStart(2, '0')).join('').toUpperCase();
}

// ───────────────────────── the named changes (mirrored in tests/parity_shape_xml.rs) ─────────────────────────

const C1 = { kind: 'srgbClr', val: 'FF0000', mods: [['alpha', 50000], ['gray', 0]] };
const C2 = { kind: 'schemeClr', val: 'accent1', mods: [['lumMod', 75000], ['lumOff', 25000]] };
const C3 = { kind: 'sysClr', val: 'windowText', lastClr: '000000' };
const GRAD = { kind: 'gradient', stops: [{ pos: 0.4, color: C1 }, { pos: 100000, color: C2 }], lin: { ang: 5400000.6, scaled: true } };
const GRADP = { kind: 'gradient', stops: [{ pos: 0, color: C3 }], path: { path: 'rect', fillToRect: { l: 50000, t: 50000, r: 50000, b: 50000 } }, rotWithShape: false, flip: 'x', tileRect: true };
const GRADN = { kind: 'gradient', stops: [] };
const PATT = { kind: 'pattern', prst: 'dkUpDiag', fg: C1, bg: C2 };
const PIC = { kind: 'picture', embed: 'rId9', tile: { tx: 1, ty: 2, sx: 3, sy: 4, flip: 'xy', algn: 'ctr' }, alpha: 40000 };
const PIC2 = { kind: 'picture', embed: 'rId8', alpha: 100000 };
const LINE1 = { w: 19050, cap: 'rnd', cmpd: 'dbl', algn: 'ctr', fill: C2, dash: 'sysDash', join: 'miter', miterLim: 800000 };
const LINE2 = { fill: null, tailRaw: '<a:tailEnd type="arrow"/>' };
const LINE3 = { w: 6350, join: 'round', fillRaw: '<a:gradFill/>' };
const REFS = {
  ln: { idx: 2, color: C2 }, fill: { idx: 1, color: null }, effect: { idx: 0, color: C1 }, font: { idx: 'minor', color: C3 },
};
const PRESET = {
  spPr: '<a:solidFill><a:schemeClr val="accent1"/></a:solidFill><a:ln w="12700"><a:solidFill><a:schemeClr val="accent1"><a:shade val="50000"/></a:schemeClr></a:solidFill></a:ln>',
  style: '<wps:style><a:lnRef idx="2"><a:schemeClr val="accent1"><a:shade val="15000"/></a:schemeClr></a:lnRef><a:fillRef idx="1"><a:schemeClr val="accent1"/></a:fillRef><a:effectRef idx="0"><a:schemeClr val="accent1"/></a:effectRef><a:fontRef idx="minor"><a:schemeClr val="lt1"/></a:fontRef></wps:style>',
};

export const CHANGES = [
  ['none', {}],
  ['fill-none', { fill: { kind: 'none' } }],
  ['fill-style', { fill: { kind: 'style' } }],
  ['fill-solid', { fill: { kind: 'solid', color: C1 } }],
  ['fill-grad', { fill: GRAD }],
  ['fill-gradp', { fill: GRADP }],
  ['fill-gradn', { fill: GRADN }],
  ['fill-patt', { fill: PATT }],
  ['fill-pic', { fill: PIC }],
  ['fill-pic2', { fill: PIC2 }],
  ['fill-raw', { fill: { kind: 'raw', xml: '<a:grpFill/>' } }],
  ['outline-null', { outline: null }],
  ['outline-1', { outline: LINE1 }],
  ['outline-2', { outline: LINE2 }],
  ['outline-3', { outline: LINE3 }],
  ['outline-empty', { outline: {} }],
  ['style-null', { style: null }],
  ['style-refs', { style: REFS }],
  ['body-ins', { body: { lIns: 0, tIns: 1000.4, anchor: 'ctr', anchorCtr: true } }],
  ['body-fit', { body: { autofit: 'shape', wrap: 'none', upright: true } }],
  ['body-vert', { body: { vert: 'vert270', rot: 5400000, autofit: 'norm' } }],
  ['body-clear', { body: { upright: false, anchorCtr: false, anchor: 'b' } }],
  ['body-same', { body: { anchor: 't', wrap: 'square', vert: 'horz' } }],
  ['story-linked', { story: { kind: 'linked', id: 5, seq: 2 } }],
  ['story-chain', { story: { kind: 'txbx', id: 3 } }],
  ['story-content', { story: { kind: 'txbx', content: '<w:p/>' } }],
  ['story-id-content', { story: { kind: 'txbx', id: 7, content: '<w:p><w:r><w:t>x</w:t></w:r></w:p>' } }],
  ['story-plain', { story: { kind: 'txbx' } }],
  ['preset', { stylePreset: PRESET }],
  ['combo', { outline: null, fill: { kind: 'solid', color: C2 }, body: { lIns: 5, anchor: 'ctr' }, style: null }],
  ['combo2', { stylePreset: PRESET, outline: LINE1, fill: GRAD, story: { kind: 'linked', id: 2, seq: 1 } }],
  ['pic-rot', { rot: 5400000, flipH: true, flipV: false }],
  ['pic-alt', { descr: 'a "b" <c>', title: 'T & U', hidden: true, name: 'Forma 9' }],
  ['pic-size', { size: { cx: 1234567, cy: 765432 }, effectExtent: { l: 1, t: 2, r: 3, b: 4 } }],
  ['pic-wrap', { wrap: 'square', behindDoc: false }],
  ['pic-line', { line: LINE1 }],
  ['pic-geom', { geom: { prst: 'roundRect', adj: { adj: 12345 } } }],
];

const DP = { ln: { idx: 1, color: C1 }, fill: { idx: 3, color: null }, effect: { idx: 2, color: C3 }, font: { idx: 'major', color: null } };

const NEWS = [
  ['default', { id: 3, name: 'Caixa de Texto 2', cx: 1828800, cy: 914400, relativeHeight: 251659264, content: '<w:p><w:r><w:t>x</w:t></w:r></w:p>' }],
  ['empty', { id: 4, name: 'Text Box 3 & <x>', cx: 100.4, cy: 200.6, relativeHeight: 5, content: '' }],
  ['behind', { id: 5, name: 'b', cx: 10, cy: 20, relativeHeight: 7, content: '<w:p/>', behindDoc: true, wrap: 'square', dist: { t: 1, b: 2, l: 3, r: 4 } }],
  ['topbottom', { id: 6, name: 'b', cx: 10, cy: 20, relativeHeight: 7, content: '<w:p/>', wrap: 'topAndBottom' }],
  ['wrap-none', { id: 6, name: 'b', cx: 10, cy: 20, relativeHeight: 7, content: '<w:p/>', wrap: 'none' }],
  ['line-null', { id: 7, name: 'n', cx: 1, cy: 1, relativeHeight: 1, content: '', line: null }],
  ['line-1', { id: 7, name: 'n', cx: 1, cy: 1, relativeHeight: 1, content: '', line: LINE1 }],
  ['line-2', { id: 7, name: 'n', cx: 1, cy: 1, relativeHeight: 1, content: '', line: LINE2 }],
  ['line-3', { id: 7, name: 'n', cx: 1, cy: 1, relativeHeight: 1, content: '', line: LINE3 }],
  ['line-bevel', { id: 7, name: 'n', cx: 1, cy: 1, relativeHeight: 1, content: '', line: { join: 'bevel', cap: 'flat' } }],
  ['line-empty', { id: 7, name: 'n', cx: 1, cy: 1, relativeHeight: 1, content: '', line: {} }],
  ['fill-none', { id: 8, name: 'f', cx: 1, cy: 1, relativeHeight: 1, content: '', fill: { kind: 'none' } }],
  ['fill-style', { id: 8, name: 'f', cx: 1, cy: 1, relativeHeight: 1, content: '', fill: { kind: 'style' } }],
  ['fill-grad', { id: 8, name: 'f', cx: 1, cy: 1, relativeHeight: 1, content: '', fill: GRAD }],
  ['fill-gradp', { id: 8, name: 'f', cx: 1, cy: 1, relativeHeight: 1, content: '', fill: GRADP }],
  ['fill-patt', { id: 8, name: 'f', cx: 1, cy: 1, relativeHeight: 1, content: '', fill: PATT }],
  ['fill-pic', { id: 8, name: 'f', cx: 1, cy: 1, relativeHeight: 1, content: '', fill: PIC }],
  ['fill-raw', { id: 8, name: 'f', cx: 1, cy: 1, relativeHeight: 1, content: '', fill: { kind: 'raw', xml: '<a:grpFill/>' } }],
  ['pos-pct', { id: 9, name: 'p', cx: 1, cy: 1, relativeHeight: 1, content: '', posH: { relativeFrom: 'page', pct: 50000, fallback: 123.5 }, posV: { relativeFrom: 'margin', align: 'center' } }],
  ['pos-offset', { id: 9, name: 'p', cx: 1, cy: 1, relativeHeight: 1, content: '', posH: { relativeFrom: 'margin', offset: 1000.5 }, posV: { relativeFrom: 'page', pct: 1000 } }],
  ['pos-align', { id: 9, name: 'p', cx: 1, cy: 1, relativeHeight: 1, content: '', posH: { relativeFrom: 'column', align: 'right', pct: 5 }, posV: { relativeFrom: 'line' } }],
  ['size-rel', { id: 9, name: 'p', cx: 1, cy: 1, relativeHeight: 1, content: '', sizeRelH: { relativeFrom: 'page', pct: 50000.4 }, sizeRelV: { relativeFrom: 'margin', pct: 25000 } }],
  ['geom-adj', { id: 10, name: 'g', cx: 1, cy: 1, relativeHeight: 1, content: '', geom: { prst: 'roundRect', adj: { adj: 16667.4, adj2: 5, 7: 1, adj1: 2 } } }],
  ['geom-empty', { id: 10, name: 'g', cx: 1, cy: 1, relativeHeight: 1, content: '', geom: { prst: 'ellipse', adj: {} } }],
  ['geom-noadj', { id: 10, name: 'g', cx: 1, cy: 1, relativeHeight: 1, content: '', geom: { prst: 'triangle' } }],
  ['style', { id: 11, name: 's', cx: 1, cy: 1, relativeHeight: 1, content: '', style: DP }],
  ['body', { id: 12, name: 'b', cx: 1, cy: 1, relativeHeight: 1, content: '', body: { lIns: 0, anchor: 'b', autofit: 'shape', vert: 'vert270', upright: true, anchorCtr: true, wrap: 'none', rot: 90 } }],
  ['body-norm', { id: 12, name: 'b', cx: 1, cy: 1, relativeHeight: 1, content: '', body: { autofit: 'norm', tIns: 0.5 } }],
  ['linked', { id: 13, name: 'l', cx: 1, cy: 1, relativeHeight: 1, content: 'ignored', linked: { id: 5, seq: 2 } }],
  ['sppr-raw', { id: 14, name: 'r', cx: 1, cy: 1, relativeHeight: 1, content: '', spPrRaw: '<a:noFill/><a:ln/>', fill: GRAD, line: LINE1, spPrExtra: '<a:effectLst/>' }],
  ['sppr-extra', { id: 14, name: 'r', cx: 1, cy: 1, relativeHeight: 1, content: '', spPrExtra: '<a:effectLst/>' }],
];

const VMLPT = [0, 1, -1, 12700, 6350, 3175, 9525, 635, -635, 457200, 1270, 12699, 12701, 0.4, 1e9, -1e9, 38100.5, 91440, 45720, 254, NaN, Infinity, -0, 1e21, 5e-324];
const VMLCOLOR = ['000000', 'ffffff', 'FF0000', '00ff00', '0000FF', 'FFFF00', 'AABBCC', 'aabbcc', 'AaBbCc', '112233', '123456', 'NAN', 'ABC', '', 'A', 'ÁÁbbcc', 'ß', '#112233', '11223', '1122334455'];
const VMLFLAGS = [['plain', { spid: 4097 }], ['next', { next: 'Text Box 9 & "x"', spid: 4100 }]];

// ───────────────────────── the corpus ─────────────────────────

const raws = new Set();
for (const { file } of corpus()) {
  let pkg;
  try { pkg = DocxPackage.load(readBytes(file)); } catch { continue; }
  for (const part of pkg.partNames().filter((p) => /^word\/(document|header\d*|footer\d*|footnotes|endnotes)\.xml$/.test(p))) {
    const xml = pkg.getPartText(part) ?? '';
    for (const m of xml.matchAll(/<w:drawing\b[\s\S]*?<\/w:drawing>/g)) if (m[0].includes('<wps:wsp')) raws.add(m[0]);
  }
}
const drawings = [...raws];
for (const [, o] of NEWS) drawings.push(S.newTextboxDrawingXml(o));

const out = new CaptureWriter(['shape-xml.ts patchShape/vmlFallback/textboxAlternateContent/write functions over corpus shapes x named changes', 'facet out: short sha256 of the result, or err']);
let n = 0;
const rec = (key, f) => {
  let r;
  try { r = f(); } catch { r = 'err'; }
  out.set(key, 'out', r === 'err' ? 'err' : sha(String(r)));
  n++;
};
const CONTENT = '<w:p><w:r><w:t>a &amp; b</w:t></w:r></w:p>';
const colors = new Map();
const addColor = (c) => {
  if (!c) return;
  const key = `${c.kind}|${c.val}|${c.lastClr ?? ''}|${(c.mods ?? []).map(([a, b]) => `${a}:${b}`).join(',')}`;
  if (!colors.has(key)) colors.set(key, c);
};
const stable = (x) => JSON.stringify(x);

drawings.forEach((raw, i) => {
  for (const [name, ch] of CHANGES) rec(`patch#${i}#${name}`, () => S.patchShape(raw, ch));
  for (const [v, o] of VMLFLAGS) {
    rec(`vml#${i}#${v}`, () => S.vmlFallback(raw, CONTENT, { color: shapeColor, ...o }));
    rec(`alt#${i}#${v}`, () => S.textboxAlternateContent(raw, CONTENT, { color: shapeColor, ...o }));
  }
  rec(`txbx#${i}`, () => String(S.txbxContentOf(raw)));
  rec(`fx#${i}`, () => {
    const f = S.readShapeFormat(raw);
    if (!f) return 'none';
    for (const k of [f.fill?.color, f.fill?.fg, f.fill?.bg, ...(f.fill?.stops ?? []).map((s) => s.color), f.line?.fill, f.style?.ln.color, f.style?.fill.color, f.style?.effect.color, f.style?.font.color]) addColor(k);
    const fm = [f.textbox, f.normalEastAsianFlow, f.story ? stable(f.story) : '-'].join(',');
    return [S.shapeFillXml(f.fill), f.style ? S.styleRefsXml(f.style) : '-', f.body ? S.bodyPrXml(f.body) : '-', fm].join('|');
  });
  rec(`hidden#${i}`, () => {
    const f = S.readShapeFormat(raw);
    const h = S.hiddenLineXml(f?.line);
    return `${h.ln}|${h.ext}`;
  });
});
NEWS.forEach(([name, o]) => rec(`new#${name}`, () => S.newTextboxDrawingXml(o)));
VMLPT.forEach((x, k) => rec(`vmlpt#${k}`, () => S.vmlPt(x)));
VMLCOLOR.forEach((x, k) => rec(`vmlcolor#${k}`, () => S.vmlColorOf(x)));

// the colours of the shapes and a grid of kinds × values × modifiers
const MODS = ['', 'shade:50000', 'tint:25000', 'lumMod:75000', 'lumOff:25000', 'lumMod:50000,lumOff:50000', 'alpha:50000', 'lumMod:120000,lumOff:-5000', 'shade:0', 'lumOff:200000', 'hue:1'];
for (const kind of ['srgbClr', 'schemeClr', 'prstClr', 'sysClr']) {
  const vals = { srgbClr: ['FF0000', '00FF00', '1F497D', 'abc', '', 'GG0000', '808080', 'FFFFFF', '000000', '4472C4'], schemeClr: ['accent1', 'accent6', 'tx1', 'bg1', 'tx2', 'bg2', 'dk1', 'lt2', 'hlink', 'phClr', 'constructor', 'toString', '__proto__'], prstClr: ['black', 'white', 'red', 'green', 'blue', 'constructor', 'toString', '__proto__', 'valueOf'], sysClr: ['window', 'windowText', 'x'] }[kind];
  for (const val of vals) for (const m of MODS) {
    addColor({ kind, val, ...(kind === 'sysClr' ? { lastClr: val === 'x' ? undefined : (val === 'window' ? 'F0F0F0' : '101010') } : {}), ...(m ? { mods: m.split(',').map((p) => { const [a, b] = p.split(':'); return [a, Number(b)]; }) } : {}) });
  }
}
const colorLines = [...colors.entries()];
colorLines.forEach(([, c], k) => rec(`color#${k}`, () => shapeColor(c)));

writeFileSync(join(crate, 'tests', 'parity', 'shape-xml.inputs'), `# shape-xml.ts inputs (js/capture-shape-xml.mjs): one escaped raw w:drawing per line (the last ${NEWS.length} are the new boxes of NEWS)\n${drawings.map(esc).join('\n')}\n`);
writeFileSync(join(crate, 'tests', 'parity', 'shape-xml.colors'), `# shape-xml.ts colours (js/capture-shape-xml.mjs): kind|val|lastClr|mod:val,mod:val\n${colorLines.map(([k]) => esc(k)).join('\n')}\n`);
out.write(join(crate, 'tests', 'parity', 'shape-xml.capture'));
console.log(`shape-xml: ${drawings.length} drawings (${NEWS.length} new), ${colorLines.length} colours, ${n} records`);
