// Wave 7c (W, item 6 — the Rust port): "Manter linhas juntas" (keepLines) num parágrafo MAIS ALTO que a página.
// O Word leva o parágrafo inteiro para a página seguinte (e lá ele parte como de costume) quando ele não cabe no
// que sobra, mesmo que não caiba numa página inteira; a regra do TS (engine.ts:1860) só o move quando cabe numa
// página inteira. Liberation Sans 11 (métrica do Arial): 12,65 pt por linha, 55 linhas na página A4.
import { buildDocx, docDefaults, p, r, sectPr } from '../docx-builder.mjs';

const filler = (n) => Array.from({ length: n }, (_, k) => p(r(`enchimento ${k + 1} Hxg`))).join('');
const lines = (n, label) => Array.from({ length: n }, (_, k) => `${label} ${k + 1} Hxg`).join('\n');
const KN = '<w:keepNext/>';
const KL = '<w:keepLines/>';
const OFF = '<w:widowControl w:val="0"/>';
const cell = (inner) => '<w:tbl><w:tblPr><w:tblW w:w="0" w:type="auto"/><w:tblLook w:val="04A0"/></w:tblPr><w:tblGrid><w:gridCol w:w="8504"/></w:tblGrid>'
  + `<w:tr><w:tc><w:tcPr><w:tcW w:w="8504" w:type="dxa"/></w:tcPr>${inner}</w:tc></w:tr></w:tbl>`;

export const cases = [];
for (const compat of [14, 15]) {
  const s = compat === 15 ? '' : `-c${compat}`;
  const D = (body, sect) => buildDocx({ body, sect, compat, defaults: docDefaults({ font: 'Liberation Sans', size: 11 }) });
  const after = p(r(lines(3, 'depois')));
  // keepLines mais alto que a página, depois de 1, 30 e 54 linhas
  for (const n of [56, 70, 120]) {
    for (const f of [1, 30, 54]) cases.push({ name: `kt-kl${n}-f${f}${s}`, docx: D(filler(f) + p(r(lines(n, 'longo')), KL) + after) });
  }
  // keepNext + keepLines mais alto que a página
  for (const n of [56, 70]) cases.push({ name: `kt-knkl${n}-f30${s}`, docx: D(filler(30) + p(r(lines(n, 'longo')), KN + KL) + after) });
  // no topo da página; exatamente uma página
  cases.push({ name: `kt-kl70-top${s}`, docx: D(p(r(lines(70, 'longo')), KL) + after) });
  cases.push({ name: `kt-kl55-f30${s}`, docx: D(filler(30) + p(r(lines(55, 'longo')), KL) + after) });
  // controle: só keepNext (parte como de costume)
  cases.push({ name: `kt-kn70-f30${s}`, docx: D(filler(30) + p(r(lines(70, 'longo')), KN) + after) });
  // keepNext + keepLines que cabe sozinho numa página, mas não com o início do seguinte
  for (const n of [50, 54, 55]) cases.push({ name: `kt-knkl${n}-next${s}`, docx: D(filler(30) + p(r(lines(n, 'longo')), KN + KL) + p(r(lines(10, 'depois')))) });
  cases.push({ name: `kt-knkl54-nextsp${s}`, docx: D(filler(30) + p(r(lines(54, 'longo')), KN + KL) + p(r(lines(10, 'depois')), '<w:spacing w:before="400"/>')) });
  // sem controle de viúvas/órfãs
  cases.push({ name: `kt-kl70-f30-off${s}`, docx: D(filler(30) + p(r(lines(70, 'longo')), KL + OFF) + after) });
  // duas colunas: o parágrafo é mais alto que a coluna
  const two = sectPr().replace('<w:cols w:space="708"/>', '<w:cols w:num="2" w:space="708"/>');
  cases.push({ name: `kt-kl70-cols${s}`, docx: D(filler(30) + p(r(lines(70, 'longo')), KL) + after, two) });
  cases.push({ name: `kt-kl40-cols${s}`, docx: D(filler(30) + p(r(lines(40, 'longo')), KL) + after, two) });
  // numa célula de tabela (a linha pode partir)
  cases.push({ name: `kt-kl70-cell${s}`, docx: D(filler(30) + cell(p(r(lines(70, 'longo')), KL)) + after) });
  cases.push({ name: `kt-kl20-cell-f50${s}`, docx: D(filler(50) + cell(p(r(lines(20, 'longo')), KL)) + after) });
  cases.push({ name: `kt-kl70-cell-f54${s}`, docx: D(filler(54) + cell(p(r(lines(70, 'longo')), KL)) + after) });
  cases.push({ name: `kt-plain70-cell${s}`, docx: D(filler(30) + cell(p(r(lines(70, 'longo')))) + after) });
  cases.push({ name: `kt-two-cell${s}`, docx: D(filler(30) + cell(p(r(lines(10, 'curto'))) + p(r(lines(70, 'longo')), KL)) + after) });
  cases.push({ name: `kt-kl70-cell-top${s}`, docx: D(cell(p(r(lines(70, 'longo')), KL)) + after) });
  cases.push({ name: `kt-kl120-cell${s}`, docx: D(filler(30) + cell(p(r(lines(120, 'longo')), KL)) + after) });
  // colunas: depois do parágrafo o texto volta às colunas; no topo; dentro da 2ª coluna
  const more = filler(60).replaceAll('enchimento', 'depois');
  cases.push({ name: `kt-kl70-cols-after${s}`, docx: D(filler(30) + p(r(lines(70, 'longo')), KL) + more, two) });
  cases.push({ name: `kt-kl70-cols-top${s}`, docx: D(p(r(lines(70, 'longo')), KL) + more, two) });
  cases.push({ name: `kt-kl70-cols-col2${s}`, docx: D(filler(60) + p(r(lines(70, 'longo')), KL) + after, two) });
  cases.push({ name: `kt-kl56-cols${s}`, docx: D(filler(30) + p(r(lines(56, 'longo')), KL) + more, two) });
}
