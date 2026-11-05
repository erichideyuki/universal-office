// The JS side of the render exports of uo-wasm (src/render.rs, uo-render frontier E): the view and
// result handles with explicit free() and a FinalizationRegistry, and the decoders of the result
// layouts. Plain ES module (no TypeScript, so the Node and WebKit tests import it as is); the TS
// shell (frontier D) wraps or ports it.
//
//   const r = bindRender(instance.exports);
//   const view = r.fixture(1);                // tests; a real view comes from the layout handle
//   const layer = r.fxLayer(view.fxLayers(0, 0, 2));   // { x0, y0, w, h, data } | null
//   view.free();

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
  }

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
    /** `maskToCanvas(mask, hex, alpha)`: `mask` a Float32Array, `hex` a string */
    coloredMask(mask, width, height, hex, alpha) {
      const hexBytes = new Uint8Array(hex.length * 2);
      for (let i = 0; i < hex.length; i++) { hexBytes[i * 2] = hex.charCodeAt(i) & 0xff; hexBytes[i * 2 + 1] = hex.charCodeAt(i) >> 8; }
      const maskBytes = new Uint8Array(mask.buffer, mask.byteOffset, mask.byteLength);
      return ok(new Buf(w.uo_render_colored_mask(put(maskBytes), maskBytes.length, width, height, put(hexBytes), hexBytes.length, alpha)), 'colored_mask');
    },
    /** `DASH[name]` of the picture outline, or null */
    dash(name) {
      const b = new Uint8Array(name.length * 2);
      for (let i = 0; i < name.length; i++) { b[i * 2] = name.charCodeAt(i) & 0xff; b[i * 2 + 1] = name.charCodeAt(i) >> 8; }
      const data = ok(new Buf(w.uo_render_dash(put(b), b.length)), 'dash');
      if (!data) return null;
      const d = dv(data);
      return Array.from({ length: d.getUint32(0, true) }, (_, i) => d.getFloat64(4 + i * 8, true));
    },
  };
}

/** FNV-1a 32 of bytes, as 8 hex digits (the checksum the tests pin; the native test computes the same). */
export function fnv(bytes) {
  let h = 0x811c9dc5;
  for (let i = 0; i < bytes.length; i++) h = Math.imul(h ^ bytes[i], 0x01000193);
  return (h >>> 0).toString(16).padStart(8, '0');
}
