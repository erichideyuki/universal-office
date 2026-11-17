// R-S4: run the frozen TS painter (`paintSlideTo` / `paintShapesTo` of slides-core `dist/render.js`) over the recording
// context (record-ctx.mjs) and return the canonical op stream. Everything the TS asks of a canvas is answered by the
// recorder; the images are fake decoded images identified by the hash of their bytes.
//
// Each run imports a FRESH instance of `render.js` (`?run=N`): the module keeps process-wide caches (the 8x8 pattern
// canvases, the path-gradient canvases) whose first use emits the canvas-building lines, so a shared instance would make
// the stream of a slide depend on which slides were painted before it.
import { createHash } from 'node:crypto';
import { join } from 'node:path';
import { pathToFileURL } from 'node:url';
import { createRecorder } from './record-ctx.mjs';
import { rasterSpecial, warmOrder } from './ts-special.mjs';

let runs = 0;
const loaded = new Map();

/** `paintSlideTo` and `paintShapesTo` of a fresh instance of the frozen render.js. */
export async function freshRender(tsRootDir) {
  const href = pathToFileURL(join(tsRootDir, 'slides', 'packages', 'slides-core', 'dist', 'render.js')).href;
  const key = `${href}?run=${++runs}`;
  const m = await import(key);
  loaded.set(key, true);
  return m;
}

/** Natural size of a PNG, JPEG or GIF (the decoded size the component would give); null when unknown. */
export function imageSize(b) {
  if (b.length > 24 && b[0] === 0x89 && b[1] === 0x50) return { w: (b[16] << 24 | b[17] << 16 | b[18] << 8 | b[19]) >>> 0, h: (b[20] << 24 | b[21] << 16 | b[22] << 8 | b[23]) >>> 0 };
  if (b.length > 10 && b[0] === 0x47 && b[1] === 0x49) return { w: b[6] | b[7] << 8, h: b[8] | b[9] << 8 };
  if (b.length > 4 && b[0] === 0xff && b[1] === 0xd8) {
    let i = 2;
    while (i + 9 < b.length) {
      if (b[i] !== 0xff) { i++; continue; }
      const mk = b[i + 1];
      if (mk >= 0xc0 && mk <= 0xcf && mk !== 0xc4 && mk !== 0xc8 && mk !== 0xcc) return { h: b[i + 5] << 8 | b[i + 6], w: b[i + 7] << 8 | b[i + 8] };
      i += 2 + (b[i + 2] << 8 | b[i + 3]);
    }
  }
  return null;
}

/** Every `{ part, bytes }` (an `ImageRef`) reachable from a slide model, by part name. */
export function collectImageRefs(model) {
  const out = new Map();
  const seen = new Set();
  const walk = (v) => {
    if (!v || typeof v !== 'object' || seen.has(v)) return;
    seen.add(v);
    if (typeof v.part === 'string' && v.bytes instanceof Uint8Array) { if (!out.has(v.part)) out.set(v.part, v.bytes); return; }
    if (v instanceof Uint8Array) return;
    for (const k of Object.keys(v)) walk(v[k]);
  };
  walk(model);
  return out;
}

/** The size of the canvas the slide is painted on: the slide at `scale` plus the origin on both sides. */
export function canvasSize(pres, scale, ox, oy) {
  return { width: Math.ceil(pres.slideW * scale + 2 * ox), height: Math.ceil(pres.slideH * scale + 2 * oy) };
}

/**
 * Record one slide. `mode` is `slide` (paintSlideTo) or `shapes` (paintShapesTo over the slide's shapes).
 * Returns `{ text, lines, sha, measures, counts, error }`.
 */
export async function recordTsSlide({ tsRoot, slide, pres, scale, ox = 0, oy = 0, mode = 'slide', caps, measure, opts = {}, render, special = false }) {
  const { paintSlideTo, paintShapesTo } = render ?? (await freshRender(tsRoot));
  const size = canvasSize(pres, scale, ox, oy);
  const rec = createRecorder({ ...size, caps, measure });
  const images = new Map();
  // B5b (`special`): the metafile, PICT and TIFF pictures are rasterized as the viewer does (at the display size of their first use)
  const displays = special ? warmOrder(slide) : null;
  for (const [part, bytes] of collectImageRefs(slide)) {
    if (special) {
      const cv = await rasterSpecial(tsRoot, bytes, displays.get(part), caps);
      if (cv) { images.set(part, cv); continue; }
    }
    const d = imageSize(bytes) ?? { w: 96, h: 96 };
    images.set(part, rec.image(bytes, d.w, d.h));
  }
  const resources = { slideW: pres.slideW, slideH: pres.slideH, images };
  rec.install();
  let error;
  try {
    if (mode === 'shapes') paintShapesTo(rec.ctx, slide.shapes, resources, scale, { ox, oy, ...opts });
    else paintSlideTo(rec.ctx, slide, resources, scale, { ox, oy, ...opts });
  } catch (e) {
    error = String(e?.message ?? e);
  } finally {
    rec.uninstall();
  }
  const text = rec.text();
  return {
    text, lines: rec.lines.length, sha: createHash('sha1').update(text).digest('hex').slice(0, 16),
    measures: rec.measureLog(), counts: rec.counts(), error,
  };
}
