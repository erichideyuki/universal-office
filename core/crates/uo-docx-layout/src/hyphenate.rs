//! Automatic hyphenation (`hyphenate.ts`, frontiers F4 and F5): the marks of `markHyphenation`
//! (F4) and Word's choice at the end of a line (F5) over `uo_fonts::Hyphenator`. Which languages
//! hyphenate is an **input** (`hyphenation_loaded`, K5, LQ16), not what is compiled in. The
//! `documentHyphenationLanguages` walk goes to `uo_docx::word` (F2, explicit `lang`-site list).

use std::sync::Arc;

use indexmap::IndexMap;
use uo_fonts::hyphen::{HyphenSession, HyphenationRegistry, hyphenation_pattern_id};

use crate::error::LResult;
use crate::options::{Hyphenation, PatternId};
use crate::para::{Glyph, GlyphFlags, GlyphKind, RunStyle};

/// `HyphenUnit` (hyphenate.ts:137): the word that did not fit.
#[derive(Clone, Copy, Debug)]
pub struct HyphenUnit {
    pub i: u32,
    pub e: u32,
    pub x: f64,
    pub right: f64,
    pub line_start: bool,
    pub content_end: f64,
    pub justify: bool,
    pub spaces_w: f64,
    pub rest_w: f64,
    pub n_spaces: f64,
    pub hyphen_run: f64,
}

/// `HyphenChoice` (hyphenate.ts:158).
#[derive(Clone, Copy, Debug)]
pub struct HyphenChoice {
    /// last glyph of the line
    pub k: u32,
    /// hyphen width
    pub w: f64,
}

/// The hyphenators of one engine (`hyphenatorFor`, typography/hyphen/index.ts:96): the pattern
/// sets the host registered, of which a call uses the ones its options say the TS process had
/// loaded (`hyphenation_loaded`, K5); one `HyphenSession` per pattern set holds the word cache
/// (the uo-fonts rule: cleared past 50,000 words, the TS one; M5).
#[derive(Debug, Default)]
pub struct HyphenSessions {
    /// the registered pattern sets (the host's or `uo_fonts::embedded_patterns`)
    pub registry: Option<Arc<HyphenationRegistry>>,
    /// the sessions opened so far, per pattern id
    pub sessions: IndexMap<PatternId, HyphenSession>,
}

impl HyphenSessions {
    #[must_use]
    pub fn new(registry: Arc<HyphenationRegistry>) -> HyphenSessions {
        HyphenSessions { registry: Some(registry), sessions: IndexMap::new() }
    }

    /// `hyphenatorFor(lang)`: the session of the language's pattern set when the call may use it
    /// (`loaded`) and the registry holds it.
    pub fn session(&mut self, lang: &str, loaded: &indexmap::IndexSet<PatternId>) -> Option<&mut HyphenSession> {
        let id = hyphenation_pattern_id(Some(lang))?;
        if !loaded.contains(id) {
            return None;
        }
        if !self.sessions.contains_key(id) {
            let h = self.registry.as_ref()?.get(id)?;
            self.sessions.insert(id.into(), HyphenSession::new(h));
        }
        self.sessions.get_mut(id)
    }

    /// Drops the word caches (`clear_caches`).
    pub fn clear(&mut self) {
        self.sessions.clear();
    }
}

/// `hyphenMins(compat)` (hyphenate.ts:44): Word's minimum letters before/after the hyphen.
#[must_use]
pub fn hyphen_mins(compat: f64) -> (u32, u32) {
    (2, if compat >= 15.0 { 3 } else { 2 })
}

/// `LETTER.test(ch)` (`/^[\p{L}\p{M}]$/u`).
fn is_letter(ch: &[u16]) -> bool {
    uo_text::props::is_single_code_point_in(ch, uo_text::props::LETTER | uo_text::props::MARK)
}

/// `MARK.test(ch)` (`/^\p{M}$/u`).
fn is_mark(ch: &[u16]) -> bool {
    uo_text::props::is_single_code_point_in(ch, uo_text::props::MARK)
}

/// `langOf(ctx, g)` (hyphenate.ts:51).
fn lang_of<'s>(styles: &'s [RunStyle], g: &Glyph, default: &'s str) -> &'s str {
    styles.get(g.style.0 as usize).and_then(|s| s.rpr.lang.as_deref()).and_then(|l| l.val.as_deref()).unwrap_or(default)
}

/// `langAllowed(ctx, lang)` (hyphenate.ts:55).
fn lang_allowed(list: Option<&[Box<str>]>, lang: &str) -> bool {
    let Some(list) = list else { return true };
    let l = uo_text::case::to_lowercase_str(lang);
    list.iter().any(|x| {
        let y = uo_text::case::to_lowercase_str(x);
        l == y || l.strip_prefix(y.as_str()).is_some_and(|r| r.starts_with('-'))
    })
}

/// `markHyphenation(ctx, ppr, glyphs, skipRuns, para)` (hyphenate.ts:86): marks the automatic
/// hyphenation points (`hyph` = may break with a hyphen AFTER this glyph) in the paragraph's runs
/// of letters. `skip_runs` are the equation runs; `para` the model paragraph (a copy has none, so
/// a manual-hyphenation skip never matches it, LA2).
pub fn mark_hyphenation(
    cx: &mut crate::engine::Cx<'_, '_>,
    fmt: &crate::model::DocFormatting,
    ppr: &crate::model::PPr,
    styles: &[RunStyle],
    glyphs: &mut [Glyph],
    skip_runs: &[u32],
    para: Option<crate::model::ParaId>,
) -> LResult<()> {
    let s = &fmt.settings;
    let opts = cx.opts;
    if (!s.auto_hyphenation && opts.hyphenation != Hyphenation::Force)
        || ppr.suppress_auto_hyphens == Some(true)
        || opts.hyphenation == Hyphenation::Off
    {
        return Ok(());
    }
    let (left, right) = hyphen_mins(cx.ldoc.compat_mode()); // wave 7c (W, item 2)
    let default_lang: &str = &opts.lang;
    let ch_of = |g: &Glyph| -> Vec<u16> { g.ch.as_units().to_vec() };
    let mut k = 0;
    while k < glyphs.len() {
        let Some(g) = glyphs.get(k) else { break };
        let ch = ch_of(g);
        if g.kind != GlyphKind::Char || !is_letter(&ch) || is_mark(&ch) || skip_runs.contains(&g.run_idx) {
            k += 1;
            continue;
        }
        let lang = lang_of(styles, g, default_lang).to_owned();
        // the run: letters of the same language; the optional hyphen stays inside
        let mut idx: Vec<usize> = Vec::new();
        let mut e = k;
        while let Some(q) = glyphs.get(e) {
            if q.kind == GlyphKind::Shy {
                e += 1;
                continue;
            }
            if q.kind != GlyphKind::Char
                || !is_letter(&ch_of(q))
                || lang_of(styles, q, default_lang) != lang
                || skip_runs.contains(&q.run_idx)
            {
                break;
            }
            idx.push(e);
            e += 1;
        }
        cx.budget.charge((e - k) as u64 + 1)?;
        let first = k;
        k = e;
        let Some(g) = glyphs.get(first) else { break };
        let Some(rpr) = styles.get(g.style.0 as usize).map(|st| &st.rpr) else { continue };
        if rpr.no_proof == Some(true)
            || rpr.cs == Some(true)
            || rpr.rtl == Some(true)
            || !lang_allowed(opts.hyphenation_languages.as_deref(), &lang)
        {
            continue;
        }
        // refused in Manual Hyphenation (the editor looks for the words with 'force')
        if let Some(pid) = para
            && opts.hyphenation_skips.0.contains(&(pid, g.offset))
        {
            continue;
        }
        if s.do_not_hyphenate_caps
            && idx.iter().all(|&j| {
                let c = glyphs.get(j).map(ch_of).unwrap_or_default();
                let up = uo_text::case::to_uppercase(&c);
                *uo_text::case::to_lowercase(&c) == *up || c == *up
            })
        {
            continue;
        }
        let Some(h) = cx.hyph.session(&lang, &opts.hyphenation_loaded) else { continue };
        // letter + combining marks -> one NFC code point (the patterns are NFC)
        let mut clusters: Vec<Vec<usize>> = Vec::new();
        for &j in &idx {
            let m = glyphs.get(j).is_some_and(|q| is_mark(q.ch.as_units()));
            match clusters.last_mut() {
                Some(last) if m => last.push(j),
                _ => clusters.push(vec![j]),
            }
        }
        if clusters.len() < (left + right) as usize {
            continue;
        }
        let mut word: Vec<u16> = Vec::new();
        for c in &clusters {
            let t: Vec<u16> = c.iter().filter_map(|&j| glyphs.get(j)).flat_map(|q| q.ch.as_units().to_vec()).collect();
            let n = uo_text::nfc::nfc(&t);
            if String::from_utf16(&n).is_ok_and(|s| s.chars().count() == 1) {
                word.extend_from_slice(&n);
            } else if let Some(q) = c.first().and_then(|&j| glyphs.get(j)) {
                word.extend_from_slice(q.ch.as_units());
            }
        }
        // every unit is a letter or mark (valid code points): the UTF-8 conversion is exact
        let Ok(word) = String::from_utf16(&word) else { continue };
        for p in h.hyphenate(&word, left, right) {
            let Some(c) = (p as usize).checked_sub(1).and_then(|i| clusters.get(i)) else { continue };
            if let Some(&j) = c.last()
                && let Some(q) = glyphs.get_mut(j)
            {
                q.flags.set(GlyphFlags::HYPH, true);
            }
        }
    }
    Ok(())
}

// ── the choice at the end of a line (hyphenate.ts 133–250), ported with `lines.rs` by F5 because
//    `LineBreaker.next` calls it (record "Stage 3 — F5", F5-3); `markHyphenation` stays F4's ──

/// `HYPHEN_GAP15` (hyphenate.ts:38): mode 15, unjustified: a word that would leave a gap of at
/// least this much is hyphenated.
pub const HYPHEN_GAP15: f64 = 421.0;
/// `HYPHEN_EXPAND15` (hyphenate.ts:41): mode 15, justified: the expansion ratio above which the
/// word is hyphenated.
pub const HYPHEN_EXPAND15: f64 = 0.4868;
/// `HYPHEN_ZONE_DEFAULT` (hyphenate.ts:194): the zone of modes ≤ 14 without `w:hyphenationZone`.
pub const HYPHEN_ZONE_DEFAULT: f64 = 425.0;

/// `LETTER.test(g.ch) && !MARK.test(g.ch)` (hyphenate.ts:48–49, `/^[\p{L}\p{M}]$/u`, `/^\p{M}$/u`).
fn letter_not_mark(g: &crate::para::Glyph) -> bool {
    let ch = g.ch.units.get(..usize::from(g.ch.len)).unwrap_or(&[]);
    uo_text::props::is_single_code_point_in(ch, uo_text::props::LETTER | uo_text::props::MARK)
        && !uo_text::props::is_single_code_point_in(ch, uo_text::props::MARK)
}

/// `hyphenCandidates(gs, i, e)` (hyphenate.ts:182): automatic points (with ≥ 2 letters since the
/// start of the unit) and optional hyphens (no minimum).
#[must_use]
pub fn hyphen_candidates(gs: &[crate::para::Glyph], i: u32, e: u32) -> Vec<u32> {
    use crate::para::{GlyphFlags, GlyphKind};
    let mut out = Vec::new();
    let mut letters = 0u32;
    let mut k = i;
    while k.saturating_add(1) < e {
        let Some(g) = gs.get(k as usize) else { break };
        if g.kind == GlyphKind::Shy {
            if k > i {
                out.push(k);
            }
            k += 1;
            continue;
        }
        if g.kind == GlyphKind::Char && letter_not_mark(g) {
            letters += 1;
        }
        // `gs[k + 1]?.kind !== 'shy'`
        if g.flags.has(GlyphFlags::HYPH) && gs.get(k as usize + 1).is_none_or(|n| n.kind != GlyphKind::Shy) && letters >= 2 {
            out.push(k);
        }
        k += 1;
    }
    out
}

/// `chooseHyphenation(ctx, gs, u)` (hyphenate.ts:206): where to hyphenate the unit that did not
/// fit, or `None` (it goes down whole / breaks by character).
pub fn choose_hyphenation<E: crate::lines::LineEnv + ?Sized>(
    env: &mut E,
    gs: &[crate::para::Glyph],
    u: &HyphenUnit,
) -> crate::error::LResult<Option<HyphenChoice>> {
    use crate::para::GlyphKind;
    use uo_jsnum::math as jm;
    let s = env.settings();
    let mut cands = hyphen_candidates(gs, u.i, u.e);
    let limit = s.consecutive_hyphen_limit;
    if limit > 0.0 && u.hyphen_run >= limit {
        cands.retain(|&k| gs.get(k as usize).is_some_and(|g| g.kind == GlyphKind::Shy));
    }
    if cands.is_empty() {
        return Ok(None);
    }
    let modern = s.compatibility_mode >= 15.0;
    let justify = u.justify && modern;
    let kind = |k: u32| gs.get(k as usize).map(|g| g.kind);
    // `hyphenW(k)`: advanceTwips(ctx, gs[k].face, 0x2d, gs[k].sizePt)
    let hyphen_w = |env: &mut E, k: u32| -> crate::error::LResult<f64> {
        let g = gs.get(k as usize).ok_or(crate::error::LayoutError::Internal("hyphenate::choose_hyphenation gs[k]"))?;
        env.advance_twips(g.face, 0x2d, g.size_pt)
    };
    // the end of each piece (up to glyph k, without the hyphen)
    let mut ends: Vec<f64> = Vec::with_capacity(cands.len());
    let mut x = u.x;
    let mut ci = 0usize;
    let mut k = u.i;
    while k < u.e && ci < cands.len() {
        let Some(g) = gs.get(k as usize) else { break };
        if g.kind != GlyphKind::Shy {
            x += g.adv;
        }
        if cands.get(ci) == Some(&k) {
            ends.push(x);
            ci += 1;
        }
        k += 1;
    }
    // `ends[n]!` (always pushed: every candidate lies in [i, e))
    let end_of = |n: usize| ends.get(n).copied().unwrap_or(f64::NAN);
    if !modern {
        // modes ≤ 14: the largest optional hyphen that fits first
        for n in (0..cands.len()).rev() {
            let k = cands.get(n).copied().unwrap_or(0);
            if kind(k) == Some(GlyphKind::Shy) && end_of(n) + hyphen_w(env, k)? - u.right <= 1e-3 {
                return Ok(Some(HyphenChoice { k, w: hyphen_w(env, k)? }));
            }
        }
        let zone = s.hyphenation_zone.unwrap_or(HYPHEN_ZONE_DEFAULT);
        if !u.line_start && u.right - u.x < zone - 1e-3 {
            return Ok(None);
        }
        // automatic points: the piece with the hyphen AND the letter after the point must fit
        for n in (0..cands.len()).rev() {
            let k = cands.get(n).copied().unwrap_or(0);
            if kind(k) == Some(GlyphKind::Shy) {
                continue;
            }
            let mut nx = k + 1;
            while nx < u.e && gs.get(nx as usize).is_some_and(|q| q.kind == GlyphKind::Shy || (q.kind == GlyphKind::Obj && q.adv == 0.0)) {
                nx += 1;
            }
            let next_adv = if nx < u.e { gs.get(nx as usize).map_or(f64::NAN, |q| q.adv) } else { 0.0 };
            let end = jm::max(end_of(n) + hyphen_w(env, k)?, end_of(n) + next_adv);
            if end - u.right <= 1e-3 {
                return Ok(Some(HyphenChoice { k, w: hyphen_w(env, k)? }));
            }
        }
        return Ok(None);
    }
    if !u.line_start {
        if justify {
            let slack = u.right - u.content_end;
            if u.rest_w > 0.0 && slack / u.rest_w <= HYPHEN_EXPAND15 {
                return Ok(None);
            }
        } else if u.right - u.content_end < HYPHEN_GAP15 {
            return Ok(None);
        }
    }
    let mut fits: Vec<f64> = Vec::with_capacity(cands.len());
    for (n, &k) in cands.iter().enumerate() {
        fits.push(end_of(n) + hyphen_w(env, k)? - u.right);
    }
    let shrink = crate::lines::JUSTIFY_SHRINK;
    for n in (0..cands.len()).rev() {
        let k = cands.get(n).copied().unwrap_or(0);
        let ov = fits.get(n).copied().unwrap_or(f64::NAN);
        if ov <= 1e-3 {
            return Ok(Some(HyphenChoice { k, w: hyphen_w(env, k)? }));
        }
        if !justify || u.n_spaces == 0.0 {
            continue;
        }
        // justified (mode 15): compress up to 25% of the spaces, losing at most half of the slack
        // the previous point would leave (oracle hyphfit); the last point compares with the word
        // going down (the jsweep rule)
        if ov > shrink.ratio * u.spaces_w + 0.0175 {
            continue;
        }
        if n > 0 {
            let slack_prev = -fits.get(n - 1).copied().unwrap_or(f64::NAN);
            if ov <= shrink.expand_ratio * slack_prev + 0.02 {
                return Ok(Some(HyphenChoice { k, w: hyphen_w(env, k)? }));
            }
        } else {
            let slack = u.right - u.content_end;
            if u.line_start
                || u.rest_w <= 0.0
                || ov / u.spaces_w <= (shrink.expand_ratio * slack) / u.rest_w + 0.0075 * u.n_spaces / u.spaces_w
            {
                return Ok(Some(HyphenChoice { k, w: hyphen_w(env, k)? }));
            }
        }
    }
    Ok(None)
}
