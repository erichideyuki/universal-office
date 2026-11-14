// WordCraft study (2026-11-13), the "page" planner, fourth series: wcpage3 found that with mirrorMargins a nextPage
// section that restarts its numbering at the wrong parity gets a BLANK page before it, and that mirrorIndents follows
// the "document" parity (first section's start + physical index). Open here: (1) evenAndOddHeaders WITHOUT mirror
// margins — blank pages at a restart? which header (displayed number vs physical)? (2) A16's continued number after a
// MIDDLE section restarted (previous displayed + 1, or document parity?); (3) mirror margins with an odd/even break.
// Run with --fixdir core/target/wordcraft-study/page/wcpage4 (rust-core worktree).
import { buildDocx, docDefaults, p, r, lorem } from '../docx-builder.mjs';

const LS = 'Liberation Sans';
const fld = (instr, shown) => `<w:r><w:fldChar w:fldCharType="begin"/></w:r><w:r><w:instrText xml:space="preserve"> ${instr} </w:instrText></w:r><w:r><w:fldChar w:fldCharType="separate"/></w:r><w:r><w:t>${shown}</w:t></w:r><w:r><w:fldChar w:fldCharType="end"/></w:r>`;
const SET = (pre, post = '') => `${pre}<w:defaultTabStop w:val="708"/>${post}<w:characterSpacingControl w:val="doNotCompress"/><w:compat><w:compatSetting w:name="compatibilityMode" w:uri="http://schemas.microsoft.com/office/word" w:val="15"/></w:compat>`;
const MI = p(r(lorem(30, 5)), '<w:mirrorIndents/><w:ind w:left="2000" w:right="400"/>');
const REFS = '<w:headerReference w:type="default" r:id="rId_header_default"/><w:headerReference w:type="even" r:id="rId_header_even"/>';
const S = ({ start, type, refs = '' } = {}) => `<w:sectPr>${refs}${type ? `<w:type w:val="${type}"/>` : ''}<w:pgSz w:w="11906" w:h="16838"/><w:pgMar w:top="1417" w:right="1134" w:bottom="1417" w:left="1701" w:header="708" w:footer="708" w:gutter="0"/>${start !== undefined ? `<w:pgNumType w:start="${start}"/>` : ''}<w:cols w:space="708"/><w:docGrid w:linePitch="360"/></w:sectPr>`;

// secs: [{ n: pages, start, type }]; o: { eo, mm }
const doc = (secs, o = {}) => {
  let body = '', k = 0;
  secs.forEach((s, si) => {
    for (let j = 0; j < s.n; j++) {
      k++;
      body += p(r(`pagina ${k} Hxg`), j > 0 ? '<w:pageBreakBefore/>' : '');
      const next = secs[si + 1];
      body += j === s.n - 1 && next ? MI.replace('</w:pPr>', `${S({ start: s.start, type: s.type, refs: REFS })}</w:pPr>`) : MI;
    }
  });
  const last = secs[secs.length - 1];
  return buildDocx({ body, sect: S({ start: last.start, type: last.type }), headers: { default: p(r('Cab ') + fld('PAGE', '1')), even: p(r('Par ') + fld('PAGE', '1')) },
    rawSettings: SET(o.mm ? '<w:mirrorMargins/>' : '', o.eo ? '<w:evenAndOddHeaders/>' : ''), defaults: docDefaults({ font: LS, size: 11 }) });
};

export const cases = [];
const add = (name, secs) => {
  cases.push({ name, docx: doc(secs) });
  cases.push({ name: `${name}-eo`, docx: doc(secs, { eo: true }) });
  cases.push({ name: `${name}-mm`, docx: doc(secs, { mm: true }) });
  cases.push({ name: `${name}-eo-mm`, docx: doc(secs, { eo: true, mm: true }) });
};
add('w-r1', [{ n: 1 }, { n: 2, start: 1 }]); // physical 2 = number 1
add('w-r2-s1r2', [{ n: 1, start: 2 }, { n: 2 }]); // first section restarted at 2, then continued
add('w-mid5-odd', [{ n: 1 }, { n: 1, start: 5 }, { n: 1, type: 'oddPage' }]); // previous shown 5 (+1 = 6 even) vs document 3
add('w-mid4-odd', [{ n: 1 }, { n: 1, start: 4 }, { n: 1, type: 'oddPage' }]); // previous shown 4 (+1 = 5 odd) vs document 3
add('w-mid5-even-r2', [{ n: 1 }, { n: 1, start: 5 }, { n: 1, type: 'evenPage', start: 2 }]);
add('w-odd-s2', [{ n: 1 }, { n: 1, type: 'oddPage', start: 2 }]);
add('w-cont-r1', [{ n: 1 }, { n: 2, start: 1, type: 'continuous' }]); // a continuous section that restarts
