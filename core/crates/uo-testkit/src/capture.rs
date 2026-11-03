//! Capture files and their replay.
//!
//! A capture is what the frozen TS engine produced for a corpus, written by the Node side
//! (`js/testkit.mjs`, `CaptureWriter`) and committed next to the tests that replay it. Format
//! (UTF-8 text, `\n` or `\r\n` line ends):
//!
//! ```text
//! # comment
//! @name value                      metadata (TS commit, corpus sizes, …)
//! key<TAB>facet=value<TAB>facet=value…
//! ```
//!
//! A key names one input (a document, a generated case); each facet is one value compared
//! exactly (typically `count:digest` of a canonical stream). Keys and facets contain no tab,
//! newline or `=` in facet names; values contain no tab or newline.
//!
//! [`Replay`] checks the values the Rust side computes against the capture: equal, mismatch,
//! an accepted difference (listed by name, with the reason: never a tolerance), or not
//! replayed. [`Report::assert_clean`] fails on any mismatch or on capture records that were
//! never replayed.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;
use std::path::Path;

/// One input of the capture: its key and its facets in file order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Record {
    pub key: String,
    pub facets: Vec<(String, String)>,
}

impl Record {
    pub fn get(&self, facet: &str) -> Option<&str> {
        self.facets.iter().find(|(f, _)| f == facet).map(|(_, v)| v.as_str())
    }
}

/// A parsed capture file.
#[derive(Debug, Clone, Default)]
pub struct Capture {
    meta: Vec<(String, String)>,
    records: Vec<Record>,
    index: BTreeMap<String, usize>,
}

impl Capture {
    /// Parses the text of a capture file.
    pub fn parse(text: &str) -> Result<Capture, String> {
        let mut c = Capture::default();
        for (n, line) in text.lines().enumerate() {
            let line = line.strip_suffix('\r').unwrap_or(line);
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            if let Some(rest) = line.strip_prefix('@') {
                let (k, v) = rest.split_once(' ').unwrap_or((rest, ""));
                c.meta.push((k.to_owned(), v.to_owned()));
                continue;
            }
            let mut fields = line.split('\t');
            let key = fields.next().unwrap_or("").to_owned();
            let mut facets = Vec::new();
            for f in fields {
                let Some((name, value)) = f.split_once('=') else { return Err(format!("line {}: facet without '=': {f}", n + 1)) };
                facets.push((name.to_owned(), value.to_owned()));
            }
            if c.index.insert(key.clone(), c.records.len()).is_some() {
                return Err(format!("line {}: duplicate key {key}", n + 1));
            }
            c.records.push(Record { key, facets });
        }
        Ok(c)
    }

    /// Reads and parses a capture file.
    pub fn read(path: impl AsRef<Path>) -> Result<Capture, String> {
        let path = path.as_ref();
        let text = std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
        Capture::parse(&text).map_err(|e| format!("{}: {e}", path.display()))
    }

    /// The records whose key satisfies `keep` (same metadata), so a corpus can be replayed by
    /// several tests in parallel, each one checked for completeness on its own share.
    pub fn subset(&self, mut keep: impl FnMut(&str) -> bool) -> Capture {
        let mut c = Capture { meta: self.meta.clone(), ..Capture::default() };
        for r in self.records.iter().filter(|r| keep(&r.key)) {
            c.index.insert(r.key.clone(), c.records.len());
            c.records.push(r.clone());
        }
        c
    }

    pub fn meta(&self, name: &str) -> Option<&str> {
        self.meta.iter().find(|(k, _)| k == name).map(|(_, v)| v.as_str())
    }

    /// The `@platform` line the Node writer adds (`platform()` in `js/testkit.mjs`).
    pub fn platform(&self) -> Result<Platform, String> {
        Platform::parse(self.meta("platform").ok_or("the capture has no @platform line (it predates the platform metadata)")?)
    }

    /// Checks the platform the capture was taken on against what the replaying test needs; the
    /// error names every field that differs. Every replay of a capture whose path calls a
    /// math function of `uo-jsnum` calls it (math bits depend on the V8, the architecture and,
    /// for `pow`, the C runtime).
    pub fn require_platform(&self, spec: &PlatformSpec) -> Result<(), String> {
        spec.check(&self.platform()?)
    }

    pub fn records(&self) -> &[Record] {
        &self.records
    }

    pub fn record(&self, key: &str) -> Option<&Record> {
        self.index.get(key).and_then(|&i| self.records.get(i))
    }

    pub fn get(&self, key: &str, facet: &str) -> Option<&str> {
        self.record(key).and_then(|r| r.get(facet))
    }

    pub fn len(&self) -> usize {
        self.records.len()
    }

    pub fn is_empty(&self) -> bool {
        self.records.is_empty()
    }
}

/// A difference found by the replay.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Mismatch {
    pub key: String,
    pub facet: String,
    /// the captured (TS) value; `None` when the capture has no such facet
    pub expected: Option<String>,
    pub actual: String,
}

/// A difference accepted by a recorded decision.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Accepted {
    pub key: String,
    pub facet: String,
    pub expected: String,
    pub actual: String,
    pub reason: String,
}

/// The replay of a capture: see the module comment.
#[derive(Debug)]
pub struct Replay<'c> {
    capture: &'c Capture,
    seen: BTreeSet<(String, String)>,
    equal: u64,
    mismatches: Vec<Mismatch>,
    accepted: Vec<Accepted>,
}

impl<'c> Replay<'c> {
    pub fn new(capture: &'c Capture) -> Self {
        Replay { capture, seen: BTreeSet::new(), equal: 0, mismatches: Vec::new(), accepted: Vec::new() }
    }

    pub fn capture(&self) -> &'c Capture {
        self.capture
    }

    fn mark(&mut self, key: &str, facet: &str) {
        self.seen.insert((key.to_owned(), facet.to_owned()));
    }

    /// Compares `actual` with the captured value; returns whether they are equal.
    pub fn check(&mut self, key: &str, facet: &str, actual: &str) -> bool {
        self.mark(key, facet);
        let expected = self.capture.get(key, facet);
        if expected == Some(actual) {
            self.equal += 1;
            return true;
        }
        self.mismatches.push(Mismatch {
            key: key.to_owned(),
            facet: facet.to_owned(),
            expected: expected.map(str::to_owned),
            actual: actual.to_owned(),
        });
        false
    }

    /// Like [`Replay::check`], but a difference is recorded as accepted with `reason` (a
    /// decision of the maintainer, named in the module record) instead of a mismatch. Use only
    /// when the caller has identified the difference as that decision.
    pub fn accept(&mut self, key: &str, facet: &str, actual: &str, reason: &str) -> bool {
        self.mark(key, facet);
        let expected = self.capture.get(key, facet);
        if expected == Some(actual) {
            self.equal += 1;
            return true;
        }
        self.accepted.push(Accepted {
            key: key.to_owned(),
            facet: facet.to_owned(),
            expected: expected.unwrap_or("").to_owned(),
            actual: actual.to_owned(),
            reason: reason.to_owned(),
        });
        false
    }

    /// Marks a captured facet as replayed without comparing it (its comparison is done by
    /// another facet, e.g. a detail line kept only for diagnostics).
    pub fn skip(&mut self, key: &str, facet: &str) {
        self.mark(key, facet);
    }

    /// The result. Captured facets never checked are reported as `missing`.
    pub fn finish(self) -> Report {
        let mut missing = Vec::new();
        for r in self.capture.records() {
            for (f, _) in &r.facets {
                if !self.seen.contains(&(r.key.clone(), f.clone())) {
                    missing.push((r.key.clone(), f.clone()));
                }
            }
        }
        Report { equal: self.equal, mismatches: self.mismatches, accepted: self.accepted, missing }
    }
}

/// The result of a replay.
#[derive(Debug, Clone, Default)]
pub struct Report {
    pub equal: u64,
    pub mismatches: Vec<Mismatch>,
    pub accepted: Vec<Accepted>,
    /// captured `(key, facet)` pairs the replay never checked
    pub missing: Vec<(String, String)>,
}

impl Report {
    /// One line: counts of equal, mismatched, accepted and missing facets, and the accepted
    /// differences grouped by reason.
    pub fn summary(&self) -> String {
        let mut s = format!(
            "{} equal, {} mismatches, {} accepted, {} not replayed",
            self.equal,
            self.mismatches.len(),
            self.accepted.len(),
            self.missing.len()
        );
        let mut by_reason: BTreeMap<&str, usize> = BTreeMap::new();
        for a in &self.accepted {
            *by_reason.entry(a.reason.as_str()).or_default() += 1;
        }
        for (r, n) in by_reason {
            let _ = write!(s, "; accepted {n}× {r}");
        }
        s
    }

    /// The details of up to `max` mismatches.
    pub fn describe(&self, max: usize) -> String {
        let mut s = String::new();
        for m in self.mismatches.iter().take(max) {
            let _ =
                writeln!(s, "{} [{}]\n  ts:   {}\n  rust: {}", m.key, m.facet, m.expected.as_deref().unwrap_or("(not captured)"), m.actual);
        }
        for (k, f) in self.missing.iter().take(max) {
            let _ = writeln!(s, "{k} [{f}] captured but not replayed");
        }
        s
    }

    pub fn is_clean(&self) -> bool {
        self.mismatches.is_empty() && self.missing.is_empty()
    }

    /// Fails with the details when there is any mismatch or unreplayed capture record.
    #[allow(clippy::panic)] // the purpose of this function: fail the test that calls it
    pub fn assert_clean(&self) {
        if !self.is_clean() {
            panic!("parity broken: {}\n{}", self.summary(), self.describe(30));
        }
    }
}

/// The platform a capture was taken on: the `key=value` pairs of the `@platform` line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Platform {
    /// `process.version`, e.g. `v24.15.0`
    pub node: String,
    /// `process.versions.v8`, e.g. `13.6.233.17-node.48`
    pub v8: String,
    /// `process.platform`: `linux`, `win32`, `darwin`
    pub os: String,
    /// `process.arch`: `x64`, `arm64`
    pub arch: String,
    /// the CPU model, spaces written as `_`
    pub cpu: String,
    /// Linux only: the `fma`, `avx2`, `avx512f` flags present, joined by `+`; `n/a` elsewhere
    pub features: String,
    /// the glibc version on Linux, `none` elsewhere
    pub glibc: String,
    /// `GLIBC_TUNABLES` or `none`
    pub tunables: String,
    /// Node's `execArgv` joined by `,` or `none`
    pub flags: String,
}

impl Platform {
    /// Parses the value of an `@platform` line; every field must be present.
    pub fn parse(value: &str) -> Result<Platform, String> {
        let field = |name: &str| -> Result<String, String> {
            value
                .split(' ')
                .find_map(|kv| kv.strip_prefix(name).and_then(|rest| rest.strip_prefix('=')))
                .map(str::to_owned)
                .ok_or_else(|| format!("@platform has no {name}=: {value}"))
        };
        Ok(Platform {
            node: field("node")?,
            v8: field("v8")?,
            os: field("os")?,
            arch: field("arch")?,
            cpu: field("cpu")?,
            features: field("features")?,
            glibc: field("glibc")?,
            tunables: field("tunables")?,
            flags: field("flags")?,
        })
    }
}

/// What a replay requires of the capture platform; `None` fields are not checked.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PlatformSpec {
    /// the V8 version must start with this (`13.6.233.17`)
    pub v8_prefix: Option<String>,
    pub arch: Option<String>,
    pub os: Option<String>,
    /// the glibc version must start with this (`2.39`)
    pub glibc_prefix: Option<String>,
    /// `GLIBC_TUNABLES` must contain this (`glibc.cpu.hwcaps=-FMA,-AVX2`)
    pub tunables_contain: Option<String>,
    /// the Node flags must be exactly this (`none` for none)
    pub flags: Option<String>,
}

impl PlatformSpec {
    /// Checks `p` against the spec; the error lists every mismatch.
    pub fn check(&self, p: &Platform) -> Result<(), String> {
        let mut bad = Vec::new();
        if let Some(v) = &self.v8_prefix
            && !p.v8.starts_with(v.as_str())
        {
            bad.push(format!("v8 {} (need {v}*)", p.v8));
        }
        if let Some(v) = &self.arch
            && &p.arch != v
        {
            bad.push(format!("arch {} (need {v})", p.arch));
        }
        if let Some(v) = &self.os
            && &p.os != v
        {
            bad.push(format!("os {} (need {v})", p.os));
        }
        if let Some(v) = &self.glibc_prefix
            && !p.glibc.starts_with(v.as_str())
        {
            bad.push(format!("glibc {} (need {v}*)", p.glibc));
        }
        if let Some(v) = &self.tunables_contain
            && !p.tunables.contains(v.as_str())
        {
            bad.push(format!("GLIBC_TUNABLES {} (need {v})", p.tunables));
        }
        if let Some(v) = &self.flags
            && &p.flags != v
        {
            bad.push(format!("node flags {} (need {v})", p.flags));
        }
        if bad.is_empty() { Ok(()) } else { Err(format!("capture platform differs: {}", bad.join("; "))) }
    }
}
