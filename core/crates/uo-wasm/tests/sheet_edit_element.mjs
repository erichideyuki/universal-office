// The editor harness of `<universal-sheet engine="rust">` (frontier C5b; the B8 style of uo-bindings.md): the WORKTREE's element
// (with the C5b routing) runs headless on the fake DOM of the TS editing tests, twice per script — `engine="ts"` and
// `engine="rust"` with `uo-sheet-editor.wasm` configured — and the scripts of `crates/uo-xlsx/js/capture-edit.mjs` (scenarios
// below, pt-BR and en-US) are driven through the element's PUBLIC API. After every step the two elements are compared:
// the saved package (every part decompressed: name + bytes), the store-relevant text of each sheet (`debugCellDisplays`),
// the selection, undo/redo availability and `isDirty()`; and the Rust element must have edited through the module (no
// fall back to the TS path). No browser, no window.
//
//   cargo build -p uo-wasm --no-default-features --features sheet,sheet-edit,count --target wasm32-unknown-unknown --profile wasm
//   UO_WASM_PATH=<that .wasm> [UO_ESBUILD=<esbuild bin>] node core/crates/uo-wasm/tests/sheet_edit_element.mjs [--only <substr>]
//
// esbuild comes from the main checkout when the worktree has no node_modules (UO_ESBUILD, else D:/Projects/OfficeNext/sheet/node_modules).
import { execFileSync } from 'node:child_process';
import { existsSync, mkdirSync, readFileSync, writeFileSync } from 'node:fs';
import { join, dirname, resolve } from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';
import { inflateRawSync } from 'node:zlib';

process.env.TZ = 'UTC';
const here = dirname(fileURLToPath(import.meta.url));
const core = resolve(here, '../../..');
const root = resolve(core, '..');
const out = join(core, 'target', 'c5b-harness');
const wasmPath = process.env.UO_WASM_PATH ?? join(core, 'target/c5b-wasm/uo-sheet-editor-count.wasm');
const argv = process.argv.slice(2);
const only = argv.includes('--only') ? argv[argv.indexOf('--only') + 1] : null;

// ---------------------------------------------------------------- bundle (the worktree element + the engine glue)
function bundle() {
  mkdirSync(out, { recursive: true });
  const candidates = [process.env.UO_ESBUILD, join(root, 'sheet/node_modules/esbuild/bin/esbuild'), 'D:/Projects/OfficeNext/sheet/node_modules/esbuild/bin/esbuild'].filter(Boolean);
  const esbuild = candidates.find((p) => existsSync(p));
  if (!esbuild) throw new Error('no esbuild (set UO_ESBUILD)');
  const sp = join(root, 'sheet/packages');
  const alias = {
    '@universal-office/sheet-core': join(sp, 'sheet-core/src/index.ts'),
    '@universal-office/sheet-calc': join(sp, 'calc/src/index.ts'),
    '@universal-office/fonts': join(root, 'shared/fonts/src/index.ts'),
    '@universal-office/ui': join(root, 'shared/ui/src/index.ts'),
    '@universal-office/charts': join(root, 'shared/charts/src/index.ts'),
    '@universal-office/drawingml': join(root, 'shared/drawingml/src/index.ts'),
    '@universal-office/slides-core': join(root, 'slides/packages/slides-core/src/index.ts'),
    '@universal-office/converter': join(root, 'converter/packages/converter/src/index.ts'),
    '@universal-office/docs-processing': join(root, 'converter/packages/docs-processing/src/index.ts'),
    '@universal-office/sheets-processing': join(root, 'converter/packages/sheets-processing/src/index.ts'),
    '@universal-office/slides-processing': join(root, 'converter/packages/slides-processing/src/index.ts'),
    '@universal-office/docx-core': join(root, 'write/packages/docx-core/src/index.ts'),
    '@universal-office/docx-layout': join(root, 'write/packages/docx-layout/src/index.ts'),
    '@universal-office/spell': join(root, 'shared/spell/src/index.ts'),
    '@universal-office/spell-sources': join(root, 'shared/spell/src/sources.ts'),
    '@universal-office/sheet-engine': join(core, 'crates/uo-wasm/js/sheet-engine.ts'),
    '@universal-office/sheet-editor-engine': join(core, 'crates/uo-wasm/js/sheet-editor-engine.ts'),
    vitest: join(core, 'crates/uo-xlsx/js/harness/vitest-shim.mjs'),
  };
  const common = ['--bundle', '--format=esm', '--platform=node', '--log-level=error', '--legal-comments=none', '--loader:.mjs=js',
    '--external:pdfjs-dist', '--external:pdfjs-dist/*',
    ...Object.entries(alias).map(([k, v]) => `--alias:${k}=${v}`)];
  const nodePaths = ['D:/Projects/OfficeNext/sheet/node_modules', 'D:/Projects/OfficeNext/write/node_modules', 'D:/Projects/OfficeNext/shared/ui/node_modules', 'D:/Projects/OfficeNext/shared/fonts/node_modules'].filter((p) => existsSync(p));
  const env = { ...process.env, NODE_PATH: nodePaths.join(process.platform === 'win32' ? ';' : ':') };
  const run = (entry, outfile) => execFileSync(process.execPath, [esbuild, entry, ...common, `--outfile=${outfile}`], { cwd: join(sp, 'sheet'), stdio: ['ignore', 'inherit', 'inherit'], env });
  run(join(root, 'write/test-support/fake-dom.ts'), join(out, 'dom.mjs'));
  run(join(sp, 'sheet/src/universal-sheet.ts'), join(out, 'sheet.mjs'));
  return { dom: join(out, 'dom.mjs'), sheet: join(out, 'sheet.mjs') };
}

// ---------------------------------------------------------------- the zip parts of a package (stored + deflate)
function parts(bytes) {
  const b = Buffer.from(bytes);
  let eocd = b.length - 22;
  while (eocd >= 0 && b.readUInt32LE(eocd) !== 0x06054b50) eocd--;
  const n = b.readUInt16LE(eocd + 10);
  let at = b.readUInt32LE(eocd + 16);
  const map = new Map();
  for (let i = 0; i < n; i++) {
    const method = b.readUInt16LE(at + 10), csize = b.readUInt32LE(at + 20), nlen = b.readUInt16LE(at + 28), xlen = b.readUInt16LE(at + 30), clen = b.readUInt16LE(at + 32), off = b.readUInt32LE(at + 42);
    const name = b.toString('utf8', at + 46, at + 46 + nlen);
    const lnlen = b.readUInt16LE(off + 26), lxlen = b.readUInt16LE(off + 28);
    const data = b.subarray(off + 30 + lnlen + lxlen, off + 30 + lnlen + lxlen + csize);
    map.set(name, method === 8 ? inflateRawSync(data) : Buffer.from(data));
    at += 46 + nlen + xlen + clen;
  }
  return map;
}

// ---------------------------------------------------------------- scripts (a subset of capture-edit.mjs, public API only)
const T = {
  'pt-BR': { num: '1,5', pct: '50%', date: '12/03/2024', text: 'texto ção', sum: '=SOMA(A1:A5)*2', seq: '=SEQUÊNCIA(3;2)', bad: '=SOMA(', today: '=HOJE()', find: 'ção' },
  'en-US': { num: '1.5', pct: '50%', date: '3/12/2024', text: 'texto ção', sum: '=SUM(A1:A5)*2', seq: '=SEQUENCE(3,2)', bad: '=SUM(', today: '=TODAY()', find: 'ção' },
};
const S = (id, wb, steps) => ({ id, wb, steps });
const sel = (el, r) => el.selectRange(r);
const scripts = [
  S('entry', 'x1/0000', (t) => [
    (el) => { sel(el, 'A1'); el.setActiveCellText(t.num); },
    (el) => { sel(el, 'B2'); el.setActiveCellText(t.text); },
    (el) => { sel(el, 'C3'); el.setActiveCellText(t.pct); },
    (el) => { sel(el, 'D4'); el.setActiveCellText(t.date); },
    (el) => { sel(el, 'D1'); el.setActiveCellText(t.sum); },
    (el) => { sel(el, 'E1'); el.setActiveCellText(t.seq); },
    (el) => { sel(el, 'D3'); el.setActiveCellText(t.bad); },
    (el) => { sel(el, 'D5'); el.setActiveCellText(t.today); },
    (el) => el.undo(), (el) => el.undo(), (el) => el.redo(),
  ]),
  S('format', 'x1/0017', () => [
    (el) => { sel(el, 'B2:C4'); el.setFont({ bold: true, size: 14, color: '#C00000' }); },
    (el) => { sel(el, 'D2:E3'); el.setFormat({ fill: '#FFFF00' }); },
    (el) => { sel(el, 'B2:C4'); el.setBorders('thickOutside', '#0000FF'); },
    (el) => { sel(el, 'B2:B4'); el.setNumberFormat('percent'); },
    (el) => el.adjustDecimals(1),
    (el) => { sel(el, 'C2:C3'); el.setAlignment({ horizontal: 'center', wrapText: true }); },
    (el) => { sel(el, 'B:B'); el.setFormat({ fill: '#DDEBF7' }); },
    (el) => { sel(el, '2:3'); el.setFont({ bold: true }); },
    (el) => el.undo(), (el) => el.redo(),
  ]),
  S('merge-fill-clip', 'x1/0020', (t) => [
    (el) => { sel(el, 'B2:C3'); el.merge('center'); },
    (el) => { sel(el, 'D2:F4'); el.merge('across'); },
    (el) => { sel(el, 'D2'); el.unmerge(); },
    (el) => { sel(el, 'A1'); el.setActiveCellText(t.sum); },
    (el) => { sel(el, 'A1:A5'); el.fill('down'); },
    (el) => { sel(el, 'A1:B2'); el.copy(); sel(el, 'D6'); el.paste(); },
    (el) => { sel(el, 'A9'); el.paste(`1\t${t.text}\r\n${t.date}\t${t.sum}\r\n`); },
    (el) => { sel(el, 'A1:C3'); el.clear('all'); },
    (el) => el.undo(), (el) => el.undo(), (el) => el.redo(),
  ]),
  S('find-view-size', 'x1/0005', (t) => [
    (el) => { sel(el, 'B2'); el.setActiveCellText(t.text); },
    (el) => { sel(el, 'A1'); el.find(t.find); },
    (el) => el.replaceAll(t.find, 'cao'),
    (el) => el.freezePanes(1, 1),
    (el) => { el.showGridlines = false; },
    (el) => { sel(el, 'B:C'); el.setColumnWidth(20); },
    (el) => { sel(el, '2:3'); el.setRowHeight(30); },
    (el) => { sel(el, '4:5'); el.setHidden('rows', true); },
    (el) => el.undo(), (el) => el.undo(), (el) => el.redo(),
  ]),
  S('sheets-autosum', 'samples/demo.xlsx', (t) => [
    (el) => { sel(el, 'A1:C4'); el.autoSum('SUM'); },
    (el) => { sel(el, 'A1'); el.setActiveCellText(t.num); },
    (el) => el.addSheet('Resumo'),
    (el) => { sel(el, 'A1'); el.setActiveCellText(t.pct); },
    (el) => el.recalculate(),
    (el) => el.undo(),
  ]),
];

// ---------------------------------------------------------------- run
const FIXED = Date.UTC(2025, 0, 15, 12, 0, 0);
const RealDate = Date;
globalThis.Date = class extends RealDate { constructor(...x) { if (x.length === 0) super(FIXED); else super(...x); } static now() { return FIXED; } };
const b = bundle();
const domMod = await import(pathToFileURL(b.dom).href);
const dom = domMod.installFakeDom();
dom.customElements.define('u-ripple', class extends globalThis.HTMLElement {});
const mod = await import(pathToFileURL(b.sheet).href);
const wasm = new Uint8Array(readFileSync(wasmPath));
mod.UniversalSheetElement.configureEngine({ wasm });
mod.UniversalSheetElement.configureEditor({ wasm });
const quiet = (f) => { const w = console.warn, e = console.error; const msgs = []; console.warn = console.error = (...a) => msgs.push(a.map(String).join(' ')); try { return [f(), msgs]; } finally { console.warn = w; console.error = e; } };
const readWb = (wb) => new Uint8Array(readFileSync(wb.startsWith('x1/') ? join(core, `crates/uo-xlsx/tests/captures/workbooks/x1/x1-${wb.slice(3)}.xlsx`) : join(root, 'sheet/samples', wb.slice(8))));

async function make(engine, bytes, lang) {
  const el = new mod.UniversalSheetElement();
  el.setAttribute('formula-locale', lang);
  el.setAttribute('engine', engine);
  dom.document.body.appendChild(el);
  quiet(() => el.loadXlsx(bytes));
  if (engine === 'rust') {
    for (let i = 0; i < 200 && !el.rustEditor; i++) await new Promise((r) => setTimeout(r, 5));
    if (!el.rustEditor) throw new Error('the Rust editor did not open');
  }
  return el;
}

const totals = { scripts: 0, steps: 0, compared: 0, equal: 0, fallbacks: 0, mismatches: [] };
for (const sc of scripts) {
  for (const lang of ['pt-BR', 'en-US']) {
    const key = `${sc.id}/${sc.wb}/${lang}`;
    if (only && !key.includes(only)) continue;
    totals.scripts++;
    const bytes = readWb(sc.wb);
    const ts = await make('ts', bytes, lang);
    const rs = await make('rust', bytes, lang);
    const steps = sc.steps(T[lang]);
    for (let k = 0; k < steps.length; k++) {
      totals.steps++;
      quiet(() => steps[k](ts));
      const [, warns] = quiet(() => steps[k](rs));
      if (!rs.rustEditor || warns.some((w) => w.includes('volta ao TS'))) { totals.fallbacks++; totals.mismatches.push(`${key} e${k}: fell back to TS (${warns.join(' | ').slice(0, 200)})`); break; }
      const facets = {
        sel: (el) => JSON.stringify(el.getSelection()),
        undo: (el) => `${el.canUndo()}/${el.canRedo()}/${el.isDirty()}`,
        sheets: (el) => el.getSheetNames().join('|'),
        // per cell (the order is the model's: the mirror reloaded from the Rust save has its rows sorted)
        displays: (el) => quiet(() => JSON.stringify(el.debugCellDisplays().map((d) => `${d.ref}	${d.display}`).sort()))[0],
      };
      for (const [name, f] of Object.entries(facets)) {
        totals.compared++;
        const a = f(ts), z = f(rs);
        if (a === z) totals.equal++;
        else {
          let at = 0;
          while (at < a.length && a[at] === z[at]) at++;
          totals.mismatches.push(`${key} e${k} ${name}: at ${at}: ts …${a.slice(Math.max(0, at - 60), at + 80)} | rust …${z.slice(Math.max(0, at - 60), at + 80)}`);
        }
      }
      const pa = parts(quiet(() => ts.saveXlsx())[0]), pz = parts(quiet(() => rs.saveXlsx())[0]);
      totals.compared++;
      const names = [...new Set([...pa.keys(), ...pz.keys()])];
      const diff = names.filter((n) => !pa.has(n) || !pz.has(n) || !pa.get(n).equals(pz.get(n)));
      if (diff.length === 0 && [...pa.keys()].join() === [...pz.keys()].join()) totals.equal++;
      else {
        const n0 = diff[0];
        const a = (pa.get(n0) ?? Buffer.alloc(0)).toString('utf8'), z = (pz.get(n0) ?? Buffer.alloc(0)).toString('utf8');
        let at = 0;
        while (at < a.length && a[at] === z[at]) at++;
        totals.mismatches.push(`${key} e${k} save: parts differ ${diff.join(' ')}; ${n0} at ${at}: ts …${a.slice(Math.max(0, at - 80), at + 80)} | rust …${z.slice(Math.max(0, at - 80), at + 80)}`);
      }
    }
    ts.remove(); rs.remove();
  }
}
// the fall back: a command of the module fails -> the TS engine edits from the Rust session's last state
if (!only) {
  const el = await make('rust', readWb('x1/0017'), 'en-US');
  quiet(() => { sel(el, 'A1'); el.setActiveCellText('rust-before'); });
  el.rustEditor.cmd = () => { throw new Error('injected failure'); };
  const [res, warns] = quiet(() => { sel(el, 'B1'); return el.setActiveCellText('ts-after'); });
  const saved = parts(quiet(() => el.saveXlsx())[0]);
  const sheet1 = [...saved].filter(([n]) => n.startsWith('xl/worksheets/')).map(([, d]) => d.toString('utf8')).join('');
  const ok = res.ok && el.rustEditor === null && warns.some((w) => w.includes('volta ao TS')) && sheet1.includes('rust-before') && sheet1.includes('ts-after');
  totals.compared++;
  if (ok) totals.equal++; else totals.mismatches.push(`fall back: ok=${res.ok} editor=${el.rustEditor} rust edit kept=${sheet1.includes('rust-before')} ts edit=${sheet1.includes('ts-after')}`);
  el.remove();
}
console.log(`sheet editor element: ${totals.scripts} scripts, ${totals.steps} steps, ${totals.compared} facets compared, ${totals.equal} equal, ${totals.fallbacks} fall-backs`);
for (const m of totals.mismatches.slice(0, 40)) console.log(`  ${m}`);
process.exitCode = totals.mismatches.length ? 1 : 0;
