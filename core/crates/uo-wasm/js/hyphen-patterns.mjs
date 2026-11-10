// The hyphenation pattern text the Rust layout registers (`env.registerHyphenation`), built from the TS pattern modules of the
// frozen engine (`write/packages/docx-layout/src/typography/hyphen/patterns/<id>.ts`), so a bundle that carries both engines holds each
// language once (record core/docs/modules/uo-bindings.md "The TS/Rust switch", SW-2). Literal dynamic imports: a bundler puts each
// language in the same chunk the TS path loads. The text is the format of `uo-fonts/src/hyphen/mod.rs` `parse_patterns`.
const LOADERS = {
  pt: () => import('../../../../write/packages/docx-layout/src/typography/hyphen/patterns/pt.ts'),
  'en-us': () => import('../../../../write/packages/docx-layout/src/typography/hyphen/patterns/en-us.ts'),
  es: () => import('../../../../write/packages/docx-layout/src/typography/hyphen/patterns/es.ts'),
  fr: () => import('../../../../write/packages/docx-layout/src/typography/hyphen/patterns/fr.ts'),
  'de-1996': () => import('../../../../write/packages/docx-layout/src/typography/hyphen/patterns/de-1996.ts'),
  it: () => import('../../../../write/packages/docx-layout/src/typography/hyphen/patterns/it.ts'),
  nl: () => import('../../../../write/packages/docx-layout/src/typography/hyphen/patterns/nl.ts'),
};

/** The pattern file text of `id`, or null for an unknown id. */
export async function hyphenationPatternText(id) {
  if (!Object.hasOwn(LOADERS, id)) return null;
  const d = (await LOADERS[id]()).default;
  return `id ${d.id}\nleft ${d.left}\nright ${d.right}\n[patterns]\n${d.patterns}\n[exceptions]\n${d.exceptions}\n`;
}
