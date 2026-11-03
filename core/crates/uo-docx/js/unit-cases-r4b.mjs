// The hand-written unit cases of frontier R4b (tests/inline_objects_unit.rs): object elements with
// the frozen TS output of `readParagraph` over `<w:p><w:r>{element}</w:r></w:p>` as
// `JSON.stringify` (insertion order, `undefined` omitted; images as bytes). Regenerates
// tests/inline_objects_unit.cases:
//
//   UO_TS_ROOT=<built checkout of main> node core/crates/uo-docx/js/unit-cases-r4b.mjs
//
// Line: `kind<TAB>name<TAB>xml<TAB>result`. `obj`: the object runs; `fmt`/`fmtany`: readPictureFormat
// of the first w:drawing; `sync`: the first run's object after the sync with the raw text edited
// by replacing the `from` text of the line's name suffix (see CASES); `fx`: blipFxImageEffects.
// The stub context: a relationship id of 4 characters (rId1…rId4) is an image, the part ids
// rIdDm/rIdCs/rIdDr are the SmartArt parts of the case; the block reader is the stub of the
// capture (one paragraph per w:p). Two key positions the model fixes are normalised as in the
// capture: `props` of a group item's run after `text`, and `paras` first and `blocks` last in a
// text box.
import { writeFileSync } from 'node:fs';
import { join, dirname } from 'node:path';
import { fileURLToPath } from 'node:url';
import { importFrom, tsRoot } from '../../uo-testkit/js/testkit.mjs';

const here = dirname(fileURLToPath(import.meta.url));
const root = tsRoot();
const core = 'write/packages/docx-core/dist/';
const { parseXml, childElements, textContent } = await importFrom(root, `${core}xml.js`);
const Doc = await importFrom(root, `${core}document.js`);
const { readPictureFormat } = await importFrom(root, `${core}picture-xml.js`);
const enc = new TextEncoder();

const D = (inner, attrs = '') => `<w:drawing><wp:inline${attrs}><wp:extent cx="1905000" cy="952500"/>${inner}</wp:inline></w:drawing>`;
const A = (inner, pos = '') => `<w:drawing><wp:anchor behindDoc="1" distL="635"><wp:positionH relativeFrom="page"><wp:posOffset>63500</wp:posOffset></wp:positionH><wp:positionV relativeFrom="paragraph"><wp:align>top</wp:align></wp:positionV><wp:extent cx="1905000" cy="952500"/><wp:wrapSquare wrapText="bothSides"/><wp:docPr id="3" name="Obj" descr="alt"/>${pos}${inner}</wp:anchor></w:drawing>`;
const G = (uri, inner) => `<a:graphic><a:graphicData uri="${uri}">${inner}</a:graphicData></a:graphic>`;
const PIC = (blip, extra = '', spPr = '') => `<pic:pic><pic:blipFill>${blip}${extra}</pic:blipFill><pic:spPr>${spPr}</pic:spPr></pic:pic>`;
const WPS = 'http://schemas.microsoft.com/office/word/2010/wordprocessingShape';
const DML_PIC = 'http://schemas.openxmlformats.org/drawingml/2006/picture';
const TXBX = '<wps:txbx><w:txbxContent><w:p><w:r><w:t>Olá</w:t></w:r></w:p><w:p/></w:txbxContent></wps:txbx>';

const DM = '<dgm:dataModel><dgm:ptLst><dgm:pt modelId="m0"><dgm:prSet presStyleLbl="node1" presStyleIdx="1"/></dgm:pt></dgm:ptLst><dgm:extLst><a:ext><dsp:dataModelExt relId="rIdDr"/></a:ext></dgm:extLst></dgm:dataModel>';
const CS = '<dgm:colorsDef><dgm:styleLbl name="node1"><dgm:txFillClrLst meth="repeat"><a:srgbClr val="ffffff"/><a:schemeClr val="accent2"><a:tint val="60000"/></a:schemeClr></dgm:txFillClrLst></dgm:styleLbl></dgm:colorsDef>';
const SP = '<dsp:sp modelId="m0"><dsp:spPr><a:xfrm><a:off x="635000" y="0"/><a:ext cx="1905000" cy="952500"/></a:xfrm><a:prstGeom prst="roundRect"/><a:solidFill><a:srgbClr val="4472C4"/></a:solidFill></dsp:spPr><dsp:txBody><a:bodyPr/><a:p><a:pPr algn="ctr"><a:spcBef><a:spcPts val="600"/></a:spcBef></a:pPr><a:r><a:rPr sz="2400" b="1"/><a:t>Etapa</a:t></a:r></a:p></dsp:txBody></dsp:sp>';
const parts = new Map([['rIdDm', DM], ['rIdCs', CS], ['rIdDr', `<dsp:drawing><dsp:spTree>${SP}</dsp:spTree></dsp:drawing>`]]);
const DIAGRAM = D(G('http://schemas.openxmlformats.org/drawingml/2006/diagram', '<dgm:relIds r:dm="rIdDm" r:cs="rIdCs"/>'));
const BAD_PARTS = new Map([['rIdDm', DM.replace('presStyleIdx="1"', 'presStyleIdx="-1"').replace('meth="repeat"', 'meth="cycle"')], ['rIdCs', CS.replace('meth="repeat"', 'meth="cycle"')], ['rIdDr', parts.get('rIdDr')]]);

/** [kind, name, xml, parts?] */
const CASES = [
  ['obj', 'inline-picture', D(G(DML_PIC, PIC('<a:blip r:embed="rId1"><a:lum bright="10000" contrast="20000"/><a:grayscl/><a:extLst><a:ext><asvg:svgBlip r:embed="rId2"/></a:ext></a:extLst></a:blip>', '<a:srcRect l="10000" b="5000"/>', '<a:xfrm rot="5400000" flipH="1"/><a:ln w="12700"><a:solidFill><a:srgbClr val="FF0000"/></a:solidFill></a:ln>')) + '<wp:effectExtent l="635" t="0" r="1270" b="0"/>')],
  ['obj', 'inline-effect-extent', D(`<wp:effectExtent l="635" t="1270" r="0" b="635"/>${G(DML_PIC, PIC('<a:blip r:embed="rId1"/>'))}`)],
  ['obj', 'missing-media', D(G(DML_PIC, PIC('<a:blip r:embed="rId10"/>')))],
  ['obj', 'no-embed', D(G(DML_PIC, PIC('<a:blip/>')))],
  ['obj', 'bilevel', D(G(DML_PIC, PIC('<a:blip r:embed="rId1"><a:biLevel thresh="25000"/></a:blip>')))],
  ['obj', 'anchored-picture', A(G(DML_PIC, PIC('<a:blip r:embed="rId3"/>', '', '<a:prstGeom prst="ellipse"/>')))],
  ['obj', 'text-box', D(G(WPS, `<wps:wsp><wps:spPr><a:xfrm rot="5400000"><a:off x="0" y="0"/></a:xfrm><a:prstGeom prst="roundRect"><a:avLst><a:gd name="adj" fmla="val 5000"/></a:avLst></a:prstGeom><a:solidFill><a:srgbClr val="FFFF00"/></a:solidFill><a:ln w="9525"><a:solidFill><a:srgbClr val="000000"/></a:solidFill></a:ln></wps:spPr><wps:style><a:fontRef idx="minor"><a:srgbClr val="FF00FF"/></a:fontRef></wps:style>${TXBX.replace('<wps:txbx>', '<wps:txbx id="7">')}<wps:bodyPr wrap="none" lIns="91440" tIns="0" anchor="ctr" vert="vert270" upright="1" fromWordArt="1"><a:prstTxWarp prst="textWave1"/><a:spAutoFit/></wps:bodyPr></wps:wsp>`))],
  ['obj', 'text-box-flip', A(G(WPS, `<wps:wsp><wps:spPr><a:xfrm rot="5400000" flipV="1"/><a:prstGeom prst="rect"/><a:noFill/></wps:spPr>${TXBX}</wps:wsp>`))],
  ['obj', 'empty-text-box', D(G(WPS, '<wps:wsp><wps:spPr/><wps:txbx><w:txbxContent/></wps:txbx></wps:wsp>'))],
  ['obj', 'linked-text-box', D(G(WPS, '<wps:wsp><wps:spPr/><wps:linkedTxbx id="7" seq="2"/></wps:wsp>'))],
  ['obj', 'text-box-picture-fill', D(G(WPS, `<wps:wsp><wps:spPr><a:blipFill><a:blip r:embed="rId1"><a:alphaModFix amt="50000"/></a:blip><a:tile tx="635" sx="50000" algn="ctr"/></a:blipFill></wps:spPr>${TXBX}</wps:wsp>`))],
  ['obj', 'shape', D(G(WPS, '<wps:wsp><wps:spPr><a:prstGeom prst="ellipse"/><a:solidFill><a:schemeClr val="accent1"/></a:solidFill></wps:spPr></wps:wsp>'))],
  ['obj', 'hidden-shape', '<w:drawing><wp:inline><wp:extent cx="635" cy="635"/><wp:docPr id="1" name="x" hidden="1"/>' + G(WPS, '<wps:wsp><wps:spPr><a:prstGeom prst="rect"/><a:solidFill><a:srgbClr val="00FF00"/></a:solidFill></wps:spPr></wps:wsp>') + '</wp:inline></w:drawing>'],
  ['obj', 'picture-filled-shape', D(G(WPS, '<wps:wsp><wps:spPr><a:blipFill><a:blip r:embed="rId4"/><a:srcRect t="2000"/></a:blipFill></wps:spPr></wps:wsp>'))],
  ['obj', 'group', D(G('http://schemas.microsoft.com/office/word/2010/wordprocessingGroup', '<wpg:wgp><wpg:grpSpPr><a:xfrm rot="2700000" flipH="1" flipV="1"><a:off x="0" y="0"/><a:ext cx="1905000" cy="952500"/><a:chOff x="0" y="0"/><a:chExt cx="3810000" cy="1905000"/></a:xfrm></wpg:grpSpPr>'
    + `<wps:wsp><wps:spPr><a:xfrm><a:off x="635000" y="0"/><a:ext cx="635000" cy="635000"/></a:xfrm><a:prstGeom prst="rect"/><a:solidFill><a:srgbClr val="FF0000"/></a:solidFill></wps:spPr></wps:wsp>`
    + `<wps:wsp><wps:spPr><a:xfrm><a:off x="0" y="635000"/><a:ext cx="1270000" cy="635000"/></a:xfrm></wps:spPr>${TXBX}</wps:wsp>`
    + '<pic:pic><pic:blipFill><a:blip r:embed="rId1"/></pic:blipFill><pic:spPr><a:xfrm><a:off x="1270000" y="0"/><a:ext cx="635000" cy="635000"/></a:xfrm></pic:spPr></pic:pic>'
    + '<wpg:grpSp><wpg:grpSpPr><a:xfrm><a:off x="0" y="0"/><a:ext cx="635000" cy="635000"/></a:xfrm></wpg:grpSpPr><wps:wsp><wps:spPr/></wps:wsp></wpg:grpSp><wps:other/></wpg:wgp>') + '<wp:effectExtent l="635" t="635" r="1270" b="635"/>')],
  ['obj', 'empty-group', D(G('x', '<wpg:wgp><wpg:grpSpPr/></wpg:wgp>'))],
  ['obj', 'canvas', A(G('x', '<wpc:wpc><wpg:grpSpPr/><wps:wsp><wps:spPr/></wps:wsp></wpc:wpc>'))],
  ['obj', 'smartart', DIAGRAM],
  ['obj', 'smartart-throws', DIAGRAM, BAD_PARTS],
  ['obj', 'smartart-no-parts', DIAGRAM, new Map()],
  ['obj', 'unknown-graphic', D(G('x', '<foo/>'))],
  ['obj', 'no-container', '<w:drawing><foo/></w:drawing>'],
  ['obj', 'vml-text-box', '<w:pict><v:shapetype id="t"/><v:shape id="_x0000_s1026" o:spid="_x0000_s1026" style="position:absolute;margin-left:10pt;margin-top:5pt;width:100pt;height:40pt;z-index:3;mso-position-horizontal-relative:page;v-text-anchor:middle" fillcolor="#ff0000" stroked="f"><v:textbox style="layout-flow:vertical;mso-layout-flow-alt:bottom-to-top;mso-fit-shape-to-text:t;mso-next-textbox:#_x0000_s1027" inset="0,1pt,,2pt"><w:txbxContent><w:p><w:r><w:t>caixa</w:t></w:r></w:p></w:txbxContent></v:textbox><w10:wrap type="square"/></v:shape></w:pict>'],
  ['obj', 'vml-text-box-in-group', '<w:pict><v:group style="position:absolute;width:200pt;height:100pt" coordsize="2000,1000" coordorigin="100,50"><v:rect style="left:200;top:150;width:1000;height:500" fillcolor="blue"><v:textbox><w:txbxContent><w:p/></w:txbxContent></v:textbox></v:rect><v:shape style="visibility:hidden;width:1;height:1"/><v:oval style="left:0;top:0;width:100;height:100" filled="t"/><v:shapetype id="x"/><o:lock/></v:group></w:pict>'],
  ['obj', 'vml-image', '<w:pict><v:shape style="position:absolute;width:50pt;height:20pt;margin-left:1in" type="#_x0000_t75"><v:imagedata r:id="rId1" cropleft="5000f" croptop=".1" gain="19661f" blacklevel="22938f" grayscale="t" bilevel="true"/></v:shape></w:pict>'],
  ['obj', 'vml-image-missing', '<w:pict><v:shape style="width:50pt;height:20pt"><v:imagedata r:id="rId10"/></v:shape></w:pict>'],
  ['obj', 'vml-word-art', '<w:pict><v:shape id="PowerPlusWaterMarkObject1" style="position:absolute;width:468pt;height:117pt;rotation:315;z-index:-1;mso-position-horizontal:center;mso-position-vertical:center" fillcolor="silver" stroked="f"><v:fill opacity=".5"/><v:textpath style="font-family:&quot;Calibri&quot;;font-size:1pt;font-weight:bold;font-style:italic" string="RASCUNHO"/></v:shape></w:pict>'],
  ['obj', 'vml-line', '<w:pict><v:line from="0,0" to="100pt,50pt" style="mso-position-horizontal:left" strokecolor="red"/></w:pict>'],
  ['obj', 'vml-hr', '<w:pict><v:rect style="width:0;height:1.5pt" o:hr="t" o:hrpct="990" o:hralign="center" o:hrstd="t" o:hrnoshade="t" fillcolor="#a0a0a0" stroked="f"/></w:pict>'],
  ['obj', 'vml-ole', '<w:object><v:shape style="width:100pt;height:50pt;position:absolute" type="#_x0000_t75"><v:imagedata r:id="rId1"/></v:shape><o:OLEObject Type="Embed" ProgID="Excel.Sheet.12" r:id="rId2"/></w:object>'],
  ['obj', 'vml-ole-link', '<w:object><v:shape style="width:100pt;height:50pt"><v:imagedata r:id="rId1"/></v:shape><o:OLEObject Type="Link" r:id="rId2"/></w:object>'],
  ['obj', 'alternate-content', '<mc:AlternateContent><mc:Choice Requires="wps">' + D(G(WPS, '<wps:wsp><wps:spPr><a:prstGeom prst="rect"/><a:solidFill><a:srgbClr val="00FF00"/></a:solidFill></wps:spPr></wps:wsp>')) + '</mc:Choice><mc:Fallback><w:pict><v:rect fillcolor="red" style="width:1pt;height:1pt"/></w:pict></mc:Fallback></mc:AlternateContent>'],
  ['obj', 'alternate-content-fallback', '<mc:AlternateContent><mc:Choice Requires="x">' + D(G('x', '<foo/>')) + '</mc:Choice><mc:Fallback><w:pict><v:rect fillcolor="red" style="width:1pt;height:1pt"/></w:pict></mc:Fallback></mc:AlternateContent>'],
  ['obj', 'vml-textbox-throws', '<w:pict><v:shape o:spid="_x0000_s2" style="position:absolute;width:10pt;height:10pt;mso-position-horizontal:&lt;/wp:align&gt;&lt;x/&gt;"><v:textbox><w:txbxContent><w:p/></w:txbxContent></v:textbox><v:stroke dashstyle="a&quot;b"/></v:shape></w:pict>'],
  ['fmt', 'format-anchored', A(G(DML_PIC, PIC('<a:blip r:embed="rId3"><a:alphaModFix amt="60000"/><a:duotone><a:srgbClr val="000000"/><a:prstClr val="white"><a:lumMod val="50000"/></a:prstClr></a:duotone><a:clrChange useA="1"><a:clrFrom><a:srgbClr val="FFFFFF"/></a:clrFrom></a:clrChange><a:extLst><a:ext><a14:imgProps><a14:imgLayer r:embed="rId4"><a14:imgEffect><a14:saturation sat="50000"/></a14:imgEffect><a14:imgEffect><a14:sharpenSoften amount="-20000"/><a14:brightnessContrast bright="10000" contrast="-10000"/><a14:artisticBlur/></a14:imgEffect></a14:imgLayer></a14:imgProps></a:ext></a:extLst></a:blip>', '<a:srcRect l="-1000"/>', '<a:xfrm rot="60000" flipV="1"/><a:prstGeom prst="roundRect"><a:avLst><a:gd name="adj" fmla="val 1234"/><a:gd name="x" fmla="*/ 1 2 3"/></a:avLst></a:prstGeom><a:ln w="19050" cap="rnd" cmpd="dbl" algn="in"><a:solidFill><a:schemeClr val="accent1"><a:lumMod val="50000"/></a:schemeClr></a:solidFill><a:prstDash val="dash"/><a:miter lim="800000"/><a:headEnd type="arrow"/></a:ln><a:effectLst><a:outerShdw blurRad="50800" dist="38100" dir="2700000" algn="tl" rotWithShape="0"><a:srgbClr val="000000"><a:alpha val="40000"/></a:srgbClr></a:outerShdw><a:glow rad="63500"><a:srgbClr val="FF0000"/></a:glow><a:softEdge rad="12700"/><a:reflection stA="50000" endPos="30000" dir="5400000"/><a:blur rad="1"/></a:effectLst><a:scene3d><a:camera prst="orthographicFront" fov="100"><a:rot lat="1" lon="2" rev="3"/></a:camera><a:lightRig rig="threePt" dir="t"/></a:scene3d><a:sp3d extrusionH="12700" prstMaterial="matte"><a:bevelT w="1" prst="circle"/><a:extrusionClr><a:srgbClr val="00FF00"/></a:extrusionClr></a:sp3d>')) + '<wp14:sizeRelH relativeFrom="page"><wp14:pctWidth>50000</wp14:pctWidth></wp14:sizeRelH>')],
  ['fmt', 'format-inline-polygon', '<w:drawing><wp:inline distT="1" locked="1"><wp:extent cx="635" cy="1270"/><wp:effectExtent l="1" t="2" r="3" b="4"/><wp:docPr id="9" name="N" descr="d" title="t" hidden="true"><a:extLst><a:ext><adec:decorative val="1"/></a:ext></a:extLst></wp:docPr><wp:cNvGraphicFramePr><a:graphicFrameLocks noChangeAspect="1"/></wp:cNvGraphicFramePr>' + G(DML_PIC, PIC('<a:blip r:embed="rId1"/>')) + '</wp:inline></w:drawing>'],
  ['fmt', 'format-tight', '<w:drawing><wp:anchor layoutInCell="0" allowOverlap="false" relativeHeight="5"><wp:positionH relativeFrom="column"><wp14:pctPosHOffset>-5000</wp14:pctPosHOffset></wp:positionH><wp:extent cx="1" cy="2"/><wp:wrapTight wrapText="left"><wp:wrapPolygon edited="1"><wp:start x="0" y="0"/><wp:lineTo x="21600" y="21600"/></wp:wrapPolygon></wp:wrapTight>' + G(DML_PIC, PIC('<a:blip r:embed="rId1"/>')) + '</wp:anchor></w:drawing>'],
  ['fmt', 'format-shape', D(G(WPS, '<wps:wsp><wps:spPr><a:prstGeom prst="rect"/></wps:spPr></wps:wsp>'))],
  ['fmtany', 'format-shape-any', D(G(WPS, '<wps:wsp><wps:spPr><a:custGeom><a:pathLst/></a:custGeom><a:ln><a:gradFill><a:gsLst/></a:gradFill><a:tailEnd/><a:extLst/></a:ln></wps:spPr></wps:wsp>'))],
  ['fmtany', 'format-group-any', D(G('x', '<wpg:wgp><wpg:grpSpPr><a:xfrm rot="5400000"/></wpg:grpSpPr></wpg:wgp>'))],
  ['fmt', 'format-bad', '<w:drawing><foo/></w:drawing>'],
];

const FX = [
  ['no-fx', {}], ['lum-pos', { lum: { bright: 10000, contrast: 20000 } }], ['lum-neg', { lum: { bright: -20000, contrast: -50000 } }], ['lum-big', { lum: { contrast: 250000 } }],
  ['lum-empty', { lum: {} }], ['gray', { grayscl: true }], ['bilevel', { biLevel: 25000 }], ['all', { lum: { bright: 5, contrast: 5 }, grayscl: true, biLevel: 0 }],
  ['nan', { lum: { bright: NaN, contrast: NaN } }],
];

const normTextbox = (tb) => { const { paras, blocks, ...rest } = tb; return blocks ? { paras, ...rest, blocks } : { paras, ...rest }; };
const reorder = (r) => {
  const { text, props, ...rest } = r;
  const out = { text, props, ...rest };
  if (out.group) out.group = normGroup(out.group);
  if (out.textbox) out.textbox = normTextbox(out.textbox);
  return out;
};
const normGroup = (g) => ({ ...g, items: g.items.map((it) => ({ ...it, run: reorder(it.run) })) });
const objsOf = (runs) => runs.map((r) => {
  const o = { text: r.text };
  for (const k of ['image', 'textbox', 'shape', 'group']) if (r[k] !== undefined) o[k] = k === 'group' ? normGroup(r[k]) : k === 'textbox' ? normTextbox(r[k]) : r[k];
  return o;
});

const readBlocks = (c) => childElements(c, 'w:p').map((p) => ({ type: 'paragraph', paragraph: { runs: [{ text: textContent(p), props: {} }], props: {} } }));
const ctxFor = (m) => ({
  resolveImage: (rid) => (rid.length === 4 ? { data: enc.encode(`img:${rid}`), ext: 'png' } : undefined),
  resolveLink: () => undefined,
  resolvePart: (rid) => { const t = m.get(rid); return t === undefined ? undefined : parseXml(t); },
  readBlocks,
});
const NS = 'xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"';
const show = (v) => JSON.stringify(v);

const lines = ['# unit cases of frontier R4b (js/unit-cases-r4b.mjs): `kind<TAB>name<TAB>xml<TAB>JSON.stringify of the TS result` (`!throw` when the TS throws; a Uint8Array as V8 prints it)'];
for (const [set, m] of [['good', parts], ['bad', BAD_PARTS]]) for (const [rid, t] of m) lines.push(['part', set, rid, t].join('	'));
for (const [kind, name, xml, partMap] of CASES) {
  const text = `<w:root ${NS}>${xml}</w:root>`;
  const rootEl = parseXml(text);
  const ctx = ctxFor(partMap ?? parts);
  const first = (n) => { const st = [rootEl]; while (st.length) { const el = st.shift(); if (el.name === n) return el; for (const c of childElements(el)) st.push(c); } };
  let out;
  try {
    if (kind === 'obj') {
      const el = childElements(rootEl)[0];
      const runs = Doc.readParagraph({ name: 'w:p', attrs: {}, children: [{ name: 'w:r', attrs: {}, children: [el] }] }, { ...ctx, activeComments: new Set(), fieldStack: [] }).runs;
      out = show(objsOf(runs));
    } else {
      const el = first('w:drawing');
      out = show(readPictureFormat(el.raw, kind === 'fmtany' ? { anyShape: true } : {}));
    }
  } catch { out = '!throw'; }
  lines.push([kind, name, xml, out === undefined ? 'undefined' : out].join('\t'));
}
for (const [name, b] of FX) lines.push(['fx', name, JSON.stringify(b, (k, x) => (typeof x === 'number' && Number.isNaN(x) ? 'NaN' : x)), show(Doc.blipFxImageEffects(b))].join('\t'));

// sync: the first object of the first run after an edit of its raw text
const SYNC = [
  ['sync-image-size', D(G(DML_PIC, PIC('<a:blip r:embed="rId1"/>', '', '<a:xfrm rot="60000"/>'))), ['<wp:extent cx="1905000"', '<wp:extent cx="2540000"']],
  ['sync-image-anchor', D(G(DML_PIC, PIC('<a:blip r:embed="rId1"/>'))), ['<wp:inline', '<wp:anchor behindDoc="1"'], ['</wp:inline>', '</wp:anchor>']],
  ['sync-image-no-xfrm', D(G(DML_PIC, PIC('<a:blip r:embed="rId1"/>', '', '<a:xfrm rot="60000" flipH="1"/>'))), ['<a:xfrm rot="60000" flipH="1"/>', '']],
  ['sync-text-box', A(G(WPS, `<wps:wsp><wps:spPr><a:xfrm rot="5400000"/><a:prstGeom prst="roundRect"/><a:solidFill><a:srgbClr val="FFFF00"/></a:solidFill></wps:spPr>${TXBX}<wps:bodyPr anchor="b"/></wps:wsp>`)), ['anchor="b"', 'anchor="ctr" wrap="none"'], ['<a:srgbClr val="FFFF00"/>', '<a:srgbClr val="0000FF"/>']],
  ['sync-text-box-no-fill', D(G(WPS, `<wps:wsp><wps:spPr><a:solidFill><a:srgbClr val="FFFF00"/></a:solidFill></wps:spPr>${TXBX}</wps:wsp>`)), ['<a:solidFill><a:srgbClr val="FFFF00"/></a:solidFill>', '']],
  ['sync-shape', D(G(WPS, '<wps:wsp><wps:spPr><a:prstGeom prst="rect"/><a:solidFill><a:srgbClr val="00FF00"/></a:solidFill></wps:spPr></wps:wsp>')), ['prst="rect"', 'prst="ellipse"']],
  ['sync-group', D(G('http://schemas.microsoft.com/office/word/2010/wordprocessingGroup', '<wpg:wgp><wpg:grpSpPr><a:xfrm><a:off x="0" y="0"/><a:ext cx="1905000" cy="952500"/><a:chOff x="0" y="0"/><a:chExt cx="1905000" cy="952500"/></a:xfrm></wpg:grpSpPr><wps:wsp><wps:spPr><a:xfrm><a:off x="635000" y="0"/><a:ext cx="635000" cy="635000"/></a:xfrm><a:solidFill><a:srgbClr val="FF0000"/></a:solidFill></wps:spPr></wps:wsp></wpg:wgp>')), ['<wp:extent cx="1905000" cy="952500"/>', '<wp:extent cx="3810000" cy="1905000"/>'], ['<wp:inline', '<wp:anchor distL="635"']],
  ['sync-not-xml', D(G(DML_PIC, PIC('<a:blip r:embed="rId1"/>'))), ['<wp:inline', '<wp:inline <']],
];
for (const [name, xml, ...edits] of SYNC) {
  const text = `<w:root ${NS}>${xml}</w:root>`;
  const rootEl = parseXml(text);
  const el = childElements(rootEl)[0];
  const runs = Doc.readParagraph({ name: 'w:p', attrs: {}, children: [{ name: 'w:r', attrs: {}, children: [el] }] }, { ...ctxFor(parts), activeComments: new Set(), fieldStack: [] }).runs;
  const run = runs[0];
  let raw = el.raw;
  for (const [a, b] of edits) raw = raw.replace(a, b);
  const o = run.image ?? run.textbox ?? run.shape ?? run.group;
  o.rawDrawingXml = raw;
  if (run.image) Doc.syncImageFromXml(run.image); else Doc.syncShapeFromXml(run);
  lines.push(['sync', name, `${xml}\u0001${edits.map(([a, b]) => `${a}\u0002${b}`).join('\u0003')}`, show(objsOf([run]))].join('\t'));
}
writeFileSync(join(here, '..', 'tests', 'inline_objects_unit.cases'), lines.join('\n') + '\n');
console.log(`${lines.length - 1} cases`);
