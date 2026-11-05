// Seeded documents of frontier F7c (record "Stage 3 — F7c") with open fonts only: what F7c lays
// out end to end on top of F7a — headers and footers (default / first / even slots, `titlePg`,
// `evenAndOddHeaders`, inherited between sections, lists and PAGE / NUMPAGES / SECTIONPAGES /
// SECTION / DATE fields in them), footnotes (one or more paragraphs, long notes that split
// between pages, number formats and restarts, "beneath text", note columns), endnotes (at the
// section end or the document end, `noEndnote`, in columns), and the fields SEQ (`\* \r \c \h \s`),
// REF (text, `\n \r \w \d`, `\p`), PAGEREF, NOTEREF and STYLEREF over bookmarks and headings. No
// table, float, frame or textbox (F7b). `generateF7cDocx(seed)` is a pure function of the seed
// (the gen-docx.mjs contract).
import { Rng } from '../../../uo-testkit/js/testkit.mjs';

const W = 'http://schemas.openxmlformats.org/wordprocessingml/2006/main';
const R = 'http://schemas.openxmlformats.org/officeDocument/2006/relationships';
const W15 = 'http://schemas.microsoft.com/office/word/2012/wordml';
const FAMILIES = ['Liberation Serif', 'Liberation Sans', 'Liberation Mono', 'Carlito', 'Caladea'];
const WORDS = ('o motor de layout quebra linhas pagina colunas secoes notas rodape cabecalho e desenha cada glifo '
  + 'acao coracao informacao paralelepipedo hifenizacao the quick brown fox jumps over the lazy dog '
  + 'documento sessao edicao configuracao internacionalizacao responsabilidade').split(' ');
const ACCENTED = ['ação', 'coração', 'informação', 'parágrafo', 'página', 'câmera', 'português', 'útil', 'você'];
const REL = 'http://schemas.openxmlformats.org/officeDocument/2006/relationships';

const esc = (s) => s.replace(/&/g, '&amp;').replace(/</g, '&lt;').replace(/>/g, '&gt;');
const q = (s) => esc(s).replace(/"/g, '&quot;');

export function generateF7cDocx(seed, { zipSync, strToU8 }) {
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
    if (chance(25)) { const f = pick(FAMILIES); rpr.push(`<w:rFonts w:ascii="${f}" w:hAnsi="${f}" w:cs="${f}"/>`); }
    if (chance(12)) rpr.push('<w:b/>');
    if (chance(12)) rpr.push('<w:i/>');
    if (chance(20)) rpr.push(`<w:sz w:val="${pick([16, 20, 22, 24, 28])}"/>`);
    if (extra) rpr.push(extra);
    return `<w:r>${rpr.length ? `<w:rPr>${rpr.join('')}</w:rPr>` : ''}<w:t xml:space="preserve">${esc(t)}</w:t></w:r>`;
  };
  const field = (instr, cached = '1') => `<w:fldSimple w:instr=" ${q(instr)} "><w:r><w:t xml:space="preserve">${esc(cached)}</w:t></w:r></w:fldSimple>`;
  const lvlRef = () => `<w:numPr><w:ilvl w:val="${rng.below(3)}"/><w:numId w:val="${1 + rng.below(2)}"/></w:numPr>`;

  // ── notes ──
  const footnotes = [];
  const endnotes = [];
  let noteId = 1;
  const notePara = (kind, first) => {
    const parts = [];
    if (first) parts.push(`<w:r><w:rPr><w:rStyle w:val="${kind === 'f' ? 'Refdenotaderodap' : 'Refdenotadefim'}"/></w:rPr><w:${kind === 'f' ? 'footnoteRef' : 'endnoteRef'}/></w:r>`);
    parts.push(run(` ${text(3 + rng.below(chance(25) ? 90 : 20))}`));
    if (chance(8)) parts.push(field(pick(['PAGE', 'NUMPAGES', 'DATE \\@ "dd/MM/yyyy"'])));
    const ppr = [`<w:pStyle w:val="${kind === 'f' ? 'Textodenotaderodap' : 'Textodenotadefim'}"/>`];
    if (chance(15)) ppr.push(`<w:spacing w:after="${pick([0, 60, 120])}"/>`);
    if (chance(10)) ppr.push(lvlRef());
    return `<w:p><w:pPr>${ppr.join('')}</w:pPr>${parts.join('')}</w:p>`;
  };
  const noteRef = (kind) => {
    const id = ++noteId;
    const n = 1 + (chance(25) ? rng.below(3) : 0);
    const ps = [];
    for (let k = 0; k < n; k++) ps.push(notePara(kind, k === 0));
    (kind === 'f' ? footnotes : endnotes).push(`<w:${kind === 'f' ? 'footnote' : 'endnote'} w:id="${id}">${ps.join('')}</w:${kind === 'f' ? 'footnote' : 'endnote'}>`);
    const custom = chance(4);
    return `<w:r><w:rPr><w:rStyle w:val="${kind === 'f' ? 'Refdenotaderodap' : 'Refdenotadefim'}"/></w:rPr><w:${kind === 'f' ? 'footnoteReference' : 'endnoteReference'}${custom ? ' w:customMarkFollows="1"' : ''} w:id="${id}"/></w:r>${custom ? '<w:r><w:t>*</w:t></w:r>' : ''}`;
  };

  // ── bookmarks, headings, SEQ/REF fields ──
  const bookmarks = [];
  let bmId = 0;
  const STYLES = ['Normal', 'Corpo', 'Titulo1', 'Titulo2'];
  const para = (opts = {}) => {
    const ppr = [];
    const style = opts.style ?? (chance(30) ? pick(STYLES) : null);
    if (style && style !== 'Normal') ppr.push(`<w:pStyle w:val="${style}"/>`);
    if (chance(8)) ppr.push('<w:keepNext/>');
    if (chance(6)) ppr.push('<w:keepLines/>');
    if (chance(3)) ppr.push('<w:pageBreakBefore/>');
    if (chance(8)) ppr.push(`<w:widowControl w:val="${chance(50) ? '0' : '1'}"/>`);
    if (chance(10) || (style === 'Titulo1' && chance(60))) ppr.push(lvlRef());
    if (chance(15)) ppr.push(`<w:spacing w:before="${pick([0, 120, 240])}" w:after="${pick([0, 120, 160, 240])}" w:line="${pick([240, 276, 360])}" w:lineRule="${pick(['auto', 'exact', 'atLeast'])}"/>`);
    if (chance(15)) ppr.push(`<w:jc w:val="${pick(['left', 'center', 'right', 'both'])}"/>`);
    if (opts.sect) ppr.push(opts.sect);
    const parts = [];
    if (!opts.empty) {
      const n = 1 + rng.below(4);
      for (let i = 0; i < n; i++) {
        let t = run(text(1 + rng.below(opts.long ? 50 : 25)));
        if (chance(10)) {
          const name = `bm${++bmId}`;
          bookmarks.push(name);
          t = `<w:bookmarkStart w:id="${bmId}" w:name="${name}"/>${t}${chance(80) ? `<w:bookmarkEnd w:id="${bmId}"/>` : ''}`;
        }
        parts.push(t);
        if (chance(opts.notes ?? 18)) parts.push(noteRef(chance(70) ? 'f' : 'e'));
      }
      if (chance(10)) parts.push(field(pick(['SEQ Figura', 'SEQ Figura \\* ROMAN', 'SEQ Tabela \\* alphabetic', 'SEQ Figura \\r 5', 'SEQ Figura \\c', 'SEQ Figura \\h', 'SEQ Figura \\s 1', 'SEQ Tabela \\* ARABIC \\s 2']), '0'));
      if (bookmarks.length && chance(14)) {
        const bm = pick(bookmarks);
        parts.push(field(pick([`REF ${bm} \\h`, `REF ${bm} \\n`, `REF ${bm} \\r \\h`, `REF ${bm} \\w \\d "-"`, `REF ${bm} \\p`, `PAGEREF ${bm} \\h`, `PAGEREF ${bm}`, `NOTEREF ${bm} \\h`, `NOTEREF ${bm} \\p`]), '?'));
      }
      if (chance(6)) parts.push(field(pick(['STYLEREF 1', 'STYLEREF "1" \\n', 'STYLEREF 2 \\s', 'STYLEREF 1 \\n']), '?'));
      if (chance(5)) parts.push(field(pick(['PAGE', 'NUMPAGES', 'SECTIONPAGES', 'SECTION'])));
      if (chance(3)) parts.push('<w:r><w:br w:type="page"/></w:r>', run(text(3)));
      if (chance(3)) parts.push('<w:r><w:br w:type="column"/></w:r>', run(text(3)));
    }
    return `<w:p>${ppr.length ? `<w:pPr>${ppr.join('')}</w:pPr>` : ''}${parts.join('')}</w:p>`;
  };

  // ── headers and footers ──
  const parts = {};
  const hfRels = [];
  let hfN = 0;
  const hfPara = () => {
    const ppr = [`<w:pStyle w:val="${chance(50) ? 'Cabealho' : 'Rodap'}"/>`];
    if (chance(20)) ppr.push(`<w:jc w:val="${pick(['center', 'right'])}"/>`);
    if (chance(10)) ppr.push(lvlRef());
    if (chance(20)) ppr.push(`<w:spacing w:before="${pick([0, 120])}" w:after="${pick([0, 120, 240])}"${chance(30) ? ' w:afterAutospacing="1"' : ''}/>`);
    if (chance(8)) ppr.push(`<w:pBdr><w:bottom w:val="single" w:sz="${pick([4, 8])}" w:space="1" w:color="auto"/></w:pBdr>`);
    const out = [];
    if (chance(80)) out.push(run(text(1 + rng.below(chance(15) ? 40 : 8))));
    if (chance(55)) out.push(run(pick([' Página ', ' p. ', ' '])), field(pick(['PAGE', 'PAGE \\* roman', 'PAGE \\* ArabicDash', 'PAGE \\* upperLetter'])));
    if (chance(30)) out.push(run(pick([' de ', ' / '])), field(pick(['NUMPAGES', 'SECTIONPAGES'])));
    if (chance(8)) out.push(field(pick(['SECTION', 'DATE \\@ "d/M/yyyy"', 'TIME \\@ "HH:mm"'])));
    if (chance(8)) out.push('<w:r><w:tab/></w:r>', run(text(2)));
    return `<w:p><w:pPr>${ppr.join('')}</w:pPr>${out.join('')}</w:p>`;
  };
  const hfPart = (tag) => {
    const id = `rIdH${++hfN}`;
    const file = `${tag === 'hdr' ? 'header' : 'footer'}${hfN}.xml`;
    const n = chance(10) ? 0 : 1 + rng.below(chance(15) ? 4 : 2);
    const ps = [];
    for (let k = 0; k < n; k++) ps.push(hfPara());
    if (!ps.length) ps.push('<w:p/>');
    parts[`word/${file}`] = `<?xml version="1.0" encoding="UTF-8" standalone="yes"?>\n<w:${tag} xmlns:w="${W}" xmlns:r="${R}">${ps.join('')}</w:${tag}>`;
    hfRels.push(`<Relationship Id="${id}" Type="${REL}/${tag === 'hdr' ? 'header' : 'footer'}" Target="${file}"/>`);
    return id;
  };
  const hfRefs = (first) => {
    const out = [];
    if (!first && chance(35)) return ''; // inherited from the previous section
    for (const kind of ['default', 'first', 'even']) {
      if (kind !== 'default' && chance(60)) continue;
      if (chance(85)) out.push(`<w:headerReference w:type="${kind}" r:id="${hfPart('hdr')}"/>`);
      if (chance(85)) out.push(`<w:footerReference w:type="${kind}" r:id="${hfPart('ftr')}"/>`);
    }
    return out.join('');
  };

  const pgNum = () => (chance(20) ? `<w:pgNumType${chance(50) ? ` w:start="${rng.below(5)}"` : ''}${chance(40) ? ` w:fmt="${pick(['lowerRoman', 'upperLetter', 'decimal'])}"` : ''}/>` : '');
  const notePr = () => (chance(20) ? `<w:footnotePr>${chance(50) ? `<w:numFmt w:val="${pick(['lowerRoman', 'upperLetter', 'decimal', 'chicago'])}"/>` : ''}${chance(40) ? `<w:numRestart w:val="${pick(['eachPage', 'eachSect', 'continuous'])}"/>` : ''}${chance(20) ? '<w:pos w:val="beneathText"/>' : ''}</w:footnotePr>` : '')
    + (chance(10) ? `<w:endnotePr><w:numFmt w:val="${pick(['decimal', 'lowerRoman', 'upperRoman'])}"/></w:endnotePr>` : '');
  let firstSect = true;
  const sectPr = (type, cols) => {
    const s = `<w:sectPr>${hfRefs(firstSect)}${notePr()}${type ? `<w:type w:val="${type}"/>` : ''}`
      + `<w:pgSz w:w="${chance(85) ? 11906 : 16838}" w:h="${chance(85) ? 16838 : 11906}"/>`
      + `<w:pgMar w:top="${pick([1417, 720, 1701])}" w:right="1701" w:bottom="${pick([1417, 1134])}" w:left="${pick([1701, 1134])}" w:header="${pick([708, 284, 1134])}" w:footer="${pick([708, 284])}" w:gutter="0"/>`
      + `${pgNum()}<w:cols w:num="${cols}" w:space="${pick([708, 425])}"${cols > 1 && chance(30) ? ' w:sep="1"' : ''}/>`
      + `${chance(25) ? '<w:titlePg/>' : ''}${chance(6) ? '<w:noEndnote/>' : ''}${chance(8) ? `<w15:footnoteColumns w:val="${1 + rng.below(2)}"/>` : ''}</w:sectPr>`;
    firstSect = false;
    return s;
  };

  const blocks = [];
  const nSect = 1 + rng.below(3);
  for (let s = 0; s < nSect; s++) {
    const n = 2 + rng.below(22);
    for (let i = 0; i < n; i++) {
      const r = rng.below(100);
      blocks.push(para({ empty: r < 4, long: r >= 4 && r < 20 }));
    }
    if (s < nSect - 1) {
      const type = pick(['continuous', 'nextPage', 'nextPage', 'evenPage', 'oddPage']);
      blocks.push(para({ sect: sectPr(type, chance(80) ? 1 : 2), empty: chance(40) }));
    }
  }
  const lastSect = sectPr(null, chance(85) ? 1 : 2);
  const document = `<?xml version="1.0" encoding="UTF-8" standalone="yes"?>\n<w:document xmlns:w="${W}" xmlns:r="${R}" xmlns:w15="${W15}"><w:body>${blocks.join('')}${lastSect}</w:body></w:document>`;
  const sep = (kind) => `<w:${kind} w:type="separator" w:id="-1"><w:p><w:pPr><w:spacing w:after="0" w:line="240" w:lineRule="auto"/></w:pPr><w:r><w:separator/></w:r></w:p></w:${kind}>`
    + `<w:${kind} w:type="continuationSeparator" w:id="0"><w:p><w:pPr><w:spacing w:after="0" w:line="240" w:lineRule="auto"/></w:pPr><w:r><w:continuationSeparator/></w:r></w:p></w:${kind}>`;
  if (footnotes.length) parts['word/footnotes.xml'] = `<?xml version="1.0" encoding="UTF-8" standalone="yes"?>\n<w:footnotes xmlns:w="${W}" xmlns:r="${R}">${sep('footnote')}${footnotes.join('')}</w:footnotes>`;
  if (endnotes.length) parts['word/endnotes.xml'] = `<?xml version="1.0" encoding="UTF-8" standalone="yes"?>\n<w:endnotes xmlns:w="${W}" xmlns:r="${R}">${sep('endnote')}${endnotes.join('')}</w:endnotes>`;
  const styles = `<?xml version="1.0" encoding="UTF-8" standalone="yes"?>\n<w:styles xmlns:w="${W}"><w:docDefaults><w:rPrDefault><w:rPr>`
    + `<w:rFonts w:ascii="${pick(FAMILIES)}" w:hAnsi="Liberation Serif" w:eastAsia="Liberation Serif" w:cs="Liberation Serif"/><w:sz w:val="22"/><w:szCs w:val="22"/><w:lang w:val="${chance(70) ? 'pt-BR' : 'en-US'}"/>`
    + `</w:rPr></w:rPrDefault><w:pPrDefault><w:pPr><w:spacing w:after="160" w:line="259" w:lineRule="auto"/></w:pPr></w:pPrDefault></w:docDefaults>`
    + `<w:style w:type="paragraph" w:default="1" w:styleId="Normal"><w:name w:val="Normal"/></w:style>`
    + `<w:style w:type="paragraph" w:styleId="Corpo"><w:name w:val="Corpo"/><w:basedOn w:val="Normal"/><w:pPr><w:spacing w:before="120" w:after="120"/><w:contextualSpacing/></w:pPr></w:style>`
    + `<w:style w:type="paragraph" w:styleId="Titulo1"><w:name w:val="heading 1"/><w:basedOn w:val="Normal"/><w:next w:val="Normal"/><w:pPr><w:keepNext/><w:keepLines/><w:spacing w:before="240" w:after="0"/><w:outlineLvl w:val="0"/></w:pPr><w:rPr><w:sz w:val="32"/></w:rPr></w:style>`
    + `<w:style w:type="paragraph" w:styleId="Titulo2"><w:name w:val="heading 2"/><w:basedOn w:val="Normal"/><w:next w:val="Normal"/><w:pPr><w:keepNext/><w:spacing w:before="40" w:after="0"/><w:outlineLvl w:val="1"/></w:pPr><w:rPr><w:sz w:val="26"/></w:rPr></w:style>`
    + `<w:style w:type="paragraph" w:styleId="Cabealho"><w:name w:val="header"/><w:basedOn w:val="Normal"/><w:pPr><w:tabs><w:tab w:val="center" w:pos="4252"/><w:tab w:val="right" w:pos="8504"/></w:tabs><w:spacing w:after="0" w:line="240" w:lineRule="auto"/></w:pPr></w:style>`
    + `<w:style w:type="paragraph" w:styleId="Rodap"><w:name w:val="footer"/><w:basedOn w:val="Normal"/><w:pPr><w:tabs><w:tab w:val="center" w:pos="4252"/><w:tab w:val="right" w:pos="8504"/></w:tabs><w:spacing w:after="0" w:line="240" w:lineRule="auto"/></w:pPr></w:style>`
    + `<w:style w:type="paragraph" w:styleId="Textodenotaderodap"><w:name w:val="footnote text"/><w:basedOn w:val="Normal"/><w:pPr><w:spacing w:after="0" w:line="240" w:lineRule="auto"/></w:pPr><w:rPr><w:sz w:val="20"/></w:rPr></w:style>`
    + `<w:style w:type="character" w:styleId="Refdenotaderodap"><w:name w:val="footnote reference"/><w:rPr><w:vertAlign w:val="superscript"/></w:rPr></w:style>`
    + `<w:style w:type="paragraph" w:styleId="Textodenotadefim"><w:name w:val="endnote text"/><w:basedOn w:val="Normal"/><w:pPr><w:spacing w:after="0" w:line="240" w:lineRule="auto"/></w:pPr><w:rPr><w:sz w:val="20"/></w:rPr></w:style>`
    + `<w:style w:type="character" w:styleId="Refdenotadefim"><w:name w:val="endnote reference"/><w:rPr><w:vertAlign w:val="superscript"/></w:rPr></w:style></w:styles>`;
  const lvl = (i, fmt, txt, pstyle) => `<w:lvl w:ilvl="${i}"><w:start w:val="1"/><w:numFmt w:val="${fmt}"/>${pstyle ? `<w:pStyle w:val="${pstyle}"/>` : ''}<w:lvlText w:val="${txt}"/><w:lvlJc w:val="left"/><w:pPr><w:ind w:left="${720 * (i + 1)}" w:hanging="360"/></w:pPr></w:lvl>`;
  const numbering = `<?xml version="1.0" encoding="UTF-8" standalone="yes"?>\n<w:numbering xmlns:w="${W}">`
    + `<w:abstractNum w:abstractNumId="0"><w:multiLevelType w:val="multilevel"/>${lvl(0, 'decimal', '%1.')}${lvl(1, 'decimal', '%1.%2.')}${lvl(2, 'lowerRoman', '%3.')}</w:abstractNum>`
    + `<w:abstractNum w:abstractNumId="1"><w:multiLevelType w:val="hybridMultilevel"/>${lvl(0, 'bullet', '-')}${lvl(1, 'bullet', 'o')}${lvl(2, 'bullet', '*')}</w:abstractNum>`
    + `<w:num w:numId="1"><w:abstractNumId w:val="0"/></w:num><w:num w:numId="2"><w:abstractNumId w:val="1"/></w:num></w:numbering>`;
  const settingsNotes = (footnotes.length ? '<w:footnotePr><w:footnote w:id="-1"/><w:footnote w:id="0"/></w:footnotePr>' : '')
    + (endnotes.length ? `<w:endnotePr>${chance(40) ? '<w:pos w:val="sectEnd"/>' : ''}<w:endnote w:id="-1"/><w:endnote w:id="0"/></w:endnotePr>` : '');
  const settings = `<?xml version="1.0" encoding="UTF-8" standalone="yes"?>\n<w:settings xmlns:w="${W}"><w:defaultTabStop w:val="708"/>${chance(30) ? '<w:evenAndOddHeaders/>' : ''}${settingsNotes}<w:compat><w:compatSetting w:name="compatibilityMode" w:uri="http://schemas.microsoft.com/office/word" w:val="${chance(85) ? 15 : 14}"/></w:compat></w:settings>`;
  const over = (part, type) => `<Override PartName="/${part}" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.${type}+xml"/>`;
  const extra = Object.keys(parts).map((p) => over(p, /header/.test(p) ? 'header' : /footer/.test(p) ? 'footer' : /footnotes/.test(p) ? 'footnotes' : 'endnotes')).join('');
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
    + (footnotes.length ? `<Relationship Id="rIdF" Type="${REL}/footnotes" Target="footnotes.xml"/>` : '')
    + (endnotes.length ? `<Relationship Id="rIdE" Type="${REL}/endnotes" Target="endnotes.xml"/>` : '')
    + hfRels.join('') + '</Relationships>';
  const files = {
    '[Content_Types].xml': strToU8(ct), '_rels/.rels': strToU8(rels), 'word/document.xml': strToU8(document),
    'word/_rels/document.xml.rels': strToU8(docRels), 'word/styles.xml': strToU8(styles), 'word/numbering.xml': strToU8(numbering),
    'word/settings.xml': strToU8(settings),
  };
  for (const [k, v] of Object.entries(parts)) files[k] = strToU8(v);
  return zipSync(files, { mtime: new Date(Date.UTC(2026, 0, 1)) });
}
