// Writes tests/parity/fonts_embed_scenarios.txt: the scenarios of the embedding-plan parity facet
// (`planFontEmbedding` of shared/fonts/src/embedding.ts against `plan_font_embedding`), read by BOTH
// capture_fonts_small.mjs (the frozen TS, with a stub resolver) and tests/fonts_parity_small.rs.
// Deterministic (xorshift32 of uo-testkit). Run again only when a list below changes.
//
//   node tests/parity_support/gen_embed_scenarios.mjs
//
// One line per scenario, tab separated: `S`, id, app (`write`|`slides`), settings (three 0/1:
// embed, subset, skipCommonFonts), the families already embedded (lower case, `,` separated), the
// usage (`;` items of `family|variants|hex code points`, variants from `rbiz`), the resolver stub
// (`;` items of `family|source|have|load`, where have/load are `+` separated `variant=pool font`)
// and the document faces (`+` separated `family|variant=pool font`).
import { writeFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { Rng } from '../../../uo-testkit/js/testkit.mjs';

const here = dirname(fileURLToPath(import.meta.url));
const rng = new Rng(0xe3b0c442);
const pick = (a) => a[rng.below(a.length)];
const POOL = ['carlito-regular', 'carlito-bold', 'carlito-italic', 'carlito-bolditalic', 'mono-regular', 'amatic-regular', 'restricted', 'bitmap', 'editable', 'cff', 'broken-cmap', 'glyf-small', 'garbage', 'cut-dir'];
const GOOD = ['carlito-regular', 'carlito-bold', 'carlito-italic', 'carlito-bolditalic', 'mono-regular', 'amatic-regular', 'glyf-small', 'editable'];
const FAMILIES = ['Calibri', 'calibri', 'CALIBRI', 'Arial', 'arial', 'ARIAL', 'Times New Roman', 'Courier New', 'courier new', 'Aptos', 'Carlito', 'Own Font', 'Missing Co', 'Restricted Co', 'Bitmap Co', 'CFF Co', 'Broken Co', 'Mono', 'Amatic SC', 'İstanbul', 'ΣΑΣ', 'Gabriola', 'Segoe UI', 'Wfit Test'];
const SOURCES = ['host', 'local-access', 'bundled', 'page', 'installed', 'substitute', 'default'];
const V = ['r', 'b', 'i', 'z'];
const SAMPLE = [...'ABCabc 0123 .,éçãõüßΣσ€—一二'];
const PAIRS = { r: 'carlito-regular', b: 'carlito-bold', i: 'carlito-italic', z: 'carlito-bolditalic' };

const lines = [];
const hexOf = (s) => [...s].map((c) => c.codePointAt(0).toString(16)).join(',');
const textOf = () => {
  let s = '';
  const n = rng.below(14);
  for (let i = 0; i < n; i++) s += pick(SAMPLE);
  if (rng.below(8) === 0) s += '\u{1f600}';
  return s;
};
const variantsOf = () => {
  const out = V.filter(() => rng.below(2) === 0);
  return out.length ? out.join('') : pick(V);
};
const poolFor = (family, v) => {
  const key = family.toLowerCase();
  if (key.includes('restricted')) return 'restricted';
  if (key.includes('bitmap')) return 'bitmap';
  if (key.includes('cff')) return 'cff';
  if (key.includes('broken')) return 'broken-cmap';
  if (key === 'mono') return 'mono-regular';
  if (key.includes('amatic')) return 'amatic-regular';
  return rng.below(6) === 0 ? pick(POOL) : PAIRS[v] ?? pick(GOOD);
};

for (let id = 0; id < 700; id++) {
  const app = rng.below(2) ? 'write' : 'slides';
  const settings = [rng.below(8) === 0 ? 0 : 1, rng.below(2), rng.below(2)];
  const nUsage = 1 + rng.below(4);
  const usage = [];
  for (let k = 0; k < nUsage; k++) usage.push({ family: pick(FAMILIES), variants: variantsOf(), text: textOf() });
  if (rng.below(6) === 0) usage.push({ family: '', variants: 'r', text: 'x' });
  const known = new Set();
  const resolver = [];
  for (const u of usage) {
    if (!u.family || known.has(u.family.toLowerCase()) || rng.below(6) === 0) continue;
    known.add(u.family.toLowerCase());
    const source = pick(SOURCES);
    const have = [], load = [];
    for (const v of V) {
      if (rng.below(3) === 0) have.push(`${v}=${poolFor(u.family, v)}`);
      else if (rng.below(2) === 0) load.push(`${v}=${poolFor(u.family, v)}`);
    }
    const spelled = rng.below(5) === 0 ? u.family.toUpperCase() : u.family;
    resolver.push(`${spelled}|${source}|${have.join('+')}|${load.join('+')}`);
  }
  const doc = [];
  if (rng.below(4) === 0) {
    const u = pick(usage);
    for (const v of V) if (rng.below(2) === 0 && u.family) doc.push(`${u.family}|${v}=${poolFor(u.family, v)}`);
  }
  const already = [];
  if (rng.below(6) === 0) already.push(pick(usage).family.toLowerCase());
  lines.push(['S', id, app, settings.join(''), already.join(','), usage.map((u) => `${u.family}|${u.variants}|${hexOf(u.text)}`).join(';'), resolver.join(';'), doc.join('+')].join('\t'));
}
// targeted: the fonts that make the plan throw (a cmap that fails when read, no readable tables) in
// every position of the plan, with and without the subset, for both apps
let id = lines.length;
for (const bad of ['broken-cmap', 'garbage', 'cut-dir']) {
  for (const app of ['write', 'slides']) {
    for (const bits of ['110', '100', '111', '101']) {
      for (const where of ['have', 'load', 'doc']) {
        const family = bad === 'garbage' ? 'Bitmap Garbage' : bad === 'cut-dir' ? 'Cut Co' : 'Broken Co';
        const resolver = where === 'doc' ? '' : `${family}|host|${where === 'have' ? `r=${bad}` : ''}|${where === 'load' ? `r=${bad}` : ''}`;
        const doc = where === 'doc' ? `${family}|r=${bad}` : '';
        lines.push(['S', id++, app, bits, '', `${family}|r|41,5a,7a;Calibri|rb|41`, `${resolver};Calibri|bundled|r=carlito-regular+b=carlito-bold|`, doc].join('	'));
      }
    }
  }
}
writeFileSync(join(here, '..', 'parity', 'fonts_embed_scenarios.txt'), `${lines.join('\n')}\n`);
console.log(`${lines.length} scenarios`);
