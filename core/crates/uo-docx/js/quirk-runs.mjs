// Seeded generator of docx packages that stress the generating path of the D1b writer (frontier
// W1: runs, fields, revisions, hyperlinks, custom inlines, note marks, `ptab`, `sym`, math and the
// legacy paragraph properties of `paraXmlInner`). Real documents have few of these (tracked
// formatting changes, nested fields with cached results, `\h` fields, marker fields, custom inline
// tags, borders with a bar edge, hanging indents, automatic spacing), so each generated paragraph
// mixes them. `capture-runs.mjs` runs the frozen TS over these packages and writes them next to the
// capture (tests/parity/runs.inputs); the Rust replay reads that file, so both sides see the same text.
import { Rng } from '../../uo-testkit/js/testkit.mjs';

export const RUNS_SEED = 0x57a1b0c5;
export const RUNS_COUNT = 300;

const NS = [
  'xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"',
  'xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships"',
  'xmlns:m="http://schemas.openxmlformats.org/officeDocument/2006/math"',
  'xmlns:w14="http://schemas.microsoft.com/office/word/2010/wordml"',
].join(' ');
const PROLOG = '<?xml version="1.0" encoding="UTF-8" standalone="yes"?>\r\n';
const REL = 'http://schemas.openxmlformats.org/officeDocument/2006/relationships';
const DATES = ['2024-01-01T10:00:00Z', '2023-12-31T23:59:00Z', ''];

class RunsGen {
  constructor(seed) { this.rng = new Rng(seed); this.id = 100; this.bm = 0; }
  below(n) { return this.rng.below(n); }
  pick(a) { return a[this.below(a.length)]; }
  chance(n) { return this.below(n) === 0; }
  many(max, f) { let s = ''; for (let i = this.below(max + 1); i > 0; i--) s += f(); return s; }

  text() {
    return this.pick(['a', 'Olá mundo', ' lead', 'trail ', 'a\tb', 'x\ny', '  both  ', 'é', '😀 emoji', '&amp;&lt;&gt;', 'a&quot;b', '日本語', ' nb ', 'a b', '']);
  }

  rev(kind) {
    const date = this.pick(DATES);
    return `<w:${kind} w:id="${++this.id}" w:author="${this.pick(['Ana', 'Bob &amp; Co', 'é'])}"${date ? ` w:date="${date}"` : ''}/>`;
  }
  revAttrs() {
    const date = this.pick(DATES);
    return `w:id="${++this.id}" w:author="${this.pick(['Ana', 'Bob'])}"${date ? ` w:date="${date}"` : ''}`;
  }

  rPr(allowChange = true) {
    if (this.chance(3)) return '';
    const kids = this.many(4, () => this.pick(['<w:b/>', '<w:i w:val="0"/>', '<w:u w:val="single"/>', '<w:strike/>', '<w:sz w:val="28"/>', '<w:color w:val="00AA11"/>',
      '<w:rFonts w:ascii="Aptos"/>', '<w:highlight w:val="yellow"/>', '<w:vertAlign w:val="superscript"/>', '<w:rStyle w:val="Strong"/>', '<w:vanish/>', '<w:noProof/>',
      '<w:caps/>', '<w:spacing w:val="20"/>', '<w:lang w:val="pt-BR"/>']));
    const change = allowChange && this.chance(8) ? `<w:rPrChange ${this.revAttrs()}><w:rPr>${this.pick(['<w:b/>', '', '<w:i/>'])}</w:rPr></w:rPrChange>` : '';
    return `<w:rPr>${kids}${change}</w:rPr>`;
  }

  /** Text children of a run. */
  content(del = false) {
    const t = del ? 'w:delText' : 'w:t';
    return this.many(3, () => {
      switch (this.below(10)) {
        case 0: return '<w:tab/>';
        case 1: return this.pick(['<w:br/>', '<w:br w:type="page"/>', '<w:br w:type="column"/>', '<w:br w:clear="all"/>']);
        case 2: return this.pick(['<w:noBreakHyphen/>', '<w:softHyphen/>']);
        default: return `<${t}${this.chance(2) ? ' xml:space="preserve"' : ''}>${this.text()}</${t}>`;
      }
    }) || `<${t}>x</${t}>`;
  }

  run(del = false) { return `<w:r>${this.rPr()}${this.content(del)}</w:r>`; }

  /** A complex field; the instruction may carry nested fields (kept as `Run.fieldCode`). */
  complexField(depth = 0, del = false) {
    const instr = this.pick([' PAGE ', ' NUMPAGES \\* MERGEFORMAT ', ' REF bm1 \\h ', ' DATE \\@ "dd/MM/yyyy" ', ' HYPERLINK "https://x.test" ', ' TC "entry" \\f C ', ' XE "index" ',
      ' = 1 + 2 ', ' SEQ Figure \\* ARABIC ', ' IF { PAGE } > 1 "a" "b" ', ' TOC \\o "1-3" \\h ', ' PAGEREF _Toc1 \\h ']);
    const it = del ? 'w:delInstrText' : 'w:instrText';
    const r = (inner) => `<w:r>${this.rPr(false)}${inner}</w:r>`;
    let s = r('<w:fldChar w:fldCharType="begin"/>') + r(`<${it} xml:space="preserve">${instr}</${it}>`);
    if (depth < 2 && this.chance(3)) {
      s += r(`<${it} xml:space="preserve"> IF </${it}>`) + this.complexField(depth + 1, del) + r(`<${it} xml:space="preserve"> = 1 "a" </${it}>`);
    }
    if (this.chance(6)) return s + r('<w:fldChar w:fldCharType="end"/>');
    s += r('<w:fldChar w:fldCharType="separate"/>');
    s += this.many(2, () => this.run(del)) || this.run(del);
    return s + r('<w:fldChar w:fldCharType="end"/>');
  }

  inline(depth = 0) {
    switch (this.below(depth > 1 ? 14 : 30)) {
      case 0: case 1: case 2: case 3: case 4: return this.run();
      case 5: return `<w:hyperlink r:id="rIdLink"${this.chance(2) ? ` w:tooltip="${this.pick(['tip', 'é', 'a&amp;b'])}"` : ''}>${this.many(2, () => this.run()) || this.run()}</w:hyperlink>`;
      case 6: return `<w:hyperlink w:anchor="bm${this.below(3)}"${this.chance(3) ? ' w:history="1"' : ''}>${this.run()}</w:hyperlink>`;
      case 7: return `<w:bookmarkStart w:id="${++this.bm}" w:name="${this.pick(['bm', 'a&amp;b', 'é'])}${this.bm}"/>${this.run()}<w:bookmarkEnd w:id="${this.bm}"/>`;
      case 8: return `<w:fldSimple w:instr=" ${this.pick(['PAGE', 'DATE \\@ &quot;d&quot;', 'AUTHOR', 'REF x \\h', 'TC &quot;a&quot;', 'XE &quot;b&quot;', 'RD f', '= 2 * 3', 'NUMPAGES'])} ">${this.many(2, () => this.run()) || this.run()}</w:fldSimple>`;
      case 9: case 10: return this.complexField(0);
      case 11: return `<w:ins ${this.revAttrs()}>${this.many(2, () => this.run()) || this.run()}</w:ins>`;
      case 12: return `<w:del ${this.revAttrs()}>${this.many(2, () => this.run(true)) || this.run(true)}</w:del>`;
      case 13: return `<w:commentRangeStart w:id="${this.below(3)}"/>${this.run()}<w:commentRangeEnd w:id="${this.below(3)}"/><w:r><w:commentReference w:id="${this.below(3)}"/></w:r>`;
      case 14: return `<w:r>${this.rPr()}<w:footnoteReference w:id="${2 + this.below(3)}"${this.chance(3) ? ' w:customMarkFollows="1"' : ''}/></w:r>`;
      case 15: return `<w:r>${this.rPr()}<w:endnoteReference w:id="${2 + this.below(3)}"/></w:r>`;
      case 16: return `<w:r>${this.rPr()}<w:ptab w:relativeTo="${this.pick(['margin', 'indent'])}" w:alignment="${this.pick(['left', 'center', 'right'])}"${this.chance(2) ? ' w:leader="dot"' : ''}/></w:r>`;
      case 17: return `<w:r>${this.rPr()}<w:sym w:font="${this.pick(['Wingdings', 'Symbol', 'A&amp;B'])}" w:char="${this.pick(['F0A7', 'F028', '00E9', '263A'])}"/></w:r>`;
      case 18: return `<m:oMath><m:r><m:t>${this.pick(['x+y', 'a&lt;b', 'α'])}</m:t></m:r></m:oMath>`;
      case 19: return this.custom();
      case 20: return `<w:ins ${this.revAttrs()}><w:del ${this.revAttrs()}>${this.run(true)}</w:del></w:ins>`;
      case 21: return `<w:ins ${this.revAttrs()}>${this.complexField(1)}</w:ins>`;
      case 22: return `<w:del ${this.revAttrs()}>${this.complexField(1, true)}</w:del>`;
      case 23: return `<w:moveFrom ${this.revAttrs()}>${this.run(true)}</w:moveFrom><w:moveTo ${this.revAttrs()}>${this.run()}</w:moveTo>`;
      case 24: return `<w:smartTag w:uri="u" w:element="e">${this.run()}</w:smartTag>`;
      case 25: return `<w:r>${this.rPr()}<w:footnoteRef/></w:r>`;
      case 26: return `<w:r>${this.rPr()}<w:fldChar w:fldCharType="begin"/></w:r><w:r><w:instrText xml:space="preserve"> REF a </w:instrText></w:r><w:r><w:fldChar w:fldCharType="separate"/></w:r>`;
      case 27: return `<w:r><w:fldChar w:fldCharType="end"/></w:r>`;
      case 28: return `<w:hyperlink r:id="rIdLink">${this.run()}<w:bookmarkStart w:id="${++this.bm}" w:name="in${this.bm}"/>${this.run()}</w:hyperlink><w:bookmarkEnd w:id="${this.bm}"/>`;
      default: return `<w:r><w:rPr><w:rStyle w:val="Hyperlink"/></w:rPr><w:t>${this.text()}</w:t></w:r>`;
    }
  }

  custom() {
    const type = this.pick(['clickcompliance.document-tag', 'a.b-c_d', 'x']);
    const data = this.pick([{ k: 1 }, { name: 'é 😀', n: [1, 2.5, null, true] }, 'str', 7, null, { 'a b': "it's (ok)!*~", z: {} }]);
    const tag = `wordnext:inline:${type}:${encodeURIComponent(JSON.stringify(data))}`;
    return `<w:sdt><w:sdtPr><w:alias w:val="${this.pick(['fb', 'é &amp; ü', ''])}"/><w:tag w:val="${tag}"/><w:lock w:val="sdtLocked"/></w:sdtPr><w:sdtContent>${this.run()}</w:sdtContent></w:sdt>`;
  }

  edge(side, bar = false) {
    const col = this.pick(['auto', '112233', '']);
    return `<w:${side} w:val="${this.pick(['single', 'double', 'dotted', 'none'])}" w:sz="${this.pick([4, 8, 12])}"${bar ? '' : ` w:space="${this.pick([0, 1, 4])}"`}${col ? ` w:color="${col}"` : ''}/>`;
  }

  /** A `w:pPr` (`allowChange` = may carry a `w:pPrChange`). */
  pPr(allowChange = true) {
    const kids = [];
    if (this.chance(3)) kids.push(this.pick(['<w:pStyle w:val="Heading1"/>', '<w:pStyle w:val="a&amp;b"/>']));
    if (this.chance(4)) kids.push(this.pick(['<w:keepNext/>', '<w:keepNext w:val="0"/>']));
    if (this.chance(4)) kids.push(this.pick(['<w:keepLines/>', '<w:keepLines w:val="false"/>']));
    if (this.chance(5)) kids.push(this.pick(['<w:pageBreakBefore/>', '<w:pageBreakBefore w:val="0"/>']));
    if (this.chance(5)) kids.push(this.pick(['<w:widowControl/>', '<w:widowControl w:val="0"/>']));
    if (this.chance(4)) kids.push(`<w:numPr><w:ilvl w:val="${this.below(3)}"/>${this.chance(3) ? '' : `<w:numId w:val="${this.pick([0, 1, 5, 1001])}"/>`}</w:numPr>`);
    if (this.chance(6)) kids.push(this.pick(['<w:suppressLineNumbers/>', '<w:suppressAutoHyphens/>', '<w:suppressAutoHyphens w:val="0"/>']));
    if (this.chance(4)) kids.push(`<w:pBdr>${['top', 'left', 'bottom', 'right', 'between'].filter(() => this.chance(2)).map((s) => this.edge(s)).join('')}${this.chance(3) ? this.edge('bar', true) : ''}</w:pBdr>`);
    if (this.chance(5)) kids.push(`<w:shd w:val="clear" w:color="auto" w:fill="${this.pick(['FFFF00', 'auto', 'ddd'])}"/>`);
    if (this.chance(4)) kids.push(`<w:tabs>${this.many(3, () => `<w:tab w:val="${this.pick(['left', 'center', 'right', 'decimal', 'clear'])}"${this.chance(2) ? ` w:leader="${this.pick(['dot', 'hyphen', 'none'])}"` : ''} w:pos="${this.pick([720, 1440, 4320, -10])}"/>`) || '<w:tab w:val="left" w:pos="100"/>'}</w:tabs>`);
    if (this.chance(3)) {
      kids.push(`<w:spacing${this.chance(2) ? ` w:before="${this.pick([0, 120, 240])}"` : ''}${this.chance(2) ? ` w:after="${this.pick([0, 60, 200])}"` : ''}${this.chance(2) ? ` w:line="${this.pick([240, 360, 480])}" w:lineRule="${this.pick(['auto', 'exact', 'atLeast'])}"` : ''}${this.chance(4) ? ` w:beforeAutospacing="${this.pick([0, 1])}"` : ''}${this.chance(4) ? ` w:afterAutospacing="${this.pick([0, 1])}"` : ''}/>`);
    }
    if (this.chance(3)) kids.push(`<w:ind${this.chance(2) ? ` w:left="${this.pick([0, 720, -360])}"` : ''}${this.chance(3) ? ` w:right="${this.pick([0, 360])}"` : ''}${this.pick(['', ` w:firstLine="${this.pick([0, 360])}"`, ` w:hanging="${this.pick([360, 720])}"`])}/>`);
    if (this.chance(3)) kids.push(`<w:jc w:val="${this.pick(['left', 'center', 'right', 'both', 'start', 'end'])}"/>`);
    if (this.chance(10)) kids.push('<w:textboxTightWrap w:val="allLines"/>');
    if (this.chance(6)) kids.push(`<w:outlineLvl w:val="${this.below(4)}"/>`);
    if (this.chance(7)) kids.push(`<w:rPr>${this.chance(2) ? this.rev('ins') : ''}${this.chance(2) ? this.rev('del') : ''}${this.chance(3) ? '<w:b/>' : ''}</w:rPr>`);
    if (allowChange && this.chance(6)) kids.push(`<w:pPrChange ${this.revAttrs()}>${this.pPr(this.chance(4))}</w:pPrChange>`);
    return kids.length || this.chance(6) ? `<w:pPr>${kids.join('')}</w:pPr>` : '';
  }

  paragraph() {
    let body = this.pPr();
    for (let n = 1 + this.below(5); n > 0; n--) body += this.inline();
    return `<w:p>${body}</w:p>`;
  }

  documentXml() {
    let body = '';
    for (let n = 1 + this.below(5); n > 0; n--) body += this.paragraph();
    return `${PROLOG}<w:document ${NS}><w:body>${body}<w:sectPr><w:pgSz w:w="11906" w:h="16838"/></w:sectPr></w:body></w:document>`;
  }
}

/** The generated packages: `{ key, parts: [[path, text]] }`. */
export function quirkRuns(seed = RUNS_SEED, count = RUNS_COUNT) {
  const out = [];
  for (let i = 0; i < count; i++) {
    const g = new RunsGen((seed + i * 0x9e3779b1) >>> 0);
    out.push({
      key: `quirk/runs/${seed.toString(16)}-${i}.docx`,
      parts: [
        ['word/document.xml', g.documentXml()],
        ['word/_rels/document.xml.rels',
          `${PROLOG}<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">`
          + `<Relationship Id="rIdLink" Type="${REL}/hyperlink" Target="https://example.test/a?b=1&amp;c=2" TargetMode="External"/></Relationships>`],
      ],
    });
  }
  return out;
}

/** The inputs file (one part per line, TAB separated; `\`, LF, CR and TAB escaped). */
export function runsInputsText(inputs) {
  const esc = (x) => x.replace(/\\/g, '\\\\').replace(/\n/g, '\\n').replace(/\r/g, '\\r').replace(/\t/g, '\\t');
  const lines = ['# uo-docx D1b W1 synthetic packages (js/quirk-runs.mjs): `@input <key>`, then `<path>\\t<escaped text>` per part'];
  for (const { key, parts } of inputs) {
    lines.push(`@input ${key}`);
    for (const [path, data] of parts) lines.push(`${path}\t${esc(data)}`);
  }
  return lines.join('\n') + '\n';
}
