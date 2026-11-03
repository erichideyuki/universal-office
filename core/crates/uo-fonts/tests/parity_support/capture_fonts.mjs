// Stage-4 parity capture of uo-fonts: the frozen TS (main 0.9.x) over the font corpus, written as
// digests of canonical streams (uo-testkit) to tests/parity/fonts.cap, replayed by
// tests/fonts_parity.rs. Digests only: no font byte is ever written.
//
//   1. cargo run --release -p uo-fonts --example fonts_corpus -- <corpus dir>   (generated fonts, mutants, collections)
//   2. UO_TS_ROOT=<built checkout of main> node tests/parity_support/capture_fonts.mjs --corpus <corpus dir>
//        [--out <cap>] [--only <key substring>] [--trace <key> <facet>]
//   Opt-in local capture over fonts that must never be committed (npm run fonts:sync copies,
//   C:\Windows\Fonts): --dir <folder of .ttf/.otf/.ttc> --out <git-ignored cap> [--depth light|full]
//
// The facets of one font are listed in tests/fonts_parity_support/facets.rs, which writes the same
// items in the same order; where the TS throws (a lazy read past the end, G4) the value written is
// the one the Rust must give (0, null) and what was normalized is counted in a `<facet>.g4` facet
// that the replay tallies as an accepted difference by name. The TS has no exit from the NaN-offset
// component loop of the subsetter (S2): the work runs in a worker watched by the main thread, and a
// unit silent for too long is recorded as `hang` (the Rust must give the input back there).
import { execFileSync } from 'node:child_process';
import { createHash } from 'node:crypto';
import { readFileSync, readdirSync, statSync } from 'node:fs';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import vm from 'node:vm';
import { Worker, isMainThread, parentPort, workerData } from 'node:worker_threads';
import { smallFacets } from './capture_fonts_small.mjs';
import { CaptureWriter, Rng, Stream, importFrom, tsRoot, walk } from '../../../uo-testkit/js/testkit.mjs';

const here = dirname(fileURLToPath(import.meta.url));
const repoRoot = resolve(here, '../../../../..');

// ---- the facet definitions shared with tests/fonts_parity_support/facets.rs ----

const DEPTHS = {
  full: { subsetRandom: 200, kernRandom: 10000, kernClass0: 1000, gidsSets: 100, ligateSeeded: 150 },
  light: { subsetRandom: 6, kernRandom: 300, kernClass0: 100, gidsSets: 8, ligateSeeded: 20 },
};
const SIZES = [6, 10.5, 11, 12, 72];
const TEXT = 'The quick brown fox jumps over the lazy dog. \u00c1\u00c9\u00cd\u00d3\u00da \u00e0\u00e8\u00ec\u00f2\u00f9 \u00e2\u00ea\u00ee\u00f4\u00fb \u00e3\u00f5 \u00e7 \u00f1 \u00fc \u00df \u0153 \ufb01 \ufb02 \u03a3 \u03a9 \u20ac \u2014 \u2018\u2019\u201c\u201d 0123456789 \u{1f600}';
const WORDS = ['office', 'official', 'affix', 'fluffy', 'ffi', 'ffl', 'fi fl', 'Tofu', 'AVATAR To Ty', 'a1b2 fi\u00e9 ct st', 'efficient', 'shuffle', 'Wolf', 'fjord', '\ufb01', ' '];
const ODTTF_KEYS = ['{00112233-4455-6677-8899-AABBCCDDEEFF}', '{FFEEDDCC-BBAA-9988-7766-554433221100}', 'short'];
const ADV_EXTRA = [0xffff, 0x10000, 0xffffffff];
const cpOf = (s) => Array.from(s, (c) => c.codePointAt(0));

const sha = (u8) => createHash('sha256').update(u8).digest('hex').slice(0, 32);
const digestOf = (u8) => `${u8.length}:${sha(u8)}`;
const done = (s) => `${s.items}:${s.finish().short}`;

const klass = (e) => {
  const m = String(e?.message ?? e);
  let r;
  if ((r = /^Tabela (.+) ausente$/.exec(m))) return `missing:${r[1]}`;
  if (/cmap sem subtabela/.test(m)) return 'cmap-none';
  if ((r = /cmap format (\d+)/.exec(m))) return `cmap-format:${r[1]}`;
  if ((r = /sem a tabela (.+)$/.exec(m))) return `missing:${r[1]}`;
  if (e instanceof RangeError) return 'range';
  return `other:${m}`;
};

/** numGlyphs of the first `maxp` record, 0 when unreadable (facets.rs `num_glyphs_of`). */
function numGlyphsOf(b) {
  const u16 = (o) => (o + 2 <= b.length ? (b[o] << 8) | b[o + 1] : null);
  const n = u16(4);
  if (n === null) return 0;
  for (let i = 0; i < n; i++) {
    const p = 12 + i * 16;
    if (p + 4 <= b.length && b[p] === 0x6d && b[p + 1] === 0x61 && b[p + 2] === 0x78 && b[p + 3] === 0x70) {
      if (p + 12 > b.length) return 0;
      const off = ((b[p + 8] << 24) | (b[p + 9] << 16) | (b[p + 10] << 8) | b[p + 11]) >>> 0;
      return u16(off + 4) ?? 0;
    }
  }
  return 0;
}

/** The code-point sets of the `gids` facet (facets.rs `cp_sets`). */
function cpSets(depth) {
  const sets = [];
  const a = [];
  for (let cp = 0; cp < 0x3000; cp++) a.push(cp);
  for (let cp = 0x1f600; cp < 0x1f700; cp++) a.push(cp);
  sets.push(a, cpOf(TEXT));
  const rng = new Rng(0xf0f0f001);
  for (let i = 0; i < depth.gidsSets; i++) {
    const len = 1 + rng.below(60);
    const set = [];
    for (let k = 0; k < len; k++) {
      const m = rng.below(8);
      if (m <= 5) set.push(rng.below(0x3000));
      else if (m === 6) set.push(0xf000 + rng.below(0x100));
      else set.push(rng.below(0x110010));
    }
    sets.push(set);
  }
  return sets;
}

// ---- the worker: opens the TS modules once and computes the facets of the fonts it is given ----

async function loadTs(root) {
  const { SfntFont } = await importFrom(root, 'write/packages/docx-layout/dist/typography/font/sfnt.js');
  const { isSymbolFont } = await importFrom(root, 'write/packages/docx-layout/dist/fonts.js');
  const shared = await importFrom(root, 'shared/fonts/dist/sfnt.js');
  const { subsetTrueType } = await importFrom(root, 'shared/fonts/dist/subset.js');
  const { sfntToEot } = await importFrom(root, 'shared/fonts/dist/eot.js');
  const { obfuscateOdttf } = await importFrom(root, 'shared/fonts/dist/ooxml.js');
  const { deobfuscateFont, readEmbeddedFonts } = await importFrom(root, 'write/packages/docx-core/dist/fonts.js');
  const { DocxPackage } = await importFrom(root, 'write/packages/docx-core/dist/package.js');
  const { TtfProgram } = await importFrom(root, 'converter/packages/converter/src/pptx/sfnt.ts');
  return { SfntFont, isSymbolFont, shared, subsetTrueType, sfntToEot, obfuscateOdttf, deobfuscateFont, readEmbeddedFonts, DocxPackage, TtfProgram };
}

function cmapFacet(lookup) {
  const s = new Stream();
  let count = 0, throws = 0, sat = 0;
  const each = (cp) => {
    let g;
    try { g = lookup(cp); } catch { throws++; g = 0; }
    if (g > 0xffffffff) { sat++; g = 0xffffffff; }
    if (g) { count++; s.u32(cp).u32(g); }
  };
  for (let cp = 0; cp <= 0x10ffff; cp++) each(cp);
  each(0x110000);
  each(0xffffffff);
  return { value: `${count}:${s.finish().short}`, g4: { throws, sat } };
}

function advFacet(adv, n) {
  const s = new Stream();
  let throws = 0;
  const ids = [];
  for (let g = 0; g < n + 16; g++) ids.push(g);
  ids.push(...ADV_EXTRA);
  for (const g of ids) {
    let v;
    try { v = adv(g); } catch { throws++; v = 0; }
    s.u32(g).u64(v);
  }
  return { value: done(s), g4: { throws } };
}

function kernFacet(f) {
  const s = new Stream();
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
  return done(s);
}

function twipsFacet(f, depth) {
  const n = f.numGlyphs;
  const s = new Stream();
  const emit = (g1, g2) => {
    s.u32(g1).u32(g2);
    for (const size of SIZES) s.f64(f.kernGlyphsTwips(g1, g2, size));
  };
  const keys = [...f.kernMap.keys()].filter((k) => k < 2 ** 32).sort((a, b) => a - b);
  s.tag('s').u64(keys.length);
  for (const k of keys) emit(Math.floor(k / 65536), k % 65536);
  const g1s = [...f.kernClass0.keys()].sort((a, b) => a - b);
  s.tag('c');
  if (g1s.length === 0) s.u64(0);
  else {
    s.u64(depth.kernClass0);
    const rng = new Rng(0x434c3030);
    for (let i = 0; i < depth.kernClass0; i++) {
      const g1 = g1s[rng.below(g1s.length)];
      const g2 = rng.below(n + 2);
      emit(g1, g2);
    }
  }
  s.tag('r').u64(depth.kernRandom);
  const rng = new Rng(0x4b45524e);
  for (let i = 0; i < depth.kernRandom; i++) {
    const g1 = rng.below(n + 2);
    const g2 = rng.below(n + 2);
    emit(g1, g2);
  }
  return done(s);
}

const gsubOf = (f) => { f.ligate([], []); return f.gsub; };

function gsubFacet(g) {
  const s = new Stream();
  if (g === null || g === undefined) { s.tag('N'); return done(s); }
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
  return done(s);
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

function ligateFacet(f, g, depth) {
  const s = new Stream();
  if (g === null || g === undefined) { s.tag('N'); return done(s); }
  const seqs = [[]];
  for (const li of [...g.lookups.keys()].sort((a, b) => a - b)) {
    const map = g.lookups.get(li);
    for (const first of [...map.keys()].sort((a, b) => a - b)) for (const lig of map.get(first)) seqs.push([first, ...lig.rest]);
  }
  const alpha = alphabet(g);
  const rng = new Rng(0x12345678);
  for (let i = 0; i < depth.ligateSeeded; i++) {
    const len = 1 + rng.below(8);
    const seq = [];
    for (let k = 0; k < len; k++) seq.push(alpha[rng.below(alpha.length)]);
    seqs.push(seq);
  }
  for (const w of WORDS) seqs.push(cpOf(w).map((cp) => { try { return f.glyphId(cp); } catch { return 0; } }));
  const sets = [['liga'], ['liga', 'clig'], ['liga', 'clig', 'dlig', 'hlig']];
  for (const tag of g.features.keys()) sets.push([tag]);
  sets.push([], ['zzzz']);
  for (const seq of seqs) {
    for (const set of sets) {
      const r = f.ligate(seq, set);
      if (r === null) s.tag('n');
      else {
        s.tag('g').u64(r.length);
        for (const group of r) s.u64(group.gid).u64(group.len);
      }
    }
  }
  return done(s);
}

function subsetSets(shared, bytes, depth) {
  const n = numGlyphsOf(bytes);
  const all = [];
  for (let g = 0; g < n; g++) all.push(g);
  const sets = [all];
  let text = [];
  try { text = [...shared.glyphIdsFor(bytes, cpOf(TEXT))].sort((a, b) => a - b); } catch { text = []; }
  sets.push(text, [0], [n, n + 5, 0xffff, 0x10000]);
  const rng = new Rng(0xc0de0001);
  for (let i = 0; i < depth.subsetRandom; i++) {
    const len = 1 + rng.below(Math.min(n + 1, 100));
    const set = [];
    for (let k = 0; k < len; k++) set.push(rng.below(n + 3));
    sets.push(set);
  }
  return sets;
}

/** All units of one font, in order: `[name, fn]` where fn returns a value or `{ value, g4 }`. */
function unitsOf(ts, bytes, depth, entry) {
  const { SfntFont, shared } = ts;
  const units = [];
  const unit = (name, fn) => units.push([name, fn]);
  unit('in', () => digestOf(bytes));
  if (entry.facets === 'ttc') {
    unit('ttc', () => ttcFacet(ts, bytes, entry.probes));
    return units;
  }
  let font = null;
  let open = 'ok';
  try { font = new SfntFont(new Uint8Array(bytes)); } catch (e) { open = klass(e); }
  unit('open', () => open);
  if (font) {
    unit('dir', () => {
      const s = new Stream();
      s.u64(font.tables.size);
      for (const [tag, t] of font.tables) s.str(tag).u64(t.offset).u64(t.length);
      return done(s);
    });
    unit('metrics', () => {
      const m = font.metrics;
      const s = new Stream();
      s.u64(m.unitsPerEm).i64(m.ascender).i64(m.descender).i64(m.lineGap).u64(m.usWinAscent).u64(m.usWinDescent)
        .i64(m.sTypoAscender).i64(m.sTypoDescender).i64(m.sTypoLineGap).u64(m.fsSelection);
      if (m.codePageRange1 !== undefined) s.bool(true).u64(m.codePageRange1); else s.bool(false);
      if (m.ySuperscriptYOffset !== undefined) s.bool(true).i64(m.ySuperscriptYOffset); else s.bool(false);
      if (m.xAvgCharWidth !== undefined) s.bool(true).i64(m.xAvgCharWidth); else s.bool(false);
      if (m.panose !== undefined) { s.bool(true); for (const b of m.panose) s.u64(b); } else s.bool(false);
      s.i64(m.underlinePosition).i64(m.underlineThickness);
      return done(s);
    });
    unit('numglyphs', () => String(font.numGlyphs));
    unit('cmap', () => cmapFacet((cp) => font.glyphId(cp)));
    unit('adv', () => advFacet((g) => font.advanceWidth(g), font.numGlyphs));
    unit('kern', () => kernFacet(font));
    unit('twips', () => twipsFacet(font, depth));
    unit('gsub', () => gsubFacet(gsubOf(font)));
    unit('ligate', () => ligateFacet(font, gsubOf(font), depth));
    unit('vdmx', () => {
      const s = new Stream();
      let frac = 0;
      for (let p = 0; p <= 300; p++) {
        const e = font.vdmx(p);
        if (e) s.bool(true).i64(e.yMax).i64(e.yMin); else s.bool(false);
        if (font.vdmx(p + 0.5)) frac++;
      }
      return { value: done(s), g4: { frac } };
    });
    unit('colr', () => {
      const s = new Stream();
      let throws = 0;
      s.bool(font.hasColorGlyphs());
      for (let g = 0; g < font.numGlyphs + 4; g++) {
        let l;
        try { l = font.colorLayers(g); } catch { l = null; throws++; }
        if (l === null) s.u32(g).tag('N');
        else {
          s.u32(g).tag('L').u64(l.length);
          for (const x of l) {
            s.u64(x.glyphId);
            if (x.color === null) s.tag('n');
            else s.tag('c').u64(parseInt(x.color.slice(0, 2), 16)).u64(parseInt(x.color.slice(2, 4), 16)).u64(parseInt(x.color.slice(4, 6), 16));
            s.f64(x.alpha);
          }
        }
      }
      return { value: done(s), g4: { throws } };
    });
    unit('outline', () => {
      const s = new Stream();
      let nan = 0;
      const ids = [];
      for (let g = 0; g < font.numGlyphs + 2; g++) ids.push(g);
      ids.push(0xffff);
      for (const g of ids) {
        let o = font.glyphOutline(g);
        if (o !== null && o.some((c) => [c.x, c.y, c.cx, c.cy].some((v) => v !== undefined && !Number.isFinite(v)) || (c.op !== 'Z' && (c.x === undefined || c.y === undefined)) || (c.op === 'Q' && (c.cx === undefined || c.cy === undefined)))) { o = null; nan++; }
        s.u32(g);
        if (o === null) s.tag('N');
        else {
          s.tag('O').u64(o.length);
          for (const c of o) {
            if (c.op === 'M' || c.op === 'L') s.tag(c.op).f64(c.x).f64(c.y);
            else if (c.op === 'Q') s.tag('Q').f64(c.cx).f64(c.cy).f64(c.x).f64(c.y);
            else s.tag('Z');
          }
        }
      }
      return { value: done(s), g4: { nan } };
    });
    unit('symbol', () => String(ts.isSymbolFont(font)));
  }
  unit('program', () => {
    let p;
    try { p = new ts.TtfProgram(bytes); } catch (e) { return klass(e); }
    const s = new Stream();
    s.u64(p.unitsPerEm).i64(p.ascender).i64(p.descender);
    for (const v of p.fontBBox) s.i64(v);
    s.f64(p.italicAngle);
    return `ok:${done(s)}`;
  });
  let prog = null;
  try { prog = new ts.TtfProgram(bytes); } catch { prog = null; }
  if (prog) {
    unit('program.cmap', () => cmapFacet((cp) => prog.glyphId(cp)));
    unit('program.adv', () => advFacet((g) => prog.advanceWidth(g), numGlyphsOf(bytes)));
  }
  unit('info', () => {
    const s = new Stream();
    s.bool(shared.isFontFile(bytes)).bool(shared.isEmbeddable(bytes)).bool(shared.hasTrueTypeOutlines(bytes));
    try {
      const i = shared.faceInfo(bytes);
      s.tag('I').str(i.panose).u64(i.weight).bool(i.italic).u64(i.fsType);
      for (const v of i.unicodeRange) s.u64(v);
      for (const v of i.codePageRange) s.u64(v);
      s.u64(i.checkSumAdjustment);
      for (const name of [i.names.family, i.names.style, i.names.version, i.names.full]) {
        s.u64(name.length);
        for (let k = 0; k < name.length; k++) s.u32(name.charCodeAt(k));
      }
    } catch { s.tag('E'); }
    return done(s);
  });
  unit('gids', () => {
    const s = new Stream();
    let sat = 0, throws = 0;
    for (const set of cpSets(depth)) {
      try {
        const ids = new Set();
        for (let g of shared.glyphIdsFor(bytes, set)) {
          if (g > 0xffffffff) { sat++; g = 0xffffffff; }
          ids.add(g);
        }
        const sorted = [...ids].sort((a, b) => a - b);
        s.tag('I').u64(sorted.length);
        for (const id of sorted) s.u32(id);
      } catch { throws++; s.tag('E'); }
    }
    return { value: done(s), g4: { sat } };
  });
  unit('subset', () => null);
  unit('eot', () => {
    const s = new Stream();
    for (const subsetted of [false, true]) {
      let e;
      try { e = ts.sfntToEot(bytes, { subsetted }); } catch { e = null; }
      if (e === null) s.tag('E'); else s.tag('O').digestOf(e);
    }
    return done(s);
  });
  unit('odttf', () => {
    const s = new Stream();
    for (const key of ODTTF_KEYS) {
      s.digestOf(ts.obfuscateOdttf(bytes, key));
      s.digestOf(ts.deobfuscateFont(bytes, key));
    }
    return done(s);
  });
  unit('ttc', () => ttcFacet(ts, bytes, entry.probes));
  return units;
}

function ttcFacet(ts, bytes, probes) {
  const s = new Stream();
  const run = (want) => {
    try {
      const r = ts.shared.standaloneFace(bytes, want);
      if (r === bytes) s.tag('S'); else s.tag('O').digestOf(r);
    } catch { s.tag('E'); }
  };
  run(undefined);
  for (const p of probes) run(p);
  return done(s);
}

/** The `subset` cases of a font: one `subsetTrueType` call per set and `keepLayout` flag. */
function subsetCases(ts, bytes, depth) {
  return subsetSets(ts.shared, bytes, depth).flatMap((set, i) => [false, true].map((keep) => ({ idx: i * 2 + (keep ? 1 : 0), set, keep })));
}

function bytesOf(ts, entry, cache) {
  if (entry.kind === 'file') return new Uint8Array(readFileSync(join(entry.repo, entry.args[0])));
  if (entry.kind === 'gen') return new Uint8Array(readFileSync(join(entry.corpus, entry.args[0])));
  if (entry.kind === 'abs') return new Uint8Array(readFileSync(entry.args[0]));
  if (entry.kind === 'embedded') {
    const [docx, family, variant] = entry.args;
    let fonts = cache.get(docx);
    if (!fonts) {
      fonts = ts.readEmbeddedFonts(ts.DocxPackage.load(new Uint8Array(readFileSync(join(entry.repo, docx)))));
      cache.set(docx, fonts);
    }
    const f = fonts.find((x) => x.family === family && x.variant === variant);
    if (!f) throw new Error(`embedded font not found: ${docx} ${family} ${variant}`);
    return f.data;
  }
  throw new Error(`kind ${entry.kind}`);
}

if (!isMainThread) {
  const { root, entries, start, skip } = workerData;
  const ts = await loadTs(root);
  const cache = new Map();
  const skipSet = new Set(skip);
  for (let i = start; i < entries.length; i++) {
    const entry = entries[i];
    const bytes = bytesOf(ts, entry, cache);
    const depth = DEPTHS[entry.depth];
    if (skipSet.has(`${i}|setup`)) {
      parentPort.postMessage({ facet: [i, 'open', 'hang'] });
      parentPort.postMessage({ fontDone: i });
      continue;
    }
    parentPort.postMessage({ begin: [i, 'setup'], limit: 120000 });
    const units = unitsOf(ts, bytes, depth, entry);
    for (const [name, fn] of units) {
      if (name === 'subset') {
        const s = new Stream();
        const hang = [];
        const guard = vm.createContext({ subset: ts.subsetTrueType, bytes, set: [], keep: false });
        for (const c of subsetCases(ts, bytes, depth)) {
          s.u64(c.idx);
          guard.set = c.set;
          guard.keep = c.keep;
          let out;
          try {
            out = vm.runInContext('subset(bytes, set, { keepLayout: keep })', guard, { timeout: 4000 + Math.ceil(bytes.length / 500) });
          } catch (e) {
            if (e?.code === 'ERR_SCRIPT_EXECUTION_TIMEOUT') { s.tag('H'); hang.push(c.idx); continue; }
            out = undefined;
          }
          if (out === undefined) s.tag('T');
          else if (out === bytes) s.tag('S');
          else s.tag('O').digestOf(out);
        }
        parentPort.postMessage({ facet: [i, 'subset', done(s)], hang });
        continue;
      }
      if (skipSet.has(`${i}|${name}`)) { parentPort.postMessage({ facet: [i, name, 'hang'] }); continue; }
      parentPort.postMessage({ begin: [i, name], limit: 120000 });
      const r = fn();
      parentPort.postMessage({ facet: [i, name, r] });
    }
    parentPort.postMessage({ fontDone: i });
  }
  parentPort.postMessage({ end: true });
}

// ---- the main thread ----

if (isMainThread) {
  const argv = process.argv.slice(2);
  const arg = (name) => { const i = argv.indexOf(name); return i >= 0 ? argv[i + 1] : undefined; };
  const root = tsRoot(argv);
  const only = arg('--only');
  const corpusDir = arg('--corpus') ? resolve(arg('--corpus')) : null;
  const localDirs = arg('--dir') ? arg('--dir').split(',').map((d) => resolve(d)) : [];
  const localDir = localDirs.length ? localDirs[0] : null;
  if (!corpusDir && !localDir) throw new Error('pass --corpus <dir written by the fonts_corpus example> or --dir <folder of fonts> --out <cap>');
  if (localDir && !arg('--out')) throw new Error('--dir needs --out (the capture of proprietary fonts is never committed)');
  const out = resolve(arg('--out') ?? join(here, '..', 'parity', 'fonts.cap'));
  const repo = resolve(arg('--repo') ?? repoRoot);
  let entries = [];
  if (corpusDir) {
    for (const line of readFileSync(join(corpusDir, 'manifest.tsv'), 'utf8').split('\n').filter(Boolean)) {
      const [key, depth, facets, probes, kind, ...args] = line.split('\t');
      entries.push({ key, depth, facets, probes: probes === '' ? [] : probes.split('|'), kind, args, repo, corpus: corpusDir });
    }
  } else {
    const depth = arg('--depth') ?? 'light';
    localDirs.forEach((dir, d) => {
      for (const p of walk(dir, (f) => /\.(ttf|otf|ttc|otc)$/i.test(f))) {
        const rel = p.slice(dir.length + 1).replaceAll('\\', '/');
        entries.push({ key: `local${d}/${rel}`, depth, facets: /\.(ttc|otc)$/i.test(p) ? 'ttc' : 'all', probes: [], kind: 'abs', args: [p], repo, corpus: null });
      }
    });
  }
  if (only) entries = entries.filter((e) => e.key.includes(only));
  const results = entries.map(() => ({ facets: new Map(), g4: new Map(), hang: [] }));
  const skip = [];
  let start = 0;
  const t0 = Date.now();
  while (start < entries.length) {
    await new Promise((resolveRun) => {
      const worker = new Worker(fileURLToPath(import.meta.url), { workerData: { root, entries, start, skip }, resourceLimits: { maxOldGenerationSizeMb: Number(process.env.UO_FONTS_HEAP_MB ?? 4096) } });
      let cur = [start, 'setup'];
      let limit = 120000;
      let last = Date.now();
      const tick = setInterval(() => {
        if (Date.now() - last > limit) {
          clearInterval(tick);
          console.error(`hang: ${entries[cur[0]].key} ${cur[1]} (restart)`);
          skip.push(`${cur[0]}|${cur[1]}`);
          start = cur[0];
          worker.terminate().then(resolveRun);
        }
      }, 500);
      worker.on('message', (m) => {
        last = Date.now();
        if (m.begin) { cur = m.begin; limit = m.limit; }
        if (m.facet) {
          const [i, name, value] = m.facet;
          const r = results[i];
          if (value && typeof value === 'object') {
            r.facets.set(name, value.value);
            const parts = Object.entries(value.g4 ?? {}).filter(([, v]) => v > 0).map(([k, v]) => `${k}=${v}`);
            if (parts.length) r.g4.set(`${name}.g4`, parts.join(','));
          } else r.facets.set(name, value);
          if (m.hang) r.hang = m.hang;
        }
        if (m.fontDone !== undefined) { start = m.fontDone + 1; if ((m.fontDone + 1) % 25 === 0) console.error(`${m.fontDone + 1}/${entries.length} fonts, ${Math.round((Date.now() - t0) / 1000)} s`); }
        if (m.end) { clearInterval(tick); start = entries.length; worker.terminate().then(resolveRun); }
      });
      worker.on('error', (e) => {
        clearInterval(tick);
        if (e?.code !== 'ERR_WORKER_OUT_OF_MEMORY') { console.error(e); process.exit(1); }
        console.error(`out of memory: ${entries[cur[0]].key} ${cur[1]} (restart)`);
        skip.push(`${cur[0]}|${cur[1]}`);
        start = cur[0];
        resolveRun();
      });
    });
  }
  const cap = new CaptureWriter(['uo-fonts stage-4 parity capture: the frozen TS over the font corpus (digests of canonical streams, never font bytes).',
    'Written by core/crates/uo-fonts/tests/parity_support/capture_fonts.mjs; replayed by core/crates/uo-fonts/tests/fonts_parity.rs.']);
  let commit = 'unknown';
  try { commit = execFileSync('git', ['-C', root, 'rev-parse', 'HEAD'], { encoding: 'utf8' }).trim(); } catch { /* not a git checkout */ }
  cap.meta('ts-commit', commit);
  cap.meta('inputs', `${entries.length} fonts`);
  for (let i = 0; i < entries.length; i++) {
    const { facets, g4, hang } = results[i];
    for (const [name, value] of facets) cap.set(entries[i].key, name, value);
    for (const [name, value] of g4) cap.set(entries[i].key, name, value);
    if (hang.length) cap.set(entries[i].key, 'subset.hang', hang.join(','));
  }
  if (corpusDir && !only) await smallFacets(cap, root, corpusDir);
  cap.write(out);
  console.log(`${out}: ${entries.length} fonts in ${Math.round((Date.now() - t0) / 1000)} s`);
}
