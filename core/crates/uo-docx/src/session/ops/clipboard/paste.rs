//! Paste (D2b-F8): `readClipboard` (the model half), `withRich`, `pasteSource`, `pasteOptions`,
//! `repaste`, `pasteData`, `pasteDrawing`, `pasteRich`, `insertRich`, `insertClip`,
//! `charBefore`/`charAfter`, `pasteMark`, `htmlListNumbering`, `freshDrawingRun`, `listPaste`,
//! `pasteTextOnly`, `htmlClipParas`, `htmlImageRun`, `newImageRun`, `insertImageSized`,
//! `pastePicture`, `pasteSpecialFormats`, `pasteSpecial`, `pasteLink`, `addLinkSourceBookmark`,
//! `pasteAllClipboardItems` (universal-write.ts 4984–5757).

use std::collections::HashMap;
use std::sync::Arc;

use super::super::super::clipboard::{Clip, ClipCells, ClipHtmlList, ClipPara, ClipRun, ClipTable};
use super::super::super::command::{ChangeSet, Command, Event, Job, JobResult, Outcome};
use super::super::super::state::Selection;
use super::super::super::text::{CarriedProps, DocPosition, DocRange, normalize_range, without_revisions};
use super::super::super::{ExecCx, Session};
use super::super::context::EditContext;
use super::super::lists::store;
use super::super::pictures::PictureCmd;
use super::super::tables::table_ops;
use super::clone::{self, Fresh, MediaMap};
use super::html::{self, HtmlImage, TitleStyle};
use super::image::image_size;
use super::tree::{self, PasteLimits};
use super::{
    CellsPasteMode, ClipboardCmd, LastPaste, ListPasteMode, PasteData, PasteInput, PasteLayoutIn, PasteMode, PasteOptions, PasteRange,
    PasteSpecialFormat, PicturePaste, RenderRequest, Res, SpecialFormats, rx,
};
use crate::js::{JsNumber, Str};
use crate::model::para::{NumberingProps, ParaProps};
use crate::model::run::{Run, RunExt, RunProps};
use crate::text::Utf16;
use crate::word::obj;

/// The shell's data as a [`PasteInput`]: the paste-tree payload decoded within the default limits;
/// an undecodable payload or an empty `d.html` is no HTML (F8-D2).
pub(crate) fn input_of(text: Utf16, html: Option<&[u8]>, image: Option<super::super::super::command::ClipImage>) -> PasteInput {
    let html = html.and_then(|b| tree::decode_payload(b, &PasteLimits::default()).ok()).filter(|h| !h.raw.is_empty()).map(Arc::new);
    PasteInput { text, html, image }
}

/// `clipText(c)`: the text of a copy for Keep Text Only: the list numbers become text and a tab
/// (the last paragraph's empty piece does not go).
#[must_use]
pub fn clip_text(c: &Clip) -> Vec<u16> {
    let n = c.paras.len();
    let ps = if n > 1 && c.paras.last().is_some_and(|p| p.runs.is_empty()) { c.paras.get(..n - 1).unwrap_or(&[]) } else { &c.paras[..] };
    let mut out = Vec::new();
    for (i, p) in ps.iter().enumerate() {
        if i > 0 {
            out.push(10);
        }
        if let Some(m) = p.marker.as_ref().filter(|m| !m.is_empty()) {
            out.extend_from_slice(m.units());
            out.push(9);
        }
        for r in &p.runs {
            out.extend_from_slice(r.text.units());
        }
    }
    out
}

/// `htmlListLevels(ordered)`: the levels of a pasted HTML list as Word makes them (measured:
/// html-K).
#[must_use]
pub fn html_list_levels(ordered: bool) -> Vec<String> {
    (0..9)
        .map(|i| {
            let ind = 720 * (i + 1);
            let ppr = format!("<w:pPr><w:tabs><w:tab w:val=\"num\" w:pos=\"{ind}\"/></w:tabs><w:ind w:left=\"{ind}\" w:hanging=\"360\"/></w:pPr>");
            if ordered {
                return format!(
                    "<w:lvl w:ilvl=\"{i}\"><w:start w:val=\"1\"/><w:numFmt w:val=\"decimal\"/><w:lvlText w:val=\"%{}.\"/><w:lvlJc w:val=\"left\"/>{ppr}</w:lvl>",
                    i + 1
                );
            }
            let (ch, font) = match i {
                0 => ("\u{F0B7}", "Symbol"),
                1 => ("o", "Courier New"),
                _ => ("\u{F0A7}", "Wingdings"),
            };
            format!(
                "<w:lvl w:ilvl=\"{i}\"><w:start w:val=\"1\"/><w:numFmt w:val=\"bullet\"/><w:lvlText w:val=\"{ch}\"/><w:lvlJc w:val=\"left\"/>{ppr}<w:rPr><w:rFonts w:ascii=\"{font}\" w:hAnsi=\"{font}\" w:hint=\"default\"/><w:sz w:val=\"20\"/></w:rPr></w:lvl>"
            )
        })
        .collect()
}

/// `pasteSpecialFormats()` of the data read.
#[must_use]
pub fn special_formats(d: Option<&PasteData>) -> SpecialFormats {
    let Some(d) = d else { return SpecialFormats::default() };
    use PasteSpecialFormat as F;
    let mut f = Vec::new();
    match &d.rich {
        Some(r) if r.cells.is_none() && r.drawing.is_none() => f.extend([F::Rtf, F::Text, F::Picture, F::Html, F::Unicode]),
        _ if d.input.html.is_some() => f.extend([F::Html, F::Text, F::Unicode, F::Picture]),
        _ if !d.input.text.is_empty() => f.extend([F::Text, F::Unicode]),
        _ => {}
    }
    if d.input.image.is_some() {
        f.push(F::Image);
    }
    let rich = d.rich.as_deref();
    let path = rich.and_then(|r| r.path.as_deref()).filter(|p| !p.is_empty());
    let link = path.is_some() && rich.is_some_and(|r| r.src.is_some() && r.cells.is_none() && r.drawing.is_none());
    SpecialFormats { formats: f, link, source: path.unwrap_or("").into() }
}

/// Appends a later change set to an earlier one (a command that runs several steps, as the TS
/// fires their events in order).
pub(crate) fn merge_cs(acc: &mut Option<ChangeSet>, cs: ChangeSet) {
    match acc {
        None => *acc = Some(cs),
        Some(a) => {
            a.events.extend(cs.events);
            a.stories.extend(cs.stories);
            a.selection = cs.selection;
            a.dirty = cs.dirty;
            a.can_undo = cs.can_undo;
            a.can_redo = cs.can_redo;
            a.formatting |= cs.formatting;
        }
    }
}

/// Puts an earlier change set in front of an outcome.
pub(crate) fn prepend(first: Option<ChangeSet>, out: Outcome) -> Outcome {
    match (first, out) {
        (Some(mut a), Outcome::Done(b)) => {
            let mut acc = Some(std::mem::take(&mut a));
            merge_cs(&mut acc, b);
            Outcome::Done(acc.unwrap_or_default())
        }
        (_, o) => o,
    }
}

fn kind_is_footnote(kind: &str) -> bool {
    kind != "endnote"
}

fn has_link(p: &RunProps) -> bool {
    p.link.as_deref().is_some_and(|l| !l.is_empty())
}

/// The literal `{ ...props, numbering }` of a patch.
fn numbering_props(kind: &str, ilvl: f64, num_id: f64) -> NumberingProps {
    NumberingProps { kind: kind.into(), ilvl: JsNumber::Num(ilvl), num_id: Some(JsNumber::Num(num_id)), ..NumberingProps::default() }
}

/// `{ ...a, ...b }` of run props.
fn spread_props(a: &RunProps, b: &RunProps) -> RunProps {
    obj::spread(a, b)
}

/// The paste context (`PasteCtx`).
pub(crate) struct PasteCtx {
    pub other: bool,
    pub lists: Vec<(f64, Arc<str>)>,
    pub num_map: Vec<(f64, f64)>,
    pub html_lists: Vec<(String, f64)>,
}

impl PasteCtx {
    pub(crate) fn new(other: bool, lists: Vec<(f64, Arc<str>)>) -> PasteCtx {
        PasteCtx { other, lists, num_map: Vec::new(), html_lists: Vec::new() }
    }
}

impl Session {
    fn touched_abs(&mut self, abs: f64) {
        if !self.f2.num_touched.iter().any(|a| *a == abs || (a.is_nan() && abs.is_nan())) {
            self.f2.num_touched.push(abs);
        }
    }

    /// `contentWidthTwips()`: the shell's value, else the print width of the body section (F8-D3).
    pub(crate) fn paste_content_width(&self, layout: &PasteLayoutIn) -> f64 {
        layout.content_width_twips.unwrap_or_else(|| {
            let s = &self.sections.body_sect_pr;
            s.page_width_twips.to_number() - s.margin_left_twips.to_number() - s.margin_right_twips.to_number()
        })
    }

    /// The shell's answer to `ReadClipboard` (`readClipboard()`): `Failed` is the TS `catch` (no
    /// permission: only the internal copy); any other answer reads nothing.
    pub(crate) fn read_result(&self, result: JobResult, cx: &mut ExecCx<'_>) -> Option<PasteData> {
        match result {
            JobResult::Clipboard { text, html_tree, image, .. } => {
                let input = input_of(text.unwrap_or_default(), html_tree.as_deref(), image);
                Some(self.with_rich(input, cx))
            }
            JobResult::Failed => {
                let clip = cx.clipboard.current()?;
                let input = PasteInput { text: clip.text.clone(), ..PasteInput::default() };
                Some(self.with_rich(input, cx))
            }
            _ => None,
        }
    }

    /// `withRich(d)`: the rich copy goes with the data when the system's text is still its own
    /// (Windows normalises `\r\n`), or when the clipboard is empty and the copy is an object or a
    /// note reference.
    pub(crate) fn with_rich(&self, input: PasteInput, cx: &mut ExecCx<'_>) -> PasteData {
        let t = rx::normalize_newlines(input.text.units());
        let rich = cx.clipboard.current().cloned().filter(|c| {
            c.text.units() == t.as_slice()
                || (t.is_empty()
                    && input.html.is_none()
                    && input.image.is_none()
                    && (c.drawing.is_some() || c.paras.iter().any(|p| p.runs.iter().any(|r| r.note_ref.is_some() || r.obj.is_some()))))
        });
        PasteData { input: Arc::new(input), rich }
    }

    fn same_doc(&self, c: &Clip) -> bool {
        c.origin == Some(self.clip_origin())
    }

    /// `pasteOptions(d)`.
    pub(crate) fn paste_options_of(&self, d: Option<&Arc<PasteData>>) -> PasteOptions {
        let Some(d) = d else { return PasteOptions::default() };
        let lp = self.f8.last_paste.as_ref();
        let is_last = lp.is_some_and(|l| Arc::ptr_eq(&l.data, d));
        // after another edit the paste is not redone any more (the button goes, as in Word)
        if is_last && lp.is_some_and(|l| self.history.undo_len() != l.depth + 1) {
            return PasteOptions::default();
        }
        let rich = d.rich.as_deref();
        let modes = if rich.is_some_and(|r| r.cells.is_some() || r.drawing.is_some())
            || (d.input.image.is_some() && d.input.text.is_empty() && rich.is_none())
        {
            vec![PasteMode::Keep]
        } else {
            vec![PasteMode::Keep, PasteMode::Merge, PasteMode::Picture, PasteMode::Text]
        };
        let has_list = rich.is_some_and(|r| r.paras.iter().any(|p| p.mark.as_ref().is_some_and(|m| m.numbering.is_some())));
        let list = if !has_list {
            Vec::new()
        } else if is_last && lp.and_then(|l| l.list).is_some_and(|l| matches!(l, ListPasteMode::Merge | ListPasteMode::NoMerge)) {
            vec![ListPasteMode::Merge, ListPasteMode::NoMerge]
        } else {
            vec![ListPasteMode::Continue, ListPasteMode::New]
        };
        PasteOptions { modes, list, current: if is_last { lp.map(|l| (l.mode, l.list)) } else { None } }
    }

    /// `repaste(mode, list?)`: undo the last paste and paste again with another option.
    pub(crate) fn repaste(&mut self, mode: PasteMode, list: Option<ListPasteMode>, layout: PasteLayoutIn, cx: &mut ExecCx<'_>) -> Res {
        let Some(lp) = self.f8.last_paste.clone() else { return self.done_f8() };
        if self.locked() || self.history.undo_len() != lp.depth + 1 {
            return self.done_f8();
        }
        let cs = self.undo(cx);
        // the undo's change set comes back in front of the paste's: now, or with the session's
        // next `Done` outcome when the paste goes pending (a render, F7's picture insert), is
        // cancelled or fails (F8-D18)
        let mut acc = self.f8.carried.take();
        merge_cs(&mut acc, cs);
        self.f8.carried = acc;
        let out = self.paste_data(lp.data, Some(mode), list, layout, cx)?;
        self.with_f8_carried(Ok(out))
    }

    /// `pasteSource(d)`.
    fn paste_mode_of(&self, d: &PasteData) -> PasteMode {
        let df = self.f8.defaults;
        match &d.rich {
            Some(r) if self.same_doc(r) => df.within,
            Some(_) => df.between,
            None => df.other,
        }
    }

    /// `pasteData(d, mode?, list?)`: pastes the data with the option asked (else the source's
    /// default).
    pub(crate) fn paste_data(
        &mut self,
        d: Arc<PasteData>,
        mode: Option<PasteMode>,
        list: Option<ListPasteMode>,
        layout: PasteLayoutIn,
        cx: &mut ExecCx<'_>,
    ) -> Res {
        if self.paras().is_none() || self.locked() {
            return self.done_f8();
        }
        let mut m = mode.unwrap_or_else(|| self.paste_mode_of(&d));
        let depth = self.history.undo_len();
        let rich = d.rich.clone();
        // the formatting policy: what it does not allow is left out or converted
        let rp = self.policy.as_ref().map(|(_, r)| r.clone());
        let allows = |f: super::super::super::policy::Flag| rp.as_ref().is_none_or(|r| r.allows(f));
        use super::super::super::policy::Flag;
        if rp.is_some() && rich.as_ref().is_some_and(|r| r.cells.is_some()) && !allows(Flag::Tables) && m != PasteMode::Picture {
            m = PasteMode::Text;
        }
        if rp.is_some() && m == PasteMode::Picture && !allows(Flag::Images) {
            return self.done_f8();
        }
        if let Some(r) = &rich
            && let Some(dr) = &r.drawing
            && m != PasteMode::Text
        {
            if rp.is_some() && !self.policy_object_ok(dr) {
                self.f8.last_paste = None;
                return self.done_f8();
            }
            self.paste_drawing(dr, &r.media);
            self.f8.last_paste = None;
            return self.done_f8();
        }
        if let Some(r) = &rich
            && let Some(cells) = &r.cells
            && m != PasteMode::Text
        {
            let table = match &rp {
                Some(rp) => {
                    let mut ids = std::mem::take(&mut self.doc.ids);
                    let c = table_ops::clone_table(&cells.table, &mut ids);
                    self.doc.ids = ids;
                    Arc::new(self.policy_table(c, rp, Some(r)))
                }
                None => Arc::clone(&cells.table),
            };
            let cells = ClipCells { table, rows: cells.rows, whole: cells.whole };
            // `pasteRich`: whole rows in a table go in as new rows; else they replace
            let in_table = self.table_target().is_some();
            let mode = if in_table && cells.rows && self.table_sel.is_none() { CellsPasteMode::Rows } else { CellsPasteMode::Overwrite };
            let (out, _) = self.paste_cells(mode, &cells, Some(&r.media), &layout)?;
            self.f8.last_paste = None;
            return Ok(out);
        }
        if m == PasteMode::Picture {
            return self.paste_picture(d, layout);
        }
        let merge = m == PasteMode::Merge;
        let r: Option<PasteRange> = if m == PasteMode::Text {
            let t = match &rich {
                Some(c) => clip_text(c),
                None => d.input.text.units().to_vec(),
            };
            self.paste_text_only(&t)?
        } else if let Some(rc) = &rich {
            let (paras, tables) = match &rp {
                Some(rp) => self.policy_clip(&rc.paras, rp, Some(rc), &rc.tables),
                None => (rc.paras.clone(), rc.tables.clone()),
            };
            self.insert_clip(&paras, merge, &tables, Some(rc), Some(&rc.media))?
        } else if let Some(h) = d.input.html.as_ref().filter(|h| !rx::is_blank(h.raw.units())) {
            let raw = h.raw.units();
            if allows(Flag::Tables)
                && rx::has_table_tag(raw)
                && !rx::has_block_tag(&rx::strip_tables(raw))
                && let Some(out) = self.paste_html_tables(h, &layout)?
            {
                self.f8.last_paste = None;
                return Ok(out);
            }
            let paras = self.html_clip_paras(h, m, &layout);
            let paras = match &rp {
                Some(rp) => self.policy_clip(&paras, rp, None, &[]).0,
                None => paras,
            };
            self.insert_clip(&paras, merge, &[], None, None)?
        } else if let Some(img) = &d.input.image {
            if !allows(Flag::Images) {
                return self.done_f8();
            }
            self.f8.last_paste = None;
            let cmd = PictureCmd::Insert {
                data: Arc::clone(&img.data),
                ext: img.ext.clone(),
                ole: None,
                max_width_twips: layout.content_width_twips,
            };
            return self.exec_pictures(cmd, cx);
        } else if !d.input.text.is_empty() {
            let t = d.input.text.units().to_vec();
            self.paste_text_only(&t)?
        } else {
            None
        };
        let Some(r) = r else { return self.done_f8() };
        // (HTML lists already come in as new lists)
        let applied = if m != PasteMode::Text && rich.is_some() { self.list_paste(&r, list)? } else { None };
        self.f8.last_paste = Some(LastPaste { data: Arc::clone(&d), mode: m, list: applied, depth, range: r });
        self.sync_numbering();
        let mut cs = self.finish();
        // the Paste Options button comes at the end of what came in
        cs.events.push(Event::Pasted);
        Ok(Outcome::Done(cs))
    }

    /// `freshDrawingRun(src)` / the copy of `pasteDrawing`: a copied object with new ids, its
    /// media in this document.
    fn fresh_object(&mut self, src: &Run, media: Option<&[Arc<[u8]>]>) -> Run {
        let mut run = src.clone();
        let mut ids = std::mem::take(&mut self.doc.ids);
        match media {
            Some(list) => {
                let mut mm = MediaMap::FromClip { list, to: &mut self.doc.media, seen: HashMap::new() };
                clone::run(&mut run, &mut Fresh { ids: Some(&mut ids), media: &mut mm });
            }
            None => {
                let mut mm = MediaMap::Shift { by: 0 };
                clone::run(&mut run, &mut Fresh { ids: Some(&mut ids), media: &mut mm });
            }
        }
        self.doc.ids = ids;
        run
    }

    /// The object of a drawing run (`textbox ?? shape ?? group ?? image`): its raw XML, set by `f`.
    fn with_raw(run: &mut Run, f: impl FnOnce(&str) -> String) -> bool {
        let Some(e) = run.ext.as_deref_mut() else { return false };
        macro_rules! go {
            ($o:expr) => {{
                if let Some(raw) = $o.raw_drawing_xml.as_deref() {
                    let next = f(raw);
                    obj::set(&mut *$o, "rawDrawingXml", |x| x.raw_drawing_xml = Some(next.into()));
                    obj::set(&mut *$o, "edited", |x| x.edited = Some(true));
                }
                return true;
            }};
        }
        if let Some(o) = e.textbox.as_deref_mut() {
            go!(o)
        }
        if let Some(o) = e.shape.as_deref_mut() {
            go!(o)
        }
        if let Some(o) = e.group.as_deref_mut() {
            go!(o)
        }
        if let Some(o) = e.image.as_deref_mut() {
            go!(o)
        }
        false
    }

    /// `freshDrawingRun(src)`: a copied object goes in with a new drawing id.
    fn fresh_drawing_run(&mut self, src: &Run, media: Option<&[Arc<[u8]>]>) -> Option<Run> {
        let mut run = self.fresh_object(src, media);
        let has_raw = super::super::drawing::raw_of(&run).is_some();
        let is_obj = run.ext().is_some_and(|e| e.textbox.is_some() || e.shape.is_some() || e.group.is_some() || e.image.is_some());
        if !is_obj {
            return None;
        }
        if has_raw {
            let id = uo_jsnum::format::to_js_string(self.next_drawing_id());
            Self::with_raw(&mut run, |raw| super::super::drawing::rx::replace_doc_pr_id(raw, &id));
        }
        Some(run)
    }

    /// `pasteDrawing(src)`: a new copy (new drawing id and name) anchored in the caret's paragraph,
    /// selected.
    fn paste_drawing(&mut self, src: &Run, media: &[Arc<[u8]>]) {
        if self.paras().is_none() || self.locked() || matches!(self.ctx, EditContext::Textbox { .. }) {
            return;
        }
        let mut run = self.fresh_object(src, Some(media));
        let id = uo_jsnum::format::to_js_string(self.next_drawing_id());
        Self::with_raw(&mut run, |raw| rx::renumber_doc_pr_name(&super::super::drawing::rx::replace_doc_pr_id(raw, &id), &id));
        self.begin_step(false, &[]);
        if matches!(self.ctx, EditContext::Cell { .. }) {
            self.exit_cell();
        }
        let at = self.selection.focus;
        let Some(p) = self.paras_mut().and_then(|ps| ps.make_mut(at.para as usize)) else {
            self.discard_step();
            return;
        };
        let k = super::super::super::text::insert_index(p, at.offset as usize);
        let k = k.min(p.runs.len());
        p.runs.insert(k, run);
        self.touch_para(at.para as usize);
        let path = vec![super::super::drawing::Step::Para { para: at.para, run: u32::try_from(k).unwrap_or(u32::MAX) }];
        self.set_float_sel(Some(path));
        self.changed();
    }

    /// `charBefore(pos)` / `charAfter(pos)`: one UTF-16 unit of the paragraph ('' at the edge).
    fn char_around(&self, pos: DocPosition, before: bool) -> Vec<u16> {
        let Some(p) = self.paras().and_then(|ps| ps.get(pos.para as usize)) else { return Vec::new() };
        let t = super::super::super::text::para_text(p);
        let at = if before {
            if pos.offset == 0 {
                return Vec::new();
            }
            pos.offset as usize - 1
        } else {
            pos.offset as usize
        };
        t.get(at).map(|u| vec![*u]).unwrap_or_default()
    }

    /// `propsOf(r)` of `insertClip`.
    fn props_of(r: &ClipRun, merge: bool, dest: &CarriedProps, hyper: Option<&Str>) -> CarriedProps {
        if !merge {
            return CarriedProps { props: without_revisions(&r.props), link: r.link.clone() };
        }
        let mut p = dest.props.clone();
        let rp = &r.props;
        obj::set(&mut p, "bold", |x| x.bold = Some(rp.bold == Some(true) || r.block_bold));
        obj::set(&mut p, "italic", |x| x.italic = Some(rp.italic == Some(true)));
        obj::set(&mut p, "underline", |x| x.underline = Some(rp.underline == Some(true)));
        match (rp.underline == Some(true), rp.underline_style.clone()) {
            (true, Some(s)) => obj::set(&mut p, "underlineStyle", |x| x.underline_style = Some(s)),
            _ => obj::set_undefined(&mut p, "underlineStyle", |x| x.underline_style = None),
        }
        if has_link(rp) {
            let l = rp.link.clone();
            obj::set(&mut p, "link", |x| x.link = l);
            if let Some(h) = hyper {
                obj::set(&mut p, "charStyle", |x| x.char_style = Some(h.clone()));
            }
        }
        CarriedProps { props: p, link: dest.link.clone() }
    }

    /// `insertClip(paras, o)`: the copy's paragraphs at the caret, as Word: Keep takes each piece's
    /// formatting and, where the mark came along, the paragraph's; Merge takes the target's with
    /// the emphasis and the lists of the source; a one-line piece gets the missing space between
    /// words (smart cut and paste). One undo step; the range pasted.
    pub(crate) fn insert_clip(
        &mut self,
        paras: &[ClipPara],
        merge: bool,
        tables: &[ClipTable],
        from: Option<&Arc<Clip>>,
        media: Option<&[Arc<[u8]>]>,
    ) -> Result<Option<PasteRange>, super::super::super::command::CommandError> {
        if self.paras().is_none() || paras.is_empty() {
            return Ok(None);
        }
        self.begin_step(false, &[]);
        if self.has_selection() {
            let p0 = self.delete_selection()?;
            self.selection = Selection::caret(p0);
        }
        let mut pos = self.selection.focus;
        let start = pos;
        let base = match self.pending_props.clone() {
            Some(p) => p,
            None => self.current_props(),
        };
        let dest = CarriedProps { props: without_revisions(&base.props), link: base.link };
        let body = matches!(self.ctx, EditContext::Body | EditContext::Note { .. });
        let notes_ok = paras.iter().any(|p| p.runs.iter().any(|r| r.note_ref.is_some())) && self.can_insert_note();
        let other = from.is_some_and(|c| !self.same_doc(c));
        let mut ctx = PasteCtx::new(other, from.map(|c| c.lists.clone()).unwrap_or_default());
        let hyper: Option<Str> = if merge && paras.iter().any(|p| p.runs.iter().any(|r| has_link(&r.props))) {
            self.builtin_style_id("Hyperlink").ok()
        } else {
            None
        };
        let mut marked: Vec<u32> = Vec::new();
        // smart cut and paste: a piece without a mark gets the space between words
        let single: Option<&[ClipRun]> = match paras {
            [p] if p.mark.is_none() => Some(&p.runs),
            _ => None,
        };
        let first = single.and_then(|s| s.iter().find(|r| !r.text.is_empty()));
        if let Some(fr) = first
            && rx::starts_word(fr.text.units())
            && rx::ends_word_or_punct(&self.char_around(pos, true))
        {
            let props = Self::props_of(fr, merge, &dest, hyper.as_ref());
            pos = self.insert_typed(pos, &[32], Some(&props))?;
        }
        let same = from.is_some_and(|c| self.same_doc(c));
        for (i, cp) in paras.iter().enumerate() {
            if i > 0 {
                let Some(prev) = paras.get(i - 1) else { continue };
                self.shift_anchors(pos.para, 1);
                pos = self.split_typed(pos)?;
                // the paragraph the copied mark closes takes its formatting
                let left = self.paras().and_then(|ps| ps.get(pos.para as usize - 1)).map(|p| Arc::clone(&p.props)).unwrap_or_default();
                if let Some(patch) = self.paste_mark(prev, merge, &mut ctx, &left)? {
                    self.set_para_at(pos.para as usize - 1, &patch)?;
                    marked.push(pos.para - 1);
                }
                // a table that was between the copied paragraphs: in front of this one
                for t in tables {
                    if t.before as usize != i || !body {
                        continue;
                    }
                    let mut ids = std::mem::take(&mut self.doc.ids);
                    let mut c = table_ops::clone_table(&t.table, &mut ids);
                    self.doc.ids = ids;
                    if let Some(list) = media {
                        let mut mm = MediaMap::FromClip { list, to: &mut self.doc.media, seen: HashMap::new() };
                        clone::table_media(&mut c, &mut mm);
                    }
                    let anchors = &mut self.doc.body.tables;
                    let at = anchors.iter().position(|a| a.before_para > pos.para).unwrap_or(anchors.len());
                    anchors.insert(at, Arc::new(crate::model::story::TableAnchor { before_para: pos.para, table: Arc::new(c) }));
                    self.touch_structure();
                }
            }
            for run in &cp.runs {
                if let Some(nr) = &run.note_ref {
                    // a new note with a copy of the content; outside the main text Word does not take it
                    if !notes_ok {
                        continue;
                    }
                    let footnote = kind_is_footnote(&nr.kind);
                    let id = super::super::notes::next_note_id(&self.doc, footnote);
                    let mut note = run.note.clone().unwrap_or_default();
                    let mut ids = std::mem::take(&mut self.doc.ids);
                    {
                        let mut shift = MediaMap::Shift { by: 0 };
                        let mut from_clip = media.map(|list| MediaMap::FromClip { list, to: &mut self.doc.media, seen: HashMap::new() });
                        let mm = match from_clip.as_mut() {
                            Some(m) => m,
                            None => &mut shift,
                        };
                        clone::story(&mut note, &mut Fresh { ids: Some(&mut ids), media: mm });
                    }
                    self.doc.ids = ids;
                    super::super::context::set_note_story(&mut self.doc, footnote, id, note);
                    let mut nref = nr.clone();
                    nref.id = JsNumber::Num(id);
                    let mut r = Run {
                        text: Utf16::from("\u{FFFC}"),
                        props: Arc::new(without_revisions(&run.props)),
                        rpr_link: run.link.clone(),
                        ..Run::default()
                    };
                    r.ext = Some(Box::new(RunExt { note_ref: Some(nref), ..RunExt::default() }));
                    pos = self.insert_whole_run(pos, r)?;
                } else if let Some(o) = &run.obj {
                    if let Some(r) = self.fresh_drawing_run(o, media) {
                        pos = self.insert_whole_run(pos, r)?;
                    }
                } else if let Some(c) = run.custom.as_ref().filter(|c| same || self.f8.inline_types.iter().any(|t| **t == *c.r#type)) {
                    // a plugin inline copied from this editor stays inline
                    let p = Self::props_of(run, merge, &dest, hyper.as_ref());
                    let mut r = Run { text: Utf16::from("\u{FFFC}"), props: Arc::new(p.props), rpr_link: p.link, ..Run::default() };
                    r.ext = Some(Box::new(RunExt { custom: Some(Box::new(c.clone())), ..RunExt::default() }));
                    pos = self.insert_whole_run(pos, r)?;
                } else if !run.text.is_empty() {
                    let p = Self::props_of(run, merge, &dest, hyper.as_ref());
                    pos = self.insert_typed(pos, run.text.units(), Some(&p))?;
                }
            }
        }
        let last = single.and_then(|s| s.iter().rev().find(|r| !r.text.is_empty()));
        if let Some(lr) = last
            && rx::ends_word(lr.text.units())
            && rx::starts_word(&self.char_around(pos, false))
        {
            let props = if merge {
                dest.clone()
            } else {
                let l = single.and_then(<[ClipRun]>::last).unwrap_or(lr);
                CarriedProps { props: without_revisions(&l.props), link: l.link.clone() }
            };
            self.insert_typed(pos, &[32], Some(&props))?;
        }
        self.selection = Selection::caret(pos);
        Ok(Some(PasteRange { start, end: pos, first: start.para, last: pos.para, marked }))
    }

    /// `pasteMark(cp, mode, ctx, left)`: the formatting the copied mark gives the paragraph it
    /// closes (`None`: the target's stays). Keep: every key of the target's formatting and of the
    /// mark, with the mark's value (`undefined` clears: U4a).
    fn paste_mark(
        &mut self,
        cp: &ClipPara,
        merge: bool,
        ctx: &mut PasteCtx,
        left: &ParaProps,
    ) -> Result<Option<ParaProps>, super::super::super::command::CommandError> {
        let mut numbering: Option<NumberingProps> = cp.mark.as_ref().and_then(|m| m.numbering.clone());
        if let Some(hl) = &cp.html_list {
            numbering = Some(self.html_list_numbering(*hl, ctx));
        } else if let Some(n) = numbering.clone()
            && let Some(nid) = n.num_id.map(|x| x.to_number()).filter(|x| uo_jsnum::conv::truthy(*x) && *x > 0.0)
            && ctx.other
        {
            // a list of another document: its definition comes along (abstractNum copied, new w:num)
            let id = match ctx.num_map.iter().find(|(k, _)| *k == nid).map(|(_, v)| *v) {
                Some(v) => v,
                None => {
                    let raw = ctx.lists.iter().find(|(k, _)| *k == nid).map(|(_, r)| Arc::clone(r)).filter(|r| !r.is_empty());
                    let abs = raw.map(|r| store::import_abstract(&mut self.numbering, &mut *self.env, &r));
                    let id = match abs {
                        Some(a) => store::add_num(&mut self.numbering, a, ""),
                        None => 0.0,
                    };
                    if let Some(a) = abs {
                        self.touched_abs(a);
                    }
                    ctx.num_map.push((nid, id));
                    id
                }
            };
            numbering = if uo_jsnum::conv::truthy(id) {
                let mut n2 = n;
                obj::set(&mut n2, "numId", |x| x.num_id = Some(JsNumber::Num(id)));
                Some(n2)
            } else {
                None
            };
        }
        if merge {
            return Ok(numbering.map(|n| {
                let mut p = ParaProps::default();
                obj::set(&mut p, "numbering", |x| x.numbering = Some(n));
                p
            }));
        }
        let Some(mark) = cp.mark.as_deref() else { return Ok(None) };
        // Keep: the whole formatting of the mark instead of the target's
        let mut patch = ParaProps::default();
        let left_f = super::super::super::revisions::para_formatting(left);
        let mut keys: Vec<u8> = obj::key_indices(&left_f);
        for k in obj::key_indices(mark) {
            if !keys.contains(&k) {
                keys.push(k);
            }
        }
        for k in &keys {
            put_slot(&mut patch, mark, usize::from(*k));
        }
        match numbering {
            Some(n) => obj::set(&mut patch, "numbering", |x| x.numbering = Some(n)),
            None => obj::set_undefined(&mut patch, "numbering", |x| x.numbering = None),
        }
        Ok(Some(patch))
    }

    /// `htmlListNumbering(l, ctx)`: a pasted HTML list (ul/ol): a new list per top list, as Word.
    pub(crate) fn html_list_numbering(&mut self, l: ClipHtmlList, ctx: &mut PasteCtx) -> NumberingProps {
        let key = format!("{}{}", if l.ordered { "o" } else { "u" }, uo_jsnum::format::to_js_string(l.key));
        let num_id = match ctx.html_lists.iter().find(|(k, _)| *k == key).map(|(_, v)| *v) {
            Some(v) => v,
            None => {
                let policy_lists = self.policy.as_ref().is_some_and(|(_, r)| r.lists.is_some());
                let num_id = if policy_lists {
                    let lib =
                        if l.ordered { super::super::lists::library::NUMBER_LIBRARY } else { super::super::lists::library::BULLET_LIBRARY };
                    let levels = match lib.first() {
                        Some(def) => {
                            let loc = self.list_locale();
                            super::super::lists::library_levels(def, &loc)
                        }
                        None => Vec::new(),
                    };
                    let o = store::ListOpts { multi_level_type: "hybridMultilevel", ..store::ListOpts::default() };
                    store::add_list(&mut self.numbering, &mut *self.env, &levels, &o)
                } else {
                    let o = store::ListOpts { multi_level_type: "multilevel", ..store::ListOpts::default() };
                    store::add_list(&mut self.numbering, &mut *self.env, &html_list_levels(l.ordered), &o)
                };
                if let Some(a) = self.abstract_of(num_id) {
                    self.touched_abs(a);
                }
                ctx.html_lists.push((key, num_id));
                num_id
            }
        };
        numbering_props(if l.ordered { "decimal" } else { "bullet" }, l.level, num_id)
    }

    /// `listPaste(r, mode)`: the list options of a paste (measured: list-*).
    fn list_paste(
        &mut self,
        r: &PasteRange,
        mode: Option<ListPasteMode>,
    ) -> Result<Option<ListPasteMode>, super::super::super::command::CommandError> {
        let num_of = |s: &Session, k: u32| -> f64 {
            s.paras()
                .and_then(|ps| ps.get(k as usize))
                .and_then(|p| p.props.numbering.as_ref())
                .and_then(|n| n.num_id)
                .map_or(0.0, |x| x.to_number())
        };
        let marked: Vec<u32> = r.marked.iter().copied().filter(|k| num_of(self, *k) > 0.0).collect();
        let (Some(&first), Some(&last)) = (marked.first(), marked.last()) else { return Ok(None) };
        let mut ids: Vec<f64> = Vec::new();
        for k in &marked {
            let id = num_of(self, *k);
            if !ids.contains(&id) {
                ids.push(id);
            }
        }
        let n = self.paras().map_or(0, crate::pvec::PVec::len);
        let before = if first > 0 { self.para_num_at(first as usize - 1) } else { None };
        let after = if (last as usize) + 1 < n { self.para_num_at(last as usize + 1) } else { None };
        let inside = before.is_some_and(|b| ids.contains(&b.num_id))
            || (after.is_some_and(|a| ids.contains(&a.num_id)) && !marked.contains(&(last + 1)));
        let m = mode.unwrap_or(if inside { ListPasteMode::Merge } else { ListPasteMode::New });
        if m == ListPasteMode::Merge {
            return Ok(Some(m));
        }
        for id in ids {
            let Some(abs) = self.abstract_of(id) else { continue };
            let mut to = 0.0;
            if m == ListPasteMode::Continue {
                let lvl = self.num_level(id, 0.0);
                let mut k = i64::from(first) - 1;
                while k >= 0 && to == 0.0 {
                    if let Some(q) = self.para_num_at(k as usize)
                        && q.num_id > 0.0
                    {
                        let ql = self.num_level(q.num_id, 0.0);
                        if store::same_level_format(ql.as_ref(), lvl.as_ref()) {
                            to = q.num_id;
                        }
                    }
                    k -= 1;
                }
                if to == 0.0 {
                    continue;
                }
            } else {
                let Some(copy) = store::copy_abstract(&mut self.numbering, &mut *self.env, abs) else { continue };
                self.touched_abs(copy);
                to = store::add_num(&mut self.numbering, copy, "");
            }
            let upto = if m == ListPasteMode::NoMerge { u32::try_from(n.saturating_sub(1)).unwrap_or(u32::MAX) } else { last };
            for k in first..=upto {
                let Some(q) = self.paras().and_then(|ps| ps.get(k as usize)).and_then(|p| p.props.numbering.clone()) else { continue };
                if q.num_id.map(|x| x.to_number()) != Some(id) {
                    continue;
                }
                if k > last && m == ListPasteMode::NoMerge && after.is_none() {
                    break;
                }
                let mut nq = q;
                obj::set(&mut nq, "numId", |x| x.num_id = Some(JsNumber::Num(to)));
                let mut patch = ParaProps::default();
                obj::set(&mut patch, "numbering", |x| x.numbering = Some(nq));
                self.set_para_at(k as usize, &patch)?;
            }
        }
        Ok(Some(m))
    }

    /// `pasteTextOnly(text)`: Keep Text Only: the text with the target's formatting; the last
    /// piece goes into the caret's paragraph (measured: same-T).
    pub(crate) fn paste_text_only(&mut self, text: &[u16]) -> Result<Option<PasteRange>, super::super::super::command::CommandError> {
        if self.paras().is_none() || self.opts.read_view || text.is_empty() {
            return Ok(None);
        }
        self.begin_step(false, &[]);
        if self.has_selection() {
            let pos = self.delete_selection()?;
            self.selection = Selection::caret(pos);
        }
        let parts = rx::split_lines(text);
        let mut pos = self.selection.focus;
        let start = pos;
        for (i, part) in parts.iter().enumerate() {
            if i > 0 {
                self.shift_anchors(pos.para, 1);
                pos = self.split_typed(pos)?;
            }
            if !part.is_empty() {
                pos = self.insert_typed(pos, part, None)?;
            }
        }
        self.selection = Selection::caret(pos);
        Ok(Some(PasteRange { start, end: pos, first: start.para, last: pos.para, marked: Vec::new() }))
    }

    /// `htmlClipParas(html, mode)`: the paragraphs of a pasted HTML by the option: Keep (direct
    /// formatting, the HTML base: Times New Roman 12, automatic spacing; measured html-K), default
    /// (the document's Heading N and Normal (Web); html-0) or Merge (emphasis; html-M).
    pub(crate) fn html_clip_paras(&mut self, h: &super::tree::PasteHtml, mode: PasteMode, layout: &PasteLayoutIn) -> Vec<ClipPara> {
        let src = html::html_to_paras(h);
        let keep = mode == PasteMode::Keep;
        let web: Option<Str> = if mode == PasteMode::Default { self.builtin_style_id("Normal (Web)").ok() } else { None };
        let any_link = src.iter().any(|p| p.runs.iter().any(|r| has_link(&r.props)));
        let hyper: Option<Str> = if any_link && mode == PasteMode::Default { self.builtin_style_id("Hyperlink").ok() } else { None };
        const HEAD_HALFPT: [f64; 6] = [48.0, 36.0, 27.0, 24.0, 20.0, 15.0];
        let policy = self.policy.is_some();
        // with a formatting policy the headings come in through the STYLES, also in Keep
        let styled = mode == PasteMode::Default || policy;
        let defaults = crate::read::plain::default_run_props();
        let mut out: Vec<ClipPara> = Vec::with_capacity(src.len() + 1);
        for hp in &src {
            let mut base = RunProps::default();
            if keep && !policy {
                obj::set(&mut base, "fontAscii", |x| x.font_ascii = Some("Times New Roman".into()));
                let sz = match hp.block.heading {
                    // `HEAD_HALFPT[heading - 1] ?? 24` (headings are 1 to 9)
                    Some(hd) => match uo_jsnum::conv::to_uint32(hd) {
                        k @ 1..=6 if f64::from(k) == hd => HEAD_HALFPT.get(k as usize - 1).copied().unwrap_or(24.0),
                        _ => 24.0,
                    },
                    None => 24.0,
                };
                obj::set(&mut base, "szHalfPt", |x| x.sz_half_pt = Some(JsNumber::Num(sz)));
                if hp.block.heading.is_some() {
                    obj::set(&mut base, "bold", |x| x.bold = Some(true));
                }
            }
            let mut runs: Vec<ClipRun> = Vec::with_capacity(hp.runs.len());
            for r in &hp.runs {
                if let Some(img) = &r.image {
                    let run = self.html_image_run(img, layout);
                    runs.push(ClipRun { props: Arc::new(defaults.clone()), obj: Some(Box::new(run)), ..ClipRun::default() });
                    continue;
                }
                let mut props = spread_props(&spread_props(&defaults, &base), &r.props);
                if keep && has_link(&r.props) {
                    if props.color.is_none() {
                        obj::set(&mut props, "color", |x| x.color = Some("0000FF".into()));
                    }
                    if r.props.underline.is_none() {
                        obj::set(&mut props, "underline", |x| x.underline = Some(true));
                    }
                }
                if !keep
                    && has_link(&r.props)
                    && let Some(hy) = &hyper
                {
                    obj::set(&mut props, "charStyle", |x| x.char_style = Some(hy.clone()));
                }
                runs.push(ClipRun { text: r.text.clone(), props: Arc::new(props), block_bold: r.block_bold, ..ClipRun::default() });
            }
            let mut mark = ParaProps::default();
            if let Some(a) = hp.block.align.filter(|a| *a != "left") {
                obj::set(&mut mark, "alignment", |x| x.alignment = Some(a.into()));
            }
            let auto = |m: &mut ParaProps| {
                obj::set(m, "spaceBeforeTwips", |x| x.space_before_twips = Some(JsNumber::Num(100.0)));
                obj::set(m, "spaceAfterTwips", |x| x.space_after_twips = Some(JsNumber::Num(100.0)));
                obj::set(m, "spaceBeforeAuto", |x| x.space_before_auto = Some(true));
                obj::set(m, "spaceAfterAuto", |x| x.space_after_auto = Some(true));
                obj::set(m, "lineTwentieths", |x| x.line_twentieths = Some(JsNumber::Num(240.0)));
            };
            if keep && !styled {
                auto(&mut mark);
                if let Some(hd) = hp.block.heading {
                    obj::set(&mut mark, "outlineLvl", |x| x.outline_lvl = Some(JsNumber::Num(hd - 1.0)));
                }
            } else if styled {
                let style: Option<Str> = if let Some(hd) = hp.block.heading {
                    Some(self.builtin_style_id(&format!("heading {}", uo_jsnum::format::to_js_string(hd))).unwrap_or_default())
                } else if let Some(st) = hp.block.style {
                    Some(self.builtin_style_id(if st == TitleStyle::Title { "Title" } else { "Subtitle" }).unwrap_or_default())
                } else if hp.block.list.is_some() {
                    auto(&mut mark);
                    None
                } else {
                    web.clone()
                };
                if let Some(s) = style {
                    obj::set(&mut mark, "styleId", |x| x.style_id = Some(s));
                }
            }
            out.push(ClipPara { runs, mark: Some(Arc::new(mark)), marker: None, html_list: hp.block.list });
        }
        // every HTML paragraph is whole (with its mark): the target's comes after
        out.push(ClipPara::default());
        out
    }

    /// `htmlImageRun(img)`: a `data:` picture of a pasted HTML as an inline picture.
    fn html_image_run(&mut self, img: &HtmlImage, layout: &PasteLayoutIn) -> Run {
        let size = image_size(&img.data);
        let mut w = img.w_px.or(size.map(|s| s.0)).unwrap_or(96.0) * 15.0;
        let hh = match (img.h_px, size, img.w_px) {
            (Some(h), _, _) => Some(h),
            (None, Some((sw, sh)), Some(wp)) => Some(sh * wp / sw),
            (None, Some((_, sh)), None) => Some(sh),
            (None, None, _) => None,
        };
        let mut h = hh.unwrap_or(96.0) * 15.0;
        let max_w = self.paste_content_width(layout);
        if w > max_w {
            h = (h * max_w) / w;
            w = max_w;
        }
        self.new_image_run(&img.data, img.ext.clone(), w, h, &CarriedProps::plain(crate::read::plain::default_run_props()))
    }

    /// `newImageRun(data, ext, w, h, props)`: a new inline picture run with the XML Word writes.
    fn new_image_run(&mut self, data: &[u8], ext: Str, w: f64, h: f64, props: &CarriedProps) -> Run {
        let image = self.new_picture_image(data, ext, w, h);
        let p = self.new_run_props_from(&props.props);
        let mut run = Run { text: Utf16::from("\u{FFFC}"), props: Arc::new(p), rpr_link: props.link.clone(), ..Run::default() };
        run.ext = Some(Box::new(RunExt { image: Some(Box::new(image)), ..RunExt::default() }));
        run
    }

    /// `insertImageSized(data, ext, w, h)`: an inline picture of the given size (twips; fits the
    /// text width).
    fn insert_image_sized(
        &mut self,
        data: &[u8],
        w: f64,
        h: f64,
        layout: &PasteLayoutIn,
    ) -> Result<(), super::super::super::command::CommandError> {
        if self.paras().is_none() || self.locked() {
            return Ok(());
        }
        let (mut w, mut h) = (w, h);
        let max_w = self.paste_content_width(layout);
        if w > max_w {
            h = (h * max_w) / w;
            w = max_w;
        }
        self.begin_step(false, &[]);
        if self.has_selection() {
            let p0 = self.delete_selection()?;
            self.selection = Selection::caret(p0);
        }
        let props = match self.pending_props.clone() {
            Some(p) => p,
            None => self.current_props(),
        };
        let run = self.new_image_run(data, "png".into(), w, h, &props);
        let pos = self.insert_whole_run(self.selection.focus, run)?;
        self.selection = Selection::caret(pos);
        Ok(())
    }

    /// `pastePicture(d)`: what is on the clipboard painted by the engine (PNG, inline, the text's
    /// width): the paragraphs now, the PNG from the shell (`RenderClip`).
    fn paste_picture(&mut self, d: Arc<PasteData>, layout: PasteLayoutIn) -> Res {
        let (paras, rich_media) = if let Some(r) = &d.rich {
            (r.paras.clone(), true)
        } else if let Some(h) = d.input.html.clone() {
            (self.html_clip_paras(&h, PasteMode::Keep, &layout), false)
        } else {
            let cur = self.current_props();
            let props = Arc::new(without_revisions(&cur.props));
            let paras = rx::split_lines(d.input.text.units())
                .into_iter()
                .map(|t| ClipPara {
                    runs: vec![ClipRun {
                        text: Utf16::from_units(t),
                        props: Arc::clone(&props),
                        link: cur.link.clone(),
                        ..ClipRun::default()
                    }],
                    ..ClipPara::default()
                })
                .collect();
            (paras, false)
        };
        let job = PicturePaste { data: d, paras: Arc::new(paras), rich_media };
        let id = self.job_id();
        Ok(self.defer(
            Command::Clipboard(ClipboardCmd::RenderPictures { queue: vec![job], carried: None, layout }),
            vec![Job::RenderClip { id }],
        ))
    }

    /// The story and page `renderClipPng` lays out for a picture paste.
    pub(crate) fn render_request_of(&self, p: &PicturePaste) -> RenderRequest {
        let mut story = crate::model::Story::new();
        let mut ids = crate::ids::IdAlloc::default();
        for cp in p.paras.iter() {
            let mut para = crate::model::para::Paragraph::empty(ids.para());
            para.props = Arc::new(cp.mark.as_deref().cloned().unwrap_or_default());
            for r in &cp.runs {
                if let Some(o) = &r.obj {
                    para.runs.push((**o).clone());
                } else if !r.text.is_empty() {
                    para.runs.push(Run { text: r.text.clone(), props: Arc::clone(&r.props), ..Run::default() });
                }
            }
            story.paras.push(para);
        }
        let media: Vec<Arc<[u8]>> = if p.rich_media {
            p.data.rich.as_ref().map(|r| r.media.clone()).unwrap_or_default()
        } else {
            let mut list = Vec::new();
            let mut mm = MediaMap::ToClip { from: &self.doc.media, list: &mut list, seen: HashMap::new() };
            clone::story(&mut story, &mut Fresh { ids: None, media: &mut mm });
            list
        };
        let mut sp: crate::model::section::SectPr = (*self.sections.body_sect_pr).clone();
        obj::delete(&mut sp, "pgBorders");
        obj::delete(&mut sp, "cols");
        let h = uo_jsnum::math::max(sp.page_height_twips.to_number(), 31_680.0);
        sp.page_height_twips = JsNumber::Num(h);
        RenderRequest { story, sect_pr: sp, media }
    }

    /// The PNG of a picture paste is in (or the shell failed: nothing happens).
    pub(crate) fn render_done(
        &mut self,
        mut queue: Vec<PicturePaste>,
        carried: Option<Box<ChangeSet>>,
        result: JobResult,
        layout: PasteLayoutIn,
        _cx: &mut ExecCx<'_>,
    ) -> Res {
        if queue.is_empty() {
            return self.done_f8();
        }
        let head = queue.remove(0);
        let mut acc: Option<ChangeSet> = carried.map(|c| *c);
        if let JobResult::Rendered { png, width_twips, height_twips } = result {
            let depth = self.history.undo_len();
            let start = normalize_range(self.range()).start;
            self.insert_image_sized(&png, width_twips, height_twips, &layout)?;
            let end = self.selection.focus;
            self.f8.last_paste = Some(LastPaste {
                data: head.data,
                mode: PasteMode::Picture,
                list: None,
                depth,
                range: PasteRange { start, end, first: start.para, last: end.para, marked: Vec::new() },
            });
            let mut cs = self.finish();
            cs.events.push(Event::Pasted);
            merge_cs(&mut acc, cs);
        } else {
            merge_cs(&mut acc, self.finish_if_stepped());
        }
        if !queue.is_empty() {
            let id = self.job_id();
            return Ok(self.defer(
                Command::Clipboard(ClipboardCmd::RenderPictures { queue, carried: acc.map(Box::new), layout }),
                vec![Job::RenderClip { id }],
            ));
        }
        Ok(Outcome::Done(acc.unwrap_or_default()))
    }

    /// `pasteSpecial(format, { link })` once the data is known.
    pub(crate) fn paste_special_with(
        &mut self,
        d: Arc<PasteData>,
        format: PasteSpecialFormat,
        link: bool,
        layout: PasteLayoutIn,
        cx: &mut ExecCx<'_>,
    ) -> Res {
        use PasteSpecialFormat as F;
        if link
            && let Some(rich) = d.rich.clone()
            && rich.path.as_deref().is_some_and(|p| !p.is_empty())
            && rich.src.is_some()
            && matches!(format, F::Rtf | F::Html | F::Text | F::Unicode)
        {
            return self.paste_link(&rich, matches!(format, F::Text | F::Unicode), cx);
        }
        match format {
            F::Rtf => self.paste_data(d, Some(PasteMode::Keep), None, layout, cx),
            F::Html => {
                if d.rich.is_some() {
                    self.paste_data(d, Some(PasteMode::Keep), None, layout, cx)
                } else {
                    let d2 = Arc::new(PasteData { input: Arc::clone(&d.input), rich: None });
                    self.paste_data(d2, Some(PasteMode::Default), None, layout, cx)
                }
            }
            F::Text | F::Unicode => self.paste_data(d, Some(PasteMode::Text), None, layout, cx),
            F::Picture => self.paste_data(d, Some(PasteMode::Picture), None, layout, cx),
            F::Image => match &d.input.image {
                Some(img) => {
                    let cmd = PictureCmd::Insert {
                        data: Arc::clone(&img.data),
                        ext: img.ext.clone(),
                        ole: None,
                        max_width_twips: layout.content_width_twips,
                    };
                    self.exec_pictures(cmd, cx)
                }
                None => self.done_f8(),
            },
        }
    }

    /// `pasteLink(clip, text)` in one session: the source is this document (F8-D4: between two
    /// sessions the binding runs `AddLinkSource` on the source, then `PasteLinkWith` here).
    fn paste_link(&mut self, clip: &Arc<Clip>, text: bool, cx: &mut ExecCx<'_>) -> Res {
        if self.paras().is_none() || clip.path.as_deref().is_none_or(str::is_empty) || !self.same_doc(clip) {
            return self.done_f8();
        }
        let Some(name) = self.add_link_source_bookmark(clip) else { return self.done_f8() };
        self.paste_link_named(clip, &name, text, cx)
    }

    /// The target half of Paste Link: the `LINK` field (the source path and bookmark) around the
    /// copy (formatted, `\r`) or its text (`\t`), one undo step.
    pub(crate) fn paste_link_named(&mut self, clip: &Arc<Clip>, name: &str, text: bool, _cx: &mut ExecCx<'_>) -> Res {
        let Some(path) = clip.path.as_deref().filter(|p| !p.is_empty()) else { return self.done_f8() };
        if self.paras().is_none() {
            return self.done_f8();
        }
        let instr =
            format!(" LINK Word.Document.12 \"{}\" \"{name}\" \\a {} ", rx::double_backslashes(path), if text { "\\t" } else { "\\r" });
        self.begin_step(false, &[]);
        if self.has_selection() {
            let p0 = self.delete_selection()?;
            self.selection = Selection::caret(p0);
        }
        let base = match self.pending_props.clone() {
            Some(p) => p,
            None => self.current_props(),
        };
        let props = Arc::new(without_revisions(&base.props));
        let mut begin = Run { text: Utf16::new(), props: Arc::clone(&props), rpr_link: base.link.clone(), ..Run::default() };
        begin.ext = Some(Box::new(RunExt { field_begin: Some(instr.into()), ..RunExt::default() }));
        let mut pos = self.insert_whole_run(self.selection.focus, begin)?;
        self.selection = Selection::caret(pos);
        // the result: the copy (no undo step of its own: the same step)
        let depth = self.history.undo_len();
        let r = if text {
            let t = clip_text(clip);
            self.paste_text_only(&t)?
        } else {
            self.insert_clip(&clip.paras, false, &[], Some(clip), Some(&clip.media))?
        };
        self.history.collapse_to(depth);
        if let Some(r) = r {
            pos = r.end;
        }
        let mut end = Run { text: Utf16::new(), props, rpr_link: base.link, ..Run::default() };
        end.ext = Some(Box::new(RunExt { field_end: Some(true), ..RunExt::default() }));
        pos = self.insert_whole_run(pos, end)?;
        self.selection = Selection::caret(pos);
        self.f8.last_paste = None;
        Ok(Outcome::Done(self.finish()))
    }

    /// `addLinkSourceBookmark(clip)`: marks the copied range with the bookmark `OLE_LINKn` (the
    /// source of Paste Link); `None` if the range changed since the copy. No undo step, as the TS.
    pub(crate) fn add_link_source_bookmark(&mut self, clip: &Clip) -> Option<Str> {
        if self.paras().is_none() || !self.same_doc(clip) {
            return None;
        }
        let (_, DocRange { start, end }) = clip.src?;
        let paras = &self.doc.body.paras;
        if end.para as usize >= paras.len() || start.para > end.para {
            return None;
        }
        let mut now: Vec<u16> = Vec::new();
        for i in 0..=(end.para - start.para) {
            let p = paras.get((start.para + i) as usize)?;
            let mut t: Vec<u16> = Vec::new();
            for x in &p.runs {
                match x.ext() {
                    Some(e) if e.custom.is_some() => t.extend_from_slice(e.custom.as_deref().map_or(&[][..], |c| c.fallback_text.units())),
                    Some(e) if e.note_ref.is_some() => {}
                    _ => t.extend_from_slice(x.text.units()),
                }
            }
            let a = if i == 0 { (start.offset as usize).min(t.len()) } else { 0 };
            let b = if start.para + i == end.para { (end.offset as usize).min(t.len()) } else { t.len() };
            if i > 0 {
                now.push(10);
            }
            now.extend_from_slice(t.get(a..b.max(a)).unwrap_or(&[]));
        }
        if now.as_slice() != clip.text.units() {
            return None;
        }
        let taken: Vec<Str> = super::super::bookmarks::bookmarks_in(paras.iter()).into_iter().map(|b| b.name).collect();
        let mut n = 1u32;
        while taken.iter().any(|t| **t == *format!("OLE_LINK{n}")) {
            n += 1;
        }
        let name = format!("OLE_LINK{n}");
        let id = self.new_bookmark_id();
        super::super::bookmarks::add_bookmark_marks(&mut self.doc.body.paras, &name, id, start, end).ok()?;
        if matches!(self.ctx, EditContext::Body) {
            for i in start.para..=end.para {
                self.touch_para(i as usize);
            }
        } else {
            self.touch_all();
        }
        Some(name.into())
    }

    /// `pasteAllClipboardItems()`: the items, oldest first, each its own paste.
    pub(crate) fn paste_all(&mut self, layout: PasteLayoutIn, cx: &mut ExecCx<'_>) -> Res {
        let clips: Vec<Arc<Clip>> = cx.clipboard.items().map(|x| Arc::clone(&x.clip)).collect();
        let mut acc: Option<ChangeSet> = None;
        let mut pictures: Vec<PicturePaste> = Vec::new();
        for clip in clips.into_iter().rev() {
            let d = PasteData { input: Arc::new(PasteInput { text: clip.text.clone(), ..PasteInput::default() }), rich: Some(clip) };
            match self.paste_data(Arc::new(d), None, None, layout, cx)? {
                Outcome::Done(cs) => merge_cs(&mut acc, cs),
                Outcome::NeedsJobs(_) => {
                    // a picture paste (the `void this.pastePicture(d)` of the loop): it goes on after
                    // the others, in order
                    if let Some(p) = self.pending.take()
                        && let Command::Clipboard(ClipboardCmd::RenderPictures { queue, .. }) = p.command
                    {
                        pictures.extend(queue);
                    }
                }
            }
        }
        if pictures.is_empty() {
            return Ok(Outcome::Done(acc.unwrap_or_default()));
        }
        let id = self.job_id();
        Ok(self.defer(
            Command::Clipboard(ClipboardCmd::RenderPictures { queue: pictures, carried: acc.map(Box::new), layout }),
            vec![Job::RenderClip { id }],
        ))
    }
}

/// `patch[k] = mark[k]` for key `k`: the mark's value, or `undefined` when the mark does not have
/// the key (a new key of the patch, in order).
fn put_slot(patch: &mut ParaProps, mark: &ParaProps, i: usize) {
    let had = obj::has_key(patch, i);
    let mut order = if had { Vec::new() } else { obj::key_indices(patch) };
    if obj::has_key(mark, i) {
        crate::js::JsAssign::assign_slot(patch, mark, i);
    } else {
        crate::js::JsAssign::remove_slot(patch, i);
        crate::js::JsObject::cleared_mut(patch).set(u32::try_from(i).unwrap_or(u32::MAX));
    }
    if !had {
        order.push(u8::try_from(i).unwrap_or(u8::MAX));
        obj::set_key_order(patch, &order);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn levels() {
        assert_eq!(html_list_levels(true).len(), 9);
        assert!(html_list_levels(false)[1].contains("Courier New"));
    }
}
