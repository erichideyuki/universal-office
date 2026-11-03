// The regex sites of the D1b font-embedding step (shared/fonts/src/ooxml.ts, docx side, and the
// app-neutral helpers the pptx writer will share) against V8, checked by tests/regex_d1b_fonts.rs
// over the matchers of src/write/fonts. The regexes are copied verbatim from the frozen TS; the
// composite rows (the chained `unescAttr`/`escAttr`, `findElement` + `elementEnd`, the removal loop,
// the settings root edit, a string-pattern `replace` with `$` patterns) run the TS code copied as
// written, through an object whose `Symbol.replace` the `replace` row calls. Positions never leave
// a row (UTF-16 indices would differ from Rust's byte offsets): rows return strings.
import { Rng } from '../../../uo-testkit/js/testkit.mjs';

const REL = 'http://schemas.openxmlformats.org/officeDocument/2006/relationships';

// ooxml.ts:43–47, 132–146 (verbatim)
const escAttr = (s) => s.replace(/&/g, '&amp;').replace(/</g, '&lt;').replace(/>/g, '&gt;').replace(/"/g, '&quot;');
const unescAttr = (s) => s.replace(/&quot;/g, '"').replace(/&lt;/g, '<').replace(/&gt;/g, '>').replace(/&apos;/g, "'").replace(/&amp;/g, '&');
const escRe = (s) => s.replace(/[.*+?^${}()|[\]\\]/g, '\\$&');
function elementEnd(xml, start, qname) {
  const gt = xml.indexOf('>', start);
  if (gt < 0) return -1;
  if (xml[gt - 1] === '/') return gt + 1;
  const close = xml.indexOf(`</${qname}>`, gt);
  return close < 0 ? -1 : close + qname.length + 3;
}
function findElement(xml, qname) {
  const m = new RegExp(`<${escRe(qname)}(?=[\\s/>])`).exec(xml);
  if (!m) return null;
  const end = elementEnd(xml, m.index, qname);
  return end < 0 ? null : { start: m.index, end };
}

/** A `replace` row running `f(input)` (the `Symbol.replace` protocol of `String.prototype.replace`). */
const composite = (f) => [{ [Symbol.replace]: (s) => f(s) }, ''];

const findRow = (q) => composite((x) => { const el = findElement(x, q); return el ? JSON.stringify([x.slice(0, el.start), x.slice(el.start, el.end)]) : 'null'; });
const removeRow = (q) => composite((x) => { for (let el = findElement(x, q); el; el = findElement(x, q)) x = x.slice(0, el.start) + x.slice(el.end); return x; });
const onRow = (n) => new RegExp(`<w:${n}(?![^>]*w:val="(?:false|0|off)")[\\s/>]`);

/** Families of the `writeDocxEmbedding` element regex (case folding of `i` without `u`, escapes, `$`). */
export const FAMILIES = [
  'Calibri', 'calibri', 'CALIBRI', 'A&B', 'a&b', 'Q"x', 'a<b>c', 'Über', 'über', 'ÜBER', 'ſ', 's', 'S', 'K', 'k', 'K',
  'ß', 'SS', 'İ', 'i', 'ı', 'Σ', 'σ', 'ς', 'µ', 'Μ', 'μ', 'Ǆ', 'ǅ', 'ǆ', 'ﬀ', '😀', '$&', '$`', 'x.y*', 'Times New Roman',
  'times new roman', 'Wfit Test', '',
];

export const SITES = [
  ['rels.all', [/<Relationship\b[^>]*\/?>/g, (m) => `[${m}]`], 'replace'],
  ['rels.id', /\bId="([^"]*)"/, 'group'],
  ['rels.type', /\bType="([^"]*)"/, 'group'],
  ['rels.target', /\bTarget="([^"]*)"/, 'group'],
  ['rels.mode', /\bTargetMode="([^"]*)"/, 'group'],
  ['attr.unesc', composite(unescAttr), 'replace'],
  ['attr.esc', composite(escAttr), 'replace'],
  ['rels.expand', [/<Relationships\b([^>]*?)\s*\/>/, '<Relationships$1></Relationships>'], 'replace'],
  ['rels.insert', [/<\/Relationships>\s*$/, '<Relationship Id="rId9" Type="t" Target="fonts/font1.odttf"/></Relationships>'], 'replace'],
  ['ct.has_odttf', /<Default\b[^>]*Extension="odttf"/i, 'test'],
  ['ct.after_types', [/(<Types\b[^>]*>)/, '$1<Default Extension="odttf" ContentType="application/vnd.openxmlformats-officedocument.obfuscatedFont"/>'], 'replace'],
  ['ct.before_types_close', [/<\/Types>\s*$/, '<Override PartName="/word/fontTable.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.fontTable+xml"/></Types>'], 'replace'],
  ['el.find.ttf', findRow('w:embedTrueTypeFonts'), 'replace'],
  ['el.find.zoom', findRow('w:zoom'), 'replace'],
  ['el.remove.ttf', removeRow('w:embedTrueTypeFonts'), 'replace'],
  ['el.remove.sys', removeRow('w:embedSystemFonts'), 'replace'],
  ['el.remove.a', removeRow('a'), 'replace'],
  ['settings.on.ttf', onRow('embedTrueTypeFonts'), 'test'],
  ['settings.on.subset', onRow('saveSubsetFonts'), 'test'],
  ['settings.on.sys', onRow('embedSystemFonts'), 'test'],
  ['settings.root', /<w:settings\b[^>]*>/, 'match'],
  ['settings.root_edit', composite((xml) => {
    const root = /<w:settings\b[^>]*>/.exec(xml);
    if (!root) return 'THROW';
    if (root[0].endsWith('/>')) xml = xml.replace(root[0], `${root[0].slice(0, -2)}></w:settings>`);
    const r2 = /<w:settings\b[^>]*>/.exec(xml);
    return JSON.stringify([xml.slice(0, r2.index + r2[0].length), xml.slice(r2.index + r2[0].length)]);
  }), 'replace'],
  ['fonttable.entries', [/<w:font\b[^>]*w:name="([^"]*)"[^>]*>([\s\S]*?)<\/w:font>/g, (m, g1, g2) => `[${JSON.stringify([g1, g2])}]`], 'replace'],
  ['fonttable.embed_tag', /<w:embed(?:Regular|Bold|Italic|BoldItalic)\b/, 'test'],
  ['fonttable.strip', [/<w:embed(?:Regular|Bold|Italic|BoldItalic)\b[^>]*\/>/g, ''], 'replace'],
  ['fonts.has_r', /<w:fonts\b[^>]*xmlns:r=/, 'test'],
  ['fonts.add_r', [/<w:fonts\b/, `<w:fonts xmlns:r="${REL}"`], 'replace'],
  ['fonts.self_closing', /<w:fonts\b[^>]*\/>/, 'test'],
  ['fonts.expand', [/(<w:fonts\b[^>]*)\/>/, '$1></w:fonts>'], 'replace'],
  ...FAMILIES.map((f) => [`family.${[...f].map((c) => c.codePointAt(0).toString(16)).join('-') || 'empty'}`, new RegExp(`<w:font\\b[^>]*w:name="${escRe(escAttr(f))}"[^>]*?(\\/>|>[\\s\\S]*?<\\/w:font>)`, 'i'), 'match']),
  ['font.close_tail', [/<\/w:font>$/, '<w:embedBold r:id="rId1" w:fontKey="{K}"/></w:font>'], 'replace'],
  ['fonts.close_subst', [/<\/w:fonts>\s*$/, '<w:font w:name="$&|$`|$\'|$$|$1|$0|$01|$<x>|$">X</w:font></w:fonts>'], 'replace'],
  ['str.subst', composite((x) => x.replace('<w:font', 'A$&B$`C$\'D$$E$1F$<G$')), 'replace'],
];

const FIXED = [
  '', ' ', '>', '/>', '<', '$', '$$', '&amp;&lt;&gt;&quot;&apos;&amp;lt;&amp;amp;&#38;&x;&', 'a&b"c<d>e\'f',
  // rels
  '<Relationships xmlns="x"><Relationship Id="rId1" Type="t" Target="a.xml"/></Relationships>',
  '<Relationships xmlns="x"/>', '<Relationships xmlns="x"  \t/>', '<Relationships/>', '<Relationships>', '<Relationships a="1"/> \n',
  '<Relationships xmlns="x"></Relationships>  \r\n', '<Relationships xmlns="x"></Relationships>x', '<Relationships xmlns="x"></Relationships> ﻿',
  '<Relationship Id="rId2" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/font" Target="fonts/font1.odttf"/>',
  '<Relationship Type="t" Target="x" TargetMode="External" Id="a"/><Relationship Id="" Target="&amp;x&lt;"/>',
  '<Relationship xId="1" Id="2" r:Id="3"/>', '<Relationship r:Id="3" Id="4">', '<Relationship Id="a>b" Type="t"/>', '<Relationship Id="unclosed Type="t"/>',
  '<Relationship_x Id="1"/><Relationship\tId="2"/><Relationship', '<RelationshipId="1"/>', '<Relationships><Relationship Id="1"/><Relationships/></Relationships>',
  '<Relationship Id="1" Target="a" TargetMode="external"/><Relationship Id="2" TargetMode="External"', '<Relationship Id="é😀" Target="ü"/>',
  // content types
  '<Types xmlns="x"><Default Extension="xml" ContentType="application/xml"/></Types>',
  '<Types xmlns="x"><Default Extension="odttf" ContentType="c"/></Types>', '<Types><DEFAULT EXTENSION="ODTTF"/></Types>', '<Types><default x extension="OdTtF" /></Types>',
  '<Types><DefaultX Extension="odttf"/></Types>', '<Types><Default Extension="odttfx"/></Types>', '<Types><Default a=">" Extension="odttf"/></Types>',
  '<Types><Default Extension="odttf"', '<Default Extension="odttf">', '<Types/>', '<Types>', '<TypesX><Types a="1">', '<Types></Types>  ', '</Types>\n\t',
  '<Types></Types><x/>', '<Defaultſ Extension="odttf"/>', '<Default Extension="odttfK"/>',
  // settings
  '<w:settings xmlns:w="w"><w:zoom w:percent="100"/><w:embedTrueTypeFonts/></w:settings>',
  '<w:settings><w:embedTrueTypeFonts w:val="0"/></w:settings>', '<w:settings><w:embedTrueTypeFonts w:val="false"/><w:embedTrueTypeFonts/></w:settings>',
  '<w:settings><w:embedTrueTypeFonts w:val="off" ></w:embedTrueTypeFonts></w:settings>', '<w:settings><w:embedTrueTypeFonts w:val="1"/><w:saveSubsetFonts w:val="true"/></w:settings>',
  '<w:embedTrueTypeFonts', '<w:embedTrueTypeFonts>', '<w:embedTrueTypeFontsX/>', '<w:embedTrueTypeFonts w:val="0"/>', '<w:embedTrueTypeFonts />',
  '<w:embedTrueTypeFonts><w:embedTrueTypeFonts/></w:embedTrueTypeFonts>tail', '<w:embedTrueTypeFonts a="b">no close',
  '<w:embedTrue<w:embedTrueTypeFonts/>TypeFonts/>x', '<w:embedTrue<w:embedTrueTypeFonts>y</w:embedTrueTypeFonts>TypeFonts></w:embedTrueTypeFonts>z',
  '<a<a/> /><a<a></a>/>', '<a<a<a/>/>/>', '<a <a/>/>', '<a/><a><a/></a></a>', '<a', '<a　x>y</a>',
  '<w:embedSystemFonts/><w:embedSystemFonts w:val="0"></w:embedSystemFonts>', '<w:zoom/><w:view w:val="web"></w:view><w:zoom w:percent="90"></w:zoom>',
  '<w:settings/>', '<w:settings xmlns:w="w" a="$&"/>', '<w:settings a="$`$\'"/><w:settings/>', '<w:settingsX/><w:settings  />', '<w:settings', '<w:settings x="/>">',
  // font table
  '<w:fonts xmlns:w="w"><w:font w:name="Calibri"><w:panose1 w:val="020F0502020204030204"/></w:font></w:fonts>',
  '<w:fonts xmlns:w="w" xmlns:r="r"><w:font w:name="A&amp;B"><w:embedRegular r:id="rId1" w:fontKey="{K}"/></w:font><w:font w:name="Über"/></w:fonts>',
  '<w:fonts><w:font w:name="x"/><w:font w:name="y"><w:embedBold/></w:font></w:fonts>', '<w:fonts/>', '<w:fonts xmlns:r="r"/>', '<w:fonts a="1" />  ',
  '<w:fontsX/><w:fonts>', '<w:fonts xmlns:w="w">', '<w:fonts\txmlns:r="1">', '<w:fonts a=">" xmlns:r="1"/>',
  '<w:font w:name="a" w:name="b">c</w:font>', '<w:font w:name="a>b" x>c</w:font>', '<w:font w:name="a" w:name="b>x">c</w:font>', '<w:font w:name="CALIBRI">x</W:FONT>',
  '<W:FONT W:NAME="calibri"/>', '<w:font w:name="Calibri" />', '<w:font  w:name="Calibri"></w:font>', '<w:fontx w:name="Calibri"/>',
  '<w:font w:name="ſ"/><w:font w:name="S"/>', '<w:font w:name="k"/><w:font w:name="K"/>', '<w:font w:name="SS"/><w:font w:name="ß"/>',
  '<w:font w:name="ı"/><w:font w:name="I"/><w:font w:name="İ"/>', '<w:font w:name="ς"/><w:font w:name="Σ"/>', '<w:font w:name="µ"/><w:font w:name="μ"/>',
  '<w:font w:name="ǅ"/><w:font w:name="ǆ"/>', '<w:font w:name="ﬀ"/><w:font w:name="😀"/>', '<w:font w:name="$&amp;"/><w:font w:name="$&"/>',
  '<w:font w:name="a&lt;b&gt;c"/><w:font w:name="Q&quot;x">', '<w:font w:name="Times New Roman"><w:embedItalic r:id="1"></w:embedItalic></w:font>',
  '<w:embedRegular/><w:embedBoldItalic x="/" /><w:embedBoldX/><w:embedItalic>/></w:embedItalic><w:embedBold', '<w:embedRegular_/><w:embedBold />',
  '<w:font w:name="Wfit Test"><w:embedRegular r:id="rId3" w:subsetted="1" w:fontKey="{0}"/></w:font></w:fonts>\n',
  '</w:font>', '</w:font> ', 'x</w:font>', '</w:fonts>', '</w:fonts> \r\n', '</w:fonts>x', '</w:fonts></w:fonts>',
];

/** FIXED plus seeded concatenations of their pieces (the joins the hand matchers must get right). */
function generated() {
  const rng = new Rng(0xf0a7e5);
  const pieces = ['<w:font', ' w:name="', 'Calibri', 'über', '"', '>', '/>', '</w:font>', '</W:FONT>', '<w:fonts', 'xmlns:r=', '<Relationship', ' Id="rId1"',
    ' Type="t"', ' Target="a&amp;b"', '<Default', ' Extension="odttf"', '<Types>', '</Types>', '<w:embedTrueTypeFonts', ' w:val="0"', ' w:val="off"', '<w:zoom',
    '<w:embedBold', '<w:embedRegular', '<w:settings', '</w:embedTrueTypeFonts>', '<a', '</a>', ' ', '\n', ' ', 'é', '😀', '$&', 'x', '<', '$`'];
  const out = [];
  for (let i = 0; i < 120; i++) {
    let s = '';
    for (let n = 1 + rng.below(14); n > 0; n--) s += pieces[rng.below(pieces.length)];
    out.push(s);
  }
  return out;
}

export const INPUTS = [...FIXED, ...generated()];
