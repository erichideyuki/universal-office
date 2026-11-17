// The registry of generated deck groups. Each group has a fixed seed range (disjoint from every
// other group, so adding a group never changes another), a profile and a size; `ci` groups are
// committed (decks + captures), the others are local-only runs.
//
// Later waves (B3 text layout, B4 render, B5 SmartArt/metafiles, B6 timing) add entries here,
// each with its own seed range and a builder from lib/gen/*.mjs.
import { buildDeck } from './b1.mjs';
import { b3Slide } from './b3.mjs';
import { buildSmartArtDeck } from './b5a.mjs';
import { b6Slide } from './b6.mjs';
import { b5bSlide } from './b5b.mjs';
import { c1dSlide } from './c1d.mjs';

const B1 = { slides: [1, 4], shapes: [3, 10], notes: 0.25 };

export const GROUPS = {
  // B1 read path, CI subset: committed decks and captures
  b1a: { seedBase: 100000, count: 200, profile: B1, build: buildDeck, ci: true },
  // B1 read path, big local run (never committed)
  // B3 text layout, CI subset: text-heavy slides (b3.mjs) over the B1 package
  b3a: { seedBase: 300000, count: 120, profile: { slides: [1, 3], shapes: [3, 7], notes: 0, slide: b3Slide }, build: buildDeck, ci: true },
  // B5a SmartArt: regeneration (diagram-layout.ts) and fallback (diagram-fallback.ts), with and without the cached drawing
  b5a: { seedBase: 500000, count: 150, profile: { slides: [1, 2] }, build: buildSmartArtDeck, ci: true },
  // B5b metafiles, PICT and TIFF as pictures (b5b.mjs): the rasters of raster.ts at the display size of the picture
  b5b: { seedBase: 700000, count: 40, profile: { slides: [1, 3], shapes: [2, 5], notes: 0, slide: b5bSlide }, build: buildDeck, ci: true },
  // B6 timing and playback: animation-heavy slides (b6.mjs), every transition kind, media nodes
  b6a: { seedBase: 600000, count: 100, profile: { slides: [1, 3], shapes: [3, 7], notes: 0, slide: b6Slide }, build: buildDeck, ci: true },
  // C-6 charts in slides: generated charts of the uo-charts generator as chart parts, with pictures and themeOverride (c1d.mjs)
  c1d: { seedBase: 800000, count: 60, profile: { slides: [1, 2], shapes: [0, 0], notes: 0, slide: c1dSlide }, build: buildDeck, ci: true },
  'b1-big': { seedBase: 200000, count: 2000, profile: { ...B1, shapes: [4, 14], slides: [1, 6] }, build: buildDeck, ci: false },
};

export const groupKey = (name, i) => `gen/${name}/${String(i).padStart(4, '0')}`;
export const deckFile = (name, i) => `${String(i).padStart(4, '0')}.pptx`;

/** Builds deck `i` of group `name`: { bytes, features, slides, seed }. */
export function buildGroupDeck(name, i) {
  const g = GROUPS[name];
  if (!g) throw new Error(`unknown group ${name} (known: ${Object.keys(GROUPS).join(', ')})`);
  const seed = g.seedBase + i;
  return { ...g.build(seed, g.profile), seed };
}
