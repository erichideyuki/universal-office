// The D2b-F3 editor harness (tables; record uo-docx.md "D2a — session (architecture)", "D2b frontier
// briefs", docs/modules/uo-docx-d2b/F3.md): the frozen `<universal-write>` element in headless
// Chromium (Playwright), driven through the public methods of the F3 commands (and the private
// entries the shell reaches: `enterCellAt`, `tableSelKey`, `tabNewRow`, `moveTableRow`,
// `clearSelectedCells`) mixed with the D2a ops (typing, Enter, Backspace, Delete, selection,
// tracking, undo, redo), one element per page load. Same set-up as the D2a harness (which this file
// copies: that script is shared and not imported, its top level runs the capture), plus the math
// override of G17 (`d2b-math-override.mjs`: the table formulas go through `Math.pow`).
//
//   UO_TS_ROOT=<built checkout of main> node core/crates/uo-docx/js/capture-d2b-f3-session.mjs [--docs N] [--steps N]
//
// Writes tests/parity/d2b-f3-session.capture: per `<doc>#<j>` the facets of the D2a harness (op,
// paras, sel, anc, ev, flags, docxml) plus
//   lay   what the TS reads from the layout for the op (`contentWidthTwips()` and the displayed height
//         of every row of the target table): the replay hands it to the command (the session never lays
//         out), as `<width>|<h0>,<h1>,…`;
//   tbl   the canonical digest of the tables of the state an undo would restore (`state.tb`, relinked);
//   ctx   `JSON.stringify(state.ce)` (the cell being edited and the cursor in it);
//   ts    `JSON.stringify(state.ts)` (the selection of cells).
import { createRequire } from 'node:module';
import { createServer } from 'node:http';
import { readFileSync, existsSync, readdirSync } from 'node:fs';
import { join, extname, normalize } from 'node:path';
import { createHash } from 'node:crypto';
import { CaptureWriter, Rng, Stream } from '../../uo-testkit/js/testkit.mjs';
const Digest = { of: (b) => ({ short: createHash('sha256').update(b).digest('hex').slice(0, 32) }) };
import { Canon } from './canon.mjs';
import { crate, corpus, readBytes, root, ts } from './capture-r2-common.mjs';
import { mathOverride } from './d2b-math-override.mjs';

const argv = process.argv.slice(2);
const opt = (k, d) => (argv.includes(k) ? Number(argv[argv.indexOf(k) + 1]) : d);
const DOCS = opt('--docs', 70);
const STEPS = opt('--steps', 70);
// `--dump <doc>#<step>`: prints the JSON of the tables of the restored state and stops
const DUMP = argv.includes('--dump') ? argv[argv.indexOf('--dump') + 1] : undefined;
const { DocxPackage } = await ts('package.js');
const { readFullDocument } = await ts('fulldoc.js');
const T = await ts('table-ops.js');

const pwFrom = [join(root, 'slides', 'package.json'), join(root, 'sheet', 'package.json')].find((p) => existsSync(p));
const { chromium } = createRequire(pwFrom)('playwright');

const MIME = { '.js': 'text/javascript', '.mjs': 'text/javascript', '.html': 'text/html', '.json': 'application/json', '.wasm': 'application/wasm', '.css': 'text/css', '.ttf': 'font/ttf', '.otf': 'font/otf', '.woff2': 'font/woff2' };
const server = createServer((req, res) => {
  const url = decodeURIComponent((req.url ?? '/').split('?')[0]);
  if (url === '/__d2a.html') {
    res.writeHead(200, { 'content-type': 'text/html' });
    res.end('<!doctype html><meta charset="utf-8"><body><script type="module">import "/write/packages/editor/publish/index.js"; window.__ready = true;</script></body>');
    return;
  }
  const file = normalize(join(root, url));
  if (!file.startsWith(normalize(root)) || !existsSync(file)) { res.writeHead(404); res.end(); return; }
  res.writeHead(200, { 'content-type': MIME[extname(file)] ?? 'application/octet-stream' });
  res.end(readFileSync(file));
});
await new Promise((r) => server.listen(0, '127.0.0.1', r));
const port = server.address().port;

const installed = (() => {
  const base = join(process.env.LOCALAPPDATA ?? '', 'ms-playwright');
  if (!existsSync(base)) return undefined;
  const dirs = readdirSync(base).filter((d) => d.startsWith('chromium_headless_shell-')).sort();
  const last = dirs[dirs.length - 1];
  const exe = last && join(base, last, 'chrome-headless-shell-win64', 'chrome-headless-shell.exe');
  return exe && existsSync(exe) ? exe : undefined;
})();
const browser = await chromium.launch({ headless: true, executablePath: process.env.UO_CHROMIUM ?? installed });
const context = await browser.newContext({ timezoneId: 'UTC', locale: 'pt-BR' });
const mo = await mathOverride();
await context.addInitScript(mo.initScript);
await context.addInitScript(() => {
  let s = 1;
  Math.random = () => { s ^= s << 13; s >>>= 0; s ^= s >>> 17; s ^= s << 5; s >>>= 0; return s / 4294967296; };
  const T0 = Date.UTC(2026, 0, 2, 3, 4, 0);
  const D = Date;
  // eslint-disable-next-line no-global-assign
  Date = class extends D { constructor(...a) { if (a.length) super(...a); else super(T0); } static now() { return T0; } };
});

const EVENTS = ['uw-change', 'uw-dirty-change', 'uw-input', 'uw-track-changes', 'uw-cellmode', 'uw-notemode', 'uw-hfmode', 'uw-tbmode', 'uw-protected', 'uw-blocked', 'uw-sort-error', 'uw-styles-change'];
const TEXTS = ['a', 'Olá ', ' ', '10', '5,5', '😀', 'abc def', 'R$ 3,00', '(2)', '12%', '1.234,5'];
const hex = (t) => [...Array(t.length).keys()].map((i) => t.charCodeAt(i).toString(16)).join('.');

const revive = (v) => (v && typeof v === 'object' ? (v.__u8 !== undefined ? Uint8Array.from(Buffer.from(v.__u8, 'base64')) : (Array.isArray(v) ? v.map(revive) : Object.fromEntries(Object.entries(v).map(([k, x]) => [k, revive(x)])))) : v);

function digest(paras) {
  const s = new Stream();
  const c = new Canon(s);
  paras.forEach((p, i) => { s.tag('E').u32(i); c.value(p, 'paragraph'); });
  return s.finish().short;
}
function tdigest(tables) {
  const s = new Stream();
  const c = new Canon(s);
  tables.forEach((t, i) => { s.tag('E').u32(i); c.value(t, 'table'); });
  return s.finish().short;
}

const out = new CaptureWriter([
  'D2b-F3 session in Chromium: the frozen <universal-write> driven by seeded scripts (tables mixed with typing, Enter, Backspace, Delete, selection, undo, redo, tracking), math override on',
  'facets: op, lay, paras, sel, anc, ev, flags, docxml, tbl, ctx, ts',
]);
out.meta('steps', STEPS);
out.meta('browser', `chromium ${browser.version()}`);
out.meta('bundle', Digest.of(readFileSync(join(root, 'write/packages/editor/publish/index.js'))).short);
out.meta('math-override', mo.meta);

// the argument tables of the F3 ops (the Rust replay has the same tables, by index)
const A = {
  SHIFT_INS: ['right', 'down', 'row', 'column'],
  SHIFT_DEL: ['left', 'up', 'row', 'column'],
  SPLITS: [[2, 1, true], [1, 2, true], [3, 1, false], [2, 2, true], [1, 1, true], [2, 1, false]],
  ROWH: [[0, 'atLeast'], [400, 'atLeast'], [600, 'exact']],
  COLW: [500, 1440, 3000],
  AF: ['content', 'window', 'fixed'],
  AH: ['left', 'center', 'right', 'both'],
  AV: ['top', 'center', 'bottom'],
  DIR: ['lrTb', 'tbRl', 'btLr'],
  MARGINS: [[{ top: 50, left: 70 }, undefined, undefined], [{}, 40, true], [{ left: 100, right: 100, top: 0, bottom: 20 }, 0, false], [{}, undefined, true]],
  SORT: [
    { keys: [{ column: 0, type: 'text' }] },
    { keys: [{ column: 1, type: 'number', desc: true }], header: true },
    { keys: [{ column: 0, type: 'date' }, { column: 1, type: 'text', desc: true }], caseSensitive: true },
    { keys: [{ column: 0, type: 'number' }], header: true, caseSensitive: false },
  ],
  SEP: ['tab', 'paragraph', 'semicolon', 'comma', { char: '|' }],
  X2T: [{ separator: 'tab' }, {}, { separator: 'paragraph', cols: 2 }, { separator: 'comma', autofit: 'content' }, { separator: 'semicolon', colWidthTwips: 1200, autofit: 'window' }],
  STYLES: ['Table Grid', 'Grid Table 4 - Accent 1', 'Plain Table 2', 'List Table 3 - Accent 2', 'No Such Style', null],
  STYLEOPTS: [{ headerRow: false }, { totalRow: true, bandedRows: false }, { firstColumn: false, lastColumn: true, bandedColumns: true }, { headerRow: true, bandedColumns: false }],
  SHADES: [
    ['FF0000'], ['#00ff00', { val: 'pct25', color: '0000FF' }], [{ themeColor: 'accent1', tint: '33' }], [{ themeColor: 'accent2', shade: 'BF' }, null, 'table'],
    [null], [null, { val: 'horzStripe' }, 'cell'], [{ themeColor: 'text2' }, { val: 'clear' }, 'cell'],
  ],
  BORDERSIDES: [
    [{ top: { val: 'single', sz: 8, space: 0, color: 'FF0000' }, bottom: null }, 'cell'],
    [{ left: { val: 'double', sz: 4, space: 0, color: 'auto' }, insideH: { val: 'single', sz: 4, space: 0, color: 'auto' } }, 'table'],
    [{ top: null, left: null, bottom: null, right: null, insideH: null, insideV: null }, 'table'],
    [{ right: { val: 'dashed', sz: 12, space: 0, color: '00FF00' }, insideV: null }, 'cell'],
  ],
  KINDS: ['bottom', 'top', 'left', 'right', 'none', 'all', 'outside', 'inside', 'insideH', 'insideV', 'tl2br', 'tr2bl'],
  PEN: [undefined, null, { val: 'dashed', sz: 8, space: 0, color: '0000FF' }],
  TPROPS: [
    { table: { width: { w: 5000, type: 'pct' }, align: 'center' } },
    { table: { width: null, indentTwips: 360 } },
    { table: { wrap: 'around', overlap: false, title: 'T', description: 'D' } },
    { table: { wrap: 'none', position: { leftFromText: 100, vertAnchor: 'page', tblpX: 500, tblpY: 700 } } },
    { row: { height: { twips: 500, rule: 'exact' }, allowBreak: false, header: true } },
    { row: { height: null, allowBreak: true, header: false } },
    { column: { width: { w: 25, type: 'pct' } } },
    { column: { width: { w: 2000, type: 'dxa' } } },
    { column: { width: null } },
    { cell: { width: { w: 1500, type: 'dxa' }, vAlign: 'center', margins: { top: 10, left: 20 }, wrap: false, fitText: true } },
    { cell: { width: { w: 3000, type: 'pct' }, vAlign: 'top', margins: null, wrap: true, fitText: false } },
    { table: { align: 'right', indentTwips: 100, width: { w: 6000, type: 'dxa' }, overlap: true } },
  ],
  FORMULAS: [[undefined, undefined], ['=SUM(LEFT)', undefined], ['=SUM(ABOVE)', '0,00'], ['=2^3^2', undefined], ['=ROUND(10/3;2)', '#.##0,00'], ['=1+', undefined], ['SUM', undefined], ['=A1+B1', '0%'], ['=10^-2', undefined]],
  SELKEYS: [['ArrowLeft', false], ['ArrowRight', true], ['ArrowUp', false], ['ArrowDown', true], ['ArrowRight', false], ['ArrowDown', false], ['Delete', false], ['Backspace', false], ['Escape', false]],
};
const NEEDS_LAYOUT = new Set(['it', 'sc', 'dsr', 'af', 'x2t', 'tp', 'rr']);

function nextOp(rng, L, tables, inCell, tg) {
  const k = rng.below(100);
  const pos = () => { const para = rng.below(L.length); return { para, offset: rng.below(L[para] + 1) }; };
  const pickTable = () => tables.length ? tables[rng.below(tables.length)] : null;
  if (k < 7) return `ty:${hex(TEXTS[rng.below(TEXTS.length)])}`;
  if (k < 9) return 'en';
  if (k < 11) return 'bs';
  if (k < 12) return 'de';
  if (k < 17) { const a = pos(); const f = rng.below(4) === 0 ? a : pos(); return `se:${a.para}:${a.offset}:${f.para}:${f.offset}`; }
  if (k < 18) return `tk:${rng.below(2)}`;
  if (k < 22) return 'un';
  if (k < 24) return 're';
  if (k < 34) {
    const t = pickTable();
    if (!t) return 'it:2:3:0:0';
    return `ec:${t.i}:${rng.below(t.rows)}:${rng.below(Math.max(1, t.cols))}`;
  }
  // a table op needs the cursor in a cell (or a selection of cells): most of the time, go there first
  if (!inCell && tables.length && rng.below(10) < 7) { const t = pickTable(); return `ec:${t.i}:${rng.below(t.rows)}:${rng.below(Math.max(1, t.cols))}`; }
  const ops = [
    () => `it:${1 + rng.below(4)}:${1 + rng.below(4)}:${rng.below(3)}:${[0, 0, 1500][rng.below(3)]}`,
    () => 'ra', () => 'rb', () => 'cl', () => 'cr',
    () => `ic:${rng.below(2)}`, () => `ic:${2 + rng.below(2)}`,
    () => 'dr', () => 'dc', () => 'dt', () => `dl:${rng.below(2)}`, () => `dl:${2 + rng.below(2)}`,
    () => 'mc', () => 'mc', () => `sc:${rng.below(A.SPLITS.length)}`, () => `sc:${rng.below(A.SPLITS.length)}`, () => 'st',
    () => `sx:${rng.below(4)}`, () => `sx:${rng.below(4)}`, () => `sx:${rng.below(4)}`,
    () => 'dsr', () => 'dsc', () => `rh:${rng.below(A.ROWH.length)}`, () => `cw:${rng.below(A.COLW.length)}`, () => `af:${rng.below(3)}`,
    () => `ca:${rng.below(4)}:${rng.below(3)}`, () => `td:${rng.below(3)}`, () => 'ctd', () => 'rp', () => `mg:${rng.below(A.MARGINS.length)}`,
    () => `so:${rng.below(A.SORT.length)}`, () => `so:${rng.below(A.SORT.length)}`, () => `t2t:${rng.below(A.SEP.length)}`, () => `x2t:${rng.below(A.X2T.length)}`,
    () => `ts:${rng.below(A.STYLES.length)}`, () => `tso:${rng.below(A.STYLEOPTS.length)}`,
    () => `sh:${rng.below(A.SHADES.length)}`, () => `sh:${rng.below(A.SHADES.length)}`,
    () => `bd:${rng.below(A.BORDERSIDES.length)}`,
    () => `bk:${rng.below(A.KINDS.length)}:${rng.below(A.PEN.length)}`, () => `bk:${rng.below(A.KINDS.length)}:${rng.below(A.PEN.length)}`, () => `bk:${rng.below(A.KINDS.length)}:0`,
    () => `tp:${rng.below(A.TPROPS.length)}`, () => `tp:${rng.below(A.TPROPS.length)}`,
    () => `fm:${rng.below(A.FORMULAS.length)}`, () => `fm:${rng.below(2)}`,
    () => 'tn', () => `mv:${rng.below(2) ? 1 : -1}`,
    () => `rc:${rng.below((tg?.cols ?? 3) + 1)}:${[-300, 0, 1, 250, 700][rng.below(5)]}`,
    () => `rr:${rng.below(Math.max(1, tg?.rows ?? 1))}:${[-100, 1, 90, 400][rng.below(4)]}`,
    () => 'cs', () => `sk:${rng.below(A.SELKEYS.length)}`, () => `sk:${rng.below(A.SELKEYS.length)}`,
  ];
  return ops[rng.below(ops.length)]();
}

const files = corpus().filter((_, i) => i % 3 === 1).filter((f) => {
  try { return readFullDocument(DocxPackage.load(readBytes(f.file)), {}).blocks.some((b) => b.type === 'table'); } catch { return false; }
}).slice(0, DOCS).filter((f) => !DUMP || DUMP.startsWith(`${f.rel}#`));
let n = 0;
for (const { rel, file } of files) {
  const page = await context.newPage();
  await page.goto(`http://127.0.0.1:${port}/__d2a.html`);
  await page.waitForFunction(() => window.__ready === true);
  const bytes = readBytes(file);
  const ok = await page.evaluate(async ([b64, EVENTS]) => {
    const el = document.createElement('universal-write');
    el.setAttribute('fonts', '/write/packages/editor/demo/fonts');
    const ready = new Promise((r) => el.addEventListener('uw-ready', r, { once: true }));
    document.body.append(el);
    await ready;
    window.__el = el;
    window.__ev = [];
    for (const t of EVENTS) el.addEventListener(t, (e) => { const d = e.detail ?? {}; window.__ev.push(`${t}:${d.dirty ?? d.active ?? d.on ?? ''}`); });
    try {
      el.author = 'A';
      el.loadDocx(Uint8Array.from(atob(b64), (c) => c.charCodeAt(0)), { protected: false });
      return !!el.fonts;
    } catch { return false; }
  }, [Buffer.from(bytes).toString('base64'), EVENTS]);
  if (!ok) { await page.close(); continue; }
  let seed = 0x5e55 ^ 0xf3f3 ^ 0x3333;
  for (let i = 0; i < rel.length; i++) seed = (Math.imul(seed, 31) + rel.charCodeAt(i)) >>> 0;
  const rng = new Rng(seed);
  const info = () => page.evaluate(() => {
    const el = window.__el;
    // the paragraph lengths of the story being edited, the body tables (anchor, rows, grid columns)
    const L = el.model.paragraphs.map((p) => p.runs.reduce((a, r) => a + r.text.length, 0));
    const tables = el.bodyState().tableAnchors.map((a, i) => ({ i, rows: a.table.rows.length, cols: a.table.gridColsTwips.length }));
    const tg = el.tableTarget();
    return { L, tables, inCell: !!el.cellEdit || !!el.tableSel, tg: tg ? { rows: tg.table.rows.length, cols: tg.table.gridColsTwips.length } : null };
  });
  for (let j = 0; j <= STEPS; j++) {
    const key = `${rel}#${j}`;
    let code = 'init';
    let lay;
    if (j > 0) {
      const { L, tables, inCell, tg } = await info();
      code = nextOp(rng, L, tables, inCell, tg);
      const name = code.split(':')[0];
      if (NEEDS_LAYOUT.has(name)) {
        lay = await page.evaluate(() => {
          const el = window.__el;
          const tg = el.tableTarget();
          const hs = tg ? el.rowHeights(tg.anchor, tg.path, 0, tg.table.rows.length - 1) : [];
          return `${el.contentWidthTwips()}|${hs.join(',')}`;
        });
      }
      const threw = await page.evaluate(async ([code, A]) => {
        const el = window.__el;
        window.__ev = [];
        try {
        const f = code.split(':');
        const clone = (x) => (x === undefined ? undefined : JSON.parse(JSON.stringify(x)));
        const key = (k, shift = false) => ({ key: k, shiftKey: shift, ctrlKey: false, altKey: false, preventDefault() {} });
        switch (f[0]) {
          case 'ty': el.type(String.fromCharCode(...f[1].split('.').map((h) => parseInt(h, 16)))); break;
          case 'en': el.pressEnter(); break;
          case 'bs': el.pressBackspace(); break;
          case 'de': el.pressDelete(); break;
          case 'se': el.setSelection({ para: +f[1], offset: +f[2] }, { para: +f[3], offset: +f[4] }); break;
          case 'tk': el.toggleTrackChanges(f[1] === '1'); break;
          case 'un': el.undo(); break;
          case 're': el.redo(); break;
          case 'ec': el.enterCellAt(+f[1], [], +f[2], +f[3], false, false); break;
          case 'it': el.insertTable(+f[1], +f[2], { autofit: [undefined, 'content', 'window'][+f[3]], colWidthTwips: +f[4] || undefined }); break;
          case 'ra': el.insertRowAbove(); break;
          case 'rb': el.insertRowBelow(); break;
          case 'cl': el.insertColumnLeft(); break;
          case 'cr': el.insertColumnRight(); break;
          case 'ic': el.insertCells(A.SHIFT_INS[+f[1]]); break;
          case 'dr': el.deleteRow(); break;
          case 'dc': el.deleteColumn(); break;
          case 'dt': el.deleteTable(); break;
          case 'dl': el.deleteCells(A.SHIFT_DEL[+f[1]]); break;
          case 'mc': el.mergeCells(); break;
          case 'sc': el.splitCell(...A.SPLITS[+f[1]]); break;
          case 'st': el.splitTable(); break;
          case 'sx': [() => el.selectCell(), () => el.selectRow(), () => el.selectColumn(), () => el.selectTable()][+f[1]](); break;
          case 'dsr': el.distributeRows(); break;
          case 'dsc': el.distributeColumns(); break;
          case 'rh': el.setRowHeight(...A.ROWH[+f[1]]); break;
          case 'cw': el.setColumnWidth(A.COLW[+f[1]]); break;
          case 'af': el.autoFitTable(A.AF[+f[1]]); break;
          case 'ca': el.setCellAlignment(A.AH[+f[1]], A.AV[+f[2]]); break;
          case 'td': el.setTextDirection(A.DIR[+f[1]]); break;
          case 'ctd': el.cycleTextDirection(); break;
          case 'rp': el.toggleRepeatHeaderRows(); break;
          case 'mg': el.setTableCellMargins(...A.MARGINS[+f[1]]); break;
          case 'so': el.sort(clone(A.SORT[+f[1]])); break;
          case 't2t': el.convertTableToText(clone(A.SEP[+f[1]])); break;
          case 'x2t': el.convertTextToTable(clone(A.X2T[+f[1]])); break;
          case 'ts': await el.setTableStyle(A.STYLES[+f[1]]); break;
          case 'tso': el.setTableStyleOptions(clone(A.STYLEOPTS[+f[1]])); break;
          case 'sh': el.setCellShading(...clone(A.SHADES[+f[1]])); break;
          case 'bd': el.setTableBorderSides(...clone(A.BORDERSIDES[+f[1]])); break;
          case 'bk': el.setCellBorders(A.KINDS[+f[1]], clone(A.PEN[+f[2]])); break;
          case 'tp': el.setTableProperties(clone(A.TPROPS[+f[1]])); break;
          case 'fm': el.insertFormula(...A.FORMULAS[+f[1]]); break;
          case 'tn': el.tabNewRow(); break;
          case 'mv': el.moveTableRow(+f[1]); break;
          case 'rc': el.resizeTableColumn(+f[1], +f[2]); break;
          case 'rr': el.resizeTableRow(+f[1], +f[2]); break;
          case 'cs': el.clearSelectedCells(); break;
          case 'sk': { const [k, s] = A.SELKEYS[+f[1]]; if (el.tableSel) el.tableSelKey(key(k, s)); break; }
        }
        return false;
        } catch (e) { return String(e).slice(0, 120); }
      }, [code, A]);
      // the TS threw (a table the editor cannot handle, a TypeError): nothing after it is comparable
      if (threw) { out.set(key, 'op', code); out.set(key, 'thr', threw.replace(/\s+/g, ' ')); break; }
    }
    out.set(key, 'op', code);
    if (lay !== undefined) out.set(key, 'lay', lay);
    const st = await page.evaluate(() => {
      const el = window.__el;
      const enc = (v) => (v instanceof Uint8Array ? { __u8: btoa(String.fromCharCode(...v)) } : v);
      const walk = (v) => (v && typeof v === 'object' && !(v instanceof Uint8Array) ? (Array.isArray(v) ? v.map(walk) : Object.fromEntries(Object.entries(v).map(([k, x]) => [k, walk(enc(x))]))) : enc(v));
      const state = el.parseState(el.stringifyState());
      return {
        paras: walk(state.m.paragraphs),
        tb: walk(state.tb ?? []),
        sel: JSON.stringify(state.s),
        anc: JSON.stringify(state.anc ?? null),
        ctx: JSON.stringify(state.ce ?? null),
        ts: JSON.stringify(state.ts ?? null),
        ev: window.__ev.join(' '),
        flags: `${el.isDirty() ? 1 : 0}${el.undoStack.length ? 1 : 0}${el.redoStack.length ? 1 : 0}:${el.editsSinceLoad}`,
      };
    });
    const tables = revive(st.tb).map((t) => T.relink(t));
    if (DUMP && DUMP === key) {
      console.log(JSON.stringify(tables, (k, v) => (v instanceof Uint8Array ? `#${v.length}` : v)));
      process.exit(0);
    }
    if (DUMP && DUMP.startsWith(`${key}:p`)) {
      const para = Number(DUMP.slice(key.length + 2));
      console.log(JSON.stringify(revive(st.paras)[para], (k, v) => (v instanceof Uint8Array ? `#${v.length}` : v)));
      process.exit(0);
    }
    out.set(key, 'paras', digest(revive(st.paras)));
    out.set(key, 'tbl', tdigest(tables));
    out.set(key, 'sel', st.sel);
    out.set(key, 'anc', st.anc);
    out.set(key, 'ctx', st.ctx);
    out.set(key, 'ts', st.ts);
    out.set(key, 'ev', st.ev || '-');
    out.set(key, 'flags', st.flags);
    if (j % 8 === 0 || j === STEPS) {
      const b64 = await page.evaluate(() => { try { const b = window.__el.saveDocx(); let s = ''; for (let i = 0; i < b.length; i += 0x8000) s += String.fromCharCode(...b.subarray(i, i + 0x8000)); return btoa(s); } catch { return null; } });
      let d = 'err';
      if (b64) {
        const pkg = DocxPackage.load(Uint8Array.from(Buffer.from(b64, 'base64')));
        d = Digest.of(Buffer.from(pkg.getPartText('word/document.xml') ?? '', 'utf8')).short;
        if (DUMP === `${key}:xml`) { console.log(pkg.getPartText('word/document.xml')); process.exit(0); }
      }
      out.set(key, 'docxml', d);
    }
  }
  await page.close();
  n++;
}
await browser.close();
server.close();
out.write(join(crate, 'tests', 'parity', 'd2b-f3-session.capture'));
console.log(`d2b-f3-session: ${n} documents x ${STEPS} steps`);
