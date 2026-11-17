// The B6 slide builder (`profile.slide` hook of b1.mjs): slides made for timing and playback. Every
// slide has 2-7 animatable shapes (shapes with text for per-paragraph/word/letter effects, text boxes
// with and without a background, groups, a video and an audio picture with media commands) and a
// `<p:timing>` tree written the way PowerPoint writes it: a main sequence of click steps (first one
// automatic when it starts "with/after previous"), click / with / after-previous effects of the
// entrance, emphasis, exit, motion-path and media classes, repeat, auto-reverse, accel/decel, `spd`,
// `tmFilter`, `fill`, "until next click" ends, `<p:iterate>` by element/word/letter (forward and
// backwards, percentage and absolute gaps), build by paragraph, chart/SmartArt part targets, trigger
// (interactive) sequences and media-bookmark sequences. Every transition kind is written (p, p14, p15,
// p159 inside `mc:AlternateContent`) cycling with the deck and slide number so a group covers them all.
import { NS, XML } from './parts.mjs';
import { cxnXml, grpXml, picXml, spXml } from './shapes.mjs';
import { randomPng } from '../png.mjs';
import { lineXml } from './dml.mjs';

const GRP = '<p:nvGrpSpPr><p:cNvPr id="1" name=""/><p:cNvGrpSpPr/><p:nvPr/></p:nvGrpSpPr><p:grpSpPr><a:xfrm><a:off x="0" y="0"/><a:ext cx="0" cy="0"/><a:chOff x="0" y="0"/><a:chExt cx="0" cy="0"/></a:xfrm></p:grpSpPr>';
const P14 = 'xmlns:p14="http://schemas.microsoft.com/office/powerpoint/2010/main"';
const MC = 'xmlns:mc="http://schemas.openxmlformats.org/markup-compatibility/2006"';
const A = 'xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main"';

class Ids { constructor() { this.n = 2; } next() { return ++this.n; } }

const tgt = (spid, extra = '') => `<p:tgtEl><p:spTgt spid="${spid}">${extra}</p:spTgt></p:tgtEl>`;
const cond = (delay, extra = '') => `<p:cond delay="${delay}"${extra}/>`;
const attrs = (...names) => `<p:attrNameLst>${names.map((n) => `<p:attrName>${n}</p:attrName>`).join('')}</p:attrNameLst>`;
const bhvr = (ids, spid, extra, dur, names, o = {}) => `<p:cBhvr${o.additive ? ` additive="${o.additive}"` : ''}><p:cTn id="${ids.next()}" dur="${dur}"${o.fill === null ? '' : ` fill="${o.fill ?? 'hold'}"`}${extra}>${o.delay ? `<p:stCondLst>${cond(o.delay)}</p:stCondLst>` : ''}</p:cTn>${tgt(spid, o.tgtExtra ?? '')}${names ? attrs(...names) : ''}</p:cBhvr>`;
const setVis = (ids, spid, to = 'visible', o = {}) => `<p:set>${bhvr(ids, spid, '', 1, ['style.visibility'], { delay: o.delay ?? 0, tgtExtra: o.tgtExtra })}<p:to><p:strVal val="${to}"/></p:to></p:set>`;
const anim = (ids, spid, attr, from, to, dur, o = {}) => `<p:anim calcmode="${o.calc ?? 'lin'}" valueType="${o.vt ?? 'num'}">${bhvr(ids, spid, '', dur, [attr], o)}<p:tavLst><p:tav tm="0"><p:val><p:strVal val="${from}"/></p:val></p:tav><p:tav tm="100000"><p:val><p:strVal val="${to}"/></p:val></p:tav></p:tavLst></p:anim>`;
const animTav = (ids, spid, attr, tavs, dur, o = {}) => `<p:anim calcmode="${o.calc ?? 'lin'}" valueType="${o.vt ?? 'num'}">${bhvr(ids, spid, '', dur, [attr], o)}<p:tavLst>${tavs.map(([tm, val, fmla]) => `<p:tav tm="${tm}"${fmla ? ` fmla="${fmla}"` : ''}><p:val><p:${typeof val === 'number' ? 'fltVal' : 'strVal'} val="${val}"/></p:val></p:tav>`).join('')}</p:tavLst></p:anim>`;
const animFromTo = (ids, spid, attr, from, to, dur, o = {}) => `<p:anim${from !== null ? ` from="${from}"` : ''} to="${to}">${bhvr(ids, spid, '', dur, [attr], o)}</p:anim>`;
const animBy = (ids, spid, attr, by, dur, o = {}) => `<p:anim by="${by}">${bhvr(ids, spid, '', dur, [attr], o)}</p:anim>`;
const fx = (ids, spid, dir, filter, dur, o = {}) => `<p:animEffect transition="${dir}" filter="${filter}"${o.prLst ? ` prLst="${o.prLst}"` : ''}>${bhvr(ids, spid, '', dur, null, { fill: null, tgtExtra: o.tgtExtra })}</p:animEffect>`;
const rot = (ids, spid, by, dur, o = {}) => `<p:animRot by="${by}">${bhvr(ids, spid, '', dur, ['r'], o)}</p:animRot>`;
const rotFT = (ids, spid, from, to, dur, o = {}) => `<p:animRot from="${from}" to="${to}">${bhvr(ids, spid, '', dur, ['r'], o)}</p:animRot>`;
const scale = (ids, spid, by, dur, o = {}) => `<p:animScale>${bhvr(ids, spid, '', dur, null, o)}<p:by x="${by[0]}" y="${by[1]}"/></p:animScale>`;
const scaleFT = (ids, spid, from, to, dur, o = {}) => `<p:animScale>${bhvr(ids, spid, '', dur, null, o)}<p:from x="${from[0]}" y="${from[1]}"/><p:to x="${to[0]}" y="${to[1]}"/></p:animScale>`;
const clr = (ids, spid, attr, to, dur, o = {}) => `<p:animClr clrSpc="${o.spc ?? 'rgb'}"${o.dir ? ` dir="${o.dir}"` : ''}>${bhvr(ids, spid, '', dur, [attr], o)}${o.from ? `<p:from><a:srgbClr ${A} val="${o.from}"/></p:from>` : ''}<p:to><a:srgbClr ${A} val="${to}"/></p:to></p:animClr>`;
const clrBy = (ids, spid, attr, hsl, dur, o = {}) => `<p:animClr clrSpc="hsl"${o.dir ? ` dir="${o.dir}"` : ''}>${bhvr(ids, spid, '', dur, [attr], o)}<p:by><p:hsl h="${hsl[0]}" s="${hsl[1]}" l="${hsl[2]}"/></p:by></p:animClr>`;
const motion = (ids, spid, path, dur, o = {}) => `<p:animMotion origin="layout" path="${path}" pathEditMode="${o.edit ?? 'relative'}"${o.rAng ? ` rAng="${o.rAng}"` : ''} ptsTypes="AA">${bhvr(ids, spid, '', dur, ['ppt_x', 'ppt_y'], o)}<p:rCtr x="0" y="0"/></p:animMotion>`;
const motionBy = (ids, spid, by, dur, o = {}) => `<p:animMotion origin="layout">${bhvr(ids, spid, '', dur, ['ppt_x', 'ppt_y'], o)}<p:by x="${by[0]}" y="${by[1]}"/>${o.from ? `<p:from x="${o.from[0]}" y="${o.from[1]}"/>` : ''}</p:animMotion>`;
const cmdNode = (ids, spid, cmd, dur = 0) => `<p:cmd type="call" cmd="${cmd}"><p:cBhvr><p:cTn id="${ids.next()}" dur="${dur || 1}" fill="hold"/>${tgt(spid)}</p:cBhvr></p:cmd>`;

const DIRS4 = ['left', 'right', 'up', 'down'];

// [class, presetID, subtype, name, builder (ids, spid, dur, r, o) -> behaviours xml, {container attrs}]
const EFFECTS = [
  ['entr', 1, 0, 'appear', (ids, s, d, r, o) => setVis(ids, s, 'visible', o)],
  ['entr', 10, 0, 'fade', (ids, s, d, r, o) => setVis(ids, s, 'visible', o) + fx(ids, s, 'in', 'fade', d, o)],
  ['entr', 22, 8, 'wipe', (ids, s, d, r, o) => setVis(ids, s, 'visible', o) + fx(ids, s, 'in', `wipe(${r.pick(DIRS4)})`, d, o)],
  ['entr', 16, 21, 'split', (ids, s, d, r, o) => setVis(ids, s, 'visible', o) + fx(ids, s, 'in', `barn(${r.pick(['inVertical', 'outVertical', 'inHorizontal', 'outHorizontal'])})`, d, o)],
  ['entr', 2, 4, 'fly-in', (ids, s, d, r, o) => setVis(ids, s, 'visible', o) + (r.chance(0.5)
    ? anim(ids, s, 'ppt_x', '#ppt_x', '#ppt_x', d, o) + anim(ids, s, 'ppt_y', '1+#ppt_h/2', '#ppt_y', d, o)
    : anim(ids, s, 'ppt_x', '0-#ppt_w/2', '#ppt_x', d, o) + anim(ids, s, 'ppt_y', '#ppt_y', '#ppt_y', d, o))],
  ['entr', 53, 16, 'zoom', (ids, s, d, r, o) => setVis(ids, s, 'visible', o) + anim(ids, s, 'ppt_w', '0', '#ppt_w', d, o) + anim(ids, s, 'ppt_h', '0', '#ppt_h', d, o)],
  ['entr', 12, 4, 'wheel', (ids, s, d, r, o) => setVis(ids, s, 'visible', o) + fx(ids, s, 'in', `wheel(${r.pick([1, 2, 3, 4, 8])})`, d, o)],
  ['entr', 3, 0, 'blinds', (ids, s, d, r, o) => setVis(ids, s, 'visible', o) + fx(ids, s, 'in', `blinds(${r.pick(['horizontal', 'vertical'])})`, d, o)],
  ['entr', 4, 0, 'box', (ids, s, d, r, o) => setVis(ids, s, 'visible', o) + fx(ids, s, 'in', `box(${r.pick(['in', 'out'])})`, d, o)],
  ['entr', 5, 0, 'checkerboard', (ids, s, d, r, o) => setVis(ids, s, 'visible', o) + fx(ids, s, 'in', `checkerboard(${r.pick(['across', 'down'])})`, d, o)],
  ['entr', 9, 0, 'dissolve', (ids, s, d, r, o) => setVis(ids, s, 'visible', o) + fx(ids, s, 'in', 'dissolve', d, o)],
  ['entr', 14, 0, 'random-bars', (ids, s, d, r, o) => setVis(ids, s, 'visible', o) + fx(ids, s, 'in', `randombar(${r.pick(['horizontal', 'vertical'])})`, d, o)],
  ['entr', 26, 0, 'bounce', (ids, s, d, r, o) => setVis(ids, s, 'visible', o)
    + animTav(ids, s, 'ppt_y', [[0, '0-#ppt_h/2'], [20000, '#ppt_y'], [40000, '#ppt_y-0.1'], [60000, '#ppt_y'], [80000, '#ppt_y-0.04'], [100000, '#ppt_y']], d, o)
    + animTav(ids, s, 'ppt_x', [[0, '#ppt_x'], [100000, '#ppt_x']], d, o)],
  ['entr', 52, 0, 'spiral', (ids, s, d, r, o) => setVis(ids, s, 'visible', o)
    + animTav(ids, s, 'ppt_x', [[0, '#ppt_x+0.4*cos(pi*(1-$))'], [50000, '#ppt_x+0.2*cos(pi*(1-$))'], [100000, '#ppt_x']], d, { ...o, calc: 'lin' })
    + animTav(ids, s, 'ppt_y', [[0, '#ppt_y+0.4*sin(pi*(1-$))'], [100000, '#ppt_y']], d, o)],
  ['entr', 31, 0, 'grow-turn', (ids, s, d, r, o) => setVis(ids, s, 'visible', o) + anim(ids, s, 'ppt_w', '0', '#ppt_w', d, o) + anim(ids, s, 'ppt_h', '0', '#ppt_h', d, o) + anim(ids, s, 'style.rotation', '90', '0', d, o) + fx(ids, s, 'in', 'fade', d, o)],
  ['entr', 37, 0, 'ease-in', (ids, s, d, r, o) => setVis(ids, s, 'visible', o) + animTav(ids, s, 'ppt_h', [[0, '0'], [100000, '#ppt_h', '$*$']], d, o)],
  ['emph', 8, 0, 'spin', (ids, s, d, r, o) => rot(ids, s, r.pick([21600000, 5400000, -10800000, 43200000]), d, o)],
  ['emph', 6, 0, 'grow-shrink', (ids, s, d, r, o) => scale(ids, s, [r.pick([150000, 50000, 200000, 80000]), r.pick([150000, 50000, 200000, 80000])], d, o)],
  ['emph', 26, 0, 'pulse', (ids, s, d, r, o) => scale(ids, s, [105000, 105000], d, o)],
  ['emph', 19, 0, 'color', (ids, s, d, r, o) => clr(ids, s, 'style.color', r.hex6(), d, { ...o, from: r.chance(0.4) ? r.hex6() : undefined, spc: r.pick(['rgb', 'hsl']), dir: r.pick(['cw', 'ccw']) })],
  ['emph', 1, 0, 'fill-colour', (ids, s, d, r, o) => clr(ids, s, 'fillcolor', r.hex6(), d, { ...o, spc: r.pick(['rgb', 'hsl']) }) + `<p:set>${bhvr(ids, s, '', 1, ['fill.type'], { delay: 0 })}<p:to><p:strVal val="solid"/></p:to></p:set>`],
  ['emph', 7, 0, 'complementary', (ids, s, d, r, o) => clrBy(ids, s, 'fillcolor', [r.int(-10800000, 10800000), r.int(-50000, 50000), r.int(-30000, 30000)], d, { ...o, dir: r.pick(['cw', 'ccw']) })],
  ['emph', 9, 0, 'transparency', (ids, s, d, r, o) => `<p:animEffect transition="in" filter="image" prLst="opacity: ${r.pick(['0.5', '0.25', '0.75', '1'])}">${bhvr(ids, s, '', d, null, { fill: 'hold' })}</p:animEffect>`],
  ['emph', 15, 0, 'bold-flash', (ids, s, d, r, o) => `<p:set>${bhvr(ids, s, '', d, ['style.fontWeight'], { delay: 0 })}<p:to><p:strVal val="bold"/></p:to></p:set>`],
  ['emph', 14, 0, 'underline', (ids, s, d, r, o) => `<p:set>${bhvr(ids, s, '', d, ['style.textDecorationUnderline'], { delay: 0 })}<p:to><p:strVal val="true"/></p:to></p:set>`],
  ['emph', 3, 0, 'font-style', (ids, s, d, r, o) => animTav(ids, s, 'style.fontStyle', [[0, 'normal'], [50000, 'italic'], [100000, 'normal']], d, { ...o, calc: 'discrete', vt: 'str' })],
  ['emph', 23, 0, 'font-size', (ids, s, d, r, o) => anim(ids, s, 'style.fontSize', '1', r.pick(['2', '0.5', '1.5']), d, o)],
  ['emph', 32, 0, 'teeter', (ids, s, d, r, o) => animTav(ids, s, 'r', [[0, 0], [25000, 5], [75000, '-5'], [100000, 0]], d, { ...o, vt: 'num' })],
  ['emph', 12, 0, 'rot-from-to', (ids, s, d, r, o) => rotFT(ids, s, 0, r.pick([5400000, 10800000, -5400000]), d, o)],
  ['emph', 21, 0, 'scale-from-to', (ids, s, d, r, o) => scaleFT(ids, s, [100000, 100000], [r.int(20, 250) * 1000, r.int(20, 250) * 1000], d, o)],
  ['emph', 28, 0, 'anim-by', (ids, s, d, r, o) => animBy(ids, s, 'ppt_x', r.pick(['0.1', '#ppt_w/2', '-0.1']), d, o)],
  ['emph', 29, 0, 'anim-to', (ids, s, d, r, o) => animFromTo(ids, s, 'ppt_y', r.chance(0.5) ? '#ppt_y' : null, r.pick(['#ppt_y+0.2', '0.5', 'hidden']), d, o)],
  ['exit', 1, 0, 'disappear', (ids, s, d, r, o) => setVis(ids, s, 'hidden', o)],
  ['exit', 10, 0, 'fade-out', (ids, s, d, r, o) => fx(ids, s, 'out', 'fade', d, o) + setVis(ids, s, 'hidden', { delay: d - 1 })],
  ['exit', 22, 1, 'wipe-out', (ids, s, d, r, o) => fx(ids, s, 'out', `wipe(${r.pick(DIRS4)})`, d, o) + setVis(ids, s, 'hidden', { delay: d - 1 })],
  ['exit', 2, 4, 'fly-out', (ids, s, d, r, o) => anim(ids, s, 'ppt_x', '#ppt_x', '#ppt_x', d, o) + anim(ids, s, 'ppt_y', '#ppt_y', '1+#ppt_h/2', d, o) + setVis(ids, s, 'hidden', { delay: d - 1 })],
  ['exit', 53, 16, 'zoom-out', (ids, s, d, r, o) => anim(ids, s, 'ppt_w', '#ppt_w', '0', d, o) + anim(ids, s, 'ppt_h', '#ppt_h', '0', d, o) + setVis(ids, s, 'hidden', { delay: d - 1 })],
  ['exit', 12, 4, 'wheel-out', (ids, s, d, r, o) => fx(ids, s, 'out', 'wheel(1)', d, o) + setVis(ids, s, 'hidden', { delay: d - 1 })],
  ['path', 0, 0, 'line-path', (ids, s, d, r, o) => motion(ids, s, `M 0 0 L ${(r.int(-30, 30) / 100).toFixed(2)} ${(r.int(-30, 30) / 100).toFixed(2)} E`, d * 3, o)],
  ['path', 0, 0, 'curve-path', (ids, s, d, r, o) => motion(ids, s, `M 0 0 C ${(r.int(-20, 20) / 100).toFixed(3)} ${(r.int(-20, 20) / 100).toFixed(3)} ${(r.int(-30, 30) / 100).toFixed(3)} ${(r.int(-30, 30) / 100).toFixed(3)} ${(r.int(-30, 30) / 100).toFixed(3)} 0 E`, d * 3, o)],
  ['path', 0, 0, 'relative-path', (ids, s, d, r, o) => motion(ids, s, 'm 0 0 l 0.1 0.1 c 0.05 0.05 0.1 0.02 0.2 -0.1 l -0.1 -0.05 z e', d * 3, { ...o, rAng: r.pick([0, 0, 5400000, 2700000]) })],
  ['path', 0, 0, 'exponent-path', (ids, s, d, r, o) => motion(ids, s, 'M -4.58333E-6 -1.11111E-6 L 0.2 0.15 L 1.5E-1 -0.1 E', d * 3, o)],
  ['path', 0, 0, 'odd-path', (ids, s, d, r, o) => motion(ids, s, r.pick(['M 0 0 Z 5 6 L 0.1 0.1 X E', 'L 0.2 0.2', 'M 0.1', 'M 0 0 L 0.1 0.1 Z L 0.2 0.0 E E', 'M 0 0 L 0 0 L 0 0 E', '']), d * 3, o)],
  ['path', 0, 0, 'by-motion', (ids, s, d, r, o) => motionBy(ids, s, [r.pick([0.1, -0.2, 0.25]), r.pick([0.1, -0.2, 0.25])], d * 2, { ...o, from: r.chance(0.4) ? [0.05, 0.05] : undefined })],
];

const DURS = [250, 500, 500, 750, 1000, 2000, 3000];

function effectXml(ids, r, spid, nodeType, grpId, c, opts = {}) {
  const pool = opts.only ? EFFECTS.filter((e) => opts.only.includes(e[0])) : EFFECTS;
  const [cls, pid, sub, name, body] = r.pick(pool);
  c.feat(`b6:${cls}:${name}`);
  const dur = r.pick(DURS);
  const delay = r.chance(0.3) ? r.pick([250, 500, 1000, 2000]) : 0;
  const idV = ids.next();
  const x = [];
  const extra = [];
  if (r.chance(0.12)) { x.push(r.pick([' repeatCount="2000"', ' repeatCount="1500"', ' repeatCount="3000"', ' repeatCount="indefinite"', ' repeatCount="500"'])); c.feat('b6:repeat'); }
  if (r.chance(0.1)) { x.push(' autoRev="1"'); c.feat('b6:autoRev'); }
  if (r.chance(0.08)) { x.push(` accel="${r.int(0, 60000)}" decel="${r.int(0, 60000)}"`); c.feat('b6:accel'); }
  if (r.chance(0.08)) { x.push(` spd="${r.pick([50000, 200000, 300000])}"`); c.feat('b6:spd'); }
  if (r.chance(0.05)) { x.push(' tmFilter="0,0; 0.25,0.4; 0.5,0.5; 0.75,0.6; 1,1"'); c.feat('b6:tmFilter'); }
  const fill = r.chance(0.08) ? r.pick(['remove', 'freeze', 'hold']) : 'hold';
  if (r.chance(0.05)) { extra.push(`<p:endCondLst><p:cond evt="onNext" delay="0"><p:tgtEl><p:sldTgt/></p:tgtEl></p:cond></p:endCondLst>`); c.feat('b6:untilNext'); }
  let iter = '';
  if (opts.iterate) {
    const it = opts.iterate;
    iter = `<p:iterate type="${it.type}"${it.backwards ? ' backwards="1"' : ''}>${it.abs !== undefined ? `<p:tmAbs val="${it.abs}"/>` : `<p:tmPct val="${it.pct}"/>`}</p:iterate>`;
    c.feat(`b6:iterate:${it.type}${it.backwards ? ':back' : ''}`);
  }
  const o = { tgtExtra: opts.tgtExtra };
  return `<p:par><p:cTn id="${idV}" presetID="${pid}" presetClass="${cls}" presetSubtype="${sub}" fill="${fill}" grpId="${grpId}" nodeType="${nodeType}"${x.join('')}><p:stCondLst>${cond(delay)}</p:stCondLst>${extra.join('')}${iter}<p:childTnLst>${body(ids, spid, dur, r, o)}</p:childTnLst></p:cTn></p:par>`;
}

/** A click group: `par` (click) > `par` (delay groups) > effects. `effects` = [{ nodeType, xml }]. */
function groupXml(ids, r, effects, auto) {
  const groups = [];
  let cur = null;
  let acc = 0;
  effects.forEach((e, i) => {
    if (e.nodeType === 'afterEffect' || i === 0 || !cur) {
      acc += 500;
      cur = { delay: e.nodeType === 'afterEffect' && i > 0 ? r.pick([500, 1000, acc]) : 0, effects: [] };
      groups.push(cur);
    }
    cur.effects.push(e.xml);
  });
  const outerId = ids.next();
  const st = auto ? `${cond('indefinite')}<p:cond evt="onBegin" delay="0"><p:tn val="2"/></p:cond>` : cond('indefinite');
  const inner = groups.map((g) => `<p:par><p:cTn id="${ids.next()}" fill="hold"><p:stCondLst>${cond(g.delay)}</p:stCondLst><p:childTnLst>${g.effects.join('')}</p:childTnLst></p:cTn></p:par>`).join('');
  return `<p:par><p:cTn id="${outerId}" fill="hold"><p:stCondLst>${st}</p:stCondLst><p:childTnLst>${inner}</p:childTnLst></p:cTn></p:par>`;
}

/** Targets of a slide: { id, kind: 'text' | 'plain' | 'group' | 'video' | 'audio' }. */
function timingFor(c, targets, media, triggerIds) {
  const r = c.r;
  const ids = new Ids();
  const steps = [];
  const bld = [];
  const text = targets.filter((t) => t.kind === 'text');
  const plain = targets.filter((t) => t.kind !== 'video' && t.kind !== 'audio');
  const usedBg = new Set();
  const nSteps = r.int(1, 4);
  for (let s = 0; s < nSteps; s++) {
    const effects = [];
    const nEff = r.weighted([[1, 40], [2, 30], [3, 20], [4, 10]]);
    const first = s === 0 && r.chance(0.2) ? r.pick(['withEffect', 'afterEffect']) : 'clickEffect';
    for (let e = 0; e < nEff; e++) {
      const nodeType = e === 0 ? first : r.pick(['withEffect', 'withEffect', 'afterEffect']);
      c.feat('b6:trigger:' + nodeType);
      const mode = r.weighted([['plain', 45], ['iterate', 22], ['para', 15], ['bg', 4], ['chart', 3], ['dgm', 2], ['group', 9]]);
      const pool = mode === 'iterate' || mode === 'para' || mode === 'bg' ? (text.length ? text : plain) : plain;
      if (!pool.length) continue;
      const tg = r.pick(pool);
      let opts = {};
      if (mode === 'iterate' && tg.kind === 'text') {
        const type = r.pick(['wd', 'lt', 'el']);
        const it = { type, backwards: r.chance(0.25) };
        if (r.chance(0.5)) it.pct = r.pick([10000, 25000, 50000, 5000]); else it.abs = r.pick([30, 50, 100, 200, 500]);
        opts = { iterate: it };
        if (r.chance(0.3)) opts.tgtExtra = `<p:txEl><p:pRg st="${r.int(0, 1)}" end="${r.int(1, 2)}"/></p:txEl>`;
        else if (r.chance(0.15)) opts.tgtExtra = `<p:txEl><p:charRg st="0" end="${r.int(2, 9)}"/></p:txEl>`;
        if (r.chance(0.4)) usedBg.add(tg.id);
      } else if (mode === 'para' && tg.kind === 'text') {
        const st = r.int(0, 2);
        opts = { tgtExtra: `<p:txEl><p:pRg st="${st}" end="${st + r.int(0, 1)}"/></p:txEl>` };
      } else if (mode === 'bg') opts = { tgtExtra: '<p:bg/>' };
      else if (mode === 'chart') opts = { tgtExtra: `<p:graphicEl><a:chart seriesIdx="${r.int(-4, 2)}" categoryIdx="${r.int(-4, 2)}" bldStep="${r.pick(['gridLegend', 'series', 'category', 'ptInSeries', 'ptInCategory', 'allPts'])}"/></p:graphicEl>` };
      else if (mode === 'dgm') opts = { tgtExtra: `<p:graphicEl><a:dgm id="{${r.int(10000000, 99999999)}-0000-4000-8000-000000000000}"/></p:graphicEl>` };
      bld.push(tg.id);
      effects.push({ nodeType, xml: effectXml(ids, r, tg.id, nodeType, 0, c, opts) });
    }
    // media commands as their own click/auto step
    if (media.length && r.chance(0.5)) {
      const m = r.pick(media);
      const cmds = r.pick([['playFrom(0.0)'], ['playFrom(0.0)', 'pause'], ['playFrom(1.5)', 'pause', 'resume'], ['playFrom(0.0)', 'togglePause', 'togglePause'], ['playFrom(0.0)', 'stop']]);
      const body = cmds.map((cm, k) => `<p:par><p:cTn id="${ids.next()}" fill="hold"><p:stCondLst>${cond(k * 1500)}</p:stCondLst><p:childTnLst>${cmdNode(ids, m.id, cm, k === 0 ? 4000 : 0)}</p:childTnLst></p:cTn></p:par>`).join('');
      const xml = `<p:par><p:cTn id="${ids.next()}" presetID="1" presetClass="mediacall" presetSubtype="0" fill="hold" nodeType="${first === 'clickEffect' ? 'clickEffect' : 'afterEffect'}"><p:stCondLst>${cond(0)}</p:stCondLst><p:childTnLst>${body}</p:childTnLst></p:cTn></p:par>`;
      effects.push({ nodeType: 'clickEffect', xml });
      c.feat('b6:mediacall');
    }
    if (effects.length) steps.push(groupXml(ids, r, effects, s === 0 && first !== 'clickEffect'));
  }
  // media nodes (video/audio) beside the sequence
  const mediaNodes = media.map((m) => {
    const vol = r.chance(0.5) ? ` vol="${r.pick([0, 50000, 80000, 100000])}"` : '';
    const mute = r.chance(0.2) ? ' mute="1"' : '';
    const sws = r.chance(0.3) ? ` showWhenStopped="${r.pick(['0', '1'])}"` : '';
    const full = m.kind === 'video' && r.chance(0.2) ? ' fullScrn="1"' : '';
    const rep = r.chance(0.2) ? ' repeatCount="indefinite"' : '';
    const fill = r.chance(0.3) ? 'remove' : 'hold';
    return `<p:${m.kind}${full}><p:cMediaNode${vol}${mute}${sws}><p:cTn id="${ids.next()}" fill="${fill}" display="0"${rep}><p:stCondLst>${cond('indefinite')}</p:stCondLst><p:endCondLst><p:cond evt="onStopAudio" delay="0"><p:tgtEl><p:sldTgt/></p:tgtEl></p:cond></p:endCondLst></p:cTn>${tgt(m.id)}</p:cMediaNode></p:${m.kind}>`;
  }).join('');
  let interactive = '';
  const spare = plain.filter((t) => !bld.includes(t.id));
  if (triggerIds.length && spare.length && r.chance(0.5)) {
    const trig = r.pick(triggerIds);
    const spid = r.pick(spare).id;
    bld.push(spid);
    c.feat('b6:interactive');
    const g = `<p:par><p:cTn id="${ids.next()}" fill="hold"><p:stCondLst>${cond(0)}</p:stCondLst><p:childTnLst><p:par><p:cTn id="${ids.next()}" fill="hold"><p:stCondLst>${cond(0)}</p:stCondLst><p:childTnLst>${effectXml(ids, r, spid, 'clickEffect', 0, c)}</p:childTnLst></p:cTn></p:par></p:childTnLst></p:cTn></p:par>`;
    interactive = `<p:seq concurrent="1" nextAc="seek"><p:cTn id="${ids.next()}" restart="whenNotActive" fill="hold" evtFilter="cancelBubble" nodeType="interactiveSeq"><p:stCondLst><p:cond evt="onClick" delay="0">${tgt(trig)}</p:cond></p:stCondLst><p:endSync evt="end" delay="0"><p:rtn val="all"/></p:endSync><p:childTnLst>${g}${r.chance(0.3) ? g.replace(/id="\d+"/g, () => `id="${ids.next()}"`) : ''}</p:childTnLst></p:cTn><p:nextCondLst><p:cond evt="onClick" delay="0">${tgt(trig)}</p:cond></p:nextCondLst></p:seq>`;
  }
  // media bookmark sequence: fires when the media passes the point
  let bookmarkSeq = '';
  const vids = media.filter((m) => m.kind === 'video' && m.bookmark);
  if (vids.length && plain.length && r.chance(0.7)) {
    const m = r.pick(vids);
    const spid = r.pick(plain).id;
    bld.push(spid);
    c.feat('b6:bookmark');
    const g = (k) => `<p:par><p:cTn id="${ids.next()}" fill="hold"><p:stCondLst>${cond(0)}</p:stCondLst><p:childTnLst><p:par><p:cTn id="${ids.next()}" fill="hold"><p:stCondLst>${cond(0)}</p:stCondLst><p:childTnLst>${effectXml(ids, r, spid, 'afterEffect', k, c)}</p:childTnLst></p:cTn></p:par></p:childTnLst></p:cTn></p:par>`;
    bookmarkSeq = `<p:seq concurrent="1" nextAc="seek"><p:cTn id="${ids.next()}" restart="whenNotActive" fill="hold" evtFilter="cancelBubble" nodeType="interactiveSeq"><p:stCondLst><p:cond evt="onMediaBookmark" delay="0"><p:tgtEl><p14:bmkTgt ${P14} spid="${m.id}" bmkName="${m.bookmark}"/></p:tgtEl></p:cond></p:stCondLst><p:endSync evt="end" delay="0"><p:rtn val="all"/></p:endSync><p:childTnLst>${g(0)}${r.chance(0.5) ? g(0) : ''}</p:childTnLst></p:cTn></p:seq>`;
  }
  const bldXml = [...new Set(bld)].map((spid) => `<p:bldP spid="${spid}" grpId="0"${usedBg.has(spid) || r.chance(0.15) ? ' animBg="1"' : ''}/>`).join('');
  if (!steps.length && !interactive && !bookmarkSeq && !mediaNodes) return '';
  const main = steps.length ? `<p:seq concurrent="1" nextAc="seek"><p:cTn id="2" dur="indefinite" nodeType="mainSeq"><p:childTnLst>${steps.join('')}</p:childTnLst></p:cTn><p:prevCondLst><p:cond evt="onPrev" delay="0"><p:tgtEl><p:sldTgt/></p:tgtEl></p:cond></p:prevCondLst><p:nextCondLst><p:cond evt="onNext" delay="0"><p:tgtEl><p:sldTgt/></p:tgtEl></p:cond></p:nextCondLst></p:seq>` : '';
  return `<p:timing><p:tnLst><p:par><p:cTn id="1" dur="indefinite" restart="never" nodeType="tmRoot"><p:childTnLst>${main}${mediaNodes}${interactive}${bookmarkSeq}</p:childTnLst></p:cTn></p:par></p:tnLst>${bldXml ? `<p:bldLst>${bldXml}</p:bldLst>` : ''}</p:timing>`;
}

// ───────────────────────── transitions ─────────────────────────

const T_BASIC = [
  ['<p:fade/>'], ['<p:fade thruBlk="1"/>'], ['<p:cut/>'], ['<p:cut thruBlk="1"/>'], ['<p:push dir="l"/>'], ['<p:push dir="u"/>'], ['<p:push/>'], ['<p:wipe dir="r"/>'], ['<p:wipe/>'],
  ['<p:cover dir="rd"/>'], ['<p:cover dir="u"/>'], ['<p:pull dir="ld"/>'], ['<p:pull/>'], ['<p:dissolve/>'], ['<p:blinds dir="vert"/>'], ['<p:blinds dir="horz"/>'],
  ['<p:split orient="vert" dir="out"/>'], ['<p:split orient="horz" dir="in"/>'], ['<p:circle/>'], ['<p:checker dir="horz"/>'], ['<p:comb dir="vert"/>'], ['<p:newsflash/>'],
  ['<p:plus/>'], ['<p:random/>'], ['<p:randomBar dir="vert"/>'], ['<p:strips dir="ru"/>'], ['<p:strips/>'], ['<p:wedge/>'], ['<p:wheel spokes="4"/>'], ['<p:zoom dir="in"/>'], ['<p:zoom dir="out"/>'], ['<p:diamond/>'],
];
const T_P14 = ['vortex', 'switch', 'flip', 'ripple', 'honeycomb', 'prism', 'doors', 'window', 'ferris', 'gallery', 'conveyor', 'pan', 'glitter', 'warp', 'flythrough', 'flash', 'shred', 'reveal', 'wheelReverse'];
const T_DIR = new Set(['vortex', 'switch', 'flip', 'ripple', 'prism', 'doors', 'window', 'ferris', 'gallery', 'conveyor', 'pan', 'glitter', 'warp', 'flythrough', 'reveal']);

/** A transition of any kind (basic, p14, p15, morph), random speed and timing: the group's slides cover them all. */
function transitionFor(c) {
  const r = c.r;
  const total = T_BASIC.length + T_P14.length + 9; // the three rare kinds three times as likely
  const n = r.below(total);
  const spd = r.pick(['fast', 'med', 'slow']);
  const adv = [];
  if (r.chance(0.15)) adv.push('advClick="0"');
  if (r.chance(0.35)) adv.push(`advTm="${r.pick([1000, 3000, 5000, 8000, 12000])}"`);
  const a = adv.length ? ' ' + adv.join(' ') : '';
  const snd = r.chance(0.1) ? '<p:sndAc><p:stSnd><p:snd r:embed="rIdX" name="chime.wav"/></p:stSnd></p:sndAc>' : '';
  c.feat('b6:transition');
  if (n < T_BASIC.length) {
    return { xml: `<p:transition spd="${spd}"${a}>${T_BASIC[n][0]}${snd}</p:transition>` };
  }
  const q = n - T_BASIC.length;
  let body, ns, dur;
  if (q < T_P14.length) {
    const kind = T_P14[q];
    const dir = T_DIR.has(kind) ? ` dir="${r.pick(['l', 'r', 'u', 'd'])}"` : '';
    body = `<p14:${kind}${dir}/>`; ns = `${P14}`; dur = r.pick([500, 1000, 2000, 3000, 750]);
    c.feat('b6:transition:p14');
  } else if (q % 3 === T_P14.length % 3) {
    body = `<p15:prstTrans prst="${r.pick(['fallOver', 'drape', 'curtains', 'wind', 'prestige', 'fracture', 'crush', 'peelOff', 'pageCurlDouble', 'airplane', 'origami'])}"/>`; ns = 'xmlns:p15="http://schemas.microsoft.com/office/powerpoint/2012/main"'; dur = r.pick([1500, 2500]);
    c.feat('b6:transition:p15');
  } else if (q % 3 === (T_P14.length + 1) % 3) {
    body = `<p159:morph option="${r.pick(['byObject', 'byWord', 'byChar'])}"/>`; ns = 'xmlns:p159="http://schemas.microsoft.com/office/powerpoint/2015/09/main"'; dur = r.pick([2000, 3500]);
    c.feat('b6:transition:morph');
  } else {
    body = `<p14:vortex dir="${r.pick(['l', 'r'])}"/>`; ns = P14; dur = 100; // a duration outside fast/med/slow, as the application writes it
  }
  const pfx = body.slice(1, body.indexOf(':'));
  return {
    xml: `<mc:AlternateContent ${MC}><mc:Choice ${ns}${pfx === 'p14' ? '' : ` ${P14}`} Requires="${pfx}"><p:transition spd="${spd}" p14:dur="${dur}"${a}>${body}${snd}</p:transition></mc:Choice><mc:Fallback><p:transition spd="${spd}"${a}><p:fade/></p:transition></mc:Fallback></mc:AlternateContent>`,
  };
}

// ───────────────────────── media shapes ─────────────────────────

function mediaPic(c, area, kind) {
  const r = c.r;
  const id = c.nextId();
  const idx = c.media.length + 1;
  const ext = kind === 'video' ? 'mp4' : 'mp3';
  const bytes = new Uint8Array(64 + r.below(64));
  for (let i = 0; i < bytes.length; i++) bytes[i] = r.below(256);
  c.media.push([`ppt/media/media${idx}.${ext}`, bytes]);
  const link = c.addRel(kind === 'video' ? 'video' : 'audio', `../media/media${idx}.${ext}`);
  const embed = c.addRel('http://schemas.microsoft.com/office/2007/relationships/media', `../media/media${idx}.${ext}`);
  const img = c.addImage(randomPng(r));
  const bookmark = kind === 'video' && r.chance(0.6) ? `B${id}` : null;
  const trim = r.chance(0.3) ? `<p14:trim st="${r.pick([500, 1000])}" end="${r.pick([0, 500])}"/>` : '';
  const fade = r.chance(0.3) ? `<p14:fade in="${r.pick([500, 1000])}" out="${r.pick([0, 500])}"/>` : '';
  const bmk = bookmark ? `<p14:bmkLst><p14:bmk name="${bookmark}" time="${r.pick([1000, 2500, 3500])}"/></p14:bmkLst>` : '';
  const tag = kind === 'video' ? `<a:videoFile r:link="${link}"/>` : `<a:audioFile r:link="${link}"/>`;
  const b = { x: r.int(0, Math.floor(area.w / 2)), y: r.int(0, Math.floor(area.h / 2)), cx: Math.floor(area.w / 4), cy: Math.floor(area.h / 4) };
  c.feat(`b6:media:${kind}`);
  const xml = `<p:pic><p:nvPicPr><p:cNvPr id="${id}" name="${kind} ${id}"/><p:cNvPicPr><a:picLocks noChangeAspect="1"/></p:cNvPicPr><p:nvPr>${tag}<p:extLst><p:ext uri="{DAA4B4D4-6D71-4841-9C94-3DE7FCFB9230}"><p14:media ${P14} r:embed="${embed}">${trim}${fade}${bmk}</p14:media></p:ext></p:extLst></p:nvPr></p:nvPicPr><p:blipFill><a:blip r:embed="${img}"/><a:stretch><a:fillRect/></a:stretch></p:blipFill><p:spPr><a:xfrm><a:off x="${b.x}" y="${b.y}"/><a:ext cx="${b.cx}" cy="${b.cy}"/></a:xfrm><a:prstGeom prst="rect"><a:avLst/></a:prstGeom></p:spPr></p:pic>`;
  return { xml, id: String(id), kind, bookmark };
}

/** The B6 slide: shapes + timing + transition. */
export function b6Slide(c, t, spec, profile) {
  const r = c.r;
  const area = { w: t.w, h: t.h };
  const tops = [];
  const targets = [];
  const media = [];
  const idOf = (x) => /<p:cNvPr id="(\d+)"/.exec(x)?.[1];
  const n = r.int(profile.shapes[0], profile.shapes[1]);
  for (let i = 0; i < n; i++) {
    const kind = r.weighted([['text', 40], ['plain', 28], ['group', 10], ['pic', 8], ['cxn', 4], ['video', 6], ['audio', 4]]);
    let xml;
    if (kind === 'text') { xml = spXml(c, area, { textBox: true }); targets.push({ id: idOf(xml), kind: 'text' }); }
    else if (kind === 'plain') { xml = spXml(c, area, { text: r.chance(0.5) }); targets.push({ id: idOf(xml), kind: /<p:txBody>/.test(xml) ? 'text' : 'plain' }); }
    else if (kind === 'group') { xml = grpXml(c, area); targets.push({ id: idOf(xml), kind: 'group' }); }
    else if (kind === 'pic') { xml = picXml(c, area); targets.push({ id: idOf(xml), kind: 'plain' }); }
    else if (kind === 'cxn') { xml = cxnXml(c, area); targets.push({ id: idOf(xml), kind: 'plain' }); }
    else { const m = mediaPic(c, area, kind); xml = m.xml; media.push(m); targets.push({ id: m.id, kind }); }
    tops.push(xml);
  }
  if (!targets.some((x) => x.kind === 'text')) { const xml = spXml(c, area, { textBox: true }); tops.push(xml); targets.push({ id: idOf(xml), kind: 'text' }); }
  const trigger = targets.filter((x) => x.kind !== 'video' && x.kind !== 'audio').map((x) => x.id);
  const timing = r.chance(0.92) ? timingFor(c, targets, media, trigger) : '';
  const tr = transitionFor(c);
  const show = r.chance(0.05) ? ' show="0"' : '';
  if (show) c.feat('b6:hidden');
  return `${XML}<p:sld ${NS}${show}><p:cSld><p:spTree>${GRP}${tops.join('')}</p:spTree></p:cSld><p:clrMapOvr><a:masterClrMapping/></p:clrMapOvr>${tr.xml}${timing}</p:sld>`;
}
