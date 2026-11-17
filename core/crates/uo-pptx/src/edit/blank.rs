//! `blank-deck.ts`: the BLANK presentation of "New" (the Office theme with Calibri Light/Calibri, a 16:9
//! or 4:3 slide, six layouts, a notes master and an empty title slide) and the default notes master
//! `deck-edit` creates on demand. Text of the parts byte for byte as the TS.

use std::sync::Arc;

use uo_jsnum::math::round as jround;

use super::pkg::fmt;
use crate::package::{PartStore, RawParts};

const DECL: &str = "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n";
const A: &str = "http://schemas.openxmlformats.org/drawingml/2006/main";
const R: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships";
const P: &str = "http://schemas.openxmlformats.org/presentationml/2006/main";

fn ns() -> String {
    format!("xmlns:a=\"{A}\" xmlns:r=\"{R}\" xmlns:p=\"{P}\"")
}

fn ct(t: &str) -> String {
    format!("application/vnd.openxmlformats-officedocument.presentationml.{t}+xml")
}

fn rels(list: &[(&str, &str, &str)]) -> String {
    let mut s = format!("{DECL}<Relationships xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\">");
    for (id, type_, target) in list {
        let t = if type_.starts_with("http") { (*type_).to_owned() } else { format!("{R}/{type_}") };
        s.push_str(&format!("<Relationship Id=\"{id}\" Type=\"{t}\" Target=\"{target}\"/>"));
    }
    s.push_str("</Relationships>");
    s
}

/// The Office theme (colours, fonts, default effects), also that of the notes master created on demand.
pub fn theme(name: &str) -> String {
    let mut s = format!("{DECL}<a:theme xmlns:a=\"{A}\" name=\"{name}\"><a:themeElements>");
    s.push_str(concat!(
        "<a:clrScheme name=\"Office\"><a:dk1><a:sysClr val=\"windowText\" lastClr=\"000000\"/></a:dk1><a:lt1><a:sysClr val=\"window\" lastClr=\"FFFFFF\"/></a:lt1>",
        "<a:dk2><a:srgbClr val=\"44546A\"/></a:dk2><a:lt2><a:srgbClr val=\"E7E6E6\"/></a:lt2><a:accent1><a:srgbClr val=\"4472C4\"/></a:accent1>",
        "<a:accent2><a:srgbClr val=\"ED7D31\"/></a:accent2><a:accent3><a:srgbClr val=\"A5A5A5\"/></a:accent3><a:accent4><a:srgbClr val=\"FFC000\"/></a:accent4>",
        "<a:accent5><a:srgbClr val=\"5B9BD5\"/></a:accent5><a:accent6><a:srgbClr val=\"70AD47\"/></a:accent6><a:hlink><a:srgbClr val=\"0563C1\"/></a:hlink>",
        "<a:folHlink><a:srgbClr val=\"954F72\"/></a:folHlink></a:clrScheme>",
        "<a:fontScheme name=\"Office\"><a:majorFont><a:latin typeface=\"Calibri Light\" panose=\"020F0302020204030204\"/><a:ea typeface=\"\"/><a:cs typeface=\"\"/></a:majorFont>",
        "<a:minorFont><a:latin typeface=\"Calibri\" panose=\"020F0502020204030204\"/><a:ea typeface=\"\"/><a:cs typeface=\"\"/></a:minorFont></a:fontScheme>",
        "<a:fmtScheme name=\"Office\"><a:fillStyleLst><a:solidFill><a:schemeClr val=\"phClr\"/></a:solidFill>",
        "<a:gradFill rotWithShape=\"1\"><a:gsLst><a:gs pos=\"0\"><a:schemeClr val=\"phClr\"><a:lumMod val=\"110000\"/><a:satMod val=\"105000\"/><a:tint val=\"67000\"/></a:schemeClr></a:gs>",
        "<a:gs pos=\"50000\"><a:schemeClr val=\"phClr\"><a:lumMod val=\"105000\"/><a:satMod val=\"103000\"/><a:tint val=\"73000\"/></a:schemeClr></a:gs>",
        "<a:gs pos=\"100000\"><a:schemeClr val=\"phClr\"><a:lumMod val=\"105000\"/><a:satMod val=\"109000\"/><a:tint val=\"81000\"/></a:schemeClr></a:gs></a:gsLst><a:lin ang=\"5400000\" scaled=\"0\"/></a:gradFill>",
        "<a:gradFill rotWithShape=\"1\"><a:gsLst><a:gs pos=\"0\"><a:schemeClr val=\"phClr\"><a:satMod val=\"103000\"/><a:lumMod val=\"102000\"/><a:tint val=\"94000\"/></a:schemeClr></a:gs>",
        "<a:gs pos=\"50000\"><a:schemeClr val=\"phClr\"><a:satMod val=\"110000\"/><a:lumMod val=\"100000\"/><a:shade val=\"100000\"/></a:schemeClr></a:gs>",
        "<a:gs pos=\"100000\"><a:schemeClr val=\"phClr\"><a:lumMod val=\"99000\"/><a:satMod val=\"120000\"/><a:shade val=\"78000\"/></a:schemeClr></a:gs></a:gsLst><a:lin ang=\"5400000\" scaled=\"0\"/></a:gradFill></a:fillStyleLst>",
        "<a:lnStyleLst><a:ln w=\"6350\" cap=\"flat\" cmpd=\"sng\" algn=\"ctr\"><a:solidFill><a:schemeClr val=\"phClr\"/></a:solidFill><a:prstDash val=\"solid\"/><a:miter lim=\"800000\"/></a:ln>",
        "<a:ln w=\"12700\" cap=\"flat\" cmpd=\"sng\" algn=\"ctr\"><a:solidFill><a:schemeClr val=\"phClr\"/></a:solidFill><a:prstDash val=\"solid\"/><a:miter lim=\"800000\"/></a:ln>",
        "<a:ln w=\"19050\" cap=\"flat\" cmpd=\"sng\" algn=\"ctr\"><a:solidFill><a:schemeClr val=\"phClr\"/></a:solidFill><a:prstDash val=\"solid\"/><a:miter lim=\"800000\"/></a:ln></a:lnStyleLst>",
        "<a:effectStyleLst><a:effectStyle><a:effectLst/></a:effectStyle><a:effectStyle><a:effectLst/></a:effectStyle>",
        "<a:effectStyle><a:effectLst><a:outerShdw blurRad=\"57150\" dist=\"19050\" dir=\"5400000\" algn=\"ctr\" rotWithShape=\"0\"><a:srgbClr val=\"000000\"><a:alpha val=\"63000\"/></a:srgbClr></a:outerShdw></a:effectLst></a:effectStyle></a:effectStyleLst>",
        "<a:bgFillStyleLst><a:solidFill><a:schemeClr val=\"phClr\"/></a:solidFill><a:solidFill><a:schemeClr val=\"phClr\"><a:tint val=\"95000\"/><a:satMod val=\"170000\"/></a:schemeClr></a:solidFill>",
        "<a:gradFill rotWithShape=\"1\"><a:gsLst><a:gs pos=\"0\"><a:schemeClr val=\"phClr\"><a:tint val=\"93000\"/><a:satMod val=\"150000\"/><a:shade val=\"98000\"/><a:lumMod val=\"102000\"/></a:schemeClr></a:gs>",
        "<a:gs pos=\"50000\"><a:schemeClr val=\"phClr\"><a:tint val=\"98000\"/><a:satMod val=\"130000\"/><a:shade val=\"90000\"/><a:lumMod val=\"103000\"/></a:schemeClr></a:gs>",
        "<a:gs pos=\"100000\"><a:schemeClr val=\"phClr\"><a:shade val=\"63000\"/><a:satMod val=\"120000\"/></a:schemeClr></a:gs></a:gsLst><a:lin ang=\"5400000\" scaled=\"0\"/></a:gradFill></a:bgFillStyleLst>",
        "</a:fmtScheme></a:themeElements><a:objectDefaults/><a:extraClrSchemeLst/></a:theme>",
    ));
    s
}

const TREE_HEAD: &str = concat!(
    "<p:nvGrpSpPr><p:cNvPr id=\"1\" name=\"\"/><p:cNvGrpSpPr/><p:nvPr/></p:nvGrpSpPr>",
    "<p:grpSpPr><a:xfrm><a:off x=\"0\" y=\"0\"/><a:ext cx=\"0\" cy=\"0\"/><a:chOff x=\"0\" y=\"0\"/><a:chExt cx=\"0\" cy=\"0\"/></a:xfrm></p:grpSpPr>"
);
const CLR_MAP: &str = "bg1=\"lt1\" tx1=\"dk1\" bg2=\"lt2\" tx2=\"dk2\" accent1=\"accent1\" accent2=\"accent2\" accent3=\"accent3\" accent4=\"accent4\" accent5=\"accent5\" accent6=\"accent6\" hlink=\"hlink\" folHlink=\"folHlink\"";

type Box = [i64; 4];

struct Ph<'a> {
    id: u32,
    name: &'a str,
    ph: &'a str,
    xfrm: Option<Box>,
    body: Option<&'a str>,
    lst: Option<&'a str>,
    text: Option<&'a str>,
}

impl<'a> Ph<'a> {
    fn new(id: u32, name: &'a str, ph: &'a str) -> Ph<'a> {
        Ph { id, name, ph, xfrm: None, body: None, lst: None, text: None }
    }
}

/// A placeholder: `ph` is the middle of the tag (e.g. `type="title"`).
fn ph(p: &Ph<'_>) -> String {
    let xf = match p.xfrm {
        Some(x) => format!("<a:xfrm><a:off x=\"{}\" y=\"{}\"/><a:ext cx=\"{}\" cy=\"{}\"/></a:xfrm>", x[0], x[1], x[2], x[3]),
        None => String::new(),
    };
    let lst = match p.lst.filter(|l| !l.is_empty()) {
        Some(l) => format!("<a:lstStyle>{l}</a:lstStyle>"),
        None => "<a:lstStyle/>".to_owned(),
    };
    let text = match p.text.filter(|t| !t.is_empty()) {
        Some(t) => format!("<a:r><a:rPr lang=\"en-US\"/><a:t>{t}</a:t></a:r>"),
        None => String::new(),
    };
    format!(
        "<p:sp><p:nvSpPr><p:cNvPr id=\"{}\" name=\"{}\"/><p:cNvSpPr><a:spLocks noGrp=\"1\"/></p:cNvSpPr><p:nvPr><p:ph {}/></p:nvPr></p:nvSpPr><p:spPr>{xf}</p:spPr><p:txBody>{}{lst}<a:p>{text}<a:endParaRPr lang=\"en-US\"/></a:p></p:txBody></p:sp>",
        p.id,
        p.name,
        p.ph,
        p.body.unwrap_or("<a:bodyPr/>")
    )
}

struct Geometry {
    title: Box,
    body: Box,
    dt: Box,
    ftr: Box,
    sld_num: Box,
    ctr_title: Box,
    sub_title: Box,
    sec_title: Box,
    sec_body: Box,
    left: Box,
    right: Box,
}

/// Geometry of the master, proportional to the slide width (16:9 = 12192000).
fn geometry(w: f64, h: f64) -> Geometry {
    let sx = |v: f64| jround(v * w / 12_192_000.0);
    let sy = |v: f64| jround(v * h / 6_858_000.0);
    let int = |v: f64| -> i64 { fmt(v).parse::<i64>().unwrap_or(0) };
    let b = |x: f64, y: f64, cx: f64, cy: f64| -> Box { [int(sx(x)), int(sy(y)), int(sx(cx)), int(sy(cy))] };
    Geometry {
        title: b(838_200.0, 365_125.0, 10_515_600.0, 1_325_563.0),
        body: b(838_200.0, 1_825_625.0, 10_515_600.0, 4_351_338.0),
        dt: b(838_200.0, 6_356_350.0, 2_743_200.0, 365_125.0),
        ftr: b(4_038_600.0, 6_356_350.0, 4_114_800.0, 365_125.0),
        sld_num: b(8_610_600.0, 6_356_350.0, 2_743_200.0, 365_125.0),
        ctr_title: b(1_524_000.0, 1_122_363.0, 9_144_000.0, 2_387_600.0),
        sub_title: b(1_524_000.0, 3_602_038.0, 9_144_000.0, 1_655_762.0),
        sec_title: b(831_850.0, 1_709_738.0, 10_515_600.0, 2_852_737.0),
        sec_body: b(831_850.0, 4_589_463.0, 10_515_600.0, 1_500_187.0),
        left: b(838_200.0, 1_825_625.0, 5_181_600.0, 4_351_338.0),
        right: b(6_172_200.0, 1_825_625.0, 5_181_600.0, 4_351_338.0),
    }
}

fn lvl(n: u32, mar_l: u32, sz: u32) -> String {
    format!(
        "<a:lvl{n}pPr marL=\"{mar_l}\" indent=\"-228600\" algn=\"l\" defTabSz=\"914400\" rtl=\"0\" eaLnBrk=\"1\" latinLnBrk=\"0\" hangingPunct=\"1\"><a:lnSpc><a:spcPct val=\"90000\"/></a:lnSpc><a:spcBef><a:spcPts val=\"{}\"/></a:spcBef><a:buFont typeface=\"Arial\" panose=\"020B0604020202020204\" pitchFamily=\"34\" charset=\"0\"/><a:buChar char=\"\u{2022}\"/><a:defRPr sz=\"{sz}\" kern=\"1200\"><a:solidFill><a:schemeClr val=\"tx1\"/></a:solidFill><a:latin typeface=\"+mn-lt\"/><a:ea typeface=\"+mn-ea\"/><a:cs typeface=\"+mn-cs\"/></a:defRPr></a:lvl{n}pPr>",
        if n == 1 { 1000 } else { 500 }
    )
}

fn master(g: &Geometry) -> String {
    let footer = |id: u32, name: &str, type_: &str, x: Box, algn: &str| -> String {
        let idx = if type_ == "dt" {
            2
        } else if type_ == "ftr" {
            3
        } else {
            4
        };
        let ph_attr = format!("type=\"{type_}\" sz=\"quarter\" idx=\"{idx}\"");
        let lst = format!(
            "<a:lvl1pPr algn=\"{algn}\"><a:defRPr sz=\"1200\"><a:solidFill><a:schemeClr val=\"tx1\"><a:tint val=\"75000\"/></a:schemeClr></a:solidFill></a:defRPr></a:lvl1pPr>"
        );
        ph(&Ph {
            id,
            name,
            ph: &ph_attr,
            xfrm: Some(x),
            body: Some("<a:bodyPr vert=\"horz\" lIns=\"91440\" tIns=\"45720\" rIns=\"91440\" bIns=\"45720\" rtlCol=\"0\" anchor=\"ctr\"/>"),
            lst: Some(&lst),
            text: None,
        })
    };
    let mut s = format!(
        "{DECL}<p:sldMaster {}><p:cSld><p:bg><p:bgRef idx=\"1001\"><a:schemeClr val=\"bg1\"/></p:bgRef></p:bg><p:spTree>{TREE_HEAD}",
        ns()
    );
    s.push_str(&ph(&Ph {
        xfrm: Some(g.title),
        body: Some("<a:bodyPr vert=\"horz\" lIns=\"91440\" tIns=\"45720\" rIns=\"91440\" bIns=\"45720\" rtlCol=\"0\" anchor=\"ctr\"><a:normAutofit/></a:bodyPr>"),
        text: Some("Click to edit Master title style"),
        ..Ph::new(2, "Title Placeholder 1", "type=\"title\"")
    }));
    s.push_str(&ph(&Ph {
        xfrm: Some(g.body),
        body: Some(
            "<a:bodyPr vert=\"horz\" lIns=\"91440\" tIns=\"45720\" rIns=\"91440\" bIns=\"45720\" rtlCol=\"0\"><a:normAutofit/></a:bodyPr>",
        ),
        text: Some("Click to edit Master text styles"),
        ..Ph::new(3, "Text Placeholder 2", "type=\"body\" idx=\"1\"")
    }));
    s.push_str(&footer(4, "Date Placeholder 3", "dt", g.dt, "l"));
    s.push_str(&footer(5, "Footer Placeholder 4", "ftr", g.ftr, "ctr"));
    s.push_str(&footer(6, "Slide Number Placeholder 5", "sldNum", g.sld_num, "r"));
    s.push_str(&format!("</p:spTree></p:cSld><p:clrMap {CLR_MAP}/>"));
    s.push_str("<p:sldLayoutIdLst>");
    for i in 1..=6u32 {
        s.push_str(&format!("<p:sldLayoutId id=\"{}\" r:id=\"rId{i}\"/>", 2_147_483_648u64 + u64::from(i)));
    }
    s.push_str("</p:sldLayoutIdLst>");
    s.push_str(concat!(
        "<p:txStyles><p:titleStyle><a:lvl1pPr algn=\"l\" defTabSz=\"914400\" rtl=\"0\" eaLnBrk=\"1\" latinLnBrk=\"0\" hangingPunct=\"1\"><a:lnSpc><a:spcPct val=\"90000\"/></a:lnSpc><a:spcBef><a:spcPct val=\"0\"/></a:spcBef><a:buNone/>",
        "<a:defRPr sz=\"4400\" kern=\"1200\"><a:solidFill><a:schemeClr val=\"tx1\"/></a:solidFill><a:latin typeface=\"+mj-lt\"/><a:ea typeface=\"+mj-ea\"/><a:cs typeface=\"+mj-cs\"/></a:defRPr></a:lvl1pPr></p:titleStyle>"
    ));
    s.push_str(&format!(
        "<p:bodyStyle>{}{}{}{}{}</p:bodyStyle>",
        lvl(1, 228_600, 2800),
        lvl(2, 685_800, 2400),
        lvl(3, 1_143_000, 2000),
        lvl(4, 1_600_200, 1800),
        lvl(5, 2_057_400, 1800)
    ));
    s.push_str(concat!(
        "<p:otherStyle><a:defPPr><a:defRPr lang=\"en-US\"/></a:defPPr><a:lvl1pPr marL=\"0\" algn=\"l\" defTabSz=\"914400\" rtl=\"0\" eaLnBrk=\"1\" latinLnBrk=\"0\" hangingPunct=\"1\"><a:defRPr sz=\"1800\" kern=\"1200\"><a:solidFill><a:schemeClr val=\"tx1\"/></a:solidFill><a:latin typeface=\"+mn-lt\"/><a:ea typeface=\"+mn-ea\"/><a:cs typeface=\"+mn-cs\"/></a:defRPr></a:lvl1pPr></p:otherStyle>",
        "</p:txStyles></p:sldMaster>"
    ));
    s
}

const FOOTERS: &str = concat!(
    "<p:sp><p:nvSpPr><p:cNvPr id=\"90\" name=\"Date Placeholder\"/><p:cNvSpPr><a:spLocks noGrp=\"1\"/></p:cNvSpPr><p:nvPr><p:ph type=\"dt\" sz=\"half\" idx=\"10\"/></p:nvPr></p:nvSpPr><p:spPr/><p:txBody><a:bodyPr/><a:lstStyle/><a:p><a:endParaRPr lang=\"en-US\"/></a:p></p:txBody></p:sp>",
    "<p:sp><p:nvSpPr><p:cNvPr id=\"91\" name=\"Footer Placeholder\"/><p:cNvSpPr><a:spLocks noGrp=\"1\"/></p:cNvSpPr><p:nvPr><p:ph type=\"ftr\" sz=\"quarter\" idx=\"11\"/></p:nvPr></p:nvSpPr><p:spPr/><p:txBody><a:bodyPr/><a:lstStyle/><a:p><a:endParaRPr lang=\"en-US\"/></a:p></p:txBody></p:sp>",
    "<p:sp><p:nvSpPr><p:cNvPr id=\"92\" name=\"Slide Number Placeholder\"/><p:cNvSpPr><a:spLocks noGrp=\"1\"/></p:cNvSpPr><p:nvPr><p:ph type=\"sldNum\" sz=\"quarter\" idx=\"12\"/></p:nvPr></p:nvSpPr><p:spPr/><p:txBody><a:bodyPr/><a:lstStyle/><a:p><a:fld id=\"{B6F15528-21DE-4FAA-801E-634DDDAF4B2B}\" type=\"slidenum\"><a:rPr lang=\"en-US\"/><a:t>\u{2039}#\u{203a}</a:t></a:fld><a:endParaRPr lang=\"en-US\"/></a:p></p:txBody></p:sp>"
);

fn layout(type_: &str, name: &str, shapes: &str) -> String {
    format!(
        "{DECL}<p:sldLayout {} type=\"{type_}\" preserve=\"1\"><p:cSld name=\"{name}\"><p:spTree>{TREE_HEAD}{shapes}{FOOTERS}</p:spTree></p:cSld><p:clrMapOvr><a:masterClrMapping/></p:clrMapOvr></p:sldLayout>",
        ns()
    )
}

fn layouts(g: &Geometry) -> Vec<String> {
    let title = ph(&Ph { text: Some("Click to edit Master title style"), ..Ph::new(2, "Title 1", "type=\"title\"") });
    let no_bullet =
        |sz: u32, extra: &str| format!("<a:lvl1pPr marL=\"0\" indent=\"0\"{extra}><a:buNone/><a:defRPr sz=\"{sz}\"/></a:lvl1pPr>");
    let ph_title_slide = ph(&Ph {
        xfrm: Some(g.ctr_title),
        body: Some("<a:bodyPr anchor=\"b\"/>"),
        lst: Some("<a:lvl1pPr algn=\"ctr\"><a:defRPr sz=\"6000\"/></a:lvl1pPr>"),
        text: Some("Click to edit Master title style"),
        ..Ph::new(2, "Title 1", "type=\"ctrTitle\"")
    }) + &ph(&Ph {
        xfrm: Some(g.sub_title),
        lst: Some(&no_bullet(2400, " algn=\"ctr\"")),
        text: Some("Click to edit Master subtitle style"),
        ..Ph::new(3, "Subtitle 2", "type=\"subTitle\" idx=\"1\"")
    });
    let sec_head = ph(&Ph {
        xfrm: Some(g.sec_title),
        body: Some("<a:bodyPr anchor=\"b\"/>"),
        lst: Some("<a:lvl1pPr><a:defRPr sz=\"6000\"/></a:lvl1pPr>"),
        text: Some("Click to edit Master title style"),
        ..Ph::new(2, "Title 1", "type=\"title\"")
    }) + &ph(&Ph {
        xfrm: Some(g.sec_body),
        lst: Some(
            "<a:lvl1pPr marL=\"0\" indent=\"0\"><a:buNone/><a:defRPr sz=\"2400\"><a:solidFill><a:schemeClr val=\"tx1\"><a:tint val=\"75000\"/></a:schemeClr></a:solidFill></a:defRPr></a:lvl1pPr>",
        ),
        text: Some("Click to edit Master text styles"),
        ..Ph::new(3, "Text Placeholder 2", "type=\"body\" idx=\"1\"")
    });
    let content = ph(&Ph { text: Some("Click to edit Master text styles"), ..Ph::new(3, "Content Placeholder 2", "idx=\"1\"") });
    let two = format!(
        "{title}{}{}",
        ph(&Ph {
            xfrm: Some(g.left),
            text: Some("Click to edit Master text styles"),
            ..Ph::new(3, "Content Placeholder 2", "sz=\"half\" idx=\"1\"")
        }),
        ph(&Ph {
            xfrm: Some(g.right),
            text: Some("Click to edit Master text styles"),
            ..Ph::new(4, "Content Placeholder 3", "sz=\"half\" idx=\"2\"")
        })
    );
    vec![
        layout("title", "Title Slide", &ph_title_slide),
        layout("obj", "Title and Content", &format!("{title}{content}")),
        layout("secHead", "Section Header", &sec_head),
        layout("twoObj", "Two Content", &two),
        layout("titleOnly", "Title Only", &title),
        layout("blank", "Blank", ""),
    ]
}

/// The default notes master (slide image + body) (`notesMaster`); `deck-edit` creates it when the
/// presentation has none.
pub fn notes_master() -> String {
    let mut s = format!(
        "{DECL}<p:notesMaster {}><p:cSld><p:bg><p:bgRef idx=\"1001\"><a:schemeClr val=\"bg1\"/></p:bgRef></p:bg><p:spTree>{TREE_HEAD}",
        ns()
    );
    s.push_str(concat!(
        "<p:sp><p:nvSpPr><p:cNvPr id=\"2\" name=\"Slide Image Placeholder 1\"/><p:cNvSpPr><a:spLocks noGrp=\"1\" noRot=\"1\" noChangeAspect=\"1\"/></p:cNvSpPr><p:nvPr><p:ph type=\"sldImg\" idx=\"2\"/></p:nvPr></p:nvSpPr>",
        "<p:spPr><a:xfrm><a:off x=\"685800\" y=\"1143000\"/><a:ext cx=\"5486400\" cy=\"3086100\"/></a:xfrm><a:prstGeom prst=\"rect\"><a:avLst/></a:prstGeom><a:noFill/><a:ln w=\"12700\"><a:solidFill><a:prstClr val=\"black\"/></a:solidFill></a:ln></p:spPr></p:sp>",
        "<p:sp><p:nvSpPr><p:cNvPr id=\"3\" name=\"Notes Placeholder 2\"/><p:cNvSpPr><a:spLocks noGrp=\"1\"/></p:cNvSpPr><p:nvPr><p:ph type=\"body\" sz=\"quarter\" idx=\"3\"/></p:nvPr></p:nvSpPr>",
        "<p:spPr><a:xfrm><a:off x=\"685800\" y=\"4400550\"/><a:ext cx=\"5486400\" cy=\"3600450\"/></a:xfrm><a:prstGeom prst=\"rect\"><a:avLst/></a:prstGeom></p:spPr>",
        "<p:txBody><a:bodyPr vert=\"horz\" lIns=\"91440\" tIns=\"45720\" rIns=\"91440\" bIns=\"45720\" rtlCol=\"0\"/><a:lstStyle/><a:p><a:pPr lvl=\"0\"/><a:r><a:rPr lang=\"en-US\"/><a:t>Click to edit Master text styles</a:t></a:r></a:p></p:txBody></p:sp>",
    ));
    s.push_str(&format!("</p:spTree></p:cSld><p:clrMap {CLR_MAP}/>"));
    s.push_str(concat!(
        "<p:notesStyle><a:lvl1pPr marL=\"0\" algn=\"l\" defTabSz=\"914400\" rtl=\"0\" eaLnBrk=\"1\" latinLnBrk=\"0\" hangingPunct=\"1\"><a:defRPr sz=\"1200\" kern=\"1200\"><a:solidFill><a:schemeClr val=\"tx1\"/></a:solidFill><a:latin typeface=\"+mn-lt\"/><a:ea typeface=\"+mn-ea\"/><a:cs typeface=\"+mn-cs\"/></a:defRPr></a:lvl1pPr></p:notesStyle>",
        "</p:notesMaster>"
    ));
    s
}

/// `BlankDeckOptions`.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct BlankDeckOptions {
    /// 16:9 (default, 33.867 x 19.05 cm) or 4:3 (25.4 x 19.05 cm)
    pub wide: bool,
    /// the title written in the properties
    pub title: Option<String>,
    /// the language of the text (`lang` of new runs)
    pub lang: String,
    /// `new Date().toISOString()` without the milliseconds (`2026-01-02T03:04:00Z`): the host's clock
    pub now: String,
}

impl Default for BlankDeckOptions {
    fn default() -> Self {
        BlankDeckOptions { wide: true, title: None, lang: "en-US".to_owned(), now: "1970-01-01T00:00:00Z".to_owned() }
    }
}

fn esc(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;")
}

/// The parts of a blank presentation (one empty title slide), in the order of the TS (`blankDeck`
/// before `PptxPackage.fromParts(...).save()`).
pub fn blank_deck_parts(opts: &BlankDeckOptions) -> RawParts {
    let wide = opts.wide;
    let (w, h) = (if wide { 12_192_000.0 } else { 9_144_000.0 }, 6_858_000.0);
    let g = geometry(w, h);
    let lang = &opts.lang;
    let now = &opts.now;
    let n = ns();
    let mut files: Vec<(String, String)> = Vec::new();
    let mut put = |k: &str, v: String| files.push((k.to_owned(), v));

    let mut types = format!("{DECL}<Types xmlns=\"http://schemas.openxmlformats.org/package/2006/content-types\">");
    types.push_str("<Default Extension=\"rels\" ContentType=\"application/vnd.openxmlformats-package.relationships+xml\"/><Default Extension=\"xml\" ContentType=\"application/xml\"/>");
    types.push_str("<Default Extension=\"png\" ContentType=\"image/png\"/><Default Extension=\"jpeg\" ContentType=\"image/jpeg\"/>");
    types.push_str(&format!("<Override PartName=\"/ppt/presentation.xml\" ContentType=\"{}\"/>", ct("presentation.main")));
    types.push_str(&format!("<Override PartName=\"/ppt/slideMasters/slideMaster1.xml\" ContentType=\"{}\"/>", ct("slideMaster")));
    for i in 1..=6 {
        types.push_str(&format!("<Override PartName=\"/ppt/slideLayouts/slideLayout{i}.xml\" ContentType=\"{}\"/>", ct("slideLayout")));
    }
    types.push_str(&format!("<Override PartName=\"/ppt/slides/slide1.xml\" ContentType=\"{}\"/>", ct("slide")));
    types.push_str(&format!("<Override PartName=\"/ppt/notesMasters/notesMaster1.xml\" ContentType=\"{}\"/>", ct("notesMaster")));
    types
        .push_str("<Override PartName=\"/ppt/theme/theme1.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.theme+xml\"/>");
    types
        .push_str("<Override PartName=\"/ppt/theme/theme2.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.theme+xml\"/>");
    types.push_str(&format!(
        "<Override PartName=\"/ppt/presProps.xml\" ContentType=\"{}\"/><Override PartName=\"/ppt/viewProps.xml\" ContentType=\"{}\"/>",
        ct("presProps"),
        ct("viewProps")
    ));
    types.push_str(&format!("<Override PartName=\"/ppt/tableStyles.xml\" ContentType=\"{}\"/>", ct("tableStyles")));
    types
        .push_str("<Override PartName=\"/docProps/core.xml\" ContentType=\"application/vnd.openxmlformats-package.core-properties+xml\"/>");
    types.push_str("<Override PartName=\"/docProps/app.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.extended-properties+xml\"/></Types>");
    put("[Content_Types].xml", types);

    put(
        "_rels/.rels",
        rels(&[
            ("rId1", "officeDocument", "ppt/presentation.xml"),
            ("rId2", "http://schemas.openxmlformats.org/package/2006/relationships/metadata/core-properties", "docProps/core.xml"),
            ("rId3", "extended-properties", "docProps/app.xml"),
        ]),
    );
    let title = match opts.title.as_deref().filter(|t| !t.is_empty()) {
        Some(t) => format!("<dc:title>{}</dc:title>", esc(t)),
        None => String::new(),
    };
    put(
        "docProps/core.xml",
        format!(
            "{DECL}<cp:coreProperties xmlns:cp=\"http://schemas.openxmlformats.org/package/2006/metadata/core-properties\" xmlns:dc=\"http://purl.org/dc/elements/1.1/\" xmlns:dcterms=\"http://purl.org/dc/terms/\" xmlns:dcmitype=\"http://purl.org/dc/dcmitype/\" xmlns:xsi=\"http://www.w3.org/2001/XMLSchema-instance\">{title}<cp:revision>1</cp:revision><dcterms:created xsi:type=\"dcterms:W3CDTF\">{now}</dcterms:created><dcterms:modified xsi:type=\"dcterms:W3CDTF\">{now}</dcterms:modified></cp:coreProperties>"
        ),
    );
    put(
        "docProps/app.xml",
        format!(
            "{DECL}<Properties xmlns=\"http://schemas.openxmlformats.org/officeDocument/2006/extended-properties\" xmlns:vt=\"http://schemas.openxmlformats.org/officeDocument/2006/docPropsVTypes\"><TotalTime>0</TotalTime><Words>0</Words><Application>Universal Slides</Application><PresentationFormat>{}</PresentationFormat><Slides>1</Slides><Notes>0</Notes></Properties>",
            if wide { "Widescreen" } else { "On-screen Show (4:3)" }
        ),
    );
    put(
        "ppt/presentation.xml",
        format!(
            "{DECL}<p:presentation {n} saveSubsetFonts=\"1\"><p:sldMasterIdLst><p:sldMasterId id=\"2147483648\" r:id=\"rId1\"/></p:sldMasterIdLst><p:notesMasterIdLst><p:notesMasterId r:id=\"rId8\"/></p:notesMasterIdLst><p:sldIdLst><p:sldId id=\"256\" r:id=\"rId2\"/></p:sldIdLst><p:sldSz cx=\"{}\" cy=\"{}\"{}/><p:notesSz cx=\"6858000\" cy=\"9144000\"/><p:defaultTextStyle><a:defPPr><a:defRPr lang=\"en-US\"/></a:defPPr><a:lvl1pPr marL=\"0\" algn=\"l\" defTabSz=\"914400\" rtl=\"0\" eaLnBrk=\"1\" latinLnBrk=\"0\" hangingPunct=\"1\"><a:defRPr sz=\"1800\" kern=\"1200\"><a:solidFill><a:schemeClr val=\"tx1\"/></a:solidFill><a:latin typeface=\"+mn-lt\"/><a:ea typeface=\"+mn-ea\"/><a:cs typeface=\"+mn-cs\"/></a:defRPr></a:lvl1pPr></p:defaultTextStyle></p:presentation>",
            fmt(w),
            fmt(h),
            if wide { "" } else { " type=\"screen4x3\"" }
        ),
    );
    put(
        "ppt/_rels/presentation.xml.rels",
        rels(&[
            ("rId1", "slideMaster", "slideMasters/slideMaster1.xml"),
            ("rId2", "slide", "slides/slide1.xml"),
            ("rId3", "presProps", "presProps.xml"),
            ("rId4", "viewProps", "viewProps.xml"),
            ("rId5", "theme", "theme/theme1.xml"),
            ("rId6", "tableStyles", "tableStyles.xml"),
            ("rId8", "notesMaster", "notesMasters/notesMaster1.xml"),
        ]),
    );
    put(
        "ppt/presProps.xml",
        format!(
            "{DECL}<p:presentationPr {n}><p:showPr showNarration=\"1\"><p:present/><p:sldAll/><p:penClr><a:prstClr val=\"red\"/></p:penClr></p:showPr></p:presentationPr>"
        ),
    );
    put(
        "ppt/viewProps.xml",
        format!(
            "{DECL}<p:viewPr {n}><p:normalViewPr><p:restoredLeft sz=\"15620\"/><p:restoredTop sz=\"94660\"/></p:normalViewPr><p:gridSpacing cx=\"76200\" cy=\"76200\"/></p:viewPr>"
        ),
    );
    put("ppt/tableStyles.xml", format!("{DECL}<a:tblStyleLst xmlns:a=\"{A}\" def=\"{{5C22544A-7EE6-4342-B048-85BDC9FD1C3A}}\"/>"));
    put("ppt/theme/theme1.xml", theme("Office Theme"));
    put("ppt/theme/theme2.xml", theme("Office Theme"));
    put("ppt/slideMasters/slideMaster1.xml", master(&g));
    put(
        "ppt/slideMasters/_rels/slideMaster1.xml.rels",
        rels(&[
            ("rId1", "slideLayout", "../slideLayouts/slideLayout1.xml"),
            ("rId2", "slideLayout", "../slideLayouts/slideLayout2.xml"),
            ("rId3", "slideLayout", "../slideLayouts/slideLayout3.xml"),
            ("rId4", "slideLayout", "../slideLayouts/slideLayout4.xml"),
            ("rId5", "slideLayout", "../slideLayouts/slideLayout5.xml"),
            ("rId6", "slideLayout", "../slideLayouts/slideLayout6.xml"),
            ("rId7", "theme", "../theme/theme1.xml"),
        ]),
    );
    put("ppt/notesMasters/notesMaster1.xml", notes_master());
    put("ppt/notesMasters/_rels/notesMaster1.xml.rels", rels(&[("rId1", "theme", "../theme/theme2.xml")]));
    put(
        "ppt/slides/slide1.xml",
        format!(
            "{DECL}<p:sld {n}><p:cSld><p:spTree>{TREE_HEAD}<p:sp><p:nvSpPr><p:cNvPr id=\"2\" name=\"Title 1\"/><p:cNvSpPr><a:spLocks noGrp=\"1\"/></p:cNvSpPr><p:nvPr><p:ph type=\"ctrTitle\"/></p:nvPr></p:nvSpPr><p:spPr/><p:txBody><a:bodyPr/><a:lstStyle/><a:p><a:endParaRPr lang=\"{lang}\" dirty=\"0\"/></a:p></p:txBody></p:sp><p:sp><p:nvSpPr><p:cNvPr id=\"3\" name=\"Subtitle 2\"/><p:cNvSpPr><a:spLocks noGrp=\"1\"/></p:cNvSpPr><p:nvPr><p:ph type=\"subTitle\" idx=\"1\"/></p:nvPr></p:nvSpPr><p:spPr/><p:txBody><a:bodyPr/><a:lstStyle/><a:p><a:endParaRPr lang=\"{lang}\" dirty=\"0\"/></a:p></p:txBody></p:sp></p:spTree></p:cSld><p:clrMapOvr><a:masterClrMapping/></p:clrMapOvr></p:sld>"
        ),
    );
    put("ppt/slides/_rels/slide1.xml.rels", rels(&[("rId1", "slideLayout", "../slideLayouts/slideLayout1.xml")]));
    for (i, xml) in layouts(&g).into_iter().enumerate() {
        put(&format!("ppt/slideLayouts/slideLayout{}.xml", i + 1), xml);
        put(
            &format!("ppt/slideLayouts/_rels/slideLayout{}.xml.rels", i + 1),
            rels(&[("rId1", "slideMaster", "../slideMasters/slideMaster1.xml")]),
        );
    }
    files.into_iter().map(|(k, v)| (k.into_boxed_str(), Arc::<[u8]>::from(v.into_bytes()))).collect()
}

/// A part store of the blank presentation.
pub fn blank_deck_store(opts: &BlankDeckOptions) -> PartStore {
    PartStore::from_parts(blank_deck_parts(opts))
}
