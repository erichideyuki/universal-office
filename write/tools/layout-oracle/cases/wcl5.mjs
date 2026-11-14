// WordCraft study, planner "L", fifth batch: emphasis-mark line height under the spacing rules (single, multiple
// 1.15/1.5/2/3, at least 12/20 pt, exact 15 pt), with the control body without w:em. --fixdir core/target/wordcraft-study/L/wcl5.
import { buildDocx, docDefaults, p, r } from '../docx-builder.mjs';

const defaults = docDefaults({ font: 'Calibri', size: 11 }).replace(/<w:rFonts[^>]*\/>/, '<w:rFonts w:ascii="Calibri" w:hAnsi="Calibri" w:eastAsia="MS Gothic" w:cs="Calibri"/>');
const sp = [['240', 'auto'], ['276', 'auto'], ['360', 'auto'], ['480', 'auto'], ['720', 'auto'], ['240', 'atLeast'], ['400', 'atLeast'], ['300', 'exact']];
export const cases = [];
for (const em of ['dot', 'underDot', null]) {
  let body = p(r('ref 0 Hxg'));
  sp.forEach(([line, rule], k) => {
    body += p(r('enfase Hxg', em ? `<w:em w:val="${em}"/>` : ''), `<w:spacing w:line="${line}" w:lineRule="${rule}"/>`) + p(r(`ref ${k + 1} Hxg`));
  });
  cases.push({ name: `o-emph-sp-${em ?? 'none'}`, docx: buildDocx({ body, defaults }) });
}
