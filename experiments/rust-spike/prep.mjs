// Entradas do spike, as mesmas para TS, C# e Rust: as 20 fontes abertas da suíte, as 5 fontes
// embutidas no demo.docx (já desofuscadas pelo docx-core) e todo o texto do documento.
//   node prep.mjs   →   data/fonts/*.ttf, data/fonts.txt (lista), data/text.txt
import { readFileSync, writeFileSync, mkdirSync, copyFileSync } from 'node:fs';
import { join, resolve, dirname } from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';

const here = dirname(fileURLToPath(import.meta.url));
const root = resolve(here, '../..');
const core = await import(pathToFileURL(join(root, 'write/packages/docx-core/dist/index.js')).href);
const out = join(here, 'data');
mkdirSync(join(out, 'fonts'), { recursive: true });

const list = [];
for (const name of ['carlito', 'caladea', 'liberationsans', 'liberationserif', 'liberationmono']) {
  for (const v of ['regular', 'bold', 'italic', 'bolditalic']) {
    const f = `${name}-${v}.ttf`;
    copyFileSync(join(root, 'site/assets/fonts', f), join(out, 'fonts', f));
    list.push(f);
  }
}
const pkg = core.DocxPackage.load(new Uint8Array(readFileSync(join(root, 'site/assets/samples/demo.docx'))));
for (const e of core.readEmbeddedFonts(pkg)) {
  const f = `embed-${e.family.replace(/\W+/g, '')}-${e.variant}.ttf`;
  writeFileSync(join(out, 'fonts', f), e.data);
  list.push(f);
}
writeFileSync(join(out, 'fonts.txt'), list.join('\n') + '\n');
const xml = pkg.documentXml();
const text = [...xml.matchAll(/<w:t(?:\s[^>]*)?>([^<]*)<\/w:t>/g)].map((m) => m[1]
  .replace(/&lt;/g, '<').replace(/&gt;/g, '>').replace(/&quot;/g, '"').replace(/&apos;/g, "'").replace(/&amp;/g, '&')).join(' ');
writeFileSync(join(out, 'text.txt'), text);
console.log(`${list.length} fontes, texto com ${[...text].length} code points`);
