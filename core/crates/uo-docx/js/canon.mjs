// The TS side of the canonical model digest of uo-docx (architecture A6; Rust side:
// src/canon.rs and the `JsValue::canon` of every model type). A generic walker over the TS
// objects of docx-core (`readFullDocument` and its parts) that writes the same items as Rust:
//
//   undefined (own key or array element) → tag U · null → tag z · boolean → tag b, bool
//   number → tag n, f64 (NaN canonical, -0 kept) · string → tag s, bytes (UTF-16LE units)
//   Uint8Array → tag B, digestOf · array → tag [, u32 length, items
//   plain object → tag {, u32 key count, per key in UTF-16 order: bytes (UTF-16LE key), value
//   Map → tag M, u32 size, key, value per entry · Set → tag S, u32 size, values
//   relabelled id (process-global counters) → tag i, u32 (first appearance = 0, per space)
//   source text (SourceStore raw slices) → tag h, digestOf (UTF-8)
//
// Aliased views are written once from the block view with a synthetic key `@views`
// ("synced" when the other view is the same objects; "phantom" for a textbox whose `paras` is a
// fresh empty paragraph outside empty blocks; "split" otherwise, in which case both views are
// written): the document's `footnotes`/`endnotes` (views of `footnoteBlocks`/`endnoteBlocks`),
// a section's `headers`/`footers` (views of `headerBlocks`/`footerBlocks`), a cell's
// `paragraphs`/`tables` (views of `blocks`), a textbox's `paras` (view of `blocks`).
//
// The key-order facet (non-compared, G23/LQ20) is a second stream: per object in walk order,
// tag o, u32 count, then `Object.keys` (minus the alias keys written through `@views`).
import { Buffer } from 'node:buffer';

const utf16le = (s) => Buffer.from(s, 'utf16le');
const cmp16 = (a, b) => (a < b ? -1 : a > b ? 1 : 0);

/** The paragraphs of a block list with those of the cells (the TS `walk` of textboxes/headers). */
function walkParas(blocks, out = []) {
  for (const b of blocks ?? []) {
    if (b.type === 'paragraph') out.push(b.paragraph);
    else for (const r of b.rows) for (const c of r.cells) walkParas(c.blocks ?? c.paragraphs.map((paragraph) => ({ type: 'paragraph', paragraph })), out);
  }
  return out;
}

const sameObjects = (a, b) => Array.isArray(a) && Array.isArray(b) && a.length === b.length && a.every((x, i) => x === b[i]);
const isFreshEmptyPara = (p) => p && Array.isArray(p.runs) && p.runs.length === 0 && Object.keys(p).length === 2 && Object.keys(p.props ?? {}).length === 0;

export class Canon {
  /** `stream`: a testkit Stream; `order`: a second Stream for the key-order facet (optional);
   *  `parts`: the XML texts of the parts read with sources (for `at` positions). */
  constructor(stream, { order, parts = [] } = {}) {
    this.s = stream;
    this.o = order;
    this.parts = parts;
    this.relabel = new Map();
  }

  id(space, raw) {
    let m = this.relabel.get(space);
    if (!m) this.relabel.set(space, (m = new Map()));
    let v = m.get(raw);
    if (v === undefined) m.set(raw, (v = m.size));
    this.s.tag('i').u32(v);
  }

  num(x) {
    this.s.tag('n').f64(Number.isNaN(x) ? NaN : x);
  }

  str(x) {
    this.s.tag('s').bytes(utf16le(x));
  }

  orderFacet(keys) {
    if (!this.o) return;
    this.o.tag('o').u32(keys.length);
    for (const k of keys) this.o.bytes(utf16le(k));
  }

  /** A value in context: `key` is the key (or array name) it was found under. */
  value(v, key) {
    if (v === undefined) return void this.s.tag('U');
    if (v === null) return void this.s.tag('z');
    switch (typeof v) {
      case 'boolean': return void this.s.tag('b').bool(v);
      case 'number': return this.num(v);
      case 'string': return this.str(v);
      default: break;
    }
    if (v instanceof Uint8Array) return void this.s.tag('B').digestOf(v);
    if (Array.isArray(v)) {
      this.s.tag('[').u32(v.length);
      for (const x of v) this.value(x, `${key}[]`);
      return;
    }
    if (v instanceof Map) {
      this.s.tag('M').u32(v.size);
      for (const [k, x] of v) { this.value(k, `${key}{k}`); this.value(x, `${key}{}`); }
      return;
    }
    if (v instanceof Set) {
      this.s.tag('S').u32(v.size);
      for (const x of v) this.value(x, `${key}{}`);
      return;
    }
    this.object(v, key);
  }

  /** A plain object, with the alias and relabel rules of its context. */
  object(o, key, extra = {}, skip = new Set()) {
    const own = Object.keys(o).filter((k) => !skip.has(k));
    let entries = own.map((k) => [k, o[k]]);
    const rules = Object.hasOwn(RULES, key) ? RULES[key] : undefined;
    if (rules) ({ entries, extra } = rules(this, o, entries, extra));
    const keys = [...entries.map(([k]) => k), ...Object.keys(extra)];
    this.orderFacet(keys);
    const all = [...entries.map(([k, v]) => [k, () => this.field(o, k, v, key)]), ...Object.entries(extra)];
    all.sort((a, b) => cmp16(a[0], b[0]));
    this.s.tag('{').u32(all.length);
    for (const [k, w] of all) { this.s.bytes(utf16le(k)); w(); }
  }

  /** One field of an object: relabelled ids and source positions by context. */
  field(o, k, v, ctx) {
    if (ctx === 'blockSdt' && k === 'key' && typeof v === 'string') return this.id('sdt', v);
    if (k === 'sdtKey' && typeof v === 'string') return this.id('sdt', v);
    if ((ctx === 'pre[]' || ctx === 'post[]') && (k === 'open' || k === 'close') && typeof v === 'number') return this.id('wrap', v);
    if (SOURCE_CTX.has(ctx) && k === 'at' && typeof v === 'number') return this.num(this.utf8At(o.raw, v));
    if (SOURCE_CTX.has(ctx) && TEXT_KEYS.has(k) && typeof v === 'string') return void this.s.tag('h').digestOf(Buffer.from(v, 'utf8'));
    if ((ctx === 'segs[]' || ctx === 'groups[]' || ctx === 'inner[]' || ctx === 'items[]' || ctx === 'pre[]' || ctx === 'post[]' || ctx === 'story') && TEXT_KEYS.has(k) && typeof v === 'string') {
      return void this.s.tag('h').digestOf(Buffer.from(v, 'utf8'));
    }
    return this.value(v, k);
  }

  /** UTF-16 offset `at` of `raw` → UTF-8 offset in the part that has `raw` there (-1 if none). */
  utf8At(raw, at) {
    for (const p of this.parts) {
      if (typeof raw === 'string' && p.startsWith(raw, at)) return Buffer.byteLength(p.slice(0, at), 'utf8');
    }
    return -1;
  }

  /** The `FullDocument` of `readFullDocument`, without `sources` (its own facet). */
  document(full) {
    this.object(full, '@doc');
  }

  /** The `sources` facet: the SourceStore (or undefined). */
  sources(store) {
    if (!store) return void this.s.tag('U');
    const e = [
      ['next', store.next], ['paras', store.paras], ['runs', store.runs], ['tables', store.tables], ['trs', store.trs], ['tcs', store.tcs],
      ['stories', store.stories], ['txbxGroups', store.txbxGroups], ['noteParts', store.noteParts], ['commentIds', store.commentIds],
    ];
    if (store.commentsSig !== undefined) e.push(['commentsSig', store.commentsSig]);
    this.orderFacet(e.map(([k]) => k));
    e.sort((a, b) => cmp16(a[0], b[0]));
    this.s.tag('{').u32(e.length);
    for (const [k, v] of e) {
      this.s.bytes(utf16le(k));
      if (v instanceof Map) {
        this.s.tag('M').u32(v.size);
        for (const [mk, mv] of v) { this.value(mk, `${k}{k}`); this.sourceValue(mv, k); }
      } else if (k === 'txbxGroups') {
        this.s.tag('[').u32(v.length);
        for (const g of v) { this.s.tag('[').u32(g.length); for (const x of g) this.object(x, 'groups[]'); }
      } else this.value(v, k);
    }
  }

  sourceValue(v, mapName) {
    const ctx = { paras: '@para', runs: '@run', tables: '@table', trs: '@row', tcs: '@cell', stories: 'story', noteParts: '@notes' }[mapName];
    this.object(v, ctx);
  }
}

const TEXT_KEYS = new Set(['raw', 'open', 'close', 'pPr', 'rPr', 'obj', 'tblPr', 'tblGrid', 'trPr', 'tblPrEx', 'tcPr', 'wrapOpen', 'wrapClose', 'sectPr']);
const SOURCE_CTX = new Set(['@para', '@run', '@table', '@row', '@cell', '@notes']);

/** Context rules: (canon, object, own entries, extra) → { entries, extra }. */
const RULES = {
  '@doc': (c, full, entries, extra) => {
    const views = ['footnote', 'endnote'].every((k) => {
      const paras = full[`${k}s`], blocks = full[`${k}Blocks`];
      if (!paras || !blocks) return paras === blocks;
      if (paras.size !== blocks.size) return false;
      for (const [id, ps] of paras) {
        const bs = blocks.get(id);
        if (!bs || !sameObjects(ps, bs.filter((b) => b.type === 'paragraph').map((b) => b.paragraph))) return false;
      }
      return true;
    });
    const drop = new Set(['sources', ...(views ? ['footnotes', 'endnotes'] : [])]);
    return { entries: entries.filter(([k]) => !drop.has(k)), extra: { ...extra, '@views': () => c.str(views ? 'synced' : 'split') } };
  },
  'sections[]': (c, s, entries, extra) => {
    const same = (set, blocks) => {
      const a = Object.keys(set ?? {}), b = Object.keys(blocks ?? {});
      return a.length === b.length && a.every((k) => sameObjects(set[k], walkParas(blocks[k])));
    };
    const views = same(s.headers, s.headerBlocks) && same(s.footers, s.footerBlocks);
    const drop = new Set(views ? ['headers', 'footers'] : []);
    return { entries: entries.filter(([k]) => !drop.has(k)), extra: { ...extra, '@views': () => c.str(views ? 'synced' : 'split') } };
  },
  'cells[]': (c, cell, entries, extra) => {
    if (!cell.blocks) return { entries, extra };
    const views = sameObjects(cell.paragraphs, cell.blocks.filter((b) => b.type === 'paragraph').map((b) => b.paragraph))
      && sameObjects(cell.tables, cell.blocks.filter((b) => b.type === 'table'));
    const drop = new Set(views ? ['paragraphs', 'tables'] : []);
    return { entries: entries.filter(([k]) => !drop.has(k)), extra: { ...extra, '@views': () => c.str(views ? 'synced' : 'split') } };
  },
  textbox: (c, tb, entries, extra) => {
    if (!tb.blocks) return { entries, extra };
    const walked = walkParas(tb.blocks);
    const views = sameObjects(tb.paras, walked) ? 'synced' : walked.length === 0 && tb.paras?.length === 1 && isFreshEmptyPara(tb.paras[0]) ? 'phantom' : 'split';
    const drop = new Set(views === 'split' ? [] : ['paras']);
    return { entries: entries.filter(([k]) => !drop.has(k)), extra: { ...extra, '@views': () => c.str(views) } };
  },
};
