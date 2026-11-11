//! Copy and cut (D2b-F8): `selectedText`, `plainTextSlice`, `extractSelection`, `listMarkersOf`,
//! `copy`, `clipHtml`, `cut`, `cellsClip`, `tableClipText` (universal-write.ts 4757–4976,
//! 20433–20464). The copy keeps the clip in the ring of the page (`OFFICE_CLIPBOARD`, D2a) and
//! tells the shell what to write to the system clipboard ([`super::CopyOut`]).

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use super::super::super::clipboard::{Clip, ClipCells, ClipOrigin, ClipPara, ClipRun, ClipTable};
use super::super::super::revisions::para_formatting;
use super::super::super::state::Selection;
use super::super::super::text::{DocRange, normalize_range, para_length};
use super::super::super::{ExecCx, Session};
use super::super::context::EditContext;
use super::super::tables::table_ops;
use super::super::tables::{TableCmd, table_at};
use super::clone::{self, Fresh, MediaMap};
use super::{CopyOut, Res, rx};
use crate::model::run::Run;
use crate::model::table::Table;
use crate::text::Utf16;
use crate::word::obj;

fn is_object(r: &Run) -> bool {
    r.ext().is_some_and(|e| e.image.is_some() || e.shape.is_some() || e.textbox.is_some() || e.group.is_some())
}

fn is_note_mark(r: &Run) -> bool {
    r.ext().and_then(|e| e.field.as_deref()).is_some_and(super::super::super::settle::is_note_mark_field)
}

/// `{ ...run.props }` with the symbol keys.
fn clip_props(r: &Run) -> ClipRun {
    ClipRun { props: Arc::clone(&r.props), link: r.rpr_link.clone(), ..ClipRun::default() }
}

/// `clipMark(p)`: the formatting of a copied mark without the drop cap.
pub(crate) fn clip_mark(p: &crate::model::para::ParaProps) -> crate::model::para::ParaProps {
    let mut m = para_formatting(p);
    obj::delete(&mut m, "dropCap");
    m
}

/// `tableClipText(t)`: the text (cells by tab, rows by line break) and the HTML of a table.
#[must_use]
pub fn table_clip_text(t: &Table) -> (Vec<u16>, String) {
    let mut text: Vec<u16> = Vec::new();
    for (ri, row) in t.rows.iter().enumerate() {
        if ri > 0 {
            text.push(10);
        }
        for (ci, c) in row.cells.iter().enumerate() {
            if ci > 0 {
                text.push(9);
            }
            text.extend(rx::newlines_to_spaces(&super::super::tables::formula::cell_text(c)));
        }
    }
    let mut html: Vec<u16> = rx::units("<table border=\"1\" style=\"border-collapse:collapse\">");
    for row in &t.rows {
        html.extend(rx::units("<tr>"));
        for c in &row.cells {
            let span = c.tc_pr.as_ref().and_then(|p| p.grid_span).map(|g| g.to_number());
            html.extend(rx::units("<td"));
            // `(c.tcPr?.gridSpan ?? 1) > 1`
            if let Some(g) = span.filter(|g| *g > 1.0) {
                html.extend(rx::units(&format!(" colspan=\"{}\"", uo_jsnum::format::to_js_string(g))));
            }
            html.extend(rx::units(">"));
            for p in c.content.paras.iter() {
                html.extend(rx::units("<p>"));
                let mut t: Vec<u16> = Vec::new();
                for r in &p.runs {
                    t.extend_from_slice(r.text.units());
                }
                html.extend(rx::esc_html(&t));
                html.extend(rx::units("</p>"));
            }
            html.extend(rx::units("</td>"));
        }
        html.extend(rx::units("</tr>"));
    }
    html.extend(rx::units("</table>"));
    (text, String::from_utf16_lossy(&html))
}

impl Session {
    /// `plainTextSlice(p, from, to)`: note references and note marks are not text; a plugin
    /// inline gives its alternative text.
    fn plain_text_slice(p: &crate::model::para::Paragraph, from: usize, to: usize, out: &mut Vec<u16>) {
        let mut acc = 0usize;
        for run in &p.runs {
            let start = acc;
            let end = acc + run.text.len();
            acc = end;
            let (s, e) = (from.max(start), to.min(end));
            if e <= s {
                continue;
            }
            if run.ext().is_some_and(|x| x.note_ref.is_some()) || is_note_mark(run) {
                continue;
            }
            match run.ext().and_then(|x| x.custom.as_deref()) {
                Some(c) => out.extend_from_slice(c.fallback_text.units()),
                None => out.extend_from_slice(run.text.units().get(s - start..e - start).unwrap_or(&[])),
            }
        }
    }

    /// `selectedText()`: the plain text of the selection (paragraphs by `\n`).
    pub(crate) fn selected_text_units(&self) -> Vec<u16> {
        let Some(paras) = self.paras() else { return Vec::new() };
        if !self.has_selection() {
            return Vec::new();
        }
        let DocRange { start, end } = normalize_range(self.range());
        let mut out = Vec::new();
        for pi in start.para..=end.para {
            let Some(p) = paras.get(pi as usize) else { break };
            if pi > start.para {
                out.push(10);
            }
            let from = if pi == start.para { start.offset as usize } else { 0 };
            let to = if pi == end.para { end.offset as usize } else { para_length(p) };
            Self::plain_text_slice(p, from, to, &mut out);
        }
        out
    }

    /// `listMarkersOf(from, to)`: the number text of each list paragraph of the range (the
    /// `listItemValues` of the whole document, by paragraph identity).
    fn list_markers_of(&mut self, from: u32, to: u32) -> HashMap<u32, Utf16> {
        let mut out = HashMap::new();
        let n = self.paras().map_or(0, crate::pvec::PVec::len);
        let mut any = false;
        let mut i = from;
        while i <= to && !any && (i as usize) < n {
            any = self.para_num_at(i as usize).is_some();
            i += 1;
        }
        if !any {
            return out;
        }
        let vals = crate::word::list_item_values(crate::view::DocView::new(&self.doc), None);
        let Some(paras) = self.paras() else { return out };
        for i in from..=to {
            let Some(p) = paras.get(i as usize) else { break };
            if let Some(m) = vals.get(&p.id)
                && !m.text.is_empty()
            {
                out.insert(i, m.text.clone());
            }
        }
        out
    }

    /// The clip's origin (`doc: this.docToken`).
    pub(crate) fn clip_origin(&self) -> ClipOrigin {
        let (session, generation) = self.clip_identity();
        ClipOrigin { session, generation }
    }

    /// `extractSelection()`: the selection as clip paragraphs (each copied mark with its
    /// formatting, the list number as text), the tables between them (body), the lists and the
    /// paragraph styles used, and where it came from.
    pub(crate) fn extract_selection(&mut self) -> Option<Clip> {
        self.paras()?;
        if !self.has_selection() {
            return None;
        }
        let DocRange { start, end } = normalize_range(self.range());
        let markers = self.list_markers_of(start.para, end.para);
        let mut media_list: Vec<Arc<[u8]>> = Vec::new();
        let footnote_of = |kind: &str| kind != "endnote";
        let mut paras: Vec<ClipPara> = Vec::new();
        {
            let doc = &self.doc;
            let mut mm = MediaMap::ToClip { from: &doc.media, list: &mut media_list, seen: HashMap::new() };
            let story = super::super::context::story_of(doc, &self.ctx)?;
            for pi in start.para..=end.para {
                let Some(p) = story.paras.get(pi as usize) else { break };
                let from = if pi == start.para { start.offset as usize } else { 0 };
                let to = if pi == end.para { end.offset as usize } else { para_length(p) };
                let mut runs: Vec<ClipRun> = Vec::new();
                let mut acc = 0usize;
                for run in &p.runs {
                    let (rs, re) = (acc, acc + run.text.len());
                    acc = re;
                    let (s, e) = (from.max(rs), to.min(re));
                    if e <= s {
                        continue;
                    }
                    if is_object(run) {
                        let mut obj = run.clone();
                        clone::run(&mut obj, &mut Fresh { ids: None, media: &mut mm });
                        runs.push(ClipRun { text: Utf16::new(), obj: Some(Box::new(obj)), ..clip_props(run) });
                        continue;
                    }
                    let ext = run.ext();
                    let custom = ext.and_then(|x| x.custom.as_deref());
                    let note_ref = ext.and_then(|x| x.note_ref.clone());
                    let text = match (custom, &note_ref) {
                        (Some(c), _) => c.fallback_text.clone(),
                        (None, Some(_)) => Utf16::new(),
                        _ => Utf16::from_units(run.text.units().get(s - rs..e - rs).unwrap_or(&[]).to_vec()),
                    };
                    let mut cr = ClipRun { text, ..clip_props(run) };
                    if let Some(nr) = note_ref {
                        // the note's content goes with it (`cloneBlocks(noteBlocks(...))`)
                        let mut note =
                            super::super::notes::note_story(doc, footnote_of(&nr.kind), nr.id.to_number()).cloned().unwrap_or_default();
                        clone::story(&mut note, &mut Fresh { ids: None, media: &mut mm });
                        cr.note = Some(note);
                        cr.note_ref = Some(nr);
                    }
                    if let Some(c) = custom {
                        cr.custom = Some(c.clone());
                    }
                    runs.push(cr);
                }
                let mut cp = ClipPara { runs, ..ClipPara::default() };
                // (the last paragraph's mark goes only when the selection passes it)
                if pi < end.para {
                    cp.mark = Some(Arc::new(clip_mark(&p.props)));
                }
                if from == 0
                    && let Some(mk) = markers.get(&pi)
                {
                    cp.marker = Some(mk.clone());
                }
                paras.push(cp);
            }
        }
        let mut text: Vec<u16> = Vec::new();
        for (i, cp) in paras.iter().enumerate() {
            if i > 0 {
                text.push(10);
            }
            for r in &cp.runs {
                text.extend_from_slice(r.text.units());
            }
        }
        // whole tables between the paragraphs of the selection (the TS `tableAnchors`: the body's,
        // also in a note) go with it
        let in_part = matches!(self.ctx, EditContext::Cell { .. } | EditContext::HeaderFooter { .. } | EditContext::Textbox { .. });
        let mut tables: Vec<ClipTable> = Vec::new();
        if !in_part {
            let anchors: Vec<(u32, Arc<Table>)> = self
                .doc
                .body
                .tables
                .iter()
                .filter(|a| a.before_para > start.para && a.before_para <= end.para)
                .map(|a| (a.before_para, Arc::clone(&a.table)))
                .collect();
            let mut ids = std::mem::take(&mut self.doc.ids);
            for (before, t) in anchors {
                let mut c = table_ops::clone_table(&t, &mut ids);
                let mut mm = MediaMap::ToClip { from: &self.doc.media, list: &mut media_list, seen: HashMap::new() };
                clone::table_media(&mut c, &mut mm);
                tables.push(ClipTable { before: before - start.para, table: Arc::new(c) });
            }
            self.doc.ids = ids;
        }
        // the lists copied take their definition (paste into another document)
        let mut lists: Vec<(f64, Arc<str>)> = Vec::new();
        for cp in &paras {
            let id = cp.mark.as_ref().and_then(|m| m.numbering.as_ref()).and_then(|n| n.num_id).map(|n| n.to_number());
            let Some(id) = id.filter(|x| uo_jsnum::conv::truthy(*x)) else { continue };
            let abs = if id > 0.0 { self.abstract_of(id) } else { None };
            let raw = abs.and_then(|a| super::super::lists::store::abstract_xml(&self.numbering, a)).filter(|r| !r.is_empty());
            if let Some(raw) = raw {
                match lists.iter_mut().find(|(k, _)| *k == id) {
                    Some(e) => e.1 = raw.into(),
                    None => lists.push((id, raw.into())),
                }
            }
        }
        let src = matches!(self.ctx, EditContext::Body).then(|| (self.clip_origin(), DocRange { start, end }));
        let mut style_names: Vec<(crate::js::Str, crate::js::Str)> = Vec::new();
        if self.styles.xml().is_some() {
            let idx = self.style_index();
            for cp in &paras {
                let Some(id) = cp.mark.as_ref().and_then(|m| m.style_id.as_deref()).filter(|s| !s.is_empty()) else { continue };
                let name = idx.styles.iter().find(|s| s.id == id).map(|s| s.name.as_str()).filter(|n| !n.is_empty());
                if let Some(name) = name {
                    match style_names.iter_mut().find(|(k, _)| &**k == id) {
                        Some(e) => e.1 = name.into(),
                        None => style_names.push((id.into(), name.into())),
                    }
                }
            }
        }
        Some(Clip {
            text: Utf16::from_units(text),
            paras,
            tables,
            origin: Some(self.clip_origin()),
            path: self.f8.document_path.clone(),
            lists,
            style_names,
            src,
            cells: None,
            drawing: None,
            media: media_list,
        })
    }

    /// `cellsClip()`: the selected cells as a new table (without the sources); `rows` = whole rows,
    /// `whole` = the whole table.
    pub(crate) fn cells_clip(&mut self, media: &mut Vec<Arc<[u8]>>) -> Option<ClipCells> {
        let ts = self.table_sel.clone()?;
        let t = Arc::new(table_at(&self.doc, ts.anchor, &ts.path)?.clone());
        let m = table_ops::map_table(&t);
        let rect = table_ops::CellRect { r1: i64::from(ts.row0), c1: i64::from(ts.col0), r2: i64::from(ts.row1), c2: i64::from(ts.col1) };
        let r = table_ops::norm_rect(&t, &m, rect);
        let whole = table_ops::whole_rect(&t);
        let mut ids = std::mem::take(&mut self.doc.ids);
        let mut sub = table_ops::clone_table(&t, &mut ids);
        // the TS `Set` of cells: a hash set, not a list (a whole 10,000-row table is 10^5 cells)
        let keep: HashSet<(usize, usize)> = table_ops::cells_in(&m, r).iter().map(|g| (g.ri, g.ci)).collect();
        let r1 = usize::try_from(r.r1).unwrap_or(0);
        let r2 = usize::try_from(r.r2).unwrap_or(0);
        let rows: Vec<crate::model::table::Row> =
            sub.rows.drain(..).enumerate().filter(|(i, _)| *i >= r1 && *i <= r2).map(|(_, x)| x).collect();
        sub.rows = rows
            .into_iter()
            .enumerate()
            .map(|(i, mut row)| {
                let ri = r1 + i;
                let cells = std::mem::take(&mut row.cells);
                row.cells = cells.into_iter().enumerate().filter(|(ci, _)| keep.contains(&(ri, *ci))).map(|(_, c)| c).collect();
                if let Some(tp) = row.tr_pr.as_mut() {
                    let tp = Arc::make_mut(tp);
                    for k in ["gridBefore", "gridAfter", "wBefore", "wAfter"] {
                        obj::delete(tp, k);
                    }
                }
                row
            })
            .collect();
        // a vertical merge cut at the top: the first row restarts
        if let Some(row) = sub.rows.first_mut() {
            for c in &mut row.cells {
                if c.tc_pr.as_ref().is_some_and(|p| p.v_merge.as_deref() == Some("continue"))
                    && let Some(p) = c.tc_pr.as_mut()
                {
                    Arc::make_mut(p).v_merge = Some("restart".into());
                }
            }
        }
        let (c1, c2) = (usize::try_from(r.c1).unwrap_or(0), usize::try_from(r.c2).unwrap_or(0));
        sub.grid_cols_twips = t.grid_cols_twips.iter().enumerate().filter(|(i, _)| *i >= c1 && *i <= c2).map(|(_, x)| *x).collect();
        table_ops::sync_legacy(&mut sub);
        self.doc.ids = ids;
        let mut mm = MediaMap::ToClip { from: &self.doc.media, list: media, seen: HashMap::new() };
        clone::table_media(&mut sub, &mut mm);
        Some(ClipCells {
            table: Arc::new(sub),
            rows: r.c1 == 0 && r.c2 >= whole.c2,
            whole: r.r1 == 0 && r.c1 == 0 && r.r2 >= whole.r2 && r.c2 >= whole.c2,
        })
    }

    /// `clipHtml(clip)`: the paragraphs with their direct formatting, for other programs.
    #[cfg(feature = "export")]
    fn clip_html(&mut self, clip: &Clip) -> Option<String> {
        use crate::model::para::{ParaProps, Paragraph};
        let mut store = crate::model::media::MediaStore::new();
        for b in &clip.media {
            store.add(b);
        }
        let mut s = crate::model::Story::new();
        for cp in &clip.paras {
            let props: ParaProps = cp.mark.as_deref().cloned().unwrap_or_default();
            let runs: Vec<Run> = cp
                .runs
                .iter()
                .map(|r| match &r.obj {
                    Some(o) => (**o).clone(),
                    None => Run { text: r.text.clone(), props: Arc::clone(&r.props), ..Run::default() },
                })
                .collect();
            let mut p = Paragraph::empty(self.doc.ids.para());
            p.runs = runs;
            p.props = Arc::new(props);
            s.paras.push(p);
        }
        let doc = crate::export::ExportDoc {
            title: None,
            body: &s,
            comments: &[],
            footnotes: None,
            endnotes: None,
            media: &store,
            image_data_uris: None,
        };
        Some(crate::export::to_html(&doc, &crate::export::HtmlOptions { full_document: Some(false), title: None, images: None }))
    }

    #[cfg(not(feature = "export"))]
    fn clip_html(&mut self, _clip: &Clip) -> Option<String> {
        None
    }

    fn remember(&mut self, clip: Clip, cx: &mut ExecCx<'_>) -> Arc<Clip> {
        let c = Arc::new(clip);
        cx.clipboard.set_current(Some(Arc::clone(&c)));
        let at = self.env.now();
        cx.clipboard.remember(Arc::clone(&c), at);
        c
    }

    /// Whether the selected object is a floating drawing outside a textbox edit (`fs && !tbEdit &&
    /// (fs.textbox || fs.shape || fs.group || fs.image?.floating)`), and whether it has its XML
    /// (`rawOf(fs)`: only `copy` tests it, the first branch of `cut` does not).
    fn float_object_selected(&self) -> Option<bool> {
        if matches!(self.ctx, EditContext::Textbox { .. }) {
            return None;
        }
        let path = self.sel_path()?;
        let r = self.run_ref(&path)?;
        let floating = r.ext().is_some_and(|e| {
            e.textbox.is_some() || e.shape.is_some() || e.group.is_some() || e.image.as_deref().is_some_and(|i| i.floating == Some(true))
        });
        floating.then(|| super::super::drawing::raw_of(r).is_some())
    }

    /// `copy()`: text and HTML for the system (the shell writes [`super::CopyOut`]) and the RICH
    /// copy of the page (the ring).
    pub(crate) fn copy(&mut self, cx: &mut ExecCx<'_>) {
        self.f8.copied = None;
        // a drawing object selected (not the inline picture, which is text): its copy
        if self.float_object_selected() == Some(true) {
            let Some(path) = self.sel_path() else { return };
            let Some(run) = self.run_ref(&path).cloned() else { return };
            let mut media: Vec<Arc<[u8]>> = Vec::new();
            let mut obj = run;
            {
                let mut mm = MediaMap::ToClip { from: &self.doc.media, list: &mut media, seen: HashMap::new() };
                clone::run(&mut obj, &mut Fresh { ids: None, media: &mut mm });
            }
            let clip = Clip { drawing: Some(obj), origin: Some(self.clip_origin()), media, ..Clip::default() };
            self.remember(clip, cx);
            self.f8.copied = Some(CopyOut { text: Utf16::new(), html: None });
            return;
        }
        // selected cells: the cut table (text by tab + HTML)
        let mut media: Vec<Arc<[u8]>> = Vec::new();
        if let Some(cells) = self.cells_clip(&mut media) {
            let (text, html) = table_clip_text(&cells.table);
            let text = Utf16::from_units(text);
            let clip = Clip { text: text.clone(), cells: Some(cells), origin: Some(self.clip_origin()), media, ..Clip::default() };
            self.remember(clip, cx);
            self.f8.copied = Some(CopyOut { text, html: Some(html) });
            return;
        }
        // a non-contiguous selection: the pieces in order, one paragraph each
        let more = self.extra_ranges();
        if !more.is_empty() {
            let main = self.selection;
            let mut texts: Vec<u16> = Vec::new();
            let mut paras: Vec<ClipPara> = Vec::new();
            let mut media: Vec<Arc<[u8]>> = Vec::new();
            for (i, r) in self.selection_ranges().into_iter().enumerate() {
                self.selection = Selection { anchor: r.start, focus: r.end };
                if i > 0 {
                    texts.push(10);
                }
                texts.extend(self.selected_text_units());
                if let Some(c) = self.extract_selection() {
                    // the pieces' pictures refer to one media list
                    let base = media.len();
                    media.extend(c.media);
                    for mut cp in c.paras {
                        for r in &mut cp.runs {
                            shift_media(r, base);
                        }
                        paras.push(cp);
                    }
                }
            }
            self.selection = main;
            let text = Utf16::from_units(texts);
            let mc = Clip { text: text.clone(), paras, origin: Some(self.clip_origin()), media, ..Clip::default() };
            let html = self.clip_html(&mc);
            self.remember(mc, cx);
            self.f8.copied = Some(CopyOut { text, html });
            return;
        }
        let t = self.selected_text_units();
        let clip = self.extract_selection();
        // (only a note reference or an object selected: no text, but it pastes)
        let has_obj = clip.as_ref().is_some_and(|c| c.paras.iter().any(|p| p.runs.iter().any(|r| r.note_ref.is_some() || r.obj.is_some())));
        if t.is_empty() && !has_obj {
            return;
        }
        let html = match &clip {
            Some(c) => self.clip_html(c),
            None => None,
        };
        match clip {
            Some(c) => {
                self.remember(c, cx);
            }
            None => cx.clipboard.set_current(None),
        }
        self.f8.copied = Some(CopyOut { text: Utf16::from_units(t), html });
    }

    /// `cut()`.
    pub(crate) fn cut(&mut self, cx: &mut ExecCx<'_>) -> Res {
        // what this command writes (nothing when it returns early)
        self.f8.copied = None;
        // a drawing object selected: copy and delete
        if self.float_object_selected().is_some() && !self.locked() {
            self.copy(cx);
            if cx.clipboard.current().is_some_and(|c| c.drawing.is_some()) {
                self.delete_selected_float();
            }
            return self.done_f8();
        }
        // cells: the whole table goes; whole rows go; else the content is cleared (Word)
        if self.table_sel.is_some() && !self.locked() {
            let mut scratch = Vec::new();
            let c = self.cells_clip(&mut scratch);
            self.copy(cx);
            let cmd = match &c {
                Some(c) if c.whole => TableCmd::DeleteTable,
                Some(c) if c.rows => TableCmd::DeleteRow,
                _ => TableCmd::ClearSelectedCells,
            };
            return self.exec_tables(cmd, cx);
        }
        if self.paras().is_none() || !self.has_selection() || self.locked() {
            return self.done_f8();
        }
        self.copy(cx);
        self.begin_step(false, &[]);
        let pos = self.delete_selected_ranges()?;
        self.selection = Selection::caret(pos);
        Ok(super::super::super::command::Outcome::Done(self.finish()))
    }
}

/// Shifts the clip-local media ids of a run by `base` (the pieces of a multi-range copy share one
/// list).
fn shift_media(r: &mut ClipRun, base: usize) {
    if base == 0 {
        return;
    }
    let mut mm = MediaMap::Shift { by: base };
    if let Some(o) = r.obj.as_deref_mut() {
        clone::run(o, &mut Fresh { ids: None, media: &mut mm });
    }
    if let Some(n) = r.note.as_mut() {
        clone::story(n, &mut Fresh { ids: None, media: &mut mm });
    }
}
