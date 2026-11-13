// Wave 7c (W, item 3 — the Rust port): caixas de texto VINCULADAS ancoradas numa célula de tabela (e os controles fora
// da tabela). B mediu no Word: a 1ª caixa mostra o começo da cadeia; o TS e o Rust, no modo 15, deixavam a 1ª vazia
// (o controle de linhas órfãs mandava o parágrafo inteiro para a 2ª). Liberation Sans 11; modos 15 e 12.
import { buildDocx, docDefaults, p, r, lorem } from '../docx-builder.mjs';
import { tbx } from './_tbx.mjs';

const BD = '<w:tblBorders>' + ['top', 'left', 'bottom', 'right', 'insideH', 'insideV'].map((k) => `<w:${k} w:val="single" w:sz="4" w:space="0" w:color="auto"/>`).join('') + '</w:tblBorders>';
const cellTbl = (c0, c1 = p(r('celula 01'))) => `<w:tbl><w:tblPr><w:tblW w:w="0" w:type="auto"/>${BD}<w:tblLook w:val="04A0"/></w:tblPr><w:tblGrid><w:gridCol w:w="4252"/><w:gridCol w:w="4252"/></w:tblGrid>`
  + `<w:tr><w:tc><w:tcPr><w:tcW w:w="4252" w:type="dxa"/></w:tcPr>${c0}</w:tc><w:tc><w:tcPr><w:tcW w:w="4252" w:type="dxa"/></w:tcPr>${c1}</w:tc></w:tr></w:tbl>`;
const TEXT = 'primeira caixa com um texto longo que continua na segunda caixa encadeada';
const LONG = lorem(60, 4);
// caixas relativas à coluna/parágrafo (como o Word grava uma caixa inserida na célula), quebra quadrada
const box = (o) => tbx({ relH: 'column', relV: 'paragraph', wrap: 'wrapSquare', w: 104, ...o });
const one = (content, h1 = 23.62, y2 = 39.37, h2 = 31.5) => box({ i: 1, x: 3.94, y: 2.36, h: h1, id: 1, content })
  + box({ i: 2, x: 3.94, y: y2, h: h2, link: { id: 1, seq: 1 } });
const three = (content) => box({ i: 1, x: 3.94, y: 2.36, h: 23.62, id: 1, content })
  + box({ i: 2, x: 3.94, y: 39.37, h: 23.62, link: { id: 1, seq: 1 } }) + box({ i: 3, x: 3.94, y: 70, h: 60, link: { id: 1, seq: 2 } });
const inCell = (objs) => p(r('antes')) + cellTbl(p(r('celula com duas caixas ') + objs) + p('') + p('') + p('') + p('') + p('') + p('')) + p(r('depois'));
const inBody = (objs) => p(r('antes')) + p(r('paragrafo com duas caixas ') + objs) + p('') + p('') + p('') + p('') + p('') + p('') + p(r('depois'));

export const cases = [];
for (const compat of [15, 12]) {
  const s = compat === 15 ? '' : `-c${compat}`;
  const D = (body) => buildDocx({ body, compat, defaults: docDefaults({ font: 'Liberation Sans', size: 11 }) });
  const T = p(r(TEXT));
  cases.push({ name: `lc-cell${s}`, docx: D(inCell(one(T))) });
  cases.push({ name: `lc-body${s}`, docx: D(inBody(one(T))) });
  // a 1ª com duas linhas de altura; parágrafo longo (órfãs/viúvas no meio)
  cases.push({ name: `lc-cell-2l${s}`, docx: D(inCell(one(p(r(LONG)), 36.3, 45, 120))) });
  cases.push({ name: `lc-body-2l${s}`, docx: D(inBody(one(p(r(LONG)), 36.3, 45, 120))) });
  cases.push({ name: `lc-cell-long${s}`, docx: D(inCell(one(p(r(LONG)), 23.62, 39.37, 150))) });
  cases.push({ name: `lc-body-long${s}`, docx: D(inBody(one(p(r(LONG)), 23.62, 39.37, 150))) });
  // dois parágrafos: o 1º de uma linha cabe na 1ª caixa
  cases.push({ name: `lc-cell-2p${s}`, docx: D(inCell(one(p(r('curto')) + p(r(TEXT))))) });
  // três caixas; a 1ª baixa demais para uma linha
  cases.push({ name: `lc-cell-3${s}`, docx: D(inCell(three(p(r(LONG))))) });
  cases.push({ name: `lc-cell-tiny${s}`, docx: D(inCell(one(T, 10))) });
  // layoutInCell = 0
  cases.push({ name: `lc-cell-lic0${s}`, docx: D(inCell(one(T)).replaceAll('layoutInCell="1"', 'layoutInCell="0"')) });
  // sem controle de linhas órfãs/viúvas
  cases.push({ name: `lc-cell-nowc${s}`, docx: D(inCell(one(p(r(TEXT), '<w:widowControl w:val="0"/>')))) });
}
