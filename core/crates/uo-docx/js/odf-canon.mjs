// The canonical digest of a `readOdt` result, shared by capture-odf.mjs and odf-dump.mjs: the canonical stream
// (js/canon.mjs) of `{ blocks, sections, comments, styles }`, with each cell given its `blocks` (the Rust cell is
// one story: paragraphs, then tables) and the empty `headers`/`footers` of a section left out. The TS cell has
// `paragraphs` then `tables` and no `blocks`; `blocks` is built from them with the same objects, so the walker's
// aliased-views rule writes them once.
import { Stream } from '../../uo-testkit/js/testkit.mjs';
import { R2Canon } from './capture-r2-common.mjs';

export function normalizeOdt(full) {
  const memo = new Map();
  const para = (p) => { let q = memo.get(p); if (!q) { q = { ...p, runs: p.runs.map(run) }; memo.set(p, q); } return q; };
  const run = (r) => (r.textbox ? { ...r, textbox: tbox(r.textbox) } : r);
  const tbox = (tb) => ({ ...tb, paras: tb.paras.map(para), blocks: tb.blocks.map(block) });
  const block = (b) => (b.type === 'paragraph' ? { type: 'paragraph', paragraph: para(b.paragraph) } : table(b));
  const table = (t) => ({ ...t, rows: t.rows.map((row) => ({ ...row, cells: row.cells.map(cell) })) });
  const cell = (c) => {
    const paragraphs = c.paragraphs.map(para);
    const tables = c.tables.map(table);
    return { ...c, paragraphs, tables, blocks: [...paragraphs.map((paragraph) => ({ type: 'paragraph', paragraph })), ...tables] };
  };
  return { blocks: full.blocks.map(block), sections: full.sections.map(({ headers, footers, ...rest }) => rest), comments: full.comments, styles: full.styles };
}

/** `items:digest` of the model. */
export function digestOdt(full, trace = false) {
  const st = new Stream({ trace });
  new R2Canon(st).value(normalizeOdt(full), '@odf');
  return trace ? st.trace() : `${st.items}:${st.finish().short}`;
}
