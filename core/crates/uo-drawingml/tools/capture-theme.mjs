// Differential capture of the theme reader (uo-docx plan G12, uo-drawingml `theme`): the frozen
// TS `readTheme` (docx-core ooxml/parts.ts) over the `word/theme/theme1.xml` of every docx of
// the oracle corpus (write/packages/docx-layout/test/oracle), deduplicated by the SHA-256 of the
// part text; written to tests/captures/theme.capture as `sha256 \t JSON.stringify(theme minus
// the shape-style lists)` (fillStyles/bgFillStyles/effectStyles are Word `readSpPr` output,
// compared by uo-docx R2). The Rust test `theme_parity` maps the as-written view with the Word
// rules and compares.
//
//   UO_TS_ROOT=<built checkout of main> node core/crates/uo-drawingml/tools/capture-theme.mjs
import { createHash } from 'node:crypto';
import { readFileSync, writeFileSync } from 'node:fs';
import { join, dirname } from 'node:path';
import { fileURLToPath } from 'node:url';
import { importFrom, tsRoot, walk, platform, Rng } from '../../uo-testkit/js/testkit.mjs';

const here = dirname(fileURLToPath(import.meta.url));
const root = tsRoot();
const { DocxPackage } = await importFrom(root, 'write/packages/docx-core/dist/package.js');
const { readTheme } = await importFrom(root, 'write/packages/docx-core/dist/ooxml/parts.js');

const corpus = join(here, '..', '..', '..', '..', 'write', 'packages', 'docx-layout', 'test', 'oracle');
const seen = new Map();
for (const f of walk(corpus, (p) => p.endsWith('.docx'))) {
  let text;
  try { text = DocxPackage.load(new Uint8Array(readFileSync(f))).getPartText('word/theme/theme1.xml'); } catch { continue; }
  if (!text) continue;
  const h = createHash('sha256').update(Buffer.from(text, 'utf8')).digest('hex');
  if (seen.has(h)) continue;
  const t = readTheme(text);
  delete t.fillStyles; delete t.bgFillStyles; delete t.effectStyles;
  seen.set(h, JSON.stringify(t));
}
const lines = ['# uo-drawingml theme differential: readTheme of the frozen TS per distinct theme part of the oracle corpus, then seeded synthetic themes (`xml:` lines)', `@platform ${platform()}`];
for (const [h, j] of seen) lines.push(`${h}\t${j}`);
// synthetic themes: the shapes the corpus lacks (scripts, sysClr variants, missing lists, odd children)
const rng = new Rng(0x7e3e0001);
const pick = (a) => a[rng.below(a.length)];
const opt = (s) => (rng.below(3) === 0 ? '' : s);
const A = 'xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main"';
const clr = () => pick([
  '<a:srgbClr val="1f497d"/>', '<a:srgbClr val=""/>', '<a:srgbClr/>', '<a:sysClr val="window"/>', '<a:sysClr val="windowText"/>',
  '<a:sysClr val="other" lastClr="abcdef"/>', '<a:sysClr val="other"/>', '<a:prstClr val="red"/>', '', '<a:srgbClr val="ff0000"/><a:sysClr val="window"/>',
]);
const font = (tag) => opt(`<a:${tag}${opt(` typeface="${pick(['Calibri', '', 'MS 明朝', 'Times'])}"`)}/>`);
const fontSet = (tag) => opt(`<a:${tag}>${font('latin')}${font('ea')}${font('cs')}${opt('<a:font script="Jpan" typeface="游明朝"/>')}${opt('<a:font script="Hang"/>')}${opt('<a:font typeface="X"/>')}${opt('<a:font script="0" typeface="Zero"/>')}</a:${tag}>`);
for (let i = 0; i < 300; i++) {
  const scheme = opt(`<a:clrScheme name="s">${['dk1', 'lt1', 'accent1', 'hlink', 'x:odd'].map((k) => opt(`<a:${k}>${clr()}</a:${k}>`)).join('')}</a:clrScheme>`);
  const fonts = opt(`<a:fontScheme name="f">${fontSet('majorFont')}${fontSet('minorFont')}</a:fontScheme>`);
  const ln = opt(`<a:fmtScheme><a:lnStyleLst>${opt('<a:ln w="6350"/>')}${opt('<a:ln/>')}${opt('<a:ln w="abc"/>')}${opt('<a:ln w="-12700"/>')}</a:lnStyleLst></a:fmtScheme>`);
  const xml = `<a:theme ${A}><a:themeElements>${rng.below(2) ? scheme + fonts : fonts + scheme}${ln}</a:themeElements></a:theme>`;
  const t = readTheme(xml);
  delete t.fillStyles; delete t.bgFillStyles; delete t.effectStyles;
  lines.push(`xml:${xml}\t${JSON.stringify(t)}`);
}
writeFileSync(join(here, '..', 'tests', 'captures', 'theme.capture'), lines.join('\n') + '\n');
console.log(`${seen.size} distinct themes`);
