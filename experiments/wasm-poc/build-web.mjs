// Monta web-dist/: a página, o app (esbuild, com os aliases do build:site-live do editor),
// o build .NET em WebAssembly (out/interp ou out/aot), as fontes abertas e o demo.docx.
//   node build-web.mjs [out/interp|out/aot]
import { cpSync, mkdirSync, rmSync, copyFileSync } from 'node:fs';
import { join, resolve, dirname } from 'node:path';
import { fileURLToPath } from 'node:url';
import { execFileSync } from 'node:child_process';
import { FONT_FILES } from './fonts.mjs';

const here = dirname(fileURLToPath(import.meta.url));
const root = resolve(here, '../..');
const build = resolve(here, process.argv[2] ?? 'out/interp');
const out = join(here, 'web-dist');
rmSync(out, { recursive: true, force: true });
mkdirSync(join(out, 'fonts'), { recursive: true });
copyFileSync(join(here, 'web/index.html'), join(out, 'index.html'));
cpSync(join(build, 'wwwroot'), join(out, 'wasm'), { recursive: true });
for (const f of FONT_FILES) copyFileSync(join(root, 'site/assets/fonts', f.path), join(out, 'fonts', f.path));
copyFileSync(join(root, 'site/assets/samples/demo.docx'), join(out, 'demo.docx'));
const editor = join(root, 'write/packages/editor');
const alias = (k, v) => `--alias:@universal-office/${k}=${join(root, v)}`;
execFileSync(join(root, 'write/node_modules/.bin/esbuild.cmd'), [
  join(here, 'web/app.ts'), '--bundle', '--format=esm', '--minify', `--outfile=${join(out, 'app.js')}`,
  '--external:./wasm/*',
  alias('docx-core', 'write/packages/docx-core/src/index.ts'),
  alias('docx-layout', 'write/packages/docx-layout/src/index.ts'),
  alias('drawingml', 'shared/drawingml/src/index.ts'),
  alias('fonts', 'shared/fonts/src/index.ts'),
], { cwd: editor, stdio: 'inherit', shell: true });
console.log(`web-dist pronto (${process.argv[2] ?? 'out/interp'})`);
