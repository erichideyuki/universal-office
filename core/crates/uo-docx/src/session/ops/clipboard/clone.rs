//! The clipboard's copies of model objects (D2b-F8).
//!
//! - `structuredClone` (`cloneDrawingRun`, `cloneBlocks`, `structuredClone(cellContent)`): every
//!   object new (fresh `ParaId`/`TableId`/`DrawingId`, U3), the symbol keys gone (`rpr_link`,
//!   `FormatChange::old_link`), everything else as it was: explicit-`undefined` keys and `NaN`
//!   stay (unlike the JSON copy of the undo, `normalize.rs`), the textbox wrappers (`TxbxSrc`) and
//!   the views of a textbox stay (F8-D6).
//! - The media of a clip: the TS objects hold the image bytes; here a run refers to its document's
//!   `MediaStore`, so a clip refers to its OWN list of payloads ([`MediaMap`]): copying maps the
//!   source document's ids to the clip's, pasting maps the clip's to the target's (the store
//!   dedupes equal bytes). Same bytes, same result as the TS.

use std::collections::HashMap;
use std::sync::Arc;

use crate::ids::{IdAlloc, MediaId};
use crate::model::Story;
use crate::model::media::{MediaRef, MediaStore};
use crate::model::run::Run;
use crate::model::table::Table;

/// The deepest nesting walked, in story levels (a table cell or a textbox content is one level;
/// the reader stops at 64, Nest pastes can add more): twice the reader's, for stack safety on
/// hostile models (F8-D17: content deeper keeps its ids and media refs).
const MAX_DEPTH: usize = 128;

/// How a copy maps media ids.
pub(crate) enum MediaMap<'a> {
    /// source document → the clip's own list (copy)
    ToClip { from: &'a MediaStore, list: &'a mut Vec<Arc<[u8]>>, seen: HashMap<MediaId, MediaId> },
    /// the clip's own list → the target document (paste)
    FromClip { list: &'a [Arc<[u8]>], to: &'a mut MediaStore, seen: HashMap<MediaId, MediaId> },
    /// the clip's own ids moved by `by` (the pieces of a multi-range copy share one list)
    Shift { by: usize },
}

impl MediaMap<'_> {
    fn map(&mut self, m: &mut MediaRef) {
        let id = m.0;
        match self {
            MediaMap::ToClip { from, list, seen } => {
                let new = *seen.entry(id).or_insert_with(|| {
                    let bytes: Arc<[u8]> = from.get_arc(id).cloned().unwrap_or_else(|| Arc::from(Vec::new()));
                    list.push(bytes);
                    MediaId(u32::try_from(list.len() - 1).unwrap_or(u32::MAX))
                });
                m.0 = new;
            }
            MediaMap::FromClip { list, to, seen } => {
                let new = *seen.entry(id).or_insert_with(|| match list.get(id.index()) {
                    Some(b) => to.add(b),
                    None => to.add(&[]),
                });
                m.0 = new;
            }
            MediaMap::Shift { by } => m.0 = MediaId(u32::try_from(id.index() + *by).unwrap_or(u32::MAX)),
        }
    }
}

/// The copy rules of one clipboard copy: the ids to take fresh (`None`: keep them, e.g. the
/// copy into the clip, which is copied again at every paste) and the media map.
pub(crate) struct Fresh<'a, 'm> {
    pub ids: Option<&'a mut IdAlloc>,
    pub media: &'a mut MediaMap<'m>,
}

fn shape_style(s: &mut crate::model::drawing::ShapeStyle, f: &mut Fresh<'_, '_>) {
    if let Some(b) = s.blip.as_deref_mut() {
        f.media.map(&mut b.image.data);
    }
}

/// A run as `structuredClone(run)` leaves it (in place on a Rust copy).
pub(crate) fn run(r: &mut Run, f: &mut Fresh<'_, '_>) {
    run_at(r, f, 0);
}

fn run_at(r: &mut Run, f: &mut Fresh<'_, '_>, depth: usize) {
    r.rpr_link = None;
    if r.props.fmt.as_ref().is_some_and(|x| x.old_link.is_some())
        && let Some(fc) = Arc::make_mut(&mut r.props).fmt.as_deref_mut()
    {
        fc.old_link = None;
    }
    if depth > MAX_DEPTH {
        return;
    }
    let Some(e) = r.ext.as_deref_mut() else { return };
    if let Some(img) = e.image.as_deref_mut() {
        if let Some(ids) = f.ids.as_deref_mut() {
            img.drawing = ids.drawing();
        }
        f.media.map(&mut img.data);
        if let Some(s) = img.svg.as_mut() {
            f.media.map(s);
        }
        if let Some(o) = img.ole.as_deref_mut() {
            f.media.map(&mut o.data);
        }
        if let Some(l) = img.layer.as_mut() {
            f.media.map(&mut l.data);
        }
        if let Some(s) = img.style.as_deref_mut() {
            shape_style(s, f);
        }
    }
    if let Some(sh) = e.shape.as_deref_mut() {
        if let Some(ids) = f.ids.as_deref_mut() {
            sh.drawing = ids.drawing();
        }
        shape_style(&mut sh.style, f);
    }
    if let Some(tb) = e.textbox.as_deref_mut() {
        if let Some(ids) = f.ids.as_deref_mut() {
            tb.drawing = ids.drawing();
        }
        if let Some(s) = tb.shape.as_deref_mut() {
            shape_style(s, f);
        }
        story_at(&mut tb.content, f, depth + 1);
    }
    if let Some(g) = e.group.as_deref_mut() {
        if let Some(ids) = f.ids.as_deref_mut() {
            g.drawing = ids.drawing();
        }
        for it in &mut g.items {
            run_at(&mut it.run, f, depth + 1);
        }
    }
    if let Some(rs) = e.field_result.as_mut() {
        for x in rs {
            run_at(x, f, depth + 1);
        }
    }
}

/// A story as `structuredClone(blocks)` leaves it.
pub(crate) fn story(s: &mut Story, f: &mut Fresh<'_, '_>) {
    story_at(s, f, 0);
}

fn story_at(s: &mut Story, f: &mut Fresh<'_, '_>, depth: usize) {
    if depth > MAX_DEPTH {
        return;
    }
    for i in 0..s.paras.len() {
        let Some(p) = s.paras.make_mut(i) else { continue };
        if let Some(ids) = f.ids.as_deref_mut() {
            p.id = ids.para();
        }
        for r in &mut p.runs {
            run_at(r, f, depth);
        }
    }
    for i in 0..s.tables.len() {
        let Some(a) = s.tables.make_mut(i) else { continue };
        table_at(Arc::make_mut(&mut a.table), f, depth);
    }
}

/// A table as the copy leaves it (the ids, the media of its cells).
pub(crate) fn table(t: &mut Table, f: &mut Fresh<'_, '_>) {
    table_at(t, f, 0);
}

fn table_at(t: &mut Table, f: &mut Fresh<'_, '_>, depth: usize) {
    if depth > MAX_DEPTH {
        return;
    }
    if let Some(ids) = f.ids.as_deref_mut() {
        t.id = ids.table();
    }
    for row in &mut t.rows {
        for c in &mut row.cells {
            story_at(&mut c.content, f, depth + 1);
        }
    }
}

/// Only the media of a table (a JSON copy, `TOps.cloneTable`, took the ids already).
pub(crate) fn table_media(t: &mut Table, media: &mut MediaMap<'_>) {
    table(t, &mut Fresh { ids: None, media });
}
