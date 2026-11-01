// Generates ts_oracle.txt: the frozen TS XML parsers (docx-core, sheet-core, slides-core, charts
// canonicalNs over sheet-core) run over hand-written quirk cases and a seeded random corpus.
// The Rust test xml_ts_oracle.rs replays every line with the matching preset.
//
//   node gen_ts_oracle.mjs <repo root with built dist/>   (e.g. a worktree of main 0.9.x)
//
// Line format: preset \t JSON(input) \t dump | ERR | HANG
// dump(el) = name [k=JSON(v),…] {child,…} with strings as JSON; docx-pos adds @s,cs,ce,e and
// ^raw for captured elements. Lone surrogates become U+FFFD (decision in the record).
import { writeFileSync } from 'node:fs';
import vm from 'node:vm';
import { pathToFileURL } from 'node:url';

const root = process.argv[2];
if (!root) throw new Error('usage: node gen_ts_oracle.mjs <repo root>');
const imp = (p) => import(pathToFileURL(`${root}/${p}`).href);
const docx = await imp('write/packages/docx-core/dist/xml.js');
const sheet = await imp('sheet/packages/sheet-core/dist/xml.js');
const slides = await imp('slides/packages/slides-core/dist/xml.js');
const charts = await imp('shared/charts/dist/xml.js');

const fffd = (s) => s.replace(/[\uD800-\uDBFF](?![\uDC00-\uDFFF])|(?<![\uD800-\uDBFF])[\uDC00-\uDFFF]/g, '�');
const q = (s) => JSON.stringify(fffd(s));
function dump(el, pos) {
  let out = el.name + '[' + Object.entries(el.attrs).map(([k, v]) => `${k}=${q(v)}`).join(',') + ']';
  if (pos) out += `@${el.s},${el.cs},${el.ce},${el.e}` + (el.raw !== undefined ? '^' + q(el.raw) : '');
  return out + '{' + el.children.map((c) => (typeof c === 'string' ? q(c) : dump(c, pos))).join(',') + '}';
}

// sheet-core loops forever on some unclosed constructs (gate 1, G5: an error in Rust)
const ctx = vm.createContext({ sheet, charts, dump, q });
function guarded(expr, input) {
  ctx.input = input;
  try {
    return vm.runInContext(expr, ctx, { timeout: 200 });
  } catch (e) {
    return /timed out/.test(String(e.message)) ? 'HANG' : 'ERR';
  }
}
const run = (f) => { try { return f(); } catch { return 'ERR'; } };

const presets = {
  docx: (x) => run(() => dump(docx.parseXml(x), false)),
  'docx-pos': (x) => run(() => dump(docx.parseXml(x, { positions: true }), true)),
  sheet: (x) => guarded('dump(sheet.parseXml(input), false)', x),
  slides: (x) => run(() => dump(slides.parseXml(x), false)),
  'sheet-chart': (x) => guarded('dump(charts.canonicalNs(sheet.parseXml(input)), false)', x),
};

const W = 'http://schemas.openxmlformats.org/wordprocessingml/2006/main';
const MC = 'http://schemas.openxmlformats.org/markup-compatibility/2006';
const C = 'http://schemas.openxmlformats.org/drawingml/2006/chart';
const CS = 'http://purl.oclc.org/ooxml/drawingml/chart';
const SML = 'http://schemas.openxmlformats.org/spreadsheetml/2006/main';
const hand = [
  '<a/>', '<a></a>', '  <a>x</a>  trailing', '<?xml version="1.0"?>\r\n<a b="1">t</a>',
  '<a v="&#X41;|&#1a;|&#xD83D;&#xDE00;|&#xD83D;x|&#xDE00;|&#x;|&#123abc;|&amp;amp;|&lt;&gt;&quot;&apos;"/>',
  '<a v="&constructor;&toString;&__proto__;&hasOwnProperty;"/>', '<a>&constructor;</a>',
  '<a v="&#a;"/>', '<a>&#ff;</a>', '<a v="&#x110000;"/>', '<a>&#99999999999;&#65;</a>', '<a>&#xFFFFFFFF;&#x41;</a>',
  '<a>&#0;&#x0;</a>', '<a>&#xD83D;&#xD83D;&#xDE00;&#xDE00;</a>', '<a>&#xd83d;<![CDATA[x]]>&#xde00;</a>',
  '<a b="1" c="2" b="3" 2="x" 1="y" 01="z" 4294967295="m" 4294967294="n"/>',
  '<a> 　﻿</a>', '<a b="1" c="2"/>', '﻿<a/>', '  <a/>',
  '<!DOCTYPE x [<!ENTITY e "z">]><a>&e;</a>', '<!DOCTYPE a><a>&e;</a>', '<!-- c --><?pi?><a/>',
  '<a><?pi x?></a>', '<a><?pi?></a>', '<a><![CDATA[]]></a>', '<a><![CDATA[x\r\ny\rz]]></a>',
  '<a>a\r\nb\rc&#13;&#10;d</a>', '<a v="x\r\ny\tz&#10;w&#9;\n"/>', '<a>  </a>', '<a> <b/> </a>',
  '<a><b></c></a>', '<a><b>x</b', '<a><b/><!-- sem fim', '<a><![CDATA[ sem fim', '<a></a',
  '<a', '<a b', '<a b=', '<a b="x', '<a b=x/>', '<a/ >', '<a /x>', '', '   ', 'text', '<>x</>',
  '<a b="1"c="2"/>', "<a b='1\"'/>", '<a=b>x</a=b>', '<a b>=1"/>', '<a><b><c/></b></a>',
  '<a><!--x--y--></a>', '<a>]]></a>', '<a>x<!DOCTYPE y>z</a>', '<a>&#x41</a>', '<a>&;&#;&amp</a>',
  '<a>&&amp;&#38;&#x26;</a>', '<a>&#1F;&#12x;&#xx41;</a>',
  `<w:document xmlns:w="${W}"><w:body><w:p><w:r><w:t xml:space="preserve"> a </w:t></w:r></w:p></w:body></w:document>`,
  `<r xmlns:ve="${MC}"><ve:Choice ve:x="1" mc:x="2" Requires="w"/><ve:Fallback/></r>`,
  `<r xmlns:ve="${MC}"><x xmlns:ve="urn:other"><ve:Choice/></x><ve:Choice/></r>`,
  `<w:document xmlns:w="${W}" xmlns:ww="${W}"><ww:body ww:a="1" w:a="2"/></w:document>`,
  `<x:document xmlns:x="${W}"><x:body><x:p/></x:body></x:document>`,
  `<a xmlns:w="urn:not-word"><w:p/></a>`, `<w:drawing a="1"><x/>t</w:drawing>`,
  `<r><w:drawing><wp:inline/></w:drawing><w:drawing/></r>`,
  `<x:worksheet xmlns:x="${SML}" xmlns:r="r"><x:sheetData><x:row r="1"><x:c r="A1"><x:v>1</x:v></x:c></x:row></x:sheetData></x:worksheet>`,
  `<x:a xmlns:y="${SML}" xmlns:x="${SML}"><y:b/><x:c/><z:d/></x:a>`,
  `<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="t" Target="a.xml"/></Relationships>`,
  `<chartSpace xmlns="${C}" xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main"><chart><plotArea a:x="1" y="2"/></chart></chartSpace>`,
  `<ns0:chartSpace xmlns:ns0="${CS}"><ns0:chart ns0:v="1" v="2"/></ns0:chartSpace>`,
  `<c:chartSpace xmlns:c="${C}"><c:chart/><d xmlns="${C}"><e/></d><f/></c:chartSpace>`,
  `<k xmlns="${C}" xmlns:="${C}"><:m :n="1"/></k>`,
  `<x:chartSpace xmlns:x="${SML}" xmlns="${C}"><x:chart/></x:chartSpace>`,
  `<a xmlns="${C}"><b xmlns=""><c/></b></a>`,
];

// seeded random corpus: token soup (mostly malformed) and random trees with quirks
let seed = 0x9e3779b9;
const rnd = (n) => { seed ^= seed << 13; seed >>>= 0; seed ^= seed >>> 17; seed ^= seed << 5; seed >>>= 0; return seed % n; };
const pick = (a) => a[rnd(a.length)];
const tokens = ['<', '>', '/', '/>', '</', 'a', 'w:p', 'x:', ':', ' ', '\t', '\r\n', '\r', '\n', '=', '"', "'", '&amp;', '&lt;',
  '&#x41;', '&#1a;', '&#a;', '&#xD83D;', '&#xDE00;', '&bogus;', '&', ';', '#', '<!--', '-->', '<![CDATA[', ']]>', '<?', '?>',
  '<!', ' ', '　', 'é', '😀', 'b="1"', "c='2'", `xmlns:ve="${MC}"`, 've:Choice', `xmlns="${C}"`, `xmlns:x="${SML}"`, 'x:c'];
const soup = () => { let s = ''; const n = 1 + rnd(14); for (let i = 0; i < n; i++) s += pick(tokens); return s; };
const names = ['a', 'b', 'w:p', 'w:r', 've:Choice', 'mc:Fallback', 'x:row', 'c', 'chart', 'w:drawing', '1', 'ns0:v'];
const texts = ['', 'x', ' ', '\r\n', 'a&amp;b', '&#x41;', '&#1a;', '&#xD83D;&#xDE00;', ' ', '<![CDATA[c\r\nd]]>', '<!--k-->', '<?p q?>', '&bogus;'];
const attrVals = ['1', '', 'a b', 'x\r\ny\tz', '&amp;', '&#10;', '&#xD83D;&#xDE00;', MC, C, SML, W];
const attrNames = ['v', 'w:val', 've:x', 'mc:x', 'r', '2', '1', `xmlns:ve`, 'xmlns', 'xmlns:x', 'xmlns:c', 'a:x'];
function tree(depth) {
  const name = pick(names);
  let a = '';
  const na = rnd(4);
  for (let i = 0; i < na; i++) a += ` ${pick(attrNames)}${pick(['=', ' = '])}${pick(['"', "'"]).replace(/./, (qq) => qq + pick(attrVals) + qq)}`;
  if (depth > 2 || rnd(4) === 0) return `<${name}${a}${pick(['/>', ' />'])}`;
  let inner = '';
  const nk = rnd(4);
  for (let i = 0; i < nk; i++) inner += rnd(2) ? tree(depth + 1) : pick(texts);
  return `<${name}${a}>${inner}</${rnd(8) ? name : 'zz'}>`;
}
const corpus = [...hand];
for (let i = 0; i < 500; i++) corpus.push(soup());
for (let i = 0; i < 500; i++) {
  let t = tree(0);
  if (rnd(10) === 0) t = t.slice(0, rnd(t.length)); // truncated
  if (rnd(10) === 0) t = '<?xml version="1.0"?>' + t;
  if (t.length <= 260) corpus.push(t);
}

const lines = [];
const ascii = (s) => /^[\x00-\x7f]*$/.test(s);
for (const x of corpus) {
  for (const [p, f] of Object.entries(presets)) {
    if (p === 'docx-pos' && !ascii(x)) continue; // positions are UTF-16 indices in the TS
    lines.push(`${p}\t${JSON.stringify(x)}\t${f(x)}`);
  }
}
writeFileSync(new URL('./ts_oracle.txt', import.meta.url), lines.join('\n') + '\n');
console.log(`${lines.length} lines, ${lines.filter((l) => l.endsWith('\tHANG')).length} HANG`);
