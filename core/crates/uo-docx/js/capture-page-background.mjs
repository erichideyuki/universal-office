// Parity capture of the W5 write halves (page-background.ts and watermark.ts) from the frozen TS
// engine (frontier W5 of D1b, facet `page-background`):
//
//   bg       every `w:background` of the corpus plus seeded fills: readPageBackground (R3) then
//            pageBackgroundXml, the text hashed
//   vca      vmlColorAttr over colour texts
//   ocg      oneColorGradientColor2 over degrees
//   rc2      resolveColor2 over (colour 2, colour 1)
//   gif      patternGif over the 48 Word patterns and seeded rows: the BYTES hashed (and the length)
//   wm       watermarkRunXml over text/picture specs, with and without a custom vmlColor
//   box      autoTextBox / explicitTextBox / pictureBox
//   gallery  WATERMARK_GALLERY and WATERMARK_TEXTS (joined)
//   pat      WORD_PATTERNS (prst and the three titles, joined)
//   wr       writePageBackground over synthetic packages (a base package, a document.xml, rels and
//            content types of several shapes, several calls with `prev`): after every call the added
//            record and the digest of the whole package (part names, lengths, SHA-256)
//
//   UO_TS_ROOT=<built checkout of main> node core/crates/uo-docx/js/capture-page-background.mjs
//
// Writes tests/parity/page-background.inputs (`<kind>\t<field>...`, fields escaped with `\\ \n \r \t`)
// and tests/parity/page-background.capture, one record per `<kind>#<index>`. tests/parity_page_background.rs
// replays both.
import { writeFileSync } from 'node:fs';
import { join } from 'node:path';
import { createHash } from 'node:crypto';
import { CaptureWriter, Rng, importFrom } from '../../uo-testkit/js/testkit.mjs';
import { corpus, crate, readBytes, root, ts } from './capture-r2-common.mjs';

const { DocxPackage } = await ts('package.js');
const { parseXml } = await ts('xml.js');
const pb = await ts('page-background.js');
const wm = await ts('watermark.js');
const { patternRows } = await importFrom(root, 'shared/drawingml/dist/patterns.js');

const sha = (u8) => createHash('sha256').update(u8).digest('hex').slice(0, 32);
const shaS = (s) => sha(new TextEncoder().encode(s));
const esc = (x) => x.replace(/\\/g, '\\\\').replace(/\n/g, '\\n').replace(/\r/g, '\\r').replace(/\t/g, '\\t');
const rng = new Rng(0x5ec0de57);
const pick = (a) => a[rng.below(a.length)];
const chance = (n) => rng.below(n) === 0;

const inputs = ['# page-background inputs (js/capture-page-background.mjs): `<kind>\\t<field>...`, fields escaped'];
const out = new CaptureWriter(['W5: page-background.ts and watermark.ts write halves over corpus backgrounds and seeded inputs', 'facets: out (short sha256 of the text/bytes), len, added, state']);
const counts = {};
function add(kind, fields, compute) {
  const i = counts[kind] = (counts[kind] ?? -1) + 1;
  inputs.push([kind, ...fields.map(String).map(esc)].join('\t'));
  const key = `${kind}#${i}`;
  try {
    for (const [facet, v] of Object.entries(compute())) out.set(key, facet, v);
  } catch (e) {
    out.set(key, 'out', 'err');
  }
}
const optS = (s) => (s === undefined ? 'u' : `s${s}`);
const fromOpt = (f) => (f === 'u' ? undefined : f.slice(1));
const optN = (n) => (n === undefined ? 'u' : `n${n}`);
const fromOptN = (f) => (f === 'u' ? undefined : Number(f.slice(1)));

// ── bg: corpus backgrounds + seeded ──
const stubImage = (rid) => (rid.length % 2 === 0 ? { data: new TextEncoder().encode(`img:${rid}`), ext: 'png' } : undefined);
const bgOut = (xml) => {
  const el = parseXml(xml);
  const bg = pb.readPageBackground(el, stubImage);
  if (!bg) return { out: 'undefined' };
  const rid = bg.fill?.image ? 'rId9' : undefined;
  return { out: shaS(pb.pageBackgroundXml(bg, rid)) };
};
const bgs = new Set();
for (const { file } of corpus()) {
  let pkg;
  try { pkg = DocxPackage.load(readBytes(file)); } catch { continue; }
  const xml = pkg.getPartText('word/document.xml') ?? '';
  const m = /<w:background\b[^>]*?(?:\/>|>[\s\S]*?<\/w:background>)/.exec(xml);
  if (m) bgs.add(m[0]);
}
const NSDECL = ' xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main" xmlns:v="urn:schemas-microsoft-com:vml" xmlns:o="urn:schemas-microsoft-com:office:office" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships"';
const withNs = (s) => s.replace(/^<w:background/, `<w:background${NSDECL}`);
for (const x of bgs) add('bg', [withNs(x)], () => bgOut(withNs(x)));

const COLORS = ['red', 'blue', 'silver', '#abc', '#0080ff', '#00AA11', '#112233', 'fill', 'fill darken(128)', 'fill lighten(100)', 'Fill  DARKEN( 5 )', 'FILL lighten(300)', '#12', '', 'x y', '#', '#ABCD', 'rgb(1,2,3)', 'white', 'black', 'yellow', 'navy', '#ffffff', '#FFF'];
const TYPES = ['gradient', 'gradientRadial', 'pattern', 'frame', 'tile', 'gradient', 'pattern', 'solid', ''];
const ANGLES = ['0', '-90', '-135', '-45', '45.6', '180', '12abc', 'x', '', '-0.4', '1e2', '90', '270'];
const FOCI = ['100%', '-50%', '50%', '0%', '49152f', '.5', '1', '-1', 'x', '', '33.33%', '0.123456%', '65536f', '-32768f'];
const FPOS = ['.5,.5', '0.5,1', '1,0', ',', '1', ',1', '.3333,.6667', '0,0', ' .5 , .25 ', 'x,y', '', '19661f,0', '.1234567,.9'];
const STOPS = ['0 #000082;19661f #66008f;1 red', '0 red;1 blue', '0 #fff;.5 silver;1 #000', 'x;y z', '0 red', ';;', '0 #123456;3 #abc', '10000f #2f2f2f', '0 notacolor;1 blue'];
const METHODS = ['linear sigma', 'none', 'any', '', 'a"b<c&d'];
const TITLES = ['5%', ' 10%', 'Ziguezague', 'a&amp;b.png', 'q"uote', '<tag>', '', 'é😀', 'x>y'];
const THEME = ['accent1', 'text2', '', 'a"b'];
const att = (v) => String(v).replace(/&/g, '&amp;').replace(/"/g, '&quot;').replace(/</g, '&lt;');
const seeded = () => {
  const bgAttrs = [];
  if (chance(2)) bgAttrs.push(`w:color="${att(pick(['FFFF00', 'ff0000', 'abc', '', '00FF00', 'zz']))}"`);
  if (chance(3)) bgAttrs.push(`w:themeColor="${att(pick(THEME))}"`);
  if (chance(3)) bgAttrs.push(`w:themeTint="${att(pick(['BF', '80', '']))}"`);
  if (chance(3)) bgAttrs.push(`w:themeShade="${att(pick(['99', 'FF', '']))}"`);
  let inner = '';
  if (chance(4) !== 0) {
    const vattrs = ['id="_x0000_s1025"', 'o:bwmode="white"'];
    if (chance(2)) vattrs.push(`fillcolor="${att(pick(COLORS))}"`);
    if (chance(3)) vattrs.push('o:targetscreensize="1024,768"');
    const f = [];
    if (chance(5) !== 0) f.push(`type="${att(pick(TYPES))}"`);
    if (chance(2)) f.push(`color2="${att(pick(COLORS))}"`);
    if (chance(2)) f.push(`angle="${att(pick(ANGLES))}"`);
    if (chance(2)) f.push(`focus="${att(pick(FOCI))}"`);
    if (chance(3)) f.push(`focusposition="${att(pick(FPOS))}"`);
    if (chance(3)) f.push(`colors="${att(pick(STOPS))}"`);
    if (chance(3)) f.push(`method="${att(pick(METHODS))}"`);
    if (chance(2)) f.push(`o:title="${att(pick(TITLES))}"`);
    if (chance(3)) f.push(`r:id="${att(pick(['rId1', 'rId12', 'rId7', 'x']))}"`);
    const kid = chance(4) === 0 ? '<o:fill v:ext="view" type="gradientCenter"/>' : '';
    inner = `<v:background ${vattrs.join(' ')}><v:fill ${f.join(' ')}${kid ? `>${kid}</v:fill>` : '/>'}</v:background>`;
  }
  return `<w:background${NSDECL} ${bgAttrs.join(' ')}${inner ? `>${inner}</w:background>` : '/>'}`;
};
for (let i = 0; i < 4000; i++) { const x = seeded(); add('bg', [x], () => bgOut(x)); }

// ── vca / ocg / rc2 ──
const HEXES = ['000000', 'ffffff', 'FF0000', '008000', '0000FF', 'FFFF00', '808080', 'C0C0C0', '000080', '800000', '008080', '808000', '800080', 'FF00FF', '00FFFF', '00FF00', '#ff0000', '#FF8800', '112233', 'AABBCC', 'abcdef', '#ABCDEF', 'aabbcc', 'AA11BB', '', '#', 'a', 'ab', 'aa', 'aabb', 'aabbc', 'aabbccd', '##aabbcc', 'ÀÀÀÀÀÀ', 'ßßßßßß', '😀😀😀', 'ǅǅǅǅǅǅ', 'İİİİİİ', ' aabbcc', 'aabbcc ', 'ΣΣΣΣΣΣ', 'ABCDEF1', 'a😀'];
for (const h of HEXES) add('vca', [h], () => ({ out: pb.vmlColorAttr(h) }));
for (let i = 0; i < 300; i++) {
  const n = rng.below(9);
  let h = '';
  for (let k = 0; k < n; k++) h += pick(['0', '1', 'a', 'A', 'f', 'F', 'z', '#', 'é', 'ß']);
  add('vca', [h], () => ({ out: pb.vmlColorAttr(h) }));
}
const DEG = ['0', '1', '0.5', '0.25', '0.75', '0.4999995', '0.5000005', '0.500001', '0.499999', '-1', '2', 'NaN', 'Infinity', '-Infinity', '0.1', '0.9', '0.3333333', '0.123456789', '1e-7', '0.999999', '-0'];
for (let i = 0; i < 100; i++) DEG.push(String(rng.below(10001) / 10000));
for (const d of DEG) add('ocg', [d], () => ({ out: pb.oneColorGradientColor2(Number(d)) }));
const C2 = ['fill', 'fill darken(128)', 'fill lighten(128)', 'FILL', 'Fill Darken(0)', 'fill darken(255)', 'fill lighten(255)', 'fill darken(300)', 'fill lighten(99999999999999999999999)', ' fill ', ' fill darken(5) ', 'fill  lighten(1)', 'fill darken (5)', 'filldarken(5)', 'fill darken(5', 'fill darken()', 'fill darken(5)x', 'fill\ndarken(7)\n', 'fillx', 'fi ll', 'fı' + 'll', 'fill darken(٣)', 'fill lighten(0x5)', 'blue', 'red', '#abc', '#0080ff', 'silver', 'x', 'fill darken(5) ', 'fill lighten(9)', 'fill﻿darken(9)', 'fill dark(9)', '', 'fill lighten(-5)', 'fill lighten(5.5)'];
const C1 = ['FF8800', '000000', 'FFFFFF', '123456', 'abcdef', 'AB', '', 'XYZ', '1', 'ff00', 'GG0000', '0x1122', '-1-2-3', ' 1 2 3', '12\ud83d1'];
for (const c2 of [...C2, undefined]) for (const c1 of C1.slice(0, 6)) add('rc2', [optS(c2), c1], () => ({ out: pb.resolveColor2(c2, c1) }));
for (const c2 of ['fill darken(77)', 'fill lighten(77)']) for (const c1 of C1) add('rc2', [optS(c2), c1], () => ({ out: pb.resolveColor2(c2, c1) }));

// ── gif ──
const gifOut = (rows) => { const b = pb.patternGif(rows); return { out: sha(b), len: b.length }; };
const gifRows = [];
for (const p of pb.WORD_PATTERNS) gifRows.push(Array.from(patternRows(p.prst)));
gifRows.push([0, 0, 0, 0, 0, 0, 0, 0], [255, 255, 255, 255, 255, 255, 255, 255], [], [1], [255, 0, 255], [256, 257, -1, 1.5, 2 ** 32 + 5, 2 ** 31, NaN, -(2 ** 31)], [170, 85, 170, 85, 170, 85, 170, 85], [1, 2, 4, 8, 16, 32, 64, 128, 7]);
for (let i = 0; i < 400; i++) gifRows.push(Array.from({ length: 8 }, () => (chance(8) ? 0 : rng.below(256))));
for (const rows of gifRows) add('gif', [rows.join(',')], () => gifOut(rows));
add('pat', [], () => ({ out: shaS(pb.WORD_PATTERNS.map((p) => `${p.prst}|${p.title.pt}|${p.title.en}|${p.title.es}`).join('\n')) }));

// ── watermark ──
const WTEXT = ['RASCUNHO', 'CONFIDENCIAL', 'a & b', '<x>', 'q"uote', 'é😀', '', 'DO NOT COPY', ' a b '];
const WFONT = ['Calibri', 'Times New Roman', 'a"b', '&amp;', '', 'Segoe UI'];
const WCOL = ['C0C0C0', 'ff0000', 'ABCDEF', 'a', ''];
const NUMS = ['0', '1', '468', '117', '527.85', '131.95', '0.005', '100.125', '1e21', '-3.14159', '12345.678', 'NaN', '1e-7', '-0', '0.1'];
const num = () => pick(NUMS);
const vmlCustom = (h) => `vml:${h}`;
const wmText = (f) => {
  const [text, font, size, color, semi, layout, w, h, n, spid, anchor, custom] = f;
  const spec = { kind: 'text', text, font, ...(size === 'u' ? {} : { sizePt: Number(size) }), color, semitransparent: semi === '1', layout };
  return wm.watermarkRunXml(spec, { widthPt: Number(w), heightPt: Number(h) }, { n: Number(n), spid: Number(spid), anchorId: anchor }, undefined, custom === '1' ? vmlCustom : undefined);
};
for (let i = 0; i < 700; i++) {
  const f = [pick(WTEXT), pick(WFONT), chance(3) ? 'u' : num(), pick(WCOL), chance(2) ? '1' : '0', pick(['diagonal', 'horizontal', 'other']), num(), num(), pick(['1', '2', '17', '0', '3.5']), pick(['2049', '1026', '5']), pick(['5C1B2A3D', '', 'a"b']), chance(2) ? '1' : '0'];
  add('wm', ['text', ...f], () => ({ out: shaS(wmText(f)) }));
}
const wmPic = (f) => {
  const [title, washout, w, h, n, spid, anchor, rid] = f;
  const spec = { kind: 'picture', data: new Uint8Array([1, 2, 3]), ext: 'png', title, washout: washout === '1' };
  return wm.watermarkRunXml(spec, { widthPt: Number(w), heightPt: Number(h) }, { n: Number(n), spid: Number(spid), anchorId: anchor }, fromOpt(rid));
};
for (let i = 0; i < 300; i++) {
  const f = [pick(['logo.png', 'a&b.png', '<x>', 'q"t', '', 'é.png']), chance(2) ? '1' : '0', num(), num(), pick(['1', '2', '9']), pick(['2049', '7']), pick(['AABB', '']), optS(pick([undefined, 'rId5', '', 'a"b']))];
  add('wm', ['pic', ...f], () => ({ out: shaS(wmPic(f)) }));
}
const box = (b) => `${b.widthPt},${b.heightPt}`;
for (let i = 0; i < 300; i++) {
  const t = pick(WTEXT), lay = pick(['diagonal', 'horizontal']), cw = pick(['468', '451.3', '0', '600.5', '1', '-5', 'NaN']);
  add('box', ['auto', t, lay, cw], () => ({ out: box(wm.autoTextBox(t, lay, Number(cw))) }));
  const a = num(), s = num(), lf = pick(['1.2207', '1', '0', '1.5', 'NaN']);
  add('box', ['exp', a, s, lf], () => ({ out: box(wm.explicitTextBox(Number(a), Number(s), Number(lf))) }));
  const w = pick(['100', '0', '-1', '50.5', 'NaN', '640']), h = pick(['50', '0', '-1', '33.3', '480']), sc = optN(chance(3) ? undefined : pick([0, 50, 100, 33.333, NaN]));
  add('box', ['pic', w, h, sc, cw], () => ({ out: box(wm.pictureBox({ w: Number(w), h: Number(h) }, fromOptN(sc), Number(cw))) }));
}
add('gallery', [], () => ({ out: shaS(wm.WATERMARK_GALLERY.map((g) => [g.id, g.category, g.text.pt, g.text.en, g.text.es, g.layout, g.box.pt.join(','), g.box.en.join(','), g.box.es.join(',')].join('|')).join('\n')) }));
add('texts', [], () => ({ out: shaS(['en', 'pt', 'es'].map((l) => wm.WATERMARK_TEXTS[l].join('|')).join('\n')) }));

// ── writePageBackground over synthetic packages ──
const enc = new TextEncoder();
const W_NS = 'xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"';
const docXml = (open, body) => `<?xml version="1.0" encoding="UTF-8" standalone="yes"?>${open}${body}</w:document>`;
const OPENS = [`<w:document ${W_NS}>`, `<w:document ${W_NS} xmlns:v="urn:schemas-microsoft-com:vml" xmlns:o="urn:schemas-microsoft-com:office:office" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships">`, `<w:document ${W_NS} xmlns:o="urn:schemas-microsoft-com:office:office">`, `<w:document\n${W_NS}\n>`, '<w:documentX>', `<w:document ${W_NS}`, '<w:docum'];
const BODIES = ['<w:body/>', '<w:body><w:p/></w:body>', '<w:background w:color="FF0000"/><w:body/>', '<w:background w:color="FFFFFF"><v:background id="_x0000_s1025"><v:fill r:id="rId5" type="frame"/></v:background></w:background><w:body/>', '<w:background w:color="0000FF"><v:background><v:fill r:id="rId5x" type="tile"/></v:background></w:background><w:body/>', '<w:body/><w:background w:color="00FF00"/>', '<w:background w:color="A">x</w:background><w:body/>', '<w:backgroundX/><w:body/>', '<w:background w:color="B"><v:fill r:id="rId5"/></w:background><w:body><w:p r:id="rId5"/></w:body>', '<w:background w:color="B"><v:fill r:id=""/></w:background><w:body/>'];
const R_HEAD = '<?xml version="1.0" encoding="UTF-8" standalone="yes"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">';
const IMG_T = 'http://schemas.openxmlformats.org/officeDocument/2006/relationships/image';
const RELS = [undefined, `${R_HEAD}</Relationships>`, `${R_HEAD}<Relationship Id="rId5" Type="${IMG_T}" Target="media/image1.png"/></Relationships>`, `${R_HEAD}<Relationship Id="rId1" Type="${IMG_T}" Target="media/a.png"/><Relationship Id="rId2" Type="${IMG_T}" Target="media/b.png"/><Relationship Id="rId5" Type="${IMG_T}" Target="media/image1.png"/></Relationships>`, `${R_HEAD}<Relationship Id="rId5" Type="${IMG_T}" Target="media/image1.png"></Relationship></Relationships>`, `${R_HEAD}<Relationship Id="rId5x" Type="${IMG_T}" Target="x.png"/></Relationships>`, '', `${R_HEAD}<Relationship Target="media/image1.png" Id="rId5" Type="${IMG_T}"/><Relationship Id="rId6" Type="${IMG_T}" Target="y.png"/></Relationships>`, `${R_HEAD}<Relationship Id="rId1" Type="x"/>`];
const CT_HEAD = '<?xml version="1.0" encoding="UTF-8" standalone="yes"?><Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types">';
const CTS = [undefined, `${CT_HEAD}<Default Extension="xml" ContentType="application/xml"/></Types>`, `${CT_HEAD}<Default Extension="png" ContentType="image/png"/><Default Extension="xml" ContentType="application/xml"/></Types>`, `${CT_HEAD}<Default Extension="PNG" ContentType="image/png"/></Types>`, `${CT_HEAD}<Default Extension="jpeg" ContentType="image/jpeg"/></Types>`, '', '<Types', `<Types>\n<Default Extension="png"/></Types>`, `<TypesX><Types a="1">`];
const EXTS = ['png', 'jpg', 'JPG', 'jpeg', 'gif', 'bmp', 'tif', 'TIFF', 'emf', 'wmf', 'svg', 'x$&y', 'a$1b', "a$'b", 'a$`b', '$$', 'a$10', 'webp', 'PnG'];
const IMGBG = (type) => `<w:background w:color="FF0000" ${NSDECL.trim()}><v:background fillcolor="red"><v:fill type="${type}" r:id="rId2" o:title="img.png"/></v:background></w:background>`;
const NOIMG = [`<w:background ${NSDECL.trim()} w:color="00FF00"/>`, `<w:background ${NSDECL.trim()} w:color="0000FF"><v:background fillcolor="blue"><v:fill type="gradient" color2="red" angle="-90"/></v:background></w:background>`];
const nul = null;
const RELOC = [null, null, null, null, null, { target: 'doc2.xml', path: 'word/doc2.xml' }, { target: '/custom/x.xml', path: 'custom/x.xml' }, { target: './doc3.xml', path: 'word/doc3.xml' }, { target: 'missing.xml', path: null }];
const scenario = () => {
  const parts = [];
  const open = pick(OPENS), body = pick(BODIES);
  const reloc = pick(RELOC);
  let rels = pick(RELS);
  if (reloc) {
    const rel = `<Relationship Id="rId9" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="${reloc.target}"/>`;
    rels = rels && rels.includes('</Relationships>') ? rels.replace('</Relationships>', `${rel}</Relationships>`) : `${R_HEAD}${rel}</Relationships>`;
  }
  parts.push([reloc?.path ?? 'word/document.xml', docXml(open, body)]);
  if (reloc && reloc.path) parts.push([`${reloc.path.replace(/[^/]+$/, '')}_rels/${reloc.path.replace(/^.*\//, '')}.rels`, pick(RELS) ?? '']);
  if (rels !== undefined) parts.push(['word/_rels/document.xml.rels', rels]);
  const ct = pick(CTS);
  if (ct !== undefined) parts.push(['[Content_Types].xml', ct]);
  if (chance(3)) parts.push(['word/media/image1.png', 'PNGDATA']);
  if (chance(4)) parts.push(['word/media/image2.jpeg', 'JPEG']);
  const ops = [];
  for (let k = 1 + rng.below(4); k > 0; k--) {
    const kind = rng.below(5);
    const bg = kind === 0 ? nul : kind === 1 ? pick(NOIMG) : IMGBG(pick(['frame', 'tile']));
    ops.push({ bg, ext: pick(EXTS), prev: chance(2) });
  }
  return { parts, ops };
};
const runScenario = (scn) => {
  const pkg = DocxPackage.empty();
  for (const [p, d] of scn.parts) pkg.setPartText(p, d);
  const res = {};
  let prev;
  scn.ops.forEach((op, j) => {
    const media = new Map();
    let bg;
    if (op.bg !== null) {
      bg = pb.readPageBackground(parseXml(op.bg), () => ({ data: enc.encode(`DATA-${j}`), ext: op.ext }));
    }
    let added;
    try {
      added = pb.writePageBackground(pkg, bg ?? null, op.prev ? prev : undefined);
    } catch (e) {
      res[`added${j}`] = 'err';
      res[`state${j}`] = 'err';
      return;
    }
    prev = added;
    res[`added${j}`] = added ? `${added.rid}|${added.media}` : '-';
    const state = pkg.partNames().map((n) => `${n}\u0000${sha(pkg.getPart(n))}`).join('\n');
    res[`state${j}`] = shaS(state);
  });
  return res;
};
for (let i = 0; i < 1500; i++) {
  const scn = scenario();
  add('wr', [JSON.stringify(scn)], () => runScenario(scn));
}

writeFileSync(join(crate, 'tests', 'parity', 'page-background.inputs'), inputs.join('\n') + '\n');
out.write(join(crate, 'tests', 'parity', 'page-background.capture'));
console.log('page-background:', JSON.stringify(counts));
