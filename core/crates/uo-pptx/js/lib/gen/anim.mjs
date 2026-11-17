// Animations (`<p:timing>`) and transitions (`<p:transition>`), in the shapes PowerPoint writes:
// a main sequence of click steps, each with effects (click / with previous / after previous)
// of the entrance, emphasis, exit and motion-path classes, plus interactive (trigger) sequences.

class Ids {
  constructor() { this.n = 2; }
  next() { return ++this.n; }
}

const tgt = (spid, extra = '') => `<p:tgtEl><p:spTgt spid="${spid}">${extra}</p:spTgt></p:tgtEl>`;
const setVis = (ids, spid, to = 'visible', delay = 0) => `<p:set><p:cBhvr><p:cTn id="${ids.next()}" dur="1" fill="hold"><p:stCondLst><p:cond delay="${delay}"/></p:stCondLst></p:cTn>${tgt(spid)}<p:attrNameLst><p:attrName>style.visibility</p:attrName></p:attrNameLst></p:cBhvr><p:to><p:strVal val="${to}"/></p:to></p:set>`;
const anim = (ids, spid, attr, from, to, dur, extra = '') => `<p:anim calcmode="lin" valueType="num"><p:cBhvr${extra}><p:cTn id="${ids.next()}" dur="${dur}" fill="hold"/>${tgt(spid)}<p:attrNameLst><p:attrName>${attr}</p:attrName></p:attrNameLst></p:cBhvr><p:tavLst><p:tav tm="0"><p:val><p:strVal val="${from}"/></p:val></p:tav><p:tav tm="100000"><p:val><p:strVal val="${to}"/></p:val></p:tav></p:tavLst></p:anim>`;
const animEffect = (ids, spid, dir, filter, dur) => `<p:animEffect transition="${dir}" filter="${filter}"><p:cBhvr><p:cTn id="${ids.next()}" dur="${dur}"/>${tgt(spid)}</p:cBhvr></p:animEffect>`;

// [class, presetID, subtype, name, body builder (ids, spid, dur, r) -> xml]
const EFFECTS = [
  ['entr', 1, 0, 'appear', (ids, s) => setVis(ids, s)],
  ['entr', 10, 0, 'fade', (ids, s, d) => setVis(ids, s) + animEffect(ids, s, 'in', 'fade', d)],
  ['entr', 22, 8, 'wipe', (ids, s, d, r) => setVis(ids, s) + animEffect(ids, s, 'in', `wipe(${r.pick(['left', 'right', 'up', 'down'])})`, d)],
  ['entr', 16, 21, 'split', (ids, s, d) => setVis(ids, s) + animEffect(ids, s, 'in', 'barn(inVertical)', d)],
  ['entr', 2, 4, 'fly', (ids, s, d, r) => setVis(ids, s) + (r.chance(0.5) ? anim(ids, s, 'ppt_x', '#ppt_x', '#ppt_x', d) + anim(ids, s, 'ppt_y', '1+#ppt_h/2', '#ppt_y', d) : anim(ids, s, 'ppt_x', '0-#ppt_w/2', '#ppt_x', d) + anim(ids, s, 'ppt_y', '#ppt_y', '#ppt_y', d))],
  ['entr', 53, 16, 'zoom', (ids, s, d) => setVis(ids, s) + anim(ids, s, 'ppt_w', '0', '#ppt_w', d) + anim(ids, s, 'ppt_h', '0', '#ppt_h', d)],
  ['entr', 12, 4, 'wheel', (ids, s, d) => setVis(ids, s) + animEffect(ids, s, 'in', 'wheel(1)', d)],
  ['emph', 8, 0, 'spin', (ids, s, d) => `<p:animRot by="${21600000}"><p:cBhvr><p:cTn id="${ids.next()}" dur="${d}" fill="hold"/>${tgt(s)}<p:attrNameLst><p:attrName>r</p:attrName></p:attrNameLst></p:cBhvr></p:animRot>`],
  ['emph', 6, 0, 'grow', (ids, s, d) => `<p:animScale><p:cBhvr><p:cTn id="${ids.next()}" dur="${d}" fill="hold"/>${tgt(s)}</p:cBhvr><p:by x="150000" y="150000"/></p:animScale>`],
  ['emph', 26, 0, 'pulse', (ids, s, d) => `<p:animScale><p:cBhvr><p:cTn id="${ids.next()}" dur="${d}" autoRev="1" fill="hold"/>${tgt(s)}</p:cBhvr><p:by x="105000" y="105000"/></p:animScale>`],
  ['emph', 19, 0, 'color', (ids, s, d, r) => `<p:animClr clrSpc="rgb" dir="cw"><p:cBhvr override="childStyle"><p:cTn id="${ids.next()}" dur="${d}" fill="hold"/>${tgt(s)}<p:attrNameLst><p:attrName>style.color</p:attrName></p:attrNameLst></p:cBhvr><p:to><a:srgbClr xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main" val="${r.hex6()}"/></p:to></p:animClr>`],
  ['exit', 1, 0, 'disappear', (ids, s) => setVis(ids, s, 'hidden')],
  ['exit', 10, 0, 'fade-out', (ids, s, d) => animEffect(ids, s, 'out', 'fade', d) + setVis(ids, s, 'hidden', d - 1)],
  ['exit', 22, 1, 'wipe-out', (ids, s, d) => animEffect(ids, s, 'out', 'wipe(down)', d) + setVis(ids, s, 'hidden', d - 1)],
  ['path', 0, 0, 'motion', (ids, s, d, r) => `<p:animMotion origin="layout" path="M 0 0 L ${(r.int(-30, 30) / 100).toFixed(2)} ${(r.int(-30, 30) / 100).toFixed(2)} E" pathEditMode="relative" ptsTypes="AA"><p:cBhvr><p:cTn id="${ids.next()}" dur="${d * 3}" fill="hold"/>${tgt(s)}<p:attrNameLst><p:attrName>ppt_x</p:attrName><p:attrName>ppt_y</p:attrName></p:attrNameLst></p:cBhvr><p:rCtr x="0" y="0"/></p:animMotion>`],
];

function effectXml(ids, r, spid, nodeType, grpId, c) {
  const [cls, pid, sub, name, body] = r.pick(EFFECTS);
  c.feat(`anim:${cls}:${name}`);
  const dur = r.pick([250, 500, 500, 750, 1000, 2000]);
  const delay = r.chance(0.3) ? r.pick([250, 500, 1000, 2000]) : 0;
  const idV = ids.next();
  const inner = body(ids, spid, dur, r);
  const rep = r.chance(0.05) ? ' repeatCount="2000"' : '';
  const acc = r.chance(0.05) ? ` accel="${r.int(0, 50000)}" decel="${r.int(0, 50000)}"` : '';
  return `<p:par><p:cTn id="${idV}" presetID="${pid}" presetClass="${cls}" presetSubtype="${sub}" fill="hold" grpId="${grpId}" nodeType="${nodeType}"${rep}${acc}><p:stCondLst><p:cond delay="${delay}"/></p:stCondLst><p:childTnLst>${inner}</p:childTnLst></p:cTn></p:par>`;
}

/**
 * The `<p:timing>` of a slide: `targets` are shape ids (integers as strings). Returns '' when
 * there is nothing to animate.
 */
export function timingXml(c, targets, triggerIds = []) {
  const r = c.r;
  if (!targets.length) return '';
  const ids = new Ids();
  const steps = [];
  const nSteps = r.int(1, Math.min(4, targets.length));
  const bld = new Set();
  let ti = 0;
  for (let s = 0; s < nSteps && ti < targets.length; s++) {
    const nEff = Math.min(r.weighted([[1, 55], [2, 30], [3, 15]]), targets.length - ti);
    const first = r.chance(0.2) && s === 0 ? r.pick(['withEffect', 'afterEffect']) : 'clickEffect';
    const groups = [];
    let cur = null;
    for (let e = 0; e < nEff; e++, ti++) {
      const spid = targets[ti];
      bld.add(spid);
      const nt = e === 0 ? first : r.pick(['withEffect', 'withEffect', 'afterEffect']);
      c.feat('anim:trigger:' + nt);
      if (nt === 'afterEffect' || e === 0 || !cur) { cur = { delay: nt === 'afterEffect' && e > 0 ? r.pick([500, 1000, 'after']) : 0, effects: [] }; groups.push(cur); }
      cur.effects.push(effectXml(ids, r, spid, nt, 0, c));
    }
    const outerId = ids.next();
    const auto = first !== 'clickEffect';
    const cond = auto ? '<p:cond delay="indefinite"/><p:cond evt="onBegin" delay="0"><p:tn val="2"/></p:cond>' : '<p:cond delay="indefinite"/>';
    let acc = 0;
    const inner = groups.map((g) => {
      const d = typeof g.delay === 'number' ? g.delay : (acc += 500);
      return `<p:par><p:cTn id="${ids.next()}" fill="hold"><p:stCondLst><p:cond delay="${d}"/></p:stCondLst><p:childTnLst>${g.effects.join('')}</p:childTnLst></p:cTn></p:par>`;
    }).join('');
    steps.push(`<p:par><p:cTn id="${outerId}" fill="hold"><p:stCondLst>${cond}</p:stCondLst><p:childTnLst>${inner}</p:childTnLst></p:cTn></p:par>`);
  }
  let interactive = '';
  const spare = targets.slice(ti);
  if (triggerIds.length && spare.length && r.chance(0.6)) {
    const trig = r.pick(triggerIds);
    const spid = spare[0];
    bld.add(spid);
    c.feat('anim:interactive');
    interactive = `<p:seq concurrent="1" nextAc="seek"><p:cTn id="${ids.next()}" restart="whenNotActive" fill="hold" evtFilter="cancelBubble" nodeType="interactiveSeq"><p:stCondLst><p:cond evt="onClick" delay="0">${tgt(trig).replace('<p:tgtEl>', '<p:tgtEl>')}</p:cond></p:stCondLst><p:endSync evt="end" delay="0"><p:rtn val="all"/></p:endSync><p:childTnLst><p:par><p:cTn id="${ids.next()}" fill="hold"><p:stCondLst><p:cond delay="0"/></p:stCondLst><p:childTnLst><p:par><p:cTn id="${ids.next()}" fill="hold"><p:stCondLst><p:cond delay="0"/></p:stCondLst><p:childTnLst>${effectXml(ids, r, spid, 'clickEffect', 0, c)}</p:childTnLst></p:cTn></p:par></p:childTnLst></p:cTn></p:par></p:childTnLst></p:cTn><p:nextCondLst><p:cond evt="onClick" delay="0">${tgt(trig)}</p:cond></p:nextCondLst></p:seq>`;
  }
  const bldXml = [...bld].map((spid) => `<p:bldP spid="${spid}" grpId="0"${r.chance(0.2) ? ' animBg="1"' : ''}/>`).join('');
  return `<p:timing><p:tnLst><p:par><p:cTn id="1" dur="indefinite" restart="never" nodeType="tmRoot"><p:childTnLst><p:seq concurrent="1" nextAc="seek"><p:cTn id="2" dur="indefinite" nodeType="mainSeq"><p:childTnLst>${steps.join('')}</p:childTnLst></p:cTn><p:prevCondLst><p:cond evt="onPrev" delay="0"><p:tgtEl><p:sldTgt/></p:tgtEl></p:cond></p:prevCondLst><p:nextCondLst><p:cond evt="onNext" delay="0"><p:tgtEl><p:sldTgt/></p:tgtEl></p:cond></p:nextCondLst></p:seq>${interactive}</p:childTnLst></p:cTn></p:par></p:tnLst>${bldXml ? `<p:bldLst>${bldXml}</p:bldLst>` : ''}</p:timing>`;
}

const TRANSITIONS = [
  () => '<p:fade/>', () => '<p:fade thruBlk="1"/>', (r) => `<p:push dir="${r.pick(['l', 'r', 'u', 'd'])}"/>`, (r) => `<p:wipe dir="${r.pick(['l', 'r', 'u', 'd'])}"/>`,
  (r) => `<p:cover dir="${r.pick(['l', 'r', 'u', 'd', 'lu', 'ru', 'ld', 'rd'])}"/>`, (r) => `<p:pull dir="${r.pick(['l', 'r', 'u', 'd'])}"/>`, () => '<p:dissolve/>',
  (r) => `<p:blinds dir="${r.pick(['horz', 'vert'])}"/>`, (r) => `<p:split orient="${r.pick(['horz', 'vert'])}" dir="${r.pick(['in', 'out'])}"/>`, () => '<p:circle/>',
  (r) => `<p:checker dir="${r.pick(['horz', 'vert'])}"/>`, (r) => `<p:comb dir="${r.pick(['horz', 'vert'])}"/>`, () => '<p:cut/>', () => '<p:newsflash/>', () => '<p:plus/>', () => '<p:random/>',
  (r) => `<p:randomBar dir="${r.pick(['horz', 'vert'])}"/>`, (r) => `<p:strips dir="${r.pick(['lu', 'ru', 'ld', 'rd'])}"/>`, (r) => `<p:wedge/>`, (r) => `<p:wheel spokes="${r.pick([1, 2, 3, 4, 8])}"/>`, (r) => `<p:zoom dir="${r.pick(['in', 'out'])}"/>`, () => '<p:diamond/>',
];

/** A slide transition element, direct or inside an AlternateContent with a p14 transition. */
export function transitionXml(c) {
  const r = c.r;
  if (!r.chance(0.35)) return { xml: '', ns: '' };
  const spd = r.pick(['fast', 'med', 'slow']);
  const adv = [];
  if (r.chance(0.2)) adv.push('advClick="0"');
  if (r.chance(0.25)) adv.push(`advTm="${r.pick([1000, 3000, 5000, 10000])}"`);
  c.feat('transition');
  if (r.chance(0.2)) {
    c.feat('transition:p14');
    const p14 = r.pick(['vortex', 'ripple', 'glitter', 'honeycomb', 'flash', 'prism', 'doors', 'window', 'ferris', 'gallery', 'conveyor', 'pan', 'switch', 'flip', 'warp', 'flythrough', 'shred', 'reveal', 'wheelReverse']);
    const dur = r.pick([500, 1000, 2000, 3000]);
    const dir = ['vortex', 'ripple', 'glitter', 'prism', 'doors', 'window', 'ferris', 'gallery', 'conveyor', 'pan', 'switch', 'flip', 'warp', 'flythrough', 'reveal'].includes(p14) ? ` dir="${r.pick(['l', 'r', 'u', 'd'])}"` : '';
    return { xml: `<mc:AlternateContent xmlns:mc="http://schemas.openxmlformats.org/markup-compatibility/2006"><mc:Choice xmlns:p14="http://schemas.microsoft.com/office/powerpoint/2010/main" Requires="p14"><p:transition spd="${spd}" p14:dur="${dur}"${adv.length ? ' ' + adv.join(' ') : ''}><p14:${p14}${dir}/></p:transition></mc:Choice><mc:Fallback><p:transition spd="${spd}"${adv.length ? ' ' + adv.join(' ') : ''}><p:fade/></p:transition></mc:Fallback></mc:AlternateContent>`, ns: '' };
  }
  const body = r.pick(TRANSITIONS)(r);
  c.feat('transition:basic');
  return { xml: `<p:transition spd="${spd}"${adv.length ? ' ' + adv.join(' ') : ''}>${body}</p:transition>`, ns: '' };
}
