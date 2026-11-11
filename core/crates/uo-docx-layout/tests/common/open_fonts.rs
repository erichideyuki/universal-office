//! The repository's open fonts behind a fixed family map, one answer identity per request key
//! (moved from `g4_cancel.rs` for the LA7 tests and `examples/bench_relayout.rs`; the other test
//! copies are a declared follow-up).

use std::collections::HashMap;
use std::path::Path;
use std::sync::{Arc, Mutex};

use uo_docx_layout::fonts::{FontAnswer, FontProvider};
use uo_fonts::FontLimits;
use uo_fonts::registry::{AnswerParts, FaceCache, FontBudget};

/// The open faces of `shared/fonts/files` and `site/assets/fonts`.
pub struct OpenFonts {
    cache: Arc<FaceCache>,
    files: HashMap<String, Arc<[u8]>>,
    answers: Mutex<HashMap<(String, bool, bool), Arc<FontAnswer>>>,
}

impl OpenFonts {
    /// Over the repository at `repo`.
    #[must_use]
    pub fn at(repo: &Path) -> OpenFonts {
        let mut files = HashMap::new();
        for dir in ["shared/fonts/files", "site/assets/fonts"] {
            let Ok(rd) = std::fs::read_dir(repo.join(dir)) else { continue };
            for e in rd.flatten() {
                let p = e.path();
                if p.extension().is_some_and(|x| x == "ttf")
                    && let (Some(stem), Ok(bytes)) = (p.file_stem(), std::fs::read(&p))
                {
                    files.insert(stem.to_string_lossy().into_owned(), Arc::from(bytes));
                }
            }
        }
        OpenFonts {
            cache: Arc::new(FaceCache::new(FontBudget::of(uo_fonts::registry::FontProfile::Desktop), FontLimits::default())),
            files,
            answers: Mutex::new(HashMap::new()),
        }
    }
}

impl FontProvider for OpenFonts {
    fn face(&self, family: &str, bold: bool, italic: bool) -> Option<Arc<FontAnswer>> {
        let fam = family.to_lowercase();
        let k = (fam.clone(), bold, italic);
        if let Some(a) = self.answers.lock().ok()?.get(&k) {
            return Some(Arc::clone(a));
        }
        let base = match fam.as_str() {
            "carlito" | "calibri" => "carlito",
            "caladea" | "cambria" => "caladea",
            "liberation serif" | "times new roman" => "liberationserif",
            "liberation mono" | "courier new" => "liberationmono",
            _ => "liberationsans",
        };
        let v = match (bold, italic) {
            (false, false) => "regular",
            (true, false) => "bold",
            (false, true) => "italic",
            (true, true) => "bolditalic",
        };
        let bytes = self.files.get(&format!("{base}-{v}"))?.clone();
        let a = FontAnswer::new(AnswerParts {
            key: format!("{family}|{}{}", u8::from(bold), u8::from(italic)),
            family: family.to_owned(),
            face: self.cache.get_or_parse(bytes).ok()?,
            bold,
            italic,
            synth_bold: false,
            synth_italic: false,
            stand_in: false,
            advances: None,
            ticket: None,
        });
        self.answers.lock().ok()?.insert(k, Arc::clone(&a));
        Some(a)
    }
}
