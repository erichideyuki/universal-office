// The regex sites of frontier R4b (document.ts inline objects, picture-xml.ts) against V8, checked
// by tests/regex_r4b.rs. A row is `[id, regex, op]` (see regex-diff.mjs).

/** [site id, the TS regex (or [regex, replacement] for `replace`), operation]. */
export const SITES = [
  // document.ts readDrawingRun: `/\/drawingml\/2006\/diagram$/.test(gd?.attrs['uri'] ?? '')`
  ['r1.diagram_uri', /\/drawingml\/2006\/diagram$/, 'test'],
  // document.ts readDrawingRun docPr hidden and dmlParagraphs spcFirstLastPara: `/^(1|true)$/`
  ['r2.one_of', /^(1|true)$/, 'test'],
  // document.ts readVmlRuns (WordArt): `/bold|[6-9]00/.test(font-weight)`, `/italic/.test(font-style)`
  ['r3.bold', /bold|[6-9]00/, 'test'],
  ['r3.italic', /italic/, 'test'],
  // document.ts readVmlRuns: `.replace(/^["']|["']$/g, '')`, `.replace(/^#/, '')`, `.replace(/\D/g, '')`, `.replace(/^v:/, '')`
  ['r4.quotes', [/^["']|["']$/g, ''], 'replace'],
  ['r4.hash', [/^#/, ''], 'replace'],
  ['r4.digits', [/\D/g, ''], 'replace'],
  ['r4.vtag', [/^v:/, ''], 'replace'],
  // document.ts syncShapeFromXml: `/<a:blipFill\b/.test(raw)`
  ['r5.blipfill', /<a:blipFill\b/, 'index'],
  // picture-xml.ts readPictureFormat: `/^val\s+(-?\d+)/.exec(gd.attrs['fmla'] ?? '')`
  ['r6.val_int', /^val\s+(-?\d+)/, 'group'],
];

export const INPUTS = [
  '', ' ', '1', 'true', 'TRUE', ' 1', '1 ', '1\n',
  'http://schemas.openxmlformats.org/drawingml/2006/diagram', 'http://schemas.openxmlformats.org/drawingml/2006/diagram/', 'x/drawingml/2006/diagram\n',
  '/drawingml/2006/diagram', 'drawingml/2006/diagram', 'http://schemas.openxmlformats.org/drawingml/2006/picture',
  'bold', 'Bold', 'bolder', '600', '700', '900', '500', '1600', '100', '6000', 'x700y', '60', '0600', 'normal', 'italic', 'Italic', 'oblique', 'bold italic',
  '"Arial"', "'Times'", '"', "'", '""', "'\"", '"a', 'a"', 'a"b', '  "x"  ', '"Calibri', "Calibri'", '', '﻿"x"',
  '#_x0000_s1027', '#', '##a', 'x#', ' #a', '_x0000_s1', '#é',
  '_x0000_s1026', '99999999999999999999', 'a1b2c3', '１２', '12 34', '-5', '1e3', '\n1\n', 'é1😀2',
  'v:shape', 'v:rect', 'v:', 'v', 'V:shape', 'x:v:shape', ' v:shape', 'o:spid',
  '<a:blipFill', '<a:blipFill>', '<a:blipFill r:embed="x"/>', '<a:blipFillx/><a:blipFill r:embed="x"/>', '<a:blipFill_/>', '<a:blipFill', '<dml:blipFill>', 'x <a:blipFill\n',
  'val 12', 'val -3x', 'val-3', 'val  ', 'val\t+5', 'VAL 1', 'val 1x', 'val -', ' val 1', 'val 12345678901234567890', 'val\u00a05', 'val\u30005', '*/ 1 2 3',
];
