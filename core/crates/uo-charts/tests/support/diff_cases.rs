//! C-2: the differential cases of `charts-diff.cases.jsonl.gz` run against `scale`, `trend` and `chartex::histogram_bins`: the
//! argument array (canonical JSON) → the result as the canonical JSON text the capture digests (`js/lib/diff.mjs`).

use uo_charts::canon::Json;
use uo_charts::chartex::histogram_bins;
use uo_charts::layout::Scale;
use uo_charts::scale::{Fixed, auto_scale, frac, nice_step, tick_values};
use uo_charts::trend::{TrendOpts, fit_trend, trend_num};
use uo_jsnum::math::{max_n, min_n};
use uo_opc::xml::{ParseOptions, parse};

use super::data::DiffCase;
use super::json::{self, J};

fn num(j: &J) -> f64 {
    j.num().unwrap_or(f64::NAN)
}

fn opt(j: &J, k: &str) -> Option<f64> {
    j.get(k).and_then(J::num)
}

fn scale_of(j: &J) -> Scale {
    Scale {
        lo: opt(j, "lo").unwrap_or(f64::NAN),
        hi: opt(j, "hi").unwrap_or(f64::NAN),
        major: opt(j, "major").unwrap_or(f64::NAN),
        minor: opt(j, "minor").unwrap_or(f64::NAN),
        log: opt(j, "log"),
    }
}

fn scale_json(s: &Scale) -> Json {
    let mut o = vec![
        ("lo".to_owned(), Json::Num(s.lo)),
        ("hi".to_owned(), Json::Num(s.hi)),
        ("major".to_owned(), Json::Num(s.major)),
        ("minor".to_owned(), Json::Num(s.minor)),
    ];
    if let Some(l) = s.log {
        o.push(("log".to_owned(), Json::Num(l)));
    }
    Json::Obj(o)
}

fn throw(msg: &str) -> Json {
    Json::Obj(vec![("$throw".to_owned(), Json::s(msg))])
}

/// A `cx:` node of the canonical JSON as XML text (the names are parsed leniently, as the Slides chart parts).
fn node_xml(j: &J, out: &mut String) {
    let name = j.get("name").and_then(J::str).unwrap_or("x");
    out.push('<');
    out.push_str(name);
    if let Some(J::Obj(attrs)) = j.get("attrs") {
        for (k, v) in attrs {
            let v = match v {
                J::Str(s) => s.clone(),
                other => format!("{other:?}"),
            };
            out.push_str(&format!(" {k}=\"{}\"", v.replace('&', "&amp;").replace('"', "&quot;").replace('<', "&lt;")));
        }
    }
    out.push('>');
    for c in j.get("children").map(J::arr).unwrap_or(&[]) {
        node_xml(c, out);
    }
    out.push_str(&format!("</{name}>"));
}

/// The result text of a case, or `None` for a function this harness does not know.
pub fn run(case: &DiffCase) -> Option<String> {
    let args = json::parse(&case.args).ok()?;
    let a = args.arr();
    let arg = |i: usize| a.get(i);
    let j = match case.func.as_str() {
        "niceStep" => Json::Num(nice_step(num(arg(0)?), num(arg(1)?))),
        "autoScale" => {
            let f = arg(3)?;
            let fixed =
                Fixed { min: opt(f, "min"), max: opt(f, "max"), major: opt(f, "major"), minor: opt(f, "minor"), log: opt(f, "log") };
            let pad = arg(4).map_or(0.05, num);
            scale_json(&auto_scale(num(arg(0)?), num(arg(1)?), num(arg(2)?), &fixed, pad))
        }
        "tickValues" => {
            let s = scale_of(arg(0)?);
            Json::Arr(tick_values(&s, arg(1).map(num)).into_iter().map(Json::Num).collect())
        }
        "frac" => Json::Num(frac(&scale_of(arg(0)?), num(arg(1)?))),
        "trendNum" => Json::Str(trend_num(num(arg(0)?))),
        "fitTrend" => {
            let kind = arg(0)?.str()?;
            let xs: Vec<f64> = arg(1)?.arr().iter().map(num).collect();
            let ys: Vec<f64> = arg(2)?.arr().iter().map(num).collect();
            let o = arg(3)?;
            let opts = TrendOpts { order: opt(o, "order"), period: opt(o, "period"), intercept: opt(o, "intercept") };
            match fit_trend(kind, &xs, &ys, &opts) {
                Err(_) => throw("Invalid array length"),
                Ok(None) => Json::Null,
                Ok(Some(fit)) => {
                    let mut o: Vec<(String, Json)> = Vec::new();
                    if let Some(p) = &fit.points {
                        let pts = p.iter().map(|(x, y)| Json::Obj(vec![("x".to_owned(), Json::Num(*x)), ("y".to_owned(), Json::Num(*y))]));
                        o.push(("points".to_owned(), Json::Arr(pts.collect())));
                    }
                    if let Some(r) = fit.r2 {
                        o.push(("r2".to_owned(), Json::Num(r)));
                    }
                    if let Some(eq) = &fit.eq {
                        let parts = eq.iter().map(|e| {
                            let mut k = vec![("text".to_owned(), Json::s(&e.text))];
                            if e.sup {
                                k.push(("sup".to_owned(), Json::Bool(true)));
                            }
                            Json::Obj(k)
                        });
                        o.push(("eq".to_owned(), Json::Arr(parts.collect())));
                    }
                    let mut probe = xs.clone();
                    probe.extend([min_n(&xs) - 1.0, max_n(&xs) + 1.0, 0.0, 1.0, -1.0]);
                    probe.truncate(70);
                    let at = probe.iter().map(|x| {
                        let y = fit.eval(*x).map_or_else(|| Json::s("undefined"), Json::Num);
                        Json::Arr(vec![Json::Num(*x), y])
                    });
                    o.push(("at".to_owned(), Json::Arr(at.collect())));
                    Json::Obj(o)
                }
            }
        }
        "histogramBins" => {
            let vals: Vec<f64> = arg(0)?.arr().iter().map(num).collect();
            let mut xml = String::new();
            node_xml(arg(1)?, &mut xml);
            let doc = parse(xml, &ParseOptions::LENIENT).ok()?;
            let fmt = arg(2).and_then(J::str);
            match histogram_bins(&vals, Some(doc.root()), fmt) {
                Err(_) => throw("Invalid array length"),
                Ok(h) => Json::Obj(vec![
                    ("labels".to_owned(), Json::Arr(h.labels.iter().map(|l| Json::s(l)).collect())),
                    ("counts".to_owned(), Json::Arr(h.counts.iter().map(|c| Json::Num(*c)).collect())),
                    ("edges".to_owned(), Json::Arr(h.edges.iter().map(|e| Json::Num(*e)).collect())),
                ]),
            }
        }
        _ => return None,
    };
    Some(j.to_text())
}
