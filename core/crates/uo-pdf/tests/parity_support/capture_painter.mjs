// PP0 of the uo-pdf painter (frontier P): the frozen TS `renderPdf` (write/packages/docx-layout/src/pdf.ts,
// pdf-picture.ts, pdf-word-spaces.ts, built to dist/) over LayoutResult-shaped inputs, compared by
// tests/painter_parity.rs with `render_pdf` over the same pages decoded into uo-render types.
//
//   UO_TS_ROOT=<built checkout of main> node tests/parity_support/capture_painter.mjs [--pages N] [--dump <dir>]
//   UO_TS_ROOT=<...> node tests/parity_support/capture_painter.mjs --docs <dir with .docx> --dump <dir> --out <file> [--open-fonts]
//
// Inputs ("specs", JSON in the capture; numbers JSON cannot hold are the strings "NaN", "Infinity",
// "-Infinity", "-0"):
//   * the generated corpus (default): seeded pages (the xorshift32 of uo-testkit) that exercise every branch of
//     the painter - glyph runs (word spaces, advance tables, pua codes, synthetic styles, as-path, text
//     effects), rectangles, lines of every dash, images (PNG/BMP/JPEG, crops, rotations, picture looks), shapes
//     (presets, custom paths, gradients, patterns, blips, shadows, effects), paths, clips, transforms, page
//     backgrounds and page fills, links and an outline. Open fonts only (shared/fonts/files).
//   * `--docs`: the frozen TS engine lays out real .docx files (its own `layoutDocument`, the open fonts of the
//     repository unless --system-fonts) and the LayoutResult is serialized into the same spec. The spec of such
//     a document references faces and images by sha256 and goes to `--dump` (never committed when the faces are
//     not open fonts).
// Each record holds
//   spec   the input (omitted in --docs mode: it lives in <dump>/<key>.spec.json)
//   raw    length:sha256 of the PDF made with compress: false          (compared byte for byte)
//   norm   objects:deflated:digest of the PDF made with compress: true (compared structurally, as capture_writer.mjs)
//   deflated  streams that were deflated in the TS file (the "only if smaller" rule can flip against miniz_oxide)
import { createHash } from 'node:crypto';
import { mkdirSync, readFileSync, readdirSync, writeFileSync, existsSync } from 'node:fs';
import { dirname, join, basename } from 'node:path';
import { fileURLToPath } from 'node:url';
import { deflateSync, inflateSync, crc32 } from 'node:zlib';
import { CaptureWriter, Rng, Stream, importFrom, tsRoot } from '../../../uo-testkit/js/testkit.mjs';

const here = dirname(fileURLToPath(import.meta.url));
const repo = join(here, '../../../../..');
const fontDir = join(repo, 'shared/fonts/files');
const argv = process.argv.slice(2);
const arg = (name, dflt) => { const i = argv.indexOf(name); return i >= 0 ? argv[i + 1] : dflt; };
const PAGES = Number(arg('--pages', '90'));
const DUMP = arg('--dump', null);
const DOCS = arg('--docs', null);
const OUT = arg('--out', join(here, '../parity/painter.cap'));

const root = tsRoot(argv);
const W = 'write/packages/docx-layout/';
const D = `${W}dist/`;
const { renderPdf } = await importFrom(root, `${D}pdf.js`);
const { SfntFont } = await importFrom(root, `${D}typography/font/sfnt.js`);

export const FONT_FILES = readdirSync(fontDir).filter((f) => f.endsWith('.ttf')).sort();
const fontBytes = new Map(FONT_FILES.map((f) => [f, new Uint8Array(readFileSync(join(fontDir, f)))]));
const shaOf = (u8) => createHash('sha256').update(u8).digest('hex');
const fontBySha = new Map([...fontBytes].map(([f, b]) => [shaOf(b), f]));
// open fonts of the site assets (Liberation Sans) are found by the Rust side in site/assets/fonts
const siteFonts = join(repo, 'site/assets/fonts');
if (existsSync(siteFonts)) for (const f of readdirSync(siteFonts).filter((x) => x.endsWith('.ttf'))) fontBySha.set(shaOf(new Uint8Array(readFileSync(join(siteFonts, f)))), f);

// ---- numbers ---------------------------------------------------------------------------------

const enc = (v) => (Number.isNaN(v) ? 'NaN' : v === Infinity ? 'Infinity' : v === -Infinity ? '-Infinity' : Object.is(v, -0) ? '-0' : v);
const json = (v) => JSON.stringify(v, (k, x) => (typeof x === 'number' ? enc(x) : x));
const unit = (r) => r.next() / 4294967296;
const pick = (r, a) => a[r.below(a.length)];
const chance = (r, pct) => r.below(100) < pct;
const range = (r, lo, hi) => lo + unit(r) * (hi - lo);

// ---- image encoders (PNG, BMP, fake JPEG) -----------------------------------------------------

function chunk(type, data) {
  const t = Buffer.from(type, 'latin1');
  const len = Buffer.alloc(4); len.writeUInt32BE(data.length);
  const crc = Buffer.alloc(4); crc.writeUInt32BE(crc32(Buffer.concat([t, data])) >>> 0);
  return Buffer.concat([len, t, Buffer.from(data), crc]);
}
function png(w, h, rgba, colorType) {
  const ch = colorType === 6 ? 4 : colorType === 2 ? 3 : 1;
  const rows = [];
  for (let y = 0; y < h; y++) {
    const row = Buffer.alloc(1 + w * ch);
    for (let x = 0; x < w; x++) for (let c = 0; c < ch; c++) row[1 + x * ch + c] = rgba[(y * w + x) * 4 + (ch === 1 ? 0 : c)];
    rows.push(row);
  }
  const ihdr = Buffer.alloc(13); ihdr.writeUInt32BE(w, 0); ihdr.writeUInt32BE(h, 4); ihdr[8] = 8; ihdr[9] = colorType;
  return new Uint8Array(Buffer.concat([Buffer.from([0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a]), chunk('IHDR', ihdr), chunk('IDAT', deflateSync(Buffer.concat(rows))), chunk('IEND', Buffer.alloc(0))]));
}
function bmp(w, h, rgba) {
  const rowBytes = Math.ceil((w * 3) / 4) * 4;
  const b = Buffer.alloc(54 + rowBytes * h);
  b.write('BM'); b.writeUInt32LE(b.length, 2); b.writeUInt32LE(54, 10); b.writeUInt32LE(40, 14);
  b.writeInt32LE(w, 18); b.writeInt32LE(h, 22); b.writeUInt16LE(1, 26); b.writeUInt16LE(24, 28); b.writeUInt32LE(rowBytes * h, 34);
  for (let y = 0; y < h; y++) for (let x = 0; x < w; x++) {
    const o = 54 + (h - 1 - y) * rowBytes + x * 3, i = (y * w + x) * 4;
    b[o] = rgba[i + 2]; b[o + 1] = rgba[i + 1]; b[o + 2] = rgba[i];
  }
  return new Uint8Array(b);
}
/** a JPEG shell: the headers the PDF reads (SOF, Adobe APP14); the entropy-coded data is noise */
function jpeg(r, w, h, comps, adobe) {
  const parts = [Buffer.from([0xff, 0xd8])];
  if (adobe) parts.push(Buffer.from([0xff, 0xee, 0x00, 0x0e, 0x41, 0x64, 0x6f, 0x62, 0x65, 0x00, 0x64, 0, 0, 0, 0, comps === 4 ? 2 : 1]));
  const sof = Buffer.alloc(10 + comps * 3); sof[0] = 0xff; sof[1] = 0xc0; sof.writeUInt16BE(8 + comps * 3, 2); sof[4] = 8; sof.writeUInt16BE(h, 5); sof.writeUInt16BE(w, 7); sof[9] = comps;
  parts.push(sof);
  const noise = Buffer.alloc(40 + r.below(300)); for (let i = 0; i < noise.length; i++) noise[i] = r.below(256);
  parts.push(Buffer.from([0xff, 0xda, 0x00, 0x08, 0x01, 0x01, 0x00, 0x00, 0x3f, 0x00]), noise, Buffer.from([0xff, 0xd9]));
  return new Uint8Array(Buffer.concat(parts));
}

// ---- spec generator -------------------------------------------------------------------------------

const FAMILY = { carlito: 'Carlito', caladea: 'Caladea', liberationmono: 'Liberation Mono', liberationserif: 'Liberation Serif' };
const WORDS = 'o motor de layout quebra linhas pagina tabelas notas campos e desenha cada glifo na posicao exata acao coracao informacao paralelepipedo the quick brown fox jumps over lazy dog 0123 (a) [b] -x- ABC XYZ Wmw'.split(' ');
const SIZES = [8, 9, 9.5, 10, 10.5, 11, 11.04, 12, 14, 16, 18, 24];
const PRESETS = ['rect', 'roundRect', 'ellipse', 'triangle', 'rtTriangle', 'diamond', 'parallelogram', 'trapezoid', 'homePlate', 'chevron', 'hexagon', 'octagon', 'rightArrow', 'leftArrow', 'upArrow', 'downArrow', 'upDownArrow', 'leftRightArrow', 'round2SameRect', 'frame', 'bentConnector3', 'line', 'straightConnector1', 'straightConnector2', 'curvedConnector3', 'star5', 'cloud', 'custom', 'unknownShape'];
const PICT_PRESETS = ['rect', 'roundRect', 'ellipse', 'triangle', 'diamond', 'hexagon', 'rightArrow', 'star5', 'heart', 'cloud', 'flowChartProcess', 'wedgeRectCallout'];
const PATTERNS = ['pct5', 'pct10', 'pct50', 'pct90', 'horz', 'vert', 'dnDiag', 'upDiag', 'cross', 'diagCross', 'smCheck', 'lgCheck', 'dotGrid', 'solidDmnd', 'plaid', 'weave', 'divot', 'unknownPattern'];
const DASHES = [undefined, 'dotted', 'dashed', 'dashDot', 'dashDotDot', 'bdrDot', 'bdrDash', 'bdrDashDot', 'bdrDashDotDot', 'bdrDashSmall'];
const PIC_DASH = ['sysDot', 'sysDash', 'dash', 'dashDot', 'lgDash', 'lgDashDot', 'lgDashDotDot', 'sysDashDot', 'sysDashDotDot', 'solid', 'weird'];

const hex6 = (r) => { let s = ''; for (let i = 0; i < 6; i++) s += '0123456789ABCDEF'[r.below(16)]; return s; };
const colorOf = (r) => { const k = r.below(40); return k === 0 ? 'auto' : k === 1 ? `#${hex6(r).toLowerCase()}` : k === 2 ? 'zz11aa' : k === 3 ? hex6(r).toLowerCase() : hex6(r); };
const strictColor = (r) => hex6(r);

/** A coordinate in twips: mostly plain, rarely the values the painter must survive. */
function tw(r, lo = -300, hi = 12000, hostile = true) {
  const k = r.below(100);
  if (k < 70) return Math.round(range(r, lo, hi) * 4) / 4;
  if (k < 85) return range(r, lo, hi);
  if (!hostile) return range(r, lo, hi);
  if (k < 90) return pick(r, [NaN, Infinity, -Infinity, -0, 0]);
  if (k < 95) return pick(r, [1e9, -1e9, 1e300, 5e-324, 0.0004999, 0.0005, 0.0015]);
  return Math.round(range(r, lo, hi));
}
const opt = (r, pct, f) => (chance(r, pct) ? f() : undefined);
const alphaOf = (r) => pick(r, [0.5, 0.25, 0.9, 0.999, 0, 1, 0.123456]);
const clean = (o) => { for (const k of Object.keys(o)) if (o[k] === undefined) delete o[k]; return o; };

function genImages(r, n) {
  const out = [];
  for (let i = 0; i < n; i++) {
    const k = r.below(10);
    const w = 2 + r.below(30), h = 2 + r.below(30);
    const px = new Uint8Array(w * h * 4);
    for (let j = 0; j < w * h; j++) {
      px[j * 4] = (j * 7 + r.below(40)) & 255; px[j * 4 + 1] = (j * 3 + r.below(40)) & 255; px[j * 4 + 2] = (255 - j * 5 + r.below(30)) & 255;
      px[j * 4 + 3] = k < 4 ? 255 : pick(r, [255, 255, 128, 0, 64, 200]);
    }
    let data, ext;
    if (k < 3) { data = png(w, h, px, 6); ext = 'png'; } else if (k < 5) { data = png(w, h, px, 2); ext = 'png'; } else if (k === 5) { data = png(w, h, px, 0); ext = 'png'; }
    else if (k === 6) { data = bmp(w, h, px); ext = 'bmp'; } else if (k === 7) { data = jpeg(r, 20 + r.below(300), 20 + r.below(300), pick(r, [1, 3, 3, 4]), chance(r, 40)); ext = 'jpeg'; }
    else if (k === 8) { data = new Uint8Array(40 + r.below(100)).map(() => r.below(256)); ext = 'png'; } // not an image: skipped
    else { data = png(w, h, px, 6); ext = 'png'; }
    const fx = chance(r, 30) ? clean({ gray: chance(r, 40) || undefined, gain: opt(r, 50, () => pick(r, [0.5, 1.3, 2])), bright: opt(r, 50, () => pick(r, [-0.3, 0.2, 0.5])), floor: chance(r, 20) || undefined, biLevel: opt(r, 15, () => pick(r, [0.3, 0.5, 0.8])) }) : undefined;
    out.push(clean({ b64: Buffer.from(data).toString('base64'), ext, fx }));
  }
  return out;
}

function genFaces(r) {
  const n = 3 + r.below(4);
  const faces = [];
  for (let i = 0; i < n; i++) {
    const file = pick(r, FONT_FILES);
    const base = file.replace(/-(regular|bold|italic|bolditalic)\.ttf$/, '');
    const style = file.replace(/^.*-/, '').replace('.ttf', '');
    const bold = style.includes('bold'), italic = style.includes('italic');
    const family = FAMILY[base];
    const key = chance(r, 15) && faces.length ? faces[r.below(faces.length)].key : `${family}|${bold ? 'b' : ''}${italic ? 'i' : ''}${r.below(2) ? '' : 'x'}`;
    faces.push(clean({
      file, key, family: chance(r, 20) ? `${family} \t Std` : family, bold, italic, bytes: chance(r, 92) ? true : undefined,
      adv: chance(r, 25) ? genAdvances(r) : undefined,
    }));
  }
  return faces;
}
function genAdvances(r) {
  const t = {};
  for (const s of ['10', '11', '12', '10.5']) {
    if (chance(r, 70)) { t[s] = {}; for (const ch of 'aeiou tnrsl') if (chance(r, 70)) t[s][ch] = Math.round(range(r, 50, 400) * 4) / 4; }
  }
  return t;
}

function genFx(r) {
  const fill = () => pick(r, [() => ({ kind: 'none' }), () => ({ kind: 'solid', hex: strictColor(r), alpha: pick(r, [1, 0.5, 0.2]) }),
    () => ({ kind: 'grad', stops: Array.from({ length: 2 + r.below(3) }, (_, i) => clean({ pos: i / 3, hex: strictColor(r), alpha: pick(r, [1, 0.6, 1]) })), angleDeg: pick(r, [0, 45, 90, 180, 270]), scaled: chance(r, 40), ...(chance(r, 30) ? { path: pick(r, ['circle', 'rect', 'shape']), fillTo: { l: 0.5, t: 0.5, r: 0.5, b: 0.5 } } : {}) })])();
  return clean({
    fill: chance(r, 55) ? fill() : undefined,
    outline: chance(r, 40) ? { widthPt: pick(r, [0.5, 1, 2.25]), fill: pick(r, [() => ({ kind: 'solid', hex: strictColor(r), alpha: pick(r, [1, 0.5]) }), () => ({ kind: 'grad', stops: [{ pos: 0, hex: strictColor(r), alpha: 1 }, { pos: 1, hex: strictColor(r), alpha: 1 }], angleDeg: 0, scaled: false })])(), dash: opt(r, 50, () => pick(r, ['dot', 'sysDash', 'dash', 'lgDashDot', 'solid', 'sysDashDotDot'])), cap: 'butt', join: 'miter' } : undefined,
    shadow: chance(r, 40) ? { blurPt: pick(r, [0, 2, 4]), distPt: pick(r, [2, 3.5, 6]), dirDeg: pick(r, [45, 90, 315]), sx: 1, sy: pick(r, [1, 0.9]), kxDeg: 0, kyDeg: pick(r, [0, 10]), algn: pick(r, ['b', 'tl', 'ctr']), hex: strictColor(r), alpha: pick(r, [1, 0.4]) } : undefined,
    glow: chance(r, 30) ? { radPt: pick(r, [3, 6, 10]), hex: strictColor(r), alpha: pick(r, [1, 0.5]) } : undefined,
    reflection: chance(r, 30) ? { blurPt: pick(r, [0, 0.5]), stA: pick(r, [0.5, 0.3]), stPos: 0, endA: 0, endPos: pick(r, [0.5, 0.9, 1]), distPt: pick(r, [0, 2]), dirDeg: 90, algn: 'bl' } : undefined,
  });
}

function genRun(r, faces, y, x0, hostile) {
  const face = r.below(faces.length);
  const size = pick(r, SIZES);
  const glyphs = [];
  let x = x0;
  const words = 1 + r.below(5);
  for (let w = 0; w < words; w++) {
    const word = pick(r, WORDS);
    for (const ch of word) {
      const cp = ch.codePointAt(0);
      glyphs.push(chance(r, 3) ? [cp, x, r.below(120)] : [cp, x]);
      x += size * 20 * pick(r, [0.45, 0.5, 0.55, 0.6]);
    }
    if (chance(r, 25)) { glyphs.push([32, x]); x += size * 20 * 0.226; } else x += size * 20 * pick(r, [0.1, 0.22, 0.4]);
  }
  if (chance(r, 8)) glyphs.push([0xf000 + 0x20 + r.below(0xd0), x]);
  if (chance(r, 3)) glyphs.push([0x1f600, x + 100]);
  const hasFx = chance(r, 10);
  // (a NaN or infinite x on a run with effects gives the TS a layer of NaN size, which the Rust refuses: G-D9)
  if (hostile && chance(r, 4)) glyphs[r.below(glyphs.length)][1] = pick(r, hasFx ? [-0, 1e5] : [NaN, Infinity, -0, 1e300]);
  const item = clean({
    t: 'glyphs', face, sizePt: size, color: colorOf(r), y, glyphs,
    // (a NaN scaleX on a run with effects gives the TS a layer of NaN size, which the Rust refuses: G-D9)
    scaleX: opt(r, 12, () => pick(r, hasFx ? [1, 0.9, 1.2, 0] : [1, 0.9, 1.2, 0, NaN])),
    synthBold: chance(r, 8) || undefined, synthItalic: chance(r, 8) || undefined,
    alpha: opt(r, 8, () => alphaOf(r)), asPath: chance(r, 8) || undefined, colr: chance(r, 3) || undefined,
    fx: hasFx ? genFx(r) : undefined,
  });
  return { item, x };
}

function genLook(r, nImages) {
  const k = () => chance(r, 40);
  return clean({
    geom: chance(r, 50) ? clean({ prst: pick(r, PICT_PRESETS), adj: chance(r, 20) ? { adj: pick(r, [0, 10000, 50000]) } : undefined }) : undefined,
    line: k() ? clean({ color: strictColor(r), alpha: pick(r, [1, 0.5]), widthPt: pick(r, [0.5, 1, 3]), dash: opt(r, 50, () => pick(r, PIC_DASH)) }) : undefined,
    shadow: k() ? clean({ kind: pick(r, ['outer', 'outer', 'inner']), color: strictColor(r), alpha: pick(r, [0.5, 0.35, 1]), blurPt: pick(r, [0, 3, 6]), distPt: pick(r, [0, 3, 5]), dirDeg: pick(r, [0, 45, 90, 135]), sx: opt(r, 40, () => pick(r, [1, 1.1, -1])), sy: opt(r, 40, () => pick(r, [1, 0.8, -1])), kxDeg: opt(r, 20, () => pick(r, [0, 10])), kyDeg: opt(r, 20, () => pick(r, [0, -10])), algn: opt(r, 50, () => pick(r, ['b', 'tl', 'ctr', 'br'])), rotWithShape: chance(r, 40) || undefined }) : undefined,
    glow: chance(r, 25) ? { color: strictColor(r), alpha: pick(r, [0.4, 0.6]), radPt: pick(r, [4, 8]) } : undefined,
    softEdgePt: opt(r, 25, () => pick(r, [2, 5, 0])),
    bevel: chance(r, 15) ? clean({ wPt: pick(r, [3, 6]), hPt: pick(r, [3, 6]), prst: opt(r, 50, () => pick(r, ['circle', 'relaxedInset', 'cross', 'slope', 'hardEdge'])), rig: opt(r, 20, () => 'threePt'), dir: opt(r, 20, () => 't'), material: opt(r, 20, () => 'warmMatte') }) : undefined,
    reflection: chance(r, 25) ? clean({ blurPt: pick(r, [0, 0.5]), stA: pick(r, [0.5, 0.35]), stPos: 0, endA: 0, endPos: pick(r, [0.5, 0.9]), distPt: pick(r, [0, 3]), dirDeg: 90, sy: -1, algn: 'bl', rotWithShape: false }) : undefined,
    alpha: opt(r, 20, () => pick(r, [0.5, 0.8, 1])),
    duotone: chance(r, 12) ? [strictColor(r), strictColor(r)] : undefined,
    clrChange: chance(r, 12) ? { from: strictColor(r), to: strictColor(r), toAlpha: pick(r, [0, 1, 0.5]) } : undefined,
  });
}

function genShape(r, nImages) {
  const prst = pick(r, PRESETS);
  const x = tw(r, 0, 9000, false), y = tw(r, 0, 12000, false), w = range(r, 200, 5000), h = range(r, 200, 4000);
  const style = clean({
    geom: prst, adj: chance(r, 20) ? { adj: pick(r, [0, 25000, 50000]) } : undefined,
    cust: prst === 'custom' && chance(r, 85) ? Array.from({ length: 1 + r.below(2) }, () => ({
      w: pick(r, [100, 21600, 0]), h: pick(r, [100, 21600]), fill: chance(r, 80), stroke: chance(r, 80),
      cmds: [{ op: 'M', x: 0, y: 0 }, { op: 'L', x: 50, y: 0 }, { op: 'C', pts: [[80, 20], [90, 60], [50, 80]] }, { op: 'Q', pts: [[20, 90], [0, 50]] }, ...(chance(r, 40) ? [{ op: 'A', wR: 30, hR: 30, st: 0, sw: 90 }] : []), { op: 'Z' }],
    })) : undefined,
    flipH: chance(r, 20) || undefined, flipV: chance(r, 20) || undefined, rot: opt(r, 30, () => pick(r, [0, 15, 45, 90, 180, 300, 0.5])),
    fillAlpha: opt(r, 15, () => alphaOf(r)), line: chance(r, 20) ? { color: '000000', widthPt: 1, alpha: pick(r, [0.5, 1]) } : undefined,
  });
  const it = { t: 'shape', x, y, w, h, style };
  const fill = pick(r, ['kind-none', 'solid', 'solid', 'grad', 'pattern', 'blip', 'rectgrad']);
  if (fill === 'solid' || chance(r, 20)) it.fill = chance(r, 8) ? 'zz0000' : strictColor(r);
  if (fill === 'grad' || fill === 'rectgrad') {
    it.grad = clean({
      stops: ((n) => Array.from({ length: n }, (_, i) => ({ pos: i / (n - 1), color: strictColor(r) })))(2 + r.below(3)),
      angleDeg: pick(r, [0, 30, 90, 225]), scaled: chance(r, 40) || undefined, rotWithShape: chance(r, 50) ? true : chance(r, 50) ? false : undefined,
      ...(fill === 'rectgrad' ? { path: pick(r, ['rect', 'shape']), fillTo: { l: pick(r, [0.5, 0.25]), t: pick(r, [0.5, 0.25]), r: pick(r, [0.5, 0.25]), b: pick(r, [0.5, 0.25]) } } : chance(r, 25) ? { path: 'circle', fillTo: { l: 0.5, t: 0.5, r: 0.5, b: 0.5 } } : {}),
    });
  }
  if (fill === 'pattern') it.pattern = { prst: pick(r, PATTERNS), fg: strictColor(r), bg: strictColor(r) };
  if (fill === 'blip' && nImages > 0) it.blip = clean({ image: r.below(nImages), tile: chance(r, 60) ? { tx: pick(r, [0, 95250]), ty: pick(r, [0, -95250]), sx: pick(r, [100000, 50000, 200000]), sy: pick(r, [100000, 50000]), algn: pick(r, ['tl', 'ctr', 'br', 't', 'r', 'b', 'l']), flip: 'none' } : undefined, alpha: opt(r, 30, () => alphaOf(r)) });
  if (chance(r, 35)) it.stroke = clean({ color: chance(r, 8) ? 'qq0000' : strictColor(r), widthPt: pick(r, [0, 0.1, 0.75, 2, 4]), dash: opt(r, 25, () => 'dash') });
  if ((prst === 'line' || prst.startsWith('straightConnector')) && chance(r, 50)) it.line = { x1: range(r, 0, 800), y1: range(r, 0, 800), x2: range(r, 0, 800), y2: range(r, 0, 800) };
  if (chance(r, 25)) it.shadow = clean({ dx: pick(r, [60, 100, -40]), dy: pick(r, [60, 100]), color: chance(r, 6) ? 'auto' : strictColor(r), alpha: opt(r, 50, () => alphaOf(r)) });
  if (chance(r, 20)) it.fx = genLook(r, 0) && clean({ shadow: genLook(r, 0).shadow, glow: chance(r, 40) ? { color: strictColor(r), alpha: 0.5, radPt: 5 } : undefined, softEdgePt: opt(r, 30, () => 3), reflection: chance(r, 30) ? { blurPt: 0, stA: 0.5, stPos: 0, endA: 0, endPos: 0.8, distPt: 1, dirDeg: 90, sy: -1, algn: 'bl' } : undefined });
  return clean(it);
}

function genPageFill(r, nImages) {
  const kind = pick(r, ['gradient', 'gradientRadial', 'pattern', 'frame', 'tile']);
  const f = { type: kind, c1: strictColor(r), c2: strictColor(r), angle: pick(r, [0, 45, 90, 135]), focus: pick(r, [0, 50, 100, -50]) };
  if (chance(r, 40)) f.stops = [{ pos: 0, color: strictColor(r) }, { pos: 0.5, color: strictColor(r) }, { pos: 1, color: strictColor(r) }];
  if (chance(r, 30)) f.focusPosition = [pick(r, [0, 0.5, 1]), pick(r, [0, 0.5, 1])];
  if (chance(r, 30)) f.gradientCenter = true;
  if (kind === 'pattern') f.pattern = Array.from({ length: 8 }, () => r.below(256));
  if ((kind === 'frame' || kind === 'tile') && nImages > 0) { f.image = r.below(nImages); f.imagePx = { w: pick(r, [100, 300, 1024]), h: pick(r, [100, 200, 768]), dpi: pick(r, [96, 0, 144, 300]) }; } else if (kind === 'frame' || kind === 'tile') f.type = 'gradient';
  return f;
}

function genPath(r) {
  const cmds = [{ op: 'M', x: tw(r), y: tw(r) }];
  for (let i = r.below(6); i > 0; i--) {
    const k = r.below(4);
    cmds.push(k === 0 ? { op: 'L', x: tw(r), y: tw(r) } : k === 1 ? { op: 'C', x1: tw(r), y1: tw(r), x2: tw(r), y2: tw(r), x: tw(r), y: tw(r) } : k === 2 ? { op: 'Z' } : { op: 'M', x: tw(r), y: tw(r) });
  }
  return clean({ t: 'path', cmds, fill: opt(r, 70, () => (chance(r, 8) ? 'zz' : strictColor(r))), stroke: opt(r, 60, () => clean({ color: colorOf(r), widthPt: pick(r, [0, 0.2, 1, 2]), dash: opt(r, 30, () => 'd') })), evenOdd: chance(r, 30) || undefined });
}

export function genSpec(seed) {
  const r = new Rng(seed);
  const faces = genFaces(r);
  const images = genImages(r, 1 + r.below(5));
  const pages = [];
  const nPages = 1 + r.below(3);
  for (let p = 0; p < nPages; p++) {
    const items = [];
    const n = 4 + r.below(26);
    let y = 1200;
    for (let i = 0; i < n; i++) {
      const k = r.below(100);
      if (k < 38) {
        // a line of runs on one baseline (the cursor of the word spaces crosses them)
        let x = tw(r, 1000, 2000, false);
        for (let s = 1 + r.below(3); s > 0; s--) { const g = genRun(r, faces, y, x, true); items.push(g.item); x = g.x + (chance(r, 50) ? 0 : 400); }
        y += pick(r, [240, 300, 450, 600, 1200]);
      } else if (k < 46) items.push({ t: 'rect', x: tw(r), y: tw(r), w: tw(r, 0, 3000), h: tw(r, 0, 800), fill: colorOf(r), ...(chance(r, 25) ? { alpha: alphaOf(r) } : {}) });
      else if (k < 54) items.push(clean({ t: 'line', x1: tw(r), y1: tw(r), x2: tw(r), y2: tw(r), color: colorOf(r), widthPt: pick(r, [0.25, 0.5, 1, 2, 6, NaN]), dash: pick(r, DASHES) }));
      else if (k < 68) {
        const img = r.below(images.length);
        const rot = pick(r, [undefined, undefined, 0, 90, 17.5, 270, NaN]);
        items.push(clean({
          t: 'image', x: tw(r, 0, 9000, false), y: tw(r, 0, 12000, false), w: range(r, 300, 5000), h: range(r, 300, 4000), image: img,
          crop: opt(r, 25, () => ({ l: pick(r, [0, 0.1, 0.25]), t: pick(r, [0, 0.2]), r: pick(r, [0, 0.1, 0.3]), b: pick(r, [0, 0.15]) })),
          rot, flipH: chance(r, 15) || undefined, flipV: chance(r, 15) || undefined, pic: opt(r, 50, () => genLook(r, images.length)),
        }));
      } else if (k < 82) items.push(genShape(r, images.length));
      else if (k < 88) items.push(genPath(r));
      else if (k < 94) {
        items.push({ t: 'clip', op: 'push', x: tw(r, 0, 5000), y: tw(r, 0, 5000), w: tw(r, 100, 6000), h: tw(r, 100, 6000) });
        const g = genRun(r, faces, y, 1500, false); items.push(g.item); items.push({ t: 'clip', op: 'pop' });
      } else if (k < 97) {
        items.push({ t: 'xform', op: 'push', m: [pick(r, [1, 0, 0.7]), pick(r, [0, 1, -0.7]), pick(r, [0, -1, 0.7]), pick(r, [1, 0, 0.7]), tw(r, 0, 3000, false), tw(r, 0, 3000, false)] });
        const g = genRun(r, faces, 800, 400, false); items.push(g.item); items.push({ t: 'xform', op: 'pop' });
      } else items.push({ t: 'xform', op: 'push' }); // a push without m acts as a pop
    }
    pages.push(clean({
      widthTw: pick(r, [11906, 12240, 8000, 16838]), heightTw: pick(r, [16838, 15840, 10000]),
      background: opt(r, 12, () => colorOf(r)), backgroundFill: opt(r, 14, () => genPageFill(r, images.length)), items,
    }));
  }
  const nav = chance(r, 60) ? {
    links: Array.from({ length: r.below(8) }, () => clean({ page: r.below(nPages + 1), link: clean({ xPt: range(r, 0, 500), yTopPt: range(r, 0, 700), wPt: range(r, 5, 200), hPt: range(r, 5, 30), ...(chance(r, 60) ? { uri: pick(r, ['http://a.example/x y', 'https://b.example', 'mailto:a@b.c', 'javascript:alert(1)', 'ftp://x', ' HTTP://UP.EXAMPLE/(p)']) } : { pageIndex: r.below(nPages + 1), destTopPt: range(r, 0, 700) }) }) })),
    outline: Array.from({ length: r.below(7) }, () => ({ title: pick(r, ['Capítulo 1', 'Intro (a)', '  ', 'Título \u{1F600}', 'x\\y']), level: pick(r, [1, 1, 2, 3, 0]), pageIndex: r.below(nPages + 1), topPt: range(r, 0, 700) })),
  } : null;
  return { faces, images, pages, nav };
}

// ---- spec -> TS objects ---------------------------------------------------------------------------------

export function materialize(spec, loadFace, loadImage) {
  const faces = spec.faces.map((f) => {
    const bytes = loadFace(f);
    const font = new SfntFont(bytes);
    if (f.adv) font.setAdvanceTable(f.adv);
    return { key: f.key, family: f.family, font, ...(f.bytes === false ? {} : { bytes }), bold: f.bold, italic: f.italic };
  });
  const images = spec.images.map((im) => ({ data: loadImage(im), ext: im.ext, widthTwips: 0, heightTwips: 0, ...(im.fx ? { effects: { ...im.fx } } : {}) }));
  const fillOf = (f) => ({ ...f, ...(f.image !== undefined ? { image: images[f.image] } : {}) });
  const pages = spec.pages.map((p) => ({
    widthTw: p.widthTw, heightTw: p.heightTw, section: 0, number: 1, lines: [],
    ...(p.background !== undefined ? { background: p.background } : {}),
    ...(p.backgroundFill ? { backgroundFill: fillOf(p.backgroundFill) } : {}),
    items: p.items.map((it) => {
      if (it.t === 'glyphs') return { ...it, face: faces[it.face], text: '', glyphs: it.glyphs.map(([cp, x, gid]) => (gid === undefined ? { cp, x } : { cp, x, gid })) };
      if (it.t === 'image') return { ...it, image: images[it.image] };
      if (it.t === 'shape' && it.blip) return { ...it, blip: { ...it.blip, image: images[it.blip.image] } };
      return it;
    }),
  }));
  const nav = spec.nav ? { links: spec.nav.links, outline: spec.nav.outline } : undefined;
  return { result: { pages, warnings: [] }, navigation: nav };
}
const fromSpec = (spec) => materialize(spec, (f) => (f.file ? fontBytes.get(f.file) : (() => { throw new Error(`no font ${f.sha}`); })()), (im) => new Uint8Array(Buffer.from(im.b64, 'base64')));

// ---- the PDF comparison (as capture_writer.mjs) ---------------------------------------------------------

function latin1(u8) { let s = ''; for (let i = 0; i < u8.length; i++) s += String.fromCharCode(u8[i]); return s; }
function parsePdf(u8) {
  const text = latin1(u8);
  const sx = text.lastIndexOf('startxref\n');
  const xrefAt = Number(/^(\d+)/.exec(text.slice(sx + 10))[1]);
  if (!text.startsWith('xref\n0 ', xrefAt)) throw new Error('bad xref');
  const m = /^xref\n0 (\d+)\n/.exec(text.slice(xrefAt));
  const count = Number(m[1]);
  let p = xrefAt + m[0].length;
  if (text.slice(p, p + 20) !== '0000000000 65535 f \n') throw new Error('bad free entry');
  p += 20;
  const offs = [];
  for (let i = 1; i < count; i++) {
    const e = text.slice(p, p + 20);
    if (!/^\d{10} 00000 n \n$/.test(e)) throw new Error('bad xref entry');
    offs.push(Number(e.slice(0, 10))); p += 20;
  }
  const tr = /^trailer\n<< \/Size (\d+) \/Root (\d+) 0 R >>\nstartxref\n(\d+)\n%%EOF\n$/.exec(text.slice(p));
  if (!tr || Number(tr[1]) !== count || Number(tr[3]) !== xrefAt) throw new Error('bad trailer');
  const objs = [];
  for (let i = 0; i < offs.length; i++) {
    const head = `${i + 1} 0 obj\n`;
    if (!text.startsWith(head, offs[i])) throw new Error(`xref entry ${i + 1} does not hit its object`);
    const end = (i + 1 < offs.length ? offs[i + 1] : xrefAt) - 8;
    if (text.slice(end, end + 8) !== '\nendobj\n') throw new Error(`object ${i + 1} not closed`);
    objs.push(u8.subarray(offs[i] + head.length, end));
  }
  return { objs, root: Number(tr[2]), count };
}
const STREAM = /^(<<[\s\S]*? \/Length (\d+) >>)\nstream\n/;
export function normalize(u8) {
  const { objs, root, count } = parsePdf(u8);
  const s = new Stream();
  s.u32(count).u32(root);
  let deflated = 0;
  objs.forEach((body, i) => {
    s.tag('o').u32(i + 1);
    const t = latin1(body.subarray(0, Math.min(body.length, 4000)));
    const m = STREAM.exec(t);
    if (!m) { s.tag('d').digestOf(body); return; }
    const len = Number(m[2]);
    const headLen = m[0].length;
    const payload = body.subarray(headLen, headLen + len);
    if (latin1(body.subarray(headLen + len)) !== '\nendstream') throw new Error(`object ${i + 1}: bad endstream`);
    const flate = m[1].includes(' /Filter /FlateDecode');
    const dict = m[1].replace(' /Filter /FlateDecode', '').replace(/ \/Length \d+ >>$/, ' /Length N >>');
    let data = payload;
    if (flate) { data = inflateSync(payload); deflated++; }
    s.tag('s').str(dict).digestOf(data);
  });
  return { objects: count - 1, deflated, digest: s.finish().short };
}

/** Both PDFs of a spec: [raw, comp] or null when the TS throws. */
function runTs(spec, build = fromSpec) {
  try {
    const { result, navigation } = build(spec);
    const raw = renderPdf(result, { compress: false, ...(navigation ? { navigation } : {}) });
    const comp = renderPdf(build(spec).result, { compress: true, ...(navigation ? { navigation } : {}) });
    return { raw, comp };
  } catch (e) {
    if (process.env.UO_VERBOSE) console.error(`TS threw: ${e?.stack ?? e}`);
    return null;
  }
}

function record(cap, key, spec, pdfs, withSpec) {
  if (withSpec) cap.set(key, 'spec', json(spec));
  const raw = createHash('sha256').update(pdfs.raw).digest('hex').slice(0, 32);
  const nr = normalize(pdfs.raw), nc = normalize(pdfs.comp);
  if (nr.objects !== nc.objects || nr.digest !== nc.digest) throw new Error(`${key}: the TS compressed and uncompressed files are not structurally equal`);
  cap.set(key, 'raw', `${pdfs.raw.length}:${raw}`);
  cap.set(key, 'norm', `${nc.objects}:${nc.digest}`);
  cap.set(key, 'deflated', nc.deflated);
  return nc;
}

// ---- real documents (--docs) -------------------------------------------------------------------------------------

async function realDocuments(cap) {
  process.env.TZ = 'UTC';
  const core = await importFrom(root, 'write/packages/docx-core/dist/index.js');
  const eng = await importFrom(root, `${D}index.js`);
  const { pdfNavigation } = await importFrom(root, `${D}pdf-navigation.js`);
  if (argv.includes('--open-fonts')) {
    process.env.WINDIR = join(here, '__no_system_fonts__');
    process.env.LOCALAPPDATA = join(here, '__no_system_fonts__');
    process.env.EXTRA_FONT_DIRS = [join(root, 'shared/fonts/files'), join(root, 'site/assets/fonts')].join(';');
  }
  const { createNodeFontProvider } = await importFrom(root, 'write/tools/corpus-lab/node-fonts.mjs');
  const provider = await createNodeFontProvider();
  await eng.loadHyphenation(eng.HYPHENATION_PATTERN_IDS);
  const NOW = new Date(Date.UTC(2026, 0, 15, 12, 0, 0));
  const files = readdirSync(DOCS).filter((f) => /.docx$/i.test(f)).sort().slice(0, Number(arg('--max', '100000')));
  let docs = 0, pages = 0, errors = 0, thrown = 0;
  mkdirSync(join(DUMP, 'fonts'), { recursive: true });
  mkdirSync(join(DUMP, 'media'), { recursive: true });
  for (const f of files) {
    const key = basename(f).replace(/\.docx$/i, '');
    try {
      const pkg = core.DocxPackage.load(new Uint8Array(readFileSync(join(DOCS, f))));
      const full = core.readFullDocument(pkg);
      let embedded = [];
      try { embedded = core.readEmbeddedFonts(pkg); } catch { /* none */ }
      const res = eng.layoutDocument(full, eng.withEmbeddedFonts(provider, embedded, (b) => new eng.SfntFont(b)), { now: NOW });
      const navigation = pdfNavigation(full, res);
      const spec = serialize(res, navigation, DUMP);
      writeFileSync(join(DUMP, `${key}.spec.json`), json(spec));
      // the serialized spec must produce the very PDF of the engine's own result
      const direct = renderPdf(res, { compress: false, navigation });
      const pdfs = runTs(spec, (s) => materialize(s, (fc) => new Uint8Array(readFileSync(join(DUMP, 'fonts', `${fc.sha}.bin`))), (im) => new Uint8Array(readFileSync(join(DUMP, 'media', `${im.sha}.bin`)))));
      if (!pdfs) { thrown++; cap.set(key, 'raw', 'throw'); continue; }
      if (shaOf(direct) !== shaOf(pdfs.raw)) throw new Error(`${key}: the serialized spec does not reproduce the engine's PDF`);
      // --inline-spec: open fonts only, small images - the spec goes into the capture (the committed fixture)
      record(cap, key, spec, pdfs, argv.includes('--inline-spec') && spec.faces.every((fc) => fc.file) && spec.images.every((im) => im.sha === undefined || true));
      docs++; pages += res.pages.length;
    } catch (e) { errors++; cap.set(key, 'error', String(e?.message ?? e).replace(/[\t\n\r]/g, ' ').slice(0, 300)); }
  }
  cap.meta('documents', docs); cap.meta('pages', pages); cap.meta('errors', errors); cap.meta('ts-throws', thrown);
  return docs;
}

/** LayoutResult -> spec (faces and images by sha256, written to the dump). */
function serialize(res, navigation, dump) {
  const faces = [], images = [];
  const fidx = new Map(), iidx = new Map();
  const faceOf = (face) => {
    if (fidx.has(face)) return fidx.get(face);
    const bytes = face.bytes ?? new Uint8Array(0);
    const sha = shaOf(bytes);
    if (bytes.length) writeFileSync(join(dump, 'fonts', `${sha}.bin`), bytes);
    const adv = face.font.advanceTable ?? face.font['advanceTable'];
    faces.push(clean({ sha, file: fontBySha.get(sha), key: face.key, family: face.family, bold: face.bold, italic: face.italic, bytes: face.bytes ? undefined : false, adv: adv ?? undefined }));
    fidx.set(face, faces.length - 1);
    return faces.length - 1;
  };
  const imageOf = (im) => {
    if (iidx.has(im)) return iidx.get(im);
    const sha = shaOf(im.data);
    writeFileSync(join(dump, 'media', `${sha}.bin`), im.data);
    images.push(clean({ sha, ...(argv.includes('--inline-spec') ? { b64: Buffer.from(im.data).toString('base64') } : {}), ext: im.ext, fx: im.effects ? { ...im.effects } : undefined }));
    iidx.set(im, images.length - 1);
    return images.length - 1;
  };
  const pages = res.pages.map((p) => clean({
    widthTw: p.widthTw, heightTw: p.heightTw, background: p.background,
    backgroundFill: p.backgroundFill ? { ...p.backgroundFill, ...(p.backgroundFill.image ? { image: imageOf(p.backgroundFill.image) } : {}) } : undefined,
    items: p.items.map((it) => {
      if (it.t === 'glyphs') {
        const { face, glyphs, text, lineBox, lineNum, ...rest } = it;
        return { ...rest, face: faceOf(face), glyphs: glyphs.map((g) => (g.gid === undefined ? [g.cp, g.x] : [g.cp, g.x, g.gid])) };
      }
      if (it.t === 'image') return { ...it, image: imageOf(it.image) };
      if (it.t === 'shape') {
        const { style, ...rest } = it;
        const s = { ...rest, style };
        if (it.blip) s.blip = { ...it.blip, image: imageOf(it.blip.image) };
        return s;
      }
      if (it.t === 'clip') { const { src, ...rest } = it; return rest; }
      return it;
    }),
  }));
  return { faces, images, pages, nav: navigation ? { links: navigation.links, outline: navigation.outline } : null };
}

// ---- main ----------------------------------------------------------------------------------------------------------

async function main() {
  const cap = new CaptureWriter(['PP0 of the uo-pdf painter (frontier P): the frozen TS renderPdf over generated and real layouts', 'regenerate: see tests/parity_support/capture_painter.mjs']);
  if (DOCS) {
    if (!DUMP) throw new Error('--docs needs --dump <dir>');
    const n = await realDocuments(cap);
    cap.write(OUT);
    console.log(`wrote ${n} documents to ${OUT}`);
    return;
  }
  if (DUMP) mkdirSync(DUMP, { recursive: true });
  let written = 0, thrown = 0, deflated = 0, objects = 0;
  const stats = { glyphs: 0, shapes: 0, images: 0, picturesWithLook: 0, fx: 0 };
  for (let i = 0; written < PAGES && i < PAGES * 3; i++) {
    const spec = genSpec(0x2000 + i * 7919);
    const pdfs = runTs(spec);
    if (!pdfs) { thrown++; continue; }
    const key = `p${String(written).padStart(4, '0')}`;
    const nc = record(cap, key, spec, pdfs, true);
    deflated += nc.deflated; objects += nc.objects;
    for (const p of spec.pages) for (const it of p.items) {
      if (it.t === 'glyphs') { stats.glyphs++; if (it.fx) stats.fx++; } else if (it.t === 'shape') stats.shapes++; else if (it.t === 'image') { stats.images++; if (it.pic) stats.picturesWithLook++; }
    }
    if (DUMP) { writeFileSync(join(DUMP, `${key}.raw.pdf`), pdfs.raw); writeFileSync(join(DUMP, `${key}.comp.pdf`), pdfs.comp); }
    written++;
  }
  cap.meta('specs', written);
  cap.meta('ts-throws-skipped', thrown);
  cap.meta('deflated-streams-ts', deflated);
  cap.meta('objects', objects);
  for (const [k, v] of Object.entries(stats)) cap.meta(`items-${k}`, v);
  cap.write(OUT);
  console.log(`wrote ${written} specs (${thrown} skipped: the TS threw), ${objects} objects, ${deflated} deflated streams; ${JSON.stringify(stats)}`);
}

if (process.argv[1]?.endsWith('capture_painter.mjs')) await main();
