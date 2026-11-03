// Seeded documents for the F2 facets of capture-layout.mjs (record "Wave 1 — F2"): lists
// (multi-level, start overrides, restarts, legal numbering, a numbering style link, bullets and
// the symbol formats), headings with outline levels, a linked character style, a table style with
// conditional formats, DATE/TIME/CREATEDATE fields with pictures and text formats, formula fields
// over nested page fields, and automatic hyphenation with several languages. Open fonts only.
// Neither the Word oracle nor the F0 generator has DATE or formula fields.
//
//   node tools/gen-f2-docs.mjs --ts-root <checkout> --out <dir> [--count N] [--seed S]
//
// Then: node tools/capture-layout.mjs --ts-root <checkout> --mode files <dir>/*.docx --out … --dump …
const { mkdirSync, writeFileSync } = await import('node:fs');
const { join } = await import('node:path');
const { importFrom, tsRoot, Rng } = await import('../../uo-testkit/js/testkit.mjs');

const argv = process.argv.slice(2);
const opt = (n, d) => { const i = argv.indexOf(n); return i >= 0 ? argv[i + 1] : d; };
const root = tsRoot(argv);
const fflate = await importFrom(root, 'write/node_modules/fflate/esm/index.mjs');
const outDir = opt('--out');
const count = Number(opt('--count', '4'));
const seed = Number(opt('--seed', '3'));
mkdirSync(outDir, { recursive: true });

const W = 'http://schemas.openxmlformats.org/wordprocessingml/2006/main';
const R = 'http://schemas.openxmlformats.org/officeDocument/2006/relationships';
const esc = (s) => s.replace(/&/g, '&amp;').replace(/</g, '&lt;').replace(/>/g, '&gt;').replace(/"/g, '&quot;');

function doc(s) {
  const rng = new Rng(s);
  const pick = (a) => a[rng.below(a.length)];
  const chance = (pct) => rng.below(100) < pct;
  const t = (x) => `<w:r><w:t xml:space="preserve">${esc(x)}</w:t></w:r>`;
  const fld = (instr, result) => `<w:r><w:fldChar w:fldCharType="begin"/></w:r><w:r><w:instrText xml:space="preserve">${esc(instr)}</w:instrText></w:r><w:r><w:fldChar w:fldCharType="separate"/></w:r>${t(result)}<w:r><w:fldChar w:fldCharType="end"/></w:r>`;
  const formula = (pre, nested, post) => `<w:r><w:fldChar w:fldCharType="begin"/></w:r><w:r><w:instrText xml:space="preserve">${esc(pre)}</w:instrText></w:r>${fld(` ${nested} `, '3')}<w:r><w:instrText xml:space="preserve">${esc(post)}</w:instrText></w:r><w:r><w:fldChar w:fldCharType="separate"/></w:r>${t('2')}<w:r><w:fldChar w:fldCharType="end"/></w:r>`;
  const words = 'o motor diagrama listas numeradas campos de data hifenização automática informação internacionalização responsabilidade the quick brown fox jumps over the lazy dog extraordinarily'.split(' ');
  const text = (n) => Array.from({ length: n }, () => pick(words)).join(' ');
  const lang = () => pick(['pt-BR', 'en-US', 'es-ES', 'de-DE']);
  const para = (ppr, inner) => `<w:p>${ppr ? `<w:pPr>${ppr}</w:pPr>` : ''}${inner}</w:p>`;
  const blocks = [];
  const n = 12 + rng.below(30);
  for (let i = 0; i < n; i++) {
    const k = rng.below(10);
    if (k < 4) {
      const numId = pick(['1', '2', '3', '4', '5']);
      const ilvl = pick(['0', '0', '1', '1', '2', '3']);
      blocks.push(para(`<w:numPr><w:ilvl w:val="${ilvl}"/><w:numId w:val="${numId}"/></w:numPr>`, t(text(3 + rng.below(8)))));
    } else if (k === 4) {
      blocks.push(para(`<w:pStyle w:val="${pick(['Heading1', 'Heading2', 'Heading3', 'ListPara'])}"/>`, t(text(3))));
    } else if (k === 5) {
      const pic = pick(['"dd/MM/yyyy"', '"dddd, d \'de\' MMMM \'de\' yyyy"', '"h:mm am/pm"', '"MMMM yy"', 'dddd', '"HH:mm:ss"']);
      const name = pick(['DATE', 'TIME', 'CREATEDATE', 'SAVEDATE']);
      const fmt = pick(['', ' \\* Upper', ' \\* Caps', ' \\* FirstCap', ' \\* MERGEFORMAT']);
      blocks.push(para('', t('Data: ') + fld(` ${name} \\@ ${pic}${fmt} `, '01/01/2020')));
    } else if (k === 6) {
      const f = pick([
        () => formula(' = ', 'NUMPAGES', ' - 1 '),
        () => formula(' = ', 'PAGE', ' * 2 \\# "00" '),
        () => formula(' = (', 'SECTIONPAGES', ' + 1) / 3 '),
        () => formula(' = ', 'PAGE', ' \\* ROMAN '),
        () => fld(' =4/3 ', '1'),
        () => fld(' = 10 / 4 \\# "0,00" ', '2'),
      ]);
      blocks.push(para('', t('Total: ') + f()));
    } else {
      const rpr = chance(40) ? `<w:rPr><w:lang w:val="${lang()}"/>${chance(30) ? '<w:rStyle w:val="Strong"/>' : ''}</w:rPr>` : '';
      blocks.push(para(chance(20) ? '<w:pStyle w:val="ListPara"/>' : '', `<w:r>${rpr}<w:t xml:space="preserve">${esc(text(10 + rng.below(30)))}</w:t></w:r>`));
    }
    if (chance(8)) {
      const cell = (x) => `<w:tc><w:tcPr><w:tcW w:w="3000" w:type="dxa"/></w:tcPr>${para(chance(50) ? '<w:numPr><w:ilvl w:val="0"/><w:numId w:val="2"/></w:numPr>' : '', t(x))}</w:tc>`;
      blocks.push(`<w:tbl><w:tblPr><w:tblStyle w:val="Grid"/><w:tblW w:w="9000" w:type="dxa"/><w:tblLook w:firstRow="1" w:noHBand="0"/></w:tblPr><w:tblGrid><w:gridCol w:w="3000"/><w:gridCol w:w="3000"/><w:gridCol w:w="3000"/></w:tblGrid>`
        + `<w:tr>${cell('a')}${cell('b')}${cell('c')}</w:tr><w:tr>${cell('d')}${cell('e')}${cell('f')}</w:tr></w:tbl>`);
    }
  }
  const sectPr = '<w:sectPr><w:footerReference w:type="default" r:id="rIdF"/><w:pgSz w:w="11906" w:h="16838"/><w:pgMar w:top="1417" w:right="1701" w:bottom="1417" w:left="1701" w:header="708" w:footer="708" w:gutter="0"/>'
    + `${chance(50) ? '<w:pgNumType w:start="0"/>' : ''}<w:cols w:space="708"/></w:sectPr>`;
  const document = `<?xml version="1.0" encoding="UTF-8" standalone="yes"?>\n<w:document xmlns:w="${W}" xmlns:r="${R}"><w:body>${blocks.join('')}${sectPr}</w:body></w:document>`;
  const footer = `<?xml version="1.0" encoding="UTF-8" standalone="yes"?>\n<w:ftr xmlns:w="${W}" xmlns:r="${R}"><w:p><w:pPr><w:jc w:val="center"/></w:pPr>${fld(' PAGE ', '1')}${t(' de ')}${formula(' = ', 'NUMPAGES', ' - 1 ')}${t(' — ')}${fld(' DATE \\@ "d/M/yyyy" ', 'x')}</w:p></w:ftr>`;
  const lvl = (i, fmt, text, extra = '') => `<w:lvl w:ilvl="${i}"><w:start w:val="${pick(['1', '1', '0', '3'])}"/><w:numFmt w:val="${fmt}"/><w:lvlText w:val="${esc(text)}"/>${extra}<w:pPr><w:ind w:left="${720 * (i + 1)}" w:hanging="360"/></w:pPr></w:lvl>`;
  const abs = (id, levels, link = '') => `<w:abstractNum w:abstractNumId="${id}">${link}${levels.join('')}</w:abstractNum>`;
  const numbering = `<?xml version="1.0" encoding="UTF-8" standalone="yes"?>\n<w:numbering xmlns:w="${W}">`
    + abs('0', [lvl(0, 'decimal', '%1.'), lvl(1, 'decimal', '%1.%2.'), lvl(2, 'lowerLetter', '%3)', '<w:lvlRestart w:val="1"/>'), lvl(3, 'decimal', '%1.%2.%3.%4', '<w:isLgl/>')])
    + abs('1', [lvl(0, 'upperRoman', 'Capítulo %1'), lvl(1, pick(['chicago', 'cardinalText', 'ordinal']), '%2'), lvl(2, 'bullet', '•'), lvl(3, 'decimalZero', '(%4)')])
    + abs('2', [lvl(0, 'bullet', '–'), lvl(1, 'lowerRoman', '%2.')])
    + abs('3', [], '<w:numStyleLink w:val="ListNum"/>')
    + '<w:num w:numId="1"><w:abstractNumId w:val="0"/></w:num>'
    + '<w:num w:numId="2"><w:abstractNumId w:val="2"/></w:num>'
    + `<w:num w:numId="3"><w:abstractNumId w:val="1"/><w:lvlOverride w:ilvl="0"><w:startOverride w:val="${pick(['4', '2'])}"/></w:lvlOverride></w:num>`
    + `<w:num w:numId="4"><w:abstractNumId w:val="0"/><w:lvlOverride w:ilvl="1"><w:startOverride w:val="5"/></w:lvlOverride></w:num>`
    + '<w:num w:numId="5"><w:abstractNumId w:val="3"/></w:num>'
    + '</w:numbering>';
  const style = (type, id, name, inner, extra = '') => `<w:style w:type="${type}" w:styleId="${id}"${extra}><w:name w:val="${name}"/>${inner}</w:style>`;
  const styles = `<?xml version="1.0" encoding="UTF-8" standalone="yes"?>\n<w:styles xmlns:w="${W}"><w:docDefaults><w:rPrDefault><w:rPr>`
    + `<w:rFonts w:ascii="Liberation Serif" w:hAnsi="Liberation Serif" w:eastAsia="Liberation Serif" w:cs="Liberation Serif"/><w:sz w:val="22"/><w:lang w:val="${lang()}"/>`
    + `</w:rPr></w:rPrDefault><w:pPrDefault><w:pPr><w:spacing w:after="160" w:line="259" w:lineRule="auto"/></w:pPr></w:pPrDefault></w:docDefaults>`
    + style('paragraph', 'Normal', 'Normal', '', ' w:default="1"')
    + style('paragraph', 'Heading1', 'heading 1', '<w:basedOn w:val="Normal"/><w:pPr><w:keepNext/><w:numPr><w:numId w:val="1"/></w:numPr><w:outlineLvl w:val="0"/></w:pPr><w:rPr><w:b/><w:caps/><w:sz w:val="32"/></w:rPr>')
    + style('paragraph', 'Heading2', 'heading 2', '<w:basedOn w:val="Heading1"/><w:pPr><w:numPr><w:ilvl w:val="1"/></w:numPr><w:outlineLvl w:val="1"/></w:pPr><w:rPr><w:i/></w:rPr>')
    + style('paragraph', 'Heading3', 'Título 3', '<w:basedOn w:val="Normal"/><w:pPr><w:outlineLvl w:val="2"/></w:pPr><w:rPr><w:lang w:val="es-ES"/></w:rPr>')
    + style('paragraph', 'ListPara', 'List Paragraph', '<w:basedOn w:val="Normal"/><w:pPr><w:ind w:left="720"/><w:contextualSpacing/></w:pPr>')
    + style('character', 'Strong', 'Strong', '<w:rPr><w:b/><w:color w:val="C00000" w:themeColor="accent2"/></w:rPr>')
    + style('numbering', 'ListNum', 'List Num', '<w:pPr><w:numPr><w:numId w:val="1"/></w:numPr></w:pPr>')
    + style('table', 'Grid', 'Table Grid', '<w:tblPr><w:tblBorders><w:top w:val="single" w:sz="4"/></w:tblBorders><w:tblCellMar><w:left w:w="108" w:type="dxa"/></w:tblCellMar></w:tblPr><w:tblStylePr w:type="firstRow"><w:rPr><w:b/></w:rPr><w:tcPr><w:shd w:val="clear" w:fill="D9D9D9"/></w:tcPr></w:tblStylePr>')
    + '</w:styles>';
  const settings = `<?xml version="1.0" encoding="UTF-8" standalone="yes"?>\n<w:settings xmlns:w="${W}"><w:defaultTabStop w:val="708"/>${chance(70) ? '<w:autoHyphenation/>' : ''}<w:compat><w:compatSetting w:name="compatibilityMode" w:uri="http://schemas.microsoft.com/office/word" w:val="15"/></w:compat></w:settings>`;
  const core = `<?xml version="1.0" encoding="UTF-8" standalone="yes"?>\n<cp:coreProperties xmlns:cp="http://schemas.openxmlformats.org/package/2006/metadata/core-properties" xmlns:dc="http://purl.org/dc/elements/1.1/" xmlns:dcterms="http://purl.org/dc/terms/" xmlns:xsi="http://www.w3.org/2001/XMLSchema-instance">`
    + `<dcterms:created xsi:type="dcterms:W3CDTF">${pick(['2019-02-17T01:30:00Z', '1999-12-31T23:59:59Z', '2026-01-15T12:00:00Z'])}</dcterms:created><dcterms:modified xsi:type="dcterms:W3CDTF">2024-06-30T22:15:00Z</dcterms:modified></cp:coreProperties>`;
  const ct = '<?xml version="1.0" encoding="UTF-8" standalone="yes"?>\n<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types">'
    + '<Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/><Default Extension="xml" ContentType="application/xml"/>'
    + '<Override PartName="/word/document.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml"/>'
    + '<Override PartName="/word/styles.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.styles+xml"/>'
    + '<Override PartName="/word/numbering.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.numbering+xml"/>'
    + '<Override PartName="/word/settings.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.settings+xml"/>'
    + '<Override PartName="/word/footer1.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.footer+xml"/>'
    + '<Override PartName="/docProps/core.xml" ContentType="application/vnd.openxmlformats-package.core-properties+xml"/></Types>';
  const rels = '<?xml version="1.0" encoding="UTF-8" standalone="yes"?>\n<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">'
    + '<Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="word/document.xml"/>'
    + '<Relationship Id="rId2" Type="http://schemas.openxmlformats.org/package/2006/relationships/metadata/core-properties" Target="docProps/core.xml"/></Relationships>';
  const docRels = '<?xml version="1.0" encoding="UTF-8" standalone="yes"?>\n<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">'
    + '<Relationship Id="rIdS" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/styles" Target="styles.xml"/>'
    + '<Relationship Id="rIdN" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/numbering" Target="numbering.xml"/>'
    + '<Relationship Id="rIdT" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/settings" Target="settings.xml"/>'
    + '<Relationship Id="rIdF" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/footer" Target="footer1.xml"/></Relationships>';
  const { zipSync, strToU8 } = fflate;
  return zipSync({
    '[Content_Types].xml': strToU8(ct), '_rels/.rels': strToU8(rels), 'word/document.xml': strToU8(document), 'word/_rels/document.xml.rels': strToU8(docRels),
    'word/styles.xml': strToU8(styles), 'word/numbering.xml': strToU8(numbering), 'word/settings.xml': strToU8(settings), 'word/footer1.xml': strToU8(footer), 'docProps/core.xml': strToU8(core),
  }, { mtime: new Date(Date.UTC(2026, 0, 1)) });
}

for (let i = 0; i < count; i++) writeFileSync(join(outDir, `f2-${seed}-${i}.docx`), doc((seed * 1000003 + i) >>> 0));
console.log(`${count} documents -> ${outDir}`);
