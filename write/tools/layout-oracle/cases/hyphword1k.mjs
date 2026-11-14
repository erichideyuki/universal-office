// WordCraft study (2026-11-13), item D1: a BIGGER English sample of the per-word hyphenation agreement with Word
// (hyphword.mjs measured 150 words per language). 1,000 English words (11–16 lower-case letters) drawn with a fixed seed
// from the open en-US Hunspell dictionary (shared/spell/src/dicts/en-US.ts), excluding the 150 hyphword already measured;
// each word alone in a paragraph at six widths (30–80 pt), Liberation Sans 11, automatic hyphenation (same layout as
// hyphword, so the agreement script reads both). Measurement only: nothing here becomes an exception list.
import { readFileSync } from 'node:fs';
import { buildDocx, docDefaults, p, r } from '../docx-builder.mjs';
import { withHyphenation, NARROW_SECT, NARROW_TEXTW } from '../hyph-settings.mjs';

const DICTS = new URL('../../../../shared/spell/src/dicts/', import.meta.url);
const COUNT = 1000;
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

const all = words('en-US.ts');
// the 150 hyphword drew (seed 7 + 'e')
const old = new Set();
{
  const next = rng(7 + 'e'.charCodeAt(0));
  while (old.size < 150) old.add(all[Math.floor(next() * all.length)]);
}
const pick = new Set();
{
  const next = rng(20261113);
  while (pick.size < COUNT) {
    const w = all[Math.floor(next() * all.length)];
    if (!old.has(w)) pick.add(w);
  }
}
const list = [...pick];
const para = (word, wPt) => p(r(word, '<w:lang w:val="en-US"/>'), `<w:spacing w:after="0"/><w:jc w:val="left"/><w:ind w:left="0" w:right="${NARROW_TEXTW - wPt * 20}"/>`);

export const cases = [];
for (let d = 0; d * PER_DOC < list.length; d++) {
  const chunk = list.slice(d * PER_DOC, (d + 1) * PER_DOC);
  const body = chunk.map((w) => WIDTHS_PT.map((wPt) => para(w, wPt)).join('')).join('');
  cases.push({
    name: `hw-en-${String(d).padStart(2, '0')}`,
    words: chunk,
    docx: withHyphenation(buildDocx({ body, sect: NARROW_SECT, defaults: docDefaults({ font: 'Liberation Sans', size: 11, lang: 'en-US' }) }), { zone: 1 }),
  });
}
