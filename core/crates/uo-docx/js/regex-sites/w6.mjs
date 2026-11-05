// The regex sites of frontier W6 (picture-xml.ts, text-effects.ts) against V8, checked by
// tests/regex_w6.rs. A row is `[id, regex | function, op]` (see js/regex-diff.mjs); the sites that
// are an expression around a regex (`XmlPatch.setAttrs` with its dynamic per-attribute pattern, the
// prefix replacement with its template) are functions: V8 evaluates the TS expression whole. The
// dynamic pattern `new RegExp('\\s' + k.replace(/[:.]/g, '\\$&') + '="[^"]*"')` is run with the keys the
// callers pass (constants: `name`, `descr`, `r:embed`, `distT`, `noChangeAspect`, `wrapText`, …), so no
// metacharacter other than the escaped `:` and `.` reaches it.
import { INPUTS as BASE } from './templates.mjs';

const escAttr = (s) => s.replace(/&/g, '&amp;').replace(/</g, '&lt;').replace(/>/g, '&gt;').replace(/"/g, '&quot;');
/** picture-xml.ts:42–47, one entry of `setAttrs`' `set`. */
const setAttr = (tag, k, v, selfClose) => {
  const re = new RegExp(`\\s${k.replace(/[:.]/g, '\\$&')}="[^"]*"`);
  if (v === undefined) return tag.replace(re, '');
  const a = ` ${k}="${escAttr(String(v))}"`;
  if (re.test(tag)) return tag.replace(re, () => a);
  return tag.replace(selfClose ? /\s*\/>$/ : />$/, (m) => `${a}${m.trim() === '/>' ? '/>' : '>'}`);
};
const wpPrefix = (pfx) => (s) => s.replace(/<(\/?)wp:/g, `<$1${pfx}:`);

/** [site id, the TS regex (or function), operation]. */
export const SITES = [
  ['w6.set_name', (s) => setAttr(s, 'name', 'a"b<c>&', s.endsWith('/>')), 'fn'],
  ['w6.set_name_inv', (s) => setAttr(s, 'name', 'x', !s.endsWith('/>')), 'fn'],
  ['w6.set_embed', (s) => setAttr(s, 'r:embed', 'rId9', s.endsWith('/>')), 'fn'],
  ['w6.set_dist', (s) => setAttr(s, 'distT', 12, s.endsWith('/>')), 'fn'],
  ['w6.set_dot', (s) => setAttr(s, 'a.b', 'x', s.endsWith('/>')), 'fn'],
  ['w6.remove_name', (s) => setAttr(s, 'name', undefined, s.endsWith('/>')), 'fn'],
  ['w6.remove_embed', (s) => setAttr(s, 'r:embed', undefined, s.endsWith('/>')), 'fn'],
  ['w6.attr_span', /\sr:embed="[^"]*"/, 'match'],
  ['w6.open_name', /^<([^\s/>]+)/, 'group'],
  ['w6.tag_prefix', /^<([^:\s>]+):/, 'group'],
  ['w6.self_close_open', (s) => s.replace(/\s*\/>$/, '>'), 'fn'],
  ['w6.wp_xx', wpPrefix('xx'), 'fn'],
  ['w6.wp_dollar', wpPrefix('a$b'), 'fn'],
  ['w6.wp_amp', wpPrefix('p$&q'), 'fn'],
  ['w6.wp_back', wpPrefix('p$`q'), 'fn'],
  ['w6.wp_quote', wpPrefix("p$'q"), 'fn'],
  ['w6.wp_dd', wpPrefix('$$'), 'fn'],
  ['w6.wp_group', wpPrefix('p$1q$2'), 'fn'],
  ['w6.esc_attr', escAttr, 'fn'],
];

const tags = [
  '<wp:docPr id="1" name="Picture 1"/>', '<wp:docPr id="1" name="Picture 1">', '<wp:docPr name="a" name="b">', '<a:blip r:embed="rId4"/>', '<a:blip r:embed="rId4" r:embed="rId5">', '<a:blip/>', '<a:blip >', '<a:blip  />',
  '<a:blip\n/>', '<a:blip\tr:embed="x"\n/>', '<a:blip r:embed="">', '<a:blip r:embed=">">', '<a:blip rr:embed="x"/>', '<a:blip xr:embed="x"/>', '<a:blip r_embed="x"/>', '<a:blip r:embed=\'x\'/>', '<a:blip r:embed=x/>',
  '<wp:anchor distT="0" distB="0">', '<wp:anchor distT="0" distT="5"/>', '<wp:anchor xdistT="0"/>', '<wp:anchor  distT = "0">', '<wp:anchor distT="é😀">', '<wp:anchor a.b="1"/>', '<wp:anchor aXb="1"/>', '<wp:anchor a.b="1" aXb="2">',
  '<a:ext cx="1" cy="2"/>', '<a:xfrm rot="5" flipH="1">', '<x name="1" name="2"/>', '<x name="1"/>', '<x name="1">', '<x\tname="1">', '<x\nname="a"\n>', '<x name="a" />', '<x name="a"  />', '<x a="1"　/>',
  '<', '<x', '<x ', '<x>', '<x/>', '<x />', '', ' ', '/>', '>', ' />', '/ >', '<x/ >', '<x a="/>', '<x a="/">', '<x a="/" />', '<x a=">"/>', '<é name="1"/>', '<x name="é"/>', '<wp:inline distT="0" distB="0" distL="0" distR="0">',
  '<wp:docPr id="1" name="x" descr="y" title="z" hidden="1">', '<wp:docPr hidden="1"/>', '<wp:docPr id="1"\n  name="x"\n  />',
  '<w:drawing', '<wp:inline', '<wp:anchor', '<xx:inline', '<a-b:c', '<a_b:c', '<a.b:c', '<:a', '<a::b', '<a b:c', '<a>b:c', '<a:', '<1:b', '<é:b', '< a:b', '<a :b', '<a/:b',
  '<wp:inline><wp:extent/></wp:inline>', '<wp:a/><wp:b></wp:b><wpx:c/><wp/><wp:/><w p:d/><wp: e/>', '<<wp:a>', '</wp:a></ wp:a></wp:', '<wp:<wp:', '<//wp:a>', '</wp:wp:wp:>', 'text <wp:a> and </wp:a> end', '<WP:a>', '<wp :a>',
  '<a14:artisticBlur radius="10"/>', '<a:blur rad="9"/>', '<a:fillOverlay blend="over"/>', '<x', '</x>', '<!--x-->', '<?x?>', '<![CDATA[x]]>', '<a:glow\trad="1"/>',
];
export const INPUTS = [...new Set([...tags, ...BASE.slice(0, 40)])];
