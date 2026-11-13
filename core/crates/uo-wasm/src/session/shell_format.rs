//! Shell exports of the wave-7 frontier A1 of the editor switch (record `core/docs/modules/uo-bindings.md` "Wave 7
//! — A1: File, View, Font and Paragraph"): what the TS shell needs over a session for the File, View, Font and
//! Paragraph families that wave 6 left disabled. Same conventions as `shell.rs` (`UoBuf` statuses 0 ok, 1 none,
//! 5 bad argument, 11 null handle); the answers are JSON text (UTF-8) in the TS shapes, the commands an
//! `ExecResult` (the native-only commands of uo-docx: `insertCustomInline`, `ensureBuiltinStyle`, the plain
//! `replaceAll`), the exports bytes (ODT) or UTF-16 units (HTML, Markdown).
//!
//! Arguments: `(ptr, len)` from `uo_alloc_bytes`, consumed; a list of strings is `u32 count` then, per string,
//! `u32 units` and the UTF-16LE units (little endian).

use std::cell::Cell;
use std::collections::HashMap;
use std::rc::Rc;

use uo_docx::ids::DrawingId;
use uo_docx::js::{JsValue, JsonWriter};
use uo_docx::model::Document;
use uo_docx::model::para::Paragraph;
use uo_docx::model::run::Run;
use uo_docx::model::story::{Block, Story};
use uo_docx::model::table::Table;
use uo_docx::session::ops::find::FindCmd;
use uo_docx::session::ops::paragraph::{ParaBorderSide, SpacingArg};
use uo_docx::session::ops::styles::StyleCmd;
use uo_docx::session::ops::text::TextCmd;
use uo_docx::session::{Command, Session};
use uo_docx::text::Utf16;
use uo_jsnum::json::{push_number, push_string};

use super::env::HostEnv;
use super::handle::{BAD_ARGUMENT, HostFonts, UoWriteSession, decode_open, exec_result, open_failed, outcome};
use crate::pipeline::{self, code};
use crate::render::{NONE, NULL_HANDLE, OK, UoBuf, buf, take_bytes};
use crate::write::{UoWriteEnv, env_parts};

// ───────────────────────────── arguments ─────────────────────────────

/// The strings of an argument buffer (see the module comment); `None` when it does not parse.
fn strings(b: &[u8]) -> Option<Vec<Vec<u16>>> {
    let u32_at = |i: usize| -> Option<u32> { Some(u32::from_le_bytes(b.get(i..i + 4)?.try_into().ok()?)) };
    let n = u32_at(0)? as usize;
    let mut at = 4usize;
    let mut out = Vec::with_capacity(n.min(64));
    for _ in 0..n {
        let len = u32_at(at)? as usize;
        at += 4;
        let bytes = b.get(at..at.checked_add(len.checked_mul(2)?)?)?;
        out.push(bytes.as_chunks::<2>().0.iter().map(|c| u16::from_le_bytes(*c)).collect());
        at += len * 2;
    }
    Some(out)
}

fn text(u: &[u16]) -> String {
    String::from_utf16_lossy(u)
}

fn json_of(v: &dyn JsValue) -> String {
    JsonWriter::stringify(v, None).unwrap_or_else(|| "null".to_owned())
}

fn bool_json(out: &mut String, b: Option<bool>) {
    out.push_str(match b {
        Some(true) => "true",
        Some(false) => "false",
        None => "null",
    });
}

fn num_or_null(out: &mut String, x: Option<f64>) {
    match x {
        Some(v) => push_number(out, v),
        None => out.push_str("null"),
    }
}

fn units_buf(units: &[u16]) -> *mut UoBuf {
    let mut out = Vec::with_capacity(units.len() * 2);
    for u in units {
        out.extend_from_slice(&u.to_le_bytes());
    }
    buf(OK, out)
}

fn side_key(s: ParaBorderSide) -> &'static str {
    match s {
        ParaBorderSide::Top => "top",
        ParaBorderSide::Left => "left",
        ParaBorderSide::Bottom => "bottom",
        ParaBorderSide::Right => "right",
        ParaBorderSide::Between => "between",
        ParaBorderSide::Bar => "bar",
    }
}

const FLOW_KEYS: [&str; 6] = ["widowControl", "keepNext", "keepLines", "pageBreakBefore", "suppressLineNumbers", "suppressAutoHyphens"];

/// `documentThemeColors()` (universal-write.ts 19990): the theme's colours in its key order, or the Office 2023
/// palette when the document has none.
fn theme_colors(s: &Session) -> String {
    let doc = s.document();
    let c = &doc.formatting.theme.colors;
    match c {
        c if !c.is_empty() => json_of(c),
        _ => "{\"dk1\":\"000000\",\"lt1\":\"FFFFFF\",\"dk2\":\"0E2841\",\"lt2\":\"E8E8E8\",\"accent1\":\"156082\",\"accent2\":\"E97132\",\"accent3\":\"196B24\",\"accent4\":\"0F9ED5\",\"accent5\":\"A02B93\",\"accent6\":\"4EA72E\",\"hlink\":\"467886\",\"folHlink\":\"96607D\"}".to_owned(),
    }
}

/// The JSON answers of `uo_shell_fmt_json` (see there).
fn answer(s: &Session, what: u32, args: &[Vec<u16>]) -> Option<String> {
    let arg = |i: usize| args.get(i).map(|u| text(u));
    let mut out = String::with_capacity(128);
    match what {
        // getParagraphFlow()
        1 => {
            let Some(f) = s.paragraph_flow() else { return Some("null".to_owned()) };
            out.push('{');
            for (i, (k, v)) in FLOW_KEYS.iter().zip(f.flags).enumerate() {
                if i > 0 {
                    out.push(',');
                }
                push_string(&mut out, k);
                out.push(':');
                bool_json(&mut out, v);
            }
            out.push_str(",\"textboxTightWrap\":");
            match &f.textbox_tight_wrap {
                Some(t) => push_string(&mut out, t),
                None => out.push_str("null"),
            }
            out.push('}');
        }
        // getParagraphLayout()
        2 => {
            let Some(l) = s.paragraph_layout() else { return Some("null".to_owned()) };
            let sp = |out: &mut String, v: Option<SpacingArg>| match v {
                Some(SpacingArg::Auto) => out.push_str("\"auto\""),
                Some(SpacingArg::Twips(t)) => push_number(out, t),
                None => out.push_str("null"),
            };
            out.push_str("{\"left\":");
            num_or_null(&mut out, l.left);
            out.push_str(",\"right\":");
            num_or_null(&mut out, l.right);
            out.push_str(",\"before\":");
            sp(&mut out, l.before);
            out.push_str(",\"after\":");
            sp(&mut out, l.after);
            out.push('}');
        }
        // getParagraphBorders(): `{ val: style, sz: sizeEighthPt, space: spacePt, color: color ?? 'auto' }`
        3 => {
            out.push('{');
            for (i, (side, e)) in s.paragraph_borders().into_iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                push_string(&mut out, side_key(side));
                out.push_str(":{\"val\":");
                push_string(&mut out, &e.style);
                out.push_str(",\"sz\":");
                out.push_str(&json_of(&e.size_eighth_pt));
                out.push_str(",\"space\":");
                out.push_str(&json_of(&e.space_pt));
                out.push_str(",\"color\":");
                push_string(&mut out, e.color.as_deref().unwrap_or("auto"));
                out.push('}');
            }
            out.push('}');
        }
        // getDefaultTabStop()
        4 => push_number(&mut out, s.default_tab_stop_now()),
        // documentThemeColors()
        5 => out = theme_colors(s),
        // hasExternalFields()
        6 => out.push_str(if s.has_external_fields() { "true" } else { "false" }),
        // getLinks(): `documentLinks(bodyBlocks())`
        7 => {
            let links = uo_docx::read::inspect::document_links(&s.document().body);
            out.push('[');
            for (i, l) in links.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                out.push_str(&json_of(l));
            }
            out.push(']');
        }
        // getCustomInlines(type?): the argument present = the type
        8 => {
            let t = arg(0);
            let items = uo_docx::read::inspect::document_custom_inlines(&s.document().body, t.as_deref());
            out.push('[');
            for (i, c) in items.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                out.push_str(&json_of(c));
            }
            out.push(']');
        }
        // styleKeyOf(id)
        9 => push_string(&mut out, &s.style_key_for(&arg(0)?)),
        // the id ensureBuiltinStyle(name) answers
        10 => push_string(&mut out, &s.builtin_style_id_of(&arg(0)?)),
        // the drawing grid of section `args[0]` on a page of `args[1]` × `args[2]` twips (paintDocGrid 18607):
        // `{ box: [x0, y0, x1, y1], xs, ys }`, null without the section
        11 => {
            let n = |i: usize| arg(i).map(|t| uo_jsnum::parse::to_number(&t)).filter(|v| v.is_finite());
            let (sec, pw, ph) = (n(0)?, n(1)?, n(2)?);
            let doc = s.document();
            let sp = doc.sections.get(sec as usize).map(|x| &*x.sect_pr)?;
            let settings = s.package().part_text("word/settings.xml").ok().flatten();
            let g = uo_docx::session::ops::navigation::drawing_grid_of(settings.as_deref());
            let num = |v: uo_docx::js::JsNumber| v.to_number();
            let bx = uo_docx::session::ops::navigation::GridBox {
                x0: num(sp.margin_left_twips),
                y0: num(sp.margin_top_twips),
                x1: pw - num(sp.margin_right_twips),
                y1: ph - num(sp.margin_bottom_twips),
            };
            let origin = if g.from_margins { (bx.x0, bx.y0) } else { (g.origin_x, g.origin_y) };
            let (xs, ys) = uo_docx::session::ops::navigation::grid_lines(&g, bx, origin);
            out.push_str("{\"box\":[");
            for (i, v) in [bx.x0, bx.y0, bx.x1, bx.y1].into_iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                push_number(&mut out, v);
            }
            for (name, list) in [("xs", &xs), ("ys", &ys)] {
                out.push_str(",\"");
                out.push_str(name);
                out.push_str("\":[");
                for (i, v) in list.iter().enumerate() {
                    if i > 0 {
                        out.push(',');
                    }
                    push_number(&mut out, *v);
                }
                out.push(']');
            }
            out.push('}');
        }
        // section `args[0]` (`sectionOfPage(pg)`, the ruler and the marks): `{ sectPr, sect }` as their JSON
        12 => {
            let i = arg(0)?.parse::<usize>().ok()?;
            let sec = s.document().sections.get(i)?;
            out.push_str("{\"sectPr\":");
            out.push_str(&json_of(&*sec.sect_pr));
            out.push_str(",\"sect\":");
            match &sec.sect {
                Some(x) => out.push_str(&json_of(&**x)),
                None => out.push_str("null"),
            }
            out.push('}');
        }
        // the bookmark marks of paragraph `args[0]` of the edit story (paintBookmarkMarks 6306): `[{ at, end, id, hidden }]` in
        // run order, `at` the run's offset
        13 => {
            let pi = arg(0)?.parse::<usize>().ok()?;
            let p = s.paras()?.get(pi)?;
            let mut acc = 0usize;
            out.push('[');
            let mut first = true;
            for r in &p.runs {
                let at = acc;
                acc += r.text.len();
                let Some(b) = r.ext().and_then(|e| e.bookmark.as_ref()) else { continue };
                if !first {
                    out.push(',');
                }
                first = false;
                out.push_str("{\"at\":");
                push_number(&mut out, at as f64);
                out.push_str(if b.end == Some(true) { ",\"end\":true" } else { ",\"end\":false" });
                out.push_str(",\"id\":");
                out.push_str(&json_of(&b.id));
                out.push_str(if uo_docx::session::ops::bookmarks::is_hidden_bookmark(&b.name) {
                    ",\"hidden\":true}"
                } else {
                    ",\"hidden\":false}"
                });
            }
            out.push(']');
        }
        // `tocSdtRange(para)`: `[first, last]` or null
        14 => {
            let pi = arg(0)?.parse::<usize>().ok()?;
            match s.toc_sdt_range(pi) {
                Some((a, b)) => {
                    out.push('[');
                    push_number(&mut out, a as f64);
                    out.push(',');
                    push_number(&mut out, b as f64);
                    out.push(']');
                }
                None => out.push_str("null"),
            }
        }
        // the plugin inlines of paragraph `args[0]` of the edit story (paintEngineInlines 17085): `[{ at, custom, props }]`, `custom`
        // the run's `CustomInline` and `props` its `RunProps` (their JSON)
        15 => {
            let pi = arg(0)?.parse::<usize>().ok()?;
            let p = s.paras()?.get(pi)?;
            let mut acc = 0usize;
            let mut first = true;
            out.push('[');
            for r in &p.runs {
                let at = acc;
                acc += r.text.len();
                let Some(c) = r.ext().and_then(|e| e.custom.as_deref()) else { continue };
                if !first {
                    out.push(',');
                }
                first = false;
                out.push_str("{\"at\":");
                push_number(&mut out, at as f64);
                out.push_str(",\"custom\":");
                out.push_str(&json_of(c));
                out.push_str(",\"props\":");
                out.push_str(&json_of(&*r.props));
                out.push('}');
            }
            out.push(']');
        }
        // the plain text of every paragraph `stats()` counts (universal-write.ts 8919): the edit story, but the body while a note or a
        // text box is edited (the TS `saved.model`); `plainTextSlice`: note references and note marks out, a plugin inline as its
        // alternative text. A JSON array of strings
        16 => {
            use uo_docx::session::ops::context::EditContext;
            let paras = match s.context() {
                EditContext::Note { .. } | EditContext::Textbox { .. } => Some(&s.document().body.paras),
                _ => s.paras(),
            }?;
            out.push('[');
            for (i, p) in paras.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                let mut t: Vec<u16> = Vec::new();
                for run in &p.runs {
                    let ext = run.ext();
                    if ext.is_some_and(|x| x.note_ref.is_some())
                        || ext.and_then(|x| x.field.as_deref()).is_some_and(uo_docx::session::settle::is_note_mark_field)
                    {
                        continue;
                    }
                    match ext.and_then(|x| x.custom.as_deref()) {
                        Some(c) => t.extend_from_slice(c.fallback_text.units()),
                        None => t.extend_from_slice(run.text.units()),
                    }
                }
                push_string(&mut out, &String::from_utf16_lossy(&t));
            }
            out.push(']');
        }
        // the body paragraphs that end a section (`props.sectionBreak`): the TS `sectionStarts` (section k starts after the
        // k-th), for `sectionIndexForPara` (the formatting marks)
        17 => {
            out.push('[');
            let mut first = true;
            for (i, p) in s.document().body.paras.iter().enumerate() {
                if p.props.section_break.is_none() {
                    continue;
                }
                if !first {
                    out.push(',');
                }
                first = false;
                push_number(&mut out, i as f64);
            }
            out.push(']');
        }
        // `loadedFormat` (universal-write.ts 2129): `"odt"` after `loadOdt`, else `"docx"`; and whether `listStyles` takes its `.odt`
        // branch (4156): `{ format, odtStyles }`
        18 => {
            out.push_str("{\"format\":");
            push_string(&mut out, s.loaded_format());
            out.push_str(if s.odt_style_list() { ",\"odtStyles\":true}" } else { ",\"odtStyles\":false}" });
        }
        _ => return None,
    }
    Some(out)
}

/// A JSON answer for the shell over session `h` (UTF-8): `what` 1 `getParagraphFlow()`, 2 `getParagraphLayout()`,
/// 3 `getParagraphBorders()`, 4 `getDefaultTabStop()`, 5 `documentThemeColors()`, 6 `hasExternalFields()`, 7
/// `getLinks()`, 8 `getCustomInlines(type?)`, 9 `styleKeyOf(id)`, 10 the id of `ensureBuiltinStyle(name)`, 11 the
/// drawing grid of a page, 12 a section's `sectPr` (13–17: see `answer`), 18 the loaded format (fix round 2). Status 1 for an
/// unknown `what` or missing arguments.
///
/// # Safety
/// `h` null or live; `(ptr, len)` a buffer of `uo_alloc_bytes` (or `0, 0`).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn uo_shell_fmt_json(h: *const UoWriteSession, what: u32, ptr: *mut u8, len: usize) -> *mut UoBuf {
    // SAFETY: the caller guarantees the buffer; it is consumed here.
    let bytes = if len == 0 { Vec::new() } else { unsafe { take_bytes(ptr, len) } };
    // SAFETY: the caller guarantees `h` is null or live.
    let Some(s) = (unsafe { h.as_ref() }) else { return buf(NULL_HANDLE, Vec::new()) };
    let args = if bytes.is_empty() { Vec::new() } else { strings(&bytes).unwrap_or_default() };
    match answer(&s.session, what, &args) {
        Some(j) => buf(OK, j.into_bytes()),
        None => buf(NONE, Vec::new()),
    }
}

/// A native-only command of the session (an `ExecResult`): `what` 1 `insertCustomInline` (`type`, `JSON.stringify(data)`,
/// `fallbackText`), 2 `ensureBuiltinStyle(name)`, 3 `replaceAll(needle, replacement)` (the count is the
/// `NavigationResult` query's `Count`).
///
/// # Safety
/// As [`uo_shell_fmt_json`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn uo_shell_fmt_exec(h: *mut UoWriteSession, what: u32, ptr: *mut u8, len: usize) -> *mut UoBuf {
    // SAFETY: the caller guarantees the buffer; it is consumed here.
    let bytes = if len == 0 { Vec::new() } else { unsafe { take_bytes(ptr, len) } };
    // SAFETY: the caller guarantees `h` is null or live.
    let Some(s) = (unsafe { h.as_mut() }) else { return buf(NULL_HANDLE, Vec::new()) };
    let Some(args) = strings(&bytes) else { return buf(BAD_ARGUMENT, b"arguments".to_vec()) };
    let a = |i: usize| args.get(i).cloned().unwrap_or_default();
    let cmd = match what {
        1 => Command::Text(TextCmd::InsertCustomInline {
            r#type: text(&a(0)),
            data_json: text(&a(1)),
            fallback_text: Utf16::from_units(a(2)),
        }),
        2 => Command::Style(StyleCmd::EnsureBuiltinStyle { name: text(&a(0)) }),
        3 => Command::Find(FindCmd::ReplaceAllPlain { needle: a(0), replacement: a(1) }),
        _ => return buf(BAD_ARGUMENT, b"what".to_vec()),
    };
    exec_result(outcome(s.command(cmd)))
}

/// The hyphenation patterns of environment `env` for the Manual Hyphenation (`nextManualHyphenation` reads the
/// session's registry, F10-D3): the TS `loadHyphenation(langs)` before the forced layout. Null `env` drops them.
///
/// # Safety
/// `h`, `env` null or live.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn uo_shell_fmt_hyphenation(h: *mut UoWriteSession, env: *const UoWriteEnv) -> i32 {
    // SAFETY: the caller guarantees the handles are null or live.
    let Some(s) = (unsafe { h.as_mut() }) else { return NULL_HANDLE };
    // SAFETY: as above.
    let reg = unsafe { env.as_ref() }.map(|e| std::sync::Arc::clone(env_parts(e).1));
    s.session.set_hyphenation_registry(reg);
    OK
}

// ───────────────────────────── export ─────────────────────────────

/// `withPluginFallback(paragraph)` over every top-level paragraph of `story` (universal-write.ts 8711): a NEW paragraph
/// `{ props, runs }` (the direct `pPr` and the rest dropped, as the TS literal), a plugin inline becoming
/// `{ text: fallbackText, props }`.
fn with_plugin_fallback(story: &Story) -> Story {
    let mut out = story.clone();
    for i in 0..out.paras.len() {
        if let Some(p) = out.paras.make_mut(i) {
            let runs: Vec<Run> = p
                .runs
                .iter()
                .map(|r| match r.ext().and_then(|e| e.custom.as_deref()) {
                    Some(c) => Run { text: c.fallback_text.clone(), props: std::sync::Arc::clone(&r.props), ..Run::default() },
                    None => r.clone(),
                })
                .collect();
            *p = Paragraph { id: p.id, runs, props: std::sync::Arc::clone(&p.props), ..Paragraph::default() };
        }
    }
    out
}

fn table_images(t: &Table, out: &mut Vec<(DrawingId, String, Vec<u8>)>, doc: &Document) {
    for row in &t.rows {
        for c in &row.cells {
            story_images(&c.content, out, doc);
        }
    }
}

fn story_images(story: &Story, out: &mut Vec<(DrawingId, String, Vec<u8>)>, doc: &Document) {
    for b in story.blocks() {
        match b {
            Block::Paragraph(p) => {
                for r in &p.runs {
                    if let Some(img) = r.ext().and_then(|e| e.image.as_deref()) {
                        out.push((img.drawing, img.ext.to_string(), doc.media.get(img.data.0).unwrap_or(&[]).to_vec()));
                    }
                }
            }
            Block::Table(t) => table_images(t, out, doc),
        }
    }
}

/// `collectImages()` of the export (22886): the run images of the body, its tables (recursively) and the
/// header/footer parts, each with its drawing id (the key of `imageDataUris`).
fn collect_images(doc: &Document) -> Vec<(DrawingId, String, Vec<u8>)> {
    let mut out = Vec::new();
    story_images(&doc.body, &mut out, doc);
    for part in doc.hf.iter() {
        story_images(&part.story, &mut out, doc);
    }
    out
}

/// `exportOdt(opts)` (8814): the body (with the plugin fallback) as ONE section with the body's `sectPr` and no
/// headers/footers, the notes, comments, styles and bookmarks of the document; nothing else of it (the TS
/// `FullDocument` literal has no formatting, background or properties).
fn export_odt(s: &Session, omit_images: bool) -> Result<Vec<u8>, String> {
    let doc = s.document();
    let body = with_plugin_fallback(&doc.body);
    let blocks = body.block_count();
    let sect_pr = doc.sections.last().map(|x| std::sync::Arc::clone(&x.sect_pr)).unwrap_or_default();
    let section = uo_docx::model::section::Section {
        sect_pr,
        block_start: uo_docx::js::JsNumber::Num(0.0),
        block_end: uo_docx::js::JsNumber::Num(blocks as f64),
        ..uo_docx::model::section::Section::default()
    };
    let fd = Document {
        body,
        sections: std::sync::Arc::from(vec![section]),
        notes: std::sync::Arc::clone(&doc.notes),
        comments: std::sync::Arc::clone(&doc.comments),
        styles: std::sync::Arc::clone(&doc.styles),
        bookmarks: std::sync::Arc::clone(&doc.bookmarks),
        media: doc.media.clone(),
        ..Document::default()
    };
    let images = if omit_images { uo_docx::odf::write_odt::OdtImages::Omit } else { uo_docx::odf::write_odt::OdtImages::Embed };
    uo_docx::odf::write_odt::write_odt(&fd, &uo_docx::odf::write_odt::OdtOptions { images }).map_err(|e| format!("{e:?}"))
}

/// `exportHtml`/`exportMarkdown` (8798, 8804) over `buildExportDoc` (8698): the body with the plugin fallback, the
/// comments and notes of the document, the data URIs the shell compressed (by drawing id).
fn export_text(
    s: &Session,
    markdown: bool,
    title: Option<&str>,
    omit: bool,
    uris: &HashMap<DrawingId, Box<str>>,
) -> Result<Vec<u16>, String> {
    let doc = s.document();
    let body = with_plugin_fallback(&doc.body);
    let ed = uo_docx::export::ExportDoc {
        title,
        body: &body,
        comments: &doc.comments,
        footnotes: Some(&doc.notes.footnotes),
        endnotes: Some(&doc.notes.endnotes),
        media: &doc.media,
        image_data_uris: if omit { None } else { Some(uris) },
    };
    let images = if omit { uo_docx::export::ImagesMode::Omit } else { uo_docx::export::ImagesMode::Data };
    if markdown {
        uo_docx::export::to_markdown_units(&ed, &uo_docx::export::MarkdownOptions { title, images: Some(images) })
            .map_err(|e| format!("{e}"))
    } else {
        Ok(uo_docx::export::to_html_units(&ed, &uo_docx::export::HtmlOptions { full_document: None, title, images: Some(images) }))
    }
}

/// The document exports of the shell: `kind` 0 `exportOdt` (`args`: `'omit'` or not; bytes), 1 `exportHtml` and 2
/// `exportMarkdown` (`args`: the title (`\u{1}` = none), `'omit'`/`'data'`, then pairs of drawing id and data URI;
/// UTF-16 units), 3 the images to compress (`u32 count`, then per image `u32 drawing`, `u32 ext length`, the
/// ext (UTF-8), `u32 byte length`, the bytes). Status 5 with the error text when the exporter fails.
///
/// # Safety
/// As [`uo_shell_fmt_json`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn uo_shell_fmt_export(h: *const UoWriteSession, kind: u32, ptr: *mut u8, len: usize) -> *mut UoBuf {
    // SAFETY: the caller guarantees the buffer; it is consumed here.
    let bytes = if len == 0 { Vec::new() } else { unsafe { take_bytes(ptr, len) } };
    // SAFETY: the caller guarantees `h` is null or live.
    let Some(s) = (unsafe { h.as_ref() }) else { return buf(NULL_HANDLE, Vec::new()) };
    let args = if bytes.is_empty() { Vec::new() } else { strings(&bytes).unwrap_or_default() };
    let a = |i: usize| args.get(i).map(|u| text(u));
    match kind {
        0 => match export_odt(&s.session, a(0).as_deref() == Some("omit")) {
            Ok(b) => buf(OK, b),
            Err(e) => buf(BAD_ARGUMENT, e.into_bytes()),
        },
        1 | 2 => {
            let title = a(0).filter(|t| t != "\u{1}");
            let omit = a(1).as_deref() == Some("omit");
            let mut uris: HashMap<DrawingId, Box<str>> = HashMap::new();
            let mut i = 2;
            while let (Some(id), Some(u)) = (a(i), a(i + 1)) {
                if let Ok(n) = id.parse::<u32>() {
                    uris.insert(DrawingId(n), u.into_boxed_str());
                }
                i += 2;
            }
            match export_text(&s.session, kind == 2, title.as_deref(), omit, &uris) {
                Ok(u) => units_buf(&u),
                Err(e) => buf(BAD_ARGUMENT, e.into_bytes()),
            }
        }
        3 => {
            let imgs = collect_images(s.session.document());
            let mut out = Vec::new();
            out.extend_from_slice(&u32::try_from(imgs.len()).unwrap_or(u32::MAX).to_le_bytes());
            for (id, ext, data) in imgs {
                out.extend_from_slice(&id.0.to_le_bytes());
                out.extend_from_slice(&u32::try_from(ext.len()).unwrap_or(0).to_le_bytes());
                out.extend_from_slice(ext.as_bytes());
                out.extend_from_slice(&u32::try_from(data.len()).unwrap_or(0).to_le_bytes());
                out.extend_from_slice(&data);
            }
            buf(OK, out)
        }
        _ => buf(NONE, Vec::new()),
    }
}

/// Opens an `.odt` (`loadOdt`, consumed) with the options of a `SessionOpen` message (`opts`, consumed), as
/// `uo_session_open`: the handle, or null with the last error set. The font embedding is Write's default (the TS
/// `DEFAULT_EMBEDDING.write`); the document embeds no fonts.
///
/// # Safety
/// As `uo_session_open`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn uo_shell_open_odt(
    env: *const UoWriteEnv,
    opts_ptr: *mut u8,
    opts_len: usize,
    doc_ptr: *mut u8,
    doc_len: usize,
) -> *mut UoWriteSession {
    pipeline::clear_last();
    // SAFETY: the caller guarantees the buffers; both are consumed here.
    let (opts, doc) = unsafe { (take_bytes(opts_ptr, opts_len), take_bytes(doc_ptr, doc_len)) };
    let (o, now, seed) = match decode_open(&opts) {
        Ok(x) => x,
        Err(d) => {
            pipeline::fail_with(code::ARGUMENT, &format!("open options: {d}"));
            return std::ptr::null_mut();
        }
    };
    let now = Rc::new(Cell::new(now));
    let host = HostEnv::new(Rc::clone(&now), seed);
    match Session::open_odt(&doc, o, Box::new(host)) {
        Ok(session) => {
            // SAFETY: the caller guarantees `env` is null or live.
            let fonts = unsafe { env.as_ref() }.map(HostFonts::of);
            Box::into_raw(Box::new(UoWriteSession::new(session, Vec::new(), fonts, now, uo_fonts::embed::WORD_DEFAULT_EMBEDDING)))
        }
        Err(e) => {
            open_failed(e);
            std::ptr::null_mut()
        }
    }
}

// ───────────────────────────── the ruler ─────────────────────────────

/// The engine line of the focus for the ruler (`getRulerInfo` 17158, engine branch): `{ page, x, y, frame: [l, r] |
/// null, pfmt: { left, right, firstLine, tabs: [{ val, pos, leader? }] } | null }` (UTF-8 JSON) of the edit scope's line
/// holding the selection's focus on layout `l`; status 1 without one (no line: the TS `line` undefined).
///
/// # Safety
/// `h`, `l` null or live.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn uo_shell_fmt_ruler(h: *mut UoWriteSession, l: *const crate::write::UoLayout) -> *mut UoBuf {
    // SAFETY: the caller guarantees the handles are null or live.
    let (Some(s), Some(l)) = (unsafe { h.as_mut() }, unsafe { l.as_ref() }) else { return buf(NULL_HANDLE, Vec::new()) };
    let focus = s.session.selection().focus;
    let Some(el) = s.lines(l) else { return buf(NONE, Vec::new()) };
    let Some(line) = el.line_of(uo_docx_layout::hit::EditPos { para: focus.para, offset: focus.offset }).cloned() else {
        return buf(NONE, Vec::new());
    };
    let geom = crate::write::layout_geom(l);
    let mut out = String::with_capacity(256);
    out.push_str("{\"page\":");
    push_number(&mut out, f64::from(line.page));
    out.push_str(",\"x\":");
    push_number(&mut out, line.x);
    out.push_str(",\"y\":");
    push_number(&mut out, line.y);
    let page = geom.pages.get(line.page as usize);
    // the page line the edit line came from: same start, top and left (a text box line is shifted onto the page: none)
    let lg = page.and_then(|p| {
        p.lines
            .iter()
            .chain(p.content_lines.iter())
            .find(|g| g.start == line.line_start && (g.y - line.y).abs() < 1e-6 && (g.x - line.x).abs() < 1e-6)
    });
    out.push_str(",\"frame\":");
    match lg.and_then(|g| g.frame) {
        Some((fl, fr)) => {
            out.push('[');
            push_number(&mut out, fl);
            out.push(',');
            push_number(&mut out, fr);
            out.push(']');
        }
        None => out.push_str("null"),
    }
    out.push_str(",\"pfmt\":");
    match lg.and_then(|g| g.pfmt).and_then(|i| page.and_then(|p| p.pfmts.get(i as usize))) {
        Some(f) => {
            out.push_str("{\"left\":");
            push_number(&mut out, f.left);
            out.push_str(",\"right\":");
            push_number(&mut out, f.right);
            out.push_str(",\"firstLine\":");
            push_number(&mut out, f.first_line);
            out.push_str(",\"tabs\":[");
            for (i, t) in f.tabs.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                out.push_str("{\"val\":");
                push_string(&mut out, t.val.name());
                out.push_str(",\"pos\":");
                push_number(&mut out, t.pos);
                if let Some(ld) = t.leader {
                    out.push_str(",\"leader\":");
                    push_string(&mut out, ld.name());
                }
                out.push('}');
            }
            out.push_str("]}");
        }
        None => out.push_str("null"),
    }
    out.push('}');
    buf(OK, out.into_bytes())
}

// ───────────────────────────── the views ─────────────────────────────

/// The view the shell engine `e` lays out from now on (`setView`): `kind` 0 Print (the default), 1 Web of `width_tw`
/// twips, 2 Draft, 3 Outline (frontier B's transform: Draft plus 720 twips for the unstyled paragraphs). The next
/// relayout sees the changed options (a full one, reason `view`).
///
/// # Safety
/// `e` null or live.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn uo_shell_engine_set_view(e: *mut super::shell::UoShellEngine, kind: u32, width_tw: f64) -> i32 {
    use uo_docx_layout::options::ViewTransform;
    // SAFETY: the caller guarantees `e` is null or live.
    let Some(e) = (unsafe { e.as_mut() }) else { return NULL_HANDLE };
    e.view = match kind {
        1 => Some(ViewTransform::Web { width_tw }),
        2 => Some(ViewTransform::Draft),
        3 => Some(ViewTransform::Outline),
        _ => None,
    };
    OK
}

/// Wave 7c (V): the editor's continuous views (`ViewTransform::Continuous`): `kind` 1 Web, 2 Draft, 3 Outline (0 = Print);
/// `width_tw` the Web sheet's width (the TS `contentWidthTwips() + 2 × contMarginTwips()`), `margin_tw` the TS
/// `contMarginTwips()`, `ts_strict` ≠ 0 the frozen TS's rules where Word differs (the harness's strict screens). The next
/// relayout sees the changed options.
///
/// # Safety
/// `e` null or live.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn uo_shell_engine_set_continuous(
    e: *mut super::shell::UoShellEngine,
    kind: u32,
    width_tw: f64,
    margin_tw: f64,
    ts_strict: u32,
) -> i32 {
    use uo_docx_layout::options::{ContinuousKind, ContinuousView, ViewTransform};
    // SAFETY: the caller guarantees `e` is null or live.
    let Some(e) = (unsafe { e.as_mut() }) else { return NULL_HANDLE };
    let kind = match kind {
        1 => Some(ContinuousKind::Web),
        2 => Some(ContinuousKind::Draft),
        3 => Some(ContinuousKind::Outline),
        _ => None,
    };
    let width_tw = if width_tw.is_finite() { width_tw } else { 0.0 };
    let margin_tw = if margin_tw.is_finite() && margin_tw >= 0.0 { margin_tw } else { 600.0 };
    e.view = kind.map(|kind| ViewTransform::Continuous(ContinuousView { kind, width_tw, margin_tw, ts_strict: ts_strict != 0 }));
    OK
}

// ───────────────────────────── the TOC gallery preview ─────────────────────────────

fn toc_id(id: u32) -> Option<uo_docx::session::ops::references::toc::TocTemplateId> {
    use uo_docx::session::ops::references::toc::TocTemplateId;
    match id {
        0 => Some(TocTemplateId::Auto1),
        1 => Some(TocTemplateId::Auto2),
        2 => Some(TocTemplateId::Manual),
        _ => None,
    }
}

/// The layout of the preview of TOC template `id` (0 auto1, 1 auto2, 2 manual) for `paintTocTemplate` (universal-write.ts
/// 7123): uo-docx's `toc_template_preview` (the heading and three entries with the document's styles, the missing ones by
/// their built-in definition, not written to the document) laid out with the session's fonts and the editor's options
/// without revisions; an owned layout handle (the viewer's `UoLayout`: free it), null with the last error set.
///
/// # Safety
/// `h`, `env` null or live.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn uo_shell_fmt_toc_template(
    h: *mut UoWriteSession,
    env: *const UoWriteEnv,
    id: u32,
    now_ms: f64,
    tz_offset_min: i32,
) -> *mut crate::write::UoLayout {
    pipeline::clear_last();
    // SAFETY: the caller guarantees the handles are null or live.
    let (Some(s), Some(env)) = (unsafe { h.as_mut() }, unsafe { env.as_ref() }) else {
        pipeline::fail(&crate::pipeline::JobError::Argument("session or environment"));
        return std::ptr::null_mut();
    };
    let Some(id) = toc_id(id) else {
        pipeline::fail(&crate::pipeline::JobError::Argument("template id"));
        return std::ptr::null_mut();
    };
    let Some(preview) = s.session.toc_template_preview(id) else {
        pipeline::fail(&crate::pipeline::JobError::Argument("template"));
        return std::ptr::null_mut();
    };
    let Some(base) = crate::write::session_layout_options(env, now_ms, tz_offset_min, 0, &[], &[]) else {
        pipeline::fail(&crate::pipeline::JobError::Argument("layout options"));
        return std::ptr::null_mut();
    };
    let mut o = super::layout::editor_options(&s.session, &base);
    o.revisions = None;
    let fonts = HostFonts::of(env);
    let view = uo_docx::view::DocView::new(&preview.document);
    let laid = (|| -> Result<crate::pipeline::LaidOut, crate::pipeline::JobError> {
        let provider = super::oracle::font_provider(&fonts, &s.embedded).map_err(crate::pipeline::JobError::Argument)?;
        let ldoc = uo_docx_layout::derive::build(view, &o)?;
        let mut hyph = uo_docx_layout::hyphenate::HyphenSessions::new(std::sync::Arc::clone(&fonts.hyphenation));
        let layout = uo_docx_layout::engine::layout_document(&ldoc, &o, &*provider, &mut hyph)?;
        let navigation = layout.navigation(view);
        let links = layout.run_boxes(view, uo_docx_layout::run_boxes::RunFilter::Links);
        let inlines = layout.run_boxes(view, uo_docx_layout::run_boxes::RunFilter::CustomInlines);
        Ok(crate::pipeline::LaidOut { layout, navigation, links, inlines })
    })();
    match laid {
        Ok(l) => Box::into_raw(Box::new(crate::write::layout_handle(l, view, fonts.phone, 0))),
        Err(e) => {
            pipeline::fail(&e);
            std::ptr::null_mut()
        }
    }
}

/// The crop of the TOC preview `id` (twips): `{ cropLeft, marginRight }` (UTF-8 JSON; status 1 for an unknown id).
///
/// # Safety
/// `h` null or live.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn uo_shell_fmt_toc_crop(h: *mut UoWriteSession, id: u32) -> *mut UoBuf {
    // SAFETY: the caller guarantees `h` is null or live.
    let Some(s) = (unsafe { h.as_mut() }) else { return buf(NULL_HANDLE, Vec::new()) };
    let Some(p) = toc_id(id).and_then(|id| s.session.toc_template_preview(id)) else { return buf(NONE, Vec::new()) };
    let mut out = String::from("{\"cropLeft\":");
    push_number(&mut out, p.crop_left_twips);
    out.push_str(",\"marginRight\":");
    push_number(&mut out, p.margin_right_twips);
    out.push('}');
    buf(OK, out.into_bytes())
}

// ───────────────────────────── File > Compare ─────────────────────────────

/// File > Compare (wave 7, A1, D-A1-3): `diffDocuments(a, b, opts)` of the compare package ported in uo-docx (`compare`,
/// D5a) over two `.docx` (the session's saved document and the picked one), both consumed; the answer is
/// `JSON.stringify(diff)` of the TS `DocDiff` (UTF-8, `DocDiff::to_json`). `flags`: bit 0 the Myers algorithm (else
/// histogram), bit 1 no word refinement. The work is bounded by the default limits of `compare::budget` (the TS has none:
/// a hostile pair answers a valid, not minimal, script). Status 5 with the reason when a document does not read.
///
/// # Safety
/// The buffers come from `uo_alloc_bytes` (or are empty: `len` 0).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn uo_shell_fmt_compare(a_ptr: *mut u8, a_len: usize, b_ptr: *mut u8, b_len: usize, flags: u32) -> *mut UoBuf {
    use uo_docx::compare::{DiffAlgorithm, DocDiffOptions, diff_documents};
    // SAFETY: the caller guarantees the buffers; both are consumed here.
    let a = if a_len == 0 { Vec::new() } else { unsafe { take_bytes(a_ptr, a_len) } };
    // SAFETY: as above.
    let b = if b_len == 0 { Vec::new() } else { unsafe { take_bytes(b_ptr, b_len) } };
    // `readFullDocument(DocxPackage.load(bytes))` of the diff element
    let read = |bytes: &[u8], side: &str| -> Result<Document, String> {
        let pkg = uo_opc::Package::open(bytes, &uo_opc::Limits::DEFAULT, &uo_opc::package::OpenOptions::default())
            .map_err(|e| format!("{side}: {e}"))?;
        uo_docx::read::read_document(&pkg, &uo_docx::read::ReadOptions::default()).map_err(|e| format!("{side}: {e}"))
    };
    let (da, db) = match (read(&a, "a"), read(&b, "b")) {
        (Ok(da), Ok(db)) => (da, db),
        (Err(e), _) | (_, Err(e)) => return buf(BAD_ARGUMENT, e.into_bytes()),
    };
    let opts = DocDiffOptions {
        algorithm: if flags & 1 != 0 { DiffAlgorithm::Myers } else { DiffAlgorithm::Histogram },
        intra_word: flags & 2 == 0,
    };
    buf(OK, diff_documents(&da, &db, &opts).to_json().into_bytes())
}

// ───────────────────────────── XPS (fix round 2) ─────────────────────────────

/// File > Save as XPS (`exportXps()`, universal-write.ts 22640; D-A1-2, fix round 2): the XPS package of layout `l`'s
/// pages, written by uo-pdf's XPS writer from the display list (the faces' bytes, the images, alphas, clips, paths).
/// Status 5 with the reason when the zip writer refuses a part.
///
/// # Safety
/// `l` null or live.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn uo_shell_fmt_xps(l: *const crate::write::UoLayout) -> *mut UoBuf {
    // SAFETY: the caller guarantees the handle is null or live.
    let Some(l) = (unsafe { l.as_ref() }) else { return buf(NULL_HANDLE, Vec::new()) };
    let v = crate::write::layout_view(l);
    match uo_pdf::render_xps(v.pages(), v.resources(), &uo_pdf::XpsOptions::default()) {
        Ok(bytes) => buf(OK, bytes),
        Err(e) => buf(BAD_ARGUMENT, e.to_string().into_bytes()),
    }
}
