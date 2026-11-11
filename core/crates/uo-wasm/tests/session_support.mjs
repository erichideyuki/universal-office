// Shared by the session_*.test.mjs files: the repository root, the committed fixtures, the open fonts of the
// repository for a write environment, the layout options of the tests, and the module loader.
import { readFileSync } from 'node:fs';
import { join, dirname } from 'node:path';
import { fileURLToPath } from 'node:url';
import { writeImports, bindWrite } from '../js/write.mjs';
import { bindSession } from '../js/session.mjs';

const here = dirname(fileURLToPath(import.meta.url));
export const repo = join(here, '../../../..');
export const wasmPath = process.env.UO_WASM_PATH ?? join(here, '../../../target/wasm32-unknown-unknown/wasm/uo_wasm.wasm');

/** Instantiates the module at `UO_WASM_PATH`: `{ has, names, w, uo, ss, counting, live, settle }` (nulls when it has no session exports). */
export async function loadModule() {
  const bytes = readFileSync(wasmPath);
  const names = WebAssembly.Module.exports(new WebAssembly.Module(bytes)).map((e) => e.name);
  const has = names.includes('uo_session_open');
  const { imports, host } = writeImports();
  const w = has ? (await WebAssembly.instantiate(bytes, imports)).instance.exports : null;
  const uo = has ? bindWrite(w, host) : null;
  const ss = has ? bindSession(w, uo) : null;
  const counting = has && w.uo_memory_live_bytes() >= 0;
  const live = () => w.uo_memory_live_bytes();
  const settle = async () => {
    for (let i = 0; i < 5; i++) { globalThis.gc?.(); await new Promise((ok) => setTimeout(ok, 10)); }
    return live();
  };
  return { has, names, w, uo, ss, counting, live, settle };
}

const OPEN_FONTS = [
  ['Liberation Sans', 'site/assets/fonts', 'liberationsans'],
  ['Liberation Serif', 'shared/fonts/files', 'liberationserif'],
  ['Liberation Mono', 'shared/fonts/files', 'liberationmono'],
  ['Carlito', 'shared/fonts/files', 'carlito'],
  ['Caladea', 'shared/fonts/files', 'caladea'],
];
const VARIANTS = ['regular', 'bold', 'italic', 'boldItalic'];
let fontFilesCache = null;
/** `[[family, [4 file buffers]]]`, read once. */
export const fontFiles = () => (fontFilesCache ??= OPEN_FONTS.map(([family, dir, stem]) => [family, ['regular', 'bold', 'italic', 'bolditalic'].map((s) => readFileSync(join(repo, dir, `${stem}-${s}.ttf`)))]));
export const withFonts = (env) => {
  for (const [family, files] of fontFiles()) env.pushFamily(family, { source: 'installed', embeddable: true, faces: files.map((b, i) => ({ variant: VARIANTS[i], bytes: b })) });
  return env;
};
export const fixture = (key) => readFileSync(join(repo, `core/crates/uo-docx-layout/tests/captures/f8/docs/${key}.docx`));
export const OPTS = { now: 1_768_478_400_000, tzOffsetMin: 0, hyphenationLanguages: ['pt-BR', 'en', 'es'] };
