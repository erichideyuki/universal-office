// The non-font facets of the stage-4 parity capture of uo-fonts (imported by capture_fonts.mjs, which
// writes them to the same tests/parity/fonts.cap): odttf keys, style names, language tags, the
// catalog tables, the hyphenation patterns and the hyphens of the oracle words, and the embedding
// plan with a stub resolver. The Rust replay is tests/fonts_parity_small.rs; the streams below and
// the ones there must stay identical.
import { createHash } from 'node:crypto';
import { readFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';
import { Stream, importFrom } from '../../../uo-testkit/js/testkit.mjs';

const here = dirname(fileURLToPath(import.meta.url));
const parity = join(here, '..', 'parity');
const sha = (u8) => createHash('sha256').update(u8).digest('hex').slice(0, 32);
const done = (s) => `${s.items}:${s.finish().short}`;
const fromUnits = (field) => (field === '~' ? undefined : String.fromCharCode(...field.split(',').filter(Boolean).map((h) => parseInt(h, 16))));
const LANGS = ['pt', 'en-us', 'es', 'fr', 'de-1996', 'it', 'nl'];
const VARIANT_OF = { r: 'regular', b: 'bold', i: 'italic', z: 'boldItalic' };

export function writeInfo(s, i) {
  s.tag('I').str(i.panose).u64(i.weight).bool(i.italic).u64(i.fsType);
  for (const v of i.unicodeRange) s.u64(v);
  for (const v of i.codePageRange) s.u64(v);
  s.u64(i.checkSumAdjustment);
  for (const name of [i.names.family, i.names.style, i.names.version, i.names.full]) {
    s.u64(name.length);
    for (let k = 0; k < name.length; k++) s.u32(name.charCodeAt(k));
  }
}

export async function smallFacets(cap, root, corpusDir) {
  const types = await importFrom(root, 'shared/fonts/dist/types.js');
  const ooxml = await importFrom(root, 'shared/fonts/dist/ooxml.js');
  const docxFonts = await importFrom(root, 'write/packages/docx-core/dist/fonts.js');
  const hyphenIndex = await importFrom(root, 'write/packages/docx-layout/dist/typography/hyphen/index.js');
  const { Hyphenator } = await importFrom(root, 'write/packages/docx-layout/dist/typography/hyphen/liang.js');
  const catalog = await importFrom(root, 'shared/fonts/dist/catalog.js');
  const embedding = await importFrom(root, 'shared/fonts/dist/embedding.js');
  const shared = await importFrom(root, 'shared/fonts/dist/sfnt.js');

  // ---- odttf, style, lang (the shared corpus of frontier D, read again here) ----
  const lines = readFileSync(join(parity, 'frontier_d_inputs.txt'), 'utf8').split('\n').filter(Boolean).map((l) => l.split('\t'));
  const data = new Uint8Array(100).map((_, i) => (i * 7 + 3) & 0xff);
  let n = 0;
  for (const [kind, a] of lines) {
    if (kind !== 'odttf') continue;
    const key = fromUnits(a);
    const k = `odttf/${n++}`;
    cap.set(k, 'in', a);
    cap.set(k, 'ob', sha(ooxml.obfuscateOdttf(data, key)));
    const de = docxFonts.deobfuscateFont(data, key);
    cap.set(k, 'de', de === data ? 'same' : sha(de));
  }
  n = 0;
  for (const [kind, a] of lines) {
    if (kind !== 'style') continue;
    const k = `style/${n++}`;
    cap.set(k, 'in', a);
    cap.set(k, 'variant', String(types.variantOfStyle(fromUnits(a))));
  }
  n = 0;
  for (const [kind, a] of lines) {
    if (kind !== 'lang') continue;
    const k = `lang/${n++}`;
    cap.set(k, 'in', a);
    cap.set(k, 'id', String(hyphenIndex.hyphenationPatternId(fromUnits(a))));
  }
  for (const [a, b] of [[false, false], [true, false], [false, true], [true, true]]) {
    cap.set(`variant/${a}${b}`, 'of', types.variantOf(a, b));
  }
  for (const v of types.VARIANTS) cap.set(`variant/${v}`, 'flags', `${types.isBoldVariant(v)},${types.isItalicVariant(v)}`);

  // ---- loadHyphenation: which pattern sets a list of languages loads (a fresh module instance per list) ----
  {
    const url = pathToFileURL(join(root, 'write/packages/docx-layout/dist/typography/hyphen/index.js')).href;
    const tags = lines.filter((l) => l[0] === 'lang').map((l) => fromUnits(l[1]));
    let k = 0;
    for (let from = 0; from < tags.length; k++) {
      const size = 1 + (k % 4);
      const list = tags.slice(from, from + size);
      from += size;
      const mod = await import(`${url}?list=${k}`);
      let v;
      try {
        await mod.loadHyphenation(list);
        v = mod.HYPHENATION_PATTERN_IDS.filter((id) => mod.hyphenatorFor(id) !== undefined).join(',');
      } catch { v = 'R'; }
      cap.set(`loadids/${k}`, 'in', list.map((t) => (t === undefined ? '~' : t.length)).join(','));
      cap.set(`loadids/${k}`, 'ids', v);
    }
  }

  // ---- the whole catalog tables, in insertion order ----
  {
    const s = new Stream();
    s.str(catalog.DEFAULT_FAMILY);
    cap.set('catalog', 'default', done(s));
    const sub = new Stream();
    for (const [k, v] of Object.entries(catalog.METRIC_SUBSTITUTES)) sub.str(k).str(v);
    cap.set('catalog', 'substitutes', done(sub));
    const bun = new Stream();
    for (const [k, v] of Object.entries(catalog.BUNDLED)) bun.str(k).str(v.family).str(v.file);
    cap.set('catalog', 'bundled', done(bun));
    const probe = new Stream();
    for (const f of catalog.PROBE_FAMILIES) probe.str(f);
    cap.set('catalog', 'probe', done(probe));
    const names = new Stream();
    for (const [, v] of Object.entries(catalog.BUNDLED)) for (const variant of types.VARIANTS) names.str(catalog.bundledFileName(v.file, variant));
    cap.set('catalog', 'files', done(names));
  }

  // ---- hyphenation: the pattern fields and the hyphens of the oracle words ----
  const oracle = readFileSync(join(parity, 'fonts_oracle_words.txt'), 'utf8').split('\n').filter(Boolean);
  cap.set('hyphen', 'ids', hyphenIndex.HYPHENATION_PATTERN_IDS.join(','));
  const tokens = (s) => s.split(/\s+/).filter(Boolean);
  for (const id of LANGS) {
    const { default: patterns } = await importFrom(root, `write/packages/docx-layout/dist/typography/hyphen/patterns/${id}.js`);
    const key = `hyphen/${id}`;
    const f = new Stream();
    f.str(patterns.id).u32(patterns.left).u32(patterns.right);
    const pats = tokens(patterns.patterns);
    const excs = tokens(patterns.exceptions);
    f.u64(pats.length);
    for (const t of pats) f.str(t);
    f.u64(excs.length);
    for (const t of excs) f.str(t);
    cap.set(key, 'fields', done(f));
    cap.set(key, 'nfc', String(pats.every((t) => t.normalize('NFC') === t) && excs.every((t) => t.normalize('NFC') === t)));
    const h = new Hyphenator(patterns);
    const words = [...oracle];
    for (const w of oracle) words.push(w.toUpperCase(), w.length ? w[0].toUpperCase() + w.slice(1) : w);
    for (const e of excs) words.push(e.replace(/-/g, ''));
    cap.set(key, 'words', String(words.length));
    for (let from = 0, chunk = 0; from < words.length; from += 250, chunk++) {
      const st = new Stream();
      for (const w of words.slice(from, from + 250)) {
        st.tag('w');
        const pts = h.points(w);
        st.u32(pts.length);
        for (const p of pts) st.u32(p);
        for (let l = 1; l <= 3; l++) for (let r = 1; r <= 3; r++) {
          const hy = h.hyphenate(w, l, r);
          st.u32(hy.length);
          for (const p of hy) st.u32(p);
        }
      }
      cap.set(key, `chunk${chunk}`, done(st));
    }
  }

  // ---- the embedding plan with a stub resolver ----
  const pool = new Map();
  for (const name of ['carlito-regular', 'carlito-bold', 'carlito-italic', 'carlito-bolditalic', 'mono-regular', 'amatic-regular', 'restricted', 'bitmap', 'editable', 'cff', 'broken-cmap', 'glyf-small', 'garbage', 'cut-dir']) {
    const bytes = new Uint8Array(readFileSync(join(corpusDir, 'pool', `${name}.bin`)));
    pool.set(name, bytes);
    cap.set(`embed/pool/${name}`, 'in', `${bytes.length}:${sha(bytes)}`);
  }
  const bytesOf = (name) => pool.get(name);
  const mapOf = (field) => {
    const out = new Map();
    for (const item of field.split('+').filter(Boolean)) {
      const [v, poolName] = item.split('=');
      out.set(VARIANT_OF[v], poolName);
    }
    return out;
  };
  const scenarios = readFileSync(join(parity, 'fonts_embed_scenarios.txt'), 'utf8').split('\n').filter(Boolean).map((l) => l.split('\t'));
  for (const [, id, app, settingsBits, alreadyField, usageField, resolverField, docField] of scenarios) {
    const key = `embed/${id}`;
    const usage = usageField.split(';').map((item) => {
      const [family, vs, hex] = item.split('|');
      return { family, variants: [...vs].map((c) => VARIANT_OF[c]), text: hex ? String.fromCodePoint(...hex.split(',').map((h) => parseInt(h, 16))) : '' };
    });
    const table = new Map();
    for (const item of resolverField.split(';').filter(Boolean)) {
      const [family, source, have, load] = item.split('|');
      const faces = {};
      for (const [v, name] of mapOf(have)) faces[v] = bytesOf(name);
      table.set(family.toLowerCase(), { family, source, faces, embeddable: true, load: mapOf(load) });
    }
    const docFaces = new Map();
    for (const item of docField.split('+').filter(Boolean)) {
      const [family, rest] = item.split('|');
      const [v, name] = rest.split('=');
      docFaces.set(`${family}\u0000${VARIANT_OF[v]}`, bytesOf(name));
    }
    const docCalls = new Stream();
    const loadCalls = new Stream();
    let nDoc = 0, nLoad = 0;
    const resolver = {
      resolve: async (families) => families.map((f) => table.get(f.toLowerCase())).filter(Boolean),
      loadFace: async (r, v) => { loadCalls.str(r.family).str(v); nLoad++; const name = r.load.get(v); return name ? bytesOf(name) : undefined; },
    };
    const settings = { embed: settingsBits[0] === '1', subset: settingsBits[1] === '1', skipCommonFonts: settingsBits[2] === '1' };
    const opts = {
      documentFace: (family, v) => { docCalls.str(family).str(v); nDoc++; return docFaces.get(`${family}\u0000${v}`); },
      alreadyEmbedded: new Set(alreadyField.split(',').filter(Boolean)),
    };
    const out = new Stream();
    try {
      const plan = await embedding.planFontEmbedding(resolver, usage, settings, app, opts);
      out.tag('P').u64(plan.faces.length);
      for (const f of plan.faces) {
        out.str(f.family).str(f.variant).digestOf(f.bytes).bool(f.subsetted);
        writeInfo(out, f.info);
      }
      out.u64(plan.skipped.length);
      for (const sk of plan.skipped) out.str(sk.family).str(sk.reason);
    } catch { out.tag('R'); }
    cap.set(key, 'in', `${app},${settingsBits}`);
    cap.set(key, 'doc', `${nDoc}:${docCalls.finish().short}`);
    cap.set(key, 'load', `${nLoad}:${loadCalls.finish().short}`);
    cap.set(key, 'plan', done(out));
  }
  void shared;
}
