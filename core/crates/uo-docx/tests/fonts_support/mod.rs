//! Shared by the D1b font-embedding tests: the inputs of `js/capture-font-embedding.mjs`
//! (`tests/parity/font-embedding.inputs`: the synthetic packages and every record's plan), the open
//! fonts they name, and the plan and GUID source built from them.
#![allow(dead_code, clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;

use uo_fonts::{EmbeddingPlan, FaceToEmbed, FontEmbeddingSettings, Variant, face_info};
use uo_opc::Package;
use uo_opc::package::{OpenOptions, SaveOptions};
use uo_testkit::generate::Rng;
use uo_testkit::repo_root;

pub type Parts = Vec<(String, Vec<u8>)>;

#[derive(Debug, Clone)]
pub struct FaceSpec {
    pub family: String,
    pub variant: Variant,
    pub file: String,
    pub subsetted: bool,
    pub panose: String,
}

#[derive(Debug, Clone)]
pub struct PlanSpec {
    pub settings: FontEmbeddingSettings,
    pub seed: u32,
    pub faces: Vec<FaceSpec>,
}

#[derive(Debug, Default)]
pub struct Inputs {
    pub packages: Vec<(String, Parts)>,
    pub plans: HashMap<String, PlanSpec>,
}

pub fn unescape(s: &str) -> String {
    let mut out = String::new();
    let mut it = s.chars();
    while let Some(c) = it.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }
        match it.next() {
            Some('n') => out.push('\n'),
            Some('r') => out.push('\r'),
            Some('t') => out.push('\t'),
            Some(o) => out.push(o),
            None => {}
        }
    }
    out
}

fn from_hex(h: &str) -> Vec<u8> {
    (0..h.len() / 2).map(|i| u8::from_str_radix(&h[2 * i..2 * i + 2], 16).unwrap()).collect()
}

fn variant(v: &str) -> Variant {
    match v {
        "regular" => Variant::Regular,
        "bold" => Variant::Bold,
        "italic" => Variant::Italic,
        "boldItalic" => Variant::BoldItalic,
        _ => panic!("variant {v}"),
    }
}

pub fn inputs() -> Inputs {
    let text = std::fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/parity/font-embedding.inputs")).unwrap();
    let mut out = Inputs::default();
    let mut plan_key: Option<String> = None;
    for line in text.lines().filter(|l| !l.starts_with('#')) {
        if let Some(key) = line.strip_prefix("@input ") {
            out.packages.push((key.to_owned(), Vec::new()));
            plan_key = None;
        } else if let Some(rest) = line.strip_prefix("@plan\t") {
            let f: Vec<&str> = rest.split('\t').collect();
            let b = f[1].as_bytes();
            let settings = FontEmbeddingSettings { embed: b[0] == b'1', subset: b[1] == b'1', skip_common_fonts: b[2] == b'1' };
            out.plans.insert(f[0].to_owned(), PlanSpec { settings, seed: f[2].parse().unwrap(), faces: Vec::new() });
            plan_key = Some(f[0].to_owned());
        } else if let Some(rest) = line.strip_prefix("face\t") {
            let f: Vec<&str> = rest.split('\t').collect();
            let spec = FaceSpec {
                family: unescape(f[0]),
                variant: variant(f[1]),
                file: f[2].to_owned(),
                subsetted: f[3] == "1",
                panose: unescape(f[4]),
            };
            out.plans.get_mut(plan_key.as_ref().unwrap()).unwrap().faces.push(spec);
        } else if let Some((path, data)) = line.split_once('\t') {
            let bytes = match data.strip_prefix("hex:") {
                Some(h) => from_hex(h),
                None => unescape(data).into_bytes(),
            };
            out.packages.last_mut().unwrap().1.push((path.to_owned(), bytes));
        }
    }
    out
}

/// The open fonts of the repository (`shared/fonts/files/*.ttf`), by file name.
pub fn fonts() -> HashMap<String, Arc<[u8]>> {
    let dir = repo_root(env!("CARGO_MANIFEST_DIR")).join("shared/fonts/files");
    let mut out = HashMap::new();
    for e in std::fs::read_dir(dir).unwrap() {
        let p = e.unwrap().path();
        if p.extension().is_some_and(|x| x == "ttf") {
            out.insert(p.file_name().unwrap().to_string_lossy().into_owned(), Arc::from(std::fs::read(&p).unwrap()));
        }
    }
    out
}

/// The plan of a spec: the face bytes shared with the font table, `info` from uo-fonts with the
/// panose the TS recorded.
pub fn plan_of(spec: &PlanSpec, fonts: &HashMap<String, Arc<[u8]>>) -> EmbeddingPlan {
    let faces = spec
        .faces
        .iter()
        .map(|f| {
            let bytes = fonts[&f.file].clone();
            let mut info = face_info(&bytes).unwrap();
            info.panose = f.panose.clone();
            FaceToEmbed { family: f.family.clone(), variant: f.variant, bytes, subsetted: f.subsetted, info }
        })
        .collect();
    EmbeddingPlan { faces, skipped: Vec::new() }
}

/// The TS `crypto.getRandomValues` replacement: 16 draws of the testkit xorshift32, low byte each.
pub fn guid_source(seed: u32) -> impl FnMut() -> [u8; 16] {
    let mut rng = Rng::new(seed);
    move || {
        let mut b = [0u8; 16];
        for x in &mut b {
            *x = rng.next_u32() as u8;
        }
        b
    }
}

/// A synthetic package as a saved and reopened zip (its parts are entries of the archive, so a
/// part the step sets is the only kind without an [`Package::entry`]).
pub fn open_parts(parts: &Parts) -> Package {
    let limits = uo_opc::Limits::DEFAULT;
    let opts = OpenOptions::default();
    let bytes = Package::from_parts(parts.clone(), &limits, &opts).save(&SaveOptions::default()).unwrap();
    Package::open(&bytes, &limits, &opts).unwrap()
}
