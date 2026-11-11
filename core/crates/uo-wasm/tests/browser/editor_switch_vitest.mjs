// The editor's (or the viewer's) own vitest suite of THIS worktree without a node_modules in it (record uo-bindings.md
// "Milestone 2: the editor switch", verification (b)): vitest and the npm dependencies come from the frozen TS root
// (UO_TS_ROOT), the internal packages from the worktree's sources, the engine's JS side (`@universal-office/write-engine`,
// `@universal-office/write-session`) from core/crates/uo-wasm/js. A plain-object config (no `vitest/config` import, which
// would need the worktree's node_modules). `npm install` in the worktree is not used: it would create junctions.
//   cd write/packages/editor && UO_ES_PKG=editor node $UO_TS_ROOT/write/node_modules/vitest/vitest.mjs run --config <this file>
//   cd write/packages/viewer && UO_ES_PKG=viewer node $UO_TS_ROOT/write/node_modules/vitest/vitest.mjs run --config <this file>
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

const here = dirname(fileURLToPath(import.meta.url));
const W = resolve(here, '../../../../..').replace(/\\/g, '/');
const R = (process.env.UO_TS_ROOT ?? '').replace(/\\/g, '/');
if (!R) throw new Error('UO_TS_ROOT: the frozen TS root');
const PKG = process.env.UO_ES_PKG ?? 'editor';
const NM = `${R}/write/node_modules`;

export default {
  root: `${W}/write/packages/${PKG}`,
  server: { fs: { strict: false } },
  resolve: {
    alias: [
      { find: '@universal-office/ui', replacement: `${W}/shared/ui/src/index.ts` },
      { find: '@universal-office/collation', replacement: `${W}/sheet/packages/calc/src/collation.ts` },
      { find: '@universal-office/spell-sources', replacement: `${W}/shared/spell/src/sources.ts` },
      { find: /^@universal-office\/spell$/, replacement: `${W}/shared/spell/src/index.ts` },
      { find: /^@universal-office\/docx-core$/, replacement: `${W}/write/packages/docx-core/src/index.ts` },
      { find: /^@universal-office\/docx-layout$/, replacement: `${W}/write/packages/docx-layout/src/index.ts` },
      { find: /^@universal-office\/fonts$/, replacement: `${W}/shared/fonts/src/index.ts` },
      { find: /^@universal-office\/drawingml$/, replacement: `${W}/shared/drawingml/src/index.ts` },
      { find: /^@universal-office\/compare$/, replacement: `${W}/write/packages/compare/src/index.ts` },
      { find: /^@universal-office\/charts$/, replacement: `${W}/shared/charts/src/index.ts` },
      { find: /^@universal-office\/write-engine$/, replacement: `${W}/core/crates/uo-wasm/js/viewer.ts` },
      { find: /^@universal-office\/write-session$/, replacement: `${W}/core/crates/uo-wasm/js/session.mjs` },
      { find: /^@universal-office\/converter$/, replacement: `${R}/converter/packages/converter/dist/index.js` },
      { find: /^vitest$/, replacement: join(NM, 'vitest/dist/index.js').replace(/\\/g, '/') },
      { find: /^(fflate|@universal-material\/web)(\/.*)?$/, replacement: `${NM}/$1$2` },
    ],
  },
  // the editor's tests are in src/, the viewer's in test/
  test: { include: ['src/**/*.test.ts', 'test/**/*.test.ts'], pool: 'forks', poolOptions: { forks: { maxForks: 3, minForks: 1 } } },
};
