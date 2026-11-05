// Packages that stress the whole-document read (frontier R5 of the D1a reader, facet `full`): what
// `readFullDocument` does around the paragraphs, which the oracle corpus has few of: sections and their
// header/footer references (shared parts, missing, empty, odd relationships), the sectPr properties,
// bookmarks (first one wins, ends without starts, offsets), comments, notes (ids, separators, notes in
// tables), the core properties, the page background, chains of VML text boxes, block content controls with
// a gallery, tables (cells, rows and wrappers, odd widths), the body with no section, an empty body, nesting
// of tables and text boxes around the limit of 64 (G20), and seeded documents that mix the R6 generator's
// blocks with random sections and parts. `capture-full.mjs` runs the frozen TS over them and writes them
// next to the capture (tests/parity/full.inputs, the same format as sources.inputs); the Rust replay reads
// that file, so both sides see the same text.
import { Rng } from '../../uo-testkit/js/testkit.mjs';
import { Gen } from './quirk-sources.mjs';

export const QUIRK_FULL_SEED = 0xf0110c05;
export const QUIRK_FULL_SEEDED = 200;

const NS = [
  'xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"',
  'xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships"',
  'xmlns:wp="http://schemas.openxmlformats.org/drawingml/2006/wordprocessingDrawing"',
  'xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main"',
  'xmlns:wps="http://schemas.microsoft.com/office/word/2010/wordprocessingShape"',
  'xmlns:mc="http://schemas.openxmlformats.org/markup-compatibility/2006"',
  'xmlns:v="urn:schemas-microsoft-com:vml"',
  'xmlns:o="urn:schemas-microsoft-com:office:office"',
  'xmlns:w14="http://schemas.microsoft.com/office/word/2010/wordml"',
  'xmlns:m="http://schemas.openxmlformats.org/officeDocument/2006/math"',
  'xmlns:dcterms="http://purl.org/dc/terms/"',
  'xmlns:cp="http://schemas.openxmlformats.org/package/2006/metadata/core-properties"',
].join(' ');
const PROLOG = '<?xml version="1.0" encoding="UTF-8" standalone="yes"?>\r\n';
const REL = 'http://schemas.openxmlformats.org/officeDocument/2006/relationships';
const RELS_NS = 'http://schemas.openxmlformats.org/package/2006/relationships';

const rels = (items) => `${PROLOG}<Relationships xmlns="${RELS_NS}">${items.map(([id, type, target, ext]) => `<Relationship Id="${id}" Type="${REL}/${type}" Target="${target}"${ext ? ' TargetMode="External"' : ''}/>`).join('')}</Relationships>`;
const HF_RELS = rels([['rIdH1', 'header', 'header1.xml'], ['rIdH2', 'header', 'header2.xml'], ['rIdH3', 'header', 'header3.xml'], ['rIdF1', 'footer', 'footer1.xml'], ['rIdF2', 'footer', 'footer2.xml'], ['rIdLink', 'hyperlink', 'https://x.test/a', true], ['rIdImg', 'image', 'media/image1.png']]);
const p = (t = 'x') => `<w:p><w:r><w:t>${t}</w:t></w:r></w:p>`;
const sect = (inner = '') => `<w:sectPr>${inner}<w:pgSz w:w="11906" w:h="16838"/></w:sectPr>`;
const sectPara = (inner = '', t = 's') => `<w:p><w:pPr>${sect(inner)}</w:pPr><w:r><w:t>${t}</w:t></w:r></w:p>`;
const docXml = (body, { ns = NS, pre = '' } = {}) => `${PROLOG}<w:document ${ns}>${pre}<w:body>${body}</w:body></w:document>`;
const story = (root, inner) => `${PROLOG}<${root} ${NS}>${inner}</${root}>`;
const notes = (kind, inner) => `${PROLOG}<w:${kind}s ${NS}>${inner}</w:${kind}s>`;
const tbl = (rows) => `<w:tbl><w:tblPr/><w:tblGrid><w:gridCol w:w="100"/></w:tblGrid>${rows}</w:tbl>`;
const txbx = (inner, n = 1) => `<w:r><w:drawing><wp:inline><wp:extent cx="1905000" cy="952500"/><wp:docPr id="${n}" name="Box"/><a:graphic><a:graphicData uri="http://schemas.microsoft.com/office/word/2010/wordprocessingShape"><wps:wsp><wps:spPr><a:xfrm><a:off x="0" y="0"/><a:ext cx="1905000" cy="952500"/></a:xfrm><a:prstGeom prst="rect"><a:avLst/></a:prstGeom></wps:spPr><wps:txbx><w:txbxContent>${inner}</w:txbxContent></wps:txbx><wps:bodyPr/></wps:wsp></a:graphicData></a:graphic></wp:inline></w:drawing></w:r>`;
const vbox = (id, inner, next = '') => `<w:r><w:pict><v:shape id="${id}" type="#_x0000_t202" style="position:absolute;width:100pt;height:50pt"><v:textbox${next ? ` style="mso-next-textbox:#${next}"` : ''}><w:txbxContent>${inner}</w:txbxContent></v:textbox></v:shape></w:pict></w:r>`;

/** A case: `{ key, parts: [[path, text]] }`. `document.xml` and the rels are filled in when not given. */
function pkg(name, body, extra = [], { docRels = HF_RELS, doc } = {}) {
  const parts = doc === null ? [] : [['word/document.xml', doc ?? docXml(body)]];
  if (docRels !== null) parts.push(['word/_rels/document.xml.rels', docRels]);
  parts.push(...extra);
  return { key: `quirk/full/${name}.docx`, parts };
}

const HDR = (inner = p('h')) => ['word/header1.xml', story('w:hdr', inner)];

function handWritten() {
  const out = [];
  const add = (...a) => out.push(pkg(...a));

  // sections and their header/footer references
  add('sect-basic', `${p('a')}${sectPara('<w:headerReference w:type="default" r:id="rIdH1"/><w:headerReference w:type="first" r:id="rIdH2"/><w:headerReference w:type="even" r:id="rIdH3"/><w:footerReference w:type="default" r:id="rIdF1"/><w:titlePg/>')}${p('b')}${sect('<w:headerReference w:type="default" r:id="rIdH1"/><w:footerReference w:type="default" r:id="rIdF2"/>')}`, [
    HDR(), ['word/header2.xml', story('w:hdr', p('h2'))], ['word/header3.xml', story('w:hdr', p('h3'))], ['word/footer1.xml', story('w:ftr', p('f1'))], ['word/footer2.xml', story('w:ftr', p('f2'))]]);
  add('sect-shared-hf', `${sectPara('<w:headerReference w:type="default" r:id="rIdH1"/>')}${sectPara('<w:headerReference w:type="default" r:id="rIdH1"/><w:headerReference w:type="first" r:id="rIdH1"/>')}${sect('<w:headerReference w:type="default" r:id="rIdH1"/>')}`, [HDR(p('shared'))]);
  add('sect-bad-refs', `${sectPara('<w:headerReference w:type="default"/><w:headerReference w:type="first" r:id=""/><w:headerReference w:type="even" r:id="rIdNone"/><w:footerReference w:type="default" r:id="rIdLink"/><w:headerReference r:id="rIdH2"/>')}${p('x')}`, [['word/header2.xml', story('w:hdr', p('h2'))]]);
  add('sect-dup-type', sect('<w:headerReference w:type="default" r:id="rIdH1"/><w:headerReference w:type="default" r:id="rIdH2"/><w:footerReference w:type="first" r:id="rIdF1"/><w:footerReference w:type="first" r:id="rIdF1"/>'), [HDR(p('one')), ['word/header2.xml', story('w:hdr', p('two'))], ['word/footer1.xml', story('w:ftr', p('f'))]]);
  add('sect-odd-type', `${sectPara('<w:type w:val="foo"/>')}${sectPara('<w:type/>')}${sectPara('<w:type w:val=""/>')}${sectPara('<w:type w:val="continuous"/>')}${sect('<w:type w:val="evenPage"/>')}`);
  add('sect-slot-names', sect('<w:headerReference w:type="1" r:id="rIdH1"/><w:headerReference w:type="a" r:id="rIdH2"/><w:headerReference w:type="0" r:id="rIdH3"/><w:headerReference w:type="__proto__" r:id="rIdH1"/><w:headerReference w:type="constructor" r:id="rIdH2"/>'), [HDR(), ['word/header2.xml', story('w:hdr', p('2'))], ['word/header3.xml', story('w:hdr', p('3'))]]);
  add('hf-empty-parts', `${sectPara('<w:headerReference w:type="default" r:id="rIdH1"/><w:headerReference w:type="first" r:id="rIdH2"/><w:footerReference w:type="default" r:id="rIdF1"/><w:footerReference w:type="first" r:id="rIdF2"/>')}${p('x')}`, [['word/header1.xml', ''], ['word/header2.xml', '﻿'], ['word/footer1.xml', story('w:ftr', '')], ['word/footer2.xml', `${PROLOG}<w:ftr/>`]]);
  add('hf-content', sect('<w:headerReference w:type="default" r:id="rIdH1"/><w:footerReference w:type="default" r:id="rIdF1"/>'), [
    ['word/header1.xml', story('w:hdr', `${p('h')}${tbl('<w:tr><w:tc><w:p><w:r><w:t>c</w:t></w:r></w:p></w:tc></w:tr>')}<w:sdt><w:sdtPr><w:docPartObj><w:docPartGallery w:val="Page Numbers (Top of Page)"/></w:docPartObj></w:sdtPr><w:sdtContent>${p('pn')}</w:sdtContent></w:sdt><w:p><w:bookmarkStart w:id="1" w:name="hb"/><w:r><w:t>b</w:t></w:r><w:bookmarkEnd w:id="1"/>${txbx(p('in header'))}</w:p>`)],
    ['word/_rels/header1.xml.rels', rels([['rIdImg', 'image', 'media/image1.png'], ['rIdLink', 'hyperlink', 'https://h.test/', true]])], ['word/media/image1.png', 'IMG'],
    ['word/footer1.xml', story('w:ftr', `<w:p><w:r><w:fldChar w:fldCharType="begin"/></w:r><w:r><w:instrText> PAGE </w:instrText></w:r><w:r><w:fldChar w:fldCharType="separate"/></w:r><w:r><w:t>1</w:t></w:r><w:r><w:fldChar w:fldCharType="end"/></w:r></w:p>`)]]);
  add('sect-body-positions', `${p('a')}${sect('<w:headerReference w:type="default" r:id="rIdH1"/>')}${p('b')}${sect()}`, [HDR()]);
  add('sect-first-child', `${sect()}${p('a')}`);
  add('sect-in-sdt', `<w:sdt><w:sdtPr><w:id w:val="1"/></w:sdtPr><w:sdtContent>${p('a')}${sectPara('<w:headerReference w:type="default" r:id="rIdH1"/>')}${tbl('<w:tr><w:tc>' + p('t') + '</w:tc></w:tr>')}${sectPara()}</w:sdtContent></w:sdt>${p('after')}${sect()}`, [HDR()]);
  add('sect-in-customxml', `<w:customXml w:element="e"><w:customXmlPr/>${sectPara()}${p('x')}</w:customXml>${sect()}`);
  add('sect-in-cell', `${tbl(`<w:tr><w:tc>${sectPara()}</w:tc></w:tr>`)}${p('x')}`);
  add('sect-none', `${p('a')}${p('b')}`);
  add('sect-empty-body', '');
  add('sect-only', sect('<w:headerReference w:type="default" r:id="rIdH1"/>'), [HDR()]);
  add('sect-no-body', '', [], { doc: `${PROLOG}<w:document ${NS}><w:p/></w:document>` });
  add('sect-no-doc-part', '', [], { doc: null });
  add('sect-extra-children', `<w:bookmarkStart w:id="1" w:name="b"/><w:proofErr w:type="x"/>${p('a')}<w:bookmarkEnd w:id="1"/><w:permStart w:id="2"/>${sect()}`);
  const props = [
    '<w:pgSz w:w="1" w:h="2" w:orient="landscape"/>', '<w:pgSz w:orient="portrait"/>', '<w:pgSz w:orient="x"/>', '<w:pgSz/>',
    '<w:pgMar w:gutter="360"/>', '<w:pgMar w:gutter="x"/>', '<w:pgMar w:gutter=""/>', '<w:pgMar w:gutter="12pt"/>', '<w:pgMar w:gutter="-0"/>', '<w:pgMar w:gutter="1e3"/>',
    '<w:cols w:num="2" w:space="720" w:sep="1"/>', '<w:cols w:equalWidth="0"><w:col w:w="3000" w:space="200"/><w:col w:w="x"/><w:col/></w:cols>', '<w:cols w:equalWidth="false"/>', '<w:cols w:equalWidth="1"/>', '<w:cols w:equalWidth=""/>', '<w:cols w:equalWidth="true"><w:col w:w="1" w:space="2"/></w:cols>',
    '<w:vAlign w:val="center"/>', '<w:vAlign w:val=""/>', '<w:vAlign/>',
    '<w:docGrid w:type="lines" w:linePitch="360" w:charSpace="4096"/>', '<w:docGrid w:linePitch="x" w:charSpace=""/>', '<w:docGrid/>', '<w:docGrid w:linePitch="0"/>',
    '<w:pgBorders w:offsetFrom="page" w:display="allPages" w:zOrder="back"><w:top w:val="single" w:sz="4" w:space="24" w:color="auto"/><w:left w:val="double" w:sz="8" w:color="FF0000"/><w:bottom w:val="none"/><w:right/></w:pgBorders>', '<w:pgBorders/>', '<w:pgBorders w:offsetFrom=""><w:top w:val="single" w:sz="x"/></w:pgBorders>',
    '<w:pgNumType w:fmt="upperRoman" w:start="3"/>', '<w:pgNumType w:fmt=""/>', '<w:pgNumType/>',
    '<w:lnNumType w:countBy="2" w:start="1" w:restart="newPage" w:distance="360"/>', '<w:lnNumType w:countBy="x" w:start="" w:distance="1e2"/>', '<w:lnNumType/>',
    '<w:bidi/>', '<w:bidi w:val="0"/>', '<w:rtlGutter/>', '<w:formProt/>', '<w:noEndnote/>', '<w:bidi/><w:rtlGutter/><w:formProt/><w:noEndnote/>',
    '<w:footnotePr><w:pos w:val="beneathText"/><w:numFmt w:val="lowerRoman"/></w:footnotePr><w:endnotePr><w:numStart w:val="2"/></w:endnotePr>', '<w:titlePg/><w:type w:val="nextColumn"/>',
  ];
  props.forEach((x, i) => add(`sect-props-${i}`, `${sectPara(x)}${p('end')}`));
  add('sect-props-all', sect(props.slice(0, 40).join('')));
  add('sect-props-two-sect', `${sectPara('<w:cols w:num="2"/>')}${sectPara('<w:cols w:num="3" w:sep="1"/>')}${sect('<w:cols w:num="4"/>')}`);

  // bookmarks
  add('bm-dups', `<w:p><w:bookmarkStart w:id="1" w:name="a"/><w:r><w:t>abc</w:t></w:r><w:bookmarkEnd w:id="1"/></w:p><w:p><w:r><w:t>xy</w:t></w:r><w:bookmarkStart w:id="2" w:name="a"/><w:r><w:t>z</w:t></w:r><w:bookmarkEnd w:id="2"/></w:p><w:p><w:bookmarkEnd w:id="7"/><w:bookmarkStart w:id="3" w:name=""/><w:bookmarkEnd w:id="3"/><w:bookmarkStart w:id="4" w:name="b"/><w:bookmarkStart w:id="4" w:name="c"/><w:bookmarkEnd w:id="4"/><w:bookmarkEnd w:id="4"/></w:p>`);
  add('bm-blocks', `${p('a')}${tbl(`<w:tr><w:tc><w:p><w:bookmarkStart w:id="1" w:name="incell"/><w:r><w:t>q</w:t></w:r><w:bookmarkEnd w:id="1"/></w:p></w:tc></w:tr>`)}<w:p><w:bookmarkStart w:id="2" w:name="incell"/><w:bookmarkStart w:id="3" w:name="after"/><w:r><w:t>text</w:t></w:r></w:p><w:p>${txbx('<w:p><w:bookmarkStart w:id="4" w:name="intxbx"/><w:r><w:t>t</w:t></w:r><w:bookmarkEnd w:id="4"/></w:p>')}<w:bookmarkEnd w:id="2"/></w:p><w:sdt><w:sdtPr/><w:sdtContent><w:p><w:bookmarkStart w:id="5" w:name="s1"/></w:p><w:p><w:bookmarkStart w:id="6" w:name="s2"/><w:bookmarkEnd w:id="6"/></w:p></w:sdtContent></w:sdt><w:p><w:bookmarkEnd w:id="5"/></w:p>`);
  add('bm-fields', `<w:p><w:r><w:t>ab</w:t></w:r><w:hyperlink w:anchor="x"><w:bookmarkStart w:id="1" w:name="inlink"/><w:r><w:t>cd</w:t></w:r></w:hyperlink><w:r><w:fldChar w:fldCharType="begin"/></w:r><w:r><w:instrText> REF inlink </w:instrText></w:r><w:r><w:fldChar w:fldCharType="separate"/></w:r><w:bookmarkStart w:id="2" w:name="infield"/><w:r><w:t>res</w:t></w:r><w:r><w:fldChar w:fldCharType="end"/></w:r><w:bookmarkEnd w:id="1"/><w:r><w:t>😀é</w:t></w:r><w:bookmarkStart w:id="3" w:name="late"/></w:p>`);
  add('bm-in-hf-and-notes', p('b'), [HDR('<w:p><w:bookmarkStart w:id="1" w:name="hdrbm"/></w:p>'), ['word/footnotes.xml', notes('footnote', '<w:footnote w:id="2"><w:p><w:bookmarkStart w:id="1" w:name="fnbm"/></w:p></w:footnote>')]]);

  // comments
  const cm = (inner) => ['word/comments.xml', `${PROLOG}<w:comments ${NS}>${inner}</w:comments>`];
  add('comments-basic', p('c'), [cm('<w:comment w:id="0" w:author="Ana"><w:p><w:r><w:t>one</w:t></w:r></w:p><w:p><w:r><w:t>two</w:t></w:r><w:r><w:t>!</w:t></w:r></w:p></w:comment><w:comment w:id="1"><w:p/></w:comment><w:comment w:id="x" w:author=""><w:p><w:r><w:t>é😀</w:t></w:r></w:p></w:comment><w:comment><w:p><w:r><w:t>no id</w:t></w:r></w:p></w:comment><w:comment w:id="1e3" w:author="B"/>')]);
  add('comments-rich', p('c'), [cm(`<w:comment w:id="2" w:author="C"><w:p>${txbx('<w:p><w:r><w:t>in box</w:t></w:r></w:p>')}<w:r><w:t>after</w:t></w:r></w:p><w:p><w:r><w:fldChar w:fldCharType="begin"/></w:r><w:r><w:instrText> PAGE </w:instrText></w:r><w:r><w:fldChar w:fldCharType="separate"/></w:r><w:r><w:t>9</w:t></w:r><w:r><w:fldChar w:fldCharType="end"/></w:r></w:p>${tbl('<w:tr><w:tc><w:p><w:r><w:t>ignored table</w:t></w:r></w:p></w:tc></w:tr>')}</w:comment>`)]);
  add('comments-empty-part', p('c'), [['word/comments.xml', '']]);
  add('comments-bad-xml', p('c'), [['word/comments.xml', '<w:comments><w:comment']]);

  // notes
  const ref = (kind, id) => `<w:r><w:${kind}Reference w:id="${id}"/></w:r>`;
  const sep = (kind) => `<w:${kind} w:type="separator" w:id="-1"><w:p><w:r><w:separator/></w:r></w:p></w:${kind}><w:${kind} w:type="continuationSeparator" w:id="0"><w:p><w:r><w:continuationSeparator/></w:r></w:p></w:${kind}>`;
  const bodyRefs = `<w:p>${ref('footnote', 2)}${ref('footnote', 2)}${ref('footnote', 3)}${ref('endnote', 2)}${ref('footnote', 'x')}</w:p>${tbl(`<w:tr><w:tc><w:p>${ref('footnote', 4)}</w:p></w:tc></w:tr>`)}<w:p>${txbx(`<w:p>${ref('footnote', 9)}</w:p>`)}</w:p>`;
  add('notes-basic', bodyRefs, [
    ['word/footnotes.xml', notes('footnote', `${sep('footnote')}<w:footnote w:id="2"><w:p><w:r><w:t>one</w:t></w:r></w:p></w:footnote><w:footnote w:id="2"><w:p><w:r><w:t>dup</w:t></w:r></w:p></w:footnote><w:footnote w:id="3">${tbl('<w:tr><w:tc><w:p><w:r><w:t>cell</w:t></w:r></w:p></w:tc></w:tr>')}<w:p/></w:footnote><w:footnote w:id="x"><w:p/></w:footnote><w:footnote w:id="y"><w:p/></w:footnote><w:footnote><w:p><w:r><w:t>noid</w:t></w:r></w:p></w:footnote><w:footnote w:id="4" w:type="normal"><w:sdt><w:sdtPr/><w:sdtContent><w:p><w:r><w:t>sdt</w:t></w:r></w:p></w:sdtContent></w:sdt></w:footnote><w:footnote w:id="5"/>`)],
    ['word/endnotes.xml', notes('endnote', `${sep('endnote')}<w:endnote w:type="continuationNotice" w:id="1"><w:p/></w:endnote><w:endnote w:id="2"><w:p><w:r><w:t>e</w:t></w:r></w:p></w:endnote>`)]]);
  add('notes-ids', bodyRefs, [['word/footnotes.xml', notes('footnote', '<w:footnote w:id="1e3"><w:p/></w:footnote><w:footnote w:id="-0"><w:p/></w:footnote><w:footnote w:id="0"><w:p/></w:footnote><w:footnote w:id=" 7 "><w:p/></w:footnote><w:footnote w:id="0x10"><w:p/></w:footnote><w:footnote w:id="NaN"><w:p/></w:footnote>')]]);
  add('notes-rich', bodyRefs, [['word/footnotes.xml', notes('footnote', `<w:footnote w:id="2"><w:p>${txbx('<w:p><w:r><w:t>nb</w:t></w:r></w:p>')}</w:p><w:p><w:bookmarkStart w:id="1" w:name="n"/></w:p></w:footnote>`)]]);
  add('notes-empty-parts', bodyRefs, [['word/footnotes.xml', ''], ['word/endnotes.xml', `${PROLOG}<w:endnotes/>`]]);

  // core properties
  const core = (inner) => ['docProps/core.xml', `${PROLOG}<cp:coreProperties ${NS}>${inner}</cp:coreProperties>`];
  add('core-basic', p('x'), [core('<dcterms:created xsi:type="dcterms:W3CDTF">2020-01-02T03:04:05Z</dcterms:created><dcterms:modified xsi:type="dcterms:W3CDTF"> 2021-02-03T04:05:06Z </dcterms:modified><cp:lastPrinted>2019-01-01T00:00:00Z</cp:lastPrinted>')]);
  add('core-partial', p('x'), [core('<dcterms:modified>2021-02-03T04:05:06Z</dcterms:modified>')]);
  add('core-none-of-them', p('x'), [core('<dc:title>t</dc:title>')]);
  add('core-empty-tags', p('x'), [core('<dcterms:created/><dcterms:modified></dcterms:modified><cp:lastPrinted>  </cp:lastPrinted>')]);
  add('core-selfclose-then-text', p('x'), [core('<dcterms:created/>\n  <dcterms:modified>m</dcterms:modified>')]);
  add('core-prefix-of-longer', p('x'), [core('<dcterms:created2>no</dcterms:created2><dcterms:createdX a="b">yes</dcterms:createdX><dcterms:created>real</dcterms:created>')]);
  add('core-nested', p('x'), [core('<dcterms:created><b>inner</b>tail</dcterms:created><cp:lastPrinted>é😀</cp:lastPrinted>')]);
  add('core-empty-part', p('x'), [['docProps/core.xml', '']]);
  add('core-no-close', p('x'), [['docProps/core.xml', '<dcterms:created>never closed']]);

  // page background
  add('bg-color', p('x'), [], { doc: docXml(p('x'), { pre: '<w:background w:color="FFFF00" w:themeColor="accent1" w:themeTint="80" w:themeShade="40"/>' }) });
  add('bg-empty', p('x'), [], { doc: docXml(p('x'), { pre: '<w:background/>' }) });
  const vfill = (attrs, rid = 'rIdImg') => `<w:background w:color="auto"><v:background id="_x0000_s1025" o:bwmode="white" fillcolor="#ffe7c1" o:targetscreensize="800,600"><v:fill ${attrs}${rid ? ` r:id="${rid}"` : ''}/></v:background></w:background>`;
  add('bg-fill-image', p('x'), [['word/media/image1.png', 'PNGDATA']], { doc: docXml(p('x'), { pre: vfill('type="frame" color2="#ffffff" angle="45" focus="50%" focusposition=".5,.5" method="sigma" o:title="Pic"') }) });
  add('bg-fill-missing-image', p('x'), [], { doc: docXml(p('x'), { pre: vfill('type="tile"', 'rIdNone') }) });
  add('bg-fill-external-image', p('x'), [], { doc: docXml(p('x'), { pre: vfill('type="tile"', 'rIdLink') }) });
  add('bg-fill-gradient', p('x'), [], { doc: docXml(p('x'), { pre: vfill('type="gradient" colors="0 #fff;.5 red;1 #00f" angle="90" focus="-50%"', '') }) });

  // chains of VML text boxes
  const chain = (...boxes) => `<w:p>${boxes.join('')}</w:p>`;
  add('vml-chain-basic', chain(vbox('a', p('A'), 'b'), vbox('b', p('B'), 'c'), vbox('c', p('C'))));
  add('vml-chain-loop', chain(vbox('a', p('A'), 'b'), vbox('b', p('B'), 'a')));
  add('vml-chain-head-and-loop', chain(vbox('h', p('H'), 'a'), vbox('a', p('A'), 'b'), vbox('b', p('B'), 'a')));
  add('vml-chain-dangling', chain(vbox('a', p('A'), 'zzz'), vbox('b', p('B'))));
  add('vml-chain-self', chain(vbox('a', p('A'), 'a')));
  add('vml-chain-dup-ids', chain(vbox('a', p('A1'), 'b'), vbox('a', p('A2')), vbox('b', p('B1')), vbox('b', p('B2'), 'c'), vbox('c', p('C'))));
  add('vml-chain-two-heads-one-tail', chain(vbox('a', p('A'), 'c'), vbox('b', p('B'), 'c'), vbox('c', p('C'))));
  add('vml-chain-empty-next', `<w:p><w:r><w:pict><v:shape id="a" type="#_x0000_t202" style="position:absolute"><v:textbox style="mso-next-textbox:#"><w:txbxContent>${p('A')}</w:txbxContent></v:textbox></v:shape></w:pict></w:r></w:p>`);
  add('vml-chain-in-cells', tbl(`<w:tr><w:tc><w:p>${vbox('a', p('A'), 'b')}</w:p></w:tc><w:tc><w:p>${vbox('b', p('B'))}</w:p></w:tc></w:tr>`));
  add('vml-chain-nested-member', chain(vbox('a', p('A'), 'b'), vbox('b', `<w:p>${vbox('x', p('X'), 'y')}${vbox('y', p('Y'))}</w:p>`, 'c'), vbox('c', p('C'))));
  add('vml-chain-nested-head', chain(vbox('o', `<w:p>${vbox('x', p('X'), 'y')}${vbox('y', p('Y'))}</w:p>`), vbox('a', p('A'), 'o')));
  add('vml-chain-with-wps-story', `<w:p>${txbx(p('W'))}${vbox('a', p('A'), 'b')}${vbox('b', p('B'))}</w:p>`);
  add('vml-chain-two-chains', chain(vbox('a', p('A'), 'b'), vbox('b', p('B')), vbox('c', p('C'), 'd'), vbox('d', p('D'))));
  add('vml-chain-in-hf', sect('<w:headerReference w:type="default" r:id="rIdH1"/>'), [HDR(`<w:p>${vbox('a', p('A'), 'b')}${vbox('b', p('B'))}</w:p>`)]);
  add('vml-chain-in-group', `<w:p><w:r><w:pict><v:group id="g" style="position:absolute;width:200pt;height:100pt" coordsize="2000,1000"><v:shape id="a" type="#_x0000_t202" style="position:absolute;width:90pt;height:40pt"><v:textbox style="mso-next-textbox:#b"><w:txbxContent>${p('A')}</w:txbxContent></v:textbox></v:shape><v:shape id="b" type="#_x0000_t202" style="position:absolute;width:90pt;height:40pt"><v:textbox><w:txbxContent>${p('B')}</w:txbxContent></v:textbox></v:shape></v:group></w:pict></w:r></w:p>`);

  // block content controls with a gallery
  const cover = (gallery, inner) => `<w:sdt><w:sdtPr><w:id w:val="1"/><w:docPartObj><w:docPartGallery w:val="${gallery}"/><w:docPartUnique/></w:docPartObj></w:sdtPr><w:sdtContent>${inner}</w:sdtContent></w:sdt>`;
  add('sdt-cover', `${cover('Cover Pages', `${p('title')}${p('sub')}${tbl('<w:tr><w:tc>' + p('t') + '</w:tc></w:tr>')}`)}${cover('Table of Contents', p('toc'))}<w:sdt><w:sdtPr><w:docPartObj/></w:sdtPr><w:sdtContent>${p('nogallery')}</w:sdtContent></w:sdt><w:sdt><w:sdtPr><w:text/></w:sdtPr><w:sdtContent>${p('plain')}</w:sdtContent></w:sdt><w:sdt><w:sdtContent>${p('nopr')}</w:sdtContent></w:sdt>${sect()}`);
  add('sdt-nested', `<w:sdt><w:sdtPr><w:docPartObj><w:docPartGallery w:val="Cover Pages"/></w:docPartObj></w:sdtPr><w:sdtContent>${p('a')}<w:sdt><w:sdtPr><w:docPartObj><w:docPartGallery w:val="Inner"/></w:docPartObj></w:sdtPr><w:sdtContent>${p('b')}</w:sdtContent></w:sdt><w:customXml w:element="x"><w:customXmlPr/>${p('c')}</w:customXml></w:sdtContent></w:sdt>`);
  add('sdt-empty', `<w:sdt><w:sdtPr><w:docPartObj><w:docPartGallery w:val="Cover Pages"/></w:docPartObj></w:sdtPr><w:sdtContent/></w:sdt><w:sdt><w:sdtPr><w:docPartObj/></w:sdtPr></w:sdt>${p('x')}`);
  add('customxml-body', `<w:customXml w:element="e"><w:customXmlPr><w:attr w:name="a" w:val="b"/></w:customXmlPr>${p('in')}${tbl('<w:tr><w:tc>' + p('t') + '</w:tc></w:tr>')}</w:customXml><w:customXml w:element="f"/>${p('out')}`);

  // tables
  const tc = (inner = p('c'), pr = '') => `<w:tc>${pr}${inner}</w:tc>`;
  add('tbl-cells', tbl(`<w:tr><w:trPr><w:cantSplit/><w:trHeight w:val="400"/></w:trPr><w:tblPrEx><w:tblW w:w="0" w:type="auto"/></w:tblPrEx>${tc(p('a'), '<w:tcPr><w:tcW w:w="2000" w:type="dxa"/><w:shd w:val="clear" w:fill="FFFF00"/><w:gridSpan w:val="2"/><w:tcBorders><w:top w:val="single" w:sz="4" w:color="auto"/><w:insideH w:val="dashed"/></w:tcBorders></w:tcPr>')}${tc(p('b'), '<w:tcPr><w:tcW w:w="x" w:type="dxa"/><w:shd w:val="clear" w:fill="auto"/><w:gridSpan w:val="x"/></w:tcPr>')}${tc(p('c'), '<w:tcPr><w:tcW w:w="50" w:type="pct"/><w:shd w:fill=""/></w:tcPr>')}${tc(p('d'), '<w:tcPr><w:tcW w:type="dxa"/><w:gridSpan w:val="0"/></w:tcPr>')}${tc('<w:tbl><w:tr><w:tc/></w:tr></w:tbl>')}${tc('')}<w:tc/></w:tr><w:tr/><w:tr><w:trPr/></w:tr>`));
  add('tbl-grid', `<w:tbl><w:tblPr><w:tblBorders><w:top w:val="single" w:sz="4"/><w:left w:val="nil"/><w:insideH w:val="dotted" w:sz="2" w:color="00FF00"/><w:insideV/></w:tblBorders></w:tblPr><w:tblGrid><w:gridCol w:w="100"/><w:gridCol/><w:gridCol w:w="x"/><w:gridCol w:w="12pt"/><w:gridCol w:w=""/><w:gridCol w:w="1e3"/></w:tblGrid><w:tr>${tc()}</w:tr></w:tbl>`);
  add('tbl-no-grid', '<w:tbl><w:tr><w:tc><w:p/></w:tc></w:tr></w:tbl>');
  add('tbl-empty', '<w:tbl/><w:tbl><w:tblPr/><w:tblGrid/></w:tbl>');
  add('tbl-wrapped-rows', `<w:tbl><w:tblPr/><w:tblGrid><w:gridCol w:w="1"/></w:tblGrid><w:sdt><w:sdtPr/><w:sdtContent><w:tr>${tc()}</w:tr><w:sdt><w:sdtPr/><w:sdtContent><w:tr>${tc()}</w:tr></w:sdtContent></w:sdt></w:sdtContent></w:sdt><w:customXml w:element="r"><w:customXmlPr/><w:tr>${tc()}</w:tr></w:customXml><w:tr><w:sdt><w:sdtPr/><w:sdtContent>${tc()}</w:sdtContent></w:sdt><w:customXml w:element="c"><w:customXmlPr/>${tc()}</w:customXml><w:bookmarkStart w:id="9" w:name="x"/>${tc()}</w:tr><w:sdt><w:sdtPr/><w:sdtContent/></w:sdt></w:tbl>`);
  add('tbl-cell-content', tbl(`<w:tr><w:tc><w:sdt><w:sdtPr><w:docPartObj><w:docPartGallery w:val="G"/></w:docPartObj></w:sdtPr><w:sdtContent>${p('in sdt')}</w:sdtContent></w:sdt><w:bookmarkStart w:id="1" w:name="cb"/><w:customXml w:element="e"><w:customXmlPr/>${p('cx')}${tbl('<w:tr><w:tc>' + p('nested') + '</w:tc></w:tr>')}</w:customXml></w:tc><w:tc>${tbl('<w:tr><w:tc/></w:tr>')}</w:tc></w:tr>`));
  add('tbl-in-sdt-in-cell-only-tables', tbl(`<w:tr><w:tc>${tbl('<w:tr><w:tc>' + tbl('<w:tr><w:tc/></w:tr>') + '</w:tc></w:tr>')}</w:tc></w:tr>`));

  // rels: the target forms (the library resolves them with the OPC rule; the TS prefixes `word/`)
  const hrels = (target, ext = false) => rels([['rIdH1', 'header', target, ext]]);
  const withTarget = (name, target, ext) => add(name, sect('<w:headerReference w:type="default" r:id="rIdH1"/>'), [HDR(p('h')), ['header1.xml', story('w:hdr', p('root'))]], { docRels: hrels(target, ext) });
  withTarget('rel-plain', 'header1.xml');
  withTarget('rel-dot-slash', './header1.xml');
  withTarget('rel-absolute', '/word/header1.xml');
  withTarget('rel-dotdot', '../word/header1.xml');
  withTarget('rel-external', 'header1.xml', true);
  withTarget('rel-uppercase', 'HEADER1.XML');
  withTarget('rel-fragment', 'header1.xml#x');
  withTarget('rel-encoded', 'header%31.xml');
  withTarget('rel-empty', '');
  add('rel-duplicate-ids', sect('<w:headerReference w:type="default" r:id="rIdH1"/><w:footerReference w:type="default" r:id="rIdH1"/>'), [HDR(), ['word/footer1.xml', story('w:ftr', p('f'))]], { docRels: rels([['rIdH1', 'header', 'header1.xml'], ['rIdH1', 'footer', 'footer1.xml']]) });
  add('rel-duplicate-ids-last-missing', sect('<w:headerReference w:type="default" r:id="rIdH1"/>'), [HDR()], { docRels: rels([['rIdH1', 'header', 'header1.xml'], ['rIdH1', 'header', 'nothing.xml']]) });
  add('rel-none', sect('<w:headerReference w:type="default" r:id="rIdH1"/>'), [HDR()], { docRels: null });
  add('rel-bad-xml', sect('<w:headerReference w:type="default" r:id="rIdH1"/>'), [HDR()], { docRels: '<Relationships><Relationship' });
  add('rel-empty-part', sect('<w:headerReference w:type="default" r:id="rIdH1"/>'), [HDR()], { docRels: '' });

  // empty and odd parts around the body
  add('body-whitespace', `  \n<!-- c -->${p('a')}\n  `);
  add('body-text-children', `text${p('a')}more`);
  add('body-surrogates', `<w:p><w:r><w:t>&#x1F600; a&#xD800;b &#xDC00;</w:t></w:r></w:p>`);
  return out;
}

/** Nesting around the limit of 64 stories (G20): `n` tables in cells, `n` text boxes in text boxes. */
function nesting() {
  const out = [];
  const leaf = p('leaf');
  const tables = (n) => (n === 0 ? leaf : tbl(`<w:tr><w:tc>${tables(n - 1)}${p(`t${n}`)}</w:tc></w:tr>`));
  const boxes = (n) => (n === 0 ? leaf : `<w:p>${txbx(boxes(n - 1), n)}</w:p>`);
  const vboxes = (n) => (n === 0 ? leaf : `<w:p>${vbox(`v${n}`, vboxes(n - 1))}</w:p>`);
  const mixed = (n) => (n === 0 ? leaf : n % 2 ? tbl(`<w:tr><w:tc>${mixed(n - 1)}</w:tc></w:tr>`) : `<w:p>${txbx(mixed(n - 1), n)}</w:p>`);
  for (const n of [1, 2, 5, 31, 32, 62, 63, 64, 65, 66, 67, 70]) {
    out.push(pkg(`nest-tables-${n}`, tables(n)));
    out.push(pkg(`nest-boxes-${n}`, boxes(n)));
    out.push(pkg(`nest-mixed-${n}`, mixed(n)));
  }
  for (const n of [3, 63, 64, 65]) out.push(pkg(`nest-vml-${n}`, vboxes(n)));
  return out;
}

/** Seeded documents: the R6 generator's blocks and parts with random sections, references, notes. */
function seeded(seed, count) {
  const out = [];
  for (let i = 0; i < count; i++) {
    const g = new Gen((seed + i * 0x9e3779b1) >>> 0);
    const rng = new Rng((seed ^ (i * 0x85ebca6b)) >>> 0);
    const pick = (a) => a[rng.below(a.length)];
    const many = (n, f) => { let s = ''; for (let k = rng.below(n + 1); k > 0; k--) s += f(); return s; };
    const refs = () => many(3, () => pick([
      '<w:headerReference w:type="default" r:id="rIdH1"/>', '<w:headerReference w:type="first" r:id="rIdH2"/>', '<w:headerReference w:type="even" r:id="rIdH3"/>',
      '<w:footerReference w:type="default" r:id="rIdF1"/>', '<w:footerReference w:type="first" r:id="rIdF2"/>', '<w:footerReference w:type="even" r:id="rIdNone"/>',
      '<w:headerReference w:type="default" r:id="rIdH2"/>', '<w:footerReference r:id="rIdF1"/>',
    ]));
    const props = () => many(4, () => pick([
      '<w:type w:val="continuous"/>', '<w:cols w:num="2" w:space="360"/>', '<w:cols w:equalWidth="0"><w:col w:w="2000" w:space="100"/><w:col w:w="3000"/></w:cols>', '<w:pgNumType w:fmt="lowerRoman"/>',
      '<w:lnNumType w:countBy="1"/>', '<w:vAlign w:val="both"/>', '<w:docGrid w:linePitch="360"/>', '<w:bidi/>', '<w:titlePg/>', '<w:pgMar w:gutter="200"/>', '<w:pgBorders w:display="firstPage"><w:top w:val="single"/></w:pgBorders>',
    ]));
    g.openField = 0;
    let body = '';
    const blocks = 1 + rng.below(6);
    for (let k = 0; k < blocks; k++) {
      body += g.blocks(0, 2);
      if (rng.below(3) === 0) body += `<w:p><w:pPr><w:sectPr>${refs()}${props()}<w:pgSz w:w="12240" w:h="15840"/></w:sectPr></w:pPr></w:p>`;
      if (rng.below(8) === 0) body += `<w:bookmarkStart w:id="${900 + k}" w:name="top${k}"/><w:bookmarkEnd w:id="${900 + k}"/>`;
    }
    if (g.openField > 0) body += '<w:p><w:r><w:fldChar w:fldCharType="end"/></w:r></w:p>';
    if (rng.below(6) !== 0) body += `<w:sectPr>${refs()}${props()}<w:pgSz w:w="11906" w:h="16838"/></w:sectPr>`;
    const bg = rng.below(8) === 0 ? '<w:background w:color="E0E0E0"/>' : '';
    const parts = [['word/document.xml', docXml(body, { pre: bg })], ['word/_rels/document.xml.rels', HF_RELS]];
    for (const [path, root] of [['word/header1.xml', 'w:hdr'], ['word/header2.xml', 'w:hdr'], ['word/header3.xml', 'w:hdr'], ['word/footer1.xml', 'w:ftr'], ['word/footer2.xml', 'w:ftr']]) {
      if (rng.below(2) === 0) parts.push([path, g.story(root, NS)]);
    }
    if (rng.below(2) === 0) parts.push(['word/footnotes.xml', g.notesXml('footnote')]);
    if (rng.below(3) === 0) parts.push(['word/endnotes.xml', g.notesXml('endnote')]);
    if (rng.below(3) === 0) parts.push(['word/comments.xml', `${PROLOG}<w:comments ${NS}>${[0, 1, 2].filter(() => rng.below(2) === 0).map((id) => `<w:comment w:id="${id}" w:author="Au"><w:p><w:r><w:t>c${id}</w:t></w:r></w:p></w:comment>`).join('')}</w:comments>`]);
    if (rng.below(4) === 0) parts.push(['docProps/core.xml', `${PROLOG}<cp:coreProperties ${NS}><dcterms:created>2020-0${1 + rng.below(9)}-01T00:00:00Z</dcterms:created>${rng.below(2) ? '<dcterms:modified>2021-01-01T00:00:00Z</dcterms:modified>' : ''}</cp:coreProperties>`]);
    if (rng.below(2) === 0) parts.push(['word/media/image1.png', 'IMG']);
    out.push({ key: `quirk/full/seeded-${seed.toString(16)}-${i}.docx`, parts });
  }
  return out;
}

/** The generated packages: `{ key, parts: [[path, text]] }`. */
export function quirkFull() {
  const all = [...handWritten(), ...nesting(), ...seeded(QUIRK_FULL_SEED, QUIRK_FULL_SEEDED)];
  const keys = new Set();
  for (const c of all) { if (keys.has(c.key)) throw new Error(`duplicate key ${c.key}`); keys.add(c.key); }
  return all;
}

/** The inputs file (one part per line, TAB separated; `\`, LF, CR and TAB escaped). A package without
 *  `word/document.xml` has none listed: the TS side removes the part its `DocxPackage.empty()` starts with. */
export function inputsText(inputs) {
  const esc = (x) => x.replace(/\\/g, '\\\\').replace(/\n/g, '\\n').replace(/\r/g, '\\r').replace(/\t/g, '\\t');
  const lines = ['# uo-docx R5 synthetic whole-document packages (js/quirk-full.mjs): `@input <key>`, then `<path>\\t<escaped text>` per part'];
  for (const { key, parts } of inputs) {
    lines.push(`@input ${key}`);
    for (const [path, data] of parts) lines.push(`${path}\t${esc(data)}`);
  }
  return lines.join('\n') + '\n';
}
