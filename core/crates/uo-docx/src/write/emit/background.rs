//! Frontier **W5** (page background and watermark, write halves): `page-background.ts`
//! (`vmlColorAttr`, `pageBackgroundXml`, `writePageBackground`, `WORD_PATTERNS`, `patternGif` +
//! `lzwEncode`, `oneColorGradientColor2`, `resolveColor2`) and `watermark.ts` (`watermarkRunXml`,
//! `autoTextBox`, `explicitTextBox`, `pictureBox`, `WATERMARK_GALLERY`, `WATERMARK_TEXTS`). Read
//! halves are R3's (`read::page_background`, `read::watermark`). Called by the session's save
//! (D2a: `saveDocxBody` writes the background when it changed) and by the D2b page-design
//! commands, not by `write_document`.
//!
//! The declarations of the nine kinds are in `core/docs/modules/uo-docx-d1b/W5.md`. The spec
//! types of the watermark (`WatermarkSpec`, `WatermarkIds`, the boxes) are inputs of the
//! dialogs, not model: plain Rust values (numbers `f64`, strings `&str`/`String`: a lone
//! surrogate is not representable there, declared).

use std::borrow::Cow;
use std::sync::Arc;

use uo_jsnum::conv::{to_int32, truthy};
use uo_jsnum::format::{to_js_string, to_radix_string};
use uo_jsnum::math;
use uo_jsnum::parse::parse_int;
use uo_jsnum::text::{is_js_space, to_lowercase, trim_start};
use uo_opc::Package;

use crate::js::JsNumber;
use crate::model::background::{FillStop, PageBackground, PageFill};
use crate::model::media::MediaStore;
use crate::read::drawing::vml_color;
use crate::write::js::{esc_attr, truthy_str};
use crate::write::lossless::WResult;
use crate::write::matchers::{TagPattern, Tail, boundary_at, element_matches, find_from, find_literal_ci, has_xmlns, tag_matches};
use crate::write::parts::{CONTENT_TYPES, part_by_rel_type};

// ───────────────────────── JS helpers of this file ─────────────────────────

/// `s.toUpperCase()`.
fn upper(s: &str) -> String {
    uo_text::case::to_uppercase_str(s)
}

/// `String(Math.round(v * k) / k)` — the `Math.round(v * 100) / 100` of the VML writers.
fn round_to(v: f64, k: f64) -> String {
    to_js_string(math::round(v * k) / k)
}

/// A JS number as `${n}`.
fn n_str(v: f64) -> String {
    to_js_string(v)
}

fn truthy_num(x: Option<JsNumber>) -> bool {
    x.is_some_and(JsNumber::truthy)
}

// ───────────────────────── vmlColorAttr ─────────────────────────

/// The 16 basic VML names (`NAMED`, page-background.ts:129).
fn named(hex: &str) -> Option<&'static str> {
    Some(match hex {
        "000000" => "black",
        "FFFFFF" => "white",
        "FF0000" => "red",
        "008000" => "green",
        "0000FF" => "blue",
        "FFFF00" => "yellow",
        "808080" => "gray",
        "C0C0C0" => "silver",
        "000080" => "navy",
        "800000" => "maroon",
        "008080" => "teal",
        "808000" => "olive",
        "800080" => "purple",
        "FF00FF" => "fuchsia",
        "00FFFF" => "aqua",
        "00FF00" => "lime",
        _ => return None,
    })
}

/// `${l[i]}` of a string indexed by UTF-16 unit: `undefined` past the end.
fn unit_str(units: &[u16], i: usize) -> String {
    units.get(i).map_or_else(|| "undefined".to_owned(), |&u| String::from_utf16_lossy(&[u]))
}

/// `vmlColorAttr(hex)`: RRGGBB as a VML colour as Word writes it: the name of the 16 basic ones,
/// `#rgb` when it can be, else `#rrggbb` (the `l[i]` reads are by UTF-16 unit, so a string
/// shorter than six units prints `undefined` where the TS does).
#[must_use]
pub fn vml_color_attr(hex: &str) -> String {
    let h = upper(hex.strip_prefix('#').unwrap_or(hex));
    if let Some(name) = named(&h) {
        return name.to_owned();
    }
    let l = to_lowercase(&h);
    let u: Vec<u16> = l.encode_utf16().collect();
    let at = |i: usize| u.get(i).copied();
    if at(0) == at(1) && at(2) == at(3) && at(4) == at(5) {
        format!("#{}{}{}", unit_str(&u, 0), unit_str(&u, 2), unit_str(&u, 4))
    } else {
        format!("#{l}")
    }
}

/// `escAttr` of page-background.ts: `&`, `"`, `<` (not `>`).
pub fn esc_pb(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '"' => out.push_str("&quot;"),
            '<' => out.push_str("&lt;"),
            c => out.push(c),
        }
    }
    out
}

/// `pctAttr(v)`: `${Math.round(v * 100) / 100}%`.
pub fn pct_attr(v: f64) -> String {
    format!("{}%", round_to(v, 100.0))
}

/// `fracAttr(v)`: `0`, `1`, or the number rounded to 3 places without the leading `0` of `0.x`.
pub fn frac_attr(v: f64) -> String {
    if v == 0.0 {
        return "0".to_owned();
    }
    if v == 1.0 {
        return "1".to_owned();
    }
    let s = round_to(v, 1000.0);
    match s.strip_prefix("0.") {
        Some(rest) => format!(".{rest}"),
        None => s,
    }
}

/// The text of a stop position: `0`, `1`, or `${Math.round(pos * 65536)}f` (`null * 65536` is 0).
fn stop_pos(s: &FillStop) -> String {
    let p = s.pos.as_f64();
    if p == Some(0.0) {
        "0".to_owned()
    } else if p == Some(1.0) {
        "1".to_owned()
    } else {
        format!("{}f", n_str(math::round(s.pos.to_number() * 65536.0)))
    }
}

// ───────────────────────── pageBackgroundXml ─────────────────────────

/// `pageBackgroundXml(bg, rid)`: the `w:background` element (`rid` = the relationship of the
/// effect's image, when there is one). The colour, theme attributes, `type` and `rid` are
/// printed unescaped, as the TS does.
#[must_use]
pub fn page_background_xml(bg: &PageBackground, rid: Option<&str>) -> String {
    let f: Option<&PageFill> = bg.fill.as_deref();
    let image = f.is_some_and(|f| matches!(&*f.r#type, "frame" | "tile"));
    let color = if image { "FFFFFF" } else { bg.color.as_deref().or(f.and_then(|f| f.color.as_deref())).unwrap_or("FFFFFF") };
    let mut attrs = format!("w:color=\"{}\"", upper(color));
    if truthy_str(bg.theme_color.as_deref()) && f.is_none() {
        attrs.push_str(&format!(" w:themeColor=\"{}\"", bg.theme_color.as_deref().unwrap_or("")));
    }
    if truthy_str(bg.theme_tint.as_deref()) && f.is_none() {
        attrs.push_str(&format!(" w:themeTint=\"{}\"", bg.theme_tint.as_deref().unwrap_or("")));
    }
    if truthy_str(bg.theme_shade.as_deref()) && f.is_none() {
        attrs.push_str(&format!(" w:themeShade=\"{}\"", bg.theme_shade.as_deref().unwrap_or("")));
    }
    let Some(f) = f else { return format!("<w:background {attrs}/>") };
    let mut vbg = String::from("<v:background id=\"_x0000_s1025\" o:bwmode=\"white\"");
    if !image && truthy_str(f.color.as_deref()) {
        vbg.push_str(&format!(" fillcolor=\"{}\"", vml_color_attr(f.color.as_deref().unwrap_or(""))));
    }
    if matches!(&*f.r#type, "gradient" | "gradientRadial" | "frame") {
        vbg.push_str(" o:targetscreensize=\"1024,768\"");
    }
    vbg.push('>');
    let mut fill = String::from("<v:fill");
    if let Some(rid) = rid.filter(|r| !r.is_empty()) {
        fill.push_str(&format!(" r:id=\"{rid}\""));
    }
    if let Some(t) = f.title.as_deref()
        && (&*f.r#type == "pattern" || image)
    {
        fill.push_str(&format!(" o:title=\"{}\"", esc_pb(t)));
    }
    if let Some(c2) = f.color2.as_deref() {
        fill.push_str(&format!(" color2=\"{}\"", esc_pb(c2)));
    }
    if let Some(colors) = f.colors.as_ref().filter(|c| !c.is_empty()) {
        let list: Vec<String> = colors.iter().map(|s| format!("{} {}", stop_pos(s), vml_color_attr(&s.color))).collect();
        fill.push_str(&format!(" colors=\"{}\"", list.join(";")));
    }
    if truthy_num(f.angle) {
        let a = f.angle.map_or(0.0, JsNumber::to_number);
        fill.push_str(&format!(" angle=\"{}\"", n_str(math::round(a))));
    }
    if let Some(m) = f.method.as_deref().filter(|m| !m.is_empty()) {
        fill.push_str(&format!(" method=\"{}\"", esc_pb(m)));
    }
    if let Some((x, y)) = f.focus_position {
        let xs = if x.truthy() { frac_attr(x.to_number()) } else { String::new() };
        let ys = if y.truthy() { format!(",{}", frac_attr(y.to_number())) } else { String::new() };
        fill.push_str(&format!(" focusposition=\"{xs}{ys}\" focussize=\"\""));
    }
    if truthy_num(f.focus) {
        fill.push_str(&format!(" focus=\"{}\"", pct_attr(f.focus.map_or(0.0, JsNumber::to_number))));
    }
    fill.push_str(&format!(" type=\"{}\"", f.r#type));
    if f.gradient_center == Some(true) {
        fill.push_str("><o:fill v:ext=\"view\" type=\"gradientCenter\"/></v:fill>");
    } else {
        fill.push_str("/>");
    }
    format!("<w:background {attrs}>{vbg}{fill}</v:background></w:background>")
}

// ───────────────────────── writePageBackground ─────────────────────────

/// What a `writePageBackground` added to the package (`{ rid, media }`): the next save passes it
/// back as `prev` so the previous image and relationship leave first.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BackgroundAdded {
    pub rid: String,
    pub media: String,
}

const NS: [(&str, &str); 3] = [
    ("v", "urn:schemas-microsoft-com:vml"),
    ("o", "urn:schemas-microsoft-com:office:office"),
    ("r", "http://schemas.openxmlformats.org/officeDocument/2006/relationships"),
];

/// `MIME[ext]` (own keys only: `constructor` reads `Object.prototype` in the TS, accepted by
/// name, as `MEDIA_MIME` of the core).
fn mime_of(ext: &str) -> Option<&'static str> {
    Some(match ext {
        "png" => "image/png",
        "jpeg" | "jpg" => "image/jpeg",
        "gif" => "image/gif",
        "bmp" => "image/bmp",
        "tif" | "tiff" => "image/tiff",
        "emf" => "image/x-emf",
        "wmf" => "image/x-wmf",
        _ => return None,
    })
}

/// `path.replace(/([^/]+)$/, '_rels/$1.rels')`: the last path segment becomes `_rels/<segment>.rels`;
/// a path that is empty or ends with `/` has no match and stays.
#[must_use]
pub fn rels_path_of(path: &str) -> String {
    if path.is_empty() || path.ends_with('/') {
        return path.to_owned();
    }
    match path.rsplit_once('/') {
        Some((dir, base)) => format!("{dir}/_rels/{base}.rels"),
        None => format!("_rels/{path}.rels"),
    }
}

/// `/\br:id="([^"]+)"/.exec(s)?.[1]`: the value of the first `r:id` attribute (not preceded by a
/// word character) with a non-empty value.
#[must_use]
pub fn first_r_id(s: &str) -> Option<&str> {
    let mut from = 0;
    while let Some(i) = find_from(s, "r:id=\"", from) {
        from = i + 1;
        if !boundary_at(s, i) {
            continue;
        }
        let rest = s.get(i + 6..)?;
        let end = rest.find('"')?;
        if end > 0 {
            return rest.get(..end);
        }
    }
    None
}

/// `/<NAME\b[^>]*>/.exec(s)`: the byte range of the first start tag of `name` (the match needs a `>`
/// after the name; none after the first candidate means none at all).
#[must_use]
pub fn first_open_tag(s: &str, name: &str) -> Option<(usize, usize)> {
    let open = format!("<{name}");
    let mut from = 0;
    while let Some(i) = find_from(s, &open, from) {
        from = i + 1;
        let after = i + open.len();
        if !boundary_at(s, after) {
            continue;
        }
        let gt = s.get(after..)?.find('>')?;
        return Some((i, after + gt + 1));
    }
    None
}

/// `GetSubstitution` (ECMA-262) of `template` for a match with the capture groups `caps` (a
/// group that did not take part is `None`): `$$`, `$&`, `` $` ``, `$'`, `$n`, `$nn`; `$<` and any
/// other `$` stay as written (the regex has no named groups).
pub(crate) fn get_substitution(template: &str, matched: &str, before: &str, after: &str, caps: &[Option<&str>]) -> String {
    let mut out = String::new();
    let mut rest = template;
    while let Some(k) = rest.find('$') {
        out.push_str(rest.get(..k).unwrap_or(""));
        let tail = rest.get(k..).unwrap_or("");
        let b = tail.as_bytes();
        let digit = |i: usize| b.get(i).filter(|c| c.is_ascii_digit()).map(|c| usize::from(c - b'0'));
        let (insert, used): (Cow<'_, str>, usize) = match b.get(1) {
            Some(b'$') => ("$".into(), 2),
            Some(b'&') => (matched.into(), 2),
            Some(b'`') => (before.into(), 2),
            Some(b'\'') => (after.into(), 2),
            Some(c) if c.is_ascii_digit() => {
                let d1 = digit(1).unwrap_or(0);
                let two = digit(2).map(|d2| d1 * 10 + d2).filter(|n| (1..=caps.len()).contains(n));
                if let Some(n) = two {
                    (caps.get(n - 1).copied().flatten().unwrap_or("").into(), 3)
                } else if (1..=caps.len()).contains(&d1) {
                    (caps.get(d1 - 1).copied().flatten().unwrap_or("").into(), 2)
                } else {
                    ("$".into(), 1)
                }
            }
            _ => ("$".into(), 1),
        };
        out.push_str(&insert);
        rest = tail.get(used..).unwrap_or("");
    }
    out.push_str(rest);
    out
}

/// `ct.replace(/(<Types\b[^>]*>)/, `$1<Default Extension="${ext}" ContentType="${mime}"/>`)`: the
/// string template (so a `$` in the extension does what it does in the TS).
#[must_use]
pub fn add_default_extension(ct: &str, ext: &str) -> String {
    let Some((a, b)) = first_open_tag(ct, "Types") else { return ct.to_owned() };
    let mime = mime_of(ext).map_or_else(|| format!("image/{ext}"), str::to_owned);
    let template = format!("$1<Default Extension=\"{ext}\" ContentType=\"{mime}\"/>");
    let (before, matched, after) = (ct.get(..a).unwrap_or(""), ct.get(a..b).unwrap_or(""), ct.get(b..).unwrap_or(""));
    format!("{before}{}{after}", get_substitution(&template, matched, before, after, &[Some(matched)]))
}

/// The first `<Relationship\b[^>]*\bId="rid"[^>]*/>` of `rels` removed (the rid a literal).
pub fn remove_relationship(rels: &str, rid: &str) -> String {
    let pat = TagPattern {
        names: &["Relationship"],
        name_boundary: true,
        attr: "Id",
        attr_boundary: true,
        values: &[rid],
        tail: Tail::SelfClose,
    };
    match tag_matches(rels, &pat).first() {
        Some(&(a, b)) => format!("{}{}", rels.get(..a).unwrap_or(""), rels.get(b..).unwrap_or("")),
        None => rels.to_owned(),
    }
}

/// Sets a part's text when it differs (W-D10: the same decompressed bytes; keeps the raw copy of
/// an unchanged entry).
fn set_text(pkg: &mut Package, name: &str, text: String) -> WResult<()> {
    if pkg.part_text(name)?.is_some_and(|t| *t == *text) {
        return Ok(());
    }
    pkg.set_part_text(name, text);
    Ok(())
}

pub(crate) const EMPTY_DOC_RELS: &str = "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\r\n<Relationships xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\"></Relationships>";
const REL_IMAGE: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships/image";

/// `writePageBackground(pkg, bg, prev)`: writes (or removes, with `None`) the `w:background` of the
/// serialized `document.xml`: replaces the element, adds the missing namespace declarations,
/// writes the effect's image (`word/media`, relationship, content type) and drops the relationship
/// of the previous image that is left unused. `prev` is what the previous write on this package
/// added (the editor saves several times on the same package): it leaves first. `media` is the
/// store the model's image refers to. Returns what this call added.
///
/// The `r:id` that was in the old element is a literal in the two dynamic patterns that look for
/// it (a metacharacter in it changes or throws in the TS, accepted by name, G9).
pub fn write_page_background(
    pkg: &mut Package,
    media: &MediaStore,
    bg: Option<&PageBackground>,
    prev: Option<&BackgroundAdded>,
) -> WResult<Option<BackgroundAdded>> {
    let Some(path) = part_by_rel_type(pkg, "officeDocument", "word/document.xml")? else { return Ok(None) };
    let Some(mut xml) = pkg.part_text(&path)?.map(Cow::into_owned).filter(|x| !x.is_empty()) else { return Ok(None) };
    let rels_path = rels_path_of(&path);
    let mut rels = pkg.part_text(&rels_path)?.map_or_else(|| EMPTY_DOC_RELS.to_owned(), Cow::into_owned);
    if let Some(prev) = prev {
        rels = remove_relationship(&rels, &prev.rid);
        pkg.remove_part(&prev.media);
    }
    let mut added = None;
    let old = element_matches(&xml, "w:background").first().copied();
    let old_rid = old.and_then(|(a, b)| xml.get(a..b)).and_then(first_r_id).map(str::to_owned);
    if let Some((a, b)) = old {
        xml = format!("{}{}", xml.get(..a).unwrap_or(""), xml.get(b..).unwrap_or(""));
    }
    let mut rid: Option<String> = None;
    let img = bg.and_then(|b| b.fill.as_deref()).and_then(|f| f.image.as_ref());
    if let Some(img) = img {
        let lower = to_lowercase(&img.ext);
        let ext = if lower == "jpg" { "jpeg".to_owned() } else { lower };
        let mut n = 1u64;
        while pkg.contains(&format!("word/media/image{n}.{ext}")) {
            n += 1;
        }
        let target = format!("media/image{n}.{ext}");
        pkg.set_part(format!("word/{target}"), media.get(img.data.0).unwrap_or(&[]).to_vec());
        let mut k = 1u64;
        while rels.contains(&format!("Id=\"rId{k}\"")) {
            k += 1;
        }
        let new_rid = format!("rId{k}");
        added = Some(BackgroundAdded { rid: new_rid.clone(), media: format!("word/{target}") });
        rels = rels.replacen(
            "</Relationships>",
            &format!("<Relationship Id=\"{new_rid}\" Type=\"{REL_IMAGE}\" Target=\"{target}\"/></Relationships>"),
            1,
        );
        rid = Some(new_rid);
        let ct = pkg.part_text(CONTENT_TYPES)?.map_or_else(String::new, Cow::into_owned);
        if !ct.is_empty() && !find_literal_ci(&ct, &format!("<Default Extension=\"{ext}\"")) {
            let ct = add_default_extension(&ct, &ext);
            set_text(pkg, CONTENT_TYPES, ct)?;
        }
    }
    if let Some(bg) = bg {
        let el = page_background_xml(bg, rid.as_deref());
        if let Some((a, b)) = first_open_tag(&xml, "w:document") {
            let mut tag = xml.get(a..b).unwrap_or("").to_owned();
            if bg.fill.is_some() {
                for (p, uri) in NS {
                    if !has_xmlns(&tag, p) {
                        tag.pop();
                        tag.push_str(&format!(" xmlns:{p}=\"{uri}\">"));
                    }
                }
            }
            xml = format!("{}{tag}{el}{}", xml.get(..a).unwrap_or(""), xml.get(b..).unwrap_or(""));
        }
    }
    if let Some(old_rid) = old_rid.as_deref()
        && !xml.contains(&format!("\"{old_rid}\""))
    {
        rels = remove_relationship(&rels, old_rid);
    }
    set_text(pkg, &path, xml)?;
    if img.is_some() || old_rid.is_some() || prev.is_some() {
        set_text(pkg, &rels_path, rels)?;
    }
    Ok(added)
}

// ───────────────────────── patterns and the GIF ─────────────────────────

/// A name of a Word pattern in the three dialog languages.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PatternTitle {
    pub pt: &'static str,
    pub en: &'static str,
    pub es: &'static str,
}

/// An entry of `WORD_PATTERNS`: the DrawingML preset of the same bitmap and the `o:title` Word
/// writes (pt-BR keeps its leading space).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WordPattern {
    pub prst: &'static str,
    pub title: PatternTitle,
}

const fn pat(prst: &'static str, pt: &'static str, en: &'static str, es: &'static str) -> WordPattern {
    WordPattern { prst, title: PatternTitle { pt, en, es } }
}

/// The 48 patterns of Fill › Pattern, in Word's order (`Fill.Patterned` 1..48).
pub const WORD_PATTERNS: [WordPattern; 48] = [
    pat("pct5", " 5%", "5%", "5%"),
    pat("pct10", " 10%", "10%", "10%"),
    pat("pct20", " 20%", "20%", "20%"),
    pat("pct25", " 25%", "25%", "25%"),
    pat("pct30", " 30%", "30%", "30%"),
    pat("pct40", " 40%", "40%", "40%"),
    pat("pct50", " 50%", "50%", "50%"),
    pat("pct60", " 60%", "60%", "60%"),
    pat("pct70", " 70%", "70%", "70%"),
    pat("pct75", " 75%", "75%", "75%"),
    pat("pct80", " 80%", "80%", "80%"),
    pat("pct90", " 90%", "90%", "90%"),
    pat("dkHorz", " Escuras", "Dark horizontal", "Horizontal oscura"),
    pat("dkVert", " escura", "Dark vertical", "Vertical oscura"),
    pat("dkDnDiag", " Escuras para baixo", "Dark downward diagonal", "Diagonal hacia abajo oscura"),
    pat("dkUpDiag", " Escuras para cima", "Dark upward diagonal", "Diagonal hacia arriba oscura"),
    pat("smCheck", "Quadriculado pequeno", "Small checker board", "Damero pequeño"),
    pat("trellis", "Treliça", "Trellis", "Enrejado"),
    pat("ltHorz", " Claras", "Light horizontal", "Horizontal clara"),
    pat("ltVert", " clara", "Light vertical", "Vertical clara"),
    pat("ltDnDiag", " Claras para baixo", "Light downward diagonal", "Diagonal hacia abajo clara"),
    pat("ltUpDiag", " Claras para cima", "Light upward diagonal", "Diagonal hacia arriba clara"),
    pat("smGrid", "Grade fechada", "Small grid", "Cuadrícula pequeña"),
    pat("dotDmnd", "Losango pontilhado", "Dotted diamond", "Diamante punteado"),
    pat("wdDnDiag", " Largas para baixo", "Wide downward diagonal", "Diagonal ancha hacia abajo"),
    pat("wdUpDiag", " Largas para cima", "Wide upward diagonal", "Diagonal ancha hacia arriba"),
    pat("dashUpDiag", " Barras", "Dashed upward diagonal", "Diagonal discontinua hacia arriba"),
    pat("dashDnDiag", " Barras invertidas", "Dashed downward diagonal", "Diagonal discontinua hacia abajo"),
    pat("narVert", " estreita", "Narrow vertical", "Vertical estrecha"),
    pat("narHorz", " Estreitas", "Narrow horizontal", "Horizontal estrecha"),
    pat("dashVert", " alternando linhas verticais", "Dashed vertical", "Vertical discontinua"),
    pat("dashHorz", " alternando linhas horizontais", "Dashed horizontal", "Horizontal discontinua"),
    pat("lgConfetti", "Confetes grandes", "Large confetti", "Confeti grande"),
    pat("lgGrid", "Grade aberta", "Large grid", "Cuadrícula grande"),
    pat("horzBrick", "Tijolo na horizontal", "Horizontal brick", "Ladrillo horizontal"),
    pat("lgCheck", "Quadriculado grande", "Large checker board", "Damero grande"),
    pat("smConfetti", "Confetes pequenos", "Small confetti", "Confeti pequeño"),
    pat("zigZag", "Ziguezague", "Zig zag", "Zigzag"),
    pat("solidDmnd", "Losango sólido", "Solid diamond", "Diamante sólido"),
    pat("diagBrick", "Tijolo na diagonal", "Diagonal brick", "Ladrillo diagonal"),
    pat("openDmnd", "Contorno de losango", "Outlined diamond", "Diamante con contorno"),
    pat("plaid", "Xadrez", "Plaid", "Tartán"),
    pat("sphere", "Esfera", "Sphere", "Esfera"),
    pat("weave", "Trança", "Weave", "Trama"),
    pat("dotGrid", "Grade pontilhada", "Dotted grid", "Cuadrícula de puntos"),
    pat("divot", "Malha", "Divot", "Malla"),
    pat("shingle", "Telhas", "Shingle", "Tejas"),
    pat("wave", "Ondulado", "Wave", "Onda"),
];

/// `patternGif(rows)`: the pattern's GIF as Word writes it (GIF87a 8x8, local palette [white,
/// black], the comment `Software: Microsoft Office`): a WHITE pixel is the foreground colour
/// (`fillcolor`), black the background (`color2`) — the preset's bits inverted. `rows` are the 8
/// rows of the preset (bit 7 = column 0, 1 = foreground); a missing row is 0, and a row is
/// shifted as a JS int32.
#[must_use]
pub fn pattern_gif(rows: &[f64]) -> Vec<u8> {
    let mut idx: Vec<u8> = Vec::with_capacity(64);
    for y in 0..8usize {
        let row = to_int32(rows.get(y).copied().unwrap_or(0.0));
        for x in 0..8u32 {
            idx.push(if (row >> (7 - x)) & 1 != 0 { 0 } else { 1 });
        }
    }
    let comment = b"Software: Microsoft Office";
    let mut out: Vec<u8> = Vec::with_capacity(128);
    out.extend_from_slice(b"GIF87a");
    out.extend_from_slice(&[8, 0, 8, 0, 0x77, 1, 0, 0x21, 0xfe]);
    out.push(u8::try_from(comment.len()).unwrap_or(0));
    out.extend_from_slice(comment);
    out.extend_from_slice(&[0, 0x2c, 0, 0, 0, 0, 8, 0, 8, 0, 0x80, 0xff, 0xff, 0xff, 0, 0, 0]);
    let lzw = lzw_encode(&idx, 2);
    out.push(2);
    for blk in lzw.chunks(255) {
        out.push(u8::try_from(blk.len()).unwrap_or(0));
        out.extend_from_slice(blk);
    }
    out.extend_from_slice(&[0, 0x3b]);
    out
}

/// `emit(code)` of `lzwEncode`: `code` in `size` bits, LSB first.
fn lzw_emit(code: u32, size: u32, cur: &mut u32, bits: &mut u32, out: &mut Vec<u8>) {
    *cur |= code << *bits;
    *bits += size;
    while *bits >= 8 {
        out.push(u8::try_from(*cur & 0xff).unwrap_or(0));
        *cur >>= 8;
        *bits -= 8;
    }
}

/// `lzwEncode(data, minCode)`: the GIF LZW (variable-width codes, LSB first), the TS algorithm:
/// the dictionary keyed by (code of the prefix, next symbol) instead of by the string of the
/// sequence (the same entries, since every multi-symbol sequence extends one already there).
/// `data` is never empty (the 64 pixels of a pattern); empty gives the clear code only.
fn lzw_encode(data: &[u8], min_code: u32) -> Vec<u8> {
    use std::collections::HashMap;
    let clear: u32 = 1 << min_code;
    let eoi = clear + 1;
    let mut out: Vec<u8> = Vec::new();
    let (mut cur, mut bits) = (0u32, 0u32);
    let mut size = min_code + 1;
    lzw_emit(clear, size, &mut cur, &mut bits, &mut out);
    let mut dict: HashMap<(u32, u8), u32> = HashMap::new();
    let mut next = eoi + 1;
    let Some(&first) = data.first() else { return out };
    let mut w: u32 = u32::from(first);
    for &k in data.iter().skip(1) {
        if let Some(&code) = dict.get(&(w, k)) {
            w = code;
            continue;
        }
        lzw_emit(w, size, &mut cur, &mut bits, &mut out);
        dict.insert((w, k), next);
        next += 1;
        if next > (1 << size) && size < 12 {
            size += 1;
        }
        if next >= 4095 {
            lzw_emit(clear, size, &mut cur, &mut bits, &mut out);
            dict.clear();
            next = eoi + 1;
            size = min_code + 1;
        }
        w = u32::from(k);
    }
    lzw_emit(w, size, &mut cur, &mut bits, &mut out);
    lzw_emit(eoi, size, &mut cur, &mut bits, &mut out);
    if bits > 0 {
        out.push(u8::try_from(cur & 0xff).unwrap_or(0));
    }
    out
}

// ───────────────────────── colours ─────────────────────────

/// `oneColorGradientColor2(degree)`: colour 2 of a ONE-colour gradient (degree 0..1 →
/// `fill darken(n)` / `fill` / `fill lighten(n)`); a NaN degree stays NaN, as `Math.max/min` do.
#[must_use]
pub fn one_color_gradient_color2(degree: f64) -> String {
    let d = math::max(0.0, math::min(1.0, degree));
    if math::abs(d - 0.5) < 1e-6 {
        return "fill".to_owned();
    }
    if d < 0.5 {
        format!("fill darken({})", n_str(math::round(510.0 * d)))
    } else {
        format!("fill lighten({})", n_str(math::round(510.0 * (1.0 - d))))
    }
}

/// The match of `/^\s*fill(?:\s+(darken|lighten)\((\d+)\))?\s*$/i`: `None` when the text does not
/// match; else the optional group (the word and the digits), `None` inside for the bare `fill`.
#[must_use]
pub fn fill_color2_match(s: &str) -> Option<Option<(&str, &str)>> {
    let after_ws = s.trim_start_matches(is_js_space);
    let rest = strip_prefix_ci(after_ws, "fill")?;
    let group = || -> Option<(&str, &str)> {
        let t = trim_start(rest);
        if t.len() == rest.len() {
            return None;
        }
        let (word, t) =
            if let Some(r) = strip_prefix_ci(t, "darken") { (t.get(..6)?, r) } else { (t.get(..7)?, strip_prefix_ci(t, "lighten")?) };
        let t = t.strip_prefix('(')?;
        let n = t.bytes().take_while(u8::is_ascii_digit).count();
        if n == 0 {
            return None;
        }
        let (digits, t) = t.split_at(n);
        let t = t.strip_prefix(')')?;
        t.chars().all(is_js_space).then_some((word, digits))
    };
    if let Some(g) = group() {
        return Some(Some(g));
    }
    rest.chars().all(is_js_space).then_some(None)
}

/// `s` without the prefix `p` (an ASCII word), ASCII-case-insensitively: the `i` flag without `u`
/// canonicalizes by `toUpperCase`, and no non-ASCII letter has an ASCII upper case that the
/// canonicalization allows (`ı`, `ſ`), so only ASCII letters match.
fn strip_prefix_ci<'a>(s: &'a str, p: &str) -> Option<&'a str> {
    let head = s.get(..p.len())?;
    head.eq_ignore_ascii_case(p).then(|| s.get(p.len()..)).flatten()
}

/// `n.toString(16)` of an integer-valued number (`NaN` and the infinities as JS prints them).
fn hex_string(x: f64) -> String {
    to_radix_string(x, 16).unwrap_or_else(|_| n_str(x))
}

/// `resolveColor2(color2, color1)`: the VML colour 2 resolved against colour 1 (`fill darken(n)`,
/// `fill lighten(n)`, `fill`, or a colour). Truncated like Word: darken = ⌊c·n/255⌋, lighten =
/// 255 − ⌊(255−c)·n/255⌋.
#[must_use]
pub fn resolve_color2(color2: Option<&str>, color1: &str) -> String {
    let Some(color2) = color2.filter(|c| !c.is_empty()) else { return "FFFFFF".to_owned() };
    let Some(m) = fill_color2_match(color2) else {
        return vml_color(Some(color2)).unwrap_or_else(|| "FFFFFF".to_owned());
    };
    let Some((word, digits)) = m else { return color1.to_owned() };
    let n = math::max(0.0, math::min(255.0, parse_int(digits, 10)));
    let units: Vec<u16> = color1.encode_utf16().collect();
    let chan = |i: usize| -> f64 {
        let s = String::from_utf16_lossy(units.get(i..(i + 2).min(units.len())).unwrap_or(&[]));
        parse_int(&s, 16)
    };
    let c = [chan(0), chan(2), chan(4)];
    let darken = word.eq_ignore_ascii_case("darken");
    let r: Vec<String> = c
        .iter()
        .map(|&v| {
            let x = if darken { math::floor((v * n) / 255.0) } else { 255.0 - math::floor(((255.0 - v) * n) / 255.0) };
            let h = hex_string(x);
            if h.len() < 2 { format!("0{h}") } else { h }
        })
        .collect();
    upper(&r.concat())
}

// ───────────────────────── watermark ─────────────────────────

/// `WatermarkLayout`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WatermarkLayout {
    Diagonal,
    Horizontal,
}

/// `WatermarkTextSpec`.
#[derive(Debug, Clone, PartialEq)]
pub struct WatermarkTextSpec {
    pub text: String,
    pub font: String,
    /// pt; absent (or 0) = Automatic (`font-size:1pt`, the box gives the size)
    pub size_pt: Option<f64>,
    /// RRGGBB
    pub color: String,
    pub semitransparent: bool,
    pub layout: WatermarkLayout,
    /// the language of the text (`w:lang` of the dialog), BCP 47
    pub lang: Option<String>,
}

/// `WatermarkPictureSpec`.
#[derive(Debug, Clone, PartialEq)]
pub struct WatermarkPictureSpec {
    pub data: Arc<[u8]>,
    pub ext: String,
    /// the file name (`o:title`)
    pub title: String,
    /// scale in %; absent (or 0) = Automatic (the width of the content area)
    pub scale: Option<f64>,
    pub washout: bool,
}

/// `WatermarkSpec`.
#[derive(Debug, Clone, PartialEq)]
pub enum WatermarkSpec {
    Text(WatermarkTextSpec),
    Picture(WatermarkPictureSpec),
}

/// The size of the shape in pt (`{ widthPt, heightPt }`).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct WatermarkBox {
    pub width_pt: f64,
    pub height_pt: f64,
}

/// `WatermarkIds`: `n` = suffix of the name (`PowerPlusWaterMarkObject{n}`), `spid` = `o:spid`
/// (`_x0000_s{spid}`), `anchor_id` = `w14:anchorId` (printed unescaped).
#[derive(Debug, Clone, PartialEq)]
pub struct WatermarkIds {
    pub n: f64,
    pub spid: f64,
    pub anchor_id: String,
}

const SHAPETYPE_TEXT: &str = concat!(
    "<v:shapetype id=\"_x0000_t136\" coordsize=\"21600,21600\" o:spt=\"136\" adj=\"10800\" path=\"m@7,l@8,m@5,21600l@6,21600e\">",
    "<v:formulas><v:f eqn=\"sum #0 0 10800\"/><v:f eqn=\"prod #0 2 1\"/><v:f eqn=\"sum 21600 0 @1\"/><v:f eqn=\"sum 0 0 @2\"/><v:f eqn=\"sum 21600 0 @3\"/>",
    "<v:f eqn=\"if @0 @3 0\"/><v:f eqn=\"if @0 21600 @1\"/><v:f eqn=\"if @0 0 @2\"/><v:f eqn=\"if @0 @4 21600\"/><v:f eqn=\"mid @5 @6\"/><v:f eqn=\"mid @8 @5\"/>",
    "<v:f eqn=\"mid @7 @8\"/><v:f eqn=\"mid @6 @7\"/><v:f eqn=\"sum @6 0 @5\"/></v:formulas>",
    "<v:path textpathok=\"t\" o:connecttype=\"custom\" o:connectlocs=\"@9,0;@10,10800;@11,21600;@12,10800\" o:connectangles=\"270,180,90,0\"/>",
    "<v:textpath on=\"t\" fitshape=\"t\"/><v:handles><v:h position=\"#0,bottomRight\" xrange=\"6629,14971\"/></v:handles>",
    "<o:lock v:ext=\"edit\" text=\"t\" shapetype=\"t\"/></v:shapetype>"
);

const SHAPETYPE_PICTURE: &str = concat!(
    "<v:shapetype id=\"_x0000_t75\" coordsize=\"21600,21600\" o:spt=\"75\" o:preferrelative=\"t\" path=\"m@4@5l@4@11@9@11@9@5xe\" filled=\"f\" stroked=\"f\">",
    "<v:stroke joinstyle=\"miter\"/><v:formulas><v:f eqn=\"if lineDrawn pixelLineWidth 0\"/><v:f eqn=\"sum @0 1 0\"/><v:f eqn=\"sum 0 0 @1\"/>",
    "<v:f eqn=\"prod @2 1 2\"/><v:f eqn=\"prod @3 21600 pixelWidth\"/><v:f eqn=\"prod @3 21600 pixelHeight\"/><v:f eqn=\"sum @0 0 1\"/>",
    "<v:f eqn=\"prod @6 1 2\"/><v:f eqn=\"prod @7 21600 pixelWidth\"/><v:f eqn=\"sum @8 21600 0\"/><v:f eqn=\"prod @7 21600 pixelHeight\"/>",
    "<v:f eqn=\"sum @10 21600 0\"/></v:formulas><v:path o:extrusionok=\"f\" gradientshapeok=\"t\" o:connecttype=\"rect\"/>",
    "<o:lock v:ext=\"edit\" aspectratio=\"t\"/></v:shapetype>"
);

const POS: &str = "mso-position-horizontal:center;mso-position-horizontal-relative:margin;mso-position-vertical:center;mso-position-vertical-relative:margin";

/// `ptv(v)`: pt as Word's VML writes it (up to 2 places, no trailing zeros).
fn ptv(v: f64) -> String {
    format!("{}pt", round_to(v, 100.0))
}

/// `(h) => `#${h.toLowerCase()}``: the default colour of `watermarkRunXml`.
#[must_use]
pub fn default_vml_color(hex: &str) -> String {
    format!("#{}", to_lowercase(hex))
}

/// `watermarkRunXml(spec, box, ids, rid, vmlColor)`: the `w:r` of the watermark for the first
/// paragraph of a header. `rid` is the image relationship (picture marks only); `vml_color`
/// defaults to [`default_vml_color`]. Names, ids, `rid` and the fill colour are printed as the TS
/// does (the font, text and title escaped; `rid`, ids and the colour not).
#[must_use]
pub fn watermark_run_xml(
    spec: &WatermarkSpec,
    box_: WatermarkBox,
    ids: &WatermarkIds,
    rid: Option<&str>,
    vml_color: Option<&dyn Fn(&str) -> String>,
) -> String {
    let head = format!("<w:r><w:rPr><w:noProof/></w:rPr><w:pict w14:anchorId=\"{}\">", ids.anchor_id);
    let (n, spid) = (n_str(ids.n), n_str(ids.spid));
    let (w, h) = (ptv(box_.width_pt), ptv(box_.height_pt));
    match spec {
        WatermarkSpec::Picture(p) => {
            let fx = if p.washout { " gain=\"19661f\" blacklevel=\"22938f\"" } else { "" };
            format!(
                "{head}{SHAPETYPE_PICTURE}<v:shape id=\"WordPictureWatermark{n}\" o:spid=\"_x0000_s{spid}\" type=\"#_x0000_t75\" \
                 style=\"position:absolute;margin-left:0;margin-top:0;width:{w};height:{h};z-index:-251657216;{POS}\" o:allowincell=\"f\">\
                 <v:imagedata r:id=\"{}\" o:title=\"{}\"{fx}/><w10:wrap anchorx=\"margin\" anchory=\"margin\"/></v:shape></w:pict></w:r>",
                rid.unwrap_or(""),
                esc_attr(&p.title)
            )
        }
        WatermarkSpec::Text(t) => {
            let rot = if t.layout == WatermarkLayout::Diagonal { "rotation:315;" } else { "" };
            let size = match t.size_pt {
                Some(s) if truthy(s) => format!("{}pt", round_to(s, 100.0)),
                _ => "1pt".to_owned(),
            };
            let color = vml_color.map_or_else(|| default_vml_color(&t.color), |f| f(&t.color));
            let fill = if t.semitransparent { "<v:fill opacity=\".5\"/>" } else { "" };
            format!(
                "{head}{SHAPETYPE_TEXT}<v:shape id=\"PowerPlusWaterMarkObject{n}\" o:spid=\"_x0000_s{spid}\" type=\"#_x0000_t136\" alt=\"\" \
                 style=\"position:absolute;margin-left:0;margin-top:0;width:{w};height:{h};{rot}z-index:-251657216;\
                 mso-wrap-edited:f;mso-width-percent:0;mso-height-percent:0;{POS};mso-width-percent:0;mso-height-percent:0\" \
                 o:allowincell=\"f\" fillcolor=\"{color}\" stroked=\"f\">{fill}\
                 <v:textpath style=\"font-family:&quot;{}&quot;;font-size:{size}\" string=\"{}\"/>\
                 <w10:wrap anchorx=\"margin\" anchory=\"margin\"/></v:shape></w:pict></w:r>",
                esc_attr(&t.font),
                esc_attr(&t.text)
            )
        }
    }
}

/// `(DIAG_FIT)`: diagonal, (width + height)·cos 45° = this factor × the width of the content
/// area (the Word blocks: 466.55 / 468).
const DIAG_FIT: f64 = 466.55 / 468.0;

/// `text.replace(/\s+/g, '').length`: the UTF-16 length without JS white space.
pub fn letters_len(s: &str) -> f64 {
    let n: usize = s.chars().filter(|&c| !is_js_space(c)).map(char::len_utf16).sum();
    f64::from(u32::try_from(n).unwrap_or(u32::MAX))
}

/// `round2(v)`: `Math.round(v * 100) / 100`.
fn round2(v: f64) -> f64 {
    math::round(v * 100.0) / 100.0
}

/// `autoTextBox(text, layout, contentWidthPt)`: the box of the AUTOMATIC text mark in a content
/// area of `content_width_pt`: aspect = characters without spaces / 3 (the rule the 12 Word blocks
/// follow); horizontal takes the width; diagonal has the rotated box in the width.
#[must_use]
pub fn auto_text_box(text: &str, layout: WatermarkLayout, content_width_pt: f64) -> WatermarkBox {
    let n = math::max(1.0, letters_len(text));
    let aspect = n / 3.0;
    if layout == WatermarkLayout::Horizontal {
        return WatermarkBox { width_pt: round2(content_width_pt), height_pt: round2(content_width_pt / aspect) };
    }
    let h = (DIAG_FIT * content_width_pt * std::f64::consts::SQRT_2) / (aspect + 1.0);
    WatermarkBox { width_pt: round2(aspect * h), height_pt: round2(h) }
}

/// `explicitTextBox(advancePt, sizePt, lineFactor)`: the box of a mark with an EXPLICIT size (the
/// width is the advance of the text and the height is size·lineFactor, in whole pt).
#[must_use]
pub fn explicit_text_box(advance_pt: f64, size_pt: f64, line_factor: f64) -> WatermarkBox {
    WatermarkBox { width_pt: math::max(1.0, math::round(advance_pt)), height_pt: math::max(1.0, math::round(size_pt * line_factor)) }
}

/// `pictureBox(naturalPt, scale, contentWidthPt)`: Automatic = the width of the content area;
/// otherwise the natural size × scale.
#[must_use]
pub fn picture_box(natural_w: f64, natural_h: f64, scale: Option<f64>, content_width_pt: f64) -> WatermarkBox {
    if natural_w <= 0.0 || natural_h <= 0.0 {
        return WatermarkBox { width_pt: content_width_pt, height_pt: content_width_pt };
    }
    match scale {
        Some(s) if truthy(s) => WatermarkBox { width_pt: round2((natural_w * s) / 100.0), height_pt: round2((natural_h * s) / 100.0) },
        _ => WatermarkBox { width_pt: round2(content_width_pt), height_pt: round2((content_width_pt * natural_h) / natural_w) },
    }
}

/// An item of the Watermark gallery (`WatermarkGalleryItem`): the text and the box (pt) per
/// dialog language.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct WatermarkGalleryItem {
    pub id: &'static str,
    /// confidential | disclaimers | urgent
    pub category: &'static str,
    pub text_pt: &'static str,
    pub text_en: &'static str,
    pub text_es: &'static str,
    pub layout: WatermarkLayout,
    pub box_pt: [f64; 2],
    pub box_en: [f64; 2],
    pub box_es: [f64; 2],
}

struct PtEn {
    id: &'static str,
    category: &'static str,
    pt: &'static str,
    en: &'static str,
    layout: WatermarkLayout,
    box_pt: [f64; 2],
    box_en: [f64; 2],
}

const fn gal(
    id: &'static str,
    category: &'static str,
    pt: &'static str,
    en: &'static str,
    layout: WatermarkLayout,
    box_pt: [f64; 2],
    box_en: [f64; 2],
) -> PtEn {
    PtEn { id, category, pt, en, layout, box_pt, box_en }
}

/// The 12 items of the gallery (the Word pt-BR type-8 blocks), Portuguese and English.
const GALLERY_PT_EN: [PtEn; 12] = {
    use WatermarkLayout::{Diagonal as D, Horizontal as H};
    [
        gal("confidential1", "confidential", "CONFIDENCIAL", "CONFIDENTIAL", D, [527.85, 131.95], [527.85, 131.95]),
        gal("confidential2", "confidential", "CONFIDENCIAL", "CONFIDENTIAL", H, [468.0, 117.0], [468.0, 117.0]),
        gal("donotcopy1", "confidential", "NÃO COPIAR", "DO NOT COPY", D, [494.9, 164.95], [494.9, 164.95]),
        gal("donotcopy2", "confidential", "NÃO COPIAR", "DO NOT COPY", H, [467.95, 155.95], [467.95, 155.95]),
        gal("draft1", "disclaimers", "RASCUNHO", "DRAFT", D, [412.4, 247.45], [412.4, 247.45]),
        gal("draft2", "disclaimers", "RASCUNHO", "DRAFT", H, [468.0, 280.8], [468.0, 280.8]),
        gal("sample1", "disclaimers", "EXEMPLO", "SAMPLE", D, [461.85, 197.95], [439.9, 219.95]),
        gal("sample2", "disclaimers", "EXEMPLO", "SAMPLE", H, [467.95, 200.55], [468.0, 234.0]),
        gal("asap1", "urgent", "ASAP", "ASAP", D, [377.05, 282.8], [377.05, 282.8]),
        gal("asap2", "urgent", "ASAP", "ASAP", H, [468.0, 351.0], [468.0, 351.0]),
        gal("urgent1", "urgent", "URGENTE", "URGENT", D, [439.9, 219.95], [439.9, 219.95]),
        gal("urgent2", "urgent", "URGENTE", "URGENT", H, [468.0, 234.0], [468.0, 234.0]),
    ]
};

/// The Spanish text of Word per item kind (`TEXT_ES`; no oracle, written by hand).
fn text_es(kind: &str) -> Option<&'static str> {
    Some(match kind {
        "confidential" => "CONFIDENCIAL",
        "donotcopy" => "NO COPIAR",
        "draft" => "BORRADOR",
        "sample" => "MUESTRA",
        "asap" => "ASAP",
        "urgent" => "URGENTE",
        _ => return None,
    })
}

/// `g.id.replace(/\d+$/, '')`.
pub fn strip_trailing_digits(id: &str) -> &str {
    id.trim_end_matches(|c: char| c.is_ascii_digit())
}

/// `WATERMARK_GALLERY`: the 12 items with Spanish. The Spanish box follows the English rule: the
/// block's own pt-BR box when the text has as many letters as the Spanish one; else the box of the
/// block with the same layout and that number of letters (NO COPIAR = RASCUNHO); with none, the
/// item's own.
#[must_use]
pub fn watermark_gallery() -> Vec<WatermarkGalleryItem> {
    GALLERY_PT_EN
        .iter()
        .map(|g| {
            let es = text_es(strip_trailing_digits(g.id)).unwrap_or(g.en);
            let same = if letters_len(g.pt) == letters_len(es) {
                Some(g)
            } else {
                GALLERY_PT_EN.iter().find(|o| o.layout == g.layout && letters_len(o.pt) == letters_len(es))
            };
            WatermarkGalleryItem {
                id: g.id,
                category: g.category,
                text_pt: g.pt,
                text_en: g.en,
                text_es: es,
                layout: g.layout,
                box_pt: g.box_pt,
                box_en: g.box_en,
                box_es: same.map_or(g.box_pt, |s| s.box_pt),
            }
        })
        .collect()
}

/// `WATERMARK_TEXTS.en`: the dialog's list.
pub const WATERMARK_TEXTS_EN: [&str; 10] =
    ["ASAP", "CONFIDENTIAL", "COPY", "DO NOT COPY", "DRAFT", "ORIGINAL", "PERSONAL", "SAMPLE", "TOP SECRET", "URGENT"];
/// `WATERMARK_TEXTS.pt`.
pub const WATERMARK_TEXTS_PT: [&str; 10] =
    ["ASAP", "CONFIDENCIAL", "CÓPIA", "EXEMPLO", "NÃO COPIAR", "ORIGINAL", "PESSOAL", "RASCUNHO", "ULTRASSECRETO", "URGENTE"];
/// `WATERMARK_TEXTS.es`.
pub const WATERMARK_TEXTS_ES: [&str; 10] =
    ["ALTO SECRETO", "ASAP", "BORRADOR", "CONFIDENCIAL", "COPIA", "MUESTRA", "NO COPIAR", "ORIGINAL", "PERSONAL", "URGENTE"];

/// `WATERMARK_TEXTS[lang]` for `en`, `pt` or `es`.
#[must_use]
pub fn watermark_texts(lang: &str) -> Option<&'static [&'static str; 10]> {
    match lang {
        "en" => Some(&WATERMARK_TEXTS_EN),
        "pt" => Some(&WATERMARK_TEXTS_PT),
        "es" => Some(&WATERMARK_TEXTS_ES),
        _ => None,
    }
}
