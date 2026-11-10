import { fileURLToPath } from 'node:url';
import { defineConfig } from 'vitest/config';

// @universal-office/ui pelo src (como no editor): o registro de plugins e os
// controles vêm de shared/ui/src, que o dist/ pode ainda não ter.
export default defineConfig({
  resolve: {
    alias: {
      '@universal-office/ui': fileURLToPath(new URL('../../../shared/ui/src/index.ts', import.meta.url)),
      // engine="rust": o lado JS do motor (core/crates/uo-wasm/js), como no build:demo e no build-pkg
      '@universal-office/write-engine': fileURLToPath(new URL('../../../core/crates/uo-wasm/js/viewer.ts', import.meta.url)),
    },
  },
});
