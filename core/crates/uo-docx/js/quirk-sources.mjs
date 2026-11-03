// Seeded generator of docx packages that stress the source capture (frontier R6 of the D1a
// reader): the raw slices, wrappers and positions the lossless save depends on. Real documents
// rarely have the odd shapes (sdt around rows and cells, sdt in sdt, customXml, fields that join
// several children or cross paragraphs, self-closing paragraphs and cells, XML comments, CDATA,
// entity references, text between elements, multi-byte text before every position), so each
// generated package mixes them. `capture-sources.mjs` runs the frozen TS over these packages and
// writes them next to the capture (tests/parity/sources.inputs); the Rust replay reads that file,
// so both sides see the same text.
import { Rng } from '../../uo-testkit/js/testkit.mjs';

export const QUIRK_SEED = 0x5ec0de06;
export const QUIRK_COUNT = 400;

const NS = [
  'xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"',
  'xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships"',
  'xmlns:wp="http://schemas.openxmlformats.org/drawingml/2006/wordprocessingDrawing"',
  'xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main"',
  'xmlns:wps="http://schemas.microsoft.com/office/word/2010/wordprocessingShape"',
  'xmlns:mc="http://schemas.openxmlformats.org/markup-compatibility/2006"',
  'xmlns:v="urn:schemas-microsoft-com:vml"',
  'xmlns:w14="http://schemas.microsoft.com/office/word/2010/wordml"',
  'xmlns:m="http://schemas.openxmlformats.org/officeDocument/2006/math"',
].join(' ');
const PROLOG = '<?xml version="1.0" encoding="UTF-8" standalone="yes"?>\r\n';
const REL = 'http://schemas.openxmlformats.org/officeDocument/2006/relationships';

class Gen {
  constructor(seed) { this.rng = new Rng(seed); this.fieldDepth = 0; this.bm = 0; this.sdt = 0; this.paraId = 0; this.openField = 0; }
  below(n) { return this.rng.below(n); }
  pick(a) { return a[this.below(a.length)]; }
  chance(n) { return this.below(n) === 0; }
  many(max, f) { let s = ''; for (let i = this.below(max + 1); i > 0; i--) s += f(); return s; }

  /** Space, newline, comment, PI or nothing between two elements. */
  gap() {
    if (this.below(9) !== 0) return '';
    return this.pick([' ', '\n', '\r\n  ', '<!-- é -->', '\t', ' <!--c--> ']);
  }

  text() {
    return this.pick(['a', 'Olá mundo', 'x y', 'é', '😀 emoji', '&amp;&lt;&gt;', '&#xE9;', '&#x1F600;', '<![CDATA[<raw & text>]]>', ' lead', 'trail ', '日本語', 'ΑΒΓ', '']);
  }

  rPr() { return this.chance(2) ? '' : `<w:rPr>${this.many(3, () => this.pick(['<w:b/>', '<w:i w:val="0"/>', '<w:sz w:val="24"/>', '<w:color w:val="FF0000"/>', '<w:rFonts w:ascii="Aptos"/>']))}</w:rPr>`; }

  run(text = this.text()) {
    return `<w:r${this.chance(6) ? ' w:rsidR="00AB12"' : ''}>${this.rPr()}${this.gap()}<w:t${this.chance(3) ? ' xml:space="preserve"' : ''}>${text}</w:t></w:r>`;
  }

  /** One complex field in run children; `parts` splits it across several children (the join case). */
  fieldRuns(cross = false) {
    const instr = this.pick([' PAGE ', ' NUMPAGES \\* MERGEFORMAT ', ' TOC \\o "1-3" \\h ', ' REF bm1 \\h ', ' DATE \\@ "dd/MM/yyyy" ', ' HYPERLINK "https://x.test" ']);
    const r = (inner) => `<w:r>${inner}</w:r>`;
    let s = r('<w:fldChar w:fldCharType="begin"/>') + this.gap() + r(`<w:instrText xml:space="preserve">${instr}</w:instrText>`);
    s += r('<w:fldChar w:fldCharType="separate"/>');
    s += this.many(2, () => this.run());
    if (!cross) s += r('<w:fldChar w:fldCharType="end"/>');
    return s;
  }

  inline(depth = 0) {
    const k = this.below(depth > 2 ? 9 : 22);
    switch (k) {
      case 0: case 1: case 2: case 3: return this.run();
      case 4: return `<w:hyperlink r:id="rIdLink" w:tooltip="${this.pick(['tip', 'é', ''])}">${this.many(2, () => this.run())}</w:hyperlink>`;
      case 5: return `<w:hyperlink w:anchor="bm${this.below(3)}"${this.chance(3) ? ' w:history="1"' : ''}>${this.run()}</w:hyperlink>`;
      case 6: return `<w:bookmarkStart w:id="${++this.bm}" w:name="bm${this.bm}"/>${this.gap()}${this.run()}<w:bookmarkEnd w:id="${this.bm}"/>`;
      case 7: return '<w:proofErr w:type="spellStart"/>';
      case 8: return `<w:fldSimple w:instr=" ${this.pick(['PAGE', 'DATE', 'AUTHOR'])} ">${this.run()}</w:fldSimple>`;
      case 9: return this.fieldRuns(false);
      case 10: return `<w:sdt><w:sdtPr><w:alias w:val="c"/><w:id w:val="${++this.sdt}"/>${this.chance(3) ? '<w:text/>' : ''}</w:sdtPr><w:sdtContent>${this.many(2, () => this.inline(depth + 1)) || this.run()}</w:sdtContent></w:sdt>`;
      case 11: return `<w:sdt><w:sdtPr/><w:sdtContent/></w:sdt>`;
      case 12: return `<w:smartTag w:uri="u" w:element="e">${this.run()}</w:smartTag>`;
      case 13: return `<w:customXml w:uri="u" w:element="e"><w:customXmlPr><w:attr w:name="a" w:val="b"/></w:customXmlPr>${this.many(2, () => this.inline(depth + 1)) || this.run()}</w:customXml>`;
      case 14: return `<w:ins w:id="${++this.bm}" w:author="A" w:date="2024-01-01T00:00:00Z">${this.run()}</w:ins>`;
      case 15: return `<w:del w:id="${++this.bm}" w:author="B"><w:r><w:delText>${this.text()}</w:delText></w:r></w:del>`;
      case 16: return `<w:commentRangeStart w:id="${this.below(3)}"/>${this.run()}<w:commentRangeEnd w:id="${this.below(3)}"/><w:r><w:commentReference w:id="${this.below(3)}"/></w:r>`;
      case 17: return `<w:r><w:footnoteReference w:id="${2 + this.below(3)}"/></w:r>`;
      case 18: return this.drawing(depth);
      case 19: return this.pict(depth);
      case 20: return `<mc:AlternateContent><mc:Choice Requires="wps">${this.drawing(depth + 1)}</mc:Choice><mc:Fallback>${this.pict(depth + 1)}</mc:Fallback></mc:AlternateContent>`;
      default: return `<w:bdo w:val="rtl">${this.run()}</w:bdo><w:r><w:tab/><w:br/><w:noBreakHyphen/></w:r><m:oMath><m:r><m:t>x</m:t></m:r></m:oMath>`;
    }
  }

  drawing(depth) {
    const inner = this.chance(2) ? this.blocks(depth + 2, 2) : this.paragraph(depth + 2);
    return `<w:r><w:drawing><wp:inline distT="0" distB="0" distL="0" distR="0"><wp:extent cx="1905000" cy="952500"/><wp:docPr id="${++this.bm}" name="Box"/>`
      + `<a:graphic><a:graphicData uri="http://schemas.microsoft.com/office/word/2010/wordprocessingShape"><wps:wsp><wps:spPr><a:xfrm><a:off x="0" y="0"/><a:ext cx="1905000" cy="952500"/></a:xfrm><a:prstGeom prst="rect"><a:avLst/></a:prstGeom></wps:spPr>`
      + `<wps:txbx><w:txbxContent>${inner}</w:txbxContent></wps:txbx><wps:bodyPr/></wps:wsp></a:graphicData></a:graphic></wp:inline></w:drawing></w:r>`;
  }

  pict(depth) {
    if (this.chance(4)) {
      const box = (n) => `<v:shape id="${n}${++this.bm}" type="#_x0000_t202" style="position:absolute;width:90pt;height:40pt"><v:textbox><w:txbxContent>${this.paragraph(depth + 2)}</w:txbxContent></v:textbox></v:shape>`;
      return `<w:r><w:pict><v:group id="g${++this.bm}" style="position:absolute;width:200pt;height:100pt" coordsize="2000,1000">${box('a')}${box('b')}${this.chance(2) ? box('c') : ''}</v:group></w:pict></w:r>`;
    }
    const inner = this.chance(2) ? this.blocks(depth + 2, 2) : this.paragraph(depth + 2);
    return `<w:r><w:pict><v:shape id="s${++this.bm}" type="#_x0000_t202" style="position:absolute;width:100pt;height:50pt"><v:textbox><w:txbxContent>${inner}</w:txbxContent></v:textbox></v:shape></w:pict></w:r>`;
  }

  pPr(sect = false) {
    const kids = this.many(3, () => this.pick(['<w:jc w:val="center"/>', '<w:keepNext/>', '<w:pStyle w:val="Heading1"/>', '<w:spacing w:before="120"/>', '<w:rPr><w:b/></w:rPr>']));
    const s = sect ? '<w:sectPr><w:pgSz w:w="12240" w:h="15840"/></w:sectPr>' : '';
    return kids || s ? `<w:pPr>${kids}${s}</w:pPr>` : this.chance(5) ? '<w:pPr/>' : '';
  }

  paragraph(depth = 0, sect = false) {
    if (this.chance(12)) return this.pick(['<w:p/>', '<w:p />', '<w:p w14:paraId="0A0B0C0D"/>', '<w:p\n/>']);
    const id = this.chance(2) ? ` w14:paraId="${(0x10000000 + ++this.paraId).toString(16).toUpperCase()}"` : '';
    let body = this.pPr(sect);
    const n = this.below(5);
    for (let i = 0; i < n; i++) body += this.gap() + this.inline(depth);
    if (this.openField > 0 && this.chance(2)) { body += '<w:r><w:fldChar w:fldCharType="end"/></w:r>'; this.openField--; }
    else if (this.openField === 0 && this.chance(10)) { body += this.fieldRuns(true); this.openField++; }
    return `<w:p${id}>${body}</w:p>`;
  }

  cellContent(depth) {
    const k = this.below(8);
    if (k === 0) return this.blockSdt(depth + 1);
    if (k === 1 && depth < 3) return this.table(depth + 1);
    if (k === 2) return `<w:bookmarkStart w:id="${++this.bm}" w:name="cb${this.bm}"/>${this.paragraph(depth)}<w:bookmarkEnd w:id="${this.bm}"/>`;
    return this.paragraph(depth) + this.gap();
  }

  table(depth = 0) {
    let rows = '';
    const nrows = 1 + this.below(3);
    for (let i = 0; i < nrows; i++) {
      let cells = '';
      const ncells = 1 + this.below(3);
      for (let j = 0; j < ncells; j++) {
        let tc;
        if (this.chance(14)) tc = '<w:tc/>';
        else {
          const inner = this.many(2, () => this.cellContent(depth)) || this.paragraph(depth);
          tc = `<w:tc>${this.chance(2) ? '<w:tcPr><w:tcW w:w="2000" w:type="dxa"/></w:tcPr>' : ''}${inner}</w:tc>`;
        }
        if (this.chance(10)) tc = `<w:sdt><w:sdtPr><w:id w:val="${++this.sdt}"/></w:sdtPr><w:sdtContent>${tc}</w:sdtContent></w:sdt>`;
        else if (this.chance(12)) tc = `<w:permStart w:id="${++this.bm}" w:edGrp="everyone"/>${tc}<w:permEnd w:id="${this.bm}"/>`;
        cells += tc + this.gap();
      }
      let tr = `<w:tr${this.chance(3) ? ' w14:paraId="1A2B3C4D"' : ''}>${this.chance(2) ? '<w:trPr><w:cantSplit/></w:trPr>' : ''}${this.chance(8) ? '<w:tblPrEx><w:tblW w:w="0" w:type="auto"/></w:tblPrEx>' : ''}${cells}</w:tr>`;
      if (this.chance(8)) tr = `<w:sdt><w:sdtPr/><w:sdtContent>${tr}</w:sdtContent></w:sdt>`;
      else if (this.chance(10)) tr = `<w:customXml w:element="r"><w:customXmlPr/>${tr}</w:customXml>`;
      else if (this.chance(10)) tr = `<w:bookmarkStart w:id="${++this.bm}" w:name="rb${this.bm}"/>${tr}<w:bookmarkEnd w:id="${this.bm}"/>`;
      rows += tr;
    }
    const tblAttr = this.chance(4) ? ' w14:paraId="0000AAAA"' : '';
    return `<w:tbl${tblAttr}><w:tblPr><w:tblW w:w="0" w:type="auto"/></w:tblPr><w:tblGrid><w:gridCol w:w="2000"/><w:gridCol w:w="2000"/></w:tblGrid>${rows}</w:tbl>`;
  }

  blockSdt(depth) {
    const gallery = this.pick(['<w:docPartObj><w:docPartGallery w:val="Cover Pages"/><w:docPartUnique/></w:docPartObj>', '<w:docPartObj><w:docPartGallery w:val="Table of Contents"/></w:docPartObj>', '<w:docPartObj/>', '<w:text/>', '']);
    const inner = depth > 3 ? this.paragraph(depth) : (this.blocks(depth + 1, 3) || this.paragraph(depth));
    if (this.chance(9)) return `<w:sdt><w:sdtPr>${gallery}</w:sdtPr><w:sdtContent/></w:sdt>`;
    if (this.chance(9)) return `<w:sdt><w:sdtPr>${gallery}</w:sdtPr></w:sdt>`;
    return `<w:sdt><w:sdtPr><w:id w:val="${++this.sdt}"/>${gallery}</w:sdtPr>${this.chance(5) ? '<w:sdtEndPr/>' : ''}<w:sdtContent>${inner}</w:sdtContent></w:sdt>`;
  }

  block(depth) {
    const k = this.below(depth > 2 ? 4 : 12);
    if (k <= 4) return this.paragraph(depth);
    if (k === 5 || k === 6) return this.table(depth);
    if (k === 7 || k === 8) return this.blockSdt(depth);
    if (k === 9) return `<w:customXml w:element="b"><w:customXmlPr><w:attr w:name="a" w:val="b"/></w:customXmlPr>${this.blocks(depth + 1, 2)}</w:customXml>`;
    if (k === 10) return `<w:bookmarkStart w:id="${++this.bm}" w:name="blk${this.bm}"/>`;
    return this.paragraph(depth, true);
  }

  blocks(depth, max) {
    let s = '';
    for (let i = 1 + this.below(max); i > 0; i--) s += this.gap() + this.block(depth) + this.gap();
    return s;
  }

  story(root, ns) {
    this.openField = 0;
    return `${PROLOG}<${root} ${ns}>${this.chance(4) ? '<!-- a -->' : ''}${this.blocks(0, 4)}</${root}>`;
  }

  documentXml() {
    this.openField = 0;
    const refs = this.chance(2) ? '<w:headerReference w:type="default" r:id="rIdH1"/><w:footerReference w:type="default" r:id="rIdF1"/>' : '';
    const end = this.chance(8) ? '' : `<w:sectPr>${refs}<w:pgSz w:w="11906" w:h="16838"/></w:sectPr>`;
    const bg = this.chance(10) ? '<w:background w:color="FFFF00"/>' : '';
    const body = this.blocks(0, 7) + (this.openField > 0 ? '<w:p><w:r><w:fldChar w:fldCharType="end"/></w:r></w:p>' : '');
    return `${PROLOG}<w:document ${NS}>${bg}<w:body>${body}${end}</w:body></w:document>`;
  }

  notesXml(kind) {
    this.openField = 0;
    const el = `w:${kind}`;
    let s = `<${el} w:type="separator" w:id="0"><w:p><w:r><w:separator/></w:r></w:p></${el}><${el} w:type="continuationSeparator" w:id="1"><w:p/></${el}>`;
    for (let id = 2; id < 5; id++) {
      if (this.chance(5)) continue;
      s += `<${el} w:id="${id}">${this.chance(3) ? this.blocks(1, 2) : this.paragraph(1)}</${el}>${this.gap()}`;
    }
    return `${PROLOG}<w:${kind}s ${NS}>${s}</w:${kind}s>`;
  }
}

/** The generated packages: `{ key, parts: [[path, text]] }`. */
export function quirkSources(seed = QUIRK_SEED, count = QUIRK_COUNT) {
  const out = [];
  for (let i = 0; i < count; i++) {
    const g = new Gen((seed + i * 0x9e3779b1) >>> 0);
    const parts = [];
    parts.push(['word/document.xml', g.documentXml()]);
    parts.push(['word/_rels/document.xml.rels',
      `${PROLOG}<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">`
      + `<Relationship Id="rIdLink" Type="${REL}/hyperlink" Target="https://example.test/a?b=1&amp;c=2" TargetMode="External"/>`
      + `<Relationship Id="rIdH1" Type="${REL}/header" Target="header1.xml"/><Relationship Id="rIdF1" Type="${REL}/footer" Target="footer1.xml"/></Relationships>`]);
    if (g.chance(2)) parts.push(['word/header1.xml', g.story('w:hdr', NS)]);
    if (g.chance(2)) parts.push(['word/footer1.xml', g.story('w:ftr', NS)]);
    if (g.chance(2)) parts.push(['word/footnotes.xml', g.notesXml('footnote')]);
    if (g.chance(3)) parts.push(['word/endnotes.xml', g.notesXml('endnote')]);
    if (g.chance(3)) {
      parts.push(['word/comments.xml', `${PROLOG}<w:comments ${NS}>${[0, 1, 2].filter(() => g.chance(2)).map((id) => `<w:comment w:id="${id}" w:author="Au"><w:p><w:r><w:t>c${id}</w:t></w:r></w:p></w:comment>`).join('')}</w:comments>`]);
    }
    out.push({ key: `quirk/sources/${seed.toString(16)}-${i}.docx`, parts });
  }
  return out;
}

/** The inputs file (one part per line, TAB separated; `\`, LF, CR and TAB escaped). */
export function inputsText(inputs) {
  const esc = (x) => x.replace(/\\/g, '\\\\').replace(/\n/g, '\\n').replace(/\r/g, '\\r').replace(/\t/g, '\\t');
  const lines = ['# uo-docx R6 synthetic source-capture packages (js/quirk-sources.mjs): `@input <key>`, then `<path>\\t<escaped text>` per part'];
  for (const { key, parts } of inputs) {
    lines.push(`@input ${key}`);
    for (const [path, data] of parts) lines.push(`${path}\t${esc(data)}`);
  }
  return lines.join('\n') + '\n';
}
