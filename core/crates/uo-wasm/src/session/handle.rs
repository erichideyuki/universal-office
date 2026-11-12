//! The session handle and its exports (see `session.rs` for the conventions). Every export is
//! `extern "C"`; input buffers come from `uo_alloc_bytes` and are consumed by the call (also when it
//! fails).

use std::cell::{Cell, OnceCell};
use std::rc::Rc;
use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};

use uo_docx::read::EmbeddedFont;
use uo_docx::session::font_usage::{
    EmbeddingPatch, FontSave, begin_save_with_fonts, finish_save_with_fonts, initial_embedding_settings, set_font_embedding,
};
use uo_docx::session::ops::format::{FormatCmd, FormatPainter};
use uo_docx::session::ops::text::TextCmd;
use uo_docx::session::{
    ChangeSet, Command, CommandError, DocPosition, ExecCx, HistoryLimits, JobId, JobResult, LocalNow, OpenError, OpenOptions, Outcome,
    Session, SessionOptions,
};
use uo_docx::text::Utf16;
use uo_docx_layout::LayoutOptions;
use uo_docx_layout::hit::{EditLines, EditPos, EditScope, LineIndex, SelRect};
use uo_docx_layout::options::{Clock, TimeZoneRule};
use uo_docx_layout::refs::Locator;
use uo_fonts::embed::{FaceRequest, FontEmbeddingSettings, WORD_DEFAULT_EMBEDDING};
use uo_fonts::hyphen::HyphenationRegistry;
use uo_fonts::registry::FontRegistry;
use uo_fonts::{FontLimits, Variant};

use super::caret::{Dir, move_caret};
use super::codec as w;
use super::env::{HostEnv, default_now};
use super::layout::{edit_scope, editor_options, lay_out_session, scope_key};
use super::oracle::LazyOracle;
use super::query;
use super::ring::with_ring;
use super::wire::{BindError, FromWire, IntoWire};
use crate::pipeline::{self, JobError, code};
use crate::render::{NONE, NULL_HANDLE, OK, UoBuf, buf, take_bytes};
use crate::write::{UoLayout, UoWriteEnv, env_parts, layout_generation, layout_geom, layout_handle, session_layout_options};

/// `UoBuf` status: there is no pending font save (`uo_session_font_requests` first).
pub const NO_FONT_SAVE: i32 = 2;
/// `UoBuf` status: the save failed (the last error says why).
pub const SAVE_FAILED: i32 = 3;
/// `UoBuf` status: a message that does not decode or bind, an argument out of its domain (UTF-8 detail).
pub const BAD_ARGUMENT: i32 = 5;

/// The host's fonts and hyphenation patterns (shared with the `UoWriteEnv` they came from: freeing
/// the environment does not invalidate the session).
pub struct HostFonts {
    pub registry: Arc<FontRegistry>,
    pub hyphenation: Arc<HyphenationRegistry>,
    pub phone: bool,
}

impl HostFonts {
    fn of(env: &UoWriteEnv) -> HostFonts {
        let (registry, hyphenation, phone) = env_parts(env);
        HostFonts { registry: Arc::clone(registry), hyphenation: Arc::clone(hyphenation), phone }
    }
}

/// The lines of the edited paragraphs in the shell's last layout (`layoutCache`), by layout and scope.
pub struct LinesCache {
    pub generation: u32,
    pub scope: String,
    pub lines: EditLines,
}

/// One open document being edited (see the module documentation of `session.rs`).
pub struct UoWriteSession {
    pub(crate) session: Session,
    pub(crate) embedded: Vec<EmbeddedFont>,
    pub(crate) fonts: Option<HostFonts>,
    pub(crate) now: Rc<Cell<LocalNow>>,
    pub(crate) font_embedding: FontEmbeddingSettings,
    pub(crate) painter: Option<FormatPainter>,
    pub(crate) font_save: Option<FontSave>,
    pub(crate) lines: Option<LinesCache>,
    /// test-only: the scripted layout oracle of the F10 replay (what the TS layout answered), instead of the lazy one
    #[cfg(all(feature = "session-selftest", not(test)))]
    pub(crate) test_oracle: Option<Box<crate::d2b_f10_support::replay::ReplayOracle>>,
}

static NEXT_SESSION: AtomicU32 = AtomicU32::new(1);

/// `Date` of a clock: ms since the epoch of the UTC parts, the offset of the local parts (minutes).
fn clock_of(now: LocalNow) -> Clock {
    // WS-D14: the one civil-date arithmetic of the session (its DATE fields use it too)
    let days = |y: i32, m: u32, d: u32| uo_docx::session::ops::tables::sort::days_from_civil(i64::from(y), i64::from(m), i64::from(d));
    let minutes = |p: uo_docx::session::DateParts| days(p.year, p.month, p.day) * 1440 + i64::from(p.hour) * 60 + i64::from(p.minute);
    let utc = minutes(now.utc);
    let offset = i32::try_from(minutes(now.local) - utc).unwrap_or(0);
    // minutes since the epoch: exact in an f64 for any i32 year
    let ms = f64::from(i32::try_from(utc.div_euclid(1440)).unwrap_or(0)) * 86_400_000.0
        + f64::from(i32::try_from(utc.rem_euclid(1440)).unwrap_or(0)) * 60_000.0;
    Clock { now_ms: ms, tz: TimeZoneRule::Fixed { offset_min: offset } }
}

impl UoWriteSession {
    fn new(
        session: Session,
        embedded: Vec<EmbeddedFont>,
        fonts: Option<HostFonts>,
        now: Rc<Cell<LocalNow>>,
        font_embedding: FontEmbeddingSettings,
    ) -> Self {
        UoWriteSession {
            session,
            embedded,
            fonts,
            now,
            font_embedding,
            painter: None,
            font_save: None,
            lines: None,
            #[cfg(all(feature = "session-selftest", not(test)))]
            test_oracle: None,
        }
    }

    /// The options of the oracle's layouts: the host's clock and loaded pattern sets, the editor's.
    fn oracle_options(&self) -> LayoutOptions {
        let base = LayoutOptions {
            clock: clock_of(self.now.get()),
            hyphenation_loaded: self
                .fonts
                .as_ref()
                .map(|f| f.hyphenation.ids().into_iter().map(Box::<str>::from).collect())
                .unwrap_or_default(),
            ..LayoutOptions::default()
        };
        editor_options(&self.session, &base)
    }

    /// Runs `f` with the command context: the lazy oracle and the instance's clipboard ring.
    pub(crate) fn run<R>(&mut self, f: impl FnOnce(&mut Session, &mut ExecCx<'_>) -> R) -> R {
        #[cfg(all(feature = "session-selftest", not(test)))]
        if let Some(o) = self.test_oracle.as_deref_mut() {
            let session = &mut self.session;
            return with_ring(|ring| {
                let mut cx = ExecCx { layout: o, clipboard: ring };
                f(session, &mut cx)
            });
        }
        let opts = self.oracle_options();
        let provider = OnceCell::new();
        let UoWriteSession { session, embedded, fonts, .. } = self;
        let mut oracle = LazyOracle::new(fonts.as_ref(), embedded, &opts, &provider);
        with_ring(|ring| {
            let mut cx = ExecCx { layout: &mut oracle, clipboard: ring };
            f(session, &mut cx)
        })
    }

    /// One command.
    pub(crate) fn command(&mut self, c: Command) -> Result<Outcome, CommandError> {
        self.lines = None;
        self.run(|s, cx| s.exec(c, cx))
    }

    /// One operation of `uo_session_exec`.
    pub(crate) fn exec_op(&mut self, op: w::ExecOp) -> Result<w::ExecOutcome, String> {
        let done =
            |cs: ChangeSet| -> Result<w::ExecOutcome, String> { Ok(w::ExecOutcome::Done(cs.into_wire().map_err(|e| e.to_string())?)) };
        match op {
            w::ExecOp::Command(c) => {
                let c = Command::from_wire(c).map_err(|e| e.to_string())?;
                outcome(self.command(c))
            }
            w::ExecOp::Undo => {
                self.lines = None;
                done(self.run(|s, cx| s.undo(cx)))
            }
            w::ExecOp::Redo => {
                self.lines = None;
                done(self.run(|s, cx| s.redo(cx)))
            }
            w::ExecOp::CancelPending => {
                self.session.cancel_pending();
                done(self.session.finish())
            }
            w::ExecOp::SetDirty(d) => done(self.session.set_dirty(d)),
            w::ExecOp::ApplyPainter => {
                let Some(p) = self.painter.clone() else { return Err("no painter: query CopyFormat first".to_owned()) };
                outcome(self.command(Command::Format(FormatCmd::ApplyPainter { painter: Box::new(p) })))
            }
            w::ExecOp::SetFontEmbedding(p) => {
                let patch = EmbeddingPatch { embed: p.embed, subset: p.subset, skip_common_fonts: p.skip_common_fonts };
                match set_font_embedding(&mut self.session, &mut self.font_embedding, patch) {
                    Some(cs) => done(cs),
                    None => done(self.session.finish()),
                }
            }
            w::ExecOp::SetClock(n) => {
                self.now.set(LocalNow::from_wire(n).map_err(|e| e.to_string())?);
                done(self.session.finish())
            }
            w::ExecOp::SetUiLang(l) => {
                self.session.set_ui_lang(FromWire::from_wire(l).map_err(|e: BindError| e.to_string())?);
                done(self.session.finish())
            }
        }
    }

    /// One query of `uo_session_query`.
    pub(crate) fn query(&mut self, q: w::QueryOp) -> Result<w::QueryAnswer, String> {
        let bind = |e: BindError| e.to_string();
        if let Some(a) = query::answer(&self.session, q.clone()).map_err(bind)? {
            return Ok(a);
        }
        Ok(match q {
            w::QueryOp::ClipboardItems => with_ring(|r| -> Result<w::QueryAnswer, BindError> {
                let items = r
                    .items()
                    .map(|it| Ok(w::ClipItem { id: it.id, text: it.clip.text.into_wire()?, at: it.at.into_wire()? }))
                    .collect::<Result<Vec<_>, BindError>>()?;
                Ok(w::QueryAnswer::ClipboardItems(w::ClipItems { version: r.version(), items }))
            })
            .map_err(bind)?,
            w::QueryOp::CopyFormat => {
                self.painter = self.session.copy_format();
                w::QueryAnswer::CopyFormat(self.painter.is_some())
            }
            w::QueryOp::FontUsage => w::QueryAnswer::FontUsage(
                self.session
                    .font_usage()
                    .into_iter()
                    .map(|u| w::FontUse {
                        family: u.family.encode_utf16().collect(),
                        variant: variant_wire(u.variant),
                        text: u.text.units().to_vec(),
                    })
                    .collect(),
            ),
            w::QueryOp::FontEmbedding => w::QueryAnswer::FontEmbedding(settings_wire(self.font_embedding)),
            _ => return Err("query".to_owned()),
        })
    }

    /// The lines of the edited paragraphs in `layout` (cached by layout and scope); `None` when the
    /// context has no scope (a textbox).
    pub(crate) fn lines(&mut self, l: &UoLayout) -> Option<&EditLines> {
        let generation = layout_generation(l);
        // wave 5, frontier S (WS-D6): a text box context has a borrowed scope, its chain; the cache key is the chain's
        // drawings (unique in the document: they name the outer story) and the header/footer page
        let chain = super::shell_stories::textbox_chain(&self.session);
        let key = match &chain {
            Some((c, page)) => {
                let ids: Vec<String> = c.iter().map(|d| d.0.to_string()).collect();
                format!("t{}@{}", ids.join("."), page.map_or(-1, i64::from))
            }
            None => scope_key(&self.session),
        };
        let fresh = self.lines.as_ref().is_some_and(|c| c.generation == generation && c.scope == key);
        if !fresh {
            let scope = match &chain {
                Some((c, page)) => EditScope::Textbox { chain: c, page: *page },
                None => edit_scope(&self.session)?,
            };
            let geom = layout_geom(l);
            let loc = Locator::new(self.session.view());
            let index = LineIndex::new(&loc, geom);
            let lines = EditLines::new(&loc, geom, &index, &scope);
            self.lines = Some(LinesCache { generation, scope: key, lines });
        }
        self.lines.as_ref().map(|c| &c.lines)
    }
}

pub(crate) fn outcome(r: Result<Outcome, CommandError>) -> Result<w::ExecOutcome, String> {
    let e = |x: BindError| x.to_string();
    Ok(match r {
        Ok(Outcome::Done(cs)) => w::ExecOutcome::Done(cs.into_wire().map_err(e)?),
        Ok(Outcome::NeedsJobs(jobs)) => w::ExecOutcome::Jobs(w::JobList { jobs: jobs.into_wire().map_err(e)? }),
        Err(err) => w::ExecOutcome::Error(err.into_wire().map_err(e)?),
    })
}

fn variant_wire(v: Variant) -> w::FontVariant {
    match v {
        Variant::Regular => w::FontVariant::Regular,
        Variant::Bold => w::FontVariant::Bold,
        Variant::Italic => w::FontVariant::Italic,
        Variant::BoldItalic => w::FontVariant::BoldItalic,
    }
}

fn settings_wire(s: FontEmbeddingSettings) -> w::EmbeddingSettings {
    w::EmbeddingSettings { embed: s.embed, subset: s.subset, skip_common_fonts: s.skip_common_fonts }
}

fn bad(detail: &str) -> *mut UoBuf {
    buf(BAD_ARGUMENT, detail.as_bytes().to_vec())
}

pub(crate) fn exec_result(r: Result<w::ExecOutcome, String>) -> *mut UoBuf {
    match r {
        Ok(outcome) => {
            let mut out = Vec::new();
            match w::encode_exec_result(&mut out, &w::Delta::default(), &w::ExecResult { outcome }) {
                Ok(()) => buf(OK, out),
                Err(e) => bad(&format!("encode: {e:?}")),
            }
        }
        Err(d) => bad(&d),
    }
}

/// # Safety
/// `h` must be null or a live handle.
unsafe fn session_mut<'a>(h: *mut UoWriteSession) -> Option<&'a mut UoWriteSession> {
    // SAFETY: the caller guarantees `h` is null or live.
    unsafe { h.as_mut() }
}

// ───────────────────────────── create and free ─────────────────────────────

fn open_options(m: w::SessionOpen) -> Result<(OpenOptions, LocalNow, u32), String> {
    let e = |x: BindError| x.to_string();
    let mut session = SessionOptions::default();
    if let Some(a) = m.author {
        session.author = String::from_wire(a).map_err(e)?.into_boxed_str();
    }
    if let Some(t) = m.tracking {
        session.tracking = t;
    }
    if let Some(mk) = m.markup {
        session.markup = FromWire::from_wire(mk).map_err(e)?;
    }
    let mut history = HistoryLimits::default();
    if let Some(n) = m.max_history_steps {
        history.max_steps = usize::try_from(n).unwrap_or(usize::MAX);
    }
    let now = match m.clock {
        Some(c) => LocalNow::from_wire(c).map_err(e)?,
        None => default_now(),
    };
    let o = OpenOptions {
        protected: m.protected,
        session,
        history,
        session_id: m.session_id.unwrap_or_else(|| NEXT_SESSION.fetch_add(1, Ordering::Relaxed)),
        lang: m.lang.map(String::from_wire).transpose().map_err(e)?.map(String::into_boxed_str),
        ..OpenOptions::default()
    };
    Ok((o, now, m.seed))
}

fn decode_open(bytes: &[u8]) -> Result<(OpenOptions, LocalNow, u32), String> {
    let m = w::decode_session_open(bytes, &mut w::Tables::new(), &w::limits()).map_err(|e| format!("{e:?}"))?;
    open_options(m)
}

fn open_failed(e: OpenError) {
    match e {
        OpenError::Docx(d) => {
            pipeline::fail(&JobError::Read(d));
        }
        // the `.doc` path (feature `doc` of uo-docx, never in this module's build)
        #[allow(unreachable_patterns)]
        _ => {
            pipeline::fail_with(code::INVALID_DATA, "the document does not open");
        }
    }
}

/// Opens a docx (`doc`, consumed) with the options of a `SessionOpen` message (`opts`, consumed): the
/// handle, or null with the last error set. `env` (nullable, borrowed) is the font environment of the
/// layout oracle and the session's layouts (`uo_session_set_env` changes it).
///
/// # Safety
/// `env` null or live; each `(ptr, len)` a buffer of `uo_alloc_bytes(len)` filled by JS.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn uo_session_open(
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
    match Session::open(doc, o, Box::new(host)) {
        Ok(session) => {
            let embedded = uo_docx::read::read_embedded_fonts(session.package()).unwrap_or_default();
            let settings = initial_embedding_settings(session.package());
            // SAFETY: the caller guarantees `env` is null or live.
            let fonts = unsafe { env.as_ref() }.map(HostFonts::of);
            Box::into_raw(Box::new(UoWriteSession::new(session, embedded, fonts, now, settings)))
        }
        Err(e) => {
            open_failed(e);
            std::ptr::null_mut()
        }
    }
}

/// A new document (`newDocument()`, the language of `SessionOpen.lang`): the handle, or null with the
/// last error set.
///
/// # Safety
/// As [`uo_session_open`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn uo_session_new_blank(env: *const UoWriteEnv, opts_ptr: *mut u8, opts_len: usize) -> *mut UoWriteSession {
    pipeline::clear_last();
    // SAFETY: the caller guarantees the buffer; it is consumed here.
    let opts = unsafe { take_bytes(opts_ptr, opts_len) };
    let (o, now, seed) = match decode_open(&opts) {
        Ok(x) => x,
        Err(d) => {
            pipeline::fail_with(code::ARGUMENT, &format!("open options: {d}"));
            return std::ptr::null_mut();
        }
    };
    let now = Rc::new(Cell::new(now));
    let host = HostEnv::new(Rc::clone(&now), seed);
    match Session::new_blank(o, Box::new(host)) {
        Ok(session) => {
            // SAFETY: the caller guarantees `env` is null or live.
            let fonts = unsafe { env.as_ref() }.map(HostFonts::of);
            Box::into_raw(Box::new(UoWriteSession::new(session, Vec::new(), fonts, now, WORD_DEFAULT_EMBEDDING)))
        }
        Err(e) => {
            open_failed(e);
            std::ptr::null_mut()
        }
    }
}

/// # Safety
/// `h` must be null or a live handle, released only once.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn uo_session_free(h: *mut UoWriteSession) {
    if !h.is_null() {
        // SAFETY: the caller guarantees `h` came from uo_session_open/new_blank and is released once.
        drop(unsafe { Box::from_raw(h) });
    }
}

/// The font environment of the session from now on (null: none; the layout family then answers
/// `CommandError::Layout`). The session keeps the fonts and patterns, not the handle.
///
/// # Safety
/// `h`, `env` null or live.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn uo_session_set_env(h: *mut UoWriteSession, env: *const UoWriteEnv) -> i32 {
    // SAFETY: the caller guarantees the handles are null or live.
    let Some(s) = (unsafe { session_mut(h) }) else { return code::ARGUMENT };
    // SAFETY: as above.
    s.fonts = unsafe { env.as_ref() }.map(HostFonts::of);
    code::OK
}

// ───────────────────────────── exec, complete, query ─────────────────────────────

/// An `Exec` message (consumed): an `ExecResult`.
///
/// # Safety
/// `h` null or live; `(ptr, len)` a buffer of `uo_alloc_bytes(len)` filled by JS.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn uo_session_exec(h: *mut UoWriteSession, ptr: *mut u8, len: usize) -> *mut UoBuf {
    // SAFETY: the caller guarantees the buffer; it is consumed here.
    let bytes = unsafe { take_bytes(ptr, len) };
    // SAFETY: the caller guarantees `h` is null or live.
    let Some(s) = (unsafe { session_mut(h) }) else { return buf(NULL_HANDLE, Vec::new()) };
    match w::decode_exec(&bytes, &mut w::Tables::new(), &w::limits()) {
        Ok(m) => exec_result(s.exec_op(m.op)),
        Err(e) => bad(&format!("decode: {e:?}")),
    }
}

/// A `Complete` message (consumed, the answer to a job of the pending command): an `ExecResult`.
///
/// # Safety
/// As [`uo_session_exec`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn uo_session_complete(h: *mut UoWriteSession, ptr: *mut u8, len: usize) -> *mut UoBuf {
    // SAFETY: the caller guarantees the buffer; it is consumed here.
    let bytes = unsafe { take_bytes(ptr, len) };
    // SAFETY: the caller guarantees `h` is null or live.
    let Some(s) = (unsafe { session_mut(h) }) else { return buf(NULL_HANDLE, Vec::new()) };
    let m = match w::decode_complete(&bytes, &mut w::Tables::new(), &w::limits()) {
        Ok(m) => m,
        Err(e) => return bad(&format!("decode: {e:?}")),
    };
    let result = match JobResult::from_wire(m.result) {
        Ok(r) => r,
        Err(e) => return bad(&e.to_string()),
    };
    s.lines = None;
    let r = s.run(|ss, cx| ss.complete(JobId(m.job), result, cx));
    exec_result(outcome(r))
}

/// A `Query` message (consumed): a `QueryResult`.
///
/// # Safety
/// As [`uo_session_exec`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn uo_session_query(h: *mut UoWriteSession, ptr: *mut u8, len: usize) -> *mut UoBuf {
    // SAFETY: the caller guarantees the buffer; it is consumed here.
    let bytes = unsafe { take_bytes(ptr, len) };
    // SAFETY: the caller guarantees `h` is null or live.
    let Some(s) = (unsafe { session_mut(h) }) else { return buf(NULL_HANDLE, Vec::new()) };
    let q = match w::decode_query(&bytes, &mut w::Tables::new(), &w::limits()) {
        Ok(m) => m.q,
        Err(e) => return bad(&format!("decode: {e:?}")),
    };
    match s.query(q) {
        Ok(r) => {
            let mut out = Vec::new();
            match w::encode_query_result(&mut out, &w::Delta::default(), &w::QueryResult { r }) {
                Ok(()) => buf(OK, out),
                Err(e) => bad(&format!("encode: {e:?}")),
            }
        }
        Err(d) => bad(&d),
    }
}

// ───────────────────────────── hot paths ─────────────────────────────

fn hot(h: *mut UoWriteSession, c: impl FnOnce() -> Command) -> *mut UoBuf {
    // SAFETY: the export's caller guarantees `h` is null or live.
    let Some(s) = (unsafe { session_mut(h) }) else { return buf(NULL_HANDLE, Vec::new()) };
    exec_result(outcome(s.command(c())))
}

/// Typed text (`type(text)`): UTF-16 code units, little endian (consumed). An `ExecResult`.
///
/// # Safety
/// As [`uo_session_exec`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn uo_session_type(h: *mut UoWriteSession, units_ptr: *mut u8, units_len: usize) -> *mut UoBuf {
    // SAFETY: the caller guarantees the buffer; it is consumed here.
    let bytes = unsafe { take_bytes(units_ptr, units_len) };
    if h.is_null() {
        return buf(NULL_HANDLE, Vec::new());
    }
    if bytes.len() % 2 != 0 {
        return bad("UTF-16 units: an odd byte count");
    }
    let units: Vec<u16> =
        bytes.chunks_exact(2).map(|c| u16::from_le_bytes([c.first().copied().unwrap_or(0), c.get(1).copied().unwrap_or(0)])).collect();
    hot(h, || Command::Text(TextCmd::Type { text: Utf16::from_units(units) }))
}

/// `pressEnter()`. An `ExecResult`.
///
/// # Safety
/// `h` null or live.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn uo_session_enter(h: *mut UoWriteSession) -> *mut UoBuf {
    hot(h, || Command::Text(TextCmd::Enter))
}

/// `pressBackspace()`. An `ExecResult`.
///
/// # Safety
/// `h` null or live.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn uo_session_backspace(h: *mut UoWriteSession) -> *mut UoBuf {
    hot(h, || Command::Text(TextCmd::Backspace))
}

/// `pressDelete()`. An `ExecResult`.
///
/// # Safety
/// `h` null or live.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn uo_session_delete(h: *mut UoWriteSession) -> *mut UoBuf {
    hot(h, || Command::Text(TextCmd::Delete))
}

/// `setSelection(anchor, focus)`. An `ExecResult`.
///
/// # Safety
/// `h` null or live.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn uo_session_set_selection(h: *mut UoWriteSession, a_para: u32, a_off: u32, f_para: u32, f_off: u32) -> *mut UoBuf {
    hot(h, || Command::Text(TextCmd::SetSelection { anchor: DocPosition::new(a_para, a_off), focus: DocPosition::new(f_para, f_off) }))
}

/// `moveCaret(dir, extend)` / `moveWord` (dir 0 left, 1 right, 2 up, 3 down, 4 home, 5 end, 6 word
/// left, 7 word right). `layout` (borrowed, nullable for 0, 1, 6, 7) is the shell's last layout of the
/// session. An `ExecResult`; status 1 when nothing moves (no paragraphs, no line), 5 for a direction
/// out of range or a line move without a layout.
///
/// # Safety
/// `h`, `layout` null or live.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn uo_session_move_caret(h: *mut UoWriteSession, layout: *const UoLayout, dir: u32, extend: u32) -> *mut UoBuf {
    // SAFETY: the caller guarantees `h` is null or live.
    let Some(s) = (unsafe { session_mut(h) }) else { return buf(NULL_HANDLE, Vec::new()) };
    let Some(d) = Dir::from_code(dir) else { return bad("direction") };
    // SAFETY: the caller guarantees `layout` is null or live.
    let l = unsafe { layout.as_ref() };
    if d.needs_lines() && l.is_none() {
        return bad("a line move needs the layout");
    }
    if let Some(l) = l {
        s.lines(l);
    }
    let UoWriteSession { session, lines, .. } = s;
    let el = if d.needs_lines() { lines.as_ref().map(|c| &c.lines) } else { None };
    match move_caret(session, el, d, extend != 0) {
        Some(cs) => exec_result(cs.into_wire().map(w::ExecOutcome::Done).map_err(|e| e.to_string())),
        None => buf(NONE, Vec::new()),
    }
}

/// The position at `(x, y)` (page twips) on `page` of `layout` in the edited paragraphs: a `Hit`;
/// status 1 when the context has no lines there (a textbox).
///
/// # Safety
/// `h`, `layout` null or live.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn uo_session_hit_test(h: *mut UoWriteSession, layout: *const UoLayout, page: u32, x: f64, y: f64) -> *mut UoBuf {
    // SAFETY: the caller guarantees the handles are null or live.
    let (Some(s), Some(l)) = (unsafe { session_mut(h) }, unsafe { layout.as_ref() }) else { return buf(NULL_HANDLE, Vec::new()) };
    let Some(el) = s.lines(l) else { return buf(NONE, Vec::new()) };
    let p = el.hit(page, x, y);
    let mut out = Vec::new();
    match w::encode_hit(&mut out, &w::Delta::default(), &w::Hit { para: p.para, offset: p.offset }) {
        Ok(()) => buf(OK, out),
        Err(e) => bad(&format!("encode: {e:?}")),
    }
}

/// The caret of the selection's focus in `layout`: a `Caret`; status 1 when it has no line.
///
/// # Safety
/// `h`, `layout` null or live.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn uo_session_caret_rect(h: *mut UoWriteSession, layout: *const UoLayout) -> *mut UoBuf {
    // SAFETY: the caller guarantees the handles are null or live.
    let (Some(s), Some(l)) = (unsafe { session_mut(h) }, unsafe { layout.as_ref() }) else { return buf(NULL_HANDLE, Vec::new()) };
    let f = s.session.selection().focus;
    let Some(el) = s.lines(l) else { return buf(NONE, Vec::new()) };
    let Some(c) = el.caret(EditPos { para: f.para, offset: f.offset }) else { return buf(NONE, Vec::new()) };
    let mut out = Vec::new();
    let m = w::Caret { page: c.page, x: c.x, y: c.y, h: c.h, xf: c.xf.map(|m| m.to_vec()) };
    match w::encode_caret(&mut out, &w::Delta::default(), &m) {
        Ok(()) => buf(OK, out),
        Err(e) => bad(&format!("encode: {e:?}")),
    }
}

/// The selection's rectangles on `page` of `layout`: a `SelRects` (empty without a range).
///
/// # Safety
/// `h`, `layout` null or live.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn uo_session_selection_rects(h: *mut UoWriteSession, layout: *const UoLayout, page: u32) -> *mut UoBuf {
    // SAFETY: the caller guarantees the handles are null or live.
    let (Some(s), Some(l)) = (unsafe { session_mut(h) }, unsafe { layout.as_ref() }) else { return buf(NULL_HANDLE, Vec::new()) };
    let r = s.session.range();
    let Some(el) = s.lines(l) else { return buf(NONE, Vec::new()) };
    let mut rects: Vec<SelRect> = Vec::new();
    if r.start != r.end {
        el.selection_rects(
            page,
            EditPos { para: r.start.para, offset: r.start.offset },
            EditPos { para: r.end.para, offset: r.end.offset },
            &mut rects,
        );
    }
    let m =
        w::SelRects { rects: rects.iter().map(|r| w::SelRect { x: r.x, y: r.y, w: r.w, h: r.h, xf: r.xf.map(|m| m.to_vec()) }).collect() };
    let mut out = Vec::new();
    match w::encode_sel_rects(&mut out, &w::Delta::default(), &m) {
        Ok(()) => buf(OK, out),
        Err(e) => bad(&format!("encode: {e:?}")),
    }
}

// ───────────────────────────── layout ─────────────────────────────

/// Lays the session's view out (the edit context's model, as the editor) with the options of
/// `uo_write_layout` (`flags` bits 4–6 = 0: the session's own revision view, `revisionView()`), and
/// answers an OWNED `UoLayout` (free it with `uo_layout_free`; every `uo_layout_*` / `uo_render_*`
/// export reads it). Null with the last error set on failure. The buffers are consumed; `env` is also
/// the session's font environment from now on.
///
/// # Safety
/// `h`, `env` null or live; each `(ptr, len)` a buffer of `uo_alloc_bytes(len)` filled by JS.
#[unsafe(no_mangle)]
#[allow(clippy::too_many_arguments)]
pub unsafe extern "C" fn uo_session_layout(
    h: *mut UoWriteSession,
    env: *const UoWriteEnv,
    now_ms: f64,
    tz_offset_min: i32,
    flags: u32,
    lang_ptr: *mut u8,
    lang_len: usize,
    hyph_ptr: *mut u8,
    hyph_len: usize,
    max_pages: u32,
) -> *mut UoLayout {
    pipeline::clear_last();
    // SAFETY: the caller guarantees the buffers; both are consumed here.
    let (lang, hyph) = unsafe { (take_bytes(lang_ptr, lang_len), take_bytes(hyph_ptr, hyph_len)) };
    // SAFETY: the caller guarantees the handles are null or live.
    let (Some(s), Some(env)) = (unsafe { session_mut(h) }, unsafe { env.as_ref() }) else {
        pipeline::fail(&JobError::Argument("session or environment"));
        return std::ptr::null_mut();
    };
    let Some(base) = session_layout_options(env, now_ms, tz_offset_min, flags, &lang, &hyph) else {
        pipeline::fail(&JobError::Argument("layout options"));
        return std::ptr::null_mut();
    };
    // WS-D15: the editor's options (`layoutOpts()`); an explicit revision mode of `flags` wins over the
    // session's own revision view
    let explicit = (flags >> crate::write::LAYOUT_REVISIONS_SHIFT) & 7 != 0;
    let keep = base.revisions.clone();
    let mut o = editor_options(&s.session, &base);
    if explicit {
        o.revisions = keep;
    }
    if max_pages > 0 {
        o.limits.max_pages = max_pages;
    }
    s.fonts = Some(HostFonts::of(env));
    let Some(fonts) = s.fonts.as_ref() else { return std::ptr::null_mut() };
    match lay_out_session(&s.session, &s.embedded, fonts, &o) {
        Ok(laid) => Box::into_raw(Box::new(layout_handle(laid, s.session.view(), fonts.phone, 0))),
        Err(e) => {
            pipeline::fail(&e);
            std::ptr::null_mut()
        }
    }
}

// ───────────────────────────── save ─────────────────────────────

/// `saveDocx()`: the docx bytes; status 3 with the last error set when the save fails.
///
/// # Safety
/// `h` null or live.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn uo_session_save(h: *mut UoWriteSession) -> *mut UoBuf {
    pipeline::clear_last();
    // SAFETY: the caller guarantees `h` is null or live.
    let Some(s) = (unsafe { session_mut(h) }) else { return buf(NULL_HANDLE, Vec::new()) };
    match s.session.save_docx() {
        Ok(b) => buf(OK, b),
        Err(e) => {
            pipeline::fail(&JobError::Read(e));
            buf(SAVE_FAILED, Vec::new())
        }
    }
}

/// Phase 1 of the save with fonts (`saveDocxWithFonts`): saves, keeps the saved package in the handle
/// and answers `FontRequests` (the settings and the faces to fetch, in order). Status 3 when the save
/// fails.
///
/// # Safety
/// `h` null or live.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn uo_session_font_requests(h: *mut UoWriteSession) -> *mut UoBuf {
    pipeline::clear_last();
    // SAFETY: the caller guarantees `h` is null or live.
    let Some(s) = (unsafe { session_mut(h) }) else { return buf(NULL_HANDLE, Vec::new()) };
    s.font_save = None;
    let f = match begin_save_with_fonts(&mut s.session, s.font_embedding) {
        Ok(f) => f,
        Err(e) => {
            pipeline::fail(&JobError::Read(e));
            return buf(SAVE_FAILED, Vec::new());
        }
    };
    let m = w::FontRequests {
        settings: settings_wire(f.settings),
        requests: f
            .requests
            .iter()
            .map(|r| w::FaceRequest { family: r.family.encode_utf16().collect(), variant: variant_wire(r.variant) })
            .collect(),
    };
    s.font_save = Some(f);
    let mut out = Vec::new();
    match w::encode_font_requests(&mut out, &w::Delta::default(), &m) {
        Ok(()) => buf(OK, out),
        Err(e) => bad(&format!("encode: {e:?}")),
    }
}

/// Phase 2: a `FontFaces` message (consumed: one answer per request in order, 16 random bytes per
/// request for the GUIDs) → `SaveResult` (the bytes and `uw-font-embedding`). The document's own
/// embedded face of a request wins over the host's (`documentFace ?? resolver`). Status 2 without a
/// pending phase 1, 5 for a message that does not match it, 3 when the plan or the save fails.
///
/// # Safety
/// As [`uo_session_exec`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn uo_session_save_with_fonts(h: *mut UoWriteSession, ptr: *mut u8, len: usize) -> *mut UoBuf {
    pipeline::clear_last();
    // SAFETY: the caller guarantees the buffer; it is consumed here.
    let bytes = unsafe { take_bytes(ptr, len) };
    // SAFETY: the caller guarantees `h` is null or live.
    let Some(s) = (unsafe { session_mut(h) }) else { return buf(NULL_HANDLE, Vec::new()) };
    let Some(pending) = s.font_save.as_ref() else { return buf(NO_FONT_SAVE, Vec::new()) };
    let wanted = pending.requests.len();
    // WS-D13: the answer is decoded and checked BEFORE the pending save is taken, so a malformed phase 2
    // leaves phase 1 alive and the host can answer again.
    let m = match w::decode_font_faces(&bytes, &mut w::Tables::new(), &w::limits()) {
        Ok(m) => m,
        Err(e) => return bad(&format!("decode: {e:?}")),
    };
    if m.faces.len() != wanted || m.guids.len() < 16 * wanted {
        return bad("one face answer and 16 GUID bytes per request");
    }
    let Some(f) = s.font_save.take() else { return buf(NO_FONT_SAVE, Vec::new()) };
    let host: Vec<Option<Arc<[u8]>>> = m.faces.into_iter().map(|a| a.bytes.map(Arc::from)).collect();
    let embedded = &s.embedded;
    let requests = &f.requests;
    let faces = |r: &FaceRequest| -> Option<Arc<[u8]>> {
        if let Some(e) = embedded.iter().find(|e| *e.family == *r.family && *e.variant == *r.variant.name()) {
            return Some(Arc::clone(&e.data));
        }
        let i = requests.iter().position(|q| q == r)?;
        host.get(i).cloned().flatten()
    };
    let mut at = 0usize;
    let guids = &m.guids;
    let mut guid = || {
        let mut g = [0u8; 16];
        if let Some(chunk) = guids.get(at..at + 16) {
            g.copy_from_slice(chunk);
        }
        at += 16;
        g
    };
    match finish_save_with_fonts(&f, &faces, &mut guid, &FontLimits::default()) {
        Ok((bytes, ev)) => {
            let events = match ev.into_wire() {
                Ok(e) => vec![e],
                Err(e) => return bad(&e.to_string()),
            };
            let mut out = Vec::new();
            match w::encode_save_result(&mut out, &w::Delta::default(), &w::SaveResult { bytes, events }) {
                Ok(()) => buf(OK, out),
                Err(e) => bad(&format!("encode: {e:?}")),
            }
        }
        Err(e) => {
            pipeline::fail(&JobError::Read(e));
            buf(SAVE_FAILED, Vec::new())
        }
    }
}

/// Drops the session's caches (M1 of the tests: the history pricing caches, the lines).
///
/// # Safety
/// `h` null or live.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn uo_session_clear_caches(h: *mut UoWriteSession) {
    // SAFETY: the caller guarantees `h` is null or live.
    if let Some(s) = unsafe { session_mut(h) } {
        s.session.clear_caches();
        s.lines = None;
        s.font_save = None;
    }
}
