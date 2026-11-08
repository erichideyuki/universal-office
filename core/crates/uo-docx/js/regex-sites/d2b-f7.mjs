// The regex sites of frontier D2b-F7 (universal-write.ts: nextDrawingId, themePartPath,
// textboxDefaults, setAsDefaultTextbox, insertTextbox of a user item, changeImage's asvg strip and
// the glow preset parse) against V8, checked by tests/regex_d2b_f7.rs. A row is `[id, function,
// 'fn']` (see js/regex-diff.mjs); the TS expressions are copied as written.
import { INPUTS as BASE } from './templates.mjs';

// universal-write.ts:14312 (setAsDefaultTextbox)
const spIn = (raw) => {
  const sp = /<wps:spPr\b[^>]*>([\s\S]*?)<\/wps:spPr>/.exec(raw)?.[1] ?? '';
  return sp.replace(/<a:xfrm\b[\s\S]*?<\/a:xfrm>|<a:xfrm\/>/, '').replace(/<a:prstGeom\b[\s\S]*?<\/a:prstGeom>|<a:prstGeom\b[^>]*\/>|<a:custGeom\b[\s\S]*?<\/a:custGeom>/, '');
};
const xfrmOnly = (s) => s.replace(/<a:xfrm\b[\s\S]*?<\/a:xfrm>|<a:xfrm\/>/, '');
const geomOnly = (s) => s.replace(/<a:prstGeom\b[\s\S]*?<\/a:prstGeom>|<a:prstGeom\b[^>]*\/>|<a:custGeom\b[\s\S]*?<\/a:custGeom>/, '');
// universal-write.ts:14633 (nextDrawingId)
const docPrIds = (x) => JSON.stringify([...x.matchAll(/<wp:docPr\b[^>]*?\bid="(\d+)"/g)].map((m) => m[1]));
// universal-write.ts:14390-14393 (insertTextbox of a user item)
const userId = (xml) => xml.replace(/(<wp:docPr\b[^>]*?\bid=")\d+"/, (_m, a) => `${a}77"`);
const userName = (xml) => xml.replace(/(<wp:docPr\b[^>]*?\bname=")[^"]*"/, (_m, a) => `${a}Text Box 77"`);
const userZ = (xml) => xml.replace(/\brelativeHeight="\d+"/, () => `relativeHeight="251659264"`);
// universal-write.ts:14658 (themePartPath)
const themePath = (rels) => {
  const m = rels && /<Relationship\b[^>]*Type="[^"]*\/theme"[^>]*>/.exec(rels)?.[0];
  const t = m && /Target="([^"]+)"/.exec(m)?.[1];
  return t ? `word/${t.replace(/^\/?word\//, '').replace(/^\.\//, '')}` : null;
};
// universal-write.ts:14672 (textboxDefaults)
const txDefaults = (x) => {
  const td = x && /<a:objectDefaults>[\s\S]*?<a:txDef>([\s\S]*?)<\/a:txDef>/.exec(x)?.[1];
  if (!td) return null;
  const spPr = /<a:spPr>([\s\S]*?)<\/a:spPr>|<a:spPr\/>/.exec(td)?.[1] ?? '';
  const bp = /<a:bodyPr\b([^>]*?)\/?>/.exec(td)?.[1] ?? '';
  const at = (n) => new RegExp(`\\s${n}="([^"]*)"`).exec(bp)?.[1];
  return JSON.stringify([spPr, bp, at('lIns') ?? null, at('tIns') ?? null, at('rIns') ?? null, at('bIns') ?? null, at('vert') ?? null, at('wrap') ?? null, at('anchor') ?? null, /<a:spAutoFit\/>/.test(td)]);
};
// universal-write.ts:14701-14703 (the txDef replace of setAsDefaultTextbox)
const txDefSpan = (theme) => (/<a:txDef>[\s\S]*?<\/a:txDef>/.test(theme) ? theme.replace(/<a:txDef>[\s\S]*?<\/a:txDef>/, () => '[TX]') : null);
// universal-write.ts:15643 (changeImage: asvg:svgBlip)
const svgExt = (raw) => raw.replace(/<a:ext uri="\{96DAC541-7B7A-43D3-8B79-37D633B846F1\}">[\s\S]*?<\/a:ext>/g, '');
// universal-write.ts:15439 (setImageEffect glow preset)
const glow = (preset) => { const m = /^glow-(\d+)-(\d)$/.exec(preset); return m ? JSON.stringify([m[1], m[2]]) : null; };

// shape-ops.ts:14-27 (shapeStyleIndex up to the comparison loop, which parity_contour proves)
const squash = (s) => s.replace(/>\s+</g, '><').replace(/\s+/g, ' ');
const styleParts = (raw) => {
  const style = /<wps:style>[\s\S]*?<\/wps:style>/.exec(raw)?.[0];
  if (!style) return null;
  const st = squash(style);
  const sp = /<wps:spPr\b[^>]*>([\s\S]*?)<\/wps:spPr>/.exec(raw)?.[1] ?? '';
  const fillLn = squash(sp.replace(/^[\s\S]*?(<\/a:prstGeom>|<\/a:custGeom>|<a:prstGeom\b[^>]*\/>)/, '')).replace(/<a:effectLst[\s\S]*$|<a:scene3d[\s\S]*$|<a:sp3d[\s\S]*$|<a:extLst[\s\S]*$/, '');
  return JSON.stringify([st, fillLn]);
};

/** [site id, the TS expression, operation]. */
export const SITES = [
  ['d2b-f7.sp_in', spIn, 'fn'],
  ['d2b-f7.strip_xfrm', xfrmOnly, 'fn'],
  ['d2b-f7.strip_geometry', geomOnly, 'fn'],
  ['d2b-f7.doc_pr_ids', docPrIds, 'fn'],
  ['d2b-f7.user_id', userId, 'fn'],
  ['d2b-f7.user_name', userName, 'fn'],
  ['d2b-f7.user_z', userZ, 'fn'],
  ['d2b-f7.theme_path', themePath, 'fn'],
  ['d2b-f7.tx_defaults', txDefaults, 'fn'],
  ['d2b-f7.tx_def_span', txDefSpan, 'fn'],
  ['d2b-f7.svg_ext', svgExt, 'fn'],
  ['d2b-f7.glow', glow, 'fn'],
  ['d2b-f7.squash', squash, 'fn'],
  ['d2b-f7.style_parts', styleParts, 'fn'],
];

const SVG = '<a:ext uri="{96DAC541-7B7A-43D3-8B79-37D633B846F1}">';
const rel = (type, target) => `<Relationship Id="rId1" Type="${type}" Target="${target}"/>`;
const T = 'http://schemas.openxmlformats.org/officeDocument/2006/relationships/theme';
const od = (inner) => `<a:themeElements/><a:objectDefaults><a:txDef>${inner}</a:txDef></a:objectDefaults>`;

const WS = (inner, style = '<wps:style><a:lnRef idx="2"/></wps:style>') => `<wps:wsp><wps:spPr>${inner}</wps:spPr>${style}</wps:wsp>`;
export const INPUTS = [
  ...BASE,
  // shapeStyleIndex
  WS('<a:xfrm/><a:prstGeom prst="rect"><a:avLst/></a:prstGeom><a:solidFill><a:schemeClr val="accent1"/></a:solidFill><a:ln w="12700"/><a:effectLst/>'),
  WS('<a:prstGeom prst="rect"/>\n  <a:noFill/>\n  <a:ln/>'), WS('<a:custGeom><a:x/></a:custGeom> <a:ln> </a:ln> <a:extLst/>'), WS('<a:noFill/>'), WS('<a:ln/><a:scene3d/>'),
  WS('<a:prstGeom prst="a"/>x<a:prstGeom prst="b"></a:prstGeom><a:ln/>'), WS('<a:prstGeom>a</a:prstGeom>b<a:sp3d/>'), WS('a  >  <b>\t\n<c/>'), WS('<a:ln/><a:sp3d/><a:effectLst/>'),
  WS('<a:prstGeomX/>x</a:prstGeom>y'), WS('<a:prstGeom/>'), WS('<a:prstGeom a="/>">'), WS('  <a:prstGeom/>  <a:noFill/>  '), WS('<a:x/> <a:y/>'), WS('> <'), WS('<a:effectLst/><a:ln/>'),
  WS('<a:ln/>', ''), WS('<a:ln/>', '<wps:style>'), WS('<a:ln/>', '<wps:style></wps:style>'), WS('<a:ln/>', '<wps:style>\n<a:lnRef idx="1">\n<a:schemeClr val="accent1"/>\n</a:lnRef>\n</wps:style>'),
  '<wps:style>A</wps:style><wps:spPr>B</wps:spPr>', '<wps:spPr>B</wps:spPr><wps:style>A</wps:style>', '<wps:style>A</wps:style>', '<wps:style>A</wps:style><wps:style>B</wps:style>', '<wps:style>A</wps:style></wps:style>',
  ' a  b\n\nc ', '>  <', '> a <', '>\n\t<x>\r\n<', '<>　<', 'x  y', '﻿>﻿<', '>​<', '>\u0085<',
  // spPr and geometry
  '<wps:spPr><a:xfrm><a:off x="0" y="0"/></a:xfrm><a:prstGeom prst="rect"><a:avLst/></a:prstGeom><a:solidFill/></wps:spPr>',
  '<wps:spPr><a:xfrm/><a:prstGeom prst="rect"/><a:ln/></wps:spPr>',
  '<wps:spPr><a:prstGeom prst="rect"/><a:xfrm/><a:prstGeom prst="ellipse"></a:prstGeom></wps:spPr>',
  '<wps:spPr><a:prstGeomX/></wps:spPr>', '<wps:spPr><a:prstGeom_x/></wps:spPr>', '<wps:spPr><a:prstGeom-x/></wps:spPr>',
  '<wps:spPr><a:custGeom><a:pathLst/></a:custGeom><a:noFill/></wps:spPr>', '<wps:spPr><a:custGeom/><a:noFill/></wps:spPr>', '<wps:spPr><a:custGeomX></a:custGeomX></wps:spPr>',
  '<wps:spPr><a:xfrmX><a:off/></a:xfrmX><a:xfrm/></wps:spPr>', '<wps:spPr><a:xfrm_/></a:xfrm></wps:spPr>', '<wps:spPr><a:xfrm><a:off/></wps:spPr>',
  '<wps:spPr><a:xfrm/></a:xfrm></wps:spPr>', '<wps:spPr><a:xfrm/><a:xfrm/></wps:spPr>', '<wps:spPr><a:xfrm><a:ext/></a:xfrm><a:xfrm/></wps:spPr>',
  '<wps:spPrX><a:xfrm/></wps:spPrX><wps:spPr><a:ln/></wps:spPr>', '<wps:spPr/><a:ln/></wps:spPr>', '<wps:spPr a="b"><a:ln/></wps:spPr>', '<wps:spPr', '<wps:spPr>', '<wps:spPr><a:ln/>',
  '<wps:spPr><a:prstGeom prst="rect"><a:avLst/>', '<wps:spPr><a:prstGeom prst="rect">/></wps:spPr>', '<wps:spPr><a:prstGeom prst="a/b"/>x/></wps:spPr>',
  '<a:prstGeom prst="rect"/>x</a:prstGeom>', '<a:prstGeom/>', '<a:prstGeom>', '<a:prstGeom a="/>">z</a:prstGeom>', '<a:prstGeom a="x/>"', '<a:prstGeom\n prst="rect"\n/>',
  '<a:custGeom></a:custGeom><a:prstGeom/>', '<a:prstGeom/><a:custGeom></a:custGeom>', '<a:custGeom><a:prstGeom/></a:custGeom>', '<a:prstGeom><a:custGeom/></a:prstGeom></a:custGeom>',
  '<a:xfrm>', '<a:xfrm/>', 'x<a:xfrm/>y', '<a:xfrm\n/>', '<a:xfrm></a:xfrm>', '<a:xfrm rot="1"><a:off/></a:xfrm>tail', '<a:xfrm/>…</a:xfrm>',
  '<wps:spPr>é<a:xfrm/>𝒜</wps:spPr>', '<wps:spPr> <a:xfrm/></wps:spPr>',
  // docPr tags
  '<wp:docPr id="4" name="Caixa 4"/>', '<wp:docPr name="a" id="12"/>', '<wp:docPr id="4" name="x"/><wp:docPr id="9" name="y"/>', '<wp:docPrX id="4"/><wp:docPr id="5"/>',
  '<wp:docPr_ id="4"/>', '<wp:docPr-x id="4"/>', '<wp:docPr xid="4" id="6"/>', '<wp:docPr x:id="4"/>', '<wp:docPr x_id="4" id="8"/>', '<wp:docPr id="a" id="3"/>',
  '<wp:docPr id="" id="2"/>', '<wp:docPr id="1x" id="2"/>', '<wp:docPr id="٣"/>', '<wp:docPr id=\'9\'/>', '<wp:docPr/><wp:docPr id="3"/>', '<wp:docPr descr="id=&quot;5&quot;" id="6"/>',
  '<wp:docPr descr=">" id="6"/>', '<wp:docPr name=">" id="6"/>', '<wp:docPr id="1" id="2"/>', '<wp:docPr\nid="7"\nname="n"/>', '<wp:docPr id="99999999999999999999"/>', '<wp:docPr id="2147483646"/>',
  '<wp:docPr id="0012"/>', '<wp:docPr id="1"', '<wp:docPr id="1', '<wp:docPr name="a', '<wp:docPr name="a"', '<wp:docPr name="" id="3"/>', '<wp:docPr name="a" name="b" id="3"/>',
  '<wp:docPr xname="a" name="b" id="3"/>', '<wp:docPr uname="a"/>', '<wp:docPr x:name="a"/>', '<wp:docPr name="é𝒜" id="1"/>', '<wp:docPr id="1" name="a>b"/>',
  '<wp:anchor relativeHeight="251659264"><wp:docPr id="1" name="a"/>', '<wp:anchor relativeHeight="1" relativeHeight="2">', '<wp:anchor xrelativeHeight="1" relativeHeight="2">',
  '<wp:anchor relativeHeight="">', '<wp:anchor relativeHeight="a" relativeHeight="3">', '<wp:anchor relativeHeight="12', '<wp:anchor relativeHeight=\'1\'>', 'relativeHeight="7"', '_relativeHeight="7"',
  '-relativeHeight="7"', ':relativeHeight="7"', 'wp14:relativeHeight="7"', '1relativeHeight="7"', 'éelativeHeight="7"', 'relativeHeight="٣"', 'relativeHeight="007"',
  // the theme relationships
  rel(T, 'theme/theme1.xml'), rel(T, '/word/theme/theme1.xml'), rel(T, 'word/theme/theme1.xml'), rel(T, './theme/theme1.xml'), rel(T, '/theme/theme1.xml'), rel(T, '../theme/theme1.xml'),
  rel(T, '/word/./theme/theme1.xml'), rel(T, './word/theme/t.xml'), rel(T, 'wordtheme.xml'), rel(T, 'Word/theme.xml'), rel(T, '//word/x.xml'), rel(T, ''),
  rel('http://x/styles', 'styles.xml') + rel(T, 'theme/a.xml'), rel(T, 'theme/a.xml') + rel(T, 'theme/b.xml'),
  '<Relationship Type="x/theme" Target="t.xml"/>', '<Relationship Target="t.xml" Type="x/theme"/>', '<Relationship Type="x/themeX" Target="t.xml"/>', '<Relationship Type="/theme"/>',
  '<Relationship xType="x/theme" Target="t.xml"/>', '<Relationship Type="a/b" Type="x/theme" Target="t.xml"/>', '<Relationship Type="x/theme" Type="a/b" Target="t.xml"/>',
  '<RelationshipX Type="x/theme" Target="t.xml"/>', '<Relationship_ Type="x/theme" Target="t.xml"/>', '<Relationship Type="x/theme"', '<Relationship Type="x/theme" Target="t.xml"',
  '<Relationship Type="a>b/theme" Target="t.xml"/>', '<Relationship Type="x/theme" Target="a>b.xml"/>', '<Relationship Type="x/theme" Target=""/>', '<Relationship Type="x/theme" Target="" Target="u.xml"/>',
  '<Relationship Type="x/theme" xTarget="v.xml"/>', '<Relationship Type="x/theme" Target="a"b.xml"/>', '<Relationship\nType="x/theme"\nTarget="n.xml"/>', '<Relationship Type="x/theme" Target="t.xml"/><Relationship Type="y/theme" Target="u.xml"/>',
  '<Relationship Id="1" Target="s.xml"/><Relationship Type="x/theme" Target="t.xml"/>', '<Relationships><Relationship Type="x/theme"/><Relationship Type="y/theme" Target="u.xml"/></Relationships>',
  // objectDefaults / txDef
  od('<a:spPr><a:solidFill/></a:spPr><a:bodyPr lIns="1" tIns="2" rIns="3" bIns="4" vert="vert" wrap="none" anchor="ctr"><a:spAutoFit/></a:bodyPr><a:lstStyle/>'),
  od('<a:spPr/><a:bodyPr rtlCol="0"/>'), od('<a:bodyPr/>'), od('<a:spPr></a:spPr><a:bodyPr wrap="square"/>'), od(''), '<a:objectDefaults></a:objectDefaults>', '<a:objectDefaults/>',
  od('<a:spPr/><a:spPr>X</a:spPr><a:bodyPr/>'), od('<a:spPr>X<a:bodyPr/>'), od('<a:spPrX>Y</a:spPrX><a:spPr>Z</a:spPr>'), od('<a:bodyPrX a="1"/><a:bodyPr b="2"/>'), od('<a:bodyPr b="2"'),
  od('<a:bodyPr lIns="5"/>'), od('<a:bodyPr\nlIns="5"\ttIns="6"/>'), od('<a:bodyPr xlIns="5"/>'), od('<a:bodyPr lIns=""/>'), od('<a:bodyPr lIns="a" lIns="b"/>'), od('<a:bodyPr xlIns="5" lIns="6"/>'),
  od('<a:bodyPr lIns="5"/>'), od('<a:bodyPr vert="x"/>'), od('<a:bodyPr\u0085vert="x"/>'), od('<a:bodyPr wrap="none/>'), od('<a:bodyPr wrap="none"/ >'), od('<a:bodyPr/ >'),
  od('<a:bodyPr anchor="b" / >'), od('<a:bodyPr a="x/"/>'), od('<a:bodyPr a="/">'), od('<a:bodyPr/><a:spAutoFit/>'), od('<a:bodyPr><a:spAutoFit /></a:bodyPr>'), od('<a:bodyPr vert="eaVert"/>'),
  '<a:objectDefaults><a:other/></a:objectDefaults><a:txDef>Q</a:txDef>', '<a:txDef>Q</a:txDef><a:objectDefaults></a:objectDefaults>', '<a:objectDefaults><a:txDef>A</a:txDef><a:txDef>B</a:txDef></a:objectDefaults>',
  '<a:objectDefaults><a:txDef>A', '<a:objectDefaults><a:txDef>A</a:txDef', '<a:objectDefaults><a:txDef></a:txDef></a:objectDefaults>', '<a:objectDefaults></a:txDef><a:txDef>K</a:txDef></a:objectDefaults>',
  '</a:txDef><a:txDef>K</a:txDef>', '<a:txDef/>', '<a:txDef>x</a:txDef>', '<a:txDef>x</a:txDef><a:txDef>y</a:txDef>', '</a:txDef>', '<a:txDef>', '<a:objectDefaults><a:objectDefaults><a:txDef>I</a:txDef>',
  // the svg ext and the glow presets
  `<a:blip>${SVG}<asvg:svgBlip r:embed="rId9"/></a:ext></a:blip>`, `${SVG}a</a:ext>x${SVG}b</a:ext>`, `${SVG}unterminated`, `x${SVG}</a:ext>`, `${SVG}${SVG}</a:ext></a:ext>`,
  '<a:ext uri="{96dac541-7b7a-43d3-8b79-37d633b846f1}">x</a:ext>', '<a:ext uri="{96DAC541-7B7A-43D3-8B79-37D633B846F1}" >x</a:ext>', '<a:ext  uri="{96DAC541-7B7A-43D3-8B79-37D633B846F1}">x</a:ext>',
  `${SVG}</a:ext>${SVG}</a:ext>`, `<a:extLst>${SVG}<x/></a:ext><a:ext uri="{FFF}">k</a:ext></a:extLst>`,
  'glow-0-1', 'glow-3-6', 'glow-12-3', 'glow--1-1', 'glow-1-', 'glow--', 'glow-1-12', 'glow-1-1\n', 'glow-1-1 ', ' glow-1-1', 'Glow-1-1', 'glow-٣-1', 'glow-1-٣', 'glow-1-1-1', 'glow-00-0',
  'glow-99999999999999999999-9', 'glow-1.5-1', 'glow-1e1-1', 'glow-', 'glow', 'glow-1', 'glow-+1-1',
];
