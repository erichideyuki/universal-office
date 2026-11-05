// P4 facets of uo-docx-layout frontier F8 (record core/docs/modules/uo-docx-layout.md "Stage 3 —
// F8"): the editor- and viewer-facing queries of a layout, run by the frozen TS over the layout of
// the capture run, replayed by tests/f8_parity.rs through the Rust engine and the Rust queries.
//
// Behaviour sources, none edited:
//  - exported functions, imported from the frozen sources: editor/src/engine-doc.ts (`caretXIn`,
//    `offsetAtX`, `locateRef`, `resolveCell`, `hfBlocksFor`, `sameHops`, `cellBlocks`,
//    `paragraphAtPath`), editor/src/notes.ts (`noteBlocks`, `flattenBlocks`), viewer/src/views.ts
//    (`webDocument`), docx-layout dist (`runBoxes`, `pdfNavigation`, `paintCustomInlines`) — Node
//    24 strips the types of the .ts files;
//  - private methods of the <universal-write> element, EXTRACTED from the frozen
//    editor/src/universal-write.ts at capture time (their text, types stripped by
//    `module.stripTypeScriptTypes`) and run on a plain object standing for the element:
//    `installEngineResult`, `lineParagraph`, `rebuildEditLines`, `rebuildTextboxLines`,
//    `allLines`, `lineOf`, `caretX`, `hitLineX`, `hitTestRaw`, `paintEngineSelection`,
//    `engLineAt`, `cellBoxAt`, and the module functions `shiftEngLine`, `xfInvert`,
//    `collapseCustomChars`. The element's zoom is 15 (`PX_PER_TWIP * 15 === 1`), so client pixels
//    are page twips; one canvas per page at the origin, the others out of reach.
//  - one closure transcribed (it is local to `search()`): `pageOf` (universal-write.ts:17874).
//
// Facets (Sink streams; plain strings where marked):
//   p4.locate   every page line: locateRef kind + the paragraph it resolves to
//   p4.caret    caretXIn / offsetAtX over every page line (sampled offsets and x)
//   p4.edit     per edit scope (body, cells, header/footer parts, notes, textboxes): the edit lines,
//               hitTestRaw over sample points, lineOf+caretX (caretRectAt) per sampled offset,
//               paintEngineSelection per sampled range and page
//   p4.lineat   engLineAt over sample points;  p4.cellat  cellBoxAt over the same points
//   p4.pageof   the find list's pageOf for body and cell paragraphs
//   p4.runboxes runBoxes for links and plugin inlines;  p4.inline  paintCustomInlines' run0 + measureText
//   p4.nav      pdfNavigation;  p4.fields  onField (the main layout's values)
//   p4.hyph     two `hyphenation: 'force'` layouts (no skip; then the first two queried points
//               skipped): the body hyphenation points in page order;  p4.hyphskip (plain) the skips
//   p4.web      the P1 stream of layoutDocument(webDocument(full, 9000))
import { readFileSync, writeFileSync } from 'node:fs';
import { stripTypeScriptTypes } from 'node:module';
import { join } from 'node:path';
import { pathToFileURL } from 'node:url';
import { Sink } from './sink.mjs';
import { canonLayout } from './canon.mjs';

const PX_PER_TWIP = 96 / 72 / 20;
const ZOOM = 15; // PX_PER_TWIP * 15 === 1

/** The text of a class method (2-space indent) or a module function of universal-write.ts. */
function extract(src, name, kind) {
  const re = kind === 'method'
    ? new RegExp(`^  (?:private |public )?${name}\\(`, 'm')
    : kind === 'const' ? new RegExp(`^const ${name} = `, 'm') : new RegExp(`^function ${name}\\(`, 'm');
  const m = re.exec(src);
  if (!m) throw new Error(`facets-f8: ${name} not found in universal-write.ts`);
  if (kind === 'const') return src.slice(m.index, src.indexOf('\n', m.index));
  const end = src.indexOf(kind === 'method' ? '\n  }\n' : '\n}\n', m.index);
  return src.slice(m.index, end + (kind === 'method' ? 4 : 2));
}

export async function setupF8(root) {
  const ts = (p) => import(pathToFileURL(join(root, p)).href);
  const ed = await ts('write/packages/editor/src/engine-doc.ts');
  const notes = await ts('write/packages/editor/src/notes.ts');
  const views = await ts('write/packages/viewer/src/views.ts');
  const core = await ts('write/packages/docx-core/dist/index.js');
  const lay = await ts('write/packages/docx-layout/dist/index.js');
  const src = readFileSync(join(root, 'write/packages/editor/src/universal-write.ts'), 'utf8');
  const methods = ['allLines', 'lineOf', 'caretX', 'hitLineX', 'hitTestRaw', 'rebuildEditLines', 'rebuildTextboxLines',
    'installEngineResult', 'lineParagraph', 'paintEngineSelection', 'engLineAt', 'cellBoxAt'];
  const text = [
    extract(src, 'shiftEngLine', 'function'), extract(src, 'xfApply', 'const'), extract(src, 'xfInvert', 'function'),
    extract(src, 'collapseCustomChars', 'function'),
    `class Element {\n${methods.map((m) => extract(src, m, 'method')).join('\n')}\n}`,
  ].join('\n');
  const js = stripTypeScriptTypes(text, { mode: 'strip' });
  const deps = { caretXIn: ed.caretXIn, offsetAtX: ed.offsetAtX, locateRef: ed.locateRef, cellBlocks: ed.cellBlocks, sameHops: ed.sameHops,
    hfBlocksFor: ed.hfBlocksFor, paragraphAtPath: ed.paragraphAtPath, resolveCell: ed.resolveCell, paraLength: core.paraLength,
    noteBlocks: notes.noteBlocks, PX_PER_TWIP };
  // eslint-disable-next-line no-new-func
  const Element = new Function(...Object.keys(deps), `${js}\nreturn Element;`)(...Object.values(deps));
  return { ed, notes, views, core, lay, Element };
}

// ───────────────────────── naming (the same text the Rust replay prints) ─────────────────────────

const blocksOfCell = (c) => c.blocks ?? c.paragraphs.map((paragraph) => ({ type: 'paragraph', paragraph }));
const blocksOfParas = (ps) => ps.map((paragraph) => ({ type: 'paragraph', paragraph }));

/** paragraph object → `<story>:<path>` (cells by their index in the row); run object → `<para>#<i>`
 *  (field results `/f<j>`, recursively; group members `/g<j>`). Stories: `b` body, `hf<n>` (first
 *  appearance over sections × [header default first even, footer default first even]),
 *  `fn<id>`/`en<id>`, and a textbox's content `<run>/tx`. */
export function names(full) {
  const paras = new Map(), runs = new Map();
  const fr = (r, ref) => (r.fieldResult ?? []).forEach((q, j) => { const k = `${ref}/f${j}`; if (!runs.has(q)) runs.set(q, k); fr(q, k); });
  const walk = (bs, story, base) => bs.forEach((b, i) => {
    const p = base ? `${base}/b${i}` : `b${i}`;
    if (b.type === 'paragraph') {
      if (!paras.has(b.paragraph)) paras.set(b.paragraph, `${story}:${p}`);
      b.paragraph.runs.forEach((r, k) => {
        const ref = `${story}:${p}#${k}`;
        if (!runs.has(r)) runs.set(r, ref);
        fr(r, ref);
        // textbox contents (story `<run>/tx`) and group members (`<run>/g<j>`, their boxes `…/tx`)
        if (r.textbox) walk(r.textbox.blocks ?? blocksOfParas(r.textbox.paras), `${ref}/tx`, '');
        (r.group?.items ?? []).forEach((it, j) => {
          const g = `${ref}/g${j}`;
          if (!runs.has(it.run)) runs.set(it.run, g);
          fr(it.run, g);
          if (it.run.textbox) walk(it.run.textbox.blocks ?? blocksOfParas(it.run.textbox.paras), `${g}/tx`, '');
        });
      });
    } else b.rows.forEach((row, ri) => row.cells.forEach((c, ci) => walk(blocksOfCell(c), story, `${p}/r${ri}/c${ci}`)));
  });
  walk(full.blocks, 'b', '');
  const hf = new Map();
  for (const s of full.sections ?? []) {
    for (const [set, ps] of [[s.headerBlocks, s.headers], [s.footerBlocks, s.footers]]) {
      for (const k of ['default', 'first', 'even']) {
        const arr = set?.[k] ?? ps?.[k];
        if (!arr || hf.has(arr)) continue;
        hf.set(arr, `hf${hf.size}`);
        walk(set?.[k] ?? blocksOfParas(arr), hf.get(arr), '');
      }
    }
  }
  for (const [kind, blocks, ps] of [['fn', full.footnoteBlocks, full.footnotes], ['en', full.endnoteBlocks, full.endnotes]]) {
    for (const [id, list] of ps ?? []) walk(blocks?.get(id) ?? blocksOfParas(list), `${kind}${id}`, '');
  }
  return { paras, runs, hfParts: hf };
}

// ───────────────────────── sampling (mirrored in tests/f8_parity.rs) ─────────────────────────

/** The pages probed: all up to 6, else the first and last three. */
export function samplePages(n) {
  if (n <= 6) return [...Array(n).keys()];
  return [0, 1, 2, n - 3, n - 2, n - 1];
}

/** Points of a page (twips), inside the page: a grid and three per line (first 40 lines). */
export function samplePoints(page) {
  const w = page.widthTw, h = page.heightTw, out = [];
  for (const y of [0, h * 0.05, h * 0.2, h * 0.35, h * 0.5, h * 0.65, h * 0.8, h * 0.95, h]) {
    for (const x of [-100, w * 0.13, w * 0.37, w * 0.61, w * 0.89, w + 100]) out.push([x, y]);
  }
  for (const l of page.lines.slice(0, 40)) {
    out.push([l.x - 10, l.y + l.height / 2], [l.x + l.width * 0.31, l.y + l.height / 2], [l.x + l.width + 10, l.y - 5]);
  }
  return out.filter(([x, y]) => x >= 0 && x <= w && y >= 0 && y <= h);
}

/** Offsets probed in a paragraph of `len` with these edit lines. */
export function sampleOffsets(len, lines) {
  const s = new Set();
  for (let o = 0; o <= Math.min(len, 12); o++) s.add(o);
  s.add(len);
  for (const l of lines) { s.add(l.rich.start); s.add(l.rich.end); }
  return [...s].sort((a, b) => a - b);
}

/** Ranges probed over the scope's paragraphs (`lens[i]` = paragraph lengths). */
export function sampleRanges(lens) {
  const n = lens.length, out = [];
  if (!n) return out;
  out.push([[0, 0], [n - 1, lens[n - 1]]]);
  for (let i = 0; i + 1 < n && i < 25; i++) out.push([[i, Math.min(1, lens[i])], [i + 1, Math.min(2, lens[i + 1])]]);
  for (let i = 0; i < n && i < 25; i++) out.push([[i, 0], [i, lens[i]]]);
  return out;
}

// ───────────────────────── the facets ─────────────────────────

/** Everything but the extra layouts. `res`/`full`: the capture's main layout; `fields`: its onField map. */
export function editorFacets(F, full, res, fields, traceKey = 'doc') {
  const out = {};
  const { paras, runs, hfParts } = names(full);
  const pname = (p) => (p ? paras.get(p) ?? '?' : '-');

  // origins and the editor's body state for a document as read (buildEngineDoc's inverse)
  const origins = [], bodyParas = [], tableAnchors = [];
  for (const b of full.blocks) {
    if (b.type === 'paragraph') { origins.push({ kind: 'para', para: bodyParas.length }); bodyParas.push(b.paragraph); }
    else { origins.push({ kind: 'table', anchor: tableAnchors.length }); tableAnchors.push({ beforePara: bodyParas.length, table: b }); }
  }

  // p4.caret: caretXIn / offsetAtX over the page lines as laid out
  {
    const s = new Sink();
    let n = 0;
    for (const page of res.pages) {
      for (const l of page.lines) {
        if (n++ >= 400) break;
        for (const o of [l.start - 1, l.start, l.start + 1, Math.floor((l.start + l.end) / 2), l.end - 1, l.end, l.end + 1]) s.num(F.ed.caretXIn(l, o));
        const xs = [l.x - 50, l.x, l.x + l.width / 2, l.x + l.width + 50];
        for (const c of (l.chars ?? []).slice(0, 30)) xs.push(c.x + c.w / 2, c.x + c.w / 2 - 0.001);
        for (const x of xs) s.u32(F.ed.offsetAtX(l, x, l.end)).u32(F.ed.offsetAtX(l, x, l.end + 7));
      }
    }
    out['p4.caret'] = s.finish().value;
  }

  // p4.runboxes, p4.inline (before installEngineResult collapses the lines' characters)
  {
    const s = new Sink();
    const box = (b, v) => s.u32(b.page).num(b.x).num(b.y).num(b.w).num(b.h).num(b.baseline).u16s(runs.get(b.run) ?? '?').u16s(v);
    const links = F.lay.runBoxes(full, res, (r) => (r.custom ? undefined : r.props.link));
    s.tag('L').u32(links.length);
    for (const b of links) box(b, b.value);
    const customs = F.lay.runBoxes(full, res, (r) => r.custom);
    s.tag('C').u32(customs.length);
    for (const b of customs) box(b, b.value.type);
    out['p4.runboxes'] = s.finish().value;
    // paintCustomInlines over each page with a recording renderer
    const si = new Sink();
    const ctx = { getTransform: () => ({ a: 1, e: 0, f: 0 }), save() {}, restore() {}, fillRect() {}, setTransform() {}, set font(_v) {}, set fillStyle(_v) {} };
    const env = { fontOf: () => ({ family: 'x', weight: 400, italic: false }), color: (c) => c };
    res.pages.forEach((page, pg) => {
      const mine = customs.filter((b) => b.page === pg);
      if (!mine.length) return;
      const types = new Set(mine.map((b) => b.value.type));
      const renderers = new Map([...types].map((t) => [t, {
        type: t,
        draw: (c) => { si.num(c.sizePt).u16s(c.color).num(c.measureText(c.fallbackText)).num(c.measureText('Wq😀\uD800x')).num(c.measureText('')); },
      }]));
      F.lay.paintCustomInlines(ctx, page, mine, renderers, env);
    });
    out['p4.inline'] = si.finish().value;
  }

  // p4.nav
  {
    const s = new Sink();
    const nav = F.lay.pdfNavigation(full, res);
    s.u32(nav.links.length);
    for (const { page, link } of nav.links) {
      s.u32(page).num(link.xPt).num(link.yTopPt).num(link.wPt).num(link.hPt);
      if (link.uri !== undefined) s.tag('U').u16s(link.uri); else s.tag('D').u32(link.pageIndex).num(link.destTopPt);
    }
    s.u32(nav.outline.length);
    for (const o of nav.outline) s.u16s(o.title).num(o.level).u32(o.pageIndex).num(o.topPt);
    out['p4.nav'] = s.finish().value;
  }

  // p4.fields: the model runs reported, in first-set order (runs without a model name — slices,
  // frame copies — are not reported by the Rust layout: left out here too, counted)
  {
    const s = new Sink();
    let unnamed = 0;
    const named = [...fields].filter(([r]) => { const ok = runs.has(r); if (!ok) unnamed++; return ok; });
    s.u32(named.length);
    for (const [r, v] of named) s.u16s(runs.get(r)).u16s(v);
    out['p4.fields'] = s.finish().value;
    out['p4.fields.unnamed'] = String(unnamed);
  }

  // the element: installEngineResult over the layout (it collapses plugin-inline characters)
  const el = new F.Element();
  Object.assign(el, {
    model: { paragraphs: bodyParas }, fullDoc: full, zoom: ZOOM, layoutCache: [], tbEdit: null, cellEdit: null, hfEdit: null,
    noteEdit: null, hfActivePage: 0, canvases: [], engVersion: 0,
    bodyState: () => ({ model: { paragraphs: bodyParas }, tableAnchors }),
  });
  const doc = { full, origins, paraBlock: [], sectionMap: full.sections.map((_, i) => i) };
  el.installEngineResult(doc, res);
  const eng = el.eng;

  // p4.locate: the paragraph of every engine line
  {
    const s = new Sink();
    for (const e of eng.lines) s.u32(e.page).u16s(e.loc.kind).u16s(pname(e.para));
    out['p4.locate'] = s.finish().value;
  }

  // canvases: page `pg` at the origin, the others out of reach
  const canvasesFor = (pg) => res.pages.map((p, i) => ({
    getBoundingClientRect: () => (i === pg
      ? { left: 0, top: 0, right: p.widthTw, bottom: p.heightTw }
      : { left: 0, top: 1e15, right: p.widthTw, bottom: 1e15 + 1 }),
  }));
  // scale = PX_PER_TWIP * ZOOM === 1: client pixels are page twips (`(x - 0) / 1 === x`)
  const mouse = (x, y) => ({ clientX: x, clientY: y });

  // p4.lineat, p4.cellat
  {
    const sl = new Sink(), sc = new Sink();
    for (const pg of samplePages(res.pages.length)) {
      const page = res.pages[pg];
      el.canvases = canvasesFor(pg);
      const pts = samplePoints(page);
      sl.u32(pg).u32(pts.length);
      sc.u32(pg).u32(pts.length);
      for (const [x, y] of pts) {
        const e = el.engLineAt(mouse(x, y));
        sl.optNum(e ? page.lines.indexOf(e.line) : undefined);
        const c = el.cellBoxAt(mouse(x, y));
        sc.optNum(c ? page.cells.indexOf(c.box) : undefined);
      }
    }
    out['p4.lineat'] = sl.finish().value;
    out['p4.cellat'] = sc.finish().value;
  }

  // p4.pageof: `pageOf(p, off)` of search() (transcribed: it is a closure of that method)
  {
    const s = new Sink();
    const pageOf = (p, off) => { let best = -1; for (const l of eng.lines) if (l.para === p && l.line.start <= off) best = l.page; return best; };
    const visit = (bs) => {
      for (const b of bs) {
        if (b.type === 'paragraph') {
          const len = F.core.paraLength(b.paragraph);
          for (const off of [0, Math.floor(len / 2), len]) s.num(pageOf(b.paragraph, off));
        } else for (const row of b.rows) for (const c of row.cells) visit(blocksOfCell(c));
      }
    };
    visit(full.blocks);
    out['p4.pageof'] = s.finish().value;
  }

  // p4.edit: the scopes
  {
    const s = new Sink({ trace: !!process.env.UO_F8_TRACE });
    const runScope = (tag, setup) => {
      el.layoutCache = []; el.cellEdit = null; el.hfEdit = null; el.noteEdit = null; el.tbEdit = null; el.hfActivePage = 0;
      setup();
      el.rebuildEditLines();
      const cache = el.layoutCache;
      s.tag('S').u16s(tag).u32(cache.length).u32(el.hfActivePage);
      cache.forEach((pl) => {
        s.u32(pl.lines.length);
        for (const ln of pl.lines) {
          const l = ln.eng;
          s.u32(ln.page).u32(ln.rich.start).u32(ln.rich.end).num(l.x).num(l.y).num(l.height).num(l.baseline).num(l.width).u32(l.start).bool(!!ln.xf);
          if (ln.xf) for (const v of ln.xf) s.num(v);
        }
      });
      // hits
      for (const pg of samplePages(res.pages.length)) {
        el.canvases = canvasesFor(pg);
        const pts = samplePoints(res.pages[pg]);
        s.u32(pg).u32(pts.length);
        for (const [x, y] of pts) {
          const h = el.hitTestRaw(mouse(x, y));
          if (h) s.u32(h.para).u32(h.offset); else s.u32(0xffffffff);
        }
      }
      // carets (caretRectAt in page twips)
      cache.slice(0, 60).forEach((pl, pi) => {
        const len = F.core.paraLength(el.model.paragraphs[pi]);
        for (const off of sampleOffsets(len, pl.lines)) {
          const pos = { para: pi, offset: off };
          const line = el.lineOf(pos);
          if (!line) { s.bool(false); continue; }
          s.bool(true).u32(line.page).num(el.caretX(pos, line)).num(line.yTwips).num(line.heightTwips);
        }
      });
      // selections
      const pages = [...new Set(cache.flatMap((pl) => pl.lines.map((l) => l.page)))].sort((a, b) => a - b);
      const lens = el.model.paragraphs.map((p) => F.core.paraLength(p));
      for (const [[a, ao], [b, bo]] of sampleRanges(lens)) {
        const sel = { start: { para: a, offset: ao }, end: { para: b, offset: bo } };
        for (const pg of pages) {
          const rects = [];
          let xf = null;
          const ctx = { save() {}, restore() { xf = null; }, transform(...m) { xf = m; }, fillRect(x, y, w, h) { rects.push([x, y, w, h, xf]); }, set fillStyle(_v) {} };
          el.paintEngineSelection(ctx, pg, sel);
          s.u32(pg).u32(rects.length);
          for (const [x, y, w, h, m] of rects) { s.num(x).num(y).num(w).num(h).bool(!!m); if (m) for (const v of m) s.num(v); }
        }
      }
    };
    runScope('body', () => { el.model = { paragraphs: bodyParas }; });
    // cells: the first six, depth first (grid columns as the engine's refs)
    const cells = [];
    const visitTable = (t, anchor, hops) => t.rows.forEach((row, r) => {
      let col = row.trPr?.gridBefore ?? 0;
      row.cells.forEach((cell) => {
        const c = col;
        col += Math.max(1, cell.tcPr?.gridSpan ?? cell.gridSpan ?? 1);
        if (cells.length < 6) cells.push({ anchor, hops: [...hops, { r, c }], cell });
        blocksOfCell(cell).forEach((b, bi) => {
          if (b.type === 'table' && cells.length < 6) visitTable(b, anchor, [...hops, { r, c, b: bi }]);
        });
      });
    });
    tableAnchors.forEach((a, i) => { if (cells.length < 6) visitTable(a.table, i, []); });
    for (const c of cells) {
      runScope(`cell ${c.anchor} ${c.hops.map((h) => `${h.r}.${h.c}.${h.b ?? ''}`).join('/')}`, () => {
        el.cellEdit = { anchor: c.anchor, hops: c.hops, cell: c.cell };
        el.model = { paragraphs: c.cell.paragraphs };
      });
    }
    // header/footer parts (first four), on the first and the last page
    let nh = 0;
    const seen = new Set();
    for (const s0 of full.sections) {
      for (const [set, ps, kind] of [[s0.headerBlocks, s0.headers, 'header'], [s0.footerBlocks, s0.footers, 'footer']]) {
        for (const k of ['default', 'first', 'even']) {
          const blocks = set?.[k];
          if (!blocks || !hfParts.has(blocks) || nh >= 4) continue;
          if (seen.has(blocks)) continue;
          seen.add(blocks);
          nh++;
          for (const page of [0, res.pages.length - 1]) {
            runScope(`${hfParts.get(blocks)} ${kind} ${page}`, () => {
              el.hfEdit = { kind };
              el.hfActivePage = page;
              // the part's paragraph array IS the model (`enterHf`): the flat editing view, cells included
              el.model = { paragraphs: ps?.[k] ?? F.notes.flattenBlocks(blocks) };
            });
          }
        }
      }
    }
    // notes: the first three of each kind
    for (const [kind, map] of [['footnote', full.footnotes], ['endnote', full.endnotes]]) {
      let n = 0;
      for (const id of map?.keys() ?? []) {
        if (n++ >= 3) break;
        runScope(`${kind} ${id}`, () => {
          el.noteEdit = { kind, id };
          el.model = { paragraphs: F.notes.flattenBlocks(F.notes.noteBlocks(full, kind, id) ?? []) };
        });
      }
    }
    // model textboxes with content on the pages (first four), each alone in its chain
    const tbs = [];
    res.pages.forEach((page, pg) => {
      for (const o of page.objects ?? []) {
        const t = o.run.textbox;
        // model textboxes only (a lifted frame's box is synthetic: the editor never edits it as one)
        if (!t || !o.content || !runs.has(o.run) || tbs.some((x) => x.t === t) || tbs.length >= 4) continue;
        tbs.push({ t, run: o.run, pg, hf: o.hf });
      }
    });
    for (const x of tbs) {
      runScope(`textbox ${x.pg}${x.hf ? ' hf' : ''}`, () => {
        el.tbEdit = { hostRun: x.run, chain: [x.t], focusRun: x.run, pageIdx: x.pg, rectTwips: null };
        el.hfEdit = x.hf ? { kind: 'header' } : null;
        el.model = { paragraphs: x.t.paras };
      });
    }
    const fin = s.finish();
    out['p4.edit'] = fin.value;
    // UO_F8_TRACE=<dir>: the item trace of p4.edit (a mismatch is located by diffing it with the
    // Rust one)
    if (process.env.UO_F8_TRACE) writeFileSync(join(process.env.UO_F8_TRACE, `${traceKey}.ts.trace`), fin.trace);
  }
  return out;
}

/** The extra layouts: forced hyphenation (twice) and the Web view. `read()` = a fresh FullDocument
 *  (K3: never lay the same objects out twice). */
export function layoutFacets(F, read, layout) {
  const out = {};
  // p4.hyph
  {
    const s = new Sink();
    const points = (full, res) => {
      const origins = full.blocks.map((b, i) => ({ kind: b.type === 'paragraph' ? 'para' : 'table', para: i, anchor: i }));
      const list = [];
      for (const page of res.pages) for (const ln of page.lines) {
        if (ln.hyph === undefined) continue;
        const loc = F.ed.locateRef(ln.ref, origins);
        if (loc.kind !== 'body') continue;
        list.push([loc.para, ln.hyph]);
      }
      return list;
    };
    const f1 = read();
    const asked = [];
    const r1 = layout(f1, { hyphenation: 'force', hyphenationSkip: (p, off) => { asked.push([p, off]); return false; } });
    const p1 = points(f1, r1);
    s.u32(p1.length);
    for (const [b, h] of p1) s.u32(b).u32(h);
    // the first two distinct points asked about in top-level body paragraphs, refused
    const skip = [];
    for (const [p, off] of asked) {
      const b = f1.blocks.findIndex((x) => x.type === 'paragraph' && x.paragraph === p);
      if (b < 0 || skip.some(([sb, so]) => sb === b && so === off)) continue;
      skip.push([b, off]);
      if (skip.length >= 2) break;
    }
    out['p4.hyphskip'] = skip.map(([b, o]) => `${b}:${o}`).join(',');
    const f2 = read();
    const sk = new Set(skip.map(([b, o]) => `${b}:${o}`));
    const r2 = layout(f2, { hyphenation: 'force', hyphenationSkip: (p, off) => sk.has(`${f2.blocks.findIndex((x) => x.type === 'paragraph' && x.paragraph === p)}:${off}`) });
    const p2 = points(f2, r2);
    s.u32(p2.length);
    for (const [b, h] of p2) s.u32(b).u32(h);
    out['p4.hyph'] = s.finish().value;
  }
  // p4.web
  {
    const full = F.views.webDocument(read(), 9000);
    const res = layout(full, {});
    const sink = new Sink();
    canonLayout(res, full, sink);
    out['p4.web'] = sink.finish().value;
    out['p4.web.pages'] = String(res.pages.length);
  }
  return out;
}

// ───────────────────────── the `--gen f8` documents ─────────────────────────

/** `--gen f8`: an F7c (headers, footers, notes, fields, bookmarks, headings) or F7b (tables,
 *  floats, textboxes, frames) document of the same seed, with hyperlinks (to bookmarks and
 *  external, through the document's relationships) and plugin inlines (`w:sdt` tagged
 *  `wordnext:inline:<type>:<json>`) appended to some paragraphs — what the run boxes, the
 *  navigation and the inline faces read. Pure function of the seed. */
export function decorateF8(bytes, seed, { unzipSync, zipSync, strToU8, strFromU8 }, Rng) {
  const rng = new Rng((seed ^ 0x5f8) >>> 0);
  const files = unzipSync(bytes);
  let doc = strFromU8(files['word/document.xml']);
  const names = [...doc.matchAll(/w:bookmarkStart w:id="\d+" w:name="([^"]+)"/g)].map((m) => m[1]);
  const rels = [];
  let n = 0;
  doc = doc.replace(/<\/w:p>/g, () => {
    const add = [];
    const roll = rng.below(100);
    if (roll < 18 && names.length) add.push(`<w:hyperlink w:anchor="${names[rng.below(names.length)]}"><w:r><w:rPr><w:u w:val="single"/></w:rPr><w:t xml:space="preserve"> veja ${rng.below(99)}</w:t></w:r></w:hyperlink>`);
    else if (roll < 30) {
      const id = `rIdF8${n++}`;
      rels.push(`<Relationship Id="${id}" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/hyperlink" Target="https://example.org/p/${rng.below(1000)}" TargetMode="External"/>`);
      add.push(`<w:hyperlink r:id="${id}"><w:r><w:t xml:space="preserve"> link ${rng.below(99)} longo o bastante</w:t></w:r></w:hyperlink>`);
    } else if (roll < 34) add.push(`<w:hyperlink w:anchor="nao_existe"><w:r><w:t xml:space="preserve"> quebrado</w:t></w:r></w:hyperlink>`);
    if (rng.below(100) < 12) {
      const data = encodeURIComponent(JSON.stringify({ id: rng.below(50) }));
      const fb = ['@ana', '@bê', ' ', '@😀x', '@Fulano de Tal'][rng.below(5)];
      add.push(`<w:sdt><w:sdtPr><w:tag w:val="wordnext:inline:${rng.below(2) ? 'mention' : 'chip'}:${data}"/></w:sdtPr><w:sdtContent><w:r><w:t xml:space="preserve">${fb}</w:t></w:r></w:sdtContent></w:sdt>`);
      if (rng.below(2)) add.push(`<w:r><w:t xml:space="preserve"> depois</w:t></w:r>`);
    }
    return `${add.join('')}</w:p>`;
  });
  if (!/xmlns:r=/.test(doc.slice(0, 2000))) doc = doc.replace('<w:document ', '<w:document xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships" ');
  files['word/document.xml'] = strToU8(doc);
  const relPath = 'word/_rels/document.xml.rels';
  const base = files[relPath] ? strFromU8(files[relPath]) : '<?xml version="1.0" encoding="UTF-8" standalone="yes"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"></Relationships>';
  files[relPath] = strToU8(base.replace('</Relationships>', `${rels.join('')}</Relationships>`));
  return zipSync(files);
}
