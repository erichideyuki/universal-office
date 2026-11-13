//! Wave 7 (A2) of the editor switch (record `core/docs/modules/uo-bindings.md` "Wave 7 — A2: lists, paste,
//! tables, objects and the page"): the shell exports of the last families the TS shell still answered over its
//! own empty model. Conventions as `shell_objects.rs`: `UoBuf` statuses 0 ok, 1 none, 5 bad argument,
//! 11 null handle; JSON answers are UTF-8 text in the TS key order; the edits answer an `ExecResult`.
//!
//! - lists: `listInfo()` (+ `canContinue`), `documentLists()`, `listStylesInDocument()`,
//!   `numberingValueInfo()`, `listLibrary(kind)` — uo-docx `session::ops::completion`;
//! - tables: the gallery entries before the shell's `localeCompare` sort, `tableStyleXml`, `tableStyleBases`,
//!   `newTableStyleId`, the grid of a table (the edge drag's limits);
//! - clipboard: `hasCellsClip()`, the kind of each ring item (`clipboardItems()`);
//! - `uo_shell_edit`: a uo-docx `ShellEdit` given as JSON (no wire codec: decision 1 of the record).

use uo_docx::js::JsonValue;
use uo_docx::model::props::BorderSpec;
use uo_docx::session::CellHop;
use uo_docx::session::ops::clipboard::{ItemKind, clipboard_items, has_cells_clip};
use uo_docx::session::ops::completion::LevelView;
use uo_docx::session::ops::drawing::{ObjFacts, ObjFrame, ObjRect};
use uo_docx::session::ops::shell_edits::ShellEdit;
use uo_docx::session::ops::tables::completion::{CellSide, FilledTable, TableGesture};
use uo_jsnum::json::{push_number, push_string};

use super::handle::{BAD_ARGUMENT, UoWriteSession, exec_result, outcome};
use super::ring::with_ring;
use crate::render::{NONE, NULL_HANDLE, OK, UoBuf, buf, take_bytes};

fn json_buf(s: String) -> *mut UoBuf {
    buf(OK, s.into_bytes())
}

fn key(out: &mut String, first: &mut bool, name: &str) {
    if !*first {
        out.push(',');
    }
    *first = false;
    push_string(out, name);
    out.push(':');
}

/// `ListLevelSpec` in the TS key order (`listLevelSpec`).
fn level_json(out: &mut String, l: &LevelView) {
    let mut f = true;
    out.push('{');
    key(out, &mut f, "ilvl");
    push_number(out, l.ilvl);
    key(out, &mut f, "numFmt");
    push_string(out, &l.num_fmt);
    key(out, &mut f, "lvlText");
    push_string(out, &l.lvl_text);
    if let Some(x) = &l.font {
        key(out, &mut f, "font");
        push_string(out, x);
    }
    if l.bold {
        key(out, &mut f, "bold");
        out.push_str("true");
    }
    if l.italic {
        key(out, &mut f, "italic");
        out.push_str("true");
    }
    if let Some(x) = l.size_pt {
        key(out, &mut f, "sizePt");
        push_number(out, x);
    }
    if let Some(x) = &l.color {
        key(out, &mut f, "color");
        push_string(out, x);
    }
    key(out, &mut f, "start");
    push_number(out, l.start);
    if let Some(x) = &l.p_style {
        key(out, &mut f, "pStyle");
        push_string(out, x);
    }
    key(out, &mut f, "indLeft");
    push_number(out, l.ind_left);
    key(out, &mut f, "hanging");
    push_number(out, l.hanging);
    key(out, &mut f, "jc");
    push_string(out, l.jc);
    key(out, &mut f, "suff");
    push_string(out, &l.suff);
    if let Some(x) = l.tab_pos {
        key(out, &mut f, "tabPos");
        push_number(out, x);
    }
    if l.is_lgl {
        key(out, &mut f, "isLgl");
        out.push_str("true");
    }
    if let Some(x) = l.lvl_restart {
        key(out, &mut f, "lvlRestart");
        push_number(out, x);
    }
    out.push('}');
}

fn levels_json(out: &mut String, ls: &[LevelView]) {
    out.push('[');
    for (i, l) in ls.iter().enumerate() {
        if i > 0 {
            out.push(',');
        }
        level_json(out, l);
    }
    out.push(']');
}

/// # Safety
/// `h` null or live.
unsafe fn session_mut<'a>(h: *mut UoWriteSession) -> Option<&'a mut UoWriteSession> {
    // SAFETY: the caller guarantees `h` is null or live.
    unsafe { h.as_mut() }
}

/// `listInfo()` of the caret's paragraph and `canContinue` (JSON), or none outside a list:
/// `{ numId, ilvl, absId, kind, level?, multilevel, levels, canContinue }`.
///
/// # Safety
/// `h` null or live.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn uo_shell_list_info(h: *mut UoWriteSession) -> *mut UoBuf {
    // SAFETY: the caller guarantees `h`.
    let Some(s) = (unsafe { session_mut(h) }) else { return buf(NULL_HANDLE, Vec::new()) };
    let Some(i) = s.session.list_info_view() else { return buf(NONE, Vec::new()) };
    let mut out = String::with_capacity(512);
    let mut f = true;
    out.push('{');
    key(&mut out, &mut f, "numId");
    push_number(&mut out, i.num_id);
    key(&mut out, &mut f, "ilvl");
    push_number(&mut out, i.ilvl);
    key(&mut out, &mut f, "absId");
    push_number(&mut out, i.abs_id);
    key(&mut out, &mut f, "kind");
    push_string(&mut out, if i.bullet { "bullet" } else { "number" });
    if let Some(l) = &i.level {
        key(&mut out, &mut f, "level");
        level_json(&mut out, l);
    }
    key(&mut out, &mut f, "multilevel");
    out.push_str(if i.multilevel { "true" } else { "false" });
    key(&mut out, &mut f, "levels");
    levels_json(&mut out, &i.levels);
    key(&mut out, &mut f, "canContinue");
    out.push_str(if i.can_continue { "true" } else { "false" });
    out.push('}');
    json_buf(out)
}

/// `documentLists()` (JSON): `[{ numId, kind, level, levels }]`.
///
/// # Safety
/// `h` null or live.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn uo_shell_document_lists(h: *mut UoWriteSession) -> *mut UoBuf {
    // SAFETY: the caller guarantees `h`.
    let Some(s) = (unsafe { session_mut(h) }) else { return buf(NULL_HANDLE, Vec::new()) };
    let lists = s.session.document_lists_view();
    let mut out = String::from("[");
    for (k, d) in lists.iter().enumerate() {
        if k > 0 {
            out.push(',');
        }
        let mut f = true;
        out.push('{');
        key(&mut out, &mut f, "numId");
        push_number(&mut out, d.num_id);
        key(&mut out, &mut f, "kind");
        push_string(&mut out, if d.bullet { "bullet" } else { "number" });
        key(&mut out, &mut f, "level");
        level_json(&mut out, &d.level);
        key(&mut out, &mut f, "levels");
        levels_json(&mut out, &d.levels);
        out.push('}');
    }
    out.push(']');
    json_buf(out)
}

/// `listStylesInDocument()` (JSON): `[{ id, name, numId, levels }]`.
///
/// # Safety
/// `h` null or live.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn uo_shell_list_styles(h: *mut UoWriteSession) -> *mut UoBuf {
    // SAFETY: the caller guarantees `h`.
    let Some(s) = (unsafe { session_mut(h) }) else { return buf(NULL_HANDLE, Vec::new()) };
    let list = s.session.list_styles_view();
    let mut out = String::from("[");
    for (k, d) in list.iter().enumerate() {
        if k > 0 {
            out.push(',');
        }
        let mut f = true;
        out.push('{');
        key(&mut out, &mut f, "id");
        push_string(&mut out, &d.id);
        key(&mut out, &mut f, "name");
        push_string(&mut out, &d.name);
        key(&mut out, &mut f, "numId");
        push_number(&mut out, d.num_id);
        key(&mut out, &mut f, "levels");
        levels_json(&mut out, &d.levels);
        out.push('}');
    }
    out.push(']');
    json_buf(out)
}

/// `numberingValueInfo()` (JSON `{ canContinue, value, numFmt }`), or none.
///
/// # Safety
/// `h` null or live.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn uo_shell_numbering_value(h: *mut UoWriteSession) -> *mut UoBuf {
    // SAFETY: the caller guarantees `h`.
    let Some(s) = (unsafe { session_mut(h) }) else { return buf(NULL_HANDLE, Vec::new()) };
    let Some((can, value, fmt)) = s.session.numbering_value_view() else { return buf(NONE, Vec::new()) };
    let mut out = String::from("{\"canContinue\":");
    out.push_str(if can { "true" } else { "false" });
    out.push_str(",\"value\":");
    push_number(&mut out, value);
    out.push_str(",\"numFmt\":");
    push_string(&mut out, &fmt);
    out.push('}');
    json_buf(out)
}

/// `listLibrary(kind)` (JSON `ListLevelSpec[][]`): 0 bullet, 1 number, 2 multilevel.
///
/// # Safety
/// `h` null or live.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn uo_shell_list_library(h: *mut UoWriteSession, kind: u32) -> *mut UoBuf {
    // SAFETY: the caller guarantees `h`.
    let Some(s) = (unsafe { session_mut(h) }) else { return buf(NULL_HANDLE, Vec::new()) };
    let lib = s.session.list_library_view(kind);
    let mut out = String::from("[");
    for (k, ls) in lib.iter().enumerate() {
        if k > 0 {
            out.push(',');
        }
        levels_json(&mut out, ls);
    }
    out.push(']');
    json_buf(out)
}

/// The table style gallery's entries in the TS order BEFORE its sort (the shell sorts: `localeCompare`):
/// `[{ name, label: { en, pt, es }, group, id? }]`.
///
/// # Safety
/// `h` null or live.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn uo_shell_table_styles(h: *mut UoWriteSession) -> *mut UoBuf {
    // SAFETY: the caller guarantees `h`.
    let Some(s) = (unsafe { session_mut(h) }) else { return buf(NULL_HANDLE, Vec::new()) };
    let list = s.session.table_style_listing();
    let mut out = String::with_capacity(list.len() * 96 + 2);
    out.push('[');
    for (k, t) in list.iter().enumerate() {
        if k > 0 {
            out.push(',');
        }
        out.push_str("{\"name\":");
        push_string(&mut out, &t.name);
        out.push_str(",\"label\":{\"en\":");
        push_string(&mut out, &t.en);
        out.push_str(",\"pt\":");
        push_string(&mut out, &t.pt);
        out.push_str(",\"es\":");
        push_string(&mut out, &t.es);
        out.push_str("},\"group\":");
        push_string(&mut out, t.group);
        if let Some(id) = &t.id {
            out.push_str(",\"id\":");
            push_string(&mut out, id);
        }
        out.push('}');
    }
    out.push(']');
    json_buf(out)
}

/// `tableStyleXml(nameOrId)` (JSON `{ id, xml, inDoc }`), or none.
///
/// # Safety
/// `h` null or live; `(ptr, len)` a buffer of `uo_alloc_bytes(len)` (consumed).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn uo_shell_table_style_xml(h: *mut UoWriteSession, ptr: *mut u8, len: usize) -> *mut UoBuf {
    // SAFETY: the caller guarantees the buffer; consumed here.
    let name = String::from_utf8_lossy(&unsafe { take_bytes(ptr, len) }).into_owned();
    // SAFETY: the caller guarantees `h`.
    let Some(s) = (unsafe { session_mut(h) }) else { return buf(NULL_HANDLE, Vec::new()) };
    let Some((id, xml, in_doc)) = s.session.table_style_xml(&name) else { return buf(NONE, Vec::new()) };
    let mut out = String::from("{\"id\":");
    push_string(&mut out, &id);
    out.push_str(",\"xml\":");
    push_string(&mut out, &xml);
    out.push_str(",\"inDoc\":");
    out.push_str(if in_doc { "true" } else { "false" });
    out.push('}');
    json_buf(out)
}

/// `tableStyleBases()` (JSON `[{ id, name }]`).
///
/// # Safety
/// `h` null or live.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn uo_shell_table_style_bases(h: *mut UoWriteSession) -> *mut UoBuf {
    // SAFETY: the caller guarantees `h`.
    let Some(s) = (unsafe { session_mut(h) }) else { return buf(NULL_HANDLE, Vec::new()) };
    let mut out = String::from("[");
    for (k, (id, name)) in s.session.table_style_bases().iter().enumerate() {
        if k > 0 {
            out.push(',');
        }
        out.push_str("{\"id\":");
        push_string(&mut out, id);
        out.push_str(",\"name\":");
        push_string(&mut out, name);
        out.push('}');
    }
    out.push(']');
    json_buf(out)
}

/// `newTableStyleId(name)` (JSON string).
///
/// # Safety
/// `h` null or live; `(ptr, len)` a buffer of `uo_alloc_bytes(len)` (consumed).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn uo_shell_new_table_style_id(h: *mut UoWriteSession, ptr: *mut u8, len: usize) -> *mut UoBuf {
    // SAFETY: the caller guarantees the buffer; consumed here.
    let name = String::from_utf8_lossy(&unsafe { take_bytes(ptr, len) }).into_owned();
    // SAFETY: the caller guarantees `h`.
    let Some(s) = (unsafe { session_mut(h) }) else { return buf(NULL_HANDLE, Vec::new()) };
    let mut out = String::new();
    push_string(&mut out, &s.session.new_table_style_id(&name));
    json_buf(out)
}

/// The grid column widths of the table at `anchor` and the hops `path_json` (`[[row, col, block|-1]…]`):
/// JSON numbers, or none when the table is gone.
///
/// # Safety
/// `h` null or live; `(ptr, len)` a buffer of `uo_alloc_bytes(len)` (consumed).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn uo_shell_table_grid(h: *mut UoWriteSession, anchor: u32, ptr: *mut u8, len: usize) -> *mut UoBuf {
    // SAFETY: the caller guarantees the buffer; consumed here.
    let text = String::from_utf8_lossy(&unsafe { take_bytes(ptr, len) }).into_owned();
    // SAFETY: the caller guarantees `h`.
    let Some(s) = (unsafe { session_mut(h) }) else { return buf(NULL_HANDLE, Vec::new()) };
    let path = match uo_docx::read::json_parse::parse(&text).as_ref().map(hops) {
        Some(Ok(p)) => p,
        _ => return buf(BAD_ARGUMENT, b"path".to_vec()),
    };
    let Some(g) = s.session.shell_table_grid(anchor, &path) else { return buf(NONE, Vec::new()) };
    let mut out = String::from("[");
    for (k, w) in g.iter().enumerate() {
        if k > 0 {
            out.push(',');
        }
        push_number(&mut out, *w);
    }
    out.push(']');
    json_buf(out)
}

/// `hasCellsClip()` of the instance's ring (JSON boolean).
#[unsafe(no_mangle)]
pub extern "C" fn uo_shell_has_cells_clip() -> *mut UoBuf {
    let on = with_ring(|r| has_cells_clip(r));
    json_buf(if on { "true" } else { "false" }.to_owned())
}

/// The kind of each item of the instance's ring, newest first (JSON `[[id, kind]…]`; kind `text` | `table` |
/// `drawing` | `image`, the TS `clipboardItems()`).
#[unsafe(no_mangle)]
pub extern "C" fn uo_shell_clipboard_kinds() -> *mut UoBuf {
    let items = with_ring(|r| clipboard_items(r));
    let mut out = String::from("[");
    for (k, it) in items.iter().enumerate() {
        if k > 0 {
            out.push(',');
        }
        out.push('[');
        push_number(&mut out, f64::from(it.id));
        out.push(',');
        push_string(
            &mut out,
            match it.kind {
                ItemKind::Text => "text",
                ItemKind::Table => "table",
                ItemKind::Drawing => "drawing",
                ItemKind::Image => "image",
            },
        );
        out.push(']');
    }
    out.push(']');
    json_buf(out)
}

// ───────────────────────── the shell edit (JSON) ─────────────────────────

fn field<'a>(v: &'a JsonValue, k: &str) -> Option<&'a JsonValue> {
    match v {
        JsonValue::Object(o) => o.get(k),
        _ => None,
    }
}

fn f64_of(v: &JsonValue, k: &str) -> Result<f64, String> {
    match field(v, k) {
        Some(JsonValue::Num(n)) if n.is_finite() => Ok(*n),
        _ => Err(format!("{k}: a finite number")),
    }
}

fn opt_f64(v: &JsonValue, k: &str) -> Option<f64> {
    match field(v, k) {
        Some(JsonValue::Num(n)) if n.is_finite() => Some(*n),
        _ => None,
    }
}

fn int_of(v: &JsonValue, k: &str) -> Result<i64, String> {
    let n = f64_of(v, k)?;
    if n.fract() != 0.0 || n.abs() > 2_147_483_647.0 {
        return Err(format!("{k}: an integer"));
    }
    // checked above: an integer in the i32 range
    Ok(i64::from(i32::try_from(n.to_string().parse::<i64>().unwrap_or(0)).unwrap_or(0)))
}

fn u32_of(v: &JsonValue, k: &str) -> Result<u32, String> {
    u32::try_from(int_of(v, k)?).map_err(|_| format!("{k}: not negative"))
}

fn bool_of(v: &JsonValue, k: &str) -> bool {
    matches!(field(v, k), Some(JsonValue::Bool(true)))
}

fn str_of(v: &JsonValue, k: &str) -> Option<String> {
    match field(v, k) {
        Some(JsonValue::Str(s)) => Some(s.to_string_lossy()),
        _ => None,
    }
}

fn arr<'a>(v: &'a JsonValue, k: &str) -> &'a [JsonValue] {
    match field(v, k) {
        Some(JsonValue::Array(a)) => a,
        _ => &[],
    }
}

fn num_list(v: &JsonValue, k: &str) -> Vec<f64> {
    arr(v, k).iter().filter_map(|x| if let JsonValue::Num(n) = x { n.is_finite().then_some(*n) } else { None }).collect()
}

/// `[[row, col, block|-1]…]`.
fn hops(v: &JsonValue) -> Result<Vec<CellHop>, String> {
    let JsonValue::Array(a) = v else { return Err("path".to_owned()) };
    a.iter()
        .map(|h| {
            let JsonValue::Array(t) = h else { return Err("hop".to_owned()) };
            let n = |i: usize| match t.get(i) {
                Some(JsonValue::Num(x)) if x.is_finite() && x.fract() == 0.0 => Ok(*x),
                _ => Err("hop".to_owned()),
            };
            let (r, c, b) = (n(0)?, n(1)?, n(2).unwrap_or(-1.0));
            let u = |x: f64| -> Result<u32, String> {
                if (0.0..=4_294_967_295.0).contains(&x) {
                    x.to_string().parse::<u32>().map_err(|_| "hop".to_owned())
                } else {
                    Err("hop".to_owned())
                }
            };
            Ok(CellHop { row: u(r)?, col: u(c)?, block: if b < 0.0 { None } else { Some(u(b)?) } })
        })
        .collect()
}

fn side_of(v: &JsonValue) -> Result<CellSide, String> {
    match str_of(v, "side").as_deref() {
        Some("top") => Ok(CellSide::Top),
        Some("bottom") => Ok(CellSide::Bottom),
        Some("left") => Ok(CellSide::Left),
        Some("right") => Ok(CellSide::Right),
        _ => Err("side".to_owned()),
    }
}

fn pen_of(v: &JsonValue) -> Result<BorderSpec, String> {
    let p = field(v, "pen").ok_or("pen")?;
    let mut b = BorderSpec { val: str_of(p, "val").ok_or("pen.val")?.into(), ..BorderSpec::default() };
    b.sz = opt_f64(p, "sz").map(uo_docx::js::JsNumber::Num);
    b.space = opt_f64(p, "space").map(uo_docx::js::JsNumber::Num);
    b.color = str_of(p, "color").map(Into::into);
    b.theme_color = str_of(p, "themeColor").map(Into::into);
    b.theme_tint = str_of(p, "themeTint").map(Into::into);
    b.theme_shade = str_of(p, "themeShade").map(Into::into);
    if let Some(JsonValue::Bool(x)) = field(p, "shadow") {
        b.shadow = Some(*x);
    }
    if let Some(JsonValue::Bool(x)) = field(p, "frame") {
        b.frame = Some(*x);
    }
    Ok(b)
}

/// An `ObjFrame` from its JSON (`{ pageW, pageH, marL, … }`).
fn frame_of(r: &JsonValue) -> Option<ObjFrame> {
    Some(ObjFrame {
        page_w: opt_f64(r, "pageW")?,
        page_h: opt_f64(r, "pageH")?,
        mar_l: opt_f64(r, "marL")?,
        mar_r: opt_f64(r, "marR")?,
        mar_t: opt_f64(r, "marT")?,
        mar_b: opt_f64(r, "marB")?,
        col_x: opt_f64(r, "colX")?,
        col_w: opt_f64(r, "colW")?,
        body_top: opt_f64(r, "bodyTop")?,
        para_top: opt_f64(r, "paraTop")?,
    })
}

fn facts_of(v: &JsonValue) -> Vec<ObjFacts> {
    arr(v, "facts")
        .iter()
        .map(|o| {
            let rect = field(o, "rect")
                .and_then(|r| Some(ObjRect { x: opt_f64(r, "x")?, y: opt_f64(r, "y")?, w: opt_f64(r, "w")?, h: opt_f64(r, "h")? }));
            let frame = field(o, "frame").and_then(frame_of);
            ObjFacts { rect, frame }
        })
        .collect()
}

/// The points `[[x, y]…]` of `v[k]` (finite numbers).
fn pts_of_key(v: &JsonValue, k: &str) -> Result<Vec<(f64, f64)>, String> {
    arr(v, k)
        .iter()
        .map(|p| match p {
            JsonValue::Array(xy) => match (xy.first(), xy.get(1)) {
                (Some(JsonValue::Num(x)), Some(JsonValue::Num(y))) if x.is_finite() && y.is_finite() => Ok((*x, *y)),
                _ => Err("pts".to_owned()),
            },
            _ => Err("pts".to_owned()),
        })
        .collect()
}

fn pts_of(v: &JsonValue) -> Result<Vec<(f64, f64)>, String> {
    pts_of_key(v, "pts")
}

/// A `ShellEdit` from its JSON (`{ t, ... }`).
fn decode_edit(v: &JsonValue) -> Result<ShellEdit, String> {
    let t = str_of(v, "t").ok_or("t")?;
    let path = || field(v, "path").map_or(Ok(Vec::new()), hops);
    let g = |x: TableGesture| Ok(ShellEdit::Table(Box::new(x)));
    match t.as_str() {
        "filledTable" => {
            let rows: Vec<Vec<String>> = arr(v, "rows")
                .iter()
                .map(|r| match r {
                    JsonValue::Array(c) => {
                        c.iter().map(|x| if let JsonValue::Str(s) = x { s.to_string_lossy() } else { String::new() }).collect()
                    }
                    _ => Vec::new(),
                })
                .collect();
            let merges = arr(v, "merges")
                .iter()
                .map(|m| Ok((int_of(m, "r")?, int_of(m, "c1")?, int_of(m, "c2")?)))
                .collect::<Result<Vec<_>, String>>()?;
            Ok(ShellEdit::FilledTable(Box::new(FilledTable {
                rows,
                style: str_of(v, "style").unwrap_or_default(),
                first_column_off: bool_of(v, "firstColumnOff"),
                merges,
                center_body: bool_of(v, "centerBody"),
            })))
        }
        "edge" => g(TableGesture::Edge {
            col: bool_of(v, "col"),
            anchor: u32_of(v, "anchor")?,
            path: path()?,
            boundary: int_of(v, "boundary")?,
            delta: f64_of(v, "delta")?,
            row_height: opt_f64(v, "rowHeight").unwrap_or(0.0),
        }),
        "move" => g(TableGesture::Move { anchor: u32_of(v, "anchor")?, dx: f64_of(v, "dx")?, dy: f64_of(v, "dy")? }),
        "size" => g(TableGesture::Size {
            anchor: u32_of(v, "anchor")?,
            dx: f64_of(v, "dx")?,
            dy: f64_of(v, "dy")?,
            row_heights: field(v, "rowHeights").map(|_| num_list(v, "rowHeights")),
        }),
        "paint" => g(TableGesture::Paint {
            anchor: u32_of(v, "anchor")?,
            path: path()?,
            r: int_of(v, "r")?,
            c: int_of(v, "c")?,
            side: side_of(v)?,
            pen: pen_of(v)?,
        }),
        "erase" => g(TableGesture::Erase {
            anchor: u32_of(v, "anchor")?,
            path: path()?,
            r: int_of(v, "r")?,
            c: int_of(v, "c")?,
            side: side_of(v)?,
        }),
        "drawSplit" => g(TableGesture::DrawSplit {
            anchor: u32_of(v, "anchor")?,
            path: path()?,
            r: int_of(v, "r")?,
            c: int_of(v, "c")?,
            vertical: bool_of(v, "vertical"),
            left: f64_of(v, "left")?,
            box_w: f64_of(v, "boxW")?,
            row_heights: num_list(v, "rowHeights"),
        }),
        "drawNew" => g(TableGesture::DrawNew { w: f64_of(v, "w")?, h: f64_of(v, "h")? }),
        "wrapPolygon" => Ok(ShellEdit::SetWrapPolygon { pts: pts_of(v)? }),
        // wave 7c (V): the live object gestures (`LiveBegin` at the mousedown, `LiveSet` per move, `LiveEnd { commit }` or
        // `LiveCancel` at the mouseup)
        "liveBegin" => Ok(ShellEdit::LiveBegin),
        "liveSet" => {
            let pair = |k: &str| -> Option<(f64, f64)> {
                let a = num_list(v, k);
                (a.len() == 2 && a.iter().all(|x| x.is_finite())).then(|| (a[0], a[1]))
            };
            let crop = num_list(v, "crop");
            Ok(ShellEdit::LiveSet(Box::new(uo_docx::session::ops::shell_edits::LiveForm {
                wrap: if field(v, "wrap").is_some() { Some(pts_of_key(v, "wrap")?) } else { None },
                size: pair("size"),
                offset: pair("offset"),
                page_at: pair("pageAt"),
                rot: opt_f64(v, "rot").filter(|r| r.is_finite()),
                crop: (crop.len() == 4 && crop.iter().all(|x| x.is_finite())).then(|| (crop[0], crop[1], crop[2], crop[3])),
            })))
        }
        "liveEnd" => {
            let c = field(v, "commit").ok_or("commit")?;
            let commit = decode_edit(c)?;
            if !matches!(commit, ShellEdit::SetWrapPolygon { .. } | ShellEdit::CommitDrag(_)) {
                return Err("liveEnd commit".to_owned());
            }
            Ok(ShellEdit::LiveEnd(Box::new(commit)))
        }
        "liveCancel" => Ok(ShellEdit::LiveCancel),
        "moveSelected" => Ok(ShellEdit::MoveSelectedBy { dx: f64_of(v, "dx")?, dy: f64_of(v, "dy")?, facts: facts_of(v) }),
        "deleteSelected" => Ok(ShellEdit::DeleteSelectedObjects),
        // wave 7 (A2): ED-204 and ED-202/215
        "moveInline" => Ok(ShellEdit::MoveInlineRun {
            para: u32_of(v, "para")?,
            run: u32_of(v, "run")?,
            to: uo_docx::session::DocPosition::new(u32_of(v, "toPara")?, u32_of(v, "toOffset")?),
        }),
        "commitDrag" => {
            let pair = |k: &str| -> Option<(f64, f64)> {
                let a = num_list(v, k);
                (a.len() == 2).then(|| (a[0], a[1]))
            };
            let crop = num_list(v, "srcRect");
            let at = pair("at").and_then(|(x, y)| frame_of(field(v, "frame")?).map(|f| (x, y, f)));
            Ok(ShellEdit::CommitDrag(Box::new(uo_docx::session::ops::shell_edits::DragCommit {
                rot: opt_f64(v, "rot"),
                size: pair("size"),
                reset_rel: bool_of(v, "resetRel"),
                src_rect: (crop.len() == 4).then(|| (crop[0], crop[1], crop[2], crop[3])),
                at,
                group_start: pair("groupStart"),
            })))
        }
        other => Err(format!("unknown shell edit {other}")),
    }
}

/// Runs a `ShellEdit` given as JSON (see `decode_edit`); answers the `ExecResult` (status 5 when the JSON does
/// not decode).
///
/// # Safety
/// `h` null or live; `(ptr, len)` a buffer of `uo_alloc_bytes(len)` (consumed).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn uo_shell_edit(h: *mut UoWriteSession, ptr: *mut u8, len: usize) -> *mut UoBuf {
    // SAFETY: the caller guarantees the buffer; consumed here.
    let text = String::from_utf8_lossy(&unsafe { take_bytes(ptr, len) }).into_owned();
    // SAFETY: the caller guarantees `h`.
    let Some(s) = (unsafe { session_mut(h) }) else { return buf(NULL_HANDLE, Vec::new()) };
    let Some(v) = uo_docx::read::json_parse::parse(&text) else { return buf(BAD_ARGUMENT, b"json".to_vec()) };
    let e = match decode_edit(&v) {
        Ok(e) => e,
        Err(d) => return buf(BAD_ARGUMENT, d.into_bytes()),
    };
    s.lines_edited();
    let r = s.run(|ss, cx| ss.exec_shell_edit(e, cx));
    exec_result(outcome(r))
}

// ───────────────────────── the previews (covers, watermarks, table styles, Paste as Picture) ─────────────────────────

fn opt_bool(v: &JsonValue, k: &str) -> Option<bool> {
    match field(v, k) {
        Some(JsonValue::Bool(b)) => Some(*b),
        _ => None,
    }
}

fn lang_of(v: &JsonValue) -> uo_docx::session::ops::page::PageLang {
    use uo_docx::session::ops::page::PageLang;
    match str_of(v, "lang").as_deref() {
        Some("pt") => PageLang::Pt,
        Some("es") => PageLang::Es,
        _ => PageLang::En,
    }
}

fn look_of(v: &JsonValue) -> Option<uo_docx::model::props::TblLook> {
    let l = field(v, "look").filter(|l| matches!(l, JsonValue::Object(_)))?;
    Some(uo_docx::model::props::TblLook {
        first_row: opt_bool(l, "firstRow"),
        last_row: opt_bool(l, "lastRow"),
        first_column: opt_bool(l, "firstColumn"),
        last_column: opt_bool(l, "lastColumn"),
        no_h_band: opt_bool(l, "noHBand"),
        no_v_band: opt_bool(l, "noVBand"),
        ..Default::default()
    })
}

/// A `PreviewKind` from `kind` (0 cover, 1 watermark, 2 table style XML, 3 table style by name, 4 the pending Paste as
/// Picture, 5 a header/footer gallery item) and its JSON arguments (`{ id, lang }`, `{ xml, look, lang }`, `{ name, look }`,
/// `{}`, `{ gallery, id, lang, section }`).
fn preview_kind(kind: u32, v: &JsonValue) -> Option<uo_docx::session::ops::previews::PreviewKind> {
    use uo_docx::session::ops::previews::PreviewKind;
    Some(match kind {
        0 => PreviewKind::Cover { id: str_of(v, "id")?, lang: lang_of(v) },
        1 => PreviewKind::Watermark { id: str_of(v, "id")?, lang: lang_of(v) },
        2 => PreviewKind::TableStyleXml { xml: str_of(v, "xml")?, look: look_of(v), lang: lang_of(v) },
        3 => PreviewKind::TableStyleName { name: str_of(v, "name")?, look: look_of(v) },
        4 => PreviewKind::RenderClip,
        5 => PreviewKind::HeaderFooter {
            gallery: uo_docx::session::ops::header_footer::hf_templates::HfGallery::parse(&str_of(v, "gallery")?)?,
            id: str_of(v, "id")?,
            lang: lang_of(v),
            section: u32_of(v, "section").unwrap_or(0),
        },
        _ => return None,
    })
}

/// The layout of a preview mini-document (uo-docx `Session::preview_document`) for `paintCoverTemplate`,
/// `paintWatermarkTemplate`, `renderTableStyleThumb`/`renderTableStyleXmlThumb` and the Paste as Picture's
/// `renderClipPng`, with the session's fonts and the editor's options without revisions (the TS `{ ...layoutOpts(),
/// revisions: undefined }`; the table thumbnails' `ENGINE_LAYOUT_OPTS` is the same set). An owned layout handle (free
/// it), null with the last error set (nothing to show: `Argument("preview")`). The JSON arguments are consumed.
/// Declared duplicate: the layout pipeline is `uo_shell_fmt_toc_template`'s (shell_format.rs, frontier A1).
///
/// # Safety
/// `h`, `env` null or live; `(ptr, len)` a buffer of `uo_alloc_bytes(len)` (consumed).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn uo_shell_preview(
    h: *mut UoWriteSession,
    env: *const crate::write::UoWriteEnv,
    kind: u32,
    ptr: *mut u8,
    len: usize,
    now_ms: f64,
    tz_offset_min: i32,
) -> *mut crate::write::UoLayout {
    use crate::pipeline::{self, JobError};
    crate::pipeline::clear_last();
    // SAFETY: the caller guarantees the buffer; consumed here.
    let text = String::from_utf8_lossy(&unsafe { take_bytes(ptr, len) }).into_owned();
    // SAFETY: the caller guarantees the handles are null or live.
    let (Some(s), Some(env)) = (unsafe { session_mut(h) }, unsafe { env.as_ref() }) else {
        pipeline::fail(&JobError::Argument("session or environment"));
        return std::ptr::null_mut();
    };
    let v = if text.is_empty() { JsonValue::Null } else { uo_docx::read::json_parse::parse(&text).unwrap_or_default() };
    let Some(k) = preview_kind(kind, &v) else {
        pipeline::fail(&JobError::Argument("preview kind"));
        return std::ptr::null_mut();
    };
    let Some(preview) = s.session.preview_document(&k) else {
        pipeline::fail(&JobError::Argument("preview"));
        return std::ptr::null_mut();
    };
    let Some(base) = crate::write::session_layout_options(env, now_ms, tz_offset_min, 0, &[], &[]) else {
        pipeline::fail(&JobError::Argument("layout options"));
        return std::ptr::null_mut();
    };
    let mut o = super::layout::editor_options(&s.session, &base);
    o.revisions = None;
    let fonts = super::handle::HostFonts::of(env);
    let view = uo_docx::view::DocView::new(&preview.document);
    let laid = (|| -> Result<crate::pipeline::LaidOut, JobError> {
        let provider = super::oracle::font_provider(&fonts, &s.embedded).map_err(JobError::Argument)?;
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

/// The page margins of the pending Paste as Picture (`{ left, right }` twips: the crop of `renderClipPng`), status 1
/// when no picture paste waits.
///
/// # Safety
/// `h` null or live.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn uo_shell_render_clip_margins(h: *mut UoWriteSession) -> *mut UoBuf {
    // SAFETY: the caller guarantees `h`.
    let Some(s) = (unsafe { session_mut(h) }) else { return buf(NULL_HANDLE, Vec::new()) };
    let Some(r) = s.session.render_clip_request() else { return buf(NONE, Vec::new()) };
    let mut out = String::from("{\"left\":");
    push_number(&mut out, r.sect_pr.margin_left_twips.to_number());
    out.push_str(",\"right\":");
    push_number(&mut out, r.sect_pr.margin_right_twips.to_number());
    out.push('}');
    json_buf(out)
}

// ───────────────────────── the text box gallery and the embedded spreadsheet ─────────────────────────

/// `saveSelectionToTextboxGallery`'s model half: `{ xml, name }` (the selected box's `w:drawing` with its content as
/// `serializeBlocks` writes it, its `docPr` name or null); status 1 without a selected text box.
///
/// # Safety
/// `h` null or live.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn uo_shell_textbox_gallery_item(h: *mut UoWriteSession) -> *mut UoBuf {
    // SAFETY: the caller guarantees `h`.
    let Some(s) = (unsafe { session_mut(h) }) else { return buf(NULL_HANDLE, Vec::new()) };
    let Some((xml, name)) = s.session.textbox_gallery_item() else { return buf(NONE, Vec::new()) };
    let mut out = String::from("{\"xml\":");
    push_string(&mut out, &xml);
    out.push_str(",\"name\":");
    match name {
        Some(n) => push_string(&mut out, &n),
        None => out.push_str("null"),
    }
    out.push('}');
    json_buf(out)
}

/// `oleRunAt(e)` at the body position `(para, offset)`: `{ para, run, progId, ext }`, status 1 without an embedded
/// object there.
///
/// # Safety
/// `h` null or live.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn uo_shell_ole_at(h: *mut UoWriteSession, para: u32, offset: u32) -> *mut UoBuf {
    // SAFETY: the caller guarantees `h`.
    let Some(s) = (unsafe { session_mut(h) }) else { return buf(NULL_HANDLE, Vec::new()) };
    let Some(o) = s.session.ole_at(para, offset) else { return buf(NONE, Vec::new()) };
    let mut out = String::from("{\"para\":");
    push_number(&mut out, f64::from(o.para));
    out.push_str(",\"run\":");
    push_number(&mut out, f64::from(o.run));
    out.push_str(",\"progId\":");
    push_string(&mut out, &o.prog_id);
    out.push_str(",\"ext\":");
    push_string(&mut out, &o.ext);
    out.push('}');
    json_buf(out)
}

/// The bytes of the embedded package at `(para, offset)` (status 1 without one).
///
/// # Safety
/// `h` null or live.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn uo_shell_ole_data(h: *mut UoWriteSession, para: u32, offset: u32) -> *mut UoBuf {
    // SAFETY: the caller guarantees `h`.
    let Some(s) = (unsafe { session_mut(h) }) else { return buf(NULL_HANDLE, Vec::new()) };
    let Some(o) = s.session.ole_at(para, offset) else { return buf(NONE, Vec::new()) };
    buf(OK, o.data.to_vec())
}

/// `editEmbeddedObject`'s commit (uo-docx `ShellEdit::ReplaceOle`): the body run `(para, run)` gets the edited package
/// `(dptr, dlen)` and the preview PNG `(pptr, plen)` of `width × height` twips; answers the `ExecResult`.
///
/// # Safety
/// `h` null or live; both buffers of `uo_alloc_bytes` (consumed).
#[unsafe(no_mangle)]
#[allow(clippy::too_many_arguments)]
pub unsafe extern "C" fn uo_shell_replace_ole(
    h: *mut UoWriteSession,
    para: u32,
    run: u32,
    dptr: *mut u8,
    dlen: usize,
    pptr: *mut u8,
    plen: usize,
    width: f64,
    height: f64,
) -> *mut UoBuf {
    // SAFETY: the caller guarantees the buffers; consumed here.
    let data: std::sync::Arc<[u8]> = std::sync::Arc::from(unsafe { take_bytes(dptr, dlen) });
    // SAFETY: as above.
    let png: std::sync::Arc<[u8]> = std::sync::Arc::from(unsafe { take_bytes(pptr, plen) });
    // SAFETY: the caller guarantees `h`.
    let Some(s) = (unsafe { session_mut(h) }) else { return buf(NULL_HANDLE, Vec::new()) };
    s.lines_edited();
    let e = ShellEdit::ReplaceOle { para, run, data, png, width, height };
    let r = s.run(|ss, cx| ss.exec_shell_edit(e, cx));
    exec_result(outcome(r))
}

// ───────────────────────── the header/footer gallery (wave 7, A2) ─────────────────────────

/// `headerFooterTemplateXml(gallery, id)` (universal-write.ts 11534): the XML of a template (`{ gallery, id, lang }` as
/// JSON, consumed), as a JSON string; status 1 for an unknown gallery or id. Draws the random drawing ids as the TS does.
///
/// # Safety
/// `h` null or live; `(ptr, len)` a buffer of `uo_alloc_bytes(len)` (consumed).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn uo_shell_hf_template_xml(h: *mut UoWriteSession, ptr: *mut u8, len: usize) -> *mut UoBuf {
    // SAFETY: the caller guarantees the buffer; consumed here.
    let text = String::from_utf8_lossy(&unsafe { take_bytes(ptr, len) }).into_owned();
    // SAFETY: the caller guarantees `h`.
    let Some(s) = (unsafe { session_mut(h) }) else { return buf(NULL_HANDLE, Vec::new()) };
    let Some(v) = uo_docx::read::json_parse::parse(&text) else { return buf(BAD_ARGUMENT, b"json".to_vec()) };
    let Some(g) = str_of(&v, "gallery").and_then(|g| uo_docx::session::ops::header_footer::hf_templates::HfGallery::parse(&g)) else {
        return buf(NONE, Vec::new());
    };
    let id = str_of(&v, "id").unwrap_or_default();
    let Some(xml) = s.session.header_footer_template_xml(g, &id, lang_of(&v)) else { return buf(NONE, Vec::new()) };
    let mut out = String::new();
    push_string(&mut out, &xml);
    json_buf(out)
}

/// `sectPrAt(sectionIdx)` (universal-write.ts 10985): the JSON of the body section's properties.
///
/// # Safety
/// `h` null or live.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn uo_shell_sect_pr_at(h: *mut UoWriteSession, section: u32) -> *mut UoBuf {
    // SAFETY: the caller guarantees `h`.
    let Some(s) = (unsafe { session_mut(h) }) else { return buf(NULL_HANDLE, Vec::new()) };
    json_buf(s.session.sect_pr_json(section))
}

// ───────────────────────── Paste Link between two sessions (wave 7, A2) ─────────────────────────

/// The identities a Paste Link between two editors needs (F8-D4: `AddLinkSource` on the source session, then
/// `PasteLinkWith` here): `{ me, clip, link }` — this session's clip identity (`"session:generation"`), the identity of
/// the ring's current copy (null without one) and whether that copy can be linked (`pasteSpecialFormats().link` of the
/// TS: a source path, a source range, no cells, no drawing).
///
/// # Safety
/// `h` null or live.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn uo_shell_link_origin(h: *mut UoWriteSession) -> *mut UoBuf {
    // SAFETY: the caller guarantees `h`.
    let Some(s) = (unsafe { session_mut(h) }) else { return buf(NULL_HANDLE, Vec::new()) };
    let (sid, generation) = s.session.clip_identity();
    let mut out = format!("{{\"me\":\"{sid}:{generation}\",\"clip\":");
    let (origin, link) = with_ring(|r| match r.current() {
        Some(c) => (
            c.origin.map(|o| format!("\"{}:{}\"", o.session, o.generation)),
            c.path.as_deref().is_some_and(|p| !p.is_empty()) && c.src.is_some() && c.cells.is_none() && c.drawing.is_none(),
        ),
        None => (None, false),
    });
    out.push_str(origin.as_deref().unwrap_or("null"));
    out.push_str(",\"link\":");
    out.push_str(if link { "true" } else { "false" });
    out.push('}');
    json_buf(out)
}

/// The shape parts of `selectedShape().format` (ED-213): `{ fill, style?, textbox, body?, story?, normalEastAsianFlow? }`
/// of the selected shape or text box (status 1 for a picture, a group or no selection).
///
/// # Safety
/// `h` null or live.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn uo_shell_shape_parts(h: *mut UoWriteSession) -> *mut UoBuf {
    // SAFETY: the caller guarantees `h`.
    let Some(s) = (unsafe { session_mut(h) }) else { return buf(NULL_HANDLE, Vec::new()) };
    match s.session.selected_shape_parts_json() {
        Some(j) => json_buf(j),
        None => buf(NONE, Vec::new()),
    }
}

// ───────────────────────── the custom text watermark's box (wave 7, A2: ED-208) ─────────────────────────

/// `watermarkTextBox(text, font, sizePt)` (universal-write.ts 11901): the box of a text watermark with an explicit size —
/// the text's advance (the face of `font`, else Calibri, of the session's fonts: the sfnt advance at the 600-DPI em) and
/// the line (`(usWinAscent + usWinDescent) / unitsPerEm`, hhea when 0), through `explicitTextBox`. `{ text, font,
/// sizePt }` as JSON (consumed); answers `{ widthPt, heightPt }`.
///
/// # Safety
/// `h`, `env` null or live; `(ptr, len)` a buffer of `uo_alloc_bytes(len)` (consumed).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn uo_shell_watermark_text_box(
    h: *mut UoWriteSession,
    env: *const crate::write::UoWriteEnv,
    ptr: *mut u8,
    len: usize,
) -> *mut UoBuf {
    // SAFETY: the caller guarantees the buffer; consumed here.
    let text = String::from_utf8_lossy(&unsafe { take_bytes(ptr, len) }).into_owned();
    // SAFETY: the caller guarantees the handles are null or live.
    let (Some(s), Some(env)) = (unsafe { session_mut(h) }, unsafe { env.as_ref() }) else { return buf(NULL_HANDLE, Vec::new()) };
    let Some(v) = uo_docx::read::json_parse::parse(&text) else { return buf(BAD_ARGUMENT, b"json".to_vec()) };
    let t = str_of(&v, "text").unwrap_or_default();
    let font = str_of(&v, "font").unwrap_or_default();
    let size = opt_f64(&v, "sizePt").unwrap_or(0.0);
    let fonts = super::handle::HostFonts::of(env);
    let face = super::oracle::font_provider(&fonts, &s.embedded)
        .ok()
        .and_then(|p| p.face(&font, false, false).or_else(|| p.face("Calibri", false, false)));
    let (w, hh) = match face {
        None => (size * f64::from(u32::try_from(t.encode_utf16().count()).unwrap_or(u32::MAX)) * 0.6, size * 1.2),
        Some(a) => {
            let f = a.face();
            let fm = f.metrics();
            let upm = if fm.units_per_em == 0 { 1000.0 } else { f64::from(fm.units_per_em) };
            let dev = (uo_jsnum::math::round((size * 600.0) / 72.0) * 72.0) / 600.0;
            let mut tw = 0.0;
            for ch in t.chars() {
                tw += (f64::from(f.advance_width(f.glyph_id(u32::from(ch)))) * dev * 20.0) / upm;
            }
            let asc = if fm.us_win_ascent == 0 { f64::from(fm.ascender) } else { f64::from(fm.us_win_ascent) };
            let desc = if fm.us_win_descent == 0 { -f64::from(fm.descender) } else { f64::from(fm.us_win_descent) };
            let b = uo_docx::write::emit::background::explicit_text_box(tw / 20.0, size, (asc + desc) / upm);
            (b.width_pt, b.height_pt)
        }
    };
    let mut out = String::from("{\"widthPt\":");
    push_number(&mut out, w);
    out.push_str(",\"heightPt\":");
    push_number(&mut out, hh);
    out.push('}');
    json_buf(out)
}

// ───────────────────────── the picture bullet (wave 7, A2) ─────────────────────────

/// `defineListLevel({ ..., picture })` (universal-write.ts 4010, uo-docx `ShellEdit::DefineListLevelPicture`): the level's
/// format and the picture's facts as JSON `{ format: { bullet?, numFmt?, lvlText?, start?, align?, font?, bold?, italic?,
/// sizePt?, color? }, ext, wPt, hPt, name }` (consumed) and the picture's bytes `(dptr, dlen)` (consumed); answers the
/// `ExecResult` (status 5 on a bad message).
///
/// # Safety
/// `h` null or live; both buffers of `uo_alloc_bytes` (consumed).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn uo_shell_define_list_level_picture(
    h: *mut UoWriteSession,
    ptr: *mut u8,
    len: usize,
    dptr: *mut u8,
    dlen: usize,
) -> *mut UoBuf {
    // SAFETY: the caller guarantees the buffers; consumed here.
    let text = String::from_utf8_lossy(&unsafe { take_bytes(ptr, len) }).into_owned();
    // SAFETY: as above.
    let data: std::sync::Arc<[u8]> = std::sync::Arc::from(unsafe { take_bytes(dptr, dlen) });
    // SAFETY: the caller guarantees `h`.
    let Some(s) = (unsafe { session_mut(h) }) else { return buf(NULL_HANDLE, Vec::new()) };
    let Some(v) = uo_docx::read::json_parse::parse(&text) else { return buf(BAD_ARGUMENT, b"json".to_vec()) };
    let Some(f) = field(&v, "format") else { return buf(BAD_ARGUMENT, b"format".to_vec()) };
    let format = uo_docx::session::ops::lists::ListLevelFormat {
        bullet: str_of(f, "bullet"),
        num_fmt: str_of(f, "numFmt"),
        lvl_text: str_of(f, "lvlText"),
        start: opt_f64(f, "start"),
        align: str_of(f, "align"),
        font: str_of(f, "font"),
        bold: bool_of(f, "bold"),
        italic: bool_of(f, "italic"),
        size_pt: opt_f64(f, "sizePt"),
        color: str_of(f, "color"),
    };
    let pic = uo_docx::session::ops::lists::PicBulletIn {
        data,
        ext: str_of(&v, "ext").unwrap_or_else(|| "png".to_owned()).into(),
        w_pt: opt_f64(&v, "wPt").unwrap_or(72.0),
        h_pt: opt_f64(&v, "hPt").unwrap_or(72.0),
        name: str_of(&v, "name").unwrap_or_else(|| "bullet".to_owned()).into(),
    };
    s.lines_edited();
    let e = ShellEdit::DefineListLevelPicture { format: Box::new(format), pic: Box::new(pic) };
    let r = s.run(|ss, cx| ss.exec_shell_edit(e, cx));
    exec_result(outcome(r))
}
