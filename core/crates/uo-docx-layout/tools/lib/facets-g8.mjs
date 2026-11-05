// The G8 facets of uo-docx-layout (record "Write page section (G8)"): what the frozen TS hands the
// viewer for each page — the Write geometry of the LayoutPage (lines with chars, pfmt by value,
// objects, cells, hfItems, section, number, the clip items with a `src`) and what the viewer's text
// layer reads from the MODEL for each line: `para(line.ref)` (universal-document-viewer.ts: the
// paragraph of a `b…` ref through `paragraphAtPath`), its text `p.runs.map((r) => r.text).join('')`
// and its heading level `levelResolver(full)(p)` (the real viewer dist, annotations.js). The Rust
// replay (tests/g8.rs) writes the same stream from the DECODED Write section of its page messages.
//
// Stream (the Sink of sink.mjs), per document:
//   'W' pages; per page 'P' section num(number) optNum(hfItems) lines… objects? cells? clips
//   line: 'n' u16s(ref) num(start) num(end) num(x y height baseline width) chars? hyph? rev? frame?
//         pfmt? and, for a page line (not textbox content), the text layer: bool(para resolves) then
//         bool(first line of that paragraph on this page) [+ u16s(text) optNum(level) | u32(first
//         line index)] — paragraph identity is the object's (the text layer's block grouping)
//   object: 'o' optU16s(runRefs name) num(x y w h) u16s(layer) bool(hf) frame? content?
//   cell: 'k' u16s(ref) num(x y w h span); clips: u32(n) then the item index of each clip with a src
// Plus `g8.annotations`: `collectAnnotations(full, layout)` of the viewer (comments with their rects,
// headings), the oracle of the Rust `annotations` query.
import { Sink } from './sink.mjs';
import { runRefs } from './canon.mjs';

export async function setupG8(root, importFrom) {
  const ann = await importFrom(root, 'write/packages/viewer/dist/annotations.js');
  const lay = await importFrom(root, 'write/packages/docx-layout/dist/index.js');
  return { levelResolver: ann.levelResolver, collectAnnotations: ann.collectAnnotations, paragraphAtPath: lay.paragraphAtPath };
}

function line(s, l) {
  s.tag('n').u16s(l.ref).num(l.start).num(l.end);
  for (const v of [l.x, l.y, l.height, l.baseline, l.width]) s.num(v);
  s.bool(l.chars !== undefined);
  if (l.chars !== undefined) {
    s.u32(l.chars.length);
    for (const c of l.chars) s.num(c.o).num(c.x).num(c.w);
  }
  s.optNum(l.hyph);
  s.bool(l.rev !== undefined);
  if (l.rev !== undefined) s.num(l.rev.before).num(l.rev.after);
  s.bool(l.frame !== undefined);
  if (l.frame !== undefined) s.num(l.frame.l).num(l.frame.r);
  s.bool(l.pfmt !== undefined);
  if (l.pfmt !== undefined) {
    s.num(l.pfmt.left).num(l.pfmt.right).num(l.pfmt.firstLine).u32(l.pfmt.tabs.length);
    for (const t of l.pfmt.tabs) s.u16s(t.val).num(t.pos).optU16s(t.leader);
  }
}

export function g8Facets(G, full, res) {
  const levelOf = G.levelResolver(full);
  const cache = new Map();
  // the viewer's `para` (universal-document-viewer.ts), cached by ref as there
  const para = (ref) => {
    let p = cache.get(ref);
    if (p === undefined) {
      p = /^b\d/.test(ref) ? G.paragraphAtPath(full.blocks, ref.split('/')) : null;
      cache.set(ref, p);
    }
    return p;
  };
  const names = runRefs(full);
  const s = new Sink();
  s.tag('W').u32(res.pages.length);
  for (const p of res.pages) {
    s.tag('P').u32(p.section).num(p.number).optNum(p.hfItems);
    s.u32(p.lines.length);
    const first = new Map();
    p.lines.forEach((l, i) => {
      line(s, l);
      const q = para(l.ref);
      s.bool(!!q);
      if (!q) return;
      const f = first.get(q);
      s.bool(f === undefined);
      if (f === undefined) {
        first.set(q, i);
        s.u16s(q.runs.map((r) => r.text).join('')).optNum(levelOf(q));
      } else s.u32(f);
    });
    s.bool(p.objects !== undefined);
    if (p.objects !== undefined) {
      s.u32(p.objects.length);
      for (const o of p.objects) {
        s.tag('o').optU16s(names.get(o.run));
        for (const v of [o.x, o.y, o.w, o.h]) s.num(v);
        s.u16s(o.layer).bool(!!o.hf);
        s.bool(o.frame !== undefined);
        if (o.frame !== undefined) {
          const f = o.frame;
          for (const v of [f.colX, f.colW, f.paraTop, f.pageW, f.pageH, f.marL, f.marR, f.marT, f.marB, f.bodyTop]) s.num(v);
        }
        s.bool(o.content !== undefined);
        if (o.content !== undefined) {
          s.u32(o.content.lines.length);
          for (const l of o.content.lines) line(s, l);
          for (const v of o.content.xf) s.num(v);
        }
      }
    }
    s.bool(p.cells !== undefined);
    if (p.cells !== undefined) {
      s.u32(p.cells.length);
      for (const c of p.cells) s.tag('k').u16s(c.ref).num(c.x).num(c.y).num(c.w).num(c.h).num(c.span);
    }
    const clips = [];
    p.items.forEach((it, i) => { if (it.t === 'clip' && it.src) clips.push(i); });
    s.u32(clips.length);
    for (const i of clips) s.u32(i);
  }
  const out = { 'g8.section': s.finish().value };
  // the viewer's annotations (comments and headings) over the same layout
  const a = G.collectAnnotations(full, res);
  const t = new Sink();
  t.tag('A').u32(a.comments.length);
  for (const c of a.comments) {
    t.tag('c').num(c.id).optU16s(c.author).u16s(c.text).u32(c.rects.length);
    for (const r of c.rects) t.u32(r.page).num(r.x).num(r.y).num(r.w).num(r.h);
  }
  t.u32(a.headings.length);
  for (const h of a.headings) t.tag('h').num(h.level).u16s(h.text).u32(h.page).num(h.y);
  out['g8.annotations'] = t.finish().value;
  return out;
}
