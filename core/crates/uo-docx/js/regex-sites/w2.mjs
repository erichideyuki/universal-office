// The regex sites of the D1b frontier W2 (src/write/emit/object.rs: serialize.ts `patchDrawingGeometry`,
// `docPrName`, `freshDrawingIds`, `editedGroupXml`, `shapeOut`, `pictureXml`; shape-xml.ts `txbxContentOf`)
// against V8, checked by tests/regex_w2.rs. The rows run the TS code copied as written (the functions are
// private in serialize.ts); rows return strings or booleans, never positions (UTF-16 indices would differ
// from Rust's byte offsets).
import { Rng } from '../../../uo-testkit/js/testkit.mjs';

const EMU_PER_TWIP = 635;
const escAttr = (s) => s.replace(/&/g, '&amp;').replace(/</g, '&lt;').replace(/>/g, '&gt;').replace(/"/g, '&quot;');

// serialize.ts:127–147 (verbatim)
function patchDrawingGeometry(raw, obj) {
  const cx = Math.round(obj.widthTwips * EMU_PER_TWIP);
  const cy = Math.round(obj.heightTwips * EMU_PER_TWIP);
  let out = raw
    .replace(/<wp:extent\b[^>]*\/>/, (m) => m.replace(/cx="\d+"/, `cx="${cx}"`).replace(/cy="\d+"/, `cy="${cy}"`))
    .replace(/<a:ext\b[^>]*\/>/g, (m) => m.replace(/cx="\d+"/, `cx="${cx}"`).replace(/cy="\d+"/, `cy="${cy}"`));
  if (obj.offsetXTwips !== undefined) {
    const ex = Math.round(obj.offsetXTwips * EMU_PER_TWIP);
    out = out.replace(/<wp:positionH\b[^>]*>[\s\S]*?<\/wp:positionH>/, (b) =>
      b.replace(/(<wp:posOffset>)-?\d+(<\/wp:posOffset>)/, `$1${ex}$2`));
  }
  if (obj.offsetYTwips !== undefined) {
    const ey = Math.round(obj.offsetYTwips * EMU_PER_TWIP);
    out = out.replace(/<wp:positionV\b[^>]*>[\s\S]*?<\/wp:positionV>/, (b) =>
      b.replace(/(<wp:posOffset>)-?\d+(<\/wp:posOffset>)/, `$1${ey}$2`));
  }
  return out;
}

// serialize.ts:254
const docPrName = (raw) => (raw ? /<wp:docPr\b[^>]*\bname="([^"]*)"/.exec(raw)?.[1] : undefined);

// serialize.ts:432–439 (verbatim; the counter is local to a row)
function freshDrawingIds(xml) {
  let freshId = 0;
  const out = xml
    .replace(/(<wp:docPr\b[^>]*?\bid=")\d+"/g, (_m, a) => `${a}${900000 + ++freshId}"`)
    .replace(/\s(?:wp14:anchorId|wp14:editId|o:spid)="[^"]*"/g, '');
  return `${out}|${freshId}`;
}

// serialize.ts:334–361 (verbatim)
function editedGroupXml(srcObj, raw) {
  let xml = srcObj.replace(/<w:drawing\b[\s\S]*?<\/w:drawing>/, () => raw);
  if (!/<mc:AlternateContent\b/.test(xml)) return raw;
  const name = docPrName(raw);
  const hidden = /<wp:docPr\b[^>]*\bhidden="1"/.test(raw);
  const rh = /\brelativeHeight="(\d+)"/.exec(raw)?.[1];
  const behind = /\bbehindDoc="1"/.test(raw);
  xml = xml.replace(/<v:group\b[^>]*>/, (tag) => {
    let t = tag;
    if (name) t = /\sid="[^"]*"/.test(t) ? t.replace(/\sid="[^"]*"/, () => ` id="${escAttr(name)}"`) : t.replace(/^<v:group/, () => `<v:group id="${escAttr(name)}"`);
    t = t.replace(/style="([^"]*)"/, (_m, st) => {
      let s = st.replace(/;?visibility:[^;]*/g, '');
      s += `;visibility:${hidden ? 'hidden' : 'visible'}`;
      if (rh) s = s.replace(/z-index:-?\d+/, `z-index:${behind ? -(Number(rh) - 2048) : rh}`);
      return `style="${s.replace(/^;/, '')}"`;
    });
    return t;
  });
  return xml;
}

// serialize.ts:249–252 (the strip of `shapeOut`)
const stripFallback = (v) => v.replace(/^<mc:Fallback>/, '').replace(/<\/mc:Fallback>$/, '');

// shape-xml.ts:603–606 (verbatim)
function txbxContentOf(drawingXml) {
  const m = /<w:txbxContent\b[^>]*>([\s\S]*?)<\/w:txbxContent>/.exec(drawingXml);
  return m ? m[1] : undefined;
}

const RAW = '<w:drawing><wp:anchor behindDoc="1" relativeHeight="251659264"><wp:docPr id="2" name="Grupo &amp; 1" hidden="1"/></wp:anchor></w:drawing>';
const RAW2 = '<w:drawing><wp:anchor behindDoc="0" relativeHeight="0007"><wp:docPr id="2" hidden="0"/></wp:anchor></w:drawing>';
const SRC = '<mc:AlternateContent><mc:Choice Requires="wpg"><w:drawing><wp:anchor/></w:drawing></mc:Choice><mc:Fallback><w:pict><v:group id="old" style="position:absolute;visibility:visible;z-index:5"></v:group></w:pict></mc:Fallback></mc:AlternateContent>';
const composite = (f) => [{ [Symbol.replace]: (s) => f(s) }, ''];
const s = (x) => String(x);

export const SITES = [
  ['geo.1', composite((x) => patchDrawingGeometry(x, { widthTwips: 1234, heightTwips: 567, offsetXTwips: 10, offsetYTwips: -5 })), 'replace'],
  ['geo.2', composite((x) => patchDrawingGeometry(x, { widthTwips: 100.5, heightTwips: 0 })), 'replace'],
  ['geo.3', composite((x) => patchDrawingGeometry(x, { widthTwips: 1e15, heightTwips: -3, offsetXTwips: 12345678901, offsetYTwips: 0 })), 'replace'],
  ['docpr.name', composite((x) => s(docPrName(x))), 'replace'],
  ['docpr.hidden', /<wp:docPr\b[^>]*\bhidden="1"/, 'test'],
  ['rh', /\brelativeHeight="(\d+)"/, 'group'],
  ['behind', /\bbehindDoc="1"/, 'test'],
  ['fresh', composite(freshDrawingIds), 'replace'],
  ['group.src', composite((x) => editedGroupXml(x, RAW)), 'replace'],
  ['group.src2', composite((x) => editedGroupXml(x, RAW2)), 'replace'],
  ['group.raw', composite((x) => editedGroupXml(SRC, x)), 'replace'],
  ['ac', /<mc:AlternateContent\b/, 'test'],
  ['strip', composite(stripFallback), 'replace'],
  ['txbx.of', composite((x) => s(txbxContentOf(x))), 'replace'],
  ['drawing.first', [/<w:drawing\b[\s\S]*?<\/w:drawing>/, () => 'RAW'], 'replace'],
];

// ── inputs: hand-written edge cases and seeded tag soup over the patterns' vocabulary ──
const HAND = [
  '', 'x', '>', '/>', '<',
  '<wp:extent cx="1" cy="2"/>', '<wp:extent cx="1" cy="2">', '<wp:extent/>', '<wp:extent  cx="9"/><wp:extent cx="1" cy="2"/>', '<wp:extentX cx="1"/>',
  '<wp:extent cx="" cy="a" cx="5" cy="6"/>', '<wp:extent xcx="1" cy="2"/>', '<wp:extent cx="1"cy="2"/>', '<wp:extent cx="12',
  '<wp:extent cx="1" cy="2" ></wp:extent>', '<wp:extent cx="1" cy="2"/ >',
  '<a:ext cx="5" cy="6"/><a:ext uri="u"><x/></a:ext><a:extLst/><a:ext/>', '<a:ext cx="5" cy="6"></a:ext>', '<a:ext  cx="5"  cy="6"  />', '<a:ext cx="1" cy="2"/><a:ext cx="3" cy="4"/>',
  '<a:ext uri="{x}"><a:ext cx="7" cy="8"/></a:ext>', '<a:ext cx="1" cy="2"', '<a:ext cx="1" cy="2"> /> <a:ext cx="3" cy="4"/>',
  '<wp:positionH relativeFrom="column"><wp:posOffset>100</wp:posOffset></wp:positionH>',
  '<wp:positionH relativeFrom="column"><wp:posOffset>-100</wp:posOffset><wp:posOffset>5</wp:posOffset></wp:positionH>',
  '<wp:positionH relativeFrom="page"><wp:align>left</wp:align></wp:positionH><wp:positionV><wp:posOffset>7</wp:posOffset></wp:positionV>',
  '<wp:positionH/><wp:posOffset>1</wp:posOffset></wp:positionH>', '<wp:positionH', '<wp:positionHX><wp:posOffset>1</wp:posOffset></wp:positionH>',
  '<wp:positionH><wp:posOffset>x</wp:posOffset></wp:positionH>', '<wp:positionH><wp:posOffset>1</wp:posOffset>',
  '<wp:positionV relativeFrom="paragraph"><wp:posOffset>+4</wp:posOffset></wp:positionV>', '<wp:positionV><wp:posOffset>-0</wp:posOffset></wp:positionV>',
  '<wp:docPr id="1" name="a"/>', '<wp:docPr name="a" id="3" name="b"/>', '<wp:docPr id="1" xname="a"/>', '<wp:docPr id="1" name="a>b"/>', '<wp:docPr id="1" name="a',
  '<wp:docPr id="1" hidden="1" name="a"/><wp:docPr id="2"/>', '<wp:docPrX id="1" name="a"/><wp:docPr id="2" name="b"/>', '<wp:docPr id="12" id="13"/>',
  '<wp:docPr id="1"/><wp:docPr id="22" wp14:id="5" o:spid="x" xid="9"/>', '<wp:docPr id="x" id="7"/>', '<wp:docPr id="7x" id=\'8\'/>', '<wp:docPr/>', '<wp:docPr',
  '<wp:docPr a="<wp:docPr id=" id="4"/>', '<wp:docPr name=""/>', '<wp:docPr name="é😀" id="1"/>', '<wp:docPr hidden="1"/>', '<wp:docPr hidden="10"/>', '<wp:docPr x:hidden="1"/>',
  '<wp:anchor relativeHeight="0" behindDoc="1">', '<wp:anchor xrelativeHeight="1" relativeHeight="251659264">', '<wp:anchor relativeHeight="">', '<wp:anchor relativeHeight="1a" relativeHeight="22">',
  '<wp:anchor behindDoc="10">', '<wp:anchor xbehindDoc="1">', ' behindDoc="1"', 'behindDoc="1"',
  ' wp14:anchorId="1A" wp14:editId="2B" o:spid="_x0000_s1026" a="b"', '\twp14:anchorId="x"', 'a wp14:anchorId="x', ' wp14:anchorId="1"wp14:editId="2"', ' o:spid="1" o:spid="2"',
  '<wp:docPr id="5" wp14:anchorId="1" o:spid="s"/> o:spid="q"',
  '<mc:AlternateContent>', '<mc:AlternateContentX>', '<mc:AlternateContent', 'x<mc:AlternateContent/>',
  '<w:drawing>a</w:drawing>', '<w:drawing a="1">a</w:drawing><w:drawing>b</w:drawing>', '<w:drawingX>a</w:drawing>', '<w:drawing>open', '<w:drawing></w:drawing></w:drawing>',
  '<mc:AlternateContent><mc:Choice><w:drawing><wp:anchor/></w:drawing></mc:Choice><mc:Fallback><v:group id="old" style="a;visibility:hidden;b;z-index:-7"/></mc:Fallback></mc:AlternateContent>',
  '<mc:AlternateContent><w:drawing/><v:group style="z-index:0">', '<mc:AlternateContent><v:group>', '<mc:AlternateContent><v:group id="a" id="b" style="">',
  '<mc:AlternateContent><v:group  id=\'q\' style="visibility:hidden">', '<mc:AlternateContent><v:group style="visibility:x;;visibility:y">', '<mc:AlternateContent><v:group style="z-index:5;z-index:6">',
  '<mc:AlternateContent><v:group style="a:1" o:style="b">', '<mc:AlternateContent><v:group mso-style="z" style="visibility:hidden"', '<mc:AlternateContent><v:groupX id="1">',
  '<mc:AlternateContent><v:group\tid="old"\nstyle="x">', '<mc:AlternateContent><v:group style="x;;visibility:hidden;z-index:-1y">', '<mc:AlternateContent><v:group>text</v:group><v:group id="z">',
  '<mc:AlternateContent>$1<v:group style="z-index:9">$&', '<mc:Fallback>x</mc:Fallback>', '<mc:Fallback></mc:Fallback>', '<mc:Fallback>', '</mc:Fallback>', '<mc:Fallback>a</mc:Fallback>b</mc:Fallback>',
  '<mc:Fallback><mc:Fallback>x</mc:Fallback>', 'x<mc:Fallback>y</mc:Fallback>', '<mc:Fallback>y</mc:Fallback>\n',
  '<w:txbxContent><w:p/></w:txbxContent>', '<w:txbxContent a="1"/><w:p/></w:txbxContent>', '<w:txbxContentX><w:txbxContent>b</w:txbxContent>', '<w:txbxContent>open',
  '<w:txbxContent></w:txbxContent>', '<w:txbxContent a=">"></w:txbxContent>', 'a<w:txbxContent>1</w:txbxContent><w:txbxContent>2</w:txbxContent>',
  'r:embed="uw:media"', 'r:embed="uw:media" r:embed="uw:media"', 'r:embed="uw:medi"', 'xr:embed="uw:media"', 'r:embed="uw:layer"',
];
const VOCAB = ['<wp:extent ', '<a:ext ', 'cx="1" ', 'cy="22" ', '/>', '>', '"', '<wp:positionH>', '</wp:positionH>', '<wp:positionV ', '</wp:positionV>', '<wp:posOffset>', '</wp:posOffset>',
  '5', '-7', '<wp:docPr ', 'id="3" ', 'name="n" ', 'hidden="1" ', 'x:id="4" ', 'wp14:anchorId="1" ', 'o:spid="s" ', ' ', '\n', '<mc:AlternateContent>', '<mc:Fallback>', '</mc:Fallback>',
  '<v:group ', 'style="visibility:hidden;z-index:9" ', 'relativeHeight="12" ', 'behindDoc="1" ', '<w:drawing>', '</w:drawing>', '<w:txbxContent>', '</w:txbxContent>', 'r:embed="uw:media"', 'é', '😀'];

function seeded(seed, count) {
  const rng = new Rng(seed);
  const out = [];
  for (let i = 0; i < count; i++) {
    let t = '';
    for (let k = 1 + rng.below(16); k > 0; k--) t += VOCAB[rng.below(VOCAB.length)];
    out.push(t);
  }
  return out;
}

export const INPUTS = [...HAND, ...seeded(0xd1b2, 600)];
