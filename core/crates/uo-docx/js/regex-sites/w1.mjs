// The regex sites of the D1b frontier W1 (src/write/emit/run.rs, serialize.ts runs and fields) against V8,
// checked by tests/regex_w1.rs. Composite rows run the TS code copied as written (rows return strings or
// booleans, never positions: UTF-16 indices would differ from Rust's byte offsets).
import { Rng } from '../../../uo-testkit/js/testkit.mjs';

const composite = (f) => [{ [Symbol.replace]: (s) => f(s) }, ''];

// serialize.ts:38–40
const esc = (s) => s.replace(/&/g, '&amp;').replace(/</g, '&lt;').replace(/>/g, '&gt;');

// serialize.ts:448–466 (verbatim)
function textXml(text, deleted = false) {
  const t = deleted ? 'w:delText' : 'w:t';
  const parts = [];
  for (const seg of text.split(/(\t|\n|\f|\u000B|\u2011|\u00AD|\u2028)/)) {
    if (seg === '\t') parts.push('<w:tab/>');
    else if (seg === '\n') parts.push('<w:br/>');
    else if (seg === '\f') parts.push('<w:br w:type="page"/>');
    else if (seg === '\u000B') parts.push('<w:br w:type="column"/>');
    else if (seg === '\u2011') parts.push('<w:noBreakHyphen/>');
    else if (seg === '\u00AD') parts.push('<w:softHyphen/>');
    else if (seg === '\u2028') parts.push('<w:br w:clear="all"/>');
    else if (seg.length > 0) {
      const space = seg !== seg.trim() ? ' xml:space="preserve"' : '';
      parts.push(`<${t}${space}>${esc(seg)}</${t}>`);
    }
  }
  return parts.join('');
}
const fldCharXml = (rpr, type) => `<w:r>${rpr}<w:fldChar w:fldCharType="${type}"/></w:r>`;

// serialize.ts:607–636 (verbatim)
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
function fieldCodeXml(code, flat, rpr, it) {
  const parts = codeParts(code.trim());
  const reSrc = parts.map((p) => (p.text !== undefined ? p.text.replace(/[.*+?^${}()|[\]\\]/g, '\\$&') : '([\\s\\S]*?)')).join('');
  const m = flat !== undefined ? new RegExp(`^${reSrc}$`).exec(flat.trim()) : null;
  let k = 0;
  return parts.map((p) => {
    if (p.text !== undefined) return `<w:r>${rpr}<${it} xml:space="preserve">${esc(p.text)}</${it}></w:r>`;
    const cached = m?.[++k];
    return fldCharXml(rpr, 'begin') + fieldCodeXml(` ${p.nested.trim()} `, undefined, rpr, it) + fldCharXml(rpr, 'separate')
      + (cached ? `<w:r>${rpr}${textXml(cached)}</w:r>` : '') + fldCharXml(rpr, 'end');
  }).join('');
}

// serialize.ts:686–693 (verbatim)
function withNoProof(rpr) {
  if (!rpr || rpr === '<w:rPr/>') return '<w:rPr><w:noProof/></w:rPr>';
  if (/<w:noProof\b/.test(rpr)) return rpr;
  const m = /<w:(snapToGrid|vanish|webHidden|color|spacing|w|kern|position|sz|szCs|highlight|u|effect|bdr|shd|fitText|vertAlign|rtl|cs|em|lang|eastAsianLayout|specVanish|oMath|rPrChange)\b/.exec(rpr);
  const at = m ? m.index : rpr.lastIndexOf('</w:rPr>');
  return rpr.slice(0, at) + '<w:noProof/>' + rpr.slice(at);
}

export const SITES = [
  ['note.mark', /^\s*(FOOTNOTE|ENDNOTE)REF_MARK\s*$/, 'test'],
  ['formula', /^\s*=/, 'test'],
  ['marker', /^\s*(TC|XE|RD)\b/i, 'test'],
  ['hswitch', /\\h\b/i, 'test'],
  ['custom.type', /^[a-zA-Z0-9._-]+$/, 'test'],
  ['text.xml', composite((x) => textXml(x, false)), 'replace'],
  ['text.del', composite((x) => textXml(x, true)), 'replace'],
  ['no.proof', composite(withNoProof), 'replace'],
  ['field.code', composite((x) => {
    const [code, flat] = x.split('§');
    return fieldCodeXml(code, flat, '<w:rPr/>', 'w:instrText');
  }), 'replace'],
  ['uri', composite((x) => { try { return encodeURIComponent(x); } catch { return 'URIError'; } }), 'replace'],
];

// ── inputs: hand-written edge cases and seeded soup over the patterns' vocabulary ──
const HAND = [
  '', 'x', ' ', 'FOOTNOTEREF_MARK', ' ENDNOTEREF_MARK ', '\tFOOTNOTEREF_MARK\n', 'footnoteref_mark', 'FOOTNOTEREF_MARKX', 'XFOOTNOTEREF_MARK', 'FOOTNOTEREF MARK',
  ' FOOTNOTEREF_MARK ', '\uFEFFENDNOTEREF_MARK', '\u0085ENDNOTEREF_MARK', '\u2028ENDNOTEREF_MARK\u2029', 'FOOTNOTE ENDNOTEREF_MARK',
  '=', ' = 1 + 2 ', '\n=SUM(ABOVE)', 'x=', ' =', '\uFEFF=', ' =1',
  'TC', 'tc "a"', ' TC "a" \\f C ', 'XE "x"', 'xe', 'RD file', 'rd', 'TCX', 'TC_', 'TC1', 'TC-1', 'TCé', 'TCé', 'T', 'TCD', 'ſc', 'ıe', 'KE', 'Kc',
  'REF a \\h', 'REF a \\H', 'REF a \\h ', 'REF a \\hx', 'REF a \\h_', 'REF a \\h1', '\\h', '\\', '\\H\\h', 'a\\hé', 'a \\ h', 'a\\\\h', 'a\\h\\h1',
  'abc', 'a.b-c_d', 'a b', 'a/b', '-', '_', '.', 'a\n', 'é', 'clickcompliance.document-tag',
  'a\tb', 'x\ny', 'a\fb', 'a\u000Bb', 'a\u2011b', 'a\u00ADb', 'a\u2028b', '\t', '\n\n', 'a\t\tb', '  lead', 'trail  ', ' ', '  ', ' nb ', 'a\rb', 'a\r\nb',
  '\uFEFFx', 'x\u2029', 'a&b<c>d', '&amp;', '😀\t😀', '\t😀', '😀 ', '　x　', '᠎x', 'a b',
  '<w:rPr/>', '<w:rPr></w:rPr>', '<w:rPr><w:b/></w:rPr>', '<w:rPr><w:noProof/></w:rPr>', '<w:rPr><w:noProofX/></w:rPr>', '<w:rPr><w:rFonts w:ascii="a"/><w:b/><w:sz w:val="2"/></w:rPr>',
  '<w:rPr><w:szCs w:val="2"/></w:rPr>', '<w:rPr><w:sz w:val="2"/><w:szCs w:val="2"/></w:rPr>', '<w:rPr><w:w w:val="90"/></w:rPr>', '<w:rPr><w:wx/></w:rPr>', '<w:rPr><w:lang w:val="x"/></w:rPr>',
  '<w:rPr><w:rPrChange w:id="1"><w:rPr/></w:rPrChange></w:rPr>', '<w:rPr><w:b/><w:i/></w:rPr>', '<w:rPr w:x="1"><w:b/></w:rPr>', '<w:rPr>', 'no tag', '<w:rPr><w:u/>é</w:rPr>', '<w:rPr><w:b/>😀',
  '<w:rPr><w:color w:val="a"/></w:rPr>', '<w:rPr><w:colorX/><w:color/></w:rPr>', '<w:rPr><w:b/><w:noProof /></w:rPr>', '<w:rPr><w:noProof\n/></w:rPr>', '<w:rPr><w:noProof/><w:sz/></w:rPr>', '<w:rPr><w:sz/><w:noProof_/></w:rPr>',
  'x😀', '😀', '<w:rPr>😀', '<w:rPr><w:b/>é',
  'PAGE§PAGE', ' IF { PAGE } > 1 "a" "b" §IF 3 > 1 "a" "b"', '{ A }{ B }§x y', 'a{x}b{y}b§a1b2b3b', 'A{B}C§AxD', '{X}§', 'x§x', '{a{b}c}§zz', 'T.*{X}§T.*v', '$1{X}$§$1q$',
  'a }{ b§a }{ b', '{{}}§x', '{ }§ ', 'a{ b§a{ b', 'a{b}}c§aXc', '{a}{b}§ab', '{a}b{c}§1b2', ' { PAGE } § 7 ', 'x{y}z§xz', '{y}{z}§', '{}§',
];
const VOCAB = ['\t', '\n', '\f', '\u000B', '\u2011', '\u00AD', '\u2028', ' ', 'a', 'b', '<', '>', '&', '"', '=', 'TC', 'XE', 'RD', 'FOOTNOTEREF_MARK', 'ENDNOTEREF_MARK', '\\h', '\\', 'h', '_', '1', '.', '-',
  '{', '}', '§', '<w:rPr>', '</w:rPr>', '<w:noProof/>', '<w:sz/>', '<w:b/>', '<w:color/>', '<w:w/>', '<w:rPrChange>', 'é', '😀', ' ', '\uFEFF', '\u2029', '\u0085', "'", '(', ')', '!', '~', '*', '%', '/', '#'];
function seeded(seed, count) {
  const rng = new Rng(seed);
  const out = [];
  for (let i = 0; i < count; i++) {
    let s = '';
    for (let k = 1 + rng.below(12); k > 0; k--) s += VOCAB[rng.below(VOCAB.length)];
    out.push(s);
  }
  return out;
}
export const INPUTS = [...HAND, ...seeded(0x57a1c0de, 500)];
