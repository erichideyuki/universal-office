// WordCraft study (2026-11-13), the "page" planner, third series: (1) the frozen corpus's documents that the A15/A16
// rules touch, re-measured with today's Word (16.0.20527) next to their 2026-10 .word.json; (2) the page parity that
// mirrorIndents / mirrorMargins / even headers use across SECTIONS with restarted numbering (wcpage2 mi2-sect-restart
// mirrored page 2 = number 1, while mi-restart follows the number); (3) printTwoOnOne on a landscape sheet with a gutter.
// Run with --fixdir core/target/wordcraft-study/page/wcpage3 (rust-core worktree).
import { readFileSync } from 'node:fs';
import { buildDocx, docDefaults, p, r, lorem } from '../docx-builder.mjs';

const LS = 'Liberation Sans';
const fld = (instr, shown) => `<w:r><w:fldChar w:fldCharType="begin"/></w:r><w:r><w:instrText xml:space="preserve"> ${instr} </w:instrText></w:r><w:r><w:fldChar w:fldCharType="separate"/></w:r><w:r><w:t>${shown}</w:t></w:r><w:r><w:fldChar w:fldCharType="end"/></w:r>`;
const SET = (pre, post = '', compat = 15, tail = '') => `${pre}<w:defaultTabStop w:val="708"/>${post}<w:characterSpacingControl w:val="doNotCompress"/>${tail}<w:compat><w:compatSetting w:name="compatibilityMode" w:uri="http://schemas.microsoft.com/office/word" w:val="${compat}"/></w:compat>`;

export const cases = [];

// (1) corpus documents (bytes as versioned)
for (const f of ['brfirst/bf-page-start', 'brfirst/bf-page-start-hang', 'numpgbr/lead-14', 'numpgbr/lead-15', 'pbfit/pf-break-text', 'pbfit/pf14-break-text',
  'tocref/tocref-sectgap', 'tocref/tocref-sectgap-soma', 'sections/sb-odd', 'sections/sb-even', 'sections/sb-odd-2pg', 'sections/sb-even-2pg']) {
  cases.push({ name: `c-${f.split('/')[1]}`, docx: new Uint8Array(readFileSync(new URL(`../../../packages/docx-layout/test/oracle/${f}.docx`, import.meta.url))) });
}

// (2) parity across sections: each page has a mirrorIndents paragraph (left 2000, right 400: normal x 85.1+100 on a
// 1701-margin page) and the header shows PAGE; mirror margins (left 1701, right 1134) where named
{
  const MI = p(r(lorem(30, 5)), '<w:mirrorIndents/><w:ind w:left="2000" w:right="400"/>');
  const S = ({ start, type } = {}) => `<w:sectPr>${type ? `<w:type w:val="${type}"/>` : ''}<w:pgSz w:w="11906" w:h="16838"/><w:pgMar w:top="1417" w:right="1134" w:bottom="1417" w:left="1701" w:header="708" w:footer="708" w:gutter="0"/>${start !== undefined ? `<w:pgNumType w:start="${start}"/>` : ''}<w:cols w:space="708"/><w:docGrid w:linePitch="360"/></w:sectPr>`;
  // sections: list of [pages, start]
  const doc = (secs, o = {}) => {
    let body = '', k = 0;
    secs.forEach(([n, start], si) => {
      for (let j = 0; j < n; j++) {
        k++;
        const brk = k > 1 && j > 0 ? '<w:pageBreakBefore/>' : '';
        body += p(r(`pagina ${k} Hxg`), brk) + (j === n - 1 && si < secs.length - 1 ? MI.replace('</w:pPr>', `${S({ start }).replace('<w:sectPr>', '<w:sectPr><w:headerReference w:type="default" r:id="rId_header_default"/>')}</w:pPr>`) : MI);
      }
    });
    // the sectPr of each section but the last closes its last paragraph's pPr
    return buildDocx({ body, sect: S({ start: secs[secs.length - 1][1] }), headers: o.eo ? { default: p(r('Cab ') + fld('PAGE', '1')), even: p(r('Par ') + fld('PAGE', '1')) } : { default: p(r('Cab ') + fld('PAGE', '1')) },
      rawSettings: SET(o.mm ? '<w:mirrorMargins/>' : '', '', o.compat ?? 15, o.eo ? '' : ''), settings: '', defaults: docDefaults({ font: LS, size: 11 }), compat: o.compat ?? 15 });
  };
  const variants = [
    ['ms-r1', [[1, undefined], [2, 1]]], // physical 2,3 = numbers 1,2
    ['ms-r2', [[1, undefined], [2, 2]]], // physical 2,3 = numbers 2,3
    ['ms-2r2', [[2, undefined], [2, 2]]], // physical 3,4 = numbers 2,3
    ['ms-s2cont', [[1, 2], [2, undefined]]], // physical 1 = 2; physical 2,3 = 3,4 (continued)
    ['ms-r1r1', [[1, 1], [1, 1], [1, 1]]], // every page number 1
  ];
  for (const [name, secs] of variants) {
    cases.push({ name, docx: doc(secs) });
    cases.push({ name: `${name}-mm`, docx: doc(secs, { mm: true }) });
    cases.push({ name: `${name}-c14`, docx: doc(secs, { compat: 14 }) });
  }
}

// (3) printTwoOnOne, landscape sheet, with a gutter (wcpage2 g2-two-land had none)
{
  const S = `<w:sectPr><w:pgSz w:w="16838" w:h="11906" w:orient="landscape"/><w:pgMar w:top="1417" w:right="1134" w:bottom="1134" w:left="1701" w:header="708" w:footer="708" w:gutter="1440"/><w:cols w:space="708"/><w:docGrid w:linePitch="360"/></w:sectPr>`;
  const body = Array.from({ length: 80 }, (_, k) => p(r(`linha ${k + 1} Hxg`))).join('');
  cases.push({ name: 'g3-two-land-gut', docx: buildDocx({ body, sect: S, rawSettings: SET('', '', 15, '<w:printTwoOnOne/>'), headers: { default: p(r('Cab esquerda Hxg')) }, defaults: docDefaults({ font: LS, size: 11 }) }) });
  cases.push({ name: 'g3-fold-land-gut-mm', docx: buildDocx({ body, sect: S, rawSettings: SET('<w:mirrorMargins/>', '<w:bookFoldPrinting/>'), headers: { default: p(r('Cab esquerda Hxg')) }, defaults: docDefaults({ font: LS, size: 11 }) }) });
}
