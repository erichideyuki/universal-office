// WordCraft study (2026-11-13), the "page" planner, fifth series: mirrorIndents outside the body flow (header, text box,
// footnote), mirrorIndents when gutterAtTop disables mirror margins, and the evenAndOddHeaders / mirrorMargins blank page
// at a restarted section in modes 14 and 12 (wcpage4 measured mode 15). Run with --fixdir
// core/target/wordcraft-study/page/wcpage5 (rust-core worktree).
import { strToU8 } from 'fflate';
import { buildDocx, docDefaults, p, r, lorem } from '../docx-builder.mjs';
import { tbx } from './_tbx.mjs';

const LS = 'Liberation Sans';
const fld = (instr, shown) => `<w:r><w:fldChar w:fldCharType="begin"/></w:r><w:r><w:instrText xml:space="preserve"> ${instr} </w:instrText></w:r><w:r><w:fldChar w:fldCharType="separate"/></w:r><w:r><w:t>${shown}</w:t></w:r><w:r><w:fldChar w:fldCharType="end"/></w:r>`;
const SET = (pre, post = '', compat = 15) => `${pre}<w:defaultTabStop w:val="708"/>${post}<w:characterSpacingControl w:val="doNotCompress"/><w:compat><w:compatSetting w:name="compatibilityMode" w:uri="http://schemas.microsoft.com/office/word" w:val="${compat}"/></w:compat>`;
const MIP = '<w:mirrorIndents/><w:ind w:left="2000" w:right="400"/>';
const D = (body, o = {}) => buildDocx({ body, defaults: docDefaults({ font: LS, size: 11 }), ...o });
const W_NS = 'xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships"';

export const cases = [];

// header, text box and footnote paragraphs with mirrorIndents, on page 1 (odd) and page 2 (even)
{
  const body = (k) => p(r(`pagina ${k} Hxg`), k > 1 ? '<w:pageBreakBefore/>' : '') + p(r(lorem(30, 5)), MIP);
  cases.push({ name: 'mi5-hdr', docx: D(body(1) + body(2), { headers: { default: p(r('cabecalho espelho Hxg'), MIP) }, footers: { default: p(r('rodape espelho Hxg'), MIP) } }) });
  const box = (i) => tbx({ i, x: 0, y: 300, w: 300, h: 60, relH: 'margin', relV: 'page', content: p(r('caixa espelho Hxg'), MIP) });
  cases.push({ name: 'mi5-txbx', docx: D(p(box(1) + r('pagina 1 Hxg')) + p(box(2) + r('pagina 2 Hxg'), '<w:pageBreakBefore/>')) });
  const fnRef = (id) => `<w:r><w:rPr><w:vertAlign w:val="superscript"/></w:rPr><w:footnoteReference w:id="${id}"/></w:r>`;
  const notes = `<?xml version="1.0" encoding="UTF-8" standalone="yes"?><w:footnotes ${W_NS}>`
    + '<w:footnote w:type="separator" w:id="-1"><w:p><w:pPr><w:spacing w:after="0" w:line="240" w:lineRule="auto"/></w:pPr><w:r><w:separator/></w:r></w:p></w:footnote>'
    + '<w:footnote w:type="continuationSeparator" w:id="0"><w:p><w:pPr><w:spacing w:after="0" w:line="240" w:lineRule="auto"/></w:pPr><w:r><w:continuationSeparator/></w:r></w:p></w:footnote>'
    + [1, 2].map((id) => `<w:footnote w:id="${id}"><w:p><w:pPr>${MIP}</w:pPr><w:r><w:rPr><w:vertAlign w:val="superscript"/></w:rPr><w:footnoteRef/></w:r><w:r><w:t xml:space="preserve"> nota espelho ${id} Hxg</w:t></w:r></w:p></w:footnote>`).join('')
    + '</w:footnotes>';
  cases.push({ name: 'mi5-fn', docx: D(p(r('pagina 1 Hxg') + fnRef(1)) + p(r('pagina 2 Hxg') + fnRef(2), '<w:pageBreakBefore/>'), { extraParts: { 'word/footnotes.xml': strToU8(notes) } }) });
  // gutterAtTop turns mirror margins off: and mirrorIndents?
  const sect = '<w:sectPr><w:pgSz w:w="11906" w:h="16838"/><w:pgMar w:top="1417" w:right="1134" w:bottom="1417" w:left="1701" w:header="708" w:footer="708" w:gutter="720"/><w:cols w:space="708"/><w:docGrid w:linePitch="360"/></w:sectPr>';
  cases.push({ name: 'mi5-topmm', docx: D(body(1) + body(2), { sect, rawSettings: SET('<w:mirrorMargins/><w:gutterAtTop/>') }) });
}

// the eo / mm blank page at a restarted section, modes 14 and 12
{
  const REFS = '<w:headerReference w:type="default" r:id="rId_header_default"/><w:headerReference w:type="even" r:id="rId_header_even"/>';
  const S = ({ start, refs = '' } = {}) => `<w:sectPr>${refs}<w:pgSz w:w="11906" w:h="16838"/><w:pgMar w:top="1417" w:right="1134" w:bottom="1417" w:left="1701" w:header="708" w:footer="708" w:gutter="0"/>${start !== undefined ? `<w:pgNumType w:start="${start}"/>` : ''}<w:cols w:space="708"/><w:docGrid w:linePitch="360"/></w:sectPr>`;
  const body = p(r('pagina 1 Hxg')) + p(r(lorem(30, 5)), MIP + S({ refs: REFS })) + p(r('pagina 2 Hxg')) + p(r(lorem(30, 5)), MIP) + p(r('pagina 3 Hxg'), '<w:pageBreakBefore/>') + p(r(lorem(30, 5)), MIP);
  for (const c of [14, 12]) {
    for (const [tag, pre, post] of [['eo', '', '<w:evenAndOddHeaders/>'], ['mm', '<w:mirrorMargins/>', '']]) {
      cases.push({ name: `w5-r1-${tag}-c${c}`, docx: D(body, { sect: S({ start: 1 }), headers: { default: p(r('Cab ') + fld('PAGE', '1')), even: p(r('Par ') + fld('PAGE', '1')) }, rawSettings: SET(pre, post, c) }) });
    }
  }
}

// inside/outside float alignment on a page whose number (1, restarted) and document parity (2) differ, no eo/mm
{
  const REFS = '<w:headerReference w:type="default" r:id="rId_header_default"/>';
  const S = ({ start, refs = '' } = {}) => `<w:sectPr>${refs}<w:pgSz w:w="11906" w:h="16838"/><w:pgMar w:top="1417" w:right="1134" w:bottom="1417" w:left="1701" w:header="708" w:footer="708" w:gutter="0"/>${start !== undefined ? `<w:pgNumType w:start="${start}"/>` : ''}<w:cols w:space="708"/><w:docGrid w:linePitch="360"/></w:sectPr>`;
  const box = (i) => tbx({ i, alignH: 'inside', y: 400, w: 60, h: 20, relH: 'margin', relV: 'page', content: p(r('IM Hxg')) })
    + tbx({ i: i + 1, x: 0, y: 440, w: 60, h: 20, relH: 'insideMargin', relV: 'page', content: p(r('IN Hxg')) });
  const body = p(box(1) + r('pagina 1 Hxg')) + p(r(lorem(30, 5)), MIP + S({ refs: REFS })) + p(box(3) + r('pagina 2 Hxg')) + p(r(lorem(30, 5)), MIP);
  cases.push({ name: 'f5-inside-r1', docx: D(body, { sect: S({ start: 1 }), headers: { default: p(r('Cab ') + fld('PAGE', '1')) } }) });
}

// A15: the document's FIRST paragraph starts with a page break (before 480), modes 15 and 14
for (const c of [15, 14]) {
  cases.push({ name: `p5-first-br-c${c}`, docx: D(p('<w:r><w:br w:type="page"/></w:r>' + r('Primeiro Hxg'), '<w:spacing w:before="480" w:after="0"/>') + p(r('Segundo Hxg')), { compat: c }) });
}
