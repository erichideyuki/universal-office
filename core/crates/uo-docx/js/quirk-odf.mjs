// Generated `.odt` packages with quirks for the D5b port (`odf/**`: `readOdt`), since the repository has no
// `.odt` of its own (plan U20) and LibreOffice/Word are not available on the build machines: namespaces
// and prefixes, missing parts, style inheritance chains (cycles, defaults, duplicates), the units of
// `units.ts` in every form it accepts and the malformed ones, the constructs of `index.ts` (headings,
// lists, tables with spans and repeats, frames with images and text boxes, annotations, notes, bookmarks,
// unknown elements), and seeded random documents that mix them. `capture-odf.mjs` runs the frozen TS over
// them and writes them next to the capture (tests/parity/odf.inputs); the Rust replay reads that file, so
// both sides see the same bytes.
//
// A case is `{ key, parts: [[name, string | Uint8Array]] }` (the zip is built from the parts, in order,
// with `mimetype` stored first) or `{ key, raw: Uint8Array }` (zip-level damage, the bytes as they are).
import { Rng } from '../../uo-testkit/js/testkit.mjs';

export const QUIRK_ODF_SEED = 0x0df5b001;
export const QUIRK_ODF_SEEDED = 400;
export const MIME = 'application/vnd.oasis.opendocument.text';

export const NS = [
  'xmlns:office="urn:oasis:names:tc:opendocument:xmlns:office:1.0"',
  'xmlns:style="urn:oasis:names:tc:opendocument:xmlns:style:1.0"',
  'xmlns:text="urn:oasis:names:tc:opendocument:xmlns:text:1.0"',
  'xmlns:table="urn:oasis:names:tc:opendocument:xmlns:table:1.0"',
  'xmlns:draw="urn:oasis:names:tc:opendocument:xmlns:drawing:1.0"',
  'xmlns:fo="urn:oasis:names:tc:opendocument:xmlns:xsl-fo-compatible:1.0"',
  'xmlns:xlink="http://www.w3.org/1999/xlink"',
  'xmlns:dc="http://purl.org/dc/elements/1.1/"',
  'xmlns:svg="urn:oasis:names:tc:opendocument:xmlns:svg-compatible:1.0"',
].join(' ');
const PROLOG = '<?xml version="1.0" encoding="UTF-8"?>';

export const content = (body, auto = '', { ns = NS, tag = 'office:document-content' } = {}) =>
  `${PROLOG}<${tag} ${ns} office:version="1.3"><office:automatic-styles>${auto}</office:automatic-styles><office:body><office:text>${body}</office:text></office:body></${tag}>`;
export const styles = (named = '', auto = '', { ns = NS } = {}) =>
  `${PROLOG}<office:document-styles ${ns} office:version="1.3"><office:styles>${named}</office:styles><office:automatic-styles>${auto}</office:automatic-styles></office:document-styles>`;
const PNG = new Uint8Array([0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a, 0, 0, 0, 13, 0x49, 0x48, 0x44, 0x52]);
const b64 = (u8) => Buffer.from(u8).toString('base64');

/** A package from content (+ optional styles) with the usual extra parts. */
function odt(key, c, s, extra = []) {
  const parts = [['mimetype', MIME], ['content.xml', c]];
  if (s !== undefined && s !== null) parts.push(['styles.xml', s]);
  parts.push(['META-INF/manifest.xml', '<manifest/>'], ...extra);
  return { key: `quirk/odf/${key}.odt`, parts };
}

const sty = (name, family, inner = '', attrs = '') => `<style:style style:name="${name}" style:family="${family}"${attrs}>${inner}</style:style>`;
const pp = (a) => `<style:paragraph-properties ${a}/>`;
const tp = (a) => `<style:text-properties ${a}/>`;
const para = (text, style = '') => `<text:p${style ? ` text:style-name="${style}"` : ''}>${text}</text:p>`;

/** The lengths of `units.ts`: every unit, every form the regex takes, and what it refuses. */
export const LENGTHS = [
  '0cm', '1cm', '2.54cm', '0.5in', '1in', '12pt', '0pt', '10mm', '120px', '1pc', '1.27cm', '-1.5pt', '-0cm', '.5in', '-.5in', '+3pt', '5.pt', '5.', '.', '-', '', ' ', '  12pt  ',
  '\t1cm\n', ' 2 in ', '1 cm', '1cm ', ' 1cm', '1CM', '1Cm', '1 pt', '1pt1', '1pt pt', '1,5cm', '1e3pt', '1E3pt', '0x10pt', '1_0pt', 'NaN', 'Infinity', 'Infinitypt', '-Infinity pt', '10', '10 ', 'pt', 'px',
  '12.5', '12.5.5pt', '0.000001cm', '00012pt', '007.5mm', '99999999999pt', '1e21pt', '123456789012345678901234567890pt', `${'9'.repeat(400)}cm`, `-${'9'.repeat(400)}pt`, `${'0'.repeat(60)}1in`,
  '0.00000000001in', '1﻿cm', '﻿1cm', '1 cm', ' 1cm', '1　pt', '1pt', '1᠎pt', '1​cm', '1 cm', ' 1cm', '1٣cm', '٣cm', '１２pt', '1cm2', '1cmx', 'cm1',
  '1.5in', '2.5pc', '3.3mm', '14.4pt', '0.35mm', '17.5cm', '21cm', '29.7cm', '8.5in', '11in', '1.1pt', '2.2pt', '0.1pt', '0.05pt', '0.3pt', '8.5pt', '9.5pt', '10.5pt', '11.5pt', '5.5pt', '6.5pt', '0.25pt', '0.75pt',
  '1px', '3px', '7px', '13px', '96px', '97px', '1.5px', '100.5px', '0.1in', '0.2in', '0.3in', '0.7in', '1.25in', '1.0in', '2in', '3in', '0.1cm', '0.2cm', '0.45cm', '1.5cm', '2cm', '3cm', '5cm',
];

function hand() {
  const out = [];
  const add = (...a) => out.push(odt(...a));

  // ───── units: every length in the places the reader takes one ─────
  LENGTHS.forEach((l, i) => {
    const e = l.replace(/&/g, '&amp;').replace(/"/g, '&quot;').replace(/</g, '&lt;');
    const auto = sty('P1', 'paragraph', pp(`fo:margin-left="${e}" fo:margin-right="${e}" fo:text-indent="${e}" fo:margin-top="${e}" fo:margin-bottom="${e}" fo:border="${e} solid #123456" fo:background-color="#abc"`) + tp(`fo:font-size="${e}"`))
      + sty('T1', 'table-cell', '<style:table-cell-properties fo:border="' + e + ' solid #000000" fo:background-color="' + e + '"/>')
      + sty('C1', 'table-column', `<style:table-column-properties style:column-width="${e}"/>`);
    const body = para('x', 'P1')
      + `<table:table><table:table-column table:style-name="C1" table:number-columns-repeated="2"/><table:table-row><table:table-cell table:style-name="T1"><text:p>c</text:p></table:table-cell><table:table-cell><text:p>d</text:p></table:table-cell></table:table-row></table:table>`
      + `<text:p><draw:frame svg:width="${e}" svg:height="${e}" draw:name="i"><draw:image xlink:href="Pictures/a.png"/></draw:frame></text:p>`
      + `<text:p><draw:frame svg:width="${e}" svg:height="${e}" svg:x="${e}" svg:y="${e}" draw:style-name="G1" text:anchor-type="page" draw:z-index="${i}"><draw:text-box fo:min-width="${e}" fo:min-height="${e}"><text:p>b</text:p></draw:text-box></draw:frame></text:p>`;
    const s = styles('', `<style:page-layout style:name="pm1"><style:page-layout-properties fo:page-width="${e}" fo:page-height="${e}" fo:margin-left="${e}" fo:margin-right="${e}" fo:margin-top="${e}" fo:margin-bottom="${e}"/></style:page-layout>`)
      .replace('</office:automatic-styles>', sty('G1', 'graphic', `<style:graphic-properties fo:padding="${e}" fo:padding-left="${e}" style:wrap="parallel"/>`) + '</office:automatic-styles>');
    add(`unit-${i}`, content(body, auto), s, [['Pictures/a.png', PNG]]);
  });

  // ───── text properties ─────
  const tpCases = [
    ['bold', 'fo:font-weight="bold"'], ['w700', 'fo:font-weight="700"'], ['w400', 'fo:font-weight="400"'], ['wnormal', 'fo:font-weight="normal"'], ['wtrue', 'fo:font-weight="true"'], ['wspace', 'fo:font-weight="bold "'],
    ['italic', 'fo:font-style="italic"'], ['oblique', 'fo:font-style="oblique"'], ['normal', 'fo:font-style="normal"'], ['empty-style', 'fo:font-style=""'],
    ['u-solid', 'style:text-underline-style="solid"'], ['u-none', 'style:text-underline-style="none"'], ['u-empty', 'style:text-underline-style=""'], ['st-solid', 'style:text-line-through-style="solid"'], ['st-none', 'style:text-line-through-style="none"'],
    ['color', 'fo:color="#aabbcc"'], ['color-nohash', 'fo:color="aabbcc"'], ['color-short', 'fo:color="#abc"'], ['color-name', 'fo:color="red"'], ['color-sharp', 'fo:color="straße"'], ['color-ff', 'fo:color="#ﬀ00ff"'], ['color-turkish', 'fo:color="#ıİiI"'], ['color-empty', 'fo:color=""'], ['color-2hash', 'fo:color="##ab"'],
    ['bg', 'fo:background-color="#ffff00"'], ['bg-transparent', 'fo:background-color="transparent"'], ['bg-empty', 'fo:background-color=""'],
    ['size', 'fo:font-size="12pt"'], ['size-bad', 'fo:font-size="big"'], ['size-1', 'fo:font-size="0.5pt"'], ['size-odd', 'fo:font-size="10.3pt"'], ['size-mm', 'fo:font-size="4.2mm"'],
    ['font-name', 'style:font-name="Liberation Serif"'], ['font-quoted', `fo:font-family="'Arial'"`], ['font-dq', 'fo:font-family="&quot;Times New Roman&quot;, serif"'], ['font-list', 'fo:font-family="A,B,C"'], ['font-squote', `fo:font-family="'"`], ['font-quotes2', `fo:font-family="''"`],
    ['font-space', 'fo:font-family=" X , Y "'], ['font-both', 'style:font-name="N" fo:font-family="F"'], ['font-empty-name', 'style:font-name="" fo:font-family="F"'], ['font-comma', 'fo:font-family=",x"'], ['font-end-quote', `fo:font-family="x'"`], ['font-nbsp', 'fo:font-family=" x "'],
    ['caps', 'fo:font-variant="small-caps"'], ['caps2', 'style:font-variant="small-caps"'], ['caps-no', 'fo:font-variant="normal"'], ['upper', 'fo:text-transform="uppercase"'], ['upper-no', 'fo:text-transform="lowercase"'],
    ['pos-super', 'style:text-position="super 58%"'], ['pos-sub', 'style:text-position="sub 58%"'], ['pos-pct', 'style:text-position="33% 58%"'], ['pos-neg', 'style:text-position="-33% 58%"'], ['pos-zero', 'style:text-position="0% 100%"'], ['pos-supers', 'style:text-position="superscript"'],
    ['pos-subs', 'style:text-position="subfoo"'], ['pos-space', 'style:text-position=" super"'], ['pos-exact-super', 'style:text-position="super"'], ['pos-exact-sub', 'style:text-position="sub"'], ['pos-super_', 'style:text-position="super_x"'], ['pos-super-', 'style:text-position="super-x"'], ['pos-supé', 'style:text-position="superé"'],
    ['pos-0', 'style:text-position="0"'], ['pos-minus', 'style:text-position="-"'], ['pos-empty', 'style:text-position=""'], ['pos-9', 'style:text-position="9"'],
    ['all', 'fo:font-weight="bold" fo:font-style="italic" style:text-underline-style="solid" style:text-line-through-style="solid" fo:color="#ff0000" fo:background-color="#00ff00" fo:font-size="14pt" style:font-name="X" fo:font-variant="small-caps" fo:text-transform="uppercase" style:text-position="super 58%"'],
  ];
  tpCases.forEach(([name, a], i) => {
    const auto = sty('T1', 'text', tp(a)) + sty('P1', 'paragraph', tp(a));
    add(`tp-${i}-${name}`, content(`${para('plain ', 'P1')}${para(`a<text:span text:style-name="T1">b</text:span><text:span text:style-name="T1"><text:span>c</text:span></text:span>`)}`, auto));
  });

  // ───── paragraph properties ─────
  const ppCases = [
    'fo:text-align="start"', 'fo:text-align="left"', 'fo:text-align="end"', 'fo:text-align="right"', 'fo:text-align="center"', 'fo:text-align="justify"', 'fo:text-align="LEFT"', 'fo:text-align=""', 'fo:text-align="inherit"',
    'fo:margin-left="1cm" fo:margin-right="2cm" fo:text-indent="-0.5cm" fo:margin-top="3pt" fo:margin-bottom="4pt"', 'fo:margin="1cm"', 'fo:background-color="#abcdef"', 'fo:background-color="transparent"', 'fo:break-before="page"', 'fo:break-before="column"', 'fo:break-before="auto"',
    'fo:border="0.5pt solid #000000"', 'fo:border="1pt dashed #ff0000"', 'fo:border="2pt double #00FF00"', 'fo:border="0.1pt dotted #abc"', 'fo:border="none"', 'fo:border="1pt hidden"', 'fo:border="1pt solid none"', 'fo:border="solid"', 'fo:border="#12345g"', 'fo:border="#1234567"', 'fo:border="1pt 2pt"',
    'fo:border="   "', 'fo:border=" 1pt  solid  #000 "', 'fo:border-top="1pt solid #111111" fo:border-bottom="2pt dashed #222222"', 'fo:border-left="3pt dotted" fo:border-right="none" fo:border="1pt solid #333333"', 'fo:border-top="none" fo:border="1pt solid #444444"', 'fo:border="0pt solid #000000"', 'fo:border="1.9pt groove #000000"',
    'fo:border="99999999999pt solid"', 'fo:border="1px solid red"', 'fo:border="1pt solid"', 'fo:border="1pt　solid　#aabbcc"',
  ];
  ppCases.forEach((a, i) => {
    const auto = sty('P1', 'paragraph', pp(a));
    add(`pp-${i}`, content(`${para('x', 'P1')}${para('y', 'P1')}<text:h text:outline-level="1" text:style-name="P1">h</text:h>`, auto));
  });
  add('pp-border-chain', content(para('x', 'P3')), styles(sty('P1', 'paragraph', pp('fo:border="1pt solid #010101"')) + sty('P2', 'paragraph', '', ' style:parent-style-name="P1"') + sty('P3', 'paragraph', pp('fo:margin-left="1cm"'), ' style:parent-style-name="P2"')));
  add('pp-border-chain-none', content(para('x', 'P3')), styles(sty('P1', 'paragraph', '') + sty('P2', 'paragraph', '', ' style:parent-style-name="P1"') + sty('P3', 'paragraph', '', ' style:parent-style-name="P2"')));
  add('pp-border-cycle', content(para('x', 'P1')), styles(sty('P1', 'paragraph', '', ' style:parent-style-name="P2"') + sty('P2', 'paragraph', '', ' style:parent-style-name="P1"')));
  add('pp-border-self', content(para('x', 'P1')), styles(sty('P1', 'paragraph', '', ' style:parent-style-name="P1"')));
  add('pp-border-missing-parent', content(para('x', 'P1')), styles(sty('P1', 'paragraph', '', ' style:parent-style-name="Nope"')));

  // ───── style chains ─────
  for (const depth of [1, 2, 3, 5, 10, 40, 300]) {
    let named = '';
    for (let i = 0; i < depth; i++) named += sty(`S${i}`, 'paragraph', (i % 3 === 0 ? tp(`fo:font-weight="${i % 2 ? 'bold' : 'normal'}" fo:font-size="${10 + i}pt"`) : '') + (i % 2 === 0 ? pp(`fo:margin-left="${i}mm"`) : ''), i + 1 < depth ? ` style:parent-style-name="S${i + 1}"` : '');
    add(`chain-${depth}`, content(para('x', 'S0') + para('y', `S${depth - 1}`)), styles(named));
  }
  add('chain-cycle', content(para('x', 'A') + para('y', 'B') + para('z', 'C')), styles(sty('A', 'paragraph', tp('fo:font-size="11pt"'), ' style:parent-style-name="B"') + sty('B', 'paragraph', tp('fo:color="#112233"'), ' style:parent-style-name="A"') + sty('C', 'paragraph', tp('fo:font-style="italic"'), ' style:parent-style-name="B"')));
  add('chain-cycle3', content(para('x', 'A') + para('y', 'C')), styles(sty('A', 'paragraph', tp('fo:font-size="11pt"'), ' style:parent-style-name="B"') + sty('B', 'paragraph', tp('fo:color="#112233"'), ' style:parent-style-name="C"') + sty('C', 'paragraph', pp('fo:margin-left="1cm"'), ' style:parent-style-name="A"')));
  add('chain-self', content(para('x', 'A')), styles(sty('A', 'paragraph', tp('fo:font-size="11pt"'), ' style:parent-style-name="A"')));
  add('chain-empty-parent', content(para('x', 'A')), styles(sty('A', 'paragraph', tp('fo:font-size="11pt"'), ' style:parent-style-name=""')));
  add('chain-missing', content(para('x', 'A') + para('y', 'Nope') + para('z', '')), styles(sty('A', 'paragraph', tp('fo:font-size="11pt"'), ' style:parent-style-name="Gone"')));
  add('chain-defaults', content(para('x', 'A') + `<text:p><text:span text:style-name="T">s</text:span></text:p>`), styles(`<style:default-style style:family="paragraph">${pp('fo:margin-left="3mm" fo:text-align="center"')}${tp('fo:font-size="9pt" fo:font-weight="bold"')}</style:default-style><style:default-style style:family="text">${tp('fo:color="#fedcba"')}</style:default-style><style:default-style style:family="">${tp('fo:font-size="1pt"')}</style:default-style>` + sty('A', 'paragraph', tp('fo:font-style="italic"'), ' style:parent-style-name="B"') + sty('B', 'paragraph', pp('fo:margin-left="7mm"')) + sty('T', 'text', '')));
  add('chain-dup-names', content(para('x', 'P1') + para('y', 'D'), sty('P1', 'paragraph', tp('fo:font-size="30pt"')) + sty('D', 'paragraph', tp('fo:color="#000001"'))), styles(sty('P1', 'paragraph', tp('fo:font-size="10pt"')) + sty('D', 'paragraph', tp('fo:color="#000002"')) + sty('D', 'paragraph', tp('fo:color="#000003"'), ' style:parent-style-name="P1"'), sty('P1', 'paragraph', tp('fo:font-size="20pt"'))));
  add('chain-empty-names', content(para('x', ''), sty('', 'paragraph', tp('fo:font-size="30pt"'))), styles(sty('', 'paragraph', tp('fo:font-size="10pt"')) + '<style:style style:family="paragraph"/>' + `<style:default-style/>`));
  add('chain-names-ws', content(para('x', 'Heading 1') + para('y', 'Text body') + `<text:h text:outline-level="2" text:style-name="Heading 2">h</text:h>`), styles(sty('Heading 1', 'paragraph', tp('fo:font-size="18pt"')) + sty('Text body', 'paragraph', pp('fo:margin-left="1cm"')) + sty('Heading 2', 'paragraph', tp('fo:color="#101010"')) + sty('Heading 2', 'paragraph', tp('fo:color="#202020"')) + sty('Heading1', 'paragraph', tp('fo:color="#303030"')) + sty(' A  B ', 'paragraph', '') + sty('x\ty', 'paragraph', '') + sty('  ', 'paragraph', '')));
  add('chain-heading-style-not-in-map', content('<text:h text:outline-level="3">h</text:h>'), styles(sty('Heading_20_3', 'paragraph', tp('fo:font-size="13pt"'))));
  add('chain-family-text', content(`<text:p>a<text:span text:style-name="T2">b</text:span></text:p>`), styles(`<style:default-style style:family="text">${tp('fo:font-weight="bold"')}</style:default-style>` + sty('T1', 'text', tp('fo:font-style="italic"')) + sty('T2', 'text', tp('fo:color="#445566"'), ' style:parent-style-name="T1"')));
  add('chain-list-style-inherit', content(`<text:list><text:list-item><text:p text:style-name="Q">a</text:p></text:list-item></text:list>`), styles(sty('Q', 'paragraph', '', ' style:parent-style-name="Q0"') + sty('Q0', 'paragraph', '', ' style:list-style-name="LN"') + '<text:list-style style:name="LN"><text:list-level-style-number text:level="1"/></text:list-style>'));
  add('styles-in-office-styles-and-auto', content(para('x', 'A') + para('y', 'B') + para('z', 'C')), `${PROLOG}<office:document-styles ${NS}><office:styles>${sty('A', 'paragraph', tp('fo:font-size="11pt"'))}</office:styles><office:automatic-styles>${sty('B', 'paragraph', tp('fo:font-size="12pt"'))}</office:automatic-styles><office:master-styles>${sty('C', 'paragraph', tp('fo:font-size="13pt"'))}</office:master-styles></office:document-styles>`);

  // ───── headings ─────
  ['1', '2', '3', '4', '5', '6', '7', '0', '-1', 'x', '', '2.7', ' 3', '1e1', '99', '9007199254740993', '+2', '０', '٣'].forEach((lv, i) => {
    add(`heading-${i}`, content(`<text:h text:outline-level="${lv}">title <text:span text:style-name="T1">bold</text:span></text:h><text:h>no level</text:h><text:h text:style-name="P1" text:outline-level="${lv}">styled</text:h>`, sty('P1', 'paragraph', tp('fo:font-size="21pt" fo:color="#010203"')) + sty('T1', 'text', tp('fo:font-weight="bold"'))));
  });
  add('heading-in-list', content(`<text:list><text:list-header><text:h text:outline-level="1">hh</text:h><text:p>hp</text:p></text:list-header><text:list-item><text:h text:outline-level="2">lh</text:h><text:p>lp</text:p></text:list-item></text:list>`));
  add('heading-look-override', content(`<text:h text:outline-level="1" text:style-name="H">a</text:h><text:h text:outline-level="2" text:style-name="H2">b</text:h>`), styles(sty('H', 'paragraph', tp('fo:font-weight="normal" fo:font-size="9pt" fo:color="#ffffff"')) + sty('H2', 'paragraph', tp('fo:font-weight="bold"'))));
  add('heading-styles-map', content(`<text:h text:outline-level="1">a</text:h>`), styles(sty('Heading 1', 'paragraph', tp('fo:font-size="30pt"')) + sty('Heading 4', 'paragraph', tp('fo:font-size="31pt"')) + sty('Heading3', 'paragraph', tp('fo:font-size="32pt"')) + sty('Heading_20_5', 'paragraph', '')));

  // ───── text content ─────
  add('text-s', content(`<text:p>a<text:s/>b<text:s text:c="3"/>c<text:s text:c="0"/>d<text:s text:c="-2"/>e<text:s text:c="x"/>f<text:s text:c=" 4 "/>g<text:s text:c="2.9"/>h<text:s text:c="1e1"/>i<text:s text:c=""/>j</text:p>`));
  add('text-tab-lb', content(`<text:p>a<text:tab/>b<text:line-break/>c<text:tab>ignored text</text:tab></text:p><text:p><text:line-break/><text:tab/></text:p>`));
  add('text-entities', content(`<text:p>a &amp; b &lt; c &gt; &quot;d&quot; &apos;e&apos; &#233; &#x1F600; &#x10FFFF; &nbsp; &unknown; &#0; &#xD7FF; &#xE000;</text:p><text:p><![CDATA[x < y & z]]> tail</text:p>`));
  add('text-entity-surrogate', content(`<text:p>a&#xD800;b&#xDC00;c</text:p>`));
  add('text-entity-bad', content(`<text:p>a&#x110000;b</text:p>`));
  add('text-entity-huge', content(`<text:p>a&#99999999999;b</text:p>`));
  add('text-nonbmp', content(`<text:p>😀 𝄞 é ñ 日本語 \u{10FFFF}</text:p>`));
  add('text-whitespace', content(`<text:p>  lead and  inner   spaces \n newline\t tab\r\ncrlf  </text:p><text:p> </text:p><text:p/><text:p></text:p>`));
  add('text-links', content(`<text:p><text:a xlink:href="http://x.test/a">link <text:span text:style-name="T1">styled</text:span></text:a> after <text:a xlink:href="">empty</text:a><text:a>nohref</text:a><text:a xlink:href="#b"><text:a xlink:href="#c">nested</text:a>tail</text:a></text:p>`, sty('T1', 'text', tp('fo:color="#000000" fo:font-weight="bold"'))));
  add('text-link-color-kept', content(`<text:p><text:span text:style-name="T1"><text:a xlink:href="u">kept</text:a></text:span><text:a xlink:href="u2"><text:span text:style-name="T1">span in link</text:span></text:a></text:p>`, sty('T1', 'text', tp('fo:color="#ff0000"'))));
  add('text-unknown-order', content(`<text:p>before<text:meta>in meta</text:meta>after<text:span>sp</text:span>end<text:date text:style-name="T1">2020<text:meta>deep</text:meta>tail</text:date>last</text:p>`, sty('T1', 'text', tp('fo:font-weight="bold"'))));
  add('text-unknown-nested-deep', content(`<text:p>${'<text:meta>x'.repeat(50)}${'</text:meta>y'.repeat(50)}</text:p>`));
  add('text-span-nested', content(`<text:p><text:span text:style-name="A">a<text:span text:style-name="B">b<text:span text:style-name="C">c</text:span>b2</text:span>a2</text:span></text:p>`, sty('A', 'text', tp('fo:font-weight="bold"')) + sty('B', 'text', tp('fo:font-style="italic"')) + sty('C', 'text', tp('fo:font-weight="normal" fo:color="#00ff00"'))));
  add('text-span-deep', content(`<text:p>${'<text:span text:style-name="A">1'.repeat(200)}${'</text:span>2'.repeat(200)}</text:p>`, sty('A', 'text', tp('fo:font-weight="bold"'))));
  add('text-bookmarks', content(`<text:p>a<text:bookmark text:name="x"/>b<text:bookmark-start text:name="y"/>c<text:bookmark-end text:name="y"/>d<text:soft-page-break/>e<text:annotation-end/>f<office:annotation-end/>g</text:p>`));
  add('text-soft-page-break-block', content(`<text:soft-page-break/><text:p>a</text:p><text:sequence-decls/><text:unknown-block>ignored</text:unknown-block>`));
  add('text-empty-body', content(''));
  add('text-only-ignored', content('<text:soft-page-break/><text:sequence-decls/><draw:frame/>'));
  add('text-section', content(`<text:section text:name="s"><text:p>in</text:p><text:section><text:p>in2</text:p></text:section><table:table><table:table-row><table:table-cell><text:p>t</text:p></table:table-cell></table:table-row></table:table></text:section><text:p>out</text:p>`));
  add('text-section-deep', content(`${'<text:section>'.repeat(100)}<text:p>deep</text:p>${'</text:section>'.repeat(100)}`));
  add('text-note', content(`<text:p>body<text:note text:note-class="footnote" text:id="ftn1"><text:note-citation>1</text:note-citation><text:note-body><text:p>first <text:span text:style-name="T1">styled</text:span></text:p><text:p>second</text:p></text:note-body></text:note> after</text:p><text:p><text:note><text:note-citation>2</text:note-citation></text:note>x<text:note><text:note-body/></text:note></text:p>`, sty('T1', 'text', tp('fo:font-weight="bold"'))));
  add('text-note-link', content(`<text:p><text:a xlink:href="u">l<text:note><text:note-body><text:p>in note</text:p></text:note-body></text:note>m</text:a></text:p>`));
  add('text-note-nested', content(`<text:p><text:note><text:note-body><text:p>a<text:note><text:note-body><text:p>b</text:p></text:note-body></text:note>c</text:p></text:note-body></text:note></text:p>`));
  add('text-annotation', content(`<text:p>a<office:annotation><dc:creator>Ana</dc:creator><dc:date>2020-01-01</dc:date><text:p>first</text:p><text:p>sec<text:span text:style-name="T1">ond</text:span></text:p></office:annotation>b<office:annotation><text:p>no author</text:p></office:annotation>c<office:annotation><dc:creator/></office:annotation>d<office:annotation><dc:creator>é😀</dc:creator><text:p/><text:p>x</text:p></office:annotation></text:p>`, sty('T1', 'text', tp('fo:font-weight="bold"'))));
  add('text-annotation-nested', content(`<text:p><office:annotation><text:p>x<office:annotation><dc:creator>inner</dc:creator><text:p>i</text:p></office:annotation>y</text:p></office:annotation></text:p>`));
  add('text-annotation-in-table', content(`<table:table><table:table-row><table:table-cell><text:p><office:annotation><text:p>c</text:p></office:annotation></text:p></table:table-cell></table:table-row></table:table>`));

  // ───── lists ─────
  const ls = (n, kind) => `<text:list-style style:name="${n}">${[1, 2, 3].map((l) => `<text:list-level-style-${kind} text:level="${l}"/>`).join('')}</text:list-style>`;
  const item = (t, inner = '') => `<text:list-item><text:p>${t}</text:p>${inner}</text:list-item>`;
  add('list-bullet-default', content(`<text:list>${item('a')}${item('b', `<text:list>${item('c')}</text:list>`)}</text:list>`));
  add('list-direct-number', content(`<text:list text:style-name="LN">${item('a')}${item('b', `<text:list>${item('c', `<text:list>${item('d')}</text:list>`)}</text:list>`)}</text:list>`, ls('LN', 'number')));
  add('list-direct-bullet', content(`<text:list text:style-name="LB">${item('a')}</text:list>`, ls('LB', 'bullet')));
  add('list-image', content(`<text:list text:style-name="LI">${item('a')}</text:list>`, ls('LI', 'image')));
  add('list-mixed-levels', content(`<text:list text:style-name="LM">${item('a', `<text:list>${item('b', `<text:list>${item('c', `<text:list>${item('d')}</text:list>`)}</text:list>`)}</text:list>`)}</text:list>`, '<text:list-style style:name="LM"><text:list-level-style-number text:level="1"/><text:list-level-style-bullet text:level="2"/><text:list-level-style-number text:level="3"/></text:list-style>'));
  add('list-level-fallback', content(`<text:list text:style-name="LF">${item('a', `<text:list>${item('b', `<text:list>${item('c')}</text:list>`)}</text:list>`)}</text:list>`, '<text:list-style style:name="LF"><text:list-level-style-number text:level="1"/><text:list-level-style-bullet text:level="3"/></text:list-style>'));
  add('list-level-nan', content(`<text:list text:style-name="LX">${item('a')}</text:list>`, '<text:list-style style:name="LX"><text:list-level-style-number text:level="x"/><text:list-level-style-number/><text:list-level-style-bullet text:level="1.5"/></text:list-style>'));
  add('list-via-para-style', content(`<text:list>${`<text:list-item><text:p text:style-name="Q">a</text:p></text:list-item>`}</text:list><text:list><text:list-item><text:p text:style-name="Q2">b</text:p></text:list-item><text:list-item><text:p text:style-name="Q">c</text:p></text:list-item></text:list>`, sty('Q', 'paragraph', '', ' style:list-style-name="LN"') + sty('Q2', 'paragraph', '') + ls('LN', 'number')));
  add('list-unknown-style', content(`<text:list text:style-name="Nope">${item('a')}</text:list>`));
  add('list-header', content(`<text:list><text:list-header><text:p>header</text:p><text:h>hh</text:h><text:list><text:list-item><text:p>ignored</text:p></text:list-item></text:list></text:list-header>${item('a')}<text:list-header><text:p>h2</text:p></text:list-header></text:list>`));
  add('list-item-order', content(`<text:list><text:list-item><text:list><text:list-item><text:p>deep first</text:p></text:list-item></text:list><text:p>then para</text:p><text:list><text:list-item><text:p>again</text:p></text:list-item></text:list></text:list-item></text:list>`));
  add('list-empty', content(`<text:list/><text:list><text:list-item/></text:list><text:list><text:list-item><text:list/></text:list-item></text:list>`));
  add('list-deep', content(`${'<text:list><text:list-item>'.repeat(30)}<text:p>deep</text:p>${'</text:list-item></text:list>'.repeat(30)}`));
  add('list-deeper-than-limit', content(`${'<text:list><text:list-item>'.repeat(70)}<text:p>deep</text:p>${'</text:list-item></text:list>'.repeat(70)}`));
  add('list-in-cell', content(`<table:table><table:table-row><table:table-cell><text:list>${item('a')}</text:list><text:p>b</text:p></table:table-cell></table:table-row></table:table>`));
  add('list-text-in-item-other', content(`<text:list><text:list-item>loose text<text:soft-page-break/><text:p>p</text:p><table:table><table:table-row><table:table-cell><text:p>ignored</text:p></table:table-cell></table:table-row></table:table></text:list-item></text:list>`));

  // ───── tables ─────
  const cell = (t = 'c', a = '') => `<table:table-cell${a}><text:p>${t}</text:p></table:table-cell>`;
  const row = (cells, a = '') => `<table:table-row${a}>${cells}</table:table-row>`;
  const tbl = (inner, a = '') => `<table:table${a}>${inner}</table:table>`;
  const cols = (n, st = '') => `<table:table-column${st ? ` table:style-name="${st}"` : ''}${n > 1 ? ` table:number-columns-repeated="${n}"` : ''}/>`;
  const tauto = sty('C1', 'table-column', '<style:table-column-properties style:column-width="3cm"/>') + sty('C2', 'table-column', '<style:table-column-properties style:column-width="1in"/>') + sty('C3', 'table-column', '<style:table-column-properties style:column-width="bad"/>')
    + sty('TC1', 'table-cell', '<style:table-cell-properties fo:background-color="#ff0000" fo:border="0.5pt solid #000000"/>') + sty('TC2', 'table-cell', '<style:table-cell-properties fo:border-top="1pt dashed #111111" fo:border-left="2pt solid" fo:background-color="transparent"/>')
    + sty('TC3', 'table-cell', '', ' style:parent-style-name="TC1"') + sty('TC4', 'table-cell', '', ' style:parent-style-name="TC4"') + sty('TC5', 'table-cell', '', ' style:parent-style-name="TC6"') + sty('TC6', 'table-cell', '', ' style:parent-style-name="TC5"') + sty('TC7', 'table-cell', '<style:table-cell-properties fo:border="none" fo:background-color="#abcdef"/>');
  add('table-basic', content(tbl(`${cols(2, 'C1')}${row(`${cell('a')}${cell('b')}`)}${row(`${cell('c')}${cell('d')}`)}`), tauto));
  add('table-no-cols', content(tbl(`${row(`${cell('a')}${cell('b')}${cell('c')}`)}${row(cell('d'))}`)));
  add('table-col-groups', content(tbl(`<table:table-columns>${cols(1, 'C1')}${cols(2, 'C2')}</table:table-columns><table:table-header-columns>${cols(1, 'C3')}</table:table-header-columns>${cols(1)}${row(`${cell('a')}${cell('b')}${cell('c')}${cell('d')}${cell('e')}`)}`), tauto));
  add('table-col-groups-nested', content(tbl(`<table:table-columns><table:table-columns>${cols(1, 'C1')}<table:table-header-columns>${cols(1, 'C2')}</table:table-header-columns></table:table-columns>${cols(1, 'C1')}</table:table-columns>${cols(1, 'C2')}${row(`${cell('a')}${cell('b')}${cell('c')}${cell('d')}`)}`), tauto));
  add('table-repeat', content(tbl(`${cols(3, 'C1')}${row(`${cell('a')}${cell('b')}${cell('c')}`)}`), tauto));
  ['0', '-1', 'x', '', '2.5', '1e1', ' 3'].forEach((r, i) => add(`table-repeat-bad-${i}`, content(tbl(`<table:table-column table:style-name="C1" table:number-columns-repeated="${r}"/>${row(`${cell('a')}${cell('b')}`)}`), tauto)));
  add('table-width-partial', content(tbl(`${cols(1, 'C1')}${cols(1)}${cols(1, 'C3')}${row(`${cell('a')}${cell('b')}${cell('c')}${cell('d')}`)}`), tauto));
  add('table-width-more-cells', content(tbl(`${cols(1, 'C1')}${row(`${cell('a')}${cell('b')}${cell('c')}`)}`), tauto));
  add('table-width-full', content(tbl(`${cols(1, 'C1')}${cols(1, 'C2')}${row(`${cell('a')}${cell('b')}`)}`), tauto));
  add('table-col-style-missing', content(tbl(`${cols(2, 'NoSuch')}${row(`${cell('a')}${cell('b')}`)}`), tauto));
  add('table-span', content(tbl(`${cols(3, 'C1')}${row(`${cell('a', ' table:number-columns-spanned="2"')}<table:covered-table-cell/>${cell('c')}`)}${row(`${cell('d')}${cell('e')}${cell('f')}`)}`), tauto));
  ['0', '-1', 'x', '', '2.5', '1e1', '5', '1e300', '99999999999'].forEach((r, i) => add(`table-span-bad-${i}`, content(tbl(`${cols(2, 'C1')}${row(`${cell('a', ` table:number-columns-spanned="${r}"`)}${cell('b')}`)}`), tauto)));
  add('table-covered-only', content(tbl(`${row('<table:covered-table-cell/><table:covered-table-cell/>')}${row(cell('x'))}`)));
  add('table-empty-rows', content(tbl(`${row('')}<table:table-row/>${row(cell('x'))}`)));
  add('table-empty-table', content(tbl('') + tbl(cols(2)) + tbl(row(''))));
  add('table-header-rows', content(tbl(`${cols(2)}<table:table-header-rows>${row(`${cell('h1')}${cell('h2')}`)}${row(`${cell('h3')}${cell('h4')}`)}<table:table-header-rows>${row(cell('nested ignored'))}</table:table-header-rows></table:table-header-rows>${row(`${cell('a')}${cell('b')}`)}<table:table-row-group>${row(`${cell('g1')}${cell('g2')}`)}<table:table-row-group>${row(cell('g nested ignored'))}</table:table-row-group></table:table-row-group>${row(`${cell('z')}`)}`)));
  add('table-cell-styles', content(tbl(`${cols(1)}${row(`${cell('a', ' table:style-name="TC1"')}${cell('b', ' table:style-name="TC2"')}${cell('c', ' table:style-name="TC3"')}${cell('d', ' table:style-name="TC4"')}${cell('e', ' table:style-name="TC5"')}${cell('f', ' table:style-name="TC7"')}${cell('g', ' table:style-name="Nope"')}${cell('h', ' table:style-name=""')}`)}`), tauto));
  add('table-cell-content', content(tbl(`${row(`<table:table-cell><text:p>p1</text:p><text:h>h</text:h><text:list><text:list-item><text:p>li</text:p></text:list-item></text:list>${tbl(row(cell('inner')))}<text:p>p2</text:p>${tbl(row(cell('inner2')))}<text:section><text:p>sec</text:p></text:section></table:table-cell><table:table-cell/><table:table-cell>stray text</table:table-cell>`)}`)));
  add('table-nested-deep', content(`${'<table:table><table:table-row><table:table-cell>'.repeat(20)}<text:p>deep</text:p>${'</table:table-cell></table:table-row></table:table>'.repeat(20)}`));
  add('table-nested-deeper-than-limit', content(`${'<table:table><table:table-row><table:table-cell>'.repeat(70)}<text:p>deep</text:p>${'</table:table-cell></table:table-row></table:table>'.repeat(70)}`));
  add('table-row-attrs', content(tbl(`${row(cell('a'), ' table:style-name="R1" table:number-rows-repeated="3"')}`)));
  add('table-in-text-p-order', content(`<text:p>a</text:p>${tbl(row(cell('t')))}<text:p>b</text:p>${tbl(row(cell('u')))}${tbl(row(cell('v')))}`));
  add('table-cell-lists-etc', content(tbl(row(`<table:table-cell><draw:frame svg:width="1cm" svg:height="1cm"><draw:image xlink:href="Pictures/a.png"/></draw:frame><text:p><draw:frame svg:width="1cm" svg:height="1cm"><draw:image xlink:href="Pictures/a.png"/></draw:frame>t</text:p></table:table-cell>`))), null, [['Pictures/a.png', PNG]]);

  // ───── images and frames ─────
  const frame = (a, inner) => `<text:p>x<draw:frame ${a}>${inner}</draw:frame>y</text:p>`;
  add('img-basic', content(frame('svg:width="2cm" svg:height="1cm" draw:name="i"', '<draw:image xlink:href="Pictures/a.png" xlink:type="simple"/>')), null, [['Pictures/a.png', PNG]]);
  add('img-dot-slash', content(frame('svg:width="2cm" svg:height="1cm"', '<draw:image xlink:href="./Pictures/a.png"/>')), null, [['Pictures/a.png', PNG]]);
  add('img-dot-slash-twice', content(frame('svg:width="2cm" svg:height="1cm"', '<draw:image xlink:href="././Pictures/a.png"/>')), null, [['././Pictures/a.png', PNG], ['Pictures/a.png', new Uint8Array([1, 2, 3])]]);
  add('img-missing', content(frame('svg:width="2cm" svg:height="1cm"', '<draw:image xlink:href="Pictures/none.png"/>')));
  add('img-ext-forms', content(['a.PNG', 'b.JpG', 'c', 'd.', 'e.tar.gz', 'f.x/y', 'Pictures/g.SVG', 'h.WMF', '.hidden', 'a.b.c.D'].map((n) => frame('svg:width="1cm" svg:height="1cm"', `<draw:image xlink:href="${n}"/>`)).join('')), null, ['a.PNG', 'b.JpG', 'c', 'd.', 'e.tar.gz', 'f.x/y', 'Pictures/g.SVG', 'h.WMF', '.hidden', 'a.b.c.D'].map((n) => [n, PNG]));
  add('img-empty-href', content(frame('svg:width="2cm" svg:height="1cm"', `<draw:image xlink:href=""><office:binary-data>${b64(PNG)}</office:binary-data></draw:image>`)));
  add('img-base64', content(frame('svg:width="2cm" svg:height="1cm"', `<draw:image><office:binary-data>\n  ${b64(PNG).slice(0, 8)}\n\t${b64(PNG).slice(8)}\n</office:binary-data></draw:image>`)));
  add('img-base64-href-found', content(frame('svg:width="2cm" svg:height="1cm"', `<draw:image xlink:href="Pictures/a.png"><office:binary-data>QUJD</office:binary-data></draw:image>`)), null, [['Pictures/a.png', PNG]]);
  add('img-base64-href-missing', content(frame('svg:width="2cm" svg:height="1cm"', `<draw:image xlink:href="Pictures/zz.png"><office:binary-data>QUJD</office:binary-data></draw:image>`)));
  ['QQ==', 'QUI=', 'QUJD', 'QUJDRA', 'QUJDRA==', 'QUJDRA=', 'QUJDR', 'Q', '', '=', '====', 'QQ', 'QQ=', 'QQ===', 'Q=Q=', 'QU JD', 'QUJD====', 'QUJD ', ' QUJD', 'QUJD RA==', 'QUJ*', 'QUJ-', 'QUJ_', 'QUJD=\n=', 'QUJDRAA=', 'AAAA', '////', '++++', 'ñ', 'QUJDé'].forEach((d, i) => {
    add(`img-b64-${i}`, content(frame('svg:width="2cm" svg:height="1cm"', `<draw:image><office:binary-data>${d.replace(/&/g, '&amp;')}</office:binary-data></draw:image>`)));
  });
  add('img-no-data', content(frame('svg:width="2cm" svg:height="1cm"', '<draw:image/>') + frame('svg:width="2cm"', '<draw:image><office:binary-data/></draw:image>')));
  add('img-no-size', content(frame('', '<draw:image xlink:href="Pictures/a.png"/>')), null, [['Pictures/a.png', PNG]]);
  add('img-two-images', content(frame('svg:width="1cm" svg:height="1cm"', '<draw:image xlink:href="Pictures/a.png"/><draw:image xlink:href="Pictures/b.png"/>')), null, [['Pictures/a.png', PNG], ['Pictures/b.png', new Uint8Array([9, 9])]]);
  add('img-same-bytes-twice', content(frame('svg:width="1cm" svg:height="1cm"', '<draw:image xlink:href="Pictures/a.png"/>') + frame('svg:width="2cm" svg:height="2cm"', '<draw:image xlink:href="Pictures/b.png"/>')), null, [['Pictures/a.png', PNG], ['Pictures/b.png', PNG]]);
  add('img-in-link', content(`<text:p><text:a xlink:href="u">a<draw:frame svg:width="1cm" svg:height="1cm"><draw:image xlink:href="Pictures/a.png"/></draw:frame>b</text:a></text:p>`), null, [['Pictures/a.png', PNG]]);
  add('img-frame-text-and-image', content(frame('svg:width="1cm" svg:height="1cm"', '<draw:image xlink:href="Pictures/a.png"/><draw:text-box><text:p>box</text:p></draw:text-box>')), null, [['Pictures/a.png', PNG]]);
  add('img-empty-part', content(frame('svg:width="1cm" svg:height="1cm"', '<draw:image xlink:href="Pictures/a.png"/>')), null, [['Pictures/a.png', new Uint8Array(0)]]);

  // text frames
  const tgs = (name, inner) => sty(name, 'graphic', `<style:graphic-properties ${inner}/>`);
  const tb = (a, g = '', text = '<text:p>box</text:p>', box = '') => ({ body: `<text:p>a<draw:frame ${a}><draw:text-box${box}>${text}</draw:text-box></draw:frame>b</text:p>`, auto: g });
  const frameCases = [
    ['as-char', 'text:anchor-type="as-char" svg:width="3cm" svg:height="2cm"', ''],
    ['paragraph', 'text:anchor-type="paragraph" svg:width="3cm" svg:height="2cm" svg:x="1cm" svg:y="2cm"', ''],
    ['page', 'text:anchor-type="page" svg:width="3cm" svg:height="2cm" svg:x="1cm" svg:y="2cm" draw:z-index="5"', ''],
    ['no-anchor', 'svg:width="3cm" svg:height="2cm" svg:x="1cm" svg:y="2cm" draw:z-index="-3"', ''],
    ['char-anchor', 'text:anchor-type="char" svg:width="3cm" svg:x="1cm"', ''],
    ['to-paragraph', 'text:anchor-type="to-paragraph" svg:width="3cm"', ''],
    ['wrap-none', 'draw:style-name="G" svg:width="3cm"', tgs('G', 'style:wrap="none"')],
    ['wrap-parallel', 'draw:style-name="G" svg:width="3cm"', tgs('G', 'style:wrap="parallel"')],
    ['wrap-left', 'draw:style-name="G" svg:width="3cm"', tgs('G', 'style:wrap="left"')],
    ['wrap-right', 'draw:style-name="G" svg:width="3cm"', tgs('G', 'style:wrap="right"')],
    ['wrap-biggest', 'draw:style-name="G" svg:width="3cm"', tgs('G', 'style:wrap="biggest"')],
    ['wrap-dynamic', 'draw:style-name="G" svg:width="3cm"', tgs('G', 'style:wrap="dynamic"')],
    ['wrap-foo', 'draw:style-name="G" svg:width="3cm"', tgs('G', 'style:wrap="foo"')],
    ['wrap-runthrough', 'draw:style-name="G" svg:width="3cm"', tgs('G', 'style:wrap="run-through"')],
    ['wrap-runthrough-bg', 'draw:style-name="G" svg:width="3cm"', tgs('G', 'style:wrap="run-through" style:run-through="background"')],
    ['wrap-runthrough-fg', 'draw:style-name="G" svg:width="3cm"', tgs('G', 'style:wrap="run-through" style:run-through="foreground"')],
    ['wrap-none-bg', 'draw:style-name="G" svg:width="3cm"', tgs('G', 'style:wrap="none" style:run-through="background"')],
    ['hrel-page', 'draw:style-name="G" svg:width="3cm" svg:x="1cm"', tgs('G', 'style:horizontal-rel="page" style:vertical-rel="page"')],
    ['hrel-page-content', 'draw:style-name="G" svg:width="3cm" svg:x="1cm"', tgs('G', 'style:horizontal-rel="page-content" style:vertical-rel="page-content"')],
    ['hrel-char', 'draw:style-name="G" svg:width="3cm" svg:x="1cm"', tgs('G', 'style:horizontal-rel="char" style:vertical-rel="char"')],
    ['vrel-line', 'draw:style-name="G" svg:width="3cm" svg:x="1cm"', tgs('G', 'style:vertical-rel="line"')],
    ['rel-foo', 'draw:style-name="G" svg:width="3cm"', tgs('G', 'style:horizontal-rel="foo" style:vertical-rel="bar"')],
    ['rel-paragraph', 'draw:style-name="G" svg:width="3cm"', tgs('G', 'style:horizontal-rel="paragraph" style:vertical-rel="paragraph"')],
    ['pos-center', 'draw:style-name="G" svg:width="3cm" svg:x="1cm"', tgs('G', 'style:horizontal-pos="center" style:vertical-pos="middle"')],
    ['pos-right-bottom', 'draw:style-name="G" svg:width="3cm" svg:x="1cm"', tgs('G', 'style:horizontal-pos="right" style:vertical-pos="bottom"')],
    ['pos-left-top', 'draw:style-name="G" svg:width="3cm" svg:x="1cm"', tgs('G', 'style:horizontal-pos="left" style:vertical-pos="top"')],
    ['pos-from', 'draw:style-name="G" svg:width="3cm" svg:x="1cm" svg:y="2cm"', tgs('G', 'style:horizontal-pos="from-left" style:vertical-pos="from-top"')],
    ['pos-inside', 'draw:style-name="G" svg:width="3cm" svg:x="1cm"', tgs('G', 'style:horizontal-pos="inside" style:vertical-pos="below"')],
    ['pos-empty', 'draw:style-name="G" svg:width="3cm" svg:x="1cm" svg:y="2cm"', tgs('G', 'style:horizontal-pos="" style:vertical-pos=""')],
    ['padding', 'draw:style-name="G" svg:width="3cm"', tgs('G', 'fo:padding="0.2cm" fo:padding-top="0.4cm" fo:padding-left="bad"')],
    ['padding-sides', 'draw:style-name="G" svg:width="3cm"', tgs('G', 'fo:padding-left="1mm" fo:padding-right="2mm" fo:padding-top="3mm" fo:padding-bottom="4mm"')],
    ['style-missing', 'draw:style-name="Nope" svg:width="3cm"', ''],
    ['style-not-graphic', 'draw:style-name="P" svg:width="3cm"', sty('P', 'paragraph', tp('fo:font-size="9pt"'))],
    ['no-size-box-min', 'text:anchor-type="paragraph"', '', ' fo:min-width="4cm" fo:min-height="1cm"'],
    ['size-bad-no-fallback', 'svg:width="bad" svg:height="bad"', '', ' fo:min-width="4cm" fo:min-height="1cm"'],
    ['size-and-min', 'svg:width="2cm" svg:height="2cm"', '', ' fo:min-width="4cm" fo:min-height="1cm"'],
    ['z-bad', 'svg:width="1cm" draw:z-index="abc"', ''],
    ['z-zero', 'svg:width="1cm" draw:z-index="0"', ''],
    ['z-neg-zero', 'svg:width="1cm" draw:z-index="-0"', ''],
    ['z-float', 'svg:width="1cm" draw:z-index="2.9"', ''],
    ['z-big', 'svg:width="1cm" draw:z-index="99999999999999999999"', ''],
  ];
  frameCases.forEach(([name, a, g, box = ''], i) => {
    const r = tb(a, g, '<text:p>box</text:p>', box);
    add(`frame-${i}-${name}`, content(r.body, r.auto));
  });
  add('frame-content-rich', content(`<text:p><draw:frame svg:width="3cm" svg:height="2cm"><draw:text-box><text:p text:style-name="P1">one</text:p><text:h text:outline-level="2">h</text:h><text:list><text:list-item><text:p>li</text:p></text:list-item></text:list><table:table><table:table-row><table:table-cell><text:p>tc</text:p></table:table-cell></table:table-row></table:table><text:p>two</text:p></draw:text-box></draw:frame></text:p>`, sty('P1', 'paragraph', tp('fo:font-size="12pt"'))));
  add('frame-empty-box', content(`<text:p><draw:frame svg:width="3cm" svg:height="2cm"><draw:text-box/></draw:frame></text:p><text:p><draw:frame><draw:text-box></draw:text-box></draw:frame></text:p>`));
  add('frame-no-box-text-inline', content(`<text:p>a<draw:frame svg:width="3cm"><text:p>inline text</text:p>loose<draw:frame><draw:text-box><text:p>inner box</text:p></draw:text-box></draw:frame></draw:frame>b</text:p>`));
  add('frame-no-box-in-link', content(`<text:p><text:a xlink:href="u">a<draw:frame><draw:object/>txt</draw:frame>b</text:a></text:p>`));
  add('frame-nested', content(`<text:p><draw:frame svg:width="9cm"><draw:text-box><text:p>outer<draw:frame svg:width="5cm"><draw:text-box><text:p>inner<draw:frame svg:width="2cm"><draw:text-box><text:p>innermost</text:p></draw:text-box></draw:frame></text:p></draw:text-box></draw:frame></text:p></draw:text-box></draw:frame></text:p>`));
  add('frame-nested-deeper-than-limit', content(`<text:p>${'<draw:frame svg:width="9cm"><draw:text-box><text:p>t'.repeat(70)}${'</text:p></draw:text-box></draw:frame>'.repeat(70)}</text:p>`));
  add('frame-with-annotation-and-image', content(`<text:p><draw:frame svg:width="9cm"><draw:text-box><text:p><office:annotation><text:p>note</text:p></office:annotation><draw:frame svg:width="1cm" svg:height="1cm"><draw:image xlink:href="Pictures/a.png"/></draw:frame></text:p></draw:text-box></draw:frame></text:p>`), null, [['Pictures/a.png', PNG]]);
  add('frame-twice-in-para', content(`<text:p>a<draw:frame svg:width="3cm"><draw:text-box><text:p>1</text:p></draw:text-box></draw:frame>b<draw:frame svg:width="4cm"><draw:text-box><text:p>2</text:p></draw:text-box></draw:frame>c</text:p>`));

  // ───── page geometry ─────
  const pl = (inner, where = 'auto') => (where === 'auto' ? [null, styles('', `<style:page-layout style:name="pm1">${inner}</style:page-layout>`)] : where === 'styles-named' ? [null, styles(`<style:page-layout style:name="pm1">${inner}</style:page-layout>`)] : [content('<text:p>x</text:p>', `<style:page-layout style:name="pm1">${inner}</style:page-layout>`), null]);
  const plp = (a) => `<style:page-layout-properties ${a}/>`;
  const pageCases = [
    ['a4', plp('fo:page-width="21cm" fo:page-height="29.7cm" fo:margin-left="2cm" fo:margin-right="2cm" fo:margin-top="2cm" fo:margin-bottom="2cm"')],
    ['letter', plp('fo:page-width="8.5in" fo:page-height="11in" fo:margin="1in"')], ['partial', plp('fo:page-width="10cm"')], ['empty-props', plp('')], ['no-props', ''], ['bad', plp('fo:page-width="wide" fo:margin-left="x"')],
    ['two-layouts', plp('fo:page-width="1cm"') + '</style:page-layout><style:page-layout style:name="pm2">' + plp('fo:page-width="2cm"')], ['two-props', plp('fo:page-width="1cm"') + plp('fo:page-width="2cm"')],
    ['neg', plp('fo:page-width="-1cm" fo:margin-left="-2cm"')], ['px', plp('fo:page-width="800px" fo:page-height="1200px" fo:margin-left="0px"')], ['zero', plp('fo:page-width="0cm" fo:page-height="0cm" fo:margin-left="0cm" fo:margin-top="0cm"')],
  ];
  pageCases.forEach(([name, inner], i) => {
    for (const where of ['auto', 'styles-named', 'content']) {
      const [c, s] = pl(inner, where);
      add(`page-${i}-${name}-${where}`, c ?? content('<text:p>x</text:p>'), s);
    }
  });
  add('page-both', content('<text:p>x</text:p>', `<style:page-layout style:name="c">${plp('fo:page-width="1cm"')}</style:page-layout>`), styles('', `<style:page-layout style:name="s">${plp('fo:page-width="2cm"')}</style:page-layout>`));
  add('page-styles-without-layout-content-has', content('<text:p>x</text:p>', `<style:page-layout style:name="c">${plp('fo:page-width="1cm" fo:margin-top="3cm"')}</style:page-layout>`), styles(''));
  add('page-styles-layout-no-props-content-has', content('<text:p>x</text:p>', `<style:page-layout style:name="c">${plp('fo:page-width="1cm"')}</style:page-layout>`), styles('', '<style:page-layout style:name="s"/>'));

  // ───── namespaces, prefixes, structure, packages ─────
  add('ns-other-prefix', `${PROLOG}<o:document-content xmlns:o="urn:oasis:names:tc:opendocument:xmlns:office:1.0" xmlns:t="urn:oasis:names:tc:opendocument:xmlns:text:1.0"><o:body><o:text><t:p>other prefixes</t:p></o:text></o:body></o:document-content>`, undefined);
  add('ns-default-namespace', `${PROLOG}<document-content xmlns="urn:oasis:names:tc:opendocument:xmlns:office:1.0"><body><text><p>default ns</p></text></body></document-content>`, undefined);
  add('ns-word-prefix-rename', `${PROLOG}<office:document-content xmlns:office="urn:oasis:names:tc:opendocument:xmlns:office:1.0" xmlns:text="urn:oasis:names:tc:opendocument:xmlns:text:1.0" xmlns:x="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><office:body><office:text><text:p>a<x:t>word-ns</x:t></text:p></office:text></office:body></office:document-content>`, undefined);
  add('ns-text-bound-to-word', `${PROLOG}<office:document-content xmlns:office="urn:oasis:names:tc:opendocument:xmlns:office:1.0" xmlns:text="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><office:body><office:text><text:p>x</text:p></office:text></office:body></office:document-content>`, undefined);
  add('ns-no-declarations', `${PROLOG}<office:document-content><office:body><office:text><text:p>no declarations</text:p></office:text></office:body></office:document-content>`, undefined);
  add('ns-root-other-name', `${PROLOG}<root><office:body><office:text><text:p>root</text:p></office:text></office:body></root>`, undefined);
  add('xml-bom-prolog', `﻿${PROLOG}\n<!-- c --><!DOCTYPE x [<!ENTITY a "b">]>${content('<text:p>bom &amp;a;</text:p>').slice(PROLOG.length)}`, undefined);
  add('xml-pi-comments', content('<text:p>a<?pi data?>b<!-- comment -->c</text:p><!-- between --><text:p>d</text:p>'));
  add('xml-single-quotes', `${PROLOG}<office:document-content xmlns:office='urn:oasis:names:tc:opendocument:xmlns:office:1.0' xmlns:text='urn:oasis:names:tc:opendocument:xmlns:text:1.0'><office:body><office:text><text:p text:style-name='P1'>single</text:p></office:text></office:body></office:document-content>`, styles(sty('P1', 'paragraph', tp('fo:font-weight="bold"'))));
  add('xml-attr-dup', content('<text:p text:style-name="A" text:style-name="B">dup</text:p>', sty('A', 'paragraph', tp('fo:font-weight="bold"')) + sty('B', 'paragraph', tp('fo:font-style="italic"'))));
  add('xml-bad-unclosed', `${PROLOG}<office:document-content ${NS}><office:body><office:text><text:p>unclosed</office:text></office:body></office:document-content>`, undefined);
  add('xml-bad-mismatch', content('<text:p>a</text:span>'), undefined);
  add('xml-bad-attr', content('<text:p text:style-name=P1>a</text:p>'), undefined);
  add('xml-bad-attr-no-eq', content('<text:p text:style-name>a</text:p>'), undefined);
  add('xml-bad-empty', '', undefined);
  add('xml-bad-text-only', 'not xml', undefined);
  add('xml-bad-lt-in-attr', content('<text:p text:style-name="a<b">a</text:p>'), undefined);
  add('xml-styles-bad', content('<text:p>x</text:p>'), '<office:document-styles><office:styles>');
  add('xml-styles-empty', content('<text:p>x</text:p>'), '');
  add('xml-styles-other-root', content('<text:p>x</text:p>'), '<foo/>');
  add('xml-deep-1000', `${PROLOG}<office:document-content ${NS}><office:body><office:text><text:p>${'<text:meta>'.repeat(990)}x${'</text:meta>'.repeat(990)}</text:p></office:text></office:body></office:document-content>`, undefined);
  add('xml-deep-1100', `${PROLOG}<office:document-content ${NS}><office:body><office:text><text:p>${'<text:meta>'.repeat(1100)}x${'</text:meta>'.repeat(1100)}</text:p></office:text></office:body></office:document-content>`, undefined);
  add('struct-no-body', `${PROLOG}<office:document-content ${NS}><office:automatic-styles/></office:document-content>`, undefined);
  add('struct-no-text', `${PROLOG}<office:document-content ${NS}><office:body><office:spreadsheet/></office:body></office:document-content>`, undefined);
  add('struct-two-bodies', `${PROLOG}<office:document-content ${NS}><office:body><office:text><text:p>first</text:p></office:text></office:body><office:body><office:text><text:p>second</text:p></office:text></office:body></office:document-content>`, undefined);
  add('struct-two-texts', `${PROLOG}<office:document-content ${NS}><office:body><office:text><text:p>first</text:p></office:text><office:text><text:p>second</text:p></office:text></office:body></office:document-content>`, undefined);
  add('struct-text-in-text', `${PROLOG}<office:document-content ${NS}><office:body><office:text><office:text><text:p>nested text</text:p></office:text></office:text></office:body></office:document-content>`, undefined);
  add('struct-body-whitespace', `${PROLOG}<office:document-content ${NS}>\n  <office:body>\n    <office:text>\n      <text:p>ws</text:p>\n    </office:text>\n  </office:body>\n</office:document-content>`, undefined);
  add('struct-extra-parts', content('<text:p>x</text:p>'), styles(''), [['meta.xml', '<meta/>'], ['settings.xml', '<s/>'], ['Thumbnails/thumbnail.png', PNG], ['Configurations2/x', 'y']]);
  add('struct-no-mimetype', content('<text:p>x</text:p>'));
  out[out.length - 1].parts.shift();
  add('struct-wrong-mimetype', content('<text:p>x</text:p>'));
  out[out.length - 1].parts[0][1] = 'application/vnd.oasis.opendocument.spreadsheet';
  add('struct-no-content', content('<text:p>x</text:p>'));
  out[out.length - 1].parts = out[out.length - 1].parts.filter(([n]) => n !== 'content.xml');
  add('struct-content-wrong-case', content('<text:p>x</text:p>'));
  out[out.length - 1].parts[1][0] = 'Content.xml';
  add('struct-content-in-dir', content('<text:p>x</text:p>'));
  out[out.length - 1].parts[1][0] = 'sub/content.xml';
  add('struct-content-twice', content('<text:p>first</text:p>'));
  out[out.length - 1].parts.push(['content.xml', content('<text:p>second</text:p>')]);
  add('struct-content-bom', content('<text:p>bom</text:p>'));
  out[out.length - 1].parts[1][1] = '﻿' + out[out.length - 1].parts[1][1];
  add('struct-content-invalid-utf8', content('<text:p>x</text:p>'));
  out[out.length - 1].parts[1][1] = new Uint8Array([...Buffer.from(content('<text:p>a')), 0xff, 0xc3, 0x28, ...Buffer.from('</text:p></office:text></office:body></office:document-content>')]);
  add('struct-content-utf16', content('<text:p>x</text:p>'));
  out[out.length - 1].parts[1][1] = Buffer.from('﻿' + content('<text:p>utf16</text:p>'), 'utf16le');
  add('struct-big-text', content(`<text:p>${'lorem ipsum '.repeat(20000)}</text:p>`));
  add('struct-many-paragraphs', content(Array.from({ length: 3000 }, (_, i) => `<text:p text:style-name="P${i % 7}">p${i}<text:span text:style-name="T${i % 5}">s</text:span></text:p>`).join(''), Array.from({ length: 7 }, (_, i) => sty(`P${i}`, 'paragraph', pp(`fo:margin-left="${i}mm"`))).join('') + Array.from({ length: 5 }, (_, i) => sty(`T${i}`, 'text', tp(`fo:font-size="${8 + i}pt"`))).join('')));

  // zip-level damage
  const good = odt('zip-base', content('<text:p>zip</text:p>'), styles(''));
  out.push({ key: 'quirk/odf/zip-empty.odt', raw: new Uint8Array(0) });
  out.push({ key: 'quirk/odf/zip-garbage.odt', raw: new Uint8Array([1, 2, 3, 4, 5, 6, 7, 8, 9, 10]) });
  out.push({ key: 'quirk/odf/zip-pk-only.odt', raw: new Uint8Array([0x50, 0x4b, 0x03, 0x04]) });
  out.push({ key: 'quirk/odf/zip-eocd-only.odt', raw: new Uint8Array([0x50, 0x4b, 0x05, 0x06, ...new Array(18).fill(0)]) });
  out.push({ key: 'quirk/odf/zip-text.odt', raw: new TextEncoder().encode('PK not really a zip file at all, just text') });
  out.push({ ...good, key: 'quirk/odf/zip-cut-end.odt', cut: 20 });
  out.push({ ...good, key: 'quirk/odf/zip-cut-half.odt', cutTo: 0.5 });
  out.push({ ...good, key: 'quirk/odf/zip-flip-content.odt', flip: 'content.xml' });
  out.push({ ...good, key: 'quirk/odf/zip-flip-unrelated.odt', flip: 'META-INF/manifest.xml', extraDeflated: true });
  return out;
}

// ───────────────────────── seeded random documents ─────────────────────────

class Gen {
  constructor(seed) { this.r = new Rng(seed); }
  below(n) { return this.r.below(n); }
  pick(a) { return a[this.r.below(a.length)]; }
  chance(n) { return this.r.below(n) === 0; }
}

const WORDS = ['alpha', 'béta', 'γάμμα', '日本', '😀', 'x & y', '<tag>', 'a  b', 'tab\tx', '"q"', "it's", '', ' ', ' ', 'ﬁ', 'İstanbul'];
const FAMILIES = ['paragraph', 'text', 'table-cell', 'table-column', 'graphic', 'table', 'section', 'foo'];
const COLORS = ['#ff0000', '#00F', 'red', '#abcdef', 'transparent', '', '#AbCdEf', 'ß', '#12345'];

function randomDoc(seed) {
  const g = new Gen(seed);
  const nstyles = 3 + g.below(12);
  const names = ['Standard', 'Text_20_body', 'Heading_20_1', ...Array.from({ length: nstyles }, (_, i) => `S${i}`), 'P1', 'T1', 'C1', 'TC1', 'G1', 'L1', 'L2'];
  const length = () => g.pick(LENGTHS.concat(Array.from({ length: 10 }, () => `${g.below(1000) / 10}${g.pick(['cm', 'mm', 'in', 'pt', 'pc', 'px'])}`)));
  const cyc = g.chance(30);
  const styleXml = (name, i) => {
    const family = i < 4 ? 'paragraph' : g.pick(FAMILIES);
    const parent = i > 0 && g.chance(2) ? ` style:parent-style-name="${names[g.below(i)]}"` : cyc && g.chance(4) ? ` style:parent-style-name="${g.pick(names)}"` : '';
    const lsn = g.chance(8) ? ` style:list-style-name="${g.pick(['L1', 'L2', 'none'])}"` : '';
    let inner = '';
    if (g.chance(2)) inner += tp([g.chance(2) ? `fo:font-weight="${g.pick(['bold', 'normal', '700', 'x'])}"` : '', g.chance(3) ? `fo:font-style="${g.pick(['italic', 'oblique', 'normal'])}"` : '', g.chance(3) ? `fo:color="${g.pick(COLORS)}"` : '', g.chance(3) ? `fo:font-size="${length()}"` : '', g.chance(5) ? `style:font-name="${g.pick(['Arial', `'X Y'`, 'A,B', '', '"q"'].map((x) => x.replace(/"/g, '&quot;')))}"` : '', g.chance(6) ? `style:text-position="${g.pick(['super 58%', 'sub 58%', '33%', '-10%', 'superb', ''])}"` : '', g.chance(6) ? `style:text-underline-style="${g.pick(['solid', 'none'])}"` : '', g.chance(8) ? 'fo:font-variant="small-caps"' : '', g.chance(8) ? 'fo:text-transform="uppercase"' : ''].filter(Boolean).join(' '));
    if (g.chance(2)) inner += pp([g.chance(2) ? `fo:text-align="${g.pick(['start', 'end', 'center', 'justify', 'left', 'right', 'x'])}"` : '', g.chance(2) ? `fo:margin-left="${length()}"` : '', g.chance(3) ? `fo:text-indent="${length()}"` : '', g.chance(3) ? `fo:margin-top="${length()}" fo:margin-bottom="${length()}"` : '', g.chance(4) ? `fo:border="${g.pick(['0.5pt solid #000000', '1pt dashed', 'none', 'solid #fff', '2pt double red', '  ', '1pt'])}"` : '', g.chance(5) ? `fo:border-top="${g.pick(['1pt solid #111111', 'none'])}"` : '', g.chance(5) ? 'fo:break-before="page"' : '', g.chance(5) ? `fo:background-color="${g.pick(COLORS)}"` : ''].filter(Boolean).join(' '));
    if (family === 'table-cell' && g.chance(2)) inner += `<style:table-cell-properties fo:border="${g.pick(['0.5pt solid #000000', '1pt solid', 'none'])}" fo:background-color="${g.pick(COLORS)}"/>`;
    if (family === 'table-column' && g.chance(1)) inner += `<style:table-column-properties style:column-width="${length()}"/>`;
    if (family === 'graphic') inner += `<style:graphic-properties ${[g.chance(2) ? `style:wrap="${g.pick(['none', 'parallel', 'left', 'right', 'biggest', 'run-through', 'dynamic'])}"` : '', g.chance(3) ? `style:horizontal-rel="${g.pick(['page', 'page-content', 'char', 'paragraph'])}"` : '', g.chance(3) ? `style:vertical-rel="${g.pick(['page', 'page-content', 'line', 'char', 'paragraph'])}"` : '', g.chance(3) ? `style:horizontal-pos="${g.pick(['center', 'right', 'left', 'from-left'])}"` : '', g.chance(3) ? `style:vertical-pos="${g.pick(['middle', 'bottom', 'top', 'from-top'])}"` : '', g.chance(3) ? `fo:padding="${length()}"` : '', g.chance(4) ? 'style:run-through="background"' : ''].filter(Boolean).join(' ')}/>`;
    return sty(name, family, inner, `${parent}${lsn}`);
  };
  const lstyles = ['L1', 'L2'].map((n) => `<text:list-style style:name="${n}">${[1, 2, 3, 4].map((l) => g.chance(2) ? `<text:list-level-style-${g.pick(['bullet', 'number', 'image'])} text:level="${g.chance(10) ? g.pick(['x', '0', '1.5']) : l}"/>` : '').join('')}</text:list-style>`).join('');
  let auto = '';
  let named = '';
  for (let i = 0; i < names.length; i++) {
    const w = g.below(3);
    if (w === 0) auto += styleXml(names[i], i);
    else if (w === 1) named += styleXml(names[i], i);
  }
  if (g.chance(3)) named += `<style:default-style style:family="${g.pick(['paragraph', 'text'])}">${tp(`fo:font-size="${length()}" fo:font-weight="bold"`)}</style:default-style>`;
  const sname = () => (g.chance(6) ? '' : ` text:style-name="${g.pick(names)}"`);

  const text = () => g.pick(WORDS).replace(/&/g, '&amp;').replace(/</g, '&lt;');
  const inline = (depth) => {
    let s = '';
    const n = g.below(5);
    for (let i = 0; i < n; i++) {
      const k = g.below(20);
      if (k < 7) s += text();
      else if (k < 9 && depth < 4) s += `<text:span${sname()}>${inline(depth + 1)}</text:span>`;
      else if (k === 9 && depth < 4) s += `<text:a xlink:href="${g.pick(['http://x.test', '', '#a'])}">${inline(depth + 1)}</text:a>`;
      else if (k === 10) s += `<text:s text:c="${g.pick(['1', '2', '5', '0', 'x', ''])}"/>`;
      else if (k === 11) s += '<text:tab/>';
      else if (k === 12) s += '<text:line-break/>';
      else if (k === 13 && depth < 3) s += `<text:meta>${inline(depth + 1)}</text:meta>`;
      else if (k === 14) s += `<office:annotation><dc:creator>${text()}</dc:creator><text:p>${text()}</text:p></office:annotation>`;
      else if (k === 15 && depth < 3) s += `<text:note><text:note-body><text:p>${inline(depth + 1)}</text:p></text:note-body></text:note>`;
      else if (k === 16) s += '<text:bookmark text:name="b"/>';
      else if (k === 17) s += `<draw:frame svg:width="${length()}" svg:height="${length()}"><draw:image xlink:href="${g.pick(['Pictures/a.png', 'Pictures/none.png', './Pictures/a.png'])}"/></draw:frame>`;
      else if (k === 18 && depth < 3) s += `<draw:frame${g.chance(2) ? ` draw:style-name="${g.pick(names)}"` : ''} ${g.pick(['text:anchor-type="as-char"', 'text:anchor-type="page"', 'text:anchor-type="paragraph"', ''])} svg:width="${length()}" svg:height="${length()}" svg:x="${length()}" svg:y="${length()}"><draw:text-box>${blocks(depth + 2)}</draw:text-box></draw:frame>`;
      else s += text();
    }
    return s;
  };
  const cell = (depth) => `<table:table-cell${g.chance(3) ? ` table:style-name="${g.pick(names)}"` : ''}${g.chance(5) ? ` table:number-columns-spanned="${g.pick(['2', '3', '1', '0', 'x'])}"` : ''}>${g.chance(6) ? '' : blocks(depth + 1)}</table:table-cell>`;
  const table = (depth) => {
    let s = '<table:table>';
    const nc = g.below(4);
    for (let i = 0; i < nc; i++) s += `<table:table-column${g.chance(2) ? ` table:style-name="${g.pick(names)}"` : ''}${g.chance(3) ? ` table:number-columns-repeated="${g.pick(['2', '3', 'x', '0'])}"` : ''}/>`;
    const nr = g.below(4);
    for (let i = 0; i < nr; i++) {
      let r = '';
      const ncell = g.below(4);
      for (let j = 0; j < ncell; j++) r += g.chance(7) ? '<table:covered-table-cell/>' : cell(depth);
      const row = `<table:table-row>${r}</table:table-row>`;
      s += g.chance(6) ? `<table:table-header-rows>${row}</table:table-header-rows>` : row;
    }
    return `${s}</table:table>`;
  };
  const list = (depth) => {
    let s = `<text:list${g.chance(2) ? ` text:style-name="${g.pick(['L1', 'L2', 'nope'])}"` : ''}>`;
    const n = g.below(4);
    for (let i = 0; i < n; i++) {
      s += `<text:list-item>${g.chance(5) ? '' : `<text:p${sname()}>${inline(2)}</text:p>`}${g.chance(3) && depth < 5 ? list(depth + 1) : ''}</text:list-item>`;
    }
    if (g.chance(8)) s += `<text:list-header><text:p>${text()}</text:p></text:list-header>`;
    return `${s}</text:list>`;
  };
  const blocks = (depth) => {
    let s = '';
    const n = depth > 3 ? 1 : 1 + g.below(5);
    for (let i = 0; i < n; i++) {
      const k = g.below(12);
      if (k < 5) s += `<text:p${sname()}>${inline(0)}</text:p>`;
      else if (k < 7) s += `<text:h text:outline-level="${g.pick(['1', '2', '3', '6', '7', 'x', '0'])}"${sname()}>${inline(1)}</text:h>`;
      else if (k < 9 && depth < 4) s += list(depth);
      else if (k < 11 && depth < 4) s += table(depth);
      else if (k === 11 && depth < 3) s += `<text:section>${blocks(depth + 1)}</text:section>`;
      else s += `<text:p>${inline(0)}</text:p>`;
    }
    return s;
  };
  const body = blocks(0);
  const page = g.chance(2) ? `<style:page-layout style:name="pm"><style:page-layout-properties fo:page-width="${length()}" fo:page-height="${length()}" fo:margin-left="${length()}" fo:margin-top="${length()}"/></style:page-layout>` : '';
  const hasStyles = g.chance(5) !== 0;
  const c = content(body, auto + lstyles);
  return odt(`seeded-${seed.toString(16)}`, c, hasStyles ? styles(named, page) : null, [['Pictures/a.png', PNG]]);
}

export function quirkOdf(seed = QUIRK_ODF_SEED, count = QUIRK_ODF_SEEDED) {
  const out = hand();
  const r = new Rng(seed);
  for (let i = 0; i < count; i++) out.push(randomDoc(r.next()));
  return out;
}

const hexOf = (u8) => Buffer.from(u8).toString('hex');
/** The text of tests/parity/odf.inputs: `@input <key>` then `<name>\t<escaped text>` (or `<name>\t@hex:<hex>` for bytes, `@raw hex` for a whole zip). */
export function inputsText(inputs, zipOf) {
  const esc = (x) => x.replace(/\\/g, '\\\\').replace(/\n/g, '\\n').replace(/\r/g, '\\r').replace(/\t/g, '\\t');
  const lines = ['# uo-docx D5b synthetic .odt packages (js/quirk-odf.mjs): `@input <key>`, then `<part>\\t<escaped text>` or `<part>\\t@hex:<bytes>` per part, in order; `@raw <hex>` is a whole file (zip-level damage)'];
  for (const c of inputs) {
    lines.push(`@input ${c.key}`);
    if (c.raw) { lines.push(`@raw ${hexOf(c.raw)}`); continue; }
    if (c.cut || c.cutTo || c.flip) { lines.push(`@raw ${hexOf(zipOf(c))}`); continue; }
    for (const [name, data] of c.parts) lines.push(typeof data === 'string' ? `${name}\t${esc(data)}` : `${name}\t@hex:${hexOf(data)}`);
  }
  return lines.join('\n') + '\n';
}
