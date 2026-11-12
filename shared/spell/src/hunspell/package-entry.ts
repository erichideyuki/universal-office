// index.js do pacote @universal-office/spellcheck: o motor (glue + binário) e as
// URLs dos arquivos ao lado dele, para o Worker e o glue subirem sem blob:/data:
// (CSP com script-src 'self'). As URLs saem do próprio módulo publicado.
export { glue, wasmBase64 } from './wasm.js';

/** URL absoluta do Worker do motor (worker.js, ao lado deste módulo). */
export const workerUrl: string = new URL('./worker.js', import.meta.url).href;

/** URL absoluta do glue do emscripten como arquivo (hunspell.js, ao lado deste módulo). */
export const glueUrl: string = new URL('./hunspell.js', import.meta.url).href;
