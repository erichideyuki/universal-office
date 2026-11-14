//! The field values of frontier F7c (`fieldValue`'s SEQ, REF, NOTEREF, STYLEREF and PAGEREF
//! answers, engine.ts:426–668): `seqValue`, `positionWord`, `noteRefValue`, `refValue`,
//! `listNumberOf`, `styleRefValue`, `bookmarkPages`. `fields.rs` dispatches to them.
//!
//! Regex sites (hand matchers over UTF-16, non-`u` regexes: `\s` = the JS space set, `\d` =
//! ASCII digits, `\w`/`\b` = ASCII word characters, `/i` = ASCII case folding):
//! `/\\s\s*(\d)/i`, `/\\r\s*(\d+)/i`, `/\\c\b/i`, `/\\h\b/i`, `/\\\*\s*(\w+)/`, `/\\p\b/i`,
//! `/\\[nrw]\b/i`, `/\\d\s+"([^"]*)"/i`, `/\\w\b/i`, `/\\[sn]\b/i`, `/\\s\b/i`, `/\.+$/`,
//! `/[\t\n\f\u000B]/g`, `/^pt/i`, `/^\s*STYLEREF\s+"?([1-9])"?/i`, `/^b(\d+)/` (a body line ref).

use indexmap::IndexMap;

use crate::error::{LResult, LayoutError, LimitKind};
use crate::keys::{LParaId, RunKey};
use crate::ldoc::LBlock;
use crate::model::NumKey;
use crate::word::{self, NumberMarker};
use uo_jsnum::math as jm;

use super::flow::{PSrc, fields_of};
use super::{Cx, Engine, js_eq};

fn utf16(s: &str) -> Box<[u16]> {
    s.encode_utf16().collect()
}

/// `instr.trim().split(/\s+/)[k]`.
pub(crate) fn tok(instr: &[u16], k: usize) -> Option<&[u16]> {
    uo_text::space::split_spaces(uo_text::space::trim(instr)).nth(k)
}

const BSL: u16 = 0x5C;

/// A non-`u` regex `\w` (word character) for `\b`.
fn is_word(u: u16) -> bool {
    u8::try_from(u).is_ok_and(uo_jsnum::text::is_js_word)
}

/// ASCII case-insensitive unit equality (`/i` without `u` folds no non-ASCII unit to ASCII).
fn ieq(u: u16, c: u8) -> bool {
    u8::try_from(u).is_ok_and(|b| b.eq_ignore_ascii_case(&c))
}

/// `/\\[letters]\b/i.test(s)`: a backslash, one of `letters`, then a non-word unit or the end.
fn has_switch(s: &[u16], letters: &[u8]) -> bool {
    (0..s.len()).any(|i| {
        s.get(i) == Some(&BSL)
            && s.get(i + 1).is_some_and(|&u| letters.iter().any(|&c| ieq(u, c)))
            && s.get(i + 2).is_none_or(|&u| !is_word(u))
    })
}

/// The first match of `/\\L\s*(\d)/i` (`many`: `(\d+)`): its digits.
fn switch_digits(s: &[u16], letter: u8, many: bool) -> Option<&[u16]> {
    for i in 0..s.len() {
        if s.get(i) != Some(&BSL) || !s.get(i + 1).is_some_and(|&u| ieq(u, letter)) {
            continue;
        }
        let mut j = i + 2;
        while s.get(j).is_some_and(|&u| uo_text::space::is_space(u)) {
            j += 1;
        }
        let d0 = j;
        while s.get(j).is_some_and(|&u| (0x30..=0x39).contains(&u)) && (many || j == d0) {
            j += 1;
        }
        if j > d0 {
            return s.get(d0..j);
        }
    }
    None
}

/// `/\\\*\s*(\w+)/.exec(s)?.[1]` (no `i`).
fn star_format(s: &[u16]) -> Option<&[u16]> {
    for i in 0..s.len() {
        if s.get(i) != Some(&BSL) || s.get(i + 1) != Some(&0x2A) {
            continue;
        }
        let mut j = i + 2;
        while s.get(j).is_some_and(|&u| uo_text::space::is_space(u)) {
            j += 1;
        }
        let w0 = j;
        while s.get(j).is_some_and(|&u| is_word(u)) {
            j += 1;
        }
        if j > w0 {
            return s.get(w0..j);
        }
    }
    None
}

/// `/\\d\s+"([^"]*)"/i.exec(s)?.[1]`.
fn d_separator(s: &[u16]) -> Option<&[u16]> {
    for i in 0..s.len() {
        if s.get(i) != Some(&BSL) || !s.get(i + 1).is_some_and(|&u| ieq(u, b'd')) {
            continue;
        }
        let mut j = i + 2;
        let sp0 = j;
        while s.get(j).is_some_and(|&u| uo_text::space::is_space(u)) {
            j += 1;
        }
        if j == sp0 || s.get(j) != Some(&0x22) {
            continue;
        }
        let q0 = j + 1;
        if let Some(q1) = s.get(q0..).and_then(|r| r.iter().position(|&u| u == 0x22)) {
            return s.get(q0..q0 + q1);
        }
    }
    None
}

/// `t.replace(/\.+$/, '')`.
fn strip_dots(t: &[u16]) -> Box<[u16]> {
    let n = t.iter().rev().take_while(|&&u| u == 0x2E).count();
    t.get(..t.len() - n).unwrap_or(&[]).into()
}

/// `.replace(/[\t\n\f\u000B]/g, ' ').trim()`.
fn clean_text(t: &[u16]) -> Box<[u16]> {
    let v: Vec<u16> = t.iter().map(|&u| if matches!(u, 0x09..=0x0C) { 0x20 } else { u }).collect();
    uo_text::space::trim(&v).into()
}

/// `/^pt/i.test(lang)`.
fn is_pt(lang: &str) -> bool {
    lang.as_bytes().get(..2).is_some_and(|p| p.eq_ignore_ascii_case(b"pt"))
}

fn is_es(lang: &str) -> bool {
    lang.as_bytes().get(..2).is_some_and(|p| p.eq_ignore_ascii_case(b"es"))
}

/// Wave 7c (W, item 4; oracle refbm): Word's REF result for an undefined bookmark (missing, without `w:bookmarkEnd`, or
/// ending before it starts) — measured in Word 16 pt-BR (every switch: `\h \n \r \w \p`); the English and Spanish
/// strings are Word's UI texts (not measured here).
pub(crate) fn ref_error(lang: &str) -> &'static str {
    if is_pt(lang) {
        "Erro! Fonte de referência não encontrada."
    } else if is_es(lang) {
        "¡Error! No se encuentra el origen de la referencia."
    } else {
        "Error! Reference source not found."
    }
}

/// Wave 7c (W, item 4; oracle refbm): Word's PAGEREF and NOTEREF result for an undefined bookmark (measured pt-BR).
pub(crate) fn bookmark_error(lang: &str) -> &'static str {
    if is_pt(lang) {
        "Erro! Indicador não definido."
    } else if is_es(lang) {
        "¡Error! Marcador no definido."
    } else {
        "Error! Bookmark not defined."
    }
}

/// Wave 7c (W, item 4; oracle refbm): a field value that is one of Word's reference error texts (REF, PAGEREF, NOTEREF,
/// STYLEREF) — Word writes them in BOLD (measured: LiberationSans-Bold in its PDF). Read by the paragraph preparation on
/// the value (the paragraph memo of the incremental relayout replays values, not flags).
#[must_use]
pub fn is_ref_error_text(v: &[u16]) -> bool {
    const ALL: [&str; 8] = [
        "Erro! Fonte de referência não encontrada.",
        "¡Error! No se encuentra el origen de la referencia.",
        "Error! Reference source not found.",
        "Erro! Indicador não definido.",
        "¡Error! Marcador no definido.",
        "Error! Bookmark not defined.",
        "Erro! Nenhum texto com o estilo especificado foi encontrado no documento.",
        "Error! No text of specified style in document.",
    ];
    // WordCraft study (fields, B21): Word's STYLEREF error for a name no style answers to, bold as the others (measured:
    // the line breaks of `oracle_wc/styleref` sr-body, sr-n)
    const UNKNOWN_STYLE: [(&str, &str); 3] = [
        ("Erro! Use a guia Página Inicial para aplicar ", " ao texto que deverá aparecer aqui."),
        ("Error! Use the Home tab to apply ", " to the text that you want to appear here."),
        ("¡Error! Utilice la pestaña Inicio para aplicar ", " al texto que desea que aparezca aquí."),
    ];
    let t = String::from_utf16_lossy(v);
    ALL.iter().any(|s| s.encode_utf16().eq(v.iter().copied()))
        || UNKNOWN_STYLE.iter().any(|(a, b)| t.len() > a.len() + b.len() && t.starts_with(a) && t.ends_with(b))
}

/// Whether derived paragraph `k` holds the start mark of bookmark `bm` (wave 7c, W, item 4).
fn para_holds_start(cx: &Cx<'_, '_>, k: LParaId, bm: &str) -> bool {
    cx.ldoc.para(k).is_some_and(|lp| {
        lp.runs.iter().any(|&rk| {
            crate::paint::model_run(cx.ldoc, rk)
                .and_then(|r| r.ext())
                .and_then(|e| e.bookmark.as_ref())
                .is_some_and(|m| &*m.name == bm && m.end != Some(true))
        })
    })
}

/// Wave 7c (W, item 4, fix round 1): the bookmark marks of every derived paragraph (see [`crate::ldoc::BookmarkMarkIndex`]):
/// the first paragraph that opens a name decides it — the text of the runs after its start mark up to the first end mark
/// with its id in that paragraph (images, text boxes, shapes and groups skipped), or `None` when the paragraph ends first.
fn bookmark_mark_index(ldoc: &crate::ldoc::LDoc<'_>) -> crate::ldoc::BookmarkMarkIndex {
    let mut ix = crate::ldoc::BookmarkMarkIndex::default();
    // (id, name, start in `text`) of the names open in the paragraph
    let mut open: Vec<(f64, Box<str>, usize)> = Vec::new();
    let mut text: Vec<u16> = Vec::new();
    for lp in &ldoc.paras {
        open.clear();
        text.clear();
        for &rk in &lp.runs {
            let Some(e) = crate::paint::model_run(ldoc, rk).and_then(|r| r.ext()) else {
                if !open.is_empty() {
                    text.extend_from_slice(ldoc.run_text(rk));
                }
                continue;
            };
            if let Some(m) = e.bookmark.as_ref() {
                if m.end == Some(true) {
                    let id = m.id.to_number();
                    let mut i = 0;
                    while i < open.len() {
                        if open.get(i).is_some_and(|o| js_eq(o.0, id)) {
                            let (_, name, at) = open.remove(i);
                            ix.insert(name, Some(text.get(at..).unwrap_or(&[]).to_vec()));
                        } else {
                            i += 1;
                        }
                    }
                } else if !ix.contains_key(&*m.name) && !open.iter().any(|o| *o.1 == *m.name) {
                    open.push((m.id.to_number(), Box::from(&*m.name), text.len()));
                }
                continue;
            }
            if !open.is_empty() && e.image.is_none() && e.textbox.is_none() && e.shape.is_none() && e.group.is_none() {
                text.extend_from_slice(ldoc.run_text(rk));
            }
        }
        for (_, name, _) in open.drain(..) {
            ix.insert(name, None);
        }
    }
    ix
}

/// Where a bookmark is, for Word's reference rules (wave 7c, W, item 4).
pub(crate) enum BmWhere {
    /// in the body index, with an end in its own block or after it
    Body,
    /// not defined for Word: missing, without an end, or ending before it starts
    Undefined,
    /// outside the body index (a header/footer, a text box, a note, a cell paragraph): the text of its range
    Elsewhere(Vec<u16>),
    /// a name the body index lacks, in a document read without bookmark marks: not knowable here
    Unknown,
}

/// `ToIntegerOrInfinity(x)`.
fn to_int_inf(x: f64) -> f64 {
    if x.is_nan() { 0.0 } else { jm::trunc(x) }
}

/// `text.slice(a, b)` of a JS string (`ToIntegerOrInfinity`, negatives from the end).
fn js_slice(t: &[u16], a: f64, b: f64) -> &[u16] {
    let len = f64::from(u32::try_from(t.len()).unwrap_or(u32::MAX));
    let rel = |x: f64| {
        let x = to_int_inf(x);
        if x < 0.0 { jm::max(len + x, 0.0) } else { jm::min(x, len) }
    };
    let (from, to) = (rel(a), rel(b));
    if from >= to {
        return &[];
    }
    // (both in [0, len]: integers below 2^32)
    t.get(uo_jsnum::conv::to_uint32(from) as usize..uo_jsnum::conv::to_uint32(to) as usize).unwrap_or(&[])
}

/// `SEQ \* format` names (a plain object in the TS: an `Object.prototype` name reads as absent
/// here, F7c-4).
fn seq_format(f: &[u16]) -> &'static str {
    match String::from_utf16_lossy(f).as_str() {
        "ARABIC" => "decimal",
        "ALPHABETIC" => "upperLetter",
        "alphabetic" => "lowerLetter",
        "ROMAN" => "upperRoman",
        "roman" => "lowerRoman",
        _ => "decimal",
    }
}

/// What a STYLEREF looks for (WordCraft study, fields B21).
enum SrMatch {
    /// a heading level (`STYLEREF 1`)
    Level(f64),
    /// a paragraph style id
    Para(Box<str>),
    /// a character style id
    Char(Box<str>),
}

/// The character style of a run (`w:rStyle`).
fn run_char_style(r: &crate::model::Run) -> Option<&str> {
    r.props.char_style.as_deref().or_else(|| r.rpr.as_deref().and_then(|p| p.r_style.as_deref())).filter(|s| !s.is_empty())
}

/// The name of `STYLEREF name …` (WordCraft study, fields B21): a quoted name (the quotes dropped), else the token up to
/// a space or a backslash; `None` without one.
fn styleref_target(instr: &[u16]) -> Option<String> {
    let s = uo_text::space::trim_start(instr);
    let kw = b"STYLEREF";
    if s.len() < kw.len() || !s.iter().zip(kw).all(|(&u, &c)| ieq(u, c)) {
        return None;
    }
    let rest = s.get(kw.len()..).unwrap_or(&[]);
    let r = uo_text::space::trim_start(rest);
    if r.len() == rest.len() {
        return None;
    }
    let name: &[u16] = if r.first() == Some(&0x22) {
        let body = r.get(1..).unwrap_or(&[]);
        body.get(..body.iter().position(|&u| u == 0x22).unwrap_or(body.len())).unwrap_or(&[])
    } else {
        r.get(..r.iter().position(|&u| uo_text::space::is_space(u) || u == BSL).unwrap_or(r.len())).unwrap_or(&[])
    };
    let name = String::from_utf16_lossy(name);
    let t = uo_jsnum::text::trim(&name);
    (!t.is_empty()).then(|| t.to_owned())
}

/// `n?.numId ? st.next(n.numId, n.ilvl ?? 0) : null` of `listNumberOf`.
fn list_next(st: &mut word::NumberingState, fmt: &crate::model::DocFormatting, r: &word::ResolvedPPr) -> Option<NumberMarker> {
    let n = r.ppr.num_pr.as_deref()?;
    let id = n.num_id.filter(|v| v.truthy())?;
    let ilvl = n.ilvl.filter(|v| !v.is_null()).unwrap_or(crate::model::JsNumber::Num(0.0));
    if !st.rules.word_lang {
        return st.next(fmt, id, ilvl);
    }
    // WordCraft study (fields, B26): the words of the marker in the language of the paragraph MARK (its direct run
    // properties, the paragraph style's, the document defaults), as the paragraph's own marker (`para.rs`)
    let of = |p: Option<&crate::model::RPr>| p.and_then(|p| p.lang.as_ref()).and_then(|l| l.val.clone()).filter(|v| !v.is_empty());
    let lang = of(r.ppr.r_pr.as_deref()).or_else(|| of(Some(&*r.style_rpr))).or_else(|| of(fmt.styles.doc_defaults.r_pr.as_deref()));
    st.next_in(fmt, id, ilvl, lang.as_deref())
}

/// A block index as a JS number.
fn num(i: usize) -> f64 {
    f64::from(u32::try_from(i).unwrap_or(u32::MAX))
}

/// The text length of a run as a JS number.
fn run_len(cx: &Cx<'_, '_>, k: RunKey) -> f64 {
    num(cx.ldoc.run_text(k).len())
}

/// [`Engine::bookmark_pages_with_numbers`]: each bookmark's page text and its page number.
pub type BookmarkPages = (IndexMap<Box<str>, Box<[u16]>>, IndexMap<Box<str>, f64>);

impl Engine {
    /// `seqValue(instr)` (engine.ts:426): `SEQ id [\* format] [\r n] [\c] [\h] [\s n]` — counters in
    /// document order; `\s n` resets the counter after a heading of level ≤ n since it last counted
    /// (oracle refs-lab, sr-*).
    pub(crate) fn seq_value(&mut self, instr: &[u16], lang: &str, cap: &mut word::TextCap) -> Option<Box<[u16]>> {
        let (v, hidden) = self.seq_number(instr)?;
        if hidden {
            return Some(Box::default());
        }
        let fmt = star_format(instr).map_or("decimal", seq_format);
        Some(word::format_number(v, Some(fmt), lang, cap).units().into())
    }

    /// The counter of `seqValue` (the sequence advanced as the TS does) and whether `\h` hides it (WordCraft study,
    /// fields B22: the format is then Word's `\*`, applied by the caller).
    pub(crate) fn seq_number(&mut self, instr: &[u16]) -> Option<(f64, bool)> {
        let id16 = uo_text::case::to_lowercase(tok(instr, 1)?).into_owned();
        if id16.is_empty() || id16.first() == Some(&BSL) {
            return None;
        }
        let id: Box<str> = String::from_utf16_lossy(&id16).into();
        let reset = switch_digits(instr, b's', false).map_or(f64::NAN, |d| uo_jsnum::parse::parse_int_utf16(d, 10));
        let flow = &mut self.ctx.flow;
        let since = flow.seq_heads.get(&id).copied().unwrap_or(0.0);
        let heads_len = num(flow.heads.len());
        if uo_jsnum::conv::truthy(reset) {
            // `heads.slice(since)` (`since` is a heads length: an integer)
            let from = to_int_inf(since);
            let from = if from < 0.0 { jm::max(heads_len + from, 0.0) } else { jm::min(from, heads_len) };
            let from = uo_jsnum::conv::to_uint32(from) as usize;
            if flow.heads.get(from..).is_some_and(|h| h.iter().any(|&l| l <= reset)) {
                flow.seq.insert(id.clone(), 0.0);
            }
        }
        flow.seq_heads.insert(id.clone(), heads_len);
        let mut v = flow.seq.get(&id).copied().unwrap_or(0.0);
        if let Some(r) = switch_digits(instr, b'r', true) {
            v = uo_jsnum::parse::parse_int_utf16(r, 10);
        } else if !has_switch(instr, b"c") {
            v += 1.0;
        }
        flow.seq.insert(id, v);
        Some((v, has_switch(instr, b"h")))
    }

    /// `positionWord(bm, block, at)` (engine.ts:450): "above"/"below" (REF/NOTEREF `\p`): the field
    /// comes after or before the bookmark in document order; in the same paragraph the offset
    /// decides (oracle refs, ref-pos). Without the field's position, the cache stays.
    fn position_word(&mut self, cx: &Cx<'_, '_>, bm: &str, block: Option<i64>, at: Option<f64>) -> Option<Box<[u16]>> {
        let b = cx.ldoc.view.bookmarks().starts.get(bm)?;
        let block = block.filter(|b| *b >= 0)?;
        let bk = b.block.to_number();
        let off = b.offset.as_f64().unwrap_or(0.0);
        let blk = f64::from(u32::try_from(block).unwrap_or(u32::MAX));
        let below = blk < bk || (js_eq(blk, bk) && at.is_some_and(|a| a < off));
        // WordCraft study (fields, B24): Word writes the word in the language of the field paragraph's STYLE (its run
        // properties, then the document defaults; measured: the field runs and the mark's direct lang do not count);
        // the frozen TS in the UI language (`ts_ref_relative`), pt or else English (engine.ts:450 knows no Spanish)
        if cx.opts.ts_ref_relative {
            let pt = is_pt(&cx.opts.lang);
            return Some(utf16(match (below, pt) {
                (true, true) => "abaixo",
                (true, false) => "below",
                (false, true) => "acima",
                (false, false) => "above",
            }));
        }
        let lang = self.para_style_lang(cx, block);
        let w = match (below, uo_docx::word::numwords::WordsLang::of(&lang)) {
            (true, uo_docx::word::numwords::WordsLang::Pt) => "abaixo",
            (true, uo_docx::word::numwords::WordsLang::En) => "below",
            (true, uo_docx::word::numwords::WordsLang::Es) => "más adelante",
            (false, uo_docx::word::numwords::WordsLang::Pt) => "acima",
            (false, uo_docx::word::numwords::WordsLang::En) => "above",
            (false, uo_docx::word::numwords::WordsLang::Es) => "más atrás",
        };
        Some(utf16(w))
    }

    /// WordCraft study (fields, B24): the `w:lang` of a flow block's paragraph style (its run properties), else of the
    /// document defaults, else the layout's language.
    fn para_style_lang(&mut self, cx: &Cx<'_, '_>, block: i64) -> Box<str> {
        let fmt = cx.ldoc.view.formatting();
        let style = usize::try_from(block).ok().and_then(|i| match self.flow_block(cx, i) {
            Some(LBlock::Paragraph(k)) => fields_of(cx.ldoc, &PSrc::Doc(k))
                .ok()
                .map(|(ppr, props, _, _)| word::resolve_ppr(fmt, &mut self.ctx.resolver, ppr.as_deref(), &props, None).style_rpr),
            _ => None,
        });
        let of = |r: Option<&crate::model::RPr>| r.and_then(|r| r.lang.as_ref()).and_then(|l| l.val.clone()).filter(|v| !v.is_empty());
        of(style.as_deref()).or_else(|| of(fmt.styles.doc_defaults.r_pr.as_deref())).unwrap_or_else(|| cx.opts.lang.clone())
    }

    /// Wave 7c (W, item 4): how Word sees bookmark `bm` (see [`BmWhere`]). The body index decides when the bookmark's
    /// start is a body paragraph that holds it; otherwise (a name outside the index, a table block, a text box's bookmark
    /// indexed on its anchor paragraph) the bookmark marks of every derived paragraph decide — headers/footers, notes,
    /// text boxes, cells. The marks exist only in a lossless read (the editor's session: `ReadOptions::sources`); without
    /// them a name the index lacks is `Unknown` (the frozen TS's behaviour stays: the cached result).
    ///
    /// Fix round 1 (reviewer W/4): the marks are read once per layout call — [`crate::ldoc::BookmarkMarkIndex`], built on
    /// the first lookup that needs it, its runs charged to the budget — not once per reference and pass.
    pub(crate) fn bookmark_where(&self, cx: &mut Cx<'_, '_>, bm: &str) -> LResult<BmWhere> {
        let bmk = cx.ldoc.view.bookmarks();
        let marks = cx.ldoc.bookmark_marks;
        if let Some(st) = bmk.starts.get(bm) {
            let b = st.block.to_number();
            if let Some(k) = self.bookmark_para(cx, b)
                && (!marks || para_holds_start(cx, k, bm))
            {
                let Some(end) = bmk.ends.get(bm) else { return Ok(BmWhere::Undefined) };
                let eb = end.block.to_number();
                if eb < b || (js_eq(eb, b) && end.offset.to_number() < st.offset.as_f64().unwrap_or(0.0)) {
                    return Ok(BmWhere::Undefined);
                }
                return Ok(BmWhere::Body);
            }
        }
        if !marks {
            // without the marks the index still knows an end (a bookmark in a cell is indexed on its table's block)
            let Some(st) = bmk.starts.get(bm) else { return Ok(BmWhere::Unknown) };
            let b = st.block.to_number();
            return Ok(match bmk.ends.get(bm) {
                Some(end)
                    if end.block.to_number() > b
                        || (js_eq(end.block.to_number(), b) && end.offset.to_number() >= st.offset.as_f64().unwrap_or(0.0)) =>
                {
                    BmWhere::Body
                }
                _ => BmWhere::Undefined,
            });
        }
        // every story can hold it: the whole document is read (LA7)
        self.note_read_all();
        if cx.ldoc.bookmark_mark_index.get().is_none() {
            let runs: usize = cx.ldoc.paras.iter().map(|lp| lp.runs.len()).sum();
            cx.budget.charge(u64::try_from(runs).unwrap_or(u64::MAX))?;
            let _ = cx.ldoc.bookmark_mark_index.set(bookmark_mark_index(cx.ldoc));
        }
        Ok(match cx.ldoc.bookmark_mark_index.get().and_then(|ix| ix.get(bm)) {
            Some(Some(text)) => BmWhere::Elsewhere(text.clone()),
            _ => BmWhere::Undefined,
        })
    }

    /// The flow block of a bookmark (`this.full.blocks[b]`) when it is a paragraph.
    fn bookmark_para(&self, cx: &Cx<'_, '_>, b: f64) -> Option<LParaId> {
        match super::index_of(b).and_then(|i| self.flow_block(cx, i)) {
            Some(LBlock::Paragraph(k)) => Some(k),
            _ => None,
        }
    }

    /// `noteRefValue(instr, block, at)` (engine.ts:461): NOTEREF bookmark — the mark of the note
    /// whose reference is in the bookmark's range.
    pub(crate) fn note_ref_value(
        &mut self,
        cx: &mut Cx<'_, '_>,
        instr: &[u16],
        block: Option<i64>,
        at: Option<f64>,
    ) -> LResult<Option<Box<[u16]>>> {
        let Some(bm) = tok(instr, 1).filter(|t| !t.is_empty()).map(String::from_utf16_lossy) else { return Ok(None) };
        // wave 7c (W, item 4; oracle refbm): an undefined bookmark is Word's error (the frozen TS keeps the cached result)
        if !cx.opts.ts_ref_bookmarks && matches!(self.bookmark_where(cx, &bm)?, BmWhere::Undefined) {
            return Ok(Some(utf16(bookmark_error(&cx.opts.lang))));
        }
        Ok(self.note_ref_body(cx, &bm, instr, block, at))
    }

    /// `noteRefValue`'s body once the bookmark is known (wave 7c, W, fix round 1: split from [`Engine::note_ref_value`]).
    fn note_ref_body(&mut self, cx: &Cx<'_, '_>, bm: &str, instr: &[u16], block: Option<i64>, at: Option<f64>) -> Option<Box<[u16]>> {
        if has_switch(instr, b"p") {
            return self.position_word(cx, bm, block, at);
        }
        let bmk = cx.ldoc.view.bookmarks();
        let st = bmk.starts.get(bm)?;
        let b = st.block.to_number();
        let k = self.bookmark_para(cx, b)?;
        let from = st.offset.as_f64().unwrap_or(0.0);
        let to = match bmk.ends.get(bm) {
            Some(e) if js_eq(e.block.to_number(), b) => e.offset.to_number(),
            _ => f64::INFINITY,
        };
        let lp = cx.ldoc.para(k)?;
        let mut acc = 0.0;
        for &rk in &lp.runs {
            let rs = acc;
            acc += run_len(cx, rk);
            let Some(nr) = crate::paint::model_run(cx.ldoc, rk).and_then(|r| r.ext()).and_then(|e| e.note_ref.as_ref()) else { continue };
            if nr.custom == Some(true) || rs < from || rs > to || (js_eq(rs, to) && to > from) {
                continue;
            }
            self.found.used_note_ref = true;
            let kind = if &*nr.kind == "footnote" { crate::context::NoteKindFlow::Footnote } else { crate::context::NoteKindFlow::Endnote };
            if let Some(m) = self.ctx.note_mark_of(kind, NumKey::new(nr.id.to_number())) {
                return Some(m.into());
            }
            // `${kind[0]}${id}`: the first unit of the kind string
            let mut key = String::new();
            key.extend(nr.kind.chars().next());
            key.push_str(&uo_jsnum::format::to_js_string(nr.id.to_number()));
            return cx.input.note_ref_guess.get(key.as_str()).cloned();
        }
        None
    }

    /// `refValue(instr, block, at)` (engine.ts:485): REF bookmark — the text of the bookmark's
    /// range (without a known end, its paragraph's); `\n \r \w` = the paragraph number; `\p` =
    /// above/below.
    pub(crate) fn ref_value(
        &mut self,
        cx: &mut Cx<'_, '_>,
        instr: &[u16],
        block: Option<i64>,
        at: Option<f64>,
    ) -> LResult<Option<Box<[u16]>>> {
        let Some(bm) = tok(instr, 1).filter(|t| !t.is_empty()).map(String::from_utf16_lossy) else { return Ok(None) };
        // wave 7c (W, item 4; oracle refbm): Word's rules — an undefined bookmark (missing, no end, end before start) is
        // its error text with every switch; a bookmark outside the body (header/footer, text box, note, cell) gives its
        // range's text; a range over several paragraphs gives them all (a line break between them). The frozen TS
        // (`ts_ref_bookmarks`) shows the start paragraph's text, or keeps the cached result.
        if !cx.opts.ts_ref_bookmarks {
            match self.bookmark_where(cx, &bm)? {
                BmWhere::Undefined => return Ok(Some(utf16(ref_error(&cx.opts.lang)))),
                BmWhere::Elsewhere(t) if !has_switch(instr, b"pnrw") => return Ok((!t.is_empty()).then(|| t.into_boxed_slice())),
                _ => {}
            }
        }
        if has_switch(instr, b"p") {
            return Ok(self.position_word(cx, &bm, block, at));
        }
        let bmk = cx.ldoc.view.bookmarks();
        if has_switch(instr, b"nrw") {
            // the paragraph number: Word recomputes it on export; without the final dots, and "0"
            // when the bookmark's paragraph is not numbered (oracle numbering3, ref-num); \w = the
            // full context (\d between the levels); \n and \r = the level's number (oracle
            // refs-lab, rn-*)
            let Some(st) = bmk.starts.get(bm.as_str()) else { return Ok(None) };
            let bi = st.block.to_number();
            if self.bookmark_para(cx, bi).is_none() {
                return Ok(None);
            }
            let Some(m) = self.list_number_of(cx, bi)?.filter(|m| !m.text.is_empty()) else { return Ok(Some(utf16("0"))) };
            // WordCraft study (fields, B24): `\r` is Word's relative context — the full number less the leading levels the
            // field's own list position shares with the target (same list, equal parts); the frozen TS gives the level's
            // own number (`ts_ref_relative`)
            if !cx.opts.ts_ref_relative
                && has_switch(instr, b"r")
                && !has_switch(instr, b"w")
                && let Some(parts) = m.parts.as_ref()
            {
                let mut own = match block.filter(|b| *b >= 0) {
                    Some(b) => self.list_number_of(cx, f64::from(u32::try_from(b).unwrap_or(u32::MAX)))?,
                    None => None,
                };
                // an unnumbered field paragraph stands where the list last was: the nearest numbered paragraph of the
                // target's list before it (oracle `refs/ref-num`: after "b)" of "2.", `\r` gives "b)"; b24 `fim`, after
                // "2.", the full "1.a)i"; before the list, the full number)
                if own.as_ref().is_none_or(|o| o.text.is_empty())
                    && let Some(b) = block.filter(|b| *b > 0).and_then(|b| u32::try_from(b).ok())
                {
                    own = None;
                    for k in (0..b).rev() {
                        cx.budget.charge(1)?;
                        if let Some(o) = self.list_number_of(cx, f64::from(k))?.filter(|o| !o.text.is_empty() && o.list_key == m.list_key) {
                            own = Some(o);
                            break;
                        }
                    }
                }
                let shared = match own.as_ref().filter(|o| o.list_key == m.list_key).and_then(|o| o.parts.as_ref()) {
                    Some(fp) => fp.iter().zip(parts.iter()).take_while(|(a, b)| a == b).count().min(parts.len().saturating_sub(1)),
                    None => 0,
                };
                let mut v = Vec::new();
                for p in parts.iter().skip(shared) {
                    v.extend_from_slice(p.units());
                }
                return Ok(Some(strip_dots(&v)));
            }
            let t: Vec<u16> = match (&m.parts, has_switch(instr, b"w")) {
                (Some(parts), true) => {
                    let sep = d_separator(instr).unwrap_or(&[]);
                    let mut v = Vec::new();
                    for (i, p) in parts.iter().enumerate() {
                        if i > 0 {
                            v.extend_from_slice(sep);
                        }
                        v.extend_from_slice(p.units());
                    }
                    v
                }
                _ => m.text.units().to_vec(),
            };
            return Ok(Some(strip_dots(&t)));
        }
        let Some(st) = bmk.starts.get(bm.as_str()) else { return Ok(None) };
        let b = st.block.to_number();
        let Some(k) = self.bookmark_para(cx, b) else { return Ok(None) };
        let lp = cx.ldoc.para(k).ok_or(LayoutError::Internal("REF: paragraph"))?;
        let ext = |rk: RunKey| crate::paint::model_run(cx.ldoc, rk).and_then(|r| r.ext());
        // wave 7c (W, item 4; oracle refbm rb-multi, rb-multi3): the range goes on in later blocks — Word gives every
        // paragraph of it (each break a line break here: the paragraphs' own formatting is not carried)
        if !cx.opts.ts_ref_bookmarks
            && let Some(end) = bmk.ends.get(bm.as_str())
            && end.block.to_number() > b
        {
            let piece = |k: LParaId, from: f64, to: f64, out: &mut Vec<u16>| {
                let Some(lp) = cx.ldoc.para(k) else { return };
                let mut acc = 0.0;
                for &rk in &lp.runs {
                    let rs = acc;
                    let t = cx.ldoc.run_text(rk);
                    acc += num(t.len());
                    if ext(rk).is_some_and(|e| e.image.is_some() || e.textbox.is_some() || e.shape.is_some() || e.group.is_some()) {
                        continue;
                    }
                    out.extend_from_slice(js_slice(t, jm::max(0.0, from - rs), jm::max(0.0, to - rs)));
                }
            };
            let mut text = Vec::new();
            piece(k, st.offset.to_number(), f64::INFINITY, &mut text);
            let last = end.block.to_number();
            let mut i = b + 1.0;
            while i <= last {
                cx.budget.charge(1)?;
                // (a traced read of each block: LA7)
                if let Some(q) = self.bookmark_para(cx, i) {
                    text.push(0x0a);
                    piece(q, 0.0, if js_eq(i, last) { end.offset.to_number() } else { f64::INFINITY }, &mut text);
                }
                i += 1.0;
            }
            return Ok((!text.is_empty()).then(|| text.into_boxed_slice()));
        }
        if let Some(end) = bmk.ends.get(bm.as_str())
            && js_eq(end.block.to_number(), b)
        {
            // the exact range (fields by their cached result: "Figura 1 - Gráfico")
            let from = st.offset.to_number();
            let (mut text, mut acc) = (Vec::new(), 0.0);
            for &rk in &lp.runs {
                let rs = acc;
                let t = cx.ldoc.run_text(rk);
                acc += num(t.len());
                if ext(rk).is_some_and(|e| e.image.is_some() || e.textbox.is_some() || e.shape.is_some() || e.group.is_some()) {
                    continue;
                }
                text.extend_from_slice(js_slice(t, jm::max(0.0, from - rs), jm::max(0.0, end.offset.to_number() - rs)));
            }
            return Ok((!text.is_empty()).then(|| text.into_boxed_slice()));
        }
        let mut all = Vec::new();
        for &rk in &lp.runs {
            if ext(rk).is_some_and(|e| e.field.is_some() || e.image.is_some() || e.textbox.is_some()) {
                continue;
            }
            all.extend_from_slice(cx.ldoc.run_text(rk));
        }
        let t = clean_text(&all);
        Ok((!t.is_empty()).then_some(t))
    }

    /// `listNumberOf(block)` (engine.ts:525): the list number of each paragraph of the flow (block
    /// → marker), in document order, computed once per pass over the blocks of the flow walked at
    /// the first call (cell paragraphs count without a marker of their own).
    pub(crate) fn list_number_of(&mut self, cx: &mut Cx<'_, '_>, block: f64) -> LResult<Option<NumberMarker>> {
        if self.list_texts.is_none() {
            // the list markers of every block (LA7)
            self.note_read_all();
            let fmt = cx.ldoc.view.formatting();
            let mut st = word::numbering_state_for(cx.opts);
            let mut out: IndexMap<u32, Option<NumberMarker>> = IndexMap::new();
            let n = self.story_len(cx);
            let mut i = 0usize;
            while num(i) < n {
                cx.budget.charge(1)?;
                match self.flow_block(cx, i) {
                    Some(LBlock::Paragraph(k)) => {
                        let (ppr, props, _, _) = fields_of(cx.ldoc, &PSrc::Doc(k))?;
                        let r = word::resolve_ppr(fmt, &mut self.ctx.resolver, ppr.as_deref(), &props, None);
                        let m = list_next(&mut st, fmt, &r);
                        out.insert(u32::try_from(i).map_err(|_| LayoutError::Limit(LimitKind::Capacity))?, m);
                    }
                    Some(LBlock::Table { block: tb }) => {
                        if let Some(t) = self.flow_table(cx, i, tb) {
                            self.list_visit_table(cx, &mut st, t, 0)?;
                        }
                    }
                    None => {}
                }
                i += 1;
            }
            self.list_texts = Some(out);
        }
        let key = super::index_of(block).and_then(|i| u32::try_from(i).ok());
        Ok(key.and_then(|k| self.list_texts.as_ref().and_then(|m| m.get(&k).cloned())).flatten())
    }

    /// The model table of flow block `i` (`LBlock::Table { block }`): from the body story, or
    /// from its endnote's story in the endnote flow.
    pub(crate) fn flow_table<'a>(&self, cx: &Cx<'_, 'a>, i: usize, block: u32) -> Option<&'a crate::model::Table> {
        self.note_read(i);
        let story = match &self.en_blocks {
            Some(en) => en.get(i)?.story?,
            None => self.story,
        };
        let model = cx.ldoc.stories.get(story as usize).and_then(|s| s.sref).and_then(|r| cx.ldoc.view.story(&r))?;
        match model.blocks().nth(block as usize)? {
            uo_docx::model::story::Block::Table(t) => Some(t),
            uo_docx::model::story::Block::Paragraph(_) => None,
        }
    }

    fn list_visit_table(&mut self, cx: &mut Cx<'_, '_>, st: &mut word::NumberingState, t: &crate::model::Table, depth: u32) -> LResult<()> {
        if depth > cx.opts.limits.max_depth {
            return Err(LayoutError::Limit(LimitKind::Depth));
        }
        let fmt = cx.ldoc.view.formatting();
        for row in &t.rows {
            for cell in &row.cells {
                for b in cell.content.blocks() {
                    cx.budget.charge(1)?;
                    match b {
                        uo_docx::model::story::Block::Paragraph(p) => {
                            let r = word::resolve_ppr(fmt, &mut self.ctx.resolver, p.ppr.as_deref(), &p.props, None);
                            let _ = list_next(st, fmt, &r);
                        }
                        uo_docx::model::story::Block::Table(t2) => self.list_visit_table(cx, st, t2, depth + 1)?,
                    }
                }
            }
        }
        Ok(())
    }

    /// `headingStyleLevel(p)` of flow block `i` (0 = not a paragraph, or another style).
    fn block_heading_level(&self, cx: &Cx<'_, '_>, i: usize) -> f64 {
        let Some(LBlock::Paragraph(k)) = self.flow_block(cx, i) else { return 0.0 };
        fields_of(cx.ldoc, &PSrc::Doc(k)).map_or(0.0, |(ppr, props, _, _)| super::flow::heading_style_level_of(cx, &props, ppr.as_deref()))
    }

    /// `styleRefValue(instr, block)` (engine.ts:565): STYLEREF n in the body — the nearest heading
    /// of level n BEFORE the field (its own paragraph included); without one, the first after
    /// (oracle refs-lab, sr-*). No switch: the heading's text; \n: its number (no final dot; "0"
    /// without one); \s: only the numbers. No heading of the level: Word's error text.
    pub(crate) fn style_ref_value(&mut self, cx: &mut Cx<'_, '_>, instr: &[u16], block: Option<i64>) -> LResult<Option<Box<[u16]>>> {
        // `/^\s*STYLEREF\s+"?([1-9])"?/i`
        let s = uo_text::space::trim_start(instr);
        let kw = b"STYLEREF";
        if s.len() < kw.len() || !s.iter().zip(kw).all(|(&u, &c)| ieq(u, c)) {
            return Ok(None);
        }
        let rest = s.get(kw.len()..).unwrap_or(&[]);
        let r2 = uo_text::space::trim_start(rest);
        if r2.len() == rest.len() {
            return Ok(None);
        }
        let r2 = if r2.first() == Some(&0x22) { r2.get(1..).unwrap_or(&[]) } else { r2 };
        let n = match r2.first() {
            Some(&d @ 0x31..=0x39) => f64::from(d - 0x30),
            _ => return Ok(None),
        };
        let Some(block) = block.filter(|b| *b >= 0).and_then(|b| usize::try_from(b).ok()) else { return Ok(None) };
        let len = self.story_len(cx);
        let start = jm::min(num(block), len - 1.0);
        let mut hit: Option<usize> = None;
        let mut i = start;
        while i >= 0.0 && hit.is_none() {
            let ix = uo_jsnum::conv::to_uint32(i) as usize;
            if self.block_heading_level(cx, ix) == n {
                hit = Some(ix);
            }
            i -= 1.0;
        }
        let mut i = block.saturating_add(1);
        while num(i) < len && hit.is_none() {
            if self.block_heading_level(cx, i) == n {
                hit = Some(i);
            }
            i += 1;
        }
        let Some(hit) = hit else {
            return Ok(Some(utf16(if is_pt(&cx.opts.lang) {
                "Erro! Nenhum texto com o estilo especificado foi encontrado no documento."
            } else {
                "Error! No text of specified style in document."
            })));
        };
        if has_switch(instr, b"sn") {
            let Some(m) = self.list_number_of(cx, num(hit))?.filter(|m| !m.text.is_empty()) else { return Ok(Some(utf16("0"))) };
            let t: Vec<u16> = if has_switch(instr, b"s") {
                let bullet = m.level.get(cx.ldoc.view.formatting()).and_then(|l| l.num_fmt.as_deref()) == Some("bullet");
                if bullet { m.text.units().to_vec() } else { m.chapter.as_ref().map(|c| c.units().to_vec()).unwrap_or_default() }
            } else {
                m.text.units().to_vec()
            };
            return Ok(Some(strip_dots(&t)));
        }
        let Some(LBlock::Paragraph(k)) = self.flow_block(cx, hit) else { return Ok(None) };
        let lp = cx.ldoc.para(k).ok_or(LayoutError::Internal("STYLEREF: paragraph"))?;
        let mut all = Vec::new();
        for &rk in &lp.runs {
            let skip = crate::paint::model_run(cx.ldoc, rk)
                .and_then(|r| r.ext())
                .is_some_and(|e| e.image.is_some() || e.textbox.is_some() || e.shape.is_some() || e.group.is_some());
            if !skip {
                all.extend_from_slice(cx.ldoc.run_text(rk));
            }
        }
        Ok(Some(clean_text(&all)))
    }

    /// WordCraft study (fields, B21): STYLEREF as Word evaluates it (measured: `oracle_wc/styleref`). The target is a
    /// heading level (`STYLEREF 1`) or a style NAME (`uo_docx::word::style_names`: the UI name, a custom name, an alias);
    /// a name no style answers to is Word's "Use a guia Página Inicial…" error, a style no text has its "Nenhum texto…".
    /// In the body: the nearest paragraph with the style before the field (its own included), else the first after. In a
    /// header/footer (`block` = `None`), per page `ps`: the first paragraph with the style on the page (`\l`: the last),
    /// else the nearest before the page, else the first after it — over the previous pass's body lines per page
    /// (`PassInput::page_ranges_guess`; the first pass reads from the start). A paragraph style matches its own
    /// paragraphs (no basedOn inheritance), a character style the paragraph holding a run of it (the text: that run
    /// group). `\n \r \w \s` give the paragraph's number without its final period (`\w`: the full context, `\s`:
    /// the numbers only). The frozen TS (`ts_styleref_per_page`) keeps [`Engine::style_ref_value`].
    pub(crate) fn style_ref_at(
        &mut self,
        cx: &mut Cx<'_, '_>,
        instr: &[u16],
        block: Option<i64>,
        ps: crate::keys::PageIx,
    ) -> LResult<Option<Box<[u16]>>> {
        if cx.opts.ts_styleref_per_page {
            return self.style_ref_value(cx, instr, block);
        }
        let Some(name) = styleref_target(instr) else { return Ok(None) };
        let fmt = cx.ldoc.view.formatting();
        let want = match name.as_bytes() {
            [d @ b'1'..=b'9'] => SrMatch::Level(f64::from(d - b'0')),
            _ => match uo_docx::word::style_names::style_by_ui_name(fmt, &name, &cx.opts.lang) {
                Some(uo_docx::word::style_names::StyleTarget::Para(id)) => SrMatch::Para(id),
                Some(uo_docx::word::style_names::StyleTarget::Char(id)) => SrMatch::Char(id),
                None => return Ok(Some(utf16(&uo_docx::word::style_names::style_ref_unknown_error(&cx.opts.lang, &name)))),
            },
        };
        let len = self.story_len(cx);
        let n = if len > 0.0 { uo_jsnum::conv::to_uint32(len) as usize } else { 0 };
        let last = has_switch(instr, b"l");
        let mut found: Option<usize> = None;
        let try_range = |e: &mut Self, cx: &mut Cx<'_, '_>, it: &mut dyn Iterator<Item = usize>| -> LResult<Option<usize>> {
            for i in it {
                cx.budget.charge(1)?;
                if e.styleref_matches(cx, i, &want) {
                    return Ok(Some(i));
                }
            }
            Ok(None)
        };
        match block.filter(|b| *b >= 0).and_then(|b| usize::try_from(b).ok()) {
            Some(b) => {
                let from = b.min(n.saturating_sub(1));
                if n > 0 {
                    found = try_range(self, cx, &mut (0..=from).rev())?;
                }
                if found.is_none() {
                    found = try_range(self, cx, &mut (b.saturating_add(1)..n))?;
                }
            }
            None => {
                self.found.used_styleref_hf = true;
                let range = cx.input.page_ranges_guess.get(ps.0 as usize).copied().flatten().filter(|&(a, z)| a <= z && z < n);
                match range {
                    Some((a, z)) => {
                        found = if last { try_range(self, cx, &mut (a..=z).rev())? } else { try_range(self, cx, &mut (a..=z))? };
                        if found.is_none() {
                            found = try_range(self, cx, &mut (0..a).rev())?;
                        }
                        if found.is_none() {
                            found = try_range(self, cx, &mut (z + 1..n))?;
                        }
                    }
                    None => found = try_range(self, cx, &mut (0..n))?,
                }
            }
        }
        let Some(hit) = found else { return Ok(Some(utf16(uo_docx::word::style_names::style_ref_no_text_error(&cx.opts.lang)))) };
        if !matches!(want, SrMatch::Char(_)) && has_switch(instr, b"nrws") {
            let Some(m) = self.list_number_of(cx, num(hit))?.filter(|m| !m.text.is_empty()) else { return Ok(Some(utf16("0"))) };
            let t: Vec<u16> = if has_switch(instr, b"s") {
                let bullet = m.level.get(cx.ldoc.view.formatting()).and_then(|l| l.num_fmt.as_deref()) == Some("bullet");
                if bullet { m.text.units().to_vec() } else { m.chapter.as_ref().map(|c| c.units().to_vec()).unwrap_or_default() }
            } else if has_switch(instr, b"w") {
                m.parts.as_ref().map_or_else(|| m.text.units().to_vec(), |ps| ps.iter().flat_map(|p| p.units().iter().copied()).collect())
            } else {
                m.text.units().to_vec()
            };
            return Ok(Some(strip_dots(&t)));
        }
        let Some(LBlock::Paragraph(k)) = self.flow_block(cx, hit) else { return Ok(None) };
        let lp = cx.ldoc.para(k).ok_or(LayoutError::Internal("STYLEREF: paragraph"))?;
        let mut all = Vec::new();
        let mut started = false;
        for &rk in &lp.runs {
            let run = crate::paint::model_run(cx.ldoc, rk);
            let skip = run
                .and_then(|r| r.ext())
                .is_some_and(|e| e.image.is_some() || e.textbox.is_some() || e.shape.is_some() || e.group.is_some());
            if let SrMatch::Char(id) = &want {
                // the first group of runs with the character style
                let styled = run.is_some_and(|r| run_char_style(r) == Some(&**id));
                if !styled {
                    if started {
                        break;
                    }
                    continue;
                }
                started = true;
            }
            if !skip {
                all.extend_from_slice(cx.ldoc.run_text(rk));
            }
        }
        Ok(Some(clean_text(&all)))
    }

    /// Whether flow block `i` is what a STYLEREF looks for (WordCraft study, fields B21).
    fn styleref_matches(&self, cx: &Cx<'_, '_>, i: usize, want: &SrMatch) -> bool {
        match want {
            SrMatch::Level(n) => self.block_heading_level(cx, i) == *n,
            SrMatch::Para(id) => {
                let Some(LBlock::Paragraph(k)) = self.flow_block(cx, i) else { return false };
                fields_of(cx.ldoc, &PSrc::Doc(k)).is_ok_and(|(ppr, props, _, _)| {
                    props.style_id.as_deref().or_else(|| ppr.as_deref().and_then(|q| q.p_style.as_deref())) == Some(&**id)
                })
            }
            SrMatch::Char(id) => {
                let Some(LBlock::Paragraph(k)) = self.flow_block(cx, i) else { return false };
                cx.ldoc.para(k).is_some_and(|lp| {
                    lp.runs.iter().any(|&rk| crate::paint::model_run(cx.ldoc, rk).is_some_and(|r| run_char_style(r) == Some(&**id)))
                })
            }
        }
    }

    /// `bookmarkPages()` (engine.ts:638): the displayed (formatted) page of each bookmark's
    /// paragraph — inside the paragraph the line holding the bookmark's offset (after a page break
    /// in the middle of the paragraph, the next page — oracle tocref-pbstart); a bookmark in a
    /// block without lines takes the next laid-out line (≤ 50 blocks on).
    #[must_use]
    pub fn bookmark_pages(&self, ldoc: &crate::ldoc::LDoc<'_>, opts: &crate::options::LayoutOptions) -> IndexMap<Box<str>, Box<[u16]>> {
        self.bookmark_pages_with_numbers(ldoc, opts).0
    }

    /// [`Engine::bookmark_pages`] and the page NUMBER of each bookmark (WordCraft study, fields B22: PAGEREF with a `\*`
    /// format numbers the page itself).
    #[must_use]
    pub fn bookmark_pages_with_numbers(&self, ldoc: &crate::ldoc::LDoc<'_>, opts: &crate::options::LayoutOptions) -> BookmarkPages {
        let mut out: IndexMap<Box<str>, Box<[u16]>> = IndexMap::new();
        let mut nums: IndexMap<Box<str>, f64> = IndexMap::new();
        // `/^b(\d+)/.exec(l.ref)`: a body line, by its first block
        let mut first_page: IndexMap<u32, usize> = IndexMap::new();
        let mut para_lines: IndexMap<u32, Vec<(u32, usize)>> = IndexMap::new();
        for (pi, ps) in self.pages.iter().enumerate() {
            for l in &ps.page.lines {
                if l.at.story != crate::output::LineStory::Body {
                    continue;
                }
                let Some(&b) = l.at.path.0.first() else { continue };
                first_page.entry(b).or_insert(pi);
                if l.at.path.0.len() == 1 {
                    para_lines.entry(b).or_default().push((l.end, pi));
                }
            }
        }
        let mut cap = word::text_cap(&opts.limits);
        // Word writes the bookmark page as its PAGE field: the chapter numbers of a `w:chapStyle` section before it
        // (ED-713, measured; `chapter_of` over this pass's headings). The frozen TS writes the bare number.
        let chapters = (!opts.ts_pageref_without_chapter && self.geoms.iter().any(|g| g.chap.is_some())).then(|| self.chapter_index());
        let key_of = |b: f64| super::index_of(b).and_then(|i| u32::try_from(i).ok());
        for (name, st) in &ldoc.view.bookmarks().starts {
            let block = st.block.to_number();
            let mut pg: Option<usize> = None;
            if let Some(off) = st.offset.as_f64() {
                pg = key_of(block)
                    .and_then(|k| para_lines.get(&k))
                    .and_then(|ls| ls.iter().find(|(end, _)| f64::from(*end) > off))
                    .map(|x| x.1);
            }
            let mut b = block;
            while b < block + 50.0 && pg.is_none() {
                pg = key_of(b).and_then(|k| first_page.get(&k).copied());
                b += 1.0;
            }
            let Some((pi, ps)) = pg.and_then(|pi| self.pages.get(pi).map(|ps| (pi, ps))) else { continue };
            nums.insert(name.clone(), ps.page.number);
            let g = self.geoms.get(ps.geom as usize);
            let fmt = g.and_then(|g| g.pg_fmt.clone()).unwrap_or_else(|| "decimal".into());
            let num = word::format_number(ps.page.number, Some(&fmt), &opts.lang, &mut cap);
            // `chapter_of(level, page)`: the first heading of the level, then the last one on a page up to this one
            let chap = chapters.as_ref().zip(g.and_then(|g| g.chap.as_ref())).and_then(|(idx, c)| {
                let key = if c.level == jm::trunc(c.level) && c.level.abs() < 9.0e15 {
                    uo_jsnum::conv::to_int32(c.level).into()
                } else {
                    i64::MIN
                };
                let list = idx.get(&key)?;
                let page = f64::from(u32::try_from(pi).unwrap_or(u32::MAX));
                let mut text = &list.first()?.text;
                for h in list {
                    if h.page > page {
                        break;
                    }
                    text = &h.text;
                }
                Some((text.clone(), c.sep.clone()))
            });
            match chap {
                Some((ch, sep)) => {
                    let mut v = ch.to_vec();
                    v.extend(sep.encode_utf16());
                    v.extend_from_slice(num.units());
                    out.insert(name.clone(), v.into_boxed_slice());
                }
                None => {
                    out.insert(name.clone(), num.units().into());
                }
            }
        }
        (out, nums)
    }
}
