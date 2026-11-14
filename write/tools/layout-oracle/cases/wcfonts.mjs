// WordCraft study (2026-11-13), section C (fonts): what Word for Windows does where WordCraft's rules (mostly Word for
// Mac) and ours differ. Run with --fresh (Word remembers the substitution of a missing font for the session).
//  c1-*  line height of a line with a Symbol/Wingdings bullet or run (Calibri 11, multiple 1.08 and single)
//  c2-*  Windows 3.1 charset-suffix names ("Times New Roman Cyr", "Arial CE"…): which face Word draws
//  c4-*  a run in a MISSING Thai font: which face(s) Word draws the Thai and the Latin/digits of that run in
//  c5-*  PostScript / full / legacy names of installed faces ("Cambria-Bold", "Calibri Light", "Segoe UI Semibold"…)
//  c6-*  Symbol-encoded PUA text (U+F0xx) in a MISSING symbol font (charset 02)
//  k-*   confirmations of rules already measured (Times, Helvetica, unknown swiss, unknown outside the font table)
// The face is read from the PDF (font name per character), the geometry from the .word.json lines.
import { strToU8 } from 'fflate';
import { buildDocx, docDefaults, esc, p, r, sz } from '../docx-builder.mjs';

const W_NS = 'xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"';
const fontTable = (entries) => strToU8(`<?xml version="1.0" encoding="UTF-8" standalone="yes"?><w:fonts ${W_NS}>${entries}</w:fonts>`);
const fe = (name, { charset = '00', family = 'auto', panose, pitch = 'variable' } = {}) =>
  `<w:font w:name="${esc(name)}">${panose ? `<w:panose1 w:val="${panose}"/>` : ''}<w:charset w:val="${charset}"/><w:family w:val="${family}"/><w:pitch w:val="${pitch}"/></w:font>`;
const FT_BASE = fe('Calibri', { family: 'swiss', panose: '020F0502020204030204' }) + fe('Times New Roman', { family: 'roman', panose: '02020603050405020304' });
const SYMBOL = fe('Symbol', { charset: '02', family: 'roman', panose: '05050102010706020507' });
const WINGDINGS = fe('Wingdings', { charset: '02', family: 'auto', panose: '05000000000000000000' });
const rf = (f, extra = '') => `<w:rFonts w:ascii="${esc(f)}" w:hAnsi="${esc(f)}" w:cs="${esc(f)}"${extra}/>`;
const TEXT = 'Hxg WMwm iIl1 0O quick brown fox jumps over the lazy dog';
const LONG = `${TEXT}; ${TEXT}; ${TEXT}.`;

const bulletNumbering = (font, ch) => `<w:abstractNum w:abstractNumId="0"><w:multiLevelType w:val="hybridMultilevel"/><w:lvl w:ilvl="0"><w:start w:val="1"/><w:numFmt w:val="bullet"/><w:lvlText w:val="${ch}"/><w:lvlJc w:val="left"/><w:pPr><w:ind w:left="720" w:hanging="360"/></w:pPr><w:rPr><w:rFonts w:ascii="${font}" w:hAnsi="${font}" w:hint="default"/></w:rPr></w:lvl></w:abstractNum><w:num w:numId="1"><w:abstractNumId w:val="0"/></w:num>`;

export const cases = [];
const add = (name, body, { font = 'Calibri', size = 11, line = 259, ft = '', numbering, lang = 'en-US' } = {}) => cases.push({
  name,
  docx: buildDocx({ body, numbering, defaults: docDefaults({ font, size, after: 0, line, lang }), extraParts: { 'word/fontTable.xml': fontTable(FT_BASE + ft) } }),
});

// ── C1: Symbol / Wingdings bullets and runs ──
const plain = (n) => Array.from({ length: n }, (_, i) => p(r(`Plain line ${i + 1} Hxg`))).join('');
const bullets = (n) => Array.from({ length: n }, (_, i) => p(r(`Bullet item ${i + 1} Hxg`), '<w:numPr><w:ilvl w:val="0"/><w:numId w:val="1"/></w:numPr>')).join('');
for (const [line, tag] of [[259, '108'], [240, '100']]) {
  add(`c1-symbul-${tag}`, plain(6) + bullets(8) + plain(6), { line, ft: SYMBOL, numbering: bulletNumbering('Symbol', '') });
  add(`c1-wingbul-${tag}`, plain(6) + bullets(8) + plain(6), { line, ft: WINGDINGS, numbering: bulletNumbering('Wingdings', '') });
  add(`c1-symrun-${tag}`, plain(6) + Array.from({ length: 8 }, (_, i) => p(r(`Run ${i + 1} Hxg `) + r('abg', rf('Symbol')))).join('') + plain(6), { line, ft: SYMBOL });
  add(`c1-wingrun-${tag}`, plain(6) + Array.from({ length: 8 }, (_, i) => p(r(`Run ${i + 1} Hxg `) + r('', rf('Wingdings')))).join('') + plain(6), { line, ft: WINGDINGS });
}

// ── C2: charset-suffix aliases (in the font table with the matching charset, as old documents have them) ──
for (const [name, charset, family, panose] of [
  ['Times New Roman Cyr', 'CC', 'roman', '02020603050405020304'],
  ['Arial CE', 'EE', 'swiss', '020B0604020202020204'],
  ['Courier New Baltic', 'BA', 'modern', '02070309020205020404'],
  ['Arial (Hebrew)', 'B1', 'swiss', '020B0604020202020204'],
  ['Times New Roman Greek', 'A1', 'roman', '02020603050405020304'],
  ['Arial TUR', 'A2', 'swiss', '020B0604020202020204'],
  ['Arial Cyr', 'CC', 'swiss', '020B0604020202020204'],
]) {
  const slug = name.toLowerCase().replace(/[^a-z]+/g, '-').replace(/-+$/, '');
  const body = p(r('Before Hxg')) + p(r(LONG, rf(name) + sz(12))) + p(r('After Hxg'));
  add(`c2-${slug}`, body, { line: 240, ft: fe(name, { charset, family, panose, pitch: family === 'modern' ? 'fixed' : 'variable' }) });
  add(`c2n-${slug}`, body, { line: 240 }); // not in the font table
}

// ── C4: a run in a missing Thai font (Thai + Latin + digits in ONE run) ──
const THAI = 'abc ภาษาไทยทดสอบ def 123 ภาษา xyz';
for (const [tag, name, inTable] of [['zz', 'ZzThaiFont', false], ['angsana', 'Angsana New', true], ['sarabun', 'TH SarabunPSK', true]]) {
  const body = p(r('Before Hxg')) + p(r(THAI, rf(name) + sz(14) + '<w:lang w:val="en-US" w:bidi="th-TH"/>'), '<w:spacing w:line="240" w:lineRule="auto"/>') + p(r('After Hxg'));
  add(`c4-${tag}`, body, { line: 240, ft: inTable ? fe(name, { charset: 'DE', family: 'roman', panose: '02020603050405020304' }) : '' });
  // the same with only the complex-script slot naming the Thai font (ascii/hAnsi = Calibri)
  const body2 = p(r('Before Hxg')) + p(r(THAI, `<w:rFonts w:cs="${esc(name)}"/>` + sz(14) + '<w:lang w:val="en-US" w:bidi="th-TH"/>')) + p(r('After Hxg'));
  add(`c4cs-${tag}`, body2, { line: 240, ft: inTable ? fe(name, { charset: 'DE', family: 'roman', panose: '02020603050405020304' }) : '' });
}

// C4 follow-up: is the one-face-per-run substitution keyed on the font table's Thai charset (DE) or on the name?
// (fictitious / non-Microsoft names: Word cannot fetch them from the Office cloud fonts)
for (const [tag, name, entry] of [
  ['zzde-roman', 'ZzThaiFont', { charset: 'DE', family: 'roman', panose: '02020603050405020304' }],
  ['zzde-swiss', 'ZzThaiFont', { charset: 'DE', family: 'swiss', panose: '020B0604020202020204' }],
  ['niramit', 'TH Niramit AS', null],
  ['niramit-de', 'TH Niramit AS', { charset: 'DE', family: 'auto' }],
  ['zz00-roman', 'ZzThaiFont', { charset: '00', family: 'roman', panose: '02020603050405020304' }],
]) {
  const body = p(r('Before Hxg')) + p(r(THAI, rf(name) + sz(14) + '<w:lang w:val="en-US" w:bidi="th-TH"/>')) + p(r('After Hxg'));
  add(`c4t-${tag}`, body, { line: 240, ft: entry ? fe(name, entry) : '' });
}

// ── C5: PostScript / full / legacy family names of installed faces ──
for (const name of ['Cambria-Bold', 'Cambria Bold', 'Calibri Light', 'Calibri-Light', 'Segoe UI Semibold', 'SegoeUI-Semibold', 'Segoe UI Light',
  'Arial-BoldMT', 'Arial Bold', 'TimesNewRomanPS-BoldMT', 'Times New Roman Bold', 'ArialMT', 'Arial Narrow', 'ArialNarrow', 'Bahnschrift Light', 'Bahnschrift']) {
  const slug = name.toLowerCase().replace(/ /g, '_').replace(/[^a-z_-]+/g, '');
  add(`c5-${slug}`, p(r('Before Hxg')) + p(r(LONG, rf(name) + sz(12))) + p(r('After Hxg')), { line: 240 });
}

// ── C6: symbol-encoded PUA text in a missing symbol font, and the installed Symbol for reference ──
for (const [tag, name] of [['zz', 'ZzSymbolFont'], ['symbol', 'Symbol']]) {
  const ft = fe(name, { charset: '02', family: 'roman', panose: '05050102010706020507' });
  const body = p(r('Before Hxg')) + p(r('A ') + r('', rf(name) + sz(14)) + r(' Z'))
    + p(r('B ') + `<w:r><w:rPr>${sz(14)}</w:rPr><w:sym w:font="${name}" w:char="F0B7"/></w:r>` + `<w:r><w:rPr>${sz(14)}</w:rPr><w:sym w:font="${name}" w:char="F061"/></w:r>` + r(' Z'))
    + p(r('C ') + r('abg', rf(name) + sz(14)) + r(' Z')) + p(r('After Hxg'));
  add(`c6-${tag}`, body, { line: 240, ft });
}

// ── confirmations ──
for (const [tag, name, ft] of [
  ['times', 'Times', ''],
  ['helvetica', 'Helvetica', ''],
  ['zzswiss', 'ZzSansFont', fe('ZzSansFont', { family: 'swiss', panose: '020B0604020202020204' })],
  ['zzroman', 'ZzSerifFont', fe('ZzSerifFont', { family: 'roman', panose: '02020603050405020304' })],
  ['zznotable', 'ZzNoTableFont', ''],
]) {
  add(`k-${tag}`, p(r('Before Hxg')) + p(r(LONG, rf(name) + sz(12))) + p(r('After Hxg')), { line: 240, ft });
}
