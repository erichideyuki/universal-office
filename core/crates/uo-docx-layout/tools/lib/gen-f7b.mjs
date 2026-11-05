// Seeded documents of the engine's tables and floats (frontier F7b, record "Stage 3 — F7b") with
// open fonts only: body paragraphs mixed with tables — borders, shading, widths (dxa/pct/auto),
// gridSpan and vMerge, nested tables, row heights (exact/atLeast), cantSplit, header rows,
// keepNext rows, cell margins, vertical alignment, long rows that split between pages, floating
// tables (tblpPr) — and floating objects: DrawingML shapes and textboxes (wps) anchored to the
// paragraph, column, margin or page with square / topAndBottom / none wrapping, behind/in front,
// inline shapes, and text frames (w:framePr). No header, footer, note or linked textbox (F7c and
// the chains are out of F7b's generator). `generateF7bDocx(seed)` is a pure function of the seed
// (the gen-docx.mjs contract).
import { Rng } from '../../../uo-testkit/js/testkit.mjs';

const W = 'http://schemas.openxmlformats.org/wordprocessingml/2006/main';
const R = 'http://schemas.openxmlformats.org/officeDocument/2006/relationships';
const WP = 'http://schemas.openxmlformats.org/drawingml/2006/wordprocessingDrawing';
const A = 'http://schemas.openxmlformats.org/drawingml/2006/main';
const WPS = 'http://schemas.microsoft.com/office/word/2010/wordprocessingShape';
const FAMILIES = ['Liberation Serif', 'Liberation Sans', 'Liberation Mono', 'Carlito', 'Caladea'];
const WORDS = ('o motor de layout quebra linhas pagina colunas tabelas celulas bordas e desenha cada objeto '
  + 'flutuante na posicao exata acao coracao informacao the quick brown fox jumps over the lazy dog').split(' ');

const esc = (s) => s.replace(/&/g, '&amp;').replace(/</g, '&lt;').replace(/>/g, '&gt;');

export function generateF7bDocx(seed, { zipSync, strToU8 }) {
  const rng = new Rng(seed);
  const pick = (a) => a[rng.below(a.length)];
  const chance = (pct) => rng.below(100) < pct;
  const text = (n) => {
    const out = [];
    for (let i = 0; i < n; i++) out.push(pick(WORDS));
    return out.join(' ');
  };
  const run = (t) => {
    const rpr = [];
    if (chance(25)) { const f = pick(FAMILIES); rpr.push(`<w:rFonts w:ascii="${f}" w:hAnsi="${f}" w:cs="${f}"/>`); }
    if (chance(12)) rpr.push('<w:b/>');
    if (chance(20)) rpr.push(`<w:sz w:val="${pick([16, 20, 22, 28])}"/>`);
    return `<w:r>${rpr.length ? `<w:rPr>${rpr.join('')}</w:rPr>` : ''}<w:t xml:space="preserve">${esc(t)}</w:t></w:r>`;
  };
  let docPr = 1;
  const emu = (tw) => Math.round(tw * 635);
  const posH = () => {
    const rel = pick(['column', 'margin', 'page', 'character']);
    return chance(40) ? `<wp:positionH relativeFrom="${rel}"><wp:align>${pick(['left', 'center', 'right'])}</wp:align></wp:positionH>`
      : `<wp:positionH relativeFrom="${rel}"><wp:posOffset>${emu(pick([0, 360, 1440, 3000]))}</wp:posOffset></wp:positionH>`;
  };
  const posV = () => {
    const rel = pick(['paragraph', 'paragraph', 'line', 'margin', 'page']);
    return chance(25) && rel !== 'paragraph' && rel !== 'line' ? `<wp:positionV relativeFrom="${rel}"><wp:align>${pick(['top', 'center', 'bottom'])}</wp:align></wp:positionV>`
      : `<wp:positionV relativeFrom="${rel}"><wp:posOffset>${emu(pick([0, 120, 600, -240]))}</wp:posOffset></wp:positionV>`;
  };
  const wrap = () => pick(['<wp:wrapSquare wrapText="bothSides"/>', '<wp:wrapSquare wrapText="largest"/>', '<wp:wrapTopAndBottom/>', '<wp:wrapNone/>', '<wp:wrapSquare wrapText="left"/>']);
  const spPr = (w, h) => `<wps:spPr><a:xfrm><a:off x="0" y="0"/><a:ext cx="${emu(w)}" cy="${emu(h)}"/></a:xfrm><a:prstGeom prst="${pick(['rect', 'roundRect', 'ellipse'])}"><a:avLst/></a:prstGeom>`
    + `${chance(70) ? `<a:solidFill><a:srgbClr val="${pick(['4472C4', 'ED7D31', 'A5A5A5', 'FFC000'])}"/></a:solidFill>` : '<a:noFill/>'}`
    + `${chance(60) ? `<a:ln w="${pick([6350, 12700, 25400])}"><a:solidFill><a:srgbClr val="000000"/></a:solidFill></a:ln>` : ''}</wps:spPr>`;
  // a floating DrawingML shape or textbox anchored in the run's paragraph
  // (in a cell: small and relative to the paragraph — a page-relative float makes the cell, and its
  // row, taller than the page; the TS then emits the row's empty parts on 500 pages)
  const anchored = (inCell) => {
    const w = inCell ? pick([720, 1080]) : pick([1440, 2160, 3000]), h = inCell ? pick([360, 720]) : pick([720, 1080, 1800]);
    const pos = inCell ? `<wp:positionH relativeFrom="column"><wp:posOffset>${emu(pick([0, 360]))}</wp:posOffset></wp:positionH><wp:positionV relativeFrom="paragraph"><wp:posOffset>${emu(pick([0, 120]))}</wp:posOffset></wp:positionV>` : `${posH()}${posV()}`;
    const wr = inCell ? pick(['<wp:wrapSquare wrapText="bothSides"/>', '<wp:wrapNone/>']) : wrap();
    const id = docPr++;
    const tb = chance(45);
    const body = tb
      ? `<wps:txbx><w:txbxContent><w:p>${run(text(3 + rng.below(12)))}</w:p>${chance(30) ? `<w:p>${run(text(4))}</w:p>` : ''}</w:txbxContent></wps:txbx><wps:bodyPr rot="0" vert="horz" wrap="square" lIns="91440" tIns="45720" rIns="91440" bIns="45720" anchor="${pick(['t', 'ctr', 'b'])}">${chance(30) ? '<a:spAutoFit/>' : '<a:noAutofit/>'}</wps:bodyPr>`
      : '<wps:bodyPr/>';
    return `<w:r><w:drawing><wp:anchor distT="${pick([0, 45720])}" distB="${pick([0, 45720])}" distL="114300" distR="114300" simplePos="0" relativeHeight="${251658240 + id}" behindDoc="${chance(15) ? 1 : 0}" locked="0" layoutInCell="1" allowOverlap="1">`
      + `<wp:simplePos x="0" y="0"/>${pos}<wp:extent cx="${emu(w)}" cy="${emu(h)}"/><wp:effectExtent l="0" t="0" r="0" b="0"/>${wr}`
      + `<wp:docPr id="${id}" name="Shape ${id}"/><wp:cNvGraphicFramePr/><a:graphic><a:graphicData uri="${WPS}"><wps:wsp><wps:cNvSpPr${tb ? ' txBox="1"' : ''}/>${spPr(w, h)}${body}</wps:wsp></a:graphicData></a:graphic></wp:anchor></w:drawing></w:r>`;
  };
  const inlineShape = () => {
    const w = pick([360, 720]), h = pick([240, 360]);
    const id = docPr++;
    return `<w:r><w:drawing><wp:inline distT="0" distB="0" distL="0" distR="0"><wp:extent cx="${emu(w)}" cy="${emu(h)}"/><wp:effectExtent l="0" t="0" r="0" b="0"/>`
      + `<wp:docPr id="${id}" name="Inline ${id}"/><wp:cNvGraphicFramePr/><a:graphic><a:graphicData uri="${WPS}"><wps:wsp><wps:cNvSpPr/>${spPr(w, h)}<wps:bodyPr/></wps:wsp></a:graphicData></a:graphic></wp:inline></w:drawing></w:r>`;
  };
  const para = (opts = {}) => {
    const ppr = [];
    if (chance(10)) ppr.push('<w:keepNext/>');
    if (chance(15)) ppr.push(`<w:spacing w:before="${pick([0, 120, 240])}" w:after="${pick([0, 120, 240])}"/>`);
    if (chance(15)) ppr.push(`<w:jc w:val="${pick(['left', 'center', 'right', 'both'])}"/>`);
    if (opts.frame) ppr.push(opts.frame);
    const parts = [];
    const n = 1 + rng.below(3);
    for (let i = 0; i < n; i++) parts.push(run(text(1 + rng.below(opts.short ? 6 : 40))));
    if (opts.floats) {
      const k = 1 + rng.below(2);
      for (let i = 0; i < k; i++) parts.splice(rng.below(parts.length + 1), 0, anchored(!!opts.inCell));
    }
    if (chance(8)) parts.splice(rng.below(parts.length + 1), 0, inlineShape());
    return `<w:p>${ppr.length ? `<w:pPr>${ppr.join('')}</w:pPr>` : ''}${parts.join('')}</w:p>`;
  };
  const bdr = (side) => `<w:${side} w:val="${pick(['single', 'double', 'dotted', 'thick', 'nil'])}" w:sz="${pick([4, 8, 12])}" w:space="0" w:color="${pick(['000000', 'auto', '2F5496'])}"/>`;
  const table = (depth = 0) => {
    const cols = 1 + rng.below(depth ? 2 : 4);
    const widths = Array.from({ length: cols }, () => pick([1200, 1800, 2400, 3000]));
    const tpr = [];
    const pct = chance(15);
    tpr.push(pct ? `<w:tblW w:w="${pick([3000, 5000])}" w:type="pct"/>` : chance(70) ? `<w:tblW w:w="${widths.reduce((a, b) => a + b, 0)}" w:type="dxa"/>` : '<w:tblW w:w="0" w:type="auto"/>');
    if (chance(70)) tpr.push(`<w:tblBorders>${bdr('top')}${bdr('left')}${bdr('bottom')}${bdr('right')}${bdr('insideH')}${bdr('insideV')}</w:tblBorders>`);
    if (chance(20)) tpr.push(`<w:jc w:val="${pick(['center', 'right'])}"/>`);
    if (chance(15)) tpr.push('<w:tblLayout w:type="fixed"/>');
    if (chance(15)) tpr.push(`<w:tblCellMar><w:left w:w="${pick([0, 108, 200])}" w:type="dxa"/><w:right w:w="${pick([0, 108, 200])}" w:type="dxa"/></w:tblCellMar>`);
    if (!depth && chance(10)) tpr.push(`<w:tblpPr w:leftFromText="${pick([0, 180])}" w:rightFromText="180" w:vertAnchor="${pick(['text', 'margin', 'page'])}" w:horzAnchor="${pick(['text', 'margin', 'page'])}" w:tblpX="${pick([0, 1440, 2880])}" w:tblpY="${pick([0, 240, 1440])}"/>`);
    const nRows = 1 + rng.below(depth ? 3 : 8);
    // header rows only in tables whose rows all split: a row that cannot split and does not fit under
    // the repeated headers makes the TS repeat them on 500 pages (the guard of placeTable)
    const hdr = !depth && nRows > 2 && chance(25);
    const rows = [];
    let vm = Array.from({ length: cols }, () => false);
    for (let r = 0; r < nRows; r++) {
      const trpr = [];
      // one tall row that splits between pages (never cantSplit: a cantSplit row taller than a page
      // repeats the header rows on 500 pages, the TS guard)
      const big = r === 2 && !depth && chance(40);
      if (r === 0 && hdr) trpr.push('<w:tblHeader/>');
      // (a header row and a cantSplit row stay short: taller than a page, the TS repeats it on every
      // page up to its guard)
      const cant = (!hdr && !big && chance(10)) || (hdr && r === 0);
      if (cant && !(hdr && r === 0)) trpr.push('<w:cantSplit/>');
      if (chance(15)) trpr.push(`<w:trHeight w:val="${pick([300, 600, 1200])}"${!hdr && chance(40) ? ' w:hRule="exact"' : ''}/>`);
      const cells = [];
      for (let c = 0; c < cols; c++) {
        const span = c + 1 < cols && chance(12) ? 2 : 1;
        const tcpr = [`<w:tcW w:w="${widths.slice(c, c + span).reduce((a, b) => a + b, 0)}" w:type="dxa"/>`];
        if (span > 1) tcpr.push(`<w:gridSpan w:val="${span}"/>`);
        // (no vertical merge, nested table or float under repeated header rows: a row part that cannot
        // start below them repeats them up to the TS guard)
        const merge = !hdr && span === 1 && r > 0 && (vm[c] ? chance(60) : chance(10));
        if (merge) tcpr.push('<w:vMerge/>');
        else if (!hdr && span === 1 && r + 1 < nRows && chance(10)) { tcpr.push('<w:vMerge w:val="restart"/>'); vm[c] = true; }
        if (!merge && !tcpr.some((t) => t.includes('restart'))) vm[c] = false;
        if (chance(15)) tcpr.push(`<w:shd w:val="clear" w:color="auto" w:fill="${pick(['D9E2F3', 'FFF2CC', '404040'])}"/>`);
        if (chance(15)) tcpr.push(`<w:vAlign w:val="${pick(['center', 'bottom'])}"/>`);
        const content = [];
        const np = big ? 3 + rng.below(5) : 1 + rng.below(3);
        // (a cantSplit row stays short: taller than a page it repeats on 500 pages, the TS guard)
        for (let i = 0; i < np; i++) content.push(para({ short: cant || (!big && chance(60)), floats: !hdr && !cant && depth === 0 && chance(6), inCell: true }));
        if (!hdr && !cant && depth < 1 && chance(8)) { content.push(table(depth + 1)); content.push('<w:p/>'); }
        cells.push(`<w:tc><w:tcPr>${tcpr.join('')}</w:tcPr>${content.join('')}</w:tc>`);
        c += span - 1;
      }
      rows.push(`<w:tr>${trpr.length ? `<w:trPr>${trpr.join('')}</w:trPr>` : ''}${cells.join('')}</w:tr>`);
    }
    return `<w:tbl><w:tblPr>${tpr.join('')}</w:tblPr><w:tblGrid>${widths.map((w) => `<w:gridCol w:w="${w}"/>`).join('')}</w:tblGrid>${rows.join('')}</w:tbl>`;
  };
  const frame = () => `<w:framePr w:w="${pick([0, 2400, 3600])}"${chance(40) ? ` w:h="${pick([600, 1200])}" w:hRule="${pick(['atLeast', 'exact'])}"` : ''} w:hSpace="${pick([0, 180])}" w:vSpace="${pick([0, 120])}" w:wrap="${pick(['around', 'notBeside', 'none'])}" w:vAnchor="${pick(['text', 'margin', 'page'])}" w:hAnchor="${pick(['text', 'margin', 'page'])}" ${chance(50) ? `w:xAlign="${pick(['left', 'center', 'right'])}"` : `w:x="${pick([0, 1440])}"`} w:y="${pick([0, 240, 1440])}"/>`;

  const blocks = [];
  const n = 4 + rng.below(18);
  for (let i = 0; i < n; i++) {
    const r = rng.below(100);
    if (r < 35) blocks.push(table());
    else if (r < 50) blocks.push(para({ floats: true }));
    else if (r < 56) { const f = frame(); blocks.push(para({ frame: f, short: true })); if (chance(40)) blocks.push(para({ frame: f, short: true })); }
    else blocks.push(para());
  }
  blocks.push(para());
  const sectPr = `<w:sectPr><w:pgSz w:w="11906" w:h="16838"/><w:pgMar w:top="${pick([1417, 720])}" w:right="1701" w:bottom="1417" w:left="1701" w:header="708" w:footer="708" w:gutter="0"/><w:cols w:space="708"/></w:sectPr>`;
  const document = `<?xml version="1.0" encoding="UTF-8" standalone="yes"?>\n<w:document xmlns:w="${W}" xmlns:r="${R}" xmlns:wp="${WP}" xmlns:a="${A}" xmlns:wps="${WPS}"><w:body>${blocks.join('')}${sectPr}</w:body></w:document>`;
  const styles = `<?xml version="1.0" encoding="UTF-8" standalone="yes"?>\n<w:styles xmlns:w="${W}"><w:docDefaults><w:rPrDefault><w:rPr>`
    + `<w:rFonts w:ascii="${pick(FAMILIES)}" w:hAnsi="Liberation Serif" w:eastAsia="Liberation Serif" w:cs="Liberation Serif"/><w:sz w:val="22"/><w:szCs w:val="22"/><w:lang w:val="pt-BR"/>`
    + `</w:rPr></w:rPrDefault><w:pPrDefault><w:pPr><w:spacing w:after="160" w:line="259" w:lineRule="auto"/></w:pPr></w:pPrDefault></w:docDefaults>`
    + `<w:style w:type="paragraph" w:default="1" w:styleId="Normal"><w:name w:val="Normal"/></w:style>`
    + `<w:style w:type="table" w:default="1" w:styleId="TableNormal"><w:name w:val="Normal Table"/><w:tblPr><w:tblInd w:w="0" w:type="dxa"/><w:tblCellMar><w:top w:w="0" w:type="dxa"/><w:left w:w="108" w:type="dxa"/><w:bottom w:w="0" w:type="dxa"/><w:right w:w="108" w:type="dxa"/></w:tblCellMar></w:tblPr></w:style></w:styles>`;
  const settings = `<?xml version="1.0" encoding="UTF-8" standalone="yes"?>\n<w:settings xmlns:w="${W}"><w:defaultTabStop w:val="708"/><w:compat><w:compatSetting w:name="compatibilityMode" w:uri="http://schemas.microsoft.com/office/word" w:val="${chance(80) ? 15 : 14}"/></w:compat></w:settings>`;
  const ct = '<?xml version="1.0" encoding="UTF-8" standalone="yes"?>\n<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types">'
    + '<Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/><Default Extension="xml" ContentType="application/xml"/>'
    + '<Override PartName="/word/document.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml"/>'
    + '<Override PartName="/word/styles.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.styles+xml"/>'
    + '<Override PartName="/word/settings.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.settings+xml"/></Types>';
  const rels = '<?xml version="1.0" encoding="UTF-8" standalone="yes"?>\n<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">'
    + '<Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="word/document.xml"/></Relationships>';
  const docRels = '<?xml version="1.0" encoding="UTF-8" standalone="yes"?>\n<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">'
    + '<Relationship Id="rIdS" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/styles" Target="styles.xml"/>'
    + '<Relationship Id="rIdT" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/settings" Target="settings.xml"/></Relationships>';
  return zipSync({
    '[Content_Types].xml': strToU8(ct), '_rels/.rels': strToU8(rels), 'word/document.xml': strToU8(document),
    'word/_rels/document.xml.rels': strToU8(docRels), 'word/styles.xml': strToU8(styles), 'word/settings.xml': strToU8(settings),
  }, { mtime: new Date(Date.UTC(2026, 0, 1)) });
}
