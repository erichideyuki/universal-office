// Parity capture of uo-numfmt: the frozen TS number-format engine (shared/charts/dist/numfmt.js of
// the checkout UO_TS_ROOT, default the ts-ref-092 worktree) over
//   (a) the 1,709 Excel-measured oracle cases (numfmt-oracle.json),
//   (b) the cases of numfmt.test.ts (the formatValue/formatValueParts/formatGeneral/decimalDigits
//       assertions, re-stated here with the same inputs),
//   (c) a seeded differential generator: format codes x values x locales.
//
//   node core/crates/uo-numfmt/js/capture-numfmt.mjs [--out <file>] [--ts-root <dir>] [--n <codes>] [--seed <n>]
//
// Writes core/crates/uo-numfmt/tests/captures/numfmt.capture, replayed by tests/parity.rs. Record
// kinds (the input is part of the record, so the replay needs no generator):
//   k<N>   one format code with several values: code (UTF-16 units in hex), loc, opts (d1904, zeros,
//          width as f64 bits), then for each value i: v<i> (n<f64 bits> | s<units>), t<i> (the
//          formatValue text as a JSON string), c<i> (colour or -) and p<i> (formatValueParts, see
//          `parts` below)
//   g<N>   formatGeneral(x, loc, budget): x, loc, b, out
//   d<N>   decimalDigits(x, n): x, n, out (integer part '|' fraction)
//   b<N>   builtinFormatCode(id, loc): id, loc, out
//   s<N>   serialToDate(x).getTime() and its UTC fields
//   o<N>   an oracle case: code, v, loc, ts (TS text), xl (the Excel text, JSON)
// `parts`: the parts joined by a space, each T<json string> | S<unit> | F<unit> | G<f64 bits>,
// followed by `!<colour>` and `#` for overflow.
import { readFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { CaptureWriter, Rng, importFrom } from '../../uo-testkit/js/testkit.mjs';

const here = dirname(fileURLToPath(import.meta.url));
const crate = join(here, '..');
const argv = process.argv.slice(2);
const opt = (name) => { const i = argv.indexOf(name); return i >= 0 ? argv[i + 1] : undefined; };
const tsRoot = opt('--ts-root') ?? process.env.UO_TS_ROOT ?? 'D:/Projects/OfficeNext/.claude/worktrees/ts-ref-092';
const out = opt('--out') ?? join(crate, 'tests/captures/numfmt.capture');
const N_CODES = Number(opt('--n') ?? 9000);
const SEED = Number(opt('--seed') ?? 0x5eed0001); // other seeds: local runs only (UO_NUMFMT_BIG=1, tests/parity.rs)

const nf = await importFrom(tsRoot, 'shared/charts/dist/numfmt.js');
const { formatValue, formatValueParts, formatGeneral, decimalDigits, builtinFormatCode, serialToDate, PT_BR } = nf;

// ---------------------------------------------------------------- locales (mirrored by tests/parity.rs)
const clone = (o) => JSON.parse(JSON.stringify(o));
const EN = {
  decimal: '.', group: ',',
  monthsShort: ['Jan', 'Feb', 'Mar', 'Apr', 'May', 'Jun', 'Jul', 'Aug', 'Sep', 'Oct', 'Nov', 'Dec'],
  monthsLong: ['January', 'February', 'March', 'April', 'May', 'June', 'July', 'August', 'September', 'October', 'November', 'December'],
  daysShort: ['Sun', 'Mon', 'Tue', 'Wed', 'Thu', 'Fri', 'Sat'],
  daysLong: ['Sunday', 'Monday', 'Tuesday', 'Wednesday', 'Thursday', 'Friday', 'Saturday'],
  ampm: ['AM', 'PM'],
  sysDate: 'dddd, mmmm d, yyyy', sysTime: 'h:mm:ss AM/PM',
};
const ODD = {
  decimal: '::', group: "'",
  monthsShort: Array.from({ length: 12 }, (_, i) => `m${i + 1}`),
  monthsLong: Array.from({ length: 12 }, (_, i) => `month-${i + 1}`),
  daysShort: Array.from({ length: 7 }, (_, i) => `d${i}`),
  daysLong: Array.from({ length: 7 }, (_, i) => `day-${i}`),
  ampm: ['vm', 'Nm'],
  builtins: { 14: 'dd-mm', 5: '0.0' },
  sysDate: 'yyyy"/"mm', sysTime: '[$-409]hh',
};
const BLANK = { ...clone(EN), decimal: '', group: '' };
const LOCALES = { ptbr: PT_BR, ptclone: clone(PT_BR), en: EN, odd: ODD, blank: BLANK };
const LOC_NAMES = ['ptbr', 'ptclone', 'en', 'odd', 'blank'];
const locOf = (name) => (name === 'default' ? undefined : LOCALES[name]);

// ---------------------------------------------------------------- encodings
const hex = (n) => n.toString(16);
const units = (s) => (s.length === 0 ? '-' : Array.from({ length: s.length }, (_, i) => hex(s.charCodeAt(i))).join(','));
const f64 = (x) => { const v = new DataView(new ArrayBuffer(8)); v.setFloat64(0, Number.isNaN(x) ? NaN : x); return v.getBigUint64(0).toString(16).padStart(16, '0'); };
const val = (v) => (typeof v === 'string' ? 's' + units(v) : 'n' + f64(v));
const json = (s) => JSON.stringify(s);
const partsText = (r) => {
  const ps = r.parts.map((p) => (p.t === 'text' ? 'T' + json(p.s) : p.t === 'skip' ? 'S' + p.ch.charCodeAt(0) : p.t === 'fill' ? 'F' + p.ch.charCodeAt(0) : 'G' + f64(p.v)));
  return ps.join(' ') + (r.color ? '!' + r.color : '') + (r.overflow ? '#' : '');
};
const optsText = (o) => `${o.date1904 ? 1 : 0}${o.showZeros === false ? 0 : 1}${o.colWidthChars !== undefined ? ',' + f64(o.colWidthChars) : ''}`;

const cap = new CaptureWriter(['uo-numfmt parity capture: shared/charts/dist/numfmt.js (frozen TS), see js/capture-numfmt.mjs']);
cap.meta('ts-root-note', 'numfmt.ts of the ts-ref-092 worktree');
let kseq = 0;
const stats = { codes: 0, cases: 0 };

/** One code with its values: the TS results. */
function addCode(code, values, locName = 'default', o = {}) {
  const key = 'k' + kseq++;
  cap.set(key, 'code', code === undefined ? '~' : units(code));
  cap.set(key, 'loc', locName);
  cap.set(key, 'opts', optsText(o));
  const opts = { ...o, locale: locOf(locName) };
  values.forEach((v, i) => {
    const t = formatValue(v, code, opts);
    const p = formatValueParts(v, code, opts);
    cap.set(key, `v${i}`, val(v));
    cap.set(key, `t${i}`, json(t.text));
    cap.set(key, `c${i}`, t.color ?? '-');
    cap.set(key, `p${i}`, partsText(p));
    stats.cases++;
  });
  stats.codes++;
}

// ---------------------------------------------------------------- (a) the oracle
const oracleDir = join(tsRoot, 'shared/charts/src');
const oracle = JSON.parse(readFileSync(join(oracleDir, 'numfmt-oracle.json'), 'utf8'));
let oseq = 0;
for (const c of oracle) {
  const code = c.fmt.startsWith('builtin:') ? builtinFormatCode(+c.fmt.slice(8)) : c.fmt;
  const key = 'o' + oseq++;
  cap.set(key, 'code', code === undefined ? '~' : units(code));
  cap.set(key, 'v', val(c.v));
  cap.set(key, 'ts', json(formatValue(c.v, code).text));
  cap.set(key, 'xl', json(c.text));
}
cap.meta('oracle-cases', oracle.length);

// ---------------------------------------------------------------- (b) numfmt.test.ts
{
  const T = (code, v, o = {}) => addCode(code, [v], 'default', o);
  // partes para quem conhece a largura
  T('_(* #,##0.00_);_(* (#,##0.00);_(* "-"??_);_(@_)', 1234.5);
  T('*x0*y', 5);
  T('General_)', 5);
  T('General', 7.26126896);
  T('[<10]"a"0;[<20]"b"0', 25);
  T('dd/mm/yyyy', -1);
  T('dd/mm/yyyy', 3000000);
  T('General', 0, { showZeros: false });
  T('0.00', 0, { showZeros: false });
  T('0;-0;"zero"', 0, { showZeros: false });
  T('0.00', -0.0001, { showZeros: false });
  T('_-* #,##0.00_-;-* #,##0.00_-;_-* "-"??_-;_-@_-', 0);
  // negativo em seção só de literais e tempo decorrido
  for (const [v, c] of [[-5, '"x"'], [-0.5, 'x'], [-5, '""'], [-5, '-'], [5, '"x"'], [-5, '"x";"y"'], [-5, 'x;'],
    [-5, '[h]'], [-0.5, '[mm]'], [-1.25, '[s]'], [-1.25, '[h] "x"'], [-1.25, '[h]:mm'], [-0.5, 'h:mm']]) T(c, v);
  // científico e frações
  const table = [
    ['#,#E+#', 123456.789], ['#,#E+#', 0.0000123456789], ['#,#E+#', 0], ['#,#E-#', 0], ['#,##E+0', 123456.789],
    ['#,#|E+|#', 123456.789], ['0E+"x"0', 123456.789], ['0E+"x"0', 0],
    ['# #/#', 0.75], ['# #/#', 2], ['# #/#', -0.75], ['# 0/#', 0], ['# 0/#', 2], ['# #?/#', 0], ['# #/0?', 2], ['# #/?0', 2],
    ['# #/000', 0.75], ['# #/?00', 0.6875], ['# #/??', 0.75], ['# ?0#/000', 0], ['# ?0#/000', 23.75],
    ['#\\:? ?=/=#', 23.75], ['#\\:? ?=/=#', 0.75], ['#_#/#', 3.75], ['0/\\x#', 12.5], ['0/"x"10', 3.75],
    ['# ?/?', 0.1], ['# ?/?', 0.3], ['# ?/??', 3.14159265], ['# ?/???', 0.123], ['# ?/???', 0.333],
  ];
  for (const [c, v] of table) T(c, v);
  // tempo decorrido
  for (const [v, c, d] of [[3.14159, '[h] h'], [3.14159, '[s] ss'], [0.123456, '[h] s'], [1.75, '[m] mm:ss'],
    [3.14159, 's:m" @ hour "[hh]'], [0.123456, '[s] m'], [0.123456, '[h]:mm h'], [0.123456, 'ss.00 [h]'],
    [1, 'h [m]'], [1, '[h] [m]'], [-0.5, '[h] "x" h'], [-0.5, '[h]:mm'],
    [-0.08546296296296296, 'dd/mm/yyyy', true], [-0.08546296296296296, 's [h]', true]]) T(c, v, { date1904: !!d });
  // formatGeneral / decimalDigits
  let g = 0;
  const G = (x, loc, b) => { const k = 'g' + g++; cap.set(k, 'x', val(x)); cap.set(k, 'loc', loc); cap.set(k, 'b', b === undefined ? '~' : String(b)); cap.set(k, 'out', json(formatGeneral(x, locOf(loc), b))); };
  for (const [x, b] of [[7.26126896, 3], [7.26126896, 2], [1234567.891, 7], [1234567.891, 6], [12345678901, 10], [0.000123456, 5], [0.000123456, 6], [99.999999, 8]]) G(x, 'default', b);
  let d = 0;
  const D = (x, n) => { const k = 'd' + d++; cap.set(k, 'x', val(x)); cap.set(k, 'n', String(n)); const r = decimalDigits(x, n); cap.set(k, 'out', `${r[0]}|${r[1]}`); };
  for (const [x, n] of [[0.005, 2], [2.5, 0], [347.99232604148267, 15], [9.995, 2]]) D(x, n);
  cap.meta('test-file-cases', stats.cases);
}

// ---------------------------------------------------------------- (c) the differential generator
const rng = new Rng(SEED);
const pick = (a) => a[rng.below(a.length)];
const chance = (n) => rng.below(n) === 0;
const rnd01 = () => (rng.next() * 4294967296 + rng.next()) / 18446744073709551616;

const SPECIAL = [0, -0, 1, -1, 0.5, -0.5, 0.25, 0.75, 1.5, 2.5, -2.5, 0.005, 9.995, 99.5, 999.5, 0.999999, 1e-10, 123456789012345678, 1e21, 1e22, 1e300, -1e300,
  5e-324, 1.7976931348623157e308, NaN, Infinity, -Infinity, 0.1, 0.2, 0.3, 0.7, 1 / 3, 2 / 3, 3.14159265358979, 12345.6789, 1234567.891, 43101.5, 36526.999999,
  0.99999999, 59, 60, 61, 0.5 / 24, 1.75, 2958465, 2958465.9999, 2958466, 44927.25, 25569, 1461, 31, 365.25, 0.000011574074074074, 1e15, 123456789012345,
  0.1 + 0.2, 0.4999971064814, 0.99999, 1e-5, 12.5, 100, 1000, 999999.5, 0.045, 1.005, 4.35, 62, 5551234, 209496, -209496, 0.0000123456789, 123456.789, 3.75, 23.75,
  16, 4, 0.6875, 0.333, 0.123, 12345, 123456789, 0.00009, 9.9e-5, 1e-4, 8.9999999e15, 9007199254740993, 1462, 2958465.5, 61.5, 44.9999999];
const TEXTS = ['', 'abc', 'Olá mundo', '  x ', '😀', 'a\tb', '\ud800', 'x\ud83dy', 'ÅKelvin K', '12', 'General', '@', '"q"'];
function numValue() {
  const k = rng.below(10);
  let v;
  if (k < 3) v = pick(SPECIAL);
  else if (k === 3) v = rng.below(2000000) - 1000;
  else if (k === 4) v = Math.round(rnd01() * 1e6) / 10 ** (1 + rng.below(6));
  else if (k === 5) v = 1 + rng.below(80000) + (chance(2) ? rnd01() : Math.round(rnd01() * 86400) / 86400); // dates/times
  else if (k === 6) v = rnd01() * 10 ** (rng.below(30) - 12);
  else if (k === 7) v = rng.below(1000) / 1000;
  else if (k === 8) v = (rng.below(2000) - 1000) / 8;
  else v = Math.round(rnd01() * 1e15) / 10 ** rng.below(16);
  if (chance(5) && !Number.isNaN(v)) v = -v;
  return v;
}
const values = (n) => { const a = []; for (let i = 0; i < n; i++) a.push(numValue()); if (chance(6)) a.push(pick(TEXTS)); return a; };

const SEP = [' ', '-', '/', ':', '.', ',', '(', ')', '$', 'x', 'é', '😀', '\\-', '\\ ', '"s"', '" de "', '_)', '_-', '* ', '*-', '\\E', 'm', '%'];
const COLORS = ['[Red]', '[Blue]', '[Color5]', '[Color56]', '[Color57]', '[Color0]', '[GREEN]', '[ColoR 12]', '[Black]', '[White]', '[blacK]', '[Cyan]', '[Magenta]', '[Yellow]'];
const CONDS = ['[<100]', '[>=5]', '[=0]', '[<>3]', '[<=-1]', '[>0]', '[<0]', '[>1e3]', '[>= 5]', '[<1.5e-1]', '[=-5]', '[>.5]', '[<.]', '[>10]', '[<=9999999]'];
const LCIDS = ['[$-409]', '[$-416]', '[$-F800]', '[$-F400]', '[$€-2]', '[$-40C]', '[$-411]', '[$-407]', '[$-40A]', '[$-410]', '[$USD]', '[$-0409]', '[$-1F800]', '[$R$-416]', '[$-]', '[$-xyz]', '[$-x-sysdate]', '[$-FFFFFFFFFFFFFFFFFF]'];
const maybe = (n, s) => (chance(n) ? s : '');

function intPattern() {
  const n = 1 + rng.below(7);
  let s = '';
  for (let i = 0; i < n; i++) {
    s += pick(['0', '0', '#', '#', '?', '0', '#']);
    if (chance(4)) s += pick([',', ',', '.', '\\ ', ' ', '-', '"x"', '_)', '*x']);
  }
  return s;
}
function numberFormat() {
  let s = maybe(5, pick(['"R$ "', '$', '(', '\\-', '"kg "', '_(']));
  s += intPattern();
  if (chance(2)) s += '.' + pick(['0', '00', '000', '#', '##', '?', '0#', '??', '0000000000000000', '#0']).repeat(1);
  if (chance(5)) s += pick([',', ',,', ',,,']);
  if (chance(5)) s += pick(['%', '%%']);
  if (chance(6)) s += pick(['E+0', 'E+00', 'E-0', 'e+0', 'E+000', 'E+#', 'E+"x"0', 'E+|0']);
  s += maybe(5, pick([')', ' "un"', '_)', '*-', ' %', '"x"']));
  return s;
}
function fractionFormat() {
  return maybe(4, pick(['0 ', '# ', '?', '#', '#\\ ', '# #', '??'])) + pick(['?', '#', '0', '??', '###', '0?', '?0', '000', '#?']) + maybe(8, pick(['"x"', '\\:', '_#', '=', '%'])) + '/' +
    maybe(8, pick(['=', '"x"', '_)'])) + pick(['?', '#', '0', '??', '10', '100', '1', '2', '16', '"x"10', '0?', '?00', '8', '99999']);
}
const DT = ['yyyy', 'yy', 'y', 'yyy', 'mmmm', 'mmm', 'mm', 'm', 'mmmmm', 'dddd', 'ddd', 'dd', 'd', 'hh', 'h', 'ss', 's', 'AM/PM', 'am/pm', 'A/P', 'a/p', 'a/P', '[h]', '[hh]', '[mm]', '[m]', '[ss]', '[s]', '.0', '.00', '.000', '.0000', 'e', 'ee', 'g', 'gg', 'b', 'bb', 'bbbb', 'aaa', 'aaaa', 'aa', 'General', '0', '#', '@', 'E+', ':'];
function dateFormat() {
  const n = 1 + rng.below(6);
  let s = '';
  for (let i = 0; i < n; i++) {
    s += pick(DT);
    if (chance(2)) s += pick(['/', '-', ' ', ':', ', ', '.', '" "', '\\ ', 'T', '"h"', '_x', '*x']);
  }
  return s;
}
function chaos() {
  const parts = [...DT, ...SEP, '0', '#', '?', '.', ',', '%', '/', '@', ';', '"', '[', ']', '\\', '_', '*', 'E+', 'E-', 'General'];
  const n = 1 + rng.below(10);
  let s = '';
  for (let i = 0; i < n; i++) s += pick(parts);
  return s;
}
function section(kind) {
  const k = kind ?? rng.below(8);
  let body = k < 3 ? numberFormat() : k === 3 ? fractionFormat() : k < 6 ? dateFormat() : k === 6 ? chaos() : pick(['@', '"t "@', '@" x"', '@@', '*x@', '_(@_)', '"a"', '', 'General', '0', '@;@', '"-"']);
  return maybe(3, pick(COLORS)) + maybe(5, pick(COND_OR(k))) + maybe(8, pick(LCIDS)) + body + maybe(12, pick(COLORS));
}
function COND_OR() { return CONDS; }
function code() {
  const k = rng.below(20);
  if (k < 6) return numberFormat();
  if (k < 8) return fractionFormat();
  if (k < 11) return dateFormat();
  if (k < 14) return [section(), section(), section(), section()].slice(0, 1 + rng.below(4)).join(';');
  if (k < 16) return chaos();
  if (k === 16) return pick(COLORS) + pick(CONDS) + maybe(2, ';') + maybe(2, pick(COLORS));
  if (k === 17) return pick(CONDS) + numberFormat() + ';' + pick(CONDS) + numberFormat() + maybe(2, ';' + numberFormat());
  if (k === 18) return numberFormat() + ';' + numberFormat() + ';' + numberFormat() + ';' + pick(['@', '"t "@', '@" x"', '*x@']);
  return pick(['General', 'general', 'GENERAL', '0.00', '#,##0', '0%', 'h:mm:ss', 'yyyy-mm-dd', '@', '', '0.00E+00', '##0.0E+0', '# ?/?', '[h]:mm:ss', 'mm:ss.0', '0.', '.0', '#.#', '0,', '0,,', '#,##0,', '0.0,,"M"']);
}
const OPTS = [{}, {}, {}, {}, {}, { date1904: true }, { showZeros: false }, { colWidthChars: 12 }, { colWidthChars: 0 }, { colWidthChars: 3.7 }, { date1904: true, colWidthChars: 20 }, { showZeros: false, colWidthChars: 8 }];
for (let i = 0; i < N_CODES; i++) {
  const loc = chance(3) ? pick(['default', ...LOC_NAMES]) : 'default';
  addCode(code(), values(4 + rng.below(4)), loc, pick(OPTS));
}
// astral and lone-surrogate corner cases of the code-unit indexing
for (const c of ['_😀0', '*😀0', '\\😀0', '0*😀', '"😀"0', '[$😀-409]0', '0_\ud83d', '*\ude00x', 'yyyy_😀mm', '@_😀', '0.0"\ud800"', '[😀]0', 'm\ud83d', '[K]0', '[blacK]0', '[constructorx]0'])
  addCode(c, [1234.5, -2, 0, 43101.5], 'default', { colWidthChars: 9 });
// the TS key lookups that are not a colour: [constructor]/[__proto__] store a non-string in the TS (not compared)
// ---------------------------------------------------------------- general / digits / builtins / serials
{
  let g = 1000;
  for (let i = 0; i < 4000; i++) {
    const x = numValue();
    const k = 'g' + g++;
    const loc = chance(3) ? pick(LOC_NAMES) : 'default';
    const b = chance(4) ? undefined : rng.below(16) - 1;
    cap.set(k, 'x', val(x)); cap.set(k, 'loc', loc); cap.set(k, 'b', b === undefined ? '~' : String(b));
    cap.set(k, 'out', json(formatGeneral(x, locOf(loc), b)));
  }
  let d = 1000;
  for (let i = 0; i < 4000; i++) {
    const x = numValue();
    const n = rng.below(20);
    const k = 'd' + d++;
    cap.set(k, 'x', val(x)); cap.set(k, 'n', String(n));
    const r = decimalDigits(x, n);
    cap.set(k, 'out', `${r[0]}|${r[1]}`);
  }
  let b = 0;
  for (const loc of ['default', ...LOC_NAMES]) {
    for (let id = 0; id <= 90; id++) {
      const k = 'b' + b++;
      cap.set(k, 'id', String(id)); cap.set(k, 'loc', loc);
      const r = builtinFormatCode(id, locOf(loc));
      cap.set(k, 'out', r === undefined ? '-' : json(r));
    }
  }
  let s = 0;
  const ser = [0, 1, 25569, 36526.5, 59.5, 60, 61, -1, 2958465, 2958466, 1e5, 4e6, 1e8, 1e9, 1e300, NaN, Infinity, -Infinity, -2209161600, 0.3, 44927.123456789, 100000000, 100000001, 1e8 + 1.5, -1e8, -100000001, -25569, -25570, 1.5e-9];
  for (let i = 0; i < 300; i++) ser.push(numValue());
  for (const x of ser) {
    const k = 's' + s++;
    const t = serialToDate(x).getTime();
    cap.set(k, 'x', val(x));
    cap.set(k, 't', f64(t));
    if (Number.isNaN(t)) cap.set(k, 'f', '-');
    else { const d = serialToDate(x); cap.set(k, 'f', [d.getUTCFullYear(), d.getUTCMonth(), d.getUTCDate(), d.getUTCDay(), d.getUTCHours(), d.getUTCMinutes(), d.getUTCSeconds(), d.getUTCMilliseconds()].join(',')); }
  }
}
cap.meta('codes', stats.codes);
cap.meta('format-cases', stats.cases);
cap.meta('seed', '0x' + SEED.toString(16));
cap.write(out);
console.log(`wrote ${out}: ${stats.codes} codes, ${stats.cases} formatValue cases, ${oracle.length} oracle cases`);
