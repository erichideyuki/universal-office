// Inputs of the strict-mode oracle (strict_oracle.cs): hand-written cases and a seeded random
// corpus of mostly well-formed, namespace-heavy XML with injected faults.
//   node gen_strict_inputs.mjs && dotnet run strict_oracle.cs -- strict_inputs.txt strict_oracle.txt
import { writeFileSync } from 'node:fs';

const W = 'http://schemas.openxmlformats.org/wordprocessingml/2006/main';
const hand = [
  '<a/>', '<a></a>', '<?xml version="1.0" encoding="UTF-8" standalone="yes"?>\r\n<a b="1">t</a>', '﻿<a/>',
  '<!DOCTYPE a><a/>', '<!DOCTYPE a [<!ENTITY e "z">]><a>&e;</a>', '<a>&e;</a>', '<a>&amp;&lt;&gt;&quot;&apos;</a>',
  '<a>&#65;&#x41;&#X41;</a>', '<a>&#1;</a>', '<a>&#xD83D;&#xDE00;</a>', '<a>&#x1F600;</a>', '<a>&#0;</a>', '<a>&#xFFFE;</a>',
  '<a>&#x110000;</a>', '<a>&#;</a>', '<a>& b</a>', '<a b="&lt;"/>', '<a b="<"/>', '<a b="1" b="2"/>',
  `<a xmlns:p="u" xmlns:q="u" p:x="1" q:x="2"/>`, `<a xmlns:p="u" p:x="1" x="2"/>`, '<p:a/>', '<a p:b="1"/>',
  `<p:a xmlns:p="u"></p:a>`, `<p:a xmlns:p="u"></q:a>`, '<a></b>', '<a><b></a></b>', '<a>x</a>y', '<a/><b/>',
  'x<a/>', '<a/>  ', '<a/><!-- c --><?pi x?>', '<?pi x?><!-- c --><a/>', '<a><?xml x?></a>', '<?xml version="1.0"?><?xml version="1.0"?><a/>',
  ' <?xml version="1.0"?><a/>', '<a><!-- a--b --></a>', '<a><!-- a- --></a>', '<a><!----></a>', '<a>]]></a>', '<a>]]&gt;</a>',
  '<a><![CDATA[<&]]>x</a>', '<a><![CDATA[a\r\nb\rc]]></a>', '<a>a\r\nb\rc&#13;</a>', '<a v="x\r\ny\tz&#10;&#9;"/>',
  '<a b="1"c="2"/>', '<a b = "1" />', "<a b='\"'/>", '<1a/>', '<a:b:c/>', '<:a/>', '<a: xmlns:a="u"/>', '<a\u0001/>', '<a>\u0001</a>',
  '<a>￿</a>', '<a xmlns:xml="http://www.w3.org/XML/1998/namespace"/>', '<a xmlns:xml="u"/>', '<a xmlns:xmlns="u"/>',
  '<a xmlns:p=""/>', '<a xmlns=""/>', '<a xmlns="u"><b xmlns=""/></a>', '<a xml:space="preserve"/>', '<a xmlns:p="http://www.w3.org/XML/1998/namespace"/>',
  `<w:document xmlns:w="${W}"><w:body><w:p><w:r><w:t xml:space="preserve"> a </w:t></w:r></w:p></w:body></w:document>`,
  '<a>' + '<b>'.repeat(255) + '</b>'.repeat(255) + '</a>', '<a>' + '<b>'.repeat(256) + '</b>'.repeat(256) + '</a>',
  '<a>' + '<b>'.repeat(255) + 'x' + '</b>'.repeat(255) + '</a>', '<a>' + '<b>'.repeat(256) + 'x' + '</b>'.repeat(256) + '</a>',
  '<a>' + '<b>'.repeat(256) + '<c/>' + '</b>'.repeat(256) + '</a>', '<a>' + '<b>'.repeat(255) + '<!--c-->' + '</b>'.repeat(255) + '</a>',
  '<a>' + '<b>'.repeat(256) + '<!--c-->' + '</b>'.repeat(256) + '</a>', '<a>' + '<b>'.repeat(256) + ' ' + '</b>'.repeat(256) + '</a>',
  '<a', '<a b', '<a b=', '<a b="x', '<a>', '<a><!--', '<a><![CDATA[', '<a><?p', '', '   ', '<!-- only -->', '<a>&amp</a>',
  '<a>&#x41</a>', '<a b=x/>', '<a xml:space=" preserve "/>', '<a xml:space="Preserve"/>', '<a xml:space=""/>', '<a xml:lang="zz zz"/>', '<a xml:foo="1"/>', '<a/ >', '<a></a >', '</a>', '<a><!DOCTYPE b></a>', '<a><!ELEMENT></a>', '<a>é😀</a>',
];

let seed = 0x2545f491;
const rnd = (n) => { seed ^= seed << 13; seed >>>= 0; seed ^= seed >>> 17; seed ^= seed << 5; seed >>>= 0; return seed % n; };
const pick = (a) => a[rnd(a.length)];
const names = ['a', 'b', 'w:p', 'w:r', 'p:x', 'q:y', 'c', 'x:z', 'é', 'a', 'w:p'];
const attrNames = ['v', 'w:val', 'p:x', 'q:x', 'xml:space', 'xmlns:p', 'xmlns:q', 'xmlns', 'xmlns:w', 'r', 'v', 'w:val', 'r'];
// faults are rare so that most trees are well-formed
const vals = ['1', '', 'a b', 'u', 'x\r\ny\tz', '&amp;', '&#10;', '&#xD83D;&#xDE00;', W, 'preserve', '2', 'v', '1', 'u', '&bogus;'];
const texts = ['', 'x', ' ', '\r\n', 'a&amp;b', '&#x41;', '<![CDATA[c\r\nd]]>', '<!--k-->', '<?p q?>', 'y', ' z ', 'x', '', '&#1;', ']]>'];
function tree(depth) {
  const name = pick(names);
  let a = '';
  const na = rnd(4);
  for (let i = 0; i < na; i++) a += ` ${pick(attrNames)}=${pick(['"', "'"]).replace(/./, (qq) => qq + pick(vals) + qq)}`;
  if (depth > 2 || rnd(4) === 0) return `<${name}${a}/>`;
  let inner = '';
  const nk = rnd(4);
  for (let i = 0; i < nk; i++) inner += rnd(2) ? tree(depth + 1) : pick(texts);
  return `<${name}${a}>${inner}</${rnd(10) ? name : 'zz'}>`;
}
const corpus = [...hand];
for (let i = 0; i < 900; i++) {
  let t = tree(0);
  if (rnd(6) !== 0) t = `<root xmlns:p="u" xmlns:q="v" xmlns:x="u" xmlns:w="${W}">${t}</root>`;
  if (rnd(16) === 0) t = t.slice(0, rnd(t.length));
  if (rnd(8) === 0) t = '<?xml version="1.0"?>' + t;
  if (t.length <= 300) corpus.push(t);
}
writeFileSync(new URL('./strict_inputs.txt', import.meta.url), corpus.map((s) => JSON.stringify(s)).join('\n') + '\n');
console.log(`${corpus.length} inputs`);
