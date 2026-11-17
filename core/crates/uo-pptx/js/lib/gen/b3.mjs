// The B3 slide builder: text-heavy slides for the text-layout capture (capture-textlayout.mjs). The package (theme,
// master, layouts) is the B1 one (b1.mjs `buildDeck` with `profile.slide`); every slide carries the layout's
// placeholders and several text boxes and shapes with long paragraphs in mixed scripts (Latin with long words and
// dashes, CJK with kinsoku punctuation, Arabic, Hebrew, Thai, Devanagari conjuncts, combining marks, emoji ZWJ
// sequences and flags, symbol-area characters), autofit with fontScale/lnSpcReduction, columns, vertical text,
// bullets and auto-numbering in every scheme and level, tabs with tab stops, line spacing in percent and points,
// spacing before/after, alignments (justify and distribute included), caps, small caps, superscript, kerning,
// character spacing and tables with long cell text. Fonts are named, never embedded: the open families, a few
// names that fall back to the substitute, and the symbol fonts.
import { esc } from './dml.mjs';
import { NS, XML } from './parts.mjs';
import { rPrXml } from './text.mjs';
import { randomPng } from '../png.mjs';

const GRP = '<p:nvGrpSpPr><p:cNvPr id="1" name=""/><p:cNvGrpSpPr/><p:nvPr/></p:nvGrpSpPr><p:grpSpPr><a:xfrm><a:off x="0" y="0"/><a:ext cx="0" cy="0"/><a:chOff x="0" y="0"/><a:chExt cx="0" cy="0"/></a:xfrm></p:grpSpPr>';

const FONTS = ['Liberation Sans', 'Liberation Serif', 'Liberation Mono', 'Carlito', 'Caladea', 'Calibri', 'Arial', 'Times New Roman', 'Courier New', 'Cambria', 'Segoe UI', '+mn-lt', '+mj-lt'];
const LATIN = ('the quick brown fox jumps over the lazy dog office affluent waffle fjord AVATAR Tomorrow WAVE Yard '
  + 'lorem ipsum dolor sit amet consectetur adipiscing elit sed do eiusmod tempor incididunt ut labore et dolore magna aliqua '
  + 'naïve café coração ação Ærøskøbing straße 12,345.67 R$ 1.234,56 (a+b)/c 2026-11-17 e-mail well-known state-of-the-art '
  + 'self‑contained co‐operate soft­hyphen em—dash en–dash 100% #1 $5 “quoted” ‘single’ …').split(' ');
const LONG = ['Supercalifragilisticexpialidocious', 'pneumonoultramicroscopicsilicovolcanoconiosis', 'https://example.org/a/very/long/path/with-segments/and_underscores?x=1&y=2',
  'Donaudampfschifffahrtselektrizitätenhauptbetriebswerkbauunterbeamtengesellschaft', 'ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789'];
const SCRIPTS = {
  ja: ['これは日本語のテキストです。', '句読点、改行。', '「引用」と（括弧）を含む文章。', 'カタカナのアカウントしかわからない', 'ッャュョー', '漢字かな交じり文'],
  zh: ['这是一个测试文本，包含标点。', '中文没有空格', '《书名》与“引号”', '测试！问号？'],
  ko: ['한국어 텍스트입니다', '띄어쓰기가 있는 문장', '가나다라마바사'],
  ar: ['هذا نص تجريبي باللغة العربية', 'مرحبا بالعالم', 'السلام عليكم'],
  he: ['זהו טקסט לדוגמה בעברית', 'שלום עולם'],
  th: ['ภาษาไทยไม่มีช่องว่างระหว่างคำ', 'สวัสดีครับ'],
  hi: ['यह एक परीक्षण पाठ है', 'क्षत्रिय', 'नमस्ते दुनिया', 'स्त्री'],
  marks: ['é', 'ǟ', 'ño', 'Z͑ͫ̓', 'o̶̶'],
  emoji: ['\u{1F600}', '\u{1F468}‍\u{1F469}‍\u{1F467}‍\u{1F466}', '\u{1F1E7}\u{1F1F7}', '\u{1F44D}\u{1F3FD}', '❤️', '\u{1F3F3}️‍\u{1F308}'],
  sym: ['', '', ''],
  nbsp: ['ipsum.  dolor', '10 km', 'a b c'],
};
const LANG = { ja: 'ja-JP', zh: 'zh-CN', ko: 'ko-KR', ar: 'ar-SA', he: 'he-IL', th: 'th-TH', hi: 'hi-IN' };
const AUTONUM = ['arabicPeriod', 'arabicParenR', 'arabicPlain', 'romanUcPeriod', 'romanLcPeriod', 'alphaUcPeriod', 'alphaLcPeriod', 'alphaLcParenR', 'alphaUcParenR', 'arabicParenBoth', 'circleNumDbPlain', 'ea1JpnChsDbPeriod', 'thaiNumPeriod'];
const BULLETS = ['•', '–', '▪', '➢', '○', '■', '✓', '»', '-', '\u{1F449}'];

/** Text of `n` words drawn from the pools; returns { text, lang } (the lang of the last script used). */
function stressText(c, n) {
  const r = c.r;
  const out = [];
  let lang;
  for (let i = 0; i < n; i++) {
    const k = r.weighted([['latin', 60], ['long', 3], ['script', 22], ['tab', 2], ['dash', 3], ['space', 4], ['extra', 6]]);
    if (k === 'latin') out.push(r.pick(LATIN));
    else if (k === 'long') { out.push(r.pick(LONG)); c.feat('b3:longword'); }
    else if (k === 'tab') { out.push('\t'); c.feat('b3:tab-in-run'); }
    else if (k === 'dash') out.push(r.pick(['-', '—', '–', 'x-y', 'a—b']));
    else if (k === 'space') out.push(r.pick(['  ', '   ', ' ', '   ']));
    else if (k === 'extra') out.push(r.pick(['', '0', '.', ',', 'a.b,c', '12.50', '3,1416']));
    else {
      const s = r.pick(Object.keys(SCRIPTS));
      out.push(r.pick(SCRIPTS[s]));
      if (LANG[s]) lang = LANG[s];
      c.feat('b3:script:' + s);
    }
  }
  return { text: out.join(r.chance(0.08) ? '' : ' '), lang };
}

function runXml(c) {
  const r = c.r;
  const k = r.weighted([['r', 86], ['br', 6], ['fld', 3], ['symfont', 5]]);
  if (k === 'br') { c.feat('b3:br'); return `<a:br>${rPrXml(c, 'rPr', { rich: false })}</a:br>`; }
  if (k === 'fld') return `<a:fld id="${r.guid()}" type="slidenum">${rPrXml(c, 'rPr', { rich: false })}<a:t>‹#›</a:t></a:fld>`;
  const w = stressText(c, r.weighted([[1, 15], [3, 20], [8, 25], [20, 25], [45, 15]]));
  const at = [];
  const lang = w.lang && r.chance(0.8) ? w.lang : r.chance(0.2) ? r.pick(['en-US', 'pt-BR', 'ja-JP', 'zh-TW', 'ko-KR', 'JA', 'ja_JP']) : undefined;
  if (lang) at.push(`lang="${lang}"`);
  if (r.chance(0.8)) at.push(`sz="${r.weighted([[800, 3], [1000, 4], [1200, 8], [1400, 8], [1800, 10], [2400, 8], [3200, 5], [4400, 3], [6000, 2], [r.int(500, 9600), 6]])}"`);
  if (r.chance(0.2)) at.push(`b="${r.below(2)}"`);
  if (r.chance(0.2)) at.push(`i="${r.below(2)}"`);
  if (r.chance(0.1)) at.push('u="sng"');
  if (r.chance(0.06)) at.push('strike="sngStrike"');
  if (r.chance(0.15)) { at.push(`spc="${r.int(-200, 800)}"`); c.feat('b3:spc'); }
  if (r.chance(0.1)) { at.push(`baseline="${r.pick([30000, -25000, 50000])}"`); c.feat('b3:baseline'); }
  if (r.chance(0.12)) { at.push(`cap="${r.pick(['all', 'small'])}"`); c.feat('b3:cap'); }
  if (r.chance(0.3)) { at.push(`kern="${r.pick([0, 1000, 1200, 2400])}"`); c.feat('b3:kern'); }
  const kids = [];
  if (r.chance(0.25)) kids.push(`<a:solidFill><a:srgbClr val="${r.hex6()}"/></a:solidFill>`);
  if (k === 'symfont') { kids.push(`<a:latin typeface="${r.pick(['Wingdings', 'Symbol', 'Webdings', 'Wingdings 2', 'Monotype Sorts'])}"/>`); c.feat('b3:symbol-font'); }
  else if (r.chance(0.6)) kids.push(`<a:latin typeface="${r.pick(FONTS)}"/>`);
  if (r.chance(0.05)) kids.push(`<a:sym typeface="${r.pick(['Wingdings', 'Symbol', 'Liberation Sans'])}"/>`);
  if (r.chance(0.04) && c.addLink) kids.push(`<a:hlinkClick r:id="${c.addLink(`https://example.org/${r.int(1, 99)}`)}"/>`);
  const rpr = `<a:rPr${at.length ? ' ' + at.join(' ') : ''}${kids.length ? `>${kids.join('')}</a:rPr>` : '/>'}`;
  const text = k === 'symfont' ? r.pick(['abc', 'Ø§', 'lmnop', '', ' x ']) : w.text + (r.chance(0.3) ? ' ' : '') + (r.chance(0.004) ? String.fromCharCode(0xf07f) : '');
  return `<a:r>${rpr}<a:t xml:space="preserve">${esc(text)}</a:t></a:r>`;
}

function bulletXml(c, lvl) {
  const r = c.r;
  const k = r.weighted([['none', 15], ['char', 30], ['auto', 40], ['blip', 3], ['symchar', 7], ['inherit', 5]]);
  if (k === 'inherit') return '';
  c.feat('b3:bullet:' + k);
  const pre = [];
  if (k !== 'none' && r.chance(0.3)) pre.push(`<a:buClr><a:srgbClr val="${r.hex6()}"/></a:buClr>`);
  if (k !== 'none' && r.chance(0.4)) pre.push(r.chance(0.5) ? `<a:buSzPct val="${r.int(25000, 300000)}"/>` : `<a:buSzPts val="${r.int(400, 4000)}"/>`);
  if (k === 'none') return '<a:buNone/>';
  if (k === 'char') return `${pre.join('')}${r.chance(0.3) ? `<a:buFont typeface="${r.pick(FONTS)}"/>` : ''}<a:buChar char="${esc(r.pick(BULLETS))}"/>`;
  if (k === 'symchar') return `${pre.join('')}<a:buFont typeface="${r.pick(['Wingdings', 'Symbol'])}"/><a:buChar char="${r.pick(['§', 'Ø', 'l', 'n', 'ü'])}"/>`;
  if (k === 'auto') {
    const font = r.chance(0.2) ? `<a:buFont typeface="${r.pick(['Wingdings', '+mj-lt', 'Liberation Serif'])}"/>` : '';
    return `${pre.join('')}${font}<a:buAutoNum type="${r.pick(AUTONUM)}"${r.chance(0.3) ? ` startAt="${r.int(1, 30)}"` : ''}/>`;
  }
  const rId = c.addImage(randomPng(r));
  return `${pre.join('')}<a:buBlip><a:blip r:embed="${rId}"/></a:buBlip>`;
}

function paragraphXml(c, { levels }) {
  const r = c.r;
  const lvl = levels ? r.weighted([[0, 40], [1, 25], [2, 15], [3, 8], [r.int(4, 8), 12]]) : 0;
  const at = [];
  if (lvl) at.push(`lvl="${lvl}"`);
  if (r.chance(0.55)) { at.push(`algn="${r.weighted([['l', 3], ['ctr', 3], ['r', 3], ['just', 4], ['dist', 3]])}"`); c.feat('b3:align'); }
  if (r.chance(0.45)) { const marL = r.int(0, 1400000); at.push(`marL="${marL}"`); if (r.chance(0.8)) at.push(`indent="${r.chance(0.8) ? -r.int(0, Math.min(marL + 200000, 700000)) : r.int(0, 400000)}"`); }
  if (r.chance(0.15)) at.push(`eaLnBrk="${r.below(2)}" hangingPunct="${r.below(2)}"`);
  if (r.chance(0.1)) at.push(`defTabSz="${r.pick([457200, 914400, 360000, 0])}"`);
  if (r.chance(0.1)) at.push(`fontAlgn="${r.pick(['t', 'ctr', 'b', 'base', 'auto'])}"`);
  const kids = [];
  if (r.chance(0.45)) { kids.push(`<a:lnSpc>${r.chance(0.6) ? `<a:spcPct val="${r.pick([50000, 80000, 90000, 100000, 120000, 150000, 200000, 250000])}"/>` : `<a:spcPts val="${r.int(500, 6000)}"/>`}</a:lnSpc>`); c.feat('b3:lnspc'); }
  if (r.chance(0.35)) kids.push(`<a:spcBef>${r.chance(0.5) ? `<a:spcPct val="${r.int(0, 150000)}"/>` : `<a:spcPts val="${r.int(0, 3600)}"/>`}</a:spcBef>`);
  if (r.chance(0.35)) kids.push(`<a:spcAft>${r.chance(0.5) ? `<a:spcPct val="${r.int(0, 150000)}"/>` : `<a:spcPts val="${r.int(0, 3600)}"/>`}</a:spcAft>`);
  if (r.chance(levels ? 0.85 : 0.45)) kids.push(bulletXml(c, lvl));
  if (r.chance(0.25)) {
    const n = r.int(1, 5);
    let pos = r.chance(0.2) ? 0 : r.int(0, 300000);
    const tabs = [];
    for (let i = 0; i < n; i++) { tabs.push(`<a:tab pos="${pos}" algn="${r.pick(['l', 'ctr', 'r', 'dec'])}"/>`); pos += r.int(150000, 1200000); }
    kids.push(`<a:tabLst>${tabs.join('')}</a:tabLst>`);
    c.feat('b3:tabs');
  }
  const ppr = at.length || kids.length ? `<a:pPr${at.length ? ' ' + at.join(' ') : ''}${kids.length ? `>${kids.join('')}</a:pPr>` : '/>'}` : '';
  const n = r.weighted([[0, 6], [1, 30], [2, 30], [3, 20], [5, 14]]);
  const runs = [];
  for (let i = 0; i < n; i++) runs.push(runXml(c));
  // tabs between runs (stops and decimal alignment)
  if (r.chance(0.2) && runs.length) { runs.splice(r.below(runs.length + 1), 0, '<a:r><a:rPr lang="en-US"/><a:t>\t</a:t></a:r>'); c.feat('b3:tab-run'); }
  const end = r.chance(0.4) ? `<a:endParaRPr lang="en-US" sz="${r.int(200, 4800)}"/>` : '';
  return `<a:p>${ppr}${runs.join('')}${end}</a:p>`;
}

function bodyPrXml(c, { ph }) {
  const r = c.r;
  const at = [];
  if (r.chance(0.12)) at.push('wrap="none"');
  if (r.chance(0.3)) at.push(`lIns="${r.int(0, 250000)}" tIns="${r.int(0, 150000)}" rIns="${r.int(0, 250000)}" bIns="${r.int(0, 150000)}"`);
  if (r.chance(0.5)) at.push(`anchor="${r.pick(['t', 'ctr', 'b'])}"`);
  if (r.chance(0.12)) { at.push(`vert="${r.pick(['vert', 'vert270', 'eaVert', 'wordArtVert'])}"`); c.feat('b3:vert'); }
  if (r.chance(0.15)) { at.push(`numCol="${r.int(2, 3)}" spcCol="${r.int(0, 400000)}"`); c.feat('b3:columns'); }
  if (r.chance(0.06)) at.push('spcFirstLastPara="1"');
  const kids = [];
  if (r.chance(0.05)) { kids.push(`<a:prstTxWarp prst="${r.pick(['textPlain', 'textArchUp', 'textWave1', 'textCircle'])}"><a:avLst/></a:prstTxWarp>`); c.feat('b3:warp'); }
  const fit = r.weighted([['', 40], ['norm', 15], ['scale', 30], ['sp', 10], ['no', 5]]);
  if (fit === 'norm') kids.push('<a:normAutofit/>');
  else if (fit === 'scale') { kids.push(`<a:normAutofit fontScale="${r.pick([92500, 85000, 77500, 70000, 62500, 55000, 47500, 40000, 25000])}"${r.chance(0.7) ? ` lnSpcReduction="${r.pick([10000, 20000])}"` : ''}/>`); c.feat('b3:autofit-scale'); }
  else if (fit === 'sp') kids.push('<a:spAutoFit/>');
  else if (fit === 'no') kids.push('<a:noAutofit/>');
  if (ph && !at.length && !kids.length) return '<a:bodyPr/>';
  return `<a:bodyPr${at.length ? ' ' + at.join(' ') : ''}${kids.length ? `>${kids.join('')}</a:bodyPr>` : '/>'}`;
}

function txBody(c, { tag = 'p:txBody', ph = false, levels = false, paras }) {
  const r = c.r;
  const n = paras ?? r.weighted([[1, 15], [2, 25], [3, 25], [5, 20], [8, 15]]);
  const ps = [];
  for (let i = 0; i < n; i++) ps.push(paragraphXml(c, { levels }));
  return `<${tag}>${bodyPrXml(c, { ph })}<a:lstStyle/>${ps.join('')}</${tag}>`;
}

function box(c, area) {
  const r = c.r;
  const cx = r.int(Math.round(area.w * 0.12), Math.round(area.w * 0.7));
  const cy = r.int(Math.round(area.h * 0.08), Math.round(area.h * 0.6));
  return { x: r.int(0, area.w - cx), y: r.int(0, area.h - cy), cx, cy };
}

function shapeXml(c, area) {
  const r = c.r;
  const id = c.nextId();
  const b = box(c, area);
  const geom = r.pick(['rect', 'rect', 'roundRect', 'ellipse', 'wedgeRectCallout']);
  const tbx = r.chance(0.5);
  return `<p:sp><p:nvSpPr><p:cNvPr id="${id}" name="Text ${id}"/><p:cNvSpPr${tbx ? ' txBox="1"' : ''}/><p:nvPr/></p:nvSpPr><p:spPr><a:xfrm${r.chance(0.08) ? ' rot="5400000"' : ''}><a:off x="${b.x}" y="${b.y}"/><a:ext cx="${b.cx}" cy="${b.cy}"/></a:xfrm><a:prstGeom prst="${geom}"><a:avLst/></a:prstGeom><a:noFill/></p:spPr>${txBody(c, { levels: r.chance(0.35) })}</p:sp>`;
}

function tableXml(c, area) {
  const r = c.r;
  const id = c.nextId();
  const cols = r.int(1, 4), rows = r.int(1, 4);
  const b = box(c, area);
  const colW = Array.from({ length: cols }, () => r.int(300000, 2500000));
  const trs = [];
  for (let ri = 0; ri < rows; ri++) {
    const tcs = [];
    for (let ci = 0; ci < cols; ci++) {
      const span = ci < cols - 1 && r.chance(0.12) ? 2 : 1;
      const mar = r.chance(0.3) ? ` marL="${r.int(0, 200000)}" marR="${r.int(0, 200000)}" marT="${r.int(0, 100000)}" marB="${r.int(0, 100000)}"` : '';
      const anchor = r.chance(0.3) ? ` anchor="${r.pick(['t', 'ctr', 'b'])}"` : '';
      tcs.push(`<a:tc${span > 1 ? ` gridSpan="${span}"` : ''}>${txBody(c, { tag: 'a:txBody', paras: r.int(1, 3) })}<a:tcPr${mar}${anchor}/></a:tc>`);
      if (span > 1) { tcs.push(`<a:tc hMerge="1"><a:txBody><a:bodyPr/><a:lstStyle/><a:p><a:endParaRPr lang="en-US"/></a:p></a:txBody><a:tcPr/></a:tc>`); ci++; }
    }
    trs.push(`<a:tr h="${r.int(150000, 500000)}">${tcs.join('')}</a:tr>`);
  }
  c.feat('b3:table');
  return `<p:graphicFrame><p:nvGraphicFramePr><p:cNvPr id="${id}" name="Table ${id}"/><p:cNvGraphicFramePr><a:graphicFrameLocks noGrp="1"/></p:cNvGraphicFramePr><p:nvPr/></p:nvGraphicFramePr><p:xfrm><a:off x="${b.x}" y="${b.y}"/><a:ext cx="${b.cx}" cy="${b.cy}"/></p:xfrm><a:graphic><a:graphicData uri="http://schemas.openxmlformats.org/drawingml/2006/table"><a:tbl><a:tblPr firstRow="${r.below(2)}" bandRow="1"><a:tableStyleId>{5C22544A-7EE6-4342-B048-85BDC9FD1C3A}</a:tableStyleId></a:tblPr><a:tblGrid>${colW.map((w) => `<a:gridCol w="${w}"/>`).join('')}</a:tblGrid>${trs.join('')}</a:tbl></a:graphicData></a:graphic></p:graphicFrame>`;
}

/** A text-heavy slide (the `profile.slide` hook of b1.mjs `buildDeck`). */
export function b3Slide(c, t, spec, profile) {
  const r = c.r;
  const area = { w: t.w, h: t.h };
  const tops = [];
  for (const [attr] of spec.phs) {
    const type = /type="(\w+)"/.exec(attr)?.[1];
    const idx = /idx="(\d+)"/.exec(attr)?.[1];
    const isTitle = type === 'title' || type === 'ctrTitle';
    const id = c.nextId();
    const ph = `<p:ph${type ? ` type="${type}"` : ''}${idx !== undefined ? ` idx="${idx}"` : ''}/>`;
    tops.push(`<p:sp><p:nvSpPr><p:cNvPr id="${id}" name="Placeholder ${id}"/><p:cNvSpPr><a:spLocks noGrp="1"/></p:cNvSpPr><p:nvPr>${ph}</p:nvPr></p:nvSpPr><p:spPr/>${txBody(c, { ph: true, levels: !isTitle, paras: isTitle ? 1 : r.int(2, 9) })}</p:sp>`);
  }
  const n = r.int(profile.shapes[0], profile.shapes[1]);
  for (let i = 0; i < n; i++) tops.push(r.chance(0.12) ? tableXml(c, area) : shapeXml(c, area));
  return `${XML}<p:sld ${NS}><p:cSld><p:spTree>${GRP}${tops.join('')}</p:spTree></p:cSld><p:clrMapOvr><a:masterClrMapping/></p:clrMapOvr></p:sld>`;
}
