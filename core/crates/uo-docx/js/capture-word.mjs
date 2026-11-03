// Unit capture of uo-docx-layout frontier F2, the Word semantics in `uo_docx::word` (record
// core/docs/modules/uo-docx-layout.md, "Wave 1 — F2"): seeded generated inputs through the frozen
// TS functions of docx-layout (`resolve.ts`, `context.ts` `directRPr`/`directPPr`, `para.ts`
// `resolvePPr`, `paraformat.ts`, `numbering.ts`, `datefmt.ts`, `field-formula.ts`,
// `revisions.ts`, `outline.ts` `headingText`/`outlineLevelOf`) and docx-core `table-formula.ts`
// (`formatPicture`, `formatFormulaResult`).
//
//   UO_TS_ROOT=<built checkout of main> node core/crates/uo-docx/js/capture-word.mjs [--seed S] [--count N] [--fmt-count N] [--tz <IANA zone>] [--out <path without extension>]
//
// UO_TS_ROOT must be a checkout whose dist/ is built from the frozen TS of this branch (the
// `write/` tree of rust-core; check with `git diff --stat <commit> -- write/packages` and the
// dist mtimes).
//
// Inputs are OOXML (`w:rPr`, `w:pPr`, `w:tblPr`, `w:trPr`, `w:tcPr`, styles.xml, numbering.xml,
// theme XML) read on both sides by the D1a readers (R1, R2; their own parity is theirs: the
// facet `in` is the canonical digest of what was read, so a reader difference is told apart from
// a Word-semantics one), plain strings and numbers, and small edit scripts over the editing props.
// Outputs are canonical digests (A6, `canon.mjs`) of the values (`out`) and of their key-order
// facet (`ord`, K9/LQ20), or the JSON text of strings (`s`).
//
// Files: tests/parity/`word.capture` (records `<suite>/<n>`) and `word.inputs`
// (`<key>\t<field>…`, every string field as `x` + the hex of its UTF-16 units, `-` = undefined).
// With `--tz` (the São Paulo facet of LQ10) only the date suites run, with the instants' local
// offsets taken from the process time zone, into uo-docx-layout's
// tests/captures/f2/`datefmt-<zone>.capture`/`.inputs` plus the zone's transition table (meta
// `transitions`), replayed there through the layout's `Clock` and `TimeZoneRule`.
const argv = process.argv.slice(2);
const opt = (n, d) => { const i = argv.indexOf(n); return i >= 0 ? argv[i + 1] : d; };
const TZ = opt('--tz', 'UTC');
process.env.TZ = TZ;

const { writeFileSync } = await import('node:fs');
const { join, dirname } = await import('node:path');
const { fileURLToPath } = await import('node:url');
const { Stream, CaptureWriter, importFrom, tsRoot } = await import('../../uo-testkit/js/testkit.mjs');
const { Canon } = await import('./canon.mjs');

const here = dirname(fileURLToPath(import.meta.url));
const root = tsRoot();
const L = (f) => importFrom(root, `write/packages/docx-layout/dist/${f}`);
const C = (f) => importFrom(root, `write/packages/docx-core/dist/${f}`);
const { parseXml } = await C('xml.js');
const { readRPr, readPPr, readTblPr, readTrPr, readTcPr } = await C('ooxml/props.js');
const { readStyleSheet, readNumbering, readTheme, readSettings } = await C('ooxml/parts.js');
const { runPropsFromRPr, paraPropsFromPPr, DEFAULT_RUN_PROPS } = await C('document.js');
const { formatPicture, formatFormulaResult } = await C('table-formula.js');
const R = await L('resolve.js');
const { NumberingState, formatNumber } = await L('numbering.js');
const { formatWordDate, dateFieldValue } = await L('datefmt.js');
const { evaluateFieldFormula } = await L('field-formula.js');
const { DocContext, directRPr, directPPr } = await L('context.js');
const { resolvePPr } = await L('para.js');
const { inheritedPPr, effectivePPr } = await L('paraformat.js');
const { RevisionDisplay, revisionRanges, joinDeleted } = await L('revisions.js');
const { headingText, outlineLevelOf } = await L('outline.js');

const seed = Number(opt('--seed', '7'));
const count = Number(opt('--count', '300'));
const fmtCount = Number(opt('--fmt-count', '40'));
let st = seed >>> 0 || 1;
const rnd = () => { st = (st + 0x6d2b79f5) >>> 0; let t = st; t = Math.imul(t ^ (t >>> 15), t | 1); t ^= t + Math.imul(t ^ (t >>> 7), t | 61); return ((t ^ (t >>> 14)) >>> 0) / 4294967296; };
const int = (n) => Math.floor(rnd() * n);
const pick = (a) => a[int(a.length)];
const chance = (p) => rnd() < p;
const shuffle = (a) => { for (let i = a.length - 1; i > 0; i--) { const j = int(i + 1); [a[i], a[j]] = [a[j], a[i]]; } return a; };

const W = 'xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main" xmlns:w14="http://schemas.microsoft.com/office/word/2010/wordml" xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main"';
const attr = (k, v) => (v === undefined ? '' : ` ${k}="${v}"`);
const el = (name, attrs = {}, inner = '') => `<${name}${Object.entries(attrs).map(([k, v]) => attr(k, v)).join('')}${inner ? `>${inner}</${name}>` : '/>'}`;

// ───────────── generated OOXML ─────────────
const HEX = ['FF0000', '00ff00', '156082', 'auto', '', 'zz0000', 'ABC', '0563C1', '000000', 'FFFFFF'];
const THEME_COLORS = ['accent1', 'accent2', 'text1', 'background1', 'hyperlink', 'dk2', 'tx1', 'bogus', ''];
const TINTS = [undefined, '99', '33', 'BF', '80', 'zz', '', 'FF', '00'];
const ONOFF = [undefined, '0', '1', 'false', 'true', 'off'];
const NUMS = ['0', '1', '2', '12', '24', '-4', '20', '80', '100', 'x', '1e3', '-0', '240', '720', '360', '1440'];
const TOGGLE_NAMES = ['b', 'bCs', 'i', 'iCs', 'caps', 'smallCaps', 'strike', 'dstrike', 'outline', 'shadow', 'emboss', 'imprint', 'vanish'];
const CHAR_IDS = ['Char1', 'Char2', 'Heading1', 'Normal', '10', 'Missing'];
const FONTS = ['Arial', 'Calibri', 'Aptos', 'Times New Roman', ''];
const FONT_THEMES = ['minorHAnsi', 'majorHAnsi', 'majorEastAsia', 'minorBidi', 'majorAscii', 'minorEastAsia', ''];
const LANGS = ['pt-BR', 'en-US', 'es-ES', 'de-DE', 'x'];

function rFontsXml() {
  const keys = shuffle(['w:ascii', 'w:hAnsi', 'w:eastAsia', 'w:cs', 'w:asciiTheme', 'w:hAnsiTheme', 'w:eastAsiaTheme', 'w:cstheme', 'w:hint']).slice(0, 1 + int(4));
  const a = {};
  for (const k of keys) a[k] = k.endsWith('heme') || k === 'w:cstheme' ? pick(FONT_THEMES) : k === 'w:hint' ? pick(['eastAsia', 'cs', 'default']) : pick(FONTS);
  return el('w:rFonts', a);
}
const colorAttrs = () => ({ 'w:val': chance(0.8) ? pick(HEX) : undefined, 'w:themeColor': chance(0.4) ? pick(THEME_COLORS) : undefined, 'w:themeTint': chance(0.3) ? pick(TINTS) : undefined, 'w:themeShade': chance(0.3) ? pick(TINTS) : undefined });
const shdXml = () => el('w:shd', {
  'w:val': chance(0.9) ? pick(['clear', 'solid', 'nil', 'pct25', 'pct5', 'pct', 'horzStripe', 'diagCross', 'thinDiagCross', 'pct50', 'pct150', 'bogus']) : undefined,
  'w:color': chance(0.6) ? pick(HEX) : undefined,
  'w:fill': chance(0.7) ? pick(HEX) : undefined,
  'w:themeFill': chance(0.2) ? pick(THEME_COLORS) : undefined,
  'w:themeFillTint': chance(0.2) ? pick(TINTS) : undefined,
  'w:themeColor': chance(0.15) ? pick(THEME_COLORS) : undefined,
});
const borderXml = (name) => el(name, { 'w:val': pick(['single', 'double', 'nil', 'none', 'dashed']), 'w:sz': chance(0.8) ? pick(['4', '8', '12', 'x']) : undefined, 'w:space': chance(0.6) ? pick(['0', '1', '4']) : undefined, 'w:color': chance(0.6) ? pick(HEX) : undefined, 'w:themeColor': chance(0.2) ? pick(THEME_COLORS) : undefined });

function rPrInner(depth = 0) {
  const parts = [];
  const n = int(7);
  for (let i = 0; i < n; i++) {
    switch (int(19)) {
      case 0: parts.push(el('w:rStyle', { 'w:val': pick(CHAR_IDS) })); break;
      case 1: parts.push(rFontsXml()); break;
      case 2: case 3: case 4: parts.push(el(`w:${pick(TOGGLE_NAMES)}`, { 'w:val': pick(ONOFF) })); break;
      case 5: parts.push(el('w:color', colorAttrs())); break;
      case 6: parts.push(el(`w:${pick(['sz', 'szCs', 'kern', 'position', 'spacing', 'w'])}`, { 'w:val': pick(NUMS) })); break;
      case 7: parts.push(el('w:highlight', { 'w:val': pick(['yellow', 'darkBlue', 'none', 'bogus', 'lightGray', 'constructor']) })); break;
      case 8: parts.push(el('w:u', { 'w:val': pick(['single', 'double', 'none', 'wave', '']), ...(chance(0.4) ? { 'w:color': pick(HEX) } : {}), ...(chance(0.2) ? { 'w:themeColor': pick(THEME_COLORS) } : {}) })); break;
      case 9: parts.push(shdXml()); break;
      case 10: parts.push(el('w:vertAlign', { 'w:val': pick(['superscript', 'subscript', 'baseline']) })); break;
      case 11: parts.push(el('w:lang', { 'w:val': chance(0.8) ? pick(LANGS) : undefined, 'w:eastAsia': chance(0.3) ? pick(['ja-JP', 'zh-TW', 'ko-KR']) : undefined })); break;
      case 12: parts.push(el('w14:ligatures', { 'w14:val': pick(['standard', 'none', 'all']) })); break;
      case 13: parts.push(el('w14:numForm', { 'w14:val': pick(['lining', 'oldStyle']) })); break;
      case 14: parts.push(pick([
        '<w14:textFill><w14:solidFill><w14:srgbClr w14:val="FF0000"><w14:alpha w14:val="40000"/></w14:srgbClr></w14:solidFill></w14:textFill>',
        '<w14:textFill><w14:noFill/></w14:textFill>',
        '<w14:textFill><w14:solidFill><w14:schemeClr w14:val="accent1"/></w14:solidFill></w14:textFill>',
      ])); break;
      case 15: parts.push(pick([
        '<w14:shadow w14:blurRad="38100" w14:dist="19050" w14:dir="2700000" w14:sx="100000" w14:sy="100000" w14:kx="0" w14:ky="0" w14:algn="tl"><w14:srgbClr w14:val="000000"/></w14:shadow>',
        '<w14:textOutline w14:w="9525"><w14:solidFill><w14:srgbClr w14:val="00FF00"/></w14:solidFill></w14:textOutline>',
        '<w14:textOutline w14:w="9525"><w14:noFill/></w14:textOutline>',
      ])); break;
      case 16: if (depth === 0 && chance(0.5)) parts.push(`<w:rPrChange w:author="${pick(['Ana', 'Bia'])}" w:date="${pick(['d1', 'd2'])}"><w:rPr>${rPrInner(1)}</w:rPr></w:rPrChange>`); break;
      case 17: parts.push(el('w:em', { 'w:val': 'dot' })); break;
      default: parts.push(el('w:noProof')); break;
    }
  }
  return parts.join('');
}
const rPrXml = () => `<w:rPr ${W}>${rPrInner()}</w:rPr>`;

function pPrInner(depth = 0) {
  const parts = [];
  const n = int(7);
  for (let i = 0; i < n; i++) {
    switch (int(18)) {
      case 0: parts.push(el('w:pStyle', { 'w:val': pick(['Heading1', 'Heading2', 'Normal', 'ListPara', '2', '10', 'Missing', 'Char1']) })); break;
      case 1: parts.push(el(`w:${pick(['keepNext', 'keepLines', 'pageBreakBefore', 'widowControl', 'suppressLineNumbers', 'suppressAutoHyphens', 'contextualSpacing'])}`, { 'w:val': pick(ONOFF) })); break;
      case 2: parts.push(el('w:framePr', { 'w:w': pick(['2000', undefined]), 'w:wrap': pick(['around', undefined]), 'w:dropCap': pick(['drop', undefined]) })); break;
      case 3: parts.push(`<w:numPr>${chance(0.8) ? el('w:ilvl', { 'w:val': pick(['0', '1', '2', '8', '9', 'x', '-1']) }) : ''}${chance(0.9) ? el('w:numId', { 'w:val': pick(['0', '1', '2', '3', '5', '7', 'x']) }) : ''}</w:numPr>`); break;
      case 4: parts.push(`<w:pBdr>${shuffle(['top', 'left', 'bottom', 'right', 'between', 'bar']).slice(0, 1 + int(3)).map((s) => borderXml(`w:${s}`)).join('')}</w:pBdr>`); break;
      case 5: parts.push(shdXml()); break;
      case 6: parts.push(`<w:tabs>${Array.from({ length: 1 + int(4) }, () => el('w:tab', { 'w:val': pick(['left', 'center', 'right', 'decimal', 'clear', 'bar']), 'w:pos': pick(['720', '1440', '2160', '720', 'x', '-100', '0']), 'w:leader': chance(0.3) ? pick(['dot', 'hyphen']) : undefined })).join('')}</w:tabs>`); break;
      case 7: parts.push(el('w:spacing', { 'w:before': chance(0.5) ? pick(NUMS) : undefined, 'w:after': chance(0.5) ? pick(NUMS) : undefined, 'w:line': chance(0.5) ? pick(['240', '360', '276', 'x']) : undefined, 'w:lineRule': chance(0.4) ? pick(['auto', 'exact', 'atLeast']) : undefined, 'w:beforeAutospacing': chance(0.2) ? pick(ONOFF) : undefined, 'w:afterAutospacing': chance(0.2) ? pick(ONOFF) : undefined })); break;
      case 8: parts.push(el('w:ind', Object.fromEntries(shuffle(['w:left', 'w:right', 'w:firstLine', 'w:hanging', 'w:start', 'w:end', 'w:firstLineChars', 'w:hangingChars']).slice(0, 1 + int(3)).map((k) => [k, pick(NUMS)])))); break;
      case 9: parts.push(el('w:jc', { 'w:val': pick(['left', 'center', 'right', 'both', 'start', 'end', 'distribute', 'bogus']) })); break;
      case 10: parts.push(el('w:outlineLvl', { 'w:val': pick(['0', '1', '8', '9', 'x']) })); break;
      case 11: parts.push(`<w:rPr>${rPrInner(1)}</w:rPr>`); break;
      case 12: if (depth === 0 && chance(0.4)) parts.push(`<w:pPrChange w:author="${pick(['Ana', 'Bia'])}" w:date="d1"><w:pPr>${pPrInner(1)}</w:pPr></w:pPrChange>`); break;
      case 13: parts.push(el('w:textboxTightWrap', { 'w:val': pick(['allLines', 'none']) })); break;
      default: parts.push(el('w:snapToGrid', { 'w:val': '0' })); break;
    }
  }
  return parts.join('');
}
const pPrXml = () => `<w:pPr ${W}>${pPrInner()}</w:pPr>`;

const widthXml = (name) => el(name, { 'w:w': pick(['0', '100', '5000', 'x']), 'w:type': pick(['dxa', 'pct', 'auto', 'nil']) });
function tblPrXml() {
  const p = [];
  for (let i = 0, n = int(6); i < n; i++) {
    switch (int(8)) {
      case 0: p.push(el('w:tblStyle', { 'w:val': pick(['TableGrid', 'TblB', 'Missing']) })); break;
      case 1: p.push(widthXml(pick(['w:tblW', 'w:tblInd']))); break;
      case 2: p.push(el('w:jc', { 'w:val': pick(['center', 'left']) })); break;
      case 3: p.push(`<w:tblBorders>${shuffle(['top', 'left', 'bottom', 'right', 'insideH', 'insideV']).slice(0, 1 + int(3)).map((s) => borderXml(`w:${s}`)).join('')}</w:tblBorders>`); break;
      case 4: p.push(shdXml()); break;
      case 5: p.push(`<w:tblCellMar>${shuffle(['top', 'left', 'bottom', 'right']).slice(0, 1 + int(3)).map((s) => widthXml(`w:${s}`)).join('')}</w:tblCellMar>`); break;
      case 6: p.push(el('w:tblLook', { 'w:firstRow': pick(ONOFF), 'w:noHBand': pick(ONOFF), 'w:lastColumn': pick(ONOFF) })); break;
      default: p.push(el('w:tblLayout', { 'w:type': 'fixed' })); break;
    }
  }
  return `<w:tblPr ${W}>${p.join('')}</w:tblPr>`;
}
function trPrXml() {
  const p = [];
  for (let i = 0, n = int(4); i < n; i++) {
    p.push(pick([el('w:cantSplit', { 'w:val': pick(ONOFF) }), el('w:trHeight', { 'w:val': pick(NUMS), 'w:hRule': pick(['exact', 'atLeast', undefined]) }), el('w:tblHeader'), el('w:jc', { 'w:val': 'center' }), el('w:gridBefore', { 'w:val': pick(['1', '2']) })]));
  }
  return `<w:trPr ${W}>${p.join('')}</w:trPr>`;
}
function tcPrXml() {
  const p = [];
  for (let i = 0, n = int(6); i < n; i++) {
    switch (int(7)) {
      case 0: p.push(widthXml('w:tcW')); break;
      case 1: p.push(el('w:gridSpan', { 'w:val': pick(['1', '2', 'x']) })); break;
      case 2: p.push(el('w:vMerge', { 'w:val': pick(['restart', undefined]) })); break;
      case 3: p.push(`<w:tcBorders>${shuffle(['top', 'left', 'bottom', 'right', 'insideH', 'tl2br']).slice(0, 1 + int(3)).map((s) => borderXml(`w:${s}`)).join('')}</w:tcBorders>`); break;
      case 4: p.push(shdXml()); break;
      case 5: p.push(`<w:tcMar>${shuffle(['top', 'left', 'bottom', 'right']).slice(0, 1 + int(3)).map((s) => widthXml(`w:${s}`)).join('')}</w:tcMar>`); break;
      default: p.push(el('w:vAlign', { 'w:val': pick(['center', 'bottom']) })); break;
    }
  }
  return `<w:tcPr ${W}>${p.join('')}</w:tcPr>`;
}

const STYLE_IDS = ['Normal', 'Heading1', 'Heading2', 'Title', 'ListPara', '2', '10', '1', 'Char1', 'Char2', 'TableGrid', 'TblB', 'NumSt', 'constructor'];
function stylesXml() {
  const out = [];
  if (chance(0.8)) out.push(`<w:docDefaults>${chance(0.8) ? `<w:rPrDefault><w:rPr>${rPrInner(1)}</w:rPr></w:rPrDefault>` : ''}${chance(0.8) ? `<w:pPrDefault><w:pPr>${pPrInner(1)}</w:pPr></w:pPrDefault>` : ''}</w:docDefaults>`);
  for (const id of shuffle([...STYLE_IDS])) {
    if (chance(0.15)) continue;
    const type = id.startsWith('Char') ? pick(['character', 'character', 'paragraph']) : id.startsWith('T') && id !== 'Title' ? 'table' : id === 'NumSt' ? 'numbering' : pick(['paragraph', 'paragraph', 'paragraph', 'character']);
    const name = id === 'Heading1' ? pick(['heading 1', 'Heading 1', 'HEADING 1']) : id === 'Heading2' ? 'heading 2' : id === 'Title' ? pick(['Title', 'heading 9']) : id === '2' ? 'heading 3' : id;
    let inner = el('w:name', { 'w:val': name });
    if (chance(0.6)) inner += el('w:basedOn', { 'w:val': pick(STYLE_IDS) });
    if (chance(0.7)) inner += `<w:pPr>${pPrInner(1)}</w:pPr>`;
    if (chance(0.7)) inner += `<w:rPr>${rPrInner(1)}</w:rPr>`;
    if (type === 'table') {
      if (chance(0.7)) inner += tblPrXml().replace(` ${W}`, '');
      if (chance(0.4)) inner += trPrXml().replace(` ${W}`, '');
      if (chance(0.5)) inner += tcPrXml().replace(` ${W}`, '');
      for (const t of shuffle(['firstRow', 'lastRow', 'band1Horz', 'wholeTable', 'nwCell', '2', '__proto__']).slice(0, int(4))) {
        inner += `<w:tblStylePr w:type="${t}">${chance(0.6) ? `<w:pPr>${pPrInner(1)}</w:pPr>` : ''}${chance(0.6) ? `<w:rPr>${rPrInner(1)}</w:rPr>` : ''}${chance(0.4) ? tcPrXml().replace(` ${W}`, '') : ''}</w:tblStylePr>`;
      }
    }
    out.push(`<w:style w:type="${type}" w:styleId="${id}"${chance(0.2) ? ' w:default="1"' : ''}>${inner}</w:style>`);
  }
  return `<w:styles ${W}>${out.join('')}</w:styles>`;
}

const LVL_TEXTS = ['%1.', '%1.%2.', '(%3)', 'Capítulo %1', '•', '', '%10', '%%1', 'x%9y', '😀%1', '%1.%2.%3', '%2)', 'Art. %1º'];
const NUM_FMTS = ['decimal', 'upperRoman', 'lowerRoman', 'upperLetter', 'lowerLetter', 'bullet', 'chicago', 'cardinalText', 'ordinal', 'ordinalText', 'hex', 'decimalZero', 'none', 'decimalEnclosedCircle', 'numberInDash', 'bogus'];
function lvlXml(ilvl) {
  let inner = '';
  if (chance(0.85)) inner += el('w:start', { 'w:val': pick(['0', '1', '1', '5', '-2', 'x', '3999', '27']) });
  if (chance(0.9)) inner += el('w:numFmt', { 'w:val': pick(NUM_FMTS) });
  if (chance(0.25)) inner += el('w:lvlRestart', { 'w:val': pick(['0', '1', '2', '5']) });
  if (chance(0.15)) inner += el('w:isLgl');
  if (chance(0.9)) inner += chance(0.05) ? '<w:lvlText w:null="1"/>' : el('w:lvlText', { 'w:val': pick(LVL_TEXTS) });
  if (chance(0.4)) inner += `<w:pPr>${pPrInner(1)}</w:pPr>`;
  return `<w:lvl w:ilvl="${ilvl}">${inner}</w:lvl>`;
}
function numberingXml() {
  const out = [];
  for (const a of shuffle(['1', '2', '3', '10']).slice(0, 1 + int(4))) {
    let inner = chance(0.15) ? el('w:numStyleLink', { 'w:val': 'NumSt' }) : '';
    for (let l = 0; l < 9; l++) if (chance(0.85)) inner += lvlXml(chance(0.05) ? 'x' : String(l));
    out.push(`<w:abstractNum w:abstractNumId="${a}">${inner}</w:abstractNum>`);
  }
  for (const n of shuffle(['1', '2', '3', '5', '7']).slice(0, 2 + int(4))) {
    let inner = el('w:abstractNumId', { 'w:val': pick(['1', '2', '3', '10', '99']) });
    for (let l = 0; l < 3; l++) {
      if (!chance(0.3)) continue;
      inner += `<w:lvlOverride w:ilvl="${pick(['0', '1', '2', '3'])}">${chance(0.7) ? el('w:startOverride', { 'w:val': pick(['1', '3', '0', 'x']) }) : ''}${chance(0.3) ? lvlXml(String(l)) : ''}</w:lvlOverride>`;
    }
    out.push(`<w:num w:numId="${n}">${inner}</w:num>`);
  }
  return `<w:numbering ${W}>${out.join('')}</w:numbering>`;
}
function themeXml() {
  const clr = ['dk1', 'lt1', 'dk2', 'lt2', 'accent1', 'accent2', 'hlink', 'folHlink'].map((k) => {
    if (k === 'dk1' && chance(0.5)) return '<a:dk1><a:sysClr val="windowText" lastClr="000000"/></a:dk1>';
    if (k === 'lt1' && chance(0.5)) return '<a:lt1><a:sysClr val="window"/></a:lt1>';
    return `<a:${k}><a:srgbClr val="${pick(['156082', 'e97132', '0563c1', 'zz12', '1F1F1F', 'FFFFFF'])}"/></a:${k}>`;
  }).join('');
  const font = (tag) => `<a:${tag}><a:latin typeface="${pick(['Aptos', 'Calibri', 'Aptos Display'])}"/><a:ea typeface="${pick(['', 'MS Mincho'])}"/><a:cs typeface="${pick(['', 'Arial'])}"/></a:${tag}>`;
  return `<a:theme ${W}><a:themeElements><a:clrScheme name="x">${clr}</a:clrScheme><a:fontScheme name="f">${font('majorFont')}${font('minorFont')}</a:fontScheme></a:themeElements></a:theme>`;
}

// ───────────── helpers ─────────────
const first = (xml) => parseXml(xml);
const rpr = (xml) => (xml === undefined ? undefined : readRPr(first(xml)));
const ppr = (xml) => (xml === undefined ? undefined : readPPr(first(xml)));
const tblpr = (xml) => (xml === undefined ? undefined : readTblPr(first(xml)));
const trpr = (xml) => (xml === undefined ? undefined : readTrPr(first(xml)));
const tcpr = (xml) => (xml === undefined ? undefined : readTcPr(first(xml)));
const formatting = (f) => ({ styles: readStyleSheet(f.styles), numbering: readNumbering(f.numbering), theme: readTheme(f.theme), settings: readSettings(undefined), fontTable: {} });

function digests(v) {
  const s = new Stream(), o = new Stream();
  new Canon(s, { order: o }).value(v, 'x');
  return [`${s.items}:${s.finish().short}`, `${o.items}:${o.finish().short}`];
}
// a string field: `s` + the string with `%`, tab, CR, LF and lone surrogates escaped (`%25`,
// `%09`, `%0D`, `%0A`, `%uDXXX`); `-` = undefined
const enc = (x) => {
  if (x === undefined) return '-';
  let out = 's';
  for (let i = 0; i < x.length; i++) {
    const c = x.charCodeAt(i);
    if (c === 0x25 || c === 9 || c === 10 || c === 13) out += '%' + c.toString(16).padStart(2, '0').toUpperCase();
    else if (c >= 0xd800 && c <= 0xdbff && i + 1 < x.length && x.charCodeAt(i + 1) >= 0xdc00 && x.charCodeAt(i + 1) <= 0xdfff) { out += x[i] + x[i + 1]; i++; }
    else if (c >= 0xd800 && c <= 0xdfff) out += '%u' + c.toString(16).toUpperCase();
    else out += x[i];
  }
  return out;
};
const numEnc = (n) => (n === undefined ? '-' : n === null ? 'null' : Object.is(n, -0) ? '-0' : String(n));

const cap = new CaptureWriter([
  'uo-docx-layout F2 unit capture (core/crates/uo-docx/js/capture-word.mjs; replay core/crates/uo-docx/tests/word_unit.rs).',
  'in = canon digest of the inputs as read; out/ord = canon digest and key-order digest of the result; s = JSON of a string result.',
]);
cap.meta('seed', seed);
cap.meta('tz', TZ);
const inputs = ['# key<TAB>fields (s+escaped string | - = undefined | numbers as String(n))'];
const put = (key, fields) => inputs.push([key, ...fields].join('\t'));
const record = (key, facet, v) => cap.set(key, facet, v);
const recordValue = (key, v, facet = 'out') => { const [d, o] = digests(v); record(key, facet, d); record(key, `${facet}.ord`, o); };
const recordStr = (key, s, facet = 's') => record(key, facet, s === undefined ? 'undefined' : s === null ? 'null' : JSON.stringify(s));
const guard = (key, f) => { try { f(); } catch (e) { if (process.env.UO_WORD_DEBUG) console.error(key, e); record(key, 'throw', JSON.stringify(String(e?.name ?? e))); } };

// ───────────── date and formula suites (also the São Paulo facet) ─────────────
const PICTURES = ["dd/MM/yyyy", "d 'de' MMMM 'de' yyyy", 'dddd, d MMMM yyyy', 'ddd dd MMM yy', 'HH:mm:ss', 'h:mm am/pm', 'h:mm AM/PM', 'hh:mm', "'quoted", 'yyyy-MM-ddTHH:mm', 'M/d/y', 'DDDD YYYY', 'Am/Pm am/PM', 'SS ss s', 'mmmm', '😀 d', 'x\uD800y'];
const INSTRS = [' DATE ', ' TIME ', ' DATE \\@ "dd/MM/yyyy" ', ' DATE \\@ dddd ', ' TIME \\@ "HH:mm" \\* MERGEFORMAT ', ' DATE \\@ "MMMM" \\* Upper ', ' DATE \\@ "dddd d" \\* Caps ', ' DATE \\@ "MMMM d" \\* FirstCap ', ' date \\@ "MMMM" \\* lower ', ' TIME \\* Caps', ' DATE \\@ "unclosed ', ' DATE \\@', ' CREATEDATE \\@ "yyyy" ', '  DATE  \\@  "a  b"  \\*  caps ', ' DATE \\@ "ß x" \\* upper ', ' DATE \\* FIRSTCAP \\@ "dddd"'];
const INSTANTS = () => [Date.UTC(2026, 0, 15, 12, 0, 0), Date.UTC(1999, 11, 31, 23, 59, 59), Date.UTC(2000, 1, 29, 3, 7, 9), Date.UTC(1985, 9, 20, 2, 30), Date.UTC(2018, 10, 4, 2, 59, 59), Date.UTC(2018, 10, 4, 3, 0, 0), Date.UTC(2019, 1, 17, 1, 59, 59), Date.UTC(2019, 1, 17, 2, 0, 0), -1, 0, Date.UTC(1969, 6, 20, 20, 17), Date.UTC(275760, 8, 13), NaN, Date.UTC(1, 0, 1), Date.UTC(-500, 5, 5), Date.UTC(1932, 9, 3, 3), Math.floor(rnd() * 4e12) - 1e12];

function dateSuites(prefix) {
  let k = 0;
  // outside UTC only from 1914 (the zone's local mean time before has sub-minute offsets, below the
  // resolution of the layout's TimeZoneRule)
  for (const t of INSTANTS().filter((x) => TZ === 'UTC' || Number.isNaN(x) || x >= Date.UTC(1914, 1, 1))) {
    const d = new Date(t);
    const off = Number.isNaN(t) ? 'NaN' : String(-d.getTimezoneOffset());
    for (const lang of ['pt-BR', 'en-US', 'EN-gb']) {
      for (const pic of PICTURES) {
        const key = `${prefix}date/${k++}`;
        put(key, ['pic', numEnc(t), off, enc(lang), enc(pic)]);
        guard(key, () => recordStr(key, formatWordDate(d, pic, lang)));
      }
      for (const instr of INSTRS) {
        const key = `${prefix}date/${k++}`;
        put(key, ['field', numEnc(t), off, enc(lang), enc(instr)]);
        guard(key, () => recordStr(key, dateFieldValue(instr, d, lang)));
      }
    }
  }
}

if (TZ !== 'UTC') {
  // the São Paulo facet: the date suites only, local offsets from the process zone
  dateSuites('tz/');
  // the zone's transitions over the instants' range (hourly scan): what the Rust replay builds
  // its TimeZoneRule::Table from
  const trans = [];
  let prev = null;
  for (let t = Date.UTC(1914, 1, 1); t <= Date.UTC(2040, 0, 1); t += 3600e3) {
    const o = -new Date(t).getTimezoneOffset();
    if (o !== prev) { trans.push(`${t}:${o}`); prev = o; }
  }
  cap.meta('transitions', trans.join(','));
  // replayed by uo-docx-layout (tests/f2_parity.rs) through its Clock and TimeZoneRule
  const tag = TZ.replace(/[^A-Za-z0-9]+/g, '-');
  const out = opt('--out', join(here, '..', '..', 'uo-docx-layout', 'tests', 'captures', 'f2', `datefmt-${tag}`));
  cap.write(`${out}.capture`);
  writeFileSync(`${out}.inputs`, inputs.join('\n') + '\n');
  console.log(`${inputs.length - 1} date cases under ${TZ}`);
  process.exit(0);
}

dateSuites('');

// field formulas: codes over a fixed table of nested values
const FIELD_VALUES = { PAGE: '3', NUMPAGES: '12', SECTIONPAGES: '1.234', SEQ: '2,5', DATE: 'hoje', REF: undefined, NEG: '-7', BIG: '999.999.999', ZERO: '0' };
const fieldValue = (name, instr) => (name in FIELD_VALUES ? FIELD_VALUES[name] : undefined);
const CODES = ['= { NUMPAGES } - 1', '={ PAGE }*2', '= { PAGE } / { NUMPAGES }', '=4/3', '=1/0', '=10/4 \\# "0,00"', '= 2 * (3 + 4)', '= -(-2)', '=--3', '=+5', '= { SECTIONPAGES } + 1',
  '= { SEQ } * 2', '= { DATE }', '= { REF x }', '= { NEG } \\* ROMAN', '= 14 \\* roman', '= 14 \\* Alphabetic', '= 0 \\* ROMAN', '= 2,5 \\* ARABIC', '=  { BIG } \\# "#.##0"', '= 1.000,5 + 1',
  '= 3 \\* MERGEFORMAT \\* ROMAN', '=SUM(1;2)', '= { = { PAGE } + 1 } * 2', '= 1 +', '= (1 + 2', '= 1 2', '= 7 \\# "00" \\* ROMAN', '= -0', '= 1,5 * 2,25', '= 10 / 3 \\# "#,##"',
  '= 5 \\# "0;(0);zero"', '= -5 \\# "0;(0);zero"', '= 0 \\# "0;(0);\'z\'"', '= 1234567,891 \\# "#.##0,00"', '= 0,1 + 0,2', '= 1e3', '= 1.2.3', '= { ZERO } \\* ROMAN', '={PAGE}{PAGE}', '= 2 \\* constructor', '= 3 \\* __proto__'];
for (let i = 0; i < CODES.length; i++) {
  const key = `formula/${i}`;
  put(key, [enc(CODES[i])]);
  guard(key, () => recordStr(key, evaluateFieldFormula(CODES[i], fieldValue, 'pt-BR')));
}
const PICS = ['0', '00,00', '#.##0', '#.##0,00', '0;(0)', '0;(0);zero', "'x'0", '#', 'x0', '#,##', '0,0x', '$ #.##0,00', '0,000000', '##0%', '0.0', 'R$ #.##0,00;-R$ #.##0,00'];
const VALUES = [0, -0, 1, -1, 1.5, 1234.5678, -1234.5, 0.005, 99.995, 1e21, 123456789, 0.1 + 0.2, 2 / 3, -2 / 3, NaN];
let fp = 0;
for (const v of VALUES) {
  for (const p of PICS) {
    const key = `picture/${fp++}`;
    put(key, [numEnc(v), enc(p)]);
    guard(key, () => recordStr(key, formatPicture(v, p)));
  }
  for (const dec of [0, 1, 3]) {
    for (const th of [false, true]) {
      const key = `result/${fp++}`;
      put(key, [numEnc(v), String(dec), th ? '1' : '0']);
      guard(key, () => recordStr(key, formatFormulaResult(v, { n: v, decimals: dec, thousands: th })));
    }
  }
}

// formatNumber
const FMT_NS = [0, -0, 1, 2, 3, 4, 5, 9, 10, 11, 12, 13, 14, 19, 20, 21, 22, 23, 25, 26, 27, 52, 99, 100, 101, 111, 112, 3999, 4000, -3, 2.5, 25.5, NaN, Infinity, 1e15];
// roman of a huge counter builds the text one symbol at a time (the TS never ends): not captured
const ROMAN_MAX = 1e6;
let fn = 0;
for (const n of FMT_NS) {
  for (const f of [undefined, ...NUM_FMTS, '']) {
    if ((f === 'upperRoman' || f === 'lowerRoman') && n > ROMAN_MAX) continue;
    for (const lang of ['pt-BR', 'en-US']) {
      const key = `fmtnum/${fn++}`;
      put(key, [numEnc(n), enc(f), enc(lang)]);
      guard(key, () => { const s = formatNumber(n, f, lang); recordStr(key, s.length > 200 ? `len:${s.length}` : s); });
    }
  }
}

// ───────────── merges, cascade, colours ─────────────
for (let i = 0; i < count; i++) {
  const kind = pick(['rPr', 'pPr', 'tblPr', 'trPr', 'tcPr']);
  const gen = { rPr: rPrXml, pPr: pPrXml, tblPr: tblPrXml, trPr: trPrXml, tcPr: tcPrXml }[kind];
  const read = { rPr: rpr, pPr: ppr, tblPr: tblpr, trPr: trpr, tcPr: tcpr }[kind];
  const merge = { rPr: R.mergeRPr, pPr: R.mergePPr, tblPr: R.mergeTblPr, trPr: R.mergeTrPr, tcPr: R.mergeTcPr }[kind];
  const a = chance(0.9) ? gen() : undefined, b = chance(0.9) ? gen() : undefined;
  const key = `merge/${i}`;
  put(key, [kind, enc(a), enc(b)]);
  guard(key, () => {
    const ra = read(a), rb = read(b);
    recordValue(key, [ra, rb], 'in');
    recordValue(key, merge(ra, rb));
    if (kind === 'rPr') recordValue(key, R.mergeRFonts(ra?.rFonts, rb?.rFonts), 'fonts');
  });
}
for (let i = 0; i < count; i++) {
  const dd = chance(0.8) ? rPrXml() : undefined;
  const levels = Array.from({ length: int(4) }, () => (chance(0.85) ? rPrXml() : undefined));
  const direct = chance(0.7) ? rPrXml() : undefined;
  const key = `cascade/${i}`;
  put(key, [enc(dd), enc(direct), String(levels.length), ...levels.map(enc)]);
  guard(key, () => {
    const lv = levels.map(rpr);
    recordValue(key, [rpr(dd), lv, rpr(direct)], 'in');
    recordValue(key, R.cascadeRPr(rpr(dd), lv, rpr(direct)));
  });
}
for (let i = 0; i < count; i++) {
  const theme = themeXml();
  const r = rPrXml();
  const key = `color/${i}`;
  const hex = pick(['156082', 'e97132', 'zz', '', 'FFFFFF', '000000', '7F7F7F', 'abcdef', '12']);
  const tint = pick(TINTS), shade = pick(TINTS);
  const hl = pick(['yellow', 'darkBlue', 'none', 'toString', undefined, 'white']);
  const slot = pick(['ascii', 'hAnsi', 'eastAsia', 'cs']);
  put(key, [enc(theme), enc(r), enc(hex), enc(tint), enc(shade), enc(hl), slot]);
  guard(key, () => {
    const th = readTheme(theme), rp = rpr(r);
    recordValue(key, [th, rp], 'in');
    recordStr(key, R.resolveColor(rp.color, th), 'color');
    recordStr(key, R.resolveColor(rp.u ? { val: rp.u.color, themeColor: rp.u.themeColor, themeTint: rp.u.themeTint, themeShade: rp.u.themeShade } : undefined, th), 'ucolor');
    recordStr(key, R.shadingFill(rp.shd, th), 'shd');
    recordStr(key, R.applyTintShade(hex, tint, shade), 'tint');
    recordStr(key, R.highlightHex(hl), 'hl');
    recordStr(key, R.fontOfSlot(rp.rFonts, slot, th), 'font');
  });
}

// ───────────── styles, numbering, direct formatting, resolvePPr ─────────────
const RUN_EDITS = ['bold', 'italic', 'underline', 'strike', 'allCaps', 'smallCaps', 'hidden', 'doubleStrike', 'underlineStyle', 'underlineColor', 'color', 'colorTheme', 'underlineTheme', 'highlight', 'fontAscii', 'szHalfPt', 'scalePct', 'positionHalfPt', 'kernHalfPt', 'charSpacingTwips', 'charStyle', 'vertAlign', 'link', 'textFx', 'openType'];
function runEdit() {
  const k = pick(RUN_EDITS);
  const v = {
    bold: () => pick([true, false, undefined]), italic: () => pick([true, false]), underline: () => pick([true, false]), strike: () => pick([true, false]), allCaps: () => pick([true, false]),
    smallCaps: () => pick([true, false]), hidden: () => pick([true, false, undefined]), doubleStrike: () => pick([true, false, undefined]), underlineStyle: () => pick(['double', 'wave', undefined]),
    underlineColor: () => pick(['FF0000', undefined]), color: () => pick(['FF0000', '0563C1', undefined]), colorTheme: () => pick([{ themeColor: 'accent1', tint: '99' }, { themeColor: 'text1' }, undefined]),
    underlineTheme: () => pick([{ themeColor: 'accent2', shade: 'BF' }, undefined]), highlight: () => pick(['yellow', undefined]), fontAscii: () => pick(['Arial', 'Wingdings', undefined]),
    szHalfPt: () => pick([24, 22, undefined, NaN]), scalePct: () => pick([100, 80, undefined]), positionHalfPt: () => pick([0, 4, undefined]), kernHalfPt: () => pick([0, 16, undefined]),
    charSpacingTwips: () => pick([0, 20, undefined]), charStyle: () => pick(['Char1', undefined]), vertAlign: () => pick(['superscript', 'subscript', undefined]), link: () => pick(['https://x', undefined]),
    textFx: () => pick([undefined, { fill: { type: 'none' } }, { fill: { type: 'solid', color: { rgb: '00FF00', mods: [['alpha', 25000]] } }, shadow: { blurRad: 12700, dist: 25400, dir: 5400000, sx: 0, sy: 0, kx: 0, ky: 0, algn: 'tl', color: { rgb: '000000' } } }, { outline: { w: 9525, fill: { type: 'solid', color: { scheme: 'accent1' } } } }]),
    openType: () => pick([undefined, { ligatures: 'standard' }, { numForm: 'lining' }]),
  }[k]();
  return [k, v];
}
const PARA_EDITS = ['alignment', 'styleId', 'numbering', 'indentLeftTwips', 'indentRightTwips', 'indentFirstLineTwips', 'lineTwentieths', 'spaceAfterTwips', 'spaceBeforeTwips', 'spaceBeforeAuto', 'spaceAfterAuto', 'shadingHex', 'tabStops', 'borders', 'keepNext', 'pageBreakBefore', 'widowControl', 'textboxTightWrap'];
function paraEdit() {
  const k = pick(PARA_EDITS);
  const v = {
    alignment: () => pick(['center', 'both', undefined]), styleId: () => pick(['Heading1', '2', undefined]),
    numbering: () => pick([undefined, { kind: 'decimal', ilvl: 1 }, { kind: 'bullet', ilvl: 0 }, { kind: 'decimal', ilvl: 2, numId: 3 }]),
    indentLeftTwips: () => pick([720, 0, undefined]), indentRightTwips: () => pick([360, undefined]), indentFirstLineTwips: () => pick([-360, 360, 0, undefined, NaN]),
    lineTwentieths: () => pick([240, 360, undefined]), spaceAfterTwips: () => pick([0, 120, undefined]), spaceBeforeTwips: () => pick([240, undefined]),
    spaceBeforeAuto: () => pick([true, false, undefined]), spaceAfterAuto: () => pick([true, undefined]), shadingHex: () => pick(['FFFF00', undefined]),
    tabStops: () => pick([undefined, [], [{ posTwips: 720, align: 'left' }, { posTwips: 1440, align: 'right', leader: 'dot' }]]),
    borders: () => pick([undefined, { top: { style: 'single', sizeEighthPt: 4, color: 'FF0000', spacePt: 1 }, bar: { style: 'double', sizeEighthPt: 8, color: undefined, spacePt: 0 } }]),
    keepNext: () => pick([true, false, undefined]), pageBreakBefore: () => pick([true, undefined]), widowControl: () => pick([false, undefined]), textboxTightWrap: () => pick(['allLines', undefined]),
  }[k]();
  return [k, v];
}
// edits travel as JSON with `undefined` as {"$u":1} and NaN as {"$n":"NaN"}
const editJson = (edits) => JSON.stringify(edits, (_k, v) => (v === undefined ? { $u: 1 } : typeof v === 'number' && Number.isNaN(v) ? { $n: 'NaN' } : v));
const applyEdits = (props, edits) => { let p = props; for (const [k, v] of edits) p = { ...p, [k]: v }; return p; };

for (let i = 0; i < fmtCount; i++) {
  const f = { styles: chance(0.95) ? stylesXml() : undefined, numbering: chance(0.9) ? numberingXml() : undefined, theme: themeXml() };
  const key0 = `fmt/${i}`;
  put(key0, [enc(f.styles), enc(f.numbering), enc(f.theme)]);
  let fmt;
  try { fmt = formatting(f); recordValue(key0, fmt, 'in'); } catch (e) { record(key0, 'throw', JSON.stringify(String(e?.name ?? e))); continue; }
  // the style resolver
  const sr = new R.StyleResolver(fmt);
  for (const id of [...STYLE_IDS, undefined, '']) {
    const key = `${key0}/style/${id ?? '-'}`;
    guard(key, () => {
      record(key, 'id', JSON.stringify(sr.paraStyleId(id) ?? null));
      record(key, 'chain', JSON.stringify(sr.paraChain(id).map((s) => s.styleId)));
      recordValue(key, sr.paraStyle(id), 'para');
      recordValue(key, sr.charStyle(id), 'char');
      const t = sr.tableStyle(id);
      recordValue(key, t, 'table');
    });
  }
  // numbering sequences
  const ns = new NumberingState(fmt.numbering, fmt.styles.styles, pick(['pt-BR', 'en-US']));
  const ops = Array.from({ length: 30 }, () => [pick([0, 1, 2, 3, 5, 7, 99, NaN, null]), pick([0, 0, 0, 1, 1, 2, 3, 8, 9, -1, NaN, null, undefined])]);
  const keyN = `${key0}/num`;
  put(keyN, [enc(ns.lang), ...ops.map(([a, b]) => `${numEnc(a)},${numEnc(b)}`)]);
  ops.forEach(([numId, ilvl], j) => {
    const key = `${keyN}/${j}`;
    guard(key, () => {
      const m = ns.next(numId, ilvl ?? 0);
      record(key, 'listKey', JSON.stringify(ns.listKey(numId) ?? null));
      const lvl = ns.level(numId, ilvl ?? 0);
      if (lvl) recordValue(key, lvl, 'level');
      if (!m) { record(key, 'm', 'null'); return; }
      recordValue(key, m, 'm');
    });
  });
  // direct formatting and resolvePPr
  const ctx = new DocContext(fmt, { face: () => null }, { lang: 'pt-BR' });
  for (let j = 0; j < 6; j++) {
    const key = `${key0}/run/${j}`;
    const hasRpr = chance(0.8);
    const rx = hasRpr ? rPrXml() : undefined;
    const edits = Array.from({ length: int(4) }, runEdit);
    const ext = pick(['', 'note', 'sym']);
    put(key, [enc(rx), enc(editJson(edits)), ext]);
    guard(key, () => {
      const r = rpr(rx);
      const base = r ? runPropsFromRPr(r) : { ...DEFAULT_RUN_PROPS };
      const props = applyEdits(base, edits);
      const run = { text: 'x', props, ...(r ? { rpr: r } : {}), ...(ext === 'note' ? { noteRef: { kind: 'footnote', id: 1 } } : ext === 'sym' ? { symFont: 'Symbol' } : {}) };
      recordValue(key, props, 'in');
      recordValue(key, directRPr(run));
      // the Original revision view (the props before a tracked change)
      const rd = new RevisionDisplay({ mode: 'original' }, []);
      recordValue(key, rd.directRPr(run), 'orig');
    });
  }
  for (let j = 0; j < 6; j++) {
    const key = `${key0}/para/${j}`;
    const px = chance(0.85) ? pPrXml() : undefined;
    const edits = Array.from({ length: int(4) }, paraEdit);
    const table = chance(0.3) ? pPrXml() : undefined;
    put(key, [enc(px), enc(editJson(edits)), enc(table)]);
    guard(key, () => {
      const p = ppr(px);
      const props = applyEdits(paraPropsFromPPr(p), edits);
      const para = { runs: [], props, ...(p ? { ppr: p } : {}) };
      recordValue(key, props, 'in');
      recordValue(key, directPPr(para));
      const res = resolvePPr(ctx, para, table ? { tablePPr: ppr(table) } : {});
      recordValue(key, res.ppr, 'ppr');
      record(key, 'styleId', JSON.stringify(res.styleId ?? null));
      recordValue(key, res.styleRPr, 'styleRPr');
      recordValue(key, inheritedPPr(fmt, para), 'inherited');
      recordValue(key, effectivePPr(fmt, para), 'effective');
      record(key, 'outline', JSON.stringify(outlineLevelOf(ctx, para, res.ppr)));
      const rd = new RevisionDisplay({ mode: 'original' }, []);
      const q = rd.paragraph(para);
      recordValue(key, q === para ? null : { ppr: q.ppr, props: q.props }, 'orig');
    });
  }
}

// ───────────── revisions and heading text ─────────────
// (an author named like an Object.prototype key, e.g. `toString`, reads a function from the
// `colors` record in the TS: a named difference, not generated)
const AUTHORS = ['Ana', 'Bia', 'Caio', 'Dani'];
const revInfo = () => ({ author: pick(AUTHORS), ...(chance(0.5) ? { date: 'd1' } : {}), ...(chance(0.2) ? { move: true } : {}) });
for (let i = 0; i < count; i++) {
  const view = { mode: pick(['final', 'simple', 'all', 'original']), ...(chance(0.3) ? { insDel: chance(0.5) } : {}), ...(chance(0.3) ? { formatting: chance(0.5) } : {}), ...(chance(0.3) ? { authors: AUTHORS.slice(0, 1 + int(3)) } : {}), ...(chance(0.2) ? { colors: { Bia: '123456' } } : {}), ...(chance(0.3) ? { order: shuffle([...AUTHORS]) } : {}) };
  const docOrder = shuffle([...AUTHORS]).slice(0, int(4));
  const runs = Array.from({ length: 1 + int(5) }, () => ({ text: pick(['ab', '', 'x😀', 'texto longo']), props: { ...(chance(0.3) ? { ins: revInfo() } : {}), ...(chance(0.3) ? { del: revInfo() } : {}), ...(chance(0.2) ? { fmt: { ...revInfo(), old: {} } } : {}) } }));
  const pprops = { ...(chance(0.3) ? { pfmt: { ...revInfo(), old: {} } } : {}), ...(chance(0.3) ? { markIns: revInfo() } : {}), ...(chance(0.3) ? { markDel: revInfo() } : {}) };
  // a deleted paragraph mark joined to the next one (`joinDeleted`): the first `carried` runs
  const carried = chance(0.3) ? int(runs.length + 1) : -1;
  const key = `rev/${i}`;
  put(key, [enc(editJson(view)), enc(editJson(docOrder)), enc(editJson(runs)), enc(editJson(pprops)), String(carried)]);
  guard(key, () => {
    const rd = new RevisionDisplay(view, docOrder);
    record(key, 'runs', JSON.stringify(runs.map((r) => rd.run(r))));
    record(key, 'gone', JSON.stringify(rd.markGone({ runs: [], props: pprops })));
    record(key, 'bar', JSON.stringify(rd.barColor()));
    record(key, 'colors', JSON.stringify(AUTHORS.map((a) => rd.color(a))));
    const para = carried >= 0 ? joinDeleted(runs.slice(0, carried), { runs: runs.slice(carried), props: pprops }) : { runs, props: pprops };
    record(key, 'ranges', JSON.stringify(revisionRanges(rd, para)));
  });
}

const TEXTS = ['  Título­ um\t dois\n', '￼Objeto', 'ß straße', 'ǅ x', ' \u000b\u000c ', 'a b', '😀 emoji', 'x\uD800'];
for (let i = 0; i < 60; i++) {
  const runs = Array.from({ length: 1 + int(4) }, () => ({ text: pick(TEXTS), props: chance(0.2) ? { del: { author: 'A' } } : {}, caps: chance(0.4) }));
  const key = `heading/${i}`;
  put(key, [enc(editJson(runs.map((r) => [r.text, r.props.del ? 1 : 0, r.caps ? 1 : 0])))]);
  guard(key, () => {
    const p = { runs: runs.map((r) => ({ text: r.text, props: r.props })), props: {} };
    const capsOf = new Map(p.runs.map((r, j) => [r, runs[j].caps]));
    recordStr(key, headingText(p, (r) => capsOf.get(r)));
    recordStr(key, headingText(p), 'plain');
  });
}

const out = opt('--out', join(here, '..', 'tests', 'parity', 'word'));
cap.write(`${out}.capture`);
writeFileSync(`${out}.inputs`, inputs.join('\n') + '\n');
console.log(`${inputs.length - 1} cases`);
