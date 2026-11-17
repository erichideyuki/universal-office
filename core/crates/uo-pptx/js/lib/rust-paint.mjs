// R-S4: run `paint-slides.ts` (the TS interpreter of the Slides display list, uo-wasm) over the recording context and
// return the canonical op stream, the same shape `ts-paint.mjs` returns for the frozen TS painter, so the two compare.
import { createHash } from 'node:crypto';
import { SlidesPainter } from '../../../uo-wasm/js/paint-slides.ts';
import { createRecorder } from './record-ctx.mjs';
import { imageSize } from './ts-paint.mjs';
import * as light3d from '../../../../../shared/drawingml/src/light3d.ts';
import { patternRows as sharedPatternRows } from '../../../../../shared/drawingml/src/patterns.ts';

/**
 * Paint a decoded list on a fresh recording context.
 *  - `media(index)`: the bytes of `tables.media[index]` (the part of the package), or null (the picture is missing);
 *  - `bitmap(index)`: the RGBA bytes of `tables.bitmaps[index]`;
 *  - `frame`: a decoded `SlotFrame` or undefined; `mediaFrame(shape)`: the host's video frame hook;
 *  - `light`, `patternRows`: the uo-drawingml tables for the bevel and patterned WordArt (default: the TS tables of
 *    `shared/drawingml`, the ones the frozen painter imports).
 * The canvas is `width` x `height` px (the producer's view box plus the origin on both sides).
 */
export function recordList({ list, tables, frame, width, height, caps, measure, media, bitmap, mediaFrame, light, patternRows, env: extra = {} }) {
  const rec = createRecorder({ width, height, caps, measure });
  const images = new Map();
  const env = {
    media(index) {
      if (images.has(index)) return images.get(index);
      const bytes = media ? media(index) : null;
      let img = null;
      if (bytes) {
        const nat = tables.media[index]?.natural;
        const d = nat ? { w: nat.w, h: nat.h } : imageSize(bytes) ?? { w: 96, h: 96 };
        img = rec.image(bytes, d.w, d.h);
      }
      images.set(index, img);
      return img;
    },
    bitmap: (index) => (bitmap ? bitmap(index) : null),
    ...(mediaFrame ? { mediaFrame } : {}),
    light: light ?? light3d,
    patternRows: patternRows ?? sharedPatternRows,
    ...extra,
  };
  const painter = new SlidesPainter(env);
  rec.install();
  let error;
  try {
    painter.paint(rec.ctx, list, tables, frame);
  } catch (e) {
    error = String(e?.message ?? e);
  } finally {
    rec.uninstall();
  }
  const text = rec.text();
  return {
    text, lines: rec.lines.length, sha: createHash('sha1').update(text).digest('hex').slice(0, 16),
    measures: rec.measureLog(), counts: rec.counts(), error, painter, rec,
  };
}
