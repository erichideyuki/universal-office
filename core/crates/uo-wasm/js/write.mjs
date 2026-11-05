// The JS side of the Write exports of uo-wasm (src/write.rs, feature `write`; record
// core/docs/modules/uo-bindings.md): the environment (fonts, hyphenation), document and layout
// handles with explicit free() and a FinalizationRegistry, the decoders of the result layouts, and
// the host import that receives the PDF in chunks. Plain ES module (the Node and WebKit tests import
// it as is); write.d.ts types it for the TS viewer.
//
//   const { imports, host } = writeImports();
//   const { instance } = await WebAssembly.instantiate(bytes, imports);
//   const uo = bindWrite(instance.exports, host);
//   const env = uo.newEnv();
//   env.pushFamily('Carlito', { source: 'bundled', faces: [{ variant: 'regular', bytes }] });
//   const doc = uo.open(docxBytes);
//   const layout = doc.layout(env, { hyphenationLanguages: ['pt-BR', 'en', 'es'] });
//   const msg = layout.encodePage(0);            // the page message of page 0: js/gen/render.ts decodes it, its
//                                                // `app` is the Write section (js/gen/write.ts `decodeWritePage`)
//   const pdf = layout.pdf();                    // { chunks: Uint8Array[], length }
//   layout.free(); doc.free(); env.free();

import { bindRender } from './render.mjs';

const CODES = { 1: 'invalid argument', 2: 'invalid operation', 3: 'invalid data', 4: 'malformed XML', 5: 'limit exceeded', 6: 'format', 7: 'overflow', 8: 'JSON', 9: 'argument out of range', 10: 'cancelled', 11: 'no font', 12: 'internal error', 13: 'output refused' };
const STAGES = ['', 'read', 'layout', 'pdf'];
const VARIANTS = ['regular', 'bold', 'italic', 'boldItalic'];
const SOURCES = ['host', 'localAccess', 'bundled', 'page', 'installed', 'substitute', 'default'];

/** An error of a Write export: `code` (record "Error codes"), `stage`, `max` (a limit), `message`. */
export class WriteError extends Error {
  constructor(code, stage, max, message, what) {
    super(`${what}: ${CODES[code] ?? code}${message ? ` (${message})` : ''}`);
    this.code = code;
    this.stage = STAGES[stage] ?? String(stage);
    this.max = max;
    this.detail = message;
  }
}

/** The import object the module needs (`uo_host.pdf_chunk`) and the slot `bindWrite` fills. */
export function writeImports() {
  const host = { memory: null, sink: null };
  const imports = {
    uo_host: {
      // copies the chunk out of wasm memory (which may move when it grows) and calls nothing back
      pdf_chunk(ptr, len) {
        if (!host.sink) return 1;
        host.sink(new Uint8Array(host.memory.buffer, ptr, len).slice());
        return 0;
      },
    },
  };
  return { imports, host };
}

const enc = new TextEncoder();
const dec = new TextDecoder();

export function bindWrite(w, host) {
  host.memory = w.memory;
  const r = bindRender(w);
  const { Buf, put } = r;
  const envRegistry = new FinalizationRegistry((ptr) => w.uo_write_env_free(ptr));
  const docRegistry = new FinalizationRegistry((ptr) => w.uo_write_document_free(ptr));
  const layoutRegistry = new FinalizationRegistry((ptr) => w.uo_layout_free(ptr));

  const lastError = (what) => {
    const info = w.uo_alloc_bytes(24);
    const code = w.uo_last_error(info);
    const d = new DataView(w.memory.buffer, info, 24);
    const stage = d.getInt32(4, true), max = Number(d.getBigUint64(8, true)), n = Number(d.getBigUint64(16, true));
    w.uo_free_bytes(info, 24);
    let message = '';
    if (n > 0) {
      const p = w.uo_alloc_bytes(n);
      w.uo_last_error_message(p, n);
      message = dec.decode(new Uint8Array(w.memory.buffer, p, n).slice());
      w.uo_free_bytes(p, n);
    }
    w.uo_last_error_clear();
    return new WriteError(code, stage, max, message, what);
  };
  const check = (status, what) => { if (status !== 0) throw lastError(what); };
  /** a consumed input buffer: `[ptr, len]` */
  const arg = (bytes) => (bytes.length === 0 ? [0, 0] : [put(bytes), bytes.length]);
  const text = (s) => arg(enc.encode(s ?? ''));
  /** status + bytes of a Buf; null for "none"; throws on the other statuses */
  const take = (ptr, what) => {
    const { status, data } = new Buf(ptr).take();
    if (status === 1) return null;
    if (status !== 0) throw new Error(`${what}: status ${status}`);
    return data;
  };
  const dv = (data) => new DataView(data.buffer, data.byteOffset, data.byteLength);
  const reader = (data) => {
    const d = dv(data);
    let at = 0;
    return {
      u8: () => d.getUint8(at++),
      u32: () => { const v = d.getUint32(at, true); at += 4; return v; },
      i32: () => { const v = d.getInt32(at, true); at += 4; return v; },
      f64: () => { const v = d.getFloat64(at, true); at += 8; return v; },
      str: () => { const n = d.getUint32(at, true); at += 4; let s = ''; for (let i = 0; i < n; i++) { s += String.fromCharCode(d.getUint16(at, true)); at += 2; } return s; },
    };
  };
  const lines = (data) => (data && data.length ? dec.decode(data).split('\n').filter((l) => l) : []);

  class WriteEnv {
    #ptr;
    constructor(phone) {
      this.#ptr = w.uo_write_env_new(phone ? 1 : 0);
      envRegistry.register(this, this.#ptr, this);
    }
    get ptr() { return this.#ptr; }
    #live() { if (!this.#ptr) throw new Error('environment freed'); return this.#ptr; }
    free() {
      if (!this.#ptr) return;
      envRegistry.unregister(this);
      w.uo_write_env_free(this.#ptr);
      this.#ptr = 0;
    }
    /** `resolver.getWithFile(family)`: `{ source, embeddable, faces: [{ variant, bytes, ttcIndex? }] }` */
    pushFamily(family, { source = 'host', embeddable = true, faces = [] } = {}) {
      const parts = [];
      for (const f of faces) {
        const head = new Uint8Array(9);
        const d = new DataView(head.buffer);
        head[0] = VARIANTS.indexOf(f.variant ?? 'regular');
        d.setInt32(1, f.ttcIndex ?? -1, true);
        d.setUint32(5, f.bytes.length, true);
        parts.push(head, f.bytes);
      }
      const total = parts.reduce((n, p) => n + p.length, 0);
      const all = new Uint8Array(total);
      let at = 0;
      for (const p of parts) { all.set(p, at); at += p.length; }
      const s = SOURCES.indexOf(source);
      check(w.uo_write_env_push_family(this.#live(), ...text(family), s, embeddable ? 1 : 0, ...arg(all)), 'pushFamily');
    }
    /** one hosted asset face, with its `*-advances.json` text when there is one */
    pushAsset(family, variant, bytes, advancesJson) {
      check(w.uo_write_env_push_asset(this.#live(), ...text(family), VARIANTS.indexOf(variant), ...arg(bytes), ...text(advancesJson)), 'pushAsset');
    }
    clearFonts() { w.uo_write_env_clear_fonts(this.#live()); }
    /** `[{ family, variants }]` the layouts asked for and nothing answered (each pair once) */
    takeMissing() {
      return lines(take(w.uo_write_env_take_missing(this.#live()), 'takeMissing')).map((l) => {
        const [family, v] = l.split('\t');
        return { family, variants: v.split(',') };
      });
    }
    registerHyphenation(patternText) {
      check(w.uo_write_env_register_hyphenation(this.#live(), ...text(patternText)), 'registerHyphenation');
    }
  }

  class WriteDocument {
    #ptr;
    constructor(ptr) {
      this.#ptr = ptr;
      docRegistry.register(this, ptr, this);
    }
    #live() { if (!this.#ptr) throw new Error('document freed'); return this.#ptr; }
    free() {
      if (!this.#ptr) return;
      docRegistry.unregister(this);
      w.uo_write_document_free(this.#ptr);
      this.#ptr = 0;
    }
    /** the pattern ids to fetch and `env.registerHyphenation` before laying out */
    hyphenationPatterns(env, appLanguages = ['pt-BR', 'en', 'es']) {
      return lines(take(w.uo_write_hyphenation_patterns(this.#live(), env.ptr, ...text(appLanguages.join('\n'))), 'hyphenationPatterns'));
    }
    /** `layoutDocument(full, viewer provider, options)` */
    layout(env, o = {}) {
      const now = o.now instanceof Date ? o.now.getTime() : (o.now ?? Date.now());
      const tz = o.tzOffsetMin ?? -new Date(now).getTimezoneOffset();
      let flags = ['on', 'off', 'force'].indexOf(o.hyphenation ?? 'on');
      flags |= ['print', 'web', 'draft'].indexOf(o.view ?? 'print') << 2;
      flags |= [undefined, 'final', 'simple', 'all', 'original'].indexOf(o.revisions) << 4;
      if (o.hyphenationLanguages) flags |= 1 << 8;
      const p = w.uo_write_layout(this.#live(), env.ptr, now, tz, flags, o.webWidthTw ?? 0, ...text(o.lang), ...text((o.hyphenationLanguages ?? []).join('\n')), o.maxPages ?? 0, o.generation ?? 0);
      if (!p) throw lastError('layout');
      return new WriteLayout(p);
    }
  }

  class WriteLayout {
    #ptr;
    constructor(ptr) {
      this.#ptr = ptr;
      layoutRegistry.register(this, ptr, this);
    }
    #live() { if (!this.#ptr) throw new Error('layout freed'); return this.#ptr; }
    free() {
      if (!this.#ptr) return;
      layoutRegistry.unregister(this);
      w.uo_layout_free(this.#ptr);
      this.#ptr = 0;
    }
    get pageCount() { return w.uo_layout_page_count(this.#live()); }
    /** `{ w, h, section, number, hfItems }` (twips) */
    pageInfo(i) {
      const g = reader(take(w.uo_layout_page_info(this.#live(), i), 'pageInfo'));
      const out = { w: g.f64(), h: g.f64(), section: g.u32(), number: g.f64() };
      const hf = g.i32();
      return hf < 0 ? out : { ...out, hfItems: hf };
    }
    /** the page message of page `i` (resource deltas since the last page this layout encoded) with the
     *  Write section as its `app`: lines, chars, formats, objects, cells and the text layer's paragraphs */
    encodePage(i) { return take(w.uo_layout_encode_page(this.#live(), i), 'encodePage'); }
    /** `collectAnnotations` of the viewer: `{ comments: [{ id, author?, text, rects }], headings: [{ level, text, page, y }] }` (twips) */
    annotations() {
      const g = reader(take(w.uo_layout_annotations(this.#live()), 'annotations'));
      const comments = [];
      for (let i = 0, n = g.u32(); i < n; i++) {
        const id = g.f64();
        const c = g.u8() === 1 ? { id, author: g.str() } : { id };
        c.text = g.str();
        c.rects = [];
        for (let k = 0, m = g.u32(); k < m; k++) c.rects.push({ page: g.u32(), x: g.f64(), y: g.f64(), w: g.f64(), h: g.f64() });
        comments.push(c);
      }
      const headings = [];
      for (let i = 0, n = g.u32(); i < n; i++) {
        const level = g.f64(), page = g.u32(), y = g.f64();
        headings.push({ level, text: g.str(), page, y });
      }
      return { comments, headings };
    }
    /** the item-addressed paint operations of render.mjs, on this layout's view (Buf results) */
    get paint() {
      const v = () => w.uo_layout_view(this.#live());
      return {
        /** the render message alone (no Write section), on the same resource mark as `encodePage` */
        encodePage: (page) => new Buf(w.uo_render_encode_page(v(), page)),
        fxLayers: (page, item, ppt) => new Buf(w.uo_render_fx_layers(v(), page, item, ppt)),
        pattern: (page, item) => new Buf(w.uo_render_pattern(v(), page, item)),
        rectGradient: (page, item) => new Buf(w.uo_render_rect_gradient(v(), page, item)),
        blipTiles: (page, item) => new Buf(w.uo_render_blip_tiles(v(), page, item)),
        pageFillTileTw: (page, devicePxTw) => new Buf(w.uo_render_page_fill_tile_tw(v(), page, devicePxTw)),
        pageFillRaster: (page) => new Buf(w.uo_render_page_fill_raster(v(), page)),
        pictureBounds: (page, item) => new Buf(w.uo_render_picture_bounds(v(), page, item)),
        pictureSubs: (page, item) => new Buf(w.uo_render_picture_subs(v(), page, item)),
        setMaxFxPx: (px) => w.uo_render_view_set_max_fx_px(v(), px),
      };
    }
    /** `{ links: [{ page, x, y, w, h, uri } | { page, x, y, w, h, dest: { page, top } }], outline: [{ title, level, page, top }] }` (points) */
    navigation() {
      const g = reader(take(w.uo_layout_navigation(this.#live()), 'navigation'));
      const links = [];
      for (let i = 0, n = g.u32(); i < n; i++) {
        const l = { page: g.u32(), x: g.f64(), y: g.f64(), w: g.f64(), h: g.f64() };
        if (g.u8() === 0) l.uri = g.str(); else l.dest = { page: g.u32(), top: g.f64() };
        links.push(l);
      }
      const outline = [];
      for (let i = 0, n = g.u32(); i < n; i++) {
        const level = g.u32(), page = g.u32(), top = g.f64();
        outline.push({ title: g.str(), level, page, top });
      }
      return { links, outline };
    }
    /** `runBoxes`: `'links'` or `'inlines'` → `[{ page, x, y, w, h, baseline, value }]` (twips) */
    runBoxes(kind = 'links') {
      const g = reader(take(w.uo_layout_run_boxes(this.#live(), kind === 'links' ? 0 : 1), 'runBoxes'));
      const out = [];
      for (let i = 0, n = g.u32(); i < n; i++) out.push({ page: g.u32(), x: g.f64(), y: g.f64(), w: g.f64(), h: g.f64(), baseline: g.f64(), value: g.str() });
      return out;
    }
    /** `blockLine(layout, block)`: `{ page, y }` or null */
    blockLine(block) {
      const data = take(w.uo_layout_block_line(this.#live(), block), 'blockLine');
      if (!data) return null;
      const g = reader(data);
      return { page: g.u32(), y: g.f64() };
    }
    warnings() { return lines(take(w.uo_layout_warnings(this.#live()), 'warnings')); }
    missingFaces() {
      return lines(take(w.uo_layout_missing_faces(this.#live()), 'missingFaces')).map((l) => {
        const [family, b, i] = l.split('\t');
        return { family, bold: b === '1', italic: i === '1' };
      });
    }
    /** the font file of face `i` of the messages (for a `FontFace`), or null */
    faceBytes(i) { const { status, data } = new Buf(w.uo_layout_face_bytes(this.#live(), i)).take(); return status === 0 ? data : null; }
    imageBytes(i) { const { status, data } = new Buf(w.uo_layout_image_bytes(this.#live(), i)).take(); return status === 0 ? data : null; }
    /** the PDF in chunks: `{ chunks, length }` (`new Blob(chunks, { type: 'application/pdf' })` in a browser) */
    pdf({ compress = true, untrusted = false, chunk = 0 } = {}) {
      const chunks = [];
      host.sink = (c) => chunks.push(c);
      let n;
      try {
        n = w.uo_layout_pdf(this.#live(), (compress ? 1 : 0) | (untrusted ? 2 : 0), chunk);
      } finally {
        host.sink = null;
      }
      if (n < 0) throw lastError('pdf');
      return { chunks, length: n };
    }
  }

  return {
    WriteEnv,
    WriteDocument,
    WriteLayout,
    render: r,
    newEnv: ({ phone = false } = {}) => new WriteEnv(phone),
    /** opens docx bytes; throws a WriteError */
    open(bytes) {
      const p = w.uo_write_open(...arg(bytes));
      if (!p) throw lastError('open');
      return new WriteDocument(p);
    },
  };
}
