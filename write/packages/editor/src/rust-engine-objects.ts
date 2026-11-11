// Handlers da frente O da leva 5 do engine="rust" (imagens, formas/caixas de texto (objeto), configurar página, exibições, menu de contexto): cada chave é um nome de
// ROUTES_OBJECTS.public/private (engine-routes-objects.ts). Carregado só com o motor Rust (import estático de rust-engine.ts).
import type { RustEditor } from './rust-engine.js';

export function objectsHandlers(_ed: RustEditor): Record<string, (...args: never[]) => unknown> {
  return {};
}
