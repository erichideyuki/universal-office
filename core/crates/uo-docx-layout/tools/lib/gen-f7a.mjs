// Seeded documents of the engine core (frontier F7a, record "Stage 3 — F7a") with open fonts only:
// body paragraphs without headers, footers, tables, floats or notes — the content F7a lays out end
// to end — so the CI replay compares the P1 stream of the whole engine. Covered: sections (next
// page, continuous, even/odd page, next column) with one to three columns, the column separator,
// column and page breaks, keepNext / keepLines / widow control / page break before, contextual
// spacing with styles, automatic spacing, paragraph borders (groups with `between`) and shading,
// line numbering, the document grid, hidden paragraph marks, deleted paragraph marks (`mk-` keys
// are laid out with `revisions: all` by the capture; here the final view), PAGE / NUMPAGES /
// SECTIONPAGES / SECTION / DATE fields in the body, page-number formats and starts, multilevel
// lists. `generateF7aDocx(seed)` is a pure function of the seed (the gen-docx.mjs contract).
import { Rng } from '../../../uo-testkit/js/testkit.mjs';

const W = 'http://schemas.openxmlformats.org/wordprocessingml/2006/main';
const R = 'http://schemas.openxmlformats.org/officeDocument/2006/relationships';
const FAMILIES = ['Liberation Serif', 'Liberation Sans', 'Liberation Mono', 'Carlito', 'Caladea'];
const WORDS = ('o motor de layout quebra linhas pagina colunas secoes bordas e desenha cada glifo na posicao exata '
  + 'acao coracao informacao paralelepipedo hifenizacao the quick brown fox jumps over the lazy dog '
  + 'documento sessao edicao configuracao internacionalizacao responsabilidade').split(' ');
const ACCENTED = ['ação', 'coração', 'informação', 'parágrafo', 'página', 'câmera', 'português', 'útil', 'você'];

const esc = (s) => s.replace(/&/g, '&amp;').replace(/</g, '&lt;').replace(/>/g, '&gt;');

export function generateF7aDocx(seed, { zipSync, strToU8 }) {
  const rng = new Rng(seed);
  const pick = (a) => a[rng.below(a.length)];
  const chance = (pct) => rng.below(100) < pct;
  const text = (n) => {
    const out = [];
    for (let i = 0; i < n; i++) out.push(chance(15) ? pick(ACCENTED) : pick(WORDS));
    return out.join(' ');
  };
  const run = (t, extra = '') => {
    const rpr = [];
    if (chance(30)) { const f = pick(FAMILIES); rpr.push(`<w:rFonts w:ascii="${f}" w:hAnsi="${f}" w:cs="${f}"/>`); }
    if (chance(15)) rpr.push('<w:b/>');
    if (chance(15)) rpr.push('<w:i/>');
    if (chance(8)) rpr.push('<w:u w:val="single"/>');
    if (chance(25)) rpr.push(`<w:sz w:val="${pick([16, 20, 22, 24, 28, 36])}"/>`);
    if (extra) rpr.push(extra);
    return `<w:r>${rpr.length ? `<w:rPr>${rpr.join('')}</w:rPr>` : ''}<w:t xml:space="preserve">${esc(t)}</w:t></w:r>`;
  };
  const field = (instr) => `<w:fldSimple w:instr=" ${esc(instr).replace(/"/g, '&quot;')} "><w:r><w:t>1</w:t></w:r></w:fldSimple>`;
  const border = (side, v = pick(['single', 'double', 'dotted', 'thick'])) => `<w:${side} w:val="${v}" w:sz="${pick([4, 8, 12])}" w:space="${pick([0, 1, 4])}" w:color="${pick(['000000', 'auto', '2F5496'])}"/>`;
  const STYLES = ['Normal', 'Corpo', 'Lista', 'Titulo1'];
  let groupBdr = null;
  const para = (opts = {}) => {
    const ppr = [];
    const style = opts.style ?? (chance(35) ? pick(STYLES) : null);
    if (style && style !== 'Normal') ppr.push(`<w:pStyle w:val="${style}"/>`);
    if (chance(12)) ppr.push('<w:keepNext/>');
    if (chance(10)) ppr.push('<w:keepLines/>');
    if (chance(4)) ppr.push('<w:pageBreakBefore/>');
    if (chance(10)) ppr.push(`<w:widowControl w:val="${chance(50) ? '0' : '1'}"/>`);
    if (chance(8)) ppr.push(`<w:numPr><w:ilvl w:val="${rng.below(3)}"/><w:numId w:val="${1 + rng.below(2)}"/></w:numPr>`);
    // border groups: the same pBdr and ind on consecutive paragraphs
    if (groupBdr && chance(70)) ppr.push(groupBdr);
    else if (chance(10)) {
      groupBdr = `<w:pBdr>${border('top')}${border('left')}${border('bottom')}${border('right')}${chance(60) ? border('between', 'single') : ''}</w:pBdr>`;
      ppr.push(groupBdr);
    } else groupBdr = null;
    if (chance(8)) ppr.push(`<w:shd w:val="clear" w:color="auto" w:fill="${pick(['D9E2F3', 'FFF2CC', '404040'])}"/>`);
    if (chance(20)) ppr.push(`<w:spacing w:before="${pick([0, 120, 240])}" w:after="${pick([0, 120, 160, 240])}"${chance(20) ? ' w:beforeAutospacing="1" w:afterAutospacing="1"' : ''} w:line="${pick([240, 276, 360])}" w:lineRule="${pick(['auto', 'exact', 'atLeast'])}"/>`);
    if (chance(10)) ppr.push('<w:contextualSpacing/>');
    if (chance(15)) ppr.push(`<w:ind w:left="${pick([0, 360, 720])}" w:right="${pick([0, 360])}" w:${chance(50) ? 'firstLine' : 'hanging'}="${pick([0, 284])}"/>`);
    if (chance(20)) ppr.push(`<w:jc w:val="${pick(['left', 'center', 'right', 'both'])}"/>`);
    if (chance(5)) ppr.push('<w:suppressLineNumbers/>');
    if (chance(4)) ppr.push('<w:snapToGrid w:val="0"/>');
    if (opts.hiddenMark) ppr.push('<w:rPr><w:vanish/></w:rPr>');
    if (opts.delMark) ppr.push('<w:rPr><w:del w:id="9" w:author="Ana" w:date="2026-01-02T00:00:00Z"/></w:rPr>');
    if (opts.sect) ppr.push(opts.sect);
    const parts = [];
    if (!opts.empty) {
      const n = 1 + rng.below(4);
      for (let i = 0; i < n; i++) parts.push(run(text(1 + rng.below(opts.long ? 60 : 30))));
      if (chance(8)) parts.push(field(pick(['PAGE', 'NUMPAGES', 'SECTIONPAGES', 'SECTION', 'PAGE \\* ROMAN', 'DATE \\@ "dd/MM/yyyy"'])));
      if (chance(4)) parts.push('<w:r><w:br w:type="page"/></w:r>', run(text(3)));
      if (chance(4)) parts.push('<w:r><w:br w:type="column"/></w:r>', run(text(3)));
      if (chance(3)) parts.unshift('<w:r><w:br w:type="page"/></w:r>');
      if (chance(6)) parts.push('<w:r><w:tab/></w:r>', run(text(2)));
    }
    return `<w:p>${ppr.length ? `<w:pPr>${ppr.join('')}</w:pPr>` : ''}${parts.join('')}</w:p>`;
  };
  const pgNum = () => (chance(20) ? `<w:pgNumType${chance(50) ? ` w:start="${rng.below(5)}"` : ''}${chance(40) ? ` w:fmt="${pick(['lowerRoman', 'upperLetter', 'decimal'])}"` : ''}/>` : '');
  const sectPr = (type, cols) => `<w:sectPr>${type ? `<w:type w:val="${type}"/>` : ''}`
    + `<w:pgSz w:w="${chance(85) ? 11906 : 16838}" w:h="${chance(85) ? 16838 : 11906}"/>`
    + `<w:pgMar w:top="${pick([1417, 720, 1701])}" w:right="1701" w:bottom="${pick([1417, 1134])}" w:left="${pick([1701, 1134])}" w:header="708" w:footer="708" w:gutter="0"/>`
    + `${pgNum()}<w:cols w:num="${cols}" w:space="${pick([708, 425, 1134])}"${cols > 1 && chance(40) ? ' w:sep="1"' : ''}/>`
    + `${chance(12) ? `<w:lnNumType w:countBy="${pick([1, 2, 5])}"${chance(50) ? ` w:restart="${pick(['continuous', 'newSection', 'newPage'])}"` : ''}${chance(30) ? ' w:distance="567"' : ''}/>` : ''}`
    + `${chance(8) ? `<w:docGrid w:type="lines" w:linePitch="${pick([360, 312])}"/>` : ''}</w:sectPr>`;

  const blocks = [];
  const nSect = 1 + rng.below(4);
  for (let s = 0; s < nSect; s++) {
    const n = 2 + rng.below(25);
    for (let i = 0; i < n; i++) {
      const r = rng.below(100);
      blocks.push(para({ hiddenMark: r < 4, delMark: r >= 4 && r < 7, empty: r >= 7 && r < 10, long: r >= 10 && r < 25 }));
    }
    if (s < nSect - 1) {
      const type = pick(['continuous', 'continuous', 'nextPage', 'evenPage', 'oddPage', 'nextColumn']);
      // the paragraph that carries the break: empty sometimes (oracle colbreak)
      blocks.push(para({ sect: sectPr(type, 1 + rng.below(3)), empty: chance(40) }));
    }
  }
  const lastCols = 1 + rng.below(3);
  const document = `<?xml version="1.0" encoding="UTF-8" standalone="yes"?>\n<w:document xmlns:w="${W}" xmlns:r="${R}"><w:body>${blocks.join('')}${sectPr(chance(80) ? null : 'continuous', lastCols)}</w:body></w:document>`;
  const styles = `<?xml version="1.0" encoding="UTF-8" standalone="yes"?>\n<w:styles xmlns:w="${W}"><w:docDefaults><w:rPrDefault><w:rPr>`
    + `<w:rFonts w:ascii="${pick(FAMILIES)}" w:hAnsi="Liberation Serif" w:eastAsia="Liberation Serif" w:cs="Liberation Serif"/><w:sz w:val="22"/><w:szCs w:val="22"/><w:lang w:val="pt-BR"/>`
    + `</w:rPr></w:rPrDefault><w:pPrDefault><w:pPr><w:spacing w:after="160" w:line="259" w:lineRule="auto"/></w:pPr></w:pPrDefault></w:docDefaults>`
    + `<w:style w:type="paragraph" w:default="1" w:styleId="Normal"><w:name w:val="Normal"/></w:style>`
    + `<w:style w:type="paragraph" w:styleId="Corpo"><w:name w:val="Corpo"/><w:basedOn w:val="Normal"/><w:pPr><w:spacing w:before="120" w:after="120"/><w:contextualSpacing/></w:pPr></w:style>`
    + `<w:style w:type="paragraph" w:styleId="Lista"><w:name w:val="List Paragraph"/><w:basedOn w:val="Normal"/><w:pPr><w:spacing w:before="100" w:beforeAutospacing="1" w:after="100" w:afterAutospacing="1"/><w:ind w:left="720"/></w:pPr></w:style>`
    + `<w:style w:type="paragraph" w:styleId="Titulo1"><w:name w:val="heading 1"/><w:basedOn w:val="Normal"/><w:next w:val="Normal"/><w:pPr><w:keepNext/><w:keepLines/><w:spacing w:before="240" w:after="0"/><w:outlineLvl w:val="0"/></w:pPr><w:rPr><w:sz w:val="32"/></w:rPr></w:style>`
    + `<w:style w:type="character" w:styleId="NmeroDeLinha"><w:name w:val="line number"/><w:rPr><w:color w:val="808080"/></w:rPr></w:style></w:styles>`;
  const lvl = (i, fmt, txt) => `<w:lvl w:ilvl="${i}"><w:start w:val="1"/><w:numFmt w:val="${fmt}"/><w:lvlText w:val="${txt}"/><w:lvlJc w:val="left"/><w:pPr><w:ind w:left="${720 * (i + 1)}" w:hanging="360"/></w:pPr></w:lvl>`;
  const numbering = `<?xml version="1.0" encoding="UTF-8" standalone="yes"?>\n<w:numbering xmlns:w="${W}">`
    + `<w:abstractNum w:abstractNumId="0"><w:multiLevelType w:val="multilevel"/>${lvl(0, 'decimal', '%1.')}${lvl(1, 'lowerLetter', '%2)')}${lvl(2, 'lowerRoman', '%3.')}</w:abstractNum>`
    + `<w:abstractNum w:abstractNumId="1"><w:multiLevelType w:val="hybridMultilevel"/>${lvl(0, 'bullet', '-')}${lvl(1, 'bullet', 'o')}${lvl(2, 'bullet', '*')}</w:abstractNum>`
    + `<w:num w:numId="1"><w:abstractNumId w:val="0"/></w:num><w:num w:numId="2"><w:abstractNumId w:val="1"/></w:num></w:numbering>`;
  const settings = `<?xml version="1.0" encoding="UTF-8" standalone="yes"?>\n<w:settings xmlns:w="${W}"><w:defaultTabStop w:val="708"/>${chance(30) ? '<w:autoHyphenation/>' : ''}${chance(20) ? '<w:evenAndOddHeaders/>' : ''}<w:compat>${chance(15) ? '<w:doNotUseHTMLParagraphAutoSpacing/>' : ''}<w:compatSetting w:name="compatibilityMode" w:uri="http://schemas.microsoft.com/office/word" w:val="${chance(80) ? 15 : 14}"/></w:compat></w:settings>`;
  const ct = '<?xml version="1.0" encoding="UTF-8" standalone="yes"?>\n<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types">'
    + '<Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/><Default Extension="xml" ContentType="application/xml"/>'
    + '<Override PartName="/word/document.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml"/>'
    + '<Override PartName="/word/styles.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.styles+xml"/>'
    + '<Override PartName="/word/numbering.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.numbering+xml"/>'
    + '<Override PartName="/word/settings.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.settings+xml"/></Types>';
  const rels = '<?xml version="1.0" encoding="UTF-8" standalone="yes"?>\n<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">'
    + '<Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="word/document.xml"/></Relationships>';
  const docRels = '<?xml version="1.0" encoding="UTF-8" standalone="yes"?>\n<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">'
    + '<Relationship Id="rIdS" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/styles" Target="styles.xml"/>'
    + '<Relationship Id="rIdN" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/numbering" Target="numbering.xml"/>'
    + '<Relationship Id="rIdT" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/settings" Target="settings.xml"/></Relationships>';
  return zipSync({
    '[Content_Types].xml': strToU8(ct), '_rels/.rels': strToU8(rels), 'word/document.xml': strToU8(document),
    'word/_rels/document.xml.rels': strToU8(docRels), 'word/styles.xml': strToU8(styles), 'word/numbering.xml': strToU8(numbering),
    'word/settings.xml': strToU8(settings),
  }, { mtime: new Date(Date.UTC(2026, 0, 1)) });
}
