// WordCraft study, section A (text / paragraph / page): Word for Windows decides between our rules and WordCraft's
// (rules measured mostly in Word for Mac). Queue: core/target/wordcraft-study/QUEUE.md (rust-core worktree); results in
// core/target/wordcraft-study/A. Run with --fixdir there (not in the frozen TS corpus). Calibri 11 / A4 unless noted;
// invented texts; installed fonts referenced by NAME only (never embedded, never versioned).
import { buildDocx, docDefaults, p, r, sz, font, esc } from '../docx-builder.mjs';
import { createNodeFontProvider } from '../../corpus-lab/node-fonts.mjs';

const prov = await createNodeFontProvider();
const widthOf = (name, size) => {
  const f = prov.face(name, false, false)?.font;
  if (!f) return null;
  return (s) => [...s].reduce((a, ch) => a + Math.floor((f.advanceWidth(f.glyphId(ch.codePointAt(0))) * size * 4096) / f.metrics.unitsPerEm) / 4096 * 20, 0);
};
const TEXTW = 11906 - 2 * 1701;
const geo = (extra = '', after = '') => `<w:pgSz w:w="11906" w:h="16838"/><w:pgMar w:top="1417" w:right="1701" w:bottom="1417" w:left="1701" w:header="708" w:footer="708" w:gutter="0"/>${extra}<w:cols w:space="708"/>${after}<w:docGrid w:linePitch="360"/>`;
const sect = (before = '', extra = '', after = '') => `<w:sectPr>${before}${geo(extra, after)}</w:sectPr>`;
const D = (body, o = {}) => buildDocx({ body, defaults: docDefaults({ font: o.font ?? 'Calibri', size: o.size ?? 11, kern: o.kern }), ...o });
const lines = (n, label) => Array.from({ length: n }, (_, k) => `${label} ${k + 1} Hxg`).join('\n');
const filler = (n, label = 'enchimento') => Array.from({ length: n }, (_, k) => p(r(`${label} ${k + 1} Hxg`))).join('');
const tab = '<w:r><w:tab/></w:r>';
const R = (right) => `<w:ind w:left="0" w:right="${Math.round(right)}"/>`;
const fld = (instr, shown) => `<w:r><w:fldChar w:fldCharType="begin"/></w:r><w:r><w:instrText xml:space="preserve"> ${instr} </w:instrText></w:r><w:r><w:fldChar w:fldCharType="separate"/></w:r><w:r><w:t>${shown}</w:t></w:r><w:r><w:fldChar w:fldCharType="end"/></w:r>`;

export const cases = [];

// ── A1: compression of justified text with jc=distribute (jsweep's sweep; WordCraft: no compression for distribute)
{
  const w = widthOf('Calibri', 11);
  const g = w(' ');
  let body = '';
  for (const n of [4, 8, 16]) {
    const T = 'que';
    const base = n * (w('ab') + g);
    const W = w(T);
    const pred = Math.min(0.25 * n * g, (0.5 * n * (W + g)) / (n - 1 + 0.5 * n));
    for (let d = -3; d <= 3; d++) {
      const A = Math.round(base + W - pred) - d * 3;
      body += p(r(Array(n).fill('ab').join(' ') + ' ' + T + ' fim da linha seguinte'), `<w:jc w:val="distribute"/>${R(TEXTW - A)}`);
    }
    // and the plain control: no compression needed (fits with 10 tw to spare)
    body += p(r(Array(n).fill('ab').join(' ') + ' ' + T + ' fim da linha seguinte'), `<w:jc w:val="distribute"/>${R(TEXTW - Math.round(base + W + 10))}`);
  }
  cases.push({ name: 'a1-dist', docx: D(body) });
  // the same paragraphs justified "both" (control, jsweep's rule)
  cases.push({ name: 'a1-both', docx: D(body.replaceAll('w:val="distribute"', 'w:val="both"')) });
}

// ── A2: vertical metrics of CJK / other installed fonts (vmetrics covers 18 Latin fonts): pitch and first baseline
for (const f of ['SimSun', 'MS Gothic', 'Yu Gothic', 'Microsoft YaHei', 'Malgun Gothic', 'Segoe UI']) {
  let body = '';
  [10.5, 12, 24].forEach((s, i) => {
    body += p(r(lines(20, `Hxg ${s}`), sz(s)), i > 0 ? '<w:pageBreakBefore/>' : '');
  });
  const ff = `<w:rFonts w:ascii="${f}" w:hAnsi="${f}" w:eastAsia="${f}" w:cs="${f}"/>`;
  const slug = f.toLowerCase().replace(/\s+/g, '-');
  cases.push({ name: `a2-vm-${slug}`, docx: buildDocx({ body, defaults: docDefaults({ font: f, size: 11 }).replace(/<w:rFonts[^>]*\/>/, ff) }) });
  // with CJK text on the lines
  const cjk = p(r(Array.from({ length: 20 }, (_, k) => `Hxg 日本語 ${k}`).join('\n'), sz(12)));
  cases.push({ name: `a2-vmcjk-${slug}`, docx: buildDocx({ body: cjk, defaults: docDefaults({ font: f, size: 11, lang: 'ja-JP' }).replace(/<w:rFonts[^>]*\/>/, ff) }) });
}

// ── A5: line-break opportunities (UAX#14 in WordCraft): the token "aaaaaaaa<sep>bbbbbbbb" does not fit after "Lead "; a
// break after (or before) <sep> keeps part of it on line 1
{
  const w = widthOf('Calibri', 11);
  let body = '';
  const L = 'aaaaaaaa', B = 'bbbbbbbb';
  const after = ['?', '!', '…', '}', ')', ']', '—', '–', '-', '/', '%', ';', ':', ',', '.', '”', '\''];
  for (const s of after) {
    const A = w('Lead ' + L + s) + 20;
    body += p(r(`Lead ${L}${s}${B} fim`), R(TEXTW - A));
  }
  // before a digit
  for (const s of ['?', '!', '/', '-']) {
    const A = w('Lead ' + L + s) + 20;
    body += p(r(`Lead ${L}${s}12345678 fim`), R(TEXTW - A));
  }
  // break BEFORE the separator
  const before = ['—', '–', '(', '{', '[', '$', '…', '“'];
  for (const s of before) {
    const A = w('Lead ' + L) + 20;
    body += p(r(`Lead ${L}${s}${B} fim`), R(TEXTW - A));
  }
  cases.push({ name: 'a5-breaks', docx: D(body) });
}

// ── A7: exact line spacing: where the baseline sits in the line (all lines exact, so each line top is known)
{
  const EX = (tw) => `<w:spacing w:after="0" w:line="${tw}" w:lineRule="exact"/>`;
  const run = (t, f, s) => r(t, font(f) + sz(s));
  let body = '';
  for (const [f, s] of [['Calibri', 12], ['Times New Roman', 12], ['Arial', 12], ['Cambria', 12], ['Calibri', 24], ['SimSun', 12]]) {
    body += p(run(lines(3, `${f} ${s}`), f, s), EX(600));
  }
  body += p(run(lines(3, 'Calibri 12 em 10'), 'Calibri', 12), EX(200));
  body += p(run(lines(3, 'Times 12 em 10'), 'Times New Roman', 12), EX(200));
  cases.push({ name: 'a7-exact', docx: D(body) });
}

// ── A8: line grid (docGrid type=lines, pitch 18 pt): multiple and atLeast
{
  const grid = `<w:sectPr><w:pgSz w:w="11906" w:h="16838"/><w:pgMar w:top="1417" w:right="1701" w:bottom="1417" w:left="1701" w:header="708" w:footer="708" w:gutter="0"/><w:cols w:space="708"/><w:docGrid w:type="lines" w:linePitch="360"/></w:sectPr>`;
  const sp = (line, rule = 'auto') => `<w:spacing w:after="0" w:line="${line}" w:lineRule="${rule}"/>`;
  let body = '';
  body += p(r(lines(3, 'g11 single'), sz(11)), sp(240));
  body += p(r(lines(3, 'g24 x1.5'), sz(24)), sp(360));
  body += p(r(lines(3, 'g12 x0.8'), sz(12)), sp(192));
  body += p(r(lines(3, 'g11 x0.8'), sz(11)), sp(192));
  body += p(r(lines(3, 'g12 least30'), sz(12)), sp(600, 'atLeast'));
  body += p(r(lines(3, 'g24 least20'), sz(24)), sp(400, 'atLeast'));
  body += p(r(lines(3, 'g30 x1'), sz(30)), sp(240));
  body += p(r(lines(3, 'g12 x2'), sz(12)), sp(480));
  body += p(r(lines(3, 'g11 fim'), sz(11)), sp(240));
  cases.push({ name: 'a8-grid', docx: D(body, { sect: grid }) });
}

// ── A9: right / centre / decimal tab past the right margin (text width 16 cm, stop at 17 cm), modes 15 and 14
for (const compat of [15, 14]) {
  const sp = `<w:sectPr><w:pgSz w:w="11906" w:h="16838"/><w:pgMar w:top="1417" w:right="1417" w:bottom="1417" w:left="1417" w:header="708" w:footer="708" w:gutter="0"/><w:cols w:space="708"/><w:docGrid w:linePitch="360"/></w:sectPr>`;
  const T = (val, leader = 'dot') => `<w:tabs><w:tab w:val="${val}" w:leader="${leader}" w:pos="9638"/></w:tabs>`;
  let body = '';
  body += p(r('Item direita') + tab + r('12'), T('right'));
  body += p(r('Item direita sem guia') + tab + r('12'), T('right', 'none'));
  body += p(r('Item centro') + tab + r('12'), T('center'));
  body += p(r('Item decimal') + tab + r('12,5'), T('decimal'));
  body += p(r('Item esquerda') + tab + r('12'), T('left'));
  body += p(r('Fim Hxg'));
  cases.push({ name: `a9-tabpast-c${compat}`, docx: D(body, { sect: sp, compat }) });
}

// ── A11: super/subscript raise and size (supraise covers the raise; this adds Calibri/Times 11/15/25, x of the glyphs)
{
  let body = '';
  for (const f of ['Arial', 'Calibri', 'Times New Roman']) {
    for (const s of [11, 15, 25]) {
      body += p(r('x', font(f) + sz(s)) + r('2', font(f) + sz(s) + '<w:vertAlign w:val="superscript"/>') + r(' y', font(f) + sz(s)) + r('2', font(f) + sz(s) + '<w:vertAlign w:val="subscript"/>') + r(' Hxg', font(f) + sz(s)));
    }
  }
  cases.push({ name: 'a11-supsub', docx: D(body) });
}

// ── A12: small caps size (x of each glyph)
{
  let body = '';
  for (const [f, s] of [['Calibri', 11], ['Times New Roman', 11], ['Arial', 11], ['Calibri', 9], ['Calibri', 13], ['Arial', 15]]) {
    body += p(r('Ab cd Ef gh ij', font(f) + sz(s) + '<w:smallCaps/>') + r(' Hxg', font(f) + sz(s)));
  }
  cases.push({ name: 'a12-smallcaps', docx: D(body) });
}

// ── A14: auto (HTML) spacing between list items: same list across levels
{
  const AUTO = '<w:spacing w:before="100" w:beforeAutospacing="1" w:after="100" w:afterAutospacing="1"/>';
  const lvl = (i, fmt, text) => `<w:lvl w:ilvl="${i}"><w:start w:val="1"/><w:numFmt w:val="${fmt}"/><w:lvlText w:val="${text}"/><w:lvlJc w:val="left"/><w:pPr><w:ind w:left="${720 + 360 * i}" w:hanging="360"/></w:pPr></w:lvl>`;
  const numbering = `<w:abstractNum w:abstractNumId="1">${lvl(0, 'decimal', '%1)')}${lvl(1, 'lowerLetter', '%2)')}</w:abstractNum><w:abstractNum w:abstractNumId="2">${lvl(0, 'decimal', '%1.')}</w:abstractNum>`
    + '<w:num w:numId="1"><w:abstractNumId w:val="1"/></w:num><w:num w:numId="2"><w:abstractNumId w:val="2"/></w:num><w:num w:numId="3"><w:abstractNumId w:val="1"/><w:lvlOverride w:ilvl="0"><w:startOverride w:val="1"/></w:lvlOverride></w:num>';
  const it = (t, n, l = 0) => p(r(t), `${AUTO}<w:numPr><w:ilvl w:val="${l}"/><w:numId w:val="${n}"/></w:numPr>`);
  const body = it('um Hxg', 1) + it('sub a Hxg', 1, 1) + it('sub b Hxg', 1, 1) + it('dois Hxg', 1) + it('outra lista Hxg', 2) + it('outra dois Hxg', 2) + it('restart Hxg', 3) + it('restart dois Hxg', 3);
  cases.push({ name: 'a14-autolevels', docx: buildDocx({ body, numbering, defaults: docDefaults({ font: 'Calibri', size: 11, after: 160, line: 259 }) }) });
}

// ── A15: space before of a paragraph that starts on a new page because of a page break
{
  const P1 = (extra = '') => p(r('Primeiro Hxg') + extra, '<w:spacing w:before="0" w:after="200"/>');
  const BR = '<w:r><w:br w:type="page"/></w:r>';
  cases.push({ name: 'a15-br-in-p2', docx: D(P1() + p(BR + r('Segundo Hxg'), '<w:spacing w:before="480" w:after="0"/>') + p(r('Terceiro Hxg'))) });
  cases.push({ name: 'a15-br-in-p2-b6', docx: D(P1() + p(BR + r('Segundo Hxg'), '<w:spacing w:before="120" w:after="0"/>') + p(r('Terceiro Hxg'))) });
  cases.push({ name: 'a15-br-end-p1', docx: D(P1(BR) + p(r('Segundo Hxg'), '<w:spacing w:before="480" w:after="0"/>') + p(r('Terceiro Hxg'))) });
  cases.push({ name: 'a15-pbb', docx: D(P1() + p(r('Segundo Hxg'), '<w:pageBreakBefore/><w:spacing w:before="480" w:after="0"/>') + p(r('Terceiro Hxg'))) });
  cases.push({ name: 'a15-br-alone', docx: D(P1() + p(BR) + p(r('Segundo Hxg'), '<w:spacing w:before="480" w:after="0"/>') + p(r('Terceiro Hxg'))) });
  cases.push({ name: 'a15-br-in-p2-c14', docx: D(P1() + p(BR + r('Segundo Hxg'), '<w:spacing w:before="480" w:after="0"/>') + p(r('Terceiro Hxg')), { compat: 14 }) });
}

// ── A16: odd/even section break with a restarted page number (PAGE in the header shows the number)
{
  const hdr = p(r('Cab ') + fld('PAGE', '1'));
  const S = (type, start, more = '') => `<w:sectPr>${type ? `<w:type w:val="${type}"/>` : ''}<w:pgSz w:w="11906" w:h="16838"/><w:pgMar w:top="1417" w:right="1701" w:bottom="1417" w:left="1701" w:header="708" w:footer="708" w:gutter="0"/>${start !== undefined ? `<w:pgNumType w:start="${start}"/>` : ''}<w:cols w:space="708"/>${more}<w:docGrid w:linePitch="360"/></w:sectPr>`;
  for (const [type, start] of [['oddPage', 2], ['oddPage', 3], ['oddPage', 1], ['evenPage', 1], ['evenPage', 2], ['oddPage', undefined]]) {
    for (const eo of [false, true]) {
      const body = p(r('S1 pagina Hxg')) + p(r('Fim S1 Hxg'), S(undefined, undefined)) + p(r('S2 texto Hxg'));
      const name = `a16-${type}-s${start ?? 'none'}${eo ? '-eo' : ''}`;
      cases.push({ name, docx: buildDocx({ body, sect: S(type, start), headers: eo ? { default: hdr, even: p(r('Par ') + fld('PAGE', '1')) } : { default: hdr }, settings: eo ? '<w:evenAndOddHeaders/>' : '', defaults: docDefaults({ font: 'Calibri', size: 11 }) }) });
    }
  }
}

// ── A18: three balanced columns in a continuous section, with vAlign=center on the column section
{
  const lens = [12, 30, 7, 45, 20, 9, 33, 15, 26, 5, 40, 18, 11, 28, 8, 36, 14, 22, 6, 31];
  const paras = lens.map((n, i) => p(r(`P${i + 1} ` + 'texto curto de enchimento '.repeat(Math.ceil(n / 4)).split(' ').slice(0, n).join(' ')))).join('');
  for (const va of ['', 'center', 'bottom']) {
    const s1 = '<w:p><w:pPr><w:sectPr><w:pgSz w:w="11906" w:h="16838"/><w:pgMar w:top="1417" w:right="1701" w:bottom="1417" w:left="1701" w:header="708" w:footer="708" w:gutter="0"/><w:cols w:space="708"/><w:docGrid w:linePitch="360"/></w:sectPr></w:pPr><w:r><w:t>Inicio Hxg</w:t></w:r></w:p>';
    const s2 = `<w:p><w:pPr><w:sectPr><w:type w:val="continuous"/><w:pgSz w:w="11906" w:h="16838"/><w:pgMar w:top="1417" w:right="1701" w:bottom="1417" w:left="1701" w:header="708" w:footer="708" w:gutter="0"/><w:cols w:num="3" w:space="708"/>${va ? `<w:vAlign w:val="${va}"/>` : ''}<w:docGrid w:linePitch="360"/></w:sectPr></w:pPr><w:r><w:t>Ultimo Hxg</w:t></w:r></w:p>`;
    const fin = `<w:sectPr><w:type w:val="continuous"/><w:pgSz w:w="11906" w:h="16838"/><w:pgMar w:top="1417" w:right="1701" w:bottom="1417" w:left="1701" w:header="708" w:footer="708" w:gutter="0"/><w:cols w:space="708"/>${va ? `<w:vAlign w:val="${va}"/>` : ''}<w:docGrid w:linePitch="360"/></w:sectPr>`;
    cases.push({ name: `a18-cols3${va ? '-' + va : ''}`, docx: D(s1 + paras + s2 + p(r('Depois Hxg')), { sect: fin }) });
  }
}

// ── A19: jc=distribute: where the extra space goes (x of each glyph)
{
  const DI = '<w:jc w:val="distribute"/>';
  const ja = '<w:rFonts w:ascii="MS Gothic" w:hAnsi="MS Gothic" w:eastAsia="MS Gothic"/>';
  let body = '';
  body += p(r('ab cd'), DI);
  body += p(r('abcd'), DI);
  body += p(r('ab cd ef gh'), DI);
  body += p(r('あいうえお', ja + '<w:lang w:eastAsia="ja-JP"/>'), DI);
  body += p(r('日本 語', ja + '<w:lang w:eastAsia="ja-JP"/>'), DI);
  body += p(r('ab cd'), '<w:jc w:val="both"/>');
  cases.push({ name: 'a19-distribute', docx: D(body) });
}

// ── A20: positional tabs relative to margin / indent, and a ptab whose target is left of the text
{
  const PT = (al, rel) => `<w:r><w:ptab w:relativeTo="${rel}" w:alignment="${al}" w:leader="none"/></w:r>`;
  const IND = '<w:ind w:left="1440" w:right="1440"/>';
  let body = '';
  for (const rel of ['margin', 'indent']) {
    for (const al of ['center', 'right']) body += p(r('Esq') + PT(al, rel) + r('Alvo'), IND);
  }
  body += p(r('Um texto bem comprido que passa do centro da coluna sim') + PT('center', 'margin') + r('Alvo centro'));
  body += p(r('Um texto bem comprido que passa do centro da coluna e vai quase ate a margem direita') + PT('right', 'margin') + r('Alvo direita'));
  body += p(r('Esq') + PT('left', 'margin') + r('Alvo esquerda'), IND);
  cases.push({ name: 'a20-ptab', docx: D(body) });
}

// ── A21: number suffix: the number ends exactly ON the stop (MS Gothic 12: "1." = 240 tw); space suffix width
{
  const lvl = (ind, suff = '', f = '') => `<w:lvl w:ilvl="0"><w:start w:val="1"/><w:numFmt w:val="decimal"/>${suff}<w:lvlText w:val="%1."/><w:lvlJc w:val="left"/><w:pPr><w:ind w:left="${ind}" w:hanging="${ind}"/></w:pPr>${f}</w:lvl>`;
  const inds = [238, 239, 240, 241, 242];
  const numbering = inds.map((ind, i) => `<w:abstractNum w:abstractNumId="${i + 1}">${lvl(ind)}</w:abstractNum>`).join('')
    + inds.map((_, i) => `<w:num w:numId="${i + 1}"><w:abstractNumId w:val="${i + 1}"/></w:num>`).join('');
  const body = inds.map((ind, i) => p(r(`texto ${ind} Hxg`), `<w:numPr><w:ilvl w:val="0"/><w:numId w:val="${i + 1}"/></w:numPr>`)).join('');
  const g = '<w:rFonts w:ascii="MS Gothic" w:hAnsi="MS Gothic" w:eastAsia="MS Gothic" w:cs="MS Gothic"/>';
  cases.push({ name: 'a21-numstop', docx: buildDocx({ body, numbering, defaults: docDefaults({ font: 'MS Gothic', size: 12 }).replace(/<w:rFonts[^>]*\/>/, g) }) });
  // space suffix in Arial / Calibri / Times 11 and 20
  const fonts = [['Arial', 11], ['Calibri', 11], ['Times New Roman', 11], ['Arial', 20]];
  const numbering2 = fonts.map(([f, s], i) => `<w:abstractNum w:abstractNumId="${i + 1}"><w:lvl w:ilvl="0"><w:start w:val="1"/><w:numFmt w:val="decimal"/><w:suff w:val="space"/><w:lvlText w:val="%1."/><w:lvlJc w:val="left"/><w:pPr><w:ind w:left="720" w:hanging="360"/></w:pPr><w:rPr>${font(f)}${sz(s)}</w:rPr></w:lvl></w:abstractNum>`).join('')
    + fonts.map((_, i) => `<w:num w:numId="${i + 1}"><w:abstractNumId w:val="${i + 1}"/></w:num>`).join('');
  const body2 = fonts.map(([f, s], i) => p(r(`texto Hxg`, font(f) + sz(s)), `<w:numPr><w:ilvl w:val="0"/><w:numId w:val="${i + 1}"/></w:numPr>`)).join('');
  cases.push({ name: 'a21-space', docx: buildDocx({ body: body2, numbering: numbering2, defaults: docDefaults({ font: 'Calibri', size: 11 }) }) });
}

// ── A22: default tab stop from a start LEFT of the margin (negative indent)
{
  let body = '';
  for (const ind of [-1440, -1000, -708, -300]) body += p(tab + r('X Hxg'), `<w:ind w:left="${ind}"/>`);
  for (const ind of [-1440, -1000]) body += p(r('ab') + tab + r('X Hxg'), `<w:ind w:left="${ind}"/>`);
  cases.push({ name: 'a22-negtab', docx: D(body) });
}

// ── A23: bar tabs are not stops
{
  let body = '';
  body += p(r('a') + tab + r('b Hxg'), '<w:tabs><w:tab w:val="bar" w:pos="2000"/><w:tab w:val="left" w:pos="4000"/></w:tabs>');
  body += p(r('a') + tab + r('b Hxg'), '<w:tabs><w:tab w:val="bar" w:pos="2000"/></w:tabs>');
  body += p(r('a') + tab + r('b') + tab + r('c Hxg'), '<w:tabs><w:tab w:val="left" w:pos="1000"/><w:tab w:val="bar" w:pos="2000"/><w:tab w:val="right" w:pos="5000"/></w:tabs>');
  cases.push({ name: 'a23-bartab', docx: D(body) });
}

// ── A24: which spaces make a line taller: a 20 pt U+0020 / NBSP / U+3000 / U+2002 / tab in 9 pt text
{
  let body = '';
  const big = (t) => `<w:r><w:rPr>${sz(20)}</w:rPr><w:t xml:space="preserve">${t}</w:t></w:r>`;
  for (const [k, t] of [['sp', ' '], ['nbsp', ' '], ['ideo', '　'], ['en', ' '], ['tab', null]]) {
    body += p(r(`antes ${k}`, sz(9)));
    body += p(r('bbb', sz(9)) + (t === null ? `<w:r><w:rPr>${sz(20)}</w:rPr><w:tab/></w:r>` : big(t)) + r('ccc', sz(9)));
    body += p(r(`depois ${k}`, sz(9)));
  }
  cases.push({ name: 'a24-spaces', docx: D(body, { size: 9 }) });
}

// ── A27: body bottom raised by a tall footer; multiple spacing at the bottom with it
{
  const tall = Array.from({ length: 8 }, (_, k) => p(r(`rodape ${k + 1}`))).join('');
  cases.push({ name: 'a27-tallfooter', docx: D(filler(70), { footers: { default: tall } }) });
  const m15 = Array.from({ length: 50 }, (_, k) => p(r(`linha ${k + 1} Hxg`), '<w:spacing w:after="0" w:line="360" w:lineRule="auto"/>')).join('');
  cases.push({ name: 'a27-tallfooter-m15', docx: D(m15, { footers: { default: tall } }) });
  cases.push({ name: 'a27-m15', docx: D(m15) });
}

// ── A28: body top below a tall EVEN header (evenAndOddHeaders)
{
  const tallH = Array.from({ length: 8 }, (_, k) => p(r(`cab par ${k + 1}`))).join('');
  cases.push({ name: 'a28-evenhdr', docx: D(filler(130), { headers: { default: p(r('cab impar')), even: tallH }, settings: '<w:evenAndOddHeaders/>' }) });
  cases.push({ name: 'a28-firsthdr', docx: D(filler(130), { headers: { default: p(r('cab padrao')), first: tallH }, sect: sect('', '', '<w:titlePg/>') }) });
}

// ── Rules WordCraft has and we lack: does Word apply them?
{
  const ja = '<w:rFonts w:ascii="MS Gothic" w:hAnsi="MS Gothic" w:eastAsia="MS Gothic"/><w:lang w:eastAsia="ja-JP"/>';
  // character grid (linesAndChars, charSpace)
  const cg = (type, cs) => `<w:sectPr><w:pgSz w:w="11906" w:h="16838"/><w:pgMar w:top="1417" w:right="1701" w:bottom="1417" w:left="1701" w:header="708" w:footer="708" w:gutter="0"/><w:cols w:space="708"/><w:docGrid w:type="${type}" w:linePitch="360" w:charSpace="${cs}"/></w:sectPr>`;
  const cgBody = p(r('日本語の文字列です', ja + sz(10.5))) + p(r('Latin text Hxg', sz(10.5))) + p(r('混合 mixed 文字', ja + sz(10.5)));
  cases.push({ name: 'r-chargrid-lc', docx: D(cgBody, { sect: cg('linesAndChars', 4096) }) });
  cases.push({ name: 'r-chargrid-snap', docx: D(cgBody, { sect: cg('snapToChars', 4096) }) });
  cases.push({ name: 'r-chargrid-none', docx: D(cgBody) });
  // autoSpaceDE / autoSpaceDN (default on)
  const mix = r('日本語abc日本語123日本', ja + sz(10.5));
  cases.push({ name: 'r-autospace', docx: D(p(mix) + p(mix, '<w:autoSpaceDE w:val="0"/><w:autoSpaceDN w:val="0"/>')) });
  // mirrorIndents
  const mi = p(r(lines(2, 'espelho impar')), '<w:mirrorIndents/><w:ind w:left="2000" w:right="400"/>') + p(r(lines(2, 'espelho par')), '<w:pageBreakBefore/><w:mirrorIndents/><w:ind w:left="2000" w:right="400"/>');
  cases.push({ name: 'r-mirrorind', docx: D(mi) });
  // gutter (+ mirror margins)
  const gut = (mm) => `<w:sectPr><w:pgSz w:w="11906" w:h="16838"/><w:pgMar w:top="1417" w:right="1701" w:bottom="1417" w:left="1701" w:header="708" w:footer="708" w:gutter="1440"/><w:cols w:space="708"/><w:docGrid w:linePitch="360"/></w:sectPr>`;
  const twoPages = p(r('pagina um Hxg')) + p(r('pagina dois Hxg'), '<w:pageBreakBefore/>');
  cases.push({ name: 'r-gutter', docx: D(twoPages, { sect: gut() }) });
  cases.push({ name: 'r-gutter-mirror', docx: D(twoPages, { sect: gut(), settings: '<w:mirrorMargins/>' }) });
  // firstLineChars / leftChars
  cases.push({ name: 'r-chars-ind', docx: D(p(r(lines(2, 'primeira 200')), '<w:ind w:firstLineChars="200" w:firstLine="420"/>') + p(r(lines(2, 'esquerda 300')), '<w:ind w:leftChars="300" w:left="630"/>') + p(r(lines(2, 'jp primeira'), ja + sz(10.5)), '<w:ind w:firstLineChars="200" w:firstLine="420"/>')) });
  // doNotExpandShiftReturn
  const sr = p(r('Uma linha justificada curta') + '<w:r><w:br/></w:r>' + r('segunda linha depois da quebra manual Hxg'), '<w:jc w:val="both"/>');
  cases.push({ name: 'r-shiftret', docx: D(sr) });
  cases.push({ name: 'r-shiftret-dne', docx: D(sr, { extraCompat: '<w:doNotExpandShiftReturn/>' }) });
  // emphasis marks and line height
  cases.push({ name: 'r-emph', docx: D(p(r('antes Hxg')) + p(r('強調の点 Hxg', ja + '<w:em w:val="dot"/>')) + p(r('meio Hxg')) + p(r('ênfase latina Hxg', '<w:em w:val="dot"/>')) + p(r('depois Hxg'))) });
  // kinsoku: small kana / prolonged sound mark / ideographic comma at the start of a line (MS Gothic 12: 10 per line)
  const kin = (s) => p(r(s, ja + sz(12)), '<w:ind w:left="0" w:right="' + (TEXTW - 2400) + '"/>');
  cases.push({ name: 'r-kinsoku', docx: D(kin('ああああああああああゃああ') + kin('ああああああああああーああ') + kin('ああああああああああ、ああ') + kin('ああああああああああっああ') + kin('ああああああああああァああ')) });
}
