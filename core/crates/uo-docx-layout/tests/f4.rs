//! Frontier F4 (record "Stage 3 — F4"): memory (M1, M5) and robustness of the paragraph
//! preparation, the hyphenation marks and the equation layout. Faces are the repository's open
//! fonts behind a small provider (Carlito for Calibri, Caladea for Cambria, Liberation for the
//! rest); the documents are built in code or read from the committed F4 fixture.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::indexing_slicing)] // test code

mod common;

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use uo_alloc::assert_no_leak;
use uo_docx::DocView;
use uo_docx::model::Document;
use uo_docx::model::effects::OpenTypeProps;
use uo_docx::model::props::{IndSpec, Lang, NumPr, SpacingSpec, TabSpec, TextFill};
use uo_docx::model::run::{CustomInline, FormField, MathNode, MathZone, NoteRef, RunExt, RunProps};
use uo_docx::model::story::Story;
use uo_docx::read::{ReadOptions, read_document};
use uo_docx::text::Utf16;
use uo_docx::view::{BlockPath, RunRef, RunSub, StoryRef};
use uo_docx_layout::context::DocContext;
use uo_docx_layout::engine::{Cx, PassInput};
use uo_docx_layout::error::{LResult, LayoutError, LimitKind};
use uo_docx_layout::fonts::{FaceResolverCache, FaceTable, FontAnswer, FontProvider};
use uo_docx_layout::hyphenate::HyphenSessions;
use uo_docx_layout::keys::{LParaId, RunKey};
use uo_docx_layout::ldoc::{LDoc, LPara, ParaFlags, ParaSrc, RunSrc};
use uo_docx_layout::model::{JsNumber, PPr, Paragraph, RPr, Run};
use uo_docx_layout::options::{Budget, Hyphenation, LayoutOptions};
use uo_docx_layout::para::{FieldSource, GlyphFlags, ParaEnv, PrepFlow, para_visibility, prepare_paragraph};
use uo_docx_layout::word;
use uo_fonts::FontLimits;
use uo_fonts::registry::{AnswerParts, FaceCache, FontBudget};
use uo_render::Resources;

// ───────────────────────── fonts and flow ─────────────────────────

/// The open fonts by family (one answer object per request key, as a caching provider).
struct OpenFonts {
    cache: Arc<FaceCache>,
    files: HashMap<String, Arc<[u8]>>,
    answers: Mutex<HashMap<(String, bool, bool), Arc<FontAnswer>>>,
}

impl OpenFonts {
    fn new() -> OpenFonts {
        let mut files = HashMap::new();
        for dir in ["shared/fonts/files", "site/assets/fonts"] {
            for e in std::fs::read_dir(common::repo().join(dir)).unwrap().flatten() {
                let p = e.path();
                if p.extension().is_some_and(|x| x == "ttf") {
                    let name = p.file_stem().unwrap().to_string_lossy().into_owned();
                    files.insert(name, Arc::from(std::fs::read(&p).unwrap()));
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
        if let Some(a) = self.answers.lock().unwrap().get(&k) {
            return Some(Arc::clone(a));
        }
        let base = match fam.as_str() {
            "calibri" => "carlito",
            "cambria" | "cambria math" => "caladea",
            "times new roman" | "liberation serif" => "liberationserif",
            "courier new" | "liberation mono" => "liberationmono",
            "nothing at all" => return None,
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
        self.answers.lock().unwrap().insert(k, Arc::clone(&a));
        Some(a)
    }
}

/// The engine-like flow: a field answers its name in brackets ("PAGE" → "7"), the numbering and
/// the note marks are the real context's.
struct Flow {
    ctx: DocContext,
}

impl PrepFlow for Flow {
    fn ctx(&mut self) -> &mut DocContext {
        &mut self.ctx
    }
    fn field(&mut self, _cx: &mut Cx<'_, '_>, _src: FieldSource, name: &[u16], _instr: &[u16], _at: f64) -> LResult<Option<Box<[u16]>>> {
        Ok(match String::from_utf16_lossy(name).as_str() {
            "PAGE" => Some(Utf16::from("7").units().into()),
            "NUMPAGES" => Some(Utf16::from("12").units().into()),
            "REF" => None,
            _ => Some(Utf16::from("x y").units().into()),
        })
    }
}

fn hyph_registry() -> Arc<uo_fonts::hyphen::HyphenationRegistry> {
    let mut reg = uo_fonts::hyphen::HyphenationRegistry::new();
    for id in uo_fonts::hyphen::HYPHENATION_PATTERN_IDS {
        if let Some(p) = uo_fonts::embedded_patterns(id) {
            reg.register(&p);
        }
    }
    Arc::new(reg)
}

/// The body paragraphs of a document as model paragraphs of an LDoc (with field-result keys).
fn ldoc_of(doc: &Document) -> (LDoc<'_>, Vec<LParaId>) {
    let mut l = LDoc::empty(DocView::new(doc));
    let mut ids = Vec::new();
    for (pi, p) in doc.body.paras.iter().enumerate() {
        let path = BlockPath::root(u32::try_from(pi).unwrap());
        let mut runs = Vec::new();
        for (i, r) in p.runs.iter().enumerate() {
            let key = RunKey(u32::try_from(l.runs.len()).unwrap());
            let at = RunRef { story: StoryRef::Body, path: path.clone(), run: u32::try_from(i).unwrap(), sub: RunSub::None };
            l.runs.push(RunSrc::Model { run: r, at });
            runs.push(key);
            for (j, x) in r.ext().and_then(|e| e.field_result.as_deref()).unwrap_or(&[]).iter().enumerate() {
                let j = u32::try_from(j).unwrap();
                let k2 = RunKey(u32::try_from(l.runs.len()).unwrap());
                let at = RunRef { story: StoryRef::Body, path: path.clone(), run: u32::try_from(i).unwrap(), sub: RunSub::FieldResult(j) };
                l.runs.push(RunSrc::Model { run: x, at });
                l.field_results.insert((key, j), k2);
            }
        }
        ids.push(LParaId(u32::try_from(l.paras.len()).unwrap()));
        l.paras.push(LPara {
            src: ParaSrc::Model { para: p, id: p.id },
            ppr: None,
            props: None,
            runs,
            rev_joins: Vec::new(),
            flags: ParaFlags::default(),
            frame_carry: None,
            story: StoryRef::Body,
            path,
        });
    }
    (l, ids)
}

/// Prepares every body paragraph with a fresh per-call state (faces, resources, sessions); the
/// results and errors are returned (or dropped by the caller).
fn prepare_all(
    doc: &Document,
    provider: &dyn FontProvider,
    reg: &Arc<uo_fonts::hyphen::HyphenationRegistry>,
    env: &ParaEnv,
    opts: &LayoutOptions,
) -> (usize, Vec<LayoutError>) {
    let (ldoc, ids) = ldoc_of(doc);
    let input = PassInput::default();
    let mut fonts = FaceTable::new();
    let mut resolver = FaceResolverCache::new();
    let mut res = Resources::new();
    let mut hyph = HyphenSessions::new(Arc::clone(reg));
    let mut budget = Budget::unlimited();
    let mut seq = 0u32;
    let mut flow = Flow {
        ctx: DocContext::new(word::StyleResolver::new(), word::numbering_state(&opts.lang, &opts.limits), word::revision_display(None)),
    };
    let mut ok = 0;
    let mut errs = Vec::new();
    for id in ids {
        let mut cx = Cx {
            ldoc: &ldoc,
            input: &input,
            opts,
            fonts: &mut fonts,
            provider,
            resolver: &mut resolver,
            res: &mut res,
            hyph: &mut hyph,
            budget: &mut budget,
            depth: 0,
            line_seq: &mut seq,
            images: &mut uo_docx_layout::engine::objects::ImageTable::new(),
        };
        match prepare_paragraph(&mut cx, &mut flow, id, env) {
            Ok(p) => {
                ok += 1;
                for g in p.glyphs.iter().chain(p.marker.iter().flat_map(|m| m.glyphs.iter())) {
                    assert!((g.style.0 as usize) < p.styles.len());
                    assert!(cx.fonts.entry(g.face).is_some());
                    if let Some(m) = g.math {
                        assert!((m as usize) < p.math.len());
                    }
                }
            }
            Err(e) => errs.push(e),
        }
        let _ = para_visibility(&mut cx, &mut flow, id, env);
    }
    (ok, errs)
}

fn fixture_doc() -> Document {
    let dir = common::manifest().join("tests/captures/f4/docs");
    let p = std::fs::read_dir(&dir).unwrap().flatten().map(|e| e.path()).find(|p| p.extension().is_some_and(|x| x == "docx")).unwrap();
    let bytes = std::fs::read(p).unwrap();
    let pkg = uo_opc::Package::open(&bytes, &uo_opc::Limits::DEFAULT, &uo_opc::package::OpenOptions::default()).unwrap();
    read_document(&pkg, &ReadOptions::default()).unwrap()
}

// ───────────────────────── model builders ─────────────────────────

fn run(text: &str, rpr: RPr) -> Run {
    Run { text: Utf16::from(text), props: Arc::new(RunProps::default()), rpr: Some(Arc::new(rpr)), ..Run::default() }
}

fn with_ext(mut r: Run, ext: RunExt) -> Run {
    r.ext = Some(Box::new(ext));
    r
}

fn para(runs: Vec<Run>, ppr: PPr) -> Paragraph {
    Paragraph { runs, ppr: Some(Arc::new(ppr)), ..Paragraph::default() }
}

fn mnode(t: &str, text: Option<&str>) -> MathNode {
    MathNode { t: t.into(), text: text.map(Utf16::from), ..MathNode::default() }
}

fn frac(num: Vec<MathNode>, den: Vec<MathNode>) -> MathNode {
    MathNode { t: "f".into(), num: Some(num), den: Some(den), ..MathNode::default() }
}

/// A document of every path of `prepareParagraph`: small caps and caps, kerning, ligatures and
/// OpenType sets, superscript, text effects, a footnote reference, fields (PAGE, a formula, a
/// result), a drop-down, a check box, a custom inline, a `w:ptab`, an equation with a fraction,
/// a synthetic list marker, tabs, hyphenated Portuguese, CJK kinsoku, NBSP glue.
fn rich_doc(base: &Document) -> Document {
    let mut d = base.clone();
    {
        let f = Arc::make_mut(&mut d.formatting);
        f.settings.auto_hyphenation = true;
    }
    let kern = RPr { kern: Some(JsNumber::Num(2.0)), sz: Some(JsNumber::Num(22.0)), ..RPr::default() };
    let sc = RPr { small_caps: Some(true), kern: Some(JsNumber::Num(2.0)), ..RPr::default() };
    let caps = RPr { caps: Some(true), ..RPr::default() };
    let lig = RPr {
        ligatures: Some("all".into()),
        ot: Some(Box::new(OpenTypeProps {
            num_form: Some("oldStyle".into()),
            stylistic_sets: Some(vec![JsNumber::Num(1.0), JsNumber::Num(f64::NAN)]),
            ..OpenTypeProps::default()
        })),
        ..RPr::default()
    };
    let sup = RPr { vert_align: Some("superscript".into()), ..RPr::default() };
    let tf = RPr {
        text_fill: Some(Box::new(TextFill { color: Some("FF0000".into()), alpha: Some(JsNumber::Num(0.5)), ..TextFill::default() })),
        ..RPr::default()
    };
    let pt = RPr { lang: Some(Box::new(Lang { val: Some("pt-BR".into()), ..Lang::default() })), ..RPr::default() };
    let ja = RPr { lang: Some(Box::new(Lang { east_asia: Some("ja-JP".into()), ..Lang::default() })), ..RPr::default() };
    let mut story = Story::new();
    story.push_paragraph(para(
        vec![
            run("Kerning AVATAR To ", kern),
            run("Small Caps straße ", sc),
            run("caps ß ", caps),
            run("office fi 1234 ", lig),
            run("2", sup),
            run("transparente ", tf),
        ],
        PPr::default(),
    ));
    story.push_paragraph(para(
        vec![
            run("anticonstitucionalissimamente paralelepípedo otorrinolaringologista inconstitucionalidade ", pt.clone()),
            run("e\u{301}cole\u{a0} \u{a0}x\u{2011}y\u{ad}z \u{feff} ", pt),
            run("$ 100 「日本語」、テスト。", ja),
        ],
        PPr {
            tabs: Some(vec![
                TabSpec { val: "right".into(), pos: JsNumber::Num(9000.0), leader: Some("dot".into()), ..TabSpec::default() },
                TabSpec { val: "clear".into(), pos: JsNumber::Num(720.0), leader: None, ..TabSpec::default() },
            ]),
            ..PPr::default()
        },
    ));
    story.push_paragraph(para(
        vec![
            with_ext(run("1", RPr::default()), RunExt { field: Some(" PAGE ".into()), ..RunExt::default() }),
            with_ext(
                run("9", RPr::default()),
                RunExt { field: Some("= 2 + 3".into()), field_code: Some(" = {NUMPAGES} - 1 ".into()), ..RunExt::default() },
            ),
            with_ext(
                run("", RPr::default()),
                RunExt {
                    field: Some("TOC".into()),
                    field_result: Some(vec![
                        run("result text", RPr::default()),
                        with_ext(run("3", RPr::default()), RunExt { field: Some("PAGEREF x".into()), ..RunExt::default() }),
                    ]),
                    ..RunExt::default()
                },
            ),
            with_ext(
                run("", RPr::default()),
                RunExt {
                    form_field: Some(Box::new(FormField {
                        r#type: "dropDown".into(),
                        entries: Some(vec![Utf16::from("one"), Utf16::from("two")]),
                        selected: Some(JsNumber::Num(1.0)),
                        ..FormField::default()
                    })),
                    ..RunExt::default()
                },
            ),
            with_ext(
                run("", RPr::default()),
                RunExt {
                    form_field: Some(Box::new(FormField { r#type: "checkBox".into(), checked: Some(true), ..FormField::default() })),
                    ..RunExt::default()
                },
            ),
            with_ext(
                run("\u{fffc}", RPr::default()),
                RunExt {
                    custom: Some(Box::new(CustomInline {
                        r#type: "mention".into(),
                        fallback_text: Utf16::from("@a b"),
                        ..CustomInline::default()
                    })),
                    ..RunExt::default()
                },
            ),
            with_ext(
                run("1", RPr::default()),
                RunExt {
                    note_ref: Some(NoteRef { kind: "footnote".into(), id: JsNumber::Num(3.0), custom: None, ..NoteRef::default() }),
                    ..RunExt::default()
                },
            ),
            with_ext(
                run("", RPr::default()),
                RunExt {
                    ptab: Some(uo_docx::model::run::Ptab {
                        alignment: "right".into(),
                        relative_to: "margin".into(),
                        leader: Some("dot".into()),
                        ..uo_docx::model::run::Ptab::default()
                    }),
                    ..RunExt::default()
                },
            ),
            with_ext(run("\u{f0b7}\u{2192}\u{2200}", RPr::default()), RunExt { sym_font: Some("Symbol".into()), ..RunExt::default() }),
        ],
        PPr::default(),
    ));
    let zone = MathZone {
        jc: None,
        nodes: vec![
            mnode("r", Some("x=a+")),
            frac(vec![mnode("r", Some("−b±c"))], vec![mnode("r", Some("2a"))]),
            mnode("br", None),
            MathNode { t: "r".into(), text: Some(Utf16::from("y")), sty: Some("b".into()), ..MathNode::default() },
        ],
        ..MathZone::default()
    };
    story.push_paragraph(para(
        vec![with_ext(run("xa+b2ay", RPr::default()), RunExt { math: Some(Box::new(zone)), ..RunExt::default() })],
        PPr::default(),
    ));
    story.push_paragraph(para(
        vec![run("item", RPr::default())],
        PPr {
            num_pr: Some(Box::new(NumPr { num_id: Some(JsNumber::Num(1001.0)), ilvl: Some(JsNumber::Num(1.0)), ..NumPr::default() })),
            ind: Some(Box::new(IndSpec { left: Some(JsNumber::Num(720.0)), hanging: Some(JsNumber::Null), ..IndSpec::default() })),
            spacing: Some(Box::new(SpacingSpec {
                line_rule: Some("exact".into()),
                line: Some(JsNumber::Num(300.0)),
                ..SpacingSpec::default()
            })),
            ..PPr::default()
        },
    ));
    d.body = story;
    d
}

fn opts() -> LayoutOptions {
    LayoutOptions {
        hyphenation_loaded: uo_fonts::hyphen::HYPHENATION_PATTERN_IDS.iter().map(|s| Box::<str>::from(*s)).collect(),
        ..LayoutOptions::default()
    }
}

// ───────────────────────── M1, M5 ─────────────────────────

#[test]
fn m1_prepare_fixture_and_every_path_return_to_zero() {
    let fonts = OpenFonts::new();
    let reg = hyph_registry();
    let base = fixture_doc();
    let rich = rich_doc(&base);
    let o = opts();
    let env = ParaEnv { field: Some(FieldSource::Page), ..ParaEnv::default() };
    // warm-up: the provider's answers and the parsed faces are made once (they outlive a layout)
    let (n0, e0) = prepare_all(&base, &fonts, &reg, &env, &o);
    let (n1, e1) = prepare_all(&rich, &fonts, &reg, &env, &o);
    assert!(n0 > 10 && e0.is_empty(), "{n0} {e0:?}");
    assert_eq!((n1, e1.len()), (5, 0), "{e1:?}");
    let u = assert_no_leak(|| prepare_all(&base, &fonts, &reg, &env, &o));
    eprintln!("M1 prepare (fixture, {n0} paragraphs): {u:?}");
    let u = assert_no_leak(|| prepare_all(&rich, &fonts, &reg, &env, &o));
    eprintln!("M1 prepare (every path): {u:?}");
    let tenv = ParaEnv { hf: true, dark_background: true, no_page_breaks: true, numbering: Some(false), continuation: true, ..env.clone() };
    let u = assert_no_leak(|| prepare_all(&rich, &fonts, &reg, &tenv, &o));
    eprintln!("M1 prepare (header env): {u:?}");
}

#[test]
fn hyphenation_marks_and_sessions() {
    let fonts = OpenFonts::new();
    let reg = hyph_registry();
    let rich = rich_doc(&fixture_doc());
    let (ldoc, ids) = ldoc_of(&rich);
    let o = opts();
    let input = PassInput::default();
    let (mut ft, mut rs, mut res) = (FaceTable::new(), FaceResolverCache::new(), Resources::new());
    let mut hyph = HyphenSessions::new(Arc::clone(&reg));
    let (mut budget, mut seq) = (Budget::unlimited(), 0u32);
    let mut flow =
        Flow { ctx: DocContext::new(word::StyleResolver::new(), word::numbering_state("pt-BR", &o.limits), word::revision_display(None)) };
    let mut cx = Cx {
        ldoc: &ldoc,
        input: &input,
        opts: &o,
        fonts: &mut ft,
        provider: &fonts,
        resolver: &mut rs,
        res: &mut res,
        hyph: &mut hyph,
        budget: &mut budget,
        depth: 0,
        line_seq: &mut seq,
        images: &mut uo_docx_layout::engine::objects::ImageTable::new(),
    };
    let p = prepare_paragraph(&mut cx, &mut flow, ids[1], &ParaEnv::default()).unwrap();
    let marks = p.glyphs.iter().filter(|g| g.flags.has(GlyphFlags::HYPH)).count();
    assert!(marks >= 8, "Portuguese words get their points: {marks}");
    // one session per pattern set used (M5: the word cache is the uo-fonts session's, capped)
    assert_eq!(cx.hyph.sessions.len(), 1);
    // hyphenation off, or a language not loaded: no marks
    let mut o2 = o.clone();
    o2.hyphenation = Hyphenation::Off;
    let mut cx2 = Cx { opts: &o2, ..cx };
    let p2 = prepare_paragraph(&mut cx2, &mut flow, ids[1], &ParaEnv::default()).unwrap();
    assert!(p2.glyphs.iter().all(|g| !g.flags.has(GlyphFlags::HYPH)));
}

// ───────────────────────── robustness ─────────────────────────

/// A small deterministic generator.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }
    fn pick<T: Clone>(&mut self, xs: &[T]) -> T {
        xs[(self.next() % xs.len() as u64) as usize].clone()
    }
    fn num(&mut self) -> JsNumber {
        self.pick(&[
            JsNumber::Num(f64::NAN),
            JsNumber::Num(f64::INFINITY),
            JsNumber::Num(-1.0),
            JsNumber::Num(0.0),
            JsNumber::Num(-0.0),
            JsNumber::Num(1e300),
            JsNumber::Num(2.5),
            JsNumber::Num(22.0),
            JsNumber::Num(100.0),
            JsNumber::Null,
        ])
    }
    fn text(&mut self) -> Utf16 {
        let atoms: [&[u16]; 18] = [
            &[0x61, 0x62],
            &[0xd800],
            &[0xdc00, 0x61],
            &[0xf0b7],
            &[0x09],
            &[0x0c],
            &[0x0b],
            &[0x2028],
            &[0xa0, 0x20],
            &[0xad],
            &[0x2011],
            &[0x65, 0x301],
            &[0x65e5, 0x672c],
            &[0xdf],
            &[0xfeff],
            &[0x2d, 0x61],
            &[0x5c],
            &[0xd83d, 0xde00],
        ];
        let n = self.next() % 12;
        let mut v = Vec::new();
        for _ in 0..n {
            v.extend_from_slice(self.pick(&atoms));
        }
        Utf16::from_units(v)
    }
}

fn hostile_rpr(r: &mut Rng) -> RPr {
    RPr {
        sz: Some(r.num()),
        kern: Some(r.num()),
        w: Some(r.num()),
        spacing: Some(r.num()),
        position: Some(r.num()),
        caps: Some(r.next().is_multiple_of(4)),
        small_caps: Some(r.next().is_multiple_of(3)),
        vert_align: if r.next().is_multiple_of(4) { Some(r.pick(&["superscript", "subscript", "baseline"]).into()) } else { None },
        lang: Some(Box::new(Lang {
            val: Some(r.pick(&["pt-BR", "en-US", "xx", "", "constructor"]).into()),
            east_asia: Some(r.pick(&["ja", "zh-TW", "ko", ""]).into()),
            ..Lang::default()
        })),
        ligatures: Some(r.pick(&["all", "standard", "none", ""]).into()),
        ot: Some(Box::new(OpenTypeProps {
            stylistic_sets: Some(vec![r.num(), r.num()]),
            num_spacing: Some("tabular".into()),
            ..OpenTypeProps::default()
        })),
        text_fill: if r.next().is_multiple_of(5) {
            Some(Box::new(TextFill { alpha: Some(r.num()), theme_color: Some("accent1".into()), ..TextFill::default() }))
        } else {
            None
        },
        cs: Some(r.next().is_multiple_of(9)),
        ..RPr::default()
    }
}

fn hostile_doc(base: &Document, seed: u64) -> Document {
    let mut r = Rng(seed | 1);
    let mut d = base.clone();
    {
        let f = Arc::make_mut(&mut d.formatting);
        f.settings.auto_hyphenation = r.next().is_multiple_of(2);
        f.settings.do_not_hyphenate_caps = r.next().is_multiple_of(2);
        f.settings.compatibility_mode = r.num();
    }
    let mut story = Story::new();
    for _ in 0..6 {
        let mut runs = Vec::new();
        for _ in 0..(r.next() % 5) {
            let mut x = run("", hostile_rpr(&mut r));
            x.text = r.text();
            match r.next() % 12 {
                0 => {
                    x = with_ext(
                        x,
                        RunExt {
                            field: Some(r.pick(&["PAGE", "= (((1", "STYLEREF x", "=", "SEQ a", " "]).into()),
                            field_code: Some("{ = { PAGE } * 2 }".into()),
                            ..RunExt::default()
                        },
                    )
                }
                1 => {
                    // a deep chain of field results
                    let mut inner = run("z", RPr::default());
                    for _ in 0..40 {
                        inner = with_ext(
                            run("", RPr::default()),
                            RunExt { field: Some("TOC".into()), field_result: Some(vec![inner]), ..RunExt::default() },
                        );
                    }
                    x = inner;
                }
                2 => {
                    x = with_ext(
                        x,
                        RunExt {
                            note_ref: Some(NoteRef {
                                kind: r.pick(&["footnote", "endnote", "x"]).into(),
                                id: r.num(),
                                custom: Some(r.next().is_multiple_of(2)),
                                ..NoteRef::default()
                            }),
                            ..RunExt::default()
                        },
                    )
                }
                3 => {
                    let mut n = mnode("r", Some("x"));
                    for _ in 0..(r.next() % 300) {
                        n = frac(vec![n], vec![mnode("r", Some("1"))]);
                    }
                    let nodes = vec![n, mnode("r", None), MathNode { t: "f".into(), ..MathNode::default() }, mnode("br", None)];
                    x = with_ext(
                        x,
                        RunExt {
                            math: Some(Box::new(MathZone { jc: Some("left".into()), nodes, ..MathZone::default() })),
                            ..RunExt::default()
                        },
                    );
                }
                4 => {
                    x = with_ext(
                        x,
                        RunExt {
                            form_field: Some(Box::new(FormField {
                                r#type: "dropDown".into(),
                                entries: Some(vec![Utf16::from("a")]),
                                selected: Some(r.num()),
                                ..FormField::default()
                            })),
                            ..RunExt::default()
                        },
                    )
                }
                5 => {
                    x = with_ext(
                        x,
                        RunExt {
                            form_field: Some(Box::new(FormField {
                                r#type: "checkBox".into(),
                                size_half_pt: Some(r.num()),
                                ..FormField::default()
                            })),
                            ..RunExt::default()
                        },
                    )
                }
                6 => x = with_ext(x, RunExt { sym_font: Some(r.pick(&["Symbol", "", "Nothing At All"]).into()), ..RunExt::default() }),
                _ => {}
            }
            runs.push(x);
        }
        let ppr = PPr {
            num_pr: if r.next().is_multiple_of(3) {
                Some(Box::new(NumPr {
                    num_id: Some(r.pick(&[JsNumber::Num(1001.0), JsNumber::Num(1002.0), JsNumber::Num(5.0)])),
                    ilvl: Some(r.num()),
                    ..NumPr::default()
                }))
            } else {
                None
            },
            tabs: Some(vec![TabSpec { val: "decimal".into(), pos: r.num(), leader: Some("weird".into()), ..TabSpec::default() }]),
            ind: Some(Box::new(IndSpec { hanging: Some(r.num()), left: Some(r.num()), ..IndSpec::default() })),
            spacing: Some(Box::new(SpacingSpec {
                line: Some(r.num()),
                line_rule: Some(r.pick(&["auto", "exact", "atLeast", "bogus"]).into()),
                ..SpacingSpec::default()
            })),
            kinsoku: Some(r.next().is_multiple_of(2)),
            jc: Some(r.pick(&["start", "end", "both", "distribute"]).into()),
            ..PPr::default()
        };
        story.push_paragraph(para(runs, ppr));
    }
    d.body = story;
    d
}

#[test]
fn no_panic_on_hostile_paragraphs() {
    let fonts = OpenFonts::new();
    let reg = hyph_registry();
    let base = fixture_doc();
    let mut o = opts();
    o.limits.max_depth = 64;
    let env = ParaEnv { field: Some(FieldSource::Page), ..ParaEnv::default() };
    let (mut ok, mut errs) = (0usize, HashMap::<String, usize>::new());
    for seed in 0..400u64 {
        let d = hostile_doc(&base, seed.wrapping_mul(0x9e37_79b9_7f4a_7c15) ^ 0xabcdef);
        let (n, e) = prepare_all(&d, &fonts, &reg, &env, &o);
        ok += n;
        for x in e {
            assert!(
                matches!(x, LayoutError::Internal(_) | LayoutError::Limit(LimitKind::Depth | LimitKind::Work) | LayoutError::NoFace),
                "{x:?}"
            );
            *errs.entry(format!("{x:?}")).or_default() += 1;
        }
    }
    eprintln!("hostile: {ok} prepared, errors {errs:?}");
    assert!(ok > 1000);
}

#[test]
fn work_budget_stops_a_huge_paragraph() {
    let fonts = OpenFonts::new();
    let reg = hyph_registry();
    let base = fixture_doc();
    let mut d = base.clone();
    let mut story = Story::new();
    story.push_paragraph(para(vec![run(&"palavra ".repeat(20_000), RPr::default())], PPr::default()));
    d.body = story;
    let (ldoc, ids) = ldoc_of(&d);
    let o = opts();
    let input = PassInput::default();
    let (mut ft, mut rs, mut res) = (FaceTable::new(), FaceResolverCache::new(), Resources::new());
    let mut hyph = HyphenSessions::new(reg);
    let mut budget = Budget::new(&o.limits, &uo_docx_layout::options::WorkInput::default());
    let mut seq = 0u32;
    let mut flow =
        Flow { ctx: DocContext::new(word::StyleResolver::new(), word::numbering_state("pt-BR", &o.limits), word::revision_display(None)) };
    let mut cx = Cx {
        ldoc: &ldoc,
        input: &input,
        opts: &o,
        fonts: &mut ft,
        provider: &fonts,
        resolver: &mut rs,
        res: &mut res,
        hyph: &mut hyph,
        budget: &mut budget,
        depth: 0,
        line_seq: &mut seq,
        images: &mut uo_docx_layout::engine::objects::ImageTable::new(),
    };
    let p = prepare_paragraph(&mut cx, &mut flow, ids[0], &ParaEnv::default()).unwrap();
    assert_eq!(p.glyphs.len(), 160_000);
    assert!(cx.budget.spent() >= 160_000);
}
