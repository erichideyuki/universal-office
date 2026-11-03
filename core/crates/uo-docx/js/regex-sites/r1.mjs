// The regex site of frontier R1 (ooxml/props.ts `measureTwips`), checked by tests/regex_r1.rs: the
// Rust `read::props::split_measure` (a `Cur` walk, matcher template 5) against V8. Group 1 is the
// number text, group 2 the unit; `props.rs` applies it to `v.trim()`.

/** [site id, the TS regex, operation]. */
export const SITES = [
  ['r1.measure_test', /^(-?[0-9.]+)(mm|cm|in|pt|pc|pi)?$/, 'test'],
  ['r1.measure_number', /^(-?[0-9.]+)(mm|cm|in|pt|pc|pi)?$/, 'group'],
  ['r1.measure_unit', /^-?[0-9.]+(mm|cm|in|pt|pc|pi)?$/, 'group'],
  // text-effects.ts `escX`: the four chained global replaces, run as one `replace` call
  ['r1.esc_x', [{ [Symbol.replace]: (s) => s.replace(/&/g, '&amp;').replace(/</g, '&lt;').replace(/>/g, '&gt;').replace(/"/g, '&quot;') }, ''], 'replace'],
];

/** Attribute values a document can hold, trimmed and not. */
export const INPUTS = [
  '', ' ', '0', '1', '-1', '12', '-12', '240', '1e3', '1E3', ' 12 ', '12 ', '\t7', '12pt', '12PT', '12Pt', '0x10', '-0', '+5', 'NaN', 'Infinity', '1.5', '1.5.2',
  '.', '-', '-.', '..5', '.5', '5.', '-.5pt', '12mm', '12cm', '12in', '12pc', '12pi', '1.5in', '-3.7mm', '99999999999999999999', '1e400', '９', '१२', '12px',
  '&', '<', '>', '"', "'", '&amp;', 'a&b<c>"d"', '&lt;&gt;', '<<&&>>""', '😀&', 'x	y',
  '12%', '50%', '5 0', '--5', '0012', '1_000', '2.5pt', '-0pt', '1e2pt', '12 pt', '12ptx', 'pt', '12p', '12m', '12i', '12ppt', '12mmm', '12pt\n', '12pt ',
  ' 12', '12 ', '12ｐｔ', '12µ', '4294967296', '-2147483649', '.pt', '-.pt', '1.pt', '-pt', 'é12', '12é', '😀', '12😀', '--', '..', '0.0.0cm',
];
