// WordCraft study, LINES (L9 residual): jc=distribute in compatibility modes 14 and 12 (the mode-15 rule is
// wcl/l-jspread-*). Open fonts only. Run with --fixdir core/target/wc-lines/fix14 (measurement, not versioned).
import { buildDocx, docDefaults, p, r, sz } from '../docx-builder.mjs';
import { createNodeFontProvider } from '../../corpus-lab/node-fonts.mjs';

const prov = await createNodeFontProvider();
const widthOf = (name, size) => {
  const f = prov.face(name, false, false)?.font;
  if (!f) throw new Error('font ' + name);
  return (s) => [...s].reduce((a, ch) => a + Math.floor((f.advanceWidth(f.glyphId(ch.codePointAt(0))) * size * 4096) / f.metrics.unitsPerEm) / 4096 * 20, 0);
};
const TEXTW = 11906 - 2 * 1701;
const ind = (left, right) => `<w:ind w:left="${Math.round(left)}" w:right="${Math.round(right)}"/>`;
export const cases = [];
const texts = ['abcdefgh', 'abcd efgh', 'ab cd ef gh', 'ab ab ab ab ab ab ab ab'];
const slacks = [0.3, 1, 2, 3, 5, 8, 12, 20, 40, 100];
for (const [f, s] of [['Carlito', 11], ['Carlito', 22], ['Liberation Sans', 11], ['Liberation Serif', 11]]) {
  const w = widthOf(f, s);
  let body = '';
  for (const ctx of ['last', 'mid']) {
    for (const t of texts) {
      for (const sl of slacks) {
        const A = Math.round(w(t) + sl * 20);
        const txt = ctx === 'last' ? t : t + ' ' + 'X'.repeat(90);
        body += p(r(txt, sz(s)), `<w:jc w:val="distribute"/>${ind(0, TEXTW - A)}`);
      }
    }
  }
  for (const c of [14, 12]) {
    cases.push({ name: `d${c}-${f.toLowerCase().replace(/\s+/g, '')}${s}`, docx: buildDocx({ body, defaults: docDefaults({ font: f, size: s }), compat: c }) });
  }
}
