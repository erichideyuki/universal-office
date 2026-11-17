// The B5b input set: synthetic EMF, EMF+, WMF and PICT files (and, in later sections, TIFF and EOT) with a deterministic
// seed range (`b5b`, 700000..) plus mutated variants (truncated, bit-flipped), each with the display sizes it is rasterized at.
import { Rand } from './rand.mjs';
import { genEmf, genEmfPlus, genEmfPlusFocused } from './meta-emf.mjs';
import { genWmf } from './meta-wmf.mjs';
import { genPict } from './meta-pict.mjs';
import { genTiff } from './meta-tiff.mjs';
import { genEot } from './meta-eot.mjs';

export const META_SEED = 700000;

/** A damaged copy: cut at a random point, or with a few bytes changed (the headers stay mostly intact). */
export function mutate(bytes, seed) {
  const r = new Rand(seed);
  const b = new Uint8Array(bytes);
  const kind = r.pick(['cut', 'cut', 'flip', 'flip', 'zero', 'big']);
  if (kind === 'cut') return b.slice(0, Math.max(1, r.int(Math.min(b.length, 20), b.length)));
  const n = r.int(1, 6);
  for (let i = 0; i < n; i++) {
    const at = r.chance(0.5) ? r.int(Math.min(b.length - 1, 24), b.length - 1) : r.below(Math.min(b.length, 120));
    b[at] = kind === 'zero' ? 0 : kind === 'big' ? 0xff : b[at] ^ (1 << r.below(8));
  }
  return b;
}

/** `{ key, kind, bytes, displays }` of every case; `displays` are `null` (no display) or `{ w, h }` in px at 96 dpi. */
export function metaCases() {
  const out = [];
  const add = (key, kind, bytes, displays) => out.push({ key, kind, bytes, displays });
  const disp = (seed) => {
    const r = new Rand(seed);
    return [null, { w: r.pick([480, 96, 1200, 300, 2500]), h: r.pick([270, 96, 700, 300, 40]) }];
  };
  const N = { emf: 120, emfplus: 70, 'emfplus-focus': 16, wmf: 90, pict: 70, tiff: 130, eot: 160 };
  const gens = { emf: genEmf, emfplus: genEmfPlus, 'emfplus-focus': genEmfPlusFocused, wmf: genWmf, pict: genPict, tiff: genTiff, eot: genEot };
  let base = META_SEED;
  for (const kind of Object.keys(N)) {
    for (let i = 0; i < N[kind]; i++) {
      const bytes = gens[kind](base + i);
      add(`${kind}/${String(i).padStart(4, '0')}`, kind, bytes, kind === 'pict' || kind === 'tiff' || kind === 'eot' ? [null] : disp(base + i));
    }
    for (let i = 0; i < Math.round(N[kind] / 3); i++) {
      const bytes = gens[kind](base + i);
      add(`${kind}-mut/${String(i).padStart(4, '0')}`, kind, mutate(bytes, base + 90000 + i), kind === 'pict' || kind === 'tiff' || kind === 'eot' ? [null] : disp(base + i + 5));
    }
    base += 1000;
  }
  return out;
}
