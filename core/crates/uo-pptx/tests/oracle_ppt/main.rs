//! The PowerPoint-PDF oracle (M4 gate prep, record `core/docs/modules/uo-pdf.md` "PowerPoint agreement"): writes the Rust
//! Slides PDF of every deck of `decks.tsv` for the comparison against PowerPoint's own PDF (`slides/tools/ppt-lab/pdf-oracle.mjs`).
//! Does nothing unless `UO_PPT_ORACLE_OUT=<dir>` is set (a plain `cargo test` stays fast); `.ppt` decks need `--features ppt`.
//!
//! Two font policies, one folder each (`<dir>/rust-<policy>/<id>.pdf` + `report.tsv`), chosen with `UO_PPT_ORACLE_FONTS`
//! (default `system,open`):
//! - `system`: the machine's installed fonts READ IN PLACE (`%WINDIR%\Fonts`, the user's fonts, the Office `CloudFonts` cache —
//!   the files PowerPoint draws with), the family by name ID 1 and the variant by score; a family the machine lacks goes to the
//!   open set below (its metric substitute, else Liberation Sans). Nothing is copied anywhere: only the PDFs this test writes
//!   embed subsets, under `core/target` (outside git), as any PDF made on this machine would;
//! - `open`: the repository's open fonts only (Liberation, Carlito, Caladea: the shipped default of the server PDF).
#![cfg(feature = "pdf")]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::indexing_slicing)]

use std::collections::HashMap;
use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Instant;

use uo_fonts::{Face, FontLimits};
use uo_pptx::pdf::{FaceSet, FontSource, PdfOptions, fallback_chain};
use uo_pptx::{Deck, Limits};

#[path = "../support/open_fonts.rs"]
mod open_fonts;

use open_fonts::{open_fonts, root};

/// One installed face: the file, the face in it, the variant its tables declare.
struct Entry {
    path: PathBuf,
    index: u32,
    bold: bool,
    italic: bool,
    weight: u16,
}

/// Faces loaded so far, by file and face index (`None`: the file did not parse).
type Loaded = Mutex<HashMap<(PathBuf, u32), Option<Arc<Face>>>>;

/// The installed fonts by lower-case family (name ID 1), loaded on demand; the open set answers what the machine lacks.
struct SystemFonts {
    families: HashMap<String, Vec<Entry>>,
    loaded: Loaded,
    open: FaceSet,
}

fn lower(s: &str) -> String {
    s.chars().flat_map(char::to_lowercase).collect()
}

impl SystemFonts {
    fn scan() -> SystemFonts {
        let env = |k: &str| std::env::var_os(k).map(PathBuf::from);
        let local = env("LOCALAPPDATA").unwrap_or_default();
        let roots = [
            env("WINDIR").unwrap_or_else(|| PathBuf::from("C:/Windows")).join("Fonts"),
            local.join("Microsoft/Windows/Fonts"),
            local.join("Microsoft/FontCache/4/CloudFonts"),
        ];
        let mut families: HashMap<String, Vec<Entry>> = HashMap::new();
        let mut stack: Vec<PathBuf> = roots.into_iter().filter(|p| p.exists()).collect();
        while let Some(dir) = stack.pop() {
            let Ok(rd) = std::fs::read_dir(&dir) else { continue };
            for e in rd.filter_map(Result::ok) {
                let Ok(ft) = e.file_type() else { continue };
                let p = e.path();
                if ft.is_symlink() {
                    continue;
                }
                if ft.is_dir() {
                    stack.push(p);
                    continue;
                }
                if !p.extension().is_some_and(|x| ["ttf", "ttc", "otf"].iter().any(|k| x.eq_ignore_ascii_case(k))) {
                    continue;
                }
                let Ok(bytes) = std::fs::read(&p) else { continue };
                let count = if bytes.starts_with(b"ttcf") && bytes.len() > 12 {
                    u32::from_be_bytes([bytes[8], bytes[9], bytes[10], bytes[11]]).min(64)
                } else {
                    1
                };
                for index in 0..count {
                    let one: Vec<u8> = if count > 1 || bytes.starts_with(b"ttcf") {
                        match uo_fonts::registry::ttc_face_by_index(&bytes, index, &FontLimits::NATIVE) {
                            Ok(b) => b,
                            Err(_) => continue,
                        }
                    } else {
                        bytes.clone()
                    };
                    let Ok(info) = uo_fonts::face_info(&one) else { continue };
                    let family = lower(&String::from_utf16_lossy(&info.names.family));
                    if family.is_empty() {
                        continue;
                    }
                    let list = families.entry(family).or_default();
                    let (bold, italic) = (info.weight >= 600, info.italic);
                    if list.iter().any(|f| f.bold == bold && f.italic == italic && f.weight == info.weight) {
                        continue;
                    }
                    list.push(Entry { path: p.clone(), index, bold, italic, weight: info.weight });
                }
            }
        }
        SystemFonts { families, loaded: Mutex::new(HashMap::new()), open: open_fonts() }
    }

    fn load(&self, e: &Entry) -> Option<Arc<Face>> {
        let key = (e.path.clone(), e.index);
        if let Some(f) = self.loaded.lock().unwrap().get(&key) {
            return f.clone();
        }
        let face = std::fs::read(&e.path).ok().and_then(|bytes| {
            let one = if bytes.starts_with(b"ttcf") {
                uo_fonts::registry::ttc_face_by_index(&bytes, e.index, &FontLimits::NATIVE).ok()?
            } else {
                bytes
            };
            Face::parse(Arc::from(one), &FontLimits::NATIVE).ok().map(Arc::new)
        });
        self.loaded.lock().unwrap().insert(key, face.clone());
        face
    }
}

impl FontSource for SystemFonts {
    fn face(&self, family: &str, bold: bool, italic: bool) -> Option<Arc<Face>> {
        if let Some(list) = self.families.get(&lower(family)).filter(|l| !l.is_empty()) {
            // the pick of the system index: 4 for bold not matching, 2 for italic, plus |weight - 400 or 700| / 1000
            let target = if bold { 700 } else { 400 };
            let score = |f: &Entry| {
                f64::from(u32::from(f.bold != bold) * 4 + u32::from(f.italic != italic) * 2)
                    + f64::from(u32::from(f.weight).abs_diff(target)) / 1000.0
            };
            let mut best = &list[0];
            for f in &list[1..] {
                if score(f) < score(best) {
                    best = f;
                }
            }
            if let Some(face) = self.load(best) {
                return Some(face);
            }
        }
        self.open.face(family, bold, italic)
    }

    fn fallback(&self, cp: u32, bold: bool, italic: bool, primary: &Face) -> Option<Arc<Face>> {
        fallback_chain(self, cp, bold, italic, primary).or_else(|| self.open.fallback(cp, bold, italic, primary))
    }
}

fn open_deck(path: &Path) -> Result<Deck, String> {
    let bytes = std::fs::read(path).map_err(|e| e.to_string())?;
    if path.extension().is_some_and(|x| x.eq_ignore_ascii_case("ppt")) {
        #[cfg(feature = "ppt")]
        {
            let parts = uo_pptx::ppt::convert(&bytes).map_err(|e| format!("ppt: {e}"))?;
            return Deck::from_parts(parts, Limits::default()).map_err(|e| e.to_string());
        }
        #[cfg(not(feature = "ppt"))]
        return Err("skipped: .ppt needs --features ppt".into());
    }
    Deck::open(&bytes, Limits::default()).map_err(|e| e.to_string())
}

#[test]
fn export_rust_pdfs_for_the_powerpoint_oracle() {
    let Ok(out) = std::env::var("UO_PPT_ORACLE_OUT") else {
        return;
    };
    let out = PathBuf::from(out);
    let list = std::fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/oracle_ppt/decks.tsv")).unwrap();
    let decks: Vec<(String, PathBuf)> = list
        .lines()
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
        .map(|l| {
            let mut f = l.split('\t');
            (f.next().unwrap().to_owned(), root().join(f.next().unwrap()))
        })
        .collect();
    let only: Option<Vec<String>> = std::env::var("UO_PPT_ORACLE_ONLY").ok().map(|s| s.split(',').map(str::to_owned).collect());
    let policies = std::env::var("UO_PPT_ORACLE_FONTS").unwrap_or_else(|_| "system,open".into());
    for policy in policies.split(',') {
        let t0 = Instant::now();
        let system;
        let open;
        let fonts: &dyn FontSource = if policy == "system" {
            system = SystemFonts::scan();
            eprintln!("system fonts: {} families indexed in {:.1} s", system.families.len(), t0.elapsed().as_secs_f64());
            &system
        } else {
            open = open_fonts();
            &open
        };
        let dir = out.join(format!("rust-{policy}"));
        std::fs::create_dir_all(&dir).unwrap();
        let mut report = String::from(
            "# id\tpages\tfailed slides\tglyphs\tfallback glyphs\tlinks\tshadows\traster layers\tbevels\twarps\tmissing (key=count;...)\tmissing per page\tms\terror\n",
        );
        let mut done = 0usize;
        for (id, path) in &decks {
            if only.as_ref().is_some_and(|o| !o.contains(id)) {
                continue;
            }
            done += 1;
            let t = Instant::now();
            let result = open_deck(path).and_then(|mut deck| uo_pptx::pdf(&mut deck, &PdfOptions::new(fonts)).map_err(|e| e.to_string()));
            let ms = t.elapsed().as_millis();
            match result {
                Ok(o) => {
                    std::fs::write(dir.join(format!("{id}.pdf")), &o.bytes).unwrap();
                    let r = &o.report;
                    let missing: Vec<String> = r.missing.iter().filter(|(_, c)| **c > 0).map(|(k, c)| format!("{k}={c}")).collect();
                    let per_page: Vec<String> = r.missing_per_page.iter().map(u64::to_string).collect();
                    let failed: Vec<String> = r.failed_slides.iter().map(u32::to_string).collect();
                    writeln!(
                        report,
                        "{id}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{ms}\t",
                        r.pages,
                        failed.join(","),
                        r.glyphs,
                        r.fallback_glyphs,
                        r.links,
                        r.shadows,
                        r.raster_layers,
                        r.bevels,
                        r.warps,
                        missing.join(";"),
                        per_page.join(",")
                    )
                    .unwrap();
                }
                Err(e) => {
                    writeln!(report, "{id}\t\t\t\t\t\t\t\t\t\t\t\t{ms}\t{}", e.replace(['\t', '\n'], " ")).unwrap();
                }
            }
        }
        std::fs::write(dir.join("report.tsv"), report).unwrap();
        eprintln!("rust-{policy}: {done} decks in {:.1} s -> {}", t0.elapsed().as_secs_f64(), dir.display());
    }
}
