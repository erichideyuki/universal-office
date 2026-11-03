//! No-panic loops of frontier R4b: every public function of `read::inline_objects` and
//! `read::picture_format` over the quirk corpus, seeded mutations of it (a character replaced,
//! removed, duplicated, a junk string or a surrogate-pair half inserted, a tag cut), every element
//! of every parse (so each function also sees elements it was not written for), documents with
//! hostile numeric attributes, deeply nested groups and text boxes, and random strings for the
//! string helpers. A panic fails the test; the result is not looked at (the parity tests look at
//! it) except for two invariants: the JSON of a result is produced, and the sync of an edited raw
//! text leaves the object with the same kind.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

#[path = "support/r4b_host.rs"]
mod r4b_host;

use std::collections::HashMap;

use r4b_host::{QuirkHost, empty_package, quirk_cases};
use uo_docx::js::{JsValue, JsonWriter};
use uo_docx::model::run::RunExt;
use uo_docx::read::DOCX;
use uo_docx::read::inline_objects::{
    has_bold_weight, is_diagram_uri, read_drawing_run, read_object_runs, read_vml_runs, strip_leading_hash, strip_vml_prefix,
    sync_image_from_xml, sync_shape_from_xml,
};
use uo_docx::read::picture_format::{
    parse_drawing, parts_of, pic_bool, pic_int, read_3d, read_blip, read_dml_color, read_dml_color_element, read_effects, read_pic_line,
    read_picture_format,
};
use uo_opc::Package;
use uo_opc::xml::{Document, parse};

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

fn json_all(run: &RunExt, host: &QuirkHost<'_>) -> usize {
    let mut n = 0;
    for v in [
        run.image.as_deref().map(|x| x as &dyn JsValue),
        run.textbox.as_deref().map(|x| x as &dyn JsValue),
        run.shape.as_deref().map(|x| x as &dyn JsValue),
        run.group.as_deref().map(|x| x as &dyn JsValue),
    ]
    .into_iter()
    .flatten()
    {
        n += JsonWriter::stringify(v, Some(&host.ctx.media)).map_or(0, |s| s.len());
    }
    n
}

fn kind(run: &RunExt) -> u8 {
    u8::from(run.image.is_some())
        | u8::from(run.textbox.is_some()) << 1
        | u8::from(run.shape.is_some()) << 2
        | u8::from(run.group.is_some()) << 3
}

fn read_all(d: &Document, pkg: &Package, parts: &HashMap<String, String>) -> usize {
    let root = d.root();
    let mut n = 0;
    for el in std::iter::once(root).chain(root.descendants()) {
        let mut host = QuirkHost::new(pkg, parts.clone());
        // every element is offered to every reader
        if let Ok(runs) = read_object_runs(el, &mut host) {
            for r in &runs {
                n += json_all(r, &host);
            }
        }
        for inline in [false, true] {
            if let Ok(runs) = read_vml_runs(el, inline, &mut host) {
                for r in &runs {
                    n += json_all(r, &host);
                }
            }
        }
        if let Ok(Some(run)) = read_drawing_run(el, el.raw(), &mut host) {
            n += json_all(&run, &host);
            let before = kind(&run);
            // a sync of an edited raw text keeps the kind of the object
            let mut run = run;
            let raw = el.raw().unwrap_or("").replacen("<wp:inline", "<wp:anchor", 1).replacen("</wp:inline>", "</wp:anchor>", 1);
            if let Some(i) = run.image.as_mut() {
                i.raw_drawing_xml = Some(raw.as_str().into());
                sync_image_from_xml(i);
            } else {
                if let Some(t) = run.textbox.as_mut() {
                    t.raw_drawing_xml = Some(raw.as_str().into());
                } else if let Some(s) = run.shape.as_mut() {
                    s.raw_drawing_xml = Some(raw.as_str().into());
                } else if let Some(g) = run.group.as_mut() {
                    g.raw_drawing_xml = Some(raw.as_str().into());
                }
                sync_shape_from_xml(&mut run);
            }
            assert_eq!(kind(&run), before);
            n += json_all(&run, &host);
        }
        let text = el.source_slice().unwrap_or("");
        n += usize::from(read_picture_format(text, true).is_some());
        n += usize::from(read_picture_format(el.raw().unwrap_or(""), false).is_some());
        n += usize::from(read_dml_color(Some(el)).is_some());
        n += usize::from(read_dml_color_element(el).val.is_empty());
        n += usize::from(read_pic_line(Some(el)).is_some());
        n += usize::from(read_3d(Some(el), el.first_child("a:sp3d")).is_some());
        n += usize::from(read_blip(Some(el)).lum.is_some());
        n += usize::from(read_effects(Some(el)).shadow.is_some());
        if let Some(pd) = parse_drawing(text) {
            n += usize::from(parts_of(&pd).is_some());
        }
    }
    n
}

fn mutate(rng: &mut Rng, text: &str) -> String {
    let mut chars: Vec<char> = text.chars().collect();
    for _ in 0..1 + rng.below(4) {
        if chars.is_empty() {
            break;
        }
        let at = rng.below(chars.len());
        match rng.below(7) {
            0 => chars[at] = ['"', '<', '>', '/', '=', ';', ':', ' ', '\u{a0}', '\u{0}', '😀', 'é', '&', '\''][rng.below(14)],
            1 => {
                chars.remove(at);
            }
            2 => chars.insert(at, chars[at]),
            3 => {
                let junk =
                    ["#😀", "1e999", "--", "99999999999999999999", "-0", "NaN", "&#xD800;", "\u{feff}", "px", "%", "f", ".", ",", "\\"]
                        [rng.below(14)];
                for (k, c) in junk.chars().enumerate() {
                    chars.insert(at + k, c);
                }
            }
            4 => chars.truncate(at),
            5 => {
                let to = (at + 1 + rng.below(40)).min(chars.len());
                chars.drain(at..to);
            }
            _ => {
                let other = rng.below(chars.len());
                chars.swap(at, other);
            }
        }
    }
    chars.into_iter().collect()
}

#[test]
fn quirk_corpus_does_not_panic() {
    let pkg = empty_package();
    let mut total = 0;
    for (_, frag, parts) in quirk_cases() {
        if let Ok(d) = parse(frag.as_str(), &DOCX) {
            total += read_all(&d, &pkg, &parts);
        }
    }
    assert!(total > 0);
}

#[test]
fn mutated_corpus_does_not_panic() {
    let pkg = empty_package();
    let cases = quirk_cases();
    let mut rng = Rng(0x2545_f491);
    let (mut parsed, mut total) = (0, 0);
    for _ in 0..6000 {
        let (_, base, parts) = &cases[rng.below(cases.len())];
        let t = mutate(&mut rng, base);
        if let Ok(d) = parse(t.as_str(), &DOCX) {
            parsed += 1;
            total += read_all(&d, &pkg, parts);
        }
        total += usize::from(read_picture_format(&t, true).is_some());
    }
    assert!(parsed > 2000 && total > 0, "{parsed} {total}");
}

const HOSTILE: [&str; 16] = [
    "1e999",
    "-1e999",
    "99999999999999999999999999999",
    "NaN",
    "-0",
    "0x10",
    "",
    " 12 ",
    "12pt",
    "-5",
    "2147483648",
    "-2147483649",
    "0",
    "1",
    ".5",
    "1e-9",
];

/// Documents whose numeric attributes are hostile.
fn hostile_docs(rng: &mut Rng) -> String {
    let h = |rng: &mut Rng| HOSTILE[rng.below(HOSTILE.len())];
    let pic = |rng: &mut Rng| {
        format!(
            "<pic:pic><pic:blipFill><a:blip r:embed=\"rId1\"><a:lum bright=\"{}\" contrast=\"{}\"/><a:biLevel thresh=\"{}\"/><a:duotone><a:srgbClr><a:alpha val=\"{}\"/></a:srgbClr></a:duotone></a:blip><a:srcRect l=\"{}\" t=\"{}\"/></pic:blipFill><pic:spPr><a:xfrm rot=\"{}\" flipH=\"1\"><a:off x=\"{}\" y=\"{}\"/><a:ext cx=\"{}\" cy=\"{}\"/></a:xfrm><a:ln w=\"{}\"><a:solidFill><a:schemeClr val=\"accent1\"><a:tint val=\"{}\"/><a:shade val=\"{}\"/></a:schemeClr></a:solidFill></a:ln></pic:spPr></pic:pic>",
            h(rng),
            h(rng),
            h(rng),
            h(rng),
            h(rng),
            h(rng),
            h(rng),
            h(rng),
            h(rng),
            h(rng),
            h(rng),
            h(rng),
            h(rng),
            h(rng)
        )
    };
    let wsp = |rng: &mut Rng| {
        format!(
            "<wps:wsp><wps:spPr><a:xfrm rot=\"{}\" flipV=\"1\"/><a:prstGeom prst=\"roundRect\"/><a:solidFill><a:schemeClr val=\"accent1\"><a:tint val=\"{}\"/></a:schemeClr></a:solidFill></wps:spPr><wps:style><a:fontRef idx=\"minor\"><a:schemeClr val=\"lt1\"><a:shade val=\"{}\"/></a:schemeClr></a:fontRef></wps:style><wps:txbx id=\"{}\"><w:txbxContent><w:p/></w:txbxContent></wps:txbx><wps:bodyPr lIns=\"{}\" tIns=\"{}\" rIns=\"{}\"><a:spAutoFit/></wps:bodyPr></wps:wsp>",
            h(rng),
            h(rng),
            h(rng),
            h(rng),
            h(rng),
            h(rng),
            h(rng)
        )
    };
    let body = match rng.below(4) {
        0 => pic(rng),
        1 => wsp(rng),
        2 => format!(
            "<wpg:wgp><wpg:grpSpPr><a:xfrm rot=\"{}\" flipH=\"1\"><a:chOff x=\"{}\" y=\"{}\"/><a:chExt cx=\"{}\" cy=\"{}\"/><a:ext cx=\"{}\" cy=\"{}\"/></a:xfrm></wpg:grpSpPr>{}{}</wpg:wgp>",
            h(rng),
            h(rng),
            h(rng),
            h(rng),
            h(rng),
            h(rng),
            h(rng),
            wsp(rng),
            pic(rng)
        ),
        _ => format!(
            "<wps:wsp><wps:spPr><a:blipFill><a:blip r:embed=\"rId1\"><a:alphaModFix amt=\"{}\"/></a:blip><a:tile tx=\"{}\" sx=\"{}\"/></a:blipFill></wps:spPr></wps:wsp>",
            h(rng),
            h(rng),
            h(rng)
        ),
    };
    let wp = if rng.below(2) == 0 { "wp:inline" } else { "wp:anchor" };
    let vml = format!(
        "<w:pict><v:shape style=\"position:absolute;width:{}pt;height:{}pt;rotation:{};z-index:{}\" fillcolor=\"#f00\"><v:imagedata r:id=\"rId1\" cropleft=\"{}\" gain=\"{}\" blacklevel=\"{}\"/></v:shape><v:group style=\"width:{}pt;height:{}pt\" coordsize=\"{},{}\" coordorigin=\"{},{}\"><v:rect style=\"left:{};top:{};width:{};height:{}\"/></v:group><v:line from=\"{},{}\" to=\"{}pt,{}\"/></w:pict>",
        h(rng),
        h(rng),
        h(rng),
        h(rng),
        h(rng),
        h(rng),
        h(rng),
        h(rng),
        h(rng),
        h(rng),
        h(rng),
        h(rng),
        h(rng),
        h(rng),
        h(rng),
        h(rng),
        h(rng),
        h(rng),
        h(rng),
        h(rng),
        h(rng)
    );
    format!(
        "<w:root><w:drawing><{wp} distT=\"{}\" behindDoc=\"1\"><wp:extent cx=\"{}\" cy=\"{}\"/><wp:effectExtent l=\"{}\" t=\"{}\"/><wp:positionH relativeFrom=\"page\"><wp:posOffset>{}</wp:posOffset></wp:positionH><a:graphic><a:graphicData uri=\"x\">{body}</a:graphicData></a:graphic></{wp}></w:drawing>{vml}</w:root>",
        h(rng),
        h(rng),
        h(rng),
        h(rng),
        h(rng),
        h(rng)
    )
}

#[test]
fn hostile_numbers_do_not_panic() {
    let pkg = empty_package();
    let mut rng = Rng(0x00c0_ffee);
    let mut total = 0;
    for _ in 0..3000 {
        let t = hostile_docs(&mut rng);
        let d = parse(t.as_str(), &DOCX).unwrap();
        total += read_all(&d, &pkg, &HashMap::new());
    }
    assert!(total > 0);
}

fn nested(depth: usize, open: &str, close: &str, leaf: &str) -> String {
    format!("{}{leaf}{}", open.repeat(depth), close.repeat(depth))
}

#[test]
fn deep_nesting_does_not_overflow_the_stack() {
    let pkg = empty_package();
    // groups nested 900 deep (the XML depth limit is 1,000) and text boxes in text boxes
    let leaf = "<wps:wsp><wps:spPr><a:solidFill><a:srgbClr val=\"FF0000\"/></a:solidFill></wps:spPr></wps:wsp>";
    let groups = nested(300, "<wpg:grpSp><wpg:grpSpPr/>", "</wpg:grpSp>", leaf);
    let t = format!(
        "<w:root><w:drawing><wp:inline><wp:extent cx=\"635\" cy=\"635\"/><a:graphic><a:graphicData uri=\"x\"><wpg:wgp><wpg:grpSpPr/>{groups}</wpg:wgp></a:graphicData></a:graphic></wp:inline></w:drawing></w:root>"
    );
    let d = parse(t.as_str(), &DOCX).unwrap();
    assert!(read_all(&d, &pkg, &HashMap::new()) > 0);
    // text boxes nested through the host's reader: the stub reads flat, so nest by VML groups of w:pict
    let mut s = String::from("<w:root>");
    s.push_str(&nested(
        250,
        "<w:pict><v:group style=\"width:10pt;height:10pt\">",
        "</v:group></w:pict>",
        "<v:rect style=\"width:1pt;height:1pt\" fillcolor=\"red\"/>",
    ));
    s.push_str("</w:root>");
    let d = parse(s.as_str(), &DOCX).unwrap();
    assert!(read_all(&d, &pkg, &HashMap::new()) > 0);
}

#[test]
fn string_helpers_do_not_panic() {
    let mut rng = Rng(0x1234_5678);
    let alphabet: Vec<char> = "ab#:/\"' \t\n6789000boldvtalicxyz-.1e+😀é\u{a0}".chars().collect();
    for _ in 0..30_000 {
        let n = rng.below(24);
        let s: String = (0..n).map(|_| alphabet[rng.below(alphabet.len())]).collect();
        let _ = (has_bold_weight(&s), is_diagram_uri(&s), strip_leading_hash(&s), strip_vml_prefix(&s));
        let _ = (pic_int(Some(&s), 3.0), pic_bool(Some(&s), true), pic_int(None, 1.0));
        let _ = read_picture_format(&s, true);
    }
}
