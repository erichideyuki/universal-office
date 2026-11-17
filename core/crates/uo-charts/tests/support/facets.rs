//! The replay driver: walks every record of a capture, and for each facet either checks it (the capture's own integrity, or a
//! stage of `stages.rs`) or skips it and counts it per stage. A facet of a stage that is not ported is NEVER a failure.

use std::collections::{BTreeMap, HashMap};

use uo_testkit::{Capture, Replay, Report};

use super::data::{ChartData, DiffCase, Measure, OpsStream, load_diff_cases};
use super::digest::{input_digest, lines_digest, ordered_digest, set_digest};
use super::stages;

/// What a replay did: how many facets were compared, and how many were skipped, by stage (or reason).
#[derive(Default, Debug)]
pub struct Tally {
    pub checked: usize,
    pub skipped: BTreeMap<String, usize>,
    /// compared facets per stage (`stage_of`) and per family (`(compared, skipped)`, the first token of the chart's `src`)
    pub stage: BTreeMap<&'static str, usize>,
    pub family: BTreeMap<String, (usize, usize)>,
}

impl Tally {
    fn skip(&mut self, why: &str) {
        *self.skipped.entry(why.to_owned()).or_default() += 1;
    }

    pub fn skipped_total(&self) -> usize {
        self.skipped.values().sum()
    }

    pub fn summary(&self) -> String {
        let s: Vec<String> = self.skipped.iter().map(|(k, n)| format!("{n} {k}")).collect();
        format!("{} compared, {} skipped ({})", self.checked, self.skipped_total(), s.join(", "))
    }

    /// The per-stage and per-family counts as text.
    pub fn detail(&self) -> String {
        let st: Vec<String> = self.stage.iter().map(|(k, n)| format!("{k}: {n}")).collect();
        let fam: Vec<String> = self.family.iter().map(|(k, (c, s))| format!("{k} {c}/{}", c + s)).collect();
        format!("compared by stage [{}]; compared/total by family [{}]", st.join(", "), fam.join(", "))
    }
}

/// Facets that only describe the input (generator metadata): neither compared nor counted as skipped work.
const META: [&str; 4] = ["seed", "feat", "src", "also"];

/// The chart-level facets C-1 owns and `tests/c1_replay.rs` replays.
const C1: [&str; 3] = ["parse", "model", "dropped"];

/// Replays one capture group (`charts-<group>.capture` with its sidecars) through the driver.
pub fn replay_group(
    cap: &Capture,
    data: &[ChartData],
    table: &HashMap<(String, String), Measure>,
    ops: &HashMap<(String, String, String), OpsStream>,
) -> (Report, Tally) {
    let by_key: HashMap<&str, &ChartData> = data.iter().map(|d| (d.key.as_str(), d)).collect();
    let mut rp = Replay::new(cap);
    let mut t = Tally::default();
    // `UO_CHARTS_ONLY=0007,0459` replays only the charts whose key ends with one of the suffixes (a debugging aid)
    let only: Vec<String> = std::env::var("UO_CHARTS_ONLY").map(|v| v.split(',').map(str::to_owned).collect()).unwrap_or_default();
    for rec in cap.records() {
        let key = rec.key.as_str();
        if !only.is_empty() && !only.iter().any(|o| key.ends_with(o.as_str())) {
            for (f, _) in &rec.facets {
                rp.skip(key, f);
            }
            continue;
        }
        let Some(chart) = by_key.get(key).copied() else {
            // a chart whose parse threw has no data line: its facets are input/profile/parse=err only
            for (f, _) in &rec.facets {
                rp.skip(key, f);
            }
            continue;
        };
        let family = rec.get("src").map_or("other", |v| v.split('-').next().unwrap_or(v)).to_owned();
        for (facet, _) in &rec.facets {
            let f = facet.as_str();
            let before = (t.checked, t.skipped_total());
            if META.contains(&f) {
                rp.skip(key, f);
            } else if C1.contains(&f) {
                rp.skip(key, f);
                t.skip("C-1 (tests/c1_replay.rs)");
            } else if f == "input" {
                t.checked += 1;
                rp.check(key, f, &input_digest(&chart.xml));
            } else if f == "profile" {
                t.checked += 1;
                rp.check(key, f, &profile_text(chart));
            } else if f == "host" {
                t.checked += 1;
                rp.check(key, f, &ordered_digest(&chart.host_log.iter().map(|e| e.line()).collect::<Vec<_>>()));
            } else if f == "hostx" {
                t.checked += 1;
                rp.check(key, f, &ordered_digest(&chart.host_paint.iter().map(|e| e.line()).collect::<Vec<_>>()));
            } else if let Some(rest) = f.strip_prefix("ops.") {
                let (v, s) = rest.split_once('.').unwrap_or((rest, "full"));
                // the stream file keeps the first charts only: its digest must be the facet (the capture's own integrity)
                if let Some(st) = ops.get(&(key.to_owned(), v.to_owned(), s.to_owned())) {
                    let expect = match &st.error {
                        Some(e) => format!("err:{}", e.replace('=', ":")),
                        None => lines_digest(&st.lines),
                    };
                    t.checked += 1;
                    rp.check(key, f, &expect);
                } else if let Some(out) = stages::ops(chart, table, &stages::view(v), s)
                    && !out.unsupported
                {
                    t.checked += 1;
                    let actual = match out.error {
                        Some(e) => format!("err:{}", e.replace('=', ":")),
                        None => lines_digest(&out.lines),
                    };
                    check_or_accept(&mut rp, key, f, &actual);
                } else {
                    rp.skip(key, f);
                    t.skip(stages::stage_of(f));
                }
            } else if let Some(rest) = f.strip_prefix("ask.") {
                let (v, s) = rest.split_once('.').unwrap_or((rest, "full"));
                // the questions asked do not depend on what the canvas could print: a tiled picture (the shell painter's) still
                // asks the same set
                match stages::ops(chart, table, &stages::view(v), s) {
                    Some(out) => {
                        t.checked += 1;
                        rp.check(key, f, &set_digest(out.asks));
                    }
                    None => {
                        rp.skip(key, f);
                        t.skip(stages::stage_of(f));
                    }
                }
            } else if let Some(rest) = f.strip_prefix("bounds.") {
                let (v, s) = rest.split_once('.').unwrap_or((rest, "full"));
                match stages::bounds(chart, table, &stages::view(v), s) {
                    Some(b) => {
                        t.checked += 1;
                        rp.check(key, f, &b);
                    }
                    None => {
                        rp.skip(key, f);
                        t.skip(stages::stage_of(f));
                    }
                }
            } else if let Some(rest) = f.strip_prefix("lay.") {
                let (v, m) = rest.split_once('.').map_or((rest, None), |(v, m)| (v, Some(m)));
                match stages::layout(chart, table, &stages::view(v), m) {
                    Some(lines) => {
                        t.checked += 1;
                        check_or_accept(&mut rp, key, f, &ordered_digest(&lines));
                    }
                    None => {
                        rp.skip(key, f);
                        t.skip(stages::stage_of(f));
                    }
                }
            } else {
                rp.skip(key, f);
                t.skip("unknown facet");
            }
            let (c, k) = (t.checked - before.0, t.skipped_total() - before.1);
            if c > 0 && (f.starts_with("ops.") || f.starts_with("ask.") || f.starts_with("bounds.") || f.starts_with("lay.")) {
                *t.stage.entry(stages::stage_of(f)).or_default() += c;
            }
            if f.starts_with("ops.") || f.starts_with("ask.") || f.starts_with("bounds.") || f.starts_with("lay.") {
                let e = t.family.entry(family.clone()).or_default();
                e.0 += c;
                e.1 += k;
            }
        }
    }
    (rp.finish(), t)
}

fn profile_text(c: &ChartData) -> String {
    use super::data::Host;
    use uo_charts::host::FileFormat;
    let ff = match c.file_format {
        Some(FileFormat::Mso2007) => "mso2007",
        Some(FileFormat::Modern) => "modern",
        None => "-",
    };
    format!("{}/{ff}/{}", if c.host == Host::Xl { "xl" } else { "ppt" }, c.theme)
}

/// The facets of the generated charts whose trendline curve (and what is drawn from it) differs from the capture by ONE ULP in
/// a `Math.pow` (`**`) of the fit or of the evaluation: V8's pow is not the glibc 2.39 pow `uo_jsnum::math::pow` pins (record
/// G1 of `uo-jsnum.md`). Evidence (C-4, 2026-11-17): chart 0409 calls `pow` 432 times with distinct arguments; a node run of
/// `Math.pow` over the same arguments differs from `uo_jsnum::math::pow` in 2 of them, by one ulp (2.1666666666666665 ** 1.0471526243442981
/// and 4.375 ** 0.8247114876778696); the polynomial and power fits of the other charts amplify such a difference to 1e-12 in the
/// sampled curve. Named, never a tolerance: each entry is a `(chart key, facet)` that differs for this reason and no other.
const POW_FACETS: [(&str, &str); 23] = [
    ("gen/c1a/0007", "lay.a"),
    ("gen/c1a/0007", "lay.a.trendCurve"),
    ("gen/c1a/0010", "lay.a"),
    ("gen/c1a/0010", "lay.a.trendCurve"),
    ("gen/c1a/0336", "lay.a"),
    ("gen/c1a/0336", "lay.a.trendCurve"),
    ("gen/c1a/0336", "lay.b"),
    ("gen/c1a/0336", "lay.b.trendCurve"),
    ("gen/c1a/0409", "lay.a"),
    ("gen/c1a/0409", "lay.a.trendCurve"),
    ("gen/c1a/0459", "ops.a.none"),
    ("gen/c1a/0459", "ops.a.s0"),
    ("gen/c1a/0459", "ops.a.p02"),
    ("gen/c1a/0459", "lay.a"),
    ("gen/c1a/0459", "lay.a.trendCurve"),
    ("gen/c1a/0459", "lay.b"),
    ("gen/c1a/0459", "lay.b.trendCurve"),
    ("gen/c1a/0471", "lay.a"),
    ("gen/c1a/0471", "lay.a.trendCurve"),
    ("gen/c1a/0471", "lay.b"),
    ("gen/c1a/0471", "lay.b.trendCurve"),
    ("gen/c1a/0503", "lay.a"),
    ("gen/c1a/0503", "lay.a.trendCurve"),
];

fn check_or_accept(rp: &mut Replay, key: &str, facet: &str, actual: &str) {
    if POW_FACETS.contains(&(key, facet)) {
        rp.accept(key, facet, actual, "Math.pow: V8's pow differs by one ulp from the glibc pow uo_jsnum pins (G1)");
    } else {
        rp.check(key, facet, actual);
    }
}

/// The differential cases whose result differs from the capture by ONE ULP in a `Math.pow` (`**`): the capture ran on Windows,
/// where V8's `std::pow` is the UCRT one, and `uo_jsnum::math::pow` pins glibc 2.39 non-FMA (the record of `uo-jsnum`, G1, measured
/// 146 results in 400k different between the two). `autoScale` 187 is `3 ** 34 = 16677181699666569`, a tie: UCRT rounds to ...570,
/// glibc to ...568; the two `fitTrend` ones read `x ** b` of the power fit.
const POW_PLATFORM: [(&str, &str); 3] = [("autoScale", "187"), ("fitTrend", "212"), ("fitTrend", "374")];
const POW_REASON: &str = "Math.pow: the capture's UCRT std::pow differs by one ulp from the glibc pow uo_jsnum pins";

/// Replays the differential cases: `diff/<fn>` facets (`<index>` = digest of the result text).
pub fn replay_diff(cap: &Capture) -> (Report, Tally) {
    let cases: Vec<DiffCase> = load_diff_cases();
    let mut rp = Replay::new(cap);
    let mut t = Tally::default();
    for c in &cases {
        let key = format!("diff/{}", c.func);
        match stages::diff(c) {
            Some(text) => {
                t.checked += 1;
                if text != c.result {
                    let at = text.bytes().zip(c.result.bytes()).position(|(a, b)| a != b).unwrap_or(text.len().min(c.result.len()));
                    let ctx = |t: &str| t.get(at.saturating_sub(60)..(at + 80).min(t.len())).unwrap_or("").to_owned();
                    eprintln!(
                        "{key} [{}] differs at byte {at}
  ts:   {}
  rust: {}",
                        c.index,
                        ctx(&c.result),
                        ctx(&text)
                    );
                }
                let digest = super::digest::text_digest(&text);
                if POW_PLATFORM.contains(&(c.func.as_str(), c.index.as_str())) {
                    rp.accept(&key, &c.index, &digest, POW_REASON);
                } else {
                    rp.check(&key, &c.index, &digest);
                }
            }
            None => {
                // the stage is not ported: the sidecar's stored result must at least be the facet (the capture's integrity)
                t.checked += 1;
                rp.check(&key, &c.index, &super::digest::text_digest(&c.result));
                t.skip("C-2 differential cases (stage not ported; sidecar digests checked)");
            }
        }
    }
    (rp.finish(), t)
}
