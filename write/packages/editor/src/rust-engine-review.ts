// Handlers da frente R da leva 5 do engine="rust" (comentários, revisões, campos/referências, localizar, ortografia, estilos, área de transferência sem evento, teclas Alt): cada chave é um nome de
// ROUTES_REVIEW.public/private (engine-routes-review.ts). Carregado só com o motor Rust (import estático de rust-engine.ts).
import type { RustEditor } from './rust-engine.js';

export function reviewHandlers(_ed: RustEditor): Record<string, (...args: never[]) => unknown> {
  return {};
}
