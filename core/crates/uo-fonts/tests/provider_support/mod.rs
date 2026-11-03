//! Shared by the frontier-P tests, the corpus example and the capture scripts: the generated font
//! folder the system index and the C#/Node providers read (`corpus_files`), the pool the viewer
//! scenarios push, the scenario reader and the answer line every side prints. No proprietary font
//! anywhere: the real faces are the open Carlito, Caladea and Liberation of `shared/fonts/files`.
#![allow(dead_code, clippy::unwrap_used, clippy::panic)]

use std::path::{Path, PathBuf};
use std::sync::Arc;

use crate::b_support::{Font, name_table, os2, utf16be};
use uo_fonts::registry::FontAnswer;
use uo_testkit::generate::sfnt::{SfntOptions, build_ttc};
use uo_testkit::sha256::sha256;

/// The repository root, found from the real location of this crate (a workspace may reach it
/// through a link, which `canonicalize` resolves).
pub fn repo_root() -> PathBuf {
    let manifest = std::fs::canonicalize(env!("CARGO_MANIFEST_DIR")).unwrap();
    let root = manifest.join("../../..");
    let root = std::fs::canonicalize(&root).unwrap();
    assert!(root.join("shared/fonts/files").exists(), "{} is not the repository root", root.display());
    root
}

/// `(language, name id, string)` records of platform 3.
pub type Names<'a> = &'a [(u16, u16, &'a str)];

/// A loadable font with these names; `weight` and `fs_selection` in its `OS/2`; `variant` changes
/// the advance of glyph 0 so that every font of the corpus has its own bytes.
pub fn named_font(names: Names, weight: u16, fs_selection: u16, variant: u16) -> Font {
    let records: Vec<(u16, u16, u16, u16, Vec<u8>)> = names.iter().map(|(lang, id, s)| (3, 1, *lang, *id, utf16be(s))).collect();
    let mut os = os2(4, 96, 0, fs_selection);
    os[4..6].copy_from_slice(&weight.to_be_bytes());
    let hmtx = crate::b_support::hmtx(&[(500 + variant, 10), (600, 20), (700, 30)], 2);
    Font::basic().set(b"name", name_table(&records)).set(b"OS/2", os).set(b"hmtx", hmtx)
}

fn simple(family: &str, style: &str, weight: u16, fs: u16, variant: u16) -> Font {
    named_font(&[(0x409, 1, family), (0x409, 2, style)], weight, fs, variant)
}

/// The generated files of the system corpus, `(relative path, bytes)`, in a fixed order. Every
/// (family, bold, italic, weight) is unique across files, so that the pick does not depend on the
/// order the operating system lists a folder in; the order-dependent cases (ties, duplicates)
/// live inside `.ttc` files, where the face order is the file's.
pub fn corpus_files() -> Vec<(String, Vec<u8>)> {
    let mut out: Vec<(String, Vec<u8>)> = Vec::new();
    let mut add = |path: &str, bytes: Vec<u8>| out.push((path.to_owned(), bytes));
    add("sans/testsans-regular.ttf", simple("Test Sans", "Regular", 400, 0x40, 1).build());
    add("sans/testsans-bold.ttf", simple("Test Sans", "Bold", 700, 0x20, 2).build());
    add("sans/testsans-italic.ttf", simple("Test Sans", "Italic", 400, 0x01, 3).build());
    add("semi/semi-regular.ttf", simple("Semi Co", "Regular", 600, 0x40, 4).build());
    add("semi/semi-bold.ttf", simple("Semi Co", "Bold", 700, 0x20, 5).build());
    add("weights/wt-regular.ttf", simple("Wt Fam", "Regular", 400, 0x40, 6).build());
    add("weights/wt-medium.ttf", simple("Wt Fam", "Medium", 500, 0x40, 7).build());
    add(
        "typo/typo-display.ttf",
        named_font(&[(0x409, 1, "Typo Display"), (0x409, 2, "Regular"), (0x409, 16, "Typo")], 400, 0x40, 8).build(),
    );
    add(
        "typo/typo-shadowed.ttf",
        named_font(&[(0x409, 1, "Typo Shadowed"), (0x409, 2, "Regular"), (0x409, 16, "Typo Display")], 400, 0x40, 9).build(),
    );
    add("typo/aptos-like.ttf", simple("Aptos Like", "Regular", 400, 0x40, 10).build());
    add(
        "typo/aptos-like-display.ttf",
        named_font(&[(0x409, 1, "Aptos Like Display"), (0x409, 2, "Regular"), (0x409, 16, "Aptos Like")], 350, 0x40, 11).build(),
    );
    add(
        "names/catalan-first.ttf",
        named_font(
            &[
                (0x403, 1, "Nom Catal\u{e0}"),
                (0x409, 1, "English Name"),
                (0x403, 2, "Negreta"),
                (0x409, 2, "Bold Italic"),
                (0x403, 16, "Cat Typo"),
            ],
            700,
            0x00,
            12,
        )
        .build(),
    );
    add("names/vertical.ttf", simple("@Vertical Gothic", "Regular", 400, 0x40, 13).build());
    add("names/no-family.ttf", named_font(&[(0x409, 2, "Regular")], 400, 0x40, 14).build());
    add("names/blank-family.ttf", simple("   ", "Regular", 400, 0x40, 15).build());
    add("names/unicode.ttf", simple("\u{dc}n\u{ef}code F\u{f6}nt", "Regular", 400, 0x40, 16).build());
    add("names/padded.ttf", simple("  Padded Name\u{a0}", "Regular", 400, 0x40, 17).build());
    add("names/oblique.ttf", named_font(&[(0x409, 1, "Slant Co"), (0x409, 2, "Bold Oblique")], 0, 0x00, 18).build());
    add("names/bold-in-word.ttf", named_font(&[(0x409, 1, "Wordy Co"), (0x409, 2, "SemiBold Condensed")], 600, 0x00, 19).build());
    add("names/no-subfamily.ttf", named_font(&[(0x409, 1, "No Sub Co")], 400, 0x00, 20).build());
    add("names/mac-only.ttf", {
        let records = vec![(1u16, 0u16, 0u16, 1u16, b"Mac Only".to_vec()), (1, 0, 0, 2, b"Regular".to_vec())];
        Font::basic().set(b"name", name_table(&records)).build()
    });
    let odd = named_font(&[(0x409, 1, "Odd Length"), (0x409, 2, "Regular")], 400, 0x40, 21);
    add(
        "names/odd-length.ttf",
        odd.set(b"name", name_table(&[(3, 1, 0x409, 1, b"Odd".to_vec()), (3, 1, 0x409, 2, b"R".to_vec())])).build(),
    );
    add("names/no-name-table.ttf", Font::basic().without(b"name").build());
    add("names/broken-face.ttf", simple("Broken Face", "Regular", 400, 0x40, 22).without(b"head").build());
    add("names/short-os2.ttf", simple("Short Os2", "Bold", 400, 0x40, 23).set(b"OS/2", vec![0u8; 50]).build());
    add("deep/a/b/deep-font.OTF", simple("Deep Font", "Regular", 400, 0x40, 24).build());
    add("ignored/readme.txt", b"not a font".to_vec());
    add("ignored/sans.ttf.bak", simple("Backup Only", "Regular", 400, 0x40, 25).build());
    add("ignored/truncated.ttf", simple("Truncated", "Regular", 400, 0x40, 26).build()[..40].to_vec());
    let face = |names: Names, weight: u16, fs: u16, variant: u16| {
        let f = named_font(names, weight, fs, variant);
        (f.tables.clone(), SfntOptions::default())
    };
    let coll = build_ttc(
        &[
            face(&[(0x409, 1, "Coll A"), (0x409, 2, "Regular")], 400, 0x40, 30),
            face(&[(0x409, 1, "Coll A"), (0x409, 2, "Bold")], 700, 0x20, 31),
            (Font::basic().without(b"name").tables, SfntOptions::default()),
            face(&[(0x409, 1, "Coll B"), (0x409, 2, "Regular")], 400, 0x40, 33),
            face(&[(0x409, 1, "Coll A"), (0x409, 2, "Regular")], 400, 0x40, 34),
        ],
        0x0001_0000,
    );
    add("ttc/coll.ttc", coll);
    let tie = build_ttc(
        &[
            face(&[(0x409, 1, "Tie Fam"), (0x409, 2, "Regular")], 300, 0x40, 40),
            face(&[(0x409, 1, "Tie Fam"), (0x409, 2, "Regular")], 500, 0x40, 41),
        ],
        0x0001_0000,
    );
    add("ttc/tie.ttc", tie);
    out
}

/// Writes the corpus files under `dir`, and the viewer pool under `dir/pool/<name>.bin`.
pub fn write_corpus(dir: &Path) {
    std::fs::create_dir_all(dir.join("pool")).unwrap();
    for (name, bytes) in pool_files() {
        std::fs::write(dir.join("pool").join(format!("{name}.bin")), bytes).unwrap();
    }
    for (rel, bytes) in corpus_files() {
        let path = dir.join(&rel);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, bytes).unwrap();
    }
}

/// A folder under the system temp dir, removed when dropped.
pub struct TempDir(pub PathBuf);

impl TempDir {
    pub fn new(tag: &str) -> TempDir {
        let path = std::env::temp_dir()
            .join(format!("uo-fonts-{tag}-{}-{:?}", std::process::id(), std::thread::current().id()).replace(['(', ')'], ""));
        let _ = std::fs::remove_dir_all(&path);
        std::fs::create_dir_all(&path).unwrap();
        TempDir(path)
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// The files of the viewer pool, `(name, bytes)`: small generated faces, a text that is no font and
/// a real font cut short (so that hashing the pool a thousand times costs nothing).
pub fn pool_files() -> Vec<(&'static str, Vec<u8>)> {
    let font = |variant: u16, style: &str, weight: u16, fs: u16| simple("Pool Fam", style, weight, fs, 100 + variant).build();
    let regular = font(0, "Regular", 400, 0x40);
    let cut = regular[..40].to_vec();
    vec![
        ("carlito-regular", regular),
        ("carlito-bold", font(1, "Bold", 700, 0x20)),
        ("carlito-italic", font(2, "Italic", 400, 0x01)),
        ("carlito-bolditalic", font(3, "Bold Italic", 700, 0x21)),
        ("caladea-regular", font(4, "Regular", 400, 0x40)),
        ("caladea-bold", font(5, "Bold", 700, 0x20)),
        ("serif-regular", font(6, "Regular", 400, 0x40)),
        ("mono-regular", font(7, "Regular", 400, 0x40)),
        ("garbage", (0u8..64).collect()),
        ("cut", cut),
    ]
}

/// The viewer pool as shared buffers.
pub fn viewer_pool() -> Vec<(&'static str, Arc<[u8]>)> {
    pool_files().into_iter().map(|(n, b)| (n, Arc::from(b))).collect()
}

pub fn sha_hex(bytes: &[u8]) -> String {
    sha256(bytes).iter().map(|b| format!("{b:02x}")).collect()
}

/// The path of a key reduced to its file name: `family|bi|<path>#index` to `family|bi|<name>#index`,
/// so a capture taken on another machine compares.
pub fn normalize_key(key: &str) -> String {
    let Some((head, tail)) = key.rsplit_once('#') else { return key.to_owned() };
    let mut parts = head.splitn(3, '|');
    let (family, flags, path) = (parts.next().unwrap_or(""), parts.next().unwrap_or(""), parts.next().unwrap_or(""));
    let name = path.rsplit(['/', '\\']).next().unwrap_or(path);
    format!("{family}|{flags}|{name}#{tail}")
}

/// The line every side prints for an answer: `-` for none, else `key|family|sha256|bold|italic|
/// synthBold|synthItalic|standIn|identity` with the identity the index of the first answer (in
/// `seen`) that is the same object, flags as `0`/`1`.
pub fn answer_line(answer: Option<&Arc<FontAnswer>>, seen: &mut Vec<u64>) -> String {
    let Some(a) = answer else { return "-".to_owned() };
    let id = a.id().get();
    let at = seen.iter().position(|s| *s == id).unwrap_or_else(|| {
        seen.push(id);
        seen.len() - 1
    });
    let f = |b: bool| u8::from(b);
    format!(
        "{}|{}|{}|{}{}{}{}{}|{at}",
        normalize_key(&a.key),
        a.family,
        sha_hex(a.bytes()),
        f(a.bold),
        f(a.italic),
        f(a.synth_bold),
        f(a.synth_italic),
        f(a.stand_in)
    )
}

/// One row of the scenario file.
pub type Op = Vec<String>;

/// `(id, ops)` of the scenarios opened by `kind` (`V` viewer, `S` system) in
/// `tests/parity/provider_scenarios.txt`.
pub fn scenarios(kind: &str) -> Vec<(String, Vec<Op>)> {
    let text = std::fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/parity/provider_scenarios.txt")).unwrap();
    let rows: Vec<Op> = text.lines().filter(|l| !l.is_empty()).map(|l| l.split('\t').map(str::to_owned).collect()).collect();
    let mut out = Vec::new();
    let mut i = 0;
    while i < rows.len() {
        if rows[i][0] == kind {
            let id = rows[i][1].clone();
            let mut ops = Vec::new();
            i += 1;
            while rows[i][0] != "E" {
                ops.push(rows[i].clone());
                i += 1;
            }
            out.push((id, ops));
        }
        i += 1;
    }
    out
}
