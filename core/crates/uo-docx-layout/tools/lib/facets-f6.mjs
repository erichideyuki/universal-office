// P3 facets of frontier F6 (tables, `table.ts`; record "Stage 3 — F6"), recorded by facets.mjs
// during every layout of capture-layout.mjs, with their inputs dumped for the Rust replay
// (tests/f6_parity.rs), which reads the same docx with the uo-docx reader and rebuilds the streams:
//
//   p3.table        every `layoutTable(ctx, table, frameX, frameWidth, layoutCell, ref)` call, in
//                   completion order (a nested table completes before its outer one): one `bytes`
//                   item per call holding the value below (the whole TableLayout);
//   p3.rowsplit     every `row.split(avail)` the engine makes (the outermost call; a split made
//                   inside another split or a `splitContent` is part of that one): its two parts;
//   p3.splitcontent every `splitContent(c, avail)` the engine makes (outermost): its two parts.
//
// The dump `<key>.f6.bin` (a raw Sink stream) holds, in completion order:
//   'T' u32(call) u16s(table ref) num(frameX frameWidth) u16s(ref) transcript* 'E' bytes(value)
//   'S' u16s(row id) num(avail) 'E' bytes(value)
//   'C' bytes(content) num(avail) 'E' bytes(value)
//   'Z'
// transcript — what `layoutTable` asked outside the model, in order:
//   'c' u32(row) u32(cell) num(x width) env u16s(ref) bool(pure) bytes(content)      layoutCell
//   'p' u16s(para ref) penv bool(pure) num(indLeft indRight firstLine) u32 n (u16s(val) num(pos))*
//       bool(marker?)[num(width) u16s(suff)] num(markStyle.sizePt) u32 n (u16s(kind) num(adv) bool(breakAfter))*
//                                                       prepareParagraph made by the table code
//   'v' u16s(para ref) bool(markHidden visible)          paraVisibility made by isHiddenRow
//   env  := optU16s(digest tablePPr) optU16s(digest tableRPr) bool(darkBackground)
//           bool(cellMar?)[num(l r) optNum(t)]
//   penv := bool(tablePPr?)[u16s('{}' | digest)] bool(tableRPr?)[u16s('{}' | digest)]
// Row ids: `<call>.<row>` for the rows `layoutTable` returns, `s<n>b` for the rest of the n-th
// recorded split (the split closures are wrapped when they leave `layoutTable`/a split; an
// engine copy `{ ...row }` keeps the wrapper and so the id).
//
// Values: `bytes(structure) bytes(P1)`. P1 is one canon.mjs stream whose pages are the item and
// line lists of the value in the order the structure names them (`list`/`lines` = u32 page
// index), so faces, images and lineBox lines are labelled by first appearance across the value;
// faces and images are first made canonical per descriptor (two TS objects with the same
// descriptor are one). The structure:
//   table   := num(x width) optNum(borderLeft effR) optU16s(digest tblpPr) u32 n row*
//   row     := num(height) bool(cantSplit header keepNext) optNum(natural breakW) bool(split?)
//              optNum(topCut cut) list(items) lines(lines) list(borderItems) bool[list(breakBorderItems)]
//              bool[u32 n (num(col at endRow))*]                                  mergeAt
//              bool[u32 n (num(col) idx(items) idx(lines) num(dy) optNum(start) bool[content]
//                   optNum(marT band) optU16s(vAlign))*]                          mergeText
//              bool[idx(borderItems)] bool[list(ownTopBorders)]                   topBorders
//              bool[u32 n (i32 index in items, num(end))*]                         mergeFills
//              bool[u32 n (u16s(ref) num(x w span) optNum(dy h))*]                 cells
//   content := num(height) bool(keepNext) optNum(alignH) bool(shape) list(items) lines(lines)
//              bool[u32 n (u16s(ref) num(lines) bool(widow) num(after) optNum(band))*]   paras
//              bool[u32 n (num(top) row [u16s(split id | '')] idx(items) u32 m (i32 line index, u16s(ref))*)*]  nest
//              (the split id only in the inputs: the `layoutCell` contents and the `splitContent` input)
//   parts   := bool(null?) | 'P' (row row | content content)
//   list    := u32(page) u32 n (u32 index num(z seq) bool(behind))*   (the LATE_ITEMS marks)
//   lines   := u32(page)
//   idx     := u32 n (i32 index of the object in the list, -1 = not in it)*
import { createHash } from 'node:crypto';
import { Stream } from '../../../uo-testkit/js/testkit.mjs';
import { Canon } from '../../../uo-docx/js/canon.mjs';
import { canonLayout } from './canon.mjs';
import { Raw } from './facets-f5.mjs';
import { Sink } from './sink.mjs';

/** Module functions F6 wraps (merged into facets.mjs WRAPS). */
export const F6_WRAPS = { 'table.js': ['layoutTable', 'splitContent'], 'para.js': ['paraVisibility'] };

const T = (v) => v !== undefined && v !== null;

const digest = (v) => {
  const s = new Stream();
  new Canon(s).value(v, 'x');
  return `${s.items}:${s.finish().short}`;
};

/** Table and paragraph objects -> model positions (the walk of facets-f4.mjs `modelRefs`). */
function positions(full) {
  // rows: Row -> { table, ri } (a row-sliced copy `{ ...t, rows: t.rows.slice(k) }` of a linked
  // textbox chain, engine.ts:3336, is named `<table ref>+r<k>`: F7d)
  const tables = new Map(), paras = new Map(), rows = new Map();
  const blocksOf = (c) => c.blocks ?? c.paragraphs?.map((paragraph) => ({ type: 'paragraph', paragraph })) ?? [];
  const runList = (rs, base) => {
    rs.forEach((r, i) => {
      const ref = `${base}#${i}`;
      if (r.textbox) walk(r.textbox.blocks ?? r.textbox.paras.map((paragraph) => ({ type: 'paragraph', paragraph })), `${ref}/tx`);
      (r.group?.items ?? []).forEach((it, j) => runList([it.run], `${ref}/g${j}`));
    });
  };
  const walk = (bs, base) => {
    bs.forEach((b, i) => {
      const p = `${base}/b${i}`;
      if (b.type === 'paragraph') {
        if (!paras.has(b.paragraph)) paras.set(b.paragraph, p);
        runList(b.paragraph.runs, p);
      } else {
        if (!tables.has(b)) tables.set(b, p);
        b.rows.forEach((row, ri) => { if (!rows.has(row)) rows.set(row, { table: b, ri }); });
        b.rows.forEach((row, ri) => row.cells.forEach((c, ci) => walk(blocksOf(c), `${p}/r${ri}/c${ci}`)));
      }
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
  strip(tables); strip(paras);
  return { tables, paras, rows };
}

/** counters over the process (`UO_F6_STATS=1` prints them at exit) */
export const STATS = { tables: 0, cells: 0, prepares: 0, splits: 0, contents: 0, unknownTables: 0, unknownSplitIds: 0 };
if (process.env.UO_F6_STATS) process.on('exit', () => console.error('f6 stats', JSON.stringify(STATS)));

export function f6Recorder({ LATE_ITEMS }) {
  let st = null;

  // ── canonical faces/images (per descriptor) ──
  const faceDig = new WeakMap(), imgDig = new WeakMap();
  const sha = (u8) => createHash('sha256').update(u8).digest('hex');
  const faceKey = (f) => {
    if (!f) return 'null';
    let d = faceDig.get(f);
    if (d === undefined) {
      d = [f.key ?? '', f.family ?? '', !!f.bold, !!f.italic, !!f.synthBold, !!f.synthItalic, !!f.standIn, T(f.bytes) ? sha(f.bytes) : '-'].join('|');
      faceDig.set(f, d);
    }
    return d;
  };
  const imgKey = (im) => {
    if (!im) return 'null';
    let d = imgDig.get(im);
    if (d === undefined) {
      const e = im.effects;
      d = [im.ext ?? '', sha(im.data ?? new Uint8Array(0)), T(im.svg) ? sha(im.svg) : '-', im.widthTwips, im.heightTwips,
        T(e) ? [!!e.gray, e.gain, e.bright, !!e.floor, e.biLevel].join(',') : '-'].join('|');
      imgDig.set(im, d);
    }
    return d;
  };
  const canonOf = (map, key, o) => { const k = key(o); let c = map.get(k); if (c === undefined) { c = o; map.set(k, c); } return c; };

  /** Encoder of one value: the structure and the pages of its lists. */
  class Enc {
    /** `ids`: an input of the replay (the nested rows' split ids are written) */
    constructor(ids = false) { this.s = new Raw(); this.pages = []; this.faces = new Map(); this.images = new Map(); this.ids = ids; }
    item(it) {
      const face = (f) => canonOf(this.faces, faceKey, f);
      const img = (i) => canonOf(this.images, imgKey, i);
      if (it.t === 'glyphs') return { ...it, face: face(it.face) };
      if (it.t === 'image') return { ...it, image: img(it.image) };
      if (it.t === 'shape' && it.blip) return { ...it, blip: { ...it.blip, image: img(it.blip.image) } };
      if (it.t === 'clip' && it.src) return { ...it, src: img(it.src) };
      return it;
    }
    list(arr) {
      this.s.u32(this.pages.length);
      this.pages.push({ items: arr.map((it) => this.item(it)), lines: [] });
      const marked = [];
      arr.forEach((it, i) => { const m = LATE_ITEMS.get(it); if (m) marked.push([i, m]); });
      this.s.u32(marked.length);
      for (const [i, m] of marked) this.s.u32(i).num(m.z).num(m.seq).bool(!!m.behind);
    }
    lines(arr) { this.s.u32(this.pages.length); this.pages.push({ items: [], lines: arr }); }
    idx(objs, arr) { this.s.u32(objs.length); for (const o of objs) this.s.u32(arr.indexOf(o)); }
    finish() {
      const sink = new Sink({ raw: true });
      const pages = this.pages.map((p) => ({ widthTw: 0, heightTw: 0, section: 0, number: 0, items: p.items, lines: p.lines }));
      canonLayout({ pages, warnings: [] }, null, sink);
      return new Raw().bytes(this.s.finish()).bytes(sink.finish().bytes).finish();
    }
  }

  const splitIds = new WeakMap();
  const row = (e, r) => {
    const s = e.s;
    s.num(r.height).bool(!!r.cantSplit).bool(!!r.header).bool(!!r.keepNext).optNum(r.natural).optNum(r.breakW).bool(typeof r.split === 'function')
      .optNum(r.topCut).optNum(r.cut);
    e.list(r.items); e.lines(r.lines); e.list(r.borderItems);
    s.bool(T(r.breakBorderItems)); if (T(r.breakBorderItems)) e.list(r.breakBorderItems);
    s.bool(T(r.mergeAt)); if (T(r.mergeAt)) { s.u32(r.mergeAt.length); for (const m of r.mergeAt) s.num(m.col).num(m.at).num(m.endRow); }
    s.bool(T(r.mergeText));
    if (T(r.mergeText)) {
      s.u32(r.mergeText.length);
      for (const m of r.mergeText) {
        s.num(m.col); e.idx(m.items, r.items); e.idx(m.lines, r.lines); s.num(m.dy).optNum(m.start);
        s.bool(T(m.content)); if (T(m.content)) content(e, m.content);
        s.optNum(m.marT).optNum(m.band).optU16s(m.vAlign);
      }
    }
    s.bool(T(r.topBorders)); if (T(r.topBorders)) e.idx(r.topBorders, r.borderItems);
    s.bool(T(r.ownTopBorders)); if (T(r.ownTopBorders)) e.list(r.ownTopBorders);
    s.bool(T(r.mergeFills)); if (T(r.mergeFills)) { s.u32(r.mergeFills.length); for (const f of r.mergeFills) s.u32(r.items.indexOf(f.item)).num(f.end); }
    s.bool(T(r.cells)); if (T(r.cells)) { s.u32(r.cells.length); for (const c of r.cells) s.u16s(c.ref).num(c.x).num(c.w).num(c.span).optNum(c.dy).optNum(c.h); }
  };
  const content = (e, c) => {
    const s = e.s;
    s.num(c.height).bool(!!c.keepNext).optNum(c.alignH).bool(!!c.shape);
    e.list(c.items); e.lines(c.lines);
    s.bool(T(c.paras)); if (T(c.paras)) { s.u32(c.paras.size); for (const [k, v] of c.paras) s.u16s(k).num(v.lines).bool(!!v.widow).num(v.after).optNum(v.band); }
    s.bool(T(c.nest));
    if (T(c.nest)) {
      s.u32(c.nest.length);
      for (const n of c.nest) {
        s.num(n.top); row(e, n.row);
        const sid = typeof n.row.split === 'function' ? splitIds.get(n.row.split) : undefined;
        if (typeof n.row.split === 'function' && sid === undefined) STATS.unknownSplitIds++;
        if (e.ids) s.u16s(sid ?? '');
        e.idx(n.items, c.items);
        s.u32(n.lines.length); for (const l of n.lines) s.u32(c.lines.indexOf(l)).u16s(l.ref);
      }
    }
  };
  const table = (t) => {
    const e = new Enc(), s = e.s;
    s.num(t.x).num(t.width).optNum(t.borderLeft).optNum(t.effR).optU16s(T(t.tblpPr) ? digest(t.tblpPr) : undefined).u32(t.rows.length);
    for (const r of t.rows) row(e, r);
    return e.finish();
  };
  const contentValue = (c) => { const e = new Enc(true); content(e, c); return e.finish(); };
  const parts = (p, kind) => {
    const e = new Enc();
    e.s.bool(p === null || p === undefined);
    if (p) { e.s.tag('P'); for (const x of p) (kind === 'row' ? row : content)(e, x); }
    return e.finish();
  };

  /** wraps a row's split closure: the outermost calls are recorded, the rest gets an id */
  const wrapSplit = (fn, id) => {
    const w = (avail) => {
      if (!st || st.depth > 0) return fn(avail);
      st.depth++;
      let p;
      try { p = fn(avail); } finally { st.depth--; }
      const n = st.splits++;
      STATS.splits++;
      if (p && typeof p[1].split === 'function') p[1].split = wrapSplit(p[1].split, `s${n}b`);
      const v = parts(p, 'row');
      st.rowsplit.bytes(v);
      st.d.tag('S').u16s(id).num(avail).tag('E').bytes(v);
      return p;
    };
    splitIds.set(w, id);
    return w;
  };

  const top = () => st?.frames[st.frames.length - 1];

  return {
    start() {
      st = {
        d: new Raw(), table: new Sink(), rowsplit: new Sink(), splitcontent: new Sink(), frames: [], calls: 0, splits: 0, depth: 0,
        full: null, pos: null, sc: [],
      };
    },
    /** layoutDocument(full, …): the document of the calls that follow */
    layout(full) {
      if (!st) return;
      if (st.full !== full) { st.full = full; st.pos = positions(full); }
      st.frames = []; st.depth = 0; st.sc = [];
    },
    pre(name, args) {
      if (!st) return undefined;
      if (name === 'layoutTable') {
        const [ctx, table0, frameX, frameWidth, layoutCell, ref] = args;
        let tref = st.pos?.tables.get(table0);
        if (tref === undefined && table0.rows.length) {
          // a row-sliced copy of a table (chainSlice, F7d): same fields, the rows from k on
          const at = st.pos?.rows.get(table0.rows[0]);
          const base = at && st.pos.tables.get(at.table);
          const same = at && base !== undefined && table0.rows.length === at.table.rows.length - at.ri
            && table0.rows.every((r, i) => at.table.rows[at.ri + i] === r)
            && Object.keys(table0).every((k) => k === 'rows' || table0[k] === at.table[k]);
          if (same) tref = `${base}+r${at.ri}`;
        }
        if (tref === undefined) STATS.unknownTables++;
        const f = { call: st.calls++, tr: new Raw(), inCell: 0, pure: 0, ctx, had: Object.prototype.hasOwnProperty.call(ctx, 'pure'), prev: ctx.pure };
        f.head = new Raw().tag('T').u32(f.call).u16s(tref ?? '').num(frameX).num(frameWidth).u16s(ref);
        ctx.pure = function (fn) { f.pure++; try { return f.prev.call(this, fn); } finally { f.pure--; } };
        args[4] = (cell, x, width, env, cref) => {
          f.inCell++;
          let c;
          try { c = layoutCell(cell, x, width, env, cref); } finally { f.inCell--; }
          let ri = -1, ci = -1;
          table0.rows.forEach((r, i) => { const k = r.cells.indexOf(cell); if (k >= 0 && ri < 0) { ri = i; ci = k; } });
          const e = env ?? {};
          f.tr.tag('c').u32(ri).u32(ci).num(x).num(width)
            .optU16s(T(e.tablePPr) ? digest(e.tablePPr) : undefined).optU16s(T(e.tableRPr) ? digest(e.tableRPr) : undefined)
            .bool(!!e.darkBackground).bool(T(e.cellMar));
          if (T(e.cellMar)) f.tr.num(e.cellMar.l).num(e.cellMar.r).optNum(e.cellMar.t);
          f.tr.u16s(cref).bool(f.pure > 0).bytes(contentValue(c));
          STATS.cells++;
          return c;
        };
        st.frames.push(f);
        STATS.tables++;
        return f;
      }
      if (name === 'splitContent') {
        const rec = st.depth === 0;
        st.sc.push(rec);
        st.depth++;
        return rec ? { input: contentValue(args[0]) } : undefined;
      }
      return undefined;
    },
    report(name, args, r, pre) {
      if (!st) return;
      if (name === 'prepareParagraph' || name === 'paraVisibility') {
        const f = top();
        if (!f || f.inCell > 0) return;
        const pref = st.pos?.paras.get(args[1]) ?? '';
        if (name === 'paraVisibility') { f.tr.tag('v').u16s(pref).bool(!!r.markHidden).bool(!!r.visible); return; }
        const e = args[2] ?? {};
        const penv = (x) => { f.tr.bool(T(x)); if (T(x)) f.tr.u16s(Object.keys(x).length === 0 && Object.getPrototypeOf(x) === Object.prototype ? '{}' : digest(x)); };
        f.tr.tag('p').u16s(pref);
        penv(e.tablePPr); penv(e.tableRPr);
        f.tr.bool(f.pure > 0).num(r.indLeft).num(r.indRight).num(r.firstLine).u32(r.tabs.length);
        for (const t of r.tabs) f.tr.u16s(t.val).num(t.pos);
        f.tr.bool(T(r.marker)); if (T(r.marker)) f.tr.num(r.marker.width).u16s(r.marker.suff);
        f.tr.num(r.markStyle.sizePt).u32(r.glyphs.length);
        for (const g of r.glyphs) f.tr.u16s(g.kind).num(g.adv).bool(!!g.breakAfter);
        STATS.prepares++;
        return;
      }
      if (name === 'layoutTable' && pre) {
        const f = pre;
        if (f.had) f.ctx.pure = f.prev; else delete f.ctx.pure;
        st.frames.pop();
        // wrap the split closures of the rows leaving layoutTable
        r.rows.forEach((row, ri) => { if (typeof row.split === 'function') row.split = wrapSplit(row.split, `${f.call}.${ri}`); });
        const v = table(r);
        st.table.bytes(v);
        st.d.parts.push(f.head.finish(), f.tr.finish());
        st.d.tag('E').bytes(v);
        return;
      }
      if (name === 'splitContent') {
        st.depth--;
        const rec = st.sc.pop();
        if (!rec || !pre) return;
        const v = parts(r, 'content');
        st.splitcontent.bytes(v);
        st.d.tag('C').bytes(pre.input).num(args[1]).tag('E').bytes(v);
        STATS.contents++;
      }
    },
    finish() {
      const facets = { 'p3.table': st.table.finish().value, 'p3.rowsplit': st.rowsplit.finish().value, 'p3.splitcontent': st.splitcontent.finish().value };
      const dump = st.d.tag('Z').finish();
      st = null;
      return { facets, dump };
    },
  };
}
