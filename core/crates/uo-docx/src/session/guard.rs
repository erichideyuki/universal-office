//! The run guard of the editor's text operations (`editor/src/edit-guard.ts`, ported 1:1).
//!
//! The operations of [`super::text`] split, rebuild and merge runs looking only at the EDITING
//! props: a rebuilt run loses its original `w:rPr` (character style, theme fonts/colours…) and
//! its extras (textbox, shape, group, `ptab`, field result), and neighbouring runs with the same
//! props but different `rPr`s merge — typing one letter in a paragraph with "Strong" used to drop
//! the style of the whole paragraph in the layout.
//!
//! Before the operation every run of the affected paragraphs gets a unique mark in
//! `props.fontAscii` (a field the merge compares, so distinct runs never merge; typed text
//! inherits the neighbour's mark and joins it). After it, the mark goes back to the original value
//! and the run gets back the original's `rPr` (and its extras, when it stayed whole).
//!
//! The TS identifies the original runs by object (`r !== o.run`); here every use of that test is
//! decided by the text alone (see [`restore`]), so no run identity is needed.

use std::collections::HashMap;
use std::sync::Arc;

use crate::js::Str;
use crate::model::para::Paragraph;
use crate::model::run::{Run, RunProps};
use crate::pvec::PVec;
use crate::word::obj;

/// `MARK`: the prefix of the guard's marks (`\u0000` never comes from XML).
pub const MARK: &str = "\u{0}uw-run:";

/// What the guard keeps of an original run (`orig`: the run object and its font; of the run,
/// what `restore` reads: its text, `rpr` and extras, none of which an operation changes in place).
struct Orig {
    text: crate::text::Utf16,
    rpr: Option<Arc<crate::model::props::RPr>>,
    ext: Option<Box<crate::model::run::RunExt>>,
    /// `r.props.fontAscii` before the mark (`None` = undefined, absent or explicit)
    font: Option<Str>,
}

fn is_mark(s: Option<&Str>) -> bool {
    s.is_some_and(|s| s.starts_with(MARK))
}

/// `setFont(r, font)`: `{ ...r.props }` with `fontAscii` set back or deleted.
fn set_font(r: &mut Run, font: Option<&Str>) {
    let mut props = (*r.props).clone();
    match font {
        None => obj::delete(&mut props, "fontAscii"),
        Some(f) => obj::set(&mut props, "fontAscii", |p| p.font_ascii = Some(f.clone())),
    }
    r.props = Arc::new(props);
}

/// `guardRuns(model, lo, hi, op)`: runs `op` with the runs of paragraphs `lo..=hi` marked, then
/// restores them (also when `op` fails).
pub fn guard_runs<T>(paras: &mut PVec<Paragraph>, lo: u32, hi: u32, op: impl FnOnce(&mut PVec<Paragraph>) -> T) -> T {
    let len = paras.len();
    // `Math.max(0, Math.min(lo, hi))`, `Math.min(len - 1, Math.max(lo, hi))`: empty when `a > b`
    let a = lo.min(hi) as usize;
    let Some(last) = len.checked_sub(1) else { return op(paras) };
    let b = (hi.max(lo) as usize).min(last);
    let mut orig: HashMap<Str, Orig> = HashMap::new();
    let mut n = 0u64;
    for i in a..=b {
        let Some(p) = paras.make_mut(i) else { continue };
        for r in &mut p.runs {
            let key: Str = format!("{MARK}{n}").into();
            n += 1;
            let mut props: RunProps = (*r.props).clone();
            // a tracked formatting change with the SAME font as now: the mark goes there too
            // (undoing the formatting keeps comparing equal)
            let same_font = props.fmt.as_deref().is_some_and(|f| f.old.font_ascii == props.font_ascii);
            let font = props.font_ascii.clone();
            obj::set(&mut props, "fontAscii", |p| p.font_ascii = Some(key.clone()));
            if same_font && let Some(f) = props.fmt.as_deref() {
                let mut f = f.clone();
                obj::set(&mut *f.old, "fontAscii", |o| o.font_ascii = Some(key.clone()));
                obj::set(&mut props, "fmt", |p| p.fmt = Some(Box::new(f)));
            }
            r.props = Arc::new(props);
            orig.insert(key, Orig { text: r.text.clone(), rpr: r.rpr.clone(), ext: r.ext.clone(), font });
        }
    }
    let out = op(paras);
    // marks may have moved to the next paragraph (Enter): one more is scanned
    let Some(last) = paras.len().checked_sub(1) else { return out };
    let end = (b + 1).min(last);
    for i in a..=end {
        if paras.get(i).is_some_and(|p| p.runs.iter().any(needs_restore))
            && let Some(p) = paras.make_mut(i)
        {
            restore(p, &orig);
        }
    }
    // (the TS also sets the font back on the original run objects that left the scanned range:
    // with value runs those are gone, and every run of the story is inside `a..=end`)
    out
}

fn needs_restore(r: &Run) -> bool {
    is_mark(r.props.font_ascii.as_ref()) || r.props.fmt.as_deref().is_some_and(|f| is_mark(f.old.font_ascii.as_ref()))
}

/// The guarded extras (`EXTRAS`): what the text operations drop when they rebuild a run.
fn restore_extras(dst: &mut Run, src: Option<&crate::model::run::RunExt>) {
    let Some(s) = src else { return };
    macro_rules! fill {
        ($($f:ident),*) => {
            $(
                if s.$f.is_some() && dst.ext().is_none_or(|d| d.$f.is_none()) {
                    let v = s.$f.clone();
                    dst.ext_mut().$f = v;
                }
            )*
        };
    }
    fill!(image, textbox, shape, group, ptab, field_result, field, note_ref, custom, sym_font, form_field, in_toc);
}

/// `restore(p, orig)`.
fn restore(p: &mut Paragraph, orig: &HashMap<Str, Orig>) {
    for r in &mut p.runs {
        // a tracked formatting created in the operation: the props "before" took the mark
        if let Some(f) = r.props.fmt.as_deref()
            && let Some(key) = f.old.font_ascii.as_ref().filter(|k| k.starts_with(MARK))
        {
            let font = orig.get(key).and_then(|o| o.font.clone());
            let mut o = (*f.old).clone();
            match font {
                None => obj::delete(&mut o, "fontAscii"),
                Some(v) => obj::set(&mut o, "fontAscii", |x| x.font_ascii = Some(v)),
            }
            let mut f = f.clone();
            f.old = Box::new(o);
            let mut props = (*r.props).clone();
            obj::set(&mut props, "fmt", |x| x.fmt = Some(Box::new(f)));
            r.props = Arc::new(props);
        }
        let Some(key) = r.props.font_ascii.clone().filter(|k| k.starts_with(MARK)) else { continue };
        let o = orig.get(&key);
        set_font(r, o.and_then(|o| o.font.as_ref()));
        let Some(o) = o else { continue };
        if r.rpr.is_none()
            && let Some(rpr) = &o.rpr
        {
            obj::set(r, "rpr", |x| x.rpr = Some(Arc::clone(rpr)));
        }
        if r.text == o.text {
            // a whole run (only rebuilt): everything the operations dropped comes back (on the
            // original object itself this changes nothing: it has every one of them)
            restore_extras(r, o.ext.as_deref());
        } else {
            // (texts differ: never the original object)
            let sym = o.ext.as_deref().and_then(|e| e.sym_font.clone());
            if r.ext().is_none_or(|e| e.sym_font.as_ref().is_none_or(|s| s.is_empty()))
                && let Some(s) = sym.filter(|s| !s.is_empty())
            {
                r.ext_mut().sym_font = Some(s);
            }
            let toc = o.ext.as_deref().is_some_and(|e| e.in_toc == Some(true));
            if r.ext().is_none_or(|e| e.in_toc != Some(true)) && toc {
                r.ext_mut().in_toc = Some(true);
            }
        }
    }
}

/// The guarded editing operations of `universal-write.ts` (lines 899–931): every model op the
/// editor calls goes through [`guard_runs`] over the paragraphs it touches.
pub mod guarded {
    use super::guard_runs;
    use crate::ids::IdAlloc;
    use crate::model::para::Paragraph;
    use crate::model::props::RevisionInfo;
    use crate::model::run::{Run, RunProps};
    use crate::pvec::PVec;
    use crate::session::revisions::{self, TrackedDelete};
    use crate::session::text::{self, CarriedProps, DocPosition, DocRange, OpResult, normalize_range};

    pub fn insert_text(m: &mut PVec<Paragraph>, pos: DocPosition, t: &[u16], props: Option<&CarriedProps>) -> OpResult<DocPosition> {
        guard_runs(m, pos.para, pos.para, |m| text::insert_text(m, pos, t, props))
    }

    pub fn insert_run(m: &mut PVec<Paragraph>, pos: DocPosition, run: Run) -> OpResult<DocPosition> {
        guard_runs(m, pos.para, pos.para, |m| text::insert_run(m, pos, run))
    }

    pub fn delete_range(m: &mut PVec<Paragraph>, range: DocRange) -> OpResult<DocPosition> {
        let r = normalize_range(range);
        guard_runs(m, r.start.para, r.end.para, |m| text::delete_range(m, range))
    }

    pub fn split_paragraph(m: &mut PVec<Paragraph>, ids: &mut IdAlloc, pos: DocPosition) -> OpResult<DocPosition> {
        guard_runs(m, pos.para, pos.para, |m| text::split_paragraph(m, ids, pos))
    }

    /// `mergeWithPrevious` (`guardRuns(m, para - 1, para, …)`; `para - 1` of 0 is −1, clamped to 0
    /// by the guard's `Math.max(0, …)`).
    pub fn merge_with_previous(m: &mut PVec<Paragraph>, para: u32) -> OpResult<DocPosition> {
        guard_runs(m, para.saturating_sub(1), para, |m| text::merge_with_previous(m, para))
    }

    pub fn apply_run_props(m: &mut PVec<Paragraph>, range: DocRange, patch: &RunProps) -> OpResult<()> {
        let r = normalize_range(range);
        guard_runs(m, r.start.para, r.end.para, |m| text::apply_run_props(m, range, patch))
    }

    pub fn track_insert_text(
        m: &mut PVec<Paragraph>,
        pos: DocPosition,
        t: &[u16],
        rev: &RevisionInfo,
        props: Option<&CarriedProps>,
    ) -> OpResult<DocPosition> {
        guard_runs(m, pos.para, pos.para, |m| revisions::track_insert_text(m, pos, t, rev, props))
    }

    pub fn track_insert_run(m: &mut PVec<Paragraph>, pos: DocPosition, run: Run, rev: &RevisionInfo) -> OpResult<DocPosition> {
        guard_runs(m, pos.para, pos.para, |m| revisions::track_insert_run(m, pos, run, rev))
    }

    pub fn track_split_paragraph(
        m: &mut PVec<Paragraph>,
        ids: &mut IdAlloc,
        pos: DocPosition,
        rev: &RevisionInfo,
    ) -> OpResult<DocPosition> {
        guard_runs(m, pos.para, pos.para, |m| revisions::track_split_paragraph(m, ids, pos, rev))
    }

    pub fn track_delete(m: &mut PVec<Paragraph>, range: DocRange, rev: &RevisionInfo) -> OpResult<TrackedDelete> {
        let r = normalize_range(range);
        guard_runs(m, r.start.para, r.end.para, |m| revisions::track_delete(m, range, rev))
    }

    pub fn track_apply_run_props(m: &mut PVec<Paragraph>, range: DocRange, patch: &RunProps, rev: &RevisionInfo) -> OpResult<()> {
        let r = normalize_range(range);
        guard_runs(m, r.start.para, r.end.para, |m| revisions::track_apply_run_props(m, range, patch, rev))
    }
}
