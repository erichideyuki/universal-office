//! Debug aid of the C-4/C-5 port (kept: prints the Rust side of one facet so it can be diffed against the capture sidecars).
//! `UO_DBG_GROUP=c1a UO_DBG_KEY=gen/c1a/0007 UO_DBG_VIEW=a UO_DBG_WHAT=layout:trendCurve|ops:full cargo test --test c45_dbg -- --nocapture`
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

mod support;

use support::data::{load_data, load_measures};
use support::stages;

#[test]
fn dump() {
    let (Ok(group), Ok(key), Ok(view), Ok(what)) =
        (std::env::var("UO_DBG_GROUP"), std::env::var("UO_DBG_KEY"), std::env::var("UO_DBG_VIEW"), std::env::var("UO_DBG_WHAT"))
    else {
        return;
    };
    let data = load_data(&group);
    let table = load_measures(&group);
    let chart = data.iter().find(|c| c.key == key).expect("chart");
    let v = stages::view(&view);
    let (kind, arg) = what.split_once(':').unwrap_or((&what, ""));
    match kind {
        "layout" => {
            let m = if arg.is_empty() { None } else { Some(arg) };
            for l in stages::layout(chart, &table, &v, m).unwrap_or_default() {
                println!("{l}");
            }
        }
        "ops" => {
            let out = stages::ops(chart, &table, &v, arg).expect("ops");
            if let Some(e) = &out.error {
                println!("#error {e}");
            }
            for l in &out.lines {
                println!("{l}");
            }
        }
        _ => panic!("what"),
    }
}
