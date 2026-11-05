//! No-panic loops of frontier W5 (`write::emit::background`): random and mutated texts through every
//! function (colour and number texts, the XML of backgrounds read by R3, document, relationship and
//! content-type parts of the package edit), seeded. A panic fails the test; the results are not looked
//! at (the parity test does) except for the invariants named in the checks.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::sync::Arc;

use uo_docx::model::media::{ImageData, MediaRef, MediaStore};
use uo_docx::read::DOCX;
use uo_docx::read::page_background::read_page_background;
use uo_docx::write::emit::background::{
    WatermarkBox, WatermarkIds, WatermarkLayout, WatermarkPictureSpec, WatermarkSpec, WatermarkTextSpec, add_default_extension,
    auto_text_box, explicit_text_box, fill_color2_match, first_open_tag, first_r_id, one_color_gradient_color2, page_background_xml,
    pattern_gif, picture_box, rels_path_of, resolve_color2, vml_color_attr, watermark_run_xml, write_page_background,
};
use uo_jsnum::parse::to_number;
use uo_opc::Package;
use uo_opc::package::OpenOptions;
use uo_opc::xml::parse;

struct Rng(u32);

impl Rng {
    fn next(&mut self) -> u32 {
        let mut s = self.0;
        s ^= s << 13;
        s ^= s >> 17;
        s ^= s << 5;
        self.0 = s;
        s
    }

    fn below(&mut self, n: usize) -> usize {
        (self.next() as usize) % n.max(1)
    }
}

const PIECES: &[&str] = &[
    "<",
    ">",
    "/>",
    "\"",
    "'",
    "&",
    "&amp;",
    "$",
    "$&",
    "$`",
    "$'",
    "$1",
    "$$",
    "\\",
    " ",
    "\t",
    "\n",
    "é",
    "😀",
    "\u{2028}",
    "\u{feff}",
    "#",
    "fill",
    "darken(",
    "lighten(",
    ")",
    "(",
    "0",
    "1",
    "255",
    "99999999999999999999",
    "-",
    ".",
    "e",
    "NaN",
    "Infinity",
    "rId",
    "rId5",
    "r:id=\"",
    "Id=\"",
    "<w:background",
    "</w:background>",
    "<w:document",
    "<Types",
    "</Types>",
    "<Relationship",
    "</Relationships>",
    "xmlns:v=\"x\"",
    "w:color=\"",
    "v:fill",
    "type=\"frame\"",
    "type=\"gradient\"",
    "angle=\"",
    "focus=\"",
    "focusposition=\"",
    "colors=\"",
    "color2=\"",
    "red",
    "#abc",
    "silver",
];

fn junk(r: &mut Rng, max: usize) -> String {
    (0..r.below(max)).map(|_| PIECES[r.below(PIECES.len())]).collect()
}

fn mutate(r: &mut Rng, s: &str) -> String {
    let mut out: Vec<char> = s.chars().collect();
    for _ in 0..1 + r.below(4) {
        let at = r.below(out.len() + 1);
        match r.below(4) {
            0 if !out.is_empty() => {
                out.remove(at.min(out.len() - 1));
            }
            1 => {
                for c in PIECES[r.below(PIECES.len())].chars().rev() {
                    out.insert(at, c);
                }
            }
            2 if !out.is_empty() => out.truncate(at),
            _ => {}
        }
    }
    out.into_iter().collect()
}

const BASE_BG: &str = "<w:background xmlns:w=\"w\" xmlns:v=\"v\" xmlns:o=\"o\" xmlns:r=\"r\" w:color=\"FF0000\"><v:background id=\"_x0000_s1025\" fillcolor=\"red\"><v:fill type=\"gradient\" r:id=\"rId2\" o:title=\"t\" color2=\"fill darken(5)\" angle=\"-90\" focus=\"50%\" focusposition=\".5,1\" colors=\"0 red;1 blue\" method=\"linear sigma\"/></v:background></w:background>";
const BASE_DOC: &str = "<?xml version=\"1.0\"?><w:document xmlns:w=\"w\"><w:background w:color=\"00FF00\"><v:fill r:id=\"rId5\"/></w:background><w:body/></w:document>";
const BASE_RELS: &str = "<Relationships xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\"><Relationship Id=\"rId5\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/image\" Target=\"media/image1.png\"/></Relationships>";
const BASE_CT: &str = "<Types xmlns=\"http://schemas.openxmlformats.org/package/2006/content-types\"><Default Extension=\"xml\" ContentType=\"application/xml\"/></Types>";

#[test]
fn text_functions_never_panic() {
    let mut r = Rng(0x5eed_0c01);
    for i in 0..6000 {
        let s = junk(&mut r, 12);
        let _ = vml_color_attr(&s);
        let _ = resolve_color2(Some(&s), &junk(&mut r, 6));
        let _ = resolve_color2(Some(&format!("fill darken({s})")), &s);
        let _ = fill_color2_match(&s);
        let _ = rels_path_of(&s);
        let _ = first_r_id(&s);
        let _ = first_open_tag(&s, "Types");
        let _ = first_open_tag(&s, "w:document");
        let _ = add_default_extension(&s, &junk(&mut r, 4));
        let _ = add_default_extension(&format!("<Types {s}>"), "a$'b$&$`");
        let n = to_number(&s);
        let _ = one_color_gradient_color2(n);
        let rows: Vec<f64> = (0..r.below(12)).map(|_| to_number(&junk(&mut r, 3))).collect();
        let g = pattern_gif(&rows);
        assert!(g.starts_with(b"GIF87a") && g.ends_with(&[0, 0x3b]), "a GIF {i}");
        let _ = auto_text_box(&s, WatermarkLayout::Diagonal, n);
        let _ = auto_text_box(&s, WatermarkLayout::Horizontal, n);
        let _ = explicit_text_box(n, n, n);
        let _ = picture_box(n, to_number(&junk(&mut r, 3)), Some(n), n);
        let b = WatermarkBox { width_pt: n, height_pt: n };
        let ids = WatermarkIds { n, spid: n, anchor_id: s.clone() };
        let text = WatermarkSpec::Text(WatermarkTextSpec {
            text: s.clone(),
            font: s.clone(),
            size_pt: Some(n),
            color: s.clone(),
            semitransparent: r.below(2) == 0,
            layout: WatermarkLayout::Diagonal,
            lang: None,
        });
        let _ = watermark_run_xml(&text, b, &ids, Some(&s), None);
        let pic = WatermarkSpec::Picture(WatermarkPictureSpec {
            data: Arc::from(vec![0u8]),
            ext: s.clone(),
            title: s.clone(),
            scale: None,
            washout: true,
        });
        let _ = watermark_run_xml(&pic, b, &ids, None, Some(&|h: &str| h.to_owned()));
    }
}

#[test]
fn backgrounds_and_package_edits_never_panic() {
    let mut r = Rng(0x5eed_0c02);
    let (mut wrote, mut refused) = (0, 0);
    for _ in 0..4000 {
        let xml = mutate(&mut r, BASE_BG);
        let mut media = MediaStore::new();
        let bg = match parse(&xml, &DOCX) {
            Ok(doc) => read_page_background(doc.root(), &mut |rid| {
                (rid.len() % 2 == 0).then(|| ImageData {
                    data: MediaRef(media.add(rid.as_bytes())),
                    ext: "PNG".into(),
                    ..ImageData::default()
                })
            }),
            Err(_) => None,
        };
        if let Some(bg) = &bg {
            let _ = page_background_xml(bg, Some("rId9"));
            let _ = page_background_xml(bg, None);
        }
        let doc = mutate(&mut r, BASE_DOC);
        let rels = mutate(&mut r, BASE_RELS);
        let ct = mutate(&mut r, BASE_CT);
        let mut parts: Vec<(String, Vec<u8>)> = vec![("word/document.xml".into(), doc.into_bytes())];
        if r.below(5) != 0 {
            parts.push(("word/_rels/document.xml.rels".into(), rels.into_bytes()));
        }
        if r.below(5) != 0 {
            parts.push(("[Content_Types].xml".into(), ct.into_bytes()));
        }
        let mut pkg = Package::from_parts(parts, &uo_opc::Limits::DEFAULT, &OpenOptions::default());
        let mut prev = None;
        for _ in 0..3 {
            match write_page_background(&mut pkg, &media, bg.as_ref(), prev.as_ref()) {
                Ok(added) => {
                    wrote += 1;
                    prev = added;
                }
                Err(_) => refused += 1,
            }
        }
    }
    println!("package edits: {wrote} written, {refused} refused");
    assert!(wrote > 1000);
}
