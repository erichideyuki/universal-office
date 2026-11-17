// The deterministic measurer of the chart captures: the REAL advance widths of the open fonts (Liberation Sans/Serif/Mono,
// Carlito, Caladea; every other family through its metric-compatible substitute, else Liberation Sans), the same rule as
// B3's `capture-textlayout.mjs` (a COPY of its `openFont`/`kernUnits`/`fakeWidth`, kept until B4 moves the measurer to
// `uo-pptx/js/lib/font-measure.mjs`; record uo-charts.md "Capture", item 3), plus the font box of `'Hg'`
// (`fontBoundingBoxAscent/Descent` = hhea ascender / descender x px / upem).
//
//   width   (advances + GPOS/kern pairs unless the kerning is `none`) x px / upem
//   box     ascent = hhea.ascender, descent = -hhea.descender (font units) x px / upem
import { readFileSync } from 'node:fs';
import { join } from 'node:path';
import { pathToFileURL } from 'node:url';
import { fffd } from '../../../uo-testkit/js/testkit.mjs';
import { repoDir } from '../../../uo-pptx/js/lib/common.mjs';

const FONT_FILES = {
  'liberation sans': join(repoDir, 'site', 'assets', 'fonts', 'liberationsans'),
  'liberation serif': join(repoDir, 'shared', 'fonts', 'files', 'liberationserif'),
  'liberation mono': join(repoDir, 'shared', 'fonts', 'files', 'liberationmono'),
  carlito: join(repoDir, 'shared', 'fonts', 'files', 'carlito'),
  caladea: join(repoDir, 'shared', 'fonts', 'files', 'caladea'),
};
const SUBST = { calibri: 'carlito', cambria: 'caladea', 'times new roman': 'liberation serif', times: 'liberation serif', 'courier new': 'liberation mono', courier: 'liberation mono', arial: 'liberation sans', serif: 'liberation serif', monospace: 'liberation mono' };

/** Substitute face name of a family (what the measurer uses). */
export const faceOf = (family) => {
  const f = family.toLowerCase();
  return FONT_FILES[f] ? f : SUBST[f] ?? 'liberation sans';
};

/** `[italic ][bold ]<px>px <stack>` (or the canvas default `10px sans-serif`) -> { italic, bold, px, family }. */
export function parseChartFont(s) {
  const m = /^(italic )?(bold )?(\S+)px (.*)$/.exec(s);
  if (!m) throw new Error(`unexpected font string ${s}`);
  const rest = m[4];
  const q = /^'([^']*)'/.exec(rest);
  const family = q ? q[1] : rest.split(',')[0].trim();
  return { italic: !!m[1], bold: !!m[2], px: m[3], family };
}

/** Builds the measurer over the frozen docx-layout `SfntFont`. `log` receives each answered key once. */
export async function createMeasurer(tsRootDir) {
  const { SfntFont } = await import(pathToFileURL(join(tsRootDir, 'write', 'packages', 'docx-layout', 'dist', 'typography', 'font', 'sfnt.js')).href);
  const fonts = new Map();
  const openFont = (face, bold, italic) => {
    const style = bold && italic ? 'bolditalic' : bold ? 'bold' : italic ? 'italic' : 'regular';
    const key = `${face}-${style}`;
    let font = fonts.get(key);
    if (!font) { font = new SfntFont(new Uint8Array(readFileSync(`${FONT_FILES[face]}-${style}.ttf`))); fonts.set(key, font); }
    return font;
  };
  const kernUnits = (font, g1, g2) => {
    if (g1 > 0xffff || g2 > 0xffff) return 0;
    let u = font.kernMap.get(g1 * 65536 + g2);
    if (u === undefined) {
      const c0 = font.kernClass0.get(g1);
      if (c0 && !c0.cd2.has(g2)) u = c0.adv;
    }
    return u || 0;
  };
  const width = (f, kerning, text) => {
    const font = openFont(faceOf(f.family), f.bold, f.italic);
    let units = 0, prev = -1;
    for (const ch of text) {
      let cp = ch.codePointAt(0);
      if (cp >= 0xd800 && cp <= 0xdfff) cp = 0xfffd;
      const g = font.glyphId(cp);
      units += font.advanceWidth(g);
      if (kerning !== 'none' && prev >= 0) units += kernUnits(font, prev, g);
      prev = g;
    }
    return (units * Number(f.px)) / font.metrics.unitsPerEm;
  };
  const box = (f) => {
    const font = openFont(faceOf(f.family), f.bold, f.italic);
    const px = Number(f.px), u = font.metrics.unitsPerEm;
    return { ascent: (font.metrics.ascender * px) / u, descent: (-font.metrics.descender * px) / u };
  };
  /**
   * The `measure(font, text, kerning)` of `record-ctx.mjs`; `onAsk(font, text, w, ascent, descent)` sees every call.
   * The answer carries the font box too (the recorder returns it as `fontBoundingBox*`).
   */
  const measure = (onAsk) => (font, text, kerning = 'auto') => {
    const f = parseChartFont(font);
    const w = width(f, kerning, fffd(String(text)));
    const b = box(f);
    onAsk?.(font, String(text), w, b.ascent, b.descent);
    return { width: w, size: Number(f.px), ascent: b.ascent, descent: b.descent };
  };
  return { measure, width, box };
}
