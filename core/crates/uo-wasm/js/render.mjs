// The JS side of the render exports of uo-wasm (src/render.rs, uo-render frontier E): the view and
// result handles with explicit free() and a FinalizationRegistry, and the decoders of the result
// layouts. Plain ES module (no TypeScript, so the Node and WebKit tests import it as is); the TS
// shell (frontier D) wraps or ports it.
//
//   const r = bindRender(instance.exports);
//   const view = r.fixture(1);                // tests; a real view comes from the layout handle
//   const layer = r.fxLayer(view.fxLayers(0, 0, 2));   // { x0, y0, w, h, data } | null
//   view.free();
//
//   // the decoded facade the TS painter (paint.ts) takes as its `RasterSource`: every Buf taken and freed, null for
//   // "nothing to paint" (status 1) and for a limit (status 4), a RenderError for the other statuses
//   const rasters = r.rasters(view.paint);

const STATUS = { 0: 'ok', 1: 'none', 2: 'version mismatch', 3: 'malformed message', 4: 'limit exceeded', 5: 'invalid argument', 6: 'no such page or item', 7: 'wrong item', 8: 'missing resource', 9: 'not a font', 10: 'page did not encode', 11: 'null handle' };

export class RenderError extends Error {
  constructor(status, what) {
    super(`${what}: ${STATUS[status] ?? status}`);
    this.status = status;
  }
}

export function bindRender(w) {
  const viewRegistry = new FinalizationRegistry((ptr) => w.uo_render_view_free(ptr));
  const bufRegistry = new FinalizationRegistry((ptr) => w.uo_buf_free(ptr));
  const put = (bytes) => {
    const p = w.uo_alloc_bytes(bytes.length);
    new Uint8Array(w.memory.buffer, p, bytes.length).set(bytes);
    return p;
  };

  /** A result: status and bytes in wasm memory until freed. */
  class Buf {
    #ptr;
    constructor(ptr) {
      this.#ptr = ptr;
      bufRegistry.register(this, ptr, this);
    }
    get status() { return w.uo_buf_status(this.#ptr); }
    get length() { return w.uo_buf_len(this.#ptr); }
    /** a copy of the bytes (the memory may move when wasm grows) */
    bytes() {
      const n = w.uo_buf_len(this.#ptr);
      return n === 0 ? new Uint8Array(0) : new Uint8Array(w.memory.buffer, w.uo_buf_ptr(this.#ptr), n).slice();
    }
    free() {
      if (this.#ptr === 0) return;
      bufRegistry.unregister(this);
      w.uo_buf_free(this.#ptr);
      this.#ptr = 0;
    }
    /** status + a copy of the bytes, then free */
    take() {
      const out = { status: this.status, data: this.bytes() };
      this.free();
      return out;
    }
  }

  const ok = (b, what) => {
    const { status, data } = b.take();
    if (status === 1) return null;
    if (status !== 0) throw new RenderError(status, what);
    return data;
  };

  class RenderView {
    #ptr;
    constructor(ptr) {
      if (!ptr) throw new Error('null view');
      this.#ptr = ptr;
      viewRegistry.register(this, ptr, this);
    }
    get ptr() { return this.#ptr; }
    get pageCount() { return w.uo_render_view_page_count(this.#ptr); }
    free() {
      if (this.#ptr === 0) return;
      viewRegistry.unregister(this);
      w.uo_render_view_free(this.#ptr);
      this.#ptr = 0;
    }
    /** Appends a page from an encoded message; the page index. */
    pushMessage(bytes) {
      const r = w.uo_render_view_push_message(this.#ptr, put(bytes), bytes.length);
      if (r < 0) throw new RenderError(-r, 'push_message');
      return r;
    }
    setFaceBytes(face, bytes) {
      const s = w.uo_render_view_set_face_bytes(this.#ptr, face, put(bytes), bytes.length);
      if (s !== 0) throw new RenderError(s, 'set_face_bytes');
    }
    setImageBytes(image, bytes, naturalW = 0, naturalH = 0) {
      const s = w.uo_render_view_set_image_bytes(this.#ptr, image, put(bytes), bytes.length, naturalW, naturalH);
      if (s !== 0) throw new RenderError(s, 'set_image_bytes');
    }
    setApp(page, bytes) {
      const s = w.uo_render_view_set_app(this.#ptr, page, put(bytes), bytes.length);
      if (s !== 0) throw new RenderError(s, 'set_app');
    }
    setMaxFxPx(px) {
      const s = w.uo_render_view_set_max_fx_px(this.#ptr, px);
      if (s !== 0) throw new RenderError(s, 'set_max_fx_px');
    }
    // the raw operations: a Buf each (call take()/free() on it)
    encodePage(page) { return new Buf(w.uo_render_encode_page(this.#ptr, page)); }
    fxLayers(page, item, ppt) { return new Buf(w.uo_render_fx_layers(this.#ptr, page, item, ppt)); }
    pattern(page, item) { return new Buf(w.uo_render_pattern(this.#ptr, page, item)); }
    rectGradient(page, item) { return new Buf(w.uo_render_rect_gradient(this.#ptr, page, item)); }
    blipTiles(page, item) { return new Buf(w.uo_render_blip_tiles(this.#ptr, page, item)); }
    pageFillTileTw(page, devicePxTw) { return new Buf(w.uo_render_page_fill_tile_tw(this.#ptr, page, devicePxTw)); }
    pageFillRaster(page) { return new Buf(w.uo_render_page_fill_raster(this.#ptr, page)); }
    pictureBounds(page, item) { return new Buf(w.uo_render_picture_bounds(this.#ptr, page, item)); }
    pictureSubs(page, item) { return new Buf(w.uo_render_picture_subs(this.#ptr, page, item)); }
    /** the item-addressed operations (a Buf each), the object `rasters()` takes */
    get paint() { return paintOps(() => this.#ptr); }
  }

  /** bytes in: `[ptr, len]` of a consumed input buffer */
  const putBytes = (bytes) => (bytes.length === 0 ? [0, 0] : [put(bytes), bytes.length]);
  const putF32 = (a) => putBytes(new Uint8Array(a.buffer, a.byteOffset, a.byteLength));
  const putU8 = (a) => putBytes(new Uint8Array(a.buffer, a.byteOffset, a.byteLength));
  const utf16 = (s) => {
    const b = new Uint8Array(s.length * 2);
    for (let i = 0; i < s.length; i++) { b[i * 2] = s.charCodeAt(i) & 0xff; b[i * 2 + 1] = s.charCodeAt(i) >> 8; }
    return b;
  };

  /**
   * The item-addressed paint operations on the view `v()` returns (a Buf each): the zoom-dependent rasters, the picture
   * helpers and the picture-layer masks (frontier D). `RenderView.paint` and the layout's `paint` are this.
   */
  const paintOps = (v) => ({
    encodePage: (page) => new Buf(w.uo_render_encode_page(v(), page)),
    fxLayers: (page, item, ppt) => new Buf(w.uo_render_fx_layers(v(), page, item, ppt)),
    pattern: (page, item) => new Buf(w.uo_render_pattern(v(), page, item)),
    rectGradient: (page, item) => new Buf(w.uo_render_rect_gradient(v(), page, item)),
    blipTiles: (page, item) => new Buf(w.uo_render_blip_tiles(v(), page, item)),
    pageFillTileTw: (page, devicePxTw) => new Buf(w.uo_render_page_fill_tile_tw(v(), page, devicePxTw)),
    pageFillRaster: (page) => new Buf(w.uo_render_page_fill_raster(v(), page)),
    pictureBounds: (page, item) => new Buf(w.uo_render_picture_bounds(v(), page, item)),
    pictureSubs: (page, item) => new Buf(w.uo_render_picture_subs(v(), page, item)),
    picturePolys: (page, item, ppt, ox, oy) => new Buf(w.uo_render_picture_polys(v(), page, item, ppt, ox, oy)),
    pictureSilhouette: (page, item, ppt, ox, oy, W, H) => new Buf(w.uo_render_picture_silhouette(v(), page, item, ppt, ox, oy, W, H)),
    pictureBevel: (page, item, ppt, ox, oy, W, H, rgba) => new Buf(w.uo_render_picture_bevel(v(), page, item, ppt, ox, oy, W, H, ...putU8(rgba))),
    pictureAdjust: (page, item, rgba) => new Buf(w.uo_render_picture_adjust(v(), page, item, ...putU8(rgba))),
    outerShadow: (page, item, sil, W, H, box, dirDeg, ppt) => new Buf(w.uo_render_outer_shadow(v(), page, item, ...putF32(sil), W, H, box.x, box.y, box.w, box.h, dirDeg, ppt)),
    reflectionAlphas: (page, item, top, hPpt, rows) => new Buf(w.uo_render_reflection_alphas(v(), page, item, top, hPpt, rows)),
    innerShadow: (sil, W, H, blurPt, distPt, dirDeg, ppt) => new Buf(w.uo_render_inner_shadow(v(), ...putF32(sil), W, H, blurPt, distPt, dirDeg, ppt)),
    glow: (sil, W, H, radPt, ppt) => new Buf(w.uo_render_glow(v(), ...putF32(sil), W, H, radPt, ppt)),
    softEdge: (sil, W, H, radPt, ppt) => new Buf(w.uo_render_soft_edge(v(), ...putF32(sil), W, H, radPt, ppt)),
    gaussianBlur: (m, W, H, sigma) => new Buf(w.uo_render_gaussian_blur(v(), ...putF32(m), W, H, sigma)),
    setMaxFxPx: (px) => w.uo_render_view_set_max_fx_px(v(), px),
  });

  const dv = (data) => new DataView(data.buffer, data.byteOffset, data.byteLength);
  const decoders = {
    /** `x0, y0` (twips), `w, h`, RGBA */
    fxLayer(data) {
      if (!data) return null;
      const d = dv(data);
      const w_ = d.getUint32(16, true), h = d.getUint32(20, true);
      return { x0: d.getFloat64(0, true), y0: d.getFloat64(8, true), w: w_, h, data: data.subarray(24, 24 + w_ * h * 4) };
    },
    /** the pattern raster: grid corner, size in cells, pixel size, RGBA */
    pattern(data) {
      if (!data) return null;
      const d = dv(data);
      const w_ = d.getUint32(32, true), h = d.getUint32(36, true);
      return { x: d.getFloat64(0, true), y: d.getFloat64(8, true), cellsW: d.getFloat64(16, true), cellsH: d.getFloat64(24, true), w: w_, h, data: data.subarray(40, 40 + w_ * h * 4) };
    },
    /** `w, h`, RGBA (the rectangular gradient and the page-fill source tile) */
    raster(data) {
      if (!data) return null;
      const d = dv(data);
      const w_ = d.getUint32(0, true), h = d.getUint32(4, true);
      return { w: w_, h, data: data.subarray(8, 8 + w_ * h * 4) };
    },
    blipTiles(data) {
      const d = dv(data);
      const n = d.getUint32(0, true);
      return Array.from({ length: n }, (_, i) => ({ x: d.getFloat64(4 + i * 32, true), y: d.getFloat64(12 + i * 32, true), w: d.getFloat64(20 + i * 32, true), h: d.getFloat64(28 + i * 32, true) }));
    },
    /** a mask: `w * h` coverage values (a copy, so it survives memory growth) */
    mask(data) { return new Float32Array(data.buffer.slice(data.byteOffset, data.byteOffset + data.byteLength)); },
    /** polygons: one Float64Array of `[x0, y0, x1, y1, ...]` each */
    polygons(data) {
      const d = dv(data);
      const n = d.getUint32(0, true);
      let at = 4;
      const out = [];
      for (let i = 0; i < n; i++) {
        const m = d.getUint32(at, true);
        at += 4;
        const p = new Float64Array(m);
        for (let j = 0; j < m; j++, at += 8) p[j] = d.getFloat64(at, true);
        out.push(p);
      }
      return out;
    },
    f64s(data) { return new Float64Array(data.buffer.slice(data.byteOffset, data.byteOffset + data.byteLength)); },
    pair(data) { if (!data) return null; const d = dv(data); return { w: d.getFloat64(0, true), h: d.getFloat64(8, true) }; },
    bounds(data) { const d = dv(data); return { x0: d.getFloat64(0, true), y0: d.getFloat64(8, true), x1: d.getFloat64(16, true), y1: d.getFloat64(24, true) }; },
    subpaths(data) {
      const d = dv(data);
      let at = 0;
      const u32 = () => { const v = d.getUint32(at, true); at += 4; return v; };
      const f64 = () => { const v = d.getFloat64(at, true); at += 8; return v; };
      const out = [];
      for (let i = 0, n = u32(); i < n; i++) {
        const sub = [];
        for (let j = 0, m = u32(); j < m; j++) {
          const c = String.fromCharCode(d.getUint8(at++));
          if (c === 'M' || c === 'L') sub.push({ c, x: f64(), y: f64() });
          else if (c === 'C') sub.push({ c, x1: f64(), y1: f64(), x2: f64(), y2: f64(), x: f64(), y: f64() });
          else if (c === 'Q') sub.push({ c, x1: f64(), y1: f64(), x: f64(), y: f64() });
          else if (c === 'E') { const o = { c, cx: f64(), cy: f64(), rx: f64(), ry: f64(), a0: f64(), a1: f64() }; o.ccw = d.getUint8(at++) === 1; sub.push(o); } else sub.push({ c });
        }
        out.push(sub);
      }
      return out;
    },
  };

  /** the decoded `DASH[name]` array of a dash result */
  const dashArray = (d) => { const v = dv(d); return Array.from({ length: v.getUint32(0, true) }, (_, i) => v.getFloat64(4 + i * 8, true)); };

  const coloredMaskBuf = (mask, width, height, hex, alpha) => {
    const hexBytes = utf16(hex);
    const maskBytes = new Uint8Array(mask.buffer, mask.byteOffset, mask.byteLength);
    return new Buf(w.uo_render_colored_mask(put(maskBytes), maskBytes.length, width, height, put(hexBytes), hexBytes.length, alpha));
  };

  /**
   * The decoded facade over `ops` (a view's `paint`, or a layout's): what the TS painter takes as its `RasterSource`.
   * Every Buf is taken and freed here. A result is `null` for status 1 (nothing to paint) and 4 (limit: paint nothing and
   * keep going — the TS has no limits and would exhaust memory); the other statuses are bugs of the caller and throw a
   * `RenderError`, except `rectGradient` with status 5 (a NaN side: null, decision D-6 of the record).
   */
  function rasters(ops) {
    const lenient = (b, what, soft = []) => {
      const { status, data } = b.take();
      if (status === 1 || status === 4 || soft.includes(status)) return null;
      if (status !== 0) throw new RenderError(status, what);
      return data;
    };
    const dashes = new Map();
    const dec = (b, what, f) => { const d = lenient(b, what); return d ? f(d) : null; };
    return {
      fxLayer: (page, item, ppt) => dec(ops.fxLayers(page, item, ppt), 'fxLayers', decoders.fxLayer),
      pattern: (page, item) => dec(ops.pattern(page, item), 'pattern', decoders.pattern),
      rectGradient: (page, item) => { const d = lenient(ops.rectGradient(page, item), 'rectGradient', [5]); return d ? decoders.raster(d) : null; },
      blipTiles: (page, item) => dec(ops.blipTiles(page, item), 'blipTiles', decoders.blipTiles),
      pageFillTileTw: (page, devicePxTw) => dec(ops.pageFillTileTw(page, devicePxTw), 'pageFillTileTw', decoders.pair),
      pageFillRaster: (page) => dec(ops.pageFillRaster(page), 'pageFillRaster', decoders.raster),
      pictureBounds: (page, item) => dec(ops.pictureBounds(page, item), 'pictureBounds', decoders.bounds),
      picturePolys: (page, item, ppt, ox, oy) => dec(ops.picturePolys(page, item, ppt, ox, oy), 'picturePolys', decoders.polygons),
      pictureSilhouette: (page, item, ppt, ox, oy, W, H) => dec(ops.pictureSilhouette(page, item, ppt, ox, oy, W, H), 'pictureSilhouette', decoders.mask),
      pictureBevel: (page, item, ppt, ox, oy, W, H, rgba) => lenient(ops.pictureBevel(page, item, ppt, ox, oy, W, H, rgba), 'pictureBevel'),
      pictureAdjust: (page, item, rgba) => lenient(ops.pictureAdjust(page, item, rgba), 'pictureAdjust'),
      outerShadow: (page, item, sil, W, H, box, dirDeg, ppt) => dec(ops.outerShadow(page, item, sil, W, H, box, dirDeg, ppt), 'outerShadow', decoders.mask),
      reflectionAlphas: (page, item, top, hPpt, rows) => dec(ops.reflectionAlphas(page, item, top, hPpt, rows), 'reflectionAlphas', decoders.f64s),
      innerShadow: (sil, W, H, blurPt, distPt, dirDeg, ppt) => dec(ops.innerShadow(sil, W, H, blurPt, distPt, dirDeg, ppt), 'innerShadow', decoders.mask),
      glow: (sil, W, H, radPt, ppt) => dec(ops.glow(sil, W, H, radPt, ppt), 'glow', decoders.mask),
      softEdge: (sil, W, H, radPt, ppt) => dec(ops.softEdge(sil, W, H, radPt, ppt), 'softEdge', decoders.mask),
      gaussianBlur: (m, W, H, sigma) => dec(ops.gaussianBlur(m, W, H, sigma), 'gaussianBlur', decoders.mask),
      coloredMask: (mask, W, H, hex, alpha) => lenient(coloredMaskBuf(mask, W, H, hex, alpha), 'coloredMask'),
      /** `DASH[name]` of the picture outline (a small cache: the table has nine names) */
      dash(name) {
        const hit = dashes.get(name);
        if (hit !== undefined) return hit;
        const d = lenient(new Buf(w.uo_render_dash(...putBytes(utf16(name)))), 'dash');
        const out = d ? dashArray(d) : null;
        if (dashes.size >= 64) dashes.clear();
        dashes.set(name, out);
        return out;
      },
    };
  }

  return {
    RenderView,
    Buf,
    ok,
    put,
    decoders,
    /** a new view fed by pushMessage */
    newView: (generation) => new RenderView(w.uo_render_view_new(generation)),
    /** the test fixture views (feature render-fixture) */
    fixture: (kind) => new RenderView(w.uo_render_fixture(kind)),
    paintOps,
    /** `maskToCanvas(mask, hex, alpha)`: `mask` a Float32Array, `hex` a string */
    coloredMask(mask, width, height, hex, alpha) {
      return ok(coloredMaskBuf(mask, width, height, hex, alpha), 'colored_mask');
    },
    rasters,
    /** `DASH[name]` of the picture outline, or null */
    dash(name) {
      const data = ok(new Buf(w.uo_render_dash(...putBytes(utf16(name)))), 'dash');
      return data ? dashArray(data) : null;
    },
  };
}

/** FNV-1a 32 of bytes, as 8 hex digits (the checksum the tests pin; the native test computes the same). */
export function fnv(bytes) {
  let h = 0x811c9dc5;
  for (let i = 0; i < bytes.length; i++) h = Math.imul(h ^ bytes[i], 0x01000193);
  return (h >>> 0).toString(16).padStart(8, '0');
}
