// Package skeleton parts: content types, relationships, presentation, theme, master, layouts,
// notes master, presProps, tableStyles, docProps. Built from a "template" description chosen by
// the seed (palette, fonts, slide size, clrMap, master text styles, layouts).
import { colorXml, effectListXml, esc, fillXml, lineXml } from './dml.mjs';
import { OPEN_FONTS, rPrXml } from './text.mjs';

export const NS = 'xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships" xmlns:p="http://schemas.openxmlformats.org/presentationml/2006/main"';
const XML = '<?xml version="1.0" encoding="UTF-8" standalone="yes"?>\n';
export const REL = 'http://schemas.openxmlformats.org/officeDocument/2006/relationships';

export const relsXml = (rels) => `${XML}<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">${rels.map(([id, type, target, ext]) => `<Relationship Id="${id}" Type="${type.startsWith('http') ? type : `${REL}/${type}`}" Target="${esc(target)}"${ext ? ' TargetMode="External"' : ''}/>`).join('')}</Relationships>`;

const SIZES = [[12192000, 6858000, ''], [9144000, 6858000, 'screen4x3'], [9144000, 5143500, ''], [12192000, 6858000, ''], [10972800, 6858000, 'custom'], [6858000, 9144000, 'custom']];

const PALETTES = [
  ['1F1F1F', 'FFFFFF', '44546A', 'E7E6E6', '4472C4', 'ED7D31', 'A5A5A5', 'FFC000', '5B9BD5', '70AD47', '0563C1', '954F72'],
  ['000000', 'FFFFFF', '1F497D', 'EEECE1', '4F81BD', 'C0504D', '9BBB59', '8064A2', '4BACC6', 'F79646', '0000FF', '800080'],
  ['262626', 'FAFAFA', '3B3B3B', 'DEDEDE', 'E4572E', '17BEBB', 'FFC914', '76B041', '2E282A', 'A8A8A8', '0B5FA5', '7A4E8A'],
  ['0B132B', 'F4F4F9', '1C2541', 'D9DCE5', '3A506B', '5BC0BE', '6FFFE9', 'F26419', 'F6AE2D', '86BBD8', '2A6FDB', '9A4EAE'],
];

/** The deck's design decisions, all drawn from the seed. */
export function makeTemplate(c) {
  const r = c.r;
  const [w, h, type] = r.pick(SIZES);
  const pal = r.pick(PALETTES).slice();
  if (r.chance(0.4)) for (let i = 4; i < 10; i++) pal[i] = r.hex6();
  const dark = r.chance(0.2);
  return {
    w, h, type, pal,
    sysClr: r.chance(0.3),
    major: r.pick(OPEN_FONTS), minor: r.pick(OPEN_FONTS),
    dark,
    bgKind: r.weighted([['ref', 4], ['solid', 3], ['grad', 1]]),
    decor: r.chance(0.5),
    layouts: r.int(2, 4),
    appVersion: r.pick(['16.0000', '12.0000', '14.0000']),
    showPr: r.chance(0.15) ? r.pick(['<p:showPr loop="1" showNarration="1"><p:present/></p:showPr>', '<p:showPr useTimings="0" showAnimation="0"><p:browse/></p:showPr>', '<p:showPr showNarration="1"><p:kiosk/></p:showPr>', '<p:showPr><p:present/><p:sldRg st="1" end="2"/></p:showPr>']) : '',
    // title/body placeholder boxes (master)
    title: { x: Math.round(w * 0.05), y: Math.round(h * 0.05), cx: Math.round(w * 0.9), cy: Math.round(h * 0.17) },
    body: { x: Math.round(w * 0.05), y: Math.round(h * 0.25), cx: Math.round(w * 0.9), cy: Math.round(h * 0.65) },
  };
}

export function contentTypes(parts) {
  const defs = { rels: 'application/vnd.openxmlformats-package.relationships+xml', xml: 'application/xml', png: 'image/png' };
  // B5b: the media the browser cannot decode
  for (const p of parts) void p;
  defs.emf = 'image/x-emf'; defs.wmf = 'image/x-wmf'; defs.pict = 'image/pict'; defs.tif = 'image/tiff';
  const over = {};
  for (const p of parts) {
    if (/^ppt\/slides\/slide\d+\.xml$/.test(p)) over[p] = 'application/vnd.openxmlformats-officedocument.presentationml.slide+xml';
    else if (/^ppt\/slideLayouts\/slideLayout\d+\.xml$/.test(p)) over[p] = 'application/vnd.openxmlformats-officedocument.presentationml.slideLayout+xml';
    else if (/^ppt\/slideMasters\/slideMaster\d+\.xml$/.test(p)) over[p] = 'application/vnd.openxmlformats-officedocument.presentationml.slideMaster+xml';
    else if (/^ppt\/theme\/theme\d+\.xml$/.test(p)) over[p] = 'application/vnd.openxmlformats-officedocument.theme+xml';
    else if (/^ppt\/notesSlides\/notesSlide\d+\.xml$/.test(p)) over[p] = 'application/vnd.openxmlformats-officedocument.presentationml.notesSlide+xml';
    else if (/^ppt\/notesMasters\/notesMaster\d+\.xml$/.test(p)) over[p] = 'application/vnd.openxmlformats-officedocument.presentationml.notesMaster+xml';
  }
  over['/ppt/presentation.xml'] = 'application/vnd.openxmlformats-officedocument.presentationml.presentation.main+xml';
  over['/ppt/presProps.xml'] = 'application/vnd.openxmlformats-officedocument.presentationml.presProps+xml';
  over['/ppt/tableStyles.xml'] = 'application/vnd.openxmlformats-officedocument.presentationml.tableStyles+xml';
  over['/docProps/app.xml'] = 'application/vnd.openxmlformats-officedocument.extended-properties+xml';
  over['/docProps/core.xml'] = 'application/vnd.openxmlformats-package.core-properties+xml';
  const o = Object.entries(over).map(([p, t]) => `<Override PartName="${p.startsWith('/') ? p : '/' + p}" ContentType="${t}"/>`).join('');
  return `${XML}<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types">${Object.entries(defs).map(([e, t]) => `<Default Extension="${e}" ContentType="${t}"/>`).join('')}${o}</Types>`;
}

const clr = (name, hex, sys) => (sys ? `<a:${name}><a:sysClr val="${hex === 'FFFFFF' || hex === 'FAFAFA' ? 'window' : 'windowText'}" lastClr="${hex}"/></a:${name}>` : `<a:${name}><a:srgbClr val="${hex}"/></a:${name}>`);

export function themeXml(t, name = 'GenTheme') {
  const [dk1, lt1, dk2, lt2, a1, a2, a3, a4, a5, a6, hl, fh] = t.pal;
  const scheme = [clr('dk1', dk1, t.sysClr), clr('lt1', lt1, t.sysClr), clr('dk2', dk2), clr('lt2', lt2), clr('accent1', a1), clr('accent2', a2), clr('accent3', a3), clr('accent4', a4), clr('accent5', a5), clr('accent6', a6), clr('hlink', hl), clr('folHlink', fh)].join('');
  const ph = '<a:schemeClr val="phClr"/>';
  const grad = (a, b) => `<a:gradFill rotWithShape="1"><a:gsLst><a:gs pos="0"><a:schemeClr val="phClr"><a:tint val="${a}"/></a:schemeClr></a:gs><a:gs pos="100000"><a:schemeClr val="phClr"><a:shade val="${b}"/></a:schemeClr></a:gs></a:gsLst><a:lin ang="5400000" scaled="0"/></a:gradFill>`;
  return `${XML}<a:theme xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main" name="${name}"><a:themeElements><a:clrScheme name="Gen">${scheme}</a:clrScheme><a:fontScheme name="Gen"><a:majorFont><a:latin typeface="${t.major}"/><a:ea typeface=""/><a:cs typeface=""/></a:majorFont><a:minorFont><a:latin typeface="${t.minor}"/><a:ea typeface=""/><a:cs typeface=""/></a:minorFont></a:fontScheme><a:fmtScheme name="Gen"><a:fillStyleLst><a:solidFill>${ph}</a:solidFill>${grad(90000, 80000)}${grad(60000, 50000)}</a:fillStyleLst><a:lnStyleLst><a:ln w="6350" cap="flat" cmpd="sng" algn="ctr"><a:solidFill>${ph}</a:solidFill><a:prstDash val="solid"/></a:ln><a:ln w="12700" cap="flat" cmpd="sng" algn="ctr"><a:solidFill>${ph}</a:solidFill><a:prstDash val="solid"/></a:ln><a:ln w="19050" cap="flat" cmpd="sng" algn="ctr"><a:solidFill>${ph}</a:solidFill><a:prstDash val="solid"/></a:ln></a:lnStyleLst><a:effectStyleLst><a:effectStyle><a:effectLst/></a:effectStyle><a:effectStyle><a:effectLst><a:outerShdw blurRad="57150" dist="19050" dir="5400000" algn="ctr" rotWithShape="0"><a:srgbClr val="000000"><a:alpha val="63000"/></a:srgbClr></a:outerShdw></a:effectLst></a:effectStyle><a:effectStyle><a:effectLst><a:outerShdw blurRad="63500" dist="50800" dir="2700000" algn="tl" rotWithShape="0"><a:srgbClr val="000000"><a:alpha val="40000"/></a:srgbClr></a:outerShdw></a:effectLst></a:effectStyle></a:effectStyleLst><a:bgFillStyleLst><a:solidFill>${ph}</a:solidFill>${grad(95000, 85000)}${grad(70000, 40000)}</a:bgFillStyleLst></a:fmtScheme></a:themeElements></a:theme>`;
}

const GRP = '<p:nvGrpSpPr><p:cNvPr id="1" name=""/><p:cNvGrpSpPr/><p:nvPr/></p:nvGrpSpPr><p:grpSpPr><a:xfrm><a:off x="0" y="0"/><a:ext cx="0" cy="0"/><a:chOff x="0" y="0"/><a:chExt cx="0" cy="0"/></a:xfrm></p:grpSpPr>';
const xf = (b) => `<a:xfrm><a:off x="${b.x}" y="${b.y}"/><a:ext cx="${b.cx}" cy="${b.cy}"/></a:xfrm>`;
const phSp = (id, name, ph, box, body = '') => `<p:sp><p:nvSpPr><p:cNvPr id="${id}" name="${name}"/><p:cNvSpPr><a:spLocks noGrp="1"/></p:cNvSpPr><p:nvPr><p:ph${ph}/></p:nvPr></p:nvSpPr><p:spPr>${box ? `${xf(box)}<a:prstGeom prst="rect"><a:avLst/></a:prstGeom>` : ''}</p:spPr>${body || '<p:txBody><a:bodyPr/><a:lstStyle/><a:p><a:endParaRPr lang="en-US"/></a:p></p:txBody>'}</p:sp>`;

function levelStyles(c, t, tag, { bullets, base }) {
  const r = c.r;
  const out = [];
  for (let i = 1; i <= 9; i++) {
    const marL = bullets ? (i - 1) * 457200 + (i > 1 ? 342900 - 457200 + 457200 : 0) : 0;
    const bu = bullets ? `<a:buFont typeface="${r.pick(OPEN_FONTS)}"/><a:buChar char="${i % 2 ? '•' : '–'}"/>` : '<a:buNone/>';
    const sz = Math.max(1000, base - (i - 1) * (bullets ? 200 : 0));
    out.push(`<a:lvl${i}pPr marL="${bullets ? marL : (i - 1) * 457200}" indent="${bullets ? -342900 : 0}" algn="${tag === 'title' && i === 1 ? r.pick(['l', 'ctr']) : 'l'}" defTabSz="914400" rtl="0" eaLnBrk="1" latinLnBrk="0" hangingPunct="1"><a:lnSpc><a:spcPct val="${bullets ? 90000 : 100000}"/></a:lnSpc><a:spcBef><a:spcPts val="${bullets ? 1000 : 0}"/></a:spcBef>${bu}<a:defRPr sz="${sz}" kern="1200"><a:solidFill><a:schemeClr val="tx1"/></a:solidFill><a:latin typeface="${tag === 'title' ? '+mj-lt' : '+mn-lt'}"/><a:ea typeface="+mn-ea"/><a:cs typeface="+mn-cs"/></a:defRPr></a:lvl${i}pPr>`);
  }
  return out.join('');
}

export function masterXml(c, t, layoutRIds) {
  const r = c.r;
  let bg;
  if (t.bgKind === 'ref') bg = '<p:bg><p:bgRef idx="1001"><a:schemeClr val="bg1"/></p:bgRef></p:bg>';
  else if (t.bgKind === 'solid') bg = `<p:bg><p:bgPr><a:solidFill><a:schemeClr val="bg${r.int(1, 2)}"/></a:solidFill><a:effectLst/></p:bgPr></p:bg>`;
  else bg = `<p:bg><p:bgPr>${fillXml(c, 'grad')}<a:effectLst/></p:bgPr></p:bg>`;
  const decor = t.decor ? `<p:sp><p:nvSpPr><p:cNvPr id="20" name="Band"/><p:cNvSpPr/><p:nvPr userDrawn="1"/></p:nvSpPr><p:spPr><a:xfrm><a:off x="0" y="${t.h - 180000}"/><a:ext cx="${t.w}" cy="180000"/></a:xfrm><a:prstGeom prst="rect"><a:avLst/></a:prstGeom><a:solidFill>${colorXml(c, { mods: true, noPhClr: true })}</a:solidFill><a:ln><a:noFill/></a:ln></p:spPr></p:sp>` : '';
  const hf = { y: t.h - 400000, cy: 300000 };
  const clrMap = t.dark ? 'bg1="dk1" tx1="lt1" bg2="dk2" tx2="lt2"' : 'bg1="lt1" tx1="dk1" bg2="lt2" tx2="dk2"';
  const tx = `<p:txStyles><p:titleStyle>${levelStyles(c, t, 'title', { bullets: false, base: r.pick([3200, 4000, 4400]) }).split('</a:lvl1pPr>')[0]}</a:lvl1pPr></p:titleStyle><p:bodyStyle>${levelStyles(c, t, 'body', { bullets: true, base: r.pick([2000, 2400, 2800]) })}</p:bodyStyle><p:otherStyle><a:defPPr><a:defRPr lang="en-US"/></a:defPPr>${levelStyles(c, t, 'other', { bullets: false, base: 1800 })}</p:otherStyle></p:txStyles>`;
  const ids = layoutRIds.map((rid, i) => `<p:sldLayoutId id="${2147483649 + i}" r:id="${rid}"/>`).join('');
  return `${XML}<p:sldMaster ${NS}><p:cSld>${bg}<p:spTree>${GRP}${decor}${phSp(2, 'Title', ' type="title"', t.title)}${phSp(3, 'Body', ' type="body" idx="1"', t.body)}${phSp(4, 'Date', ' type="dt" sz="half" idx="2"', { x: Math.round(t.w * 0.05), ...hf, cx: Math.round(t.w * 0.2) })}${phSp(5, 'Footer', ' type="ftr" sz="quarter" idx="3"', { x: Math.round(t.w * 0.35), ...hf, cx: Math.round(t.w * 0.3) })}${phSp(6, 'Number', ' type="sldNum" sz="quarter" idx="4"', { x: Math.round(t.w * 0.75), ...hf, cx: Math.round(t.w * 0.2) })}</p:spTree></p:cSld><p:clrMap ${clrMap} accent1="accent1" accent2="accent2" accent3="accent3" accent4="accent4" accent5="accent5" accent6="accent6" hlink="hlink" folHlink="folHlink"/><p:sldLayoutIdLst>${ids}</p:sldLayoutIdLst>${tx}</p:sldMaster>`;
}

/** Layout kinds: [name, type attr, placeholders [type, idx, box fn]] */
export function layoutSpecs(t) {
  const { title, body, w, h } = t;
  const half = (i) => ({ x: i ? Math.round(w * 0.52) : body.x, y: body.y, cx: Math.round(w * 0.43), cy: body.cy });
  return [
    { name: 'Title Slide', type: 'title', phs: [[' type="ctrTitle"', { x: title.x, y: Math.round(h * 0.3), cx: title.cx, cy: Math.round(h * 0.2) }], [' type="subTitle" idx="1"', { x: title.x, y: Math.round(h * 0.55), cx: title.cx, cy: Math.round(h * 0.15) }]] },
    { name: 'Title and Content', type: 'obj', phs: [[' type="title"', null], [' idx="1"', null]] },
    { name: 'Two Content', type: 'twoObj', phs: [[' type="title"', null], [' sz="half" idx="1"', half(0)], [' sz="half" idx="2"', half(1)]] },
    { name: 'Title Only', type: 'titleOnly', phs: [[' type="title"', null]] },
    { name: 'Blank', type: 'blank', phs: [] },
  ];
}

export function layoutXml(c, t, spec, i) {
  const r = c.r;
  const sps = spec.phs.map(([ph, box], k) => phSp(k + 2, `Placeholder ${k + 2}`, ph, box)).join('');
  const hide = r.chance(0.1) ? ' showMasterSp="0"' : '';
  const decor = r.chance(0.2) ? `<p:sp><p:nvSpPr><p:cNvPr id="30" name="Layout accent"/><p:cNvSpPr/><p:nvPr userDrawn="1"/></p:nvSpPr><p:spPr><a:xfrm><a:off x="0" y="0"/><a:ext cx="${Math.round(t.w * 0.03)}" cy="${t.h}"/></a:xfrm><a:prstGeom prst="rect"><a:avLst/></a:prstGeom>${fillXml(c, 'solid')}</p:spPr></p:sp>` : '';
  const ovr = r.chance(0.1) ? `<p:clrMapOvr><a:overrideClrMapping bg1="dk1" tx1="lt1" bg2="dk2" tx2="lt2" accent1="accent1" accent2="accent2" accent3="accent3" accent4="accent4" accent5="accent5" accent6="accent6" hlink="hlink" folHlink="folHlink"/></p:clrMapOvr>` : '<p:clrMapOvr><a:masterClrMapping/></p:clrMapOvr>';
  return `${XML}<p:sldLayout ${NS} type="${spec.type}" preserve="1"${hide}><p:cSld name="${spec.name}"><p:spTree>${GRP}${decor}${sps}</p:spTree></p:cSld>${ovr}</p:sldLayout>`;
}

export function notesMasterXml(t) {
  return `${XML}<p:notesMaster ${NS}><p:cSld><p:bg><p:bgRef idx="1001"><a:schemeClr val="bg1"/></p:bgRef></p:bg><p:spTree>${GRP}<p:sp><p:nvSpPr><p:cNvPr id="2" name="Slide Image"/><p:cNvSpPr><a:spLocks noGrp="1" noRot="1" noChangeAspect="1"/></p:cNvSpPr><p:nvPr><p:ph type="sldImg" idx="2"/></p:nvPr></p:nvSpPr><p:spPr><a:xfrm><a:off x="685800" y="1143000"/><a:ext cx="5486400" cy="3086100"/></a:xfrm><a:prstGeom prst="rect"><a:avLst/></a:prstGeom></p:spPr></p:sp><p:sp><p:nvSpPr><p:cNvPr id="3" name="Notes"/><p:cNvSpPr><a:spLocks noGrp="1"/></p:cNvSpPr><p:nvPr><p:ph type="body" sz="quarter" idx="3"/></p:nvPr></p:nvSpPr><p:spPr><a:xfrm><a:off x="685800" y="4400550"/><a:ext cx="5486400" cy="3600450"/></a:xfrm><a:prstGeom prst="rect"><a:avLst/></a:prstGeom></p:spPr><p:txBody><a:bodyPr/><a:lstStyle/><a:p><a:endParaRPr lang="en-US"/></a:p></p:txBody></p:sp></p:spTree></p:cSld><p:clrMap bg1="lt1" tx1="dk1" bg2="lt2" tx2="dk2" accent1="accent1" accent2="accent2" accent3="accent3" accent4="accent4" accent5="accent5" accent6="accent6" hlink="hlink" folHlink="folHlink"/></p:notesMaster>`;
}

export function notesSlideXml(text) {
  const ps = text.split('\n').map((l) => `<a:p><a:r><a:rPr lang="en-US"/><a:t>${esc(l)}</a:t></a:r></a:p>`).join('');
  return `${XML}<p:notes ${NS}><p:cSld><p:spTree>${GRP}<p:sp><p:nvSpPr><p:cNvPr id="2" name="Slide Image"/><p:cNvSpPr><a:spLocks noGrp="1" noRot="1" noChangeAspect="1"/></p:cNvSpPr><p:nvPr><p:ph type="sldImg"/></p:nvPr></p:nvSpPr><p:spPr/></p:sp><p:sp><p:nvSpPr><p:cNvPr id="3" name="Notes"/><p:cNvSpPr><a:spLocks noGrp="1"/></p:cNvSpPr><p:nvPr><p:ph type="body" idx="1"/></p:nvPr></p:nvSpPr><p:spPr/><p:txBody><a:bodyPr/><a:lstStyle/>${ps}</p:txBody></p:sp></p:spTree></p:cSld><p:clrMapOvr><a:masterClrMapping/></p:clrMapOvr></p:notes>`;
}

export const presPropsXml = (t) => `${XML}<p:presentationPr ${NS}>${t.showPr}</p:presentationPr>`;
export const tableStylesXml = () => `${XML}<a:tblStyleLst xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main" def="{5C22544A-7EE6-4342-B048-85BDC9FD1C3A}"/>`;
export const appXml = (t, nSlides) => `${XML}<Properties xmlns="http://schemas.openxmlformats.org/officeDocument/2006/extended-properties"><Application>UniversalOffice generator</Application><PresentationFormat>Custom</PresentationFormat><Slides>${nSlides}</Slides><AppVersion>${t.appVersion}</AppVersion></Properties>`;
export const coreXml = (title) => `${XML}<cp:coreProperties xmlns:cp="http://schemas.openxmlformats.org/package/2006/metadata/core-properties" xmlns:dc="http://purl.org/dc/elements/1.1/"><dc:title>${esc(title)}</dc:title><dc:creator>generator</dc:creator></cp:coreProperties>`;

export function presentationXml(t, slideRIds, { masterRId, notesMasterRId }) {
  const ids = slideRIds.map((rid, i) => `<p:sldId id="${256 + i}" r:id="${rid}"/>`).join('');
  const sz = `<p:sldSz cx="${t.w}" cy="${t.h}"${t.type ? ` type="${t.type}"` : ''}/>`;
  return `${XML}<p:presentation ${NS} saveSubsetFonts="1"><p:sldMasterIdLst><p:sldMasterId id="2147483648" r:id="${masterRId}"/></p:sldMasterIdLst>${notesMasterRId ? `<p:notesMasterIdLst><p:notesMasterId r:id="${notesMasterRId}"/></p:notesMasterIdLst>` : ''}<p:sldIdLst>${ids}</p:sldIdLst>${sz}<p:notesSz cx="6858000" cy="9144000"/><p:defaultTextStyle><a:defPPr><a:defRPr lang="en-US"/></a:defPPr><a:lvl1pPr marL="0" algn="l" defTabSz="914400" rtl="0" eaLnBrk="1" latinLnBrk="0" hangingPunct="1"><a:defRPr sz="1800" kern="1200"><a:solidFill><a:schemeClr val="tx1"/></a:solidFill><a:latin typeface="+mn-lt"/><a:ea typeface="+mn-ea"/><a:cs typeface="+mn-cs"/></a:defRPr></a:lvl1pPr></p:defaultTextStyle></p:presentation>`;
}
export { XML, effectListXml, lineXml, rPrXml };
