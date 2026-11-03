//! How tracked changes show (`docx-layout/src/revisions.ts`), measured in Word: insertions
//! underlined and deletions struck in the author's colour, moves in green, change bars, the
//! per-author palette, and the four modes (`final`, `simple`, `all`, `original`).
//!
//! `joinDeleted` builds a derived paragraph (the layout's `LDoc` does that, LA2); here is its data:
//! [`join_offset`], the `REV_JOINS` offset it records, which [`revision_ranges`] takes back.

use indexmap::{IndexMap, IndexSet};

use super::direct::{direct_ppr, direct_rpr, direct_rpr_with};
use super::obj::{json_eq, set};
use crate::js::JsValue;
use crate::model::para::{ParaProps, Paragraph};
use crate::model::props::{PPr, RPr, RevisionInfo};
use crate::model::run::{Run, RunProps};
use crate::read::props_map::{para_props_from_ppr, run_props_from_rpr};

/// `RevisionMode`.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum RevisionMode {
    #[default]
    Final,
    Simple,
    All,
    Original,
}

/// The per-author palette of Word 365 (`AUTHOR_PALETTE`, 20 colours, cycling).
pub const AUTHOR_PALETTE: [&str; 20] = [
    "D13438", "0078D4", "5C2E91", "498205", "CC3595", "7160E8", "038387", "6D5700", "CF0F1F", "4E6AED", "B146C2", "394146", "0B6A0B",
    "CA5010", "750B1C", "5D5A58", "881798", "69797E", "005B70", "8E562E",
];
/// Moves (`w:moveFrom`/`w:moveTo`).
pub const MOVE_COLOR: &str = "2C6234";
/// The change bar of All Markup (black) and of Simple Markup (red).
pub const BAR_COLOR: &str = "000000";
pub const SIMPLE_BAR_COLOR: &str = "C00000";
/// Bar width (twips): 0.72 pt.
pub const BAR_WIDTH: f64 = 14.4;

/// `authorColor(order, author)` (revisions.ts:50).
#[must_use]
pub fn author_color<S: AsRef<str>>(order: &[S], author: &str) -> &'static str {
    let i = order.iter().position(|a| a.as_ref() == author).unwrap_or(order.len());
    AUTHOR_PALETTE.get(i % AUTHOR_PALETTE.len()).copied().unwrap_or("D13438")
}

/// `barX(marginLeft)` (revisions.ts:56): the left edge of the bar (twips).
#[must_use]
pub fn bar_x(margin_left: f64) -> f64 {
    uo_jsnum::math::max(268.8, if margin_left >= 1440.0 { margin_left - 720.0 } else { margin_left / 2.0 })
}

/// `RunRevision` (revisions.ts:61): hide the run, show it marked, or as is (`None`).
#[derive(Debug, Clone, PartialEq)]
pub enum RunRevision {
    Hide,
    Marked { color: Box<str>, del: bool, r#move: bool },
}

/// Which markup a check is about (`'insDel' | 'fmt'`).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum MarkKind {
    InsDel,
    Fmt,
}

/// `RevisionDisplay` (revisions.ts:63). `Default` = `new RevisionDisplay(undefined, [])`.
#[derive(Clone, Debug)]
pub struct RevisionDisplay {
    pub mode: RevisionMode,
    ins_del: bool,
    fmt: bool,
    authors: Option<IndexSet<Box<str>>>,
    order: Vec<Box<str>>,
    colors: IndexMap<Box<str>, Box<str>>,
}

impl Default for RevisionDisplay {
    fn default() -> RevisionDisplay {
        RevisionDisplay::new(RevisionMode::Final, None, None, None, None, None, &[])
    }
}

impl RevisionDisplay {
    /// `new RevisionDisplay(view, docOrder)`: the view's fields (`insDel`, `formatting`, `authors`,
    /// `colors`, `order`; absent = the defaults) and the document's author order.
    #[must_use]
    pub fn new(
        mode: RevisionMode,
        ins_del: Option<bool>,
        formatting: Option<bool>,
        authors: Option<&[Box<str>]>,
        colors: Option<&IndexMap<Box<str>, Box<str>>>,
        order: Option<&[Box<str>]>,
        doc_order: &[Box<str>],
    ) -> RevisionDisplay {
        RevisionDisplay {
            mode,
            ins_del: ins_del.unwrap_or(true),
            fmt: formatting.unwrap_or(true),
            authors: authors.map(|a| a.iter().cloned().collect()),
            order: order.unwrap_or(doc_order).to_vec(),
            colors: colors.cloned().unwrap_or_default(),
        }
    }

    /// `shown(r, kind)` (revisions.ts:82).
    fn shown(&self, r: Option<&RevisionInfo>, kind: MarkKind) -> bool {
        let Some(r) = r else { return false };
        if if kind == MarkKind::InsDel { !self.ins_del } else { !self.fmt } {
            return false;
        }
        self.authors.as_ref().is_none_or(|a| a.contains(&*r.author))
    }

    /// `color(author)` (revisions.ts:88).
    #[must_use]
    pub fn color(&self, author: &str) -> Box<str> {
        match self.colors.get(author) {
            Some(c) => c.clone(),
            None => author_color(&self.order, author).into(),
        }
    }

    /// `run(run)` (revisions.ts:93), over the run's props.
    #[must_use]
    pub fn run(&self, props: &RunProps) -> Option<RunRevision> {
        let (ins, del) = (props.ins.as_deref(), props.del.as_deref());
        if ins.is_none() && del.is_none() {
            return None;
        }
        if self.mode == RevisionMode::Original {
            return ins.map(|_| RunRevision::Hide);
        }
        let marks = self.mode == RevisionMode::All;
        if let Some(del) = del {
            if !marks || !self.shown(Some(del), MarkKind::InsDel) {
                return Some(RunRevision::Hide);
            }
            let mv = del.r#move == Some(true);
            return Some(RunRevision::Marked {
                color: if mv { MOVE_COLOR.into() } else { self.color(&del.author) },
                del: true,
                r#move: mv,
            });
        }
        if !marks || !self.shown(ins, MarkKind::InsDel) {
            return None;
        }
        let ins = ins?;
        let mv = ins.r#move == Some(true);
        Some(RunRevision::Marked { color: if mv { MOVE_COLOR.into() } else { self.color(&ins.author) }, del: false, r#move: mv })
    }

    /// `directRPr(run)` (revisions.ts:107): in the Original view, the `rPr` before the tracked
    /// formatting change.
    #[must_use]
    pub fn direct_rpr(&self, run: &Run) -> RPr {
        let Some(f) = run.props.fmt.as_deref().filter(|_| self.mode == RevisionMode::Original) else { return direct_rpr(run) };
        let c = run.rpr.as_deref().and_then(|r| r.change.as_deref());
        // from the file and untouched: the exact old rPr; created by editing: the props before
        if let Some(c) = c
            && c.author == f.author
            && c.date == f.date
            && json_eq(Some(&run_props_from_rpr(Some(&c.r_pr)) as &dyn JsValue), Some(&*f.old as &dyn JsValue))
        {
            return (*c.r_pr).clone();
        }
        let p = &run.props;
        let mut props: RunProps = (*f.old).clone();
        if let Some(src) = p.src {
            set(&mut props, "src", |x| x.src = Some(src));
        }
        if let Some(link) = p.link.clone().filter(|s| !s.is_empty()) {
            set(&mut props, "link", |x| x.link = Some(link));
        }
        direct_rpr_with(run, &props)
    }

    /// `paragraph(para)` (revisions.ts:118): in the Original view, the paragraph with the
    /// formatting before the tracked change — as the (`ppr`, `props`) override of a copy
    /// (`{ ...para, ppr, props }`, LA2); `None` = the paragraph itself.
    #[must_use]
    pub fn paragraph(&self, para: &Paragraph) -> Option<(PPr, ParaProps)> {
        let f = para.props.pfmt.as_deref().filter(|_| self.mode == RevisionMode::Original)?;
        let c = para.ppr.as_deref().and_then(|p| p.change.as_deref());
        let meta = |props: &mut ParaProps| {
            if let Some(src) = para.props.src {
                set(props, "src", |x| x.src = Some(src));
            }
            if let Some(sb) = para.props.section_break.clone() {
                set(props, "sectionBreak", |x| x.section_break = Some(sb));
            }
        };
        if let Some(c) = c
            && c.author == f.author
            && c.date == f.date
            && json_eq(Some(&para_props_from_ppr(Some(&c.p_pr)) as &dyn JsValue), Some(&*f.old as &dyn JsValue))
        {
            let mut ppr: PPr = (*c.p_pr).clone();
            if let Some(r) = para.ppr.as_deref().and_then(|p| p.r_pr.clone()) {
                set(&mut ppr, "rPr", |x| x.r_pr = Some(r));
            }
            let mut props = para_props_from_ppr(Some(&ppr));
            meta(&mut props);
            return Some((ppr, props));
        }
        let mut props: ParaProps = (*f.old).clone();
        meta(&mut props);
        Some((direct_ppr(para.ppr.as_deref(), &props), props))
    }

    /// `markGone(para)` (revisions.ts:131): the paragraph mark disappears (its text joins the
    /// next paragraph) in this view.
    #[must_use]
    pub fn mark_gone(&self, props: &ParaProps) -> bool {
        if self.mode == RevisionMode::Original {
            return props.mark_ins.is_some();
        }
        let Some(md) = props.mark_del.as_deref() else { return false };
        self.mode != RevisionMode::All || !self.shown(Some(md), MarkKind::InsDel)
    }

    /// `barFor(r, kind)` (revisions.ts:139).
    #[must_use]
    pub fn bar_for(&self, r: Option<&RevisionInfo>, kind: MarkKind) -> bool {
        if r.is_none() || (self.mode != RevisionMode::All && self.mode != RevisionMode::Simple) {
            return false;
        }
        if self.mode == RevisionMode::Simple {
            return if kind == MarkKind::Fmt { self.fmt } else { self.ins_del };
        }
        self.shown(r, kind)
    }

    /// `barColor()` (revisions.ts:146): `None` = no bars.
    #[must_use]
    pub fn bar_color(&self) -> Option<&'static str> {
        match self.mode {
            RevisionMode::All => Some(BAR_COLOR),
            RevisionMode::Simple => Some(SIMPLE_BAR_COLOR),
            _ => None,
        }
    }
}

/// The `REV_JOINS` offset `joinDeleted(carried, next)` records (revisions.ts:157): the UTF-16
/// length of the carried runs' text.
#[must_use]
pub fn join_offset<'r>(carried: impl IntoIterator<Item = &'r Run>) -> u32 {
    let n: usize = carried.into_iter().map(|r| r.text.len()).sum();
    u32::try_from(n).unwrap_or(u32::MAX)
}

/// `revisionRanges(rd, para)`'s answer (revisions.ts:165).
#[derive(Debug, Clone, PartialEq)]
pub struct RevisionRanges {
    /// `[start, end, hidden]` offsets of the runs whose revision asks for a bar
    pub ranges: Vec<(f64, f64, bool)>,
    /// the whole paragraph (tracked paragraph formatting)
    pub whole: bool,
    /// the paragraph mark
    pub mark: bool,
}

/// `revisionRanges(rd, para)` (revisions.ts:165) over the paragraph's runs (props and UTF-16 text
/// length, in order), its props and its `REV_JOINS` offsets.
pub fn revision_ranges<'r>(
    rd: &RevisionDisplay,
    runs: impl IntoIterator<Item = (&'r RunProps, usize)>,
    props: &ParaProps,
    rev_joins: &[u32],
) -> Option<RevisionRanges> {
    rd.bar_color()?;
    let mut ranges = Vec::new();
    let mut acc = 0usize;
    for (rp, len) in runs {
        let rs = acc;
        acc += len;
        if rd.bar_for(rp.ins.as_deref(), MarkKind::InsDel)
            || rd.bar_for(rp.del.as_deref(), MarkKind::InsDel)
            || rd.bar_for(rp.fmt.as_deref().map(fmt_info).as_ref(), MarkKind::Fmt)
        {
            ranges.push((as_f64(rs), as_f64(acc), rd.run(rp) == Some(RunRevision::Hide)));
        }
    }
    // a deleted mark that vanished in the join (Simple Markup): a bar on the join's line
    if rd.mode == RevisionMode::Simple {
        for &k in rev_joins {
            ranges.push((f64::from(k), f64::from(k), true));
        }
    }
    let whole = rd.bar_for(props.pfmt.as_deref().map(pfmt_info).as_ref(), MarkKind::Fmt);
    let mark = rd.bar_for(props.mark_ins.as_deref(), MarkKind::InsDel) || rd.bar_for(props.mark_del.as_deref(), MarkKind::InsDel);
    (!ranges.is_empty() || whole || mark).then_some(RevisionRanges { ranges, whole, mark })
}

fn as_f64(n: usize) -> f64 {
    f64::from(u32::try_from(n).unwrap_or(u32::MAX))
}

/// The authorship of a run format change (`barFor` reads only `author`).
fn fmt_info(f: &crate::model::run::FormatChange) -> RevisionInfo {
    RevisionInfo {
        author: f.author.clone(),
        date: f.date.clone(),
        date_utc: f.date_utc.clone(),
        r#move: f.r#move,
        ..RevisionInfo::default()
    }
}

/// The authorship of a paragraph format change.
fn pfmt_info(f: &crate::model::para::ParaFormatChange) -> RevisionInfo {
    RevisionInfo {
        author: f.author.clone(),
        date: f.date.clone(),
        date_utc: f.date_utc.clone(),
        r#move: f.r#move,
        ..RevisionInfo::default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn palette_and_bars() {
        let order: Vec<Box<str>> = vec!["Ana".into(), "Bia".into()];
        assert_eq!(author_color(&order, "Bia"), "0078D4");
        assert_eq!(author_color(&order, "Zé"), "5C2E91");
        assert_eq!(bar_x(1440.0), 720.0);
        assert_eq!(bar_x(400.0), 268.8);
        let rd = RevisionDisplay::new(RevisionMode::All, None, None, None, None, None, &order);
        let props = RunProps { ins: Some(Box::new(RevisionInfo { author: "Ana".into(), ..Default::default() })), ..Default::default() };
        assert_eq!(rd.run(&props), Some(RunRevision::Marked { color: "D13438".into(), del: false, r#move: false }));
        let r = revision_ranges(&rd, [(&props, 3usize), (&RunProps::default(), 2)], &ParaProps::default(), &[]).unwrap();
        assert_eq!(r.ranges, vec![(0.0, 3.0, false)]);
    }
}
