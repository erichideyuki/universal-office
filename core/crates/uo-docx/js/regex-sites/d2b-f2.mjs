// The regex sites of D2b-F2 (lists and styles): numbering-store.ts, universal-write.ts (lists),
// docx-core style-sheet.ts and the editor's styles.ts, against V8, checked by the unit test
// `session::ops::lists::regex_tests` over the matchers of src/session/ops/{lists,styles}. The
// regexes are copied verbatim from the frozen TS; composite rows run the TS code as written.
// Inputs are ASCII (UTF-16 indices equal byte offsets); rows return strings or numbers.

const lvl = (i, extra = '') => `<w:lvl w:ilvl="${i}"${extra}><w:start w:val="1"/><w:numFmt w:val="decimal"/><w:lvlText w:val="%${i + 1}."/><w:lvlJc w:val="left"/><w:pPr><w:ind w:left="${720 * (i + 1)}" w:hanging="360"/></w:pPr></w:lvl>`;
const abs = (id, body, attrs = '') => `<w:abstractNum w:abstractNumId="${id}"${attrs}><w:nsid w:val="0A1B2C3D"/><w:multiLevelType w:val="hybridMultilevel"/><w:tmpl w:val="DEADBEEF"/>${body}</w:abstractNum>`;
const num = (id, a) => `<w:num w:numId="${id}"><w:abstractNumId w:val="${a}"/></w:num>`;
const part = (inner) => `<?xml version="1.0"?>\r\n<w:numbering xmlns:w="w">${inner}</w:numbering>`;

export const INPUTS = [
  '',
  part(''),
  part(abs(0, lvl(0) + lvl(1)) + abs(1, lvl(0, ' w:tplc="04160001"') + lvl(1, ' w:tplc="04160003" w:tentative="1"')) + num(1, 0) + num(2, 1)),
  part(abs(0, lvl(0)) + num(1, 0) + '<w:numIdMacAtCleanup w:val="2"/>'),
  part(abs(12, lvl(0) + lvl(2), ' w15:restartNumberingAfterBreak="0"') + `<w:num w:numId="9"/>` + num(10, 12)),
  part('<w:abstractNum w:abstractNumId="1"><w:numStyleLink w:val="LS"/></w:abstractNum><w:num w:numId="1"><w:abstractNumId w:val="1"/></w:num>'),
  '<w:numbering><w:numFmt w:val="x"/><w:numPr/></w:numbering>',
  '<w:numbering><w:abstractNum w:abstractNumId="1"><w:lvl w:ilvl="0"></w:abstractNum></w:numbering>',
  '<w:numbering><w:num w:numId="1"/><w:num w:numId="2" w:x="1"></w:num></w:numbering>  \n',
  '<w:numbering><w:num w:numId="1">',
  abs(3, lvl(0) + lvl(1) + lvl(2)),
  lvl(0) + lvl(1),
  '<w:lvl w:ilvl="0" w:tplc="0A0B0C0D"/><w:lvl w:ilvl="1" w:tplc="0A0B0C0D" w:tplc="1"></w:lvl>',
  '<w:ind w:left="720" w:hanging="360"/><w:ind w:left="5" w:right="2" w:left="9"/>',
  '<w:ind w:leftx="1" w:left="3"/><xw:ind w:left="4"/><w:ind w:left="4">',
  '<w:start w:val="-3"/><w:start w:val="2"/>',
  '<w:tmpl w:val="0123456"/><w:tmpl w:val="0123456789"/><w:tmpl w:val="0123abcd"/>',
  '<w:style w:styleId="H1"><w:name w:val="heading 1"/><w:pPr><w:keepNext/><w:keepLines w:val="1"/><w:spacing w:after="0"/></w:pPr><w:rPr/></w:style>',
  '<w:style w:styleId="H2"><w:pPr><w:keepNext/><w:keepLines/><w:widowControl/><w:ind w:left="0"/></w:pPr></w:style>',
  '<w:style w:styleId="H3"><w:pPr><w:numPr><w:numId w:val="0"/></w:numPr></w:pPr></w:style>',
  '<w:style w:styleId="H4"><w:pPr><w:numPr><w:ilvl w:val="1"/><w:numId w:val="3"/></w:numPr></w:pPr><w:rPr/></w:style>',
  '<w:style w:styleId="H5"><w:tblPr/></w:style>',
  '<w:style w:styleId="H6"></w:style>',
  '<w:style w:styleId="H7"><w:numPr></w:numPr><w:numPr><w:numId w:val="12"/></w:numPr></w:style>',
  '<w:latentStyles><w:lsdException w:name="heading 1" w:uiPriority="9" w:qFormat="1"/><w:lsdException w:name="heading 1" w:semiHidden="1"/><w:lsdException w:name="Heading 1" w:uiPriority="9"  w:qFormat="0" /></w:latentStyles>',
  '<w:lsdException  w:name="heading 1"\n w:qFormat="1" />',
  '<w:style w:styleId="A"><w:basedOn w:val="X"/><w:next w:val="X"/><w:link w:val="Z"/></w:style><w:basedOn w:val="X"/ ><w:link w:val="X"/>',
  '<w:style><w:basedOn w:val="Y"/><w:next  w:val="X"/><w:next w:val="X"/></w:style>',
];

const re = (p, f = '') => new RegExp(p, f);
const lvlsOf = (s) => [...s.matchAll(/<w:lvl\b[^>]*>[\s\S]*?<\/w:lvl>/g)].map((m) => m[0]);

/** verbatim from numbering-store.ts / universal-write.ts */
const nextId = (xml, attr) => {
  const r = attr === 'w:abstractNumId' ? /<w:abstractNum\b[^>]*\bw:abstractNumId="(\d+)"/g : /<w:num\b[^>]*\bw:numId="(\d+)"/g;
  let max = attr === 'w:abstractNumId' ? -1 : 0;
  for (const m of xml.matchAll(r)) max = Math.max(max, Number(m[1]));
  return max + 1;
};
const tentative = (raw, used) => raw.replace(/<w:lvl\b([^>]*)>/g, (_m, attrs) => {
  const ilvl = Number(/w:ilvl="(\d+)"/.exec(attrs)?.[1] ?? 0);
  const clean = attrs.replace(/ w:tentative="1"/, '');
  return used.has(ilvl) ? `<w:lvl${clean}>` : `<w:lvl${clean} w:tentative="1">`;
});
const putNumPr = (raw, numPr) => {
  let next;
  if (/<w:numPr>[\s\S]*?<\/w:numPr>/.test(raw)) next = raw.replace(/<w:numPr>[\s\S]*?<\/w:numPr>/, () => numPr);
  else if (/<w:pPr>/.test(raw)) next = raw.replace(/<w:pPr>((?:<w:(?:pStyle|keepNext|keepLines|pageBreakBefore|framePr|widowControl)\b[^>]*\/>)*)/, (_m, pre) => `<w:pPr>${pre}${numPr}`);
  else next = raw.replace(/<w:rPr>|<w:tblPr>|<\/w:style>/, (t) => `<w:pPr>${numPr}</w:pPr>${t}`);
  return next;
};
/** style-sheet.ts setLatentQFormat */
const setLatentQFormat = (xml, name, on) => {
  const r = new RegExp(`<w:lsdException\\b[^>]*\\bw:name="${name.replace(/[.*+?^${}()|[\]\\]/g, '\\$&').replace(/"/g, '&quot;')}"[^>]*/>`);
  const m = r.exec(xml);
  if (!m) return xml;
  const tag = m[0];
  let out = tag.replace(/\s+w:qFormat="[^"]*"/, '');
  if (on) out = out.replace(/\s*\/>$/, ' w:qFormat="1"/>');
  return out === tag ? xml : xml.slice(0, m.index) + out + xml.slice(m.index + tag.length);
};
const composite = (f) => [{ [Symbol.replace]: (s) => f(s) }, ''];
const fn = (f) => f;

export const SITES = [
  ['abs.find', fn((x) => { const m = re('<w:abstractNum\\b[^>]*\\bw:abstractNumId="1"[^>]*>[\\s\\S]*?</w:abstractNum>').exec(x); return m ? m[0] : null; }), 'fn'],
  ['abs.find12', fn((x) => { const m = re('<w:abstractNum\\b[^>]*\\bw:abstractNumId="12"[^>]*>[\\s\\S]*?</w:abstractNum>').exec(x); return m ? m[0] : null; }), 'fn'],
  ['num.find', fn((x) => { const m = re('<w:num\\b[^>]*\\bw:numId="1"[^>]*(?:/>|>[\\s\\S]*?</w:num>)').exec(x); return m ? m[0] : null; }), 'fn'],
  ['num.find2', fn((x) => { const m = re('<w:num\\b[^>]*\\bw:numId="2"[^>]*(?:/>|>[\\s\\S]*?</w:num>)').exec(x); return m ? m[0] : null; }), 'fn'],
  ['lvl.find', fn((x) => { const m = re('<w:lvl\\b[^>]*\\bw:ilvl="1"[^>]*>[\\s\\S]*?</w:lvl>').exec(x); return m ? m[0] : null; }), 'fn'],
  ['nextid.abs', fn((x) => nextId(x, 'w:abstractNumId')), 'fn'],
  ['nextid.num', fn((x) => nextId(x, 'w:numId')), 'fn'],
  ['search.abs', /<w:num\b|<w:numIdMacAtCleanup\b|<\/w:numbering>\s*$/, 'index'],
  ['search.num', /<w:numIdMacAtCleanup\b|<\/w:numbering>\s*$/, 'index'],
  ['copy.tplc', [/(<w:lvl\b[^>]*?) w:tplc="[0-9A-Fa-f]+"/g, '$1 w:tplc="FFFFFFFF"'], 'replace'],
  ['copy.absid', [/w:abstractNumId="\d+"/, 'w:abstractNumId="7"'], 'replace'],
  ['copy.nsid', composite((x) => x.replace(/<w:nsid w:val="[0-9A-Fa-f]+"\/>/, () => '<w:nsid w:val="AAAAAAAA"/>')), 'replace'],
  ['setlevels', composite((x) => x.replace(/<w:lvl\b[\s\S]*<\/w:lvl>/, () => 'LVLS')), 'replace'],
  ['mlt', composite((x) => x.replace(/<w:multiLevelType w:val="\w+"\/>/, () => '<w:multiLevelType w:val="multilevel"/>')), 'replace'],
  ['tmpl.set', composite((x) => x.replace(/<w:tmpl w:val="[0-9A-Fa-f]+"\/>/, () => '<w:tmpl w:val="01234567"/>')), 'replace'],
  ['tentative', fn((x) => tentative(x, new Set([0]))), 'fn'],
  ['ind.find', fn((x) => /<w:ind\b[^>]*\/>/.exec(x)?.[0] ?? null), 'fn'],
  ['ind.replace', composite((x) => x.replace(/<w:ind\b[^>]*\/>/, () => '<w:ind/>')), 'replace'],
  ['ilvl.replace', [/w:ilvl="\d+"/, 'w:ilvl="3"'], 'replace'],
  ['indleft', composite((x) => x.replace(/(<w:ind\b[^>]*\bw:left=")(\d+)(")/, (_m, a, v, b) => `${a}${Number(v) + 360}${b}`)), 'replace'],
  ['start.test', /<w:start\b/, 'test'],
  ['start.set', composite((x) => (/<w:start\b/.test(x) ? x.replace(/<w:start w:val="-?\d+"\/>/, () => '<w:start w:val="5"/>') : x.replace(/^<w:lvl\b[^>]*>/, (t) => t + '<w:start w:val="5"/>'))), 'replace'],
  ['tmpl8', /<w:tmpl w:val="([0-9A-Fa-f]{8})"\/>/, 'group'],
  ['numpr.test', /<w:numPr>[\s\S]*?<w:numId w:val="[1-9]\d*"/, 'test'],
  ['put.numpr', composite((x) => putNumPr(x, '<w:numPr><w:numId w:val="9"/></w:numPr>')), 'replace'],
  ['lvl.all', fn((x) => lvlsOf(x)), 'fn'],
  ['lsd.q', composite((x) => setLatentQFormat(x, 'heading 1', true)), 'replace'],
  ['lsd.noq', composite((x) => setLatentQFormat(x, 'heading 1', false)), 'replace'],
  ['refs.in', fn((x) => ([...x.matchAll(/<w:(?:basedOn|next|link) w:val="([^"]*)"\/>/g)].map((m) => m[1]))), 'fn'],
  ['retarget', composite((x) => x.replace(/(<w:(?:basedOn|next|link) w:val=")([^"]*)("\/>)/g, (m, a, v, b) => (v === 'X' ? a + 'Y' + b : m))), 'replace'],
  ['qformat.remove', composite((x) => x.replace(/\s+w:qFormat="[^"]*"/, '')), 'replace'],
];
