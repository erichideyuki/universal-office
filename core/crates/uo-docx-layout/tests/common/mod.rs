//! Shared parity tooling of the layout tests (record "Milestone 1 — F0"): the canonical-stream
//! sink over uo-testkit, the P2 transcript reader and its face provider (the Rust replay answers
//! every `provider.face` from the transcript, with the TS answer identities), and the font index
//! by SHA-256 (a face whose digest is not found fails: LQ21).
#![allow(dead_code)]

pub mod decode;
pub mod docgen;
pub mod hyph;
pub mod open_fonts;

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use std::sync::Mutex;
use uo_docx_layout::canon::CanonSink;

use uo_docx_layout::fonts::{FontAnswer, FontProvider};
use uo_fonts::registry::{AnswerParts, FaceCache};
use uo_testkit::Stream;

/// A [`CanonSink`] over a uo-testkit [`Stream`].
pub struct StreamSink(pub Stream);

impl CanonSink for StreamSink {
    fn tag(&mut self, c: u8) {
        self.0.tag(c);
    }
    fn u32(&mut self, v: u32) {
        self.0.u32(v);
    }
    fn f64(&mut self, v: f64) {
        self.0.f64(v);
    }
    fn bool(&mut self, b: bool) {
        self.0.bool(b);
    }
    fn bytes(&mut self, b: &[u8]) {
        self.0.bytes(b);
    }
    fn digest_of(&mut self, b: &[u8]) {
        self.0.digest_of(b);
    }
}

impl StreamSink {
    /// `count:digest`, the form of the capture files.
    pub fn value(self) -> String {
        let n = self.0.items();
        format!("{n}:{}", self.0.finish().short())
    }
}

pub fn manifest() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

pub fn repo() -> PathBuf {
    uo_testkit::repo_root(env!("CARGO_MANIFEST_DIR"))
}

/// `decodeURIComponent` of the transcript strings.
pub fn uri_decode(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        match (b[i], b.get(i + 1..i + 3)) {
            (b'%', Some(h)) => {
                let hex = std::str::from_utf8(h).ok().and_then(|h| u8::from_str_radix(h, 16).ok());
                if let Some(v) = hex {
                    out.push(v);
                    i += 3;
                    continue;
                }
                out.push(b'%');
            }
            (c, _) => out.push(c),
        }
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// One answer of the transcript.
#[derive(Clone, Debug)]
pub struct TAnswer {
    pub label: u32,
    pub key: String,
    pub family: String,
    pub bold: bool,
    pub italic: bool,
    pub synth_bold: bool,
    pub synth_italic: bool,
    pub stand_in: bool,
    /// lowercase hex SHA-256 of `bytes`
    pub sha256: Option<String>,
    pub len: Option<usize>,
    pub path: Option<String>,
    pub embedded: bool,
}

/// One `provider.face` call.
#[derive(Clone, Debug)]
pub struct TCall {
    pub family: String,
    pub bold: bool,
    pub italic: bool,
    /// 0 = null
    pub answer: u32,
}

/// A P2 transcript (`<key>.transcript.tsv`).
#[derive(Clone, Debug, Default)]
pub struct Transcript {
    pub answers: Vec<TAnswer>,
    pub calls: Vec<TCall>,
}

impl Transcript {
    pub fn parse(text: &str) -> Result<Transcript, String> {
        let mut t = Transcript::default();
        for (n, line) in text.lines().enumerate() {
            let f: Vec<&str> = line.split('\t').collect();
            let flag = |i: usize| f.get(i) == Some(&"1");
            let opt = |i: usize| f.get(i).filter(|v| **v != "-").map(|v| uri_decode(v));
            match f.first() {
                Some(&"A") if f.len() == 13 => t.answers.push(TAnswer {
                    label: f[1].parse().map_err(|e| format!("line {}: {e}", n + 1))?,
                    key: uri_decode(f[2]),
                    family: uri_decode(f[3]),
                    bold: flag(4),
                    italic: flag(5),
                    synth_bold: flag(6),
                    synth_italic: flag(7),
                    stand_in: flag(8),
                    sha256: opt(9),
                    len: opt(10).and_then(|v| v.parse().ok()),
                    embedded: f[11] == "embed",
                    path: opt(11).filter(|p| p != "embed"),
                }),
                Some(&"C") if f.len() == 5 => t.calls.push(TCall {
                    family: uri_decode(f[1]),
                    bold: flag(2),
                    italic: flag(3),
                    answer: f[4].parse().map_err(|e| format!("line {}: {e}", n + 1))?,
                }),
                Some(&"") | None => {}
                _ => return Err(format!("line {}: bad transcript line", n + 1)),
            }
        }
        Ok(t)
    }

    pub fn read(path: &Path) -> Result<Transcript, String> {
        let text = std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
        Transcript::parse(&text)
    }
}

/// Font files by SHA-256: the repository's open fonts (CI), plus any file the transcript names
/// (oracle mode, local; LQ21: a digest that is not found fails the gate instead of skipping).
#[derive(Default)]
pub struct FontIndex {
    by_sha: HashMap<String, Arc<[u8]>>,
}

impl FontIndex {
    pub fn open_fonts() -> FontIndex {
        let mut ix = FontIndex::default();
        for dir in ["shared/fonts/files", "site/assets/fonts"] {
            let Ok(rd) = std::fs::read_dir(repo().join(dir)) else { continue };
            for e in rd.flatten() {
                let p = e.path();
                if p.extension().is_some_and(|x| x.eq_ignore_ascii_case("ttf") || x.eq_ignore_ascii_case("otf")) {
                    ix.add_file(&p);
                }
            }
        }
        ix
    }

    pub fn add_file(&mut self, p: &Path) {
        if let Ok(b) = std::fs::read(p) {
            self.add_bytes(b.into());
        }
    }

    pub fn add_bytes(&mut self, b: Arc<[u8]>) {
        let h = uo_testkit::stream::hex(&uo_testkit::sha256::sha256(&b));
        self.by_sha.insert(h, b);
    }

    /// The faces a transcript names by path (oracle mode: installed fonts, read and never
    /// copied), a collection's face extracted as node-fonts does (`ttcFace`).
    pub fn add_transcript_sources(&mut self, t: &Transcript) {
        for a in &t.answers {
            let (Some(path), Some(sha)) = (&a.path, &a.sha256) else { continue };
            if self.by_sha.contains_key(sha) {
                continue;
            }
            let Ok(b) = std::fs::read(path) else { continue };
            let index = a.key.rsplit('#').next().and_then(|i| i.parse::<u32>().ok()).unwrap_or(0);
            // node-fonts' `ttcFace`, ported by the registry (frontier P)
            let face = if b.starts_with(b"ttcf") {
                uo_fonts::registry::ttc_face_by_index(&b, index, &uo_fonts::FontLimits::default()).ok()
            } else {
                Some(b)
            };
            if let Some(f) = face {
                self.add_bytes(f.into());
            }
        }
    }

    pub fn get(&self, sha256: &str) -> Option<Arc<[u8]>> {
        self.by_sha.get(sha256).cloned()
    }
}

/// An answer with its bytes and parsed face.
pub type LoadedAnswer = (TAnswer, Option<Arc<[u8]>>, Option<uo_fonts::Face>);

/// The face answers of a transcript, loaded from the font index: `FaceRes` material for the
/// adapter check, `FontAnswer`s for the replay provider.
pub struct LoadedAnswers {
    pub answers: Vec<LoadedAnswer>,
    /// answers whose bytes digest was not found (fails the gate, LQ21)
    pub missing: Vec<String>,
}

pub fn load_answers(t: &Transcript, fonts: &FontIndex) -> LoadedAnswers {
    let mut out = LoadedAnswers { answers: Vec::new(), missing: Vec::new() };
    for a in &t.answers {
        let bytes = a.sha256.as_deref().and_then(|h| fonts.get(h));
        if a.sha256.is_some() && bytes.is_none() {
            out.missing.push(format!("{} ({})", a.key, a.sha256.as_deref().unwrap_or("")));
        }
        let face = bytes.as_ref().and_then(|b| uo_fonts::Face::parse(b.clone(), &uo_fonts::FontLimits::default()).ok());
        out.answers.push((a.clone(), bytes, face));
    }
    out
}

/// The transcript replay policy: answers `face(family, bold, italic)` with the TS answer the
/// transcript recorded for that request, **with its identity** (one `FontAnswer` per TS answer
/// object, K2a); a request the TS never made is recorded in `unexpected` and answered `None`.
pub struct TranscriptProvider {
    by_request: HashMap<(String, bool, bool), u32>,
    answers: Vec<Option<Arc<FontAnswer>>>,
    pub unexpected: Mutex<Vec<(String, bool, bool)>>,
}

impl TranscriptProvider {
    pub fn new(t: &Transcript, loaded: LoadedAnswers) -> TranscriptProvider {
        let mut by_request = HashMap::new();
        for c in &t.calls {
            by_request.entry((c.family.clone(), c.bold, c.italic)).or_insert(c.answer);
        }
        let cache = FaceCache::shared();
        let answers = loaded
            .answers
            .into_iter()
            .map(|(a, bytes, _)| {
                let face = cache.get_or_parse(bytes?).ok()?;
                Some(FontAnswer::new(AnswerParts {
                    key: a.key,
                    family: a.family,
                    face,
                    bold: a.bold,
                    italic: a.italic,
                    synth_bold: a.synth_bold,
                    synth_italic: a.synth_italic,
                    stand_in: a.stand_in,
                    advances: None,
                    ticket: None,
                }))
            })
            .collect();
        TranscriptProvider { by_request, answers, unexpected: Mutex::new(Vec::new()) }
    }
}

impl FontProvider for TranscriptProvider {
    fn face(&self, family: &str, bold: bool, italic: bool) -> Option<Arc<FontAnswer>> {
        let Some(&label) = self.by_request.get(&(family.to_owned(), bold, italic)) else {
            if let Ok(mut u) = self.unexpected.lock() {
                u.push((family.to_owned(), bold, italic));
            }
            return None;
        };
        self.answers.get((label as usize).checked_sub(1)?)?.clone()
    }
}
