// P3, the per-stage facets (record §5: localization, not the gate). Each frontier owns its facet
// here and the matching Rust writer next to its module (same name, same stream): a P1 mismatch
// then points at the first stage that differs. F0 ships the mechanism and two seed facets:
//
//   p3.prepare  every `prepareParagraph` result (para.ts:729), in call order (all passes) —
//               F4 replaced the F0 seed (facets-f4.mjs: the whole PreparedPara per call, with
//               the call's inputs dumped for tests/f4_parity.rs);
//   p3.lines    every `LineBreaker.next(frame)` (lines.ts) with its frame and the line it
//               returned — the F0 seed; F5 replaces it (facets-f5.mjs: every breaker call with
//               the placed glyph identity, plus p3.paint) when its dependencies are passed.
//
// Functions are wrapped by the loader hook (loader.mjs) for module functions and by prototype
// patching for classes; both are in memory only, the frozen dist is never modified.
import { register } from 'node:module';
import { join } from 'node:path';
import { pathToFileURL } from 'node:url';
import { Sink } from './sink.mjs';
import { F1_WRAPS, f1Recorder } from './facets-f1.mjs';
import { F3_WRAPS, f3Recorder } from './facets-f3.mjs';
import { F5_WRAPS, f5Recorder } from './facets-f5.mjs';
import { F2_WRAPS, f2Recorder } from './facets-f2.mjs';
import { F4_WRAPS, f4Recorder } from './facets-f4.mjs';
import { F6_WRAPS, f6Recorder } from './facets-f6.mjs';

/** Wrapped module functions per dist file of docx-layout (paths relative to the dist). */
export const WRAPS = {
  'para.js': ['prepareParagraph'],
};
// F1 (fonts.ts faceMetrics, para.ts advanceTwips): facets-f1.mjs
for (const [file, names] of Object.entries(F1_WRAPS)) WRAPS[file] = [...(WRAPS[file] ?? []), ...names];
// F3 (drawing values and metafiles): facets-f3.mjs
for (const [file, names] of Object.entries(F3_WRAPS)) WRAPS[file] = [...(WRAPS[file] ?? []), ...names];
// F5 (lines.ts LineBreaker, paint.ts paintLine): facets-f5.mjs (p3.lines replaces the F0 seed)
for (const [file, names] of Object.entries(F5_WRAPS)) WRAPS[file] = [...(WRAPS[file] ?? []), ...names];
// F4 (resolve.ts mergePPr/mergeRPr: the table-style env of cell paragraphs): facets-f4.mjs
for (const [file, names] of Object.entries(F4_WRAPS)) WRAPS[file] = [...(WRAPS[file] ?? []), ...names];
// F2 (datefmt.ts, field-formula.ts, engine.ts layoutDocument for the hyphenation walk): facets-f2.mjs
for (const [file, names] of Object.entries(F2_WRAPS)) WRAPS[file] = [...(WRAPS[file] ?? []), ...names];
// F6 (table.ts layoutTable/splitContent, para.ts paraVisibility): facets-f6.mjs
for (const [file, names] of Object.entries(F6_WRAPS)) WRAPS[file] = [...(WRAPS[file] ?? []), ...names];

/** Installs the loader hook; call before importing the docx-layout dist. */
export function installFacetHooks(tsRoot) {
  const dist = join(tsRoot, 'write/packages/docx-layout/dist');
  const wraps = {};
  for (const [file, names] of Object.entries(WRAPS)) wraps[pathToFileURL(join(dist, file)).href] = names;
  register('./loader.mjs', import.meta.url, { data: { wraps } });
}

/** Facet recorder for one layout run; `start()` before `layoutDocument`, `finish()` after. */
export function facetRecorder({ LineBreaker, FaceResolver, SfntFont, f5: f5Deps, f2: f2Deps, f6: f6Deps }) {
  // F2: p3.datefmt, p3.formula, p3.numbering, p3.style, p3.hyphlangs (+ the `f2.tsv` dump of their inputs)
  const f2 = f2Deps ? f2Recorder(f2Deps) : null;
  let prep = null, lines = null, paint = null;
  // F5: p3.lines, p3.paint (+ the `f5.bin` dump of their inputs); without its dependencies the F0
  // seed of p3.lines below stays
  const f5 = f5Deps ? f5Recorder({ LineBreaker, ...f5Deps }) : null;
  // F1: p3.metrics, p3.advance, p3.kern, p3.faces (+ the `f1.tsv` dump of their inputs)
  const f1 = FaceResolver && SfntFont ? f1Recorder({ FaceResolver, SfntFont }) : null;
  // F3: p3.emf, p3.wmf, p3.picture, p3.pagefill, p3.textfx, p3.shape (+ the `f3.jsonl` dump of their inputs)
  const f3 = f3Recorder();
  // F4: p3.prepare (+ the `f4.bin` dump of its inputs)
  const f4 = f4Recorder();
  // F6: p3.table, p3.rowsplit, p3.splitcontent (+ the `f6.bin` dump of their inputs)
  const f6 = f6Deps ? f6Recorder(f6Deps) : null;
  let unhookF4 = null;
  const origNext = LineBreaker.prototype.next;
  const patched = function (frame) {
    const l = origNext.call(this, frame);
    if (lines) {
      const s = lines;
      s.tag('n').num(frame.left).num(frame.right).bool(!!frame.gap).optNum(frame.next?.left).optNum(frame.next?.right);
      s.num(l.start).num(l.end).num(l.width).num(l.xStart).num(l.asc).num(l.desc).num(l.height).num(l.baseline).bool(!!l.last)
        .optU16s(l.breakAfter).bool(!!l.clear).optU16s(l.gapTab).bool(!!l.spanned).u32(l.placed.length);
      for (const q of l.placed) s.num(q.x).num(q.w).optU16s(q.leader);
    }
    return l;
  };
  return {
    start() {
      prep = new Sink(); lines = new Sink(); paint = new Sink();
      if (f5) { f5.start(); f5.bind(lines, paint); } else LineBreaker.prototype.next = patched;
      f4.start(); f4.bind(prep);
      globalThis.__uoFacetPre = (name, args) => {
        if (name === 'prepareParagraph') return f4.pre(name, args);
        if (name === 'layoutDocument') f4.layout(args[0], args[2]);
        return f5 ? f5.pre(name, args) : undefined;
      };
      f1?.start();
      f3.start();
      globalThis.__uoFacet = (name, args, r, pre) => { f4.report(name, args, r, pre); f1?.report(name, args, r); f3.report(name, args, r); f5?.report(name, args, r, pre); };
      // F2, chained on the hooks above
      if (f2) {
        f2.start();
        const pre = globalThis.__uoFacetPre, rep = globalThis.__uoFacet;
        globalThis.__uoFacetPre = (n, a) => { f2.pre(n, a); return pre ? pre(n, a) : undefined; };
        globalThis.__uoFacet = (n, a, r, p) => { f2.report(n, a, r); rep(n, a, r, p); };
      }
      // F6, chained on the hooks above (its own pre value for layoutTable/splitContent)
      if (f6) {
        f6.start();
        const pre = globalThis.__uoFacetPre, rep = globalThis.__uoFacet;
        globalThis.__uoFacetPre = (n, a) => {
          if (n === 'layoutDocument') f6.layout(a[0]);
          if (n === 'layoutTable' || n === 'splitContent') return f6.pre(n, a);
          return pre ? pre(n, a) : undefined;
        };
        globalThis.__uoFacet = (n, a, r, p) => { f6.report(n, a, r, p); rep(n, a, r, p); };
      }
      // F4: the table styles of every resolver (after F2 patched the class)
      unhookF4 = f2Deps?.StyleResolver ? f4.hook({ StyleResolver: f2Deps.StyleResolver }) : null;
    },
    finish() {
      unhookF4?.();
      unhookF4 = null;
      globalThis.__uoFacet = undefined;
      globalThis.__uoFacetPre = undefined;
      LineBreaker.prototype.next = origNext;
      const out = { 'p3.prepare': prep.finish().value, 'p3.lines': lines.finish().value };
      if (f5) out['p3.paint'] = paint.finish().value;
      // dump files of the facets' inputs (`<key>.<name>` in the --dump dir): not facets, so not enumerable
      const dumps = {};
      if (f1) { const r = f1.finish(); Object.assign(out, r.facets); dumps['f1.tsv'] = r.dump; }
      { const r = f3.finish(); Object.assign(out, r.facets); if (r.dump) dumps['f3.jsonl'] = r.dump; }
      if (f5) dumps['f5.bin'] = f5.finish();
      dumps['f4.bin'] = f4.finish();
      if (f2) { const r = f2.finish(); Object.assign(out, r.facets); dumps['f2.tsv'] = r.dump; }
      if (f6) { const r = f6.finish(); Object.assign(out, r.facets); dumps['f6.bin'] = r.dump; }
      Object.defineProperty(out, 'dumps', { value: dumps, enumerable: false });
      prep = null; lines = null; paint = null;
      return out;
    },
  };
}
