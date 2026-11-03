// The regex sites of frontier R3 (drawing.ts, vml-shape.ts, page-background.ts, watermark.ts)
// against V8, checked by tests/regex_r3.rs. A row is `[id, regex, op]`; where the site is an
// expression around a regex (a split, a dynamic pattern with its entity decoding), the "regex" is
// an object with the site's own `exec` (op `match` and `group` read `x[0]` / `x[1]`), so the whole
// expression of the TS is what V8 evaluates.

const attrOf = (name) => ({
  // watermark.ts `attr(xml, name)`
  exec: (xml) => {
    const m = new RegExp(`\\s${name}="([^"]*)"`).exec(xml);
    return m ? [null, m[1].replace(/&quot;/g, '"').replace(/&lt;/g, '<').replace(/&gt;/g, '>').replace(/&amp;/g, '&')] : null;
  },
});
const styleProp = (k) => ({
  // watermark.ts `styleProp(style, k)`
  exec: (style) => {
    const v = new RegExp(`(?:^|;)\\s*${k}\\s*:\\s*([^;]+)`).exec(style)?.[1]?.trim();
    return v === undefined ? null : [null, v];
  },
});
/** [site id, the TS regex (or an `exec` object, or [regex, replacement] for `replace`), operation]. */
export const SITES = [
  ['d1.val_int', /^val\s+(-?\d+)/, 'group'],
  ['d2.len_test', /^(-?[0-9.]+)\s*(pt|in|px|cm|mm|pc|em)?$/, 'test'],
  ['d2.len_num', /^(-?[0-9.]+)\s*(pt|in|px|cm|mm|pc|em)?$/, 'group'],
  ['d2.len_unit', { exec: (s) => { const m = /^(-?[0-9.]+)\s*(pt|in|px|cm|mm|pc|em)?$/.exec(s); return m ? [null, m[2] ?? ''] : null; } }, 'group'],
  ['d3.color_token', { exec: (s) => [s.trim().split(/\s+/)[0]] }, 'match'],
  ['d4.ends_lower', /[a-z]$/, 'test'],
  ['d5.gd_op', { exec: (s) => [s.trim().split(/\s+/)[0]] }, 'match'],
  ['d5.gd_arg1', { exec: (s) => { const p = s.trim().split(/\s+/); return p.length > 1 ? [p[0], p[1]] : null; } }, 'group'],
  ['v1.x0000', /^_x0000_/, 'test'],
  ['p1.colors_first', { exec: (s) => { const p = s.trim().split(/\s+/); return p.length >= 2 ? [p[0], p[1]] : null; } }, 'match'],
  ['p1.colors_second', { exec: (s) => { const p = s.trim().split(/\s+/); return p.length >= 2 ? [p[0], p[1]] : null; } }, 'group'],
  ['w1.is_wm', /\bid="(?:PowerPlusWaterMarkObject|WordPictureWatermark)\d*"/, 'test'],
  ['w2.attr_style', attrOf('style'), 'group'],
  ['w2.attr_rid', attrOf('r:id'), 'group'],
  ['w2.attr_title', attrOf('o:title'), 'group'],
  ['w2.attr_string', attrOf('string'), 'group'],
  ['w3.prop_width', styleProp('width'), 'group'],
  ['w3.prop_font_family', styleProp('font-family'), 'group'],
  ['w3.prop_font_size', styleProp('font-size'), 'group'],
  ['w3.prop_rotation', styleProp('rotation'), 'group'],
  ['w4.shape_match', /<v:shape\b[^>]*\bid="(PowerPlusWaterMarkObject|WordPictureWatermark)\d*"[^>]*>[\s\S]*?<\/v:shape>/, 'match'],
  ['w4.shape_kind', /<v:shape\b[^>]*\bid="(PowerPlusWaterMarkObject|WordPictureWatermark)\d*"[^>]*>[\s\S]*?<\/v:shape>/, 'group'],
  ['w5.tag_shape', /<v:shape\b[^>]*>/, 'match'],
  ['w5.tag_imagedata', /<v:imagedata\b[^>]*>/, 'match'],
  ['w5.tag_textpath', /<v:textpath\b[^>]*>/, 'match'],
  ['w5.tag_fill', /<v:fill\b[^>]*>/, 'match'],
  ['w6.gain', /\bgain=/, 'test'],
  ['w6.blacklevel', /\bblacklevel=/, 'test'],
  ['w7.quotes', [/^["']|["']$/g, ''], 'replace'],
];

const lens = ['12pt', '1in', '5cm', '10mm', '3pc', '2em', '16px', '16', '-.5in', '1.2.3pt', '.', '-', '', ' ', '5 pt', ' 7in ', '1e3pt', 'abc', '100%', '0', '-0pt', '12PT', '5pt ', '\u00a05pt', '5\u00a0pt', '5\u2028pt', '\ufeff5in', '5\tcm', '5ptx', '5 p t', '--5pt', '-', '+5pt', '5.pt', '..', '1-2', '12\u0085pt'];
const fmlas = ['val 5000', 'val -12', 'val  7', 'val\t3', 'VAL 1', 'val 1x', 'val-3', 'val', 'val ', 'val  -', 'val 12345678901234567890', '*/ w h 2', '+- 1 2 3', '  abs  -5 ', '\u00a0val 1', 'val\u00a05', 'val\u30005', 'val --5', 'val 5.5'];
const styles = [
  'position:absolute;margin-left:0;margin-top:0;width:468pt;height:117pt;rotation:315;z-index:-251657216',
  'width:468pt;height:117pt', ' width : 10in ; height:2.5cm', ';width:', 'width: ;', 'width:\t;', 'width:;', 'x-width:1', 'width:1;width:2', ';;font-size: 12pt;',
  'font-family:&quot;Calibri&quot;;font-size:1pt', 'font-family:"Arial";font-size:40pt', "font-family:'Times New Roman';font-size:12.5pt", 'font-size:2pt', 'font-family:;font-size: ',
  'rotation:315', 'rotation: -45', 'rotation:', 'rotation:  ;', ';rotation:1e3;', 'ROTATION:5', 'width\n:\n5pt', 'width:5pt\n;height:1', ';\u00a0width:5', 'width:5pt;', 'font-family:"', 'font-family:\'', 'font-size:\u00a010pt',
  'width:1;;width:2', '\u2028width:3', ';font-size:\u2003', ';font-size:\t\t;',
];
const xmls = [
  '<w:pict><v:shape id="PowerPlusWaterMarkObject1" style="width:468pt;height:117pt;rotation:315" fillcolor="silver"><v:fill opacity=".5"/><v:textpath style="font-family:&quot;Calibri&quot;;font-size:1pt" string="RASCUNHO"/></v:shape></w:pict>',
  '<w:pict><v:shapetype id="_x0000_t75"/><v:shape id="WordPictureWatermark3" style="width:10in;height:5in"><v:imagedata r:id="rId7" o:title="a&amp;b.png" gain="19661f"/></v:shape></w:pict>',
  '<v:shape id="PowerPlusWaterMarkObject" style="x"></v:shape>', '<v:shape id="PowerPlusWaterMarkObject12"', '<v:shape id="PowerPlusWaterMarkObject12">', '<v:shape id="PowerPlusWaterMarkObject12"></v:shape',
  '<v:shapetype id="PowerPlusWaterMarkObject1"></v:shapetype>', '<v:shapex id="WordPictureWatermark1"></v:shapex>', '<v:shape o:spid="x" id="WordPictureWatermark1" xid="WordPictureWatermark2"/></v:shape>',
  '<v:shape id="WordPictureWatermark1" id="PowerPlusWaterMarkObject2" style="a"></v:shape>', '<v:shape id="PowerPlusWaterMarkObject1" id="WordPictureWatermark2"></v:shape>',
  '<v:shape xml:id="PowerPlusWaterMarkObject1"></v:shape>', '<v:shape\tid="PowerPlusWaterMarkObject1"\n></v:shape>', '<v:shape>id="PowerPlusWaterMarkObject1"</v:shape>',
  '<v:shape style="id=&quot;x&quot;" id=\'WordPictureWatermark1\'></v:shape>', '<v:shape a="id=" id="WordPictureWatermark1"></v:shape></v:shape>', '<v:shape id="WordPictureWatermark1"></v:shape><v:shape id="PowerPlusWaterMarkObject2"></v:shape>',
  '<v:shape a="1"><v:shape id="PowerPlusWaterMarkObject1"></v:shape>', '<v:shape id="PowerPlusWaterMarkObject1" a=">" b="id=&quot;WordPictureWatermark2&quot;"></v:shape>',
  ' style="a;b" style="c"', ' r:id="rId1"', '\tr:id="x&quot;y&lt;z&gt;&amp;quot;"', 'r:id="rId1"', ' string=""', ' string="a', ' o:title="é😀"', '\u00a0string="nbsp"', ' string="&amp;lt;"',
  '<v:imagedata r:id="rId1" blacklevel="1"/>', '<v:imagedata xgain="1"/>', '<v:imagedata gain="1"/>', '<v:imagedata _gain="1" /> gain=', 'gain=', ' gain=x', 'é gain=', 'blacklevel=',
  '<v:imagedata', '<v:imagedataX a="1">', '<v:imagedata\ta>', '<v:fill opacity=".5"/>', '<v:fillx/><v:fill a="1">', '<v:textpath string="x"', '<v:textpath>', '<v:textpath_>',
  'id="PowerPlusWaterMarkObject"', 'id="WordPictureWatermark99"', 'xid="WordPictureWatermark9"', ' id="WordPictureWatermark9x"', 'id="WordPictureWatermark', 'o:id="PowerPlusWaterMarkObject1"', '_id="PowerPlusWaterMarkObject1"',
  'id="PowerPlusWaterMarkObject1" id="x"', 'ID="PowerPlusWaterMarkObject1"', 'id=\'PowerPlusWaterMarkObject1\'',
  '"', "'", '""', "''", '"a"', "'a'", '"a', 'a"', "'a\"", '"\'', '""a""', "a''", '"a\'b"', "\u201ca\u201d", '', ' ', '_x0000_s1026', '_x0000_', 'x_x0000_', '\u00e9_x0000_',
];
export const INPUTS = [...new Set([...lens, ...fmlas, ...styles, ...xmls])];
