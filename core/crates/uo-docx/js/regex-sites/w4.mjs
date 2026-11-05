// The regex sites of the D1b frontier W4/W4b (src/write/emit/notes.rs and settings.rs: notes.ts, settings-xml.ts,
// hf-ops.ts) against V8, checked by tests/regex_w4.rs. Composite rows run the TS code copied as written (rows
// return strings, booleans or JSON, never positions: UTF-16 indices would differ from Rust's byte offsets).
import { Rng } from '../../../uo-testkit/js/testkit.mjs';

// notes.ts:17–23 (verbatim)
function notePropsInner(p) {
  if (!p) return '';
  return (p.pos ? `<w:pos w:val="${p.pos}"/>` : '')
    + (p.numFmt ? `<w:numFmt w:val="${p.numFmt}"/>` : '')
    + (p.numStart !== undefined ? `<w:numStart w:val="${p.numStart}"/>` : '')
    + (p.numRestart ? `<w:numRestart w:val="${p.numRestart}"/>` : '');
}

// notes.ts:93–137 (verbatim)
const SETTINGS_AFTER_NOTES = ['w:compat', 'w:docVars', 'w:rsids', 'm:mathPr', 'w:attachedSchema', 'w:themeFontLang',
  'w:clrSchemeMapping', 'w:doNotIncludeSubdocsInStats', 'w:doNotAutoCompressPictures', 'w:forceUpgrade', 'w:captions',
  'w:readModeInkLockDown', 'w:smartTagType', 'sl:schemaLibrary', 'w:shapeDefaults', 'w:doNotEmbedSmartTags',
  'w:decimalSymbol', 'w:listSeparator'];
function patchSettingsNotes(xml, props) {
  let out = xml;
  for (const kind of ['footnote', 'endnote']) {
    const tag = `w:${kind}Pr`;
    const re = new RegExp(`<${tag}(?:\\s[^>]*)?(?:/>|>[\\s\\S]*?</${tag}>)`);
    const m = re.exec(out);
    const want = props[kind];
    const seps = `<w:${kind} w:id="-1"/><w:${kind} w:id="0"/>`;
    if (m) {
      const whole = m[0];
      const close = `</${tag}>`;
      const body = whole.endsWith(close) ? whole.slice(whole.indexOf('>') + 1, whole.length - close.length) : '';
      const refRe = new RegExp(`<w:${kind}\\b[^>]*/>`, 'g');
      const refs = body.match(refRe)?.join('') ?? '';
      const p = want === undefined ? body.replace(refRe, '') : notePropsInner(want ?? undefined);
      const next = `<${tag}>${p}${refs || seps}</${tag}>`;
      if (next !== whole) out = out.slice(0, m.index) + next + out.slice(m.index + whole.length);
    } else {
      const el = `<${tag}>${notePropsInner(want ?? undefined)}${seps}</${tag}>`;
      const enAt = kind === 'footnote' ? out.indexOf('<w:endnotePr') : -1;
      let at = enAt;
      if (at < 0) {
        for (const n of SETTINGS_AFTER_NOTES) {
          const k = out.search(new RegExp(`<${n}[\\s/>]`));
          if (k >= 0 && (at < 0 || k < at)) at = k;
        }
      }
      if (at < 0) at = out.lastIndexOf('</w:settings>');
      out = out.slice(0, at) + el + out.slice(at);
    }
  }
  return out;
}
// the TextEncoder step of the package: a lone surrogate (the cut of `slice(…, -1)`) is U+FFFD
const encoded = (s) => new TextDecoder().decode(new TextEncoder().encode(s));

const REL = 'http://schemas.openxmlformats.org/officeDocument/2006/relationships';

// hf-ops.ts:47–51 (verbatim)
function copyHfPartXml(xml, newId) {
  return xml
    .replace(/(<wp:docPr\b[^>]*?\bid=")(\d+)(")/g, (_m, a, _id, b) => `${a}${newId()}${b}`)
    .replace(/\s(?:w14:paraId|w14:textId|wp14:anchorId|wp14:editId)="[0-9A-Fa-f]+"/g, '');
}

// hf-ops.ts:62–83, the text half (the package calls become pushes)
function dropText(rels, ct, drop) {
  const removed = [];
  for (const m of [...rels.matchAll(/<Relationship\b[^>]*?\/>/g)]) {
    const tag = m[0];
    if (!/relationships\/(?:header|footer)"/.test(tag)) continue;
    const id = /\bId="([^"]+)"/.exec(tag)?.[1];
    const target = /\bTarget="([^"]+)"/.exec(tag)?.[1];
    if (!id || !target || !drop.has(id)) continue;
    const path = `word/${target.replace(/^\.\//, '').replace(/^\/?word\//, '')}`;
    rels = rels.replace(tag, '');
    ct = ct.replace(new RegExp(`<Override PartName="/${path.replace(/[.]/g, '\\.')}"[^>]*/>`), '');
    removed.push(path);
  }
  return { rels, ct, removed };
}

// a path as the patterns take it literally: no regex syntax but the escaped dot (accepted by name, G9)
const literalPath = (p) => p.replace(/[^A-Za-z0-9._/-]/g, '');

export const SITES = [
  ['tab.cur', /<w:defaultTabStop\b[^>]*?\/>/, 'match'],
  ['tab.val', (x) => {
    const [v, hay = ''] = x.split('§');
    const val = String(Math.round(Number(v)));
    return new RegExp(`w:val="${val}"`).test(hay);
  }, 'fn'],
  ['hy.cur', /<w:autoHyphenation\b[^>]*?(?:\/>|>[\s\S]*?<\/w:autoHyphenation>)/, 'match'],
  ['flag.cur', /<w:evenAndOddHeaders\b[^>]*?(?:\/>|>[\s\S]*?<\/w:evenAndOddHeaders>)/, 'match'],
  ['flag.off', /w:val="(?:0|false|off)"/, 'test'],
  ['notes.props', /<w:footnotePr(?:\s[^>]*)?(?:\/>|>[\s\S]*?<\/w:footnotePr>)/, 'match'],
  ['notes.refs', (x) => x.match(/<w:footnote\b[^>]*\/>/g)?.join('') ?? '', 'fn'],
  ['notes.norefs', [/<w:footnote\b[^>]*\/>/g, ''], 'replace'],
  ['notes.keep', (x) => encoded(patchSettingsNotes(x, {})), 'fn'],
  ['notes.set', (x) => encoded(patchSettingsNotes(x, { footnote: { pos: 'pageBottom', numFmt: 'lowerRoman', numStart: 3, numRestart: 'eachPage' }, endnote: null })), 'fn'],
  ['notes.set2', (x) => encoded(patchSettingsNotes(x, { footnote: null, endnote: { pos: 'sectEnd', numStart: 0 } })), 'fn'],
  ['notes.relfn', (x) => new RegExp(`Type="${REL}/footnotes"`).test(x), 'fn'],
  ['notes.relen', (x) => new RegExp(`Type="${REL}/endnotes"`).test(x), 'fn'],
  ['notes.free', (x) => { let n = 1; while (x.includes(`Id="rId${n}"`)) n++; return n; }, 'fn'],
  ['gallery', /<w:docPartGallery\s+w:val="Page Numbers/, 'test'],
  ['attr.id', /\bId="([^"]+)"/, 'group'],
  ['attr.target', /\bTarget="([^"]+)"/, 'group'],
  ['hf.rel', /relationships\/(?:header|footer)"/, 'test'],
  ['hf.target', (x) => `word/${x.replace(/^\.\//, '').replace(/^\/?word\//, '')}`, 'fn'],
  ['hf.override', (x) => {
    const [p0, ct = ''] = x.split('§');
    const path = literalPath(p0);
    return ct.replace(new RegExp(`<Override PartName="/${path.replace(/[.]/g, '\\.')}"[^>]*/>`), '');
  }, 'fn'],
  ['hf.copy', (x) => { let n = 100; return copyHfPartXml(x, () => ++n); }, 'fn'],
  ['hf.drop', (x) => {
    const [rels = '', ct = '', ids = ''] = x.split('§');
    const r = dropText(rels, ct, new Set(ids.split(',')));
    return r;
  }, 'fn'],
];

// ── inputs: hand-written edge cases and seeded soup over the patterns' vocabulary ──
const SETTINGS = '<?xml version="1.0" encoding="UTF-8" standalone="yes"?><w:settings xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main">';
const HAND = [
  '', 'x', ' ', '<', '>', '/>', '</w:settings>',
  // defaultTabStop / flags
  '<w:defaultTabStop w:val="720"/>', '<w:defaultTabStop/>', '<w:defaultTabStop w:val="720">', '<w:defaultTabStopX w:val="1"/>', '<w:defaultTabStop\nw:val="1"\n/>',
  '<w:defaultTabStop w:val="1"></w:defaultTabStop><w:defaultTabStop w:val="2"/>', '<w:defaultTabStop w:val="1" / >', '<w:defaultTabStop/ >', '<w:defaultTabStop w:val="1"/',
  '<w:defaultTabStop w:val=">"/>', '<w:defaultTabStopé/>', '<w:defaultTabStop_/>', 'é<w:defaultTabStop/>',
  '<w:autoHyphenation/>', '<w:autoHyphenation w:val="0"/>', '<w:autoHyphenation></w:autoHyphenation>', '<w:autoHyphenation w:val="1">x</w:autoHyphenation>', '<w:autoHyphenationX/>', '<w:autoHyphenation/><w:autoHyphenation w:val="1"/>',
  '<w:autoHyphenation w:val="1"/><w:consecutiveHyphenLimit/></w:autoHyphenation>', '<w:autoHyphenation><w:x/>', '<w:evenAndOddHeaders/>', '<w:evenAndOddHeaders w:val="off"/>', '<w:evenAndOddHeaders w:val="false"></w:evenAndOddHeaders>',
  '<w:evenAndOddHeaders w:val="on"/>', 'xw:val="0"', 'w:val="0"', 'w:val="false"', 'w:val="off"', 'w:val="1"', 'w:val="00"', 'w:val=\'0\'', 'a w:val="OFF"',
  '720§w:val="720"', '720§w:val="7200"', '720§x w:val="720" y', '-5§w:val="-5"', '-0§w:val="0"', '0.4§w:val="0"', '0.5§w:val="1"', '-0.5§w:val="0"', '-0.5§w:val="-0"', 'x§w:val="NaN"', 'Infinity§w:val="Infinity"', '-Infinity§w:val="-Infinity"',
  '1e21§w:val="1e+21"', '1e21§w:val="1ee21"', '1e21§w:val="1eee21"', '1e21§w:val="1e21"', '1.5e300§w:val="1.5e+300"', '1.5e300§w:val="1X5ee300"', '1.5e300§w:val="1\n5e300"', '1.5e300§w:val="1.5e300"', '1.5e300§w:val="1😀5e300"',
  '-1e21§w:val="-1e+21"', '-1e21§w:val="-1ee21"', '123456789012345680000§w:val="123456789012345680000"', '1e21§w:val="1e', '1e21§w:val=', '2§', '2', '1e21§w:val="1e+21" w:val="1e+21"', '1e21§w:val="1ee2" w:val="1ee21"',
  // notes
  SETTINGS + '</w:settings>', SETTINGS + '<w:compat/></w:settings>', SETTINGS + '<w:compat></w:compat></w:settings>', SETTINGS + '<w:zoom/><w:compat><w:x/></w:compat><w:rsids/></w:settings>',
  SETTINGS + '<w:footnotePr><w:footnote w:id="-1"/><w:footnote w:id="0"/></w:footnotePr><w:endnotePr><w:endnote w:id="-1"/><w:endnote w:id="0"/></w:endnotePr><w:compat/></w:settings>',
  SETTINGS + '<w:footnotePr><w:pos w:val="beneathText"/><w:footnote w:id="-1"/><w:footnote w:id="0"/></w:footnotePr><w:compat/></w:settings>',
  SETTINGS + '<w:footnotePr><w:numFmt w:val="decimal"/></w:footnotePr><w:endnotePr/><w:compat/></w:settings>',
  SETTINGS + '<w:footnotePr/><w:compat/></w:settings>', SETTINGS + '<w:footnotePr w:x="1"/><w:compat/></w:settings>', SETTINGS + '<w:footnotePr w:x="1"/><w:compat/><w:footnotePr/></w:settings>',
  SETTINGS + '<w:footnotePr w:x="1"/><w:compat/></w:footnotePr></w:settings>', SETTINGS + '<w:footnotePr w:x="1"/></w:footnotePr>', SETTINGS + '<w:footnotePr/></w:footnotePr>',
  '<w:footnotePr/ ></w:footnotePr>', '<w:footnotePr/></w:footnotePr>', '<w:footnotePr>', '<w:footnotePr', '<w:footnotePr ', '<w:footnotePr >', '<w:footnotePr\t/>', '<w:footnotePr\u00a0x="1"/>', '<w:footnotePr\ufeff/>', '<w:footnotePrX/>', '<w:footnotePrX></w:footnotePrX>', '<w:footnotePr-/>',
  '<w:footnotePr><w:footnote w:id="-1"/></w:footnotePr>', '<w:footnotePr><w:footnote w:id="1"/><w:footnotePr/></w:footnotePr>', '<w:footnotePr><w:footnoteRef/></w:footnotePr>', '<w:footnotePr><w:footnote/></w:footnotePr>', '<w:footnotePr><w:footnote-x/></w:footnotePr>', '<w:footnotePr><w:footnote w:id="1" / ></w:footnotePr>', '<w:footnotePr><w:footnote w:id="1"></w:footnote></w:footnotePr>',
  '<w:footnote w:id="-1"/><w:footnote w:id="0"/>', '<w:footnote\nw:id="-1"/>', '<w:footnote w:id=">"/>', '<w:footnote/>', '<w:footnoteX/>', '<w:footnote_/>', '<w:footnote é/>', '<w:footnote /', '<w:footnote /><w:footnote />',
  SETTINGS + '<w:endnotePr><w:pos w:val="docEnd"/></w:endnotePr><w:compat/></w:settings>', SETTINGS + '<w:endnotePrX/></w:settings>', SETTINGS + '<w:compatibility/><w:themeFontLang/></w:settings>', SETTINGS + '<w:compat\n/></w:settings>', SETTINGS + '<w:compat\t></w:settings>',
  SETTINGS + '<w:decimalSymbol/><w:listSeparator/></w:settings>', SETTINGS + '<w:listSeparator w:val=","/><w:decimalSymbol w:val="."/></w:settings>', SETTINGS + '<m:mathPr/></w:settings>', SETTINGS + '<sl:schemaLibrary/><w:shapeDefaults/></w:settings>', SETTINGS + '<w:rsids></w:rsids>',
  '<w:compat/>', 'no settings', 'é', '😀', 'x😀', '<w:compat/>😀', '</w:settings>😀', '😀</w:settings>', '<w:docVars/>😀',
  // relationships
  '<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"></Relationships>',
  '<Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/footnotes" Target="footnotes.xml"/>',
  '<Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/endnotes" Target="endnotes.xml"/>',
  'Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/footnotes"', 'Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/endnotes"',
  'Type="http://schemas-openxmlformats-org/officeDocument/2006/relationships/footnotes"', 'Type="http://schemasXopenxmlformatsXorg/officeDocument/2006/relationships/footnotes"', 'Type="http://schemas\nopenxmlformats.org/officeDocument/2006/relationships/footnotes"',
  'Type="http://schemas😀openxmlformats.org/officeDocument/2006/relationships/footnotes"', 'Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/footnotes', 'Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/footnotesX"', 'Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/footnote"',
  'Type="http://schemas..openxmlformats.org/officeDocument/2006/relationships/footnotes"', 'Type="http://schemas.openxmlformats.orgXofficeDocument/2006/relationships/footnotes"', 'xType="http://schemas.openxmlformats.org/officeDocument/2006/relationships/footnotes"',
  'Type="http://schemasTYPE="http://schemas.openxmlformats.org/officeDocument/2006/relationships/footnotes"', 'Type="http://schemas\u2028openxmlformats.org/officeDocument/2006/relationships/endnotes"', 'Type="http://schemas\u00a0openxmlformats\Feffxorg/officeDocument/2006/relationships/endnotes"',
  'Id="rId1"', 'Id="rId1" Id="rId2" Id="rId3"', 'Id="rId2"', 'Id="rId1"Id="rId2"', 'ParentId="rId1"', 'Id="rId01"', 'Id="rId0"', 'Id="rId1" Id="rId01" Id="rId3"', 'Id="rId"', 'Id="rIdA"', 'Id="rId1', 'Id=rId1', 'Id="rId1 "', 'Id=" rId1"', 'Id="rId99999999999999999"', 'Id="rId9007199254740993"', 'Id="rId1" Id="rId2" Id="rId3" Id="rId4" Id="rId5"',
  // header/footer
  '<wp:docPr id="1" name="a"/>', '<wp:docPr id="1" name="a"/><wp:docPr id="22" name="b"/>', '<wp:docPr name="a" id="7"/>', '<wp:docPr name="a" xid="7"/>', '<wp:docPr name="a" _id="7"/>', '<wp:docPr name="a" id="7" id="8"/>', '<wp:docPr name="a" id="x7" id="8"/>',
  '<wp:docPr id=""/>', '<wp:docPr id="7x"/>', '<wp:docPr id=\'7\'/>', '<wp:docPr id="7', '<wp:docPr id="007"/>', '<wp:docPr descr="id=&quot;1&quot;" id="3"/>', '<wp:docPr descr=" id=&quot;1&quot;" id="3"/>', '<wp:docPr descr=" id="1" id="3"/>', '<wp:docPrX id="1"/>', '<wp:docPr>id="1"</wp:docPr>', '<wp:docPr\nid="1"/>', '<wp:docPr\u00a0id="1"/>',
  '<wp:docPr id="1"<wp:docPr id="2"/>', '<wp:docPr <wp:docPr id="2">', '<wp:docPr id="1"><wp:docPr id="2"/>', '<wp:docPr é id="1"/>', '<wp:docPr id="١"/>', '<wp:docPr id="1"/>😀<wp:docPr id="2"/>',
  '<w:p w14:paraId="0A1B2C3D" w14:textId="77777777"><w:r/></w:p>', '<w:p\nw14:paraId="0a1b"/>', '<w:p w14:paraId=""/>', '<w:p w14:paraId="xyz"/>', '<w:p w14:paraId="1G"/>', '<w:p w14:paraId=\'AB\'/>', '<w:p w14:paraId="AB" w14:textId="CD" w14:paraId="EF"/>',
  '<wp:anchor wp14:anchorId="1A" wp14:editId="2B" distT="0"/>', '<wp:anchor\twp14:anchorId="1A"\u2028wp14:editId="2B"/>', '<wp:anchor wp14:anchorIdX="1A"/>', '<wp:anchor xwp14:anchorId="1A"/>', '<wp:anchor wp14:anchorId="1A"wp14:editId="2B"/>', '<wp:anchor w14:anchorId="1A" wp14:paraId="2B"/>', '<wp:anchor\u00a0wp14:editId="FF"\ufeffw14:textId="00"/>', '<wp:anchor wp14:editId="FF',
  '<wp:docPr id="5"/><w:p w14:paraId="AA" w14:textId="BB"/><wp:docPr id="6"/>', '<wp:docPr id="5" w14:paraId="AA"/>',
  '<Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/header" Target="header1.xml"/>',
  '<Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/footer" Target="/word/footer1.xml"/>',
  '<Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/header" Target="./header1.xml"/>',
  'relationships/header"', 'relationships/footer"', 'relationships/headers"', 'relationships/header', 'relationship/header"', 'relationships/Header"', 'xrelationships/footer"', 'relationships/footer"relationships/header"',
  'Target="x"', 'xTarget="x"', '_Target="x"', 'Target=""', 'Target="" Target="y"', 'Target="x', 'Target=x', 'Id="a" Id="b"', 'Id="" Id="b"', 'ParentId="a" Id="b"', 'Id="é"', '-Id="a"', 'é Id="a"', ' Id="a"', 'Id="a"',
  '<w:sdtPr><w:docPartObj><w:docPartGallery w:val="Page Numbers (Bottom of Page)"/></w:docPartObj></w:sdtPr>', '<w:docPartGallery w:val="Page Numbers"/>', '<w:docPartGallery\nw:val="Page Numbers (Top)"/>', '<w:docPartGallery w:val="Cover Pages"/>', '<w:docPartGallery  w:val="Page Numbers"/>',
  '<w:docPartGallery w:val="page Numbers"/>', '<w:docPartGallery w:val="Page Number"/>', '<w:docPartGallery/>', '<w:docPartGallery', '<w:docPartGalleryw:val="Page Numbers"/>', '<w:docPartGallery\u00a0\ufeffw:val="Page Numbers"/>', '<w:docPartGallery\u2028w:val="Page Numbers"/>', '<w:docPartGallery x w:val="Page Numbers"/>',
  '<w:docPartGallery w:val="Cover"/><w:docPartGallery w:val="Page Numbers"/>', '<w:docPartGallery', '<w:docPartGallery ',
  './header1.xml', '/word/header1.xml', 'word/header1.xml', '/header1.xml', 'header1.xml', '././header1.xml', '../header1.xml', '/word/word/header1.xml', 'word/', '/word/', './', '', '//word/x', 'xword/x', '/wordx/x', './/word/x', './word/word/x', '\\word\\x',
  'header1.xml§<Override PartName="/header1.xml" ContentType="a"/>', 'word/header1.xml§<Override PartName="/word/header1.xml" ContentType="a"/><Override PartName="/word/header1.xml" ContentType="b"/>',
  'word/header1.xml§<Override PartName="/word/header1.xml"/>', 'word/header1.xml§<Override PartName="/word/header1.xml" ContentType="a">', 'word/header1.xml§<Override PartName="/word/header1.xml" ContentType="a"></Override>', 'word/header1.xml§<Override PartName="/word/header1.xml" a="b" / >',
  'word/header1.xml§<Override PartName="/word/header1.xml"/ >', 'word/header1.xml§<Override PartName="/word/header1.xml">x/>', 'word/header1.xml§<Override PartName="/word/header1.xml"><Override PartName="/word/header1.xml"/>',
  'word/header1.xml§<Override PartName="/word/header1.xml" /', 'word/headerX.xml§<Override PartName="/word/header1.xml"/>', 'word/header1xml§<Override PartName="/word/header1.xml"/>', 'word/header1.xml§<Override PartName="/word/header1Xxml"/>', 'word/header(1).xml§<Override PartName="/word/header1.xml"/>',
  'word/header1.xml§<Override  PartName="/word/header1.xml"/>', 'word/header1.xml§<override PartName="/word/header1.xml"/>', 'word/header1.xml§<Override PartName="/word/header1.xml" é/>', 'word/header1.xml§', 'word/header1.xml', '§x', '§', 'word/h😀.xml§<Override PartName="/word/h.xml"/>',
  '<Relationships><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/header" Target="header1.xml"/><Relationship Id="rId2" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/footer" Target="footer1.xml"/><Relationship Id="rId3" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/styles" Target="styles.xml"/></Relationships>§<Types><Override PartName="/word/header1.xml" ContentType="h"/><Override PartName="/word/footer1.xml" ContentType="f"/></Types>§rId1,rId3',
  '<Relationships><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/header" Target="header1.xml"/><Relationship Id="rId2" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/footer" Target="footer1.xml"/></Relationships>§<Types><Override PartName="/word/header1.xml" ContentType="h"/><Override PartName="/word/footer1.xml" ContentType="f"/></Types>§rId1,rId2',
  '<Relationship Id="a" Type="x/header" Target="h.xml"/><Relationship Id="a" Type="x/header" Target="h.xml"/>§<Override PartName="/word/h.xml"/><Override PartName="/word/h.xml"/>§a',
  '<Relationship Id="a" Type="x/header" Target="h.xml"/>§<Override PartName="/word/h.xml"/>§', '<Relationship Id="a" Type="x/header" Target="h.xml"/>§<Override PartName="/word/h.xml"/>§a,b', '<Relationship Id="" Type="x/header" Target="h.xml"/>§§',
  '<Relationship Id="a" Type="x/header" Target="h.xml">§<Override PartName="/word/h.xml"/>§a', '<Relationship Id="a" Type="x/header" Target="h.xml" />§<Override PartName="/word/h.xml"/>§a', '<Relationship Id="a" Type="x/header" Target="h.xml"/ >§§a',
  '<Relationship Id="a" Type="x/header" Target="/word/h.xml"/><Relationship Id="b" Type="x/footer" Target="./word/f.xml"/>§<Override PartName="/word/h.xml"/><Override PartName="/word/f.xml"/>§a,b',
  '<Relationship Id="a" Type="x/header" Target="h.xml"/><Relationshipx Id="b" Type="x/header" Target="h2.xml"/>§<Override PartName="/word/h2.xml"/>§a,b', '<RelationshipId="a" Type="x/header" Target="h.xml"/>§§a', '<Relationship\nId="a" Type="x/header" Target="h.xml"/>§§a',
  '<Relationship Id="a" Type="x/header" Target="h$&.xml"/>§<Override PartName="/word/h$&.xml"/>§a', '<Relationship Id="a" Type="x/header" Target="h$1.xml"/>§<Override PartName="/word/h$1.xml"/>§a',
  '<Relationship Id="$&" Type="x/header" Target="h.xml"/>§<Override PartName="/word/h.xml"/>§$&', '<Relationship Id="a" Type="x/header" Target="a/b/h.xml"/>§<Override PartName="/word/a/b/h.xml"/>§a',
  'é<Relationship Id="a" Type="x/header" Target="é.xml"/>é§<Override PartName="/word/é.xml"/>§a',
  '<Relationship Id="a" Type="x/header" Target="h.xml"/>😀§<Override PartName="/word/h.xml"/>😀§a',
];
const VOCAB = ['<w:footnotePr', '</w:footnotePr>', '<w:endnotePr', '<w:footnote w:id="-1"/>', '<w:footnote', '<w:footnoteRef/>', '/>', '>', '/', ' ', '\n', '\u00a0', '<w:compat', '<w:compat/>', '</w:settings>', '<w:settings>', '<w:rsids/>',
  '<w:defaultTabStop', '<w:autoHyphenation', '</w:autoHyphenation>', '<w:evenAndOddHeaders', ' w:val="0"', ' w:val="720"', 'w:val="', '"', '1', '2', '.', '+', 'e', '-', '§', ',',
  '<wp:docPr', ' id="', ' id="3"', 'id="9"', ' w14:paraId="0A"', ' wp14:editId="FF"', ' wp14:anchorId="1"', ' w14:textId="B"', '<w:docPartGallery', ' w:val="Page Numbers', 'w:val="Page Numbers (Top)"',
  '<Relationship', ' Id="a"', ' Id="rId1"', ' Target="h.xml"', ' Target="/word/h.xml"', ' Type="x/header"', ' Type="x/footer"', 'relationships/header"', 'relationships/footer"', 'Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/footnotes"',
  '<Override PartName="/word/h.xml"', '<Override PartName="/word/h.xml"/>', 'word/h.xml', './', '/word/', 'word/', 'é', '😀', '=', '<', '$&', '</Relationships>'];
function seeded(seed, count) {
  const rng = new Rng(seed);
  const out = [];
  for (let i = 0; i < count; i++) {
    let s = '';
    for (let k = 1 + rng.below(14); k > 0; k--) s += VOCAB[rng.below(VOCAB.length)];
    out.push(s);
  }
  return out;
}
export const INPUTS = [...HAND, ...seeded(0x57a4c0de, 600)];
