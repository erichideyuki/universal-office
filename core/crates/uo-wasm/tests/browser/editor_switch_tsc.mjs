// The editor's and the viewer's own tsc configs without a node_modules in the worktree (record uo-bindings.md "Milestone 2:
// the editor switch", verification (b)): each package's tsconfig.json extended with path mappings for the npm dependencies of
// the frozen TS root (UO_TS_ROOT) and the internal packages by source, written to $CARGO_TARGET_DIR/es/ (outside git), then
// `tsc -p` of the frozen toolchain. `npm install` in the worktree is not used: it would create junctions.
//   UO_TS_ROOT=<frozen root> CARGO_TARGET_DIR=<target> node core/crates/uo-wasm/tests/browser/editor_switch_tsc.mjs
// Exit 0 and no output from tsc for both packages is the pass.
import { mkdirSync, writeFileSync } from 'node:fs';
import { join, dirname, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { spawnSync } from 'node:child_process';

const here = dirname(fileURLToPath(import.meta.url));
const W = resolve(here, '../../../../..').replace(/\\/g, '/');
const R = (process.env.UO_TS_ROOT ?? '').replace(/\\/g, '/');
if (!R) throw new Error('UO_TS_ROOT: the frozen TS root');
const out = join(process.env.CARGO_TARGET_DIR ?? join(W, 'core/target'), 'es').replace(/\\/g, '/');
mkdirSync(out, { recursive: true });
const NM = `${R}/write/node_modules`;

/** the path-mapped config of one package (its own tsconfig.json extended) */
function config(pkg) {
  const dir = `${W}/write/packages/${pkg}`;
  return {
    extends: `${dir}/tsconfig.json`,
    compilerOptions: {
      // the viewer's config emits to dist/: never here
      noEmit: true,
      // the internal packages come by source (path mappings): the viewer's `rootDir: src` cannot hold them
      rootDir: W,
      baseUrl: dir,
      typeRoots: [`${NM}/@types`],
      paths: {
        '@universal-office/docx-core': ['../docx-core/src/index.ts'],
        '@universal-office/docx-layout': ['../docx-layout/src/index.ts'],
        '@universal-office/fonts': ['../../../shared/fonts/src/index.ts'],
        '@universal-office/ui': ['../../../shared/ui/src/index.ts'],
        '@universal-office/collation': ['../../../sheet/packages/calc/src/collation.ts'],
        '@universal-office/spell-sources': ['../../../shared/spell/src/sources.ts'],
        '@universal-office/spell': ['../../../shared/spell/src/index.ts'],
        '@universal-office/drawingml': ['../../../shared/drawingml/src/index.ts'],
        '@universal-office/compare': ['../compare/src/index.ts'],
        '@universal-office/charts': ['../../../shared/charts/src/index.ts'],
        '@universal-office/converter': [`${R}/converter/packages/converter/dist/index.d.ts`],
        fflate: [`${NM}/fflate`],
        '@universal-material/web': [`${NM}/@universal-material/web`],
        '@universal-material/web/*': [`${NM}/@universal-material/web/*`],
      },
    },
    include: pkg === 'editor'
      ? [`${dir}/src`, `${dir}/demo`, `${W}/write/packages/viewer/src/write-engine.d.ts`]
      : [`${dir}/src`],
    exclude: [`${dir}/demo/offline.ts`, `${dir}/src/**/*.test.ts`],
  };
}

let failed = false;
for (const pkg of ['editor', 'viewer']) {
  const file = `${out}/tsconfig.${pkg}.json`;
  writeFileSync(file, JSON.stringify(config(pkg), null, 2));
  const r = spawnSync(process.execPath, [`${NM}/typescript/bin/tsc`, '-p', file], { encoding: 'utf8' });
  const text = `${r.stdout ?? ''}${r.stderr ?? ''}`.trim();
  console.log(`${pkg}: tsc exit ${r.status}${text ? `\n${text}` : ', no output'}`);
  if (r.status !== 0 || text) failed = true;
}
process.exit(failed ? 1 : 0);
