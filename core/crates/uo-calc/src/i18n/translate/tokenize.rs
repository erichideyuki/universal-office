//! `tokenize`, `rewriteOperators`, `scopeParams`, `parseStructured`, `render` and `renderStructured` of
//! `translate.ts`, over spans of [`Translator::src`].

use super::*;

const Q: u16 = b'\'' as u16;
const DQ: u16 = b'"' as u16;

impl Translator {
    fn push_items(&mut self, v: &[SItem]) -> (u32, u32) {
        let s = self.items.len() as u32;
        self.items.extend_from_slice(v);
        (s, v.len() as u32)
    }

    /// `kw(s)`: the keyword of a folded, trimmed text.
    fn kw(&mut self, d: &Dialect, s: Sp) -> Option<Kw> {
        let t = uo_text::space::trim(&self.src[s.0 as usize..s.1 as usize]);
        upper_utf8_into(JsStr::Utf16(t), &mut self.fold);
        d.lang.kw_in.iter().rev().find(|(k, _)| *k == self.fold).map(|(_, v)| *v)
    }

    /// `parseStructured(inner, src)` of the span `[a, b)`.
    fn parse_structured(&mut self, a: u32, b: u32, d: &Dialect) -> SBody {
        let t = &self.src[a as usize..b as usize];
        if uo_text::space::trim(t).is_empty() {
            return SBody { form: Form::Empty, this_row: false, items: (self.items.len() as u32, 0) };
        }
        if t[0] == u16::from(b'@') {
            let rest = (a + 1, b);
            if rest.0 == rest.1 {
                return SBody { form: Form::Simple, this_row: true, items: (self.items.len() as u32, 0) };
            }
            if self.src[rest.0 as usize] == u16::from(b'[')
                && let Some(g) = self.groups(rest.0, rest.1, d)
            {
                return SBody { form: Form::Groups, this_row: true, items: g };
            }
            let it = self.push_items(&[SItem::Col(rest)]);
            return SBody { form: Form::Simple, this_row: true, items: it };
        }
        if t[0] == u16::from(b'[') {
            if let Some((gs, gn)) = self.groups(a, b, d) {
                let this_row = self.items[gs as usize..(gs + gn) as usize].iter().any(|x| matches!(x, SItem::Kw(Kw::ThisRow)));
                if this_row {
                    // `g.filter(x => !(kw === thisRow))`, in place at the end of the item list
                    let mut w = gs as usize;
                    for r in gs as usize..(gs + gn) as usize {
                        let x = self.items[r];
                        if !matches!(x, SItem::Kw(Kw::ThisRow)) {
                            self.items[w] = x;
                            w += 1;
                        }
                    }
                    self.items.truncate(w);
                    return SBody { form: Form::Groups, this_row: true, items: (gs, w as u32 - gs) };
                }
                return SBody { form: Form::Groups, this_row: false, items: (gs, gn) };
            }
            let it = self.push_items(&[SItem::Raw((a, b))]);
            return SBody { form: Form::Simple, this_row: false, items: it };
        }
        if t[0] == u16::from(b'#') {
            let k = self.kw(d, (a, b));
            if k == Some(Kw::ThisRow) {
                return SBody { form: Form::Simple, this_row: true, items: (self.items.len() as u32, 0) };
            }
            let it = self.push_items(&[k.map_or(SItem::Raw((a, b)), SItem::Kw)]);
            return SBody { form: Form::Simple, this_row: false, items: it };
        }
        let it = self.push_items(&[SItem::Col((a, b))]);
        SBody { form: Form::Simple, this_row: false, items: it }
    }

    /// `groups(s)` of `parseStructured`: `[a]`, `[a]:[b]` items separated by the list separator; `None` = not that
    /// shape (the items pushed meanwhile are dropped).
    fn groups(&mut self, a: u32, b: u32, d: &Dialect) -> Option<(u32, u32)> {
        let start = self.items.len();
        let (a, b) = (a as usize, b as usize);
        let mut i = a;
        let sp = u16::from(b' ');
        let fail = |me: &mut Self| {
            me.items.truncate(start);
            None
        };
        // `one()`: spaces, `[`, the matching `]`
        let one = |src: &[u16], i: &mut usize| -> Option<Sp> {
            while *i < b && src[*i] == sp {
                *i += 1;
            }
            if *i >= b || src[*i] != u16::from(b'[') {
                return None;
            }
            // `matchBracket(s, i)` within the slice
            let e = match_bracket(&src[..b], *i)?;
            let v = ((*i + 1) as u32, e as u32);
            *i = e + 1;
            Some(v)
        };
        while i < b {
            let Some(x) = one(&self.src, &mut i) else { return fail(self) };
            while i < b && self.src[i] == sp {
                i += 1;
            }
            if i < b && self.src[i] == u16::from(b':') {
                i += 1;
                let Some(y) = one(&self.src, &mut i) else { return fail(self) };
                self.items.push(SItem::Range(x, y));
            } else {
                let hash = x.1 > x.0 && self.src[x.0 as usize] == u16::from(b'#');
                let k = if hash { self.kw(d, x) } else { None };
                self.items.push(match k {
                    Some(k) => SItem::Kw(k),
                    None if hash => SItem::Raw(x),
                    None => SItem::Col(x),
                });
            }
            while i < b && self.src[i] == sp {
                i += 1;
            }
            if i >= b {
                break;
            }
            if starts(&self.src[..b], i, &d.list) {
                i += d.list.len();
                continue;
            }
            return fail(self);
        }
        Some((start as u32, (self.items.len() - start) as u32))
    }

    /// `tokenize(text, src, names)`.
    #[allow(clippy::too_many_lines)]
    pub(super) fn tokenize(&mut self, d: &Dialect, names: Option<&NameSet>) {
        let n = self.src.len();
        let mut i = 0usize;
        let mut in_array = false;
        let names = names.filter(|s| !s.is_empty());
        while i < n {
            let c = self.src[i];
            if is_ws(c) {
                let mut j = i + 1;
                while j < n && is_ws(self.src[j]) {
                    j += 1;
                }
                self.toks.push(Tok::Ws((i as u32, j as u32)));
                i = j;
                continue;
            }
            if c == DQ {
                let mut j = i + 1;
                while j < n {
                    if self.src[j] == DQ {
                        if self.src.get(j + 1) == Some(&DQ) {
                            j += 1;
                        } else {
                            break;
                        }
                    }
                    j += 1;
                }
                self.toks.push(Tok::Text(Txt::Span((i as u32, (j + 1).min(n) as u32))));
                i = j + 1;
                continue;
            }
            if c == Q {
                let mut j = i + 1;
                while j < n {
                    if self.src[j] == Q {
                        if self.src.get(j + 1) == Some(&Q) {
                            j += 1;
                        } else {
                            break;
                        }
                    }
                    j += 1;
                }
                if self.src.get(j + 1) == Some(&u16::from(b'!')) {
                    self.toks.push(Tok::Text(Txt::Span((i as u32, (j + 2) as u32))));
                    i = j + 2;
                    continue;
                }
                // `q.slice(1, -1)` of `q = text.slice(i, j + 1)`
                let qend = (j + 1).min(n);
                let inner = ((i + 1) as u32, (qend.saturating_sub(1)).max(i + 1) as u32);
                self.toks.push(Tok::Name { s: NameS::Span(inner, true), pre: Pre::None, quoted: true });
                i = j + 1;
                continue;
            }
            if c == u16::from(b'{') {
                in_array = true;
                self.toks.push(Tok::Text(Txt::Span((i as u32, i as u32 + 1))));
                i += 1;
                continue;
            }
            if c == u16::from(b'}') {
                in_array = false;
                self.toks.push(Tok::Text(Txt::Span((i as u32, i as u32 + 1))));
                i += 1;
                continue;
            }
            if c == u16::from(b'(') {
                self.toks.push(Tok::Open);
                i += 1;
                continue;
            }
            if c == u16::from(b')') {
                self.toks.push(Tok::Close);
                i += 1;
                continue;
            }
            // numbers (before the separators: the decimal may be ",")
            let dl = d.decimal.len();
            if is_digit(c) || (starts_or_empty(&self.src, i, &d.decimal) && self.src.get(i + dl).is_some_and(|&u| is_digit(u))) {
                let mut j = i;
                while j < n && is_digit(self.src[j]) {
                    j += 1;
                }
                let int = (i as u32, j as u32);
                let mut frac = None;
                if starts_or_empty(&self.src, j, &d.decimal) && !(in_array && d.decimal == d.acol) {
                    let mut k = j + dl;
                    while k < n && is_digit(self.src[k]) {
                        k += 1;
                    }
                    frac = Some(((j + dl) as u32, k as u32));
                    j = k;
                }
                // `/^[eE][+-]?[0-9]+/`
                let e0 = j;
                if j < n && (self.src[j] == u16::from(b'e') || self.src[j] == u16::from(b'E')) {
                    let mut k = j + 1;
                    if k < n && (self.src[k] == u16::from(b'+') || self.src[k] == u16::from(b'-')) {
                        k += 1;
                    }
                    let k0 = k;
                    while k < n && is_digit(self.src[k]) {
                        k += 1;
                    }
                    if k > k0 {
                        j = k;
                    }
                }
                self.toks.push(Tok::Num { int, frac, exp: (e0 as u32, j as u32) });
                i = j;
                continue;
            }
            if in_array {
                if starts(&self.src, i, &d.acol) {
                    self.toks.push(Tok::Sep(SepK::Col));
                    i += d.acol.len();
                    continue;
                }
                if starts(&self.src, i, &d.arow) {
                    self.toks.push(Tok::Sep(SepK::Row));
                    i += d.arow.len();
                    continue;
                }
            } else if starts(&self.src, i, &d.list) {
                self.toks.push(Tok::Sep(SepK::List));
                i += d.list.len();
                continue;
            }
            // R1C1
            if d.r1c1
                && id_start(c)
                && let Some((len, row, col)) = r1c1_at(&self.src, i, d)
            {
                let after = self.src.get(i + len).copied();
                let after_ok = match after {
                    None => true,
                    Some(a) => {
                        (!id_char(a) || (a == u16::from(b'.') && self.src.get(i + len + 1) == Some(&u16::from(b':'))))
                            && a != u16::from(b'(')
                            && a != u16::from(b'!')
                    }
                };
                if after_ok {
                    self.toks.push(Tok::R1c1 { row, col });
                    i += len;
                    continue;
                }
            }
            if id_start(c) && !in_array {
                let mut j = i + 1;
                while j < n && id_char(self.src[j]) {
                    j += 1;
                }
                let next = self.src.get(j).copied();
                if next == Some(u16::from(b'(')) {
                    self.fn_token(i, j, d);
                    i = j;
                    continue;
                }
                if next == Some(u16::from(b'!')) {
                    self.toks.push(Tok::Text(Txt::Span((i as u32, (j + 1) as u32))));
                    i = j + 1;
                    continue;
                }
                if next == Some(u16::from(b'['))
                    && let Some(e) = self.bracket(j).filter(|&e| e > 0)
                {
                    let body = self.parse_structured((j + 1) as u32, e as u32, d);
                    self.toks.push(Tok::Sref { table: (i as u32, j as u32), body });
                    i = e + 1;
                    continue;
                }
                let prev_ch = if i > 0 { self.src.get(i - 1).copied() } else { None };
                let in_range = next == Some(u16::from(b':')) || prev_ch == Some(u16::from(b':'));
                let word = &self.src[i..j];
                if starts_ascii_ci(word, "_xlpm.") {
                    self.toks.push(Tok::Name { s: NameS::Span(((i + 6) as u32, j as u32), false), pre: Pre::Pm, quoted: false });
                    i = j;
                    continue;
                }
                if starts_ascii_ci(word, "_xlop.") {
                    self.toks.push(Tok::Name { s: NameS::Span(((i + 6) as u32, j as u32), false), pre: Pre::Op, quoted: false });
                    i = j;
                    continue;
                }
                if starts_ascii_ci(word, "_xleta.") {
                    let t = match self.catalog_canon_at(i + 7, j) {
                        Some(canon) => Tok::Eta { canon, raw: ((i + 7) as u32, j as u32) },
                        None => Tok::Name { s: NameS::Span((i as u32, j as u32), false), pre: Pre::None, quoted: false },
                    };
                    self.toks.push(t);
                    i = j;
                    continue;
                }
                upper_utf8_into(JsStr::Utf16(word), &mut self.fold);
                if self.fold == d.bool_f[0] || self.fold == d.bool_f[1] {
                    let v = self.fold == d.bool_f[0];
                    self.toks.push(Tok::Bool(v));
                    i = j;
                    continue;
                }
                // an absolute R1C1 typed in A1 mode (L1C1 → $A$1, measured in pt-BR)
                if !d.r1c1
                    && !in_range
                    && let Some((rd, cv)) = abs_rc(word, d)
                {
                    let rv = digits_value(rd);
                    if (1..=1_048_576).contains(&rv) && (1..=16_384).contains(&cv) {
                        let off = (rd.as_ptr() as usize - word.as_ptr() as usize) / 2;
                        let rs = (i + off) as u32;
                        self.toks.push(Tok::Name { s: NameS::AbsRc(cv as u32, (rs, rs + rd.len() as u32)), pre: Pre::None, quoted: false });
                        i = j;
                        continue;
                    }
                }
                // a function as a value (eta: =GROUPBY(...;SOMA), =SUM)
                let prev_bang = matches!(self.toks.last(), Some(Tok::Text(t)) if self.txt_ends_bang(*t));
                let named = names.is_some_and(|s| s.contains(&self.fold));
                if !in_range && !is_a1_cell(word) && !prev_bang && !named {
                    let canon = if d.canonical { self.catalog_canon_at(i, j) } else { self.fn_lookup_at(d.lang, i, j).map(Canon::Static) };
                    if let Some(canon) = canon {
                        self.toks.push(Tok::Eta { canon, raw: (i as u32, j as u32) });
                        i = j;
                        continue;
                    }
                }
                self.toks.push(Tok::Name { s: NameS::Span((i as u32, j as u32), false), pre: Pre::None, quoted: false });
                i = j;
                continue;
            }
            if id_start(c) && in_array {
                // only booleans inside { }: letters only (the column separator may be "\" or ".")
                let mut j = i + 1;
                while j < n && mask(self.src[j]) & uo_text::props::LETTER != 0 {
                    j += 1;
                }
                upper_utf8_into(JsStr::Utf16(&self.src[i..j]), &mut self.fold);
                if self.fold == d.bool_f[0] || self.fold == d.bool_f[1] {
                    let v = self.fold == d.bool_f[0];
                    self.toks.push(Tok::Bool(v));
                } else {
                    self.toks.push(Tok::Text(Txt::Span((i as u32, j as u32))));
                }
                i = j;
                continue;
            }
            if c == u16::from(b'[')
                && let Some(e) = self.bracket(i).filter(|&e| e > 0)
            {
                let after = self.src.get(e + 1).copied();
                if after.is_some_and(|a| id_char(a) || a == Q || a == u16::from(b'!')) {
                    // an external reference: [1]Plan1!A1, [Pasta.xlsx]Plan1!A1
                    self.toks.push(Tok::Text(Txt::Span((i as u32, (e + 1) as u32))));
                } else {
                    let body = self.parse_structured((i + 1) as u32, e as u32, d);
                    self.toks.push(Tok::Sref { table: (i as u32, i as u32), body });
                }
                i = e + 1;
                continue;
            }
            if c == u16::from(b'#') {
                let ref_like = matches!(self.toks.last(), Some(Tok::Name { .. } | Tok::R1c1 { .. } | Tok::Sref { .. } | Tok::Close));
                if ref_like {
                    self.toks.push(Tok::Text(Txt::Lit("#")));
                    i += 1;
                    continue;
                }
                let head_end = (i + 24).min(n);
                upper_utf8_into(JsStr::Utf16(&self.src[i..head_end]), &mut self.fold);
                if let Some(&(name, code)) = d.lang.err_in.iter().find(|(name, _)| self.fold.starts_with(name)) {
                    self.toks.push(Tok::Err(code));
                    // `i += hit[0].length`: the folded name's length in UTF-16 units
                    i += name.encode_utf16().count();
                    continue;
                }
                self.toks.push(Tok::Text(Txt::Lit("#")));
                i += 1;
                continue;
            }
            self.toks.push(Tok::Text(Txt::Span((i as u32, i as u32 + 1))));
            i += 1;
        }
    }

    /// `matchBracket(text, i)` over the whole text, linear overall: the first scan that fails (an unbalanced `[`)
    /// matches every `[` it visits in one stack pass, so a run of unbalanced brackets is not quadratic. A memo hit
    /// is the TS answer: a `[` visited by that pass is scanned by `matchBracket` over the same positions.
    fn bracket(&mut self, i: usize) -> Option<usize> {
        const NOT_YET: u32 = u32::MAX;
        const NONE_: u32 = u32::MAX - 1;
        if let Some(&m) = self.bmemo.get(i)
            && m != NOT_YET
        {
            return (m != NONE_).then_some(m as usize);
        }
        let r = match_bracket(&self.src, i);
        if r.is_none() {
            let n = self.src.len();
            if self.bmemo.len() != n {
                self.bmemo.clear();
                self.bmemo.resize(n, NOT_YET);
            }
            self.stack.clear();
            let mut j = i;
            while j < n {
                let c = self.src[j];
                if c == Q {
                    j += 2;
                    continue;
                }
                if c == u16::from(b'[') {
                    self.stack.push(j as u32);
                } else if c == u16::from(b']')
                    && let Some(o) = self.stack.pop()
                {
                    self.bmemo[o as usize] = j as u32;
                }
                j += 1;
            }
            for &o in &self.stack {
                self.bmemo[o as usize] = NONE_;
            }
            self.stack.clear();
        }
        r
    }

    fn txt_ends_bang(&self, t: Txt) -> bool {
        match t {
            Txt::Span((a, b)) => b > a && self.src[b as usize - 1] == u16::from(b'!'),
            Txt::Lit(s) => s.ends_with('!'),
        }
    }

    /// The `fn` token of the word `[i, j)` (followed by `(`).
    fn fn_token(&mut self, i: usize, j: usize, d: &Dialect) {
        let word = &self.src[i..j];
        let pre_len = prefix_len(word);
        // `preU.includes('_xludf.')`
        let pre = &word[..pre_len];
        if let Some(k) = find_ascii_ci(pre, "_xludf.") {
            self.toks.push(Tok::Fn { canon: None, raw: ((i + k + 7) as u32, j as u32) });
            return;
        }
        let canon = if d.canonical {
            match self.catalog_canon_at(i, j) {
                Some(c) => Some(c),
                None => {
                    // `OPERATOR_FUNCTIONS[stripFilePrefix(word)]`
                    upper_utf8_into(JsStr::Utf16(&self.src[i..j]), &mut self.fold);
                    operator_name(strip_upper_prefix(&self.fold)).map(Canon::Static)
                }
            }
        } else {
            // Excel drops `_xlfn.` and looks the LOCAL name up (`_xlfn.PROCX` → XLOOKUP)
            let drop = ["_xlfn.", "_xlfn._xlws.", "_xlws."].iter().any(|p| pre.len() == p.len() && starts_ascii_ci(pre, p));
            self.fn_lookup_at(d.lang, if drop { i + pre_len } else { i }, j).map(Canon::Static)
        };
        self.toks.push(Tok::Fn { canon, raw: (i as u32, j as u32) });
    }
}

fn starts_or_empty(t: &[u16], i: usize, p: &[u16]) -> bool {
    p.is_empty() || (t.len() >= i + p.len() && t[i..i + p.len()] == *p)
}

/// The key of [`super::super::tables::OPERATOR_FUNCTIONS`] equal to `s`.
fn operator_name(s: &str) -> Option<&'static str> {
    super::super::tables::OPERATOR_FUNCTIONS.iter().find(|(k, _)| *k == s).map(|(k, _)| *k)
}

/// Length of `^((?:_xl[a-z]+\.)*)` (ASCII, case-insensitive).
fn prefix_len(w: &[u16]) -> usize {
    let mut p = 0;
    loop {
        let rest = &w[p..];
        if !starts_ascii_ci(rest, "_xl") {
            return p;
        }
        let mut k = 3;
        while k < rest.len() && rest[k] < 0x80 && (rest[k] as u8).is_ascii_alphabetic() {
            k += 1;
        }
        if k == 3 || rest.get(k) != Some(&u16::from(b'.')) {
            return p;
        }
        p += k + 1;
    }
}

fn find_ascii_ci(t: &[u16], p: &str) -> Option<usize> {
    (0..t.len()).find(|&k| starts_ascii_ci(&t[k..], p))
}

/// The R1C1 `RegExp` at `i`: `(len, row part, col part)`; `None` when it matched nothing.
fn r1c1_at(t: &[u16], i: usize, d: &Dialect) -> Option<(usize, Part, Part)> {
    let mut j = i;
    let row = letter_part(t, &mut j, &d.row, d);
    let col = letter_part(t, &mut j, &d.col, d);
    (j > i).then_some((j - i, row, col))
}

fn letter_part(t: &[u16], j: &mut usize, letter: &[u16], d: &Dialect) -> Part {
    if !ci_match(t, *j, letter) {
        return Part::Absent;
    }
    *j += letter.len();
    // `open([+-]?[0-9]+)close`
    if ci_match(t, *j, &d.open) {
        let mut k = *j + d.open.len();
        let s = k;
        if k < t.len() && (t[k] == u16::from(b'+') || t[k] == u16::from(b'-')) {
            k += 1;
        }
        let k0 = k;
        while k < t.len() && is_digit(t[k]) {
            k += 1;
        }
        if k > k0 && ci_match(t, k, &d.close) {
            let p = Part::Rel((s as u32, k as u32));
            *j = k + d.close.len();
            return p;
        }
    }
    let k0 = *j;
    let mut k = *j;
    while k < t.len() && is_digit(t[k]) {
        k += 1;
    }
    if k > k0 {
        *j = k;
        return Part::Abs((k0 as u32, k as u32));
    }
    Part::Bare
}

/// `^R([0-9]+)C([0-9]+)$` (case-insensitive) on a word: (row digits, column value).
fn abs_rc<'w>(w: &'w [u16], d: &Dialect) -> Option<(&'w [u16], u64)> {
    if !ci_match(w, 0, &d.row) {
        return None;
    }
    let mut k = d.row.len();
    let r0 = k;
    while k < w.len() && is_digit(w[k]) {
        k += 1;
    }
    if k == r0 || !ci_match(w, k, &d.col) {
        return None;
    }
    let rd = &w[r0..k];
    k += d.col.len();
    let c0 = k;
    while k < w.len() && is_digit(w[k]) {
        k += 1;
    }
    if k == c0 || k != w.len() {
        return None;
    }
    Some((rd, digits_value(&w[c0..k])))
}

impl Translator {
    /// `rewriteOperators(toks)` on an explicit stack: `_xlfn.SINGLE(x)` → `@x`, `ANCHORARRAY(A1)` → `A1#`,
    /// `_TRO_*(a:b)` → `a:.b` (the last depth-zero `:` gets the dots; none → the call stays as written).
    pub(super) fn rewrite_operators(&mut self) {
        let toks = core::mem::take(&mut self.toks);
        let n = toks.len();
        // matching close of every open (global: equal to the TS per-slice search, parentheses are balanced inside)
        self.close_of.clear();
        self.close_of.resize(n, u32::MAX);
        self.stack.clear();
        for (k, t) in toks.iter().enumerate() {
            match t {
                Tok::Open => self.stack.push(k as u32),
                Tok::Close => {
                    if let Some(o) = self.stack.pop() {
                        self.close_of[o as usize] = k as u32;
                    }
                }
                _ => {}
            }
        }
        let mut out = core::mem::take(&mut self.toks2);
        out.clear();
        self.frames.clear();
        let mut i = 0;
        while i < n {
            if let Some(f) = self.frames.last().copied()
                && f.close as usize == i
            {
                self.frames.pop();
                let s = f.out_start as usize;
                match f.op {
                    // pushed when the frame opened
                    "@" => {}
                    "#" => out.push(Tok::Text(Txt::Lit("#"))),
                    op => {
                        let mut d2 = 0i64;
                        let mut colon = None;
                        for (k, x) in out[s..].iter().enumerate() {
                            match x {
                                Tok::Open => d2 += 1,
                                Tok::Close => d2 -= 1,
                                Tok::Text(t) if d2 == 0 && self.txt_is(*t, ":") => colon = Some(s + k),
                                _ => {}
                            }
                        }
                        match colon {
                            Some(c) => out[c] = Tok::Text(Txt::Lit(op)),
                            None => {
                                out.truncate(s);
                                out.extend_from_slice(&toks[f.fn_at as usize..=f.close as usize]);
                            }
                        }
                    }
                }
                i += 1;
                continue;
            }
            let t = toks[i];
            let op = match t {
                Tok::Fn { canon: Some(c), .. } => operator_of(self.canon_str(c)),
                _ => None,
            };
            match op {
                Some(op) if matches!(toks.get(i + 1), Some(Tok::Open)) && self.close_of[i + 1] != u32::MAX => {
                    self.frames.push(OpFrame { op, out_start: out.len() as u32, close: self.close_of[i + 1], fn_at: i as u32 });
                    if op == "@" {
                        out.push(Tok::Text(Txt::Lit("@")));
                    }
                    i += 2;
                }
                _ => {
                    out.push(t);
                    i += 1;
                }
            }
        }
        self.toks2 = toks;
        self.toks = out;
    }

    fn txt_is(&self, t: Txt, s: &str) -> bool {
        match t {
            Txt::Lit(l) => l == s,
            Txt::Span((a, b)) => {
                let u = &self.src[a as usize..b as usize];
                u.len() == s.len() && u.iter().zip(s.bytes()).all(|(&x, y)| x == u16::from(y))
            }
        }
    }

    /// The text of a name token (`''` unescaped) into `out`.
    fn name_units(&self, s: NameS, out: &mut Vec<u16>) {
        out.clear();
        match s {
            NameS::Span((a, b), unesc) => {
                let u = &self.src[a as usize..b as usize];
                if unesc {
                    let mut k = 0;
                    while k < u.len() {
                        out.push(u[k]);
                        k += if u[k] == Q && u.get(k + 1) == Some(&Q) { 2 } else { 1 };
                    }
                } else {
                    out.extend_from_slice(u);
                }
            }
            NameS::AbsRc(col, (a, b)) => {
                out.push(u16::from(b'$'));
                col_letters(col, out);
                out.push(u16::from(b'$'));
                out.extend_from_slice(&self.src[a as usize..b as usize]);
            }
        }
    }

    /// `scopeParams(toks, mark)`: a LET/LAMBDA parameter (declared or used inside) is a name, never a function as a
    /// value; with `mark` it gets `_xlpm.` (and the optional `[b]` `_xlop.`).
    pub(super) fn scope_params(&mut self, mark: bool) {
        self.pframes.clear();
        self.params.clear();
        self.param_units.clear();
        let mut depth = 0i64;
        let n = self.toks.len();
        let sig_before = |toks: &[Tok], k: usize| toks[..k].iter().rev().find(|t| !matches!(t, Tok::Ws(_))).copied();
        let sig_after = |toks: &[Tok], k: usize| toks[k + 1..].iter().find(|t| !matches!(t, Tok::Ws(_))).copied();
        let mut ubuf = core::mem::take(&mut self.u16a);
        for k in 0..n {
            let t = self.toks[k];
            match t {
                Tok::Open => {
                    depth += 1;
                    if let Some(Tok::Fn { canon: Some(c), .. }) = sig_before(&self.toks, k) {
                        let cs = self.canon_str(c);
                        if cs == "LET" || cs == "LAMBDA" {
                            self.pframes.push(PFrame { lambda: cs == "LAMBDA", depth, arg: 0 });
                        }
                    }
                    continue;
                }
                Tok::Close => {
                    if self.pframes.last().is_some_and(|f| f.depth == depth) {
                        let fi = self.pframes.len() as u32 - 1;
                        self.pframes.pop();
                        while self.params.last().is_some_and(|p| p.0 == fi) {
                            self.params.pop();
                        }
                    }
                    depth -= 1;
                    continue;
                }
                _ => {}
            }
            let top = self.pframes.last().copied();
            if let Tok::Sep(SepK::List) = t
                && let Some(tp) = top
                && tp.depth == depth
            {
                if let Some(f) = self.pframes.last_mut() {
                    f.arg += 1;
                }
                continue;
            }
            let decl_pos = top.is_some_and(|tp| tp.depth == depth)
                && matches!(sig_before(&self.toks, k), Some(Tok::Open | Tok::Sep(_)))
                && matches!(sig_after(&self.toks, k), Some(Tok::Sep(_)));
            let declaring = decl_pos && top.is_some_and(|tp| tp.lambda || tp.arg % 2 == 0);
            // `nameOf(t)` and the text `s` it is compared by
            let s: Option<(NameS, bool)> = match t {
                Tok::Name { s, pre: Pre::None, .. } => Some((s, false)),
                Tok::Eta { raw, .. } => Some((NameS::Span(raw, false), true)),
                _ => None,
            };
            if let Some((s, _)) = s {
                self.name_units(s, &mut ubuf);
                let mut up = core::mem::take(&mut self.u16c);
                upper_units_into(&ubuf, &mut up);
                // the recreated token keeps the text (an eta's raw word) but not `quoted`
                let renamed = Tok::Name { s, pre: if mark { Pre::Pm } else { Pre::None }, quoted: false };
                if declaring {
                    let fi = self.pframes.len() as u32 - 1;
                    let st = self.param_units.len() as u32;
                    self.param_units.extend_from_slice(&up);
                    self.params.push((fi, st, up.len() as u32));
                    self.toks[k] = renamed;
                } else if self.visible(&up) {
                    self.toks[k] = renamed;
                }
                self.u16c = up;
            } else if let Tok::Sref { table, body } = t
                && table.0 == table.1
                && declaring
                && top.is_some_and(|tp| tp.lambda)
                && body.form == Form::Simple
                && !body.this_row
                && let Some(SItem::Col(c)) = self.items.get(body.items.0 as usize).copied().filter(|_| body.items.1 > 0)
            {
                let mut up = core::mem::take(&mut self.u16c);
                upper_units_into(&self.src[c.0 as usize..c.1 as usize], &mut up);
                let fi = self.pframes.len() as u32 - 1;
                let st = self.param_units.len() as u32;
                self.param_units.extend_from_slice(&up);
                self.params.push((fi, st, up.len() as u32));
                self.u16c = up;
                if mark {
                    self.toks[k] = Tok::Name { s: NameS::Span(c, false), pre: Pre::Op, quoted: false };
                }
            }
        }
        self.u16a = ubuf;
    }

    fn visible(&self, up: &[u16]) -> bool {
        self.params.iter().any(|&(_, s, l)| &self.param_units[s as usize..(s + l) as usize] == up)
    }
}

// ------------------------------------------------------------------ render

fn push_str(out: &mut Vec<u16>, s: &str) {
    out.extend(s.encode_utf16());
}

impl Translator {
    fn push_txt(&self, out: &mut Vec<u16>, t: Txt) {
        match t {
            Txt::Span((a, b)) => out.extend_from_slice(&self.src[a as usize..b as usize]),
            Txt::Lit(s) => push_str(out, s),
        }
    }

    fn span(&self, s: Sp) -> &[u16] {
        &self.src[s.0 as usize..s.1 as usize]
    }

    /// `render(toks, tgt, o)`.
    pub(super) fn render(&mut self, tgt: &Dialect, file_prefixes: bool, in_table: Option<JsStr<'_>>, out: &mut Vec<u16>) {
        let toks = core::mem::take(&mut self.toks);
        let in_table: Option<Vec<u16>> = in_table.map(|t| t.units().collect());
        let mut nb = core::mem::take(&mut self.u16b);
        for &t in &toks {
            match t {
                Tok::Text(x) => self.push_txt(out, x),
                Tok::Ws(s) => {
                    let u = self.span(s);
                    if tgt.canonical {
                        out.extend_from_slice(u);
                    } else {
                        // the formula bar shows a line break as \n (the file may have \r\n)
                        let mut k = 0;
                        while k < u.len() {
                            if u[k] == u16::from(b'\r') {
                                out.push(u16::from(b'\n'));
                                k += if u.get(k + 1) == Some(&u16::from(b'\n')) { 2 } else { 1 };
                            } else {
                                out.push(u[k]);
                                k += 1;
                            }
                        }
                    }
                }
                Tok::Open => out.push(u16::from(b'(')),
                Tok::Close => out.push(u16::from(b')')),
                Tok::Num { int, frac, exp } => {
                    out.extend_from_slice(self.span(int));
                    if let Some(f) = frac {
                        out.extend_from_slice(&tgt.decimal);
                        out.extend_from_slice(self.span(f));
                    }
                    out.extend_from_slice(self.span(exp));
                }
                Tok::Sep(k) => out.extend_from_slice(match k {
                    SepK::List => &tgt.list,
                    SepK::Col => &tgt.acol,
                    SepK::Row => &tgt.arow,
                }),
                Tok::Bool(v) => push_str(out, tgt.lang.bool_texts()[usize::from(!v)]),
                Tok::Err(code) => push_str(out, tgt.lang.error_text(code)),
                Tok::Fn { canon: Some(c), .. } => {
                    let cs = self.canon_str(c);
                    if tgt.canonical {
                        if file_prefixes {
                            let entry = entry_exact(strip_upper_prefix(cs));
                            match entry {
                                Some(e) => push_str(out, e.prefix),
                                None if operator_of(cs).is_some() => push_str(out, "_xlfn."),
                                None => {}
                            }
                        }
                        push_str(out, cs);
                    } else {
                        push_str(out, tgt.lang.local_function(cs));
                    }
                }
                Tok::Fn { canon: None, raw } => {
                    // unknown: in the canonical form, mark the one that collides with a function (as the file)
                    if tgt.canonical {
                        upper_utf8_into(JsStr::Utf16(&self.src[raw.0 as usize..raw.1 as usize]), &mut self.fold);
                        if entry_exact(strip_upper_prefix(&self.fold)).is_some() {
                            push_str(out, "_xludf.");
                        }
                    }
                    out.extend_from_slice(self.span(raw));
                }
                Tok::Eta { canon, .. } => {
                    let cs = self.canon_str(canon);
                    if tgt.canonical {
                        if file_prefixes {
                            push_str(out, "_xleta.");
                        }
                        push_str(out, cs);
                    } else {
                        push_str(out, tgt.lang.local_function(cs));
                    }
                }
                Tok::Name { s, pre, quoted } => {
                    self.name_units(s, &mut nb);
                    if pre == Pre::Op {
                        if tgt.canonical && file_prefixes {
                            push_str(out, "_xlop.");
                            out.extend_from_slice(&nb);
                        } else {
                            out.push(u16::from(b'['));
                            out.extend_from_slice(&nb);
                            out.push(u16::from(b']'));
                        }
                        continue;
                    }
                    if pre == Pre::Pm && tgt.canonical && file_prefixes {
                        push_str(out, "_xlpm.");
                        out.extend_from_slice(&nb);
                        continue;
                    }
                    upper_utf8_into(JsStr::Utf16(&nb), &mut self.fold);
                    let is_bool = self.fold == tgt.bool_f[0] || self.fold == tgt.bool_f[1];
                    if is_bool || (quoted && !plain_name(&nb)) {
                        out.push(Q);
                        for &u in &nb {
                            out.push(u);
                            if u == Q {
                                out.push(Q);
                            }
                        }
                        out.push(Q);
                    } else {
                        out.extend_from_slice(&nb);
                    }
                }
                Tok::R1c1 { row, col } => {
                    self.render_part(out, &tgt.row, row, tgt);
                    self.render_part(out, &tgt.col, col, tgt);
                }
                Tok::Sref { table, body } => self.render_structured(out, table, body, tgt, file_prefixes, in_table.as_deref()),
            }
        }
        self.u16b = nb;
        self.toks = toks;
    }

    fn render_part(&self, out: &mut Vec<u16>, letter: &[u16], p: Part, tgt: &Dialect) {
        match p {
            Part::Absent => {}
            Part::Bare => out.extend_from_slice(letter),
            Part::Abs(s) => {
                out.extend_from_slice(letter);
                out.extend_from_slice(self.span(s));
            }
            Part::Rel(s) => {
                out.extend_from_slice(letter);
                out.extend_from_slice(&tgt.open);
                out.extend_from_slice(self.span(s));
                out.extend_from_slice(&tgt.close);
            }
        }
    }

    fn render_item(&self, out: &mut Vec<u16>, x: SItem, tgt: &Dialect) {
        out.push(u16::from(b'['));
        match x {
            SItem::Kw(k) => push_str(out, tgt.lang.kw_out[k.index()]),
            SItem::Col(s) | SItem::Raw(s) => out.extend_from_slice(self.span(s)),
            SItem::Range(a, b) => {
                out.extend_from_slice(self.span(a));
                push_str(out, "]:[");
                out.extend_from_slice(self.span(b));
            }
        }
        out.push(u16::from(b']'));
    }

    /// `renderStructured(t, tgt, o)`.
    fn render_structured(&self, out: &mut Vec<u16>, table: Sp, b: SBody, tgt: &Dialect, file_prefixes: bool, in_table: Option<&[u16]>) {
        let items = &self.items[b.items.0 as usize..(b.items.0 + b.items.1) as usize];
        // the table name: a span of the input, or `inTable` substituted (`None` = the input's)
        let mut tname: Option<&[u16]> = Some(self.span(table));
        let same_table = |t: &[u16], it: &[u16]| uo_text::case::to_uppercase(t) == uo_text::case::to_uppercase(it);
        let fp = file_prefixes && tgt.canonical;
        if b.this_row {
            if fp {
                if tname.is_some_and(<[u16]>::is_empty)
                    && let Some(it) = in_table
                {
                    tname = Some(it);
                }
                out.extend_from_slice(tname.unwrap_or_default());
                out.push(u16::from(b'['));
                if items.is_empty() {
                    push_str(out, tgt.lang.kw_out[Kw::ThisRow.index()]);
                } else {
                    out.push(u16::from(b'['));
                    push_str(out, tgt.lang.kw_out[Kw::ThisRow.index()]);
                    out.push(u16::from(b']'));
                    for &x in items {
                        out.extend_from_slice(&tgt.list);
                        self.render_item(out, x, tgt);
                    }
                }
                out.push(u16::from(b']'));
                return;
            }
            // inside its own table (measured): one column without the name, a column range WITH the name
            if let Some(it) = in_table {
                let t = tname.unwrap_or_default();
                if t.is_empty() || same_table(t, it) {
                    match items {
                        [SItem::Col(_)] => tname = Some(&[]),
                        [SItem::Range(..)] if t.is_empty() => tname = Some(it),
                        _ => {}
                    }
                }
            }
            let t = tname.unwrap_or_default();
            out.extend_from_slice(t);
            if items.is_empty() {
                push_str(out, "[@]");
                return;
            }
            if let [SItem::Col(c)] = items
                && simple_col(self.span(*c))
            {
                push_str(out, "[@");
                out.extend_from_slice(self.span(*c));
                out.push(u16::from(b']'));
                return;
            }
            push_str(out, "[@");
            for (k, &x) in items.iter().enumerate() {
                if k > 0 {
                    out.extend_from_slice(&tgt.list);
                }
                self.render_item(out, x, tgt);
            }
            out.push(u16::from(b']'));
            return;
        }
        let t = tname.unwrap_or_default();
        if b.form == Form::Empty {
            if t.is_empty() {
                push_str(out, "[]");
            } else {
                out.extend_from_slice(t);
                if fp {
                    push_str(out, "[]");
                }
            }
            return;
        }
        // one column of its own table (also in the totals row) without the name (measured in the corpus)
        let mut t = t;
        if !fp
            && let Some(it) = in_table
            && !t.is_empty()
            && same_table(t, it)
            && matches!(items, [SItem::Col(_)])
        {
            t = &[];
        }
        out.extend_from_slice(t);
        if b.form == Form::Simple {
            out.push(u16::from(b'['));
            match items.first() {
                Some(SItem::Kw(k)) => push_str(out, tgt.lang.kw_out[k.index()]),
                Some(SItem::Col(s) | SItem::Raw(s)) => out.extend_from_slice(self.span(*s)),
                Some(&x @ SItem::Range(..)) => self.render_item(out, x, tgt),
                None => {}
            }
            out.push(u16::from(b']'));
            return;
        }
        out.push(u16::from(b'['));
        for (k, &x) in items.iter().enumerate() {
            if k > 0 {
                out.extend_from_slice(&tgt.list);
            }
            self.render_item(out, x, tgt);
        }
        out.push(u16::from(b']'));
    }
}
