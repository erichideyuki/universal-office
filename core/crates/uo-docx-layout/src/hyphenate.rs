//! Automatic hyphenation (`hyphenate.ts`, frontier F4): Word's choice over
//! `uo_fonts::Hyphenator`. Which languages hyphenate is an **input** (`hyphenation_loaded`, K5,
//! LQ16), not what is compiled in. The `documentHyphenationLanguages` walk goes to
//! `uo_docx::word` (F2, explicit `lang`-site list).

use indexmap::IndexMap;

use crate::options::PatternId;

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

/// The hyphenation sessions of one engine: one per loaded pattern set, each with a word cache
/// capped at 100,000 words (M5). Filled by F4 over the uo-fonts hyphenation registry.
#[derive(Debug, Default)]
pub struct HyphenSessions {
    /// the pattern sets this call may use (`LayoutOptions::hyphenation_loaded`)
    pub loaded: Vec<PatternId>,
    /// words already split, per pattern set: offsets after which a hyphen may go
    pub cache: IndexMap<PatternId, WordCache>,
}

/// Split words of one pattern set: word → offsets after which a hyphen may go.
pub type WordCache = IndexMap<Box<[u16]>, Box<[u32]>>;

/// The word cache cap per language (M5).
pub const WORD_CACHE_CAP: usize = 100_000;

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
