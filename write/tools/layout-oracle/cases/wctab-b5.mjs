// WordCraft study (2026-11-14), the "tables" fixer: the B5 check in Word on the harness documents the row keep rule
// changes. `core/target/wc-tables/tools/b5_variant.py` writes `<doc>-b5.docx` (rust-core worktree): the document with
// every table-cell keepNext removed except the first paragraph's of each row's first cell. Word's PDF of the copy
// against Word's PDF of the original (`wctab-attr`): equal = those keepNext do nothing in Word (B5). Run with --fixdir
// core/target/wc-tables/b5.
import { readFileSync, readdirSync } from 'node:fs';
import { join, dirname } from 'node:path';
import { fileURLToPath } from 'node:url';

const here = dirname(fileURLToPath(import.meta.url));
const dir = join(here, '..', '..', '..', '..', 'core', 'target', 'wc-tables', 'b5');
// `<doc>-b5l.docx` (`--last-only`): only the keepNext of each cell's LAST paragraph removed (a keepNext between two
// paragraphs of a cell keeps them together when Word splits the row: not B5)
export const cases = readdirSync(dir).filter((f) => /-b5l?\.docx$/.test(f)).sort()
  .map((f) => ({ name: f.replace(/\.docx$/, ''), docx: readFileSync(join(dir, f)) }));
