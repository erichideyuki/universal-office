// Differential capture of the Word colour path (uo-docx plan G12, implemented in
// uo-drawingml `dml`): seeded `a:spPr` fills through the frozen TS `readSpPr` (docx-core
// drawing.ts, which calls `srgbOf`) and `resolveDrawingColor` (docx-layout resolve.ts), written
// with their inputs to tests/captures/dml_color.capture; the Rust test `dml_color_parity`
// replays them through `word_spec` + `WordSpec::to_ts_string` + `resolve_word_spec`.
//
//   UO_TS_ROOT=<built checkout of main> node core/crates/uo-drawingml/tools/capture-dml-color.mjs
//
// Line format: `xml \t fill \t fillAlpha \t resolved` (`~` = undefined).
import { writeFileSync } from 'node:fs';
import { join, dirname } from 'node:path';
import { fileURLToPath } from 'node:url';
import { Rng, importFrom, tsRoot, platform } from '../../uo-testkit/js/testkit.mjs';

const here = dirname(fileURLToPath(import.meta.url));
const root = tsRoot();
const { parseXml } = await importFrom(root, 'write/packages/docx-core/dist/xml.js');
const { readSpPr } = await importFrom(root, 'write/packages/docx-core/dist/drawing.js');
const { resolveDrawingColor } = await importFrom(root, 'write/packages/docx-layout/dist/resolve.js');

const rng = new Rng(0x6d1c0105);
const pick = (a) => a[rng.below(a.length)];
const theme = {
  major: { scripts: {} }, minor: { scripts: {} },
  colors: { dk1: '000000', lt1: 'FFFFFF', dk2: '44546A', lt2: 'E7E6E6', accent1: '4472C4', accent2: 'ED7D31', accent3: 'A5A5A5', accent4: 'FFC000', accent5: '5B9BD5', accent6: '70AD47', hlink: '0563C1', folHlink: '954F72', weird: 'zz00zz' },
};
const VALS = ['4472c4', 'FF0000', '00ff00', '12345', 'zz', '', '0A0B0C', 'ffffff', 'abcdefg'];
const SCHEME = ['accent1', 'accent6', 'tx1', 'tx2', 'bg1', 'bg2', 'dk1', 'lt2', 'phClr', 'text1', 'hyperlink', 'weird', '', 'nope'];
const SYS = ['window', 'windowText', 'btnFace'];
const PRST = ['black', 'white', 'red', 'green', 'blue', 'yellow', 'gray', 'grey', 'navy', ''];
const MODS = ['alpha', 'lumMod', 'lumOff', 'tint', 'shade', 'satMod', 'satOff', 'sat', 'hueOff', 'hueMod', 'hue', 'lum', 'gray', 'comp', 'inv', 'gamma', 'invGamma', 'red', 'bogus'];
const MODVALS = ['75000', '25000', '50000', '-20000', '0', '100000', '120000', '1e3', 'abc', '', ' 5000', '21600000', '60000.5'];

const attr = (k, v) => (v === undefined ? '' : ` ${k}="${v}"`);
function colorEl() {
  const kind = pick(['srgbClr', 'schemeClr', 'sysClr', 'prstClr', 'scrgbClr']);
  const opt = (a) => (rng.below(6) === 0 ? undefined : pick(a));
  let a = '';
  if (kind === 'srgbClr') a = attr('val', opt(VALS));
  else if (kind === 'schemeClr') a = attr('val', opt(SCHEME));
  else if (kind === 'sysClr') a = attr('val', opt(SYS)) + attr('lastClr', rng.below(2) ? pick(VALS) : undefined);
  else if (kind === 'prstClr') a = attr('val', opt(PRST));
  else a = ' r="50000" g="0" b="0"';
  let mods = '';
  for (let k = rng.below(5); k > 0; k--) mods += `<a:${pick(MODS)}${attr('val', rng.below(5) === 0 ? undefined : pick(MODVALS))}/>`;
  return mods ? `<a:${kind}${a}>${mods}</a:${kind}>` : `<a:${kind}${a}/>`;
}

const lines = [
  '# uo-drawingml dml colour differential: readSpPr fill (srgbOf) and resolveDrawingColor of the frozen TS',
  `@platform ${platform()}`,
];
const esc = (v) => (v === undefined ? '~' : String(v));
for (let i = 0; i < 1500; i++) {
  let inner = colorEl();
  if (rng.below(4) === 0) inner = colorEl() + inner;
  const xml = `<a:spPr xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main"><a:solidFill>${inner}</a:solidFill></a:spPr>`;
  const st = readSpPr(parseXml(xml));
  const resolved = resolveDrawingColor(st.fill, theme);
  lines.push([xml, esc(st.fill), st.fillAlpha === undefined ? '~' : String(st.fillAlpha), esc(resolved)].join('\t'));
}
writeFileSync(join(here, '..', 'tests', 'captures', 'dml_color.capture'), lines.join('\n') + '\n');
console.log(`wrote ${lines.length - 2} cases`);
