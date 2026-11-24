//! How Excel 365 WRITES formulas into the `.xlsx` (`file-form.ts`, frontier A5; measured with formulas typed by
//! `Formula2` and saved by Excel):
//!
//! - a formula the LEGACY semantics calculates the same way (the user's `@` are exactly those Excel inserts when it
//!   opens a legacy formula) is written as a plain formula WITHOUT the `@`: `=@A1:A3` → `<f>A1:A3</f>`;
//! - else as a DYNAMIC array: `<c cm="…"><f t="array" ref="area">` with `_xlfn.SINGLE(…)` for the explicit `@`;
//! - `ca="1"`: formulas with a volatile function/name AND everything that depends on them (transitively); a
//!   dynamic one with `ca` also gets `aca="1"`; a blocked spill (`#SPILL!`) gets `aca` + `ca`
//!   ([`crate::save::save_flags`]).
//!
//! Every walk here is iterative (no recursion through operators). [`to_file_formula`] builds its probe trees in
//! the caller's arena and rolls them back.

use crate::ast::{Ast, BinOp, FnName, Node, NodeId};
use crate::implicit::{LegacyHost, ParamClass, fn_class, legacy_to_dynamic, param_class};
use crate::print::{PrintOptions, print_formula};
use crate::strings::JsString;

/// The file form of a formula (`FileFormula`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FileForm {
    /// The `<f>` text (file form: `_xlfn.`, `_xlpm.`…).
    pub text: JsString,
    /// A dynamic array: `t="array"` + `cm`.
    pub dynamic: bool,
}

fn is_single(_ast: &Ast, name: FnName) -> bool {
    matches!(name, FnName::Known(ix) if ix.entry().name == "SINGLE")
}

/// `stripSingles(n)`: the tree without its `@` (`SINGLE` with one argument), copy-on-change.
pub fn strip_singles(ast: &mut Ast, root: NodeId) -> NodeId {
    #[derive(Clone, Copy)]
    enum T {
        Visit(NodeId),
        Fn(NodeId),
        Call(NodeId),
        Unary(NodeId),
        Paren(NodeId),
        Bin(NodeId),
    }
    let mut tasks = vec![T::Visit(root)];
    let mut res: Vec<NodeId> = Vec::new();
    let mut kids: Vec<NodeId> = Vec::new();
    while let Some(t) = tasks.pop() {
        match t {
            T::Visit(id) => match *ast.node(id) {
                Node::Fn { name, args, .. } if is_single(ast, name) && args.len == 1 => tasks.push(T::Visit(ast.kids(args)[0])),
                Node::Fn { args, .. } => {
                    tasks.push(T::Fn(id));
                    tasks.extend(ast.kids(args).iter().rev().map(|&a| T::Visit(a)));
                }
                Node::Call { f, args } => {
                    tasks.push(T::Call(id));
                    tasks.extend(ast.kids(args).iter().rev().map(|&a| T::Visit(a)));
                    tasks.push(T::Visit(f));
                }
                Node::Unary { e, .. } => {
                    tasks.push(T::Unary(id));
                    tasks.push(T::Visit(e));
                }
                Node::Paren(e) => {
                    tasks.push(T::Paren(id));
                    tasks.push(T::Visit(e));
                }
                Node::Bin { a, b, .. } => {
                    tasks.push(T::Bin(id));
                    tasks.push(T::Visit(b));
                    tasks.push(T::Visit(a));
                }
                _ => res.push(id),
            },
            T::Fn(id) | T::Call(id) => {
                let n = *ast.node(id);
                let (args, f) = match n {
                    Node::Fn { args, .. } => (args, None),
                    Node::Call { f, args } => (args, Some(f)),
                    _ => continue,
                };
                let k = args.len as usize;
                let start = res.len().saturating_sub(k);
                kids.clear();
                kids.extend_from_slice(&res[start..]);
                res.truncate(start);
                let f2 = f.map(|f| res.pop().unwrap_or(f));
                if kids.as_slice() == ast.kids(args) && f2 == f {
                    res.push(id);
                    continue;
                }
                let span = ast.push_kids(&kids);
                let pos = ast.pos(id);
                let nn = match n {
                    Node::Fn { name, prefix, sym, .. } => Node::Fn { name, prefix, args: span, sym },
                    _ => Node::Call { f: f2.unwrap_or(NodeId(0)), args: span },
                };
                res.push(ast.push(nn, pos));
            }
            T::Unary(id) => {
                let Node::Unary { op, e } = *ast.node(id) else { continue };
                let e2 = res.pop().unwrap_or(e);
                let pos = ast.pos(id);
                res.push(if e2 == e { id } else { ast.push(Node::Unary { op, e: e2 }, pos) });
            }
            T::Paren(id) => {
                let Node::Paren(e) = *ast.node(id) else { continue };
                let e2 = res.pop().unwrap_or(e);
                let pos = ast.pos(id);
                res.push(if e2 == e { id } else { ast.push(Node::Paren(e2), pos) });
            }
            T::Bin(id) => {
                let Node::Bin { op, a, b } = *ast.node(id) else { continue };
                let b2 = res.pop().unwrap_or(b);
                let a2 = res.pop().unwrap_or(a);
                let pos = ast.pos(id);
                res.push(if a2 == a && b2 == b { id } else { ast.push(Node::Bin { op, a: a2, b: b2 }, pos) });
            }
        }
    }
    res.pop().unwrap_or(root)
}

/// `hasCall(n, isName)`: an expression call (`LAMBDA(…)(…)`, `Name(…)`) — the legacy semantics has none.
pub fn has_call<H: LegacyHost + ?Sized>(ast: &Ast, root: NodeId, host: &H) -> bool {
    let mut st = vec![root];
    while let Some(id) = st.pop() {
        match *ast.node(id) {
            Node::Call { .. } => return true,
            Node::Fn { name, args, .. } => {
                if matches!(name, FnName::Other(_)) && host.is_name(ast, ast.fn_name(name)) {
                    return true;
                }
                st.extend(ast.kids(args).iter().rev());
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

/// `arrayInValue(n)`: an array constant in a VALUE context from the top (`=UPPER({"a";"b"})`): the legacy form
/// would take only the first element, the dynamic one spills — Excel writes it as dynamic (measured).
pub fn array_in_value(ast: &Ast, root: NodeId) -> bool {
    let mut st = vec![root];
    while let Some(id) = st.pop() {
        match *ast.node(id) {
            Node::Arr { rows, cols, .. } => {
                if u64::from(rows) * u64::from(cols) > 1 {
                    return true;
                }
            }
            Node::Paren(e) | Node::Unary { e, .. } => st.push(e),
            Node::Bin { op, a, b } => {
                if !matches!(op, BinOp::Range | BinOp::Isect | BinOp::Union) {
                    st.push(b);
                    st.push(a);
                }
            }
            Node::Fn { name, args, .. } => {
                if is_single(ast, name) {
                    continue;
                }
                let info = match name {
                    FnName::Known(ix) => fn_class(ix.entry().name),
                    FnName::Other(x) => {
                        let s = ast.str(x);
                        if s.units().all(|u| u < 0x80) {
                            fn_class(&s.units().map(|u| u as u8 as char).collect::<String>())
                        } else {
                            fn_class("")
                        }
                    }
                };
                for (i, &a) in ast.kids(args).iter().enumerate().rev() {
                    if param_class(&info, i) == ParamClass::V {
                        st.push(a);
                    }
                }
            }
            _ => {}
        }
    }
    false
}

/// `toFileFormula(node, row, col, o)`: the file form of a formula in DYNAMIC semantics (the 365 formula bar, with
/// explicit `@`) at `(row, col)`. The probe trees are rolled back from the arena.
pub fn to_file_formula<H: LegacyHost + ?Sized>(ast: &mut Ast, node: NodeId, row: u32, col: u32, host: &H) -> FileForm {
    let ui_o = PrintOptions { row, col, r1c1: false, ui: true };
    let file_o = PrintOptions { row, col, r1c1: false, ui: false };
    let ui = print_formula(ast, node, &ui_o);
    if !has_call(ast, node, host) && !array_in_value(ast, node) {
        let mark = ast.mark();
        let legacy = strip_singles(ast, node);
        let back = legacy_to_dynamic(ast, legacy, host);
        let same = print_formula(ast, back.root, &ui_o) == ui;
        let text = same.then(|| print_formula(ast, legacy, &file_o));
        ast.rollback(mark);
        if let Some(text) = text {
            return FileForm { text, dynamic: false };
        }
    }
    FileForm { text: print_formula(ast, node, &file_o), dynamic: true }
}
