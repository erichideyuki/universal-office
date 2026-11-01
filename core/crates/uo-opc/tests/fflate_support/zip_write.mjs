// Generator of the constant FFLATE_STORED in tests/zip_write.rs.
//
// Run: node tests/fflate_support/zip_write.mjs <fflate location>
//   e.g. <fflate location> = a UniversalOffice checkout with write/node_modules installed.
// Prints the hex of zipSync({ mimetype, 'é/ü.xml', 'a/b.xml', 'dir/' }, { level: 0,
// mtime: new Date(2000, 0, 1) }); the Rust writer must produce the same bytes.

import { existsSync, readFileSync } from 'node:fs';
import { join, resolve } from 'node:path';
import { pathToFileURL } from 'node:url';

// Loads fflate from the directory given as the first argument: the fflate package itself, a
// directory with node_modules/fflate, or a checkout whose write/node_modules has it.
async function loadFflate(arg) {
  if (!arg) {
    console.error('usage: node <script> <fflate package dir | dir with node_modules | UniversalOffice checkout>');
    process.exit(2);
  }
  const base = resolve(arg);
  for (const dir of [base, join(base, 'node_modules', 'fflate'), join(base, 'write', 'node_modules', 'fflate')]) {
    const entry = join(dir, 'esm', 'index.mjs');
    if (existsSync(entry)) {
      const { version } = JSON.parse(readFileSync(join(dir, 'package.json'), 'utf8'));
      if (version !== '0.8.3') console.error(`warning: fflate ${version}; the fixtures were made with 0.8.3`);
      return import(pathToFileURL(entry).href);
    }
  }
  console.error(`fflate not found under ${base}`);
  process.exit(2);
}
const f = await loadFflate(process.argv[2]);
const enc = (s) => new TextEncoder().encode(s);
const z = f.zipSync({ 'mimetype': enc('application/vnd.oasis.opendocument.text'), 'é/ü.xml': enc('<x/>'), 'a/b.xml': new Uint8Array(0), 'dir/': new Uint8Array(0) }, { level: 0, mtime: new Date(2000, 0, 1) });
console.log(Buffer.from(z).toString('hex'));
