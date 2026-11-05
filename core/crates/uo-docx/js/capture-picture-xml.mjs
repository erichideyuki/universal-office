// Parity capture of the W6 write halves (picture-xml.ts and text-effects.ts) from the frozen TS
// engine (frontier W6 of D1b, facet `picture-xml`):
//
//   pic    patchPicture(raw w:drawing, change) over every distinct picture, shape and group of the
//          corpus, each with every named PictureChange (CHANGES below: one per kind of edit) and with
//          seeded combinations of them; the result text hashed (or `err`)
//   fmt    the exported `*Xml` functions over what readPictureFormat reads back from each drawing
//          (picLineXml, effectLstXml, shadowXml, reflectionXml, scene3dXml, sp3dXml)
//   new    newPictureXml over seeded arguments
//   fx     textEffectsXml(readTextEffects(w:rPr)) over every corpus `w:rPr` that has a w14 child and
//          over seeded ones
//
//   UO_TS_ROOT=<built checkout of main> node core/crates/uo-docx/js/capture-picture-xml.mjs
//
// Writes tests/parity/picture-xml.inputs (`<kind>\t<field>...`, fields escaped with `\\ \n \r \t`; the
// changes are JSON: `null` is a removal, an absent key is `undefined`, the order of the keys of `dist`
// is the order of the attributes added) and tests/parity/picture-xml.capture, one record per
// `<kind>#<index>[#<change>]`. tests/parity_picture_xml.rs replays both.
import { writeFileSync } from 'node:fs';
import { join } from 'node:path';
import { createHash } from 'node:crypto';
import { CaptureWriter, Rng } from '../../uo-testkit/js/testkit.mjs';
import { corpus, crate, readBytes, ts } from './capture-r2-common.mjs';

const { DocxPackage } = await ts('package.js');
const { parseXml } = await ts('xml.js');
const px = await ts('picture-xml.js');
const te = await ts('text-effects.js');

const sha = (s) => createHash('sha256').update(new TextEncoder().encode(s)).digest('hex').slice(0, 32);
const esc = (x) => String(x).replace(/\\/g, '\\\\').replace(/\n/g, '\\n').replace(/\r/g, '\\r').replace(/\t/g, '\\t');
const rng = new Rng(0x5ec0de66);
const pick = (a) => a[rng.below(a.length)];
const chance = (n) => rng.below(n) === 0;

const inputs = ['# picture-xml inputs (js/capture-picture-xml.mjs): `<kind>\\t<field>...`, fields escaped'];
const out = new CaptureWriter(['W6: picture-xml.ts and text-effects.ts write halves over corpus drawings, named changes and seeded inputs', 'facets: out (short sha256 of the text, or err), line, fx, shadow, refl, scene, sp3d']);
const counts = {};
let records = 0;
function add(kind, fields, compute) {
  const i = counts[kind] = (counts[kind] ?? -1) + 1;
  inputs.push([kind, ...fields.map(String).map(esc)].join('\t'));
  const key = `${kind}#${i}`;
  try {
    for (const [facet, v] of Object.entries(compute())) out.set(key, facet, v);
  } catch (e) {
    out.set(key, 'out', 'err');
  }
  records++;
}

// ── the corpus drawings ──
const drawings = new Set();
const shapes = new Set();
const groups = new Set();
const rprs = new Set();
const CAP = { pic: 450, shape: 120, group: 40, rpr: 700 };
const walk = (xml, onEl) => {
  let root;
  try { root = parseXml(xml, { positions: true }); } catch { return; }
  const st = [root];
  while (st.length) {
    const el = st.pop();
    onEl(el);
    for (const c of el.children) if (typeof c !== 'string') st.push(c);
  }
};
for (const { file } of corpus()) {
  let pkg;
  try { pkg = DocxPackage.load(readBytes(file)); } catch { continue; }
  for (const part of pkg.partNames().filter((p) => /^word\/(document|header\d*|footer\d*|footnotes|endnotes)\.xml$/.test(p))) {
    const xml = pkg.getPartText(part);
    if (!xml || (!xml.includes('<w:drawing') && !xml.includes('w14:'))) continue;
    walk(xml, (el) => {
      if (el.name === 'w:drawing' && el.s !== undefined) {
        const raw = xml.slice(el.s, el.e);
        const kind = raw.includes('<pic:pic') ? 'pic' : raw.includes('<wps:wsp') ? 'shape' : raw.includes('<wpg:wgp') ? 'group' : null;
        const set = kind === 'pic' ? drawings : kind === 'shape' ? shapes : kind === 'group' ? groups : null;
        if (set && set.size < CAP[kind]) set.add(raw);
      } else if (el.name === 'w:rPr' && el.s !== undefined && rprs.size < CAP.rpr && el.children.some((c) => typeof c !== 'string' && c.name.startsWith('w14:'))) {
        rprs.add(xml.slice(el.s, el.e));
      }
    });
  }
}

// ── synthetic drawings: the shapes the corpus lacks (anchors with each wrap, missing parts) ──
const NS = 'xmlns:wp="http://schemas.openxmlformats.org/drawingml/2006/wordprocessingDrawing" xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main" xmlns:pic="http://schemas.openxmlformats.org/drawingml/2006/picture" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships" xmlns:wp14="http://schemas.microsoft.com/office/word/2010/wordprocessingDrawing" xmlns:mc="http://schemas.openxmlformats.org/markup-compatibility/2006" xmlns:a14="http://schemas.microsoft.com/office/drawing/2010/main" xmlns:adec="http://schemas.microsoft.com/office/drawing/2017/decorative"';
const PIC = (id, spPr, blip = '<a:blip r:embed="rId4"/>', nv = `<pic:nvPicPr><pic:cNvPr id="${id}" name="Pic ${id}"/><pic:cNvPicPr/></pic:nvPicPr>`) => `<a:graphic><a:graphicData uri="http://schemas.openxmlformats.org/drawingml/2006/picture"><pic:pic>${nv}<pic:blipFill>${blip}<a:stretch><a:fillRect/></a:stretch></pic:blipFill>${spPr}</pic:pic></a:graphicData></a:graphic>`;
const SPPR = '<pic:spPr><a:xfrm><a:off x="0" y="0"/><a:ext cx="952500" cy="476250"/></a:xfrm><a:prstGeom prst="rect"><a:avLst/></a:prstGeom></pic:spPr>';
const inline = (extra = '', spPr = SPPR, blip, nv, attrs = ' distT="0" distB="0" distL="0" distR="0"') =>
  `<w:drawing ${NS}><wp:inline${attrs}><wp:extent cx="952500" cy="476250"/><wp:effectExtent l="0" t="0" r="0" b="0"/><wp:docPr id="1" name="Picture 1"${extra}/><wp:cNvGraphicFramePr><a:graphicFrameLocks noChangeAspect="1"/></wp:cNvGraphicFramePr>${PIC(1, spPr, blip, nv)}</wp:inline></w:drawing>`;
const anchor = (wrap, attrs = '', posH = '<wp:positionH relativeFrom="column"><wp:posOffset>100</wp:posOffset></wp:positionH>', posV = '<wp:positionV relativeFrom="paragraph"><wp:posOffset>200</wp:posOffset></wp:positionV>', spPr = SPPR) =>
  `<w:drawing ${NS}><wp:anchor distT="0" distB="0" distL="114300" distR="114300" simplePos="0" relativeHeight="251658240" behindDoc="0" locked="0" layoutInCell="1" allowOverlap="1"${attrs}><wp:simplePos x="0" y="0"/>${posH}${posV}<wp:extent cx="952500" cy="476250"/><wp:effectExtent l="0" t="0" r="0" b="0"/>${wrap}<wp:docPr id="2" name="Picture 2"/><wp:cNvGraphicFramePr/>${PIC(2, spPr)}<wp14:sizeRelH relativeFrom="page"><wp14:pctWidth>0</wp14:pctWidth></wp14:sizeRelH></wp:anchor></w:drawing>`;
const POLY = '<wp:wrapPolygon edited="0"><wp:start x="0" y="0"/><wp:lineTo x="0" y="21600"/><wp:lineTo x="21600" y="21600"/></wp:wrapPolygon>';
const synth = [
  inline(), inline(' descr="x"'), inline('', SPPR.replace('<a:prstGeom prst="rect"><a:avLst/></a:prstGeom>', '<a:custGeom><a:pathLst/></a:custGeom>')),
  inline('', '<pic:spPr><a:xfrm rot="900000" flipH="1"><a:off x="0" y="0"/><a:ext cx="1" cy="2"/></a:xfrm><a:prstGeom prst="roundRect"><a:avLst><a:gd name="adj" fmla="val 5000"/></a:avLst></a:prstGeom><a:ln w="9525"><a:solidFill><a:srgbClr val="FF0000"/></a:solidFill></a:ln><a:effectLst><a:outerShdw blurRad="1" dist="2" dir="3"><a:srgbClr val="000000"/></a:outerShdw><a:glow rad="5"><a:srgbClr val="FFFF00"/></a:glow><a:softEdge rad="7"/><a:blur rad="9"/></a:effectLst><a:scene3d><a:camera prst="orthographicFront"/><a:lightRig rig="threePt" dir="t"/></a:scene3d><a:sp3d/></pic:spPr>'),
  inline('', SPPR, '<a:blip r:embed="rId4"><a:lum bright="1000"/><a:grayscl/><a:extLst><a:ext uri="{BEBA8EAE-BF5A-486C-A8C5-ECC9F3942E4B}"><a14:imgProps><a14:imgLayer r:embed="rId9"><a14:imgEffect><a14:saturation sat="50000"/></a14:imgEffect></a14:imgLayer></a14:imgProps></a:ext><a:ext uri="{28A0092B-C50C-407E-A947-70E740481C1C}"><a14:useLocalDpi val="0"/></a:ext></a:extLst></a:blip>'),
  inline('', SPPR, '<a:blip r:embed="rId4"/>', '<pic:nvPicPr><pic:cNvPr id="1" name=""/><pic:cNvPicPr/></pic:nvPicPr>'),
  inline('', SPPR, '<a:blip r:embed="rId4"/>', '<pic:nvPicPr><pic:cNvPr id="1" name="n" descr="old"><a:extLst><a:ext uri="{C183D7F6-B498-43B3-948B-1728B52AA6E4}"><adec:decorative val="1"/></a:ext><a:ext uri="other"/></a:extLst></pic:cNvPr><pic:cNvPicPr/></pic:nvPicPr>'),
  inline('', SPPR, '<a:blip r:embed="rId4"/>', '<pic:nvPicPr><pic:cNvPr id="1" name="n"><a:hlinkClick r:id="rId8"/></pic:cNvPr><pic:cNvPicPr/></pic:nvPicPr>'),
  inline('', SPPR, '<a:blip r:embed="rId4"></a:blip>'), inline('', SPPR, '<a:blip/>'),
  inline('', '<pic:spPr/>'), inline('', '<pic:spPr></pic:spPr>'), inline('', ''),
  `<w:drawing ${NS}><wp:inline><wp:extent cx="1" cy="1"/>${PIC(3, SPPR)}</wp:inline></w:drawing>`,
  `<w:drawing ${NS}><wp:inline><wp:docPr id="1" name="a"/></wp:inline></w:drawing>`,
  `<w:drawing ${NS}><wp:inline/></w:drawing>`,
  `<w:drawing ${NS}><wp:inline distT="0"></wp:inline></w:drawing>`,
  anchor('<wp:wrapNone/>'), anchor('<wp:wrapSquare wrapText="bothSides"/>'), anchor(`<wp:wrapTight wrapText="left">${POLY}</wp:wrapTight>`), anchor(`<wp:wrapThrough wrapText="right">${POLY}</wp:wrapThrough>`),
  anchor('<wp:wrapTopAndBottom/>'), anchor('<wp:wrapTight wrapText="bothSides"/>'), anchor(''), anchor('<wp:wrapNone/>', ' behindDoc="1"'),
  anchor('<wp:wrapSquare wrapText="largest"/>', '', '<wp:positionH relativeFrom="margin"><wp:align>center</wp:align></wp:positionH>', '<wp:positionV relativeFrom="page"><wp:align>top</wp:align></wp:positionV>'),
  anchor('<wp:wrapNone/>', '', '<mc:AlternateContent><mc:Choice Requires="wp14"><wp:positionH relativeFrom="page"><wp14:pctPosHOffset>50000</wp14:pctPosHOffset></wp:positionH></mc:Choice><mc:Fallback><wp:positionH relativeFrom="page"><wp:posOffset>3000</wp:posOffset></wp:positionH></mc:Fallback></mc:AlternateContent>'),
  anchor('<wp:wrapNone/>', '', '', ''),
  anchor('<wp:wrapSquare wrapText="bothSides"/>').replace(/<(\/?)wp:/g, '<$1w:'),
  inline().replace(/<(\/?)wp:/g, '<$1xx:').replace('xmlns:wp=', 'xmlns:xx='),
  anchor('<wp:wrapNone/>').replace('<wp:docPr id="2" name="Picture 2"/><wp:cNvGraphicFramePr/>', '<wp:docPr id="2" name="Picture 2" hidden="1"/><wp:cNvGraphicFramePr><a:graphicFrameLocks/></wp:cNvGraphicFramePr>'),
  anchor('<wp:wrapNone/>').replace('<wp:cNvGraphicFramePr/>', '<wp:cNvGraphicFramePr></wp:cNvGraphicFramePr>'),
  anchor('<wp:wrapNone/>').replace('<wp:cNvGraphicFramePr/>', ''),
  inline('', SPPR, undefined, undefined, '').replace('<wp:effectExtent l="0" t="0" r="0" b="0"/>', ''),
  inline('').replace('<wp:extent cx="952500" cy="476250"/>', '<wp:extent/>'),
  inline('').replace('<wp:effectExtent l="0" t="0" r="0" b="0"/>', '<wp:effectExtent l="1" t="2" r="3" b="4"/>'),
  inline('').replace('<wp:cNvGraphicFramePr><a:graphicFrameLocks noChangeAspect="1"/></wp:cNvGraphicFramePr>', '<wp:cNvGraphicFramePr><a:graphicFrameLocks noChangeAspect="1" x="2"/></wp:cNvGraphicFramePr>'),
  `<w:drawing ${NS}><wp:inline distT="0" distB="0" distL="0" distR="0"><wp:extent cx="1" cy="1"/><wp:effectExtent l="0" t="0" r="0" b="0"/><wp:docPr id="1" name="Shape"/><wp:cNvGraphicFramePr/><a:graphic><a:graphicData uri="x"><wps:wsp xmlns:wps="http://schemas.microsoft.com/office/word/2010/wordprocessingShape"><wps:cNvSpPr><a:spLocks noChangeAspect="1"/></wps:cNvSpPr><wps:spPr><a:xfrm><a:off x="0" y="0"/><a:ext cx="1" cy="1"/></a:xfrm><a:prstGeom prst="rect"><a:avLst/></a:prstGeom></wps:spPr></wps:wsp></a:graphicData></a:graphic></wp:inline></w:drawing>`,
  `<w:drawing ${NS}><wp:inline distT="0" distB="0" distL="0" distR="0"><wp:extent cx="1" cy="1"/><wp:docPr id="1" name="Shape"/><wp:cNvGraphicFramePr/><a:graphic><a:graphicData uri="x"><wps:wsp xmlns:wps="http://schemas.microsoft.com/office/word/2010/wordprocessingShape"><wps:cNvSpPr/><wps:spPr/></wps:wsp></a:graphicData></a:graphic></wp:inline></w:drawing>`,
  `<w:drawing ${NS}><wp:inline distT="0" distB="0" distL="0" distR="0"><wp:extent cx="1" cy="1"/><wp:docPr id="1" name="Shape"/><wp:cNvGraphicFramePr/><a:graphic><a:graphicData uri="x"><wps:wsp xmlns:wps="http://schemas.microsoft.com/office/word/2010/wordprocessingShape"><wps:cNvSpPr><a:spLocks noChangeAspect="1" noGrp="1"/></wps:cNvSpPr><wps:spPr/></wps:wsp></a:graphicData></a:graphic></wp:inline></w:drawing>`,
  `<w:drawing ${NS}><wp:inline distT="0" distB="0" distL="0" distR="0"><wp:extent cx="1" cy="1"/><wp:docPr id="1" name="Group"/><wp:cNvGraphicFramePr/><a:graphic><a:graphicData uri="x"><wpg:wgp xmlns:wpg="http://schemas.microsoft.com/office/word/2010/wordprocessingGroup"><wpg:grpSpPr><a:xfrm><a:off x="0" y="0"/><a:ext cx="1" cy="1"/></a:xfrm></wpg:grpSpPr></wpg:wgp></a:graphicData></a:graphic></wp:inline></w:drawing>`,
  `<w:drawing ${NS}><wp:inline><wp:extent cx="1" cy="1"/><a:graphic><a:graphicData/></a:graphic></wp:inline></w:drawing>`,
  '<w:drawing><wp:inline', '<w:drawing/>', '', 'not xml', `<w:drawing ${NS}><wp:inline distT="0"><wp:extent cx="1" cy="1"/>${PIC(1, SPPR)}</wp:inline><wp:inline/></w:drawing>`,
  '<w:drawing xmlns:wp="w" xmlns:a="a" xmlns:pic="p">\n<wp:inline distT="0" distB="0" distL="0" distR="0">\n<wp:extent cx="1" cy="1"/>\n<wp:docPr id="1" name="é😀 &amp; &quot;q&quot;"/>\n<a:graphic><a:graphicData><pic:pic><pic:nvPicPr><pic:cNvPr id="1" name=""/></pic:nvPicPr><pic:blipFill><a:blip r:embed="rId1"/></pic:blipFill><pic:spPr><a:xfrm><a:off x="0" y="0"/><a:ext cx="1" cy="1"/></a:xfrm></pic:spPr></pic:pic></a:graphicData></a:graphic>\n</wp:inline>\n</w:drawing>',
];
for (const s of synth) drawings.add(s);

// ── the named changes ──
const C = (kind, val, mods) => ({ kind, val, ...(mods ? { mods } : {}) });
const RED = C('srgbClr', 'FF0000');
const NAMED = [
  ['wrap-inline', { wrap: 'inline' }],
  ['wrap-square', { wrap: 'square', at: { x: 114300, y: 57150.4 }, wrapText: 'left', newRelativeHeight: 251659264 }],
  ['wrap-tight', { wrap: 'tight', at: { x: 0, y: 0 } }],
  ['wrap-through', { wrap: 'through', at: { x: 5, y: 6 }, wrapText: 'right' }],
  ['wrap-topbottom', { wrap: 'topAndBottom', at: { x: 1, y: 2 } }],
  ['wrap-behind', { wrap: 'behind', at: { x: 1, y: 2 } }],
  ['wrap-front', { wrap: 'front' }],
  ['wrap-square-noat', { wrap: 'square' }],
  ['wraptext-right', { wrapText: 'right' }],
  ['wrap-tight-poly', { wrap: 'tight', polygon: { edited: true, pts: [[0, 0], [0, 100], [100, 100]] } }],
  ['dist-all', { dist: { t: 1000, b: 2000.5, l: 3000, r: 4000 } }],
  ['dist-rl', { dist: { r: 114300, l: 114300 } }],
  ['dist-t', { dist: { t: 7 } }],
  ['pos-align', { posH: { relativeFrom: 'margin', align: 'center' }, posV: { relativeFrom: 'page', align: 'top' } }],
  ['pos-offset', { posH: { relativeFrom: 'column', offset: 12345.6 }, posV: { relativeFrom: 'paragraph', offset: -500.5 } }],
  ['pos-pct', { posH: { relativeFrom: 'page', pct: 50000.4, fallback: 3000000.5 }, posV: { relativeFrom: 'page', pct: 25000 } }],
  ['pos-pct-nofallback', { posH: { relativeFrom: 'page', pct: 1 } }],
  ['pos-empty', { posH: { relativeFrom: 'column' }, posV: { relativeFrom: 'line', align: 'bottom', offset: 5 } }],
  ['locked', { locked: true }],
  ['not-in-cell', { layoutInCell: false, allowOverlap: false }],
  ['behind-doc', { behindDoc: true, relativeHeight: 12345.4 }],
  ['flags-off', { locked: false, layoutInCell: true, allowOverlap: true, behindDoc: false }],
  ['polygon-null', { polygon: null }],
  ['polygon-pts', { polygon: { edited: true, pts: [[0, 0], [0, 100], [100.5, 100], [100, 0]] } }],
  ['polygon-empty', { polygon: { edited: false, pts: [] } }],
  ['polygon-one', { polygon: { edited: false, pts: [[5, 6]] } }],
  ['size', { size: { cx: 1000000.4, cy: 500000.5 } }],
  ['sizerel', { sizeRelH: { relativeFrom: 'margin', pct: 50000.4 }, sizeRelV: null }],
  ['sizerel-null', { sizeRelH: null, sizeRelV: null }],
  ['sizerel-v', { sizeRelV: { relativeFrom: 'page', pct: 12000 } }],
  ['ee', { effectExtent: { l: 1, t: 2.5, r: 3, b: 4 } }],
  ['name', { name: 'Picture 1 <x> & "q"' }],
  ['alt', { descr: 'Alt "text" <b>', title: 'T&C' }],
  ['alt-empty', { descr: '', title: '' }],
  ['alt-name', { name: 'N', descr: 'd' }],
  ['alt-title-name', { title: 't', name: '' }],
  ['hidden-on', { hidden: true }],
  ['hidden-off', { hidden: false }],
  ['decor-on', { decorative: true }],
  ['decor-off', { decorative: false }],
  ['decor-alt', { decorative: true, descr: 'x', name: 'Nm' }],
  ['aspect-on', { noChangeAspect: true }],
  ['aspect-off', { noChangeAspect: false }],
  ['rot', { rot: 5400000.4 }],
  ['rot0', { rot: 0 }],
  ['flip', { flipH: true, flipV: false }],
  ['flip2', { flipH: false, flipV: true }],
  ['src', { srcRect: { l: 1000, t: 0, r: 2000.4, b: -500 } }],
  ['src0', { srcRect: { l: 0, t: 0, r: 0, b: 0 } }],
  ['geom-round', { geom: { prst: 'roundRect', adj: { adj: 16667.4 } } }],
  ['geom-ellipse', { geom: { prst: 'ellipse' } }],
  ['geom-star', { geom: { prst: 'star5', adj: { adj: 19098, hf: 105146, vf: 110557 } } }],
  ['line-solid', { line: { w: 19050, cap: 'rnd', cmpd: 'dbl', algn: 'in', fill: RED, dash: 'dash', join: 'miter', miterLim: 800000 } }],
  ['line-scheme', { line: { fill: C('schemeClr', 'accent1', [['lumMod', 75000], ['alpha', 50000]]), join: 'round' } }],
  ['line-null', { line: null }],
  ['line-nofill', { line: { fill: null } }],
  ['line-raw', { line: { fillRaw: '<a:gradFill><a:gsLst/></a:gradFill>', tailRaw: '<a:headEnd type="arrow"/>', join: 'bevel' } }],
  ['line-empty', { line: {} }],
  ['line-w0', { line: { w: 0, miterLim: 0, join: 'miter' } }],
  ['line-sys', { line: { fill: { kind: 'sysClr', val: 'window', lastClr: 'FFFFFF', mods: [['gray', 0], ['comp', 0], ['inv', 1], ['gamma', 2], ['invGamma', 3]] } } }],
  ['line-prst', { line: { fill: C('prstClr', 'black'), dash: 'sysDot' } }],
  ['shadow-outer', { shadow: { kind: 'outer', blurRad: 50800, dist: 38100, dir: 2700000, sx: 100000, sy: 100000, kx: 0, ky: 0, algn: 'tl', rotWithShape: false, color: C('srgbClr', '000000', [['alpha', 40000]]) } }],
  ['shadow-inner', { shadow: { kind: 'inner', blurRad: 1, dist: 2, dir: 3, sx: 4, algn: 'x', rotWithShape: true, color: RED } }],
  ['shadow-prst', { shadow: { kind: 'prst', prst: 'shdw3', dist: 10, dir: 20, color: RED } }],
  ['shadow-prst-default', { shadow: { kind: 'prst', color: RED } }],
  ['shadow-null', { shadow: null }],
  ['reflection', { reflection: { blurRad: 6350, stA: 52000, stPos: 0, endA: 300, endPos: 35000, dist: 0, dir: 5400000, fadeDir: 5400000, sx: 100000, sy: -100000, kx: 0, ky: 0, algn: 'bl', rotWithShape: false } }],
  ['reflection-min', { reflection: {} }],
  ['reflection-null', { reflection: null }],
  ['glow', { glow: { rad: 101600.4, color: C('srgbClr', 'FFC000', [['alpha', 40000], ['satMod', 175000]]) } }],
  ['glow-null', { glow: null }],
  ['soft', { softEdge: 63500.4 }],
  ['soft-null', { softEdge: null }],
  ['soft0', { softEdge: 0 }],
  ['fx-all', { shadow: { kind: 'outer', blurRad: 1, color: RED }, reflection: { dist: 1 }, glow: { rad: 5, color: RED }, softEdge: 7 }],
  ['3d', { threeD: { camera: { prst: 'perspectiveFront', fov: 5400000, zoom: 100000, rot: { lat: 1, lon: 2, rev: 3 } }, lightRig: { rig: 'balanced', dir: 't', rot: { lat: 0, lon: 0, rev: 0 } }, bevelT: { w: 76200, h: 25400, prst: 'circle' }, bevelB: { w: 1 }, extrusionH: 12700, contourW: 6350, z: 100, prstMaterial: 'plastic', extrusionClr: RED, contourClr: C('srgbClr', '0000FF') } }],
  ['3d-camera', { threeD: { camera: { prst: 'isometricTopUp' } } }],
  ['3d-light', { threeD: { lightRig: { rig: 'flat', dir: 'b' } } }],
  ['3d-sp3d', { threeD: { z: 5, bevelT: {} } }],
  ['3d-empty', { threeD: {} }],
  ['3d-null', { threeD: null }],
  ['blip-all', { blipFx: { alphaModFix: 50000, lum: { bright: 20000, contrast: -10000 }, grayscl: true, biLevel: 40000, duotone: [RED, C('srgbClr', '0000FF')], clrChange: { from: RED, to: C('schemeClr', 'bg1'), useA: true } } }],
  ['blip-none', { blipFx: {} }],
  ['blip-alpha100', { blipFx: { alphaModFix: 100000 } }],
  ['blip-lum0', { blipFx: { lum: { bright: 0, contrast: 0 } } }],
  ['blip-lum-b', { blipFx: { lum: { bright: 5 } } }],
  ['blip-clr', { blipFx: { clrChange: { from: RED, to: RED } } }],
  ['blip-bi0', { blipFx: { biLevel: 0, grayscl: false } }],
  ['a14-all', { a14: { layerRid: 'rId9', saturation: 200000, colorTemp: 4700, sharpen: 50000, bright: 10000, contrast: -20000, otherRaw: ['<a14:artisticBlur radius="10"/>'] } }],
  ['a14-null', { a14: null }],
  ['a14-empty', { a14: {} }],
  ['a14-bc0', { a14: { bright: 0, contrast: 5 } }],
  ['a14-raw', { a14: { otherRaw: ['<a14:x/>', '<a14:y a="1"/>'] } }],
  ['embed', { embed: 'rId77' }],
  ['embed-empty', { embed: '' }],
  ['combo-float', { wrap: 'square', at: { x: 100, y: 200 }, dist: { t: 1, b: 2, l: 3, r: 4 }, size: { cx: 5, cy: 6 }, effectExtent: { l: 1, t: 1, r: 1, b: 1 }, line: { w: 12700, fill: RED }, shadow: { kind: 'outer', color: RED }, descr: 'alt', rot: 60000, flipH: true }],
  ['combo-inline', { wrap: 'inline', size: { cx: 7, cy: 8 }, geom: { prst: 'ellipse' }, glow: { rad: 1, color: RED }, decorative: true, noChangeAspect: true, embed: 'rId5', blipFx: { grayscl: true }, a14: { sharpen: 3 } }],
  ['combo-3d', { threeD: { camera: { prst: 'orthographicFront' }, extrusionH: 5 }, softEdge: 3, reflection: { dist: 2 }, line: null, srcRect: { l: 5, t: 5, r: 5, b: 5 } }],
];
const changeNames = NAMED.map((n) => n[0]);
const changeLines = [];
for (const [name, ch] of NAMED) changeLines.push(['chg', name, JSON.stringify(ch)]);
inputs.push(...changeLines.map((l) => l.map(esc).join('\t')));

// ── patchPicture over every drawing × every named change ──
const allDrawings = [...drawings, ...shapes, ...groups];
const picOut = (raw, ch) => { try { return sha(px.patchPicture(raw, ch)); } catch { return 'err'; } };
inputs.push('# drawings');
const drawIndex = [];
for (const raw of allDrawings) {
  const i = drawIndex.length;
  drawIndex.push(raw);
  inputs.push(['draw', raw].map(esc).join('\t'));
  for (const [name, ch] of NAMED) {
    const key = `pic#${i}#${name}`;
    out.set(key, 'out', picOut(raw, ch));
    records++;
  }
}
// seeded combinations on a subset
const combos = [];
for (let k = 0; k < 6000; k++) {
  const i = rng.below(Math.min(allDrawings.length, 600));
  const n = 2 + rng.below(4);
  let ch = {};
  const names = [];
  for (let j = 0; j < n; j++) { const [name, c] = pick(NAMED); names.push(name); ch = { ...ch, ...c }; }
  combos.push([i, ch]);
  inputs.push(['combo', i, JSON.stringify(ch)].map(esc).join('\t'));
  out.set(`combo#${k}`, 'out', picOut(allDrawings[i], ch));
  records++;
}
counts.pic = allDrawings.length * NAMED.length;

// ── fmt: the exported *Xml functions over what is read back ──
const { readPictureFormat } = px;
for (const raw of allDrawings) {
  add('fmt', [raw], () => {
    const f = readPictureFormat(raw, { anyShape: true });
    const r = {};
    if (!f) return { out: 'none' };
    if (f.line) r.line = sha(px.picLineXml(f.line));
    r.fx = sha(px.effectLstXml(f.effects));
    if (f.effects.shadow) r.shadow = sha(px.shadowXml(f.effects.shadow));
    if (f.effects.reflection) r.refl = sha(px.reflectionXml(f.effects.reflection));
    if (f.threeD) { r.scene = sha(px.scene3dXml(f.threeD)); r.sp3d = sha(px.sp3dXml(f.threeD)); }
    r.out = 'ok';
    return r;
  });
}

// ── newPictureXml ──
const NAMES = ['Picture 1', 'a & b', '<x>', 'q"t', '', 'é😀', "it's"];
for (let i = 0; i < 300; i++) {
  const a = [pick([1, 2, 3, 99, 1e6, 0, -5, 3.5]), pick(NAMES), pick(['rId4', '', 'rId"x', 'rId&']), pick([952500, 0, 1.5, 1e9, 12345.5, -1, 0.4999]), pick([476250, 0, 2.5, 7.6, 1e9])];
  add('new', a, () => ({ out: sha(px.newPictureXml({ id: Number(a[0]), name: a[1], rid: a[2], cx: Number(a[3]), cy: Number(a[4]) })) }));
}

// ── text effects ──
const W14NS = 'xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main" xmlns:w14="http://schemas.microsoft.com/office/word/2010/wordml"';
const fxCol = () => pick(['<w14:srgbClr w14:val="FF0000"/>', '<w14:srgbClr w14:val="ff8800"><w14:alpha w14:val="60000"/><w14:lumMod w14:val="50000"/></w14:srgbClr>', '<w14:schemeClr w14:val="accent1"><w14:satMod w14:val="175000"/></w14:schemeClr>', '<w14:schemeClr w14:val="tx1"/>', '<w14:srgbClr/>', '<w14:srgbClr w14:val="abc"><w14:x w14:val="1.5"/></w14:srgbClr>', '<w14:schemeClr/>']);
const fxFill = () => pick([
  '<w14:noFill/>', `<w14:solidFill>${fxCol()}</w14:solidFill>`, '<w14:solidFill/>',
  `<w14:gradFill><w14:gsLst><w14:gs w14:pos="0">${fxCol()}</w14:gs><w14:gs w14:pos="100000">${fxCol()}</w14:gs></w14:gsLst><w14:lin w14:ang="5400000" w14:scaled="0"/></w14:gradFill>`,
  `<w14:gradFill><w14:gsLst><w14:gs w14:pos="50000.5">${fxCol()}</w14:gs></w14:gsLst><w14:path w14:path="rect"><w14:fillToRect w14:l="50000" w14:t="50000" w14:r="50000" w14:b="50000"/></w14:path></w14:gradFill>`,
  '<w14:gradFill><w14:gsLst/></w14:gradFill>', '<w14:gradFill/>', `<w14:gradFill><w14:gs w14:pos="1">${fxCol()}</w14:gs><w14:lin w14:scaled="1"/></w14:gradFill>`,
  `<w14:gradFill><w14:gsLst><w14:gs w14:pos="x">${fxCol()}</w14:gs></w14:gsLst><w14:path/></w14:gradFill>`,
]);
const A = (k, vals) => (chance(2) ? ` w14:${k}="${pick(vals)}"` : '');
const fxKids = () => {
  const kids = [];
  if (chance(2)) kids.push(`<w14:glow${A('rad', ['63500', '101600.5', 'x', '0'])}>${fxCol()}</w14:glow>`);
  if (chance(2)) kids.push(`<w14:shadow${A('blurRad', ['50800', '0', '1.5'])}${A('dist', ['38100', '0'])}${A('dir', ['2700000', '5400000'])}${A('sx', ['100000', '50000', '1.5'])}${A('sy', ['100000'])}${A('kx', ['0', '5'])}${A('ky', ['0'])}${A('algn', ['tl', 'b', 'ctr'])}>${fxCol()}</w14:shadow>`);
  if (chance(2)) kids.push(`<w14:reflection${A('blurRad', ['6350'])}${A('stA', ['52000', '100000'])}${A('stPos', ['0'])}${A('endA', ['300'])}${A('endPos', ['35000'])}${A('dist', ['0', '1'])}${A('dir', ['5400000'])}${A('fadeDir', ['5400000', '0'])}${A('sx', ['100000'])}${A('sy', ['-100000'])}${A('kx', ['0'])}${A('ky', ['0'])}${A('algn', ['bl'])}/>`);
  if (chance(2)) kids.push(`<w14:textOutline${A('w', ['9525', '12700.4', '0'])}${A('cap', ['rnd', 'flat', ''])}${A('cmpd', ['sng', 'dbl'])}${A('algn', ['ctr', 'in'])}>${chance(2) ? fxFill() : ''}${chance(3) ? `<w14:prstDash w14:val="${pick(['solid', 'dash'])}"/>` : ''}${pick(['', '<w14:round/>', '<w14:bevel/>', '<w14:miter w14:lim="800000"/>', '<w14:miter/>'])}</w14:textOutline>`);
  if (chance(2)) kids.push(`<w14:textFill>${fxFill()}</w14:textFill>`);
  if (chance(4)) kids.push(pick(['<w14:scene3d><w14:camera w14:prst="orthographicFront"/><w14:lightRig w14:rig="threePt" w14:dir="t"/></w14:scene3d>', '<w14:scene3d a="é&amp;"/>', '<w14:scene3d/>']));
  if (chance(4)) kids.push(pick(['<w14:props3d w14:extrusionH="12700"><w14:bevelT w14:w="38100"/></w14:props3d>', '<w14:props3d/>']));
  if (chance(3)) kids.push(`<w14:ligatures${A('val', ['standard', 'all', 'none', 'standardContextual'])}/>`);
  if (chance(3)) kids.push(`<w14:numForm${A('val', ['lining', 'oldStyle', 'default'])}/>`);
  if (chance(3)) kids.push(`<w14:numSpacing${A('val', ['tabular', 'proportional', 'default'])}/>`);
  if (chance(3)) kids.push(`<w14:stylisticSets>${pick(['<w14:styleSet w14:id="1"/><w14:styleSet w14:id="20"/>', '<w14:styleSet w14:id="0"/><w14:styleSet w14:id="21"/>', '', '<w14:styleSet w14:id="3"/>'])}</w14:stylisticSets>`);
  if (chance(3)) kids.push(`<w14:cntxtAlts${A('val', ['0', '1', 'false', 'true'])}/>`);
  return kids;
};
const seededRprs = [];
for (let i = 0; i < 2500; i++) {
  const kids = fxKids();
  const other = chance(4) ? pick(['<w:b/>', '<w:color w:val="FF0000"/>']) : '';
  seededRprs.push(`<w:rPr ${W14NS}>${other}${kids.join('')}</w:rPr>`);
}
seededRprs.push(`<w:rPr ${W14NS}/>`, `<w:rPr ${W14NS}><w:b/></w:rPr>`);
const fxOut = (raw) => {
  const el = parseXml(raw);
  const { fx, ot } = te.readTextEffects(el);
  return { out: sha(te.textEffectsXml(fx, ot)) };
};
for (const raw of [...rprs, ...seededRprs]) add('fx', [raw], () => fxOut(raw));

writeFileSync(join(crate, 'tests', 'parity', 'picture-xml.inputs'), inputs.join('\n') + '\n');
out.write(join(crate, 'tests', 'parity', 'picture-xml.capture'));
console.log('picture-xml:', JSON.stringify({ ...counts, drawings: allDrawings.length, named: NAMED.length, combos: combos.length, records }));
