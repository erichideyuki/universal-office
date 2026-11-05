//! Frontier **W1** (runs, fields and generated paragraphs; `serialize.ts`): `runXml`, `textXml`,
//! `fldCharXml`, `markXml`, `fieldOrRunXml`, `nestedFieldXml`, `fieldCodeXml` (over
//! [`crate::write::matchers::code_parts`] and [`crate::write::matchers::field_code_groups`]),
//! `fieldResultXml`, `revisionRuns` (with `revKey` = `JSON.stringify(r ?? null)` in the save's
//! signature mode), `formulaFieldXml`, `withNoProof`, `genRuns` (hyperlinks regrouped,
//! `linkRelId` through [`crate::write::ctx::Writer::link_rel_id`]), `paraXmlInner` (the
//! path without sources), the custom-inline `w:sdt` (`encodeURIComponent` of the JSON; the
//! JSON text of a model value never holds a lone surrogate, so the TS `URIError` cannot happen).
//!
//! Evaluation order follows the TS template literals: revision ids are taken from
//! [`crate::write::rev::RevState`] where `revisionAttrs`/`wrapRevision` evaluate (the inner XML of
//! a wrapper first, then its own id), and `rPrXml` (which can take ids for an `rPrChange`) runs
//! where the TS calls it.
//!
//! Ported already (the lossless core uses them on every save): [`comment_marks`],
//! [`sect_pr_inner`].

use std::fmt::Write as _;
use std::sync::Arc;

use indexmap::IndexMap;
use uo_jsnum::text as jstext;

use crate::error::DocxError;
use crate::js::{JsValue, JsonWriter, NumKey, SigEncoding};
use crate::model::para::ParaProps;
use crate::model::props::RevisionInfo;
use crate::model::run::{CustomInline, Run};
use crate::model::section::SectPr;
use crate::model::sources::NumSet;
use crate::write::ctx::{WriteCtx, Writer};
use crate::write::js::{esc_attr, esc_text, num_str, or_num, push_esc_attr, rel_num, truthy};
use crate::write::lossless::{HfRefXml, WResult, cols_xml, ln_num_type_xml, pg_num_type_xml};
use crate::write::matchers::{CodePart, code_parts, field_code_groups, find_from, is_word_byte};
use crate::write::rev::RevKind;
use crate::write::sig::SigMode;

/// The deepest `{ … }` nesting of a field code that [`field_code_at`] follows (the TS recursion
/// stops at the engine's stack, a `RangeError` around ten thousand levels; the reader never gives
/// more than its field depth limit, 64).
pub const MAX_FIELD_CODE_DEPTH: u32 = 512;

/// `isMarkRun(r)` (document.ts): a bookmark, or the beginning or end of a field that crosses
/// paragraphs.
#[must_use]
pub fn is_mark_run(r: &Run) -> bool {
    r.ext().is_some_and(|e| e.bookmark.is_some() || e.field_begin.is_some() || e.field_end == Some(true))
}

/// A run with the comment marks around it (`{ run, before, after }`).
#[derive(Debug, Clone)]
pub struct GenItem<'a> {
    pub run: &'a Run,
    pub before: String,
    pub after: String,
}

/// `commentMarks(runs, skip)`: the start before the first run carrying a comment id, the end and
/// the reference after the last; `skip` = ids whose marks are already in the raw XML.
#[must_use]
pub fn comment_marks(runs: &[Run], skip: Option<&NumSet>) -> (Vec<String>, Vec<String>) {
    let mut first_of: IndexMap<NumKey, usize> = IndexMap::new();
    let mut last_of: IndexMap<NumKey, usize> = IndexMap::new();
    let mut ids_text: IndexMap<NumKey, String> = IndexMap::new();
    for (i, r) in runs.iter().enumerate() {
        for id in r.props.comment_ids.iter().flatten() {
            let k = NumKey::new(id.to_number());
            if skip.is_some_and(|s| s.0.contains(&k)) {
                continue;
            }
            first_of.entry(k).or_insert(i);
            // `Map.set` on an existing key keeps its place
            last_of.insert(k, i);
            ids_text.entry(k).or_insert_with(|| num_str(*id));
        }
    }
    let mut before = vec![String::new(); runs.len()];
    let mut after = vec![String::new(); runs.len()];
    for (k, i) in &first_of {
        if let (Some(b), Some(t)) = (before.get_mut(*i), ids_text.get(k)) {
            b.push_str(&format!("<w:commentRangeStart w:id=\"{t}\"/>"));
        }
    }
    for (k, i) in &last_of {
        if let (Some(a), Some(t)) = (after.get_mut(*i), ids_text.get(k)) {
            a.push_str(&format!(
                "<w:commentRangeEnd w:id=\"{t}\"/><w:r><w:rPr><w:rStyle w:val=\"CommentReference\"/></w:rPr><w:commentReference w:id=\"{t}\"/></w:r>"
            ));
        }
    }
    (before, after)
}

// ───────────────────────────── runs ─────────────────────────────

/// `encodeURIComponent(s)`.
pub fn encode_uri_component(s: &str) -> String {
    const HEX: &[u8; 16] = b"0123456789ABCDEF";
    let mut out = String::with_capacity(s.len());
    for &b in s.as_bytes() {
        if b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.' | b'!' | b'~' | b'*' | b'\'' | b'(' | b')') {
            out.push(char::from(b));
        } else {
            out.push('%');
            out.push(char::from(HEX[usize::from(b >> 4)]));
            out.push(char::from(HEX[usize::from(b & 15)]));
        }
    }
    out
}

/// Appends `esc(s)` of serialize.ts (`& < >`).
fn push_esc_text(out: &mut String, s: &str) {
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            c => out.push(c),
        }
    }
}

/// `textXml(text, deleted)`: the text of a run, its tabs, breaks and hyphens as elements.
#[must_use]
pub fn text_xml(text: &str, deleted: bool) -> String {
    let tag = if deleted { "w:delText" } else { "w:t" };
    let mut out = String::with_capacity(text.len() + 32);
    let flush = |out: &mut String, seg: &str| {
        if seg.is_empty() {
            return;
        }
        let space = if jstext::trim(seg).len() != seg.len() { " xml:space=\"preserve\"" } else { "" };
        out.push('<');
        out.push_str(tag);
        out.push_str(space);
        out.push('>');
        push_esc_text(out, seg);
        out.push_str("</");
        out.push_str(tag);
        out.push('>');
    };
    let mut from = 0;
    for (i, c) in text.char_indices() {
        let sep = match c {
            '\t' => "<w:tab/>",
            '\n' => "<w:br/>",
            '\u{c}' => "<w:br w:type=\"page\"/>",
            '\u{b}' => "<w:br w:type=\"column\"/>",
            '\u{2011}' => "<w:noBreakHyphen/>",
            '\u{ad}' => "<w:softHyphen/>",
            '\u{2028}' => "<w:br w:clear=\"all\"/>",
            _ => continue,
        };
        flush(&mut out, &text[from..i]);
        out.push_str(sep);
        from = i + c.len_utf8();
    }
    flush(&mut out, &text[from..]);
    out
}

/// `fldCharXml(rpr, type)`.
fn fld_char_xml(rpr: &str, kind: &str) -> String {
    format!("<w:r>{rpr}<w:fldChar w:fldCharType=\"{kind}\"/></w:r>")
}

/// `/^\s*(FOOTNOTE|ENDNOTE)REF_MARK\s*$/.test(field)`.
pub fn is_note_ref_mark(field: &str) -> bool {
    matches!(jstext::trim(field), "FOOTNOTEREF_MARK" | "ENDNOTEREF_MARK")
}

/// `/^\s*=/.test(field)`.
pub fn is_formula(field: &str) -> bool {
    jstext::trim_start(field).starts_with('=')
}

/// `/^\s*(TC|XE|RD)\b/i.test(field)` (`i` without `u`: only ASCII letters fold onto ASCII).
pub fn is_marker_field(field: &str) -> bool {
    let b = jstext::trim_start(field).as_bytes();
    let Some(head) = b.get(..2) else { return false };
    ["TC", "XE", "RD"].iter().any(|k| head.eq_ignore_ascii_case(k.as_bytes())) && !b.get(2).is_some_and(|&c| is_word_byte(c))
}

/// `/\\h\b/i.test(field)`: the `\h` switch (a hyperlink).
pub fn has_h_switch(field: &str) -> bool {
    let b = field.as_bytes();
    b.iter().enumerate().any(|(i, &c)| {
        c == b'\\' && b.get(i + 1).is_some_and(|&h| h.eq_ignore_ascii_case(&b'h')) && !b.get(i + 2).is_some_and(|&n| is_word_byte(n))
    })
}

/// `markXml(run)`: the zero-width marks of the editor model (bookmark, beginning and end of a field
/// that crosses paragraphs).
fn mark_xml(w: &mut Writer<'_>, run: &Run) -> WResult<Option<String>> {
    let Some(e) = run.ext() else { return Ok(None) };
    if let Some(b) = e.bookmark.as_ref() {
        let id = num_str(b.id);
        return Ok(Some(if b.end == Some(true) {
            format!("<w:bookmarkEnd w:id=\"{id}\"/>")
        } else {
            format!("<w:bookmarkStart w:id=\"{id}\" w:name=\"{}\"/>", esc_attr(&b.name))
        }));
    }
    if let Some(begin) = e.field_begin.as_deref() {
        let rpr = w.rpr_xml(run)?;
        return Ok(Some(
            fld_char_xml(&rpr, "begin")
                + &format!("<w:r>{rpr}<w:instrText xml:space=\"preserve\">{}</w:instrText></w:r>", esc_text(begin))
                + &fld_char_xml(&rpr, "separate"),
        ));
    }
    if e.field_end == Some(true) {
        let rpr = w.rpr_xml(run)?;
        return Ok(Some(fld_char_xml(&rpr, "end")));
    }
    Ok(None)
}

/// The custom inline `w:sdt` (`run.custom`).
fn custom_inline_xml(w: &mut Writer<'_>, run: &Run, custom: &CustomInline) -> WResult<String> {
    let ty: &str = &custom.r#type;
    if ty.is_empty() || !ty.bytes().all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'-')) {
        return Err(DocxError::Write("invalid custom inline type"));
    }
    let Some(json) = JsonWriter::stringify(&custom.data, None) else {
        return Err(DocxError::Write("custom inline data is not serializable"));
    };
    let tag = format!("wordnext:inline:{ty}:{}", encode_uri_component(&json));
    let fallback_text = custom.fallback_text.to_string_lossy();
    let fallback = Run { text: custom.fallback_text.clone(), props: Arc::clone(&run.props), ..Run::default() };
    // `runXml(fallbackRun)`: a run without the rare fields goes straight to the plain text path
    let inner = plain_run_xml(w, &fallback)?;
    Ok(format!(
        "<w:sdt><w:sdtPr><w:alias w:val=\"{}\"/><w:tag w:val=\"{}\"/><w:lock w:val=\"sdtLocked\"/></w:sdtPr><w:sdtContent>{inner}</w:sdtContent></w:sdt>",
        esc_attr(&fallback_text),
        esc_attr(&tag)
    ))
}

/// `runXml(run)`.
pub fn run_xml<'a>(w: &mut Writer<'a>, run: &'a Run) -> WResult<String> {
    if let Some(m) = mark_xml(w, run)? {
        return Ok(m);
    }
    let ext = run.ext();
    if let Some(c) = ext.and_then(|e| e.custom.as_deref()) {
        return custom_inline_xml(w, run, c);
    }
    let image = ext.and_then(|e| e.image.as_deref());
    let textbox = ext.and_then(|e| e.textbox.as_deref());
    let shape = ext.and_then(|e| e.shape.as_deref());
    let group = ext.and_then(|e| e.group.as_deref());
    if w.active()
        && (image.is_some() || textbox.is_some() || shape.is_some() || group.is_some())
        && let Some(x) = w.object_xml(run)?
    {
        return Ok(x);
    }
    let raw_of = |s: Option<&str>| s.is_some_and(|r| !r.is_empty());
    if let Some(img) = image {
        // the original picture: its raw `w:drawing` with the current geometry; only a new one is rebuilt
        if raw_of(img.raw_drawing_xml.as_deref()) {
            let p = super::object::picture_xml(w, img, None)?;
            return Ok(format!("<w:r>{p}</w:r>"));
        }
        if img.ole.is_some() {
            let o = super::object::ole_object_xml(w, run)?;
            return Ok(format!("<w:r>{o}</w:r>"));
        }
        let d = super::object::drawing_xml(w, run)?;
        return Ok(format!("<w:r>{d}</w:r>"));
    }
    // a new or edited box/shape (Format Shape): wps + the VML fallback
    let tb_edited = textbox.is_some_and(|t| t.edited == Some(true) && raw_of(t.raw_drawing_xml.as_deref()));
    let sh_edited = shape.is_some_and(|s| s.edited == Some(true) && raw_of(s.raw_drawing_xml.as_deref()));
    if tb_edited || sh_edited {
        let rpr = w.rpr_xml(run)?;
        let drawing = textbox.map(|t| t.drawing).or(shape.map(|s| s.drawing)).unwrap_or_default();
        let xml = super::object::edited_shape_xml(w, run)?;
        let xml = super::object::once_shape(w, drawing, xml)?;
        return Ok(format!("<w:r>{rpr}{xml}</w:r>"));
    }
    // a new VML shape of the editor (Horizontal Line): the raw `w:pict`
    if let Some(vml) = shape.and_then(|s| s.raw_vml.as_deref()).filter(|v| !v.is_empty()) {
        let rpr = w.rpr_xml(run)?;
        return Ok(format!("<w:r>{rpr}{vml}</w:r>"));
    }
    // a text box: its raw `w:drawing` with the current geometry and only `w:txbxContent` replaced
    // by the paragraphs; without the raw one (a box we created) nothing
    if let Some(tb) = textbox {
        if let Some(raw) = tb.raw_drawing_xml.as_deref().filter(|r| !r.is_empty()) {
            if tb.vml == Some(true) {
                let rpr = w.rpr_xml(run)?;
                let xml = super::object::edited_shape_xml(w, run)?;
                let xml = super::object::once_shape(w, tb.drawing, xml)?;
                return Ok(format!("<w:r>{rpr}{xml}</w:r>"));
            }
            let geo = super::object::patch_drawing_geometry(raw, &super::object::Geometry::of_textbox(tb))?;
            let inner = w.textbox_paras_xml(tb)?;
            let out = crate::write::matchers::replace_txbx_contents(&geo, |_| inner.clone());
            return Ok(format!("<w:r>{out}</w:r>"));
        }
        return Ok(String::new());
    }
    plain_run_xml(w, run)
}

/// `runXml` from `const rpr = rPrXml(run)` on: the note marks, `ptab`, `sym`, math and the text.
fn plain_run_xml(w: &mut Writer<'_>, run: &Run) -> WResult<String> {
    let rpr = w.rpr_xml(run)?;
    let ext = run.ext();
    if let Some(n) = ext.and_then(|e| e.note_ref.as_ref()) {
        // the text of the run is only the place of the mark in the editor (the number is the
        // engine's); a custom mark is the text of the next run
        let tag = if &*n.kind == "footnote" { "w:footnoteReference" } else { "w:endnoteReference" };
        let r = if rpr.is_empty() { "<w:rPr><w:vertAlign w:val=\"superscript\"/></w:rPr>" } else { rpr.as_str() };
        let custom = if n.custom == Some(true) { " w:customMarkFollows=\"1\"" } else { "" };
        return Ok(format!("<w:r>{r}<{tag}{custom} w:id=\"{}\"/></w:r>", num_str(n.id)));
    }
    if let Some(field) = ext.and_then(|e| e.field.as_deref()).filter(|f| !f.is_empty())
        && (run.text.is_empty() || run.text.units() == [0xFFFC])
        && is_note_ref_mark(field)
    {
        let tag = if field.contains("FOOTNOTE") { "footnoteRef" } else { "endnoteRef" };
        return Ok(format!("<w:r>{rpr}<w:{tag}/></w:r>"));
    }
    if w.active()
        && let Some(t) = ext.and_then(|e| e.ptab.as_ref())
    {
        let leader = t.leader.as_deref().filter(|l| !l.is_empty()).map(|l| format!(" w:leader=\"{}\"", esc_attr(l))).unwrap_or_default();
        return Ok(format!(
            "<w:r>{rpr}<w:ptab w:relativeTo=\"{}\" w:alignment=\"{}\"{leader}/></w:r>",
            esc_attr(&t.relative_to),
            esc_attr(&t.alignment)
        ));
    }
    if w.active()
        && let Some(font) = ext.and_then(|e| e.sym_font.as_deref()).filter(|f| !f.is_empty())
        && let [code] = run.text.units()
    {
        let hex = if (0xf000..=0xf0ff).contains(code) { code & 0xff } else { *code };
        return Ok(format!("<w:r>{rpr}<w:sym w:font=\"{}\" w:char=\"{hex:04X}\"/></w:r>", esc_attr(font)));
    }
    if w.active() && (ext.is_some_and(|e| e.math.is_some()) || run.props.is_math == Some(true)) {
        return Ok(format!(
            "<m:oMath xmlns:m=\"http://schemas.openxmlformats.org/officeDocument/2006/math\"><m:r><m:t>{}</m:t></m:r></m:oMath>",
            esc_text(&run.text.to_string_lossy())
        ));
    }
    Ok(format!("<w:r>{rpr}{}</w:r>", text_xml(&run.text.to_string_lossy(), run.props.del.is_some())))
}

// ───────────────────────────── fields ─────────────────────────────

/// `fieldOrRunXml(r)`: a field as `w:fldSimple` with its current result (Word refreshes it); inside
/// a revision (`w:ins`/`w:del` hold only runs) the complex form.
pub fn field_or_run_xml<'a>(w: &mut Writer<'a>, r: &'a Run) -> WResult<String> {
    let ext = r.ext();
    let Some(field) = ext.and_then(|e| e.field.as_deref()).filter(|f| !f.is_empty()) else { return run_xml(w, r) };
    if w.active() && is_note_ref_mark(field) {
        return run_xml(w, r);
    }
    let field_code = ext.and_then(|e| e.field_code.as_deref()).filter(|c| !c.is_empty());
    if r.props.ins.is_none() && r.props.del.is_none() {
        if field_code.is_some() {
            return nested_field_xml(w, r);
        }
        if is_formula(field) {
            return formula_field_xml(w, r);
        }
        // a marker field without a result (TC/XE/RD): begin, instruction and end, no separator, as
        // "Mark Table of Contents Entry" writes it (oracle toc-lab, tc-mark)
        let n_result = ext.and_then(|e| e.field_result.as_deref()).map_or(0, <[Run]>::len);
        if is_marker_field(field) && r.text.is_empty() && n_result == 0 {
            let rpr = w.rpr_xml(r)?;
            return Ok(fld_char_xml(&rpr, "begin")
                + &format!("<w:r>{rpr}<w:instrText xml:space=\"preserve\">{}</w:instrText></w:r>", esc_text(field))
                + &fld_char_xml(&rpr, "end"));
        }
        // Word writes the complex form for a field with a hyperlink (\h) and for a result with more
        // than one formatting; the rest as w:fldSimple (oracle refs-lab, xr-*)
        let result = field_result_xml(w, r)?;
        if has_h_switch(field) || (r.props.src.is_none() && n_result > 1) {
            let rpr = w.rpr_xml(r)?;
            return Ok(fld_char_xml(&rpr, "begin")
                + &format!("<w:r>{rpr}<w:instrText xml:space=\"preserve\">{}</w:instrText></w:r>", esc_text(field))
                + &fld_char_xml(&rpr, "separate")
                + &result
                + &fld_char_xml(&rpr, "end"));
        }
        return Ok(format!("<w:fldSimple w:instr=\"{}\">{result}</w:fldSimple>", esc_attr(field)));
    }
    let rpr = w.rpr_xml(r)?;
    let it = if r.props.del.is_some() { "w:delInstrText" } else { "w:instrText" };
    let code = match field_code {
        Some(c) => field_code_xml(c, Some(field), &rpr, it)?,
        None => format!("<w:r>{rpr}<{it} xml:space=\"preserve\">{}</{it}></w:r>", esc_text(field)),
    };
    let mut out = fld_char_xml(&rpr, "begin") + &code + &fld_char_xml(&rpr, "separate");
    out.push_str(&run_xml(w, r)?);
    out.push_str(&fld_char_xml(&rpr, "end"));
    Ok(out)
}

/// `nestedFieldXml(r)`: a field with a field nested in its code (`Run.fieldCode`): the complex
/// form, the nested ones back as fields (the result of a formula carries `w:noProof`, as Word
/// writes it).
fn nested_field_xml<'a>(w: &mut Writer<'a>, r: &'a Run) -> WResult<String> {
    let rpr = w.rpr_xml(r)?;
    let ext = r.ext();
    let field = ext.and_then(|e| e.field.as_deref()).unwrap_or("");
    let code = ext.and_then(|e| e.field_code.as_deref()).unwrap_or("");
    let formula = is_formula(field);
    let mut out = fld_char_xml(&rpr, "begin") + &field_code_xml(code, Some(field), &rpr, "w:instrText")? + &fld_char_xml(&rpr, "separate");
    if formula {
        out.push_str(&format!("<w:r>{}{}</w:r>", with_no_proof(&rpr), text_xml(&r.text.to_string_lossy(), false)));
    } else {
        out.push_str(&field_result_xml(w, r)?);
    }
    out.push_str(&fld_char_xml(&rpr, "end"));
    Ok(out)
}

/// `fieldCodeXml(code, flat, rpr, it)`: a field code as `w:instrText` and nested fields. `flat` is
/// the instruction with the nested ones replaced by their cached result (`Run.field`): aligned with
/// the code it gives the result of each first-level nested one; the innermost go without a result
/// (Word recalculates them). The pattern `^…$` of the TS is [`field_code_groups`].
pub fn field_code_xml(code: &str, flat: Option<&str>, rpr: &str, it: &str) -> WResult<String> {
    field_code_at(code, flat, rpr, it, 0)
}

fn field_code_at(code: &str, flat: Option<&str>, rpr: &str, it: &str, depth: u32) -> WResult<String> {
    if depth > MAX_FIELD_CODE_DEPTH {
        return Err(DocxError::Write("field code nested too deeply"));
    }
    let parts = code_parts(jstext::trim(code));
    let groups = flat.and_then(|f| field_code_groups(&parts, jstext::trim(f)));
    let mut k = 0;
    let mut out = String::new();
    for p in &parts {
        match p {
            CodePart::Text(t) => out.push_str(&format!("<w:r>{rpr}<{it} xml:space=\"preserve\">{}</{it}></w:r>", esc_text(t))),
            CodePart::Nested(n) => {
                let cached = groups.as_ref().and_then(|g| g.get(k)).copied();
                k += 1;
                out.push_str(&fld_char_xml(rpr, "begin"));
                out.push_str(&field_code_at(&format!(" {} ", jstext::trim(n)), None, rpr, it, depth + 1)?);
                out.push_str(&fld_char_xml(rpr, "separate"));
                if let Some(c) = cached.filter(|c| !c.is_empty()) {
                    out.push_str(&format!("<w:r>{rpr}{}</w:r>", text_xml(c, false)));
                }
                out.push_str(&fld_char_xml(rpr, "end"));
            }
        }
    }
    Ok(out)
}

/// `fieldResultXml(r)`: the result of a field: a read one goes out as the flattened run (with its
/// source `w:rPr`); one created in the editor, by the formatted runs of the result (REF copies the
/// direct formatting of the span; PAGEREF/SEQ carry `w:noProof`, oracle refs-lab).
fn field_result_xml<'a>(w: &mut Writer<'a>, r: &'a Run) -> WResult<String> {
    let result = r.ext().and_then(|e| e.field_result.as_deref()).filter(|x| !x.is_empty());
    match result {
        Some(runs) if r.props.src.is_none() => {
            let mut out = String::new();
            for x in runs {
                out.push_str(&run_xml(w, x)?);
            }
            Ok(out)
        }
        _ => run_xml(w, r),
    }
}

/// `formulaFieldXml(r)`: a formula field (= …) as the complex field Word writes: the result carries
/// `w:noProof`.
fn formula_field_xml(w: &mut Writer<'_>, r: &Run) -> WResult<String> {
    let rpr = w.rpr_xml(r)?;
    let field = r.ext().and_then(|e| e.field.as_deref()).unwrap_or("");
    let wrap = |inner: &str, pr: &str| format!("<w:r>{pr}{inner}</w:r>");
    Ok(wrap("<w:fldChar w:fldCharType=\"begin\"/>", &rpr)
        + &wrap(&format!("<w:instrText xml:space=\"preserve\">{}</w:instrText>", esc_text(field)), &rpr)
        + &wrap("<w:fldChar w:fldCharType=\"separate\"/>", &rpr)
        + &wrap(&text_xml(&r.text.to_string_lossy(), false), &with_no_proof(&rpr))
        + &wrap("<w:fldChar w:fldCharType=\"end\"/>", &rpr))
}

/// The names `withNoProof` puts `w:noProof` before (the schema order of `CT_RPr`).
const NO_PROOF_BEFORE: &[&str] = &[
    "snapToGrid",
    "vanish",
    "webHidden",
    "color",
    "spacing",
    "w",
    "kern",
    "position",
    "sz",
    "szCs",
    "highlight",
    "u",
    "effect",
    "bdr",
    "shd",
    "fitText",
    "vertAlign",
    "rtl",
    "cs",
    "em",
    "lang",
    "eastAsianLayout",
    "specVanish",
    "oMath",
    "rPrChange",
];

/// The `w:` element names of a text in order, each with its byte index (`<w:NAME\b`: the name is the
/// whole run of `\w` characters after the prefix).
fn w_names(s: &str) -> impl Iterator<Item = (usize, &str)> {
    let mut from = 0;
    std::iter::from_fn(move || {
        let i = find_from(s, "<w:", from)?;
        from = i + 1;
        let rest = s.get(i + 3..).unwrap_or("");
        let len = rest.bytes().take_while(|&b| is_word_byte(b)).count();
        Some((i, rest.get(..len).unwrap_or("")))
    })
}

/// `withNoProof(rpr)`: `w:noProof` in its place of the schema inside a `w:rPr` (or a new `w:rPr`).
#[must_use]
pub fn with_no_proof(rpr: &str) -> String {
    if rpr.is_empty() || rpr == "<w:rPr/>" {
        return "<w:rPr><w:noProof/></w:rPr>".to_owned();
    }
    if w_names(rpr).any(|(_, n)| n == "noProof") {
        return rpr.to_owned();
    }
    if let Some((at, _)) = w_names(rpr).find(|(_, n)| NO_PROOF_BEFORE.contains(n)) {
        return format!("{}<w:noProof/>{}", &rpr[..at], &rpr[at..]);
    }
    match rpr.rfind("</w:rPr>") {
        Some(at) => format!("{}<w:noProof/>{}", &rpr[..at], &rpr[at..]),
        // `lastIndexOf` = −1: `slice(0, -1)` and `slice(-1)` around the last code unit
        None => {
            let (head, last) = crate::write::parts::split_last_unit(rpr);
            format!("{head}<w:noProof/>{last}")
        }
    }
}

// ───────────────────────────── generated runs ─────────────────────────────

/// `revKey(a) === revKey(b)` with `revKey(r)` = `JSON.stringify(r ?? null)` in the save's signature mode: a
/// revision never serializes as `null`, so an absent one equals only an absent one.
fn same_rev(mode: SigMode, a: Option<&RevisionInfo>, b: Option<&RevisionInfo>) -> bool {
    let (a, b) = match (a, b) {
        (None, None) => return true,
        (Some(a), Some(b)) => (a, b),
        _ => return false,
    };
    if std::ptr::eq(a, b) {
        return true;
    }
    let key = |r: &RevisionInfo| {
        let mut w = JsonWriter::for_signature(None, SigEncoding { sorted_keys: mode == SigMode::Canonical });
        if r.json(&mut w) { w.finish() } else { "null".to_owned() }
    };
    key(a) == key(b)
}

/// `plain(xs)` of `revisionRuns`: each run with its marks, fields as fields.
fn plain_runs<'a>(w: &mut Writer<'a>, items: &[GenItem<'a>]) -> WResult<String> {
    let mut out = String::new();
    for it in items {
        out.push_str(&it.before);
        out.push_str(&field_or_run_xml(w, it.run)?);
        out.push_str(&it.after);
    }
    Ok(out)
}

/// `dels(xs)` of `revisionRuns`: the `w:del` envelopes (`w:del` inside `w:ins`).
fn del_runs<'a>(w: &mut Writer<'a>, items: &[GenItem<'a>]) -> WResult<String> {
    let mode = w.cx.mode;
    let mut out = String::new();
    let mut i = 0;
    while i < items.len() {
        let d = items[i].run.props.del.as_deref();
        let mut j = i + 1;
        while j < items.len() && same_rev(mode, items[j].run.props.del.as_deref(), d) {
            j += 1;
        }
        let inner = plain_runs(w, &items[i..j])?;
        match d {
            Some(d) => out.push_str(&w.cx.rev.wrap(RevKind::Del, d, &inner)),
            None => out.push_str(&inner),
        }
        i = j;
    }
    Ok(out)
}

/// `revisionRuns(items)`: runs in the revision envelopes, from the props: `w:ins` outside, `w:del`
/// inside (an insertion by another author that was deleted is `<w:ins><w:del>…`, as Word writes it).
pub fn revision_runs<'a>(w: &mut Writer<'a>, items: &[GenItem<'a>]) -> WResult<String> {
    let mode = w.cx.mode;
    let mut out = String::new();
    let mut i = 0;
    while i < items.len() {
        let ins = items[i].run.props.ins.as_deref();
        let mut j = i + 1;
        while j < items.len() && same_rev(mode, items[j].run.props.ins.as_deref(), ins) {
            j += 1;
        }
        let inner = del_runs(w, &items[i..j])?;
        match ins {
            Some(r) => out.push_str(&w.cx.rev.wrap(RevKind::Ins, r, &inner)),
            None => out.push_str(&inner),
        }
        i = j;
    }
    Ok(out)
}

/// `genRuns(items, inWrapper)`: generated runs with the comment marks around them and the
/// hyperlinks regrouped (only in the lossless save: the old path never wrote `w:hyperlink`).
pub fn gen_runs<'a>(w: &mut Writer<'a>, items: &[GenItem<'a>], in_wrapper: bool) -> WResult<String> {
    let linking = w.active() && !in_wrapper;
    let link_of = |k: usize| -> Option<&'a str> {
        if linking { items.get(k).and_then(|it| it.run.props.link.as_deref()).filter(|l| !l.is_empty()) } else { None }
    };
    let mut out = String::new();
    let mut i = 0;
    while i < items.len() {
        let Some(link) = link_of(i) else {
            let mut j = i + 1;
            while j < items.len() && link_of(j).is_none() {
                j += 1;
            }
            out.push_str(&revision_runs(w, &items[i..j])?);
            i = j;
            continue;
        };
        // `sameLink(k)`: from k on, a run of the same link before any run that is not a mark (a
        // zero-width mark in the middle of the link stays inside it). The scan from k ends at the
        // same item for every k up to it, so one scan serves the marks it crossed.
        let mut scan: Option<(usize, bool)> = None;
        let mut same_link = |k: usize| -> bool {
            if let Some((end, res)) = scan
                && k <= end
            {
                return res;
            }
            let mut m = k;
            let res = loop {
                match items.get(m) {
                    None => break false,
                    Some(it) if it.run.props.link.as_deref() == Some(link) => break true,
                    Some(it) if !is_mark_run(it.run) => break false,
                    Some(_) => m += 1,
                }
            };
            scan = Some((m, res));
            res
        };
        let mut j = i;
        while j < items.len() && same_link(j) {
            j += 1;
        }
        let inner = revision_runs(w, &items[i..j])?;
        let tip = items[i].run.props.link_tip.as_deref().filter(|t| !t.is_empty());
        let attr = match link.strip_prefix('#') {
            Some(anchor) => format!(" w:anchor=\"{}\"", esc_attr(anchor)),
            None => w.link_rel_id(link)?.filter(|id| !id.is_empty()).map(|id| format!(" r:id=\"{id}\"")).unwrap_or_default(),
        };
        // w:tooltip (screen tip) before w:history, as Word writes it (oracle refs-lab, hl-*)
        if attr.is_empty() {
            out.push_str(&inner);
        } else {
            let tooltip = tip.map(|t| format!(" w:tooltip=\"{}\"", esc_attr(t))).unwrap_or_default();
            out.push_str(&format!("<w:hyperlink{attr}{tooltip} w:history=\"1\">{inner}</w:hyperlink>"));
        }
        i = j;
    }
    Ok(out)
}

// ───────────────────────────── the paragraph without sources ─────────────────────────────

/// `flag(name, v)` of `paraXmlInner`: `false` is "off" against the inherited value.
fn flag(out: &mut String, name: &str, v: Option<bool>) {
    if let Some(v) = v {
        out.push('<');
        out.push_str(name);
        out.push_str(if v { "/>" } else { " w:val=\"0\"/>" });
    }
}

/// Appends the children of the `w:pPr` of [`para_xml_inner`] in the legacy order (`ppr` of the TS,
/// joined).
fn ppr_children(w: &mut Writer<'_>, pp: &ParaProps, out: &mut String) -> WResult<()> {
    if let Some(s) = pp.style_id.as_deref().filter(|s| !s.is_empty()) {
        out.push_str("<w:pStyle w:val=\"");
        push_esc_attr(out, s);
        out.push_str("\"/>");
    }
    flag(out, "w:keepNext", pp.keep_next);
    flag(out, "w:keepLines", pp.keep_lines);
    // a section break: the sectPr INSIDE the pPr (ends the section at this paragraph)
    if let Some(sb) = pp.section_break.as_deref() {
        let refs: Vec<HfRefXml> = sb
            .header_refs
            .iter()
            .chain(sb.footer_refs.iter())
            .map(|r| HfRefXml { kind: r.kind.clone(), r#type: r.r#type.clone(), rid: r.rid.clone() })
            .collect();
        let _ = write!(out, "<w:sectPr>{}</w:sectPr>", sect_pr_inner(&w.cx, &sb.sect_pr, &refs)?);
    }
    flag(out, "w:pageBreakBefore", pp.page_break_before);
    flag(out, "w:widowControl", pp.widow_control);
    if let Some(tabs) = pp.tab_stops.as_deref().filter(|t| !t.is_empty()) {
        out.push_str("<w:tabs>");
        for t in tabs {
            let _ = write!(out, "<w:tab w:val=\"{}\"", t.align);
            if let Some(l) = t.leader.as_deref().filter(|l| !l.is_empty()) {
                let _ = write!(out, " w:leader=\"{l}\"");
            }
            let _ = write!(out, " w:pos=\"{}\"/>", num_str(t.pos_twips));
        }
        out.push_str("</w:tabs>");
    }
    if let Some(n) = pp.numbering.as_ref() {
        let default = if &*n.kind == "decimal" { 1002.0 } else { 1001.0 };
        let num_id = uo_jsnum::format::to_js_string(or_num(n.num_id, default));
        let _ = write!(out, "<w:numPr><w:ilvl w:val=\"{}\"/><w:numId w:val=\"{num_id}\"/></w:numPr>", num_str(n.ilvl));
    }
    flag(out, "w:suppressLineNumbers", pp.suppress_line_numbers);
    if let Some(b) = pp.borders.as_deref() {
        let mut inner = String::new();
        for (side, e) in
            [("top", &b.top), ("left", &b.left), ("bottom", &b.bottom), ("right", &b.right), ("between", &b.between), ("bar", &b.bar)]
        {
            let Some(e) = e else { continue };
            let _ = write!(inner, "<w:{side} w:val=\"");
            push_esc_attr(&mut inner, &e.style);
            let _ = write!(inner, "\" w:sz=\"{}\" ", num_str(e.size_eighth_pt));
            if side != "bar" {
                let _ = write!(inner, "w:space=\"{}\" ", num_str(e.space_pt));
            }
            inner.push_str("w:color=\"");
            match e.color.as_deref().filter(|c| !c.is_empty()) {
                Some(c) => push_esc_attr(&mut inner, c),
                None => inner.push_str("auto"),
            }
            inner.push_str("\"/>");
        }
        if !inner.is_empty() {
            let _ = write!(out, "<w:pBdr>{inner}</w:pBdr>");
        }
    } else if pp.box_border == Some(true) {
        out.push_str("<w:pBdr>");
        for side in ["top", "left", "bottom", "right"] {
            let _ = write!(out, "<w:{side} w:val=\"single\" w:sz=\"4\" w:space=\"1\" w:color=\"auto\"/>");
        }
        out.push_str("</w:pBdr>");
    }
    if let Some(s) = pp.shading_hex.as_deref().filter(|s| !s.is_empty()) {
        out.push_str("<w:shd w:val=\"clear\" w:color=\"auto\" w:fill=\"");
        push_esc_attr(out, s);
        out.push_str("\"/>");
    }
    flag(out, "w:suppressAutoHyphens", pp.suppress_auto_hyphens);
    if pp.line_twentieths.is_some()
        || pp.space_after_twips.is_some()
        || pp.space_before_twips.is_some()
        || pp.space_before_auto.is_some()
        || pp.space_after_auto.is_some()
    {
        out.push_str("<w:spacing");
        if let Some(v) = pp.space_before_twips {
            let _ = write!(out, " w:before=\"{}\"", num_str(v));
        }
        if let Some(v) = pp.space_before_auto {
            let _ = write!(out, " w:beforeAutospacing=\"{}\"", i32::from(v));
        }
        if let Some(v) = pp.space_after_twips {
            let _ = write!(out, " w:after=\"{}\"", num_str(v));
        }
        if let Some(v) = pp.space_after_auto {
            let _ = write!(out, " w:afterAutospacing=\"{}\"", i32::from(v));
        }
        if let Some(l) = pp.line_twentieths {
            let _ = write!(out, " w:line=\"{}\" w:lineRule=\"auto\"", num_str(l));
        }
        out.push_str("/>");
    }
    if pp.indent_left_twips.is_some() || pp.indent_right_twips.is_some() || pp.indent_first_line_twips.is_some() {
        out.push_str("<w:ind");
        if let Some(v) = pp.indent_left_twips {
            let _ = write!(out, " w:left=\"{}\"", num_str(v));
        }
        if let Some(v) = pp.indent_right_twips {
            let _ = write!(out, " w:right=\"{}\"", num_str(v));
        }
        if let Some(v) = pp.indent_first_line_twips {
            if rel_num(Some(v)) >= 0.0 {
                let _ = write!(out, " w:firstLine=\"{}\"", num_str(v));
            } else {
                let _ = write!(out, " w:hanging=\"{}\"", uo_jsnum::format::to_js_string(-v.to_number()));
            }
        }
        out.push_str("/>");
    }
    if let Some(a) = pp.alignment.as_deref().filter(|a| !a.is_empty() && *a != "left") {
        let _ = write!(out, "<w:jc w:val=\"{a}\"/>");
    }
    if let Some(t) = pp.textbox_tight_wrap.as_deref().filter(|t| !t.is_empty()) {
        let _ = write!(out, "<w:textboxTightWrap w:val=\"{t}\"/>");
    }
    // revisions: the paragraph mark (w:rPr) and the tracked formatting (w:pPrChange, the last)
    let mut mark = String::new();
    if let Some(r) = pp.mark_ins.as_deref() {
        mark.push_str(&w.cx.rev.mark_xml(RevKind::Ins, r));
    }
    if let Some(r) = pp.mark_del.as_deref() {
        mark.push_str(&w.cx.rev.mark_xml(RevKind::Del, r));
    }
    if !mark.is_empty() {
        let _ = write!(out, "<w:rPr>{mark}</w:rPr>");
    }
    if let Some(pf) = pp.pfmt.as_deref() {
        // `paraXmlInner({ runs: [], props: old }, '')` and its `w:pPr` content (empty without one)
        let mut old = String::new();
        ppr_children(w, &pf.old, &mut old)?;
        let attrs = w.cx.rev.attrs_of(&pf.author, pf.date.as_deref(), pf.date_utc.as_deref());
        if old.is_empty() {
            let _ = write!(out, "<w:pPrChange{attrs}><w:pPr/></w:pPrChange>");
        } else {
            let _ = write!(out, "<w:pPrChange{attrs}><w:pPr>{old}</w:pPr></w:pPrChange>");
        }
    }
    Ok(())
}

/// `paraXmlInner(para, runsXml)`: the paragraph without sources.
pub fn para_xml_inner(w: &mut Writer<'_>, pp: &ParaProps, runs_xml: &str) -> WResult<String> {
    let mut ppr = String::new();
    ppr_children(w, pp, &mut ppr)?;
    let mut out = String::with_capacity(ppr.len() + runs_xml.len() + 32);
    out.push_str("<w:p>");
    if !ppr.is_empty() {
        out.push_str("<w:pPr>");
        out.push_str(&ppr);
        out.push_str("</w:pPr>");
    }
    out.push_str(runs_xml);
    out.push_str("</w:p>");
    Ok(out)
}

/// `sectPrInner(s, refs)`: the content of a generated `w:sectPr`.
pub fn sect_pr_inner(cx: &WriteCtx, s: &SectPr, refs: &[HfRefXml]) -> WResult<String> {
    let refs_xml: String = refs.iter().map(|h| format!("<w:{}Reference w:type=\"{}\" r:id=\"{}\"/>", h.kind, h.r#type, h.rid)).collect();
    let orient = if s.page_width_twips.to_number() > s.page_height_twips.to_number() { " w:orient=\"landscape\"" } else { "" };
    let ty =
        s.r#type.as_deref().filter(|t| !t.is_empty() && *t != "nextPage").map(|t| format!("<w:type w:val=\"{t}\"/>")).unwrap_or_default();
    let cols = match s.cols.as_ref() {
        Some(c) => cols_xml(Some(c), None, &cx.limits)?,
        None => String::new(),
    };
    let title_pg = if s.title_pg == Some(true) { "<w:titlePg/>" } else { "" };
    let pg_num_type = pg_num_type_xml(s, "")?.unwrap_or_default();
    let pg_borders = super::table::pg_borders_xml(s.pg_borders.as_ref().and_then(|b| b.as_val()).map(|b| &**b)).unwrap_or_default();
    let notes =
        super::notes::note_props_xml("footnote", s.footnote_pr.as_ref()) + &super::notes::note_props_xml("endnote", s.endnote_pr.as_ref());
    let fn_cols = if truthy(s.footnote_columns) {
        format!("<w15:footnoteColumns w:val=\"{}\"/>", crate::write::js::opt_num_str(s.footnote_columns))
    } else {
        String::new()
    };
    let ln_num = s.ln_num_type.as_ref().map(ln_num_type_xml).unwrap_or_default();
    let v_align = s.v_align.as_deref().filter(|v| !v.is_empty()).map(|v| format!("<w:vAlign w:val=\"{v}\"/>")).unwrap_or_default();
    let n = |x| num_str(x);
    Ok(format!(
        "{refs_xml}{notes}{ty}<w:pgSz w:w=\"{}\" w:h=\"{}\"{orient}/><w:pgMar w:top=\"{}\" w:right=\"{}\" w:bottom=\"{}\" w:left=\"{}\" w:header=\"{}\" w:footer=\"{}\" w:gutter=\"0\"/>{pg_borders}{ln_num}{pg_num_type}{cols}{v_align}{title_pg}{fn_cols}",
        n(s.page_width_twips),
        n(s.page_height_twips),
        n(s.margin_top_twips),
        n(s.margin_right_twips),
        n(s.margin_bottom_twips),
        n(s.margin_left_twips),
        uo_jsnum::format::to_js_string(or_num(s.header_distance_twips, 720.0)),
        uo_jsnum::format::to_js_string(or_num(s.footer_distance_twips, 720.0)),
    ))
}
