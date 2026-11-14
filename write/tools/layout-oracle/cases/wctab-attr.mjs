// WordCraft study (2026-11-14), the "tables" fixer: the committed capture fixtures that a tables switch changes
// (uo-docx-layout word_wc_tables.rs `attribute_documents_to_the_tables_rules`), exported by Word as they are, so the
// harness declarations can be checked against Word switch by switch (word_wc_tables.rs
// `adhoc_documents_against_word_by_switch`). Run with --fixdir core/target/wc-tables/adhoc (rust-core worktree).
import { readFileSync } from 'node:fs';
import { join, dirname } from 'node:path';
import { fileURLToPath } from 'node:url';

const here = dirname(fileURLToPath(import.meta.url));
const caps = join(here, '..', '..', '..', '..', 'core', 'crates', 'uo-docx-layout', 'tests', 'captures');
const DOCS = [
  'f7b__7__11', 'f7b__7__2', 'f7b__7__3', 'f7b__7__4', 'f7b__7__5', 'f7b__7__6', 'f7b__7__7', 'f7b__7__9',
  'f8__7__1', 'f8__7__3', 'f8__7__5', 'f8__7__7', 'f8__7__9',
];
export const cases = DOCS.map((name) => ({ name, docx: readFileSync(join(caps, name.replace(/__.*$/, ''), 'docs', `${name}.docx`)) }));
