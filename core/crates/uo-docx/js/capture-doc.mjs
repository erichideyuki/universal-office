// Parity capture of the `.doc` converter (frontier D4, F0-F6): digests of the frozen TS
// `docx-core/src/doc/**` (the `.doc` -> `.docx` converter and the readers/emitters behind it) over the 11
// repository fixtures, 4,008 byte-level mutations of them and 2,500 + 700 + 700 + 800 generated
// grpprls / style sheets / list tables / DOPs. Replay: tests/parity_doc.rs (its canonical streams mirror
// the functions of this file that have the same names).
//
//   UO_TS_ROOT=<built checkout of main> node core/crates/uo-docx/js/capture-doc.mjs [--out <file>] [--inputs <capture>]
//
// Writes tests/parity/doc.capture (or `--out`). Run twice: byte-identical.
//
// THE INPUTS. A record's input is rebuilt from what the capture stores about it, never from a PRNG:
//   doc/<name>            the fixture write/packages/docx-core/test/fixtures/doc/<name>.doc
//   mut/<name>/<tag>-<k>  that fixture with `edits=pos:val,...` applied (positions past the end are ignored),
//                         then truncated to `cut=len` (`-` = none)
//   gen/grpprl/<k>        the grpprl in `hex`
//   gen/stsh/<k>          a Stshi + rglpstd in `hex` (the whole table stream, `Stshf` = 0:len)
//   gen/lists/<k>         the table stream in `hex`, `lst=fc:lcb` (PlfLst) and `lfo=fc:lcb` (PlfLfo)
//   gen/dop/<k>           the Dop in `hex`, with `nfib` and `ftcfe`
// They are read from the previous capture (`--inputs`, default: the committed doc.capture), in its record
// order: the mutation and table generators (aimed at the CFB header/directory/FAT, the FIB counters and
// fc/lcb pairs, the Clx, FKP pages, grpprl bytes, STSH, PlfLst/PlfLfo, PLC sizes, noise and cuts; ccp
// counts kept <= 10^6) are not part of this file. The `@corpus` count `left-out-ts-memory` carries over
// from the inputs' capture plus the inputs that exhaust the TS here.
//
// THE FACETS (each value a hash of a canonical stream, `items:sha256[0..32]` unless said otherwise):
//   files (doc/ and mut/)
//     edits, cut   copied from the inputs (mutations only)
//     input        `len:sha256[0..16]` of the rebuilt file
//     isword       `isWordBinary` (1/0)
//     rd           `ok` or the class of the failure of `new DocReader(bytes)` (below)
//     convert      `ok` or the class of the failure of `convertDocToDocx(bytes)`
//     parts        (convert ok) part names in zip order and each part's decompressed bytes as len:SHA-256
//     model        (convert ok) `readFullDocument(DocxPackage.load(docx))` through canon.mjs (`main/order`
//                  streams), `err` when the reader throws
//     (rd ok:)
//     fib text fkp fonts   FIB fields and every fc/lcb pair; pieces + prcs + the decoded text; CHPX/PAPX runs and
//                  their sprms; the font table
//     chp chp2 pap pap2 pap3 tap sep rpr ppr   `count:hash` of the appliers (applyChp/Pap/Tap/Sep and the
//                  rPrInner/pPrInner emitters) over every CHPX/PAPX grpprl of the file and every section's
//                  SEPX; `count` = grpprls + sections
//     stsh stsh2 stsh3     style table + resolved styles in ascending, descending order, character styles
//     lists        PlfLst/PlfLfo
//     xml.styles styleids xml.numbering xml.settings xml.settings2 dop    the F5 emitters over a fresh resolver
//     sections notes comments bookmarks subs fspas textboxes ffdata picf shapes    the reader assembly
//   gen/grpprl: hex sprm chp chp2 pap pap2 pap3 tap sep rpr ppr
//   gen/stsh:   hex stsh stsh2 stsh3 xml.styles styleids
//   gen/lists:  hex lst lfo lists xml.numbering
//   gen/dop:    hex nfib ftcfe xml.settings xml.settings2 dop
// Error classes (from the Portuguese messages of the DocFormatError throw sites): not-cfb, cfb (readCfb
// failed), encrypted, obfuscated, no-word-document, pre-word97, fib-truncated, fib-signature,
// no-table-stream, no-clx, no-pcdt, empty-pieces, corrupt (anything else, `convertDocToDocx` catch-all).
//
// THE WORKER. The TS runs in a worker with a 1.5 GB heap: V8 aborts the process on out-of-memory and a
// worker dies alone. An input that exhausts it is left out (no record) and counted in `left-out-ts-memory`.
import { createHash } from 'node:crypto';
import { readFileSync } from 'node:fs';
import { join } from 'node:path';
import { Buffer } from 'node:buffer';
import { Worker, isMainThread, parentPort, workerData } from 'node:worker_threads';
import { fileURLToPath } from 'node:url';
import { CaptureWriter, Stream } from '../../uo-testkit/js/testkit.mjs';

const HEADER = '`.doc` converter (D4), frontiers F0-F6: digests of the TS docx-core/src/doc over the fixtures, generated mutations and generated grpprls, see js/capture-doc.mjs';
const DOP_KEYS = 'facingPages,mirrorMargins,dxaTab,dxaHotZ,cConsecHypLim,autoHyphen,hyphCapitals,revMarking,fpc,rncFtn,nFtn,rncEdn,nEdn,epc,zoom,copts60,copts80hi,copts,copts2,protForms,lockAtn,lockRev,lockAtnReadOnly,styleLock,enforceDocProt,docProtCur,printFormData,saveFormData,shadeFormData,includeSubdocsInStats,snapBorder,includeHeader,includeFooter,kerningPunct,justification';
const CHP_KEYS = 'bold,italic,strike,dstrike,outline,shadow,emboss,imprint,smallCaps,caps,vanish,specVanish,webHidden,boldBi,italicBi,rtl,cs,noProof,kul,cvUl,hps,hpsBi,hpsPos,hpsKern,ico,cv,highlight,iss,dxaSpace,charScale,kcd,ftcAscii,ftcFE,ftcOther,ftcBi,lid,lidFE,lidBi,istd,fSpec,fData,fOle2,fObj,picLocation,symbol,shd,brc,rmarkDel,rmarkIns,ibstRMark,dttmRMark,ibstRMarkDel,dttmRMarkDel,fldVanish,idctHint,sfxText';
const PAP_KEYS = 'jc,dxaLeft,dxaRight,dxaLeft1,dxcLeft,dxcRight,dxcLeft1,dyaBefore,dyaAfter,beforeAuto,afterAuto,dylBefore,dylAfter,lineSpacing,keep,keepFollow,pageBreakBefore,widowControl,noLineNumb,noAutoHyph,contextualSpacing,mirrorIndents,kinsoku,wordWrap,overflowPunct,topLinePunct,autoSpaceDE,autoSpaceDN,bidi,adjustRight,usePgsuSettings,wAlignFont,ilfo,ilvl,outLvl,tabs,tabsCleared,brcTop,brcLeft,brcBottom,brcRight,brcBetween,brcBar,shd,inTable,ttp,itap,innerTableCell,innerTtp,dxaAbs,dyaAbs,dxaWidth,pc,wr,wHeightAbs,dyaFromText,dxaFromText,frameLocked,dcs,tableGrpprl';
const CHP_LIST = CHP_KEYS.split(',');
const PAP_LIST = PAP_KEYS.split(',');
const DOP_LIST = DOP_KEYS.split(',');

const here = fileURLToPath(new URL('.', import.meta.url));
const crate = join(here, '..');
const repo = join(here, '..', '..', '..', '..');
const FIXTURES = join(repo, 'write', 'packages', 'docx-core', 'test', 'fixtures', 'doc');
const DEFAULT_CAPTURE = join(crate, 'tests', 'parity', 'doc.capture');

const argv = process.argv.slice(2);
const arg = (name, dflt) => (argv.includes(name) ? argv[argv.indexOf(name) + 1] : dflt);
const inputsPath = arg('--inputs', DEFAULT_CAPTURE);

/** The records of a capture file: [{ key, facets: [[name, value]...] }] and its `@meta` lines. */
function parseCapture(path) {
  const records = [];
  const meta = new Map();
  for (const line of readFileSync(path, 'utf8').split('\n')) {
    if (!line || line.startsWith('#')) continue;
    if (line.startsWith('@')) {
      const sp = line.indexOf(' ');
      meta.set(line.slice(1, sp), line.slice(sp + 1));
      continue;
    }
    const [key, ...rest] = line.split('\t');
    records.push({ key, facets: rest.map((f) => { const i = f.indexOf('='); return [f.slice(0, i), f.slice(i + 1)]; }) });
  }
  return { records, meta };
}

const unhex = (h) => (h === '-' ? new Uint8Array(0) : new Uint8Array(Buffer.from(h, 'hex')));
const get = (rec, name) => rec.facets.find(([n]) => n === name)?.[1];

// ═════════════════════════════════════════ worker ═════════════════════════════════════════

async function runWorker() {
  const { importFrom, tsRoot } = await import('../../uo-testkit/js/testkit.mjs');
  const { createRequire } = await import('node:module');
  const root = tsRoot();
  const docCore = (rel) => importFrom(root, `write/packages/docx-core/dist/${rel}`);
  const { R2Canon } = await import('./capture-r2-common.mjs');
  const { isWordBinary, convertDocToDocx } = await docCore('doc/index.js');
  const { DocReader } = await docCore('doc/reader.js');
  const { DocFormatError } = await docCore('doc/fib.js');
  const { applyChp, applyPap, applyTap, applySep, defaultSep, finishTap, newTap, pruneNoneBorders } = await docCore('doc/props.js');
  const { prls } = await docCore('doc/sprm.js');
  const { FC } = await docCore('doc/fib.js');
  const { readPlc } = await docCore('doc/fkp.js');
  const { ByteReader } = await docCore('doc/bytes.js');
  const { readStsh, StyleResolver } = await docCore('doc/stsh.js');
  const { readLists } = await docCore('doc/lists.js');
  const { buildStyles, styleIdMap } = await docCore('doc/styles-xml.js');
  const { buildNumbering } = await docCore('doc/numbering.js');
  const { buildSettings, readDop } = await docCore('doc/settings.js');
  const { pPrInner, rPrInner } = await docCore('doc/ooxml-props.js');
  const { ffDataXml } = await docCore('doc/formfields.js');
  const { readInlinePicture, shapeBlip, picFxOf } = await docCore('doc/pictures.js');
  const { DocxPackage } = await docCore('package.js');
  const { readFullDocument } = await docCore('fulldoc.js');
  const fflate = createRequire(join(root, 'write', 'packages', 'docx-core', 'dist', 'package.js'))('fflate');

  const utf16 = (s) => Buffer.from(s, 'utf16le');
  const summary = (s) => `${s.items}:${s.finish().short}`;
  const sha = (u8) => createHash('sha256').update(u8).digest('hex');

  // ───────────── error classes ─────────────
  const classOf = (e) => {
    const m = e instanceof DocFormatError ? e.message : '';
    if (!(e instanceof DocFormatError)) return 'corrupt';
    if (m.startsWith('O arquivo não é um .doc do Word 97-2003')) return 'not-cfb';
    if (m.startsWith('Arquivo .doc corrompido ou não suportado')) return 'corrupt';
    if (m.startsWith('Arquivo .doc corrompido: FIB truncado')) return 'fib-truncated';
    if (m.startsWith('Arquivo .doc corrompido: assinatura do FIB')) return 'fib-signature';
    if (/^Arquivo \.doc corrompido: stream [01]Table ausente/.test(m)) return 'no-table-stream';
    if (m.startsWith('Arquivo .doc corrompido: tabela de peças (Clx) ausente')) return 'no-clx';
    if (m.startsWith('Arquivo .doc corrompido: Pcdt ausente')) return 'no-pcdt';
    if (m.startsWith('Arquivo .doc corrompido: tabela de peças vazia')) return 'empty-pieces';
    if (m.includes('ofuscação XOR')) return 'obfuscated';
    if (m.startsWith('O documento está protegido por senha')) return 'encrypted';
    if (m.includes('stream WordDocument ausente')) return 'no-word-document';
    if (m.startsWith('Documento de uma versão anterior ao Word 97')) return 'pre-word97';
    if (m.startsWith('Arquivo .doc corrompido: ')) return 'cfb';
    return 'corrupt';
  };

  // ───────────── canonical streams (mirrored in tests/parity_doc.rs) ─────────────
  const prlList = (s, grpprl) => {
    const list = [...prls(grpprl)];
    s.u32(list.length);
    for (const p of list) s.u32(p.sprm).u64(p.op).u64(p.len);
  };
  const fibDigest = (f) => {
    const s = new Stream();
    s.tag('F').u32(f.wIdent).u32(f.nFib).u32(f.lid);
    for (const b of [f.fDot, f.fComplex, f.fEncrypted, f.fWhichTblStm, f.fObfuscated, f.fExtChar, f.fFarEast]) s.bool(b);
    s.u32(f.lidFE);
    for (const v of [f.ccpText, f.ccpFtn, f.ccpHdd, f.ccpMcr, f.ccpAtn, f.ccpEdn, f.ccpTxbx, f.ccpHdrTxbx]) s.i64(v);
    for (let i = 0; i < 93; i++) { const p = f.fcLcb(i); s.u32(p.fc).u32(p.lcb); }
    return summary(s);
  };
  const runsStream = (runs, tag) => {
    const s = new Stream();
    s.tag(tag).u32(runs.length);
    for (const run of runs) {
      s.u32(run.fcStart).u32(run.fcEnd).u32(run.istd).digestOf(run.grpprl);
      prlList(s, run.grpprl);
    }
    return s;
  };
  const fkpDigest = (chpx, papx) => {
    const s = new Stream();
    const c = runsStream(chpx, 'c').finish().short;
    const p = runsStream(papx, 'p').finish().short;
    s.str(c).str(p).u32(chpx.length).u32(papx.length);
    return summary(s);
  };

  const vNum = (s, v) => { if (v === undefined) s.tag('-'); else s.tag('n').f64(v); };
  const vBool = (s, v) => { if (v === undefined) s.tag('-'); else s.tag('b').bool(v); };
  const vBrc = (s, b) => {
    if (!b) s.tag('-');
    else s.tag('B').f64(b.color).f64(b.width).f64(b.type).f64(b.space).bool(b.shadow).bool(b.frame);
  };
  const vShd = (s, h) => {
    if (!h) s.tag('-');
    else s.tag('S').f64(h.fore).f64(h.back).f64(h.pat);
  };
  const vWidth = (s, w) => { if (!w) s.tag('-'); else s.tag('W').f64(w.fts).f64(w.w); };
  const vShds = (s, list) => {
    if (!list) { s.tag('-'); return; }
    s.tag('a').u32(list.length);
    for (let i = 0; i < list.length; i++) vShd(s, list[i]);
  };
  const vShorts = (s, v) => {
    s.tag('a').u32(v.length);
    for (let i = 0; i < v.length; i++) vNum(s, v[i]);
  };
  /** A field of Chp/Pap. */
  const vField = (s, v) => {
    if (v === undefined) { s.tag('-'); return; }
    if (typeof v === 'boolean') s.tag('b').bool(v);
    else if (typeof v === 'number') s.tag('n').f64(v);
    else if (v instanceof Uint8Array) s.tag('y').bytes(v);
    else if (Array.isArray(v)) {
      s.tag('a').u32(v.length);
      for (const x of v) {
        if (typeof x === 'number') s.tag('n').f64(x);
        else s.tag('T').f64(x.pos).f64(x.jc).f64(x.tlc);
      }
    } else if ('ftc' in v) s.tag('Y').f64(v.ftc).f64(v.xchar);
    else if ('mult' in v) s.tag('L').f64(v.line).bool(v.mult);
    else if ('fore' in v) vShd(s, v);
    else vBrc(s, v);
  };
  const chpDigest = (s, chp, touched) => {
    s.tag('C');
    for (const k of CHP_LIST) vField(s, chp[k]);
    s.u32(touched.size);
    for (const k of touched) s.str(k);
  };
  const papDigest = (s, pap, touched, istds) => {
    s.tag('P').f64(pap.istd);
    for (const k of PAP_LIST) vField(s, pap[k]);
    s.u32(touched.size);
    for (const k of touched) s.str(k);
    s.u32(istds.length);
    for (const v of istds) s.f64(v);
  };
  const tcDigest = (s, tc) => {
    s.tag('c');
    for (const v of [tc.horzMerge, tc.vertMerge, tc.vertAlign, tc.textFlow, tc.ftsWidth, tc.wWidth]) s.f64(v);
    for (const b of [tc.fitText, tc.noWrap, tc.hideMark]) s.bool(b);
    for (const b of [tc.brcTop, tc.brcLeft, tc.brcBottom, tc.brcRight]) vBrc(s, b);
    for (const k of ['top', 'left', 'bottom', 'right']) s.bool(tc.brcSet[k]);
    vShd(s, tc.shd);
    for (const k of ['top', 'left', 'bottom', 'right']) vNum(s, tc.mar[k]);
  };
  const tapDigest = (s, tap) => {
    s.tag('A');
    vNum(s, tap.jc); vNum(s, tap.dxaLeft); vNum(s, tap.dxaGapHalf); vNum(s, tap.rowHeight);
    vBool(s, tap.cantSplit); vBool(s, tap.header);
    const b = tap.borders;
    for (const x of [b.top, b.left, b.bottom, b.right, b.insideH, b.insideV]) vBrc(s, x);
    s.bool(tap.bordersSet);
    s.u32(tap.centers.length);
    for (const c of tap.centers) s.f64(c);
    s.u32(tap.tcs.length);
    for (const tc of tap.tcs) tcDigest(s, tc);
    for (const w of [tap.tableWidth, tap.widthBefore, tap.widthAfter, tap.widthIndent]) vWidth(s, w);
    vBool(s, tap.autofit);
    for (const k of ['top', 'left', 'bottom', 'right']) vNum(s, tap.padding[k]);
    vNum(s, tap.spacing);
    vNum(s, tap.istd);
    if (!tap.tlp) s.tag('-'); else s.tag('P').f64(tap.tlp.itl).f64(tap.tlp.grfatl);
    vShd(s, tap.shdTable);
    vBool(s, tap.bidi);
    vNum(s, tap.pc);
    for (const v of [tap.dxaAbs, tap.dyaAbs, tap.dxaFromText, tap.dyaFromText, tap.dxaFromTextRight, tap.dyaFromTextBottom]) vNum(s, v);
    vBool(s, tap.noAllowOverlap);
    vBool(s, tap.insertBuilt);
    vShds(s, tap.shdRaw); vShds(s, tap.shdDef); vShds(s, tap.shd80);
  };
  const sepDigest = (s, e) => {
    s.tag('E');
    const n = (v) => { s.tag('n').f64(v); };
    const b = (v) => { s.tag('b').bool(v); };
    n(e.bkc); b(e.titlePage); n(e.ccolM1); n(e.dxaColumns); b(e.evenlySpaced);
    vShorts(s, e.colWidths); vShorts(s, e.colSpacings);
    b(e.lBetween);
    for (const v of [e.xaPage, e.yaPage, e.orientation, e.dxaLeft, e.dxaRight, e.dyaTop, e.dyaBottom]) n(v);
    for (const v of [e.dzaGutter, e.dyaHdrTop, e.dyaHdrBottom, e.nfcPgn]) n(v);
    b(e.pgnRestart);
    for (const v of [e.pgnStart, e.vjc, e.lnnMod, e.dxaLnn, e.lnc, e.lnnMin]) n(v);
    for (const x of [e.brcTop, e.brcLeft, e.brcBottom, e.brcRight]) vBrc(s, x);
    n(e.pgbProp); n(e.clm);
    vNum(s, e.dyaLinePitch);
    n(e.dxtCharSpace); n(e.textFlow); b(e.bidi); b(e.rtlGutter);
    for (const v of [e.fpc, e.rncFtn, e.nFtn, e.nfcFtnRef, e.rncEdn, e.nEdn, e.nfcEdnRef]) vNum(s, v);
    b(e.fEndnote);
    n(e.dmBinFirst); n(e.dmBinOther);
    vNum(s, e.dmPaperReq);
    vBool(s, e.protectedSec);
    vNum(s, e.cnsPgn); vNum(s, e.headingPgn);
  };

  // ───────────── the generated inputs' context ─────────────
  const paraChp = () => ({ hps: 24, bold: true, ico: 2, ftcAscii: 3, istd: 5 });
  const charStyle = (istd) => {
    const c = paraChp();
    c.istd = istd;
    if (istd & 1) c.italic = true; else c.hps = 30 + (istd % 7);
    return c;
  };
  const tab = (pos, jc, tlc) => ({ pos, jc, tlc });
  const initialPap = (i) => {
    if (i === 0) return { istd: 0 };
    if (i === 1) return { istd: 3, dxaLeft: 100, tabs: [tab(720, 0, 0), tab(1440, 1, 2)], tabsCleared: [], outLvl: 2, widowControl: true, jc: 1 };
    return { istd: 12, outLvl: 5, tabs: [tab(360, 3, 1)], tabsCleared: [100] };
  };
  const GEN_FONTS = [['Arial', 0], ['Times New Roman', 128], ['Calibri', 134], ['Ágata & <Co>', 238], ['MS Mincho', 136]]
    .map(([name, charset]) => ({ name, altName: undefined, prq: 0, trueType: false, ff: 0, weight: 0, charset, panose: new Uint8Array(0), sig: [0, 0, 0, 0, 0, 0] }));
  const genStyleId = (istd) => (istd % 4 !== 3 ? `Style${istd}` : undefined);
  const genNumId = (ilfo) => (ilfo % 5 === 0 ? 0 : ilfo + 100);

  const PROP_FACETS = ['chp', 'chp2', 'pap', 'pap2', 'pap3', 'tap', 'sep', 'rpr', 'ppr'];
  const newStreams = () => PROP_FACETS.map(() => new Stream());

  /** The appliers over one grpprl into the rolling streams, and the rPr/pPr content of the emitters. */
  const applyProps = (st, grpprl) => {
    const ctx = { fonts: GEN_FONTS, styleId: genStyleId };
    const chp1 = {}; const t1 = new Set();
    {
      const cctx = { base: {} };
      for (const p of prls(grpprl)) applyChp(chp1, p, cctx, t1);
      chpDigest(st[0], chp1, t1);
    }
    const chp2 = paraChp(); const t2 = new Set();
    {
      const cctx = { base: paraChp(), applyCharStyle: charStyle, paraStyle: paraChp() };
      for (const p of prls(grpprl)) applyChp(chp2, p, cctx, t2);
      chpDigest(st[1], chp2, t2);
    }
    const paps = [];
    for (let i = 0; i < 3; i++) {
      const pap = initialPap(i);
      const base = initialPap(i);
      const touched = new Set();
      const istds = [];
      const pctx = { onIstd: (v) => istds.push(v) };
      for (const p of prls(grpprl)) applyPap(pap, p, touched, pctx);
      pruneNoneBorders(pap, base, touched);
      papDigest(st[2 + i], pap, touched, istds);
      paps.push([pap, touched, base]);
    }
    {
      const tap = newTap();
      for (const p of prls(grpprl)) applyTap(tap, p);
      finishTap(tap);
      tapDigest(st[5], tap);
    }
    {
      const sep = defaultSep();
      for (const p of prls(grpprl)) applySep(sep, p);
      sepDigest(st[6], sep);
    }
    st[7].tag('R').bytes(utf16(rPrInner(chp1, t1, ctx, true))).bytes(utf16(rPrInner(chp1, t1, ctx, false)));
    st[7].bytes(utf16(rPrInner(chp2, t2, ctx, true)));
    paps.forEach(([pap, touched, base], i) => {
      const opts = i === 0 ? {}
        : i === 1 ? { baseTabs: base.tabs, withStyle: false }
          : { baseTabs: [], numId: genNumId, rPr: '<w:b/>', sectPr: '<w:sectPr/>', withStyle: true };
      st[8].tag('P').bytes(utf16(pPrInner(pap, touched, ctx, opts)));
    });
  };
  const streamFacets = (out, st, count) => {
    PROP_FACETS.forEach((name, i) => { out[name] = `${count}:${st[i].finish().short}`; });
  };

  // ───────────── F3: style sheets and lists ─────────────
  const vOpt = (s, v, f) => { if (v === undefined) s.tag('-'); else { s.tag('+'); f(s, v); } };
  const stshTables = (s, sheet) => {
    const f = sheet.stshif;
    s.tag('H');
    for (const v of [f.cstd, f.cbSTDBaseInFile, f.stiMaxWhenSaved, f.ftcAsci, f.ftcFE, f.ftcOther, f.ftcBi]) s.f64(v);
    s.bool(f.stdStylenamesWritten);
    s.u32(sheet.styles.length);
    for (let i = 0; i < sheet.styles.length; i++) {
      const st = sheet.styles[i];
      if (!st) { s.tag('-'); continue; }
      s.tag('D');
      for (const v of [st.istd, st.sti, st.stk, st.istdBase, st.istdNext]) s.f64(v);
      vOpt(s, st.istdLink, (q, v) => q.f64(v));
      s.bytes(utf16(st.name));
      vOpt(s, st.aliases, (q, v) => q.bytes(utf16(v)));
      for (const b of [st.autoRedef, st.hidden, st.semiHidden, st.locked, st.unhideWhenUsed, st.qFormat]) s.bool(b);
      vOpt(s, st.priority, (q, v) => q.f64(v));
      for (const u of [st.papx, st.chpx, st.tapx]) vOpt(s, u, (q, v) => q.bytes(v));
    }
  };
  const resolvedDigest = (s, res) => {
    papDigest(s, res.pap, res.ownPap, []);
    chpDigest(s, res.chp, res.ownChp);
  };
  const stshFacets = (out, sheet) => {
    const n = sheet.styles.length;
    const ids = [...Array(n).keys(), n, 0x0fff, 0xffff];
    const a = new Stream();
    stshTables(a, sheet);
    const ra = new StyleResolver(sheet);
    for (const i of ids) resolvedDigest(a, ra.resolve(i));
    const b = new Stream();
    const rb = new StyleResolver(sheet);
    for (const i of [...ids].reverse()) { b.f64(i); resolvedDigest(b, rb.resolve(i)); }
    const c = new Stream();
    const rc = new StyleResolver(sheet);
    c.tag('Q');
    chpDigest(c, rc.rootChp, new Set());
    for (const i of ids) {
      const { chp, own } = rc.resolveChar(i);
      chpDigest(c, chp, own);
      chpDigest(c, rc.applyCharStyle(i, { ...rc.rootChp }), new Set());
      chpDigest(c, rc.applyCharStyle(i, paraChp()), new Set());
    }
    out.stsh = summary(a); out.stsh2 = summary(b); out.stsh3 = summary(c);
  };
  const lvlDigest = (s, l) => {
    s.tag('V');
    for (const v of [l.start, l.nfc, l.jc, l.follow, l.ilvlRestartLim]) s.f64(v);
    for (const b of [l.legal, l.noRestart, l.tentative]) s.bool(b);
    s.bytes(utf16(l.xst));
    s.u32(l.rgbxchNums.length);
    for (const v of l.rgbxchNums) s.f64(v);
    s.bytes(l.grpprlPapx).bytes(l.grpprlChpx);
  };
  const listsDigest = (lists) => {
    const s = new Stream();
    s.tag('L').u32(lists.lsts.length);
    for (const l of lists.lsts) {
      s.f64(l.lsid).f64(l.tplc).bool(l.simple).bool(l.hybrid);
      for (const v of l.rgistdPara) s.f64(v);
      s.u32(l.levels.length);
      for (const lv of l.levels) lvlDigest(s, lv);
    }
    s.u32(lists.lfos.length);
    for (const f of lists.lfos) {
      s.f64(f.lsid).u32(f.overrides.length);
      for (const o of f.overrides) {
        s.f64(o.ilvl);
        vOpt(s, o.startAt, (q, v) => q.f64(v));
        vOpt(s, o.lvl, (q, v) => lvlDigest(q, v));
      }
    }
    return summary(s);
  };
  const xmlDigest = (text) => {
    const s = new Stream();
    s.tag('X').bytes(utf16(text ?? '')).bool(text === undefined);
    return summary(s);
  };
  const idsDigest = (ids) => {
    const s = new Stream();
    s.tag('I').u32(ids.size);
    for (const [istd, id] of ids) s.f64(istd).str(id);
    return summary(s);
  };
  const dopDigest = (d) => {
    const s = new Stream();
    s.tag('O');
    for (const k of DOP_LIST) {
      const v = d[k];
      if (v === undefined) s.tag('-');
      else if (typeof v === 'boolean') s.tag('b').bool(v);
      else s.tag('n').f64(v);
    }
    const g = d.grid;
    if (!g) s.tag('-');
    else {
      s.tag('G');
      for (const v of [g.xa, g.ya, g.dxa, g.dya, g.dyDisplay, g.dxDisplay]) s.f64(v);
      s.bool(g.followMargins);
    }
    return summary(s);
  };
  const fakeFib = (pairs) => ({ fcLcb: (i) => pairs[i] ?? { fc: 0, lcb: 0 } });

  // The F5 emitters over a fresh resolver (a stub of the reader members they use).
  const partFacets = (out, rd) => {
    const ids = styleIdMap(rd.stsh.styles);
    const ctx = { fonts: rd.fonts, styleId: (i) => ids.get(i) };
    const lfoCount = rd.lists.lfos.length;
    const numIdOf = (n) => (n >= 1 && n <= lfoCount ? n : 0);
    const stub = { stsh: rd.stsh, styles: new StyleResolver(rd.stsh), fonts: rd.fonts, lists: rd.lists, dop: rd.dop, fib: rd.fib };
    out['xml.styles'] = xmlDigest(buildStyles(stub, ctx, ids, numIdOf));
    out.styleids = idsDigest(ids);
    out['xml.numbering'] = xmlDigest(buildNumbering(stub, ctx));
    out['xml.settings'] = xmlDigest(buildSettings(stub, { footnotes: false, endnotes: false }));
    out['xml.settings2'] = xmlDigest(buildSettings(stub, { footnotes: true, endnotes: true }));
    out.dop = dopDigest(readDop(rd.dop, rd.fib.nFib));
  };

  // ───────────── F4: the reader assembly ─────────────
  const vOptI64 = (s, v) => { if (v === undefined) s.tag('-'); else s.tag('+').i64(v); };
  const blipDigest = (s, b) => {
    if (!b) s.tag('-'); else s.tag('+').str(b.ext).u64(b.bytes.length).digestOf(b.bytes);
  };
  const fxDigest = (s, f) => {
    if (!f) { s.tag('-'); return; }
    s.tag('+');
    for (const v of [f.bright, f.contrast]) { if (v === undefined) s.tag('-'); else s.tag('+').f64(v); }
    for (const v of [f.gray, f.biLevel]) { if (v) s.tag('+').bool(true); else s.tag('-'); }
  };
  const picDigest = (s, p) => {
    if (!p) { s.tag('-'); return; }
    s.tag('+').f64(p.cx).f64(p.cy);
    if (p.name === undefined) s.tag('-'); else s.tag('+').bytes(utf16(p.name));
    blipDigest(s, p.blip);
    if (!p.crop) s.tag('-'); else s.tag('+').f64(p.crop.l).f64(p.crop.t).f64(p.crop.r).f64(p.crop.b);
    if (!p.hr) s.tag('-');
    else {
      s.tag('+').str(p.hr.align);
      if (p.hr.pct === undefined) s.tag('-'); else s.tag('+').f64(p.hr.pct);
      if (p.hr.fill === undefined) s.tag('-'); else s.tag('+').f64(p.hr.fill);
      s.bool(p.hr.noshade);
    }
    fxDigest(s, p.fx);
  };
  const shapeDigest = (s, rd, shape) => {
    s.f64(shape.spid).f64(shape.type).f64(shape.flags).bool(rd.masters.has(shape.spid));
    if (shape.z === undefined) s.tag('-'); else s.tag('+').f64(shape.z);
    blipDigest(s, shapeBlip(rd, shape));
    fxDigest(s, picFxOf(shape));
    s.u32(shape.children ? shape.children.length : 0xffffffff);
    for (const c of shape.children ?? []) shapeDigest(s, rd, c);
  };
  const picLocations = (rd) => {
    const locs = [];
    for (const run of rd.chpx) {
      for (const p of prls(run.grpprl)) {
        if (p.sprm !== 0x6a03) continue;
        const v = p.r.i32(p.op);
        if (!locs.includes(v)) locs.push(v);
      }
      if (locs.length >= 64) break;
    }
    return locs;
  };
  const readerFacets = (out, bytes) => {
    const rd = new DocReader(bytes);
    let s = new Stream();
    s.tag('S').u32(rd.sections.length);
    for (const sec of rd.sections) { s.i64(sec.cpStart).i64(sec.cpEnd); sepDigest(s, sec.sep); }
    out.sections = summary(s);

    s = new Stream();
    s.tag('N');
    for (const kind of ['ftn', 'edn']) {
      const l = rd.notes(kind);
      s.u32(l.length);
      for (const n of l) s.i64(n.refCp).bool(n.auto).i64(n.cpStart).i64(n.cpEnd);
    }
    out.notes = summary(s);

    s = new Stream();
    let l = rd.comments();
    s.tag('M').u32(l.length);
    for (const c of l) {
      s.i64(c.refCp).i64(c.cpStart).i64(c.cpEnd).bytes(utf16(c.author)).bytes(utf16(c.initials));
      vOptI64(s, c.rangeStart); vOptI64(s, c.rangeEnd);
    }
    out.comments = summary(s);

    s = new Stream();
    l = rd.bookmarks();
    s.tag('K').u32(l.length);
    for (const b of l) s.bytes(utf16(b.name)).i64(b.cpStart).i64(b.cpEnd);
    const authors = rd.revisionAuthors();
    s.u32(authors.length);
    for (const a of authors) s.bytes(utf16(a));
    out.bookmarks = summary(s);

    s = new Stream();
    s.tag('U');
    for (const [a, b] of [rd.sub.main, rd.sub.ftn, rd.sub.hdd, rd.sub.atn, rd.sub.edn, rd.sub.txbx, rd.sub.hdrTxbx]) s.i64(a).i64(b);
    const h = rd.headerStories();
    s.u32(h.length);
    for (const x of h) { if (!x) s.tag('-'); else s.tag('+').i64(x[0]).i64(x[1]); }
    out.subs = summary(s);

    s = new Stream();
    s.tag('F');
    for (const hdr of [false, true]) {
      l = rd.fspas(hdr);
      s.u32(l.length);
      for (const f of l) {
        s.i64(f.cp).i64(f.spid).i64(f.xaLeft).i64(f.yaTop).i64(f.xaRight).i64(f.yaBottom)
          .u32(f.bx).u32(f.by).u32(f.wr).u32(f.wrk).bool(f.belowText).bool(f.anchorLock);
      }
    }
    out.fspas = summary(s);

    s = new Stream();
    s.tag('X');
    for (const hdr of [false, true]) {
      l = rd.textboxes(hdr);
      s.u32(l.length);
      for (const t of l) s.i64(t.spid).i64(t.cpStart).i64(t.cpEnd);
    }
    out.textboxes = summary(s);

    const locs = picLocations(rd);
    s = new Stream();
    s.tag('D').u32(locs.length);
    for (const loc of locs) {
      s.i64(loc);
      const x = ffDataXml(rd.data, loc);
      if (x === undefined) s.tag('-'); else s.tag('+').bytes(utf16(x));
    }
    out.ffdata = summary(s);

    s = new Stream();
    s.tag('I').u32(locs.length);
    for (const loc of locs) { s.i64(loc); picDigest(s, readInlinePicture(rd, loc)); }
    out.picf = summary(s);

    s = new Stream();
    s.tag('H');
    for (const hdr of [false, true]) {
      l = rd.fspas(hdr);
      s.u32(l.length);
      for (const f of l) {
        const sh = rd.shape(f.spid, hdr);
        if (!sh) s.tag('-'); else { s.tag('+'); shapeDigest(s, rd, sh); }
      }
    }
    const bg = rd.backgroundShape();
    if (!bg) s.tag('-'); else s.tag('+').f64(bg.spid);
    for (const pib of [0, 1, 2, 3, 0xffff]) {
      const b = rd.bse(pib);
      if (!b) s.tag('-'); else s.tag('+').f64(b.btWin32).f64(b.size).f64(b.foDelay).bool(b.embedded !== undefined);
    }
    out.shapes = summary(s);
  };

  /** Moves `rpr` after `props` in a run, a text box's `blocks` before `paras`, a cell's `blocks` after its
   *  other keys: the order the Rust model's digest writes (the key-order stream of the model facet). */
  const normalize = (v, seen = new Set()) => {
    if (v === null || typeof v !== 'object' || v instanceof Uint8Array || seen.has(v)) return;
    seen.add(v);
    if (Array.isArray(v)) { for (const x of v) normalize(x, seen); return; }
    if (v instanceof Map) { for (const x of v.values()) normalize(x, seen); return; }
    if (typeof v.text === 'string' && v.props !== undefined) {
      const copy = { ...v };
      const rest = Object.keys(copy).filter((k) => k !== 'text' && k !== 'props' && k !== 'rpr');
      const hasRpr = Object.hasOwn(copy, 'rpr');
      for (const k of Object.keys(v)) delete v[k];
      v.text = copy.text; v.props = copy.props;
      if (hasRpr) v.rpr = copy.rpr;
      for (const k of rest) v[k] = copy[k];
    }
    if (Array.isArray(v.paras) && typeof v.widthTwips === 'number') {
      const { paras, blocks } = v;
      const hasBlocks = Object.hasOwn(v, 'blocks');
      delete v.paras;
      delete v.blocks;
      if (hasBlocks) v.blocks = blocks;
      v.paras = paras;
    }
    if (Array.isArray(v.paragraphs) && Array.isArray(v.tables) && typeof v.gridSpan === 'number' && Object.hasOwn(v, 'blocks')) {
      const blocks = v.blocks;
      delete v.blocks;
      v.blocks = blocks;
    }
    for (const k of Object.keys(v)) normalize(v[k], seen);
  };

  // ───────────── parts and model of the converted docx ─────────────
  const partsAndModel = (zip) => {
    const files = fflate.unzipSync(zip);
    const s = new Stream();
    const names = Object.keys(files);
    s.tag('Z').u32(names.length);
    for (const n of names) s.str(n).digestOf(files[n]);
    const parts = summary(s);
    let model;
    try {
      const full = readFullDocument(DocxPackage.load(zip));
      normalize(full);
      const main = new Stream();
      const order = new Stream();
      new R2Canon(main, { order }).document(full);
      model = `${main.items}:${main.finish().short}/${order.items}:${order.finish().short}`;
    } catch { model = 'err'; }
    return { parts, model };
  };

  // ───────────── one input ─────────────
  const facetsOfFile = (bytes) => {
    const out = {};
    out.input = `${bytes.length}:${sha(bytes).slice(0, 16)}`;
    out.isword = isWordBinary(bytes) ? '1' : '0';
    let rd;
    try { rd = new DocReader(bytes); out.rd = 'ok'; } catch (e) { out.rd = classOf(e); }
    try {
      const zip = convertDocToDocx(bytes);
      out.convert = 'ok';
      Object.assign(out, partsAndModel(zip));
    } catch (e) { out.convert = classOf(e); }
    if (out.rd !== 'ok') return out;

    out.fib = fibDigest(rd.fib);
    const s = new Stream();
    s.tag('T').u32(rd.pt.pieces.length);
    for (const p of rd.pt.pieces) s.u32(p.cpStart).u32(p.cpEnd).f64(p.fc).bool(p.compressed).u32(p.prm);
    s.u32(rd.pt.prcs.length);
    for (const p of rd.pt.prcs) s.digestOf(p);
    s.bytes(utf16(rd.text));
    out.text = summary(s);
    out.fkp = fkpDigest(rd.chpx, rd.papx);
    const f = new Stream();
    f.tag('N').u32(rd.fonts.length);
    for (const x of rd.fonts) {
      f.bytes(utf16(x.name)).bool(x.altName !== undefined);
      if (x.altName !== undefined) f.bytes(utf16(x.altName));
      f.u32(x.prq).bool(x.trueType).u32(x.ff).i64(x.weight).u32(x.charset).bytes(x.panose);
      for (const v of x.sig) f.u32(v);
    }
    out.fonts = summary(f);

    // the appliers over every CHPX/PAPX grpprl and every section's SEPX
    const st = newStreams();
    let n = 0;
    for (const run of [...rd.chpx, ...rd.papx]) { applyProps(st, run.grpprl); n++; }
    const sed = rd.fib.fcLcb(FC.PlcfSed);
    const plc = readPlc(rd.table, sed.fc, sed.lcb, 12);
    if (plc) {
      const w = new ByteReader(rd.word);
      for (let i = 0; i < plc.n; i++) {
        const fcSepx = plc.r.u32(plc.dataAt(i) + 2);
        const sep = defaultSep();
        if (fcSepx !== 0xffffffff && fcSepx + 2 <= rd.word.length) {
          const cb = w.i16(fcSepx);
          for (const q of prls(w.sub(fcSepx + 2, fcSepx + 2 + Math.max(0, cb)))) applySep(sep, q);
        }
        sepDigest(st[6], sep);
        n++;
      }
    }
    streamFacets(out, st, n);

    stshFacets(out, rd.stsh);
    out.lists = listsDigest(rd.lists);
    partFacets(out, rd);
    readerFacets(out, bytes);
    return out;
  };

  const facetsOfGrpprl = (hex) => {
    const g = unhex(hex);
    const out = {};
    const s = new Stream();
    prlList(s, g);
    out.sprm = summary(s);
    const st = newStreams();
    applyProps(st, g);
    streamFacets(out, st, 1);
    return out;
  };
  const facetsOfStsh = (hex) => {
    const table = unhex(hex);
    const sheet = readStsh(fakeFib({ [FC.Stshf]: { fc: 0, lcb: table.length } }), table);
    const ids = styleIdMap(sheet.styles);
    const stub = { stsh: sheet, styles: new StyleResolver(sheet), fonts: GEN_FONTS };
    const ctx = { fonts: GEN_FONTS, styleId: (i) => ids.get(i) };
    const out = {};
    stshFacets(out, sheet);
    out['xml.styles'] = xmlDigest(buildStyles(stub, ctx, ids, (n) => (n >= 1 && n <= 3 ? n : 0)));
    out.styleids = idsDigest(ids);
    return out;
  };
  const pairOf = (v) => { const [a, b] = v.split(':'); return { fc: Number(a), lcb: Number(b) }; };
  const facetsOfLists = (hex, lst, lfo) => {
    const table = unhex(hex);
    const lists = readLists(fakeFib({ [FC.PlfLst]: pairOf(lst), [FC.PlfLfo]: pairOf(lfo) }), table);
    const ctx = { fonts: GEN_FONTS, styleId: (i) => (i % 3 !== 0 ? `S${i}` : undefined) };
    return { lists: listsDigest(lists), 'xml.numbering': xmlDigest(buildNumbering({ lists }, ctx)) };
  };
  const facetsOfDop = (hex, nfib, ftcfe) => {
    const bytes = unhex(hex);
    const stub = { dop: bytes, fib: { nFib: Number(nfib) }, fonts: GEN_FONTS, stsh: { stshif: { ftcFE: Number(ftcfe) } } };
    return {
      'xml.settings': xmlDigest(buildSettings(stub, { footnotes: false, endnotes: false })),
      'xml.settings2': xmlDigest(buildSettings(stub, { footnotes: true, endnotes: true })),
      dop: dopDigest(readDop(bytes, Number(nfib))),
    };
  };

  /** The facets of a record, in the order they are written. */
  const compute = (rec) => {
    if (rec.key.startsWith('doc/') || rec.key.startsWith('mut/')) {
      const out = facetsOfFile(inputBytes(rec));
      if (rec.key.startsWith('mut/')) return { edits: get(rec, 'edits'), cut: get(rec, 'cut'), ...out };
      return out;
    }
    const hex = get(rec, 'hex');
    if (rec.key.startsWith('gen/grpprl/')) return { hex, ...facetsOfGrpprl(hex) };
    if (rec.key.startsWith('gen/stsh/')) return { hex, ...facetsOfStsh(hex) };
    if (rec.key.startsWith('gen/lists/')) {
      const lst = get(rec, 'lst'); const lfo = get(rec, 'lfo');
      return { hex, lst, lfo, ...facetsOfLists(hex, lst, lfo) };
    }
    if (rec.key.startsWith('gen/dop/')) {
      const nfib = get(rec, 'nfib'); const ftcfe = get(rec, 'ftcfe');
      return { hex, nfib, ftcfe, ...facetsOfDop(hex, nfib, ftcfe) };
    }
    throw new Error(`unknown record ${rec.key}`);
  };

  const { records } = parseCapture(workerData.inputs);
  parentPort.on('message', (i) => {
    try { parentPort.postMessage({ i, facets: Object.entries(compute(records[i])) }); } catch (e) {
      parentPort.postMessage({ i, error: String(e?.stack ?? e) });
    }
  });
  parentPort.postMessage({ ready: true });
}

/** The file of a `doc/` or `mut/` record: the fixture with the edits and the cut applied. */
function inputBytes(rec) {
  const name = rec.key.split('/')[1];
  const b = new Uint8Array(readFileSync(join(FIXTURES, `${name}.doc`)));
  const edits = get(rec, 'edits');
  if (edits !== undefined && edits !== '-') {
    for (const e of edits.split(',')) {
      const [p, v] = e.split(':').map(Number);
      if (p < b.length) b[p] = v;
    }
  }
  const cut = get(rec, 'cut');
  return cut !== undefined && cut !== '-' ? b.subarray(0, Number(cut)) : b;
}

// ═════════════════════════════════════════ main ═════════════════════════════════════════

async function main() {
  const t0 = Date.now();
  const { records, meta } = parseCapture(inputsPath);
  const out = arg('--out', DEFAULT_CAPTURE);
  const cap = new CaptureWriter([HEADER]);
  cap.meta('dop-keys', DOP_KEYS);
  cap.meta('chp-keys', CHP_KEYS);
  cap.meta('pap-keys', PAP_KEYS);

  let worker = null;
  let pending = null;
  const spawn = () => new Promise((resolve, reject) => {
    let up = false;
    const w = new Worker(fileURLToPath(import.meta.url), {
      workerData: { inputs: inputsPath },
      resourceLimits: { maxOldGenerationSizeMb: 1536 },
    });
    w.on('message', (m) => {
      if (m.ready) { up = true; return resolve(w); }
      const p = pending; pending = null;
      p?.resolve(m);
    });
    const dead = (why) => { if (!up) return reject(new Error(`TS worker failed to start: ${why}`)); const p = pending; pending = null; p?.resolve({ dead: why }); };
    w.on('error', (e) => dead(up ? String(e?.code ?? e) : String(e?.stack ?? e)));
    w.on('exit', () => dead('exit'));
  });
  const ask = (i) => new Promise((resolve) => { pending = { resolve }; worker.postMessage(i); });

  worker = await spawn();
  const results = [];
  let leftOut = 0;
  for (let i = 0; i < records.length; i++) {
    const r = await ask(i);
    if (r.dead) {
      leftOut++;
      process.stderr.write(`left out (TS worker died: ${r.dead}): ${records[i].key}\n`);
      worker = await spawn();
      continue;
    }
    if (r.error) throw new Error(`${records[i].key}: ${r.error}`);
    results.push([records[i].key, r.facets]);
    if (results.length % 500 === 0) process.stderr.write(`${results.length}/${records.length}\n`);
  }
  await worker.terminate();

  const count = (p) => results.filter(([k]) => k.startsWith(p)).length;
  const prev = Number(/left-out-ts-memory=(\d+)/.exec(meta.get('corpus') ?? '')?.[1] ?? 0);
  cap.meta('corpus', `fixtures=${count('doc/')} mutated=${count('mut/')} gen=${count('gen/grpprl/')}+${count('gen/stsh/')}+${count('gen/lists/')}+${count('gen/dop/')} left-out-ts-memory=${prev + leftOut}`);
  for (const [key, facets] of results) for (const [f, v] of facets) cap.set(key, f, v);
  cap.write(out);
  process.stderr.write(`wrote ${out}: ${results.length} records in ${((Date.now() - t0) / 1000).toFixed(1)} s\n`);
}

if (isMainThread) await main(); else await runWorker();
