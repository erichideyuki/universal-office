//! The canonical JSON of the shape model (feature `canon`): the text the parity captures hash
//! (`core/crates/uo-pptx/js/lib/canon-json.mjs`, rules in `core/docs/modules/uo-pptx-captures.md`).
//!
//! - object keys sorted by UTF-16 code unit, no white space; a key whose TS value is `undefined`
//!   is not written;
//! - numbers: `String(x)`, `-0` kept, NaN/±Infinity as `{"$num":"NaN"|"Infinity"|"-Infinity"}`;
//! - strings by `JSON.stringify`; over 4,096 UTF-16 units `{"$text":<units>,"sha256":<hex of the UTF-8>}`;
//! - image bytes `{"$bytes":<len>,"sha256":<hex>}`;
//! - shared references are written in full at every occurrence.
//!
//! The printer reads the Rust model with the TS field names (`Shape`, `Run`, `Theme`, …); an
//! `ImageRef` is completed with the part name and bytes of the host's media table.

use uo_drawingml::PathSeg;
use uo_jsnum::format::to_js_string;
use uo_opc::xml::{Document, Element, Node};
use uo_testkit::sha256::sha256;

use crate::color::CssColor;
use crate::model::*;
use crate::source::PartSource;
use crate::text_style::{StyleLevels, TextStyleLevel};
use crate::theme::{JsRecord, Theme};

/// A value of the canonical JSON.
#[derive(Clone, PartialEq, Debug)]
pub enum Json {
    Null,
    Bool(bool),
    Num(f64),
    Str(String),
    Arr(Vec<Json>),
    Obj(Vec<(String, Json)>),
    /// `{"$bytes":len,"sha256":hex}`
    Bytes(Vec<u8>),
}

/// Strings longer than this many UTF-16 units are digested (`LONG_TEXT`).
pub const LONG_TEXT: usize = 4096;

/// Lower-case hex of a SHA-256.
pub fn hex(d: &[u8]) -> String {
    d.iter().map(|b| format!("{b:02x}")).collect()
}

fn units(s: &str) -> usize {
    s.chars().map(char::len_utf16).sum()
}

/// UTF-16 code unit order of two strings (the JS `<` on strings).
fn cmp16(a: &str, b: &str) -> std::cmp::Ordering {
    a.encode_utf16().cmp(b.encode_utf16())
}

impl Json {
    /// A string value.
    pub fn s(v: &str) -> Json {
        Json::Str(v.to_string())
    }

    /// An object builder.
    pub fn obj() -> Obj {
        Obj(Vec::new())
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
            Json::Bytes(b) => out.push_str(&format!("{{\"$bytes\":{},\"sha256\":\"{}\"}}", b.len(), hex(&sha256(b)))),
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
pub struct Obj(pub Vec<(String, Json)>);

impl Obj {
    pub fn put(&mut self, k: &str, v: Json) -> &mut Self {
        self.0.push((k.to_string(), v));
        self
    }
    pub fn num(&mut self, k: &str, v: Option<f64>) -> &mut Self {
        if let Some(v) = v {
            self.put(k, Json::Num(v));
        }
        self
    }
    pub fn n(&mut self, k: &str, v: f64) -> &mut Self {
        self.put(k, Json::Num(v))
    }
    pub fn string(&mut self, k: &str, v: Option<&str>) -> &mut Self {
        if let Some(v) = v {
            self.put(k, Json::s(v));
        }
        self
    }
    pub fn st(&mut self, k: &str, v: &str) -> &mut Self {
        self.put(k, Json::s(v))
    }
    pub fn boolean(&mut self, k: &str, v: Option<bool>) -> &mut Self {
        if let Some(v) = v {
            self.put(k, Json::Bool(v));
        }
        self
    }
    pub fn color(&mut self, k: &str, v: Option<&CssColor>) -> &mut Self {
        if let Some(v) = v {
            self.put(k, Json::s(v.as_str()));
        }
        self
    }
    pub fn json(&mut self, k: &str, v: Option<Json>) -> &mut Self {
        if let Some(v) = v {
            self.put(k, v);
        }
        self
    }
    pub fn done(&mut self) -> Json {
        Json::Obj(std::mem::take(&mut self.0))
    }
}

fn nums(v: &[f64]) -> Json {
    Json::Arr(v.iter().map(|x| Json::Num(*x)).collect())
}

fn strs(v: &[Box<str>]) -> Json {
    Json::Arr(v.iter().map(|s| Json::s(s)).collect())
}

fn bools(v: &[bool]) -> Json {
    Json::Arr(v.iter().map(|b| Json::Bool(*b)).collect())
}

/// The printer: knows the host's media table (image part names and bytes) and part texts (charts).
pub struct Canon<'a> {
    pub src: &'a dyn PartSource,
}

impl Canon<'_> {
    /// An `ImageRef` with its part and bytes.
    pub fn image(&self, r: &ImageRef) -> Json {
        let mut o = Json::obj();
        if let Some((part, bytes)) = self.src.media_entry(r.media) {
            o.st("part", &part);
            o.put("bytes", Json::Bytes(bytes.to_vec()));
        }
        o.st("mime", &r.mime);
        if let Some(fx) = &r.fx {
            o.put("fx", image_fx(fx));
        }
        if let Some(t) = &r.tile {
            let mut to = Json::obj();
            to.n("sx", t.sx).n("sy", t.sy).n("tx", t.tx).n("ty", t.ty).string("algn", t.algn.as_deref()).string("flip", t.flip.as_deref());
            o.put("tile", to.done());
        }
        o.json("fillRect", r.fill_rect.map(inset));
        o.done()
    }

    fn opt_image(&self, o: &mut Obj, k: &str, r: Option<&ImageRef>) {
        if let Some(r) = r {
            o.put(k, self.image(r));
        }
    }

    pub fn run(&self, r: &Run) -> Json {
        let mut o = Json::obj();
        o.st("text", &r.text);
        o.num("sz", r.sz).boolean("bold", r.bold).boolean("italic", r.italic).boolean("underline", r.underline);
        o.color("color", r.color.as_ref()).string("font", r.font.as_deref()).string("lang", r.lang.as_deref());
        o.num("kern", r.kern);
        if let Some(e) = &r.extras {
            o.string("field", e.field.as_deref());
            if let Some((c, w)) = &e.fx().outline {
                let mut oo = Json::obj();
                oo.st("color", c.as_str()).n("width", *w);
                o.put("outline", oo.done());
            }
            if let Some(p) = &e.fx().patt_fill {
                o.put("pattFill", pattern(p));
            }
            o.json("gradFill", e.fx().grad_fill.as_ref().map(|f| self.fill(f)));
            self.opt_image(&mut o, "fillImage", e.fx().fill_image.as_ref());
            o.boolean("fillTile", e.fx().fill_tile).num("alpha", e.alpha).string("symFont", e.sym_font.as_deref());
            o.string("hyperlink", e.hyperlink.as_deref());
            o.json("shadow", e.fx().shadow.as_ref().map(shadow));
            o.num("baseline", e.baseline).string("cap", e.cap.as_deref()).boolean("strike", e.strike).num("spc", e.spc);
            o.color("linkColor", e.link_color.as_ref());
        }
        o.done()
    }

    pub fn paragraph(&self, p: &Paragraph) -> Json {
        let mut o = Json::obj();
        o.string("align", p.align.as_deref()).num("level", p.level);
        o.put("runs", Json::Arr(p.runs.iter().map(|r| self.run(r)).collect()));
        o.boolean("bullet", p.bullet)
            .string("bulletChar", p.bu().bullet_char.as_deref())
            .string("bulletAutoNum", p.bu().bullet_auto_num.as_deref());
        o.num("marL", p.mar_l).num("indent", p.indent).num("lineSpacing", p.line_spacing).num("lineSpacingPt", p.line_spacing_pt);
        o.json("tabs", p.x().tabs.as_deref().map(tabs)).num("defTabSz", p.def_tab_sz);
        o.num("spaceBeforePt", p.space_before_pt).num("spaceAfterPt", p.space_after_pt);
        o.num("spaceBeforePct", p.space_before_pct).num("spaceAfterPct", p.space_after_pct);
        o.string("bulletFont", p.bu().bullet_font.as_deref()).color("bulletColor", p.bu().bullet_color.as_ref());
        o.num("bulletSizePct", p.bu().bullet_size_pct).num("bulletSizePt", p.bu().bullet_size_pt);
        self.opt_image(&mut o, "bulletImage", p.bu().bullet_image.as_deref());
        o.num("endSz", p.end_sz)
            .boolean("eaLnBrk", p.x().ea_ln_brk)
            .boolean("hangingPunct", p.x().hanging_punct)
            .string("fontAlgn", p.x().font_algn.as_deref());
        o.done()
    }

    pub fn text_body(&self, t: &TextBody) -> Json {
        let mut o = Json::obj();
        o.string("anchor", t.anchor.as_deref());
        o.put("paragraphs", Json::Arr(t.paragraphs.iter().map(|p| self.paragraph(p)).collect()));
        o.string("wrap", t.wrap.as_deref()).num("fontScale", t.x().font_scale);
        o.string(
            "autofit",
            t.autofit.map(|a| match a {
                Autofit::Norm => "norm",
                Autofit::None => "none",
                Autofit::Shape => "shape",
            }),
        );
        o.num("lnSpcReduction", t.x().ln_spc_reduction).boolean("spcFirstLastPara", t.x().spc_first_last_para);
        if let Some(w) = &t.x().warp {
            let mut wo = Json::obj();
            wo.st("prst", &w.prst).json("adj", w.adj.as_deref().map(nums)).boolean("fromWordArt", w.from_word_art);
            o.put("warp", wo.done());
        }
        o.num("insL", t.ins_l).num("insT", t.ins_t).num("insR", t.ins_r).num("insB", t.ins_b);
        o.string("vert", t.x().vert.as_deref()).num("rot", t.x().rot).num("numCol", t.x().num_col).num("spcCol", t.x().spc_col);
        o.string("vertOverflow", t.x().vert_overflow.as_deref()).string("horzOverflow", t.x().horz_overflow.as_deref());
        o.json("lstStyle", t.lst_style.as_deref().map(|l| self.levels(l)));
        o.done()
    }

    pub fn level(&self, l: &TextStyleLevel) -> Json {
        let mut o = Json::obj();
        o.json("tabs", l.p().tabs.as_deref().map(tabs))
            .num("defTabSz", l.p().def_tab_sz)
            .num("sz", l.r().sz)
            .boolean("bold", l.r().bold)
            .boolean("italic", l.r().italic);
        o.color("color", l.r().color.as_ref()).string("font", l.r().font.as_deref()).string("align", l.p().align.as_deref());
        o.string("buChar", l.b().bu_char.as_deref()).string("buAutoNum", l.b().bu_auto_num.as_deref()).boolean("buNone", l.b().bu_none);
        o.num("marL", l.p().mar_l).num("indent", l.p().indent).json("shadow", l.r().shadow.as_ref().map(shadow));
        o.num("lineSpacing", l.p().line_spacing).num("lineSpacingPt", l.p().line_spacing_pt);
        o.num("spaceBeforePt", l.p().space_before_pt).num("spaceAfterPt", l.p().space_after_pt);
        o.num("spaceBeforePct", l.p().space_before_pct).num("spaceAfterPct", l.p().space_after_pct);
        o.string("buFont", l.b().bu_font.as_deref())
            .color("buClr", l.b().bu_clr.as_ref())
            .num("buSzPct", l.b().bu_sz_pct)
            .num("buSzPt", l.b().bu_sz_pt);
        self.opt_image(&mut o, "buBlip", l.b().bu_blip.as_deref());
        o.num("baseline", l.r().baseline)
            .string("cap", l.r().cap.as_deref())
            .boolean("strike", l.r().strike)
            .num("spc", l.r().spc)
            .num("kern", l.r().kern);
        o.done()
    }

    pub fn levels(&self, l: &StyleLevels) -> Json {
        Json::Arr(l.iter().map(|x| self.level(x)).collect())
    }

    pub fn fill(&self, f: &Fill) -> Json {
        let mut o = Json::obj();
        o.st(
            "type",
            match f.kind {
                FillKind::Solid => "solid",
                FillKind::None => "none",
                FillKind::Gradient => "gradient",
            },
        );
        o.color("color", f.color.as_ref());
        if let Some(stops) = &f.stops {
            o.put(
                "stops",
                Json::Arr(
                    stops
                        .iter()
                        .map(|s| {
                            let mut so = Json::obj();
                            so.n("pos", s.pos).st("color", s.color.as_str()).num("alpha", s.alpha);
                            so.done()
                        })
                        .collect(),
                ),
            );
        }
        o.num("angleDeg", f.angle_deg).boolean("scaled", f.scaled).string("path", f.path.as_deref());
        o.json("fillTo", f.fill_to.map(inset));
        o.json("pattern", f.pattern.as_deref().map(pattern));
        o.done()
    }

    fn table_line(l: &TableLine) -> Json {
        let mut o = Json::obj();
        o.st("color", l.color.as_str())
            .n("width", l.width)
            .num("alpha", l.alpha)
            .string("dash", l.dash.as_deref())
            .string("cmpd", l.cmpd.as_deref());
        o.done()
    }

    fn cell_sides<T>(s: &Sides<T>, f: impl Fn(&T) -> Json) -> Json {
        let mut o = Json::obj();
        for (k, v) in [("l", &s.l), ("r", &s.r), ("t", &s.t), ("b", &s.b)] {
            if let Some(v) = v {
                o.put(k, f(v));
            }
        }
        o.done()
    }

    pub fn table(&self, t: &Table) -> Json {
        let mut o = Json::obj();
        o.put("colWidthsEmu", nums(&t.col_widths_emu));
        if let Some((c, a)) = &t.bg {
            let mut b = Json::obj();
            b.st("color", c.as_str()).num("alpha", *a);
            o.put("bg", b.done());
        }
        if let Some(b) = &t.bg_paint {
            let mut p = Json::obj();
            p.json("fill", b.fill.as_ref().map(|f| self.fill(f))).num("alpha", b.alpha);
            self.opt_image(&mut p, "image", b.image.as_deref());
            p.boolean("tile", b.tile);
            o.put("bgPaint", p.done());
        }
        o.json("shadow", t.shadow.as_ref().map(shadow));
        o.put(
            "rows",
            Json::Arr(
                t.rows
                    .iter()
                    .map(|r| {
                        let mut ro = Json::obj();
                        ro.n("heightEmu", r.height_emu);
                        ro.put("cells", Json::Arr(r.cells.iter().map(|c| self.cell(c)).collect()));
                        ro.done()
                    })
                    .collect(),
            ),
        );
        for (k, g) in [("hLines", &t.h_lines), ("vLines", &t.v_lines)] {
            if let Some(g) = g {
                let rows = (0..g.rows as usize)
                    .map(|r| {
                        Json::Arr(
                            (0..g.cols as usize)
                                .map(|c| match g.get(r, c) {
                                    Some(l) => Self::table_line(l),
                                    None => Json::Null,
                                })
                                .collect(),
                        )
                    })
                    .collect();
                o.put(k, Json::Arr(rows));
            }
        }
        o.done()
    }

    pub fn cell(&self, c: &TableCell) -> Json {
        let mut o = Json::obj();
        o.num("gridSpan", c.grid_span).num("rowSpan", c.row_span).boolean("hMerge", c.h_merge).boolean("vMerge", c.v_merge);
        o.json("txBody", c.tx_body.as_deref().map(|t| self.text_body(t)));
        o.color("fill", c.fill.as_ref())
            .color("borderColor", c.border_color.as_ref())
            .num("borderWidth", c.border_width)
            .num("fillAlpha", c.fill_alpha);
        o.json(
            "borders",
            c.borders.as_deref().map(|b| {
                Self::cell_sides(b, |e| {
                    let mut eo = Json::obj();
                    eo.st("color", e.color.as_str()).n("width", e.width).string("dash", e.dash.as_deref());
                    eo.done()
                })
            }),
        );
        o.string("anchor", c.anchor.as_deref()).string("vert", c.vert.as_deref()).boolean("noFill", c.no_fill);
        o.json("borderNone", c.border_none.as_ref().map(|b| Self::cell_sides(b, |v| Json::Bool(*v))));
        o.num("marL", c.mar_l).num("marR", c.mar_r).num("marT", c.mar_t).num("marB", c.mar_b);
        o.color("txColor", c.tx_color.as_ref()).boolean("txBold", c.tx_bold).boolean("txItalic", c.tx_italic);
        o.json("fillPaint", c.fill_paint.as_deref().map(|f| self.fill(f)));
        self.opt_image(&mut o, "fillImage", c.fill_image.as_deref());
        o.boolean("fillTile", c.fill_tile);
        o.json(
            "lines",
            c.lines.as_deref().map(|l| {
                Self::cell_sides(l, |v| match v {
                    CellLine::NoFill => Json::Null,
                    CellLine::Line(x) => Self::table_line(x),
                })
            }),
        );
        o.done()
    }

    pub fn shape(&self, s: &Shape) -> Json {
        let mut o = Json::obj();
        o.st("id", &s.id).st("name", &s.name);
        o.string("phType", s.ph_type.as_deref()).string("phIdx", s.ph_idx.as_deref()).boolean("isTextBox", s.is_text_box);
        o.json(
            "xfrm",
            s.xfrm.map(|x| {
                let mut xo = Json::obj();
                xo.n("x", x.x)
                    .n("y", x.y)
                    .n("cx", x.cx)
                    .n("cy", x.cy)
                    .num("rot", x.rot)
                    .boolean("flipH", x.flip_h)
                    .boolean("flipV", x.flip_v);
                xo.done()
            }),
        );
        o.string("geom", s.geom.as_deref()).boolean("grpFill", s.grp_fill).json("adj", s.x().adj.as_deref().map(nums));
        o.json("adjNames", s.x().adj_names.as_deref().map(strs));
        o.json("custGeom", s.cust_geom.as_deref().map(cust_geom));
        o.json("fill", s.fill.as_deref().map(|f| self.fill(f)));
        o.color("lineColor", s.line_color.as_ref())
            .num("lineAlpha", s.x().line_alpha)
            .num("lineWidth", s.line_width)
            .boolean("lineNone", s.line_none);
        o.string("lineDash", s.x().line_dash.as_deref());
        o.json("txBody", s.tx_body.as_deref().map(|t| self.text_body(t)));
        self.opt_image(&mut o, "image", s.paint().image.as_deref());
        o.json("srcRect", s.paint().src_rect.map(inset))
            .num("alpha", s.paint().alpha)
            .boolean("useBgFill", s.paint().use_bg_fill)
            .num("fillAlpha", s.paint().fill_alpha);
        self.opt_image(&mut o, "fillImage", s.paint().fill_image.as_deref());
        o.boolean("fillTile", s.paint().fill_tile).boolean("fillNoRot", s.paint().fill_no_rot);
        o.color("styleFontColor", s.sf().color.as_ref()).string("styleFontIdx", s.sf().idx.as_deref()).boolean("inherited", s.inherited);
        if let Some(e) = &s.extras {
            o.string("descr", e.descr.as_deref()).string("dgmId", e.dgm_id.as_deref()).string("dgmTextOf", e.dgm_text_of.as_deref());
            if let Some(m) = &e.media {
                let mut mo = Json::obj();
                mo.st("kind", if m.kind == MediaKind::Video { "video" } else { "audio" })
                    .string("part", m.part.as_deref())
                    .string("url", m.url.as_deref());
                mo.st("mime", &m.mime);
                if let Some((a, b)) = m.trim {
                    let mut t = Json::obj();
                    t.n("startMs", a).n("endMs", b);
                    mo.put("trim", t.done());
                }
                if let Some((a, b)) = m.fade {
                    let mut t = Json::obj();
                    t.n("inMs", a).n("outMs", b);
                    mo.put("fade", t.done());
                }
                if let Some(b) = &m.bookmarks {
                    mo.put(
                        "bookmarks",
                        Json::Arr(
                            b.iter()
                                .map(|x| {
                                    let mut bo = Json::obj();
                                    bo.st("name", &x.name).n("timeMs", x.time_ms);
                                    bo.done()
                                })
                                .collect(),
                        ),
                    );
                }
                o.put("media", mo.done());
            }
            if let Some(c) = &e.chart {
                // `chartXml`: the chart part's text, as the TS keeps it in every shape
                if let Some(t) = self.src.text(&c.part) {
                    o.put("chartXml", Json::Str(t));
                }
                o.st("chartPart", &c.part);
                o.put("chartTheme", self.theme(&c.theme));
                if let Some(imgs) = &c.images {
                    let mut io = Json::obj();
                    for ci in imgs.iter() {
                        io.put(&ci.r_id, self.image(&ci.image));
                    }
                    o.put("chartImages", io.done());
                }
                o.string(
                    "chartFormat",
                    c.format.map(|f| match f {
                        ChartFormat::Mso2007 => "mso2007",
                        ChartFormat::Modern => "modern",
                    }),
                );
            }
            o.json("table", e.table.as_deref().map(|t| self.table(t)));
            o.json("shadow", e.fx().shadow.as_ref().map(shadow));
            if let Some(g) = &e.fx().glow {
                let mut go = Json::obj();
                go.n("rad", g.rad).st("color", g.color.as_str()).n("alpha", g.alpha);
                o.put("glow", go.done());
            }
            o.num("softEdge", e.fx().soft_edge);
            if let Some(r) = &e.fx().reflection {
                let mut ro = Json::obj();
                ro.n("blurRad", r.blur_rad).n("stA", r.st_a).n("endA", r.end_a).n("stPos", r.st_pos).n("endPos", r.end_pos);
                ro.n("dist", r.dist).n("dirDeg", r.dir_deg).n("sy", r.sy).n("fadeDirDeg", r.fade_dir_deg);
                o.put("reflection", ro.done());
            }
            if let Some(b) = &e.fx().bevel {
                let mut bo = Json::obj();
                bo.n("w", b.w)
                    .n("h", b.h)
                    .string("prst", b.prst.as_deref())
                    .string("rig", b.rig.as_deref())
                    .string("dir", b.dir.as_deref());
                bo.string("mat", b.mat.as_deref());
                o.put("bevel", bo.done());
            }
            if let Some(x) = &e.fx().extrusion {
                let mut xo = Json::obj();
                xo.n("h", x.h).color("color", x.color.as_ref()).string("rig", x.rig.as_deref()).string("dir", x.dir.as_deref());
                xo.string("mat", x.mat.as_deref());
                o.put("extrusion", xo.done());
            }
            o.string("camPrst", e.fx().cam_prst.as_deref());
            if let Some(c) = &e.fx().cam_rot {
                let mut co = Json::obj();
                co.n("lat", c.lat).n("lon", c.lon).n("rev", c.rev);
                o.put("camRot", co.done());
            }
            if let Some(i) = &e.fx().inner_shadow {
                let mut io = Json::obj();
                io.st("color", i.color.as_str()).n("blurRad", i.blur_rad).n("dist", i.dist).n("dirDeg", i.dir_deg).n("alpha", i.alpha);
                o.put("innerShadow", io.done());
            }
            if let Some(f) = &e.fx().fill_overlay {
                let mut fo = Json::obj();
                fo.st("blend", &f.blend).put("fill", self.fill(&f.fill));
                o.put("fillOverlay", fo.done());
            }
            o.string("headArrow", e.head_arrow.as_deref()).string("tailArrow", e.tail_arrow.as_deref());
            o.json("groupIds", e.group_ids.as_deref().map(strs));
        }
        o.done()
    }

    /// A `TextLayoutResult` (`text-layout.ts:79`) with the TS field names: the text-layout capture hashes it.
    pub fn layout(&self, r: &crate::text::TextLayoutResult) -> Json {
        let font = |f: &crate::text::LayoutFont| {
            let mut o = Json::obj();
            o.st("family", &f.family).n("sizePt", f.size_pt).put("bold", Json::Bool(f.bold)).put("italic", Json::Bool(f.italic));
            if f.kern {
                o.put("kern", Json::Bool(true));
            }
            o.done()
        };
        let lines = r
            .lines
            .iter()
            .map(|l| {
                let runs = l
                    .runs
                    .iter()
                    .map(|pr| {
                        let mut o = Json::obj();
                        o.st("text", &pr.text).n("x", pr.x).n("w", pr.w).put("font", font(&pr.font)).st("color", pr.color.as_str());
                        o.put("underline", Json::Bool(pr.underline)).string("hyperlink", pr.hyperlink.as_deref());
                        o.json("shadow", pr.shadow.as_ref().map(shadow)).num("rise", pr.rise);
                        if pr.strike {
                            o.put("strike", Json::Bool(true));
                        }
                        o.num("spc", pr.spc);
                        if let Some((c, w)) = &pr.outline {
                            let mut oo = Json::obj();
                            oo.st("color", c.as_str()).n("width", *w);
                            o.put("outline", oo.done());
                        }
                        o.json("pattFill", pr.patt_fill.as_ref().map(pattern));
                        o.json("gradFill", pr.grad_fill.as_ref().map(|f| self.fill(f)));
                        self.opt_image(&mut o, "fillImage", pr.fill_image.as_ref());
                        if pr.fill_tile {
                            o.put("fillTile", Json::Bool(true));
                        }
                        o.num("alpha", pr.alpha);
                        o.done()
                    })
                    .collect();
                let mut o = Json::obj();
                o.put("runs", Json::Arr(runs)).n("top", l.top).n("baseline", l.baseline).n("height", l.height).n("para", f64::from(l.para));
                if let Some(b) = &l.bullet {
                    let mut bo = Json::obj();
                    bo.st("text", &b.text).n("x", b.x).put("font", font(&b.font)).st("color", b.color.as_str());
                    self.opt_image(&mut bo, "image", b.image.as_ref());
                    o.put("bullet", bo.done());
                }
                o.done()
            })
            .collect();
        let mut o = Json::obj();
        o.put("lines", Json::Arr(lines)).n("totalH", r.total_h);
        o.done()
    }

    pub fn theme(&self, t: &Theme) -> Json {
        let mut o = Json::obj();
        o.put("colors", record_colors(&t.colors));
        o.json("scheme", t.scheme.as_ref().map(record_colors));
        if let Some(f) = &t.fonts {
            let mut fo = Json::obj();
            fo.string("major", f.major.as_deref()).string("minor", f.minor.as_deref());
            o.put("fonts", fo.done());
        }
        if let Some(m) = &t.clr_map {
            let mut mo = Json::obj();
            for (k, v) in m.iter() {
                mo.st(k, v);
            }
            o.put("clrMap", mo.done());
        }
        o.json("lineWidths", t.line_widths.as_deref().map(nums));
        if let Some(s) = &t.styles {
            let list = |ids: &Option<Vec<uo_opc::xml::ElementId>>| -> Option<Json> {
                let ids = ids.as_ref()?;
                Some(Json::Arr(ids.iter().filter_map(|id| s.doc.element(*id)).map(xml_element).collect()))
            };
            o.json("fillStyles", list(&s.fill)).json("bgFillStyles", list(&s.bg_fill)).json("effectStyles", list(&s.effect));
        }
        o.string("part", t.part.as_deref());
        o.done()
    }
}

fn record_colors(r: &JsRecord<CssColor>) -> Json {
    let mut o = Json::obj();
    for (k, v) in r.iter() {
        o.st(k, v.as_str());
    }
    o.done()
}

/// A parsed `XmlElement` as the TS keeps it: `{name, attrs, children}`.
pub fn xml_element(e: Element<'_>) -> Json {
    let mut o = Json::obj();
    o.st("name", e.name());
    let mut attrs = Json::obj();
    for a in e.attrs() {
        attrs.st(a.name, a.value);
    }
    o.put("attrs", attrs.done());
    let children: Vec<Json> = e
        .children()
        .filter_map(|n| match n {
            Node::Element(c) => Some(xml_element(c)),
            Node::Text(t) | Node::CData(t) => Some(Json::s(t)),
            _ => None,
        })
        .collect();
    o.put("children", Json::Arr(children));
    o.done()
}

/// The root of a document as a TS `XmlElement`.
pub fn xml_document(d: &Document) -> Json {
    xml_element(d.root())
}

fn inset(i: Inset) -> Json {
    let mut o = Json::obj();
    o.n("l", i.l).n("t", i.t).n("r", i.r).n("b", i.b);
    o.done()
}

fn pattern(p: &PatternFill) -> Json {
    let mut o = Json::obj();
    o.st("prst", &p.prst).st("fg", p.fg.as_str()).st("bg", p.bg.as_str()).num("fgA", p.fg_a).num("bgA", p.bg_a);
    o.done()
}

fn shadow(s: &OuterShadow) -> Json {
    let mut o = Json::obj();
    o.st("color", s.color.as_str()).n("blurRad", s.blur_rad).n("dist", s.dist).n("dirDeg", s.dir_deg).n("alpha", s.alpha);
    o.num("sx", s.sx).num("sy", s.sy).string("algn", s.algn.as_deref());
    o.done()
}

fn tabs(t: &[TabStop]) -> Json {
    Json::Arr(
        t.iter()
            .map(|x| {
                let mut o = Json::obj();
                o.n("pos", x.pos).st("algn", &x.algn);
                o.done()
            })
            .collect(),
    )
}

fn image_fx(fx: &ImageFx) -> Json {
    let mut o = Json::obj();
    if let Some([a, b]) = &fx.duotone {
        o.put("duotone", Json::Arr(vec![Json::s(a.as_str()), Json::s(b.as_str())]));
    }
    o.num("alpha", fx.alpha).boolean("gray", fx.gray).num("biLevel", fx.bi_level).num("bright", fx.bright).num("contrast", fx.contrast);
    if let Some(c) = &fx.clr_change {
        let mut co = Json::obj();
        co.st("from", c.from.as_str()).st("to", c.to.as_str()).n("toAlpha", c.to_alpha);
        o.put("clrChange", co.done());
    }
    o.done()
}

/// A path segment as the TS object (`{ c: 'M', x, y }`…).
pub fn seg(s: &PathSeg) -> Json {
    let mut o = Json::obj();
    match *s {
        PathSeg::M { x, y } => o.st("c", "M").n("x", x).n("y", y),
        PathSeg::L { x, y } => o.st("c", "L").n("x", x).n("y", y),
        PathSeg::C { x1, y1, x2, y2, x, y } => o.st("c", "C").n("x1", x1).n("y1", y1).n("x2", x2).n("y2", y2).n("x", x).n("y", y),
        PathSeg::Q { x1, y1, x, y } => o.st("c", "Q").n("x1", x1).n("y1", y1).n("x", x).n("y", y),
        PathSeg::E { cx, cy, rx, ry, a0, a1, ccw } => {
            o.st("c", "E").n("cx", cx).n("cy", cy).n("rx", rx).n("ry", ry).n("a0", a0).n("a1", a1).put("ccw", Json::Bool(ccw))
        }
        PathSeg::Z => o.st("c", "Z"),
    };
    o.done()
}

fn cust_geom(c: &CustGeom) -> Json {
    let mut o = Json::obj();
    o.n("w", c.w).n("h", c.h);
    o.put("subs", Json::Arr(c.subs.iter().map(|s| Json::Arr(s.iter().map(seg).collect())).collect()));
    o.json("noFill", c.no_fill.as_deref().map(bools))
        .json("noStroke", c.no_stroke.as_deref().map(bools))
        .json("lum", c.lum.as_deref().map(nums));
    o.json("textRect", c.text_rect.map(inset));
    o.done()
}
