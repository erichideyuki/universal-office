// The views a draw-op capture records and the capability sets it pins (shared by capture-drawops.mjs and compare-drawops.mjs).
import { DEFAULT_CAPS } from './record-ctx.mjs';

/** `a`: 1280 px wide at the origin; `b`: 640 px wide at (17.5, 9.25) (a non-zero origin and another scale). */
export const VIEWS = {
  a: (pres) => ({ scale: 1280 / pres.slideW, ox: 0, oy: 0 }),
  b: (pres) => ({ scale: 640 / pres.slideW, ox: 17.5, oy: 9.25 }),
};

/** `nolayers`, `nopattern`, `nospacing`, `nokerning`, comma separated (each turns one canvas capability off). */
export function parseCaps(spec) {
  const caps = { ...DEFAULT_CAPS };
  for (const c of (spec ?? '').split(',').filter(Boolean)) {
    if (c === 'nolayers') caps.layers = false;
    else if (c === 'nopattern') caps.patternTransform = false;
    else if (c === 'nospacing') caps.letterSpacing = false;
    else if (c === 'nokerning') caps.fontKerning = false;
    else throw new Error(`unknown capability ${c}`);
  }
  return caps;
}
