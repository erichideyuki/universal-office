//! The FINAL view of the track-changes state (docx-core `revisions.ts` `finalRuns`,
//! `finalParagraphs`; the part of that module that exports and the diff read): the document as if
//! everything were accepted. The rest of `revisions.ts` (listing, accepting, tracked editing) is the
//! session's (D2a); when it lands it takes these two functions over from here.

use std::borrow::Cow;

use crate::model::{Paragraph, Run};

/// Whether the run is a deletion in revision (`isDeletedRun`: `!!r.props.del`).
#[must_use]
pub fn is_deleted_run(r: &Run) -> bool {
    r.props.del.is_some()
}

/// `{ ...p, runs }`: the paragraph with other runs, everything else shared.
fn with_runs(p: &Paragraph, runs: Vec<Run>) -> Paragraph {
    Paragraph {
        runs,
        props: p.props.clone(),
        ppr: p.ppr.clone(),
        ppr_edited: p.ppr_edited,
        cleared: p.cleared,
        keys: p.keys.clone(),
        id: p.id,
    }
}

/// The result of [`final_paragraphs`].
#[derive(Debug, Clone)]
pub struct FinalParagraphs<'a> {
    /// the paragraphs of the final view (borrowed when nothing changed in them)
    pub paras: Vec<Cow<'a, Paragraph>>,
    /// `index[i]`: the position, in `paras`, of the paragraph that receives the text of the `i`-th
    pub index: Vec<usize>,
}

/// `finalParagraphs(paras)`: without the deleted text, and a paragraph whose mark is deleted joined
/// to the next one, with the next one's formatting (the mark that is left).
pub fn final_paragraphs<'a>(paras: impl IntoIterator<Item = &'a Paragraph>) -> FinalParagraphs<'a> {
    let all: Vec<&'a Paragraph> = paras.into_iter().collect();
    let last = all.len().saturating_sub(1);
    let mut out: Vec<Cow<'a, Paragraph>> = Vec::with_capacity(all.len());
    let mut index = Vec::with_capacity(all.len());
    let mut carry: Vec<Run> = Vec::new();
    for (i, &p) in all.iter().enumerate() {
        index.push(out.len());
        let has_deleted = p.runs.iter().any(is_deleted_run);
        if carry.is_empty() && !has_deleted {
            if p.props.mark_del.is_some() && i < last {
                carry = p.runs.clone();
                continue;
            }
            out.push(Cow::Borrowed(p));
            continue;
        }
        let mut runs = std::mem::take(&mut carry);
        if has_deleted {
            runs.extend(p.runs.iter().filter(|r| !is_deleted_run(r)).cloned());
        } else {
            runs.extend(p.runs.iter().cloned());
        }
        if p.props.mark_del.is_some() && i < last {
            carry = runs;
            continue;
        }
        out.push(Cow::Owned(with_runs(p, runs)));
    }
    FinalParagraphs { paras: out, index }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::*;
    use crate::ids::ParaId;
    use crate::model::props::RevisionInfo;
    use crate::model::{ParaProps, RunProps};
    use crate::text::Utf16;

    fn run(text: &str, deleted: bool) -> Run {
        let mut props = RunProps::default();
        if deleted {
            props.del = Some(Box::new(RevisionInfo::default()));
        }
        Run { text: Utf16::from(text), props: Arc::new(props), ..Run::default() }
    }

    fn para(id: u32, runs: Vec<Run>, mark_del: bool) -> Paragraph {
        let mut props = ParaProps::default();
        if mark_del {
            props.mark_del = Some(Box::new(RevisionInfo::default()));
        }
        Paragraph { runs, props: Arc::new(props), id: ParaId(id), ..Paragraph::default() }
    }

    fn texts(f: &FinalParagraphs<'_>) -> Vec<String> {
        f.paras.iter().map(|p| p.runs.iter().map(|r| r.text.to_string_lossy()).collect::<String>()).collect()
    }

    #[test]
    fn untouched_paragraphs_are_borrowed() {
        let a = [para(1, vec![run("a", false)], false), para(2, vec![run("b", false)], false)];
        let f = final_paragraphs(a.iter());
        assert!(f.paras.iter().all(|p| matches!(p, Cow::Borrowed(_))));
        assert_eq!(f.index, [0, 1]);
    }

    #[test]
    fn deleted_text_goes_and_a_deleted_mark_joins_the_next() {
        let a = [
            para(1, vec![run("a", false), run("x", true)], true),
            para(2, vec![run("b", false)], false),
            para(3, vec![run("c", false)], true),
        ];
        let f = final_paragraphs(a.iter());
        assert_eq!(texts(&f), ["ab", "c"]);
        assert_eq!(f.index, [0, 0, 1]);
        assert_eq!(f.paras.first().map(|p| p.id), Some(ParaId(2)));
    }
}
