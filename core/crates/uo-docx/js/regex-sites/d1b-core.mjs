// The regex sites of the D1b writer core (src/write/matchers.rs, parts.rs, rev.rs, lossless.rs) against
// V8, checked by tests/regex_d1b_core.rs. The dynamic patterns are built here exactly as the TS builds
// them, with metacharacter-free values (a value holding a metacharacter is matched as a literal in Rust:
// accepted by name, G9); the composite rows run the TS code copied as written. Rows return strings or
// booleans, never positions (UTF-16 indices would differ from Rust's byte offsets).
import { Rng } from '../../../uo-testkit/js/testkit.mjs';

const composite = (f) => [{ [Symbol.replace]: (s) => f(s) }, ''];
const IDS = ['1', '12', '-3'];
const PARA = ['0A1B', 'FF', '12'];
const DUR = ['d1', 'D2', 'x-y'];

// serialize.ts:1445–1451 (verbatim)
function dropCommentMarks(xml, gone) {
  if (gone.size === 0) return xml;
  const ids = [...gone].join('|');
  return xml
    .replace(new RegExp(`<w:commentRange(?:Start|End)\\b[^>]*\\bw:id="(?:${ids})"[^>]*/>`, 'g'), '')
    .replace(new RegExp(`<w:r\\b[^>]*>(?:(?!</w:r>)[\\s\\S])*?<w:commentReference\\b[^>]*\\bw:id="(?:${ids})"[^>]*/>(?:(?!</w:r>)[\\s\\S])*?</w:r>`, 'g'), '');
}

// serialize.ts:607–636 (the match part of fieldCodeXml)
function codeParts(code) {
  const out = [];
  let depth = 0, from = 0;
  for (let i = 0; i < code.length; i++) {
    const c = code[i];
    if (c === '{') { if (depth++ === 0) { if (i > from) out.push({ text: code.slice(from, i) }); from = i + 1; } }
    else if (c === '}' && depth > 0 && --depth === 0) { out.push({ nested: code.slice(from, i) }); from = i + 1; }
  }
  if (from < code.length) out.push({ text: code.slice(from) });
  return out;
}
function fieldGroups(code, flat) {
  const parts = codeParts(code.trim());
  const reSrc = parts.map((p) => (p.text !== undefined ? p.text.replace(/[.*+?^${}()|[\]\\]/g, '\\$&') : '([\\s\\S]*?)')).join('');
  const m = new RegExp(`^${reSrc}$`).exec(flat.trim());
  return m ? JSON.stringify(m.slice(1)) : 'null';
}

// rev-xml.ts:83–92, 119–130 (verbatim)
function maxId(x) {
  let max = 0;
  for (const m of x.matchAll(/\bw:id="(-?\d+)"/g)) max = Math.max(max, Number(m[1]));
  return String(max + 1);
}
function ensureRevisionNamespaces(open, rootName) {
  const at = open.indexOf(`<${rootName}`);
  if (at < 0) return open;
  const end = open.indexOf('>', at);
  let tag = open.slice(at, end);
  if (!/\sxmlns:w16du=/.test(tag)) tag += ' xmlns:w16du="U"';
  if (!/\sxmlns:mc=/.test(tag)) tag += ' xmlns:mc="M"';
  const ign = /\smc:Ignorable="([^"]*)"/.exec(tag);
  if (!ign) tag += ' mc:Ignorable="w16du"';
  else if (!ign[1].split(/\s+/).includes('w16du')) tag = tag.replace(ign[0], () => ` mc:Ignorable="${ign[1] ? `${ign[1]} ` : ''}w16du"`);
  return open.slice(0, at) + tag + open.slice(end);
}
function readTrackRevisions(xml) {
  const m = /<w:trackRevisions\b([^>]*?)\/?>/.exec(xml);
  return !!m && !/w:val="(?:false|0|off)"/.test(m[1]);
}

// serialize.ts:1461–1466, 1151, 1490–1498 (numbering with ids 1001/1002, abstracts and nums as markers)
function freeNumIds(xml) {
  const used = new Set([...(xml ?? '').matchAll(/w:(?:abstractNumId|numId)(?: w:val)?="(\d+)"/g)].map((m) => Number(m[1])));
  let n = 1001;
  while (used.has(n) || used.has(n + 1)) n += 2;
  return `${n},${n + 1}`;
}
const gridOf = (raw) => [...raw.matchAll(/<w:gridCol\b[^>]*w:w="(\d+)"/g)].map((m) => m[1]).join(',');
function mergeNumbering(xml) {
  if (new RegExp('<w:num w:numId="1001">').test(xml)) return xml;
  const firstNum = xml.search(/<w:num\b/);
  const withAbs = firstNum >= 0 ? xml.slice(0, firstNum) + '[A]' + xml.slice(firstNum) : xml.replace(/<\/w:numbering>\s*$/, '[A]</w:numbering>');
  const tail = withAbs.search(/<w:numIdMacAtCleanup\b|<\/w:numbering>\s*$/);
  return withAbs.slice(0, tail) + '[N]' + withAbs.slice(tail);
}

// serialize.ts:1419–1442 (verbatim but for the URIs, shortened)
function withNamespaces(open, rootName, content = '') {
  const at = open.indexOf(`<${rootName}`);
  if (at < 0) return open;
  const end = open.indexOf('>', at);
  let tag = open.slice(at, end);
  const need = [
    ['w', 'W'], ['r', 'R'], ['wp', 'WP'],
    ...(content.includes('<w15:') ? [['w15', 'W15']] : []),
    ...([
      ['v', 'V', /<v:/], ['o', 'O', /\so:\w+=|<o:/], ['w10', 'W10', /<w10:/], ['w14', 'W14', /\sw14:\w+=|<w14:/],
      ['mc', 'MC', /<mc:/], ['wps', 'WPS', /<wps:|Requires="wps"/], ['a', 'A', /<a:/], ['wp14', 'WP14', /<wp14:|\swp14:\w+=/],
    ]).filter(([pfx, , re]) => re.test(content) && !content.includes(`xmlns:${pfx}=`)).map(([pfx, uri]) => [pfx, uri]),
  ];
  for (const [pfx, uri] of need) if (!new RegExp(`\\sxmlns:${pfx}=`).test(tag)) tag += ` xmlns:${pfx}="${uri}"`;
  return open.slice(0, at) + tag + open.slice(end);
}

const relTags = (x) => [...x.matchAll(/<Relationship\b[^>]*>/g)].map((m) => /\bId="([^"]+)"/.exec(m[0])?.[1] ?? '-').join(',');

export const SITES = [
  ['drop.range', [new RegExp(`<w:commentRange(?:Start|End)\\b[^>]*\\bw:id="(?:${IDS.join('|')})"[^>]*/>`, 'g'), ''], 'replace'],
  ['drop.ref', [new RegExp(`<w:r\\b[^>]*>(?:(?!</w:r>)[\\s\\S])*?<w:commentReference\\b[^>]*\\bw:id="(?:${IDS.join('|')})"[^>]*/>(?:(?!</w:r>)[\\s\\S])*?</w:r>`, 'g'), ''], 'replace'],
  ['drop.all', composite((x) => dropCommentMarks(x, new Set(IDS))), 'replace'],
  ['cids', [new RegExp(`<w16cid:commentId\\b[^>]*w16cid:paraId="(?:${PARA.join('|')})"[^>]*/>`, 'g'), (m) => `[${/w16cid:durableId="([^"]+)"/.exec(m)?.[1] ?? ''}]`], 'replace'],
  ['cex', [new RegExp(`<w15:commentEx\\b[^>]*w15:paraId="(?:${PARA.join('|')})"[^>]*/>`, 'g'), ''], 'replace'],
  ['cexb', [new RegExp(`<w16cex:commentExtensible\\b[^>]*w16cex:durableId="(?:${DUR.join('|')})"[^>]*?(?:/>|>[\\s\\S]*?</w16cex:commentExtensible>)`, 'g'), ''], 'replace'],
  ['comment.el', [/<w:comment\b[^>]*?\bw:id="(-?\d+)"[^>]*?(?:\/>|>[\s\S]*?<\/w:comment>)/g, (_m, id) => `[${id}]`], 'replace'],
  ['para.ids', composite((x) => [...x.matchAll(/w14:paraId="([0-9A-Fa-f]+)"/g)].map((p) => p[1]).join(',')), 'replace'],
  ['track.rm', [/<w:trackRevisions\b[^>]*?(?:\/>|>[\s\S]*?<\/w:trackRevisions>)/g, ''], 'replace'],
  ['track.on', composite((x) => String(readTrackRevisions(x))), 'replace'],
  ['field.groups', composite((x) => { const [code, flat] = x.split('§'); return flat === undefined ? 'skip' : fieldGroups(code, flat); }), 'replace'],
  ['ci.png', new RegExp('Extension="png"', 'i'), 'test'],
  ['ci.svg', new RegExp('Extension="svg"', 'i'), 'test'],
  ['xmlns.wp', new RegExp('\\sxmlns:wp='), 'test'],
  ['attr.o', /\so:\w+=|<o:/, 'test'],
  ['attr.w14', /\sw14:\w+=|<w14:/, 'test'],
  ['attr.wp14', /<wp14:|\swp14:\w+=/, 'test'],
  ['with.ns', composite((x) => withNamespaces(x, 'w:document', x)), 'replace'],
  ['rev.ns', composite((x) => ensureRevisionNamespaces(x, 'w:document')), 'replace'],
  ['rev.max', composite(maxId), 'replace'],
  ['num.free', composite(freeNumIds), 'replace'],
  ['num.merge', composite(mergeNumbering), 'replace'],
  ['grid', composite(gridOf), 'replace'],
  ['rels.ids', composite(relTags), 'replace'],
  ['rels.path', /^word\/_rels\/[^/]+$/, 'test'],
  ['empty.ind', /^<w:ind\s*\/>$/, 'test'],
  ['empty.pgnum', /^<w:pgNumType\s*\/>$/, 'test'],
  ['pgnum.strip', [/\s+w:(?:fmt|start|chapStyle|chapSep)="[^"]*"/g, ''], 'replace'],
  ['txbx.replace', [/<w:txbxContent\b[^>]*>[\s\S]*?<\/w:txbxContent>/g, (m) => `${/^<w:txbxContent\b[^>]*>/.exec(m)[0]}[X]</w:txbxContent>`], 'replace'],
  ['txbx.count', composite((x) => String((x.match(/<w:txbxContent\b/g) ?? []).length)), 'replace'],
];

// ── inputs: hand-written edge cases and seeded tag soup over the patterns' vocabulary ──
const HAND = [
  '', 'x', '<w:p/>', '>', '/>', '<', '<w:commentRangeStart w:id="1"/>', '<w:commentRangeStart w:id="12"/>', '<w:commentRangeStart w:id="2"/>',
  '<w:commentRangeStartX w:id="1"/>', '<w:commentRangeEnd w:id="-3" />', '<w:commentRangeEnd w:id="-3">', '<w:commentRangeStart  xw:id="1"/>',
  '<w:commentRangeStart a:w:id="1"/>', '<w:commentRangeStart w:id="1" w:id="5"/>', '<w:commentRangeStart w:id="5" w:id="1"/>', '<w:commentRangeStart w:id="1"',
  '<w:r><w:rPr/><w:commentReference w:id="1"/></w:r>', '<w:r><w:commentReference w:id="12"/><w:t>a</w:t></w:r><w:r><w:commentReference w:id="1"/></w:r>',
  '<w:r/><w:commentReference w:id="1"/></w:r>', '<w:rPr><w:commentReference w:id="1"/></w:r>', '<w:r><w:r><w:commentReference w:id="1"/></w:r>',
  '<w:r><w:t>x</w:t></w:r><w:commentReference w:id="1"/></w:r>', '<w:r a="1"><w:commentReference w:id="1"></w:r>', '<w:r><w:commentReference w:id="1"/>',
  '<w16cid:commentId w16cid:paraId="FF" w16cid:durableId="d1"/>', '<w16cid:commentId w16cid:paraId="0A1B" w16cid:durableId=""/>', '<w16cid:commentIdx w16cid:paraId="FF"/>',
  '<w15:commentEx w15:paraId="12" w15:done="0"/>', '<w15:commentEx w15:paraId="12">', '<w16cex:commentExtensible w16cex:durableId="d1"/>',
  '<w16cex:commentExtensible w16cex:durableId="D2"><x/></w16cex:commentExtensible>', '<w16cex:commentExtensible w16cex:durableId="x-y">open',
  '<w:comment w:id="3" w:author="a"><w:p w14:paraId="0A1B"/></w:comment>', '<w:comment w:id="x" w:id="4"/>', '<w:comment w:id="-1"/><w:comments>',
  '<w:trackRevisions/>', '<w:trackRevisions w:val="0"/>', '<w:trackRevisions w:val="off">x</w:trackRevisions>', '<w:trackRevisionsX/><w:trackRevisions>',
  'IF { PAGE } = 1 "a"§IF 3 = 1 "a"', ' { A }{ B } § x y ', 'a{x}b{y}b§a1b2b3b', 'A{B}C§AxD', '{X}§', 'x§x', '{a{b}c}§zz', 'T.*{X}§T.*v', '$1{X}$§$1q$',
  '<Default Extension="PNG" ContentType="x"/>', 'extension="png"', 'Extension="ſvg"', 'Extension="SVG"',
  '<w:document xmlns:w="a">', '<w:document\txmlns:wp="a" xmlns:r="b" xmlns:w="c">', '<w:document xmlns:wp="a"', '<w:documentX>', '<x/><w:document a="1">',
  ' o:spid="1"', '<o:lock/>', 'o:x=', ' o:=', ' w14:paraId="1"', '<w14:x/>', '<wp14:sizeRelH>', ' wp14:anchorId="1"', '<v:shape/><mc:x/><wps:wsp/><a:b/><w15:y/><w10:z/>',
  '<w:document mc:Ignorable="w14"><w:body/>', '<w:document mc:Ignorable=" w16du "><w:body/>', '<w:document mc:Ignorable=""><w:body/>',
  '<w:p w:id="5"/><w:p w:id="-7"/><w:p xw:id="99"/>', '<w:p w:id="12a"/>', '<w:num w:numId="1002"><w:abstractNumId w:val="1001"/></w:num>',
  '<w:numbering><w:abstractNum/><w:num w:numId="3"/><w:numIdMacAtCleanup w:val="1"/></w:numbering>  ', '<w:numbering></w:numbering>\n', '<w:numbering>',
  '<w:numbering><w:numx/></w:numbering>', '<w:gridCol w:w="100"/><w:gridCol w:w="20" w:w="30"/>', '<w:gridColX w:w="5"/><w:gridCol w:w="x"/>',
  '<Relationship Id="rId1" Target="a"/><Relationship Id="" Id="rId2"/><RelationshipX Id="3"/>', 'word/_rels/document.xml.rels', 'word/_rels/a/b.rels', 'word/_rels/',
  '<w:ind/>', '<w:ind  />', '<w:ind\n/>', '<w:ind a="1"/>', '<w:pgNumType />', '<w:pgNumType w:fmt="x"  w:start="2"\tw:chapSep="y" w:x="1"/>',
  '<w:txbxContent><w:p/></w:txbxContent>', '<w:txbxContent a="1"/><w:p/></w:txbxContent>', '<w:txbxContentX><w:txbxContent></w:txbxContent>', '<w:txbxContent>open',
];
const VOCAB = ['<w:r>', '</w:r>', '<w:r ', '<w:rPr>', '<w:commentReference ', '<w:commentRangeStart ', '<w:commentRangeEnd ', 'w:id="1"', 'w:id="12"', 'w:id="2"',
  'w:id="-3"', ' ', '/>', '>', '"', 'x', '<w:t>', '</w:t>', '<w:comment ', '</w:comment>', 'w14:paraId="FF"', '<w16cex:commentExtensible ', 'w16cex:durableId="d1"',
  '</w16cex:commentExtensible>', '<w:txbxContent>', '</w:txbxContent>', '<w:trackRevisions', '</w:trackRevisions>', 'w:val="0"', '\n', '\t', 'é', '😀'];

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

export const INPUTS = [...HAND, ...seeded(0xd1bc0de, 400)];
