// The paragraph quirk corpus of the D1a reader (frontier R4c; extends the generator of R1,
// `quirk-corpus.mjs`): seeded `word/document.xml` texts whose paragraphs hold what the TS
// `readParagraph` has special cases for: complex fields (begin/instr/separate/result/end across
// runs, wrappers and paragraphs, nested, unmatched, with `ffData`), simple fields, hyperlinks,
// bookmarks and comment ranges (ids of every kind), revisions, content controls with custom-inline
// tags (valid, bad JSON, bad percent-encoding, bad type), smart tags, OMML math, `w:sym`, note
// references, tabs, breaks, `w:ptab`, ruby, soft hyphens and CR/LF in text, non-BMP text and
// surrogate character references, plus nesting chains of wrappers.
//
//   node core/crates/uo-docx/js/quirk-paragraph.mjs     writes tests/parity/paragraph.quirks.txt
//
// The file (one case per line, `key<TAB>text` with `\`, LF, CR and TAB escaped) is committed and
// read by both sides (capture-paragraph.mjs, tests/parity_paragraph.rs). `generate()` is
// deterministic. The relationships of every case are `QUIRK_RELS` (hyperlink targets).
import { readFileSync, writeFileSync } from 'node:fs';
import { join, dirname } from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';
import { Gen } from './quirk-corpus.mjs';

const here = dirname(fileURLToPath(import.meta.url));

export const NS = 'xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main" xmlns:w14="http://schemas.microsoft.com/office/word/2010/wordml" '
  + 'xmlns:w15="http://schemas.microsoft.com/office/word/2012/wordml" xmlns:w16du="http://schemas.microsoft.com/office/word/2023/wordml/word16du" '
  + 'xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships" xmlns:m="http://schemas.openxmlformats.org/officeDocument/2006/math" '
  + 'xmlns:mc="http://schemas.openxmlformats.org/markup-compatibility/2006" xmlns:wp="http://schemas.openxmlformats.org/drawingml/2006/wordprocessingDrawing" '
  + 'xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main"';

export const wrap = (inner) => `<w:document ${NS}><w:body>${inner}</w:body></w:document>`;

/** The relationships part every case reads (`word/_rels/document.xml.rels`). */
export const QUIRK_RELS = '<?xml version="1.0" encoding="UTF-8" standalone="yes"?>'
  + '<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">'
  + '<Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/hyperlink" Target="https://example.com/a?b=1&amp;c=2" TargetMode="External"/>'
  + '<Relationship Id="rId2" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/hyperlink" Target="./local.html" TargetMode="External"/>'
  + '<Relationship Id="rId3" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/hyperlink" Target="media/internal.html"/>'
  + '<Relationship Id="rId4" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/hyperlink" Target="" TargetMode="External"/>'
  + '<Relationship Id="rId5" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/hyperlink" Target="mailto:x@y.z" TargetMode="External"/>'
  + '<Relationship Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/hyperlink" Target="https://noid.example" TargetMode="External"/>'
  + '<Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/hyperlink" Target="https://later.example/dup" TargetMode="External"/>'
  + '</Relationships>';

const escAttr = (s) => s.replace(/&/g, '&amp;').replace(/</g, '&lt;').replace(/"/g, '&quot;');
const escText = (s) => s.replace(/&(?!#)/g, '&amp;').replace(/</g, '&lt;');

const TEXTS = ['', 'abc', ' lead trail ', 'a­b', 'x\r\ny', 'x\ry', 'x\nyz', 'é😀', 'pair &#x1F600; ref', 'tab\there', ' nb', 'a b', 'TOC', '12', '­', '\r\n', '\n\n', 'k﻿z', 'a&amp;b'];
const IDS = ['0', '1', '2', '3', '7', '-1', '-0', '1e1', ' 4', '5x', 'NaN', 'Infinity', '', '99999999999999999999', '010', '0x2', '1.9'];
const INSTRS = [' PAGE ', ' NUMPAGES \\* MERGEFORMAT ', ' TOC \\o "1-3" \\h \\z \\u ', ' HYPERLINK "https://x.y" ', ' REF _Ref1 \\h ', ' PAGEREF _Toc1 \\h ', ' SEQ Figure \\* ARABIC ', 'TOC', 'toc', ' XTOC ', ' TOCX ', ' TOC_ ', '  ', '', ' DATE \\@ "dd/MM/yyyy" ', ' IF { PAGE } > 1 "a" "b" ', ' = 1 + 2 ', ' FORMTEXT ', ' FORMCHECKBOX ', ' FORMDROPDOWN ', ' xTOC TOC '];
const TAG_TYPES = ['a', 'clickcompliance.document-tag', 'a.b-c_d9', 'A', 'a b', '', 'é', 'a/b', 'x:y', '-', '9'];
const JSONS = ['{"a":1}', '[1,2,{"b":null}]', '"s"', '1e5', '-0', 'true', 'null', '{"a":1,"a":2}', '{"2":1,"1":2,"b":3,"01":4}', '{', '', "{'a':1}", 'undefined', '{"é":"\\ud800"}', '  [ 1 , 2 ]  ', '{"a":{"b":{"c":[]}}}', '1.', '01', '[1,]', '"\\u00e9\\n"', '{"k":"v:w:x"}', '1e999', '{"__proto__":1}', '"a\nb"', '12345678901234567890', '{"x":0.1e-2}'];

/** Builds a `w:sdt` tag attribute value (`wordnext:inline:<type>:<payload>`). */
function customTag(g) {
  const prefix = g.chance(12) ? g.pick(['wordnext:inline', 'wordnext:inline:', 'WORDNEXT:inline:', 'word:inline:', '']) : 'wordnext:inline:';
  const type = g.pick(TAG_TYPES);
  const json = g.pick(JSONS);
  const enc = g.pick([
    () => encodeURIComponent(json),
    () => encodeURIComponent(json),
    () => json,
    () => encodeURIComponent(json).replace(/%7B/g, '{'),
    () => encodeURIComponent(json) + '%',
    () => '%E2%82%AC' + encodeURIComponent(json),
    () => '%C0%80',
    () => '%ED%A0%80',
    () => '%zz',
    () => '%F0%9F%98%80',
    () => `%22${encodeURIComponent('é')}%22`,
  ])();
  return `${prefix}${type}${g.chance(15) ? '' : ':'}${enc}`;
}

class PGen extends Gen {
  text() { return escText(this.pick(TEXTS)); }
  id() { return this.pick(IDS); }
  rPrOpt() {
    if (this.chance(2)) return '';
    return this.chance(3) ? `<w:rPr>${this.rPr()}</w:rPr>` : this.pick(['<w:rPr><w:b/></w:rPr>', '<w:rPr><w:rFonts w:ascii="Arial"/></w:rPr>', '<w:rPr><w:rStyle w:val="Hyperlink"/></w:rPr>', '<w:rPr><w:color w:val="FF0000"/><w:u w:val="single"/></w:rPr>', '<w:rPr/>', '<w:rPr><w:vertAlign w:val="subscript"/></w:rPr>']);
  }

  ffData() {
    return this.pick([
      () => '',
      () => '<w:ffData><w:checkBox><w:sizeAuto/><w:default w:val="1"/></w:checkBox></w:ffData>',
      () => `<w:ffData><w:checkBox><w:size${this.attr('w:val', IDS)}/><w:default${this.attr('w:val', ['0', '1', 'true', 'false', ''])}/><w:checked${this.attr('w:val', ['0', '1', 'true', 'false', 'off', ''])}/></w:checkBox></w:ffData>`,
      () => '<w:ffData><w:checkBox><w:checked/></w:checkBox></w:ffData>',
      () => `<w:ffData><w:ddList><w:result${this.attr('w:val', IDS)}/><w:default${this.attr('w:val', IDS)}/>${this.some([() => `<w:listEntry${this.attr('w:val', ['A', 'B', '', 'é'])}/>`], 0, 4)}</w:ddList></w:ffData>`,
      () => '<w:ffData><w:ddList/></w:ffData>',
      () => '<w:ffData><w:textInput><w:default w:val="x"/></w:textInput></w:ffData>',
      () => '<w:ffData><w:name w:val="x"/></w:ffData>',
    ])();
  }

  fldChar(kind) {
    const type = kind ?? this.pick(['begin', 'begin', 'separate', 'end', 'end', 'x', '']);
    if (type === 'begin') return `<w:fldChar w:fldCharType="begin">${this.ffData()}</w:fldChar>`;
    if (type === '') return '<w:fldChar/>';
    return `<w:fldChar w:fldCharType="${type}"/>`;
  }

  runChild(depth) {
    const makers = [
      () => `<w:t>${this.text()}</w:t>`, () => `<w:t xml:space="preserve">${this.text()}</w:t>`, () => `<w:delText>${this.text()}</w:delText>`,
      () => `<w:t>${this.text()}</w:t>`, () => '<w:tab/>', () => '<w:br/>', () => `<w:br${this.attr('w:type', ['page', 'column', 'textWrapping', 'x', ''])}${this.attr('w:clear', ['none', 'left', 'all', 'right', ''])}/>`,
      () => `<w:cr/>`, () => '<w:noBreakHyphen/>', () => '<w:softHyphen/>', () => '<w:lastRenderedPageBreak/>', () => '<w:contentPart/>',
      () => `<w:sym${this.attr('w:font', ['Wingdings', 'Symbol', '', 'é'])}${this.attr('w:char', ['F0B7', '41', '0x41', '-5', 'NaN', 'D800', 'DC00', 'FFFFFFFFFF', '', '2612', 'f0fc', '0', '-0', 'zz', 'FF', '100', 'D83D'])}/>`,
      () => `<w:footnoteReference${this.attr('w:id', IDS)}${this.attr('w:customMarkFollows', ['1', 'true', '0', 'false', ''])}/>`,
      () => `<w:endnoteReference${this.attr('w:id', IDS)}${this.attr('w:customMarkFollows', ['1', 'true', '0', ''])}/>`,
      () => '<w:footnoteRef/>', () => '<w:endnoteRef/>', () => '<w:pgNum/>',
      () => `<w:ptab${this.attr('w:alignment', ['left', 'center', 'right', 'x', ''])}${this.attr('w:relativeTo', ['margin', 'indent', 'x', ''])}${this.attr('w:leader', ['none', 'dot', 'x', ''])}/>`,
      () => `<w:instrText>${this.pick(INSTRS)}</w:instrText>`, () => `<w:instrText xml:space="preserve">${this.pick(INSTRS)}</w:instrText>`, () => `<w:delInstrText>${this.pick(INSTRS)}</w:delInstrText>`,
      () => this.fldChar(), () => this.fldChar(), () => this.fldChar(),
      () => '<w:unknownChild>zz</w:unknownChild>',
    ];
    if (depth < 3) {
      makers.push(
        () => `<w:ruby><w:rubyPr/><w:rt><w:r><w:t>rt</w:t></w:r></w:rt><w:rubyBase>${this.runs(depth + 1, 3)}</w:rubyBase></w:ruby>`,
        () => '<w:ruby><w:rt><w:r><w:t>rt</w:t></w:r></w:rt></w:ruby>',
        () => `<w:ruby><w:rubyBase>${this.some([() => this.run(depth + 1)], 0, 2)}</w:rubyBase></w:ruby>`,
      );
    }
    return this.pick(makers)();
  }

  run(depth = 0) {
    return `<w:r>${this.rPrOpt()}${this.some([() => this.runChild(depth)], 0, 5)}</w:r>`;
  }

  runs(depth, max = 4) { return this.some([() => this.run(depth)], 0, max); }

  /** A coordinated complex field spread over a few runs. */
  fieldSeq(depth) {
    const instr = this.pick(INSTRS);
    const parts = [];
    parts.push(`<w:r>${this.rPrOpt()}${this.fldChar('begin')}</w:r>`);
    parts.push(`<w:r>${this.rPrOpt()}<w:instrText xml:space="preserve">${instr}</w:instrText></w:r>`);
    if (depth < 2 && this.chance(3)) parts.push(this.fieldSeq(depth + 1));
    if (this.chance(6)) parts.push(`<w:r><w:instrText>${this.pick(INSTRS)}</w:instrText></w:r>`);
    if (!this.chance(8)) parts.push(`<w:r>${this.fldChar('separate')}</w:r>`);
    parts.push(this.some([() => this.run(depth + 1), () => `<w:r>${this.rPrOpt()}<w:t>${this.text()}</w:t></w:r>`, () => this.inline(depth + 1)], 0, 3));
    if (depth < 2 && this.chance(4)) parts.push(this.fieldSeq(depth + 1));
    if (!this.chance(8)) parts.push(`<w:r>${this.fldChar('end')}</w:r>`);
    return parts.join('');
  }

  mathNodes(depth) {
    const t = () => `<m:t>${this.pick(['x', '1', 'é', '', 'a b', '😀'])}</m:t>`;
    const r = () => `<m:r>${this.pick(['', '<m:rPr><m:sty m:val="p"/></m:rPr>', '<m:rPr><m:nor/></m:rPr>', '<m:rPr><m:sty/></m:rPr>', '<m:rPr/>'])}${this.pick(['', '<w:rPr><w:b/><w:sz w:val="1e1"/></w:rPr>', '<w:rPr/>'])}${this.some([t, () => '<w:br/>', t], 0, 3)}</m:r>`;
    const makers = [r, r, () => `<w:r><w:t>w</w:t></w:r>`, () => '<m:ctrlPr/>', () => '<m:rPr/>'];
    if (depth < 3) {
      makers.push(
        () => `<m:f>${this.pick(['', '<m:fPr/>', '<m:fPr><m:type m:val="lin"/></m:fPr>', '<m:fPr><m:type/></m:fPr>'])}${this.pick(['', `<m:num>${this.mathNodes(depth + 1)}</m:num>`])}${this.pick(['', `<m:den>${this.mathNodes(depth + 1)}</m:den>`])}</m:f>`,
        () => `<m:d>${this.pick(['', '<m:dPr><m:begChr m:val="["/><m:sepChr m:val=","/><m:endChr m:val="]"/></m:dPr>', '<m:dPr><m:begChr/><m:endChr/></m:dPr>', '<m:dPr><m:sepChr m:val=""/></m:dPr>'])}${this.some([() => `<m:e>${this.mathNodes(depth + 1)}</m:e>`], 0, 3)}</m:d>`,
        () => `<m:sSup><m:e>${this.mathNodes(depth + 1)}</m:e><m:sup>${this.mathNodes(depth + 1)}</m:sup></m:sSup>`,
        () => `<m:rad><m:radPr/><m:deg/><m:e>${this.mathNodes(depth + 1)}</m:e></m:rad>`,
      );
    }
    return this.some(makers, 0, 4);
  }

  inline(depth = 0) {
    const makers = [
      () => this.run(depth), () => this.run(depth), () => this.run(depth), () => this.run(depth),
      () => this.fieldSeq(depth), () => this.fieldSeq(depth),
      () => `<w:bookmarkStart${this.attr('w:id', IDS)}${this.attr('w:name', ['_Toc1', 'bm', '', 'é', '_GoBack', 'a b'])}${this.chance(8) ? ' w:colFirst="0"' : ''}/>`,
      () => `<w:bookmarkEnd${this.attr('w:id', IDS)}/>`,
      () => `<w:commentRangeStart${this.attr('w:id', IDS)}/>`, () => `<w:commentRangeEnd${this.attr('w:id', IDS)}/>`,
      () => `<w:commentRangeStart w:id="${this.pick(['0', '1', '2'])}"/>${this.runs(depth)}<w:commentRangeEnd w:id="${this.pick(['0', '1', '2'])}"/>`,
      () => `<m:oMath>${this.mathNodes(0)}</m:oMath>`,
      () => `<m:oMathPara>${this.pick(['', '<m:oMathParaPr/>', '<m:oMathParaPr><m:jc m:val="left"/></m:oMathParaPr>', '<m:oMathParaPr><m:jc/></m:oMathParaPr>'])}${this.some([() => `<m:oMath>${this.mathNodes(0)}</m:oMath>`], 0, 3)}</m:oMathPara>`,
      () => `<w:pPr><w:jc w:val="right"/></w:pPr>`,
      () => '<w:proofErr w:type="spellStart"/>', () => '<w:unknownInline><w:r><w:t>ignored</w:t></w:r></w:unknownInline>',
    ];
    if (depth < 4) {
      makers.push(
        () => `<w:hyperlink${this.pick([' r:id="rId1"', ' r:id="rId2"', ' r:id="rId3"', ' r:id="rId4"', ' r:id="rId5"', ' r:id="rId99"', ' r:id=""', '', ' w:anchor="_Toc1"', ' r:id="rId4" w:anchor="x"', ' r:id="rId1" w:anchor="x"', ' w:anchor=""'])}${this.chance(3) ? ` w:tooltip="${escAttr(this.pick(['tip', '', 'é']))}"` : ''}>${this.some([() => this.inline(depth + 1)], 0, 3)}</w:hyperlink>`,
        () => `<w:fldSimple${this.attr('w:instr', INSTRS.map(escAttr))}>${this.some([() => this.inline(depth + 1)], 0, 3)}</w:fldSimple>`,
        () => `<w:sdt><w:sdtPr>${this.pick(['', `<w:tag w:val="${escAttr(customTag(this))}"/>`, `<w:tag w:val="${escAttr(customTag(this))}"/>`, '<w:tag/>', '<w:alias w:val="x"/>'])}</w:sdtPr>${this.pick([`<w:sdtContent>${this.some([() => this.inline(depth + 1)], 0, 3)}</w:sdtContent>`, `<w:sdtContent>${this.some([() => this.inline(depth + 1)], 0, 3)}</w:sdtContent>`, '', '<w:sdtContent/>', '<w:sdtContent><w:sdt><w:sdtPr/><w:sdtContent><w:r><w:t>deep</w:t></w:r></w:sdtContent></w:sdt></w:sdtContent>'])}</w:sdt>`,
        () => `<w:sdt>${this.pick(['<w:sdtContent><w:r><w:t>no pr</w:t></w:r></w:sdtContent>', '<w:sdtPr><w:id w:val="1"/></w:sdtPr><w:wrap><w:sdtContent><w:r><w:t>wrapped</w:t></w:r></w:sdtContent></w:wrap>'])}</w:sdt>`,
        () => { const tag = this.pick(['w:smartTag', 'w:customXml', 'w:dir', 'w:bdo']); return `<${tag}>${this.some([() => this.inline(depth + 1)], 0, 3)}</${tag}>`; },
        () => `<w:ins${this.attr('w:author', ['a', '', 'é'])}${this.attr('w:date', ['2024-01-02T03:04:05Z', ''])}${this.attr('w16du:dateUtc', ['2024-01-02T03:04:05Z', ''])}>${this.some([() => this.inline(depth + 1)], 0, 3)}</w:ins>`,
        () => `<w:del${this.attr('w:author', ['b', ''])}${this.attr('w:date', ['2024-01-02T03:04:05Z', ''])}>${this.some([() => this.inline(depth + 1)], 0, 3)}</w:del>`,
        () => `<w:moveFrom${this.attr('w:author', ['c', ''])}>${this.some([() => this.inline(depth + 1)], 0, 2)}</w:moveFrom>`,
        () => `<w:moveTo${this.attr('w:author', ['d', ''])}>${this.some([() => this.inline(depth + 1)], 0, 2)}</w:moveTo>`,
      );
    }
    return this.pick(makers)();
  }

  paragraph() {
    const ppr = this.chance(3) ? `<w:pPr>${this.pPr()}</w:pPr>` : this.pick(['', '', '<w:pPr/>', '<w:pPr><w:pStyle w:val="TOC1"/></w:pPr>']);
    return `<w:p>${ppr}${this.some([() => this.inline(0)], 0, 8)}</w:p>`;
  }
}

/** Closing tags matched to the opening ones of a `fixed` case. */
const P = (inner, ppr = '') => `<w:p>${ppr}${inner}</w:p>`;
const R = (inner, rpr = '') => `<w:r>${rpr}${inner}</w:r>`;
const T = (s) => `<w:t>${s}</w:t>`;
const FB = '<w:fldChar w:fldCharType="begin"/>';
const FS = '<w:fldChar w:fldCharType="separate"/>';
const FE = '<w:fldChar w:fldCharType="end"/>';
const IT = (s) => `<w:instrText xml:space="preserve">${s}</w:instrText>`;

function fixed() {
  const c = [];
  const add = (k, inner) => c.push([k, wrap(inner)]);
  add('empty-body', '');
  add('simple-field-page', P(R(FB) + R(IT(' PAGE ')) + R(FS) + R(T('3'), '<w:rPr><w:b/></w:rPr>') + R(FE)));
  add('toc-across-paragraphs', P(R(FB) + R(IT(' TOC \\o "1-3" \\h ')) + R(FS) + R(T('Entry 1'))) + P(R(T('Entry 2'))) + P(R(T('Entry 3')) + R(FE)) + P(R(T('after'))));
  add('toc-unclosed', P(R(FB) + R(IT(' TOC ')) + R(FS) + R(T('a'))) + P(R(T('b'))));
  add('toc-instr-only-paragraph', P(R(FB) + R(IT(' TOC \\o ')) + R(FS)) + P(R(T('x')) + R(FE)));
  add('toc-word-boundaries', P(R(IT('XTOC'))) + P(R(IT('TOC'))) + P(R(IT('TOC1'))) + P(R(IT('a TOC b'))) + P(R(IT('toc'))) + P(R(IT('TOC_x'))));
  add('nested-field-in-instr', P(R(FB) + R(IT(' IF ')) + R(FB) + R(IT(' PAGE ')) + R(FS) + R(T('1')) + R(FE) + R(IT(' > 0 "x" "y" ')) + R(FS) + R(T('x')) + R(FE)));
  add('nested-field-in-result', P(R(FB) + R(IT(' REF a ')) + R(FS) + R(T('r1')) + R(FB) + R(IT(' PAGE ')) + R(FS) + R(T('2')) + R(FE) + R(T('r2')) + R(FE)));
  add('field-in-field-in-instr-deep', P(R(FB) + R(IT('A ')) + R(FB) + R(IT('B ')) + R(FB) + R(IT('C ')) + R(FS) + R(T('c')) + R(FE) + R(FS) + R(T('b')) + R(FE) + R(FS) + R(T('a')) + R(FE)));
  add('unmatched-end', P(R(FE) + R(T('x')) + R(FE)));
  add('unmatched-begin', P(R(FB) + R(IT(' X ')) + R(T('dropped'))));
  add('separate-without-begin', P(R(FS) + R(T('x'))));
  add('instr-in-result-phase', P(R(FB) + R(IT(' X ')) + R(FS) + R(IT(' ignored ')) + R(T('res')) + R(FE)));
  add('del-instr', P(R(FB) + '<w:r><w:delInstrText> DEL </w:delInstrText></w:r>' + R(FS) + R(T('r')) + R(FE)));
  add('fldchar-bad-type', P(R('<w:fldChar w:fldCharType="x"/>' + T('a') + '<w:fldChar/>' + T('b'))));
  add('field-inside-one-run', P(R(FB + IT(' PAGE ') + FS + T('4') + FE + T('tail'))));
  add('field-object-and-text-result', P(R(FB) + R(IT(' X ')) + R(FS) + R(T('a')) + R('<w:pgNum/>') + R('<w:footnoteRef/>') + R(FE)));
  add('field-with-formatting-first-run', P(R(FB, '<w:rPr><w:i/></w:rPr>') + R(IT(' X '), '<w:rPr><w:b/></w:rPr>') + R(FS) + R(T('first'), '<w:rPr><w:u w:val="single"/></w:rPr>') + R(T('second'), '<w:rPr><w:strike/></w:rPr>') + R(FE)));
  add('field-empty-result', P(R(FB) + R(IT(' X ')) + R(FS) + R(FE)));
  add('field-no-separate', P(R(FB) + R(IT(' X ')) + R(T('ignored')) + R(FE)));
  add('formfield-checkbox', P(R(FB + '<w:fldChar w:fldCharType="begin"><w:ffData><w:checkBox><w:sizeAuto/><w:default w:val="1"/></w:checkBox></w:ffData></w:fldChar>', '<w:rPr><w:b/></w:rPr>') + R(IT(' FORMCHECKBOX ')) + R(FS) + R(FE)));
  add('formfield-checkbox-sizes', P(R('<w:fldChar w:fldCharType="begin"><w:ffData><w:checkBox><w:size w:val="24"/><w:checked w:val="0"/></w:checkBox></w:ffData></w:fldChar>') + R(FE)) + P(R('<w:fldChar w:fldCharType="begin"><w:ffData><w:checkBox><w:size w:val=""/><w:checked/></w:checkBox></w:ffData></w:fldChar>') + R(FE)) + P(R('<w:fldChar w:fldCharType="begin"><w:ffData><w:checkBox><w:size w:val="x"/><w:sizeAuto/><w:default w:val="false"/></w:checkBox></w:ffData></w:fldChar>') + R(FE)));
  add('formfield-dropdown', P(R('<w:fldChar w:fldCharType="begin"><w:ffData><w:ddList><w:result w:val="1"/><w:listEntry w:val="A"/><w:listEntry w:val="B"/><w:listEntry/></w:ddList></w:ffData></w:fldChar>') + R(IT(' FORMDROPDOWN ')) + R(FS) + R(T('cached')) + R(FE)));
  add('formfield-dropdown-index-edge', ['-0', '9', '1.5', ' 1', '1e0', 'x', '', '-1', '2'].map((v) => P(R(`<w:fldChar w:fldCharType="begin"><w:ffData><w:ddList><w:result w:val="${v}"/><w:default w:val="1"/><w:listEntry w:val="A"/><w:listEntry w:val="B"/><w:listEntry w:val="C"/></w:ddList></w:ffData></w:fldChar>`) + R(FE))).join(''));
  add('formfield-textinput', P(R('<w:fldChar w:fldCharType="begin"><w:ffData><w:textInput><w:default w:val="v"/></w:textInput></w:ffData></w:fldChar>') + R(IT(' FORMTEXT ')) + R(FS) + R(T('text')) + R(FE)));
  add('fldsimple', P('<w:fldSimple w:instr=" PAGE \\* MERGEFORMAT ">' + R(T('9'), '<w:rPr><w:b/></w:rPr>') + '</w:fldSimple>') + P('<w:fldSimple/>') + P('<w:fldSimple w:instr="X">' + R(T('a')) + R(T('b'), '<w:rPr><w:i/></w:rPr>') + '</w:fldSimple>'));
  add('fldsimple-nested', P('<w:fldSimple w:instr="A"><w:fldSimple w:instr="B">' + R(T('in')) + '</w:fldSimple>' + R(T('out')) + '</w:fldSimple>'));
  add('fldsimple-in-complex-field', P(R(FB) + R(IT(' A ')) + '<w:fldSimple w:instr="B">' + R(T('s')) + '</w:fldSimple>' + R(FS) + '<w:fldSimple w:instr="C">' + R(T('c')) + '</w:fldSimple>' + R(FE)));
  add('fldsimple-with-complex-inside', P('<w:fldSimple w:instr="O">' + R(FB) + R(IT(' P ')) + R(FS) + R(T('r')) + R(FE) + '</w:fldSimple>'));
  add('fldsimple-with-open-field-inside', P('<w:fldSimple w:instr="O">' + R(FB) + R(IT(' P ')) + R(FS) + R(T('r')) + '</w:fldSimple>' + R(T('after')) + R(FE)));
  add('hyperlinks', P('<w:hyperlink r:id="rId1" w:tooltip="Tip">' + R(T('ext')) + R(T('b'), '<w:rPr><w:b/><w:color w:val="00FF00"/></w:rPr>') + '</w:hyperlink>' + '<w:hyperlink w:anchor="bm">' + R(T('anc'), '<w:rPr><w:u w:val="none"/></w:rPr>') + '</w:hyperlink>' + '<w:hyperlink r:id="rId3">' + R(T('internal')) + '</w:hyperlink>' + '<w:hyperlink r:id="rId2" w:anchor="z">' + R(T('dot-slash')) + '</w:hyperlink>' + '<w:hyperlink r:id="rId4" w:anchor="y">' + R(T('empty-target')) + '</w:hyperlink>' + '<w:hyperlink r:id="rId77">' + R(T('missing')) + '</w:hyperlink>'));
  add('hyperlinks-nested', P('<w:hyperlink r:id="rId1" w:tooltip="outer"><w:hyperlink r:id="rId99">' + R(T('inner no link keeps tip')) + '</w:hyperlink>' + '<w:hyperlink w:anchor="a" w:tooltip="inner">' + R(T('inner anchor')) + '</w:hyperlink>' + '<w:ins w:author="x"><w:hyperlink>' + R(T('ins-hl')) + '</w:hyperlink></w:ins>' + '</w:hyperlink>'));
  add('hyperlink-with-field', P('<w:hyperlink r:id="rId1">' + R(FB) + R(IT(' PAGEREF x ')) + R(FS) + R(T('2')) + R(FE) + '</w:hyperlink>'));
  add('hyperlink-around-field-ends', P('<w:hyperlink r:id="rId5">' + R(FB) + R(IT(' X ')) + '</w:hyperlink>' + R(FS) + R(T('res')) + R(FE)));
  add('hyperlink-sym-and-note', P('<w:hyperlink r:id="rId1">' + R('<w:sym w:font="Wingdings" w:char="F0FC"/>') + R('<w:footnoteReference w:id="2"/>') + '</w:hyperlink>'));
  add('bookmarks', P('<w:bookmarkStart w:id="1" w:name="a"/>' + R(T('xy')) + '<w:bookmarkEnd w:id="1"/>' + '<w:bookmarkStart w:id="2" w:name="b"/><w:bookmarkEnd w:id="2"/>' + '<w:bookmarkEnd w:id="9"/>' + '<w:bookmarkStart w:id="3" w:name=""/><w:bookmarkEnd w:id="3"/>' + '<w:bookmarkStart w:id="x" w:name="nan"/><w:bookmarkEnd w:id="x"/><w:bookmarkEnd/>'));
  add('bookmarks-in-hyperlink', P('<w:hyperlink r:id="rId1" w:tooltip="t"><w:bookmarkStart w:id="4" w:name="h"/>' + R(T('t')) + '<w:bookmarkEnd w:id="4"/></w:hyperlink>' + '<w:bookmarkStart w:id="5" w:name="col" w:colFirst="1"/><w:bookmarkEnd w:id="5"/>'));
  add('bookmark-in-field', P(R(FB) + R(IT(' X ')) + '<w:bookmarkStart w:id="1" w:name="inf"/>' + R(FS) + '<w:bookmarkStart w:id="2" w:name="inres"/>' + R(T('r')) + '<w:bookmarkEnd w:id="2"/>' + R(FE) + '<w:bookmarkEnd w:id="1"/>'));
  add('bookmark-across-paragraphs', P('<w:bookmarkStart w:id="1" w:name="across"/>' + R(T('a'))) + P(R(T('b')) + '<w:bookmarkEnd w:id="1"/>') + P('<w:bookmarkStart w:id="1" w:name="reused"/><w:bookmarkEnd w:id="1"/>'));
  add('bookmark-duplicate-ids', P('<w:bookmarkStart w:id="1" w:name="one"/><w:bookmarkStart w:id="1" w:name="two"/><w:bookmarkEnd w:id="1"/><w:bookmarkEnd w:id="1"/>'));
  add('bookmark-infinite-id', P(`<w:bookmarkStart w:id="${'9'.repeat(320)}" w:name="inf"/><w:bookmarkEnd w:id="${'9'.repeat(320)}"/>`));
  add('comments', P('<w:commentRangeStart w:id="0"/>' + R(T('c0')) + '<w:commentRangeStart w:id="1"/>' + R(T('c01')) + '<w:commentRangeEnd w:id="0"/>' + R(T('c1')) + '<w:commentRangeEnd w:id="1"/>' + R(T('none'))) + P('<w:commentRangeStart w:id="x"/><w:commentRangeStart w:id="NaN"/>' + R(T('nan')) + '<w:commentRangeEnd w:id="y"/>' + R(T('still')) + '<w:commentRangeStart w:id="-0"/><w:commentRangeStart w:id="0"/>' + R(T('zero'))));
  add('comments-reentry-order', P('<w:commentRangeStart w:id="1"/><w:commentRangeStart w:id="2"/><w:commentRangeEnd w:id="1"/><w:commentRangeStart w:id="1"/>' + R(T('x')) + '<w:commentRangeStart w:id="3"/>' + R(FB) + R(IT(' X ')) + R(FS) + R(T('r')) + R(FE)));
  add('comments-in-hyperlink-and-field', P('<w:commentRangeStart w:id="5"/><w:hyperlink r:id="rId1">' + R(T('hl')) + '</w:hyperlink>' + R(FB) + R(IT(' X ')) + R(FS) + R(T('in field')) + R(FE) + '<w:commentRangeEnd w:id="5"/>'));
  add('revisions', P('<w:ins w:id="1" w:author="Ann" w:date="2024-01-02T03:04:05Z" w16du:dateUtc="2024-01-02T03:04:05Z">' + R(T('inserted')) + '</w:ins>' + '<w:del w:author="Bob">' + R('<w:delText>deleted</w:delText>') + '</w:del>' + '<w:moveFrom w:author="C">' + R(T('mf')) + '</w:moveFrom>' + '<w:moveTo w:author="D" w:date="x">' + R(T('mt')) + '</w:moveTo>' + '<w:ins>' + R(T('no author')) + '</w:ins>'));
  add('revisions-nested', P('<w:ins w:author="A"><w:del w:author="B">' + R('<w:delText>both</w:delText>') + '</w:del><w:hyperlink r:id="rId1">' + R(T('hl in ins')) + '</w:hyperlink></w:ins>' + '<w:del w:author="B"><w:ins w:author="A">' + R(T('ins in del')) + '</w:ins></w:del>'));
  add('revision-field', P('<w:ins w:author="A">' + R(FB) + R(IT(' X ')) + R(FS) + '</w:ins>' + '<w:del w:author="B">' + R(T('res')) + R(FE) + '</w:del>'));
  add('revision-with-comment-and-link', P('<w:commentRangeStart w:id="3"/><w:ins w:author="Z"><w:hyperlink r:id="rId1">' + R(T('all')) + '</w:hyperlink></w:ins><w:commentRangeEnd w:id="3"/>'));
  add('sdt-plain', P('<w:sdt><w:sdtPr><w:alias w:val="x"/></w:sdtPr><w:sdtContent>' + R(T('in sdt')) + '</w:sdtContent></w:sdt>' + '<w:sdt><w:sdtPr/></w:sdt>' + '<w:sdt><w:sdtContent/></w:sdt>'));
  add('sdt-custom-valid', P(`<w:sdt><w:sdtPr><w:tag w:val="wordnext:inline:clickcompliance.document-tag:${encodeURIComponent('{"tag":"name","label":"Nome é"}')}"/></w:sdtPr><w:sdtContent>` + R(T('fallback '), '<w:rPr><w:b/></w:rPr>') + R(T('text')) + '</w:sdtContent></w:sdt>'));
  add('sdt-custom-no-content-runs', P(`<w:sdt><w:sdtPr><w:tag w:val="wordnext:inline:a:%7B%7D"/></w:sdtPr><w:sdtContent/></w:sdt>` + `<w:sdt><w:sdtPr><w:tag w:val="wordnext:inline:a:1"/></w:sdtPr></w:sdt>`));
  add('sdt-custom-bad-forms', ['wordnext:inline:', 'wordnext:inline::1', 'wordnext:inline:a', 'wordnext:inline:a b:1', 'wordnext:inline:a:%', 'wordnext:inline:a:%C0%80', 'wordnext:inline:a:{', 'wordnext:inline:é:1', 'wordnext:inline:a:1:2', 'x', ''].map((t) => P(`<w:sdt><w:sdtPr><w:tag w:val="${escAttr(t)}"/></w:sdtPr><w:sdtContent>` + R(T('t')) + '</w:sdtContent></w:sdt>')).join(''));
  add('sdt-custom-json-forms', JSONS.map((j) => P(`<w:sdt><w:sdtPr><w:tag w:val="wordnext:inline:a:${encodeURIComponent(j)}"/></w:sdtPr><w:sdtContent>` + R(T('t')) + '</w:sdtContent></w:sdt>')).join(''));
  add('sdt-custom-nested-and-fields', P(`<w:sdt><w:sdtPr><w:tag w:val="wordnext:inline:o:%22x%22"/></w:sdtPr><w:sdtContent>` + R(FB) + R(IT(' X ')) + R(FS) + R(T('r')) + R(FE) + `<w:sdt><w:sdtPr><w:tag w:val="wordnext:inline:i:1"/></w:sdtPr><w:sdtContent>` + R(T('in')) + '</w:sdtContent></w:sdt>' + '</w:sdtContent></w:sdt>'));
  add('sdt-custom-opens-field', P(`<w:sdt><w:sdtPr><w:tag w:val="wordnext:inline:o:1"/></w:sdtPr><w:sdtContent>` + R(FB) + R(IT(' X ')) + R(FS) + R(T('r')) + '</w:sdtContent></w:sdt>' + R(T('after')) + R(FE)));
  add('sdt-deep-sdtcontent', P('<w:sdt><w:sdtPr/><w:wrapper><w:sdtContent>' + R(T('deep find')) + '</w:sdtContent></w:wrapper></w:sdt>'));
  add('sdt-first-pr-last-content', P('<w:sdt><w:sdtContent>' + R(T('c1')) + '</w:sdtContent><w:sdtPr><w:tag w:val="wordnext:inline:a:1"/></w:sdtPr></w:sdt>'));
  add('smarttag-and-friends', P('<w:smartTag w:uri="x" w:element="y">' + R(T('st')) + '</w:smartTag><w:customXml w:element="c">' + R(T('cx')) + '</w:customXml><w:dir w:val="rtl">' + R(T('dir')) + '</w:dir><w:bdo w:val="ltr">' + R(T('bdo')) + '</w:bdo>'));
  add('math-inline', P('<m:oMath><m:r><m:t>x</m:t></m:r><m:sSup><m:e><m:r><m:t>y</m:t></m:r></m:e><m:sup><m:r><m:t>2</m:t></m:r></m:sup></m:sSup></m:oMath>' + '<m:oMath><m:ctrlPr/></m:oMath>' + '<m:oMath><m:r><m:t></m:t></m:r></m:oMath>'));
  add('math-display', P('<m:oMathPara><m:oMathParaPr><m:jc m:val="left"/></m:oMathParaPr><m:oMath><m:f><m:fPr><m:type m:val="lin"/></m:fPr><m:num><m:r><m:t>a</m:t></m:r></m:num><m:den><m:r><m:rPr><m:sty m:val="b"/></m:rPr><w:rPr><w:b/></w:rPr><m:t>b</m:t><w:br/><m:t>c</m:t></m:r></m:den></m:f></m:oMath><m:oMath><m:d><m:dPr><m:begChr m:val="["/><m:sepChr m:val=";"/></m:dPr><m:e><m:r><m:t>1</m:t></m:r></m:e><m:e><m:r><m:rPr><m:nor/></m:rPr><m:t>2</m:t></m:r></m:e><m:e/></m:d></m:oMath></m:oMathPara>'));
  add('math-delims-defaults', P('<m:oMathPara><m:oMath><m:d><m:e><m:r><m:t>a</m:t></m:r></m:e><m:e><m:r><m:t>b</m:t></m:r></m:e></m:d></m:oMath><m:oMath><m:d><m:dPr><m:begChr/><m:sepChr/><m:endChr/></m:dPr><m:e><m:r><m:t>q</m:t></m:r></m:e><m:e><m:r><m:t>w</m:t></m:r></m:e></m:d></m:oMath></m:oMathPara>'));
  add('math-nested-fractions', P('<m:oMathPara><m:oMath><m:f><m:num><m:f><m:num><m:r><m:t>1</m:t></m:r></m:num><m:den><m:r><m:t>2</m:t></m:r></m:den></m:f></m:num><m:den><m:d><m:e><m:f><m:num/><m:den><m:r><m:t>3</m:t></m:r></m:den></m:f></m:e></m:d></m:den></m:f></m:oMath></m:oMathPara>'));
  add('math-oMathPara-jc-empty', P('<m:oMathPara><m:oMathParaPr><m:jc/></m:oMathParaPr><m:oMath><m:r><m:t>q</m:t></m:r></m:oMath></m:oMathPara>' + '<m:oMathPara><m:oMathParaPr><m:jc m:val=""/></m:oMathParaPr><m:oMath><m:r><m:t>z</m:t></m:r></m:oMath></m:oMathPara>' + '<m:oMathPara><m:oMath/><m:oMath><m:r><m:t>w</m:t></m:r></m:oMath></m:oMathPara>'));
  add('math-in-field-result', P(R(FB) + R(IT(' EQ ')) + R(FS) + '<m:oMath><m:r><m:t>x</m:t></m:r></m:oMath>' + R(FE) + '<m:oMath><m:r><m:t>after</m:t></m:r></m:oMath>'));
  add('math-in-instr', P(R(FB) + '<m:oMath><m:r><m:t>dropped</m:t></m:r></m:oMath>' + R(IT(' X ')) + R(FS) + R(FE)));
  add('sym-chars', ['F0B7', '41', '0x41', '-5', 'NaN', 'D800', 'DC00', 'FFFFFFFFFF', '', '2612', 'f0fc', '0', '-0', 'zz', 'FF', '100', 'D83D', '-0x1', '1e3'].map((v) => P(R(`<w:sym w:font="Wingdings" w:char="${v}"/>`) + R(`<w:sym w:char="${v}"/>`, '<w:rPr><w:rFonts w:ascii="Arial"/></w:rPr>'))).join('') + P(R('<w:sym/>')) + P(R('<w:sym w:font=""/>', '<w:rPr><w:rFonts w:hAnsi="H"/></w:rPr>')));
  add('sym-in-link-and-toc', P(R(FB) + R(IT(' TOC ')) + R(FS) + R('<w:sym w:font="W" w:char="F0B7"/>' + T('t')) + R(FE)) + P('<w:hyperlink r:id="rId1">' + R('<w:sym w:char="F0B7"/>') + '</w:hyperlink>'));
  add('note-refs', P(R('<w:footnoteReference w:id="1"/>') + R('<w:endnoteReference w:id="2" w:customMarkFollows="1"/>') + R('<w:footnoteReference/>') + R('<w:endnoteReference w:id="x" w:customMarkFollows="true"/>') + R('<w:footnoteReference w:id="3" w:customMarkFollows="0"/>', '<w:rPr><w:vertAlign w:val="subscript"/></w:rPr>') + R('<w:footnoteReference w:id="4"/>', '<w:rPr><w:vertAlign w:val="baseline"/></w:rPr>') + R('<w:footnoteRef/>') + R('<w:endnoteRef/>')));
  add('note-ref-in-hyperlink-toc-comment', P('<w:commentRangeStart w:id="1"/>' + R(FB) + R(IT(' TOC ')) + R(FS) + '<w:hyperlink r:id="rId1">' + R('<w:footnoteReference w:id="7"/>') + '</w:hyperlink>' + R(FE) + '<w:commentRangeEnd w:id="1"/>'));
  add('text-controls', P(R('<w:t>a­b</w:t><w:tab/><w:t>c\r\nd</w:t><w:br/><w:t>e\rf\ng</w:t><w:noBreakHyphen/><w:softHyphen/><w:cr/>') + R('<w:br w:type="page"/><w:br w:type="column"/><w:br w:clear="all"/><w:br w:clear="none"/><w:br w:type="textWrapping" w:clear="left"/><w:br w:type="x"/>') + R('<w:delText>x­</w:delText>')));
  add('text-with-child-elements', P(R('<w:t>a<w:foo>zz</w:foo>b</w:t><w:t><![CDATA[c<d]]></w:t><w:t>e<!-- c -->f</w:t>')));
  add('text-empty-runs', P(R('') + R('<w:t/>') + R('<w:t></w:t><w:tab/>') + R('<w:rPr><w:b/></w:rPr>')));
  add('text-run-split-by-objects', P(R(T('a') + '<w:sym w:char="41"/>' + T('b') + '<w:ptab w:alignment="right" w:relativeTo="margin" w:leader="dot"/>' + T('c') + '<w:footnoteRef/>' + T('d') + '<w:pgNum/>' + T('e'))));
  add('ptab-variants', P(R('<w:ptab/>') + R('<w:ptab w:alignment="center"/>') + R('<w:ptab w:relativeTo="indent" w:leader="none"/>') + R('<w:ptab w:alignment="" w:relativeTo="" w:leader=""/>')));
  add('ruby', P(R('<w:ruby><w:rubyPr/><w:rt><w:r><w:t>furigana</w:t></w:r></w:rt><w:rubyBase><w:r><w:t>base</w:t></w:r><w:r><w:t>2</w:t></w:r></w:rubyBase></w:ruby>' + T('after')) + R('<w:ruby><w:rt><w:r><w:t>r</w:t></w:r></w:rt></w:ruby>') + R(T('before') + '<w:ruby><w:rubyBase>' + R(T('x') + '<w:ruby><w:rubyBase>' + R(T('nested')) + '</w:rubyBase></w:ruby>') + '</w:rubyBase></w:ruby>' + T('tail'))));
  add('ruby-with-fields-and-hyperlink', P('<w:hyperlink r:id="rId1">' + R('<w:ruby><w:rubyBase>' + R(FB) + R(IT(' X ')) + R(FS) + R(T('r')) + R(FE) + '</w:rubyBase></w:ruby>') + '</w:hyperlink>'));
  add('props-link-underline-color', P('<w:hyperlink r:id="rId1">' + R(T('a'), '<w:rPr><w:u w:val="none"/></w:rPr>') + R(T('b'), '<w:rPr><w:color w:val="auto"/></w:rPr>') + R(T('c'), '<w:rPr><w:color w:val="123456"/></w:rPr>') + R(T('d'), '<w:rPr><w:rFonts w:hAnsi="x"/><w:rPrChange w:author="a"><w:rPr><w:b/></w:rPr></w:rPrChange></w:rPr>') + '</w:hyperlink>'));
  add('props-with-all-run-extras', P('<w:commentRangeStart w:id="1"/><w:ins w:author="A"><w:hyperlink r:id="rId1" w:tooltip="t">' + R(FB) + R(IT(' X ')) + R(FS) + R(T('f') + '<w:sym w:char="F0B7"/><w:footnoteReference w:id="1"/>') + R(FE) + '</w:hyperlink></w:ins><w:commentRangeEnd w:id="1"/>'));
  add('non-bmp-and-refs', P(R(T('😀 &#x1F600; &#xD83D; &#xDE00; &#55357;&#56832; &#xD83D;&#xDE00; end'))) + P(R('<w:instrText>TOC &#xD83D;</w:instrText>')));
  add('lone-surrogate-in-field-instr', P(R(FB) + R(IT(' A ')) + R(FB) + R(IT(' B ')) + R(FS) + R('<w:sym w:char="D800"/>') + R(FE) + R(FS) + R(T('x')) + R(FE)));
  add('toc-instr-trim-and-find', P(R(FB) + R(IT('   TOC \\o   ')) + R(FS) + R(FE)) + P(R(IT('\t\n TOC  ')) ));
  add('toc-instr-in-textbox-like', P(R('<w:pict><v:shape xmlns:v="urn:schemas-microsoft-com:vml"><v:textbox><w:txbxContent><w:p><w:r><w:instrText> TOC </w:instrText></w:r></w:p></w:txbxContent></v:textbox></v:shape></w:pict>')));
  add('toc-result-runs-inToc', P(R(FB) + R(IT(' TOC ')) + R(FS) + R(T('a') + '<w:tab/>' + T('3')) + '<w:hyperlink w:anchor="x">' + R(T('h')) + '</w:hyperlink>' + R(FE) + R(T('out'))));
  add('toc-inside-instr-nested-then-result', P(R(FB) + R(IT(' IF ')) + R(FB) + R(IT(' TOC ')) + R(FS) + R(T('n')) + R(FE) + R(FS) + R(T('res')) + R(FE)));
  add('toc-result-then-nested-field-text', P(R(FB) + R(IT(' TOC ')) + R(FS) + R(FB) + R(IT(' PAGEREF ')) + R(FS) + R(T('2')) + R(FE) + R(FE)));
  add('paragraph-with-ppr-and-field-before-ppr', '<w:p>' + R(FB) + '<w:pPr><w:jc w:val="center"/></w:pPr>' + R(IT(' X ')) + R(FS) + R(T('t')) + '</w:p><w:p>' + R(FE) + '</w:p>');
  add('crossing-two-open-fields', P(R(FB) + R(IT(' A ')) + R(FS) + R(T('a1')) + R(FB) + R(IT(' B ')) + R(FS) + R(T('b1'))) + P(R(T('mid'))) + P(R(T('b2')) + R(FE) + R(T('a2')) + R(FE) + R(T('end'))));
  add('crossing-field-with-object-result', P(R(FB) + R(IT(' A ')) + R(FS) + R(T('x') + '<w:pgNum/>')) + P(R(FE)));
  add('crossing-field-bookmark-link', P('<w:hyperlink r:id="rId1">' + R(FB) + R(IT(' A ')) + R(FS) + R(T('x')) + '</w:hyperlink>') + P('<w:bookmarkStart w:id="1" w:name="n"/>' + R(T('y')) + '<w:hyperlink r:id="rId2">' + R(T('z')) + '</w:hyperlink>' + R(FE) + '<w:bookmarkEnd w:id="1"/>'));
  add('crossing-field-sdt-wrapper', P('<w:sdt><w:sdtPr/><w:sdtContent>' + R(FB) + R(IT(' A ')) + R(FS) + R(T('x')) + '</w:sdtContent></w:sdt>') + P('<w:sdt><w:sdtPr/><w:sdtContent>' + R(T('y') + FE) + '</w:sdtContent></w:sdt>' + '<w:sdt><w:sdtPr/><w:sdtContent>' + R(T('z')) + '</w:sdtContent></w:sdt>'));
  add('instr-phase-across-paragraphs', P(R(FB) + R(IT(' A '))) + P(R(IT(' B ')) + R(FS) + R(T('res')) + R(FE)));
  add('wrapper-then-comments-after-field', P(R(FB) + R(IT(' A ')) + R(FS) + R(T('x'))) + P('<w:commentRangeStart w:id="1"/>' + R(T('y')) + '<w:commentRangeEnd w:id="1"/>' + R(FE)));
  add('table-cell-paragraphs', '<w:tbl><w:tr><w:tc><w:p><w:r><w:t>cell</w:t></w:r></w:p></w:tc><w:tc>' + P(R(FB) + R(IT(' X ')) + R(FS)) + P(R(FE)) + '</w:tc></w:tr></w:tbl>' + P(R(T('after table'))));
  add('self-closing-and-empty-paragraphs', '<w:p/><w:p></w:p><w:p><w:pPr/></w:p><w:p><w:pPr></w:pPr><w:r/></w:p>');
  add('wrapper-self-closing-and-empty', P('<w:hyperlink r:id="rId1"/><w:sdt/><w:smartTag/><w:ins/><w:del/><w:fldSimple/><w:sdt><w:sdtContent/></w:sdt><w:customXml></w:customXml>' + R(T('x'))));
  add('wrapper-pr-children', P('<w:customXml w:element="e"><w:customXmlPr><w:attr/></w:customXmlPr>' + R(T('cx')) + '</w:customXml><w:smartTag><w:smartTagPr/>' + R(T('st')) + '</w:smartTag>'));
  add('ppr-in-middle', P(R(T('a')) + '<w:pPr><w:jc w:val="right"/></w:pPr>' + R(T('b'))));
  add('mc-alternate-content-no-objects', P(R('<mc:AlternateContent><mc:Choice Requires="wps"><w:r/></mc:Choice><mc:Fallback/></mc:AlternateContent>' + T('t'))));
  add('empty-drawing-like-runs', P(R('<w:drawing/>' + T('d')) + R('<w:pict/>' + T('p')) + R('<w:object/>' + T('o')) + R('<mc:AlternateContent/>' + T('m'))));
  add('empty-objects-in-field-instr', P(R(FB) + R('<w:drawing/>') + R(IT(' X ')) + R(FS) + R(T('r') + '<w:drawing/>') + R(FE)));
  // nesting chains (the XML depth limit is 1,000)
  const chain = (open, close, n, leaf) => open.repeat(n) + leaf + close.repeat(n);
  add('deep-hyperlinks-200', P(chain('<w:hyperlink r:id="rId1">', '</w:hyperlink>', 200, R(T('deep')))));
  add('deep-ins-del-300', P(chain('<w:ins w:author="a"><w:del w:author="b">', '</w:del></w:ins>', 150, R(T('deep')))));
  add('deep-sdt-custom-60', P(chain('<w:sdt><w:sdtPr><w:tag w:val="wordnext:inline:a:1"/></w:sdtPr><w:sdtContent>', '</w:sdtContent></w:sdt>', 60, R(T('deep')))));
  add('deep-sdt-plain-300', P(chain('<w:sdt><w:sdtContent>', '</w:sdtContent></w:sdt>', 300, R(T('deep')))));
  add('deep-fldsimple-100', P(chain('<w:fldSimple w:instr="a">', '</w:fldSimple>', 100, R(T('deep')))));
  add('deep-smarttag-400', P(chain('<w:smartTag>', '</w:smartTag>', 400, R(T('deep')))));
  add('deep-ruby-80', P(chain('<w:r><w:ruby><w:rubyBase>', '</w:rubyBase></w:ruby></w:r>', 80, R(T('deep')))));
  add('deep-math-fractions-200', P('<m:oMathPara><m:oMath>' + chain('<m:f><m:num>', '</m:num><m:den><m:r><m:t>d</m:t></m:r></m:den></m:f>', 200, '<m:r><m:t>x</m:t></m:r>') + '</m:oMath></m:oMathPara>'));
  add('deep-math-generic-300', P('<m:oMath>' + chain('<m:box><m:e>', '</m:e></m:box>', 300, '<m:r><m:t>x</m:t></m:r>') + '</m:oMath>'));
  add('deep-math-delims-100', P('<m:oMathPara><m:oMath>' + chain('<m:d><m:e>', '</m:e><m:e><m:r><m:t>s</m:t></m:r></m:e></m:d>', 100, '<m:r><m:t>x</m:t></m:r>') + '</m:oMath></m:oMathPara>'));
  add('deep-wrapper-mix-250', P(chain('<w:hyperlink r:id="rId1"><w:ins w:author="a"><w:smartTag>', '</w:smartTag></w:ins></w:hyperlink>', 80, R(FB) + R(IT(' X ')) + R(FS) + R(T('deep')) + R(FE))));
  add('deep-instr-text-wrapped', P(chain('<w:smartTag>', '</w:smartTag>', 300, R(IT(' TOC ')))));
  // field nesting within the limit (64 open at once)
  add('fields-nested-60', P(R(FB + IT('x ')).repeat(60) + R(FS + T('v')) .repeat(1) + R(FE).repeat(60)));
  add('fields-nested-60-with-results', P(R(FB) .repeat(1) + R(IT(' o ')) + R(FS) + (R(FB) + R(IT(' i ')) + R(FS)).repeat(58) + R(T('core')) + R(FE).repeat(58) + R(FE)));
  add('many-fields-sequential', P(Array.from({ length: 300 }, (_, i) => R(FB) + R(IT(` F${i} `)) + R(FS) + R(T(String(i))) + R(FE)).join('')));
  add('too-deep', `<w:document ${NS}>${'<w:x>'.repeat(1001)}<w:p/>${'</w:x>'.repeat(1001)}</w:document>`);
  add('not-xml', '<w:document><w:p><w:r>');
  return c;
}

/** The whole corpus: `[key, text]`, hand-written cases first, then `n` seeded ones. */
export function generate(n = 700, seed = 0x4c0de4c) {
  const out = fixed().map(([k, t]) => [`quirk/${k}`, t]);
  const g = new PGen(seed);
  for (let i = 0; i < n; i++) {
    const count = 1 + g.rng.below(4);
    let inner = '';
    for (let k = 0; k < count; k++) inner += g.paragraph();
    out.push([`quirk/gen-${String(i).padStart(4, '0')}`, wrap(inner)]);
  }
  return out;
}

const escLine = (x) => x.replace(/\\/g, '\\\\').replace(/\n/g, '\\n').replace(/\r/g, '\\r').replace(/\t/g, '\\t');
const unescLine = (x) => x.replace(/\\(.)/gs, (_, c) => (c === 'n' ? '\n' : c === 'r' ? '\r' : c === 't' ? '\t' : c));

export const CORPUS_FILE = join(here, '..', 'tests', 'parity', 'paragraph.quirks.txt');

export function writeCorpus(cases = generate(), file = CORPUS_FILE) {
  const lines = ['# paragraph quirk corpus of the D1a reader (js/quirk-paragraph.mjs, seeded): key TAB text with \\, LF, CR, TAB escaped', ...cases.map(([k, t]) => `${k}\t${escLine(t)}`)];
  writeFileSync(file, lines.join('\n') + '\n');
  return cases.length;
}

/** The committed corpus (`[key, text]`). */
export function readCorpus(file = CORPUS_FILE) {
  return readFileSync(file, 'utf8').split('\n').filter((l) => l && !l.startsWith('#')).map((l) => { const i = l.indexOf('\t'); return [l.slice(0, i), unescLine(l.slice(i + 1))]; });
}

if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
  console.log(`${writeCorpus()} cases written to ${CORPUS_FILE}`);
}
