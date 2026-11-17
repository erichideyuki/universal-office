// Generates the grapheme-cluster tables of uo-text (`src/grapheme_tables.rs`) and their V8 oracle
// (`tests/captures/grapheme_v8.txt`) from the ICU inside Node 24.15: what `Intl.Segmenter` with
// `granularity: 'grapheme'` computes is the specification (the Slides text layout, text-layout.ts:217), so
// every property is derived by PROBING the segmenter, never by reading a Unicode data file. Deterministic: the
// same Node 24.15 gives byte-identical output.
//   node crates/uo-text/js/gen-grapheme.mjs            writes both files
//   node crates/uo-text/js/gen-grapheme.mjs --check    exits 1 when a committed file differs
//
// Properties (UAX #29 names; each one found by where the segmenter breaks around the code point c):
//   GCB    Grapheme_Cluster_Break: CR LF Control Extend ZWJ Regional_Indicator Prepend SpacingMark L V T LV LVT
//   EP     Extended_Pictographic (GB11: c ZWJ U+1F600 stays one cluster)
//   InCB   Indic_Conjunct_Break Consonant / Extend / Linker (GB9c: KA VIRAMA c, KA c KA, KA c VIRAMA KA)
// The generator then runs its own implementation of the rules over those tables (the algorithm `src/grapheme.rs`
// ports) against the segmenter on every pair and triple of representatives and on seeded random strings, and
// stops if a single boundary differs; the same strings and boundaries are the oracle the Rust tests replay.
import { readFileSync, writeFileSync } from 'node:fs';

const check = process.argv.includes('--check');
const here = (p) => new URL(p, import.meta.url);
if (!/^v24\.15\./.test(process.version)) throw new Error(`the tables are pinned to Node 24.15, this is ${process.version}`);
const UNICODE = process.versions.unicode;
const ICU = process.versions.icu;

const MAX = 0x10ffff;
const seg = new Intl.Segmenter(undefined, { granularity: 'grapheme' });
const hex = (n) => `0x${n.toString(16).toUpperCase()}`;
/** The JS string of a code point; a surrogate code point is the lone code unit. */
const s1 = (cp) => (cp >= 0xd800 && cp <= 0xdfff ? String.fromCharCode(cp) : String.fromCodePoint(cp));

/** Interior boundaries (UTF-16 offsets, 0 and length excluded) the segmenter puts in `s`. */
function bounds(s) {
  const out = [];
  for (const x of seg.segment(s)) if (x.index > 0) out.push(x.index);
  return out;
}
/** Does the segmenter break `s` at UTF-16 offset `pos`? */
const brk = (s, pos) => bounds(s).includes(pos);

// ---------------------------------------------------------------- probing
const C = { Any: 0, CR: 1, LF: 2, Control: 3, Extend: 4, ZWJ: 5, RI: 6, Prepend: 7, SpacingMark: 8, L: 9, V: 10, T: 11, LV: 12, LVT: 13 };
const NAMES = Object.keys(C);
const JL = 'ᄀ', JV = 'ᅡ', JT = 'ᆨ', JLVT = '각';
const ACUTE = '́', ZWJ = '‍', GRIN = '\u{1F600}';
const KA = 'क', VIR = '्';

function classify(cp) {
  const s = s1(cp);
  const n = s.length;
  const before = brk('a' + s, 1); // Any c
  const after = brk(s + 'a', n); // c Any
  if (cp === 0x0d) return C.CR;
  if (cp === 0x0a) return C.LF;
  if (cp === 0x200d) return !before && after ? C.ZWJ : fail(cp, 'ZWJ');
  if (!before && after) {
    // Extend or SpacingMark: only Extend continues an emoji ZWJ sequence (GB11 `ExtPict Extend* ZWJ x ExtPict`)
    return brk(GRIN + s + ZWJ + GRIN, 2 + n + 1) ? C.SpacingMark : C.Extend;
  }
  if (before && !after) return C.Prepend;
  if (!before && !after) return fail(cp, 'joins on both sides');
  // breaks on both sides of an Any: Control, a Hangul jamo/syllable, a regional indicator or Any
  if (brk(s + ACUTE, n)) return C.Control; // GB4 before GB9
  const fromL = !brk(JL + s, 1);
  const toV = !brk(s + JV, n);
  const toT = !brk(s + JT, n);
  const fromV = !brk(JV + s, 1);
  if (fromL && toV && !toT) return C.L;
  if (fromL && toV && toT && fromV) return C.V;
  if (fromL && toV && toT && !fromV) return C.LV;
  if (fromL && !toV && toT) return C.LVT;
  if (!fromL && toT && fromV && !brk(JLVT + s, 1)) return C.T;
  if (fromL || toV || toT || fromV) return fail(cp, 'partial Hangul behaviour');
  // a pair joins and a third one starts a new cluster (GB12/GB13)
  if (!brk(s + s, n)) return brk(s + s + s, 2 * n) ? C.RI : fail(cp, 'pairs without the RI rule');
  return C.Any;
}
function fail(cp, why) {
  throw new Error(`unexpected segmenter behaviour at ${hex(cp)}: ${why}`);
}

const gcb = new Uint8Array(MAX + 1);
const ep = new Uint8Array(MAX + 1);
/** 1 Consonant, 2 Extend, 3 Linker */
const incb = new Uint8Array(MAX + 1);
for (let cp = 0; cp <= MAX; cp++) {
  const g = classify(cp);
  gcb[cp] = g;
  const s = s1(cp);
  const n = s.length;
  if (!brk(s + ZWJ + GRIN, n + 1)) ep[cp] = 1;
  if (!brk(s + VIR + KA, n + 1)) incb[cp] = 1;
  else if (g === C.Extend || g === C.ZWJ) {
    if (!brk(KA + s + KA, 1 + n)) incb[cp] = 3;
    else if (!brk(KA + s + VIR + KA, 1 + n + 1)) incb[cp] = 2;
  }
}
// the regex property must agree with the probe (both come from the same ICU)
const EP_RE = /^\p{Extended_Pictographic}$/u;
for (let cp = 0; cp <= MAX; cp++) {
  if (cp >= 0xd800 && cp <= 0xdfff) continue;
  if (EP_RE.test(String.fromCodePoint(cp)) !== (ep[cp] === 1)) fail(cp, 'Extended_Pictographic probe != \\p{Extended_Pictographic}');
}

function runsOf(arr) {
  const runs = [];
  for (let cp = 0; cp <= MAX; cp++) {
    const v = arr[cp];
    if (v === 0) continue;
    const last = runs[runs.length - 1];
    if (last && last.end + 1 === cp && last.v === v) last.end = cp;
    else runs.push({ start: cp, end: cp, v });
  }
  return runs;
}
const gcbRuns = runsOf(gcb);
const epRuns = runsOf(ep);
const incbRuns = runsOf(incb);

// ---------------------------------------------------------------- the rules (ported 1:1 to src/grapheme.rs)
/** Code points of a JS string with their UTF-16 offsets; a lone surrogate is its own code point. */
function cpsWithOffsets(s) {
  const out = [];
  for (let i = 0; i < s.length; ) {
    const cp = s.codePointAt(i);
    out.push([cp, i]);
    i += cp > 0xffff ? 2 : 1;
  }
  return out;
}
const isCtl = (g) => g === C.CR || g === C.LF || g === C.Control;
function modelBounds(s) {
  const cps = cpsWithOffsets(s);
  const out = [];
  let prev = -1;
  let ri = 0; // regional indicators in a row ending at prev
  let epSeq = false; // prev ends `ExtPict Extend*`
  let epZwj = false; // prev is the ZWJ of `ExtPict Extend* ZWJ`
  let conj = 0; // 0 none, 1 `Consonant [Extend Linker]*`, 2 the same with a Linker seen
  for (const [cp, off] of cps) {
    const g = gcb[cp];
    const e = ep[cp] === 1;
    const ic = incb[cp];
    if (prev >= 0) {
      const p = gcb[prev];
      let join;
      if (p === C.CR && g === C.LF) join = true;
      else if (isCtl(p) || isCtl(g)) join = false;
      else if (p === C.L && (g === C.L || g === C.V || g === C.LV || g === C.LVT)) join = true;
      else if ((p === C.LV || p === C.V) && (g === C.V || g === C.T)) join = true;
      else if ((p === C.LVT || p === C.T) && g === C.T) join = true;
      else if (g === C.Extend || g === C.ZWJ || g === C.SpacingMark) join = true;
      else if (p === C.Prepend) join = true;
      else if (ic === 1 && conj === 2) join = true;
      else if (p === C.ZWJ && e && epZwj) join = true;
      else if (p === C.RI && g === C.RI) join = ri % 2 === 1;
      else join = false;
      if (!join) out.push(off);
    }
    // state after cp
    epZwj = g === C.ZWJ && epSeq;
    epSeq = e || (epSeq && g === C.Extend);
    ri = g === C.RI ? ri + 1 : 0;
    if (ic === 1) conj = 1;
    else if (conj > 0 && ic === 3) conj = 2;
    else if (conj > 0 && ic === 2) conj = conj;
    else conj = 0;
    prev = cp;
  }
  return out;
}

// ---------------------------------------------------------------- self-check and oracle
// representatives: the first and the last code point of each class, of each InCB value, of EP, plus fixed probes
const reps = new Set([0x61, 0x628, 0xd800, 0xdc00, 0x1f600, 0xa9, 0x1f3fb, 0x1f1e6, 0x1f1e8, 0x915, 0x94d, 0x93c, 0x200d, 0x0d, 0x0a, 0x20, 0xe33, 0xe31]);
for (const runs of [gcbRuns, epRuns, incbRuns]) {
  const seen = new Map();
  for (const r of runs) {
    if (!seen.has(r.v)) seen.set(r.v, [r.start, r.end]);
    else seen.get(r.v)[1] = r.end;
  }
  for (const [a, b] of seen.values()) { reps.add(a); reps.add(b); }
}
const repList = [...reps].sort((a, b) => a - b);

// xorshift32 (the uo-testkit Rng) for the random strings
let rs = 0x9e3779b9;
const next = () => { rs ^= rs << 13; rs >>>= 0; rs ^= rs >>> 17; rs ^= rs << 5; rs >>>= 0; return rs; };
const below = (n) => next() % n;

const cases = [];
for (const a of repList) for (const b of repList) cases.push(s1(a) + s1(b));
for (const a of repList) for (const b of repList) for (const c of [0x1f600, 0x200d, 0x94d, 0x915, 0x301, 0x1f1e6, 0x1161, 0x11a8, 0x0a]) cases.push(s1(a) + s1(b) + s1(c));
const pool = [...repList, 0x301, 0x1f466, 0x1f3fd, 0x2764, 0xfe0f, 0x0e40, 0x0904, 0x0930, 0x93f, 0x0d4e, 0x110bd, 0x0600];
for (let i = 0; i < 6000; i++) {
  const n = 1 + below(12);
  let s = '';
  for (let k = 0; k < n; k++) s += s1(below(4) === 0 ? below(0x30000) : pool[below(pool.length)]);
  cases.push(s);
}
let bad = 0;
const lines = [];
for (const s of cases) {
  const want = bounds(s);
  const got = modelBounds(s);
  if (want.join(',') !== got.join(',')) {
    if (bad++ < 20) console.error(`rule model differs: ${[...s].map((c) => hex(c.codePointAt(0))).join(' ')} segmenter [${want}] model [${got}]`);
  }
  const units = Array.from({ length: s.length }, (_, i) => s.charCodeAt(i).toString(16)).join(',');
  lines.push(`${units}\t${want.join(',') || '-'}`);
}
if (bad) throw new Error(`${bad} oracle strings disagree with the rule model: fix the probes or the rules`);

// \p{Ll} of every code point (props::is_lowercase_letter), as runs
const llRuns = [];
const LL = /^\p{Ll}$/u;
for (let cp = 0; cp <= MAX; cp++) {
  if (cp >= 0xd800 && cp <= 0xdfff) continue;
  if (!LL.test(String.fromCodePoint(cp))) continue;
  const last = llRuns[llRuns.length - 1];
  if (last && last[1] + 1 === cp) last[1] = cp;
  else llRuns.push([cp, cp]);
}

// ---------------------------------------------------------------- output
function arrayText(name, type, items, fmt, perLine, doc) {
  const body = [];
  for (let i = 0; i < items.length; i += perLine) body.push('    ' + items.slice(i, i + perLine).map(fmt).join(', ') + ',');
  return `/// ${doc}\npub static ${name}: [${type}; ${items.length}] = [\n${body.join('\n')}\n];\n`;
}
const tables =
  `// @generated by js/gen-grapheme.mjs from the ICU of Node 24.15 (V8 ${process.versions.v8.split('-')[0]}, ICU ${ICU}, Unicode ${UNICODE}). Do not edit: rerun the generator.\n` +
  '// Grapheme_Cluster_Break, Extended_Pictographic and Indic_Conjunct_Break, probed through Intl.Segmenter.\n\n' +
  `/// The Unicode version of the segmentation tables.\npub const UNICODE_VERSION: &str = "${UNICODE}";\n/// The ICU version they were probed from.\npub const ICU_VERSION: &str = "${ICU}";\n\n` +
  arrayText('GCB', '(u32, u32)', gcbRuns, (r) => `(${hex(r.start)}, ${hex(r.end | (r.v << 24))})`, 4,
    `Grapheme_Cluster_Break as runs (first, last | class << 24); absent = Any. Classes: ${NAMES.map((k, i) => `${i} ${k}`).slice(1).join(', ')}.`) +
  '\n' +
  arrayText('EXT_PICT', '(u32, u32)', epRuns, (r) => `(${hex(r.start)}, ${hex(r.end)})`, 4, 'Extended_Pictographic as runs (first, last).') +
  '\n' +
  arrayText('INCB', '(u32, u32)', incbRuns, (r) => `(${hex(r.start)}, ${hex(r.end | (r.v << 24))})`, 4,
    'Indic_Conjunct_Break as runs (first, last | value << 24): 1 Consonant, 2 Extend, 3 Linker; absent = None.');
const oracle =
  `# grapheme oracle: Intl.Segmenter('grapheme') of Node ${process.version} (ICU ${ICU}, Unicode ${UNICODE}); written by js/gen-grapheme.mjs\n` +
  `# ll <first>-<last> (hex): the \\p{Ll} runs\n# <utf-16 code units, hex>\\t<interior boundaries as UTF-16 offsets, '-' for none>\n` +
  llRuns.map(([a, b]) => `ll ${a.toString(16)}-${b.toString(16)}`).join('\n') + '\n' +
  lines.join('\n') + '\n';

let differs = false;
for (const [path, text] of [['../src/grapheme_tables.rs', tables], ['../tests/captures/grapheme_v8.txt', oracle]]) {
  if (check) {
    let have = '';
    try { have = readFileSync(here(path), 'utf8'); } catch { have = ''; }
    if (have !== text) { console.error(`${path} differs from what the generator produces`); differs = true; }
  } else writeFileSync(here(path), text);
}
console.log(`${check ? 'checked' : 'wrote'} grapheme tables: Unicode ${UNICODE}, ICU ${ICU}; GCB ${gcbRuns.length} runs, EP ${epRuns.length}, InCB ${incbRuns.length}; ${cases.length} oracle strings, ${llRuns.length} Ll runs`);
process.exit(differs ? 1 : 0);
