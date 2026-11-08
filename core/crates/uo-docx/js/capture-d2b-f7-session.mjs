// D2b-F7 (drawing objects, text boxes, pictures) in the D2 editor harness (G17): the frozen
// `<universal-write>` element in headless Chromium (Playwright), driven through its public API and
// the TS-private entry points the D2a harness uses (`type`, `pressEnter`, `pressBackspace`,
// `pressDelete`, `stringifyState`, `parseState`) plus `floatSel`/`floatMulti` for the selection a
// click makes. The page set-up is the one of `capture-d2b-f6-session.mjs` and the math override of
// `capture-d2b-f3-session.mjs` (the objects' extents go through sin/cos/tan): the element
// initialises with the open default font so that the layout pass runs, `Math.random` = the testkit
// xorshift32, `Date` fixed (2026-01-02 03:04 UTC), timezone UTC.
//
// The TS reads facts the Rust session takes as arguments or as job answers: the layout rectangle of
// the selected object (`selectedFloat`, `selectedFloats`), the positioning references of an object
// (`engObjectFrame`), where a new box landed (`engObjectOf`), and the pixels of the pictures
// (`createImageBitmap`, `getImageData`, `putImageData`, `convertToBlob`). The page wraps them and
// logs each call into `window.__fx`; the log of a step is the facet `fx`, which the replay turns
// into the arguments and the job answers, in order. The pictures are tiny synthetic ones made here
// (PNG/JPEG, opaque, with transparency, all transparent, and one 600 x 40 px to reach the 480 px
// scale of the contour); their bytes are in the capture (`img#<k>`, facet `b64`).
//
//   UO_TS_ROOT=<built checkout of main> node core/crates/uo-docx/js/capture-d2b-f7-session.mjs [--docs N] [--steps N]
//
// Writes tests/parity/d2b-f7-session.capture: per `<doc>#<j>` the D2a facets (op, paras, sel, anc,
// ev, flags, docxml) and F7's own:
//   fs     `stringifyState().fs` (the index of the selected object);
//   tb     the text box in edit (`te`: host, focus, page);
//   pane   `selectionPaneItems()`;
//   st     `selectedShape()` / `selectedImage()` model values (EMU, no unit conversions);
//   fx     the fact log of the step;
//   enc    the hash of every `putImageData` of the step (the pixels handed to an encoder);
//   n      the return value of `compressImages`.
import { createRequire } from 'node:module';
import { createServer } from 'node:http';
import { readFileSync, existsSync, readdirSync } from 'node:fs';
import { join, extname, normalize } from 'node:path';
import { createHash } from 'node:crypto';
import { deflateSync } from 'node:zlib';
import { CaptureWriter, Rng, Stream } from '../../uo-testkit/js/testkit.mjs';
import { Canon } from './canon.mjs';
import { mathOverride } from './d2b-math-override.mjs';
import { crate, corpus, readBytes, root, ts } from './capture-r2-common.mjs';

const Digest = { of: (b) => ({ short: createHash('sha256').update(b).digest('hex').slice(0, 32) }) };
const argv = process.argv.slice(2);
const opt = (k, d) => (argv.includes(k) ? Number(argv[argv.indexOf(k) + 1]) : d);
const DOCS = opt('--docs', 40);
const STEPS = opt('--steps', 60);
const { DocxPackage } = await ts('package.js');

const pwFrom = [join(root, 'slides', 'package.json'), join(root, 'sheet', 'package.json')].find((p) => existsSync(p));
const { chromium } = createRequire(pwFrom)('playwright');

const MIME = { '.js': 'text/javascript', '.mjs': 'text/javascript', '.html': 'text/html', '.json': 'application/json', '.wasm': 'application/wasm', '.css': 'text/css', '.ttf': 'font/ttf', '.otf': 'font/otf', '.woff2': 'font/woff2' };
const server = createServer((req, res) => {
  const url = decodeURIComponent((req.url ?? '/').split('?')[0]);
  if (url === '/__d2b.html') {
    res.writeHead(200, { 'content-type': 'text/html' });
    res.end('<!doctype html><meta charset="utf-8"><body><script type="module">import "/write/packages/editor/publish/index.js"; window.__ready = true;</script></body>');
    return;
  }
  const file = normalize(join(root, url));
  if (!file.startsWith(normalize(root)) || !existsSync(file)) { res.writeHead(404); res.end(); return; }
  res.writeHead(200, { 'content-type': MIME[extname(file)] ?? 'application/octet-stream' });
  res.end(readFileSync(file));
});
await new Promise((r) => server.listen(0, '127.0.0.1', r));
const port = server.address().port;

const installed = (() => {
  const base = join(process.env.LOCALAPPDATA ?? '', 'ms-playwright');
  if (!existsSync(base)) return undefined;
  const dirs = readdirSync(base).filter((d) => d.startsWith('chromium_headless_shell-')).sort();
  const last = dirs[dirs.length - 1];
  const exe = last && join(base, last, 'chrome-headless-shell-win64', 'chrome-headless-shell.exe');
  return exe && existsSync(exe) ? exe : undefined;
})();
const browser = await chromium.launch({ headless: true, executablePath: process.env.UO_CHROMIUM ?? installed });
const context = await browser.newContext({ timezoneId: 'UTC', locale: 'pt-BR' });
const mo = await mathOverride();
await context.addInitScript(mo.initScript);
await context.addInitScript(() => {
  let s = 1;
  let rc = 0;
  window.__rc = () => rc;
  Math.random = () => { rc++; s ^= s << 13; s >>>= 0; s ^= s >>> 17; s ^= s << 5; s >>>= 0; return s / 4294967296; };
  const T = Date.UTC(2026, 0, 2, 3, 4, 0);
  const D = Date;
  // eslint-disable-next-line no-global-assign
  Date = class extends D { constructor(...a) { if (a.length) super(...a); else super(T); } static now() { return T; } };
  // the fact log: the pixels the editor reads and writes
  window.__fx = [];
  const b64 = (u8) => { let t = ''; for (let i = 0; i < u8.length; i += 0x8000) t += String.fromCharCode(...u8.subarray(i, i + 0x8000)); return btoa(t); };
  const fnv = (u8) => { let h = 0x811c9dc5; for (let i = 0; i < u8.length; i++) { h ^= u8[i]; h = Math.imul(h, 0x01000193) >>> 0; } return h.toString(16); };
  const cib = window.createImageBitmap;
  window.createImageBitmap = async function (...a) { const r = await cib.apply(this, a); if (window.__fxOn && window.__fxPix) window.__fx.push(['bmp', r.width, r.height]); return r; };
  for (const C of [OffscreenCanvasRenderingContext2D, CanvasRenderingContext2D]) {
    const g = C.prototype.getImageData;
    C.prototype.getImageData = function (...a) { const d = g.apply(this, a); if (window.__fxOn && window.__fxPix && !window.__fxNoGid) window.__fx.push(['gid', d.width, d.height, b64(new Uint8Array(d.data.buffer.slice(0)))]); return d; };
    const p = C.prototype.putImageData;
    C.prototype.putImageData = function (d, ...a) { if (window.__fxOn && window.__fxPix) window.__fx.push(['put', d.width, d.height, fnv(new Uint8Array(d.data.buffer.slice(0)))]); return p.call(this, d, ...a); };
  }
  const ctb = OffscreenCanvas.prototype.convertToBlob;
  OffscreenCanvas.prototype.convertToBlob = async function (...a) {
    const b = await ctb.apply(this, a);
    if (window.__fxOn && window.__fxPix) window.__fx.push(['blob', a[0]?.type ?? 'image/png', a[0]?.quality ?? null, b64(new Uint8Array(await b.arrayBuffer()))]);
    return b;
  };
});

const EVENTS = ['uw-change', 'uw-dirty-change', 'uw-input', 'uw-track-changes', 'uw-cellmode', 'uw-notemode', 'uw-hfmode', 'uw-tbmode', 'uw-protected', 'uw-blocked', 'uw-revisions-changed', 'uw-comments-changed', 'uw-bookmarks-change', 'uw-markup'];
const TEXTS = ['a', 'Olá ', ' ', 'ção', '😀', '𝒜!', 'abc def'];
const hex = (t) => [...Array(t.length).keys()].map((i) => t.charCodeAt(i).toString(16)).join('.');

// ───────────── the synthetic pictures ─────────────
const crcTable = (() => { const t = []; for (let n = 0; n < 256; n++) { let c = n; for (let k = 0; k < 8; k++) c = c & 1 ? 0xedb88320 ^ (c >>> 1) : c >>> 1; t[n] = c >>> 0; } return t; })();
const crc32 = (b) => { let c = 0xffffffff; for (const x of b) c = crcTable[(c ^ x) & 0xff] ^ (c >>> 8); return (c ^ 0xffffffff) >>> 0; };
function png(w, h, px) {
  const raw = Buffer.alloc((w * 4 + 1) * h);
  for (let y = 0; y < h; y++) { raw[y * (w * 4 + 1)] = 0; for (let x = 0; x < w; x++) { const [r, g, b, a] = px(x, y); const o = y * (w * 4 + 1) + 1 + x * 4; raw[o] = r; raw[o + 1] = g; raw[o + 2] = b; raw[o + 3] = a; } }
  const chunk = (type, data) => { const out = Buffer.alloc(12 + data.length); out.writeUInt32BE(data.length, 0); out.write(type, 4, 'ascii'); data.copy(out, 8); out.writeUInt32BE(crc32(out.subarray(4, 8 + data.length)), 8 + data.length); return out; };
  const ihdr = Buffer.alloc(13); ihdr.writeUInt32BE(w, 0); ihdr.writeUInt32BE(h, 4); ihdr[8] = 8; ihdr[9] = 6;
  return Buffer.concat([Buffer.from([0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a]), chunk('IHDR', ihdr), chunk('IDAT', deflateSync(raw)), chunk('IEND', Buffer.alloc(0))]);
}
const IMGS = [
  { ext: 'png', bytes: png(20, 12, (x, y) => [x * 12, y * 20, 90, 255]) },
  { ext: 'png', bytes: png(16, 16, (x, y) => ((x - 8) ** 2 + (y - 8) ** 2 < 40 ? [200, 30, 30, 255] : [0, 0, 0, 0])) },
  { ext: 'png', bytes: png(8, 8, () => [0, 0, 0, 0]) },
  { ext: 'png', bytes: png(600, 40, (x, y) => (x > 100 && x < 500 && y > 8 && y < 32 ? [30, 90, 200, 255] : [0, 0, 0, 0])) },
  { ext: 'jpg', bytes: null }, // made by the page (a canvas JPEG)
];

const revive = (v) => (v && typeof v === 'object' ? (v.__u8 !== undefined ? Uint8Array.from(Buffer.from(v.__u8, 'base64')) : (Array.isArray(v) ? v.map(revive) : Object.fromEntries(Object.entries(v).map(([k, x]) => [k, revive(x)])))) : v);

let lastTrace = '';
function digest(paras, trace = false) {
  const s = new Stream({ trace });
  const c = new Canon(s);
  paras.forEach((p, i) => { s.tag('E').u32(i); c.value(p, 'paragraph'); });
  const r = s.finish().short;
  if (trace) lastTrace = s.trace();
  return r;
}
const sha = (t) => Digest.of(Buffer.from(t, 'utf8')).short;

const out = new CaptureWriter([
  'D2b-F7 session in Chromium: the frozen <universal-write> driven by seeded scripts (object selection, the selection pane, shapes, text boxes, pictures, the two-phase commands)',
  'facets: op, paras, sel, anc, ev, flags, docxml, fs, tb, pane, st, fx, enc, n (img#<k>: b64)',
]);
out.meta('steps', STEPS);
out.meta('browser', `chromium ${browser.version()}`);
out.meta('bundle', Digest.of(readFileSync(join(root, 'write/packages/editor/publish/index.js'))).short);
out.meta('math-override', mo.meta);

/** Documents with drawings first (60%), then the rest. */
function pick() {
  const all = corpus();
  const rich = [];
  const plain = [];
  for (const f of all) {
    let t = '';
    try { t = DocxPackage.load(readBytes(f.file)).getPartText('word/document.xml') ?? ''; } catch { continue; }
    (/<w:drawing\b|<w:pict\b|<wps:|<pic:pic|<w:txbxContent/.test(t) ? rich : plain).push(f);
  }
  const take = (list, n, mod) => list.filter((_, i) => i % mod === 0).slice(0, n);
  return [...take(rich, Math.ceil(DOCS * 0.6), 1), ...take(plain, Math.floor(DOCS * 0.4), 23)].slice(0, DOCS);
}
const files = pick();

// the JPEG of the set (a canvas): one page, once
{
  const page = await context.newPage();
  await page.goto(`http://127.0.0.1:${port}/__d2b.html`);
  const b64 = await page.evaluate(async () => {
    const c = new OffscreenCanvas(24, 18);
    const x = c.getContext('2d');
    for (let i = 0; i < 24; i++) { x.fillStyle = `rgb(${i * 10},${200 - i * 5},${i * 3})`; x.fillRect(i, 0, 1, 18); }
    const b = await c.convertToBlob({ type: 'image/jpeg', quality: 0.9 });
    const u = new Uint8Array(await b.arrayBuffer());
    let t = '';
    for (let i = 0; i < u.length; i += 0x8000) t += String.fromCharCode(...u.subarray(i, i + 0x8000));
    return btoa(t);
  });
  IMGS[4].bytes = Buffer.from(b64, 'base64');
  await page.close();
}
IMGS.forEach((im, k) => { out.set(`img#${k}`, 'ext', im.ext); out.set(`img#${k}`, 'b64', im.bytes.toString('base64')); });

const WRAPS = ['inline', 'square', 'tight', 'through', 'topAndBottom', 'behind', 'front'];
const POSITIONS = ['inline', 'top-left', 'top-center', 'top-right', 'center-left', 'center-center', 'bottom-right', 'bottom-center', 'nope'];
const SHAPES = ['rect', 'ellipse', 'triangle', 'roundRect', 'star5', 'nope'];
const EFFECTS = ['shadow', 'reflection', 'glow', 'softEdge', 'bevel', 'rotation'];
const PRESET_IDS = [
  ['outer-br', 'outer-b', 'inner-t', 'nope'],
  ['tight-4', 'half-4', 'full-8', 'nope'],
  ['glow-0-1', 'glow-3-6', 'glow-9-9', 'nope', ''],
  ['5', '10', '25', '0', 'x'],
  ['circle', 'relaxedInset', 'hardEdge', 'nope', ''],
  ['x'],
];
const RECOLORS = ['grayscale', 'washout', 'blackwhite', 'sepia', 'nope', '#336699'];
const TBOXES = ['simple', 'quote', 'sidebarRight', 'sidebarLeft', 'band', 'note', 'nope'];
const ARRANGE = ['front', 'back', 'forward', 'backward', 'frontOfText', 'behindText'];
const ALIGN = ['left', 'center', 'right', 'top', 'middle', 'bottom', 'distributeH', 'distributeV'];

let n = 0;
for (const { rel, file } of files) {
  const page = await context.newPage();
  await page.goto(`http://127.0.0.1:${port}/__d2b.html`);
  await page.waitForFunction(() => window.__ready === true);
  const bytes = readBytes(file);
  const ok = await page.evaluate(async ([b64, EVENTS]) => {
    const el = document.createElement('universal-write');
    el.setAttribute('fonts', '/write/packages/editor/demo/fonts');
    const ready = new Promise((r) => el.addEventListener('uw-ready', r, { once: true }));
    document.body.append(el);
    await ready;
    window.__el = el;
    window.__ev = [];
    for (const t of EVENTS) el.addEventListener(t, (e) => { const d = e.detail ?? {}; window.__ev.push(`${t}:${d.dirty ?? d.active ?? d.on ?? ''}`); });
    // log the layout facts the objects' commands read
    const rect = (h) => [h.rect.x, h.rect.y, h.rect.w, h.rect.h, h.page, h.inline ? 1 : 0];
    const wrap = (name, tag, enc) => { const f = el[name].bind(el); el[name] = (...a) => { const v = f(...a); if (window.__fxOn) window.__fx.push([tag, ...enc(v)]); return v; }; };
    wrap('selectedFloat', 'sf', (h) => (h ? rect(h) : ['null']));
    wrap('selectedFloats', 'sfs', (hs) => hs.flatMap(rect));
    wrap('engObjectFrame', 'fr', (f) => (f ? [f.pageW, f.pageH, f.marL, f.marR, f.marT, f.marB, f.colX, f.colW, f.bodyTop, f.paraTop] : ['null']));
    wrap('engObjectOf', 'eo', (o) => (o ? [o.x, o.y, o.w, o.h, o.page] : ['null']));
    try {
      el.author = 'A';
      el.loadDocx(Uint8Array.from(atob(b64), (c) => c.charCodeAt(0)), { protected: false });
      return !!el.fonts;
    } catch { return false; }
  }, [Buffer.from(bytes).toString('base64'), EVENTS]);
  if (!ok) { await page.close(); continue; }
  let seed = 0xf7f7;
  for (let i = 0; i < rel.length; i++) seed = (Math.imul(seed, 31) + rel.charCodeAt(i)) >>> 0;
  const rng = new Rng(seed);
  const info = () => page.evaluate(() => {
    const el = window.__el;
    const items = el.selectionPaneItems();
    const si = el.selectedImage();
    const sh = el.selectedShape();
    const inl = [];
    el.model.paragraphs.forEach((p, pi) => p.runs.forEach((r, ri) => { if (r.image) inl.push([pi, ri]); }));
    const sel = si ? { ext: si.format ? el.picRun().image.ext : '', src: si.format.srcRect, flip: si.flipH || si.flipV, inline: si.inline, nat: si.naturalPt } : null;
    return {
      lens: el.model.paragraphs.map((p) => p.runs.reduce((a, r) => a + r.text.length, 0)),
      keys: items.map((i) => i.key),
      nobj: el.contextDrawingRuns(el.model).length,
      inl,
      img: sel,
      shape: sh ? { kind: sh.kind } : null,
      sel: !!el.drawRun(),
      tb: !!el.tbEdit,
      cell: !!el.cellEdit,
      accent: el.documentThemeColors()['accent1'] ?? '156082',
      maxw: el.contentWidthTwips(),
    };
  });
  const reset = () => page.evaluate(() => { window.__fx.length = 0; });
  for (let j = 0; j <= STEPS; j++) {
    const key = `${rel}#${j}`;
    let code = 'init';
    if (j > 0) {
      const I = await info();
      await reset();
      const L = I.lens;
      const pos = () => { const para = rng.below(L.length); return { para, offset: rng.below(L[para] + 1) }; };
      const pickKey = () => (I.keys.length ? rng.below(I.keys.length + 1) : 0);
      const k = rng.below(100);
      const num = () => [0, 1, 15, 45, 90, 180, 359, -30, 400, 12.5][rng.below(10)];
      const img = I.img;
      if (!I.sel && rng.below(100) < 45) {
        // nothing selected: select something, or put a picture in
        const q = rng.below(10);
        if (q < 4 && I.nobj) code = `so:${rng.below(I.nobj)}`;
        else if (q < 6 && I.inl.length) { const [pi, ri] = I.inl[rng.below(I.inl.length)]; code = `si:${pi}:${ri}`; }
        else if (q < 7 && I.keys.length) code = `pi:${rng.below(I.keys.length)}:0`;
        else if (q < 9) code = `ins:${rng.below(IMGS.length)}:${I.maxw}`;
        else code = `itb:${TBOXES[rng.below(TBOXES.length - 1)]}:${I.accent}`;
      } else if (k < 6) code = `ty:${hex(TEXTS[rng.below(TEXTS.length)])}`;
      else if (k < 8) code = 'en';
      else if (k < 10) code = 'bs';
      else if (k < 14) { const a = pos(); const f = rng.below(2) === 0 ? a : pos(); code = `se:${a.para}:${a.offset}:${f.para}:${f.offset}`; }
      else if (k < 17) code = 'un';
      else if (k < 19) code = 're';
      else if (k < 26) code = `so:${rng.below(I.nobj + 1)}`;
      else if (k < 29 && I.inl.length) { const [pi, ri] = I.inl[rng.below(I.inl.length)]; code = `si:${pi}:${ri}`; }
      else if (k < 33) code = `pi:${pickKey()}:${rng.below(2)}`;
      else if (k < 34) code = `pm:${pickKey()}:${pickKey()}`;
      else if (k < 35) code = `pr:${pickKey()}:${hex(['Nome', 'a<b>&"c', ''][rng.below(3)])}`;
      else if (k < 36) code = `ph:${rng.below(2) ? pickKey() : '-'}:${rng.below(2)}`;
      else if (k < 38) code = `sf:${rng.below(5)}`;
      else if (k < 39) code = `sol:${rng.below(4)}`;
      else if (k < 41) code = `sty:${[0, 1, 2, 42, 43, 77, 78, 1.5][rng.below(8)]}`;
      else if (k < 42) code = `chs:${SHAPES[rng.below(SHAPES.length)]}`;
      else if (k < 43) code = `tbo:${rng.below(4)}`;
      else if (k < 44) { const e = rng.below(6); code = `sef:${EFFECTS[e]}:${rng.below(PRESET_IDS[e].length + 1)}`; }
      else if (k < 45) code = 'dsh';
      else if (k < 49) code = `itb:${TBOXES[rng.below(TBOXES.length)]}:${I.accent}`;
      else if (k < 50) code = `lk:${pickKey()}`;
      else if (k < 51) code = 'bl';
      else if (k < 52) code = 'sdt';
      else if (k < 57) {
        // setImageWrap: tight/through only where the contour has no crop/flip to reproduce
        let w = WRAPS[rng.below(WRAPS.length)];
        if ((w === 'tight' || w === 'through') && img && (img.flip || img.src.l || img.src.t || img.src.r || img.src.b)) w = 'square';
        code = `iw:${w}`;
      }
      else if (k < 59) code = `ip:${POSITIONS[rng.below(POSITIONS.length)]}`;
      else if (k < 60) code = `imw:${rng.below(2)}`;
      else if (k < 62) code = `isz:${[40, 100, 200, 'x'][rng.below(4)]}:${[30, 60, 'x'][rng.below(3)]}:${['x', 0, 1][rng.below(3)]}`;
      else if (k < 63) code = `ir:${num()}`;
      else if (k < 64) code = `rt:${num()}`;
      else if (k < 65) code = `fl:${rng.below(2) ? 'h' : 'v'}`;
      else if (k < 66) code = `ia:${hex(['alt', '', 'a<b'][rng.below(3)])}:${rng.below(2) ? hex('titulo') : 'x'}:${['x', 0, 1][rng.below(3)]}`;
      else if (k < 68) code = `ar:${ARRANGE[rng.below(6)]}`;
      else if (k < 69) code = `cff:${rng.below(2) ? 'fill' : 'fit'}:${img && img.nat ? `${img.nat.w}:${img.nat.h}` : 'x:x'}`;
      else if (k < 70) code = `cta:${[1, 16, 4, 3][rng.below(4)]}:${[1, 9, 3, 4][rng.below(4)]}`;
      else if (k < 71) code = `cts:${SHAPES[rng.below(SHAPES.length)]}`;
      else if (k < 72) code = `icr:${[0, 10, 25, -5][rng.below(4)]}:${[0, 5][rng.below(2)]}:${[0, 10, 30][rng.below(3)]}:${[0, 20][rng.below(2)]}`;
      else if (k < 73) code = `ib:${rng.below(4)}`;
      else if (k < 75) { const e = rng.below(6); code = `ef:${EFFECTS[e]}:${rng.below(PRESET_IDS[e].length + 1)}`; }
      else if (k < 76) code = `it:${[0, 30, 100, 50.5, -5][rng.below(5)]}`;
      else if (k < 77) code = `irc:${rng.below(RECOLORS.length)}`;
      else if (k < 78) code = `itc:${rng.below(2) ? 'FF0000' : '-'}`;
      else if (k < 79) code = `ire:${rng.below(2)}:${img && img.nat ? `${img.nat.w}:${img.nat.h}` : 'x:x'}`;
      else if (k < 80) code = 'dim';
      else if (k < 82) { let n = rng.below(6); if (n === 5 && img && (img.flip || img.src.l || img.src.t || img.src.r || img.src.b)) n = 0; code = `isl:${n}`; }
      else if (k < 86) code = `ins:${rng.below(IMGS.length)}:${I.maxw}`;
      else if (k < 87) code = `chg:${rng.below(IMGS.length)}`;
      else if (k < 90) code = `adj:${rng.below(6)}`;
      else if (k < 93) code = `cmp:${[96, 150, 220, 600][rng.below(4)]}:${rng.below(2)}:${rng.below(2)}`;
      else if (k < 95) code = `al:${ALIGN[rng.below(8)]}`;
      else if (k < 97) code = `ial:${ALIGN[rng.below(6)]}:${['margin', 'page', 'x'][rng.below(3)]}`;
      else if (k < 98) code = `spf:${rng.below(IMGS.length)}:${rng.below(2)}`;
      else if (k < 99) code = 'est';
      else code = `cdt:${[0, 600, 1200][rng.below(3)]}:${[0, 400, 900][rng.below(3)]}:${[1200, 2400][rng.below(2)]}:${[600, 1500][rng.below(2)]}:${rng.below(2)}:${rng.below(2) ? rng.below(L.length) : 'x'}`;
      // in a text box: the text edits and the picture commands on the run model of the box content
      if (I.tb && rng.below(100) < 45) {
        const q = rng.below(8);
        code = q < 3 ? `ty:${hex(TEXTS[rng.below(TEXTS.length)])}` : q < 4 ? 'en' : q < 5 ? 'bs' : q < 7 ? `ins:${rng.below(IMGS.length)}:${I.maxw}` : `chg:${rng.below(IMGS.length)}`;
      }
      // F7-D11: the draw mode of the UI leaves a box first; the port refuses a drawn box inside one
      if (I.tb && /^cdt(:|$)/.test(code)) code = `pi:${pickKey()}:0`;
      // the click that selects an object leaves the box first; a bare assignment of `floatSel` does not
      if (I.tb && /^(so|si)(:|$)/.test(code)) code = `pi:${pickKey()}:0`;
      // F7-D10: typing, Enter, Backspace or insertImage with an object selected (outside a box): the TS keeps
      // `floatSel` as the run object, and whether the text edit replaces that object is the TS edit engine's
      // identity, which the port (runs by value) does not model. UO_F7_INCLUDE=sel puts these steps back.
      if (!(process.env.UO_F7_INCLUDE ?? '').includes('sel') && I.sel && !I.tb && /^(ty|en|bs|ins)(:|$)/.test(code)) code = `se:0:0:0:0`;
      const res = await page.evaluate(async ([code, I, IMGS_B64, TBOXES, RECOLORS, PRESET_IDS]) => {
        const el = window.__el;
        window.__ev = [];
        const f = code.split(':');
        const nn = (x) => (x === 'x' ? undefined : +x);
        const hs = (h) => String.fromCharCode(...h.split('.').filter(Boolean).map((x) => parseInt(x, 16)));
        const key = (i) => (+i < I.keys.length ? I.keys[+i] : 'nope');
        const col = (i) => [{ kind: 'srgbClr', val: 'FF0000' }, { kind: 'schemeClr', val: 'accent1' }, { kind: 'srgbClr', val: '336699' }, { kind: 'prstClr', val: 'black' }][i % 4];
        const bytesOf = (k) => Uint8Array.from(atob(IMGS_B64[k][1]), (c) => c.charCodeAt(0));
        const eff = (e, i) => { const l = PRESET_IDS[['shadow', 'reflection', 'glow', 'softEdge', 'bevel', 'rotation'].indexOf(e)]; return +i < l.length ? l[+i] : null; };
        let ret = null;
        window.__fxOn = true;
        // the pixels matter to the commands with jobs; the paint of the effects reads them too, and is not a job
        window.__fxPix = ['adj', 'cmp', 'iw', 'isl', 'ip', 'ins', 'chg'].includes(f[0]);
        // `insertImage` and `changeImage` read the size only; the pixel reads of their step are the paint
        window.__fxNoGid = ['ins', 'chg'].includes(f[0]);
        try {
        switch (f[0]) {
          case 'ty': el.type(hs(f[1])); break;
          case 'en': el.pressEnter(); break;
          case 'bs': el.pressBackspace(); break;
          case 'se': el.setSelection({ para: +f[1], offset: +f[2] }, { para: +f[3], offset: +f[4] }); break;
          case 'un': el.undo(); break;
          case 're': el.redo(); break;
          case 'so': { const runs = el.contextDrawingRuns(el.model); el.floatSel = runs[+f[1]] ?? null; el.floatMulti = []; break; }
          case 'si': { const r = el.model.paragraphs[+f[1]]?.runs[+f[2]]; if (r?.image) el.selectInlineRun(r); break; }
          case 'pi': el.selectPaneItem(key(f[1]), f[2] === '1'); break;
          case 'pm': el.selectPaneItems([key(f[1]), key(f[2])]); break;
          case 'pr': el.renamePaneItem(key(f[1]), hs(f[2])); break;
          case 'ph': el.setPaneItemHidden(f[1] === '-' ? null : key(f[1]), f[2] === '1'); break;
          case 'sf': el.setShapeFill([null, col(1), col(0), col(2), { kind: 'schemeClr', val: 'accent2', mods: [['lumMod', 50000]] }][+f[1]]); break;
          case 'sol': el.setShapeOutline([{ color: col(0) }, { color: null }, { weightPt: 3, dash: 'dash' }, { color: col(2), weightPt: 0.5, compound: 'dbl' }][+f[1]]); break;
          case 'sty': el.setShapeStyle(+f[1]); break;
          case 'chs': el.changeShape(f[1]); break;
          case 'tbo': el.setTextboxOptions([{ marginsPt: { l: 10, t: 5 } }, { anchor: 'ctr', wrap: false }, { direction: 'vert', autofit: 'shape' }, { anchor: 'b', upright: true, autofit: 'none' }][+f[1]]); break;
          case 'sef': el.setShapeEffect(f[1], eff(f[1], f[2])); break;
          case 'dsh': el.deleteSelectedShape(); break;
          case 'itb': ret = el.insertTextbox(f[1]); break;
          case 'spf': await el.setShapePictureFill(bytesOf(+f[1]), IMGS_B64[+f[1]][0], f[2] === '1'); break;
          case 'est': ret = el.editSelectedTextbox(); break;
          case 'cdt': el.createDrawnTextbox(0, { x: +f[1], y: +f[2], w: +f[3], h: +f[4] }, f[5] === '1', f[6] === 'x' ? null : { para: +f[6], offset: 0 }); break;
          case 'lk': el.linkTextboxTo(key(f[1])); break;
          case 'bl': el.breakTextboxLink(); break;
          case 'sdt': el.setAsDefaultTextbox(); break;
          case 'iw': el.setImageWrap(f[1]); break;
          case 'ip': el.setImagePosition(f[1]); break;
          case 'imw': el.setImageMoveWithText(f[1] === '1'); break;
          case 'isz': el.setImageSize({ widthPt: nn(f[1]), heightPt: nn(f[2]), ...(f[3] === 'x' ? {} : { lockAspect: f[3] === '1' }) }); break;
          case 'ir': el.setImageRotation(+f[1]); break;
          case 'rt': el.rotateImage(+f[1]); break;
          case 'fl': el.flipImage(f[1]); break;
          case 'ia': el.setImageAltText({ text: hs(f[1]), ...(f[2] === 'x' ? {} : { title: hs(f[2]) }), ...(f[3] === 'x' ? {} : { decorative: f[3] === '1' }) }); break;
          case 'ar': el.arrangeImage(f[1]); break;
          case 'cff': el.cropImageFillFit(f[1]); break;
          case 'cta': el.cropImageToAspect(+f[1], +f[2]); break;
          case 'cts': el.cropImageToShape(f[1]); break;
          case 'icr': el.setImageCrop({ l: +f[1], t: +f[2], r: +f[3], b: +f[4] }); break;
          case 'ib': el.setImageBorder([{ color: col(0), weightPt: 2, dash: 'dash' }, { color: null }, { weightPt: 4 }, { color: col(2) }][+f[1]]); break;
          case 'ef': el.setImageEffect(f[1], eff(f[1], f[2])); break;
          case 'it': el.setImageTransparency(+f[1]); break;
          case 'irc': { const r = RECOLORS[+f[1]]; el.setImageRecolor(r.startsWith('#') ? { kind: 'srgbClr', val: r.slice(1) } : r); break; }
          case 'itc': el.setImageTransparentColor(f[1] === '-' ? null : f[1]); break;
          case 'ire': el.resetImage(f[1] === '1'); break;
          case 'dim': el.deleteSelectedImage(); break;
          case 'isl': el.setImageLayout([
            { wrap: 'square', widthPt: 120 },
            { wrap: 'topAndBottom', posH: { relativeFrom: 'margin', align: 'center' }, posV: { relativeFrom: 'paragraph', offsetPt: 10 } },
            { wrap: 'inline' },
            { widthPt: 80, heightPt: 40, lockAspect: false, rotation: 20 },
            { posH: { relativeFrom: 'page', offsetPt: 30 }, posV: { relativeFrom: 'page', offsetPt: 40 }, wrap: 'front', moveWithText: false },
            { wrap: 'through', distPt: { t: 3, b: 4, l: 5, r: 6 }, allowOverlap: true, layoutInCell: false },
          ][+f[1]]); break;
          case 'ins': await el.insertImage(bytesOf(+f[1]), IMGS_B64[+f[1]][0]); break;
          case 'chg': await el.changeImage(bytesOf(+f[1]), IMGS_B64[+f[1]][0]); break;
          case 'adj': await el.setImageAdjust([{ bright: 0.2 }, { contrast: -0.3, saturation: 1.5 }, { sharpen: 0.5 }, { temperature: 5000 }, { bright: 0, contrast: 0, saturation: 1, temperature: 6500 }, { bright: -0.4, sharpen: 0.2 }][+f[1]]); break;
          case 'cmp': ret = await el.compressImages({ ppi: +f[1], deleteCropped: f[2] === '1', all: f[3] === '1' }); break;
          case 'al': el.alignObjects(f[1]); break;
          case 'ial': el.alignImage(f[1], f[2] === 'x' ? undefined : f[2]); break;
        }
        } finally { window.__fxOn = false; }
        return { ret };
      }, [code, I, IMGS.map((im) => [im.ext, im.bytes.toString('base64')]), TBOXES, RECOLORS, PRESET_IDS]);
      if (res.ret !== null && res.ret !== undefined) out.set(key, 'n', String(res.ret));
    }
    out.set(key, 'op', code);
    const st = await page.evaluate(() => {
      const el = window.__el;
      // the paint of the handles calls `selectedFloat()`, which drops the selection of an inline
      // picture the text selection moved off; the harness does it as the page would, before reading
      el.selectedFloat();
      const enc = (v) => (v instanceof Uint8Array ? { __u8: btoa(String.fromCharCode(...v)) } : v);
      const walk = (v) => (v && typeof v === 'object' && !(v instanceof Uint8Array) ? (Array.isArray(v) ? v.map(walk) : Object.fromEntries(Object.entries(v).map(([k, x]) => [k, walk(enc(x))]))) : enc(v));
      const state = el.parseState(el.stringifyState());
      const hx = (t) => [...Array(t.length).keys()].map((i) => t.charCodeAt(i).toString(16)).join('.');
      const fx = window.__fx.map((e) => e.map(String).join(';')).join('|');
      const puts = window.__fx.filter((e) => e[0] === 'put').map((e) => `${e[1]}x${e[2]}:${e[3]}`).join(',');
      window.__fx.length = 0;
      const fmtState = (s) => {
        if (!s) return '-';
        const f = s.format;
        const t = s.text ? `${f.body ? `${f.body.lIns},${f.body.tIns},${f.body.rIns},${f.body.bIns}` : ''}|${s.text.anchor}|${s.text.direction}|${s.text.wrap ? 1 : 0}|${s.text.autofit}|${s.text.upright ? 1 : 0}` : '-';
        return [s.kind ?? 'image', s.inline ? 1 : 0, s.wrap, f.cx, f.cy, f.rot, f.flipH ? 1 : 0, f.flipV ? 1 : 0, s.lockAspect ? 1 : 0, s.moveWithText ? 1 : 0, s.lockAnchor ? 1 : 0,
          hx(s.name), hx(s.altText), s.decorative ? 1 : 0, hx(s.geometry ?? ''), s.styleIndex ?? 0, s.editingText ? 1 : 0, t, s.link ? `${s.link.next ? 1 : 0}${s.link.previous ? 1 : 0}` : '-'].join('|');
      };
      const items = el.selectionPaneItems().map((i) => `${hx(i.key)}|${hx(i.name)}|${i.hidden ? 1 : 0}|${i.selected ? 1 : 0}|${i.kind}`).join('\n');
      const shape = el.selectedShape();
      const image = el.selectedImage();
      window.__fx.length = 0;
      return {
        paras: walk(state.m.paragraphs),
        sel: JSON.stringify(state.s),
        anc: JSON.stringify(state.anc ?? null),
        ev: window.__ev.join(' '),
        flags: `${el.isDirty() ? 1 : 0}${el.undoStack.length ? 1 : 0}${el.redoStack.length ? 1 : 0}:${el.editsSinceLoad}`,
        fs: state.fs === undefined ? '-' : String(state.fs),
        tb: state.te ? `${state.te.h}|${state.te.f}|${state.te.pg}|${JSON.stringify(state.te.s)}` : '-',
        pane: items,
        st: `${fmtState(shape)}#${fmtState(image)}`,
        fx,
        puts,
        count: el.selectedObjectCount(),
        rc: window.__rc(),
        mu: el.floatMulti.length,
        inBox: !!el.tbEdit,
        bx: (() => { const m = el.savedForTb ? el.savedForTb.model : el.model; let best = ''; for (const p of m.paragraphs) for (const r of p.runs) { const x = r.textbox?.rawDrawingXml; if (x) best = x; } return best; })(),
      };
    });
    const wantTrace = process.env.UO_F7_TRACE && key.includes(process.env.UO_F7_TRACE);
    out.set(key, 'paras', digest(revive(st.paras), !!wantTrace));
    if (wantTrace) out.set(key, 'paras.trace', Buffer.from(lastTrace, 'utf8').toString('base64'));
    out.set(key, 'sel', st.sel);
    out.set(key, 'anc', st.anc);
    out.set(key, 'ev', st.ev || '-');
    out.set(key, 'flags', st.flags);
    out.set(key, 'fs', st.fs);
    out.set(key, 'tb', st.tb);
    out.set(key, 'pane', sha(st.pane));
    out.set(key, 'st', sha(st.st));
    out.set(key, 'fx', st.fx || '-');
    out.set(key, 'enc', st.puts || '-');
    out.set(key, 'cnt', String(st.count));
    out.set(key, 'rc', String(st.rc));
    out.set(key, 'mu', String(st.mu));
    if (process.env.UO_F7_TEXT && code.startsWith('itb:')) out.set(key, 'bx.text', st.bx.split('\n').join(' '));
    if (process.env.UO_F7_TEXT) { out.set(key, 'pane.text', st.pane.split('\n').join(' ~ ')); out.set(key, 'st.text', st.st); }
    // the file is saved from the outside story: a save inside a box is not compared (the TS drops the tables of the body there: F7-O3)
    if (!st.inBox && (j % 8 === 0 || j === STEPS || process.env.UO_F7_DXALL)) {
      const b64 = await page.evaluate(() => { try { const b = window.__el.saveDocx(); let s = ''; for (let i = 0; i < b.length; i += 0x8000) s += String.fromCharCode(...b.subarray(i, i + 0x8000)); return btoa(s); } catch { return null; } });
      let d = 'err';
      if (b64) {
        const pkg = DocxPackage.load(Uint8Array.from(Buffer.from(b64, 'base64')));
        const dx = pkg.getPartText('word/document.xml') ?? '';
        d = Digest.of(Buffer.from(dx, 'utf8')).short;
        if (process.env.UO_F7_TEXT && (code.startsWith('itb:') || process.env.UO_F7_DXALL)) out.set(key, 'dx.full', Buffer.from(dx, 'utf8').toString('base64'));
      }
      out.set(key, 'docxml', d);
    }
  }
  await page.close();
  n++;
}
await browser.close();
server.close();
out.write(join(crate, 'tests', 'parity', 'd2b-f7-session.capture'));
console.log(`d2b-f7-session: ${n} documents x ${STEPS} steps`);
