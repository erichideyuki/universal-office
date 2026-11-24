// Parity capture group `a1` (frontier A1, record core/docs/modules/uo-calc.md "A1"): the per-parameter traps and
// seeded inputs of every A1 entry point of the frozen TS (SKILL.md "Capture sources" 2 and 3) — coerce, collate/
// collation, criteria, dates, stats, special-functions, functions/libm, functions/math-util — called directly on the
// calc dist with the clock frozen (2025-01-15 12:00 UTC: `new Date().getFullYear()` = 2025).
//
//   node core/crates/uo-calc/js/capture-a1.mjs                    # CI -> tests/captures/a1.capture.gz (gzip of the capture text)
//   node core/crates/uo-calc/js/capture-a1.mjs --big --out core/target/sheet-cap/a1-big.capture.gz   # 10x the seeded inputs
//
// Records `<fn>#<i>`, facets: `in` the arguments as a JSON array of value texts, `out` the result text. Value texts
// (lib/common.mjs makeValueText, plus): n<f64 hex>, s<JSON string>, b1/b0, _ (null), e<code>, u (undefined),
// L<locale key> (en, pt, de = a host locale with NBSP group, "." time separator, YMD, € and German names; xx = an
// empty group separator, a "٫" decimal and a "h" time separator), N[<n>,…] number lists, V[<v>,…] scalar lists.
// Structured results: parts `d<n>;t<n>` (absent part = `-`), ymd `y<n>;m<n>;d<n>`, hms likewise, pairs `<n>;<n>`,
// criteria `<blank 0|1>:<one bit per value of the @crit-values list>`, collect `N[...]` or the error.
process.env.TZ = 'UTC';
const { join, dirname } = await import('node:path');
const { mkdirSync, writeFileSync } = await import('node:fs');
const { gzipSync } = await import('node:zlib');
const { pathToFileURL } = await import('node:url');
const { CaptureWriter, Rng } = await import('../../uo-testkit/js/testkit.mjs');
const C = await import('./lib/common.mjs');

const a = C.args();
const root = C.tsRoot(a);
const big = a.flag('--big');
const out = a.opt('--out', join(C.capturesDir, 'a1.capture.gz'));
const SCALE = big ? 10 : 1;

// frozen clock (the recorder's): currentYear() = 2025
const FIXED = Date.UTC(2025, 0, 15, 12, 0, 0);
const RealDate = Date;
globalThis.Date = class extends RealDate {
  constructor(...x) { if (x.length === 0) super(FIXED); else super(...x); }
  static now() { return FIXED; }
};

const base = join(root, 'sheet', 'packages', 'calc', 'dist');
const imp = (p) => import(pathToFileURL(join(base, p)).href);
const values = await imp('values.js');
const coerce = await imp('coerce.js');
const collate = await imp('collate.js');
const collation = await imp('collation.js');
const criteria = await imp('criteria.js');
const dates = await imp('dates.js');
const stats = await imp('stats.js');
const special = await imp('special-functions.js');
const libm = await imp('functions/libm.js');
const mu = await imp('functions/math-util.js');
const fn = await imp('fn.js');
const { CalcError, ArrayValue, RefValue } = values;
const vt0 = C.makeValueText(values);
const vt = (v) => (v === undefined ? 'u' : vt0(v));
const nt = (x) => `n${C.f64hex(x)}`;

const DE = {
  id: 'de-DE', decimal: ',', group: ' ', trueText: 'WAHR', falseText: 'FALSCH', dateOrder: 'YMD', dateSep: '.', timeSep: '.',
  am: 'vorm.', pm: 'nachm.',
  monthNames: ['Januar', 'Februar', 'März', 'April', 'Mai', 'Juni', 'Juli', 'August', 'September', 'Oktober', 'November', 'Dezember'],
  monthAbbr: ['Jan', 'Feb', 'Mär', 'Apr', 'Mai', 'Jun', 'Jul', 'Aug', 'Sep', 'Okt', 'Nov', 'Dez'],
  dayNames: ['Sonntag', 'Montag', 'Dienstag', 'Mittwoch', 'Donnerstag', 'Freitag', 'Samstag'],
  dayAbbr: ['So', 'Mo', 'Di', 'Mi', 'Do', 'Fr', 'Sa'],
  currency: '€', generalName: 'Standard',
};
const XX = { ...fn.EN_US, id: 'xx', decimal: '٫', group: '', timeSep: 'h', currency: 'Kč', dateOrder: 'DMY', am: 'dop.', pm: 'odp.' };
const LOCALES = { en: fn.EN_US, pt: fn.PT_BR, de: DE, xx: XX };

const w = new CaptureWriter([
  'a1 (frontier A1): direct calls of the A1 entry points of the frozen TS with traps and seeded inputs, see core/crates/uo-calc/js/capture-a1.mjs',
]);
C.stamp(w, root, { mode: big ? 'big' : 'ci', locales: Object.keys(LOCALES).join(','), 'locale.de': JSON.stringify(DE), 'locale.xx': JSON.stringify(XX) });
const counts = new Map();
let total = 0;
function rec(name, args, result) {
  const i = counts.get(name) ?? 0;
  counts.set(name, i + 1);
  w.set(`${name}#${i}`, 'in', JSON.stringify(args));
  w.set(`${name}#${i}`, 'out', result);
  total++;
}
function call(name, args, f) {
  let r;
  try { r = f(); } catch (e) { r = `throw:${C.oneLine(e?.message ?? e, 80)}`; }
  rec(name, args, r);
}

const rng = new Rng(0xa1a1_0001);
const pick = (xs) => xs[rng.below(xs.length)];
const unit = () => rng.next() / 4294967296;
const f64 = (hi, lo) => { const v = new DataView(new ArrayBuffer(8)); v.setUint32(0, hi); v.setUint32(4, lo); return v.getFloat64(0); };

// ---------------------------------------------------------------- number generators
const TRAPS = [NaN, -0, 0, Infinity, -Infinity, 1e21, 1e-7, 2 ** 53 + 2, 0.30000000000000004, 5e-324, 2.2250738585072014e-308,
  1, -1, 0.5, 1.5, 2.5, -2.5, 1e15, 1e16, 123456789012345680, 0.1, 0.7, 1e-300, 1.7976931348623157e308, 4294967296, 60, 61, 1462, 2958465];
function rnum() {
  switch (rng.below(9)) {
    case 0: return f64(rng.next(), rng.next());
    case 1: return (rng.below(2) ? -1 : 1) * rng.below(100000) / 10 ** rng.below(8);
    case 2: return rng.below(4000000) - 1000000;
    case 3: return pick(TRAPS);
    case 4: return (unit() - 0.5) * 10 ** (rng.below(40) - 20);
    case 5: { const x = rng.below(1000) / 10; return x + (rng.below(2) ? 1 : -1) * rng.below(8) * Number.EPSILON * Math.max(1, x); }
    case 6: return unit() * 50;
    case 7: return 10 ** (rng.below(600) - 300) * (1 + unit());
    default: return rng.below(30);
  }
}
const pos = () => { const x = Math.abs(rnum()); return Number.isNaN(x) ? 1 : x; };

// ---------------------------------------------------------------- string generators
const MONTHS = [...fn.PT_BR.monthNames, ...fn.PT_BR.monthAbbr, ...fn.EN_US.monthNames, ...fn.EN_US.monthAbbr, ...DE.monthNames, 'MARÇO', 'Fev', 'JAN'];
const TOK = ['0', '1', '9', '12', '123', '1234', '2024', '000', '31', '00', '1900', '99999', '12345678901234567', '0001'];
const SEP = ['.', ',', ' ', '  ', ' ', '\t', "'", '/', '-', ':', '%', 'e', 'E', 'e+', 'E-', '(', ')', '+', '-', 'R$', '$', '€', 'R$ ', '٫', 'h', 'Kč', 'AM', 'PM', ' AM', ' p', ' a', 'vorm.', ' nachm.', ', '];
const ODD = ['\ud800', '😀', 'é', 'ß', 'İ', 'ı', '̀', 'x', 'TRUE', 'falso', 'VERDADEIRO', '#N/D', '#N/A', '٣', '١٢', '*', '?', '~', '<', '>', '=', ''];
function rtext() {
  const n = 1 + rng.below(6);
  let s = '';
  for (let i = 0; i < n; i++) {
    const k = rng.below(10);
    s += k < 4 ? pick(TOK) : k < 7 ? pick(SEP) : k < 8 ? pick(MONTHS) : k < 9 ? String(rng.below(32)) : pick(ODD);
  }
  if (rng.below(8) === 0) s = ' ' + s;
  if (rng.below(8) === 0) s += ' ';
  return s;
}
function rformatted(loc) {
  // a number written in the locale (grouping, decimal, currency, percent, sign, exponent)
  const x = rnum();
  if (!Number.isFinite(x)) return String(x);
  let [i, f] = Math.abs(x).toFixed(rng.below(4)).split('.');
  if (rng.below(2) && loc.group !== undefined) i = i.replace(/\B(?=(\d{3})+(?!\d))/g, loc.group);
  let s = f ? `${i}${loc.decimal}${f}` : i;
  if (rng.below(6) === 0) s += pick(['e3', 'E-2', 'e+400', 'e-320']);
  if (x < 0) s = pick(['-', '(', '- ']) + s + (s.startsWith('(') ? ')' : '');
  if (rng.below(4) === 0) s = pick([loc.currency, loc.currency + ' ', '-' + loc.currency]) + s;
  if (rng.below(5) === 0) s = rng.below(2) ? s + '%' : '%' + s;
  return s;
}
function rdate(loc) {
  const y = pick([1899, 1900, 1904, 1999, 2024, 2025, 9999, 24, 29, 30, 99, 5]);
  const m = 1 + rng.below(13), d = rng.below(33);
  const sep = pick(['/', '-', ' ', '.', ' / ']);
  const mon = rng.below(3) === 0 ? pick([...loc.monthNames, ...loc.monthAbbr]) : String(m);
  const parts = loc.dateOrder === 'MDY' ? [mon, d, y] : loc.dateOrder === 'YMD' ? [y, mon, d] : [d, mon, y];
  let s = rng.below(5) === 0 ? parts.slice(0, 2).join(sep) : parts.join(sep);
  if (rng.below(3) === 0) {
    const t = `${rng.below(30)}${loc.timeSep}${String(rng.below(70)).padStart(2, '0')}` + (rng.below(2) ? `${loc.timeSep}${rng.below(61)}` : '') +
      (rng.below(3) === 0 ? `${loc.decimal}${rng.below(1000)}` : '') + (rng.below(3) === 0 ? pick([' AM', ' PM', ' a', ' P', ` ${loc.am}`, ` ${loc.pm}`]) : '');
    s = rng.below(2) ? `${s} ${t}` : `${t} ${s}`;
  }
  return s;
}
const FIXED_TEXTS = ['', ' ', '1', ' 1 ', '+1', '-1', '(1)', '-(1)', '(-1)', '1.234,5', '1,234.5', '1.2345.678', '1.23', '1.0', '1.', ',5', '1,', '.5',
  'R$ 1', '-R$1', 'R$ -1', 'R$(1)', '(R$ 1)', '$1', '50%', '%50', '5%%', 'R$5%', '1,5e2', '1e400', '1e-400', '2082e-310%', '0.001', '00.000', '-0.000',
  '0.000,5', '01.000', '0001.000', '9999999999999999', '1 1/2', '-1 1/2', '1 1/0', '1 32767/32767', '1 32768/2', '٣٫٥', '١٢٣', '\t1', '1 ',
  '10:30', '10 : 30', '23:9999', '1:60:00', '24:60', '0:60:60', '12:', '10 PM', '13 PM', '12 AM', '1:30,5', '1:30.5', '10:30:15,123', '10:30 01/02/2024',
  '31/12/2024', '2024-12-31', '1/2/24 3:04 PM', '31-jan-2024', '1 fev 2024', '1 jan, 2024', '1-fev', 'fev-24', 'jan-45', 'jan 1 2024', '1jan2024',
  'fev1941', 'jan, 2024', 'mar,3412', '2/24', '12/24', '1-2024', '29/2/1900', '29/2/1901', '1/1/1899', '31/12/9999', '1/1/10000', '0/0', '13/13',
  '1e', 'e1', '1e+', '1E5', '+', '-', '%', '(', ')', '()', 'abc', 'TRUE', 'VERDADEIRO', '\ud800', '1\ud8001', 'Mär 3', '3. März 2024', '2024.03.05'];

// ---------------------------------------------------------------- coerce
for (const [lk, loc] of Object.entries(LOCALES)) {
  const texts = [...FIXED_TEXTS];
  for (let i = 0; i < 700 * SCALE; i++) texts.push(rtext());
  for (let i = 0; i < 300 * SCALE; i++) texts.push(rformatted(loc));
  for (let i = 0; i < 300 * SCALE; i++) texts.push(rdate(loc));
  for (const t of texts) {
    const d1904 = rng.below(5) === 0;
    call('parseNumberText', [vt(t), `L${lk}`, d1904 ? 'b1' : 'b0'], () => vt(coerce.parseNumberText(t, loc, d1904)));
  }
  for (let i = 0; i < 150 * SCALE; i++) {
    const t = rng.below(2) ? rdate(loc) : rtext();
    const year = pick([2024, 2025, 1900, 2100]);
    const d1904 = rng.below(4) === 0;
    call('parseDateTimeParts', [vt(t), `L${lk}`, d1904 ? 'b1' : 'b0', nt(year)], () => {
      const p = coerce.parseDateTimeParts(t, loc, d1904, year);
      return p === undefined ? 'u' : `d${p.date === undefined ? '-' : nt(p.date)};t${p.time === undefined ? '-' : nt(p.time)}`;
    });
  }
  for (let i = 0; i < 200 * SCALE; i++) {
    const x = rnum();
    call('numberToText', [nt(x), `L${lk}`], () => vt(coerce.numberToText(x, loc)));
  }
  const bools = ['TRUE', ' true ', 'False', 'VERDADEIRO', 'verdadeiro', 'FALSO', 'WAHR', 'falsch', 'x', '', '\tTRUE ', 'TRUEx'];
  for (const t of [...bools, ...Array.from({ length: 30 * SCALE }, rtext)]) call('toBool', [vt(t), `L${lk}`], () => vt(coerce.toBool(t, loc)));
  const scalars = () => [rnum(), rtext(), rformatted(loc), true, false, null, values.ERR.NA, values.ERR.DIV0];
  for (let i = 0; i < 40 * SCALE; i++) {
    for (const v of scalars()) {
      call('toNumber', [vt(v), `L${lk}`], () => vt(coerce.toNumber(v, loc)));
      call('toText', [vt(v), `L${lk}`], () => vt(coerce.toText(v, loc)));
    }
  }
}

// ---------------------------------------------------------------- order
const TEXT_POOL = ['', 'a', 'A', 'b', 'á', 'à', 'â', 'ä', 'ã', 'å', 'ª', 'a b', 'aa', 'ab', 'ab-', "a'b", 'a-b', 'a--b', '-ab', 'ac', 'ß', 'ss', 'SS', 'æ', 'ae',
  'ﬁ', 'fi', '10', '9', '01', '1', '¹', '½', '!', '~', '£', '€', '+', '<', '±', '×', 'µ', 'Ω', 'ω', 'я', 'Я', 'ア', 'あ', 'ｱ', 'Ａ', 'a', '中', '😀', '\ud800',
  'é', 'é', 'Æble', 'ø', 'o', 'ł', 'ı', 'İ', 'i', 'I', 'ŉ', 'a­b', 'Straße', 'STRASSE', 'Ǆ', 'ǆ', '\u0001', '\u007f', 'a\tb', 'a b',
  "can't", 'cant', 'co-op', 'coop', 'Zürich', 'zurich', 'Ñandú', 'nandu', 'ΑΣ', 'ας'];
function rstr() {
  if (rng.below(3) === 0) return pick(TEXT_POOL);
  let s = '';
  const n = rng.below(6);
  for (let i = 0; i < n; i++) s += rng.below(3) ? pick(TEXT_POOL) : String.fromCharCode(32 + rng.below(95));
  return s;
}
for (let i = 0; i < 2500 * SCALE; i++) {
  const x = rstr(), y = rng.below(4) === 0 ? x.toUpperCase() : rstr();
  call('compareText', [vt(x), vt(y)], () => vt(collation.compareText(x, y)));
  call('sameTextCI', [vt(x), vt(y)], () => vt(coerce.sameTextCI(x, y)));
}
for (let i = 0; i < 200 * SCALE; i++) { const x = rstr(); call('collationKey', [vt(x)], () => vt(collation.collationKey(x))); }
function rscalar() {
  switch (rng.below(6)) {
    case 0: return rnum();
    case 1: return rstr();
    case 2: return rng.below(2) === 0;
    case 3: return null;
    case 4: return pick([values.ERR.NA, values.ERR.VALUE]);
    default: return rng.below(20) - 5;
  }
}
for (let i = 0; i < 1500 * SCALE; i++) {
  const x = rscalar(), y = rscalar();
  call('compareScalars', [vt(x), vt(y)], () => vt(coerce.compareScalars(x, y)));
}
for (let i = 0; i < 1500 * SCALE; i++) {
  const a0 = rnum();
  const b0 = rng.below(3) === 0 ? rnum() : a0 * (1 + (rng.below(60) - 30) * 2 ** -53) + (rng.below(3) === 0 ? (rng.below(5) - 2) * 1e-14 : 0);
  call('compareNumbers', [nt(a0), nt(b0)], () => vt(collate.compareNumbers(a0, b0)));
  call('compare15', [nt(a0), nt(b0)], () => vt(criteria.compare15(a0, b0)));
  call('round15', [nt(a0)], () => vt(collate.round15(a0)));
}

// ---------------------------------------------------------------- criteria
const CRIT_VALUES = [null, '', ' ', 0, 1, -1, 1.5, 2, 3, 10, 100, 1 + 16 * 2 ** -52, 1 + 24 * 2 ** -52, 45658, 45292, 0.4375, '1', ' 1', '01', '1,5', '1.5',
  '01/01/2024', '1/1/2024', '2024-01-01', 'a', 'A', 'abc', 'ABC', 'apple', 'Applesauce', 'b', 'z', 'á', 'ß', 'ss', 'a*', 'a?c', '~', '*', 'x~y', '',
  'TRUE', 'VERDADEIRO', 'FALSO', true, false, values.ERR.NA, values.ERR.DIV0, values.ERR.VALUE, '#N/A', '#N/D', 'R$ 1', '50%', 'abc~', 'abc*', 'çã',
  '\ud800', 'É', 'é'];
w.meta('crit-values', JSON.stringify(CRIT_VALUES.map(vt)));
const CRIT_REST = ['', '1', '2', ' 1', '+1', '1E0', '(1)', 'R$ 1', '50%', '2,5', '2.5', '01/01/2024', '1/1/2024', '2024-01-01', '10:30', 'VERDADEIRO', 'TRUE',
  'FALSO', 'FALSE', 'verdadeiro', 'WAHR', '#DIV/0!', '#N/D', '#N/A', ' #N/A ', '#VALOR!', '#VALUE!', '#nulo!', 'a', 'A', 'abc', 'a*', '*', '?', 'a?c',
  '~*', '~?', '~~', 'a~', 'x~y', '~a*', 'apple', 'Apple', 'ß', 'ss', 'á', 'b', 'z', '*c', '*b*', '1*', '=1', '<1', 'abc~', 'É', '\ud800', '٣'];
const OPS = ['', '=', '<>', '<', '<=', '>', '>=', '==', '=<', '><'];
for (const [lk, loc] of Object.entries(LOCALES)) {
  const crits = [];
  for (const op of OPS) for (const r of CRIT_REST) crits.push(op + r);
  for (let i = 0; i < 100 * SCALE; i++) crits.push(pick(OPS) + rtext());
  crits.push(0, 1, 1.5, true, false, null, values.ERR.NA, values.ERR.DIV0);
  for (const c of crits) {
    for (const prefix of lk === 'pt' || lk === 'en' ? [false, true] : [false]) {
      call(prefix ? 'parseCriterionPrefix' : 'parseCriterion', [vt(c), `L${lk}`], () => {
        const p = criteria.parseCriterion(c, loc, prefix ? { prefix: true } : {});
        return `${p.blank ? 1 : 0}:${CRIT_VALUES.map((v) => (p(v) ? '1' : '0')).join('')}`;
      });
    }
  }
  for (const t of CRIT_REST) call('localeErrorCode', [vt(t), `L${lk}`], () => vt(criteria.localeErrorCode(t, loc)));
}
for (const t of [...CRIT_REST, '~', '~~*', 'a~*?', '**', '']) call('hasWildcard', [vt(t)], () => vt(criteria.hasWildcard(t)));

// ---------------------------------------------------------------- dates
const dnum = () => pick([rnum(), rng.below(20000) - 1000, rng.below(13) + 0.5, -rng.below(40), 1e6, 1e7, 2e6, -1e6, 99, 100, 0, 1900, 2025, 9999, 10000, -0.5, 50.5, NaN]);
for (let i = 0; i < 600 * SCALE; i++) {
  const y = dnum(), m = rng.below(3) ? rng.below(40) - 10 : dnum(), d = rng.below(3) ? rng.below(70) - 20 : dnum(), b = rng.below(4) === 0;
  call('ymdToSerial', [nt(y), nt(m), nt(d), b ? 'b1' : 'b0'], () => vt(dates.ymdToSerial(y, m, d, b)));
  call('excelDate', [nt(y), nt(m), nt(d), b ? 'b1' : 'b0'], () => vt(dates.excelDate(y, m, d, b)));
  call('daysInMonth', [nt(y), nt(m), b ? 'b1' : 'b0'], () => vt(dates.daysInMonth(y, m, b)));
  call('isLeap', [nt(y)], () => vt(dates.isLeap(y)));
}
const snum = () => pick([rnum(), rng.below(3000000) - 10000, unit() * 3000000, rng.below(3000000) + unit(), 0, 60, 61, 59, 1, -1, 2958465, 2958465.99999, 1e16, 9e15]);
for (let i = 0; i < 600 * SCALE; i++) {
  const s = snum(), b = rng.below(4) === 0;
  call('serialToYmd', [nt(s), b ? 'b1' : 'b0'], () => { const r = dates.serialToYmd(s, b); return `y${nt(r.y)};m${nt(r.m)};d${nt(r.d)}`; });
  call('weekday', [nt(s), b ? 'b1' : 'b0'], () => vt(dates.weekday(s, b)));
  call('serialToHms', [nt(s)], () => { const r = dates.serialToHms(s); return `h${nt(r.h)};m${nt(r.mi)};s${nt(r.s)}`; });
  call('splitSerial', [nt(s), b ? 'b1' : 'b0'], () => { const r = dates.splitSerial(s, b); return r === undefined ? 'u' : `${nt(r.day)};${nt(r.sec)}`; });
}

// ---------------------------------------------------------------- stats
const nlist = (xs) => `N[${xs.map(nt).join(',')}]`;
function rlist() {
  const n = rng.below(12);
  const kind = rng.below(4);
  return Array.from({ length: n }, () => (kind === 0 ? rnum() : kind === 1 ? rng.below(10) : kind === 2 ? 1e15 + rng.below(100) / 10 : (unit() - 0.5) * 10 ** rng.below(5)));
}
for (let i = 0; i < 800 * SCALE; i++) {
  const x = rnum(), y = rng.below(2) ? -x * (1 + (rng.below(40) - 20) * 2 ** -53) : rnum();
  call('xlAdd', [nt(x), nt(y)], () => vt(stats.xlAdd(x, y)));
  call('snapAdd', [nt(x), nt(y)], () => vt(mu.snapAdd(x, y)));
  call('statNum', [nt(x)], () => vt(stats.num(x)));
}
for (let i = 0; i < 500 * SCALE; i++) {
  const xs = rlist();
  const L = [nlist(xs)];
  call('xlSum', L, () => vt(stats.xlSum(xs)));
  call('mean', L, () => vt(stats.mean(xs)));
  call('devsq', L, () => vt(stats.devsq(xs)));
  call('varianceS', L, () => vt(stats.variance(xs, true)));
  call('varianceP', L, () => vt(stats.variance(xs, false)));
  call('skew', L, () => vt(stats.skew(xs)));
  call('skewP', L, () => vt(stats.skewP(xs)));
  call('kurt', L, () => vt(stats.kurt(xs)));
  call('sorted', L, () => nlist(stats.sorted(xs)));
}
// collectNumbers with a fake context (the reference walker: the existing cells of each area, row by row)
const COLLECT_OPTS = [{}, { refText: 'zero', refBool: 'num' }, { arrText: 'zero', arrBool: 'num' }, { directText: 'skip' }, { directText: 'ignore' },
  { directBool: 'skip' }, { errors: 'skip' }, { refText: 'zero', arrText: 'zero', directText: 'skip', errors: 'skip', directBool: 'skip' }];
const vlist = (xs) => `V[${xs.map(vt).join(',')}]`;
for (const [lk, loc] of [['en', fn.EN_US], ['pt', fn.PT_BR]]) {
  for (let i = 0; i < 150 * SCALE; i++) {
    const oi = rng.below(COLLECT_OPTS.length);
    const n = 1 + rng.below(4);
    const args = [], enc = [];
    const cells = new Map();
    for (let k = 0; k < n; k++) {
      const kind = rng.below(3);
      const items = Array.from({ length: rng.below(5) }, () => (rng.below(3) ? rscalar() : rformatted(loc)));
      if (kind === 0) { const v = items[0] ?? null; args.push(v); enc.push(`d${vt(v)}`); }
      else if (kind === 1) { args.push(new ArrayValue(1, items.length, items)); enc.push(`a${vlist(items)}`); }
      else { const area = { sheet: k, r1: 0, c1: 0, r2: 0, c2: items.length }; cells.set(k, items.filter((v) => v !== null)); args.push(new RefValue([area])); enc.push(`r${vlist(items.filter((v) => v !== null))}`); }
    }
    const ctx = { locale: loc, forEachCell: (area, f) => { (cells.get(area.sheet) ?? []).forEach((v, c) => f(v, 0, c)); } };
    call('collectNumbers', [`o${oi}`, `L${lk}`, ...enc], () => { const r = stats.collectNumbers(args, ctx, COLLECT_OPTS[oi]); return r instanceof CalcError ? vt(r) : nlist(r); });
  }
  for (let i = 0; i < 200 * SCALE; i++) {
    const v = rng.below(2) ? rscalar() : pick([rtext(), rformatted(loc), 'TRUE', ' verdadeiro ', 'FALSO', '0', '2']);
    const ctx = { locale: loc };
    call('argNumber', [vt(v), `L${lk}`], () => vt(stats.argNumber(v, ctx)));
    call('argInt', [vt(v), `L${lk}`], () => vt(stats.argInt(v, ctx)));
    call('argBool', [vt(v), `L${lk}`], () => vt(stats.argBool(v, ctx)));
    call('atpNum', [vt(v), `L${lk}`], () => vt(mu.atpNum(v, ctx)));
  }
}

// ---------------------------------------------------------------- special functions
const sp = () => pick([pos(), unit() * 5, unit() * 50, unit() * 500, unit() * 1e5, rng.below(40), rng.below(40) + 0.5, 1e-10 * unit(), 0, -unit() * 5, NaN, Infinity, 1e300, 0.5, 1, 2, 10, 171, 171.5, 172]);
const prob = () => pick([unit(), unit() * 1e-5, 1 - unit() * 1e-10, 0, 1, 0.5, 0.025, 0.975, -0.1, 1.1, NaN, 1e-300]);
const one = (name, f, gen) => { for (let i = 0; i < 300 * SCALE; i++) { const x = gen(); call(name, [nt(x)], () => vt(f(x))); } };
const two = (name, f, g1, g2) => { for (let i = 0; i < 300 * SCALE; i++) { const x = g1(), y = g2(); call(name, [nt(x), nt(y)], () => vt(f(x, y))); } };
const pair = (r, a, b) => `${nt(r[a])};${nt(r[b])}`;
const anyx = () => pick([rnum(), (unit() - 0.5) * 20, (unit() - 0.5) * 80, (unit() - 0.5) * 2]);
for (let i = 0; i < 300 * SCALE; i++) { const x = anyx(); call('pnormBoth', [nt(x)], () => pair(special.pnormBoth(x), 'cum', 'ccum')); }
one('dnorm', special.dnorm, anyx);
one('qnorm', special.qnorm, prob);
one('lgamma', special.lgamma, sp);
one('lgammacor', special.lgammacor, () => 10 + sp());
one('gamma', special.gamma, () => pick([sp(), -sp(), -2.5, -0.5, 0.25]));
one('erf', special.erf, anyx);
one('erfc', special.erfc, anyx);
one('normCdfXl', special.normCdfXl, anyx);
one('stirlerr', (x) => special.stirlerr(x) ?? NaN, () => pick([sp(), rng.below(31) / 2, 15.5, 16, 36, 81, 501]));
two('bd0', special.bd0, sp, sp);
one('log1pmx', special.log1pmx, () => pick([unit() * 2 - 1, (unit() - 0.5) * 0.02, 5 * unit(), -0.8, -0.79149064, 1]));
one('lgamma1p', special.lgamma1p, () => pick([unit() - 0.5, sp(), 0.49, -0.49]));
for (let i = 0; i < 300 * SCALE; i++) {
  const n = rng.below(3) ? rng.below(200) : sp(), x = rng.below(3) ? rng.below(Math.max(1, Math.floor(n) + 2)) : sp(), p = prob();
  call('dbinomRaw', [nt(x), nt(n), nt(p), nt(1 - p)], () => vt(special.dbinomRaw(x, n, p, 1 - p)));
}
two('dpoisRaw', special.dpoisRaw, () => pick([sp(), rng.below(50)]), sp);
for (let i = 0; i < 500 * SCALE; i++) {
  const x = sp(), al = sp(), lower = rng.below(2) === 0;
  call('pgamma', [nt(x), nt(al), lower ? 'b1' : 'b0'], () => vt(special.pgamma(x, al, lower)));
}
for (let i = 0; i < 200 * SCALE; i++) { const a0 = sp(), x = sp(); call('gammaInc', [nt(a0), nt(x)], () => pair(special.gammaInc(a0, x), 'p', 'q')); }
two('lbeta', special.lbeta, sp, sp);
for (let i = 0; i < 500 * SCALE; i++) {
  const x = prob(), a0 = sp(), b0 = sp();
  call('betaInc', [nt(x), nt(a0), nt(b0)], () => pair(special.betaInc(x, a0, b0), 'p', 'q'));
}

// ---------------------------------------------------------------- libm
const lx = () => pick([rnum(), pos(), unit() * 2, unit() * 1000 - 500, (unit() - 0.5) * 1e-3, 709.78, -745.13, 710, -746, 2.5, Math.PI / 2, 1e8, 1 + 2 ** -52, 1 - 2 ** -53, 1e-310]);
for (const name of ['crLog', 'crLog10', 'crLog1p', 'crExp', 'xlExp', 'xlSin', 'xlCos', 'xlTan', 'crAtan', 'crAsin', 'crAcos', 'crSinh', 'crCosh', 'crExpm1']) {
  const f = libm[name];
  const gen = name === 'crAsin' || name === 'crAcos' ? () => pick([unit() * 2 - 1, -1, 1, 0, lx()]) : lx;
  one(name, f, gen);
}
two('crPow', libm.crPow, () => pick([pos(), unit() * 3, 1, 0, -2]), () => pick([rnum(), (unit() - 0.5) * 20, 0.5, 2, -1.5]));
two('crAtan2', libm.crAtan2, lx, lx);

// ---------------------------------------------------------------- math-util
one('approx15', mu.approx15, rnum);
one('exponentOf', mu.exponentOf, rnum);
one('fin', mu.fin, rnum);
one('finFlush', mu.finFlush, rnum);
for (let i = 0; i < 900 * SCALE; i++) {
  const x = rng.below(3) ? (unit() - 0.5) * 10 ** rng.below(12) : rnum();
  const d = rng.below(6) ? rng.below(30) - 15 : pick([NaN, 2.5, -1.5, 1e9, -1e9, Infinity]);
  const m = rng.below(3);
  call('decimalRound', [nt(x), nt(d), nt(m)], () => vt(mu.decimalRound(x, d, m)));
}
for (let i = 0; i < 600 * SCALE; i++) {
  const x = pick([rnum(), 10, -8, 0, 1.1, 1.0000001, -2, unit() * 4, -unit() * 4, 2]);
  const y = pick([rnum(), rng.below(800) - 400, 0.5, 1 / 3, 0.333333333333333, 0.142857142857143, -0.5, (unit() - 0.5) * 30, 1e9, 4294967296]);
  call('excelPower', [nt(x), nt(y)], () => vt(mu.excelPower(x, y)));
}

w.meta('stats', `calls=${total} functions=${counts.size}`);
mkdirSync(dirname(out), { recursive: true });
if (out.endsWith('.gz')) writeFileSync(out, gzipSync(Buffer.from(w.text(), 'utf8'), { level: 9 }));
else w.write(out);
console.log(`a1${big ? ' (big)' : ''}: ${total} calls, ${counts.size} functions -> ${out}`);
