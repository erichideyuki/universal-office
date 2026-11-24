//! The text surgery of `updateWorksheetXml` (`serialize.ts:138-254`) on the frame of a worksheet part: `<cols>`,
//! `<mergeCells>` and the first `<sheetView>` (gridlines, frozen pane). The regular expressions are the TS's, run
//! on the same ECMAScript engine uo-calc uses (`regress`), so the matching (first occurrence, `\b`, lazy
//! `[^>]*?`, JS white space) is the TS's by construction.
//!
//! The TS runs them on the whole part text; here on `prefix + <sheetData/> + suffix` (the frame, typically a few
//! KB), and the streamed `<sheetData>` replaces the sentinel afterwards.

use super::SaveError;
use super::rx::Rx;
use super::text::{esc_attr, put_num};
use crate::read::model::{ColDef, PaneInfo, Worksheet};

/// Elements of CT_Worksheet after `<mergeCells>` (schema order).
const AFTER_MERGES: [&str; 24] = [
    "phoneticPr",
    "conditionalFormatting",
    "dataValidations",
    "hyperlinks",
    "printOptions",
    "pageMargins",
    "pageSetup",
    "headerFooter",
    "rowBreaks",
    "colBreaks",
    "customProperties",
    "cellWatches",
    "ignoredErrors",
    "smartTags",
    "drawing",
    "legacyDrawing",
    "legacyDrawingHF",
    "drawingHF",
    "picture",
    "oleObjects",
    "controls",
    "webPublishItems",
    "tableParts",
    "extLst",
];
/// Elements after `<sheetViews>`.
const AFTER_VIEWS: [&str; 3] = ["sheetFormatPr", "cols", "sheetData"];

/// `/<((?:\w+:)?)worksheet\b/.exec(xml)?.[1] ?? ''`.
fn root_prefix(xml: &str) -> Result<String, SaveError> {
    let re = Rx::new(r"<((?:\w+:)?)worksheet\b")?;
    Ok(re.find(xml, 0).and_then(|m| m.group(1)).map(|r| xml[r].to_owned()).unwrap_or_default())
}

/// `insertionPoint`: before the first element of `before` (at the worksheet level), else before `</worksheet>`.
fn insertion_point(xml: &str, p: &str, before: &[&str]) -> Result<Option<usize>, SaveError> {
    let mut at: Option<usize> = None;
    for name in before {
        let re = Rx::new(&format!(r"<{p}{name}[\s/>]"))?;
        if let Some(m) = re.find(xml, 0)
            && at.is_none_or(|a| m.start() < a)
        {
            at = Some(m.start());
        }
    }
    Ok(at.or_else(|| xml.rfind(&format!("</{p}worksheet>"))))
}

/// `serializeMergeCells`.
fn merge_block(merges: &[String], p: &str) -> String {
    if merges.is_empty() {
        return String::new();
    }
    let mut s = format!("<{p}mergeCells count=\"{}\">", merges.len());
    for m in merges {
        s.push_str(&format!("<{p}mergeCell ref=\"{}\"/>", esc_attr(m)));
    }
    s.push_str(&format!("</{p}mergeCells>"));
    s
}

fn replace_merge_cells(xml: &str, merges: &[String]) -> Result<String, SaveError> {
    let p = root_prefix(xml)?;
    let block = merge_block(merges, &p);
    if let Some(m) = Rx::new(&format!(r"<{p}mergeCells[\s/>]"))?.find(xml, 0) {
        let open = m.start();
        let end = match Rx::new(&format!(r"^<{p}mergeCells\b[^>]*/>"))?.find(&xml[open..], 0) {
            Some(s) => open + s.end(),
            None => {
                let close = format!("</{p}mergeCells>");
                match xml[open..].find(&close) {
                    Some(i) => open + i + close.len(),
                    None => return Err(SaveError::Frame("unclosed <mergeCells>")),
                }
            }
        };
        return Ok(format!("{}{}{}", &xml[..open], block, &xml[end..]));
    }
    if block.is_empty() {
        return Ok(xml.to_owned());
    }
    match insertion_point(xml, &p, &AFTER_MERGES)? {
        Some(at) => Ok(format!("{}{}{}", &xml[..at], block, &xml[at..])),
        None => Ok(xml.to_owned()),
    }
}

/// `rowColToRef(col, row)` on JS numbers.
fn row_col_ref(col: f64, row: f64) -> String {
    let mut c = col;
    let mut letters: Vec<char> = Vec::new();
    while c > 0.0 {
        let rem = (c - 1.0) % 26.0;
        letters.push(char::from(65 + rem as u8));
        c = ((c - 1.0) / 26.0).floor();
    }
    letters.reverse();
    let mut s: String = letters.into_iter().collect();
    s.push_str(&num_text(row));
    s
}

fn num_text(n: f64) -> String {
    let mut b = Vec::new();
    put_num(&mut b, n);
    String::from_utf8_lossy(&b).into_owned()
}

/// `paneXml`.
fn pane_xml(pane: Option<&PaneInfo>, p: &str) -> String {
    let Some(pane) = pane else { return String::new() };
    if pane.state != "frozen" && pane.state != "frozenSplit" {
        return String::new();
    }
    let clamp = |v: f64| if v.is_nan() { f64::NAN } else { uo_jsnum::math::max(0.0, v.floor()) };
    let (x, y) = (clamp(pane.x_split), clamp(pane.y_split));
    let truthy = |v: f64| !v.is_nan() && v != 0.0;
    let (xt, yt) = (truthy(x), truthy(y));
    if !xt && !yt {
        return String::new();
    }
    let active = if xt && yt {
        "bottomRight"
    } else if xt {
        "topRight"
    } else {
        "bottomLeft"
    };
    let tl = pane.top_left_cell.clone().unwrap_or_else(|| row_col_ref(x + 1.0, y + 1.0));
    format!(
        "<{p}pane{}{} topLeftCell=\"{}\" activePane=\"{active}\" state=\"frozen\"/>",
        if xt { format!(" xSplit=\"{}\"", num_text(x)) } else { String::new() },
        if yt { format!(" ySplit=\"{}\"", num_text(y)) } else { String::new() },
        esc_attr(&tl)
    )
}

/// `grid(open)` of `updateSheetView`.
fn grid(open: &str, off: bool, p: &str) -> Result<String, SaveError> {
    let mut o = match Rx::new(r#"\sshowGridLines="[^"]*""#)?.find(open, 0) {
        Some(m) => format!("{}{}", &open[..m.start()], &open[m.end()..]),
        None => open.to_owned(),
    };
    if off {
        let head = format!("<{p}sheetView");
        if o.starts_with(&head) {
            o = format!("{head} showGridLines=\"0\"{}", &o[head.len()..]);
        }
    }
    Ok(o)
}

/// `body.replace(/<p name\b[^>]*?(?:\/>|>[\s\S]*?<\/p name>)/g, '')`.
fn strip_elements(body: &str, p: &str, name: &str) -> Result<String, SaveError> {
    let re = Rx::new(&format!(r"<{p}{name}\b[^>]*?(?:/>|>[\s\S]*?</{p}{name}>)"))?;
    let mut out = String::with_capacity(body.len());
    let mut pos = 0;
    for m in re.find_all(body) {
        out.push_str(&body[pos..m.start()]);
        pos = m.end();
    }
    out.push_str(&body[pos..]);
    Ok(out)
}

fn update_sheet_view(xml: &str, ws: &Worksheet) -> Result<String, SaveError> {
    let p = root_prefix(xml)?;
    let pane = pane_xml(ws.pane.as_ref(), &p);
    let off = ws.show_grid_lines_off;
    let Some(m) = Rx::new(&format!(r"<{p}sheetView\b([^>]*?)(/?)>"))?.find(xml, 0) else {
        let tail = if pane.is_empty() { "/>".to_owned() } else { format!(">{pane}</{p}sheetView>") };
        let views = format!("<{p}sheetViews>{}{tail}</{p}sheetViews>", grid(&format!("<{p}sheetView workbookViewId=\"0\""), off, &p)?);
        return Ok(match insertion_point(xml, &p, &AFTER_VIEWS)? {
            Some(at) => format!("{}{views}{}", &xml[..at], &xml[at..]),
            None => xml.to_owned(),
        });
    };
    let attrs = m.group(1).map_or("", |r| &xml[r]);
    let slash = m.group(2).is_some_and(|r| !r.is_empty());
    // `m[1]!.replace(/\s+$/, '')`
    let open = grid(&format!("<{p}sheetView{}", uo_jsnum::text::trim_end(attrs)), off, &p)?;
    let (start, head_end) = (m.start(), m.end());
    if slash {
        let tail = if pane.is_empty() { "/>".to_owned() } else { format!(">{pane}</{p}sheetView>") };
        return Ok(format!("{}{open}{tail}{}", &xml[..start], &xml[head_end..]));
    }
    let Some(close) = xml[head_end..].find(&format!("</{p}sheetView>")).map(|i| head_end + i) else { return Ok(xml.to_owned()) };
    let inner = &xml[head_end..close];
    // the file's own `<pane>`: when it is the model's, only the gridlines changed
    let cur = Rx::new(&format!(r"<{p}pane\b([^>]*)"))?.find(inner, 0).and_then(|c| c.group(1)).map(|r| &inner[r]);
    let attr = |n: &str| -> Result<Option<&str>, SaveError> {
        let Some(c) = cur else { return Ok(None) };
        let re = Rx::new(&format!(r#"\s{n}="([^"]*)""#))?;
        Ok(re.find(c, 0).and_then(|a| a.group(1)).map(|r| &c[r]))
    };
    let num = |v: Option<&str>| v.map_or(0.0, uo_jsnum::parse::to_number);
    let same_pane = match cur {
        Some(_) => match ws.pane.as_ref() {
            Some(wp) => {
                attr("state")? == Some(wp.state.as_str()) && num(attr("xSplit")?) == wp.x_split && num(attr("ySplit")?) == wp.y_split
            }
            None => false,
        },
        None => pane.is_empty(),
    };
    if same_pane {
        return Ok(format!("{}{open}>{}", &xml[..start], &xml[head_end..]));
    }
    let body = strip_elements(&strip_elements(inner, &p, "pane")?, &p, "selection")?;
    Ok(format!("{}{open}>{pane}{body}{}", &xml[..start], &xml[close..]))
}

/// `serializeCols`.
pub(crate) fn cols_block(cols: &[ColDef]) -> String {
    if cols.is_empty() {
        return String::new();
    }
    let mut s = String::from("<cols>");
    for c in cols {
        s.push_str(&format!("<col min=\"{}\" max=\"{}\"", num_text(c.min), num_text(c.max)));
        if let Some(w) = c.width {
            s.push_str(&format!(" width=\"{}\"", num_text(w)));
        }
        if c.custom_width {
            s.push_str(" customWidth=\"1\"");
        }
        if c.hidden {
            s.push_str(" hidden=\"1\"");
        }
        if let Some(st) = c.style {
            s.push_str(&format!(" style=\"{}\"", num_text(st)));
        }
        s.push_str("/>");
    }
    s.push_str("</cols>");
    s
}

/// What else to rewrite besides `<sheetData>`/`<cols>` (`WorksheetXmlOptions`).
#[derive(Clone, Copy, Debug, Default)]
pub struct FrameEdits {
    /// `<mergeCells>` from the model's merges.
    pub merges: bool,
    /// `showGridLines` and the frozen `<pane>` of the first `<sheetView>`.
    pub sheet_view: bool,
}

/// The sentinel the streamed `<sheetData>` replaces.
const SENTINEL: &str = "<sheetData/>";

/// `updateWorksheetXml` on the frame: the new `(prefix, suffix)` around the streamed `<sheetData>`.
pub(crate) fn update_frame(prefix: &str, suffix: &str, ws: &Worksheet, edits: FrameEdits) -> Result<(String, String), SaveError> {
    let mut xml = String::with_capacity(prefix.len() + suffix.len() + SENTINEL.len());
    xml.push_str(prefix);
    xml.push_str(SENTINEL);
    xml.push_str(suffix);
    if edits.merges {
        xml = replace_merge_cells(&xml, &ws.merges)?;
    }
    if edits.sheet_view {
        xml = update_sheet_view(&xml, ws)?;
    }
    let sd = Rx::new(r"<sheetData[\s/>]")?;
    if sd.find(&xml, 0).is_none() {
        return Err(SaveError::Frame("worksheet without <sheetData>"));
    }
    let cols = cols_block(&ws.cols);
    if let Some(m) = Rx::new(r"<cols[\s>]")?.find(&xml, 0) {
        let open = m.start();
        let Some(e) = xml[open..].find("</cols>") else { return Err(SaveError::Frame("unclosed <cols>")) };
        xml = format!("{}{cols}{}", &xml[..open], &xml[open + e + "</cols>".len()..]);
    } else if !cols.is_empty()
        && let Some(m) = sd.find(&xml, 0)
    {
        xml = format!("{}{cols}{}", &xml[..m.start()], &xml[m.start()..]);
    }
    let Some(m) = sd.find(&xml, 0) else { return Err(SaveError::Frame("worksheet without <sheetData>")) };
    let at = m.start();
    if !xml[at..].starts_with(SENTINEL) {
        return Err(SaveError::Frame("<sheetData> not where the frame put it"));
    }
    Ok((xml[..at].to_owned(), xml[at + SENTINEL.len()..].to_owned()))
}
