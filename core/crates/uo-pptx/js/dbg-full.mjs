// Debug helper (B4): full-form TS stream vs the full-form stream of the interpreter over the Rust list, for one deck/slide/view.
//   node dbg-full.mjs <deck key> <slide> <view a|b> <uolf dir> [--ts-root dir]
import { readFileSync } from 'node:fs';
import { join } from 'node:path';
import { args, capturesDir, loadSlidesCore, readSample, tsRoot } from './lib/common.mjs';
import { GROUPS, buildGroupDeck, deckFile } from './lib/gen/groups.mjs';
import { recordTsSlide } from './lib/ts-paint.mjs';
import { recordList } from './lib/rust-paint.mjs';
import { VIEWS } from './lib/views.mjs';
import { decodeSlidesList, newTables } from '../../uo-wasm/js/gen/slides.ts';

const a = args();
const [key, slideNo, view, dir] = a.argv;
const core = await loadSlidesCore(tsRoot(a));
function deckBytes(k) {
  if (k.startsWith('samples/')) return readSample(k.slice('samples/'.length));
  const m = /^gen\/([^/]+)\/(\d+)$/.exec(k);
  return new Uint8Array(readFileSync(join(capturesDir, 'decks', m[1], deckFile(m[1], Number(m[2])))));
}
const bytes = deckBytes(key);
const pkg = core.PptxPackage.load(bytes);
const pres = core.readPresentation(pkg);
const slide = core.readSlide(pkg, pres.slides[Number(slideNo)].target);
const v = VIEWS[view](pres);
const ts = await recordTsSlide({ tsRoot: tsRoot(a), slide, pres, ...v });
// frames
const buf = readFileSync(join(dir, `${key.replace(/\//g, '__')}.uolf`));
let p = 5; const frames = [];
const u32 = () => { const x = buf.readUInt32LE(p); p += 4; return x; };
while (p < buf.length) { const n = u32(); const list = new Uint8Array(buf.subarray(p, p + n)); p += n; const nb = u32(); const bm = []; for (let i = 0; i < nb; i++) { const l = u32(); bm.push(new Uint8Array(buf.subarray(p, p + l))); p += l; } frames.push({ list, bm }); }
const tables = newTables();
const bitmaps = [];
let idx = -1;
// the entries of this deck in capture order are unknown here: decode every frame up to the wanted one by slide order
const want = Number(slideNo) * 2 + (view === 'b' ? 1 : 0);
let list;
for (let i = 0; i < frames.length; i++) { const l = decodeSlidesList(frames[i].list, tables); for (const b of frames[i].bm) bitmaps.push(b); if (i === (a.opt('--frame') ? Number(a.opt('--frame')) : want)) { list = l; break; } }
const [w, h] = [Math.ceil(pres.slideW * v.scale + 2 * v.ox), Math.ceil(pres.slideH * v.scale + 2 * v.oy)];
const r = recordList({ list, tables, width: w, height: h, bitmap: (i) => bitmaps[i], media: (i) => { const m = tables.media[i]; return m ? pkg.getPart(m.part) ?? null : null; } });
const onlyCv = a.flag('--cv');
const keep = (l) => (onlyCv ? l.startsWith('@cv') : !/^(set font|set fontKerning|measure|set letterSpacing)/.test(l));
const A = ts.text.split('\n').filter(keep), B = r.text.split('\n').filter(keep);
console.log('lines', A.length, B.length, 'error', ts.error, r.error);
let n = 0;
for (let i = 0; i < Math.max(A.length, B.length) && n < Number(a.opt('--show', '6')); i++) if (A[i] !== B[i]) { n++; console.log(`@${i}\n TS: ${A[i]}\n RS: ${B[i]}`); }
