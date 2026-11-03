//! Word numbering (`docx-layout/src/numbering.ts`): the list counters (per abstract numbering,
//! level restarts, start overrides on the first use of a `w:num`), `lvlText` with `%1`..`%9`
//! (each level in its own format, `isLgl` forcing decimal) and the number formats.
//!
//! [`NumberingState`] is plain data: the counters, the `w:num`s seen and the deferred overrides;
//! every method takes the [`DocFormatting`] it reads (the TS kept `numbering` and `styles` on the
//! object), so the state clones cheaply (`pure()`, the layout's flow state) and holds no borrow
//! (K18). Texts are UTF-16 (`lvlText` can hold any character).
//!
//! Huge counters (K11b): roman, alphabetic and chicago texts repeat a symbol per unit of the
//! number; the TS builds them until memory runs out (or `repeat` throws `RangeError`). Here every
//! text stops at [`TextCap::max`] UTF-16 units and records that it was cut.

use uo_jsnum::conv::{is_integer, to_uint16};
use uo_jsnum::format::to_js_string;
use uo_jsnum::math::{floor, max, min, rem};

use super::num::js_to_radix;
use crate::js::{JsNumber, JsRecord};
use crate::model::formatting::{DocFormatting, NumInstance, NumLevel, StyleDef};
use crate::text::Utf16;

/// A cap on the length of a generated text (`max_field_text` of the layout limits, K11b) and
/// whether a text was cut since the last [`TextCap::take_truncated`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TextCap {
    /// UTF-16 units of one generated text
    pub max: usize,
    truncated: bool,
}

impl Default for TextCap {
    fn default() -> TextCap {
        TextCap::new(65_536)
    }
}

impl TextCap {
    #[must_use]
    pub fn new(max: usize) -> TextCap {
        TextCap { max, truncated: false }
    }

    /// Whether a text was cut since the last call (and resets it).
    pub fn take_truncated(&mut self) -> bool {
        std::mem::take(&mut self.truncated)
    }

    /// Records that a text was cut.
    pub fn mark_cut(&mut self) {
        self.truncated = true;
    }
}

/// Appends `unit` `count` times, within the cap (`s.repeat(count)` appended).
fn push_repeat(out: &mut Vec<u16>, units: &[u16], count: f64, cap: &mut TextCap) {
    let mut c = count;
    while c >= 1.0 {
        if out.len() + units.len() > cap.max {
            cap.mark_cut();
            return;
        }
        out.extend_from_slice(units);
        c -= 1.0;
    }
}

const ROMAN: [(f64, &str); 13] = [
    (1000.0, "m"),
    (900.0, "cm"),
    (500.0, "d"),
    (400.0, "cd"),
    (100.0, "c"),
    (90.0, "xc"),
    (50.0, "l"),
    (40.0, "xl"),
    (10.0, "x"),
    (9.0, "ix"),
    (5.0, "v"),
    (4.0, "iv"),
    (1.0, "i"),
];

/// `roman(n)` (numbering.ts:14), lower case.
fn roman(n: f64, cap: &mut TextCap) -> String {
    if n <= 0.0 {
        return to_js_string(n);
    }
    let mut n = n;
    let mut out = String::new();
    for (v, s) in ROMAN {
        while n >= v {
            if out.len() + s.len() > cap.max {
                cap.mark_cut();
                return out;
            }
            out.push_str(s);
            let next = n - v;
            if next == n {
                // the TS never ends here (n ≥ 2^63: n - 1000 rounds back to n); the cap stops it
                cap.mark_cut();
                return out;
            }
            n = next;
        }
    }
    out
}

/// `letters(n)` (numbering.ts:21): a..z, aa..zz, aaa… (the letter repeated).
fn letters(n: f64, cap: &mut TextCap) -> Vec<u16> {
    if n <= 0.0 {
        return to_js_string(n).encode_utf16().collect();
    }
    // String.fromCharCode(97 + ((n - 1) % 26)): ToUint16 of the code
    let ch = to_uint16(97.0 + rem(n - 1.0, 26.0));
    let count = floor((n - 1.0) / 26.0) + 1.0;
    let mut out = Vec::new();
    if count.is_infinite() {
        // `repeat(Infinity)` throws RangeError in the TS (accepted by name, K11b): capped here
        cap.mark_cut();
    }
    push_repeat(&mut out, &[ch], if count.is_nan() { 0.0 } else { count }, cap);
    out
}

const UNITS_PT: [&str; 20] = [
    "zero",
    "um",
    "dois",
    "três",
    "quatro",
    "cinco",
    "seis",
    "sete",
    "oito",
    "nove",
    "dez",
    "onze",
    "doze",
    "treze",
    "catorze",
    "quinze",
    "dezesseis",
    "dezessete",
    "dezoito",
    "dezenove",
];
const TENS_PT: [&str; 10] = ["", "", "vinte", "trinta", "quarenta", "cinquenta", "sessenta", "setenta", "oitenta", "noventa"];

/// `arr[n]` of a JS array: the element for an integral index in range, else `undefined`.
fn at<'a>(arr: &[&'a str], n: f64) -> Option<&'a str> {
    if n >= 0.0 && is_integer(n) && n < 4_294_967_295.0 {
        let i = usize::try_from(uo_jsnum::conv::to_uint32(n)).ok()?;
        arr.get(i).copied()
    } else {
        None
    }
}

/// `cardinalPt(n)` (numbering.ts:32).
fn cardinal_pt(n: f64) -> String {
    if n < 20.0 {
        return at(&UNITS_PT, n).map_or_else(|| to_js_string(n), str::to_owned);
    }
    if n < 100.0 {
        let t = floor(n / 10.0);
        let u = rem(n, 10.0);
        let mut s = at(&TENS_PT, t).unwrap_or("undefined").to_owned();
        if uo_jsnum::conv::truthy(u) {
            s.push_str(" e ");
            s.push_str(at(&UNITS_PT, u).unwrap_or("undefined"));
        }
        return s;
    }
    to_js_string(n)
}

/// `s.charAt(0).toUpperCase() + s.slice(1)` over UTF-16 units (a lone first surrogate stays).
#[must_use]
pub fn first_upper(s: &[u16]) -> Vec<u16> {
    let Some((first, rest)) = s.split_first() else { return Vec::new() };
    let mut out: Vec<u16> = uo_text::case::to_uppercase(std::slice::from_ref(first)).into_owned();
    out.extend_from_slice(rest);
    out
}

/// `formatNumber(n, fmt, lang)` (numbering.ts:41).
#[must_use]
pub fn format_number(n: f64, fmt: Option<&str>, lang: &str, cap: &mut TextCap) -> Utf16 {
    let s: String = match fmt {
        None | Some("decimal") => to_js_string(n),
        Some("decimalZero") => {
            if (0.0..10.0).contains(&n) {
                format!("0{}", to_js_string(n))
            } else {
                to_js_string(n)
            }
        }
        Some("upperRoman") => uo_text::case::to_uppercase_str(&roman(n, cap)),
        Some("lowerRoman") => roman(n, cap),
        Some("upperLetter") => return Utf16::from_units(uo_text::case::to_uppercase(&letters(n, cap)).into_owned()),
        Some("lowerLetter") => return Utf16::from_units(letters(n, cap)),
        Some("ordinal") => {
            if lang.starts_with("pt") {
                format!("{}º", to_js_string(n))
            } else {
                let r100 = rem(n, 100.0);
                let idx = if r100 > 10.0 && r100 < 14.0 { 0.0 } else { rem(min(rem(n, 10.0), 4.0), 4.0) };
                format!("{}{}", to_js_string(n), at(&["th", "st", "nd", "rd"], idx).unwrap_or("th"))
            }
        }
        Some("cardinalText") => return Utf16::from_units(first_upper(&cardinal_pt(n).encode_utf16().collect::<Vec<u16>>())),
        Some("ordinalText") => format!("{}º", to_js_string(n)),
        Some("hex") => js_to_radix(n, 16).to_ascii_uppercase(),
        Some("decimalEnclosedCircle") => {
            if (1.0..=20.0).contains(&n) {
                // String.fromCodePoint(0x2460 + n - 1): a fraction throws RangeError in the TS
                // (accepted by name, K11): String(n) here
                if is_integer(n) {
                    char::from_u32(0x2460 + uo_jsnum::conv::to_uint32(n) - 1).map_or_else(|| to_js_string(n), String::from)
                } else {
                    to_js_string(n)
                }
            } else {
                to_js_string(n)
            }
        }
        Some("decimalEnclosedParen") => format!("({})", to_js_string(n)),
        Some("decimalEnclosedFullstop") => format!("{}.", to_js_string(n)),
        Some("numberInDash") => format!("- {} -", to_js_string(n)),
        Some("chicago") => {
            if n <= 0.0 {
                to_js_string(n)
            } else {
                // '*†‡§'[(n - 1) % 4].repeat(…): a fraction or NaN indexes `undefined` and the TS
                // throws TypeError (accepted by name, K11): empty here
                let i = rem(n - 1.0, 4.0);
                let sym = at(&["*", "†", "‡", "§"], i);
                let mut out = Vec::new();
                if let Some(sym) = sym {
                    let units: Vec<u16> = sym.encode_utf16().collect();
                    let count = floor((n - 1.0) / 4.0) + 1.0;
                    if count.is_infinite() {
                        cap.mark_cut();
                    }
                    push_repeat(&mut out, &units, count, cap);
                }
                return Utf16::from_units(out);
            }
        }
        Some("none" | "bullet") => String::new(),
        Some(_) => to_js_string(n),
    };
    Utf16::from_units(s.encode_utf16().collect::<Vec<u16>>())
}

/// Where a marker's level comes from (the TS keeps the `NumLevel` object; here its place in the
/// formatting, read back with [`NumberMarker::level_in`]).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LevelSrc {
    /// `nums[num].overrides[ilvl].lvl`
    Override { num: Box<str>, ilvl: Box<str> },
    /// `abstractNums[abs].levels[ilvl]`
    Abstract { abs: Box<str>, ilvl: Box<str> },
}

impl LevelSrc {
    /// The level in `fmt` (the formatting the marker was computed from).
    #[must_use]
    pub fn get<'f>(&self, fmt: &'f DocFormatting) -> Option<&'f NumLevel> {
        let n = &fmt.numbering;
        match self {
            LevelSrc::Override { num, ilvl } => n.nums.get(num)?.overrides.get(ilvl)?.lvl.as_deref(),
            LevelSrc::Abstract { abs, ilvl } => n.abstract_nums.get(abs)?.levels.get(ilvl),
        }
    }
}

/// `NumberMarker` (numbering.ts:70).
#[derive(Debug, Clone, PartialEq)]
pub struct NumberMarker {
    pub text: Utf16,
    /// `parts` (REF `\w`): the texts of the levels above that the item's text does not cite,
    /// outside in, and the item's text last (absent for bullets)
    pub parts: Option<Vec<Utf16>>,
    /// `level`
    pub level: LevelSrc,
    pub num_id: f64,
    pub ilvl: f64,
    /// `value`: the counter of the level
    pub value: f64,
    /// `listKey` (`a<abstractNumId>`)
    pub list_key: Box<str>,
    /// `chapter`: the numbers of the marker (first to last `%n`), for PAGE `w:chapStyle` and
    /// STYLEREF `\s`
    pub chapter: Option<Utf16>,
}

impl NumberMarker {
    /// `marker.level` in `fmt`.
    #[must_use]
    pub fn level_in<'f>(&self, fmt: &'f DocFormatting) -> Option<&'f NumLevel> {
        self.level.get(fmt)
    }
}

/// `LevelCount` (numbering.ts:92): `real` = a paragraph of this level was numbered; otherwise the
/// value exists because a deeper level needed it. `v` stores a `null` start override as 0, the
/// only arithmetic it takes part in (`null + 1`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LevelCount {
    pub v: f64,
    pub real: bool,
}

/// The counters of one list: levels 0–8, plus the slot a NaN level writes to (`c[NaN]`, an array
/// property in the TS).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Counters {
    levels: [Option<LevelCount>; 9],
    nan: Option<LevelCount>,
}

impl Counters {
    fn get(&self, l: f64) -> Option<LevelCount> {
        if l.is_nan() {
            return self.nan;
        }
        level_index(l).and_then(|i| self.levels.get(i).copied().flatten())
    }

    fn set(&mut self, l: f64, v: Option<LevelCount>) {
        if l.is_nan() {
            self.nan = v;
        } else if let Some(slot) = level_index(l).and_then(|i| self.levels.get_mut(i)) {
            *slot = v;
        }
    }
}

/// A level 0..=8 as an index.
fn level_index(l: f64) -> Option<usize> {
    if (0.0..9.0).contains(&l) && is_integer(l) { usize::try_from(uo_jsnum::conv::to_uint32(l)).ok() } else { None }
}

/// `String(n)`, the property key of a numeric index (`nums[numId]`, `overrides[ilvl]`).
fn key(n: JsNumber) -> String {
    match n {
        JsNumber::Num(x) => to_js_string(x),
        JsNumber::Null => "null".into(),
    }
}

/// `NumberingState` (numbering.ts:94).
#[derive(Clone, Debug, PartialEq)]
pub struct NumberingState {
    /// counters per abstract numbering (`a<id>`), TS `Map` order
    counters: indexmap::IndexMap<Box<str>, Counters>,
    /// `w:num`s already used (the start override acts on the first use only)
    seen_nums: indexmap::IndexSet<Box<str>>,
    /// `numId:ilvl` with a deferred start override
    deferred: indexmap::IndexSet<Box<str>>,
    lang: Box<str>,
    /// the cap of the generated texts
    pub cap: TextCap,
}

impl Default for NumberingState {
    fn default() -> NumberingState {
        NumberingState::new("pt-BR", TextCap::default())
    }
}

impl NumberingState {
    #[must_use]
    pub fn new(lang: &str, cap: TextCap) -> NumberingState {
        NumberingState {
            counters: indexmap::IndexMap::new(),
            seen_nums: indexmap::IndexSet::new(),
            deferred: indexmap::IndexSet::new(),
            lang: lang.into(),
            cap,
        }
    }

    /// The language of the number formats.
    #[must_use]
    pub fn lang(&self) -> &str {
        &self.lang
    }

    fn inst(fmt: &DocFormatting, num_id: JsNumber) -> Option<&NumInstance> {
        fmt.numbering.nums.get(&key(num_id))
    }

    /// `listKey(numId)` (numbering.ts:118).
    #[must_use]
    pub fn list_key(&self, fmt: &DocFormatting, num_id: JsNumber) -> Option<Box<str>> {
        if !num_id.truthy() {
            return None;
        }
        Self::abstract_for(fmt, num_id, 0).map(|a| format!("a{}", key(a)).into())
    }

    /// `abstractFor(numId)` (numbering.ts:124): follows `numStyleLink` → numbering style → numId.
    fn abstract_for(fmt: &DocFormatting, num_id: JsNumber, depth: u32) -> Option<JsNumber> {
        let inst = Self::inst(fmt, num_id)?;
        let abs = fmt.numbering.abstract_nums.get(&key(inst.abstract_num_id))?;
        if let Some(link) = abs.num_style_link.as_deref().filter(|s| !s.is_empty())
            && depth < 4
        {
            let st: Option<&StyleDef> = fmt.styles.styles.get(link);
            let linked = st.and_then(|s| s.p_pr.as_deref()).and_then(|p| p.num_pr.as_deref()).and_then(|n| n.num_id);
            if let Some(l) = linked
                && !same(l, num_id)
                && let Some(r) = Self::abstract_for(fmt, l, depth + 1)
            {
                return Some(r);
            }
        }
        Some(abs.abstract_num_id)
    }

    /// `level(numId, ilvl)` (numbering.ts:141): the instance override (with `lvlText` or
    /// `numFmt`) over the abstract numbering's level.
    #[must_use]
    pub fn level<'f>(&self, fmt: &'f DocFormatting, num_id: JsNumber, ilvl: JsNumber) -> Option<&'f NumLevel> {
        Self::level_in(fmt, num_id, ilvl)
    }

    /// `level(numId, ilvl)` without a state (it reads only the formatting).
    #[must_use]
    pub fn level_in(fmt: &DocFormatting, num_id: JsNumber, ilvl: JsNumber) -> Option<&NumLevel> {
        Self::level_src(fmt, num_id, ilvl).and_then(|s| s.get(fmt))
    }

    fn level_src(fmt: &DocFormatting, num_id: JsNumber, ilvl: JsNumber) -> Option<LevelSrc> {
        let inst = Self::inst(fmt, num_id)?;
        let ik = key(ilvl);
        if let Some(ov) = inst.overrides.get(&ik).and_then(|o| o.lvl.as_deref())
            && (ov.lvl_text.is_some() || ov.num_fmt.is_some())
        {
            return Some(LevelSrc::Override { num: key(num_id).into(), ilvl: ik.into() });
        }
        let ak = key(Self::abstract_for(fmt, num_id, 0)?);
        let abs = fmt.numbering.abstract_nums.get(&ak)?;
        abs.levels.get(&ik).map(|_| LevelSrc::Abstract { abs: ak.into(), ilvl: ik.into() })
    }

    /// `next(numId, ilvl)` (numbering.ts:164): advances the counter and returns the paragraph's
    /// marker (`None` = no numbering). The rules measured in Word are in the TS doc comment.
    pub fn next(&mut self, fmt: &DocFormatting, num_id: JsNumber, ilvl: JsNumber) -> Option<NumberMarker> {
        if !num_id.truthy() {
            return None;
        }
        let inst = Self::inst(fmt, num_id)?;
        let abs_id = Self::abstract_for(fmt, num_id, 0)?;
        let ilvl = max(0.0, min(8.0, ilvl.to_number()));
        let lvl_src = Self::level_src(fmt, num_id, JsNumber::Num(ilvl))?;
        let lvl = lvl_src.get(fmt)?;
        let list_key: Box<str> = format!("a{}", key(abs_id)).into();
        let num_key = key(num_id);
        let ov_start = |l: f64| -> Option<JsNumber> { inst.overrides.get(&key(JsNumber::Num(l))).and_then(|o| o.start_override) };
        // the level's start (what a virtual parent shows)
        let lvl_start = |l: f64| -> f64 {
            match Self::level_in(fmt, num_id, JsNumber::Num(l)).and_then(|v| v.start) {
                Some(JsNumber::Num(x)) => x,
                _ => 0.0,
            }
        };
        // the level's start in this w:num (its start override, if any; `??` skips null)
        let start_of = |l: f64| -> f64 {
            match ov_start(l) {
                Some(JsNumber::Num(x)) => x,
                _ => lvl_start(l),
            }
        };
        let mut c = self.counters.get(&list_key).cloned().unwrap_or_default();
        if !self.seen_nums.contains(num_key.as_str()) {
            self.seen_nums.insert(num_key.clone().into());
            for l in 0..9u8 {
                let lf = f64::from(l);
                let Some(s) = ov_start(lf) else { continue };
                if lf >= ilvl {
                    c.set(lf, None);
                } else if !c.get(lf).is_some_and(|x| x.real) {
                    c.set(lf, Some(LevelCount { v: s.to_number(), real: false }));
                    self.deferred.insert(format!("{num_key}:{l}").into());
                }
            }
        }
        let cur = c.get(ilvl);
        let was_deferred = self.deferred.shift_remove(format!("{num_key}:{}", to_js_string(ilvl)).as_str());
        match cur {
            Some(cur) if !was_deferred => c.set(ilvl, Some(LevelCount { v: cur.v + 1.0, real: true })),
            _ => c.set(ilvl, Some(LevelCount { v: start_of(ilvl), real: true })),
        }
        // parents never used start counting (with the level's start)
        let mut l = 0.0;
        while l < ilvl {
            if c.get(l).is_none() {
                c.set(l, Some(LevelCount { v: start_of(l), real: false }));
            }
            l += 1.0;
        }
        // deeper levels restart (unless lvlRestart says otherwise)
        let mut d = ilvl + 1.0;
        while d < 9.0 {
            match Self::level_in(fmt, num_id, JsNumber::Num(d)).and_then(|v| v.lvl_restart) {
                None => c.set(d, None),
                Some(JsNumber::Num(x)) => {
                    if x != 0.0 && ilvl < x {
                        c.set(d, None);
                    }
                }
                // `null === 0` is false and `ilvl < null` is `ilvl < 0`: never
                Some(JsNumber::Null) => {}
            }
            d += 1.0;
        }
        let lvl_text: Vec<u16> = if lvl.lvl_text_null == Some(true) { Vec::new() } else { units(lvl.lvl_text.as_deref()) };
        let is_bullet = lvl.num_fmt.as_deref() == Some("bullet");
        let lang = &*self.lang;
        let mut cap = self.cap;
        // what the placeholders read, per level (pure within this call; the TS looks them up per
        // placeholder, which a level text with thousands of them makes slow, not different)
        let level_starts: [f64; 9] = std::array::from_fn(|l| lvl_start(f64::from(u8::try_from(l).unwrap_or(0))));
        let level_fmts: [Option<&str>; 9] = std::array::from_fn(|l| {
            Self::level_in(fmt, num_id, JsNumber::Num(f64::from(u8::try_from(l).unwrap_or(0)))).and_then(|x| x.num_fmt.as_deref())
        });
        let at = |a: &[f64; 9], l: f64| level_index(l).and_then(|i| a.get(i).copied());
        let fill = |t: &[u16], own: &NumLevel, cap: &mut TextCap| -> Vec<u16> {
            let max = cap.max;
            let mut cut = false;
            let (out, over) = fill_text(t, max, |l| {
                let v = match c.get(l) {
                    Some(n) if n.real => n.v,
                    _ => at(&level_starts, l).unwrap_or_else(|| lvl_start(l)),
                };
                let f: Option<&str> = if own.is_lgl == Some(true) && l != ilvl {
                    Some("decimal")
                } else {
                    level_index(l).and_then(|i| level_fmts.get(i).copied()).flatten().or(Some("decimal"))
                };
                let f = if f == Some("bullet") { Some("decimal") } else { f };
                let mut c2 = TextCap::new(max);
                let s = format_number(v, f, lang, &mut c2).units().to_vec();
                cut |= c2.take_truncated();
                s
            });
            if over || cut {
                cap.mark_cut();
            }
            out
        };
        let mut chapter: Option<Vec<u16>> = None;
        let text: Vec<u16> = if is_bullet {
            lvl_text.clone()
        } else {
            let t = fill(&lvl_text, lvl, &mut cap);
            // the chapter number (PAGE w:chapStyle, STYLEREF \s): first to last placeholder
            let a = search_placeholder(&lvl_text);
            let b = lvl_text.iter().rposition(|&u| u == u16::from(b'%'));
            if let (Some(a), Some(b)) = (a, b)
                && b >= a
            {
                chapter = Some(fill(super::num::slice16(&lvl_text, a, b + 2), lvl, &mut cap));
            }
            t
        };
        let value = c.get(ilvl).map_or(f64::NAN, |x| x.v);
        let mut parts: Option<Vec<Utf16>> = None;
        if !is_bullet {
            let mut ps: Vec<Vec<u16>> = vec![text.clone()];
            let mut cited: Vec<f64> = refs(&lvl_text);
            let mut l = ilvl - 1.0;
            while l >= 0.0 && !cited.contains(&l) {
                let Some(pl) = Self::level_in(fmt, num_id, JsNumber::Num(l)) else { break };
                if pl.num_fmt.as_deref() == Some("bullet") {
                    break;
                }
                let pt: Vec<u16> = if pl.lvl_text_null == Some(true) { Vec::new() } else { units(pl.lvl_text.as_deref()) };
                ps.insert(0, fill(&pt, pl, &mut cap));
                for r in refs(&pt) {
                    if !cited.contains(&r) {
                        cited.push(r);
                    }
                }
                l -= 1.0;
            }
            parts = Some(ps.into_iter().map(Utf16::from_units).collect());
        }
        self.cap = cap;
        self.counters.insert(list_key.clone(), c);
        Some(NumberMarker {
            text: Utf16::from_units(text),
            parts,
            level: lvl_src,
            num_id: num_id.to_number(),
            ilvl,
            value,
            list_key,
            chapter: chapter.map(Utf16::from_units),
        })
    }

    /// Drops the counters (a fresh state for the same formatting and language).
    pub fn reset(&mut self) {
        self.counters.clear();
        self.seen_nums.clear();
        self.deferred.clear();
    }
}

/// `===` between two numbers of the model (`null` only equals `null`, NaN never equal).
fn same(a: JsNumber, b: JsNumber) -> bool {
    match (a, b) {
        (JsNumber::Num(x), JsNumber::Num(y)) => x == y,
        (JsNumber::Null, JsNumber::Null) => true,
        _ => false,
    }
}

fn units(s: Option<&str>) -> Vec<u16> {
    s.map(|s| s.encode_utf16().collect()).unwrap_or_default()
}

/// `%` followed by `1`..`9`.
fn placeholder_at(t: &[u16], i: usize) -> Option<f64> {
    if t.get(i) != Some(&u16::from(b'%')) {
        return None;
    }
    let d = *t.get(i + 1)?;
    (u16::from(b'1')..=u16::from(b'9')).contains(&d).then(|| f64::from(d - u16::from(b'1')))
}

/// `t.search(/%[1-9]/)`.
fn search_placeholder(t: &[u16]) -> Option<usize> {
    (0..t.len()).find(|&i| placeholder_at(t, i).is_some())
}

/// `[...t.matchAll(/%([1-9])/g)].map(m => +m[1] - 1)`.
fn refs(t: &[u16]) -> Vec<f64> {
    let mut out = Vec::new();
    let mut i = 0;
    while i < t.len() {
        if let Some(l) = placeholder_at(t, i) {
            out.push(l);
            i += 2;
        } else {
            i += 1;
        }
    }
    out
}

/// `t.replace(/%([1-9])/g, (_, k) => f(k - 1))`, stopping at `max` units or past `max`
/// placeholders (K11b: a level text with many placeholders of huge counters, or of the `none`
/// format, whose empty texts would not stop it); the flag says it was cut.
fn fill_text(t: &[u16], max: usize, mut f: impl FnMut(f64) -> Vec<u16>) -> (Vec<u16>, bool) {
    let mut out = Vec::with_capacity(t.len().min(max));
    let mut i = 0;
    let mut placeholders = 0usize;
    while let Some(&u) = t.get(i) {
        let piece: Vec<u16> = if let Some(l) = placeholder_at(t, i) {
            i += 2;
            placeholders += 1;
            if placeholders > max {
                return (out, true);
            }
            f(l)
        } else {
            i += 1;
            vec![u]
        };
        if out.len() + piece.len() > max {
            out.extend_from_slice(piece.get(..max - out.len()).unwrap_or(&[]));
            return (out, true);
        }
        out.extend(piece);
    }
    (out, false)
}

/// The record a `NumberingState` reads, re-exported for the callers' signatures.
pub type NumInstances = JsRecord<NumInstance>;

#[cfg(test)]
mod tests {
    use super::*;

    fn s(u: &Utf16) -> String {
        String::from_utf16_lossy(u.units())
    }

    #[test]
    fn formats() {
        let mut cap = TextCap::default();
        let f = |n: f64, f: &str, cap: &mut TextCap| s(&format_number(n, Some(f), "pt-BR", cap));
        assert_eq!(f(14.0, "upperRoman", &mut cap), "XIV");
        assert_eq!(f(28.0, "lowerLetter", &mut cap), "bb");
        assert_eq!(f(5.0, "chicago", &mut cap), "**");
        assert_eq!(f(25.0, "cardinalText", &mut cap), "Vinte e cinco");
        assert_eq!(f(25.5, "cardinalText", &mut cap), "Vinte e undefined");
        assert_eq!(f(255.0, "hex", &mut cap), "FF");
        assert_eq!(f(3.0, "decimalEnclosedCircle", &mut cap), "\u{2462}");
        assert_eq!(f(-0.0, "decimalZero", &mut cap), "00");
        assert_eq!(s(&format_number(12.0, Some("ordinal"), "en-US", &mut cap)), "12th");
        assert_eq!(s(&format_number(22.0, Some("ordinal"), "en-US", &mut cap)), "22nd");
        assert!(!cap.take_truncated());
        let mut small = TextCap::new(10);
        assert_eq!(f(1e15, "lowerRoman", &mut small).len(), 10);
        assert!(small.take_truncated());
        assert_eq!(f(f64::INFINITY, "lowerLetter", &mut small).encode_utf16().count(), 10);
        assert!(small.take_truncated());
    }
}
