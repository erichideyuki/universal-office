// Parity capture of the CONDITIONAL FORMATS (frontier C3b, record core/docs/modules/uo-xlsx.md "C3b — as built"): the
// frozen TS `CfEngine` (cond-format.ts) inside the real element, headless (the harness of capture-grid.mjs), its formula
// evaluator (cf-formula.ts) over per-parameter traps, and the icon / data bar painters (cf-icons.ts, paintDataBar)
// over a recording canvas. Deterministic: TZ = UTC, `Date` frozen at 2025-01-15 12:00 UTC (todaySerial 45672, NOW at
// noon), the open-recalc off (`shouldRecalcOnOpen = () => false`, as C3a): the engine reads the file's values.
//
//   node core/crates/uo-xlsx/js/capture-cf.mjs                          # -> tests/captures/sheet-cf.{capture,side.gz}
//   node core/crates/uo-xlsx/js/capture-cf.mjs --write-workbooks        # (re)writes tests/captures/workbooks/cf1/*.xlsx
//   node core/crates/uo-xlsx/js/capture-cf.mjs --big                    # local: cf-big (300 generated) + x1-big (400,
//        from core/target/sheet-cap/workbooks/x1-big) -> core/target/sheet-cap/cf-big.{capture,side.gz}
//
// Keys and facets (sidecar lines `<key>\t<facet>\t<text>`):
//   <workbook>     (`gen/x1/NNNN`, `gen/cf1/NNNN`, `samples/<file>`, `gen/cf-big/NNNN`, `gen/x1-big/NNNN`)
//     b<i>         the evaluated rectangle of sheet i: `c1 r1 c2 r2` (1-based; the union of the CF ranges clipped to
//                  the used area + 2 and to 40,000 cells)
//     n<i>         cells evaluated (every cell of the rectangle inside some CF range, row-major)
//     cf<i>        per cell with a result: `ref<TAB>canonical JSON of the CfResult` (fill as the model Fill, bar/icon)
//   formula        `evalFormula` over a fixed cell table (facet `table`) in three contexts: hand-picked traps, the
//                  generator's formulas, 3,000 seeded random formulas (token soup)
//   icons / bars   the recorded canvas calls of `paintCfIcon` (every set × index) and `paintDataBar`
process.env.TZ = 'UTC';
const { readFileSync, mkdirSync, writeFileSync, createWriteStream, existsSync } = await import('node:fs');
const { join, dirname } = await import('node:path');
const { createHash } = await import('node:crypto');
const { createGzip } = await import('node:zlib');
const { pathToFileURL } = await import('node:url');
const { execFileSync } = await import('node:child_process');
const { CaptureWriter, Rng } = await import('../../uo-testkit/js/testkit.mjs');
const { canonJson } = await import('../../uo-pptx/js/lib/canon-json.mjs');
const C = await import('./lib/common.mjs');
const { GROUPS, groupKey, workbookFile } = await import('./lib/gen-workbook.mjs');
const { CF_GROUPS, buildCfWorkbook, cfKey } = await import('./lib/gen-cf.mjs');
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
const outPath = a.opt('--out', BIG ? join(C.bigDir, 'cf-big.capture') : join(C.capturesDir, 'sheet-cf.capture'));
const cfDir = join(C.capturesDir, 'workbooks', 'cf1');

if (a.flag('--write-workbooks')) {
  mkdirSync(cfDir, { recursive: true });
  for (let i = 0; i < CF_GROUPS.cf1.count; i++) { const b = buildCfWorkbook('cf1', i); writeFileSync(join(cfDir, b.name), b.bytes); }
  console.log(`cf1: ${CF_GROUPS.cf1.count} workbooks -> ${cfDir}`);
  process.exit(0);
}

// ---------------------------------------------------------------- the bundle
function buildCfBundle() {
  const out = join(C.coreDir, 'target', 'sheet-cap', 'harness');
  const h = buildHarness(root, out);
  const file = join(out, 'cf.mjs');
  const stamp = join(out, 'cf.stamp');
  const sp = join(root, 'sheet', 'packages');
  const want = JSON.stringify({ commit: h.commit, v: 1 });
  if (existsSync(file) && existsSync(stamp) && readFileSync(stamp, 'utf8') === want) return { ...h, cf: file };
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
  const entry = join(out, 'cf-entry.ts');
  writeFileSync(entry, [
    `export * from ${JSON.stringify(p('sheet/src/universal-sheet.ts'))};`,
    `export * as cond from ${JSON.stringify(p('sheet/src/cond-format.ts'))};`,
    `export * as cff from ${JSON.stringify(p('sheet/src/cf-formula.ts'))};`,
    `export * as icons from ${JSON.stringify(p('sheet/src/cf-icons.ts'))};`,
    '',
  ].join('\n'));
  execFileSync(process.execPath, [esbuild, entry, '--bundle', '--format=esm', '--platform=node', '--log-level=warning', '--legal-comments=none',
    ...Object.entries(alias).map(([k, v]) => `--alias:${k}=${v}`), `--outfile=${file}`], { cwd: join(sp, 'sheet'), stdio: ['ignore', 'inherit', 'inherit'] });
  writeFileSync(stamp, want);
  return { ...h, cf: file };
}

const built = buildCfBundle();
const domMod = await import(pathToFileURL(built.dom).href);
const dom = domMod.installFakeDom();
dom.customElements.define('u-ripple', class extends globalThis.HTMLElement {});
const M = await import(pathToFileURL(built.cf).href);
const newSheet = (attrs = {}) => {
  const el = new M.UniversalSheetElement();
  for (const [k, v] of Object.entries(attrs)) el.setAttribute(k, v);
  dom.document.body.appendChild(el);
  return el;
};

const sha = (b) => createHash('sha256').update(b).digest('hex');
const dig = (s) => { const b = Buffer.from(s, 'utf8'); return `${b.length}:${sha(b).slice(0, 32)}`; };
const listDig = (lines) => dig(lines.join('\n'));
const side = [];
const w = new CaptureWriter(['Sheet conditional formats (frontier C3b): see core/crates/uo-xlsx/js/capture-cf.mjs']);
w.meta('ts-root-commit', built.commit);
w.meta('ts-root-state', C.tsClean(root));
w.meta('tz', process.env.TZ);
w.meta('clock', '2025-01-15T12:00:00Z (todaySerial 45672)');
const sx = (key, facet, text) => side.push(`${key}\t${facet}\t${text}`);
const facet = (key, name, lines) => { w.set(key, name, listDig(lines)); for (const l of lines) sx(key, name, l); };

/** The canonical JSON of a CfResult (`{}` for the empty object, `-` is never written: undefined has no line). */
function canonRes(res) {
  const o = {};
  for (const k of ['fill', 'fontColor', 'bold', 'italic', 'underline', 'strike', 'border', 'numFmtCode', 'numFmtId', 'icon', 'hideValue']) if (res[k] !== undefined) o[k] = res[k];
  if (res.bar) o.bar = { ...res.bar };
  return canonJson(o);
}

// ---------------------------------------------------------------- workbooks
const MAX_CELLS = 40000;
const results = [];
function cfFacets(key, bytes) {
  const el = newSheet({ 'formula-locale': 'pt-BR' });
  el.shouldRecalcOnOpen = () => false;
  try { el.loadXlsx(bytes); } catch (e) { w.set(key, 'open', `err:${C.oneLine(e?.message ?? e)}`); el.remove(); return; }
  w.set(key, 'open', 'ok');
  const views = el.views ?? [];
  let any = false;
  views.forEach((v, i) => {
    if (!v.condFormats.length) return;
    any = true;
    el.sheetIndex = i;
    const eng = el.cfEngine();
    const rects = v.condFormats.flatMap((f) => M.cond.parseSqref(f.sqref));
    if (!rects.length) { w.set(key, `n${i}`, '0'); return; }
    let maxR = 0, maxC = 0;
    for (const r of v.worksheet.rows) { maxR = Math.max(maxR, r.r); for (const c of r.cells) { try { maxC = Math.max(maxC, refCol(c.ref)); } catch { /* odd ref */ } } }
    const c1 = Math.min(...rects.map((r) => r.c1)), r1 = Math.min(...rects.map((r) => r.r1));
    let c2 = Math.min(Math.max(...rects.map((r) => r.c2)), maxC + 2), r2 = Math.min(Math.max(...rects.map((r) => r.r2)), maxR + 2);
    if (c2 < c1 || r2 < r1) { w.set(key, `b${i}`, `${c1} ${r1} ${c1 - 1} ${r1 - 1}`); w.set(key, `n${i}`, '0'); facet(key, `cf${i}`, []); return; }
    const width = c2 - c1 + 1;
    r2 = Math.min(r2, r1 + Math.max(1, Math.floor(MAX_CELLS / width)) - 1);
    w.set(key, `b${i}`, `${c1} ${r1} ${c2} ${r2}`);
    const lines = [];
    let n = 0;
    for (let r = r1; r <= r2; r++) for (let c = c1; c <= c2; c++) {
      if (!rects.some((q) => c >= q.c1 && c <= q.c2 && r >= q.r1 && r <= q.r2)) continue;
      n++;
      const res = eng.at(c, r);
      if (res !== undefined) lines.push(`${colLetters(c)}${r}\t${canonRes(res)}`);
    }
    w.set(key, `n${i}`, String(n));
    facet(key, `cf${i}`, lines);
  });
  w.set(key, 'cf', any ? 'yes' : 'no');
  el.remove();
}
function colLetters(col) { let s = ''; for (let c = col; c > 0; c = Math.floor((c - 1) / 26)) s = String.fromCharCode(65 + ((c - 1) % 26)) + s; return s; }
function refCol(rf) { const m = /([A-Z]+)(\d+)/.exec(rf); if (!m) return 0; let n = 0; for (const ch of m[1]) n = n * 26 + ch.charCodeAt(0) - 64; return n; }

const t0 = performance.now();
if (!BIG) {
  for (let i = 0; i < GROUPS.x1.count; i++) { const key = groupKey('x1', i); cfFacets(key, new Uint8Array(readFileSync(join(C.capturesDir, 'workbooks', 'x1', workbookFile('x1', i))))); results.push(key); }
  for (let i = 0; i < CF_GROUPS.cf1.count; i++) { const key = cfKey('cf1', i); const b = buildCfWorkbook('cf1', i); cfFacets(key, b.bytes); results.push(key); }
  for (const rel of C.trackedSamples()) { if (/giant/.test(rel)) continue; const key = `samples/${rel}`; cfFacets(key, C.readSample(rel)); results.push(key); }
} else {
  const bigCf = join(C.bigDir, 'workbooks', 'cf-big');
  mkdirSync(bigCf, { recursive: true });
  for (let i = 0; i < CF_GROUPS['cf-big'].count; i++) {
    const key = cfKey('cf-big', i);
    const b = buildCfWorkbook('cf-big', i);
    writeFileSync(join(bigCf, b.name), b.bytes);
    cfFacets(key, b.bytes);
    results.push(key);
  }
  const dir = a.opt('--workbooks', join(C.bigDir, 'workbooks', 'x1-big'));
  for (let i = 0; i < GROUPS['x1-big'].count; i++) { const key = groupKey('x1-big', i); cfFacets(key, new Uint8Array(readFileSync(join(dir, workbookFile('x1-big', i))))); results.push(key); }
}

// ---------------------------------------------------------------- formula traps (cf-formula.ts over a fixed table)
if (!BIG) {
  const F = M.cff;
  const E = (c) => new F.FErr(c);
  // the table: own sheet (undefined) and 'Outra'; values with traps
  const TABLE = [
    [undefined, 1, 1, 5], [undefined, 1, 2, 10], [undefined, 1, 3, -2.5], [undefined, 1, 4, 'alfa'], [undefined, 1, 5, 'ALFA'], [undefined, 1, 6, ''],
    [undefined, 1, 7, true], [undefined, 1, 8, E('#N/A')], [undefined, 1, 9, 0], [undefined, 1, 10, 1e21], [undefined, 1, 11, '12'], [undefined, 1, 12, ' 3,5 '],
    [undefined, 2, 1, 'Straße'], [undefined, 2, 2, 'İstanbul'], [undefined, 2, 3, '\ud800x'], [undefined, 2, 4, '😀ab'], [undefined, 2, 5, 'a*b?c'], [undefined, 2, 6, false],
    [undefined, 2, 7, E('#DIV/0!')], [undefined, 2, 8, 45672], [undefined, 2, 9, 0.1], [undefined, 2, 10, -0], [undefined, 2, 11, 'VERDADEIRO'], [undefined, 2, 12, '  '],
    [undefined, 3, 1, 3], [undefined, 3, 3, 7], [undefined, 3, 5, 'x'], [undefined, 4, 2, 2.675], [undefined, 4, 4, 1e-7], [undefined, 5, 5, 'TRUE'],
    [undefined, 16384, 1048576, 99], [undefined, 1, 1048576, 'last'],
    ['Outra', 1, 1, 50], ['Outra', 1, 2, 'o'], ['Outra', 2, 1, E('#N/A')],
  ];
  const tab = new Map();
  for (const [s, c, r, v] of TABLE) tab.set(`${s ?? ''}|${c}|${r}`, v);
  const known = (s) => s === undefined || s.toLowerCase() === 'outra';
  const ctxOf = (col, row, dc, dr, date1904) => ({
    cell: (s, c, r) => { if (!known(s)) return E('#REF!'); const v = tab.get(`${s === undefined ? '' : 'Outra'}|${c}|${r}`); return v === undefined ? null : v; },
    each: (s, c1, r1, c2, r2, fn) => {
      if (!known(s)) return;
      const pre = s === undefined ? '' : 'Outra';
      const cells = TABLE.filter(([ss, c, r]) => (ss ?? '') === pre && c >= c1 && c <= c2 && r >= r1 && r <= r2).sort((x, y) => x[2] - y[2] || x[1] - y[1]);
      for (const [, c, r, v] of cells) fn(v, c, r);
    },
    col, row, dc, dr, today: 45672, date1904,
  });
  const CTX = [ctxOf(3, 4, 2, 3, false), ctxOf(1, 1, 0, 0, false), ctxOf(16384, 1048576, 16383, 1048575, true)];
  const show = (x) => {
    if (x === undefined) return 'u';
    if (x === null) return 'z';
    if (typeof x === 'number') return `n${canonJson(x)}`;
    if (typeof x === 'string') return `s${JSON.stringify(x)}`;
    if (typeof x === 'boolean') return x ? 'b1' : 'b0';
    if (x instanceof F.FErr) return x.code === '#N/A' ? 'eNA' : 'ex';
    return '?';
  };
  const tl = TABLE.map(([s, c, r, v]) => `${s ?? ''}\t${c}\t${r}\t${show(v)}`);
  facet('formula', 'table', tl);
  const TRAPS = [
    '1+1', '=1+1', '-2^2', '2^-1', '10/0', '0/0', '1e308*10', '2^1024', '(-8)^(1/3)', '1e21&""', '0.1+0.2', '1/3&""', '-0', '-A1*0&""', '5%', '50%%', '"a"%',
    'A1', '$A$1', 'A$1', '$A1', 'B2', 'XFD1048576', 'ZZZ1', 'A0', 'A1:A3', 'A:A', '1:1', '$1:$2', 'A1:B2:C3', 'Outra!A1', "'Outra'!A1", 'outra!A2', 'Nope!A1', 'Outra!A:A',
    'SUM(Outra!A1:B2)', 'SUM(A1:A12)', 'SUM(A:A)', 'SUM(1:1)', 'AVERAGE(A1:A3)', 'AVERAGE(B1:B1)', 'MAX(A1:B12)', 'MIN(A1:B12)', 'MAX()', 'MEDIAN(A1:A3,4)', 'MEDIAN()',
    'LARGE(A1:C12,1)', 'LARGE(A1:C12,2.5)', 'SMALL(A1:C12,0)', 'SMALL(A1:C12,99)', 'COUNT(A1:C12)', 'COUNTA(A1:C12)', 'COUNTBLANK(A1:C12)', 'COUNTBLANK(A1)',
    'COUNTIF(A1:B12,">0")', 'COUNTIF(A1:B12,"alfa")', 'COUNTIF(A1:B12,"a*")', 'COUNTIF(A1:B12,"<>")', 'COUNTIF(A1:B12,"")', 'COUNTIF(A1:B12,"=")', 'COUNTIF(A1:B12,"12")',
    'COUNTIF(A1:B12,12)', 'COUNTIF(A1:B12,TRUE)', 'COUNTIF(A1:B12,"<b")', 'COUNTIF(A1:B12,">=5")', 'COUNTIF(A1:B12,"~*")', 'COUNTIF(A1:B12,"a~*b?c")', 'COUNTIF(A1,"5")', 'COUNTIF(C1,5)',
    'COUNTIFS(A1:A12,">0",B1:B12,"<>")', 'COUNTIF(A1:A12,">0",B1:B12,"<>")', 'SUMIF(A1:A12,">0")', 'SUMIF(A1:A12,">0",C1:C12)', 'SUMIF(A1,">0",B1)', 'SUMIF(A1:A3,#N/A)',
    'MATCH(10,A1:A12,0)', 'MATCH("ALFA",A1:A12,0)', 'MATCH(1,A1:C1,0)', 'MATCH(1,A1)', 'VLOOKUP(10,A1:B12,2,FALSE)', 'VLOOKUP("alfa",A1:C12,1.5)', 'VLOOKUP(5,A1:B12)',
    'HLOOKUP(5,A1:C3,3,FALSE)', 'HLOOKUP(9,A1:C3,2)', 'ROW()', 'ROW(A5)', 'ROW(A5:B6)', 'COLUMN()', 'COLUMN(C1:D1)', 'ROWS(A1:A9)', 'ROWS(A1)', 'COLUMNS(A:C)', 'ROWS()',
    'MOD(10,3)', 'MOD(-10,3)', 'MOD(10,-3)', 'MOD(1,0)', 'MOD(5.5,2)', 'ABS(-0)', 'INT(-1.5)', 'TRUNC(-1.5)', 'SIGN(-0)', 'SIGN(A2)', 'SQRT(-1)', 'SQRT(2)', 'POWER(0,0)', 'POWER(-8,1/3)',
    'ROUND(2.675,2)', 'ROUND(-2.5,0)', 'ROUND(1234.5,-2)', 'ROUNDUP(1.001,1)', 'ROUNDDOWN(-1.99,0)', 'ROUND(1e21,2)', 'ROUND(A1)', 'ROUND(0.1+0.2,15)',
    'LEN(B1)', 'LEN(B4)', 'LEN(B3)', 'LEN(A1)', 'LEN(A10)', 'LEFT(B1)', 'LEFT(B1,3)', 'LEFT(B1,-1)', 'LEFT(B1,1/0)', 'LEFT(B4,1)', 'RIGHT(B1,2)', 'RIGHT(B1,0)', 'RIGHT(B1,99)',
    'MID(B1,2,3)', 'MID(B1,0,1)', 'MID(B1,1.9,2)', 'MID(B1,99,2)', 'MID(B1,2,-1)', 'UPPER(B1)', 'LOWER(B2)', 'UPPER(B3)', 'LOWER("ΣΑΣ")', 'TRIM("  a  b  ")', 'TRIM(B12)',
    'CLEAN("a"&CHAR(9))', 'EXACT(A4,A5)', 'EXACT("a","a")', 'SEARCH("lf",A4)', 'SEARCH("L?A",A5)', 'SEARCH("*",A4)', 'SEARCH("",A4)', 'SEARCH("a",A4,3)', 'SEARCH("a",A4,0)',
    'SEARCH("a",A4,-1)', 'SEARCH("a",A4,1.5)', 'SEARCH("~?",B5)', 'SEARCH("b?c",B5)', 'SEARCH("ss",B1)', 'SEARCH("x",A4)', 'FIND("a",A4)', 'FIND("A",A4)', 'FIND("",A4,99)',
    'FIND("a",A4,0)', 'SUBSTITUTE(A4,"a","o")', 'SUBSTITUTE(A4,"","o")', 'SUBSTITUTE("aaa","aa","b")', 'CONCATENATE(A1,A4,A7)', 'CONCAT(A1:B2)', 'CONCATENATE(A8)',
    'VALUE(A11)', 'VALUE(A12)', 'VALUE("1.5")', 'VALUE("1,5")', 'VALUE("")', 'VALUE(TRUE)', 'N(A7)', 'N(A4)', 'N(A8)', 'T(A4)', 'T(A1)', 'T(A8)',
    'TEXT(A1,"0.00")', 'TEXT(B8,"dd/mm/yyyy")', 'TEXT(A4,"0")', 'TEXT(A8,"0")', 'TEXT(1234.5,"#,##0.0")', 'TEXT(0.5,"0%")', 'TEXT(A1,"")', 'TEXT(-1,"0;(0)")',
    'TODAY()', 'NOW()', 'DATE(2025,1,15)', 'DATE(25,1,15)', 'DATE(-1850,1,1)', 'DATE(2025,13,1)', 'DATE(2025,1,0)', 'DATE(2025.9,1.9,1.9)', 'DATE(1e7,1,1)', 'DATE(A4,1,1)',
    'YEAR(B8)', 'MONTH(B8)', 'DAY(B8)', 'WEEKDAY(B8)', 'WEEKDAY(B8,2)', 'WEEKDAY(B8,3)', 'WEEKDAY(B8,"2")', 'YEAR(1e10)', 'MONTH(-1)', 'DAY(0)', 'YEAR(60)', 'DAY(61)',
    'EOMONTH(B8,0)', 'EOMONTH(B8,1.7)', 'EOMONTH(B8,-13)', 'EDATE(45657,1)', 'EDATE(45688,1)', 'EDATE(B8,-1)', 'EOMONTH(1e12,0)', 'EDATE("x",1)',
    'AND(A1,A7)', 'AND(A4)', 'AND()', 'OR(A9,A6)', 'OR(A8,TRUE)', 'XOR(TRUE,TRUE,TRUE)', 'XOR(A4)', 'XOR(A6)', 'NOT(A11)', 'NOT(B11)', 'NOT("x")', 'IF(A1>1,"y","n")', 'IF(A9,"y")',
    'IF(A9,,)', 'IF(1,)', 'IF(A8,1,2)', 'IF("TRUE",1,2)', 'IFERROR(A8,"e")', 'IFERROR(1/0,)', 'IFNA(A8,1)', 'IFNA(B7,1)', 'ISBLANK(C2)', 'ISBLANK(A6)', 'ISNUMBER(A11)', 'ISTEXT(A6)',
    'ISNONTEXT(C2)', 'ISLOGICAL(A7)', 'ISERROR(B7)', 'ISERR(A8)', 'ISERR(B7)', 'ISNA(A8)', 'ISEVEN(-3)', 'ISODD(2.9)', 'ISEVEN(A4)', 'ISODD(1/0)', 'NA()', 'TRUE()', 'FALSE()',
    'TRUE', 'false', 'True+1', 'A1=A5', 'A4=A5', 'A4<A5', '"a"<"B"', '"b"<"B"', '1<"a"', '"a"<TRUE', 'C2=0', 'C2=""', 'C2=FALSE', 'C2<1', 'A8=1', 'A6=0', 'A4>A1', 'B6<0',
    '"x"&TRUE', '"x"&A8', 'A1&A3', '1&1', '"a""b"', '"unterminated', '#n/a', '#NAME?', '#VALUE!+1', '#GETTING_DATA', '#FOO!', '#REF!=1',
    'foo', 'foo()', 'INDIRECT("A1")', 'OFFSET(A1,1,1)', '_xlfn.CONCAT(A4,A5)', '_xlws.FILTER(A1)', 'SUM(1;2;3)', 'SUM(1,,2)', 'SUM(,1)', 'LEN(,)', 'IF(",",1)', 'IF(")")',
    'A1 + 1', 'A1\t*2', ' 1 ', '(1', '1)', '1 2', '+', '-', '()', '', '=', '==1', '1+*2', '{1,2}', 'A1:', ':A1', 'A1!', '!A1', 'Sheet 1!A1', "'a''b'!A1", "''!A1",
    '1E3', '1e+3', '1e', '.5', '5.', '1.2.3', '0x10', '1,5', '1;5', 'A1;A2', 'R1C1', 'R[1]C[1]', 'A1.5', 'LOG10(10)', 'A1(', 'A1 (1)', 'SUM (1)', 'ß', 'Ä1', 'ÀB1',
    'A1>=1E3', '.5<A1', 'A1 > 2', '-A1%', '--A1', '+-+A1', '2^3^2', '-2^0.5', '(-2)^0.5', '1-1-1', '8/2/2', '"10">9', 'TEXT(1e21,"0")', 'TEXT(1e-7,"General")',
    'LEN(1e21)', 'LEN(0.1+0.2)', 'LEN(-0)', 'LEN(1/3)', 'LEN(123456789012)', 'UPPER(1.5)', 'A1+"3,5"', 'A1+" 3,5 "', 'A1+"1e3"', 'A1+"abc"', 'A1+""', 'A1+TRUE',
  ];
  const forms = [...TRAPS];
  // seeded token soup
  const R = new Rng(0x6c3b9a2f);
  const pk = (l) => l[R.below(l.length)];
  const ATOMS = ['1', '0', '-1', '2.5', '1e3', '.5', '1e400', '"a"', '"A"', '""', '"1,5"', '"x""y"', 'TRUE', 'FALSE', '#N/A', '#DIV/0!', 'A1', 'B2', '$A$1', 'A$2', '$C3', 'A1:A3',
    'A:A', '1:2', 'A1:C12', 'B1:B12', 'Outra!A1', "'Outra'!A1:B2", 'X!A1', 'XFD1', 'A1048576', 'foo', 'C2'];
  const FNS = ['SUM', 'AVERAGE', 'MAX', 'MIN', 'MEDIAN', 'LARGE', 'SMALL', 'COUNT', 'COUNTA', 'COUNTBLANK', 'COUNTIF', 'COUNTIFS', 'SUMIF', 'MATCH', 'VLOOKUP', 'HLOOKUP',
    'AND', 'OR', 'NOT', 'XOR', 'IF', 'IFERROR', 'IFNA', 'ISBLANK', 'ISNUMBER', 'ISTEXT', 'ISNONTEXT', 'ISLOGICAL', 'ISERROR', 'ISERR', 'ISNA', 'ISEVEN', 'ISODD', 'ROW', 'COLUMN',
    'ROWS', 'COLUMNS', 'MOD', 'ABS', 'INT', 'TRUNC', 'SIGN', 'SQRT', 'POWER', 'ROUND', 'ROUNDUP', 'ROUNDDOWN', 'LEN', 'LEFT', 'RIGHT', 'MID', 'UPPER', 'LOWER', 'TRIM', 'CLEAN',
    'EXACT', 'SEARCH', 'FIND', 'SUBSTITUTE', 'CONCATENATE', 'CONCAT', 'VALUE', 'N', 'T', 'TEXT', 'TODAY', 'NOW', 'DATE', 'YEAR', 'MONTH', 'DAY', 'WEEKDAY', 'EOMONTH', 'EDATE', 'NA', 'BOGUS'];
  const OPS = ['+', '-', '*', '/', '^', '&', '=', '<>', '<', '>', '<=', '>=', ',', ';', ':'];
  const gen = (d) => {
    const k = R.below(d > 3 ? 3 : 8);
    if (k <= 2) return pk(ATOMS);
    if (k === 3) return `${gen(d + 1)}${pk(OPS)}${gen(d + 1)}`;
    if (k === 4) return `(${gen(d + 1)})`;
    if (k === 5) return `${pk(['-', '+', ''])}${gen(d + 1)}${pk(['%', '', ''])}`;
    const n = R.below(4);
    return `${pk(FNS)}(${Array.from({ length: n }, () => (R.below(10) === 0 ? '' : gen(d + 1))).join(pk([',', ';']))})`;
  };
  for (let k = 0; k < 3000; k++) {
    let f = gen(0);
    if (R.below(25) === 0) { const at = R.below(f.length + 1); f = f.slice(0, at) + pk(['(', ')', '"', '!', '#', '$', ' ', '@', '{']) + f.slice(at); }
    forms.push(f);
  }
  const lines = [];
  for (const f of forms) {
    const out = CTX.map((cx) => { try { return show(F.evalFormula(f, cx)); } catch (e) { return `T${C.oneLine(e?.message ?? e, 30)}`; } });
    lines.push(`${JSON.stringify(f)}\t${out.join(' ')}`);
  }
  facet('formula', 'eval', lines);
}

// ---------------------------------------------------------------- icons and data bars over a recording canvas
if (!BIG) {
  const num = (x) => String(x);
  class P2 { constructor() { this.ops = []; } moveTo(x, y) { this.ops.push(`M ${num(x)} ${num(y)}`); } lineTo(x, y) { this.ops.push(`L ${num(x)} ${num(y)}`); }
    arc(x, y, r, a0, a1) { this.ops.push(`A ${num(x)} ${num(y)} ${num(r)} ${num(a0)} ${num(a1)}`); } rect(x, y, w2, h) { this.ops.push(`R ${num(x)} ${num(y)} ${num(w2)} ${num(h)}`); }
    roundRect(x, y, w2, h, r) { this.ops.push(`RR ${num(x)} ${num(y)} ${num(w2)} ${num(h)} ${num(r)}`); } closePath() { this.ops.push('Z'); } }
  globalThis.Path2D = P2;
  const rec = () => {
    const out = [];
    let cur = new P2();
    const style = (s) => (typeof s === 'string' ? s : `G ${num(s.x0)} ${num(s.x1)} ${s.stops.join(' ')}`);
    const ctx = {
      fillStyle: '#000', strokeStyle: '#000',
      set lineWidth(v) { out.push(`LW ${num(v)}`); }, set lineJoin(v) { out.push(`LJ ${v}`); }, set lineCap(v) { out.push(`LC ${v}`); },
      save() { out.push('save'); }, restore() { out.push('restore'); }, translate(x, y) { out.push(`T ${num(x)} ${num(y)}`); }, scale(x, y) { out.push(`S ${num(x)} ${num(y)}`); },
      beginPath() { cur = new P2(); }, moveTo(x, y) { cur.moveTo(x, y); }, lineTo(x, y) { cur.lineTo(x, y); }, rect(x, y, w2, h) { cur.rect(x, y, w2, h); },
      fill(p) { out.push(`F ${style(this.fillStyle)} [${(p ?? cur).ops.join(';')}]`); }, stroke(p) { out.push(`K ${style(this.strokeStyle)} [${(p ?? cur).ops.join(';')}]`); },
      clip() { out.push(`CLIP [${cur.ops.join(';')}]`); },
      fillRect(x, y, w2, h) { out.push(`FR ${style(this.fillStyle)} ${num(x)} ${num(y)} ${num(w2)} ${num(h)}`); },
      strokeRect(x, y, w2, h) { out.push(`KR ${style(this.strokeStyle)} ${num(x)} ${num(y)} ${num(w2)} ${num(h)}`); },
      createLinearGradient(x0, y0, x1) { const g = { x0, x1, stops: [], addColorStop(_o, c) { this.stops.push(c); } }; return g; },
    };
    return { ctx, out };
  };
  const SETS = ['3Arrows', '3ArrowsGray', '3Flags', '3TrafficLights1', '3TrafficLights2', '3Signs', '3Symbols', '3Symbols2', '3Stars', '3Triangles', '4Arrows', '4ArrowsGray',
    '4RedToBlack', '4Rating', '4TrafficLights', '5Arrows', '5ArrowsGray', '5Rating', '5Quarters', '5Boxes', 'NoIcons', 'Bogus'];
  const il = [];
  for (const s of SETS) for (const idx of [-1, 0, 1, 2, 3, 4, 7, NaN]) for (const [x, y, sz] of [[10, 20, 16], [3.5, 7.25, 20]]) {
    const { ctx, out } = rec();
    M.icons.paintCfIcon(ctx, s, idx, x, y, sz);
    il.push(`${s} ${num(idx)} ${num(x)} ${num(y)} ${num(sz)}\t${out.join(' | ')}`);
  }
  facet('icons', 'ops', il);
  const bl = [];
  const BARS = [
    { from: 0, to: 0.5, color: '#638EC6', gradient: true, legacy: true, negative: false },
    { from: 0, to: 1, color: '#FF555A', gradient: false, border: '#FF555A', legacy: false, negative: false },
    { from: 0.3, to: 0.6, color: '#63C384', gradient: true, axis: 0.3, axisColor: '#000000', legacy: false, negative: false },
    { from: 0.1, to: 0.3, color: '#FF0000', gradient: true, border: '#FF0000', axis: 0.3, axisColor: '#7030A0', legacy: false, negative: true },
    { from: 0, to: 0.1, color: 'zz', gradient: true, legacy: true, negative: false },
    { from: 0.5, to: 0.5, color: '#123456', gradient: false, axis: 0.5, axisColor: '#000000', legacy: false, negative: false },
  ];
  for (const b of BARS) for (const [x, y, w2, h] of [[0, 0, 64, 20], [10.4, 30.6, 100.5, 15.5], [5, 5, 3, 3], [0, 0, 2, 40], [7, -3, 200, 4]]) {
    const { ctx, out } = rec();
    M.cond.paintDataBar(ctx, b, x, y, w2, h);
    bl.push(`${canonJson(b)} ${num(x)} ${num(y)} ${num(w2)} ${num(h)}\t${out.join(' | ')}`);
  }
  facet('bars', 'ops', bl);
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
console.log(`sheet-cf: ${results.length} workbooks, ${side.length} sidecar lines in ${((performance.now() - t0) / 1000).toFixed(1)} s -> ${outPath}`);
