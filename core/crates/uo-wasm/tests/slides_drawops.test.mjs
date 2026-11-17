// The op-stream gate of B4 through the wasm module (Node only: it reads the gzip captures and runs the recording context): every
// (slide, view) of the TS draw-op captures (`drawops-b1a`, `drawops-samples`) is emitted by uo-slides.wasm (answering the layout's
// requests with the capture's deterministic stub), decoded, painted by `paint-slides.ts` over the recording context and compared
// line by line with the frozen TS stream: the same numbers as the native compare (`compare-drawops.mjs`), now through the module.
//
//   cargo build -p uo-wasm --features count,slides --target wasm32-unknown-unknown --profile wasm
//   UO_WASM_PATH=<that .wasm> [UO_SLIDES_DECKS=24] [UO_PPT_WASM_PATH=<uo-slides-ppt.wasm>] node --test crates/uo-wasm/tests/slides_drawops.test.mjs
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { existsSync, readFileSync } from 'node:fs';
import { join, dirname } from 'node:path';
import { gunzipSync } from 'node:zlib';
import { fileURLToPath } from 'node:url';
import { bindSlides, bindSlidesPpt } from '../js/slides.mjs';
import { decodeSlidesList, newTables } from '../js/gen/slides.ts';
import { capturesDir, readSample } from '../../uo-pptx/js/lib/common.mjs';
import { GROUPS, buildGroupDeck, deckFile } from '../../uo-pptx/js/lib/gen/groups.mjs';
import { compare, form, parseOps } from '../../uo-pptx/js/lib/opstream.mjs';
import { recordList } from '../../uo-pptx/js/lib/rust-paint.mjs';
import { emitAll } from './support/slides-stub.mjs';

const here = dirname(fileURLToPath(import.meta.url));
const wasmPath = process.env.UO_WASM_PATH ?? join(here, '../../../target/wasm32-unknown-unknown/wasm/uo_wasm.wasm');
const uo = bindSlides((await WebAssembly.instantiate(readFileSync(wasmPath), {})).instance.exports);
// the lazily loaded `.ppt` converter module (feature slides-ppt): the samples include legacy decks
const pptPath = process.env.UO_PPT_WASM_PATH ?? join(here, '../../../target/b7/uo-slides-ppt.wasm');
const ppt = existsSync(pptPath) ? bindSlidesPpt((await WebAssembly.instantiate(readFileSync(pptPath), {})).instance.exports) : null;
const isCfb = (b) => b[0] === 0xd0 && b[1] === 0xcf && b[2] === 0x11 && b[3] === 0xe0;
const openAny = (bytes) => uo.open(isCfb(bytes) && ppt ? ppt.convert(bytes) : bytes);
const DECKS = Number(process.env.UO_SLIDES_DECKS ?? 24);

function groupBytes(group, i) {
  const f = join(capturesDir, 'decks', group, deckFile(group, i));
  return existsSync(f) ? new Uint8Array(readFileSync(f)) : new Uint8Array(buildGroupDeck(group, i).bytes);
}

/** Every (slide, view) of the capture, a fresh view per slide as the TS capture's fresh renderer. */
function drawopsGate(capture, deckOf, label) {
  test(`the wasm list equals the frozen TS draw-op stream (${label})`, () => {
    const entries = parseOps(gunzipSync(readFileSync(join(capturesDir, capture))).toString('utf8'));
    const byDeck = new Map();
    for (const e of entries) {
      if (!byDeck.has(e.deck)) byDeck.set(e.deck, []);
      byDeck.get(e.deck).push(e);
    }
    let decks = 0, compared = 0, equal = 0, charts = 0;
    const bad = [];
    for (const [key, slides] of byDeck) {
      if (decks >= DECKS) break;
      decks++;
      const deck = openAny(deckOf(key));
      for (const s of slides) {
        if (s.error) continue;
        const view = uo.newView(1);
        const { scale, ox, oy } = Object.fromEntries(Object.entries(s.header).map(([k, v]) => [k, Number(v)]));
        const r = emitAll(uo, deck, view, s.slide, scale, ox, oy);
        assert.equal(r.status, 0, `${key} s${s.slide}.${s.view}: status ${r.status}`);
        const tables = newTables();
        const list = decodeSlidesList(r.message, tables);
        const [cw, ch] = s.header.canvas.split('x').map(Number);
        const rec = recordList({
          list, tables, width: cw, height: ch,
          bitmap: (i) => view.bitmap(tables.bitmaps[i].data),
          media: (i) => { const m = tables.media[i]; return m ? deck.part(m.part) : null; },
        });
        assert.equal(rec.error, undefined, `${key} s${s.slide}.${s.view}: ${rec.error}`);
        if (r.charts.length) { charts++; view.free(); continue; } // frames the list did not paint (none since C-6: charts are list items)
        compared++;
        if (compare(form(s.lines, 'draw'), rec.text, { which: 'draw', eps: 0 }).equal) equal++;
        else bad.push(`${key} s${s.slide}.${s.view}`);
        view.free();
      }
      deck.free();
    }
    console.log(`  ${label}: ${decks} decks, ${compared} views compared, ${equal} equal, ${charts} views with chart frames skipped`);
    assert.deepEqual(bad.slice(0, 5), [], `${bad.length} views differ`);
    assert.ok(compared > 0);
  });
}

drawopsGate('drawops-b1a.ops.gz', (key) => {
  const m = /^gen\/([^/]+)\/(\d+)$/.exec(key);
  if (!m || !GROUPS[m[1]]) throw new Error(`unknown deck key ${key}`);
  return groupBytes(m[1], Number(m[2]));
}, 'b1a');
drawopsGate('drawops-samples.ops.gz', (key) => readSample(key.slice('samples/'.length)), 'samples');
drawopsGate('drawops-c1d.ops.gz', (key) => {
  const m = /^gen\/([^/]+)\/(\d+)$/.exec(key);
  if (!m || !GROUPS[m[1]]) throw new Error(`unknown deck key ${key}`);
  return groupBytes(m[1], Number(m[2]));
}, 'c1d charts'); // C-6: 60 generated decks whose slides hold charts
