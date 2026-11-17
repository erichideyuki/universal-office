//! The capture files of one group (`tests/captures/charts-<group>.*`): the facets (`.capture`), the inputs and host log
//! (`.data.jsonl.gz`), the measurement log (`.measures.tsv.gz`), the op streams of the first charts (`.ops.gz`) and the
//! differential cases (`charts-diff.cases.jsonl.gz`). Formats: the header of `js/capture-charts.mjs`.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use uo_charts::host::FileFormat;
use uo_testkit::Capture;

use super::json::{self, J};

pub fn captures_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/captures")
}

pub fn gunzip(path: &Path) -> String {
    let b = std::fs::read(path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    assert!(b.len() > 18 && b[0] == 0x1f && b[1] == 0x8b && b[3] == 0, "plain gzip from zlib.gzipSync");
    let raw = uo_opc::flate::inflate_raw(&b[10..b.len() - 8], None, 1 << 31).unwrap_or_else(|e| panic!("{}: {e:?}", path.display()));
    String::from_utf8(raw).unwrap_or_else(|e| panic!("{}: {e}", path.display()))
}

pub fn read_capture(stem: &str) -> Capture {
    Capture::read(captures_dir().join(format!("{stem}.capture"))).unwrap_or_else(|e| panic!("{stem}: {e}"))
}

/// `Slides` bridge (`ppt`: `chart-engine.ts`) or `Sheet` bridge (`xl`: `chart.ts`).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Host {
    Ppt,
    Xl,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Kind {
    Color,
    Blip,
    Fill,
}

/// One host call of the TS parse (or of the paint, `themeFill`): `key` is the canonical JSON of the element (`Color`, `Blip`)
/// or `<idx>|<phClr>` (`Fill`); `ans` the canonical JSON of the answer (`null` = undefined).
#[derive(Clone, Debug)]
pub struct HostEntry {
    pub kind: Kind,
    pub key: String,
    pub ans: String,
}

impl HostEntry {
    /// The line the capture digests: `kind<TAB>key<TAB>answer` (kind `color` / `blip` / `fill`).
    pub fn line(&self) -> String {
        let k = match self.kind {
            Kind::Color => "color",
            Kind::Blip => "blip",
            Kind::Fill => "fill",
        };
        format!("{k}\t{}\t{}", self.key, self.ans)
    }
}

/// A picture of a chart as `RenderOptions.image` gets it (decoded size, natural size in pt from the file's resolution) and
/// the recorder's source hash (`img(<sha1[0..12] of the bytes>)`).
#[derive(Clone, Debug)]
pub struct ImageData {
    pub rid: String,
    pub part: String,
    pub w: f64,
    pub h: f64,
    pub w_pt: Option<f64>,
    pub h_pt: Option<f64>,
    pub sha: String,
}

#[derive(Clone, Debug)]
pub struct ChartData {
    pub key: String,
    pub host: Host,
    pub file_format: Option<FileFormat>,
    pub theme: String,
    pub no_theme: bool,
    pub xml: String,
    pub user_shapes: Option<String>,
    pub images: Vec<ImageData>,
    pub host_log: Vec<HostEntry>,
    pub host_paint: Vec<HostEntry>,
    /// the theme's fonts (`+mj-lt`, `+mn-lt`) and `a:lnStyleLst` widths the host bridge saw
    pub major_font: Option<String>,
    pub minor_font: Option<String>,
    pub theme_ln: Vec<f64>,
}

fn entries(j: Option<&J>) -> Vec<HostEntry> {
    j.map(J::arr)
        .unwrap_or(&[])
        .iter()
        .map(|e| HostEntry {
            kind: match e.get("k").and_then(J::str) {
                Some("color") => Kind::Color,
                Some("blip") => Kind::Blip,
                _ => Kind::Fill,
            },
            key: e.get("key").and_then(J::str).unwrap_or("").to_owned(),
            ans: e.get("ans").and_then(J::str).unwrap_or("null").to_owned(),
        })
        .collect()
}

/// Loads `charts-<group>.data.jsonl.gz`.
pub fn load_data(group: &str) -> Vec<ChartData> {
    let text = gunzip(&captures_dir().join(format!("charts-{group}.data.jsonl.gz")));
    text.lines()
        .filter(|l| !l.is_empty())
        .map(|l| {
            let j = json::parse(l).unwrap_or_else(|e| panic!("data line: {e}"));
            let s = |k: &str| j.get(k).and_then(J::str).map(str::to_owned);
            ChartData {
                key: s("key").unwrap_or_default(),
                host: if s("host").as_deref() == Some("xl") { Host::Xl } else { Host::Ppt },
                file_format: match s("fileFormat").as_deref() {
                    Some("mso2007") => Some(FileFormat::Mso2007),
                    Some("modern") => Some(FileFormat::Modern),
                    _ => None,
                },
                theme: s("theme").unwrap_or_default(),
                no_theme: j.get("noTheme") == Some(&J::Bool(true)),
                xml: s("xml").unwrap_or_default(),
                user_shapes: s("userShapes"),
                images: j
                    .get("images")
                    .map(J::arr)
                    .unwrap_or(&[])
                    .iter()
                    .map(|i| ImageData {
                        rid: i.get("rid").and_then(J::str).unwrap_or("").to_owned(),
                        part: i.get("part").and_then(J::str).unwrap_or("").to_owned(),
                        w: i.get("w").and_then(J::num).unwrap_or(0.0),
                        h: i.get("h").and_then(J::num).unwrap_or(0.0),
                        w_pt: i.get("wPt").and_then(J::num),
                        h_pt: i.get("hPt").and_then(J::num),
                        sha: i.get("sha").and_then(J::str).unwrap_or("").to_owned(),
                    })
                    .collect(),
                host_log: entries(j.get("hostLog")),
                host_paint: entries(j.get("hostPaint")),
                major_font: j.get("fonts").and_then(|f| f.get("major")).and_then(J::str).map(str::to_owned),
                minor_font: j.get("fonts").and_then(|f| f.get("minor")).and_then(J::str).map(str::to_owned),
                theme_ln: j.get("themeLn").map(J::arr).unwrap_or(&[]).iter().map(|v| v.num().unwrap_or(f64::NAN)).collect(),
            }
        })
        .collect()
}

/// The answer of the deterministic measurer for `(font string, text)`: width and the font box.
#[derive(Clone, Copy, Debug)]
pub struct Measure {
    pub width: f64,
    pub ascent: f64,
    pub descent: f64,
}

/// `charts-<group>.measures.tsv.gz`: `"font"<TAB>"text"<TAB>width<TAB>ascent<TAB>descent`, font and text JSON-quoted. The
/// map is keyed by the two JSON strings' contents (font string, text).
pub fn load_measures(group: &str) -> HashMap<(String, String), Measure> {
    let text = gunzip(&captures_dir().join(format!("charts-{group}.measures.tsv.gz")));
    let mut m = HashMap::new();
    for line in text.lines().filter(|l| !l.is_empty()) {
        let f: Vec<&str> = line.split('\t').collect();
        if f.len() != 5 {
            continue;
        }
        let (Ok(J::Str(font)), Ok(J::Str(t))) = (json::parse(f[0]), json::parse(f[1])) else { continue };
        let n = |s: &str| uo_jsnum::parse::to_number(s);
        m.insert((font, t), Measure { width: n(f[2]), ascent: n(f[3]), descent: n(f[4]) });
    }
    m
}

/// One recorded stream of `charts-<group>.ops.gz` (only the first charts keep them).
#[derive(Clone, Debug)]
pub struct OpsStream {
    pub lines: Vec<String>,
    pub header_digest: String,
    pub error: Option<String>,
}

/// `(chart key, view, sel)` -> stream.
pub fn load_ops(group: &str) -> HashMap<(String, String, String), OpsStream> {
    let text = gunzip(&captures_dir().join(format!("charts-{group}.ops.gz")));
    let mut out = HashMap::new();
    let (mut key, mut cur): (String, Option<(String, String)>) = (String::new(), None);
    let mut s = OpsStream { lines: Vec::new(), header_digest: String::new(), error: None };
    let mut flush = |key: &str, cur: &mut Option<(String, String)>, s: &mut OpsStream| {
        if let Some((v, sel)) = cur.take() {
            out.insert(
                (key.to_owned(), v, sel),
                std::mem::replace(s, OpsStream { lines: Vec::new(), header_digest: String::new(), error: None }),
            );
        }
    };
    for line in text.lines() {
        if let Some(k) = line.strip_prefix("#chart ") {
            flush(&key, &mut cur, &mut s);
            key = k.to_owned();
        } else if let Some(h) = line.strip_prefix("#view ") {
            flush(&key, &mut cur, &mut s);
            let kv: HashMap<&str, &str> = h.split(' ').filter_map(|p| p.split_once('=')).collect();
            let v = h.split(' ').next().unwrap_or("").to_owned();
            cur = Some((v, kv.get("sel").copied().unwrap_or("").to_owned()));
            s.header_digest = format!("{}:{}", kv.get("lines").copied().unwrap_or("0"), kv.get("sha").copied().unwrap_or(""));
        } else if let Some(e) = line.strip_prefix("#error ") {
            s.error = Some(e.to_owned());
        } else if !line.is_empty() {
            s.lines.push(line.to_owned());
        }
    }
    flush(&key, &mut cur, &mut s);
    out
}

/// A differential case: `fn`, index, the argument array and the result, both as canonical JSON text.
#[derive(Clone, Debug)]
pub struct DiffCase {
    pub func: String,
    pub index: String,
    pub args: String,
    pub result: String,
}

pub fn load_diff_cases() -> Vec<DiffCase> {
    gunzip(&captures_dir().join("charts-diff.cases.jsonl.gz"))
        .lines()
        .filter(|l| !l.is_empty())
        .filter_map(|l| {
            let f: Vec<&str> = l.splitn(4, '\t').collect();
            (f.len() == 4).then(|| DiffCase { func: f[0].into(), index: f[1].into(), args: f[2].into(), result: f[3].into() })
        })
        .collect()
}
