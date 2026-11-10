// Parity capture of frontier V of uo-render (painter parity): the frozen TS `layoutDocument` lays out every committed fixture
// document (open fonts only, the 7 hyphenation sets), and the frozen canvas painter (`paintPageBackground` + `paintItems`,
// canvas.js) paints every page on a RECORDING context (crates/uo-wasm/tests/support/recording-context.mjs) in four
// configurations (kerning on/off x two scales). The log of every call and property set is digested per document and
// configuration. crates/uo-wasm/tests/paint_parity.test.mjs lays the same documents out with the wasm engine, decodes
// the encoded pages and runs the TS painter of decoded pages (js/paint.ts) on the same recording context in the same
// order: equal digests = the same draw calls.
//
//   UO_TS_ROOT=<frozen checkout> node --max-old-space-size=8192 tools/capture-v.mjs      (or --ts-root)
//
// Writes tests/captures/v_paint.capture. Taken on win32 Node v24.15.0 like E-D8 (the gradient samples use `**`).
// The frozen painter keeps module-level caches (page-fill tiles, shape-effect layers); the pages are painted in a FIXED order
// in ONE process, and the replay paints in the same order with fresh caches.
process.env.TZ = 'UTC';
import { readFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';
import { CaptureWriter, Stream, importFrom, tsRoot } from '../../uo-testkit/js/testkit.mjs';
import { Recording, DigestSink, bitmapOf, fontOf, colorOf } from '../../uo-wasm/tests/support/recording-context.mjs';
import { CONFIGS, NOW, fixtureDocs, relDoc } from '../../uo-wasm/tests/support/paint-common.mjs';

const here = dirname(fileURLToPath(import.meta.url));
const argv = process.argv.slice(2);
const opt = (name) => { const i = argv.indexOf(name); return i >= 0 ? argv[i + 1] : undefined; };
const outDir = opt('--out-dir') ?? join(here, '..', 'tests', 'captures');
const root = tsRoot(argv);
const only = opt('--only');

// the open fonts only (no system fonts): the same setup as uo-pdf capture_painter.mjs --open-fonts
process.env.WINDIR = join(here, '__no_system_fonts__');
process.env.LOCALAPPDATA = join(here, '__no_system_fonts__');
process.env.EXTRA_FONT_DIRS = [join(root, 'shared/fonts/files'), join(root, 'site/assets/fonts')].join(';');

const D = 'write/packages/docx-layout/dist/';
const tscore = await importFrom(root, 'write/packages/docx-core/dist/index.js');
const eng = await importFrom(root, `${D}index.js`);
const canvas = await import(pathToFileURL(join(root, D, 'canvas.js')).href);
const { createNodeFontProvider } = await importFrom(root, 'write/tools/corpus-lab/node-fonts.mjs');
const provider = await createNodeFontProvider();
await eng.loadHyphenation(eng.HYPHENATION_PATTERN_IDS);

const ENV = { fontOf, image: (img) => bitmapOf(img.data), color: colorOf };

const cap = new CaptureWriter(['Parity capture of frontier V of uo-render (painter parity): written by core/crates/uo-render/tools/capture-v.mjs, replayed by core/crates/uo-wasm/tests/paint_parity.test.mjs.']);
const rec = new Recording(null).install();
let docs = 0, pages = 0, items = 0, calls = 0;
let files = fixtureDocs();
if (only) files = files.filter((f) => f.includes(only));
for (const file of files) {
  const buf = readFileSync(file);
  const pkg = tscore.DocxPackage.load(new Uint8Array(buf));
  const full = tscore.readFullDocument(pkg);
  let embedded = [];
  try { embedded = tscore.readEmbeddedFonts(pkg); } catch { /* none */ }
  const res = eng.layoutDocument(full, eng.withEmbeddedFonts(provider, embedded, (b) => new eng.SfntFont(b)), { now: new Date(NOW) });
  const streams = CONFIGS.map(() => ({ s: new Stream(), sink: null }));
  streams.forEach((x) => { x.sink = new DigestSink(x.s); });
  let docItems = 0;
  res.pages.forEach((page, pi) => {
    docItems += page.items.length;
    CONFIGS.forEach((cfg, ci) => {
      const { s, sink } = streams[ci];
      s.str('page'); s.u32(pi);
      rec.sink = sink; rec.kerning = cfg.kerning; rec.scale = cfg.scale;
      const ctx = rec.pageContext();
      canvas.paintPageBackground(ctx, page, ENV, { devicePxTw: 1 / cfg.scale, screen: cfg.screen, paper: '#ffffff' });
      canvas.paintItems(ctx, page.items, ENV, cfg.alpha === undefined ? {} : { alpha: cfg.alpha });
    });
  });
  const key = relDoc(file);
  cap.set(key, 'pages', res.pages.length);
  cap.set(key, 'items', docItems);
  CONFIGS.forEach((cfg, ci) => { cap.set(key, cfg.name, `${streams[ci].sink.calls}:${streams[ci].s.finish().short}`); });
  docs++; pages += res.pages.length; items += docItems; calls += streams[0].sink.calls;
}
rec.uninstall();
cap.meta('documents', docs);
cap.meta('pages', pages);
cap.meta('items', items);
cap.meta('calls-k1s1', calls);
cap.write(join(outDir, 'v_paint.capture'));
console.log(`v_paint.capture: ${docs} documents, ${pages} pages, ${items} items, ${calls} calls (config k1s1)`);
