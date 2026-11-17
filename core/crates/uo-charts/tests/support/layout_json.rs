//! The layout facets of the capture (`lay.<view>.<method>`): the arguments and results of the patched `Renderer` methods as
//! the capture's `plain()` + `canonJson` print them (`js/capture-charts.mjs`): model objects as `{"$ref":"g0.s1"}`,
//! `undefined` as `{"$undef":1}`, numbers by the canonical rules.

use uo_charts::LayoutCall;
use uo_charts::canon::Json;
use uo_charts::emit::Rect;
use uo_charts::layout::{AxisDir, AxisInfo, HNode, KeyKind, LegendEntry, LegendLayout, ResolvedText, Scale, TitleLayout, TrendCurve};
use uo_charts::model::SeriesRef;

fn num(x: f64) -> Json {
    Json::Num(x)
}

fn obj(kv: Vec<(&str, Json)>) -> Json {
    Json::Obj(kv.into_iter().map(|(k, v)| (k.to_owned(), v)).collect())
}

fn undef() -> Json {
    obj(vec![("$undef", num(1.0))])
}

fn rect(r: &Rect) -> Json {
    obj(vec![("x", num(r.x)), ("y", num(r.y)), ("w", num(r.w)), ("h", num(r.h))])
}

fn text_style(t: &ResolvedText) -> Json {
    let mut o = vec![
        ("sz", num(t.sz)),
        ("b", Json::Bool(t.b)),
        ("i", Json::Bool(t.i)),
        ("u", Json::Bool(t.u)),
        ("color", Json::s(t.color.as_str())),
        ("alpha", num(t.alpha)),
        ("font", Json::s(&t.font)),
    ];
    if let Some(r) = t.rot {
        o.push(("rot", num(r)));
    }
    if let Some(v) = &t.vert {
        o.push(("vert", Json::s(v)));
    }
    obj(o)
}

fn refj(s: &str) -> Json {
    obj(vec![("$ref", Json::s(s))])
}

fn series_ref(r: SeriesRef) -> String {
    format!("g{}.s{}", r.group, r.series)
}

fn entry(e: &LegendEntry) -> Json {
    let mut o = vec![
        ("label", Json::s(&e.label)),
        ("kind", Json::s(if e.kind == KeyKind::Line { "line" } else { "box" })),
        ("s", refj(&series_ref(e.series))),
        ("g", refj(&format!("g{}", e.series.group))),
    ];
    if let Some(p) = e.pt {
        o.push(("pt", num(f64::from(p))));
    }
    if let Some(t) = e.trendline {
        o.push(("tl", refj(&format!("{}.t{t}", series_ref(e.series)))));
    }
    obj(o)
}

fn title(t: &TitleLayout) -> Json {
    let lines = t
        .lines
        .iter()
        .map(|ln| Json::Arr(ln.iter().map(|p| obj(vec![("text", Json::s(&p.text)), ("t", text_style(&p.style))])).collect()))
        .collect();
    obj(vec![
        ("lines", Json::Arr(lines)),
        ("x", num(t.x)),
        ("y", num(t.y)),
        ("w", num(t.w)),
        ("h", num(t.h)),
        ("overlay", Json::Bool(t.overlay)),
    ])
}

fn legend(l: &LegendLayout) -> Json {
    let rows = l
        .rows
        .iter()
        .map(|r| {
            let mut o = vec![("x", num(r.x)), ("y", num(r.y)), ("e", l.entries.get(r.entry as usize).map_or_else(undef, entry))];
            if let Some(ls) = &r.lines {
                o.push(("lines", Json::Arr(ls.iter().map(|s| Json::s(s)).collect())));
            }
            obj(o)
        })
        .collect();
    obj(vec![
        ("entries", Json::Arr(l.entries.iter().map(entry).collect())),
        ("t", text_style(&l.style)),
        ("rows", Json::Arr(rows)),
        ("box", rect(&l.bx)),
        ("rest", rect(&l.rest)),
        ("overlay", Json::Bool(l.overlay)),
    ])
}

fn scale(sc: &Scale) -> Json {
    let mut o = vec![("lo", num(sc.lo)), ("hi", num(sc.hi)), ("major", num(sc.major)), ("minor", num(sc.minor))];
    if let Some(l) = sc.log {
        o.push(("log", num(l)));
    }
    obj(o)
}

fn strs(v: &[Box<str>]) -> Json {
    Json::Arr(v.iter().map(|s| Json::s(s)).collect())
}

fn axis_info(i: &AxisInfo) -> Json {
    let mut o = vec![
        ("ax", refj(&format!("ax{}", i.ax.0))),
        ("dir", Json::s(if i.dir == AxisDir::H { "h" } else { "v" })),
        ("fmt", Json::s(&String::from_utf16_lossy(i.fmt.units()))),
        ("groups", Json::Arr(i.groups.iter().map(|g| refj(&format!("g{g}"))).collect())),
        ("showLabels", Json::Bool(i.show_labels)),
        ("extent", num(i.extent)),
        ("labelRot", num(i.label_rot)),
        ("labelLines", Json::Arr(i.label_lines.iter().map(|l| strs(l)).collect())),
        ("labelSkip", num(i.label_skip)),
        ("secondary", Json::Bool(i.secondary)),
    ];
    if let Some(c) = i.cross {
        o.push(("cross", refj(&format!("ax{}", c.0))));
    }
    if let Some(sc) = &i.scale {
        o.push(("scale", scale(sc)));
    }
    if let Some(c) = &i.cats {
        o.push(("cats", strs(c)));
    }
    if let Some(l) = &i.cat_levels {
        o.push(("catLevels", Json::Arr(l.iter().map(|x| strs(x)).collect())));
    }
    if let Some(n) = i.n_cats {
        o.push(("nCats", num(n)));
    }
    if let Some(b) = i.on_ticks {
        o.push(("onTicks", Json::Bool(b)));
    }
    if let Some(t) = i.title_extent {
        o.push(("titleExtent", num(t)));
    }
    if let Some(idx) = &i.label_idx {
        o.push(("labelIdx", Json::Arr(idx.iter().map(|k| num(f64::from(*k))).collect())));
    }
    if let Some(d) = &i.dates {
        o.push(("dates", Json::Arr(d.iter().map(|x| x.map_or(Json::Null, num)).collect())));
    }
    obj(o)
}

fn throw(m: &str) -> Json {
    obj(vec![("$throw", Json::s(m))])
}

fn unit_res(r: &Result<(), &'static str>) -> Json {
    match r {
        Ok(()) => undef(),
        Err(m) => throw(m),
    }
}

fn pts(p: &[(f64, f64)]) -> Json {
    Json::Arr(p.iter().map(|(x, y)| obj(vec![("x", num(*x)), ("y", num(*y))])).collect())
}

fn trend_curve(tc: &TrendCurve) -> Json {
    let mut fit: Vec<(&str, Json)> = Vec::new();
    if let Some(p) = &tc.fit.points {
        fit.push(("points", pts(p)));
    }
    if let Some(r) = tc.fit.r2 {
        fit.push(("r2", num(r)));
    }
    if let Some(eq) = &tc.fit.eq {
        let parts = eq.iter().map(|e| {
            let mut k = vec![("text", Json::s(&e.text))];
            if e.sup {
                k.push(("sup", Json::Bool(true)));
            }
            obj(k)
        });
        fit.push(("eq", Json::Arr(parts.collect())));
    }
    obj(vec![("pts", pts(&tc.pts)), ("fit", obj(fit))])
}

fn hnode(n: &HNode) -> Json {
    obj(vec![
        ("name", Json::s(&n.name)),
        ("size", num(n.size)),
        ("children", Json::Arr(n.children.iter().map(hnode).collect())),
        ("top", num(n.top)),
        ("leaf", num(n.leaf)),
    ])
}

/// `(method, canonJson({args, res}))` of a recorded call (`None` for a call that never ended).
pub fn call_line(c: &LayoutCall) -> Option<(&'static str, String)> {
    let (m, args, res) = match c {
        LayoutCall::Pending => return None,
        LayoutCall::Title { inner, res } => ("layoutTitle", vec![rect(inner)], res.as_ref().map_or_else(undef, title)),
        LayoutCall::Legend { inner, extra_b, res } => {
            ("layoutLegend", vec![rect(inner), num(*extra_b)], res.as_ref().map_or_else(undef, legend))
        }
        LayoutCall::LegendEntries { res } => ("legendEntries", Vec::new(), Json::Arr(res.iter().map(entry).collect())),
        LayoutCall::PrepValueAxis { inf, len, res } => {
            let mut a = vec![axis_info(inf)];
            if let Some(l) = len {
                a.push(num(*l));
            }
            ("prepValueAxis", a, unit_res(res))
        }
        LayoutCall::PrepCatAxis { inf } => ("prepCatAxis", vec![axis_info(inf)], undef()),
        LayoutCall::MeasureAxis { inf, plot, res } => ("measureAxis", vec![axis_info(inf), rect(plot)], unit_res(res)),
        LayoutCall::PlotRect { outer, infos, res } => (
            "plotRect",
            vec![rect(outer), Json::Arr(infos.iter().map(axis_info).collect())],
            match res {
                Ok(r) => rect(r),
                Err(m) => throw(m),
            },
        ),
        LayoutCall::DateLabels { dates, band, need, fixed, res } => (
            "dateLabels",
            vec![
                Json::Arr(dates.iter().map(|d| d.map_or(Json::Null, num)).collect()),
                num(*band),
                num(*need),
                fixed.map_or_else(undef, |(u, k)| Json::Arr(vec![Json::s(&u.to_string()), num(k)])),
            ],
            Json::Arr(res.iter().map(|v| num(*v)).collect()),
        ),
        LayoutCall::ValueLabels { inf, res } => {
            let labels = res.iter().map(|l| {
                let mut o = vec![("s", Json::s(&l.s)), ("v", num(l.v))];
                if let Some(c) = &l.color {
                    o.push(("color", Json::s(c)));
                }
                obj(o)
            });
            ("valueLabels", vec![axis_info(inf)], Json::Arr(labels.collect()))
        }
        LayoutCall::TrendCurve { group, series, trend, res } => (
            "trendCurve",
            vec![refj(&format!("g{group}")), refj(&format!("g{group}.s{series}")), refj(&format!("g{group}.s{series}.t{trend}"))],
            match res {
                Ok(Some(tc)) => trend_curve(tc),
                Ok(None) => undef(),
                Err(m) => throw(m),
            },
        ),
        LayoutCall::Hierarchy { group, res } => ("hierarchy", vec![refj(&format!("g{group}"))], hnode(res)),
    };
    Some((m, obj(vec![("args", Json::Arr(args)), ("res", res)]).to_text()))
}
