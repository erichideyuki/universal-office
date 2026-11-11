// The regex sites of the D2b-F4 remainder (watermark, cover pages, header/footer templates:
// universal-write.ts `watermarkRun`, `addPartImage`, `coverParaIndexes`, `coverEnv`,
// `insertCoverPage`, `templateEnv`, `readTemplatePart`, `insertPageNumberHere`) against V8, checked by
// tests/regex_d2b_f4.rs. A row is `[id, function, 'fn']` (see js/regex-diff.mjs); the TS expressions
// are copied as written.
import { INPUTS as BASE } from './templates.mjs';

// universal-write.ts:11892
const pictSpan = (x) => /<w:pict\b[\s\S]*<\/w:pict>/.exec(x)?.[0] ?? null;
// universal-write.ts:11880
const hfName = (n) => /^word\/(header|footer)\d*\.xml$/.test(n);
// universal-write.ts:11881-11882
const spidOf = (used) => {
  let spid = 1025;
  for (const mm of used.matchAll(/_x0000_s(\d+)/g)) spid = Math.max(spid, parseInt(mm[1], 10) + 1);
  return spid;
};
// universal-write.ts:11973
const coverGallery = (w) => /<w:docPartGallery\s+w:val="Cover Pages"/.test(w);
// universal-write.ts:12010
const prop = (xml, tag) => {
  const m = new RegExp(`<${tag}[^>]*>([^<]*)<`).exec(xml);
  return m ? m[1].replace(/&lt;/g, '<').replace(/&gt;/g, '>').replace(/&quot;/g, '"').replace(/&amp;/g, '&').trim() : '';
};
const propRaw = (xml, tag) => {
  const m = new RegExp(`<${tag}[^>]*>([^<]*)<`).exec(xml);
  return m ? m[1].replace(/&lt;/g, '<').replace(/&gt;/g, '>').replace(/&quot;/g, '"').replace(/&amp;/g, '&') : null;
};
// universal-write.ts:12058
const bracketed = (t) => /^\[.*\]$/.test(t);
// universal-write.ts:11502, 12005
const stripHash = (s) => s.replace(/^#/, '');
// universal-write.ts:11587 and :11552
const numPagesCi = (f) => /NUMPAGES/i.test(f);
const pageCi = (f) => /PAGE/i.test(f);
const pageCs = (f) => /PAGE/.test(f);
// universal-write.ts:11905-11910 (addPartImage)
const hasRid = (rels) => { let k = 1; while (new RegExp(`Id="rId${k}"`).test(rels)) k++; return k; };
const defaultPng = (ct) => new RegExp(`<Default Extension="png"`, 'i').test(ct);
const defaultJpeg = (ct) => new RegExp(`<Default Extension="jpeg"`, 'i').test(ct);

/** [site id, the TS expression, operation]. */
export const SITES = [
  ['d2b-f4.pict_span', pictSpan, 'fn'],
  ['d2b-f4.hf_part_name', hfName, 'fn'],
  ['d2b-f4.next_spid', (x) => { const r = spidOf(x); return Number.isFinite(r) ? String(r) : 'inf'; }, 'fn'],
  ['d2b-f4.cover_gallery', coverGallery, 'fn'],
  ['d2b-f4.prop_title', (x) => prop(x, 'dc:title'), 'fn'],
  ['d2b-f4.prop_company', (x) => prop(x, 'Company'), 'fn'],
  ['d2b-f4.prop_raw_creator', (x) => propRaw(x, 'dc:creator'), 'fn'],
  ['d2b-f4.bracketed', bracketed, 'fn'],
  ['d2b-f4.strip_hash', stripHash, 'fn'],
  ['d2b-f4.numpages_ci', numPagesCi, 'fn'],
  ['d2b-f4.page_ci', pageCi, 'fn'],
  ['d2b-f4.page_cs', pageCs, 'fn'],
  ['d2b-f4.next_rid', hasRid, 'fn'],
  ['d2b-f4.default_png', defaultPng, 'fn'],
  ['d2b-f4.default_jpeg', defaultJpeg, 'fn'],
];

const CT = (inner) => `<?xml version="1.0"?><Types xmlns="x">${inner}</Types>`;
const rels = (...ids) => `<Relationships>${ids.map((i) => `<Relationship Id="${i}" Target="t"/>`).join('')}</Relationships>`;
export const INPUTS = [
  ...BASE,
  // pict
  '<w:r><w:pict w14:anchorId="1"><v:shape/></w:pict></w:r>', '<w:pict>a</w:pict>b<w:pict>c</w:pict>', '<w:pictx>a</w:pictx></w:pict>', '<w:pictx></w:pict><w:pict></w:pict>',
  '<w:pict', '<w:pict>', '</w:pict>', '</w:pict><w:pict>', '<w:pict></w:pict>', '<w:pict></w:pict></w:pict>x', '<w:pict_></w:pict>', '<w:pict-></w:pict>', '<w:pict:x></w:pict>',
  '<w:pict\n>x\n</w:pict>', '<w:pict></w:pict>\n<w:pict>\n</w:pict>\n', '<w:pict', '<w:pict</w:pict>', '<w:pictéx></w:pict>', '<w:pict é></w:pict>',
  // part names
  'word/header1.xml', 'word/footer.xml', 'word/header.xml', 'word/header12.xml', 'word/header1a.xml', 'word/headers1.xml', 'word/header1.xml\n', 'xword/header1.xml', 'word/header1.xmlx',
  'word/header٣.xml', 'word/_rels/header1.xml.rels', 'Word/header1.xml', 'word/footer99999999999.xml', 'word/header1.XML', 'word//header1.xml', '/word/header1.xml', '',
  // spids
  '_x0000_s1026', '_x0000_s1', '_x0000_s', '_x0000_sa', 'x_x0000_s2000y_x0000_s3000', '_x0000_s1030_x0000_s1040', '_x0000_s_x0000_s5000', '_x0000_s99999999999999999999',
  '_x0000_s1e3', '_x0000_s0001025', '_x0000_s٣', '_x0000_S2000', '_X0000_s2000', 'o:spid="_x0000_s1027" id="PowerPlusWaterMarkObject1" o:spid="_x0000_s2050"',
  '_x0000_s_x0000_s', '_x0000_s1_x0000_s2_x0000_s', '__x0000_s3000', '_x0000_s1024', '_x0000_s1025', '_x0000_s4294967295', '_x0000_s9007199254740993',
  '_x0000_s' + '9'.repeat(400),
  // cover gallery
  '<w:sdt><w:sdtPr><w:docPartObj><w:docPartGallery w:val="Cover Pages"/>', '<w:docPartGallery  w:val="Cover Pages"', '<w:docPartGallery\tw:val="Cover Pages"', '<w:docPartGallery\nw:val="Cover Pages"',
  '<w:docPartGallery w:val="Cover Pages"', '<w:docPartGallery w:val="Cover Pages"', '<w:docPartGallery﻿w:val="Cover Pages"', '<w:docPartGallery​w:val="Cover Pages"',
  '<w:docPartGallery w:val="Cover Pages (x)"', '<w:docPartGallery w:val="cover pages"', '<w:docPartGallery w:val=\'Cover Pages\'', '<w:docPartGallery w:val="Cover Pages', '<w:docPartGallery w:val="Page Numbers"/>',
  '<w:docPartGallery w:val="Page Numbers"/><w:docPartGallery w:val="Cover Pages"', '<w:docPartGallery' , '<w:docPartGallery/>', '<w:docPartGalleryX w:val="Cover Pages"', '<w:docPartGallery', '<w:docPartGallery　w:val="Cover Pages"',
  '<w:docPartGallery w:val="Cover Pages"<w:docPartGallery', '<w:docPartGallery x<w:docPartGallery w:val="Cover Pages"',
  // core.xml / app.xml properties
  '<dc:title>My title</dc:title>', '<dc:title></dc:title>', '<dc:title/>', '<dc:title/>tail<', '<dc:title>  padded  </dc:title>', '<dc:title>a &amp; b &lt;c&gt; &quot;d&quot; &amp;lt;</dc:title>',
  '<dc:title xml:lang="pt">Titulo</dc:title>', '<dc:title>no end', '<dc:title>', '<dc:title', '<dc:title>x</dc:title><dc:title>y</dc:title>', '<dc:titleX>z</dc:titleX><dc:title>y</dc:title>',
  '<dc:title <dc:title>x<', '<dc:title a="b>c">d<', '<dc:title\n>nl\n<', '<dc:title>\t\n tab  ﻿</dc:title>', '<dc:title>&amp;amp;</dc:title>', '<dc:title>&#39;</dc:title>',
  '<dc:title>a<b>c</b></dc:title>', '<dc:title>é𝒜</dc:title>', '<dc:title> x </dc:title>', '<dc:title>x</dc:title', '<dc:title>x', '<dc:title><', '<dc:title>><',
  // linear-time shapes (no '>' / no '<' after the '>'): the matcher breaks instead of rescanning
  '<dc:title'.repeat(3000), '<dc:title'.repeat(3000) + '>', '<dc:title>'.repeat(1) + 'x', '<Company'.repeat(3000) + '>', '<dc:title '.repeat(2000) + '>x<',
  '<dc:creator>Eric</dc:creator>', '<dc:creator/>', '<dc:creator>', '<cp:coreProperties><dc:creator>A &amp; B</dc:creator></cp:coreProperties>', '<dc:creator>  </dc:creator>',
  '<Company>ACME</Company>', '<Company/>', '<Company></Company>', '<Company>ACME', '<CompanyX>a</CompanyX><Company>b</Company>', '<Properties><Company> Foo &lt;Bar&gt; </Company></Properties>',
  '<Company a="1">c</Company>', '<Company>c', '<Company>>x<', '<Company>\n</Company>',
  '<dc:title>1</dc:title><Company>2</Company><dc:creator>3</dc:creator>',
  // bracketed
  '[Title]', '[]', '[', ']', '[a]]', '[[a]', '[ a ]', '[a\nb]', '[a\rb]', '[a b]', '[a b]', '[a\u0085b]', '[a\u000bb]', '[a\u000cb]', '[a]\n', '\n[a]', ' [a]', '[a] ', '[Título do documento]', '[𝒜]', '[é]',
  '[\n]', '[]]', '[ ]', '[a]b]', '[ab', 'ab]', '[a] ',
  // strip hash
  '#156082', '156082', '##AA', '# ', '#', ' #aa', '\n#a', '#\n', 'a#b',
  // fields
  ' PAGE   \\* MERGEFORMAT ', ' NUMPAGES   \\* MERGEFORMAT ', 'page', 'numpages', 'NumPages', 'PAGEREF x', 'SECTIONPAGES', 'NUM PAGES', 'PAG E', 'ſ', 'ＰＡＧＥ', 'pAgE', 'PAGİE', 'NUMPAGeS', 'NUMPAGES', 'PAGES',
  'paKe', 'PAGEı', 'NUMPAGEſ', 'numpageſ',
  // relationships and content types (addPartImage)
  rels(), rels('rId1'), rels('rId1', 'rId2'), rels('rId2'), rels('rId1', 'rId3'), rels('rId1', 'rId2', 'rId3', 'rId4'), rels('rId10'), rels('rId1', 'rId2', 'rId3', 'rId4', 'rId5', 'rId6', 'rId7', 'rId8', 'rId9', 'rId10'),
  '<Relationship Id="rId1x"/>', '<Relationship xId="rId1"/>', '<Relationship Id=\'rId1\'/>', '<Relationship Id = "rId1"/>', '<Relationship Id="rid1"/>', '<Relationship Id="rId01"/>', 'Id="rId1"Id="rId2"',
  '<Relationship Target="Id=&quot;rId1&quot;"/>', '<Relationship Target=\'Id="rId1"\'/>', 'ID="rId1"',
  CT(''), CT('<Default Extension="png" ContentType="image/png"/>'), CT('<Default Extension="PNG" ContentType="image/png"/>'), CT('<Default Extension="jpeg" ContentType="image/jpeg"/>'),
  CT('<Default Extension="jpg" ContentType="image/jpeg"/>'), CT('<Default Extension="xml" ContentType="application/xml"/><Default Extension="rels"/>'), CT('<Default  Extension="png"/>'), CT('<Default Extension="png "/>'),
  CT('<Default Extension="pngx"/>'), CT('<Default Extension="png"'), CT('<default extension="png"/>'), CT('<DEFAULT EXTENSION="JPEG"/>'), CT('<Default Extension=\'png\'/>'), CT('<Override Extension="png"/>'),
  '<Default Extension="png"', '<Default Extension="PnG"', '<Default Extension="Jpeg"/>', 'x<Default Extension="jpeg"y', '<Default\nExtension="png"/>', '<Default Extension="png"/><Default Extension="jpeg"/>',
];
