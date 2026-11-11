//! The names the F10 session capture gives to field runs and paragraphs
//! (`js/capture-d2b-f10-session.mjs`, `walk` / `fieldKeys` / `paraOrd` in `PAGE_SETUP`): the body walk in
//! document order (blocks, table cells in order, nested tables too), a field run keyed by its ordinal
//! among the field runs (`<n>`, and `<n>.<j>` for the j-th run of its `fieldResult`), a paragraph by its
//! ordinal among the paragraphs of the body (the notes take `f<id>:<n>` / `e<id>:<n>` keys, no paragraph
//! ordinal). A field run in a header/footer part, in a text box (any story) or a group item gets the engine-doc
//! name of `facets-f8.mjs names(full)` with an `x:` prefix: `<story>:<path>#<run>` with `/f<j>` (field result)
//! or `/g<j>` (group item), stories `hf<n>` (first appearance over sections x [headers, footers] x
//! [default, first, even]), `<run>/tx` (text box), `<run>/g<j>/tx` (group member's box). What `names(full)`
//! names deeper (a result of a group item) has no `RunRef` and is the capture's `unk`.
#![allow(dead_code, clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::collections::HashMap;

use uo_docx::model::story::{Block, Story};
use uo_docx::view::{BlockPath, DocView, NoteKind, RunRef, RunSub, StoryRef};

/// What one walk of a body names.
#[derive(Default)]
pub struct Names {
    /// every named field run, in order
    pub fields: Vec<(String, RunRef)>,
    /// every paragraph: its ordinal and its path
    pub paras: Vec<(u32, BlockPath)>,
}

impl Names {
    pub fn run_of(&self) -> HashMap<&str, &RunRef> {
        self.fields.iter().map(|(k, r)| (k.as_str(), r)).collect()
    }

    pub fn key_of(&self) -> HashMap<&RunRef, &str> {
        self.fields.iter().map(|(k, r)| (r, k.as_str())).collect()
    }

    pub fn ord_of(&self) -> HashMap<&BlockPath, u32> {
        self.paras.iter().map(|(o, p)| (p, *o)).collect()
    }
}

pub fn names(view: DocView<'_>) -> Names {
    let mut out = Names::default();
    let mut fields = 0u32;
    let mut paras = 0u32;
    walk(view.body(), None, &StoryRef::Body, "", &mut out, &mut fields, &mut paras, 0);
    xnames(view, &mut out);
    // the notes: footnotes then endnotes, in the order of their maps; fields keyed `f<id>:<n>` / `e<id>:<n>`;
    // their paragraphs take no ordinal (the page labels are the body's)
    for (kind, tag, map) in [(NoteKind::Footnote, 'f', &view.notes().footnotes), (NoteKind::Endnote, 'e', &view.notes().endnotes)] {
        for (id, story) in map.iter() {
            let mut n = 0u32;
            let mut skip = 0u32;
            let prefix = format!("{tag}{}:", uo_jsnum::format::to_js_string(id.get()));
            walk(story, None, &StoryRef::Note(kind, *id), &prefix, &mut out, &mut n, &mut skip, 1);
        }
    }
    out
}

#[allow(clippy::too_many_arguments)]
fn walk(
    s: &Story,
    base: Option<(&BlockPath, u32, u32)>,
    story: &StoryRef,
    prefix: &str,
    out: &mut Names,
    fields: &mut u32,
    paras: &mut u32,
    depth: usize,
) {
    if depth > 8 {
        return;
    }
    let in_body = matches!(story, StoryRef::Body);
    for (bi, b) in s.blocks().enumerate() {
        let bi = u32::try_from(bi).unwrap();
        let path = match base {
            None => BlockPath::root(bi),
            Some((tp, r, c)) => tp.cell_block(r, c, bi),
        };
        match b {
            Block::Paragraph(p) => {
                if in_body {
                    out.paras.push((*paras, path.clone()));
                    *paras += 1;
                }
                for (ri, r) in p.runs.iter().enumerate() {
                    let ri = u32::try_from(ri).unwrap();
                    if r.ext().and_then(|e| e.field.as_deref()).is_none_or(str::is_empty) {
                        continue;
                    }
                    let k = format!("{prefix}{}", *fields);
                    *fields += 1;
                    out.fields.push((k.clone(), RunRef { story: *story, path: path.clone(), run: ri, sub: RunSub::None }));
                    for (j, x) in r.ext().and_then(|e| e.field_result.as_ref()).into_iter().flatten().enumerate() {
                        if x.ext().and_then(|e| e.field.as_deref()).is_some_and(|f| !f.is_empty()) {
                            let j = u32::try_from(j).unwrap();
                            out.fields.push((
                                format!("{k}.{j}"),
                                RunRef { story: *story, path: path.clone(), run: ri, sub: RunSub::FieldResult(j) },
                            ));
                        }
                    }
                }
            }
            Block::Table(t) => {
                for (ri, row) in t.rows.iter().enumerate() {
                    for (ci, cell) in row.cells.iter().enumerate() {
                        walk(
                            &cell.content,
                            Some((&path, u32::try_from(ri).unwrap(), u32::try_from(ci).unwrap())),
                            story,
                            prefix,
                            out,
                            fields,
                            paras,
                            depth + 1,
                        );
                    }
                }
            }
        }
    }
}

/// The `x:` names (see the module doc).
fn xnames(view: DocView<'_>, out: &mut Names) {
    use std::collections::HashSet;
    use uo_docx::ids::HfPartId;
    use uo_docx::model::run::Run;
    // header/footer parts, numbered by first appearance
    let mut hf: Vec<HfPartId> = Vec::new();
    for sec in view.sections() {
        for set in [&sec.headers, &sec.footers] {
            for k in ["default", "first", "even"] {
                if let Some(&p) = set.get(k)
                    && !hf.contains(&p)
                {
                    hf.push(p);
                }
            }
        }
    }
    struct Cx<'a> {
        out: &'a mut Names,
        boxes: HashSet<uo_docx::ids::DrawingId>,
    }
    fn has_field(r: &Run) -> bool {
        r.ext().and_then(|e| e.field.as_deref()).is_some_and(|f| !f.is_empty())
    }
    fn walk(cx: &mut Cx<'_>, s: &Story, story: &str, sref: StoryRef, x_story: bool, base: Option<(&BlockPath, u32, u32)>, depth: usize) {
        if depth > 8 {
            return;
        }
        for (bi, b) in s.blocks().enumerate() {
            let bi = u32::try_from(bi).unwrap();
            let path = match base {
                None => BlockPath::root(bi),
                Some((tp, r, c)) => tp.cell_block(r, c, bi),
            };
            match b {
                Block::Paragraph(p) => {
                    for (ri, r) in p.runs.iter().enumerate() {
                        let ri = u32::try_from(ri).unwrap();
                        let rf = format!("{story}:{path}#{ri}");
                        let mut push = |name: String, sub: RunSub| {
                            cx.out.fields.push((format!("x:{name}"), RunRef { story: sref, path: path.clone(), run: ri, sub }));
                        };
                        if x_story && has_field(r) {
                            push(rf.clone(), RunSub::None);
                        }
                        if x_story {
                            for (j, q) in r.ext().and_then(|e| e.field_result.as_ref()).into_iter().flatten().enumerate() {
                                if has_field(q) {
                                    let j = u32::try_from(j).unwrap();
                                    push(format!("{rf}/f{j}"), RunSub::FieldResult(j));
                                }
                            }
                        }
                        for (j, it) in r.ext().and_then(|e| e.group.as_deref()).into_iter().flat_map(|g| g.items.iter()).enumerate() {
                            if has_field(&it.run) {
                                let j = u32::try_from(j).unwrap();
                                push(format!("{rf}/g{j}"), RunSub::GroupItem(j));
                            }
                        }
                        let mut boxes: Vec<(String, &uo_docx::model::run::DrawingTextbox)> = Vec::new();
                        if let Some(t) = r.ext().and_then(|e| e.textbox.as_deref()) {
                            boxes.push((format!("{rf}/tx"), t));
                        }
                        for (j, it) in r.ext().and_then(|e| e.group.as_deref()).into_iter().flat_map(|g| g.items.iter()).enumerate() {
                            if let Some(t) = it.run.ext().and_then(|e| e.textbox.as_deref()) {
                                boxes.push((format!("{rf}/g{j}/tx"), t));
                            }
                        }
                        for (st, t) in boxes {
                            if cx.boxes.insert(t.drawing) {
                                walk(cx, &t.content, &st, StoryRef::Textbox(t.drawing), true, None, depth + 1);
                            }
                        }
                    }
                }
                Block::Table(t) => {
                    for (r, row) in t.rows.iter().enumerate() {
                        for (c, cell) in row.cells.iter().enumerate() {
                            walk(
                                cx,
                                &cell.content,
                                story,
                                sref,
                                x_story,
                                Some((&path, u32::try_from(r).unwrap(), u32::try_from(c).unwrap())),
                                depth + 1,
                            );
                        }
                    }
                }
            }
        }
    }
    let mut cx = Cx { out, boxes: HashSet::new() };
    walk(&mut cx, view.body(), "b", StoryRef::Body, false, None, 0);
    for (i, p) in hf.iter().enumerate() {
        if let Some(s) = view.hf_story(*p) {
            walk(&mut cx, s, &format!("hf{i}"), StoryRef::HeaderFooter(*p), true, None, 0);
        }
    }
    for (kind, map) in [(NoteKind::Footnote, &view.notes().footnotes), (NoteKind::Endnote, &view.notes().endnotes)] {
        let tag = if kind == NoteKind::Footnote { "fn" } else { "en" };
        for (id, s) in map.iter() {
            walk(&mut cx, s, &format!("{tag}{}", uo_jsnum::format::to_js_string(id.get())), StoryRef::Note(kind, *id), false, None, 0);
        }
    }
}
