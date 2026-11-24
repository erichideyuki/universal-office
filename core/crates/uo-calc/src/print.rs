//! Tree → formula text (`print.ts`). FILE form (default: canonical OOXML, `_xlfn.`/`_xlpm.`, `_xlfn.SINGLE(…)`,
//! `Tabela[[#This Row],[Col]]`) or SCREEN form (`ui`: `@x`, `A1#`, `[@Col]`, no prefixes). A1 needs the host cell
//! (relative references are offsets); R1C1 does not. The text is built as UTF-16 units (names and literals may
//! hold lone surrogates). Iterative (a stack of tasks): any tree prints in constant native stack.

use crate::a1::{is_js_space, sheet_name_needs_quote, wrap_col, wrap_row};
use crate::ast::{Ast, BinOp, FnName, Node, NodeId, Part, RefPart, SheetSpec, StrIx, TableItem, UnaryOp, name_flags};
use crate::fnreg::{CatPrefix, catalog_entry, lookup_fn};
use crate::strings::{JsStr, JsString};
use crate::value::{Scalar, StrRef};

/// `PrintOptions`.
#[derive(Clone, Copy, Debug, Default)]
pub struct PrintOptions {
    /// The formula's row (0-based) for relative A1.
    pub row: u32,
    /// The formula's column.
    pub col: u32,
    /// R1C1.
    pub r1c1: bool,
    /// Screen form.
    pub ui: bool,
}

/// `numberLiteral(v)`: a number as Excel writes it in a formula (`1E+20`, `0.5`).
#[must_use]
pub fn number_literal(v: f64) -> String {
    crate::cell::number_literal(v)
}

/// `printFormula(n, o)` (no `=`).
#[must_use]
pub fn print_formula(ast: &Ast, n: NodeId, o: &PrintOptions) -> JsString {
    let mut out = Vec::new();
    print_into(ast, n, o, &mut out);
    JsString::from_utf16(&out)
}

/// `printFormula` appended to a unit buffer (no allocation besides its growth).
pub fn print_into(ast: &Ast, n: NodeId, o: &PrintOptions, out: &mut Vec<u16>) {
    Printer { ast, o, out }.run(n);
}

/// A step of the iterative printer (pushed in reverse output order).
#[derive(Clone, Copy)]
enum Task {
    /// Print a node.
    Node(NodeId),
    /// One unit.
    Ch(u16),
    /// ASCII text.
    Str(&'static str),
    /// A function's file prefix (when not on screen) and name.
    FnName(FnName, u32, bool),
    /// Remember the output length (the start of a TRIMRANGE argument).
    Mark,
    /// Replace the last `:` printed since the mark by the TRIMRANGE dots.
    TroFix(&'static str),
}

struct Printer<'a> {
    ast: &'a Ast,
    o: &'a PrintOptions,
    out: &'a mut Vec<u16>,
}

fn push_ascii(out: &mut Vec<u16>, s: &str) {
    out.extend(s.bytes().map(u16::from));
}

fn push_js(out: &mut Vec<u16>, s: JsStr<'_>) {
    out.extend(s.units());
}

fn push_num(out: &mut Vec<u16>, x: f64) {
    push_ascii(out, &uo_jsnum::format::to_js_string(x));
}

/// `colToLetters` of a double (NaN or ≤ -1 → empty, as the TS loop never runs).
fn push_letters(out: &mut Vec<u16>, col: f64) {
    let mut buf = [0u16; 16];
    let mut n = 0;
    let mut c = col + 1.0;
    while c > 0.0 && n < buf.len() {
        let r = (c - 1.0) % 26.0;
        buf[n] = 65 + crate::cast::to_u32(r) as u16;
        n += 1;
        c = ((c - 1.0) / 26.0).floor();
    }
    out.extend(buf[..n].iter().rev());
}

/// `scalarLiteral(v)` of an array constant.
fn scalar_literal(ast: &Ast, out: &mut Vec<u16>, v: &Scalar) {
    match *v {
        Scalar::Num(x) => push_ascii(out, &number_literal(x)),
        Scalar::Str(r) => quoted(out, str_of(ast, r)),
        Scalar::Bool(b) => push_ascii(out, if b { "TRUE" } else { "FALSE" }),
        Scalar::Err(e) => push_ascii(out, crate::cell::error_text(e)),
        Scalar::Empty => {}
    }
}

fn str_of(ast: &Ast, r: StrRef) -> JsStr<'_> {
    ast.lit_str(r).unwrap_or(JsStr::EMPTY)
}

/// `"${v.replace(/"/g, '""')}"`.
fn quoted(out: &mut Vec<u16>, s: JsStr<'_>) {
    out.push(0x22);
    for u in s.units() {
        out.push(u);
        if u == 0x22 {
            out.push(u);
        }
    }
    out.push(0x22);
}

/// `COL_SPECIAL`: `[\s,:.[\]#'"{}$^&*+=\-<>/]`.
fn col_special(s: JsStr<'_>) -> bool {
    s.units().any(|u| is_js_space(u) || b",:.[]#'\"{}$^&*+=-<>/".iter().any(|&c| u16::from(c) == u))
}

/// `escapeCol`: `[`, `]`, `#`, `'` prefixed with `'`.
fn push_escaped_col(out: &mut Vec<u16>, s: JsStr<'_>) {
    for u in s.units() {
        if matches!(u, 0x5b | 0x5d | 0x23 | 0x27) {
            out.push(0x27);
        }
        out.push(u);
    }
}

/// `TRO`: file `_xlfn._TRO_*` ↔ screen `:.`, `.:`, `.:.`.
fn tro(name: JsStr<'_>) -> Option<&'static str> {
    if name.eq_str("_TRO_TRAILING") {
        Some(":.")
    } else if name.eq_str("_TRO_LEADING") {
        Some(".:")
    } else if name.eq_str("_TRO_ALL") {
        Some(".:.")
    } else {
        None
    }
}

/// `sheetPrefix(s)`.
fn sheet_prefix(ast: &Ast, out: &mut Vec<u16>, s: Option<&SheetSpec>) {
    let Some(s) = s else { return };
    let mut ext: Vec<u16> = Vec::new();
    if let Some(e) = s.ext {
        ext.push(0x5b);
        push_num(&mut ext, e);
        ext.push(0x5d);
    }
    let sheet = ast.str(s.sheet);
    if sheet.is_empty() {
        out.extend_from_slice(&ext);
        out.push(0x21);
        return;
    }
    let need = sheet_name_needs_quote(sheet) || s.sheet2.is_some_and(|s2| sheet_name_needs_quote(ast.str(s2)));
    let mut body = ext;
    push_js(&mut body, sheet);
    if let Some(s2) = s.sheet2 {
        body.push(0x3a);
        push_js(&mut body, ast.str(s2));
    }
    if need {
        out.push(0x27);
        for &u in &body {
            out.push(u);
            if u == 0x27 {
                out.push(u);
            }
        }
        out.push(0x27);
    } else {
        out.extend_from_slice(&body);
    }
    out.push(0x21);
}

impl Printer<'_> {
    fn part(&self, p: Part) -> RefPart {
        self.ast.part(p).unwrap_or(RefPart { n: f64::NAN, abs: true })
    }

    fn row_text(&mut self, p: Part) {
        let p = self.part(p);
        if self.o.r1c1 {
            if p.abs {
                self.out.push(u16::from(b'R'));
                push_num(self.out, p.n + 1.0);
            } else if p.n != 0.0 && !p.n.is_nan() {
                push_ascii(self.out, "R[");
                push_num(self.out, p.n);
                self.out.push(0x5d);
            } else {
                self.out.push(u16::from(b'R'));
            }
            return;
        }
        if p.abs {
            self.out.push(0x24);
        }
        let r = if p.abs { p.n } else { wrap_row(f64::from(self.o.row) + p.n) };
        push_num(self.out, r + 1.0);
    }

    fn col_text(&mut self, p: Part) {
        let p = self.part(p);
        if self.o.r1c1 {
            if p.abs {
                self.out.push(u16::from(b'C'));
                push_num(self.out, p.n + 1.0);
            } else if p.n != 0.0 && !p.n.is_nan() {
                push_ascii(self.out, "C[");
                push_num(self.out, p.n);
                self.out.push(0x5d);
            } else {
                self.out.push(u16::from(b'C'));
            }
            return;
        }
        if p.abs {
            self.out.push(0x24);
        }
        let c = if p.abs { p.n } else { wrap_col(f64::from(self.o.col) + p.n) };
        push_letters(self.out, c);
    }

    fn cell_text(&mut self, row: Part, col: Part) {
        if self.o.r1c1 {
            self.row_text(row);
            self.col_text(col);
        } else {
            self.col_text(col);
            self.row_text(row);
        }
    }

    fn same_ref(&self, a: Part, b: Part) -> bool {
        let (a, b) = (self.part(a), self.part(b));
        a.n == b.n && a.abs == b.abs
    }

    /// `print(n)` without recursion: a stack of tasks, so any depth prints (A2-2).
    fn run(&mut self, root: NodeId) {
        let mut tasks: Vec<Task> = vec![Task::Node(root)];
        let mut marks: Vec<usize> = Vec::new();
        while let Some(t) = tasks.pop() {
            match t {
                Task::Ch(c) => self.out.push(c),
                Task::Str(x) => push_ascii(self.out, x),
                Task::FnName(name, prefix, with_prefix) => {
                    if with_prefix {
                        self.func_prefix(name, prefix);
                    }
                    push_js(self.out, self.ast.fn_name(name));
                }
                Task::Mark => marks.push(self.out.len()),
                Task::TroFix(dots) => {
                    // t.replace(/:(?=[^:]*$)/, dots): the last ':'
                    let t0 = marks.pop().unwrap_or(0);
                    if let Some(k) = self.out[t0..].iter().rposition(|&u| u == 0x3a) {
                        let at = t0 + k;
                        self.out.splice(at..=at, dots.bytes().map(u16::from));
                    }
                }
                Task::Node(n) => self.expand(n, &mut tasks),
            }
        }
    }

    /// Pushes the tasks of a node with children (in reverse output order); prints a leaf directly.
    fn expand(&mut self, n: NodeId, tasks: &mut Vec<Task>) {
        let ast = self.ast;
        let list = |tasks: &mut Vec<Task>, ids: &[NodeId]| {
            for (i, &a) in ids.iter().enumerate().rev() {
                tasks.push(Task::Node(a));
                if i > 0 {
                    tasks.push(Task::Ch(0x2c));
                }
            }
        };
        match *ast.node(n) {
            Node::Fn { name, prefix, args, .. } => {
                let args = ast.kids(args);
                let nm = ast.fn_name(name);
                if self.o.ui && args.len() == 1 {
                    if nm.eq_str("SINGLE") {
                        tasks.push(Task::Node(args[0]));
                        tasks.push(Task::Ch(0x40));
                        return;
                    }
                    if nm.eq_str("ANCHORARRAY") {
                        tasks.push(Task::Ch(0x23));
                        tasks.push(Task::Node(args[0]));
                        return;
                    }
                    if let Some(dots) = tro(nm) {
                        let a = args[0];
                        if matches!(ast.node(a), Node::Area { .. } | Node::Bin { op: BinOp::Range, .. }) {
                            tasks.push(Task::TroFix(dots));
                            tasks.push(Task::Node(a));
                            tasks.push(Task::Mark);
                        } else {
                            tasks.push(Task::Ch(0x29));
                            tasks.push(Task::Node(a));
                            tasks.push(Task::Ch(0x28));
                            tasks.push(Task::FnName(name, 0, false));
                        }
                        return;
                    }
                }
                tasks.push(Task::Ch(0x29));
                list(tasks, args);
                tasks.push(Task::Ch(0x28));
                tasks.push(Task::FnName(name, prefix, !self.o.ui));
            }
            Node::Call { f, args } => {
                tasks.push(Task::Ch(0x29));
                list(tasks, ast.kids(args));
                tasks.push(Task::Ch(0x28));
                tasks.push(Task::Node(f));
            }
            Node::Unary { op, e } => {
                if op == UnaryOp::Percent {
                    tasks.push(Task::Ch(0x25));
                    tasks.push(Task::Node(e));
                } else {
                    tasks.push(Task::Node(e));
                    tasks.push(Task::Str(op.text()));
                }
            }
            Node::Bin { op, a, b } => {
                tasks.push(Task::Node(b));
                tasks.push(Task::Str(op.text()));
                tasks.push(Task::Node(a));
            }
            Node::Paren(e) => {
                tasks.push(Task::Ch(0x29));
                tasks.push(Task::Node(e));
                tasks.push(Task::Ch(0x28));
            }
            _ => self.leaf(n),
        }
    }

    /// The prefix of the file form.
    fn func_prefix(&mut self, name: FnName, prefix: u32) {
        let ast = self.ast;
        let nm = ast.fn_name(name);
        if prefix != 0 {
            push_js(self.out, ast.str(StrIx(prefix - 1)));
        } else {
            let p = file_prefix(name, nm, tro(nm).is_some() || nm.eq_str("ANCHORARRAY"));
            push_ascii(self.out, p);
        }
    }

    #[inline(never)]
    fn leaf(&mut self, n: NodeId) {
        let ast = self.ast;
        match *ast.node(n) {
            Node::Num { v, raw } => {
                if raw != Ast::NO_RAW && !ast.str(StrIx(raw)).is_empty() {
                    push_js(self.out, ast.str(StrIx(raw)));
                } else {
                    push_ascii(self.out, &number_literal(v));
                }
            }
            Node::Str(s) => quoted(self.out, ast.str(s)),
            Node::Bool(b) => push_ascii(self.out, if b { "TRUE" } else { "FALSE" }),
            Node::Err(e) => push_ascii(self.out, crate::cell::error_text(e)),
            Node::Arr { start, rows, cols } => {
                self.out.push(0x7b);
                for r in 0..rows {
                    if r > 0 {
                        self.out.push(0x3b);
                    }
                    let row = &ast.array_data(start)[(r * cols) as usize..((r + 1) * cols) as usize];
                    for (c, v) in row.iter().enumerate() {
                        if c > 0 {
                            self.out.push(0x2c);
                        }
                        scalar_literal(ast, self.out, v);
                    }
                }
                self.out.push(0x7d);
            }
            Node::Cell { sheet, row, col } => {
                sheet_prefix(ast, self.out, ast.spec(sheet));
                self.cell_text(row, col);
            }
            Node::Area { sheet, r1, c1, r2, c2 } => {
                sheet_prefix(ast, self.out, ast.spec(sheet));
                if !r1.is_some() || !r2.is_some() {
                    // whole columns
                    if self.o.r1c1 && self.same_ref(c1, c2) {
                        self.col_text(c1);
                    } else {
                        self.col_text(c1);
                        self.out.push(0x3a);
                        self.col_text(c2);
                    }
                    return;
                }
                if !c1.is_some() || !c2.is_some() {
                    if self.o.r1c1 && self.same_ref(r1, r2) {
                        self.row_text(r1);
                    } else {
                        self.row_text(r1);
                        self.out.push(0x3a);
                        self.row_text(r2);
                    }
                    return;
                }
                self.cell_text(r1, c1);
                self.out.push(0x3a);
                self.cell_text(r2, c2);
            }
            Node::RefErr { sheet } => {
                sheet_prefix(ast, self.out, ast.spec(sheet));
                push_ascii(self.out, "#REF!");
            }
            Node::Name { name, sheet, flags, .. } => {
                let nm = ast.str(name);
                if flags & name_flags::ETA != 0 {
                    if !self.o.ui {
                        push_ascii(self.out, "_xleta.");
                    }
                    push_js(self.out, nm);
                } else if flags & name_flags::OPTIONAL != 0 {
                    if self.o.ui {
                        self.out.push(0x5b);
                        push_js(self.out, nm);
                        self.out.push(0x5d);
                    } else {
                        push_ascii(self.out, "_xlop.");
                        push_js(self.out, nm);
                    }
                } else {
                    sheet_prefix(ast, self.out, ast.spec(sheet));
                    if flags & name_flags::PARAM != 0 && !self.o.ui {
                        push_ascii(self.out, "_xlpm.");
                    }
                    push_js(self.out, nm);
                }
            }
            Node::Table { table, items, n_items, col1, col2 } => self.table(table, ast.items(items, n_items), col1, col2),
            _ => {}
        }
    }

    #[inline(never)]
    fn table(&mut self, table: StrIx, items: &[TableItem], col1: u32, col2: u32) {
        let ast = self.ast;
        let c1 = (col1 != 0).then(|| ast.str(StrIx(col1 - 1)));
        let c2 = (col2 != 0).then(|| ast.str(StrIx(col2 - 1)));
        let cols0 = |out: &mut Vec<u16>| {
            if let Some(c1) = c1 {
                out.push(0x5b);
                push_escaped_col(out, c1);
                out.push(0x5d);
                if let Some(c2) = c2 {
                    push_ascii(out, ":[");
                    push_escaped_col(out, c2);
                    out.push(0x5d);
                }
            }
        };
        push_js(self.out, ast.str(table));
        let this_row = items.len() == 1 && items[0] == TableItem::ThisRow;
        if self.o.ui && this_row {
            match c1 {
                None => push_ascii(self.out, "[@]"),
                Some(c) if c2.is_none() && !col_special(c) => {
                    push_ascii(self.out, "[@");
                    push_escaped_col(self.out, c);
                    self.out.push(0x5d);
                }
                Some(_) => {
                    push_ascii(self.out, "[@");
                    cols0(self.out);
                    self.out.push(0x5d);
                }
            }
            return;
        }
        if items.is_empty() {
            match c1 {
                None => push_ascii(self.out, "[]"),
                Some(c) if c2.is_none() && !col_special(c) => {
                    self.out.push(0x5b);
                    push_escaped_col(self.out, c);
                    self.out.push(0x5d);
                }
                Some(_) => {
                    self.out.push(0x5b);
                    cols0(self.out);
                    self.out.push(0x5d);
                }
            }
            return;
        }
        if items.len() == 1 && c1.is_none() {
            self.out.push(0x5b);
            push_ascii(self.out, items[0].text());
            self.out.push(0x5d);
            return;
        }
        self.out.push(0x5b);
        for (i, it) in items.iter().enumerate() {
            if i > 0 {
                self.out.push(0x2c);
            }
            self.out.push(0x5b);
            push_ascii(self.out, it.text());
            self.out.push(0x5d);
        }
        if c1.is_some() {
            self.out.push(0x2c);
            cols0(self.out);
        }
        self.out.push(0x5d);
    }
}

/// `catalogEntry(name)?.prefix ?? lookupFn(name)?.prefix ?? (TRO[name] || name === 'ANCHORARRAY' ? '_xlfn.' : '')`.
fn file_prefix(name: FnName, text: JsStr<'_>, tro_or_anchor: bool) -> &'static str {
    let ix = match name {
        FnName::Known(ix) => Some(ix),
        FnName::Other(_) => catalog_entry(text).or_else(|| lookup_fn(text)),
    };
    if let Some(ix) = ix {
        let e = ix.entry();
        if let Some(p) = e.catalog.text() {
            return p;
        }
        if let Some(p) = e.def.and_then(|d| d.prefix) {
            return p;
        }
        debug_assert!(e.catalog == CatPrefix::None);
    }
    if tro_or_anchor { "_xlfn." } else { "" }
}
