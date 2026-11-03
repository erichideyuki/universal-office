// Differential reference of the reader's regex sites (G9, src/read/matchers.rs): V8 `RegExp`
// over the inputs of each site file in js/regex-sites/ (`templates.mjs`, and one per frontier),
// written to tests/regex/<file>.expected and tests/regex/<file>.inputs.txt; the Rust tests
// (`regex_templates`, `regex_r1`…) run their matchers over the same inputs.
//
//   node core/crates/uo-docx/js/regex-diff.mjs
//
// A site file exports SITES (rows `[id, regex | [regex, replacement], op]`) and optionally
// INPUTS (default: the template inputs). Results are JSON: `test` → boolean, `index` → the
// UTF-16 index or null, `group` → group 1 or null, `replace` → the string, `match` → the whole
// match or null.
import { readdirSync, writeFileSync } from 'node:fs';
import { join, dirname, basename } from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';

const here = dirname(fileURLToPath(import.meta.url));
const dir = join(here, 'regex-sites');
const base = await import(pathToFileURL(join(dir, 'templates.mjs')).href);

// one input per line, with `\`, LF, CR and TAB escaped
const escLine = (x) => x.replace(/\\/g, '\\\\').replace(/\n/g, '\\n').replace(/\r/g, '\\r').replace(/\t/g, '\\t');

for (const file of readdirSync(dir).filter((f) => f.endsWith('.mjs')).sort()) {
  const m = await import(pathToFileURL(join(dir, file)).href);
  const inputs = m.INPUTS ?? base.INPUTS;
  const name = basename(file, '.mjs');
  const lines = [`# regex differential (${file}): V8 RegExp (Node 24.15) over ${name}.inputs.txt, JSON per (site, input)`];
  for (const [id, re, op] of m.SITES) {
    inputs.forEach((input, i) => {
      let r;
      if (op === 'test') r = re.test(input);
      else if (op === 'index') { const x = re.exec(input); r = x ? x.index : null; }
      else if (op === 'group') { const x = re.exec(input); r = x ? (x[1] ?? null) : null; }
      else if (op === 'match') { const x = re.exec(input); r = x ? x[0] : null; }
      else r = input.replace(re[0], re[1]);
      if (re instanceof RegExp) re.lastIndex = 0;
      lines.push(`${id}\t${i}\t${JSON.stringify(r)}`);
    });
  }
  writeFileSync(join(here, '..', 'tests', 'regex', `${name}.expected`), lines.join('\n') + '\n');
  writeFileSync(join(here, '..', 'tests', 'regex', `${name}.inputs.txt`), inputs.map(escLine).join('\n') + '\n');
  console.log(`${file}: ${lines.length - 1} results`);
}
