// The regex sites of frontier R2 (ooxml/parts.ts, table-styles.ts, style-names-es.ts) against V8,
// checked by tests/regex_r2.rs. Row ids are listed with their TS file:line in
// core/docs/modules/uo-docx-d1a/R2.md item 2.

const W = 'width', H = 'height';
const vml = (prop) => new RegExp(`(?:^|;)\\s*${prop}:\\s*([0-9.]+)\\s*(pt|in|px|cm|mm|pc)?`);
const vmlUnit = (prop) => new RegExp(`(?:^|;)\\s*${prop}:\\s*[0-9.]+\\s*(pt|in|px|cm|mm|pc)?`);
const attr = (name) => new RegExp(`${name}="([^"]*)"`);

/** The TABLE_FAMILY rules of style-names-es.ts, in order, with their replacements. */
const FAMILY = [
  [/^Grid Table (\d)$/, 'Tabla de cuadrícula $1'],
  [/^Grid Table (\d) Light$/, 'Tabla de cuadrícula $1 clara'],
  [/^Grid Table (\d) Dark$/, 'Tabla de cuadrícula $1 oscura'],
  [/^Grid Table (\d) Colorful$/, 'Tabla de cuadrícula $1 con colores'],
  [/^List Table (\d)$/, 'Tabla de lista $1'],
  [/^List Table (\d) Light$/, 'Tabla de lista $1 clara'],
  [/^List Table (\d) Dark$/, 'Tabla de lista $1 oscura'],
  [/^List Table (\d) Colorful$/, 'Tabla de lista $1 con colores'],
  [/^Plain Table (\d)$/, 'Tabla normal $1'],
  [/^Light Shading$/, 'Sombreado claro'], [/^Medium Shading (\d)$/, 'Sombreado medio $1'],
  [/^Dark Shading$/, 'Sombreado oscuro'], [/^Colorful Shading$/, 'Sombreado vistoso'],
  [/^Light List$/, 'Lista clara'], [/^Medium List (\d)$/, 'Lista media $1'], [/^Dark List$/, 'Lista oscura'], [/^Colorful List$/, 'Lista vistosa'],
  [/^Light Grid$/, 'Cuadrícula clara'], [/^Medium Grid (\d)$/, 'Cuadrícula media $1'], [/^Colorful Grid$/, 'Cuadrícula vistosa'],
  [/^Table Classic (\d)$/, 'Tabla clásica $1'], [/^Table Colorful (\d)$/, 'Tabla con colores $1'],
  [/^Table 3D effects (\d)$/, 'Tabla con efectos 3D $1'], [/^Table Grid (\d)$/, 'Tabla con cuadrícula $1'],
  [/^Table Web (\d)$/, 'Tabla web $1'], [/^Table Columns (\d)$/, 'Tabla con columnas $1'], [/^Table List (\d)$/, 'Tabla con lista $1'],
  [/^Table Simple (\d)$/, 'Tabla sencilla $1'], [/^Table Subtle (\d)$/, 'Tabla sutil $1'],
];

/** [site id, the TS regex (or [regex, replacement] for `replace`), operation]. */
export const SITES = [
  // parts.ts vmlLenPt (a regex built from the constants 'width' / 'height'): the number and the unit
  ['parts.vml_width_num', vml(W), 'group'],
  ['parts.vml_width_unit', vmlUnit(W), 'group'],
  ['parts.vml_height_num', vml(H), 'group'],
  ['parts.vml_height_unit', vmlUnit(H), 'group'],
  // parts.ts numberingImages: the extension of a target
  ['parts.target_ext', /\.([a-z0-9]+)$/i, 'group'],
  // parts.ts readTheme, readSettings, readFontTable: the prefix of a name
  ['parts.strip_a', [/^a:/, ''], 'replace'],
  ['parts.strip_w', [/^w:/, ''], 'replace'],
  // table-styles.ts
  ['table.local_id', [/[^A-Za-z0-9-]/g, ''], 'replace'],
  ['table.accent_sub', [/\{A\}/g, '7'], 'replace'],
  ['table.id_sub', ['{ID}', 'Q'], 'replace'],
  ['table.attr_themeColor', attr('w:themeColor'), 'group'],
  ['table.attr_themeFillTint', attr('w:themeFillTint'), 'group'],
  ['table.attr_themeFill', attr('w:themeFill'), 'group'],
  ['table.marker_color', [/w:color="~"([^/>]*)/g, '[$1]'], 'replace'],
  ['table.marker_fill', [/w:fill="~"([^/>]*)/g, '[$1]'], 'replace'],
  // style-names-es.ts tableStyleNameEs
  ['es.accent_base', /^(.*?)(?: Accent (\d))?$/, 'group'],
  ['es.accent_digit', /^(?:.*?)(?: Accent (\d))?$/, 'group'],
  ...FAMILY.map(([re, out], i) => [`es.family_${i}`, [re, out], 'replace']),
];

/** Inputs every row runs on (the reader's strings come from XML: no lone surrogates). */
export const INPUTS = [
  '', ' ', 'width:10pt;height:5in', ' ; width : 3', 'WIDTH:3', 'width:.5cm;height:2mm', 'width:1.2.3mm', 'width:abc', 'width:1e3px', ';width:4pc;;height:6', 'height:7 in',
  'width:8;width:9pt', ' width:5px', 'x;width:5pt', 'max-width:5pt', 'width:5 ptx', 'width:5\tpt', 'width:\n5pt', 'height:5 \u00a0pt', ';height:2;width:3in', 'width:5 mm;',
  'width:5Pt', 'width: .', 'width:..', 'width :5pt', ';;;width:1', 'a;b;width:2cm', 'width:5pcpt', 'width:5 pi', ';  \t width:  7  px ; height:8', 'width:5;', 'width:', 'width:  ',
  'height:3pt;width:4pt', '\u00a0width:5', 'a width:5', '; height : 9.5.5 cm', 'width:0', 'width:5 \u2003 in', 'width:5\u2028pt', 'width:١٢pt',
  'media/a.png', 'a.PNG', 'a.tar.gz', 'a.', 'a', '.png', '..png', 'x.p-g', 'a.é', 'a.png ', 'a.pN9', 'dir.v1/file', 'dir.v1/file.JPEG', 'a.png\n', '../media/b.JPEG', 'a.ſ', 'a.K',
  'a:b', 'a:', 'a:a:b', ':a', 'w:a', 'w:w:b', 'W:a', 'a:\n', 'xa:b',
  'Tabela com grade', 'Grade Clara - Ênfase 1', 'a-b_c.d e', '{A}', '{A}{A}x', '{ID}x{ID}', '{ID', 'çÇãõ', '😀x', '---', 'a_b', 'é',
  '<w:top w:val="single" w:color="~" w:themeColor="accent1" w:themeShade="BF"/>',
  '<w:shd w:val="clear" w:color="auto" w:fill="~" w:themeFill="accent1" w:themeFillTint="33"/>',
  '<a w:color="~"/><b w:color="~" w:themeColor="dk1"><c w:color="~"w:themeColor="lt1" w:themeTint="80"/>',
  'w:color="~"', 'w:color="~">', 'w:color="~"x', 'w:color="~" w:themeColor="" w:themeTint=""/>', 'w:fill="~"w:fill="~"', 'w:color="~" w:fill="~" w:themeFill="x"/>',
  'w:themeColor="a" w:themeColor="b"', 'xw:themeColor="a"', 'w:themeColor="unterminated', 'w:themeColor="" ', 'w:themeFillTint="9" w:themeFill="8"', 'w:themeFill="8" w:themeFillTint="9"', 'w:themeFill=""',
  'Normal Table', 'Table Grid', 'Table Grid Accent 1', 'Grid Table 4', 'Grid Table 4 Accent 1', 'Grid Table 4 Accent 9', 'Grid Table 4 Accent 10', 'Grid Table 4 Accent ', 'Grid Table 4  Accent 1',
  'Grid Table 4 accent 1', 'Grid Table 4 Accent 1 ', 'Grid Table 44', 'Grid Table 4 Light', 'Grid Table 4 Light Accent 2', 'Grid Table 7 Colorful', 'Grid Table 7 Dark', 'List Table 1', 'List Table 2 Light',
  'List Table 3 Dark', 'List Table 4 Colorful', 'Plain Table 5', 'Light Shading', 'Medium Shading 1', 'Medium Shading', 'Dark Shading', 'Colorful Shading', 'Light List', 'Medium List 2', 'Dark List', 'Colorful List',
  'Light Grid', 'Medium Grid 3', 'Colorful Grid', 'Table Classic 3', 'Table Colorful 1', 'Table 3D effects 2', 'Table 3D effects 22', 'Table Grid 5', 'Table Web 3', 'Table Columns 5', 'Table List 8', 'Table Simple 2',
  'Table Subtle 1', 'Table\nGrid', 'Table Grid\r', 'Table Grid Accent 1\n', 'Table\u2028Grid Accent 1', 'Grid Table \u0663', 'Grid Table 4 Accent \u0663', ' Accent 1', 'Accent 1', 'é Accent 1', 'Table Grid Accent 12',
  'Table Grid Accent 1Accent 2', 'Table Grid Accent 1 Accent 2', 'Grid Table 4 Light Accent 1', 'Table Subtle 1 Accent 3', 'Grid Table 4😀', 'Light Grid Accent 5', 'List Table 4 Colorful Accent 6',
];
