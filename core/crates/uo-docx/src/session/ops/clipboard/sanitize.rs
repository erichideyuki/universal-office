//! The paste under the formatting policy (D2b-F8): `policyObjectOk`, `clipListKind`,
//! `policyClip`, `tableClipParas`, `policyTable`, `policyStyleId`, `styleKeyOf`
//! (universal-write.ts 2049–2068, 5273–5414) over the pure half of `formatting-policy.ts` (D2a's
//! `policy.rs`).

use std::sync::Arc;

use super::super::super::Session;
use super::super::super::clipboard::{Clip, ClipHtmlList, ClipPara, ClipRun, ClipTable};
use super::super::super::policy::{
    Flag, ListKind, ResolvedPolicy, StyleResolver, builtin_name_of_key, list_kind_of_format, policy_list, sanitize_para_props,
    sanitize_run_props, style_key,
};
use super::super::styles::sheet::StyleIndex;
use super::super::tables::table_ops;
use super::copy::clip_mark;
use super::paste::PasteCtx;
use super::rx;
use crate::js::JsNumber;
use crate::model::para::{NumberingProps, ParaProps};
use crate::model::run::Run;
use crate::model::story::OwnedBlock;
use crate::model::table::Table;
use crate::text::Utf16;
use crate::word::obj;

fn is_object(r: &Run) -> bool {
    r.ext().is_some_and(|e| e.image.is_some() || e.shape.is_some() || e.textbox.is_some() || e.group.is_some())
}

fn kind_of(s: &str) -> ListKind {
    if s == "bullet" { ListKind::Bullet } else { ListKind::Decimal }
}

/// The `StyleResolver` of a paste: the key of a source style (by the source's `w:name` for a copy
/// of another document, else the live `styleKeyOf`) and the target's id of an allowed key
/// (`policyStyleId`, which may define a built-in).
struct Resolver<'a> {
    s: &'a mut Session,
    other: bool,
    names: &'a [(crate::js::Str, crate::js::Str)],
    idx: Arc<StyleIndex>,
}

impl StyleResolver for Resolver<'_> {
    fn key_of(&self, id: &str) -> String {
        if self.other
            && let Some((_, n)) = self.names.iter().find(|(k, _)| &**k == id)
        {
            return style_key(n);
        }
        style_key_of_live(&self.idx, self.s.styles.xml().is_some(), self.s.f2.lang, id)
    }
    fn id_of(&mut self, key: &str) -> Option<String> {
        let out = self.s.policy_style_id(key);
        self.idx = self.s.style_index();
        out
    }
}

/// `styleKeyOf(id)` on the live style sheet: the key of the style's `w:name`, else of the listed
/// style's raw name, else of the id.
fn style_key_of_live(idx: &StyleIndex, has_xml: bool, lang: super::super::styles::sheet::UiLang, id: &str) -> String {
    if !has_xml {
        return style_key(id);
    }
    if let Some(m) = idx.styles.iter().find(|s| s.id == id)
        && !m.name.is_empty()
    {
        return style_key(&m.name);
    }
    let all = super::super::styles::xform::list_styles_all(idx, lang);
    match all.iter().find(|s| s.id == id) {
        Some(s) => style_key(&s.raw_name),
        None => style_key(id),
    }
}

impl Session {
    /// `policyStyleId(key)`: the id (defined in `styles.xml`) of the allowed style of a key; `None`
    /// = Normal / none. The styles are editable in every document the session opens (`.docx` and
    /// `.doc`; `stylesEditable()` is false for ODT only).
    pub(crate) fn policy_style_id(&mut self, key: &str) -> Option<String> {
        if key == "normal" {
            return None;
        }
        if let Some(b) = builtin_name_of_key(key) {
            return self.builtin_style_id(&b).ok().map(|s| s.to_string());
        }
        let idx = self.style_index();
        let all = super::super::styles::xform::list_styles_all(&idx, self.f2.lang);
        all.into_iter().find(|s| style_key(&s.raw_name) == key || style_key(&s.id) == key).map(|s| s.id)
    }

    /// `policyObjectOk(run, rp)`.
    pub(crate) fn policy_object_ok(&self, r: &Run) -> bool {
        let Some((_, rp)) = &self.policy else { return true };
        let Some(e) = r.ext() else { return true };
        if e.textbox.is_some() || e.shape.is_some() || e.group.is_some() {
            return rp.allows(Flag::Shapes);
        }
        if e.image.is_some() {
            return rp.allows(Flag::Images);
        }
        true
    }

    /// `clipListKind(num, srcLists)`.
    fn clip_list_kind(&mut self, num: &NumberingProps, src_lists: Option<&[(f64, Arc<str>)]>) -> ListKind {
        let id = num.num_id.map_or(0.0, |x| x.to_number());
        if id > 0.0 {
            if let Some(lists) = src_lists {
                let raw = lists.iter().find(|(k, _)| *k == id).map(|(_, r)| Arc::clone(r)).filter(|r| !r.is_empty());
                let ilvl = match num.ilvl {
                    JsNumber::Num(x) => uo_jsnum::format::to_js_string(x),
                    JsNumber::Null => "null".to_owned(),
                };
                let fmt = raw.and_then(|r| rx::level_num_fmt(&r, &ilvl));
                return list_kind_of_format(fmt.as_deref());
            }
            let lvl = self.num_level(id, num.ilvl.to_number());
            return list_kind_of_format(lvl.as_ref().and_then(|l| l.num_fmt.as_deref()));
        }
        kind_of(&num.kind)
    }

    /// `policyClip(paras, rp, src?, tables?)`: the paragraphs under the policy (only the allowed
    /// formatting, the nearest allowed styles, any list → bullet/numbered, objects not allowed out,
    /// tables not allowed → their cells' paragraphs). The copy is not changed (a repaste reuses it).
    pub(crate) fn policy_clip(
        &mut self,
        paras: &[ClipPara],
        rp: &ResolvedPolicy,
        src: Option<&Arc<Clip>>,
        tables: &[ClipTable],
    ) -> (Vec<ClipPara>, Vec<ClipTable>) {
        let other = src.is_some_and(|c| c.origin != Some(self.clip_origin()));
        let empty: Vec<(crate::js::Str, crate::js::Str)> = Vec::new();
        let names = src.map_or(&empty[..], |c| &c.style_names[..]);
        let src_lists: Option<Vec<(f64, Arc<str>)>> = if other { src.map(|c| c.lists.clone()) } else { None };
        let conv = |s: &mut Session, cp: &ClipPara| -> ClipPara {
            let runs: Vec<ClipRun> = cp
                .runs
                .iter()
                .filter(|r| r.obj.as_deref().is_none_or(|o| s.policy_object_ok(o)))
                .map(|r| {
                    if r.obj.is_some() {
                        r.clone()
                    } else {
                        ClipRun { props: Arc::new(sanitize_run_props(&r.props, Some(rp))), ..r.clone() }
                    }
                })
                .collect();
            let mut out = ClipPara { runs, mark: cp.mark.clone(), marker: cp.marker.clone(), html_list: None };
            if let Some(hl) = cp.html_list
                && let Some((k, level)) = policy_list(Some(rp), if hl.ordered { ListKind::Decimal } else { ListKind::Bullet }, hl.level)
            {
                out.html_list = Some(ClipHtmlList { ordered: k == ListKind::Decimal, level, key: hl.key });
            }
            if let Some(mark) = &cp.mark {
                let idx = s.style_index();
                let mut res = Resolver { s, other, names, idx };
                let mut m = sanitize_para_props(mark, Some(rp), &mut res);
                let s = res.s;
                let num = m.numbering.clone();
                if let (Some(_), Some(num)) = (rp.lists, num)
                    && num.num_id.is_none_or(|x| x.to_number() != 0.0)
                {
                    // a list of another format: bullet or numbered, a new list per source list
                    obj::delete(&mut m, "numbering");
                    let kind = s.clip_list_kind(&num, src_lists.as_deref());
                    if let Some((k, level)) = policy_list(Some(rp), kind, num.ilvl.to_number()) {
                        let base = match num.num_id {
                            Some(x) => x.to_number(),
                            None => {
                                if &*num.kind == "bullet" {
                                    -1.0
                                } else {
                                    -2.0
                                }
                            }
                        };
                        out.html_list = Some(ClipHtmlList { ordered: k == ListKind::Decimal, level, key: 200_000.0 + base });
                    }
                }
                out.mark = Some(Arc::new(m));
            }
            out
        };
        if tables.is_empty() || rp.allows(Flag::Tables) {
            let ps: Vec<ClipPara> = paras.iter().map(|cp| conv(self, cp)).collect();
            let ts: Vec<ClipTable> = tables
                .iter()
                .map(|t| {
                    let mut ids = std::mem::take(&mut self.doc.ids);
                    let c = table_ops::clone_table(&t.table, &mut ids);
                    self.doc.ids = ids;
                    ClipTable { before: t.before, table: Arc::new(self.policy_table(c, rp, src)) }
                })
                .collect();
            return (ps, ts);
        }
        // tables not allowed: the cells' content comes in as paragraphs, where they were
        let mut out: Vec<ClipPara> = Vec::new();
        for (i, cp) in paras.iter().enumerate() {
            if i > 0 {
                for t in tables {
                    if t.before as usize == i {
                        for p in table_clip_paras(&t.table) {
                            out.push(conv(self, &p));
                        }
                    }
                }
            }
            out.push(conv(self, cp));
        }
        (out, Vec::new())
    }

    /// `policyTable(t, rp, src?)`: a pasted table under the policy (the table stays): its cells'
    /// content cleaned like pasted paragraphs, the source XML (`w:pPr`/`w:rPr`) dropped.
    pub(crate) fn policy_table(&mut self, mut t: Table, rp: &ResolvedPolicy, src: Option<&Arc<Clip>>) -> Table {
        enum Work {
            Table(Vec<(usize, usize, usize)>),
            Cell(Vec<(usize, usize, usize)>, usize, usize),
        }
        let other = src.is_some_and(|c| c.origin != Some(self.clip_origin()));
        let empty: Vec<(crate::js::Str, crate::js::Str)> = Vec::new();
        let names = src.map_or(&empty[..], |c| &c.style_names[..]);
        let src_lists: Option<Vec<(f64, Arc<str>)>> = if other { src.map(|c| c.lists.clone()) } else { None };
        let mut ctx = PasteCtx::new(false, Vec::new());
        let allows_shapes = rp.allows(Flag::Shapes);
        let allows_images = rp.allows(Flag::Images);
        // the TS recursion `table → cells → (paragraphs, nested tables)`, depth first, as a stack
        let mut stack: Vec<Work> = vec![Work::Table(Vec::new())];
        while let Some(w) = stack.pop() {
            match w {
                Work::Table(path) => {
                    let Some(tb) = table_at_path(&mut t, &path) else { continue };
                    let mut cells: Vec<(usize, usize)> = Vec::new();
                    for (ri, row) in tb.rows.iter().enumerate() {
                        for ci in 0..row.cells.len() {
                            cells.push((ri, ci));
                        }
                    }
                    for (ri, ci) in cells.into_iter().rev() {
                        stack.push(Work::Cell(path.clone(), ri, ci));
                    }
                }
                Work::Cell(path, ri, ci) => {
                    let props0: Vec<ParaProps> =
                        match table_at_path(&mut t, &path).and_then(|tb| tb.rows.get(ri)).and_then(|r| r.cells.get(ci)) {
                            Some(cell) => cell.content.paras.iter().map(|p| (*p.props).clone()).collect(),
                            None => continue,
                        };
                    let mut new_props: Vec<ParaProps> = Vec::with_capacity(props0.len());
                    for p0 in props0 {
                        let idx = self.style_index();
                        let mut res = Resolver { s: self, other, names, idx };
                        let mut props = sanitize_para_props(&p0, Some(rp), &mut res);
                        let num = props.numbering.clone();
                        if let (Some(_), Some(num)) = (rp.lists, num)
                            && num.num_id.is_none_or(|x| x.to_number() != 0.0)
                        {
                            let kind = self.clip_list_kind(&num, src_lists.as_deref());
                            match policy_list(Some(rp), kind, num.ilvl.to_number()) {
                                Some((k, level)) => {
                                    let key = 300_000.0 + num.num_id.map_or(0.0, |x| x.to_number());
                                    let n =
                                        self.html_list_numbering(ClipHtmlList { ordered: k == ListKind::Decimal, level, key }, &mut ctx);
                                    obj::set(&mut props, "numbering", |x| x.numbering = Some(n));
                                }
                                None => obj::delete(&mut props, "numbering"),
                            }
                        }
                        obj::delete(&mut props, "src");
                        obj::delete(&mut props, "fmtSrc");
                        new_props.push(props);
                    }
                    let Some(cell) = table_at_path(&mut t, &path).and_then(|tb| tb.rows.get_mut(ri)).and_then(|r| r.cells.get_mut(ci))
                    else {
                        continue;
                    };
                    for (pi, props) in new_props.into_iter().enumerate() {
                        let Some(p) = cell.content.paras.make_mut(pi) else { continue };
                        p.props = Arc::new(props);
                        obj::delete(p, "ppr");
                        obj::delete(p, "pprEdited");
                        p.runs.retain(|r| {
                            let Some(e) = r.ext() else { return true };
                            if e.textbox.is_some() || e.shape.is_some() || e.group.is_some() {
                                return allows_shapes;
                            }
                            if e.image.is_some() {
                                return allows_images;
                            }
                            true
                        });
                        for r in &mut p.runs {
                            if is_object(r) {
                                continue;
                            }
                            let mut np = sanitize_run_props(&r.props, Some(rp));
                            obj::delete(&mut np, "src");
                            r.props = Arc::new(np);
                            obj::delete(r, "rpr");
                            obj::delete(r, "rprEdited");
                        }
                    }
                    for k in (0..cell.content.tables.len()).rev() {
                        let mut p = path.clone();
                        p.push((ri, ci, k));
                        stack.push(Work::Table(p));
                    }
                }
            }
        }
        t
    }
}

/// The table at a path of (row, cell, table index) steps.
fn table_at_path<'a>(t: &'a mut Table, path: &[(usize, usize, usize)]) -> Option<&'a mut Table> {
    let mut cur = t;
    for &(ri, ci, k) in path {
        let cell = cur.rows.get_mut(ri)?.cells.get_mut(ci)?;
        let a = cell.content.tables.make_mut(k)?;
        cur = Arc::make_mut(&mut a.table);
    }
    Some(cur)
}

/// `tableClipParas(t)`: the paragraphs of a table's cells in order (nested tables included), as a
/// copy; deleted runs left out.
#[must_use]
pub fn table_clip_paras(t: &Table) -> Vec<ClipPara> {
    let mut out: Vec<ClipPara> = Vec::new();
    // an explicit stack of block lists (the TS recursion)
    let mut stack: Vec<std::vec::IntoIter<OwnedBlock>> = Vec::new();
    let cells = |t: &Table| -> Vec<OwnedBlock> {
        let mut v = Vec::new();
        for row in &t.rows {
            for cell in &row.cells {
                v.extend(table_ops::cell_content(cell));
            }
        }
        v
    };
    stack.push(cells(t).into_iter());
    while let Some(top) = stack.last_mut() {
        let Some(b) = top.next() else {
            stack.pop();
            continue;
        };
        match b {
            OwnedBlock::Table(nt) => stack.push(cells(&nt).into_iter()),
            OwnedBlock::Paragraph(p) => {
                let runs: Vec<ClipRun> = p
                    .runs
                    .iter()
                    .filter(|r| r.props.del.is_none())
                    .map(|r| {
                        let base = ClipRun { props: Arc::clone(&r.props), link: r.rpr_link.clone(), ..ClipRun::default() };
                        if is_object(r) {
                            let mut o = r.clone();
                            o.rpr_link = None;
                            ClipRun { obj: Some(Box::new(o)), ..base }
                        } else {
                            let e = r.ext();
                            let text = match (e.and_then(|x| x.custom.as_deref()), e.and_then(|x| x.note_ref.as_ref())) {
                                (Some(c), _) => c.fallback_text.clone(),
                                (None, Some(_)) => Utf16::new(),
                                _ => r.text.clone(),
                            };
                            ClipRun { text, ..base }
                        }
                    })
                    .collect();
                out.push(ClipPara { runs, mark: Some(Arc::new(clip_mark(&p.props))), ..ClipPara::default() });
            }
        }
    }
    out
}
