// Parity capture of the `table-styles` facet (frontier R2 of the D1a reader): the frozen TS
// `table-styles.ts` (read side: `CORE_TABLE_STYLES`, `localStyleId`, `loadBuiltinTableStyles`,
// `themeHex`, `materializeStyleXml`, `styleDefOf`, `styleIdByName`) and `style-names-es.ts`
// (`tableStyleNameEs`), over the built-in styles, the themes of the corpus and seeded colour sets,
// the formatting of the corpus documents, and a list of names.
//
//   UO_TS_ROOT=<built checkout of main> node core/crates/uo-docx/js/capture-table-styles.mjs
//
// Writes tests/parity/table-styles.capture and tests/parity/table-styles.inputs (the colour sets
// and the names, so the Rust replay reads the same text). Records (digests of `uo-testkit` streams
// of the strings the functions return; names as the string itself):
//   `builtin`           the 247 built-in styles (name, local, id, modern, xml)
//   `core`              `CORE_TABLE_STYLES` and `localStyleId` over a list of local names
//   `themeHex#<n>`      `themeHex` over the theme colours x tint/shade combinations, colour set n
//   `materialize#<n>`   `materializeStyleXml` of every built-in style, colour set n
//   `styleDefOf`        `styleDefOf` of every built-in style (materialized with the Office theme)
//                       and of odd inputs: the canonical digest of the definitions
//   `idByName#<doc>`    `styleIdByName` over the formatting of each corpus document
//   `es#<n>`            `tableStyleNameEs(name n)`: the JSON string, `U` (undefined), `T` (throws:
//                       a name with a line terminator makes `exec` null) or `X` (not a string:
//                       `TABLE_FIXED` is a plain object, so `constructor` finds a function, and
//                       `${es} - Énfasis 1` prints it)
import { writeFileSync } from 'node:fs';
import { join } from 'node:path';
import { CaptureWriter, Rng, Stream } from '../../uo-testkit/js/testkit.mjs';
import { R2Canon, crate, corpus, readBytes, ts } from './capture-r2-common.mjs';

const { DocxPackage } = await ts('package.js');
const parts = await ts('ooxml/parts.js');
const tsStyles = await ts('table-styles.js');
const { tableStyleNameEs } = await ts('style-names-es.js');

const esc = (x) => x.replace(/\\/g, '\\\\').replace(/\n/g, '\\n').replace(/\r/g, '\\r').replace(/\t/g, '\\t');
const rng = new Rng(0x7ab1e5);
const pick = (a) => a[rng.below(a.length)];

// ── colour sets: the distinct themes of the corpus, then seeded ones ──
const colorSets = [{}];
const seen = new Set();
for (const { file } of corpus()) {
  let pkg;
  try { pkg = DocxPackage.load(readBytes(file)); } catch { continue; }
  const f = parts.readDocFormatting(pkg);
  const k = JSON.stringify(f.theme.colors);
  if (seen.has(k)) continue;
  seen.add(k);
  colorSets.push(f.theme.colors);
}
const KEYS = ['dk1', 'lt1', 'dk2', 'lt2', 'accent1', 'accent2', 'accent3', 'accent4', 'accent5', 'accent6', 'hlink', 'folHlink', 'weird', 'text1', 'background1', '', '0', 'constructor'];
const VALUES = ['4472C4', 'ABC', '', 'FFFFFFF', 'zz00zz', '000000', '1F497D', 'ffffff', '0e2841', 'F', '12345G', 'FF0000', '808080', 'FFFFFF', '7F7F7F'];
for (let i = 0; i < 24; i++) {
  const c = {};
  for (const k of KEYS) if (rng.below(2)) c[k] = pick(VALUES);
  colorSets.push(c);
}

const THEME_COLORS = ['text1', 'background1', 'text2', 'background2', 'dark1', 'light1', 'dark2', 'light2', 'hyperlink', 'followedHyperlink', 'dk1', 'lt1', 'dk2', 'lt2',
  'accent1', 'accent2', 'accent3', 'accent4', 'accent5', 'accent6', 'hlink', 'folHlink', 'weird', '', 'Accent1', 'constructor', '__proto__', 'toString', 'hasOwnProperty', 'valueOf', 'isPrototypeOf', 'toLocaleString'];
const COMBOS = [[undefined, undefined], [undefined, ''], ['', undefined], ['80', undefined], ['BF', undefined], [undefined, '40'], ['ff', '00'], ['00', 'ff'], ['zz', '1'], ['1', 'zz'], ['', ''],
  ['A', 'B'], ['FFF', undefined], ['8000000', undefined], [undefined, 'BF'], ['99', '99'], ['33', undefined], [undefined, 'ff'], ['20', 'ff']];

// ── names ──
const NAMES = new Set(['', 'Normal Table', 'Table Grid', 'Grid Table Light', 'Table Theme', 'Table Contemporary', 'Table Elegant', 'Table Professional', 'x', ' Accent 1', 'Accent 1', 'Table Grid Accent 1',
  'Grid Table 4 Accent 1', 'Grid Table 4 Accent 9', 'Grid Table 4 Accent 10', 'Grid Table 4 Accent ', 'Grid Table 4  Accent 1', 'Grid Table 4 accent 1', 'Grid Table 4 Accent 1 ', 'Grid Table 44', 'Grid Table 4 Light', 'Grid Table 4 Light Accent 2',
  'Grid Table 7 Colorful Accent 6', 'List Table 1 Dark Accent 3', 'Plain Table 5', 'Plain Table 5 Accent 1', 'Medium Shading 1 Accent 2', 'Medium Shading', 'Light Shading Accent 1', 'Colorful Grid Accent 4', 'Table 3D effects 2', 'Table 3D effects 22',
  'Table Classic 3', 'Table Subtle 1', 'Table Web 3', 'Table Columns 5', 'Table List 8', 'Table Simple 2', 'Table Colorful 1', 'Table Grid 5', 'Light List - Accent 1', 'Light Grid Accent 5', 'Dark List Accent 1', 'Colorful List Accent 1',
  'constructor', 'toString', '__proto__', 'hasOwnProperty', 'valueOf', 'constructor Accent 1', 'Table\nGrid', 'Table Grid\r', 'Table Grid Accent 1', 'Table Grid Accent 1\n', '٣', 'Grid Table ٣', 'Grid Table 4 Accent ٣', 'Tabela com grade', 'GRID TABLE 4', 'Grid Table 4😀', 'é Accent 1', 'Table Grid Accent 12', 'Table Grid Accent 1Accent 2']);
for (const s of await tsStyles.loadBuiltinTableStyles()) NAMES.add(s.name);
const names = [...NAMES];
const XMLS = ['', 'x', '<w:style/>', '<w:style w:type="table"/>', '<w:style w:type="table" w:styleId="A"><w:name w:val="N"/></w:style>', '<w:style w:type="table" w:styleId="A"/><w:style w:type="table" w:styleId="B"/>',
  '<w:style w:type="paragraph" w:styleId="5"/><w:style w:type="table" w:styleId="A"/>', '<w:style w:type="table" w:styleId="__proto__"><w:name w:val="p"/></w:style>', '<w:style w:type="table" w:styleId="A"><w:tblPr><w:tblW w:w="1" w:type="dxa"/></w:tblPr></w:style>'];
const LOCALS = ['Tabela normal', 'Tabela com grade', 'Tabela Simples 1', 'Grade Clara - Ênfase 1', '', 'a-b_c.d e', '😀x', 'Tabela com Grade 3-A', 'çÇãõ', '---', 'a\u0000b'];

// ── inputs file ──
const lines = ['# uo-docx R2 table-styles inputs (js/capture-table-styles.mjs): `colors` TAB key TAB value ... | `name` TAB escaped | `xml` TAB escaped | `local` TAB escaped'];
for (const c of colorSets) lines.push(['colors', ...Object.entries(c).flatMap(([k, v]) => [esc(k), esc(v)])].join('\t'));
for (const n of names) lines.push(`name\t${esc(n)}`);
for (const x of XMLS) lines.push(`xml\t${esc(x)}`);
for (const l of LOCALS) lines.push(`local\t${esc(l)}`);
writeFileSync(join(crate, 'tests', 'parity', 'table-styles.inputs'), lines.join('\n') + '\n');

const cap = new CaptureWriter(['table-styles facet of the D1a reader (frontier R2): see js/capture-table-styles.mjs']);
const strs = (st, xs) => { for (const x of xs) st.str(x); return st.finish().short; };

const builtin = await tsStyles.loadBuiltinTableStyles();
{
  const st = new Stream();
  st.u32(builtin.length);
  for (const b of builtin) { st.str(b.name); st.str(b.local); st.str(b.id); st.bool(b.modern); st.str(b.xml); }
  cap.set('builtin', 'list', st.finish().short);
  cap.set('builtin', 'count', String(builtin.length));
}
{
  const st = new Stream();
  for (const [name, d] of Object.entries(tsStyles.CORE_TABLE_STYLES)) { st.str(name); st.str(d.id); st.str(d.local); st.str(d.xml); }
  for (const l of LOCALS) st.str(tsStyles.localStyleId(l));
  cap.set('core', 'core', st.finish().short);
}
colorSets.forEach((c, n) => {
  const out = [];
  for (const tc of THEME_COLORS) for (const [tint, shade] of COMBOS) out.push(tsStyles.themeHex(c, tc, tint, shade));
  cap.set(`themeHex#${n}`, 'themeHex', strs(new Stream(), out));
  const mat = builtin.map((b) => tsStyles.materializeStyleXml(b.xml, c));
  cap.set(`materialize#${n}`, 'materialize', strs(new Stream(), mat));
});
{
  const st = new Stream();
  const canon = new R2Canon(st);
  for (const b of builtin) canon.value(tsStyles.styleDefOf(tsStyles.materializeStyleXml(b.xml, {})), 'def');
  for (const x of XMLS) {
    let d;
    try { d = tsStyles.styleDefOf(x); } catch { st.str('throws'); continue; }
    canon.value(d, 'def');
  }
  cap.set('styleDefOf', 'defs', st.finish().short);
}
const BY_NAME = ['Table Grid', 'table grid', 'NORMAL TABLE', 'x', 'Tabela com grade', '', 'Light Grid', 'İ'];
let docs = 0;
for (const { rel, file } of corpus()) {
  let pkg;
  try { pkg = DocxPackage.load(readBytes(file)); } catch { continue; }
  const f = parts.readDocFormatting(pkg);
  const st = new Stream();
  for (const n of BY_NAME) { const r = tsStyles.styleIdByName(f, n); st.bool(r !== undefined); if (r !== undefined) st.str(r); }
  st.bool(tsStyles.styleIdByName(undefined, 'Table Grid') === undefined);
  cap.set(`${rel}#idByName`, 'idByName', st.finish().short);
  docs++;
}
names.forEach((n, i) => {
  let r;
  try { r = tableStyleNameEs(n); } catch { cap.set(`es#${i}`, 'es', 'T'); return; }
  cap.set(`es#${i}`, 'es', r === undefined ? 'U' : typeof r === 'string' && !/native code|\[object /.test(r) ? JSON.stringify(r) : 'X');
});
cap.meta('corpus', `colorSets=${colorSets.length} names=${names.length} builtin=${builtin.length} docs=${docs}`);
cap.write(join(crate, 'tests', 'parity', 'table-styles.capture'));
console.log(`colour sets ${colorSets.length}, names ${names.length}, builtin ${builtin.length}, docs ${docs}`);
