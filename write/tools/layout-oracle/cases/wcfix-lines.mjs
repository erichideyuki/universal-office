// WordCraft study, area LINES (plan core/target/wordcraft-study/PLAN-L.md, record core/docs/modules/uo-docx-layout.md
// "WordCraft study — lines"): the Word fixtures of core/crates/uo-docx-layout/tests/word_wc_lines.rs. OPEN fonts only
// (Liberation Sans/Serif, Carlito, Caladea) — or the family NAMES Arial/Calibri/Times New Roman/Cambria where a rule is
// keyed by the family (Word draws its own fonts, the test maps the names to the metric twins; nothing embedded).
// Each case name starts with its group: run with
//   node --import file:///<wt>/core/target/wordcraft-study/A/resolve-hook.mjs write/tools/layout-oracle/run.mjs \
//     wcfix-lines --fixdir core/target/wc-lines/fix
// then the .wordx.json (glyph origins) with core/target/wordcraft-study/A/extract_xs.py over .out/wcfix-lines/pdf, and
// each <group>-*.{docx,word.json,wordx.json} moved to core/crates/uo-docx-layout/tests/oracle_wc/l-<group>/.
import { buildDocx, docDefaults, p, r, sz, font, esc } from '../docx-builder.mjs';
import { createNodeFontProvider } from '../../corpus-lab/node-fonts.mjs';

const prov = await createNodeFontProvider();
const widthOf = (name, size) => {
  const f = prov.face(name, false, false)?.font;
  if (!f) throw new Error('font ' + name);
  return (s) => [...s].reduce((a, ch) => a + Math.floor((f.advanceWidth(f.glyphId(ch.codePointAt(0))) * size * 4096) / f.metrics.unitsPerEm) / 4096 * 20, 0);
};
const TEXTW = 11906 - 2 * 1701;
const D = (body, o = {}) => buildDocx({ body, defaults: docDefaults({ font: o.font ?? 'Carlito', size: o.size ?? 11 }), ...o });
const ind = (left, right, extra = '') => `<w:ind w:left="${Math.round(left)}" w:right="${Math.round(right)}"${extra}/>`;
const tabsXml = (list) => `<w:tabs>${list.map(([v, pos]) => `<w:tab w:val="${v}" w:pos="${pos}"/>`).join('')}</w:tabs>`;
const TAB = '<w:r><w:tab/></w:r>';
const sect = (grid) => `<w:sectPr><w:pgSz w:w="11906" w:h="16838"/><w:pgMar w:top="1417" w:right="1701" w:bottom="1417" w:left="1701" w:header="708" w:footer="708" w:gutter="0"/><w:cols w:space="708"/>${grid}</w:sectPr>`;

export const cases = [];

// ── L1 (spaceh): which space characters make the line taller — 9 pt text, one 20 pt character in the middle / at the end
{
  const cps = [0x20, 0xa0, 0x2000, 0x2001, 0x2002, 0x2003, 0x2004, 0x2005, 0x2006, 0x2007, 0x2008, 0x2009, 0x200a, 0x200b, 0x202f, 0x205f, 0x3000];
  // (the code points the open font lacks are left out — U+00A0 in Carlito, U+205F in Liberation: Word's fallback for a
  // missing glyph is another rule — but U+3000, which none has, stays: it gives no height in Word whatever the face)
  const missing = { 'Carlito': [0xa0], 'Liberation Sans': [0x205f], 'Liberation Serif': [0x205f] };
  for (const f of ['Liberation Sans', 'Liberation Serif', 'Carlito']) {
    let body = p(r('inicio Hxg', sz(9)));
    for (const cp of cps.filter((c) => !missing[f].includes(c))) {
      const c = String.fromCodePoint(cp);
      body += p(r('meio Hxg', sz(9)) + r(c, sz(20)) + r('Hxg fim', sz(9)));
      body += p(r('ref Hxg', sz(9)));
      body += p(r('final Hxg', sz(9)) + r(c, sz(20)));
      body += p(r('ref Hxg', sz(9)));
    }
    cases.push({ name: `spaceh-${f.toLowerCase().replace(/\s+/g, '')}`, docx: D(body, { font: f, size: 9 }) });
  }
}

// ── L2 (numsp): the SPACE suffix after a list number
{
  const lvl = (id, { f = 'Carlito', s, text = '%1.', jc = 'left', fmt = 'decimal' }) =>
    `<w:abstractNum w:abstractNumId="${id}"><w:lvl w:ilvl="0"><w:start w:val="1"/><w:numFmt w:val="${fmt}"/><w:suff w:val="space"/><w:lvlText w:val="${esc(text)}"/><w:lvlJc w:val="${jc}"/><w:pPr><w:ind w:left="2880" w:hanging="2160"/></w:pPr><w:rPr>${f ? font(f) : ''}${s ? sz(s) : ''}</w:rPr></w:lvl></w:abstractNum>`;
  const specs = [];
  for (const s of [8, 11, 20, 36, 72]) specs.push({ l: { s }, run: 11 });
  for (const s of [11, 36, 72]) specs.push({ l: { f: 'Liberation Serif', s }, run: 11 });
  for (const s of [11, 36]) specs.push({ l: { f: 'Liberation Sans', s }, run: 11 });
  for (const s of [11, 36]) specs.push({ l: { f: 'Liberation Mono', s }, run: 11 });
  for (const m of [11, 20]) specs.push({ l: { f: null, s: null }, run: 11, mark: m });
  specs.push({ l: { s: 11 }, run: 36 });
  for (const s of [11, 36]) specs.push({ l: { f: 'Liberation Sans', s, text: '•', fmt: 'bullet' }, run: 11 });
  for (const jc of ['right', 'center']) {
    specs.push({ l: { s: 36, jc }, run: 11 });
    specs.push({ l: { f: 'Liberation Sans', s: 20, jc }, run: 11 });
  }
  const numbering = specs.map((x, i) => lvl(i + 1, x.l)).join('') + specs.map((_, i) => `<w:num w:numId="${i + 1}"><w:abstractNumId w:val="${i + 1}"/></w:num>`).join('');
  const body = specs.map((x, i) => p(r('texto Hxg', sz(x.run)), `<w:numPr><w:ilvl w:val="0"/><w:numId w:val="${i + 1}"/></w:numPr>${x.mark ? `<w:rPr>${sz(x.mark)}</w:rPr>` : ''}`)).join('');
  cases.push({ name: 'numsp-c15', docx: D(body, { numbering }) });
  cases.push({ name: 'numsp-c14', docx: D(body, { numbering, compat: 14 }) });
}

// ── L3 (jboth): the jc=both compression threshold, 1-twip steps around the rule (each paragraph = 2 lines)
{
  for (const f of ['Liberation Sans', 'Carlito']) {
    const w = widthOf(f, 11);
    const g = w(' ');
    let body = '';
    for (const n of [4, 8, 16, 24]) {
      const S = n * g;
      for (const T of ['que', 'documento', 'abcdefgh']) {
        const W = w(T);
        const base = n * (w('ab') + g) + W;
        const t = Math.min((0.5 * S * (W + g)) / (S - g + 0.5 * S), 0.25 * S);
        for (let ov = Math.floor(t - 8); ov <= Math.ceil(t + 8); ov++) {
          const A = Math.round(base - ov);
          body += p(r(Array(n).fill('ab').join(' ') + ' ' + T + ' fim da linha seguinte'), `<w:jc w:val="both"/>${ind(0, TEXTW - A)}`);
        }
      }
    }
    cases.push({ name: `jboth-${f.toLowerCase().replace(/\s+/g, '')}`, docx: D(body, { font: f }) });
  }
}

// ── L5 (cgrid): the character grid with Latin text (open fonts): linesAndChars charSpace, snapToChars cells
{
  const LAT = 'The quick brown fox jumps over the lazy dog and keeps running far away';
  const body = [
    p(r(LAT + ' ' + LAT)),
    p(r('abc def 12 Hxg mixed end')),
    p(r(LAT, sz(20))),
    p(r(LAT + ' ' + LAT), '<w:jc w:val="both"/>'),
    p(r('abc'), '<w:jc w:val="center"/>'),
    p(r('abc def'), '<w:jc w:val="right"/>'),
    p(r(LAT), ind(420, 0)),
    p(r(LAT), ind(500, 0)),
    p(r('Hxg abc', '<w:spacing w:val="40"/>')),
    p(r('abc def ') + r('ghi jkl', '<w:snapToGrid w:val="0"/>') + r(' mno')),
    p(r('sem grade de linha'), '<w:snapToGrid w:val="0"/>'),
  ].join('');
  for (const type of ['linesAndChars', 'snapToChars']) {
    for (const cs of [0, 4096, -2048, 20480]) {
      const grid = `<w:docGrid w:type="${type}" w:linePitch="360" w:charSpace="${cs}"/>`;
      cases.push({ name: `cgrid-${type === 'linesAndChars' ? 'lc' : 'sc'}-${String(cs).replace('-', 'm')}`, docx: D(body, { sect: sect(grid) }) });
    }
  }
  cases.push({ name: 'cgrid-sc-normal14', docx: D(body, { sect: sect('<w:docGrid w:type="snapToChars" w:linePitch="360" w:charSpace="0"/>'), normal: `<w:rPr>${sz(14)}</w:rPr>` }) });
}

// ── L6 (charsind): firstLineChars / leftChars / hangingChars / rightChars
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
    p(r(T('leftc100 hang200')), '<w:ind w:leftChars="100" w:hangingChars="200"/>'),
    p(r(T('leftneg')), '<w:ind w:leftChars="-100"/>'),
    p(r(T('left twips fl')), '<w:ind w:left="720" w:firstLineChars="200"/>'),
    p(r(T('leftc fl twips')), '<w:ind w:leftChars="200" w:firstLine="360"/>'),
    p(r(T('right400')), '<w:jc w:val="right"/><w:ind w:rightChars="400" w:right="100"/>'),
    p(r(T('run20'), sz(20)), '<w:ind w:firstLineChars="200"/>'),
    p(r(T('mark20')), '<w:ind w:firstLineChars="200"/><w:rPr>' + sz(20) + '</w:rPr>'),
    p(r('pequeno ', sz(8)) + r(T('then20'), sz(20)), '<w:ind w:firstLineChars="200"/>'),
    p(r(T('left run20'), sz(20)), '<w:ind w:leftChars="200"/>'),
  ].join('');
  cases.push({ name: 'charsind-lat', docx: D(body) });
  cases.push({ name: 'charsind-normal14', docx: D(body, { normal: `<w:rPr>${sz(14)}</w:rPr>` }) });
  cases.push({ name: 'charsind-grid', docx: D(body, { sect: sect('<w:docGrid w:type="linesAndChars" w:linePitch="360" w:charSpace="4096"/>') }) });
}

// ── L7 (shiftret): doNotExpandShiftReturn × jc × compatibility mode
{
  const line = (jc) => p(r('Uma linha ' + jc + ' curta') + '<w:r><w:br/></w:r>' + r('segunda linha depois da quebra manual Hxg') + '<w:r><w:br/></w:r>' + r('fim'), `<w:jc w:val="${jc}"/>`);
  const body = ['both', 'distribute', 'left', 'center'].map(line).join('')
    + p(r('texto longo que enche a primeira linha inteira e continua um pouco mais adiante para quebrar sozinho aqui e ali') + '<w:r><w:br/></w:r>' + r('depois'), '<w:jc w:val="both"/>');
  for (const c of [15, 14]) {
    cases.push({ name: `shiftret-c${c}`, docx: D(body, { compat: c }) });
    cases.push({ name: `shiftret-dne-c${c}`, docx: D(body, { compat: c, extraCompat: '<w:doNotExpandShiftReturn/>' }) });
  }
}

// ── L8 (ptab): w:ptab, indents × alignment × relativeTo; the pen past the target
{
  const pt = (a, rel, leader = 'none') => `<w:r><w:ptab w:relativeTo="${rel}" w:alignment="${a}" w:leader="${leader}"/></w:r>`;
  const inds = [[0, 0], [1440, 0], [0, 1440], [1440, 1440], [-720, 0], [720, 2880], [0, -720]];
  let body = '';
  for (const [L, R] of inds) {
    for (const a of ['left', 'center', 'right']) {
      for (const rel of ['margin', 'indent']) body += p(r('Esq') + pt(a, rel) + r('Alvo fim'), ind(L, R));
    }
  }
  cases.push({ name: 'ptab-short', docx: D(body, { font: 'Liberation Sans' }) });
  let body2 = '';
  const lorem = 'texto de enchimento que vai longe na linha e passa do meio da coluna com sobra';
  for (const [L, R] of [[0, 0], [1440, 1440], [1440, 0]]) {
    for (const rel of ['margin', 'indent']) {
      body2 += p(r(lorem.slice(0, 60)) + pt('center', rel) + r('Alvo centro'), ind(L, R));
      body2 += p(r(lorem.slice(0, 28)) + pt('center', rel) + r('Alvo centro'), ind(L, R));
      body2 += p(r(lorem + ' mais') + pt('right', rel) + r('Alvo direita'), ind(L, R));
      body2 += p(r('Esq longo texto') + pt('left', rel) + r('Alvo esquerda'), ind(L, R));
      body2 += p(pt('center', rel) + r('So alvo'), ind(L, R));
      body2 += p(r('Abc') + pt('right', rel, 'dot') + r('9'), ind(L, R));
    }
  }
  for (const rel of ['margin', 'indent']) {
    body2 += p(r('Primeira') + pt('center', rel) + r('Alvo'), ind(1440, 0, ' w:firstLine="1440"'));
    body2 += p(r('Pendente') + pt('center', rel) + r('Alvo'), ind(1440, 0, ' w:hanging="1440"'));
    body2 += p(r('Pendente') + pt('left', rel) + r('Alvo'), ind(1440, 0, ' w:hanging="1440"'));
  }
  cases.push({ name: 'ptab-past', docx: D(body2, { font: 'Liberation Sans' }) });
}

// ── L10 / L11 (tabs): default stops left of the margin; bar tabs
{
  let body = '';
  for (const L of [-1416, -1417, -1415, -708, -709, -707, -700, -1, 0, 1]) {
    body += p(TAB + r('X'), ind(L, 0));
    body += p(r('ab') + TAB + r('X'), ind(L, 0));
  }
  body += p(TAB + r('X'), ind(0, 0, ' w:hanging="708"'));
  body += p(TAB + r('X'), ind(708, 0, ' w:hanging="1416"'));
  cases.push({ name: 'tabs-neg', docx: D(body, { font: 'Liberation Sans' }) });
  let b2 = '';
  b2 += p(r('a') + TAB + r('b'), tabsXml([['bar', 2000]]));
  b2 += p(r('a') + TAB + r('b') + TAB + r('c'), tabsXml([['bar', 2000]]));
  b2 += p(r('abcdefghijklmnopq') + TAB + r('b'), tabsXml([['left', 1000], ['bar', 2000]]));
  b2 += p(r('abcdefghijklmnopqrstuvwxyzabcdefghijklmnopq') + TAB + r('b'), tabsXml([['bar', 2000], ['left', 4000]]));
  b2 += p(r('a') + TAB + r('b'), tabsXml([['bar', 300]]));
  b2 += p(r('a') + TAB + r('b') + TAB + r('c'), tabsXml([['bar', 700], ['bar', 1500]]));
  b2 += p(r('abcdefghijklmnopq') + TAB + r('b'), tabsXml([['bar', 2000], ['bar', 4000]]));
  for (const c of [15, 14]) cases.push({ name: `tabs-bar-c${c}`, docx: D(b2, { font: 'Liberation Sans', compat: c }) });
}

// ── L12 (emph): emphasis marks with Latin text — the line pitch and the marks' place
{
  const filler = (k) => p(r(`ref ${k} Hxg`));
  let body = filler(0);
  let k = 1;
  const runs = [['enfase latina Hxg', ''], ['enfase grande Hxg', sz(22)], ['enfase sans Hxg', font('Liberation Sans')], ['enfase pequena', sz(8)]];
  for (const em of ['dot', 'comma', 'circle', 'underDot']) {
    for (const [t, rpr] of runs) {
      body += p(r(t, rpr + `<w:em w:val="${em}"/>`));
      body += filler(k++);
    }
  }
  body += p(r('so um ') + r('x', '<w:em w:val="dot"/>') + r(' aqui Hxg')) + filler(k++);
  body += p(r('mista ', sz(20)) + r('enfase Hxg', '<w:em w:val="dot"/>')) + filler(k++);
  for (const [name, sp] of [['exata', '<w:spacing w:line="300" w:lineRule="exact"/>'], ['minima', '<w:spacing w:line="300" w:lineRule="atLeast"/>'],
    ['minima alta', '<w:spacing w:line="500" w:lineRule="atLeast"/>'], ['m115', '<w:spacing w:line="276" w:lineRule="auto"/>'],
    ['m15', '<w:spacing w:line="360" w:lineRule="auto"/>'], ['dupla', '<w:spacing w:line="480" w:lineRule="auto"/>']]) {
    for (const em of ['dot', 'underDot']) {
      body += p(r(name + ' enfase Hxg', `<w:em w:val="${em}"/>`), sp) + filler(k++);
    }
  }
  body += p(r('linha um com enfase que segue e segue ate quebrar a linha no fim da coluna e ir para a segunda linha', '<w:em w:val="dot"/>')) + filler(k++);
  cases.push({ name: 'emph-carlito', docx: D(body) });
  cases.push({ name: 'emph-none', docx: D(body.replace(/<w:em w:val="[a-zA-Z]+"\/>/g, '')) });
}

// ── L13 (supbig): superscript / subscript above 24 pt — the family names of the measured table (Word: its fonts; the
// test: the metric twins)
{
  const big = '<w:sectPr><w:pgSz w:w="31680" w:h="31680"/><w:pgMar w:top="720" w:right="720" w:bottom="720" w:left="720" w:header="360" w:footer="360" w:gutter="0"/><w:cols w:space="708"/><w:docGrid w:linePitch="360"/></w:sectPr>';
  const sizes = [24, 25, 30.5, 45, 58, 72, 100, 150, 200, 250, 400];
  for (const f of ['Arial', 'Calibri', 'Times New Roman', 'Cambria']) {
    const body = sizes.map((s) => p(r('H', sz(s)) + r('x', sz(s) + '<w:vertAlign w:val="superscript"/>') + r('H', sz(s)) + r('x', sz(s) + '<w:vertAlign w:val="subscript"/>'))).join('');
    cases.push({ name: `supbig-${f.toLowerCase().replace(/\s+/g, '')}`, docx: D(body, { font: f, sect: big }) });
  }
}
