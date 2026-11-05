// P3 facet of frontier F4 (paragraph preparation, hyphenation marks and equation layout; record
// "Stage 3 — F4"), recorded by facets.mjs during every layout of capture-layout.mjs, with its
// inputs dumped for the Rust replay (tests/f4_parity.rs), which reads the same docx with the
// uo-docx reader, runs `prepare_paragraph` and rebuilds the same stream:
//
//   p3.prepare  every `prepareParagraph(ctx, para, env)` (para.ts:729) in call order (all passes,
//               measuring calls inside `pure()` included): one `bytes` item per call holding the
//               raw encoding of the PreparedPara below (replaces the F0 seed of facets.mjs).
//
// What the call reads outside the model and the pure Word semantics is in the dump `<key>.f4.bin`
// (a raw Sink stream), per call, in call order:
//   'O' optU16s(opts.revisions.mode) bool(insDel?)… once per layout:  u16s(mode|'') u16s(lang)
//   'Q' u32(call) u16s(para ref | '') bool(model) env transcript* 'E' bytes(TS raw value)
//   env  := bool(tablePPr) bool(tableRPr) bool(field) optBool(numbering) bool(darkBackground
//           noPageBreaks hf continuation) optU16s(defaultColor)
//   transcript (the answers of the call, in order):
//     'f' u16s(name) u16s(instr) num(at) optU16s(answer)                    env.field
//     'm' num(numId) num(ilvl) bool(marker?)[u16s(text) u16s(level kind 'o'|'a') u16s(k1) u16s(k2)
//         num(numId) num(ilvl) num(value) u16s(listKey) optU16s(chapter) bool(parts?)[u32 n u16s*]]
//                                                                           ctx.numbering.next
//     'n' u16s(kind) num(id) bool(custom) u16s(mark)                       ctx.noteMark
// Para refs are the model positions of the paragraph object (`paraRefs(full)` below, the walk of
// canon.mjs `runRefs` at paragraph level): `b3`, `b3/r0/c1/b0`, `hf<n>/b0`, `fn<id>/b0`,
// `<run ref>/tx/b<k>`; '' = not a model paragraph (a copy the engine derives, LA2).
//
// The value of a call (Raw encoding of facets-f5.mjs):
//   'P' optU16s(styleId) u16s(digest(ppr)) u32(markStyle) u32(markFace)
//       num(indLeft indRight firstLine) u16s(jc) u32 n (u16s(val) num(pos) optU16s(leader))*
//       num(spaceBefore spaceAfter) bool(beforeAuto afterAuto) u16s(lineRule) num(line)
//       bool(contextual keepNext keepLines widowControl pageBreakBefore empty)
//       bool(revBars?)[u32 n (num num bool)* bool(whole) bool(mark)]
//       bool(marker?)[u16s(text) num(width) u16s(level) u16s(suff) u16s(jc) bool(picBullet?)
//                     optU16s(chapter) u32 n glyph*]
//       u32 n glyph*
//   with faces and styles defined inline at first use in the call:
//   'F' u32 u16s(key family) bool(bold italic synthBold synthItalic standIn)
//   'S' u32 num(sizePt) u16s(color) bool(autoColor bold italic) bool(underline?)[u16s(val) optU16s(color)]
//       bool(strike dstrike) optU16s(highlight shading) num(shift) bool(hidden) optU16s(link)
//       optNum(alpha) bool(outline) bool(fx?)[textfx] optU16s(fxKey) u16s(digest(rpr))
//   glyph := (refs first) u16s(kind ch) num(cp) u32(face) u32(style) num(adv asc desc) optNum(nAsc nDesc)
//            num(offset runIdx) bool(breakAfter glue clear fixed hang hyph) num(sizePt)
//            optNum(hr feShrink kernAdj ligStart ligEnd subGid noteId)
//            obj := '-' | 'r' u16s(run ref) | 'b' num(w h) | '?'
//            bool(ptab?)[u16s(alignment relativeTo) optU16s(leader)] bool(box?)[num(side) bool(checked)]
//            bool(math?)[num(w top bottom lineTop lineBottom) u32 n (num(cp x y) u32(face) num(sizePt)
//            u32(style))* u32 n (num(x y w h) u16s(color))*]
// Digests (ppr, rpr): the uo-docx canonical digest (canon.mjs) of the object, as facets-f2.mjs.
import { Stream } from '../../../uo-testkit/js/testkit.mjs';
import { Canon } from '../../../uo-docx/js/canon.mjs';
import { Raw, textfx } from './facets-f5.mjs';

/** Module functions F4 wraps (merged into facets.mjs WRAPS): `mergePPr`/`mergeRPr` record how each
 *  merged object was built, so the table-style env of a cell paragraph (table.ts:741–757: the
 *  style's pPr/rPr merged with its conditional parts) is replayed as (style key, conditions);
 *  `prepareParagraph` itself is wrapped by facets.mjs. */
export const F4_WRAPS = { 'resolve.js': ['mergePPr', 'mergeRPr'] };

const T = (v) => v !== undefined && v !== null;

const digest = (v) => {
  const s = new Stream();
  new Canon(s).value(v, 'x');
  return `${s.items}:${s.finish().short}`;
};

/** Paragraph object -> model position, run object -> model position (`#i`, field results `/f<i>`). */
export function modelRefs(full) {
  const paras = new Map(), runs = new Map();
  const blocksOf = (c) => c.blocks ?? c.paragraphs?.map((paragraph) => ({ type: 'paragraph', paragraph })) ?? [];
  const runList = (rs, base) => {
    rs.forEach((r, i) => {
      const ref = `${base}#${i}`;
      if (!runs.has(r)) runs.set(r, ref);
      (r.fieldResult ?? []).forEach((fr, j) => fieldResult(fr, `${ref}/f${j}`));
      if (r.textbox) walk(r.textbox.blocks ?? r.textbox.paras.map((paragraph) => ({ type: 'paragraph', paragraph })), `${ref}/tx`);
      (r.group?.items ?? []).forEach((it, j) => runList([it.run], `${ref}/g${j}`));
    });
  };
  const fieldResult = (r, ref) => {
    if (!runs.has(r)) runs.set(r, ref);
    (r.fieldResult ?? []).forEach((fr, j) => fieldResult(fr, `${ref}/f${j}`));
  };
  const walk = (bs, base) => {
    bs.forEach((b, i) => {
      const p = `${base}/b${i}`;
      if (b.type === 'paragraph') {
        if (!paras.has(b.paragraph)) paras.set(b.paragraph, p);
        runList(b.paragraph.runs, p);
      } else b.rows.forEach((row, ri) => row.cells.forEach((c, ci) => walk(blocksOf(c), `${p}/r${ri}/c${ci}`)));
    });
  };
  walk(full.blocks, '');
  const hf = new Map();
  for (const s of full.sections ?? []) {
    for (const [set, ps] of [[s.headerBlocks, s.headers], [s.footerBlocks, s.footers]]) {
      for (const k of ['default', 'first', 'even']) {
        const arr = set?.[k] ?? ps?.[k];
        if (!arr || hf.has(arr)) continue;
        hf.set(arr, `hf${hf.size}`);
        walk(set?.[k] ?? arr.map((paragraph) => ({ type: 'paragraph', paragraph })), hf.get(arr));
      }
    }
  }
  for (const [kind, blocks, ps] of [['fn', full.footnoteBlocks, full.footnotes], ['en', full.endnoteBlocks, full.endnotes]]) {
    for (const [id, list] of ps ?? []) walk(blocks?.get(id) ?? list.map((paragraph) => ({ type: 'paragraph', paragraph })), `${kind}${id}`);
  }
  const strip = (m) => { for (const [k, ref] of m) if (ref.startsWith('/')) m.set(k, ref.slice(1)); };
  strip(paras); strip(runs);
  return { paras, runs };
}

/** level object -> [kind, k1, k2] (`nums[k1].overrides[k2].lvl` = 'o', `abstractNums[k1].levels[k2]` = 'a') */
function levelIndex(numbering) {
  const out = new Map();
  for (const [n, num] of Object.entries(numbering?.nums ?? {})) for (const [i, ov] of Object.entries(num.overrides ?? {})) if (ov?.lvl && !out.has(ov.lvl)) out.set(ov.lvl, ['o', n, i]);
  for (const [a, abs] of Object.entries(numbering?.abstractNums ?? {})) for (const [i, lvl] of Object.entries(abs.levels ?? {})) if (lvl && !out.has(lvl)) out.set(lvl, ['a', a, i]);
  return out;
}

/** counters over the process (`UO_F4_STATS=1` prints them at exit) */
export const STATS = { calls: 0, copies: 0, table: 0, copyTable: 0, tableUnknown: 0, kinds: {} };
if (process.env.UO_F4_STATS) process.on('exit', () => console.error('f4 stats', JSON.stringify(STATS)));

export function f4Recorder() {
  let st = null;
  /** the call being recorded: its transcript and labels */
  let call = null;

  const docOf = (full) => {
    if (st.full !== full) { st.full = full; st.refs = modelRefs(full); st.levels = levelIndex(full.formatting?.numbering); }
  };

  const encodeValue = (p) => {
    const r = new Raw();
    const faces = new Map(), styles = new Map();
    const face = (f) => {
      let l = faces.get(f);
      if (l === undefined) {
        l = faces.size; faces.set(f, l);
        r.tag('F').u32(l).u16s(f.key ?? '').u16s(f.family ?? '').bool(!!f.bold).bool(!!f.italic).bool(!!f.synthBold).bool(!!f.synthItalic).bool(!!f.standIn);
      }
      return l;
    };
    const style = (s) => {
      let l = styles.get(s);
      if (l === undefined) {
        l = styles.size; styles.set(s, l);
        r.tag('S').u32(l).num(s.sizePt).u16s(s.color).bool(!!s.autoColor).bool(!!s.bold).bool(!!s.italic);
        r.bool(T(s.underline)); if (T(s.underline)) r.u16s(s.underline.val).optU16s(s.underline.color);
        r.bool(!!s.strike).bool(!!s.dstrike).optU16s(s.highlight).optU16s(s.shading).num(s.shift).bool(!!s.hidden).optU16s(s.link)
          .optNum(s.alpha).bool(!!s.outline);
        r.bool(T(s.fx)); if (T(s.fx)) textfx(r, s.fx);
        r.optU16s(s.fxKey).u16s(digest(s.rpr));
      }
      return l;
    };
    const refs = (g) => {
      face(g.face); style(g.style);
      if (g.math) for (const it of g.math.items) { face(it.face); style(it.style); }
    };
    const glyph = (g) => {
      r.u16s(g.kind).u16s(g.ch ?? '').num(g.cp).u32(faces.get(g.face)).u32(styles.get(g.style)).num(g.adv).num(g.asc).num(g.desc)
        .optNum(g.nAsc).optNum(g.nDesc).num(g.offset).num(g.runIdx)
        .bool(!!g.breakAfter).bool(!!g.glue).bool(!!g.clear).bool(!!g.fixed).bool(!!g.hang).bool(!!g.hyph).num(g.sizePt)
        .optNum(g.hr).optNum(g.feShrink).optNum(g.kernAdj).optNum(g.ligStart).optNum(g.ligEnd).optNum(g.subGid).optNum(g.noteId);
      if (!T(g.obj)) r.tag('-');
      else {
        const ref = st.refs.runs.get(g.obj);
        if (ref !== undefined) r.tag('r').u16s(ref);
        else if (g.obj.image && g.runIdx === -1) r.tag('b').num(g.obj.image.widthTwips).num(g.obj.image.heightTwips);
        else r.tag('?');
      }
      r.bool(T(g.ptab)); if (T(g.ptab)) r.u16s(g.ptab.alignment).u16s(g.ptab.relativeTo).optU16s(g.ptab.leader);
      r.bool(T(g.box)); if (T(g.box)) r.num(g.box.side).bool(!!g.box.checked);
      r.bool(T(g.math));
      if (T(g.math)) {
        const m = g.math;
        r.num(m.w).num(m.top).num(m.bottom).num(m.lineTop).num(m.lineBottom).u32(m.items.length);
        for (const it of m.items) r.num(it.cp).num(it.x).num(it.y).u32(faces.get(it.face)).num(it.sizePt).u32(styles.get(it.style));
        r.u32(m.rules.length);
        for (const x of m.rules) r.num(x.x).num(x.y).num(x.w).num(x.h).u16s(x.color);
      }
    };
    // definitions first, in a fixed order: the mark's style and face, the marker's glyphs, the glyphs
    style(p.markStyle); face(p.markFace);
    for (const g of p.marker?.glyphs ?? []) refs(g);
    for (const g of p.glyphs) refs(g);
    r.tag('P').optU16s(p.styleId).u16s(digest(p.ppr)).u32(styles.get(p.markStyle)).u32(faces.get(p.markFace))
      .num(p.indLeft).num(p.indRight).num(p.firstLine).u16s(p.jc).u32(p.tabs.length);
    for (const t of p.tabs) r.u16s(t.val).num(t.pos).optU16s(t.leader);
    r.num(p.spaceBefore).num(p.spaceAfter).bool(!!p.beforeAuto).bool(!!p.afterAuto).u16s(p.lineRule).num(p.line)
      .bool(!!p.contextual).bool(!!p.keepNext).bool(!!p.keepLines).bool(!!p.widowControl).bool(!!p.pageBreakBefore).bool(!!p.empty);
    r.bool(T(p.revBars));
    if (T(p.revBars)) { r.u32(p.revBars.ranges.length); for (const [a, b, m] of p.revBars.ranges) r.num(a).num(b).bool(!!m); r.bool(!!p.revBars.whole).bool(!!p.revBars.mark); }
    const mk = p.marker;
    r.bool(T(mk));
    if (T(mk)) {
      const lv = st.levels.get(mk.level);
      r.u16s(mk.text).num(mk.width).u16s(lv ? lv.join('|') : `s|${mk.level.ilvl}|${mk.level.numFmt}`).u16s(mk.suff).u16s(mk.jc).bool(T(mk.picBullet)).optU16s(mk.chapter).u32(mk.glyphs.length);
      for (const g of mk.glyphs) glyph(g);
    }
    r.u32(p.glyphs.length);
    for (const g of p.glyphs) glyph(g);
    return r.finish();
  };

  /** leaf objects of the table styles of every resolver of the run (a table may be laid out by
   *  another pass context than the paragraph's, so the leaves are registered when any resolver
   *  answers `tableStyle`): obj -> [key, cond ('' = the style itself)] */
  const register = (resolver) => {
    for (const [key, r] of resolver?.tableCache ?? []) {
      if (st.seenStyles.has(r)) continue;
      st.seenStyles.add(r);
      for (const k of ['pPr', 'rPr']) if (r[k] && !st.leaves.has(r[k])) st.leaves.set(r[k], [key, '']);
      for (const [c, part] of Object.entries(r.cond ?? {})) for (const k of ['pPr', 'rPr']) if (part?.[k] && !st.leaves.has(part[k])) st.leaves.set(part[k], [key, c]);
    }
  };
  const tableLeaves = (resolver) => { register(resolver); return st.leaves; };
  /** the leaves `x` was merged from, in order (false when it is not a merge of leaves) */
  const recipe = (x, leaves, k, list) => {
    const l = leaves.get(x);
    if (l) { list.push(l); return true; }
    const m = st.recipes.get(x);
    if (!m) return false;
    if (!recipe(m[0], leaves, k, list)) return false;
    return m[1] === undefined || m[1] === null ? true : recipe(m[1], leaves, k, list);
  };

  return {
    start() {
      st = { d: new Raw(), prep: null, n: 0, full: null, refs: null, levels: null, opts: false, recipes: new WeakMap(), leaves: new WeakMap(), seenStyles: new WeakSet() };
    },
    /** after the other recorders patched it: every resolver's `tableStyle` answer registers its leaves */
    hook({ StyleResolver }) {
      const prev = StyleResolver.prototype.tableStyle;
      StyleResolver.prototype.tableStyle = function (id) { const r = prev.call(this, id); if (st) register(this); return r; };
      return () => { StyleResolver.prototype.tableStyle = prev; };
    },
    /** a merge (`mergePPr`/`mergeRPr`): how its result was built */
    merged(args, r) { if (st && r && typeof r === 'object') st.recipes.set(r, [args[0], args[1]]); },
    bind(prep) { st.prep = prep; },
    /** layoutDocument(full, provider, opts): the document of the calls that follow */
    layout(full, opts) {
      if (!st) return;
      docOf(full);
      if (!st.opts) { st.opts = true; st.d.tag('O').u16s(opts?.revisions?.mode ?? '').u16s(opts?.lang ?? 'pt-BR'); }
    },
    pre(name, args) {
      if (!st || name !== 'prepareParagraph') return undefined;
      const [ctx, para, env] = args;
      const c = { tr: new Raw(), restore: [] };
      // the field callback, the numbering and the note marks answer through the transcript
      if (env && typeof env.field === 'function') {
        const f = env.field;
        args[2] = { ...env, field: (name, instr, at) => { const a = f(name, instr, at); c.tr.tag('f').u16s(name).u16s(instr ?? '').num(at).optU16s(a); return a; } };
      }
      const ns = ctx.numbering;
      if (ns) {
        const had = Object.prototype.hasOwnProperty.call(ns, 'next');
        const prev = ns.next;
        ns.next = function (numId, ilvl) {
          const m = prev.call(this, numId, ilvl);
          c.tr.tag('m').num(numId).num(ilvl).bool(T(m));
          if (T(m)) {
            const lv = st.levels.get(m.level) ?? ['?', '', ''];
            c.tr.u16s(m.text).u16s(lv[0]).u16s(lv[1]).u16s(lv[2]).num(m.numId).num(m.ilvl).num(m.value).u16s(m.listKey).optU16s(m.chapter).bool(T(m.parts));
            if (T(m.parts)) { c.tr.u32(m.parts.length); for (const x of m.parts) c.tr.u16s(x); }
          }
          return m;
        };
        c.restore.push(() => { if (had) ns.next = prev; else delete ns.next; });
      }
      const hadNm = Object.prototype.hasOwnProperty.call(ctx, 'noteMark');
      const prevNm = ctx.noteMark;
      ctx.noteMark = function (kind, id, custom) {
        const m = prevNm.call(this, kind, id, custom);
        c.tr.tag('n').u16s(kind).num(id).bool(!!custom).u16s(m);
        return m;
      };
      c.restore.push(() => { if (hadNm) ctx.noteMark = prevNm; else delete ctx.noteMark; });
      const ref = st.refs?.paras.get(para);
      STATS.calls++; if (ref === undefined) { STATS.copies++; if (process.env.UO_F4_STATS) { const allModel = para.runs.every((r) => st.refs.runs.has(r)); const k = (allModel ? "runs-model" : "runs-other") + "|" + (para.runs.length ? "" : "empty") + "|" + Object.keys(para).join(","); STATS.kinds[k] = (STATS.kinds[k] ?? 0) + 1; } } if (env?.tablePPr || env?.tableRPr) STATS.table++; if (ref === undefined && (env?.tablePPr || env?.tableRPr)) STATS.copyTable++;
      const e = env ?? {};
      c.head = new Raw().tag('Q').u32(st.n++).u16s(ref ?? '').bool(ref !== undefined);
      // the table-style env: (style key, conditions) of each of tablePPr and tableRPr
      const leaves = (T(e.tablePPr) || T(e.tableRPr)) ? tableLeaves(ctx.resolver) : null;
      for (const [x, k] of [[e.tablePPr, 'pPr'], [e.tableRPr, 'rPr']]) {
        c.head.bool(T(x));
        if (!T(x)) continue;
        const list = [];
        // the nested-table measure passes a literal `{ pPr: {}, rPr: {} }` (table.ts:537)
        if (!st.recipes.has(x) && Object.keys(x).length === 0 && Object.getPrototypeOf(x) === Object.prototype && !leaves.has(x)) { c.head.u16s('{}'); continue; }
        if (!recipe(x, leaves, k, list) || !list.length || list[0][1] !== '' || list.some((l) => l[0] !== list[0][0])) { c.head.u16s('?'); STATS.tableUnknown++; continue; }
        c.head.u16s(list[0][0]).u32(list.length - 1);
        for (const l of list.slice(1)) c.head.u16s(l[1]);
      }
      c.head.bool(typeof e.field === 'function').bool(e.numbering !== undefined);
      if (e.numbering !== undefined) c.head.bool(!!e.numbering);
      c.head.bool(!!e.darkBackground).bool(!!e.noPageBreaks).bool(!!e.hf).bool(!!e.continuation).optU16s(e.defaultColor);
      call = c;
      return c;
    },
    report(name, args, r, pre) {
      if (name === 'mergePPr' || name === 'mergeRPr') { this.merged(args, r); return; }
      if (!st || name !== 'prepareParagraph' || !pre) return;
      for (const f of pre.restore) f();
      call = null;
      const raw = encodeValue(r);
      st.prep.bytes(raw);
      st.d.parts.push(pre.head.finish(), pre.tr.finish());
      st.d.tag('E').bytes(raw);
    },
    finish() {
      const dump = st.d.tag('Z').finish();
      st = null;
      void call;
      return dump;
    },
  };
}
