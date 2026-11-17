// Loaders of the frozen TS the chart capture runs over, and the TWO host bridges rebuilt with a call log:
//   ppt  slides-core `chart-engine.ts` `parseChart` (no canonical prefixes, `colorFromDml` over the deck theme, images)
//   xl   sheet-core `chart.ts` `parseChart` (canonicalNs, `dmlColor` over the workbook scheme, Office 2023 theme without
//        a theme part, theme line widths) WITHOUT `refreshChartData` (the cells are the Sheet host's, track C)
// Each bridge is a copy of the 20 lines of the TS with a recording `ChartHost`; the capture checks the model of the copy
// against the model of the real `parseChart` (ppt) so a drift of the copy cannot go unnoticed.
//
// The frozen tree is only read. `Renderer` is not exported by `render.js`: `patchedRender` evaluates an in-memory copy of
// the module text (+ `export { Renderer }`, relative imports made absolute) from a `data:` URL; nothing is written.
import { readFileSync } from 'node:fs';
import { join } from 'node:path';
import { pathToFileURL } from 'node:url';
import { canonJson } from '../../../uo-pptx/js/lib/canon-json.mjs';

const url = (p) => pathToFileURL(p).href;
let runs = 0;

export async function loadTs(root) {
  const chartsDist = (f) => join(root, 'shared', 'charts', 'dist', f);
  const charts = await import(url(chartsDist('index.js')));
  const slides = await import(url(join(root, 'slides', 'packages', 'slides-core', 'dist', 'index.js')));
  const sheet = await import(url(join(root, 'sheet', 'packages', 'sheet-core', 'dist', 'index.js')));
  const scale = await import(url(chartsDist('scale.js')));
  const trend = await import(url(chartsDist('trend.js')));
  const chartex = await import(url(chartsDist('chartex.js')));
  const image = await import(url(chartsDist('image.js')));
  return {
    root, charts, slides, sheet, scale, trend, chartex, image,
    /** a FRESH instance of the unpatched render.js (module-level caches start empty, like freshRender of B4's capture) */
    freshRender: () => import(`${url(chartsDist('render.js'))}?run=${++runs}`),
    /** the in-memory patched copy: { Renderer, paintChartSpace } */
    patchedRender: () => patchedRender(chartsDist('render.js'), chartsDist),
  };
}

let patched;
async function patchedRender(file, dist) {
  if (patched) return patched;
  let src = readFileSync(file, 'utf8');
  src = src.replace(/^(import .* from )'\.\/([^']+)';$/gm, (_m, a, f) => `${a}'${url(dist(f))}';`);
  src = src.replace(/\n\/\/# sourceMappingURL=.*$/m, '');
  src += '\nexport { Renderer };\n';
  patched = await import(`data:text/javascript;base64,${Buffer.from(src, 'utf8').toString('base64')}`);
  return patched;
}

// ------------------------------------------------------------------------------------------------ host call log
/**
 * A recording host. `log` receives `{ k: 'color'|'blip'|'fill', key, ans }` in call order; `key` is the canonical JSON of the
 * element (`color`, `blip`: name, attrs sorted, children) or `idx|phClr` (`fill`); `ans` the canonical JSON of the answer
 * (`null` for undefined).
 */
export function recordingHost(inner, log) {
  const ans = (v) => (v === undefined ? 'null' : canonJson(v));
  const host = { ...inner };
  host.color = (el) => { const r = inner.color(el); log.push({ k: 'color', key: canonJson(el), ans: ans(r) }); return r; };
  if (inner.blip) host.blip = (el) => { const r = inner.blip(el); log.push({ k: 'blip', key: canonJson(el), ans: ans(r) }); return r; };
  if (inner.fillStyle) host.fillStyle = (i, ph) => { const r = inner.fillStyle(i, ph); log.push({ k: 'fill', key: `${i}|${ph}`, ans: ans(r) }); return r; };
  return host;
}

/** Office 2013–2022 palette the Sheet merges under a theme (sheet-core chart.ts FALLBACK). */
const XL_FALLBACK = {
  dk1: '#000000', lt1: '#FFFFFF', dk2: '#44546A', lt2: '#E7E6E6',
  accent1: '#4472C4', accent2: '#ED7D31', accent3: '#A5A5A5', accent4: '#FFC000', accent5: '#5B9BD5', accent6: '#70AD47',
  hlink: '#0563C1', folHlink: '#954F72',
};
/** The Office 2023 theme of a workbook without a theme part (chart.ts OFFICE_2023). */
const OFFICE_2023 = {
  colors: [],
  byName: {
    dk1: '#000000', lt1: '#FFFFFF', dk2: '#0E2841', lt2: '#E8E8E8',
    accent1: '#156082', accent2: '#E97132', accent3: '#196B24', accent4: '#0F9ED5', accent5: '#A02B93', accent6: '#4EA72E',
    hlink: '#467886', folHlink: '#96607D',
  },
  fonts: { major: 'Aptos Display', minor: 'Aptos' },
  lineWidths: [1, 1.5, 2],
};

/** The fixed synthetic theme of the generated `ppt` charts (a Slides `Theme`: colours after the clrMap, fonts, fill styles). */
export function syntheticPptTheme(parseXml) {
  const fill = (xml) => parseXml(xml);
  return {
    colors: {
      dk1: '#000000', lt1: '#FFFFFF', dk2: '#44546A', lt2: '#E7E6E6',
      accent1: '#4472C4', accent2: '#ED7D31', accent3: '#A5A5A5', accent4: '#FFC000', accent5: '#5B9BD5', accent6: '#70AD47',
      hlink: '#0563C1', folHlink: '#954F72', tx1: '#000000', bg1: '#FFFFFF', tx2: '#44546A', bg2: '#E7E6E6',
    },
    fonts: { major: 'Calibri Light', minor: 'Calibri' },
    lineWidths: [6350, 12700, 19050],
    fillStyles: [
      fill('<a:solidFill xmlns:a="x"><a:schemeClr val="phClr"/></a:solidFill>'),
      fill('<a:gradFill xmlns:a="x" rotWithShape="1"><a:gsLst><a:gs pos="0"><a:schemeClr val="phClr"><a:lumMod val="110000"/><a:satMod val="105000"/><a:tint val="67000"/></a:schemeClr></a:gs><a:gs pos="50000"><a:schemeClr val="phClr"><a:lumMod val="105000"/><a:satMod val="103000"/><a:tint val="73000"/></a:schemeClr></a:gs><a:gs pos="100000"><a:schemeClr val="phClr"><a:lumMod val="105000"/><a:satMod val="109000"/><a:tint val="81000"/></a:schemeClr></a:gs></a:gsLst><a:lin ang="5400000" scaled="0"/></a:gradFill>'),
      fill('<a:gradFill xmlns:a="x" rotWithShape="1"><a:gsLst><a:gs pos="0"><a:schemeClr val="phClr"><a:satMod val="103000"/><a:lumMod val="102000"/><a:tint val="94000"/></a:schemeClr></a:gs><a:gs pos="50000"><a:schemeClr val="phClr"><a:satMod val="110000"/><a:lumMod val="100000"/><a:shade val="100000"/></a:schemeClr></a:gs><a:gs pos="100000"><a:schemeClr val="phClr"><a:lumMod val="99000"/><a:satMod val="120000"/><a:shade val="78000"/></a:schemeClr></a:gs></a:gsLst><a:lin ang="5400000" scaled="0"/></a:gradFill>'),
    ],
  };
}

const PPT_FALLBACK = {
  accent1: '#4472C4', accent2: '#ED7D31', accent3: '#A5A5A5', accent4: '#FFC000', accent5: '#5B9BD5', accent6: '#70AD47',
  tx1: '#000000', bg1: '#FFFFFF', dk1: '#000000', lt1: '#FFFFFF', tx2: '#44546A', bg2: '#E7E6E6', dk2: '#44546A', lt2: '#E7E6E6',
};

/**
 * `ppt` bridge: the body of slides-core `parseChart`, with the recording host. Returns `{ space, log }`.
 * `in`: { xml, theme, images (rId -> { part, bytes, mime }), fileFormat }.
 */
export function parseChartPpt(ts, input, log) {
  const { parseChartSpace, parsePaint } = ts.charts;
  const { parseXml, colorFromDml } = ts.slides;
  const { xml, theme, images, fileFormat } = input;
  let root;
  try { root = parseXml(xml); } catch { root = { name: 'c:chartSpace', attrs: {}, children: [] }; }
  const th = { ...(theme ?? {}), colors: { ...PPT_FALLBACK, ...(theme?.colors ?? {}) } };
  const host = recordingHost({
    color: (el) => colorFromDml(el, th),
    blip: (el) => {
      const rid = el.attrs['r:embed'] ?? el.attrs['embed'];
      return rid ? images?.[rid]?.part : undefined;
    },
    ...(fileFormat ? { fileFormat } : {}),
    fonts: { ...(theme?.fonts?.minor ? { minor: theme.fonts.minor } : {}), ...(theme?.fonts?.major ? { major: theme.fonts.major } : {}) },
    fillStyle: (idx, ph) => {
      const el = theme?.fillStyles?.[idx - 1];
      if (!el) return undefined;
      const tp = { ...th, colors: { ...th.colors, phClr: ph } };
      return parsePaint({ name: 'w', attrs: {}, children: [el] }, (e) => colorFromDml(e, tp));
    },
  }, log);
  return { space: parseChartSpace(root, host), root, host };
}

/**
 * `xl` bridge: the body of sheet-core `parseChart` without `refreshChartData`. `in`: { xml, theme?, fileFormat, userShapesXml?,
 * noTheme }. `theme` is a sheet-core `Theme` (byName, fonts, lineWidths, fillStyles).
 */
export function parseChartXl(ts, input, log) {
  const { parseChartSpace, parsePaint, dmlColor, canonicalNs } = ts.charts;
  const { parseXml } = ts.sheet;
  let theme = input.theme;
  if (input.noTheme) theme = OFFICE_2023;
  let root;
  try { root = canonicalNs(parseXml(input.xml)); } catch { root = { name: 'c:chartSpace', attrs: {}, children: [] }; }
  let userShapes;
  if (input.userShapesXml) { try { userShapes = canonicalNs(parseXml(input.userShapesXml)); } catch { userShapes = undefined; } }
  const scheme = { ...XL_FALLBACK, ...(theme?.byName ?? {}) };
  const host = recordingHost({
    ...(input.fileFormat ? { fileFormat: input.fileFormat } : {}),
    ...(userShapes ? { userShapes } : {}),
    themeLineWidths: theme?.lineWidths?.length ? theme.lineWidths : [0.5, 1, 1.5],
    color: (el) => dmlColor(el, scheme),
    fonts: { ...(theme?.fonts?.minor ? { minor: theme.fonts.minor } : { minor: 'Calibri' }), ...(theme?.fonts?.major ? { major: theme.fonts.major } : {}) },
    fillStyle: (idx, ph) => {
      const el = theme?.fillStyles?.[idx - 1];
      if (!el) return undefined;
      const sc = { ...scheme, phClr: ph };
      return parsePaint({ name: 'w', attrs: {}, children: [el] }, (e) => dmlColor(e, sc));
    },
  }, log);
  return { space: parseChartSpace(root, host), root, userShapes, host };
}
