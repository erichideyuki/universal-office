//! Unit tests of frontier R2 (`read::parts`, `read::styles`, `read::table_styles`,
//! `read::style_names_es`): what each reader returns on small inputs whose TS results are known
//! (the parity tests compare the whole corpus), the error paths, and the edge cases named in
//! `core/docs/modules/uo-docx-d1a/R2.md`.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use uo_docx::DocxError;
use uo_docx::error::DocxLimitKind;
use uo_docx::js::{JsNumber, JsObject, JsRecord, Slot, Str};
use uo_docx::limits::DocxLimits;
use uo_docx::read::ReadOptions;
use uo_docx::read::ctx::ReadCtx;
use uo_docx::read::parts::{
    numbering_images, parse_text, part_by_rel_type, read_doc_formatting, read_font_table, read_font_table_text, read_note_props,
    read_numbering, read_numbering_text, read_settings, read_settings_text, read_style_sheet, read_style_sheet_text, read_theme,
    read_theme_text, target_ext, vml_len_pt,
};
use uo_docx::read::style_names_es::{builtin_style_name_es, table_style_name_es};
use uo_docx::read::styles::{read_style_para_props, read_style_run_props, read_styles, read_styles_text, read_styles_xml};
use uo_docx::read::table_styles::{
    CORE_TABLE_STYLES, builtin_table_styles, local_style_id, materialize_style_xml, style_def_of, style_id_by_name, theme_hex,
};
use uo_opc::Package;
use uo_opc::xml::{ParseOptions, parse};

const W: &str = r#"xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main""#;
const A: &str = r#"xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main""#;

fn n(x: f64) -> Option<JsNumber> {
    Some(JsNumber::Num(x))
}

fn keys<T: JsObject>(o: &T) -> Vec<&'static str> {
    o.slots_in_order().into_iter().map(|(k, _)| k).collect()
}

fn package(parts: &[(&str, &str)]) -> Package {
    Package::from_parts(
        parts.iter().map(|(p, t)| ((*p).to_owned(), t.as_bytes().to_vec())),
        &uo_opc::Limits::DEFAULT,
        &uo_opc::package::OpenOptions::default(),
    )
}

fn colors(pairs: &[(&str, &str)]) -> JsRecord<Str> {
    let mut r = JsRecord::new();
    for (k, v) in pairs {
        r.insert(k, (*v).into());
    }
    r
}

// ───────────────────────── vml, extensions ─────────────────────────

#[test]
fn vml_lengths() {
    assert_eq!(vml_len_pt("width:10pt;height:5in", "width"), Some(10.0));
    assert_eq!(vml_len_pt("width:10pt;height:5in", "height"), Some(360.0));
    assert_eq!(vml_len_pt("width:8", "width"), Some(6.0));
    assert_eq!(vml_len_pt(" ; width : 3", "width"), None);
    assert_eq!(vml_len_pt("max-width:5pt", "width"), None);
    assert_eq!(vml_len_pt(";width:1.2.3mm", "width"), Some(1.2 * (72.0 / 25.4)));
    assert_eq!(vml_len_pt("width:.", "width"), None);
    assert_eq!(vml_len_pt("", "width"), None);
    assert_eq!(vml_len_pt(&format!("width:{}pt", "9".repeat(400)), "width"), None);
}

#[test]
fn target_extensions() {
    assert_eq!(target_ext("media/a.PNG"), "png");
    assert_eq!(target_ext("a.tar.gz"), "gz");
    assert_eq!(target_ext("a."), "png");
    assert_eq!(target_ext("a"), "png");
    assert_eq!(target_ext("a.png\n"), "png");
}

// ───────────────────────── styles.xml ─────────────────────────

#[test]
fn style_sheet_reads_definitions_and_defaults() {
    let xml = format!(
        r#"<w:styles {W}><w:docDefaults><w:rPrDefault><w:rPr><w:sz w:val="22"/></w:rPr></w:rPrDefault><w:pPrDefault><w:pPr><w:jc w:val="both"/></w:pPr></w:pPrDefault></w:docDefaults>
<w:style w:type="table" w:default="1" w:styleId="T1"><w:name w:val="Normal Table"/><w:tblStylePr w:type="firstRow"><w:rPr><w:b/></w:rPr></w:tblStylePr><w:tblStylePr w:type="__proto__"><w:rPr><w:i/></w:rPr></w:tblStylePr></w:style>
<w:style w:styleId="P1" w:default="true"><w:name w:val=""/><w:basedOn w:val=""/><w:next w:val="P2"/></w:style>
<w:style w:type="paragraph" w:styleId="P2" w:default="on"/><w:style w:type="bogus" w:styleId="X"/><w:style w:type="paragraph"/>
<w:style w:type="paragraph" w:styleId="P1"><w:name w:val="again"/></w:style><w:style w:type="character" w:styleId="__proto__"/><w:style w:type="character" w:styleId="5"/></w:styles>"#
    );
    let doc = parse(xml, &uo_docx::read::DOCX).unwrap();
    let sheet = read_style_sheet(Some(&doc));
    assert!(sheet.doc_defaults.r_pr.is_some() && sheet.doc_defaults.p_pr.is_some());
    assert_eq!(sheet.styles.keys().collect::<Vec<_>>(), ["5", "T1", "P1", "P2"]);
    let p1 = sheet.styles.get("P1").unwrap();
    assert_eq!(p1.name.as_deref(), Some("again"));
    assert!(p1.based_on.is_none() && p1.next.is_none() && p1.is_default.is_none());
    let t1 = sheet.styles.get("T1").unwrap();
    assert_eq!(t1.is_default, Some(true));
    assert_eq!(t1.tbl_style_pr.as_ref().unwrap().keys().collect::<Vec<_>>(), ["firstRow"]);
    assert_eq!(sheet.defaults.table.as_deref(), Some("T1"));
    assert_eq!(sheet.defaults.paragraph.as_deref(), Some("P1"));
    assert_eq!(keys(&sheet.defaults), ["table", "paragraph"]);
    assert!(sheet.defaults.character.is_none());
}

#[test]
fn style_sheet_without_a_part_is_empty_and_the_text_entry_point_errors() {
    let sheet = read_style_sheet(None);
    assert!(sheet.styles.is_empty() && sheet.doc_defaults.r_pr.is_none());
    assert!(read_style_sheet_text("").unwrap().styles.is_empty());
    assert!(parse_text("").unwrap().is_none());
    assert!(matches!(read_style_sheet_text("<w:styles"), Err(DocxError::Package(_))));
    assert_eq!(read_style_sheet_text(&format!("<w:styles {W}><w:style w:styleId=\"A\"/></w:styles>")).unwrap().styles.len(), 1);
}

// ───────────────────────── numbering.xml ─────────────────────────

#[test]
fn numbering_levels_overrides_and_picture_bullets() {
    let xml = format!(
        r#"<w:numbering {W} {A} xmlns:v="urn:schemas-microsoft-com:vml" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships">
<w:numPicBullet w:numPicBulletId="3"><w:pict><v:shape style="width:10pt;height:5in"><v:imagedata r:id="rId1"/></v:shape></w:pict></w:numPicBullet>
<w:numPicBullet w:numPicBulletId="x"><a:blip r:embed="rId2"/></w:numPicBullet>
<w:abstractNum w:abstractNumId="7"><w:multiLevelType w:val="multilevel"/><w:lvl w:ilvl="1"><w:start w:val="5"/><w:numFmt w:val="decimal" w:format="001"/><w:isLgl/><w:suff w:val="space"/><w:lvlText w:val="%1" w:null="true"/></w:lvl><w:lvl w:ilvl="0"><w:lvlJc w:val="left"/></w:lvl><w:lvl w:ilvl="-0"/></w:abstractNum>
<w:num w:numId="2"><w:abstractNumId w:val="7"/><w:lvlOverride w:ilvl="1"><w:startOverride w:val="3"/><w:lvl w:ilvl="1"/></w:lvlOverride></w:num></w:numbering>"#
    );
    let doc = parse(xml, &uo_docx::read::DOCX).unwrap();
    let out = read_numbering(Some(&doc), &DocxLimits::DEFAULT).unwrap();
    assert_eq!(out.pic_bullets.keys().collect::<Vec<_>>(), ["3", "NaN"]);
    let pb = out.pic_bullets.get("3").unwrap();
    assert_eq!((pb.rid.as_deref(), pb.width_pt, pb.height_pt), (Some("rId1"), n(10.0), n(360.0)));
    assert_eq!(out.pic_bullets.get("NaN").unwrap().rid.as_deref(), Some("rId2"));
    let a = out.abstract_nums.get("7").unwrap();
    assert_eq!(keys(a), ["abstractNumId", "levels", "multiLevelType"]);
    assert_eq!(a.levels.keys().collect::<Vec<_>>(), ["0", "1"]);
    let l1 = a.levels.get("1").unwrap();
    assert_eq!((l1.start, l1.is_lgl, l1.lvl_text_null), (n(5.0), Some(true), Some(true)));
    assert_eq!(
        (l1.num_fmt.as_deref(), l1.num_fmt_custom.as_deref(), l1.suff.as_deref(), l1.lvl_text.as_deref()),
        (Some("decimal"), Some("001"), Some("space"), Some("%1"))
    );
    let inst = out.nums.get("2").unwrap();
    let ov = inst.overrides.get("1").unwrap();
    assert_eq!(ov.start_override, n(3.0));
    assert!(ov.lvl.is_some());
}

#[test]
fn numbering_limit_is_an_error_and_a_missing_part_is_empty() {
    let lvls: String = (0..4).map(|i| format!("<w:lvl w:ilvl=\"{i}\"/>")).collect();
    let xml = format!("<w:numbering {W}><w:abstractNum w:abstractNumId=\"1\">{lvls}</w:abstractNum></w:numbering>");
    let limits = DocxLimits { max_numbering_levels: 3, ..DocxLimits::DEFAULT };
    assert!(matches!(
        read_numbering_text(&xml, &limits),
        Err(DocxError::Limit { kind: DocxLimitKind::NumberingLevels, max: 3, part: None })
    ));
    assert_eq!(read_numbering_text(&xml, &DocxLimits::DEFAULT).unwrap().abstract_nums.get("1").unwrap().levels.len(), 4);
    assert!(read_numbering(None, &limits).unwrap().abstract_nums.is_empty());
    assert!(read_numbering_text("", &limits).unwrap().nums.is_empty());
    assert!(read_numbering_text("<w:numbering", &limits).is_err());
}

// ───────────────────────── theme ─────────────────────────

#[test]
fn theme_follows_the_word_rules() {
    let xml = format!(
        r#"<a:theme {A}><a:themeElements><a:clrScheme name="x"><a:dk1><a:sysClr val="windowText"/></a:dk1><a:lt1><a:sysClr val="window" lastClr=""/></a:lt1><a:accent1><a:srgbClr val="4472c4"/></a:accent1><a:accent2><a:srgbClr val=""/></a:accent2><a:__proto__><a:srgbClr val="111111"/></a:__proto__></a:clrScheme>
<a:fontScheme name="f"><a:majorFont><a:latin typeface="Calibri Light"/><a:ea typeface=""/><a:font script="Jpan" typeface="A"/><a:font script="Jpan" typeface="B"/><a:font script="Hang"/></a:majorFont></a:fontScheme>
<a:fmtScheme><a:fillStyleLst><a:solidFill><a:srgbClr val="FF0000"/></a:solidFill><a:noFill/><a:solidFill/></a:fillStyleLst><a:lnStyleLst><a:ln w="12700"/><a:ln w="abc"/><a:ln/></a:lnStyleLst>
<a:effectStyleLst><a:effectStyle/><a:effectStyle><a:effectLst><a:outerShdw dist="12700" dir="0"><a:srgbClr val="000000"/></a:outerShdw></a:effectLst></a:effectStyle></a:effectStyleLst></a:fmtScheme></a:themeElements></a:theme>"#
    );
    let doc = parse(xml, &uo_docx::read::DOCX).unwrap();
    let t = read_theme(Some(&doc)).unwrap();
    assert_eq!(t.colors.iter().map(|(k, v)| (k, &**v)).collect::<Vec<_>>(), [("dk1", "000000"), ("accent1", "4472C4")]);
    assert_eq!(t.major.latin.as_deref(), Some("Calibri Light"));
    assert!(t.major.ea.is_none() && t.minor.latin.is_none());
    assert_eq!(t.major.scripts.iter().map(|(k, v)| (k, &**v)).collect::<Vec<_>>(), [("Jpan", "B")]);
    assert_eq!(t.line_widths_pt, Some(vec![JsNumber::Num(1.0), JsNumber::Num(0.0), JsNumber::Num(0.0)]));
    assert_eq!(t.fill_styles.as_ref().unwrap().len(), 3);
    assert_eq!(t.bg_fill_styles, None);
    let fx = t.effect_styles.unwrap();
    assert_eq!(fx.len(), 2);
    assert!(fx[0].is_none() && fx[1].is_some());
    assert_eq!(keys(&read_theme(None).unwrap()), ["major", "minor", "colors"]);
    assert!(read_theme_text("").unwrap().colors.is_empty());
    assert!(read_theme_text("<a:theme").is_err());
}

// ───────────────────────── settings, font table ─────────────────────────

#[test]
fn settings_defaults_and_values() {
    let none = read_settings(None);
    assert_eq!((none.default_tab_stop, none.compatibility_mode), (JsNumber::Num(720.0), JsNumber::Num(15.0)));
    let doc = parse(
        format!(
            r#"<w:settings {W}><w:defaultTabStop w:val="12pt"/><w:evenAndOddHeaders/><w:mirrorMargins w:val="off"/><w:doNotExpandShiftReturn w:val="0"/><w:hyphenationZone w:val="."/><w:consecutiveHyphenLimit w:val="x"/>
<w:themeFontLang w:val="pt-BR" w:bidi=""/><w:footnotePr><w:pos w:val="beneathText"/><w:numStart w:val="3"/><w:numRestart w:val="bad"/></w:footnotePr><w:endnotePr/>
<w:compat><w:compatSetting w:name="compatibilityMode" w:uri="http://schemas.microsoft.com/office/word" w:val="14"/><w:useFELayout/><w:__proto__/></w:compat></w:settings>"#
        ),
        &uo_docx::read::DOCX,
    )
    .unwrap();
    let s = read_settings(Some(&doc));
    assert_eq!((s.default_tab_stop, s.compatibility_mode), (JsNumber::Num(240.0), JsNumber::Num(14.0)));
    assert!(s.even_and_odd_headers && !s.mirror_margins && !s.auto_hyphenation);
    assert!(s.hyphenation_zone.is_some_and(|z| z.to_number().is_nan()));
    assert!(s.consecutive_hyphen_limit.is_some_and(|z| z.to_number().is_nan()));
    assert_eq!(s.compat.iter().map(|(k, v)| (k, *v)).collect::<Vec<_>>(), [("doNotExpandShiftReturn", false), ("useFELayout", true)]);
    let tfl = s.theme_font_lang.unwrap();
    assert_eq!((tfl.val.as_deref(), tfl.bidi), (Some("pt-BR"), None));
    let fnp = s.footnote_pr.unwrap();
    assert_eq!((fnp.pos.as_deref(), fnp.num_start, fnp.num_restart), (Some("beneathText"), n(3.0), None));
    assert!(s.endnote_pr.is_none());
    assert_eq!(read_settings_text("").unwrap().compatibility_mode, JsNumber::Num(15.0));
    assert_eq!(read_settings_text(&format!("<w:settings {W}/>")).unwrap().compatibility_mode, JsNumber::Num(12.0));
    assert!(read_settings_text("<w:settings").is_err());
}

#[test]
fn note_props() {
    let doc = parse(format!("<w:footnotePr {W}/>"), &uo_docx::read::DOCX).unwrap();
    assert!(read_note_props(Some(doc.root())).is_none());
    assert!(read_note_props(None).is_none());
    let doc = parse(
        format!("<w:endnotePr {W}><w:numFmt w:val=\"chicago\"/><w:numRestart w:val=\"eachPage\"/></w:endnotePr>"),
        &uo_docx::read::DOCX,
    )
    .unwrap();
    let p = read_note_props(Some(doc.root())).unwrap();
    assert_eq!((p.num_fmt.as_deref(), p.num_restart.as_deref()), (Some("chicago"), Some("eachPage")));
}

#[test]
fn font_table() {
    let doc = parse(
        format!(
            r#"<w:fonts {W}><w:font w:name="Calibri"><w:altName w:val="Alt"/><w:family w:val="swiss"/><w:sig w:usb0="E0" w:csb1="x" foo="y" w:5="five"/></w:font><w:font/><w:font w:name="__proto__"/><w:font w:name="10"/><w:font w:name="Calibri"><w:pitch w:val="variable"/></w:font></w:fonts>"#
        ),
        &uo_docx::read::DOCX,
    )
    .unwrap();
    let t = read_font_table(Some(&doc));
    assert_eq!(t.keys().collect::<Vec<_>>(), ["10", "Calibri"]);
    let c = t.get("Calibri").unwrap();
    assert_eq!((c.pitch.as_deref(), c.alt_name.as_deref()), (Some("variable"), None));
    assert!(read_font_table(None).is_empty());
    assert!(read_font_table_text("").unwrap().is_empty());
    assert!(read_font_table_text("<w:fonts").is_err());
}

// ───────────────────────── the package ─────────────────────────

const REL: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships";

#[test]
fn part_lookup_by_relationship_type() {
    let rels = format!(
        r#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="a" Type="{REL}/styles" Target="custom/styles2.xml"/><Relationship Id="b" Type="{REL}/numbering" Target="gone.xml"/><Relationship Id="c" Type="{REL}/theme" Target="https://x.test/t.xml" TargetMode="External"/><Relationship Id="d" Type="{REL}/theme" Target="./theme/t.xml"/><Relationship Id="e" Type="{REL}/settings" Target=""/></Relationships>"#
    );
    let pkg =
        package(&[("word/_rels/document.xml.rels", &rels), ("word/custom/styles2.xml", "<w:styles/>"), ("word/theme/t.xml", "<a:theme/>")]);
    let opts = ReadOptions::default();
    let ctx = ReadCtx::new(&pkg, &opts);
    let list = uo_opc::rels::from_document(&parse(&rels, &ParseOptions::LENIENT).unwrap());
    assert_eq!(part_by_rel_type(&ctx, &list, "styles", "word/styles.xml").as_deref(), Some("word/custom/styles2.xml"));
    assert_eq!(part_by_rel_type(&ctx, &list, "numbering", "word/numbering.xml"), None);
    assert_eq!(part_by_rel_type(&ctx, &list, "theme", "word/theme/theme1.xml").as_deref(), Some("word/theme/t.xml"));
    assert_eq!(part_by_rel_type(&ctx, &list, "settings", "word/settings.xml").as_deref(), Some("word/settings.xml"));
    assert_eq!(part_by_rel_type(&ctx, &list, "fontTable", "word/fontTable.xml").as_deref(), Some("word/fontTable.xml"));
}

#[test]
fn doc_formatting_attaches_the_picture_bullet_images() {
    let rels = format!(
        r#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="{REL}/numbering" Target="custom/num.xml"/></Relationships>"#
    );
    let num_rels = format!(
        r#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="{REL}/image" Target="../media/a.PNG"/><Relationship Id="rId2" Type="{REL}/image" Target="../media/none.png"/><Relationship Id="rId3" Type="{REL}/image" Target="../media/a.PNG" TargetMode="External"/></Relationships>"#
    );
    let numbering = format!(
        r#"<w:numbering {W} xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships" {A}><w:numPicBullet w:numPicBulletId="0"><a:blip r:embed="rId1"/></w:numPicBullet><w:numPicBullet w:numPicBulletId="1"><a:blip r:embed="rId2"/></w:numPicBullet></w:numbering>"#
    );
    let pkg = package(&[
        ("word/_rels/document.xml.rels", &rels),
        ("word/custom/num.xml", &numbering),
        ("word/custom/_rels/num.xml.rels", &num_rels),
        ("word/media/a.PNG", "PNGDATA"),
        ("word/styles.xml", &format!("<w:styles {W}><w:style w:styleId=\"A\"/></w:styles>")),
    ]);
    let opts = ReadOptions::default();
    let mut ctx = ReadCtx::new(&pkg, &opts);
    let images = numbering_images(&ctx, "word/custom/num.xml").unwrap();
    assert_eq!(images, [("rId1".to_owned(), "word/media/a.PNG".to_owned(), "png".to_owned())]);
    let f = read_doc_formatting(&mut ctx).unwrap();
    assert_eq!(f.styles.styles.len(), 1);
    let b0 = f.numbering.pic_bullets.get("0").unwrap();
    let img = b0.image.as_ref().unwrap();
    assert_eq!(&*img.ext, "png");
    assert_eq!(ctx.media.get(img.data.0), Some(&b"PNGDATA"[..]));
    assert!(f.numbering.pic_bullets.get("1").unwrap().image.is_none());
    assert_eq!(f.settings.compatibility_mode, JsNumber::Num(15.0));
}

#[test]
fn doc_formatting_of_an_empty_part_and_a_malformed_one() {
    let pkg = package(&[("word/styles.xml", ""), ("word/settings.xml", "<w:settings")]);
    let opts = ReadOptions::default();
    let mut ctx = ReadCtx::new(&pkg, &opts);
    assert!(matches!(read_doc_formatting(&mut ctx), Err(DocxError::Package(_))));
    let pkg = package(&[("word/styles.xml", ""), ("word/_rels/document.xml.rels", "")]);
    let mut ctx = ReadCtx::new(&pkg, &opts);
    let f = read_doc_formatting(&mut ctx).unwrap();
    assert!(f.styles.styles.is_empty());
    assert_eq!(f.settings.compatibility_mode, JsNumber::Num(15.0));
}

// ───────────────────────── styles.ts ─────────────────────────

fn styles_xml(body: &str) -> String {
    format!("<w:styles {W}>{body}</w:styles>")
}

#[test]
fn style_props() {
    let doc = parse(
        format!(
            r#"<w:rPr {W}><w:i w:val="0"/><w:b/><w:sz w:val="x"/><w:color w:val="auto"/><w:u/><w:rFonts w:ascii="Aptos"/><w:b w:val="none"/><w:strike w:val="false"/></w:rPr>"#
        ),
        &uo_docx::read::DOCX,
    )
    .unwrap();
    let r = read_style_run_props(Some(doc.root()));
    assert_eq!((r.italic, r.bold, r.underline, r.strike, r.color.is_none()), (Some(false), Some(false), Some(true), Some(false), true));
    assert!(r.sz_half_pt.is_some_and(|v| v.to_number().is_nan()));
    assert_eq!(keys(&r), ["italic", "bold", "szHalfPt", "underline", "fontAscii", "strike"]);
    assert!(read_style_run_props(None).bold.is_none());
    let doc = parse(
        format!(
            r#"<w:pPr {W}><w:spacing w:line="300" w:lineRule="exact" w:after="10" w:before="x"/><w:jc w:val="start"/><w:ind w:right="5"/><w:pageBreakBefore/><w:pBdr><w:bottom w:val="single"/><w:top w:val="none"/></w:pBdr><w:shd w:fill="auto"/></w:pPr>"#
        ),
        &uo_docx::read::DOCX,
    )
    .unwrap();
    let p = read_style_para_props(Some(doc.root()));
    assert_eq!(keys(&p), ["spaceAfterTwips", "spaceBeforeTwips", "indentRightTwips", "pageBreakBefore", "borders"]);
    assert!(p.alignment.is_none() && p.line_twentieths.is_none() && p.shading_hex.is_none());
    assert!(p.borders.unwrap().bottom.is_some());
}

#[test]
fn styles_resolve_through_based_on_in_post_order() {
    let xml = styles_xml(
        r#"<w:docDefaults><w:rPrDefault><w:rPr><w:sz w:val="22"/></w:rPr></w:rPrDefault></w:docDefaults>
<w:style w:type="paragraph" w:styleId="A"><w:name w:val="a"/><w:basedOn w:val="B"/><w:rPr><w:b/></w:rPr></w:style>
<w:style w:type="character" w:styleId="B"><w:rPr><w:i/><w:sz w:val="30"/></w:rPr></w:style>
<w:style w:type="table" w:styleId="T"/><w:style w:type="paragraph" w:styleId="C"><w:basedOn w:val="D"/></w:style><w:style w:type="paragraph" w:styleId="D"><w:basedOn w:val="C"/></w:style>
<w:style w:type="paragraph" w:styleId="E"><w:basedOn w:val="missing"/></w:style>"#,
    );
    let doc = parse(xml, &uo_docx::read::DOCX).unwrap();
    let m = read_styles_xml(Some(&doc), &DocxLimits::DEFAULT).unwrap();
    assert_eq!(m.keys().map(|k| &**k).collect::<Vec<_>>(), ["B", "A", "D", "C", "E", "docDefaults"]);
    let a = m.get("A").unwrap();
    assert_eq!(a.name.as_deref(), Some("a"));
    assert_eq!((a.run.bold, a.run.italic, a.run.sz_half_pt), (Some(true), Some(true), n(30.0)));
    assert!(matches!(m.get("B").unwrap().run.sz_half_pt, Some(JsNumber::Num(v)) if v == 30.0));
    assert!(matches!(m.get("E").unwrap().run.sz_half_pt, Some(JsNumber::Num(v)) if v == 22.0));
    let b = m.get("B").unwrap();
    assert!(matches!(b.name_slot(), Slot::Undefined) && b.r#type.as_deref() == Some("character"));
    let dd = m.get("docDefaults").unwrap();
    assert!(matches!(dd.name_slot(), Slot::Absent) && dd.r#type.is_none());
}

trait NameSlot {
    fn name_slot(&self) -> Slot<'_>;
}

impl NameSlot for uo_docx::model::formatting::ResolvedStyle {
    fn name_slot(&self) -> Slot<'_> {
        self.slot(2)
    }
}

#[test]
fn based_on_chain_limit_and_missing_parts() {
    let chain: String =
        (0..6).map(|i| format!("<w:style w:type=\"paragraph\" w:styleId=\"S{i}\"><w:basedOn w:val=\"S{}\"/></w:style>", i + 1)).collect();
    let xml = styles_xml(&chain);
    let limits = DocxLimits { max_based_on_chain: 4, ..DocxLimits::DEFAULT };
    assert!(matches!(read_styles_text(&xml, &limits), Err(DocxError::Limit { kind: DocxLimitKind::BasedOnChain, max: 4, part: None })));
    assert_eq!(read_styles_text(&xml, &DocxLimits::DEFAULT).unwrap().len(), 7);
    assert!(read_styles_xml(None, &limits).unwrap().is_empty());
    assert!(read_styles_text("", &limits).unwrap().is_empty());
    assert!(read_styles_text("<w:styles", &limits).is_err());
    let pkg = package(&[("word/styles.xml", &xml)]);
    let opts = ReadOptions::default();
    let ctx = ReadCtx::new(&pkg, &opts);
    assert_eq!(read_styles(&ctx).unwrap().len(), 7);
    let none = package(&[]);
    assert!(read_styles(&ReadCtx::new(&none, &opts)).unwrap().is_empty());
}

// ───────────────────────── table-styles.ts ─────────────────────────

#[test]
fn theme_hex_matches_the_ts() {
    let c = colors(&[("accent1", "4472C4"), ("dk1", "000000"), ("lt2", "E7E6E6")]);
    assert_eq!(theme_hex(&c, "accent1", Some("80"), None), "A1B8E1");
    assert_eq!(theme_hex(&c, "accent1", None, Some("BF")), "2F5496");
    assert_eq!(theme_hex(&c, "accent1", Some("99"), Some("80")), "527DC8");
    assert_eq!(theme_hex(&JsRecord::new(), "accent2", None, None), "E97132");
    assert_eq!(theme_hex(&JsRecord::new(), "text2", Some("33"), None), "B8D4EF");
    assert_eq!(theme_hex(&c, "nope", Some("80"), None), "7F7F7F");
    assert_eq!(theme_hex(&colors(&[("accent1", "abc")]), "accent1", Some("80"), None), "NANNANNAN");
    assert_eq!(theme_hex(&colors(&[("constructor", "123456")]), "constructor", None, None), "000000");
    assert_eq!(theme_hex(&colors(&[("accent1", "abcdef")]), "accent1", Some(""), Some("")), "ABCDEF");
}

#[test]
fn materialize_replaces_the_markers() {
    let c = colors(&[("accent1", "4472C4")]);
    let xml = r#"<w:top w:color="~" w:themeColor="accent1" w:themeShade="BF"/><w:shd w:fill="~" w:themeFill="accent1" w:themeFillTint="33"/><w:x w:color="~"/>"#;
    assert_eq!(
        materialize_style_xml(xml, &c),
        r#"<w:top w:color="2F5496" w:themeColor="accent1" w:themeShade="BF"/><w:shd w:fill="D9E2F3" w:themeFill="accent1" w:themeFillTint="33"/><w:x w:color="000000"/>"#
    );
    assert_eq!(materialize_style_xml("", &c), "");
}

#[test]
fn builtin_list_and_core_styles() {
    let list = builtin_table_styles();
    assert_eq!(list.len(), 247);
    assert_eq!((list[0].id.as_str(), list[0].name.as_str()), ("Tabelanormal", "Normal Table"));
    let light = list.iter().find(|b| b.name == "Light Grid").unwrap();
    assert_eq!((light.local.as_str(), light.id.as_str()), ("Grade Clara", "GradeClara"));
    assert!(light.xml.contains("w:styleId=\"GradeClara\""));
    assert!(list.iter().any(|b| b.id == "GradeClara-nfase3"));
    assert_eq!(CORE_TABLE_STYLES[1].id, "Tabelacomgrade");
    assert_eq!(local_style_id("Tabela com grade - Ênfase 1"), "Tabelacomgrade-nfase1");
}

#[test]
fn style_definitions_and_lookup_by_name() {
    let def = style_def_of(CORE_TABLE_STYLES[1].xml).unwrap().unwrap();
    assert_eq!(
        (&*def.style_id, &*def.r#type, def.name.as_deref(), def.based_on.as_deref()),
        ("Tabelacomgrade", "table", Some("Table Grid"), Some("Tabelanormal"))
    );
    assert!(def.tbl_pr.is_some() && def.p_pr.is_some());
    assert!(style_def_of("").unwrap().is_none());
    assert!(style_def_of("<w:style").is_err());
    let doc = parse(
        format!(r#"<w:styles {W}><w:style w:type="paragraph" w:styleId="P"><w:name w:val="Table Grid"/></w:style><w:style w:type="table" w:styleId="T"><w:name w:val="Table GRID"/></w:style></w:styles>"#),
        &uo_docx::read::DOCX,
    )
    .unwrap();
    let f = uo_docx::model::formatting::DocFormatting { styles: read_style_sheet(Some(&doc)), ..Default::default() };
    assert_eq!(style_id_by_name(Some(&f), "table grid"), Some("T"));
    assert_eq!(style_id_by_name(Some(&f), "x"), None);
    assert_eq!(style_id_by_name(None, "table grid"), None);
}

// ───────────────────────── style-names-es.ts ─────────────────────────

#[test]
fn spanish_table_style_names() {
    let es = |n: &str| table_style_name_es(n);
    assert_eq!(es("Normal Table").as_deref(), Some("Tabla normal"));
    assert_eq!(es("Grid Table 4 Accent 1").as_deref(), Some("Tabla de cuadrícula 4 - Énfasis 1"));
    assert_eq!(es("Grid Table 7 Colorful Accent 6").as_deref(), Some("Tabla de cuadrícula 7 con colores - Énfasis 6"));
    assert_eq!(es("Light Shading").as_deref(), Some("Sombreado claro"));
    assert_eq!(es("Table 3D effects 2").as_deref(), Some("Tabla con efectos 3D 2"));
    assert_eq!(es("Grid Table 4 Accent 10"), None);
    assert_eq!(es("Table\nGrid"), None);
    assert_eq!(es("constructor"), None);
    assert_eq!(es(""), None);
    assert_eq!(es(" Accent 1"), None);
    assert_eq!(builtin_style_name_es("heading 1"), Some("Título 1"));
    assert_eq!(builtin_style_name_es("constructor"), None);
}

#[test]
fn limit_errors_get_the_part_name_only_when_they_have_none() {
    let limit = DocxError::Limit { kind: DocxLimitKind::BasedOnChain, max: 4, part: None };
    assert_eq!(
        limit.clone().in_part("word/styles.xml"),
        DocxError::Limit { kind: DocxLimitKind::BasedOnChain, max: 4, part: Some("word/styles.xml".into()) }
    );
    let named = limit.in_part("word/styles.xml");
    assert_eq!(named.clone().in_part("other.xml"), named);
    assert_eq!(DocxError::NotWordprocessing.in_part("x"), DocxError::NotWordprocessing);
}
