//! `functions/filterxml.ts`: FILTERXML(xml; xpath) — the TS's small XML parser and its XPath 1.0 subset (paths `/`
//! and `//`, `*`, `@attribute`, `.`/`..`, predicates `[n]`, `[last()]`, `[position() op n]`, comparisons, `and`/`or`,
//! `+`/`-`, union `|`), read unit by unit as the TS reads them.
//!
//! Data model: the elements live in one arena in document order (the TS `order` counter), so a node is its index;
//! an attribute item carries a serial number because the TS builds a NEW object per attribute read (`new Set`
//! never merges two attribute items). The TS evaluates through closures; here the predicates are a small expression
//! tree. Bounds (A4d-5): the descendant walks and predicate evaluations of one call share a budget of
//! [`WORK_BUDGET`] steps (`//*//*//*…` over a big document is quadratic per step in the TS too), past it the call
//! throws the `RangeError` path; a `&#…;` outside Unicode throws the TS's `RangeError` (`String.fromCodePoint`).

use super::js::{eq_str, index_of, u};
use crate::coerce::parse_number_text;
use crate::fnreg::{CallArgs, FnBody, FnContext, Throw};
use crate::functions::shared::tdl::txt;
use crate::functions::shared::{R, Stop, bodies, scalar};
use crate::strings::JsStr;
use crate::value::{ArrayValue, ErrorCode, Scalar, Value};

/// The bodies of `filterxml.ts`.
pub(super) const FILTERXML: &[(&str, FnBody)] = bodies!["FILTERXML" => filterxml];

/// Node visits and predicate evaluations one call may spend.
const WORK_BUDGET: u64 = 1 << 25;

/// An element (`XNode`); index 0 is the `#document` root.
struct XNode {
    name: Vec<u16>,
    /// Attributes in `Map` order (a repeated name keeps its first position and the last value).
    attrs: Vec<(Vec<u16>, Vec<u16>)>,
    children: Vec<u32>,
    /// `text.join('')`: the direct text.
    text: Vec<u16>,
    parent: Option<u32>,
    /// One past the last descendant (document order: the descendants are `id + 1 .. end`).
    end: u32,
}

/// An item of a node set: an element or an attribute of one (owner, index, serial).
#[derive(Clone, Copy, PartialEq, Eq)]
enum Item {
    Node(u32),
    Attr(u32, u32, u32),
}

impl Item {
    /// The `order` sort key (an attribute sits after its owner, before the next element).
    fn key(self) -> u64 {
        match self {
            Item::Node(n) => u64::from(n) * 2,
            Item::Attr(n, ..) => u64::from(n) * 2 + 1,
        }
    }
}

const fn c(b: u8) -> u16 {
    b as u16
}

/// `decode(s)`: the five entities and numeric references (`/&(#x[0-9a-f]+|#\d+|amp|lt|gt|quot|apos);/gi`).
fn decode(s: &[u16], out: &mut Vec<u16>) -> Result<(), Throw> {
    let mut i = 0;
    while i < s.len() {
        if s[i] == c(b'&')
            && let Some((len, v)) = entity(&s[i + 1..])
        {
            match v {
                Ent::Unit(x) => out.push(x),
                Ent::Cp(cp) => {
                    if cp > 0x10_FFFF {
                        return Err(Throw::Range);
                    }
                    super::js::push_cp(out, cp);
                }
            }
            i += 1 + len;
            continue;
        }
        out.push(s[i]);
        i += 1;
    }
    Ok(())
}

enum Ent {
    Unit(u16),
    Cp(u64),
}

/// The entity after a `&` (length including the `;`).
fn entity(s: &[u16]) -> Option<(usize, Ent)> {
    let is = |p: &[u8]| super::js::starts_ci(s, 0, p) && s.get(p.len()) == Some(&c(b';'));
    for (name, v) in [(b"amp" as &[u8], b'&'), (b"lt", b'<'), (b"gt", b'>'), (b"quot", b'"'), (b"apos", b'\'')] {
        if is(name) {
            return Some((name.len() + 1, Ent::Unit(c(v))));
        }
    }
    if s.first() != Some(&c(b'#')) {
        return None;
    }
    let hex = s.get(1).is_some_and(|&x| x == c(b'x') || x == c(b'X'));
    let (start, ok): (usize, fn(u16) -> bool) =
        if hex { (2, |x| x < 0x80 && (x as u8).is_ascii_hexdigit()) } else { (1, |x| (c(b'0')..=c(b'9')).contains(&x)) };
    let n = s.get(start..).map_or(0, |r| r.iter().take_while(|&&x| ok(x)).count());
    if n == 0 || s.get(start + n) != Some(&c(b';')) {
        // `#x` followed by no hex digit may still be `#\d+`? No: `x` is not a digit
        return None;
    }
    let digits = &s[start..start + n];
    let v = uo_jsnum::parse::parse_int_utf16(digits, if hex { 16 } else { 10 });
    // a value past U+10FFFF (or past 2^64) is the `RangeError`
    let cp = if v.is_finite() && v <= 1.8e19 { crate::cast::trunc_u64(v) } else { u64::MAX };
    Some((start + n + 1, Ent::Cp(cp)))
}

/// `/\s/` without `u` on one unit.
fn is_space(x: u16) -> bool {
    uo_jsnum::text::is_js_space_unit(x)
}

/// `[\w.:-]` (ASCII `\w`).
fn is_name_char(x: u16) -> bool {
    x < 0x80 && ((x as u8).is_ascii_alphanumeric() || matches!(x as u8, b'_' | b'.' | b':' | b'-'))
}

fn is_name_start(x: u16) -> bool {
    x < 0x80 && ((x as u8).is_ascii_alphabetic() || x == c(b'_'))
}

/// A name `[A-Za-z_][\w.:-]*` at the start of `s` (its length; 0 = none).
fn name_len(s: &[u16]) -> usize {
    match s.first() {
        Some(&x) if is_name_start(x) => 1 + s[1..].iter().take_while(|&&x| is_name_char(x)).count(),
        _ => 0,
    }
}

/// `parseXml(src)`: the element arena (`Ok(None)` = malformed).
fn parse_xml(src: &[u16]) -> Result<Option<Vec<XNode>>, Throw> {
    let mut nodes = vec![XNode { name: u("#document"), attrs: Vec::new(), children: Vec::new(), text: Vec::new(), parent: None, end: 0 }];
    let n = src.len();
    let mut i = 0;
    let mut cur: u32 = 0;
    let starts = |i: usize, p: &str| src.get(i..).is_some_and(|r| r.len() >= p.len() && eq_str(&r[..p.len()], p));
    while i < n {
        if src[i] != c(b'<') {
            let j = index_of(src, &[c(b'<')], i).unwrap_or(n);
            let t = &src[i..j];
            if cur == 0 {
                if !uo_text::space::trim(t).is_empty() {
                    return Ok(None);
                }
            } else {
                decode(t, &mut nodes[cur as usize].text)?;
            }
            i = j;
            continue;
        }
        if starts(i, "<!--") {
            let Some(j) = index_of(src, &u("-->"), i + 4) else { return Ok(None) };
            i = j + 3;
            continue;
        }
        if starts(i, "<![CDATA[") {
            let Some(j) = index_of(src, &u("]]>"), i + 9) else { return Ok(None) };
            if cur == 0 {
                return Ok(None);
            }
            nodes[cur as usize].text.extend_from_slice(&src[i + 9..j]);
            i = j + 3;
            continue;
        }
        if starts(i, "<?") {
            let Some(j) = index_of(src, &u("?>"), i + 2) else { return Ok(None) };
            i = j + 2;
            continue;
        }
        if starts(i, "<!") {
            let Some(j) = index_of(src, &[c(b'>')], i + 2) else { return Ok(None) };
            i = j + 1;
            continue;
        }
        if src.get(i + 1) == Some(&c(b'/')) {
            let Some(j) = index_of(src, &[c(b'>')], i) else { return Ok(None) };
            let name = uo_text::space::trim(src.get(i + 2..j).unwrap_or(&[]));
            let node = &nodes[cur as usize];
            let Some(parent) = node.parent else { return Ok(None) };
            if node.name[..] != *name {
                return Ok(None);
            }
            nodes[cur as usize].end = nodes.len() as u32;
            cur = parent;
            i = j + 1;
            continue;
        }
        // `/^<([A-Za-z_][\w.:-]*)/.exec(src.slice(i, i + 256))`
        let window = &src[i + 1..(i + 256).min(n)];
        let nl = name_len(window);
        if nl == 0 {
            return Ok(None);
        }
        let id = nodes.len() as u32;
        nodes.push(XNode {
            name: window[..nl].to_vec(),
            attrs: Vec::new(),
            children: Vec::new(),
            text: Vec::new(),
            parent: Some(cur),
            end: 0,
        });
        i += 1 + nl;
        loop {
            while i < n && is_space(src[i]) {
                i += 1;
            }
            if starts(i, "/>") {
                i += 2;
                nodes[cur as usize].children.push(id);
                nodes[id as usize].end = id + 1;
                break;
            }
            if src.get(i) == Some(&c(b'>')) {
                i += 1;
                nodes[cur as usize].children.push(id);
                cur = id;
                break;
            }
            // `/^([A-Za-z_][\w.:-]*)\s*=\s*("([^"]*)"|'([^']*)')/`
            let rest = &src[i..];
            let an = name_len(rest);
            if an == 0 {
                return Ok(None);
            }
            let mut k = an;
            while k < rest.len() && is_space(rest[k]) {
                k += 1;
            }
            if rest.get(k) != Some(&c(b'=')) {
                return Ok(None);
            }
            k += 1;
            while k < rest.len() && is_space(rest[k]) {
                k += 1;
            }
            let q = match rest.get(k) {
                Some(&q) if q == c(b'"') || q == c(b'\'') => q,
                _ => return Ok(None),
            };
            let Some(e) = index_of(rest, &[q], k + 1) else { return Ok(None) };
            let mut v = Vec::new();
            decode(&rest[k + 1..e], &mut v)?;
            let name = &rest[..an];
            let attrs = &mut nodes[id as usize].attrs;
            match attrs.iter_mut().find(|(a, _)| a[..] == *name) {
                Some(slot) => slot.1 = v,
                None => attrs.push((name.to_vec(), v)),
            }
            i += e + 1;
        }
    }
    if cur != 0 || nodes[0].children.len() != 1 {
        return Ok(None);
    }
    nodes[0].end = nodes.len() as u32;
    Ok(Some(nodes))
}

// ───────────────────────────── XPath ─────────────────────────────

/// `tokenize(s)` (`None` = the TS `XPathError`).
fn tokenize(s: &[u16]) -> Option<Vec<Vec<u16>>> {
    let mut out = Vec::new();
    let mut i = 0;
    while i < s.len() {
        let ch = s[i];
        if is_space(ch) {
            i += 1;
            continue;
        }
        let two = s.get(i..i + 2);
        if let Some(t) = two
            && ["//", "!=", "<=", ">=", ".."].iter().any(|p| eq_str(t, p))
        {
            out.push(t.to_vec());
            i += 2;
            continue;
        }
        if ch < 0x80 && b"/[]()@|,=<>*.-+".contains(&(ch as u8)) {
            out.push(vec![ch]);
            i += 1;
            continue;
        }
        if ch == c(b'"') || ch == c(b'\'') {
            let j = index_of(s, &[ch], i + 1)?;
            out.push(s[i..=j].to_vec());
            i = j + 1;
            continue;
        }
        // `/^(\d+(\.\d+)?|[A-Za-z_][\w.:-]*)/`
        let nd = crate::jstext::digit_run(s, i);
        let len = if nd > 0 {
            let frac = if s.get(i + nd) == Some(&c(b'.')) { crate::jstext::digit_run(s, i + nd + 1) } else { 0 };
            if frac > 0 { nd + 1 + frac } else { nd }
        } else {
            name_len(&s[i..])
        };
        if len == 0 {
            return None;
        }
        out.push(s[i..i + len].to_vec());
        i += len;
    }
    Some(out)
}

#[derive(Clone, Copy, PartialEq)]
enum Axis {
    Child,
    Desc,
    Attr,
    Current,
    Parent,
    /// `//@name`.
    DescAttr,
}

struct Step {
    axis: Axis,
    /// `*` matches any name.
    name: Vec<u16>,
    preds: Vec<Ev>,
}

#[derive(Clone, Copy)]
enum CmpOp {
    Eq,
    Ne,
    Lt,
    Gt,
    Le,
    Ge,
}

/// An expression of a predicate (the TS closures).
enum Ev {
    Num(f64),
    Str(Vec<u16>),
    Or(Box<Ev>, Box<Ev>),
    And(Box<Ev>, Box<Ev>),
    Cmp(CmpOp, Box<Ev>, Box<Ev>),
    Add(bool, Box<Ev>, Box<Ev>),
    Attr(Vec<u16>),
    Dot,
    Last,
    Position,
    Text,
    Child(Vec<u16>),
}

/// A value of an expression (`EvVal`): a list holds items or texts (`text()`).
enum Val {
    Num(f64),
    Str(Vec<u16>),
    Bool(bool),
    List(Vec<LItem>),
}

enum LItem {
    Item(Item),
    Text(Vec<u16>),
}

struct Parser {
    t: Vec<Vec<u16>>,
    i: usize,
}

impl Parser {
    fn peek_is(&self, s: &str) -> bool {
        self.t.get(self.i).is_some_and(|x| eq_str(x, s))
    }
    fn next(&mut self) -> Option<Vec<u16>> {
        let x = self.t.get(self.i)?.clone();
        self.i += 1;
        Some(x)
    }
    fn eat(&mut self, s: &str) -> bool {
        if self.peek_is(s) {
            self.i += 1;
            true
        } else {
            false
        }
    }
    fn done(&self) -> bool {
        self.i >= self.t.len()
    }

    fn union(&mut self) -> Option<Vec<Vec<Step>>> {
        let mut paths = vec![self.path()?];
        while self.eat("|") {
            paths.push(self.path()?);
        }
        Some(paths)
    }

    fn path(&mut self) -> Option<Vec<Step>> {
        let mut steps = Vec::new();
        let mut desc = false;
        if self.eat("//") {
            desc = true;
        } else if !self.eat("/") {
            steps.push(Step { axis: Axis::Current, name: u("#ctx"), preds: Vec::new() });
        }
        steps.push(self.step(desc)?);
        loop {
            if self.eat("//") {
                steps.push(self.step(true)?);
            } else if self.eat("/") {
                steps.push(self.step(false)?);
            } else {
                break;
            }
        }
        Some(steps)
    }

    fn step(&mut self, desc: bool) -> Option<Step> {
        if self.eat(".") {
            return Some(Step { axis: Axis::Current, name: u("*"), preds: Vec::new() });
        }
        if self.eat("..") {
            return Some(Step { axis: Axis::Parent, name: u("*"), preds: Vec::new() });
        }
        let mut axis = if desc { Axis::Desc } else { Axis::Child };
        if self.eat("@") {
            axis = Axis::Attr;
        }
        let name = self.next()?;
        if eq_str(&name, "text") || eq_str(&name, "node") || eq_str(&name, "comment") {
            return None;
        }
        if !(eq_str(&name, "*") || name.first().is_some_and(|&x| is_name_start(x))) {
            return None;
        }
        if self.peek_is("(") {
            return None;
        }
        let mut preds = Vec::new();
        while self.eat("[") {
            preds.push(self.or_expr()?);
            if !self.eat("]") {
                return None;
            }
        }
        if axis == Axis::Attr && desc {
            return Some(Step { axis: Axis::DescAttr, name, preds });
        }
        Some(Step { axis, name, preds })
    }

    fn or_expr(&mut self) -> Option<Ev> {
        let mut a = self.and_expr()?;
        while self.eat("or") {
            a = Ev::Or(Box::new(a), Box::new(self.and_expr()?));
        }
        Some(a)
    }

    fn and_expr(&mut self) -> Option<Ev> {
        let mut a = self.cmp_expr()?;
        while self.eat("and") {
            a = Ev::And(Box::new(a), Box::new(self.cmp_expr()?));
        }
        Some(a)
    }

    fn cmp_expr(&mut self) -> Option<Ev> {
        let l = self.add_expr()?;
        let op = [("=", CmpOp::Eq), ("!=", CmpOp::Ne), ("<", CmpOp::Lt), (">", CmpOp::Gt), ("<=", CmpOp::Le), (">=", CmpOp::Ge)]
            .into_iter()
            .find(|(s, _)| self.peek_is(s))
            .map(|(_, o)| o);
        if let Some(op) = op {
            self.i += 1;
            let r = self.add_expr()?;
            return Some(Ev::Cmp(op, Box::new(l), Box::new(r)));
        }
        Some(l)
    }

    fn add_expr(&mut self) -> Option<Ev> {
        let mut a = self.primary()?;
        loop {
            let plus = if self.peek_is("+") {
                true
            } else if self.peek_is("-") {
                false
            } else {
                break;
            };
            self.i += 1;
            a = Ev::Add(plus, Box::new(a), Box::new(self.primary()?));
        }
        Some(a)
    }

    fn primary(&mut self) -> Option<Ev> {
        let t = self.next()?;
        let first = *t.first()?;
        if crate::jstext::is_digit(first) {
            return Some(Ev::Num(uo_jsnum::parse::to_number(&String::from_utf16_lossy(&t))));
        }
        if first == c(b'"') || first == c(b'\'') {
            return Some(Ev::Str(t[1..t.len() - 1].to_vec()));
        }
        if eq_str(&t, "(") {
            let e = self.or_expr()?;
            if !self.eat(")") {
                return None;
            }
            return Some(e);
        }
        if eq_str(&t, "@") {
            return Some(Ev::Attr(self.next()?));
        }
        if eq_str(&t, ".") {
            return Some(Ev::Dot);
        }
        if (eq_str(&t, "last") || eq_str(&t, "position") || eq_str(&t, "text")) && self.peek_is("(") {
            self.i += 1;
            if !self.eat(")") {
                return None;
            }
            return Some(if eq_str(&t, "last") {
                Ev::Last
            } else if eq_str(&t, "position") {
                Ev::Position
            } else {
                Ev::Text
            });
        }
        if is_name_start(first) {
            return Some(Ev::Child(t));
        }
        None
    }
}

/// The evaluation state of one call: the document and the work budget.
struct Doc {
    nodes: Vec<XNode>,
    work: u64,
    serial: u32,
}

/// `match(name, want)`.
fn matches(name: &[u16], want: &[u16]) -> bool {
    eq_str(want, "*") || name == want
}

impl Doc {
    fn tick(&mut self, n: u64) -> Result<(), Throw> {
        self.work = self.work.saturating_add(n);
        if self.work > WORK_BUDGET { Err(Throw::Range) } else { Ok(()) }
    }

    /// `attrOf(n, want)` (fresh items; `xmlns*` skipped).
    fn attrs_of(&mut self, n: u32, want: &[u16], out: &mut Vec<Item>) {
        for (k, (name, _)) in self.nodes[n as usize].attrs.iter().enumerate() {
            if !(name.len() >= 5 && eq_str(&name[..5], "xmlns")) && matches(name, want) {
                self.serial += 1;
                out.push(Item::Attr(n, k as u32, self.serial));
            }
        }
    }

    fn str_of(&self, x: &LItem) -> Vec<u16> {
        match x {
            LItem::Text(s) => s.clone(),
            LItem::Item(Item::Node(n)) => self.nodes[*n as usize].text.clone(),
            LItem::Item(Item::Attr(n, k, _)) => self.nodes[*n as usize].attrs[*k as usize].1.clone(),
        }
    }

    fn eval(&mut self, e: &Ev, it: Item, pos: f64, size: f64) -> Result<Val, Throw> {
        self.tick(1)?;
        Ok(match e {
            Ev::Num(x) => Val::Num(*x),
            Ev::Str(s) => Val::Str(s.clone()),
            Ev::Or(a, b) => {
                let x = self.eval(a, it, pos, size)?;
                Val::Bool(truthy(&x) || truthy(&self.eval(b, it, pos, size)?))
            }
            Ev::And(a, b) => {
                let x = self.eval(a, it, pos, size)?;
                Val::Bool(truthy(&x) && truthy(&self.eval(b, it, pos, size)?))
            }
            Ev::Cmp(op, a, b) => {
                let x = self.eval(a, it, pos, size)?;
                let y = self.eval(b, it, pos, size)?;
                Val::Bool(self.compare(&x, &y, *op))
            }
            Ev::Add(plus, a, b) => {
                let x = self.eval(a, it, pos, size)?;
                let y = self.eval(b, it, pos, size)?;
                let (x, y) = (self.num(&x), self.num(&y));
                Val::Num(if *plus { x + y } else { x - y })
            }
            Ev::Attr(name) => {
                let mut v = Vec::new();
                if let Item::Node(n) = it {
                    self.attrs_of(n, name, &mut v);
                }
                Val::List(v.into_iter().map(LItem::Item).collect())
            }
            Ev::Dot => Val::List(vec![LItem::Item(it)]),
            Ev::Last => Val::Num(size),
            Ev::Position => Val::Num(pos),
            Ev::Text => match it {
                Item::Node(n) => Val::List(vec![LItem::Text(self.nodes[n as usize].text.clone())]),
                Item::Attr(..) => Val::List(Vec::new()),
            },
            Ev::Child(name) => match it {
                Item::Node(n) => {
                    let ch: Vec<LItem> = self.nodes[n as usize]
                        .children
                        .iter()
                        .filter(|&&k| matches(&self.nodes[k as usize].name, name))
                        .map(|&k| LItem::Item(Item::Node(k)))
                        .collect();
                    self.tick(self.nodes[n as usize].children.len() as u64)?;
                    Val::List(ch)
                }
                Item::Attr(..) => Val::List(Vec::new()),
            },
        })
    }

    /// `num(v)`.
    fn num(&self, v: &Val) -> f64 {
        match v {
            Val::List(l) => l.first().map_or(f64::NAN, |x| to_number(&self.str_of(x))),
            Val::Num(x) => *x,
            Val::Bool(b) => f64::from(u8::from(*b)),
            Val::Str(s) => to_number(s),
        }
    }

    /// `compare(a, b, op)`.
    fn compare(&self, a: &Val, b: &Val, op: CmpOp) -> bool {
        let flat = |v: &Val| -> Vec<Scal> {
            match v {
                Val::List(l) => l.iter().map(|x| Scal::S(self.str_of(x))).collect(),
                Val::Num(x) => vec![Scal::N(*x)],
                Val::Str(s) => vec![Scal::S(s.clone())],
                Val::Bool(b) => vec![Scal::B(*b)],
            }
        };
        let (la, lb) = (flat(a), flat(b));
        for x in &la {
            for y in &lb {
                let numeric = matches!(x, Scal::N(_)) || matches!(y, Scal::N(_)) || !matches!(op, CmpOp::Eq | CmpOp::Ne);
                let ok = if numeric {
                    let (p, q) = (x.number(), y.number());
                    match op {
                        CmpOp::Eq => p == q,
                        CmpOp::Ne => p != q,
                        CmpOp::Lt => p < q,
                        CmpOp::Gt => p > q,
                        CmpOp::Le => p <= q,
                        CmpOp::Ge => p >= q,
                    }
                } else {
                    let eq = x.string() == y.string();
                    if matches!(op, CmpOp::Eq) { eq } else { !eq }
                };
                if ok {
                    return true;
                }
            }
        }
        false
    }

    /// `evalPath(root, steps)`.
    fn eval_path(&mut self, steps: &[Step]) -> Result<Vec<Item>, Throw> {
        let mut cur: Vec<Item> = vec![Item::Node(0)];
        let mut seen = vec![false; self.nodes.len()];
        for st in steps {
            let mut next: Vec<Item> = Vec::new();
            seen.iter_mut().for_each(|s| *s = false);
            for &it in &cur {
                let Item::Node(n) = it else { continue };
                let (parent, end, nch) = {
                    let node = &self.nodes[n as usize];
                    (node.parent, node.end, node.children.len())
                };
                let mut cand: Vec<Item> = Vec::new();
                match st.axis {
                    Axis::Current => cand.push(it),
                    Axis::Parent => cand.extend(parent.map(Item::Node)),
                    Axis::Child => {
                        self.tick(nch as u64)?;
                        let node = &self.nodes[n as usize];
                        cand.extend(
                            node.children.iter().filter(|&&k| matches(&self.nodes[k as usize].name, &st.name)).map(|&k| Item::Node(k)),
                        );
                    }
                    Axis::Attr => self.attrs_of(n, &st.name, &mut cand),
                    Axis::Desc | Axis::DescAttr => {
                        let (a, b) = (n + 1, end.max(n + 1));
                        self.tick(u64::from(b - a) + 1)?;
                        if st.axis == Axis::DescAttr {
                            self.attrs_of(n, &st.name, &mut cand);
                            for d in a..b {
                                self.attrs_of(d, &st.name, &mut cand);
                            }
                        } else {
                            cand.extend((a..b).filter(|&d| matches(&self.nodes[d as usize].name, &st.name)).map(Item::Node));
                        }
                    }
                }
                for p in &st.preds {
                    let size = cand.len() as f64;
                    let mut kept = Vec::with_capacity(cand.len());
                    for (k, &x) in cand.iter().enumerate() {
                        let pos = (k + 1) as f64;
                        let v = self.eval(p, x, pos, size)?;
                        let keep = match v {
                            Val::Num(x) => x == pos,
                            v => truthy(&v),
                        };
                        if keep {
                            kept.push(x);
                        }
                    }
                    cand = kept;
                }
                // `next.push(...cand)` then `new Set`: elements once (first occurrence), attributes always
                for x in cand {
                    match x {
                        Item::Node(k) if seen[k as usize] => {}
                        Item::Node(k) => {
                            seen[k as usize] = true;
                            next.push(x);
                        }
                        Item::Attr(..) => next.push(x),
                    }
                }
            }
            next.sort_by_key(|x| x.key());
            cur = next;
        }
        Ok(cur)
    }
}

/// A comparison operand.
enum Scal {
    N(f64),
    S(Vec<u16>),
    B(bool),
}

impl Scal {
    fn number(&self) -> f64 {
        match self {
            Scal::N(x) => *x,
            Scal::S(s) => to_number(s),
            Scal::B(b) => f64::from(u8::from(*b)),
        }
    }
    fn string(&self) -> Vec<u16> {
        match self {
            Scal::N(x) => u(&uo_jsnum::format::to_js_string(*x)),
            Scal::S(s) => s.clone(),
            Scal::B(b) => u(if *b { "true" } else { "false" }),
        }
    }
}

/// `Number(s)` of a unit string (a lone surrogate is not a digit nor white space: NaN either way).
fn to_number(s: &[u16]) -> f64 {
    uo_jsnum::parse::to_number(&String::from_utf16_lossy(s))
}

/// `truthy(v)`.
fn truthy(v: &Val) -> bool {
    match v {
        Val::List(l) => !l.is_empty(),
        Val::Num(x) => *x != 0.0 && !x.is_nan(),
        Val::Str(s) => !s.is_empty(),
        Val::Bool(b) => *b,
    }
}

fn filterxml(a: &CallArgs<'_>, ctx: &mut dyn FnContext) -> R<Value> {
    let xml = txt(ctx, scalar(a, 0).unwrap_or(Scalar::Empty))?.as_js().to_utf16();
    let xp = txt(ctx, scalar(a, 1).unwrap_or(Scalar::Empty))?.as_js().to_utf16();
    let Some(nodes) = parse_xml(&xml)? else { return Err(Stop::Err(ErrorCode::Value)) };
    let Some(toks) = tokenize(&xp) else { return Err(Stop::Err(ErrorCode::Value)) };
    let mut p = Parser { t: toks, i: 0 };
    let Some(paths) = p.union() else { return Err(Stop::Err(ErrorCode::Value)) };
    if !p.done() {
        return Err(Stop::Err(ErrorCode::Value));
    }
    let mut doc = Doc { nodes, work: 0, serial: 0 };
    let mut all: Vec<Item> = Vec::new();
    let mut seen = vec![false; doc.nodes.len()];
    for path in &paths {
        for x in doc.eval_path(path)? {
            match x {
                Item::Node(k) if seen[k as usize] => {}
                Item::Node(k) => {
                    seen[k as usize] = true;
                    all.push(x);
                }
                Item::Attr(..) => all.push(x),
            }
        }
    }
    all.sort_by_key(|x| x.key());
    all.retain(|&x| x != Item::Node(0));
    if all.is_empty() {
        return Err(Stop::Err(ErrorCode::Value));
    }
    let (date1904, year) = (ctx.date1904(), ctx.year());
    let mut vals: Vec<Scalar> = Vec::with_capacity(all.len());
    for x in all {
        let s: &[u16] = match x {
            Item::Node(n) => &doc.nodes[n as usize].text,
            Item::Attr(n, k, _) => &doc.nodes[n as usize].attrs[k as usize].1,
        };
        if matches!(x, Item::Node(_)) && s.is_empty() {
            vals.push(Scalar::Err(ErrorCode::Value));
            continue;
        }
        match parse_number_text(JsStr::Utf16(s), ctx.locale(), date1904, year) {
            Some(n) => vals.push(Scalar::Num(n)),
            None => vals.push(Scalar::Str(ctx.new_text(JsStr::Utf16(s))?)),
        }
    }
    if vals.len() == 1 {
        return Ok(Value::Scalar(vals[0]));
    }
    let rows = u32::try_from(vals.len()).map_err(|_| Stop::Throw(Throw::Range))?;
    Ok(Value::Array(ArrayValue::new(rows, 1, vals)))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(xml: &str, xp: &str) -> Option<Vec<String>> {
        let nodes = parse_xml(&u(xml)).ok()??;
        let mut p = Parser { t: tokenize(&u(xp))?, i: 0 };
        let paths = p.union()?;
        let mut doc = Doc { nodes, work: 0, serial: 0 };
        let mut out = Vec::new();
        for path in &paths {
            for x in doc.eval_path(path).ok()? {
                out.push(match x {
                    Item::Node(n) => String::from_utf16_lossy(&doc.nodes[n as usize].text),
                    Item::Attr(n, k, _) => String::from_utf16_lossy(&doc.nodes[n as usize].attrs[k as usize].1),
                });
            }
        }
        Some(out)
    }

    #[test]
    fn paths_and_predicates() {
        let x = "<r><a k='1'>x<c>y</c>z</a><a k='2'>w</a></r>";
        assert_eq!(run(x, "//a").unwrap(), ["xz", "w"]);
        assert_eq!(run(x, "//a[2]").unwrap(), ["w"]);
        assert_eq!(run(x, "//a[@k='2']").unwrap(), ["w"]);
        assert_eq!(run(x, "/r/a/@k").unwrap(), ["1", "2"]);
        assert_eq!(run(x, "//a[last()]").unwrap(), ["w"]);
        assert!(parse_xml(&u("<a><b></a>")).unwrap().is_none());
        assert!(parse_xml(&u("<a>&#1114112;</a>")).is_err());
    }
}
