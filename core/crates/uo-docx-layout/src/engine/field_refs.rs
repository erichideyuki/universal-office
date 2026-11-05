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

/// `n?.numId ? st.next(n.numId, n.ilvl ?? 0) : null` of `listNumberOf`.
fn list_next(st: &mut word::NumberingState, fmt: &crate::model::DocFormatting, r: &word::ResolvedPPr) -> Option<NumberMarker> {
    let n = r.ppr.num_pr.as_deref()?;
    let id = n.num_id.filter(|v| v.truthy())?;
    let ilvl = n.ilvl.filter(|v| !v.is_null()).unwrap_or(crate::model::JsNumber::Num(0.0));
    st.next(fmt, id, ilvl)
}

/// A block index as a JS number.
fn num(i: usize) -> f64 {
    f64::from(u32::try_from(i).unwrap_or(u32::MAX))
}

/// The text length of a run as a JS number.
fn run_len(cx: &Cx<'_, '_>, k: RunKey) -> f64 {
    num(cx.ldoc.run_text(k).len())
}

impl Engine {
    /// `seqValue(instr)` (engine.ts:426): `SEQ id [\* format] [\r n] [\c] [\h] [\s n]` — counters in
    /// document order; `\s n` resets the counter after a heading of level ≤ n since it last counted
    /// (oracle refs-lab, sr-*).
    pub(crate) fn seq_value(&mut self, instr: &[u16], lang: &str, cap: &mut word::TextCap) -> Option<Box<[u16]>> {
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
        if has_switch(instr, b"h") {
            return Some(Box::default());
        }
        let fmt = star_format(instr).map_or("decimal", seq_format);
        Some(word::format_number(v, Some(fmt), lang, cap).units().into())
    }

    /// `positionWord(bm, block, at)` (engine.ts:450): "above"/"below" (REF/NOTEREF `\p`): the field
    /// comes after or before the bookmark in document order; in the same paragraph the offset
    /// decides (oracle refs, ref-pos). Without the field's position, the cache stays.
    fn position_word(cx: &Cx<'_, '_>, bm: &str, block: Option<i64>, at: Option<f64>) -> Option<Box<[u16]>> {
        let b = cx.ldoc.view.bookmarks().starts.get(bm)?;
        let block = block.filter(|b| *b >= 0)?;
        let bk = b.block.to_number();
        let off = b.offset.as_f64().unwrap_or(0.0);
        let blk = f64::from(u32::try_from(block).unwrap_or(u32::MAX));
        let below = blk < bk || (js_eq(blk, bk) && at.is_some_and(|a| a < off));
        let w = match (below, is_pt(&cx.opts.lang)) {
            (true, true) => "abaixo",
            (true, false) => "below",
            (false, true) => "acima",
            (false, false) => "above",
        };
        Some(utf16(w))
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
    pub(crate) fn note_ref_value(&mut self, cx: &Cx<'_, '_>, instr: &[u16], block: Option<i64>, at: Option<f64>) -> Option<Box<[u16]>> {
        let bm = String::from_utf16_lossy(tok(instr, 1).filter(|t| !t.is_empty())?);
        if has_switch(instr, b"p") {
            return Self::position_word(cx, &bm, block, at);
        }
        let bmk = cx.ldoc.view.bookmarks();
        let st = bmk.starts.get(bm.as_str())?;
        let b = st.block.to_number();
        let k = self.bookmark_para(cx, b)?;
        let from = st.offset.as_f64().unwrap_or(0.0);
        let to = match bmk.ends.get(bm.as_str()) {
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
        if has_switch(instr, b"p") {
            return Ok(Self::position_word(cx, &bm, block, at));
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
            let fmt = cx.ldoc.view.formatting();
            let mut st = word::numbering_state(&cx.opts.lang, &cx.opts.limits);
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

    /// `bookmarkPages()` (engine.ts:638): the displayed (formatted) page of each bookmark's
    /// paragraph — inside the paragraph the line holding the bookmark's offset (after a page break
    /// in the middle of the paragraph, the next page — oracle tocref-pbstart); a bookmark in a
    /// block without lines takes the next laid-out line (≤ 50 blocks on).
    #[must_use]
    pub fn bookmark_pages(&self, ldoc: &crate::ldoc::LDoc<'_>, opts: &crate::options::LayoutOptions) -> IndexMap<Box<str>, Box<[u16]>> {
        let mut out: IndexMap<Box<str>, Box<[u16]>> = IndexMap::new();
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
            let Some(ps) = pg.and_then(|pi| self.pages.get(pi)) else { continue };
            let fmt = self.geoms.get(ps.geom as usize).and_then(|g| g.pg_fmt.clone()).unwrap_or_else(|| "decimal".into());
            out.insert(name.clone(), word::format_number(ps.page.number, Some(&fmt), &opts.lang, &mut cap).units().into());
        }
        out
    }
}
