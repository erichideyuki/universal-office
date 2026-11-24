// Parity capture of the `.xls` (BIFF8) converter (frontier C2, record core/docs/modules/uo-xlsx.md "C2 — as built"): the
// frozen TS `xlsToXlsxPackage` (sheet-core/src/xls/*) run headless over a folder of `.xls` files; every part of the
// converted package is digested byte for byte, then the C1a facets (model, store, file order, row attributes) of the
// package as sheet-core reads it. The Rust side (`tests/xls_parity.rs`) converts the same files with `uo_xlsx::xls` and
// must give the same digests.
//
//   node core/crates/uo-xlsx/js/capture-xls.mjs --dir <folder with .xls> [--out <file>] [--only <substring>] [--no-side]
//   committed subset: --dir core/crates/uo-xlsx/tests/captures/workbooks/xls  -> tests/captures/xls.capture (+ .side.gz)
//   local (more files): --dir core/target/sheet-cap/xls-out/xls1 --out core/target/sheet-cap/xls-xls1.capture
//
// Facets of a record (`xls/<file>`), `len:sha256[0..32]` digests unless said otherwise:
//   input    `len:sha256[0..16]` of the file
//   read     ok | err:<class>:<message> (XlsEncryptedError, XlsFormatError; the class is compared, not the text)
//   parts    `<count>:<digest of the "name len sha" lines in package order>`;  p<i>  digest of part i's bytes
//   names    the part names in order, joined with `|`
//   wb styles theme sst ws<i> st<i> fo<i> rw<i> sheets   as capture-sheet.mjs (the converted package through readWorkbook…)
// The sidecar (`<out>.side.gz`, lines `<key>\t<facet>\t<text>`) keeps the text of every text part (facet `x:<name>`)
// and the model texts, for the Rust test to show the first differing place.
process.env.TZ = 'UTC';
const { readFileSync, readdirSync, mkdirSync, createWriteStream } = await import('node:fs');
const { join, dirname } = await import('node:path');
const { createHash } = await import('node:crypto');
const { createGzip } = await import('node:zlib');
const { CaptureWriter } = await import('../../uo-testkit/js/testkit.mjs');
const { canonJson } = await import('../../uo-pptx/js/lib/canon-json.mjs');
const C = await import('./lib/common.mjs');
const { loadHarness } = await import('./harness/build-harness.mjs');

const a = C.args();
const root = C.tsRoot(a);
const only = a.opt('--only');
const dir = a.opt('--dir');
if (!dir) { console.error('usage: capture-xls.mjs --dir <folder> [--out file] [--only s] [--no-side]'); process.exit(2); }
const SIDE = !a.flag('--no-side');

const h = await loadHarness(root);
const SC = h.sheetCore;

const sha = (s) => createHash('sha256').update(s).digest('hex');
const digB = (b) => `${b.length}:${sha(b).slice(0, 32)}`;
const dig = (s) => digB(Buffer.from(s, 'utf8'));
const listDig = (lines) => `${lines.length}:${sha(Buffer.from(lines.join('\n'), 'utf8')).slice(0, 32)}`;
const TEXT = /\.(xml|rels|vml)$/i;

/** The store facet lines of a worksheet model + the file order + row metadata (capture-sheet.mjs `storeOf`). */
function storeOf(ws) {
  const map = new Map();
  const seq = [];
  let sorted = true, prev = -1;
  const rowMeta = [];
  for (const row of ws.rows) {
    const meta = { customFormat: row.customFormat, customHeight: row.customHeight, hidden: row.hidden, ht: row.height, r: row.r, s: row.style, spans: row.spans, thickBot: row.thickBot, thickTop: row.thickTop };
    if (Object.keys(meta).some((k) => k !== 'r' && meta[k] !== undefined)) rowMeta.push(canonJson(meta));
    for (const c of row.cells) {
      let col;
      try { col = SC.refToRowCol(c.ref).col; } catch { col = -1; }
      const k = row.r * 20000 + col;
      seq.push(`${row.r}:${col}`);
      if (k <= prev) sorted = false;
      prev = k;
      const canonRef = col > 0 ? SC.rowColToRef(col, row.r) : undefined;
      map.set(k, { c: col, cm: c.cm, f: c.formula, fa: c.formulaAttrs, is: c.inlineString, r: row.r, ref: c.ref !== canonRef ? c.ref : undefined, s: c.style, t: c.type, v: c.value, vm: c.vm });
    }
  }
  const keys = [...map.keys()].sort((x, y) => x - y);
  const hh = createHash('sha256');
  keys.forEach((k, i) => { if (i) hh.update('\n'); hh.update(canonJson(map.get(k))); });
  return { digest: `${keys.length}:${hh.digest('hex').slice(0, 32)}`, order: sorted ? 'sorted' : listDig(seq), rowMeta };
}

const w = new CaptureWriter(['`.xls` converter parity (frontier C2): see core/crates/uo-xlsx/js/capture-xls.mjs and core/docs/modules/uo-xlsx.md "C2 — as built"']);
w.meta('ts-root-commit', h.commit);
w.meta('ts-root-state', C.tsClean(root));
w.meta('canon', 'canon-json.mjs of uo-pptx: sorted keys, shortest round-trip numbers, -0 kept, Map as {"$map"}');
const side = [];
const files = readdirSync(dir).filter((x) => /\.xls$/i.test(x)).sort();
let parts = 0, errs = 0;
const t0 = performance.now();
for (const f of files) {
  const key = `xls/${f}`;
  if (only && !key.includes(only)) continue;
  const bytes = new Uint8Array(readFileSync(join(dir, f)));
  const sx = (facet, text) => { if (SIDE) side.push(`${key}\t${facet}\t${text}`); };
  w.set(key, 'input', `${bytes.length}:${sha(bytes).slice(0, 16)}`);
  let pkg;
  try {
    pkg = SC.xlsToXlsxPackage(bytes);
  } catch (e) {
    w.set(key, 'read', `err:${e?.name ?? 'Error'}:${C.oneLine(e?.message ?? e)}`);
    errs++;
    continue;
  }
  w.set(key, 'read', 'ok');
  const names = pkg.partNames();
  const lines = [];
  names.forEach((n, i) => {
    const d = pkg.getPart(n);
    const dg = digB(d);
    lines.push(`${n} ${dg}`);
    w.set(key, `p${i}`, dg);
    if (TEXT.test(n)) sx(`x:${n}`, Buffer.from(d).toString('utf8').replace(/\\/g, '\\\\').replace(/\n/g, '\\n').replace(/\r/g, '\\r').replace(/\t/g, '\\t'));
    parts++;
  });
  w.set(key, 'parts', listDig(lines));
  w.set(key, 'names', names.join('|'));
  // ---- the model facets of the converted package (the C1a read)
  let wb, cx;
  try {
    wb = SC.readWorkbook(pkg);
    cx = SC.colorContext(pkg);
  } catch (e) {
    w.set(key, 'model', `err:${C.oneLine(e?.message ?? e)}`);
    continue;
  }
  const model = (facet, value) => { const t = canonJson(value); w.set(key, facet, dig(t)); sx(facet, t); };
  model('wb', wb);
  model('styles', SC.readStyles(pkg, cx));
  model('theme', cx.theme);
  model('sst', SC.readSharedStrings(pkg, cx));
  let n = 0;
  for (const ref of wb.sheets) {
    if (!ref.target) continue;
    let ws;
    try { ws = SC.readWorksheet(pkg, ref.target, cx); } catch (e) { w.set(key, `ws${n}`, `err:${C.oneLine(e?.message ?? e)}`); n++; continue; }
    const { rows, ...rest } = ws;
    model(`ws${n}`, rest);
    const st = storeOf(ws);
    w.set(key, `st${n}`, st.digest);
    w.set(key, `fo${n}`, st.order);
    w.set(key, `rw${n}`, listDig(st.rowMeta));
    n++;
  }
  w.set(key, 'sheets', n);
}
const out = a.opt('--out', join(C.capturesDir, 'xls.capture'));
mkdirSync(dirname(out), { recursive: true });
w.write(out);
if (SIDE) {
  const gz = createGzip({ level: 9 });
  const done = new Promise((res, rej) => gz.pipe(createWriteStream(out.replace(/\.capture$/, '.side.gz'))).on('finish', res).on('error', rej));
  for (let i = 0; i < side.length; i += 10000) gz.write(side.slice(i, i + 10000).join('\n') + '\n');
  gz.end();
  await done;
}
console.log(`xls: ${files.length} files, ${parts} parts, ${errs} errors in ${((performance.now() - t0) / 1000).toFixed(1)} s -> ${out}`);
