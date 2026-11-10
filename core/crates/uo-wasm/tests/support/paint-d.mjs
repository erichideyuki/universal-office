// The D side of the painter parity: the environment (`PaintEnv`) over a Rust layout or view, and one page painted with a
// configuration of paint-common.mjs by the TS painter of decoded pages (js/paint.ts).
import { paintItems, paintPageBackground } from '../../js/paint.ts';
import { decodePage, newTables } from '../../js/gen/render.ts';
import { bitmapOf, fontOf, colorOf } from './recording-context.mjs';

/** `PaintEnv` whose images are the stub bitmaps of the bytes `bytesOf(id)` returns (cached by id) */
export function envD(bytesOf) {
  const bitmaps = new Map();
  return {
    fontOf,
    color: colorOf,
    image(id) {
      let b = bitmaps.get(id);
      if (b === undefined) {
        const bytes = bytesOf(id);
        b = bytes ? bitmapOf(bytes) : null;
        bitmaps.set(id, b);
      }
      return b;
    },
  };
}

/** The decoded pages of a Rust layout (the messages in order, one set of tables) with the painter's `PaintPage` for each. */
export function decodedPages(layout) {
  const tables = newTables();
  const rasters = layout.rasters();
  const pages = [];
  for (let i = 0; i < layout.pageCount; i++) {
    const page = decodePage(layout.encodePage(i), tables);
    pages.push({ page, tables, index: i, rasters, owner: layout });
  }
  return pages;
}

export function paintPageD(ctx, p, env, cfg, caches) {
  paintPageBackground(ctx, p, env, { devicePxTw: 1 / cfg.scale, screen: cfg.screen, paper: '#ffffff' }, caches);
  paintItems(ctx, p, env, cfg.alpha === undefined ? {} : { alpha: cfg.alpha }, caches);
}
