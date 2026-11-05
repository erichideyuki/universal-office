//! HTML export (docx-core `export/html.ts`): semantic, fluid (not paginated) HTML for reading on the
//! web: headings, nested lists, tables, inline images (data URI), footnotes and endnotes, and the
//! comments as highlights plus a list. The output is built in UTF-16 code units and converted at the
//! end (see [`crate::u16buf`]).
//!
//! Faithful to the TS, quirks included: a note's text is HTML-escaped twice; `HIGHLIGHTS[name]` of an
//! `Object.prototype` name gives the function's source text; the list builder of a run of list items
//! stops at the first item shallower than the first one (the rest of the run is dropped); the
//! cells of a table are finalized (top-level paragraphs only) and its nested tables are not.

use std::borrow::Cow;

use uo_jsnum::format::to_js_string;
use uo_jsnum::math;

use crate::final_view::final_paragraphs;
use crate::js::JsNumber;
use crate::model::para::BorderEdge;
use crate::model::run::RunProps;
use crate::model::{Comment, Paragraph, Run, Table};
use crate::u16buf::{U16Buf, trim_units};

use super::model::{
    ExportDoc, FinalDoc, NumberKey, OrderedBlock, OrderedNumbers, comments_by_id, escape_attr_into, escape_attr_str_into, escape_html_into,
    escape_html_str_into, half_pt_to_pt, heading_level, resolve_image_uri, twips_to_pt, twips_to_px,
};
use super::style::STYLE;

/// How the images are embedded.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ImagesMode {
    /// base64 inline (default; one huge file)
    #[default]
    Data,
    /// no image: a placeholder
    Omit,
}

/// `HtmlOptions`.
#[derive(Debug, Clone, Default)]
pub struct HtmlOptions<'a> {
    /// a complete document with `<head>`/`<style>` (`true`, default) or only the body fragment
    pub full_document: Option<bool>,
    pub title: Option<&'a str>,
    pub images: Option<ImagesMode>,
}

/// The deepest table nesting rendered (the reader stops at `max_nesting` = 64 stories; a model built
/// otherwise is cut here).
const MAX_TABLE_DEPTH: usize = 256;

struct Ctx<'a, 'd> {
    fin: &'d FinalDoc<'a>,
    comments: std::collections::HashMap<NumberKey, &'d Comment>,
    used_comments: OrderedNumbers,
    notes: Vec<U16Buf>,
    images_omit: bool,
}

/// `HIGHLIGHTS[name] ?? name`: the table, and what V8 gives for the names `Object.prototype` has.
fn highlight_css(name: &str) -> Cow<'_, str> {
    let hex = match name {
        "yellow" => "#ffff00",
        "green" => "#00ff00",
        "cyan" => "#00ffff",
        "magenta" => "#ff00ff",
        "blue" => "#0000ff",
        "red" => "#ff0000",
        "darkBlue" => "#000080",
        "darkCyan" => "#008080",
        "darkGreen" => "#008000",
        "darkMagenta" => "#800080",
        "darkRed" => "#800000",
        "darkYellow" => "#808000",
        "darkGray" => "#808080",
        "lightGray" => "#c0c0c0",
        "black" => "#000000",
        "white" => "#ffffff",
        "constructor" => return Cow::Borrowed("function Object() { [native code] }"),
        "__proto__" => return Cow::Borrowed("[object Object]"),
        "toString"
        | "valueOf"
        | "hasOwnProperty"
        | "isPrototypeOf"
        | "propertyIsEnumerable"
        | "toLocaleString"
        | "__defineGetter__"
        | "__defineSetter__"
        | "__lookupGetter__"
        | "__lookupSetter__" => {
            return Cow::Owned(format!("function {name}() {{ [native code] }}"));
        }
        _ => return Cow::Borrowed(name),
    };
    Cow::Borrowed(hex)
}

fn css_border_style(s: &str) -> &'static str {
    match s {
        "dashed" => "dashed",
        "dotted" => "dotted",
        "double" => "double",
        "none" | "nil" => "none",
        _ => "solid",
    }
}

fn edge_css(e: &BorderEdge) -> String {
    let w = math::max(0.5, e.size_eighth_pt.to_number() / 8.0);
    let color = match e.color.as_deref() {
        Some(c) if !c.is_empty() => format!("#{c}"),
        _ => "#000".to_owned(),
    };
    format!("{}pt {} {}", to_js_string(w), css_border_style(&e.style), color)
}

fn truthy_str(s: Option<&str>) -> Option<&str> {
    s.filter(|s| !s.is_empty())
}

fn truthy_num(n: Option<JsNumber>) -> Option<JsNumber> {
    n.filter(|n| n.truthy())
}

fn flag(b: Option<bool>) -> bool {
    b.unwrap_or(false)
}

fn run_styles(p: &RunProps) -> Vec<String> {
    let mut styles = Vec::new();
    if let Some(c) = truthy_str(p.color.as_deref()) {
        styles.push(format!("color:#{c}"));
    }
    if let Some(n) = truthy_num(p.sz_half_pt) {
        styles.push(format!("font-size:{}pt", to_js_string(half_pt_to_pt(n))));
    }
    if let Some(h) = truthy_str(p.highlight.as_deref()) {
        styles.push(format!("background-color:{}", highlight_css(h)));
    }
    if let Some(f) = truthy_str(p.font_ascii.as_deref()) {
        styles.push(format!("font-family:'{f}',serif"));
    }
    if flag(p.all_caps) {
        styles.push("text-transform:uppercase".to_owned());
    }
    if flag(p.small_caps) {
        styles.push("font-variant:small-caps".to_owned());
    }
    if let Some(n) = truthy_num(p.char_spacing_twips) {
        styles.push(format!("letter-spacing:{}pt", to_js_string(twips_to_pt(n))));
    }
    styles
}

fn run_html(run: &Run, ctx: &mut Ctx<'_, '_>) -> U16Buf {
    let mut out = U16Buf::new();
    let ext = run.ext();
    if let Some(img) = ext.and_then(|e| e.image.as_deref()) {
        if ctx.images_omit {
            out.push_str(
                "<span class=\"uw-img-placeholder\" aria-label=\"imagem\" style=\"display:inline-block;padding:0 .35em;margin:0 .15em;background:rgba(0,0,0,.06);border-radius:4px;font-size:.9em;color:#777\">📷</span>",
            );
            return out;
        }
        out.push_str("<img src=\"");
        out.push_str(&resolve_image_uri(img, &ctx.fin.doc));
        out.push_str("\" width=\"");
        out.push_num(twips_to_px(img.width_twips));
        out.push_str("\" height=\"");
        out.push_num(twips_to_px(img.height_twips));
        out.push_str("\" alt=\"\" loading=\"lazy\">");
        return out;
    }
    let p = &*run.props;
    if let Some(nr) = ext.and_then(|e| e.note_ref.as_ref()) {
        let seq = ctx.notes.len() + 1;
        let mut note = U16Buf::new();
        if let Some(paras) = ctx.fin.note_paragraphs(&*nr.kind == "footnote", nr.id) {
            let mut joined = U16Buf::new();
            for (i, np) in paras.paras.iter().enumerate() {
                if i > 0 {
                    joined.push_unit(0x20);
                }
                for r in &np.runs {
                    escape_html_into(&mut joined, r.text.units());
                }
            }
            escape_html_into(&mut note, trim_units(joined.units()));
        }
        ctx.notes.push(note);
        out.push_str("<sup class=\"uw-note\"><a id=\"noteref-");
        out.push_str(&seq.to_string());
        out.push_str("\" href=\"#note-");
        out.push_str(&seq.to_string());
        out.push_str("\">");
        out.push_str(&seq.to_string());
        out.push_str("</a></sup>");
        return out;
    }
    let mut text = U16Buf::new();
    escape_html_into(&mut text, run.text.units());
    if text.is_empty() {
        return out;
    }

    let styles = run_styles(p);
    let mut pre: Vec<String> = Vec::new();
    let mut post: Vec<&str> = Vec::new();
    if flag(p.bold) {
        pre.push("<strong>".to_owned());
        post.push("</strong>");
    }
    if flag(p.italic) {
        pre.push("<em>".to_owned());
        post.push("</em>");
    }
    if flag(p.strike) || flag(p.double_strike) {
        pre.push("<s>".to_owned());
        post.push("</s>");
    }
    if flag(p.underline) {
        pre.push("<u>".to_owned());
        post.push("</u>");
    }
    match p.vert_align.as_deref() {
        Some("superscript") => {
            pre.push("<sup>".to_owned());
            post.push("</sup>");
        }
        Some("subscript") => {
            pre.push("<sub>".to_owned());
            post.push("</sub>");
        }
        _ => {}
    }
    if !styles.is_empty() {
        pre.push(format!("<span style=\"{}\">", styles.join(";")));
        post.push("</span>");
    }
    let mut link_open = U16Buf::new();
    if let Some(l) = truthy_str(p.link.as_deref()) {
        link_open.push_str("<a href=\"");
        escape_attr_str_into(&mut link_open, l);
        link_open.push_str("\">");
    }
    let mut comment_open = U16Buf::new();
    let has_comments = p.comment_ids.as_ref().is_some_and(|c| !c.is_empty());
    if has_comments && let Some(ids) = &p.comment_ids {
        for &id in ids {
            ctx.used_comments.add(id);
        }
        let mut tips = U16Buf::new();
        let mut first = true;
        for &id in ids {
            let Some(c) = ctx.comments.get(&NumberKey::from(id)) else { continue };
            if !first {
                tips.push_str(" | ");
            }
            first = false;
            tips.push_str(c.author.as_deref().unwrap_or("Anônimo"));
            tips.push_str(": ");
            tips.push_units(c.text.units());
        }
        comment_open.push_str("<span class=\"uw-comment\" data-comment=\"");
        let ids_text: Vec<String> = ids.iter().map(|&id| join_number(id)).collect();
        comment_open.push_str(&ids_text.join(","));
        comment_open.push_str("\" title=\"");
        escape_attr_into(&mut comment_open, tips.units());
        comment_open.push_str("\">");
    }

    if has_comments {
        out.push_buf(&comment_open);
    }
    out.push_buf(&link_open);
    for open in pre.iter().rev() {
        out.push_str(open);
    }
    out.push_buf(&text);
    for close in &post {
        out.push_str(close);
    }
    if !link_open.is_empty() {
        out.push_str("</a>");
    }
    if has_comments {
        out.push_str("</span>");
    }
    out
}

/// `[…].join(',')` of a number: `null` is the empty string.
fn join_number(n: JsNumber) -> String {
    match n {
        JsNumber::Null => String::new(),
        JsNumber::Num(x) => to_js_string(x),
    }
}

fn para_styles(p: &Paragraph) -> String {
    let mut s: Vec<String> = Vec::new();
    let props = &*p.props;
    match props.alignment.as_deref() {
        Some("center") => s.push("text-align:center".to_owned()),
        Some("right") => s.push("text-align:right".to_owned()),
        Some("both") => s.push("text-align:justify".to_owned()),
        _ => {}
    }
    if let Some(n) = truthy_num(props.indent_left_twips) {
        s.push(format!("margin-left:{}pt", to_js_string(twips_to_pt(n))));
    }
    if let Some(n) = truthy_num(props.indent_right_twips) {
        s.push(format!("margin-right:{}pt", to_js_string(twips_to_pt(n))));
    }
    if let Some(n) = truthy_num(props.indent_first_line_twips) {
        s.push(format!("text-indent:{}pt", to_js_string(twips_to_pt(n))));
    }
    if let Some(h) = truthy_str(props.shading_hex.as_deref()) {
        s.push(format!("background-color:#{h}"));
    }
    if let Some(b) = &props.borders {
        if let Some(e) = &b.top {
            s.push(format!("border-top:{}", edge_css(e)));
        }
        if let Some(e) = &b.bottom {
            s.push(format!("border-bottom:{}", edge_css(e)));
        }
        if let Some(e) = &b.left {
            s.push(format!("border-left:{}", edge_css(e)));
        }
        if let Some(e) = &b.right {
            s.push(format!("border-right:{}", edge_css(e)));
        }
    }
    s.join(";")
}

fn para_inner(p: &Paragraph, ctx: &mut Ctx<'_, '_>) -> U16Buf {
    let mut html = U16Buf::new();
    for r in &p.runs {
        let h = run_html(r, ctx);
        html.push_buf(&h);
    }
    if html.is_empty() {
        html.push_str("<br>");
    }
    html
}

fn para_html(p: &Paragraph, ctx: &mut Ctx<'_, '_>) -> U16Buf {
    let inner = para_inner(p, ctx);
    let st = para_styles(p);
    let attr = if st.is_empty() { String::new() } else { format!(" style=\"{st}\"") };
    let lvl = heading_level(p);
    let mut out = U16Buf::new();
    if lvl > 0 {
        out.push_str(&format!("<h{lvl}{attr}>"));
        out.push_buf(&inner);
        out.push_str(&format!("</h{lvl}>"));
    } else {
        out.push_str(&format!("<p{attr}>"));
        out.push_buf(&inner);
        out.push_str("</p>");
    }
    out
}

/// `tableHtml`: `finalize` is true for the tables of the body (their cells' paragraphs are in the
/// final view) and false for the tables nested in a cell (`cell.tables` is not finalized).
fn table_html(t: &Table, ctx: &mut Ctx<'_, '_>, finalize: bool, depth: usize) -> U16Buf {
    let mut html = U16Buf::new();
    html.push_str("<table>");
    if depth > MAX_TABLE_DEPTH {
        html.push_str("</table>");
        return html;
    }
    for row in &t.rows {
        html.push_str("<tr>");
        for cell in &row.cells {
            let span = match cell.grid_span {
                n if n.to_number() > 1.0 && !n.is_null() => format!(" colspan=\"{}\"", join_number(n)),
                _ => String::new(),
            };
            let style = match truthy_str(cell.shading_hex.as_deref()) {
                Some(h) => format!(" style=\"background-color:#{h}\""),
                None => String::new(),
            };
            let mut inner = U16Buf::new();
            if finalize {
                let fp = final_paragraphs(cell.content.paras.iter());
                for p in &fp.paras {
                    let h = para_html(p, ctx);
                    inner.push_buf(&h);
                }
            } else {
                for p in cell.content.paras.iter() {
                    let h = para_html(p, ctx);
                    inner.push_buf(&h);
                }
            }
            for a in cell.content.tables.iter() {
                let h = table_html(&a.table, ctx, false, depth + 1);
                inner.push_buf(&h);
            }
            html.push_str(&format!("<td{span}{style}>"));
            html.push_buf(&inner);
            html.push_str("</td>");
        }
        html.push_str("</tr>");
    }
    html.push_str("</table>");
    html
}

struct ListItem {
    ilvl: f64,
    decimal: bool,
    inner: U16Buf,
}

struct ListFrame {
    level: f64,
    tag: &'static str,
    html: U16Buf,
    li: Option<U16Buf>,
}

fn open_frame(items: &[ListItem], i: usize) -> Option<ListFrame> {
    let item = items.get(i)?;
    let tag = if item.decimal { "ol" } else { "ul" };
    let mut html = U16Buf::new();
    html.push_str(&format!("<{tag}>"));
    Some(ListFrame { level: item.ilvl, tag, html, li: None })
}

/// `listHtml`: the nested list of a run of list items, built as the TS `build` does (one call from
/// the first item's level; an item shallower than that, or deeper after a shallower one, ends the
/// list and the items left are dropped), with an explicit stack instead of the recursion.
fn list_html(items: &[ListItem]) -> U16Buf {
    let Some(first) = open_frame(items, 0) else { return U16Buf::new() };
    let mut stack = vec![first];
    let mut i = 0usize;
    loop {
        let Some(top) = stack.last_mut() else { return U16Buf::new() };
        let level = top.level;
        // the TS `it.ilvl >= level && !(it.ilvl > level)`: false for NaN on either side, as `==`
        let here = items.get(i).filter(|it| it.ilvl == level);
        if let Some(item) = here {
            let mut li = U16Buf::new();
            li.push_str("<li>");
            li.push_buf(&item.inner);
            i += 1;
            let deeper = items.get(i).is_some_and(|n| n.ilvl > level);
            if deeper && let Some(child) = open_frame(items, i) {
                top.li = Some(li);
                stack.push(child);
            } else {
                li.push_str("</li>");
                top.html.push_buf(&li);
            }
            continue;
        }
        let Some(mut done) = stack.pop() else { return U16Buf::new() };
        done.html.push_str(&format!("</{}>", done.tag));
        match stack.last_mut() {
            None => return done.html,
            Some(parent) => {
                let mut li = parent.li.take().unwrap_or_default();
                li.push_buf(&done.html);
                li.push_str("</li>");
                parent.html.push_buf(&li);
            }
        }
    }
}

fn body_html(fin: &FinalDoc<'_>, ctx: &mut Ctx<'_, '_>) -> U16Buf {
    let blocks = fin.ordered_blocks();
    let mut parts: Vec<U16Buf> = Vec::new();
    let mut list_buf: Vec<ListItem> = Vec::new();
    for b in &blocks {
        if let OrderedBlock::Para(p) = b
            && let Some(num) = &p.props.numbering
        {
            let inner = para_inner(p, ctx);
            list_buf.push(ListItem { ilvl: num.ilvl.to_number(), decimal: &*num.kind == "decimal", inner });
            continue;
        }
        if !list_buf.is_empty() {
            parts.push(list_html(&list_buf));
            list_buf.clear();
        }
        match b {
            OrderedBlock::Para(p) => parts.push(para_html(p, ctx)),
            OrderedBlock::Table(t) => parts.push(table_html(t, ctx, true, 0)),
        }
    }
    if !list_buf.is_empty() {
        parts.push(list_html(&list_buf));
        list_buf.clear();
    }

    if !ctx.notes.is_empty() {
        parts.push(buf("<hr><section class=\"uw-notes\"><ol>"));
        for (idx, n) in ctx.notes.iter().enumerate() {
            let mut li = U16Buf::new();
            li.push_str(&format!("<li id=\"note-{}\">", idx + 1));
            li.push_buf(n);
            li.push_str(&format!(" <a href=\"#noteref-{}\">↩</a></li>", idx + 1));
            parts.push(li);
        }
        parts.push(buf("</ol></section>"));
    }

    let used: Vec<JsNumber> = ctx.used_comments.iter().filter(|&id| ctx.comments.contains_key(&NumberKey::from(id))).collect();
    if !used.is_empty() {
        parts.push(buf("<section class=\"uw-comments\"><h2>Comentários</h2><ol>"));
        for id in used {
            let Some(c) = ctx.comments.get(&NumberKey::from(id)) else { continue };
            let mut li = U16Buf::new();
            li.push_str(&format!("<li id=\"cmt-{}\"><span class=\"uw-author\">", to_js_string_null(id)));
            escape_html_str_into(&mut li, c.author.as_deref().unwrap_or("Anônimo"));
            li.push_str("</span>: ");
            escape_html_into(&mut li, c.text.units());
            li.push_str("</li>");
            parts.push(li);
        }
        parts.push(buf("</ol></section>"));
    }

    let mut out = U16Buf::new();
    for (i, p) in parts.iter().enumerate() {
        if i > 0 {
            out.push_unit(0x0a);
        }
        out.push_buf(p);
    }
    out
}

/// `${id}` of a number in a template literal (`null` is `"null"`).
fn to_js_string_null(n: JsNumber) -> String {
    match n {
        JsNumber::Null => "null".to_owned(),
        JsNumber::Num(x) => to_js_string(x),
    }
}

fn buf(s: &str) -> U16Buf {
    let mut b = U16Buf::new();
    b.push_str(s);
    b
}

/// `toHtml(doc, opts)` in UTF-16 code units (lone surrogates of the text kept).
#[must_use]
pub fn to_html_units(doc: &ExportDoc<'_>, opts: &HtmlOptions<'_>) -> Vec<u16> {
    let fin = FinalDoc::new(doc);
    let mut ctx = Ctx {
        fin: &fin,
        comments: comments_by_id(doc.comments),
        used_comments: OrderedNumbers::default(),
        notes: Vec::new(),
        images_omit: opts.images == Some(ImagesMode::Omit),
    };
    let body = body_html(&fin, &mut ctx);
    if opts.full_document == Some(false) {
        return body.into_units();
    }
    let mut out = U16Buf::new();
    out.push_str("<!DOCTYPE html>\n<html lang=\"pt-BR\">\n<head>\n<meta charset=\"utf-8\">\n<meta name=\"viewport\" content=\"width=device-width, initial-scale=1\">\n<title>");
    escape_html_str_into(&mut out, opts.title.or(doc.title).unwrap_or("Documento"));
    out.push_str("</title>\n<style>");
    out.push_str(STYLE);
    out.push_str("</style>\n</head>\n<body>\n");
    out.push_buf(&body);
    out.push_str("\n</body>\n</html>");
    out.into_units()
}

/// `toHtml(doc, opts)` as UTF-8 (a lone surrogate of the text as U+FFFD, as `TextEncoder`).
#[must_use]
pub fn to_html(doc: &ExportDoc<'_>, opts: &HtmlOptions<'_>) -> String {
    String::from_utf16_lossy(&to_html_units(doc, opts))
}
