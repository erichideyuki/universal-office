// WordCraft study (2026-11-13), implementer "tables" — B6 follow-up (core/target/wordcraft-study/tables/wctabbd2).
// B/wcbtbl2's b6-band-insv (look firstRow=1, a style with ONLY a band1Horz part) put the band1Horz insideV on rows 0 and
// 2: the header row did not shift the bands. Does the firstRow look bit shift the row bands only when the style HAS a
// firstRow part (and the firstColumn bit the column bands only with a firstCol part)? 4 rows × 3 columns, 3 pt red
// borders from the style, Liberation Sans 11 (as wctabbd).
import { buildDocx, docDefaults, p, r } from '../docx-builder.mjs';

const B = (s, sz = 24, c = 'FF0000') => `<w:${s} w:val="single" w:sz="${sz}" w:space="0" w:color="${c}"/>`;
const part = (type, inner) => `<w:tblStylePr w:type="${type}">${inner}</w:tblStylePr>`;
const tcb = (x) => `<w:tcPr><w:tcBorders>${x}</w:tcBorders></w:tcPr>`;
const T = (parts, extraTblPr = '') => `<w:style w:type="table" w:customStyle="1" w:styleId="TCond"><w:name w:val="TCond"/><w:basedOn w:val="TableNormal"/><w:tblPr>${extraTblPr}</w:tblPr>`
  + parts.join('') + '</w:style>';
const tc = (inner) => `<w:tc><w:tcPr><w:tcW w:w="2000" w:type="dxa"/></w:tcPr>${inner}</w:tc>`;
const rows = (n) => Array.from({ length: n }, (_, k) => `<w:tr>${['a', 'b', 'c'].map((c) => tc(p(r(`l${k} ${c} Hxg`)))).join('')}</w:tr>`).join('');
const look = (o) => `<w:tblLook w:val="0000" w:firstRow="${o.fr ?? 0}" w:lastRow="${o.lr ?? 0}" w:firstColumn="${o.fc ?? 0}" w:lastColumn="${o.lc ?? 0}" w:noHBand="${o.nh ?? 1}" w:noVBand="${o.nv ?? 1}"/>`;
const tbl = (o) => `<w:tbl><w:tblPr><w:tblStyle w:val="TCond"/><w:tblW w:w="6000" w:type="dxa"/>${look(o)}</w:tblPr>`
  + `<w:tblGrid>${'<w:gridCol w:w="2000"/>'.repeat(3)}</w:tblGrid>${rows(4)}</w:tbl>`;
const D = (o, parts, styleTblPr) => buildDocx({ body: p(r('Antes Hxg')) + tbl(o) + p(r('Depois Hxg')), styles: T(parts, styleTblPr), defaults: docDefaults({ font: 'Liberation Sans', size: 11 }) });

const RB1 = '<w:tblStyleRowBandSize w:val="1"/>';
const CB1 = '<w:tblStyleColBandSize w:val="1"/>';
const bandH = part('band1Horz', tcb(B('insideV')));
const bandV = part('band1Vert', tcb(B('insideH')));
export const cases = [
  // row bands, look firstRow=1: no firstRow part / a firstRow part with only run properties / with only shading
  { name: 'bb-h-fr-nopart', docx: D({ fr: 1, nh: 0 }, [bandH], RB1) },
  { name: 'bb-h-fr-rpr', docx: D({ fr: 1, nh: 0 }, [bandH, part('firstRow', '<w:rPr><w:b/></w:rPr>')], RB1) },
  { name: 'bb-h-fr-shd', docx: D({ fr: 1, nh: 0 }, [bandH, part('firstRow', '<w:tcPr><w:shd w:val="clear" w:color="auto" w:fill="DDDDDD"/></w:tcPr>')], RB1) },
  { name: 'bb-h-fr-empty', docx: D({ fr: 1, nh: 0 }, [bandH, part('firstRow', '')], RB1) },
  { name: 'bb-h-nofr-part', docx: D({ fr: 0, nh: 0 }, [bandH, part('firstRow', '<w:rPr><w:b/></w:rPr>')], RB1) },
  // column bands, look firstColumn=1: no firstCol part / a firstCol part
  { name: 'bb-v-fc-nopart', docx: D({ fc: 1, nv: 0 }, [bandV], CB1) },
  { name: 'bb-v-fc-rpr', docx: D({ fc: 1, nv: 0 }, [bandV, part('firstCol', '<w:rPr><w:b/></w:rPr>')], CB1) },
  // last row / last column with bands: lastRow look bit without a lastRow part
  { name: 'bb-h-lr-nopart', docx: D({ lr: 1, nh: 0 }, [bandH], RB1) },
  { name: 'bb-v-lc-nopart', docx: D({ lc: 1, nv: 0 }, [bandV], CB1) },
];
