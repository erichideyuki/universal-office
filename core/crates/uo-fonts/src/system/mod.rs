//! The native system font index (feature `system`, never in the wasm): the machine's installed
//! fonts for the server PDF. Port of `dotnet/src/UniversalOffice.Core/Fonts/SystemFonts.cs`
//! (`SystemFontIndex`), itself the port of `write/tools/corpus-lab/node-fonts.mjs`; the roots are
//! those of the Electron shell (`shared/fonts/node/system-fonts.cjs`) plus the Linux and macOS
//! folders the C# added. The fonts are **read** from the machine at run time; nothing is copied
//! into the repository or into any output the caller did not ask for.
//!
//! What the port keeps from the reference, because it decides ties and de-duplication: the roots
//! in order, each walked depth first with a stack (files handled as met, subfolders pushed and
//! popped last in first out), the entries of a folder in the order the operating system lists them
//! (no sorting), a reparse point or symlink is never followed, only `.ttf`, `.otf` and `.ttc`
//! (any case) are read, the family indexed by the name ID 1 first and by the typographic ID 16 as
//! a second index, one entry per (family, bold, italic, weight) per index (the first found wins),
//! and the pick by score (`4` for bold not matching, `2` for italic, plus `|weight - 400 or 700| /
//! 1000`; the first of the lowest in index order).
//!
//! **Thread safety**: the index is read-only after [`SystemFontIndex::create`]; loading goes
//! through the [`FaceCache`] (one lock, parses outside it). [`SystemFontIndex::shared`] is the
//! process-wide index (the C# `SystemFontProvider.Shared`): built once under a lock, so the
//! parallel conversions of the server wait for one scan and then run without blocking each other.

mod info;

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, PoisonError};

use uo_jsnum::text::to_lowercase;

use crate::registry::{CachedFace, FaceCache, FaceSource, SourceFace, ttc_face_by_index};
use info::{FaceInfo, infos};

/// One indexed face: the file, the face in it (0 for `.ttf`/`.otf`) and what its tables declare.
/// In the typographic index, `family` is the typographic name.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SystemFontEntry {
    pub path: PathBuf,
    pub index: u32,
    pub family: String,
    pub typo: Option<String>,
    pub bold: bool,
    pub italic: bool,
    pub weight: u16,
}

/// See the module documentation.
pub struct SystemFontIndex {
    roots: Vec<PathBuf>,
    entries: Vec<SystemFontEntry>,
    /// lower-cased family (name ID 1) to its faces
    families: HashMap<String, Vec<SystemFontEntry>>,
    /// the keys of `families` in insertion order (the JS `Map` order)
    order: Vec<String>,
    /// lower-cased typographic family (name ID 16)
    typo: HashMap<String, Vec<SystemFontEntry>>,
    cache: Arc<FaceCache>,
}

/// The roots of a machine, the existing ones in order: `%WINDIR%\Fonts`, the user's
/// `Microsoft\Windows\Fonts` and the Office `CloudFonts` cache (the three the Electron shell
/// reads), on Linux and macOS the standard folders, `EXTRA_FONT_DIRS` (split on `;`) and
/// `extra_roots`.
pub fn default_roots(extra_roots: &[PathBuf]) -> Vec<PathBuf> {
    let env = |name: &str| std::env::var_os(name).filter(|v| !v.is_empty()).map(PathBuf::from);
    let home = std::env::home_dir().unwrap_or_default();
    let local = env("LOCALAPPDATA").unwrap_or_else(|| home.join("AppData").join("Local"));
    let mut all: Vec<PathBuf> = vec![
        env("WINDIR").unwrap_or_else(|| PathBuf::from("C:/Windows")).join("Fonts"),
        local.join("Microsoft/Windows/Fonts"),
        local.join("Microsoft/FontCache/4/CloudFonts"),
    ];
    if !cfg!(windows) {
        all.extend(
            [
                "/usr/share/fonts",
                "/usr/local/share/fonts",
                ".fonts",
                ".local/share/fonts",
                "/System/Library/Fonts",
                "/Library/Fonts",
                "Library/Fonts",
            ]
            .iter()
            .map(|p| if p.starts_with('/') { PathBuf::from(p) } else { home.join(p) }),
        );
    }
    if let Some(extra) = std::env::var_os("EXTRA_FONT_DIRS") {
        all.extend(extra.to_string_lossy().split(';').filter(|s| !s.is_empty()).map(PathBuf::from));
    }
    all.extend(extra_roots.iter().filter(|p| !p.as_os_str().is_empty()).cloned());
    all.into_iter().filter(|p| p.exists()).collect()
}

impl SystemFontIndex {
    /// Scans [`default_roots`] and loads through the process-wide [`FaceCache`].
    pub fn create(extra_roots: &[PathBuf]) -> SystemFontIndex {
        SystemFontIndex::with_roots(default_roots(extra_roots), FaceCache::shared())
    }

    /// Scans exactly these roots (the tests, a host that knows where its fonts are) into `cache`.
    pub fn with_roots(roots: Vec<PathBuf>, cache: Arc<FaceCache>) -> SystemFontIndex {
        let mut index =
            SystemFontIndex { roots, entries: Vec::new(), families: HashMap::new(), order: Vec::new(), typo: HashMap::new(), cache };
        index.build();
        index
    }

    /// The process-wide index (built at the first call, with the default roots).
    pub fn shared() -> Arc<SystemFontIndex> {
        let mut slot = SHARED.lock().unwrap_or_else(PoisonError::into_inner);
        Arc::clone(slot.get_or_insert_with(|| Arc::new(SystemFontIndex::create(&[]))))
    }

    /// Drops the process-wide index (its entries; the faces it loaded stay with whoever holds
    /// them, the rest in the [`FaceCache`] until it is cleared): the end of a host's life, and what
    /// the memory tests do before they measure.
    pub fn release_shared() {
        *SHARED.lock().unwrap_or_else(PoisonError::into_inner) = None;
    }

    /// Scans again and replaces the process-wide index (a font was installed, a test wants other
    /// roots). Answers already given keep their faces.
    pub fn rebuild_shared(extra_roots: &[PathBuf]) -> Arc<SystemFontIndex> {
        let mut slot = SHARED.lock().unwrap_or_else(PoisonError::into_inner);
        let index = Arc::new(SystemFontIndex::create(extra_roots));
        *slot = Some(Arc::clone(&index));
        index
    }

    /// The roots that exist, in scan order.
    pub fn roots(&self) -> &[PathBuf] {
        &self.roots
    }

    /// Every face read, in scan order (diagnostics and parity).
    pub fn entries(&self) -> &[SystemFontEntry] {
        &self.entries
    }

    /// The family names (name ID 1) in index order.
    pub fn families(&self) -> Vec<String> {
        self.order.iter().filter_map(|k| self.families.get(k).and_then(|l| l.first()).map(|e| e.family.clone())).collect()
    }

    pub fn cache(&self) -> &Arc<FaceCache> {
        &self.cache
    }

    fn build(&mut self) {
        for root in self.roots.clone() {
            let mut stack = vec![root];
            while let Some(dir) = stack.pop() {
                let Some(listing) = read_dir(&dir) else { continue };
                for (name, is_dir) in listing {
                    let path = dir.join(&name);
                    if is_dir {
                        stack.push(path);
                        continue;
                    }
                    if !has_font_extension(&name.to_string_lossy()) {
                        continue;
                    }
                    for (face, index) in infos(&path) {
                        self.add(&path, index, &face);
                    }
                }
            }
        }
    }

    fn add(&mut self, path: &Path, index: u32, face: &FaceInfo) {
        self.entries.push(SystemFontEntry {
            path: path.to_path_buf(),
            index,
            family: face.family.clone(),
            typo: face.typo.clone(),
            bold: face.bold,
            italic: face.italic,
            weight: face.weight,
        });
        // the RIBBI name (ID 1) first; the typographic one (ID 16) only as an alternative
        // ("Aptos Display" has ID 16 = "Aptos" and must not shadow Aptos)
        add_to(&mut self.families, Some(&face.family), face, path, index, Some(&mut self.order));
        add_to(&mut self.typo, face.typo.as_deref(), face, path, index, None);
    }

    /// The lower-cased lookup of the pick: the family index, else the typographic one.
    fn list_of(&self, family: &str) -> Option<&Vec<SystemFontEntry>> {
        let lower = to_lowercase(family);
        self.families.get(&lower).or_else(|| self.typo.get(&lower)).filter(|l| !l.is_empty())
    }

    fn load(&self, entry: &SystemFontEntry) -> Option<CachedFace> {
        let alias = format!("{}#{}", entry.path.display(), entry.index);
        let limits = *self.cache.limits();
        self.cache.get_or_load(&alias, || {
            let size = std::fs::metadata(&entry.path).ok()?.len();
            if size > limits.max_output_bytes {
                return None;
            }
            let bytes = std::fs::read(&entry.path).ok()?;
            if bytes.starts_with(b"ttcf") { ttc_face_by_index(&bytes, entry.index, &limits).ok() } else { Some(bytes) }
        })
    }
}

static SHARED: Mutex<Option<Arc<SystemFontIndex>>> = Mutex::new(None);

impl FaceSource for SystemFontIndex {
    fn pick(&self, family: &str, bold: bool, italic: bool) -> Option<SourceFace> {
        let list = self.list_of(family)?;
        let target = if bold { 700 } else { 400 };
        let score = |f: &SystemFontEntry| -> f64 {
            let flags = u32::from(f.bold != bold) * 4 + u32::from(f.italic != italic) * 2;
            f64::from(flags) + f64::from(u32::from(f.weight).abs_diff(target)) / 1000.0
        };
        let mut best = list.first()?;
        let mut best_score = score(best);
        for f in list.iter().skip(1) {
            let s = score(f);
            if s < best_score {
                best = f;
                best_score = s;
            }
        }
        let face = self.load(best)?;
        Some(SourceFace {
            key: format!(
                "{}|{}{}|{}#{}",
                best.family,
                if best.bold { "b" } else { "" },
                if best.italic { "i" } else { "" },
                best.path.display(),
                best.index
            ),
            family: best.family.clone(),
            face,
            synth_bold: bold && !best.bold,
            synth_italic: italic && !best.italic,
        })
    }
}

fn add_to(
    map: &mut HashMap<String, Vec<SystemFontEntry>>,
    family: Option<&str>,
    face: &FaceInfo,
    path: &Path,
    index: u32,
    order: Option<&mut Vec<String>>,
) {
    let Some(family) = family.filter(|f| !f.is_empty()) else { return };
    let key = to_lowercase(family);
    let fresh = !map.contains_key(&key);
    let list = map.entry(key.clone()).or_default();
    if fresh && let Some(order) = order {
        order.push(key);
    }
    if !list.iter().any(|f| f.bold == face.bold && f.italic == face.italic && f.weight == face.weight) {
        list.push(SystemFontEntry {
            path: path.to_path_buf(),
            index,
            family: family.to_owned(),
            typo: face.typo.clone(),
            bold: face.bold,
            italic: face.italic,
            weight: face.weight,
        });
    }
}

/// `/\.(ttf|otf|ttc)$/i`.
fn has_font_extension(name: &str) -> bool {
    let b = name.as_bytes();
    if b.len() < 4 || b[b.len() - 4] != b'.' {
        return false;
    }
    let ext = &b[b.len() - 3..];
    [b"ttf", b"otf", b"ttc"].iter().any(|e| ext.eq_ignore_ascii_case(*e))
}

/// `readdirSync(dir, { withFileTypes: true })`: the whole listing or nothing (a folder that fails
/// partway is skipped), in the order of the operating system, with whether each entry is a folder
/// (a reparse point or a symlink never is).
fn read_dir(dir: &Path) -> Option<Vec<(std::ffi::OsString, bool)>> {
    let mut out = Vec::new();
    for entry in std::fs::read_dir(dir).ok()? {
        let entry = entry.ok()?;
        out.push((entry.file_name(), is_directory(&entry)?));
    }
    Some(out)
}

#[cfg(windows)]
fn is_directory(entry: &std::fs::DirEntry) -> Option<bool> {
    use std::os::windows::fs::MetadataExt;
    const DIRECTORY: u32 = 0x10;
    const REPARSE_POINT: u32 = 0x400;
    let attributes = entry.metadata().ok()?.file_attributes();
    Some(attributes & REPARSE_POINT == 0 && attributes & DIRECTORY != 0)
}

#[cfg(not(windows))]
fn is_directory(entry: &std::fs::DirEntry) -> Option<bool> {
    Some(entry.file_type().ok()?.is_dir())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extensions_are_matched_like_the_regex() {
        for ok in ["a.ttf", "A.TTF", "x.OtF", "b.ttc", "...ttc", ".ttf"] {
            assert!(has_font_extension(ok), "{ok}");
        }
        for no in ["ttf", "a.tt", "a.ttff", "a.ttf.bak", "a_ttf", "", "a.ttc ", "a.woff"] {
            assert!(!has_font_extension(no), "{no}");
        }
    }
}
