// P3 facets of frontier F3 (record "Wave 1 — F3"): the drawing values and the metafile players,
// recorded while the frozen engine lays a document out. Each call becomes one canonical stream —
// the P1 encoding of canon.mjs over a one-page layout holding the call's result — and the facet
// is the stream of those per-call values (`u16s(count:digest)`) in call order:
//
//   p3.emf, p3.wmf   renderEmf/renderWmf(ctx, bytes, dest): the page items are the result
//   p3.picture       pictureLook(im, theme): an image item (a fixed empty image) with `pic`
//   p3.pagefill      resolvePageFill(bg, pageColor): the page's `backgroundFill`
//   p3.textfx        resolveTextFx(fx, theme, text): a glyph run (a fixed empty face) with `fx`
//   p3.shape         shapeExtras(s0, theme): a shape item with `style: s0` and the extras
//
// The dump (`<key>.f3.jsonl`, one JSON object a line) keeps the inputs of every call, so
// tests/f3_facets.rs replays each call alone (unit facets before the D1a reader, LQ22): model
// objects through JSON.stringify with Uint8Array → {"$bytes": sha256} (the bytes once, in a
// `{"f":"media"}` line, hex) and every non-finite number or -0 → {"$num": "NaN" | "Infinity" |
// "-Infinity" | "-0"}; the theme of the document; the faces the pass's `FaceResolver` had resolved
// when a metafile was played (its cache: request key → answer, read after the call, no patching).
import { createHash } from 'node:crypto';
import { canonLayout } from './canon.mjs';
import { Sink } from './sink.mjs';

/** Wrapped module functions (dist files of docx-layout). */
export const F3_WRAPS = {
  'emf.js': ['renderEmf'],
  'wmf.js': ['renderWmf'],
  'picture.js': ['pictureLook'],
  'page-fill.js': ['resolvePageFill'],
  'textfx.js': ['resolveTextFx'],
  'geometry.js': ['shapeExtras'],
};

const FACET = {
  renderEmf: 'p3.emf', renderWmf: 'p3.wmf', pictureLook: 'p3.picture', resolvePageFill: 'p3.pagefill',
  resolveTextFx: 'p3.textfx', shapeExtras: 'p3.shape',
};

const sha = (u8) => createHash('sha256').update(u8).digest('hex');

/** A fixed empty image and face, so the labels of a call's stream do not depend on other calls. */
export const EMPTY_IMAGE = { data: new Uint8Array(0), ext: '', widthTwips: 0, heightTwips: 0 };
export const EMPTY_FACE = { key: '', family: '' };

/** The one-page layout a call's result is written as (src/drawing.rs and the Rust tests build the same). */
export function f3Page(name, args, r) {
  const page = { widthTw: 0, heightTw: 0, section: 0, number: 0, items: [], lines: [] };
  switch (name) {
    case 'renderEmf': case 'renderWmf': page.items = r; break;
    case 'pictureLook': page.items = [{ t: 'image', x: 0, y: 0, w: 0, h: 0, image: EMPTY_IMAGE, ...(r ? { pic: r } : {}) }]; break;
    case 'resolvePageFill': if (r) page.backgroundFill = r; break;
    case 'resolveTextFx': page.items = [{ t: 'glyphs', face: EMPTY_FACE, sizePt: 0, color: '', y: 0, glyphs: [], text: '', ...(r ? { fx: r } : {}) }]; break;
    case 'shapeExtras': page.items = [{ t: 'shape', x: 0, y: 0, w: 0, h: 0, style: args[0], ...r }]; break;
    default: throw new Error(`facet ${name}`);
  }
  return { pages: [page], warnings: [] };
}

/** The per-call value of a facet. */
export function f3Value(name, args, r) {
  const s = new Sink();
  canonLayout(f3Page(name, args, r), null, s);
  return s.finish().value;
}

/** JSON of a model value for the dump (bytes by digest into `media`, non-finite numbers kept). */
export function dumpJson(v, media) {
  return JSON.stringify(v, function (k, x) {
    const raw = this[k];
    if (raw instanceof Uint8Array) { const h = sha(raw); media.set(h, raw); return { $bytes: h }; }
    if (typeof raw === 'number' && (!Number.isFinite(raw) || Object.is(raw, -0))) return { $num: Object.is(raw, -0) ? '-0' : String(raw) };
    return x;
  }) ?? 'null';
}

/** The answer of a face for the dump (`FontFace` → description). */
export function faceLine(req, a) {
  return `{"f":"face","req":${JSON.stringify(req)},"key":${JSON.stringify(a?.key ?? '')},"family":${JSON.stringify(a?.family ?? '')},`
    + `"bold":${!!a?.bold},"italic":${!!a?.italic},"synthBold":${!!a?.synthBold},"synthItalic":${!!a?.synthItalic},"standIn":${!!a?.standIn},`
    + `"bytes":${a?.bytes ? `"${sha(a.bytes)}"` : 'null'}}`;
}

/** The F3 recorder of one capture run: `start()`, `report(name, args, result)` per wrapped call, `finish()`. */
export function f3Recorder() {
  let sinks = null, lines = null, media = null, faces = null, themeSeen = null;
  const theme = (t) => {
    if (themeSeen === t) return;
    themeSeen = t;
    lines.push(`{"f":"theme","theme":${dumpJson(t, media)}}`);
  };
  return {
    start() {
      sinks = new Map(Object.values(FACET).map((f) => [f, new Sink()]));
      lines = []; media = new Map(); faces = new Map(); themeSeen = null;
    },
    report(name, args, r) {
      const facet = FACET[name];
      if (!facet || !sinks) return;
      const v = f3Value(name, args, r);
      sinks.get(facet).u16s(v);
      const D = (x) => dumpJson(x, media);
      switch (name) {
        case 'renderEmf': case 'renderWmf': {
          const [ctx, bytes, dest] = args;
          const h = sha(bytes); media.set(h, bytes);
          for (const [req, a] of ctx?.faces?.cache ?? []) if (!faces.has(req)) { faces.set(req, a); lines.push(faceLine(req, a)); }
          lines.push(`{"f":"${name === 'renderEmf' ? 'emf' : 'wmf'}","bytes":"${h}","dest":${D([dest.x, dest.y, dest.w, dest.h])},"v":"${v}"}`);
          break;
        }
        case 'pictureLook': theme(args[1]); lines.push(`{"f":"picture","im":${D(args[0])},"v":"${v}"}`); break;
        case 'resolvePageFill': lines.push(`{"f":"pagefill","bg":${D(args[0])},"pageColor":${D(args[1] ?? null)},"v":"${v}"}`); break;
        case 'resolveTextFx': theme(args[1]); lines.push(`{"f":"textfx","fx":${D(args[0] ?? null)},"text":${D(args[2])},"v":"${v}"}`); break;
        case 'shapeExtras': theme(args[1]); lines.push(`{"f":"shape","s":${D(args[0])},"v":"${v}"}`); break;
      }
    },
    /** `{ facets, dump }`: the facet values that were used, and the dump text (null when empty). */
    finish() {
      const facets = {};
      for (const [f, s] of sinks) if (s.items) facets[f] = s.finish().value;
      let dump = null;
      if (lines.length) {
        const head = [...media].map(([h, u8]) => `{"f":"media","sha":"${h}","hex":"${Buffer.from(u8).toString('hex')}"}`);
        dump = [...head, ...lines].join('\n') + '\n';
      }
      sinks = null; lines = null; media = null; faces = null;
      return { facets, dump };
    },
  };
}
