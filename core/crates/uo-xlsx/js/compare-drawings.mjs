// The drawings parity of the Sheet viewport (frontier C4b; record core/docs/modules/uo-xlsx.md "C4b — as built"): for every
// workbook of a viewport capture (`capture-viewport.mjs`) the Rust viewer emits the frames of the capture's states
// (`cargo run -p uo-xlsx --features view --release --example view_dump`), the drawing phases of each frame (the item ranges the
// viewer reports: `paintDrawings` per quadrant, `paintChartsheet`) are replayed by `paint-slides.ts` over the Slides recording
// context (`uo-pptx/js/lib/record-ctx.mjs`), in the draw form, and their digest is compared with the TS facet `d<i>@<k>`.
//
//   node core/crates/uo-xlsx/js/compare-drawings.mjs [--capture <file>] [--exe <view_dump.exe>] [--only <key substring>]
//        [--lines <dir of capture-viewport.mjs --lines>] [--show <n>] [--all] [--dump <dir>: the Rust text of each differing frame]
//   exit 0 when every compared frame is equal; `--all` also compares workbooks without drawings (empty phases).
import { execFileSync } from 'node:child_process';
import { createHash } from 'node:crypto';
import { existsSync, mkdirSync, readFileSync, writeFileSync } from 'node:fs';
import { join } from 'node:path';
import { gunzipSync } from 'node:zlib';
import { readCapture } from '../../uo-testkit/js/testkit.mjs';
import { form } from '../../uo-pptx/js/lib/opstream.mjs';
import { recordList } from '../../uo-pptx/js/lib/rust-paint.mjs';
import { decodeSlidesList, newTables } from '../../uo-wasm/js/gen/slides.ts';
import * as C from './lib/common.mjs';
import { unzip } from './lib/unzip.mjs';
import { PAINT_GROUPS, buildPaintWorkbook } from './lib/gen-paint.mjs';

const a = C.args();
const capPath = a.opt('--capture', join(C.capturesDir, 'sheet-viewport.capture'));
const exe = a.opt('--exe', join(C.coreDir, 'target', 'sheet-c4', 'release', 'examples', process.platform === 'win32' ? 'view_dump.exe' : 'view_dump'));
const only = a.opt('--only');
const linesDir = a.opt('--lines');
const show = Number(a.opt('--show', '3'));
const all = a.flag('--all');
const dump = a.opt('--dump');
if (dump) mkdirSync(dump, { recursive: true });
const tmp = join(C.coreDir, 'target', 'sheet-cap', 'drawings-tmp');
mkdirSync(tmp, { recursive: true });

const sha = (b) => createHash('sha256').update(b).digest('hex');
const dig = (s) => { const b = Buffer.from(s, 'utf8'); return `${b.length}:${sha(b).slice(0, 32)}`; };

/** The bytes of a capture key (committed groups, the local big groups, the samples). */
function bytesOf(key) {
  const m = /^gen\/([^/]+)\/(\d+)$/.exec(key);
  if (key.startsWith('samples/')) return C.readSample(key.slice('samples/'.length));
  if (!m) return null;
  const [, g, n] = m;
  const committed = join(C.capturesDir, 'workbooks', g, `${g}-${n}.xlsx`);
  if (existsSync(committed)) return new Uint8Array(readFileSync(committed));
  const big = join(C.bigDir, 'workbooks', g, `${g}-${n}.xlsx`);
  if (existsSync(big)) return new Uint8Array(readFileSync(big));
  if (PAINT_GROUPS[g]) return new Uint8Array(buildPaintWorkbook(g, Number(n)).bytes);
  return null;
}

/** The frames of a `.uovf` file. */
function readFrames(buf) {
  if (buf.length < 5 || buf.toString('latin1', 0, 4) !== 'UOVF' || buf[4] !== 1) throw new Error('not a UOVF v1 file');
  const frames = [];
  let p = 5;
  const u32 = () => { const v = buf.readUInt32LE(p); p += 4; return v; };
  while (p < buf.length) {
    const sheet = u32(), state = u32();
    const ns = u32();
    const spans = [];
    for (let i = 0; i < ns; i++) spans.push([u32(), u32()]);
    const n = u32();
    const list = new Uint8Array(buf.subarray(p, p + n)); p += n;
    const nb = u32();
    const bitmaps = [];
    for (let i = 0; i < nb; i++) {
      const raw = u32();
      const l = raw & 0x7fffffff;
      const bm = new Uint8Array(buf.subarray(p, p + l));
      bm.isRecipe = (raw & 0x80000000) !== 0;
      bitmaps.push(bm);
      p += l;
    }
    frames.push({ sheet, state, spans, list, bitmaps });
  }
  return frames;
}

/** The TS drawing lines of a frame (`--lines`), for the first difference. */
function tsLines(key, vi, k) {
  if (!linesDir) return null;
  const f = join(linesDir, `${key.replace(/[\\/]/g, '_')}#drawings.txt.gz`);
  if (!existsSync(f)) return null;
  const t = gunzipSync(readFileSync(f)).toString('utf8').split('\n');
  const i = t.indexOf(`# ${vi}@${k}`);
  if (i < 0) return null;
  const out = [];
  for (let j = i + 1; j < t.length && !t[j].startsWith('# '); j++) out.push(t[j]);
  return out;
}

const cap = readCapture(capPath);
const stats = { workbooks: 0, frames: 0, equal: 0, different: 0, missing: 0, lines: 0, objects: 0 };
let shown = 0;
const t0 = performance.now();
for (const [key, facets] of cap.records) {
  if (only && !key.includes(only)) continue;
  if (facets.get('open') !== 'ok') continue;
  const hasDrawings = [...facets].some(([f, v]) => f.startsWith('dc') && false) || [...facets].some(([f, v]) => f.startsWith('d') && !f.startsWith('dc') && v !== dig([...Array(Number(facets.get('dc' + f.slice(1)) ?? 0))].map(() => '#call').join('\n')));
  if (!hasDrawings && !all) continue;
  const bytes = bytesOf(key);
  if (!bytes) { stats.missing++; console.log(`missing  ${key}`); continue; }
  stats.workbooks++;
  const book = join(tmp, `${key.replace(/[\\/]/g, '_')}.xlsx`);
  const out = join(tmp, `${key.replace(/[\\/]/g, '_')}.uovf`);
  writeFileSync(book, bytes);
  try { execFileSync(exe, [book, out], { stdio: ['ignore', 'ignore', 'inherit'] }); } catch (e) { console.log(`exe-error  ${key}: ${e.status}`); stats.different++; continue; }
  const frames = readFrames(readFileSync(out));
  const parts = new Map(unzip(bytes).map((e) => [e.name.toLowerCase(), e.data]));
  const stacks = new Map(JSON.parse(facets.get('stacks') ?? '[]'));
  const fontStack = (f, b, i) => {
    if (f.startsWith('\u0001')) return `'${f.slice(1)}', Calibri, 'Liberation Sans', Arial, sans-serif`;
    const s = stacks.get(`${f}|${b ? 1 : 0}|${i ? 1 : 0}`);
    return s ?? `?stack(${f})`;
  };
  const tables = newTables();
  const bitmaps = [];
  // the view index of a sheet: its position among the sheets with a part (as the capture's `views`)
  const sheetIds = [...new Set(frames.map((f) => f.sheet))].sort((x, y) => x - y);
  for (const fr of frames) {
    for (const b of fr.bitmaps) bitmaps.push(b);
    if (!fr.list.length) continue;
    const list = decodeSlidesList(fr.list, tables);
    const vi = sheetIds.indexOf(fr.sheet);
    const want = facets.get(`d${vi}@${fr.state}`);
    if (want === undefined) continue;
    const calls = [];
    for (const [s, e] of fr.spans) {
      if (s === e) { calls.push(['#call']); continue; }
      const sub = { ...list, items: list.items.slice(s, e) };
      const r = recordList({
        list: sub, tables, width: 640, height: 360,
        bitmap: (idx) => (bitmaps[idx]?.isRecipe ? null : bitmaps[idx]),
        recipe: (idx) => (bitmaps[idx]?.isRecipe ? bitmaps[idx] : null),
        media: (idx) => { const m = tables.media[idx]; return m ? parts.get(String(m.part).toLowerCase()) ?? null : null; },
        env: { fontStack },
      });
      const lines = form(r.text, 'draw').filter((l) => !l.startsWith('@cv'));
      if (r.error) lines.push(`#throw ${r.error}`);
      calls.push(['#call', ...lines]);
      stats.lines += lines.length;
    }
    const text = calls.map((c) => c.join('\n')).join('\n');
    stats.frames++;
    if (dig(text) === want) { stats.equal++; continue; }
    stats.different++;
    if (dump) writeFileSync(join(dump, `${key.replace(/[\/]/g, '_')}-d${vi}@${fr.state}.rust.txt`), text);
    if (shown++ < show) {
      const ts = tsLines(key, vi, fr.state);
      const rs = text.split('\n');
      let i = 0;
      if (ts) while (i < ts.length && i < rs.length && ts[i] === rs[i]) i++;
      console.log(`DIFFERS  ${key} d${vi}@${fr.state} (calls ${calls.length}/${facets.get(`dc${vi}@${fr.state}`)})` +
        (ts ? `: first difference at line ${i} of ${ts.length} (TS) / ${rs.length} (Rust)\n  TS:   ${ts[i]}\n  Rust: ${rs[i]}` : ''));
    }
  }
}
console.log(`drawings: ${stats.workbooks} workbooks, ${stats.frames} frames compared (${stats.lines} draw lines), ${stats.equal} equal, ${stats.different} different, ` +
  `${stats.missing} missing; ${((performance.now() - t0) / 1000).toFixed(1)} s`);
process.exit(stats.different || stats.missing ? 1 : 0);
