//! The hyperlinks of a slide as boxes (wave 4, record `uo-dml-render.md` "Links (wave 4)" and `uo-pdf.md` "Slides PDF"):
//! what a viewer hit-tests and what a PDF places beside the run marks of the emission (`Emission::links`).
//!
//! - [`LinkSource::RunUrl`]: the frozen TS viewer's hit-test (`hitTestHyperlink` of `universal-slides-viewer.ts`): every
//!   shape with an `xfrm` and a text body holding a run with a `hyperlink` (the reader keeps only http/https runs) is one
//!   box, the shape's UNROTATED box `ox + x × scale, oy + y × scale, cx × scale, cy × scale`, with the address of the first
//!   such run ("shape-wide": the TS v1 rule);
//! - [`LinkSource::ShapeClick`]: `a:hlinkClick` on the shape's `p:cNvPr` (a group's click covers its children that have
//!   none); not in the TS viewer;
//! - [`LinkSource::RunJump`]: a run-level `a:hlinkClick` that jumps to a slide or a show position (the model keeps no
//!   such run address), shape-wide like `RunUrl`; not in the TS viewer.
//!
//! Inherited decorations (master/layout shapes) give `RunUrl` boxes only: their clicks and jumps are not resolved through
//! their own parts (declared). Ordered by shape, then `RunUrl`, `ShapeClick`, `RunJump`; at most [`MAX_LINKS`] per slide.

use rustc_hash::FxHashMap;
use uo_dml_render::PartSource;
use uo_dml_render::model::Shape;
use uo_dml_render::source::{RelMap, read_rels, rels_path_for, ta};
use uo_opc::xml::Element;

use crate::deck::Deck;
use crate::emit::View;
use crate::error::PptxError;

/// At most this many boxes per slide (hostile input).
pub const MAX_LINKS: usize = 10_000;

/// `ppaction://hlinkshowjump?jump=…`.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ShowJump {
    Next,
    Previous,
    First,
    Last,
    LastViewed,
    End,
}

/// Where a link goes.
#[derive(Clone, PartialEq, Debug)]
pub enum LinkTarget {
    /// An external address as written in the relationship (any scheme: the caller filters).
    Url(Box<str>),
    /// A slide of the presentation (an index of `Presentation::slides`).
    Slide(u32),
    /// A position of the show.
    Show(ShowJump),
}

/// What made the box.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum LinkSource {
    RunUrl,
    ShapeClick,
    RunJump,
}

/// One link box: the shape (an index of `SlideModel::shapes`), its unrotated box in px of the view (`x, y, w, h`), the
/// target and its source.
#[derive(Clone, PartialEq, Debug)]
pub struct LinkBox {
    pub shape: u32,
    pub rect: [f64; 4],
    pub target: LinkTarget,
    pub source: LinkSource,
}

/// The actions the slide XML gives one `p:cNvPr` id: the shape's own click and the first run-level jump of its text.
#[derive(Default, Clone)]
struct Actions {
    click: Option<LinkTarget>,
    run_jump: Option<LinkTarget>,
}

/// The link boxes of presentation slide `index` at `view`.
///
/// # Errors
/// The slide (or the presentation) cannot be read.
pub fn slide_links(deck: &mut Deck, index: u32, view: View) -> Result<Vec<LinkBox>, PptxError> {
    let slide = deck.slide(index)?;
    let (path, targets): (Option<Box<str>>, Vec<Option<Box<str>>>) = {
        let p = deck.presentation()?;
        (
            p.slides.get(usize::try_from(index).unwrap_or(usize::MAX)).and_then(|r| r.target.clone()),
            p.slides.iter().map(|r| r.target.clone()).collect(),
        )
    };
    // the slide XML's clicks and jumps by cNvPr id, in document order (an id can repeat: consumed in order)
    let mut by_id: FxHashMap<Box<str>, Vec<Actions>> = FxHashMap::default();
    let mut groups: FxHashMap<Box<str>, LinkTarget> = FxHashMap::default();
    if let Some(path) = path.as_deref() {
        let src = deck.source();
        if let Ok(Some(doc)) = src.xml(path) {
            let rels = read_rels(&src, &rels_path_for(path), path).unwrap_or_default();
            if let Some(tree) = doc.root().first_child("p:cSld").and_then(|c| c.first_child("p:spTree")) {
                scan_tree(tree, &rels, &targets, &mut by_id, &mut groups);
            }
        }
    }
    let mut out: Vec<LinkBox> = Vec::new();
    let mut used: FxHashMap<Box<str>, usize> = FxHashMap::default();
    for (i, sh) in slide.shapes.iter().enumerate() {
        if out.len() >= MAX_LINKS {
            break;
        }
        let Ok(shape) = u32::try_from(i) else { break };
        // the TS hit-test: `if (!sh.xfrm || !sh.txBody) continue` (a click needs the box too)
        let Some(x) = sh.xfrm.as_ref() else { continue };
        let rect = [view.ox + x.x * view.scale, view.oy + x.y * view.scale, x.cx * view.scale, x.cy * view.scale];
        if let Some(url) = first_run_url(sh) {
            out.push(LinkBox { shape, rect, target: LinkTarget::Url(url.into()), source: LinkSource::RunUrl });
        }
        if sh.inherited == Some(true) {
            continue;
        }
        let acts = by_id.get(&sh.id).and_then(|v| {
            let k = used.entry(sh.id.clone()).or_insert(0);
            let a = v.get(*k).or_else(|| v.last());
            *k += 1;
            a
        });
        let group_click = || sh.extras.as_ref().and_then(|e| e.group_ids.as_deref()).and_then(|ids| ids.iter().find_map(|g| groups.get(g)));
        if let Some(t) = acts.and_then(|a| a.click.clone()).or_else(|| group_click().cloned()) {
            out.push(LinkBox { shape, rect, target: t, source: LinkSource::ShapeClick });
        }
        if let Some(t) = acts.and_then(|a| a.run_jump.clone()) {
            out.push(LinkBox { shape, rect, target: t, source: LinkSource::RunJump });
        }
    }
    out.truncate(MAX_LINKS);
    Ok(out)
}

/// The address of the first run with a `hyperlink` of the shape's text body (`for p … for r … if (r.hyperlink) return`).
fn first_run_url(sh: &Shape) -> Option<&str> {
    let tb = sh.tx_body.as_deref()?;
    tb.paragraphs.iter().flat_map(|p| p.runs.iter()).find_map(|r| r.extras.as_deref().and_then(|e| e.hyperlink.as_deref()))
}

/// The non-visual properties element of a shape element (`p:nvSpPr`, `p:nvPicPr`, ...).
fn c_nv_pr(el: Element<'_>) -> Option<Element<'_>> {
    el.child_elements().find(|c| c.name().starts_with("p:nv")).and_then(|nv| nv.first_child("p:cNvPr"))
}

/// Walks the shape tree (groups included, iteratively, bounded) collecting the clicks and run jumps by cNvPr id.
fn scan_tree(
    tree: Element<'_>,
    rels: &RelMap,
    slides: &[Option<Box<str>>],
    by_id: &mut FxHashMap<Box<str>, Vec<Actions>>,
    groups: &mut FxHashMap<Box<str>, LinkTarget>,
) {
    const MAX_DEPTH: usize = 64;
    const MAX_SHAPES: usize = 100_000;
    let mut stack: Vec<(Element<'_>, usize)> = tree.child_elements().rev().map(|e| (e, 0)).collect();
    let mut seen = 0usize;
    while let Some((el, depth)) = stack.pop() {
        seen += 1;
        if seen > MAX_SHAPES {
            break;
        }
        let name = el.name();
        match name {
            "p:sp" | "p:pic" | "p:cxnSp" | "p:graphicFrame" | "p:grpSp" => {}
            "mc:AlternateContent" => {
                // the shapes of the choice the reader takes (the first `mc:Choice`, else the fallback): both are scanned, the id
                // queue keeps them apart only when their ids differ
                if depth < MAX_DEPTH {
                    for alt in el.child_elements().rev() {
                        for c in alt.child_elements().rev() {
                            stack.push((c, depth + 1));
                        }
                    }
                }
                continue;
            }
            _ => continue,
        }
        let Some(nv) = c_nv_pr(el) else { continue };
        let id: Box<str> = nv.attr("id").unwrap_or("").into();
        let click = nv.first_child("a:hlinkClick").and_then(|h| action_target(h, rels, slides, false));
        if name == "p:grpSp" {
            if let Some(t) = click {
                groups.entry(id).or_insert(t);
            }
            if depth < MAX_DEPTH {
                for c in el.child_elements().rev() {
                    stack.push((c, depth + 1));
                }
            }
            continue;
        }
        let run_jump = el.first_child("p:txBody").and_then(|tb| {
            tb.descendants()
                .filter(|e| e.name() == "a:rPr")
                .filter_map(|rpr| rpr.first_child("a:hlinkClick"))
                .find_map(|h| action_target(h, rels, slides, true))
        });
        by_id.entry(id).or_default().push(Actions { click, run_jump });
    }
}

/// The target of an `a:hlinkClick`: an external address (action absent or empty), a slide jump or a show jump; other
/// actions (macros, programs, OLE verbs, custom shows, "no action") are none. `jumps_only`: a run's click (its URL is
/// the model's `hyperlink`).
fn action_target(h: Element<'_>, rels: &RelMap, slides: &[Option<Box<str>>], jumps_only: bool) -> Option<LinkTarget> {
    let action = uo_jsnum::text::trim(h.attr("action").unwrap_or(""));
    let rid = ta(h, "r:id").or_else(|| ta(h, "id"));
    if action.is_empty() {
        if jumps_only {
            return None;
        }
        let t = uo_jsnum::text::trim(rels.get(rid?)?);
        return (!t.is_empty()).then(|| LinkTarget::Url(t.into()));
    }
    if action.starts_with("ppaction://hlinksldjump") {
        let part = rels.get(rid?)?;
        let i = slides.iter().position(|s| s.as_deref() == Some(part))?;
        return u32::try_from(i).ok().map(LinkTarget::Slide);
    }
    let jump = action.strip_prefix("ppaction://hlinkshowjump?")?;
    let kind = jump.split('&').find_map(|kv| kv.strip_prefix("jump="))?;
    let j = match kind.to_ascii_lowercase().as_str() {
        "nextslide" => ShowJump::Next,
        "previousslide" => ShowJump::Previous,
        "firstslide" => ShowJump::First,
        "lastslide" => ShowJump::Last,
        "lastslideviewed" => ShowJump::LastViewed,
        "endshow" => ShowJump::End,
        _ => return None,
    };
    Some(LinkTarget::Show(j))
}
