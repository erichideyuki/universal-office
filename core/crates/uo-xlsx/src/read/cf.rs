//! `read-cf.ts`: conditional formatting, the classic `<conditionalFormatting>` plus the Excel 2010+ definitions of
//! the worksheet `<extLst>`.

use super::model::{CfIcon, Cfvo, ColorContext, ColorStop, CondFormat, CondRule};
use super::styles::read_dxf;
use super::theme::resolve_color_el;
use super::{El, off, on, owned, pf, pi, trim};

/// `NUM_RE.test(s) ? parseFloat(s) : undefined`.
fn num(s: &str) -> Option<f64> {
    let t = trim(s).as_bytes();
    let mut i = 0;
    if matches!(t.first(), Some(b'+' | b'-')) {
        i += 1;
    }
    let digits = |from: usize| t.get(from..).map_or(0, |r| r.iter().take_while(|c| c.is_ascii_digit()).count());
    let d1 = digits(i);
    if d1 > 0 {
        i += d1;
        if t.get(i) == Some(&b'.') {
            i += 1;
            i += digits(i);
        }
    } else if t.get(i) == Some(&b'.') && digits(i + 1) > 0 {
        i += 1 + digits(i + 1);
    } else {
        return None;
    }
    if matches!(t.get(i), Some(b'e' | b'E')) {
        let mut j = i + 1;
        if matches!(t.get(j), Some(b'+' | b'-')) {
            j += 1;
        }
        let d = digits(j);
        if d > 0 {
            i = j + d;
        }
    }
    (i == t.len()).then(|| pf(trim(s)))
}

fn formulas(el: El<'_>, x14: bool) -> Vec<String> {
    el.children_named(if x14 { "xm:f" } else { "formula" }).map(|f| trim(&f.text_content()).to_owned()).collect()
}

fn read_cfvo(el: El<'_>) -> Cfvo {
    let val = match el.first_child("xm:f") {
        Some(f) => Some(trim(&f.text_content()).to_owned()),
        None => owned(el.attr("val")),
    };
    let mut c = Cfvo { r#type: el.attr("type").unwrap_or("min").to_owned(), ..Cfvo::default() };
    if let Some(v) = val {
        c.value = num(&v);
        c.val = Some(v);
    }
    if off(el.attr("gte")) {
        c.gte = Some(false);
    }
    c
}

fn color_or(e: Option<El<'_>>, cx: &ColorContext) -> Option<String> {
    e.and_then(|e| resolve_color_el(e, cx))
}

fn read_rule(rule_el: El<'_>, cx: &ColorContext, x14: bool) -> Option<CondRule> {
    let ty = rule_el.attr("type").filter(|t| !t.is_empty())?;
    let a = |k: &str| rule_el.attr(k);
    let p = if x14 { "x14:" } else { "" };
    let prio = pi(a("priority").unwrap_or("0"));
    let mut rule = CondRule {
        r#type: ty.to_owned(),
        priority: if prio.is_nan() { 0.0 } else { prio },
        stop_if_true: on(a("stopIfTrue")),
        dxf_id: a("dxfId").map(pi),
        operator: owned(a("operator")),
        text: owned(a("text")),
        time_period: owned(a("timePeriod")),
        rank: a("rank").map(pi),
        percent: on(a("percent")),
        bottom: on(a("bottom")),
        above_average: off(a("aboveAverage")).then_some(false),
        equal_average: on(a("equalAverage")),
        std_dev: a("stdDev").map(pi),
        ..CondRule::default()
    };
    let fs = formulas(rule_el, x14);
    if !fs.is_empty() {
        rule.formula = Some(fs);
    }
    if x14 {
        rule.x14 = true;
        rule.dxf = rule_el.first_child("x14:dxf").map(|d| read_dxf(d, cx));
    }
    match ty {
        "colorScale" => {
            let mut stops = Vec::new();
            if let Some(cs) = rule_el.first_child(&format!("{p}colorScale")) {
                let cfvos: Vec<Cfvo> = cs.children_named(&format!("{p}cfvo")).map(read_cfvo).collect();
                let colors: Vec<String> = cs
                    .children_named(&format!("{p}color"))
                    .map(|c| resolve_color_el(c, cx).unwrap_or_else(|| "#000000".to_owned()))
                    .collect();
                for (i, c) in cfvos.into_iter().enumerate() {
                    stops.push(ColorStop {
                        r#type: c.r#type,
                        value: c.value,
                        val: c.val,
                        color: colors.get(i).cloned().unwrap_or_else(|| "#000000".to_owned()),
                    });
                }
            }
            rule.stops = Some(stops);
        }
        "dataBar" => {
            if let Some(db) = rule_el.first_child(&format!("{p}dataBar")) {
                rule.cfvos = Some(db.children_named(&format!("{p}cfvo")).map(read_cfvo).collect());
                let col = db.first_child(&format!("{p}color")).or_else(|| db.first_child(&format!("{p}fillColor")));
                rule.bar_color = Some(color_or(col, cx).unwrap_or_else(|| "#638EC6".to_owned()));
                if off(db.attr("showValue")) {
                    rule.show_value = Some(false);
                }
                rule.bar_min_len = db.attr("minLength").map(pi);
                rule.bar_max_len = db.attr("maxLength").map(pi);
                if x14 {
                    apply_x14_bar(&mut rule, db, cx);
                }
            } else {
                rule.bar_color = Some("#638EC6".to_owned());
            }
        }
        "iconSet" => {
            let is = rule_el.first_child(&format!("{p}iconSet"));
            rule.icon_set = Some(is.and_then(|i| i.attr("iconSet")).unwrap_or("3TrafficLights1").to_owned());
            if let Some(is) = is {
                rule.cfvos = Some(is.children_named(&format!("{p}cfvo")).map(read_cfvo).collect());
                rule.icon_reverse = on(is.attr("reverse"));
                if off(is.attr("showValue")) {
                    rule.show_value = Some(false);
                }
                let icons = icons_of(is);
                if !icons.is_empty() && on(is.attr("custom")) {
                    rule.icons = Some(icons);
                }
            }
        }
        _ => {}
    }
    Some(rule)
}

fn icons_of(is: El<'_>) -> Vec<CfIcon> {
    is.children_named("x14:cfIcon")
        .map(|c| CfIcon { set: c.attr("iconSet").unwrap_or("NoIcons").to_owned(), id: pi(c.attr("iconId").unwrap_or("0")) })
        .collect()
}

fn apply_x14_bar(rule: &mut CondRule, db: El<'_>, cx: &ColorContext) {
    rule.x14 = true;
    let a = |k: &str| db.attr(k);
    rule.bar_min_len = Some(a("minLength").map_or(10.0, pi));
    rule.bar_max_len = Some(a("maxLength").map_or(90.0, pi));
    rule.bar_border = Some(on(a("border")));
    rule.bar_gradient = Some(!matches!(a("gradient"), Some("0" | "false")));
    if let Some(v) = a("axisPosition").filter(|v| !v.is_empty()) {
        rule.bar_axis = Some(v.to_owned());
    }
    if let Some(v) = a("direction").filter(|v| !v.is_empty()) {
        rule.bar_direction = Some(v.to_owned());
    }
    if on(a("negativeBarColorSameAsPositive")) {
        rule.bar_neg_same = true;
    }
    if !matches!(a("negativeBarBorderColorSameAsPositive"), Some("0" | "false")) {
        rule.bar_neg_border_same = true;
    }
    let col = |n: &str| db.first_child(&format!("x14:{n}")).and_then(|e| resolve_color_el(e, cx));
    if let Some(c) = col("fillColor") {
        rule.bar_color = Some(c);
    }
    if let Some(c) = col("borderColor") {
        rule.bar_border_color = Some(c);
    }
    if let Some(c) = col("negativeFillColor") {
        rule.bar_neg_color = Some(c);
    }
    if let Some(c) = col("negativeBorderColor") {
        rule.bar_neg_border_color = Some(c);
    }
    if let Some(c) = col("axisColor") {
        rule.bar_axis_color = Some(c);
    }
    let cfvos: Vec<Cfvo> = db.children_named("x14:cfvo").map(read_cfvo).collect();
    if !cfvos.is_empty() {
        rule.cfvos = Some(cfvos);
    }
}

fn merge_x14(base: &mut CondRule, rule_el: El<'_>, cx: &ColorContext) {
    if base.r#type == "dataBar"
        && let Some(db) = rule_el.first_child("x14:dataBar")
    {
        apply_x14_bar(base, db, cx);
        return;
    }
    if base.r#type == "iconSet"
        && let Some(is) = rule_el.first_child("x14:iconSet")
    {
        if let Some(s) = is.attr("iconSet") {
            base.icon_set = Some(s.to_owned());
        }
        let cf: Vec<Cfvo> = is.children_named("x14:cfvo").map(read_cfvo).collect();
        if !cf.is_empty() {
            base.cfvos = Some(cf);
        }
        let icons = icons_of(is);
        if !icons.is_empty() {
            base.icons = Some(icons);
        }
    }
}

/// `readConditionalFormats`.
pub fn read_conditional_formats(root: El<'_>, cx: &ColorContext) -> Vec<CondFormat> {
    let mut out: Vec<CondFormat> = Vec::new();
    // x14 id -> (format, rule); a later definition of the same id replaces an earlier one
    let mut by_x14: Vec<(String, usize, usize)> = Vec::new();
    for cf_el in root.children_named("conditionalFormatting") {
        let sqref = trim(cf_el.attr("sqref").unwrap_or("")).to_owned();
        let mut rules = Vec::new();
        let mut ids: Vec<(String, usize)> = Vec::new();
        for rule_el in cf_el.children_named("cfRule") {
            let Some(mut rule) = read_rule(rule_el, cx, false) else { continue };
            if let Some(ext_lst) = rule_el.first_child("extLst") {
                for ext in ext_lst.children_named("ext") {
                    if let Some(id_el) = ext.first_child("x14:id") {
                        let id = trim(&id_el.text_content()).to_owned();
                        rule.x14_id = Some(id.clone());
                        ids.push((id, rules.len()));
                        break;
                    }
                }
            }
            rules.push(rule);
        }
        if !sqref.is_empty() && !rules.is_empty() {
            for (id, ri) in ids {
                by_x14.retain(|(k, _, _)| *k != id);
                by_x14.push((id, out.len(), ri));
            }
            out.push(CondFormat { sqref, rules });
        }
    }
    if let Some(ws_ext) = root.first_child("extLst") {
        for ext in ws_ext.children_named("ext") {
            let Some(cfs) = ext.first_child("x14:conditionalFormattings") else { continue };
            for cf in cfs.children_named("x14:conditionalFormatting") {
                let sqref = cf.first_child("xm:sqref").map(|s| trim(&s.text_content()).to_owned()).unwrap_or_default();
                let mut rules = Vec::new();
                for rule_el in cf.children_named("x14:cfRule") {
                    let id = rule_el.attr("id").map(|s| trim(s).to_owned()).filter(|s| !s.is_empty());
                    let base = id.as_ref().and_then(|id| by_x14.iter().find(|(k, _, _)| k == id)).map(|&(_, fi, ri)| (fi, ri));
                    if let Some((fi, ri)) = base {
                        if let Some(r) = out.get_mut(fi).and_then(|f| f.rules.get_mut(ri)) {
                            merge_x14(r, rule_el, cx);
                        }
                        continue;
                    }
                    if let Some(r) = read_rule(rule_el, cx, true) {
                        rules.push(r);
                    }
                }
                if !sqref.is_empty() && !rules.is_empty() {
                    out.push(CondFormat { sqref, rules });
                }
            }
        }
    }
    out
}
