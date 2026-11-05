//! The find engine's regular expressions (D2b-F9, Opus): `find-engine.ts` builds a JavaScript
//! `RegExp` source from the Find box (`compileQuery`, `wildcardRegex`) and runs it with the flags
//! `gu` or `giu`; this module is that `RegExp` as a hand matcher, bounded.
//!
//! - **Parser**: the ES2025 pattern grammar in Unicode mode (`u`), over the code points of the
//!   source (a surrogate pair is one character, a lone surrogate is itself), for the constructs the
//!   builder can emit (literals, identity escapes, `\t \n \v \f \r \uHHHH \p{L} \p{N} \p{P}`, classes
//!   with ranges, `(`, `(?:`, the four lookarounds, `* + ? {n} {n,} {n,m}` greedy and lazy, `|`),
//!   with V8's error messages in V8's order (left to right; an open group is reported at the end).
//!   A construct the builder never emits (`.`, `^`, `$`, `\b`, `\d`, backreferences, named groups)
//!   answers [`RegexError::Unsupported`]; the parity capture checks that it never happens.
//! - **Matcher**: a backtracking machine with the ES semantics (`RepeatMatcher` with its empty check
//!   and the capture reset of every iteration, atomic lookarounds, lookbehinds matched backwards,
//!   the `u`-mode reading by code points and V8's step back when `lastIndex` falls inside a
//!   surrogate pair) on an explicit stack, so no pattern can overflow the call stack.
//! - **Bounded**: every instruction costs one step of a [`Budget`] shared by the whole command
//!   (catastrophic backtracking, and V8's endless loop of empty matches before an astral character,
//!   end in [`BudgetExceeded`]); the backtrack stack is capped at [`MAX_STACK`] entries. Parser and
//!   compiler work on explicit stacks too: any nesting compiles (V8 reports "Stack overflow" past
//!   about 10^4 nested capture groups; declared). A scan prune (the literal runs every match must
//!   contain, [`Regex::scan_limit`]) skips the start positions that cannot match: no result changes.
//! - **Case** (`i`): two characters match when they are in the same class of [`super::fold`] (V8's
//!   simple case folding, generated from V8); a class matches when a member of the character's fold
//!   class is in it, and a negated class when none is (V8 closes a class over case before
//!   complementing it: `/[^\p{L}]/iu` does not match U+0345).
//! - **Shortcut**: a body that can only match the empty string (a lookaround) runs once when its
//!   minimum is at least 1 and not at all otherwise — the ES result (later iterations repeat the
//!   first one at the same position, the optional ones fail the empty check), and what V8 does.

use super::fold::{INDEX, POOL};

/// Entries of the backtrack stack before a search gives up ([`BudgetExceeded`]).
pub const MAX_STACK: usize = 1 << 18;
/// The steps one find command may take (all its stories together).
pub const DEFAULT_STEP_BUDGET: u64 = 1 << 26;
/// V8's `RegExpTree::kInfinity` (`kMaxInt`): quantifier bounds clamp to it; as a maximum it means
/// unbounded.
const INFINITY: u32 = 2_147_483_647;
/// V8's `kMaxCaptures`.
const MAX_CAPTURES: u32 = 1 << 16;
const NONE: u32 = u32::MAX;

/// The step budget of one command.
#[derive(Debug, Clone)]
pub struct Budget {
    left: u64,
    used: u64,
}

impl Budget {
    #[must_use]
    pub fn new(steps: u64) -> Budget {
        Budget { left: steps, used: 0 }
    }

    /// Steps taken so far.
    #[must_use]
    pub fn used(&self) -> u64 {
        self.used
    }

    #[inline]
    fn step(&mut self) -> Result<(), BudgetExceeded> {
        self.charge(1)
    }

    /// Charges `n` steps (the scans of the engine outside the matcher).
    #[inline]
    pub fn charge(&mut self, n: u64) -> Result<(), BudgetExceeded> {
        if self.left < n {
            self.used += self.left;
            self.left = 0;
            return Err(BudgetExceeded);
        }
        self.left -= n;
        self.used += n;
        Ok(())
    }
}

impl Default for Budget {
    fn default() -> Budget {
        Budget::new(DEFAULT_STEP_BUDGET)
    }
}

/// The search ran out of its [`Budget`] (or of [`MAX_STACK`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BudgetExceeded;

/// Why a source does not compile.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RegexError {
    /// V8's `SyntaxError` reason (the text after the source in the message)
    Syntax(&'static str),
    /// a construct the find builder never emits
    Unsupported,
}

/// The fold class of `cp` (`None`: it matches only itself under `i`).
fn fold_class(cp: u32) -> Option<&'static [u32]> {
    let i = INDEX.binary_search_by_key(&cp, |e| e.0).ok()?;
    let &(_, start, n) = INDEX.get(i)?;
    POOL.get(usize::from(start)..usize::from(start) + usize::from(n))
}

// ───────────────────────── the parsed pattern ─────────────────────────

#[derive(Debug, Clone, Default)]
struct ClassSet {
    /// sorted and merged by `finish`
    ranges: Vec<(u32, u32)>,
    props: u8,
}

impl ClassSet {
    fn finish(&mut self) {
        self.ranges.sort_unstable();
        let mut out: Vec<(u32, u32)> = Vec::with_capacity(self.ranges.len());
        for &(a, b) in &self.ranges {
            match out.last_mut() {
                Some(last) if a <= last.1.saturating_add(1) => last.1 = last.1.max(b),
                _ => out.push((a, b)),
            }
        }
        self.ranges = out;
    }

    fn has(&self, cp: u32) -> bool {
        if self.props != 0 && uo_text::props::mask(cp) & self.props != 0 {
            return true;
        }
        let i = self.ranges.partition_point(|r| r.1 < cp);
        self.ranges.get(i).is_some_and(|r| r.0 <= cp)
    }
}

#[derive(Debug, Clone)]
enum Node {
    Char(u32),
    /// a class and whether it is negated (`[^]` is an empty negated class)
    Class(u32, bool),
    Seq(Vec<u32>),
    Alt(Vec<u32>),
    /// capture index (0 = non-capturing), body, the captures inside `[lo, hi)`
    Group(u32, u32, u32, u32),
    /// ahead, negative, body, the captures inside `[lo, hi)`
    Look(bool, bool, u32, u32, u32),
    /// body, min, max (`INFINITY` = unbounded), greedy, the captures inside `[lo, hi)`
    Repeat(u32, u32, u32, bool, u32, u32),
}

const fn c(ch: char) -> u32 {
    ch as u32
}

fn is_syntax(cp: u32) -> bool {
    matches!(char::from_u32(cp), Some('^' | '$' | '\\' | '.' | '*' | '+' | '?' | '(' | ')' | '[' | ']' | '{' | '}' | '|'))
}

#[derive(Debug, Clone, Copy)]
enum Kind {
    Top,
    Capture(u32),
    NonCapture,
    Look(bool, bool),
}

struct Frame {
    kind: Kind,
    alts: Vec<u32>,
    terms: Vec<u32>,
    /// what the last term takes: `Some(true)` an atom (a quantifier), `Some(false)` a lookaround
    /// ("Invalid quantifier"), `None` nothing or already quantified ("Nothing to repeat")
    last: Option<bool>,
    /// the first capture index inside the group
    caps_lo: u32,
}

#[derive(Debug, Clone, Copy)]
enum ClassAtom {
    Char(u32),
    Prop(u8),
}

struct Parser<'a> {
    s: &'a [u32],
    i: usize,
    nodes: Vec<Node>,
    classes: Vec<ClassSet>,
    ncap: u32,
}

type PResult<T> = Result<T, RegexError>;

fn idx(n: usize) -> u32 {
    u32::try_from(n).unwrap_or(u32::MAX)
}

impl Parser<'_> {
    fn at(&self, k: usize) -> Option<u32> {
        self.s.get(self.i + k).copied()
    }

    fn peek(&self) -> Option<u32> {
        self.at(0)
    }

    fn node(&mut self, n: Node) -> u32 {
        self.nodes.push(n);
        idx(self.nodes.len() - 1)
    }

    fn class(&mut self, set: ClassSet) -> u32 {
        self.classes.push(set);
        idx(self.classes.len() - 1)
    }

    fn caps_of(&self, n: u32) -> (u32, u32) {
        match self.nodes.get(n as usize) {
            Some(Node::Group(_, _, lo, hi) | Node::Look(_, _, _, lo, hi)) => (*lo, *hi),
            _ => (0, 0),
        }
    }

    /// Digits of a quantifier bound, clamped at `INFINITY` as V8 does.
    fn digits(&mut self) -> Option<u32> {
        let start = self.i;
        let mut v: u32 = 0;
        while let Some(d) = self.peek().and_then(char::from_u32).and_then(|ch| ch.to_digit(10)) {
            v = if v > (INFINITY - d) / 10 { INFINITY } else { v * 10 + d };
            self.i += 1;
        }
        (self.i > start).then_some(v)
    }

    /// `ParseIntervalQuantifier` at `{`: the bounds, or `None` with the position restored.
    fn interval(&mut self) -> Option<(u32, u32)> {
        let save = self.i;
        self.i += 1;
        let r = self.interval_rest();
        if r.is_none() {
            self.i = save;
        }
        r
    }

    fn interval_rest(&mut self) -> Option<(u32, u32)> {
        let min = self.digits()?;
        let max = if self.peek() == Some(c(',')) {
            self.i += 1;
            if self.peek() == Some(c('}')) { INFINITY } else { self.digits()? }
        } else {
            min
        };
        if self.peek() != Some(c('}')) {
            return None;
        }
        self.i += 1;
        Some((min, max))
    }

    fn hex4(&self, k: usize) -> Option<u32> {
        let mut v = 0;
        for j in 0..4 {
            v = v * 16 + char::from_u32(self.at(k + j)?)?.to_digit(16)?;
        }
        Some(v)
    }

    /// `\u…`, the position on the `u`: `\uHHHH` (an escaped surrogate pair is one character) or
    /// `\u{H…}`.
    fn unicode_escape(&mut self) -> PResult<u32> {
        const BAD: RegexError = RegexError::Syntax("Invalid Unicode escape");
        if self.at(1) == Some(c('{')) {
            let mut k = 2;
            let mut v: u32 = 0;
            while let Some(d) = self.at(k).and_then(char::from_u32).and_then(|ch| ch.to_digit(16)) {
                v = v.saturating_mul(16).saturating_add(d);
                k += 1;
            }
            if k == 2 || self.at(k) != Some(c('}')) || v > 0x10FFFF {
                return Err(BAD);
            }
            self.i += k + 1;
            return Ok(v);
        }
        let v = self.hex4(1).ok_or(BAD)?;
        self.i += 5;
        if (0xD800..0xDC00).contains(&v)
            && self.peek() == Some(c('\\'))
            && self.at(1) == Some(c('u'))
            && let Some(lo) = self.hex4(2)
            && (0xDC00..0xE000).contains(&lo)
        {
            self.i += 6;
            return Ok(0x10000 + ((v - 0xD800) << 10) + (lo - 0xDC00));
        }
        Ok(v)
    }

    /// `\p{…}`, the position on the `p`: the class mask.
    fn property(&mut self) -> PResult<u8> {
        const BAD: RegexError = RegexError::Syntax("Invalid property name");
        if self.at(1) != Some(c('{')) {
            return Err(BAD);
        }
        let mut k = 2;
        while self.at(k).is_some_and(|ch| ch != c('}')) {
            k += 1;
        }
        if self.at(k).is_none() {
            return Err(BAD);
        }
        let name: Vec<u32> = self.s.get(self.i + 2..self.i + k).map(<[u32]>::to_vec).unwrap_or_default();
        let mask = match name.as_slice() {
            [x] if *x == c('L') => uo_text::props::LETTER,
            [x] if *x == c('N') => uo_text::props::NUMBER,
            [x] if *x == c('P') => uo_text::props::PUNCTUATION,
            [x] if *x == c('M') => uo_text::props::MARK,
            _ => return Err(RegexError::Unsupported),
        };
        self.i += k + 1;
        Ok(mask)
    }

    /// One class atom (the position on it).
    fn class_atom(&mut self) -> PResult<ClassAtom> {
        let Some(ch) = self.peek() else { return Err(RegexError::Syntax("Unterminated character class")) };
        if ch != c('\\') {
            self.i += 1;
            return Ok(ClassAtom::Char(ch));
        }
        let Some(e) = self.at(1) else { return Err(RegexError::Syntax("\\ at end of pattern")) };
        self.i += 1;
        let simple = |p: &mut Self, v: u32| {
            p.i += 1;
            Ok(ClassAtom::Char(v))
        };
        match char::from_u32(e) {
            Some('-') => simple(self, e),
            Some('b') => simple(self, 8),
            Some('t') => simple(self, 9),
            Some('n') => simple(self, 10),
            Some('v') => simple(self, 11),
            Some('f') => simple(self, 12),
            Some('r') => simple(self, 13),
            Some('u') => self.unicode_escape().map(ClassAtom::Char),
            Some('p') => self.property().map(ClassAtom::Prop),
            Some('d' | 'D' | 's' | 'S' | 'w' | 'W' | 'P' | 'c' | 'x' | '0'..='9' | 'k') => Err(RegexError::Unsupported),
            _ if is_syntax(e) || e == c('/') => simple(self, e),
            _ => Err(RegexError::Syntax("Invalid escape")),
        }
    }

    /// A class, the position on the `[`.
    fn class_body(&mut self) -> PResult<u32> {
        self.i += 1;
        let neg = self.peek() == Some(c('^'));
        if neg {
            self.i += 1;
        }
        let mut set = ClassSet::default();
        loop {
            match self.peek() {
                None => return Err(RegexError::Syntax("Unterminated character class")),
                Some(ch) if ch == c(']') => {
                    self.i += 1;
                    break;
                }
                Some(_) => {}
            }
            let a = self.class_atom()?;
            if self.peek() == Some(c('-')) && self.at(1).is_some_and(|x| x != c(']')) {
                self.i += 1;
                let b = self.class_atom()?;
                let (ClassAtom::Char(lo), ClassAtom::Char(hi)) = (a, b) else {
                    return Err(RegexError::Syntax("Invalid character class"));
                };
                if lo > hi {
                    return Err(RegexError::Syntax("Range out of order in character class"));
                }
                set.ranges.push((lo, hi));
            } else {
                match a {
                    ClassAtom::Char(x) => set.ranges.push((x, x)),
                    ClassAtom::Prop(m) => set.props |= m,
                }
            }
        }
        set.finish();
        let id = self.class(set);
        Ok(self.node(Node::Class(id, neg)))
    }

    /// An escape outside a class, the position on the `\`.
    fn atom_escape(&mut self) -> PResult<u32> {
        let Some(e) = self.at(1) else { return Err(RegexError::Syntax("\\ at end of pattern")) };
        self.i += 1;
        let ch = |p: &mut Self, v: u32| {
            p.i += 1;
            Ok(p.node(Node::Char(v)))
        };
        match char::from_u32(e) {
            Some('t') => ch(self, 9),
            Some('n') => ch(self, 10),
            Some('v') => ch(self, 11),
            Some('f') => ch(self, 12),
            Some('r') => ch(self, 13),
            Some('u') => {
                let v = self.unicode_escape()?;
                Ok(self.node(Node::Char(v)))
            }
            Some('p') => {
                let m = self.property()?;
                let id = self.class(ClassSet { ranges: Vec::new(), props: m });
                Ok(self.node(Node::Class(id, false)))
            }
            Some('b' | 'B' | 'd' | 'D' | 's' | 'S' | 'w' | 'W' | 'P' | 'c' | 'x' | '0'..='9' | 'k') => Err(RegexError::Unsupported),
            _ if is_syntax(e) || e == c('/') => ch(self, e),
            _ => Err(RegexError::Syntax("Invalid escape")),
        }
    }

    fn close(&mut self, f: Frame) -> u32 {
        let Frame { mut alts, terms, .. } = f;
        let seq = self.node(Node::Seq(terms));
        if alts.is_empty() {
            return seq;
        }
        alts.push(seq);
        self.node(Node::Alt(alts))
    }

    /// The quantifier just read applies to the last term of `f`.
    fn quantify(&mut self, f: &mut Frame, min: u32, max: u32) -> PResult<()> {
        match f.last {
            None => return Err(RegexError::Syntax("Nothing to repeat")),
            Some(false) => return Err(RegexError::Syntax("Invalid quantifier")),
            Some(true) => {}
        }
        let greedy = self.peek() != Some(c('?'));
        if !greedy {
            self.i += 1;
        }
        let Some(body) = f.terms.pop() else { return Err(RegexError::Syntax("Nothing to repeat")) };
        let (lo, hi) = self.caps_of(body);
        let n = self.node(Node::Repeat(body, min, max, greedy, lo, hi));
        f.terms.push(n);
        f.last = None;
        Ok(())
    }

    fn parse(mut self) -> PResult<Parsed> {
        let mut stack: Vec<Frame> = vec![Frame { kind: Kind::Top, alts: Vec::new(), terms: Vec::new(), last: None, caps_lo: 1 }];
        while let Some(ch) = self.peek() {
            let Some(mut top) = stack.pop() else { return Err(RegexError::Unsupported) };
            match char::from_u32(ch) {
                Some('*') => {
                    self.i += 1;
                    self.quantify(&mut top, 0, INFINITY)?;
                }
                Some('+') => {
                    self.i += 1;
                    self.quantify(&mut top, 1, INFINITY)?;
                }
                Some('?') => {
                    self.i += 1;
                    self.quantify(&mut top, 0, 1)?;
                }
                Some('{') => {
                    if top.last.is_none() {
                        let why = if self.interval().is_some() { "Nothing to repeat" } else { "Lone quantifier brackets" };
                        return Err(RegexError::Syntax(why));
                    }
                    let Some((min, max)) = self.interval() else { return Err(RegexError::Syntax("Incomplete quantifier")) };
                    if max < min {
                        return Err(RegexError::Syntax("numbers out of order in {} quantifier"));
                    }
                    self.quantify(&mut top, min, max)?;
                }
                Some('}' | ']') => return Err(RegexError::Syntax("Lone quantifier brackets")),
                Some('|') => {
                    self.i += 1;
                    let terms = std::mem::take(&mut top.terms);
                    let seq = self.node(Node::Seq(terms));
                    top.alts.push(seq);
                    top.last = None;
                }
                Some('(') => {
                    let kind = if self.at(1) == Some(c('?')) {
                        let k = match (self.at(2).and_then(char::from_u32), self.at(3).and_then(char::from_u32)) {
                            (Some(':'), _) => (Kind::NonCapture, 3),
                            (Some('='), _) => (Kind::Look(true, false), 3),
                            (Some('!'), _) => (Kind::Look(true, true), 3),
                            (Some('<'), Some('=')) => (Kind::Look(false, false), 4),
                            (Some('<'), Some('!')) => (Kind::Look(false, true), 4),
                            (Some('<'), _) => return Err(RegexError::Unsupported),
                            _ => return Err(RegexError::Syntax("Invalid group")),
                        };
                        self.i += k.1;
                        k.0
                    } else {
                        if self.ncap >= MAX_CAPTURES {
                            return Err(RegexError::Syntax("Too many captures"));
                        }
                        self.ncap += 1;
                        self.i += 1;
                        Kind::Capture(self.ncap)
                    };
                    let caps_lo = match kind {
                        Kind::Capture(g) => g,
                        _ => self.ncap + 1,
                    };
                    stack.push(top);
                    top = Frame { kind, alts: Vec::new(), terms: Vec::new(), last: None, caps_lo };
                }
                Some(')') => {
                    let Some(mut outer) = stack.pop() else { return Err(RegexError::Syntax("Unmatched ')'")) };
                    self.i += 1;
                    let (kind, lo) = (top.kind, top.caps_lo);
                    let body = self.close(top);
                    let hi = self.ncap + 1;
                    let (n, quantifiable) = match kind {
                        Kind::Capture(g) => (self.node(Node::Group(g, body, lo, hi)), true),
                        Kind::NonCapture => (self.node(Node::Group(0, body, lo, hi)), true),
                        Kind::Look(ahead, neg) => (self.node(Node::Look(ahead, neg, body, lo, hi)), false),
                        Kind::Top => return Err(RegexError::Unsupported),
                    };
                    outer.terms.push(n);
                    outer.last = Some(quantifiable);
                    top = outer;
                }
                Some('[') => {
                    let n = self.class_body()?;
                    top.terms.push(n);
                    top.last = Some(true);
                }
                Some('\\') => {
                    let n = self.atom_escape()?;
                    top.terms.push(n);
                    top.last = Some(true);
                }
                Some('.' | '^' | '$') => return Err(RegexError::Unsupported),
                _ => {
                    self.i += 1;
                    let n = self.node(Node::Char(ch));
                    top.terms.push(n);
                    top.last = Some(true);
                }
            }
            stack.push(top);
        }
        if stack.len() != 1 {
            return Err(RegexError::Syntax("Unterminated group"));
        }
        let Some(f) = stack.pop() else { return Err(RegexError::Unsupported) };
        let root = self.close(f);
        Ok(Parsed { nodes: self.nodes, classes: self.classes, root, ncap: self.ncap })
    }
}

struct Parsed {
    nodes: Vec<Node>,
    classes: Vec<ClassSet>,
    root: u32,
    ncap: u32,
}

// ───────────────────────── the program ─────────────────────────

#[derive(Debug, Clone, Copy)]
enum Inst {
    Char {
        cp: u32,
        back: bool,
    },
    Fold {
        cps: [u32; 4],
        n: u8,
        back: bool,
    },
    Class {
        id: u32,
        neg: bool,
        back: bool,
    },
    Any {
        back: bool,
    },
    /// try `x`, on failure `y`
    Split {
        x: u32,
        y: u32,
    },
    Jmp(u32),
    Save(u32),
    /// captures `[lo, hi)` become undefined (the reset of each iteration)
    Clear {
        lo: u32,
        hi: u32,
    },
    RepInit(u32),
    RepLoop {
        reg: u32,
        min: u32,
        max: u32,
        greedy: bool,
        body: u32,
        exit: u32,
    },
    RepPos(u32),
    RepEnd {
        reg: u32,
        preg: u32,
        min: u32,
        max: u32,
        lp: u32,
    },
    /// a lookaround starts; `next` is the instruction after its end
    Look {
        neg: bool,
        next: u32,
    },
    LookEnd,
    Match,
}

/// A compiled `RegExp` (flags `gu`, or `giu` with `icase`).
#[derive(Debug, Clone)]
pub struct Regex {
    prog: Vec<Inst>,
    classes: Vec<ClassSet>,
    ncap: u32,
    nregs: u32,
    npregs: u32,
    icase: bool,
    /// the literal runs every match contains (`required_runs`)
    required: Vec<Vec<u32>>,
    /// the pattern starts with an unbounded `[^]*` / `[^]*?` (`lead_any_star`)
    lead_star: bool,
}

/// A match: `[start, end)` in UTF-16 units and the captures (`None` = undefined).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Match {
    pub start: u32,
    pub end: u32,
    pub groups: Vec<Option<(u32, u32)>>,
}

struct Compiler<'a> {
    nodes: &'a [Node],
    /// `(min, max)` width of every node in code points (`None` = unbounded)
    widths: Vec<(u64, Option<u64>)>,
    prog: Vec<Inst>,
    nregs: u32,
    npregs: u32,
    icase: bool,
    class_is_empty: Vec<bool>,
}

/// The widths of every node, bottom-up (the parser creates a node after its children).
fn widths(nodes: &[Node]) -> Vec<(u64, Option<u64>)> {
    let mut w: Vec<(u64, Option<u64>)> = Vec::with_capacity(nodes.len());
    for n in nodes {
        let at = |w: &Vec<(u64, Option<u64>)>, i: u32| w.get(i as usize).copied().unwrap_or((0, Some(0)));
        let v = match n {
            Node::Char(_) | Node::Class(..) => (1, Some(1)),
            Node::Seq(v) => v.iter().fold((0u64, Some(0u64)), |(a, b), &t| {
                let (x, y) = at(&w, t);
                (a.saturating_add(x), b.zip(y).map(|(b, y)| b.saturating_add(y)))
            }),
            Node::Alt(v) => {
                let mut out: Option<(u64, Option<u64>)> = None;
                for &t in v {
                    let (x, y) = at(&w, t);
                    out = Some(match out {
                        None => (x, y),
                        Some((a, b)) => (a.min(x), b.zip(y).map(|(b, y)| b.max(y))),
                    });
                }
                out.unwrap_or((0, Some(0)))
            }
            Node::Group(_, b, ..) => at(&w, *b),
            Node::Look(..) => (0, Some(0)),
            Node::Repeat(b, min, max, ..) => {
                let (x, y) = at(&w, *b);
                let hi =
                    if *max == INFINITY { if y == Some(0) { Some(0) } else { None } } else { y.map(|y| y.saturating_mul(u64::from(*max))) };
                (x.saturating_mul(u64::from(*min)), hi)
            }
        };
        w.push(v);
    }
    w
}

/// The compiler's work list (iterative: no nesting depth can overflow the call stack).
enum Task {
    Node(u32, bool),
    Emit(Inst),
    /// the end of a lookaround: `LookEnd`, then the `Look` at `at` learns where to continue
    LookEnd {
        at: u32,
        neg: bool,
    },
    /// the end of a loop body: `RepEnd`, then the `RepLoop` at `lp` learns its exit
    RepEnd {
        lp: u32,
        reg: u32,
        preg: u32,
        min: u32,
        max: u32,
        greedy: bool,
        body: u32,
    },
    /// before branch `k` of an alternation (the jumps of the finished branches, the open split)
    Alt {
        alts: std::rc::Rc<[u32]>,
        k: usize,
        split: Option<u32>,
        jumps: Vec<u32>,
        back: bool,
    },
}

impl Compiler<'_> {
    fn pc(&self) -> u32 {
        idx(self.prog.len())
    }

    fn emit(&mut self, i: Inst) -> u32 {
        self.prog.push(i);
        idx(self.prog.len() - 1)
    }

    fn patch(&mut self, at: u32, i: Inst) {
        if let Some(slot) = self.prog.get_mut(at as usize) {
            *slot = i;
        }
    }

    fn char_inst(&self, cp: u32, back: bool) -> Inst {
        match self.icase.then(|| fold_class(cp)).flatten() {
            Some(members) => {
                let mut cps = [NONE; 4];
                for (slot, &m) in cps.iter_mut().zip(members) {
                    *slot = m;
                }
                // the generated classes have at most four members (checked by a unit test)
                Inst::Fold { cps, n: u8::try_from(members.len().min(4)).unwrap_or(4), back }
            }
            None => Inst::Char { cp, back },
        }
    }

    fn compile(&mut self, root: u32) {
        let mut work: Vec<Task> = vec![Task::Node(root, false)];
        while let Some(t) = work.pop() {
            match t {
                Task::Emit(i) => {
                    self.emit(i);
                }
                Task::LookEnd { at, neg } => {
                    self.emit(Inst::LookEnd);
                    let next = self.pc();
                    self.patch(at, Inst::Look { neg, next });
                }
                Task::RepEnd { lp, reg, preg, min, max, greedy, body } => {
                    self.emit(Inst::RepEnd { reg, preg, min, max, lp });
                    let exit = self.pc();
                    self.patch(lp, Inst::RepLoop { reg, min, max, greedy, body, exit });
                }
                Task::Alt { alts, k, split, mut jumps, back } => {
                    if let Some(s) = split {
                        jumps.push(self.emit(Inst::Jmp(0)));
                        let y = self.pc();
                        self.patch(s, Inst::Split { x: s + 1, y });
                    }
                    match alts.get(k) {
                        None => {
                            let end = self.pc();
                            for j in jumps {
                                self.patch(j, Inst::Jmp(end));
                            }
                        }
                        Some(&a) if k + 1 == alts.len() => {
                            work.push(Task::Alt { alts: std::rc::Rc::clone(&alts), k: k + 1, split: None, jumps, back });
                            work.push(Task::Node(a, back));
                        }
                        Some(&a) => {
                            let s = self.emit(Inst::Split { x: 0, y: 0 });
                            work.push(Task::Alt { alts: std::rc::Rc::clone(&alts), k: k + 1, split: Some(s), jumps, back });
                            work.push(Task::Node(a, back));
                        }
                    }
                }
                Task::Node(n, back) => self.node(n, back, &mut work),
            }
        }
    }

    fn node(&mut self, n: u32, back: bool, work: &mut Vec<Task>) {
        let Some(node) = self.nodes.get(n as usize) else { return };
        match node {
            Node::Char(cp) => {
                let i = self.char_inst(*cp, back);
                self.emit(i);
            }
            Node::Class(id, neg) => {
                let empty = self.class_is_empty.get(*id as usize).copied().unwrap_or(false);
                let i = if *neg && empty { Inst::Any { back } } else { Inst::Class { id: *id, neg: *neg, back } };
                self.emit(i);
            }
            Node::Seq(terms) => {
                // the work list is a stack: push the term to run first last
                if back {
                    work.extend(terms.iter().map(|&t| Task::Node(t, back)));
                } else {
                    work.extend(terms.iter().rev().map(|&t| Task::Node(t, back)));
                }
            }
            Node::Alt(alts) => {
                work.push(Task::Alt { alts: alts.as_slice().into(), k: 0, split: None, jumps: Vec::new(), back });
            }
            Node::Group(g, body, ..) => {
                let (g, body) = (*g, *body);
                if g == 0 {
                    work.push(Task::Node(body, back));
                } else {
                    let (open, shut) = if back { (2 * g + 1, 2 * g) } else { (2 * g, 2 * g + 1) };
                    self.emit(Inst::Save(open));
                    work.push(Task::Emit(Inst::Save(shut)));
                    work.push(Task::Node(body, back));
                }
            }
            Node::Look(ahead, neg, body, ..) => {
                let (ahead, neg, body) = (*ahead, *neg, *body);
                let at = self.emit(Inst::Look { neg, next: 0 });
                work.push(Task::LookEnd { at, neg });
                work.push(Task::Node(body, !ahead));
            }
            Node::Repeat(body, min, max, greedy, lo, hi) => {
                let (body, min, max, greedy, lo, hi) = (*body, *min, *max, *greedy, *lo, *hi);
                if max == 0 {
                    return;
                }
                let (wmin, wmax) = self.widths.get(body as usize).copied().unwrap_or((0, Some(0)));
                if wmax == Some(0) {
                    // a body that only matches empty: once when required, else never (module doc)
                    if min >= 1 {
                        if hi > lo {
                            self.emit(Inst::Clear { lo, hi });
                        }
                        work.push(Task::Node(body, back));
                    }
                    return;
                }
                let reg = self.nregs;
                self.nregs += 1;
                let preg = if wmin == 0 {
                    self.npregs += 1;
                    self.npregs - 1
                } else {
                    NONE
                };
                self.emit(Inst::RepInit(reg));
                let lp = self.emit(Inst::RepLoop { reg, min, max, greedy, body: 0, exit: 0 });
                let b = self.pc();
                if preg != NONE {
                    self.emit(Inst::RepPos(preg));
                }
                if hi > lo {
                    self.emit(Inst::Clear { lo, hi });
                }
                work.push(Task::RepEnd { lp, reg, preg, min, max, greedy, body: b });
                work.push(Task::Node(body, back));
            }
        }
    }
}

/// The literal runs every match must contain, in order of the pattern (outside lookarounds,
/// alternations and optional repeats): a match starting at `p` needs each run at or after `p`,
/// so the scan stops past the last start of the rarest one (a prune: it changes no result).
fn required_runs(nodes: &[Node], root: u32) -> Vec<Vec<u32>> {
    let mut runs: Vec<Vec<u32>> = Vec::new();
    let mut cur: Vec<u32> = Vec::new();
    // a node, or `None`: the current run ends here
    let mut work: Vec<Option<u32>> = vec![Some(root)];
    while let Some(w) = work.pop() {
        let node = w.and_then(|n| nodes.get(n as usize));
        match node {
            Some(Node::Char(cp)) => {
                cur.push(*cp);
                continue;
            }
            Some(Node::Seq(terms)) => {
                work.extend(terms.iter().rev().map(|&t| Some(t)));
                continue;
            }
            Some(Node::Group(_, body, ..)) => {
                work.push(Some(*body));
                continue;
            }
            Some(Node::Repeat(body, min, ..)) if *min >= 1 => {
                // the body's literals are required, but not next to what follows or precedes
                work.push(None);
                work.push(Some(*body));
            }
            _ => {}
        }
        if !cur.is_empty() {
            runs.push(std::mem::take(&mut cur));
        }
    }
    if !cur.is_empty() {
        runs.push(cur);
    }
    runs
}

/// The pattern starts (through first terms and groups) with `[^]*` or `[^]*?`: then a failed attempt
/// at `p` means every later start fails too (the rest of the pattern meets every position the later
/// attempts would, and nothing in it depends on where the match started: no backreferences), so the
/// scan stops (a prune: it changes no result).
fn lead_any_star(nodes: &[Node], classes: &[ClassSet], root: u32) -> bool {
    let mut n = root;
    loop {
        match nodes.get(n as usize) {
            Some(Node::Seq(terms)) => match terms.first() {
                Some(&t) => n = t,
                None => return false,
            },
            Some(Node::Group(_, body, ..)) => n = *body,
            Some(Node::Repeat(body, 0, INFINITY, ..)) => {
                return matches!(nodes.get(*body as usize), Some(Node::Class(id, true)) if classes.get(*id as usize).is_some_and(|c| c.ranges.is_empty() && c.props == 0));
            }
            _ => return false,
        }
    }
}

impl Regex {
    /// Compiles a source (UTF-16, `u` mode); `icase` is the `i` flag.
    pub fn new(src: &[u16], icase: bool) -> Result<Regex, RegexError> {
        let mut cps = Vec::with_capacity(src.len());
        let mut i = 0;
        while let Some((cp, w)) = uo_text::utf16::decode_at(src, i) {
            cps.push(cp);
            i += w;
        }
        let p = Parser { s: &cps, i: 0, nodes: Vec::new(), classes: Vec::new(), ncap: 0 }.parse()?;
        let empty: Vec<bool> = p.classes.iter().map(|s| s.ranges.is_empty() && s.props == 0).collect();
        let mut comp =
            Compiler { nodes: &p.nodes, widths: widths(&p.nodes), prog: Vec::new(), nregs: 0, npregs: 0, icase, class_is_empty: empty };
        comp.compile(p.root);
        comp.emit(Inst::Match);
        let required = required_runs(&p.nodes, p.root);
        let lead_star = lead_any_star(&p.nodes, &p.classes, p.root);
        Ok(Regex { prog: comp.prog, classes: p.classes, ncap: p.ncap, nregs: comp.nregs, npregs: comp.npregs, icase, required, lead_star })
    }

    /// The same pattern without the scan prunes (the parity evidence of F9-A1: without them the
    /// search runs out of its budget like V8 runs past the capture's timeout).
    #[doc(hidden)]
    #[must_use]
    pub fn without_prunes(&self) -> Regex {
        Regex { required: Vec::new(), lead_star: false, ..self.clone() }
    }

    /// Two code points match under this pattern's flags.
    fn same(&self, a: u32, b: u32) -> bool {
        a == b || (self.icase && fold_class(a).is_some_and(|m| m.contains(&b)))
    }

    /// The last start in `text` where a match can begin (`None`: none can), from the required
    /// runs (see [`required_runs`]); `Some(len)` when the pattern requires no literal.
    pub fn scan_limit(&self, text: &[u16], budget: &mut Budget) -> Result<Option<usize>, BudgetExceeded> {
        let mut limit = text.len();
        if self.required.is_empty() {
            return Ok(Some(limit));
        }
        // the text as code points, with their unit offsets
        let mut cps: Vec<(u32, usize)> = Vec::with_capacity(text.len());
        let mut i = 0;
        while let Some((cp, w)) = uo_text::utf16::decode_at(text, i) {
            cps.push((cp, i));
            i += w;
        }
        budget.charge(cps.len() as u64)?;
        for run in &self.required {
            let n = run.len();
            let mut found = None;
            let mut k = cps.len();
            while k >= n {
                k -= 1;
                let start = k + 1 - n;
                budget.charge(1)?;
                if run.iter().zip(cps.get(start..=k).unwrap_or(&[])).all(|(&r, &(c, _))| self.same(r, c)) {
                    found = cps.get(start).map(|c| c.1);
                    break;
                }
            }
            match found {
                Some(at) => limit = limit.min(at),
                None => return Ok(None),
            }
        }
        Ok(Some(limit))
    }

    /// The number of capture groups.
    #[must_use]
    pub fn captures(&self) -> u32 {
        self.ncap
    }

    fn class_has(&self, id: u32, cp: u32) -> bool {
        let Some(set) = self.classes.get(id as usize) else { return false };
        if !self.icase {
            return set.has(cp);
        }
        match fold_class(cp) {
            Some(members) => members.iter().any(|&m| set.has(m)),
            None => set.has(cp),
        }
    }

    /// `re.lastIndex = last; re.exec(text)`: the first match at or after `last` (stepped back
    /// to the start of a surrogate pair, as V8 does in `u` mode), or `None` (`last` past the end
    /// included).
    /// `limit` is [`Regex::scan_limit`] of the same text.
    pub fn exec(
        &self,
        text: &[u16],
        last: usize,
        limit: Option<usize>,
        budget: &mut Budget,
        vm: &mut Vm,
    ) -> Result<Option<Match>, BudgetExceeded> {
        let Some(limit) = limit else { return Ok(None) };
        if last > text.len() {
            return Ok(None);
        }
        let mut p = last;
        if p > 0 && text.get(p).is_some_and(|&u| is_low(u)) && text.get(p - 1).is_some_and(|&u| is_high(u)) {
            p -= 1;
        }
        vm.prepare(self);
        loop {
            if p > limit {
                return Ok(None);
            }
            budget.step()?;
            if vm.run(self, text, idx(p), budget)? {
                let groups = (1..=self.ncap)
                    .map(|g| {
                        let a = vm.slots.get(2 * g as usize).copied().unwrap_or(NONE);
                        let b = vm.slots.get(2 * g as usize + 1).copied().unwrap_or(NONE);
                        (a != NONE && b != NONE).then_some((a, b))
                    })
                    .collect();
                let end = vm.end;
                vm.unwind();
                return Ok(Some(Match { start: idx(p), end, groups }));
            }
            if p >= text.len() || self.lead_star {
                return Ok(None);
            }
            p += read(text, idx(p), false).map_or(1, |(_, np)| np as usize - p);
        }
    }
}

#[inline]
fn is_high(u: u16) -> bool {
    (0xD800..0xDC00).contains(&u)
}

#[inline]
fn is_low(u: u16) -> bool {
    (0xDC00..0xE000).contains(&u)
}

/// The code point at `pos` in a direction and the position after it (`u` mode).
#[inline]
fn read(text: &[u16], pos: u32, back: bool) -> Option<(u32, u32)> {
    let p = pos as usize;
    if back {
        let u = *text.get(p.checked_sub(1)?)?;
        if is_low(u)
            && let Some(&h) = p.checked_sub(2).and_then(|q| text.get(q))
            && is_high(h)
        {
            return Some((0x10000 + ((u32::from(h) - 0xD800) << 10) + (u32::from(u) - 0xDC00), pos - 2));
        }
        return Some((u32::from(u), pos - 1));
    }
    let u = *text.get(p)?;
    if is_high(u)
        && let Some(&l) = text.get(p + 1)
        && is_low(l)
    {
        return Some((0x10000 + ((u32::from(u) - 0xD800) << 10) + (u32::from(l) - 0xDC00), pos + 2));
    }
    Some((u32::from(u), pos + 1))
}

#[derive(Debug, Clone, Copy)]
enum Bt {
    Choice { pc: u32, pos: u32 },
    Slot { i: u32, old: u32 },
    Reg { i: u32, old: u32 },
    PReg { i: u32, old: u32 },
    Barrier,
}

#[derive(Debug, Clone, Copy)]
struct LookFrame {
    pos: u32,
    next: u32,
    neg: bool,
    bar: usize,
}

/// The matcher's scratch state (kept between the `exec` calls of a command).
#[derive(Debug, Default)]
pub struct Vm {
    slots: Vec<u32>,
    regs: Vec<u32>,
    pregs: Vec<u32>,
    stack: Vec<Bt>,
    frames: Vec<LookFrame>,
    end: u32,
}

impl Vm {
    fn prepare(&mut self, re: &Regex) {
        let ns = 2 * (re.ncap as usize + 1);
        self.slots.clear();
        self.slots.resize(ns, NONE);
        self.regs.clear();
        self.regs.resize(re.nregs as usize, 0);
        self.pregs.clear();
        self.pregs.resize(re.npregs as usize, NONE);
        self.stack.clear();
        self.frames.clear();
    }

    /// Back to the clean state after a match (every write went through the trail).
    fn unwind(&mut self) {
        while let Some(e) = self.stack.pop() {
            self.undo(e);
        }
        self.frames.clear();
    }

    fn undo(&mut self, e: Bt) {
        match e {
            Bt::Slot { i, old } => {
                if let Some(s) = self.slots.get_mut(i as usize) {
                    *s = old;
                }
            }
            Bt::Reg { i, old } => {
                if let Some(s) = self.regs.get_mut(i as usize) {
                    *s = old;
                }
            }
            Bt::PReg { i, old } => {
                if let Some(s) = self.pregs.get_mut(i as usize) {
                    *s = old;
                }
            }
            Bt::Choice { .. } | Bt::Barrier => {}
        }
    }

    #[inline]
    fn push(&mut self, e: Bt) -> Result<(), BudgetExceeded> {
        if self.stack.len() >= MAX_STACK {
            return Err(BudgetExceeded);
        }
        self.stack.push(e);
        Ok(())
    }

    fn set_slot(&mut self, i: u32, v: u32) -> Result<(), BudgetExceeded> {
        let old = self.slots.get(i as usize).copied().unwrap_or(NONE);
        if old != v {
            self.push(Bt::Slot { i, old })?;
            if let Some(s) = self.slots.get_mut(i as usize) {
                *s = v;
            }
        }
        Ok(())
    }

    fn set_reg(&mut self, i: u32, v: u32) -> Result<(), BudgetExceeded> {
        let old = self.regs.get(i as usize).copied().unwrap_or(0);
        if old != v {
            self.push(Bt::Reg { i, old })?;
            if let Some(s) = self.regs.get_mut(i as usize) {
                *s = v;
            }
        }
        Ok(())
    }

    fn set_preg(&mut self, i: u32, v: u32) -> Result<(), BudgetExceeded> {
        let old = self.pregs.get(i as usize).copied().unwrap_or(NONE);
        if old != v {
            self.push(Bt::PReg { i, old })?;
            if let Some(s) = self.pregs.get_mut(i as usize) {
                *s = v;
            }
        }
        Ok(())
    }

    /// Pops to the last choice point (`false`: none left, no match from this start).
    fn backtrack(&mut self, pc: &mut u32, pos: &mut u32) -> bool {
        while let Some(e) = self.stack.pop() {
            match e {
                Bt::Choice { pc: p, pos: q } => {
                    *pc = p;
                    *pos = q;
                    return true;
                }
                Bt::Barrier => {
                    // the body of a lookaround failed
                    if let Some(f) = self.frames.pop()
                        && f.neg
                    {
                        *pc = f.next;
                        *pos = f.pos;
                        return true;
                    }
                }
                other => self.undo(other),
            }
        }
        false
    }

    /// One attempt at `start`: `true` with the captures in `slots` and the end in `end`.
    fn run(&mut self, re: &Regex, text: &[u16], start: u32, budget: &mut Budget) -> Result<bool, BudgetExceeded> {
        let mut pc: u32 = 0;
        let mut pos = start;
        loop {
            budget.step()?;
            let Some(&inst) = re.prog.get(pc as usize) else { return Ok(false) };
            let ok = match inst {
                Inst::Char { cp, back } => match read(text, pos, back) {
                    Some((ch, np)) if ch == cp => {
                        pos = np;
                        pc += 1;
                        true
                    }
                    _ => false,
                },
                Inst::Fold { cps, n, back } => match read(text, pos, back) {
                    Some((ch, np)) if cps.iter().take(usize::from(n)).any(|&m| m == ch) => {
                        pos = np;
                        pc += 1;
                        true
                    }
                    _ => false,
                },
                Inst::Class { id, neg, back } => match read(text, pos, back) {
                    Some((ch, np)) if re.class_has(id, ch) != neg => {
                        pos = np;
                        pc += 1;
                        true
                    }
                    _ => false,
                },
                Inst::Any { back } => match read(text, pos, back) {
                    Some((_, np)) => {
                        pos = np;
                        pc += 1;
                        true
                    }
                    None => false,
                },
                Inst::Split { x, y } => {
                    self.push(Bt::Choice { pc: y, pos })?;
                    pc = x;
                    true
                }
                Inst::Jmp(t) => {
                    pc = t;
                    true
                }
                Inst::Save(s) => {
                    self.set_slot(s, pos)?;
                    pc += 1;
                    true
                }
                Inst::Clear { lo, hi } => {
                    budget.charge(u64::from(hi - lo))?;
                    for g in lo..hi {
                        self.set_slot(2 * g, NONE)?;
                        self.set_slot(2 * g + 1, NONE)?;
                    }
                    pc += 1;
                    true
                }
                Inst::RepInit(r) => {
                    self.set_reg(r, 0)?;
                    pc += 1;
                    true
                }
                Inst::RepLoop { reg, min, max, greedy, body, exit } => {
                    let count = self.regs.get(reg as usize).copied().unwrap_or(0);
                    if count < min {
                        pc = body;
                    } else if max != INFINITY && count >= max {
                        pc = exit;
                    } else if greedy {
                        self.push(Bt::Choice { pc: exit, pos })?;
                        pc = body;
                    } else {
                        self.push(Bt::Choice { pc: body, pos })?;
                        pc = exit;
                    }
                    true
                }
                Inst::RepPos(p) => {
                    self.set_preg(p, pos)?;
                    pc += 1;
                    true
                }
                Inst::RepEnd { reg, preg, min, max, lp } => {
                    let count = self.regs.get(reg as usize).copied().unwrap_or(0);
                    if preg != NONE && count >= min && self.pregs.get(preg as usize).copied() == Some(pos) {
                        false
                    } else {
                        let next = if count < min || max != INFINITY { count.saturating_add(1) } else { count };
                        self.set_reg(reg, next)?;
                        pc = lp;
                        true
                    }
                }
                Inst::Look { neg, next } => {
                    let bar = self.stack.len();
                    self.push(Bt::Barrier)?;
                    self.frames.push(LookFrame { pos, next, neg, bar });
                    pc += 1;
                    true
                }
                Inst::LookEnd => match self.frames.pop() {
                    Some(f) if f.neg => {
                        // the body matched: undo what it did, the lookaround fails
                        while self.stack.len() > f.bar {
                            if let Some(e) = self.stack.pop() {
                                self.undo(e);
                            }
                        }
                        false
                    }
                    Some(f) => {
                        // atomic: its choice points go, its writes stay undoable
                        let tail: Vec<Bt> = self.stack.drain(f.bar..).filter(|e| !matches!(e, Bt::Choice { .. } | Bt::Barrier)).collect();
                        self.stack.extend(tail);
                        pos = f.pos;
                        pc = f.next;
                        true
                    }
                    None => false,
                },
                Inst::Match => {
                    self.end = pos;
                    return Ok(true);
                }
            };
            if !ok && !self.backtrack(&mut pc, &mut pos) {
                return Ok(false);
            }
        }
    }
}
