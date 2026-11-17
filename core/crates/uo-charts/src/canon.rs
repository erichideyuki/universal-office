//! The canonical JSON of the chart model (feature `canon`): the text the parity captures hash
//! (`core/crates/uo-charts/js/capture-charts.mjs` → `canon-json.mjs` of uo-pptx, rules in
//! `core/docs/modules/uo-pptx-captures.md`), written with the TS field names of `model.ts`.
//!
//! - object keys sorted by UTF-16 code unit, no white space; a key whose TS value is `undefined` is not
//!   written (every `Option` field is written only when present; the booleans the TS sets only when true
//!   — `noDefRPr`, `dispEq`, `dispRSqr`, `abs`, `flipH`, `flipV` — only when true);
//! - numbers: `String(x)`, `-0` kept, NaN/±Infinity as `{"$num":"NaN"|"Infinity"|"-Infinity"}`;
//! - strings by `JSON.stringify`; over 4,096 UTF-16 units `{"$text":<units>,"sha256":<hex of the UTF-8>}`;
//! - `Map` as `{"$map":[[k,v],…]}` and `Set` as `{"$set":[…]}` in insertion order; a sparse array's hole
//!   (`ptFmts`) is `null`; `themeFill` is the boolean `true`.
//!
//! A `Paint::Blip` prints the host's string for its key (`ref`, `ChartHost.blip` of the TS): [`Canon`]
//! takes the host's reverse table. Also here: the key of a host call (the canonical JSON of the XML
//! element handed to `ChartHost::color`/`blip`, the form `recordingHost` of the capture logs).

use uo_jsnum::format::to_js_string;
use uo_opc::xml::{Element, Node};
use uo_testkit::sha256::sha256;

use crate::host::BlipKey;
use crate::model::{
    Axis, AxisKind, AxisPos, BarDir, BoxOpts, ChartColor, ChartSpace, ChartTitle, CrossBetween, Crosses, DLbl, DLbls, DLblsCommon,
    DataPoint, DataTable, DispBlanksAs, FmtCode, FracRect, GradPath, Group, GroupKind, Grouping, Legend, LegendPos, LineProps,
    ManualLayout, Marker, NumFmt, OfPie, Paint, PointMap, PointSet, RichPara, Series, Shadow, ShapeProps, TextAnchor, TextProps,
    TickLblPos, TickMark, TileFlip, TrendLabel, Trendline, UpDownBars, UserShape, UserText, View3D,
};

/// A value of the canonical JSON.
#[derive(Clone, PartialEq, Debug)]
pub enum Json {
    Null,
    Bool(bool),
    Num(f64),
    Str(String),
    Arr(Vec<Json>),
    Obj(Vec<(String, Json)>),
    /// `{"$map":[[k,v],…]}`
    Map(Vec<(Json, Json)>),
    /// `{"$set":[…]}`
    Set(Vec<Json>),
}

/// Strings longer than this many UTF-16 units are digested (`LONG_TEXT` of `canon-json.mjs`).
pub const LONG_TEXT: usize = 4096;

fn units(s: &str) -> usize {
    s.chars().map(char::len_utf16).sum()
}

fn hex(d: &[u8]) -> String {
    d.iter().map(|b| format!("{b:02x}")).collect()
}

fn cmp16(a: &str, b: &str) -> std::cmp::Ordering {
    a.encode_utf16().cmp(b.encode_utf16())
}

impl Json {
    /// A string value.
    pub fn s(v: &str) -> Json {
        Json::Str(v.to_owned())
    }

    /// The canonical text.
    pub fn to_text(&self) -> String {
        let mut out = String::new();
        self.write(&mut out);
        out
    }

    fn write(&self, out: &mut String) {
        match self {
            Json::Null => out.push_str("null"),
            Json::Bool(b) => out.push_str(if *b { "true" } else { "false" }),
            Json::Num(x) => {
                if x.is_nan() {
                    out.push_str("{\"$num\":\"NaN\"}");
                } else if *x == f64::INFINITY {
                    out.push_str("{\"$num\":\"Infinity\"}");
                } else if *x == f64::NEG_INFINITY {
                    out.push_str("{\"$num\":\"-Infinity\"}");
                } else if *x == 0.0 && x.is_sign_negative() {
                    out.push_str("-0");
                } else {
                    out.push_str(&to_js_string(*x));
                }
            }
            Json::Str(s) => {
                if units(s) > LONG_TEXT {
                    out.push_str(&format!("{{\"$text\":{},\"sha256\":\"{}\"}}", units(s), hex(&sha256(s.as_bytes()))));
                } else {
                    uo_jsnum::json::push_string(out, s);
                }
            }
            Json::Arr(v) => {
                out.push('[');
                for (i, x) in v.iter().enumerate() {
                    if i > 0 {
                        out.push(',');
                    }
                    x.write(out);
                }
                out.push(']');
            }
            Json::Map(v) => {
                out.push_str("{\"$map\":[");
                for (i, (k, x)) in v.iter().enumerate() {
                    if i > 0 {
                        out.push(',');
                    }
                    out.push('[');
                    k.write(out);
                    out.push(',');
                    x.write(out);
                    out.push(']');
                }
                out.push_str("]}");
            }
            Json::Set(v) => {
                out.push_str("{\"$set\":[");
                for (i, x) in v.iter().enumerate() {
                    if i > 0 {
                        out.push(',');
                    }
                    x.write(out);
                }
                out.push_str("]}");
            }
            Json::Obj(kv) => {
                let mut idx: Vec<usize> = (0..kv.len()).collect();
                idx.sort_by(|&a, &b| match (kv.get(a), kv.get(b)) {
                    (Some(x), Some(y)) => cmp16(&x.0, &y.0),
                    _ => std::cmp::Ordering::Equal,
                });
                out.push('{');
                for (n, &i) in idx.iter().enumerate() {
                    let Some((k, v)) = kv.get(i) else { continue };
                    if n > 0 {
                        out.push(',');
                    }
                    uo_jsnum::json::push_string(out, k);
                    out.push(':');
                    v.write(out);
                }
                out.push('}');
            }
        }
    }
}

/// An object under construction: only present fields are added.
#[derive(Default)]
struct Obj(Vec<(String, Json)>);

impl Obj {
    fn put(&mut self, k: &str, v: Json) -> &mut Self {
        self.0.push((k.to_owned(), v));
        self
    }
    fn n(&mut self, k: &str, v: f64) -> &mut Self {
        self.put(k, Json::Num(v))
    }
    fn num(&mut self, k: &str, v: Option<f64>) -> &mut Self {
        if let Some(v) = v {
            self.put(k, Json::Num(v));
        }
        self
    }
    fn st(&mut self, k: &str, v: &str) -> &mut Self {
        self.put(k, Json::s(v))
    }
    fn string(&mut self, k: &str, v: Option<&str>) -> &mut Self {
        if let Some(v) = v {
            self.put(k, Json::s(v));
        }
        self
    }
    fn b(&mut self, k: &str, v: bool) -> &mut Self {
        self.put(k, Json::Bool(v))
    }
    fn boolean(&mut self, k: &str, v: Option<bool>) -> &mut Self {
        if let Some(v) = v {
            self.put(k, Json::Bool(v));
        }
        self
    }
    /// a boolean the TS sets only when true
    fn when(&mut self, k: &str, v: bool) -> &mut Self {
        if v {
            self.put(k, Json::Bool(true));
        }
        self
    }
    fn json(&mut self, k: &str, v: Option<Json>) -> &mut Self {
        if let Some(v) = v {
            self.put(k, v);
        }
        self
    }
    fn done(&mut self) -> Json {
        Json::Obj(std::mem::take(&mut self.0))
    }
}

fn color(c: &ChartColor) -> Json {
    Json::s(c.as_str())
}

fn opt_color(c: Option<&ChartColor>) -> Option<Json> {
    c.map(color)
}

fn fmt(f: &FmtCode) -> Json {
    Json::Str(String::from_utf16_lossy(f.units()))
}

fn opt_num(v: Option<f64>) -> Json {
    v.map_or(Json::Null, Json::Num)
}

fn arr<T>(v: &[T], f: impl Fn(&T) -> Json) -> Json {
    Json::Arr(v.iter().map(f).collect())
}

fn nums(v: &[Option<f64>]) -> Json {
    arr(v, |x| opt_num(*x))
}

fn strs(v: &[Box<str>]) -> Json {
    arr(v, |s| Json::s(s))
}

/// The printer. `blip` gives the host's string for the key of a picture (`ChartHost.blip` of the TS).
pub struct Canon<'a> {
    pub blip: &'a dyn Fn(BlipKey) -> String,
}

impl Canon<'_> {
    /// `canonJson({ ...space, themeFill: space.themeFill ? true : undefined })`.
    pub fn chart_space(&self, cs: &ChartSpace) -> Json {
        let mut o = Obj::default();
        o.n("style", cs.style)
            .b("office2010", cs.office2010)
            .b("roundedCorners", cs.rounded_corners)
            .json("title", cs.title.as_ref().map(|t| self.title(t)))
            .b("autoTitleDeleted", cs.auto_title_deleted)
            .put("groups", arr(&cs.groups, |g| self.group(g)))
            .put("axes", arr(&cs.axes, |a| self.axis(a)))
            .json("plotLayout", cs.plot_layout.as_ref().map(layout))
            .put("plotSp", self.shape(&cs.plot_sp))
            .json("legend", cs.legend.as_ref().map(|l| self.legend(l)))
            .put("sp", self.shape(&cs.sp))
            .put("txPr", text_props(&cs.tx_pr))
            .st(
                "dispBlanksAs",
                match cs.disp_blanks_as {
                    DispBlanksAs::Gap => "gap",
                    DispBlanksAs::Zero => "zero",
                    DispBlanksAs::Span => "span",
                },
            )
            .json("dTable", cs.d_table.as_ref().map(|d| self.data_table(d)))
            .json("view3D", cs.view_3d.as_ref().map(view_3d))
            .json("floorSp", cs.floor_sp.as_ref().map(|s| self.shape(s)))
            .json("backWallSp", cs.back_wall_sp.as_ref().map(|s| self.shape(s)))
            .json("sideWallSp", cs.side_wall_sp.as_ref().map(|s| self.shape(s)))
            .put("accents", arr(&cs.accents, color))
            .put("tx1", color(&cs.tx1))
            .put("bg1", color(&cs.bg1))
            .string("minorFont", cs.minor_font.as_deref())
            .string("majorFont", cs.major_font.as_deref())
            .when("themeFill", cs.has_theme_fill)
            .json("themeLn", cs.theme_ln.as_ref().map(|w| Json::Arr(w.iter().map(|x| Json::Num(*x)).collect())))
            .json("userShapes", cs.user_shapes.as_ref().map(|u| arr(u, |s| self.user_shape(s))));
        o.done()
    }

    fn paint(&self, p: &Paint) -> Json {
        let mut o = Obj::default();
        match p {
            Paint::None => {
                o.st("kind", "none");
            }
            Paint::Solid { color: c, alpha } => {
                o.st("kind", "solid").put("color", color(c)).num("alpha", *alpha);
            }
            Paint::Grad { stops, angle, path } => {
                let st = arr(stops, |s| {
                    let mut g = Obj::default();
                    g.n("pos", s.pos).put("color", color(&s.color)).num("alpha", s.alpha);
                    g.done()
                });
                let path = path.as_ref().map(|p| match p {
                    GradPath::Circle => "circle",
                    GradPath::Rect => "rect",
                    GradPath::Shape => "shape",
                    GradPath::Other(s) => s,
                });
                o.st("kind", "grad").put("stops", st).n("angle", *angle).string("path", path);
            }
            Paint::Patt { fg, bg, prst } => {
                o.st("kind", "patt").put("fg", color(fg)).put("bg", color(bg)).st("prst", prst);
            }
            Paint::Blip { key, tile, fill_rect, alpha } => {
                let tile = tile.as_ref().map(|t| {
                    let mut g = Obj::default();
                    g.n("sx", t.sx).n("sy", t.sy).n("tx", t.tx).n("ty", t.ty).st("algn", &t.algn).string(
                        "flip",
                        t.flip.map(|f| match f {
                            TileFlip::X => "x",
                            TileFlip::Y => "y",
                            TileFlip::Xy => "xy",
                        }),
                    );
                    g.done()
                });
                let fr = fill_rect.as_ref().map(|r| {
                    let mut g = Obj::default();
                    g.n("l", r.l).n("t", r.t).n("r", r.r).n("b", r.b);
                    g.done()
                });
                o.st("kind", "blip").st("ref", &(self.blip)(*key)).json("tile", tile).json("fillRect", fr).num("alpha", *alpha);
            }
        }
        o.done()
    }

    fn line(&self, l: &LineProps) -> Json {
        let mut o = Obj::default();
        o.json("paint", l.paint.as_ref().map(|p| self.paint(p)))
            .num("w", l.w)
            .string("dash", l.dash.as_deref())
            .string("cap", l.cap.as_deref());
        o.done()
    }

    fn shape(&self, s: &ShapeProps) -> Json {
        let mut o = Obj::default();
        o.json("fill", s.fill.as_ref().map(|p| self.paint(p))).json("line", s.line.as_ref().map(|l| self.line(l))).json(
            "shadow",
            s.shadow.as_deref().map(|Shadow { color: c, alpha, dx, dy, blur }| {
                let mut g = Obj::default();
                g.put("color", color(c)).n("alpha", *alpha).n("dx", *dx).n("dy", *dy).n("blur", *blur);
                g.done()
            }),
        );
        o.done()
    }

    fn rich(&self, p: &RichPara) -> Json {
        let mut o = Obj::default();
        o.put(
            "runs",
            arr(&p.runs, |r| {
                let mut g = Obj::default();
                g.st("text", &r.text).put("props", text_props(&r.props));
                g.done()
            }),
        )
        .put("props", text_props(&p.props))
        .string("align", p.align.as_deref())
        .when("noDefRPr", p.no_def_rpr);
        o.done()
    }

    fn title(&self, t: &ChartTitle) -> Json {
        let mut o = Obj::default();
        o.json("paras", t.paras.as_ref().map(|ps| arr(ps, |p| self.rich(p))))
            .string("text", t.text.as_deref())
            .b("overlay", t.overlay)
            .json("layout", t.layout.as_ref().map(layout))
            .put("sp", self.shape(&t.sp))
            .put("txPr", text_props(&t.tx_pr));
        o.done()
    }

    fn d_lbls_common(&self, o: &mut Obj, d: &DLblsCommon) {
        o.boolean("delete", d.delete)
            .boolean("showVal", d.show_val)
            .boolean("showCat", d.show_cat)
            .boolean("showSer", d.show_ser)
            .boolean("showPct", d.show_pct)
            .boolean("showBubble", d.show_bubble)
            .boolean("showLegendKey", d.show_legend_key)
            .string("pos", d.pos.as_deref())
            .string("sep", d.sep.as_deref())
            .json("numFmt", d.num_fmt.as_ref().map(num_fmt))
            .put("txPr", text_props(&d.tx_pr))
            .put("sp", self.shape(&d.sp))
            .json("leaderLines", d.leader_lines.as_ref().map(|s| self.shape(s)));
    }

    fn d_lbls(&self, d: &DLbls) -> Json {
        let mut o = Obj::default();
        self.d_lbls_common(&mut o, &d.common);
        o.json("points", d.points.as_ref().map(|p| self.point_map(p, |x| self.d_lbl(x))));
        o.done()
    }

    fn d_lbl(&self, d: &DLbl) -> Json {
        let mut o = Obj::default();
        self.d_lbls_common(&mut o, &d.common);
        o.string("text", d.text.as_deref())
            .json(
                "parts",
                d.parts.as_ref().map(|ps| {
                    arr(ps, |p| {
                        let mut g = Obj::default();
                        g.st("text", &p.text).string("fld", p.fld.as_deref());
                        g.done()
                    })
                }),
            )
            .json("runProps", d.run_props.as_ref().map(text_props))
            .json("layout", d.layout.as_ref().map(layout))
            .json(
                "size",
                d.size.map(|(w, h)| {
                    let mut g = Obj::default();
                    g.n("w", w).n("h", h);
                    g.done()
                }),
            );
        o.done()
    }

    fn point_map<T>(&self, m: &PointMap<T>, f: impl Fn(&T) -> Json) -> Json {
        Json::Map(m.entries.iter().map(|(k, v)| (Json::Num(k.value()), f(v))).collect())
    }

    fn trendline(&self, t: &Trendline) -> Json {
        let mut o = Obj::default();
        o.st("kind", &t.kind)
            .put("sp", self.shape(&t.sp))
            .num("order", t.order)
            .num("period", t.period)
            .string("name", t.name.as_deref())
            .num("forward", t.forward)
            .num("backward", t.backward)
            .num("intercept", t.intercept)
            .when("dispEq", t.disp_eq)
            .when("dispRSqr", t.disp_r_sqr)
            .json(
                "label",
                t.label.as_ref().map(|TrendLabel { layout: l, num_fmt: nf, tx_pr, sp }| {
                    let mut g = Obj::default();
                    g.json("layout", l.as_ref().map(layout))
                        .json("numFmt", nf.as_ref().map(num_fmt))
                        .put("txPr", text_props(tx_pr))
                        .put("sp", self.shape(sp));
                    g.done()
                }),
            );
        o.done()
    }

    fn marker(&self, m: &Marker) -> Json {
        let mut o = Obj::default();
        o.string("symbol", m.symbol.as_deref()).num("size", m.size).put("sp", self.shape(&m.sp));
        o.done()
    }

    fn data_point(&self, p: &DataPoint) -> Json {
        let mut o = Obj::default();
        o.put("sp", self.shape(&p.sp))
            .json("marker", p.marker.as_ref().map(|m| self.marker(m)))
            .num("explosion", p.explosion)
            .boolean("invertIfNegative", p.invert_if_negative)
            .boolean("bubble3D", p.bubble_3d);
        o.done()
    }

    fn series(&self, s: &Series) -> Json {
        let mut o = Obj::default();
        o.n("idx", s.idx)
            .n("order", s.order)
            .st("name", &s.name)
            .put("values", nums(&s.values))
            .json("cats", s.cats.as_ref().map(|c| strs(c)))
            .json("catNums", s.cat_nums.as_ref().map(|c| nums(c)))
            .json("catLevels", s.cat_levels.as_ref().map(|l| arr(l, |x| strs(x))))
            .json("catFmt", s.cat_fmt.as_ref().map(fmt))
            .json("xVals", s.x_vals.as_ref().map(|c| nums(c)))
            .json("bubble", s.bubble.as_ref().map(|c| nums(c)))
            .json("fmt", s.fmt.as_ref().map(fmt))
            .json("ptFmts", s.pt_fmts.as_ref().map(|p| arr(p, |f| f.as_ref().map_or(Json::Null, fmt))))
            .put("sp", self.shape(&s.sp))
            .json("marker", s.marker.as_ref().map(|m| self.marker(m)))
            .boolean("smooth", s.smooth)
            .boolean("invertIfNegative", s.invert_if_negative)
            .num("explosion", s.explosion)
            .put("dPts", self.point_map(&s.d_pts, |p| self.data_point(p)))
            .json("dLbls", s.d_lbls.as_ref().map(|d| self.d_lbls(d)))
            .put("trendlines", arr(&s.trendlines, |t| self.trendline(t)))
            .json("box", s.box_opts.as_ref().map(box_opts));
        o.done()
    }

    fn group(&self, g: &Group) -> Json {
        let mut o = Obj::default();
        o.st("kind", group_kind(g.kind))
            .b("threeD", g.three_d)
            .n("index", f64::from(g.index))
            .st("dir", if g.dir == BarDir::Bar { "bar" } else { "col" })
            .st(
                "grouping",
                match &g.grouping {
                    Grouping::Clustered => "clustered",
                    Grouping::Stacked => "stacked",
                    Grouping::PercentStacked => "percentStacked",
                    Grouping::Standard => "standard",
                    Grouping::Other(s) => s,
                },
            )
            .b("varyColors", g.vary_colors)
            .n("gapWidth", g.gap_width)
            .n("overlap", g.overlap)
            .n("holeSize", g.hole_size)
            .n("firstSliceAng", g.first_slice_ang)
            .string("scatterStyle", g.scatter_style.as_deref())
            .string("radarStyle", g.radar_style.as_deref())
            .b("showMarker", g.show_marker)
            .put("series", arr(&g.series, |s| self.series(s)))
            .put("axIds", strs(&g.ax_ids))
            .json("dLbls", g.d_lbls.as_ref().map(|d| self.d_lbls(d)))
            .json("dropLines", g.drop_lines.as_ref().map(|s| self.shape(s)))
            .json("hiLowLines", g.hi_low_lines.as_ref().map(|s| self.shape(s)))
            .json(
                "upDownBars",
                g.up_down_bars.as_ref().map(|UpDownBars { gap_width, up, down }| {
                    let mut u = Obj::default();
                    u.n("gapWidth", *gap_width).put("up", self.shape(up)).put("down", self.shape(down));
                    u.done()
                }),
            )
            .json("serLines", g.ser_lines.as_ref().map(|s| self.shape(s)))
            .num("bubbleScale", g.bubble_scale)
            .num("gapDepth", g.gap_depth)
            .string("shape", g.shape.as_deref())
            .string("ofPieType", g.of_pie_type.as_deref())
            .json(
                "ofPie",
                g.of_pie.as_ref().map(|OfPie { split_type, split_pos, cust_split, second_pie_size }| {
                    let mut u = Obj::default();
                    u.st("splitType", split_type)
                        .num("splitPos", *split_pos)
                        .json("custSplit", cust_split.as_ref().map(|c| Json::Arr(c.iter().map(|x| Json::Num(*x)).collect())))
                        .n("secondPieSize", *second_pie_size);
                    u.done()
                }),
            )
            .string("parentLabel", g.parent_label.as_deref());
        o.done()
    }

    fn axis(&self, a: &Axis) -> Json {
        let mut o = Obj::default();
        o.st("id", &a.id)
            .st(
                "kind",
                match a.kind {
                    AxisKind::Cat => "cat",
                    AxisKind::Val => "val",
                    AxisKind::Date => "date",
                    AxisKind::Ser => "ser",
                },
            )
            .st(
                "pos",
                match &a.pos {
                    AxisPos::B => "b",
                    AxisPos::L => "l",
                    AxisPos::R => "r",
                    AxisPos::T => "t",
                    AxisPos::Other(s) => s,
                },
            )
            .b("deleted", a.deleted)
            .b("reversed", a.reversed)
            .num("min", a.min)
            .num("max", a.max)
            .num("logBase", a.log_base)
            .num("majorUnit", a.major_unit)
            .num("minorUnit", a.minor_unit)
            .string("crossAx", a.cross_ax.as_deref())
            .st(
                "crosses",
                match &a.crosses {
                    Crosses::AutoZero => "autoZero",
                    Crosses::Min => "min",
                    Crosses::Max => "max",
                    Crosses::Other(s) => s,
                },
            )
            .num("crossesAt", a.crosses_at)
            .string(
                "crossBetween",
                a.cross_between.map(|c| match c {
                    CrossBetween::Between => "between",
                    CrossBetween::MidCat => "midCat",
                }),
            )
            .json("majorGrid", a.major_grid.as_ref().map(|s| self.shape(s)))
            .json("minorGrid", a.minor_grid.as_ref().map(|s| self.shape(s)))
            .st("majorTick", tick(&a.major_tick))
            .st("minorTick", tick(&a.minor_tick))
            .st(
                "tickLblPos",
                match &a.tick_lbl_pos {
                    TickLblPos::NextTo => "nextTo",
                    TickLblPos::Low => "low",
                    TickLblPos::High => "high",
                    TickLblPos::None => "none",
                    TickLblPos::Other(s) => s,
                },
            )
            .json("numFmt", a.num_fmt.as_ref().map(num_fmt))
            .json("title", a.title.as_ref().map(|t| self.title(t)))
            .put("txPr", text_props(&a.tx_pr))
            .put("sp", self.shape(&a.sp))
            .n("lblOffset", a.lbl_offset)
            .num("tickLblSkip", a.tick_lbl_skip)
            .num("tickMarkSkip", a.tick_mark_skip)
            .b("noMultiLvlLbl", a.no_multi_lvl_lbl)
            .num("dispUnits", a.disp_units)
            .string("baseTimeUnit", a.base_time_unit.as_deref())
            .string("majorTimeUnit", a.major_time_unit.as_deref());
        o.done()
    }

    fn legend(&self, l: &Legend) -> Json {
        let mut o = Obj::default();
        o.st(
            "pos",
            match &l.pos {
                LegendPos::B => "b",
                LegendPos::T => "t",
                LegendPos::L => "l",
                LegendPos::R => "r",
                LegendPos::Tr => "tr",
                LegendPos::Other(s) => s,
            },
        )
        .b("overlay", l.overlay)
        .json("layout", l.layout.as_ref().map(layout))
        .put("deleted", point_set(&l.deleted))
        .put("txPr", text_props(&l.tx_pr))
        .put("sp", self.shape(&l.sp));
        o.done()
    }

    fn data_table(&self, d: &DataTable) -> Json {
        let mut o = Obj::default();
        o.b("horz", d.horz).b("vert", d.vert).b("outline", d.outline).b("keys", d.keys).put("txPr", text_props(&d.tx_pr));
        o.done()
    }

    fn user_text(&self, t: &UserText) -> Json {
        let mut o = Obj::default();
        let mut ins = Obj::default();
        ins.n("l", t.ins.l).n("t", t.ins.t).n("r", t.ins.r).n("b", t.ins.b);
        o.put("paras", arr(&t.paras, |p| self.rich(p)))
            .put("ins", ins.done())
            .st(
                "anchor",
                match t.anchor {
                    TextAnchor::T => "t",
                    TextAnchor::Ctr => "ctr",
                    TextAnchor::B => "b",
                },
            )
            .b("wrap", t.wrap)
            .b("clip", t.clip)
            .num("rot", t.rot)
            .put("def", text_props(&t.def));
        o.done()
    }

    fn user_shape(&self, u: &UserShape) -> Json {
        let mut o = Obj::default();
        o.n("x", u.rect.x)
            .n("y", u.rect.y)
            .n("w", u.rect.w)
            .n("h", u.rect.h)
            .when("abs", u.abs)
            .json("sub", u.sub.as_ref().map(frac_rect))
            .st("geom", &u.geom)
            .num("rot", u.rot)
            .when("flipH", u.flip_h)
            .when("flipV", u.flip_v)
            .put("sp", self.shape(&u.sp))
            .json("text", u.text.as_ref().map(|t| self.user_text(t)))
            .string("blip", u.blip.map(|k| (self.blip)(k)).as_deref());
        o.done()
    }
}

fn frac_rect(r: &FracRect) -> Json {
    let mut o = Obj::default();
    o.n("x", r.x).n("y", r.y).n("w", r.w).n("h", r.h);
    o.done()
}

fn point_set(s: &PointSet) -> Json {
    Json::Set(s.keys.iter().map(|k| Json::Num(k.value())).collect())
}

fn tick(t: &TickMark) -> &str {
    match t {
        TickMark::None => "none",
        TickMark::In => "in",
        TickMark::Out => "out",
        TickMark::Cross => "cross",
        TickMark::Other(s) => s,
    }
}

fn group_kind(k: GroupKind) -> &'static str {
    match k {
        GroupKind::Bar => "bar",
        GroupKind::Line => "line",
        GroupKind::Area => "area",
        GroupKind::Pie => "pie",
        GroupKind::Doughnut => "doughnut",
        GroupKind::OfPie => "ofPie",
        GroupKind::Scatter => "scatter",
        GroupKind::Radar => "radar",
        GroupKind::Bubble => "bubble",
        GroupKind::Stock => "stock",
        GroupKind::Surface => "surface",
        GroupKind::Box => "box",
        GroupKind::Treemap => "treemap",
        GroupKind::Sunburst => "sunburst",
    }
}

fn box_opts(b: &BoxOpts) -> Json {
    let mut o = Obj::default();
    o.b("meanLine", b.mean_line)
        .b("meanMarker", b.mean_marker)
        .b("outliers", b.outliers)
        .b("nonoutliers", b.nonoutliers)
        .b("exclusive", b.exclusive);
    o.done()
}

fn view_3d(v: &View3D) -> Json {
    let mut o = Obj::default();
    o.num("rotX", v.rot_x)
        .num("rotY", v.rot_y)
        .b("rAngAx", v.r_ang_ax)
        .n("perspective", v.perspective)
        .n("depthPercent", v.depth_percent)
        .num("hPercent", v.h_percent);
    o.done()
}

fn num_fmt(n: &NumFmt) -> Json {
    let mut o = Obj::default();
    o.put("code", fmt(&n.code)).b("linked", n.linked);
    o.done()
}

fn layout(l: &ManualLayout) -> Json {
    let mut o = Obj::default();
    o.string(
        "target",
        l.target.map(|t| match t {
            crate::model::LayoutTarget::Inner => "inner",
            crate::model::LayoutTarget::Outer => "outer",
        }),
    )
    .string("xMode", l.x_mode.map(mode))
    .string("yMode", l.y_mode.map(mode))
    .num("x", l.x)
    .num("y", l.y)
    .num("w", l.w)
    .num("h", l.h);
    o.done()
}

fn mode(m: crate::model::LayoutMode) -> &'static str {
    match m {
        crate::model::LayoutMode::Edge => "edge",
        crate::model::LayoutMode::Factor => "factor",
    }
}

/// `TextProps` → the TS object (`{}` for none set).
pub fn text_props(t: &TextProps) -> Json {
    let mut o = Obj::default();
    o.num("sz", t.sz)
        .boolean("b", t.b)
        .boolean("i", t.i)
        .boolean("u", t.u)
        .json("color", opt_color(t.color.as_ref()))
        .num("alpha", t.alpha)
        .string("font", t.font.as_deref())
        .num("rot", t.rot)
        .string("vert", t.vert.as_deref());
    o.done()
}

/// The key `recordingHost` logs for a call of `ChartHost.color`/`blip`: the canonical JSON of the XML
/// element as the TS tree has it (`name`, `attrs` sorted, `children`: strings and elements).
pub fn element_json(el: Element<'_>) -> Json {
    let mut attrs = Obj::default();
    for a in el.attrs() {
        attrs.st(a.name, a.value);
    }
    let children = el
        .children()
        .filter_map(|n| match n {
            Node::Element(e) => Some(element_json(e)),
            Node::Text(t) | Node::CData(t) => Some(Json::s(t)),
            _ => None,
        })
        .collect();
    let mut o = Obj::default();
    o.st("name", el.name()).put("attrs", attrs.done()).put("children", Json::Arr(children));
    o.done()
}
