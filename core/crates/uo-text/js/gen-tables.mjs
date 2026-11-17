// Generates the Unicode tables of uo-text from the ICU inside Node 24.15 (V8 13.6): what V8
// computes is the specification, so every table is derived by probing V8 and never by reading a
// Unicode data file. Deterministic: the same Node 24.15 gives byte-identical output.
//   node crates/uo-text/js/gen-tables.mjs            writes src/tables.rs and src/tables_compat.rs
//   node crates/uo-text/js/gen-tables.mjs --check    exits 1 when the committed files differ
// Tables (each is a compaction of one probe over every code point 0..=10FFFF):
//   CCC           canonical combining class, as a rank (the NFC reordering only needs the order)
//   NFD_*         full canonical decomposition (String.prototype.normalize('NFD')), Hangul excluded
//   COMP_*        primary composites (a, b) -> c, found as NFC(a + b) === c
//   PROPS         \p{L} \p{M} \p{N} \p{P} (RegExp with the u flag)
//   SIGMA         Cased / Case_Ignorable as the Final_Sigma context of toLowerCase sees them
//   UPPER_*, LOWER_*   toUpperCase / toLowerCase of one code point (full mapping, root locale)
//   COMPAT_*      NFKD(c) where it differs from NFD(c) (only for the `collation` feature)
import { readFileSync, writeFileSync } from 'node:fs';

const check = process.argv.includes('--check');
const here = (p) => new URL(p, import.meta.url);

if (!/^v24\.15\./.test(process.version)) throw new Error(`the tables are pinned to Node 24.15, this is ${process.version}`);
const UNICODE = process.versions.unicode;
const ICU = process.versions.icu;

const MAX = 0x10ffff;
const isSurrogate = (cp) => cp >= 0xd800 && cp <= 0xdfff;
const isHangulSyllable = (cp) => cp >= 0xac00 && cp <= 0xd7a3;
const str = (cp) => String.fromCodePoint(cp);
const cpsOf = (s) => [...s].map((c) => c.codePointAt(0));
const hex = (n) => `0x${n.toString(16).toUpperCase()}`;

function units(s) {
  const out = [];
  for (let i = 0; i < s.length; i++) out.push(s.charCodeAt(i));
  return out;
}

// ---------------------------------------------------------------- combining classes
// reorders(a, b): NFD swaps a and b exactly when ccc(a) > ccc(b) > 0. With U+0345 (240) and U+0334 (1)
// as probes this finds every non-zero class (1..240), and the same relation sorts them.
const reorders = (a, b) => {
  const s = str(a) + str(b);
  return s.normalize('NFD') !== s;
};
const stable = (cp) => !isSurrogate(cp) && !isHangulSyllable(cp) && str(cp).normalize('NFD') === str(cp);
const nonZero = [];
for (let cp = 0; cp <= MAX; cp++) {
  if (!stable(cp)) continue;
  if (cp === 0x345 || reorders(0x345, cp) || reorders(cp, 0x334)) nonZero.push(cp);
}
const cmpCcc = (a, b) => (reorders(a, b) ? 1 : reorders(b, a) ? -1 : 0);
nonZero.sort((a, b) => cmpCcc(a, b) || a - b);
const rankOf = new Map();
let rank = 0;
for (let i = 0; i < nonZero.length; i++) {
  if (i === 0 || cmpCcc(nonZero[i - 1], nonZero[i]) !== 0) rank++;
  rankOf.set(nonZero[i], rank);
}
// the relation must be a total preorder (transitive classes): checked on the sorted list
for (let i = 1; i < nonZero.length; i++) {
  const a = nonZero[i - 1];
  const b = nonZero[i];
  const same = rankOf.get(a) === rankOf.get(b);
  if (same !== (cmpCcc(a, b) === 0) || (!same && !reorders(b, a))) throw new Error(`ccc order not total at ${hex(a)} ${hex(b)}`);
}
if (rank > 255) throw new Error('more than 255 combining classes');
const cccRuns = [];
for (const cp of [...nonZero].sort((a, b) => a - b)) {
  const r = rankOf.get(cp);
  const last = cccRuns[cccRuns.length - 1];
  if (last && last.end + 1 === cp && last.rank === r) last.end = cp;
  else cccRuns.push({ start: cp, end: cp, rank: r });
}

// ---------------------------------------------------------------- canonical decomposition and composition
const nfd = []; // [cp, units[]]
const compat = []; // [cp, units[]] where NFKD differs from NFD
const nfdOf = new Map();
for (let cp = 0; cp <= MAX; cp++) {
  if (isSurrogate(cp) || isHangulSyllable(cp)) continue;
  const c = str(cp);
  const d = c.normalize('NFD');
  if (d !== c) {
    nfd.push([cp, units(d)]);
    nfdOf.set(cp, d);
  }
  const k = c.normalize('NFKD');
  if (k !== d) compat.push([cp, units(k)]);
}

const comp = [];
let composites = 0;
for (const [cp] of nfd) {
  const d = cpsOf(nfdOf.get(cp));
  const c = str(cp);
  if (d.length < 2 || c.normalize('NFC') !== c) continue;
  composites++;
  const b = d[d.length - 1];
  const aStr = d.slice(0, -1).map(str).join('').normalize('NFC');
  const aCps = cpsOf(aStr);
  if (aCps.length !== 1 || (str(aCps[0]) + str(b)).normalize('NFC') !== c) throw new Error(`cannot split the primary composite ${hex(cp)}`);
  comp.push([aCps[0], b, cp]);
}
comp.sort((x, y) => x[0] - y[0] || x[1] - y[1]);
for (let i = 1; i < comp.length; i++) if (comp[i][0] === comp[i - 1][0] && comp[i][1] === comp[i - 1][1]) throw new Error('duplicate composition pair');
// (the exhaustive composition check of every first x second lives in the Rust tests, against a V8 capture)

// ---------------------------------------------------------------- properties
const PROP_RES = [/^\p{L}$/u, /^\p{M}$/u, /^\p{N}$/u, /^\p{P}$/u];
const propRuns = [];
for (let cp = 0; cp <= MAX; cp++) {
  const c = str(cp);
  let mask = 0;
  for (let i = 0; i < PROP_RES.length; i++) if (PROP_RES[i].test(c)) mask |= 1 << i;
  if (mask === 0) continue;
  const last = propRuns[propRuns.length - 1];
  if (last && last.end + 1 === cp && last.mask === mask) last.end = cp;
  else propRuns.push({ start: cp, end: cp, mask });
}

// \p{Ll} (the small-caps split of the Slides text layout, text-layout.ts:171): a subset of \p{L}, so its own runs
const LL_RE = /^\p{Ll}$/u;
const llRuns = [];
for (let cp = 0; cp <= MAX; cp++) {
  if (!LL_RE.test(str(cp))) continue;
  const last = llRuns[llRuns.length - 1];
  if (last && last.end + 1 === cp) last.end = cp;
  else llRuns.push({ start: cp, end: cp });
}

// Final_Sigma classes: probe 1 (`c` + capital sigma) gives final sigma only if c counts as a cased letter;
// probe 2 (`A c` + sigma) also when c is skipped as case-ignorable. 1 = cased, 2 = skipped.
const SIGMA_FINAL = '\u03c2';
const sigmaRuns = [];
for (let cp = 0; cp <= MAX; cp++) {
  const c = str(cp);
  const p1 = (c + '\u03a3').toLowerCase().endsWith(SIGMA_FINAL);
  const p2 = ('A' + c + '\u03a3').toLowerCase().endsWith(SIGMA_FINAL);
  const cls = p1 ? 1 : p2 ? 2 : 0;
  if (cls === 0) continue;
  const last = sigmaRuns[sigmaRuns.length - 1];
  if (last && last.end + 1 === cp && last.mask === cls) last.end = cp;
  else sigmaRuns.push({ start: cp, end: cp, mask: cls });
}

// ---------------------------------------------------------------- case mapping
function caseTables(map) {
  const singles = []; // [cp, delta]
  const specials = []; // [cp, units[]]
  for (let cp = 0; cp <= MAX; cp++) {
    if (isSurrogate(cp)) continue;
    const c = str(cp);
    const m = map(c);
    if (m === c) continue;
    const t = cpsOf(m);
    if (t.length === 1) singles.push([cp, t[0] - cp]);
    else specials.push([cp, units(m)]);
  }
  const runs = [];
  let cur = null;
  for (const [cp, delta] of singles) {
    if (cur && cur.delta === delta && (cur.count === 1 ? cp - cur.last === 1 || cp - cur.last === 2 : cp - cur.last === cur.step)) {
      if (cur.count === 1) cur.step = cp - cur.last;
      cur.last = cp;
      cur.count++;
    } else {
      if (cur) runs.push(cur);
      cur = { start: cp, last: cp, delta, step: 1, count: 1 };
    }
  }
  if (cur) runs.push(cur);
  return { runs, specials, singles: singles.length };
}
const upper = caseTables((c) => c.toUpperCase());
const lower = caseTables((c) => c.toLowerCase());

// ---------------------------------------------------------------- emission
function arrayText(name, type, items, fmt, perLine, doc) {
  const lines = [];
  for (let i = 0; i < items.length; i += perLine) lines.push(`    ${items.slice(i, i + perLine).map(fmt).join(', ')},`);
  return `${doc ? `/// ${doc}\n` : ''}pub static ${name}: [${type}; ${items.length}] = [\n${lines.join('\n')}\n];\n`;
}

function decompText(prefix, entries, what) {
  const bmp = entries.filter(([cp]) => cp <= 0xffff);
  const astral = entries.filter(([cp]) => cp > 0xffff);
  const all = [...bmp, ...astral];
  const pool = [];
  const ends = [];
  for (const [, u] of all) {
    pool.push(...u);
    ends.push(pool.length);
  }
  if (pool.length > 0xffff) throw new Error(`${prefix} pool does not fit u16 offsets`);
  return (
    arrayText(`${prefix}_KEYS_BMP`, 'u16', bmp.map((e) => e[0]), hex, 12, `${what}: the keys below U+10000, ascending.`) +
    arrayText(`${prefix}_KEYS_ASTRAL`, 'u32', astral.map((e) => e[0]), hex, 8, `${what}: the keys from U+10000, ascending.`) +
    arrayText(`${prefix}_ENDS`, 'u16', ends, String, 16, `${what}: end offset in the pool of the result of each key (BMP keys first, then astral).`) +
    arrayText(`${prefix}_POOL`, 'u16', pool, hex, 12, `${what}: the results as UTF-16 code units.`)
  );
}

function caseText(prefix, t, what) {
  const runs = t.runs.map((r) => [r.start, r.last | (r.step << 24), r.delta]);
  const pool = [];
  const specials = t.specials.map(([cp, u]) => {
    const off = pool.length;
    pool.push(...u);
    return [cp, off, u.length];
  });
  return (
    arrayText(`${prefix}_RUNS`, '(u32, u32, i32)', runs, (r) => `(${hex(r[0])}, ${hex(r[1])}, ${r[2]})`, 3, `${what}: runs of single-code-point mappings, (first, last | step << 24, delta): cp -> cp + delta for first, first + step, ... last.`) +
    arrayText(`${prefix}_SPECIAL`, '(u32, u16, u16)', specials, (s) => `(${hex(s[0])}, ${s[1]}, ${s[2]})`, 5, `${what}: the mappings that are not one code point, (cp, offset, length) into the pool.`) +
    arrayText(`${prefix}_SPECIAL_POOL`, 'u16', pool, hex, 12, `${what}: the results of the special mappings as UTF-16 code units.`)
  );
}

const header = (what) =>
  `// @generated by js/gen-tables.mjs from the ICU of Node 24.15 (V8 ${process.versions.v8.split('-')[0]}, ICU ${ICU}, Unicode ${UNICODE}). Do not edit: rerun the generator.\n// ${what}\n\n`;

const main =
  header('NFC/NFD, \\p{L} \\p{M} \\p{N} \\p{P}, Final_Sigma classes and the toUpperCase/toLowerCase mappings.') +
  `/// The Unicode version of the tables (the one of the ICU in Node 24.15).\npub const UNICODE_VERSION: &str = "${UNICODE}";\n/// The ICU version they were generated from.\npub const ICU_VERSION: &str = "${ICU}";\n\n` +
  arrayText('CCC', '(u32, u32)', cccRuns, (r) => `(${hex(r.start | (r.rank << 24))}, ${hex(r.end)})`, 4, "Non-zero canonical combining classes as ranks (the order is Unicode's, the values are not): (first | rank << 24, last).") +
  '\n' +
  decompText('NFD', nfd, 'Canonical decomposition (NFD, Hangul syllables excluded)') +
  '\n' +
  arrayText('COMP_BMP', '(u16, u16, u16)', comp.filter((p) => p[0] <= 0xffff && p[1] <= 0xffff && p[2] <= 0xffff), (p) => `(${hex(p[0])}, ${hex(p[1])}, ${hex(p[2])})`, 4, 'Primary composites (first, second, composite) when all three are below U+10000, ascending by (first, second).') +
  arrayText('COMP_ASTRAL', '(u32, u32, u32)', comp.filter((p) => !(p[0] <= 0xffff && p[1] <= 0xffff && p[2] <= 0xffff)), (p) => `(${hex(p[0])}, ${hex(p[1])}, ${hex(p[2])})`, 3, 'Primary composites with a code point from U+10000 up.') +
  '\n' +
  arrayText('PROPS', '(u32, u32)', propRuns, (r) => `(${hex(r.start)}, ${hex(r.end | (r.mask << 24))})`, 4, 'General categories L (1), M (2), N (4), P (8) as runs: (first, last | mask << 24).') +
  arrayText('SIGMA', '(u32, u32)', sigmaRuns, (r) => `(${hex(r.start)}, ${hex(r.end | (r.mask << 24))})`, 4, 'Final_Sigma classes as runs: (first, last | class << 24), class 1 = cased letter, 2 = skipped (case-ignorable).') +
  '\n' +
  caseText('UPPER', upper, 'toUpperCase') +
  '\n' +
  caseText('LOWER', lower, 'toLowerCase') +
  '\n' +
  arrayText('LOWER_LETTER', '(u32, u32)', llRuns, (r) => `(${hex(r.start)}, ${hex(r.end)})`, 4, 'General category Ll (\\p{Ll}) as runs: (first, last).');

const compatText =
  header('NFKD where it differs from NFD (feature `collation`).') + decompText('COMPAT', compat, 'Compatibility decomposition (NFKD) where it differs from NFD, Hangul syllables excluded');

const outputs = [
  ['../src/tables.rs', main],
  ['../src/tables_compat.rs', compatText],
];
let bad = false;
for (const [path, text] of outputs) {
  if (check) {
    let have = '';
    try {
      have = readFileSync(here(path), 'utf8');
    } catch {
      have = '';
    }
    if (have !== text) {
      console.error(`${path} differs from what the generator produces`);
      bad = true;
    }
  } else writeFileSync(here(path), text);
}
const stat = {
  ccc: `${cccRuns.length} runs, ${rank} ranks`,
  nfd: nfd.length,
  compat: compat.length,
  composites,
  comp: comp.length,
  props: propRuns.length,
  sigma: sigmaRuns.length,
  upper: `${upper.singles} singles in ${upper.runs.length} runs, ${upper.specials.length} special`,
  lower: `${lower.singles} singles in ${lower.runs.length} runs, ${lower.specials.length} special`,
};
console.log(`${check ? 'checked' : 'wrote'} tables: Unicode ${UNICODE}, ICU ${ICU}, node ${process.version}`, JSON.stringify(stat));
process.exit(bad ? 1 : 0);
