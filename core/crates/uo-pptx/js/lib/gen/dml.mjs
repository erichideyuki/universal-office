// DrawingML fragments: colours with modifiers, fills (solid, gradient, pattern, picture),
// lines (dashes, joins, arrows), effects (shadow, glow, soft edge, reflection), 3D, geometry.
// Every builder takes the deck context `c` ({ r: Rand, feat(tag), addImage(), addLink() ... }).
import { randomPng } from '../png.mjs';

export const esc = (s) => String(s).replace(/&/g, '&amp;').replace(/</g, '&lt;').replace(/>/g, '&gt;').replace(/"/g, '&quot;');

const SCHEME = ['bg1', 'tx1', 'bg2', 'tx2', 'accent1', 'accent2', 'accent3', 'accent4', 'accent5', 'accent6', 'hlink', 'folHlink', 'dk1', 'lt1', 'dk2', 'lt2', 'phClr'];
const PRST_CLR = ['black', 'white', 'red', 'green', 'blue', 'yellow', 'orange', 'purple', 'gray', 'dkBlue', 'ltGray', 'maroon', 'navy', 'teal', 'olive', 'silver', 'cyan', 'magenta', 'gold', 'coral', 'khaki', 'plum', 'salmon', 'tomato'];
const SYS_CLR = [['windowText', '000000'], ['window', 'FFFFFF'], ['highlight', '316AC5']];

function modifiers(c) {
  const r = c.r;
  const n = r.weighted([[0, 45], [1, 35], [2, 15], [3, 5]]);
  const out = [];
  for (let i = 0; i < n; i++) {
    const m = r.pick(['lumMod', 'lumOff', 'tint', 'shade', 'alpha', 'satMod', 'satOff', 'hueOff', 'lum', 'sat', 'alphaMod', 'alphaOff', 'comp', 'inv', 'gray', 'hue', 'hueMod', 'red', 'green', 'blue', 'redMod', 'greenMod', 'blueMod']);
    if (['comp', 'inv', 'gray'].includes(m)) out.push(`<a:${m}/>`);
    else if (m === 'hueOff' || m === 'hue') out.push(`<a:${m} val="${r.int(0, 21600000)}"/>`);
    else if (m === 'lumOff' || m === 'satOff' || m === 'alphaOff') out.push(`<a:${m} val="${r.int(-30000, 60000)}"/>`);
    else if (m === 'lumMod' || m === 'satMod' || m === 'hueMod' || m === 'alphaMod' || m.endsWith('Mod')) out.push(`<a:${m} val="${r.int(10000, 250000)}"/>`);
    else out.push(`<a:${m} val="${r.int(0, 100000)}"/>`);
    c.feat('clr:' + m);
  }
  return out.join('');
}

/** A colour element (srgbClr, schemeClr, prstClr, sysClr, scrgbClr or hslClr) with modifiers. */
export function colorXml(c, { mods = true, noPhClr = true, alpha } = {}) {
  const r = c.r;
  const kind = r.weighted([['srgb', 38], ['scheme', 38], ['prst', 8], ['sys', 4], ['scrgb', 6], ['hsl', 6]]);
  const m = (mods ? modifiers(c) : '') + (alpha !== undefined ? `<a:alpha val="${alpha}"/>` : '');
  switch (kind) {
    case 'srgb': return `<a:srgbClr val="${r.hex6()}">${m}</a:srgbClr>`;
    case 'scheme': {
      const list = noPhClr ? SCHEME.filter((s) => s !== 'phClr') : SCHEME;
      return `<a:schemeClr val="${r.pick(list)}">${m}</a:schemeClr>`;
    }
    case 'prst': return `<a:prstClr val="${r.pick(PRST_CLR)}">${m}</a:prstClr>`;
    case 'sys': { const [v, last] = r.pick(SYS_CLR); return `<a:sysClr val="${v}" lastClr="${last}">${m}</a:sysClr>`; }
    case 'scrgb': return `<a:scrgbClr r="${r.int(0, 100000)}" g="${r.int(0, 100000)}" b="${r.int(0, 100000)}">${m}</a:scrgbClr>`;
    default: return `<a:hslClr hue="${r.int(0, 21600000)}" sat="${r.int(0, 100000)}" lum="${r.int(0, 100000)}">${m}</a:hslClr>`;
  }
}

export const solidXml = (c, o) => `<a:solidFill>${colorXml(c, o)}</a:solidFill>`;

const PATTERNS = ['pct5', 'pct10', 'pct20', 'pct25', 'pct30', 'pct40', 'pct50', 'pct60', 'pct70', 'pct75', 'pct80', 'pct90', 'horz', 'vert', 'ltHorz', 'ltVert', 'dkHorz', 'dkVert', 'narHorz', 'narVert', 'dashHorz', 'dashVert', 'cross', 'dnDiag', 'upDiag', 'ltDnDiag', 'ltUpDiag', 'dkDnDiag', 'dkUpDiag', 'wdDnDiag', 'wdUpDiag', 'dashDnDiag', 'dashUpDiag', 'diagCross', 'smCheck', 'lgCheck', 'smGrid', 'lgGrid', 'dotGrid', 'smConfetti', 'lgConfetti', 'horzBrick', 'diagBrick', 'solidDmnd', 'openDmnd', 'dotDmnd', 'plaid', 'sphere', 'weave', 'divot', 'shingle', 'wave', 'trellis', 'zigZag'];

function gradientXml(c) {
  const r = c.r;
  const n = r.weighted([[2, 50], [3, 30], [4, 12], [5, 8]]);
  const poss = [0, 100000];
  while (poss.length < n) poss.push(r.int(1000, 99000));
  poss.sort((a, b) => a - b);
  const stops = poss.map((p) => `<a:gs pos="${p}">${colorXml(c, { alpha: r.chance(0.12) ? r.int(10000, 95000) : undefined })}</a:gs>`).join('');
  const rot = r.chance(0.7) ? ' rotWithShape="1"' : r.chance(0.3) ? ' rotWithShape="0"' : '';
  const flip = r.chance(0.1) ? ` flip="${r.pick(['x', 'y', 'xy'])}"` : '';
  if (r.chance(0.3)) {
    const path = r.pick(['circle', 'rect', 'shape']);
    c.feat('fill:grad-path');
    const ftr = r.chance(0.7) ? `<a:fillToRect l="${r.int(0, 100000)}" t="${r.int(0, 100000)}" r="${r.int(0, 100000)}" b="${r.int(0, 100000)}"/>` : '<a:fillToRect l="50000" t="50000" r="50000" b="50000"/>';
    return `<a:gradFill${rot}${flip}><a:gsLst>${stops}</a:gsLst><a:path path="${path}">${ftr}</a:path>${r.chance(0.2) ? '<a:tileRect/>' : ''}</a:gradFill>`;
  }
  c.feat('fill:grad-lin');
  const ang = r.pick([0, 5400000, 10800000, 16200000, 2700000, r.int(0, 21599999)]);
  return `<a:gradFill${rot}${flip}><a:gsLst>${stops}</a:gsLst><a:lin ang="${ang}" scaled="${r.below(2)}"/></a:gradFill>`;
}

function blipFillXml(c, tag = 'a:blipFill') {
  const r = c.r;
  const rId = c.addImage(randomPng(r));
  c.feat('fill:picture');
  const fx = blipEffects(c);
  const dpi = r.chance(0.2) ? ` dpi="${r.pick([0, 96, 150, 300])}"` : '';
  const rot = r.chance(0.2) ? ' rotWithShape="1"' : '';
  const src = r.chance(0.25) ? `<a:srcRect l="${r.int(0, 30000)}" t="${r.int(0, 30000)}" r="${r.int(0, 30000)}" b="${r.int(0, 30000)}"/>` : '';
  let mode;
  if (r.chance(0.3)) {
    c.feat('fill:picture-tile');
    mode = `<a:tile tx="${r.int(-500000, 500000)}" ty="${r.int(-500000, 500000)}" sx="${r.pick([50000, 100000, 200000])}" sy="${r.pick([50000, 100000, 200000])}" flip="${r.pick(['none', 'x', 'y', 'xy'])}" algn="${r.pick(['tl', 't', 'tr', 'l', 'ctr', 'r', 'bl', 'b', 'br'])}"/>`;
  } else {
    mode = r.chance(0.2) ? `<a:stretch><a:fillRect l="${r.int(-20000, 20000)}" t="${r.int(-20000, 20000)}" r="${r.int(-20000, 20000)}" b="${r.int(-20000, 20000)}"/></a:stretch>` : '<a:stretch><a:fillRect/></a:stretch>';
  }
  return `<${tag}${dpi}${rot}><a:blip r:embed="${rId}">${fx}</a:blip>${src}${mode}</${tag}>`;
}

/** Colour effects of a `<a:blip>` (also used by pictures). */
export function blipEffects(c) {
  const r = c.r;
  if (!r.chance(0.3)) return '';
  const out = [];
  const k = r.pick(['alphaModFix', 'grayscl', 'biLevel', 'lum', 'duotone', 'clrChange', 'alphaModFix+lum']);
  c.feat('blipfx:' + k);
  if (k.includes('alphaModFix')) out.push(`<a:alphaModFix amt="${r.int(10000, 100000)}"/>`);
  if (k === 'grayscl') out.push('<a:grayscl/>');
  if (k === 'biLevel') out.push(`<a:biLevel thresh="${r.int(10000, 90000)}"/>`);
  if (k.includes('lum')) out.push(`<a:lum bright="${r.int(-50000, 50000)}" contrast="${r.int(-50000, 50000)}"/>`);
  if (k === 'duotone') out.push(`<a:duotone>${colorXml(c, { mods: false })}${colorXml(c, { mods: false })}</a:duotone>`);
  if (k === 'clrChange') out.push(`<a:clrChange><a:clrFrom><a:srgbClr val="${r.hex6()}"/></a:clrFrom><a:clrTo><a:srgbClr val="${r.hex6()}"><a:alpha val="0"/></a:srgbClr></a:clrTo></a:clrChange>`);
  return out.join('');
}

/** A fill element: `kind` forces one of solid|grad|patt|blip|none|grp, else seeded. */
export function fillXml(c, kind) {
  const r = c.r;
  kind ??= r.weighted([['solid', 50], ['grad', 22], ['patt', 8], ['blip', 8], ['none', 8], ['grp', 4]]);
  c.feat('fill:' + kind);
  switch (kind) {
    case 'solid': return solidXml(c);
    case 'grad': return gradientXml(c);
    case 'patt': return `<a:pattFill prst="${r.pick(PATTERNS)}"><a:fgClr>${colorXml(c)}</a:fgClr><a:bgClr>${colorXml(c)}</a:bgClr></a:pattFill>`;
    case 'blip': return blipFillXml(c);
    case 'grp': return '<a:grpFill/>';
    default: return '<a:noFill/>';
  }
}

const ARROWS = ['none', 'triangle', 'stealth', 'diamond', 'oval', 'arrow'];
const DASHES = ['solid', 'dot', 'dash', 'lgDash', 'dashDot', 'lgDashDot', 'lgDashDotDot', 'sysDash', 'sysDot', 'sysDashDot', 'sysDashDotDot'];

/** `<a:ln>` with a width, a fill, dashes, joins and arrow heads. `arrows` allows head/tail ends. */
export function lineXml(c, { arrows = false, forceFill } = {}) {
  const r = c.r;
  c.feat('line');
  const w = r.weighted([[0, 6], [3175, 6], [6350, 15], [12700, 30], [19050, 15], [38100, 10], [76200, 5], [r.int(1000, 200000), 13]]);
  const attrs = [`w="${w}"`];
  if (r.chance(0.3)) attrs.push(`cap="${r.pick(['rnd', 'sq', 'flat'])}"`);
  if (r.chance(0.15)) { const cm = r.pick(['dbl', 'thickThin', 'thinThick', 'tri']); attrs.push(`cmpd="${cm}"`); c.feat('line:cmpd'); }
  if (r.chance(0.1)) attrs.push(`algn="${r.pick(['ctr', 'in'])}"`);
  const fill = forceFill ?? (r.chance(0.12) ? 'noFill' : r.chance(0.12) ? gradientXml(c) : r.chance(0.06) ? fillXml(c, 'patt') : solidXml(c));
  let dash = '';
  if (r.chance(0.35)) { dash = `<a:prstDash val="${r.pick(DASHES)}"/>`; c.feat('line:dash'); }
  else if (r.chance(0.05)) { dash = `<a:custDash><a:ds d="${r.int(100000, 600000)}" sp="${r.int(100000, 400000)}"/><a:ds d="${r.int(10000, 100000)}" sp="${r.int(100000, 300000)}"/></a:custDash>`; c.feat('line:custdash'); }
  const join = r.weighted([['', 40], ['<a:round/>', 25], ['<a:bevel/>', 15], [`<a:miter lim="${r.pick([100000, 800000, 1600000])}"/>`, 20]]);
  let ends = '';
  if (arrows) {
    const mk = (tag) => {
      const t = r.pick(ARROWS);
      if (t === 'none' && !r.chance(0.3)) return '';
      c.feat('line:arrow');
      return `<a:${tag} type="${t}" w="${r.pick(['sm', 'med', 'lg'])}" len="${r.pick(['sm', 'med', 'lg'])}"/>`;
    };
    ends = mk('headEnd') + mk('tailEnd');
  }
  return `<a:ln ${attrs.join(' ')}>${fill}${dash}${join}${ends}</a:ln>`;
}

/** An effect list (shadows, glow, soft edge, reflection, blur). */
export function effectListXml(c) {
  const r = c.r;
  const parts = [];
  if (r.chance(0.08)) { parts.push(`<a:blur rad="${r.int(1000, 100000)}" grow="${r.below(2)}"/>`); c.feat('fx:blur'); }
  if (r.chance(0.15)) { parts.push(`<a:glow rad="${r.int(20000, 300000)}">${colorXml(c, { alpha: r.int(20000, 80000) })}</a:glow>`); c.feat('fx:glow'); }
  if (r.chance(0.1)) { parts.push(`<a:innerShdw blurRad="${r.int(0, 150000)}" dist="${r.int(0, 100000)}" dir="${r.int(0, 21599999)}">${colorXml(c, { alpha: r.int(20000, 80000) })}</a:innerShdw>`); c.feat('fx:innerShdw'); }
  if (r.chance(0.3)) {
    const sx = r.chance(0.25) ? ` sx="${r.int(50000, 150000)}" sy="${r.int(50000, 150000)}"` : '';
    const algn = r.chance(0.3) ? ` algn="${r.pick(['tl', 't', 'tr', 'l', 'ctr', 'r', 'bl', 'b', 'br'])}"` : '';
    parts.push(`<a:outerShdw blurRad="${r.int(0, 200000)}" dist="${r.int(0, 150000)}" dir="${r.int(0, 21599999)}"${sx}${algn} rotWithShape="${r.below(2)}">${colorXml(c, { alpha: r.int(10000, 90000) })}</a:outerShdw>`);
    c.feat('fx:outerShdw');
  }
  if (r.chance(0.04)) { parts.push(`<a:prstShdw prst="shdw${r.int(1, 20)}" dist="${r.int(0, 100000)}" dir="${r.int(0, 21599999)}">${colorXml(c, { mods: false })}</a:prstShdw>`); c.feat('fx:prstShdw'); }
  if (r.chance(0.12)) {
    parts.push(`<a:reflection blurRad="${r.int(0, 100000)}" stA="${r.int(10000, 80000)}" stPos="${r.int(0, 30000)}" endA="${r.int(0, 10000)}" endPos="${r.int(30000, 100000)}" dist="${r.int(0, 100000)}" dir="5400000" fadeDir="5400000" sx="100000" sy="${r.pick([-100000, -80000, -50000])}" algn="${r.pick(['bl', 'b', 'tl'])}" rotWithShape="0"/>`);
    c.feat('fx:reflection');
  }
  if (r.chance(0.1)) { parts.push(`<a:softEdge rad="${r.int(10000, 200000)}"/>`); c.feat('fx:softEdge'); }
  return parts.length ? `<a:effectLst>${parts.join('')}</a:effectLst>` : '';
}

/** `<a:scene3d>` + `<a:sp3d>` (bevel and/or extrusion). */
export function threeDXml(c) {
  const r = c.r;
  c.feat('3d');
  const cam = r.chance(0.3) ? `<a:camera prst="${r.pick(['perspectiveFront', 'orthographicFront', 'isometricTopUp', 'obliqueTopRight', 'perspectiveRelaxed'])}"/>` : `<a:camera prst="orthographicFront"><a:rot lat="${r.int(0, 21599999)}" lon="${r.int(0, 21599999)}" rev="${r.int(0, 21599999)}"/></a:camera>`;
  const rig = `<a:lightRig rig="${r.pick(['threePt', 'balanced', 'soft', 'harsh', 'flood', 'contrasting', 'morning', 'sunrise', 'sunset', 'chilly', 'freezing', 'flat', 'twoPt', 'glow', 'brightRoom'])}" dir="${r.pick(['t', 'tl', 'tr', 'l', 'r', 'b'])}"/>`;
  const bevel = r.chance(0.7) ? `<a:bevelT w="${r.int(0, 200000)}" h="${r.int(0, 200000)}" prst="${r.pick(['circle', 'relaxedInset', 'cross', 'coolSlant', 'angle', 'softRound', 'convex', 'slope', 'divot', 'riblet', 'hardEdge', 'artDeco'])}"/>` : '';
  const ext = r.chance(0.3) ? ` extrusionH="${r.int(10000, 300000)}"` : '';
  const extClr = ext && r.chance(0.5) ? `<a:extrusionClr>${colorXml(c, { mods: false })}</a:extrusionClr>` : '';
  const mat = r.chance(0.5) ? ` prstMaterial="${r.pick(['matte', 'warmMatte', 'plastic', 'metal', 'dkEdge', 'softEdge', 'flat', 'legacyMatte', 'powder', 'translucentPowder', 'clear'])}"` : '';
  return `<a:scene3d>${cam}${rig}</a:scene3d><a:sp3d${ext}${mat}>${bevel}${extClr}</a:sp3d>`;
}

// preset geometry: name -> adjust value names with their typical ranges
export const ADJ = {
  roundRect: [['adj', 0, 50000]],
  round1Rect: [['adj', 0, 50000]],
  round2SameRect: [['adj1', 0, 50000], ['adj2', 0, 50000]],
  round2DiagRect: [['adj1', 0, 50000], ['adj2', 0, 50000]],
  snip1Rect: [['adj', 0, 50000]],
  snip2SameRect: [['adj1', 0, 50000], ['adj2', 0, 50000]],
  snipRoundRect: [['adj1', 0, 50000], ['adj2', 0, 50000]],
  triangle: [['adj', 0, 100000]],
  parallelogram: [['adj', 0, 100000]],
  trapezoid: [['adj', 0, 50000]],
  hexagon: [['adj', 0, 50000], ['vf', 90000, 130000]],
  octagon: [['adj', 0, 50000]],
  star4: [['adj', 0, 50000]],
  star5: [['adj', 0, 50000], ['hf', 100000, 120000], ['vf', 100000, 120000]],
  star6: [['adj', 0, 50000], ['hf', 100000, 120000]],
  star8: [['adj', 0, 50000]],
  star12: [['adj', 0, 50000]],
  star24: [['adj', 0, 50000]],
  rightArrow: [['adj1', 0, 100000], ['adj2', 0, 100000]],
  leftArrow: [['adj1', 0, 100000], ['adj2', 0, 100000]],
  upArrow: [['adj1', 0, 100000], ['adj2', 0, 100000]],
  leftRightArrow: [['adj1', 0, 100000], ['adj2', 0, 100000]],
  chevron: [['adj', 0, 100000]],
  homePlate: [['adj', 0, 100000]],
  wedgeRectCallout: [['adj1', -100000, 100000], ['adj2', -100000, 100000]],
  wedgeRoundRectCallout: [['adj1', -100000, 100000], ['adj2', -100000, 100000], ['adj3', 0, 50000]],
  wedgeEllipseCallout: [['adj1', -100000, 100000], ['adj2', -100000, 100000]],
  donut: [['adj', 0, 50000]],
  blockArc: [['adj1', 0, 21599999], ['adj2', 0, 21599999], ['adj3', 0, 50000]],
  arc: [['adj1', 0, 21599999], ['adj2', 0, 21599999]],
  pie: [['adj1', 0, 21599999], ['adj2', 0, 21599999]],
  chord: [['adj1', 0, 21599999], ['adj2', 0, 21599999]],
  frame: [['adj1', 0, 50000]],
  plaque: [['adj', 0, 50000]],
  can: [['adj', 0, 100000]],
  cube: [['adj', 0, 100000]],
  bevel: [['adj', 0, 50000]],
  foldedCorner: [['adj', 0, 50000]],
  cross: [['adj', 0, 50000]],
  gear6: [['adj1', 0, 20000], ['adj2', 0, 20000]],
  wave: [['adj1', 0, 20000], ['adj2', -10000, 10000]],
  leftBracket: [['adj', 0, 50000]],
  leftBrace: [['adj1', 0, 50000], ['adj2', 0, 100000]],
  bentConnector3: [['adj1', -50000, 150000]],
  curvedConnector3: [['adj1', -50000, 150000]],
};

export const PRESETS = [
  'rect', 'rect', 'rect', 'roundRect', 'roundRect', 'ellipse', 'ellipse', 'triangle', 'rtTriangle', 'parallelogram', 'trapezoid', 'diamond', 'pentagon', 'hexagon', 'heptagon', 'octagon', 'decagon', 'dodecagon',
  'star4', 'star5', 'star6', 'star7', 'star8', 'star10', 'star12', 'star16', 'star24', 'star32',
  'rightArrow', 'leftArrow', 'upArrow', 'downArrow', 'leftRightArrow', 'upDownArrow', 'quadArrow', 'leftRightUpArrow', 'bentArrow', 'uturnArrow', 'curvedRightArrow', 'curvedLeftArrow', 'curvedUpArrow', 'curvedDownArrow', 'stripedRightArrow', 'notchedRightArrow', 'homePlate', 'chevron', 'rightArrowCallout', 'leftArrowCallout', 'circularArrow', 'leftCircularArrow',
  'wedgeRectCallout', 'wedgeRoundRectCallout', 'wedgeEllipseCallout', 'cloudCallout', 'borderCallout1', 'accentCallout1', 'callout1', 'accentBorderCallout1',
  'cloud', 'heart', 'lightningBolt', 'sun', 'moon', 'smileyFace', 'noSmoking', 'blockArc', 'donut', 'arc', 'chord', 'pie', 'pieWedge', 'teardrop', 'frame', 'halfFrame', 'corner', 'diagStripe', 'cross', 'plus', 'plaque', 'can', 'cube', 'bevel', 'foldedCorner',
  'round1Rect', 'round2SameRect', 'round2DiagRect', 'snip1Rect', 'snip2SameRect', 'snip2DiagRect', 'snipRoundRect',
  'flowChartProcess', 'flowChartDecision', 'flowChartTerminator', 'flowChartDocument', 'flowChartMultidocument', 'flowChartPredefinedProcess', 'flowChartInputOutput', 'flowChartPreparation', 'flowChartManualInput', 'flowChartConnector', 'flowChartOr', 'flowChartSort', 'flowChartMagneticDisk', 'flowChartDelay', 'flowChartDisplay', 'flowChartOffpageConnector',
  'actionButtonHome', 'actionButtonHelp', 'actionButtonBlank', 'actionButtonForwardNext',
  'mathPlus', 'mathMinus', 'mathMultiply', 'mathDivide', 'mathEqual', 'mathNotEqual', 'gear6', 'gear9', 'funnel', 'wave', 'doubleWave', 'ribbon', 'ribbon2', 'ellipseRibbon', 'ellipseRibbon2', 'verticalScroll', 'horizontalScroll',
  'bracketPair', 'bracePair', 'leftBracket', 'rightBracket', 'leftBrace', 'rightBrace', 'irregularSeal1', 'irregularSeal2', 'chartPlus', 'chartX', 'chartStar', 'cornerTabs', 'squareTabs', 'plaqueTabs', 'lineInv', 'upDownArrowCallout', 'swooshArrow',
];

/** `<a:prstGeom>` with seeded adjust values when the preset has known ones. */
export function prstGeomXml(c, prst) {
  const r = c.r;
  const adj = ADJ[prst];
  let av = '';
  if (adj && r.chance(0.5)) {
    const n = r.int(1, adj.length);
    av = adj.slice(0, n).map(([name, lo, hi]) => `<a:gd name="${name}" fmla="val ${r.int(lo, hi)}"/>`).join('');
    c.feat('geom:adj');
  }
  return `<a:prstGeom prst="${prst}"><a:avLst>${av}</a:avLst></a:prstGeom>`;
}

/** `<a:custGeom>`: 1-3 sub-paths of lines, quadratic/cubic Béziers and arcs. */
export function custGeomXml(c) {
  const r = c.r;
  c.feat('geom:cust');
  const w = r.pick([100, 1000, 21600, 914400]);
  const h = r.pick([100, 1000, 21600, 457200]);
  const pt = () => `<a:pt x="${r.int(0, w)}" y="${r.int(0, h)}"/>`;
  const paths = [];
  const np = r.int(1, 3);
  for (let i = 0; i < np; i++) {
    const segs = [`<a:moveTo>${pt()}</a:moveTo>`];
    const ns = r.int(2, 7);
    for (let s = 0; s < ns; s++) {
      const k = r.weighted([['l', 40], ['q', 15], ['c', 30], ['a', 15]]);
      if (k === 'l') segs.push(`<a:lnTo>${pt()}</a:lnTo>`);
      else if (k === 'q') segs.push(`<a:quadBezTo>${pt()}${pt()}</a:quadBezTo>`);
      else if (k === 'c') segs.push(`<a:cubicBezTo>${pt()}${pt()}${pt()}</a:cubicBezTo>`);
      else segs.push(`<a:arcTo wR="${r.int(1, w)}" hR="${r.int(1, h)}" stAng="${r.int(0, 21599999)}" swAng="${r.int(-21599999, 21599999)}"/>`);
    }
    if (r.chance(0.6)) segs.push('<a:close/>');
    const fill = r.chance(0.15) ? ` fill="${r.pick(['none', 'norm', 'lighten', 'lightenLess', 'darken', 'darkenLess'])}"` : '';
    const stroke = r.chance(0.1) ? ' stroke="0"' : '';
    paths.push(`<a:path w="${w}" h="${h}"${fill}${stroke}>${segs.join('')}</a:path>`);
  }
  const rect = r.chance(0.3) ? '<a:rect l="l" t="t" r="r" b="b"/>' : '<a:rect l="0" t="0" r="r" b="b"/>';
  return `<a:custGeom><a:avLst/><a:gdLst/><a:ahLst/><a:cxnLst/>${rect}<a:pathLst>${paths.join('')}</a:pathLst></a:custGeom>`;
}
