//! PowerPoint's choice of face for a character the run's latin face lacks (Slides wave 5, record `uo-pdf.md`
//! "Fallback fonts (wave 5)"), measured in PowerPoint 365 pt-BR on Windows 11 over synthetic decks: 41 scripts × 11 run
//! languages × a theme with and without `a:font script`, explicit `a:ea`/`a:cs` (installed and unknown) and 14 latin faces;
//! the face of each character read back from PowerPoint's PDF.
//!
//! PowerPoint classifies every character: **East Asian** (Han, kana, Hangul, Bopomofo, full-width forms, CJK punctuation;
//! the ambiguous symbols too when the run is East Asian), **complex script** (Thai, Arabic, Hebrew, the Indic scripts) or
//! **latin** (the rest). An East Asian character takes the run's `a:ea` face (a theme `a:ea` of `""` resolves through the
//! theme's `a:font script` entry of the run's language: `ja` → `Jpan`, `zh-CN` → `Hans`, `zh-TW` → `Hant`, `ko` → `Hang`),
//! a complex-script character the run's `a:cs` face (`th` → `Thai`, `ar` → `Arab`, `he` → `Hebr`, `hi` → `Deva`); when
//! that face is absent, not installed or lacks the glyph, Windows' fallback of the character's script decides
//! ([`script_default`]: MS Gothic, or MS Mincho under a serif latin face; Malgun Gothic for Hangul; Angsana New for Thai;
//! Arial for Arabic and Hebrew; Nirmala UI for the Indic scripts — Mangal only through a Hindi run's slot; ...). A latin
//! character the latin face lacks goes straight to the script default. Office's cloud fonts (Angsana New, Mangal...) are
//! fetched when a face asks for them by name and then stay loaded in the session: a fresh PowerPoint draws Devanagari
//! in Nirmala UI, one that loaded Mangal earlier uses Mangal (measured; the table is the fresh-session answer).
//!
//! [`fallback_families`] gives the families to try, in order; the host checks each for the glyph (its own substitution
//! applies: a family it does not have is skipped, so the answer stays deterministic for a given font set — the open
//! substitutes when the proprietary faces are absent, never a shipped proprietary file). The frozen TS keeps the Word
//! chain instead (`SlidesRules::ts_fallback_fonts`).
//!
//! A complex-script character goes to the complex-script faces **even when the latin face has it** (measured: Arabic
//! and Hebrew in Calibri, Segoe UI, Times New Roman, Arial or Liberation Sans are drawn in the `a:cs`/default face, Arial):
//! [`complex_faces`]. East Asian and latin characters take the latin face whenever it has the glyph.

/// The class PowerPoint gives a character.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CharClass {
    Latin,
    EastAsian,
    Complex,
}

/// What the run says about its other faces. `ea`/`cs` are the run's `a:ea`/`a:cs` typefaces with the theme references
/// (`+mn-ea`, `+mj-cs`, ...) already resolved to the theme's own `a:ea`/`a:cs` (`""` when the theme leaves it empty);
/// `scripts` is the theme font collection's `a:font script` list (minor or major, as the run's latin face).
#[derive(Clone, Copy, Debug, Default)]
pub struct FallbackCtx<'a> {
    /// the run's `lang` (BCP 47, `ja-JP`; `None` when absent)
    pub lang: Option<&'a str>,
    pub ea: Option<&'a str>,
    /// `a:ea` written on the run (or its style) itself, not a theme reference: the ambiguous symbols are then East Asian
    pub ea_explicit: bool,
    pub cs: Option<&'a str>,
    pub scripts: &'a [(Box<str>, Box<str>)],
}

impl<'a> From<&'a crate::model::FontFallback> for FallbackCtx<'a> {
    fn from(fb: &'a crate::model::FontFallback) -> FallbackCtx<'a> {
        FallbackCtx { lang: fb.lang.as_deref(), ea: Some(&fb.ea), ea_explicit: fb.ea_explicit, cs: Some(&fb.cs), scripts: &fb.scripts }
    }
}

/// The theme `a:font script` entries the table reads for a run of `lang` (the reader keeps only these in
/// `FontFallback::scripts`): the East Asian script of the language, its complex script, and for Hindi the `Jpan` and
/// `Hant` entries (measured: a Hindi run draws Han with the theme's Japanese face and Bopomofo with its Hant face).
#[must_use]
pub fn scripts_for_lang(lang: Option<&str>) -> Vec<&'static str> {
    let l = lang_prefix(lang);
    let mut out = Vec::with_capacity(3);
    if l == "hi" || l.starts_with("hi-") {
        out.extend(["Jpan", "Hant"]);
    }
    if let Some(s) = ea_script(&l) {
        out.push(s);
    }
    if let Some(s) = cs_script(&l) {
        out.push(s);
    }
    out
}

fn lang_prefix(lang: Option<&str>) -> String {
    lang.unwrap_or("").chars().map(|c| if c == '_' { '-' } else { c.to_ascii_lowercase() }).collect()
}

/// The theme script of an East Asian language (`None` for the rest).
fn ea_script(lang: &str) -> Option<&'static str> {
    let l = lang;
    if l == "ja" || l.starts_with("ja-") {
        Some("Jpan")
    } else if l == "ko" || l.starts_with("ko-") {
        Some("Hang")
    } else if l.starts_with("zh-tw") || l.starts_with("zh-hk") || l.starts_with("zh-mo") || l.starts_with("zh-hant") {
        Some("Hant")
    } else if l == "zh" || l.starts_with("zh-") {
        Some("Hans")
    } else {
        None
    }
}

/// The theme script of a complex-script language (`None` for the rest).
fn cs_script(lang: &str) -> Option<&'static str> {
    let p = lang.split('-').next().unwrap_or("");
    Some(match p {
        "th" => "Thai",
        "ar" | "fa" | "ur" | "ps" | "sd" | "ug" => "Arab",
        "he" | "yi" => "Hebr",
        "hi" | "mr" | "sa" | "ne" | "kok" => "Deva",
        "bn" | "as" => "Beng",
        "ta" => "Taml",
        "te" => "Telu",
        "gu" => "Gujr",
        "pa" => "Guru",
        "kn" => "Knda",
        "ml" => "Mlym",
        "or" => "Orya",
        "si" => "Sinh",
        _ => return None,
    })
}

fn hangul(cp: u32) -> bool {
    matches!(cp, 0x1100..=0x11ff | 0x3130..=0x318f | 0xa960..=0xa97f | 0xac00..=0xd7af | 0xd7b0..=0xd7ff | 0xffa0..=0xffdc)
}

fn bopomofo(cp: u32) -> bool {
    matches!(cp, 0x3100..=0x312f | 0x31a0..=0x31bf)
}

/// Han, kana, Hangul, Bopomofo, CJK punctuation, full-width forms and the CJK compatibility blocks.
fn east_asian(cp: u32) -> bool {
    hangul(cp)
        || bopomofo(cp)
        || matches!(cp,
            0x2e80..=0x2fdf | 0x2ff0..=0x30ff | 0x31c0..=0x31ff | 0x3200..=0x4dbf | 0x4e00..=0x9fff | 0xf900..=0xfaff
            | 0xfe30..=0xfe4f | 0xff00..=0xffef | 0x20000..=0x3ffff)
}

/// Symbols PowerPoint draws with the East Asian face when the run is East Asian (measured: U+2605, U+2713, U+260E; the
/// neighbouring symbol blocks are taken alike, declared). Arrows and mathematical operators stay latin (measured).
fn ambiguous(cp: u32) -> bool {
    matches!(cp, 0x2460..=0x24ff | 0x2500..=0x257f | 0x25a0..=0x25ff | 0x2600..=0x26ff | 0x2700..=0x27bf)
}

fn complex(cp: u32) -> bool {
    matches!(cp, 0x0590..=0x05ff | 0x0600..=0x06ff | 0x0750..=0x077f | 0x08a0..=0x08ff | 0x0900..=0x0dff | 0x0e00..=0x0e7f
        | 0xfb1d..=0xfb4f | 0xfb50..=0xfdff | 0xfe70..=0xfeff)
}

/// The class of `cp` for a run of `ctx`.
#[must_use]
pub fn char_class(cp: u32, ctx: &FallbackCtx<'_>) -> CharClass {
    if east_asian(cp) {
        return CharClass::EastAsian;
    }
    if complex(cp) {
        return CharClass::Complex;
    }
    if ambiguous(cp) && (ctx.ea_explicit || ea_script(&lang_prefix(ctx.lang)).is_some()) {
        return CharClass::EastAsian;
    }
    CharClass::Latin
}

fn theme_script<'a>(ctx: &FallbackCtx<'a>, script: &str) -> Option<&'a str> {
    ctx.scripts.iter().find(|(s, _)| &**s == script).map(|(_, f)| &**f).filter(|f| !f.is_empty())
}

fn non_empty(s: Option<&str>) -> Option<&str> {
    s.filter(|s| !uo_jsnum::text::trim(s).is_empty())
}

/// The face of the run's East Asian or complex-script slot for `cp`, as PowerPoint resolves it (`None`: no face).
#[must_use]
pub fn slot_face<'a>(cp: u32, ctx: &FallbackCtx<'a>) -> Option<&'a str> {
    let lang = lang_prefix(ctx.lang);
    match char_class(cp, ctx) {
        CharClass::Latin => None,
        CharClass::EastAsian => non_empty(ctx.ea).or_else(|| {
            // measured: a Hindi run takes the theme's Jpan face for Han and kana and its Hant face for Bopomofo
            let script =
                if lang == "hi" || lang.starts_with("hi-") { Some(if bopomofo(cp) { "Hant" } else { "Jpan" }) } else { ea_script(&lang) };
            script.and_then(|s| theme_script(ctx, s))
        }),
        CharClass::Complex => non_empty(ctx.cs).or_else(|| match cs_script(&lang) {
            Some(s) => theme_script(ctx, s).or(if s == "Deva" { Some("Mangal") } else { None }),
            None => None,
        }),
    }
}

/// Windows' fallback face of `cp`'s script as PowerPoint gets it (no slot face, or the slot face lacks the glyph);
/// `serif`: the run's latin face is a serif (PANOSE serif style), which turns the Japanese and Chinese defaults to their
/// Mincho/Ming faces. `slot_missed`: a complex-script slot face was named but lacks the glyph (Arabic and Hebrew then go
/// to Times New Roman, measured under a Mangal slot).
#[must_use]
pub fn script_default(cp: u32, serif: bool, slot_missed: bool) -> Option<&'static str> {
    Some(match cp {
        _ if hangul(cp) => "Malgun Gothic",
        _ if bopomofo(cp) => {
            if serif {
                "PMingLiU"
            } else {
                "Microsoft JhengHei"
            }
        }
        _ if east_asian(cp) => {
            if serif {
                "MS Mincho"
            } else {
                "MS Gothic"
            }
        }
        0x0e00..=0x0e7f => "Angsana New",
        0x0590..=0x05ff | 0xfb1d..=0xfb4f | 0x0600..=0x06ff | 0x0750..=0x077f | 0x08a0..=0x08ff | 0xfb50..=0xfdff | 0xfe70..=0xfeff => {
            if slot_missed {
                "Times New Roman"
            } else {
                "Arial"
            }
        }
        0x0900..=0x0dff => "Nirmala UI",
        0x0e80..=0x0eff | 0x1780..=0x17ff | 0x19e0..=0x19ff => "Leelawadee UI",
        0x1000..=0x109f | 0xa9e0..=0xa9ff | 0xaa60..=0xaa7f => "Myanmar Text",
        0x0f00..=0x0fff => "Microsoft Himalaya",
        0x1200..=0x139f | 0x2d80..=0x2ddf | 0xab00..=0xab2f | 0x07c0..=0x07ff | 0x2d30..=0x2d7f => "Ebrima",
        0x10a0..=0x10ff | 0x2d00..=0x2d2f | 0x0530..=0x058f | 0xfb13..=0xfb17 => "Sylfaen",
        0x1800..=0x18af => "Mongolian Baiti",
        0xa000..=0xa4cf => "Microsoft Yi Baiti",
        0x13a0..=0x13ff | 0xab70..=0xabbf | 0x1400..=0x167f | 0x18b0..=0x18ff => "Gadugi",
        0x0700..=0x074f => "Segoe UI Historic",
        0x0780..=0x07bf => "MV Boli",
        0x2190..=0x21ff | 0x2200..=0x22ff | 0x27c0..=0x27ef | 0x2980..=0x2aff | 0x1d400..=0x1d7ff => "Cambria Math",
        0x2300..=0x23ff | 0x2460..=0x27bf | 0x2b00..=0x2bff => "Segoe UI Symbol",
        0x1f000..=0x1faff => "Segoe UI Emoji",
        _ => return None,
    })
}

/// The faces PowerPoint draws a complex-script character with whatever the latin face covers: the run's complex-script
/// slot face, then the script default (Times New Roman for Arabic/Hebrew after a named slot face); empty for the other
/// classes. The host takes the first it has with the glyph, else the run's own face.
#[must_use]
pub fn complex_faces<'a>(cp: u32, ctx: &FallbackCtx<'a>, serif: bool) -> Vec<&'a str> {
    if char_class(cp, ctx) != CharClass::Complex {
        return Vec::new();
    }
    let mut out: Vec<&'a str> = Vec::with_capacity(3);
    let slot = slot_face(cp, ctx);
    if let Some(s) = slot {
        out.push(s);
        if let Some(d) = script_default(cp, serif, true) {
            out.push(d);
        }
    }
    if let Some(d) = script_default(cp, serif, false)
        && !out.contains(&d)
    {
        out.push(d);
    }
    out
}

/// The tail tried after the script's own faces (the Word chain's last families).
pub const FALLBACK_TAIL: [&str; 6] = ["Segoe UI Symbol", "Segoe UI Emoji", "MS Gothic", "Cambria Math", "Segoe UI", "Arial"];

/// The families PowerPoint tries for `cp` when the run's latin face lacks it, in order: the run's slot face (East Asian
/// or complex script), the script default (with `slot_missed` when a slot face was named), then [`FALLBACK_TAIL`].
/// The host takes the first family it has whose face has the glyph.
#[must_use]
pub fn fallback_families<'a>(cp: u32, ctx: &FallbackCtx<'a>, serif: bool) -> Vec<&'a str> {
    let mut out: Vec<&'a str> = Vec::with_capacity(9);
    let slot = slot_face(cp, ctx);
    if let Some(s) = slot {
        out.push(s);
    }
    // the slot face missing or lacking the glyph: the measured default of a NAMED complex-script slot differs (Times
    // New Roman for Arabic/Hebrew); an East Asian slot falls to the plain default
    let named_cs = slot.is_some() && char_class(cp, ctx) == CharClass::Complex;
    if named_cs && let Some(d) = script_default(cp, serif, true) {
        out.push(d);
    }
    if let Some(d) = script_default(cp, serif, false)
        && !out.contains(&d)
    {
        out.push(d);
    }
    for f in FALLBACK_TAIL {
        if !out.contains(&f) {
            out.push(f);
        }
    }
    out
}

/// PANOSE serif test of the latin face: family kind 2 (Latin text) with a serif style 2–10 (11–15 are the sans styles).
#[must_use]
pub fn panose_serif(panose: Option<&[u8]>) -> bool {
    panose.is_some_and(|p| p.first() == Some(&2) && p.get(1).is_some_and(|s| (2..=10).contains(s)))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fams(cp: char, ctx: &FallbackCtx<'_>, serif: bool) -> Vec<String> {
        fallback_families(u32::from(cp), ctx, serif).into_iter().take(2).map(String::from).collect()
    }

    #[test]
    fn script_defaults_without_language_or_theme_scripts() {
        let c = FallbackCtx::default();
        assert_eq!(fams('漢', &c, false)[0], "MS Gothic");
        assert_eq!(fams('か', &c, true)[0], "MS Mincho");
        assert_eq!(fams('한', &c, false)[0], "Malgun Gothic");
        assert_eq!(fams('ㄅ', &c, false)[0], "Microsoft JhengHei");
        assert_eq!(fams('ㄅ', &c, true)[0], "PMingLiU");
        assert_eq!(fams('ภ', &c, false)[0], "Angsana New");
        assert_eq!(fams('ع', &c, false)[0], "Arial");
        assert_eq!(fams('ע', &c, false)[0], "Arial");
        assert_eq!(fams('ह', &c, false)[0], "Nirmala UI");
        assert_eq!(fams('ব', &c, false)[0], "Nirmala UI");
        assert_eq!(fams('ქ', &c, false)[0], "Sylfaen");
        assert_eq!(fams('Հ', &c, false)[0], "Sylfaen");
        assert_eq!(fams('ខ', &c, false)[0], "Leelawadee UI");
        assert_eq!(fams('★', &c, false)[0], "Segoe UI Symbol");
        assert_eq!(fams('⇒', &c, false)[0], "Cambria Math");
        assert_eq!(fams('ܐ', &c, false)[0], "Segoe UI Historic");
        assert_eq!(fams('ދ', &c, false)[0], "MV Boli");
    }

    #[test]
    fn language_and_theme_scripts() {
        let scripts: Vec<(Box<str>, Box<str>)> = [
            ("Jpan", "Yu Gothic"),
            ("Hans", "DengXian"),
            ("Hant", "PMingLiU"),
            ("Hang", "Malgun Gothic"),
            ("Thai", "Cordia New"),
            ("Arab", "Arial"),
            ("Deva", "Mangal"),
        ]
        .iter()
        .map(|(a, b)| (Box::from(*a), Box::from(*b)))
        .collect();
        let ctx = |lang| FallbackCtx { lang: Some(lang), ea: Some(""), cs: Some(""), scripts: &scripts, ..FallbackCtx::default() };
        assert_eq!(fams('漢', &ctx("ja-JP"), false)[0], "Yu Gothic");
        assert_eq!(fams('漢', &ctx("zh-CN"), false)[0], "DengXian");
        assert_eq!(fams('漢', &ctx("zh-TW"), false)[0], "PMingLiU");
        assert_eq!(fams('漢', &ctx("en-US"), false)[0], "MS Gothic");
        assert_eq!(fams('ㄅ', &ctx("hi-IN"), false)[0], "PMingLiU");
        assert_eq!(fams('ภ', &ctx("th-TH"), false)[0], "Cordia New");
        assert_eq!(fams('ع', &ctx("hi-IN"), false), ["Mangal", "Times New Roman"]);
        assert_eq!(fams('★', &ctx("ja-JP"), false)[0], "Yu Gothic");
        assert_eq!(fams('⇒', &ctx("ja-JP"), false)[0], "Cambria Math");
        // explicit faces
        let e = FallbackCtx { ea: Some("MS Mincho"), ea_explicit: true, cs: Some("Tahoma"), ..FallbackCtx::default() };
        assert_eq!(fams('漢', &e, false)[0], "MS Mincho");
        assert_eq!(fams('★', &e, false)[0], "MS Mincho");
        assert_eq!(fams('ภ', &e, false)[0], "Tahoma");
        assert_eq!(fams('ह', &e, false), ["Tahoma", "Nirmala UI"]);
    }

    #[test]
    fn complex_characters_go_to_the_complex_faces() {
        let c = FallbackCtx::default();
        assert_eq!(complex_faces(u32::from('ע'), &c, false), ["Arial"]);
        assert_eq!(complex_faces(u32::from('ع'), &c, true), ["Arial"]);
        assert_eq!(complex_faces(u32::from('ภ'), &c, false), ["Angsana New"]);
        assert!(complex_faces(u32::from('漢'), &c, false).is_empty());
        assert!(complex_faces(u32::from('Ж'), &c, false).is_empty());
        let e = FallbackCtx { cs: Some("Tahoma"), ..FallbackCtx::default() };
        assert_eq!(complex_faces(u32::from('ע'), &e, false), ["Tahoma", "Times New Roman", "Arial"]);
    }
}
