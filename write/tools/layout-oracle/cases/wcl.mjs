// WordCraft study, planner "L" (line-level layout): the measurements the plan needs that wca/wca2/wca3 left open
// (core/target/wordcraft-study/VERDICTS.json "not_measured"; plan core/target/wordcraft-study/PLAN-L.md, rust-core
// worktree). Run with --fixdir core/target/wordcraft-study/L/wcl (NOT the frozen TS corpus). Installed fonts referenced
// by NAME only (never embedded, never versioned); invented texts.
//  - A1: the compression threshold of jc=distribute (1-twip steps around the bracket wca3 found) and of jc=both (the
//    0.5–1 tw misses at n = 16–24);
//  - A19: how jc=distribute spreads the slack (letter gaps × spaces; last line × other lines), slack swept;
//  - A11: superscript raise / subscript drop above 24 pt (the measured table stops at 24);
//  - A21: the SPACE suffix after a list number (≈ 0.274 em: of which size? other fonts, bullets, lvlJc);
//  - A24: which space characters make a line taller (every U+200x);
//  - A20: w:ptab (relativeTo margin/indent, asymmetric indents, the target left of the pen);
//  - A22/A23: default tab stops left of the margin, bar tabs.
import { buildDocx, docDefaults, p, r, sz, font, esc } from '../docx-builder.mjs';
import { createNodeFontProvider } from '../../corpus-lab/node-fonts.mjs';

const prov = await createNodeFontProvider();
const widthOf = (name, size) => {
  const f = prov.face(name, false, false)?.font;
  if (!f) throw new Error('font ' + name);
  return (s) => [...s].reduce((a, ch) => a + Math.floor((f.advanceWidth(f.glyphId(ch.codePointAt(0))) * size * 4096) / f.metrics.unitsPerEm) / 4096 * 20, 0);
};
const TEXTW = 11906 - 2 * 1701;
const D = (body, o = {}) => buildDocx({ body, defaults: docDefaults({ font: o.font ?? 'Calibri', size: o.size ?? 11 }), ...o });
const ind = (left, right, extra = '') => `<w:ind w:left="${Math.round(left)}" w:right="${Math.round(right)}"${extra}/>`;
const tabsXml = (list) => `<w:tabs>${list.map(([v, pos]) => `<w:tab w:val="${v}" w:pos="${pos}"/>`).join('')}</w:tabs>`;
const TAB = '<w:r><w:tab/></w:r>';

export const cases = [];

// ── A1: compression thresholds (each paragraph = 2 lines; line 1 ends with T when Word compresses)
{
  const sweep = (fontName, jc, ns, words, win) => {
    const w = widthOf(fontName, 11);
    const g = w(' ');
    let body = '';
    let count = 0;
    for (const n of ns) {
      const S = n * g;
      for (const T of words) {
        const W = w(T);
        const base = n * (w('ab') + g) + W;
        const [lo, hi] = win(S, g, W);
        for (let ov = Math.floor(lo); ov <= Math.ceil(hi); ov++) {
          const A = Math.round(base - ov);
          body += p(r(Array(n).fill('ab').join(' ') + ' ' + T + ' fim da linha seguinte'), `<w:jc w:val="${jc}"/>${ind(0, TEXTW - A)}`);
          count++;
        }
      }
    }
    return body;
  };
  const cap = (S) => 0.25 * S;
  const thB = (S, g, W) => (0.5 * S * (W + g)) / (S - g + 0.5 * S);
  const thD = (S, g, W) => (0.5 * S * (W + g)) / (1.3 * (S - g) + 0.5 * S);
  for (const f of ['Calibri', 'Arial']) {
    const slug = f.toLowerCase();
    // distribute: from below the fitted rule to the "both" rule
    const bodyD = sweep(f, 'distribute', [4, 6, 8, 10, 12, 14, 16, 20, 24, 32], ['e', 'que', 'documento'],
      (S, g, W) => [Math.min(thD(S, g, W), cap(S)) - 0.035 * S - 2, Math.min(thB(S, g, W), cap(S)) + 3]);
    cases.push({ name: `l-jdist-${slug}`, docx: D(bodyD, { font: f }) });
    // both: ±8 tw around our rule
    const bodyB = sweep(f, 'both', [4, 8, 12, 16, 20, 24, 32], ['e', 'que', 'documento', 'ab', 'abcdefgh'],
      (S, g, W) => { const t = Math.min(thB(S, g, W), cap(S)); return [t - 8, t + 8]; });
    cases.push({ name: `l-jboth-${slug}`, docx: D(bodyB, { font: f }) });
  }
}

// ── A19: how distribute spreads the slack. Single-line paragraphs (the LAST line) and first lines of paragraphs whose
// next word cannot fit even compressed (a NON-last line). Slack in pt, the right indent rounded to a twip.
{
  const texts = ['abcdefgh', 'abcd efgh', 'ab cd ef gh', 'ab ab ab ab ab ab ab ab', 'ab ab ab ab ab ab ab ab ab ab ab ab'];
  const slacks = [0.3, 0.6, 1, 1.5, 2, 2.5, 3, 4, 5, 6, 8, 10, 12, 15, 20, 25, 30, 40, 60, 100, 200];
  for (const [f, s] of [['Calibri', 11], ['Arial', 11], ['Calibri', 22], ['Times New Roman', 11]]) {
    const w = widthOf(f, s);
    let body = '';
    const long = 'X'.repeat(90);
    for (const ctx of ['last', 'mid']) {
      for (const t of texts) {
        for (const sl of slacks) {
          const A = Math.round(w(t) + sl * 20);
          if (A > TEXTW) continue;
          const txt = ctx === 'last' ? t : t + ' ' + long;
          body += p(r(txt, sz(s)), `<w:jc w:val="distribute"/>${ind(0, TEXTW - A)}`);
        }
      }
    }
    cases.push({ name: `l-jspread-${f.toLowerCase().replace(/\s+/g, '')}${s}`, docx: D(body, { font: f, size: s }) });
  }
}

// ── A11: superscript / subscript above 24 pt (big page so 200 pt fits)
{
  const big = '<w:sectPr><w:pgSz w:w="31680" w:h="31680"/><w:pgMar w:top="720" w:right="720" w:bottom="720" w:left="720" w:header="360" w:footer="360" w:gutter="0"/><w:cols w:space="708"/><w:docGrid w:linePitch="360"/></w:sectPr>';
  const sizes = [];
  for (let s = 24; s <= 48; s += 0.5) sizes.push(s);
  for (let s = 49; s <= 72; s++) sizes.push(s);
  for (let s = 74; s <= 100; s += 2) sizes.push(s);
  for (let s = 110; s <= 200; s += 10) sizes.push(s);
  const fonts = ['Arial', 'Calibri', 'Courier New', 'Segoe UI', 'Times New Roman', 'Tahoma', 'Verdana', 'Cambria', 'Georgia', 'Century Gothic'];
  for (const f of fonts) {
    const body = sizes.map((s) => p(r('H', sz(s)) + r('x', sz(s) + '<w:vertAlign w:val="superscript"/>') + r('H', sz(s)) + r('x', sz(s) + '<w:vertAlign w:val="subscript"/>'))).join('');
    cases.push({ name: `l-supbig-${f.toLowerCase().replace(/\s+/g, '-')}`, docx: D(body, { font: f, sect: big }) });
  }
}

// ── A21: the SPACE suffix after the number: number size swept, text size and mark size apart, other fonts, bullets,
// lvlJc. Each paragraph: number + suffix + "texto".
{
  const lvl = (id, { f = 'Calibri', s, text = '%1.', jc = 'left', fmt = 'decimal', suff = 'space' }) =>
    `<w:abstractNum w:abstractNumId="${id}"><w:lvl w:ilvl="0"><w:start w:val="1"/><w:numFmt w:val="${fmt}"/><w:suff w:val="${suff}"/><w:lvlText w:val="${esc(text)}"/><w:lvlJc w:val="${jc}"/><w:pPr><w:ind w:left="2880" w:hanging="2160"/></w:pPr><w:rPr>${f ? font(f) : ''}${s ? sz(s) : ''}</w:rPr></w:lvl></w:abstractNum>`;
  const specs = [];
  for (const s of [8, 9, 10, 11, 12, 14, 16, 20, 24, 28, 36, 48, 72, 100]) specs.push({ k: `cal${s}`, l: { s }, run: 11 });
  for (const s of [11, 36, 72]) specs.push({ k: `times${s}`, l: { f: 'Times New Roman', s }, run: 11 });
  for (const s of [11, 36, 72]) specs.push({ k: `cour${s}`, l: { f: 'Courier New', s }, run: 11 });
  for (const s of [36, 72]) specs.push({ k: `segoe${s}`, l: { f: 'Segoe UI', s }, run: 11 });
  // no size on the level: the number takes the paragraph mark's size
  for (const m of [11, 20, 48]) specs.push({ k: `nosz-mark${m}`, l: { f: null, s: null }, run: 11, mark: m });
  // the text's size apart from the number's
  for (const t of [8, 36]) specs.push({ k: `cal11-run${t}`, l: { s: 11 }, run: t });
  specs.push({ k: 'cal36-run8', l: { s: 36 }, run: 8 });
  // bullets
  for (const s of [11, 36, 72]) specs.push({ k: `sym${s}`, l: { f: 'Symbol', s, text: '', fmt: 'bullet' }, run: 11 });
  for (const s of [11, 36]) specs.push({ k: `wing${s}`, l: { f: 'Wingdings', s, text: '', fmt: 'bullet' }, run: 11 });
  specs.push({ k: 'couro36', l: { f: 'Courier New', s: 36, text: 'o', fmt: 'bullet' }, run: 11 });
  // lvlJc right / center
  for (const jc of ['right', 'center']) specs.push({ k: `cal36-${jc}`, l: { s: 36, jc }, run: 11 });
  const numbering = specs.map((x, i) => lvl(i + 1, x.l)).join('') + specs.map((_, i) => `<w:num w:numId="${i + 1}"><w:abstractNumId w:val="${i + 1}"/></w:num>`).join('');
  const body = specs.map((x, i) => p(r('texto Hxg', sz(x.run)), `<w:numPr><w:ilvl w:val="0"/><w:numId w:val="${i + 1}"/></w:numPr>${x.mark ? `<w:rPr>${sz(x.mark)}</w:rPr>` : ''}`)).join('');
  cases.push({ name: 'l-numsp', docx: D(body, { numbering }) });
  cases.push({ name: 'l-numsp-c14', docx: D(body, { numbering, compat: 14 }) });
}

// ── A24: which space characters make the line taller: 9 pt text with one 20 pt character in the middle / at the end
{
  const cps = [0x20, 0xa0, 0x2000, 0x2001, 0x2002, 0x2003, 0x2004, 0x2005, 0x2006, 0x2007, 0x2008, 0x2009, 0x200a, 0x200b, 0x202f, 0x205f, 0x3000];
  for (const f of ['Calibri', 'Arial', 'Times New Roman']) {
    let body = p(r('inicio Hxg', sz(9)));
    for (const cp of cps) {
      const c = String.fromCodePoint(cp);
      body += p(r('meio Hxg', sz(9)) + r(c, sz(20)) + r('Hxg fim', sz(9)));
      body += p(r('ref Hxg', sz(9)));
      body += p(r('final Hxg', sz(9)) + r(c, sz(20)));
      body += p(r('ref Hxg', sz(9)));
    }
    cases.push({ name: `l-spaces-${f.toLowerCase().replace(/\s+/g, '')}`, docx: D(body, { font: f, size: 9 }) });
  }
}

// ── A20: w:ptab. Indents (left, right) × alignment × relativeTo; text before the tab short, or past the target.
{
  const pt = (a, rel, leader = 'none') => `<w:r><w:ptab w:relativeTo="${rel}" w:alignment="${a}" w:leader="${leader}"/></w:r>`;
  const inds = [[0, 0], [1440, 0], [0, 1440], [1440, 1440], [-720, 0], [720, 2880], [0, -720]];
  let body = '';
  for (const [L, R] of inds) {
    for (const a of ['left', 'center', 'right']) {
      for (const rel of ['margin', 'indent']) {
        body += p(r('Esq') + pt(a, rel) + r('Alvo fim'), ind(L, R));
      }
    }
  }
  cases.push({ name: 'l-ptab-short', docx: D(body) });
  // the pen past the target: centre (text ~ 60 % of the width), right (text ~ 95 %), left (any text)
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
  // first-line / hanging indents
  for (const rel of ['margin', 'indent']) {
    body2 += p(r('Primeira') + pt('center', rel) + r('Alvo'), ind(1440, 0, ' w:firstLine="1440"'));
    body2 += p(r('Pendente') + pt('center', rel) + r('Alvo'), ind(1440, 0, ' w:hanging="1440"'));
    body2 += p(r('Pendente') + pt('left', rel) + r('Alvo'), ind(1440, 0, ' w:hanging="1440"'));
  }
  cases.push({ name: 'l-ptab-past', docx: D(body2) });
}

// ── A22 / A23: default stops left of the margin; bar tabs
{
  let body = '';
  for (const L of [-1416, -1417, -1415, -708, -709, -707, -700, -1, 0, 1]) {
    body += p(TAB + r('X'), ind(L, 0));
    body += p(r('ab') + TAB + r('X'), ind(L, 0));
  }
  // hanging with the text start left of the margin
  body += p(TAB + r('X'), ind(0, 0, ' w:hanging="708"'));
  body += p(TAB + r('X'), ind(708, 0, ' w:hanging="1416"'));
  cases.push({ name: 'l-negtab', docx: D(body) });
  let b2 = '';
  b2 += p(r('a') + TAB + r('b'), tabsXml([['bar', 2000]]));
  b2 += p(r('a') + TAB + r('b') + TAB + r('c'), tabsXml([['bar', 2000]]));
  b2 += p(r('abcdefghijklmnopq') + TAB + r('b'), tabsXml([['left', 1000], ['bar', 2000]]));
  b2 += p(r('abcdefghijklmnopqrstuvwxyzabcdefghijklmnopq') + TAB + r('b'), tabsXml([['bar', 2000], ['left', 4000]]));
  b2 += p(r('a') + TAB + r('b'), tabsXml([['bar', 300]]));
  b2 += p(r('a') + TAB + r('b'), tabsXml([['bar', 700], ['bar', 1500]]));
  b2 += p(r('abcdefghijklmnopq') + TAB + r('b'), tabsXml([['bar', 2000], ['bar', 4000]]));
  cases.push({ name: 'l-bartab', docx: D(b2) });
  cases.push({ name: 'l-bartab-c14', docx: D(b2, { compat: 14 }) });
}
