// Text bodies: body properties (insets, anchors, vertical text, columns, autofit, warp),
// paragraphs (levels, alignment, spacing, bullets and numbering, tabs), runs (size, bold, italic,
// underline, strike, spacing, baseline, caps, colour, typeface, hyperlinks, mixed scripts),
// fields and line breaks. Fonts are named, never embedded; open families only.
import { colorXml, esc, fillXml } from './dml.mjs';
import { randomPng } from '../png.mjs';

export const OPEN_FONTS = ['Liberation Sans', 'Liberation Serif', 'Liberation Mono', 'Carlito', 'Caladea'];

const LATIN = 'lorem ipsum dolor sit amet consectetur adipiscing elit sed do eiusmod tempor incididunt ut labore et dolore magna aliqua enim minim veniam quis nostrud exercitation ullamco laboris nisi aliquip ex ea commodo consequat duis aute irure in reprehenderit voluptate velit esse cillum fugiat nulla pariatur excepteur sint occaecat cupidatat non proident sunt culpa qui officia deserunt mollit anim id est laborum office affluent final waffle fjord ffi ﬁnal AVATAR Tomorrow 12,345.67 R$ 1.234,56 (a+b)/c 2025-11-17 e-mail naïve café coração ação'.split(' ');
const SCRIPTS = {
  greek: ['Αυτό', 'είναι', 'ένα', 'δείγμα', 'κειμένου', 'Ελληνικά', 'ΑΒΓΔ', 'ώρα', 'Ωμέγα'],
  cyrillic: ['Это', 'пример', 'текста', 'на', 'русском', 'языке', 'Привет', 'мир', 'ЖЩЮ'],
  arabic: ['هذا', 'نص', 'تجريبي', 'باللغة', 'العربية', 'مرحبا', 'بالعالم', 'السلام'],
  hebrew: ['זהו', 'טקסט', 'לדוגמה', 'בעברית', 'שלום', 'עולם'],
  cjk: ['这是', '一个', '测试', '文本', 'これは', 'テスト', 'です', '日本語', '한국어', '입니다', '漢字'],
  indic: ['यह', 'एक', 'परीक्षण', 'पाठ', 'है', 'नमस्ते', 'दुनिया', 'ภาษาไทย', 'สวัสดี'],
  symbols: ['😀', '✔', '→', '★', '©', '™', '½', '°C', '≤', '∑', '日本', 'é', 'é', 'ǆ', '​', '👨‍👩‍👧'],
};
const SCRIPT_LANG = { greek: 'el-GR', cyrillic: 'ru-RU', arabic: 'ar-SA', hebrew: 'he-IL', cjk: 'zh-CN', indic: 'hi-IN', symbols: 'en-US' };

export function words(r, n, scripts = 0.15) {
  const out = [];
  let script = null;
  for (let i = 0; i < n; i++) {
    if (r.chance(scripts / 3)) script = r.pick(Object.keys(SCRIPTS));
    else if (script && r.chance(0.4)) script = null;
    out.push(script ? r.pick(SCRIPTS[script]) : r.pick(LATIN));
  }
  return { text: out.join(' '), script };
}

const BULLET_CHARS = ['•', '–', '▪', '➢', '○', '■', '✓', '»', '·', '-'];
const AUTONUM = ['arabicPeriod', 'arabicParenR', 'arabicParenBoth', 'arabicPlain', 'romanUcPeriod', 'romanLcPeriod', 'alphaUcPeriod', 'alphaLcParenR', 'alphaLcPeriod', 'circleNumDbPlain', 'ea1JpnChsDbPeriod', 'hebrew2Minus', 'thaiNumPeriod'];

function bulletXml(c) {
  const r = c.r;
  const k = r.weighted([['none', 30], ['char', 40], ['auto', 22], ['blip', 3], ['inherit', 5]]);
  if (k === 'inherit') return '';
  c.feat('bullet:' + k);
  const pre = [];
  if (k !== 'none' && r.chance(0.3)) pre.push(`<a:buClr>${colorXml(c, { mods: false })}</a:buClr>`);
  if (k !== 'none' && r.chance(0.3)) pre.push(r.chance(0.5) ? `<a:buSzPct val="${r.int(25000, 400000)}"/>` : `<a:buSzPts val="${r.int(200, 4000)}"/>`);
  if (k !== 'none' && r.chance(0.3)) pre.push(r.chance(0.3) ? '<a:buFontTx/>' : `<a:buFont typeface="${r.pick(OPEN_FONTS)}"/>`);
  if (k === 'none') return '<a:buNone/>';
  if (k === 'char') return `${pre.join('')}<a:buChar char="${r.pick(BULLET_CHARS)}"/>`;
  if (k === 'auto') return `${pre.join('')}<a:buAutoNum type="${r.pick(AUTONUM)}"${r.chance(0.3) ? ` startAt="${r.int(1, 40)}"` : ''}/>`;
  const rId = c.addImage(randomPng(r));
  return `${pre.join('')}<a:buBlip><a:blip r:embed="${rId}"/></a:buBlip>`;
}

/** `<a:rPr>`-like attributes + children; `tag` is rPr, defRPr or endParaRPr. */
export function rPrXml(c, tag, { rich = true, lang } = {}) {
  const r = c.r;
  const at = [];
  if (lang) at.push(`lang="${lang}"`);
  else if (r.chance(0.3)) at.push(`lang="${r.pick(['en-US', 'pt-BR', 'es-ES', 'de-DE', 'ja-JP'])}"`);
  if (r.chance(0.7)) at.push(`sz="${r.weighted([[1200, 8], [1400, 8], [1800, 12], [2000, 10], [2400, 12], [2800, 8], [3200, 6], [4400, 4], [r.int(600, 9600), 10]])}"`);
  if (rich) {
    if (r.chance(0.2)) at.push(`b="${r.below(2)}"`);
    if (r.chance(0.2)) at.push(`i="${r.below(2)}"`);
    if (r.chance(0.15)) at.push(`u="${r.pick(['sng', 'dbl', 'heavy', 'dotted', 'dash', 'wavy', 'words', 'dottedHeavy', 'wavyDbl'])}"`);
    if (r.chance(0.1)) at.push(`strike="${r.pick(['sngStrike', 'dblStrike'])}"`);
    if (r.chance(0.12)) at.push(`spc="${r.int(-300, 1000)}"`);
    if (r.chance(0.1)) at.push(`baseline="${r.pick([30000, -25000, 40000, -30000])}"`);
    if (r.chance(0.08)) at.push(`cap="${r.pick(['all', 'small'])}"`);
    if (r.chance(0.06)) at.push(`kern="${r.pick([0, 1200, 2400])}"`);
  }
  const kids = [];
  if (r.chance(0.12) && rich) kids.push(`<a:ln w="${r.int(3175, 19050)}">${fillXml(c, 'solid')}</a:ln>`);
  if (r.chance(0.35)) kids.push(fillXml(c, r.chance(0.12) ? 'grad' : 'solid'));
  if (r.chance(0.06) && rich) { kids.push(`<a:effectLst><a:outerShdw blurRad="38100" dist="38100" dir="2700000" algn="tl"><a:srgbClr val="000000"><a:alpha val="43137"/></a:srgbClr></a:outerShdw></a:effectLst>`); c.feat('run:shadow'); }
  if (r.chance(0.05) && rich) { kids.push(`<a:highlight>${colorXml(c, { mods: false })}</a:highlight>`); c.feat('run:highlight'); }
  const font = r.weighted([['', 50], ['+mn-lt', 15], ['+mj-lt', 10], ['open', 25]]);
  if (font === 'open') kids.push(`<a:latin typeface="${r.pick(OPEN_FONTS)}"/>`);
  else if (font) kids.push(`<a:latin typeface="${font}"/>`);
  if (r.chance(0.08)) kids.push(`<a:ea typeface="${r.pick(OPEN_FONTS)}"/>`);
  if (r.chance(0.06)) kids.push(`<a:cs typeface="${r.pick(OPEN_FONTS)}"/>`);
  if (r.chance(0.04)) kids.push(`<a:sym typeface="${r.pick(OPEN_FONTS)}"/>`);
  if (r.chance(0.05) && rich && c.addLink) { kids.push(`<a:hlinkClick r:id="${c.addLink(`https://example.org/${r.int(1, 999)}`)}"/>`); c.feat('run:hyperlink'); }
  return kids.length || at.length ? `<a:${tag}${at.length ? ' ' + at.join(' ') : ''}${kids.length ? `>${kids.join('')}</a:${tag}>` : '/>'}` : `<a:${tag}/>`;
}

function pPrXml(c, { lvl, forceBullet } = {}) {
  const r = c.r;
  const at = [];
  if (lvl !== undefined && lvl > 0) at.push(`lvl="${lvl}"`);
  if (r.chance(0.4)) at.push(`algn="${r.pick(['l', 'ctr', 'r', 'just', 'dist', 'justLow', 'thaiDist'])}"`);
  if (r.chance(0.3)) { const marL = r.int(0, 1800000); at.push(`marL="${marL}"`); if (r.chance(0.7)) at.push(`indent="${-r.int(0, Math.min(marL, 600000))}"`); }
  if (r.chance(0.04)) at.push('rtl="1"');
  if (r.chance(0.05)) at.push(`eaLnBrk="${r.below(2)}" latinLnBrk="${r.below(2)}" hangingPunct="${r.below(2)}"`);
  if (r.chance(0.05)) at.push(`defTabSz="${r.pick([457200, 914400, 360000])}"`);
  const kids = [];
  if (r.chance(0.3)) kids.push(`<a:lnSpc>${r.chance(0.7) ? `<a:spcPct val="${r.int(60000, 250000)}"/>` : `<a:spcPts val="${r.int(600, 7200)}"/>`}</a:lnSpc>`);
  if (r.chance(0.3)) kids.push(`<a:spcBef>${r.chance(0.5) ? `<a:spcPct val="${r.int(0, 200000)}"/>` : `<a:spcPts val="${r.int(0, 3600)}"/>`}</a:spcBef>`);
  if (r.chance(0.3)) kids.push(`<a:spcAft>${r.chance(0.5) ? `<a:spcPct val="${r.int(0, 200000)}"/>` : `<a:spcPts val="${r.int(0, 3600)}"/>`}</a:spcAft>`);
  if (forceBullet || r.chance(0.5)) kids.push(bulletXml(c));
  if (r.chance(0.12)) {
    const n = r.int(1, 4);
    let pos = 0;
    const tabs = [];
    for (let i = 0; i < n; i++) { pos += r.int(200000, 1500000); tabs.push(`<a:tab pos="${pos}" algn="${r.pick(['l', 'ctr', 'r', 'dec'])}"/>`); }
    kids.push(`<a:tabLst>${tabs.join('')}</a:tabLst>`);
    c.feat('para:tabs');
  }
  if (r.chance(0.15)) kids.push(rPrXml(c, 'defRPr', { rich: false }));
  return at.length || kids.length ? `<a:pPr${at.length ? ' ' + at.join(' ') : ''}${kids.length ? `>${kids.join('')}</a:pPr>` : '/>'}` : '';
}

function runXml(c, { rich = true } = {}) {
  const r = c.r;
  const k = r.weighted([['r', 82], ['br', 6], ['fld', 6], ['rtl', 6]]);
  if (k === 'br') { c.feat('run:break'); return `<a:br>${rPrXml(c, 'rPr', { rich: false })}</a:br>`; }
  if (k === 'fld') {
    c.feat('run:field');
    const type = r.pick(['slidenum', 'datetime1', 'datetime4', 'datetime13', 'datetimeFigureOut']);
    const txt = type === 'slidenum' ? '‹#›' : r.pick(['11/17/2026', 'November 17, 2026', '17/11/2026']);
    return `<a:fld id="${r.guid()}" type="${type}">${rPrXml(c, 'rPr', { rich: false })}<a:t>${esc(txt)}</a:t></a:fld>`;
  }
  const n = r.weighted([[1, 30], [2, 25], [3, 20], [5, 15], [9, 10]]);
  const w = words(r, n, k === 'rtl' ? 1 : 0.12);
  if (w.script) c.feat('script:' + w.script);
  const text = k === 'rtl' ? `${r.pick(SCRIPTS.arabic)} ${w.text}` : w.text;
  const lang = w.script && r.chance(0.7) ? SCRIPT_LANG[w.script] : undefined;
  return `<a:r>${rPrXml(c, 'rPr', { rich, lang })}<a:t>${esc(text + (r.chance(0.3) ? ' ' : ''))}</a:t></a:r>`;
}

/** `<a:p>` with 0-4 runs, optional properties and an end-paragraph run property. */
export function paragraphXml(c, { lvl, rich = true, forceBullet, maxRuns = 4 } = {}) {
  const r = c.r;
  const n = r.weighted([[0, 4], [1, 45], [2, 30], [3, 15], [4, 6]]);
  const runs = [];
  for (let i = 0; i < Math.min(n, maxRuns); i++) runs.push(runXml(c, { rich }));
  const end = r.chance(0.3) ? rPrXml(c, 'endParaRPr', { rich: false }) : '';
  return `<a:p>${pPrXml(c, { lvl, forceBullet })}${runs.join('')}${end}</a:p>`;
}

/** `<a:lstStyle>`: list-level defaults for a few levels. */
export function lstStyleXml(c) {
  const r = c.r;
  if (!r.chance(0.2)) return '<a:lstStyle/>';
  c.feat('text:lststyle');
  const lv = [];
  const n = r.int(1, 3);
  for (let i = 1; i <= n; i++) {
    const at = [];
    if (r.chance(0.5)) at.push(`marL="${r.int(0, 1500000)}"`);
    if (r.chance(0.5)) at.push(`indent="${-r.int(0, 400000)}"`);
    if (r.chance(0.4)) at.push(`algn="${r.pick(['l', 'ctr', 'r', 'just'])}"`);
    lv.push(`<a:lvl${i}pPr ${at.join(' ')}>${r.chance(0.5) ? bulletXml(c) : ''}${rPrXml(c, 'defRPr', { rich: true })}</a:lvl${i}pPr>`);
  }
  return `<a:lstStyle>${lv.join('')}</a:lstStyle>`;
}

const WARPS = ['textPlain', 'textStop', 'textTriangle', 'textArchUp', 'textCircle', 'textWave1', 'textInflate', 'textFadeUp', 'textSlantUp', 'textCanUp', 'textCascadeDown', 'textDeflate'];

/** `<a:bodyPr>` with insets, anchors, vertical text, columns, autofit, warp. */
export function bodyPrXml(c, { ph = false } = {}) {
  const r = c.r;
  const at = [];
  if (r.chance(ph ? 0.1 : 0.2)) at.push(`wrap="${r.weighted([['square', 7], ['none', 3]])}"`);
  if (r.chance(0.3)) at.push(`lIns="${r.int(0, 300000)}" tIns="${r.int(0, 200000)}" rIns="${r.int(0, 300000)}" bIns="${r.int(0, 200000)}"`);
  if (r.chance(0.4)) at.push(`anchor="${r.weighted([['t', 4], ['ctr', 4], ['b', 3], ['just', 1], ['dist', 1]])}"`);
  if (r.chance(0.08)) at.push('anchorCtr="1"');
  if (r.chance(0.08)) { const v = r.pick(['vert', 'vert270', 'eaVert', 'wordArtVert', 'mongolianVert', 'wordArtVertRtl']); at.push(`vert="${v}"`); c.feat('text:vert'); }
  if (r.chance(0.06)) at.push(`rot="${r.int(0, 21599999)}"`);
  if (r.chance(0.08)) { at.push(`numCol="${r.int(2, 4)}" spcCol="${r.int(0, 457200)}"`); c.feat('text:columns'); }
  if (r.chance(0.04)) at.push('rtlCol="1"');
  if (r.chance(0.04)) at.push('upright="1"');
  if (r.chance(0.04)) at.push('fromWordArt="1"');
  const kids = [];
  const warp = r.chance(0.04);
  if (warp) { kids.push(`<a:prstTxWarp prst="${r.pick(WARPS)}"><a:avLst/></a:prstTxWarp>`); c.feat('text:warp'); }
  const fit = r.weighted([['', 50], ['norm', 18], ['normScale', 8], ['sp', 12], ['no', 12]]);
  if (fit === 'norm') kids.push('<a:normAutofit/>');
  else if (fit === 'normScale') kids.push(`<a:normAutofit fontScale="${r.pick([92500, 85000, 77500, 70000, 62500, 55000, 47500, 40000, 25000])}" lnSpcReduction="${r.pick([0, 10000, 20000])}"/>`);
  else if (fit === 'sp') kids.push('<a:spAutoFit/>');
  else if (fit === 'no') kids.push('<a:noAutofit/>');
  if (fit) c.feat('text:autofit:' + fit);
  return kids.length || at.length ? `<a:bodyPr${at.length ? ' ' + at.join(' ') : ''}${kids.length ? `>${kids.join('')}</a:bodyPr>` : '/>'}` : '<a:bodyPr/>';
}

/**
 * A full `<p:txBody>` / `<a:txBody>`: `tag` is the element name. A placeholder body carries only
 * the paragraphs; the properties it inherits are left out (`ph`).
 */
export function txBodyXml(c, { tag = 'p:txBody', ph = false, paras, levels = false } = {}) {
  const r = c.r;
  const n = paras ?? r.weighted([[1, 30], [2, 25], [3, 20], [4, 15], [6, 10]]);
  const ps = [];
  for (let i = 0; i < n; i++) {
    const lvl = levels ? r.weighted([[0, 50], [1, 25], [2, 15], [3, 6], [4, 2], [r.int(5, 8), 2]]) : undefined;
    ps.push(paragraphXml(c, { lvl, forceBullet: levels && r.chance(0.3) }));
  }
  if (levels) c.feat('text:levels');
  return `<${tag}>${bodyPrXml(c, { ph })}${lstStyleXml(c)}${ps.join('')}</${tag}>`;
}
