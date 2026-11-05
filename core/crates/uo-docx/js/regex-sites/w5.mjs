// The regex sites of frontier W5 (page-background.ts, watermark.ts) against V8, checked by
// tests/regex_w5.rs. A row is `[id, regex | function, op]` (see js/regex-diff.mjs); the sites that
// are an expression around a regex (a replacement template, a dynamic pattern with its value as a
// literal) are functions: V8 evaluates the TS expression whole. The dynamic patterns of
// `writePageBackground` (the `Id="rId…"` and `"rId…"` tests and the `<Relationship …Id="X"…/>` removal)
// are run with the ids a save meets: `rId` followed by digits, so no metacharacter reaches the
// pattern (a metacharacter changes or throws in the TS: accepted by name, G9).
import { INPUTS as BASE } from './templates.mjs';

const fracAttr = (v) => (v === 0 ? '0' : v === 1 ? '1' : String(Math.round(v * 1000) / 1000).replace(/^0\./, '.'));
const pctAttr = (v) => `${Math.round(v * 100) / 100}%`;
const escAttr = (s) => s.replace(/&/g, '&amp;').replace(/"/g, '&quot;').replace(/</g, '&lt;');
const tpl = (ext) => `$1<Default Extension="${ext}" ContentType="image/${ext}"/>`;
const relRemove = (rid) => (s) => {
  const m = new RegExp(`<Relationship\\b[^>]*\\bId="${rid}"[^>]*/>`).exec(s);
  return m ? s.slice(0, m.index) + s.slice(m.index + m[0].length) : s;
};
const num = (s) => Number(s);

/** [site id, the TS regex (or function), operation]. */
export const SITES = [
  ['w5.rels_path', (s) => s.replace(/([^/]+)$/, '_rels/$1.rels'), 'fn'],
  ['w5.first_rid', (s) => /\br:id="([^"]+)"/.exec(s)?.[1] ?? null, 'fn'],
  ['w5.types_open', /<Types\b[^>]*>/, 'match'],
  ['w5.document_open', /<w:document\b[^>]*>/, 'match'],
  ['w5.background_el', /<w:background\b[^>]*?(?:\/>|>[\s\S]*?<\/w:background>)/, 'match'],
  ['w5.add_ext_png', (s) => s.replace(/(<Types\b[^>]*>)/, tpl('png')), 'fn'],
  ['w5.add_ext_amp', (s) => s.replace(/(<Types\b[^>]*>)/, tpl('a$&b')), 'fn'],
  ['w5.add_ext_back', (s) => s.replace(/(<Types\b[^>]*>)/, tpl('a$`b')), 'fn'],
  ['w5.add_ext_quote', (s) => s.replace(/(<Types\b[^>]*>)/, tpl("a$'b")), 'fn'],
  ['w5.add_ext_group', (s) => s.replace(/(<Types\b[^>]*>)/, tpl('a$1b$2c$10d$01e$0')), 'fn'],
  ['w5.add_ext_dollar', (s) => s.replace(/(<Types\b[^>]*>)/, tpl('$$')), 'fn'],
  ['w5.color2', (s) => { const m = /^\s*fill(?:\s+(darken|lighten)\((\d+)\))?\s*$/i.exec(s); return m ? [m[1] ?? null, m[2] ?? null] : null; }, 'fn'],
  ['w5.trailing_digits', (s) => s.replace(/\d+$/, ''), 'fn'],
  ['w5.letters_len', (s) => s.replace(/\s+/g, '').length, 'fn'],
  ['w5.letters_len_s', (s) => s.replace(/\s/g, '').length, 'fn'],
  ['w5.frac_attr', (s) => fracAttr(num(s)), 'fn'],
  ['w5.pct_attr', (s) => pctAttr(num(s)), 'fn'],
  ['w5.esc_attr', escAttr, 'fn'],
  ['w5.remove_rel_5', relRemove('rId5'), 'fn'],
  ['w5.remove_rel_12', relRemove('rId12'), 'fn'],
  ['w5.contains_id', (s) => new RegExp('Id="rId5"').test(s), 'fn'],
  ['w5.contains_quoted', (s) => new RegExp('"rId5"').test(s), 'fn'],
];

const rel = (attrs) => `<Relationship ${attrs}/>`;
const rels = [
  `<Relationships>${rel('Id="rId5" Type="x" Target="a.png"')}</Relationships>`,
  `<Relationships>${rel('Type="x" Id="rId5" Target="a.png"')}${rel('Id="rId6" Target="b"')}</Relationships>`,
  `<Relationships>${rel('Id="rId12"')}${rel('Id="rId5"')}</Relationships>`,
  '<Relationships><Relationship Id="rId5"></Relationship></Relationships>',
  '<Relationships><Relationship Id="rId5" ></Relationships>',
  '<Relationships><Relationship Id="rId5" /></Relationships>',
  '<Relationships><Relationship\tId="rId5"\n/></Relationships>',
  '<Relationships><RelationshipX Id="rId5"/></Relationships>',
  '<Relationships><Relationship xId="rId5"/></Relationships>',
  '<Relationships><Relationship _Id="rId5"/></Relationships>',
  '<Relationships><Relationship ThemeId="rId5"/></Relationships>',
  '<Relationships><Relationship a="Id=&quot;rId5&quot;" Id="rId5"/></Relationships>',
  '<Relationships><Relationship Id="rId5" b=">" Id="rId12"/></Relationships>',
  '<Relationships><Relationship Id="rId55"/></Relationships>',
  '<Relationships><Relationship Id="rId5"/><Relationship Id="rId5"/></Relationships>',
  '<Relationships><Relationship Id="rId12" Id="rId5" /></Relationships>',
  '<Relationship Id="rId5"/>', '<Relationship Id="rId5"', 'Id="rId5"', '"rId5"', 'x"rId5"y', '"rId55"', 'é Id="rId5" é',
  '<Relationships><Relationship Id="rId5" Target="é😀"/><Relationship Id="rId12"/></Relationships>',
];
const cts = [
  '<Types xmlns="x"><Default Extension="xml"/></Types>',
  '<?xml version="1.0"?><Types xmlns="x"></Types>',
  '<Types>', '<Types', '<TypesX><Types a="1">', '<TypesX a="1"><Types a="2">', '<Types\n>', '<Types\ta=">">', '<Types/>', '<Types a="é">é</Types>',
  '<Types a="1" <Types b="2">', 'prefix <Types a="1"> middle <Types b="2"> suffix', '<_Types>', 'x<Types>', '<Types_>', '<Types-a>', '<Types:a>', '<Types.a>',
  '', ' ', '<types>', '<Typesé>', '<TypesÉ>',
];
const docs = [
  '<w:document xmlns:w="x"><w:body/></w:document>', '<w:document\nxmlns:w="x"\n>', '<w:documentX>', '<w:document xmlns:w="x"', '<w:docum', '<w:document>', '<w:document/>',
  '<w:document a=">" b="1">', '<w:documents><w:document>', '<w:document-x>', '<w:document_x>', '<w:document:x>', '<w:document.x>', 'é<w:document a="é">',
  '<w:background w:color="FF0000"/>', '<w:background w:color="FF0000"></w:background>', '<w:background w:color="A">x</w:background>', '<w:backgroundX/>', '<w:background>',
  '<w:background a="/>"', '<w:background a="/" />', '<w:background/ >', '<w:background /><w:background w:color="2"/>', '<w:background><w:background></w:background>',
  '<w:background w:color="1"><v:fill r:id="rId5"/></w:background><w:body/>', '<w:background\tw:color="1"\n/>', '<w:background-x/>', '<w:background_x/>', '<w:background:x/>',
  '<w:background a="1">  </w:background  >', '<w:background a="1"></w:background >', '<w:backgroundé/>', '<w:background é="1"/>',
  '<w:background><v:fill r:id="rId5" r:id="rId6"/>', 'x r:id="a" r:id="b"', ' r:id=""', ' r:id="" r:id="b"', '\tr:id="a"', 'xr:id="a"', '_r:id="a"', 'é r:id="a"', 'ér:id="a"', 'r:id="a"', 'r:id="a', 'r:id="', 'a r:id="b"c" r:id="d"',
  'r:id="é"', ' r:id="\n"', '1r:id="a"', ':r:id="a"', '-r:id="a"', '.r:id="a"', 'w:r:id="a"',
];
const paths = ['word/document.xml', 'document.xml', '/word/document.xml', 'word/', '', '/', 'a/b/', 'a//b', 'a/b/c.xml', 'é/ü.xml', 'x.rels', '../a.xml', 'a b/c d.xml', 'word/_rels/x.xml', '/a', 'a/'];
const fills = [
  'fill', 'FILL', 'Fill', 'fill darken(128)', 'fill lighten(100)', 'FILL DARKEN(5)', ' fill', 'fill ', ' fill ', '\tfill\n', 'fill  darken(5)', 'fill\tlighten(5)', 'fill\ndarken(5)', 'fill darken(5) ', 'fill darken(5)\n',
  'fill darken(5)x', 'fill darken()', 'fill darken(5', 'fill darken 5)', 'fill darken (5)', 'fill darken( 5)', 'fill darken(5 )', 'fill darken(-5)', 'fill darken(5.5)', 'fill darken(0x5)', 'fill darken(٣)', 'fill darken(５)',
  'fill lighten(255)', 'fill lighten(99999999999999999999999999)', 'fill lighten(007)', 'fill dark(5)', 'fill lightens(5)', 'filldarken(5)', 'fillx', 'fil', 'fi ll', 'xfill', 'fill darken(5)darken(5)', 'fill darken(5) lighten(5)',
  'fıll', 'ﬁll', 'fill darken(5)', 'fill lighten(2)', ' fill', 'fill ', 'fill﻿', '﻿fill', 'fill ', 'fill\u0085', '\u0085fill', 'fill᠎', 'fill darkeN(5)', 'fill LIGHTEN(5)', 'fill ligHten(5)',
  '', ' ', 'blue', '#abc', 'fill\ndarken(5)\n\n', 'fill darken(5) ', 'fill darken(5)​', 'fill ｄarken(5)', 'fill darKen(5)', 'fill darKen(5)',
];
const nums = ['0', '1', '0.5', '-0.5', '0.0005', '0.0004', '0.0015', '12.3456', '1e21', '1e-7', 'NaN', 'Infinity', '-Infinity', '-0', '0.1', '0.2', '0.30000000000000004', '100', '33.335', '0.999', '0.9995', '123456789.123456789', '5e-324', '2.5', '-2.5', '1.005', '0.000049999'];
const texts = ['a', 'a b', ' a  b ', 'é', '😀', 'a b', 'a b', 'a﻿b', 'a᠎b', '', ' ', 'x1', 'x12', '12', 'a1b2', 'confidential1', 'donotcopy2', 'a&b', '"q"', '<x>', 'a\tb\nc\rd', '\u0085x\u0085', 'a​b', 'a　b', 'a b', 'abc123 ', 'x१२'];
export const INPUTS = [...new Set([...rels, ...cts, ...docs, ...paths, ...fills, ...nums, ...texts, ...BASE.slice(0, 40)])];
