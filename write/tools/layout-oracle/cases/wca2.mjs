// WordCraft study, section A, second batch (see wca.mjs). A25: the height the footnote SEPARATOR takes in the page's
// note area — its own paragraph (Arial 11 single: 12.65 pt, ours) or a fixed 12 pt area (WordCraft). fnsep's
// sweep (2 pt steps) cannot tell 12 from 12.65; this one steps 0.2 pt where the two predictions part:
// reference line (Arial 11 x1.15, after 0) top T fits when T + 14.55 + sep + 11.5 (Arial 10 note) <= 771.05.
import { buildDocx, docDefaults, p, r } from '../docx-builder.mjs';
import { strToU8 } from 'fflate';

const styles = '<w:style w:type="paragraph" w:styleId="FootnoteText"><w:name w:val="footnote text"/><w:basedOn w:val="Normal"/><w:pPr><w:spacing w:after="0" w:line="240" w:lineRule="auto"/></w:pPr><w:rPr><w:sz w:val="20"/><w:szCs w:val="20"/></w:rPr></w:style>'
  + '<w:style w:type="character" w:styleId="FootnoteReference"><w:name w:val="footnote reference"/><w:rPr><w:vertAlign w:val="superscript"/></w:rPr></w:style>';
const ref = '<w:r><w:rPr><w:rStyle w:val="FootnoteReference"/></w:rPr><w:footnoteReference w:id="1"/></w:r>';
const note = '<w:footnote w:id="1"><w:p><w:pPr><w:pStyle w:val="FootnoteText"/></w:pPr><w:r><w:rPr><w:rStyle w:val="FootnoteReference"/></w:rPr><w:footnoteRef/></w:r><w:r><w:t xml:space="preserve"> Nota curta.</w:t></w:r></w:p></w:footnote>';
const SEP = (line) => `<w:pPr><w:spacing w:after="0" w:line="${line}" w:lineRule="auto"/></w:pPr>`;
const footnotes = (sepPPr) => `<?xml version="1.0" encoding="UTF-8" standalone="yes"?><w:footnotes xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:footnote w:type="separator" w:id="-1"><w:p>${sepPPr}<w:r><w:separator/></w:r></w:p></w:footnote><w:footnote w:type="continuationSeparator" w:id="0"><w:p>${sepPPr}<w:r><w:continuationSeparator/></w:r></w:p></w:footnote>${note}</w:footnotes>`;
const D = (top, sepPPr, size = 11) => buildDocx({
  body: p(r('topo Hxg'), `<w:spacing w:after="0" w:line="${Math.round((top - 70.85) * 20)}" w:lineRule="exact"/>`) + p(r('Referencia') + ref + r(' Hxg'), '<w:spacing w:after="0"/>') + p(r('Depois Hxg')) + p(r('Fim Hxg')),
  styles, defaults: docDefaults({ font: 'Arial', size, after: 200, line: 276 }), extraParts: { 'word/footnotes.xml': strToU8(footnotes(sepPPr)) },
});

export const cases = [];
for (const top of [731.6, 731.8, 732.0, 732.2, 732.4, 732.6, 732.8, 733.0, 733.2]) {
  cases.push({ name: `a25-sep-top${String(top).replace('.', '_')}`, docx: D(top, SEP(240)) });
}
// separator paragraph at double spacing (25.3 pt): its own height counts?  (WordCraft: always 12 pt)
for (const top of [718, 720, 722, 724, 726]) cases.push({ name: `a25-sepdbl-top${top}`, docx: D(top, SEP(480)) });
