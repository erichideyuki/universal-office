//! The Rust side of the package parity (tests/opc_parity.rs): the per-app policies and the
//! canonical streams, item for item the same as `capture_opc.mjs`.
//!
//! The policies are what the format crates will own (uo-docx, uo-xlsx, uo-pptx): each app's open
//! options, main part, XML presets per part, relationship scope and relationship filter, as in
//! the TS of 0.9.x, and one target rule and lookup for the three apps, the D6 path (gate 3, R1,
//! R2, R3):
//!
//! | | Write (`w`) | Sheet (`s`) | Slides (`p`) |
//! |---|---|---|---|
//! | open | exact names | `CaseFold::Ascii` fallback (`XlsxPackage.caseless`; R2) | `skip_impossible_ratio` |
//! | main part | `word/document.xml` | `xl/workbook.xml` (caseless) | `ppt/presentation.xml` |
//! | XML preset | `docx-pos` | `sheet`, + `sheet-chart` for `xl/charts/`, `xl/drawings/` | `slides` |
//! | rels read | `word/_rels/*.rels` only | every `.rels` | every `.rels` |
//! | relationship kept | `Id` and `Target` non-empty, not external | same | `Id` and `Target` present, not external |
//! | target rule and lookup | `Package::resolve_target` (R1) | `Package::resolve_target` (R2, R3) | `Package::resolve_target` (R3) |
//!
//! The 0.9.x target rules and lookups (Write: `./` stripped, `/x` → `x`, else `word/${target}`,
//! exact lookup; Sheet and Slides: `resolveRelTarget`, with a Unicode-lowercase (Sheet) or exact
//! (Slides) lookup) are gone from the policies: [`legacy`] reproduces them only so that the
//! replay can show they explain the whole difference of a `rels` facet, accepted by name.
#![allow(dead_code, clippy::unwrap_used, clippy::panic)]

pub mod quirks;

use uo_opc::cfb::{CfbEntryKind, CfbFile};
use uo_opc::error::{Error, XmlErrorKind};
use uo_opc::limits::is_xml_like;
use uo_opc::package::{CaseFold, OpenOptions, Package, SaveOptions};
use uo_opc::xml::{Element, Node, ParseOptions, parse};
use uo_opc::{Limits, part_name};
use uo_testkit::Stream;
use uo_testkit::zipdir::central_directory;

use crate::xml_support::preset;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum App {
    Write,
    Sheet,
    Slides,
}

impl App {
    pub const ALL: [App; 3] = [App::Write, App::Sheet, App::Slides];

    pub fn id(self) -> &'static str {
        match self {
            App::Write => "w",
            App::Sheet => "s",
            App::Slides => "p",
        }
    }

    pub fn from_id(id: &str) -> Option<App> {
        App::ALL.into_iter().find(|a| a.id() == id)
    }

    /// The app that opens a file of this extension.
    pub fn for_extension(ext: &str) -> Option<App> {
        match ext {
            "docx" | "docm" | "dotx" | "dotm" => Some(App::Write),
            "xlsx" | "xlsm" | "xltx" => Some(App::Sheet),
            "pptx" | "pptm" | "ppsx" | "potx" => Some(App::Slides),
            _ => None,
        }
    }

    pub fn open_options(self) -> OpenOptions {
        match self {
            App::Write => OpenOptions::default(),
            App::Sheet => OpenOptions { case_fold: CaseFold::Ascii, ..OpenOptions::default() },
            App::Slides => OpenOptions { skip_impossible_ratio: true, ..OpenOptions::default() },
        }
    }

    pub fn main_part(self) -> &'static str {
        match self {
            App::Write => "word/document.xml",
            App::Sheet => "xl/workbook.xml",
            App::Slides => "ppt/presentation.xml",
        }
    }

    pub fn presets(self, part: &str) -> &'static [&'static str] {
        match self {
            App::Write => &["docx-pos"],
            App::Sheet if part.starts_with("xl/charts/") || part.starts_with("xl/drawings/") => &["sheet", "sheet-chart"],
            App::Sheet => &["sheet"],
            App::Slides => &["slides"],
        }
    }

    pub fn rels_preset(self) -> &'static str {
        match self {
            App::Write => "docx",
            App::Sheet => "sheet",
            App::Slides => "slides",
        }
    }

    pub fn rels_in_scope(self, part: &str) -> bool {
        match self {
            App::Write => part.strip_prefix("word/_rels/").is_some_and(|f| !f.contains('/') && f.ends_with(".rels")),
            _ => true,
        }
    }

    pub fn selected(self, id: Option<&str>, target: Option<&str>, external: bool) -> bool {
        match self {
            App::Slides => id.is_some() && target.is_some() && !external,
            _ => id.is_some_and(|s| !s.is_empty()) && target.is_some_and(|s| !s.is_empty()) && !external,
        }
    }

    /// The decided rule (gate 3, R1/R3) for a relationship target of `owner`: the D6 path, the
    /// same for the three apps.
    pub fn resolve(self, pkg: &Package, owner: &str, target: &str) -> RuleOutcome {
        RuleOutcome {
            name: part_name::resolve_relationship_target(owner, target),
            found: pkg.resolve_target(owner, target).map(str::to_owned),
        }
    }
}

/// What a target rule and lookup give for one relationship: the part name the rule resolves
/// (`None`: the target names no part, such as a malformed `%` escape) and the part the lookup
/// opens.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RuleOutcome {
    pub name: Option<String>,
    pub found: Option<String>,
}

impl RuleOutcome {
    /// The stream items: `R` + the name (`U` when none), then `F` + whether the part found is
    /// the name itself, or `N` (missing). The capture writes `R` and `F`/`N` the same way.
    fn write(&self, st: &mut Stream) {
        match &self.name {
            Some(n) => st.tag(b'R').str(n),
            None => st.tag(b'U'),
        };
        match &self.found {
            Some(f) => st.tag(b'F').bool(Some(f) == self.name.as_ref()),
            None => st.tag(b'N'),
        };
    }
}

/// The 0.9.x target rules and lookups, kept only to explain the captured TS (the R1, R2 and R3
/// acceptances of the replay, and `examples/opc_rule_evidence.rs`); no policy uses them.
pub mod legacy {
    use super::{App, RuleOutcome};
    use uo_opc::Package;

    /// sheet-core and slides-core `resolveRelTarget` (package.ts), the same code in both.
    pub fn resolve_rel_target(from_part: &str, target: &str) -> String {
        if let Some(abs) = target.strip_prefix('/') {
            return abs.to_owned();
        }
        let mut dir: &str = from_part.rfind('/').map_or("", |i| &from_part[..i]);
        let mut t = target;
        while let Some(rest) = t.strip_prefix("../") {
            t = rest;
            dir = dir.rfind('/').map_or("", |i| &dir[..i]);
        }
        if dir.is_empty() { t.to_owned() } else { format!("{dir}/{t}") }
    }

    /// The app's own target rule of 0.9.x.
    pub fn resolve_name(app: App, owner: &str, target: &str) -> String {
        match app {
            App::Write => {
                let t = target.strip_prefix("./").unwrap_or(target);
                match t.strip_prefix('/') {
                    Some(abs) => abs.to_owned(),
                    None => format!("word/{t}"),
                }
            }
            _ => resolve_rel_target(owner, target),
        }
    }

    /// The app's own lookup of 0.9.x: exact (Write, Slides), or exact then the first part (in
    /// order) equal under JS `toLowerCase` (Sheet `XlsxPackage.caseless`).
    pub fn lookup(app: App, pkg: &Package, name: &str) -> Option<String> {
        if pkg.contains(name) {
            return Some(name.to_owned());
        }
        if app != App::Sheet {
            return None;
        }
        let folded = name.to_lowercase();
        pkg.part_names().find(|n| n.to_lowercase() == folded).map(str::to_owned)
    }

    /// The app's rule and lookup of 0.9.x on one relationship.
    pub fn resolve(app: App, pkg: &Package, owner: &str, target: &str) -> RuleOutcome {
        let name = resolve_name(app, owner, target);
        RuleOutcome { found: lookup(app, pkg, &name), name: Some(name) }
    }
}

/// The gate-3 decisions that change the `rels` facet.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Rule {
    /// Write's target rule → D6
    R1,
    /// Sheet's Unicode case folding → ASCII
    R2,
    /// Sheet and Slides `resolveRelTarget` (and the Slides exact lookup) → D6
    R3,
}

/// The decision that explains a relationship whose 0.9.x outcome differs from D6's, and how the
/// outcome changed.
pub fn rule_difference(app: App, legacy: &RuleOutcome, d6: &RuleOutcome) -> (Rule, &'static str) {
    let rule = match app {
        App::Write => Rule::R1,
        // the same name, another lookup: Sheet's Unicode folding against ASCII
        App::Sheet if legacy.name == d6.name => Rule::R2,
        _ => Rule::R3,
    };
    let kind = match (&legacy.found, &d6.found) {
        (None, Some(_)) => "missing → found",
        (Some(_), None) => "found → missing",
        (Some(a), Some(b)) if a != b => "another part",
        _ => "same outcome, another resolved name",
    };
    (rule, kind)
}

/// `dir/_rels/file.rels` → `dir/file`; `_rels/.rels` → `` (the part a relationship part
/// belongs to).
pub fn rels_owner(p: &str) -> String {
    let i = p.rfind("_rels/").unwrap_or(0);
    let file = p.get(i + 6..p.len().saturating_sub(5)).unwrap_or("");
    format!("{}{file}", &p[..i])
}

/// A `.rels` part name (`(^|/)_rels/[^/]*\.rels$`).
pub fn is_rels_part(p: &str) -> bool {
    let Some(i) = p.rfind("_rels/") else { return false };
    (i == 0 || p.as_bytes().get(i - 1) == Some(&b'/')) && !p[i + 6..].contains('/') && p.ends_with(".rels")
}

/// The fflate message the TS load reports for a Rust open error (`zip_fflate.rs` `same_class`).
pub fn error_class(e: &Error) -> String {
    match e {
        Error::NotZip | Error::Corrupt("entry count exceeds the archive") | Error::Corrupt("zip64 extra field missing") => {
            "err:zip:invalid zip data".into()
        }
        Error::UnsupportedCompression(m) => format!("err:zip:unknown compression type {m}"),
        Error::Corrupt(m) => format!("err:zip:{m}"),
        other => format!("err:rust:{other:?}"),
    }
}

/// Opens like the app's `load`: unzip with its options, inflate everything (the TS is eager),
/// then the main part check.
pub fn load(app: App, bytes: &[u8]) -> Result<Package, String> {
    let pkg =
        Package::open(bytes, &Limits::DEFAULT, &app.open_options()).and_then(|p| p.verify().map(|()| p)).map_err(|e| error_class(&e))?;
    let has_main = match app {
        App::Sheet => pkg.find_name(app.main_part()).is_some(),
        _ => pkg.contains(app.main_part()),
    };
    if has_main { Ok(pkg) } else { Err("err:main".into()) }
}

/// The entries fflate reads as zeros where uo-opc reports `Corrupt` (gate 2, D1): deflated,
/// with an empty compressed range (clamped to the archive, as `subarray` does) and a declared
/// size > 0.
pub fn d1_entries(bytes: &[u8]) -> Vec<(String, u64)> {
    let Ok(entries) = uo_opc::zip::read(bytes, u32::MAX) else { return Vec::new() };
    entries.into_iter().filter(|e| e.method == 8 && e.size > 0 && e.compressed(bytes).is_empty()).map(|e| (e.name, e.size)).collect()
}

/// The package as the TS holds it when only D1 makes `load` fail: opened without the eager
/// check, the D1 entries replaced by fflate's zeros, then `load`'s checks. `None` when D1 does
/// not explain the failure.
pub fn load_as_fflate_d1(app: App, bytes: &[u8]) -> Option<Package> {
    let d1 = d1_entries(bytes);
    if d1.is_empty() {
        return None;
    }
    let mut pkg = Package::open(bytes, &Limits::DEFAULT, &app.open_options()).ok()?;
    for (name, size) in d1 {
        if pkg.contains(&name) && size <= Limits::DEFAULT.part_cap(&name) {
            pkg.set_part(name, vec![0; usize::try_from(size).ok()?]);
        }
    }
    pkg.verify().ok()?;
    let has_main = match app {
        App::Sheet => pkg.find_name(app.main_part()).is_some(),
        _ => pkg.contains(app.main_part()),
    };
    has_main.then_some(pkg)
}

/// `in`: the digest of an input.
pub fn input_digest(bytes: &[u8]) -> String {
    let mut s = Stream::new();
    s.digest_of(bytes);
    s.finish().short()
}

fn stream(traced: bool) -> Stream {
    if traced { Stream::traced() } else { Stream::new() }
}

/// `load.*` of an opened package.
pub fn load_value(pkg: &Package, traced: bool) -> (String, Stream) {
    let mut st = stream(traced);
    for n in pkg.part_names() {
        st.str(n).digest_of(pkg.part(n).unwrap().unwrap());
    }
    (format!("ok:{}:{}", pkg.len(), st.digest().short()), st)
}

/// The canonical tree stream (iterative: trees can be deep).
pub fn tree(st: &mut Stream, root: Element<'_>, positions: bool) {
    let mut stack: Vec<Node<'_>> = vec![Node::Element(root)];
    while let Some(n) = stack.pop() {
        match n {
            Node::Element(e) => {
                st.tag(b'E').str(e.name());
                let attrs: Vec<_> = e.attrs().collect();
                st.u32(attrs.len() as u32);
                for a in attrs {
                    st.str(a.name).str(a.value);
                }
                if positions {
                    let p = e.positions().unwrap();
                    st.u64(p.start as u64).u64(p.content_start as u64).u64(p.content_end as u64).u64(p.end as u64);
                }
                match e.raw() {
                    Some(r) => st.tag(b'R').str(r),
                    None => st.tag(b'r'),
                };
                let kids: Vec<Node<'_>> = e.children().collect();
                st.u32(kids.len() as u32);
                stack.extend(kids.into_iter().rev());
            }
            other => {
                st.tag(b'T').str(other.as_text().unwrap_or(""));
            }
        }
    }
}

/// Counters of what a replay covered.
#[derive(Debug, Default, Clone)]
pub struct Tally {
    pub trees: u64,
    pub xml_errors: u64,
    /// G5: sheet-core hung, uo-opc reports an unclosed construct
    pub g5_hangs: u64,
    /// G5: sheet-core returned a tree where slides-core fails; uo-opc reports an unclosed construct
    pub g5_garbage: u64,
    pub rels_parts: u64,
    pub rels: u64,
}

/// The relationships of one `rels` facet whose D6 outcome differs from the 0.9.x rule's.
#[derive(Debug, Default, Clone)]
pub struct RuleDiffs {
    /// the facet value with the 0.9.x rules (what the capture holds when they explain it)
    pub legacy_value: String,
    /// (decision, outcome change, description) per relationship
    pub diffs: Vec<(Rule, &'static str, String)>,
}

/// The G5 cases the capture lists for an app (`g5.*`): `hang:<preset>:<part>` and
/// `garbage:<preset>:<part>:<tree digest>`.
#[derive(Debug, Default, Clone)]
pub struct G5 {
    pub hangs: Vec<(String, String)>,
    pub garbage: Vec<(String, String, String)>,
}

impl G5 {
    pub fn parse(json: Option<&str>) -> G5 {
        let mut g = G5::default();
        for s in json.map(json_strings).unwrap_or_default() {
            if let Some(rest) = s.strip_prefix("hang:") {
                let (p, n) = rest.split_once(':').unwrap();
                g.hangs.push((p.to_owned(), n.to_owned()));
            } else if let Some(rest) = s.strip_prefix("garbage:") {
                let (p, rest) = rest.split_once(':').unwrap();
                let (n, d) = rest.rsplit_once(':').unwrap();
                g.garbage.push((p.to_owned(), n.to_owned(), d.to_owned()));
            } else {
                panic!("unknown g5 entry {s}");
            }
        }
        g
    }

    fn lists_part(&self, part: &str) -> bool {
        self.hangs.iter().any(|(_, n)| n == part) || self.garbage.iter().any(|(_, n, _)| n == part)
    }
}

type Parsed = Result<uo_opc::xml::Document, Error>;

fn unclosed(r: &Parsed) -> bool {
    matches!(r, Err(Error::Xml { kind: XmlErrorKind::Unclosed, .. }))
}

fn tree_digest(doc: &uo_opc::xml::Document) -> String {
    let mut s = Stream::new();
    tree(&mut s, doc.root(), false);
    s.finish().short()
}

/// The G5 tag of a part the capture lists for this preset (`H` or `G`) when uo-opc's result is
/// the accepted one: an unclosed-construct error (on which slides-core also fails, for
/// `garbage`), or the very tree sheet-core returned. `None`: compare normally.
fn g5_tag(g5: &G5, preset_id: &str, part: &str, text: &str, result: &Parsed, tally: &mut Tally) -> Option<u8> {
    if g5.hangs.iter().any(|(p, n)| p == preset_id && n == part) && unclosed(result) {
        tally.g5_hangs += 1;
        return Some(b'H');
    }
    let (_, _, digest) = g5.garbage.iter().find(|(p, n, _)| p == preset_id && n == part)?;
    match result {
        Ok(doc) if tree_digest(doc) == *digest => Some(b'G'),
        Err(_) if unclosed(result) && parse(text, &preset("slides")).is_err() => {
            tally.g5_garbage += 1;
            Some(b'G')
        }
        _ => None,
    }
}

/// `xml.*`: every XML-like part with each preset of the app.
pub fn xml_value(app: App, pkg: &Package, g5: &G5, tally: &mut Tally, traced: bool) -> (String, Stream) {
    let mut st = stream(traced);
    let (mut parsed, mut errors) = (0u64, 0u64);
    for name in pkg.part_names() {
        if !is_xml_like(name) {
            continue;
        }
        let text = pkg.part_text(name).unwrap().unwrap();
        for &p in app.presets(name) {
            st.str(name).str(p);
            let opts: ParseOptions<'static> = preset(p);
            let result = parse(text.as_ref(), &opts);
            if let Some(tag) = g5_tag(g5, p, name, &text, &result, tally) {
                if tag == b'G' {
                    parsed += 1;
                } else {
                    errors += 1;
                }
                st.tag(tag);
                continue;
            }
            match result {
                Ok(doc) => {
                    parsed += 1;
                    tree(&mut st, doc.root(), opts.positions);
                }
                Err(_) => {
                    errors += 1;
                    st.tag(b'X');
                }
            }
        }
    }
    tally.trees += parsed;
    tally.xml_errors += errors;
    (format!("{parsed}:{errors}:{}", st.digest().short()), st)
}

/// `rels.*`: every relationship of the parts in the app's scope, resolved by the D6 path; and the
/// same facet with the 0.9.x rules, with every relationship where the two differ.
pub fn rels_value(app: App, pkg: &Package, g5: &G5, tally: &mut Tally, traced: bool) -> (String, Stream, RuleDiffs) {
    let mut st = stream(traced);
    let mut legacy_st = Stream::new();
    let mut diffs = Vec::new();
    let (mut parts, mut count) = (0u64, 0u64);
    let mut scratch = Tally::default();
    for p in pkg.part_names() {
        if !is_rels_part(p) || !app.rels_in_scope(p) {
            continue;
        }
        parts += 1;
        st.str(p);
        legacy_st.str(p);
        let text = pkg.part_text(p).unwrap().unwrap();
        let result = parse(text.as_ref(), &preset(app.rels_preset()));
        if app == App::Sheet && g5.lists_part(p) && g5_tag(g5, app.rels_preset(), p, &text, &result, &mut scratch).is_some() {
            st.tag(b'G');
            legacy_st.tag(b'G');
            continue;
        }
        let doc = match result {
            Ok(d) => d,
            Err(_) => {
                st.tag(b'X');
                legacy_st.tag(b'X');
                continue;
            }
        };
        let owner = rels_owner(p);
        for rel in doc.root().child_elements().filter(|e| e.name() == "Relationship") {
            count += 1;
            let (id, target) = (rel.attr("Id"), rel.attr("Target"));
            let external = rel.attr("TargetMode") == Some("External");
            for s in [&mut st, &mut legacy_st] {
                s.tag(b'L').bool(id.is_some()).str(id.unwrap_or("")).bool(target.is_some()).str(target.unwrap_or("")).bool(external);
            }
            if !app.selected(id, target, external) {
                st.tag(b'S');
                legacy_st.tag(b'S');
                continue;
            }
            let target = target.unwrap_or("");
            let d6 = app.resolve(pkg, &owner, target);
            d6.write(&mut st);
            let old = legacy::resolve(app, pkg, &owner, target);
            old.write(&mut legacy_st);
            if old != d6 {
                let (rule, kind) = rule_difference(app, &old, &d6);
                diffs.push((
                    rule,
                    kind,
                    format!("{p} Target={target:?}: 0.9 {:?} → {:?}, D6 {:?} → {:?}", old.name, old.found, d6.name, d6.found),
                ));
            }
        }
    }
    tally.rels_parts += parts;
    tally.rels += count;
    let legacy_value = format!("{parts}:{count}:{}", legacy_st.digest().short());
    (format!("{parts}:{count}:{}", st.digest().short()), st, RuleDiffs { legacy_value, diffs })
}

fn decode_name(raw: &[u8], flags: u16) -> String {
    if flags & 0x800 != 0 {
        let body = raw.strip_prefix(&[0xEF, 0xBB, 0xBF]).unwrap_or(raw);
        String::from_utf8_lossy(body).into_owned()
    } else {
        raw.iter().map(|&b| char::from(b)).collect()
    }
}

/// `save.*` of an archive the Rust side wrote: the independent central directory listing for
/// names, flags and methods, the Rust reader for the bytes. Also returns the methods.
pub fn save_value(saved: &[u8], traced: bool) -> (String, Stream) {
    let list = central_directory(saved).unwrap();
    let back = Package::open(saved, &Limits::DEFAULT, &OpenOptions::default()).unwrap();
    let mut full = stream(traced);
    let mut no_method = Stream::new();
    for c in &list {
        let name = decode_name(&c.name, c.flags);
        let data = back.part(&name).unwrap().unwrap_or_else(|| panic!("saved entry {name:?} not read back"));
        full.bytes(&c.name).u32(u32::from(c.flags & 0x800)).u32(u32::from(c.method)).digest_of(data);
        no_method.bytes(&c.name).u32(u32::from(c.flags & 0x800)).digest_of(data);
    }
    (format!("{}:{}:{}", list.len(), full.digest().short(), no_method.finish().short()), full)
}

/// The package saved as the Rust package saves it (untouched entries copied raw, G4).
pub fn save_raw(pkg: &Package) -> Vec<u8> {
    pkg.save(&SaveOptions::default()).unwrap()
}

/// The package with every part set again (all compressed, as `zipSync` does), then saved.
pub fn save_rewritten(pkg: &Package) -> Vec<u8> {
    let parts: Vec<(String, Vec<u8>)> = pkg.part_names().map(|n| (n.to_owned(), pkg.part(n).unwrap().unwrap().to_vec())).collect();
    Package::from_parts(parts, &Limits::DEFAULT, &OpenOptions::default()).save(&SaveOptions::default()).unwrap()
}

/// `cfb.*`: entries and streams as the TS readers list them.
pub fn cfb_value(bytes: &[u8], traced: bool) -> (String, Stream) {
    let mut st = stream(traced);
    let Ok(f) = CfbFile::open(bytes) else { return ("err".into(), st) };
    for e in f.entries() {
        let kind = match e.kind {
            CfbEntryKind::Root => "root",
            CfbEntryKind::Storage => "storage",
            CfbEntryKind::Stream => "stream",
        };
        // the JS number: rounded to f64, printed as an exact integer
        let size = ((e.size as f64) as u128).to_string();
        st.str(&e.name).str(&e.path).str(kind).str(&size).u32(e.start).bool(f.has(&e.path));
        match f.stream(&e.path) {
            Ok(Some(d)) => st.tag(b'D').digest_of(&d),
            Ok(None) => st.tag(b'-'),
            Err(_) => st.tag(b'E'),
        };
    }
    (format!("ok:{}:{}", f.entries().len(), st.digest().short()), st)
}

/// The strings of a JSON array of strings (the `g5.*` facet).
pub fn json_strings(s: &str) -> Vec<String> {
    let mut out = Vec::new();
    let b: Vec<char> = s.chars().collect();
    let mut i = 0;
    while i < b.len() {
        if b[i] == '"' {
            let start = i;
            i += 1;
            while i < b.len() && b[i] != '"' {
                if b[i] == '\\' {
                    i += 1;
                }
                i += 1;
            }
            out.push(crate::xml_support::unq(&b[start..=i.min(b.len() - 1)].iter().collect::<String>()));
        }
        i += 1;
    }
    out
}

/// JS `CanonicalNumericIndexString` for the names the quirk corpus uses: `-0`, `NaN`,
/// `Infinity`, `-Infinity` and decimal numbers that `ToString(ToNumber(s))` gives back unchanged
/// (up to 15 significant digits, no exponent).
fn js_canonical_numeric(s: &str) -> bool {
    if matches!(s, "-0" | "NaN" | "Infinity" | "-Infinity") {
        return true;
    }
    let body = s.strip_prefix('-').unwrap_or(s);
    let (int, frac) = body.split_once('.').map_or((body, None), |(i, f)| (i, Some(f)));
    let int_ok = int == "0" || (!int.is_empty() && !int.starts_with('0') && int.bytes().all(|b| b.is_ascii_digit()));
    let frac_ok = frac.is_none_or(|f| !f.is_empty() && f.bytes().all(|b| b.is_ascii_digit()) && !f.ends_with('0'));
    int_ok && frac_ok && int.len() + frac.map_or(0, str::len) <= 15 && s != "-0"
}

/// The names fflate's `unzipSync` silently drops after an entry named `__proto__`: that
/// assignment makes the entry's bytes (a Uint8Array) the prototype of the result object, and a
/// typed array's [[Set]] ignores a canonical numeric name that is not a valid index of it
/// (ECMA-262 TypedArray [[Set]], receiver ≠ target), unless the result already owns that name.
/// Reproduced here only to show that this is the whole difference (uo-opc keeps them: gate 2, D4).
pub fn js_proto_dropped(zip: &[u8]) -> Vec<String> {
    let Ok(list) = central_directory(zip) else { return Vec::new() };
    let mut own: std::collections::HashSet<String> = std::collections::HashSet::new();
    let mut proto_len: Option<u64> = None;
    let mut dropped = Vec::new();
    for c in &list {
        let name = decode_name(&c.name, c.flags);
        if name == "__proto__" {
            proto_len = Some(c.size);
            continue;
        }
        if let Some(len) = proto_len {
            let valid_index = !name.contains('.') && !name.starts_with('-') && name.parse::<u64>().is_ok_and(|v| v < len);
            if !own.contains(&name) && js_canonical_numeric(&name) && !valid_index {
                dropped.push(name);
                continue;
            }
        }
        own.insert(name);
    }
    dropped
}

/// Names and bytes of an archive as uo-opc reads it, in central directory order: what the .NET
/// read-back (`save_readback.cs`) can be compared on.
pub fn names_bytes_value(saved: &[u8]) -> String {
    let list = central_directory(saved).unwrap();
    let back = Package::open(saved, &Limits::DEFAULT, &OpenOptions::default()).unwrap();
    let mut st = Stream::new();
    for c in &list {
        let name = decode_name(&c.name, c.flags);
        st.str(&name).digest_of(back.part(&name).unwrap().unwrap());
    }
    format!("{}:{}", list.len(), st.finish().short())
}

/// The edit script of the capture (`editFacet`): set an existing part (keeps its position), remove
/// one (Sheet: by its caseless name) and set it again (goes last), add a new part and an
/// integer-like one (first in the zip). False when the package has fewer than 3 parts.
pub fn edit(app: App, pkg: &mut Package) -> bool {
    let names: Vec<String> = pkg.part_names().map(str::to_owned).collect();
    if names.len() < 3 {
        return false;
    }
    pkg.set_part(names[1].clone(), b"edited".to_vec());
    match app {
        App::Sheet => {
            if let Some(n) = pkg.find_name(&names[2]).map(str::to_owned) {
                pkg.remove_part(&n);
            }
        }
        _ => {
            pkg.remove_part(&names[2]);
        }
    }
    pkg.set_part("new/part.xml", b"<new/>".to_vec());
    pkg.set_part(names[2].clone(), b"readded".to_vec());
    pkg.set_part("7", b"seven".to_vec());
    true
}
