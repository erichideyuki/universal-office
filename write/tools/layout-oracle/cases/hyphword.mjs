// Wave 7c (W, item 5 — the Rust port): concordância PONTO A PONTO da hifenização com o Word, por idioma.
// Palavras longas (11–16 letras, minúsculas) sorteadas com semente fixa dos dicionários Hunspell abertos do repositório
// (shared/spell/src/dicts: pt-BR VERO, en-US SCOWL, es RLA-ES); cada palavra sozinha num parágrafo, em seis larguras
// (30–80 pt) — a palavra não cabe, então cada largura mostra o último ponto de hifenização que cabe (ou a quebra de
// emergência por caractere). Liberation Sans 11, hifenização automática. Só mede: as palavras NÃO foram escolhidas onde
// os padrões e o Word concordam (ao contrário de hyphlang), e o resultado não vira exceção (diretriz do mantenedor).
import { readFileSync } from 'node:fs';
import { buildDocx, docDefaults, p, r } from '../docx-builder.mjs';
import { withHyphenation, NARROW_SECT, NARROW_TEXTW } from '../hyph-settings.mjs';

const DICTS = new URL('../../../../shared/spell/src/dicts/', import.meta.url);
const LANGS = [['pt', 'pt-BR', 'pt-BR.ts'], ['en', 'en-US', 'en-US.ts'], ['es', 'es-ES', 'es.ts']];
const PER_LANG = 150;
const PER_DOC = 50;
const WIDTHS_PT = [30, 40, 50, 60, 70, 80];

function words(file) {
  const src = readFileSync(new URL(file, DICTS), 'utf8');
  const dic = src.slice(src.indexOf('export const dic = `') + 20);
  const body = dic.slice(0, dic.indexOf('`'));
  const out = new Set();
  for (const line of body.split('\n').slice(1)) {
    const w = line.split('/')[0].trim();
    if (w.length >= 11 && w.length <= 16 && /^\p{Ll}+$/u.test(w)) out.add(w);
  }
  return [...out].sort();
}

/** mulberry32: a fixed sample, the same on every run */
function rng(seed) {
  let a = seed >>> 0;
  return () => {
    a = (a + 0x6D2B79F5) >>> 0;
    let t = a;
    t = Math.imul(t ^ (t >>> 15), t | 1);
    t ^= t + Math.imul(t ^ (t >>> 7), t | 61);
    return ((t ^ (t >>> 14)) >>> 0) / 4294967296;
  };
}

const para = (word, lang, wPt) => p(r(word, `<w:lang w:val="${lang}"/>`), `<w:spacing w:after="0"/><w:jc w:val="left"/><w:ind w:left="0" w:right="${NARROW_TEXTW - wPt * 20}"/>`);

export const cases = [];
for (const [id, lang, file] of LANGS) {
  const all = words(file);
  const next = rng(7 + id.charCodeAt(0));
  const pick = new Set();
  while (pick.size < PER_LANG && pick.size < all.length) pick.add(all[Math.floor(next() * all.length)]);
  const list = [...pick];
  for (let d = 0; d * PER_DOC < list.length; d++) {
    const chunk = list.slice(d * PER_DOC, (d + 1) * PER_DOC);
    const body = chunk.map((w) => WIDTHS_PT.map((wPt) => para(w, lang, wPt)).join('')).join('');
    cases.push({
      name: `hw-${id}-${d}`,
      words: chunk,
      docx: withHyphenation(buildDocx({ body, sect: NARROW_SECT, defaults: docDefaults({ font: 'Liberation Sans', size: 11, lang }) }), { zone: 1 }),
    });
  }
}
