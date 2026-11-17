// Slide-tree elements: shapes (p:sp), connectors (p:cxnSp), pictures (p:pic), groups
// (p:grpSp), tables (p:graphicFrame) and placeholders. `c` is the deck context; ids come from
// `c.nextId()`. Geometry is in EMU on the slide (or in the child space of a group).
import { ADJ, PRESETS, blipEffects, custGeomXml, effectListXml, esc, fillXml, lineXml, prstGeomXml, solidXml, threeDXml } from './dml.mjs';
import { bodyPrXml, lstStyleXml, paragraphXml, txBodyXml } from './text.mjs';
import { randomPng } from '../png.mjs';

/** `<a:xfrm>` (shape) with optional rotation and flips. */
export function xfrmXml(c, box, { tag = 'a:xfrm', rotate = true } = {}) {
  const r = c.r;
  const at = [];
  if (rotate && r.chance(0.2)) { at.push(`rot="${r.weighted([[5400000, 3], [10800000, 2], [16200000, 2], [r.int(1, 21599999), 5]])}"`); c.feat('xfrm:rot'); }
  if (rotate && r.chance(0.1)) { at.push('flipH="1"'); c.feat('xfrm:flipH'); }
  if (rotate && r.chance(0.1)) { at.push('flipV="1"'); c.feat('xfrm:flipV'); }
  return `<${tag}${at.length ? ' ' + at.join(' ') : ''}><a:off x="${box.x}" y="${box.y}"/><a:ext cx="${box.cx}" cy="${box.cy}"/></${tag}>`;
}

/** A random box inside `area` ({w,h}) with a minimum size. */
export function randomBox(c, area) {
  const r = c.r;
  const minW = Math.min(area.w, 150000), minH = Math.min(area.h, 120000);
  const cx = r.int(minW, Math.max(minW, Math.floor(area.w * (r.chance(0.2) ? 0.9 : 0.45))));
  const cy = r.int(minH, Math.max(minH, Math.floor(area.h * (r.chance(0.2) ? 0.9 : 0.45))));
  const x = r.int(0, Math.max(0, area.w - cx));
  const y = r.int(0, Math.max(0, area.h - cy));
  // a few shapes may hang off the slide edge
  const off = r.chance(0.04) ? r.pick([-300000, 300000]) : 0;
  return { x: x + off, y, cx, cy };
}

const nvPr = '<p:nvPr/>';

function cNvPr(c, id, name, descr) {
  const d = descr && c.r.chance(0.3) ? ` descr="${esc(descr)}"` : '';
  const hidden = c.r.chance(0.02) ? ' hidden="1"' : '';
  return `<p:cNvPr id="${id}" name="${esc(name)}"${d}${hidden}/>`;
}

/** A plain shape (p:sp): preset or custom geometry, fill, line, effects, optional text. */
export function spXml(c, area, { text = true, textBox = false, box } = {}) {
  const r = c.r;
  const id = c.nextId();
  const b = box ?? randomBox(c, area);
  const prst = textBox ? 'rect' : r.pick(PRESETS);
  const cust = !textBox && r.chance(0.06);
  c.feat(cust ? 'geom:cust' : 'geom:prst:' + prst);
  const geom = cust ? custGeomXml(c) : prstGeomXml(c, prst);
  const parts = [xfrmXml(c, b), geom];
  parts.push(textBox ? (r.chance(0.3) ? fillXml(c, 'solid') : '<a:noFill/>') : r.chance(0.9) ? fillXml(c) : '');
  if (r.chance(textBox ? 0.1 : 0.75)) parts.push(lineXml(c));
  if (r.chance(0.35)) parts.push(effectListXml(c));
  if (r.chance(0.06)) parts.push(threeDXml(c));
  const styled = r.chance(0.2);
  const style = styled ? `<p:style><a:lnRef idx="${r.int(0, 3)}"><a:schemeClr val="accent${r.int(1, 6)}"/></a:lnRef><a:fillRef idx="${r.int(0, 3)}"><a:schemeClr val="accent${r.int(1, 6)}"/></a:fillRef><a:effectRef idx="${r.int(0, 3)}"><a:schemeClr val="accent${r.int(1, 6)}"/></a:effectRef><a:fontRef idx="${r.pick(['minor', 'major'])}"><a:schemeClr val="${r.pick(['lt1', 'dk1', 'tx1'])}"/></a:fontRef></p:style>` : '';
  if (styled) c.feat('shape:style');
  const tb = text && r.chance(textBox ? 1 : 0.5) ? txBodyXml(c, { levels: r.chance(0.2) }) : '';
  const cnv = textBox ? '<p:cNvSpPr txBox="1"/>' : '<p:cNvSpPr/>';
  return `<p:sp><p:nvSpPr>${cNvPr(c, id, `${textBox ? 'TextBox' : 'Shape'} ${id}`, 'Generated shape')}${cnv}${nvPr}</p:nvSpPr><p:spPr>${parts.join('')}</p:spPr>${style}${tb}</p:sp>`;
}

/** A connector (p:cxnSp): straight, elbow or curved, with arrow heads. */
export function cxnXml(c, area) {
  const r = c.r;
  const id = c.nextId();
  const prst = r.pick(['line', 'straightConnector1', 'bentConnector2', 'bentConnector3', 'bentConnector4', 'bentConnector5', 'curvedConnector2', 'curvedConnector3', 'curvedConnector4']);
  c.feat('connector:' + prst);
  const b = randomBox(c, area);
  const adj = ADJ[prst];
  const av = adj && r.chance(0.5) ? `<a:gd name="${adj[0][0]}" fmla="val ${r.int(adj[0][1], adj[0][2])}"/>` : '';
  return `<p:cxnSp><p:nvCxnSpPr>${cNvPr(c, id, `Connector ${id}`)}<p:cNvCxnSpPr/>${nvPr}</p:nvCxnSpPr><p:spPr>${xfrmXml(c, b)}<a:prstGeom prst="${prst}"><a:avLst>${av}</a:avLst></a:prstGeom>${lineXml(c, { arrows: true })}${r.chance(0.2) ? effectListXml(c) : ''}</p:spPr></p:cxnSp>`;
}

/** A picture (p:pic) with crop, colour effects, geometry and an outline. */
export function picXml(c, area) {
  const r = c.r;
  const id = c.nextId();
  const rId = c.addImage(randomPng(r));
  c.feat('picture');
  const b = randomBox(c, area);
  const src = r.chance(0.3) ? `<a:srcRect l="${r.int(0, 30000)}" t="${r.int(0, 30000)}" r="${r.int(0, 30000)}" b="${r.int(0, 30000)}"/>` : '';
  const geom = r.chance(0.3) ? `<a:prstGeom prst="${r.pick(['ellipse', 'roundRect', 'rect', 'star5', 'hexagon', 'diamond'])}"><a:avLst/></a:prstGeom>` : '<a:prstGeom prst="rect"><a:avLst/></a:prstGeom>';
  return `<p:pic><p:nvPicPr>${cNvPr(c, id, `Picture ${id}`, 'Generated picture')}<p:cNvPicPr><a:picLocks noChangeAspect="1"/></p:cNvPicPr>${nvPr}</p:nvPicPr><p:blipFill><a:blip r:embed="${rId}">${blipEffects(c)}</a:blip>${src}<a:stretch><a:fillRect/></a:stretch></p:blipFill><p:spPr>${xfrmXml(c, b)}${geom}${r.chance(0.3) ? lineXml(c) : ''}${r.chance(0.2) ? effectListXml(c) : ''}</p:spPr></p:pic>`;
}

/** A group (p:grpSp) with its own child coordinate space (scaled, offset, rotated, nested). */
export function grpXml(c, area, depth = 0) {
  const r = c.r;
  const id = c.nextId();
  c.feat('group');
  if (depth) c.feat('group:nested');
  const b = randomBox(c, area);
  // child space: different from the group extent, so children are scaled
  const scale = r.pick([1, 1, 2, 0.5, 1.5]);
  const chExt = { w: Math.max(1, Math.round(b.cx * scale)), h: Math.max(1, Math.round(b.cy * scale)) };
  const chOff = { x: r.chance(0.5) ? 0 : r.int(-100000, 500000), y: r.chance(0.5) ? 0 : r.int(-100000, 500000) };
  if (scale !== 1) c.feat('group:scaled');
  const n = r.int(2, 4);
  const kids = [];
  for (let i = 0; i < n; i++) {
    const kind = r.weighted([['sp', 60], ['pic', 8], ['cxn', 10], ['grp', depth < 1 ? 14 : 0], ['tb', 8]]);
    const sub = { w: chExt.w, h: chExt.h };
    const box = randomBox(c, sub);
    box.x += chOff.x; box.y += chOff.y;
    if (kind === 'sp') kids.push(spXml(c, sub, { box }));
    else if (kind === 'tb') kids.push(spXml(c, sub, { textBox: true, box }));
    else if (kind === 'pic') kids.push(picXml(c, sub));
    else if (kind === 'cxn') kids.push(cxnXml(c, sub));
    else kids.push(grpXml(c, sub, depth + 1));
  }
  const rot = r.chance(0.2) ? ` rot="${r.int(1, 21599999)}"` : '';
  const flip = r.chance(0.1) ? ' flipH="1"' : '';
  if (rot) c.feat('xfrm:rot');
  const grpFill = r.chance(0.2) ? fillXml(c, r.pick(['solid', 'grad'])) : '';
  return `<p:grpSp><p:nvGrpSpPr>${cNvPr(c, id, `Group ${id}`)}<p:cNvGrpSpPr/>${nvPr}</p:nvGrpSpPr><p:grpSpPr><a:xfrm${rot}${flip}><a:off x="${b.x}" y="${b.y}"/><a:ext cx="${b.cx}" cy="${b.cy}"/><a:chOff x="${chOff.x}" y="${chOff.y}"/><a:chExt cx="${chExt.w}" cy="${chExt.h}"/></a:xfrm>${grpFill}</p:grpSpPr>${kids.join('')}</p:grpSp>`;
}

// built-in table style GUIDs (the app resolves them by id) and one unknown (custom) id
const TABLE_STYLES = ['{5C22544A-7EE6-4342-B048-85BDC9FD1C3A}', '{2D5ABB26-0587-4C30-8999-92F81FD0307C}', '{3C2FFA5D-87B4-456A-9821-1D502468CF0F}', '{284E427A-3D55-4303-BF80-6455036E1DE7}', '{5940675A-B579-460E-94D1-54222C63F5DA}', '{D113A9D2-9D6B-4929-AA2D-F23B5EE8CBE7}', '{3B4B98B0-60AC-42C2-AFA5-B58CD77FA1E5}', '{7E9639D4-E3E2-4D34-9284-5A2195B3D0D7}', '{00A15C55-8517-42AA-B614-E9B94910E393}', '{0505E3EF-67EA-436B-97B2-0124C06EBD24}', '{793D81CF-94F2-401A-BA57-92F5A7B2D0C5}'];

function tcXml(c, { span = 1, rowSpan = 1, hMerge = false, vMerge = false } = {}) {
  const r = c.r;
  const at = [];
  if (span > 1) at.push(`gridSpan="${span}"`);
  if (rowSpan > 1) at.push(`rowSpan="${rowSpan}"`);
  if (hMerge) at.push('hMerge="1"');
  if (vMerge) at.push('vMerge="1"');
  const body = hMerge || vMerge || r.chance(0.1) ? '<a:txBody><a:bodyPr/><a:lstStyle/><a:p><a:endParaRPr lang="en-US"/></a:p></a:txBody>' : `<a:txBody><a:bodyPr/><a:lstStyle/>${paragraphXml(c, { maxRuns: 2 })}</a:txBody>`;
  const pr = [];
  const pat = [];
  if (r.chance(0.2)) pat.push(`marL="${r.int(0, 200000)}" marR="${r.int(0, 200000)}" marT="${r.int(0, 100000)}" marB="${r.int(0, 100000)}"`);
  if (r.chance(0.2)) pat.push(`anchor="${r.pick(['t', 'ctr', 'b'])}"`);
  if (r.chance(0.04)) pat.push(`vert="${r.pick(['vert', 'vert270'])}"`);
  for (const side of ['lnL', 'lnR', 'lnT', 'lnB']) {
    if (r.chance(0.2)) {
      c.feat('table:border');
      pr.push(r.chance(0.2) ? `<a:${side} w="0"><a:noFill/></a:${side}>` : `<a:${side} w="${r.pick([6350, 12700, 25400, 38100])}" cap="flat" cmpd="${r.pick(['sng', 'sng', 'dbl'])}"><a:solidFill><a:srgbClr val="${r.hex6()}"/></a:solidFill>${r.chance(0.3) ? `<a:prstDash val="${r.pick(['dash', 'dot', 'solid'])}"/>` : ''}</a:${side}>`);
    }
  }
  if (r.chance(0.3)) { c.feat('table:cellfill'); pr.push(fillXml(c, r.weighted([['solid', 7], ['grad', 2], ['patt', 1], ['none', 1]]))); }
  return `<a:tc${at.length ? ' ' + at.join(' ') : ''}>${body}<a:tcPr${pat.length ? ' ' + pat.join(' ') : ''}>${pr.join('')}</a:tcPr></a:tc>`;
}

/** A table in a graphic frame: style id and flags, merged cells, per-cell borders and fills. */
export function tableXml(c, area) {
  const r = c.r;
  const id = c.nextId();
  c.feat('table');
  const cols = r.int(1, 6), rows = r.int(1, 7);
  const b = randomBox(c, area);
  b.cx = Math.max(b.cx, cols * 200000); b.cy = Math.max(b.cy, rows * 150000);
  const colW = [];
  let left = b.cx;
  for (let i = 0; i < cols; i++) { const w = i === cols - 1 ? left : Math.max(100000, Math.floor(b.cx / cols) + r.int(-50000, 50000)); colW.push(w); left -= w; }
  const flags = ['firstRow', 'lastRow', 'firstCol', 'lastCol', 'bandRow', 'bandCol'].filter(() => r.chance(0.45)).map((f) => `${f}="1"`);
  const style = r.pick(TABLE_STYLES);
  c.feat('table:style');
  const covered = new Map(); // "row,col" -> { h, v } for the cells a merge covers
  const trs = [];
  for (let ri = 0; ri < rows; ri++) {
    const tcs = [];
    for (let ci = 0; ci < cols; ci++) {
      const cov = covered.get(`${ri},${ci}`);
      if (cov) { tcs.push(tcXml(c, { hMerge: cov.h, vMerge: cov.v })); continue; }
      let span = 1, rs = 1;
      if (r.chance(0.12) && ci < cols - 1) span = r.int(2, cols - ci);
      if (r.chance(0.08) && ri < rows - 1) rs = r.int(2, rows - ri);
      if (span > 1 || rs > 1) c.feat('table:merge');
      for (let dy = 0; dy < rs; dy++) for (let dx = 0; dx < span; dx++) if (dx || dy) covered.set(`${ri + dy},${ci + dx}`, { h: dx > 0, v: dy > 0 });
      tcs.push(tcXml(c, { span, rowSpan: rs }));
    }
    trs.push(`<a:tr h="${r.int(150000, 600000)}">${tcs.join('')}</a:tr>`);
  }
  return `<p:graphicFrame><p:nvGraphicFramePr>${cNvPr(c, id, `Table ${id}`)}<p:cNvGraphicFramePr><a:graphicFrameLocks noGrp="1"/></p:cNvGraphicFramePr>${nvPr}</p:nvGraphicFramePr><p:xfrm><a:off x="${b.x}" y="${b.y}"/><a:ext cx="${b.cx}" cy="${b.cy}"/></p:xfrm><a:graphic><a:graphicData uri="http://schemas.openxmlformats.org/drawingml/2006/table"><a:tbl><a:tblPr${flags.length ? ' ' + flags.join(' ') : ''}><a:tableStyleId>${style}</a:tableStyleId></a:tblPr><a:tblGrid>${colW.map((w) => `<a:gridCol w="${w}"/>`).join('')}</a:tblGrid>${trs.join('')}</a:tbl></a:graphicData></a:graphic></p:graphicFrame>`;
}

/** A placeholder shape on a slide: inherits geometry from the layout unless `own` is set. */
export function phXml(c, { type, idx, own, area, texts = 2, levels = false }) {
  const r = c.r;
  const id = c.nextId();
  const ph = `<p:ph${type ? ` type="${type}"` : ''}${idx !== undefined ? ` idx="${idx}"` : ''}${r.chance(0.05) ? ' sz="half"' : ''}/>`;
  c.feat('placeholder:' + (type ?? 'obj'));
  const sp = own ? `<p:spPr>${xfrmXml(c, randomBox(c, area), { rotate: false })}${r.chance(0.2) ? fillXml(c) : ''}${r.chance(0.2) ? lineXml(c) : ''}</p:spPr>` : '<p:spPr/>';
  if (own) c.feat('placeholder:own-xfrm');
  return `<p:sp><p:nvSpPr>${cNvPr(c, id, `Placeholder ${id}`)}<p:cNvSpPr><a:spLocks noGrp="1"/></p:cNvSpPr><p:nvPr>${ph}</p:nvPr></p:nvSpPr>${sp}${txBodyXml(c, { ph: true, paras: texts, levels })}</p:sp>`;
}
export { solidXml, bodyPrXml, lstStyleXml };
