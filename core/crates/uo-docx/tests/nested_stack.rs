//! Nested stories on a bounded stack (G20, record "Nested-story stack — fixed"): a text box in a
//! text box, 64 levels deep (the `max_nesting` limit) and past it, held by every kind of object
//! (VML shape, DrawingML shape, DrawingML and VML groups, `mc:AlternateContent`, all of them in
//! turn), read with and without sources, as paragraphs and through the block reader of the full
//! document, then digested and dropped, on a thread of `NESTED_STACK_KIB`. The wasm32 budget
//! (256 KiB) is measured by `js/wasm-stack.mjs`.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

#[path = "support/nested_stories.rs"]
mod nested_stories;

use nested_stories::{NestedRead, Nesting, digest, on_nested_stack, read_nested, story_depth};

/// The deepest levels the XML depth limit (1,000) lets every kind reach.
const LEVELS: [usize; 4] = [1, 64, 65, 80];

#[test]
fn nested_text_boxes_read_within_the_stack_budget() {
    for kind in Nesting::ALL {
        for sources in [false, true] {
            for blocks in [false, true] {
                let how = NestedRead { kind, sources, blocks };
                for levels in LEVELS {
                    let (depth, items) = on_nested_stack(move || {
                        let (paras, media) = read_nested(levels, how).unwrap_or_else(|| panic!("{how:?} {levels}"));
                        (story_depth(&paras), digest(&paras, &media))
                    });
                    // the 65th text box is read, with its content as plain paragraphs (G20)
                    assert_eq!(depth, levels.min(65), "{how:?} {levels}");
                    assert!(items > 0, "{how:?} {levels}");
                }
            }
        }
    }
}

/// The text box past `max_nesting` (the 65th) holds what is below it as plain paragraphs (G20): the
/// core paragraph's text is there.
#[test]
fn the_text_past_the_limit_is_kept() {
    for kind in Nesting::ALL {
        let how = NestedRead { kind, sources: false, blocks: false };
        let (paras, _) = on_nested_stack(move || read_nested(80, how).unwrap());
        let mut texts: Vec<String> = Vec::new();
        let mut cur = paras.first();
        while let Some(p) = cur {
            let tb = p.runs.iter().filter_map(|r| r.ext.as_deref()).find_map(|e| {
                e.textbox.as_deref().or_else(|| {
                    e.group.as_deref().and_then(|g| g.items.iter().find_map(|it| it.run.ext.as_deref().and_then(|x| x.textbox.as_deref())))
                })
            });
            let Some(tb) = tb else { break };
            let inner = tb.content.flat_paragraphs();
            texts = inner.iter().map(|q| q.runs.iter().map(|r| r.text.to_string_lossy()).collect()).collect();
            cur = inner.into_iter().next();
        }
        assert!(texts.iter().any(|t| t == "core"), "{kind:?}: {texts:?}");
    }
}
