import { fileURLToPath } from 'node:url';
import { defineConfig } from 'vitest/config';

// @universal-office/ui pelo src (como no tsconfig e nos bundles do demo): as
// barras usam o framework em shared/ui/src/toolbar, que o dist/ pode não ter.
export default defineConfig({
  resolve: {
    alias: {
      '@universal-office/ui': fileURLToPath(new URL('../../../shared/ui/src/index.ts', import.meta.url)),
      // a colação do Windows do Sheet (Classificar): só o arquivo, sem o motor de cálculo
      '@universal-office/collation': fileURLToPath(new URL('../../../sheet/packages/calc/src/collation.ts', import.meta.url)),
      // só o registro de fontes da revisão (motor/dicionários), sem o SpellChecker
      '@universal-office/spell-sources': fileURLToPath(new URL('../../../shared/spell/src/sources.ts', import.meta.url)),
      // engine="rust": o lado JS do motor (core/crates/uo-wasm/js), como no viewer
      '@universal-office/write-engine': fileURLToPath(new URL('../../../core/crates/uo-wasm/js/viewer.ts', import.meta.url)),
      '@universal-office/write-session': fileURLToPath(new URL('../../../core/crates/uo-wasm/js/session.mjs', import.meta.url)),
    },
  },
});
