// P3, the per-stage facets (record §5: localization, not the gate). Each frontier owns its facet
// here and the matching Rust writer next to its module (same name, same stream): a P1 mismatch
// then points at the first stage that differs. F0 ships the mechanism and two seed facets:
//
//   p3.prepare  every `prepareParagraph` result (para.ts:729), in call order (all passes):
//               the paragraph metrics and every glyph's kind, code point, advance, ascent,
//               descent, offset, run index, break flag and size — F4 extends it (face label,
//               flags, marker, revision bars);
//   p3.lines    every `LineBreaker.next(frame)` (lines.ts) with its frame and the line it
//               returned — F5 extends it (placed glyph identity across the marker, hyphens).
//
// Functions are wrapped by the loader hook (loader.mjs) for module functions and by prototype
// patching for classes; both are in memory only, the frozen dist is never modified.
import { register } from 'node:module';
import { join } from 'node:path';
import { pathToFileURL } from 'node:url';
import { Sink } from './sink.mjs';

/** Wrapped module functions per dist file of docx-layout (paths relative to the dist). */
export const WRAPS = {
  'para.js': ['prepareParagraph'],
};

/** Installs the loader hook; call before importing the docx-layout dist. */
export function installFacetHooks(tsRoot) {
  const dist = join(tsRoot, 'write/packages/docx-layout/dist');
  const wraps = {};
  for (const [file, names] of Object.entries(WRAPS)) wraps[pathToFileURL(join(dist, file)).href] = names;
  register('./loader.mjs', import.meta.url, { data: { wraps } });
}

/** Facet recorder for one layout run; `start()` before `layoutDocument`, `finish()` after. */
export function facetRecorder({ LineBreaker }) {
  let prep = null, lines = null;
  const prepare = (_args, p) => {
    const s = prep;
    s.tag('p').num(p.indLeft).num(p.indRight).num(p.firstLine).u16s(p.jc).num(p.spaceBefore).num(p.spaceAfter)
      .u16s(p.lineRule).num(p.line).bool(!!p.empty).u32(p.glyphs.length);
    for (const g of p.glyphs) {
      s.u16s(g.kind).num(g.cp).num(g.adv).num(g.asc).num(g.desc).num(g.offset).num(g.runIdx).bool(!!g.breakAfter).num(g.sizePt);
    }
  };
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
      prep = new Sink(); lines = new Sink();
      LineBreaker.prototype.next = patched;
      globalThis.__uoFacet = (name, args, r) => { if (name === 'prepareParagraph' && prep) prepare(args, r); };
    },
    finish() {
      globalThis.__uoFacet = undefined;
      LineBreaker.prototype.next = origNext;
      const out = { 'p3.prepare': prep.finish().value, 'p3.lines': lines.finish().value };
      prep = null; lines = null;
      return out;
    },
  };
}
