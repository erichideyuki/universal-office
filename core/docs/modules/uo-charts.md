# uo-charts — the shared chart engine (Slides + Sheet), plan §6

Record of the rust-port workflow (hybrid mode). Status: **architecture pass done 2026-11-17 (Opus)**; type skeleton in
`core/crates/uo-charts` (model, host, measure, emit, layout, error). **C-0, C-1, C-2 (chartex, scale, trend, dpi), C-3 (render core), C-4 (axes and XY), C-5 (pies, chartex, radar, labels) and C-6 (Slides host) done**
(sections below); C-7 continues (parity, memory and fuzz run in CI; the Office agreement is pending). Port frontiers C-0…C-7 below.
Companion records: `uo-dml-render.md` (the shape renderer that calls this crate, the measurement protocol),
`uo-render.md` "Slides extensions" (the display list), `uo-numfmt.md` (labels format through it), `uo-pptx.md`.

Behaviour source: the frozen TS at `D:\Projects\OfficeNext\.claude\worktrees\ts-ref-092` (`b92b60fa`, 0.9.2):
`shared/charts/src` (line numbers below are of its files), the host bridges
`slides/packages/slides-core/src/chart-engine.ts` (205) and `sheet/packages/sheet-core/src/chart.ts` (145), the call
sites `slides-core/src/render.ts:760–800` and `sheet/src/universal-sheet.ts:3528–3645`. Repository rule
"Gráficos e formatação numérica: um lugar só": one engine; the host hands the parsed XML tree and its theme colour
resolver; the engine lays out, scales and paints.

## Scope

| TS | Raw lines | Rust (`uo_charts::…`) | Frontier |
|---|---|---|---|
| `model.ts` | 336 | `model` (skeleton done) | — |
| `parse.ts` (`Parser`, `parseChartSpace`, `fixAxes`, `styleOf`, `parsePaint`) | 829 | `parse` | C-1 |
| `xml.ts` helpers (`valNum`, `valBool`, `valStr`, `descend`…) | ≈ 60 of 135 | `parse::xml` over `uo_opc::xml::Element` | C-1 |
| `xml.ts` `canonicalNs` | ≈ 75 | **not ported**: `uo-opc` `CanonicalPrefixes { table, default_namespace: true }` at the host's parse (a `CHART_PREFIXES` table constant exported here) | C-1 |
| `dml-color.ts` (`dmlColor`: the Sheet's resolver) | 103 | `color::dml_color` | C-1 |
| `color.ts` (`adjustLum`, `mixWhite`, `tintLinear`, `rgba`, `chartShadeTint`) | 81 | `color` | C-1 |
| `chartex.ts` (`parseChartEx`, `histogramBins`) | 392 | `chartex` | C-2 |
| `scale.ts`, `trend.ts` | 93 + 120 | `scale`, `trend` | C-2 |
| `image.ts` `imageDpi` (+ PNG/JPEG/EXIF readers) | ≈ 85 of 158 | `image` (or `uo-image` if it grows a dpi reader) | C-2 |
| `image.ts` `tilePattern` + `flippedTile` | ≈ 70 | **shell painter** (reads the live transform and draws on an offscreen canvas): `paint-slides.ts`, shared with Slides shape tiles (B4 already moved it behind `PatternRes.tile`) | C-6 / B4 |
| `render.ts` 1–785: `Renderer` setup, text/wrap/`drawLines`, fills/lines/`shapeRect`/blip, automatic colours and styles, `paint`, user shapes, title, legend | 785 | `render::{core, text, paint, color, title, legend, user}` | C-3 |
| `chart-engine.ts` `chartPartBounds` (144–205) | ≈ 55 | `render::bounds` (`BoundsCanvas`) | C-3 |
| `render.ts` 785–1990: axes charts (`paintAxesChart`, value/category/date axes, ticks, `plotRect`, `measureAxis`, grid, axis, data table, 3D walls/floor/boxes, bars, markers, lines, trendlines, area, scatter, bubbles) | ≈ 1,205 | `render::{axes, bars, lines, xy, trend3d}` | C-4 |
| `render.ts` 1990–2862: pie 3D, box & whisker, treemap/sunburst, bar/pie of pie, pies/doughnut, radar, data labels, helpers | ≈ 870 | `render::{pie, hier, radar, labels, util}` | C-5 |
| `chart-engine.ts` `parseChart`/`paintChart`/`summarize` + render.ts 760–800 (Slides host) | ≈ 160 | `uo_dml_render::emit::chart` (implements `ChartHost`, `Canvas`, `ChartMeasure`) | C-6 (with/after B4) |
| `sheet-core/src/chart.ts` (Sheet host: Office 2023 theme, `chartFileFormat`, user shapes, `refreshChartData`) | 145 | Sheet's crate (track C, C4) | track C |

Total ≈ **5,100 raw TS lines** in the crate (numfmt excluded) + ≈ 160 of Slides host. `numfmt.ts` is `uo-numfmt`
(done). Error bars (`c:errBars`) are not read by the TS: ignored here too (the generator checks it). `surface` charts
are parsed and painted as axes only (render.ts:895–905), `regionMap` as the title only (chartex.ts:338).

## Dependency direction (correction of `uo-dml-render.md`)

```
uo-jsnum, uo-text ──┐
uo-opc (xml) ───────┼──► uo-charts ◄── uo-numfmt
                    │        ▲  (Canvas, ChartHost, ChartMeasure traits: no uo-render, no uo-dml-render)
                    │        │
uo-render ──► uo-dml-render ─┘  emit::chart: impl ChartHost (theme + color_from_dml), impl ChartMeasure for
                    ▲            MeasureTable, impl Canvas for its emission context (calls → SItems)
               uo-pptx / Sheet crates
```

- `uo-charts` depends on `uo-opc` (the `Element` the host hands), `uo-numfmt`, and in the port `uo-jsnum`/`uo-text`.
  **Not** on `uo-render`: it paints through the `Canvas` trait (one method per canvas call render.ts makes), and the
  host's emitter (B4's `emit::Cx` in `uo-dml-render`; the Sheet viewport list later) turns each call into one item.
  This keeps the arrow `uo-dml-render → uo-charts` (the emitter reaches charts in z-order inside `paintShape`) and
  avoids a second list builder. The task brief allowed a `uo-render` dependency "as needed by the types": none is.
- The measurement trait lives here (`ChartMeasure`) for the same reason; `uo-dml-render` implements it for its
  `MeasureTable`, so a slide batches chart questions with shape questions in one round.
- Duplicates to remove later (after B4 lands): `model::ChartColor` = `uo_dml_render::CssColor`, `measure::ChartFace`
  = `uo_dml_render::text::FaceKey`. Both should move down (`CssColor` → `uo-drawingml`, `FaceKey` → `uo-text` or a
  tiny shared module) and be re-exported; not done now because B4 is editing `uo-dml-render`.

## Data model (skeleton `src/model.rs`, `src/layout.rs`)

- **Owned trees, immutable after the parse**: `ChartSpace { groups: Vec<Group { series: Vec<Series> }>, axes:
  Vec<Axis>, … }`. A chart is parsed once per (part, theme, images, format) and shared by the host's cache as
  `Arc<ChartSpace>` (uo-pptx M5: 32 models). Same reasoning as `uo-dml-render` decision A1 (arenas pay off only for
  structures cloned across lists; nothing here is). Chartex conversion builds new owned series.
- **Identity maps → indices**: `Map<Series, number>` (render.ts:136) → a global series index computed at emit from
  `SeriesRef { group, series }`; `Map<Axis, AxisInfo>` (render.ts:792) → `Vec<AxisInfo>` in insertion order with
  `AxisIx` (position in `ChartSpace::axes`); `[...cs.groups].sort(rank)` (894) → a `Vec<u32>` of group indices sorted
  with `uo_jsnum::sort` (stable).
- **Ordered maps**: `dPts`, `DLbls.points` (`Map<number, …>`) and `Legend.deleted` (`Set<number>`) are
  `PointMap`/`PointSet`: insertion order, JS `Map.set` replaces in place, keys as `JsNumKey` (SameValueZero:
  `parseFloat` keys like `1.5`, `-0`, NaN behave as in the TS).
- **`undefined` vs absent**: `Option` wherever the TS tests `!== undefined` or spreads `stripUndef` (the merges of
  render.ts:2800–2820 become field-wise `Option::or`); `Option<bool>` for tri-state booleans.
- **Enums**: closed where the parser validates (`GroupKind`, `AxisKind`, `BarDir`, `CrossBetween`, `DispBlanksAs`,
  `LayoutTarget/Mode`, `TileFlip`); unions the parser only casts (`as Axis['pos']`, parse.ts:545–555, 489, 598) keep
  the unknown spelling in `Other(Box<str>)` so a comparison with a known literal stays false; open strings compared at
  render time (`dLbls.pos`, `dash`, `cap`, marker `symbol`, trendline `kind`, `scatterStyle`, `geom`, `algn`…) stay
  `Box<str>`.
- **Colours** are CSS strings (`ChartColor`, inline `#RRGGBB`), exactly as the host resolver and `rgbToHex` produce
  them (`'#'+val` lower case survives: the canvas gets the string unparsed).
- **Format codes** are `FmtCode(Box<[u16]>)`: `uo-numfmt` takes UTF-16, so no conversion per label.
- **`themeFill` closure** (model.ts:333) is not stored: `ChartSpace::has_theme_fill` records its presence, and the
  emit calls `ChartHost::fill_style` on the host it is given.
- **Layout results** (`layout.rs`): `ResolvedText`, `Scale`, `AxisInfo`, `TitleLayout`, `LegendLayout`, `Depth3D`,
  `DataTableLayout`, `HNode`, `TrendFit` (the TS closure `f(x)` becomes `TrendModel` + coefficients, evaluated with the
  TS operation order), `HistogramBins`. They live for one emit and are the capture's layout facets.
- Sizes: no budget on `size_of` (a chart has tens of series, not thousands of shapes); M3 measures retained bytes
  per chart instead (below).

## Host interface (`src/host.rs`)

```rust
pub trait ChartHost {
    fn color(&self, src: ColorSource<'_>) -> Option<ChartColor>;   // Element(..) | Scheme("accent1") (parse.ts:721)
    fn fonts(&self) -> HostFonts<'_>;                                 // +mj-lt / +mn-lt
    fn has_fill_style(&self) -> bool;
    fn fill_style(&self, idx: u32, ph_clr: &str) -> Option<Paint>;   // fillStyleLst[idx-1] with phClr
    fn blip(&self, blip: Element<'_>) -> Option<BlipKey>;
    fn file_format(&self) -> Option<FileFormat>;                      // Mso2007 | Modern | None
    fn user_shapes(&self) -> Option<Element<'_>>;
    fn theme_line_widths(&self) -> &[f64];
}
// C-1..C-5 public API (signatures fixed here):
pub fn parse_chart_space(root: Element<'_>, host: &dyn ChartHost) -> ChartSpace;
pub fn parse_paint(el: Element<'_>, color: &dyn Fn(ColorSource<'_>) -> Option<ChartColor>) -> Option<Paint>;
pub fn dml_color(el: Element<'_>, scheme: &Scheme) -> Option<ChartColor>;            // the Sheet's resolver
pub const CHART_PREFIXES: &[(&str, &str)];                                           // canonicalNs table for uo-opc
pub fn need_measures(cs: &ChartSpace, host: &dyn ChartHost, p: &EmitParams, m: &dyn ChartMeasure) -> Vec<ChartMeasureReq>;
pub fn emit_chart<C: Canvas>(cs: &ChartSpace, host: &dyn ChartHost, p: &EmitParams, m: &dyn ChartMeasure, c: &mut C) -> EmitResult;
pub fn part_bounds(cs: &ChartSpace, host: &dyn ChartHost, p: &EmitParams, m: &dyn ChartMeasure) -> Result<Option<Rect>, ChartError>;
pub fn summarize(cs: &ChartSpace) -> ChartSummary;                                   // type/title/series/categories (both hosts)
```

- **Slides** (C-6, in `uo_dml_render::emit::chart`): parse the chart part with `uo-opc` + `CHART_PREFIXES` (the TS
  slides parser did not canonicalize, the Sheet did: canonicalizing in Slides changes nothing for canonical files and
  fixes default-namespace ones — **declared difference CH-4**, or keep it off for Slides for strict parity, to decide);
  host = `ChartRef.theme` with the `FALLBACK` palette merged (chart-engine.ts:65–73), `color_from_dml`, `fill_style`
  via `parse_paint` over `theme.fill_styles[idx-1]` with `phClr`, `blip` → `ChartRef.images` by `r:embed` → media id,
  `file_format` = `ChartRef.format`. Emit with `HostProfile { area_fill_bg1: false, area_line: None,
  excel_axis_density: false }`, `px_per_pt = EMU_PER_POINT × fit.scale`, locale pt-BR. Animated chart parts
  (render.ts:776–797): per part `Save`, `SlotBegin(ChartPart j)` with `bx = part_bounds(sel)` (the frame for `bg`
  parts), soft-reveal `LayerBegin` → `emit_chart(sel)` → `LayerEnd(Composite)`, else block `emit_chart(sel)` again
  (the no-layer fallback), `SlotEnd`, `Restore` — the B4 slot/layer contracts (R-S3 rules 1–2).
- **Sheet** (track C): parse after `refreshChartData` (the Sheet rewrites the caches from the cells as XML text and
  parses it: behaviour-identical to the TS tree rewrite, no engine hook), host = `dml_color` over the workbook scheme
  (Office 2023 theme without a theme part), `theme_line_widths` (default `[0.5, 1, 1.5]`), `HostProfile {
  area_fill_bg1: true, area_line: Some(Auto2010 | Color), excel_axis_density: true }`, `px_per_pt = zoom × 96/72`.
  The Sheet viewport list implements `Canvas` the same way (or reuses `uo-dml-render`'s context).

## Text through MeasureTable

Every measurement `render.ts` asks (two kinds, one capability):

| # | Site (render.ts) | Asked | Font | Used as |
|---|---|---|---|---|
| CM1 | `measure()` 182–185 and its callers: `wrap` 197 (each `cand.trimEnd()`), user text 515, title 579/589/607, legend 662/731–732, data table names 830, value-label width estimate 958, `plotRect` 1134/1137, `measureAxis` 1184/1194/1209/1213–1214/1228/1231, trendline labels 1874/1895 (superscript font), pie 3D 2100, sunburst 2297, pie labels 2499–2500, data labels 2669 | `measureText(s).width` | `font(t)` = `[italic ][bold ]<(sz·S).toFixed(2)>px <stack>` | widths, wraps, rotations, label skips, densities |
| CM2 | `drawLines` underline 220 | same (font just set by `drawLines`) | same | underline length |
| CM3 | `paintTitle` 615: `x += c.measureText(r.text).width` | same | same | run advance |
| CM4 | `fontK` 1054–1065 | `fontBoundingBoxAscent + fontBoundingBoxDescent` of `'Hg'` | `font({…t, sz: 100/S})` ≈ 100 px | tick lengths and label gaps scale with the font's line height (clamped 0.8–1.25; non-finite or missing → 1) |
| CM5 | `RenderOptions.image(ref)` (setBlip 256, user picture 473) | decoded pixel size; natural pt from `imageDpi` | — | stretch/tile matrices, `im.w > 0` tests |

- Kerning and letter spacing are the canvas's **current** state at every chart call (`auto`, `0px`: the Slides
  painter restores both after each `TextRun`); `uo-dml-render` maps CM1–CM3 to `Width(StringKey { kerning: Current,
  letter_spacing_px: 0 })`, CM4 to `Metrics(… "Hg")`, CM5 to `ImageSize` (+ the media's dpi).
- **Size key**: the canvas parses the TS string, so the key is `parse(toFixed(sz·S, 2))` (`uo_jsnum::format::to_fixed`
  then `uo_jsnum::parse`), not `sz·S`.
- **Font validity** (trap): an invalid `ctx.font` assignment (`NaN`, negative or non-finite size) is ignored by a
  canvas, so the following `measureText` uses the **previous** font. The engine keeps the effective font state with
  that rule (initial state: the inherited canvas default `10px sans-serif`, declared CH-5) and measures with it. The
  recorder of R-S4 accepts any string today: it needs the same rule (addition R4 below).
- **Two-phase**: a pass counts a missing answer as 0, continues, and returns `ChartError::NeedMeasures` with every
  miss (deduplicated); `need_measures` runs the pass over a `NullCanvas`. Rounds are data-dependent: `wrap` asks
  `cur + word` candidates whose prefix depends on the previous break, so a label of n lines may need n rounds. Rule
  CH-1 (to confirm on the captures): on the first miss inside `wrap`, also ask every trimmed contiguous token span of
  that paragraph when it has ≤ 32 tokens (≤ 528 strings), which settles the wrap in one round; past that, rounds.
  The cap stays `MAX_ROUNDS` (8) unless the captures show more; measured round counts go into this record.
- **Server/PDF**: the same answers from `uo-fonts` (widths: advances + kerning of the whole string, as B3's
  measurer; font box: hhea/OS/2 ascent + descent × px / upem).

## Emission (`src/emit.rs`)

The engine owns the canvas-state rules (font, `lineWidth` validity and read-back — `shapeRect`'s
`c.lineWidth <= 1.5`, render.ts:327 — per save level) in a wrapper, then forwards each call to the `Canvas`. Map of the
calls to the Slides list (C-6):

| Canvas call (count in render.ts) | Slides item |
|---|---|
| `save`/`restore` (13), `translate` (7), `rotate` (5) | `Save`, `Restore`, `Translate`, `Rotate` |
| `fillStyle` CSS / linear / radial (`createLinearGradient`, `createRadialGradient`) | `FillStyle(Css)`, `FillStyle(Linear/Radial)` + `gradients`/`stops` arenas |
| `fillStyle` stretched picture (`createPattern(no-repeat)` + `setTransform`) | `FillStyle(Pattern)`, `PatternRes { src: Image(Media), repeat: NoRepeat, transform }` |
| `fillStyle` tiled picture (`tilePattern`) | `PatternRes { tile: TileDef }` (B4's addition) — needs **A4** |
| `strokeStyle`, `lineWidth`, `setLineDash` (30), `lineCap`, `lineJoin` | `StrokeStyle`, `LineWidth`, `LineDash`, `LineCap`, `LineJoin` |
| `shadowColor/Blur/OffsetX/OffsetY` (shapeRect 315–321) | `ShadowColor`, `ShadowBlur`, `ShadowOffset(CtmOperand { add: dx·S, k: 0 })` (k = 0: no CTM term) |
| `globalAlpha *= a` | `GlobalAlphaMul` |
| `font` (8) | **A1** `ChartFont` |
| `textAlign`, `textBaseline` | `TextAlign`, `TextBaseline` |
| `beginPath`, `moveTo`, `lineTo`, `bezierCurveTo`, `rect`, `arc`, `ellipse`, `closePath`, `fill`, `stroke`, `clip` | `BeginPath`, `AppendPath(span)` (`PathOp`), `Fill(NonZero)`, `Stroke`, `Clip(NonZero)` |
| `roundRect` (2) | **A3** `PathOp::RoundRect` |
| `fillRect`, `strokeRect` | `FillRect`, `StrokeRect` |
| `fillText` (6) | **A2** `FillText` |
| `drawImage(img, x, y, w, h)` (1) | `DrawImage { src: Media, s: None, d }` |

### Schema additions needed (for the schema owner — B4 is editing `slides.uos` now; not changed here)

- **A1** `SItem::ChartFont = "chartFont" (ChartFontRef)`, `struct ChartFontRef { face: ref<faces>, size_px: f64 }`.
  Painter: `ctx.font = (italic ? 'italic ' : '') + (bold ? 'bold ' : '') + size_px.toFixed(2) + 'px ' + stack(face)`
  (render.ts:177–180). `Font(FontRef)` prints `{700|400} <JS number>px`: a different string in the recorder's
  `font=` and in `toFixed(2)` (`13.30` vs `13.3`), so it cannot be reused. (Alternative: a `form` field on `FontRef`,
  which touches `TextRun`.)
- **A2** `SItem::FillText = "fillText" (FillTextItem)`, `struct FillTextItem { text: Span, x: f64, y: f64 }`: a plain
  `fillText` under the current state. `TextRun` (even with B4's new `bullet: true`) sets its own font and fill.
- **A3** `PathOp::RoundRect = "roundRect" (RoundRectOp)`, `struct RoundRectOp { x, y, w, h, r: f64 }` (one radius;
  the engine returns `Throws(RangeError)` for a negative radius before emitting, as the canvas throws).
- **A4** `TileParams.unit_den_x: f64?`, `unit_den_y: f64?` (absent = 1): the painter's natural size becomes
  `(iw × unit_x / den) × scale`. Charts compute `(w × 72 / dpi) × S` (slides render.ts:436, then charts render.ts:261) or
  `(w × 0.75) × S`; Slides shapes `iw × (914400 / dpi) × scale` — `x / 1` is exact, so Slides bytes do not change, and
  charts set `unit_x = 72, den = dpi` (or 0.75, 1), `scale = S`, `tx/ty` in pt. Without A4 the chart's `72 / dpi` would
  be rounded once more than the TS.
- **R4** (recorder `record-ctx.mjs`, R-S4): `roundRect` recorded in device space (four corners + `r` + the linear
  part, like `arcTo`); `set font` ignores a string a canvas rejects (size token not a finite non-negative number);
  `measureText` returns `fontBoundingBoxAscent/Descent` from the capture measurer (the stub returns width only, so
  `fontK` would always be 1).
- **R5** (painter `paint-slides.ts`): `chartFont`, `fillText`, `roundRect`, and the A4 denominator in `tilePattern`.

Nothing else is needed: shadows, clips, gradients, stretched patterns, images, alpha and the `ChartPart` slot exist.

### Part bounds (`chartPartBounds`, chart-engine.ts:144–205)

`part_bounds` runs the emit with `images: false` and no frame line (`paintChart(rec, …, undefined, …)`) over
`BoundsCanvas`, which reproduces the proxy exactly: `translate` accumulates (rotation ignored), `save`/`restore`
stack the translation, `lineWidth` assignments tracked as `Number(v) || 0` (even ones a canvas rejects; not restored by
`restore`), `fill` flushes the path points, `stroke` with `lw / 2` padding, `fillRect`/`strokeRect`/`rect`/`arc`
(quadrant points)/`ellipse` (`max(rx, ry)`) as written, text ignored; `None` when nothing was drawn. Its measurement
answers are the same table (the TS proxy forwards `measureText`). The TS `boundsCache` (per model, key `sel.key@x,y,w,
h,ppt` with `toFixed(2)`) becomes a per-model LRU in the host's chart cache (M5).

## Per-scale decisions (one emit per chart rect × px-per-pt × origin)

1. Every coordinate is computed in target px from `rect` and `S = pxPerPt`.
2. Font sizes are `(sz·S).toFixed(2)` px: measured widths are not linear in S, so wraps, label rotation (0/45/90),
   label skip, legend columns and date-label thinning (`dateLabels` `need`) can change with the zoom.
3. Value-axis density `maxDiv = ⌊L / lineH⌋ − 1` etc. is a ratio of px values: mathematically scale-free, but `floor`
   of floating ratios flips at boundaries.
4. Minimum widths in px: `max(1, w·S)` lines (render.ts:298), underline `max(1, sz·S/14)`.
5. Pixel snapping in **list px** (not device px): `shapeRect` strokes `round(x) + 0.5` when `lineWidth ≤ 1.5`
   (327–330), so a change of origin changes the rounding.
6. Tiled pictures: the integer device tile size reads the live CTM (painter, `PatternRes.tile`).
7. Rounded corners `6 × pxPerPt`, shadows `blur·S`, `dx·S`.
So no scale-free layout cache: Slides re-emits per (slide, scale), Sheet per zoom (it repaints anyway).

## Frontiers (Sonnet ports inside this architecture; parity verifier on top)

| # | Content | Files (TS) | Rust API | Capture points | Size (TS → Rust) | Order |
|---|---|---|---|---|---|---|
| **C-0** (done, see "C-0 — harness") | harness: capture script, generator group `c1a`, native replay scaffold, `RecCanvas` (the recorder's `draw` form printed natively from `Canvas` calls) | — | `tests/parity.rs`, `tests/support/rec.rs`, `js/` | all below | ≈ 900 new (JS + Rust) | first, ∥ C-1 |
| **C-1** | model + parser + colours | `parse.ts`, `model.ts`, `xml.ts` helpers, `dml-color.ts`, `color.ts` | `parse_chart_space`, `parse_paint`, `dml_color`, `CHART_PREFIXES`, `color::*`, `summarize` | model JSON (sorted keys; `PointMap` as entry arrays), host call log | 1,410 → ≈ 1,700 | after C-0 starts |
| **C-2** | chartex, scale, trendlines, dpi | `chartex.ts`, `scale.ts`, `trend.ts`, `image.ts` `imageDpi` | `chartex::parse`, `histogram_bins`, `scale::{auto_scale, tick_values, frac, nice_step}`, `trend::fit_trend`, `trend_num`, `image::image_dpi` | chartex model JSON; scale/trend/histogram differential facets | 690 → ≈ 850 | after C-1 |
| **C-3** | render core | render.ts 1–785, chart-engine.ts 144–205 | `emit_chart`, `need_measures`, `part_bounds`, `Canvas` state wrapper, `NullCanvas`, `BoundsCanvas` | op stream (`draw` form) of title/legend/user-shape/area charts, layout facets `TitleLayout`/`LegendLayout`, bounds facets, measure-request sets | 840 → ≈ 1,300 | ∥ C-2 |
| **C-4** (done, see "C-4") | axes and XY families | render.ts 785–1990 | (inside `emit_chart`) | op stream; facets `AxisInfo`, `Scale`, plot rect, `Depth3D`, `DataTableLayout` | 1,205 → ≈ 2,160 | after C-3 |
| **C-5** (done, see "C-5") | pies, hierarchy, radar, labels | render.ts 1990–2862 | (inside `emit_chart`) | op stream; facets `HNode`, label texts | 870 → ≈ 1,400 | ∥ C-4 |
| **C-6** (done, see "C-6") | Slides host | chart-engine.ts host half, render.ts 760–800, `tilePattern` (CT-1) | `uo_dml_render::emit::chart` | B4 drawops over the 18 chart slides of the samples + the `c1d` deck group + the tile facets of `c1a` | ≈ 160 → ≈ 620 (host, canvas, cache, verification) | after A1–A3 + C-5, inside/after B4 |
| **C-7** | verification: parity, memory, fuzz, Office agreement | — | — | — | — | continuous |

Total ≈ 5,100 TS lines → ≈ 6,900 Rust (incl. harness). Probable time with two agents: C-0 ∥ C-1 → C-2 ∥ C-3 → C-4 ∥ C-5
→ C-6 + verification ≈ **2 days** (plan §6 said 1.5). C-1 and C-2 need nothing from B4; C-3 to C-5 compare natively
through `RecCanvas` before A1–A4 exist; only C-6 waits for B4.

## Parity plan

### Capture (`core/crates/uo-charts/js/capture-charts.mjs`, new; C-0)

Inputs: chart XML files (generator group or tracked samples), each with a host profile (`ppt`: Slides defaults,
`colorFromDml`-like resolver over a synthetic theme; `xl`: Sheet defaults with `dmlColor` and the Office 2013 palette)
and a `fileFormat` (`mso2007`/`modern`/none). For each chart, against the frozen `shared/charts/dist`:

1. **Model facet**: `parseChartSpace(root, recordingHost)` → canonical JSON (keys sorted; `Map`s as `[key, value]`
   arrays in insertion order, `Set`s as arrays; `themeFill` as `true`), and the **host log**: every `color(el)` call as
   (element path from the root, or `scheme:<val>`) → answer, `fillStyle(idx, ph)` → paint, `blip` → key. The replay
   answers the host from this log, so the parse is tested without porting a theme resolver.
2. **Op facet**: `paintChartSpace(recCtx, cs, x, y, w, h, opts)` over `record-ctx.mjs` (`draw` form), for views
   `a` (480 × 300 at the origin, `pxPerPt` 1.333) and `b` (fractional origin, `pxPerPt` 0.8833), × selections {full,
   `bg` only, `data: null`, series 0, category 1, point (0, 2), not-covered(series 0)} on a subset.
3. **Measure log**: each `(font string, text)` → width and each `'Hg'` font box, from a deterministic measurer over
   the open fonts (B3's: hmtx advances + GPOS/`kern` pairs × px / upem; font box hhea ascent + descent; substitutes
   calibri→carlito, cambria→caladea, else Liberation Sans). B3's measurer lives inside `capture-textlayout.mjs`: move it
   to `uo-pptx/js/lib/font-measure.mjs` after B4 (both captures import it) — until then the chart capture keeps a copy.
4. **Layout facets**: the frozen `Renderer` is not exported, so the capture loads a **patched in-memory copy** of
   `dist/render.js` (source text + `export { Renderer }`, relative imports rewritten to absolute file URLs, evaluated
   from a `data:` URL; the frozen tree is never written) and wraps `layoutTitle`, `layoutLegend`, `legendEntries`,
   `prepValueAxis`, `prepCatAxis`, `measureAxis`, `plotRect`, `dateLabels`, `valueLabels`, `trendCurve`, `hierarchy`
   to log arguments and results. The op facet still runs the unpatched module.
5. **Bounds facet**: `chartPartBounds` of `slides-core/dist/chart-engine.js` for the selections of 2.
6. **Differential facets**: `autoScale`/`tickValues`/`fitTrend`/`histogramBins` on seeded random inputs (extremes,
   NaN, huge spans, tiny units).

Output like the B captures: `tests/captures/charts-<group>.{capture,ops.gz,measures.tsv.gz,layout.jsonl.gz}`, run
twice byte-identical, committed CI subset < 10 MB. Replay `tests/parity.rs`: XML parsed by `uo-opc` with
`CHART_PREFIXES`, host and measurer from the logs, every facet compared exactly (`RecCanvas` prints the same `draw`
lines natively); then (C-6) the product path: Rust → `SlidesList` → `paint-slides.ts` → recorder, compared with the
same `ops.gz` by a `compare-chartops.mjs` modelled on `compare-drawops.mjs`.

### Generator (`core/crates/uo-charts/js/gen/charts.mjs`, groups `c1a` CI 600 charts, `c1-big` 5,000 local, `c1d` decks)

The lost `gen-charts.cjs` redone from the rules of `charts.test.ts` (350 lines, 30 cases), the two `CORPUS-REPORT.md`
chart paragraphs and the memory `chart-oracle-facts`. Seeded per chart (`seedBase` disjoint from the B groups, e.g.
700000), raw `c:chartSpace`/`cx:chartSpace` text:

- **Families**: bar/col × clustered/stacked/percentStacked × gapWidth/overlap × invertIfNegative × `bar3DChart`
  (shape, gapDepth, view3D rAngAx 0/1, rotX/rotY, depthPercent); line × smooth absent/0/1 × markers (9 symbols, sizes,
  none) × stacked/percent × drop/hiLow lines × upDownBars, `line3DChart`; area × standard/stacked/percent, 3D; pie/
  `pie3DChart`/doughnut (holeSize) × explosion (series/point) × firstSliceAng × varyColors; ofPie pie/bar ×
  splitType pos/val/percent/cust/auto × secondPieSize × serLines; scatter × the 5 scatterStyles × numeric/blank x;
  bubble × bubbleScale × bubble3D; radar standard/marker/filled; stock (HLC, OHLC with upDownBars); surface (axes
  only); combos (bar + line on a secondary axis, area + line); `c:errBars` present (must be ignored).
- **Axes**: positions incl. secondary r/t, crosses autoZero/min/max/crossesAt, maxMin, logBase (incl. out of 2–1000),
  min/max/majorUnit/minorUnit (incl. absurd), dispUnits built-in/custom, date axes (baseTimeUnit, majorTimeUnit +
  majorUnit), multi-level categories × noMultiLvlLbl, tickLblSkip/tickMarkSkip, tickLblPos, tick marks, major/minor
  grids, axis titles with rotation, deleted axes, `fixAxes` cases (no axes, one axis).
- **Labels and text**: dLbls show* combinations × pos (ctr, inEnd, inBase, outEnd, l, r, t, b, bestFit) × numFmt from
  the `uo-numfmt` corpus (linked/unlinked, `m/d/yyyy`, %, currency, `[Red]`) × sep × per-point `c:dLbl` with rich
  fields (CATEGORYNAME, VALUE, SERIESNAME, PERCENTAGE, CELLRANGE), manual layout, `c15:layout` size, leader lines;
  `c:pt formatCode`; titles (rich with/without defRPr, strRef, automatic, deleted, overlay, manual layout); legend
  (every pos, overlay, manual layout with/without w/h, deleted entries, long labels → table and wrap); dTable flags;
  txPr (sz, b, i, u, colour, font incl. `+mn-lt`, rot, vert); long words, `-` breaks, `\n`, astral characters.
- **Styles and paints**: `c:style` 1–48 and `c14:style` 101–148, roundedCorners, `office2010`; spPr solid/alpha,
  gradients lin/path with unsorted and NaN stops, patterns, picture stretch/fillRect/tile (algn, flips, scales; PNG
  and JPEG with pHYs/JFIF/EXIF dpi), noFill, dashes, caps, `w="0"`, outer shadows; chart drawings (rel/abs anchors,
  groups, the five geometries, text wrap/anchor/clip/rot, pictures).
- **Data**: 1–30 series (colour wraps past 6 and 12), 1–200 categories, blanks × dispBlanksAs, negative, zero-sum,
  NaN text, 1e300, all equal, idx gaps and duplicates, `ptCount` above 100,000; trendlines (6 kinds, order 2–6,
  period, forward/backward, intercept, dispEq/dispRSqr, label numFmt).
- **Chartex**: waterfall (subtotals), funnel, histogram (binSize, binCount, underflow/overflow, intervalClosed) and
  Pareto, boxWhisker (mean line/marker, outliers, exclusive/inclusive quartiles), treemap/sunburst (parentLabel
  overlapping/banner/none, ragged levels), regionMap (title only), unknown layoutId.
- **Hosts**: `ppt`/`xl` profiles × fileFormat; `c1d` wraps 150 of them in decks (one chart per slide, with and without
  `themeOverride` and chart images) for the Slides host path (C-6) and the PowerPoint oracle.

## C-0 — harness (done 2026-11-17, Sonnet)

What exists (all under `core/crates/uo-charts/`; the TS reference is the frozen 0.9.2 tree, `UO_TS_ROOT` or `--ts-root`):

| File | Role |
|---|---|
| `js/capture-charts.mjs` | the capture (modes `--gen c1a`, `--samples`, `--diff`; options `--out --only --max --count --ops-max --detail`) |
| `js/lib/ts-charts.mjs` | loaders of the frozen modules; the two host bridges (`parseChartPpt` = slides-core `parseChart`, `parseChartXl` = sheet-core `parseChart` without `refreshChartData`) with a recording `ChartHost`; the patched in-memory copy of `render.js` (`export { Renderer }`, nothing written to the frozen tree) |
| `js/lib/font-measure.mjs` | the deterministic measurer (copy of B3's open-font rule: hmtx + GPOS/kern, substitutes as B3) plus the `'Hg'` font box (hhea ascender/descender) |
| `js/lib/diff.mjs` | the differential cases (`niceStep`, `autoScale`, `tickValues`, `frac`, `fitTrend`, `trendNum`, `histogramBins`), seed 790000 |
| `js/gen/charts.mjs`, `js/gen/xml.mjs` | the seeded generator (group `c1a`, seed base 700000) and XML/PNG/JPEG builders |
| `js/gen-rec-selftest.mjs` | writes `tests/captures/rec-selftest.txt` (the `RecCanvas` self-test cases with the recorder's output) |
| `tests/parity.rs` | the replay (`c1a_capture`, `samples_capture`, `diff_capture`, `rec_canvas_matches_the_recorder`, digest and JSON unit tests; `UO_CHARTS_STEM=<group>` replays another group) |
| `tests/support/{data,digest,facets,json,rec,selftest,stages}.rs` | capture readers, SHA-1/SHA-256 digests as the JS writes them, the facet driver, `RecCanvas`, the stage plug points |
| `tests/captures/charts-{c1a,samples,diff}.*`, `rec-selftest.txt` | the committed captures (4.5 MB: `c1a` .capture 779 KB, .data 992 KB, .measures 857 KB, .ops 1.05 MB and .layout 82 KB for the first 80 charts; `samples` 113 KB; `diff` 714 KB) |

Commands (all from the repository root; `core/target/c0` is only the cargo target):

```text
node core/crates/uo-charts/js/capture-charts.mjs --gen c1a      # 55–62 s  → tests/captures/charts-c1a.*
node core/crates/uo-charts/js/capture-charts.mjs --samples      # 3.7 s    → charts-samples.*
node core/crates/uo-charts/js/capture-charts.mjs --diff         # 0.3 s    → charts-diff.*
node core/crates/uo-charts/js/gen-rec-selftest.mjs              # rec-selftest.txt
node core/crates/uo-charts/js/capture-charts.mjs --gen c1a --ops-max 9999 --out D:/tmp/charts-c1a   # local: all 600 op streams (13.5 MB .ops.gz) and layout lines (5.3 MB)
node core/crates/uo-charts/js/capture-charts.mjs --gen c1a --count 5000 --out D:/tmp/c1-big          # local, the `c1-big` size (not committed; ~8 min)
CARGO_TARGET_DIR=…/core/target/c0 cargo test -j 4 -p uo-charts --test parity -- --test-threads=4 --nocapture
```

Every capture was run twice and is byte-identical (also the gzip files; no `@platform` difference on this machine). The
frozen bridges are COPIES of the 20 lines of each `parseChart`; each chart checks the model of the copy against the real
`parseChart` of slides-core / sheet-core (throws on a drift). The recorder gained (opt-in capabilities, so the Slides
captures regenerate byte for byte — `drawops-samples.*` re-run and compared, `selftest-drawops.mjs` 12/12): `roundRect`
(`caps.roundRect`: device-space corners, `r`, the linear part), the canvas rule "an invalid font string is ignored"
(`caps.strictFont`: size token `NaN`, `Infinity` or negative) and `fontBoundingBoxAscent/Descent` taken from the measurer's
result when it gives them. Finding: the sample deck `showcase_animado.pptx` slide 4 holds a chart with rounded corners; in the
committed `drawops-samples` stream that chart's shape throws at the missing `roundRect` (109 lines instead of 261) — C-6/B4
must expect the stream to change when the recorder of the chart layer is used (`caps.roundRect`).

### What a capture holds

Per chart (key `gen/c1a/NNNN` or `samples/<file>#c<n>` / `samples/<book>#chartN.xml`), facets in `.capture`:

| Facet | Value | Frontier |
|---|---|---|
| `input`, `profile`, `seed`, `feat`, `src`, `also` | XML digest; `<host>/<fileFormat>/<theme>`; generator metadata (families, ~214 feature names) | meta |
| `parse`, `model`, `dropped` | `ok`/`err:`; `len:sha256[0..32]` of the canonical JSON of the `ChartSpace` (`canon-json.mjs` rules, `themeFill` as the boolean `true`); what the canonical JSON dropped (nothing in the 626 charts) | C-1 |
| `host`, `hostx` | `n:sha256[0..16]` of the host call log of the parse (ordered) / of the paint-time `themeFill` calls (deduplicated) | C-1, C-3 |
| `ops.<view>.<sel>` | `lines:sha1[0..16]` of the `draw` form of `paintChartSpace` over `record-ctx.mjs`, or `err:<message>` | C-3…C-5 |
| `ask.<view>.<sel>` | `n:sha256[0..16]` of the sorted distinct `measureText` keys (`<font string>TAB<JSON text>`) | C-3…C-5 |
| `bounds.<view>.<sel>` | canonical JSON of `chartPartBounds`, `null`, or `err:` (Slides profile only) | C-3 |
| `lay.<view>`, `lay.<view>.<method>` | `n:sha256[0..16]` of the layout facets of the patched `Renderer`: `layoutTitle layoutLegend legendEntries prepValueAxis prepCatAxis measureAxis plotRect dateLabels valueLabels trendCurve hierarchy`, each as `canonJson({args, res})` after the call (model objects as `{"$ref":"g0.s1"}`/`ax2`, `undefined` as `{"$undef":1}`) | C-3…C-5 |
| `diff/<fn>` · `<index>` | digest of the result of one differential case (arguments and result texts in `charts-diff.cases.jsonl.gz`) | C-2 |

Views: `a` = (0, 0, 480 × 300) at 4/3 px per pt; `b` = (17.5, 9.25, 360.5 × 240.25) at 0.8833. Selections (`sel_of` in
`tests/support/stages.rs`): `full` (no `sel`), `bg` ({bg, data: null}), `none`, `s0`, `c1`, `p02`, `not-s0`. Every chart: view a
`full` + `bg`, view b `full`; every third chart and every sample chart: all selections on view a, `bg` on b, the layout facets
also on view b. Options per host: `ppt` = `defaultAreaFill none`, the Slides stack; `xl` = `bg1`, `defaultAreaLine auto2010`,
`excelAxisDensity`, the Sheet stack. Fonts are `[italic ][bold ]<toFixed(2)>px 'F', …` with the stack of the host (the recorder
prints it in `font="…"`; `RecCanvas::ppt_stack/xl_stack`).

Sidecars: `.data.jsonl.gz` one JSON line per chart: `key, host, fileFormat, theme, noTheme, xml, userShapes, images [{rid, part, w, h,
wPt, hPt, sha}], fonts {major, minor}, themeLn, hostLog, hostPaint`; the host answers are the log (`kind`, `key`, `ans`): the key of
`color`/`blip` is the canonical JSON of the element (`{"attrs":{sorted},"children":[…],"name":…}`, `uo_charts::canon::element_json`),
of `fill` `<idx>|<phClr>`; the answer is the canonical JSON text or `null`. `.measures.tsv.gz`: `"font"TAB"text"TAB width TAB ascent TAB
descent` (JSON-quoted, lone surrogates as U+FFFD), one table per group, a pure function of the pair. `.ops.gz`: `#chart <key>`,
`#view <v> sel=<s> … lines=<n> sha=<h>`, optional `#error`, the lines. `.layout.jsonl.gz`: `key TAB view TAB seq TAB method TAB json`.
Only the first 80 charts keep the stream and layout text (the facets carry the digests of all; `--ops-max`).

### Numbers (committed run)

| Group | Charts | Facets | Op streams (draw lines) | Bounds | Layout calls | Parse host calls | Distinct measure keys |
|---|---|---|---|---|---|---|---|
| `c1a` | 600 (0 parse errors; 12 throw at `ops.a.full`, 43 of 3,000 streams, 32 of 1,960 bounds — the NaN gradient stop `IndexSizeError`) | 20,097 | 3,000 (5,303,189) | 1,960 | 13,889 | 13,967 | 236,079 |
| `samples` | 26 (23 deck charts of 5 decks, 3 workbook charts; the `.ppt` samples add none that the `.pptx` twins lack) | 1,267 | 234 (12,519) | 184 | 772 | 455 | 228 |
| `diff` | 2,760 cases: niceStep 400, autoScale 700, tickValues 500, frac 300, fitTrend 400, trendNum 160, histogramBins 300 | 2,760 | — | — | — | — | — |

Profiles in `c1a`: `ppt` 420 (modern 210, mso2007 111, unknown 99), `xl` 180 (modern 105, mso2007 39, unknown 36). 45 families (bar/col
clustered/stacked/percent, bar3D shapes, line variants, line3D, area, area3D, pie/pie3D/doughnut, ofPie ×5 splits, scatter ×5 styles,
bubble, radar ×3, stock ×3, surface, combos incl. secondary axes and bar+scatter, 13 edge cases, the nine chartex layouts); 214
features, each hit by at least one chart. Not covered: error bars are generated but the TS ignores them (as the record says);
`regionMap` has the title only; the `c1d` deck group (charts inside decks, `themeOverride`, the PowerPoint oracle) is NOT built
here — C-6 builds it from this generator (`buildChart` gives the XML, `images`, `themeName`; the deck wrapper is
`uo-pptx/js/lib/gen/b1.mjs`).

### Replay and the plug points

`tests/parity.rs` replays each group through `support/facets.rs`: a facet is compared when something can compute it now, else
skipped and counted per stage (never failed). Today: `input`, `profile`, `host`, `hostx` (recomputed from the sidecar), the op streams
kept in `.ops.gz` (digest of the stored lines) and the differential sidecar are compared (c1a 2,802, samples 338, diff 2,760); the rest
is skipped by stage (c1a 15,495: 1,800 C-1 — replayed by `tests/c1_replay.rs` —, 7,558 emit, 6,137 layout). A frontier plugs in by
replacing a body in `tests/support/stages.rs`:

- **C-1**: `tests/c1_replay.rs` already replays `model`/`host`; it needs the generator's theme fonts and line widths, which the data
  lines now carry (`fonts`, `themeLn`). Its `RHost` should move into `tests/support` for C-3.
- **C-2**: `stages::diff(case)` → the canonical JSON text of the result from `case.args` (`fitTrend`: the fit minus `f`, plus `at`
  `[[x, f(x) | "undefined"]…]` at the probe points; `histogramBins` takes the `cx:layoutPr` node as canonical JSON).
- **C-3…C-5**: `stages::ops(chart, table, view, sel)`: parse with the host of `c1_replay.rs` (`BlipKey(i)` = index into
  `ChartData::images`), build `EmitParams` from `view()`, `sel_of()`, `profile_of()`, `MapMeasure` (a `ChartMeasure` over the
  measurement log: it rebuilds the TS font string from face and size), run `emit_chart` over `rec_canvas(chart)` (a `RecCanvas` with
  the host's stack and the pictures), loop on `ChartError::NeedMeasures` (the table is complete, so a miss is a real difference;
  `MapMeasure::misses` lists them), return the lines, the asked keys and the error. `stages::bounds` and `stages::layout` likewise.
  A tiled picture sets `RecCanvas::unsupported` (the TS draws it through `tilePattern` on an offscreen canvas: `@cvK` lines and a
  `canvas(…)` source — the shell painter's job, C-6): compare those facets after C-6 or accept them by name.

`RecCanvas` is verified against the frozen recorder on 5 hand-written call sequences (159 lines: transforms, rects, strokes with dash/cap/join/
shadow, arcs, ellipse, `roundRect`, clips, text with an invalid font ignored, gradients, a stretched picture, `drawImage`): equal
at the first run. `src/` was not touched (no test-only helper was needed).

Process metrics: 1 agent session; capture of `c1a` 55–62 s wall (3,000 op streams, 13,889 layout calls, patched-copy paints), samples 3.7 s,
diff 0.3 s; the parity test 2.1 s in the debug profile (compile of the test target 3 s after the dependencies).

## C-1 — model, parser, colours (done 2026-11-17, Sonnet)

What exists (`core/crates/uo-charts/`):

| File | Role (TS) |
|---|---|
| `src/parse/mod.rs` | `parse.ts`: `Parser` (paint, blipFill, line, shape, runProps, textProps, rich, layout, title, numFmt, dLbls, marker, nums/strs/ptStrings, series, group, axis, legend, `userShapes`), `parse_chart_space`, `parse_paint`, `style_of`, `fix_axes`; the order of the host calls is the TS order |
| `src/parse/xml.rs` | `xml.ts` helpers over `uo_opc::xml::Element` (`val_num`, `val_str`, `first`, `descend`, `truthy`) and `CHART_PREFIXES` (the `canonicalNs` table for `uo-opc`'s `CanonicalPrefixes { default_namespace: true }`) |
| `src/color.rs` | `color.ts` (`hex_to_rgb`, `rgb_to_hex`, `adjust_lum`, `mix_white`, `tint_linear`, `rgba`, `chart_shade_tint`) and `dml-color.ts` (`dml_color`, `Scheme`) |
| `src/summary.rs` | `summarize` of `chart-engine.ts` (`ChartSummary`, `ChartType`) |
| `src/chartex.rs` | the hook `parse_chart_ex(root, &Parser, cs)` the parser calls for `cx:chartSpace`; **returns the base model until C-2** |
| `src/canon.rs` (feature `canon`) | the canonical JSON printer of the parity captures (`Canon::chart_space`, `element_json`: the host-log key of an element); enabled for `cargo test` through the self dev-dependency, as `uo-pptx` |
| `tests/c1_parse.rs` | 21 unit tests: the expected values of `charts.test.ts` (parse, colours, file-format rules, 3D, tables, titles, picture fills, chart drawing, theme widths, point labels) and the traps below |
| `tests/c1_replay.rs`, `tests/c1_support/` | the replay of the `model` and `host` facets of `charts-c1a` and `charts-samples` (host answered from the capture's log, fonts and `themeLn` from the data line) |
| `tests/c1_memory.rs`, `tests/c1_nopanic.rs` | M1/M2/M3 and the bounded no-panic over mutated charts, hostile drawings and extremes |
| `core/fuzz/src/charts.rs`, `fuzz_targets/chart_parse.rs` | fuzz target `chart_parse` (seeds in `src/bin/seed.rs`, entry in `TARGETS` and `Cargo.toml`) |

### Parity

`tests/c1_replay.rs` against the committed captures: **`charts-c1a` 483 charts, 1,449 facets equal (`parse`, `model`, `host` × 483), 0
mismatches; `charts-samples` 26 charts, 78 equal, 0 mismatches** (also on the full 600-chart run, local). The 117 `cx:chartSpace` charts of
`c1a` are skipped until C-2 (their `model`/`host` facets need `chartex`). The model digest is the canonical JSON of the whole
`ChartSpace` (every colour, every `Map`/`Set` in insertion order, `ptFmts` holes as `null`, NaN as `{"$num":"NaN"}`), so the 483
digests prove every field the generator reaches (214 features) and the order of the host calls (`host` facet = ordered digest).
`parity.rs` (C-0) counts the 1,800 C-1 facets as replayed here.

### Memory

- **M1**: `parse_chart_space` (3 host profiles × the built-in corpus: 3 bar charts of 4/120/800 points, pie, scatter, line, empty, `cx:`),
  `summarize`, `parse_paint`/`parse_paint_node` over every element, `dml_color`, the colour functions, `clear_caches`: live bytes
  return exactly (tolerance 0). One parse of 3 series × 40 points allocates 49 KB, peak 32 KB.
- **M2**: 2,000 parse + summarize cycles over the corpus × 3 hosts: flat from the second sample (tolerance 0).
- **M3**: retained `ChartSpace` heap / chart XML bytes, median over the 471 non-chartex charts of `c1a` + the built-in corpus: **1.76**
  (target ≤ 2); maximum 628 for a chart whose `c:ptCount` is 100,000 with one point (the TS also builds the 100,000-element arrays: the cap
  of 100,000 is the bound). The first measurement was 2.71 (median) with the skeleton types: 2.35 after boxing `ShapeProps::line`/`shadow` and the
  `Paint::Blip` payload, 1.76 after boxing the rare big parts (`Series::d_lbls`, `Series::marker`, `DataPoint::marker`, `Group::d_lbls`,
  `Axis::title`). `size_of`: `ShapeProps` 72 B (was 344), `Series` 824 (1,912), `Group` 1,008 (2,912), `Axis` 1,000 (2,088), `TextProps` 112,
  `DLblsCommon` 320 (864). Remaining compaction if C-5 needs it: `TextProps` (three `Option<f64>`).
- **M5**: the engine keeps no cache (`clear_caches` is empty); nothing to cap.
- **No-panic** (`c1_nopanic`, 4 s): 1,909 mutated charts (hostile values `NaN`, `1e308`, `100000`, `0x10`, an emoji…, element deletion,
  duplication, renaming) × 3 hosts, every element as a paint parent/node/colour: no panic, caps respected; a chart drawing with 20,000 nested
  groups returns (the recursion stops at 128); 5,000 gradient stops sorted; `ptCount` 1e9 and 99,999,999,999 capped at 100,000.
- **Fuzz** `chart_parse` (stable driver, debug profile, 40 s, seeds: 5 chart families × 4 host flags): 1,325,041 executions, 0 crashes, slowest
  22 ms. Invariants: caps, series sorted by `order`, every group's axes exist, the parse is deterministic (compared through the canonical
  JSON, NaN-safe).

### Decisions outside the source semantics (all declared)

1. **Types changed from the skeleton** (for M3, same fields and meaning): `ShapeProps::line: Option<Box<LineProps>>`, `shadow:
   Option<Box<Shadow>>`; `Paint::Blip { tile: Option<Box<BlipTile>>, fill_rect: Option<Box<FillRect>> }`; `Series::d_lbls`/`marker`,
   `DataPoint::marker`, `Group::d_lbls`: `Option<Box<…>>`; `Axis::title: Option<Box<ChartTitle>>`. Added helpers: `ChartColor::new/as_str`,
   `FmtCode::new/units`, `JsNumKey::new/value`, `PointMap::{set, get, len}`, `PointSet::{add, has}`, `TextProps::{is_empty, over}` (the
   TS spread `{ ...a, ...b }` is a field-wise `or`: the TS parser never stores an `undefined` key).
2. **`parse_paint(el, color)`** takes the element whose CHILDREN hold the fill (as the TS) and is joined by **`parse_paint_node(el, color)`**
   for a single fill element: Rust cannot synthesise the wrapper `{ name: 'w', children: [el] }` the Slides/Sheet bridges build around an entry
   of the theme's `fillStyleLst` (C-6 uses it in `ChartHost::fill_style`).
3. **`dml_color(el, &Scheme)`** plus **`dml_color_source(ColorSource, &Scheme)`**: the parser also asks the resolver for the synthetic
   `<a:schemeClr val="accentN"/>` (`ColorSource::Scheme`), which a Sheet host answers with `dml_color_source`. `Scheme` is a small ordered
   map (`Record<string, string>`); the TS lookup also sees `Object.prototype` names (`scheme['constructor']`): not ported.
4. **Group nesting of the chart drawing** is followed to `MAX_GROUP_DEPTH` = 128 (the TS recurses without a cap and would overflow the
   stack on a hostile file); deeper groups are skipped.
5. **Lone surrogates**: `slice(-6)` of `a:srgbClr val` cuts UTF-16 units; a cut pair becomes U+FFFD (a Rust string cannot hold it). Colour
   strings that are not `#RRGGBB` (`#abc`, `#NANNANNAN` from a hostile modifier) are kept as the TS prints them (`ChartColor::Other`).
6. **`cx:chartSpace`**: `chartex.rs` is only the hook, with the base model, until C-2.
7. `canon.rs` has its own `Json` (the same rules as `uo_dml_render::canon`): CH-3 forbids the dependency in either direction.
8. `Parser`'s constructor and readers are `pub(crate)` (C-2 reads `cx:` parts with them: `bool_of`, `shape`, `text_props`, `rich`,
   `num_fmt`, `layout`, `paint`, `color`); the public entry points are `parse_chart_space` and `parse_paint*`.

### What C-2 / C-3 need

- **C-2** (`chartex.rs`): fill in `parse_chart_ex(root, &Parser, base)`; it gets the base model with `accents`/`tx1`/`bg1`/fonts/`sp`/`tx_pr`
  of the root already set and calls the `Parser` readers (the TS `P`). The `model`/`host` facets of the 117 `cx:` charts of `c1a` are replayed by
  `tests/c1_replay.rs` as soon as the `xml.contains("<cx:chartSpace")` skip is removed. `Group::parent_label`, `Series::box_opts`, `GroupKind::{Box,
  Treemap, Sunburst}` and `DataPoint::bubble_3d` exist in the model and in the canonical printer.
- **C-3**: `RHost` of `c1_replay.rs` (host answered from the log; `BlipKey(i)` indexes the blip names it interns) moves into `tests/support` as C-0
  planned; `ChartSpace` is immutable and `Arc`-shareable; `has_theme_fill` says whether the emit may call `ChartHost::fill_style`; the colour
  helpers for the automatic colours are `uo_charts::color::*` (`chart_shade_tint`, `tint_linear`, `adjust_lum`, `mix_white`, `rgba`,
  `hex_to_rgb`, `rgb_to_hex`; hex strings upper case, NaN prints as the TS does); `PointMap::get(idx)` answers `dPts.get(i)`/`dLbls.points.get(i)`.

Process metrics: 1 agent session, about 45 min of wall time (reading the TS and records 15 min, port 20 min, tests and replay against
the capture 15 min, fuzz, lint and record 10 min). Capture runs used: `--gen c1a` 53 s, `--samples` 3.9 s; the replay of both groups
1.7 s in the debug profile; `c1_nopanic` 4.6 s; the fuzz run 42 s. Clippy native (`--all-targets`) and wasm32 (lib) clean, `fmt` clean,
`grep-bans.sh` clean. The port was faithful on the first replay except the `cx:` charts, which are C-2 (0 parity rounds).

## C-2 — chartex, scale, trendlines, dpi (done 2026-11-17, Sonnet)

Files (`core/crates/uo-charts/`): `src/chartex.rs` (`parse_chart_ex`, `histogram_bins`), `src/scale.rs` (`nice_step`, `auto_scale`, `tick_values`, `frac`,
`Fixed`), `src/trend.rs` (`fit_trend`, `TrendFit::eval` = the TS closure `f`, `trend_num`, `TrendOpts`), `src/image.rs` (`image_dpi`);
tests `c2_unit.rs` (14: the expectations of `charts.test.ts` for scale, trend, dpi, chartex, plus the traps), `c2_gate.rs` (M1, M2, no-panic),
`tests/support/diff_cases.rs` (the `stages::diff` plug).

### Parity

- `charts-diff`: **2,760 cases, 2,757 equal, 3 accepted by name, 0 mismatches** (niceStep 400, autoScale 700, tickValues 500, frac 300, fitTrend 400,
  trendNum 160, histogramBins 300). The 3 are ONE ULP in a `Math.pow`: `autoScale` 187 is `3 ** 34`, an exact tie (UCRT `std::pow`, which the capture's
  Node used on Windows, rounds to ...570; the glibc 2.39 pow that `uo-jsnum` pins gives ...568), and `fitTrend` 212 and 374 read `x ** b` of the power
  fit. They are listed in `POW_PLATFORM` of `tests/support/facets.rs` and recorded as `accepted` with the reason (the `uo-jsnum` record G1 measured 146 in
  400k different between the two libms). No tolerance anywhere.
- `charts-c1a`: the 117 `cx:` charts replay with the 483 others: **600 charts, 1,800 facets (`parse`, `model`, `host`) equal, 0 mismatches** (the
  `<cx:chartSpace` skip of `tests/c1_replay.rs` is gone); `charts-samples` 78 equal. First run, 0 parity rounds.

### Memory and robustness

M1 (every entry point, tolerance 0), M2 (2,000 cycles flat), no-panic (20,000 scale cases, 2,000 trend sets, 1,500 histograms with hostile bins, 2,000 image
byte strings; seconds). Fuzz `chart_scale` (`core/fuzz/src/charts_render.rs`: modes scale / trend / histogram / image dpi): 1.17M executions, 0 crashes.
The first fuzz run found a 90 s execution (a histogram of 1e6 bins formatted label by label in the debug profile): `MAX_BINS` is now 100,000.

### Decisions outside the source semantics (declared)

1. `histogram_bins` and `fit_trend` return `Result<_, JsErrorKind>`: a bin count that is not an array length (NaN, fraction, > `MAX_BINS` = 100,000) and a
   polynomial `order` of NaN (`new Array(NaN)`) are the TS RangeError. In `parse_chart_ex` a failed histogram is an EMPTY histogram (the TS would throw and
   the host skips the chart; `parse_chart_space` is infallible).
2. chartex arrays: a negative `ptCount` is 0 (the TS `new Array(-1)` throws); a `pt` whose `idx` is past `ptCount` fills the hole with `""`/`null` (the TS holds a
   hole, which the model cannot).
3. `image_dpi` reads past the end as 0, which is what the TS bitwise `undefined` is; offsets are `u64` (wasm32 safe).
4. The `fixed.log` / `unit` truthiness tests keep JS rules (`0`, NaN falsy); `Scale::log: Option<f64>`.

What C-4 needs: `scale::{auto_scale, tick_values, frac}` with the TS call order, `trend::fit_trend` for `trendCurve`; chartex groups/axes are complete.

Process metrics: 1 agent session; port + replay 45 min wall; the diff replay 0.8 s and the c1a replay 1.8 s (debug); fuzz 2 x 25-40 s.

## C-3 — render core (done 2026-11-17, Sonnet)

Files: `src/render/mod.rs` (`Renderer`: setup, `text`, `measure`, `wrap` with the CH-1 rule, `draw_lines`, `set_fill` / `set_blip` / `with_fill_alpha`, `set_line`,
`shape_rect`, automatic colours and styles, `marker_of`/`paint_marker`/`series_line_w`/`trend_line`/`trend_name`, `paint`; the API `emit_chart`, `need_measures`,
`part_bounds`, `trace_layout`, `emit_envelope`), `render/cv.rs` (the `Canvas` state wrapper), `render/canvases.rs` (`NullCanvas`, `BoundsCanvas`), `render/title.rs`,
`render/legend.rs` (+ `LayoutTrace`), `render/user.rs` (chart drawing shapes and their text). Harness: `tests/support/host.rs` (`RHost`: the replay host answering from the
parse log AND the paint-phase `themeFill` log; `BlipKey(i)` = index in `ChartData::images`), `layout_json.rs`, the `ops`/`bounds`/`layout`/`envelope` stages, `tests/c3_gate.rs`.

The family painters (`paintAxesChart` C-4, `paintPies`/`paintHierarchy`/`paintRadar` C-5) plug into `Renderer::paint_body`; until then it answers
`ChartError::NotPorted`, AFTER the title and legend have been laid out, so the layout facets of those two are complete for every chart.

### Parity (equal / compared, 0 mismatches in every stage)

| Stage | `charts-c1a` | `charts-samples` |
|---|---|---|
| `lay.<v>.layoutTitle/layoutLegend/legendEntries` (C-3 layout) | **2,245** | **156** |
| `ops`/`ask`/`bounds` (charts that need no family painter: no group, regionMap; the 12 gradient-stop throws among them) | **928** | 234 |
| envelope check: the lines before and after the family body of the KEPT TS streams | 303 streams | 234 streams |
| skipped and counted (family painters): emit 7,032 (c1a) / 418 (samples), layout of axes/hierarchy/trend 3,092 / 228 | | |

Compared / total per family in `c1a` (facets of ops, ask, bounds, layout): area 212/1039, area3D 47/250, bar 448/2302, bar3D 100/552, bubble 44/202, combo 242/1185,
cx 768/2645, doughnut 44/150, edge 194/688, line 326/1884, line3D 47/228, ofPie 44/168, pie 285/995, pie3D 44/150, radar 52/167, scatter 138/716, stock 94/564,
surface 44/212. In `samples` every deck and workbook chart compares its 156 layout facets (title, legend) and its 234 envelope streams.

The ENVELOPE check verifies the legend, title, frame and chart-drawing OP STREAMS of charts whose body is not ported: `emit_envelope` replaces the family painter by one marker
`fillText`; the frozen stream must start with the lines before it and end with the lines after it. Finding: the family body leaves `ctx.lineJoin = 'round'` set (never restored,
`setLine` does not touch it), so the legend strokes after it carry `join=round`; the check normalises that one token and the C-4 body will produce it itself. `lay.<v>` (all calls)
and the axes' methods are skipped until C-4.

### CH-1 measured (the wrap prefetch; `c3_gate::ch1_round_counts`, 1,200 paints of `c1a`, views a and b, the capture table as oracle)

| | rounds 0 | 1 | 2 | 3 | 4 | 5 | 11 | max | questions asked |
|---|---|---|---|---|---|---|---|---|---|
| with the rule | 102 | 1,078 | 20 | - | - | - | - | **2** | 11,521 |
| without | 102 | 1,068 | 12 | 13 | 2 | 1 | 2 | **11** | 10,010 |

The rule costs +15 % questions and bounds the rounds by 2 for this scope (the second round is the legend box `wrap` of a manual legend that is only reached once the first widths are
known: a data dependency, not a wrap chain). `MAX_ROUNDS` 8 holds. Axes labels (C-4) are the next place to re-measure.

### Memory

M1: `emit_chart`, `emit_envelope`, `need_measures`, `part_bounds`, `trace_layout` (also hostile answers, `wrap` prefetch on and off) return live bytes exactly; M2: 2,000 cycles flat
(tolerance 0); one emit of the built-in corpus peaks at 1,713 B (the engine keeps no list; the host owns the display list). M5: the part-bounds LRU is the HOST's per-model cache (64
keys, `sel.key@x,y,w,h,ppt` with `toFixed(2)`): `part_bounds` is pure and keeps nothing, so the cap is tested where the cache lives (C-6). No-panic: 5 s of mutated charts x hostile rects
(NaN, infinite, 1e308, negative, zero px/pt) x hostile measure answers (NaN, 0, 1e308, negative, missing), emit/envelope/bounds/need_measures: 2,244 emits, 5 throws, 574 need-measures,
no panic. Fuzz `chart_emit`: 2.0M executions, 0 crashes, slowest 31 ms (bound: canvas calls <= 20,000 + 400 x (points + series + shapes)).

### Decisions outside the source semantics (declared)

1. `ChartError::Throws(kind, message)` carries the recording canvas's message (`IndexSizeError: color stop offset`, `RangeError: negative radius`, the engine's own TypeErrors); the parity
   facets compare it. New variant `ChartError::NotPorted(&str)` (removed when C-5 lands).
2. CT-1 (replaces A4): `CanvasTile` carries the TS arguments as they are (`nat_w = (wPt ?? w * 0.75) * S`, `tx = t.tx * S`, `iw`, `ih`, `sx`, `sy`, `algn`, `flip`, box): the painter calls
   its `tilePattern` with them, no denominators, so the bits are the TS's.
3. A negative `arc`/`ellipse`/`roundRect` radius and a stop offset outside 0..1 are checked in the wrapper before the canvas call (the Canvas trait only forwards).
4. `BoundsCanvas`: the `while (e < s) e += 2*PI` loop and the quadrant loop stop after 100,000 turns (the TS spins on angles around 1e16).
5. `part_bounds` forces `images: false` and the PowerPoint profile (no frame line, transparent area), as `paintChart` is called there.
6. A legend column count that comes out NaN (a NaN measure answer) is 1.
7. The empty-family font of `Cv` = the inherited `10px sans-serif` (`MapMeasure::font_string`), CH-5.

What C-4/C-5/C-6 need: C-4/C-5 fill `Renderer::paint_body` (`cv`, `text`, `measure`, `wrap`, `set_fill`, `set_line`, `shape_rect`, `auto_color`, `point_fill`, `series_color`,
`marker_of`/`paint_marker`, `plot_default`, `show_data`, `format_text`, `draw_lines` are already ported; `fontK` needs `ChartMeasure::font_box`), remove `NotPorted` and the envelope
normalisation, add the axes' layout calls to `LayoutCall`/`layout_json.rs` and the stage `layout` match; the per-family tally printed by `parity.rs` shows which families turn from
skipped to compared. C-6 needs the Slides painter for `CanvasPaint::Tile` (CT-1), `chartFont`, `fillText`, `roundRect` (R4/R5), the per-model bounds cache, and a host over the real theme.

Process metrics: 1 agent session (C-2 + C-3), about 2 h of wall time; the C-3 port needed 0 parity rounds (the first replay of the layout and ops facets was 0 mismatches; the envelope
check found the lineJoin leak, a property of the frozen stream). clippy native (`--all-targets --all-features`) and wasm32 (lib) clean, `fmt` clean, `grep-bans.sh` clean.

## C-4 — axes and the XY families (done 2026-11-17, Sonnet)

Files (`core/crates/uo-charts/src/render/`): `axes.rs` (`paintAxesChart`; the axis infos; `prepValueAxis`, `prepCatAxis`, `dateLabels`, `valueLabels`, `fontK`, tick sizes,
`plotRect`, `measureAxis`, `pos`/`crossPos`, gridlines, axes, axis labels and titles, the data table, 3D walls/floor/depth, `trendData`/`trendCurve`), `xy.rs` (bars and the 3D
box, `pt`, lines with smooth paths, stock lines, up/down bars, drop lines, trendlines with their equation labels, areas, scatter, bubbles), `util.rs` (the helpers at the end of
`render.ts`: `pickFmt`, `scaleRgb`, `mixWhite`, `minorLog`, `rank`, `quartile`, `squarify`, `boxCats`, `mergeDLbls`, `mergePointLabel`, and the float to index conversions).
`render.ts` 785–1990 is 1,205 raw lines; the Rust is about 1,550 (axes) + 610 (xy) lines. The TS `Map<Axis, AxisInfo>` + `infosCache` become the `Vec<AxisInfo>` the painter owns and
passes down (`&mut [AxisInfo]` where a call measures, `&[AxisInfo]` where it reads: same object identity, same insertion order). Harness: `LayoutCall` has a variant for each
patched method (`prepValueAxis`, `prepCatAxis`, `measureAxis`, `plotRect`, `dateLabels`, `valueLabels`, `trendCurve`, `hierarchy`) plus `Pending` — a call is numbered when it starts, as the
TS wrapper numbers it, so nested calls come after their parent; `tests/support/layout_json.rs` prints the AxisInfo/Scale/HNode/TrendFit JSON of the capture's `plain()` (a thrown call prints
`{"$throw": message}`); the `lay.<view>` all-calls digest (seq, method, JSON separated by tabs) is replayed now too.

### Parity (replay driver, `cargo test -p uo-charts --test parity`; stages `ops`, `ask`, `bounds`, `lay.*`)

| | `charts-c1a` (600 charts) | `charts-samples` (26) | `charts-diff` |
|---|---|---|---|
| compared | 16,236 (+ 1,800 C-1 facets in `c1_replay`) | 1,140 (+ 78 C-1) | 2,760 |
| equal | **16,213** | **1,140** | 2,757 |
| mismatches | **0** | **0** | 0 |
| accepted by name (Math.pow platform, G1) | 23 | 0 | 3 (C-2's) |
| skipped | 261 (the `ops` streams of charts that paint a picture tile) | 0 | 0 |

Compared / total per family in `c1a` (facets of ops, ask, bounds, layout): area 999/1039, area3D 241/250, bar 2233/2302, bar3D 546/552, bubble 202/202, combo 1168/1185, cx 2645/2645,
doughnut 147/150, edge 688/688, line 1823/1884, line3D 225/228, ofPie 165/168, pie 984/995, pie3D 144/150, radar 167/167, scatter 712/716, stock 535/564, surface 212/212 (the gap in each
family is the tile `ops` facets). By stage: C-3 layout 2,245, emit (ops, ask, bounds) 7,699, all-calls layout 800, C-4 layout (axes, plot rect, labels) 2,937, trendCurve 129, hierarchy 26.
In `samples` (the 18 chart slides, the three workbook charts, `stress`, `playback`) everything compares: 156 title/legend + 176 axis layout + 52 all-calls layout + 652 emit facets.
First replay of the whole port: 15,903 equal, 23 mismatches, **all** the `Math.pow` platform difference below; no other parity round.

**The `Math.pow` platform rule (record G1 of `uo-jsnum.md`).** 23 facets of 7 generated charts (0007, 0010, 0336, 0409, 0459, 0471, 0503: poly/power trendlines) differ from the
capture; they are accepted BY NAME in `tests/support/facets.rs` (`POW_FACETS`: `(chart key, facet)`, never a tolerance). Evidence: logging every `pow(x, y)` of chart 0409 (432
distinct argument pairs) and evaluating `Math.pow` in node on the same bits, `uo_jsnum::math::pow` (glibc 2.39, pinned) differs in 2 (`2.1666666666666665 ** 1.0471526243442981` and
`4.375 ** 0.8247114876778696`, one ulp); the order-5/6 polynomial and power fits amplify one ulp to 1e-12 in the sampled curve (`x ** i` and the normal equations). Replacing the fit and
the evaluation by the platform `powf` fixed 6 of the 23 and broke chart 0471's ops (which match with the pinned pow), so the pinned pow stays and the difference stays named.

### CH-1 measured again with the axis labels (`c3_gate::ch1_round_counts`, 1,200 paints of `c1a`, views a and b, the capture table as oracle)

| | rounds 0 | 1 | 2 | 3 | 4 | 5 to 13 | max | questions asked |
|---|---|---|---|---|---|---|---|---|
| with the rule | 38 | 971 | 162 | 27 | 2 | - | **4** | 263,071 |
| without | 38 | 959 | 68 | 35 | 8 | 92 | **13** | 255,048 |

With the axes in, one paint asks about 220 distinct (font, text) keys on average (every category label, every value label per plot width, the `'Hg'` font box); the rule costs +3.2 % of the
questions and keeps the rounds at 4 (13 without it: the label widths decide rotation, wrap and skip, which change the labels asked in the next layout pass of `plotRect`). The rounds
that remain are data dependencies (plot width, band, wrap candidates), not wrap chains. `MAX_ROUNDS` 8 holds with the rule.

### Memory and robustness

M1 (`c3_gate::m1_and_peak_over_every_family`): `emit_chart`, `need_measures`, `part_bounds`, `trace_layout` and a hostile-measure emit over 86 charts of every family of `c1a` (every 7th)
return live bytes exactly; one emit peaks at 103,245 B above the start at most (the engine keeps no list; the host owns the display list). M2 (`m2_soak_over_every_family`): 1,000 cycles flat
(tolerance 0). No-panic: the C-3 bounded loop (5 s of mutated charts x hostile rectangles x hostile measure answers) now paints whole charts; and the fuzz target `chart_emit`
(`core/fuzz/src/charts_render.rs`, extended with seven seeds: explicit axes with titles, rotated labels, a log secondary pair, a percent-stacked combo with data table, 3D area + bar, scatter +
bubbles with poly/power trendlines and labels, radar, bar of pie, doughnut) runs through `tests/c45_fuzz.rs` (the same function, included by path: the fuzz crate does not build while B5b's
`uo-dml-render::metafile` is half written): **13,940 executions in 90 s (release), 0 failures**, slowest 5.5 s (a waterfall whose `pt idx` is 50,000: 50,001 categories; the cost is linear in the
points, about 1 s per emit at 50k categories, and the parser's cap of 100,000 doubles it). Two fixes of the target's bounds, not of the engine: the canvas-call bound counts the value axes (up
to 1,000 ticks each, every label measured — a `font` call — in each of about 30 `measureAxis` passes: 75,000 calls per axis) and the points count includes `catNums` (a numeric category axis
of `ptCount` 100,000).

### Decisions outside the source semantics (declared)

1. `ChartError::NotPorted`, `emit_envelope`, `ENVELOPE_MARK`, the `partial` renderer flag, the envelope check (`parity.rs`, `stages::envelope`) and its `lineJoin` normalisation are removed: the
   families paint now, and the whole stream of every chart is compared.
2. Hostile counts: `Array.from({ length: ceil(n / tickLblSkip) })` and `new Array(n)` past 10,000,000 elements answer `Throws(RangeError, "Invalid array length")` (`util::array_len`; the TS
   throws past 2^32-1 and runs for minutes in between); `stackedTotals` of a group without series (`Math.max(...[])` is -Infinity) throws the RangeError as the TS does (faithful, not a decision).
3. `for (let i = 0; i < k; i += tickMarkSkip)` with a skip of 0 or less never ends in the TS; the Rust draws the first tick only (a NaN skip ends after the first in both).
4. `valueLabels` of an axis without a scale (the TS `inf.scale!`) returns no labels; no caller asks.
5. `RecCanvas` still does not print a picture TILE (the shell painter's, C-6): the 261 `ops` facets of the charts that paint one (152 of the 600 generated charts carry an `a:tile`, 123 with
   a `flip`) are skipped and counted; their `ask`, `bounds` and layout facets compare. This is the only skip left.
6. The `ask` facet now compares for those charts too (the set of questions does not depend on what the canvas can print).
7. `tests/c3_gate.rs::no_panic_over_...` asserts `emitted > 40` (a 5 s debug run paints whole charts now; it was 200).

## C-5 — pies, chartex painters, radar, data labels (done 2026-11-17, Sonnet)

Files: `render/pie.rs` (`paintPies` — rings, doughnut, explosion, 3D side, labels with leader lines; `paintPie3D`; `paintOfPie`; `pieLabelsOutside`), `render/cx.rs` (`paintBoxes`, `hierarchy`,
`hierFill`, `paintHierarchy` (squarified treemap with parent labels), `paintSunburst`), `render/radar.rs`, `render/labels.rs` (`pointLabel`, `labelText`, `labelShift`, `besideLabel`,
`paintDataLabels`). `render.ts` 1990–2862 is 870 raw lines; about 1,400 Rust. Parity numbers are the table of C-4 (the families share the replay): pie 984/995, pie3D 144/150, doughnut
147/150, ofPie 165/168, radar 167/167, cx 2,645/2,645 (treemap, sunburst, box & whisker, waterfall, funnel, histogram), the data labels inside every family; `hierarchy` 26/26 layout facets;
0 mismatches, the gaps are tile `ops`. Same memory, robustness and CH-1 numbers (the pies ask the label boxes and the wrap candidates of `c15:layout` boxes).

Decisions outside the source semantics: none beyond C-4's list (the ofPie `custSplit` indices are integers >= 0 by the parser's filter, so the integer filter in `paintOfPie` is the TS's).

### What C-6 needs (updated)

The emit is complete: `emit_chart` paints every family; the canvas calls the families add to C-3's are all in the `Canvas` trait and in the Slides item map (`bezierCurveTo` of smooth
lines, `ellipse` of pies and doughnuts, `arc` with the anticlockwise flag of sunburst rings, `clip` of the plot and the treemap tiles, `lineJoin`, direct `strokeStyle`/`lineWidth`/`fillStyle`
sets, `fillText` of rotated axis labels and trendline equations). Open for the Slides host (CH-6): the painter side of `CanvasPaint::Tile` (CT-1), `chartFont`, `fillText`, `roundRect` (R4/R5),
the per-model bounds cache, a host over the real theme, and the tile comparison through the TS interpreter (the 261 skipped `ops` facets are the check). Budget numbers for the host: up to
4 measure rounds per chart (cap 8), about 220 questions per paint (batch them with the shape questions), a chart emits a few thousand canvas calls (a hostile 1,000-tick axis about 75,000, a
50,000-point waterfall about 1 s): the list builder should cap the items per chart.

Process metrics (C-4 + C-5, one agent session, about 1 h wall): port of `render.ts` 785–2862 about 25 min (writing, 1 compile round with 2 fixes), replay and analysis of the 23 pow facets about
15 min (the replay is 127 s for `c1a` in debug, 2 s with `UO_CHARTS_ONLY=<key suffixes>`), memory/no-panic/fuzz about 15 min, lint and record about 10 min. clippy native (`--all-targets
--all-features`) and wasm32 (lib) clean, `fmt` clean, `grep-bans.sh` clean. Debug aid kept: `tests/c45_dbg.rs` (prints the Rust side of one facet to diff against the sidecars) and `UO_CHARTS_ONLY`.

## C-6 — Slides host (done 2026-11-17, Sonnet)

The engine paints into the Slides display list: `emit::chart` of `uo-dml-render` implements the three traits of this crate over the emission context, the
TS interpreter (`paint-slides.ts`) replays the list, and the draw-op streams of the frozen TS engine are reproduced line for line.

Files: `core/crates/uo-dml-render/src/emit/chart.rs` (`paint_chart_slot` — the TS `paintBody` chart branch —, `chart_part_bounds`, `emit_chart_with`),
`chart_host.rs` (`SlidesHost: ChartHost`, `TableMeasure: ChartMeasure`, `request_of`, `ChartCache`), `chart_canvas.rs` (`SlidesCanvas: Canvas`); small edits in
`cx.rs` (`EmitCaches::charts`, `chart_sizes`), `mod.rs` (exports; `Cx` is public now), `shape.rs` (`paint_chart_slot(..)?`), `theme.rs` (`color_in` crate-visible),
`Cargo.toml` (`uo-charts`, `uo-numfmt`). `uo-render` (`slides.rs`: validation of `ChartFont`/`FillText`, re-exports) and `schema/slides.uos` (regenerated with `uo-schema`:
the Rust and TS codecs; **Write's schemas, bytes and the pinned hash are untouched**, `uo-schema check` clean). Painter side: `uo-wasm/js/paint-slides.ts`
(`chartFont`, `fillText`, the chart tile), `slides-effects.ts` (`roundRect` in `replayPath`), `slides-engine.ts` (the empty-family font), the viewer's `rust-engine.ts`
(no `skipCharts`). Verification: `uo-pptx/js/lib/gen/c1d.mjs` (group `c1d`), `uo-charts/tests/c6_tiles.rs` + `js/compare-tiles.mjs`, `uo-dml-render/tests/charts.rs`.

### Schema changes (CH-6: A1–A3, CT-1 instead of A4, R4–R5, one change)

| Addition | Shape | Painter |
|---|---|---|
| `SItem::ChartFont` (A1) | `FontRef { face, size_px }` (the existing struct) | `ctx.font = [italic ][bold ]<size_px.toFixed(2)>px <stack>` — render.ts:177–180, not `Font`'s `700 <JS number>px` |
| `SItem::FillText` (A2) | `FillTextItem { text: Span, x, y }` | a plain `fillText` under the current state |
| `PathOp::RoundRect` (A3) | `RoundRectOp { x, y, w, h, r }` | `ctx.roundRect(x, y, w, h, r)` (in `replayPath`) |
| `TileDef.nat: Pair?` (CT-1) | the natural tile size in px, computed by the engine with the TS arithmetic; with it `scale` is 1, `unit_x`/`unit_y` are unused and `tx`/`ty` are already px | `tilePattern(ctx, img, iw, ih, nat.x, nat.y, { sx, sy, tx, ty, algn, flip }, bx)` — the TS arguments, no denominators |

R4 (recorder) was already in `record-ctx.mjs` as opt-in capabilities (C-0): `DEFAULT_CAPS` now has `roundRect: true` and `strictFont: true`, because the Slides painter's own code never calls
`roundRect` and a canvas ignores an invalid font string; `measureText` already returned the capture measurer's font box (the stub's 0.9 / 0.21 of the size). The stub of the
parity examples and tests (`uo-pptx/tests/support/stub_measure.rs`, `uo-wasm/tests/support/slides-stub.mjs`) answers `Metrics` the same way.

### Design as built

- **Host** (`parseChart` of chart-engine.ts): colours = `color_in` over `{...FALLBACK, ...theme.colors}` (`Scheme(name)` is the synthetic `<a:schemeClr val=name/>`); `fill_style(idx, phClr)` =
  `parse_paint_node` over `theme.styles.fill_style(idx − 1)` with `phClr` added to the colours; `blip` = `r:embed ?? embed` → the first `chartImages` entry with the same picture (the TS key is
  the part name); fonts, `format`, no user shapes, no theme line widths (the Slides host passes none). CH-4 kept: no canonical prefixes (the chart part comes from `PartSource::xml`, the Slides
  preset); a part that does not parse is an empty `c:chartSpace`, as the TS catch.
- **Measurement**: a width is `MeasureReq::Width(StringKey { kerning: Current, letter_spacing_px: 0 })`, the font box of `'Hg'` is `Metrics` of the same key, so a chart's questions join the
  slide's `NeedMeasures` (one shell batch per round; the engine's own rounds are rounds of the slide). Per chart the pass is `need_measures` first (a `NullCanvas`, no items), then, when it is
  complete, `emit_chart` over `SlidesCanvas`. The canvas default font (CH-5) is the face with an empty family, measured at 10 px: the shell measures `10px sans-serif`.
- **Pictures** are resolved lazily, as the TS resolver is: only the pictures the engine asks for (`ChartMeasureReq::Image`) are looked up (`Cx::image_plain`: the media, its `ImageSize`
  question, the dpi → `w_pt = w × 72 / dpi.x`), then the need pass repeats.
- **Canvas → items**: save/restore/translate/rotate, styles (`FillStyle` with `gradients`/`stops`, stretched `PatternRes`, the tile of CT-1), line state, `GlobalAlphaMul`, `ChartFont`, `TextAlign`,
  `TextBaseline`, paths (buffered: one `AppendPath` span per run of path calls), `Fill`/`Stroke`/`Clip`, `FillRect`/`StrokeRect`, `FillText`, `DrawImage`. `ShadowOffset` sets both offsets in one item, so
  `SlidesCanvas` holds the pair (with the canvas rule: a non-finite value is ignored, saves and restores stack it) and re-states the other one.
- **Throws**: an engine `Throws` aborts the shape (B4-1); what was drawn stays.
- **Caps (M5)**: `ChartCache` keeps 32 parsed charts (`Arc<ChartSpace>`, key = part + the media of its pictures; `PaintResources.charts` is unbounded in the TS) and, per model, 64 part-bounds
  boxes, both least recently used (`boundsCache` is unbounded in the TS). `MAX_CHART_ITEMS` = 200,000 items per chart: past it the canvas drops every call except the `restore`s of the saves it did emit.
- **Not in static emission** (B4-3): the animated chart parts (`SlotBegin(ChartPart j)`, the soft-reveal layer and the no-layer fallback of render.ts:772–798) arrive with B6;
  `chart_part_bounds` (with `bounds_key`, the TS `${sel.key}@x.toFixed(2),…,ppt` string) is ready and tested. `Emission::chart_frames` stays in the API and is always empty.

### Parity

| Gate | Views / facets | Equal |
|---|---|---|
| `drawops-samples` (53 decks, views a and b), `emit_uolf` + `compare-drawops` | 1,202 | **1,202** (the 36 chart views now equal; the capture was re-recorded with `roundRect`/`strictFont`: the sample chart with rounded corners now draws its frame) |
| `drawops-b1a` | 246 | 246 |
| `drawops-c1d` (**new**: 60 generated decks, 91 slides, 1–2 charts each from the `c1a` generator: pictures, textures with `flip`, `themeOverride`; 244,618 draw lines, 12,449 distinct measure keys) | 182 | **182** |
| the same three through `uo-slides.wasm` (`slides_drawops.test.mjs`) | 246 + 1,202 + 182 | all equal |
| the tile `ops` facets of `charts-c1a` through the interpreter (`c6_tiles.rs` → `compare-tiles.mjs`) | 310 facets (the 261 skipped ones plus the others whose streams the capture file keeps) | 300 equal + 6 accepted by name (chart 0459, poly/power trendline: the `Math.pow` platform difference G1, 1e-12 in the sampled curve, same lines); 4 facets expect an error and the engine throws on both sides (checked natively) |

Every gate was equal on its first run: 0 parity rounds. Coverage of the c1d TS streams: 1,384 offscreen-canvas lines, 930 picture patterns, 176 tile/flip canvas patterns, 164 `roundRect`, 19,574 `fillText`.

### Chromium pixels (`slides_switch.mjs --mode all`, real Chromium, open fonts, tolerance 0)

213 decks (53 samples, 100 `b1a`, 60 `c1d`), **938 slides, 938 pixel-equal** between `engine="ts"` and `engine="rust"` (968,355,840 pixels); **109 slides have charts and all are painted by the
Rust list: 1,925 paints, 0 left to the TS painter**. Main canvas 596 compared, thumbnails, notes and a11y text 0 different; memory phase: module live bytes 1,148,702 flat (spread 0), 0 handles
after the element left; faults and link filter as before. The harness counts the engine's paint statuses now and fails on any slide difference, charts included.

### Sizes

`uo-slides.wasm` (`--profile wasm`, `--features slides`): **2,914,742 B raw, 1,045,866 B gzip -9** (before B5b and the charts: 2,042,577 / 728,867; the charts and `uo-numfmt` are about 317 KB of gzip), 65 % of the
1.6 MB budget; `wasm-tools validate` with the Safari 15 feature set passes.

### Decisions outside the source semantics (declared)

1. **C6-1** (CT-1): `TileDef.nat` instead of the record's A4 denominators (see the schema table).
2. **C6-2** Pictures: only plain pictures (`ImageSrc::Media`) are answered; a chart picture that is a metafile, PICT or TIFF raster (a bitmap of the resources, which the canvas calls cannot name) is
   "unavailable" — the TS resolver would draw its raster. No corpus chart has one (declared gap).
3. **C6-3** The `ShadowOffset` pair (above); a recorded stream differs from the TS only in `set` lines, which the draw form drops.
4. **C6-4** `MAX_CHART_ITEMS` (above) and the abort-on-throw rule.
5. **C6-5** `DEFAULT_CAPS` of the recorder gained `roundRect` and `strictFont` for every capture; `drawops-samples` and its measures were re-recorded (only the 36 chart views change), `drawops-b1a`
   is unchanged.
6. **C6-6** The model cache key is the part name and the media ids of its pictures (the TS key is the XML text and the picture parts); one `EmitCaches` belongs to one deck generation.
7. **C6-7** The `chart_frames` consumers (wasm, the PDF note, the example) are untouched: the list is empty, so the viewer has nothing to leave to the TS painter; `skipCharts` stays a no-op option.

### Memory and robustness

M5: `uo-dml-render/tests/charts.rs` (32 models and 64 bounds keys, least recently used; the host over a theme; the item cap with balanced saves). M1/M2: `uo-pptx/tests/emit.rs` now includes `c1d` decks
(emit, wire round trip, leak and soak): live bytes return exactly. No-panic: the mutation test also mutates the chart parts (744 mutants of 8 decks in 52 s with 12 rounds), hostile viewports reach
the charts (`scale` 0, NaN, ±∞); the fuzz target `slides_emit` gained a chart frame (a titled two-series bar chart as the chart part, hostile measure answers including the `'Hg'` font
box): the `fallback` driver ran 125,475 executions in 60 s, 0 crashes, slowest 17 ms.

Process metrics: one agent session, about 2 h of wall time. Schema, painter, host and canvas were written in one pass (1 compile error round: the generated types' re-exports); the parity gates were equal
on the first run (0 parity rounds); the longest steps are the Chromium harness (6 min for 213 decks) and the wasm build (22–42 s). clippy native (`--all-targets --all-features`: `uo-dml-render`,
`uo-charts`, `uo-render`, `uo-pptx`, `uo-wasm`) and wasm32 (lib) clean (one pre-existing `chunks_exact` lint of B7's `slides.rs` fixed), `fmt` clean, `grep-bans.sh` clean, `uo-schema check` clean.
The disk (D:) filled during the run (other builds); `cargo clean --profile dev` of `target/c6` freed 4.2 GB. The CI tsc line needed no new TS file (the changed ones were listed already); one run of it
shows two errors in `editor.check.ts` (`CompletionBinding`, `FormatBinding` missing from `write-session.d.ts`), not from this change.

## Office oracle plan (agreement, not the parity gate)

Rule blocks already measured (encoded in the TS, memory `chart-oracle-facts`, `drawingml-color-oracle-facts`):
(1) chart area fill/frame and rounded corners; (2) titles (rich, automatic, 18.62 pt without defRPr); (3) smooth by
default and automatic markers; (4) automatic colours in linear RGB (weight 1.41·|f|, grey style 1); (5) styles 1–48
(line widths, gradients, dark rows, plot fills); (6) legend order and manual layout, table layout; (7) pie diameter
97 %, linked formats, `m/d/yyyy`; (8) value-axis density; (9) `c14:style` grey lines; (10) picture fills and tiles by
file dpi, tick length 0.3·sz + 0.75 × font factor, lblOffset, file-format defaults (AppVersion 12 × 16); (11) Excel
rules (frame #898989, layoutTarget, overlay, Office 2023 theme, density, series line = k × theme width, Scott
histogram, chartex defaults, ofPie, trendline labels); (12) 3D oblique bars and pie 3D, dTable rows. None is
re-measured for the port.

After parity (C-7): (a) **PowerPoint**: a new `core/tools/oracle/ppt-export-charts.ps1` modelled on
`slides/tools/ppt-lab/export-deck.ps1` (read-only open without a window, warm-up export, no Quit when PowerPoint was
running, UTF-8 BOM, `-ExecutionPolicy Bypass`, no focus steal) exports the `c1d` decks to PNG; Chromium renders the
Rust lists; the mean absolute pixel difference per chart is recorded per family (the old `chartcmp` metric: corpus
mean 8.4, median 5.6 at 0.9.x). (b) **Excel**: the `Chart.Export` driver was lost with the corpus; rebuild as
`core/tools/oracle/xl-export-charts.ps1` over generated `.xlsx` (the same XML in a workbook): `ChartObject.Chart.
Export(png)` per chart with `ScreenUpdating` on and the chart scrolled into view (an off-screen chart exports empty),
the 1.5 pt frame crop of the corpus report, COM names translated. Differences found become measured follow-ups behind
switches (the "Word manda" rule), never silent changes of the port.

## Memory (M1–M5)

Caches in the TS chart code and where they go:

| TS cache | Where | Rust |
|---|---|---|
| `Renderer.fontKCache` (render.ts:1046), `seriesIndex`, `infosCache`, `depth`, `dtab` | per paint | locals of one emit (bounded by the chart) |
| `boundsCache` WeakMap<ChartModel, Map<key, box>> (chart-engine.ts:136), unbounded | Slides host | per model entry of the host chart cache, LRU 64 keys |
| `PaintResources.charts` Map<xml, ChartModel> (slides render.ts:54), unbounded | Slides host | uo-pptx M5: 32 `Arc<ChartSpace>` |
| `chartCache` (universal-sheet.ts:3528), `formatCache` WeakMap<pkg> (chart.ts:112) | Sheet | Sheet crate: capped cache, package field |
| `dpiCache` WeakMap<bytes> (slides render.ts:511) | Slides host | the media table's dpi (B4) |
| `flipCache` WeakMap<image, Map> (image.ts:56) | shell painter | painter cache, capped (R5) |
| `uo-numfmt` parsed-code cache | `uo-numfmt` | already capped |

- **M1**: `parse_chart_space`, `parse_paint`, `dml_color`, `emit_chart` (complete, `NeedMeasures`, `Throws`),
  `need_measures`, `part_bounds`, `histogram_bins`, `fit_trend`, `auto_scale`: live bytes back exactly after drop.
- **M2**: soak of 2,000 parse + emit cycles over `c1a`, flat (tolerance 0).
- **M3**: retained `ChartSpace` ≤ 2 × the chart XML bytes (median over `c1a`); peak of one emit ≤ 4 × the emitted
  items × `size_of::<SItem>()`; numbers recorded at C-3/C-5.
- **M4**: no handles (the host owns models and lists).
- **M5**: the engine keeps none; the host caches above are tested with their caps.

## Fuzz targets

`chart_parse` (structure-aware `c:`/`cx:` trees: deep nesting, `ptCount`/`idx` at and past 100,000, NaN/∞/huge
attributes, `c14:style` garbage, logBase 0/1/1e9, holeSize/firstSliceAng extremes, empty and duplicated axes),
`chart_emit` (random `ChartSpace` × random measure answers incl. NaN, 0, 1e308, missing → emit through `NullCanvas`
and `BoundsCanvas`: no panic, item count ≤ f(points, ticks ≤ 1000, legend entries)), `chart_scale` (`auto_scale`/
`tick_values`/`minor_log` with extreme spans and units), `chart_trend` (degenerate Gauss systems, order 6 on 1e300),
`chart_histogram` (binCount/binSize that would allocate billions: capped, see traps), `chart_squarify`.

## JS traps (each a rule for the port)

- Numbers → strings through `uo_jsnum::format`: `toFixed(2)` of font sizes (render.ts:179) and of the bounds key,
  `toPrecision(12)` of scale ends and ticks (scale.ts:65, 80), `toPrecision(2)` of the Scott width (chartex.ts:157),
  `(Math.round(v·1e4)/1e4).toFixed(4)` + `/\.?0+$/` of trendline numbers (trend.ts:64), `String(i + 1)` labels,
  `toString(16).padStart(2, '0')` hex, `rgba(r,g,b,a)` with JS number printing.
- `Math.*` through `uo_jsnum::math` (fdlibm): `log10` in `niceStep` (`floor(log10(x))` at exact powers of ten is where
  libms differ), `log`, `exp`, `pow`/`**`, `cbrt`, `hypot`, `cos`/`sin`/`atan2`; `Math.round` is JS rounding.
- `Math.min/max` propagate NaN and `Math.max(...[])` is `-Infinity` (`Math.min(...[])` `Infinity`): Rust `f64::max`
  ignores NaN — use `uo_jsnum::math::{max, min, max_n, min_n}` at every site (legend widths, `minGap` in
  `dateLabels`, label extents).
- Sorts: `stops.sort((a,b) => a.pos - b.pos)` with NaN positions, `g.series.sort(order)`, `strips.sort(z)`,
  box values, treemap children — `uo_jsnum::sort::js_sort_by` (V8 order also for inconsistent comparators).
- `parseInt(x, 10)`/`parseFloat` via `uo_jsnum::parse`; `valNum` keeps only finite numbers but `parseInt` sites
  (`ln w`, `c14:style`, gradient `pos`, shadow `dist/dir/blurRad`) can yield NaN that flows on.
- Regex splits: `split(/(\s+|(?<=-))/)` (wrap: separators kept, break after `-`), `split(/(\s+)/)`, `/\S/`, `trimEnd`/
  `trimStart` — JS white space (`uo_jsnum::text::is_js_space`), UTF-16 semantics for lone surrogates (B3-3 rule).
- `new Date(ms).getUTCFullYear/Month` (dateLabels) → `uo_numfmt::utc_fields`.
- Hostile loops: `minorLog` inner `for (k = 1; k < b; k++)` with a huge base (the TS spins; Rust stops once `x > hi`,
  same output, declared), `histogramBins` with a bin count from the file (`Array.from({length: k + 1})`: the TS
  exhausts memory or throws `RangeError` past 2³²−1; Rust caps at 10⁷ bins → `Throws(RangeError)`, declared),
  `tickValues` capped by `MAX_TICKS` already.
- Throws that skip the shape: style numbers outside 1–48 that index no accent (`acc[scheme - 2]` undefined →
  `hexToRgb(undefined)` TypeError), negative `arc`/`ellipse`/`roundRect` radii (DomException/RangeError),
  `addColorStop` with a NaN offset (the clamp passes NaN), non-finite `createLinearGradient` arguments (TypeError).
  Calls a canvas silently ignores (non-finite `moveTo`, `fillRect`, `translate`, `lineWidth`) are emitted as the TS
  issues them.
- Spreads `{...a, ...stripUndef(b)}` are field-wise overrides; object key order never reaches output.

## Decisions (applied by the orchestrator on 2026-11-17 with the recommended option; the maintainer may revisit at M4)

- CH-1: APPLIED — the wrap prefetch rule (contiguous token spans ≤ 32 tokens); it only adds questions, never changes
  results; C-3 records the measured round counts.
- CH-2: APPLIED — native `RecCanvas` (a Rust port of the recorder's `draw` form) as the fast parity path; the TS
  interpreter path stays the product check.
- CH-3: APPLIED — `Canvas` trait + host emitters, no `uo-render` dependency (no cycle).
- CH-4: APPLIED — NO canonical prefixes for Slides chart parts: the TS Slides bridge does not canonicalize, so strict
  parity keeps it off (no behaviour change for parity's sake); the Sheet host keeps its canonicalization.
- CH-5: APPLIED — the canvas rule: an invalid font assignment keeps the previous font; initial `10px sans-serif`.
- CH-6: APPLIED — B4 holds `slides.uos` now; A1–A4 and the recorder/painter additions R4–R5 are made by C-6 (the Slides
  host frontier, after B4 is committed), in one schema change. DONE in C-6 (A4 replaced by CT-1, `TileDef.nat`).

## Process metrics

Architecture pass (Opus, 2026-11-17): reading of the five records, `shared/charts/src` (render.ts in full for the
measurement and canvas sites), the two host bridges, the Slides list schema and B4's in-progress schema diff; skeleton
crate (6 modules, ≈ 1,350 lines with docs); `clippy` native (`--all-targets`) and wasm32 (lib) clean, `fmt` clean,
`grep-bans.sh` clean.
