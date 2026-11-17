// The JS side of the Slides exports of uo-wasm (src/slides.rs, feature `slides`; record
// core/docs/modules/uo-bindings.md "Slides viewer (B7)"): the deck, view, show and timeline handles with explicit free()
// and a FinalizationRegistry, the decoders of the result layouts and the module loader of the lazily loaded `.ppt`
// converter. Plain ES module (the Node and WebKit tests import it as is); slides.d.mts types it for the TS engine layer.
//
//   const uo = bindSlides((await WebAssembly.instantiate(bytes, {})).instance.exports);
//   const deck = uo.open(pptxBytes);                 // throws SlidesError (code 14 = a .ppt: see convertPpt)
//   const view = uo.newView(1);                      // one generation of the list stream
//   let r = deck.emit(view, 0, scale, ox, oy);       // { status: 0, flags, charts, message } | { status: 12, requests } | { status }
//   while (r.status === 12) { deck.answer(Float64Array.from(answersFor(r.requests))); r = deck.emit(...); }
//   view.free(); deck.free();

import { bindRender } from './render.mjs';

export const STATUS = { 0: 'ok', 1: 'none', 3: 'the deck could not be read', 4: 'limit exceeded', 5: 'invalid argument', 6: 'no such slide, part or media', 10: 'the list did not encode', 11: 'null handle', 12: 'measurements needed', 14: 'not an OOXML package (a .ppt)' };

/** An error of a Slides export: `status` (the table above) and the module's message. */
export class SlidesError extends Error {
  constructor(status, what, detail) {
    super(`${what}: ${STATUS[status] ?? status}${detail ? ` (${detail})` : ''}`);
    this.status = status;
    this.detail = detail ?? '';
  }
}

const enc = new TextEncoder();
const dec = new TextDecoder();

const KERNING = ['current', 'none', 'normal'];
const ACTIONS = ['none', 'transition', 'step', 'slide', 'end', 'exit'];

export function bindSlides(w) {
  const r = bindRender(w);
  const { Buf, put } = r;
  const deckRegistry = new FinalizationRegistry((ptr) => w.uo_slides_deck_free(ptr));
  const viewRegistry = new FinalizationRegistry((ptr) => w.uo_slides_view_free(ptr));
  const showRegistry = new FinalizationRegistry((ptr) => w.uo_slides_show_free(ptr));
  const timelineRegistry = new FinalizationRegistry((ptr) => w.uo_slides_timeline_free(ptr));

  const arg = (bytes) => (bytes.length === 0 ? [0, 0] : [put(bytes), bytes.length]);
  const take = (ptr) => new Buf(ptr).take();
  const dv = (data) => new DataView(data.buffer, data.byteOffset, data.byteLength);
  const reader = (data) => {
    const d = dv(data);
    let at = 0;
    return {
      get at() { return at; },
      u8: () => d.getUint8(at++),
      u32: () => { const v = d.getUint32(at, true); at += 4; return v; },
      i32: () => { const v = d.getInt32(at, true); at += 4; return v; },
      f64: () => { const v = d.getFloat64(at, true); at += 8; return v; },
      str: () => { const n = d.getUint32(at, true); at += 4; const s = dec.decode(data.subarray(at, at + n)); at += n; return s; },
      bytes: (n) => { const b = data.subarray(at, at + n); at += n; return b; },
      rest: () => data.subarray(at),
    };
  };
  const ok = (b, what) => {
    const { status, data } = b;
    if (status !== 0) throw new SlidesError(status, what);
    return data;
  };

  const face = (rd) => ({ family: rd.str(), bold: rd.u8() !== 0, italic: rd.u8() !== 0 });
  const key = (rd) => {
    const f = face(rd);
    return { face: f, sizePx: rd.f64(), kerning: KERNING[rd.u8()] ?? 'current', letterSpacingPx: rd.f64(), text: rd.str() };
  };
  /** the measurement requests of a status-12 result */
  const requests = (data) => {
    const rd = reader(data);
    const n = rd.u32();
    const out = [];
    for (let i = 0; i < n; i++) {
      const tag = rd.u8();
      if (tag === 1) out.push({ kind: 'charAt1000', face: face(rd), ch: String.fromCodePoint(rd.u32()) });
      else if (tag === 2) out.push({ kind: 'kernAt1000', face: face(rd), text: rd.str() });
      else if (tag === 3) out.push({ kind: 'width', ...key(rd) });
      else if (tag === 4) out.push({ kind: 'metrics', ...key(rd) });
      else if (tag === 5) out.push({ kind: 'fontAvailable', family: rd.str() });
      else if (tag === 6) out.push({ kind: 'imageSize', media: rd.u32(), part: rd.str() });
      else if (tag === 7) out.push({ kind: 'metaText', font: rd.str(), text: rd.str() });
      else if (tag === 8) {
        const key = rd.str();
        const n = rd.u32();
        out.push({ kind: 'metaDecode', key, data: rd.bytes(n) });
      }
      else throw new Error(`slides: unknown request tag ${tag}`);
    }
    return out;
  };

  const transition = (rd) => {
    if (rd.u8() === 0) return null;
    const kind = rd.str(), ns = rd.str();
    const n = rd.u32();
    const attrs = {};
    for (let i = 0; i < n; i++) { const k = rd.str(); attrs[k] = rd.str(); }
    const durMs = rd.f64(), adv = rd.f64(), advClick = rd.u8() !== 0;
    return { type: kind, ns, attrs, durMs, ...(Number.isNaN(adv) ? {} : { advTmMs: adv }), advClick };
  };

  class SlidesView {
    #ptr;
    constructor(ptr) {
      if (!ptr) throw new Error('null view');
      this.#ptr = ptr;
      viewRegistry.register(this, ptr, this);
    }
    get ptr() { return this.#ptr; }
    /** the RGBA bytes of a generated bitmap (`Bitmap.data` of the decoded tables) */
    bitmap(handle) {
      const b = take(w.uo_slides_view_bitmap(this.#ptr, handle));
      return b.status === 0 ? b.data : null;
    }
    free() {
      if (this.#ptr === 0) return;
      viewRegistry.unregister(this);
      w.uo_slides_view_free(this.#ptr);
      this.#ptr = 0;
    }
  }

  class SlidesShow {
    #ptr;
    constructor(ptr) {
      if (!ptr) throw new Error('null show');
      this.#ptr = ptr;
      showRegistry.register(this, ptr, this);
    }
    #cmd(c, arg2, now, mouse) {
      const a = w.uo_slides_show_command(this.#ptr, c, arg2, now, mouse ? 1 : 0);
      if (a < 0) throw new SlidesError(-a, 'show command');
      return ACTIONS[a];
    }
    next(now, mouse = false) { return this.#cmd(0, 0, now, mouse); }
    prev(now) { return this.#cmd(1, 0, now, false); }
    goTo(index, now) { return this.#cmd(2, index, now, false); }
    first(now) { return this.#cmd(3, 0, now, false); }
    last(now) { return this.#cmd(4, 0, now, false); }
    update(now) { return this.#cmd(5, 0, now, false); }
    dueAt() { return w.uo_slides_show_due_at(this.#ptr); }
    wakeAt(now) { return w.uo_slides_show_wake_at(this.#ptr, now); }
    /** the frame at `now`: the slide on stage (`index`, -1 = the end screen), the animation clock, the transition */
    frame(now) {
      const rd = reader(ok(take(w.uo_slides_show_frame(this.#ptr, now)), 'show frame'));
      const index = rd.i32(), step = rd.u32(), steps = rd.u32(), ended = rd.u8() !== 0, animMs = rd.f64(), progress = rd.f64(), effects = rd.u32();
      let from = null;
      if (rd.u8() === 1) from = { index: rd.i32(), animMs: rd.f64(), key: rd.u32(), effects: rd.u32() };
      return { index, step, steps, ended, animMs, transitionProgress: progress, effects, from, transition: transition(rd) };
    }
    free() {
      if (this.#ptr === 0) return;
      showRegistry.unregister(this);
      w.uo_slides_show_free(this.#ptr);
      this.#ptr = 0;
    }
  }

  class SlidesTimeline {
    #ptr;
    constructor(ptr) {
      if (!ptr) throw new Error('null timeline');
      this.#ptr = ptr;
      timelineRegistry.register(this, ptr, this);
    }
    info() {
      const rd = reader(ok(take(w.uo_slides_timeline_info(this.#ptr)), 'timeline info'));
      const durationMs = rd.f64(), tickMs = rd.f64(), mediaClock = rd.f64(), n = rd.u32();
      const slides = [];
      for (let i = 0; i < n; i++) slides.push({ index: rd.u32(), startMs: rd.f64(), endMs: rd.f64(), animStartMs: rd.f64(), transitionMs: rd.f64() });
      return { durationMs, tickMs, mediaClock, slides };
    }
    stateAt(t) {
      const rd = reader(ok(take(w.uo_slides_timeline_state(this.#ptr, t)), 'timeline state'));
      const pos = rd.i32(), index = rd.i32(), from = rd.i32(), progress = rd.f64(), animMs = rd.f64(), ended = rd.u8() !== 0, effects = rd.u32();
      return { pos, index, from, transitionProgress: progress, animMs, ended, effects, transition: transition(rd) };
    }
    free() {
      if (this.#ptr === 0) return;
      timelineRegistry.unregister(this);
      w.uo_slides_timeline_free(this.#ptr);
      this.#ptr = 0;
    }
  }

  class SlidesDeck {
    #ptr;
    constructor(ptr) {
      if (!ptr) throw new Error('null deck');
      this.#ptr = ptr;
      deckRegistry.register(this, ptr, this);
    }
    get ptr() { return this.#ptr; }
    get count() { return w.uo_slides_count(this.#ptr); }
    /** `{ w, h, count }`: the slide size in EMU */
    size() {
      const rd = reader(ok(take(w.uo_slides_size(this.#ptr)), 'size'));
      return { w: rd.f64(), h: rd.f64(), count: rd.u32() };
    }
    notes(i) {
      const b = take(w.uo_slides_notes(this.#ptr, i));
      return b.status === 0 ? dec.decode(b.data) : '';
    }
    /** the bytes of a package part, or null */
    part(name) {
      const [p, n] = arg(enc.encode(name));
      const b = take(w.uo_slides_part(this.#ptr, p, n));
      return b.status === 0 ? b.data : null;
    }
    /** `{ fontKerning, letterSpacing, segmenter, fontBoundingBox }` of the canvas (changing them empties the answers) */
    setCaps(c) {
      const bits = (c.fontKerning ? 1 : 0) | (c.letterSpacing ? 2 : 0) | (c.segmenter ? 4 : 0) | (c.fontBoundingBox ? 8 : 0);
      const s = w.uo_slides_set_caps(this.#ptr, bits);
      if (s !== 0) throw new SlidesError(s, 'set_caps');
    }
    setGeneration(g) {
      const s = w.uo_slides_set_generation(this.#ptr, g);
      if (s !== 0) throw new SlidesError(s, 'set_generation');
    }
    setImageSize(media, wpx, hpx) {
      const s = w.uo_slides_set_image_size(this.#ptr, media, wpx, hpx);
      if (s !== 0) throw new SlidesError(s, 'set_image_size');
    }
    clearCaches() {
      const s = w.uo_slides_clear_caches(this.#ptr);
      if (s !== 0) throw new SlidesError(s, 'clear_caches');
    }
    /**
     * One pass of the emission: `{ status: 0, flags, newGeneration, charts, message }` (the list message for
     * js/gen/slides.ts), `{ status: 12, requests }` (answer them, emit again) or `{ status }` for an error status.
     */
    emit(view, index, scale, ox, oy) {
      const b = take(w.uo_slides_emit(this.#ptr, view.ptr, index, scale, ox, oy));
      if (b.status === 0) {
        const rd = reader(b.data);
        const flags = rd.u8(), n = rd.u32();
        const charts = [];
        for (let i = 0; i < n; i++) charts.push({ shape: rd.u32(), x: rd.f64(), y: rd.f64(), w: rd.f64(), h: rd.f64() });
        return { status: 0, flags, newGeneration: (flags & 1) !== 0, charts, message: rd.rest() };
      }
      if (b.status === 12) return { status: 12, requests: requests(b.data) };
      return { status: b.status };
    }
    /** the answers to the requests of the last status-12 emit, in order (layout in src/slides.rs) */
    answer(nums) {
      const bytes = new Uint8Array(nums.buffer, nums.byteOffset, nums.byteLength);
      const [p, n] = arg(bytes.slice());
      const s = w.uo_slides_answer(this.#ptr, p, n);
      if (s !== 0) throw new SlidesError(s, 'answer');
    }
    /** a click slide show; `from` -1 = the first visible slide, flags -1 default / 0 / 1, `range` = [lo, hi] or null */
    newShow({ from = -1, now = 0, useTimings = -1, loop = -1, animations = -1, range = null } = {}) {
      const p = w.uo_slides_show_new(this.#ptr, from, now, useTimings, loop, animations, range ? range[0] : -1, range ? range[1] : -1);
      return new SlidesShow(p);
    }
    newTimeline({ useTimings = -1, defaultSlideMs = NaN, tickMs = NaN, transitionFps = NaN } = {}) {
      return new SlidesTimeline(w.uo_slides_timeline_new(this.#ptr, useTimings, defaultSlideMs, tickMs, transitionFps));
    }
    free() {
      if (this.#ptr === 0) return;
      deckRegistry.unregister(this);
      w.uo_slides_deck_free(this.#ptr);
      this.#ptr = 0;
    }
  }

  const lastError = (what) => {
    const { status, data } = take(w.uo_slides_error());
    return new SlidesError(status || 3, what, dec.decode(data));
  };

  return {
    /** opens a `.pptx`; throws a SlidesError (status 14: a legacy `.ppt`, convert it with `bindSlidesPpt` first) */
    open(bytes) {
      const [p, n] = arg(bytes.slice());
      const ptr = w.uo_slides_open(p, n);
      if (!ptr) throw lastError('open');
      return new SlidesDeck(ptr);
    },
    newView(generation) { return new SlidesView(w.uo_slides_view_new(generation)); },
    maxRounds: w.uo_slides_max_rounds(),
    /** the live bytes of the module (the count build), or -1 */
    liveBytes: () => w.uo_memory_live_bytes(),
    SlidesDeck,
    SlidesView,
  };
}

/** The `.ppt` converter module (`uo-slides-ppt.wasm`, feature `slides-ppt`): `convert(pptBytes)` -> the `.pptx` bytes. */
export function bindSlidesPpt(w) {
  const r = bindRender(w);
  const { Buf, put } = r;
  return {
    convert(bytes) {
      const p = bytes.length === 0 ? 0 : put(bytes);
      const { status, data } = new Buf(w.uo_slides_ppt_convert(p, bytes.length)).take();
      if (status !== 0) throw new SlidesError(status, 'ppt convert', dec.decode(data));
      return data;
    },
  };
}
