// worker.js do pacote @universal-office/spellcheck: o motor no Worker subindo
// pela URL do pacote (sem blob:), com o glue do emscripten ao lado (hunspell.js),
// resolvido pela URL do próprio worker — nunca por uma mensagem.
import { spellEngineMain, type EngineScope } from './engine.js';

spellEngineMain(self as unknown as EngineScope, new URL('./hunspell.js', import.meta.url).href);
