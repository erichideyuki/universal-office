// Writes tests/parity/fonts_oracle_words.txt: the letter runs of the text (`w:t`) of the committed
// oracle documents of docx-layout, segmented as `markHyphenation` does (letters and combining marks,
// each cluster NFC-normalized to one code point when it makes one), one distinct word per line,
// sorted by code unit. Read by BOTH capture_fonts.mjs (the frozen TS hyphenator) and the Rust
// replay, so the two sides hyphenate the same words. The text of the documents is the repository's
// own test data.
//
//   UO_TS_ROOT=<built checkout of main> node tests/parity_support/gen_oracle_words.mjs
import { readFileSync, writeFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { importFrom, tsRoot, walk } from '../../../uo-testkit/js/testkit.mjs';

const here = dirname(fileURLToPath(import.meta.url));
const root = tsRoot();
const { unzipSync } = await importFrom(root, 'write/node_modules/fflate/esm/index.mjs');
const entities = { amp: '&', lt: '<', gt: '>', quot: '"', apos: "'" };
const decode = (s) => s.replace(/&(#x[0-9a-f]+|#\d+|[a-z]+);/gi, (m, e) => {
  if (e[0] === '#') { const cp = e[1].toLowerCase() === 'x' ? parseInt(e.slice(2), 16) : parseInt(e.slice(1), 10); return cp > 0x10ffff ? m : String.fromCodePoint(cp); }
  return entities[e] ?? m;
});
const LETTER = /^[\p{L}\p{M}]$/u;
const MARK = /^\p{M}$/u;
const words = new Set();
const files = walk(join(root, 'write/packages/docx-layout/test/oracle'), (f) => f.endsWith('.docx'));
for (const f of files) {
  let zip;
  try { zip = unzipSync(new Uint8Array(readFileSync(f)), { filter: (e) => /^word\/(document|footnotes|endnotes|header\d*|footer\d*)\.xml$/.test(e.name) }); } catch { continue; }
  for (const name of Object.keys(zip)) {
    const xml = Buffer.from(zip[name]).toString('utf8');
    let text = '';
    for (const m of xml.matchAll(/<w:t(?:\s[^>]*)?>([^<]*)<\/w:t>/g)) text += `${decode(m[1])} `;
    let cur = [];
    const flush = () => {
      if (cur.length) {
        const clusters = [];
        for (const ch of cur) { if (MARK.test(ch) && clusters.length) clusters[clusters.length - 1].push(ch); else clusters.push([ch]); }
        const w = clusters.map((c) => { const n = c.join('').normalize('NFC'); return Array.from(n).length === 1 ? n : c[0]; }).join('');
        if (Array.from(w).length >= 2 && Array.from(w).length <= 60) words.add(w);
      }
      cur = [];
    };
    for (const ch of text) { if (LETTER.test(ch)) cur.push(ch); else flush(); }
    flush();
  }
}
const sorted = [...words].sort();
writeFileSync(join(here, '..', 'parity', 'fonts_oracle_words.txt'), `${sorted.join('\n')}\n`);
console.log(`${files.length} documents, ${sorted.length} distinct words`);
