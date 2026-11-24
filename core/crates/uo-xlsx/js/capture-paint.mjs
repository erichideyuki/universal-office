// Parity capture of the PAINT layers (frontier C3c, record core/docs/modules/uo-xlsx.md "C3c — as built"): the frozen TS
// table/pivot styles (table-styles.ts, pivot-styles.ts), the cell style under table + CF (`styleAt`), borders
// (borders.ts, via the real `paintGridBorders` over a recording canvas), fills (fills.ts), the spill border and the
// grid-layout helpers, inside the real element headless (the harness of capture-grid.mjs, `shouldRecalcOnOpen = () =>
// false`, `Date` frozen). Deterministic: two runs give identical files.
//
//   node core/crates/uo-xlsx/js/capture-paint.mjs                       # -> tests/captures/sheet-paint.{capture,side.gz}
//   node core/crates/uo-xlsx/js/capture-paint.mjs --write-workbooks     # (re)writes tests/captures/workbooks/paint1/*.xlsx
//   node core/crates/uo-xlsx/js/capture-paint.mjs --lines               # also the sidecar lines of the border ops and displays (diagnosis)
//   node core/crates/uo-xlsx/js/capture-paint.mjs --big                 # local: paint-big (160 generated) + x1-big (400, from
//        core/target/sheet-cap/workbooks/x1-big) -> core/target/sheet-cap/paint-big.{capture,side.gz}
//
// Keys and facets (sidecar lines `<key>\t<facet>\t<text>`; colours canonical: `#rrggbb` lower case or the lower-cased
// text, `-` = none; floats as 16 hex digits of the IEEE bits, `nan`):
//   <workbook>     (`gen/x1/NNNN`, `gen/paint1/NNNN`, `samples/<file>`, `gen/paint-big/NNNN`, `gen/x1-big/NNNN`)
//     b<i>         the region of sheet i: `cols rows` (1..cols x 1..rows: the used area + 2, at most 60 x 120)
//     sp<i>        per table/pivot style name used: its resolved spec by element (`resolveTableStyle`)
//     t<i>         per cell with a table/pivot layer (`tableFmt`): fill, font colour, flags, border sides
//     f<i>         per cell with an autofilter button or a table handle (`filterAt`, `tableEnds`)
//     s<i>         per interesting cell the final `styleAt`: fill (as paint), font facets, own borders, table borders,
//                  diagonal, whether a table/CF layer applies
//     p<i>         the canvas calls of `paintCellFill` for every filled cell (zoom 1.1, scroll 29.25 x 13.5)
//     bd<i>@<k>    the `fillRect`s of the real `paintGridBorders` over the region at two zoom/scroll settings
//     dg<i>        the strokes of `paintDiagonals` per cell with a diagonal
//     d<i>         the displayed text of every cell (`debugCellDisplays`)
//   misc/*         the pattern masks, `winningEdge`/`edgeExtent`, `paintHEdge`/`paintVEdge` over styles x corners x anchors,
//                  the 144 built-in styles under three themes, the grid-layout helpers, spill/handle, `layoutRichLines`
process.env.TZ = 'UTC';
const { readFileSync, mkdirSync, writeFileSync, createWriteStream, existsSync } = await import('node:fs');
const { join, dirname } = await import('node:path');
const { createHash } = await import('node:crypto');
const { createGzip } = await import('node:zlib');
const { pathToFileURL } = await import('node:url');
const { execFileSync } = await import('node:child_process');
const { CaptureWriter } = await import('../../uo-testkit/js/testkit.mjs');
const C = await import('./lib/common.mjs');
const { GROUPS, groupKey, workbookFile } = await import('./lib/gen-workbook.mjs');
const { PAINT_GROUPS, paintKey, buildPaintWorkbook, TABLE_STYLE_NAMES, PIVOT_STYLE_NAMES, BORDER_STYLES } = await import('./lib/gen-paint.mjs');
const { buildHarness } = await import('./harness/build-harness.mjs');

const FIXED = Date.UTC(2025, 0, 15, 12, 0, 0);
const RealDate = Date;
globalThis.Date = class FixedDate extends RealDate {
  constructor(...x) { if (x.length === 0) super(FIXED); else super(...x); }
  static now() { return FIXED; }
};

const a = C.args();
const root = C.tsRoot(a);
const BIG = a.flag('--big');
const outPath = a.opt('--out', BIG ? join(C.bigDir, 'paint-big.capture') : join(C.capturesDir, 'sheet-paint.capture'));
const paintDir = join(C.capturesDir, 'workbooks', 'paint1');

if (a.flag('--write-workbooks')) {
  mkdirSync(paintDir, { recursive: true });
  for (let i = 0; i < PAINT_GROUPS.paint1.count; i++) { const b = buildPaintWorkbook('paint1', i); writeFileSync(join(paintDir, b.name), b.bytes); }
  console.log(`paint1: ${PAINT_GROUPS.paint1.count} workbooks -> ${paintDir}`);
  process.exit(0);
}

// ---------------------------------------------------------------- the bundle
function buildPaintBundle() {
  const out = join(C.coreDir, 'target', 'sheet-cap', 'harness');
  const h = buildHarness(root, out);
  const file = join(out, 'paint.mjs');
  const stamp = join(out, 'paint.stamp');
  const sp = join(root, 'sheet', 'packages');
  const want = JSON.stringify({ commit: h.commit, v: 1 });
  if (existsSync(file) && existsSync(stamp) && readFileSync(stamp, 'utf8') === want) return { ...h, paint: file };
  const esbuild = join(root, 'sheet', 'node_modules', 'esbuild', 'bin', 'esbuild');
  const alias = {
    '@universal-office/sheet-core': join(sp, 'sheet-core/src/index.ts'),
    '@universal-office/sheet-calc': join(sp, 'calc/src/index.ts'),
    '@universal-office/fonts': join(root, 'shared/fonts/src/index.ts'),
    '@universal-office/ui': join(root, 'shared/ui/src/index.ts'),
    '@universal-office/charts': join(root, 'shared/charts/src/index.ts'),
    '@universal-office/drawingml': join(root, 'shared/drawingml/src/index.ts'),
    '@universal-office/slides-core': join(root, 'slides/packages/slides-core/src/index.ts'),
    vitest: join(C.crateDir, 'js', 'harness', 'vitest-shim.mjs'),
  };
  const p = (rel) => join(sp, rel).replace(/\\/g, '/');
  const entry = join(out, 'paint-entry.ts');
  writeFileSync(entry, [
    `export * from ${JSON.stringify(p('sheet/src/universal-sheet.ts'))};`,
    `export * as sheetCore from ${JSON.stringify(p('sheet-core/src/index.ts'))};`,
    `export * as fills from ${JSON.stringify(p('sheet/src/fills.ts'))};`,
    `export * as borders from ${JSON.stringify(p('sheet/src/borders.ts'))};`,
    `export * as tableStyles from ${JSON.stringify(p('sheet/src/table-styles.ts'))};`,
    `export * as gridLayout from ${JSON.stringify(p('sheet/src/grid-layout.ts'))};`,
    `export * as spill from ${JSON.stringify(p('sheet/src/spill-border.ts'))};`,
    '',
  ].join('\n'));
  execFileSync(process.execPath, [esbuild, entry, '--bundle', '--format=esm', '--platform=node', '--log-level=warning', '--legal-comments=none',
    ...Object.entries(alias).map(([k, v]) => `--alias:${k}=${v}`), `--outfile=${file}`], { cwd: join(sp, 'sheet'), stdio: ['ignore', 'inherit', 'inherit'] });
  writeFileSync(stamp, want);
  return { ...h, paint: file };
}

const built = buildPaintBundle();
const domMod = await import(pathToFileURL(built.dom).href);
const dom = domMod.installFakeDom();
dom.customElements.define('u-ripple', class extends globalThis.HTMLElement {});
const M = await import(pathToFileURL(built.paint).href);
let stubOn = false;
const newSheet = (attrs = {}) => {
  stubOn = false;
  const el = new M.UniversalSheetElement();
  stubOn = true;
  for (const [k, v] of Object.entries(attrs)) el.setAttribute(k, v);
  dom.document.body.appendChild(el);
  return el;
};
globalThis.DOMMatrix = class { constructor(m) { [this.a, this.b, this.c, this.d, this.e, this.f] = m; } };

const sha = (b) => createHash('sha256').update(b).digest('hex');
const dig = (s) => { const b = Buffer.from(s, 'utf8'); return `${b.length}:${sha(b).slice(0, 32)}`; };
const listDig = (lines) => dig(lines.join('\n'));
const side = [];
const w = new CaptureWriter(['Sheet paint layers (frontier C3c): see core/crates/uo-xlsx/js/capture-paint.mjs']);
w.meta('ts-root-commit', built.commit);
w.meta('ts-root-state', C.tsClean(root));
w.meta('tz', process.env.TZ);
const sx = (key, facet, text) => side.push(`${key}\t${facet}\t${text}`);
const facet = (key, name, lines) => { w.set(key, name, listDig(lines)); for (const l of lines) sx(key, name, l); };
/** Digest only (the sidecar lines of the big facets are written with `--lines`: committed size). */
const LINES = a.flag('--lines');
const facetD = (key, name, lines) => { if (LINES) facet(key, name, lines); else w.set(key, name, listDig(lines)); };

// ---------------------------------------------------------------- canonical texts
const fh = (x) => (Number.isNaN(x) ? 'nan' : C.f64hex(x));
const cc = (s) => {
  if (s === undefined || s === null) return '-';
  return /^#[0-9a-fA-F]{6}$/.test(s) ? s.toLowerCase() : s.toLowerCase();
};
const MASKS = M.gridLayout.PATTERN_MASKS;
const hasOwn = (o, k) => Object.prototype.hasOwnProperty.call(o, k);
const sty = (s) => (hasOwn(M.borders.BORDER_RANK, s) ? s : '?');
const ec = (e) => (e && e.style ? `${sty(e.style)}:${cc(e.color)}` : '-');
const sides4 = (b) => [b?.left, b?.right, b?.top, b?.bottom].map(ec).join('|');
const tri = (v) => (v === undefined ? '-' : v ? '1' : '0');
/** A fill as it paints (`hasFill` false = `-`). */
function fillDesc(f) {
  if (!f || !M.fills.hasFill(f)) return '-';
  const g = f.gradient;
  if (g && g.stops.length > 0) {
    if (g.stops.length === 1) return `S:${cc(g.stops[0].color)}`;
    const st = (arr) => arr.map((s) => `${fh(s.position)}=${cc(s.color)}`).join(',');
    if (g.type === 'path') return `GP:${[g.left ?? 0, g.right ?? 0, g.top ?? 0, g.bottom ?? 0].map(fh).join(',')}:${st([...g.stops].sort((x, y) => x.position - y.position))}`;
    return `GL:${fh(g.degree)}:${st(g.stops)}`;
  }
  const p = f.pattern;
  if (p === 'solid') return `S:${cc(f.fgColor ?? '#000000')}`;
  return `${hasOwn(MASKS, p) ? `P:${p}` : 'P?'}:${cc(f.fgColor ?? '#000000')}:${cc(f.bgColor ?? '#FFFFFF')}`;
}
/** A fill OBJECT (table layer): `u` absent, `0` present but invisible. */
const fillObj = (f) => (f === undefined ? 'u' : M.fills.hasFill(f) ? fillDesc(f) : '0');
const fontDesc = (f) => `${cc(f?.color)}|${f?.bold ? 1 : 0}${f?.italic ? 1 : 0}${f?.underline ? 1 : 0}${f?.strike ? 1 : 0}`;
const diagDesc = (b) => (b?.diagonal?.style && (b.diagonalUp || b.diagonalDown) ? `${ec(b.diagonal)}:${b.diagonalUp ? 1 : 0}${b.diagonalDown ? 1 : 0}` : '-');
const tfDesc = (t) => `F=${fillObj(t.fill)};C=${cc(t.fontColor)};f=${tri(t.bold)}${tri(t.italic)}${tri(t.underline)}${tri(t.strike)};B=${sides4(t.border)}`;
const KNOWN = ['wholeTable', 'firstColumnStripe', 'secondColumnStripe', 'firstRowStripe', 'secondRowStripe', 'lastColumn', 'firstColumn', 'headerRow', 'totalRow', 'firstHeaderCell', 'lastHeaderCell', 'firstTotalCell', 'lastTotalCell', 'firstSubtotalColumn', 'secondSubtotalColumn', 'thirdSubtotalColumn', 'firstSubtotalRow', 'secondSubtotalRow', 'thirdSubtotalRow', 'firstRowSubheading', 'secondRowSubheading', 'thirdRowSubheading', 'blankRow', 'pageFieldLabels', 'pageFieldValues'];
const elDesc = (f) => `F=${fillObj(f.fill)};C=${cc(f.fontColor)};f=${tri(f.bold)}${tri(f.italic)}${tri(f.underline)}${tri(f.strike)};sz=${f.size === undefined ? '-' : fh(f.size)};B=${[f.border?.left, f.border?.right, f.border?.top, f.border?.bottom, f.border?.vertical, f.border?.horizontal].map(ec).join('|')}`;
const specDesc = (spec) => (spec ? KNOWN.filter((k) => spec.has(k)).map((k) => `${k}{${elDesc(spec.get(k))}}`).join(';') : 'null');
const colLetters = (col) => { let s = ''; for (let c = col; c > 0; c = Math.floor((c - 1) / 26)) s = String.fromCharCode(65 + ((c - 1) % 26)) + s; return s; };
const refOf = (c, r) => `${colLetters(c)}${r}`;

// ---------------------------------------------------------------- the recording canvas
const num = (x) => String(x);
let tileSeq = 0;
function stubCanvas() {
  const rec = { bg: '-', fg: '-', rows: Array(8).fill(0), seq: ++tileSeq };
  const base = {
    fillStyle: '#000',
    fillRect(x, y, ww, hh) {
      if (ww === 8 && hh === 8) { rec.bg = cc(this.fillStyle); return; }
      rec.fg = cc(this.fillStyle);
      rec.rows[y] |= 1 << (7 - x);
    },
    measureText: (t) => ({ width: t.length * 6, actualBoundingBoxAscent: 8, actualBoundingBoxDescent: 2, fontBoundingBoxAscent: 9, fontBoundingBoxDescent: 3 }),
    createImageData(ww, hh) { return { width: ww, height: hh, data: new Uint8ClampedArray(ww * hh * 4) }; },
    putImageData(img) { rec.img = sha(Buffer.from(img.data.buffer, img.data.byteOffset, img.data.byteLength)).slice(0, 16); },
  };
  // any other canvas method does nothing, as the fake DOM's context
  const ctx = new Proxy(base, { get(t, k) { return k in t ? t[k] : () => undefined; }, set(t, k, v) { t[k] = v; return true; } });
  return { width: 0, height: 0, rec, getContext: () => ctx };
}
const origCreate = dom.document.createElement.bind(dom.document);
dom.document.createElement = (tag) => (stubOn && tag === 'canvas' ? stubCanvas() : origCreate(tag));
// the element's own canvas is the fake DOM's; every other canvas (pattern tiles, the path-gradient raster, the text
// measurer) is a recording stub, also while the element paints on load (the tile cache of fills.ts is module-global)
const styleOf = (s) => {
  if (typeof s === 'string') return cc(s);
  if (s?.kind === 'lin') return `L(${s.p.map(fh).join(',')};${s.stops.map(([p, c]) => `${fh(p)}=${cc(c)}`).join(',')})`;
  if (s?.kind === "pat" && !s.tile.rec) throw new Error("tile without rec: " + Object.keys(s.tile).join(","));
  if (s?.kind === "pat") return `PAT(${cc(s.tile.rec.bg)}/${cc(s.tile.rec.fg)}/${s.tile.rec.rows.map((r) => r.toString(16).padStart(2, '0')).join('')}/${s.tr ? s.tr.map(fh).join(',') : '-'})`;
  return '?';
};
function fillRecorder() {
  const out = [];
  const ctx = {
    fillStyle: '#000',
    fillRect(x, y, ww, hh) { out.push(`FR ${styleOf(this.fillStyle)} ${fh(x)} ${fh(y)} ${fh(ww)} ${fh(hh)}`); },
    createLinearGradient(x0, y0, x1, y1) { return { kind: 'lin', p: [x0, y0, x1, y1], stops: [], addColorStop(p, c) { this.stops.push([p, c]); } }; },
    createPattern(tile) { return { kind: 'pat', tile, tr: undefined, setTransform(m) { this.tr = [m.e, m.f]; } }; },
    drawImage(off, x, y, ww, hh) { out.push(`DI ${off.rec.img} ${fh(x)} ${fh(y)} ${fh(ww)} ${fh(hh)}`); },
  };
  return { ctx, out };
}
function rectRecorder() {
  const out = [];
  const ctx = { fillStyle: '#000', fillRect(x, y, ww, hh) { out.push(`FR ${cc(this.fillStyle)} ${x} ${y} ${ww} ${hh}`); } };
  return { ctx, out };
}
function strokeRecorder() {
  const out = [];
  let lines = [];
  const ctx = {
    strokeStyle: '#000', lineWidth: 1, dash: [], clipR: undefined,
    save() {}, restore() {}, beginPath() { lines = []; }, rect(x, y, ww, hh) { this.clipR = [x, y, ww, hh]; }, clip() {},
    setLineDash(d) { this.dash = d; },
    moveTo(x, y) { lines.push([x, y]); }, lineTo(x, y) { lines[lines.length - 1].push(x, y); },
    stroke() { out.push(`L ${lines.map((l) => l.map(fh).join(',')).join(' ')}`); lines = []; },
  };
  return { ctx, out, head: () => `ST ${cc(ctx.strokeStyle)} ${fh(ctx.lineWidth)} [${ctx.dash.map(fh).join(',')}] ${ctx.clipR.map(fh).join(',')}` };
}

// ---------------------------------------------------------------- workbooks
/**
 * `debugCellDisplays` through `styleAt` always (the TS debug helper skips the table layer when the sheet has no conditional
 * format, but the painter uses `styleAt`): the same walk of `rows` then `cells` of the model.
 */
function displayLines(el, v) {
  const out = [];
  for (const r of v.worksheet.rows) for (const c of r.cells) {
    let col;
    try { col = M.sheetCore.refToRowCol(c.ref).col; } catch { continue; }
    const mm = el.mergeAt(col, r.r);
    const m = mm?.isOrigin ? mm : null;
    let wd = 0;
    for (let k = m ? m.firstCol : col; k <= (m ? m.lastCol : col); k++) if (!v.hiddenCols.has(k)) wd += v.colWidths.get(k) ?? v.defaultColW;
    const st = el.styleAt(c, col, r.r);
    if (st.cf?.hideValue) { out.push(JSON.stringify(`${c.ref}	`)); continue; }
    const lay = el.cellLayout(c, st, wd, 1);
    out.push(JSON.stringify(`${c.ref}	${lay?.ct.text ?? ''}`));
  }
  return out;
}

const MAX_COLS = 60, MAX_ROWS = 120;
const CFG = [{ z: 1, sx: 0, sy: 0 }, { z: 1.75, sx: 41.5, sy: 17.25 }];
const FILL_CFG = { z: 1.1, sx: 29.25, sy: 13.5 };
const results = [];

function paintFacets(key, bytes) {
  const el = newSheet({ 'formula-locale': 'pt-BR' });
  el.shouldRecalcOnOpen = () => false;
  try { el.loadXlsx(bytes); } catch (e) { w.set(key, 'open', `err:${C.oneLine(e?.message ?? e)}`); el.remove(); return; }
  w.set(key, 'open', 'ok');
  const views = el.views ?? [];
  const pkg = M.sheetCore.openSpreadsheet(bytes);
  const cx = M.sheetCore.colorContext(pkg);
  views.forEach((v, i) => {
    el.sheetIndex = i;
    const ws = v.worksheet;
    const idx = el.cellIndex(v, i);
    // the region: the used area + 2, and every table/pivot rectangle + 1
    let maxR = 0, maxC = 0;
    for (const r of ws.rows) { maxR = Math.max(maxR, r.r); for (const c of r.cells) { try { maxC = Math.max(maxC, M.sheetCore.refToRowCol(c.ref).col); } catch { /* odd ref */ } } }
    const rects = [...(ws.tables ?? []).map((t) => t.ref), ...(ws.pivots ?? []).map((p) => p.ref)].map((rf) => M.tableStyles.parseRange(rf)).filter(Boolean);
    for (const q of rects) { maxR = Math.max(maxR, q.r2); maxC = Math.max(maxC, q.c2); }
    const hasLayers = rects.length > 0 || v.condFormats.length > 0;
    const R2 = Math.max(8, Math.min(MAX_ROWS, maxR + 2)), C2 = Math.max(8, Math.min(MAX_COLS, maxC + 2));
    w.set(key, `b${i}`, `${C2} ${R2}`);
    // styles used
    const names = [...new Set([...(ws.tables ?? []).map((t) => t.styleName), ...(ws.pivots ?? []).map((p) => p.styleName)].filter((n) => n))].sort();
    facet(key, `sp${i}`, names.map((n) => `${JSON.stringify(n)}\t${specDesc(M.tableStyles.resolveTableStyle(n, el.styles, cx))}`));
    const tl = [], fl = [], sl = [], pl = [], dl = [], gl = [];
    const cfEng = v.condFormats.length > 0 ? el.cfEngine() : null;
    el.zoom = FILL_CFG.z; el.recalcGrid();
    const xsF = Float64Array.from(el.xs), ysF = Float64Array.from(el.ys);
    for (let r = 1; r <= R2; r++) for (let c = 1; c <= C2; c++) {
      const at = refOf(c, r);
      const tf = v.tableFmt.get(`${c},${r}`);
      if (tf) tl.push(`${at}\t${tfDesc(tf)}`);
      const fa = v.filterAt.get(`${c},${r}`);
      const end = v.tableEnds.has(`${c},${r}`);
      if (fa !== undefined || end) fl.push(`${at}\t${fa === undefined ? '-' : fa ? 1 : 0};${end ? 1 : 0}`);
      const cell = idx.get(r)?.get(c);
      const st = el.styleAt(cell, c, r);
      const cf = cfEng ? cfEng.at(c, r) : undefined;
      const diag = diagDesc(st.border);
      const any = M.fills.hasFill(st.fill) || [...(st.border ? [st.border.left, st.border.right, st.border.top, st.border.bottom] : []), ...(st.tableBorder ? [st.tableBorder.left, st.tableBorder.right, st.tableBorder.top, st.tableBorder.bottom] : [])].some((e) => e?.style) || diag !== '-' || tf || cf;
      if (any) sl.push(`${at}\tF=${fillDesc(st.fill)};N=${fontDesc(st.font)};B=${sides4(st.border)};T=${sides4(st.tableBorder)};D=${diag};K=${tf ? 1 : 0}${cf ? 1 : 0}`);
      if (M.fills.hasFill(st.fill)) {
        const { ctx, out } = fillRecorder();
        const left = xsF[c - 1] - FILL_CFG.sx, top = ysF[r - 1] - FILL_CFG.sy;
        M.fills.paintCellFill(ctx, st.fill, left, top, (xsF[c] - xsF[c - 1]) + 1, (ysF[r] - ysF[r - 1]) + 1, -FILL_CFG.sx, -FILL_CFG.sy);
        pl.push(`${at}\t${out.join(' | ')}`);
      }
      if (diag !== '-') {
        const rec = strokeRecorder();
        M.borders.paintDiagonals(rec.ctx, st.border, xsF[c - 1] - FILL_CFG.sx, ysF[r - 1] - FILL_CFG.sy, xsF[c] - xsF[c - 1], ysF[r] - ysF[r - 1]);
        gl.push(`${at}\t${rec.head()} ${rec.out.join(' ')}`);
      }
    }
    facet(key, `t${i}`, tl); facet(key, `f${i}`, fl); facet(key, `s${i}`, sl); facet(key, `p${i}`, pl); facet(key, `dg${i}`, gl);
    // the border pass of the element itself over the region
    CFG.forEach((cfg, k) => {
      el.zoom = cfg.z; el.recalcGrid();
      const { ctx, out } = rectRecorder();
      el.paintGridBorders(ctx, idx, 1, C2, 1, R2, cfg.sx, cfg.sy);
      facetD(key, `bd${i}@${k}`, out);
    });
    el.zoom = 1; el.recalcGrid();
    void hasLayers; void dl;
  });
  // displayed text (the Range.Text equivalent) of every cell: the table layer changes the measured font
  const keep = el.activeSheet;
  for (let i = 0; i < views.length && key.includes('paint'); i++) {
    el.selectSheet(i);
    facetD(key, `d${i}`, displayLines(el, views[i]));
  }
  el.selectSheet(keep);
  el.remove();
}

const t0 = performance.now();
if (!BIG) {
  for (let i = 0; i < PAINT_GROUPS.paint1.count; i++) { const key = paintKey('paint1', i); paintFacets(key, new Uint8Array(buildPaintWorkbook('paint1', i).bytes)); results.push(key); }
  for (let i = 0; i < GROUPS.x1.count; i++) { const key = groupKey('x1', i); paintFacets(key, new Uint8Array(readFileSync(join(C.capturesDir, 'workbooks', 'x1', workbookFile('x1', i))))); results.push(key); }
  for (const rel of C.trackedSamples()) { if (/giant/.test(rel)) continue; const key = `samples/${rel}`; paintFacets(key, C.readSample(rel)); results.push(key); }
} else {
  const bigDir = join(C.bigDir, 'workbooks', 'paint-big');
  mkdirSync(bigDir, { recursive: true });
  for (let i = 0; i < PAINT_GROUPS['paint-big'].count; i++) {
    const key = paintKey('paint-big', i);
    const b = buildPaintWorkbook('paint-big', i);
    writeFileSync(join(bigDir, b.name), b.bytes);
    paintFacets(key, new Uint8Array(b.bytes));
    results.push(key);
  }
  const dir = a.opt('--workbooks', join(C.bigDir, 'workbooks', 'x1-big'));
  for (let i = 0; i < GROUPS['x1-big'].count; i++) { const key = groupKey('x1-big', i); paintFacets(key, new Uint8Array(readFileSync(join(dir, workbookFile('x1-big', i))))); results.push(key); }
}

// ---------------------------------------------------------------- the built-in styles under three themes (the generated workbooks 0..2 carry the themes)
{
  const lines = [];
  for (let t = 0; t < 3; t++) {
    const bytes = new Uint8Array(buildPaintWorkbook('paint1', t).bytes);
    const pkg = M.sheetCore.openSpreadsheet(bytes);
    const cx = M.sheetCore.colorContext(pkg);
    const styles = M.sheetCore.readStyles(pkg, cx);
    for (const n of [...TABLE_STYLE_NAMES, ...PIVOT_STYLE_NAMES]) lines.push(`${t}\t${n}\t${specDesc(M.tableStyles.resolveTableStyle(n, styles, cx))}`);
    for (const n of ['', 'constructor', 'toString', 'NoSuchStyle', 'tablestylemedium2']) lines.push(`${t}\t${JSON.stringify(n)}\t${specDesc(n === 'constructor' || n === 'toString' ? undefined : M.tableStyles.resolveTableStyle(n, styles, cx))}`);
  }
  facet('misc', 'specs', lines);
}

// ---------------------------------------------------------------- pure helpers over fixed inputs
{
  facet('misc', 'masks', Object.entries(MASKS).map(([k, v]) => `${k} ${v.map((n) => n.toString(16).padStart(2, '0')).join('')}`));
  const ST = [undefined, ...BORDER_STYLES, 'zigzag'];
  const lines = [];
  for (const s of ST) lines.push(`ext ${s ?? '-'} ${M.borders.edgeExtent(s).join(',')} ${s ? (M.borders.BORDER_RANK[s] ?? 6) : 0}`);
  const E = (s, k) => (s ? { style: s, color: `#0000${k}${k}` } : undefined);
  for (const p of ST) for (const q of ST) {
    const win = M.borders.winningEdge(E(p, '1'), E(q, '2'));
    lines.push(`win ${p ?? '-'} ${q ?? '-'} ${win ? (win === E(p, '1') ? 'x' : win.color === '#000011' ? 'a' : 'b') : '-'}`);
  }
  facet('misc', 'edges', lines);
  const ops = [];
  const CORN = [M.borders.NO_CORNER, { before: 1, after: 1, doubleBefore: false, doubleAfter: false }, { before: 0, after: 1, doubleBefore: true, doubleAfter: true }, { before: 1, after: 0, doubleBefore: true, doubleAfter: false }];
  for (const s of [...BORDER_STYLES, 'zigzag']) for (let ci = 0; ci < CORN.length; ci++) for (const anchor of [0, 7, -5, 1000003]) for (const erase of [null, '#ffffff']) for (const horiz of [true, false]) {
    const { ctx, out } = rectRecorder();
    const e = { style: s, color: '#ab12cd' };
    const c0 = CORN[ci], c1 = CORN[(ci + 1) % CORN.length];
    if (horiz) M.borders.paintHEdge(ctx, e, 10, 70, 20, c0, c1, anchor, erase); else M.borders.paintVEdge(ctx, e, 10, 70, 20, c0, c1, anchor, erase);
    ops.push(`${s} ${ci} ${anchor} ${erase ? 1 : 0} ${horiz ? 'h' : 'v'}\t${out.join(' | ')}`);
  }
  facet('misc', 'hv', ops);
  // grid-layout: edges built as recalcGrid does (xs[i] = acc; acc += size(i + 1))
  const gl = [];
  const SIZES = [[1, 5, 30], [7, 7, 0], [10, 40, 17.5], [55, 60, 0]];
  const edges = (max, dflt, zoom) => {
    const xs = new Float64Array(max + 2);
    let acc = 0;
    const size = (e) => { const r = SIZES.find((q) => q[0] <= e && e <= q[1]); return (r ? r[2] : dflt) * zoom; };
    for (let k = 0; k <= max; k++) { xs[k] = acc; acc += size(k + 1); }
    xs[max + 1] = acc;
    return xs;
  };
  for (const zoom of [1, 1.75]) {
    const xs = edges(80, 64, zoom);
    for (const frozen of [0, 2, 9]) for (const scroll of [0, 123.5, 700]) {
      gl.push(`ext ${zoom} ${frozen} ${fh(M.gridLayout.frozenExtent(xs, frozen))}`);
      for (const px of [-1, 0, 0.5, 29, 30, 100, 127.9, 128, 129, 400, 1000, 5000, 1e6, NaN]) {
        gl.push(`idx ${zoom} ${frozen} ${scroll} ${fh(px)} ${M.gridLayout.pxToIndex(xs, px, frozen, scroll, 80)} ${M.gridLayout.boundaryAt(xs, px, frozen, scroll, 80)} ${M.gridLayout.boundaryAt(xs, px, frozen, scroll, 80, 0.5)}`);
      }
      for (const k of [0, 1, 2, 3, 10, 60, 81, 82, 99]) gl.push(`pos ${zoom} ${frozen} ${scroll} ${k} ${fh(M.gridLayout.indexScreenPos(xs, k, frozen, scroll))}`);
    }
    for (const bi of [0, 1, 2, 5, 8]) gl.push(`bis ${zoom} ${bi} ${[0, 29.9, 30, 150, 9999].map((x) => M.gridLayout.bisectLE(xs, x)).join(',')}`);
  }
  for (const mdw of [7, 8, 9, 6]) for (let px = 0; px <= 300; px += 7) gl.push(`cw ${mdw} ${px} ${fh(M.gridLayout.pxToColWidthChars(px, mdw))} ${fh(M.gridLayout.pxToRowHeightPt(px))}`);
  facet('misc', 'layout', gl);
  const sl = [];
  for (const [x1, y1, x2, y2] of [[10.4, 20.6, 50.2, 40.5], [10, 10, 10, 20], [0, 0, 1, 1], [5.5, 5.5, 6.5, 6.5], [-3.5, 2.5, 99.5, 9.5], [10, 10, 5, 5]]) {
    const r = rectRecorder();
    r.ctx.save = () => {}; r.ctx.restore = () => {}; r.ctx.setLineDash = () => {};
    let call;
    r.ctx.strokeRect = (...q) => { call = q.map(fh).join(','); };
    M.spill.paintSpillBorder(r.ctx, x1, y1, x2, y2);
    sl.push(`spill ${[x1, y1, x2, y2].map(fh).join(',')} ${call ?? '-'} ${cc(r.ctx.strokeStyle)}`);
  }
  for (const [rx, by] of [[100.4, 50.5], [0, 0], [-1.5, 2.5], [33.5, 17.49]]) {
    const r = rectRecorder();
    M.tableStyles.paintTableHandle(r.ctx, rx, by);
    sl.push(`handle ${fh(rx)} ${fh(by)} ${r.out.join(' | ')}`);
  }
  facet('misc', 'spill', sl);
  // rich lines: measure = 6 + run px per UTF-16 unit
  const rl = [];
  const SAMPLES = [['aaa bbb ccc', 'ddd eee'], ['a\nb', ' c d '], ['', ''], ['abcdefghij'], ['  x  y  '], ['ab\n\ncd'], ['😀😀😀 x'], ['lone\ud800surrogate word'], ['a b', 'c d', 'e f g']];
  for (const t of SAMPLES) for (const mw of [0, 10, 25, 61, 120, 1000, NaN]) {
    const lines = M.gridLayout.layoutRichLines(t, mw, (s, run) => s.length * (6 + run));
    rl.push(`${JSON.stringify(t)} ${fh(mw)}\t${lines.map((l) => l.map((sg) => `${JSON.stringify(sg.text)}#${sg.run}`).join('+')).join(' / ')}`);
  }
  facet('misc', 'rich', rl);
}

// ---------------------------------------------------------------- write
mkdirSync(dirname(outPath), { recursive: true });
w.write(outPath);
{
  const gz = createGzip({ level: 9 });
  const done = new Promise((res, rej) => gz.pipe(createWriteStream(outPath.replace(/\.capture$/, '.side.gz'))).on('finish', res).on('error', rej));
  for (let i = 0; i < side.length; i += 10000) gz.write(`${side.slice(i, i + 10000).join('\n')}\n`);
  gz.end();
  await done;
}
console.log(`sheet-paint: ${results.length} workbooks, ${side.length} sidecar lines in ${((performance.now() - t0) / 1000).toFixed(1)} s -> ${outPath}`);
