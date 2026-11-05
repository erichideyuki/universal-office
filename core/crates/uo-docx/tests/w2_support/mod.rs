//! The named edits of `js/capture-object-edits.mjs` on a read document (the top-level paragraphs of the
//! body), shared by the parity, memory and no-panic tests of frontier W2.
#![allow(dead_code, clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use uo_docx::js::{JsNumber, Str};
use uo_docx::model::Document;
use uo_docx::model::run::{OleData, Run};

pub const EDITS: &[&str] =
    &["shapes", "shapes-size", "vml-boxes", "groups", "groups-hide", "images", "images-size", "images-swap", "noraw", "noraw-ole"];

fn add(x: Option<JsNumber>, d: f64) -> Option<JsNumber> {
    x.map(|v| JsNumber::Num(v.to_number() + d))
}

/// `s.replace('a', 'b')`: the first occurrence.
fn replace_first(s: &str, a: &str, b: &str) -> String {
    s.replacen(a, b, 1)
}

/// `raw.replace(/r:embed="[^"]*"/, 'r:embed="uw:media"')`.
fn swap_embed(raw: &str) -> String {
    const OPEN: &str = "r:embed=\"";
    if let Some(i) = raw.find(OPEN)
        && let Some(q) = raw[i + OPEN.len()..].find('"')
    {
        return format!("{}r:embed=\"uw:media\"{}", &raw[..i], &raw[i + OPEN.len() + q + 1..]);
    }
    raw.to_owned()
}

/// Whether the edit touches the run (the JS functions' return value).
fn touches(r: &Run, edit: &str) -> bool {
    let Some(e) = r.ext() else { return false };
    let tb = e.textbox.as_deref().is_some_and(|t| t.raw_drawing_xml.is_some());
    let sh = e.shape.as_deref().is_some_and(|s| s.raw_drawing_xml.is_some());
    match edit {
        "shapes" | "shapes-size" => tb || sh,
        "vml-boxes" => e.textbox.is_some(),
        "groups" | "groups-hide" => e.group.as_deref().is_some_and(|g| g.raw_drawing_xml.is_some()),
        "images" | "images-size" | "images-swap" => e.image.as_deref().is_some_and(|i| i.raw_drawing_xml.is_some()),
        "noraw" | "noraw-ole" => e.image.is_some(),
        other => panic!("{other}"),
    }
}

fn edit_run(r: &mut Run, edit: &str) {
    let e = r.ext_mut();
    let size = edit.ends_with("-size");
    match edit {
        "shapes" | "shapes-size" => {
            if let Some(t) = e.textbox.as_deref_mut()
                && t.raw_drawing_xml.is_some()
            {
                t.edited = Some(true);
                if size {
                    t.width_twips = JsNumber::Num(t.width_twips.to_number() + 100.0);
                    t.height_twips = JsNumber::Num(t.height_twips.to_number() + 50.0);
                    t.offset_x_twips = add(t.offset_x_twips, 10.0);
                    t.offset_y_twips = add(t.offset_y_twips, -20.0);
                }
            }
            if let Some(s) = e.shape.as_deref_mut()
                && s.raw_drawing_xml.is_some()
            {
                s.edited = Some(true);
                if size {
                    s.width_twips = JsNumber::Num(s.width_twips.to_number() + 100.0);
                    s.height_twips = JsNumber::Num(s.height_twips.to_number() + 50.0);
                }
            }
        }
        "vml-boxes" => {
            if let Some(t) = e.textbox.as_deref_mut() {
                t.vml = Some(true);
            }
        }
        "groups" | "groups-hide" => {
            if let Some(g) = e.group.as_deref_mut()
                && let Some(raw) = g.raw_drawing_xml.clone()
            {
                g.edited = Some(true);
                if edit == "groups-hide" {
                    let x = replace_first(&raw, "<wp:docPr ", "<wp:docPr hidden=\"1\" ");
                    g.raw_drawing_xml = Some(Str::from(replace_first(&x, "behindDoc=\"0\"", "behindDoc=\"1\"")));
                }
            }
        }
        "images" | "images-size" | "images-swap" => {
            if let Some(i) = e.image.as_deref_mut()
                && let Some(raw) = i.raw_drawing_xml.clone()
            {
                i.edited = Some(true);
                if size {
                    i.width_twips = JsNumber::Num(i.width_twips.to_number() + 100.0);
                    i.height_twips = JsNumber::Num(i.height_twips.to_number() + 50.0);
                    i.offset_x_twips = add(i.offset_x_twips, 10.0);
                    i.offset_y_twips = add(i.offset_y_twips, -20.0);
                }
                if edit == "images-swap" {
                    i.raw_drawing_xml = Some(Str::from(swap_embed(&raw)));
                }
            }
        }
        "noraw" | "noraw-ole" => {
            if let Some(i) = e.image.as_deref_mut() {
                i.raw_drawing_xml = None;
                if edit == "noraw-ole" {
                    i.ole = Some(Box::new(OleData {
                        prog_id: Str::from("Excel.Sheet.12"),
                        data: i.data,
                        ext: Str::from("xlsx"),
                        ..OleData::default()
                    }));
                }
            }
        }
        other => panic!("{other}"),
    }
}

/// Applies the edit to the top-level paragraphs of the body; the number of runs touched.
pub fn apply(doc: &mut Document, edit: &str) -> usize {
    let mut hit = 0;
    for pi in 0..doc.body.paras.len() {
        if !doc.body.paras.get(pi).is_some_and(|p| p.runs.iter().any(|r| touches(r, edit))) {
            continue;
        }
        let Some(p) = doc.body.paras.make_mut(pi) else { continue };
        for r in &mut p.runs {
            if touches(r, edit) {
                edit_run(r, edit);
                hit += 1;
            }
        }
    }
    hit
}
