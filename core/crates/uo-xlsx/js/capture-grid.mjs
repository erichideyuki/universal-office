// Parity capture of the Sheet GRID MODEL (frontier C3a, record core/docs/modules/uo-xlsx.md "C3a — as built"): the
// frozen TS component (headless, the same harness as capture-sheet.mjs) and its pure modules, deterministic.
//
//   node core/crates/uo-xlsx/js/capture-grid.mjs                       # -> tests/captures/sheet-grid.{capture,side.gz}
//   node core/crates/uo-xlsx/js/capture-grid.mjs --out <file> [--no-samples] [--ts-root <dir>]
//   node core/crates/uo-xlsx/js/gen-workbooks.mjs --gen x1-big --out core/target/sheet-cap/workbooks/x1-big     # local (400)
//   node core/crates/uo-xlsx/js/capture-grid.mjs --gen x1-big --workbooks core/target/sheet-cap/workbooks/x1-big //        --workbooks-only --out core/target/sheet-cap/grid-x1-big.capture                                  # local, 1.3 MiB
//
// Keys and facets (sidecar lines `<key>\t<facet>\t<text>`, digests `len:sha256[0..32]` in the .capture):
//   <workbook>   (`gen/x1/NNNN`, `samples/<file>`; formula-locale pt-BR as capture-sheet)
//     g<i>       grid summary of sheet i after the open: mdw, normalRowPx, defaults, extents, frozen panes, and the
//                `colWidths` / `rowHeights` / `rowDesc` entries that differ from the default (the Rust model keeps only
//                those), hidden columns/rows — sorted, one JSON object
//     x<i>@<z>   `len:sha256` of the bytes (f64 little endian) of the TS `xs` + `ys` arrays at zoom z
//     d<i>       the displayed text of every cell of sheet i (lines `"ref<TAB>text"` as JSON strings); the workbooks
//                are opened WITHOUT the open-recalc (`shouldRecalcOnOpen = () => false`): the model holds the file's
//                values, which is what the Rust store holds
//   metrics      lines: excelRowPx / excelMdw / rowDescentPx / measuredFamily over families x sizes x bold, and the
//                `gdiGlyphFixes` of every measured family/bold/ppem as `cp:width` lists (digest per line)
//   layout/fonts the CellFont facts of the font specs (ppem, ascent, descent, xlAscent, xlDescent)
//   layout/*     `layoutCellText` over seeded inputs with the fonts above; the measurement is the fake canvas of the
//                harness (width = 6 per UTF-16 unit, `'Hg'` box 9/3); lines `in <TAB> out`, strings as UTF-16 hex
//   entry/<lang> `parseEntry` over typed strings (hand-picked + seeded), contexts of the current format category
//   entry/format `formatCategory`, `shouldApplyFormat`, `inferFormulaFormat`, `numberFormatFor`, `adjustDecimals`
//   entry/edit   `editText` over values x categories x languages
// Not captured: the viewport (C4), the CF/table layers of a cell (C3b/C3c), the browser canvas (the fake measures 6 px).
process.env.TZ = 'UTC';
const { readFileSync, mkdirSync, writeFileSync, createWriteStream, existsSync } = await import('node:fs');
const { join, dirname } = await import('node:path');
const { createHash } = await import('node:crypto');
const { createGzip } = await import('node:zlib');
const { pathToFileURL } = await import('node:url');
const { execFileSync } = await import('node:child_process');
const { CaptureWriter, Rng } = await import('../../uo-testkit/js/testkit.mjs');
const C = await import('./lib/common.mjs');
const { GROUPS, groupKey, workbookFile } = await import('./lib/gen-workbook.mjs');
const { buildHarness } = await import('./harness/build-harness.mjs');

// host injection (decision C1, as capture-sheet.mjs): `Date` frozen at 2025-01-15 12:00 UTC (the year of an entry typed
// without one), TZ = UTC
const FIXED = Date.UTC(2025, 0, 15, 12, 0, 0);
const RealDate = Date;
globalThis.Date = class FixedDate extends RealDate {
  constructor(...x) { if (x.length === 0) super(FIXED); else super(...x); }
  static now() { return FIXED; }
};

const a = C.args();
const root = C.tsRoot(a);
const outPath = a.opt('--out', join(C.capturesDir, 'sheet-grid.capture'));

// ---------------------------------------------------------------- the bundle (own entry: the pure modules too)
function buildGridBundle() {
  const out = join(C.coreDir, 'target', 'sheet-cap', 'harness');
  const h = buildHarness(root, out);
  const file = join(out, 'grid.mjs');
  const stamp = join(out, 'grid.stamp');
  const sp = join(root, 'sheet', 'packages');
  const want = JSON.stringify({ commit: h.commit, v: 2 });
  if (existsSync(file) && existsSync(stamp) && readFileSync(stamp, 'utf8') === want) return { ...h, grid: file };
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
  const entry = join(out, 'grid-entry.ts');
  writeFileSync(entry, [
    `export * from ${JSON.stringify(p('sheet/src/universal-sheet.ts'))};`,
    `export * as sheetCore from ${JSON.stringify(p('sheet-core/src/index.ts'))};`,
    `export * as calc from ${JSON.stringify(p('calc/src/index.ts'))};`,
    `export * as cellText from ${JSON.stringify(p('sheet/src/cell-text.ts'))};`,
    `export * as entryMod from ${JSON.stringify(p('sheet/src/entry.ts'))};`,
    `export * as entryFormat from ${JSON.stringify(p('sheet/src/entry-format.ts'))};`,
    `export * as editTextMod from ${JSON.stringify(p('sheet/src/edit-text.ts'))};`,
    `export * as formulaLocaleMod from ${JSON.stringify(p('sheet/src/formula-locale.ts'))};`,
    `export * as numberFormats from ${JSON.stringify(p('sheet/src/number-formats.ts'))};`,
    `export * as textMetrics from ${JSON.stringify(p('sheet/src/text-metrics.ts'))};`,
    `export * as cellPaint from ${JSON.stringify(p('sheet/src/cell-paint.ts'))};`,
    '',
  ].join('\n'));
  execFileSync(process.execPath, [esbuild, entry, '--bundle', '--format=esm', '--platform=node', '--log-level=warning', '--legal-comments=none',
    ...Object.entries(alias).map(([k, v]) => `--alias:${k}=${v}`), `--outfile=${file}`], { cwd: join(sp, 'sheet'), stdio: ['ignore', 'inherit', 'inherit'] });
  writeFileSync(stamp, want);
  return { ...h, grid: file };
}

const built = buildGridBundle();
const domMod = await import(pathToFileURL(built.dom).href);
const dom = domMod.installFakeDom();
dom.customElements.define('u-ripple', class extends globalThis.HTMLElement {});
const M = await import(pathToFileURL(built.grid).href);
const newSheet = (attrs = {}) => {
  const el = new M.UniversalSheetElement();
  for (const [k, v] of Object.entries(attrs)) el.setAttribute(k, v);
  dom.document.body.appendChild(el);
  return el;
};

// ---------------------------------------------------------------- helpers
const sha = (b) => createHash('sha256').update(b).digest('hex');
const dig = (s) => { const b = Buffer.from(s, 'utf8'); return `${b.length}:${sha(b).slice(0, 32)}`; };
const listDig = (lines) => dig(lines.join('\n'));
/** UTF-16 units as 4 hex digits each ('-' for undefined). */
const h16 = (s) => (s === undefined || s === null ? '-' : Array.from({ length: s.length }, (_, i) => s.charCodeAt(i).toString(16).padStart(4, '0')).join('') || '=');
const b = (v) => (v ? '1' : '0');
const side = [];
const w = new CaptureWriter(['Sheet grid model (frontier C3a): see core/crates/uo-xlsx/js/capture-grid.mjs']);
w.meta('ts-root-commit', built.commit);
w.meta('ts-root-state', C.tsClean(root));
w.meta('tz', process.env.TZ);
w.meta('canvas', 'fake: measureText(s).width = 6 per UTF-16 unit, Hg box 9/3 (write/test-support/fake-dom.ts)');
const sx = (key, facet, text) => side.push(`${key}\t${facet}\t${text}`);
const facet = (key, name, lines) => { w.set(key, name, listDig(lines)); for (const l of lines) sx(key, name, l); };

// ---------------------------------------------------------------- the element: grid facets per workbook
const ZOOMS = [1, 0.4, 1.1, 1.75, 3];
const SLICE = Number(a.opt('--slice', 200));
const entriesOf = (m, keep) => [...m.entries()].filter(([k, v]) => keep(v)).sort((x, y) => x[0] - y[0]);

function gridFacets(key, bytes) {
  const big = bytes.length > 400_000;
  const el = newSheet({ 'formula-locale': 'pt-BR' });
  // the host hook of the TS: no open-recalc, so the model holds the file's values (what the Rust store holds) and
  // the grid is the one of the open, not of the recalculation (which writes spill/array cells and grows maxRow)
  el.shouldRecalcOnOpen = () => false;
  try { el.loadXlsx(bytes); } catch (e) { w.set(key, 'open', `err:${C.oneLine(e?.message ?? e)}`); el.remove(); return; }
  w.set(key, 'open', 'ok');
  const views = el.views ?? [];
  w.set(key, 'sheets', String(views.length));
  const keepIndex = el.sheetIndex;
  views.forEach((v, i) => {
    const rowDescNormal = M.textMetrics.rowDescentPx(el.normalFont.name, el.normalFont.size, el.normalFont.bold) ?? 0;
    const g = {
      mdw: el.mdw, mdwFromCanvas: el.mdwFromCanvas, normalRowPx: el.normalRowPx, normalDesc: rowDescNormal,
      defaultColW: v.defaultColW, defaultRowH: v.defaultRowH, maxCol: v.maxCol, maxRow: v.maxRow,
      frozenCols: v.frozenCols, frozenRows: v.frozenRows,
      colWidths: entriesOf(v.colWidths, (x) => x !== v.defaultColW),
      rowHeights: entriesOf(v.rowHeights, (x) => x !== v.defaultRowH),
      rowDesc: entriesOf(v.rowDesc, (x) => x !== rowDescNormal),
      hiddenCols: [...v.hiddenCols].sort((x, y) => x - y),
      hiddenRows: [...v.hiddenRows].sort((x, y) => x - y),
      merges: v.merges.size,
    };
    const text = JSON.stringify(g);
    w.set(key, `g${i}`, dig(text));
    sx(key, `g${i}`, big && text.length > 200_000 ? `${text.slice(0, 200_000)}…` : text);
    el.sheetIndex = i;
    for (const z of ZOOMS) {
      el.zoom = z;
      el.recalcGrid();
      const buf = Buffer.concat([Buffer.from(el.xs.buffer, el.xs.byteOffset, el.xs.byteLength), Buffer.from(el.ys.buffer, el.ys.byteOffset, el.ys.byteLength)]);
      w.set(key, `x${i}@${z}`, `${el.xs.length}+${el.ys.length}:${sha(buf).slice(0, 32)}`);
    }
    el.zoom = 1;
  });
  el.sheetIndex = keepIndex;
  // the displayed text of every cell (`debugCellDisplays`: the Range.Text equivalent), per sheet; the giants keep the
  // first SLICE rows and 40 columns
  const active = el.activeSheet;
  for (let i = 0; i < views.length; i++) {
    el.selectSheet(i);
    const lines = (big ? el.debugCellDisplays(40, SLICE) : el.debugCellDisplays()).map((d) => JSON.stringify(`${d.ref}	${d.display}`));
    w.set(key, `d${i}`, `${big ? 'slice:' : ''}${listDig(lines)}`);
    for (const l of lines) sx(key, `d${i}`, l);
  }
  el.selectSheet(active);
  el.remove();
}

const t0 = performance.now();
const results = [];
const GROUP = a.opt('--gen', 'x1');
if (!a.flag('--no-workbooks')) {
  const g = GROUPS[GROUP];
  w.meta(`group-${GROUP}`, `seeds ${g.seedBase}..${g.seedBase + g.count - 1} workbooks ${g.count}`);
  const dir = a.opt('--workbooks', join(C.capturesDir, 'workbooks', GROUP));
  for (let i = 0; i < g.count; i++) {
    const key = groupKey(GROUP, i);
    gridFacets(key, new Uint8Array(readFileSync(join(dir, workbookFile(GROUP, i)))));
    results.push(key);
  }
  if (!a.flag('--no-samples') && GROUP === 'x1') {
    for (const rel of C.trackedSamples()) {
      const key = `samples/${rel}`;
      const ts = performance.now();
      gridFacets(key, C.readSample(rel));
      results.push(key);
      console.log(`  ${key}: ${((performance.now() - ts) / 1000).toFixed(1)} s`);
    }
  }
}

// ---------------------------------------------------------------- metrics
const ONLY_WB = a.flag('--workbooks-only');
if (!ONLY_WB) {
  const T = M.textMetrics, SC = M.sheetCore;
  // the measured families come from the TS tables through measuredFamily/excelRowPx; probe a fixed name list
  const names = ['Calibri', 'Arial', 'Aptos Narrow', 'Aptos', 'Aptos Display', 'Verdana', 'Tahoma', 'Times New Roman', 'Cambria', 'Courier New',
    'Segoe UI', 'MS Sans Serif', 'Century Gothic', 'Comic Sans MS', 'Arial Narrow', 'Garamond', 'Georgia', 'Trebuchet MS', 'Calibri Light', 'Consolas',
    'Liberation Sans', 'Noto Sans', 'Lucida Sans Typewriter', 'Microsoft Sans Serif', 'Book Antiqua', 'Franklin Gothic Medium', 'Gill Sans MT',
    'Arial Black', 'SimSun', 'MS PGothic', 'Yu Gothic', 'Malgun Gothic', 'PMingLiU', 'MingLiU', 'Dotum', 'Gulim', 'Batang', 'Courier', 'Symbol',
    'Wingdings', 'Segoe UI Light', 'MS Gothic', 'Arial Unicode MS', 'Eras Light ITC', 'Lucida Console', 'Palatino Linotype', 'Bookman Old Style',
    'Century', 'Arial Rounded MT Bold', 'Franklin Gothic Book', 'Microsoft YaHei', 'SimHei', 'Meiryo', 'Microsoft JhengHei', 'Lucida Sans Unicode',
    '宋体', '新宋体', 'ＭＳ Ｐゴシック', 'Helvetica', 'Helv', 'Tms Rmn', 'Times', 'Arial CYR', 'Arial CE', 'Times New Roman Cyr', 'Courier New CE',
    'No Such Font', '', 'ARIAL', '  calibri  ', 'Ｃａｌｉｂｒｉ', 'Aptos Mono', 'Segoe UI Semibold', 'Segoe UI Symbol'];
  const sizes = [];
  for (let s = 1; s <= 60; s += s < 20 ? 0.5 : 2.5) sizes.push(s);
  sizes.push(0, 0.3, 72, 100, 150, 11.25, 8.75);
  const lines = [];
  for (const n of names) {
    lines.push(`fam ${h16(n)} ${SC.measuredFamily(n) ?? '-'}`);
    for (const s of sizes) for (const bold of [false, true]) {
      const row = SC.excelRowPx(n, s, bold);
      const mdw = SC.excelMdw(n, s);
      const desc = T.rowDescentPx(n, s, bold);
      lines.push(`${h16(n)} ${s} ${b(bold)} ${row} ${mdw ?? '-'} ${desc ?? '-'}`);
    }
  }
  for (const n of [undefined, null]) lines.push(`undef ${SC.excelRowPx(n, 11, false)} ${SC.excelMdw(n, 11)} ${T.rowDescentPx(n, 11, false)}`);
  facet('metrics', 'rows', lines);
  const g = [];
  for (const n of names) for (const bold of [false, true]) for (let p = 6; p <= 34; p++) {
    const m = SC.gdiGlyphFixes(n, bold, p);
    g.push(`${h16(n)} ${b(bold)} ${p} ${m ? dig([...m.entries()].map(([ch, wd]) => `${ch.codePointAt(0)}:${wd}`).join(',')) : '-'} ${m ? m.size : -1}`);
  }
  facet('metrics', 'glyphs', g);
  const cl = [];
  for (const wd of [0, 0.5, 1, 3, 8.43, 9.140625, 10, 13, 20.5, 45.5, 100, 255, 1e9]) for (const mdw of [5, 6, 7, 8, 9, 10, 11, 12]) cl.push(`${wd} ${mdw} ${SC.colWidthToPx(wd, mdw)} ${SC.defaultColPx(mdw, wd > 0 ? wd : undefined, undefined)} ${SC.defaultColPx(mdw, undefined, wd > 0 ? wd : 8)}`);
  for (const pt of [0, 1, 7.5, 12.75, 15, 15.75, 20.25, 409.5, 1e-7]) cl.push(`pt ${pt} ${SC.rowPtToPx(pt)}`);
  facet('metrics', 'cols', cl);
}

if (!ONLY_WB) {
// ---------------------------------------------------------------- layout
const FONT_SPECS = [
  ['Calibri', 11, false, false, 1], ['Calibri', 11, true, false, 1], ['Arial', 10, false, false, 1], ['Arial', 8, true, false, 1],
  ['Times New Roman', 12, false, true, 1], ['Verdana', 9, false, false, 1], ['Segoe UI', 11, false, false, 1], ['Foo Sans', 11, false, false, 1],
  ['Courier New', 14, false, false, 1], ['Calibri', 20, true, false, 1], ['Aptos Narrow', 11, false, false, 1], ['Calibri', 11, false, false, 1.25],
  ['Arial', 11, false, false, 0.75], ['Tahoma', 6, false, false, 1], ['Lucida Sans Unicode', 13, true, false, 1],
];
const cellFonts = new M.textMetrics.CellFonts();
const fonts = FONT_SPECS.map(([n, s, bold, it, z]) => cellFonts.get(`'${n}', Arial`, n, s, bold, it, z));
{
  const lines = fonts.map((f, i) => `${i} ${f.ppem} ${f.ascent} ${f.descent} ${f.xlAscent} ${f.xlDescent} ${f.bold ? 1 : 0} ${f.italic ? 1 : 0}`);
  facet('layout', 'fonts', lines);
}
const CODES = [undefined, 'General', 'general', '0', '0.00', '#,##0.00', '0%', '0.00%', 'dd/mm/yyyy', 'hh:mm:ss', 'd "de" mmmm "de" yyyy', '"R$" #,##0.00',
  '_-"R$"\\ * #,##0.00_-;\\-"R$"\\ * #,##0.00_-;_-"R$"\\ * "-"??_-;_-@_-', '0.00E+00', '# ?/?', '[Red]0.00;[Blue]-0.00', '*-0', '0;;;', '@',
  '_(* #,##0.00_);_(* \\(#,##0.00\\);_(* "-"??_);_(@_)', 'mm:ss.0', '[h]:mm:ss', 'yyyy-mm-dd hh:mm', '"x"@', '_ @_ ', '@*.', '@*-', '*x@', ';;;', 'm/d/yyyy', 'dddd', '# ??/??', '#,##0', '0.0%', '[<1]0.00;0', 'General;General;"zero"', '#,##0_);[Red](#,##0)'];
const TEXT_CODES = [undefined, 'General', '@', '"x"@', '_ @_ ', '@*.', '@*-', '*x@', ';;;', '0.00', '"[" @ "]"', '@@', ';;;@', '[Red]@', '_-@_-'];
const NUMS = [0, 1, -1, 7, 42, 0.5, -0.5, 123456.789, 1e-5, 1e10, 1e21, -1234567.891, 0.0001234, 3.14159265358979, 45000, 45000.75, 0.75, 1 / 3, 100, 99999999999, 123456789012345680,
  2958465.99, 60, 59, 61, 0.99999, 12.5, -0.001, 1e-10, 1.5e300, 255, 1000, 999999, 0.1 + 0.2];
const TEXTS = ['abc', 'Texto longo com acentuação e mais palavras', '', ' leading', 'trail ', '😀 emoji', 'مرحبا بالعالم', 'שלום', 'ไทย', 'a'.repeat(40), 'x-y z', 'ÀÉÎÕÜ', '\ud800 lone', 'tab\tsep', 'line\nbreak', '1234', '007', '1,5', 'W'.repeat(12), '日本語のテキスト'];
const COLS = [0, 1, 4, 8, 12, 17, 24, 38, 50, 64, 100, 160, 400, 1000];
const rng = new Rng(0x6a09e667);
const pick = (l) => l[rng.below(l.length)];
function layoutLine(input, fi, colPx) {
  const f = fonts[fi];
  const ct = M.cellText.layoutCellText(input, f, colPx);
  const inLine = [input.kind, input.num === undefined ? '-' : C.f64hex(input.num), h16(input.str), h16(input.code), b(input.date1904), input.showZeros === false ? '0' : '1',
    C.f64hex(input.indentPx ?? 0), C.f64hex(input.edgePx ?? 0), C.f64hex(colPx), fi].join(' ');
  const out = [h16(ct.text), ct.color ?? '-', C.f64hex(ct.width), b(ct.hashes), b(ct.filled), b(ct.overflows), ct.pieces.map((p) => `${h16(p.s)}@${C.f64hex(p.x)}`).join(',') || '='].join(' ');
  return `${inLine}\t${out}`;
}
{
  const lines = [];
  for (let k = 0; k < 4200; k++) {
    const num = pick(NUMS);
    const inp = { kind: 'number', num, code: pick(CODES), date1904: rng.below(8) === 0, showZeros: rng.below(8) !== 0 };
    if (rng.below(6) === 0) inp.indentPx = pick([9, 18, 27]);
    if (rng.below(10) === 0) inp.edgePx = 1;
    lines.push(layoutLine(inp, rng.below(fonts.length), pick(COLS)));
  }
  facet('layout', 'num', lines);
}
{
  const lines = [];
  for (let k = 0; k < 1600; k++) {
    const inp = { kind: 'text', str: pick(TEXTS), code: pick(TEXT_CODES) };
    if (rng.below(6) === 0) inp.indentPx = pick([9, 18]);
    if (rng.below(12) === 0) inp.edgePx = 1;
    lines.push(layoutLine(inp, rng.below(fonts.length), pick(COLS)));
  }
  for (let k = 0; k < 240; k++) {
    const kind = pick(['bool', 'error']);
    const str = kind === 'bool' ? pick(['VERDADEIRO', 'FALSO', 'TRUE']) : pick(['#DIV/0!', '#N/D', '#NÚM!', '#DESPEJAR!', '#VALOR!', '#NOME?', '#OBTENDO_DADOS', '']);
    lines.push(layoutLine({ kind, str }, rng.below(fonts.length), pick(COLS)));
  }
  facet('layout', 'text', lines);
}
{
  // wrapLines (cell-paint) and the helpers of cell-text
  const lines = [];
  const WRAP = ['uma frase com várias palavras para quebrar', 'palavra-composta e-outra-ainda mais longa-assim', 'a\nb\n\nc', '', 'supercalifragilisticexpialidocious', 'x'.repeat(60), '  espaços   no   meio  ',
    'trailing   ', '😀😀😀 emoji emoji', 'a-', '---', '- -', 'one two\tthree four', '日本語 の テキスト です', 'مرحبا بالعالم الجميل', 'w'.repeat(30) + ' ' + 'm'.repeat(30)];
  for (let k = 0; k < 900; k++) {
    const fi = rng.below(fonts.length);
    const t = pick(WRAP);
    const mw = pick([1, 8, 20, 38, 64, 100, 160, 400]);
    const ml = pick([Infinity, Infinity, 1, 2, 3]);
    const r = M.cellPaint.wrapLines(fonts[fi], t, mw, ml);
    lines.push(`${fi} ${h16(t)} ${mw} ${ml === Infinity ? 'inf' : ml}\t${r.map((s) => h16(s)).join(',')}`);
  }
  facet('layout', 'wrap', lines);
  const m = [];
  for (const ppem of [6, 8, 10, 11, 12, 15, 16, 17, 20, 24, 32, 40]) m.push(`margin ${ppem} ${M.cellText.cellMarginPx(ppem)}`);
  for (const col of [0, 20, 38, 40, 64, 100, 584, 1019]) for (const [ppem, mdw] of [[15, 7], [13, 7], [20, 10], [11, 6]]) m.push(`wrap ${col} ${ppem} ${mdw} ${C.f64hex(M.cellText.autofitWrapWidthPx(col, ppem, mdw))}`);
  for (const t of ['007', '-5', '+5', '1,5', '1.234', '1.234,5', ' 12', '12%', 'R$ 12', '€12', '12€', '(12)', '1E5', ',5', '5,', '- 5', '1.5', '12.0', '1 234', '$12', '12,345,678', '1/2', '01/02/2020', '10:30', 'TRUE', '１２', '', ' ', '(', '()', 'R$', '1.2345', '12.345.678,9', '+-5', '1e', '1,e5', 'e5', ' (1.234,56) ', '1.234.', '-R$ 5', 'R$-5', '5 %', '5% %', '%5%'])
    m.push(`num ${h16(t)} ${b(M.cellText.looksLikeNumberPtBR(t))}`);
  for (const t of ['2023-01-15', '2023-01-15T10:30:00', '10:30', '10:30:15.5', '0050-03-01', '2023-13-45', '1899-12-30', '1900-02-28', '1900-03-01', '1904-01-01', '2023-1-5', '', 'x', 'T10:30', '2023-01-15T', '2023-01-15 10:30'])
    for (const d1904 of [false, true]) { const r = M.cellText.isoToSerial(t, d1904); m.push(`iso ${h16(t)} ${b(d1904)} ${r === undefined ? '-' : C.f64hex(r)}`); }
  for (const e of ['#NULL!', '#DIV/0!', '#VALUE!', '#REF!', '#NAME?', '#NUM!', '#N/A', '#GETTING_DATA', '#SPILL!', '#CALC!', '#FIELD!', '#BLOCKED!', '#UNKNOWN!', '#CONNECT!', '#BUSY!', '#PYTHON!', ' #n/a ', '#foo', ''])
    m.push(`err ${h16(e)} ${h16(M.cellText.localizeError(e))}`);
  facet('layout', 'helpers', m);
}

// ---------------------------------------------------------------- entry
const { calc, entryMod, entryFormat, editTextMod, formulaLocaleMod, numberFormats } = M;
const LOCALES = { pt: formulaLocaleMod.formulaLocale('pt-BR'), en: formulaLocaleMod.formulaLocale('en-US') };
const CATS = ['general', 'number', 'currency', 'percent', 'scientific', 'fraction', 'date', 'time', 'text'];
const fmtText = (f) => (f ? `${f.id}:${f.cat}:${h16(f.code)}` : '-');
function entryText(e) {
  switch (e.kind) {
    case 'clear': return 'C';
    case 'invalid': return `I ${h16(e.message)}`;
    case 'formula': return `F ${h16(e.text)}`;
    case 'value': {
      const v = e.value;
      const t = v === null ? '_' : typeof v === 'number' ? `n${C.f64hex(v)}` : typeof v === 'string' ? `s${h16(v)}` : typeof v === 'boolean' ? `b${b(v)}` : v instanceof calc.CalcError ? `e${v.code}` : '?';
      return `V ${t} ${fmtText(e.fmt)} ${b(e.quotePrefix)}`;
    }
    default: return '?';
  }
}
const NAMES = ['Taxa', 'Total'];
function entryLine(lang, cat, row, col, d1904, input, names = undefined) {
  const ctx = { locale: LOCALES[lang], current: cat, row, col, date1904: d1904, ...(names ? { names } : {}) };
  let r;
  try { r = entryText(entryMod.parseEntry(input, ctx)); } catch (e) { r = `T ${C.oneLine(e?.message ?? e, 60)}`; }
  return `${lang} ${cat} ${row} ${col} ${b(d1904)} ${names ? '1' : '0'} ${h16(input)}\t${r}`;
}
const TYPED = {
  pt: ['42', '-42', '+42', '-0,5', '1,5', '0,5', ',5', '5,', '50%', '1,5%', '% 5', '%5', '(5)', '(-5)', '-(5)', '1.234', '1.234,5', '1.2345', '12.34.56', '1 234', '1E3', '1e-3', '1,5E+3', '1E', '1,E3', 'R$ 10', 'R$10,50', 'R$ 1.234,56', '10 R$', '€10', '10€', '€ 10', '$10',
    'VERDADEIRO', 'verdadeiro', 'FALSO', 'Falso', ' VERDADEIRO ', 'TRUE', '#DIV/0!', '#div/0!', '#N/D', '#n/d', '#NÚM!', '#NUM!', '#NUMERO!', '#NOME?', '#VALOR!', '#REF!', '#NULO!', '#OBTENDO_DADOS', '#DESPEJAR!', '#CALC!', '#N/A', '#',
    '01/02/2024', '1/2/2024', '1/2', '31/12', '2024-01-02', '1-jan-2024', '1-jan', 'jan/24', 'janeiro 2024', '1 de janeiro de 2024', '29/02/2023', '29/02/2024', '01/13/2024', '10:30', '10:30:15', '10:30 PM', '10:30 AM', '25:00', '1:30,5', '12:00 A', '12:00 P', '10:30PM', '01/02/2024 10:30', '1/2/24 10:30:15', '1900-01-01', '0:00',
    '1/2', '3/4', '1 1/2', '10 3/4', '-1/2', '1/0', '1/32768', '1 1/0', '0/5', '1/2/3', "'texto", "'", "'123", "'=A1", '=', '==', '=1', '=1+2', '=1,5', '=1.5', '=SOMA(1;2)', '=SOMA(1;2', '=SOMA(', '=1+SOMA(1', '=(1+2', '=SOMA(ABS(1', '=SOMA()', '=SE(1)', '=AGORA(1)', '=HOJE()', '=A1', '=A1+B1', '=$A$1', '=Taxa*2', '=SOMA((1;2))', '=SOMA((A1;A2))', '=SOMA(A1:A5)', '=SE(A1>1;"s";"n")', '=1/0', '=@A1', '=A1#', '=FOO(1)', '=_xlfn.SINGLE(A1)', '=TEXTO(A1;"0,00")', '=SEQUÊNCIA(3)', '=ÚNICO(A1:A5)',
    '+A1', '-A1', '+1', '-', '+', '-abc', '+SOMA(1;2)', '- 5', '+ 5', '++5', '--5', 'texto', ' texto ', 'a b c', '1,5 abc', 'abc 1,5', '5 5', '1.5', '1,2,3', '١٢٣', '１２３', '٣', '', ' ', '  12  ', '0', '00', '-0', '1e400', '1e-400', '123456789012345678', '0,1234567890123456789', '12345678901234567890123', '9999999999999999999999999', '1,7976931348623157E308', '1E309'],
  en: ['42', '-42', '+42', '-0.5', '1.5', '.5', '5.', '50%', '1.5%', '(5)', '1,234', '1,234.5', '1,2345', '12,34,56', '1 234', '1E3', '1e-3', '1.5E+3', '$10', '$10.50', '$1,234.56', '10$', '€10', '10€', 'R$ 10',
    'TRUE', 'true', 'FALSE', 'False', ' TRUE ', 'VERDADEIRO', '#DIV/0!', '#div/0!', '#N/A', '#n/a', '#NUM!', '#NAME?', '#VALUE!', '#REF!', '#NULL!', '#GETTING_DATA', '#SPILL!', '#CALC!', '#N/D',
    '1/2/2024', '01/02/2024', '1/2', '12/31', '2024-01-02', '1-Jan-2024', '1-Jan', 'Jan/24', 'January 2024', 'Jan 1, 2024', 'Jan 1 2024', '2/29/2023', '2/29/2024', '13/1/2024', '10:30', '10:30:15', '10:30 PM', '25:00', '1:30.5', '10:30PM', '1/2/2024 10:30', '1/2/24 10:30:15',
    '1/2', '3/4', '1 1/2', '10 3/4', '-1/2', '1/0', '1/32768', "'text", "'", "'123", "'=A1", '=', '==', '=1', '=1+2', '=1,5', '=1.5', '=SUM(1,2)', '=SUM(1,2', '=SUM(', '=1+SUM(1', '=(1+2', '=SUM(ABS(1', '=SUM()', '=IF(1)', '=NOW(1)', '=TODAY()', '=A1', '=A1+B1', '=$A$1', '=Taxa*2', '=SUM((1,2))', '=SUM((A1,A2))', '=SUM(A1:A5)', '=IF(A1>1,"s","n")', '=1/0', '=@A1', '=A1#', '=FOO(1)', '=sum(1,2)', '=true', '=TRUE+1', '=SEQUENCE(3)', '=UNIQUE(A1:A5)', '=XLOOKUP(1,A1:A3,B1:B3)', '=TEXTJOIN(",",TRUE,A1:A3)', '=LET(x,1,x+1)', '=CONCAT(A1,B1)', '=SUM(A1:B2,C1)', '=ROUND(A1,2)', '=sum (1,2)', '=SUM(1,2)+', '=SUM(1,,2)', '=IF(,1,2)', '=A1:A3', '=Sheet2!A1', "='My Sheet'!A1+1", '=Total', '=Total+Taxa',
    '+A1', '-A1', '+1', '-', '+', '-abc', '+SUM(1,2)', '- 5', '++5', 'text', ' text ', 'a b c', '1.5 abc', '5 5', '1,5', '', ' ', '  12  ', '0', '-0', '1e400', '123456789012345678', '0.1234567890123456789', '9999999999999999999999999'],
};
{
  for (const lang of ['pt', 'en']) {
    const lines = [];
    for (const input of TYPED[lang]) for (const cat of ['general', 'percent', 'text', 'date', 'number', 'fraction']) lines.push(entryLine(lang, cat, 3, 2, false, input, input.includes('Taxa') || input.includes('Total') ? NAMES : undefined));
    for (const input of TYPED[lang].slice(0, 120)) lines.push(entryLine(lang, 'general', 0, 0, true, input));
    for (const cat of ['currency', 'scientific', 'time']) for (const input of TYPED[lang].slice(0, 90)) lines.push(entryLine(lang, cat, 1, 1, false, input));
    // seeded: decorated numbers, dates and fractions
    const r = new Rng(lang === 'pt' ? 0xbb67ae85 : 0x3c6ef372);
    const pk = (l) => l[r.below(l.length)];
    const dec = lang === 'pt' ? ',' : '.', grp = lang === 'pt' ? '.' : ',', cur = lang === 'pt' ? 'R$' : '$';
    for (let k = 0; k < 700; k++) {
      let s;
      const form = r.below(7);
      const int = String(r.below(1_000_000));
      const frac = r.below(3) === 0 ? '' : String(r.below(10000));
      const withGrp = r.below(3) === 0 ? int.replace(/\B(?=(\d{3})+(?!\d))/g, grp) : int;
      switch (form) {
        case 0: s = `${pk(['', '-', '+'])}${withGrp}${frac ? dec + frac : pk(['', dec])}${pk(['', '%', ' %', 'E3', 'e-2', 'E+10'])}`; break;
        case 1: s = `${pk(['', cur, `${cur} `, '€', '€ '])}${withGrp}${frac ? dec + frac : ''}${pk(['', '', cur, '€', ' €'])}`; break;
        case 2: s = `(${withGrp}${frac ? dec + frac : ''})`; break;
        case 3: s = `${r.below(32)}${pk(['/', '-', '.'])}${r.below(14)}${pk(['', '/2024', '/24', '-2024', '/1999'])}`; break;
        case 4: s = `${r.below(30)}:${String(r.below(70)).padStart(2, '0')}${pk(['', ':' + String(r.below(70)).padStart(2, '0'), ' PM', ' AM', 'am', ':30' + dec + '5'])}`; break;
        case 5: s = `${pk(['', '-', '1 ', '12 '])}${r.below(40)}/${pk([0, 1, 2, 3, 8, 32767, 32768])}`; break;
        default: s = `${pk(['1', '5', '12', '28'])} ${pk(lang === 'pt' ? ['jan', 'fev', 'mar', 'dez', 'janeiro', 'JAN'] : ['Jan', 'Feb', 'Mar', 'Dec', 'January', 'JAN'])} ${pk(['', '2024', '24', '1999'])}`;
      }
      lines.push(entryLine(lang, pk(['general', 'general', 'percent', 'number', 'date', 'time', 'fraction', 'text', 'scientific']), r.below(50), r.below(20), r.below(10) === 0, s.trimEnd()));
    }
    facet(`entry/${lang}`, 'lines', lines);
  }
}
{
  // formats of entry: category, apply rule, inference, the toolbar list
  const lines = [];
  const CODES2 = [...new Set([...CODES.filter((c) => c), '', 'General', 'GENERAL', 'Geral', '0', '0,00', '#.##0', 'R$ #.##0,00', '"R$"\\ #,##0.00', '[$€-2] #,##0.00', '[$-409]dddd', '[h]', '[hh]:mm', '[mm]', '[ss]', 'h:mm AM/PM', '[Red]0', '[Red]0;[Blue]0', '0.00;[Red]-0.00', '\\$0', '\\€0', 'mmm-yy', 'm', 'mm', 'yy', 'd', 'E+0', '0E+0', '0.0E-0', '0%;-0%', '@;@', '#?/?', '"abc"', '"abc"0', 'a', 'general', 'Generalx', 'xGeneral', '_-* #,##0_-', '*0', '\\a', '"a;b";0', '[Red]"x";0', '£0.00', '¥0', '"$"0', '[$USD]0', '[$-409]0', '[$$-409]0', '[$ -409]0', '[$-]0'])];
  for (const c of CODES2) lines.push(`cat ${h16(c)} ${entryFormat.formatCategory(c)}`);
  lines.push(`cat undefined ${entryFormat.formatCategory(undefined)}`);
  const HINTS = Object.entries(entryFormat.FMT).map(([k, v]) => [k, v]);
  for (const cur of CATS) for (const [k, h] of HINTS) lines.push(`apply ${cur} ${k} ${b(entryFormat.shouldApplyFormat(cur, h))}`);
  for (const sym of ['R$', '$', '€', 'A"B', '']) for (const dc of [false, true]) lines.push(`cur ${h16(sym)} ${b(dc)} ${fmtText(entryFormat.currencyHint(sym, dc))}`);
  const KINDS = ['general', 'number', 'comma', 'currency', 'accounting', 'shortDate', 'longDate', 'time', 'percent', 'percent0', 'fraction', 'scientific', 'text'];
  for (const k of KINDS) for (const lg of ['pt-BR', 'en-US']) { const s = numberFormats.numberFormatFor(k, lg); lines.push(`nf ${k} ${lg} ${s.id ?? '-'} ${h16(s.code)}`); }
  for (const c of [...CODES2, '0.0', '0.00', '#,##0.0', '0.0%', '0.00E+00', '0.0E+0', '0.0;[Red]0.0', '0.00_);(0.00)', '#', '#.#', '.0', '.00', '0.', '"a.b"0.0', '"a"@', '[Red]0.0;0', '0\\.0', '0_.0', '0.0*x', 'dd.mm', '0/0', '0.0 0/0'])
    for (const d of [1, -1]) lines.push(`dec ${h16(c)} ${d} ${h16(numberFormats.adjustDecimals(c, d))}`);
  // inferFormulaFormat over canonical formulas with a lookup by position
  const lookup = (sheet, row, col) => {
    if (sheet === 'Other') return { id: 14, code: 'dd/mm/yyyy' };
    const k = (row + col * 3) % 7;
    return [undefined, { id: 14, code: 'dd/mm/yyyy' }, { id: 9, code: '0%' }, { id: 4, code: '#,##0.00' }, { id: 0, code: 'General' }, { id: 22, code: 'dd/mm/yyyy hh:mm' }, { id: 8, code: '"$"#,##0.00_);[Red]("$"#,##0.00)' }][k];
  };
  const FORMULAS = ['A1', '-A1', 'A1+1', '(A1)', 'SUM(A1:A5)', 'AVERAGE(B1,C1)', 'MIN(A1,2)', 'MAX(1,B2)', 'ROUND(A1,2)', 'ROUND(1,A1)', 'A1*2', 'A1/B1', 'A1^2', 'A1&"x"', 'IF(A1>1,B1,C1)', 'VLOOKUP(1,A1:C5,2,FALSE)', 'INDEX(A1:A5,1)', 'ABS(A1)',
    'COUNT(A1:A9)', 'DATEDIF(A1,B1,"d")', 'TODAY()', 'NOW()', 'DATE(2020,1,1)', 'TIME(1,2,3)', 'A1+B1', 'A1-B1', 'A1:B5', '@A1', 'Other!A1', 'Other!A1-Other!B2', 'A1%', '-A1%', 'SUM(A1%)', 'SUM(1,2)', 'SUM(Other!A1:A3)', 'TODAY()-A1', 'A1+TODAY()', '(A1+B1)', '$A$1', 'R[1]C[1]', 'A$1+$B2', 'SUM(,A1)'];
  for (const f of FORMULAS) for (const [row, col] of [[0, 0], [3, 2], [9, 1]]) {
    let node;
    try { node = calc.parseFormula(f, { row, col }); } catch { lines.push(`infer ${h16(f)} ${row} ${col} syntax`); continue; }
    lines.push(`infer ${h16(f)} ${row} ${col} ${fmtText(entryFormat.inferFormulaFormat(node, row, col, lookup))}`);
  }
  facet('entry/format', 'lines', lines);
}
{
  const lines = [];
  const VALUES = [null, 0, 1, -1, 0.5, 0.1 + 0.2, 1 / 3, 1e20, 1e21, 1e-19, 1e-20, 1.23456789012346e-16, 123456789012345680, -2.5e-7, 12345.6789, 45000, 45000.75, 0.75, 59, 60, 61, 2958465, 2958466, 2958465.5, -1, 1e300, 5e-324, NaN, Infinity,
    true, false, 'text', "it's", '', ' x ', 'ç', calc.CalcError.of('#DIV/0!'), calc.CalcError.of('#N/A'), calc.CalcError.of('#SPILL!'), calc.CalcError.of('#NUM!'), 0.99999999999999, 99999999999999.9, 0.000001, 1234567890.12345678];
  for (const lang of ['pt', 'en']) for (const cat of CATS) for (const v of VALUES) for (const opts of [{}, { date1904: true }, { quotePrefix: true }]) {
    const tv = v === null ? '_' : typeof v === 'number' ? `n${C.f64hex(v)}` : typeof v === 'string' ? `s${h16(v)}` : typeof v === 'boolean' ? `b${b(v)}` : `e${v.code}`;
    let r;
    try { r = h16(editTextMod.editText(v, cat, LOCALES[lang], opts)); } catch (e) { r = `T ${C.oneLine(e?.message ?? e, 40)}`; }
    lines.push(`${lang} ${cat} ${tv} ${b(opts.date1904)} ${b(opts.quotePrefix)}\t${r}`);
  }
  for (const lang of ['pt', 'en']) for (const v of [0, 1.5, -2.25, 1e21, 123456789.123456789, 0.1, 1e-7, 100, 1e15, 123456789012345.6]) lines.push(`ned ${lang} ${C.f64hex(v)}\t${h16(editTextMod.numberEditText(v, LOCALES[lang].decimal))}`);
  facet('entry/edit', 'lines', lines);
  // autoClose
  const ac = [];
  for (const t of ['=SUM(1,2', '=SUM(1,2)', '=SUM(SUM(1', '=1+SUM(1', '=(1+2', '=SUM("a)"', '=SUM(\'a', '=SUM("a', '=  SUM(1', '=SUM', '=SUM(1,2))', '=ÁREA(1', '=_xlfn.IFS(1', '=a.b(1', '=1(', '=SUM((1', '=SUM(1,(2)', "=SUM('a''b'", '=SUM("a""b"'])
    ac.push(`${h16(t)} ${h16(entryMod.autoClose(t))}`);
  facet('entry/autoclose', 'lines', ac);
}

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
console.log(`sheet-grid: ${results.length} workbooks, ${side.length} sidecar lines in ${((performance.now() - t0) / 1000).toFixed(1)} s -> ${outPath}`);
