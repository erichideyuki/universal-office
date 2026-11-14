// WordCraft study (2026-11-13), planner "tables" — the placement half of B10/B11 (core/target/wordcraft-study/tables/
// wctabfl). B/wcbflt measured the anchor on a later PAGE (B10) and a float reaching above its paragraph (B11). Not
// pinned down for the placement: relativeFrom="line" when the anchor line is on the SAME page as the paragraph's first
// line (ours uses the paragraph's top), a positive line offset, "line" with the anchor in the first line, and a B11
// float whose earlier-text re-wrap pushes the anchor paragraph to the NEXT page (does the float stay pinned on the
// first page?), and B10 with the paragraph's part on the later page starting after a column break (2 columns).
// Liberation Sans 11.
import { buildDocx, docDefaults, p, r, lorem } from '../docx-builder.mjs';
import { tbx } from './_tbx.mjs';

const D = (body, extra = {}) => buildDocx({ body, defaults: docDefaults({ font: 'Liberation Sans', size: 11 }), ...extra });
const box = (o) => tbx({ i: 1, ins: [0, 0, 0, 0], content: p(r('Caixa Hxg')), wrap: 'wrapSquare', ...o });
const filler = (n, w = 'enchimento') => Array.from({ length: n }, (_, k) => p(r(`${w} ${k + 1} Hxg`))).join('');
const lines = (n, w = 'linha') => Array.from({ length: n }, (_, k) => `${w} ${k + 1} Hxg`);

export const cases = [];
// relativeFrom=line on the same page: an 8-line paragraph (soft breaks), the anchor after line `at`
for (const [name, at, y, relV] of [
  ['fl-line-same-l5', 5, 0, 'line'],
  ['fl-line-same-l5-20', 5, 20, 'line'],
  ['fl-line-same-l1', 1, 0, 'line'],
  ['fl-para-same-l5', 5, 0, 'paragraph'], // control: paragraph-relative, anchor in line 5
]) {
  const a = box({ x: 250, y, w: 120, h: 40, relH: 'margin', relV });
  const ls = lines(8);
  const runs = ls.map((t, k) => (k === at - 1 ? a : '') + r(t + (k < ls.length - 1 ? '\n' : ''))).join('');
  cases.push({ name, docx: D(filler(5) + p(runs) + filler(5, 'depois')) });
}
// B11 whose re-wrap pushes the anchor paragraph to the next page: 40 filler lines, 9 lorem paragraphs, the box (200 × 80,
// offset −120 pt, square) in the paragraph that starts near the page foot
{
  const a = box({ x: 0, y: -120, w: 200, h: 80, relH: 'margin', relV: 'paragraph' });
  for (const [name, fill] of [['fl-b11-push-38', 38], ['fl-b11-push-44', 44]]) {
    const body = filler(fill) + Array.from({ length: 4 }, (_, k) => p((k === 2 ? a : '') + r(lorem(40, 3 + k)))).join('');
    cases.push({ name, docx: D(body) });
  }
}
// B10 in two columns: the anchor paragraph starts in column 1 and ends in column 2 (same page) — the float's column
{
  const sect = '<w:sectPr><w:pgSz w:w="11906" w:h="16838"/><w:pgMar w:top="1417" w:right="1701" w:bottom="1417" w:left="1701" w:header="708" w:footer="708" w:gutter="0"/><w:cols w:num="2" w:space="708"/><w:docGrid w:linePitch="360"/></w:sectPr>';
  const a = box({ x: 20, y: 0, w: 80, h: 40, relH: 'column', relV: 'paragraph' });
  const ls = lines(12, 'longa');
  const runs = ls.map((t, k) => r(t + (k < ls.length - 1 ? '\n' : ''))).join('') + a;
  cases.push({ name: 'fl-b10-cols', docx: D(filler(50) + p(runs) + filler(6, 'depois'), { sect }) });
}
