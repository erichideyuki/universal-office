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
//   - accepted by name as the native replays: step 0's `ev` (skipped), the list-rule hook of F1/F6/F7,
//     F2's "throws after the step", F3's `thr` (the TS threw: the document's script ends there).
//   - F3 uses the step record's `lay` (what the TS read from the layout): it goes to the module with the code
//     (`<op>, a tab, <lay>`), and its facets (`tbl`, `sel` = the body's selection, `ctx`, `ts`) come from
//     `uo_session_test_facets_f3`.
// F5, F9, F10, F8 and F7 have their own facet tables (`uo_session_test_facets_fam`: copies of the native replays' functions, in
// src/session/selftest/{fam,f10,f8,f7}.rs, declared):
//   - F5 (notes): ctx, no, np, ref, nts, sect, csel, parts; F9 (find, navigation): res (incl. the direct `cm`), cursel, ext, ctx,
//     hd; the thrown step ends the script, the F9-A2 stale-search-result rule accepts the rest of the document by name;
//   - F10 (references): the handle's scripted layout oracle (`uo_session_test_f10_*`: what the TS layout answered, `i-fv` /
//     `i-pg` / `i-mh`) and HOW the session asked it (`fvs`, `pgc`, `pgv`, `mhc`); the DIVERGENCES by name (F10-A3) and F10-A4;
//   - F8 (clipboard): groups of documents with one Office Clipboard each (`uo_session_test_f8_group`), the carried paste
//     defaults / path / policy (exec actions of the pseudo-code `@setup`), the shell's answers to `readClipboard` / `renderClip`
//     from the capture (`cbt`, `pt`, `fx`), the session's random calls (`rc`, counted by the self-test build only) and the
//     acceptances F8-D14 and G7/F8-O2 by name. The self-test build has `uo-docx/export` (the HTML of a copy, F8-D5);
//   - F7 (drawing objects, pictures): the facts the commands take (`sf`, `sfs`, `fr`, `eo`) come from the step's `fx` (module side),
//     the image jobs (`decodeImage` from `gid`/`bmp`, `encodeImage` answered from `blob`, the encode hashes `enc`) are answered
//     here through `complete`; the editor's own `Math.random` uses between the steps are replayed by advancing the session's
//     environment to the capture's `rc` (`uo_session_test_f7_begin`, the test-only draw counter reset before the open); the
//     accepted: F7-D1 (compress `enc`), F7-D6 (the count of a multi-selection), the list-rule hook.
// F4 is NOT replayed in wasm: it needs a wire op for `undoReentering`/`redoReentering` (gate list).
// Other family-specific facets (pend, sv/nv of the other families, rev, cm...) are not compared here: the native replays prove them
// on the same code (gate item).
//   cargo build -p uo-wasm --features session,count,session-selftest --target wasm32-unknown-unknown --profile wasm
//   UO_WASM_PATH=<that .wasm> UO_WS_FAMILIES=d2a,f1,f2,f3,f5,f6,f7,f8,f9,f10 node --test crates/uo-wasm/tests/session_replay.test.mjs
// UO_WS_FAMILIES (default d2a,f1,f2,f3,f5,f6,f7,f8,f9,f10; f4 is not replayed in wasm yet), UO_WS_STRIDE (every n-th document, default 1), UO_WS_DOCS (at most).
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { join } from 'node:path';
import { Rng, readCapture } from '../../uo-testkit/js/testkit.mjs';
import * as codec from '../js/gen/session.ts';
import { loadModule, repo } from './session_support.mjs';

const { has, names: exportsOf, w, uo, ss } = await loadModule();
const skip = has && exportsOf.includes('uo_session_test_next') ? false : 'this build has no session self-test (--features session-selftest)';

const CAPTURES = { d2a: 'd2a-session.capture', f1: 'd2b-f1-session.capture', f2: 'd2b-f2-session.capture', f3: 'd2b-f3-session.capture', f5: 'd2b-f5-session.capture', f7: 'd2b-f7-session.capture', f8: 'd2b-f8-session.capture', f10: 'd2b-f10-session.capture', f6: 'd2b-f6-session.capture', f9: 'd2b-f9-session.capture' };
const FAMILIES = (process.env.UO_WS_FAMILIES ?? 'd2a,f1,f2,f3,f5,f6,f7,f8,f9,f10').split(',').filter(Boolean);
const STRIDE = Number(process.env.UO_WS_STRIDE ?? 1);
const MAXDOCS = Number(process.env.UO_WS_DOCS ?? Infinity);

const enc = new TextEncoder();
const dec = new TextDecoder();
const put = (b) => (b.length === 0 ? [0, 0] : [uo.render.put(b), b.length]);
const take = (ptr) => new uo.render.Buf(ptr).take();

// The documents open through the PRODUCTION `uo_session_open` with a `SessionOpen` message whose author, seed and
// clock equal `FixedEnv::default()` (the native replays' environment); the family's own setup (F2's one draw of the
// harness `Math.random()`, its Portuguese interface language) is the only difference.
const T = { year: 2026, month: 1, day: 2, hour: 3, minute: 4 };
// F2 and F10 draw one `Math.random()` at the element's construction (F10's `refSeed`): the session's source starts one value on
const drawsOne = (family) => family === 'f2' || family === 'f10';
const openProduction = (family, doc) => {
  const seed = drawsOne(family) ? new Rng(1).next() : 1;
  const s = ss.open(doc, { protected: false, author: 'A', seed, clock: { local: T, utc: T }, sessionId: 0 });
  if (drawsOne(family)) s.setUiLang('pt');
  return s;
};
// F10: the scripted layout oracle (what the TS layout answered in the step, `i-fv`/`i-pg`/`i-mh`) is the handle's test oracle
const f10Setup = (s, refSeed) => {
  const { status } = take(w.uo_session_test_f10_setup(s.ptr, refSeed));
  assert.equal(status, 0);
};
const f10Begin = (s, fv, pg, mh) => {
  const { status } = take(w.uo_session_test_f10_begin(s.ptr, ...put(enc.encode(`${fv ?? ''}\t${pg ?? ''}\t${mh ?? ''}`))));
  assert.equal(status, 0);
};
const f10Record = (s) => {
  const { status, data } = take(w.uo_session_test_f10_record(s.ptr));
  assert.equal(status, 0);
  const [fvs, pgCalls, pgReq, mhCalls, fvMissing] = dec.decode(data).split('\t');
  return { fvs, pgCalls: Number(pgCalls), pgReq: new Set(pgReq ? pgReq.split(',').map(Number) : []), mhCalls: Number(mhCalls), fvMissing: Number(fvMissing) };
};
// known divergences of the F10 capture from the session: from `step` on the document's facets are accepted by name
const F10_DIVERGENCES = [['fsep-top716.docx', 30]];
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
// families with their own facet table (f5, f9): `name=value` pairs, values never hold a tab
const FAM = new Set(['f5', 'f9', 'f10', 'f8']);
const testFacetsFam = (s, family, code, want) => {
  const { status, data } = take(w.uo_session_test_facets_fam(s.ptr, ...put(enc.encode(family)), ...put(enc.encode(code)), want));
  assert.equal(status, 0);
  const out = {};
  for (const kv of dec.decode(data).split('\t')) { const i = kv.indexOf('='); out[kv.slice(0, i)] = kv.slice(i + 1); }
  return out;
};
const testFacets = (s, docxml, family) => {
  if (family === 'f3') {
    const { status, data } = take(w.uo_session_test_facets_f3(s.ptr, docxml ? 1 : 0));
    assert.equal(status, 0);
    const [paras, anc, xml, tbl, sel, ctx, ts] = dec.decode(data).split('\t');
    return { paras, anc, docxml: xml, tbl, sel, ctx, ts };
  }
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

/** `drive` of the native F8 replay: the clipboard read answered from the capture (`cbt`, `pt`), the PNGs in order (`fx`) */
function driveJobs(s, out, shell) {
  for (let n = 0; n < 32; n++) {
    if (out.t !== 'jobs' || out.jobs.length === 0) return out;
    let last = out;
    for (const j of out.jobs) {
      let r;
      if (j.t === 'readClipboard') r = { t: 'clipboard', text: shell.text, ...(shell.tree ? { htmlTree: shell.tree } : {}) };
      else if (j.t === 'renderClip') {
        const f = shell.fx.length ? shell.fx.shift() : null;
        r = f ? { t: 'rendered', png: f.png, widthTwips: f.w, heightTwips: f.h } : { t: 'failed' };
      } else r = { t: 'failed' };
      last = s.complete(j.id, r);
    }
    out = last;
  }
  s.exec({ t: 'cancelPending' });
  return out;
}

/** F7: the image jobs answered from the step's fact log (`bmp` sizes, `gid` pixel reads, `blob` encoder outputs); `encs` collects the encode hashes */
const fnv = (px) => {
  let h = 0x811c9dc5;
  for (const b of px) { h ^= b; h = Math.imul(h, 0x01000193) >>> 0; }
  return h.toString(16);
};
function driveJobsF7(s, out, shell) {
  for (let n = 0; n < 8; n++) {
    if (out.t !== 'jobs' || out.jobs.length === 0) return out;
    let last = out;
    for (const j of out.jobs) {
      let r;
      if (j.t === 'decodeImage') {
        const g = shell.gid.shift();
        if (g) r = { t: 'image', width: g.w, height: g.h, rgba: g.px };
        else if (shell.bmp.length) { const b = shell.bmp.shift(); r = { t: 'image', width: b.w, height: b.h }; } else r = { t: 'failed' };
      } else if (j.t === 'encodeImage') {
        shell.encs.push(`${j.width}x${j.height}:${fnv(j.rgba)}`);
        const b = shell.blob.shift();
        r = b ? { t: 'bytes', v: b } : { t: 'failed' };
      } else r = { t: 'failed' };
      last = s.complete(j.id, r);
    }
    out = last;
  }
  throw new Error('too many rounds of jobs');
}

/** runs one action of the module through the TS codec and the exports; the change set when it is an exec */
function run(s, data, counts, shell = null) {
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
  // F8 / F7: the shell answers the jobs (the clipboard read, the PNGs of the clips; the image decodes and encodes) until the command is done
  if (out?.t === 'jobs' && shell) out = shell.f7 ? driveJobsF7(s, out, shell) : driveJobs(s, out, shell);
  // a struct payload is flattened into the tagged object: `{ t: 'done', stories, selection, events, ... }`
  if (out?.t === 'done') return out;
  if (out?.t === 'error') { counts.errors++; return { events: [], error: out.v }; }
  throw new Error(`unexpected outcome ${JSON.stringify(out)}`);
}

// ── F8 (the clipboard): groups of documents on one element (one Office Clipboard per group), the shell's answers from the capture ──
const F8_EV = new Set(['uw-change', 'uw-dirty-change', 'uw-input', 'uw-track-changes', 'uw-cellmode', 'uw-notemode', 'uw-hfmode', 'uw-tbmode', 'uw-protected', 'uw-styles-change', 'uw-pasted']);
const f8EvText = (evs) => {
  const e = evs.filter((x) => F8_EV.has(x.t)).map((x) => (['uw-dirty-change'].includes(x.t) ? `${x.t}:${x.dirty}` : ['uw-track-changes'].includes(x.t) ? `${x.t}:${x.on}` : ['uw-cellmode', 'uw-notemode', 'uw-hfmode', 'uw-tbmode', 'uw-protected'].includes(x.t) ? `${x.t}:${x.active}` : `${x.t}:`));
  return e.length ? e.join(' ') : '-';
};
const f8Fx = (b64) => {
  if (b64 === undefined) return [];
  return JSON.parse(Buffer.from(b64, 'base64').toString('utf8')).map((f) => (f ? { png: new Uint8Array(Buffer.from(f.b64, 'base64')), w: Number(f.w), h: Number(f.h) } : null));
};
const f8Units = (hex) => String.fromCharCode(...hex.split('.').filter(Boolean).map((h) => parseInt(h, 16)));

function replayF8() {
  const cap = readCapture(join(repo, 'core/crates/uo-docx/tests/parity', CAPTURES.f8));
  const steps = Number(cap.metas.get('steps'));
  const counts = { docs: 0, equal: 0, accepted: 0, thrown: 0, mismatches: 0, reencoded: 0, reencodeDiff: 0, hot: 0, exec: 0, errors: 0, actions: 0 };
  const first = [];
  let group = '';
  let inGroup = 0;
  let di = 0;
  for (const [k0, rec0] of cap.records) {
    if (!k0.endsWith('#rs')) continue;
    const idx = di++;
    if (idx % STRIDE !== 0 || counts.docs >= MAXDOCS) continue;
    const rel = k0.slice(0, -3);
    const g0 = (f) => rec0.get(f)?.replace(/\r$/, '');
    // a group starts over: the Office Clipboard, the paste defaults, the document path and the policy (STRIDE > 1 keeps the first document's group)
    if (g0('g') !== group) { group = g0('g'); w.uo_session_test_f8_group(); inGroup = 0; } else inGroup++;
    const ext = g0('x') !== undefined;
    const syn = g0('doc') !== undefined;
    const bytes = syn ? new Uint8Array(Buffer.from(g0('doc'), 'base64')) : readFileSync(join(repo, rel));
    const freshCounters = inGroup === 0 && !syn;
    // the page's random state before the load when the load drew from it (`rs0`), else after it
    const seed = Number(g0('rs0') ?? g0('rs'));
    const s = ss.open(bytes, { protected: false, author: 'A', seed, clock: { local: T, utc: T }, sessionId: idx + 1 });
    s.setUiLang('pt');
    assert.equal(take(w.uo_session_test_f8_open(s.ptr, ext ? 1 : 0)).status, 0);
    // the carried state of the group goes in as exec actions of the pseudo-code `@setup` (through the codec like every action)
    for (let k = 0; ; k++) {
      const data = testNext(s, 'f8', '@setup', k);
      if (!data) break;
      counts.actions++;
      run(s, data, counts, null);
    }
    let acceptFrom = false;
    let docTainted = false;
    for (let j = 0; j <= steps; j++) {
      const key = `${rel}#${j}`;
      const rec = cap.records.get(key);
      if (!rec) break;
      const get = (f) => rec.get(f)?.replace(/\r$/, '');
      const code = get('op');
      if (get('thrown') !== undefined) { counts.thrown++; break; }
      const treeB64 = get('pt');
      const tree = treeB64 !== undefined ? new Uint8Array(Buffer.from(treeB64, 'base64')) : null;
      const cbt = get('cbt');
      const shell = { text: cbt !== undefined ? f8Units(cbt) : '', tree, fx: f8Fx(get('fx')) };
      assert.equal(take(w.uo_session_test_f8_step(s.ptr, ...put(enc.encode(cbt ?? '')), ...put(tree ?? new Uint8Array(0)), tree ? 1 : 0)).status, 0);
      const evs = [];
      if (j > 0) {
        const arg = get('hf') !== undefined ? `${code}\t${get('hf')}` : code;
        for (let k = 0; ; k++) {
          const data = testNext(s, 'f8', arg, k);
          if (!data) break;
          counts.actions++;
          const r = run(s, data, counts, shell);
          if (r?.events) evs.push(...r.events);
        }
      }
      const actual = { ev: j === 0 ? '-' : f8EvText(evs) };
      actual.flags = `${s.query({ t: 'isDirty' }).v ? 1 : 0}${s.query({ t: 'canUndo' }).v ? 1 : 0}${s.query({ t: 'canRedo' }).v ? 1 : 0}:${s.query({ t: 'editsSinceLoad' }).v}`;
      const fam = testFacetsFam(s, 'f8', arg0(code, get), get('docxml') !== undefined ? 1 : 0);
      if (j === 0) delete fam.rc;
      const pasteTaint = fam._paste_taint === '1';
      const curTaint = fam._cur_taint === '1';
      delete fam._paste_taint; delete fam._cur_taint;
      // G7/F8-O2: the TS element's fresh docPr ids and image relationship numbers of the save (module counters) carry across the
      // documents of a page: only a later document of a page is accepted, by the masked digest
      if (fam.docxml !== undefined) {
        const masked = fam.docxmlm; delete fam.docxmlm;
        if (!freshCounters && get('docxml') !== fam.docxml && get('docxmlm') === masked) { counts.accepted++; delete fam.docxml; }
      }
      Object.assign(actual, fam);
      if (pasteTaint) docTainted = true;
      const tainted = docTainted || curTaint;
      for (const [facet, got] of Object.entries(actual)) {
        const want = get(facet);
        if (want === undefined) continue;
        const differs = want !== got;
        // F8-D14: the TS copies tables by JSON, so a picture in a copied table loses its bytes; only the facets those bytes reach are
        // accepted (the content, its save, what the copy wrote, the clip), every other facet is still compared
        const d14 = ['cbw', 'paras', 'docxml', 'clip'].includes(facet);
        if (differs && !acceptFrom && tainted && d14) acceptFrom = true;
        if (!differs) counts.equal++;
        else if (acceptFrom && d14) counts.accepted++;
        else {
          counts.mismatches++;
          if (first.length < 12) first.push(`${key} ${code} ${facet}: want ${want.slice(0, 120)} got ${got.slice(0, 120)}`);
        }
      }
    }
    w.uo_session_test_forget(s.ptr);
    s.free();
    counts.docs++;
  }
  return { counts, first };
}
const arg0 = (code, get) => (get('hf') !== undefined ? `${code}\t${get('hf')}` : code);

// ── F7 (drawing objects, text boxes, pictures): the facts and the image jobs from the step's `fx`, the random count advanced to `rc` ──
const f7Jobs = (t) => {
  const o = { gid: [], bmp: [], blob: [], encs: [], f7: true };
  if (t === '-') return o;
  for (const e of t.split('|')) {
    const f = e.split(';');
    if (f[0] === 'gid') o.gid.push({ w: Number(f[1]), h: Number(f[2]), px: new Uint8Array(Buffer.from(f[3], 'base64')) });
    else if (f[0] === 'bmp') o.bmp.push({ w: Number(f[1]), h: Number(f[2]) });
    else if (f[0] === 'blob') o.blob.push(new Uint8Array(Buffer.from(f[3], 'base64')));
  }
  return o;
};
// the facets the native replay checks under the family's list rule (the others are checked always)
const F7_GUARDED = new Set(['paras', 'sel', 'anc', 'flags', 'fs', 'tb', 'pane', 'st', 'ev', 'docxml']);

function replayF7() {
  const cap = readCapture(join(repo, 'core/crates/uo-docx/tests/parity', CAPTURES.f7));
  const steps = Number(cap.metas.get('steps'));
  const counts = { docs: 0, equal: 0, accepted: 0, thrown: 0, mismatches: 0, reencoded: 0, reencodeDiff: 0, hot: 0, exec: 0, errors: 0, actions: 0, steps: 0, refused: 0 };
  const first = [];
  for (let k = 0; k < 16; k++) {
    const r = cap.records.get(`img#${k}`);
    if (!r) break;
    const ext = r.get('ext').replace(/\r$/, '');
    const bytes = new Uint8Array(Buffer.from(r.get('b64').replace(/\r$/, ''), 'base64'));
    w.uo_session_test_f7_image(k, ...put(enc.encode(ext)), ...put(bytes));
  }
  let di = 0;
  for (const k0 of [...cap.records.keys()].filter((k) => k.endsWith('#0') && !k.startsWith('img#'))) {
    if (di++ % STRIDE !== 0 || counts.docs >= MAXDOCS) continue;
    const rel = k0.slice(0, -2);
    w.uo_session_test_f7_reset();
    const s = openProduction('f7', readFileSync(join(repo, rel)));
    for (let j = 0; j <= steps; j++) {
      const key = `${rel}#${j}`;
      const rec = cap.records.get(key);
      if (!rec) break;
      const get = (f) => rec.get(f)?.replace(/\r$/, '');
      const code = get('op');
      // the editor's own uses of Math.random between the steps
      const rcT = j === 0 ? get('rc') : cap.records.get(`${rel}#${j - 1}`)?.get('rc')?.replace(/\r$/, '');
      const fxText = j === 0 ? '-' : (get('fx') ?? '-');
      assert.equal(take(w.uo_session_test_f7_begin(s.ptr, ...put(enc.encode(fxText)), rcT !== undefined ? 1 : 0, rcT !== undefined ? Number(rcT) : 0)).status, 0);
      // the list rule of D2b-F2 covers only the step it applies to
      const pending = testHook(s, 'f7', code);
      let failed = false;
      let encsText = '-';
      const evs = [];
      if (j > 0) {
        counts.steps++;
        const shell = f7Jobs(fxText);
        if (code.startsWith('ins:') || code.startsWith('chg:')) shell.gid.length = 0;
        for (let k = 0; ; k++) {
          const data = testNext(s, 'f7', code, k);
          if (!data) break;
          counts.actions++;
          const r = run(s, data, counts, shell);
          if (r.error !== undefined) {
            // a refusal the TS did not have is a mismatch (the facets show it); the second half of iw/cdt is not run
            if (k === 0) { failed = true; counts.refused++; }
            s.exec({ t: 'cancelPending' });
            break;
          }
          evs.push(...r.events);
        }
        encsText = shell.encs.length ? shell.encs.join(',') : '-';
      }
      const actual = {};
      if (j > 0 && !failed) actual.ev = evText({ events: evs });
      Object.assign(actual, testFacetsFam(s, 'f7', code, get('docxml') !== undefined ? 1 : 0));
      const mu = Number(get('mu') ?? 0);
      for (const [facet, got] of Object.entries(actual)) {
        const want = get(facet);
        if (want === undefined) continue;
        const guarded = F7_GUARDED.has(facet);
        if (got === want) counts.equal++;
        else if (guarded && pending && j > 0) counts.accepted++;
        // F7-D6: selectedObjectCount counts the handles enumFloats finds on the laid-out pages (a multi-selection)
        else if (facet === 'cnt' && mu > 0 && j > 0) counts.accepted++;
        else {
          counts.mismatches++;
          if (first.length < 12) first.push(`${key} ${code} ${facet}: want ${want.slice(0, 120)} got ${got.slice(0, 120)}`);
        }
      }
      // the encoder hashes: compressImages is accepted when it differs (F7-D1: the TS resamples with drawImage, the Rust with an area
      // average), adjustImage is compared
      if (j > 0 && get('enc') !== undefined) {
        // compare first, accept only a difference (as the native `Replay::accept`)
        if (code.startsWith('cmp:')) { if (encsText === get('enc')) counts.equal++; else counts.accepted++; }
        else if (code.startsWith('adj:')) {
          if (encsText === get('enc')) counts.equal++;
          else { counts.mismatches++; if (first.length < 12) first.push(`${key} ${code} enc: want ${get('enc').slice(0, 120)} got ${encsText.slice(0, 120)}`); }
        }
      }
    }
    w.uo_session_test_forget(s.ptr);
    s.free();
    counts.docs++;
  }
  return { counts, first };
}

function replayFamily(family) {
  if (family === 'f8') return replayF8();
  if (family === 'f7') return replayF7();
  const cap = readCapture(join(repo, 'core/crates/uo-docx/tests/parity', CAPTURES[family]));
  const steps = Number(cap.metas.get('steps'));
  const counts = { docs: 0, equal: 0, accepted: 0, thrown: 0, mismatches: 0, reencoded: 0, reencodeDiff: 0, hot: 0, exec: 0, errors: 0, actions: 0 };
  const first = [];
  const keys0 = [...cap.records.keys()].filter((k) => k.endsWith('#0'));
  let di = 0;
  for (const k0 of keys0) {
    if (di++ % STRIDE !== 0 || counts.docs >= MAXDOCS) continue;
    const rel = k0.slice(0, -2);
    const s = openProduction(family, readFileSync(join(repo, rel)));
    const rec0 = cap.records.get(k0);
    if (family === 'f10') f10Setup(s, Number(rec0.get('i-seed').replace(/\r$/, '')));
    // wave 7c (S): the F5 and F10 captures save after an undone style edit or note options set back to the loaded ones; the
    // frozen TS leaves the earlier save's part (Word saves the current state: the default, `save_parts.rs`), as the native
    // replays open with `ts_stale_parts` (fix round 1: that flag only; the steps it covers are listed in uo-bindings.md,
    // "Wave 7c — S, fix round 1")
    if ((family === 'f5' || family === 'f10') && process.env.UO_WS_TS_SAVE_FLAGS !== '0') assert.equal(take(w.uo_session_test_ts_save_compat(s.ptr, 1)).status, 0);
    const diverges = family === 'f10' ? (F10_DIVERGENCES.find(([d]) => rel.endsWith(d))?.[1] ?? Infinity) : Infinity;
    let pending = null;
    let stale = false;
    let acceptedFrom = null;
    for (let j = 0; j <= steps; j++) {
      const key = `${rel}#${j}`;
      const rec = cap.records.get(key);
      if (!rec) break;
      const get = (f) => rec.get(f)?.replace(/\r$/, '');
      const code = get('op');
      // F3: the TS threw on this step (a table the editor cannot handle): the document ends here (accepted by name)
      if (family === 'f3' && get('thr') !== undefined) { counts.accepted++; break; }
      // F9: the TS threw on this step (a stale position, a TS bug): the script of the document ends there (declared in the capture)
      if ((family === 'f9' || family === 'f10') && get('thrown') !== undefined) { counts.thrown++; break; }
      // F3: the step record's `lay` (what the TS read from the layout) goes with the code
      const arg = family === 'f3' && get('lay') !== undefined ? `${code}\t${get('lay')}` : code;
      if (!pending) pending = family === 'f1' || family === 'f5' || family === 'f6' ? testHook(s, family, code) : null;
      let cs = null;
      let hadSel = false;
      if (j > 0 && family === 'f10') {
        const sel0 = s.query({ t: 'selection' });
        hadSel = sel0.anchor.para !== sel0.focus.para || sel0.anchor.offset !== sel0.focus.offset;
        f10Begin(s, get('i-fv') === '-' ? '' : get('i-fv'), get('i-pg') === '-' ? '' : get('i-pg'), get('i-mh') === '-' ? '' : get('i-mh'));
      }
      if (j > 0) {
        for (let k = 0; ; k++) {
          const data = testNext(s, family, arg, k);
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
      let fam = null;
      if (FAM.has(family)) {
        fam = testFacetsFam(s, family, code, (get('docxml') !== undefined ? 1 : 0) | (get('dtf') !== undefined ? 2 : 0));
        if (family === 'f9' && j > 0) {
          // F9-A2: `goToSearchResult` of a stale result leaves the TS selection outside the story; the session's undo/redo
          // clamps a restored selection (D2a-C4) where the TS keeps it: from there the document is accepted by name
          if (stale && acceptedFrom === null && (code === 'un' || code === 're')) acceptedFrom = j;
          if (code.startsWith('nr:') && fam._oos === '1') stale = true;
        }
        delete fam._oos;
        Object.assign(actual, fam);
      }
      // F10: HOW the session asked the oracle (`fvs`, `pgc`, `pgv`, `mhc`), against what the TS asked
      const acceptNamed = {};
      if (family === 'f10' && j > 0) {
        const o = f10Record(s);
        const asked = new Set(get('i-pg') && get('i-pg') !== '-' ? get('i-pg').split(',').map((kv) => Number(kv.split(':')[0])) : []);
        const missing = [...asked].filter((a) => !o.pgReq.has(a));
        actual.fvs = o.fvs;
        actual.pgv = missing.length === 0 ? `ok:${asked.size}` : `missing:[${missing.join(', ')}] of ${get('pgv')}`;
        const pgc = o.pgCalls === 0 ? '0' : o.pgCalls === 1 ? '1' : 'many';
        actual.pgc = pgc;
        actual.mhc = get('mhc') === '1' ? (o.mhCalls === 1 ? '1' : `calls=${o.mhCalls}`) : (o.mhCalls <= 1 ? 'x' : `calls=${o.mhCalls}`);
        if (o.fvMissing !== 0) throw new Error(`${key} ${code}: a value of the TS names a run the model does not have`);
        // F10-A4: with a selection the command deletes it before it reads the sources; the session asks the page labels first
        if (pgc === '1' && get('pgc') === '0' && hadSel && ['tt', 'tc', 'tf'].includes(code.split(':')[0])) acceptNamed.pgc = true;
      }
      const f = fam ?? testFacets(s, get('docxml') !== undefined, family);
      if (family === 'f3') { actual.sel = f.sel; actual.tbl = f.tbl; actual.ctx = f.ctx; actual.ts = f.ts; }
      actual.paras = f.paras;
      actual.anc = f.anc;
      if (get('docxml') !== undefined) actual.docxml = f.docxml;
      // F9: `ev` is "-" for the direct `cm` op and for steps that gave no change set
      for (const [facet, got] of Object.entries(actual)) {
        const want = get(facet);
        if (want === undefined) continue;
        const acceptAll = (pending && j > 0) || acceptedFrom !== null || acceptNamed[facet] === true || j >= diverges;
        const throwAccept = family === 'f2' && facet === 'ev' && want.includes(' THROW:') && want.split(' THROW:')[0] === got;
        if (got === want) counts.equal++;
        else if (acceptAll || throwAccept) counts.accepted++;
        else {
          counts.mismatches++;
          // UO_WS_FULL=1 (wave 7c S, fix round 1): every mismatch, with the comma-separated items only one side has
          if (process.env.UO_WS_FULL === '1') {
            const w = want.split(','), g = got.split(',');
            first.push(`${key} ${facet}: want-only ${w.filter((x) => !g.includes(x)).join(',')} | got-only ${g.filter((x) => !w.includes(x)).join(',')}`);
          } else if (first.length < 12) first.push(`${key} ${facet}: want ${want.slice(0, 120)} got ${got.slice(0, 120)}`);
        }
      }
    }
    w.uo_session_test_forget(s.ptr);
    s.free();
    counts.docs++;
  }
  return { counts, first };
}

for (const family of FAMILIES) {
  test(`replay ${family} through the wasm exports`, { skip }, () => {
    const t0 = performance.now();
    const { counts, first } = replayFamily(family);
    const ms = Math.round(performance.now() - t0);
    console.log(`${family}: ${counts.docs} documents, ${counts.equal} facets equal, ${counts.accepted} accepted, ${counts.thrown} steps where the TS threw (script ends), ${counts.mismatches} mismatches; ${counts.actions} actions (${counts.hot} hot path, ${counts.exec} exec, ${counts.errors} command errors), TS re-encode ${counts.reencoded} byte-identical / ${counts.reencodeDiff} different${counts.steps !== undefined ? `; ${counts.steps} steps run, ${counts.refused} refused by the session` : ''}; ${ms} ms`);
    for (const l of first) console.log(`  ${l}`);
    assert.equal(counts.reencodeDiff, 0);
    assert.equal(counts.mismatches, 0);
    assert.ok(counts.docs > 0);
  });
}
