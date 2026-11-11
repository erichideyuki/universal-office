//! `uo_session_query`: one thin arm per public accessor of `uo_docx::session::Session` that answers
//! model state (D2a-Q1: the session has typed accessors, no `Query` enum; `QueryOp` of
//! `schema/session.uos` names them). Each arm converts the arguments from the wire, calls the
//! accessor and converts the answer (`codec/bind.rs`); `QueryOp` is exhaustive, so an accessor added to
//! the schema fails to compile until it has its arm. The arms were drafted with the schema by the
//! throwaway extractor; the binding-level answers (the Office Clipboard, the painter, `fontUsage`, the
//! font embedding settings) are written here.
//!
//! Accessors left out (record "Milestone 2: the session in wasm", queries): the model and state
//! accessors (`document`, `view`, `package`, `context`, `options`, `story`, `paras`, `history`,
//! `pending`, `pending_props`, the version stores, `bookmark_paras`, `section_state`, `settings`,
//! `policy`, `resolved_policy`, `history_bytes`, `clip_identity`, `locked`: internal state or the
//! whole model, not a toolbar answer), `copied` and `copy_format` (the painter stays in the handle:
//! `CopyFormat`), `note_settings` (private entries; `note_options` answers the same), `render_clip_request`
//! (the F8 two-phase continuation: the shell gets `Job::RenderClip`), `count_matches` (its result type
//! is a private alias), `font_usage` (the binding-level `FontUsage`, in the shape of uo-fonts).

use uo_docx::session::Session;

use super::codec as w;
use super::wire::{BindError, FromWire, IntoWire};

/// The answer of the session's accessors (the binding-level queries are answered by the handle).
///
/// # Errors
/// An argument or an answer that does not convert ([`BindError`]).
#[allow(clippy::too_many_lines)]
pub fn answer(s: &Session, q: w::QueryOp) -> Result<Option<w::QueryAnswer>, BindError> {
    Ok(Some(match q {
        w::QueryOp::Selection => w::QueryAnswer::Selection(s.selection().into_wire()?),
        w::QueryOp::TableSelection => w::QueryAnswer::TableSelection(w::ATableSelection { value: s.table_selection().into_wire()? }),
        w::QueryOp::RevisionAuthors => w::QueryAnswer::RevisionAuthors(s.revision_authors().into_wire()?),
        w::QueryOp::IsDirty => w::QueryAnswer::IsDirty(s.is_dirty().into_wire()?),
        w::QueryOp::CanUndo => w::QueryAnswer::CanUndo(s.can_undo().into_wire()?),
        w::QueryOp::CanRedo => w::QueryAnswer::CanRedo(s.can_redo().into_wire()?),
        w::QueryOp::EditsSinceLoad => w::QueryAnswer::EditsSinceLoad(s.edits_since_load().into_wire()?),
        w::QueryOp::MarkStories => w::QueryAnswer::MarkStories(s.mark_stories().into_wire()?),
        w::QueryOp::HasBookmark(a) => w::QueryAnswer::HasBookmark(s.has_bookmark(&String::from_wire(a.name)?).into_wire()?),
        w::QueryOp::Bookmarks(a) => {
            w::QueryAnswer::Bookmarks(s.bookmarks(FromWire::from_wire(a.hidden)?, FromWire::from_wire(a.by_location)?).into_wire()?)
        }
        w::QueryOp::CanInsertField => w::QueryAnswer::CanInsertField(s.can_insert_field().into_wire()?),
        w::QueryOp::PasteOptions => w::QueryAnswer::PasteOptions(s.paste_options().into_wire()?),
        w::QueryOp::PasteSpecialFormats => {
            w::QueryAnswer::PasteSpecialFormats(w::APasteSpecialFormats { value: s.paste_special_formats().into_wire()? })
        }
        w::QueryOp::LastPasteRange => w::QueryAnswer::LastPasteRange(w::ALastPasteRange { value: s.last_paste_range().into_wire()? }),
        w::QueryOp::LinkSourceName => w::QueryAnswer::LinkSourceName(w::ALinkSourceName { value: s.link_source_name().into_wire()? }),
        w::QueryOp::PasteDefaults => w::QueryAnswer::PasteDefaults(s.paste_defaults().into_wire()?),
        w::QueryOp::SelectedText => w::QueryAnswer::SelectedText(s.selected_text().into_wire()?),
        w::QueryOp::Comments => w::QueryAnswer::Comments(s.comments().into_wire()?),
        w::QueryOp::PaneKeys => w::QueryAnswer::PaneKeys(s.pane_keys().into_wire()?),
        w::QueryOp::FloatSelection => w::QueryAnswer::FloatSelection(w::AFloatSelection { value: s.float_selection().into_wire()? }),
        w::QueryOp::SelectedObjectCount => w::QueryAnswer::SelectedObjectCount(s.selected_object_count().into_wire()?),
        w::QueryOp::SelectionPaneItems => w::QueryAnswer::SelectionPaneItems(s.selection_pane_items().into_wire()?),
        w::QueryOp::SelectedObjectState(a) => w::QueryAnswer::SelectedObjectState(w::ASelectedObjectState {
            value: s.selected_object_state(FromWire::from_wire(a.want_image)?).into_wire()?,
        }),
        w::QueryOp::FindQueryError(a) => {
            w::QueryAnswer::FindQueryError(w::AFindQueryError { value: s.find_query_error(&FromWire::from_wire(a.q)?).into_wire()? })
        }
        w::QueryOp::HighlightSpans => w::QueryAnswer::HighlightSpans(s.highlight_spans().into_wire()?),
        w::QueryOp::EvenOddHeaders => w::QueryAnswer::EvenOddHeaders(s.even_odd_headers().into_wire()?),
        w::QueryOp::HeaderFooterOptions(a) => {
            w::QueryAnswer::HeaderFooterOptions(s.header_footer_options(FromWire::from_wire(a.at)?).into_wire()?)
        }
        w::QueryOp::PageNumberFormat(a) => w::QueryAnswer::PageNumberFormat(s.page_number_format(FromWire::from_wire(a.at)?).into_wire()?),
        w::QueryOp::HfPartOfRef(a) => w::QueryAnswer::HfPartOfRef(w::AHfPartOfRef {
            value: s.hf_part_of_ref(FromWire::from_wire(a.kind)?, &String::from_wire(a.rid)?).into_wire()?,
        }),
        w::QueryOp::HeaderFooterTemplates(a) => w::QueryAnswer::HeaderFooterTemplates(
            s.header_footer_templates(FromWire::from_wire(a.gallery)?, FromWire::from_wire(a.lang)?).into_wire()?,
        ),
        w::QueryOp::NavigationResult => {
            w::QueryAnswer::NavigationResult(w::ANavigationResult { value: s.navigation_result().into_wire()? })
        }
        w::QueryOp::SelectionRanges => w::QueryAnswer::SelectionRanges(s.selection_ranges().into_wire()?),
        w::QueryOp::Headings => w::QueryAnswer::Headings(s.headings().into_wire()?),
        w::QueryOp::CurrentHeading => w::QueryAnswer::CurrentHeading(s.current_heading().into_wire()?),
        w::QueryOp::SearchResults => w::QueryAnswer::SearchResults(s.search_results().into_wire()?),
        w::QueryOp::EditingNote => w::QueryAnswer::EditingNote(w::AEditingNote { value: s.editing_note().into_wire()? }),
        w::QueryOp::LastConvertedNotes => w::QueryAnswer::LastConvertedNotes(s.last_converted_notes().into_wire()?),
        w::QueryOp::CanInsertNote => w::QueryAnswer::CanInsertNote(s.can_insert_note().into_wire()?),
        w::QueryOp::NoteRefs(a) => w::QueryAnswer::NoteRefs(s.note_refs(FromWire::from_wire(a.kind)?).into_wire()?),
        w::QueryOp::NoteCount(a) => w::QueryAnswer::NoteCount(s.note_count(FromWire::from_wire(a.footnote)?).into_wire()?),
        w::QueryOp::NoteOptions(a) => w::QueryAnswer::NoteOptions(s.note_options(FromWire::from_wire(a.footnote)?).into_wire()?),
        w::QueryOp::CoverPages(a) => w::QueryAnswer::CoverPages(s.cover_pages(FromWire::from_wire(a.lang)?).into_wire()?),
        w::QueryOp::HasCoverPage => w::QueryAnswer::HasCoverPage(s.has_cover_page().into_wire()?),
        w::QueryOp::CoverParaIndexes => w::QueryAnswer::CoverParaIndexes(s.cover_para_indexes().into_wire()?),
        w::QueryOp::Watermark(a) => {
            w::QueryAnswer::Watermark(w::AWatermark { value: s.watermark(FromWire::from_wire(a.at)?).into_wire()? })
        }
        w::QueryOp::WatermarkTexts(a) => w::QueryAnswer::WatermarkTexts(s.watermark_texts(FromWire::from_wire(a.lang)?).into_wire()?),
        w::QueryOp::WatermarkGalleryEntries(a) => {
            w::QueryAnswer::WatermarkGalleryEntries(s.watermark_gallery_entries(FromWire::from_wire(a.lang)?).into_wire()?)
        }
        w::QueryOp::ParagraphBorderSides => w::QueryAnswer::ParagraphBorderSides(s.paragraph_border_sides().into_wire()?),
        w::QueryOp::ParagraphIndents => w::QueryAnswer::ParagraphIndents(s.paragraph_indents().into_wire()?),
        w::QueryOp::ParagraphTabStops => w::QueryAnswer::ParagraphTabStops(s.paragraph_tab_stops().into_wire()?),
        w::QueryOp::CompressResult => w::QueryAnswer::CompressResult(s.compress_result().into_wire()?),
        w::QueryOp::ReferencesResult => {
            w::QueryAnswer::ReferencesResult(w::AReferencesResult { value: s.references_result().into_wire()? })
        }
        w::QueryOp::RefSeed => w::QueryAnswer::RefSeed(w::ARefSeed { value: s.ref_seed().into_wire()? }),
        w::QueryOp::CaptionSettings => w::QueryAnswer::CaptionSettings(s.caption_settings().into_wire()?),
        w::QueryOp::CaptionLabels => w::QueryAnswer::CaptionLabels(s.caption_labels().into_wire()?),
        w::QueryOp::CaptionNumbering(a) => w::QueryAnswer::CaptionNumbering(s.caption_numbering(&String::from_wire(a.label)?).into_wire()?),
        w::QueryOp::AutoCaption => w::QueryAnswer::AutoCaption(w::AAutoCaption { value: s.auto_caption().into_wire()? }),
        w::QueryOp::CaptionTarget => w::QueryAnswer::CaptionTarget(w::ACaptionTarget { value: s.caption_target().into_wire()? }),
        w::QueryOp::CrossReferenceKinds(a) => {
            w::QueryAnswer::CrossReferenceKinds(s.cross_reference_kinds(FromWire::from_wire(a.ty)?).into_wire()?)
        }
        w::QueryOp::CrossReferenceItems(a) => w::QueryAnswer::CrossReferenceItems(
            s.cross_reference_items(FromWire::from_wire(a.ty)?, Option::<String>::from_wire(a.label)?.as_deref()).into_wire()?,
        ),
        w::QueryOp::NextCaptionNumber(a) => {
            w::QueryAnswer::NextCaptionNumber(s.next_caption_number(&String::from_wire(a.label)?).into_wire()?)
        }
        w::QueryOp::DateTimeFormats(a) => w::QueryAnswer::DateTimeFormats(s.date_time_formats(FromWire::from_wire(a.lang)?).into_wire()?),
        w::QueryOp::ManualHyphenationState => {
            w::QueryAnswer::ManualHyphenationState(w::AManualHyphenationState { value: s.manual_hyphenation_state().into_wire()? })
        }
        w::QueryOp::ManualHyphenationFrom => {
            w::QueryAnswer::ManualHyphenationFrom(w::AManualHyphenationFrom { value: s.manual_hyphenation_from().into_wire()? })
        }
        w::QueryOp::TableOfFiguresOptions => {
            w::QueryAnswer::TableOfFiguresOptions(w::ATableOfFiguresOptions { value: s.table_of_figures_options().into_wire()? })
        }
        w::QueryOp::HasTableOfContents => w::QueryAnswer::HasTableOfContents(s.has_table_of_contents().into_wire()?),
        w::QueryOp::InTableOfContents => w::QueryAnswer::InTableOfContents(s.in_table_of_contents().into_wire()?),
        w::QueryOp::TocTemplates => w::QueryAnswer::TocTemplates(s.toc_templates().into_wire()?),
        w::QueryOp::TableOfContentsOptions => {
            w::QueryAnswer::TableOfContentsOptions(w::ATableOfContentsOptions { value: s.table_of_contents_options().into_wire()? })
        }
        w::QueryOp::TocLevel => w::QueryAnswer::TocLevel(s.toc_level().into_wire()?),
        w::QueryOp::RevisionEntries => w::QueryAnswer::RevisionEntries(s.revision_entries().into_wire()?),
        w::QueryOp::RevisionCount => w::QueryAnswer::RevisionCount(s.revision_count().into_wire()?),
        w::QueryOp::RevisionAuthorsNow => w::QueryAnswer::RevisionAuthorsNow(s.revision_authors_now().into_wire()?),
        w::QueryOp::AuthorColorOf(a) => w::QueryAnswer::AuthorColorOf(s.author_color_of(&String::from_wire(a.author)?).into_wire()?),
        w::QueryOp::MarkupOptions => w::QueryAnswer::MarkupOptions(s.markup_options().into_wire()?),
        w::QueryOp::SuggestFormula => w::QueryAnswer::SuggestFormula(s.suggest_formula().into_wire()?),
        w::QueryOp::PreviewFormula(a) => w::QueryAnswer::PreviewFormula(w::APreviewFormula {
            value: s
                .preview_formula(&String::from_wire(a.formula)?, Option::<String>::from_wire(a.number_format)?.as_deref())
                .into_wire()?,
        }),
        w::QueryOp::HasSelection => w::QueryAnswer::HasSelection(s.has_selection().into_wire()?),
        w::QueryOp::Range => w::QueryAnswer::Range(s.range().into_wire()?),
        w::QueryOp::CanonPos(a) => w::QueryAnswer::CanonPos(s.canon_pos(FromWire::from_wire(a.pos)?).into_wire()?),
        w::QueryOp::HyphenationSettings => w::QueryAnswer::HyphenationSettings(s.hyphenation_settings().into_wire()?),
        w::QueryOp::Hyphenation => w::QueryAnswer::Hyphenation(s.hyphenation().into_wire()?),
        w::QueryOp::ClipboardItems | w::QueryOp::CopyFormat | w::QueryOp::FontUsage | w::QueryOp::FontEmbedding => return Ok(None),
    }))
}
