// Seeded documents with open fonts only (Liberation, Carlito, Caladea), for the CI layout parity
// (LQ21: the Word oracle references proprietary fonts in almost every document, so CI parity is
// generated documents only). The F0 generator covers the common flow: paragraphs and runs
// (family, size, bold, italic, underline, colour), alignment, indents, spacing and line rules,
// tabs, page breaks, simple tables, two-column sections, a header and a footer with a PAGE field.
// The structure-aware generator of the plan (§5 corpus: floats, textboxes and chains, notes,
// fields, lists, revisions, nested and merged tables) grows here frontier by frontier, with the
// same seed contract: `generateDocx(seed)` is a pure function of the seed.
import { Rng } from '../../../uo-testkit/js/testkit.mjs';

const W = 'http://schemas.openxmlformats.org/wordprocessingml/2006/main';
const R = 'http://schemas.openxmlformats.org/officeDocument/2006/relationships';
const FAMILIES = ['Liberation Serif', 'Liberation Sans', 'Liberation Mono', 'Carlito', 'Caladea'];
const WORDS = ('o motor de layout quebra linhas pagina tabelas notas campos e desenha cada glifo na posicao exata '
  + 'acao coracao informacao paralelepipedo hifenizacao the quick brown fox jumps over the lazy dog '
  + 'documento sessao edicao configuracao internacionalizacao responsabilidade').split(' ');
const ACCENTED = ['ação', 'coração', 'informação', 'parágrafo', 'página', 'câmera', 'português', 'útil', 'você'];

const esc = (s) => s.replace(/&/g, '&amp;').replace(/</g, '&lt;').replace(/>/g, '&gt;');

export function generateDocx(seed, { zipSync, strToU8 }) {
  const rng = new Rng(seed);
  const pick = (a) => a[rng.below(a.length)];
  const chance = (pct) => rng.below(100) < pct;
  const text = (n) => {
    const out = [];
    for (let i = 0; i < n; i++) out.push(chance(15) ? pick(ACCENTED) : pick(WORDS));
    return out.join(' ');
  };
  const run = (t) => {
    const rpr = [];
    if (chance(40)) { const f = pick(FAMILIES); rpr.push(`<w:rFonts w:ascii="${f}" w:hAnsi="${f}" w:cs="${f}"/>`); }
    if (chance(20)) rpr.push('<w:b/>');
    if (chance(20)) rpr.push('<w:i/>');
    if (chance(10)) rpr.push('<w:u w:val="single"/>');
    if (chance(10)) rpr.push(`<w:color w:val="${pick(['C00000', '0070C0', '00B050', '7030A0'])}"/>`);
    if (chance(35)) rpr.push(`<w:sz w:val="${pick([16, 18, 20, 22, 24, 28, 32])}"/>`);
    return `<w:r>${rpr.length ? `<w:rPr>${rpr.join('')}</w:rPr>` : ''}<w:t xml:space="preserve">${esc(t)}</w:t></w:r>`;
  };
  const para = (inner = null) => {
    const ppr = [];
    if (chance(30)) ppr.push(`<w:jc w:val="${pick(['left', 'center', 'right', 'both'])}"/>`);
    if (chance(25)) ppr.push(`<w:ind w:left="${pick([0, 360, 720])}" w:right="${pick([0, 360])}" w:${chance(50) ? 'firstLine' : 'hanging'}="${pick([0, 284, 567])}"/>`);
    if (chance(30)) ppr.push(`<w:spacing w:before="${pick([0, 120, 240])}" w:after="${pick([0, 120, 160, 240])}" w:line="${pick([240, 276, 360, 480])}" w:lineRule="${pick(['auto', 'exact', 'atLeast'])}"/>`);
    if (chance(10)) ppr.push(`<w:tabs><w:tab w:val="${pick(['left', 'center', 'right', 'decimal'])}" w:pos="${pick([1440, 2880, 4320])}"/></w:tabs>`);
    let runs = inner;
    if (runs === null) {
      const parts = [];
      const n = 1 + rng.below(4);
      for (let i = 0; i < n; i++) parts.push(run(text(1 + rng.below(25))));
      if (chance(8)) parts.push('<w:r><w:tab/></w:r>', run(text(2)));
      if (chance(4)) parts.push('<w:r><w:br w:type="page"/></w:r>');
      runs = parts.join('');
    }
    return `<w:p>${ppr.length ? `<w:pPr>${ppr.join('')}</w:pPr>` : ''}${runs}</w:p>`;
  };
  const table = () => {
    const cols = 2 + rng.below(3), rows = 1 + rng.below(4), w = Math.floor(9000 / cols);
    const grid = `<w:tblGrid>${'<w:gridCol w:w="' + w + '"/>'.repeat(cols)}</w:tblGrid>`;
    const border = (s) => `<w:${s} w:val="single" w:sz="4" w:space="0" w:color="000000"/>`;
    const tblPr = `<w:tblPr><w:tblW w:w="${w * cols}" w:type="dxa"/><w:tblBorders>${['top', 'left', 'bottom', 'right', 'insideH', 'insideV'].map(border).join('')}</w:tblBorders></w:tblPr>`;
    let body = '';
    for (let r = 0; r < rows; r++) {
      body += '<w:tr>';
      for (let c = 0; c < cols; c++) body += `<w:tc><w:tcPr><w:tcW w:w="${w}" w:type="dxa"/></w:tcPr>${para()}</w:tc>`;
      body += '</w:tr>';
    }
    return `<w:tbl>${tblPr}${grid}${body}</w:tbl>`;
  };
  const sectPr = (cols, last) => `<w:sectPr>${last ? '<w:headerReference w:type="default" r:id="rIdH"/><w:footerReference w:type="default" r:id="rIdF"/>' : ''}`
    + `${last ? '' : '<w:type w:val="continuous"/>'}<w:pgSz w:w="11906" w:h="16838"/>`
    + `<w:pgMar w:top="1417" w:right="1701" w:bottom="1417" w:left="1701" w:header="708" w:footer="708" w:gutter="0"/>`
    + `<w:cols w:num="${cols}" w:space="708"/></w:sectPr>`;

  const blocks = [];
  const n = 3 + rng.below(40);
  for (let i = 0; i < n; i++) blocks.push(chance(12) ? table() : para());
  if (chance(25)) {
    // a two-column section ended by a continuous break (column balancing), then one column
    const at = rng.below(blocks.length);
    blocks.splice(at, 0, `<w:p><w:pPr>${sectPr(1, false)}</w:pPr></w:p>`);
    blocks.push(`<w:p><w:pPr>${sectPr(2, false)}</w:pPr></w:p>`, para());
  }
  const document = `<?xml version="1.0" encoding="UTF-8" standalone="yes"?>\n<w:document xmlns:w="${W}" xmlns:r="${R}"><w:body>${blocks.join('')}${sectPr(1, true)}</w:body></w:document>`;
  const hf = (tag, inner) => `<?xml version="1.0" encoding="UTF-8" standalone="yes"?>\n<w:${tag} xmlns:w="${W}" xmlns:r="${R}">${inner}</w:${tag}>`;
  const header = hf('hdr', `<w:p><w:pPr><w:jc w:val="right"/></w:pPr>${run(text(3))}</w:p>`);
  const footer = hf('ftr', '<w:p><w:pPr><w:jc w:val="center"/></w:pPr><w:r><w:fldChar w:fldCharType="begin"/></w:r><w:r><w:instrText xml:space="preserve"> PAGE </w:instrText></w:r><w:r><w:fldChar w:fldCharType="separate"/></w:r><w:r><w:t>1</w:t></w:r><w:r><w:fldChar w:fldCharType="end"/></w:r></w:p>');
  const styles = `<?xml version="1.0" encoding="UTF-8" standalone="yes"?>\n<w:styles xmlns:w="${W}"><w:docDefaults><w:rPrDefault><w:rPr>`
    + `<w:rFonts w:ascii="Liberation Serif" w:hAnsi="Liberation Serif" w:eastAsia="Liberation Serif" w:cs="Liberation Serif"/><w:sz w:val="22"/><w:szCs w:val="22"/><w:lang w:val="pt-BR"/>`
    + `</w:rPr></w:rPrDefault><w:pPrDefault><w:pPr><w:spacing w:after="160" w:line="259" w:lineRule="auto"/></w:pPr></w:pPrDefault></w:docDefaults>`
    + `<w:style w:type="paragraph" w:default="1" w:styleId="Normal"><w:name w:val="Normal"/></w:style></w:styles>`;
  const settings = `<?xml version="1.0" encoding="UTF-8" standalone="yes"?>\n<w:settings xmlns:w="${W}"><w:defaultTabStop w:val="708"/>${chance(30) ? '<w:autoHyphenation/>' : ''}<w:compat><w:compatSetting w:name="compatibilityMode" w:uri="http://schemas.microsoft.com/office/word" w:val="15"/></w:compat></w:settings>`;
  const ct = '<?xml version="1.0" encoding="UTF-8" standalone="yes"?>\n<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types">'
    + '<Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/><Default Extension="xml" ContentType="application/xml"/>'
    + '<Override PartName="/word/document.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml"/>'
    + '<Override PartName="/word/styles.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.styles+xml"/>'
    + '<Override PartName="/word/settings.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.settings+xml"/>'
    + '<Override PartName="/word/header1.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.header+xml"/>'
    + '<Override PartName="/word/footer1.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.footer+xml"/></Types>';
  const rels = '<?xml version="1.0" encoding="UTF-8" standalone="yes"?>\n<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">'
    + '<Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="word/document.xml"/></Relationships>';
  const docRels = '<?xml version="1.0" encoding="UTF-8" standalone="yes"?>\n<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">'
    + '<Relationship Id="rIdS" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/styles" Target="styles.xml"/>'
    + '<Relationship Id="rIdT" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/settings" Target="settings.xml"/>'
    + '<Relationship Id="rIdH" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/header" Target="header1.xml"/>'
    + '<Relationship Id="rIdF" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/footer" Target="footer1.xml"/></Relationships>';
  return zipSync({
    '[Content_Types].xml': strToU8(ct), '_rels/.rels': strToU8(rels), 'word/document.xml': strToU8(document),
    'word/_rels/document.xml.rels': strToU8(docRels), 'word/styles.xml': strToU8(styles), 'word/settings.xml': strToU8(settings),
    'word/header1.xml': strToU8(header), 'word/footer1.xml': strToU8(footer),
  }, { mtime: new Date(Date.UTC(2026, 0, 1)) });
}
