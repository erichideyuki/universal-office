//! Formula syntax trees (`ast.ts`) in one arena: nodes are 24-byte values addressed by [`NodeId`], argument
//! lists are spans of a shared child vector, texts are interned once in the arena's string table (names, sheet
//! names, text literals), and a reference part is a packed `u32` (the rare R1C1 offset that does not fit, or a
//! `-0`, goes to a side table). One arena serves every formula of an engine (shared formulas share their
//! anchor's tree, defined names live beside the cells' formulas), so a value can point at a literal of the tree
//! ([`crate::value::StrRef::Lit`]) without copying it.
//!
//! Relative references keep the OFFSET from the formula's cell (the TS canonical form): the same tree serves
//! every cell of a shared formula; the position appears only when evaluating and when printing A1.
//!
//! **Depth.** A parsed tree nests at most [`MAX_AST_DEPTH`] function/call levels; operators and parentheses may
//! nest without limit. A walker must therefore not recurse through operators: use an explicit stack (the
//! evaluator's operator machine, the printer, [`Ast::walk`]). Record, decision A2-2.

use std::collections::HashMap;
use std::hash::BuildHasher;
use std::sync::Arc;

use crate::fnreg::{FN_TABLE, FnIx};
use crate::strings::{JsStr, JsString};
use crate::value::{ArrayValue, ErrorCode, Scalar, StrRef};

/// Deepest NESTING the parser builds: function and call arguments inside function and call arguments (the levels
/// of recursion evaluating the tree takes). Operators and parentheses cost no level: the evaluator runs them on an
/// explicit work stack and the printer on a task stack, so `1+1+…+1`, `1^(1^(…))` or `((…))` have no depth limit.
/// Deeper nesting is a syntax error (decision A2-2: the TS has no limit and V8's stack overflows at about 2,300
/// nested functions when parsing; Excel caps a formula at 64 nested functions and 8,192 characters).
pub const MAX_AST_DEPTH: u32 = 512;

/// A node of an [`Ast`].
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, PartialOrd, Ord)]
pub struct NodeId(pub u32);

/// A string of an [`Ast`].
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct StrIx(pub u32);

/// A slice of [`Ast::kids`] (arguments of a function or call).
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct Span {
    /// First child.
    pub start: u32,
    /// Number of children.
    pub len: u32,
}

/// Binary operators (`ast.ts` `BinOp`): `:` range, ` ` intersection, `,` union.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub enum BinOp {
    /// `+`
    Add,
    /// `-`
    Sub,
    /// `*`
    Mul,
    /// `/`
    Div,
    /// `^`
    Pow,
    /// `&`
    Concat,
    /// `=`
    Eq,
    /// `<>`
    Ne,
    /// `<`
    Lt,
    /// `<=`
    Le,
    /// `>`
    Gt,
    /// `>=`
    Ge,
    /// `:`
    Range,
    /// ` `
    Isect,
    /// `,`
    Union,
}

impl BinOp {
    /// The operator's text.
    #[must_use]
    pub const fn text(self) -> &'static str {
        match self {
            Self::Add => "+",
            Self::Sub => "-",
            Self::Mul => "*",
            Self::Div => "/",
            Self::Pow => "^",
            Self::Concat => "&",
            Self::Eq => "=",
            Self::Ne => "<>",
            Self::Lt => "<",
            Self::Le => "<=",
            Self::Gt => ">",
            Self::Ge => ">=",
            Self::Range => ":",
            Self::Isect => " ",
            Self::Union => ",",
        }
    }
}

/// Unary operators: `-x`, `+x`, `x%`.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub enum UnaryOp {
    /// `-`
    Neg,
    /// `+`
    Plus,
    /// `%` (postfix)
    Percent,
}

impl UnaryOp {
    /// The operator's text.
    #[must_use]
    pub const fn text(self) -> &'static str {
        match self {
            Self::Neg => "-",
            Self::Plus => "+",
            Self::Percent => "%",
        }
    }
}

/// Special items of a structured reference (`TableItem`).
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub enum TableItem {
    /// `#All`
    All,
    /// `#Data`
    Data,
    /// `#Headers`
    Headers,
    /// `#Totals`
    Totals,
    /// `#This Row`
    ThisRow,
}

impl TableItem {
    /// The canonical text.
    #[must_use]
    pub const fn text(self) -> &'static str {
        match self {
            Self::All => "#All",
            Self::Data => "#Data",
            Self::Headers => "#Headers",
            Self::Totals => "#Totals",
            Self::ThisRow => "#This Row",
        }
    }
}

/// A row or column of a reference (`RefPart`): absolute (index) or relative (offset from the formula's cell).
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct RefPart {
    /// Index (absolute) or offset (relative); any double (an R1C1 offset `R[…]` is not range checked).
    pub n: f64,
    /// `$` / R1C1 without brackets.
    pub abs: bool,
}

/// A packed [`RefPart`]: `0` = absent; bit 0 = absolute; bit 1 = in the side table (`n` not an `i30`, or
/// `-0`); bits 2.. = the signed value (or the side-table index).
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash, Default)]
pub struct Part(u32);

const PART_SOME: u32 = 1 << 31;

impl Part {
    /// No part (a whole-row/column area side).
    pub const NONE: Self = Self(0);
    /// Present.
    #[must_use]
    pub const fn is_some(self) -> bool {
        self.0 != 0
    }
}

/// Function name of a [`Node::Fn`]: an entry of [`FN_TABLE`] or an arena string (an unknown name, a UDF).
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub enum FnName {
    /// A catalogue/registered function.
    Known(FnIx),
    /// Any other name (upper case).
    Other(StrIx),
}

/// Flags of a [`Node::Name`].
pub mod name_flags {
    /// `_xlpm.` (a LET/LAMBDA parameter).
    pub const PARAM: u8 = 1;
    /// `_xlop.` (an optional LAMBDA parameter, `[b]` on screen).
    pub const OPTIONAL: u8 = 2;
    /// `_xleta.` (a function used as a value).
    pub const ETA: u8 = 4;
}

/// A node (`ast.ts` `Node`). 24 bytes.
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Node {
    /// A number literal; `raw` = the literal text (`NO_RAW` = absent, the empty string = truncated).
    Num {
        /// Value.
        v: f64,
        /// Raw text (`StrIx`) or [`Ast::NO_RAW`].
        raw: u32,
    },
    /// A text literal.
    Str(StrIx),
    /// `TRUE`/`FALSE`.
    Bool(bool),
    /// An error literal.
    Err(ErrorCode),
    /// An array constant `{1,2;3,4}`: `rows × cols` scalars, [`Ast::array_data`]`(start)`.
    Arr {
        /// Index of the data.
        start: u32,
        /// Rows.
        rows: u32,
        /// Columns.
        cols: u32,
    },
    /// A cell.
    Cell {
        /// Sheet spec (`0` = none, else index + 1).
        sheet: u32,
        /// Row.
        row: Part,
        /// Column.
        col: Part,
    },
    /// An area; whole columns have no rows, whole rows no columns.
    Area {
        /// Sheet spec (`0` = none).
        sheet: u32,
        /// First row.
        r1: Part,
        /// First column.
        c1: Part,
        /// Last row.
        r2: Part,
        /// Last column.
        c2: Part,
    },
    /// `#REF!` in place of a reference (`Plan1!#REF!`).
    RefErr {
        /// Sheet spec (`0` = none).
        sheet: u32,
    },
    /// A defined name, a LET/LAMBDA parameter or a function used as a value.
    Name {
        /// The name as written (without the `_xlpm.`/`_xlop.` prefix; `_xleta.` names upper-cased).
        name: StrIx,
        /// The upper-cased name (environment lookups).
        sym: StrIx,
        /// Sheet spec (`0` = none).
        sheet: u32,
        /// [`name_flags`].
        flags: u8,
    },
    /// A structured reference.
    Table {
        /// Table name (empty = the cell's table).
        table: StrIx,
        /// First item at [`Ast::items`].
        items: u32,
        /// Number of items (order and repetitions kept).
        n_items: u32,
        /// First column (`0` = none, else `StrIx + 1`).
        col1: u32,
        /// Last column (`0` = none).
        col2: u32,
    },
    /// A function call by name (canonical, upper case, no prefix).
    Fn {
        /// The name.
        name: FnName,
        /// The file prefix as written (`0` = none, else `StrIx + 1`).
        prefix: u32,
        /// Arguments.
        args: Span,
        /// The upper-cased name as an arena string (environment lookups of `x(1)`).
        sym: StrIx,
    },
    /// An expression call `LAMBDA(x,x+1)(2)`.
    Call {
        /// The callee.
        f: NodeId,
        /// Arguments.
        args: Span,
    },
    /// An omitted argument.
    Missing,
    /// A unary operator.
    Unary {
        /// Operator.
        op: UnaryOp,
        /// Operand.
        e: NodeId,
    },
    /// A binary operator.
    Bin {
        /// Operator.
        op: BinOp,
        /// Left.
        a: NodeId,
        /// Right.
        b: NodeId,
    },
    /// Parentheses.
    Paren(NodeId),
}

/// A sheet prefix (`SheetSpec`).
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct SheetSpec {
    /// Sheet (empty: `!A1`, `[1]!Name`).
    pub sheet: StrIx,
    /// 3D end sheet.
    pub sheet2: Option<StrIx>,
    /// External book (`[n]`; `+digits`, so any double).
    pub ext: Option<f64>,
}

/// The interned strings of an arena: Latin-1 and UTF-16 storage, deduplicated by content.
#[derive(Clone, Default)]
struct StrTable {
    /// `(start, len << 1 | wide)` per string.
    entries: Vec<(u32, u32)>,
    narrow: Vec<u8>,
    wide: Vec<u16>,
    /// content hash → newest entry; `next` chains older ones.
    index: HashMap<u64, u32>,
    next: Vec<u32>,
    hasher: std::hash::RandomState,
}

impl core::fmt::Debug for StrTable {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "StrTable({} strings)", self.entries.len())
    }
}

impl StrTable {
    fn get(&self, i: StrIx) -> JsStr<'_> {
        let (start, lw) = self.entries[i.0 as usize];
        let (s, n) = (start as usize, (lw >> 1) as usize);
        if lw & 1 == 0 { JsStr::Latin1(&self.narrow[s..s + n]) } else { JsStr::Utf16(&self.wide[s..s + n]) }
    }

    fn hash(&self, s: JsStr<'_>) -> u64 {
        use std::hash::Hasher;
        let mut h = self.hasher.build_hasher();
        h.write_usize(s.len());
        for u in s.units() {
            h.write_u16(u);
        }
        h.finish()
    }

    fn intern(&mut self, s: JsStr<'_>) -> StrIx {
        let h = self.hash(s);
        let mut e = self.index.get(&h).copied().unwrap_or(u32::MAX);
        while e != u32::MAX {
            if self.get(StrIx(e)).eq_units(s) {
                return StrIx(e);
            }
            e = self.next[e as usize];
        }
        let id = self.entries.len() as u32;
        let narrow = s.units().all(|u| u <= 0xff);
        let len = s.len() as u32;
        if narrow {
            let start = self.narrow.len() as u32;
            self.narrow.extend(s.units().map(|u| u as u8));
            self.entries.push((start, len << 1));
        } else {
            let start = self.wide.len() as u32;
            self.wide.extend(s.units());
            self.entries.push((start, (len << 1) | 1));
        }
        let prev = self.index.insert(h, id).unwrap_or(u32::MAX);
        self.next.push(prev);
        StrIx(id)
    }

    fn truncate(&mut self, n: usize) {
        while self.entries.len() > n {
            let id = self.entries.len() as u32 - 1;
            let h = self.hash(self.get(StrIx(id)));
            let prev = self.next[id as usize];
            if prev == u32::MAX {
                self.index.remove(&h);
            } else {
                self.index.insert(h, prev);
            }
            let (start, lw) = self.entries.pop().unwrap_or_default();
            if lw & 1 == 0 {
                self.narrow.truncate(start as usize);
            } else {
                self.wide.truncate(start as usize);
            }
            self.next.pop();
        }
    }

    fn heap_bytes(&self) -> usize {
        self.entries.capacity() * 8
            + self.narrow.capacity()
            + self.wide.capacity() * 2
            + self.next.capacity() * 4
            + self.index.capacity() * 16
    }
}

/// A position in an [`Ast`] to roll back to ([`Ast::mark`]).
#[derive(Clone, Copy, Debug)]
pub struct AstMark {
    nodes: usize,
    kids: usize,
    arrays: usize,
    items: usize,
    specs: usize,
    wide: usize,
    strs: usize,
}

/// The arena of formula trees.
#[derive(Clone, Debug, Default)]
pub struct Ast {
    nodes: Vec<Node>,
    /// Start of each node in the parsed text (`Positioned.pos`), `u32::MAX` = none.
    pos: Vec<u32>,
    kids: Vec<NodeId>,
    arrays: Vec<Arc<[Scalar]>>,
    items: Vec<TableItem>,
    specs: Vec<SheetSpec>,
    wide: Vec<RefPart>,
    strs: StrTable,
}

impl Ast {
    /// `raw` of a number node without the key (a node not built by the parser).
    pub const NO_RAW: u32 = u32::MAX;

    /// An empty arena; the empty string is [`StrIx`] 0.
    #[must_use]
    pub fn new() -> Self {
        let mut a = Self::default();
        a.strs.intern(JsStr::EMPTY);
        a
    }

    /// The empty string.
    pub const EMPTY: StrIx = StrIx(0);

    /// A node.
    #[must_use]
    pub fn node(&self, id: NodeId) -> &Node {
        &self.nodes[id.0 as usize]
    }

    /// Replaces a node in place (the parser merges `A1:B2` into the left cell's slot).
    pub fn set_node(&mut self, id: NodeId, n: Node) {
        self.nodes[id.0 as usize] = n;
    }

    /// The node's position in the parsed text.
    #[must_use]
    pub fn pos(&self, id: NodeId) -> Option<u32> {
        let p = self.pos[id.0 as usize];
        (p != u32::MAX).then_some(p)
    }

    /// Sets the node's position (`at`).
    pub fn set_pos(&mut self, id: NodeId, p: Option<u32>) {
        self.pos[id.0 as usize] = p.unwrap_or(u32::MAX);
    }

    /// Number of nodes.
    #[must_use]
    pub fn len(&self) -> usize {
        self.nodes.len()
    }

    /// No node.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.nodes.is_empty()
    }

    /// Appends a node.
    pub fn push(&mut self, n: Node, pos: Option<u32>) -> NodeId {
        let id = NodeId(self.nodes.len() as u32);
        self.nodes.push(n);
        self.pos.push(pos.unwrap_or(u32::MAX));
        id
    }

    /// The children of a span.
    #[must_use]
    pub fn kids(&self, s: Span) -> &[NodeId] {
        &self.kids[s.start as usize..(s.start + s.len) as usize]
    }

    /// Appends a list of children.
    pub fn push_kids(&mut self, k: &[NodeId]) -> Span {
        let start = self.kids.len() as u32;
        self.kids.extend_from_slice(k);
        Span { start, len: k.len() as u32 }
    }

    /// An interned string.
    #[must_use]
    pub fn str(&self, i: StrIx) -> JsStr<'_> {
        self.strs.get(i)
    }

    /// Interns a string.
    pub fn intern(&mut self, s: JsStr<'_>) -> StrIx {
        self.strs.intern(s)
    }

    /// Interns UTF-16 units.
    pub fn intern_units(&mut self, u: &[u16]) -> StrIx {
        if u.iter().all(|&x| x <= 0xff) {
            let b: Vec<u8> = u.iter().map(|&x| x as u8).collect();
            self.strs.intern(JsStr::Latin1(&b))
        } else {
            self.strs.intern(JsStr::Utf16(u))
        }
    }

    /// Interns the full Unicode upper case of a string (`toUpperCase()`).
    pub fn intern_upper(&mut self, s: JsStr<'_>) -> StrIx {
        match s {
            JsStr::Latin1(b) if b.iter().all(|&x| !x.is_ascii_lowercase() && x < 0x80) => self.strs.intern(s),
            _ => {
                let u = s.to_utf16();
                let up = uo_text::case::to_uppercase(&u);
                self.intern_units(&up)
            }
        }
    }

    /// A sheet spec (`None` → 0).
    #[must_use]
    pub fn spec(&self, ix: u32) -> Option<&SheetSpec> {
        if ix == 0 { None } else { Some(&self.specs[ix as usize - 1]) }
    }

    /// Appends a sheet spec; returns its index + 1.
    pub fn push_spec(&mut self, s: SheetSpec) -> u32 {
        self.specs.push(s);
        self.specs.len() as u32
    }

    /// Packs a reference part.
    pub fn pack_part(&mut self, p: RefPart) -> Part {
        let fits = p.n.fract() == 0.0 && p.n >= -(1 << 28) as f64 && p.n < (1 << 28) as f64 && !(p.n == 0.0 && p.n.is_sign_negative());
        if fits {
            let v = crate::cast::to_i32_small(p.n).cast_unsigned() & 0x1fff_ffff;
            Part(PART_SOME | (v << 2) | u32::from(p.abs))
        } else {
            self.wide.push(p);
            Part(PART_SOME | ((self.wide.len() as u32 - 1) << 2) | 2)
        }
    }

    /// Unpacks a reference part.
    #[must_use]
    pub fn part(&self, p: Part) -> Option<RefPart> {
        if p.0 == 0 {
            return None;
        }
        let x = p.0 & !PART_SOME;
        if x & 2 != 0 {
            return Some(self.wide[(x >> 2) as usize]);
        }
        // sign-extend the 29-bit value
        let v = (((x >> 2) << 3) as i32) >> 3;
        Some(RefPart { n: f64::from(v), abs: x & 1 != 0 })
    }

    /// The scalars of an array constant (row by row).
    #[must_use]
    pub fn array_data(&self, ix: u32) -> &[Scalar] {
        &self.arrays[ix as usize]
    }

    /// The value of an array constant (shares the data: evaluating it allocates nothing).
    #[must_use]
    pub fn array(&self, ix: u32, rows: u32, cols: u32) -> ArrayValue {
        ArrayValue { rows, cols, data: Arc::clone(&self.arrays[ix as usize]) }
    }

    /// Appends the data of an array constant.
    pub fn push_array(&mut self, v: &[Scalar]) -> u32 {
        self.arrays.push(v.into());
        self.arrays.len() as u32 - 1
    }

    /// The items of a table node.
    #[must_use]
    pub fn items(&self, start: u32, n: u32) -> &[TableItem] {
        &self.items[start as usize..(start + n) as usize]
    }

    /// Appends table items.
    pub fn push_items(&mut self, v: &[TableItem]) -> u32 {
        let s = self.items.len() as u32;
        self.items.extend_from_slice(v);
        s
    }

    /// The canonical name of a function node.
    #[must_use]
    pub fn fn_name(&self, n: FnName) -> JsStr<'_> {
        match n {
            FnName::Known(ix) => JsStr::Latin1(FN_TABLE[ix.0 as usize].name.as_bytes()),
            FnName::Other(s) => self.str(s),
        }
    }

    /// The text of a string reference that points into this arena or a temporary/pooled string resolved by the
    /// caller (`None` for those).
    #[must_use]
    pub fn lit_str(&self, r: StrRef) -> Option<JsStr<'_>> {
        match r {
            StrRef::Lit(i) => Some(self.str(StrIx(i))),
            _ => None,
        }
    }

    /// The current end of every table (to roll a failed parse back).
    #[must_use]
    pub fn mark(&self) -> AstMark {
        AstMark {
            nodes: self.nodes.len(),
            kids: self.kids.len(),
            arrays: self.arrays.len(),
            items: self.items.len(),
            specs: self.specs.len(),
            wide: self.wide.len(),
            strs: self.strs.entries.len(),
        }
    }

    /// Drops everything added after `m`.
    pub fn rollback(&mut self, m: AstMark) {
        self.nodes.truncate(m.nodes);
        self.pos.truncate(m.nodes);
        self.kids.truncate(m.kids);
        self.arrays.truncate(m.arrays);
        self.items.truncate(m.items);
        self.specs.truncate(m.specs);
        self.wide.truncate(m.wide);
        self.strs.truncate(m.strs.max(1));
    }

    /// Heap bytes held (M3).
    #[must_use]
    pub fn heap_bytes(&self) -> usize {
        self.nodes.capacity() * core::mem::size_of::<Node>()
            + self.pos.capacity() * 4
            + self.kids.capacity() * 4
            + self.arrays.iter().map(|a| a.len() * core::mem::size_of::<Scalar>() + 16).sum::<usize>()
            + self.arrays.capacity() * 16
            + self.items.capacity()
            + self.specs.capacity() * core::mem::size_of::<SheetSpec>()
            + self.wide.capacity() * core::mem::size_of::<RefPart>()
            + self.strs.heap_bytes()
    }

    /// Releases spare capacity.
    pub fn shrink_to_fit(&mut self) {
        self.nodes.shrink_to_fit();
        self.pos.shrink_to_fit();
        self.kids.shrink_to_fit();
        self.arrays.shrink_to_fit();
        self.items.shrink_to_fit();
        self.specs.shrink_to_fit();
        self.wide.shrink_to_fit();
        self.strs.entries.shrink_to_fit();
        self.strs.narrow.shrink_to_fit();
        self.strs.wide.shrink_to_fit();
        self.strs.next.shrink_to_fit();
        self.strs.index.shrink_to_fit();
    }

    /// `walk(n, f)`: every node in pre-order (iterative: the tree is at most [`MAX_AST_DEPTH`] deep, but the
    /// walk needs no stack of its own).
    pub fn walk(&self, root: NodeId, mut f: impl FnMut(NodeId, &Node)) {
        let mut stack = vec![root];
        while let Some(id) = stack.pop() {
            let n = self.node(id);
            f(id, n);
            match *n {
                Node::Fn { args, .. } => stack.extend(self.kids(args).iter().rev()),
                Node::Call { f: callee, args } => {
                    stack.extend(self.kids(args).iter().rev());
                    stack.push(callee);
                }
                Node::Unary { e, .. } | Node::Paren(e) => stack.push(e),
                Node::Bin { a, b, .. } => {
                    stack.push(b);
                    stack.push(a);
                }
                _ => {}
            }
        }
    }

    /// The canonical token stream of a tree (pre-order; every field, positions included, parts resolved, strings by
    /// their interned index, array constants by content): two trees with the same stream evaluate, print and
    /// report positions identically. The engine hash-conses formulas by it (relative references are offsets, so
    /// `A1+1` in B1 and `A2+1` in B2 are one tree).
    pub fn tree_key(&self, root: NodeId, out: &mut Vec<u64>) {
        let part = |out: &mut Vec<u64>, p: Part| match self.part(p) {
            None => out.push(u64::MAX),
            Some(r) => {
                out.push(r.n.to_bits());
                out.push(u64::from(r.abs));
            }
        };
        let spec = |out: &mut Vec<u64>, s: u32| match self.spec(s) {
            None => out.push(u64::MAX),
            Some(sp) => {
                out.push(u64::from(sp.sheet.0));
                out.push(sp.sheet2.map_or(u64::MAX, |x| u64::from(x.0)));
                out.push(sp.ext.map_or(u64::MAX - 1, f64::to_bits));
            }
        };
        let mut stack = vec![root];
        while let Some(id) = stack.pop() {
            out.push(u64::from(self.pos.get(id.0 as usize).copied().unwrap_or(u32::MAX)));
            match *self.node(id) {
                Node::Num { v, raw } => out.extend([1, v.to_bits(), u64::from(raw)]),
                Node::Str(s) => out.extend([2, u64::from(s.0)]),
                Node::Bool(b) => out.extend([3, u64::from(b)]),
                Node::Err(e) => out.extend([4, e as u64]),
                Node::Arr { start, rows, cols } => {
                    out.extend([5, u64::from(rows), u64::from(cols)]);
                    for s in self.array_data(start) {
                        match *s {
                            Scalar::Empty => out.push(0),
                            Scalar::Num(x) => out.extend([1, x.to_bits()]),
                            Scalar::Bool(b) => out.extend([2, u64::from(b)]),
                            Scalar::Err(e) => out.extend([3, e as u64]),
                            Scalar::Str(StrRef::Lit(i)) => out.extend([4, u64::from(i)]),
                            Scalar::Str(StrRef::Pooled(i)) => out.extend([5, u64::from(i.raw())]),
                            Scalar::Str(StrRef::Temp(i)) => out.extend([6, u64::from(i)]),
                        }
                    }
                }
                Node::Cell { sheet, row, col } => {
                    out.push(6);
                    spec(out, sheet);
                    part(out, row);
                    part(out, col);
                }
                Node::Area { sheet, r1, c1, r2, c2 } => {
                    out.push(7);
                    spec(out, sheet);
                    for p in [r1, c1, r2, c2] {
                        part(out, p);
                    }
                }
                Node::RefErr { sheet } => {
                    out.push(8);
                    spec(out, sheet);
                }
                Node::Name { name, sym, sheet, flags } => {
                    out.extend([9, u64::from(name.0), u64::from(sym.0), u64::from(flags)]);
                    spec(out, sheet);
                }
                Node::Table { table, items, n_items, col1, col2 } => {
                    out.extend([10, u64::from(table.0), u64::from(col1), u64::from(col2), u64::from(n_items)]);
                    out.extend(self.items(items, n_items).iter().map(|&i| i as u64));
                }
                Node::Fn { name, prefix, args, sym } => {
                    let n = match name {
                        FnName::Known(ix) => u64::from(ix.0),
                        FnName::Other(s) => (1 << 32) | u64::from(s.0),
                    };
                    out.extend([11, n, u64::from(prefix), u64::from(sym.0), u64::from(args.len)]);
                    stack.extend(self.kids(args).iter().rev());
                }
                Node::Call { f, args } => {
                    out.extend([12, u64::from(args.len)]);
                    stack.extend(self.kids(args).iter().rev());
                    stack.push(f);
                }
                Node::Missing => out.push(13),
                Node::Unary { op, e } => {
                    out.extend([14, op as u64]);
                    stack.push(e);
                }
                Node::Bin { op, a, b } => {
                    out.extend([15, op as u64]);
                    stack.push(b);
                    stack.push(a);
                }
                Node::Paren(e) => {
                    out.push(16);
                    stack.push(e);
                }
            }
        }
    }

    /// Owned copy of a string.
    #[must_use]
    pub fn string(&self, i: StrIx) -> JsString {
        self.str(i).to_owned_js()
    }
}

#[cfg(feature = "facet")]
pub mod canon {
    //! The canonical JSON of a tree, as the `calc-tests` recorder writes the TS `Node` (`recorder-setup.mjs`
    //! `canon`: keys sorted, `-0`/NaN/±Infinity as `{"$num":…}`, errors in array constants as `{"$err":…}`).
    use super::{Ast, FnName, Node, NodeId, Part, SheetSpec, name_flags};
    use crate::value::{Scalar, StrRef};

    fn num(out: &mut String, x: f64) {
        if x == 0.0 && x.is_sign_negative() {
            out.push_str(r#"{"$num":"-0"}"#);
        } else if !x.is_finite() {
            out.push_str(&format!(r#"{{"$num":"{}"}}"#, uo_jsnum::format::to_js_string(x)));
        } else {
            uo_jsnum::json::push_number(out, x);
        }
    }

    fn js(out: &mut String, s: crate::strings::JsStr<'_>) {
        uo_jsnum::json::push_string_utf16(out, &s.to_utf16());
    }

    fn part(out: &mut String, ast: &Ast, p: Part) {
        let r = ast.part(p).unwrap_or(super::RefPart { n: f64::NAN, abs: false });
        out.push_str(if r.abs { r#"{"abs":true,"n":"# } else { r#"{"abs":false,"n":"# });
        num(out, r.n);
        out.push('}');
    }

    fn spec(out: &mut String, ast: &Ast, s: &SheetSpec) {
        out.push('{');
        if let Some(e) = s.ext {
            out.push_str(r#""ext":"#);
            num(out, e);
            out.push(',');
        }
        out.push_str(r#""sheet":"#);
        js(out, ast.str(s.sheet));
        if let Some(s2) = s.sheet2 {
            out.push_str(r#","sheet2":"#);
            js(out, ast.str(s2));
        }
        out.push('}');
    }

    /// A scalar of an array constant (`Scalar` canon).
    pub fn scalar(out: &mut String, ast: &Ast, v: &Scalar, other: &dyn Fn(StrRef) -> String) {
        match *v {
            Scalar::Empty => out.push_str("null"),
            Scalar::Num(x) => num(out, x),
            Scalar::Bool(b) => out.push_str(if b { "true" } else { "false" }),
            Scalar::Err(e) => {
                out.push_str(r#"{"$err":""#);
                out.push_str(crate::cell::error_text(e));
                out.push_str(r#""}"#);
            }
            Scalar::Str(r) => match ast.lit_str(r) {
                Some(s) => js(out, s),
                None => out.push_str(&other(r)),
            },
        }
    }

    /// `JSON.stringify(canon(node))`.
    #[must_use]
    pub fn node(ast: &Ast, id: NodeId) -> String {
        let mut s = String::new();
        write(&mut s, ast, id);
        s
    }

    fn key(out: &mut String, first: &mut bool, k: &str) {
        if !*first {
            out.push(',');
        }
        *first = false;
        out.push('"');
        out.push_str(k);
        out.push_str("\":");
    }

    fn list(out: &mut String, ast: &Ast, ids: &[NodeId]) {
        out.push('[');
        for (i, &k) in ids.iter().enumerate() {
            if i > 0 {
                out.push(',');
            }
            write(out, ast, k);
        }
        out.push(']');
    }

    fn write(out: &mut String, ast: &Ast, id: NodeId) {
        let pos = ast.pos(id);
        let mut f = true;
        out.push('{');
        let pos_key = |out: &mut String, f: &mut bool| {
            if let Some(p) = pos {
                key(out, f, "pos");
                out.push_str(&p.to_string());
            }
        };
        let sheet_key = |out: &mut String, f: &mut bool, sh: u32| {
            if let Some(s) = ast.spec(sh) {
                key(out, f, "sheet");
                spec(out, ast, s);
            }
        };
        match *ast.node(id) {
            Node::Num { v, raw } => {
                pos_key(out, &mut f);
                if raw != Ast::NO_RAW {
                    key(out, &mut f, "raw");
                    js(out, ast.str(super::StrIx(raw)));
                }
                key(out, &mut f, "t");
                out.push_str(r#""num""#);
                key(out, &mut f, "v");
                num(out, v);
            }
            Node::Str(s) => {
                pos_key(out, &mut f);
                key(out, &mut f, "t");
                out.push_str(r#""str""#);
                key(out, &mut f, "v");
                js(out, ast.str(s));
            }
            Node::Bool(b) => {
                pos_key(out, &mut f);
                key(out, &mut f, "t");
                out.push_str(r#""bool""#);
                key(out, &mut f, "v");
                out.push_str(if b { "true" } else { "false" });
            }
            Node::Err(e) => {
                pos_key(out, &mut f);
                key(out, &mut f, "t");
                out.push_str(r#""err""#);
                key(out, &mut f, "v");
                out.push('"');
                out.push_str(crate::cell::error_text(e));
                out.push('"');
            }
            Node::Arr { start, rows, cols } => {
                pos_key(out, &mut f);
                key(out, &mut f, "rows");
                out.push('[');
                for r in 0..rows {
                    if r > 0 {
                        out.push(',');
                    }
                    out.push('[');
                    let row = &ast.array_data(start)[(r * cols) as usize..((r + 1) * cols) as usize];
                    for (c, v) in row.iter().enumerate() {
                        if c > 0 {
                            out.push(',');
                        }
                        scalar(out, ast, v, &|_| "?".to_owned());
                    }
                    out.push(']');
                }
                out.push(']');
                key(out, &mut f, "t");
                out.push_str(r#""arr""#);
            }
            Node::Cell { sheet, row, col } => {
                key(out, &mut f, "col");
                part(out, ast, col);
                pos_key(out, &mut f);
                key(out, &mut f, "row");
                part(out, ast, row);
                sheet_key(out, &mut f, sheet);
                key(out, &mut f, "t");
                out.push_str(r#""cell""#);
            }
            Node::Area { sheet, r1, c1, r2, c2 } => {
                for (k, p) in [("c1", c1), ("c2", c2)] {
                    if p.is_some() {
                        key(out, &mut f, k);
                        part(out, ast, p);
                    }
                }
                pos_key(out, &mut f);
                for (k, p) in [("r1", r1), ("r2", r2)] {
                    if p.is_some() {
                        key(out, &mut f, k);
                        part(out, ast, p);
                    }
                }
                sheet_key(out, &mut f, sheet);
                key(out, &mut f, "t");
                out.push_str(r#""area""#);
            }
            Node::RefErr { sheet } => {
                pos_key(out, &mut f);
                sheet_key(out, &mut f, sheet);
                key(out, &mut f, "t");
                out.push_str(r#""referr""#);
            }
            Node::Name { name, sheet, flags, .. } => {
                if flags & name_flags::ETA != 0 {
                    key(out, &mut f, "eta");
                    out.push_str("true");
                }
                key(out, &mut f, "name");
                js(out, ast.str(name));
                if flags & name_flags::OPTIONAL != 0 {
                    key(out, &mut f, "optional");
                    out.push_str("true");
                }
                if flags & name_flags::PARAM != 0 {
                    key(out, &mut f, "param");
                    out.push_str("true");
                }
                pos_key(out, &mut f);
                sheet_key(out, &mut f, sheet);
                key(out, &mut f, "t");
                out.push_str(r#""name""#);
            }
            Node::Table { table, items, n_items, col1, col2 } => {
                if col1 != 0 {
                    key(out, &mut f, "col1");
                    js(out, ast.str(super::StrIx(col1 - 1)));
                }
                if col2 != 0 {
                    key(out, &mut f, "col2");
                    js(out, ast.str(super::StrIx(col2 - 1)));
                }
                key(out, &mut f, "items");
                out.push('[');
                for (i, it) in ast.items(items, n_items).iter().enumerate() {
                    if i > 0 {
                        out.push(',');
                    }
                    out.push('"');
                    out.push_str(it.text());
                    out.push('"');
                }
                out.push(']');
                pos_key(out, &mut f);
                key(out, &mut f, "t");
                out.push_str(r#""table""#);
                key(out, &mut f, "table");
                js(out, ast.str(table));
            }
            Node::Fn { name, prefix, args, .. } => {
                key(out, &mut f, "args");
                list(out, ast, ast.kids(args));
                key(out, &mut f, "name");
                match name {
                    FnName::Known(_) | FnName::Other(_) => js(out, ast.fn_name(name)),
                }
                pos_key(out, &mut f);
                if prefix != 0 {
                    key(out, &mut f, "prefix");
                    js(out, ast.str(super::StrIx(prefix - 1)));
                }
                key(out, &mut f, "t");
                out.push_str(r#""fn""#);
            }
            Node::Call { f: callee, args } => {
                key(out, &mut f, "args");
                list(out, ast, ast.kids(args));
                key(out, &mut f, "fn");
                write(out, ast, callee);
                pos_key(out, &mut f);
                key(out, &mut f, "t");
                out.push_str(r#""call""#);
            }
            Node::Missing => {
                pos_key(out, &mut f);
                key(out, &mut f, "t");
                out.push_str(r#""missing""#);
            }
            Node::Unary { op, e } => {
                key(out, &mut f, "e");
                write(out, ast, e);
                key(out, &mut f, "op");
                out.push('"');
                out.push_str(op.text());
                out.push('"');
                pos_key(out, &mut f);
                key(out, &mut f, "t");
                out.push_str(r#""unary""#);
            }
            Node::Bin { op, a, b } => {
                key(out, &mut f, "a");
                write(out, ast, a);
                key(out, &mut f, "b");
                write(out, ast, b);
                key(out, &mut f, "op");
                out.push('"');
                out.push_str(op.text());
                out.push('"');
                pos_key(out, &mut f);
                key(out, &mut f, "t");
                out.push_str(r#""bin""#);
            }
            Node::Paren(e) => {
                key(out, &mut f, "e");
                write(out, ast, e);
                pos_key(out, &mut f);
                key(out, &mut f, "t");
                out.push_str(r#""paren""#);
            }
        }
        out.push('}');
    }

    /// The TS `ParseOptions`/`PrintOptions` are plain objects: nothing else to canonicalize here.
    #[must_use]
    pub fn str_ref_text(ast: &Ast, r: StrRef) -> Option<String> {
        ast.lit_str(r).map(|s| {
            let mut o = String::new();
            js(&mut o, s);
            o
        })
    }
}
