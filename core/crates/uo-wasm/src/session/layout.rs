//! The session's layouts: the editor's layout options (`layoutOpts()`, universal-write.ts 8002: the
//! engine options with the revision view of the markup mode), the layout of the session's view (the
//! edit context's model, as the editor lays it out), and the edit scope the caret exports read
//! (`rebuildEditLines`: which paragraphs are being edited).
//!
//! Declared (WS-D6): print view only; the `viewer` font policy (uo-fonts' `Editor` policy is not
//! built: the editor's `liveFontProvider` is a milestone-2 gate item); a textbox context has no edit
//! scope here (uo-docx does not expose the chain of the edited box publicly): the caret exports answer
//! none inside a textbox.

use std::sync::Arc;

use uo_docx::model::Block;
use uo_docx::read::EmbeddedFont;
use uo_docx::session::ops::context::EditContext;
use uo_docx::session::{MarkupMode, Session};
use uo_docx::view::{BlockPath, HfWhich, NoteKind};
use uo_docx_layout::LayoutOptions;
use uo_docx_layout::hit::EditScope;
use uo_docx_layout::hyphenate::HyphenSessions;
use uo_docx_layout::options::{RevisionMode, RevisionView};
use uo_docx_layout::run_boxes::RunFilter;

use super::handle::HostFonts;
use super::oracle::font_provider;
use crate::pipeline::{JobError, LaidOut};

/// `ENGINE_LAYOUT_OPTS.hyphenationLanguages` (universal-write.ts 308).
pub const ENGINE_HYPHENATION_LANGUAGES: [&str; 3] = ["pt-BR", "en", "es"];

/// `revisionView()` (universal-write.ts 7986): the markup mode (`none` lays out the final text), the
/// "Show Markup" options and the session's author order.
#[must_use]
pub fn revision_view(s: &Session) -> RevisionView {
    let o = &s.options().markup_options;
    RevisionView {
        mode: match s.options().markup {
            MarkupMode::All => RevisionMode::All,
            MarkupMode::Simple => RevisionMode::Simple,
            MarkupMode::None => RevisionMode::Final,
            MarkupMode::Original => RevisionMode::Original,
        },
        ins_del: Some(o.ins_del),
        formatting: Some(o.formatting),
        authors: o.authors.as_ref().map(|a| a.iter().map(|x| Box::<str>::from(&**x)).collect()),
        colors: None,
        order: Some(s.revision_authors().iter().map(|x| Box::<str>::from(&**x)).collect()),
    }
}

/// `layoutOpts()`: the engine options of the editor over `base` (the host's clock, language, loaded
/// pattern sets): the hyphenation languages and the revision view.
#[must_use]
pub fn editor_options(s: &Session, base: &LayoutOptions) -> LayoutOptions {
    let mut o = base.clone();
    if o.hyphenation_languages.is_none() {
        o.hyphenation_languages = Some(ENGINE_HYPHENATION_LANGUAGES.iter().map(|l| Box::<str>::from(*l)).collect());
    }
    o.revisions = Some(revision_view(s));
    o
}

/// The layout of the session's view (the edit context's model) with the run boxes of links and
/// plugin inlines, as `pipeline::lay_out` does for an opened document.
///
/// # Errors
/// [`JobError::Argument`] when the font policy does not build; the layout's errors.
pub fn lay_out_session(s: &Session, embedded: &[EmbeddedFont], fonts: &HostFonts, o: &LayoutOptions) -> Result<LaidOut, JobError> {
    let provider = font_provider(fonts, embedded).map_err(JobError::Argument)?;
    let view = s.view();
    let layout = {
        let ldoc = uo_docx_layout::derive::build(view, o)?;
        let mut hyph = HyphenSessions::new(Arc::clone(&fonts.hyphenation));
        uo_docx_layout::engine::layout_document(&ldoc, o, &*provider, &mut hyph)?
    };
    let navigation = layout.navigation(view);
    let links = layout.run_boxes(view, RunFilter::Links);
    let inlines = layout.run_boxes(view, RunFilter::CustomInlines);
    Ok(LaidOut { layout, navigation, links, inlines })
}

/// The cell index of a hop (`cellAtGrid`: the hop's column is a grid column).
fn cell_index(t: &uo_docx::model::Table, row: u32, col: u32) -> Option<u32> {
    let cell = uo_docx::word::refs::cell_at_grid(t, f64::from(row), f64::from(col))?;
    let k = t.rows.get(usize::try_from(row).ok()?)?.cells.iter().position(|c| std::ptr::eq(c, cell))?;
    u32::try_from(k).ok()
}

/// The edit scope of the session's context (see the module comment), owned: the textbox chain is the
/// only borrowed scope and has none here.
#[must_use]
pub fn edit_scope(s: &Session) -> Option<EditScope<'static>> {
    let doc = s.document();
    match s.context() {
        EditContext::Body => Some(EditScope::Body),
        EditContext::Cell { anchor, hops, .. } => {
            // the body block of the anchor's table
            let mut seen = 0u32;
            let mut found = None;
            for (bi, b) in doc.body.blocks().enumerate() {
                if let Block::Table(t) = b {
                    if seen == *anchor {
                        found = Some((u32::try_from(bi).ok()?, t));
                        break;
                    }
                    seen += 1;
                }
            }
            let (bi, mut table) = found?;
            let mut path = BlockPath::root(bi);
            let (last, down) = hops.split_last()?;
            for h in down {
                let k = cell_index(table, h.row, h.col)?;
                let block = h.block?;
                let cell = table.rows.get(usize::try_from(h.row).ok()?)?.cells.get(usize::try_from(k).ok()?)?;
                match cell.content.block_at(usize::try_from(block).ok()?)? {
                    Block::Table(t) => table = t,
                    Block::Paragraph(_) => return None,
                }
                path = path.cell_block(h.row, k, block);
            }
            let k = cell_index(table, last.row, last.col)?;
            Some(EditScope::Cell { table: path, row: last.row, cell: k })
        }
        EditContext::HeaderFooter { part, page, .. } => {
            let header = doc.sections.iter().any(|sec| sec.headers.values().any(|p| p == part));
            let footer = doc.sections.iter().any(|sec| sec.footers.values().any(|p| p == part));
            let which = match (header, footer) {
                (true, _) => HfWhich::Header,
                (false, true) => HfWhich::Footer,
                // a part no slot names (the path says which)
                _ => {
                    let path = doc.hf.get(part.index()).map(|p| &*p.path).unwrap_or("");
                    if path.contains("footer") { HfWhich::Footer } else { HfWhich::Header }
                }
            };
            Some(EditScope::HeaderFooter { which, part: *part, page: *page })
        }
        EditContext::Note { footnote, id, .. } => Some(EditScope::Note {
            kind: if *footnote { NoteKind::Footnote } else { NoteKind::Endnote },
            id: uo_docx::js::NumKey::new(*id),
        }),
        EditContext::Textbox { .. } => None,
    }
}

/// A short key of the scope, for the lines cache.
#[must_use]
pub fn scope_key(s: &Session) -> String {
    match s.context() {
        EditContext::Body => "b".to_owned(),
        EditContext::Cell { anchor, hops, .. } => {
            let h: Vec<String> = hops.iter().map(|h| format!("{}.{}.{}", h.row, h.col, h.block.map_or(-1, i64::from))).collect();
            format!("c{anchor}/{}", h.join("/"))
        }
        EditContext::HeaderFooter { part, page, .. } => format!("h{}@{page}", part.0),
        EditContext::Note { footnote, id, .. } => format!("n{footnote}{}", id.to_bits()),
        EditContext::Textbox { at, .. } => format!("t{at}"),
    }
}
