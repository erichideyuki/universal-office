// The hand-written unit cases of frontier R1 (tests/props_unit.rs): property elements with the
// frozen TS reader's `JSON.stringify` output (insertion order, `undefined` omitted), and
// `measureTwips` / `elementXml` results. Regenerates tests/props_unit.cases:
//
//   UO_TS_ROOT=<built checkout of main> node core/crates/uo-docx/js/unit-cases.mjs
import { writeFileSync } from 'node:fs';
import { join, dirname } from 'node:path';
import { fileURLToPath } from 'node:url';
import { importFrom, tsRoot } from '../../uo-testkit/js/testkit.mjs';

const here = dirname(fileURLToPath(import.meta.url));
const root = tsRoot();
const { parseXml } = await importFrom(root, 'write/packages/docx-core/dist/xml.js');
const props = await importFrom(root, 'write/packages/docx-core/dist/ooxml/props.js');
const fx = await importFrom(root, 'write/packages/docx-core/dist/text-effects.js');
const NS = 'xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main" xmlns:w14="http://schemas.microsoft.com/office/word/2010/wordml" xmlns:w16du="x"';
const R = { rPr: props.readRPr, pPr: props.readPPr, tblPr: props.readTblPr, trPr: props.readTrPr, tcPr: props.readTcPr };
const cases = [
  ['rPr', '<w:b/><w:i w:val="0"/><w:sz w:val="22"/><w:color w:val="FF0000" w:themeShade="80"/><w:rFonts w:ascii="Calibri"/>'],
  ['rPr', '<w:sz w:val="1e3"/><w:spacing w:val="12pt"/><w:kern w:val="0x10"/><w:position w:val="-0"/><w:w w:val="50%"/>'],
  ['rPr', '<w:spacing w:val="."/><w:u/><w:vertAlign w:val="superscript"/><w:lang w:val="pt-BR" w:eastAsia=""/>'],
  ['rPr', '<w:highlight w:val=""/><w:effect w:val="x"/><w:bdr w:sz="4" w:shadow="off"/><w:shd w:val="clear" w:fill="auto"/><w:em w:val="dot"/>'],
  ['rPr', '<w14:textFill><w14:solidFill><w14:srgbClr w14:val="ab12ef"><w14:alpha w14:val="25000"/></w14:srgbClr></w14:solidFill></w14:textFill><w14:ligatures w14:val="all"/><w14:shadow w14:blurRad="25400" w14:dist="12700" w14:dir="5400000"><w14:srgbClr w14:val="000000"/></w14:shadow>'],
  ['rPr', '<w14:textOutline w14:w="9525"><w14:solidFill><w14:schemeClr w14:val="accent1"><w14:lumMod w14:val="75000"/></w14:schemeClr></w14:solidFill><w14:round/></w14:textOutline><w14:glow w14:rad="63500"><w14:srgbClr w14:val="ff0000"/></w14:glow><w14:numForm w14:val="lining"/><w14:stylisticSets><w14:styleSet w14:id="1"/><w14:styleSet w14:id="30"/></w14:stylisticSets><w14:cntxtAlts/>'],
  ['rPr', '<w:rPrChange w:author="A" w:date="d" w16du:dateUtc="u"><w:rPr><w:b/><w:rPrChange w:author="B"/></w:rPr></w:rPrChange><w:i/>'],
  ['rPr', '<w14:scene3d a="1" b="&amp;&quot;">t&lt;<w14:e g="h"/></w14:scene3d><w14:props3d/>'],
  ['pPr', '<w:jc w:val="center"/><w:spacing w:after="120" w:before="12pt" w:line="1.5in" w:lineRule="auto" w:afterAutospacing="1"/><w:ind w:start="720" w:hanging="360" w:endChars="3"/>'],
  ['pPr', '<w:tabs><w:tab w:val="start" w:pos="100"/><w:tab w:val="bogus" w:pos="1"/><w:tab w:pos="x"/><w:tab w:val="clear" w:pos="1.5cm" w:leader="dot"/></w:tabs><w:numPr><w:ilvl w:val="1"/><w:numId w:val="0"/></w:numPr>'],
  ['pPr', '<w:framePr w:y="1" w:x="2" w:hRule="exact" w:wrap="around" w:dropCap="drop" w:lines="3"/><w:pBdr><w:start w:val="s"/><w:end w:val="e"/><w:bar/></w:pBdr><w:keepNext w:val="off"/>'],
  ['pPr', '<w:rPr><w:ins w:author="i"/><w:del w:author="d"/><w:b/></w:rPr><w:pPrChange w:author="c"><w:pPr><w:jc w:val="left"/></w:pPr></w:pPrChange>'],
  ['tblPr', '<w:tblStyle w:val="Grid"/><w:tblW w:w="50%" w:type="pct"/><w:tblInd w:w="1in"/><w:tblLayout w:type="fixed"/><w:tblLook w:val="04A0" w:noVBand="1"/><w:tblBorders><w:top w:val="single"/><w:start w:val="x"/></w:tblBorders><w:tblCellMar><w:top w:w="10"/><w:end w:w="5"/></w:tblCellMar>'],
  ['tblPr', '<w:tblpPr w:tblpY="1" w:vertAnchor="page" w:tblpX="2" w:tblpXSpec="left" w:leftFromText="3"/><w:tblCaption w:val=""/><w:tblDescription/>'],
  ['trPr', '<w:trHeight w:val="400" w:hRule="exact"/><w:cantSplit/><w:wBefore w:w="5" w:type="x"/><w:gridAfter w:val="2"/><w:tblHeader w:val="false"/>'],
  ['tcPr', '<w:tcW w:w="2400" w:type="dxa"/><w:gridSpan w:val="2"/><w:vMerge/><w:hMerge w:val="restart"/><w:vAlign w:val="center"/><w:tcBorders><w:tl2br w:val="a"/><w:right w:val="r"/><w:end w:val="e"/></w:tcBorders><w:tcMar><w:left w:w="1pt"/></w:tcMar>'],
];
const measures = ['', ' ', '12', '12pt', ' 12pt ', '1.5in', '2.54cm', '25.4mm', '1pc', '3pi', '.', '..5', '-.', '1.2.3', '1e3', '0x10', '-0', '-0pt', 'abc', '12 pt', '1,5', '99999999999999999999pt'];

const lines = ['# unit cases of frontier R1 (js/unit-cases.mjs): `prop<TAB>kind<TAB>inner xml<TAB>JSON.stringify of the TS reader`, `measure<TAB>text<TAB>result`, `xml<TAB>inner xml of a w14:scene3d<TAB>elementXml`'];
for (const [k, inner] of cases) {
  const doc = parseXml(`<w:document ${NS}><w:${k}>${inner}</w:${k}></w:document>`);
  lines.push(['prop', k, inner, JSON.stringify(R[k](doc.children[0]))].join('\t'));
}
for (const v of measures) {
  const r = props.measureTwips(v);
  lines.push(['measure', v, r === undefined ? 'undefined' : Object.is(r, -0) ? '-0' : String(r)].join('\t'));
}
for (const inner of ['', 'x', 't&lt;&amp;&gt;&quot;', '<w14:e g="h"/><w14:f></w14:f>', ' <w14:a 1="x" b="y"><w14:c/>tail</w14:a>']) {
  const doc = parseXml(`<w:document ${NS}><w14:scene3d a="1" 1="x" b="&amp;&quot;">${inner}</w14:scene3d></w:document>`);
  lines.push(['xml', inner, fx.elementXml(doc.children[0])].join('\t'));
}
writeFileSync(join(here, '..', 'tests', 'props_unit.cases'), lines.join('\n') + '\n');
console.log(`${lines.length - 1} cases`);
