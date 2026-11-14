// WordCraft study, planner "L", fourth batch (see wcl.mjs): last details the plan depends on.
//  - firstLineChars / leftChars under a snapToChars grid (lc measured: unit + charSpace/4096);
//  - emphasis marks on part of a line next to a taller run without marks (which height grows);
//  - jc=distribute with a tab on the line (do the gaps before the tab spread?).
// Run with --fixdir core/target/wordcraft-study/L/wcl4.
import { buildDocx, docDefaults, p, r, sz } from '../docx-builder.mjs';

const jaDefaults = (size) => docDefaults({ font: 'Calibri', size })
  .replace(/<w:rFonts[^>]*\/>/, '<w:rFonts w:ascii="Calibri" w:hAnsi="Calibri" w:eastAsia="MS Gothic" w:cs="Calibri"/>')
  .replace('w:eastAsia="en-US"', 'w:eastAsia="ja-JP"');
const sect = (grid) => `<w:sectPr><w:pgSz w:w="11906" w:h="16838"/><w:pgMar w:top="1417" w:right="1701" w:bottom="1417" w:left="1701" w:header="708" w:footer="708" w:gutter="0"/><w:cols w:space="708"/>${grid}</w:sectPr>`;
const TAB = '<w:r><w:tab/></w:r>';
export const cases = [];

{
  const T = (s) => 'linha um do paragrafo Hxg ' + s + ' e a segunda linha que continua bastante para quebrar com folga no fim da coluna aqui';
  const body = [
    p(r(T('fl200')), '<w:ind w:firstLineChars="200"/>'),
    p(r(T('left300')), '<w:ind w:leftChars="300"/>'),
    p(r('日本語の文字列です日本語の文字列です日本語の文字列です日本語の文字列です'), '<w:ind w:firstLineChars="200"/>'),
    p(r('日本語の文字列です日本語の文字列です日本語の文字列です日本語の文字列です'), '<w:ind w:leftChars="100" w:hangingChars="100"/>'),
    p(r('日本語の文字列です'), '<w:ind w:left="300"/>'),
    p(r('日本語の文字列です'), '<w:ind w:left="700" w:right="700"/><w:jc w:val="right"/>'),
  ].join('');
  for (const cs of [0, 4096]) {
    cases.push({ name: `n-charsind-sc-${cs}`, docx: buildDocx({ body, defaults: jaDefaults(10.5), sect: sect(`<w:docGrid w:type="snapToChars" w:linePitch="360" w:charSpace="${cs}"/>`) }) });
  }
}

{
  const em = '<w:em w:val="dot"/>';
  const body = [
    p(r('ref 0 Hxg')),
    p(r('abc', em) + r(' DEF Hxg', sz(20))), p(r('ref 1 Hxg')),
    p(r('abc', em) + r(' DEF Hxg', sz(14))), p(r('ref 2 Hxg')),
    p(r('abc') + r(' DEF', sz(20) + em) + r(' ghi')), p(r('ref 3 Hxg')),
    p(r('abc', em + sz(8)) + r(' def')), p(r('ref 4 Hxg')),
    p(r('abc', '<w:em w:val="underDot"/>') + r(' DEF Hxg', sz(20))), p(r('ref 5 Hxg')),
  ].join('');
  cases.push({ name: 'n-emph-mix', docx: buildDocx({ body, defaults: jaDefaults(11) }) });
  cases.push({ name: 'n-emph-mix-none', docx: buildDocx({ body: body.replaceAll(em, '').replaceAll('<w:em w:val="underDot"/>', ''), defaults: jaDefaults(11) }) });
}

{
  const TEXTW = 11906 - 2 * 1701;
  let body = '';
  for (const R of [6000, 4000, 2000]) {
    body += p(r('ab cd') + TAB + r('ef gh'), `<w:jc w:val="distribute"/><w:ind w:left="0" w:right="${R}"/>`);
    body += p(r('ab cd') + TAB + r('ef gh ij kl mn op qr st uv wx yz ab cd ef gh ij kl mn op qr st uv wx yz fim'), `<w:jc w:val="distribute"/><w:ind w:left="0" w:right="${R}"/>`);
  }
  body += p(r('ab cd') + TAB + r('ef gh'), `<w:jc w:val="both"/><w:ind w:left="0" w:right="${TEXTW - 5000}"/>`);
  cases.push({ name: 'n-dist-tab', docx: buildDocx({ body, defaults: docDefaults({ font: 'Calibri', size: 11 }) }) });
}
