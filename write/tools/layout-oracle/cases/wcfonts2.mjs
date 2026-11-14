// WordCraft study (2026-11-13), planner "fields" — the font measurements section C left open. Run with --fresh (Word
// remembers a missing font's substitution for the session). Face per character from the PDF (fh/tools/pdffonts.py).
//  c2x-*  charset suffixes on families outside Windows' FontSubstitutes (is the strip general or a table?)
//  c5x-*  full names ("Family Style") and PostScript names of more installed families, italic too (why "Cambria Bold"
//         gave the bold face and "Arial Bold" the regular one)
//  c1x-*  multiple spacing (1.5) over a large Symbol / Wingdings / Times New Roman run: does a symbol-font run enter the
//         text height that the multiple multiplies (C1 side finding: +0.04 pt per line at 1.08)
//  c6x-*  a missing font with charset 02 and family auto / decorative / swiss
//  a2x-*  the A2 aside: Segoe UI + Japanese with lang ja-JP drew the WHOLE run (Latin too) in MS Mincho — which part of
//         the run properties triggers it
import { strToU8 } from 'fflate';
import { buildDocx, docDefaults, esc, p, r, sz } from '../docx-builder.mjs';

const W_NS = 'xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"';
const fontTable = (entries) => strToU8(`<?xml version="1.0" encoding="UTF-8" standalone="yes"?><w:fonts ${W_NS}>${entries}</w:fonts>`);
const fe = (name, { charset = '00', family = 'auto', panose, pitch = 'variable' } = {}) =>
  `<w:font w:name="${esc(name)}">${panose ? `<w:panose1 w:val="${panose}"/>` : ''}<w:charset w:val="${charset}"/><w:family w:val="${family}"/><w:pitch w:val="${pitch}"/></w:font>`;
const FT_BASE = fe('Calibri', { family: 'swiss', panose: '020F0502020204030204' }) + fe('Times New Roman', { family: 'roman', panose: '02020603050405020304' });
const rf = (f) => `<w:rFonts w:ascii="${esc(f)}" w:hAnsi="${esc(f)}" w:cs="${esc(f)}"/>`;
const TEXT = 'Hxg WMwm iIl1 0O quick brown fox jumps over the lazy dog';
const LONG = `${TEXT}; ${TEXT}; ${TEXT}.`;
export const cases = [];
const add = (name, body, { font = 'Calibri', size = 11, line = 240, ft = '', lang = 'en-US', defaults } = {}) => cases.push({
  name,
  docx: buildDocx({ body, defaults: defaults ?? docDefaults({ font, size, after: 0, line, lang }), extraParts: { 'word/fontTable.xml': fontTable(FT_BASE + ft) } }),
});
const slug = (s) => s.toLowerCase().replace(/ /g, '_').replace(/[()]/g, '').replace(/[^a-z0-9_-]+/g, '');

// ── C2: charset suffixes outside FontSubstitutes ──
for (const [name, charset, family] of [['Calibri CE', 'EE', 'swiss'], ['Verdana Cyr', 'CC', 'swiss'], ['Tahoma Greek', 'A1', 'swiss'],
  ['Georgia TUR', 'A2', 'roman'], ['Segoe UI Baltic', 'BA', 'swiss'], ['Courier New CYR', 'CC', 'modern'], ['Times New Roman (Arabic)', 'B2', 'roman'],
  ['Arial (Vietnamese)', 'A3', 'swiss']]) {
  const body = p(r('Before Hxg')) + p(r(LONG, rf(name) + sz(12))) + p(r('After Hxg'));
  add(`c2x-${slug(name)}`, body, { ft: fe(name, { charset, family, pitch: family === 'modern' ? 'fixed' : 'variable' }) });
}

// ── C5: full names and PostScript names ──
for (const name of ['Arial Italic', 'Cambria Italic', 'Cambria Bold Italic', 'Calibri Bold', 'Calibri Italic', 'Georgia Bold', 'Segoe UI Bold',
  'Verdana Bold', 'Candara Bold', 'Constantia Bold', 'Corbel Bold', 'Consolas Bold', 'Courier New Bold', 'Tahoma Bold', 'Arial Black',
  'Arial-ItalicMT', 'Arial-BoldItalicMT', 'TimesNewRomanPS-ItalicMT', 'Calibri-Bold', 'Calibri-Italic', 'Georgia-Bold', 'CalibriLight',
  'SegoeUISemibold', 'ArialNarrow-Bold', 'CourierNewPSMT', 'Verdana-Bold', 'Cambria-Italic', 'Candara-Bold']) {
  add(`c5x-${slug(name)}`, p(r('Before Hxg')) + p(r(LONG, rf(name) + sz(12))) + p(r('After Hxg')));
}

// ── C1: multiple spacing over a large symbol-font run ──
const SYMBOL = fe('Symbol', { charset: '02', family: 'roman', panose: '05050102010706020507' });
const WINGDINGS = fe('Wingdings', { charset: '02', family: 'auto', panose: '05000000000000000000' });
const plain = (n) => Array.from({ length: n }, (_, i) => p(r(`Plain line ${i + 1} Hxg`))).join('');
for (const [tag, name, ft] of [['symbol', 'Symbol', SYMBOL], ['wingdings', 'Wingdings', WINGDINGS], ['tnr', 'Times New Roman', ''], ['symsmall', 'Symbol', SYMBOL]]) {
  const big = tag === 'symsmall' ? 11 : 20;
  const lines = Array.from({ length: 6 }, (_, i) => p(r(`Run ${i + 1} Hxg `) + r(tag === 'wingdings' ? '' : 'abg', rf(name) + sz(big)))).join('');
  add(`c1x-${tag}-150`, plain(4) + lines + plain(4), { line: 360, ft });
}

// ── C6: missing font with charset 02, other families ──
for (const [tag, family] of [['auto', 'auto'], ['decorative', 'decorative'], ['swiss', 'swiss']]) {
  const name = 'ZzSymbolFont';
  const ft = fe(name, { charset: '02', family });
  const body = p(r('Before Hxg')) + p(r('A ') + r('', rf(name) + sz(14)) + r(' Z')) + p(r('C ') + r('abg', rf(name) + sz(14)) + r(' Z')) + p(r('After Hxg'));
  add(`c6x-${tag}`, body, { ft });
}

// ── A2 aside: a Latin font with Japanese text ──
const JA = (n) => Array.from({ length: n }, (_, i) => `Hxg 日本語 ${i}`).join('\n');
const LATIN = (n) => Array.from({ length: n }, (_, i) => `Hxg abc ${i}`).join('\n');
const dd = (rfonts, lang) => `<w:docDefaults><w:rPrDefault><w:rPr>${rfonts}<w:sz w:val="22"/><w:szCs w:val="22"/>${lang}</w:rPr></w:rPrDefault><w:pPrDefault><w:pPr><w:spacing w:after="0" w:line="240" w:lineRule="auto"/></w:pPr></w:pPrDefault></w:docDefaults>`;
const SEG = '<w:rFonts w:ascii="Segoe UI" w:hAnsi="Segoe UI" w:eastAsia="Segoe UI" w:cs="Segoe UI"/>';
const twelve = '<w:sz w:val="24"/><w:szCs w:val="24"/>';
for (const [tag, rfonts, lang, text, split] of [
  ['valen-eaja', SEG, '<w:lang w:val="en-US" w:eastAsia="ja-JP" w:bidi="ar-SA"/>', JA(6), false],
  ['valja-latin', SEG, '<w:lang w:val="ja-JP" w:eastAsia="en-US" w:bidi="ar-SA"/>', LATIN(6), false],
  ['valja-split', SEG, '<w:lang w:val="ja-JP" w:eastAsia="en-US" w:bidi="ar-SA"/>', null, true],
  ['valja-eagothic', '<w:rFonts w:ascii="Segoe UI" w:hAnsi="Segoe UI" w:eastAsia="MS Gothic" w:cs="Segoe UI"/>', '<w:lang w:val="ja-JP" w:eastAsia="en-US" w:bidi="ar-SA"/>', JA(6), false],
  ['valja-calibri', '<w:rFonts w:ascii="Calibri" w:hAnsi="Calibri" w:eastAsia="Calibri" w:cs="Calibri"/>', '<w:lang w:val="ja-JP" w:eastAsia="en-US" w:bidi="ar-SA"/>', JA(6), false],
  ['valja-eaja', SEG, '<w:lang w:val="ja-JP" w:eastAsia="ja-JP" w:bidi="ar-SA"/>', JA(6), false],
  ['valen-eaen', SEG, '<w:lang w:val="en-US" w:eastAsia="en-US" w:bidi="ar-SA"/>', JA(6), false],
  ['arial-zh', '<w:rFonts w:ascii="Arial" w:hAnsi="Arial" w:eastAsia="Arial" w:cs="Arial"/>', '<w:lang w:val="zh-CN" w:eastAsia="zh-CN" w:bidi="ar-SA"/>', Array.from({ length: 6 }, (_, i) => `Hxg 中文字 ${i}`).join('\n'), false],
]) {
  const body = split
    ? Array.from({ length: 6 }, (_, i) => p(r(`Hxg `, twelve) + r(`日本語 ${i}`, twelve))).join('')
    : p(r(text, twelve));
  add(`a2x-${tag}`, body, { defaults: dd(rfonts, lang) });
}
