// Vitest configuration of the `calc-tests` capture (run by ../capture-tests.mjs, never by hand on the TS checkout):
// the frozen TS's own calc tests (sheet/packages/calc/test/*.test.ts minus oracle.test.ts, which the `bank` group
// replays case by case) and the three Sheet tests that drive the calc (sheet-calc, style-pool, entry), with every
// exported function of calc/src and the public methods of Engine / SheetCalc / XfPool wrapped by a recorder
// (recorder-plugin.mjs + recorder-setup.mjs). Nothing is written into the checkout: the cache goes to
// core/target/sheet-cap, the records to UO_REC_DIR.
import { join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { recorderPlugin } from './recorder-plugin.mjs';

const here = fileURLToPath(new URL('.', import.meta.url));
const tsRoot = resolve(process.env.UO_TS_ROOT);
const packages = join(tsRoot, 'sheet', 'packages');

export default {
  root: packages,
  cacheDir: resolve(here, '../../../../target/sheet-cap/vite-cache'),
  plugins: [recorderPlugin({ packages })],
  resolve: {
    alias: {
      // as sheet/packages/sheet/vitest.config.ts
      '@universal-office/sheet-calc': join(packages, 'calc/src/index.ts'),
      '@universal-office/ui': join(tsRoot, 'shared/ui/src/index.ts'),
      '@universal-office/slides-core': join(tsRoot, 'slides/packages/slides-core/src/index.ts'),
    },
  },
  test: {
    globals: true,
    include: (process.env.UO_REC_INCLUDE ?? 'calc/test/*.test.ts,sheet/test/sheet-calc.test.ts,sheet/test/style-pool.test.ts,sheet/test/entry.test.ts').split(','),
    exclude: ['calc/test/oracle.test.ts', '**/node_modules/**'],
    setupFiles: [join(here, 'recorder-setup.mjs')],
    pool: 'forks',
    poolOptions: { forks: { singleFork: true } },
    fileParallelism: false,
    sequence: { shuffle: false },
    testTimeout: 600_000,
    env: { TZ: 'UTC' },
  },
};
