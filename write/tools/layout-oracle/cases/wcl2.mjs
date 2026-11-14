// WordCraft study, planner "L", second batch (see wcl.mjs): the "rules WordCraft has and we lack" of the line area —
// character grid (linesAndChars / snapToChars), firstLineChars & co., doNotExpandShiftReturn, emphasis marks, kinsoku —
// and the CJK side of A19 (distribute). MS Gothic / SimSun / Yu Gothic referenced by NAME only. Run with
// --fixdir core/target/wordcraft-study/L/wcl2.
import { buildDocx, docDefaults, p, r, sz, font } from '../docx-builder.mjs';

const TEXTW = 11906 - 2 * 1701;
const ind = (left, right, extra = '') => `<w:ind w:left="${Math.round(left)}" w:right="${Math.round(right)}"${extra}/>`;
const ea = (f, lang = 'ja-JP') => `<w:rFonts w:ascii="${f}" w:hAnsi="${f}" w:eastAsia="${f}" w:cs="${f}"/><w:lang w:eastAsia="${lang}"/>`;
const eaOnly = (f, lang = 'ja-JP') => `<w:rFonts w:eastAsia="${f}"/><w:lang w:eastAsia="${lang}"/>`;
// Japanese-style defaults: Latin Calibri, East Asian MS Gothic, East Asian language ja-JP
const jaDefaults = (size, eaFont = 'MS Gothic') => docDefaults({ font: 'Calibri', size })
  .replace(/<w:rFonts[^>]*\/>/, `<w:rFonts w:ascii="Calibri" w:hAnsi="Calibri" w:eastAsia="${eaFont}" w:cs="Calibri"/>`)
  .replace('w:eastAsia="en-US"', 'w:eastAsia="ja-JP"');
const sect = (grid = '<w:docGrid w:linePitch="360"/>') => `<w:sectPr><w:pgSz w:w="11906" w:h="16838"/><w:pgMar w:top="1417" w:right="1701" w:bottom="1417" w:left="1701" w:header="708" w:footer="708" w:gutter="0"/><w:cols w:space="708"/>${grid}</w:sectPr>`;
const D = (body, o = {}) => buildDocx({ body, defaults: o.defaults ?? jaDefaults(o.size ?? 10.5), ...o });

const CJK30 = 'あいうえおかきくけこさしすせそたちつてとなにぬねのはひふへほ';
const LAT = 'The quick brown fox jumps over the lazy dog and keeps running far away';

export const cases = [];

// ── character grid: type × charSpace × default size; paragraphs of CJK, Latin, mixed text at several sizes
{
  for (const base of [10.5, 11]) {
    const twice = base * 2;
    const body = [
      p(r(CJK30 + CJK30)),
      p(r(LAT + ' ' + LAT)),
      p(r('あいう abc えお 12 かき Hxg くけこ mixed 文字列 end')),
      p(r(CJK30, sz(twice))),
      p(r(CJK30, sz(8))),
      p(r(LAT, sz(twice))),
      p(r(CJK30 + CJK30), '<w:snapToGrid w:val="0"/>'),
      p(r(CJK30.slice(0, 10)) + r(CJK30.slice(10, 20), '<w:snapToGrid w:val="0"/>') + r(CJK30.slice(20))),
      p(r(CJK30 + CJK30), '<w:jc w:val="both"/>'),
      p(r(LAT + ' ' + LAT), '<w:jc w:val="both"/>'),
      p(r('あいう'), '<w:jc w:val="center"/>'),
      p(r('abc'), '<w:jc w:val="center"/>'),
      p(r('あいう abc'), '<w:jc w:val="right"/>'),
      p(r(CJK30), ind(420, 0)),
      p(r('Hxg ' + CJK30.slice(0, 5), sz(base) + '<w:spacing w:val="40"/>')),
    ].join('');
    for (const type of ['linesAndChars', 'snapToChars']) {
      for (const cs of [0, 4096, -2048, 2048, 20480]) {
        const grid = `<w:docGrid w:type="${type}" w:linePitch="360" w:charSpace="${cs}"/>`;
        cases.push({ name: `l-cgrid-${type === 'linesAndChars' ? 'lc' : 'sc'}-${String(cs).replace('-', 'm')}-${String(base).replace('.', '_')}`, docx: D(body, { size: base, sect: sect(grid) }) });
      }
    }
    cases.push({ name: `l-cgrid-none-${String(base).replace('.', '_')}`, docx: D(body, { size: base }) });
    cases.push({ name: `l-cgrid-lines-${String(base).replace('.', '_')}`, docx: D(body, { size: base, sect: sect('<w:docGrid w:type="lines" w:linePitch="360"/>') }) });
  }
}

// ── firstLineChars / leftChars / hangingChars / rightChars: the unit (which size), priority over the twips value
{
  const T = (s) => 'linha um do paragrafo Hxg ' + s + ' e a segunda linha que continua bastante para quebrar com folga no fim da coluna aqui';
  const body = [
    p(r(T('fl200')), '<w:ind w:firstLineChars="200" w:firstLine="420"/>'),
    p(r(T('fl100')), '<w:ind w:firstLineChars="100"/>'),
    p(r(T('fl150')), '<w:ind w:firstLineChars="150" w:firstLine="1000"/>'),
    p(r(T('fl0')), '<w:ind w:firstLineChars="0" w:firstLine="420"/>'),
    p(r(T('left300')), '<w:ind w:leftChars="300" w:left="630"/>'),
    p(r(T('hang200')), '<w:ind w:left="1440" w:hangingChars="200" w:hanging="300"/>'),
    p(r(T('leftc300 hang')), '<w:ind w:leftChars="300" w:left="630" w:hangingChars="100" w:hanging="210"/>'),
    p(r(T('right400')), '<w:jc w:val="right"/><w:ind w:rightChars="400" w:right="100"/>'),
    // the size: run 20 / mark 11; run 11 / mark 20; first run 8 then 20; CJK 10.5
    p(r(T('run20'), sz(20)), '<w:ind w:firstLineChars="200"/>'),
    p(r(T('mark20')), '<w:ind w:firstLineChars="200"/><w:rPr>' + sz(20) + '</w:rPr>'),
    p(r('pequeno ', sz(8)) + r(T('then20'), sz(20)), '<w:ind w:firstLineChars="200"/>'),
    p(r(T('left run20'), sz(20)), '<w:ind w:leftChars="200"/>'),
    p(r('日本語の文字列です日本語の文字列です', ea('MS Gothic') + sz(10.5)), '<w:ind w:firstLineChars="200"/>'),
    p(r('日本語の文字列です日本語の文字列です', ea('MS Gothic') + sz(21)), '<w:ind w:firstLineChars="200"/>'),
  ].join('');
  // Latin defaults (Calibri 11, en-US East Asian) and the Japanese defaults (10.5); with a character grid
  cases.push({ name: 'l-charsind-lat', docx: D(body, { defaults: docDefaults({ font: 'Calibri', size: 11 }) }) });
  cases.push({ name: 'l-charsind-ja', docx: D(body, { size: 10.5 }) });
  cases.push({ name: 'l-charsind-grid', docx: D(body, { size: 10.5, sect: sect('<w:docGrid w:type="linesAndChars" w:linePitch="360" w:charSpace="4096"/>') }) });
  // Normal style with another size than the defaults
  cases.push({ name: 'l-charsind-normal', docx: D(body, { size: 10.5, normal: `<w:rPr>${sz(14)}</w:rPr>` }) });
}

// ── doNotExpandShiftReturn × jc × compatibility mode
{
  const line = (jc) => p(r('Uma linha ' + jc + ' curta') + '<w:r><w:br/></w:r>' + r('segunda linha depois da quebra manual Hxg') + '<w:r><w:br/></w:r>' + r('fim'), `<w:jc w:val="${jc}"/>`);
  const body = ['both', 'distribute', 'left', 'center'].map(line).join('')
    // a line that WRAPS before the w:br (the br line is the second one)
    + p(r('texto longo que enche a primeira linha inteira e continua um pouco mais adiante para quebrar sozinho aqui e ali') + '<w:r><w:br/></w:r>' + r('depois'), '<w:jc w:val="both"/>');
  const defaults = docDefaults({ font: 'Calibri', size: 11 });
  for (const c of [15, 14]) {
    cases.push({ name: `l-shiftret-c${c}`, docx: D(body, { defaults, compat: c }) });
    cases.push({ name: `l-shiftret-dne-c${c}`, docx: D(body, { defaults, compat: c, extraCompat: '<w:doNotExpandShiftReturn/>' }) });
  }
}

// ── emphasis marks: kind × font/size × CJK/Latin, the line pitch around and the marks' place
{
  const filler = (k) => p(r(`ref ${k} Hxg`));
  let body = filler(0);
  let k = 1;
  const runs = [
    ['lat-cal11', 'enfase latina Hxg', ''],
    ['lat-cal22', 'enfase grande Hxg', sz(22)],
    ['lat-arial11', 'enfase arial Hxg', font('Arial')],
    ['ja-msg10', '強調の点です', ea('MS Gothic') + sz(10.5)],
    ['ja-msg21', '強調の点です', ea('MS Gothic') + sz(21)],
    ['ja-yu11', '強調の点です', ea('Yu Gothic') + sz(11)],
    ['ja-msg-calibri', '強調abc', eaOnly('MS Gothic') + font('Calibri')],
  ];
  for (const em of ['dot', 'comma', 'circle', 'underDot']) {
    for (const [, t, rpr] of runs) {
      body += p(r(t, rpr + `<w:em w:val="${em}"/>`));
      body += filler(k++);
    }
  }
  // one emphasised character in a line; exact line spacing; multiple 2.0
  body += p(r('so um ') + r('x', '<w:em w:val="dot"/>') + r(' aqui Hxg')) + filler(k++);
  body += p(r('exata enfase Hxg', '<w:em w:val="dot"/>'), '<w:spacing w:line="300" w:lineRule="exact"/>') + filler(k++);
  body += p(r('dupla enfase Hxg', '<w:em w:val="dot"/>'), '<w:spacing w:line="480" w:lineRule="auto"/>') + filler(k++);
  body += p(r('linha um com enfase que segue e segue ate quebrar a linha no fim da coluna e ir para a segunda linha', '<w:em w:val="dot"/>')) + filler(k++);
  cases.push({ name: 'l-emph', docx: D(body, { defaults: jaDefaults(11) }) });
  cases.push({ name: 'l-emph-latdef', docx: D(body, { defaults: docDefaults({ font: 'Calibri', size: 11 }) }) });
}

// ── kinsoku: may character c START a line (10 × 12 pt fill the line, then c)? may it END one (9 + c)?
{
  const noStart = '!%),.:;?]}¢°’”‰′″℃、。々〉》」』】〕゛゜ゝゞ・ヽヾ！％），．：；？］｝｡｣､･ﾞﾟ￠ぁぃぅぇぉっゃゅょゎァィゥェォッャュョヮヵヶー‐゠–〜‼⁇⁈⁉〻ヿ…‥〙〗｠»';
  const noEnd = '$([{£¥‘“〈《「『【〔＄（［｛｢￡￥〝«〘〖｟';
  const W = TEXTW - 2400; // 120 pt: ten 12 pt ideographs
  for (const [lang, f] of [['ja-JP', 'MS Gothic'], ['zh-CN', 'SimSun']]) {
    let body = '';
    for (const c of noStart) body += p(r('ああああああああああ' + c + 'ああ', ea(f, lang) + sz(12)), ind(0, W));
    for (const c of noEnd) body += p(r('あああああああああ' + c + 'あああ', ea(f, lang) + sz(12)), ind(0, W));
    cases.push({ name: `l-kinsoku-${lang.toLowerCase()}`, docx: D(body, { size: 12 }) });
  }
  // switches: kinsoku off, justified (hanging), mode 14, no East Asian language
  let b2 = '';
  for (const c of '、。」）') {
    b2 += p(r('ああああああああああ' + c + 'ああ', ea('MS Gothic') + sz(12)), ind(0, W) + '<w:kinsoku w:val="0"/>');
    b2 += p(r('ああああああああああ' + c + 'ああ', ea('MS Gothic') + sz(12)), ind(0, W) + '<w:jc w:val="both"/>');
    b2 += p(r('ああああああああああ' + c + 'ああ', ea('MS Gothic', 'en-US') + sz(12)), ind(0, W));
    b2 += p(r('ああああああああああ' + c + 'ああ', ea('MS Gothic') + sz(12)), ind(0, W) + '<w:overflowPunct w:val="0"/>');
  }
  cases.push({ name: 'l-kinsoku-sw', docx: D(b2, { size: 12 }) });
  cases.push({ name: 'l-kinsoku-sw-c14', docx: D(b2, { size: 12, compat: 14 }) });
  cases.push({ name: 'l-kinsoku-sw-latdef', docx: D(b2, { defaults: docDefaults({ font: 'Calibri', size: 12 }) }) });
}

// ── A19 on CJK text: distribute, slack swept, last line and non-last line
{
  const slacks = [1, 3, 6, 12, 24, 48, 100];
  let body = '';
  for (const t of ['あいうえお', 'あいう えお', 'あいうえおかきくけこ']) {
    const nat = [...t].reduce((a, c) => a + (c === ' ' ? 6 : 12), 0); // MS Gothic 12: 12 / 6 pt
    for (const ctx of ['last', 'mid']) {
      for (const sl of slacks) {
        const A = Math.round((nat + sl) * 20);
        // non-last: an unbreakable Latin word follows (no break inside it; a break before it, after the ideograph)
        body += p(r(ctx === 'last' ? t : t + 'X'.repeat(90), ea('MS Gothic') + sz(12)), `<w:jc w:val="distribute"/>${ind(0, TEXTW - A)}`);
      }
    }
  }
  cases.push({ name: 'l-jspread-cjk', docx: D(body, { size: 12 }) });
}
