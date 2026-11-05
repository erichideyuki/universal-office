// Seeded documents of linked textbox chains and endnote-flow tables (frontier F7d, record "Stage 3 —
// F7d") with open fonts only. Chains: a first box (`wps:txbx id="N"` with the content) and 1–4
// linked boxes (`wps:linkedTxbx id="N" seq="k"`), floating (page/margin/paragraph anchors,
// square/none wrapping), inline (`wp:inline`) or inside a drawing group, of different sizes and
// insets, in the body and in a header; the content: paragraphs of one to several runs (fonts,
// sizes, a PAGE field, a tab) that are cut between the boxes in the middle of a run or at a run
// boundary, first-line indents, numbered lists, spacing, contextual spacing, widow control on/off,
// keepNext, and tables (cantSplit rows, row heights, header rows) cut between rows; seq out of
// document order, links to a missing id, a linked box before its first box, a chain whose last box
// overflows. Endnotes: notes of several paragraphs with tables (some long enough to cross a page)
// and keepNext paragraphs right before them, at the section end or the document end.
// `generateF7dDocx(seed)` is a pure function of the seed (the gen-docx.mjs contract).
import { Rng } from '../../../uo-testkit/js/testkit.mjs';

const W = 'http://schemas.openxmlformats.org/wordprocessingml/2006/main';
const R = 'http://schemas.openxmlformats.org/officeDocument/2006/relationships';
const WP = 'http://schemas.openxmlformats.org/drawingml/2006/wordprocessingDrawing';
const A = 'http://schemas.openxmlformats.org/drawingml/2006/main';
const WPS = 'http://schemas.microsoft.com/office/word/2010/wordprocessingShape';
const WPG = 'http://schemas.microsoft.com/office/word/2010/wordprocessingGroup';
const REL = 'http://schemas.openxmlformats.org/officeDocument/2006/relationships';
const FAMILIES = ['Liberation Serif', 'Liberation Sans', 'Liberation Mono', 'Carlito', 'Caladea'];
const WORDS = ('o motor de layout quebra linhas pagina colunas tabelas celulas bordas caixas vinculadas e cada '
  + 'texto flui para a seguinte acao coracao informacao the quick brown fox jumps over the lazy dog').split(' ');

const esc = (s) => s.replace(/&/g, '&amp;').replace(/</g, '&lt;').replace(/>/g, '&gt;');

export function generateF7dDocx(seed, { zipSync, strToU8 }) {
  const rng = new Rng(seed);
  const pick = (a) => a[rng.below(a.length)];
  const chance = (pct) => rng.below(100) < pct;
  const text = (n) => {
    const out = [];
    for (let i = 0; i < n; i++) out.push(pick(WORDS));
    return out.join(' ');
  };
  const emu = (tw) => Math.round(tw * 635);
  const run = (t) => {
    const rpr = [];
    if (chance(25)) { const f = pick(FAMILIES); rpr.push(`<w:rFonts w:ascii="${f}" w:hAnsi="${f}" w:cs="${f}"/>`); }
    if (chance(12)) rpr.push('<w:b/>');
    if (chance(20)) rpr.push(`<w:sz w:val="${pick([16, 18, 20, 24, 28])}"/>`);
    return `<w:r>${rpr.length ? `<w:rPr>${rpr.join('')}</w:rPr>` : ''}<w:t xml:space="preserve">${esc(t)}</w:t></w:r>`;
  };
  const field = () => `<w:fldSimple w:instr=" PAGE "><w:r><w:t>1</w:t></w:r></w:fldSimple>`;

  // ── content of a chain's first box ──
  const boxPara = () => {
    const ppr = [];
    if (chance(15)) ppr.push('<w:keepNext/>');
    if (chance(20)) ppr.push('<w:widowControl w:val="0"/>');
    if (chance(18)) ppr.push(`<w:numPr><w:ilvl w:val="${rng.below(2)}"/><w:numId w:val="1"/></w:numPr>`);
    if (chance(25)) ppr.push(`<w:spacing w:before="${pick([0, 60, 120, 240])}" w:after="${pick([0, 60, 120, 240])}"${chance(30) ? ` w:line="${pick([240, 360, 480])}" w:lineRule="auto"` : ''}/>`);
    if (chance(20)) ppr.push(`<w:ind w:left="${pick([0, 180, 360])}"${chance(60) ? ` w:firstLine="${pick([284, 567, 720])}"` : ` w:hanging="${pick([180, 360])}"`}/>`);
    if (chance(10)) ppr.push('<w:contextualSpacing/>');
    if (chance(12)) ppr.push(`<w:jc w:val="${pick(['center', 'right', 'both'])}"/>`);
    const parts = [];
    const n = 1 + rng.below(4);
    for (let i = 0; i < n; i++) parts.push(run(`${text(1 + rng.below(chance(30) ? 45 : 14))} `));
    if (chance(10)) parts.splice(rng.below(parts.length + 1), 0, field());
    if (chance(8)) parts.splice(rng.below(parts.length + 1), 0, '<w:r><w:tab/></w:r>');
    return `<w:p>${ppr.length ? `<w:pPr>${ppr.join('')}</w:pPr>` : ''}${parts.join('')}</w:p>`;
  };
  const bdr = (side) => `<w:${side} w:val="${pick(['single', 'double', 'nil'])}" w:sz="${pick([4, 8])}" w:space="0" w:color="000000"/>`;
  const smallTable = (nRows0) => {
    const cols = 1 + rng.below(3);
    const widths = Array.from({ length: cols }, () => pick([600, 900, 1200]));
    const tpr = ['<w:tblW w:w="0" w:type="auto"/>'];
    if (chance(80)) tpr.push(`<w:tblBorders>${bdr('top')}${bdr('left')}${bdr('bottom')}${bdr('right')}${bdr('insideH')}${bdr('insideV')}</w:tblBorders>`);
    const nRows = nRows0 ?? 2 + rng.below(9);
    const hdr = nRows > 3 && chance(20);
    const rows = [];
    for (let r = 0; r < nRows; r++) {
      const trpr = [];
      if (r === 0 && hdr) trpr.push('<w:tblHeader/>');
      if (chance(12)) trpr.push('<w:cantSplit/>');
      if (chance(12)) trpr.push(`<w:trHeight w:val="${pick([300, 500])}"/>`);
      const cells = widths.map((w) => {
        const np = chance(25) ? 2 : 1;
        const ps = [];
        for (let i = 0; i < np; i++) ps.push(`<w:p><w:pPr><w:spacing w:after="0"/></w:pPr>${run(text(1 + rng.below(chance(20) ? 12 : 4)))}</w:p>`);
        return `<w:tc><w:tcPr><w:tcW w:w="${w}" w:type="dxa"/></w:tcPr>${ps.join('')}</w:tc>`;
      });
      rows.push(`<w:tr>${trpr.length ? `<w:trPr>${trpr.join('')}</w:trPr>` : ''}${cells.join('')}</w:tr>`);
    }
    return `<w:tbl><w:tblPr>${tpr.join('')}</w:tblPr><w:tblGrid>${widths.map((w) => `<w:gridCol w:w="${w}"/>`).join('')}</w:tblGrid>${rows.join('')}</w:tbl>`;
  };
  const chainContent = () => {
    const out = [];
    const n = 1 + rng.below(6);
    for (let i = 0; i < n; i++) {
      if (chance(18)) { out.push(smallTable()); if (chance(50)) out.push(boxPara()); } else out.push(boxPara());
    }
    if (!out[out.length - 1].startsWith('<w:p')) out.push('<w:p/>');
    return out.join('');
  };

  // ── boxes ──
  let docPr = 1;
  const bodyPr = () => {
    const ins = () => emu(pick([0, 72, 144, 288]));
    return `<wps:bodyPr rot="0" vert="horz" wrap="square" lIns="${chance(60) ? 91440 : ins()}" tIns="${chance(60) ? 45720 : ins()}" rIns="${chance(60) ? 91440 : ins()}" bIns="${chance(60) ? 45720 : ins()}" anchor="${pick(['t', 't', 'ctr', 'b'])}"><a:noAutofit/></wps:bodyPr>`;
  };
  const spPr = (w, h) => `<wps:spPr><a:xfrm><a:off x="0" y="0"/><a:ext cx="${emu(w)}" cy="${emu(h)}"/></a:xfrm><a:prstGeom prst="rect"><a:avLst/></a:prstGeom>`
    + `${chance(30) ? `<a:solidFill><a:srgbClr val="${pick(['FFF2CC', 'D9E2F3', '404040'])}"/></a:solidFill>` : '<a:noFill/>'}<a:ln w="6350"><a:solidFill><a:srgbClr val="000000"/></a:solidFill></a:ln></wps:spPr>`;
  /** the wps:wsp of a box: `story` = { id, content } of a first box, or { link, seq } */
  const wsp = (w, h, story) => {
    const inner = story.content !== undefined
      ? `<wps:txbx id="${story.id}"><w:txbxContent>${story.content}</w:txbxContent></wps:txbx>`
      : `<wps:linkedTxbx id="${story.link}" seq="${story.seq}"/>`;
    return `<wps:wsp><wps:cNvSpPr txBox="1"/>${spPr(w, h)}${inner}${bodyPr()}</wps:wsp>`;
  };
  const anchorPos = (hf) => {
    const relH = pick(['page', 'margin', 'column']), relV = hf ? pick(['page', 'margin']) : pick(['page', 'margin', 'paragraph', 'paragraph']);
    return `<wp:positionH relativeFrom="${relH}"><wp:posOffset>${emu(pick([0, 360, 1440, 3600, 5400]))}</wp:posOffset></wp:positionH>`
      + `<wp:positionV relativeFrom="${relV}"><wp:posOffset>${emu(relV === 'paragraph' ? pick([0, 120, 360]) : pick([720, 2880, 6000, 9000, 12000]))}</wp:posOffset></wp:positionV>`;
  };
  const floating = (story, hf = false) => {
    const w = pick([1440, 2160, 2880, 3600, 4320]), h = pick([360, 600, 900, 1440, 2160]);
    const id = docPr++;
    return `<w:r><w:drawing><wp:anchor distT="0" distB="0" distL="114300" distR="114300" simplePos="0" relativeHeight="${251658240 + id}" behindDoc="0" locked="0" layoutInCell="1" allowOverlap="1">`
      + `<wp:simplePos x="0" y="0"/>${anchorPos(hf)}<wp:extent cx="${emu(w)}" cy="${emu(h)}"/><wp:effectExtent l="0" t="0" r="0" b="0"/>${chance(70) ? '<wp:wrapNone/>' : '<wp:wrapSquare wrapText="bothSides"/>'}`
      + `<wp:docPr id="${id}" name="Caixa ${id}"/><wp:cNvGraphicFramePr/><a:graphic><a:graphicData uri="${WPS}">${wsp(w, h, story)}</a:graphicData></a:graphic></wp:anchor></w:drawing></w:r>`;
  };
  const inline = (story) => {
    const w = pick([1440, 2160, 2880]), h = pick([360, 600, 900]);
    const id = docPr++;
    return `<w:r><w:drawing><wp:inline distT="0" distB="0" distL="0" distR="0"><wp:extent cx="${emu(w)}" cy="${emu(h)}"/><wp:effectExtent l="0" t="0" r="0" b="0"/>`
      + `<wp:docPr id="${id}" name="Caixa ${id}"/><wp:cNvGraphicFramePr/><a:graphic><a:graphicData uri="${WPS}">${wsp(w, h, story)}</a:graphicData></a:graphic></wp:inline></w:drawing></w:r>`;
  };
  const group = (story) => {
    const w = pick([2160, 2880, 3600]), h = pick([600, 900, 1440]);
    const id = docPr++;
    const xfrm = `<a:xfrm><a:off x="0" y="0"/><a:ext cx="${emu(w)}" cy="${emu(h)}"/><a:chOff x="0" y="0"/><a:chExt cx="${emu(w)}" cy="${emu(h)}"/></a:xfrm>`;
    const shape = wsp(w, h, story);
    return `<w:r><w:drawing><wp:anchor distT="0" distB="0" distL="114300" distR="114300" simplePos="0" relativeHeight="${251658240 + id}" behindDoc="0" locked="0" layoutInCell="1" allowOverlap="1">`
      + `<wp:simplePos x="0" y="0"/>${anchorPos(false)}<wp:extent cx="${emu(w)}" cy="${emu(h)}"/><wp:effectExtent l="0" t="0" r="0" b="0"/><wp:wrapNone/>`
      + `<wp:docPr id="${id}" name="Grupo ${id}"/><wp:cNvGraphicFramePr/><a:graphic><a:graphicData uri="${WPG}"><wpg:wgp><wpg:cNvGrpSpPr/><wpg:grpSpPr>${xfrm}</wpg:grpSpPr>${shape}</wpg:wgp></a:graphicData></a:graphic></wp:anchor></w:drawing></w:r>`;
  };

  /** a chain: its boxes as runs, in document order (shuffled within limits) */
  let storyId = 1;
  const chain = (hf) => {
    const id = storyId++;
    const n = 1 + rng.below(4);
    const kind = () => (hf ? 'float' : chance(12) ? 'inline' : chance(8) ? 'group' : 'float');
    const make = (k, story) => (k === 'inline' ? inline(story) : k === 'group' ? group(story) : floating(story, hf));
    const boxes = [make(kind(), { id, content: chainContent() })];
    // seq 1..n, sometimes out of order or with a gap; sometimes a link to a missing id
    const seqs = Array.from({ length: n }, (_, i) => i + 1 + (chance(5) ? 3 : 0));
    if (chance(15)) seqs.reverse();
    for (const s of seqs) boxes.push(make(kind(), { link: chance(4) ? id + 50 : id, seq: s }));
    if (chance(10)) { const b = boxes.shift(); boxes.splice(1 + rng.below(boxes.length), 0, b); }
    return boxes;
  };

  // ── endnotes ──
  const endnotes = [];
  let noteId = 1;
  const notePara = (first, keep) => {
    const ppr = ['<w:pStyle w:val="Textodenotadefim"/>'];
    if (keep) ppr.push('<w:keepNext/>');
    if (chance(15)) ppr.push(`<w:spacing w:after="${pick([0, 120, 240])}"/>`);
    const parts = [];
    if (first) parts.push('<w:r><w:rPr><w:rStyle w:val="Refdenotadefim"/></w:rPr><w:endnoteRef/></w:r>');
    parts.push(run(` ${text(2 + rng.below(chance(20) ? 60 : 15))}`));
    return `<w:p><w:pPr>${ppr.join('')}</w:pPr>${parts.join('')}</w:p>`;
  };
  const endnoteRef = () => {
    const id = ++noteId;
    const blocks = [notePara(true, false)];
    const n = rng.below(4);
    for (let k = 0; k < n; k++) {
      if (chance(55)) {
        // a keepNext paragraph before the table (nextBlockLead in the endnote flow)
        if (chance(60)) blocks.push(notePara(false, true));
        blocks.push(smallTable(chance(15) ? 25 + rng.below(20) : undefined));
        if (chance(50)) blocks.push(notePara(false, false));
      } else blocks.push(notePara(false, chance(20)));
    }
    if (blocks[blocks.length - 1].startsWith('<w:tbl')) blocks.push('<w:p><w:pPr><w:pStyle w:val="Textodenotadefim"/></w:pPr></w:p>');
    endnotes.push(`<w:endnote w:id="${id}">${blocks.join('')}</w:endnote>`);
    return `<w:r><w:rPr><w:rStyle w:val="Refdenotadefim"/></w:rPr><w:endnoteReference w:id="${id}"/></w:r>`;
  };

  // ── body ──
  const para = (extra = []) => {
    const ppr = [];
    if (chance(10)) ppr.push('<w:keepNext/>');
    if (chance(15)) ppr.push(`<w:spacing w:before="${pick([0, 120, 240])}" w:after="${pick([0, 120, 240])}"/>`);
    const parts = [];
    const n = 1 + rng.below(3);
    for (let i = 0; i < n; i++) parts.push(run(`${text(1 + rng.below(30))} `));
    for (const x of extra) parts.splice(rng.below(parts.length + 1), 0, x);
    return `<w:p>${ppr.length ? `<w:pPr>${ppr.join('')}</w:pPr>` : ''}${parts.join('')}</w:p>`;
  };
  const withNotes = chance(60);
  const withChains = !withNotes || chance(60);
  const blocks = [];
  const pending = withChains ? chain(false) : [];
  for (let c = 1 + rng.below(2); withChains && c > 1; c--) pending.push(...chain(false));
  const nBlocks = 3 + rng.below(12);
  for (let i = 0; i < nBlocks; i++) {
    const extra = [];
    // the chain boxes spread over the first paragraphs
    const k = Math.min(pending.length, i === nBlocks - 1 ? pending.length : rng.below(3));
    for (let j = 0; j < k; j++) extra.push(pending.shift());
    if (withNotes && chance(40)) extra.push(endnoteRef());
    if (withNotes && chance(10)) blocks.push(smallTable());
    blocks.push(para(extra));
  }
  // a header with a chain (its own story ids, scanned apart)
  const hdr = withChains && chance(15);
  const headerXml = hdr ? `<?xml version="1.0" encoding="UTF-8" standalone="yes"?>\n<w:hdr xmlns:w="${W}" xmlns:r="${R}" xmlns:wp="${WP}" xmlns:a="${A}" xmlns:wps="${WPS}" xmlns:wpg="${WPG}">`
    + `<w:p>${run('cabecalho ')}${chain(true).join('')}</w:p></w:hdr>` : '';
  const endPos = withNotes && chance(40) ? '<w:endnotePr><w:pos w:val="sectEnd"/></w:endnotePr>' : '';
  const cols = chance(15) ? '<w:cols w:num="2" w:space="708"/>' : '<w:cols w:space="708"/>';
  const sectPr = `<w:sectPr>${hdr ? '<w:headerReference w:type="default" r:id="rIdH"/>' : ''}${endPos}<w:pgSz w:w="11906" w:h="16838"/><w:pgMar w:top="1417" w:right="1701" w:bottom="1417" w:left="1701" w:header="708" w:footer="708" w:gutter="0"/>${cols}</w:sectPr>`;
  const document = `<?xml version="1.0" encoding="UTF-8" standalone="yes"?>\n<w:document xmlns:w="${W}" xmlns:r="${R}" xmlns:wp="${WP}" xmlns:a="${A}" xmlns:wps="${WPS}" xmlns:wpg="${WPG}"><w:body>${blocks.join('')}${sectPr}</w:body></w:document>`;
  const sep = (kind) => `<w:${kind} w:type="separator" w:id="-1"><w:p><w:pPr><w:spacing w:after="0" w:line="240" w:lineRule="auto"/></w:pPr><w:r><w:separator/></w:r></w:p></w:${kind}>`
    + `<w:${kind} w:type="continuationSeparator" w:id="0"><w:p><w:pPr><w:spacing w:after="0" w:line="240" w:lineRule="auto"/></w:pPr><w:r><w:continuationSeparator/></w:r></w:p></w:${kind}>`;
  const parts = {};
  if (endnotes.length) parts['word/endnotes.xml'] = `<?xml version="1.0" encoding="UTF-8" standalone="yes"?>\n<w:endnotes xmlns:w="${W}" xmlns:r="${R}">${sep('endnote')}${endnotes.join('')}</w:endnotes>`;
  if (hdr) parts['word/header1.xml'] = headerXml;
  const styles = `<?xml version="1.0" encoding="UTF-8" standalone="yes"?>\n<w:styles xmlns:w="${W}"><w:docDefaults><w:rPrDefault><w:rPr>`
    + `<w:rFonts w:ascii="${pick(FAMILIES)}" w:hAnsi="Liberation Serif" w:eastAsia="Liberation Serif" w:cs="Liberation Serif"/><w:sz w:val="22"/><w:szCs w:val="22"/><w:lang w:val="pt-BR"/>`
    + `</w:rPr></w:rPrDefault><w:pPrDefault><w:pPr><w:spacing w:after="160" w:line="259" w:lineRule="auto"/></w:pPr></w:pPrDefault></w:docDefaults>`
    + `<w:style w:type="paragraph" w:default="1" w:styleId="Normal"><w:name w:val="Normal"/></w:style>`
    + `<w:style w:type="table" w:default="1" w:styleId="TableNormal"><w:name w:val="Normal Table"/><w:tblPr><w:tblInd w:w="0" w:type="dxa"/><w:tblCellMar><w:top w:w="0" w:type="dxa"/><w:left w:w="108" w:type="dxa"/><w:bottom w:w="0" w:type="dxa"/><w:right w:w="108" w:type="dxa"/></w:tblCellMar></w:tblPr></w:style>`
    + `<w:style w:type="paragraph" w:styleId="Textodenotadefim"><w:name w:val="endnote text"/><w:basedOn w:val="Normal"/><w:pPr><w:spacing w:after="0" w:line="240" w:lineRule="auto"/></w:pPr><w:rPr><w:sz w:val="20"/></w:rPr></w:style>`
    + `<w:style w:type="character" w:styleId="Refdenotadefim"><w:name w:val="endnote reference"/><w:rPr><w:vertAlign w:val="superscript"/></w:rPr></w:style></w:styles>`;
  const lvl = (i, fmt, txt) => `<w:lvl w:ilvl="${i}"><w:start w:val="1"/><w:numFmt w:val="${fmt}"/><w:lvlText w:val="${txt}"/><w:lvlJc w:val="left"/><w:pPr><w:ind w:left="${360 * (i + 1)}" w:hanging="360"/></w:pPr></w:lvl>`;
  const numbering = `<?xml version="1.0" encoding="UTF-8" standalone="yes"?>\n<w:numbering xmlns:w="${W}">`
    + `<w:abstractNum w:abstractNumId="0"><w:multiLevelType w:val="multilevel"/>${lvl(0, 'decimal', '%1.')}${lvl(1, 'lowerLetter', '%2)')}</w:abstractNum>`
    + '<w:num w:numId="1"><w:abstractNumId w:val="0"/></w:num></w:numbering>';
  const settings = `<?xml version="1.0" encoding="UTF-8" standalone="yes"?>\n<w:settings xmlns:w="${W}"><w:defaultTabStop w:val="708"/>`
    + (endnotes.length ? '<w:endnotePr><w:endnote w:id="-1"/><w:endnote w:id="0"/></w:endnotePr>' : '')
    + `<w:compat><w:compatSetting w:name="compatibilityMode" w:uri="http://schemas.microsoft.com/office/word" w:val="${chance(85) ? 15 : 14}"/></w:compat></w:settings>`;
  const over = (part, type) => `<Override PartName="/${part}" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.${type}+xml"/>`;
  const extra = Object.keys(parts).map((p) => over(p, /header/.test(p) ? 'header' : 'endnotes')).join('');
  const ct = '<?xml version="1.0" encoding="UTF-8" standalone="yes"?>\n<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types">'
    + '<Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/><Default Extension="xml" ContentType="application/xml"/>'
    + over('word/document.xml', 'document.main') + over('word/styles.xml', 'styles') + over('word/numbering.xml', 'numbering') + over('word/settings.xml', 'settings')
    + extra + '</Types>';
  const rels = '<?xml version="1.0" encoding="UTF-8" standalone="yes"?>\n<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">'
    + `<Relationship Id="rId1" Type="${REL}/officeDocument" Target="word/document.xml"/></Relationships>`;
  const docRels = '<?xml version="1.0" encoding="UTF-8" standalone="yes"?>\n<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">'
    + `<Relationship Id="rIdS" Type="${REL}/styles" Target="styles.xml"/>`
    + `<Relationship Id="rIdN" Type="${REL}/numbering" Target="numbering.xml"/>`
    + `<Relationship Id="rIdT" Type="${REL}/settings" Target="settings.xml"/>`
    + (endnotes.length ? `<Relationship Id="rIdE" Type="${REL}/endnotes" Target="endnotes.xml"/>` : '')
    + (hdr ? `<Relationship Id="rIdH" Type="${REL}/header" Target="header1.xml"/>` : '')
    + '</Relationships>';
  const files = {
    '[Content_Types].xml': strToU8(ct), '_rels/.rels': strToU8(rels), 'word/document.xml': strToU8(document),
    'word/_rels/document.xml.rels': strToU8(docRels), 'word/styles.xml': strToU8(styles), 'word/numbering.xml': strToU8(numbering),
    'word/settings.xml': strToU8(settings),
  };
  for (const [k, v] of Object.entries(parts)) files[k] = strToU8(v);
  return zipSync(files, { mtime: new Date(Date.UTC(2026, 0, 1)) });
}
