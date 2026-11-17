// Seeded chart generator of the uo-charts parity harness (record uo-charts.md "Generator", group `c1a`): raw
// `c:chartSpace` / `cx:chartSpace` text, one chart per seed. No Date, no Math.random: same seed -> same bytes.
//
// Coverage is DETERMINISTIC across rounds: chart `i` uses family `i % F` and round `r = floor(i / F)`; every option list is
// walked with `cx.cyc(list, salt)` (list[(r + salt) % length]) so that, with 600 charts and ~50 families, each option of
// each list is hit by several charts; the random generator only fills in data and secondary choices. The families are
// listed in FAMILIES; the modifiers (axes, labels, text, styles, paints, data) are shared helpers applied by the families.
//
// `buildChart(seed)` -> { xml, host, themeName, fileFormat, images: {rId: {part, bytes, mime}}, userShapesXml, features }.
import { Rand } from '../../../uo-pptx/js/lib/gen/rand.mjs';
import { NS, el, makePng, makeJpeg, text as tx, val } from './xml.mjs';

export const SEED_BASE = 700000;
export const C1A_COUNT = 600;

const NUMFMTS = ['General', '0', '0.00', '#,##0', '#,##0.00', '0%', '0.0%', '"R$" #,##0.00', 'm/d/yyyy', 'd/m/yyyy', '[Red]0.00;[Blue]-0.00', '0.00E+00', '@',
  '#,##0;(#,##0)', '$#,##0.00', 'dd/mm/yyyy', 'mmm-yy', 'h:mm', '0.0,"k"', '0.00 "units"', 'General', 'yyyy-mm-dd', '[$-409]d-mmm;@', '0.000'];
const SCHEME = ['tx1', 'bg1', 'tx2', 'bg2', 'accent1', 'accent2', 'accent3', 'accent4', 'accent5', 'accent6', 'dk1', 'lt1', 'dk2', 'lt2', 'hlink', 'folHlink', 'phClr'];
const FONTS = ['Calibri', 'Cambria', 'Arial', 'Times New Roman', 'Courier New', '+mn-lt', '+mj-lt', 'Segoe UI', 'Aptos', 'Liberation Serif'];
const WORDS = ['Alpha', 'Bravo', 'Charlie', 'Delta', 'Echo', 'Foxtrot', 'Golf', 'Hotel', 'India', 'Juliet', 'Kilo', 'Lima', 'Mike', 'November', 'Oscar', 'Papa', 'Quebec', 'Romeo', 'Sierra', 'Tango'];
const LONG = ['Supercalifragilisticexpialidocious-category label that wraps', 'A very long category name with many words that must wrap onto lines', 'Receita líquida operacional - 1º trimestre', 'Ünïcödé Ç ã õ 日本語 label', 'x\ny two lines', 'Emoji \u{1F600} astral', 'ab-cd-ef-gh-ij-kl-mn', 'W'.repeat(40)];

class Cx {
  constructor(seed, i, F) {
    this.i = i; this.r = Math.floor(i / F); this.rnd = new Rand(seed); this.feat = new Set(); this.images = {}; this.nimg = 0;
    this.rels = 0;
  }
  cyc(list, salt = 0) { return list[(this.r + salt) % list.length]; }
  /** cycles over the list but with every `n`th round skipping to a random pick (so combinations differ) */
  mix(list, salt = 0) { return this.rnd.chance(0.5) ? this.cyc(list, salt) : this.rnd.pick(list); }
  f(...names) { for (const n of names) this.feat.add(n); }
  /** registers a picture; returns its rId */
  picture(bytes, mime) {
    const rid = `rId${++this.rels}`;
    const ext = mime === 'image/png' ? 'png' : 'jpeg';
    this.images[rid] = { part: `ppt/media/image${++this.nimg}.${ext}`, bytes, mime };
    return rid;
  }
}

// ------------------------------------------------------------------------------------------------ colours and fills
function clr(cx, spec) {
  if (spec === undefined) spec = cx.rnd.pick(['A', 'S', 'S', 'S', 'M', 'P', 'Y', 'H', 'R']);
  switch (spec[0] === '#' ? 'hex' : spec) {
    case 'hex': return el('a:srgbClr', { val: spec.slice(1) });
    case 'A': return el('a:srgbClr', { val: cx.rnd.hex6() });
    case 'S': return el('a:schemeClr', { val: cx.rnd.pick(SCHEME) }, mods(cx));
    case 'M': return el('a:schemeClr', { val: cx.rnd.pick(SCHEME.slice(0, 10)) }, mods(cx, true));
    case 'P': return el('a:prstClr', { val: cx.rnd.pick(['black', 'white', 'red', 'green', 'blue', 'gray', 'orange', 'navy']) }, mods(cx));
    case 'Y': return el('a:sysClr', { val: cx.rnd.pick(['windowText', 'window']), lastClr: cx.rnd.pick(['000000', 'FFFFFF']) });
    case 'H': return el('a:hslClr', { hue: cx.rnd.int(0, 21600000), sat: cx.rnd.int(0, 100000), lum: cx.rnd.int(0, 100000) });
    case 'R': return el('a:scrgbClr', { r: cx.rnd.int(0, 100000), g: cx.rnd.int(0, 100000), b: cx.rnd.int(0, 100000) });
    default: return el('a:schemeClr', { val: spec }, mods(cx));
  }
}
const MODS = [['lumMod', 20000, 120000], ['lumOff', 10000, 80000], ['tint', 10000, 90000], ['shade', 10000, 90000], ['alpha', 10000, 100000], ['satMod', 50000, 150000], ['hueOff', -500000, 500000], ['comp'], ['inv'], ['gray']];
function mods(cx, force) {
  if (!force && !cx.rnd.chance(0.45)) return null;
  const out = [];
  for (const m of cx.rnd.pickN(MODS, cx.rnd.int(1, 3))) { out.push(m.length === 1 ? el(`a:${m[0]}`) : el(`a:${m[0]}`, { val: cx.rnd.int(m[1], m[2]) })); }
  if (out.length) cx.f('colorMods');
  return out;
}

function stops(cx, n, nan) {
  const out = [];
  const pos = Array.from({ length: n }, (_, k) => Math.round((k / Math.max(1, n - 1)) * 100000));
  if (cx.rnd.chance(0.3)) pos.reverse();
  pos.forEach((p, k) => out.push(el('a:gs', { pos: nan && k === 1 ? 'x' : p }, clr(cx))));
  return el('a:gsLst', {}, out);
}

function fill(cx, kind) {
  kind ??= cx.rnd.weighted([['solid', 6], ['none', 1], ['grad', 3], ['gradPath', 1], ['patt', 1], ['blipStretch', 1], ['blipTile', 1], ['solidAlpha', 1]]);
  switch (kind) {
    case 'none': cx.f('fill:none'); return el('a:noFill');
    case 'solid': return el('a:solidFill', {}, clr(cx));
    case 'solidAlpha': cx.f('fill:alpha'); return el('a:solidFill', {}, el('a:srgbClr', { val: cx.rnd.hex6() }, el('a:alpha', { val: cx.rnd.int(5, 95) * 1000 })));
    case 'grad': cx.f('fill:gradLin'); return el('a:gradFill', { rotWithShape: 1 }, stops(cx, cx.rnd.int(2, 4), cx.rnd.chance(0.03)), el('a:lin', { ang: cx.rnd.pick([0, 5400000, 2700000, 10800000, 16200000, 19000000]), scaled: 0 }));
    case 'gradPath': cx.f('fill:gradPath'); return el('a:gradFill', {}, stops(cx, 3), el('a:path', { path: cx.rnd.pick(['circle', 'rect', 'shape']) }, el('a:fillToRect', { l: 50000, t: 50000, r: 50000, b: 50000 })));
    case 'patt': cx.f('fill:patt'); return el('a:pattFill', { prst: cx.rnd.pick(['pct5', 'ltDnDiag', 'dkVert', 'cross', 'wave', 'smCheck', 'ltHorz']) }, el('a:fgClr', {}, clr(cx)), el('a:bgClr', {}, clr(cx)));
    case 'blipStretch': {
      cx.f('fill:blipStretch');
      const rid = cx.picture(makePng(cx.rnd.int(8, 64), cx.rnd.int(8, 64), cx.rnd.pick([undefined, 96, 72, 150])), 'image/png');
      return el('a:blipFill', {}, el('a:blip', { 'r:embed': rid }), cx.rnd.chance(0.5) ? el('a:srcRect', { l: 5000 }) : null, el('a:stretch', {}, cx.rnd.chance(0.5) ? el('a:fillRect', { l: cx.rnd.int(-10000, 10000), t: cx.rnd.int(0, 10000), r: 0, b: 0 }) : el('a:fillRect')));
    }
    case 'blipTile': {
      cx.f('fill:blipTile');
      const png = cx.rnd.chance(0.5);
      const rid = png ? cx.picture(makePng(cx.rnd.int(8, 48), cx.rnd.int(8, 48), cx.rnd.pick([undefined, 96, 75, 300])), 'image/png')
        : cx.picture(makeJpeg(cx.rnd.int(16, 80), cx.rnd.int(16, 80), cx.rnd.pick([{ units: 1, dens: 96 }, { units: 2, dens: 38 }, { units: 0, dens: 1, exif: 72 }, {}])), 'image/jpeg');
      return el('a:blipFill', {}, el('a:blip', { 'r:embed': rid }), el('a:tile', { tx: cx.rnd.int(-500000, 500000), ty: cx.rnd.int(0, 500000), sx: cx.rnd.pick([100000, 50000, 200000, 33333]), sy: cx.rnd.pick([100000, 50000, 150000]), flip: cx.rnd.pick(['none', 'x', 'y', 'xy']), algn: cx.rnd.pick(['tl', 'ctr', 'br', 't', 'r']) }));
    }
    default: return el('a:solidFill', {}, clr(cx));
  }
}

function line(cx, o = {}) {
  const w = o.w ?? cx.rnd.weighted([[9525, 3], [12700, 3], [28575, 2], [0, 1], [-1, 1], [3175, 1], [38100, 1]]);
  const f = o.fill ?? (cx.rnd.chance(0.12) ? el('a:noFill') : el('a:solidFill', {}, clr(cx)));
  const dash = cx.rnd.chance(0.3) ? el('a:prstDash', { val: cx.rnd.pick(['dash', 'sysDash', 'dot', 'sysDot', 'dashDot', 'lgDash', 'lgDashDot', 'solid', 'sysDashDot']) }) : null;
  const join = cx.rnd.chance(0.25) ? cx.rnd.pick([el('a:round'), el('a:bevel'), el('a:miter', { lim: 800000 })]) : null;
  return el('a:ln', { w: w < 0 ? undefined : w, cap: cx.rnd.chance(0.3) ? cx.rnd.pick(['rnd', 'sq', 'flat']) : undefined }, f, dash, join);
}

function spPr(cx, o = {}) {
  const p = o.p ?? 0.7;
  if (!cx.rnd.chance(p)) return null;
  return el('c:spPr', {}, o.fill === false ? null : (o.fillKind ? fill(cx, o.fillKind) : fill(cx)), o.line === false ? null : (cx.rnd.chance(o.lineP ?? 0.5) ? line(cx) : null), cx.rnd.chance(o.shadowP ?? 0.08) ? shadow(cx) : null);
}
function shadow(cx) {
  cx.f('shadow');
  return el('a:effectLst', {}, el('a:outerShdw', { blurRad: cx.rnd.int(0, 80000), dist: cx.rnd.int(0, 60000), dir: cx.rnd.int(0, 21600000), algn: 'tl', rotWithShape: 0 }, el('a:srgbClr', { val: '000000' }, el('a:alpha', { val: cx.rnd.int(20, 70) * 1000 }))));
}

// ------------------------------------------------------------------------------------------------ text
function txPr(cx, o = {}) {
  if (!cx.rnd.chance(o.p ?? 0.55)) return null;
  const sz = o.sz ?? cx.rnd.pick([800, 900, 1000, 1200, 1400, 1800, 2400, 600, 1100]);
  const rp = el('a:defRPr', { sz, b: cx.rnd.chance(0.25) ? 1 : cx.rnd.chance(0.1) ? 0 : undefined, i: cx.rnd.chance(0.15) ? 1 : undefined, u: cx.rnd.chance(0.1) ? 'sng' : undefined },
    cx.rnd.chance(0.5) ? el('a:solidFill', {}, clr(cx)) : null, cx.rnd.chance(0.35) ? el('a:latin', { typeface: cx.rnd.pick(FONTS) }) : null);
  const body = o.rot !== undefined ? { rot: o.rot, vert: o.vert ?? 'horz' } : cx.rnd.chance(0.1) ? { rot: cx.rnd.pick([-5400000, 5400000, 2700000, -2700000, 0, 60000000]), vert: cx.rnd.pick(['horz', 'vert', 'eaVert']) } : {};
  if (body.rot) cx.f('text:rot');
  return el('c:txPr', {}, el('a:bodyPr', body), el('a:lstStyle'), el('a:p', {}, el('a:pPr', {}, rp), el('a:endParaRPr', { lang: 'en-US' })));
}

function richPara(cx, t, o = {}) {
  const runs = [];
  const parts = String(t).split('\u0001');
  parts.forEach((part, k) => {
    if (k) runs.push(el('a:br'));
    if (part) runs.push(el('a:r', {}, el('a:rPr', { lang: 'en-US', sz: o.sz, b: o.b, i: o.i, u: o.u }, o.color ? el('a:solidFill', {}, clr(cx, o.color)) : null, o.font ? el('a:latin', { typeface: o.font }) : null), tx('a:t', {}, part)));
  });
  return el('a:p', {}, el('a:pPr', {}, o.def ?? el('a:defRPr')), runs);
}

function title(cx, kind, o = {}) {
  const t = o.text ?? cx.rnd.pick(['Sales by region', 'Receita (R$ mil)', 'Y', 'Gráfico de colunas', 'A & B < C', 'Long title that is meant to wrap across several lines in a narrow chart area']);
  const layout = cx.rnd.chance(0.15) ? manual(cx, true) : null;
  switch (kind) {
    case 'rich': cx.f('title:rich'); return el('c:title', {}, el('c:tx', {}, el('c:rich', {}, el('a:bodyPr', o.body), el('a:lstStyle'), richPara(cx, t, { sz: cx.rnd.pick([undefined, 1400, 1800, 2400]), b: cx.rnd.pick([undefined, 1, 0]), color: cx.rnd.chance(0.4) ? 'S' : undefined }))), layout, o.overlay ? val('c:overlay', 1) : null, spPr(cx, { p: 0.2 }));
    case 'defRPr': cx.f('title:defRPr'); return el('c:title', {}, el('c:tx', {}, el('c:rich', {}, el('a:bodyPr'), el('a:lstStyle'), el('a:p', {}, el('a:pPr', {}, el('a:defRPr', { sz: 1600, b: 1 }, el('a:solidFill', {}, clr(cx)))), el('a:r', {}, el('a:rPr', { lang: 'en-US' }), tx('a:t', {}, t))))), layout, val('c:overlay', 0));
    case 'multi': cx.f('title:multi'); return el('c:title', {}, el('c:tx', {}, el('c:rich', {}, el('a:bodyPr'), el('a:lstStyle'), richPara(cx, 'First \u0001line two', { sz: 1400 }), el('a:p', {}, el('a:pPr', {}, el('a:defRPr')), el('a:fld', { id: '{00000000-0000-0000-0000-000000000000}', type: 'CELLRANGE' }, el('a:rPr', { lang: 'en-US' }), tx('a:t', {}, '[CELLRANGE]')))), ), val('c:overlay', 0));
    case 'strRef': cx.f('title:strRef'); return el('c:title', {}, el('c:tx', {}, el('c:strRef', {}, tx('c:f', {}, 'Sheet1!$B$1'), el('c:strCache', {}, val('c:ptCount', 1), el('c:pt', { idx: 0 }, tx('c:v', {}, t))))), val('c:overlay', 0));
    case 'auto': cx.f('title:auto'); return el('c:title', {}, val('c:overlay', 0));
    case 'overlay': cx.f('title:overlay'); return el('c:title', {}, el('c:tx', {}, el('c:rich', {}, el('a:bodyPr'), el('a:lstStyle'), richPara(cx, t, {}))), val('c:overlay', 1));
    default: return null;
  }
}
const TITLE_KINDS = ['rich', 'defRPr', 'auto', 'multi', 'strRef', 'overlay', 'none', 'rich', 'deleted'];

function manual(cx, small) {
  cx.f('layout:manual');
  const x = cx.rnd.int(0, 60) / 100, y = cx.rnd.int(0, 60) / 100;
  return el('c:layout', {}, el('c:manualLayout', {}, cx.rnd.chance(0.5) ? val('c:layoutTarget', cx.rnd.pick(['inner', 'outer'])) : null, val('c:xMode', cx.rnd.pick(['edge', 'edge', 'factor'])), val('c:yMode', cx.rnd.pick(['edge', 'edge', 'factor'])),
    val('c:x', x), val('c:y', y), small || cx.rnd.chance(0.3) ? null : val('c:w', cx.rnd.int(20, 100 - Math.floor(x * 100)) / 100), small || cx.rnd.chance(0.3) ? null : val('c:h', cx.rnd.int(20, 100 - Math.floor(y * 100)) / 100)));
}

function legend(cx, nSeries, o = {}) {
  const kind = o.kind ?? cx.rnd.pick(['r', 'l', 't', 'b', 'tr', 'r', 'b', 'overlay', 'manual', 'entries', 'long']);
  if (kind === 'none') return null;
  cx.f(`legend:${kind}`);
  const pos = ['r', 'l', 't', 'b', 'tr'].includes(kind) ? kind : cx.rnd.pick(['r', 'b', 't']);
  const entries = kind === 'entries' && nSeries > 1 ? Array.from({ length: Math.min(nSeries - 1, 2) }, (_, k) => el('c:legendEntry', {}, val('c:idx', k * 2), val('c:delete', 1))) : null;
  return el('c:legend', {}, val('c:legendPos', pos), entries, kind === 'manual' ? manual(cx) : null, val('c:overlay', kind === 'overlay' ? 1 : 0), spPr(cx, { p: 0.3 }), txPr(cx, { p: 0.4 }));
}

// ------------------------------------------------------------------------------------------------ data
function nums(cx, n, o = {}) {
  const mode = o.mode ?? cx.cyc(['int', 'dec', 'big', 'neg', 'tiny', 'flat', 'zero', 'mixed', 'huge', 'sorted'], o.salt ?? 0);
  const out = [];
  for (let k = 0; k < n; k++) {
    let v;
    switch (mode) {
      case 'int': v = cx.rnd.int(1, 100); break;
      case 'dec': v = cx.rnd.int(0, 10000) / 1000; break;
      case 'big': v = cx.rnd.int(1000, 9999999); break;
      case 'neg': v = cx.rnd.int(-60, 100); break;
      case 'tiny': v = cx.rnd.int(1, 999) / 1e6; break;
      case 'flat': v = 42; break;
      case 'zero': v = 0; break;
      case 'huge': v = k === 0 ? 1e300 : cx.rnd.int(1, 100); break;
      case 'sorted': v = (k + 1) * cx.rnd.int(1, 9); break;
      default: v = cx.rnd.chance(0.2) ? -cx.rnd.int(1, 30) : cx.rnd.int(0, 500) / 4;
    }
    if (o.blanks && cx.rnd.chance(o.blanks)) v = null;
    out.push(v);
  }
  if (mode === 'neg') cx.f('data:negative'); else if (mode === 'zero') cx.f('data:zero'); else if (mode === 'flat') cx.f('data:flat'); else if (mode === 'huge') cx.f('data:1e300');
  if (o.blanks) cx.f('data:blanks');
  return out;
}

const numCache = (cx, vals, fmt, o = {}) => {
  const n = o.ptCount ?? vals.length;
  return el('c:numCache', {}, tx('c:formatCode', {}, fmt ?? 'General'), val('c:ptCount', n),
    vals.map((v, k) => (v === null || v === undefined ? null : el('c:pt', { idx: o.idx?.[k] ?? k, formatCode: o.ptFmt ? o.ptFmt : undefined }, tx('c:v', {}, o.nanText && k === 1 ? 'abc' : String(v))))));
};
const numRef = (cx, vals, f, fmt, o) => el('c:numRef', {}, tx('c:f', {}, f), numCache(cx, vals, fmt, o));
const strCache = (cats, n) => el('c:strCache', {}, val('c:ptCount', n ?? cats.length), cats.map((c, k) => (c === null ? null : el('c:pt', { idx: k }, tx('c:v', {}, c)))));
const strRef = (cats, f) => el('c:strRef', {}, tx('c:f', {}, f), strCache(cats));

function catLabels(cx, n, mode) {
  mode ??= cx.cyc(['words', 'num', 'long', 'words', 'short', 'blankgaps', 'dates'], 3);
  const out = [];
  for (let k = 0; k < n; k++) {
    if (mode === 'num') out.push(String(k + 1));
    else if (mode === 'long') out.push(LONG[k % LONG.length]);
    else if (mode === 'short') out.push(String.fromCharCode(65 + (k % 26)));
    else if (mode === 'blankgaps') out.push(k % 3 === 1 ? null : WORDS[k % WORDS.length]);
    else out.push(`${WORDS[k % WORDS.length]}${k >= WORDS.length ? k : ''}`);
  }
  return out;
}

function cat(cx, n, o = {}) {
  const kind = o.kind ?? cx.cyc(['str', 'str', 'num', 'multi', 'lit', 'dateNum', 'strlong'], 1);
  if (kind === 'num') { cx.f('cat:num'); return el('c:cat', {}, numRef(cx, Array.from({ length: n }, (_, k) => k + 1), 'Sheet1!$A$2:$A$' + (n + 1), 'General')); }
  if (kind === 'dateNum') { cx.f('cat:date'); return el('c:cat', {}, numRef(cx, Array.from({ length: n }, (_, k) => 43831 + k * (o.step ?? 31)), 'Sheet1!$A$2:$A$' + (n + 1), cx.rnd.pick(['m/d/yyyy', 'mmm-yy', 'd/m/yyyy', 'General']))); }
  if (kind === 'lit') { cx.f('cat:lit'); return el('c:cat', {}, el('c:strLit', {}, val('c:ptCount', n), catLabels(cx, n, 'words').map((c, k) => el('c:pt', { idx: k }, tx('c:v', {}, c))))); }
  if (kind === 'multi') {
    cx.f('cat:multiLevel');
    const inner = catLabels(cx, n, 'short'), outer = catLabels(cx, Math.ceil(n / 3), 'words');
    return el('c:cat', {}, el('c:multiLvlStrRef', {}, tx('c:f', {}, 'Sheet1!$A$2:$B$' + (n + 1)), el('c:multiLvlStrCache', {}, val('c:ptCount', n),
      el('c:lvl', {}, inner.map((c, k) => el('c:pt', { idx: k }, tx('c:v', {}, c)))), el('c:lvl', {}, outer.map((c, k) => el('c:pt', { idx: k * 3 }, tx('c:v', {}, c)))))));
  }
  const labels = catLabels(cx, n, kind === 'strlong' ? 'long' : o.mode);
  if (kind === 'strlong') cx.f('cat:long');
  return el('c:cat', {}, strRef(labels, 'Sheet1!$A$2:$A$' + (n + 1)));
}

// ------------------------------------------------------------------------------------------------ labels, markers, trendlines
const DLBL_POS = ['ctr', 'inEnd', 'inBase', 'outEnd', 'l', 'r', 't', 'b', 'bestFit'];
function dLbls(cx, o = {}) {
  if (!cx.rnd.chance(o.p ?? 0.6)) return null;
  const pie = o.pie;
  const show = cx.rnd.pick([{ val: 1 }, { val: 1, cat: 1 }, { cat: 1 }, { ser: 1, val: 1 }, { val: 1, pct: 1 }, { pct: 1 }, { val: 1, cat: 1, ser: 1, key: 1 }, {}]);
  const pos = cx.rnd.pick(pie ? ['ctr', 'inEnd', 'outEnd', 'bestFit', undefined] : o.stacked ? ['ctr', 'inEnd', 'inBase', undefined] : o.line ? ['t', 'b', 'l', 'r', 'ctr', undefined] : DLBL_POS.slice(0, 5).concat([undefined]));
  const fmt = cx.rnd.chance(0.5) ? cx.rnd.pick(NUMFMTS) : null;
  cx.f('dLbls');
  const pts = o.points ? Array.from({ length: Math.min(o.points, 3) }, (_, k) => {
    const kind = cx.rnd.pick(['plain', 'rich', 'delete', 'layout', 'c15']);
    cx.f(`dLbl:${kind}`);
    if (kind === 'delete') return el('c:dLbl', {}, val('c:idx', k), val('c:delete', 1));
    return el('c:dLbl', {}, val('c:idx', k), kind === 'layout' ? manual(cx, true) : null,
      kind === 'rich' ? el('c:tx', {}, el('c:rich', {}, el('a:bodyPr'), el('a:lstStyle'), el('a:p', {}, el('a:fld', { id: '{11111111-1111-1111-1111-111111111111}', type: cx.rnd.pick(['CATEGORYNAME', 'VALUE', 'SERIESNAME', 'PERCENTAGE', 'CELLRANGE']) }, el('a:rPr'), tx('a:t', {}, '[X]')), el('a:r', {}, el('a:rPr'), tx('a:t', {}, ' custom'))))) : null,
      fmt ? el('c:numFmt', { formatCode: fmt, sourceLinked: 0 }) : null, spPr(cx, { p: 0.2 }), txPr(cx, { p: 0.3 }), pos ? val('c:dLblPos', pos) : null,
      val('c:showLegendKey', show.key ? 1 : 0), val('c:showVal', show.val ? 1 : 0), val('c:showCatName', show.cat ? 1 : 0), val('c:showSerName', show.ser ? 1 : 0), val('c:showPercent', show.pct ? 1 : 0), val('c:showBubbleSize', 0),
      kind === 'c15' ? el('c:extLst', {}, el('c:ext', { uri: '{CE6537A1-D6FC-4f65-9D91-7224C49458BB}' }, el('c15:layout', {}, el('c:manualLayout', {}, val('c:w', 0.2), val('c:h', 0.1))))) : null);
  }) : null;
  return el('c:dLbls', {}, pts, fmt ? el('c:numFmt', { formatCode: fmt, sourceLinked: cx.rnd.pick([0, 1]) }) : null, spPr(cx, { p: 0.25 }), txPr(cx, { p: 0.4 }), pos ? val('c:dLblPos', pos) : null,
    val('c:showLegendKey', show.key ? 1 : 0), val('c:showVal', show.val ? 1 : 0), val('c:showCatName', show.cat ? 1 : 0), val('c:showSerName', show.ser ? 1 : 0), val('c:showPercent', show.pct ? 1 : 0), val('c:showBubbleSize', o.bubble ? 1 : 0),
    cx.rnd.chance(0.3) ? tx('c:separator', {}, cx.rnd.pick([', ', '; ', '\n', ' - '])) : null,
    pie && cx.rnd.chance(0.5) ? val('c:showLeaderLines', 1) : null, pie && cx.rnd.chance(0.3) ? el('c:leaderLines', {}, el('c:spPr', {}, line(cx))) : null);
}

const SYMBOLS = ['circle', 'dash', 'diamond', 'dot', 'none', 'picture', 'plus', 'square', 'star', 'triangle', 'x', 'auto'];
function marker(cx, o = {}) {
  const sym = o.symbol ?? cx.rnd.pick(SYMBOLS);
  if (sym === 'auto') return null;
  cx.f(`marker:${sym}`);
  return el('c:marker', {}, val('c:symbol', sym), sym === 'none' ? null : val('c:size', cx.rnd.pick([2, 5, 7, 9, 12, 20, 72, 1])), cx.rnd.chance(0.6) ? el('c:spPr', {}, fill(cx), cx.rnd.chance(0.5) ? line(cx) : null) : null);
}

const TREND_KINDS = ['linear', 'poly', 'exp', 'log', 'power', 'movingAvg'];
function trendlines(cx, o = {}) {
  if (!cx.rnd.chance(o.p ?? 0.3)) return null;
  const n = cx.rnd.chance(0.25) ? 2 : 1;
  return Array.from({ length: n }, (_, k) => {
    const kind = cx.cyc(TREND_KINDS, k * 2 + cx.i);
    cx.f(`trend:${kind}`);
    return el('c:trendline', {}, cx.rnd.chance(0.3) ? tx('c:name', {}, 'Trend ' + k) : null, spPr(cx, { p: 0.5, fill: false, lineP: 1 }), val('c:trendlineType', kind),
      kind === 'poly' ? val('c:order', cx.rnd.int(2, 6)) : null, kind === 'movingAvg' ? val('c:period', cx.rnd.int(2, 5)) : null,
      cx.rnd.chance(0.4) ? val('c:forward', cx.rnd.int(1, 3)) : null, cx.rnd.chance(0.3) ? val('c:backward', cx.rnd.int(1, 2)) : null,
      cx.rnd.chance(0.3) && ['linear', 'poly', 'exp'].includes(kind) ? val('c:intercept', cx.rnd.int(-2, 5)) : null,
      val('c:dispRSqr', cx.rnd.pick([0, 1])), val('c:dispEq', cx.rnd.pick([0, 1])),
      cx.rnd.chance(0.5) ? el('c:trendlineLbl', {}, manual(cx, true), cx.rnd.chance(0.5) ? el('c:numFmt', { formatCode: cx.rnd.pick(NUMFMTS), sourceLinked: 0 }) : null, spPr(cx, { p: 0.3 }), txPr(cx, { p: 0.4 })) : null);
  });
}

function errBars(cx, p = 0.15) {
  if (!cx.rnd.chance(p)) return null;
  cx.f('errBars(ignored)');
  return el('c:errBars', {}, val('c:errBarType', 'both'), val('c:errValType', 'fixedVal'), val('c:noEndCap', 0), val('c:val', 1.5));
}

function dPts(cx, n, o = {}) {
  if (!cx.rnd.chance(o.p ?? 0.35)) return null;
  cx.f('dPt');
  return Array.from({ length: Math.min(n, 3) }, (_, k) => el('c:dPt', {}, val('c:idx', cx.rnd.chance(0.1) ? 0 : k), o.bar ? val('c:invertIfNegative', 0) : null, o.marker ? marker(cx) : null, o.pie && cx.rnd.chance(0.6) ? val('c:explosion', cx.rnd.int(0, 30)) : null, el('c:spPr', {}, fill(cx), cx.rnd.chance(0.5) ? line(cx) : null)));
}

// ------------------------------------------------------------------------------------------------ axes
const axTitle = (cx) => (cx.rnd.chance(0.25) ? title(cx, 'rich', { text: cx.rnd.pick(['Axis', 'Values (R$)', 'Eixo X']), body: cx.rnd.chance(0.5) ? { rot: cx.rnd.pick([-5400000, 0, 5400000, 2700000]), vert: 'horz' } : undefined }) : null);
const TICK = ['none', 'in', 'out', 'cross'];
const TLP = ['nextTo', 'low', 'high', 'none', 'nextTo', 'nextTo'];

function scaling(cx, o = {}) {
  const lg = o.log ?? (cx.rnd.chance(0.07) ? cx.rnd.pick([10, 2, 1, 0, 1e9, 5000, -3, 1.5]) : undefined);
  if (lg !== undefined) cx.f('axis:logBase');
  const mm = cx.rnd.chance(0.25);
  if (mm) cx.f('axis:minmax');
  return el('c:scaling', {}, lg !== undefined ? val('c:logBase', lg) : null, val('c:orientation', o.rev ?? (cx.rnd.chance(0.1) ? 'maxMin' : 'minMax')),
    mm ? val('c:max', cx.rnd.pick([100, 50, 1000, 1e9, 3.5, 0, -5])) : null, mm ? val('c:min', cx.rnd.pick([0, -10, 20, 1e-9, 99])) : null);
}

function valAx(cx, id, cross, pos, o = {}) {
  const du = cx.rnd.chance(0.07) ? (cx.rnd.chance(0.5) ? el('c:dispUnits', {}, val('c:builtInUnit', cx.rnd.pick(['hundreds', 'thousands', 'millions', 'billions', 'tenThousands'])), cx.rnd.chance(0.5) ? el('c:dispUnitsLbl') : null) : el('c:dispUnits', {}, el('c:custUnit', { val: cx.rnd.pick([2, 0.5, 1000, 0]) }))) : null;
  if (du) cx.f('axis:dispUnits');
  const crosses = cx.rnd.pick([['autoZero'], ['autoZero'], ['min'], ['max'], ['at', cx.rnd.pick([0, 3, -2, 1e6])]]);
  if (crosses[0] !== 'autoZero') cx.f(`axis:crosses:${crosses[0]}`);
  const mu = cx.rnd.chance(0.25), mnu = cx.rnd.chance(0.15);
  if (mu) cx.f('axis:majorUnit');
  return el('c:valAx', {}, val('c:axId', id), scaling(cx, o), val('c:delete', o.deleted ?? (cx.rnd.chance(0.06) ? 1 : 0)), val('c:axPos', pos),
    cx.rnd.chance(0.65) ? el('c:majorGridlines', {}, cx.rnd.chance(0.5) ? el('c:spPr', {}, line(cx)) : null) : null, cx.rnd.chance(0.15) ? el('c:minorGridlines', {}, cx.rnd.chance(0.5) ? el('c:spPr', {}, line(cx)) : null) : null,
    axTitle(cx), cx.rnd.chance(0.55) ? el('c:numFmt', { formatCode: cx.rnd.pick(NUMFMTS), sourceLinked: cx.rnd.pick([0, 1]) }) : null,
    val('c:majorTickMark', cx.rnd.pick(TICK)), val('c:minorTickMark', cx.rnd.pick(TICK)), val('c:tickLblPos', cx.rnd.pick(TLP)), el('c:spPr', {}, cx.rnd.chance(0.5) ? line(cx) : null), txPr(cx, { p: 0.4 }),
    val('c:crossAx', cross), crosses[0] === 'at' ? val('c:crossesAt', crosses[1]) : val('c:crosses', crosses[0]), val('c:crossBetween', o.between ?? cx.rnd.pick(['between', 'midCat'])),
    mu ? val('c:majorUnit', cx.rnd.pick([10, 0.5, 25, 1e-9, 1e12, 0, -5, 7])) : null, mnu ? val('c:minorUnit', cx.rnd.pick([1, 0.1, 5, 0])) : null, du);
}

function catAx(cx, id, cross, pos, o = {}) {
  const date = o.date;
  const skip = cx.rnd.chance(0.15);
  if (skip) cx.f('axis:tickSkip');
  const common = [val('c:axId', id), scaling(cx, { log: undefined, rev: o.rev }), val('c:delete', o.deleted ?? (cx.rnd.chance(0.05) ? 1 : 0)), val('c:axPos', pos), cx.rnd.chance(0.3) ? el('c:majorGridlines') : null,
    axTitle(cx), cx.rnd.chance(0.2) ? el('c:numFmt', { formatCode: cx.rnd.pick(NUMFMTS), sourceLinked: cx.rnd.pick([0, 1]) }) : null,
    val('c:majorTickMark', cx.rnd.pick(TICK)), val('c:minorTickMark', cx.rnd.pick(TICK)), val('c:tickLblPos', cx.rnd.pick(TLP)), el('c:spPr', {}, cx.rnd.chance(0.5) ? line(cx) : null), txPr(cx, { p: 0.4 }),
    val('c:crossAx', cross), val('c:crosses', 'autoZero')];
  if (date) {
    cx.f('axis:date');
    return el('c:dateAx', {}, common, val('c:auto', cx.rnd.pick([0, 1])), val('c:lblOffset', cx.rnd.pick([100, 0, 500])), val('c:baseTimeUnit', cx.rnd.pick(['days', 'months', 'years'])),
      cx.rnd.chance(0.6) ? val('c:majorUnit', cx.rnd.pick([1, 2, 7, 0])) : null, cx.rnd.chance(0.6) ? val('c:majorTimeUnit', cx.rnd.pick(['days', 'months', 'years'])) : null, cx.rnd.chance(0.2) ? val('c:minorUnit', 1) : null, cx.rnd.chance(0.2) ? val('c:minorTimeUnit', 'days') : null);
  }
  return el('c:catAx', {}, common, val('c:auto', 1), val('c:lblAlgn', 'ctr'), val('c:lblOffset', cx.rnd.pick([100, 100, 0, 300])), skip ? val('c:tickLblSkip', cx.rnd.pick([1, 2, 5, 0])) : null, skip ? val('c:tickMarkSkip', cx.rnd.pick([1, 3, 0])) : null,
    val('c:noMultiLvlLbl', cx.rnd.pick([0, 1])));
}

const serAx = (cx, id, cross) => el('c:serAx', {}, val('c:axId', id), el('c:scaling', {}, val('c:orientation', 'minMax')), val('c:delete', 0), val('c:axPos', 'b'), val('c:majorTickMark', 'out'), val('c:minorTickMark', 'none'), val('c:tickLblPos', 'nextTo'), val('c:crossAx', cross), val('c:crosses', 'autoZero'));

// ------------------------------------------------------------------------------------------------ series and groups
function ser(cx, i, name, vals, o = {}) {
  const n = vals.length;
  const nameXml = o.noName ? null : el('c:tx', {}, o.nameLit ? tx('c:v', {}, name) : strRef([name], `Sheet1!$${String.fromCharCode(66 + (i % 20))}$1`));
  return el('c:ser', {}, val('c:idx', o.idx ?? i), val('c:order', o.order ?? i), nameXml, o.sp ?? spPr(cx, { p: 0.6, fillKind: o.fillKind }),
    o.pre ?? null, o.marker, o.dPt ?? null, o.dl ?? null, o.trend ?? null, o.err ?? null,
    o.xVal ?? (o.cat === false ? null : (o.catXml ?? cat(cx, n, o.catOpts))),
    o.xVal ? el('c:yVal', {}, numRef(cx, vals, `Sheet1!$${String.fromCharCode(66 + (i % 20))}$2:$${String.fromCharCode(66 + (i % 20))}$${n + 1}`, o.fmt, o.num)) : el('c:val', {}, numRef(cx, vals, `Sheet1!$${String.fromCharCode(66 + (i % 20))}$2:$${String.fromCharCode(66 + (i % 20))}$${n + 1}`, o.fmt, o.num)),
    o.bubble ? el('c:bubbleSize', {}, numRef(cx, o.bubble, 'Sheet1!$D$2:$D$9', 'General')) : null, o.bubble3D ?? null, o.smooth !== undefined ? val('c:smooth', o.smooth) : null);
}

const NAMES = ['Series1', 'Receita', 'Q' + 2, 'Série "4"', 'Very long series name that goes beyond the legend width', 'A & B', 'Costs', 'Margin'];
const serName = (cx, k) => (k < NAMES.length ? NAMES[(k + cx.r) % NAMES.length] : `Series${k + 1}`);

const gdims = (cx, salt = 0) => ({ nSer: cx.cyc([1, 2, 3, 4, 6, 7, 12, 1, 2, 30], salt), nCat: cx.cyc([1, 3, 5, 6, 8, 12, 2, 20, 4, 200], salt + 3) });

/** axes family chart: groups + axes */
function axesFamily(cx, o) {
  const d = gdims(cx, o.salt ?? 0);
  if (o.small) { d.nCat = Math.min(d.nCat, 24); d.nSer = Math.min(d.nSer, 8); }
  const date = o.dateAxis ?? cx.rnd.chance(0.12);
  const mode = cx.cyc(['int', 'dec', 'big', 'neg', 'mixed', 'tiny', 'int', 'flat', 'zero', 'huge'], o.salt ?? 0);
  const sers = Array.from({ length: d.nSer }, (_, k) => {
    const vals = nums(cx, d.nCat, { mode: k === 0 ? mode : undefined, blanks: cx.cyc([0, 0, 0.2], 7) });
    return { name: serName(cx, k), vals };
  });
  return { d, date, sers };
}

function barGroup(cx, o = {}) {
  const { d, sers } = axesFamily(cx, { salt: 0, small: o.small });
  const dir = o.dir ?? cx.cyc(['col', 'bar'], 1);
  const grouping = o.grouping ?? cx.cyc(['clustered', 'stacked', 'percentStacked'], 0);
  cx.f(`bar:${dir}`, `bar:${grouping}`);
  const date = !o.noDate && cx.rnd.chance(0.1);
  const catXml = date ? cat(cx, d.nCat, { kind: 'dateNum' }) : undefined;
  const series = sers.map((s, k) => ser(cx, k, s.name, s.vals, { catXml, pre: cx.rnd.chance(0.3) ? val('c:invertIfNegative', cx.rnd.pick([0, 1])) : null, dPt: dPts(cx, d.nCat, { bar: true }), dl: dLbls(cx, { p: 0.45, stacked: grouping !== 'clustered', points: d.nCat }), trend: grouping === 'clustered' ? trendlines(cx, { p: 0.15 }) : null, err: errBars(cx, 0.08), fmt: cx.rnd.chance(0.3) ? cx.rnd.pick(NUMFMTS) : undefined, catOpts: o.catOpts }));
  const xml = el('c:barChart', {}, val('c:barDir', dir), val('c:grouping', grouping), cx.rnd.chance(0.8) ? val('c:varyColors', cx.rnd.pick([0, 1])) : null, series, cx.rnd.chance(0.2) ? dLbls(cx, { p: 1 }) : null,
    val('c:gapWidth', cx.rnd.pick([150, 0, 20, 500, 100, 50])), grouping !== 'clustered' ? val('c:overlap', cx.rnd.pick([100, 100, 50, -20])) : cx.rnd.chance(0.3) ? val('c:overlap', cx.rnd.pick([0, -50, 30, 100])) : null,
    cx.rnd.chance(0.12) ? el('c:serLines') : null, ...o.axIds.map((a) => val('c:axId', a)));
  return { xml, d, date, dir };
}

function lineGroup(cx, o = {}) {
  const { d, sers } = axesFamily(cx, { salt: 4, small: o.small });
  const grouping = o.grouping ?? cx.cyc(['standard', 'stacked', 'percentStacked', 'standard', 'standard'], 2);
  const smooth = cx.cyc([undefined, 0, 1, 1, 0], 1);
  cx.f(`line:${grouping}`, `line:smooth=${smooth}`);
  const date = !o.noDate && cx.rnd.chance(0.12);
  const catXml = date ? cat(cx, d.nCat, { kind: 'dateNum' }) : undefined;
  const mk = cx.cyc(['auto', 'auto', 'circle', 'none', 'diamond', 'square', 'triangle', 'x', 'star', 'plus', 'dot', 'dash', 'picture'], 5);
  const series = sers.map((s, k) => ser(cx, k, s.name, s.vals, { catXml, sp: spPr(cx, { p: 0.7, fill: false, lineP: 1 }), marker: marker(cx, { symbol: k % 2 ? undefined : mk }), dPt: dPts(cx, d.nCat, { marker: true, p: 0.2 }), dl: dLbls(cx, { p: 0.45, line: true, points: d.nCat }), trend: trendlines(cx, { p: 0.25 }), err: errBars(cx, 0.08), smooth: smooth === undefined ? undefined : (k === 0 ? smooth : cx.rnd.pick([0, 1])) }));
  const extra = [];
  if (sers.length > 1 && cx.rnd.chance(0.25)) { extra.push(el('c:dropLines', {}, cx.rnd.chance(0.5) ? el('c:spPr', {}, line(cx)) : null)); cx.f('line:dropLines'); }
  if (sers.length > 1 && cx.rnd.chance(0.25)) { extra.push(el('c:hiLowLines', {}, cx.rnd.chance(0.5) ? el('c:spPr', {}, line(cx)) : null)); cx.f('line:hiLowLines'); }
  if (sers.length > 1 && cx.rnd.chance(0.25)) { extra.push(el('c:upDownBars', {}, val('c:gapWidth', 150), el('c:upBars', {}, el('c:spPr', {}, fill(cx))), el('c:downBars', {}, el('c:spPr', {}, fill(cx))))); cx.f('line:upDownBars'); }
  const xml = el('c:lineChart', {}, val('c:grouping', grouping), cx.rnd.chance(0.8) ? val('c:varyColors', 0) : null, series, extra, cx.rnd.chance(0.5) ? val('c:marker', 1) : null, ...o.axIds.map((a) => val('c:axId', a)));
  return { xml, d, date };
}

function areaGroup(cx, o = {}) {
  const { d, sers } = axesFamily(cx, { salt: 6, small: o.small });
  const grouping = o.grouping ?? cx.cyc(['standard', 'stacked', 'percentStacked'], 1);
  cx.f(`area:${grouping}`);
  const date = !o.noDate && cx.rnd.chance(0.12);
  const catXml = date ? cat(cx, d.nCat, { kind: 'dateNum' }) : undefined;
  const series = sers.map((s, k) => ser(cx, k, s.name, s.vals, { catXml, dPt: dPts(cx, d.nCat, { p: 0.15 }), dl: dLbls(cx, { p: 0.3, stacked: grouping !== 'standard' }), trend: trendlines(cx, { p: 0.1 }) }));
  const xml = el('c:areaChart', {}, val('c:grouping', grouping), val('c:varyColors', 0), series, cx.rnd.chance(0.2) ? el('c:dropLines') : null, ...o.axIds.map((a) => val('c:axId', a)));
  return { xml, d, date };
}

function view3D(cx, rAng) {
  cx.f('view3D');
  return el('c:view3D', {}, val('c:rotX', cx.rnd.pick([15, 30, 0, 90, -90, 45])), val('c:rotY', cx.rnd.pick([20, 0, 30, 360, 90])), cx.rnd.chance(0.5) ? val('c:depthPercent', cx.rnd.pick([100, 20, 500, 150])) : null, val('c:rAngAx', rAng ?? cx.rnd.pick([0, 1])),
    cx.rnd.chance(0.4) ? val('c:perspective', cx.rnd.pick([30, 0, 100])) : null, cx.rnd.chance(0.2) ? val('c:hPercent', 80) : null);
}
const walls = (cx) => (cx.rnd.chance(0.5) ? [el('c:floor', {}, val('c:thickness', 0), spPr(cx, { p: 1 })), el('c:sideWall', {}, val('c:thickness', 0), spPr(cx, { p: 1 })), el('c:backWall', {}, val('c:thickness', 0), spPr(cx, { p: 1 }))] : null);

const dTable = (cx) => (cx.rnd.chance(0.12) ? (cx.f('dTable'), el('c:dTable', {}, val('c:showHorzBorder', cx.rnd.pick([0, 1])), val('c:showVertBorder', cx.rnd.pick([0, 1])), val('c:showOutline', cx.rnd.pick([0, 1])), val('c:showKeys', cx.rnd.pick([0, 1])), txPr(cx, { p: 0.4 }))) : null);

// ------------------------------------------------------------------------------------------------ the chartSpace wrapper
function wrap(cx, p) {
  const style = p.style ?? cx.rnd.weighted([[undefined, 5], [2, 2], [cx.rnd.int(1, 48), 5]]);
  const c14 = p.c14 ?? (cx.rnd.chance(0.12) ? 100 + cx.rnd.int(1, 48) : undefined);
  const ttl = p.title ?? (() => { const k = cx.cyc(TITLE_KINDS, 2); cx.f(`titleKind:${k}`); return k === 'none' || k === 'deleted' ? null : title(cx, k); })();
  const atd = p.autoTitleDeleted ?? (cx.rnd.chance(0.2) ? 1 : undefined);
  const nSer = p.nSer ?? 1;
  const leg = p.legend === undefined ? legend(cx, nSer, { kind: cx.cyc(['r', 'none', 'b', 'overlay', 't', 'l', 'manual', 'entries', 'tr', 'long'], 5) }) : p.legend;
  const rounded = cx.rnd.pick([undefined, 0, 1]);
  const ffmt = p.fileFormat;
  if (style !== undefined) cx.f(`style:${style}`);
  return `<?xml version="1.0" encoding="UTF-8" standalone="yes"?>\n` + el('c:chartSpace', p.ns === false ? {} : { 'xmlns:c': NS.c, 'xmlns:a': NS.a, 'xmlns:r': NS.r, 'xmlns:c14': NS.c14, 'xmlns:c15': NS.c15, 'xmlns:mc': NS.mc },
    cx.rnd.chance(0.5) ? val('c:date1904', 0) : null, tx('c:lang', {}, 'en-US'), rounded !== undefined ? val('c:roundedCorners', rounded) : null,
    c14 !== undefined ? el('mc:AlternateContent', {}, el('mc:Choice', { Requires: 'c14', 'xmlns:c14': NS.c14 }, val('c14:style', c14)), el('mc:Fallback', {}, val('c:style', c14 - 100))) : style !== undefined ? val('c:style', style) : null,
    el('c:chart', {}, ttl, atd !== undefined ? val('c:autoTitleDeleted', atd) : null, p.view3D ?? null, p.walls ?? null,
      el('c:plotArea', {}, cx.rnd.chance(0.25) ? manual(cx) : el('c:layout'), p.plot, dTable(cx), spPr(cx, { p: 0.25 })),
      leg, val('c:plotVisOnly', 1), p.dispBlanksAs !== undefined ? val('c:dispBlanksAs', p.dispBlanksAs) : cx.rnd.chance(0.5) ? val('c:dispBlanksAs', cx.rnd.pick(['gap', 'zero', 'span'])) : null),
    spPr(cx, { p: 0.5, shadowP: 0.1 }), txPr(cx, { p: 0.45 }), p.userShapesRef ? el('c:userShapes', { 'r:id': 'rIdUS' }) : null);
}

// ------------------------------------------------------------------------------------------------ families
const ids = (cx) => ({ cat: String(111111 + cx.i), val: String(222222 + cx.i), ser: String(333333 + cx.i), cat2: String(444444 + cx.i), val2: String(555555 + cx.i) });

function barFamily(variant) {
  return (cx) => {
    const id = ids(cx);
    const g = barGroup(cx, { axIds: [id.cat, id.val], ...variant });
    const vertical = g.dir === 'col';
    const ca = catAx(cx, id.cat, id.val, vertical ? 'b' : 'l', { date: g.date });
    const va = valAx(cx, id.val, id.cat, vertical ? 'l' : 'b', { between: 'between' });
    return { plot: [g.xml, ca, va], nSer: g.d.nSer };
  };
}

function lineFamily(variant) {
  return (cx) => {
    const id = ids(cx);
    const g = lineGroup(cx, { axIds: [id.cat, id.val], ...variant });
    return { plot: [g.xml, catAx(cx, id.cat, id.val, 'b', { date: g.date }), valAx(cx, id.val, id.cat, 'l')], nSer: g.d.nSer };
  };
}

function areaFamily(variant) {
  return (cx) => {
    const id = ids(cx);
    const g = areaGroup(cx, { axIds: [id.cat, id.val], ...variant });
    return { plot: [g.xml, catAx(cx, id.cat, id.val, 'b', { date: g.date }), valAx(cx, id.val, id.cat, 'l', { between: cx.rnd.pick(['midCat', 'between']) })], nSer: g.d.nSer };
  };
}

function bar3D(cx) {
  const id = ids(cx);
  const { d, sers } = axesFamily(cx, { salt: 8, small: true });
  const grouping = cx.cyc(['clustered', 'stacked', 'percentStacked', 'standard'], 0);
  const shape = cx.cyc(['box', 'cylinder', 'cone', 'pyramid', 'coneToMax', 'pyramidToMax'], 1);
  cx.f('bar3D:' + grouping, 'bar3D:shape=' + shape);
  const dir = cx.cyc(['col', 'bar'], 0);
  const series = sers.map((s, k) => ser(cx, k, s.name, s.vals, { pre: null, dl: dLbls(cx, { p: 0.3 }), dPt: dPts(cx, d.nCat, { bar: true, p: 0.2 }) }));
  const xml = el('c:bar3DChart', {}, val('c:barDir', dir), val('c:grouping', grouping), val('c:varyColors', 0), series, val('c:gapWidth', cx.rnd.pick([150, 50])), cx.rnd.chance(0.5) ? val('c:gapDepth', cx.rnd.pick([150, 0, 500])) : null, val('c:shape', shape),
    ...(grouping === 'standard' ? [val('c:axId', id.cat), val('c:axId', id.val), val('c:axId', id.ser)] : [val('c:axId', id.cat), val('c:axId', id.val), val('c:axId', '0')]));
  const vertical = dir === 'col';
  return { plot: [xml, catAx(cx, id.cat, id.val, vertical ? 'b' : 'l', {}), valAx(cx, id.val, id.cat, vertical ? 'l' : 'b', {}), grouping === 'standard' ? serAx(cx, id.ser, id.val) : null], view3D: view3D(cx), walls: walls(cx), nSer: d.nSer };
}

function line3D(cx) {
  const id = ids(cx);
  const { d, sers } = axesFamily(cx, { salt: 9, small: true });
  cx.f('line3D');
  const xml = el('c:line3DChart', {}, val('c:grouping', cx.cyc(['standard', 'stacked', 'percentStacked'], 0)), val('c:varyColors', 0), sers.map((s, k) => ser(cx, k, s.name, s.vals, { smooth: 0 })), val('c:gapDepth', 150), val('c:axId', id.cat), val('c:axId', id.val), val('c:axId', id.ser));
  return { plot: [xml, catAx(cx, id.cat, id.val, 'b'), valAx(cx, id.val, id.cat, 'l'), serAx(cx, id.ser, id.val)], view3D: view3D(cx), walls: walls(cx), nSer: d.nSer };
}

function area3D(cx) {
  const id = ids(cx);
  const { d, sers } = axesFamily(cx, { salt: 10, small: true });
  const grouping = cx.cyc(['standard', 'stacked', 'percentStacked'], 0);
  cx.f('area3D:' + grouping);
  const xml = el('c:area3DChart', {}, val('c:grouping', grouping), val('c:varyColors', 0), sers.map((s, k) => ser(cx, k, s.name, s.vals, {})), val('c:gapDepth', 150), val('c:axId', id.cat), val('c:axId', id.val), grouping === 'standard' ? val('c:axId', id.ser) : null);
  return { plot: [xml, catAx(cx, id.cat, id.val, 'b'), valAx(cx, id.val, id.cat, 'l'), grouping === 'standard' ? serAx(cx, id.ser, id.val) : null], view3D: view3D(cx), walls: walls(cx), nSer: d.nSer };
}

function pieData(cx) {
  const n = cx.cyc([1, 3, 5, 6, 8, 12, 2, 20, 4], 2);
  const mode = cx.cyc(['int', 'dec', 'zero', 'neg', 'int', 'flat', 'big', 'mixed'], 1);
  const vals = nums(cx, n, { mode, blanks: cx.cyc([0, 0.15, 0], 3) });
  return { n, vals };
}

function pieSeries(cx, d, o = {}) {
  const exp = cx.cyc([undefined, 0, 10, 40, undefined, 100], 2);
  return ser(cx, 0, 'Share', d.vals, { pre: exp !== undefined ? val('c:explosion', exp) : null, dPt: dPts(cx, d.n, { pie: true, p: 0.5 }), dl: dLbls(cx, { p: 0.7, pie: true, points: d.n }), catOpts: o.catOpts });
}

function pieFamily(kind) {
  return (cx) => {
    const d = pieData(cx);
    cx.f('pie:' + kind);
    let g;
    if (kind === 'pie') g = el('c:pieChart', {}, val('c:varyColors', cx.rnd.pick([1, 1, 0])), pieSeries(cx, d), val('c:firstSliceAng', cx.rnd.pick([0, 90, 180, 359, 45, 360, -10])));
    else if (kind === 'pie3D') g = el('c:pie3DChart', {}, val('c:varyColors', 1), pieSeries(cx, d));
    else g = el('c:doughnutChart', {}, val('c:varyColors', 1), pieSeries(cx, d), val('c:firstSliceAng', cx.rnd.pick([0, 90, 270])), val('c:holeSize', cx.rnd.pick([50, 10, 90, 75, 5, 95, 0])));
    return { plot: [g], view3D: kind === 'pie3D' ? view3D(cx) : null, nSer: d.n };
  };
}

function ofPie(cx) {
  const d = pieData(cx);
  if (d.n < 4) { d.vals = nums(cx, 8, {}); d.n = 8; }
  const type = cx.cyc(['pie', 'bar'], 0), split = cx.cyc(['pos', 'val', 'percent', 'cust', 'auto'], 1);
  cx.f(`ofPie:${type}`, `ofPie:${split}`);
  const g = el('c:ofPieChart', {}, val('c:ofPieType', type), val('c:varyColors', 1), pieSeries(cx, d), val('c:gapWidth', cx.rnd.pick([100, 0, 500])), val('c:splitType', split),
    split === 'cust' ? el('c:custSplit', {}, [1, 3, 5].map((p) => val('c:secondPiePt', p))) : (split !== 'auto' ? val('c:splitPos', split === 'percent' ? 20 : split === 'val' ? 5 : 3) : null),
    val('c:secondPieSize', cx.rnd.pick([75, 5, 200, 50])), cx.rnd.chance(0.5) ? el('c:serLines', {}, el('c:spPr', {}, line(cx))) : null);
  return { plot: [g], nSer: d.n };
}

function scatterFamily(cx) {
  const id = ids(cx);
  const style = cx.cyc(['lineMarker', 'marker', 'line', 'smooth', 'smoothMarker', 'none'], 0);
  const nSer = cx.cyc([1, 2, 3, 5], 1), n = cx.cyc([2, 5, 10, 25, 3, 60, 1], 2);
  cx.f('scatter:' + style);
  const blankX = cx.rnd.chance(0.12);
  const series = Array.from({ length: nSer }, (_, k) => {
    const xs = blankX ? Array.from({ length: n }, () => null) : nums(cx, n, { mode: cx.cyc(['sorted', 'dec', 'neg', 'int', 'big', 'tiny'], k) });
    const ys = nums(cx, n, { mode: cx.cyc(['int', 'dec', 'neg', 'mixed'], k + 1), blanks: cx.cyc([0, 0.2], 3) });
    if (blankX) cx.f('scatter:blankX');
    return ser(cx, k, serName(cx, k), ys, { xVal: el('c:xVal', {}, numRef(cx, xs, 'Sheet1!$A$2:$A$9', 'General')), marker: marker(cx), sp: spPr(cx, { p: 0.6, fill: false, lineP: 1 }), dl: dLbls(cx, { p: 0.3 }), trend: trendlines(cx, { p: 0.5 }), err: errBars(cx, 0.1), smooth: cx.rnd.chance(0.5) ? cx.rnd.pick([0, 1]) : undefined, dPt: dPts(cx, n, { marker: true, p: 0.2 }) });
  });
  const xml = el('c:scatterChart', {}, val('c:scatterStyle', style), val('c:varyColors', 0), series, val('c:axId', id.cat), val('c:axId', id.val));
  return { plot: [xml, valAx(cx, id.cat, id.val, 'b', { between: 'midCat' }), valAx(cx, id.val, id.cat, 'l', { between: 'midCat' })], nSer };
}

function bubbleFamily(cx) {
  const id = ids(cx);
  const nSer = cx.cyc([1, 2, 3], 0), n = cx.cyc([3, 6, 12, 1, 20], 1);
  cx.f('bubble');
  const b3 = cx.rnd.chance(0.3);
  const series = Array.from({ length: nSer }, (_, k) => ser(cx, k, serName(cx, k), nums(cx, n, { mode: 'int' }), { xVal: el('c:xVal', {}, numRef(cx, nums(cx, n, { mode: 'sorted' }), 'Sheet1!$A$2:$A$9', 'General')), bubble: nums(cx, n, { mode: cx.cyc(['int', 'dec', 'neg', 'zero'], k) }), bubble3D: b3 ? val('c:bubble3D', 1) : null, dl: dLbls(cx, { p: 0.4, bubble: true }), sp: spPr(cx, { p: 0.5, fillKind: 'solidAlpha' }), trend: trendlines(cx, { p: 0.1 }) }));
  const xml = el('c:bubbleChart', {}, val('c:varyColors', 0), series, val('c:bubbleScale', cx.rnd.pick([100, 30, 300, 0])), cx.rnd.chance(0.3) ? val('c:showNegBubbles', 1) : null, cx.rnd.chance(0.3) ? val('c:sizeRepresents', cx.rnd.pick(['area', 'w'])) : null, val('c:axId', id.cat), val('c:axId', id.val));
  return { plot: [xml, valAx(cx, id.cat, id.val, 'b', { between: 'midCat' }), valAx(cx, id.val, id.cat, 'l', { between: 'midCat' })], nSer };
}

function radarFamily(cx) {
  const id = ids(cx);
  const style = cx.cyc(['standard', 'marker', 'filled'], 0);
  const { d, sers } = axesFamily(cx, { salt: 11, small: true });
  cx.f('radar:' + style);
  const xml = el('c:radarChart', {}, val('c:radarStyle', style), val('c:varyColors', 0), sers.map((s, k) => ser(cx, k, s.name, s.vals, { marker: style === 'marker' ? marker(cx) : null, dl: dLbls(cx, { p: 0.2 }), sp: spPr(cx, { p: 0.6 }) })), val('c:axId', id.cat), val('c:axId', id.val));
  return { plot: [xml, catAx(cx, id.cat, id.val, 'b', {}), valAx(cx, id.val, id.cat, 'l', {})], nSer: d.nSer };
}

function stockFamily(cx) {
  const id = ids(cx);
  const kind = cx.cyc(['hlc', 'ohlc', 'vohlc'], 0);
  const n = cx.cyc([5, 12, 30, 3], 1);
  cx.f('stock:' + kind);
  const base = Array.from({ length: n }, (_, k) => 50 + ((k * 7) % 13) + cx.rnd.int(0, 5));
  const hi = base.map((v) => v + cx.rnd.int(1, 8)), lo = base.map((v) => v - cx.rnd.int(1, 8)), op = base.map((v) => v + cx.rnd.int(-3, 3)), cl = base.map((v) => v + cx.rnd.int(-3, 3));
  const lineSp = el('c:spPr', {}, el('a:ln', { w: 28575 }, el('a:noFill')));
  const names = kind === 'hlc' ? ['High', 'Low', 'Close'] : ['Open', 'High', 'Low', 'Close'];
  const data = kind === 'hlc' ? [hi, lo, cl] : [op, hi, lo, cl];
  const catXml = cat(cx, n, { kind: cx.cyc(['dateNum', 'str'], 0), step: 1 });
  const stock = el('c:stockChart', {}, data.map((v, k) => ser(cx, k + (kind === 'vohlc' ? 1 : 0), names[k], v, { sp: lineSp, marker: marker(cx, { symbol: cx.rnd.pick(['none', 'dash', 'circle']) }), catXml, smooth: 0 })), cx.rnd.chance(0.5) ? el('c:dropLines') : null, kind !== 'hlc' || cx.rnd.chance(0.3) ? el('c:hiLowLines') : null,
    kind !== 'hlc' ? el('c:upDownBars', {}, val('c:gapWidth', 150), el('c:upBars', {}, el('c:spPr', {}, fill(cx))), el('c:downBars', {}, el('c:spPr', {}, fill(cx)))) : null, val('c:axId', id.cat), val('c:axId', id.val));
  const vol = kind === 'vohlc' ? el('c:barChart', {}, val('c:barDir', 'col'), val('c:grouping', 'clustered'), val('c:varyColors', 0), ser(cx, 0, 'Volume', nums(cx, n, { mode: 'big' }), { catXml }), val('c:gapWidth', 150), val('c:axId', id.cat2), val('c:axId', id.val2)) : null;
  const axes = [catAx(cx, id.cat, id.val, 'b', { date: true }), valAx(cx, id.val, id.cat, 'r'), kind === 'vohlc' ? catAx(cx, id.cat2, id.val2, 'b', { deleted: 1 }) : null, kind === 'vohlc' ? valAx(cx, id.val2, id.cat2, 'l') : null];
  return { plot: [vol, stock, axes], nSer: data.length };
}

function surfaceFamily(cx) {
  const id = ids(cx);
  const { d, sers } = axesFamily(cx, { salt: 12, small: true });
  const wire = cx.rnd.chance(0.5), threeD = cx.rnd.chance(0.5);
  cx.f('surface' + (threeD ? '3D' : ''));
  const xml = el(threeD ? 'c:surface3DChart' : 'c:surfaceChart', {}, val('c:wireframe', wire ? 1 : 0), sers.map((s, k) => ser(cx, k, s.name, s.vals, {})), el('c:bandFmts'), val('c:axId', id.cat), val('c:axId', id.val), val('c:axId', id.ser));
  return { plot: [xml, catAx(cx, id.cat, id.val, 'b'), valAx(cx, id.val, id.cat, 'l'), serAx(cx, id.ser, id.val)], view3D: threeD ? view3D(cx) : null, nSer: d.nSer };
}

function comboFamily(kind) {
  return (cx) => {
    const id = ids(cx);
    const n = cx.cyc([4, 8, 12, 5, 24], 0);
    cx.f('combo:' + kind);
    const secondary = kind !== 'areaLine' || cx.rnd.chance(0.5);
    const a = kind === 'areaLine'
      ? el('c:areaChart', {}, val('c:grouping', 'standard'), val('c:varyColors', 0), ser(cx, 0, 'Area', nums(cx, n, { mode: 'int' }), {}), val('c:axId', id.cat), val('c:axId', id.val))
      : el('c:barChart', {}, val('c:barDir', 'col'), val('c:grouping', cx.cyc(['clustered', 'stacked'], 0)), val('c:varyColors', 0), [0, 1].map((k) => ser(cx, k, serName(cx, k), nums(cx, n, { mode: 'int' }), { dl: dLbls(cx, { p: 0.3 }) })), val('c:gapWidth', 100), val('c:axId', id.cat), val('c:axId', id.val));
    const l = el('c:lineChart', {}, val('c:grouping', 'standard'), val('c:varyColors', 0), ser(cx, 2, 'Line', nums(cx, n, { mode: cx.cyc(['dec', 'big', 'int'], 1) }), { marker: marker(cx), smooth: cx.rnd.pick([0, 1]), sp: spPr(cx, { p: 1, fill: false, lineP: 1 }), dl: dLbls(cx, { p: 0.3, line: true }) }), val('c:marker', 1), val('c:axId', secondary ? id.cat2 : id.cat), val('c:axId', secondary ? id.val2 : id.val));
    const axes = [catAx(cx, id.cat, id.val, 'b', {}), valAx(cx, id.val, id.cat, 'l', {}),
      secondary ? catAx(cx, id.cat2, id.val2, 'b', { deleted: 1 }) : null, secondary ? valAx(cx, id.val2, id.cat2, cx.rnd.pick(['r', 'r', 'l']), { deleted: cx.rnd.pick([0, 0, 1]) }) : null];
    if (secondary) cx.f('axes:secondary');
    return { plot: [a, l, axes], nSer: 3 };
  };
}

function scatterLineCombo(cx) {
  // bar + scatter on secondary axes, and a second group of the same kind
  const id = ids(cx);
  const n = cx.cyc([5, 10], 0);
  cx.f('combo:barScatter');
  const b = el('c:barChart', {}, val('c:barDir', 'col'), val('c:grouping', 'clustered'), val('c:varyColors', 0), ser(cx, 0, 'Bars', nums(cx, n, { mode: 'int' }), {}), val('c:gapWidth', 150), val('c:axId', id.cat), val('c:axId', id.val));
  const s = el('c:scatterChart', {}, val('c:scatterStyle', 'lineMarker'), val('c:varyColors', 0), ser(cx, 1, 'XY', nums(cx, n, { mode: 'int' }), { xVal: el('c:xVal', {}, numRef(cx, nums(cx, n, { mode: 'sorted' }), 'Sheet1!$A$2:$A$9', 'General')), marker: marker(cx) }), val('c:axId', id.cat2), val('c:axId', id.val2));
  return { plot: [b, s, catAx(cx, id.cat, id.val, 'b', {}), valAx(cx, id.val, id.cat, 'l', {}), valAx(cx, id.cat2, id.val2, 'b', { deleted: 1 }), valAx(cx, id.val2, id.cat2, 'r', {})], nSer: 2 };
}

/** two groups of the same type with different shared axes (stacked + clustered mix) */
function twoBarGroups(cx) {
  const id = ids(cx);
  const n = cx.cyc([4, 8], 0);
  cx.f('combo:barBar');
  const mk = (grouping, a, b, k0) => el('c:barChart', {}, val('c:barDir', 'col'), val('c:grouping', grouping), val('c:varyColors', 0), [0, 1].map((k) => ser(cx, k0 + k, serName(cx, k0 + k), nums(cx, n, { mode: 'int' }), {})), val('c:gapWidth', 80), grouping === 'stacked' ? val('c:overlap', 100) : null, val('c:axId', a), val('c:axId', b));
  return { plot: [mk('stacked', id.cat, id.val, 0), mk('clustered', id.cat2, id.val2, 2), catAx(cx, id.cat, id.val, 'b', {}), valAx(cx, id.val, id.cat, 'l', {}), catAx(cx, id.cat2, id.val2, 't', { deleted: 1 }), valAx(cx, id.val2, id.cat2, 'r', {})], nSer: 4 };
}

// ------------------------------------------------------------------------------------------------ edge-case families
function edgeFamily(cx) {
  const id = ids(cx);
  const k = cx.cyc(['noAxes', 'oneAxis', 'empty', 'noSeries', 'idxGaps', 'ptCountBig', 'nanText', 'dupAxes', 'garbageStyle', 'missingCrossAx', 'unknownGroupType', 'noPlot', 'seriesNoVal'], 0);
  cx.f('edge:' + k);
  const n = 6;
  const bar = (axs, series) => el('c:barChart', {}, val('c:barDir', 'col'), val('c:grouping', 'clustered'), series ?? ser(cx, 0, 'S', nums(cx, n, { mode: 'int' }), {}), ...axs.map((a) => val('c:axId', a)));
  switch (k) {
    case 'noAxes': return { plot: [bar([id.cat, id.val])], nSer: 1 };
    case 'oneAxis': return { plot: [bar([id.cat, id.val]), catAx(cx, id.cat, id.val, 'b', {})], nSer: 1 };
    case 'empty': return { plot: [], nSer: 0 };
    case 'noSeries': return { plot: [bar([id.cat, id.val], null), catAx(cx, id.cat, id.val, 'b', {}), valAx(cx, id.val, id.cat, 'l', {})], nSer: 0 };
    case 'idxGaps': return { plot: [bar([id.cat, id.val], [4, 0, 4, 9].map((ix, q) => ser(cx, q, 'S' + q, nums(cx, n, { mode: 'int' }), { idx: ix, order: q === 2 ? 0 : q }))), catAx(cx, id.cat, id.val, 'b', {}), valAx(cx, id.val, id.cat, 'l', {})], nSer: 4 };
    case 'ptCountBig': return { plot: [bar([id.cat, id.val], ser(cx, 0, 'S', [1, 2, 3], { num: { ptCount: 100001 }, cat: false })), catAx(cx, id.cat, id.val, 'b', {}), valAx(cx, id.val, id.cat, 'l', {})], nSer: 1 };
    case 'nanText': return { plot: [bar([id.cat, id.val], ser(cx, 0, 'S', [1, 2, 3, 4], { num: { nanText: true } })), catAx(cx, id.cat, id.val, 'b', {}), valAx(cx, id.val, id.cat, 'l', {})], nSer: 1 };
    case 'dupAxes': return { plot: [bar([id.cat, id.val]), catAx(cx, id.cat, id.val, 'b', {}), catAx(cx, id.cat, id.val, 'b', {}), valAx(cx, id.val, id.cat, 'l', {}), valAx(cx, id.val, id.cat, 'l', {})], nSer: 1 };
    case 'garbageStyle': return { plot: [bar([id.cat, id.val]), catAx(cx, id.cat, id.val, 'b', {}), valAx(cx, id.val, id.cat, 'l', {})], style: cx.rnd.pick([0, 49, 99, -5, 1000, 'x']), nSer: 1 };
    case 'missingCrossAx': return { plot: [bar([id.cat, id.val]), catAx(cx, id.cat, '999', 'b', {}), valAx(cx, id.val, '998', 'l', {})], nSer: 1 };
    case 'unknownGroupType': return { plot: [el('c:futureChart', {}, ser(cx, 0, 'S', [1, 2], {})), bar([id.cat, id.val]), catAx(cx, id.cat, id.val, 'b', {}), valAx(cx, id.val, id.cat, 'l', {})], nSer: 1 };
    case 'noPlot': return { plot: null, nSer: 0, noPlotArea: true };
    default: return { plot: [bar([id.cat, id.val], el('c:ser', {}, val('c:idx', 0), val('c:order', 0))), catAx(cx, id.cat, id.val, 'b', {}), valAx(cx, id.val, id.cat, 'l', {})], nSer: 1 };
  }
}

// ------------------------------------------------------------------------------------------------ chartex
function exWrap(cx, data, series, axes, o = {}) {
  const t = o.title === false ? null : el('cx:title', { pos: 't', align: 'ctr', overlay: 0 }, o.rich ? el('cx:tx', {}, el('cx:rich', {}, el('a:bodyPr'), el('a:lstStyle'), el('a:p', {}, el('a:r', {}, el('a:rPr', { lang: 'en-US' }), tx('a:t', {}, 'Chart title'))))) : el('cx:tx', {}, el('cx:txData', {}, tx('cx:v', {}, 'Chart title'))));
  const leg = cx.rnd.chance(0.5) ? el('cx:legend', { pos: cx.rnd.pick(['t', 'b', 'r', 'l']), align: 'ctr', overlay: cx.rnd.pick([0, 1]) }) : null;
  return `<?xml version="1.0" encoding="UTF-8" standalone="yes"?>\n` + el('cx:chartSpace', { 'xmlns:a': NS.a, 'xmlns:r': NS.r, 'xmlns:cx': NS.cx },
    el('cx:chartData', {}, data), el('cx:chart', {}, t, el('cx:plotArea', {}, el('cx:plotAreaRegion', {}, o.plotSurface ?? null, series), axes), leg), o.sp ? el('cx:spPr', {}, fill(cx), line(cx)) : null, cx.rnd.chance(0.3) ? el('cx:txPr', {}, el('a:bodyPr'), el('a:lstStyle'), el('a:p', {}, el('a:pPr', {}, el('a:defRPr', { sz: 1000 })), el('a:endParaRPr', { lang: 'en-US' }))) : null);
}
const exStr = (type, vals) => el('cx:strDim', { type }, el('cx:f', {}, 'Sheet1!$A$2:$A$9'), el('cx:lvl', { ptCount: vals.length }, vals.map((v, k) => (v === null ? null : el('cx:pt', { idx: k }, v)))));
const exNum = (type, vals, fmt) => el('cx:numDim', { type }, el('cx:f', {}, 'Sheet1!$B$2:$B$9'), el('cx:lvl', { ptCount: vals.length, formatCode: fmt ?? 'General' }, vals.map((v, k) => (v === null ? null : el('cx:pt', { idx: k }, String(v))))));
const exAxes = (cx, o = {}) => [
  el('cx:axis', { id: 0, hidden: o.hidden ? 1 : undefined }, el('cx:catScaling', { gapWidth: cx.rnd.pick(['0.5', '1', '0', '2', 'auto']) }), cx.rnd.chance(0.5) ? el('cx:title', {}, el('cx:tx', {}, el('cx:txData', {}, tx('cx:v', {}, 'Categories')))) : null, el('cx:tickLabels'), cx.rnd.chance(0.3) ? el('cx:majorTickMarks', { type: 'out' }) : null, cx.rnd.chance(0.3) ? el('cx:minorTickMarks', { type: 'none' }) : null),
  el('cx:axis', { id: 1 }, el('cx:valScaling', cx.rnd.chance(0.3) ? { max: 100, min: 0, majorUnit: 20 } : {}), cx.rnd.chance(0.6) ? el('cx:majorGridlines') : null, cx.rnd.chance(0.3) ? el('cx:numFmt', { formatCode: cx.rnd.pick(NUMFMTS), sourceLinked: 0 }) : null, el('cx:tickLabels')),
];
const exDl = (cx) => (cx.rnd.chance(0.7) ? el('cx:dataLabels', { pos: cx.rnd.pick(['ctr', 'outEnd', 'inEnd', 'inBase', undefined]) }, cx.rnd.chance(0.5) ? el('cx:numFmt', { formatCode: cx.rnd.pick(NUMFMTS), sourceLinked: 0 }) : null, el('cx:visibility', { seriesName: 0, categoryName: cx.rnd.pick([0, 1]), value: 1 })) : null);
const exSer = (cx, layout, o = {}) => el('cx:series', { layoutId: layout, uniqueId: `{${String(cx.i).padStart(8, '0')}-0000-0000-0000-000000000000}`, hidden: o.hidden ? 1 : undefined }, el('cx:tx', {}, el('cx:txData', {}, el('cx:f', {}, 'Sheet1!$B$1'), tx('cx:v', {}, o.name ?? 'Series1'))), o.sp ? el('cx:spPr', {}, fill(cx), line(cx)) : null, exDl(cx), o.dataPts ?? null, el('cx:dataId', { val: o.dataId ?? 0 }), o.lp ?? null);

function chartexFamily(kind) {
  return (cx) => {
    cx.f('chartex:' + kind);
    const n = cx.cyc([5, 8, 12, 3, 20, 1], 0);
    const cats = catLabels(cx, n, 'words');
    const vals = nums(cx, n, { mode: kind === 'funnel' ? 'sorted' : cx.cyc(['neg', 'int', 'mixed', 'dec', 'big'], 1) });
    const sp = cx.rnd.chance(0.3);
    switch (kind) {
      case 'waterfall': {
        const sub = cx.rnd.chance(0.7) ? el('cx:subtotals', {}, [0, Math.min(n - 1, 4)].map((v) => el('cx:idx', { val: v }))) : null;
        return { cx: exWrap(cx, el('cx:data', { id: 0 }, exStr('cat', cats), exNum('val', vals, cx.rnd.pick(['General', '#,##0', '0.0']))), exSer(cx, 'waterfall', { sp, lp: el('cx:layoutPr', {}, sub) }), exAxes(cx), { rich: true }) };
      }
      case 'funnel': return { cx: exWrap(cx, el('cx:data', { id: 0 }, exStr('cat', cats), exNum('val', vals)), exSer(cx, 'funnel', { sp, dataPts: cx.rnd.chance(0.3) ? el('cx:dataPt', { idx: 1 }, el('cx:spPr', {}, fill(cx))) : null }), exAxes(cx)) };
      case 'histogram':
      case 'pareto': {
        const m = 40 + cx.rnd.int(0, 60);
        const hv = nums(cx, m, { mode: cx.cyc(['int', 'dec', 'mixed', 'flat', 'big'], 1) });
        const binning = cx.cyc([null, { binSize: 10 }, { binCount: 6 }, { binSize: 5, underflow: 20, overflow: 80 }, { binCount: 4, intervalClosed: 'l' }, { binSize: 0 }], 0);
        const lp = el('cx:layoutPr', {}, kind === 'histogram' ? el('cx:binning', { intervalClosed: binning?.intervalClosed ?? 'r', underflow: binning?.underflow, overflow: binning?.overflow }, binning?.binSize !== undefined ? el('cx:binSize', { val: binning.binSize }) : null, binning?.binCount !== undefined ? el('cx:binCount', { val: binning.binCount }) : null) : el('cx:aggregation', {}));
        const pcats = Array.from({ length: m }, (_, k) => WORDS[k % 6]);
        const data = kind === 'histogram' ? el('cx:data', { id: 0 }, exNum('val', hv)) : el('cx:data', { id: 0 }, exStr('cat', pcats), exNum('val', hv.map((v) => Math.abs(v ?? 0))));
        const series = kind === 'histogram' ? exSer(cx, 'clusteredColumn', { sp, lp }) : [exSer(cx, 'clusteredColumn', { sp, lp }), exSer(cx, 'paretoLine', { name: 'Cum', dataId: 0 })];
        const axes = kind === 'pareto' ? [...exAxes(cx), el('cx:axis', { id: 2 }, el('cx:valScaling'), el('cx:units', { unit: 'percentage' }), el('cx:tickLabels'))] : exAxes(cx);
        return { cx: exWrap(cx, data, series, axes) };
      }
      case 'boxWhisker': {
        const m = 24;
        const bv = nums(cx, m, { mode: cx.cyc(['int', 'dec', 'mixed'], 1) });
        const groups = Array.from({ length: m }, (_, k) => (k % 2 ? 'A' : 'B'));
        const lp = el('cx:layoutPr', {}, el('cx:visibility', { meanLine: cx.rnd.pick([0, 1]), meanMarker: cx.rnd.pick([0, 1]), nonoutliers: cx.rnd.pick([0, 1]), outliers: cx.rnd.pick([0, 1]) }), el('cx:statistics', { quartileMethod: cx.cyc(['exclusive', 'inclusive'], 0) }));
        return { cx: exWrap(cx, el('cx:data', { id: 0 }, exStr('cat', groups), exNum('val', bv)), [exSer(cx, 'boxWhisker', { sp, lp }), cx.rnd.chance(0.4) ? exSer(cx, 'boxWhisker', { sp, lp, name: 'S2', dataId: 0 }) : null], exAxes(cx)) };
      }
      case 'treemap':
      case 'sunburst': {
        const m = cx.cyc([8, 12, 20, 5], 0);
        const outer = Array.from({ length: m }, (_, k) => (k % 3 === 2 && cx.rnd.chance(0.4) ? '' : WORDS[Math.floor(k / 3) % WORDS.length]));
        const mid = Array.from({ length: m }, (_, k) => (cx.rnd.chance(0.2) ? '' : `Mid${Math.floor(k / 2)}`));
        const leaf = Array.from({ length: m }, (_, k) => (cx.rnd.chance(0.1) ? '' : `Leaf${k}`));
        const sz = nums(cx, m, { mode: cx.cyc(['int', 'dec', 'big'], 0) }).map((v) => Math.abs(v ?? 1));
        const data = el('cx:data', { id: 0 }, el('cx:strDim', { type: 'cat' }, el('cx:f', {}, 'Sheet1!$A$2:$C$9'), el('cx:lvl', { ptCount: m }, leaf.map((v, k) => (v ? el('cx:pt', { idx: k }, v) : null))), el('cx:lvl', { ptCount: m }, mid.map((v, k) => (v ? el('cx:pt', { idx: k }, v) : null))), el('cx:lvl', { ptCount: m }, outer.map((v, k) => (v ? el('cx:pt', { idx: k }, v) : null)))), exNum('size', sz));
        const lp = el('cx:layoutPr', {}, kind === 'treemap' ? el('cx:parentLabelLayout', { val: cx.cyc(['overlapping', 'banner', 'none'], 0) }) : null);
        return { cx: exWrap(cx, data, exSer(cx, kind, { lp, dataPts: cx.rnd.chance(0.3) ? el('cx:dataPt', { idx: 0 }, el('cx:spPr', {}, fill(cx))) : null }), [], { title: cx.rnd.chance(0.5) }) };
      }
      case 'regionMap': return { cx: exWrap(cx, el('cx:data', { id: 0 }, exStr('cat', cats), exNum('val', vals)), exSer(cx, 'regionMap', { lp: el('cx:layoutPr', {}, el('cx:geography', { cultureLanguage: 'en-US' })) }), []) };
      default: return { cx: exWrap(cx, el('cx:data', { id: 0 }, exStr('cat', cats), exNum('val', vals)), exSer(cx, cx.rnd.pick(['clusteredColumn', 'futureKind', 'line']), { sp }), exAxes(cx)) };
    }
  };
}

// ------------------------------------------------------------------------------------------------ user shapes (Sheet's chart drawing)
function userShapes(cx) {
  cx.f('userShapes');
  const anchor = (k) => {
    const from = el('cdr:from', {}, tx('cdr:x', {}, cx.rnd.int(0, 60) / 100), tx('cdr:y', {}, cx.rnd.int(0, 60) / 100));
    const to = el('cdr:to', {}, tx('cdr:x', {}, cx.rnd.int(65, 100) / 100), tx('cdr:y', {}, cx.rnd.int(65, 100) / 100));
    const geom = cx.rnd.pick(['rect', 'roundRect', 'ellipse', 'line', 'triangle']);
    const body = cx.rnd.chance(0.7) ? el('cdr:txBody', {}, el('a:bodyPr', { wrap: cx.rnd.pick(['square', 'none']), anchor: cx.rnd.pick(['t', 'ctr', 'b']), rot: cx.rnd.chance(0.2) ? 5400000 : undefined }), el('a:lstStyle'), el('a:p', {}, el('a:pPr', { algn: cx.rnd.pick(['l', 'ctr', 'r']) }), el('a:r', {}, el('a:rPr', { lang: 'en-US', sz: cx.rnd.pick([900, 1200, 1800]), b: cx.rnd.pick([0, 1]) }, el('a:solidFill', {}, clr(cx))), tx('a:t', {}, cx.rnd.pick(['Source: internal', 'Note\u0001two lines', 'Long text '.repeat(6)]))))) : null;
    const sp = el('cdr:sp', { macro: '', textlink: '' }, el('cdr:nvSpPr', {}, el('cdr:cNvPr', { id: 2 + k, name: 'Shape ' + k }), el('cdr:cNvSpPr')), el('cdr:spPr', {}, el('a:xfrm', {}, el('a:off', { x: 0, y: 0 }), el('a:ext', { cx: 1000000, cy: 500000 })), el('a:prstGeom', { prst: geom }, el('a:avLst')), fill(cx), line(cx)), body);
    return el('cdr:relSizeAnchor', {}, from, to, sp);
  };
  const abs = el('cdr:absSizeAnchor', {}, el('cdr:from', {}, tx('cdr:x', {}, 0.1), tx('cdr:y', {}, 0.1)), el('cdr:ext', { cx: 900000, cy: 300000 }), el('cdr:sp', {}, el('cdr:nvSpPr', {}, el('cdr:cNvPr', { id: 9, name: 'Abs' }), el('cdr:cNvSpPr', { txBox: 1 })), el('cdr:spPr', {}, el('a:prstGeom', { prst: 'rect' }), el('a:noFill')), el('cdr:txBody', {}, el('a:bodyPr'), el('a:p', {}, el('a:r', {}, el('a:rPr'), tx('a:t', {}, 'Abs anchor'))))));
  const grp = el('cdr:relSizeAnchor', {}, el('cdr:from', {}, tx('cdr:x', {}, 0.2), tx('cdr:y', {}, 0.2)), el('cdr:to', {}, tx('cdr:x', {}, 0.7), tx('cdr:y', {}, 0.7)),
    el('cdr:grpSp', {}, el('cdr:nvGrpSpPr', {}, el('cdr:cNvPr', { id: 20, name: 'Group' }), el('cdr:cNvGrpSpPr')), el('cdr:grpSpPr', {}, el('a:xfrm', {}, el('a:off', { x: 0, y: 0 }), el('a:ext', { cx: 2000000, cy: 1000000 }), el('a:chOff', { x: 0, y: 0 }), el('a:chExt', { cx: 1000000, cy: 500000 }))),
      el('cdr:sp', {}, el('cdr:nvSpPr', {}, el('cdr:cNvPr', { id: 21, name: 'In' }), el('cdr:cNvSpPr')), el('cdr:spPr', {}, el('a:xfrm', {}, el('a:off', { x: 100000, y: 100000 }), el('a:ext', { cx: 400000, cy: 300000 })), el('a:prstGeom', { prst: 'ellipse' }), fill(cx, 'solid')))));
  const cxn = el('cdr:relSizeAnchor', {}, el('cdr:from', {}, tx('cdr:x', {}, 0.05), tx('cdr:y', {}, 0.5)), el('cdr:to', {}, tx('cdr:x', {}, 0.95), tx('cdr:y', {}, 0.5)), el('cdr:cxnSp', {}, el('cdr:nvCxnSpPr', {}, el('cdr:cNvPr', { id: 30, name: 'Line' }), el('cdr:cNvCxnSpPr')), el('cdr:spPr', {}, el('a:prstGeom', { prst: 'line' }), line(cx))));
  return `<?xml version="1.0" encoding="UTF-8" standalone="yes"?>\n` + el('c:userShapes', { 'xmlns:c': NS.c, 'xmlns:cdr': NS.cdr, 'xmlns:a': NS.a, 'xmlns:r': NS.r }, [0, 1, 2].slice(0, cx.rnd.int(1, 3)).map(anchor), cx.rnd.chance(0.4) ? abs : null, cx.rnd.chance(0.3) ? grp : null, cx.rnd.chance(0.3) ? cxn : null);
}

// ------------------------------------------------------------------------------------------------ the family table
export const FAMILIES = [
  ['bar', barFamily({})], ['bar-col-clustered', barFamily({ dir: 'col', grouping: 'clustered' })], ['bar-row-stacked', barFamily({ dir: 'bar', grouping: 'stacked' })], ['bar-col-percent', barFamily({ dir: 'col', grouping: 'percentStacked' })],
  ['bar-small', barFamily({ small: true })], ['bar-long-labels', barFamily({ small: true, catOpts: { kind: 'strlong' } })], ['bar3D', bar3D],
  ['line', lineFamily({})], ['line-standard', lineFamily({ grouping: 'standard' })], ['line-stacked', lineFamily({ grouping: 'stacked' })], ['line-small', lineFamily({ small: true })], ['line3D', line3D],
  ['area', areaFamily({})], ['area-small', areaFamily({ small: true })], ['area3D', area3D],
  ['pie', pieFamily('pie')], ['pie3D', pieFamily('pie3D')], ['doughnut', pieFamily('doughnut')], ['pie-b', pieFamily('pie')], ['ofPie', ofPie],
  ['scatter', scatterFamily], ['scatter-b', scatterFamily], ['bubble', bubbleFamily], ['radar', radarFamily], ['stock', stockFamily], ['surface', surfaceFamily],
  ['combo-barLine', comboFamily('barLine')], ['combo-areaLine', comboFamily('areaLine')], ['combo-barScatter', scatterLineCombo], ['combo-barBar', twoBarGroups],
  ['edge', edgeFamily], ['edge-b', edgeFamily],
  ['cx-waterfall', chartexFamily('waterfall')], ['cx-funnel', chartexFamily('funnel')], ['cx-histogram', chartexFamily('histogram')], ['cx-pareto', chartexFamily('pareto')], ['cx-boxWhisker', chartexFamily('boxWhisker')],
  ['cx-treemap', chartexFamily('treemap')], ['cx-sunburst', chartexFamily('sunburst')], ['cx-regionMap', chartexFamily('regionMap')], ['cx-other', chartexFamily('other')],
  ['bar-b', barFamily({})], ['line-b', lineFamily({})], ['area-b', areaFamily({})], ['pie-c', pieFamily('doughnut')],
];

/** The host profile of chart `i`: 70 % Slides (`ppt`), 30 % Sheet (`xl`). */
function profile(cx) {
  const host = cx.i % 10 < 3 ? 'xl' : 'ppt';
  const fileFormat = cx.cyc([undefined, 'modern', 'mso2007', 'modern'], 1);
  const themeName = host === 'ppt' ? cx.cyc(['synth', 'none', 'alt', 'synth'], 2) : cx.cyc(['none', 'synth'], 1);
  return { host, fileFormat, themeName };
}

export function buildChart(seed, i = seed - SEED_BASE) {
  const F = FAMILIES.length;
  const cx = new Cx(seed, i, F);
  const [name, make] = FAMILIES[i % F];
  const prof = profile(cx);
  const r = make(cx);
  let xml;
  if (r.cx) xml = r.cx;
  else {
    const wrapped = r.noPlotArea
      ? `<?xml version="1.0" encoding="UTF-8" standalone="yes"?>\n${el('c:chartSpace', { 'xmlns:c': NS.c, 'xmlns:a': NS.a, 'xmlns:r': NS.r }, el('c:chart', {}, title(cx, 'rich')))}`
      : wrap(cx, { plot: r.plot, nSer: r.nSer, view3D: r.view3D, walls: r.walls, style: r.style, userShapesRef: prof.host === 'xl' && cx.rnd.chance(0.25) });
    xml = wrapped;
  }
  let userShapesXml;
  if (xml.includes('<c:userShapes ')) userShapesXml = userShapes(cx);
  // a second spelling: default namespace instead of prefixes (only the Sheet canonicalizes: CH-4)
  if (!r.cx && i % 37 === 5) { xml = xml.replace(/<(\/?)c:/g, '<$1').replace(/<chartSpace /, '<chartSpace xmlns="' + NS.c + '" ').replace('xmlns:c="' + NS.c + '" ', ''); cx.f('ns:default'); }
  return { xml, name, host: prof.host, themeName: prof.themeName, fileFormat: prof.fileFormat, images: cx.images, userShapesXml, features: [...cx.feat].sort(), seed };
}

export const chartKey = (group, i) => `gen/${group}/${String(i).padStart(4, '0')}`;
