// The differential check of gpos.rs and gsub.rs against the frozen TS (gpos.ts, gsub.ts and the
// kerning and ligature parts of sfnt.ts): the generated fonts are read by the TS `SfntFont`, and
// the kerning data, the units of a grid of glyph pairs, the GSUB data and `ligate` over seeded
// sequences are written as digests of canonical streams. The Rust side computes the same streams
// (tests/gpos_support/facets.rs); gpos_gsub_parity.rs replays this file.
//
//   cargo run -p uo-fonts --example gpos_gsub_fonts -- <fonts dir>
//   node --max-old-space-size=8192 capture_gpos_gsub.mjs --fonts <fonts dir> --ts-root <built checkout of main>
//        [--out ../parity/gpos_gsub.expected] [--skip-big]
//
// The checkout comes from --ts-root or UO_TS_ROOT (dist/ built: write/packages/docx-layout/dist),
// never from a path written here. Deterministic: no clock, no randomness beyond the seeded PRNG.
import { execFileSync } from 'node:child_process';
import { createHash } from 'node:crypto';
import { readFileSync, readdirSync, writeFileSync } from 'node:fs';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { Rng, Stream, importFrom, tsRoot } from '../../../uo-testkit/js/testkit.mjs';

const here = dirname(fileURLToPath(import.meta.url));
const argv = process.argv.slice(2);
const arg = (name) => { const i = argv.indexOf(name); return i >= 0 ? argv[i + 1] : undefined; };
const fontsDir = arg('--fonts');
if (!fontsDir) throw new Error('--fonts <dir> is required (write it with the gpos_gsub_fonts example)');
const out = resolve(arg('--out') ?? join(here, '..', 'parity', 'gpos_gsub.expected'));
const root = tsRoot(argv);
const { SfntFont } = await importFrom(root, 'write/packages/docx-layout/dist/typography/font/sfnt.js');

const open = (u8) => new SfntFont(new Uint8Array(u8));

// ---- facets (the same items, in the same order, as tests/gpos_support/facets.rs) ----

function kernFacet(s, f) {
  s.bool(f.kernInLatin).u64(f.kernMap.size);
  const keys = [...f.kernMap.keys()].sort((a, b) => a - b);
  for (const k of keys) {
    if (k >= 2 ** 32) s.tag('O').u64(k);
    else s.u64(k).i64(f.kernMap.get(k));
  }
  const g1s = [...f.kernClass0.keys()].sort((a, b) => a - b);
  s.u64(g1s.length);
  for (const g1 of g1s) {
    const e = f.kernClass0.get(g1);
    s.u64(g1).i64(e.adv);
    s.u64(e.cd2.size);
    for (const [g2, c2] of e.cd2) s.u64(g2).u64(c2);
  }
}

function gridGlyphs(f) {
  const set = new Set([0, 1, 2, 3, 0xffff, 0x10000, 0x20000]);
  let overflow = false;
  for (const k of f.kernMap.keys()) {
    if (k >= 2 ** 32) overflow = true;
    else { set.add(k >>> 16); set.add(k & 0xffff); }
  }
  if (overflow) set.add(0xffff);
  for (const [g1, e] of f.kernClass0) {
    set.add(g1);
    for (const g2 of e.cd2.keys()) if (g2 <= 0xffff) set.add(g2);
  }
  return [...set].sort((a, b) => a - b);
}

function unitsFacet(s, f) {
  const glyphs = gridGlyphs(f);
  if (glyphs.length > 400) throw new Error(`grid too large: ${glyphs.length}`);
  s.u64(glyphs.length);
  for (const g1 of glyphs) for (const g2 of glyphs) s.i64(f.kernGlyphsTwips(g1, g2, 30));
}

const gsubOf = (f) => { f.ligate([], []); return f.gsub; };

function gsubFacet(s, f) {
  const g = gsubOf(f);
  if (g === null) { s.tag('N'); return; }
  s.tag('G').u64(g.features.size);
  for (const [tag, list] of g.features) {
    s.str(tag).u64(list.length);
    for (const li of list) s.u64(li);
  }
  const lookups = [...g.lookups.keys()].sort((a, b) => a - b);
  s.u64(lookups.length);
  for (const li of lookups) {
    const map = g.lookups.get(li);
    const firsts = [...map.keys()].sort((a, b) => a - b);
    s.u64(li).u64(firsts.length);
    for (const first of firsts) {
      const list = map.get(first);
      s.u64(first).u64(list.length);
      for (const lig of list) {
        s.u64(lig.lig).u64(lig.rest.length);
        for (const r of lig.rest) s.u64(r);
      }
    }
  }
  const singles = g.singles ?? new Map();
  const sl = [...singles.keys()].sort((a, b) => a - b);
  s.u64(sl.length);
  for (const li of sl) {
    const map = singles.get(li);
    const from = [...map.keys()].sort((a, b) => a - b);
    s.u64(li).u64(from.length);
    for (const x of from) s.u64(x).u64(map.get(x));
  }
}

function alphabet(g) {
  const set = new Set([0]);
  for (const map of g.lookups.values()) {
    for (const [first, list] of map) {
      set.add(first);
      for (const lig of list) { set.add(lig.lig); for (const r of lig.rest) set.add(r); }
    }
  }
  for (const map of (g.singles ?? new Map()).values()) for (const [from, to] of map) { set.add(from); set.add(to); }
  return [...set].sort((a, b) => a - b);
}

function sequences(g, deep) {
  const out = [[]];
  for (const li of [...g.lookups.keys()].sort((a, b) => a - b)) {
    const map = g.lookups.get(li);
    for (const first of [...map.keys()].sort((a, b) => a - b)) {
      for (const lig of map.get(first)) out.push([first, ...lig.rest]);
    }
  }
  if (deep) {
    const alpha = alphabet(g);
    const rng = new Rng(0x12345678);
    for (let i = 0; i < 150; i++) {
      const len = 1 + rng.below(8);
      const seq = [];
      for (let k = 0; k < len; k++) seq.push(alpha[rng.below(alpha.length)]);
      out.push(seq);
    }
  }
  return out;
}

function featureSets(g, deep) {
  const sets = [['liga'], ['liga', 'clig'], ['liga', 'clig', 'dlig', 'hlig']];
  if (deep) {
    for (const tag of g.features.keys()) sets.push([tag]);
    sets.push([], ['zzzz']);
  }
  return sets;
}

function ligateFacet(s, f, deep) {
  const g = gsubOf(f);
  if (g === null) { s.tag('N'); return; }
  const sets = featureSets(g, deep);
  for (const seq of sequences(g, deep)) {
    for (const set of sets) {
      const r = f.ligate(seq, set);
      if (r === null) s.tag('n');
      else {
        s.tag('g').u64(r.length);
        for (const group of r) s.u64(group.gid).u64(group.len);
      }
    }
  }
}

const line = (name, facet, stream) => {
  const items = stream.items;
  return `${name}\t${facet}\t${stream.finish().short}\t${items}`;
};

// ---- the sweeps (the same variants as gpos_support::{truncations, mutants}) ----

const tailStart = (f, bytes) => Math.min(bytes.length, ...['kern', 'GSUB', 'GPOS'].filter((t) => f.tables.has(t)).map((t) => f.tables.get(t).offset));

function truncations(bytes, start) {
  const v = [];
  for (let n = start; n <= bytes.length; n++) v.push(bytes.subarray(0, n));
  return v;
}

function mutants(bytes, start, baseIndex) {
  const rng = new Rng(0x00c0ffee + baseIndex);
  const v = [];
  for (let i = 0; i < 400; i++) {
    const m = Uint8Array.from(bytes);
    const count = 1 + rng.below(3);
    for (let c = 0; c < count; c++) {
      const pos = start + rng.below(bytes.length - start);
      const mode = rng.below(4);
      let value;
      if (mode === 0) value = rng.next() & 0xff;
      else if (mode === 1) value = m[pos] ^ (1 << rng.below(8));
      else if (mode === 2) value = 0xff;
      else value = 0x00;
      m[pos] = value;
    }
    v.push(m);
  }
  return v;
}

function sweepStream(variants) {
  const s = new Stream();
  variants.forEach((bytes, i) => {
    const f = open(bytes);
    s.u64(i);
    kernFacet(s, f);
    gsubFacet(s, f);
    ligateFacet(s, f, false);
  });
  return s;
}

// ---- the run ----

const names = readdirSync(fontsDir).filter((n) => n.endsWith('.ttf')).map((n) => n.slice(0, -4)).sort();
const sweepBases = ['sweep_all', 'sweep_fallback'];
const lines = [];
let commit = 'unknown';
try { commit = execFileSync('git', ['-C', root, 'rev-parse', 'HEAD'], { encoding: 'utf8' }).trim(); } catch { /* not a git checkout */ }
const skipBig = argv.includes('--skip-big');
for (const name of names) {
  const bytes = readFileSync(join(fontsDir, `${name}.ttf`));
  if (name.startsWith('big_')) continue;
  lines.push(`${name}\tfont\t${createHash('sha256').update(bytes).digest('hex').slice(0, 32)}\t${bytes.length}`);
  const f = open(bytes);
  for (const [facet, fn] of [['kern', kernFacet], ['units', unitsFacet], ['gsub', gsubFacet], ['ligate', (s, font) => ligateFacet(s, font, true)]]) {
    const s = new Stream();
    fn(s, f);
    lines.push(line(name, facet, s));
  }
  if (sweepBases.includes(name)) {
    const start = tailStart(f, bytes);
    lines.push(line(name, 'truncations', sweepStream(truncations(bytes, start))));
    lines.push(line(name, 'mutants', sweepStream(mutants(bytes, start, sweepBases.indexOf(name)))));
  }
}

if (!skipBig) {
  for (const name of ['big_exact', 'big_over']) {
    const bytes = readFileSync(join(fontsDir, `${name}.ttf`));
    const f = open(bytes);
    const s = new Stream();
    s.bool(f.kernInLatin).u64(f.kernMap.size).u64(f.kernClass0.size);
    for (const [g1, g2] of [[0, 0], [4095, 4095], [100, 200], [4096, 4096], [4095, 0], [0, 4095], [4096, 0]]) s.i64(f.kernGlyphsTwips(g1, g2, 30));
    lines.push(line(name, 'big', s));
  }
}

const header = [
  '# uo-fonts gpos.rs / gsub.rs differential check (rust-port stage 3, frontier C): digests of canonical streams from the frozen TS SfntFont.',
  '# Written by core/crates/uo-fonts/tests/gpos_support/capture_gpos_gsub.mjs over the fonts of the gpos_gsub_fonts example; replayed by tests/gpos_gsub_parity.rs.',
  `@ts-commit ${commit}`,
  `@node ${process.version}`,
  `@fonts ${names.filter((n) => !n.startsWith('big_')).length}`,
];
writeFileSync(out, `${[...header, ...lines].join('\n')}\n`);
console.log(`${lines.length} lines written to ${out}`);
