// The regex sites of frontier D2b-F3 (docx-core table-formula.ts `parseCellNumber`, `tokenize`, the
// reference pattern and `parseFormulaInstr`; table-sort.ts `parseSortDate`) against V8, checked by
// tests/regex_d2b_f3.rs. A row is `[id, function, 'fn']` (see js/regex-diff.mjs); the TS expressions
// are copied as written.
import { INPUTS as BASE } from './templates.mjs';

// table-formula.ts:19
const cellNumber = (text) => {
  const m = /^\s*(R\$|US\$|\$|€|£)?\s*(\(?)([-+]?)(\d{1,3}(?:\.\d{3})+|\d+)(?:,(\d+))?(\)?)\s*(%?)/.exec(text);
  if (!m) return 'null';
  const int = m[4].replace(/\./g, '');
  const dec = m[5] ?? '';
  let n = parseFloat(`${int}${dec ? '.' + dec : ''}`);
  if (m[3] === '-' || (m[2] === '(' && m[6] === ')')) n = -n;
  return JSON.stringify([n, dec.length, m[4].includes('.'), m[1] ? m[1] + ' ' : null, m[7] ? '%' : null]);
};
// table-formula.ts:226
const tokenize = (s) => {
  const out = [];
  const re = /\s*(<=|>=|<>|\d[\d.]*(?:,\d+)?|[A-Za-z_][A-Za-z_0-9]*|[-+*/^%()=<>;:])/y;
  let m;
  while (re.lastIndex < s.length && (m = re.exec(s))) out.push(m[1]);
  if (re.lastIndex < s.trimEnd().length) out.push('\u0000');
  return JSON.stringify(out);
};
// table-formula.ts:166
const cellRef = (t) => { const ref = /^([A-Za-z]{1,2})(\d+)$/.exec(t); return ref ? JSON.stringify([ref[1], ref[2]]) : 'null'; };
// table-formula.ts:333
const formulaInstr = (instr) => {
  const m = /^\s*(=[^\\]*?)\s*(?:\\#\s*"([^"]*)"|\\#\s*(\S+))?\s*$/.exec(instr);
  if (!m) return 'null';
  return JSON.stringify([m[1].trim(), (m[2] ?? m[3]) ? (m[2] ?? m[3]) : null]);
};
// table-sort.ts:61
const sortDate = (text) => {
  const s = text.trim();
  let d, m, y;
  let r = /^(\d{4})-(\d{1,2})-(\d{1,2})$/.exec(s);
  if (r) { y = +r[1]; m = +r[2]; d = +r[3]; } else {
    r = /^(\d{1,2})[/.-](\d{1,2})[/.-](\d{2}|\d{4})$/.exec(s);
    if (!r) return 'null';
    d = +r[1]; m = +r[2]; y = +r[3];
    if (r[3].length === 2) y += y < 30 ? 2000 : 1900;
  }
  if (m < 1 || m > 12 || d < 1 || d > new Date(Date.UTC(y, m, 0)).getUTCDate()) return 'null';
  return String(Date.UTC(y, m - 1, d));
};

/** [site id, the TS expression, operation]. */
export const SITES = [
  ['d2b-f3.cell_number', cellNumber, 'fn'],
  ['d2b-f3.tokenize', tokenize, 'fn'],
  ['d2b-f3.cell_ref', cellRef, 'fn'],
  ['d2b-f3.formula_instr', formulaInstr, 'fn'],
  ['d2b-f3.sort_date', sortDate, 'fn'],
];

export const INPUTS = [
  ...BASE,
  // numbers: currency, parentheses, sign, thousands, decimals, percent, trailing text, odd spaces
  '10', ' R$ 1.234,56', '(4)', '5%', '-3,5', '1.000', '1.2345', 'abc', '', 'US$ 2', '€3', '£ 4,0', '1,5,5', '+7', '12.345.678,9 x', '$ (5,5)', '(5', '5)', '٣',
  '0,10', '1.00', '.5', '-.5', '100%', 'R$', '2 itens', '\t8', '1.234.5', '12345.678', '1.234.567', '12.3456', '1234.567', '999.999', '1.000,5%', 'R$-3', 'R$ -3',
  '(-4)', '-(4)', '(4,5%)', ' ( 4 )', '$ $ 4', 'US$US$4', '€ 1.000.000,01 total', '\n12', ' 12', ' 8', '1,', ',5', '1.', '1.2.3', '00', '007', '0.0', '1.000.', '1.00.000',
  '99999999999999999999', '1.000.000.000.000.000.000.000,5', '(', ')', '%', '-', '+', 'R$ (1)%', '3 %', '3\t%', '3\n%',
  // tokens of a formula: operators, numbers with dots and commas, names, stray characters, white space
  '=SUM(ABOVE)', 'SUM(ABOVE)', '1+2', ' 1 + 2 ', '1+2 ', '1 +', '2^3^2', '-2^2', '1,5*2', '1.000,5+1', 'a1:b2', 'A1:B2;C3', '<=>', '<=', '>=', '<>', '< >', '1<2', 'IF(1<2;10;20)', '10%', 'x_y9', '9x', '1..2',
  '1,,2', '1.,2', ',1', 'a b', '$', '1 $ 2', '€', '1 2', '١٢٣', 'ÀB', '=', '()', '(((', ';;', '1;2', 'AND(1;0)', 'foo', 'ZZ99', 'A0', 'a1', 'AAA1', 'A', '1A', 'A1B2', 'A-1', 'A_1', '_A1', ' A1', 'A1 ',
  // references
  'A1', 'a1', 'AB12', 'ab12', 'ABC1', 'A', '1', 'A01', 'A1.', 'A1:', 'Z9', 'zz99999999999999999999', 'A\n1', 'A1\n', 'À1', 'A１',
  // formula instructions: the picture in quotes or bare, other switches, spaces, newlines
  ' =SUM(ABOVE) ', ' =SUM(ABOVE) \\# "0,00" ', ' =SUM(ABOVE) \\# 0,00 ', '=1+1 \\# "#.##0"', '=1+1 \\#"0"', '=1+1 \\# ""', '=1+1 \\# "0" \\* MERGEFORMAT', '=1+1 \\* MERGEFORMAT',
  '=1+1 \\#', '=1+1 \\# ', '=1+1 \\# "0', '=1+1 \\# 0 1', '=1+1 \\# "a b"', '=1+1 \\# a\tb', '= 1 + 1 ', '=', '= ', 'x=1', '=1\n+1', '=1+1\n', '=1+1 \\# "0"\n', '\n=1', '=\\# "0"', '=1\\# "0"',
  '=A1\\#0', '=SUM(LEFT)\\# "0%"', '=ROUND(10/3;2) \\# "0,00;(0,00)"', '=1+1 \\# "0" "1"', '=1+1 \\#"0"x', '=1+1 \\# "0"x', '=1+1 \\# x"0"', '=1 \\ 2', '=1\\',
  // dates
  '1/2/2020', '31/12/99', '2020-02-30', '29.02.2024', '1-1-30', '0050-02-29', '31/4/2020', '1 de março de 2020', '2020-1-5', '99/9/9999', '5/5/05', ' 3/3/2003 ', '01.13.2020', '2023-12-31',
  '29/2/2023', '29/2/00', '1/1/29', '1/1/30', '1/1/99', '1.1/2000', '1/1.2000', '1-1.2000', '001/1/2000', '1/001/2000', '1/1/200', '1/1/20000', '2020-001-01', '20200-1-1', '2020/1/1', '2020-1-1-1',
  '0000-01-01', '0099-12-31', '0100-01-01', '1/1/0000', '31/12/9999', '30/2/2000', '0/1/2000', '1/0/2000', '32/1/2000', '1/13/2000', '٣/٣/٢٠٢٠', '1/1/2020\n', '\n1/1/2020', '1/\n1/2020', '1 /1/2020',
];
