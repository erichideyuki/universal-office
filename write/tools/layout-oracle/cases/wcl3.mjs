// WordCraft study, planner "L", third batch (see wcl.mjs / wcl2.mjs): what the first two left open.
//  - A1/A19: jc=distribute in more fonts — the compression factor (Calibri 0.77, Arial 0.83 of the "both" rule) and
//    the per-space cap of the spread, at 48 pt so the PDF's 0.12 pt grid matters less;
//  - A11: the superscript/subscript sizes between the samples of wcl (48.5–71.5, odd 73–99, even 102–198) and 250–1000;
//  - kinsoku: the punctuation that HANGS when justified, zh-TW and ko-KR lists, the language inherited from the defaults;
//  - emphasis marks: the same body without w:em (control) and more sizes;
//  - character grid: Latin placement under snapToChars, the base size (Normal style × defaults), alignment;
//  - hangingChars/rightChars: which size is the unit, and how they combine with w:left.
// Run with --fixdir core/target/wordcraft-study/L/wcl3.
import { buildDocx, docDefaults, p, r, sz, font } from '../docx-builder.mjs';
import { createNodeFontProvider } from '../../corpus-lab/node-fonts.mjs';

const prov = await createNodeFontProvider();
const widthOf = (name, size) => {
  const f = prov.face(name, false, false)?.font;
  if (!f) throw new Error('font ' + name);
  return (s) => [...s].reduce((a, ch) => a + Math.floor((f.advanceWidth(f.glyphId(ch.codePointAt(0))) * size * 4096) / f.metrics.unitsPerEm) / 4096 * 20, 0);
};
const TEXTW = 11906 - 2 * 1701;
const ind = (left, right, extra = '') => `<w:ind w:left="${Math.round(left)}" w:right="${Math.round(right)}"${extra}/>`;
const Dl = (body, f = 'Calibri', s = 11, o = {}) => buildDocx({ body, defaults: docDefaults({ font: f, size: s }), ...o });
const ea = (f, lang = 'ja-JP') => `<w:rFonts w:ascii="${f}" w:hAnsi="${f}" w:eastAsia="${f}" w:cs="${f}"/><w:lang w:eastAsia="${lang}"/>`;
const jaDefaults = (size, eaFont = 'MS Gothic', lang = 'ja-JP') => docDefaults({ font: 'Calibri', size })
  .replace(/<w:rFonts[^>]*\/>/, `<w:rFonts w:ascii="Calibri" w:hAnsi="Calibri" w:eastAsia="${eaFont}" w:cs="Calibri"/>`)
  .replace('w:eastAsia="en-US"', `w:eastAsia="${lang}"`);
const sect = (grid = '<w:docGrid w:linePitch="360"/>') => `<w:sectPr><w:pgSz w:w="11906" w:h="16838"/><w:pgMar w:top="1417" w:right="1701" w:bottom="1417" w:left="1701" w:header="708" w:footer="708" w:gutter="0"/><w:cols w:space="708"/>${grid}</w:sectPr>`;

export const cases = [];
const FONTS = ['Calibri', 'Arial', 'Times New Roman', 'Verdana', 'Georgia', 'Courier New', 'Segoe UI', 'Cambria', 'Liberation Sans', 'Carlito'];
const slug = (f) => f.toLowerCase().replace(/\s+/g, '');

// ── distribute compression factor per font (1-twip steps over f = 0.6 … 1.03 of the both rule)
for (const f of FONTS) {
  const w = widthOf(f, 11);
  const g = w(' ');
  let body = '';
  for (const [n, T] of [[12, 'que'], [24, 'que'], [16, 'e']]) {
    const S = n * g;
    const W = w(T);
    const base = n * (w('ab') + g) + W;
    const at = (fct) => (0.5 * fct * S * (W + g)) / (S - g + 0.5 * fct * S);
    for (let ov = Math.floor(at(0.6)); ov <= Math.ceil(Math.min(at(1.03), 0.25 * S)); ov++) {
      body += p(r(Array(n).fill('ab').join(' ') + ' ' + T + ' fim da linha seguinte'), `<w:jc w:val="distribute"/>${ind(0, TEXTW - Math.round(base - ov))}`);
    }
  }
  cases.push({ name: `m-jdist-${slug(f)}`, docx: Dl(body, f) });
}

// ── the spread at 48 pt (and the CJK font): slack swept per space
for (const f of FONTS.concat(['MS Gothic'])) {
  const s = 48;
  const w = widthOf(f, s);
  let body = '';
  for (const t of ['abcd efgh', 'ab cd ef']) {
    const nsp = t.split(' ').length - 1;
    const g = w(' ') / 20;
    for (let k = 0; k <= 48; k++) {
      const sl = nsp * g * k * 0.025; // per space 0 … 1.2 spaces
      body += p(r(t, sz(s)), `<w:jc w:val="distribute"/>${ind(0, TEXTW - Math.round(w(t) + sl * 20))}`);
    }
    for (const per of [1.5, 2, 3, 5]) body += p(r(t, sz(s)), `<w:jc w:val="distribute"/>${ind(0, TEXTW - Math.round(w(t) + nsp * g * per * 20))}`);
  }
  const extra = f === 'MS Gothic' ? '<w:lang w:eastAsia="ja-JP"/>' : '';
  cases.push({ name: `m-jspread48-${slug(f)}`, docx: buildDocx({ body: body.replaceAll('<w:rPr>', `<w:rPr>${extra}`), defaults: docDefaults({ font: f, size: s }).replace(/<w:rFonts[^>]*\/>/, `<w:rFonts w:ascii="${f}" w:hAnsi="${f}" w:eastAsia="${f}" w:cs="${f}"/>`) }) });
}

// ── superscript / subscript: the sizes between wcl's samples
{
  const big = '<w:sectPr><w:pgSz w:w="31680" w:h="31680"/><w:pgMar w:top="720" w:right="720" w:bottom="720" w:left="720" w:header="360" w:footer="360" w:gutter="0"/><w:cols w:space="708"/><w:docGrid w:linePitch="360"/></w:sectPr>';
  const sizes = [];
  for (let s = 48.5; s < 72; s += 1) sizes.push(s);
  for (let s = 72.5; s < 100; s += 0.5) if (!(s >= 74 && s % 2 === 0)) sizes.push(s);
  for (let s = 100.5; s <= 200; s += 0.5) if (s % 10 !== 0) sizes.push(s);
  for (const s of [250, 300, 400, 500, 600, 800, 1000]) sizes.push(s);
  for (const f of ['Arial', 'Calibri', 'Courier New', 'Segoe UI', 'Times New Roman', 'Tahoma', 'Verdana', 'Cambria', 'Georgia', 'Century Gothic']) {
    const body = sizes.map((s) => p(r('H', sz(s)) + r('x', sz(s) + '<w:vertAlign w:val="superscript"/>') + r('H', sz(s)) + r('x', sz(s) + '<w:vertAlign w:val="subscript"/>'))).join('');
    cases.push({ name: `m-supfill-${slug(f)}`, docx: Dl(body, f, 11, { sect: big }) });
  }
}

// ── kinsoku: hanging when justified (every no-start character), zh-TW and ko-KR lists, inherited language
{
  const noStart = '!%),.:;?]}¢°’”‰′″℃、。々〉》」』】〕゛゜ゝゞ・ヽヾ！％），．：；？］｝｡｣､･ﾞﾟ￠‐–…ぁっーゃ〜‼〙〗｠»';
  const noEnd = '$([{£¥‘“〈《「『【〔＄（［｛｢￡￥〝«〘〖｟';
  const W = TEXTW - 2400;
  let hang = '';
  for (const c of noStart) hang += p(r('ああああああああああ' + c + 'ああ', ea('MS Gothic') + sz(12)), ind(0, W) + '<w:jc w:val="both"/>');
  cases.push({ name: 'm-kinsoku-hang-ja', docx: Dl(hang, 'Calibri', 12, { defaults: jaDefaults(12) }) });
  let hangzh = '';
  for (const c of noStart) hangzh += p(r('ああああああああああ' + c + 'ああ', ea('SimSun', 'zh-CN') + sz(12)), ind(0, W) + '<w:jc w:val="both"/>');
  cases.push({ name: 'm-kinsoku-hang-zh-cn', docx: Dl(hangzh, 'Calibri', 12, { defaults: jaDefaults(12, 'SimSun', 'zh-CN') }) });
  for (const [lang, f] of [['zh-TW', 'MingLiU'], ['ko-KR', 'Malgun Gothic']]) {
    let body = '';
    for (const c of noStart) body += p(r('ああああああああああ' + c + 'ああ', ea(f, lang) + sz(12)), ind(0, W));
    for (const c of noEnd) body += p(r('あああああああああ' + c + 'あああ', ea(f, lang) + sz(12)), ind(0, W));
    cases.push({ name: `m-kinsoku-${lang.toLowerCase()}`, docx: Dl(body, 'Calibri', 12, { defaults: jaDefaults(12, f, lang) }) });
  }
  // the language only from the defaults (no w:lang on the run); and no East Asian language anywhere
  let inh = '';
  for (const c of '、」！)') inh += p(r('ああああああああああ' + c + 'ああ', `<w:rFonts w:ascii="MS Gothic" w:hAnsi="MS Gothic" w:eastAsia="MS Gothic"/>` + sz(12)), ind(0, W));
  cases.push({ name: 'm-kinsoku-inherit-ja', docx: Dl(inh, 'Calibri', 12, { defaults: jaDefaults(12) }) });
  cases.push({ name: 'm-kinsoku-inherit-en', docx: Dl(inh, 'Calibri', 12) });
}

// ── emphasis marks: control (no w:em) and the same with marks, sizes 8/16/36, Latin/CJK, Times
{
  const runs = [['enfase latina Hxg', ''], ['enfase grande Hxg', sz(22)], ['enfase arial Hxg', font('Arial')], ['強調の点です', ea('MS Gothic') + sz(10.5)],
    ['強調の点です', ea('MS Gothic') + sz(21)], ['強調の点です', ea('Yu Gothic') + sz(11)], ['enfase oito Hxg', sz(8)], ['enfase 36 Hxg', sz(36)], ['enfase times Hxg', font('Times New Roman')],
    ['強調の点です', ea('MS Gothic') + sz(36)]];
  for (const em of [null, 'dot', 'comma', 'circle', 'underDot']) {
    let body = p(r('ref 0 Hxg'));
    runs.forEach(([t, rpr], k) => { body += p(r(t, rpr + (em ? `<w:em w:val="${em}"/>` : ''))) + p(r(`ref ${k + 1} Hxg`)); });
    cases.push({ name: `m-emph-${em ?? 'none'}`, docx: Dl(body, 'Calibri', 11, { defaults: jaDefaults(11) }) });
  }
}

// ── character grid: Latin words alone under snapToChars, base size from Normal × defaults, alignment under lc
{
  const words = ['a', 'ab', 'abc', 'abcd', 'abcdefg', 'abcdefghijkl', 'Hxg Hxg', 'abc あ', 'あ abc', 'あabcあ', 'あ abcdefghij あ'];
  const body = words.map((w0) => p(r(w0))).join('') + p(r('The quick brown fox jumps over the lazy dog and keeps running far away and then the quick brown fox jumps again'));
  for (const cs of [0, 4096]) {
    const grid = `<w:docGrid w:type="snapToChars" w:linePitch="360" w:charSpace="${cs}"/>`;
    cases.push({ name: `m-cgrid-sc-lat-${cs}`, docx: Dl(body, 'Calibri', 10.5, { defaults: jaDefaults(10.5), sect: sect(grid) }) });
    // Normal style 14 over defaults 10.5: which is the base of the cell?
    cases.push({ name: `m-cgrid-sc-normal14-${cs}`, docx: Dl(body + p(r('あいうえおかきくけこさしすせそたちつてとなにぬねのはひふへほあいうえおかきくけこ')), 'Calibri', 10.5, { defaults: jaDefaults(10.5), normal: `<w:rPr>${sz(14)}</w:rPr>`, sect: sect(grid) }) });
    cases.push({ name: `m-cgrid-lc-normal14-${cs}`, docx: Dl(body + p(r('あいうえおかきくけこさしすせそたちつてとなにぬねのはひふへほあいうえおかきくけこ')), 'Calibri', 10.5, { defaults: jaDefaults(10.5), normal: `<w:rPr>${sz(14)}</w:rPr>`, sect: sect(grid.replace('snapToChars', 'linesAndChars')) }) });
  }
  // alignment and justification under snapToChars with a short CJK line, and the run-level snapToGrid
  const al = ['left', 'center', 'right', 'both', 'distribute'].map((jc) => p(r('あいう'), `<w:jc w:val="${jc}"/>`)).join('')
    + p(r('あいうえお') + r('かきくけこ', '<w:snapToGrid w:val="0"/>') + r('さしすせそ'));
  cases.push({ name: 'm-cgrid-sc-align', docx: Dl(al, 'Calibri', 10.5, { defaults: jaDefaults(10.5), sect: sect('<w:docGrid w:type="snapToChars" w:linePitch="360" w:charSpace="4096"/>') }) });
  cases.push({ name: 'm-cgrid-lc-align', docx: Dl(al, 'Calibri', 10.5, { defaults: jaDefaults(10.5), sect: sect('<w:docGrid w:type="linesAndChars" w:linePitch="360" w:charSpace="4096"/>') }) });
}

// ── hangingChars / rightChars / leftChars: the unit (run × mark) and the combination with w:left
{
  const T = (s) => 'linha um do paragrafo Hxg ' + s + ' e a segunda linha que continua bastante para quebrar com folga no fim da coluna aqui';
  const body = [
    p(r(T('hang only')), '<w:ind w:hangingChars="200"/>'),
    p(r(T('hang left0')), '<w:ind w:left="0" w:hangingChars="200"/>'),
    p(r(T('hang left1440')), '<w:ind w:left="1440" w:hangingChars="200"/>'),
    p(r(T('hang leftChars0')), '<w:ind w:leftChars="0" w:left="1440" w:hangingChars="200" w:hanging="420"/>'),
    p(r(T('hang leftChars100')), '<w:ind w:leftChars="100" w:left="1440" w:hangingChars="200" w:hanging="420"/>'),
    p(r(T('hang run20'), sz(20)), '<w:ind w:hangingChars="200"/>'),
    p(r(T('hang mark20')), '<w:ind w:hangingChars="200"/><w:rPr>' + sz(20) + '</w:rPr>'),
    p(r(T('right run20'), sz(20)), '<w:jc w:val="right"/><w:ind w:rightChars="300"/>'),
    p(r(T('right mark20')), '<w:jc w:val="right"/><w:ind w:rightChars="300"/><w:rPr>' + sz(20) + '</w:rPr>'),
    p(r(T('left mark20 run11')), '<w:ind w:leftChars="300"/><w:rPr>' + sz(20) + '</w:rPr>'),
    p(r(T('first and left')), '<w:ind w:leftChars="200" w:firstLineChars="100"/>'),
    p(r(T('first chars and left tw')), '<w:ind w:left="1440" w:firstLineChars="100"/>'),
    p(r(T('left chars and first tw')), '<w:ind w:leftChars="200" w:firstLine="720"/>'),
    p(r(T('neg leftChars')), '<w:ind w:leftChars="-100"/>'),
  ].join('');
  cases.push({ name: 'm-charsind2', docx: Dl(body) });
}
