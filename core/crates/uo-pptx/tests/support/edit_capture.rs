//! The replay of the editing captures of frontier B8 (`js/capture-edit.mjs`: `tests/captures/edit-*.capture` + the
//! `.ops.gz` sidecar with the script lines), over any [`Driver`]: `tests/edit_replay.rs` drives
//! `uo_pptx::edit::Editor` directly ([`EditorDriver`]); `uo-wasm/tests/slides_edit_native.rs` drives the editor exports
//! of the wasm binding (the message codec, the deck shared with the viewer handle, the parts the shell mirrors).
//! After EVERY step the facets are the part bytes (digest of the whole package in order), the UI state with the history
//! flags, the presentation model, the model of the current slide and what the step returned.
#![allow(dead_code, clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::cast_possible_truncation)]

use std::path::{Path, PathBuf};
use std::sync::Arc;

use uo_dml_render::canon::Canon;
use uo_jsnum::format::to_js_string;
use uo_pptx::canon::{presentation_json, slide_record_with_timing, text_digest};
use uo_pptx::edit::blank::{BlankDeckOptions, blank_deck_parts};
use uo_pptx::edit::doc::{DocProperties, ShapeClip, ShowSettingsSpec};
use uo_pptx::edit::pkg::TransitionSpec;
use uo_pptx::edit::shapes::{ArrangeMode, BulletKind, Frame, RunFormat};
use uo_pptx::edit::{Editor, Op, Outcome, Target};
use uo_pptx::{Deck, Limits, PartStore};
use uo_testkit::{Capture, Replay};

/// The uo-pptx crate directory (this file is also included from uo-wasm's tests).
pub fn pptx_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../uo-pptx")
}

pub fn captures() -> PathBuf {
    pptx_dir().join("tests/captures")
}

pub fn repo_root() -> PathBuf {
    pptx_dir().join("../../..")
}

pub fn gunzip(b: &[u8]) -> String {
    assert!(b.len() > 18 && b[0] == 0x1f && b[1] == 0x8b && b[3] == 0, "plain gzip from zlib.gzipSync");
    String::from_utf8(uo_opc::flate::inflate_raw(&b[10..b.len() - 8], None, 1 << 30).unwrap()).unwrap()
}

// ───────────────────────── tokens of the sidecar ─────────────────────────

fn pct(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%' {
            out.push(u8::from_str_radix(&s[i + 1..i + 3], 16).unwrap());
            i += 3;
        } else {
            out.push(b[i]);
            i += 1;
        }
    }
    String::from_utf8(out).unwrap()
}

fn f64_of(hex: &str) -> f64 {
    f64::from_bits(u64::from_str_radix(hex, 16).unwrap())
}

struct Toks<'a>(std::slice::Iter<'a, &'a str>);

impl<'a> Toks<'a> {
    fn next(&mut self) -> &'a str {
        self.0.next().copied().expect("missing token")
    }
    fn opt_s(&mut self) -> Option<String> {
        let t = self.next();
        match t {
            "u" | "N" => None,
            _ => Some(pct(t.strip_prefix("s:").expect("string token"))),
        }
    }
    fn s(&mut self) -> String {
        self.opt_s().expect("string")
    }
    fn opt_n(&mut self) -> Option<f64> {
        let t = self.next();
        match t {
            "u" | "N" => None,
            _ => Some(f64_of(t.strip_prefix("n:").expect("number token"))),
        }
    }
    fn n(&mut self) -> f64 {
        self.opt_n().expect("number")
    }
    fn i(&mut self) -> i64 {
        self.n() as i64
    }
    fn opt_b(&mut self) -> Option<bool> {
        let t = self.next();
        match t {
            "u" | "N" => None,
            _ => Some(t.strip_prefix("b:").expect("bool token") == "1"),
        }
    }
    fn b(&mut self) -> bool {
        self.opt_b().expect("bool")
    }
    fn frame(&mut self) -> Option<Frame> {
        let t = self.next();
        let v = t.strip_prefix("f:")?;
        let p: Vec<f64> = v.split(',').map(f64_of).collect();
        Some(Frame { x: p[0], y: p[1], cx: p[2], cy: p[3] })
    }
    fn target(&mut self) -> Target {
        let t = self.next();
        if t == "A" { Target::All } else { Target::Slide(f64_of(t.strip_prefix("n:").unwrap()) as i64) }
    }
}

/// What a script line asks for: an editor operation or a UI/session action.
pub enum Cmd {
    Op(Op),
    Copy(String),
    Paste,
    XPaste(String, i64, String),
    Select(Option<String>),
    Goto(u32),
    Undo,
    Redo,
    MarkClean,
    Embed(Option<bool>, Option<bool>),
    SaveFonts,
}

pub fn parse_cmd(parts: &[&str]) -> Cmd {
    let name = parts[0];
    let rest = parts[1..].to_vec();
    let mut t = Toks(rest.iter());
    match name {
        "addSlide" => Cmd::Op(Op::AddSlide { layout: t.opt_s() }),
        "duplicateSlide" => Cmd::Op(Op::DuplicateSlide { i: t.i() }),
        "deleteSlide" => Cmd::Op(Op::DeleteSlide { i: t.i() }),
        "moveSlide" => Cmd::Op(Op::MoveSlide { from: t.i(), to: t.i() }),
        "hideSlide" => Cmd::Op(Op::SetSlideHidden { i: t.i(), hidden: t.b() }),
        "layout" => Cmd::Op(Op::SetSlideLayout { i: t.i(), layout: t.s() }),
        "background" => Cmd::Op(Op::SetSlideBackground { target: t.target(), color: t.opt_s() }),
        "transition" => {
            let target = t.target();
            let first = t.next();
            let spec = if first == "N" {
                None
            } else {
                let ty = pct(first.strip_prefix("s:").unwrap());
                Some(TransitionSpec {
                    type_: ty,
                    dir: t.opt_s(),
                    speed: t.opt_s(),
                    advance_on_click: t.opt_b(),
                    advance_after_ms: t.opt_n(),
                })
            };
            Cmd::Op(Op::SetSlideTransition { target, spec })
        }
        "showSettings" => Cmd::Op(Op::SetShowSettings(ShowSettingsSpec {
            loop_: t.opt_b(),
            use_timings: t.opt_b(),
            show_animation: t.opt_b(),
            kiosk: t.opt_b(),
        })),
        "text" => Cmd::Op(Op::SetShapeText { shape: t.s(), text: t.s() }),
        "typing" => Cmd::Op(Op::Typing { slide: t.i(), shape: t.s(), text: t.s() }),
        "format" => {
            let shape = t.s();
            let f = RunFormat {
                bold: t.opt_b(),
                italic: t.opt_b(),
                underline: t.opt_b(),
                strike: t.opt_b(),
                size: t.opt_n(),
                font: t.opt_s(),
                color: t.opt_s(),
            };
            Cmd::Op(Op::FormatText { shape, f })
        }
        "grow" => Cmd::Op(Op::GrowFont { shape: t.s(), dir: t.i() as i32, base: t.n() }),
        "align" => Cmd::Op(Op::AlignText { shape: t.s(), algn: t.s() }),
        "bullets" => {
            let shape = t.s();
            let kind = match t.s().as_str() {
                "bullet" => BulletKind::Bullet,
                "number" => BulletKind::Number,
                _ => BulletKind::None,
            };
            Cmd::Op(Op::SetBullets { shape, kind })
        }
        "clearFormat" => Cmd::Op(Op::ClearFormatting { shape: t.s() }),
        "link" => Cmd::Op(Op::SetLink { shape: t.s(), url: t.opt_s() }),
        "textBox" => Cmd::Op(Op::InsertTextBox { text: t.s(), frame: t.frame() }),
        "shape" => Cmd::Op(Op::InsertShape { prst: t.s(), frame: t.frame() }),
        "image" => {
            let h = t.next().strip_prefix("h:").unwrap();
            let bytes: Vec<u8> = (0..h.len() / 2).map(|i| u8::from_str_radix(&h[2 * i..2 * i + 2], 16).unwrap()).collect();
            let (mime, frame) = (t.s(), t.frame());
            let size_px = match (t.opt_n(), t.opt_n()) {
                (Some(w), Some(h)) => Some((w, h)),
                (None, None) => None,
                (w, h) => Some((w.unwrap_or(640.0), h.unwrap_or(480.0))),
            };
            Cmd::Op(Op::InsertImage { bytes: Arc::from(bytes), mime, frame, size_px })
        }
        "frame" => Cmd::Op(Op::SetShapeFrame { shape: t.s(), frame: Frame { x: t.n(), y: t.n(), cx: t.n(), cy: t.n() } }),
        "fill" => Cmd::Op(Op::SetShapeFill { shape: t.s(), color: t.opt_s() }),
        "outline" => Cmd::Op(Op::SetShapeOutline { shape: t.s(), color: t.opt_s() }),
        "arrange" => {
            let shape = t.s();
            let mode = match t.s().as_str() {
                "front" => ArrangeMode::Front,
                "back" => ArrangeMode::Back,
                "forward" => ArrangeMode::Forward,
                _ => ArrangeMode::Backward,
            };
            Cmd::Op(Op::ArrangeShape { shape, mode })
        }
        "deleteShape" => Cmd::Op(Op::DeleteShape { shape: t.s() }),
        "copy" => Cmd::Copy(t.s()),
        "paste" => Cmd::Paste,
        "xpaste" => Cmd::XPaste(t.s(), t.i(), t.s()),
        "notes" => Cmd::Op(Op::SetSlideNotes { i: t.i(), text: t.s() }),
        "properties" => Cmd::Op(Op::SetProperties(DocProperties {
            title: t.opt_s(),
            subject: t.opt_s(),
            author: t.opt_s(),
            keywords: t.opt_s(),
            description: t.opt_s(),
            last_modified_by: t.opt_s(),
            ..DocProperties::default()
        })),
        "replace" => {
            let (query, replacement, match_case) = (t.s(), t.s(), t.b());
            let hit = match (t.opt_n(), t.opt_s()) {
                (Some(s), Some(id)) => Some((s as i64, id)),
                _ => None,
            };
            Cmd::Op(Op::ReplaceText { query, replacement, match_case, hit })
        }
        "select" => Cmd::Select(t.opt_s()),
        "goto" => Cmd::Goto(t.i() as u32),
        "undo" => Cmd::Undo,
        "redo" => Cmd::Redo,
        "markClean" => Cmd::MarkClean,
        "embed" => Cmd::Embed(t.opt_b(), t.opt_b()),
        "saveFonts" => Cmd::SaveFonts,
        other => panic!("unknown command {other}"),
    }
}

// ───────────────────────── facets ─────────────────────────

/// `<count>:<sha256[0..32]>` over the parts in order (name, length, SHA-256 of the bytes).
pub fn parts_digest(store: &PartStore) -> String {
    let mut h = uo_testkit::sha256::Sha256::new();
    for e in store.entries() {
        h.update(format!("{}\0{}\0", e.name, e.bytes.len()).as_bytes());
        h.update(&uo_testkit::sha256::sha256(&e.bytes));
    }
    format!("{}:{}", store.len(), &uo_dml_render::canon::hex(&h.finish())[..32])
}

pub fn part_map(store: &PartStore) -> Vec<(String, String)> {
    store
        .entries()
        .iter()
        .map(|e| {
            (e.name.to_string(), format!("{}:{}", e.bytes.len(), &uo_dml_render::canon::hex(&uo_testkit::sha256::sha256(&e.bytes))[..16]))
        })
        .collect()
}

/// The `st` facet: `i=…;sel=…;u=…;r=…;ul=…;rl=…;d=…`.
pub fn state_text(index: u32, sel: Option<&str>, u: bool, r: bool, ul: Option<&str>, rl: Option<&str>, d: bool) -> String {
    format!(
        "i={index};sel={};u={};r={};ul={};rl={};d={}",
        sel.unwrap_or("-"),
        i32::from(u),
        i32::from(r),
        ul.unwrap_or("-"),
        rl.unwrap_or("-"),
        i32::from(d)
    )
}

pub fn state_facet(e: &Editor) -> String {
    let h = e.history();
    state_text(e.index(), e.selected(), h.can_undo(), h.can_redo(), h.undo_label(), h.redo_label(), e.is_dirty())
}

/// The `pres` and `cur` facets of an editor (its current slide).
pub fn model_facets(e: &mut Editor) -> (String, String) {
    let pres = match e.deck_mut().presentation() {
        Ok(p) => p.clone(),
        Err(_) => return ("err".into(), "err".into()),
    };
    let pres_digest = text_digest(&presentation_json(&pres).to_text());
    let idx = i64::from(e.index());
    let n = i64::try_from(e.slide_count()).unwrap();
    if idx >= n {
        return (pres_digest, "-".into());
    }
    let Some(model) = e.slide_model(idx) else { return (pres_digest, "err".into()) };
    let notes = e.slide_notes(idx).unwrap_or_default();
    let src = e.deck().source();
    let text = slide_record_with_timing(&Canon { src: &src }, &notes, &model);
    (pres_digest, text_digest(&text))
}

/// The `pres` and `cur` facets of a package read fresh at slide `index` (what a shell that mirrors the package reads).
pub fn store_model_facets(store: &PartStore, index: u32) -> (String, String) {
    let mut e = Editor::new(Deck::from_store(store.clone(), Limits::default()));
    e.set_index(index);
    model_facets(&mut e)
}

pub fn ret_facet(o: &Outcome) -> String {
    match o {
        Outcome::Nothing => "none".into(),
        Outcome::Bool(b) => format!("b{}", i32::from(*b)),
        Outcome::Index(i) => format!("n{i}"),
        Outcome::Shape(Some(id)) => format!("s{id}"),
        Outcome::Shape(None) => "s".into(),
        Outcome::Count(c) => format!("n{}", to_js_string(*c)),
    }
}

/// The open font files the font captures embed (the host's answers to the font requests).
pub fn face_bytes(family: &str, variant: uo_fonts::Variant) -> Option<Arc<[u8]>> {
    let base = match family.to_ascii_lowercase().as_str() {
        "caladea" => "caladea",
        "liberation serif" => "liberationserif",
        "liberation mono" => "liberationmono",
        "carlito" => "carlito",
        _ => return None,
    };
    let v = match variant {
        uo_fonts::Variant::Regular => "regular",
        uo_fonts::Variant::Bold => "bold",
        uo_fonts::Variant::Italic => "italic",
        uo_fonts::Variant::BoldItalic => "bolditalic",
    };
    let p = repo_root().join("shared/fonts/files").join(format!("{base}-{v}.ttf"));
    Some(Arc::from(std::fs::read(p).unwrap()))
}

pub fn deck_bytes(key: &str) -> Vec<u8> {
    if let Some(rest) = key.strip_prefix("gen/") {
        let (group, n) = rest.split_once('/').unwrap();
        return std::fs::read(captures().join("decks").join(group).join(format!("{n}.pptx"))).unwrap();
    }
    std::fs::read(repo_root().join("slides/samples").join(key.strip_prefix("samples/").unwrap())).unwrap()
}

fn opts_of(init: &[&str]) -> BlankDeckOptions {
    let lang = pct(init[1]);
    let mut t = Toks(init[2..].iter());
    let wide = t.opt_b().unwrap_or(true);
    let title = t.opt_s();
    BlankDeckOptions { wide, title, lang, now: "2026-01-02T03:04:00Z".into() }
}

// ───────────────────────── the driver ─────────────────────────

/// How a script starts.
pub enum Init<'a> {
    /// an existing deck and the language of new text
    Deck(&'a [u8], String),
    Blank(BlankDeckOptions),
}

/// One editing session under replay.
pub trait Driver {
    fn init(&mut self, how: Init<'_>);
    /// an operation: its `ret` facet (`err:<message>` when it failed)
    fn apply(&mut self, op: Op) -> String;
    /// copies a shape of the current slide into the driver's clipboard: `b1`/`b0`
    fn copy(&mut self, shape: &str) -> String;
    /// pastes the clipboard (`none` when empty)
    fn paste(&mut self) -> String;
    /// copies shape `shape` of slide `slide` of another deck and pastes it
    fn xpaste(&mut self, deck: &[u8], slide: u32, shape: &str) -> String;
    fn select(&mut self, id: Option<&str>);
    fn goto(&mut self, i: u32);
    fn undo(&mut self);
    fn redo(&mut self);
    fn mark_clean(&mut self);
    fn embed(&mut self, embed: Option<bool>, subset: Option<bool>);
    /// the bytes of the save with fonts (answered with [`face_bytes`])
    fn save_fonts(&mut self) -> Vec<u8>;
    /// the package as the driver sees it
    fn parts(&self) -> &PartStore;
    fn state(&mut self) -> String;
    fn models(&mut self) -> (String, String);
}

/// The driver of `uo_pptx::edit::Editor` itself.
#[derive(Default)]
pub struct EditorDriver {
    ed: Option<Editor>,
    clip: Option<ShapeClip>,
}

impl EditorDriver {
    fn ed(&mut self) -> &mut Editor {
        self.ed.as_mut().unwrap()
    }
    fn paste_clip(&mut self, c: ShapeClip) -> String {
        match self.ed().apply(Op::PasteShape { clip: c }) {
            Ok(o) => ret_facet(&o),
            Err(e) => format!("err:{e}"),
        }
    }
}

impl Driver for EditorDriver {
    fn init(&mut self, how: Init<'_>) {
        self.clip = None;
        let mut ed = match how {
            Init::Blank(o) => Editor::blank(&o, Limits::default()),
            Init::Deck(bytes, lang) => {
                let mut e = Editor::open(bytes, Limits::default()).unwrap();
                e.set_text_lang(&lang);
                e
            }
        };
        ed.set_random_seed(1);
        self.ed = Some(ed);
    }
    fn apply(&mut self, op: Op) -> String {
        match self.ed().apply(op) {
            Ok(o) => ret_facet(&o),
            Err(e) => format!("err:{e}"),
        }
    }
    fn copy(&mut self, shape: &str) -> String {
        match self.ed().copy_shape(shape) {
            Ok(c) => {
                let ok = c.is_some();
                self.clip = c;
                format!("b{}", i32::from(ok))
            }
            Err(e) => format!("err:{e}"),
        }
    }
    fn paste(&mut self) -> String {
        match self.clip.clone() {
            None => "none".into(),
            Some(c) => self.paste_clip(c),
        }
    }
    fn xpaste(&mut self, deck: &[u8], slide: u32, shape: &str) -> String {
        let mut src = Editor::open(deck, Limits::default()).unwrap();
        src.set_index(slide);
        match src.copy_shape(shape).unwrap() {
            None => "none".into(),
            Some(c) => {
                self.clip = Some(c.clone());
                self.paste_clip(c)
            }
        }
    }
    fn select(&mut self, id: Option<&str>) {
        self.ed().select(id);
    }
    fn goto(&mut self, i: u32) {
        self.ed().set_index(i);
    }
    fn undo(&mut self) {
        self.ed().undo();
    }
    fn redo(&mut self) {
        self.ed().redo();
    }
    fn mark_clean(&mut self) {
        self.ed().mark_clean();
    }
    fn embed(&mut self, embed: Option<bool>, subset: Option<bool>) {
        self.ed().set_font_embedding(embed, subset);
    }
    fn save_fonts(&mut self) -> Vec<u8> {
        self.ed().save_with_fonts(&|r| face_bytes(&r.family, r.variant)).unwrap().bytes
    }
    fn parts(&self) -> &PartStore {
        self.ed.as_ref().unwrap().deck().parts()
    }
    fn state(&mut self) -> String {
        state_facet(self.ed())
    }
    fn models(&mut self) -> (String, String) {
        model_facets(self.ed())
    }
}

// ───────────────────────── the replay ─────────────────────────

fn dump_diff(
    key: &str,
    step: usize,
    now: &[(String, String)],
    before: &[(String, String)],
    ts: &std::collections::HashMap<String, Vec<(String, String)>>,
) {
    let want = std::env::var("UO_EDIT_DUMP").ok();
    if want.as_deref() != Some(&format!("{key}#{step}")) {
        return;
    }
    eprintln!("--- {key}#{step}: parts changed by the Rust step");
    for (n, d) in now {
        if before.iter().find(|(m, _)| m == n).is_none_or(|(_, o)| o != d) {
            eprintln!("rust  {n}  {d}");
        }
    }
    for (n, _) in before {
        if !now.iter().any(|(m, _)| m == n) {
            eprintln!("rust  {n}  -");
        }
    }
    for (n, d) in ts.get(&format!("{key}#{step}")).into_iter().flatten() {
        eprintln!("ts    {n}  {d}");
    }
}

/// Replays one capture over `d`; returns (scripts, steps, the report). `max_scripts` bounds the scripts replayed
/// (`usize::MAX` = all).
pub fn replay(stem: &str, d: &mut dyn Driver, max_scripts: usize) -> (usize, usize, uo_testkit::Report) {
    let cap = Capture::read(captures().join(format!("{stem}.capture"))).unwrap();
    let ops = gunzip(&std::fs::read(captures().join(format!("{stem}.ops.gz"))).unwrap());
    let parts_ts = gunzip(&std::fs::read(captures().join(format!("{stem}.parts.gz"))).unwrap());
    let mut ts_parts: std::collections::HashMap<String, Vec<(String, String)>> = std::collections::HashMap::new();
    for l in parts_ts.lines() {
        let f: Vec<&str> = l.split('\t').collect();
        if f.len() == 3 {
            ts_parts.entry(f[0].to_owned()).or_default().push((f[1].to_owned(), f[2].to_owned()));
        }
    }
    let mut rp = Replay::new(&cap);
    // the blank decks themselves: byte-equal parts
    for rec in cap.records() {
        let Some(i) = rec.key.strip_prefix("blank/").and_then(|r| r.parse::<usize>().ok()) else { continue };
        let variants = [
            BlankDeckOptions { now: "2026-01-02T03:04:00Z".into(), ..BlankDeckOptions::default() },
            BlankDeckOptions { wide: false, now: "2026-01-02T03:04:00Z".into(), ..BlankDeckOptions::default() },
            BlankDeckOptions {
                title: Some("Quarterly <review> & plan".into()),
                lang: "pt-BR".into(),
                now: "2026-01-02T03:04:00Z".into(),
                ..BlankDeckOptions::default()
            },
            BlankDeckOptions { wide: false, title: Some("x".into()), lang: "es-ES".into(), now: "2026-01-02T03:04:00Z".into() },
        ];
        let store = PartStore::from_parts(blank_deck_parts(&variants[i]));
        rp.check(&rec.key, "parts", &parts_digest(&store));
        for e in store.entries() {
            rp.check(&rec.key, &format!("p:{}", e.name), &uo_pptx::canon::bytes_digest(&e.bytes));
        }
    }
    let mut scripts = 0;
    let mut steps = 0;
    let mut before: Vec<(String, String)> = Vec::new();
    let mut base = String::new();
    let mut skipping = false;
    for line in ops.lines() {
        let parts: Vec<&str> = line.split('\t').collect();
        let (key, step) = parts[0].rsplit_once('#').unwrap();
        let step: usize = step.parse().unwrap();
        let facet_key = format!("{key}#{step}");
        let cmd_name = parts[1];
        if cmd_name == "init" {
            if scripts >= max_scripts {
                skipping = true;
                continue;
            }
            skipping = false;
            scripts += 1;
            base = key.to_owned();
            if key.starts_with("blank/") {
                d.init(Init::Blank(opts_of(&parts[1..])));
            } else {
                let bytes = deck_bytes(key);
                d.init(Init::Deck(&bytes, pct(parts[2])));
            }
            before = part_map(d.parts());
            rp.check(&facet_key, "op", "init");
            rp.check(&facet_key, "ret", "none");
            rp.check(&facet_key, "parts", &parts_digest(d.parts()));
            let st = d.state();
            rp.check(&facet_key, "st", &st);
            let (p, c) = d.models();
            rp.check(&facet_key, "pres", &p);
            rp.check(&facet_key, "cur", &c);
            continue;
        }
        if skipping {
            continue;
        }
        assert_eq!(key, base);
        steps += 1;
        let ret = match parse_cmd(&parts[1..]) {
            Cmd::Op(op) => d.apply(op),
            Cmd::Copy(shape) => d.copy(&shape),
            Cmd::Paste => d.paste(),
            Cmd::XPaste(deck, slide, shape) => d.xpaste(&deck_bytes(&deck), u32::try_from(slide).unwrap(), &shape),
            Cmd::Select(id) => {
                d.select(id.as_deref());
                "none".into()
            }
            Cmd::Goto(i) => {
                d.goto(i);
                "none".into()
            }
            Cmd::Undo => {
                d.undo();
                "none".into()
            }
            Cmd::Redo => {
                d.redo();
                "none".into()
            }
            Cmd::MarkClean => {
                d.mark_clean();
                "none".into()
            }
            Cmd::Embed(embed, subset) => {
                d.embed(embed, subset);
                "none".into()
            }
            Cmd::SaveFonts => {
                let bytes = d.save_fonts();
                let saved = Deck::open(&bytes, Limits::default()).unwrap();
                format!("s{}", parts_digest(saved.parts()))
            }
        };
        rp.check(&facet_key, "op", cmd_name);
        rp.check(&facet_key, "ret", &ret);
        let digest = parts_digest(d.parts());
        let now = part_map(d.parts());
        if cap.get(&facet_key, "parts") != Some(digest.as_str()) {
            dump_diff(key, step, &now, &before, &ts_parts);
            if std::env::var("UO_EDIT_DUMP").is_err() && std::env::var("UO_EDIT_FIRST").is_ok() {
                eprintln!("first parts mismatch at {facet_key} ({cmd_name})");
                for (n, dg) in &now {
                    if before.iter().find(|(m, _)| m == n).is_none_or(|(_, o)| o != dg) {
                        eprintln!("  rust {n} {dg}");
                    }
                }
                for (n, dg) in ts_parts.get(&facet_key).into_iter().flatten() {
                    eprintln!("  ts   {n} {dg}");
                }
            }
        }
        rp.check(&facet_key, "parts", &digest);
        before = now;
        let st = d.state();
        rp.check(&facet_key, "st", &st);
        let (p, c) = d.models();
        rp.check(&facet_key, "pres", &p);
        rp.check(&facet_key, "cur", &c);
    }
    (scripts, steps, rp.finish())
}
