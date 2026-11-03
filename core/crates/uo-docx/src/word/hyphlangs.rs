//! `documentHyphenationLanguages(full, fallback, force)` (`docx-layout/src/hyphenate.ts:65`): the
//! `w:lang` values of a document with automatic hyphenation, for the host to load the patterns
//! before laying out (LQ16: the layout's `hyphenation_loaded` input, computed by the binding).
//!
//! The TS walks the whole model reflectively (`Object.entries`, every object, `Map` values,
//! arrays) and adds `v.val` of every key `lang` whose value is an object with a string `val`. K5:
//! here the walk is an explicit list of every place such a key can sit, visited in the TS order
//! (each object's keys in insertion order, the TS views of a story in their key order):
//!
//! | TS object | keys visited (in the object's key order) |
//! |---|---|
//! | `fmt.styles.styles` (record order) | `StyleDef.pPr`, `.rPr`, `.tblStylePr[*].pPr`/`.rPr` |
//! | `full.blocks`, `footnoteBlocks`/`endnoteBlocks` values, `headerBlocks`/`footerBlocks` | a paragraph block's `paragraph`; a table's `rows[*].cells[*]` |
//! | `TableCell` | `paragraphs` (direct), `tables` (direct), then `blocks` (already seen) |
//! | `Section` | `headers`/`footers` (the parts' flat paragraph views), then the blocks |
//! | `Paragraph` | `runs`, `ppr` |
//! | `Run` | `rpr`, then the rare keys: `textbox` (`paras` view, then `blocks`), `fieldResult`, `custom.data` (any JSON), `group.items[*].run`, `math.nodes` |
//! | `RPr` | `lang` (the site), `change.rPr` |
//! | `PPr` | `rPr`, `change.pPr` |
//! | `MathNode` | `num`, `den`, `rpr` |
//!
//! No other model object can hold a `lang` key (`RunProps`, `ParaProps`, sections' `sectPr`/`sect`,
//! drawings, effects and the numbering are not visited or have none). The output order is the
//! order of first appearance, as the TS `Set`.

use indexmap::IndexSet;

use crate::js::{JsObject, JsonValue, Slot};
use crate::model::formatting::{StyleDef, TblStylePart};
use crate::model::para::Paragraph;
use crate::model::props::{PPr, RPr};
use crate::model::run::{DrawingTextbox, MathNode, Run, TextboxView};
use crate::model::story::{Block, Story};
use crate::model::table::{Cell, Table};
use crate::view::DocView;

struct Walk {
    out: IndexSet<Box<str>>,
}

impl Walk {
    fn rpr(&mut self, r: &RPr) {
        for (k, s) in r.slots_in_order() {
            if !matches!(s, Slot::Val(_)) {
                continue;
            }
            match k {
                "lang" => {
                    if let Some(v) = r.lang.as_deref().and_then(|l| l.val.as_deref()) {
                        self.out.insert(v.into());
                    }
                }
                "change" => {
                    if let Some(c) = r.change.as_deref() {
                        self.rpr(&c.r_pr);
                    }
                }
                _ => {}
            }
        }
    }

    fn ppr(&mut self, p: &PPr) {
        for (k, s) in p.slots_in_order() {
            if !matches!(s, Slot::Val(_)) {
                continue;
            }
            match k {
                "rPr" => {
                    if let Some(r) = p.r_pr.as_deref() {
                        self.rpr(r);
                    }
                }
                "change" => {
                    if let Some(c) = p.change.as_deref() {
                        self.ppr(&c.p_pr);
                    }
                }
                _ => {}
            }
        }
    }

    fn style_part(&mut self, t: &TblStylePart) {
        for (k, _) in t.slots_in_order() {
            match k {
                "pPr" => {
                    if let Some(p) = t.p_pr.as_deref() {
                        self.ppr(p);
                    }
                }
                "rPr" => {
                    if let Some(r) = t.r_pr.as_deref() {
                        self.rpr(r);
                    }
                }
                _ => {}
            }
        }
    }

    fn style(&mut self, s: &StyleDef) {
        for (k, _) in s.slots_in_order() {
            match k {
                "pPr" => {
                    if let Some(p) = s.p_pr.as_deref() {
                        self.ppr(p);
                    }
                }
                "rPr" => {
                    if let Some(r) = s.r_pr.as_deref() {
                        self.rpr(r);
                    }
                }
                "tblStylePr" => {
                    if let Some(parts) = &s.tbl_style_pr {
                        for (_, part) in parts.iter() {
                            self.style_part(part);
                        }
                    }
                }
                _ => {}
            }
        }
    }

    fn json(&mut self, v: &JsonValue) {
        match v {
            JsonValue::Array(a) => {
                for x in a {
                    self.json(x);
                }
            }
            JsonValue::Object(o) => {
                for (k, x) in o.iter() {
                    if k == "lang"
                        && let JsonValue::Object(l) = x
                        && let Some(JsonValue::Str(val)) = l.get("val")
                    {
                        self.out.insert(String::from_utf16_lossy(val.units()).into());
                    } else {
                        self.json(x);
                    }
                }
            }
            _ => {}
        }
    }

    fn math(&mut self, n: &MathNode) {
        for (k, _) in n.slots_in_order() {
            match k {
                "num" => n.num.iter().flatten().for_each(|m| self.math(m)),
                "den" => n.den.iter().flatten().for_each(|m| self.math(m)),
                "rpr" => {
                    if let Some(r) = n.rpr.as_deref() {
                        self.rpr(r);
                    }
                }
                _ => {}
            }
        }
    }

    fn textbox(&mut self, t: &DrawingTextbox) {
        // `paras` first (the flat view), then `blocks`
        match t.view {
            TextboxView::Phantom => self.blocks(&t.content),
            TextboxView::Synced | TextboxView::ParasOnly => {
                for p in t.content.flat_paragraphs() {
                    self.paragraph(p);
                }
                if t.view == TextboxView::Synced {
                    self.blocks(&t.content);
                }
            }
        }
    }

    fn run(&mut self, r: &Run) {
        if let Some(rpr) = r.rpr.as_deref() {
            self.rpr(rpr);
        }
        let Some(ext) = r.ext() else { return };
        for (k, _) in ext.slots_in_order() {
            match k {
                "textbox" => {
                    if let Some(t) = ext.textbox.as_deref() {
                        self.textbox(t);
                    }
                }
                "fieldResult" => {
                    for x in ext.field_result.iter().flatten() {
                        self.run(x);
                    }
                }
                "custom" => {
                    if let Some(c) = ext.custom.as_deref() {
                        self.json(&c.data);
                    }
                }
                "group" => {
                    if let Some(g) = ext.group.as_deref() {
                        for it in &g.items {
                            self.run(&it.run);
                        }
                    }
                }
                "math" => {
                    if let Some(m) = ext.math.as_deref() {
                        for n in &m.nodes {
                            self.math(n);
                        }
                    }
                }
                _ => {}
            }
        }
    }

    fn paragraph(&mut self, p: &Paragraph) {
        for (k, _) in p.slots_in_order() {
            match k {
                "runs" => {
                    for r in &p.runs {
                        self.run(r);
                    }
                }
                "ppr" => {
                    if let Some(x) = p.ppr.as_deref() {
                        self.ppr(x);
                    }
                }
                _ => {}
            }
        }
    }

    fn cell(&mut self, c: &Cell) {
        for p in c.content.paras.iter() {
            self.paragraph(p);
        }
        for a in c.content.tables.iter() {
            self.table(&a.table);
        }
    }

    fn table(&mut self, t: &Table) {
        for row in &t.rows {
            for c in &row.cells {
                self.cell(c);
            }
        }
    }

    fn blocks(&mut self, s: &Story) {
        for b in s.blocks() {
            match b {
                Block::Paragraph(p) => self.paragraph(p),
                Block::Table(t) => self.table(t),
            }
        }
    }
}

/// `documentHyphenationLanguages(full, fallback, force)`: empty without `w:autoHyphenation`
/// (unless `force`, the editor's Manual Hyphenation).
#[must_use]
pub fn document_hyphenation_languages(view: DocView<'_>, fallback: &str, force: bool) -> Vec<Box<str>> {
    let fmt = view.formatting();
    if !fmt.settings.auto_hyphenation && !force {
        return Vec::new();
    }
    let first: Box<str> = fmt
        .styles
        .doc_defaults
        .r_pr
        .as_deref()
        .and_then(|r| r.lang.as_deref())
        .and_then(|l| l.val.clone())
        .unwrap_or_else(|| fallback.into());
    let mut w = Walk { out: IndexSet::from([first]) };
    for (_, s) in fmt.styles.styles.iter() {
        w.style(s);
    }
    w.blocks(view.body());
    for (i, sec) in view.sections().iter().enumerate() {
        let _ = sec;
        for which in [crate::view::HfWhich::Header, crate::view::HfWhich::Footer] {
            for (_, part) in view.hf_slots(i, which) {
                if let Some(story) = view.hf_story(part) {
                    for p in story.flat_paragraphs() {
                        w.paragraph(p);
                    }
                }
            }
        }
    }
    for s in view.notes().footnotes.values() {
        w.blocks(s);
    }
    for s in view.notes().endnotes.values() {
        w.blocks(s);
    }
    w.out.into_iter().collect()
}
