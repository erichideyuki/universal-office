// Writes tests/parity/frontier_d_inputs.txt: the committed corpus of the small facets of frontier D
// (odttf keys, style names, language tags, hyphenation words), read by BOTH `capture_frontier_d.mjs`
// (the frozen TS) and `tests/frontier_d_parity.rs` (Rust), so the two sides compare over the same
// inputs and no generator has to be mirrored. Deterministic (the xorshift32 of uo-testkit) and
// built from the shipped pattern files: run it again only when a list below changes.
//
//   node tests/parity_support/gen_frontier_d_inputs.mjs
//
// Lines (tab separated): `odttf <units>`, `style <units>`, `lang <units>` with the string as
// comma-separated UTF-16 code units in hex (`~` for `undefined`), and `word <id> <text>` in UTF-8.
import { readFileSync, writeFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { Rng } from '../../../uo-testkit/js/testkit.mjs';

const here = dirname(fileURLToPath(import.meta.url));
const units = (s) => (s === undefined ? '~' : Array.from({ length: s.length }, (_, i) => s.charCodeAt(i).toString(16)).join(','));
const out = [];
const rng = new Rng(0xF0D0_0D01);

// odttf keys
const GUID = '00112233-4455-6677-8899-AABBCCDDEEFF';
const odttf = [
  `{${GUID}}`, GUID, `{${GUID.toLowerCase()}}`, GUID.replace(/-/g, ''), `{${GUID.slice(0, -1)}}`, '', '{}', '{-}', '-'.repeat(40),
  'A'.repeat(31), 'A'.repeat(32), 'A'.repeat(33), 'F'.repeat(100), '0x'.repeat(16), `0x${'1'.repeat(30)}`,
  `{ ${GUID} }`, ` 1 2 3 4 5 6 7 8 9 A B C D E F${'0'.repeat(20)}`, `+1+2+3+4${'5'.repeat(28)}`, `1g2g3g4g5g6g7g${'8'.repeat(26)}`,
  'zz'.repeat(16), `{é${'A'.repeat(40)}`, `{\u{1F600}${'A'.repeat(40)}`, '\u{1F600}'.repeat(15), '\u{1F600}'.repeat(16),
  `${'A'.repeat(30)}\u{1F600}\u{1F600}`, '\ud800'.repeat(32), `\ud800${'A'.repeat(40)}`, `\udc00${'A'.repeat(40)}`, `A\ud800B${'C'.repeat(40)}`,
  `${'1'.repeat(31)}\ud800`, '\u00a0'.repeat(10) + 'A'.repeat(40), '\ufeffA\ufeffB\ufeffC' + 'D'.repeat(40), '\t\n1\t\n2' + '3'.repeat(40),
  `${GUID}${GUID}${GUID}`, '9'.repeat(1000),
];
const alphabet = [...'0123456789abcdefABCDEF'.repeat(3), ...'xX+- {}gGzZé\u00a0\ufeff\t', '\u{1F600}', '\ud800', '\udc00'];
for (let i = 0; i < 260; i++) {
  const len = [4, 16, 31, 32, 33, 36, 40, 64][rng.below(8)];
  let s = '';
  while (s.length < len) s += alphabet[rng.below(alphabet.length)];
  odttf.push(s);
}
for (const k of odttf) out.push(`odttf\t${units(k)}`);

// style names (well-formed text only: Rust takes &str)
const styles = [
  undefined, '', ' ', 'Regular', 'regular', 'REGULAR', 'Normal', 'Roman', 'Bold', 'bold', 'BOLD', 'Italic', 'italic', 'Oblique', 'Bold Italic',
  'Bold Oblique', 'bold italic', 'BOLD ITALIC', 'Bold  Italic', 'Bold\tItalic', 'Bold\u00a0Italic', 'Bold\u2003Oblique', '\ufeffBold', 'Bold\ufeff',
  '\u00a0\u00a0regular\u2028', 'bold\n\r\nitalic', ' bold italic ', 'Light', 'Semibold', 'Black', 'Bold Condensed', 'Italic Bold', 'BoldItalic',
  'Bold-Italic', 'Heavy', 'Medium', 'Thin', 'Book', 'Demi', 'Extra Bold', '\u0085bold', '\u200bbold', '\u180ebold', 'Ïtalic',
  'İtalic', 'ITALIC', 'Itılic', 'ＢＯＬＤ', 'BÓLD', 'bolḋ', 'RÖMAN', 'ΣΣ', 'bold italic oblique',
  'regular regular', 'bold İ', 'Fett', 'Kursiv', 'Cursiva', 'Negrito',
];
for (let i = 0; i < 60; i++) {
  const parts = ['regular', 'bold', 'italic', 'oblique', 'normal', 'roman', 'light', 'black'];
  const seps = [' ', '  ', '\t', '\u00a0', '\u3000', '\ufeff', ''];
  const a = parts[rng.below(parts.length)];
  const s = rng.below(3) === 0 ? a : `${a}${seps[rng.below(seps.length)]}${parts[rng.below(parts.length)]}`;
  styles.push(rng.below(2) ? s.toUpperCase() : s);
}
for (const s of styles) out.push(`style\t${units(s)}`);

// language tags
const langs = [
  undefined, '', 'pt', 'PT', 'pt-BR', 'pt_BR', 'pt-PT', 'Pt-br', 'es', 'es-ES', 'es-MX', 'es-419', 'ES_ar', 'fr', 'fr-FR', 'fr-CA', 'FR-ch', 'de', 'de-DE',
  'de-AT', 'de-CH', 'DE-at', 'de_1996', 'it', 'it-IT', 'it-CH', 'nl', 'nl-NL', 'nl-BE', 'en', 'en-US', 'en-GB', 'en-AU', 'en-CA', 'EN', 'En-us', 'en_us',
  'en-us', 'en-gb-oed', 'de-1996', 'de-1901', 'pt-x-private', 'ja', 'ja-JP', 'zh', 'zh-Hans-CN', 'ko', 'ru', 'ru-RU', 'ar', 'he', 'hi', 'tr', 'pl', 'sv',
  'da', 'fi', 'cs', 'el', 'la', 'ca', 'gl', 'eu', 'ro', 'hu', 'uk', 'vi', 'th', 'id', 'english', 'portuguese', 'e', 'p', '-', '_', '-en', '_pt', '--', 'en-',
  'pt_', 'constructor', 'toString', '__proto__', 'hasOwnProperty', 'valueOf', 'prototype', 'length', 'name', 'es-', 'x', 'i-klingon', 'und', 'mul',
  'İt', 'İT-it', 'pİ', 'PT-İ', 'és', 'frç', 'nl̇', 'DÉ', ' pt', 'pt ', 'pt\t', 'pt\u00a0', '\u00a0pt',
];
const bases = ['pt', 'es', 'fr', 'de', 'it', 'nl', 'en', 'ja', 'zh', 'ru', 'ca'];
const tails = ['', '-BR', '_br', '-US', '-419', '-x', '_', '-', '--a', '-1996', '-Latn-DE'];
for (let i = 0; i < 160; i++) {
  let b = bases[rng.below(bases.length)];
  if (rng.below(3) === 0) b = b.toUpperCase();
  else if (rng.below(4) === 0) b = b[0].toUpperCase() + b.slice(1);
  langs.push(`${b}${tails[rng.below(tails.length)]}`);
}
for (const l of langs) out.push(`lang\t${units(l)}`);

// hyphenation words per language
const IDS = ['pt', 'en-us', 'es', 'fr', 'de-1996', 'it', 'nl'];
const real = {
  pt: ['hifenização', 'computador', 'paralelepípedo', 'extraordinário', 'responsabilidade', 'Constituição', 'pneumático', 'psicólogo', 'abstrato', 'transatlântico', 'exceção', 'açúcar', 'máquina', 'bibliografia', 'subterrâneo'],
  'en-us': ['hyphenation', 'computer', 'extraordinary', 'antidisestablishmentarianism', 'table', 'present', 'project', 'associate', 'mathematics', 'algorithm', 'record', 'reference', 'philanthropic', 'oddly', 'technology'],
  es: ['hifenización', 'computadora', 'extraordinario', 'paralelepípedo', 'constitución', 'desarrollo', 'transatlántico', 'niño', 'pingüino', 'biblioteca'],
  fr: ['anticonstitutionnellement', 'hyphénation', 'ordinateur', 'extraordinaire', 'république', 'forêt', 'château', 'oiseau', 'psychologie', 'immobilisation'],
  'de-1996': ['Donaudampfschifffahrtsgesellschaft', 'Silbentrennung', 'Computer', 'außergewöhnlich', 'Rechtschreibreform', 'Bundesverfassungsgericht', 'Straße', 'Grüße', 'Wissenschaft', 'Fußball'],
  it: ['sillabazione', 'computer', 'straordinario', 'costituzione', 'anticonstituzionalmente', 'psicologia', 'ferrovia', 'trasformazione', 'dell’ora', "dell'ora"],
  nl: ['woordafbreking', 'computer', 'buitengewoon', 'grondwet', 'wetenschappelijk', 'ziekenhuis', 'aardappelen', 'onafhankelijkheid', 'Nederland', 'Scheveningen'],
};
const odd = ['', 'a', 'ab', 'abc', 'a-b', "l'homme", 'co-operate', 'abc123', 'café', 'café', 'İstanbul', 'ΣΑΣ', 'straẞe', 'ßß', 'x\u00a0y', 'ab\u200bcd', '😀ab😀', '....', '.a.', 'A.B.C.D', 'ALLCAPS', 'MiXeDcAsE', 'ǅungla', 'ﬁnancial', 'wor\u00add', 'hyphen‐ation', 'a'.repeat(60), 'ab'.repeat(40)];
const gen = (id) => {
  const dataFile = readFileSync(join(here, '..', '..', 'data', 'hyphen', `${id}.txt`), 'utf8');
  const pats = dataFile.split('\n[patterns]\n')[1].split('\n[exceptions]\n')[0].split(/\s+/).filter(Boolean);
  const exc = dataFile.split('\n[exceptions]\n')[1].split(/\s+/).filter(Boolean);
  const letters = [];
  const seen = new Set();
  for (const p of pats) for (const ch of p) if (!/[0-9.]/.test(ch) && !seen.has(ch)) { seen.add(ch); letters.push(ch); }
  const words = [...real[id], ...odd];
  for (const e of exc) words.push(e.replace(/-/g, ''));
  for (let i = 0; i < 1500; i++) {
    const len = 2 + rng.below(15);
    let w = '';
    for (let j = 0; j < len; j++) w += letters[rng.below(letters.length)];
    words.push(w);
  }
  const variants = [];
  for (const w of words.slice(0, real[id].length + 40)) {
    variants.push(w.toUpperCase(), w.length ? w[0].toUpperCase() + w.slice(1) : w, w.toLowerCase());
  }
  return [...words, ...variants];
};
for (const id of IDS) for (const w of gen(id)) {
  if (/[\t\n\r]/.test(w) || /[\ud800-\udfff]/.test(w.replace(/[\ud800-\udbff][\udc00-\udfff]/g, ''))) throw new Error(`bad word ${JSON.stringify(w)}`);
  out.push(`word\t${id}\t${w}`);
}
writeFileSync(join(here, '..', 'parity', 'frontier_d_inputs.txt'), `${out.join('\n')}\n`);
console.log(`${out.length} lines`);
