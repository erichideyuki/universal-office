// Handlers da frente S da leva 5 do engine="rust" (tabelas, cabeçalho/rodapé, notas, cursor na caixa de texto): cada chave é um nome de
// ROUTES_STORIES.public/private (engine-routes-stories.ts). Carregado só com o motor Rust (import estático de rust-engine.ts).
import type { RustEditor } from './rust-engine.js';

export function storiesHandlers(_ed: RustEditor): Record<string, (...args: never[]) => unknown> {
  return {};
}
