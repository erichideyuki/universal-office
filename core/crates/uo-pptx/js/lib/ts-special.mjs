// B5b: the pictures the browser cannot decode (metafiles, PICT, TIFF) as the frozen viewer makes them: `rasterizeSpecialImage` of
// raster.ts over a canvas, once per part, at the display size of the first picture that uses the part (`picDisplaySize`,
// universal-slides-viewer.ts). The canvas is a recording canvas of a recorder of its own, so the slide's stream does not contain
// the lines that built it: the slide draws it as an image identified by its recipe hash.
import { join } from 'node:path';
import { pathToFileURL } from 'node:url';
import { createRecorder } from './record-ctx.mjs';

const mods = new Map();
let runs = 0;
async function dist(root, name, fresh) {
  const href = pathToFileURL(join(root, 'slides', 'packages', 'slides-core', 'dist', name)).href + (fresh ? `?special=${++runs}` : '');
  if (fresh) return import(href);
  if (!mods.has(href)) mods.set(href, import(href));
  return mods.get(href);
}

/** `picDisplaySize(sh)` of the viewer. */
export function picDisplaySize(sh) {
  const x = sh.xfrm;
  if (!x || !(x.cx > 0) || !(x.cy > 0)) return undefined;
  const c = sh.srcRect;
  const fw = c ? Math.max(0.05, 1 - c.l - c.r) : 1, fh = c ? Math.max(0.05, 1 - c.t - c.b) : 1;
  return { w: x.cx / 9525 / fw, h: x.cy / 9525 / fh };
}

/** The parts of a slide in the order the viewer warms them, each with the display of its first use (`undefined` = none). */
export function warmOrder(slide) {
  const out = new Map();
  const add = (ref, display) => { if (ref?.part && !out.has(ref.part)) out.set(ref.part, display); };
  add(slide.bgImage);
  for (const sh of slide.shapes ?? []) {
    add(sh.image, picDisplaySize(sh));
    add(sh.fillImage);
    for (const p of sh.paragraphs ?? []) { add(p.bulletImage); for (const r of p.runs ?? []) add(r.fillImage); }
  }
  return out;
}

/**
 * `rasterizeSpecialImage(bytes, mk, display)` on a fresh recorder (and a fresh metafile module: its font memo is module state):
 * the canvas, or null when the bytes are not a metafile, a PICT or a TIFF. `caps` as the capture's.
 */
export async function rasterSpecial(root, bytes, display, caps) {
  const meta = await dist(root, 'metafile.js', true);
  const pict = await dist(root, 'pict.js');
  const tiff = await dist(root, 'tiff.js');
  const rec = createRecorder({ width: 8, height: 8, caps });
  rec.install();
  try {
    const mk = (w, h) => rec.createCanvas(w, h);
    let cv = meta.rasterizeMetafile(bytes, mk, display ? { display } : {});
    if (!cv && pict.isPict(bytes)) cv = pict.rasterizePict(bytes, mk);
    if (!cv && tiff.isTiff(bytes)) {
      let img;
      try { img = tiff.decodeTiff(bytes); } catch { img = undefined; }
      if (img) {
        const c = mk(img.width, img.height);
        const cx = c?.getContext('2d');
        if (c && cx) { const id = cx.createImageData(img.width, img.height); id.data.set(img.data); cx.putImageData(id, 0, 0); cv = c; }
      }
    }
    return cv ?? null;
  } finally {
    rec.uninstall();
  }
}
