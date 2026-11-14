// WordCraft study (2026-11-13), planner "tables" — the aside of B (core/target/wordcraft-study/tables/wctabtw):
// in B/wcbtbl2/b6-* (tblW 9000 dxa, 3 × gridCol/tcW 3000, text width 8504 tw, table style TCond) Word fitted the
// columns to 141.7 pt (8504 / 3) while ours kept 150 pt; without the style (B/wcbmisc/tw-auto, tw-fixed) both keep
// 150. Those documents wrote <w:tblStyle> AFTER <w:tblW> (out of the schema's order) and had no tblBorders. Isolate:
// the order, the style, tblLook, a style that exists (TableGrid), a missing style id, direct properties written
// before tblStyle (borders, jc), tblW auto, mode 14.
// Liberation Sans 11, A4 margins 1701 tw.
import { buildDocx, docDefaults, p, r } from '../docx-builder.mjs';

const BD = '<w:tblBorders>' + ['top', 'left', 'bottom', 'right', 'insideH', 'insideV'].map((s) => `<w:${s} w:val="single" w:sz="4" w:space="0" w:color="auto"/>`).join('') + '</w:tblBorders>';
const BDR = '<w:tblBorders>' + ['top', 'left', 'bottom', 'right', 'insideH', 'insideV'].map((s) => `<w:${s} w:val="single" w:sz="12" w:space="0" w:color="FF0000"/>`).join('') + '</w:tblBorders>';
const TCOND = '<w:style w:type="table" w:customStyle="1" w:styleId="TCond"><w:name w:val="TCond"/><w:basedOn w:val="TableNormal"/><w:tblPr><w:tblStyleRowBandSize w:val="1"/></w:tblPr>'
  + '<w:tblStylePr w:type="firstRow"><w:tcPr><w:tcBorders><w:bottom w:val="single" w:sz="24" w:space="0" w:color="FF0000"/></w:tcBorders></w:tcPr></w:tblStylePr></w:style>';
const LOOK = '<w:tblLook w:val="04A0" w:firstRow="1" w:lastRow="0" w:firstColumn="1" w:lastColumn="0" w:noHBand="0" w:noVBand="1"/>';
const W9 = '<w:tblW w:w="9000" w:type="dxa"/>';
const tc = (t) => `<w:tc><w:tcPr><w:tcW w:w="3000" w:type="dxa"/></w:tcPr>${p(r(t))}</w:tc>`;
const tbl = (tblPr) => `<w:tbl><w:tblPr>${tblPr}</w:tblPr><w:tblGrid>${'<w:gridCol w:w="3000"/>'.repeat(3)}</w:tblGrid>`
  + [0, 1].map((k) => `<w:tr>${tc(`um ${k} Hxg`)}${tc(`dois ${k} Hxg`)}${tc(`tres ${k} Hxg`)}</w:tr>`).join('') + '</w:tbl>';
const sty = (id) => `<w:tblStyle w:val="${id}"/>`;

export const cases = [];
const add = (name, tblPr, compat = 15) => cases.push({ name, docx: buildDocx({ body: p(r('Antes Hxg')) + tbl(tblPr) + p(r('Depois Hxg')), compat, styles: TCOND, defaults: docDefaults({ font: 'Liberation Sans', size: 11 }) }) });
// the repro (b6 shape) and the same in the schema's order
add('tw-tcond-after', W9 + sty('TCond') + LOOK);
add('tw-tcond-first', sty('TCond') + W9 + LOOK);
// TableGrid (an existing built-in style with borders), after / first
add('tw-grid-after', W9 + sty('TableGrid') + LOOK);
add('tw-grid-first', sty('TableGrid') + W9 + LOOK);
// a style id that does not exist, after tblW
add('tw-missing-after', W9 + sty('NaoExiste') + LOOK);
// no style: tblLook alone after tblW; tblW + borders + tblLook (the tw-auto shape plus tblLook)
add('tw-look-only', W9 + LOOK);
add('tw-bd-look', W9 + BD + LOOK);
// the style after tblW but no tblLook
add('tw-tcond-after-nolook', W9 + sty('TCond'));
// direct properties BEFORE the out-of-order tblStyle: are they kept? (red 1.5 pt borders, centred)
add('tw-bd-before-style', W9 + '<w:jc w:val="center"/>' + BDR + sty('TableGrid') + LOOK);
add('tw-bd-first-style', sty('TableGrid') + W9 + '<w:jc w:val="center"/>' + BDR + LOOK);
// another out-of-order element (tblLayout fixed before tblW) — is it a general "out of order" rule?
add('tw-layout-before-w', '<w:tblLayout w:type="fixed"/>' + W9 + BD);
add('tw-ind-before-w', '<w:tblInd w:w="300" w:type="dxa"/>' + W9 + BD);
// tblW auto with tcW 3000 × 3 (no style): fitted?
add('tw-auto-tcw', '<w:tblW w:w="0" w:type="auto"/>' + BD);
add('tw-auto-tcw-style', sty('TableGrid') + '<w:tblW w:w="0" w:type="auto"/>');
// mode 14
add('tw-tcond-after-c14', W9 + sty('TCond') + LOOK, 14);
add('tw-tcond-first-c14', sty('TCond') + W9 + LOOK, 14);
// round 3: other direct properties before the out-of-order tblStyle — a floating position (tblpPr) and cell margins
add('tw-tblp-before-style', '<w:tblpPr w:leftFromText="0" w:rightFromText="0" w:vertAnchor="text" w:horzAnchor="margin" w:tblpX="2000" w:tblpY="1"/>' + W9.replace('9000', '6000') + sty('TableGrid') + LOOK);
add('tw-tblp-first-style', sty('TableGrid') + '<w:tblpPr w:leftFromText="0" w:rightFromText="0" w:vertAnchor="text" w:horzAnchor="margin" w:tblpX="2000" w:tblpY="1"/>' + W9.replace('9000', '6000') + LOOK);
add('tw-cellmar-before-style', W9 + '<w:tblCellMar><w:left w:w="600" w:type="dxa"/><w:right w:w="0" w:type="dxa"/></w:tblCellMar>' + sty('TableGrid') + LOOK);
add('tw-after-style-kept', sty('TableGrid').replace('TableGrid', 'TableGrid') + LOOK + W9 + '<w:jc w:val="center"/>');
add('tw-w-after-late-style', '<w:jc w:val="center"/>' + sty('TableGrid') + W9 + LOOK);
