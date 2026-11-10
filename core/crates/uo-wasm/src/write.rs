//! The Write read path for the browser and Node (milestone 1, feature `write`; record
//! `core/docs/modules/uo-bindings.md`): open a docx, lay it out with the `viewer` font policy over
//! the faces the host pushed, pull the pages as encoded render messages for the canvas painter
//! (through the view of uo-render frontier E), read the navigation and the run boxes, and write the
//! PDF in chunks the host collects.
//!
//! Handles: `UoWriteEnv` (fonts, face cache, hyphenation patterns: one per viewer), `UoWriteDocument`
//! (the model and its embedded fonts), `UoLayout` (one layout: the render view with the pages and
//! resources, the Write geometry, the navigation, the run boxes). Each is a `Box` released by its
//! `*_free` (JS: `free()` + `FinalizationRegistry`). A layout owns everything it reads: freeing the
//! document or the environment does not invalidate it.
//!
//! Conventions: input buffers come from `uo_alloc_bytes` and are **consumed** by the call (also when
//! it fails); a call that creates a handle returns null on failure and sets the last error
//! (`uo_last_error`, codes of `pipeline::code`); a call that answers bytes returns a `UoBuf` with the
//! status space of `render.rs` (0 ok, 1 none, 5 invalid argument, 6 no such page or item, 8 missing
//! resource, 11 null handle); a call that answers a status returns a `pipeline::code`.
//!
//! # Result layouts (little endian; strings are UTF-16 code units, `n u32` then `n x u16`)
//! - `uo_layout_encode_page`: the uo-render page message whose `app` is the Write section (`write.uos`
//!   `WritePage`: lines, chars, formats, objects, cells, and each body line's paragraph text and
//!   heading level for the text layer; G8). Decode with `js/gen/render.ts` then `js/gen/write.ts`.
//! - `uo_layout_annotations`: `n u32` comments, each `id f64`, `has_author u8` [+ the author string],
//!   the text string, `m u32` rects each `page u32, x f64, y f64, w f64, h f64` (twips); then `k u32`
//!   headings, each `level f64, page u32, y f64` and the text string (`collectAnnotations`, G8).
//! - `uo_layout_page_info`: `w f64, h f64` (twips), `section u32`, `number f64`, `hf_items i32` (-1 absent).
//! - `uo_layout_navigation`: `n u32` links, each `page u32, x f64, y f64, w f64, h f64` (points from the
//!   page top), `kind u8` then for 0 the URI string, for 1 `page u32, top f64`; then `m u32` outline
//!   entries, each `level u32, page u32, top f64` and the title string.
//! - `uo_layout_run_boxes`: `n u32`, each `page u32, x f64, y f64, w f64, h f64, baseline f64` (twips)
//!   and the value string (the link, or the plugin inline's type).
//! - `uo_layout_block_line`: `page u32, y f64` (twips), status 1 when the block has no line.
//! - `uo_layout_image_bytes` / `uo_layout_image_svg`: the bytes of image `i` / its SVG version; status 8 when
//!   there is none (no SVG), 6 for an unknown index.
//! - `uo_layout_warnings`: UTF-8 lines. `uo_layout_missing_faces`: UTF-8 lines `family\tbold\titalic`
//!   (`0`/`1`). `uo_write_env_take_missing`: UTF-8 lines `family\tregular,bold,…`.
//! - `uo_write_hyphenation_patterns`: UTF-8 lines, the pattern ids to fetch and register.
//!
//! # The PDF
//! `uo_layout_pdf(layout, flags, chunk)` writes the file through uo-pdf's `ChunkedSink`: every chunk
//! (`chunk` bytes, 1 MiB when 0, at least 1 KiB; the last one smaller) goes to the host import
//! `uo_host.pdf_chunk(ptr, len) -> i32`, which copies the bytes (and calls nothing back) and answers 0
//! to go on. The module never holds more than one chunk of the output, so its memory does not grow
//! with the file (wasm memory never shrinks). Answer: the bytes written, or `-code`.
//!
//! # Cancellation (G4)
//! A synchronous wasm call cannot see an `AbortSignal` or a flag the host changes, so the layout
//! asks. With `layout_flags` bit 9 (`LAYOUT_CANCEL_POLL`) it calls the host import
//! `uo_host.layout_poll() -> i32` at its first work charge and then about every
//! `CANCEL_POLL_UNITS` work units (a few ms); non-zero stops it and `uo_write_layout` fails with
//! code 10, stage none. The import must not call back into the module. The PDF is stoppable through
//! `pdf_chunk` answering non-zero (code 13, sink); `uo_layout_pdf` has no poll.

use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};

use uo_docx::DocView;
use uo_docx::read::ReadOptions;
use uo_docx_layout::Layout;
use uo_docx_layout::LayoutOptions;
use uo_docx_layout::annotations::Annotations;
use uo_docx_layout::section::{SectionError, SectionModel};
use uo_docx_layout::fonts::FontProvider;
use uo_docx_layout::options::{Clock, Hyphenation, RevisionMode, RevisionView, TimeZoneRule, ViewTransform};
use uo_docx_layout::run_boxes::RunBox;
use uo_fonts::hyphen::HyphenationRegistry;
use uo_fonts::policy::{PolicyName, provider};
use uo_fonts::registry::{AdvanceTable, FaceCache, FontProfile, FontRegistry, FontSource, HostFace, PushError};
use uo_fonts::{VARIANTS, Variant};
use uo_pdf::{ChunkedSink, PdfError};
use uo_render::view::RenderView;
use uo_render::{LinkTarget, Navigation};

use crate::pipeline::{self, JobError, LayoutJob, Opened, Trust, code};
use crate::render::{NONE, NULL_HANDLE, OK, Out, UoBuf, UoRenderView, buf, take_bytes, view_handle, view_of, view_of_mut};

const NO_PAGE: i32 = 6;
const NO_RESOURCE: i32 = 8;
const BAD_ARGUMENT: i32 = 5;
/// render.rs: the page did not encode
const NOT_ENCODED: i32 = 10;

/// The effect-layer cap of a phone (uo-render E-D12: the layer peaks at about 37 bytes a pixel).
pub const PHONE_MAX_FX_PX: u64 = 1 << 22;

// ───────────────────────────── the environment ─────────────────────────────

/// Fonts and hyphenation of one viewer.
pub struct UoWriteEnv {
    fonts: Arc<FontRegistry>,
    hyphenation: Arc<HyphenationRegistry>,
    phone: bool,
}

/// A new environment; `profile` 0 = desktop, 1 = phone (the face budgets of uo-fonts and a lower
/// effect-layer cap).
#[unsafe(no_mangle)]
pub extern "C" fn uo_write_env_new(profile: u32) -> *mut UoWriteEnv {
    let phone = profile == 1;
    let fonts = FontRegistry::with_profile(if phone { FontProfile::Phone } else { FontProfile::Desktop }, FaceCache::shared());
    Box::into_raw(Box::new(UoWriteEnv { fonts: Arc::new(fonts), hyphenation: Arc::new(HyphenationRegistry::new()), phone }))
}

/// # Safety
/// `env` must be null or a live handle, released only once.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn uo_write_env_free(env: *mut UoWriteEnv) {
    if !env.is_null() {
        // SAFETY: the caller guarantees `env` came from uo_write_env_new and is released once.
        drop(unsafe { Box::from_raw(env) });
    }
}

fn utf8(bytes: &[u8]) -> Option<&str> {
    std::str::from_utf8(bytes).ok()
}

fn variant(v: u32) -> Option<Variant> {
    VARIANTS.get(usize::try_from(v).ok()?).copied()
}

fn source(s: u32) -> Option<FontSource> {
    Some(match s {
        0 => FontSource::Host,
        1 => FontSource::LocalAccess,
        2 => FontSource::Bundled,
        3 => FontSource::Page,
        4 => FontSource::Installed,
        5 => FontSource::Substitute,
        6 => FontSource::Default,
        _ => return None,
    })
}

fn push_code(e: PushError) -> i32 {
    match e {
        PushError::OverBudget => code::LIMIT_EXCEEDED,
        PushError::Unreadable | PushError::NotAFont => code::INVALID_DATA,
    }
}

/// The faces of a `push_family` buffer: repeated `variant u8, ttc_index i32 (-1 none), len u32` and
/// the bytes.
fn parse_faces(mut b: &[u8]) -> Option<Vec<HostFace>> {
    let mut out = Vec::new();
    while !b.is_empty() {
        let (head, rest) = b.split_at_checked(9)?;
        let v = variant(u32::from(*head.first()?))?;
        let ttc = i32::from_le_bytes(head.get(1..5)?.try_into().ok()?);
        let len = usize::try_from(u32::from_le_bytes(head.get(5..9)?.try_into().ok()?)).ok()?;
        let (bytes, rest) = rest.split_at_checked(len)?;
        let mut f = HostFace::new(v, Arc::from(bytes));
        f.ttc_index = u32::try_from(ttc).ok();
        out.push(f);
        b = rest;
    }
    Some(out)
}

/// `resolver.getWithFile(family)` for the `viewer` policy: the family, where it came from
/// (0 host, 1 local font access, 2 bundled, 3 page, 4 installed, 5 substitute, 6 default), whether
/// its licence lets the PDF embed it, and its faces (see [`parse_faces`]). Consumes both buffers.
///
/// # Safety
/// `env` null or live; each `(ptr, len)` a buffer of `uo_alloc_bytes(len)` filled by JS.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn uo_write_env_push_family(
    env: *mut UoWriteEnv,
    family_ptr: *mut u8,
    family_len: usize,
    src: u32,
    embeddable: u32,
    faces_ptr: *mut u8,
    faces_len: usize,
) -> i32 {
    pipeline::clear_last();
    // SAFETY: the caller guarantees the buffers; both are consumed here.
    let (family, faces) = unsafe { (take_bytes(family_ptr, family_len), take_bytes(faces_ptr, faces_len)) };
    // SAFETY: the caller guarantees `env` is null or live.
    let Some(env) = (unsafe { env.as_ref() }) else { return pipeline::fail(&JobError::Argument("environment")) };
    let (Some(family), Some(src), Some(faces)) = (utf8(&family), source(src), parse_faces(&faces)) else {
        return pipeline::fail(&JobError::Argument("family, source or faces"));
    };
    match env.fonts.push_family(family, src, embeddable != 0, faces) {
        Ok(()) => code::OK,
        Err(e) => pipeline::fail_with(push_code(e), "the face was not pushed"),
    }
}

/// One hosted asset face (the viewer's bundled files), with its advance table (`*-advances.json`,
/// read only by the PDF; empty = none). Consumes every buffer.
///
/// # Safety
/// As [`uo_write_env_push_family`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn uo_write_env_push_asset(
    env: *mut UoWriteEnv,
    family_ptr: *mut u8,
    family_len: usize,
    var: u32,
    bytes_ptr: *mut u8,
    bytes_len: usize,
    advances_ptr: *mut u8,
    advances_len: usize,
) -> i32 {
    pipeline::clear_last();
    // SAFETY: the caller guarantees the buffers; all are consumed here.
    let (family, bytes, advances) =
        unsafe { (take_bytes(family_ptr, family_len), take_bytes(bytes_ptr, bytes_len), take_bytes(advances_ptr, advances_len)) };
    // SAFETY: the caller guarantees `env` is null or live.
    let Some(env) = (unsafe { env.as_ref() }) else { return pipeline::fail(&JobError::Argument("environment")) };
    let (Some(family), Some(v)) = (utf8(&family), variant(var)) else { return pipeline::fail(&JobError::Argument("family or variant")) };
    let mut face = HostFace::new(v, Arc::from(bytes));
    if !advances.is_empty() {
        let Some(table) = utf8(&advances).and_then(|t| AdvanceTable::parse_json(t).ok()) else {
            return pipeline::fail(&JobError::Argument("advance table"));
        };
        face.advances = Some(Arc::new(table));
    }
    match env.fonts.push_asset(family, face) {
        Ok(()) => code::OK,
        Err(e) => pipeline::fail_with(push_code(e), "the face was not pushed"),
    }
}

/// Drops every pushed face (a new document in the same viewer).
///
/// # Safety
/// `env` must be null or live.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn uo_write_env_clear_fonts(env: *mut UoWriteEnv) {
    // SAFETY: the caller guarantees `env` is null or live.
    if let Some(env) = unsafe { env.as_ref() } {
        env.fonts.clear();
    }
}

/// The TS `loadMissing` selection: what the layouts asked for and no pushed face answered (each
/// pair once per environment). The host fetches these (Local Font Access, the open substitutes),
/// pushes them and lays out again.
///
/// # Safety
/// `env` must be null or live.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn uo_write_env_take_missing(env: *mut UoWriteEnv) -> *mut UoBuf {
    // SAFETY: the caller guarantees `env` is null or live.
    let Some(env) = (unsafe { env.as_ref() }) else { return buf(NULL_HANDLE, Vec::new()) };
    let mut s = String::new();
    for m in env.fonts.take_missing() {
        s.push_str(&m.family);
        s.push('\t');
        s.push_str(&m.variants.iter().map(|v| v.name()).collect::<Vec<_>>().join(","));
        s.push('\n');
    }
    buf(OK, s.into_bytes())
}

/// Registers hyphenation patterns (the text of a `hyph-*.tex`/`.pat` set the host fetched; consumed).
///
/// # Safety
/// `env` null or live; `(ptr, len)` a buffer of `uo_alloc_bytes(len)` filled by JS.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn uo_write_env_register_hyphenation(env: *mut UoWriteEnv, ptr: *mut u8, len: usize) -> i32 {
    pipeline::clear_last();
    // SAFETY: the caller guarantees the buffer; it is consumed here.
    let text = unsafe { take_bytes(ptr, len) };
    // SAFETY: the caller guarantees `env` is null or live.
    let Some(env) = (unsafe { env.as_mut() }) else { return pipeline::fail(&JobError::Argument("environment")) };
    let Some(p) = utf8(&text).and_then(uo_fonts::hyphen::parse_patterns) else {
        return pipeline::fail_with(code::INVALID_DATA, "not a hyphenation pattern set");
    };
    // no layout holds the registry between calls (the sessions live for one layout)
    let Some(reg) = Arc::get_mut(&mut env.hyphenation) else { return pipeline::fail_with(code::INVALID_OPERATION, "patterns in use") };
    reg.register(&p);
    code::OK
}

// ───────────────────────────── documents ─────────────────────────────

/// An opened document.
pub struct UoWriteDocument {
    opened: Opened,
}

/// Opens docx bytes (consumed): the handle, or null with the last error set.
///
/// # Safety
/// `(ptr, len)` must be a buffer of `uo_alloc_bytes(len)` filled by JS.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn uo_write_open(ptr: *mut u8, len: usize) -> *mut UoWriteDocument {
    pipeline::clear_last();
    // SAFETY: the caller guarantees the buffer; it is consumed here.
    let bytes = unsafe { take_bytes(ptr, len) };
    match pipeline::open(bytes, &ReadOptions::default()) {
        Ok(opened) => Box::into_raw(Box::new(UoWriteDocument { opened })),
        Err(e) => {
            pipeline::fail(&e);
            std::ptr::null_mut()
        }
    }
}

/// # Safety
/// `doc` must be null or a live handle, released only once.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn uo_write_document_free(doc: *mut UoWriteDocument) {
    if !doc.is_null() {
        // SAFETY: the caller guarantees `doc` came from uo_write_open and is released once.
        drop(unsafe { Box::from_raw(doc) });
    }
}

/// The hyphenation pattern ids this document needs and the environment does not have yet: the
/// viewer's `hyphenationLangs(full)` — `documentHyphenationLanguages(full)` filtered by the app's
/// languages (`'\n'`-separated UTF-8, consumed; a document language matches an app language equal
/// to it or a prefix before `-`, case-insensitive) — mapped to pattern ids.
///
/// # Safety
/// `doc`, `env` null or live; `(ptr, len)` a buffer of `uo_alloc_bytes(len)` filled by JS.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn uo_write_hyphenation_patterns(
    doc: *const UoWriteDocument,
    env: *const UoWriteEnv,
    ptr: *mut u8,
    len: usize,
) -> *mut UoBuf {
    // SAFETY: the caller guarantees the buffer; it is consumed here.
    let langs = unsafe { take_bytes(ptr, len) };
    // SAFETY: the caller guarantees the handles are null or live.
    let (Some(doc), Some(env)) = (unsafe { doc.as_ref() }, unsafe { env.as_ref() }) else { return buf(NULL_HANDLE, Vec::new()) };
    let Some(app) = utf8(&langs) else { return buf(BAD_ARGUMENT, Vec::new()) };
    let app: Vec<String> = app.split('\n').filter(|l| !l.is_empty()).map(uo_jsnum::text::to_lowercase).collect();
    let all = uo_docx::word::document_hyphenation_languages(DocView::new(&doc.opened.doc), "pt-BR", false);
    let wanted = all.iter().filter(|l| {
        let x = uo_jsnum::text::to_lowercase(l);
        app.iter().any(|z| x == *z || x.strip_prefix(z.as_str()).is_some_and(|r| r.starts_with('-')))
    });
    let ids = uo_fonts::hyphen::pattern_ids_to_load(wanted.map(|l| Some(&**l)), &env.hyphenation);
    buf(OK, ids.join("\n").into_bytes())
}

// ───────────────────────────── layouts ─────────────────────────────

/// One layout: the render view (pages and resources), the Write geometry, what the Write section
/// reads from the model (G8), the navigation, the run boxes and the annotations.
pub struct UoLayout {
    view: UoRenderView,
    geom: Layout,
    /// each body line's paragraph text and level, the placed objects' run names (gathered once)
    section: SectionModel,
    /// the layout id of its messages (the render view's mark has the same)
    generation: u32,
    navigation: Navigation,
    links: Vec<RunBox>,
    inlines: Vec<RunBox>,
    annotations: Annotations,
}

/// `layout_flags` bits 0–1: hyphenation 0 on, 1 off, 2 force.
pub const LAYOUT_HYPHENATION_MASK: u32 = 3;
/// bits 2–3: view 0 print, 1 web (`web_width_tw`), 2 draft.
pub const LAYOUT_VIEW_SHIFT: u32 = 2;
/// bits 4–6: tracked changes 0 default (final, no marks), 1 final, 2 simple, 3 all, 4 original.
pub const LAYOUT_REVISIONS_SHIFT: u32 = 4;
/// bit 8: the `hyph_langs` list is given (`hyphenationLanguages`).
pub const LAYOUT_HYPHENATION_LANGUAGES: u32 = 1 << 8;
/// bit 9: poll the host import `uo_host.layout_poll()` during the layout; non-zero cancels (G4).
pub const LAYOUT_CANCEL_POLL: u32 = 1 << 9;

static GENERATION: AtomicU32 = AtomicU32::new(1);

fn layout_options(
    env: &UoWriteEnv,
    now_ms: f64,
    tz_offset_min: i32,
    flags: u32,
    web_width_tw: f64,
    lang: &[u8],
    hyph: &[u8],
) -> Option<LayoutOptions> {
    let mut o = LayoutOptions {
        clock: Clock { now_ms, tz: TimeZoneRule::Fixed { offset_min: tz_offset_min } },
        hyphenation_loaded: env.hyphenation.ids().into_iter().map(Box::<str>::from).collect(),
        ..LayoutOptions::default()
    };
    let lang = utf8(lang)?;
    if !lang.is_empty() {
        o.lang = lang.into();
    }
    o.hyphenation = match flags & LAYOUT_HYPHENATION_MASK {
        0 => Hyphenation::On,
        1 => Hyphenation::Off,
        2 => Hyphenation::Force,
        _ => return None,
    };
    o.view = match (flags >> LAYOUT_VIEW_SHIFT) & 3 {
        0 => ViewTransform::Print,
        1 => ViewTransform::Web { width_tw: web_width_tw },
        2 => ViewTransform::Draft,
        _ => return None,
    };
    let mode = match (flags >> LAYOUT_REVISIONS_SHIFT) & 7 {
        0 => None,
        1 => Some(RevisionMode::Final),
        2 => Some(RevisionMode::Simple),
        3 => Some(RevisionMode::All),
        4 => Some(RevisionMode::Original),
        _ => return None,
    };
    o.revisions = mode.map(|mode| RevisionView { mode, ..RevisionView::default() });
    if flags & LAYOUT_CANCEL_POLL != 0 {
        o.cancel_poll = Some(host::layout_poll);
    }
    if flags & LAYOUT_HYPHENATION_LANGUAGES != 0 {
        o.hyphenation_languages = Some(utf8(hyph)?.split('\n').filter(|l| !l.is_empty()).map(Box::<str>::from).collect());
    }
    Some(o)
}

/// Lays the document out (`layoutDocument(view(full), withEmbeddedFonts(viewerProvider), options)`):
/// the handle, or null with the last error set. `now_ms`/`tz_offset_min` are the clock of the date
/// fields (the host's `Date.now()` and `-getTimezoneOffset()`); `lang` (UTF-8, empty = `pt-BR`) and
/// `hyph_langs` (`'\n'`-separated UTF-8) are consumed; `max_pages` 0 = the default limit; `generation` is the layout
/// id stamped on its page messages (0 = a counter of the module).
///
/// # Safety
/// `doc`, `env` null or live; each `(ptr, len)` a buffer of `uo_alloc_bytes(len)` filled by JS.
#[unsafe(no_mangle)]
#[allow(clippy::too_many_arguments)]
pub unsafe extern "C" fn uo_write_layout(
    doc: *const UoWriteDocument,
    env: *const UoWriteEnv,
    now_ms: f64,
    tz_offset_min: i32,
    flags: u32,
    web_width_tw: f64,
    lang_ptr: *mut u8,
    lang_len: usize,
    hyph_ptr: *mut u8,
    hyph_len: usize,
    max_pages: u32,
    generation: u32,
) -> *mut UoLayout {
    pipeline::clear_last();
    // SAFETY: the caller guarantees the buffers; both are consumed here.
    let (lang, hyph) = unsafe { (take_bytes(lang_ptr, lang_len), take_bytes(hyph_ptr, hyph_len)) };
    // SAFETY: the caller guarantees the handles are null or live.
    let (Some(doc), Some(env)) = (unsafe { doc.as_ref() }, unsafe { env.as_ref() }) else {
        pipeline::fail(&JobError::Argument("document or environment"));
        return std::ptr::null_mut();
    };
    let Some(mut o) = layout_options(env, now_ms, tz_offset_min, flags, web_width_tw, &lang, &hyph) else {
        pipeline::fail(&JobError::Argument("layout options"));
        return std::ptr::null_mut();
    };
    if max_pages > 0 {
        o.limits.max_pages = max_pages;
    }
    let p: Arc<dyn FontProvider> = match provider(&env.fonts, PolicyName::Viewer) {
        Ok(p) => Arc::from(p),
        Err(_) => {
            pipeline::fail(&JobError::Argument("font policy"));
            return std::ptr::null_mut();
        }
    };
    let cache = Arc::clone(env.fonts.cache());
    let job = LayoutJob {
        provider: p,
        cache: &cache,
        ledger: Some(env.fonts.ledger()),
        hyphenation: &env.hyphenation,
        options: &o,
        cancel: None,
        run_boxes: true,
    };
    match pipeline::lay_out(&doc.opened, &job) {
        Ok(laid) => {
            let mut geom = laid.layout;
            // the viewer reads the paragraphs of `this.full`, not of the view transform (the Web
            // view keeps the blocks): the model as opened
            let model = DocView::new(&doc.opened.doc);
            let section = SectionModel::new(model, &geom);
            let annotations = geom.annotations(model);
            let pages = pipeline::take_pages(&mut geom);
            let res = std::mem::take(&mut geom.resources);
            let generation = if generation == 0 { GENERATION.fetch_add(1, Ordering::Relaxed) } else { generation };
            let mut view = RenderView::new(generation, res, pages);
            if env.phone {
                view.limits.max_fx_px = view.limits.max_fx_px.min(PHONE_MAX_FX_PX);
            }
            Box::into_raw(Box::new(UoLayout {
                view: view_handle(view),
                geom,
                section,
                generation,
                navigation: laid.navigation,
                links: laid.links,
                inlines: laid.inlines,
                annotations,
            }))
        }
        Err(e) => {
            pipeline::fail(&e);
            std::ptr::null_mut()
        }
    }
}

/// # Safety
/// `l` must be null or a live handle, released only once (its view goes with it).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn uo_layout_free(l: *mut UoLayout) {
    if !l.is_null() {
        // SAFETY: the caller guarantees `l` came from uo_write_layout and is released once.
        drop(unsafe { Box::from_raw(l) });
    }
}

/// # Safety
/// `l` must be null or live.
unsafe fn layout_ref<'a>(l: *const UoLayout) -> Option<&'a UoLayout> {
    // SAFETY: the caller guarantees `l` is null or live.
    unsafe { l.as_ref() }
}

/// # Safety
/// `l` must be null or a live handle.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn uo_layout_page_count(l: *const UoLayout) -> usize {
    // SAFETY: forwarded.
    unsafe { layout_ref(l) }.map_or(0, |l| view_of(&l.view).page_count())
}

/// The render view of the layout, for the `uo_render_*` paint exports: **borrowed** — valid until the
/// layout is freed, never given to `uo_render_view_free`.
///
/// # Safety
/// `l` must be null or a live handle.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn uo_layout_view(l: *mut UoLayout) -> *mut UoRenderView {
    // SAFETY: the caller guarantees `l` is null or live.
    unsafe { l.as_mut() }.map_or(std::ptr::null_mut(), |l| &raw mut l.view)
}

/// The message of page `i` for the canvas painter: uo-render's page message (items, resource
/// deltas since the last page this layout encoded, hints) with the **Write section** as its `app`
/// (G8; uo-docx-layout `Layout::encode_page`). Status 6 past the last page, 10 when it does not
/// encode (render.rs codes).
///
/// # Safety
/// `l` must be null or a live handle.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn uo_layout_encode_page(l: *mut UoLayout, page: usize) -> *mut UoBuf {
    // SAFETY: the caller guarantees `l` is null or live.
    let Some(l) = (unsafe { l.as_mut() }) else { return buf(NULL_HANDLE, Vec::new()) };
    let app = match l.geom.write_section(page, &l.section, l.generation) {
        Ok(a) => a,
        Err(SectionError::NoPage) => return buf(NO_PAGE, Vec::new()),
        Err(_) => return buf(NOT_ENCODED, Vec::new()),
    };
    // the view carries the section for this one encoding (encoded on demand, never kept)
    let v = view_of_mut(&mut l.view);
    if v.set_app(page, app.0).is_err() {
        return buf(NO_PAGE, Vec::new());
    }
    let mut out = Vec::new();
    let r = v.encode_page(page, &mut out);
    let _ = v.set_app(page, Vec::new());
    match r {
        Ok(()) => buf(OK, out),
        Err(uo_render::ViewError::NoPage) => buf(NO_PAGE, Vec::new()),
        Err(_) => buf(NOT_ENCODED, Vec::new()),
    }
}

/// The viewer's annotations (`collectAnnotations(full, layout)`): the comments with the
/// rectangles of their text, in reading order, and the headings of the outline pane.
///
/// # Safety
/// `l` must be null or a live handle.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn uo_layout_annotations(l: *const UoLayout) -> *mut UoBuf {
    // SAFETY: forwarded.
    let Some(l) = (unsafe { layout_ref(l) }) else { return buf(NULL_HANDLE, Vec::new()) };
    let a = &l.annotations;
    let mut o = Out::default();
    o.u32(a.comments.len());
    for c in &a.comments {
        o.f64(c.id);
        match &c.author {
            Some(au) => {
                o.u8(1);
                units(&mut o, au);
            }
            None => {
                o.u8(0);
            }
        }
        units(&mut o, &c.text);
        o.u32(c.rects.len());
        for r in &c.rects {
            o.u32(usize::try_from(r.page).unwrap_or(usize::MAX)).f64(r.x).f64(r.y).f64(r.w).f64(r.h);
        }
    }
    o.u32(a.headings.len());
    for h in &a.headings {
        o.f64(h.level).u32(usize::try_from(h.page).unwrap_or(usize::MAX)).f64(h.y);
        units(&mut o, &h.text);
    }
    buf(OK, o.0)
}

/// Size, section, displayed number and header/footer item count of a page.
///
/// # Safety
/// `l` must be null or a live handle.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn uo_layout_page_info(l: *const UoLayout, page: usize) -> *mut UoBuf {
    // SAFETY: forwarded.
    let Some(l) = (unsafe { layout_ref(l) }) else { return buf(NULL_HANDLE, Vec::new()) };
    let (Some(p), Some(g)) = (view_of(&l.view).page(page), l.geom.pages.get(page)) else { return buf(NO_PAGE, Vec::new()) };
    let mut o = Out::default();
    o.f64(p.w).f64(p.h).u32(usize::try_from(g.section).unwrap_or(usize::MAX)).f64(g.number);
    o.bytes(&g.hf_items.map_or(-1, |n| i32::try_from(n).unwrap_or(i32::MAX)).to_le_bytes());
    buf(OK, o.0)
}

fn units(o: &mut Out, u: &[u16]) {
    o.u32(u.len());
    for c in u {
        o.bytes(&c.to_le_bytes());
    }
}

/// The links (rectangles in points, URI or destination) and the outline of the layout:
/// `pdfNavigation(full, layout)`.
///
/// # Safety
/// `l` must be null or a live handle.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn uo_layout_navigation(l: *const UoLayout) -> *mut UoBuf {
    // SAFETY: forwarded.
    let Some(l) = (unsafe { layout_ref(l) }) else { return buf(NULL_HANDLE, Vec::new()) };
    let n = &l.navigation;
    let mut o = Out::default();
    o.u32(n.links.len());
    for pl in &n.links {
        let k = &pl.link;
        o.u32(usize::try_from(pl.page).unwrap_or(usize::MAX)).f64(k.x_pt).f64(k.y_top_pt).f64(k.w_pt).f64(k.h_pt);
        match &k.target {
            LinkTarget::Uri(u) => {
                o.u8(0);
                units(&mut o, u);
            }
            LinkTarget::Dest { page_index, dest_top_pt } => {
                o.u8(1).u32(usize::try_from(*page_index).unwrap_or(usize::MAX)).f64(*dest_top_pt);
            }
        }
    }
    o.u32(n.outline.len());
    for e in &n.outline {
        o.u32(usize::try_from(e.level).unwrap_or(usize::MAX)).u32(usize::try_from(e.page_index).unwrap_or(usize::MAX)).f64(e.top_pt);
        units(&mut o, &e.title);
    }
    buf(OK, o.0)
}

/// `runBoxes`: `filter` 0 = links, 1 = plugin inlines (twips).
///
/// # Safety
/// `l` must be null or a live handle.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn uo_layout_run_boxes(l: *const UoLayout, filter: u32) -> *mut UoBuf {
    // SAFETY: forwarded.
    let Some(l) = (unsafe { layout_ref(l) }) else { return buf(NULL_HANDLE, Vec::new()) };
    let boxes = match filter {
        0 => &l.links,
        1 => &l.inlines,
        _ => return buf(BAD_ARGUMENT, Vec::new()),
    };
    let mut o = Out::default();
    o.u32(boxes.len());
    for b in boxes {
        o.u32(usize::try_from(b.page).unwrap_or(usize::MAX)).f64(b.x).f64(b.y).f64(b.w).f64(b.h).f64(b.baseline);
        units(&mut o, &b.value);
    }
    buf(OK, o.0)
}

/// `blockLine(layout, block)`: the page and top of the first line of body block `block`.
///
/// # Safety
/// `l` must be null or a live handle.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn uo_layout_block_line(l: *const UoLayout, block: f64) -> *mut UoBuf {
    // SAFETY: forwarded.
    let Some(l) = (unsafe { layout_ref(l) }) else { return buf(NULL_HANDLE, Vec::new()) };
    match uo_docx_layout::navigation::block_line(&l.geom, block) {
        Some((page, y)) => {
            let mut o = Out::default();
            o.u32(usize::try_from(page).unwrap_or(usize::MAX)).f64(y);
            buf(OK, o.0)
        }
        None => buf(NONE, Vec::new()),
    }
}

/// The layout's warnings (UTF-8 lines, the TS `Set` order).
///
/// # Safety
/// `l` must be null or a live handle.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn uo_layout_warnings(l: *const UoLayout) -> *mut UoBuf {
    // SAFETY: forwarded.
    let Some(l) = (unsafe { layout_ref(l) }) else { return buf(NULL_HANDLE, Vec::new()) };
    buf(OK, l.geom.warnings.iter().map(|w| &**w).collect::<Vec<_>>().join("\n").into_bytes())
}

/// The faces the provider did not have during this layout (`family\tbold\titalic` lines).
///
/// # Safety
/// `l` must be null or a live handle.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn uo_layout_missing_faces(l: *const UoLayout) -> *mut UoBuf {
    // SAFETY: forwarded.
    let Some(l) = (unsafe { layout_ref(l) }) else { return buf(NULL_HANDLE, Vec::new()) };
    let mut s = String::new();
    for f in &l.geom.missing_faces {
        s.push_str(&format!("{}\t{}\t{}\n", f.family, u8::from(f.bold), u8::from(f.italic)));
    }
    buf(OK, s.into_bytes())
}

/// A copy of the file of face `face` of the layout (for the browser's `FontFace`: the document's
/// embedded fonts are known only here). Status 8 when the face has no file.
///
/// # Safety
/// `l` must be null or a live handle.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn uo_layout_face_bytes(l: *const UoLayout, face: usize) -> *mut UoBuf {
    // SAFETY: forwarded.
    let Some(l) = (unsafe { layout_ref(l) }) else { return buf(NULL_HANDLE, Vec::new()) };
    match view_of(&l.view).resources().faces.get(face) {
        Some(f) => f.bytes.as_ref().map_or_else(|| buf(NO_RESOURCE, Vec::new()), |b| buf(OK, b.to_vec())),
        None => buf(NO_PAGE, Vec::new()),
    }
}

/// A copy of the bytes of image `image` of the layout.
///
/// # Safety
/// `l` must be null or a live handle.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn uo_layout_image_bytes(l: *const UoLayout, image: usize) -> *mut UoBuf {
    // SAFETY: forwarded.
    let Some(l) = (unsafe { layout_ref(l) }) else { return buf(NULL_HANDLE, Vec::new()) };
    match view_of(&l.view).resources().images.get(image) {
        Some(im) => buf(OK, im.bytes.to_vec()),
        None => buf(NO_PAGE, Vec::new()),
    }
}

/// A copy of the SVG version of image `image` of the layout (the vector the viewer paints in place
/// of the raster when the document has one). Status 8 when the image has no SVG, 6 for an unknown
/// index.
///
/// # Safety
/// `l` must be null or a live handle.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn uo_layout_image_svg(l: *const UoLayout, image: usize) -> *mut UoBuf {
    // SAFETY: forwarded.
    let Some(l) = (unsafe { layout_ref(l) }) else { return buf(NULL_HANDLE, Vec::new()) };
    match view_of(&l.view).resources().images.get(image) {
        Some(im) => im.svg.as_ref().map_or_else(|| buf(NO_RESOURCE, Vec::new()), |b| buf(OK, b.to_vec())),
        None => buf(NO_PAGE, Vec::new()),
    }
}

/// `uo_layout_pdf` flags: compress the content streams.
pub const PDF_COMPRESS: u32 = 1;
/// `uo_layout_pdf` flags: an untrusted document (lower effect and image caps).
pub const PDF_UNTRUSTED: u32 = 2;

#[cfg(target_arch = "wasm32")]
mod host {
    #[link(wasm_import_module = "uo_host")]
    unsafe extern "C" {
        /// Copies `len` bytes at `ptr` of this module's memory; 0 = go on.
        fn pdf_chunk(ptr: *const u8, len: usize) -> i32;
        /// 0 = go on, non-zero cancels the layout (G4); calls nothing back.
        #[link_name = "layout_poll"]
        fn layout_poll_import() -> i32;
    }

    pub(super) fn layout_poll() -> bool {
        // SAFETY: no arguments; the import calls nothing back into the module.
        unsafe { layout_poll_import() != 0 }
    }

    pub(super) fn chunk(b: &[u8]) -> bool {
        // SAFETY: the import reads `b.len()` bytes at `b.as_ptr()` of this module's memory during
        // the call (the contract above: it copies them and calls nothing back into the module).
        unsafe { pdf_chunk(b.as_ptr(), b.len()) == 0 }
    }
}

/// The native stand-in of the host import (tests): a per-thread callback.
#[cfg(not(target_arch = "wasm32"))]
pub mod host {
    use std::cell::RefCell;

    type Sink = Box<dyn FnMut(&[u8]) -> bool>;

    thread_local! {
        static SINK: RefCell<Option<Sink>> = const { RefCell::new(None) };
    }

    /// Where the chunks of `uo_layout_pdf` go on this thread (`None`: dropped).
    pub fn set_pdf_sink(f: Option<Sink>) {
        SINK.with(|s| *s.borrow_mut() = f);
    }

    pub(super) fn chunk(b: &[u8]) -> bool {
        SINK.with(|s| s.borrow_mut().as_mut().is_none_or(|f| f(b)))
    }

    type Poll = Box<dyn FnMut() -> bool>;

    thread_local! {
        static POLL: RefCell<Option<Poll>> = const { RefCell::new(None) };
    }

    /// The answer of `uo_host.layout_poll` on this thread (`None`: never cancels); `true` cancels (G4).
    pub fn set_layout_poll(f: Option<Poll>) {
        POLL.with(|p| *p.borrow_mut() = f);
    }

    pub(super) fn layout_poll() -> bool {
        POLL.with(|p| p.borrow_mut().as_mut().is_some_and(|f| f()))
    }
}

/// The PDF of the layout (`renderPdf(layout, { navigation })`), streamed to the host in chunks (see
/// the module documentation): the bytes written, or `-code` (`pipeline::code`; the last error says more).
///
/// # Safety
/// `l` must be null or a live handle.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn uo_layout_pdf(l: *const UoLayout, flags: u32, chunk: usize) -> f64 {
    pipeline::clear_last();
    // SAFETY: forwarded.
    let Some(l) = (unsafe { layout_ref(l) }) else { return -f64::from(pipeline::fail(&JobError::Argument("layout"))) };
    let trust = if flags & PDF_UNTRUSTED != 0 { Trust::Untrusted } else { Trust::Trusted };
    let o = pipeline::pdf_options(flags & PDF_COMPRESS != 0, trust, None);
    let size = if chunk == 0 { 1 << 20 } else { chunk };
    let mut total: u64 = 0;
    let sink = ChunkedSink::new(size, |c: Vec<u8>| {
        total += u64::try_from(c.len()).unwrap_or(u64::MAX);
        if host::chunk(&c) { Ok(()) } else { Err(PdfError::Sink) }
    });
    let v = view_of(&l.view);
    let r = uo_pdf::render_pdf_to(v.pages(), v.resources(), Some(&l.navigation), &o, sink).and_then(ChunkedSink::finish);
    match r {
        #[allow(clippy::cast_precision_loss, reason = "a file under the 256 MiB wasm32 output limit is exact in an f64")]
        Ok(_) => total as f64,
        Err(e) => -f64::from(pipeline::fail(&JobError::from(e))),
    }
}
