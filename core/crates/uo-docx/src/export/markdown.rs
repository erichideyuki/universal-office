//! Markdown export (docx-core `export/markdown.ts`): GitHub Flavored Markdown with headings,
//! emphasis, nested lists, links, images (data URI), GFM tables, and the notes and comments as
//! footnotes (`[^…]`). Built in UTF-16 code units like [`super::html`].
//!
//! Where the TS throws, this returns [`ExportError`]: `'  '.repeat(ilvl)` throws `RangeError` for a
//! negative or infinite level (and past the string length limit of V8, here a limit of
//! [`MAX_LIST_LEVEL`] levels, declared in the record).

use std::collections::HashMap;

use uo_jsnum::format::to_js_string;
use uo_jsnum::math;

use crate::final_view::final_paragraphs;
use crate::js::JsNumber;
use crate::model::run::RunProps;
use crate::model::{Cell, Comment, Paragraph, Run, Table};
use crate::u16buf::{U16Buf, trim_units};

use super::html::ImagesMode;
use super::matchers::{cell_text_escape, collapse_blank_lines, escape_md_into, lead_space_len, trail_space_start};
use super::model::{ExportDoc, FinalDoc, NumberKey, OrderedBlock, OrderedNumbers, comments_by_id, heading_level, resolve_image_uri};
use super::{ExportError, MAX_LIST_LEVEL, MAX_OUTPUT_UNITS};

/// `MarkdownOptions`.
#[derive(Debug, Clone, Default)]
pub struct MarkdownOptions<'a> {
    pub title: Option<&'a str>,
    pub images: Option<ImagesMode>,
}

struct Ctx<'a, 'd> {
    fin: &'d FinalDoc<'a>,
    comments: HashMap<NumberKey, &'d Comment>,
    note_defs: Vec<U16Buf>,
    note_seq: usize,
    used_comments: OrderedNumbers,
    images_omit: bool,
}

/// The lines of the output and their total length, against [`MAX_OUTPUT_UNITS`].
#[derive(Default)]
struct Lines {
    v: Vec<U16Buf>,
    total: usize,
}

impl Lines {
    fn push(&mut self, b: U16Buf) -> Result<(), ExportError> {
        self.total = self.total.saturating_add(b.len()).saturating_add(1);
        if self.total > MAX_OUTPUT_UNITS {
            return Err(ExportError::OutputLimit);
        }
        self.v.push(b);
        Ok(())
    }
}

fn flag(b: Option<bool>) -> bool {
    b.unwrap_or(false)
}

fn emphasize(t: &[u16], p: &RunProps) -> U16Buf {
    let lead = lead_space_len(t);
    let trail = trail_space_start(t);
    let mut out = U16Buf::new();
    let core = if trail > lead { t.get(lead..trail).unwrap_or(&[]) } else { &[] };
    if core.is_empty() {
        out.push_units(t);
        return out;
    }
    let (bold, italic) = (flag(p.bold), flag(p.italic));
    let mut pre = String::new();
    let mut post = String::new();
    if bold && italic {
        pre.push_str("***");
        post.push_str("***");
    } else if bold {
        pre.push_str("**");
        post.push_str("**");
    } else if italic {
        pre.push('*');
        post.push('*');
    }
    if flag(p.strike) || flag(p.double_strike) {
        pre.insert_str(0, "~~");
        post.push_str("~~");
    }
    if flag(p.underline) {
        pre.insert_str(0, "<u>");
        post.push_str("</u>");
    }
    match p.vert_align.as_deref() {
        Some("superscript") => {
            pre.insert_str(0, "<sup>");
            post.push_str("</sup>");
        }
        Some("subscript") => {
            pre.insert_str(0, "<sub>");
            post.push_str("</sub>");
        }
        _ => {}
    }
    out.push_units(t.get(..lead).unwrap_or(&[]));
    out.push_str(&pre);
    out.push_units(core);
    out.push_str(&post);
    out.push_units(t.get(trail..).unwrap_or(&[]));
    out
}

fn run_md(run: &Run, ctx: &mut Ctx<'_, '_>) -> U16Buf {
    let mut out = U16Buf::new();
    let ext = run.ext();
    if let Some(img) = ext.and_then(|e| e.image.as_deref()) {
        if ctx.images_omit {
            out.push_str("[imagem]");
        } else {
            out.push_str("![](");
            out.push_str(&resolve_image_uri(img, &ctx.fin.doc));
            out.push_str(")");
        }
        return out;
    }
    if let Some(nr) = ext.and_then(|e| e.note_ref.as_ref()) {
        ctx.note_seq += 1;
        let seq = ctx.note_seq;
        let mut txt = U16Buf::new();
        if let Some(paras) = ctx.fin.note_paragraphs(&*nr.kind == "footnote", nr.id) {
            for (i, np) in paras.paras.iter().enumerate() {
                if i > 0 {
                    txt.push_unit(0x20);
                }
                for r in &np.runs {
                    txt.push_units(r.text.units());
                }
            }
        }
        let mut def = U16Buf::new();
        def.push_str(&format!("[^n{seq}]: "));
        def.push_units(trim_units(txt.units()));
        ctx.note_defs.push(def);
        out.push_str(&format!("[^n{seq}]"));
        return out;
    }
    let mut escaped = Vec::with_capacity(run.text.len());
    escape_md_into(&mut escaped, run.text.units());
    let mut t = emphasize(&escaped, &run.props);
    if let Some(link) = run.props.link.as_deref().filter(|l| !l.is_empty()) {
        let mut linked = U16Buf::new();
        linked.push_str("[");
        linked.push_buf(&t);
        linked.push_str("](");
        linked.push_str(link);
        linked.push_str(")");
        t = linked;
    }
    t
}

fn inline_runs(runs: &[Run], ctx: &mut Ctx<'_, '_>) -> U16Buf {
    let mut s = U16Buf::new();
    let mut ids = OrderedNumbers::default();
    for r in runs {
        let m = run_md(r, ctx);
        s.push_buf(&m);
        for &id in r.props.comment_ids.iter().flatten() {
            ids.add(id);
        }
    }
    for id in ids.iter() {
        if ctx.comments.contains_key(&NumberKey::from(id)) {
            ctx.used_comments.add(id);
            s.push_str(&format!("[^c{}]", number_text(id)));
        }
    }
    s
}

/// `${id}` of a number in a template literal (`null` is `"null"`).
fn number_text(n: JsNumber) -> String {
    match n {
        JsNumber::Null => "null".to_owned(),
        JsNumber::Num(x) => to_js_string(x),
    }
}

fn cell_text(cell: &Cell, ctx: &mut Ctx<'_, '_>, finalize: bool) -> Vec<u16> {
    let mut inner = U16Buf::new();
    let emit = |p: &Paragraph, first: bool, inner: &mut U16Buf, ctx: &mut Ctx<'_, '_>| {
        if !first {
            inner.push_str("<br>");
        }
        let t = inline_runs(&p.runs, ctx);
        inner.push_buf(&t);
    };
    if finalize {
        let fp = final_paragraphs(cell.content.paras.iter());
        for (i, p) in fp.paras.iter().enumerate() {
            emit(p, i == 0, &mut inner, ctx);
        }
    } else {
        for (i, p) in cell.content.paras.iter().enumerate() {
            emit(p, i == 0, &mut inner, ctx);
        }
    }
    cell_text_escape(inner.units())
}

fn table_md(t: &Table, ctx: &mut Ctx<'_, '_>) -> U16Buf {
    let rows: Vec<Vec<Vec<u16>>> = t.rows.iter().map(|r| r.cells.iter().map(|c| cell_text(c, ctx, true)).collect()).collect();
    let mut md = U16Buf::new();
    let Some(first) = rows.first() else { return md };
    let cols = rows.iter().map(Vec::len).max().unwrap_or(0);
    let pad = |r: &[Vec<u16>]| -> Vec<Vec<u16>> {
        let mut v = r.to_vec();
        while v.len() < cols {
            v.push(Vec::new());
        }
        v
    };
    let line = |cells: &[Vec<u16>], md: &mut U16Buf| {
        md.push_str("| ");
        for (i, c) in cells.iter().enumerate() {
            if i > 0 {
                md.push_str(" | ");
            }
            md.push_units(c);
        }
        md.push_str(" |\n");
    };
    let header = pad(first);
    line(&header, &mut md);
    md.push_str("| ");
    for i in 0..header.len() {
        if i > 0 {
            md.push_str(" | ");
        }
        md.push_str("---");
    }
    md.push_str(" |\n");
    for r in rows.iter().skip(1) {
        line(&pad(r), &mut md);
    }
    md
}

/// `'  '.repeat(ilvl)`: the count `ToIntegerOrInfinity` makes of the level, or the error the TS
/// throws (`RangeError`) or this port refuses (past [`MAX_LIST_LEVEL`]).
fn indent_count(ilvl: JsNumber) -> Result<usize, ExportError> {
    let x = ilvl.to_number();
    if x.is_nan() {
        return Ok(0);
    }
    let t = math::trunc(x);
    if t.is_infinite() || t < 0.0 {
        return Err(ExportError::InvalidListLevel);
    }
    if t > f64::from(MAX_LIST_LEVEL) {
        return Err(ExportError::ListLevelLimit);
    }
    Ok(uo_jsnum::conv::to_uint32(t) as usize)
}

/// `toMarkdown(doc, opts)` in UTF-16 code units (lone surrogates of the text kept).
///
/// # Errors
/// [`ExportError::InvalidListLevel`] where the TS throws, [`ExportError::ListLevelLimit`] past the
/// declared limit of list levels.
pub fn to_markdown_units(doc: &ExportDoc<'_>, opts: &MarkdownOptions<'_>) -> Result<Vec<u16>, ExportError> {
    let fin = FinalDoc::new(doc);
    let mut ctx = Ctx {
        fin: &fin,
        comments: comments_by_id(doc.comments),
        note_defs: Vec::new(),
        note_seq: 0,
        used_comments: OrderedNumbers::default(),
        images_omit: opts.images == Some(ImagesMode::Omit),
    };
    let mut out = Lines::default();
    if let Some(title) = opts.title.or(doc.title).filter(|t| !t.is_empty()) {
        let mut b = U16Buf::new();
        b.push_str(&format!("# {title}\n"));
        out.push(b)?;
    }

    let mut in_list = false;
    for b in fin.ordered_blocks() {
        let is_list = matches!(b, OrderedBlock::Para(p) if p.props.numbering.is_some());
        if in_list && !is_list {
            out.push(U16Buf::new())?;
        }
        in_list = is_list;
        let p = match b {
            OrderedBlock::Table(t) => {
                out.push(table_md(t, &mut ctx))?;
                continue;
            }
            OrderedBlock::Para(p) => p,
        };
        let inner = inline_runs(&p.runs, &mut ctx);
        if let Some(num) = &p.props.numbering {
            let n = indent_count(num.ilvl)?;
            let mut line = U16Buf::new();
            for _ in 0..n {
                line.push_str("  ");
            }
            line.push_str(if &*num.kind == "decimal" { "1." } else { "-" });
            line.push_str(" ");
            line.push_buf(&inner);
            out.push(line)?;
            continue;
        }
        let lvl = heading_level(p);
        let mut line = U16Buf::new();
        if lvl > 0 {
            line.push_str(&"#".repeat(lvl as usize));
            line.push_str(" ");
            line.push_buf(&inner);
            line.push_str("\n");
            out.push(line)?;
        } else if !trim_units(inner.units()).is_empty() {
            line.push_buf(&inner);
            line.push_str("\n");
            out.push(line)?;
        } else {
            out.push(U16Buf::new())?;
        }
    }

    if !ctx.note_defs.is_empty() {
        out.push(U16Buf::new())?;
        for d in &ctx.note_defs {
            out.push(d.clone())?;
        }
    }

    let used: Vec<JsNumber> = ctx.used_comments.iter().filter(|&id| ctx.comments.contains_key(&NumberKey::from(id))).collect();
    if !used.is_empty() {
        out.push(U16Buf::new())?;
        for id in used {
            let Some(c) = ctx.comments.get(&NumberKey::from(id)) else { continue };
            let mut line = U16Buf::new();
            line.push_str(&format!("[^c{}]: **", number_text(id)));
            line.push_str(c.author.as_deref().unwrap_or("Anônimo"));
            line.push_str("**: ");
            line.push_units(c.text.units());
            out.push(line)?;
        }
    }

    let mut joined = Vec::new();
    for (i, l) in out.v.iter().enumerate() {
        if i > 0 {
            joined.push(0x0a);
        }
        joined.extend_from_slice(l.units());
    }
    let collapsed = collapse_blank_lines(&joined);
    let mut res = trim_units(&collapsed).to_vec();
    res.push(0x0a);
    Ok(res)
}

/// `toMarkdown(doc, opts)` as UTF-8 (a lone surrogate of the text as U+FFFD).
///
/// # Errors
/// As [`to_markdown_units`].
pub fn to_markdown(doc: &ExportDoc<'_>, opts: &MarkdownOptions<'_>) -> Result<String, ExportError> {
    Ok(String::from_utf16_lossy(&to_markdown_units(doc, opts)?))
}
