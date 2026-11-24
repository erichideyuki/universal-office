// Builds the two Node bundles the Sheet captures and the Sheet benchmark run the frozen TS component with:
//   dom.mjs    write/test-support/fake-dom.ts (the DOM the TS's own editing tests use), `vitest` -> vitest-shim.mjs
//   sheet.mjs  sheet/packages/sheet/src/universal-sheet.ts + sheet-core + calc + the shared packages from their
//              src (the aliases of the TS `build:demo` script), so `<universal-sheet>` runs headless in Node
// Both are written under <out> (default core/target/sheet-cap/harness, gitignored) with a stamp of the TS commit;
// an up-to-date bundle (same commit and same sources mtime) is reused.
//
//   node core/crates/uo-xlsx/js/harness/build-harness.mjs [--ts-root <dir>] [--out <dir>] [--force]
import { execFileSync } from 'node:child_process';
import { existsSync, mkdirSync, readFileSync, writeFileSync } from 'node:fs';
import { join, resolve } from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';
import { args, coreDir, tsCommit, tsRoot } from '../lib/common.mjs';

const here = fileURLToPath(new URL('.', import.meta.url));

/** Builds (or reuses) the bundles; returns { dir, dom, sheet } (absolute paths). */
export function buildHarness(root, out = join(coreDir, 'target', 'sheet-cap', 'harness'), force = false) {
  mkdirSync(out, { recursive: true });
  const commit = tsCommit(root);
  const stampFile = join(out, 'stamp.json');
  const stamp = JSON.stringify({ commit, root: root.replace(/\\/g, '/'), shim: readFileSync(join(here, 'vitest-shim.mjs'), 'utf8').length });
  const dom = join(out, 'dom.mjs');
  const sheet = join(out, 'sheet.mjs');
  if (!force && existsSync(stampFile) && readFileSync(stampFile, 'utf8') === stamp && existsSync(dom) && existsSync(sheet)) return { dir: out, dom, sheet, commit };
  const esbuild = join(root, 'sheet', 'node_modules', 'esbuild', 'bin', 'esbuild');
  const sp = join(root, 'sheet', 'packages');
  const alias = {
    '@universal-office/sheet-core': join(sp, 'sheet-core/src/index.ts'),
    '@universal-office/sheet-calc': join(sp, 'calc/src/index.ts'),
    '@universal-office/fonts': join(root, 'shared/fonts/src/index.ts'),
    '@universal-office/ui': join(root, 'shared/ui/src/index.ts'),
    '@universal-office/charts': join(root, 'shared/charts/src/index.ts'),
    '@universal-office/drawingml': join(root, 'shared/drawingml/src/index.ts'),
    '@universal-office/slides-core': join(root, 'slides/packages/slides-core/src/index.ts'),
    vitest: join(here, 'vitest-shim.mjs'),
  };
  const common = ['--bundle', '--format=esm', '--platform=node', '--log-level=warning', '--legal-comments=none',
    ...Object.entries(alias).map(([k, v]) => `--alias:${k}=${v}`)];
  const run = (entry, outfile) => execFileSync(process.execPath, [esbuild, entry, ...common, `--outfile=${outfile}`],
    { cwd: join(sp, 'sheet'), stdio: ['ignore', 'inherit', 'inherit'] });
  run(join(root, 'write/test-support/fake-dom.ts'), dom);
  // the element + the sheet-core / calc entry points the captures call directly (one module graph)
  const entry = join(out, 'sheet-entry.ts');
  writeFileSync(entry, [
    `export * from ${JSON.stringify(join(sp, 'sheet/src/universal-sheet.ts').replace(/\\/g, '/'))};`,
    `export * as sheetCore from ${JSON.stringify(join(sp, 'sheet-core/src/index.ts').replace(/\\/g, '/'))};`,
    `export * as calc from ${JSON.stringify(join(sp, 'calc/src/index.ts').replace(/\\/g, '/'))};`,
    `export * as sheetCalc from ${JSON.stringify(join(sp, 'sheet/src/sheet-calc.ts').replace(/\\/g, '/'))};`,
    `export * as calcIo from ${JSON.stringify(join(sp, 'sheet/src/calc-io.ts').replace(/\\/g, '/'))};`,
    '',
  ].join('\n'));
  run(entry, sheet);
  writeFileSync(stampFile, stamp);
  return { dir: out, dom, sheet, commit };
}

/**
 * Installs the fake DOM and loads the component bundle: { Sheet (the element class), sheetCore, calc, sheetCalc,
 * calcIo, dom, newSheet() }. Fonts: the component falls back to its measured GDI tables (no canvas text).
 */
export async function loadHarness(root, opts = {}) {
  const h = buildHarness(root, opts.out, opts.force);
  const domMod = await import(pathToFileURL(h.dom).href);
  const dom = domMod.installFakeDom();
  dom.customElements.define('u-ripple', class extends globalThis.HTMLElement {});
  const mod = await import(pathToFileURL(h.sheet).href);
  const newSheet = (attrs = {}) => {
    const el = new mod.UniversalSheetElement();
    for (const [k, v] of Object.entries(attrs)) el.setAttribute(k, v);
    dom.document.body.appendChild(el);
    return el;
  };
  return { ...mod, Sheet: mod.UniversalSheetElement, dom, newSheet, commit: h.commit };
}

if (import.meta.url === pathToFileURL(process.argv[1] ?? '').href) {
  const a = args();
  const h = buildHarness(tsRoot(a), a.opt('--out') ? resolve(a.opt('--out')) : undefined, a.flag('--force'));
  console.log(`harness: ${h.dom}, ${h.sheet} (TS ${h.commit})`);
}
