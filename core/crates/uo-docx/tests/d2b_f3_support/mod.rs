//! Shared helpers of the D2b-F3 tests: the argument tables of the Node capture
//! (`js/capture-d2b-f3-ops.mjs`) and of the Chromium harness (`js/capture-d2b-f3-session.mjs`), the
//! same tables by index, built with the key order of the TS literals.
#![allow(dead_code, clippy::unwrap_used, clippy::expect_used, clippy::panic)]

pub mod hostile;
pub mod ops_codes;
pub mod session_codes;

use uo_docx::js::JsNumber;
use uo_docx::model::props::{
    BorderSpec, CellMargins, ShadingSpec, TblBorders, TblLook, TblPr, TblpPr, TcBorders, TcPr, TrHeight, TrPr, WidthSpec,
};
use uo_docx::session::ops::tables::sort::{SortKey, SortType};
use uo_docx::session::ops::tables::table_ops::TextSeparator;
use uo_docx::word::obj;

pub fn n(x: f64) -> JsNumber {
    JsNumber::Num(x)
}

pub fn w(v: f64, ty: &str) -> WidthSpec {
    WidthSpec { w: n(v), r#type: ty.into(), ..WidthSpec::default() }
}

pub fn widths(i: usize) -> Option<WidthSpec> {
    match i {
        0 => None,
        1 => Some(w(0.0, "auto")),
        2 => Some(w(5000.0, "pct")),
        3 => Some(w(4000.0, "pct")),
        4 => Some(w(6000.0, "dxa")),
        5 => Some(w(2500.4, "dxa")),
        _ => Some(w(5000.0, "nil")),
    }
}
pub const N_WIDTHS: usize = 7;

pub fn bs(val: &str, sz: Option<f64>, space: Option<f64>, color: Option<&str>) -> BorderSpec {
    let mut b = BorderSpec { val: val.into(), ..BorderSpec::default() };
    if let Some(v) = sz {
        obj::set(&mut b, "sz", |x| x.sz = Some(n(v)));
    }
    if let Some(v) = space {
        obj::set(&mut b, "space", |x| x.space = Some(n(v)));
    }
    if let Some(c) = color {
        obj::set(&mut b, "color", |x| x.color = Some(c.into()));
    }
    b
}

pub fn shd(val: &str, color: &str, fill: &str) -> ShadingSpec {
    let mut s = ShadingSpec::default();
    obj::set(&mut s, "val", |x| x.val = Some(val.into()));
    obj::set(&mut s, "color", |x| x.color = Some(color.into()));
    obj::set(&mut s, "fill", |x| x.fill = Some(fill.into()));
    s
}

/// `CELLPATCH[i]`.
pub fn cell_patch(i: usize) -> TcPr {
    let mut p = TcPr::default();
    match i {
        0 => obj::set(&mut p, "vAlign", |x| x.v_align = Some("center".into())),
        1 => obj::set_undefined(&mut p, "vAlign", |x| x.v_align = None),
        2 => obj::set(&mut p, "shd", |x| x.shd = Some(Box::new(shd("clear", "auto", "FFFF00")))),
        3 => obj::set_undefined(&mut p, "shd", |x| x.shd = None),
        4 => {
            let mut m = CellMargins::default();
            obj::set(&mut m, "top", |x| x.top = Some(w(50.0, "dxa")));
            obj::set(&mut m, "left", |x| x.left = Some(w(70.0, "dxa")));
            obj::set(&mut p, "tcMar", |x| x.tc_mar = Some(Box::new(m)));
        }
        5 => {
            obj::set(&mut p, "noWrap", |x| x.no_wrap = Some(true));
            obj::set(&mut p, "tcFitText", |x| x.tc_fit_text = Some(true));
        }
        6 => obj::set(&mut p, "tcW", |x| x.tc_w = Some(w(1800.0, "dxa"))),
        7 => {
            obj::set(&mut p, "textDirection", |x| x.text_direction = Some("tbRl".into()));
            obj::set(&mut p, "vAlign", |x| x.v_align = Some("bottom".into()));
        }
        8 => {
            let mut b = TcBorders::default();
            obj::set(&mut b, "top", |x| x.top = Some(Box::new(bs("single", Some(4.0), Some(0.0), Some("FF0000")))));
            obj::set(&mut p, "tcBorders", |x| x.tc_borders = Some(Box::new(b)));
        }
        _ => {
            obj::set_undefined(&mut p, "gridSpan", |x| x.grid_span = None);
            obj::set_undefined(&mut p, "vMerge", |x| x.v_merge = None);
        }
    }
    p
}
pub const N_CELLPATCH: usize = 10;

/// `ROWPATCH[i]`.
pub fn row_patch(i: usize) -> TrPr {
    let mut p = TrPr::default();
    match i {
        0 => obj::set(&mut p, "cantSplit", |x| x.cant_split = Some(true)),
        1 => obj::set(&mut p, "tblHeader", |x| x.tbl_header = Some(true)),
        2 => {
            obj::set_undefined(&mut p, "tblHeader", |x| x.tbl_header = None);
            obj::set_undefined(&mut p, "cantSplit", |x| x.cant_split = None);
        }
        3 => {
            obj::set(&mut p, "trHeight", |x| x.tr_height = Some(TrHeight { val: n(400.0), h_rule: "exact".into(), ..TrHeight::default() }))
        }
        4 => obj::set(&mut p, "jc", |x| x.jc = Some("center".into())),
        5 => obj::set(&mut p, "cnfStyle", |x| x.cnf_style = Some("100000000000".into())),
        _ => {
            obj::set(&mut p, "hidden", |x| x.hidden = Some(true));
            obj::set(&mut p, "tblCellSpacing", |x| x.tbl_cell_spacing = Some(w(20.0, "dxa")));
        }
    }
    p
}
pub const N_ROWPATCH: usize = 7;

/// `TBLPATCH[i]`.
pub fn tbl_patch(i: usize) -> TblPr {
    let mut p = TblPr::default();
    match i {
        0 => obj::set(&mut p, "tblStyle", |x| x.tbl_style = Some("Tabelacomgrade".into())),
        1 => obj::set_undefined(&mut p, "tblStyle", |x| x.tbl_style = None),
        2 => obj::set(&mut p, "tblLayout", |x| x.tbl_layout = Some("fixed".into())),
        3 => obj::set_undefined(&mut p, "tblLayout", |x| x.tbl_layout = None),
        4 => {
            let mut b = TblBorders::default();
            obj::set(&mut b, "top", |x| x.top = Some(Box::new(bs("single", Some(8.0), Some(0.0), Some("00FF00")))));
            obj::set(&mut b, "insideH", |x| x.inside_h = Some(Box::new(bs("double", Some(4.0), Some(0.0), Some("auto")))));
            obj::set(&mut p, "tblBorders", |x| x.tbl_borders = Some(Box::new(b)));
        }
        5 => obj::set_undefined(&mut p, "tblBorders", |x| x.tbl_borders = None),
        6 => obj::set(&mut p, "shd", |x| x.shd = Some(Box::new(shd("clear", "auto", "CCCCCC")))),
        7 => {
            obj::set(&mut p, "tblInd", |x| x.tbl_ind = Some(w(360.0, "dxa")));
            obj::set(&mut p, "tblCaption", |x| x.tbl_caption = Some("Titulo".into()));
        }
        8 => {
            let mut t = TblpPr::default();
            obj::set(&mut t, "leftFromText", |x| x.left_from_text = Some(n(141.0)));
            obj::set(&mut t, "vertAnchor", |x| x.vert_anchor = Some("text".into()));
            obj::set(&mut t, "tblpY", |x| x.tblp_y = Some(n(1.0)));
            obj::set(&mut p, "tblpPr", |x| x.tblp_pr = Some(Box::new(t)));
            obj::set(&mut p, "tblOverlap", |x| x.tbl_overlap = Some("never".into()));
        }
        9 => {
            obj::set_undefined(&mut p, "tblpPr", |x| x.tblp_pr = None);
            obj::set_undefined(&mut p, "tblOverlap", |x| x.tbl_overlap = None);
            obj::set_undefined(&mut p, "tblInd", |x| x.tbl_ind = None);
        }
        _ => {
            let mut m = CellMargins::default();
            obj::set(&mut m, "top", |x| x.top = Some(w(10.0, "dxa")));
            obj::set(&mut m, "bottom", |x| x.bottom = Some(w(20.0, "dxa")));
            obj::set(&mut p, "tblCellMar", |x| x.tbl_cell_mar = Some(Box::new(m)));
            obj::set(&mut p, "tblDescription", |x| x.tbl_description = Some("Desc".into()));
        }
    }
    p
}
pub const N_TBLPATCH: usize = 11;

/// `SPECS[i]`.
pub fn spec(i: usize) -> Option<BorderSpec> {
    match i {
        0 => None,
        1 => Some(bs("single", Some(4.0), Some(0.0), Some("auto"))),
        2 => Some(bs("double", Some(12.0), Some(0.0), Some("FF0000"))),
        3 => Some(bs("dashed", Some(8.0), Some(0.0), Some("0000FF"))),
        _ => Some(bs("nil", None, None, None)),
    }
}
pub const N_SPECS: usize = 5;

/// `STYLEB[i]`.
pub fn style_borders(i: usize) -> Option<TblBorders> {
    match i {
        0 => None,
        1 => {
            let mut b = TblBorders::default();
            obj::set(&mut b, "top", |x| x.top = Some(Box::new(bs("single", Some(4.0), Some(0.0), Some("auto")))));
            obj::set(&mut b, "insideH", |x| x.inside_h = Some(Box::new(bs("single", Some(4.0), Some(0.0), Some("auto")))));
            Some(b)
        }
        _ => {
            let mut b = TblBorders::default();
            obj::set(&mut b, "left", |x| x.left = Some(Box::new(bs("none", None, None, None))));
            obj::set(&mut b, "right", |x| x.right = Some(Box::new(bs("nil", None, None, None))));
            Some(b)
        }
    }
}
pub const N_STYLEB: usize = 3;

pub const KINDS: [&str; 12] =
    ["bottom", "top", "left", "right", "none", "all", "outside", "inside", "insideH", "insideV", "tl2br", "tr2bl"];

pub fn kind(s: &str) -> uo_docx::session::ops::tables::table_ops::BorderKind {
    use uo_docx::session::ops::tables::table_ops::BorderKind as K;
    match s {
        "bottom" => K::Bottom,
        "top" => K::Top,
        "left" => K::Left,
        "right" => K::Right,
        "none" => K::None,
        "all" => K::All,
        "outside" => K::Outside,
        "inside" => K::Inside,
        "insideH" => K::InsideH,
        "insideV" => K::InsideV,
        "tl2br" => K::Tl2br,
        _ => K::Tr2bl,
    }
}

/// `SHD[i]`.
pub fn shading(i: usize) -> Option<ShadingSpec> {
    match i {
        0 => None,
        1 => Some(shd("clear", "auto", "FF0000")),
        2 => Some(shd("pct25", "auto", "auto")),
        _ => {
            let mut s = shd("clear", "auto", "FFFFFF");
            obj::set(&mut s, "themeFill", |x| x.theme_fill = Some("accent1".into()));
            obj::set(&mut s, "themeFillTint", |x| x.theme_fill_tint = Some("33".into()));
            Some(s)
        }
    }
}
pub const N_SHD: usize = 4;

/// `LOOKS[i]`.
pub fn look(i: usize) -> TblLook {
    let mut l = TblLook::default();
    match i {
        0 => {}
        1 => obj::set(&mut l, "firstRow", |x| x.first_row = Some(false)),
        2 => {
            obj::set(&mut l, "lastRow", |x| x.last_row = Some(true));
            obj::set(&mut l, "noHBand", |x| x.no_h_band = Some(true));
        }
        _ => {
            obj::set(&mut l, "firstColumn", |x| x.first_column = Some(false));
            obj::set(&mut l, "lastColumn", |x| x.last_column = Some(true));
            obj::set(&mut l, "noVBand", |x| x.no_v_band = Some(false));
        }
    }
    l
}
pub const N_LOOKS: usize = 4;

pub const DIRS: [Option<&str>; 5] = [Some("lrTb"), Some("tbRl"), Some("btLr"), Some("lrTbV"), None];
pub const JCS: [Option<&str>; 4] = [Some("left"), Some("center"), Some("right"), None];

pub fn sep(i: usize) -> TextSeparator {
    match i {
        0 => TextSeparator::Tab,
        1 => TextSeparator::Paragraph,
        2 => TextSeparator::Semicolon,
        3 => TextSeparator::Comma,
        4 => TextSeparator::Char(vec![u16::from(b'|')]),
        _ => TextSeparator::Char(Vec::new()),
    }
}
pub const N_SEPS: usize = 6;

pub fn sort_keys(i: usize) -> Vec<SortKey> {
    let k = |col: i64, ty: SortType, desc: bool| SortKey { col, ty, desc };
    match i {
        0 => vec![k(0, SortType::Text, false)],
        1 => vec![k(0, SortType::Number, true)],
        2 => vec![k(1, SortType::Date, false), k(0, SortType::Text, true)],
        3 => vec![k(0, SortType::Text, false), k(1, SortType::Number, false)],
        _ => vec![k(2, SortType::Text, true)],
    }
}
pub const N_SORTKEYS: usize = 5;

pub const FORMULAS: [&str; 47] = [
    "=SUM(ABOVE)",
    "=SUM(LEFT)",
    "=SUM(RIGHT)",
    "=SUM(BELOW)",
    "=AVERAGE(ABOVE)",
    "=MAX(LEFT)",
    "=A1+B2",
    "=SUM(A1:B2)",
    "=2^10",
    "=2^3^2",
    "=-2^2",
    "=ROUND(10/3;2)",
    "=IF(1<2;10;20)",
    "=10%",
    "=1/0",
    "=1+",
    "=PRODUCT(1;2;3;4)",
    "=MOD(10;3)",
    "=INT(-3,7)",
    "=SIGN(-5)",
    "=ABS(-2,5)",
    "=AND(1;0)",
    "=OR(0;0)",
    "=NOT(0)",
    "=COUNT(ABOVE)",
    "=MIN(1;2;3)",
    "=SUM(ABOVE)*2",
    "= 1+2 ",
    "=3,5*2",
    "=1.000,5+1",
    "=foo",
    "=(1+2)*3",
    "=ROUND(2,5;0)",
    "=ROUND(-2,5;0)",
    "=ROUND(1234,5678;-2)",
    "=A0",
    "=ZZ99",
    "=SUM(A1:A1)",
    "=TRUE+1",
    "=DEFINED(x)",
    "=1 2",
    "=)",
    "=2^0,5",
    "=(2^2)^2",
    "=10^-2",
    "=A1:B2",
    "=SUM(A1:B2;C3)",
];
pub const PICS: [Option<&str>; 8] =
    [None, Some("0,00"), Some("#.##0,00"), Some("0%"), Some("0;(0);zero"), Some("'x'0"), Some("#"), Some("0,0000")];
pub const NUMS: [&str; 29] = [
    "10",
    " R$ 1.234,56",
    "(4)",
    "5%",
    "-3,5",
    "1.000",
    "1.2345",
    "abc",
    "",
    "US$ 2",
    "€3",
    "£ 4,0",
    "1,5,5",
    "+7",
    "12.345.678,9 x",
    "$ (5,5)",
    "(5",
    "5)",
    "٣",
    "0,10",
    "1.00",
    ".5",
    "-.5",
    "100%",
    "R$",
    "2 itens",
    "\t8",
    "1.234.5",
    "12345.678",
];
pub const DATES: [&str; 15] = [
    "1/2/2020",
    "31/12/99",
    "2020-02-30",
    "29.02.2024",
    "1-1-30",
    "0050-02-29",
    "31/4/2020",
    "1 de março de 2020",
    "2020-1-5",
    "99/9/9999",
    "5/5/05",
    " 3/3/2003 ",
    "",
    "01.13.2020",
    "2023-12-31",
];

/// The pseudo row heights of the ops capture.
pub fn heights(n: usize) -> Vec<f64> {
    (0..n).map(|i| 240.0 + 61.0 * f64::from(u32::try_from(i).unwrap())).collect()
}
