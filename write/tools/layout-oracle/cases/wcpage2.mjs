// WordCraft study (2026-11-13), the "page" planner, second series (after wcpage.mjs): the open points of the first one.
// Run with --fixdir core/target/wordcraft-study/page/wcpage2 (rust-core worktree).
//  g2-*   gutterAtTop without a gutter + mirror; body bottom under gutterAtTop / printTwoOnOne / bookFold; bookFold with
//         mirrorMargins, portrait + gutter; printTwoOnOne on a landscape sheet; mirror margins and gutter in mode 14
//  mi2-*  mirrorIndents in modes 14 and 12 (the first series' mi-c14 mirrored on the ODD page)
//  p2-*   A15/A16 in modes 14/12 (the frozen corpus's tocref-pbstart-c11/c14 bytes re-measured; odd/even breaks)
//  bx-*   B10/B11 in mode 14
// Liberation Sans 11 / A4; invented texts.
import { readFileSync } from 'node:fs';
import { buildDocx, docDefaults, p, r, lorem } from '../docx-builder.mjs';
import { tbx } from './_tbx.mjs';

const LS = 'Liberation Sans';
const D = (body, o = {}) => buildDocx({ body, defaults: docDefaults({ font: LS, size: 11 }), ...o });
const fld = (instr, shown) => `<w:r><w:fldChar w:fldCharType="begin"/></w:r><w:r><w:instrText xml:space="preserve"> ${instr} </w:instrText></w:r><w:r><w:fldChar w:fldCharType="separate"/></w:r><w:r><w:t>${shown}</w:t></w:r><w:r><w:fldChar w:fldCharType="end"/></w:r>`;
const filler = (n, w = 'enchimento') => Array.from({ length: n }, (_, k) => p(r(`${w} ${k + 1} Hxg`))).join('');
const SET = (pre, post = '', compat = 15, tail = '') => `${pre}<w:defaultTabStop w:val="708"/>${post}<w:characterSpacingControl w:val="doNotCompress"/>${tail}<w:compat><w:compatSetting w:name="compatibilityMode" w:uri="http://schemas.microsoft.com/office/word" w:val="${compat}"/></w:compat>`;

export const cases = [];

// ── page geometry ────────────────────────────────────────────────────────────────────────────────────────────────
{
  const S = ({ gut = 1440, w = 11906, h = 16838, orient = '', more = '' } = {}) => `<w:sectPr><w:pgSz w:w="${w}" w:h="${h}"${orient ? ` w:orient="${orient}"` : ''}/><w:pgMar w:top="1417" w:right="1134" w:bottom="1134" w:left="1701" w:header="708" w:footer="708" w:gutter="${gut}"/>${more}<w:cols w:space="708"/><w:docGrid w:linePitch="360"/></w:sectPr>`;
  // 130 one-line paragraphs: the last line of each page shows the body bottom; page 2 = even
  const long = filler(130, 'linha');
  const hf = { headers: { default: p(r('Cab esquerda Hxg')) + p(r('Cab direita Hxg'), '<w:jc w:val="right"/>') }, footers: { default: p(r('Rod ') + fld('PAGE', '1'), '<w:jc w:val="center"/>') } };
  const G = (name, sect, settings, o = {}) => cases.push({ name, docx: D(o.body ?? long, { sect, rawSettings: settings, ...hf, ...o }) });
  G('g2-none-long', S({ gut: 0 }), SET(''));
  G('g2-top0-mirror', S({ gut: 0 }), SET('<w:mirrorMargins/><w:gutterAtTop/>'));
  G('g2-gut-top-long', S(), SET('<w:gutterAtTop/>'));
  G('g2-gut-top-rtl', S({ more: '<w:rtlGutter/>' }), SET('<w:gutterAtTop/>'));
  G('g2-two-long', S({ gut: 0 }), SET('', '', 15, '<w:printTwoOnOne/>'));
  G('g2-two-long-gut', S(), SET('', '', 15, '<w:printTwoOnOne/>'));
  G('g2-two-land', S({ gut: 0, w: 16838, h: 11906, orient: 'landscape' }), SET('', '', 15, '<w:printTwoOnOne/>'));
  G('g2-two-mirror', S({ gut: 0 }), SET('<w:mirrorMargins/>', '', 15, '<w:printTwoOnOne/>'));
  G('g2-fold-land-long', S({ gut: 0, w: 16838, h: 11906, orient: 'landscape' }), SET('', '<w:bookFoldPrinting/>'));
  G('g2-fold-land-mirror', S({ gut: 0, w: 16838, h: 11906, orient: 'landscape' }), SET('<w:mirrorMargins/>', '<w:bookFoldPrinting/>'));
  G('g2-fold-land-sheets', S({ gut: 0, w: 16838, h: 11906, orient: 'landscape' }), SET('', '<w:bookFoldPrinting/><w:bookFoldPrintingSheets w:val="4"/>'));
  G('g2-fold-port-gut', S(), SET('', '<w:bookFoldPrinting/>'));
  G('g2-fold-port-mirror', S({ gut: 0 }), SET('<w:mirrorMargins/>', '<w:bookFoldPrinting/>'));
  // a margin-relative vertical float under gutterAtTop (page 1) and mirror margins in mode 14
  const vbox = tbx({ i: 1, x: 0, y: 0, w: 80, h: 20, relH: 'margin', relV: 'margin', content: p(r('VM Hxg')) }) + tbx({ i: 2, x: 0, y: 0, w: 80, h: 20, relH: 'page', relV: 'topMargin', content: p(r('TM Hxg')), alignV: 'bottom' });
  G('g2-gut-top-vfloat', S(), SET('<w:gutterAtTop/>'), { body: p(vbox + r('ancora Hxg')) + filler(3) });
  G('g2-mirror-c14', S({ gut: 0 }), SET('<w:mirrorMargins/>', '', 14));
  G('g2-gut-mirror-c14', S(), SET('<w:mirrorMargins/>', '', 14));
  G('g2-gut-c14', S(), SET('', '', 14));
  G('g2-gut-mirror-c12', S(), SET('<w:mirrorMargins/>', '', 12));
}

// ── mirrorIndents in modes 14 / 12 ────────────────────────────────────────────────────────────────────────────────
{
  const MI = '<w:mirrorIndents/>';
  const both = (body) => body + p(r('quebra Hxg'), '<w:pageBreakBefore/>') + body + p(r('quebra tres Hxg'), '<w:pageBreakBefore/>') + body;
  const forms = p(r(lorem(40, 3)), MI + '<w:ind w:left="1440" w:hanging="720"/>') + p(r(lorem(40, 4)), MI + '<w:ind w:left="400" w:right="2000" w:firstLine="720"/>')
    + p(r(lorem(40, 5)), MI + '<w:jc w:val="both"/><w:ind w:left="2000" w:right="400"/>');
  for (const c of [15, 14, 12, 11]) cases.push({ name: `mi2-forms-c${c}`, docx: D(both(forms), { compat: c }) });
  const Sx = (more) => `<w:sectPr><w:pgSz w:w="11906" w:h="16838"/><w:pgMar w:top="1417" w:right="1134" w:bottom="1417" w:left="1701" w:header="708" w:footer="708" w:gutter="0"/>${more}<w:cols w:space="708"/><w:docGrid w:linePitch="360"/></w:sectPr>`;
  cases.push({ name: 'mi2-restart-c14', docx: D(both(forms), { compat: 14, sect: Sx('<w:pgNumType w:start="2"/>') }) });
  cases.push({ name: 'mi2-mirror-c14', docx: D(both(forms), { sect: Sx(''), rawSettings: SET('<w:mirrorMargins/>', '', 14) }) });
  // a paragraph crossing pages in mode 14
  const longp = Array.from({ length: 12 }, (_, k) => `espelho ${k + 1} Hxg`).join('\n');
  cases.push({ name: 'mi2-span-c14', docx: D(filler(50) + p(r(longp), MI + '<w:ind w:left="2000" w:right="400"/>') + filler(3, 'depois'), { compat: 14 }) });
  // two sections: section 2 restarts at 1 on physical page 2 (number 1 = odd)
  const s1 = `<w:sectPr><w:pgSz w:w="11906" w:h="16838"/><w:pgMar w:top="1417" w:right="1134" w:bottom="1417" w:left="1701" w:header="708" w:footer="708" w:gutter="0"/><w:cols w:space="708"/><w:docGrid w:linePitch="360"/></w:sectPr>`;
  cases.push({ name: 'mi2-sect-restart', docx: D(forms + p(r('fim s1 Hxg'), s1) + forms, { sect: Sx('<w:pgNumType w:start="1"/>') }) });
}

// ── A15 / A16 in the older modes ──────────────────────────────────────────────────────────────────────────────────
{
  for (const c of ['c11', 'c14']) {
    cases.push({ name: `p2-tocref-${c}`, docx: new Uint8Array(readFileSync(new URL(`../../../packages/docx-layout/test/oracle/tocref/tocref-pbstart-${c}.docx`, import.meta.url))) });
  }
  const BR = '<w:r><w:br w:type="page"/></w:r>';
  for (const c of [12, 11]) {
    cases.push({ name: `p2-a15-c${c}`, docx: D(p(r('Primeiro Hxg'), '<w:spacing w:before="0" w:after="200"/>') + p(BR + r('Segundo Hxg'), '<w:spacing w:before="480" w:after="0"/>') + p(r('Terceiro Hxg')), { compat: c }) });
  }
  const S = ({ type, start, refs = '' } = {}) => `<w:sectPr>${refs}${type ? `<w:type w:val="${type}"/>` : ''}<w:pgSz w:w="11906" w:h="16838"/><w:pgMar w:top="1417" w:right="1701" w:bottom="1417" w:left="1701" w:header="708" w:footer="708" w:gutter="0"/>${start !== undefined ? `<w:pgNumType w:start="${start}"/>` : ''}<w:cols w:space="708"/><w:docGrid w:linePitch="360"/></w:sectPr>`;
  const hdr = (t) => p(r(`${t} `) + fld('PAGE', '1'));
  for (const c of [14, 12]) {
    for (const [name, type, start, eo] of [['odd-s2', 'oddPage', 2, false], ['odd-none', 'oddPage', undefined, false], ['even-s1-eo', 'evenPage', 1, true], ['odd-s1-eo', 'oddPage', 1, true]]) {
      cases.push({ name: `p2-a16-${name}-c${c}`, docx: buildDocx({ body: p(r('S1 pagina Hxg')) + p(r('Fim S1 Hxg'), S()) + p(r('S2 texto Hxg')), sect: S({ type, start }), compat: c, headers: eo ? { default: hdr('Cab'), even: hdr('Par') } : { default: hdr('Cab') }, settings: eo ? '<w:evenAndOddHeaders/>' : '', defaults: docDefaults({ font: LS, size: 11 }) }) });
    }
  }
}

// ── B10 / B11 in mode 14 ──────────────────────────────────────────────────────────────────────────────────────────
{
  const box = (o) => tbx({ i: 1, ins: [0, 0, 0, 0], content: p(r('Caixa Hxg')), wrap: 'wrapSquare', ...o });
  const long = Array.from({ length: 12 }, (_, k) => `longa ${k + 1} Hxg`);
  const para12 = (a) => p(long.map((t, k) => r(t + (k < 11 ? '\n' : ''))).join('') + a);
  for (const c of [14, 12]) {
    cases.push({ name: `bx-b10-para-end-c${c}`, docx: D(filler(50) + para12(box({ x: 250, y: 0, w: 120, h: 40, relH: 'margin', relV: 'paragraph' })) + filler(6, 'depois'), { compat: c }) });
    cases.push({ name: `bx-b10-line-end-c${c}`, docx: D(filler(50) + para12(box({ x: 250, y: 0, w: 120, h: 40, relH: 'margin', relV: 'line' })) + filler(6, 'depois'), { compat: c }) });
    const nine = (a) => Array.from({ length: 9 }, (_, k) => p((k === 5 ? a : '') + r(lorem(40, 3 + k)))).join('');
    cases.push({ name: `bx-b11-sq-60-c${c}`, docx: D(filler(2) + nine(box({ x: 0, y: -60, w: 200, h: 40, relH: 'margin', relV: 'paragraph' })), { compat: c }) });
  }
}
