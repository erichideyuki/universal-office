//! The exports the editor's TS shell needs over a session (milestone 2, "the editor switch"; record
//! `core/docs/modules/uo-bindings.md` "Milestone 2: the editor switch"). Additive to `handle.rs`:
//!
//! - [`UoShellEngine`]: one `LayoutEngine` with the incremental relayout of LA7 per open document, so
//!   that a keystroke lays out only what changed. `uo_shell_relayout` answers an owned `UoLayout` like
//!   `uo_session_layout` (every `uo_layout_*` / `uo_render_*` and caret export reads it) and
//!   `uo_shell_relayout_report` says which pages were laid out again (the shell repaints only those).
//! - The story queries of the shell's own code (`storyInfo`, `paraText`: Ctrl+End, the double-click word)
//!   and the hyphenation pattern ids of the session's document (the font loop of the viewer's
//!   `RustSession`), and the text/HTML of the last copy (`copied()`).
//!
//! Declared (ED-2): the layout options are built as `uo_session_layout` builds them (≈ 20 lines repeated
//! from `handle.rs`, which this frontier does not own); the engine's `Layout` is CLONED into the handle
//! after each relayout (the handle owns its pages; folding the engine into the session handle removes
//! the clone, a gate item). Conventions as `handle.rs`: input buffers consumed, null + the last error
//! for a handle-creating call, `UoBuf` statuses 0 ok, 1 none, 5 bad argument, 11 null handle.

use std::sync::Arc;

use uo_docx::DocView;
use uo_docx::session::text::para_length;
use uo_docx_layout::LayoutEngine;
use uo_docx_layout::fonts::FontProvider;
use uo_docx_layout::incr::{FallbackReason, IncrConfig, RelayoutPath};
use uo_docx_layout::run_boxes::RunFilter;
use uo_fonts::hyphen::HyphenationRegistry;

use super::handle::{BAD_ARGUMENT, HostFonts, UoWriteSession};
use super::layout::editor_options;
use super::oracle::font_provider;
use crate::pipeline::{self, JobError, LaidOut};
use crate::render::{NONE, NULL_HANDLE, OK, UoBuf, buf, take_bytes};
use crate::write::{UoLayout, UoWriteEnv, env_parts, layout_handle, session_layout_options};

/// The incremental layout engine of one edited document (owned by the shell; free it with
/// `uo_shell_engine_free`). It keeps the previous run (LA7 levels 1 and 2) between relayouts.
pub struct UoShellEngine {
    engine: LayoutEngine,
    /// the hyphenation registry the engine draws on (a new environment makes a new engine)
    hyph: Option<Arc<HyphenationRegistry>>,
    /// the font provider of the previous relayout, kept while its key holds: LA7 compares the
    /// provider's answers by face identity, and a provider built per call answers new faces (the
    /// relayout fell back to `Provider` on every keystroke, measured in tests/shell_native.rs)
    provider: Option<(ProviderKey, Arc<dyn FontProvider>)>,
}

/// What the kept provider was built from: the session, the registry and its generation (bumped by
/// every push and removal), the document's embedded fonts.
#[derive(Clone, Copy, PartialEq, Eq)]
struct ProviderKey {
    session: usize,
    registry: usize,
    generation: u64,
    embedded: usize,
}

impl UoShellEngine {
    fn new() -> UoShellEngine {
        UoShellEngine { engine: LayoutEngine::new().with_incremental(IncrConfig::default()), hyph: None, provider: None }
    }
}

/// A new engine (never null).
#[unsafe(no_mangle)]
pub extern "C" fn uo_shell_engine_new() -> *mut UoShellEngine {
    Box::into_raw(Box::new(UoShellEngine::new()))
}

/// # Safety
/// `e` must be null or a live handle, released only once.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn uo_shell_engine_free(e: *mut UoShellEngine) {
    if !e.is_null() {
        // SAFETY: the caller guarantees `e` came from uo_shell_engine_new and is released once.
        drop(unsafe { Box::from_raw(e) });
    }
}

/// Drops the engine's caches and incremental state (M1; the next relayout is a full one).
///
/// # Safety
/// `e` null or live.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn uo_shell_engine_clear(e: *mut UoShellEngine) {
    // SAFETY: the caller guarantees `e` is null or live.
    if let Some(e) = unsafe { e.as_mut() } {
        e.engine.clear_caches();
        e.provider = None;
    }
}

/// # Safety
/// `h` must be null or live.
unsafe fn session_mut<'a>(h: *mut UoWriteSession) -> Option<&'a mut UoWriteSession> {
    // SAFETY: the caller guarantees `h` is null or live.
    unsafe { h.as_mut() }
}

/// # Safety
/// `h` must be null or live.
unsafe fn session_ref<'a>(h: *const UoWriteSession) -> Option<&'a UoWriteSession> {
    // SAFETY: the caller guarantees `h` is null or live.
    unsafe { h.as_ref() }
}

/// Lays the session's view out through `e` (incremental when only body blocks changed in place, LA7)
/// with the options of `uo_session_layout` (same arguments, same meaning), and answers an OWNED
/// `UoLayout` (free it with `uo_layout_free`). Null with the last error set on failure. The buffers
/// are consumed; `env` becomes the session's font environment, as in `uo_session_layout`.
///
/// # Safety
/// `e`, `h`, `env` null or live; each `(ptr, len)` a buffer of `uo_alloc_bytes(len)` filled by JS.
#[unsafe(no_mangle)]
#[allow(clippy::too_many_arguments)]
pub unsafe extern "C" fn uo_shell_relayout(
    e: *mut UoShellEngine,
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
    let (Some(eng), Some(s), Some(env)) = (unsafe { e.as_mut() }, unsafe { session_mut(h) }, unsafe { env.as_ref() }) else {
        pipeline::fail(&JobError::Argument("engine, session or environment"));
        return std::ptr::null_mut();
    };
    let Some(base) = session_layout_options(env, now_ms, tz_offset_min, flags, &lang, &hyph) else {
        pipeline::fail(&JobError::Argument("layout options"));
        return std::ptr::null_mut();
    };
    // ED-2: the option code of `uo_session_layout` (handle.rs), repeated
    let explicit = (flags >> crate::write::LAYOUT_REVISIONS_SHIFT) & 7 != 0;
    let keep = base.revisions.clone();
    let mut o = editor_options(&s.session, &base);
    if explicit {
        o.revisions = keep;
    }
    if max_pages > 0 {
        o.limits.max_pages = max_pages;
    }
    let (registry, hyphenation, phone) = env_parts(env);
    s.fonts = Some(HostFonts { registry: Arc::clone(registry), hyphenation: Arc::clone(hyphenation), phone });
    let Some(fonts) = s.fonts.as_ref() else { return std::ptr::null_mut() };
    // the engine draws on the environment's patterns: a new environment, a new engine
    if !eng.hyph.as_ref().is_some_and(|k| Arc::ptr_eq(k, &fonts.hyphenation)) {
        eng.engine = LayoutEngine::with_hyphenation(Arc::clone(&fonts.hyphenation)).with_incremental(IncrConfig::default());
        eng.hyph = Some(Arc::clone(&fonts.hyphenation));
    }
    let key = ProviderKey {
        session: h as usize,
        registry: Arc::as_ptr(&fonts.registry) as usize,
        generation: fonts.registry.generation(),
        embedded: s.embedded.len(),
    };
    let provider = match &eng.provider {
        Some((k, p)) if *k == key => Arc::clone(p),
        _ => match font_provider(fonts, &s.embedded) {
            Ok(p) => {
                eng.provider = Some((key, Arc::clone(&p)));
                p
            }
            Err(d) => {
                eng.provider = None;
                pipeline::fail(&JobError::Argument(d));
                return std::ptr::null_mut();
            }
        },
    };
    let view = s.session.view();
    let layout = match eng.engine.relayout(view, &[], &*provider, &o) {
        Ok(l) => l.clone(),
        Err(err) => {
            pipeline::fail(&JobError::from(err));
            return std::ptr::null_mut();
        }
    };
    let navigation = layout.navigation(view);
    let links = layout.run_boxes(view, RunFilter::Links);
    let inlines = layout.run_boxes(view, RunFilter::CustomInlines);
    let laid = LaidOut { layout, navigation, links, inlines };
    Box::into_raw(Box::new(layout_handle(laid, view, phone, 0)))
}

/// The index of a fallback reason on the wire (`uo_shell_relayout_report`).
fn reason_code(r: FallbackReason) -> u8 {
    match r {
        FallbackReason::NoBase => 0,
        FallbackReason::Disabled => 1,
        FallbackReason::View => 2,
        FallbackReason::Options => 3,
        FallbackReason::Provider => 4,
        FallbackReason::Document => 5,
        FallbackReason::Context => 6,
        FallbackReason::Chains => 7,
        FallbackReason::Shape => 8,
        FallbackReason::Prologue => 9,
        FallbackReason::NoCheckpoint => 10,
        FallbackReason::PassInput => 11,
        FallbackReason::Unstable => 12,
        FallbackReason::Tables => 13,
        FallbackReason::Balancing => 14,
        FallbackReason::Memory => 15,
        FallbackReason::Failed => 16,
    }
}

/// What the last relayout of `e` did, little endian: u8 path (0 full, 1 incremental), u8 fallback
/// reason (`FallbackReason` in declaration order, 255 none), u32 first and u32 end of the pages laid
/// out again, u32 passes, i32 line shift (saturated), u32 memo hits, u32 memo misses (26 bytes).
/// Status 1 when no relayout ran.
///
/// # Safety
/// `e` null or live.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn uo_shell_relayout_report(e: *const UoShellEngine) -> *mut UoBuf {
    // SAFETY: the caller guarantees `e` is null or live.
    let Some(e) = (unsafe { e.as_ref() }) else { return buf(NULL_HANDLE, Vec::new()) };
    let Some(r) = e.engine.last_relayout() else { return buf(NONE, Vec::new()) };
    let mut out = Vec::with_capacity(26);
    out.push(match r.path {
        RelayoutPath::Full => 0,
        RelayoutPath::Incremental => 1,
    });
    out.push(r.reason.map_or(255, reason_code));
    out.extend_from_slice(&r.relaid_pages.start.to_le_bytes());
    out.extend_from_slice(&r.relaid_pages.end.to_le_bytes());
    out.extend_from_slice(&r.passes.to_le_bytes());
    let shift = i32::try_from(r.line_shift).unwrap_or(if r.line_shift < 0 { i32::MIN } else { i32::MAX });
    out.extend_from_slice(&shift.to_le_bytes());
    out.extend_from_slice(&r.memo.hits.to_le_bytes());
    out.extend_from_slice(&r.memo.misses.to_le_bytes());
    buf(OK, out)
}

fn u32_of(n: usize) -> u32 {
    u32::try_from(n).unwrap_or(u32::MAX)
}

/// The edit story of the session's context: u32 paragraph count, u32 UTF-16 length of the last
/// paragraph (8 bytes, little endian; `0, 0` when the context has no paragraphs).
///
/// # Safety
/// `h` null or live.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn uo_shell_story_info(h: *const UoWriteSession) -> *mut UoBuf {
    // SAFETY: the caller guarantees `h` is null or live.
    let Some(s) = (unsafe { session_ref(h) }) else { return buf(NULL_HANDLE, Vec::new()) };
    let (n, last) = match s.session.paras() {
        Some(p) => (p.len(), p.last().map_or(0, para_length)),
        None => (0, 0),
    };
    let mut out = Vec::with_capacity(8);
    out.extend_from_slice(&u32_of(n).to_le_bytes());
    out.extend_from_slice(&u32_of(last).to_le_bytes());
    buf(OK, out)
}

/// The text of paragraph `para` of the edit story (`paraText`), UTF-16 little endian; status 1 when
/// there is no such paragraph.
///
/// # Safety
/// `h` null or live.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn uo_shell_para_text(h: *const UoWriteSession, para: u32) -> *mut UoBuf {
    // SAFETY: the caller guarantees `h` is null or live.
    let Some(s) = (unsafe { session_ref(h) }) else { return buf(NULL_HANDLE, Vec::new()) };
    let Some(p) = usize::try_from(para).ok().and_then(|i| s.session.paras().and_then(|ps| ps.get(i))) else {
        return buf(NONE, Vec::new());
    };
    let units = uo_docx::session::text::para_text(p);
    let mut out = Vec::with_capacity(units.len() * 2);
    for u in units {
        out.extend_from_slice(&u.to_le_bytes());
    }
    buf(OK, out)
}

/// The hyphenation pattern ids the session's document needs and `env` does not have yet: the body of
/// `uo_write_hyphenation_patterns` over the session's document (`'\n'`-separated UTF-8 app languages,
/// consumed). Status 5 for languages that are not UTF-8.
///
/// # Safety
/// `h`, `env` null or live; `(ptr, len)` a buffer of `uo_alloc_bytes(len)` filled by JS.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn uo_shell_hyphenation_patterns(
    h: *const UoWriteSession,
    env: *const UoWriteEnv,
    ptr: *mut u8,
    len: usize,
) -> *mut UoBuf {
    // SAFETY: the caller guarantees the buffer; it is consumed here.
    let langs = unsafe { take_bytes(ptr, len) };
    // SAFETY: the caller guarantees the handles are null or live.
    let (Some(s), Some(env)) = (unsafe { session_ref(h) }, unsafe { env.as_ref() }) else { return buf(NULL_HANDLE, Vec::new()) };
    let Ok(app) = std::str::from_utf8(&langs) else { return buf(BAD_ARGUMENT, Vec::new()) };
    let app: Vec<String> = app.split('\n').filter(|l| !l.is_empty()).map(uo_jsnum::text::to_lowercase).collect();
    let all = uo_docx::word::document_hyphenation_languages(DocView::new(s.session.document()), "pt-BR", false);
    let wanted = all.iter().filter(|l| {
        let x = uo_jsnum::text::to_lowercase(l);
        app.iter().any(|z| x == *z || x.strip_prefix(z.as_str()).is_some_and(|r| r.starts_with('-')))
    });
    let (_, hyphenation, _) = env_parts(env);
    let ids = uo_fonts::hyphen::pattern_ids_to_load(wanted.map(|l| Some(&**l)), hyphenation);
    buf(OK, ids.join("\n").into_bytes())
}

/// The text and HTML of the last copy or cut (`copied()`): u32 UTF-16 unit count, the units (little
/// endian), then the HTML as UTF-8 (empty when the copy has none). Status 1 when nothing was copied.
///
/// # Safety
/// `h` null or live.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn uo_shell_copied(h: *const UoWriteSession) -> *mut UoBuf {
    // SAFETY: the caller guarantees `h` is null or live.
    let Some(s) = (unsafe { session_ref(h) }) else { return buf(NULL_HANDLE, Vec::new()) };
    let Some(c) = s.session.copied() else { return buf(NONE, Vec::new()) };
    let units = c.text.units();
    let html = c.html.as_deref().unwrap_or("");
    let mut out = Vec::with_capacity(4 + units.len() * 2 + html.len());
    out.extend_from_slice(&u32_of(units.len()).to_le_bytes());
    for u in units {
        out.extend_from_slice(&u.to_le_bytes());
    }
    out.extend_from_slice(html.as_bytes());
    buf(OK, out)
}
