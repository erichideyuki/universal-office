//! Where a drawing run lives (D2b-F7): the TS holds the `Run` object (`floatSel`, `tbEdit.focusRun`,
//! the pane's runs); here a run is a path of steps from a story (stored once, copy-on-write), and
//! the two orders the TS keeps are enumerations of paths: `contextDrawingRuns` (the floating
//! pictures, boxes, shapes and groups of the blocks, table cells included) and `contextTextboxRuns`
//! (`forEachTextbox`: every box, those inside boxes and groups too).

use std::sync::Arc;

use crate::model::run::Run;
use crate::model::story::Story;

/// How deep a path may go (a box in a box in a group …); the reader's limits are far below it.
pub(crate) const MAX_DEPTH: usize = 64;

/// One step of a path to a run.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum Step {
    /// into a cell of a table anchored in the current story (anchor index, row, cell)
    Cell { anchor: u32, row: u32, col: u32 },
    /// a run of a paragraph of the current story
    Para { para: u32, run: u32 },
    /// into an item of the group the current run is
    Item(u32),
    /// into the content of the box the current run is
    Content,
}

pub(crate) type Path = Vec<Step>;

/// The run at `path` (a path ends with a `Para` or an `Item` step).
pub(crate) fn run_at<'a>(story: &'a Story, path: &[Step]) -> Option<&'a Run> {
    match path.first()? {
        Step::Cell { anchor, row, col } => {
            let a = story.tables.get(*anchor as usize)?;
            let cell = a.table.rows.get(*row as usize)?.cells.get(*col as usize)?;
            run_at(&cell.content, &path[1..])
        }
        Step::Para { para, run } => {
            let r = story.paras.get(*para as usize)?.runs.get(*run as usize)?;
            run_inner(r, &path[1..])
        }
        _ => None,
    }
}

fn run_inner<'a>(r: &'a Run, rest: &[Step]) -> Option<&'a Run> {
    match rest.first() {
        None => Some(r),
        Some(Step::Item(i)) => {
            let g = r.ext()?.group.as_deref()?;
            run_inner(&g.items.get(*i as usize)?.run, &rest[1..])
        }
        Some(Step::Content) => {
            let tb = r.ext()?.textbox.as_deref()?;
            run_at(&tb.content, &rest[1..])
        }
        _ => None,
    }
}

/// The run at `path` for a change (copy-on-write down to it).
pub(crate) fn run_at_mut<'a>(story: &'a mut Story, path: &[Step]) -> Option<&'a mut Run> {
    match path.first()? {
        Step::Cell { anchor, row, col } => {
            let a = story.tables.make_mut(*anchor as usize)?;
            let t = Arc::make_mut(&mut a.table);
            let cell = t.rows.get_mut(*row as usize)?.cells.get_mut(*col as usize)?;
            run_at_mut(&mut cell.content, &path[1..])
        }
        Step::Para { para, run } => {
            let p = story.paras.make_mut(*para as usize)?;
            let r = p.runs.get_mut(*run as usize)?;
            run_inner_mut(r, &path[1..])
        }
        _ => None,
    }
}

fn run_inner_mut<'a>(r: &'a mut Run, rest: &[Step]) -> Option<&'a mut Run> {
    match rest.first() {
        None => Some(r),
        Some(Step::Item(i)) => {
            let g = r.ext.as_deref_mut()?.group.as_deref_mut()?;
            run_inner_mut(&mut g.items.get_mut(*i as usize)?.run, &rest[1..])
        }
        Some(Step::Content) => {
            let tb = r.ext.as_deref_mut()?.textbox.as_deref_mut()?;
            run_at_mut(&mut tb.content, &rest[1..])
        }
        _ => None,
    }
}

/// What a walk visits.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Mode {
    /// `contextDrawingRuns`: the blocks only (cells included), floating objects
    Drawing,
    /// `forEachTextbox`: boxes, the contents of boxes and the items of groups too
    Boxes,
}

fn is_drawing(r: &Run) -> bool {
    r.ext().is_some_and(|e| e.image.as_deref().is_some_and(|i| i.floating == Some(true)) || e.textbox.is_some() || e.shape.is_some() || e.group.is_some())
}

fn walk_story(story: &Story, base: &mut Path, mode: Mode, out: &mut Vec<Path>, depth: usize) {
    if depth > MAX_DEPTH {
        return;
    }
    let n = story.paras.len();
    let mut ti = 0usize;
    for pi in 0..=n {
        while let Some(a) = story.tables.get(ti)
            && (a.before_para as usize <= pi || pi >= n)
        {
            let at = u32::try_from(ti).unwrap_or(u32::MAX);
            for (ri, row) in a.table.rows.iter().enumerate() {
                for (ci, cell) in row.cells.iter().enumerate() {
                    base.push(Step::Cell { anchor: at, row: u32::try_from(ri).unwrap_or(u32::MAX), col: u32::try_from(ci).unwrap_or(u32::MAX) });
                    walk_story(&cell.content, base, mode, out, depth + 1);
                    base.pop();
                }
            }
            ti += 1;
        }
        let Some(p) = story.paras.get(pi) else { continue };
        for (ri, r) in p.runs.iter().enumerate() {
            base.push(Step::Para { para: u32::try_from(pi).unwrap_or(u32::MAX), run: u32::try_from(ri).unwrap_or(u32::MAX) });
            walk_run(r, base, mode, out, depth);
            base.pop();
        }
    }
}

fn walk_run(r: &Run, base: &mut Path, mode: Mode, out: &mut Vec<Path>, depth: usize) {
    match mode {
        Mode::Drawing => {
            if is_drawing(r) {
                out.push(base.clone());
            }
        }
        Mode::Boxes => {
            let Some(e) = r.ext() else { return };
            if let Some(tb) = e.textbox.as_deref() {
                out.push(base.clone());
                base.push(Step::Content);
                walk_story(&tb.content, base, mode, out, depth + 1);
                base.pop();
            }
            if let Some(g) = e.group.as_deref()
                && depth < MAX_DEPTH
            {
                for (i, it) in g.items.iter().enumerate() {
                    base.push(Step::Item(u32::try_from(i).unwrap_or(u32::MAX)));
                    walk_run(&it.run, base, mode, out, depth + 1);
                    base.pop();
                }
            }
        }
    }
}

/// `contextDrawingRuns`: the floating pictures, boxes, shapes and groups of the story, in document
/// order.
pub(crate) fn drawing_paths(story: &Story) -> Vec<Path> {
    let mut out = Vec::new();
    walk_story(story, &mut Vec::new(), Mode::Drawing, &mut out, 0);
    out
}

/// `contextTextboxRuns` (`forEachTextbox`): every box, in document order.
pub(crate) fn textbox_paths(story: &Story) -> Vec<Path> {
    let mut out = Vec::new();
    walk_story(story, &mut Vec::new(), Mode::Boxes, &mut out, 0);
    out
}

/// The top-level paragraph and run of a path that stays in the story's own paragraphs.
pub(crate) fn top_level(path: &[Step]) -> Option<(u32, u32)> {
    match path {
        [Step::Para { para, run }] => Some((*para, *run)),
        _ => None,
    }
}
