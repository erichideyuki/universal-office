// Copies the CSS template literal of the HTML exporter (docx-core export/html.ts `STYLE`) into
// src/export/style.rs byte for byte (a template literal reads CRLF as LF).
//
//   UO_TS_ROOT=<built checkout of main> node core/crates/uo-docx/tools/gen-export-style.mjs
import { readFileSync, writeFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';

const root = process.env.UO_TS_ROOT;
if (!root) throw new Error('set UO_TS_ROOT to a checkout of main');
const src = readFileSync(join(root, 'write/packages/docx-core/src/export/html.ts'), 'utf8').replace(/\r\n/g, '\n');
const m = src.match(/const STYLE = `([\s\S]*?)`;/);
if (!m) throw new Error('STYLE not found');
const css = m[1];
if (css.includes('"#') || css.includes('\\') || css.includes('${')) throw new Error('the CSS needs escaping');
const out = join(dirname(fileURLToPath(import.meta.url)), '..', 'src', 'export', 'style.rs');
writeFileSync(
  out,
  `//! The CSS of the full HTML document (\`html.ts\` \`STYLE\`, copied byte for byte by \`tools/gen-export-style.mjs\`).\n\npub(super) const STYLE: &str = r#"${css}"#;\n`,
);
console.log(`wrote ${out} (${css.length} chars)`);
