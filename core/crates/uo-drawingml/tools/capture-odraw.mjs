// Parity capture of `uo_drawingml::odraw` (OfficeArt, uo-docx plan G11): the frozen TS
// OfficeArt readers of Write (`docx-core/src/doc/officeart.ts`, `bytes.ts`, `preset-map.ts`
// and the drawing load of `reader.ts`), plus the Slides and Sheet copies where the Rust API
// has a flavor for them. Writes tests/captures/odraw.capture, replayed by tests/odraw_parity.rs.
//
//   UO_TS_ROOT=<built checkout of main> node core/crates/uo-drawingml/tools/capture-odraw.mjs
//
// Options: --out <file>. Deterministic (seeded xorshift32 of the testkit, node:zlib at fixed
// levels); no Math function but Math.round and Math.min/max on exact values, so the platform
// does not matter (it is recorded anyway).
//
// Corpus:
//  - doc/<name>: the 11 .doc fixtures of write/packages/docx-core/test/fixtures/doc. The
//    OfficeArtContent at fcDggInfo of the table stream through the real `DocReader`
//    (`drawings`: BStore, main and header shape maps after the masters, masters), every BStore
//    entry decoded as `shapeBlip` does (`blips`, embedded or at foDelay in WordDocument, one
//    document budget), and the PICF walk of the Data stream (`inline`, defined below). `src`
//    gives the table stream, fc and lcb; `streams` the SHA-256 of the streams (Rust reads them
//    with uo_opc::cfb and checks it).
//  - gen/<k>: generated OfficeArtContent streams (`hex`, at `fc`/`lcb` after random padding),
//    with a delay stream (`word`) and a Data stream of PICFs (`data`) and a budget: shapes and
//    groups (nested, deleted, duplicated spids, masters and chains of masters, backgrounds),
//    FOPT/Secondary/Tertiary with booleans, IMsoArrays (LibreOffice sizes, 0xFFF0 cells,
//    overflowing), BLIPs of every type and instance (raster, DIB headers, metafiles stored,
//    zlib, raw deflate, garbage, trailing bytes, cbSize short and long, placeable WMFs),
//    truncated and overflowing records. `drawings` comes from a copy of `loadDrawings` (checked
//    equal to the DocReader on the fixtures).
//  - mut/<base>/<k>: byte edits (`edits=pos:val,...`, relative to fc) and cuts (`cut=lcb`) of
//    the fixture and generated contents (`on=t` table, `w` delay stream, `d` Data stream).
//  - fopt/<k>: blocks of FOPT records read into one table by the three readers: Word `readFopt`
//    (`word`), Slides `readFopt` (`slides`), Sheet `readFoptBlock` (`sheet`).
//  - blip/<k>: one BLIP record decoded by Word `decodeBlip` (`word`, with `budget`), Slides
//    `decodeBlip` (`slides`, not for PICT: Slides fits the frame) and Sheet `BlipStore`
//    (`sheet`, inside a generated DggContainer).
//  - msospt/<spt>: spt 0..=255 and 4095 over the cases case/<k> (`adj` 10 values as i32 or
//    empty, `w`, `h` as f64 bits): Write `presetFor` (`word`), Slides `presetFor` (`slides`),
//    Sheet `presetGeomXml` (`sheet`). The script compares the three copies entry by entry and
//    writes the differences in the metadata (`@preset-diff`).
import { createHash } from 'node:crypto';
import { readFileSync, readdirSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { deflateRawSync, deflateSync } from 'node:zlib';
import { CaptureWriter, Rng, Stream, importFrom, tsRoot } from '../../uo-testkit/js/testkit.mjs';

const here = dirname(fileURLToPath(import.meta.url));
const argv = process.argv.slice(2);
const outIdx = argv.indexOf('--out');
const out = outIdx >= 0 ? argv[outIdx + 1] : join(here, '..', 'tests', 'captures', 'odraw.capture');
const root = tsRoot(argv);
const repo = join(here, '..', '..', '..', '..');

const { DocReader } = await importFrom(root, 'write/packages/docx-core/dist/doc/reader.js');
const { FC } = await importFrom(root, 'write/packages/docx-core/dist/doc/fib.js');
const { ByteReader, OA, children, readRec, recsIn } = await importFrom(root, 'write/packages/docx-core/dist/doc/bytes.js');
const W = await importFrom(root, 'write/packages/docx-core/dist/doc/officeart.js');
const WP = await importFrom(root, 'write/packages/docx-core/dist/doc/preset-map.js');
const S = await importFrom(root, 'slides/packages/slides-core/dist/ppt/officeart.js');
const SR = await importFrom(root, 'slides/packages/slides-core/dist/ppt/records.js');
const SP = await importFrom(root, 'slides/packages/slides-core/dist/ppt/preset-map.js');
const X = await importFrom(root, 'sheet/packages/sheet-core/dist/xls/officeart.js');
const XG = await importFrom(root, 'sheet/packages/sheet-core/dist/xls/odraw-geom.js');

const summary = (s) => { const n = s.items; return `${n}:${s.finish().short}`; };
const hex = (u8) => Buffer.from(u8.buffer, u8.byteOffset, u8.byteLength).toString('hex');
const sha = (u8) => (u8 ? createHash('sha256').update(u8).digest('hex').slice(0, 16) : '-');
const DOC_BUDGET = 512 * 1024 * 1024;

// ───────────────────────── canonical streams (mirrored in tests/odraw_parity.rs) ─────────────────────────

function propsDigest(s, props) {
  s.tag('p').u32(props.size);
  for (const [id, p] of props) {
    s.u32(id).u32(p.id).u32(p.value).bool(p.bid).bool(!!p.complex);
    if (p.complex) s.digestOf(p.complex);
  }
}
function rect(s, r) {
  s.bool(!!r);
  if (r) s.i64(r.l).i64(r.t).i64(r.r).i64(r.b);
}
function shapeDigest(s, sh) {
  s.tag('S').u32(sh.spid).u32(sh.type).u32(sh.flags).bool(sh.z !== undefined);
  if (sh.z !== undefined) s.u32(sh.z);
  propsDigest(s, sh.props);
  rect(s, sh.groupRect);
  rect(s, sh.childAnchor);
  s.bool(!!sh.children);
  if (sh.children) { s.u32(sh.children.length); for (const c of sh.children) shapeDigest(s, c); }
}
function recDigest(s, r) {
  s.bool(!!r);
  if (r) s.u32(r.type).u32(r.inst).u32(r.ver).u32(r.len).u64(r.hdr).u64(r.start).u64(r.end);
}
function bseDigest(s, e) {
  s.bool(!!e);
  if (e) { s.u32(e.btWin32).u32(e.size).u32(e.foDelay); recDigest(s, e.embedded); }
}
function blipDigest(s, b) {
  if (!b) { s.tag('n'); return; }
  s.tag('b').str(b.ext).str(b.mime).digestOf(b.bytes);
}
function mapDigest(s, tag, m) {
  s.tag(tag).u32(m.size);
  for (const [k, sh] of m) { s.u32(k); shapeDigest(s, sh); }
}
function drawingsDigest(d) {
  const s = new Stream();
  s.tag('B').u32(d.bstore.length);
  for (const e of d.bstore) bseDigest(s, e);
  mapDigest(s, 'M', d.main);
  mapDigest(s, 'H', d.hdr);
  const ms = [...d.masters].sort((a, b) => a - b);
  s.tag('X').u32(ms.length);
  for (const m of ms) s.u32(m);
  return summary(s);
}

/** `loadDrawings` of reader.ts, copied line for line (checked equal to the DocReader on the fixtures). */
function loadDrawings(table, fc, lcb) {
  const d = { main: new Map(), hdr: new Map(), bstore: [], masters: new Set() };
  if (!lcb || fc + lcb > table.length) return d;
  const r = new ByteReader(table);
  const end = fc + lcb;
  const dgg = readRec(r, fc, end);
  if (!dgg || dgg.type !== OA.DggContainer) return d;
  for (const c of children(dgg)) if (c.type === OA.BStoreContainer) d.bstore = W.readBStore(c);
  let o = dgg.start + dgg.len;
  let guard = 0;
  while (o + 9 <= end && guard++ < 16) {
    const lbl = r.u8(o);
    const dg = readRec(r, o + 1, end);
    if (!dg || dg.type !== OA.DgContainer) break;
    W.indexDrawing(dg, lbl === 1 ? d.hdr : d.main);
    o = dg.start + dg.len;
  }
  for (const m of [d.main, d.hdr]) W.applyMasters(m, d.masters);
  return d;
}

/** `blipAt` of pictures.ts. */
function blipAt(stream, off, budget) {
  if (!stream || off + 8 > stream.length) return undefined;
  const rec = readRec(new ByteReader(stream), off);
  if (!rec || rec.type < 0xf018 || rec.type > 0xf117) return undefined;
  return W.decodeBlip(rec, budget);
}

/** Every BStore entry as `shapeBlip` decodes it, in order, one budget. */
function blipsDigest(bstore, word, left) {
  const s = new Stream();
  const budget = { left };
  for (const e of bstore) {
    if (!e) { s.tag('-'); continue; }
    if (e.embedded) blipDigest(s, W.decodeBlip(e.embedded, budget));
    else if (e.foDelay !== 0xffffffff) blipDigest(s, blipAt(word, e.foDelay, budget));
    else s.tag('0');
  }
  s.u64(budget.left);
  return summary(s);
}

/** The PICF walk of the Data stream (the records of readInlinePicture at each PICF in sequence). */
function inlineDigest(data, word, left) {
  const s = new Stream();
  const budget = { left };
  if (data) {
    const d = new ByteReader(data);
    let o = 0, n = 0;
    while (o + 0x44 <= data.length && n < 64) {
      n++;
      const lcb = d.i32(o), cbHeader = d.u16(o + 4);
      if (lcb < cbHeader || cbHeader < 0x44) break;
      const end = Math.min(data.length, o + lcb);
      let p = o + cbHeader;
      if (d.i16(o + 6) === 0x66) p += 1 + d.u8(p);
      s.tag('P').u64(o);
      for (const rec of recsIn(d, p, end)) {
        if (rec.type === OA.SpContainer) shapeDigest(s, W.readSpContainer(rec));
        else if (rec.type === OA.FBSE) {
          const e = W.readFbse(rec);
          bseDigest(s, e);
          if (e.embedded) blipDigest(s, W.decodeBlip(e.embedded, budget));
          else if (e.foDelay && e.foDelay !== 0xffffffff) blipDigest(s, blipAt(word, e.foDelay, budget));
          else s.tag('0');
        } else if (rec.type >= 0xf018 && rec.type <= 0xf117) blipDigest(s, W.decodeBlip(rec, budget));
        else s.tag('?').u32(rec.type);
      }
      o += lcb;
    }
  }
  s.u64(budget.left);
  return summary(s);
}

// ───────────────────────── byte builders ─────────────────────────

class Buf {
  constructor() { this.parts = []; this.n = 0; }
  u8(v) { this.raw(Uint8Array.of(v & 0xff)); return this; }
  u16(v) { const b = Buffer.alloc(2); b.writeUInt16LE(v & 0xffff); return this.raw(b); }
  i16(v) { const b = Buffer.alloc(2); b.writeInt16LE(((v << 16) >> 16)); return this.raw(b); }
  u32(v) { const b = Buffer.alloc(4); b.writeUInt32LE(v >>> 0); return this.raw(b); }
  i32(v) { const b = Buffer.alloc(4); b.writeInt32LE(v | 0); return this.raw(b); }
  raw(u8) { this.parts.push(u8 instanceof Uint8Array ? u8 : Uint8Array.from(u8)); this.n += u8.length; return this; }
  bytes() { return new Uint8Array(Buffer.concat(this.parts)); }
}
const cat = (...xs) => new Uint8Array(Buffer.concat(xs.map((x) => (x instanceof Uint8Array ? x : Uint8Array.from(x)))));
/** A record: header (ver, inst, type, len = body length unless given) + body. */
const rec = (ver, inst, type, body, len) => cat(new Buf().u16((inst << 4) | ver).u16(type).u32(len ?? body.length).bytes(), body);

function gen(rng) {
  const below = (n) => rng.below(n);
  const chance = (k) => below(k) === 0;
  const pick = (a) => a[below(a.length)];
  const rnd = (n) => { const b = new Uint8Array(n); for (let i = 0; i < n; i++) b[i] = rng.next() & 0xff; return b; };
  const u32 = () => rng.next() >>> 0;
  return { below, chance, pick, rnd, u32 };
}

const BOOL_IDS = [0x07f, 0x0bf, 0x13f, 0x17f, 0x1bf, 0x1ff, 0x23f, 0x33f, 0x3bf, 0x3ff];
const ARRAY_IDS = [0x145, 0x146, 0x151, 0x197, 0x1cf, 0x383, 0x3a0];
const SIMPLE_IDS = [0x004, 0x080, 0x104, 0x108, 0x109, 0x147, 0x148, 0x149, 0x14a, 0x181, 0x1c0, 0x1cb, 0x140, 0x142, 0x144];

function foptBody(g, spids) {
  const n = g.below(7);
  const entries = new Buf();
  const complex = [];
  let count = 0;
  for (let i = 0; i < n; i++) {
    const kind = g.below(10);
    let id, value, flags = 0, data;
    if (kind < 3) { id = g.pick(BOOL_IDS); value = g.chance(3) ? g.u32() & 0xffff : g.u32(); }
    else if (kind < 5 && spids.length) { id = 0x301; value = g.chance(5) ? g.u32() : g.pick(spids); }
    else if (kind < 7) {
      id = g.pick(ARRAY_IDS); flags = 0x8000;
      const nE = g.below(5), cb = g.pick([2, 4, 8, 0xfff0, 3]);
      const cell = cb === 0xfff0 ? 4 : cb;
      const arr = cat(new Buf().u16(nE).u16(nE).u16(cb).bytes(), g.rnd(nE * cell));
      const mode = g.below(5);
      value = mode === 0 ? arr.length - 6 : mode === 1 ? arr.length + g.below(20) : mode === 2 ? g.u32() : arr.length;
      data = arr;
    } else if (kind < 8) { id = g.pick([0x380, 0x381, 0x3a9]); flags = 0x8000; data = g.rnd(g.below(12)); value = g.chance(4) ? data.length + 7 : data.length; }
    else { id = g.chance(4) ? g.below(0x4000) : g.pick(SIMPLE_IDS); value = g.u32(); if (id === 0x104 && g.chance(2)) flags |= 0x4000; }
    if (g.chance(12)) flags |= 0x8000;
    entries.u16(id | flags).u32(value);
    if (data) complex.push(data);
    count++;
  }
  const body = cat(entries.bytes(), ...complex);
  return { count, body };
}

function spContainer(g, spid, opts = {}) {
  const kids = [];
  const flags = (opts.flags ?? 0) | (g.chance(10) ? 0x8 : 0) | (g.chance(8) ? 0x400 : 0) | (g.chance(4) ? 0xc0 : 0) | (g.chance(6) ? 0x20 : 0);
  const spt = g.chance(6) ? g.pick([0, 24, 136, 175, 202, 4095]) : g.below(210);
  const order = g.below(4);
  const fsp = rec(2, spt, 0xf00a, new Buf().u32(spid).u32(flags).bytes());
  if (order !== 1) kids.push(fsp);
  const nf = g.below(4);
  for (let i = 0; i < nf; i++) {
    const { count, body } = foptBody(g, opts.spids ?? []);
    kids.push(rec(3, g.chance(15) ? count + 3 : count, g.pick([0xf00b, 0xf00b, 0xf121, 0xf122]), body));
  }
  if (opts.group || g.chance(8)) kids.push(rec(1, 0, 0xf009, g.chance(8) ? g.rnd(8) : new Buf().i32(g.u32()).i32(g.below(1000)).i32(g.u32()).i32(21600).bytes()));
  if (g.chance(2)) kids.push(rec(0, 0, 0xf00f, g.chance(6) ? g.rnd(g.below(16)) : g.rnd(16)));
  if (g.chance(4)) kids.push(rec(0, 0, 0xf010, g.rnd(g.pick([4, 8, 16, 18]))));
  if (g.chance(5)) kids.push(rec(0, 0, 0xf011, g.rnd(g.below(6))));
  if (g.chance(6)) kids.push(rec(0, 0, 0xf00d, g.rnd(4)));
  if (g.chance(6)) kids.push(rec(0, 0, g.pick([0xf122, 0xf00b]), g.rnd(g.below(14))));
  if (g.chance(8)) kids.push(rec(0, 0, 0xf123, g.rnd(3)));
  if (order === 1) kids.push(fsp);
  if (g.chance(5)) kids.push(fsp);
  return rec(0xf, 0, 0xf004, cat(...kids));
}

function spgrContainer(g, spids, depth) {
  const kids = [];
  if (!g.chance(8)) kids.push(spContainer(g, g.pick(spids), { flags: 0x1 | (depth === 0 ? 0x4 : 0), group: true, spids }));
  const n = g.below(depth === 0 ? 4 : 3);
  for (let i = 0; i < n; i++) {
    if (depth < 2 && g.chance(4)) kids.push(spgrContainer(g, spids, depth + 1));
    else kids.push(spContainer(g, g.pick(spids), { spids }));
  }
  if (g.chance(10)) kids.push(rec(0, 0, 0xf00a, g.rnd(8)));
  return rec(0xf, 0, 0xf003, cat(...kids));
}

function dgContainer(g, spids) {
  const kids = [rec(0, g.below(4), 0xf008, g.rnd(8))];
  const n = 1 + g.below(2);
  for (let i = 0; i < n; i++) kids.push(spgrContainer(g, spids, 0));
  if (g.chance(2)) kids.push(spContainer(g, g.pick(spids), { flags: 0x400, spids }));
  if (g.chance(5)) kids.push(rec(0xf, 0, 0xf005, g.rnd(4)));
  return rec(0xf, g.below(4), 0xf002, cat(...kids));
}

const UID2 = { 0xf01a: 0x3d5, 0xf01b: 0x217, 0xf01c: 0x543, 0xf01d: 0x46b, 0xf02a: 0x6e3, 0xf01e: 0x6e1, 0xf01f: 0x7a9, 0xf029: 0x6e5 };
const UID1 = { 0xf01a: 0x3d4, 0xf01b: 0x216, 0xf01c: 0x542, 0xf01d: 0x46a, 0xf02a: 0x6e2, 0xf01e: 0x6e0, 0xf01f: 0x7a8, 0xf029: 0x6e4 };

function dib(g) {
  const b = new Buf();
  const hdr = g.pick([40, 40, 12, 108, 0, 124]);
  b.u32(hdr).i32(g.below(5) + 1).i32(g.below(5) + 1).u16(1).u16(g.pick([1, 4, 8, 24, 32, 0, 16])).u32(g.pick([0, 0, 3, 1]));
  b.u32(g.below(100)).i32(2835).i32(2835).u32(g.pick([0, 0, 2, 300])).u32(0);
  const full = b.bytes();
  return g.chance(6) ? full.slice(0, g.below(40)) : cat(full, g.rnd(g.below(40)));
}

function metafile(g) {
  // a fake metafile: placeable WMF key sometimes, else random
  const body = g.chance(5) ? cat([0xd7, 0xcd, 0xc6, 0x9a], g.rnd(10 + g.below(24))) : cat(g.rnd(4), new Uint8Array(20 + g.below(80)).fill(g.below(256)));
  const c = g.below(8);
  if (c === 0 || c === 1) return { comp: 0, data: deflateSync(body, { level: g.pick([1, 6, 9]) }), size: body.length };
  if (c === 2) return { comp: 0, data: deflateRawSync(body), size: body.length };
  if (c === 3) return { comp: 0, data: cat(deflateSync(body), g.rnd(1 + g.below(5))), size: body.length };
  if (c === 4) return { comp: 0, data: g.rnd(g.below(30)), size: body.length };
  if (c === 5) return { comp: 0, data: deflateSync(body).slice(0, 4 + g.below(8)), size: body.length };
  return { comp: 0xfe, data: body, size: body.length };
}

function blipRec(g) {
  const type = g.pick([0xf01e, 0xf01d, 0xf02a, 0xf029, 0xf01f, 0xf01a, 0xf01b, 0xf01c, 0xf01b, 0xf01a, 0xf020, 0xf117]);
  const two = g.chance(2);
  const inst = UID2[type] ? (g.chance(10) ? g.below(0xfff) : two ? UID2[type] : UID1[type]) : g.below(0xfff);
  const uids = g.rnd(two ? 32 : 16);
  let body;
  if (type === 0xf01a || type === 0xf01b || type === 0xf01c) {
    const m = metafile(g);
    const cb = g.pick([m.size, m.size, 0, Math.max(0, m.size - 1 - g.below(10)), m.size + 50]);
    const l = g.chance(3) ? -g.below(1000) : g.below(3000), t = g.below(2000);
    const r = g.chance(6) ? l - 1 : l + g.below(70000), bt = t + g.below(5000);
    const hdr = new Buf().u32(cb).i32(l).i32(t).i32(r).i32(bt).i32(g.chance(5) ? 0 : g.chance(5) ? -5 : g.below(5_000_000) + 1).i32(g.below(5_000_000)).u32(m.data.length).u8(m.comp).u8(0xfe).bytes();
    body = cat(uids, hdr, m.data);
    if (g.chance(10)) body = body.slice(0, uids.length + g.below(34));
  } else if (type === 0xf01f) body = cat(uids, [0xff], dib(g));
  else body = cat(uids, [0xff], g.rnd(g.below(40)));
  return rec(0, inst, type, body, g.chance(12) ? body.length + g.below(50) : undefined);
}

function fbse(g, blip, foDelay) {
  const cbName = g.chance(4) ? g.below(6) : 0;
  const b = new Buf().u8(g.below(8)).u8(g.below(8)).raw(g.rnd(16)).u16(0xff).u32(blip ? blip.length : g.u32()).u32(g.below(5)).u32(foDelay).u8(0).u8(cbName).u8(0).u8(0).raw(g.rnd(cbName));
  const body = blip ? cat(b.bytes(), blip) : b.bytes();
  return rec(2, g.below(8), 0xf007, g.chance(15) ? body.slice(0, g.below(body.length + 1)) : body);
}

/** A generated document: OfficeArtContent in a padded table stream, a delay stream and a Data stream. */
function genDoc(g) {
  const spids = Array.from({ length: 2 + g.below(10) }, () => 0x400 + g.below(24));
  // delay stream: padding then BLIPs
  const word = new Buf().raw(g.rnd(g.below(64)));
  const nb = g.below(6);
  const bses = [];
  for (let i = 0; i < nb; i++) {
    const kind = g.below(6);
    if (kind === 0) { bses.push(rec(0, 0, 0xf00b, g.rnd(6))); continue; }
    if (kind <= 2) { bses.push(fbse(g, blipRec(g), g.chance(3) ? 0 : 0xffffffff)); continue; }
    const at = word.n;
    word.raw(blipRec(g));
    const fo = g.chance(8) ? g.pick([0xffffffff, at + 3, 1 << 30, word.n]) : at;
    bses.push(fbse(g, undefined, fo));
  }
  const dggKids = [rec(0, 0, 0xf006, g.rnd(16 + 8 * g.below(3)))];
  if (g.chance(2)) dggKids.push(rec(3, 1, 0xf00b, new Buf().u16(0x181).u32(g.u32()).bytes()));
  if (nb) dggKids.push(rec(0xf, nb, 0xf001, cat(...bses)));
  if (g.chance(8) && nb) dggKids.push(rec(0xf, 0, 0xf001, cat(...bses.slice(0, 1))));
  if (g.chance(6)) dggKids.push(rec(0, 4, 0xf11e, g.rnd(16)));
  const dgg = rec(0xf, 0, 0xf000, cat(...dggKids), g.chance(15) ? undefined : undefined);
  const parts = [dgg];
  const nd = g.below(3);
  for (let i = 0; i < nd; i++) { parts.push(Uint8Array.of(g.pick([0, 1, 1, 2]))); parts.push(dgContainer(g, spids)); }
  if (g.chance(4)) parts.push(g.rnd(g.below(12)));
  const content = cat(...parts);
  const pad = g.rnd(g.below(24));
  const table = cat(pad, content, g.rnd(g.below(8)));
  let lcb = content.length;
  if (g.chance(8)) lcb = Math.max(0, lcb - g.below(40));
  if (g.chance(20)) lcb = table.length;
  // Data stream: PICFs
  const data = new Buf();
  const np = g.below(4);
  for (let i = 0; i < np; i++) {
    const named = g.chance(3);
    const name = g.rnd(g.below(5));
    const recs = [];
    const nr = 1 + g.below(4);
    for (let k = 0; k < nr; k++) {
      const c = g.below(4);
      if (c === 0) recs.push(spContainer(g, g.pick(spids), { spids }));
      else if (c === 1) recs.push(fbse(g, blipRec(g), 0xffffffff));
      else if (c === 2) recs.push(fbse(g, undefined, g.pick([0, word.n, 1 << 20])));
      else recs.push(blipRec(g));
    }
    const tail = cat(...(named ? [Uint8Array.of(name.length), name] : []), ...recs);
    const cbHeader = g.chance(10) ? g.pick([0x10, 0x50]) : 0x44;
    const hdr = new Buf().i32(cbHeader + tail.length).u16(cbHeader).i16(named ? 0x66 : 0x64).raw(new Uint8Array(cbHeader - 8));
    data.raw(cat(hdr.bytes(), tail));
  }
  const budget = g.pick([DOC_BUDGET, DOC_BUDGET, 0, 10, 200, 1000, 5000]);
  return { table, fc: pad.length, lcb, word: word.bytes(), data: np ? data.bytes() : undefined, budget };
}

// ───────────────────────── capture ─────────────────────────

const cap = new CaptureWriter([
  'uo_drawingml::odraw parity: written by core/crates/uo-drawingml/tools/capture-odraw.mjs, replayed by tests/odraw_parity.rs.',
  'doc/<name>: .doc fixtures (src, streams, drawings, blips, inline); gen/<k>: generated contents (hex, fc, lcb, word, data, budget); mut/<base>/<k>: edits of a base;',
  'fopt/<k>: FOPT blocks (word, slides, sheet); blip/<k>: one BLIP (word, slides, sheet); case/<k> + msospt/<spt>: the preset map (word, slides, sheet).',
]);
cap.meta('ts-dist', 'write/packages/docx-core/dist/doc, slides/packages/slides-core/dist/ppt, sheet/packages/sheet-core/dist/xls (frozen main, 0.9.x)');

const bases = [];

// fixtures
const fixDir = join(repo, 'write', 'packages', 'docx-core', 'test', 'fixtures', 'doc');
for (const f of readdirSync(fixDir).sort().filter((n) => n.endsWith('.doc'))) {
  const r = new DocReader(new Uint8Array(readFileSync(join(fixDir, f))));
  const { fc, lcb } = r.fib.fcLcb(FC.DggInfo);
  r.shape(0, false);
  const real = { bstore: r.bstore, main: r.shapesMain, hdr: r.shapesHdr, masters: r.masters };
  const d = drawingsDigest(real);
  const copy = drawingsDigest(loadDrawings(r.table, fc, lcb));
  if (d !== copy) throw new Error(`loadDrawings copy differs from DocReader on ${f}`);
  const key = `doc/${f.replace(/\.doc$/, '')}`;
  cap.set(key, 'src', `${r.fib.fWhichTblStm ? '1Table' : '0Table'}:${fc}:${lcb}`);
  cap.set(key, 'streams', `${sha(r.table)}:${sha(r.word)}:${sha(r.data)}`);
  cap.set(key, 'drawings', d);
  cap.set(key, 'blips', blipsDigest(r.bstore, r.word, DOC_BUDGET));
  cap.set(key, 'inline', inlineDigest(r.data, r.word, DOC_BUDGET));
  bases.push({ key, table: r.table, fc, lcb, word: r.word, data: r.data, budget: DOC_BUDGET });
}

function docFacets(key, d) {
  const dr = loadDrawings(d.table, d.fc, d.lcb);
  cap.set(key, 'drawings', drawingsDigest(dr));
  cap.set(key, 'blips', blipsDigest(dr.bstore, d.word, d.budget));
  cap.set(key, 'inline', inlineDigest(d.data, d.word, d.budget));
}

// generated
const rng = new Rng(0x0d4a0001);
const g = gen(rng);
const NGEN = 100;
for (let k = 0; k < NGEN; k++) {
  const d = genDoc(g);
  const key = `gen/${k}`;
  cap.set(key, 'hex', hex(d.table));
  cap.set(key, 'fc', d.fc);
  cap.set(key, 'lcb', d.lcb);
  cap.set(key, 'word', hex(d.word));
  cap.set(key, 'data', d.data ? hex(d.data) : '-');
  cap.set(key, 'budget', d.budget);
  docFacets(key, d);
  bases.push({ key, ...d });
}

// mutations
const mrng = new Rng(0x0d4a0002);
const mg = gen(mrng);
for (const b of bases) {
  const n = b.key.startsWith('doc/') ? 24 : 2;
  for (let k = 0; k < n; k++) {
    const on = b.data && mg.chance(6) ? 'd' : b.word && b.word.length && mg.chance(6) ? 'w' : 't';
    const src = on === 'd' ? b.data : on === 'w' ? b.word : b.table;
    const lo = on === 't' ? b.fc : 0;
    const span = on === 't' ? b.lcb : src.length;
    if (!span) continue;
    const bytes = src.slice();
    const edits = [];
    const ne = 1 + mg.below(6);
    for (let i = 0; i < ne; i++) {
      const pos = mg.below(span);
      const val = mg.chance(3) ? mg.pick([0, 0xff, 0x0f, 0xf0, 0x80]) : mg.below(256);
      bytes[lo + pos] = val;
      edits.push(`${pos}:${val}`);
    }
    let lcb = b.lcb;
    if (on === 't' && mg.chance(4)) lcb = mg.below(b.lcb + 1);
    const key = `mut/${b.key}/${k}`;
    cap.set(key, 'on', on);
    cap.set(key, 'edits', edits.join(','));
    cap.set(key, 'cut', lcb);
    docFacets(key, {
      table: on === 't' ? bytes : b.table, fc: b.fc, lcb,
      word: on === 'w' ? bytes : b.word, data: on === 'd' ? bytes : b.data, budget: b.budget,
    });
  }
}

// FOPT blocks: the three readers
function wordTable(m) {
  const s = new Stream();
  s.u32(m.size);
  for (const [id, p] of m) { s.u32(id).u32(p.id).u32(p.value).bool(p.bid).bool(!!p.complex); if (p.complex) s.digestOf(p.complex); }
  return summary(s);
}
const frng = new Rng(0x0d4a0003);
const fg = gen(frng);
for (let k = 0; k < 250; k++) {
  const parts = [];
  const nr = 1 + fg.below(4);
  const spids = [0x401, 0x402];
  for (let i = 0; i < nr; i++) {
    const { count, body } = foptBody(fg, spids);
    const inst = fg.chance(10) ? count + fg.below(5) : count;
    parts.push(rec(3, inst, fg.pick([0xf00b, 0xf121, 0xf122, 0xf122]), body, fg.chance(12) ? body.length + 4 : undefined));
  }
  if (fg.chance(3)) parts.push(rec(3, 2, 0xf122, new Buf().u16(0x1bf).u32(fg.chance(2) ? 0x00100010 : 0x00100000).u16(0x1bf | 0x8000).u32(2).raw([1, 2]).bytes()));
  const block = cat(...parts);
  const key = `fopt/${k}`;
  cap.set(key, 'hex', hex(block));
  const wm = new Map();
  for (const c of recsIn(new ByteReader(block), 0, block.length)) if (c.type === 0xf00b || c.type === 0xf121 || c.type === 0xf122) W.readFopt(c, wm);
  cap.set(key, 'word', wordTable(wm));
  const sm = new Map();
  for (const c of SR.recsIn(new SR.ByteReader(block), 0, block.length)) if (c.type === 0xf00b || c.type === 0xf121 || c.type === 0xf122) S.readFopt(c, sm);
  cap.set(key, 'slides', wordTable(sm));
  const x = X.readFoptBlock(block);
  const s = new Stream();
  s.u32(x.props.size);
  for (const [id, v] of x.props) s.u32(id).u32(v);
  s.u32(x.complex.size);
  for (const [id, b] of x.complex) s.u32(id).digestOf(b);
  cap.set(key, 'sheet', summary(s));
}

// BLIPs: the three readers
const brng = new Rng(0x0d4a0004);
const bg = gen(brng);
for (let k = 0; k < 250; k++) {
  const b = blipRec(bg);
  const budget = bg.pick([DOC_BUDGET, DOC_BUDGET, DOC_BUDGET, 0, 50, 300]);
  const key = `blip/${k}`;
  cap.set(key, 'hex', hex(b));
  cap.set(key, 'budget', budget);
  const one = (decode) => {
    const s = new Stream();
    const bud = { left: budget };
    blipDigest(s, decode(bud));
    s.u64(bud.left);
    return summary(s);
  };
  cap.set(key, 'word', one((bud) => W.decodeBlip(readRec(new ByteReader(b), 0), bud)));
  const type = b[2] | (b[3] << 8);
  if (type !== 0xf01c) cap.set(key, 'slides', one((bud) => S.decodeBlip(SR.readRec(new SR.ByteReader(b), 0), bud)));
  // Sheet: inside DggContainer > BStoreContainer > FBSE (empty name), BlipStore.get(1) with its own document budget
  const fb = rec(2, 0, 0xf007, cat(new Buf().u8(0).u8(0).raw(new Uint8Array(16)).u16(0).u32(b.length).u32(1).u32(0).u8(0).u8(0).u8(0).u8(0).bytes(), b));
  const dgg = rec(0xf, 0, 0xf000, rec(0xf, 1, 0xf001, fb));
  const store = new X.BlipStore(dgg);
  const s = new Stream();
  blipDigest(s, store.get(1));
  cap.set(key, 'sheet', summary(s));
}

// msospt: cases, then each type over all cases with the three copies
const prng = new Rng(0x0d4a0005);
const pg = gen(prng);
const ADJ_POOL = [0, 1350, 5400, 10800, 16515, 17520, 21600, 25920, -1, -2147483648, 2147483647, 2538, 13800, 1329];
const bits = (x) => { const b = Buffer.alloc(8); b.writeDoubleLE(x); return b.readBigUInt64LE().toString(16).padStart(16, '0'); };
const cases = [];
const WH = [1, 1037, 749, 914400, 3, 0.5, -200, 21600, 1e9];
const WH_ODD = [0, -0, NaN];
for (let k = 0; k < 40; k++) {
  const adj = [];
  for (let i = 0; i < 10; i++) adj.push(k === 0 || pg.chance(2) ? undefined : pg.chance(2) ? pg.pick(ADJ_POOL) : (pg.u32() % 120000) - 50000);
  const odd = k >= 34;
  const w = odd ? pg.pick(WH_ODD) : pg.pick(WH), h = odd ? pg.pick([...WH_ODD, 1, 7]) : pg.pick(WH);
  cases.push({ adj, w, h });
  cap.set(`case/${k}`, 'adj', adj.map((v) => (v === undefined ? '' : String(v))).join(','));
  cap.set(`case/${k}`, 'w', bits(w));
  cap.set(`case/${k}`, 'h', bits(h));
}
const presetDigest = (s, r) => {
  if (!r) { s.tag('n'); return; }
  s.tag('r').str(r.prst).bool(!!r.flipV).u32(r.av.length);
  for (const a of r.av) s.str(a.name).f64(a.val);
};
const diffs = { same: 0, wordArtRect: 0, whOne: 0, flipVFalse: 0, other: 0 };
const types = [...Array.from({ length: 256 }, (_, i) => i), 4095];
for (const spt of types) {
  const ws = new Stream(), ss = new Stream(), xs = new Stream();
  for (const c of cases) {
    // Write and Slides callers pass w || 1 and h || 1; the raw values reach presetFor here
    const wr = WP.presetFor(spt, c.adj, c.w, c.h);
    const sr = SP.presetFor(spt, c.adj, c.w, c.h);
    const xr = XG.presetGeomXml(spt, c.adj, c.w, c.h);
    presetDigest(ws, wr);
    presetDigest(ss, sr);
    if (xr) xs.tag('x').str(xr.xml).bool(xr.flipV); else xs.tag('n');
    // entry by entry comparison
    if (JSON.stringify(wr) !== JSON.stringify(sr)) diffs.other++;
    const asXml = (r) => (r ? `<a:prstGeom prst="${r.prst}"><a:avLst>${r.av.map((a) => `<a:gd name="${a.name}" fmla="val ${a.val}"/>`).join('')}</a:avLst></a:prstGeom>` : undefined);
    const wx = asXml(wr);
    if (wx === xr?.xml && !!wr?.flipV === !!xr?.flipV) diffs.same++;
    else if (WP.isWordArtType(spt) && wr?.prst === 'rect' && !xr) diffs.wordArtRect++;
    else if (asXml(WP.presetFor(spt, c.adj, c.w || 1, c.h || 1)) === xr?.xml) diffs.whOne++;
    else diffs.other++;
    if (wr && xr && !wr.flipV && xr.flipV === false) diffs.flipVFalse++;
  }
  cap.set(`msospt/${spt}`, 'word', summary(ws));
  cap.set(`msospt/${spt}`, 'slides', summary(ss));
  cap.set(`msospt/${spt}`, 'sheet', summary(xs));
  if (WP.isWordArtType(spt) !== XG.isWordArtType(spt) || WP.isConnectorType(spt) !== XG.isConnectorType(spt) || SP.isWordArtType(spt) !== WP.isWordArtType(spt)) diffs.other++;
}
cap.meta('preset-diff', `cases=${types.length * cases.length} same=${diffs.same} wordart-rect-vs-none=${diffs.wordArtRect} w-h-or-1=${diffs.whOne} other=${diffs.other} (flipV false vs absent, same meaning: ${diffs.flipVFalse})`);
cap.write(out);
console.log(`${cap.records.size} records; preset diffs: ${JSON.stringify(diffs)}`);
