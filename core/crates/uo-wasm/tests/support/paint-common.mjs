// What the two sides of the painter parity (uo-render frontier V) share: the paint configurations, the committed fixture
// documents, the date of the layouts and the environment of the open fonts of both layout engines.
import { readdirSync, existsSync, readFileSync } from 'node:fs';
import { join, dirname, relative, sep } from 'node:path';
import { fileURLToPath } from 'node:url';

const here = dirname(fileURLToPath(import.meta.url));
export const core = join(here, '../../../..');
export const repo = join(core, '..');

/** the date both layouts use (Date.UTC(2026, 0, 15, 12, 0, 0)) and the time zone offset */
export const NOW = Date.UTC(2026, 0, 15, 12, 0, 0);

/** kerning present or not on the context x the scale (twips -> pixel: 1/15 = 100% at 96 dpi, 2.5/15) */
export const CONFIGS = [
  { name: 'k1s1', kerning: true, scale: 1 / 15, screen: true, alpha: undefined },
  { name: 'k0s1', kerning: false, scale: 1 / 15, screen: true, alpha: undefined },
  { name: 'k1s2', kerning: true, scale: 2.5 / 15, screen: false, alpha: 0.6 },
  { name: 'k0s2', kerning: false, scale: 2.5 / 15, screen: false, alpha: 0.6 },
];

/** the committed fixture documents (`crates/uo-docx-layout/tests/captures/<group>/docs/*.docx`), sorted */
export function fixtureDocs() {
  const base = join(core, 'crates/uo-docx-layout/tests/captures');
  const out = [];
  for (const g of readdirSync(base).sort()) {
    const d = join(base, g, 'docs');
    if (!existsSync(d)) continue;
    for (const f of readdirSync(d).filter((x) => x.endsWith('.docx')).sort()) out.push(join(d, f));
  }
  return out;
}
export const relDoc = (p) => relative(join(core, 'crates/uo-docx-layout/tests/captures'), p).split(sep).join('/');

/** the open families the wasm environment gets as `installed` (the same files the TS node provider finds with --open-fonts) */
export const OPEN_FONTS = [
  ['Liberation Sans', 'site/assets/fonts', 'liberationsans'],
  ['Liberation Serif', 'shared/fonts/files', 'liberationserif'],
  ['Liberation Mono', 'shared/fonts/files', 'liberationmono'],
  ['Carlito', 'shared/fonts/files', 'carlito'],
  ['Caladea', 'shared/fonts/files', 'caladea'],
];
const VARIANTS = ['regular', 'bold', 'italic', 'boldItalic'];

/** a wasm Write environment with the open fonts and ALL the hyphenation patterns registered (no `hyphenationLanguages`) */
export function openEnv(uo) {
  const env = uo.newEnv();
  for (const [family, dir, stem] of OPEN_FONTS) {
    const faces = ['regular', 'bold', 'italic', 'bolditalic'].map((s, i) => ({ variant: VARIANTS[i], bytes: readFileSync(join(repo, dir, `${stem}-${s}.ttf`)) }));
    env.pushFamily(family, { source: 'installed', embeddable: true, faces });
  }
  const hy = join(core, 'crates/uo-fonts/data/hyphen');
  for (const f of readdirSync(hy).filter((x) => x.endsWith('.txt')).sort()) env.registerHyphenation(readFileSync(join(hy, f), 'utf8'));
  return env;
}

/** the layout options of the wasm side that match the frozen TS call `layoutDocument(full, provider, { now })` */
// wave 7 (B, gate-O3): `tsEmptyTheme` keeps the frozen TS's empty theme for a package without a theme part (the product
// follows Word's Office 2023 colours there; f2-3-0..3 are the fixtures it changes)
// ED-713: `tsPagerefWithoutChapter` keeps its PAGEREF page without the chapter numbers (Word writes them)
// wave 7c (W): and the six Word-over-TS switches of wave 7c (uo-docx-layout `LayoutOptions::ts_strict`)
export const wasmOptions = () => ({
  generation: 1, now: NOW, tzOffsetMin: 0, tsEmptyTheme: true, tsPagerefWithoutChapter: true,
  tsThemeTint: true, tsMissingSettings: true, tsLinkedCell: true, tsRefBookmarks: true, tsHyphenation: true, tsKeepLines: true,
  tsWcLines: true, // WordCraft study (lines)
  tsWcPage: true, // WordCraft study (page)
  tsWcTables: true, // WordCraft study (tables)
  tsWcFields: true, // WordCraft study (fields)
});
