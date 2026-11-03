// Digest of what frontier B reads from font files, computed by the FROZEN TS (main 0.9.x) so it can
// be compared line by line with `cargo test --release -p uo-fonts --test real_fonts -- --ignored
// --nocapture digest_dir` (same lines, same hashes). Not the stage-4 capture (that one uses
// uo-testkit streams): a hand check of the port against the TS on any directory of font files.
//
//   UO_TS_ROOT=<built checkout of main> node ts_digest.mjs <dir with .ttf/.otf files>
//   UO_CONVERTER_SRC=<path of converter/packages/converter/src/pptx/sfnt.ts>   (TtfProgram, run as TS)
import { readdirSync, readFileSync } from 'node:fs';
import { join } from 'node:path';
import { pathToFileURL } from 'node:url';

const root = process.env.UO_TS_ROOT;
if (!root) throw new Error('set UO_TS_ROOT to a built checkout of main');
const dir = process.argv[2];
const { SfntFont } = await import(pathToFileURL(join(root, 'write/packages/docx-layout/dist/typography/font/sfnt.js')).href);
const shared = await import(pathToFileURL(join(root, 'shared/fonts/dist/sfnt.js')).href);
const programPath = process.env.UO_CONVERTER_SRC ?? join(root, 'converter/packages/converter/src/pptx/sfnt.ts');
const { TtfProgram } = await import(pathToFileURL(programPath).href);

const MASK = (1n << 64n) - 1n;
const PRIME = 0x100000001b3n;
const fnv = (h, v) => ((h ^ (BigInt(v) & MASK)) * PRIME) & MASK;
const OFFSET = 0xcbf29ce484222325n;
const f64 = new Float64Array(1);
const u64 = new BigUint64Array(f64.buffer);
const bits = (x) => { f64[0] = x; return u64[0]; };

const klass = (e) => {
  const m = String(e?.message ?? e);
  let r;
  if ((r = /^Tabela (.+) ausente$/.exec(m))) return `missing:${r[1]}`;
  if (/cmap sem subtabela/.test(m)) return 'cmap-none';
  if ((r = /cmap format (\d+)/.exec(m))) return `cmap-format:${r[1]}`;
  if ((r = /sem a tabela (.+)$/.exec(m))) return `missing:${r[1]}`;
  if (e instanceof RangeError) return 'range';
  return `other:${m}`;
};

const cmapDigest = (glyphId) => {
  let h = OFFSET, count = 0;
  const each = (cp) => { let g; try { g = glyphId(cp); } catch { g = 0; /* G4: the TS throws, the port gives 0 */ } if (g) { count++; h = fnv(h, cp); h = fnv(h, Math.min(g, 0xffffffff)); /* the port saturates format-12 ids above u32 */ } };
  for (let cp = 0; cp <= 0x10ffff; cp++) each(cp);
  each(0x110000);
  each(0xffffffff);
  return `${count} ${h.toString(16)}`;
};

const advDigest = (adv, n) => {
  let h = OFFSET;
  const safe = (g) => { try { return adv(g); } catch { return 0; /* G4 */ } };
  for (let g = 0; g < n + 16; g++) h = fnv(h, safe(g));
  h = fnv(h, safe(0xffff));
  return h.toString(16);
};

const hexColor = (c) => (c === null ? 0xffffffn : BigInt(parseInt(c, 16)));

function lines(name, bytes) {
  const out = [`FONT ${name}`];
  let font;
  try { font = new SfntFont(bytes); } catch (e) { out.push(`open ${klass(e)}`); font = null; }
  if (font) {
    out.push('open ok');
    const m = font.metrics;
    out.push(`metrics ${m.unitsPerEm} ${m.ascender} ${m.descender} ${m.lineGap} ${m.usWinAscent} ${m.usWinDescent} ${m.sTypoAscender} ${m.sTypoDescender} ${m.sTypoLineGap} ${m.fsSelection} ${m.codePageRange1 ?? '-'} ${m.ySuperscriptYOffset ?? '-'} ${m.xAvgCharWidth ?? '-'} ${m.panose ? m.panose.join(',') : '-'} ${m.underlinePosition} ${m.underlineThickness}`);
    out.push(`numGlyphs ${font.numGlyphs}`);
    out.push(`cmap ${cmapDigest((cp) => font.glyphId(cp))}`);
    out.push(`adv ${advDigest((g) => font.advanceWidth(g), font.numGlyphs)}`);
    let nonNull = 0, cmds = 0, h = OFFSET;
    for (let g = 0; g < font.numGlyphs; g++) {
      const o = font.glyphOutline(g);
      if (o === null) { h = fnv(h, 0xffffffffffffn); continue; }
      nonNull++;
      h = fnv(h, o.length);
      for (const c of o) {
        cmds++;
        if (c.op === 'M') { h = fnv(h, 1); h = fnv(h, bits(c.x)); h = fnv(h, bits(c.y)); }
        else if (c.op === 'L') { h = fnv(h, 2); h = fnv(h, bits(c.x)); h = fnv(h, bits(c.y)); }
        else if (c.op === 'Q') { h = fnv(h, 3); h = fnv(h, bits(c.cx)); h = fnv(h, bits(c.cy)); h = fnv(h, bits(c.x)); h = fnv(h, bits(c.y)); }
        else h = fnv(h, 4);
      }
    }
    out.push(`outline ${nonNull} ${cmds} ${h.toString(16)}`);
    h = OFFSET;
    for (let p = 0; p <= 300; p++) {
      const e = font.vdmx(p);
      h = fnv(h, e ? 1 : 0);
      if (e) { h = fnv(h, e.yMax & 0xffff); h = fnv(h, e.yMin & 0xffff); }
    }
    out.push(`vdmx ${h.toString(16)}`);
    h = OFFSET;
    for (let g = 0; g < font.numGlyphs; g++) {
      let l;
      try { l = font.colorLayers(g); } catch { l = null; /* G4 */ }
      h = fnv(h, l ? l.length + 3 : 0);
      for (const x of l ?? []) { h = fnv(h, x.glyphId); h = fnv(h, hexColor(x.color)); h = fnv(h, bits(x.alpha)); }
    }
    out.push(`colr ${font.hasColorGlyphs()} ${h.toString(16)}`);
    {
      let kh = OFFSET, nz = 0;
      const lim = Math.min(font.numGlyphs, 300);
      for (let a = 0; a < lim; a++) for (let b = 0; b < lim; b++) {
        const u = Math.round((font.kernGlyphsTwips(a, b, 12) * m.unitsPerEm) / 240);
        if (u) { nz++; kh = fnv(kh, a); kh = fnv(kh, b); kh = fnv(kh, u); }
      }
      out.push(`kern ${font.kernInLatin} ${nz} ${kh.toString(16)}`);
      let lh = OFFSET, groups = 0;
      const texts = ['office official fluffy affix ffi ffl fi fl', 'AVATAR To Ty LTV', 'a1b2 fi\u00e9 ct st'];
      for (const t of texts) {
        const gids = Array.from(t, (c) => { try { return font.glyphId(c.codePointAt(0)); } catch { return 0; /* G4 */ } });
        for (const feats of [['liga'], ['liga', 'clig'], ['liga', 'clig', 'dlig', 'hlig'], ['onum'], ['ss01']]) {
          const r = font.ligate(gids, feats);
          if (r === null) { lh = fnv(lh, 0xffff); continue; }
          lh = fnv(lh, r.length + 1);
          for (const g of r) { groups++; lh = fnv(lh, g.gid); lh = fnv(lh, g.len); }
        }
      }
      out.push(`ligate ${groups} ${lh.toString(16)}`);
    }
    let f020; try { f020 = font.glyphId(0xf020); } catch { f020 = 0; /* G4 */ }
    const sym = ((m.codePageRange1 ?? 0) >>> 31) === 1 || f020 !== 0;
    out.push(`symbol ${sym}`);
  }
  let prog;
  try { prog = new TtfProgram(bytes); } catch (e) { out.push(`program ${klass(e)}`); prog = null; }
  if (prog) {
    out.push('program ok');
    out.push(`program ${prog.unitsPerEm} ${prog.ascender} ${prog.descender} ${prog.fontBBox.join(',')} ${bits(prog.italicAngle).toString(16)}`);
    out.push(`program cmap ${cmapDigest((cp) => prog.glyphId(cp))}`);
    out.push(`program adv ${advDigest((g) => prog.advanceWidth(g), font ? font.numGlyphs : 0)}`);
  }
  out.push(`emb ${shared.isEmbeddable(bytes)} ${shared.hasTrueTypeOutlines(bytes)}`);
  try {
    const i = shared.faceInfo(bytes);
    out.push(`info ${i.panose} ${i.weight} ${i.italic} ${i.fsType} ${i.unicodeRange.join(',')} ${i.codePageRange.join(',')} ${i.checkSumAdjustment}`);
    const units = (s) => Array.from({ length: s.length }, (_, k) => s.charCodeAt(k)).join(',');
    out.push(`names ${units(i.names.family)}|${units(i.names.style)}|${units(i.names.version)}|${units(i.names.full)}`);
  } catch (e) { out.push(`info ${klass(e)}`); }
  try {
    const cps = [];
    for (let cp = 0; cp < 0x3000; cp++) cps.push(cp);
    for (let cp = 0x1f600; cp < 0x1f700; cp++) cps.push(cp);
    const ids = [...shared.glyphIdsFor(bytes, cps)].sort((a, b) => a - b);
    let h = OFFSET;
    for (const g of ids) h = fnv(h, g);
    out.push(`gids ${ids.length} ${h.toString(16)}`);
  } catch (e) { out.push(`gids ${klass(e)}`); }
  return out;
}

for (const name of readdirSync(dir).filter((f) => /\.(ttf|otf)$/i.test(f)).sort()) {
  for (const l of lines(name, new Uint8Array(readFileSync(join(dir, name))))) console.log(l);
}
