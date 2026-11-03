// Shared by the capture scripts of frontier R2 (capture-parts.mjs, capture-styles.mjs,
// capture-table-styles.mjs): the frozen TS modules, the corpus, the digest helpers.
import { readFileSync } from 'node:fs';
import { join, dirname } from 'node:path';
import { fileURLToPath } from 'node:url';
import { Stream, fffd, importFrom, tsRoot, walk, relPath } from '../../uo-testkit/js/testkit.mjs';
import { Canon } from './canon.mjs';

const here = dirname(fileURLToPath(import.meta.url));
export const crate = join(here, '..');
export const repo = join(here, '..', '..', '..', '..');
export const root = tsRoot();

/** Character references to lone surrogates are lone surrogates in the TS and U+FFFD in uo-opc (named
 *  acceptance A3 of R1, the decision of A5): strings reach the digest with them replaced. */
export class R2Canon extends Canon {
  str(x) { super.str(fffd(x)); }
}

export const ts = async (rel) => importFrom(root, `write/packages/docx-core/dist/${rel}`);

/** `{ full, order }` digests of `value` (the facet and its key-order stream); `traced` also returns
 *  the readable traces (`UO_PARITY_TRACE`, to find the first diverging item against the Rust trace). */
export function digestOf(value, key, traced = false) {
  const st = new Stream({ trace: traced });
  const order = new Stream({ trace: traced });
  new R2Canon(st, { order }).value(value, key);
  const out = { full: st.finish().short, order: order.finish().short };
  if (traced) out.traces = { full: st.trace(), order: order.trace() };
  return out;
}

/** The corpus: the oracle documents of docx-layout and the docx-core fixtures, as `{ rel, file }`. */
export function corpus() {
  return [
    ...walk(join(repo, 'write', 'packages', 'docx-layout', 'test', 'oracle'), (p) => p.endsWith('.docx')),
    ...walk(join(repo, 'write', 'packages', 'docx-core', 'test', 'fixtures'), (p) => p.endsWith('.docx')),
  ].map((file) => ({ rel: relPath(repo, file), file }));
}

export const readBytes = (file) => new Uint8Array(readFileSync(file));
