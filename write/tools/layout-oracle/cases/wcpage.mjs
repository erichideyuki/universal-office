// WordCraft study (2026-11-13), the "page" planner: page/flow/section rules Word decides (rust-core worktree,
// core/target/wordcraft-study/QUEUE.md + VERDICTS.json). Run with --fixdir core/target/wordcraft-study/page/wcpage.
//  p15-*  A15: space before of a paragraph that STARTS with a page break, mode 15 (resolves the conflict between
//         the A planner's a15-* cases — before dropped — and the frozen corpus's tocref-pbstart-c15 — before kept)
//  p16-*  A16: odd/even section breaks — physical page vs continued number, titlePg, inherited headers, NUMPAGES,
//         SECTIONPAGES, the blank page's size and section
//  g-*    gutter, mirrorMargins, gutterAtTop, rtlGutter, bookFoldPrinting, printTwoOnOne (text, header/footer,
//         table, columns, floats, page borders, restarted numbering)
//  mi-*   mirrorIndents (a paragraph crossing pages, hanging/first line, numbering, right aligned, tabs, restart)
//  b10x-* B10: line-relative float on the same page; a paragraph crossing COLUMNS with its anchor in column 2
//  b11x-* B11: line-relative negative offset; the re-wrap pushing the anchor paragraph to the next page
// Liberation Sans 11 / A4 unless noted; invented texts; installed fonts referenced by NAME only.
import { readFileSync } from 'node:fs';
import { unzipSync, strFromU8 } from 'fflate';
import { buildDocx, docDefaults, p, r, lorem } from '../docx-builder.mjs';
import { tbx } from './_tbx.mjs';

const LS = 'Liberation Sans';
const D = (body, o = {}) => buildDocx({ body, defaults: docDefaults({ font: o.font ?? LS, size: 11 }), ...o });
const fld = (instr, shown) => `<w:r><w:fldChar w:fldCharType="begin"/></w:r><w:r><w:instrText xml:space="preserve"> ${instr} </w:instrText></w:r><w:r><w:fldChar w:fldCharType="separate"/></w:r><w:r><w:t>${shown}</w:t></w:r><w:r><w:fldChar w:fldCharType="end"/></w:r>`;
const filler = (n, w = 'enchimento') => Array.from({ length: n }, (_, k) => p(r(`${w} ${k + 1} Hxg`))).join('');
const W_SETTINGS = (pre, post = '') => `${pre}<w:defaultTabStop w:val="708"/>${post}<w:characterSpacingControl w:val="doNotCompress"/><w:compat><w:compatSetting w:name="compatibilityMode" w:uri="http://schemas.microsoft.com/office/word" w:val="15"/></w:compat>`;

export const cases = [];

// ── A15 ─────────────────────────────────────────────────────────────────────────────────────────────────────────
{
  const BR = '<w:r><w:br w:type="page"/></w:r>';
  const P1 = (sp = '<w:spacing w:before="0" w:after="200"/>') => p(r('Primeiro Hxg'), sp);
  const P2 = (sp, runs = BR + r('Segundo Hxg'), extra = '') => p(runs, extra + sp);
  const P3 = p(r('Terceiro Hxg'));
  cases.push({ name: 'p15-base', docx: D(P1() + P2('<w:spacing w:before="480" w:after="0"/>') + P3) });
  cases.push({ name: 'p15-base-calibri', docx: D(P1() + P2('<w:spacing w:before="480" w:after="0"/>') + P3, { font: 'Calibri' }) });
  cases.push({ name: 'p15-b240a60', docx: D(p(r('Texto da primeira pagina Hxg')) + P2('<w:spacing w:before="240" w:after="60"/>') + P3) });
  cases.push({ name: 'p15-b480a60', docx: D(p(r('Texto da primeira pagina Hxg')) + P2('<w:spacing w:before="480" w:after="60"/>') + P3) });
  cases.push({ name: 'p15-b240a0', docx: D(p(r('Texto da primeira pagina Hxg')) + P2('<w:spacing w:before="240" w:after="0"/>') + P3) });
  const numbering = '<w:abstractNum w:abstractNumId="1"><w:multiLevelType w:val="multilevel"/><w:lvl w:ilvl="0"><w:start w:val="1"/><w:numFmt w:val="decimal"/><w:lvlText w:val="%1."/><w:lvlJc w:val="left"/><w:pPr><w:ind w:left="360" w:hanging="360"/></w:pPr></w:lvl></w:abstractNum><w:num w:numId="1"><w:abstractNumId w:val="1"/></w:num>';
  cases.push({ name: 'p15-b240a60-num', docx: D(p(r('Texto da primeira pagina Hxg')) + P2('<w:spacing w:before="240" w:after="60"/>', undefined, '<w:numPr><w:ilvl w:val="0"/><w:numId w:val="1"/></w:numPr>') + P3, { numbering }) });
  cases.push({ name: 'p15-b240a60-bm', docx: D(p(r('Texto da primeira pagina Hxg')) + P2('<w:spacing w:before="240" w:after="60"/>', BR + '<w:bookmarkStart w:id="1" w:name="_Toc1"/>' + r('Segundo Hxg') + '<w:bookmarkEnd w:id="1"/>') + P3) });
  cases.push({ name: 'p15-samerun', docx: D(p(r('Texto da primeira pagina Hxg')) + P2('<w:spacing w:before="240" w:after="60"/>', '<w:r><w:br w:type="page"/><w:t>Segundo Hxg</w:t></w:r>') + P3) });
  // the frozen corpus's document itself (measured 2026-10-11 with before kept), re-measured with today's Word
  const tocref = readFileSync(new URL('../../../packages/docx-layout/test/oracle/tocref/tocref-pbstart-c15.docx', import.meta.url));
  cases.push({ name: 'p15-tocref', docx: new Uint8Array(tocref) });
  // the same body without the TOC paragraphs (their PAGEREF/TOC fields aside)
  const z = unzipSync(new Uint8Array(tocref));
  const docXml = strFromU8(z['word/document.xml']);
  const bodyIn = docXml.slice(docXml.indexOf('<w:body>') + 8, docXml.indexOf('<w:sectPr>'));
  const paras = bodyIn.split(/(?=<w:p>)/).filter(Boolean);
  const num = strFromU8(z['word/numbering.xml']);
  const numInner = num.slice(num.indexOf('<w:abstractNum '), num.lastIndexOf('</w:numbering>'));
  cases.push({ name: 'p15-tocref-notoc', docx: D(paras.slice(5).join(''), { font: 'Arial', numbering: numInner }) });
  cases.push({ name: 'p15-tocref-notoc-ls', docx: D(paras.slice(5).join(''), { numbering: numInner }) });
}

// ── A16 ─────────────────────────────────────────────────────────────────────────────────────────────────────────
{
  const S = ({ type, start, more = '', refs = '', w = 11906, h = 16838, orient = '' } = {}) => `<w:sectPr>${refs}${type ? `<w:type w:val="${type}"/>` : ''}<w:pgSz w:w="${w}" w:h="${h}"${orient ? ` w:orient="${orient}"` : ''}/><w:pgMar w:top="1417" w:right="1701" w:bottom="1417" w:left="1701" w:header="708" w:footer="708" w:gutter="0"/>${start !== undefined ? `<w:pgNumType w:start="${start}"/>` : ''}<w:cols w:space="708"/>${more}<w:docGrid w:linePitch="360"/></w:sectPr>`;
  const hdr = (t) => p(r(`${t} `) + fld('PAGE', '1'));
  const two = (s1, s2, o = {}) => buildDocx({ body: p(r('S1 pagina Hxg')) + p(r('Fim S1 Hxg'), s1) + p(r('S2 texto Hxg')) + (o.more ?? ''), sect: s2, defaults: docDefaults({ font: LS, size: 11 }), ...o });
  const E = '<w:evenAndOddHeaders/>';
  const H = { default: hdr('Cab') }, HE = { default: hdr('Cab'), even: hdr('Par') };
  const F = { default: p(r('Rod ') + fld('PAGE', '1')) };
  // section 1 restarted at 2 (physical 1 shows 2): physical 2 vs continued 3 for the next page
  for (const [name, type, start, eo] of [
    ['p16-r2-odd-none', 'oddPage', undefined, false],
    ['p16-r2-odd-none-eo', 'oddPage', undefined, true],
    ['p16-r2-odd-s1', 'oddPage', 1, false],
    ['p16-r2-odd-s1-eo', 'oddPage', 1, true],
    ['p16-r2-even-none', 'evenPage', undefined, false],
    ['p16-r2-even-none-eo', 'evenPage', undefined, true],
    ['p16-r2-even-s2-eo', 'evenPage', 2, true],
    ['p16-r2-even-s1', 'evenPage', 1, false],
  ]) {
    cases.push({ name, docx: two(S({ start: 2 }), S({ type, start }), { headers: eo ? HE : H, footers: F, settings: eo ? E : '' }) });
  }
  // titlePg in section 2 and a blank page before it
  cases.push({ name: 'p16-titlepg', docx: two(S(), S({ type: 'oddPage', more: '<w:titlePg/>' }), { headers: { default: hdr('Cab'), first: hdr('Primeira') }, footers: F }) });
  cases.push({ name: 'p16-titlepg-r1', docx: two(S(), S({ type: 'oddPage', start: 1, more: '<w:titlePg/>' }), { headers: { default: hdr('Cab'), first: hdr('Primeira') }, footers: F, settings: E }) });
  // section 1 with its OWN header (S2 another): does the blank page show S1's?
  const s1refs = '<w:headerReference w:type="default" r:id="rId_header_s1"/><w:footerReference w:type="default" r:id="rId_footer_s1"/>';
  cases.push({ name: 'p16-s1hdr', docx: two(S({ refs: s1refs }), S({ type: 'oddPage' }), { headers: { default: hdr('Cab2'), s1: hdr('Cab1') }, footers: { default: p(r('Rod2 ') + fld('PAGE', '1')), s1: p(r('Rod1 ') + fld('PAGE', '1')) } }) });
  cases.push({ name: 'p16-s1hdr-eo', docx: two(S({ refs: s1refs }), S({ type: 'oddPage', start: 1 }), { headers: { default: hdr('Cab2'), even: hdr('Par2'), s1: hdr('Cab1') }, footers: { default: p(r('Rod2 ') + fld('PAGE', '1')), s1: p(r('Rod1 ') + fld('PAGE', '1')) }, settings: E }) });
  // NUMPAGES and SECTIONPAGES count the blank page?
  cases.push({ name: 'p16-numpages', docx: two(S(), S({ type: 'oddPage' }), { headers: { default: p(r('Cab ') + fld('PAGE', '1') + r(' de ') + fld('NUMPAGES', '1')) }, footers: { default: p(r('Secao ') + fld('SECTIONPAGES', '1')) } }) });
  cases.push({ name: 'p16-numpages-s1', docx: two(S({ refs: s1refs }), S({ type: 'oddPage' }), { headers: { default: p(r('Cab ') + fld('PAGE', '1') + r(' de ') + fld('NUMPAGES', '1')), s1: p(r('C1 ') + fld('PAGE', '1') + r(' de ') + fld('NUMPAGES', '1')) }, footers: { default: p(r('Secao ') + fld('SECTIONPAGES', '1')), s1: p(r('S1 ') + fld('SECTIONPAGES', '1')) } }) });
  // the blank page's size: section 2 landscape
  cases.push({ name: 'p16-size', docx: two(S(), S({ type: 'oddPage', w: 16838, h: 11906, orient: 'landscape' }), { headers: H }) });
  cases.push({ name: 'p16-size-eo', docx: two(S(), S({ type: 'oddPage', start: 1, w: 16838, h: 11906, orient: 'landscape' }), { headers: HE, settings: E }) });
  // section 1 of two pages, section 2 restarted at 2 on physical page 3
  cases.push({ name: 'p16-s1two-odd-s2', docx: buildDocx({ body: p(r('S1 pagina Hxg')) + p(r('S1 pagina dois Hxg'), '<w:pageBreakBefore/>' + S()) + p(r('S2 texto Hxg')), sect: S({ type: 'oddPage', start: 2 }), headers: H, defaults: docDefaults({ font: LS, size: 11 }) }) });
}

// ── gutter / mirror margins / gutterAtTop / rtlGutter / book fold / two on one ──────────────────────────────────────
{
  const text = (k) => p(r(lorem(70, 11 + k)), '<w:jc w:val="both"/>') + p(r(`direita ${k} Hxg`), '<w:jc w:val="right"/>') + p(r(`centro ${k} Hxg`), '<w:jc w:val="center"/>');
  const tbl = '<w:tbl><w:tblPr><w:tblStyle w:val="TableGrid"/><w:tblW w:w="5000" w:type="pct"/><w:tblLook w:val="04A0"/></w:tblPr><w:tblGrid><w:gridCol w:w="4000"/><w:gridCol w:w="4000"/></w:tblGrid>'
    + `<w:tr><w:tc><w:tcPr><w:tcW w:w="2500" w:type="pct"/></w:tcPr>${p(r('celula um Hxg'))}</w:tc><w:tc><w:tcPr><w:tcW w:w="2500" w:type="pct"/></w:tcPr>${p(r('celula dois Hxg'))}</w:tc></w:tr></w:tbl>`;
  const pages = (n) => Array.from({ length: n }, (_, k) => (k === 0 ? '' : p(r(`pagina ${k + 1} Hxg`), '<w:pageBreakBefore/>')) + text(k + 1) + tbl + p('')).join('');
  const hf = { headers: { default: p(r('Cab esquerda Hxg')) + p(r('Cab direita Hxg'), '<w:jc w:val="right"/>') }, footers: { default: p(r('Rod ') + fld('PAGE', '1'), '<w:jc w:val="center"/>') } };
  const S = ({ gut = 1440, more = '', w = 11906, h = 16838, orient = '', start, cols = '<w:cols w:space="708"/>', left = 1701, right = 1134 } = {}) => `<w:sectPr><w:pgSz w:w="${w}" w:h="${h}"${orient ? ` w:orient="${orient}"` : ''}/><w:pgMar w:top="1417" w:right="${right}" w:bottom="1134" w:left="${left}" w:header="708" w:footer="708" w:gutter="${gut}"/>${start !== undefined ? `<w:pgNumType w:start="${start}"/>` : ''}${cols}${more}<w:docGrid w:linePitch="360"/></w:sectPr>`;
  // settings in schema order: mirrorMargins, gutterAtTop … defaultTabStop … bookFoldRevPrinting, bookFoldPrinting
  const set = ({ mirror = false, top = false, fold = '', two = false } = {}) => W_SETTINGS(`${mirror ? '<w:mirrorMargins/>' : ''}${top ? '<w:gutterAtTop/>' : ''}`, fold);
  const twoOnOne = (s) => s.replace('<w:compat>', '<w:printTwoOnOne/><w:compat>');
  const G = (name, sect, s, n = 3) => cases.push({ name, docx: D(pages(n), { sect, rawSettings: s, ...hf }) });
  G('g-none', S({ gut: 0 }), set());
  G('g-gut', S(), set());
  G('g-gut-mirror', S(), set({ mirror: true }));
  G('g-mirror', S({ gut: 0 }), set({ mirror: true }));
  G('g-gut-top', S(), set({ top: true }));
  G('g-gut-top-mirror', S(), set({ top: true, mirror: true }));
  G('g-gut-rtl', S({ more: '<w:rtlGutter/>' }), set());
  G('g-gut-rtl-mirror', S({ more: '<w:rtlGutter/>' }), set({ mirror: true }));
  G('g-mirror-restart', S({ gut: 0, start: 2 }), set({ mirror: true }));
  G('g-gut-mirror-restart', S({ start: 2 }), set({ mirror: true }));
  G('g-fold-port', S({ gut: 0 }), set({ fold: '<w:bookFoldPrinting/>' }));
  G('g-fold-land', S({ gut: 0, w: 16838, h: 11906, orient: 'landscape' }), set({ fold: '<w:bookFoldPrinting/>' }));
  G('g-fold-land-gut', S({ w: 16838, h: 11906, orient: 'landscape' }), set({ fold: '<w:bookFoldPrinting/>' }));
  G('g-fold-rev-land', S({ gut: 0, w: 16838, h: 11906, orient: 'landscape' }), set({ fold: '<w:bookFoldRevPrinting/>' }));
  G('g-two-port', S({ gut: 0 }), twoOnOne(set()));
  G('g-two-port-gut', S(), twoOnOne(set()));
  // two columns
  const colsBody = filler(130, 'coluna');
  cases.push({ name: 'g-gut-cols', docx: D(colsBody, { sect: S({ cols: '<w:cols w:num="2" w:space="708"/>' }), rawSettings: set({ mirror: true }) }) });
  cases.push({ name: 'g-gut-cols-nomirror', docx: D(colsBody, { sect: S({ cols: '<w:cols w:num="2" w:space="708"/>' }), rawSettings: set() }) });
  cases.push({ name: 'g-gut-cols-uneq', docx: D(colsBody, { sect: S({ cols: '<w:cols w:num="2" w:space="400" w:equalWidth="0"><w:col w:w="3000" w:space="400"/><w:col w:w="4171"/></w:cols>' }), rawSettings: set({ mirror: true }) }) });
  // floats: margin/page/inside relative, on odd and even pages (wrapNone, page-relative y)
  const boxes = (k, i0) => [
    tbx({ i: i0, x: 0, y: 300, w: 60, h: 20, relH: 'margin', relV: 'page', content: p(r('M Hxg')) }),
    tbx({ i: i0 + 1, x: 100, y: 340, w: 60, h: 20, relH: 'page', relV: 'page', content: p(r('P Hxg')) }),
    tbx({ i: i0 + 2, alignH: 'inside', y: 380, w: 60, h: 20, relH: 'margin', relV: 'page', content: p(r('IM Hxg')) }),
    tbx({ i: i0 + 3, alignH: 'inside', y: 420, w: 60, h: 20, relH: 'page', relV: 'page', content: p(r('IP Hxg')) }),
    tbx({ i: i0 + 4, alignH: 'left', y: 460, w: 60, h: 20, relH: 'leftMargin', relV: 'page', content: p(r('LM Hxg')) }),
    tbx({ i: i0 + 5, alignH: 'right', y: 500, w: 60, h: 20, relH: 'margin', relV: 'page', content: p(r('RM Hxg')) }),
    tbx({ i: i0 + 6, x: 0, y: 540, w: 60, h: 20, relH: 'insideMargin', relV: 'page', content: p(r('IN Hxg')) }),
  ].join('') + r(`ancora ${k} Hxg`);
  const fb = p(boxes(1, 1)) + p(boxes(2, 11), '<w:pageBreakBefore/>') + p(boxes(3, 21), '<w:pageBreakBefore/>');
  cases.push({ name: 'g-gut-float', docx: D(fb, { sect: S(), rawSettings: set({ mirror: true }) }) });
  cases.push({ name: 'g-gut-float-nomirror', docx: D(fb, { sect: S(), rawSettings: set() }) });
  cases.push({ name: 'g-gut-float-top', docx: D(fb, { sect: S(), rawSettings: set({ top: true }) }) });
  // page borders measured from the text
  const bdr = '<w:pgBorders w:offsetFrom="text"><w:top w:val="single" w:sz="4" w:space="4" w:color="auto"/><w:left w:val="single" w:sz="4" w:space="4" w:color="auto"/><w:bottom w:val="single" w:sz="4" w:space="4" w:color="auto"/><w:right w:val="single" w:sz="4" w:space="4" w:color="auto"/></w:pgBorders>';
  cases.push({ name: 'g-gut-pgbdr', docx: D(pages(2), { sect: S({ more: '' }).replace('<w:cols', bdr + '<w:cols'), rawSettings: set({ mirror: true }), ...hf }) });
  cases.push({ name: 'g-gut-pgbdr-page', docx: D(pages(2), { sect: S().replace('<w:cols', bdr.replace('offsetFrom="text"', 'offsetFrom="page"').replaceAll('w:space="4"', 'w:space="24"') + '<w:cols'), rawSettings: set({ mirror: true }), ...hf }) });
}

// ── mirrorIndents ───────────────────────────────────────────────────────────────────────────────────────────────
{
  const MI = '<w:mirrorIndents/>';
  const long = Array.from({ length: 12 }, (_, k) => `espelho ${k + 1} Hxg`).join('\n');
  cases.push({ name: 'mi-span', docx: D(filler(50) + p(r(long), MI + '<w:ind w:left="2000" w:right="400"/>') + filler(3, 'depois')) });
  const both = (body) => body + p(r('quebra Hxg'), '<w:pageBreakBefore/>') + body;
  const hang = p(r(lorem(40, 3)), MI + '<w:ind w:left="1440" w:hanging="720"/>') + p(r(lorem(40, 4)), MI + '<w:ind w:left="400" w:right="2000" w:firstLine="720"/>')
    + p(r(`direita Hxg`), MI + '<w:jc w:val="right"/><w:ind w:left="2000" w:right="400"/>') + p(r('a\tb Hxg'), MI + '<w:tabs><w:tab w:val="left" w:pos="3000"/></w:tabs><w:ind w:left="2000" w:right="400"/>')
    + p(r('c\td Hxg'), MI + '<w:ind w:left="2000" w:right="400"/>') + p(r(lorem(40, 5)), MI + '<w:jc w:val="both"/><w:ind w:left="2000" w:right="400"/>');
  cases.push({ name: 'mi-forms', docx: D(both(hang)) });
  const numbering = '<w:abstractNum w:abstractNumId="1"><w:multiLevelType w:val="hybridMultilevel"/><w:lvl w:ilvl="0"><w:start w:val="1"/><w:numFmt w:val="decimal"/><w:lvlText w:val="%1."/><w:lvlJc w:val="left"/><w:pPr><w:ind w:left="720" w:hanging="360"/></w:pPr></w:lvl></w:abstractNum><w:num w:numId="1"><w:abstractNumId w:val="1"/></w:num>';
  const items = Array.from({ length: 3 }, (_, k) => p(r(`item ${k + 1} ` + lorem(25, 7 + k)), MI + '<w:numPr><w:ilvl w:val="0"/><w:numId w:val="1"/></w:numPr>')).join('');
  cases.push({ name: 'mi-num', docx: D(both(items), { numbering }) });
  const simple = p(r(lorem(40, 9)), MI + '<w:ind w:left="2000" w:right="400"/>');
  const Sx = (more, mar = 'w:right="1134" w:left="1701"') => `<w:sectPr><w:pgSz w:w="11906" w:h="16838"/><w:pgMar w:top="1417" ${mar} w:bottom="1417" w:header="708" w:footer="708" w:gutter="0"/>${more}<w:cols w:space="708"/><w:docGrid w:linePitch="360"/></w:sectPr>`;
  cases.push({ name: 'mi-restart', docx: D(both(simple), { sect: Sx('<w:pgNumType w:start="2"/>') }) });
  cases.push({ name: 'mi-mirror', docx: D(both(simple), { sect: Sx(''), rawSettings: W_SETTINGS('<w:mirrorMargins/>') }) });
  cases.push({ name: 'mi-mirror-restart', docx: D(both(simple), { sect: Sx('<w:pgNumType w:start="2"/>'), rawSettings: W_SETTINGS('<w:mirrorMargins/>') }) });
  cases.push({ name: 'mi-c14', docx: D(both(simple), { compat: 14 }) });
  // in a table cell on an even page
  const cell = `<w:tbl><w:tblPr><w:tblStyle w:val="TableGrid"/><w:tblW w:w="6000" w:type="dxa"/><w:tblLook w:val="04A0"/></w:tblPr><w:tblGrid><w:gridCol w:w="6000"/></w:tblGrid><w:tr><w:tc><w:tcPr><w:tcW w:w="6000" w:type="dxa"/></w:tcPr>${p(r('celula ' + lorem(20, 2)), MI + '<w:ind w:left="1000" w:right="200"/>')}</w:tc></w:tr></w:tbl>`;
  cases.push({ name: 'mi-cell', docx: D(both(cell + p(''))) });
}

// ── B10 / B11 additions ─────────────────────────────────────────────────────────────────────────────────────────
{
  const box = (o) => tbx({ i: 1, ins: [0, 0, 0, 0], content: p(r('Caixa Hxg')), wrap: 'wrapSquare', ...o });
  const lines6 = Array.from({ length: 6 }, (_, k) => `linha ${k + 1} Hxg`);
  const para6 = (a) => p(lines6.map((t, k) => r(t + (k < 5 ? '\n' : ''))).join('') + a);
  // line-relative, anchor at the end of a 6-line paragraph on the SAME page
  cases.push({ name: 'b10x-line-same', docx: D(filler(5) + para6(box({ x: 250, y: 0, w: 120, h: 40, relH: 'margin', relV: 'line' })) + filler(6, 'depois')) });
  cases.push({ name: 'b10x-line-same-20', docx: D(filler(5) + para6(box({ x: 250, y: 20, w: 120, h: 40, relH: 'margin', relV: 'line' })) + filler(6, 'depois')) });
  cases.push({ name: 'b10x-para-same', docx: D(filler(5) + para6(box({ x: 250, y: 0, w: 120, h: 40, relH: 'margin', relV: 'paragraph' })) + filler(6, 'depois')) });
  // a paragraph crossing from column 1 to column 2, the anchor at its end (column-relative x)
  const long = Array.from({ length: 12 }, (_, k) => `longa ${k + 1} Hxg`);
  const para12 = (a) => p(long.map((t, k) => r(t + (k < 11 ? '\n' : ''))).join('') + a);
  const cols2 = '<w:sectPr><w:pgSz w:w="11906" w:h="16838"/><w:pgMar w:top="1417" w:right="1701" w:bottom="1417" w:left="1701" w:header="708" w:footer="708" w:gutter="0"/><w:cols w:num="2" w:space="708"/><w:docGrid w:linePitch="360"/></w:sectPr>';
  cases.push({ name: 'b10x-col', docx: D(filler(50) + para12(box({ x: 20, y: 0, w: 120, h: 40, relH: 'column', relV: 'paragraph' })) + filler(6, 'depois'), { sect: cols2 }) });
  cases.push({ name: 'b10x-col-line', docx: D(filler(50) + para12(box({ x: 20, y: 0, w: 120, h: 40, relH: 'column', relV: 'line' })) + filler(6, 'depois'), { sect: cols2 }) });
  // B11: line-relative negative offset; the re-wrap pushes the anchor paragraph to the next page
  const nine = (a, k0 = 5) => Array.from({ length: 9 }, (_, k) => p((k === k0 ? a : '') + r(lorem(40, 3 + k)))).join('');
  cases.push({ name: 'b11x-line-60', docx: D(filler(2) + nine(box({ x: 0, y: -60, w: 200, h: 40, relH: 'margin', relV: 'line' }))) });
  cases.push({ name: 'b11x-push', docx: D(filler(31) + nine(box({ x: 0, y: -60, w: 200, h: 40, relH: 'margin', relV: 'paragraph' }))) });
  cases.push({ name: 'b11x-push2', docx: D(filler(33) + nine(box({ x: 0, y: -60, w: 200, h: 40, relH: 'margin', relV: 'paragraph' }))) });
  // two floats: the second one's re-wrap moves the first one's anchor (order of re-layouts)
  cases.push({ name: 'b11x-two', docx: D(filler(2) + Array.from({ length: 9 }, (_, k) => p((k === 4 ? box({ x: 0, y: -40, w: 150, h: 30, relH: 'margin', relV: 'paragraph' }) : k === 7 ? tbx({ i: 2, ins: [0, 0, 0, 0], content: p(r('Outra Hxg')), wrap: 'wrapSquare', x: 250, y: -80, w: 150, h: 30, relH: 'margin', relV: 'paragraph' }) : '') + r(lorem(40, 3 + k)))).join('')) });
}
