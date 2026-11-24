//! LEGACY formulas (saved without `cm`/array, from before dynamic arrays) → the equivalent dynamic form, inserting
//! the `@` (`SINGLE`) exactly where Excel 365 inserts it when it opens the file (`implicit.ts` +
//! `implicit-classes.ts`, frontier A5). Parameter classes (BIFF's): V = value (a range becomes `@range`), R =
//! reference (a range passes; an expression with an operator becomes a value), A = array (no `@` inside). `top` =
//! the function may return a reference/array: in a value context it gets a `@` in front.
//!
//! **Shapes**: the rewrite builds new nodes in the same arena only along the paths that change (the TS copies
//! every visited node and returns the original when nothing was inserted: the same trees). It runs on an explicit
//! stack (record A2-2: no recursion through operators); the shape tests `multi`/`hasRange`/`hasAnchor` are
//! iterative through operators and parentheses and recurse only through function arguments (≤ `MAX_AST_DEPTH`).
//! The engine runs it once per hash-consed tree and sheet (`execCache`).

use crate::ast::{Ast, BinOp, FnName, Node, NodeId, name_flags};
use crate::fnreg::{ArgMode, FN_TABLE, FnIx};
use crate::strings::JsStr;

/// A parameter class.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ParamClass {
    /// Value.
    V,
    /// Reference.
    R,
    /// Array.
    A,
}

/// The class of a function (`FnClassInfo`): `top` and the class per position (the last repeats).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FnClassInfo {
    /// Gets `@` in front when used as a value.
    pub top: bool,
    params: [ParamClass; 8],
    len: u8,
}

impl FnClassInfo {
    const fn of(top: bool, ps: &[ParamClass]) -> Self {
        let mut params = [ParamClass::V; 8];
        let mut i = 0;
        while i < ps.len() && i < 8 {
            params[i] = ps[i];
            i += 1;
        }
        Self { top, params, len: if ps.len() < 8 { ps.len() as u8 } else { 8 } }
    }
    /// The classes per position.
    #[must_use]
    pub fn params(&self) -> &[ParamClass] {
        &self.params[..usize::from(self.len)]
    }
}

const UDF_CLASS: FnClassInfo = FnClassInfo::of(true, &[ParamClass::R]);
/// `OVERRIDES`: parameters the probe with a "1" filler does not reach (CELL's 2nd needs a reference).
const CELL_CLASS: FnClassInfo = FnClassInfo::of(true, &[ParamClass::V, ParamClass::R]);

/// The measured class of a name (`IMPLICIT_CLASSES`, sorted).
fn measured(name: &str) -> Option<FnClassInfo> {
    let t = &crate::i18n::tables::IMPLICIT_CLASSES;
    t.binary_search_by(|(k, _, _)| (*k).cmp(name)).ok().map(|i| {
        let (_, top, ps) = t[i];
        FnClassInfo::of(top, ps)
    })
}

/// `fnClass(name)`: measured, or deduced from the registry's argument modes.
#[must_use]
pub fn fn_class(name: &str) -> FnClassInfo {
    if name == "CELL" {
        return CELL_CLASS;
    }
    if let Some(m) = measured(name) {
        return m;
    }
    let Some(def) = crate::fnreg::find(JsStr::Latin1(name.as_bytes())).and_then(|ix| ix.entry().def) else {
        return FnClassInfo::of(false, &[ParamClass::V]);
    };
    let mut ps = [ParamClass::V; 8];
    let n = usize::from(def.max_args).min(8);
    for (i, p) in ps.iter_mut().enumerate().take(n) {
        *p = match def.arg_mode(i) {
            ArgMode::Array | ArgMode::Any => ParamClass::A,
            ArgMode::Ref => ParamClass::R,
            _ => ParamClass::V,
        };
    }
    if n == 0 {
        return FnClassInfo::of(def.returns_ref(), &[ParamClass::V]);
    }
    FnClassInfo::of(def.returns_ref(), &ps[..n])
}

/// `paramClass(info, i)`: the class of the i-th parameter (repeating pairs continue the cycle).
#[must_use]
pub fn param_class(info: &FnClassInfo, i: usize) -> ParamClass {
    let ps = info.params();
    let n = ps.len();
    if i < n {
        return ps[i];
    }
    if n >= 4 && ps[n - 1] == ps[n - 3] && ps[n - 2] == ps[n - 4] && ps[n - 1] != ps[n - 2] {
        return ps[n - 2 + (i - n) % 2];
    }
    ps.last().copied().unwrap_or(ParamClass::V)
}

/// The host questions of the rewrite (`LegacyOptions`).
///
/// The arena is passed in because the host's defined names live in the same arena as the tree (the engine's).
pub trait LegacyHost {
    /// May the defined name be several cells? (Excel only shows `@` on a name that is not a single cell nor a
    /// scalar constant.) The TS default (absent) is `true`.
    fn name_is_multi(&self, _ast: &Ast, _name: JsStr<'_>) -> bool {
        true
    }
    /// Is the function name a defined name (a LAMBDA stored in a name — not a UDF)? Default `false`.
    fn is_name(&self, _ast: &Ast, _name: JsStr<'_>) -> bool {
        false
    }
}

/// The TS defaults (`legacyToDynamic(n)` without options).
#[derive(Clone, Copy, Debug, Default)]
pub struct NoNames;
impl LegacyHost for NoNames {}

/// The context of the conversion: V = value from the top; VR = value inside a reference argument (a range still
/// gets `@`, a function returning an ARRAY does not); R = reference; A = array.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Ctx {
    V,
    R,
    A,
    VR,
}

impl From<ParamClass> for Ctx {
    fn from(p: ParamClass) -> Self {
        match p {
            ParamClass::V => Self::V,
            ParamClass::R => Self::R,
            ParamClass::A => Self::A,
        }
    }
}

const fn is_value(c: Ctx) -> bool {
    matches!(c, Ctx::V | Ctx::VR)
}

/// `ctx === 'A' ? 'A' : ctx === 'V' ? 'V' : 'VR'`.
const fn sub(c: Ctx) -> Ctx {
    match c {
        Ctx::A => Ctx::A,
        Ctx::V => Ctx::V,
        _ => Ctx::VR,
    }
}

/// Functions with an array result: no `@` in the VR context (measured with `=SUM(ABS(F(…)))`).
const ARRAY_RESULT: [&str; 34] = [
    "CHOOSECOLS",
    "CHOOSEROWS",
    "DROP",
    "EXPAND",
    "FILTER",
    "FREQUENCY",
    "GROUPBY",
    "GROWTH",
    "HSTACK",
    "IFS",
    "LINEST",
    "LOGEST",
    "MINVERSE",
    "MMULT",
    "MODE.MULT",
    "MUNIT",
    "PIVOTBY",
    "RANDARRAY",
    "REGEXEXTRACT",
    "SEQUENCE",
    "SORT",
    "SORTBY",
    "STOCKHISTORY",
    "SWITCH",
    "TAKE",
    "TEXTSPLIT",
    "TOCOL",
    "TOROW",
    "TRANSPOSE",
    "TREND",
    "UNIQUE",
    "VSTACK",
    "WRAPCOLS",
    "WRAPROWS",
];

/// Functions whose value parameters do NOT become arrays inside an array context.
const NO_ARRAY_PROPAGATION: [&str; 3] = ["IF", "IFERROR", "IFNA"];

/// The rewrite of one tree: [`legacy_to_dynamic`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Legacy {
    /// The dynamic tree (the input itself when nothing was inserted).
    pub root: NodeId,
    /// The inserted `@` nodes with the position in the original text where the `@` goes (`implicitAt`; −1 =
    /// unknown), in insertion order.
    pub inserted: Vec<(NodeId, i64)>,
}

/// The canonical name of a function node as a static/arena text.
fn fn_text(ast: &Ast, name: FnName) -> JsStr<'_> {
    ast.fn_name(name)
}

fn name_is(ast: &Ast, name: FnName, s: &str) -> bool {
    match name {
        FnName::Known(ix) => ix.entry().name == s,
        FnName::Other(x) => ast.str(x).eq_str(s),
    }
}

fn is_anchor_or_tro(ast: &Ast, name: FnName) -> bool {
    match name {
        FnName::Known(ix) => ix.entry().name.starts_with("_TRO_"),
        FnName::Other(x) => {
            let s = ast.str(x);
            s.eq_str("ANCHORARRAY") || (s.len() >= 5 && (0..5).all(|i| s.unit(i) == Some(u16::from(b"_TRO_"[i]))))
        }
    }
}

/// The class used for a function's arguments: `fnClass` of its canonical name (an `Other` name — a defined name
/// called as a function — only reaches the measured table by its exact text).
fn class_of(ast: &Ast, name: FnName) -> FnClassInfo {
    match name {
        FnName::Known(ix) => fn_class(ix.entry().name),
        FnName::Other(x) => {
            let s = ast.str(x);
            if s.units().all(|u| u < 0x80) {
                let t: String = s.units().map(|u| u as u8 as char).collect();
                fn_class(&t)
            } else {
                FnClassInfo::of(false, &[ParamClass::V])
            }
        }
    }
}

fn prefix_is_udf(ast: &Ast, prefix: u32) -> bool {
    if prefix == 0 {
        return false;
    }
    let p = ast.str(crate::ast::StrIx(prefix - 1));
    let starts = |w: &str| {
        p.len() >= w.len() && w.bytes().enumerate().all(|(i, b)| p.unit(i).is_some_and(|u| u < 0x80 && (u as u8).eq_ignore_ascii_case(&b)))
    };
    starts("_xludf.") || starts("_xll.")
}

struct Shape<'a, H: LegacyHost + ?Sized> {
    ast: &'a Ast,
    host: &'a H,
}

impl<H: LegacyHost + ?Sized> Shape<'_, H> {
    fn name_multi(&self, n: &Node) -> bool {
        match *n {
            Node::Name { name, flags, .. } => {
                flags & (name_flags::PARAM | name_flags::ETA) == 0 && self.host.name_is_multi(self.ast, self.ast.str(name))
            }
            _ => false,
        }
    }

    /// `multi(n)`: the expression may be more than one cell.
    fn multi(&self, mut id: NodeId) -> bool {
        loop {
            let n = self.ast.node(id);
            return match *n {
                Node::Area { .. } | Node::Table { .. } => true,
                Node::Name { .. } => self.name_multi(n),
                Node::Paren(e) => {
                    id = e;
                    continue;
                }
                Node::Bin { op, .. } => matches!(op, BinOp::Range | BinOp::Isect | BinOp::Union),
                Node::Fn { name, args, .. } => {
                    name_is(self.ast, name, "ANCHORARRAY") || is_anchor_or_tro(self.ast, name) || {
                        let info = class_of(self.ast, name);
                        self.needs_at(name, self.ast.kids(args), &info)
                    }
                }
                _ => false,
            };
        }
    }

    /// `hasAnchor(n)`.
    fn has_anchor(&self, root: NodeId) -> bool {
        let mut st = vec![root];
        while let Some(id) = st.pop() {
            match *self.ast.node(id) {
                Node::Fn { name, args, .. } => {
                    if name_is(self.ast, name, "ANCHORARRAY") {
                        return true;
                    }
                    st.extend(self.ast.kids(args).iter().rev());
                }
                Node::Unary { e, .. } | Node::Paren(e) => st.push(e),
                Node::Bin { a, b, .. } => {
                    st.push(b);
                    st.push(a);
                }
                _ => {}
            }
        }
        false
    }

    /// `hasRange(n)`: some multi-cell reference inside.
    fn has_range(&self, root: NodeId) -> bool {
        let mut st = vec![root];
        while let Some(id) = st.pop() {
            if self.multi(id) {
                return true;
            }
            match *self.ast.node(id) {
                Node::Unary { e, .. } | Node::Paren(e) => st.push(e),
                Node::Bin { a, b, .. } => {
                    st.push(b);
                    st.push(a);
                }
                Node::Fn { args, .. } => st.extend(self.ast.kids(args).iter().rev()),
                _ => {}
            }
        }
        false
    }

    fn is_missing(&self, id: NodeId) -> bool {
        matches!(self.ast.node(id), Node::Missing)
    }

    /// `needsAt(name, args, info)`: Excel puts `@` in front of the function only when the result may be several
    /// cells, by the shape of the arguments (measured).
    fn needs_at(&self, name: FnName, args: &[NodeId], info: &FnClassInfo) -> bool {
        let nm = match name {
            FnName::Known(ix) => ix.entry().name,
            FnName::Other(_) => "",
        };
        match nm {
            "INDEX" => {
                let Some(&arr) = args.first() else { return false };
                let an = self.ast.node(arr);
                if !self.multi(arr) && !matches!(an, Node::Fn { .. }) {
                    return false;
                }
                let never0 = |a: NodeId| match *self.ast.node(a) {
                    Node::Num { v, .. } => v != 0.0,
                    Node::Fn { name, .. } => name_is(self.ast, name, "MATCH") || name_is(self.ast, name, "XMATCH"),
                    _ => false,
                };
                let (mut count, mut all) = (0, true);
                for &a in args.iter().skip(1).take(2).filter(|&&a| !self.is_missing(a)) {
                    count += 1;
                    all &= never0(a);
                }
                let direct = match *an {
                    Node::Area { .. } | Node::Name { .. } | Node::Table { .. } => true,
                    Node::Paren(e) => matches!(self.ast.node(e), Node::Bin { op: BinOp::Union, .. }),
                    _ => false,
                };
                !(direct && count > 0 && all)
            }
            "IF" | "CHOOSE" | "IFERROR" | "IFNA" => args.iter().skip(1).any(|&a| self.multi(a)),
            "OFFSET" => {
                let first_multi = args.first().is_some_and(|&a| self.multi(a));
                (first_multi && !args.first().is_some_and(|&a| matches!(self.ast.node(a), Node::Cell { .. }))) || args.len() > 3
            }
            "ISFORMULA" | "FORMULATEXT" => args.first().is_some_and(|&a| self.multi(a)),
            "ROW" | "COLUMN" => args.first().is_some_and(|&a| !matches!(self.ast.node(a), Node::Cell { .. } | Node::Missing)),
            "XLOOKUP" => {
                if args.get(3).is_some_and(|&a| self.multi(a)) {
                    return true;
                }
                let Some(&ret) = args.get(2) else { return false };
                if !self.has_range(ret) {
                    return false;
                }
                let d = self.dir(args.get(1).copied());
                !(d.is_some() && d == self.dir(Some(ret)))
            }
            _ => info.top,
        }
    }

    /// `dir(n)` of XLOOKUP: a vertical ('v') or horizontal ('h') vector area with all four parts.
    fn dir(&self, n: Option<NodeId>) -> Option<u8> {
        let Node::Area { r1, c1, r2, c2, .. } = *self.ast.node(n?) else { return None };
        let (r1, c1, r2, c2) = (self.ast.part(r1)?, self.ast.part(c1)?, self.ast.part(r2)?, self.ast.part(c2)?);
        if c1.n == c2.n && c1.abs == c2.abs {
            return Some(b'v');
        }
        if r1.n == r2.n && r1.abs == r2.abs {
            return Some(b'h');
        }
        None
    }
}

#[derive(Clone, Copy, Debug)]
enum Task {
    Visit(NodeId, Ctx),
    Unary(NodeId),
    Bin(NodeId, bool),
    Paren(NodeId),
    Fn(NodeId, bool),
}

/// The canonical `SINGLE` row.
fn single_ix() -> FnIx {
    crate::fnreg::find(JsStr::Latin1(b"SINGLE")).unwrap_or(FnIx(0))
}

/// `legacyToDynamic(n, o)`: inserts the `@` of a legacy formula (value context at the top). `inserted` lists the
/// new `SINGLE` nodes (empty = `root` is the input).
pub fn legacy_to_dynamic<H: LegacyHost + ?Sized>(ast: &mut Ast, root: NodeId, host: &H) -> Legacy {
    legacy_to_dynamic_in(ast, root, host, &mut LegacyBufs::default())
}

/// The work buffers of [`legacy_to_dynamic_in`], kept between calls.
#[derive(Debug, Default)]
pub struct LegacyBufs {
    tasks: Vec<Task>,
    res: Vec<NodeId>,
    kids: Vec<NodeId>,
}

/// [`legacy_to_dynamic`] with reused buffers (the only allocation of a rewrite that inserts nothing is none).
pub fn legacy_to_dynamic_in<H: LegacyHost + ?Sized>(ast: &mut Ast, root: NodeId, host: &H, bufs: &mut LegacyBufs) -> Legacy {
    let single = single_ix();
    debug_assert_eq!(FN_TABLE[single.0 as usize].name, "SINGLE");
    let LegacyBufs { tasks, res, kids } = bufs;
    tasks.clear();
    res.clear();
    tasks.push(Task::Visit(root, Ctx::V));
    let mut inserted: Vec<(NodeId, i64)> = Vec::new();
    // `SINGLE(e)`: a new `_xlfn.SINGLE(e)` node marked with the position of `e`
    let wrap = |ast: &mut Ast, e: NodeId, inserted: &mut Vec<(NodeId, i64)>| -> NodeId {
        let pos = ast.pos(e).map_or(-1, i64::from);
        let sym = ast.intern(JsStr::Latin1(b"SINGLE"));
        let prefix = ast.intern(JsStr::Latin1(b"_xlfn.")).0 + 1;
        let span = ast.push_kids(&[e]);
        let id = ast.push(Node::Fn { name: FnName::Known(single), prefix, args: span, sym }, None);
        inserted.push((id, pos));
        id
    };
    while let Some(t) = tasks.pop() {
        match t {
            Task::Visit(id, ctx) => {
                let n = *ast.node(id);
                match n {
                    Node::Area { .. } | Node::Table { .. } => {
                        let r = if is_value(ctx) { wrap(ast, id, &mut inserted) } else { id };
                        res.push(r);
                    }
                    Node::Name { .. } => {
                        let multi = Shape { ast: &*ast, host }.name_multi(&n);
                        let r = if is_value(ctx) && multi { wrap(ast, id, &mut inserted) } else { id };
                        res.push(r);
                    }
                    Node::Unary { e, .. } => {
                        tasks.push(Task::Unary(id));
                        tasks.push(Task::Visit(e, sub(ctx)));
                    }
                    Node::Bin { op, a, b } => {
                        if matches!(op, BinOp::Range | BinOp::Isect | BinOp::Union) {
                            // the union stays without @ (measured: =(A1:A3;B1) unchanged)
                            tasks.push(Task::Bin(id, is_value(ctx) && op != BinOp::Union));
                            tasks.push(Task::Visit(b, Ctx::R));
                            tasks.push(Task::Visit(a, Ctx::R));
                        } else {
                            tasks.push(Task::Bin(id, false));
                            tasks.push(Task::Visit(b, sub(ctx)));
                            tasks.push(Task::Visit(a, sub(ctx)));
                        }
                    }
                    Node::Paren(e) => {
                        tasks.push(Task::Paren(id));
                        tasks.push(Task::Visit(e, ctx));
                    }
                    Node::Fn { name, prefix, args, .. } => {
                        if name_is(ast, name, "SINGLE") {
                            res.push(id);
                            continue;
                        }
                        // A1# returns a reference: as a value it gets @ (measured: FORMULATEXT → "=@F4#")
                        if is_anchor_or_tro(ast, name) || name_is(ast, name, "ANCHORARRAY") {
                            let r = if is_value(ctx) { wrap(ast, id, &mut inserted) } else { id };
                            res.push(r);
                            continue;
                        }
                        // a user-defined/add-in function (_xludf., _xll., unknown name): may return an array —
                        // @ at the top and the arguments pass as references (measured: .xls ISEVEN add-in →
                        // "=@ISEVEN(A1)")
                        let udf =
                            prefix_is_udf(ast, prefix) || (matches!(name, FnName::Other(_)) && !host.is_name(ast, fn_text(ast, name)));
                        let info = if udf { UDF_CLASS } else { class_of(ast, name) };
                        let argv = ast.kids(args);
                        let shape = Shape { ast: &*ast, host };
                        // IF with A1# in the condition: the condition becomes an array and the @ goes on the IF
                        let if_anchor = name_is(ast, name, "IF") && argv.first().is_some_and(|&a| shape.has_anchor(a));
                        let at = if if_anchor {
                            is_value(ctx)
                        } else {
                            match ctx {
                                Ctx::V => shape.needs_at(name, argv, &info),
                                Ctx::VR => {
                                    let array_result = matches!(name, FnName::Known(ix) if ARRAY_RESULT.contains(&ix.entry().name));
                                    !array_result && shape.needs_at(name, argv, &info)
                                }
                                _ => false,
                            }
                        };
                        let is_type = name_is(ast, name, "TYPE");
                        let no_prop = matches!(name, FnName::Known(ix) if NO_ARRAY_PROPAGATION.contains(&ix.entry().name));
                        tasks.push(Task::Fn(id, at));
                        for i in (0..argv.len()).rev() {
                            let a = ast.kids(args)[i];
                            let c = if if_anchor && i == 0 {
                                Ctx::A
                            } else {
                                // TYPE(A1#) stays without @ (measured; TYPE(A1:A3) gets it)
                                let anchor_arg =
                                    is_type && matches!(*ast.node(a), Node::Fn { name: nm, .. } if name_is(ast, nm, "ANCHORARRAY"));
                                let cls = if anchor_arg { ParamClass::R } else { param_class(&info, i) };
                                // inside an array argument value/reference ones evaluate as arrays too
                                // (=SUMPRODUCT(SUM(A1:A3*1)) has no @), except IF/IFERROR/IFNA
                                if ctx == Ctx::A && cls != ParamClass::A && !no_prop {
                                    Ctx::A
                                } else if matches!(ctx, Ctx::R | Ctx::VR) && cls == ParamClass::V {
                                    Ctx::VR
                                } else {
                                    Ctx::from(cls)
                                }
                            };
                            tasks.push(Task::Visit(a, c));
                        }
                    }
                    _ => res.push(id),
                }
            }
            Task::Unary(id) => {
                let Node::Unary { op, e } = *ast.node(id) else { continue };
                let e2 = res.pop().unwrap_or(e);
                let r = if e2 == e { id } else { rebuild(ast, id, Node::Unary { op, e: e2 }) };
                res.push(r);
            }
            Task::Paren(id) => {
                let Node::Paren(e) = *ast.node(id) else { continue };
                let e2 = res.pop().unwrap_or(e);
                let r = if e2 == e { id } else { rebuild(ast, id, Node::Paren(e2)) };
                res.push(r);
            }
            Task::Bin(id, single_it) => {
                let Node::Bin { op, a, b } = *ast.node(id) else { continue };
                let b2 = res.pop().unwrap_or(b);
                let a2 = res.pop().unwrap_or(a);
                let r = if a2 == a && b2 == b { id } else { rebuild(ast, id, Node::Bin { op, a: a2, b: b2 }) };
                let r = if single_it { wrap(ast, r, &mut inserted) } else { r };
                res.push(r);
            }
            Task::Fn(id, at) => {
                let Node::Fn { name, prefix, args, sym } = *ast.node(id) else { continue };
                let k = args.len as usize;
                let start = res.len().saturating_sub(k);
                kids.clear();
                kids.extend_from_slice(&res[start..]);
                res.truncate(start);
                let same = kids.as_slice() == ast.kids(args);
                let f = if same {
                    id
                } else {
                    let span = ast.push_kids(kids);
                    rebuild(ast, id, Node::Fn { name, prefix, args: span, sym })
                };
                let r = if at { wrap(ast, f, &mut inserted) } else { f };
                res.push(r);
            }
        }
    }
    let out = res.pop().unwrap_or(root);
    if inserted.is_empty() { Legacy { root, inserted } } else { Legacy { root: out, inserted } }
}

/// A copy of `id` with new children (`{ ...n, … }`: the position is kept).
fn rebuild(ast: &mut Ast, id: NodeId, n: Node) -> NodeId {
    let pos = ast.pos(id);
    ast.push(n, pos)
}

/// `implicitPositions(exec)`: the positions where the conversion put `@` (−1 = unknown), from a [`Legacy`].
#[must_use]
pub fn implicit_positions(l: &Legacy) -> Vec<i64> {
    l.inserted.iter().map(|&(_, p)| p).collect()
}

/// `sourceText` (`engine.ts:486-495`): the original text with the conversion's `@` inserted at their positions
/// (dedup, right to left); `None` when a position is unknown.
#[must_use]
pub fn source_text(text: JsStr<'_>, positions: &[i64]) -> Option<crate::strings::JsString> {
    if positions.iter().any(|&p| p < 0) {
        return None;
    }
    let mut ps: Vec<i64> = positions.to_vec();
    ps.sort_unstable_by(|a, b| b.cmp(a));
    ps.dedup();
    let mut t: Vec<u16> = text.units().collect();
    for p in ps {
        // `t.slice(0, p) + '@' + t.slice(p)` (a position past the end appends)
        let at = usize::try_from(p).unwrap_or(usize::MAX).min(t.len());
        t.insert(at, u16::from(b'@'));
    }
    Some(crate::strings::JsString::from_utf16(&t))
}
