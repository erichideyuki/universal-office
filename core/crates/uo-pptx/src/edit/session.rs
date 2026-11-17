//! The editing session (`universal-slides.ts` without the DOM): [`Editor`] owns a [`Deck`], the undo
//! history and the UI state the history saves with each step (slide index, selected shape), and runs
//! the operations of `deck-edit` through the step protocol of `edit()`:
//! snapshot before -> operation -> if the package changed: re-read, select, push. An `Err` restores the
//! snapshot (no partial change; the TS lets the exception escape with what it had written).
//!
//! The shell keeps the input (clicks, keys, the inline text overlay), the painting and the bars; it
//! calls [`Editor::apply`] with an [`Op`] and gets the [`Outcome`]; the display lists of the slides come
//! from the deck's emission after the edit (the per-slide re-read is lazy: a slide is read again when a
//! part it read changed, `uo-pptx.md` P4/P6).

use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use uo_fonts::{FaceRequest, FontEmbeddingSettings, FontLimits, POWERPOINT_DEFAULT_EMBEDDING};

use super::blank::{BlankDeckOptions, blank_deck_store};
use super::doc::{
    DocProperties, ShapeClip, ShowSettingsSpec, extract_shape, paste_shape, set_slide_notes, write_properties, write_show_settings,
};
use super::fonts::{StoredEmbedding, as_usages, deck_font_usage, powerpoint_embedding_policy, read_pptx_embedding, write_pptx_embedding};
use super::pkg::{
    E, EditError, TransitionSpec, add_slide, delete_slide, duplicate_slide, layout_of, list_layouts, move_slide, set_slide_background,
    set_slide_hidden, set_slide_layout, set_slide_transition,
};
use super::random::Xorshift32;
use super::shapes::{
    ArrangeMode, BulletKind, Frame, RunFormat, align_shape_text, arrange_shape, clear_shape_formatting, delete_shape, format_shape_text,
    insert_image, insert_shape, insert_text_box, replace_in_shape, scale_shape_text, set_shape_bullets, set_shape_fill, set_shape_frame,
    set_shape_link, set_shape_outline, set_shape_text,
};
use super::{EditHistory, EditState, changed_since, restore};
use crate::Deck;
use crate::error::PptxError;
use crate::limits::Limits;
use crate::model::SlideModel;
use crate::package::PartStore;

static NEXT_ID: AtomicU64 = AtomicU64::new(1);

/// What `edit()` re-reads after the change: only its effect on the session state is kept in Rust (the
/// slide models are re-read lazily by the deck).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Reread {
    /// the current slide
    Slide,
    /// the whole deck (the slide list may have changed; the index is clamped)
    Deck,
    /// one slide
    Index(i64),
}

/// The current slide and selected shape after the edit (`then`).
#[derive(Clone, PartialEq, Eq, Debug, Default)]
pub struct Then {
    pub index: Option<i64>,
    /// `Some(None)` = clear the selection; `None` = keep the selected shape
    pub select: Option<Option<Box<str>>>,
}

/// The slides an operation applies to.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Target {
    Slide(i64),
    All,
}

/// The editing commands: one per mutating method of `<universal-slides>`, with the arguments the
/// element resolves from its UI state (slide index, shape id).
#[derive(Clone, Debug)]
pub enum Op {
    /// New slide after the current one (`layout` `None` = the default of `defaultNewLayout`).
    AddSlide {
        layout: Option<String>,
    },
    DuplicateSlide {
        i: i64,
    },
    DeleteSlide {
        i: i64,
    },
    MoveSlide {
        from: i64,
        to: i64,
    },
    SetSlideHidden {
        i: i64,
        hidden: bool,
    },
    SetSlideLayout {
        i: i64,
        layout: String,
    },
    SetSlideBackground {
        target: Target,
        color: Option<String>,
    },
    SetSlideTransition {
        target: Target,
        spec: Option<TransitionSpec>,
    },
    SetShowSettings(ShowSettingsSpec),
    SetShapeText {
        shape: String,
        text: String,
    },
    /// The commit of the inline text overlay (`commitEditShape`): label `typing`, on `slide`.
    Typing {
        slide: i64,
        shape: String,
        text: String,
    },
    FormatText {
        shape: String,
        f: RunFormat,
    },
    /// Grow/Shrink Font (`growFont`): the steps of the app, from `base` (the first run's size or 18).
    GrowFont {
        shape: String,
        dir: i32,
        base: f64,
    },
    AlignText {
        shape: String,
        algn: String,
    },
    SetBullets {
        shape: String,
        kind: BulletKind,
    },
    ClearFormatting {
        shape: String,
    },
    SetLink {
        shape: String,
        url: Option<String>,
    },
    InsertTextBox {
        text: String,
        frame: Option<Frame>,
    },
    InsertShape {
        prst: String,
        frame: Option<Frame>,
    },
    /// New picture; without a frame, the natural size (`size_px`, 640x480 if unknown) limited to the slide.
    InsertImage {
        bytes: Arc<[u8]>,
        mime: String,
        frame: Option<Frame>,
        size_px: Option<(f64, f64)>,
    },
    SetShapeFrame {
        shape: String,
        frame: Frame,
    },
    SetShapeFill {
        shape: String,
        color: Option<String>,
    },
    SetShapeOutline {
        shape: String,
        color: Option<String>,
    },
    ArrangeShape {
        shape: String,
        mode: ArrangeMode,
    },
    DeleteShape {
        shape: String,
    },
    PasteShape {
        clip: ShapeClip,
    },
    SetSlideNotes {
        i: i64,
        text: String,
    },
    SetProperties(DocProperties),
    /// Replace in the text of the shapes: in `hit` (slide, shape id) or in every shape that has `query`.
    ReplaceText {
        query: String,
        replacement: String,
        match_case: bool,
        hit: Option<(i64, String)>,
    },
}

/// What an operation gives back.
#[derive(Clone, Debug, PartialEq)]
pub enum Outcome {
    /// nothing (or the operation was a no-op by its guard)
    Nothing,
    Bool(bool),
    /// the slide index (`addSlide`, `duplicateSlide`), or -1
    Index(i64),
    /// the id of the shape an insert/paste made and selected (`None`: nothing was made)
    Shape(Option<String>),
    /// replacements made
    Count(f64),
}

/// The editor session.
#[derive(Debug)]
pub struct Editor {
    deck: Deck,
    history: EditHistory,
    index: u32,
    selected: Option<Box<str>>,
    rng: Xorshift32,
    id: u64,
    lang: Box<str>,
    embedding: FontEmbeddingSettings,
    embedding_touched: bool,
}

const FONT_STEPS: [f64; 24] = [
    8.0, 9.0, 10.0, 10.5, 11.0, 12.0, 14.0, 16.0, 18.0, 20.0, 24.0, 28.0, 32.0, 36.0, 40.0, 44.0, 48.0, 54.0, 60.0, 66.0, 72.0, 80.0, 88.0,
    96.0,
];

fn idx_u32(i: i64) -> u32 {
    u32::try_from(i.max(0)).unwrap_or(u32::MAX)
}

impl Editor {
    /// A session over an open deck (`loadPptx`: empty history, no selection).
    pub fn new(deck: Deck) -> Editor {
        let max = deck.limits().max_history_bytes;
        let mut e = Editor {
            deck,
            history: EditHistory::new(100, max),
            index: 0,
            selected: None,
            rng: Xorshift32::default(),
            id: NEXT_ID.fetch_add(1, Ordering::Relaxed),
            lang: "en-US".into(),
            embedding: POWERPOINT_DEFAULT_EMBEDDING,
            embedding_touched: false,
        };
        e.embedding = read_pptx_embedding(e.deck.parts()).0.over(POWERPOINT_DEFAULT_EMBEDDING);
        e
    }

    /// Opens a `.pptx` (`openPresentation`).
    pub fn open(bytes: &[u8], limits: Limits) -> Result<Editor, PptxError> {
        Ok(Editor::new(Deck::open(bytes, limits)?))
    }

    /// A blank presentation (`newPresentation`): the same parts as `blankDeck`.
    pub fn blank(opts: &BlankDeckOptions, limits: Limits) -> Editor {
        let mut e = Editor::new(Deck::from_store(blank_deck_store(opts), limits));
        e.lang = opts.lang.as_str().into();
        e
    }

    /// The deck (read side: models, emission).
    pub fn deck(&self) -> &Deck {
        &self.deck
    }

    /// The deck, mutable (reading a slide fills the deck's caches).
    pub fn deck_mut(&mut self) -> &mut Deck {
        &mut self.deck
    }

    /// The language written in new text (`lang` of the runs): the interface's.
    pub fn set_text_lang(&mut self, lang: &str) {
        self.lang = lang.into();
    }

    /// Seeds the source of `Math.random()` (new creation ids of copies).
    pub fn set_random_seed(&mut self, seed: u32) {
        self.rng = Xorshift32::new(seed);
    }

    /// The identity of this session (a clip made here pastes without copying shared parts).
    pub fn session_id(&self) -> u64 {
        self.id
    }

    // ───────────────────────── UI state ─────────────────────────

    /// The current slide.
    pub fn index(&self) -> u32 {
        self.index
    }

    /// Goes to a slide (the shell clamps; the selection is cleared when the slide changes).
    pub fn set_index(&mut self, i: u32) {
        if i != self.index {
            self.index = i;
            self.selected = None;
        }
    }

    /// The selected shape id.
    pub fn selected(&self) -> Option<&str> {
        self.selected.as_deref()
    }

    /// Selects a shape of the current slide (`None` clears); an id that is not a shape of the slide, or
    /// is a layout/master decoration, selects nothing.
    pub fn select(&mut self, id: Option<&str>) {
        self.select_by_id(id.map(Box::from));
    }

    fn edit_state(&self) -> EditState {
        EditState { index: self.index, shape_id: self.selected.clone() }
    }

    /// The targets of the slides with a part, in order (`slidePaths`).
    pub fn slide_paths(&mut self) -> Vec<Box<str>> {
        match self.deck.presentation() {
            Ok(p) => p.slides.iter().filter_map(|r| r.target.clone()).collect(),
            Err(_) => Vec::new(),
        }
    }

    /// Number of slides with a part.
    pub fn slide_count(&mut self) -> usize {
        self.slide_paths().len()
    }

    /// `slidePath(i)`: the part of slide `i`.
    pub fn slide_path(&mut self, i: i64) -> E<Box<str>> {
        let paths = self.slide_paths();
        usize::try_from(i).ok().and_then(|k| paths.get(k).cloned()).ok_or(EditError::NoSuchSlide(i))
    }

    /// The model of slide `i` (the `i`-th with a part), or `None` when it does not read.
    pub fn slide_model(&mut self, i: i64) -> Option<Arc<SlideModel>> {
        let k = usize::try_from(i).ok()?;
        let raw = {
            let pres = self.deck.presentation().ok()?;
            pres.slides.iter().enumerate().filter(|(_, r)| r.target.is_some()).nth(k)?.0
        };
        self.deck.slide(u32::try_from(raw).ok()?).ok()
    }

    /// The notes text of slide `i` (the `i`-th with a part).
    pub fn slide_notes(&mut self, i: i64) -> Option<String> {
        let raw = self.raw_index(i)?;
        self.deck.slide_notes(raw).ok()
    }

    fn select_by_id(&mut self, id: Option<Box<str>>) {
        self.selected = match id {
            None => None,
            Some(id) => {
                let found = self
                    .slide_model(i64::from(self.index))
                    .is_some_and(|m| m.shapes.iter().any(|s| s.inherited != Some(true) && s.id == id));
                if found { Some(id) } else { None }
            }
        };
    }

    fn reread_deck(&mut self, index: i64) {
        let n = i64::try_from(self.slide_count()).unwrap_or(i64::MAX);
        self.index = idx_u32(index.min(n - 1).max(0));
    }

    // ───────────────────────── the step protocol ─────────────────────────

    /// `edit()`: runs `f` over the package as ONE undo step; on `Err` the package is restored.
    pub fn edit<T>(&mut self, label: &str, reread: Reread, then: &Then, f: impl FnOnce(&mut PartStore, &mut Xorshift32) -> E<T>) -> E<T> {
        let before = self.deck.parts().snapshot();
        let state_before = self.edit_state();
        let result = {
            let rng = &mut self.rng;
            f(self.deck.parts_mut(), rng)
        };
        let result = match result {
            Ok(v) => v,
            Err(e) => {
                restore(self.deck.parts_mut(), &before);
                return Err(e);
            }
        };
        if !changed_since(self.deck.parts(), &before) {
            return Ok(result);
        }
        let keep = match &then.select {
            Some(s) => s.clone(),
            None => state_before.shape_id.clone(),
        };
        match reread {
            Reread::Deck => self.reread_deck(then.index.unwrap_or(i64::from(self.index))),
            Reread::Slide | Reread::Index(_) => {
                if let Some(i) = then.index {
                    self.index = idx_u32(i);
                }
            }
        }
        self.select_by_id(keep);
        let after = self.deck.parts().snapshot();
        let state_after = self.edit_state();
        self.history.push(label, before, after, state_before, state_after);
        Ok(result)
    }

    pub fn can_undo(&self) -> bool {
        self.history.can_undo()
    }

    pub fn can_redo(&self) -> bool {
        self.history.can_redo()
    }

    pub fn undo_label(&self) -> Option<&str> {
        self.history.undo_label()
    }

    pub fn redo_label(&self) -> Option<&str> {
        self.history.redo_label()
    }

    /// The history (for the memory tests and the dirty flag).
    pub fn history(&self) -> &EditHistory {
        &self.history
    }

    /// Undo the last edit: the package, the current slide and the selection go back. Returns the state.
    pub fn undo(&mut self) -> Option<EditState> {
        let (snap, state) = self.history.undo()?;
        restore(self.deck.parts_mut(), &snap);
        self.reread_deck(i64::from(state.index));
        self.select_by_id(state.shape_id.clone());
        Some(state)
    }

    /// Redo the undone edit.
    pub fn redo(&mut self) -> Option<EditState> {
        let (snap, state) = self.history.redo()?;
        restore(self.deck.parts_mut(), &snap);
        self.reread_deck(i64::from(state.index));
        self.select_by_id(state.shape_id.clone());
        Some(state)
    }

    /// There are unsaved changes (`isDirty`).
    pub fn is_dirty(&self) -> bool {
        self.history.dirty() || self.embedding_touched
    }

    /// The document was saved by the host: the current state is the clean one (`markClean`).
    pub fn mark_clean(&mut self) {
        self.history.mark_saved();
        self.embedding_touched = false;
    }

    // ───────────────────────── the operations ─────────────────────────

    /// The layout a new slide gets (`defaultNewLayout`): after a title slide, "Title and Content".
    pub fn default_new_layout(&mut self) -> Option<String> {
        let layouts = list_layouts(self.deck.parts()).ok()?;
        let cur = self.slide_path(i64::from(self.index)).ok().and_then(|p| layout_of(self.deck.parts(), &p));
        let info = layouts.iter().find(|l| Some(&l.path) == cur.as_ref());
        if let Some(info) = info.filter(|i| i.type_ == "title") {
            let next = layouts
                .iter()
                .find(|l| l.master == info.master && l.type_ == "obj")
                .or_else(|| layouts.iter().find(|l| l.master == info.master && l.type_ == "tx"));
            if let Some(n) = next {
                return Some(n.path.clone());
            }
        }
        cur.or_else(|| layouts.iter().find(|l| l.type_ == "obj").map(|l| l.path.clone()))
            .or_else(|| layouts.first().map(|l| l.path.clone()))
    }

    fn slide_size(&mut self) -> (f64, f64) {
        match self.deck.presentation() {
            Ok(p) if p.slide_w != 0.0 && !p.slide_w.is_nan() => (p.slide_w, p.slide_h),
            _ => (12_192_000.0, 6_858_000.0),
        }
    }

    fn centered(&mut self, w: f64, h: f64) -> Frame {
        let (sw, sh) = self.slide_size();
        super::doc::centered(sw, sh, w, h)
    }

    fn shape_in_group(&mut self, id: &str) -> bool {
        self.slide_model(i64::from(self.index)).is_some_and(|m| {
            m.shapes.iter().any(|s| {
                &*s.id == id
                    && s.inherited != Some(true)
                    && s.extras.as_ref().is_some_and(|x| x.group_ids.as_ref().is_some_and(|g| !g.is_empty()))
            })
        })
    }

    fn insert_with(&mut self, label: &str, f: impl FnOnce(&mut PartStore, &str) -> E<String>) -> E<Outcome> {
        if self.slide_count() == 0 {
            self.apply(Op::AddSlide { layout: None })?;
        }
        let path = self.slide_path(i64::from(self.index))?;
        let id = self.edit(label, Reread::Slide, &Then { index: None, select: Some(None) }, |s, _| f(s, &path))?;
        self.select_by_id(Some(id.as_str().into()));
        Ok(Outcome::Shape(Some(id)))
    }

    /// Runs one command (the mutating methods of `<universal-slides>`).
    pub fn apply(&mut self, op: Op) -> E<Outcome> {
        let lang = self.lang.clone();
        let cur = i64::from(self.index);
        match op {
            Op::AddSlide { layout } => {
                let n = self.slide_count();
                let at = if n > 0 { cur + 1 } else { 0 };
                let Some(path) = layout.or_else(|| self.default_new_layout()) else { return Ok(Outcome::Index(-1)) };
                self.edit("newSlide", Reread::Deck, &Then { index: Some(at), select: Some(None) }, |s, _| {
                    add_slide(s, at, &path, &lang).map(|_| ())
                })?;
                Ok(Outcome::Index(at))
            }
            Op::DuplicateSlide { i } => {
                if self.slide_path(i).is_err() {
                    return Ok(Outcome::Index(-1));
                }
                self.edit("duplicateSlide", Reread::Deck, &Then { index: Some(i + 1), select: Some(None) }, |s, rng| {
                    duplicate_slide(s, i, rng).map(|_| ())
                })?;
                Ok(Outcome::Index(i + 1))
            }
            Op::DeleteSlide { i } => {
                if self.slide_path(i).is_err() {
                    return Ok(Outcome::Nothing);
                }
                let n = i64::try_from(self.slide_count()).unwrap_or(i64::MAX);
                let next = i.min(n - 2).max(0);
                self.edit("deleteSlide", Reread::Deck, &Then { index: Some(next), select: Some(None) }, |s, _| delete_slide(s, i))?;
                Ok(Outcome::Nothing)
            }
            Op::MoveSlide { from, to } => {
                let n = i64::try_from(self.slide_count()).unwrap_or(i64::MAX);
                if from == to || self.slide_path(from).is_err() || to < 0 || to >= n {
                    return Ok(Outcome::Nothing);
                }
                let index = if cur == from { to } else { cur };
                self.edit("moveSlide", Reread::Deck, &Then { index: Some(index), select: None }, |s, _| move_slide(s, from, to))?;
                Ok(Outcome::Nothing)
            }
            Op::SetSlideHidden { i, hidden } => {
                if self.slide_path(i).is_err() {
                    return Ok(Outcome::Nothing);
                }
                let path = self.slide_path(i)?;
                self.edit("hideSlide", Reread::Index(i), &Then::default(), |s, _| {
                    set_slide_hidden(s, &path, hidden);
                    Ok(())
                })?;
                Ok(Outcome::Nothing)
            }
            Op::SetSlideLayout { i, layout } => {
                if self.slide_path(i).is_err() {
                    return Ok(Outcome::Nothing);
                }
                let path = self.slide_path(i)?;
                self.edit("layout", Reread::Index(i), &Then::default(), |s, _| set_slide_layout(s, &path, &layout))?;
                Ok(Outcome::Nothing)
            }
            Op::SetSlideBackground { target, color } => {
                let paths = self.target_paths(target)?;
                let reread = if target == Target::All { Reread::Deck } else { Reread::Index(target_index(target)) };
                self.edit("background", reread, &Then::default(), |s, _| {
                    for p in &paths {
                        set_slide_background(s, p, color.as_deref());
                    }
                    Ok(())
                })?;
                Ok(Outcome::Nothing)
            }
            Op::SetSlideTransition { target, spec } => {
                let paths = self.target_paths(target)?;
                let reread = if target == Target::All { Reread::Deck } else { Reread::Index(target_index(target)) };
                self.edit("transition", reread, &Then::default(), |s, _| {
                    for p in &paths {
                        set_slide_transition(s, p, spec.as_ref())?;
                    }
                    Ok(())
                })?;
                Ok(Outcome::Nothing)
            }
            Op::SetShowSettings(spec) => {
                self.edit("showSettings", Reread::Deck, &Then::default(), |s, _| write_show_settings(s, &spec))?;
                Ok(Outcome::Nothing)
            }
            Op::SetShapeText { shape, text } => self.on_shape("text", |s, p| set_shape_text(s, p, &shape, &text, &lang)),
            Op::Typing { slide, shape, text } => {
                let Some(path) = self.slide_path(slide).ok() else { return Ok(Outcome::Nothing) };
                let before = self.slide_model(slide).and_then(|m| {
                    m.shapes.iter().find(|s| *s.id == *shape && s.inherited != Some(true)).map(|sh| {
                        sh.tx_body.as_ref().map_or_else(String::new, |b| {
                            b.paragraphs.iter().map(|p| p.runs.iter().map(|r| &*r.text).collect::<String>()).collect::<Vec<_>>().join("\n")
                        })
                    })
                });
                let Some(before) = before else { return Ok(Outcome::Nothing) };
                if text == before {
                    return Ok(Outcome::Nothing);
                }
                let prev = self.index;
                self.index = idx_u32(slide);
                let r = self.edit("typing", Reread::Index(slide), &Then::default(), |s, _| set_shape_text(s, &path, &shape, &text, &lang));
                self.index = prev;
                Ok(Outcome::Bool(r?))
            }
            Op::FormatText { shape, f } => self.on_shape("format", |s, p| format_shape_text(s, p, &shape, &f, &lang)),
            Op::GrowFont { shape, dir, base } => {
                let mut step = |pt: Option<f64>| -> f64 {
                    let v = pt.unwrap_or(base);
                    if dir > 0 {
                        FONT_STEPS.iter().copied().find(|s| *s > v + 0.01).unwrap_or_else(|| uo_jsnum::math::round(v + 10.0))
                    } else {
                        FONT_STEPS.iter().rev().copied().find(|s| *s < v - 0.01).unwrap_or_else(|| uo_jsnum::math::max(1.0, v - 1.0))
                    }
                };
                self.on_shape("fontSize", |s, p| scale_shape_text(s, p, &shape, &mut step, &lang))
            }
            Op::AlignText { shape, algn } => self.on_shape("align", |s, p| align_shape_text(s, p, &shape, &algn)),
            Op::SetBullets { shape, kind } => self.on_shape("bullets", |s, p| set_shape_bullets(s, p, &shape, kind)),
            Op::ClearFormatting { shape } => self.on_shape("clearFormat", |s, p| clear_shape_formatting(s, p, &shape, &lang)),
            Op::SetLink { shape, url } => self.on_shape("link", |s, p| set_shape_link(s, p, &shape, url.as_deref(), &lang)),
            Op::InsertTextBox { text, frame } => {
                let (sw, _) = self.slide_size();
                let f = match frame {
                    Some(f) => f,
                    None => self.centered(sw * 0.4, 12700.0 * 40.0),
                };
                self.insert_with("textBox", |s, p| insert_text_box(s, p, &f, &text, &lang))
            }
            Op::InsertShape { prst, frame } => {
                let (_, sh) = self.slide_size();
                let side = uo_jsnum::math::round(sh * 0.3);
                let f = match frame {
                    Some(f) => f,
                    None if prst == "line" => Frame { cy: 0.0, ..self.centered(side * 1.5, 0.0) },
                    None => {
                        let w = if prst == "rect" || prst == "roundRect" { side * 1.5 } else { side };
                        self.centered(w, side)
                    }
                };
                self.insert_with("shape", |s, p| insert_shape(s, p, &prst, &f, &lang))
            }
            Op::InsertImage { bytes, mime, frame, size_px } => {
                let f = match frame {
                    Some(f) => f,
                    None => {
                        let (sw, sh) = self.slide_size();
                        let (pw, ph) = size_px.unwrap_or((640.0, 480.0));
                        let (mut w, mut h) = (pw * EMU_PER_PX, ph * EMU_PER_PX);
                        let k = uo_jsnum::math::min(uo_jsnum::math::min(1.0, (sw * 0.8) / w), (sh * 0.8) / h);
                        w *= k;
                        h *= k;
                        Frame { x: (sw - w) / 2.0, y: (sh - h) / 2.0, cx: w, cy: h }
                    }
                };
                self.insert_with("image", |s, p| insert_image(s, p, bytes, &mime, &f, "Picture").map(|(id, _)| id))
            }
            Op::SetShapeFrame { shape, frame } => {
                if self.shape_in_group(&shape) {
                    return Ok(Outcome::Nothing);
                }
                self.on_shape("frame", |s, p| set_shape_frame(s, p, &shape, &frame))
            }
            Op::SetShapeFill { shape, color } => self.on_shape("fill", |s, p| set_shape_fill(s, p, &shape, color.as_deref())),
            Op::SetShapeOutline { shape, color } => self.on_shape("outline", |s, p| set_shape_outline(s, p, &shape, color.as_deref())),
            Op::ArrangeShape { shape, mode } => self.on_shape("arrange", |s, p| arrange_shape(s, p, &shape, mode)),
            Op::DeleteShape { shape } => {
                let path = self.slide_path(cur)?;
                let r = self
                    .edit("deleteShape", Reread::Slide, &Then { index: None, select: Some(None) }, |s, _| delete_shape(s, &path, &shape))?;
                Ok(Outcome::Bool(r))
            }
            Op::PasteShape { clip } => {
                if self.slide_count() == 0 {
                    self.apply(Op::AddSlide { layout: None })?;
                }
                let path = self.slide_path(i64::from(self.index))?;
                let same = clip.source_id == self.id;
                let same_slide = same && *clip.slide_path == *path;
                let d = if same_slide { 12700.0 * 12.0 } else { 0.0 };
                let id = self.edit("paste", Reread::Slide, &Then { index: None, select: Some(None) }, |s, _| {
                    Ok(paste_shape(s, same, &path, &clip, d, d)?.unwrap_or_default())
                })?;
                if id.is_empty() {
                    return Ok(Outcome::Shape(None));
                }
                self.select_by_id(Some(id.as_str().into()));
                Ok(Outcome::Shape(Some(id)))
            }
            Op::SetSlideNotes { i, text } => {
                let Ok(path) = self.slide_path(i) else { return Ok(Outcome::Bool(false)) };
                let raw = self.raw_index(i).unwrap_or(0);
                let current = self.deck.slide_notes(raw).unwrap_or_default();
                if current == text {
                    return Ok(Outcome::Bool(false));
                }
                let r = self.edit("notes", Reread::Index(i), &Then::default(), |s, _| set_slide_notes(s, &path, &text, &lang))?;
                Ok(Outcome::Bool(r))
            }
            Op::SetProperties(p) => {
                self.edit("properties", Reread::Deck, &Then::default(), |s, _| write_properties(s, &p))?;
                Ok(Outcome::Nothing)
            }
            Op::ReplaceText { query, replacement, match_case, hit } => {
                let hits: Vec<(i64, String)> = match hit {
                    Some(h) => vec![h],
                    None => self.find_text(&query, match_case),
                };
                if hits.is_empty() {
                    return Ok(Outcome::Count(0.0));
                }
                let mut resolved: Vec<(Box<str>, String)> = Vec::new();
                for (slide, id) in hits {
                    resolved.push((self.slide_path(slide)?, id));
                }
                let mut count = 0.0;
                self.edit("replace", Reread::Deck, &Then::default(), |s, _| {
                    for (path, id) in &resolved {
                        count += replace_in_shape(s, path, id, &query, &replacement, match_case)?;
                    }
                    Ok(())
                })?;
                Ok(Outcome::Count(count))
            }
        }
    }

    fn raw_index(&mut self, i: i64) -> Option<u32> {
        let k = usize::try_from(i).ok()?;
        let pres = self.deck.presentation().ok()?;
        let raw = pres.slides.iter().enumerate().filter(|(_, r)| r.target.is_some()).nth(k)?.0;
        u32::try_from(raw).ok()
    }

    fn target_paths(&mut self, t: Target) -> E<Vec<Box<str>>> {
        match t {
            Target::Slide(i) => Ok(vec![self.slide_path(i)?]),
            Target::All => {
                let n = i64::try_from(self.slide_count()).unwrap_or(0);
                (0..n).map(|k| self.slide_path(k)).collect()
            }
        }
    }

    /// An edit of one shape of the current slide: `edit(label, ..., 'slide')`.
    fn on_shape(&mut self, label: &str, f: impl FnOnce(&mut PartStore, &str) -> E<bool>) -> E<Outcome> {
        let path = self.slide_path(i64::from(self.index))?;
        let r = self.edit(label, Reread::Slide, &Then::default(), |s, _| f(s, &path))?;
        Ok(Outcome::Bool(r))
    }

    /// The shapes whose text has `query`: (slide, shape id), slides in order (`findText`).
    pub fn find_text(&mut self, query: &str, match_case: bool) -> Vec<(i64, String)> {
        if query.is_empty() {
            return Vec::new();
        }
        let q = if match_case { query.to_owned() } else { uo_jsnum::text::to_lowercase(query) };
        let mut out = Vec::new();
        let n = i64::try_from(self.slide_count()).unwrap_or(0);
        for si in 0..n {
            let Some(m) = self.slide_model(si) else { continue };
            for sh in &m.shapes {
                if sh.inherited == Some(true) {
                    continue;
                }
                let Some(body) = &sh.tx_body else { continue };
                let text =
                    body.paragraphs.iter().map(|p| p.runs.iter().map(|r| &*r.text).collect::<String>()).collect::<Vec<_>>().join("\n");
                let hay = if match_case { text } else { uo_jsnum::text::to_lowercase(&text) };
                if hay.contains(&q) {
                    out.push((si, sh.id.to_string()));
                }
            }
        }
        out
    }

    /// Copies a shape (`copyShape`): its XML and relations, for [`Op::PasteShape`].
    pub fn copy_shape(&mut self, shape: &str) -> E<Option<ShapeClip>> {
        if self.shape_in_group(shape) {
            return Ok(None);
        }
        let path = self.slide_path(i64::from(self.index))?;
        extract_shape(self.deck.parts(), self.id, &path, shape)
    }

    // ───────────────────────── fonts in the file and save ─────────────────────────

    /// The option "fonts in the file" (`getFontEmbedding`).
    pub fn font_embedding(&self) -> FontEmbeddingSettings {
        self.embedding
    }

    /// Changes the option (`setFontEmbedding`): valid at the next save.
    pub fn set_font_embedding(&mut self, embed: Option<bool>, subset: Option<bool>) {
        if let Some(e) = embed {
            self.embedding.embed = e;
        }
        if let Some(s) = subset {
            self.embedding.subset = s;
        }
        self.embedding_touched = true;
    }

    /// Phase 1 of the save with fonts: the faces the plan wants (empty when the option is off).
    pub fn font_requests(&self) -> Vec<FaceRequest> {
        let usage = deck_font_usage(self.deck.parts());
        let usages = as_usages(&usage);
        let (_, embedded) = read_pptx_embedding(self.deck.parts());
        uo_fonts::embedding_requests(&usages, &powerpoint_embedding_policy(&self.embedding), &embedded)
    }

    /// The `.pptx` bytes with the option applied (`savePptx`), over a COPY of the package: with the option
    /// ON adds the families used that are not in the file yet; OFF removes the embedded ones. `faces`
    /// answers [`Editor::font_requests`] with the font files. The report is the `us-font-embedding` event.
    pub fn save_with_fonts(&self, faces: &dyn Fn(&FaceRequest) -> Option<Arc<[u8]>>) -> Result<SavedPackage, EditError> {
        let mut store = self.deck.parts().clone();
        let mut plan = uo_fonts::EmbeddingPlan::default();
        if self.embedding.embed {
            let usage = deck_font_usage(&store);
            let usages = as_usages(&usage);
            let (_, embedded) = read_pptx_embedding(&store);
            plan = uo_fonts::plan_font_embedding(
                &usages,
                &powerpoint_embedding_policy(&self.embedding),
                &embedded,
                faces,
                &FontLimits::DEFAULT,
            )
            .map_err(|e| EditError::Fonts(e.to_string().into()))?;
        }
        write_pptx_embedding(&mut store, &plan, &self.embedding)?;
        let bytes = store.save().map_err(|e| EditError::Deck(e.to_string().into()))?;
        let mut embedded: Vec<String> = Vec::new();
        for f in &plan.faces {
            if !embedded.contains(&f.family) {
                embedded.push(f.family.clone());
            }
        }
        let skipped = plan.skipped.iter().filter(|(_, r)| *r != uo_fonts::SkipReason::AlreadyEmbedded).cloned().collect();
        Ok(SavedPackage { bytes, embedded, skipped })
    }

    /// The `.pptx` bytes as they are (the package as a ZIP).
    pub fn save(&self) -> Result<Vec<u8>, PptxError> {
        self.deck.save()
    }

    /// What a stored option says (`readPptxEmbedding().settings`).
    pub fn stored_embedding(&self) -> StoredEmbedding {
        read_pptx_embedding(self.deck.parts()).0
    }
}

/// What [`Editor::save_with_fonts`] returns: the file and what the font option did.
#[derive(Clone, Debug)]
pub struct SavedPackage {
    pub bytes: Vec<u8>,
    /// the families embedded now (the plan's), in order
    pub embedded: Vec<String>,
    /// the families left out and why (what was already embedded is not listed)
    pub skipped: Vec<(String, uo_fonts::SkipReason)>,
}

const EMU_PER_PX: f64 = 9525.0;

fn target_index(t: Target) -> i64 {
    match t {
        Target::Slide(i) => i,
        Target::All => 0,
    }
}
