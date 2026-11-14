// WordCraft study (2026-11-14), the "page" implementer: the committed capture fixtures that a page switch changes
// (uo-docx-layout word_wc_page.rs `attribute_documents_to_the_page_rules`), exported by Word as they are, so the harness
// declarations can be checked against Word (uo-docx-layout word_w7c.rs `adhoc_documents_against_word`). Run with --fixdir
// core/target/wc-page/adhoc (rust-core worktree).
import { readFileSync } from 'node:fs';
import { join, dirname } from 'node:path';
import { fileURLToPath } from 'node:url';

const here = dirname(fileURLToPath(import.meta.url));
const caps = join(here, '..', '..', '..', '..', 'core', 'crates', 'uo-docx-layout', 'tests', 'captures');
const DOCS = [
  'f7a__7__13', 'f7a__7__15', 'f7a__7__2', 'f7a__7__5', 'f7a__7__6',
  'f7b__7__10', 'f7b__7__3', 'f7b__7__5', 'f7b__7__6',
  'f7c__7__13', 'f7c__7__6',
  'f8__7__3', 'f8__7__5', 'f8__7__6',
];
export const cases = DOCS.map((name) => ({ name, docx: readFileSync(join(caps, name.replace(/__.*$/, ''), 'docs', `${name}.docx`)) }));
