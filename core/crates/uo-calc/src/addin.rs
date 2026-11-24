//! Analysis ToolPak functions called as ADD-INS in `.xls` files (`addin.ts`, frontier A5): the converter delivers
//! them as `_xll.NAME(…)`. Excel 365 resolves them BY THE NAME IN ITS LANGUAGE (measured in pt-BR: the names Excel
//! wrote are already translated; an English name that differs stays `=@ISEVEN(A1)` with `#NAME?`).
//!
//! The rewrite is copy-on-change in the arena on an explicit stack (no recursion through operators). A resolved
//! node is `{ t: 'fn', name: canon, args }` in the TS: no prefix and no position.

use crate::ast::{Ast, FnName, Node, NodeId};
use crate::i18n::{FormulaLanguage, Translator};
use crate::strings::JsStr;

/// The ATP functions (Excel 97–2003 add-in), native today (`ATP_FUNCTIONS`), sorted.
pub const ATP_FUNCTIONS: [&str; 93] = [
    "ACCRINT",
    "ACCRINTM",
    "AMORDEGRC",
    "AMORLINC",
    "BESSELI",
    "BESSELJ",
    "BESSELK",
    "BESSELY",
    "BIN2DEC",
    "BIN2HEX",
    "BIN2OCT",
    "COMPLEX",
    "CONVERT",
    "COUPDAYBS",
    "COUPDAYS",
    "COUPDAYSNC",
    "COUPNCD",
    "COUPNUM",
    "COUPPCD",
    "CUMIPMT",
    "CUMPRINC",
    "DEC2BIN",
    "DEC2HEX",
    "DEC2OCT",
    "DELTA",
    "DISC",
    "DOLLARDE",
    "DOLLARFR",
    "DURATION",
    "EDATE",
    "EFFECT",
    "EOMONTH",
    "ERF",
    "ERFC",
    "FACTDOUBLE",
    "FVSCHEDULE",
    "GCD",
    "GESTEP",
    "HEX2BIN",
    "HEX2DEC",
    "HEX2OCT",
    "IMABS",
    "IMAGINARY",
    "IMARGUMENT",
    "IMCONJUGATE",
    "IMCOS",
    "IMDIV",
    "IMEXP",
    "IMLN",
    "IMLOG10",
    "IMLOG2",
    "IMPOWER",
    "IMPRODUCT",
    "IMREAL",
    "IMSIN",
    "IMSQRT",
    "IMSUB",
    "IMSUM",
    "INTRATE",
    "ISEVEN",
    "ISODD",
    "LCM",
    "MDURATION",
    "MROUND",
    "MULTINOMIAL",
    "NETWORKDAYS",
    "NOMINAL",
    "OCT2BIN",
    "OCT2DEC",
    "OCT2HEX",
    "ODDFPRICE",
    "ODDFYIELD",
    "ODDLPRICE",
    "ODDLYIELD",
    "PRICE",
    "PRICEDISC",
    "PRICEMAT",
    "QUOTIENT",
    "RANDBETWEEN",
    "RECEIVED",
    "SERIESSUM",
    "SQRTPI",
    "TBILLEQ",
    "TBILLPRICE",
    "TBILLYIELD",
    "WEEKNUM",
    "WORKDAY",
    "XIRR",
    "XNPV",
    "YEARFRAC",
    "YIELD",
    "YIELDDISC",
    "YIELDMAT",
];

/// `/_xll\./i.test(text)`: the formula text calls an add-in (ASCII case-insensitive).
#[must_use]
pub fn has_addin_call(text: JsStr<'_>) -> bool {
    let n = text.len();
    let pat = b"_xll.";
    (0..n.saturating_sub(4)).any(|i| (0..5).all(|k| text.unit(i + k).is_some_and(|u| u < 0x80 && (u as u8).eq_ignore_ascii_case(&pat[k]))))
}

fn prefix_is_xll(ast: &Ast, prefix: u32) -> bool {
    if prefix == 0 {
        return false;
    }
    let p = ast.str(crate::ast::StrIx(prefix - 1));
    p.len() >= 5 && (0..5).all(|k| p.unit(k).is_some_and(|u| u < 0x80 && (u as u8).eq_ignore_ascii_case(&b"_xll."[k])))
}

#[derive(Clone, Copy)]
enum Task {
    Visit(NodeId),
    Fn(NodeId),
    Call(NodeId),
    Unary(NodeId),
    Paren(NodeId),
    Bin(NodeId),
}

/// `resolveAddinCalls(n, lang)`: the `_xll.NAME(…)` calls of the ATP resolved by the name in `lang`; the rest stays
/// (`#NAME?`). Returns `root` itself when nothing changed.
pub fn resolve_addin_calls(ast: &mut Ast, root: NodeId, lang: &FormulaLanguage) -> NodeId {
    let mut tr = Translator::new();
    let mut tasks = vec![Task::Visit(root)];
    let mut res: Vec<NodeId> = Vec::new();
    let mut kids: Vec<NodeId> = Vec::new();
    let mut units: Vec<u16> = Vec::new();
    while let Some(t) = tasks.pop() {
        match t {
            Task::Visit(id) => match *ast.node(id) {
                Node::Fn { args, .. } => {
                    tasks.push(Task::Fn(id));
                    tasks.extend(ast.kids(args).iter().rev().map(|&a| Task::Visit(a)));
                }
                Node::Call { f, args } => {
                    tasks.push(Task::Call(id));
                    tasks.extend(ast.kids(args).iter().rev().map(|&a| Task::Visit(a)));
                    tasks.push(Task::Visit(f));
                }
                Node::Unary { e, .. } => {
                    tasks.push(Task::Unary(id));
                    tasks.push(Task::Visit(e));
                }
                Node::Paren(e) => {
                    tasks.push(Task::Paren(id));
                    tasks.push(Task::Visit(e));
                }
                Node::Bin { a, b, .. } => {
                    tasks.push(Task::Bin(id));
                    tasks.push(Task::Visit(b));
                    tasks.push(Task::Visit(a));
                }
                _ => res.push(id),
            },
            Task::Fn(id) => {
                let Node::Fn { name, prefix, args, sym } = *ast.node(id) else { continue };
                let k = args.len as usize;
                let start = res.len().saturating_sub(k);
                kids.clear();
                kids.extend_from_slice(&res[start..]);
                res.truncate(start);
                if prefix_is_xll(ast, prefix) {
                    units.clear();
                    units.extend(ast.fn_name(name).units());
                    if let Some(canon) = tr.fn_lookup(lang, &units)
                        && ATP_FUNCTIONS.binary_search(&canon).is_ok()
                        && let Some(ix) = crate::fnreg::find(JsStr::Latin1(canon.as_bytes()))
                    {
                        let span = ast.push_kids(&kids);
                        let sym2 = ast.intern(JsStr::Latin1(canon.as_bytes()));
                        res.push(ast.push(Node::Fn { name: FnName::Known(ix), prefix: 0, args: span, sym: sym2 }, None));
                        continue;
                    }
                }
                if kids.as_slice() == ast.kids(args) {
                    res.push(id);
                } else {
                    let span = ast.push_kids(&kids);
                    let pos = ast.pos(id);
                    res.push(ast.push(Node::Fn { name, prefix, args: span, sym }, pos));
                }
            }
            Task::Call(id) => {
                let Node::Call { f, args } = *ast.node(id) else { continue };
                let k = args.len as usize;
                let start = res.len().saturating_sub(k);
                kids.clear();
                kids.extend_from_slice(&res[start..]);
                res.truncate(start);
                let f2 = res.pop().unwrap_or(f);
                if f2 == f && kids.as_slice() == ast.kids(args) {
                    res.push(id);
                } else {
                    let span = ast.push_kids(&kids);
                    let pos = ast.pos(id);
                    res.push(ast.push(Node::Call { f: f2, args: span }, pos));
                }
            }
            Task::Unary(id) => {
                let Node::Unary { op, e } = *ast.node(id) else { continue };
                let e2 = res.pop().unwrap_or(e);
                let pos = ast.pos(id);
                res.push(if e2 == e { id } else { ast.push(Node::Unary { op, e: e2 }, pos) });
            }
            Task::Paren(id) => {
                let Node::Paren(e) = *ast.node(id) else { continue };
                let e2 = res.pop().unwrap_or(e);
                let pos = ast.pos(id);
                res.push(if e2 == e { id } else { ast.push(Node::Paren(e2), pos) });
            }
            Task::Bin(id) => {
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
