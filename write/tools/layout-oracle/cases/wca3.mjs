// WordCraft study, section A, third batch (see wca.mjs):
//  - A1: compression of jc=distribute — wca/a1-dist showed Word compresses like "both" with 4 and 8 spaces but not with
//    16 (ov/S >= 0.159); sweep ov/S from 0.02 to 0.26 for n = 4, 8, 12, 16, 24 spaces to find the rule;
//  - A21: width of the SPACE suffix after a list number (wca/a21-space: about 0.278 em in Arial, Calibri and Times —
//    Arial's space?) in fonts with other space widths (Courier New 0.6 em, Verdana 0.35 em, Georgia 0.24 em);
//  - A26: underline position/thickness (read from the PDF's paths);
//  - A15: page break inside a paragraph when the previous paragraph has no space after.
import { buildDocx, docDefaults, p, r, sz, font } from '../docx-builder.mjs';
import { createNodeFontProvider } from '../../corpus-lab/node-fonts.mjs';

const prov = await createNodeFontProvider();
const widthOf = (name, size) => {
  const f = prov.face(name, false, false)?.font;
  return (s) => [...s].reduce((a, ch) => a + Math.floor((f.advanceWidth(f.glyphId(ch.codePointAt(0))) * size * 4096) / f.metrics.unitsPerEm) / 4096 * 20, 0);
};
const TEXTW = 11906 - 2 * 1701;
export const cases = [];

{
  const w = widthOf('Calibri', 11);
  const g = w(' ');
  for (const jc of ['distribute', 'both']) {
    let body = '';
    for (const n of [4, 8, 12, 16, 24]) {
      const S = n * g;
      const W = w('que');
      const base = n * (w('ab') + g) + W;
      for (let k = 1; k <= 13; k++) {
        const ov = S * 0.02 * k; // 0.02 .. 0.26 of the spaces
        body += p(r(Array(n).fill('ab').join(' ') + ' que fim da linha seguinte'), `<w:jc w:val="${jc}"/><w:ind w:left="0" w:right="${Math.round(TEXTW - (base - ov))}"/>`);
      }
    }
    cases.push({ name: `a1-sweep-${jc}`, docx: buildDocx({ body, defaults: docDefaults({ font: 'Calibri', size: 11 }) }) });
  }
}

{
  const fonts = [['Courier New', 11], ['Verdana', 11], ['Georgia', 11], ['Calibri', 20], ['Times New Roman', 20], ['Arial', 11]];
  const numbering = fonts.map(([f, s], i) => `<w:abstractNum w:abstractNumId="${i + 1}"><w:lvl w:ilvl="0"><w:start w:val="1"/><w:numFmt w:val="decimal"/><w:suff w:val="space"/><w:lvlText w:val="%1."/><w:lvlJc w:val="left"/><w:pPr><w:ind w:left="720" w:hanging="360"/></w:pPr><w:rPr>${font(f)}${sz(s)}</w:rPr></w:lvl></w:abstractNum>`).join('')
    + fonts.map((_, i) => `<w:num w:numId="${i + 1}"><w:abstractNumId w:val="${i + 1}"/></w:num>`).join('');
  const body = fonts.map(([f, s], i) => p(r('texto Hxg', font(f) + sz(s)), `<w:numPr><w:ilvl w:val="0"/><w:numId w:val="${i + 1}"/></w:numPr>`)).join('')
    // the same numbers typed as text ("1." + space) for reference
    + fonts.map(([f, s]) => p(r('1. texto Hxg', font(f) + sz(s)), '<w:ind w:left="720" w:hanging="360"/>')).join('');
  cases.push({ name: 'a21-space2', docx: buildDocx({ body, numbering, defaults: docDefaults({ font: 'Calibri', size: 11 }) }) });
  // the paragraph-mark font differs from the number font (does the suffix follow the paragraph mark?)
  const body2 = fonts.map(([f, s], i) => p(r('texto Hxg', font(f) + sz(s)), `<w:numPr><w:ilvl w:val="0"/><w:numId w:val="${i + 1}"/></w:numPr><w:rPr>${font('Arial')}${sz(30)}</w:rPr>`)).join('');
  cases.push({ name: 'a21-space-mark', docx: buildDocx({ body: body2, numbering, defaults: docDefaults({ font: 'Calibri', size: 11 }) }) });
}

{
  let body = '';
  for (const [f, s] of [['Calibri', 11], ['Arial', 11], ['Times New Roman', 11], ['Calibri', 24], ['Arial', 36]]) {
    body += p(r('Sublinhado Hxg', font(f) + sz(s) + '<w:u w:val="single"/>'));
  }
  cases.push({ name: 'a26-underline', docx: buildDocx({ body, defaults: docDefaults({ font: 'Calibri', size: 11 }) }) });
}

{
  const BR = '<w:r><w:br w:type="page"/></w:r>';
  const D = (body, compat = 15) => buildDocx({ body, compat, defaults: docDefaults({ font: 'Calibri', size: 11 }) });
  cases.push({ name: 'a15-br-p1after0', docx: D(p(r('Primeiro Hxg'), '<w:spacing w:before="0" w:after="0"/>') + p(BR + r('Segundo Hxg'), '<w:spacing w:before="480" w:after="0"/>') + p(r('Terceiro Hxg'))) });
  cases.push({ name: 'a15-br-mid', docx: D(p(r('Primeiro Hxg'), '<w:spacing w:before="0" w:after="200"/>') + p(r('Antes da quebra Hxg') + BR + r('Segundo Hxg'), '<w:spacing w:before="480" w:after="0"/>') + p(r('Terceiro Hxg'))) });
  cases.push({ name: 'a15-br-p1after0-c14', docx: D(p(r('Primeiro Hxg'), '<w:spacing w:before="0" w:after="0"/>') + p(BR + r('Segundo Hxg'), '<w:spacing w:before="480" w:after="0"/>') + p(r('Terceiro Hxg')), 14) });
}
