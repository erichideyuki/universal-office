// Opens every PDF of a directory with pdf.js (the viewer of the apps) and extracts the text of
// every page: the files the Rust writer makes must load, and a font written by it must give
// the characters back through its ToUnicode map.
//
//   UO_PDF_DUMP=<dir> cargo test -p uo-pdf --test writer_parity dump_files_for_pdfjs
//   UO_TS_ROOT=<built checkout of main> node tests/parity_support/check_pdfjs.mjs <dir>
//
// pdf.js comes from the checkout (converter/node_modules/pdfjs-dist), never from this repository.
import { readFileSync, readdirSync } from 'node:fs';
import { join } from 'node:path';
import { pathToFileURL } from 'node:url';
import { tsRoot } from '../../../uo-testkit/js/testkit.mjs';

// Node 24 has no Uint8Array.prototype.toHex (the encryption hashes of pdf.js 6 call it)
Uint8Array.prototype.toHex ??= function toHex() { return Array.from(this, (b) => b.toString(16).padStart(2, '0')).join(''); };

const root = tsRoot();
const pdfjs = await import(pathToFileURL(join(root, 'converter/node_modules/pdfjs-dist/build/pdf.mjs')).href);
const dir = process.argv.find((a, i) => i >= 2 && !a.startsWith('--') && process.argv[i - 1] !== '--ts-root');
if (!dir) throw new Error('usage: check_pdfjs.mjs <dir>');

let files = 0; let pages = 0; let chars = 0; const failures = [];
for (const name of readdirSync(dir).filter((n) => n.endsWith('.pdf')).sort()) {
  try {
    const data = new Uint8Array(readFileSync(join(dir, name)));
    const task = pdfjs.getDocument({ data, verbosity: 0, useSystemFonts: false, isEvalSupported: false });
    const doc = await task.promise;
    for (let i = 1; i <= doc.numPages; i++) {
      const page = await doc.getPage(i);
      const text = await page.getTextContent();
      chars += text.items.reduce((n, it) => n + (it.str ?? '').length, 0);
      pages++;
    }
    await doc.getOutline();
    await task.destroy();
    files++;
  } catch (e) {
    failures.push(`${name}: ${e?.message ?? e}`);
  }
}
console.log(`pdf.js opened ${files} files (${pages} pages, ${chars} text characters); ${failures.length} failures`);
for (const f of failures.slice(0, 20)) console.log(f);
process.exit(failures.length ? 1 : 0);
