// Verification (a) of the session in wasm (record uo-bindings.md "Milestone 2: the session in wasm"): the
// committed D2a/D2b session captures of uo-docx (tests/parity/*-session.capture, the frozen element in
// Chromium) replayed through the module's exports in Node, the same facets as the native replays:
//   - every action of every op code comes from the module (`uo_session_test_next`, the native replay's
//     mapping), is decoded by the generated TS codec and re-encoded (byte-identical, counted) and runs
//     through the hot path for the text commands (type / enter / backspace / delete / setSelection) and
//     through `exec` / `query` otherwise;
//   - `ev`, `sel` and `flags` are built from the TS-decoded results (the change set, `EditsSinceLoad`);
//     `paras`, `anc` and `docxml` (the canonical digests the wire does not carry) from
//     `uo_session_test_facets`;
//   - accepted by name as the native replays: step 0's `ev` (skipped), the list-rule hook of F1/F6,
//     F2's "throws after the step".
// Family-specific facets (pend, sv/nv, rev, cm...) are not compared here: the native replays prove them
// on the same code (gate item).
//   cargo build -p uo-wasm --features session,count,session-selftest --target wasm32-unknown-unknown --profile wasm
//   UO_WASM_PATH=<that .wasm> UO_WS_FAMILIES=d2a,f1,f2,f6 node --test crates/uo-wasm/tests/session_replay.test.mjs
// UO_WS_FAMILIES (default d2a,f1,f2,f6), UO_WS_STRIDE (every n-th document, default 1), UO_WS_DOCS (at most).
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { join, dirname } from 'node:path';
import { fileURLToPath } from 'node:url';
import { writeImports, bindWrite } from '../js/write.mjs';
import { bindSession } from '../js/session.mjs';
import { readCapture } from '../../uo-testkit/js/testkit.mjs';
import * as codec from '../js/gen/session.ts';

const here = dirname(fileURLToPath(import.meta.url));
const repo = join(here, '../../../..');
const wasmPath = process.env.UO_WASM_PATH ?? join(here, '../../../target/wasm32-unknown-unknown/wasm/uo_wasm.wasm');
const bytes = readFileSync(wasmPath);
const exportsOf = WebAssembly.Module.exports(new WebAssembly.Module(bytes)).map((e) => e.name);
const skip = exportsOf.includes('uo_session_test_next') ? false : 'this build has no session self-test (--features session-selftest)';
const { imports, host } = writeImports();
const instance = skip ? null : (await WebAssembly.instantiate(bytes, imports)).instance;
const w = instance?.exports;
const uo = skip ? null : bindWrite(w, host);
const ss = skip ? null : bindSession(w, uo);

const CAPTURES = { d2a: 'd2a-session.capture', f1: 'd2b-f1-session.capture', f2: 'd2b-f2-session.capture', f6: 'd2b-f6-session.capture' };
const FAMILIES = (process.env.UO_WS_FAMILIES ?? 'd2a,f1,f2,f6').split(',').filter(Boolean);
const STRIDE = Number(process.env.UO_WS_STRIDE ?? 1);
const MAXDOCS = Number(process.env.UO_WS_DOCS ?? Infinity);

const enc = new TextEncoder();
const dec = new TextDecoder();
const put = (b) => (b.length === 0 ? [0, 0] : [uo.render.put(b), b.length]);
const take = (ptr) => new uo.render.Buf(ptr).take();

const testOpen = (family, doc) => w.uo_session_test_open(...put(enc.encode(family)), ...put(doc));
const testHook = (s, family, code) => {
  const { status, data } = take(w.uo_session_test_hook(s.ptr, ...put(enc.encode(family)), ...put(enc.encode(code))));
  return status === 0 ? dec.decode(data) : null;
};
const testNext = (s, family, code, k) => {
  const { status, data } = take(w.uo_session_test_next(s.ptr, ...put(enc.encode(family)), ...put(enc.encode(code)), k));
  if (status === 1) return null;
  assert.equal(status, 0, `next ${code} ${k}: ${dec.decode(data)}`);
  return data;
};
const testFacets = (s, docxml) => {
  const { status, data } = take(w.uo_session_test_facets(s.ptr, docxml ? 1 : 0));
  assert.equal(status, 0);
  const [paras, anc, xml] = dec.decode(data).split('\t');
  return { paras, anc, docxml: xml };
};

const evText = (cs) => {
  const e = (cs?.events ?? []).map((x) => {
    switch (x.t) {
      case 'uw-dirty-change': return `uw-dirty-change:${x.dirty}`;
      case 'uw-track-changes': return `uw-track-changes:${x.on}`;
      case 'uw-cellmode': case 'uw-notemode': case 'uw-hfmode': case 'uw-tbmode': case 'uw-protected': return `${x.t}:${x.active}`;
      default: return `${x.t}:`;
    }
  });
  return e.length ? e.join(' ') : '-';
};
const pos = (p) => `{"para":${p.para},"offset":${p.offset}}`;
const selJson = (sel) => `{"anchor":${pos(sel.anchor)},"focus":${pos(sel.focus)}}`;

/** runs one action of the module through the TS codec and the exports; the change set when it is an exec */
function run(s, data, counts) {
  const kind = String.fromCharCode(data[0]);
  const msg = data.subarray(1);
  if (kind === 'Q') {
    const m = codec.decodeQuery(msg, codec.newTables());
    const again = codec.encodeQuery(m, codec.newEncodeState(0));
    if (Buffer.compare(Buffer.from(again), Buffer.from(msg)) === 0) counts.reencoded++; else counts.reencodeDiff++;
    s.query(m.q);
    return null;
  }
  const m = codec.decodeExec(msg, codec.newTables());
  const again = codec.encodeExec(m, codec.newEncodeState(0));
  if (Buffer.compare(Buffer.from(again), Buffer.from(msg)) === 0) counts.reencoded++; else counts.reencodeDiff++;
  const op = m.op;
  let out;
  if (op.t === 'command' && op.v.t === 'text') {
    const c = op.v.v;
    counts.hot++;
    switch (c.t) {
      case 'type': out = s.type(c.text); break;
      case 'enter': out = s.enter(); break;
      case 'backspace': out = s.backspace(); break;
      case 'delete': out = s.delete(); break;
      case 'setSelection': out = s.setSelection(c.anchor, c.focus); break;
      default: out = s.exec(op);
    }
  } else {
    counts.exec++;
    out = s.exec(op);
  }
  // a struct payload is flattened into the tagged object: `{ t: 'done', stories, selection, events, ... }`
  if (out?.t === 'done') return out;
  if (out?.t === 'error') { counts.errors++; return { events: [], error: out.v }; }
  throw new Error(`unexpected outcome ${JSON.stringify(out)}`);
}

function replayFamily(family) {
  const cap = readCapture(join(repo, 'core/crates/uo-docx/tests/parity', CAPTURES[family]));
  const steps = Number(cap.metas.get('steps'));
  const counts = { docs: 0, equal: 0, accepted: 0, mismatches: 0, reencoded: 0, reencodeDiff: 0, hot: 0, exec: 0, errors: 0, actions: 0 };
  const first = [];
  const keys0 = [...cap.records.keys()].filter((k) => k.endsWith('#0'));
  let di = 0;
  for (const k0 of keys0) {
    if (di++ % STRIDE !== 0 || counts.docs >= MAXDOCS) continue;
    const rel = k0.slice(0, -2);
    const p = testOpen(family, readFileSync(join(repo, rel)));
    assert.ok(p, `open ${rel}`);
    const s = new ss.WriteSession(p);
    let pending = null;
    for (let j = 0; j <= steps; j++) {
      const key = `${rel}#${j}`;
      const rec = cap.records.get(key);
      if (!rec) break;
      const get = (f) => rec.get(f)?.replace(/\r$/, '');
      const code = get('op');
      if (!pending) pending = family === 'f1' || family === 'f6' ? testHook(s, family, code) : null;
      let cs = null;
      if (j > 0) {
        for (let k = 0; ; k++) {
          const data = testNext(s, family, code, k);
          if (!data) break;
          counts.actions++;
          const r = run(s, data, counts);
          if (r) cs = r;
        }
      }
      const actual = {};
      if (j > 0) actual.ev = evText(cs);
      let flagsBase;
      if (cs && cs.selection) {
        actual.sel = selJson(cs.selection);
        flagsBase = `${cs.dirty ? 1 : 0}${cs.canUndo ? 1 : 0}${cs.canRedo ? 1 : 0}`;
      } else {
        actual.sel = selJson(s.query({ t: 'selection' }));
        flagsBase = `${s.query({ t: 'isDirty' }).v ? 1 : 0}${s.query({ t: 'canUndo' }).v ? 1 : 0}${s.query({ t: 'canRedo' }).v ? 1 : 0}`;
      }
      actual.flags = `${flagsBase}:${s.query({ t: 'editsSinceLoad' }).v}`;
      const f = testFacets(s, get('docxml') !== undefined);
      actual.paras = f.paras;
      actual.anc = f.anc;
      if (get('docxml') !== undefined) actual.docxml = f.docxml;
      for (const [facet, got] of Object.entries(actual)) {
        const want = get(facet);
        if (want === undefined) continue;
        const acceptAll = pending && j > 0;
        const throwAccept = family === 'f2' && facet === 'ev' && want.includes(' THROW:') && want.split(' THROW:')[0] === got;
        if (got === want) counts.equal++;
        else if (acceptAll || throwAccept) counts.accepted++;
        else {
          counts.mismatches++;
          if (first.length < 12) first.push(`${key} ${facet}: want ${want.slice(0, 120)} got ${got.slice(0, 120)}`);
        }
      }
    }
    s.free();
    w.uo_session_test_forget(p);
    counts.docs++;
  }
  return { counts, first };
}

for (const family of FAMILIES) {
  test(`replay ${family} through the wasm exports`, { skip }, () => {
    const t0 = performance.now();
    const { counts, first } = replayFamily(family);
    const ms = Math.round(performance.now() - t0);
    console.log(`${family}: ${counts.docs} documents, ${counts.equal} facets equal, ${counts.accepted} accepted, ${counts.mismatches} mismatches; ${counts.actions} actions (${counts.hot} hot path, ${counts.exec} exec, ${counts.errors} command errors), TS re-encode ${counts.reencoded} byte-identical / ${counts.reencodeDiff} different; ${ms} ms`);
    for (const l of first) console.log(`  ${l}`);
    assert.equal(counts.reencodeDiff, 0);
    assert.equal(counts.mismatches, 0);
    assert.ok(counts.docs > 0);
  });
}
