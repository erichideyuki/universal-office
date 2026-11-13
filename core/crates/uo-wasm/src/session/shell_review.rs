//! Shell exports of the wave-5 frontier R (the editor switch: review and document; record
//! `core/docs/modules/uo-bindings.md` "Wave 5 — R"): the line geometry of the edit scope that the TS
//! shell's screen overlays read (the field shading and the comment highlight, the spelling squiggles,
//! the search hits, the revision and comment anchors of the balloons), the selection rectangles of an
//! arbitrary range (the extra ranges of a multi-selection) and the document's embedded font families
//! (`knownFontFamilies()`). Conventions as `shell.rs`: `UoBuf` statuses 0 ok, 1 none, 11 null handle.
//!
//! The lines are the session's cached `EditLines` of the layout (`UoWriteSession::lines`): the edit
//! scope only (the body, or the context being edited). Wave 7c (V, ED-304 closed): the overlays that the TS paints over
//! every story (`eng.lines`) read `uo_shell_story_lines`, the lines of every story of a page with their paragraphs.

use uo_docx_layout::hit::{EditLine, EditLines, EditPos, SelRect};

use super::codec as w;
use super::handle::UoWriteSession;
use crate::render::{NONE, NULL_HANDLE, OK, UoBuf, buf};
use crate::write::UoLayout;

fn line_wire(el: &EditLines, para: u32, l: &EditLine) -> w::ShellLine {
    let cs = el.chars(l);
    w::ShellLine {
        para,
        page: l.page,
        x: l.x,
        y: l.y,
        height: l.height,
        baseline: l.baseline,
        width: l.width,
        start: l.start,
        end: l.end,
        xf: l.xf.map(|m| m.to_vec()),
        o: cs.o.to_vec(),
        cx: cs.x.to_vec(),
        cw: cs.w.to_vec(),
    }
}

fn encode_lines(lines: Vec<w::ShellLine>) -> *mut UoBuf {
    let mut out = Vec::new();
    match w::encode_shell_lines(&mut out, &w::Delta::default(), &w::ShellLines { lines }) {
        Ok(()) => buf(OK, out),
        Err(e) => buf(super::handle::BAD_ARGUMENT, format!("encode: {e:?}").into_bytes()),
    }
}

/// The lines of the edit scope on page `page` (every paragraph), in paragraph order.
///
/// # Safety
/// `h`, `layout` null or live.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn uo_shell_page_lines(h: *mut UoWriteSession, layout: *const UoLayout, page: u32) -> *mut UoBuf {
    // SAFETY: the caller guarantees the handles are null or live.
    let (Some(s), Some(l)) = (unsafe { h.as_mut() }, unsafe { layout.as_ref() }) else { return buf(NULL_HANDLE, Vec::new()) };
    let Some(el) = s.lines(l) else { return buf(NONE, Vec::new()) };
    let mut lines = Vec::new();
    for (pi, p) in el.paras.iter().enumerate() {
        let para = u32::try_from(pi).unwrap_or(u32::MAX);
        for ln in p.lines.iter().filter(|ln| ln.page == page) {
            lines.push(line_wire(el, para, ln));
        }
    }
    encode_lines(lines)
}

/// The lines of paragraph `para` of the edit scope (every page).
///
/// # Safety
/// `h`, `layout` null or live.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn uo_shell_para_lines(h: *mut UoWriteSession, layout: *const UoLayout, para: u32) -> *mut UoBuf {
    // SAFETY: the caller guarantees the handles are null or live.
    let (Some(s), Some(l)) = (unsafe { h.as_mut() }, unsafe { layout.as_ref() }) else { return buf(NULL_HANDLE, Vec::new()) };
    let Some(el) = s.lines(l) else { return buf(NONE, Vec::new()) };
    let Some(p) = el.para(EditPos { para, offset: 0 }) else { return buf(NONE, Vec::new()) };
    let lines = p.lines.iter().map(|ln| line_wire(el, para, ln)).collect();
    encode_lines(lines)
}

/// The selection rectangles of the range `(ap, ao)`–`(fp, fo)` on page `page`, in either direction
/// (normalised first: a backward range answers the forward one's rectangles).
///
/// # Safety
/// `h`, `layout` null or live.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn uo_shell_range_rects(
    h: *mut UoWriteSession,
    layout: *const UoLayout,
    page: u32,
    ap: u32,
    ao: u32,
    fp: u32,
    fo: u32,
) -> *mut UoBuf {
    // SAFETY: the caller guarantees the handles are null or live.
    let (Some(s), Some(l)) = (unsafe { h.as_mut() }, unsafe { layout.as_ref() }) else { return buf(NULL_HANDLE, Vec::new()) };
    let Some(el) = s.lines(l) else { return buf(NONE, Vec::new()) };
    let (a, f) = (EditPos { para: ap, offset: ao }, EditPos { para: fp, offset: fo });
    let (start, end) = if (fp, fo) < (ap, ao) { (f, a) } else { (a, f) };
    let mut rects: Vec<SelRect> = Vec::new();
    if start != end {
        el.selection_rects(page, start, end, &mut rects);
    }
    let m =
        w::SelRects { rects: rects.iter().map(|r| w::SelRect { x: r.x, y: r.y, w: r.w, h: r.h, xf: r.xf.map(|m| m.to_vec()) }).collect() };
    let mut out = Vec::new();
    match w::encode_sel_rects(&mut out, &w::Delta::default(), &m) {
        Ok(()) => buf(OK, out),
        Err(e) => buf(super::handle::BAD_ARGUMENT, format!("encode: {e:?}").into_bytes()),
    }
}

/// The families of the document's embedded fonts (UTF-8, one per line, each once, in document order).
///
/// # Safety
/// `h` null or live.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn uo_shell_embedded_families(h: *const UoWriteSession) -> *mut UoBuf {
    // SAFETY: the caller guarantees `h` is null or live.
    let Some(s) = (unsafe { h.as_ref() }) else { return buf(NULL_HANDLE, Vec::new()) };
    let mut seen: Vec<&str> = Vec::new();
    for f in &s.embedded {
        if !seen.contains(&&*f.family) {
            seen.push(&f.family);
        }
    }
    buf(OK, seen.join("\n").into_bytes())
}

// ───────────────────────── wave 7c (V, ED-304): the lines of every story ─────────────────────────

/// A paragraph of the [`uo_shell_story_lines`] answer: its key, text, runs and bookmark marks, emitted once per answer.
fn story_para_json(
    out: &mut String,
    key: &str,
    p: &uo_docx::model::para::Paragraph,
    fmt: &uo_docx::model::formatting::DocFormatting,
    fallback: &str,
) {
    use uo_docx::session::ops::find::SpellLangs;
    use uo_docx::session::ops::find::engine::FindEnv;
    use uo_jsnum::json::{push_number, push_string};
    let langs = SpellLangs { sheet: Some(&fmt.styles), fallback };
    out.push_str("{\"key\":");
    push_string(out, key);
    let mut text: Vec<u16> = Vec::new();
    for r in &p.runs {
        text.extend_from_slice(r.text.units());
    }
    out.push_str(",\"text\":");
    push_string(out, &String::from_utf16_lossy(&text));
    out.push_str(",\"runs\":[");
    let mut acc = 0usize;
    let mut bms = String::new();
    for (i, r) in p.runs.iter().enumerate() {
        let at = acc;
        acc += r.text.len();
        if i > 0 {
            out.push(',');
        }
        let ext = r.ext();
        let field = ext.is_some_and(|e| e.field.as_deref().is_some_and(|f| !f.is_empty()) || e.field_result.is_some());
        out.push_str("{\"n\":");
        push_number(out, r.text.len() as f64);
        out.push_str(if field { ",\"f\":true" } else { ",\"f\":false" });
        out.push_str(",\"c\":");
        match r.props.comment_ids.as_ref().filter(|v| !v.is_empty()) {
            Some(v) => {
                out.push('[');
                for (k, n) in v.iter().enumerate() {
                    if k > 0 {
                        out.push(',');
                    }
                    push_number(out, n.as_f64().unwrap_or(f64::NAN));
                }
                out.push(']');
            }
            None => out.push_str("null"),
        }
        let spell = !super::review::skip_run(r) && !super::review::no_proof(fmt, r, p);
        out.push_str(if spell { ",\"sp\":true" } else { ",\"sp\":false" });
        out.push_str(",\"lang\":");
        push_string(out, &langs.lang(r, p).unwrap_or_else(|| fallback.to_owned()));
        out.push('}');
        if let Some(b) = ext.and_then(|e| e.bookmark.as_ref()) {
            if !bms.is_empty() {
                bms.push(',');
            }
            bms.push_str("{\"at\":");
            push_number(&mut bms, at as f64);
            bms.push_str(if b.end == Some(true) { ",\"end\":true" } else { ",\"end\":false" });
            bms.push_str(",\"id\":");
            bms.push_str(&uo_docx::js::JsonWriter::stringify(&b.id, None).unwrap_or_else(|| "null".to_owned()));
            bms.push_str(if uo_docx::session::ops::bookmarks::is_hidden_bookmark(&b.name) {
                ",\"hidden\":true}"
            } else {
                ",\"hidden\":false}"
            });
        }
    }
    out.push_str("],\"bm\":[");
    out.push_str(&bms);
    out.push_str("]}");
}

/// One line of the [`uo_shell_story_lines`] answer.
fn story_line_json(
    out: &mut String,
    para: usize,
    kind: &str,
    l: &uo_docx_layout::output::LineGeom,
    cs: uo_docx_layout::hit::CharsView<'_>,
    xf: Option<&[f64; 6]>,
) {
    use uo_jsnum::json::{push_number, push_string};
    out.push_str("{\"p\":");
    push_number(out, para as f64);
    out.push_str(",\"kind\":");
    push_string(out, kind);
    // fix round 2 (V, ED-304): `desc`, the line's natural text descent (the engine's `Line::desc`)
    for (name, v) in [("x", l.x), ("y", l.y), ("height", l.height), ("baseline", l.baseline), ("width", l.width), ("desc", l.desc)] {
        out.push_str(",\"");
        out.push_str(name);
        out.push_str("\":");
        push_number(out, v);
    }
    out.push_str(",\"start\":");
    push_number(out, f64::from(l.start));
    out.push_str(",\"end\":");
    push_number(out, f64::from(l.end));
    if let Some(m) = xf {
        out.push_str(",\"xf\":[");
        for (i, v) in m.iter().enumerate() {
            if i > 0 {
                out.push(',');
            }
            push_number(out, *v);
        }
        out.push(']');
    }
    let len = cs.o.len().min(cs.x.len()).min(cs.w.len());
    out.push_str(",\"o\":[");
    for (k, o) in cs.o.iter().take(len).enumerate() {
        if k > 0 {
            out.push(',');
        }
        push_number(out, f64::from(*o));
    }
    out.push_str("],\"cx\":[");
    for (k, x) in cs.x.iter().take(len).enumerate() {
        if k > 0 {
            out.push(',');
        }
        push_number(out, *x);
    }
    out.push_str("],\"cw\":[");
    for (k, w) in cs.w.iter().take(len).enumerate() {
        if k > 0 {
            out.push(',');
        }
        push_number(out, *w);
    }
    out.push_str("]}");
}

/// The tables before block `blk` of a story (`blk` minus them = the paragraph's index among the story's paragraphs).
fn tables_before(st: &uo_docx::model::story::Story, blk: u32) -> u32 {
    let n = st.tables.iter().enumerate().filter(|(i, a)| u64::from(a.before_para) + (*i as u64) < u64::from(blk)).count();
    u32::try_from(n).unwrap_or(u32::MAX)
}

/// The cell story of a body cell paragraph's model path (`[block, row, cell index, block, …, block]`): every step but
/// the last.
fn cell_story<'a>(doc: &'a uo_docx::model::Document, steps: &[u32]) -> Option<&'a uo_docx::model::story::Story> {
    use uo_docx::model::Block;
    let (&b, mut rest) = steps.split_first()?;
    let Block::Table(mut t) = doc.body.block_at(b as usize)? else { return None };
    loop {
        match rest {
            [r, c, _] => return t.cell(*r as usize, *c as usize).map(|c| &c.content),
            [r, c, nb, tail @ ..] => {
                let Block::Table(nt) = t.cell(*r as usize, *c as usize)?.content.block_at(*nb as usize)? else { return None };
                t = nt;
                rest = tail;
            }
            _ => return None,
        }
    }
}

/// `paragraphAtPath(blocks, ref)` of a text box's content: the ref's segments (`b<k>` then `r, c_grid, b` per table
/// level) as the steps of the content (`[block, row, cell index, block, …]`), `None` when a step does not resolve.
fn walk_story_path<'a>(st: &'a uo_docx::model::story::Story, segs: &[u32]) -> Option<(Vec<u32>, &'a uo_docx::model::para::Paragraph)> {
    use uo_docx::model::Block;
    let (&b, mut rest) = segs.split_first()?;
    let mut out = vec![b];
    let mut cur = st.block_at(b as usize)?;
    while let [r, c, nb, tail @ ..] = rest {
        let Block::Table(t) = cur else { return None };
        let cell = uo_docx::word::refs::cell_at_grid(t, f64::from(*r), f64::from(*c))?;
        let k = t.rows.get(*r as usize)?.cells.iter().position(|x| std::ptr::eq(x, cell))?;
        out.extend_from_slice(&[*r, u32::try_from(k).ok()?, *nb]);
        cur = cell.content.block_at(*nb as usize)?;
        rest = tail;
    }
    match cur {
        Block::Paragraph(p) => Some((out, p)),
        Block::Table(_) => None,
    }
}

/// The lines of EVERY story on page `page` of `layout` — the TS `eng.lines` (`installEngineResult`, universal-write.ts
/// 16515: body, body table cells at any depth, the header/footer parts the page shows, footnotes and endnotes; the kind
/// `other` skipped) — and, after them, the content lines of the page's text boxes (the second half of the TS
/// `paintEngineSpelling`, 16940: in the frame of the box, `xf`). Wave 7c (V, ED-304): the shell's field shading, comment
/// highlight, bookmark brackets, spelling squiggles and search hits read these, so they follow every story whatever the
/// edit scope (the edit-scope lines of `uo_shell_page_lines` stay for the caret and the selection).
///
/// JSON (UTF-8): `{ lines: [{ p, kind: 'body'|'cell'|'hf'|'note'|'tb', x, y, height, baseline, width, start, end, xf?, o,
/// cx, cw }], paras: [{ key, text, runs: [{ n, f, c, sp, lang }], bm: [{ at, end, id, hidden }] }] }` — `p` indexes
/// `paras`; a key is `b<para>` (a body paragraph: its index among the body paragraphs), `c<anchor>/<row>.<col>.<block>…|<k>`
/// (a cell paragraph as the TS search locates it: `col` the grid column, the block of every hop but the last, `k` the
/// paragraph's index among the cell's paragraphs), `h<part>|<steps>`, `n<f|e><id>|<steps>` or `t<drawing>|<steps>`.
/// `runs` are the paragraph's runs as `paraRuns` answers them (`fallback`: the spelling language of a run without one).
///
/// # Safety
/// `h`, `layout` null or live; `fallback`/`fallback_len` a buffer of this module (consumed) or null/0.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn uo_shell_story_lines(
    h: *const UoWriteSession,
    layout: *const UoLayout,
    page: u32,
    fallback: *mut u8,
    fallback_len: usize,
) -> *mut UoBuf {
    use uo_docx_layout::hit::{CharsView, collapse_custom_chars};
    use uo_docx_layout::output::{CharsSoA, LineGeom, LineStory};
    use uo_docx_layout::refs::{LocKind, Locator};
    let fb = if fallback.is_null() || fallback_len == 0 {
        Vec::new()
    } else {
        // SAFETY: the caller guarantees the buffer; consumed here.
        unsafe { crate::render::take_bytes(fallback, fallback_len) }
    };
    let fb = String::from_utf8_lossy(&fb).into_owned();
    let fb = if fb.is_empty() { "pt-BR".to_owned() } else { fb };
    // SAFETY: the caller guarantees the handles are null or live.
    let (Some(s), Some(l)) = (unsafe { h.as_ref() }, unsafe { layout.as_ref() }) else { return buf(NULL_HANDLE, Vec::new()) };
    let geom = crate::write::layout_geom(l);
    let Some(p) = geom.pages.get(page as usize) else { return buf(NONE, Vec::new()) };
    let view = s.session.view();
    let doc = s.session.document();
    let fmt = &*doc.formatting;
    let loc = Locator::new(view);
    let mut keys: Vec<String> = Vec::new();
    let mut paras = String::new();
    let mut lines = String::new();
    let mut index_of = |key: String, para: &uo_docx::model::para::Paragraph, paras: &mut String| -> usize {
        if let Some(i) = keys.iter().position(|k| *k == key) {
            return i;
        }
        if !keys.is_empty() {
            paras.push(',');
        }
        story_para_json(paras, &key, para, fmt, &fb);
        keys.push(key);
        keys.len() - 1
    };
    let path_key = |steps: &[u32]| -> String { steps.iter().map(u32::to_string).collect::<Vec<_>>().join(".") };
    // `collapseCustomChars(l, para)` (installEngineResult): a plugin inline's fallback characters are one character
    let collapsed = |l: &LineGeom, para: &uo_docx::model::para::Paragraph| -> Option<CharsSoA> {
        if !para.runs.iter().any(|r| r.ext().is_some_and(|e| e.custom.is_some())) {
            return None;
        }
        let cs = CharsView::of(p, l);
        let mut own = CharsSoA { o: cs.o.to_vec(), x: cs.x.to_vec(), w: cs.w.to_vec() };
        collapse_custom_chars(&mut own, para);
        Some(own)
    };
    let emit =
        |lines: &mut String, i: usize, kind: &str, line: &LineGeom, para: &uo_docx::model::para::Paragraph, xf: Option<&[f64; 6]>| {
            if !lines.is_empty() {
                lines.push(',');
            }
            match collapsed(line, para) {
                Some(own) => story_line_json(lines, i, kind, line, CharsView { o: &own.o, x: &own.x, w: &own.w }, xf),
                None => story_line_json(lines, i, kind, line, CharsView::of(p, line), xf),
            }
        };
    for line in &p.lines {
        let at = loc.locate(p.section, &line.at);
        if at.kind == LocKind::Other {
            continue;
        }
        let Some(pl) = at.para.as_ref() else { continue };
        let Some(para) = loc.paragraph(pl) else { continue };
        let (kind, key) = match (at.kind, &line.at.story) {
            (LocKind::Body, _) => {
                let b = line.at.path.0.first().copied().unwrap_or(0);
                ("body", format!("b{}", b.saturating_sub(tables_before(&doc.body, b))))
            }
            (LocKind::Cell, _) => {
                let Some((anchor, hops, _)) = super::shell_stories::cell_ref(doc, &line.at.path.0) else { continue };
                let steps = pl.path.steps();
                let (Some(&blk), Some(st)) = (steps.last(), cell_story(doc, steps)) else { continue };
                let k = blk.saturating_sub(tables_before(st, blk));
                let hk: Vec<String> =
                    hops.iter().map(|h| format!("{}.{}.{}", h.row, h.col, h.block.map_or(String::new(), |b| b.to_string()))).collect();
                ("cell", format!("c{anchor}/{}|{k}", hk.join("/")))
            }
            (LocKind::HeaderFooter, _) => match &pl.story {
                uo_docx::view::StoryRef::HeaderFooter(part) => ("hf", format!("h{}|{}", part.0, path_key(pl.path.steps()))),
                _ => continue,
            },
            (LocKind::Note, LineStory::Note(kind, id)) => {
                let k = if matches!(kind, uo_docx::view::NoteKind::Footnote) { 'f' } else { 'e' };
                ("note", format!("n{k}{}|{}", uo_jsnum::format::to_js_string(id.get()), path_key(pl.path.steps())))
            }
            _ => continue,
        };
        let i = index_of(key, para, &mut paras);
        emit(&mut lines, i, kind, line, para, None);
    }
    // the text boxes' content (in the first box of a chain: a follower's lines resolve in its head's content, the TS
    // `textboxChain(tb)[0]`; the head is the box of the chain with the lowest `seq`)
    let mut heads: Vec<(f64, uo_docx::ids::DrawingId, f64)> = Vec::new();
    for pg in &geom.pages {
        for o in pg.objects.as_deref().unwrap_or(&[]) {
            let Some(tb) = o.run.as_ref().and_then(|r| view.run(r)).and_then(|r| r.ext()).and_then(|e| e.textbox.as_deref()) else {
                continue;
            };
            if let Some(link) = &tb.link {
                let (id, seq) = (link.id.to_number(), link.seq.to_number());
                match heads.iter_mut().find(|h| h.0.to_bits() == id.to_bits()) {
                    Some(h) if seq < h.2 => *h = (id, tb.drawing, seq),
                    Some(_) => {}
                    None => heads.push((id, tb.drawing, seq)),
                }
            }
        }
    }
    for o in p.objects.as_deref().unwrap_or(&[]) {
        let Some((range, m)) = &o.content else { continue };
        let Some(tb) = o.run.as_ref().and_then(|r| view.run(r)).and_then(|r| r.ext()).and_then(|e| e.textbox.as_deref()) else { continue };
        let head =
            tb.link.as_ref().and_then(|lk| heads.iter().find(|h| h.0.to_bits() == lk.id.to_number().to_bits())).map_or(tb.drawing, |h| h.1);
        let Some(head_tb) = view.find_textbox(head) else { continue };
        for li in range.clone() {
            let Some(line) = p.content_lines.get(li as usize) else { continue };
            let Some((path, para)) = walk_story_path(&head_tb.content, &line.at.path.0) else { continue };
            let i = index_of(format!("t{}|{}", head.0, path_key(&path)), para, &mut paras);
            emit(&mut lines, i, "tb", line, para, Some(m));
        }
    }
    let mut out = String::with_capacity(lines.len() + paras.len() + 32);
    out.push_str("{\"lines\":[");
    out.push_str(&lines);
    out.push_str("],\"paras\":[");
    out.push_str(&paras);
    out.push_str("]}");
    buf(OK, out.into_bytes())
}
